use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::info;

use crate::config::{BrowserConfig, ExtractConfig, GroupConfig, SourceConfig};
use crate::error::Result;
use crate::models::{
    ChangeEvent, MediaCacheEntry, NotificationRecord, ScheduleState, SnapshotRecord,
    SystemNotification,
};

const SCHEMA_VERSION: i64 = 1;

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
        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v < 1 {
            self.conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS sources (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    group_id TEXT,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    notify_enabled INTEGER NOT NULL DEFAULT 1,
                    config_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_sources_group ON sources(group_id);

                CREATE TABLE IF NOT EXISTS groups (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    default_cron TEXT,
                    default_headers TEXT,
                    default_browser_config TEXT,
                    default_extract TEXT,
                    default_notify_url TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
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
                DROP TABLE IF EXISTS settings;
                "#,
            )?;
            self.conn
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
            info!(
                version = SCHEMA_VERSION,
                "fresh database schema v1 initialized"
            );
        } else {
            // 如果是从旧版本迁移过来的库，确保新表 groups 和新字段存在，并清理废弃的 tags 和 settings 表
            self.conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS groups (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    default_cron TEXT,
                    default_headers TEXT,
                    default_browser_config TEXT,
                    default_extract TEXT,
                    default_notify_url TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                DROP TABLE IF EXISTS tags;
                DROP TABLE IF EXISTS settings;
                "#,
            )?;
        }

        Ok(())
    }

    // ---- 监控源 CRUD ----

    pub fn upsert_source(&self, source: &SourceConfig) -> Result<()> {
        let config_json = serde_json::to_string(source)?;
        let enabled = if source.enabled { 1 } else { 0 };
        let notify_enabled = if source.notify_enabled { 1 } else { 0 };
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO sources(id, name, group_id, enabled, notify_enabled, config_json, updated_at) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET 
                name=excluded.name, 
                group_id=excluded.group_id, 
                enabled=excluded.enabled, 
                notify_enabled=excluded.notify_enabled, 
                config_json=excluded.config_json, 
                updated_at=excluded.updated_at",
            params![
                source.id,
                source.name,
                source.group_id,
                enabled,
                notify_enabled,
                config_json,
                now
            ],
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

    pub fn list_sources_by_group(&self, group_id: &str) -> Result<Vec<SourceConfig>> {
        let mut stmt = self
            .conn
            .prepare("SELECT config_json FROM sources WHERE group_id=?1 ORDER BY id")?;
        let rows = stmt.query_map([group_id], |r| r.get::<_, String>(0))?;
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

    // ---- 站点分组 (Group / Presets) CRUD ----

    pub fn list_groups(&self) -> Result<Vec<GroupConfig>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, default_cron, default_headers, default_browser_config, default_extract, default_notify_url, created_at, updated_at 
             FROM groups ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let headers_str: Option<String> = r.get(3)?;
            let default_headers = headers_str
                .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
                .unwrap_or_default();

            let browser_str: Option<String> = r.get(4)?;
            let default_browser_config =
                browser_str.and_then(|s| serde_json::from_str::<BrowserConfig>(&s).ok());

            let extract_str: Option<String> = r.get(5)?;
            let default_extract =
                extract_str.and_then(|s| serde_json::from_str::<ExtractConfig>(&s).ok());

            let created_at: String = r.get(7)?;
            let updated_at: String = r.get(8)?;

            Ok(GroupConfig {
                id: r.get(0)?,
                name: r.get(1)?,
                default_cron: r.get(2)?,
                default_headers,
                default_browser_config,
                default_extract,
                default_notify_url: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                created_at: Some(parse_ts(&created_at)),
                updated_at: Some(parse_ts(&updated_at)),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_group(&self, id: &str) -> Result<Option<GroupConfig>> {
        let res = self.conn.query_row(
            "SELECT id, name, default_cron, default_headers, default_browser_config, default_extract, default_notify_url, created_at, updated_at 
             FROM groups WHERE id=?1",
            [id],
            |r| {
                let headers_str: Option<String> = r.get(3)?;
                let default_headers = headers_str
                    .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
                    .unwrap_or_default();

                let browser_str: Option<String> = r.get(4)?;
                let default_browser_config = browser_str
                    .and_then(|s| serde_json::from_str::<BrowserConfig>(&s).ok());

                let extract_str: Option<String> = r.get(5)?;
                let default_extract = extract_str
                    .and_then(|s| serde_json::from_str::<ExtractConfig>(&s).ok());

                let created_at: String = r.get(7)?;
                let updated_at: String = r.get(8)?;

                Ok(GroupConfig {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    default_cron: r.get(2)?,
                    default_headers,
                    default_browser_config,
                    default_extract,
                    default_notify_url: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    created_at: Some(parse_ts(&created_at)),
                    updated_at: Some(parse_ts(&updated_at)),
                })
            },
        ).optional()?;
        Ok(res)
    }

    pub fn upsert_group(&self, group: &GroupConfig) -> Result<()> {
        let headers_json = serde_json::to_string(&group.default_headers)?;
        let browser_json = group
            .default_browser_config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let extract_json = group
            .default_extract
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO groups(id, name, default_cron, default_headers, default_browser_config, default_extract, default_notify_url, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name,
                default_cron=excluded.default_cron,
                default_headers=excluded.default_headers,
                default_browser_config=excluded.default_browser_config,
                default_extract=excluded.default_extract,
                default_notify_url=excluded.default_notify_url,
                updated_at=excluded.updated_at",
            params![
                group.id,
                group.name,
                group.default_cron,
                headers_json,
                browser_json,
                extract_json,
                group.default_notify_url,
                now,
                now
            ],
        )?;
        Ok(())
    }

    pub fn delete_group(&self, id: &str) -> Result<usize> {
        // 解除该分组下所有源的分组关联
        self.conn
            .execute("UPDATE sources SET group_id=NULL WHERE group_id=?1", [id])?;
        Ok(self.conn.execute("DELETE FROM groups WHERE id=?1", [id])?)
    }

    // ---- 快照与事件 ----

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
        Ok(self.conn.execute(
            "UPDATE change_events SET read=1 WHERE watchpoint_id=?1",
            [source_id],
        )?)
    }

    /// 标记全部变更事件为已读。返回受影响的行数。
    pub fn mark_all_events_read(&self) -> Result<usize> {
        Ok(self
            .conn
            .execute("UPDATE change_events SET read=1 WHERE read=0", [])?)
    }

    /// 标记指定变更事件为已读。返回受影响的行数。
    pub fn mark_event_read(&self, event_id: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("UPDATE change_events SET read=1 WHERE id=?1", [event_id])?)
    }

    // ---- 展示元信息 ----

    pub fn list_source_meta(
        &self,
        sources: &[SourceConfig],
    ) -> Result<Vec<crate::models::SourceMeta>> {
        use crate::models::SourceMeta;

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

    pub fn update_event_screenshot(&self, event_id: i64, path: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE change_events SET screenshot_path=?1 WHERE id=?2",
            params![path, event_id],
        )?;
        Ok(())
    }

    // ---- 调度状态 ----

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

    pub fn list_schedule_states(&self) -> Result<HashMap<String, ScheduleState>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_id, next_due_at, consecutive_failures, consecutive_changes, backoff_until, last_success_at, last_error, last_notified_fingerprint, last_notified_at, failure_notified FROM schedule_state",
        )?;
        let rows = stmt.query_map([], |r| {
            let source_id: String = r.get(0)?;
            Ok((
                source_id.clone(),
                ScheduleState {
                    source_id,
                    next_due_at: parse_ts(&r.get::<_, String>(1)?),
                    consecutive_failures: r.get::<_, i64>(2)? as u32,
                    consecutive_changes: r.get::<_, i64>(3)? as u32,
                    backoff_until: r.get::<_, Option<String>>(4)?.map(|s| parse_ts(&s)),
                    last_success_at: r.get::<_, Option<String>>(5)?.map(|s| parse_ts(&s)),
                    last_error: r.get(6)?,
                    last_notified_fingerprint: r.get(7)?,
                    last_notified_at: r.get::<_, Option<String>>(8)?.map(|s| parse_ts(&s)),
                    failure_notified: r.get::<_, i64>(9)? != 0,
                },
            ))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (sid, state) = row?;
            out.insert(sid, state);
        }
        Ok(out)
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

    pub fn prune_history(&self, per_source: usize) -> Result<()> {
        if per_source == 0 {
            return Ok(());
        }
        let limit = per_source as i64;
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

    pub fn insert_system_notification(
        &self,
        chat_id: &str,
        text: &str,
        target_json: &str,
    ) -> Result<()> {
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
