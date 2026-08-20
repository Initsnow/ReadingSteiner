use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, error, info, warn};

use crate::config::{ChangeType, Config, RuntimeConfig, SourceConfig};
use crate::db::Db;
use crate::differ;
use crate::error::Result;
use crate::fetcher::{self, FetchSpec};
use crate::images::ImageDownloader;
use crate::models::{ChangeEvent, DaemonStatus, Item, ScheduleState, SnapshotRecord};
use crate::notifier::{self, TelegramNotifier};
use crate::pipeline;

pub struct AppState {
    pub cfg: Config,
    pub runtime: RuntimeConfig,
    pub db: Arc<Mutex<Db>>,
    /// Live monitoring sources. Solely backed by the SQLite `sources` table,
    /// kept in sync as sources are added / edited / deleted via the Web/CLI.
    pub sources: Mutex<Vec<SourceConfig>>,
    pub notifier: Option<Arc<TelegramNotifier>>,
    pub images: ImageDownloader,
    pub running: AtomicBool,
    pub queue_depth: AtomicUsize,
    pub last_tick_at: Mutex<Option<DateTime<Utc>>>,
    pub engine_health: Mutex<HashMap<String, bool>>,
}

impl AppState {
    pub fn new(cfg: Config) -> Result<Self> {
        let runtime = RuntimeConfig::from_config(&cfg);
        std::fs::create_dir_all(&runtime.state_dir)?;
        let db_path = runtime.state_dir.join("reading-steiner.db");
        let db = Db::open(db_path)?;
        let notifier = match TelegramNotifier::new(&cfg.telegram) {
            Ok(n) => Some(Arc::new(n)),
            Err(e) => {
                warn!(error = %e, "telegram notifier disabled");
                None
            }
        };
        let images = ImageDownloader::new(&runtime.media_dir, 10 * 1024 * 1024, false)?;
        // SQLite is the source of truth for monitoring sources.
        let sources = db.list_sources().unwrap_or_default();
        Ok(Self {
            cfg,
            runtime,
            db: Arc::new(Mutex::new(db)),
            sources: Mutex::new(sources),
            notifier,
            images,
            running: AtomicBool::new(false),
            queue_depth: AtomicUsize::new(0),
            last_tick_at: Mutex::new(None),
            engine_health: Mutex::new(HashMap::new()),
        })
    }

    pub async fn status(&self) -> DaemonStatus {
        let sources = self.sources.lock().await;
        let enabled = sources.iter().filter(|s| s.enabled).count();
        let last_tick = *self.last_tick_at.lock().await;
        let engine_health = self.engine_health.lock().await.clone();
        DaemonStatus {
            running: self.running.load(Ordering::Relaxed),
            version: env!("CARGO_PKG_VERSION").to_string(),
            sources: sources.len(),
            enabled_sources: enabled,
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
            last_tick_at: last_tick,
            engine_health,
        }
    }
}

