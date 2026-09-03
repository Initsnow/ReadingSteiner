//! 监控源领域服务：列出、新增、更新、删除、批量改开关、立即检测、测试、预览。

use serde_json::Value;

use std::sync::Arc;

use crate::api::list_source_meta;
use crate::config::{self, SourceConfig};
use crate::error::{Error, Result};
use crate::fetcher::{FetchSpec, create_fetcher};
use crate::models::SourceMeta;
use crate::scheduler::{self, AppState};

/// 单次批量操作允许的最大监控源数量，避免持锁期间长时间执行 upsert 阻塞其他请求。
const MAX_BATCH_SIZE: usize = 100;

/// 列出全部监控源（含展示元信息）。
pub async fn sources_list(state: &AppState) -> Result<Vec<SourceMeta>> {
    list_source_meta(state).await
}

/// 新增监控源。ID / 名称留空时自动生成。
pub async fn source_add(state: &AppState, mut source: SourceConfig) -> Result<AddedSource> {
    // 存在性检查、DB 写入、内存 push 放在同一次 sources 锁内完成，避免并发添加
    // 相同 id 时因检查与 push 分处两次锁而产生重复条目（TOCTOU）。
    // 锁获取顺序统一为 db → sources，与调度器主循环保持一致以避免死锁。
    let db = state.db.lock().await;
    let mut sources = state.sources.lock().await;

    if source.id.trim().is_empty() {
        source.id = config::generate_source_id(&source.name, &source.fetch.url);
    }
    if source.name.trim().is_empty() {
        source.name = hostname_of(&source.fetch.url);
    }
    if sources.iter().any(|s| s.id == source.id) {
        return Err(Error::other(format!("source {} already exists", source.id)));
    }
    // 自动登记新标签到分组表，使分组出现在「分组管理」中供配置。
    db.ensure_tags(&source.tags)?;
    db.upsert_source(&source)?;
    sources.push(source.clone());
    Ok(AddedSource {
        source_id: source.id,
    })
}

/// 更新监控源。ID 不存在时报错（避免 upsert 静默插入新行）。
pub async fn source_update(state: &AppState, source: SourceConfig) -> Result<()> {
    let db = state.db.lock().await;
    let mut sources = state.sources.lock().await;
    if !sources.iter().any(|s| s.id == source.id) {
        return Err(Error::other(format!("source {} not found", source.id)));
    }
    db.ensure_tags(&source.tags)?;
    db.upsert_source(&source)?;
    if let Some(slot) = sources.iter_mut().find(|s| s.id == source.id) {
        *slot = source;
    }
    Ok(())
}

/// 删除监控源（级联清理其快照 / 事件 / 调度状态）。
pub async fn source_delete(state: &AppState, source_id: &str) -> Result<()> {
    let db = state.db.lock().await;
    let mut sources = state.sources.lock().await;
    db.delete_source(source_id)?;
    sources.retain(|s| s.id != source_id);
    Ok(())
}

/// 批量设置多个监控源的监控开关 / 通知开关。两个开关均可选，缺省表示不修改。
pub async fn sources_set_flags(
    state: &AppState,
    source_ids: &[String],
    enabled: Option<bool>,
    notify_enabled: Option<bool>,
) -> Result<usize> {
    // 空列表或两个开关均未指定时直接返回（幂等，避免前端空选误报错）。
    if source_ids.is_empty() || (enabled.is_none() && notify_enabled.is_none()) {
        return Ok(0);
    }
    if source_ids.len() > MAX_BATCH_SIZE {
        return Err(Error::other(format!(
            "batch size {} exceeds limit {MAX_BATCH_SIZE}",
            source_ids.len()
        )));
    }
    let db = state.db.lock().await;
    let mut sources = state.sources.lock().await;

    // 先收集全部修改后的快照并统一写库：中途失败则不改内存，保持内存与 DB 一致。
    let mut updated = Vec::with_capacity(source_ids.len());
    for id in source_ids {
        let Some(current) = sources.iter().find(|s| s.id == *id) else {
            continue;
        };
        let mut next = current.clone();
        if let Some(v) = enabled {
            next.enabled = v;
        }
        if let Some(v) = notify_enabled {
            next.notify_enabled = v;
        }
        db.upsert_source(&next)
            .map_err(|e| Error::other(format!("failed to update {id}: {e}")))?;
        updated.push(next);
    }
    let count = updated.len();
    for next in &updated {
        if let Some(slot) = sources.iter_mut().find(|s| s.id == next.id) {
            *slot = next.clone();
        }
    }
    Ok(count)
}

/// 立即检测一次（走完整抓取 + 比对 + 落库流程）。
pub async fn check_source(state: &Arc<AppState>, source_id: &str) -> Result<()> {
    scheduler::check_source(state, source_id).await
}

/// 测试监控源：抓取并按配置提取，返回摘要，**不落库**（不写快照 / 不产生事件）。
pub async fn test_source(state: &Arc<AppState>, source_id: &str) -> Result<Value> {
    scheduler::test_source(state, source_id).await
}

/// 预览 URL 标题：抓取页面并提取 `<title>` / `<h1>` / JSON 的 title|name 字段。
/// 用于添加监控源时自动填充名称。
pub async fn preview_source_title(
    state: &AppState,
    url: &str,
    engine: &str,
) -> Result<PreviewedTitle> {
    let url = url.trim();
    // SSRF 防护：仅允许 http/https 的公网地址。
    crate::net_guard::assert_public_http_url(url)?;
    let engine = if engine.is_empty() { "http" } else { engine };
    let fetch = config::FetchConfig {
        engine: engine.to_string(),
        url: url.to_string(),
        ..Default::default()
    };
    let fetcher = create_fetcher(engine, &state.cfg, &state.settings_snapshot())?;
    let doc = fetcher
        .fetch(&FetchSpec {
            fetch,
            etag: None,
            last_modified: None,
            source_id: String::new(),
        })
        .await?;

    Ok(PreviewedTitle {
        url: url.to_string(),
        title: extract_title(&doc.text, doc.content_type.as_deref()),
    })
}

/// 从响应正文提取标题：HTML 取 `<title>` / `<h1>`，JSON 取 title|name 字段。
fn extract_title(text: &str, content_type: Option<&str>) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let is_json = content_type.map(|ct| ct.contains("json")).unwrap_or(false);
    if is_json {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return String::new();
        };
        for key in ["title", "name", "Title", "Name"] {
            if let Some(s) = value.get(key).and_then(|v| v.as_str()) {
                let s = s.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
        return String::new();
    }
    let html = scraper::Html::parse_document(text);
    for selector in ["title", "h1"] {
        if let Ok(sel) = scraper::Selector::parse(selector)
            && let Some(el) = html.select(&sel).next()
        {
            let title = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
            if !title.is_empty() {
                return title;
            }
        }
    }
    String::new()
}

/// 取 URL 的主机名，用于生成可读名称。
fn hostname_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

/// 新增监控源的返回值。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AddedSource {
    pub source_id: String,
}

/// 预览 URL 的结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewedTitle {
    pub url: String,
    pub title: String,
}
