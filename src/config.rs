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
    pub pipelines: HashMap<String, PipelineConfig>,
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

    pub fn pipeline(&self, id: &str) -> Option<&PipelineConfig> {
        self.pipelines.get(id)
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
    pub pipeline: String,
    pub compare: CompareConfig,
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
            pipeline: "default".to_string(),
            compare: CompareConfig::default(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompareConfig {
    pub mode: CompareMode,
    pub stable_id: String,
    #[serde(default)]
    pub ignore_fields: Vec<String>,
    #[serde(default)]
    pub notify_on: Vec<ChangeType>,
    #[serde(default = "default_confirm_count")]
    pub confirm_count: usize,
    #[serde(default)]
    pub cooldown_secs: u64,
}

fn default_confirm_count() -> usize {
    1
}

impl Default for CompareConfig {
    fn default() -> Self {
        Self {
            mode: CompareMode::ItemSet,
            stable_id: "id".to_string(),
            ignore_fields: Vec::new(),
            notify_on: vec![ChangeType::New, ChangeType::Updated, ChangeType::Removed],
            confirm_count: 1,
            cooldown_secs: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompareMode {
    #[default]
    ItemSet,
    RawDigest,
    TextSim,
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
pub struct PipelineConfig {
    #[serde(default)]
    pub extract: Vec<ExtractConfig>,
    #[serde(default)]
    pub normalize: Vec<NormalizeConfig>,
    #[serde(default)]
    pub filter: FilterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractConfig {
    CssItems {
        selector: String,
        #[serde(default)]
        fields: HashMap<String, FieldSelector>,
    },
    Xpath {
        selector: String,
        #[serde(default)]
        fields: HashMap<String, FieldSelector>,
    },
    JsonPath {
        path: String,
        #[serde(default)]
        fields: HashMap<String, FieldSelector>,
    },
    Regex {
        pattern: String,
        #[serde(default)]
        fields: HashMap<String, FieldSelector>,
    },
    AutoText,
    AutoImages,
    CamofoxImages,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FieldSelector {
    pub selector: Option<String>,
    pub attr: Option<String>,
    pub path: Option<String>,
    pub regex: Option<String>,
    pub group: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NormalizeConfig {
    Strip {
        field: String,
        chars: String,
    },
    Trim {
        field: String,
    },
    AbsUrl {
        field: String,
        base: String,
    },
    Lowercase {
        field: String,
    },
    Replace {
        field: String,
        pattern: String,
        with: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FilterConfig {
    #[serde(default)]
    pub include: Vec<Condition>,
    #[serde(default)]
    pub exclude: Vec<Condition>,
    pub drop_duplicate: Option<DropDuplicate>,
    pub min_items: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Condition {
    Eq { field: String, value: String },
    Ne { field: String, value: String },
    Gt { field: String, value: f64 },
    Lt { field: String, value: f64 },
    Regex { field: String, pattern: String },
    Glob { field: String, pattern: String },
    Contains { field: String, value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropDuplicate {
    pub key: String,
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
