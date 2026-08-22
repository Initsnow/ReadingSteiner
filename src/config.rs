use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::{TagConfig, TelegramTarget};

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

/// `config.yaml` 的 `daemon` 段：只保留启动所需的引导项。
/// 可编辑的运行参数（并发、队列、超时、cron、UA、历史保留、失败阈值、时区）
/// 已迁移到 SQLite `settings` 表，见 [`EditableSettings`]。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub log_level: String,
}

/// Telegram 通知器运行配置。`config.yaml` 的 `telegram` 段只提供启动引导项
/// （`api_base` 等）；可编辑项 `url` / `max_images_per_event` / `template`
/// 已迁移到 SQLite `settings` 表，通过 [`EditableSettings`] 覆盖注入。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TelegramConfig {
    /// 全局通知目标，格式：`tgram://bottoken/ChatID1/ChatID2`。
    /// 编码了 bot token 与一个或多个 chat id。通常由 SQLite 设置注入。
    pub url: String,
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

    /// 以 SQLite 设置覆盖 `url` / `template` / `max_images_per_event`，
    /// 保留 `api_base` 等启动项。这是设置进入 notifier 的唯一入口。
    pub fn with_overrides(&self, settings: &EditableSettings) -> Self {
        let mut c = self.clone();
        c.url = settings.telegram_url.clone();
        c.template = settings.template.clone();
        c.max_images_per_event = settings.max_images_per_event;
        c
    }
}

/// 默认变更通知模板。占位符含义见 TelegramConfig::template 注释。
pub const DEFAULT_EVENT_TEMPLATE: &str = r#"<b>ReadingSteiner</b> — {label}
<b>{watch}</b>
<i>{time} {tz}</i>
{summary}
{items}"#;

/// 默认全局抓取频率：每小时。
pub const DEFAULT_CRON: &str = "0 * * * *";

/// 默认 User-Agent：模拟 Chrome/Edge 浏览器，避免被站点按 UA 拦截。
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36 Edg/151.0.0.0";

/// 全局可编辑设置，存于 SQLite `settings` 表，是这些运行参数的唯一来源。
/// 通过 Web 控制台「设置」页或 CLI 读写；不再与 config.yaml 双向绑定。
/// 通知目标以 tgram:// url 形式管理，不在此暴露 token 明文。
///
/// 默认值见 [`Default`]：新建数据库/升级时由 v11 迁移 seed 一条 `global` 记录，
/// 因此 `get_settings()` 恒有值，运行时不再做「缺失→默认」兜底。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 全局 Telegram 通知目标（`tgram://bottoken/ChatID1/ChatID2`）。
    pub telegram_url: String,
    /// 单事件最多附带图片数。
    pub max_images_per_event: usize,
}

