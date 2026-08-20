use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use tracing::info;

use crate::config::SourceConfig;
use crate::error::Result;
use crate::models::{
    ChangeEvent, MediaCacheEntry, NotificationRecord, ScheduleState, SnapshotRecord,
};

const SCHEMA_VERSION: i64 = 4;

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
                    detected_at TEXT NOT NULL
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
                    last_notified_fingerprint TEXT,
                    last_notified_at TEXT
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
            "INSERT INTO change_events(watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                ev.watchpoint_id,
                serde_json::to_string(&ev.change_type)?,
                ev.old_items_json,
                ev.new_items_json,
                ev.diff_summary,
                ev.fingerprint,
                ev.dedupe_key,
                ev.image_urls_json,
                ev.detected_at.to_rfc3339()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_change_event(&self, id: i64) -> Result<Option<ChangeEvent>> {
        self.conn
            .query_row(
                "SELECT id, watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at
                 FROM change_events WHERE id=?1",
                [id],
                |r| {
                    Ok(ChangeEvent {
                        id: r.get(0)?,
                        watchpoint_id: r.get(1)?,
                        change_type: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or(crate::config::ChangeType::Updated),
                        old_items_json: r.get(3)?,
                        new_items_json: r.get(4)?,
                        diff_summary: r.get(5)?,
                        fingerprint: r.get(6)?,
                        dedupe_key: r.get(7)?,
                        image_urls_json: r.get(8)?,
                        detected_at: parse_ts(&r.get::<_, String>(9)?),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_change_events(
        &self,
        watchpoint_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChangeEvent>> {
        let sql = if let Some(_wp) = watchpoint_id {
            "SELECT id, watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at
             FROM change_events WHERE watchpoint_id=?1 ORDER BY id DESC LIMIT ?2"
        } else {
            "SELECT id, watchpoint_id, change_type, old_items_json, new_items_json, diff_summary, fingerprint, dedupe_key, image_urls_json, detected_at
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

    pub fn get_schedule_state(&self, source_id: &str) -> Result<Option<ScheduleState>> {
        self.conn
            .query_row(
                "SELECT source_id, next_due_at, consecutive_failures, consecutive_changes, backoff_until, last_success_at, last_notified_fingerprint, last_notified_at FROM schedule_state WHERE source_id=?1",
                [source_id],
                |r| {
                    Ok(ScheduleState {
                        source_id: r.get(0)?,
                        next_due_at: parse_ts(&r.get::<_, String>(1)?),
                        consecutive_failures: r.get::<_, i64>(2)? as u32,
                        consecutive_changes: r.get::<_, i64>(3)? as u32,
                        backoff_until: r.get::<_, Option<String>>(4)?.map(|s| parse_ts(&s)),
                        last_success_at: r.get::<_, Option<String>>(5)?.map(|s| parse_ts(&s)),
                        last_notified_fingerprint: r.get(6)?,
                        last_notified_at: r
                            .get::<_, Option<String>>(7)?
                            .map(|s| parse_ts(&s)),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_schedule_state(&self, state: &ScheduleState) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schedule_state(source_id, next_due_at, consecutive_failures, consecutive_changes, backoff_until, last_success_at, last_notified_fingerprint, last_notified_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(source_id) DO UPDATE SET next_due_at=excluded.next_due_at, consecutive_failures=excluded.consecutive_failures, consecutive_changes=excluded.consecutive_changes, backoff_until=excluded.backoff_until, last_success_at=excluded.last_success_at, last_notified_fingerprint=excluded.last_notified_fingerprint, last_notified_at=excluded.last_notified_at",
            params![
                state.source_id,
                state.next_due_at.to_rfc3339(),
                state.consecutive_failures as i64,
                state.consecutive_changes as i64,
                state.backoff_until.map(|d| d.to_rfc3339()),
                state.last_success_at.map(|d| d.to_rfc3339()),
                state.last_notified_fingerprint,
                state.last_notified_at.map(|d| d.to_rfc3339())
            ],
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
            "INSERT INTO notifications(event_id, chat_id, message_ids_json, status, attempts, next_retry_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                notif.event_id,
                notif.chat_id,
                notif.message_ids_json,
                notif.status,
                notif.attempts,
                notif.next_retry_at.map(|d| d.to_rfc3339())
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn pending_notifications(&self, limit: usize) -> Result<Vec<NotificationRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_id, chat_id, message_ids_json, status, attempts, next_retry_at FROM notifications WHERE status='pending' ORDER BY id ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(NotificationRecord {
                id: r.get(0)?,
                event_id: r.get(1)?,
                chat_id: r.get(2)?,
                message_ids_json: r.get(3)?,
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
    })
}

pub fn ts_now() -> String {
    Utc::now().to_rfc3339()
}
