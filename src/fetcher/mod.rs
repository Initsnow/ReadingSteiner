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

/// 创建抓取器。UA / 超时取自 SQLite 设置（`settings`），配置不再从 config.yaml 读取。
pub fn create_fetcher(
    engine: &str,
    cfg: &crate::config::Config,
    settings: &crate::config::EditableSettings,
) -> Result<Box<dyn Fetcher>> {
    match engine {
        "http" => {
            let user_agent = if settings.default_user_agent.trim().is_empty() {
                crate::config::DEFAULT_USER_AGENT.to_string()
            } else {
                settings.default_user_agent.clone()
            };
            Ok(Box::new(http::HttpFetcher::new(
                &user_agent,
                settings.default_timeout_secs,
            )?))
        }
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
