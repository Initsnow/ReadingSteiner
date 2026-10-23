use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::TelegramTarget;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub state_dir: PathBuf,
    pub media_dir: PathBuf,
    pub daemon: DaemonConfig,
    pub web: WebConfig,
    pub telegram: TelegramConfig,
    pub browser: BrowserConfig,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|e| Error::config(format!("read {}: {e}", path.as_ref().display())))?;
        let mut cfg = Self::from_yaml(&text)?;
        cfg.resolve_secrets()?;
        Ok(cfg)
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

    pub fn resolve_secrets(&mut self) -> Result<()> {
        if self.telegram.url.trim().is_empty()
            && let Some(path) = &self.telegram.url_file
        {
            let text = std::fs::read_to_string(path).map_err(|e| {
                Error::config(format!("read telegram.url_file {}: {e}", path.display()))
            })?;
            self.telegram.url = text.trim().to_string();
        }
        if self.web.auth_token.trim().is_empty()
            && let Some(path) = &self.web.auth_token_file
        {
            let text = std::fs::read_to_string(path).map_err(|e| {
                Error::config(format!("read web.auth_token_file {}: {e}", path.display()))
            })?;
            self.web.auth_token = text.trim().to_string();
        }
        Ok(())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        if is_path_readonly(path.as_ref()) {
            return Err(Error::config(format!(
                "配置文件 {} 处于只读模式（由 NixOS 或外部只读挂载管理），无法写回修改",
                path.as_ref().display()
            )));
        }
        let yaml = serde_yaml::to_string(self)?;
        let tmp_path = format!("{}.tmp.{}", path.as_ref().display(), std::process::id());
        std::fs::write(&tmp_path, yaml)?;
        std::fs::rename(&tmp_path, path.as_ref())?;
        Ok(())
    }
}

pub fn is_path_readonly(path: &Path) -> bool {
    if path.is_dir() {
        let probe = path.join(format!(".write_probe_{}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&probe)
        {
            Ok(file) => {
                drop(file);
                let _ = std::fs::remove_file(probe);
                false
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => true,
            Err(_) => false,
        }
    } else if path.exists() {
        if let Ok(metadata) = std::fs::metadata(path)
            && metadata.permissions().readonly()
        {
            return true;
        }
        match std::fs::OpenOptions::new().write(true).open(path) {
            Ok(_) => false,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => true,
            Err(_) => false,
        }
    } else {
        if let Some(parent) = path.parent()
            && parent.exists()
        {
            return is_path_readonly(parent);
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebConfig {
    pub listen: String,
    pub static_dir: PathBuf,
    /// Web 控制台 API 的鉴权 Token（Bearer）。留空表示不启用鉴权（默认仅本地访问）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_token: String,
    /// Web 控制台 API 鉴权 Token 的外部文件路径（便于 sops-nix 注入，敏感信息不入 config.yaml）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token_file: Option<PathBuf>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<PathBuf>,
    pub log_level: String,
    pub concurrency: usize,
    pub queue_capacity: usize,
    pub default_timeout_secs: u64,
    pub default_cron: String,
    pub default_user_agent: String,
    pub history_limit_per_source: usize,
    pub failure_notify_threshold: u32,
    pub timezone: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: None,
            log_level: "info".to_string(),
            concurrency: 16,
            queue_capacity: 1024,
            default_timeout_secs: 120,
            default_cron: DEFAULT_CRON.to_string(),
            default_user_agent: DEFAULT_USER_AGENT.to_string(),
            history_limit_per_source: 0,
            failure_notify_threshold: 0,
            timezone: system_local_timezone(),
        }
    }
}

/// 浏览器引擎运行配置（通过 CDP 连接外部无头 Chrome）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub cdp_url: String,
    pub timeout_secs: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cdp_url: "http://127.0.0.1:9222".to_string(),
            timeout_secs: 120,
        }
    }
}

/// 站点分组（预设/模板）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct GroupConfig {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_cron: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub default_headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_browser_config: Option<BrowserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_extract: Option<ExtractConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_notify_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Telegram 通知器运行配置。统一在 `config.yaml` 配置，为唯一真相源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    /// 全局通知目标，格式：`tgram://bottoken/ChatID1/ChatID2`。
    pub url: String,
    /// 外部密钥文件路径（便于配合 sops-nix，敏感 Token 不写入 config.yaml）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_file: Option<PathBuf>,
    pub api_base: String,
    pub max_images_per_event: usize,
    pub image_bytes_budget: u64,
    pub digest_window_secs: u64,
    /// 变更通知文案模板。支持占位符：{label}、{watch}、{time}、{tz}、{summary}、{items}。
    /// 留空则使用内置默认模板。
    pub template: String,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            url_file: None,
            api_base: "https://api.telegram.org".to_string(),
            max_images_per_event: 10,
            image_bytes_budget: 10485760,
            digest_window_secs: 30,
            template: String::new(),
        }
    }
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