pub async fn run_daemon(state: Arc<AppState>) -> Result<()> {
    state.running.store(true, Ordering::Relaxed);
    let source_count = state.sources.lock().await.len();
    info!(sources = source_count, "ReadingSteiner daemon started");

    // Initialize schedule states for all live sources so they become due on first tick.
    {
        let db = state.db.lock().await;
        let now = Utc::now();
        let sources = state.sources.lock().await;
        for source in sources.iter() {
            if db.get_schedule_state(&source.id)?.is_none() {
                let due = now + chrono::Duration::seconds(1);
                db.upsert_schedule_state(&ScheduleState {
                    source_id: source.id.clone(),
                    next_due_at: due,
                    consecutive_failures: 0,
                    consecutive_changes: 0,
                    backoff_until: None,
                    last_success_at: None,
                    last_notified_fingerprint: None,
                    last_notified_at: None,
                })?;
            }
        }
    }

    let concurrency = state.runtime.concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));

    loop {
        if !state.running.load(Ordering::Relaxed) {
            break;
        }
        *state.last_tick_at.lock().await = Some(Utc::now());

        let due = {
            let db = state.db.lock().await;
            let now = Utc::now();
            let mut due = Vec::new();
            let sources = state.sources.lock().await;
            for source in sources.iter() {
                if !source.enabled {
                    continue;
                }
                let sched = db.get_schedule_state(&source.id)?.unwrap_or(ScheduleState {
                    source_id: source.id.clone(),
                    next_due_at: now,
                    consecutive_failures: 0,
                    consecutive_changes: 0,
                    backoff_until: None,
                    last_success_at: None,
                    last_notified_fingerprint: None,
                    last_notified_at: None,
                });
                if let Some(until) = sched.backoff_until
                    && until > now
                {
                    continue;
                }
                if sched.next_due_at <= now {
                    due.push(source.clone());
                }
            }
            due
        };

        state.queue_depth.store(due.len(), Ordering::Relaxed);
        for source in due {
            let state = state.clone();
            let semaphore = semaphore.clone();
            tokio::spawn(async move {
                let _permit = semaphore.acquire_owned().await;
                if let Err(e) = check_source(&state, &source.id).await {
                    error!(source = %source.id, error = %e, "check_source failed");
                    let db = state.db.lock().await;
                    if let Some(mut sched) = db.get_schedule_state(&source.id).unwrap_or(None) {
                        sched.consecutive_failures += 1;
                        let backoff = 30u64 * 2u64.pow(sched.consecutive_failures.min(5));
                        sched.backoff_until =
                            Some(Utc::now() + chrono::Duration::seconds(backoff as i64));
                        sched.next_due_at = Utc::now() + chrono::Duration::seconds(1);
                        let _ = db.upsert_schedule_state(&sched);
                    }
                }
            });
        }

        // Drain notification outbox periodically.
        if let Some(notifier) = state.notifier.clone() {
            let db = state.db.clone();
            let images = state.images.clone();
            if let Err(e) = notifier::process_outbox(&db, &images, &notifier, None).await {
                warn!(error = %e, "outbox processing failed");
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(())
}

pub async fn check_source(state: &Arc<AppState>, source_id: &str) -> Result<()> {
    let source = get_live_source(state, source_id).await?;
    let extract_cfg = source.extract.clone();

    let (prev_etag, prev_lm, prev_fp, prev_items_json) = {
        let db = state.db.lock().await;
        let snap = db.latest_snapshot(source_id)?;
        match snap {
            Some(s) => (
                s.etag,
                s.last_modified,
                Some(s.normalized_fingerprint),
                Some(s.items_json),
            ),
            None => (None, None, None, None),
        }
    };

    let fetcher = fetcher::create_fetcher(&source.fetch.engine, &state.cfg)?;
    let spec = FetchSpec {
        fetch: source.fetch.clone(),
        etag: prev_etag,
        last_modified: prev_lm,
        source_id: source.id.clone(),
    };
    let doc = fetcher.fetch(&spec).await?;

    if doc.not_modified {
        debug!(source = %source.id, "304 not modified");
        let db = state.db.lock().await;
        let prev = db.get_schedule_state(&source.id)?;
        db.upsert_schedule_state(&next_schedule(&source, 0, None, prev.as_ref()))?;
        return Ok(());
    }

    let out = pipeline::run_pipeline(&doc, &extract_cfg)?;

    let old_items: Vec<Item> = prev_items_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    let diff_result = differ::diff(
        prev_fp.as_deref().unwrap_or(""),
        &out.fingerprint,
        &old_items,
        &out.items,
    );

    let snapshot = SnapshotRecord {
        id: 0,
        watchpoint_id: source.id.clone(),
        fetched_at: Utc::now(),
        status: doc.status,
        etag: doc.etag.clone(),
        last_modified: doc.last_modified.clone(),
        content_sha256: doc.content_sha256.clone(),
        normalized_fingerprint: out.fingerprint.clone(),
        items_json: serde_json::to_string(&out.items)?,
        duration_ms: doc.duration_ms,
        engine: doc.engine.clone(),
    };
    {
        let db = state.db.lock().await;
        db.save_snapshot(&snapshot)?;
    }
    state
        .engine_health
        .lock()
        .await
        .insert(source.fetch.engine.clone(), true);

    // 连续无变化时清零连续变化计数。
    if !diff_result.changed {
        let db = state.db.lock().await;
        let prev = db.get_schedule_state(&source.id)?;
        db.upsert_schedule_state(&next_schedule(&source, 0, Some(Utc::now()), prev.as_ref()))?;
        debug!(source = %source.id, "no change");
        return Ok(());
    }

    // 用指纹去重：同一内容指纹（同一轮变化）只通知一次，避免重复告警。
    // 通过保留 last_notified_fingerprint（跨轮不清空）实现：
    // 即使内容在多个指纹间振荡，只要目标指纹已经通知过，就不重复轰炸。
    {
        let db = state.db.lock().await;
        let sched = db.get_schedule_state(&source.id)?;
        if let Some(sched) = sched
            && sched
                .last_notified_fingerprint
                .as_deref()
                .is_some_and(|fp| fp == diff_result.dedupe_key.as_str())
        {
            db.upsert_schedule_state(&next_schedule(&source, 0, Some(Utc::now()), Some(&sched)))?;
            debug!(source = %source.id, "duplicate change, suppressed");
            return Ok(());
        }
    }

    let event = ChangeEvent {
        id: 0,
        watchpoint_id: source.id.clone(),
        change_type: diff_result.change_type.unwrap_or(ChangeType::Updated),
        old_items_json: serde_json::to_string(&diff_result.old_items)?,
        new_items_json: serde_json::to_string(&diff_result.new_items)?,
        diff_summary: diff_result.diff_summary,
        fingerprint: diff_result.fingerprint,
        dedupe_key: diff_result.dedupe_key,
        image_urls_json: serde_json::to_string(&out.image_urls)?,
        detected_at: Utc::now(),
    };

    // 图片下载不阻塞检测：把挑选出的图片 URL 存入事件，
    // 由 notifier 在发送通知时按需下载/取缓存（见 process_outbox）。
    let event_id;
    {
        let db = state.db.lock().await;
        event_id = db.insert_change_event(&event)?;
        if state.notifier.is_some() {
            let chat_id = if state.cfg.telegram.default_chat_id.is_empty() {
                String::new()
            } else {
                state.cfg.telegram.default_chat_id.clone()
            };
            if !chat_id.is_empty() {
                let notif = crate::models::NotificationRecord {
                    id: 0,
                    event_id,
                    chat_id,
                    message_ids_json: "[]".to_string(),
                    status: "pending".to_string(),
                    attempts: 0,
                    next_retry_at: None,
                };
                db.insert_notification(&notif)?;
            }
        }
        let prev = db.get_schedule_state(&source.id)?;
        let mut sched = next_schedule(&source, 0, Some(Utc::now()), prev.as_ref());
        sched.last_notified_fingerprint = Some(event.dedupe_key.clone());
        sched.last_notified_at = Some(Utc::now());
        sched.consecutive_changes = prev
            .as_ref()
            .map(|p| p.consecutive_changes.saturating_add(1))
            .unwrap_or(1);
        db.upsert_schedule_state(&sched)?;
    }

    info!(
        source = %source.id,
        event_id,
        change_type = ?event.change_type,
        summary = %event.diff_summary,
        "change detected"
    );

    Ok(())
}

/// 计算下一轮调度状态。
///
/// - `failures`：连续失败次数（成功时为 0，会清除失败计数与退避）。
/// - `prev`：上一轮调度状态。用于**保留** `last_notified_*`，从而让
///   基于指纹的重复告警抑制跨轮生效；同时保留连续变化计数。
fn next_schedule(
    source: &crate::config::SourceConfig,
    failures: u32,
    last_success: Option<DateTime<Utc>>,
    prev: Option<&ScheduleState>,
) -> ScheduleState {
    let mut interval = source.schedule.interval_secs.max(1) as i64;
    if failures > 0 {
        interval = (interval as u64 * 2u64.pow(failures.min(6))).min(3600) as i64;
    }
    // 无变化时清零连续变化计数；有变化时由调用方递增。
    let consecutive_changes = if failures == 0 && last_success.is_some() {
        0
    } else {
        prev.map(|p| p.consecutive_changes).unwrap_or(0)
    };
    ScheduleState {
        source_id: source.id.clone(),
        next_due_at: Utc::now() + chrono::Duration::seconds(interval),
        consecutive_failures: failures,
        consecutive_changes,
        backoff_until: None,
        last_success_at: last_success,
        last_notified_fingerprint: prev.and_then(|p| p.last_notified_fingerprint.clone()),
        last_notified_at: prev.and_then(|p| p.last_notified_at),
    }
}

/// Fetch a live source from the in-memory store (SQLite-backed).
pub async fn get_live_source(state: &Arc<AppState>, source_id: &str) -> Result<SourceConfig> {
    let sources = state.sources.lock().await;
    sources
        .iter()
        .find(|s| s.id == source_id)
        .cloned()
        .ok_or_else(|| crate::error::Error::other(format!("source not found: {source_id}")))
}

/// Test a monitoring source: fetch its URL and run the configured pipeline,
/// returning the extracted items / fingerprint without persisting any snapshot
/// or change event. Used by the Web console "测试监控源" action.
pub async fn test_source(state: &Arc<AppState>, source: &SourceConfig) -> Result<Value> {
    let extract_cfg = source.extract.clone();
    let fetcher = fetcher::create_fetcher(&source.fetch.engine, &state.cfg)?;
    let spec = FetchSpec {
        fetch: source.fetch.clone(),
        etag: None,
        last_modified: None,
        source_id: source.id.clone(),
    };
    let doc = fetcher.fetch(&spec).await?;
    if doc.not_modified {
        return Ok(json!({ "not_modified": true }));
    }
    let out = pipeline::run_pipeline(&doc, &extract_cfg)?;
    Ok(json!({
        "source_id": source.id,
        "status": doc.status,
        "final_url": doc.final_url,
        "duration_ms": doc.duration_ms,
        "engine": doc.engine,
        "content_sha256": doc.content_sha256,
        "fingerprint": out.fingerprint,
        "text_len": doc.text.len(),
        "items": out.items,
    }))
}
