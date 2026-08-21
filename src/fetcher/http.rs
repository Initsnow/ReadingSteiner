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
    pub fn new(user_agent: &str, default_timeout_secs: u64) -> Result<Self> {
        let client = ClientBuilder::new()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(default_timeout_secs.max(1)))
            .pool_max_idle_per_host(32)
            .user_agent(user_agent)
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
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
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
                content_type,
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
        // 按响应 Content-Type 声明的 charset 解码，避免 GBK 等非 UTF-8 页面乱码
        // 导致内容被误判为“变化”或漏判。未声明时默认按 UTF-8 处理（from_utf8_lossy）。
        let charset = charset_from_content_type(content_type.as_deref());
        let text = decode_body(&bytes, charset.as_deref());
        let normalized_fingerprint = blake3::hash(text.as_bytes()).to_hex().to_string();

        debug!(
            source = %spec.source_id,
            bytes = bytes.len(),
            sha = %content_sha256,
            charset,
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
            content_type,
            not_modified: false,
        })
    }
}

/// 从 Content-Type 头解析字符集名（小写）。未指定时返回 None。
fn charset_from_content_type(content_type: Option<&str>) -> Option<String> {
    let ct = content_type?;
    for part in ct.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("charset=") {
            return Some(rest.trim().trim_matches('\"').to_lowercase());
        }
    }
    None
}

/// 按字符集解码响应体。
///
/// - 未声明 charset 或声明为 UTF-8 时，按 UTF-8 解码（无效字节用替换符）。
/// - 声明为常见单字节 / 简体中文编码时，用 encoding_rs 解码，避免乱码。
/// - 声明为未知编码时退回 UTF-8，避免 panic。
fn decode_body(bytes: &[u8], charset: Option<&str>) -> String {
    let charset = charset.unwrap_or("utf-8");
    match charset {
        "utf-8" | "utf8" | "" => String::from_utf8_lossy(bytes).into_owned(),
        _ => {
            let encoding =
                encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
            // decode_without_bom_handling：按标签解码，遇无效序列用替换符，不抛错。
            let (text, _) = encoding.decode_without_bom_handling(bytes);
            text.into_owned()
        }
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
