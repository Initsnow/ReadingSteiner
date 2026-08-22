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

/// 创建抓取器。`settings` 提供可热更新的 UA / 超时覆盖（优先级高于 config.yaml），
/// 为 None 时回退到 `cfg.daemon` 的启动值。
pub fn create_fetcher(
    engine: &str,
    cfg: &crate::config::Config,
    settings: Option<&crate::config::EditableSettings>,
) -> Result<Box<dyn Fetcher>> {
    match engine {
        "http" => Ok(Box::new(http::HttpFetcher::new(
            &settings
                .map(|s| s.default_user_agent.as_str())
                .filter(|ua| !ua.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| cfg.daemon.effective_user_agent()),
            settings.map(|s| s.default_timeout_secs).unwrap_or(cfg.daemon.default_timeout_secs),
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
