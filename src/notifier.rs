use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use crate::config::TelegramConfig;
use crate::db::Db;
use crate::error::{Error, Result};
use crate::models::{ChangeEvent, Item, MediaCacheEntry};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SendMessageResponse {
    ok: bool,
    #[serde(default)]
    result: Option<MessageResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessageResult {
    #[serde(rename = "message_id")]
    message_id: i64,
}

pub struct TelegramNotifier {
    client: Client,
    token: String,
    cfg: TelegramConfig,
}

impl TelegramNotifier {
    pub fn new(cfg: &TelegramConfig) -> Result<Self> {
        let token = if !cfg.token.is_empty() {
            cfg.token.clone()
        } else if !cfg.token_file.as_os_str().is_empty() && Path::new(&cfg.token_file).exists() {
            std::fs::read_to_string(&cfg.token_file)?.trim().to_string()
        } else {
            return Err(Error::config("telegram token or token_file is required"));
        };
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            token,
            cfg: cfg.clone(),
        })
    }

    pub fn default_chat_id(&self) -> &str {
        if self.cfg.default_chat_id.is_empty() {
            ""
        } else {
            &self.cfg.default_chat_id
        }
    }

    fn api_base(&self) -> &str {
        if self.cfg.api_base.is_empty() {
            "https://api.telegram.org"
        } else {
            &self.cfg.api_base
        }
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<i64> {
        let url = format!("{}/bot{}/sendMessage", self.api_base(), self.token);
        let resp = self
            .client
            .post(&url)
            .json(&json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            }))
            .send()
            .await?;
        let body: SendMessageResponse = resp.json().await?;
        if !body.ok {
            return Err(Error::Other(
                "telegram sendMessage returned ok=false".into(),
            ));
        }
        body.result
            .map(|r| r.message_id)
            .ok_or_else(|| Error::Other("telegram sendMessage missing message_id".into()))
    }

    pub async fn send_text(&self, chat_id: &str, text: &str) -> Result<i64> {
        self.send_message(chat_id, text).await
    }

    pub async fn send_test(&self, chat_id: Option<&str>) -> Result<i64> {
        let chat = chat_id.unwrap_or_else(|| self.default_chat_id());
        if chat.is_empty() {
            return Err(Error::config(
                "no chat id provided and default_chat_id is empty",
            ));
        }
        self.send_message(chat, "<b>ReadingSteiner</b> test notification ✅")
            .await
    }

    pub async fn send_event(
        &self,
        chat_id: &str,
        event: &ChangeEvent,
        new_items: &[Item],
        image_entries: &[MediaCacheEntry],
    ) -> Result<Vec<i64>> {
        let text = render_event_message(event, new_items);
        let mut ids = vec![self.send_message(chat_id, &text).await?];

        if !image_entries.is_empty() {
            let max = self.cfg.max_images_per_event.max(1);
            let entries: Vec<_> = image_entries.iter().take(max).collect();
            if entries.len() == 1 {
                if let Some(id) = self.send_photo(chat_id, entries[0], &text).await? {
                    ids.push(id);
                }
            } else if entries.len() > 1
                && let Some(group_ids) = self.send_media_group(chat_id, &entries).await?
            {
                ids.extend(group_ids);
            }
        }
        Ok(ids)
    }

    async fn send_photo(
        &self,
        chat_id: &str,
        entry: &MediaCacheEntry,
        caption: &str,
    ) -> Result<Option<i64>> {
        let file_id = entry.telegram_file_id.clone();
        let body = if let Some(fid) = file_id {
            json!({
                "chat_id": chat_id,
                "photo": fid,
                "caption": caption,
                "parse_mode": "HTML",
            })
        } else {
            // Use multipart upload with local file.
            let path = Path::new(&entry.file_path);
            if !path.exists() {
                warn!(path = %entry.file_path, "image file missing; skipping photo");
                return Ok(None);
            }
            let bytes = tokio::fs::read(path).await?;
            let part = Part::bytes(bytes)
                .file_name(
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("image.bin")
                        .to_string(),
                )
                .mime_str(&entry.mime)
                .unwrap_or_else(|_| Part::bytes(Vec::new()));
            let form = Form::new()
                .text("chat_id", chat_id.to_string())
                .text("caption", caption.to_string())
                .text("parse_mode", "HTML")
                .part("photo", part);
            let url = format!("{}/bot{}/sendPhoto", self.api_base(), self.token);
            let resp = self.client.post(&url).multipart(form).send().await?;
            let body: SendMessageResponse = resp.json().await?;
            if !body.ok {
                return Err(Error::Other("telegram sendPhoto returned ok=false".into()));
            }
            return Ok(body.result.map(|r| r.message_id));
        };

        let url = format!("{}/bot{}/sendPhoto", self.api_base(), self.token);
        let resp = self.client.post(&url).json(&body).send().await?;
        let resp: SendMessageResponse = resp.json().await?;
        if !resp.ok {
            return Err(Error::Other("telegram sendPhoto returned ok=false".into()));
        }
        Ok(resp.result.map(|r| r.message_id))
    }

    async fn send_media_group(
        &self,
        chat_id: &str,
        entries: &[&MediaCacheEntry],
    ) -> Result<Option<Vec<i64>>> {
        let mut media = Vec::new();
        for entry in entries {
            let item = if let Some(fid) = &entry.telegram_file_id {
                json!({
                    "type": "photo",
                    "media": fid,
                })
            } else {
                let path = Path::new(&entry.file_path);
                if !path.exists() {
                    continue;
                }
                let bytes = tokio::fs::read(path).await?;
                let part = Part::bytes(bytes)
                    .file_name(
                        path.file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("image.bin")
                            .to_string(),
                    )
                    .mime_str(&entry.mime)
                    .unwrap_or_else(|_| Part::bytes(Vec::new()));
                let form = Form::new()
                    .text("chat_id", chat_id.to_string())
                    .text(
                        "media",
                        serde_json::to_string(&media).unwrap_or_else(|_| "[]".to_string()),
                    )
                    .part(format!("photo_{}", entry.sha256), part);
                // This branch is intentionally not used for multipart groups in v1;
                // fall back to individual sends below.
                let url = format!("{}/bot{}/sendMediaGroup", self.api_base(), self.token);
                let resp = self.client.post(&url).multipart(form).send().await?;
                let body: SendMediaGroupResponse = resp.json().await?;
                if body.ok {
                    return Ok(Some(
                        body.result.into_iter().map(|r| r.message_id).collect(),
                    ));
                }
                return Err(Error::Other(
                    "telegram sendMediaGroup returned ok=false".into(),
                ));
            };
            media.push(item);
        }
        if media.is_empty() {
            return Ok(None);
        }
        let url = format!("{}/bot{}/sendMediaGroup", self.api_base(), self.token);
        let resp = self
            .client
            .post(&url)
            .json(&json!({
                "chat_id": chat_id,
                "media": media,
            }))
            .send()
            .await?;
        let body: SendMediaGroupResponse = resp.json().await?;
        if !body.ok {
            return Err(Error::Other(
                "telegram sendMediaGroup returned ok=false".into(),
            ));
        }
        Ok(Some(
            body.result.into_iter().map(|r| r.message_id).collect(),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct SendMediaGroupResponse {
    ok: bool,
    #[serde(default)]
    result: Vec<MessageResult>,
}

pub fn render_event_message(event: &ChangeEvent, new_items: &[Item]) -> String {
    let change_label = match event.change_type {
        crate::config::ChangeType::New => "🆕 NEW",
        crate::config::ChangeType::Updated => "✏️ UPDATED",
        crate::config::ChangeType::Removed => "🗑 REMOVED",
    };
    let mut text = format!(
        "<b>ReadingSteiner</b> — {change_label}\n<b>{}</b>\n<i>{}</i>\n{}",
        event.watchpoint_id,
        event.detected_at.format("%Y-%m-%d %H:%M:%S UTC"),
        html_escape(&event.diff_summary)
    );
    let preview: Vec<&Item> = new_items.iter().take(3).collect();
    for item in preview {
        let title = item
            .fields
            .get("title")
            .or_else(|| item.fields.get("name"))
            .cloned()
            .unwrap_or_else(|| item.stable_id.clone());
        text.push_str(&format!("\n• {}", html_escape(&title)));
    }
    text
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub async fn process_outbox(
    db: &Db,
    notifier: &TelegramNotifier,
    chat_id_override: Option<&str>,
) -> Result<usize> {
    let pending = db.pending_notifications(50)?;
    let mut sent = 0usize;
    for notif in pending {
        let Some(event) = db.get_change_event(notif.event_id)? else {
            db.update_notification_status(notif.id, "failed", "[]")?;
            continue;
        };
        let new_items: Vec<Item> = serde_json::from_str(&event.new_items_json).unwrap_or_default();
        let chat_id = chat_id_override.unwrap_or(&notif.chat_id);
        match notifier.send_event(chat_id, &event, &new_items, &[]).await {
            Ok(ids) => {
                let ids_json = serde_json::to_string(&ids)?;
                db.update_notification_status(notif.id, "sent", &ids_json)?;
                sent += 1;
            }
            Err(e) => {
                warn!(notification = notif.id, error = %e, "notification failed");
                let attempts = notif.attempts + 1;
                let next_retry = if attempts >= 5 {
                    None
                } else {
                    Some(Utc::now() + chrono::Duration::seconds(30 * attempts as i64))
                };
                db.mark_notification_retry(notif.id, attempts, next_retry)?;
                if attempts >= 5 {
                    db.update_notification_status(notif.id, "failed", "[]")?;
                }
            }
        }
    }
    Ok(sent)
}
