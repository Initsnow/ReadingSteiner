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
    /// 全局默认请求超时秒数（单个监控源可覆盖，见 FetchConfig::timeout_secs）。
    pub default_timeout_secs: u64,
    /// 全局默认 User-Agent（HTTP 抓取与图片下载使用）。
    pub default_user_agent: String,
    /// 每个监控源最多保留的历史变更事件条数（超出部分自动清理，0 表示不限制）。
    pub history_limit_per_source: usize,
    /// 连续失败达到多少次后发送一条 Telegram 失败通知（0 表示禁用）。
    pub failure_notify_threshold: u32,
    /// 监控检查调度器使用的时区（IANA 名称，如 Asia/Shanghai、UTC）。
    /// 影响基于本地时间的显示与告警时间戳；调度仍以 UTC 内部计算，仅做展示/告警换算。
    pub timezone: String,
    /// 全局默认检查间隔（秒）。监控源未单独覆盖时使用；默认 3600（1h）。
    pub interval_secs: u64,
    /// 全局默认检查间隔随机抖动秒数。监控源未单独覆盖时使用；默认 60s。
    pub jitter_secs: u64,
}

impl DaemonConfig {
    pub fn effective_timeout(&self, per_source: u64) -> u64 {
        if per_source > 0 {
            per_source
        } else if self.default_timeout_secs > 0 {
            self.default_timeout_secs
        } else {
            30
        }
    }

    pub fn effective_user_agent(&self) -> String {
        if self.default_user_agent.trim().is_empty() {
            format!("ReadingSteiner/{}", env!("CARGO_PKG_VERSION"))
        } else {
            self.default_user_agent.clone()
        }
    }

    /// 全局默认检查间隔（秒），0 时回退到 3600（1h）。
    pub fn effective_interval(&self) -> u64 {
        if self.interval_secs > 0 {
            self.interval_secs
        } else {
            3600
        }
    }
    /// 全局默认随机抖动（秒），0 时回退到 60s。
    pub fn effective_jitter(&self) -> u64 {
        if self.jitter_secs > 0 {
            self.jitter_secs
        } else {
            60
        }
    }

    pub fn effective_timezone(&self) -> String {
        if self.timezone.trim().is_empty() {
            system_local_timezone()
        } else {
            self.timezone.trim().to_string()
        }
    }
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
    /// 变更通知文案模板。支持占位符：{label}、{watch}、{time}、{tz}、{summary}、{items}。
    /// 留空则使用内置默认模板。
    pub template: String,
}

impl TelegramConfig {
    /// 获取事件通知模板，未配置时返回内置默认模板。
    pub fn event_template(&self) -> String {
        if self.template.trim().is_empty() {
            DEFAULT_EVENT_TEMPLATE.to_string()
        } else {
            self.template.clone()
        }
    }
}

/// 默认变更通知模板。占位符含义见 TelegramConfig::template 注释。
pub const DEFAULT_EVENT_TEMPLATE: &str = r#"<b>ReadingSteiner</b> — {label}
<b>{watch}</b>
<i>{time} {tz}</i>
{summary}
{items}"#;

/// 通过 Web/CLI 可编辑的全局设置视图。对应 config.yaml 的 daemon / telegram 段。
/// 不包含 token 等敏感密钥（token 仍通过 token_file 管理）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EditableSettings {
    /// 抓取工作线程数（并发数）。
    pub concurrency: usize,
    /// 队列容量。
    pub queue_capacity: usize,
    /// 默认请求超时秒数。
    pub default_timeout_secs: u64,
    /// 默认 User-Agent。
    pub default_user_agent: String,
    /// 每个监控源保留历史条数（0 不限制）。
    pub history_limit_per_source: usize,
    /// 连续失败达到多少次发送失败通知（0 禁用）。
    pub failure_notify_threshold: u32,
    /// 调度器时区（IANA 名称）。
    pub timezone: String,
    /// 全局默认检查间隔（秒），源未覆盖时使用。
    pub interval_secs: u64,
    /// 全局默认随机抖动（秒），源未覆盖时使用。
    pub jitter_secs: u64,
    /// 通知模板。
    pub template: String,
    /// Telegram 默认 chat id。
    pub default_chat_id: String,
    /// 单事件最多附带图片数。
    pub max_images_per_event: usize,
}

