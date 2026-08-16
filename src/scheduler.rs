use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, error, info, warn};

use crate::config::{ChangeType, Config, RuntimeConfig};
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
        Ok(Self {
            cfg,
            runtime,
            db: Arc::new(Mutex::new(db)),
            notifier,
            images,
            running: AtomicBool::new(false),
            queue_depth: AtomicUsize::new(0),
            last_tick_at: Mutex::new(None),
            engine_health: Mutex::new(HashMap::new()),
        })
    }

    pub async fn status(&self) -> DaemonStatus {
        let db = self.db.lock().await;
        let sources = db.list_sources().unwrap_or_default();
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
    info!(
        sources = state.cfg.sources.len(),
        "ReadingSteiner daemon started"
    );

    // Persist config sources so CLI/status can see them even if config changes later.
    {
        let db = state.db.lock().await;
        for source in &state.cfg.sources {
            db.upsert_source(source)?;
        }
        let now = Utc::now();
        for source in &state.cfg.sources {
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
            for source in &state.cfg.sources {
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
            let db_guard = state.db.lock().await;
            if let Err(e) = notifier::process_outbox(&db_guard, &notifier, None).await {
                warn!(error = %e, "outbox processing failed");
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(())
}

pub async fn check_source(state: &Arc<AppState>, source_id: &str) -> Result<()> {
    let source = state
        .cfg
        .source(source_id)
        .cloned()
        .ok_or_else(|| crate::error::Error::other(format!("source not found: {source_id}")))?;
    let pipeline_cfg = state
        .cfg
        .pipeline(&source.pipeline)
        .cloned()
        .ok_or_else(|| {
            crate::error::Error::config(format!("pipeline not found: {}", source.pipeline))
        })?;

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
        db.upsert_schedule_state(&next_schedule(&source, 0, None))?;
        return Ok(());
    }

    let out = pipeline::run_pipeline(&doc, &pipeline_cfg)?;

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
        &source.compare,
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

    if !diff_result.changed {
        let db = state.db.lock().await;
        db.upsert_schedule_state(&next_schedule(&source, 0, Some(Utc::now())))?;
        debug!(source = %source.id, "no change");
        return Ok(());
    }
    if !source
        .compare
        .notify_on
        .contains(&diff_result.change_type.unwrap_or(ChangeType::Updated))
    {
        let db = state.db.lock().await;
        db.upsert_schedule_state(&next_schedule(&source, 0, Some(Utc::now())))?;
        info!(source = %source.id, change_type = ?diff_result.change_type, "change suppressed by notify_on");
        return Ok(());
    }

    // Consecutive-confirm noise gate: only notify after N consecutive changes.
    let confirm_count = source.compare.confirm_count.max(1);
    if confirm_count > 1 {
        let db = state.db.lock().await;
        let mut sched = db.get_schedule_state(&source.id)?.unwrap_or(ScheduleState {
            source_id: source.id.clone(),
            next_due_at: Utc::now(),
            consecutive_failures: 0,
            consecutive_changes: 0,
            backoff_until: None,
            last_success_at: Some(Utc::now()),
            last_notified_fingerprint: None,
            last_notified_at: None,
        });
        sched.consecutive_changes += 1;
        if sched.consecutive_changes < confirm_count as u32 {
            sched.next_due_at =
                Utc::now() + chrono::Duration::seconds(source.schedule.interval_secs.max(1) as i64);
            sched.consecutive_failures = 0;
            db.upsert_schedule_state(&sched)?;
            info!(
                source = %source.id,
                consecutive = sched.consecutive_changes,
                confirm_count,
                "change observed but below confirmation threshold"
            );
            return Ok(());
        }
    }

    // Fingerprint cooldown: suppress the same dedupe key repeatedly within the cooldown window.
    if source.compare.cooldown_secs > 0 {
        let db = state.db.lock().await;
        let sched = db.get_schedule_state(&source.id)?;
        if let Some(sched) = sched {
            let within_cooldown = sched
                .last_notified_at
                .map(|t| {
                    Utc::now().signed_duration_since(t).num_seconds()
                        < source.compare.cooldown_secs as i64
                })
                .unwrap_or(false);
            if within_cooldown
                && sched
                    .last_notified_fingerprint
                    .as_deref()
                    .is_some_and(|fp| fp == diff_result.dedupe_key.as_str())
            {
                db.upsert_schedule_state(&next_schedule(&source, 0, Some(Utc::now())))?;
                info!(source = %source.id, "change suppressed by fingerprint cooldown");
                return Ok(());
            }
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
        detected_at: Utc::now(),
    };

    let mut image_entries = Vec::new();
    for img in &out.items {
        for url in &img.image_urls {
            let image_ref = crate::models::ImageRef {
                canonical_url: url.clone(),
                alt: img.fields.get("alt").cloned().unwrap_or_default(),
                width: None,
                height: None,
            };
            if let Some(entry) = state.images.ensure(&state.db, &image_ref).await? {
                image_entries.push(entry);
            }
        }
    }

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
        let mut sched = next_schedule(&source, 0, Some(Utc::now()));
        sched.last_notified_fingerprint = Some(event.dedupe_key.clone());
        sched.last_notified_at = Some(Utc::now());
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

fn next_schedule(
    source: &crate::config::SourceConfig,
    failures: u32,
    last_success: Option<DateTime<Utc>>,
) -> ScheduleState {
    let mut interval = source.schedule.interval_secs.max(1) as i64;
    if failures > 0 {
        interval = (interval as u64 * 2u64.pow(failures.min(6))).min(3600) as i64;
    }
    ScheduleState {
        source_id: source.id.clone(),
        next_due_at: Utc::now() + chrono::Duration::seconds(interval),
        consecutive_failures: failures,
        consecutive_changes: 0,
        backoff_until: None,
        last_success_at: last_success,
        last_notified_fingerprint: None,
        last_notified_at: None,
    }
}
