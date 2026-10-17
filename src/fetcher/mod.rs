pub mod browser;
pub mod http;

use async_trait::async_trait;

use crate::config::FetchConfig;
use crate::error::Result;
use crate::models::FetchedDocument;

#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, spec: &FetchSpec) -> Result<FetchedDocument>;
}

#[derive(Debug, Clone)]
pub struct FetchSpec {
    pub fetch: FetchConfig,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub source_id: String,
}

/// 创建抓取器。UA、超时与引擎配置 100% 取自权威 Config（config.yaml）。
pub fn create_fetcher(
    engine: &str,
    cfg: &crate::config::Config,
) -> Result<Box<dyn Fetcher>> {
    match engine {
        "http" => Ok(Box::new(http::HttpFetcher::new(
            &cfg.daemon.default_user_agent,
            cfg.daemon.default_timeout_secs,
        )?)),
        "browser" => {
            if !cfg.browser.enabled || cfg.browser.cdp_url.trim().is_empty() {
                return Err(crate::error::Error::config(
                    "browser engine requested but browser.enabled=false or cdp_url is empty in config.yaml",
                ));
            }
            Ok(Box::new(browser::BrowserFetcher::new(
                &cfg.browser,
                &cfg.daemon.default_user_agent,
            )))
        }
        other => Err(crate::error::Error::config(format!(
            "unknown fetch engine: {other}"
        ))),
    }
}