impl EditableSettings {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            concurrency: cfg.daemon.concurrency,
            queue_capacity: cfg.daemon.queue_capacity,
            default_timeout_secs: cfg.daemon.default_timeout_secs,
            default_user_agent: cfg.daemon.default_user_agent.clone(),
            history_limit_per_source: cfg.daemon.history_limit_per_source,
            failure_notify_threshold: cfg.daemon.failure_notify_threshold,
            timezone: cfg.daemon.timezone.clone(),
            interval_secs: cfg.daemon.effective_interval(),
            jitter_secs: cfg.daemon.effective_jitter(),
            template: cfg.telegram.template.clone(),
            default_chat_id: cfg.telegram.default_chat_id.clone(),
            max_images_per_event: cfg.telegram.max_images_per_event,
        }
    }

    /// 把可编辑设置写回 config（会合并 token 等未编辑字段）。
    pub fn apply_to(&self, cfg: &mut Config) {
        cfg.daemon.concurrency = self.concurrency;
        cfg.daemon.queue_capacity = self.queue_capacity;
        cfg.daemon.default_timeout_secs = self.default_timeout_secs;
        cfg.daemon.default_user_agent = self.default_user_agent.clone();
        cfg.daemon.history_limit_per_source = self.history_limit_per_source;
        cfg.daemon.failure_notify_threshold = self.failure_notify_threshold;
        cfg.daemon.timezone = self.timezone.clone();
        cfg.daemon.interval_secs = self.interval_secs;
        cfg.daemon.jitter_secs = self.jitter_secs;
        cfg.telegram.template = self.template.clone();
        cfg.telegram.default_chat_id = self.default_chat_id.clone();
        cfg.telegram.max_images_per_event = self.max_images_per_event;
    }
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
    /// 是否启用监控（调度检查）。false 时该源不会被调度器抓取检测。
    pub enabled: bool,
    /// 是否发送变更通知。false 时仍正常监控检测，但检测到的变更不会推送 Telegram 通知。
    #[serde(default = "default_true")]
    pub notify_enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub fetch: FetchConfig,
    pub schedule: ScheduleConfig,
    /// 内容提取方式。决定「把抓到的内容变成什么拿来比对」。
    #[serde(default)]
    pub extract: ExtractConfig,
}

/// serde 默认值辅助：返回 `true`，用于新增布尔字段（如 `notify_enabled`）
/// 反序列化旧配置时自动补齐默认值。
fn default_true() -> bool {
    true
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            notify_enabled: true,
            tags: Vec::new(),
            fetch: FetchConfig::default(),
            schedule: ScheduleConfig::default(),
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

/// 调度配置。`interval_secs` / `jitter_secs` 可选：留空（None）时使用全局设置
/// （`DaemonConfig.interval_secs` / `DaemonConfig.jitter_secs`）中的值。
/// 若设置了 `cron`，则按 cron 表达式调度，`interval_secs` / `jitter_secs` 被忽略。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScheduleConfig {
    /// 检查间隔秒数，None 表示使用全局默认（默认 3600s = 1h）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    /// 检查间隔随机抖动秒数，None 表示使用全局默认（默认 60s）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter_secs: Option<u64>,
    /// 可选 cron 表达式（标准 5 段：`分 时 日 月 周`）。
    /// 设置后按 cron 精确调度，忽略 interval_secs / jitter_secs。
    /// 例：`*/15 * * * *`（每 15 分钟）、`0 9,18 * * 1-5`（工作日 9:00/18:00）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
}

