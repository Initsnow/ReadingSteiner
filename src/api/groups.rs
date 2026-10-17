//! 站点分组领域服务：列表、详情、新增、更新、删除。

use crate::config::GroupConfig;
use crate::error::{Error, Result};
use crate::scheduler::AppState;

/// 列出全部站点分组。
pub async fn groups_list(state: &AppState) -> Result<Vec<GroupConfig>> {
    state.db.lock().await.list_groups()
}

/// 获取单个站点分组。
pub async fn group_get(state: &AppState, id: &str) -> Result<Option<GroupConfig>> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::other("group id is required"));
    }
    state.db.lock().await.get_group(id)
}

/// 新增 / 更新一个站点分组。
pub async fn group_upsert(state: &AppState, mut group: GroupConfig) -> Result<GroupConfig> {
    group.id = group.id.trim().to_string();
    group.name = group.name.trim().to_string();
    if group.id.is_empty() {
        return Err(Error::other("group id is required"));
    }
    if group.name.is_empty() {
        group.name = group.id.clone();
    }
    state.db.lock().await.upsert_group(&group)?;
    Ok(group)
}

/// 删除一个站点分组，返回是否命中。
pub async fn group_delete(state: &AppState, id: &str) -> Result<bool> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::other("group id is required"));
    }
    Ok(state.db.lock().await.delete_group(id)? > 0)
}
