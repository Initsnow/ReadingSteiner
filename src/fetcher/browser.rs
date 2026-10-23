use std::time::{Duration, Instant};

use async_trait::async_trait;
use chromiumoxide::Browser;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::cdp::browser_protocol::target::CloseTargetParams;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tracing::{debug, warn};

use crate::config::BrowserConfig;
use crate::error::{Error, Result};
use crate::fetcher::{FetchSpec, Fetcher};
use crate::models::FetchedDocument;

pub struct BrowserFetcher {
    config: BrowserConfig,
    user_agent: String,
    http_client: Client,
}

impl BrowserFetcher {
    pub fn new(config: &BrowserConfig, user_agent: &str) -> Self {
        Self {
            config: config.clone(),
            user_agent: user_agent.to_string(),
            http_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// 解析 CDP 连接地址：如果提供的是 HTTP 地址（如 http://127.0.0.1:9222），
    /// 则通过 /json/version 端点获取对应的 webSocketDebuggerUrl。
    async fn resolve_ws_url(&self) -> Result<String> {
        let raw = self.config.cdp_url.trim();
        if raw.starts_with("ws://") || raw.starts_with("wss://") {
            return Ok(raw.to_string());
        }

        let endpoint = format!("{}/json/version", raw.trim_end_matches('/'));
        let resp = self.http_client.get(&endpoint).send().await.map_err(|e| {
            Error::other(format!(
                "failed to connect to CDP endpoint at {endpoint}: {e}"
            ))
        })?;

        let val: Value = resp
            .json()
            .await
            .map_err(|e| Error::other(format!("invalid CDP json response from {endpoint}: {e}")))?;

        if let Some(ws) = val.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
            Ok(ws.to_string())
        } else {
            Err(Error::other(format!(
                "CDP endpoint {endpoint} did not return webSocketDebuggerUrl"
            )))
        }
    }
}

#[async_trait]
impl Fetcher for BrowserFetcher {
    async fn fetch(&self, spec: &FetchSpec) -> Result<FetchedDocument> {
        let started = Instant::now();
        let ws_url = self.resolve_ws_url().await?;

        let timeout_secs = if spec.fetch.timeout_secs > 0 {
            spec.fetch.timeout_secs
        } else {
            self.config.timeout_secs.max(10)
        };

        let fetch_future = async {
            let (browser, mut handler) = Browser::connect(&ws_url).await.map_err(|e| {
                Error::other(format!("failed to connect to Chrome CDP via {ws_url}: {e}"))
            })?;

            tokio::spawn(async move {
                while let Some(res) = handler.next().await {
                    if let Err(e) = res {
                        debug!("CDP handler error: {e}");
                        break;
                    }
                }
            });

            let page = browser
                .new_page("about:blank")
                .await
                .map_err(|e| Error::other(format!("failed to create page: {e}")))?;

            let target_id = page.target_id().clone();

            let fetch_result: Result<(String, Option<Vec<u8>>)> = async {
                // 设置 User-Agent（在导航前设置，确保首包生效）
                if !self.user_agent.is_empty() {
                    let _ = page.set_user_agent(&self.user_agent).await;
                }

                // 显式导航至目标 URL 并等待导航完成
                page.goto(&spec.fetch.url).await.map_err(|e| {
                    Error::other(format!("failed to navigate to {}: {e}", spec.fetch.url))
                })?;

                // 等待特定选择器（如果配置了 wait.selector）
                if let Some(ref sel) = spec.fetch.wait.selector {
                    let trimmed = sel.trim();
                    if !trimmed.is_empty() {
                        let wait_timeout = spec.fetch.wait.timeout.unwrap_or(10).max(1);
                        let wait_until = Instant::now() + Duration::from_secs(wait_timeout);
                        let mut matched = false;
                        while Instant::now() <= wait_until {
                            let eval_res = page
                                .evaluate(format!("Boolean(document.querySelector({:?}))", trimmed))
                                .await;
                            if let Ok(val) = eval_res
                                && val.into_value::<bool>().unwrap_or(false)
                            {
                                matched = true;
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                        if !matched {
                            warn!("timed out waiting for selector: {trimmed}");
                        }
                    }
                }

                // 执行自定义 JavaScript
                if let Some(ref js) = spec.fetch.evaluate
                    && !js.trim().is_empty()
                {
                    let _ = page.evaluate(js.as_str()).await;
                }

                // 获取页面 HTML 内容
                let html = page
                    .content()
                    .await
                    .map_err(|e| Error::other(format!("failed to get page content: {e}")))?;

                // 截图（若配置了 screenshot: true）
                let screenshot = if spec.fetch.screenshot {
                    let params = ScreenshotParams::builder()
                        .format(CaptureScreenshotFormat::Png)
                        .build();
                    page.screenshot(params).await.ok()
                } else {
                    None
                };

                Ok((html, screenshot))
            }
            .await;

            // 无论导航成功还是失败，均关闭 target 释放资源（兼容 Chrome 与各类无头浏览器）
            let _ = browser.execute(CloseTargetParams::new(target_id)).await;

            fetch_result
        };

        let (html, screenshot) =
            tokio::time::timeout(Duration::from_secs(timeout_secs), fetch_future)
                .await
                .map_err(|_| {
                    Error::other(format!("browser fetch timed out after {timeout_secs}s"))
                })??;

        let duration_ms = started.elapsed().as_millis() as u64;
        let content_sha256 = blake3::hash(html.as_bytes()).to_hex().to_string();
        let normalized_fingerprint = content_sha256.clone();

        Ok(FetchedDocument {
            final_url: spec.fetch.url.clone(),
            status: 200,
            text: html.clone(),
            html: Some(html),
            images: Vec::new(),
            screenshot,
            etag: None,
            last_modified: None,
            content_sha256,
            normalized_fingerprint,
            duration_ms,
            engine: "browser".to_string(),
            content_type: Some("text/html; charset=utf-8".to_string()),
            not_modified: false,
        })
    }
}
