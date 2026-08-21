use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// 全局默认 cron 表达式（单个监控源可覆盖，见 ScheduleConfig::cron）。
    /// 留空时回退到每小时（`0 * * * *`）。
    pub default_cron: String,
    /// 全局默认 User-Agent（HTTP 抓取与图片下载使用）。
    pub default_user_agent: String,
    /// 每个监控源最多保留的历史变更事件条数（超出部分自动清理，0 表示不限制）。
    pub history_limit_per_source: usize,
    /// 连续失败达到多少次后发送一条 Telegram 失败通知（0 表示禁用）。
    pub failure_notify_threshold: u32,
    /// 监控检查调度器使用的时区（IANA 名称，如 Asia/Shanghai、UTC）。
    /// 影响基于本地时间的显示与告警时间戳；调度仍以 UTC 内部计算，仅做展示/告警换算。
    pub timezone: String,
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

    /// 全局默认 cron 表达式。单个监控源未配置 cron 时使用该值；
    /// 留空时回退到每小时（`0 * * * *`）。
    pub fn effective_cron(&self) -> String {
        if self.default_cron.trim().is_empty() {
            "0 * * * *".to_string()
        } else {
            self.default_cron.trim().to_string()
        }
    }

    pub fn effective_user_agent(&self) -> String {
        if self.default_user_agent.trim().is_empty() {
            format!("ReadingSteiner/{}", env!("CARGO_PKG_VERSION"))
        } else {
            self.default_user_agent.clone()
        }
    }

    /// 全局默认随机抖动（秒），0 时回退到 60s。
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
    /// 默认 cron 表达式（新建监控源未单独配置时使用）。
    pub default_cron: String,
    /// 默认 User-Agent。
    pub default_user_agent: String,
    /// 每个监控源保留历史条数（0 不限制）。
    pub history_limit_per_source: usize,
    /// 连续失败达到多少次发送失败通知（0 禁用）。
    pub failure_notify_threshold: u32,
    /// 调度器时区（IANA 名称）。
    pub timezone: String,
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
            default_cron: cfg.daemon.default_cron.clone(),
            default_user_agent: cfg.daemon.default_user_agent.clone(),
            history_limit_per_source: cfg.daemon.history_limit_per_source,
            failure_notify_threshold: cfg.daemon.failure_notify_threshold,
            timezone: cfg.daemon.timezone.clone(),
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
        cfg.daemon.default_cron = self.default_cron.clone();
        cfg.daemon.default_user_agent = self.default_user_agent.clone();
        cfg.daemon.history_limit_per_source = self.history_limit_per_source;
        cfg.daemon.failure_notify_threshold = self.failure_notify_threshold;
        cfg.daemon.timezone = self.timezone.clone();
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
    /// 是否跟随所属分组（标签）的设置。true 时，若监控源带有已配置的分组，
    /// 则其监控开关、通知开关与历史保留条数继承分组的设置；
    /// false 时完全使用本监控源自己的 `enabled` / `notify_enabled` 设置（自覆盖）。
    #[serde(default = "default_true")]
    pub follow_group: bool,
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
            follow_group: true,
            tags: Vec::new(),
            fetch: FetchConfig::default(),
            schedule: ScheduleConfig::default(),
            extract: ExtractConfig::default(),
        }
    }
}

