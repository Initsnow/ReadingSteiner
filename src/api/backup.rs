//! 备份与恢复领域服务：在线备份、列表、zip 下载、在线恢复、删除。

use std::path::{Path, PathBuf};

use crate::backup;
use crate::config::SourceConfig;
use crate::error::{Error, Result};
use crate::scheduler::AppState;

/// 一条备份记录。
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupInfo {
    pub name: String,
    pub path: String,
    pub has_zip: bool,
}

/// 创建一次在线备份（DB 一致性快照 + media + config）。
///
/// 仅在取快照期间持有 DB 锁；zip 打包在释放锁之后进行，避免大库打包阻塞 daemon。
pub async fn backup_create(state: &AppState) -> Result<BackupInfo> {
    let dir = {
        let db = state.db.lock().await;
        backup::backup(db.connection(), &state.cfg, state.config_path.as_deref())?
    };
    // 释放锁后再打包（阻塞 I/O 走 spawn_blocking，不占着 async 执行器）。
    let dir_for_zip = dir.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = backup::pack_backup_zip(&dir_for_zip) {
            tracing::warn!(error = %e, "zip packing failed for backup");
        }
    })
    .await
    .ok();
    Ok(describe(&dir))
}

/// 列出全部备份。
pub fn backup_list(state: &AppState) -> Result<Vec<BackupInfo>> {
    Ok(backup::list_backups(&state.state_dir())?
        .into_iter()
        .map(|b| BackupInfo {
            name: b.name,
            path: b.path.display().to_string(),
            has_zip: b.has_zip,
        })
        .collect())
}

/// 在线恢复：把备份库写进实时连接，并刷新内存中的监控源。
pub async fn backup_restore(state: &AppState, name: &str) -> Result<()> {
    if !backup::is_valid_backup_name(name) {
        return Err(Error::other("invalid backup name"));
    }
    let dir = backup_dir(state, name)?;
    let mut db = state.db.lock().await;
    backup::restore(&dir, &state.cfg, Some(db.connection_mut()))?;
    // 先同步读库（Db 不同步，不能把 &Db 带到 await 点之后），再写回内存。
    let sources = load_sources(&db);
    drop(db);
    *state.sources.lock().await = sources;
    Ok(())
}

/// 从上传的 zip 备份在线恢复。
///
/// 先无锁解压（不涉及数据库），再单独持锁恢复，把 DB 锁占用时间压到最小。
pub async fn backup_restore_upload(state: &AppState, path: &Path) -> Result<String> {
    let file =
        std::fs::File::open(path).map_err(|e| Error::other(format!("无法读取上传文件: {e}")))?;
    let restored_dir = match backup::extract_zip(file, &state.cfg) {
        Ok(dir) => dir,
        Err(e) => {
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
    };

    let result: Result<()> = async {
        let mut db = state.db.lock().await;
        backup::restore(&restored_dir, &state.cfg, Some(db.connection_mut()))?;
        let sources = load_sources(&db);
        drop(db);
        *state.sources.lock().await = sources;
        Ok(())
    }
    .await;

    if let Err(e) = result {
        // 恢复失败：清理解压残留，避免留下没有 zip 的“半成品”备份。
        let _ = backup::cleanup_backup_dir(&restored_dir);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    // 补打一个 zip，便于与其它备份一致地下载 / 管理；失败不影响恢复结果。
    let _ = backup::pack_backup_zip(&restored_dir);
    let _ = std::fs::remove_file(path);
    Ok(dir_name(&restored_dir))
}

/// 定位备份的 zip 文件；zip 不存在时现场打包。
pub fn backup_zip_path(state: &AppState, name: &str) -> Result<PathBuf> {
    if !backup::is_valid_backup_name(name) {
        return Err(Error::other("invalid backup name"));
    }
    let dir = backup_dir(state, name)?;
    let zip_path = state
        .state_dir()
        .join(backup::BACKUP_SUBDIR)
        .join(format!("{name}.zip"));
    if !zip_path.exists() {
        backup::pack_zip(&dir, &zip_path)?;
    }
    Ok(zip_path)
}

/// 删除一个备份（目录 + zip）。返回是否命中。
pub fn backup_delete(state: &AppState, name: &str) -> Result<bool> {
    if !backup::is_valid_backup_name(name) {
        return Err(Error::other("invalid backup name"));
    }
    backup::delete_backup(&state.state_dir(), name)
}

fn backup_dir(state: &AppState, name: &str) -> Result<PathBuf> {
    let dir = state.state_dir().join(backup::BACKUP_SUBDIR).join(name);
    if !dir.join(backup::DB_FILE_NAME).exists() {
        return Err(Error::other(format!("backup {name} not found")));
    }
    Ok(dir)
}

fn describe(dir: &Path) -> BackupInfo {
    let name = dir_name(dir);
    BackupInfo {
        has_zip: dir.with_extension("zip").exists(),
        name,
        path: dir.display().to_string(),
    }
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string()
}

/// 从数据库读出全部监控源（同步执行，避免把 `&Db` 带到 await 点之后）。
fn load_sources(db: &crate::db::Db) -> Vec<SourceConfig> {
    db.list_sources().unwrap_or_default()
}
