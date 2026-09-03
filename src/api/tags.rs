//! 分组（标签）领域服务：列表、新增 / 更新、删除。

use crate::error::{Error, Result};
use crate::models::TagConfig;
use crate::scheduler::AppState;

/// 列出全部分组设置。
pub async fn tags_list(state: &AppState) -> Result<Vec<TagConfig>> {
    state.db.lock().await.list_tags()
}

/// 新增 / 更新一个分组设置（按名称 upsert）。
pub async fn tag_update(state: &AppState, mut tag: TagConfig) -> Result<()> {
    tag.name = tag.name.trim().to_string();
    if tag.name.is_empty() {
        return Err(Error::other("tag name is required"));
    }
    state.db.lock().await.upsert_tag(&tag)
}

/// 删除一个分组设置，返回是否命中。
pub async fn tag_delete(state: &AppState, name: &str) -> Result<bool> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::other("tag name is required"));
    }
    Ok(state.db.lock().await.delete_tag(name)? > 0)
}