impl Default for EditableSettings {
    fn default() -> Self {
        Self {
            concurrency: 16,
            queue_capacity: 1024,
            default_timeout_secs: 30,
            default_cron: DEFAULT_CRON.to_string(),
            default_user_agent: DEFAULT_USER_AGENT.to_string(),
            history_limit_per_source: 0,
            failure_notify_threshold: 0,
            timezone: system_local_timezone(),
            template: DEFAULT_EVENT_TEMPLATE.to_string(),
            telegram_url: String::new(),
            max_images_per_event: 10,
        }
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
    /// 则其历史保留条数、通知目标与内容提取配置继承分组的设置；
    /// false 时完全使用本监控源自身的设置（自覆盖）。
    /// 监控 / 通知开关始终由监控源自身控制，分组不参与叠加。
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

/// 解析监控源的「生效」开关与历史保留条数。
///
/// 监控 / 通知开关由监控源自身 `enabled` / `notify_enabled` 独立控制，分组不参与叠加。
/// 历史保留条数若监控源开启 `follow_group` 且带有已配置的分组，则取各分组中的
/// 最小值（最严格的保留策略）；否则使用全局设置。
pub fn resolve_effective_source(
    source: &SourceConfig,
    tags: &[crate::models::TagConfig],
    global_history_limit: usize,
) -> (bool, bool, usize) {
    let history = if !source.follow_group || source.tags.is_empty() {
        global_history_limit
    } else {
        let group_tags: Vec<&crate::models::TagConfig> = tags
            .iter()
            .filter(|t| source.tags.iter().any(|tag| tag == &t.name))
            .collect();
        if group_tags.is_empty() {
            // 有标签但没有对应分组配置：不改变行为，使用全局设置。
            global_history_limit
        } else {
            group_tags
                .iter()
                .map(|t| t.history_limit)
                .filter(|&h| h > 0)
                .min()
                .unwrap_or(global_history_limit)
        }
    };
    (source.enabled, source.notify_enabled, history)
}

/// 解析 `tgram://bottoken/ChatID1/ChatID2` 形式的 Telegram 通知目标。
///
/// 格式：`tgram://<token>/<chat_id1>/<chat_id2>/...`，其中 token 为
/// Bot API 的完整 token（如 `123456:ABC`），chat id 为接收者 ID，可多个。
/// 解析失败（非 tgram:// 前缀、缺少 token 或 chat id）时返回错误。
pub fn parse_telegram_url(url: &str) -> Result<TelegramTarget> {
    let s = url.trim();
    if s.is_empty() {
        return Err(Error::config("telegram url is empty"));
    }
    let rest = s
        .strip_prefix("tgram://")
        .ok_or_else(|| Error::config("telegram url must start with tgram://"))?;
    let mut parts: Vec<&str> = rest.split('/').collect();
    let token = parts.remove(0).trim().to_string();
    if token.is_empty() {
        return Err(Error::config("telegram url missing bot token"));
    }
    let chat_ids: Vec<String> = parts
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if chat_ids.is_empty() {
        return Err(Error::config("telegram url missing chat id"));
    }
    Ok(TelegramTarget { token, chat_ids })
}

/// 解析监控源的生效通知目标（token + chat ids）。
///
/// 若监控源「跟随分组」且所属分组配置了 `notify_url`，则使用分组的通知目标；
/// 多个分组配置冲突时按分组名升序取第一个非空的分组。否则沿用全局通知目标。
/// 返回 `None` 表示全局也未配置可用的通知目标。
pub fn resolve_notify_target(
    source: &SourceConfig,
    tags: &[TagConfig],
    global_url: &str,
) -> Option<TelegramTarget> {
    let fallback = || {
        parse_telegram_url(global_url)
            .ok()
            .filter(|t| t.is_valid())
    };
    if !source.follow_group || source.tags.is_empty() {
        return fallback();
    }
    let mut group_urls: Vec<&TagConfig> = tags
        .iter()
        .filter(|t| source.tags.contains(&t.name) && !t.notify_url.trim().is_empty())
        .collect();
    group_urls.sort_by_key(|t| t.name.clone());
    if let Some(t) = group_urls.first() {
        if let Ok(target) = parse_telegram_url(&t.notify_url) {
            if target.is_valid() {
                return Some(target);
            }
        }
    }
    fallback()
}

/// 解析监控源的生效内容提取配置。
///
/// 若监控源「跟随分组」且所属分组配置了 `extract`，则使用分组的提取设置；
/// 否则使用监控源自身的提取设置。多个分组都配置了提取时按分组名升序取第一个。
pub fn resolve_effective_extract(source: &SourceConfig, tags: &[TagConfig]) -> ExtractConfig {
    if !source.follow_group || source.tags.is_empty() {
        return source.extract.clone();
    }
    let mut group_extracts: Vec<&TagConfig> = tags
        .iter()
        .filter(|t| source.tags.contains(&t.name) && t.extract.is_some())
        .collect();
    group_extracts.sort_by_key(|t| t.name.clone());
    if let Some(t) = group_extracts.first() {
        if let Some(extract) = &t.extract {
            return extract.clone();
        }
    }
    source.extract.clone()
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
    /// 由启动配置（boot）+ SQLite 设置（settings）合成运行时配置。
    /// settings 是这些运行参数的唯一来源，其值已在数据库 seed/保存时保证非空，直接透传。
    pub fn from_parts(cfg: &Config, settings: &EditableSettings) -> Self {
        Self {
            state_dir: cfg.state_dir.clone(),
            media_dir: cfg.media_dir.clone(),
            socket_path: cfg.socket_path(),
            concurrency: settings.concurrency,
            queue_capacity: settings.queue_capacity,
            default_timeout_secs: settings.default_timeout_secs,
            default_cron: settings.default_cron.clone(),
            default_user_agent: settings.default_user_agent.clone(),
            history_limit_per_source: settings.history_limit_per_source,
            failure_notify_threshold: settings.failure_notify_threshold,
            timezone: settings.timezone.clone(),
            template: settings.template.clone(),
        }
    }
}

/// 返回系统本地时区的 IANA 名称（如 `Asia/Shanghai`、`America/New_York`）。
/// 检测失败时回退到 `UTC`。
pub fn system_local_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}
