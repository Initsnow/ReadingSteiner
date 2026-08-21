pub mod camofox;
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

pub fn engine_name(fetch: &FetchConfig) -> &str {
    if fetch.engine.is_empty() {
        "http"
    } else {
        &fetch.engine
    }
}

pub fn create_fetcher(engine: &str, cfg: &crate::config::Config) -> Result<Box<dyn Fetcher>> {
    match engine {
        "http" => Ok(Box::new(http::HttpFetcher::new(
            &cfg.daemon.effective_user_agent(),
            cfg.daemon.default_timeout_secs,
        )?)),
        "camofox" => {
            if !cfg.camofox.enabled {
                return Err(crate::error::Error::config(
                    "camofox engine requested but camofox.enabled=false",
                ));
            }
            Ok(Box::new(camofox::CamofoxFetcher::new(&cfg.camofox)?))
        }
        other => Err(crate::error::Error::config(format!(
            "unknown fetch engine: {other}"
        ))),
    }
}
