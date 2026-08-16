use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, IF_MODIFIED_SINCE, IF_NONE_MATCH};
use reqwest::{Client, ClientBuilder, Method, StatusCode};
use tracing::{debug, warn};

use crate::error::{Error, Result};
use crate::fetcher::{FetchSpec, Fetcher};
use crate::models::FetchedDocument;

pub struct HttpFetcher {
    client: Client,
    max_retries: u32,
}

impl HttpFetcher {
    pub fn new() -> Result<Self> {
        let client = ClientBuilder::new()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(32)
            .user_agent(concat!("ReadingSteiner/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            client,
            max_retries: 3,
        })
    }

    async fn fetch_once(&self, spec: &FetchSpec) -> Result<FetchedDocument> {
        let started = Instant::now();
        let method = Method::from_bytes(spec.fetch.method.as_bytes()).unwrap_or(Method::GET);
        let mut req = self
            .client
            .request(method, &spec.fetch.url)
            .timeout(Duration::from_secs(spec.fetch.timeout_secs.max(1)));

        let mut headers = HeaderMap::new();
        for (k, v) in &spec.fetch.headers {
            if let (Ok(name), Ok(v)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(name, v);
            }
        }
        if let Some(etag) = &spec.etag
            && let Ok(v) = HeaderValue::from_str(etag)
        {
            headers.insert(IF_NONE_MATCH, v);
        }
        if let Some(lm) = &spec.last_modified
            && let Ok(v) = HeaderValue::from_str(lm)
        {
            headers.insert(IF_MODIFIED_SINCE, v);
        }
        req = req.headers(headers);

        let resp = req.send().await?;
        let status = resp.status();
        let final_url = resp.url().to_string();
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let last_modified = resp
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);

        if status == StatusCode::NOT_MODIFIED {
            return Ok(FetchedDocument {
                final_url,
                status: 304,
                text: String::new(),
                html: None,
                images: Vec::new(),
                screenshot: None,
                etag,
                last_modified,
                content_sha256: String::new(),
                normalized_fingerprint: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
                engine: "http".to_string(),
                not_modified: true,
            });
        }

        if !status.is_success() {
            return Err(Error::Other(format!(
                "HTTP {} for {}",
                status.as_u16(),
                spec.fetch.url
            )));
        }

        let limit = spec.fetch.max_body_bytes.max(1024);
        let bytes = read_body_limited(resp, limit).await?;
        let content_sha256 = blake3::hash(&bytes).to_hex().to_string();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let normalized_fingerprint = blake3::hash(text.as_bytes()).to_hex().to_string();

        debug!(
            source = %spec.source_id,
            bytes = bytes.len(),
            sha = %content_sha256,
            "http fetch completed"
        );

        Ok(FetchedDocument {
            final_url,
            status: status.as_u16(),
            text,
            html: None,
            images: Vec::new(),
            screenshot: None,
            etag,
            last_modified,
            content_sha256,
            normalized_fingerprint,
            duration_ms: started.elapsed().as_millis() as u64,
            engine: "http".to_string(),
            not_modified: false,
        })
    }
}

async fn read_body_limited(resp: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut stream = resp.bytes_stream();
    let mut out = Vec::with_capacity(limit.min(64 * 1024));
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > limit {
            let remaining = limit - out.len();
            out.extend_from_slice(&chunk[..remaining]);
            warn!("body exceeded configured limit, truncated at {limit} bytes");
            break;
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[async_trait]
impl Fetcher for HttpFetcher {
    async fn fetch(&self, spec: &FetchSpec) -> Result<FetchedDocument> {
        let mut attempt = 0;
        loop {
            match self.fetch_once(spec).await {
                Ok(doc) => return Ok(doc),
                Err(e) => {
                    let retryable = matches!(e, Error::Http(_) | Error::Other(_));
                    attempt += 1;
                    if !retryable || attempt > self.max_retries {
                        return Err(e);
                    }
                    let backoff = Duration::from_millis(100 * 2_u64.pow(attempt - 1));
                    tokio::time::sleep(backoff).await;
                    warn!(source = %spec.source_id, attempt, error = %e, "retrying fetch");
                }
            }
        }
    }
}
