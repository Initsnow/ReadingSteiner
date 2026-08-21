use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{ChangeType, SourceConfig};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Item {
    pub stable_id: String,
    #[serde(default)]
    pub fields: HashMap<String, String>,
    #[serde(default)]
    pub image_urls: Vec<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub meta: HashMap<String, serde_json::Value>,
}

impl Item {
    pub fn fingerprint(&self, ignore_fields: &[String]) -> String {
        let mut parts = Vec::new();
        parts.push(self.stable_id.clone());
        let mut keys: Vec<&String> = self.fields.keys().collect();
        keys.sort();
        for k in keys {
            // 只做精确匹配，避免 `price` 误伤 `price2` 之类的前缀碰撞。
            if ignore_fields.iter().any(|ig| ig == k) {
                continue;
            }
            parts.push(format!(
                "{k}={}",
                self.fields.get(k).unwrap_or(&String::new())
            ));
        }
        if self.fields.is_empty() && !self.text.is_empty() {
            parts.push(format!("text={}", self.text));
        }
        let mut imgs = self.image_urls.clone();
        imgs.sort();
        parts.push(imgs.join(","));
        parts.join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FetchedDocument {
    pub final_url: String,
    pub status: u16,
    pub text: String,
    pub html: Option<String>,
    #[serde(default)]
    pub images: Vec<ImageRef>,
    pub screenshot: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_sha256: String,
    pub normalized_fingerprint: String,
    pub duration_ms: u64,
    pub engine: String,
    /// 响应头 Content-Type（用于判断内容类型，如 text/html、application/json）。
    pub content_type: Option<String>,
    pub not_modified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ImageRef {
    pub canonical_url: String,
    #[serde(default)]
    pub alt: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub id: i64,
    pub watchpoint_id: String,
    pub fetched_at: DateTime<Utc>,
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_sha256: String,
    pub normalized_fingerprint: String,
    pub items_json: String,
    pub duration_ms: u64,
    pub engine: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub id: i64,
    pub watchpoint_id: String,
    pub change_type: ChangeType,
    pub old_items_json: String,
    pub new_items_json: String,
    pub diff_summary: String,
    pub fingerprint: String,
    pub dedupe_key: String,
    /// 本次变更要随通知附带的图片 URL（JSON 数组，供 notifier 读取并发送）。
    pub image_urls_json: String,
    pub detected_at: DateTime<Utc>,
    /// 是否已读（Web 控制台标记）。
    #[serde(default)]
    pub read: bool,
    /// camofox 截图文件路径（相对 media_dir），供 Web 控制台展示。
    #[serde(default)]
    pub screenshot_path: Option<String>,
}

/// 监控源列表项：在 SourceConfig 基础上附加展示用元信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMeta {
    #[serde(flatten)]
    pub source: SourceConfig,
    /// 最近一次检查时间（成功或失败的抓取都算）。
    pub last_check_at: Option<DateTime<Utc>>,
    /// 最近一次检测到变更的时间。
    pub last_change_at: Option<DateTime<Utc>>,
    /// 未读变更事件数。
    pub unread_count: u32,
    /// 是否处于错误状态（连续失败次数 > 0）。
    pub has_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleState {
    pub source_id: String,
    pub next_due_at: DateTime<Utc>,
    pub consecutive_failures: u32,
    pub consecutive_changes: u32,
    pub backoff_until: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_notified_fingerprint: Option<String>,
    pub last_notified_at: Option<DateTime<Utc>>,
    /// 当前失败连击是否已发送过失败通知（达到 failure_notify_threshold 后置真，
    /// 成功后清零），用于避免同一段失败连击反复通知。
    pub failure_notified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaCacheEntry {
    pub canonical_url: String,
    pub sha256: String,
    pub mime: String,
    pub size: i64,
    pub file_path: String,
    pub telegram_file_id: Option<String>,
    pub phash: Option<String>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRecord {
    pub id: i64,
    pub event_id: i64,
    pub chat_id: String,
    pub message_ids_json: String,
    pub status: String,
    pub attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
}

/// 分组（标签）级设置。分组下未单独覆盖的监控源会继承这里的配置。
/// `enabled` 控制分组内监控源是否被调度检查，`notify_enabled` 控制是否推送
/// 通知，`history_limit` 控制该分组内每个监控源最多保留的变更历史条数
/// （0 表示不限制，跟随全局）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TagConfig {
    pub name: String,
    /// 是否启用分组内监控（调度检查）。true 时分组内监控源正常检查。
    pub enabled: bool,
    /// 是否发送分组内监控源的变更通知。
    pub notify_enabled: bool,
    /// 该分组下每个监控源最多保留的变更历史条数（0 表示不限制，使用全局设置）。
    pub history_limit: usize,
}

impl Default for TagConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            notify_enabled: true,
            history_limit: 0,
        }
    }
}

/// 系统级通知（连续失败告警等），不关联某个变更事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemNotification {
    pub id: i64,
    pub chat_id: String,
    pub text: String,
    pub status: String,
    pub attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub changed: bool,
    pub change_type: Option<ChangeType>,
    pub diff_summary: String,
    pub fingerprint: String,
    pub old_items: Vec<Item>,
    pub new_items: Vec<Item>,
    pub dedupe_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub version: String,
    pub sources: usize,
    pub enabled_sources: usize,
    pub queue_depth: usize,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub engine_health: HashMap<String, bool>,
    /// 服务器本地时区（IANA 名称）。
    pub timezone: String,
    /// 服务器当前 UTC 时间。
    pub server_time_utc: DateTime<Utc>,
    /// 服务器本地时间（按配置时区换算）。
    pub server_time_local: String,
}
