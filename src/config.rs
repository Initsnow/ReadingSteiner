use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub state_dir: PathBuf,
    pub media_dir: PathBuf,
    pub daemon: DaemonConfig,
    pub web: WebConfig,
    pub telegram: TelegramConfig,
    pub camofox: CamofoxConfig,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|e| Error::config(format!("read {}: {e}", path.as_ref().display())))?;
        Self::from_yaml(&text)
    }

    pub fn from_yaml(text: &str) -> Result<Self> {
        let mut cfg: Config = serde_yaml::from_str(text)?;
        if cfg.state_dir.as_os_str().is_empty() {
            cfg.state_dir = PathBuf::from("state");
        }
        if cfg.media_dir.as_os_str().is_empty() {
            cfg.media_dir = cfg.state_dir.join("media");
        }
        Ok(cfg)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(path, yaml)?;
        Ok(())
    }

    pub fn socket_path(&self) -> PathBuf {
        if self.daemon.socket_path.as_os_str().is_empty() {
            self.state_dir.join("daemon.sock")
        } else {
            self.daemon.socket_path.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebConfig {
    pub listen: String,
    pub static_dir: PathBuf,
}

impl WebConfig {
    pub fn effective_listen(&self) -> String {
        if self.listen.is_empty() {
            "127.0.0.1:8901".to_string()
        } else {
            self.listen.clone()
        }
    }
    pub fn static_dir(&self) -> PathBuf {
        if self.static_dir.as_os_str().is_empty() {
            PathBuf::from("web/dist")
        } else {
            self.static_dir.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub concurrency: usize,
    pub queue_capacity: usize,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TelegramConfig {
    pub token: String,
    pub token_file: PathBuf,
    pub default_chat_id: String,
    pub api_base: String,
    pub max_images_per_event: usize,
    pub image_bytes_budget: u64,
    pub digest_window_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CamofoxConfig {
    pub enabled: bool,
    pub base_url: String,
    pub access_key_file: PathBuf,
    pub api_key_file: PathBuf,
    pub user_id: String,
    pub session_key: String,
    pub health_check_interval_secs: u64,
    pub pool_size: usize,
}

/// 一个监控源。核心只有三件事：抓什么（fetch）、提取什么（extract）、
/// 何时检测（schedule）。不再有「流水线 / 比较模式 / 稳定字段」这些复杂概念，
/// 变更检测完全由提取结果驱动。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub fetch: FetchConfig,
    pub schedule: ScheduleConfig,
    pub priority: i32,
    /// 内容提取方式。决定「把抓到的内容变成什么拿来比对」。
    #[serde(default)]
    pub extract: ExtractConfig,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            tags: Vec::new(),
            fetch: FetchConfig::default(),
            schedule: ScheduleConfig::default(),
            priority: 0,
            extract: ExtractConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FetchConfig {
    pub engine: String,
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub max_body_bytes: usize,
    pub timeout_secs: u64,
    pub wait: WaitConfig,
    pub tab_policy: String,
    pub evaluate: Option<String>,
    pub screenshot: bool,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            engine: "http".to_string(),
            url: String::new(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            max_body_bytes: 5 * 1024 * 1024,
            timeout_secs: 30,
            wait: WaitConfig::default(),
            tab_policy: "reuse".to_string(),
            evaluate: None,
            screenshot: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WaitConfig {
    pub selector: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleConfig {
    pub interval_secs: u64,
    pub jitter_secs: u64,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            jitter_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractConfig {
    /// 整页文本监控（默认）。直接把页面（或接口返回的文本）作为比对内容，
    /// 有任何变化即视为变更。适合文章、纯文本页面、整页指纹监控。
    #[default]
    Text,
    /// 结构化条目监控。从页面 / JSON 中按规则提取出若干「条目」，
    /// 自动对比条目的新增 / 更新 / 移除，无需配置稳定字段。
    Items {
        selector: ItemSelector,
        #[serde(default)]
        fields: Vec<ItemField>,
        /// 条目排序前的去重键模板（可选）。可用 {{字段}} 占位符。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dedupe_key: Option<String>,
    },
}

/// 结构化条目提取的选择器。按内容类型自动区分：
/// HTML 用 CSS/XPath，JSON 用 JSONPath。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemSelector {
    Css { selector: String },
    JsonPath { path: String },
}

/// 条目中的单个字段提取规则。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ItemField {
    pub name: String,
    pub selector: Option<String>,
    pub attr: Option<String>,
    pub path: Option<String>,
    pub regex: Option<String>,
    pub group: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    New,
    Updated,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RuntimeConfig {
    pub state_dir: PathBuf,
    pub media_dir: PathBuf,
    pub socket_path: PathBuf,
    pub concurrency: usize,
    pub queue_capacity: usize,
}

impl RuntimeConfig {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            state_dir: cfg.state_dir.clone(),
            media_dir: cfg.media_dir.clone(),
            socket_path: cfg.socket_path(),
            concurrency: if cfg.daemon.concurrency == 0 {
                16
            } else {
                cfg.daemon.concurrency
            },
            queue_capacity: if cfg.daemon.queue_capacity == 0 {
                1024
            } else {
                cfg.daemon.queue_capacity
            },
        }
    }
}