/// 默认全局抓取频率：每天中午 12 点。
pub const DEFAULT_CRON: &str = "0 12 * * *";

/// 默认 User-Agent：模拟 Chrome/Edge 浏览器，避免被站点按 UA 拦截。
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36 Edg/151.0.0.0";

/// 全局可编辑设置的传输与视图模型，用于 Web 控制台与 REST API。
/// 权威数据直接持久化到 `config.yaml`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsView {
    pub concurrency: usize,
    pub queue_capacity: usize,
    pub default_timeout_secs: u64,
    pub default_cron: String,
    pub default_user_agent: String,
    pub history_limit_per_source: usize,
    pub failure_notify_threshold: u32,
    pub timezone: String,
    pub template: String,
    pub telegram_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_url_file: Option<String>,
    pub max_images_per_event: usize,
    pub browser_enabled: bool,
    pub browser_cdp_url: String,
    pub browser_timeout_secs: u64,
    pub readonly: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

pub type EditableSettings = SettingsView;

impl Default for SettingsView {
    fn default() -> Self {
        Self {
            concurrency: 16,
            queue_capacity: 1024,
            default_timeout_secs: 120,
            default_cron: DEFAULT_CRON.to_string(),
            default_user_agent: DEFAULT_USER_AGENT.to_string(),
            history_limit_per_source: 0,
            failure_notify_threshold: 0,
            timezone: system_local_timezone(),
            template: DEFAULT_EVENT_TEMPLATE.to_string(),
            telegram_url: String::new(),
            telegram_url_file: None,
            max_images_per_event: 10,
            browser_enabled: false,
            browser_cdp_url: "http://127.0.0.1:9222".to_string(),
            browser_timeout_secs: 120,
            readonly: false,
            config_path: None,
        }
    }
}

impl SettingsView {
    pub fn from_config(cfg: &Config, config_path: Option<&Path>) -> Self {
        let readonly = config_path.is_some_and(is_path_readonly);
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
            telegram_url: if cfg.telegram.url_file.is_some() {
                String::new()
            } else {
                cfg.telegram.url.clone()
            },
            telegram_url_file: cfg
                .telegram
                .url_file
                .as_ref()
                .map(|p| p.display().to_string()),
            max_images_per_event: cfg.telegram.max_images_per_event,
            browser_enabled: cfg.browser.enabled,
            browser_cdp_url: cfg.browser.cdp_url.clone(),
            browser_timeout_secs: cfg.browser.timeout_secs,
            readonly,
            config_path: config_path.map(|p| p.display().to_string()),
        }
    }

    pub fn apply_to_config(&self, cfg: &mut Config) {
        cfg.daemon.concurrency = self.concurrency.max(1);
        cfg.daemon.queue_capacity = self.queue_capacity.max(1);
        cfg.daemon.default_timeout_secs = self.default_timeout_secs.max(1);
        cfg.daemon.default_cron = self.default_cron.clone();
        cfg.daemon.default_user_agent = self.default_user_agent.clone();
        cfg.daemon.history_limit_per_source = self.history_limit_per_source;
        cfg.daemon.failure_notify_threshold = self.failure_notify_threshold;
        cfg.daemon.timezone = self.timezone.clone();
        if cfg.telegram.url_file.is_none() || !self.telegram_url.trim().is_empty() {
            cfg.telegram.url = self.telegram_url.clone();
        }
        cfg.telegram.max_images_per_event = self.max_images_per_event;
        cfg.telegram.template = self.template.clone();
        cfg.browser.enabled = self.browser_enabled;
        cfg.browser.cdp_url = self.browser_cdp_url.clone();
        cfg.browser.timeout_secs = self.browser_timeout_secs;
    }
}

