use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use tokio::sync::Mutex;

use crate::config::TelegramConfig;
use crate::db::Db;
use crate::error::{Error, Result};
use crate::images::ImageDownloader;
use crate::models::{ChangeEvent, ImageRef, Item, MediaCacheEntry};

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
    /// 服务器展示/告警时区（IANA 名称），用于事件通知里的 {time}/{tz} 占位符。
    timezone: String,
}

impl TelegramNotifier {
    pub fn new(cfg: &TelegramConfig, timezone: &str) -> Result<Self> {
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
            timezone: timezone.to_string(),
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
        let text =
            render_event_message(event, new_items, &self.cfg.event_template(), &self.timezone);
        let max = self.cfg.max_images_per_event.max(1);
        let entries: Vec<_> = image_entries.iter().take(max).collect();

        // 有图片时，把文案作为第一张图片的说明一起发送，避免重复发一条纯文本。
        if entries.len() == 1 {
            let mut ids = Vec::new();
            if let Some(id) = self.send_photo(chat_id, entries[0], &text).await? {
                ids.push(id);
            }
            return Ok(ids);
        }
        if entries.len() > 1 {
            let mut ids = Vec::new();
            if let Some(group_ids) = self.send_media_group(chat_id, &entries, &text).await? {
                ids.extend(group_ids);
            }
            return Ok(ids);
        }

        // 无图片：直接发纯文本。
        Ok(vec![self.send_message(chat_id, &text).await?])
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
        caption: &str,
    ) -> Result<Option<Vec<i64>>> {
        // 只上传缺少 telegram_file_id 的本地文件；已上传过的直接用 file_id。
        let mut media = Vec::new();
        let mut form = Form::new().text("chat_id", chat_id.to_string());
        let mut needs_multipart = false;
        for entry in entries {
            // 用 `media.is_empty()` 判断是否为最终媒体组的首张图：
            // 首图承载文案，避免被 file_id / 缺失文件跳过时文案丢失。
            let is_first = media.is_empty();
            if let Some(fid) = &entry.telegram_file_id {
                if is_first {
                    media.push(json!({
                        "type": "photo",
                        "media": fid,
                        "caption": caption,
                        "parse_mode": "HTML"
                    }));
                } else {
                    media.push(json!({ "type": "photo", "media": fid }));
                }
                continue;
            }
            let path = Path::new(&entry.file_path);
            if !path.exists() {
                continue;
            }
            let bytes = tokio::fs::read(path).await?;
            let attach = format!("photo_{}", entry.sha256);
            let part = Part::bytes(bytes)
                .file_name(
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("image.bin")
                        .to_string(),
                )
                .mime_str(&entry.mime)
                .unwrap_or_else(|_| Part::bytes(Vec::new()));
            form = form.part(attach.clone(), part);
            if is_first {
                media.push(json!({
                    "type": "photo",
                    "media": format!("attach://{attach}"),
                    "caption": caption,
                    "parse_mode": "HTML"
                }));
            } else {
                media.push(json!({ "type": "photo", "media": format!("attach://{attach}") }));
            }
            needs_multipart = true;
        }
        if media.is_empty() {
            return Ok(None);
        }

        if needs_multipart {
            let url = format!("{}/bot{}/sendMediaGroup", self.api_base(), self.token);
            let form = form.text("media", serde_json::to_string(&media)?);
            let resp = self.client.post(&url).multipart(form).send().await?;
            let body: SendMediaGroupResponse = resp.json().await?;
            if !body.ok {
                return Err(Error::Other(
                    "telegram sendMediaGroup returned ok=false".into(),
                ));
            }
            return Ok(Some(
                body.result.into_iter().map(|r| r.message_id).collect(),
            ));
        }

        // 全部是已上传的 file_id：直接用 JSON。
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

/// 渲染一条变更通知文本。支持模板占位符：
/// `{label}` 变化类型、`{watch}` 监控源 ID、`{time}` 检测时间（按配置时区显示）、
/// `{tz}` 服务器时区名、`{summary}` 变更摘要、`{items}` 新增条目预览列表。
pub fn render_event_message(
    event: &ChangeEvent,
    new_items: &[Item],
    template: &str,
    tz: &str,
) -> String {
    let change_label = match event.change_type {
        crate::config::ChangeType::New => "🆕 NEW",
        crate::config::ChangeType::Updated => "✏️ UPDATED",
        crate::config::ChangeType::Removed => "🗑 REMOVED",
    };
    let items = {
        let preview: Vec<&Item> = new_items.iter().take(3).collect();
        if preview.is_empty() {
            String::new()
        } else {
            let mut s = String::new();
            for item in &preview {
                let title = item
                    .fields
                    .get("title")
                    .or_else(|| item.fields.get("name"))
                    .cloned()
                    .unwrap_or_else(|| item.stable_id.clone());
                s.push_str(&format!("• {}\n", html_escape(&title)));
            }
            s
        }
    };
    render_template(
        template,
        &[
            ("{label}", change_label),
            ("{watch}", &event.watchpoint_id),
            (
                "{time}",
                &crate::scheduler::format_local_time(event.detected_at, tz),
            ),
            ("{tz}", tz),
            ("{summary}", &html_escape(&event.diff_summary)),
            ("{items}", items.trim_end()),
        ],
    )
}

/// 渲染一条连续失败告警通知文本（系统级，不关联变更事件）。
pub fn render_failure_message(
    source_id: &str,
    failures: u32,
    threshold: u32,
    error: &str,
    tz: &str,
) -> String {
    let now = Utc::now();
    let t = crate::scheduler::format_local_time(now, tz);
    format!(
        "<b>⚠️ ReadingSteiner 连续失败告警</b>\n监控源 <b>{}</b> 已连续失败 {} 次（阈值 {}）。\n最近错误：<i>{}</i>\n本地时间（{}）：{}",
        html_escape(source_id),
        failures,
        threshold,
        html_escape(error),
        html_escape(tz),
        html_escape(&t)
    )
}

/// 用占位符替换渲染模板。
fn render_template(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in pairs {
        out = out.replace(k, v);
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub async fn process_outbox(
    db: &Mutex<Db>,
    images: &ImageDownloader,
    notifier: &TelegramNotifier,
    chat_id_override: Option<&str>,
) -> Result<usize> {
    let pending = { db.lock().await.pending_notifications(50)? };
    let mut sent = 0usize;
    for notif in pending {
        let (event, new_items) = {
            let db = db.lock().await;
            let Some(event) = db.get_change_event(notif.event_id)? else {
                db.update_notification_status(notif.id, "failed", "[]")?;
                continue;
            };
            let new_items: Vec<Item> =
                serde_json::from_str(&event.new_items_json).unwrap_or_default();
            (event, new_items)
        };
        // 读取事件关联的图片 URL，下载/取缓存后随通知发送。
        let image_urls: Vec<String> =
            serde_json::from_str(&event.image_urls_json).unwrap_or_default();
        let mut entries = Vec::new();
        for url in &image_urls {
            let image_ref = ImageRef {
                canonical_url: url.clone(),
                alt: String::new(),
                width: None,
                height: None,
            };
            // 单个图片下载/解析失败仅跳过该图，不中断整批通知。
            if let Ok(Some(entry)) = images.ensure(db, &image_ref).await {
                entries.push(entry);
            }
        }
        let chat_id = chat_id_override.unwrap_or(&notif.chat_id);
        match notifier
            .send_event(chat_id, &event, &new_items, &entries)
            .await
        {
            Ok(ids) => {
                let ids_json = serde_json::to_string(&ids)?;
                db.lock()
                    .await
                    .update_notification_status(notif.id, "sent", &ids_json)?;
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
                db.lock()
                    .await
                    .mark_notification_retry(notif.id, attempts, next_retry)?;
                if attempts >= 5 {
                    db.lock()
                        .await
                        .update_notification_status(notif.id, "failed", "[]")?;
                }
            }
        }
    }

    // 处理系统级通知（连续失败告警等）。
    let sys_pending = { db.lock().await.pending_system_notifications(20)? };
    for sys in sys_pending {
        let chat_id = chat_id_override.unwrap_or(&sys.chat_id);
        match notifier.send_text(chat_id, &sys.text).await {
            Ok(_) => {
                db.lock()
                    .await
                    .update_system_notification_status(sys.id, "sent")?;
                sent += 1;
            }
            Err(e) => {
                warn!(notification = sys.id, error = %e, "system notification failed");
                let attempts = sys.attempts + 1;
                let next_retry = if attempts >= 5 {
                    None
                } else {
                    Some(Utc::now() + chrono::Duration::seconds(30 * attempts as i64))
                };
                db.lock()
                    .await
                    .mark_system_notification_retry(sys.id, attempts, next_retry)?;
                if attempts >= 5 {
                    db.lock()
                        .await
                        .update_system_notification_status(sys.id, "failed")?;
                }
            }
        }
    }

    Ok(sent)
}