/// 解析监控源的「生效」开关配置。
///
/// 若监控源开启 `follow_group` 且带有已配置的分组，则监控 / 通知开关与历史保留条数
/// 继承分组的设置；否则使用监控源自身的 `enabled` / `notify_enabled`。
///
/// 一个监控源可挂多个分组，这里采用「保守策略」：监控开关取各分组 `enabled` 的逻辑与
/// （任一分组关闭监控则该源暂停监控），通知开关取各分组 `notify_enabled` 的逻辑与，
/// 历史保留条数取各分组中的最小值（最严格的保留策略）。
pub fn resolve_effective_source(
    source: &SourceConfig,
    tags: &[crate::models::TagConfig],
    global_history_limit: usize,
) -> (bool, bool, usize) {
    // 若源未跟随分组，或没有标签，则完全使用自身设置。
    if !source.follow_group || source.tags.is_empty() {
        return (source.enabled, source.notify_enabled, global_history_limit);
    }
    let group_tags: Vec<&crate::models::TagConfig> = tags
        .iter()
        .filter(|t| source.tags.iter().any(|tag| tag == &t.name))
        .collect();
    if group_tags.is_empty() {
        // 有标签但没有对应分组配置：仍使用自身设置（分组未配置时不改变行为）。
        return (source.enabled, source.notify_enabled, global_history_limit);
    }
    let enabled = group_tags.iter().all(|t| t.enabled);
    let notify = group_tags.iter().all(|t| t.notify_enabled);
    let history = group_tags
        .iter()
        .map(|t| t.history_limit)
        .filter(|&h| h > 0)
        .min()
        .unwrap_or(global_history_limit);
    (enabled, notify, history)
}

/// 自动生成监控源 ID：优先从名称生成可读 slug，否则从 URL 主机名生成，
/// 再回退到随机短 id。保证结果非空、且适合作为标识符使用。
pub fn generate_source_id(name: &str, url: &str) -> String {
    let base = if !name.trim().is_empty() {
        name.trim().to_string()
    } else if let Ok(u) = url::Url::parse(url.trim()) {
        u.host_str().unwrap_or("").to_string()
    } else {
        String::new()
    };
    let slug = slugify(&base);
    if !slug.is_empty() {
        format!("{}-{}", slug, short_rand())
    } else {
        format!("source-{}", short_rand())
    }
}

/// 把任意字符串转成小写短横线 slug（保留 ASCII 字母数字、CJK 字符与 `-`）。
fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            // 保留中日韩等表意文字，避免中文名称退化为不可读的随机 id。
            out.push(ch);
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

/// 判断是否为中日韩统一表意文字及扩展区（含假名、谚文）。
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK 统一表意文字基本区
        | '\u{3400}'..='\u{4DBF}' // 扩展 A
        | '\u{F900}'..='\u{FAFF}' // 兼容表意
        | '\u{3040}'..='\u{30FF}' // 平假名 / 片假名
        | '\u{AC00}'..='\u{D7AF}' // 谚文音节
    )
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 生成一个 6 位十六进制随机后缀，用于避免 ID 冲突。
/// 取 UUID v4 的随机熵并叠加进程内自增计数器，保证同纳秒内快速连续
/// 添加监控源也不会撞出相同后缀，跨进程则依赖 UUID 的独立随机性。
fn short_rand() -> String {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let u = uuid::Uuid::new_v4();
    let low = u.as_u128() as u64;
    format!("{:06x}", (low ^ seq) & 0xffffff)
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

/// 调度配置。仅通过 `cron` 表达式精确调度（标准 5 段：`分 时 日 月 周`）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScheduleConfig {
    /// cron 表达式（标准 5 段：`分 时 日 月 周`）。
    /// 例：`*/15 * * * *`（每 15 分钟）、`0 9,18 * * 1-5`（工作日 9:00/18:00）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
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
    pub default_cron: String,
    pub default_user_agent: String,
    pub history_limit_per_source: usize,
    pub failure_notify_threshold: u32,
    pub timezone: String,
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
            default_cron: cfg.daemon.effective_cron(),
            default_user_agent: cfg.daemon.effective_user_agent(),
            history_limit_per_source: cfg.daemon.history_limit_per_source,
            failure_notify_threshold: cfg.daemon.failure_notify_threshold,
            timezone: cfg.daemon.effective_timezone(),
            template: cfg.telegram.event_template(),
        }
    }
}

/// 返回系统本地时区的 IANA 名称（如 `Asia/Shanghai`、`America/New_York`）。
/// 检测失败时回退到 `UTC`。
pub fn system_local_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}