/// 一个监控源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub id: String,
    pub name: String,
    /// 所属分组 ID（可选）。指定分组后可继承分组的预设规则与通知目标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// 是否启用监控（调度检查）。false 时该源不会被调度器抓取检测。
    pub enabled: bool,
    /// 是否发送变更通知。false 时仍正常监控检测，但检测到的变更不会推送 Telegram 通知。
    #[serde(default = "default_true")]
    pub notify_enabled: bool,
    pub fetch: FetchConfig,
    pub schedule: ScheduleConfig,
    /// 内容提取方式。决定「把抓到的内容变成什么拿来比对」。
    #[serde(default)]
    pub extract: ExtractConfig,
}

/// serde 默认值辅助：返回 `true`。
fn default_true() -> bool {
    true
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            group_id: None,
            enabled: true,
            notify_enabled: true,
            fetch: FetchConfig::default(),
            schedule: ScheduleConfig::default(),
            extract: ExtractConfig::default(),
        }
    }
}

/// 解析监控源的「生效」开关与历史保留条数。
pub fn resolve_effective_source(
    source: &SourceConfig,
    global_history_limit: usize,
) -> (bool, bool, usize) {
    (source.enabled, source.notify_enabled, global_history_limit)
}

/// 解析 `tgram://bottoken/ChatID1/ChatID2` 形式的 Telegram 通知目标。
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
/// 若所属分组配置了 `default_notify_url`，则优先使用分组的通知目标；
/// 否则沿用全局通知目标。返回 `None` 表示全局也未配置可用的通知目标。
pub fn resolve_notify_target(
    _source: &SourceConfig,
    group: Option<&GroupConfig>,
    global_url: &str,
) -> Option<TelegramTarget> {
    if let Some(g) = group
        && !g.default_notify_url.trim().is_empty()
        && let Ok(target) = parse_telegram_url(&g.default_notify_url)
        && target.is_valid()
    {
        return Some(target);
    }
    parse_telegram_url(global_url).ok().filter(|t| t.is_valid())
}

/// 解析监控源的生效内容提取配置。
/// 若监控源未自定义（为默认 Text 且无 selector）且所属分组配置了 `default_extract`，
/// 则使用分组的提取设置；否则使用监控源自身的提取设置。
pub fn resolve_effective_extract(
    source: &SourceConfig,
    group: Option<&GroupConfig>,
) -> ExtractConfig {
    if let Some(g) = group
        && let Some(ref ext) = g.default_extract
    {
        // 如果 source 是默认 Text 提取且无特定配置，则继承 group 的提取规则
        if source.extract == ExtractConfig::default() {
            return ext.clone();
        }
    }
    source.extract.clone()
}

/// 解析监控源的生效 cron 表达式。
pub fn resolve_effective_cron(
    source: &SourceConfig,
    group: Option<&GroupConfig>,
    default_cron: &str,
) -> String {
    if let Some(ref cron) = source.schedule.cron
        && !cron.trim().is_empty()
    {
        return cron.clone();
    }
    if let Some(g) = group
        && let Some(ref cron) = g.default_cron
        && !cron.trim().is_empty()
    {
        return cron.clone();
    }
    if default_cron.trim().is_empty() {
        DEFAULT_CRON.to_string()
    } else {
        default_cron.to_string()
    }
}

