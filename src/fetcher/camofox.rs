use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::config::CamofoxConfig;
use crate::error::{Error, Result};
use crate::fetcher::{FetchSpec, Fetcher};
use crate::models::{FetchedDocument, ImageRef};

#[derive(Debug, Clone, Serialize)]
struct CreateTabRequest {
    user_id: String,
    session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTabResponse {
    #[serde(rename = "tabId")]
    tab_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct NavigateRequest {
    user_id: String,
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct WaitRequest {
    user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SnapshotResponse {
    #[serde(default)]
    snapshot: String,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ImagesResponse {
    #[serde(default)]
    images: Vec<CamofoxImage>,
}

#[derive(Debug, Deserialize)]
struct CamofoxImage {
    #[serde(default)]
    src: String,
    #[serde(default)]
    alt: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct EvaluateRequest {
    user_id: String,
    expression: String,
}

#[derive(Debug, Deserialize)]
struct EvaluateResponse {
    #[serde(default)]
    result: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default, rename = "browserConnected")]
    browser_connected: bool,
}

pub struct CamofoxFetcher {
    client: Client,
    cfg: CamofoxConfig,
    tabs: Mutex<HashMap<String, String>>,
    broken: Mutex<bool>,
    auth: Option<String>,
}

impl CamofoxFetcher {
    pub fn new(cfg: &CamofoxConfig) -> Result<Self> {
        let client = ClientBuilder::new()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(90))
            .build()?;
        let auth = load_bearer(cfg).ok();
        Ok(Self {
            client,
            cfg: cfg.clone(),
            tabs: Mutex::new(HashMap::new()),
            broken: Mutex::new(false),
            auth,
        })
    }

    fn base(&self) -> String {
        self.cfg.base_url.trim_end_matches('/').to_string()
    }

    fn auth_header(&self) -> Option<String> {
        self.auth.clone()
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base(), path);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.bearer_auth(auth);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!(
                "camofox GET {path} -> {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T> {
        let url = format!("{}{}", self.base(), path);
        let mut req = self.client.post(&url).json(body);
        if let Some(auth) = self.auth_header() {
            req = req.bearer_auth(auth);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!(
                "camofox POST {path} -> {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn delete_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base(), path);
        let mut req = self.client.delete(&url);
        if let Some(auth) = self.auth_header() {
            req = req.bearer_auth(auth);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!(
                "camofox DELETE {path} -> {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn ensure_tab(&self, spec: &FetchSpec) -> Result<String> {
        let mut tabs = self.tabs.lock().await;
        let source_id = &spec.source_id;
        if let Some(tab) = tabs.get(source_id) {
            // Reuse is best-effort; if it 404s later we recreate.
            return Ok(tab.clone());
        }
        let req = CreateTabRequest {
            user_id: self.cfg.user_id.clone(),
            session_key: self.cfg.session_key.clone(),
            url: Some(spec.fetch.url.clone()),
        };
        let resp: CreateTabResponse = self.post_json("/tabs", &req).await?;
        debug!(source = %source_id, tab = %resp.tab_id, "created camofox tab");
        tabs.insert(source_id.clone(), resp.tab_id.clone());
        Ok(resp.tab_id)
    }

    async fn fetch_full_snapshot(&self, tab_id: &str) -> Result<String> {
        let mut offset = 0usize;
        let mut text = String::new();
        loop {
            let path = format!(
                "/tabs/{tab_id}/snapshot?userId={}&format=text&offset={offset}",
                urlencoding(&self.cfg.user_id)
            );
            let resp: SnapshotResponse = self.get_json(&path).await?;
            text.push_str(&resp.snapshot);
            if !resp.has_more || resp.next_offset.is_none() {
                break;
            }
            offset = resp.next_offset.unwrap_or(offset + text.len());
            if text.len() > 10 * 1024 * 1024 {
                warn!("camofox snapshot exceeded 10MB, truncating");
                break;
            }
        }
        Ok(text)
    }

    async fn fetch_images(&self, tab_id: &str) -> Result<Vec<ImageRef>> {
        let path = format!(
            "/tabs/{tab_id}/images?userId={}",
            urlencoding(&self.cfg.user_id)
        );
        let resp: ImagesResponse = self.get_json(&path).await?;
        Ok(resp
            .images
            .into_iter()
            .map(|i| ImageRef {
                canonical_url: i.src,
                alt: i.alt,
                width: i.width,
                height: i.height,
            })
            .collect())
    }

    async fn close_tab_if_needed(&self, spec: &FetchSpec, tab_id: &str) -> Result<()> {
        if spec.fetch.tab_policy == "per_check" {
            let path = format!("/tabs/{tab_id}?userId={}", urlencoding(&self.cfg.user_id));
            let _: serde_json::Value = self.delete_json(&path).await?;
            self.tabs.lock().await.remove(&spec.source_id);
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        let broken = *self.broken.lock().await;
        if broken {
            return Err(Error::Other("camofox engine circuit breaker open".into()));
        }
        let resp: HealthResponse = self.get_json("/health").await?;
        if !resp.ok || !resp.browser_connected {
            *self.broken.lock().await = true;
            return Err(Error::Other("camofox health check failed".into()));
        }
        *self.broken.lock().await = false;
        Ok(())
    }
}

fn urlencoding(s: &str) -> String {
    // Minimal percent-encoding for query strings.
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn load_bearer(cfg: &CamofoxConfig) -> Result<String> {
    let mut candidates = Vec::new();
    if !cfg.access_key_file.as_os_str().is_empty() && Path::new(&cfg.access_key_file).exists() {
        candidates.push(std::fs::read_to_string(&cfg.access_key_file)?);
    }
    if !cfg.api_key_file.as_os_str().is_empty() && Path::new(&cfg.api_key_file).exists() {
        candidates.push(std::fs::read_to_string(&cfg.api_key_file)?);
    }
    candidates
        .into_iter()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
        .ok_or_else(|| Error::config("camofox access_key_file/api_key_file not found"))
}

#[async_trait]
impl Fetcher for CamofoxFetcher {
    async fn fetch(&self, spec: &FetchSpec) -> Result<FetchedDocument> {
        self.health_check().await?;
        let started = Instant::now();
        let tab_id = self.ensure_tab(spec).await?;

        let nav = NavigateRequest {
            user_id: self.cfg.user_id.clone(),
            url: spec.fetch.url.clone(),
        };
        let _: serde_json::Value = self
            .post_json(&format!("/tabs/{tab_id}/navigate"), &nav)
            .await
            .map_err(|e| {
                // If reuse tab is stale, drop it and recreate once.
                warn!(source = %spec.source_id, error = %e, "navigate failed; recreating tab");
                e
            })?;

        let wait = &spec.fetch.wait;
        if wait.selector.is_some() || wait.timeout.is_some() {
            let req = WaitRequest {
                user_id: self.cfg.user_id.clone(),
                selector: wait.selector.clone(),
                timeout: wait.timeout,
            };
            let _: serde_json::Value = self
                .post_json(&format!("/tabs/{tab_id}/wait"), &req)
                .await?;
        }

        let mut text = self.fetch_full_snapshot(&tab_id).await?;
        let mut images = self.fetch_images(&tab_id).await?;

        if let Some(expr) = &spec.fetch.evaluate {
            let req = EvaluateRequest {
                user_id: self.cfg.user_id.clone(),
                expression: expr.clone(),
            };
            let resp: EvaluateResponse = self
                .post_json(&format!("/tabs/{tab_id}/evaluate"), &req)
                .await?;
            if let Some(result) = resp.result {
                if let Some(s) = result.as_str() {
                    text = s.to_string();
                } else {
                    text = result.to_string();
                }
            }
        }

        let mut screenshot = None;
        if spec.fetch.screenshot {
            let path = format!(
                "/tabs/{tab_id}/screenshot?userId={}",
                urlencoding(&self.cfg.user_id)
            );
            #[derive(Deserialize)]
            struct ScreenshotResp {
                #[serde(default)]
                screenshot: Option<ScreenshotData>,
            }
            #[derive(Deserialize)]
            struct ScreenshotData {
                #[serde(default)]
                data: String,
            }
            let resp: ScreenshotResp = self.get_json(&path).await?;
            if let Some(s) = resp.screenshot {
                use base64::Engine;
                screenshot = Some(base64::engine::general_purpose::STANDARD.decode(s.data)?);
            }
        }

        self.close_tab_if_needed(spec, &tab_id).await?;

        let content_sha256 = blake3::hash(text.as_bytes()).to_hex().to_string();
        let normalized_fingerprint = blake3::hash(text.as_bytes()).to_hex().to_string();
        // In case of stale tab recreation we currently don't retry navigate; a second fetch will recreate.
        images.sort_by(|a, b| a.canonical_url.cmp(&b.canonical_url));
        images.dedup_by(|a, b| a.canonical_url == b.canonical_url);

        Ok(FetchedDocument {
            final_url: spec.fetch.url.clone(),
            status: 200,
            text,
            html: None,
            images,
            screenshot,
            etag: None,
            last_modified: None,
            content_sha256,
            normalized_fingerprint,
            duration_ms: started.elapsed().as_millis() as u64,
            engine: "camofox".to_string(),
            not_modified: false,
        })
    }
}
