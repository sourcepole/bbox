use crate::Cli;
use bbox_common::file_search;
use crossbeam::channel;
use rusoto_s3::{PutObjectRequest, S3Client, S3};
use std::env;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task;

fn s3cfg(args: &Cli) -> anyhow::Result<(String, rusoto_core::Region)> {
    let bucket = match args.s3_path.strip_prefix("s3://") {
        None => anyhow::bail!("S3 path has to start with 's3://'"),
        Some(bucket) => {
            if bucket.contains('/') {
                anyhow::bail!("S3 path should be 's3://bucket'")
            } else {
                bucket.to_string()
            }
        }
    };

    let region = match env::var("S3_ENDPOINT_URL") {
        Ok(endpoint) => rusoto_core::Region::Custom {
            name: "region".to_string(),
            endpoint,
        },
        Err(_) => rusoto_core::Region::default(),
    };

    Ok((bucket, region))
}

pub async fn put_files_seq(args: &Cli) -> anyhow::Result<()> {
    let (bucket, region) = s3cfg(args)?;
    let client = S3Client::new(region);

    let prefix = PathBuf::from(format!("{}/", args.srcdir.to_string_lossy()));
    let files = file_search::search(&args.srcdir, "*");
    for path in files {
        let key = path.strip_prefix(&prefix)?.to_string_lossy().to_string();
        let mut input: Box<dyn std::io::Read + Send + Sync> =
            Box::new(match std::fs::File::open(&path) {
                Err(e) => anyhow::bail!("Opening input file {:?} failed: {e}", &path),
                Ok(x) => x,
            });
        let mut data = Vec::with_capacity(4096);
        let content_length = match input.read_to_end(&mut data) {
            Ok(len) => len as i64,
            Err(e) => anyhow::bail!("Reading file {:?} failed: {e}", &path),
        };
        println!("cp {key} ({content_length} bytes)");

        if let Err(e) = {
            let request = PutObjectRequest {
                bucket: bucket.clone(),
                key,
                body: Some(data.into()),
                content_length: Some(content_length),
                ..Default::default()
            };
            client.put_object(request).await
        } {
            anyhow::bail!("Upload failed {e}");
        }
    }
    Ok(())
}

pub async fn put_files_tasks(args: &Cli) -> anyhow::Result<()> {
    let (bucket, region) = s3cfg(args)?;

    // Keep a queue of tasks waiting for parallel async execution (size >= #cores).
    let task_queue_size = args.tasks.unwrap_or(256);
    let mut tasks = Vec::with_capacity(task_queue_size);

    // let await_one_task = |tasks| async {
    //     match futures_util::future::select_all(tasks).await {
    //         // Ignoring all errors
    //         (_result, _index, remaining) => remaining,
    //     }
    // };

    let prefix = PathBuf::from(format!("{}/", args.srcdir.to_string_lossy()));
    let files = file_search::search(&args.srcdir, "*");
    for path in files {
        let bucket = bucket.clone();
        let prefix = prefix.clone();
        let client = S3Client::new(region.clone());
        tasks.push(task::spawn(async move {
            let key = path.strip_prefix(&prefix)?.to_string_lossy().to_string();
            let mut input: Box<dyn std::io::Read + Send + Sync> =
                Box::new(match std::fs::File::open(&path) {
                    Err(e) => anyhow::bail!("Opening input file {:?} failed: {e}", &path),
                    Ok(x) => x,
                });
            let mut data = Vec::with_capacity(4096);
            let content_length = match input.read_to_end(&mut data) {
                Ok(len) => len as i64,
                Err(e) => anyhow::bail!("Reading file {:?} failed: {e}", &path),
            };
            println!("cp {key} ({content_length} bytes)");

            if let Err(e) = {
                let request = PutObjectRequest {
                    bucket,
                    key,
                    body: Some(data.into()),
                    content_length: Some(content_length),
                    ..Default::default()
                };
                client.put_object(request).await
            } {
                anyhow::bail!("Upload failed {e}");
            }
            Ok(())
        }));
        if tasks.len() >= task_queue_size {
            tasks = await_one_task(tasks).await;
        }
    }
    // Finish remaining tasks
    futures_util::future::join_all(tasks).await;
    Ok(())
}

async fn await_one_task<T>(tasks: Vec<task::JoinHandle<T>>) -> Vec<task::JoinHandle<T>> {
    match futures_util::future::select_all(tasks).await {
        // Ignoring all errors
        (_result, _index, remaining) => remaining,
    }
}

pub async fn put_files_channels(args: &Cli) -> anyhow::Result<()> {
    let (bucket, region) = s3cfg(args)?;

    let num_tokens = args.tasks.unwrap_or(256);
    // add initial tokens
    let (token_sender, token_receiver) = channel::bounded(num_tokens);
    for _ in 0..num_tokens {
        if token_sender.send(Ok(None)).is_err() {
            anyhow::bail!("Failed to initialize threads");
        }
    }

    let mut tile_results = Vec::new();
    let mut wait_for_tile = || {
        match token_receiver.recv() {
            Err(e) => anyhow::bail!("Failed communicate with threads: {e}"),
            Ok(Err(e)) => anyhow::bail!("Failed to upload part: {e}"),
            Ok(Ok(Some(x))) => tile_results.push(x),
            Ok(Ok(None)) => (),
        }
        Ok(())
    };
    let prefix = PathBuf::from(format!("{}/", args.srcdir.to_string_lossy()));
    let files = file_search::search(&args.srcdir, "*");
    for path in files {
        let key = path.strip_prefix(&prefix)?.to_string_lossy().to_string();

        wait_for_tile()?;

        let mut input: Box<dyn std::io::Read + Send + Sync> =
            Box::new(match std::fs::File::open(&path) {
                Err(e) => anyhow::bail!("Opening input file {:?} failed: {e}", &path),
                Ok(x) => x,
            });
        let mut data = Vec::with_capacity(4096);
        let content_length = match input.read_to_end(&mut data) {
            Ok(len) => len as i64,
            Err(e) => {
                anyhow::bail!("Reading file {:?} failed: {e}", &path);
            }
        };
        let region = region.clone();
        let bucket = bucket.clone();
        let max_retries = 3;
        let token_sender = token_sender.clone();
        tokio::spawn(async move {
            let client = S3Client::new(region);
            println!("cp {key} ({content_length} bytes)");
            let mut retry_count = 0;
            let result = loop {
                let request = PutObjectRequest {
                    bucket: bucket.clone(),
                    key: key.clone(),
                    body: Some(data.clone().into()),
                    content_length: Some(content_length),
                    ..Default::default()
                };
                match client.put_object(request).await {
                    Ok(_) => break Ok(Some(())),
                    Err(e) => {
                        retry_count += 1;
                        if retry_count > max_retries {
                            break Err(e);
                        }
                        eprintln!("Upload failed: {}, retrying", e);
                        tokio::time::sleep(Duration::from_secs(2_u64.pow(retry_count))).await;
                    }
                }
            };
            let _ = token_sender.send(result);
        });
    }

    // drain remaining results
    for _ in 0..num_tokens {
        wait_for_tile()?;
    }

    Ok(())
}