/// 解析监控源的生效 HTTP headers。
pub fn resolve_effective_headers(
    source: &SourceConfig,
    group: Option<&GroupConfig>,
) -> HashMap<String, String> {
    if !source.fetch.headers.is_empty() {
        return source.fetch.headers.clone();
    }
    if let Some(g) = group {
        return g.default_headers.clone();
    }
    HashMap::new()
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
            timeout_secs: 120,
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
    /// 文本提取模式：可指定可选 CSS selector；未指定时监控整页纯文本
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<ImageSelector>,
    },
    /// JSON 接口提取模式：可指定可选 json_path（如 $.data.version）；未指定时监控整个格式化 JSON
    Json {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        json_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<ImageSelector>,
    },
    /// 结构化条目列表模式：多条目列表提取，自动比对条目增删改
    Items {
        selector: ItemSelector,
        #[serde(default)]
        fields: Vec<ItemField>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dedupe_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<ImageSelector>,
    },
}

/// 图片通知链路：如何从页面 / 条目中挑选要随变更通知附带的图片。
///
/// - `none`（默认）：不附带图片。
/// - `first_item`：附带条目列表提取到的首张图片。
/// - `css { selector }`：用 CSS 选择器从页面匹配 `<img>`，取其 `src`/`data-src`。
/// - `screenshot`：浏览器引擎检测到变更时全屏/指定元素截图并推送。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSelector {
    #[default]
    None,
    FirstItem,
    Css {
        selector: String,
    },
    Screenshot,
}

impl ExtractConfig {
    pub fn images(&self) -> Option<&ImageSelector> {
        match self {
            ExtractConfig::Text { images, .. } => images.as_ref(),
            ExtractConfig::Json { images, .. } => images.as_ref(),
            ExtractConfig::Items { images, .. } => images.as_ref(),
        }
    }

    /// 若提取规则定义了 CSS 选择器（Text 模式或 Items CSS 模式），返回其修剪后的选择器字符串。
    pub fn css_selector(&self) -> Option<&str> {
        match self {
            ExtractConfig::Text { selector, .. } => {
                selector.as_deref().map(str::trim).filter(|s| !s.is_empty())
            }
            ExtractConfig::Items {
                selector: ItemSelector::Css { selector },
                ..
            } => {
                let trimmed = selector.trim();
                if !trimmed.is_empty() {
                    Some(trimmed)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Default for ExtractConfig {
    fn default() -> Self {
        ExtractConfig::Text {
            selector: None,
            images: None,
        }
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
    /// 由权威 Config 合成运行时配置。
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            state_dir: cfg.state_dir.clone(),
            media_dir: cfg.media_dir.clone(),
            concurrency: cfg.daemon.concurrency.max(1),
            queue_capacity: cfg.daemon.queue_capacity.max(1),
            default_timeout_secs: cfg.daemon.default_timeout_secs.max(1),
            default_cron: cfg.daemon.default_cron.clone(),
            default_user_agent: cfg.daemon.default_user_agent.clone(),
            history_limit_per_source: cfg.daemon.history_limit_per_source,
            failure_notify_threshold: cfg.daemon.failure_notify_threshold,
            timezone: cfg.daemon.timezone.clone(),
            template: cfg.telegram.template.clone(),
        }
    }

    /// 由启动配置（boot）+ 设置合成运行时配置。
    pub fn from_parts(cfg: &Config, _settings: &EditableSettings) -> Self {
        Self::from_config(cfg)
    }
}

/// 返回系统本地时区的 IANA 名称（如 `Asia/Shanghai`、`America/New_York`）。
/// 检测失败时回退到 `UTC`。
pub fn system_local_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}
