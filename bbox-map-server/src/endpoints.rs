use crate::config::WmsServerCfg;
use crate::fcgi_process::*;
use crate::metrics::{wms_metrics, WmsMetrics};
use crate::wms_fcgi_backend::WmsBackend;
use actix_web::{guard, web, Error, HttpRequest, HttpResponse};
use log::{debug, error, info, warn};
use opentelemetry::{
    global,
    trace::{SpanBuilder, SpanKind, TraceContextExt, Tracer},
    Context, KeyValue,
};
use std::io::{BufRead, Cursor, Read};
use std::str::FromStr;
use std::time::{Duration, SystemTime};

async fn wms_fcgi(
    fcgi_dispatcher: web::Data<FcgiDispatcher>,
    suffix: web::Data<String>,
    project: web::Path<String>,
    metrics: web::Data<WmsMetrics>,
    body: String,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    // --- > tracing/metrics
    let tracer = global::tracer("request");
    let ctx = Context::current();
    // ---

    let mut response = HttpResponse::Ok();
    let fcgi_query = format!(
        "map={}.{}&{}{}",
        project,
        suffix.as_str(),
        req.query_string(),
        &body
    );

    let (fcgino, pool) = fcgi_dispatcher.select(&fcgi_query);
    let available_clients = pool.status().available;

    // ---
    metrics
        .wms_requests_counter
        .with_label_values(&[
            req.path(),
            fcgi_dispatcher.backend_name(),
            &fcgino.to_string(),
        ])
        .inc();
    ctx.span()
        .set_attribute(KeyValue::new("project", project.to_string()));
    ctx.span()
        .set_attribute(KeyValue::new("fcgino", fcgino.to_string()));
    // ---

    // --- >>
    let span = tracer.start("fcgi_wait");
    let ctx = Context::current_with_span(span);
    // ---

    let fcgi_client_start = SystemTime::now();
    let fcgi_client = pool.get().await;
    let fcgi_client_wait_elapsed = fcgi_client_start.elapsed();

    // ---
    ctx.span().set_attribute(KeyValue::new(
        "available_clients",
        available_clients.to_string(),
    ));
    drop(ctx);
    metrics.fcgi_client_pool_available[fcgino]
        .with_label_values(&[fcgi_dispatcher.backend_name()])
        .set(available_clients as i64);
    if let Ok(elapsed) = fcgi_client_wait_elapsed {
        let duration =
            (elapsed.as_secs() as f64) + f64::from(elapsed.subsec_nanos()) / 1_000_000_000_f64;
        metrics.fcgi_client_wait_seconds[fcgino]
            .with_label_values(&[fcgi_dispatcher.backend_name()])
            .observe(duration);
    }
    // --- <

    let mut fcgi_client = match fcgi_client {
        Ok(fcgi) => fcgi,
        Err(_) => {
            warn!("FCGI client timeout");
            return Ok(HttpResponse::InternalServerError().finish());
        }
    };

    // --- >>
    let span = tracer.start("wms_fcgi");
    let ctx = Context::current_with_span(span);
    // ---

    let conninfo = req.connection_info();
    let host_port: Vec<&str> = conninfo.host().split(':').collect();
    debug!(
        "Forwarding query to FCGI process {}: {}",
        fcgino, &fcgi_query
    );
    let mut params = fastcgi_client::Params::new()
        .set_request_method(req.method().as_str())
        .set_request_uri(req.path())
        .set_server_name(host_port.get(0).unwrap_or(&""))
        .set_query_string(&fcgi_query);
    if let Some(port) = host_port.get(1) {
        params = params.set_server_port(port);
    }
    if conninfo.scheme() == "https" {
        params.insert("HTTPS", "ON");
    }
    // UMN uses env variables (https://github.com/MapServer/MapServer/blob/172f5cf092/maputil.c#L2534):
    // http://$(SERVER_NAME):$(SERVER_PORT)$(SCRIPT_NAME)? plus $HTTPS
    let fcgi_start = SystemTime::now();
    let output = fcgi_client.do_request(&params, &mut std::io::empty());
    if let Err(ref e) = output {
        warn!("FCGI error: {}", e);
        // Remove probably broken FCGI client from pool
        fcgi_dispatcher.remove(fcgi_client);
        return Ok(HttpResponse::InternalServerError().finish());
    }
    let fcgiout = output.unwrap().get_stdout().unwrap();

    let mut cursor = Cursor::new(fcgiout);
    let mut line = String::new();
    while let Ok(_bytes) = cursor.read_line(&mut line) {
        // Truncate newline
        let len = line.trim_end_matches(&['\r', '\n'][..]).len();
        line.truncate(len);
        if len == 0 {
            break;
        }
        let parts: Vec<&str> = line.splitn(2, ": ").collect();
        if parts.len() != 2 {
            error!("Invalid FCGI-Header received: {}", line);
            break;
        }
        let (key, value) = (parts[0], parts[1]);
        match key {
            "Content-Type" => {
                response.insert_header((key, value));
            }
            "Content-Length" | "Server" => {} // ignore
            "X-us" => {
                let us: u64 = value.parse().expect("u64 value");
                let _span = tracer.build(SpanBuilder {
                    name: "fcgi".into(),
                    span_kind: Some(SpanKind::Internal),
                    start_time: Some(fcgi_start),
                    end_time: Some(fcgi_start + Duration::from_micros(us)),
                    ..Default::default()
                });
                // Return server timing to browser
                // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Server-Timing
                // https://developer.mozilla.org/en-US/docs/Tools/Network_Monitor/request_details#timings_tab
                response.append_header(("Server-Timing", format!("wms-backend;dur={}", us / 1000)));
            }
            // "X-trace" => {
            "X-metrics" => {
                // cache_count:2,cache_hit:13,cache_miss:2
                for entry in value.split(',') {
                    let keyval: Vec<&str> = entry.splitn(2, ":").collect();
                    match keyval[0] {
                        "cache_count" => metrics.fcgi_cache_count[fcgino]
                            .with_label_values(&[fcgi_dispatcher.backend_name()])
                            .set(i64::from_str(keyval[1]).expect("i64 value")),
                        "cache_hit" => metrics.fcgi_cache_hit[fcgino]
                            .with_label_values(&[fcgi_dispatcher.backend_name()])
                            .set(i64::from_str(keyval[1]).expect("i64 value")),
                        "cache_miss" => { /* ignore */ }
                        _ => debug!("Ignoring metric entry {}", entry),
                    }
                }
            }
            _ => debug!("Ignoring FCGI-Header: {}", &line),
        }
        line.truncate(0);
    }

    // ---
    drop(ctx);
    // --- <

    let mut body = Vec::new(); // TODO: return body without copy
    let _bytes = cursor.read_to_end(&mut body);
    Ok(response.body(body))
}

pub fn register(cfg: &mut web::ServiceConfig, wms_backend: &WmsBackend) {
    let config = WmsServerCfg::from_config();
    let metrics = wms_metrics(config.num_fcgi_processes());

    cfg.app_data(web::Data::new((*metrics).clone()));

    cfg.app_data(web::Data::new(wms_backend.inventory.clone()));

    for fcgi_client in &wms_backend.fcgi_clients {
        for suffix_info in &fcgi_client.suffixes {
            let route = suffix_info.url_base.clone();
            let suffix = suffix_info.suffix.clone();
            info!("Registering WMS endpoint {route} (suffix: {suffix})");
            cfg.service(
                web::resource(route + "/{project:.+}") // :[^{}]+
                    .app_data(fcgi_client.clone())
                    .app_data(web::Data::new(suffix))
                    .route(
                        web::route()
                            .guard(guard::Any(guard::Get()).or(guard::Post()))
                            .to(wms_fcgi),
                    ),
            );
        }
    }
}
