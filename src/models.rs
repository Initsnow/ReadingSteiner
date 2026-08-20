use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::ChangeType;

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
}
