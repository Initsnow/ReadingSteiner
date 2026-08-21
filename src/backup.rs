//! 备份与恢复。
//!
//! 备份：将 SQLite 数据库（一致性快照）、media 目录与 config 副本归档到
//! `state/backups/<timestamp>/` 目录下，并额外打包成一个 `.zip` 供 Web 控制台
//! 下载，便于整体迁移或灾难恢复。
//! 恢复：从指定备份把数据库与 media 复制回 state / media 目录。支持在线恢复
//! （daemon 运行时通过 SQLite 备份接口写入实时连接），无需停止 daemon。

use std::fs;
use std::io::Write;
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

/// 把备份目录打包成 `<name>.zip`，返回 zip 路径。
///
/// 供调用方在释放 DB 锁 / 不在锁内时调用，避免大库打包阻塞 daemon。
pub fn pack_backup_zip(backup_dir: &Path) -> Result<PathBuf> {
    let zip_path = backup_dir.with_extension("zip");
    pack_zip(backup_dir, &zip_path)?;
    Ok(zip_path)
}

/// 把备份目录打包成一个 zip 文件（目录名 = `<ts>.zip`），便于用户整体下载迁移。
pub fn pack_zip(backup_dir: &Path, zip_path: &Path) -> Result<()> {
    let file = std::fs::File::create(zip_path)?;
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    fn add_dir(
        writer: &mut zip::ZipWriter<std::fs::File>,
        dir: &Path,
        base: &Path,
        options: zip::write::SimpleFileOptions,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let name = rel.to_string_lossy().replace('\\', "/");
            if entry.file_type()?.is_dir() {
                writer.add_directory(name + "/", options)?;
                add_dir(writer, &path, base, options)?;
            } else if entry.file_type()?.is_file() {
                let bytes = fs::read(&path)?;
                writer.start_file(name, options)?;
                writer.write_all(&bytes)?;
            }
        }
        Ok(())
    }

    writer.add_directory("", options)?;
    add_dir(&mut writer, backup_dir, backup_dir, options)?;
    writer.finish()?;
    Ok(())
}

/// 列出已有备份（按名称倒序，最新的在前）。每个备份以 `name` + `zip` 存在。
pub fn list_backups(state_dir: &Path) -> Result<Vec<BackupInfo>> {
    let dir = state_dir.join(BACKUP_SUBDIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut infos = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() && path.join("reading-steiner.db").exists() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let zip_path = dir.join(format!("{name}.zip"));
            infos.push(BackupInfo {
                name,
                has_zip: zip_path.exists(),
            });
        }
    }
    // 按名称倒序（最新的在前，时间戳字典序即时间序）。
    infos.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(infos)
}

/// 备份条目信息。
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupInfo {
    pub name: String,
    pub has_zip: bool,
}

/// 返回备份对应 zip 文件的路径（若存在）。
pub fn backup_zip_path(state_dir: &Path, name: &str) -> Option<PathBuf> {
    let p = state_dir.join(BACKUP_SUBDIR).join(format!("{name}.zip"));
    if p.exists() { Some(p) } else { None }
}

/// 校验备份名是否合法：备份目录名固定为 `YYYYMMDD-HHMMSS` 时间戳，
/// 仅允许数字与连字符。用于阻止 `../` 等路径遍历注入。
pub fn is_valid_backup_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}

/// 从指定备份恢复数据库与 media。
///
/// - `db_conn`：当前运行中的数据库连接（在线恢复）。若为 `None`，则直接覆盖
///   state 目录中的 db 文件（用于 daemon 停止时的 CLI 恢复）。
pub fn restore(backup_dir: &Path, cfg: &Config, db_conn: Option<&mut Connection>) -> Result<()> {
    let db_src = backup_dir.join("reading-steiner.db");
    if !db_src.exists() {
        return Err(Error::other(format!(
            "backup {} has no reading-steiner.db",
            backup_dir.display()
        )));
    }

    // 数据库回拷。
    match db_conn {
        // 在线恢复：通过 SQLite 在线备份接口把备份库写进实时连接，不覆盖正在使用的文件。
        Some(conn) => {
            let src = Connection::open(&db_src)?;
            let backup = rusqlite::backup::Backup::new(&src, conn)?;
            backup.run_to_completion(100, Duration::from_millis(5), None)?;
        }
        // 离线恢复：直接覆盖 db 文件。
        None => {
            fs::create_dir_all(&cfg.state_dir)?;
            fs::copy(&db_src, cfg.state_dir.join("reading-steiner.db"))?;
        }
    }

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
    // CLI 场景 daemon 未运行，无锁阻塞问题，直接打包 zip。
    pack_backup_zip(&dir)?;
    Ok(dir)
}
