//! 领域服务层：Web 控制台与 CLI 共用的业务逻辑。
//!
//! 这一层是**所有业务操作的唯一实现**，与传输方式无关：
//! - [`crate::web`]（HTTP REST API）把 HTTP handler 转成这里的调用；
//! - [`crate::cli`]（CLI 工具）通过 HTTP 调用 REST API。
//!
//! 所有方法返回领域类型（不是 `serde_json::Value`），由传输层负责序列化。

mod backup;
mod events;
mod groups;
mod settings;
mod sources;

pub use backup::{
    BackupInfo, backup_create, backup_delete, backup_list, backup_restore, backup_restore_upload,
    backup_zip_path,
};
pub use events::{
    event_get, event_mark_read, event_screenshot_file, events_list, events_mark_all_read,
    source_mark_read,
};
pub use groups::{group_delete, group_get, group_upsert, groups_list};
pub use settings::{settings_get, settings_update};
pub use sources::{
    AddedSource, PreviewedTitle, check_source, preview_source_title, source_add, source_delete,
    source_update, sources_list, sources_set_flags, test_source, test_source_dry_run,
};

use crate::error::Result;
use crate::models::{ChangeEvent, DaemonStatus, SourceMeta};
use crate::scheduler::AppState;

/// 列出监控源（含展示用元信息：最近检查 / 最近变更 / 未读数 / 错误）。
pub(crate) async fn list_source_meta(state: &AppState) -> Result<Vec<SourceMeta>> {
    let db = state.db.lock().await;
    let sources = state.sources.lock().await.clone();
    db.list_source_meta(&sources)
}

/// 取 daemon 运行状态快照。
pub(crate) async fn daemon_status(state: &AppState) -> DaemonStatus {
    state.status().await
}

/// 取单个变更事件。
pub(crate) async fn get_event(state: &AppState, id: i64) -> Result<Option<ChangeEvent>> {
    state.db.lock().await.get_change_event(id)
}
