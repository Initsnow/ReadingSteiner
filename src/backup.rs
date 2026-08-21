//! 备份与恢复。
//!
//! 备份：将 SQLite 数据库（一致性快照）、media 目录与 config 副本归档到
//! `state/backups/<timestamp>/` 目录下，便于整体迁移或灾难恢复。
//! 恢复：从指定备份目录把数据库与 media 复制回 state / media 目录。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;

use crate::config::Config;
use crate::error::{Error, Result};

/// 备份输出目录名（相对 state_dir）。
const BACKUP_SUBDIR: &str = "backups";

/// 执行一次完整备份，返回备份目录路径。
///
/// `db_conn` 为当前运行中的数据库连接，通过 SQLite 在线备份得到一致快照，
/// 避免直接复制 WAL 状态下正在写入的 db 文件导致损坏。
pub fn backup(db_conn: &Connection, cfg: &Config, config_path: Option<&Path>) -> Result<PathBuf> {
    let state_dir = cfg.state_dir.clone();
    let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let backup_dir = state_dir.join(BACKUP_SUBDIR).join(&ts);
    fs::create_dir_all(&backup_dir)?;

    // 1) 数据库一致性快照（在线备份到独立文件，避免 WAL 竞态损坏）。
    let db_backup = backup_dir.join("reading-steiner.db");
    db_conn.backup(rusqlite::DatabaseName::Main, &db_backup, None)?;
    // 收敛为单文件（清理可能残留的 -wal / -shm），便于整目录归档。
    if let Ok(conn) = Connection::open(&db_backup) {
        let _ = conn.pragma_update(None, "journal_mode", "DELETE");
    }

    // 2) media 目录副本。
    let media_dir = cfg.media_dir.clone();
    if media_dir.exists() {
        let dest = backup_dir.join("media");
        fs::create_dir_all(&dest)?;
        copy_dir(&media_dir, &dest)?;
    }

    // 3) config 副本（若提供了 config 路径且存在）。
    if let Some(p) = config_path
        && p.exists()
    {
        fs::copy(p, backup_dir.join("config.yaml"))?;
    }

    // 记录备份元信息。
    let meta = serde_json::json!({
        "created_at": Utc::now().to_rfc3339(),
        "db": "reading-steiner.db",
        "media": media_dir.exists(),
        "config": config_path.map(|p| p.display().to_string()),
    });
    fs::write(
        backup_dir.join("backup.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    Ok(backup_dir)
}

/// 列出已有备份目录（按名称倒序，最新的在前）。
pub fn list_backups(state_dir: &Path) -> Result<Vec<String>> {
    let dir = state_dir.join(BACKUP_SUBDIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("reading-steiner.db").exists() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    names.reverse();
    Ok(names)
}

/// 从指定备份目录恢复数据库与 media。
/// 注意：恢复会覆盖当前 state 目录中的数据库与 media，建议在 daemon 停止时执行。
pub fn restore(backup_dir: &Path, cfg: &Config) -> Result<()> {
    let db_src = backup_dir.join("reading-steiner.db");
    if !db_src.exists() {
        return Err(Error::other(format!(
            "backup {} has no reading-steiner.db",
            backup_dir.display()
        )));
    }
    // 数据库回拷。
    fs::create_dir_all(&cfg.state_dir)?;
    fs::copy(&db_src, cfg.state_dir.join("reading-steiner.db"))?;

    // media 回拷。
    let media_src = backup_dir.join("media");
    if media_src.exists() {
        fs::create_dir_all(&cfg.media_dir)?;
        clear_dir_contents(&cfg.media_dir)?;
        copy_dir(&media_src, &cfg.media_dir)?;
    }
    Ok(())
}

/// 递归复制目录内容（保持文件与目录结构）。
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let file_type = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// 清空目录内容（保留目录本身）。
fn clear_dir_contents(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// 供 CLI/控制台在未持有运行中连接时使用：直接打开数据库做在线备份。
pub fn backup_from_path(cfg: &Config, config_path: Option<&Path>) -> Result<PathBuf> {
    let db_path = cfg.state_dir.join("reading-steiner.db");
    if !db_path.exists() {
        return Err(Error::other("database not found, nothing to back up"));
    }
    let conn = Connection::open(&db_path)?;
    let _ = conn.pragma_update(
        None,
        "busy_timeout",
        Duration::from_secs(5).as_millis() as i64,
    );
    let dir = backup(&conn, cfg, config_path)?;
    Ok(dir)
}
