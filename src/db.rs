use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::info;

use crate::config::{EditableSettings, SourceConfig};
use crate::error::Result;
use crate::models::{
    ChangeEvent, MediaCacheEntry, NotificationRecord, ScheduleState, SnapshotRecord,
    SystemNotification, TagConfig,
};

const SCHEMA_VERSION: i64 = 11;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 暴露底层连接，供备份等需要在线一致性快照的场景使用。
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// 可变地暴露底层连接，供在线恢复等需要写入实时连接的场景使用。
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let mut v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v < 1 {
            self.conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS sources (
                    id TEXT PRIMARY KEY,
                    config_json TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    watchpoint_id TEXT NOT NULL,
                    fetched_at TEXT NOT NULL,
                    status INTEGER NOT NULL,
                    etag TEXT,
                    last_modified TEXT,
                    content_sha256 TEXT NOT NULL,
                    normalized_fingerprint TEXT NOT NULL,
                    items_json TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL DEFAULT 0,
                    engine TEXT NOT NULL DEFAULT 'http'
                );
                CREATE INDEX IF NOT EXISTS idx_snapshots_watchpoint ON snapshots(watchpoint_id, id DESC);
                CREATE TABLE IF NOT EXISTS change_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    watchpoint_id TEXT NOT NULL,
                    change_type TEXT NOT NULL,
                    old_items_json TEXT NOT NULL,
                    new_items_json TEXT NOT NULL,
                    diff_summary TEXT NOT NULL,
                    fingerprint TEXT NOT NULL,
                    dedupe_key TEXT NOT NULL,
                    image_urls_json TEXT NOT NULL DEFAULT '[]',
                    detected_at TEXT NOT NULL,
                    read INTEGER NOT NULL DEFAULT 0,
                    screenshot_path TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_events_watchpoint ON change_events(watchpoint_id, id DESC);
                CREATE TABLE IF NOT EXISTS media_cache (
                    canonical_url TEXT PRIMARY KEY,
                    sha256 TEXT NOT NULL,
                    mime TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    file_path TEXT NOT NULL,
                    telegram_file_id TEXT,
                    phash TEXT,
                    fetched_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS notifications (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_id INTEGER NOT NULL,
                    chat_id TEXT NOT NULL,
                    target_json TEXT NOT NULL DEFAULT '',
                    message_ids_json TEXT NOT NULL DEFAULT '[]',
                    status TEXT NOT NULL DEFAULT 'pending',
                    attempts INTEGER NOT NULL DEFAULT 0,
                    next_retry_at TEXT
                );
                CREATE TABLE IF NOT EXISTS schedule_state (
                    source_id TEXT PRIMARY KEY,
                    next_due_at TEXT NOT NULL,
                    consecutive_failures INTEGER NOT NULL DEFAULT 0,
                    consecutive_changes INTEGER NOT NULL DEFAULT 0,
                    backoff_until TEXT,
                    last_success_at TEXT,
                    last_error TEXT,
                    last_notified_fingerprint TEXT,
                    last_notified_at TEXT,
                    failure_notified INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS system_notifications (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    chat_id TEXT NOT NULL,
                    target_json TEXT NOT NULL DEFAULT '',
                    text TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    attempts INTEGER NOT NULL DEFAULT 0,
                    next_retry_at TEXT,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS tags (
                    name TEXT PRIMARY KEY,
                    history_limit INTEGER NOT NULL DEFAULT 0,
                    notify_url TEXT NOT NULL DEFAULT '',
                    extract TEXT
                );
                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                "#,
            )?;
            self.conn
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
            v = SCHEMA_VERSION;
            info!(version = SCHEMA_VERSION, "database schema initialized");
        }
        if (1..3).contains(&v) {
            let mut sql = String::new();
            if v < 2 {
                sql.push_str(
                    "ALTER TABLE schedule_state ADD COLUMN consecutive_changes INTEGER NOT NULL DEFAULT 0;",
                );
            }
            sql.push_str("ALTER TABLE schedule_state ADD COLUMN last_notified_fingerprint TEXT;");
            sql.push_str("ALTER TABLE schedule_state ADD COLUMN last_notified_at TEXT;");
            self.conn.execute_batch(&sql)?;
            self.conn.pragma_update(None, "user_version", 3)?;
            v = 3;
            info!("database schema migrated to v3");
        }
        if v < 4 {
            self.conn.execute_batch(
                "ALTER TABLE change_events ADD COLUMN image_urls_json TEXT NOT NULL DEFAULT '[]';",
            )?;
            self.conn.pragma_update(None, "user_version", 4)?;
            info!("database schema migrated to v4");
        }
        if v < 5 {
            self.conn.execute_batch(
                "ALTER TABLE schedule_state ADD COLUMN failure_notified INTEGER NOT NULL DEFAULT 0;",
            )?;
            self.conn.pragma_update(None, "user_version", 5)?;
            info!("database schema migrated to v5");
        }
        if v < 6 {
            self.conn.execute_batch(
                "ALTER TABLE change_events ADD COLUMN read INTEGER NOT NULL DEFAULT 0;\n\
                 ALTER TABLE change_events ADD COLUMN screenshot_path TEXT;",
            )?;
            self.conn.pragma_update(None, "user_version", 6)?;
            info!("database schema migrated to v6");
        }
        if v < 7 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS tags (\n\
                     name TEXT PRIMARY KEY,\n\
                     history_limit INTEGER NOT NULL DEFAULT 0\n\
                 );",
            )?;
            self.conn.pragma_update(None, "user_version", 7)?;
            info!("database schema migrated to v7");
        }
        if v < 8 {
            // v8：分组（标签）新增通知 URL 与默认内容提取；通知记录新增目标信息。
            self.conn.execute_batch(
                "ALTER TABLE tags ADD COLUMN notify_url TEXT NOT NULL DEFAULT '';\n\
                 ALTER TABLE tags ADD COLUMN extract TEXT;\n\
                 ALTER TABLE notifications ADD COLUMN target_json TEXT NOT NULL DEFAULT '';\n\
                 ALTER TABLE system_notifications ADD COLUMN target_json TEXT NOT NULL DEFAULT '';",
            )?;
            self.conn.pragma_update(None, "user_version", 8)?;
            info!("database schema migrated to v8");
        }
        if v < 9 {
            // v9：移除分组遗留的监控/通知开关（由监控源自身控制），从 schema 中彻底删除。
            self.conn.execute_batch(
                "ALTER TABLE tags DROP COLUMN enabled;\n\
                 ALTER TABLE tags DROP COLUMN notify_enabled;",
            )?;
            self.conn.pragma_update(None, "user_version", 9)?;
            info!("database schema migrated to v9");
        }
        if v < 10 {
            // v10：schedule_state 记录最近一次错误信息，供 Web 控制台展示错误原因。
            self.conn.execute_batch("ALTER TABLE schedule_state ADD COLUMN last_error TEXT;")?;
            self.conn.pragma_update(None, "user_version", 10)?;
            info!("database schema migrated to v10");
        }
        if v < 11 {
            // v11：全局可编辑设置改存 SQLite（settings 表），webui 直接读写，不再依赖 config.yaml。
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );",
            )?;
            self.conn.pragma_update(None, "user_version", 11)?;
            info!("database schema migrated to v11");
        }
        // 确保 settings 表存在一条全局默认记录。无论是新建库（v<1 直建表后跳到
        // SCHEMA_VERSION）还是老库升级，settings 都可能为空；这里用 INSERT OR IGNORE
        // 统一 seed 一条带合理默认值的 global 记录，`get_settings()` 恒有值，
        // 运行时不再做「缺失→默认」兜底。
        self.seed_default_settings()?;
        Ok(())
    }

    /// 若 settings 表没有 global 行，写入一条默认设置（`EditableSettings::default()`）。
    fn seed_default_settings(&self) -> Result<()> {
        let value = serde_json::to_string(&EditableSettings::default())?;
        self.conn.execute(
            "INSERT OR IGNORE INTO settings(key, value) VALUES('global', ?1)",
            [value],
        )?;
        Ok(())
    }

    pub fn upsert_source(&self, source: &SourceConfig) -> Result<()> {
        let config_json = serde_json::to_string(source)?;
        let enabled = if source.enabled { 1 } else { 0 };
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO sources(id, config_json, enabled, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET config_json=excluded.config_json, enabled=excluded.enabled, updated_at=excluded.updated_at",
            params![source.id, config_json, enabled, now],
        )?;
        Ok(())
    }

    pub fn list_sources(&self) -> Result<Vec<SourceConfig>> {
        let mut stmt = self
            .conn
            .prepare("SELECT config_json FROM sources ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let json = row?;
            out.push(serde_json::from_str(&json)?);
        }
        Ok(out)
    }

    pub fn delete_source(&self, id: &str) -> Result<()> {
        let conn = &self.conn;
        let deleted = conn.execute("DELETE FROM sources WHERE id=?1", [id])?;
        if deleted > 0 {
            // 级联清理该监控源的所有关联数据（watchpoint_id == source id），
            // 避免 schedule_state / snapshots / change_events 成为孤儿数据，
            // 防止同 id 重新添加时旧快照指纹导致首次检测被误判为“无变化”。
            conn.execute("DELETE FROM schedule_state WHERE source_id=?1", [id])?;
            conn.execute("DELETE FROM snapshots WHERE watchpoint_id=?1", [id])?;
            conn.execute("DELETE FROM change_events WHERE watchpoint_id=?1", [id])?;
        }
        Ok(())
    }

    pub fn get_source(&self, id: &str) -> Result<Option<SourceConfig>> {
        let json = self
            .conn
            .query_row("SELECT config_json FROM sources WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        json.map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(Into::into)
    }

    pub fn save_snapshot(&self, snap: &SnapshotRecord) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO snapshots(watchpoint_id, fetched_at, status, etag, last_modified, content_sha256, normalized_fingerprint, items_json, duration_ms, engine)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                snap.watchpoint_id,
                snap.fetched_at.to_rfc3339(),
                snap.status as i64,
                snap.etag,
                snap.last_modified,
                snap.content_sha256,
                snap.normalized_fingerprint,
                snap.items_json,
                snap.duration_ms as i64,
                snap.engine
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn latest_snapshot(&self, watchpoint_id: &str) -> Result<Option<SnapshotRecord>> {
        self.conn
            .query_row(
                "SELECT id, watchpoint_id, fetched_at, status, etag, last_modified, content_sha256, normalized_fingerprint, items_json, duration_ms, engine
                 FROM snapshots WHERE watchpoint_id=?1 ORDER BY id DESC LIMIT 1",
                [watchpoint_id],
                |r| {
                    Ok(SnapshotRecord {
                        id: r.get(0)?,
                        watchpoint_id: r.get(1)?,
                        fetched_at: parse_ts(&r.get::<_, String>(2)?),
                        status: r.get::<_, i64>(3)? as u16,
                        etag: r.get(4)?,
                        last_modified: r.get(5)?,
                        content_sha256: r.get(6)?,
                        normalized_fingerprint: r.get(7)?,
                        items_json: r.get(8)?,
                        duration_ms: r.get::<_, i64>(9)? as u64,
                        engine: r.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_change_event(&self, ev: &ChangeEvent) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO change_events(watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at, read, screenshot_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                ev.watchpoint_id,
                serde_json::to_string(&ev.change_type)?,
                ev.old_items_json,
                ev.new_items_json,
                ev.diff_summary,
                ev.fingerprint,
                ev.dedupe_key,
                ev.image_urls_json,
                ev.detected_at.to_rfc3339(),
                ev.read as i64,
                ev.screenshot_path
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_change_event(&self, id: i64) -> Result<Option<ChangeEvent>> {
        self.conn
            .query_row(
                "SELECT id, watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at, read, screenshot_path
                 FROM change_events WHERE id=?1",
                [id],
                map_event,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_change_events(
        &self,
        watchpoint_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChangeEvent>> {
        let sql = if watchpoint_id.is_some() {
            "SELECT id, watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at, read, screenshot_path
             FROM change_events WHERE watchpoint_id=?1 ORDER BY id DESC LIMIT ?2"
        } else {
            "SELECT id, watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at, read, screenshot_path
             FROM change_events ORDER BY id DESC LIMIT ?1"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows: Box<dyn Iterator<Item = rusqlite::Result<ChangeEvent>> + '_> =
            if let Some(wp) = watchpoint_id {
                Box::new(stmt.query_map(params![wp, limit as i64], map_event)?)
            } else {
                Box::new(stmt.query_map([limit as i64], map_event)?)
            };
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 标记指定监控源的全部变更事件为已读。返回受影响的行数。
    pub fn mark_source_events_read(&self, source_id: &str) -> Result<usize> {
        Ok(self
            .conn
            .execute("UPDATE change_events SET read=1 WHERE watchpoint_id=?1", [source_id])?)
    }

    /// 标记指定变更事件为已读。返回受影响的行数。
    pub fn mark_event_read(&self, event_id: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("UPDATE change_events SET read=1 WHERE id=?1", [event_id])?)
    }

    /// 列出全部分组（标签）设置。
    pub fn list_tags(&self) -> Result<Vec<TagConfig>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, history_limit, notify_url, extract FROM tags ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(TagConfig {
                name: r.get(0)?,
                history_limit: r.get::<_, i64>(1)? as usize,
                notify_url: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                extract: r
                    .get::<_, Option<String>>(3)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 获取单个分组（标签）设置。
    pub fn get_tag(&self, name: &str) -> Result<Option<TagConfig>> {
        self.conn
            .query_row(
                "SELECT name, history_limit, notify_url, extract FROM tags WHERE name=?1",
                [name],
                |r| {
                    Ok(TagConfig {
                        name: r.get(0)?,
                        history_limit: r.get::<_, i64>(1)? as usize,
                        notify_url: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        extract: r
                            .get::<_, Option<String>>(3)?
                            .and_then(|s| serde_json::from_str(&s).ok()),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// 新增 / 更新一个分组（标签）设置。
    pub fn upsert_tag(&self, tag: &TagConfig) -> Result<()> {
        let extract_json = tag
            .extract
            .as_ref()
            .map(|e| serde_json::to_string(e))
            .transpose()?;
        self.conn.execute(
            "INSERT INTO tags(name, history_limit, notify_url, extract) VALUES (?1,?2,?3,?4)\n\
             ON CONFLICT(name) DO UPDATE SET history_limit=excluded.history_limit,\
                 notify_url=excluded.notify_url, extract=excluded.extract",
            params![tag.name, tag.history_limit as i64, tag.notify_url, extract_json],
        )?;
        Ok(())
    }

    /// 删除一个分组（标签）设置。
    pub fn delete_tag(&self, name: &str) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM tags WHERE name=?1", [name])?)
    }

    /// 确保给定标签名以默认值存在于 tags 表中（缺失则插入，已存在保持不变）。
    /// 用于在监控源保存标签时自动登记分组，使新标签能出现在「分组管理」列表供配置。
    pub fn ensure_tags(&self, names: &[String]) -> Result<()> {
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO tags(name, history_limit, notify_url, extract) \
                 VALUES (?1, 0, '', NULL)",
                [trimmed],
            )?;
        }
        Ok(())
    }

    /// 读取全局可编辑设置（存于 SQLite `settings` 表）。
    /// 未配置时返回 `None`。若 global 存值损坏（无法解析），则用默认值自愈
    /// 该记录后返回默认值，避免 daemon 因数据损坏而无法启动。
    pub fn get_settings(&self) -> Result<Option<EditableSettings>> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key='global'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some(v) => match serde_json::from_str(&v) {
                Ok(s) => Ok(Some(s)),
                Err(_) => {
                    info!("settings 记录损坏，重置为默认值");
                    let value = serde_json::to_string(&EditableSettings::default())?;
                    self.conn.execute(
                        "INSERT INTO settings(key, value) VALUES('global', ?1)\n\
                         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                        [value],
                    )?;
                    Ok(Some(EditableSettings::default()))
                }
            },
        }
    }

    /// 写入全局可编辑设置（存于 SQLite `settings` 表），作为 Web/CLI 改配置的唯一落点。
    pub fn set_settings(&self, settings: &EditableSettings) -> Result<()> {
        let value = serde_json::to_string(settings)?;
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES('global', ?1)\n\
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [value],
        )?;
        Ok(())
    }

    /// 统计指定监控源未读变更事件数。
    pub fn unread_count(&self, source_id: &str) -> Result<u32> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM change_events WHERE watchpoint_id=?1 AND read=0",
            [source_id],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// 获取指定监控源最近一次检查时间（最新快照时间）。
    pub fn last_check_at(&self, source_id: &str) -> Result<Option<DateTime<Utc>>> {
        self.conn
            .query_row(
                "SELECT fetched_at FROM snapshots WHERE watchpoint_id=?1 ORDER BY id DESC LIMIT 1",
                [source_id],
                |r| r.get::<_, String>(0).map(|s| parse_ts(&s)),
            )
            .optional()
            .map_err(Into::into)
    }

    /// 获取指定监控源最近一次变更时间。
    pub fn last_change_at(&self, source_id: &str) -> Result<Option<DateTime<Utc>>> {
        self.conn
            .query_row(
                "SELECT detected_at FROM change_events WHERE watchpoint_id=?1 ORDER BY id DESC LIMIT 1",
                [source_id],
                |r| r.get::<_, String>(0).map(|s| parse_ts(&s)),
            )
            .optional()
            .map_err(Into::into)
    }

    /// 获取所有监控源的展示用元信息。
    pub fn list_source_meta(&self, sources: &[SourceConfig]) -> Result<Vec<crate::models::SourceMeta>> {
        use crate::models::SourceMeta;
        use std::collections::HashMap;

        // 最近检查时间：每个源取最新快照的 fetched_at。
        let mut last_check_at: HashMap<String, DateTime<Utc>> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT watchpoint_id, MAX(fetched_at) AS latest FROM snapshots GROUP BY watchpoint_id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, parse_ts(&r.get::<_, String>(1)?)))
            })?;
            for row in rows {
                let (wp, ts) = row?;
                last_check_at.insert(wp, ts);
            }
        }

        // 最近变更时间：每个源取最新事件的 detected_at。
        let mut last_change_at: HashMap<String, DateTime<Utc>> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT watchpoint_id, MAX(detected_at) AS latest FROM change_events GROUP BY watchpoint_id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, parse_ts(&r.get::<_, String>(1)?)))
            })?;
            for row in rows {
                let (wp, ts) = row?;
                last_change_at.insert(wp, ts);
            }
        }

        // 未读事件数：每个源统计 read=0 的事件。
        let mut unread_count: HashMap<String, u32> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT watchpoint_id, COUNT(*) FROM change_events WHERE read=0 GROUP BY watchpoint_id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (wp, n) = row?;
                unread_count.insert(wp, n as u32);
            }
        }

        // 错误状态：schedule_state 中连续失败次数 > 0 时记录具体错误信息。
        let mut has_error: HashMap<String, bool> = HashMap::new();
        let mut last_error: HashMap<String, String> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT source_id, last_error FROM schedule_state WHERE consecutive_failures > 0",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (sid, err) = row?;
                has_error.insert(sid.clone(), true);
                if let Some(err) = err {
                    last_error.insert(sid, err);
                }
            }
        }

        let mut out = Vec::with_capacity(sources.len());
        for s in sources {
            out.push(SourceMeta {
                source: s.clone(),
                last_check_at: last_check_at.get(&s.id).copied(),
                last_change_at: last_change_at.get(&s.id).copied(),
                unread_count: unread_count.get(&s.id).copied().unwrap_or(0),
                has_error: has_error.get(&s.id).copied().unwrap_or(false),
                last_error: last_error.get(&s.id).cloned(),
            });
        }
        Ok(out)
    }

    /// 更新变更事件的截图路径。传 `None` 时清空截图路径。
    pub fn update_event_screenshot(&self, event_id: i64, path: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE change_events SET screenshot_path=?1 WHERE id=?2",
            params![path, event_id],
        )?;
        Ok(())
    }

    pub fn get_schedule_state(&self, source_id: &str) -> Result<Option<ScheduleState>> {
        self.conn
            .query_row(
                "SELECT source_id, next_due_at, consecutive_failures, consecutive_changes, backoff_until, last_success_at, last_error, last_notified_fingerprint, last_notified_at, failure_notified FROM schedule_state WHERE source_id=?1",
                [source_id],
                |r| {
                    Ok(ScheduleState {
                        source_id: r.get(0)?,
                        next_due_at: parse_ts(&r.get::<_, String>(1)?),
                        consecutive_failures: r.get::<_, i64>(2)? as u32,
                        consecutive_changes: r.get::<_, i64>(3)? as u32,
                        backoff_until: r.get::<_, Option<String>>(4)?.map(|s| parse_ts(&s)),
                        last_success_at: r.get::<_, Option<String>>(5)?.map(|s| parse_ts(&s)),
                        last_error: r.get(6)?,
                        last_notified_fingerprint: r.get(7)?,
                        last_notified_at: r
                            .get::<_, Option<String>>(8)?
                            .map(|s| parse_ts(&s)),
                        failure_notified: r.get::<_, i64>(9)? != 0,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_schedule_state(&self, state: &ScheduleState) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schedule_state(source_id, next_due_at, consecutive_failures, consecutive_changes, backoff_until, last_success_at, last_error, last_notified_fingerprint, last_notified_at, failure_notified)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(source_id) DO UPDATE SET next_due_at=excluded.next_due_at, consecutive_failures=excluded.consecutive_failures, consecutive_changes=excluded.consecutive_changes, backoff_until=excluded.backoff_until, last_success_at=excluded.last_success_at, last_error=excluded.last_error, last_notified_fingerprint=excluded.last_notified_fingerprint, last_notified_at=excluded.last_notified_at, failure_notified=excluded.failure_notified",
            params![
                state.source_id,
                state.next_due_at.to_rfc3339(),
                state.consecutive_failures as i64,
                state.consecutive_changes as i64,
                state.backoff_until.map(|d| d.to_rfc3339()),
                state.last_success_at.map(|d| d.to_rfc3339()),
                state.last_error,
                state.last_notified_fingerprint,
                state.last_notified_at.map(|d| d.to_rfc3339()),
                state.failure_notified as i64
            ],
        )?;
        Ok(())
    }

    /// 清理每个监控源多余的旧快照与变更事件，使其数量不超过 `per_source` 条。
    /// `0` 表示不限制。
    pub fn prune_history(&self, per_source: usize) -> Result<()> {
        if per_source == 0 {
            return Ok(());
        }
        let limit = per_source as i64;
        // 每个监控源（watchpoint）仅保留最新 per_source 条，其余删除。
        self.conn.execute(
            "DELETE FROM change_events WHERE id NOT IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (PARTITION BY watchpoint_id ORDER BY id DESC) AS rn
                    FROM change_events
                ) WHERE rn <= ?1
            )",
            [limit],
        )?;
        self.conn.execute(
            "DELETE FROM snapshots WHERE id NOT IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (PARTITION BY watchpoint_id ORDER BY id DESC) AS rn
                    FROM snapshots
                ) WHERE rn <= ?1
            )",
            [limit],
        )?;
        Ok(())
    }

    /// 清理单个监控源多余的旧快照与变更事件，使其数量不超过 `limit` 条。
    /// `0` 表示不限制。
    pub fn prune_history_for_source(&self, watchpoint_id: &str, limit: usize) -> Result<()> {
        if limit == 0 {
            return Ok(());
        }
        let limit = limit as i64;
        self.conn.execute(
            "DELETE FROM change_events WHERE watchpoint_id=?1 AND id NOT IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (PARTITION BY watchpoint_id ORDER BY id DESC) AS rn
                    FROM change_events WHERE watchpoint_id=?1
                ) WHERE rn <= ?2
            )",
            params![watchpoint_id, limit],
        )?;
        self.conn.execute(
            "DELETE FROM snapshots WHERE watchpoint_id=?1 AND id NOT IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (PARTITION BY watchpoint_id ORDER BY id DESC) AS rn
                    FROM snapshots WHERE watchpoint_id=?1
                ) WHERE rn <= ?2
            )",
            params![watchpoint_id, limit],
        )?;
        Ok(())
    }

    /// 插入一条系统级（非事件关联）通知，用于连续失败告警等场景。
    /// `target_json` 为发送目标（token + chat ids）的 JSON；为空时发送方回退到全局目标。
    pub fn insert_system_notification(&self, chat_id: &str, text: &str, target_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO system_notifications(chat_id, target_json, text, status, attempts, next_retry_at, created_at) VALUES (?1,?2,?3,'pending',0,NULL,?4)",
            params![chat_id, target_json, text, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn insert_media_cache(&self, entry: &MediaCacheEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO media_cache(canonical_url, sha256, mime, size, file_path, telegram_file_id, phash, fetched_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(canonical_url) DO UPDATE SET sha256=excluded.sha256, mime=excluded.mime, size=excluded.size, file_path=excluded.file_path, telegram_file_id=excluded.telegram_file_id, phash=excluded.phash, fetched_at=excluded.fetched_at",
            params![
                entry.canonical_url,
                entry.sha256,
                entry.mime,
                entry.size,
                entry.file_path,
                entry.telegram_file_id,
                entry.phash,
                entry.fetched_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_media_cache(&self, canonical_url: &str) -> Result<Option<MediaCacheEntry>> {
        self.conn
            .query_row(
                "SELECT canonical_url, sha256, mime, size, file_path, telegram_file_id, phash, fetched_at FROM media_cache WHERE canonical_url=?1",
                [canonical_url],
                |r| {
                    Ok(MediaCacheEntry {
                        canonical_url: r.get(0)?,
                        sha256: r.get(1)?,
                        mime: r.get(2)?,
                        size: r.get(3)?,
                        file_path: r.get(4)?,
                        telegram_file_id: r.get(5)?,
                        phash: r.get(6)?,
                        fetched_at: parse_ts(&r.get::<_, String>(7)?),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn update_media_telegram_file_id(&self, canonical_url: &str, file_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE media_cache SET telegram_file_id=?1 WHERE canonical_url=?2",
            params![file_id, canonical_url],
        )?;
        Ok(())
    }

    pub fn insert_notification(&self, notif: &NotificationRecord) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notifications(event_id, chat_id, target_json, message_ids_json, status, attempts, next_retry_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                notif.event_id,
                notif.chat_id,
                notif.target_json,
                notif.message_ids_json,
                notif.status,
                notif.attempts,
                notif.next_retry_at.map(|d| d.to_rfc3339())
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn pending_notifications(&self, limit: usize) -> Result<Vec<NotificationRecord>> {
        // 只取出到期的待发送通知：`next_retry_at` 为空表示首次/立即可发，
        // 非空则需等到重试时间点之后才允许再次尝试，避免失败通知每 500ms 疯狂重试。
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, event_id, chat_id, target_json, message_ids_json, status, attempts, next_retry_at FROM notifications WHERE status='pending' AND (next_retry_at IS NULL OR next_retry_at <= ?1) ORDER BY id ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now, limit as i64], |r| {
            Ok(NotificationRecord {
                id: r.get(0)?,
                event_id: r.get(1)?,
                chat_id: r.get(2)?,
                target_json: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                message_ids_json: r.get(4)?,
                status: r.get(5)?,
                attempts: r.get(6)?,
                next_retry_at: r.get::<_, Option<String>>(7)?.map(|s| parse_ts(&s)),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn update_notification_status(
        &self,
        id: i64,
        status: &str,
        message_ids_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE notifications SET status=?1, message_ids_json=?2 WHERE id=?3",
            params![status, message_ids_json, id],
        )?;
        Ok(())
    }

    pub fn mark_notification_retry(
        &self,
        id: i64,
        attempts: i32,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE notifications SET attempts=?1, next_retry_at=?2 WHERE id=?3",
            params![attempts, next_retry_at.map(|d| d.to_rfc3339()), id],
        )?;
        Ok(())
    }

    /// 取出到期的待发送系统通知（连续失败告警等）。
    pub fn pending_system_notifications(&self, limit: usize) -> Result<Vec<SystemNotification>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, chat_id, target_json, text, status, attempts, next_retry_at FROM system_notifications WHERE status='pending' AND (next_retry_at IS NULL OR next_retry_at <= ?1) ORDER BY id ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now, limit as i64], |r| {
            Ok(SystemNotification {
                id: r.get(0)?,
                chat_id: r.get(1)?,
                target_json: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                text: r.get(3)?,
                status: r.get(4)?,
                attempts: r.get(5)?,
                next_retry_at: r.get::<_, Option<String>>(6)?.map(|s| parse_ts(&s)),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn update_system_notification_status(&self, id: i64, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE system_notifications SET status=?1 WHERE id=?2",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn mark_system_notification_retry(
        &self,
        id: i64,
        attempts: i32,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE system_notifications SET attempts=?1, next_retry_at=?2 WHERE id=?3",
            params![attempts, next_retry_at.map(|d| d.to_rfc3339()), id],
        )?;
        Ok(())
    }
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn map_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChangeEvent> {
    Ok(ChangeEvent {
        id: r.get(0)?,
        watchpoint_id: r.get(1)?,
        change_type: serde_json::from_str(&r.get::<_, String>(2)?)
            .unwrap_or(crate::config::ChangeType::Updated),
        old_items_json: r.get(3)?,
        new_items_json: r.get(4)?,
        diff_summary: r.get(5)?,
        fingerprint: r.get(6)?,
        dedupe_key: r.get(7)?,
        image_urls_json: r.get(8)?,
        detected_at: parse_ts(&r.get::<_, String>(9)?),
        read: r.get::<_, i64>(10)? != 0,
        screenshot_path: r.get(11)?,
    })
}

pub fn ts_now() -> String {
    Utc::now().to_rfc3339()
}
