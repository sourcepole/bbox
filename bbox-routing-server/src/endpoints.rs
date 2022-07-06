use crate::engine::Router;
use actix_web::{web, HttpResponse};
#[cfg(feature = "ogcapi")]
use bbox_common::api::{OgcApiInventory, OpenApiDoc, OpenApiDocCollection};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The definition of the route to compute.
#[derive(Debug, Deserialize, Serialize)]
pub struct RouteDefinition {
    pub name: Option<String>,
    pub preference: Option<String>,
    pub waypoints: Waypoints,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Waypoints {
    pub coordinates: Vec<Vec<f64>>,
    #[serde(rename = "type")]
    pub value_type: Type,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Type {
    MultiPoint,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    Sync,
    Async,
}

#[derive(Debug, Deserialize)]
pub struct RouteParams {
    mode: Option<SyncMode>,
}

/// compute a route
async fn compute_route(
    router: web::Data<Router>,
    route_params: web::Query<RouteParams>,
    route_def: web::Json<RouteDefinition>,
) -> HttpResponse {
    if let Some(mode) = &route_params.mode {
        if *mode == SyncMode::Async {
            return HttpResponse::UnprocessableEntity().json("Async mode not supported");
        }
    }
    dbg!(&route_def);
    let coords = &route_def.waypoints.coordinates;
    let shortest_path =
        router.calc_path((coords[0][0], coords[0][1]), (coords[1][0], coords[1][1]));
    let route = match shortest_path {
        Some(p) => {
            //let weight = p.get_weight();
            router.path_to_geojson(vec![p])
        }
        None => {
            json!({
              "type": "FeatureCollection",
              "status": "failed",
              "features": []
            })
        }
    };
    /* Full response:
    let resp = json!({
      "type": "FeatureCollection",
      "name": "Route from A to B",
      "status": "successful",
      "links": [
        {
          "href": "https://example.com/routes/5e37f",
          "rel": "self",
          "type": "application/geo+json",
          "title": "this document"
        },
        {
          "href": "https://example.com/routes/5e37f/definition",
          "rel": "describedBy",
          "type": "application/json",
          "title": "route definition for this route"
        }
      ],
      "features": [
        {
          "type": "Feature",
          "id": 1,
          "geometry": {
            "type": "LineString",
            "coordinates": [
              [
                36.1234515,
                32.6453783
              ],
              [
                36.1247213,
                32.7106286
              ]
            ]
          },
          "properties": {
            "type": "route overview",
            "length_m": 1224.7,
            "duration_s": 75,
            "maxHeight_m": 4.5,
            "comment": "This is a place to add a comment about the processing of the route."
          }
        },
        {
          "type": "Feature",
          "id": 2,
          "geometry": {
            "type": "Point",
            "coordinates": [
              36.1234515,
              32.6453783
            ]
          },
          "properties": {
            "type": "start"
          }
        },
        {
          "type": "Feature",
          "id": 3,
          "geometry": {
            "type": "Point",
            "coordinates": [
              36.1214698,
              32.655952
            ]
          },
          "properties": {
            "type": "segment",
            "length_m": 123.2,
            "duration_s": 8,
            "instruction": "left",
            "roadName": "Main Street",
            "maxHeight_m": 4.5,
            "speedLimit": 35,
            "speedLimitUnit": "mph"
          }
        },
        {
          "type": "Feature",
          "id": 5,
          "geometry": {
            "type": "Point",
            "coordinates": [
              36.1247213,
              32.7106286
            ]
          },
          "properties": {
            "type": "end"
          }
        }
      ]
    });
    */
    HttpResponse::Ok().json(route)
}

#[cfg(feature = "ogcapi")]
pub fn init_service(api: &mut OgcApiInventory, openapi: &mut OpenApiDoc) {
    use bbox_common::ogcapi::ApiLink;

    api.landing_page_links.push(ApiLink {
        href: "/routes".to_string(),
        rel: Some("routes".to_string()),
        type_: Some("application/json".to_string()),
        title: Some("OGC API routes".to_string()),
        hreflang: None,
        length: None,
    });
    api.conformance_classes.extend(vec![
        // Core
        "http://www.opengis.net/spec/ogcapi-routes-1/1.0.0-draft.1/req/core".to_string(),
        // JSON
        "http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/json".to_string(),
        // OpenAPI Specification
        "http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/oas30".to_string(),
        /*
         * OGC Process Description - http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/ogc-process-description
         * HTML - http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/html
         * Job list - http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/job-list
         * Callback - http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/callback
         * Dismiss - http://www.opengis.net/spec/ogcapi-processes-1/1.0/conf/dismiss
         */
    ]);
    openapi.extend(include_str!("openapi.yaml"), "/");
}

pub fn register(cfg: &mut web::ServiceConfig, router: &Option<Router>) {
    if let Some(router) = router {
        cfg.app_data(web::Data::new(router.clone()));
    }
    cfg.service(web::resource("/routes").route(web::post().to(compute_route)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{body, dev::Service, http, test, App, Error};

    #[actix_web::test]
    async fn test_route() -> Result<(), Error> {
        let router = Router::from_gpkg("../data/railway-test.gpkg", "flows", "geom")
            .await
            .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(router))
                .service(web::resource("/routes").route(web::post().to(compute_route))),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/routes")
            .set_json(&RouteDefinition {
                name: Some("A to B".to_string()),
                preference: None,
                waypoints: Waypoints {
                    value_type: Type::MultiPoint,
                    coordinates: vec![vec![9.35213353, 47.0935012], vec![9.3422712, 47.1011887]],
                },
            })
            .to_request();
        let resp = app.call(req).await.unwrap();

        assert_eq!(resp.status(), http::StatusCode::OK);

        let response_body = body::to_bytes(resp.into_body()).await?;

        assert_eq!(response_body, "{\"features\":[{\"geometry\":{\"coordinates\":[[9.351943003846154,47.093613230769236],[9.348591366666666,47.096161],[9.343048573684209,47.100490268421055]],\"type\":\"LineString\"},\"type\":\"Feature\"}],\"type\":\"FeatureCollection\"}");

        Ok(())
    }
}
