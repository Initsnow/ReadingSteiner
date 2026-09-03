//! 变更事件领域服务：列表、详情、标记已读、截图文件定位。

use std::path::PathBuf;

use crate::api::get_event;
use crate::error::{Error, Result};
use crate::models::ChangeEvent;
use crate::scheduler::AppState;

/// 列出变更事件。`source_id` 为 `None` 时列出全部。
pub async fn events_list(
    state: &AppState,
    source_id: Option<&str>,
    limit: usize,
) -> Result<Vec<ChangeEvent>> {
    state.db.lock().await.list_change_events(source_id, limit)
}

/// 取单个变更事件。
pub async fn event_get(state: &AppState, event_id: i64) -> Result<Option<ChangeEvent>> {
    get_event(state, event_id).await
}

/// 标记单个事件为已读，返回受影响行数。
pub async fn event_mark_read(state: &AppState, event_id: i64) -> Result<usize> {
    state.db.lock().await.mark_event_read(event_id)
}

/// 标记某个监控源的全部事件为已读，返回受影响行数。
pub async fn source_mark_read(state: &AppState, source_id: &str) -> Result<usize> {
    state.db.lock().await.mark_source_events_read(source_id)
}

/// 定位事件截图的磁盘绝对路径。
///
/// 截图路径在数据库中以 media_dir 相对路径存储，这里做三重校验后拼接：
/// 拒绝绝对路径与 `..` 段、canonicalize 后确认仍在 media_dir 内，
/// 防止越权读取 media_dir 之外的文件。
pub async fn event_screenshot_file(state: &AppState, event_id: i64) -> Result<PathBuf> {
    let event = event_get(state, event_id)
        .await?
        .ok_or_else(|| Error::other(format!("event {event_id} not found")))?;
    let Some(rel) = event.screenshot_path else {
        return Err(Error::other("event has no screenshot"));
    };
    resolve_media_path(state, &rel)
}

/// 校验一个 media_dir 内的相对路径，返回其规范化的绝对路径。
pub fn resolve_media_path(state: &AppState, rel: &str) -> Result<PathBuf> {
    // 纯字符串校验先行：即使文件被替换/移除，也绝不拼出越界路径。
    if rel.starts_with('/')
        || rel.starts_with('\\')
        || rel
            .split(['/', '\\'])
            .any(|seg| seg.is_empty() || seg == "..")
    {
        return Err(Error::other("invalid media path"));
    }
    let media_dir = state.media_dir();
    let candidate = media_dir.join(rel);
    // canonicalize 失败（文件不存在等）直接拒绝，不再回落到未规范化的路径。
    let canonical = candidate
        .canonicalize()
        .map_err(|_| Error::other("media file not found"))?;
    let media_canonical = media_dir
        .canonicalize()
        .unwrap_or_else(|_| media_dir.clone());
    if !canonical.starts_with(&media_canonical) {
        return Err(Error::other("invalid media path"));
    }
    Ok(canonical)
}