impl ScheduleConfig {
    /// 该源是否使用 cron 精确调度（而非固定间隔）。
    pub fn uses_cron(&self) -> bool {
        self.cron.as_deref().is_some_and(|c| !c.trim().is_empty())
    }
    /// 返回该源的有效检查间隔（秒）：优先用源自身的覆盖值，否则用全局默认。
    /// 仅在非 cron 模式下有意义。
    pub fn effective_interval(&self, global: u64) -> u64 {
        self.interval_secs.filter(|&v| v > 0).unwrap_or(global)
    }
    /// 返回该源的有效随机抖动（秒）。
    pub fn effective_jitter(&self, global: u64) -> u64 {
        self.jitter_secs.unwrap_or(global)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractConfig {
    /// 整页文本监控（默认）。直接把页面（或接口返回的文本）作为比对内容，
    /// 有任何变化即视为变更。适合文章、纯文本页面、整页指纹监控。
    Text {
        /// 可选图片选择器：从页面中挑选要随通知附带的图片。
        /// 缺省时不附带图片。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<ImageSelector>,
    },
    /// 结构化条目监控。从页面 / JSON 中按规则提取出若干「条目」，
    /// 自动对比条目的新增 / 更新 / 移除，无需配置稳定字段。
    Items {
        selector: ItemSelector,
        #[serde(default)]
        fields: Vec<ItemField>,
        /// 条目排序前的去重键模板（可选）。可用 {{字段}} 占位符。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dedupe_key: Option<String>,
        /// 可选图片选择器：控制如何挑选随通知附带的图片。
        /// 缺省时沿用条目提取时自动收集的图片（等价于 items）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<ImageSelector>,
    },
}

/// 图片通知链路：如何从页面 / 条目中挑选要随变更通知附带的图片。
///
/// - `none`（默认）：不附带图片。
/// - `items`：收集结构化条目提取时自动带出的 `image_urls`。
/// - `css { selector }`：用 CSS 选择器从整页匹配 `<img>`，取其 `src`/`data-src`。
/// - `changed`：只收集**发生变更的元素**相关（其自身子树与父容器）的 `<img>`，
///   避免把整页全部图片都发出去。仅对结构化条目（Items 提取）生效。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSelector {
    /// 不附带图片（默认）。
    #[default]
    None,
    /// 收集结构化条目提取时自动带出的图片。
    Items,
    /// 用 CSS 选择器从页面匹配图片元素，取其 `src`/`data-src` 等属性。
    Css { selector: String },
    /// 只收集发生变更的元素相关的图片（其自身子树 + 父容器的 img）。
    Changed,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        ExtractConfig::Text { images: None }
    }
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
    pub default_timeout_secs: u64,
    pub default_user_agent: String,
    pub history_limit_per_source: usize,
    pub failure_notify_threshold: u32,
    pub timezone: String,
    /// 全局默认检查间隔（秒，已应用有效值）。
    pub interval_secs: u64,
    /// 全局默认随机抖动（秒，已应用有效值）。
    pub jitter_secs: u64,
    pub template: String,
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
            default_timeout_secs: cfg.daemon.default_timeout_secs,
            default_user_agent: cfg.daemon.effective_user_agent(),
            history_limit_per_source: cfg.daemon.history_limit_per_source,
            failure_notify_threshold: cfg.daemon.failure_notify_threshold,
            timezone: cfg.daemon.effective_timezone(),
            interval_secs: cfg.daemon.effective_interval(),
            jitter_secs: cfg.daemon.effective_jitter(),
            template: cfg.telegram.event_template(),
        }
    }

    /// 全局默认检查间隔（秒）。
    pub fn interval_secs(&self) -> u64 {
        if self.interval_secs > 0 {
            self.interval_secs
        } else {
            3600
        }
    }
    /// 全局默认随机抖动（秒）。
    pub fn jitter_secs(&self) -> u64 {
        if self.jitter_secs > 0 {
            self.jitter_secs
        } else {
            60
        }
    }
}

/// 返回系统本地时区的 IANA 名称（如 `Asia/Shanghai`、`America/New_York`）。
/// 检测失败时回退到 `UTC`。
pub fn system_local_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}
