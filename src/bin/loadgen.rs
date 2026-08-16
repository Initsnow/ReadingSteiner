//! Simple load generator for ReadingSteiner HTTP fetch path.
//!
//! Starts a local HTTP server on 127.0.0.1:18080, then runs `requests`
//! fetches through the real HttpFetcher with configurable concurrency.
//! Prints p50/p95 latency and throughput.

use std::sync::Arc;
use std::time::Instant;

use reading_steiner::config::{Config, FetchConfig};
use reading_steiner::fetcher::FetchSpec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

const BODY: &[u8] = b"<html><body><h1>loadgen</h1></body></html>";

async fn serve() {
    let listener = TcpListener::bind("127.0.0.1:18080").await.unwrap();
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                BODY.len()
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.write_all(BODY).await;
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let requests: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let concurrency: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);

    tokio::spawn(serve());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let cfg = Config::default();
    let fetcher = Arc::new(reading_steiner::fetcher::create_fetcher("http", &cfg)?);
    let sem = Arc::new(Semaphore::new(concurrency));
    let started = Instant::now();
    let mut latencies = Vec::with_capacity(requests);
    let mut handles = Vec::new();

    for i in 0..requests {
        let fetcher = fetcher.clone();
        let sem = sem.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire_owned().await.unwrap();
            let t = Instant::now();
            let spec = FetchSpec {
                fetch: FetchConfig {
                    url: format!("http://127.0.0.1:18080/page?i={i}"),
                    ..FetchConfig::default()
                },
                etag: None,
                last_modified: None,
                source_id: format!("load-{i}"),
            };
            fetcher.fetch(&spec).await?;
            Ok::<_, anyhow::Error>(t.elapsed().as_secs_f64())
        }));
    }
    for h in handles {
        latencies.push(h.await??);
    }
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let total = started.elapsed().as_secs_f64();
    let p50 = latencies[((latencies.len() as f64 * 0.50) as usize).min(latencies.len() - 1)];
    let p95 = latencies[((latencies.len() as f64 * 0.95) as usize).min(latencies.len() - 1)];
    println!(
        "requests={requests} concurrency={concurrency} total={total:.3}s throughput={:.0} req/s p50={:.4}s p95={:.4}s",
        requests as f64 / total,
        p50,
        p95
    );
    Ok(())
}
