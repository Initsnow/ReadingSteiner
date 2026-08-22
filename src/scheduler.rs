use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use cron::Schedule as CronSchedule;
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, error, info, warn};

use crate::config::{
    ChangeType, Config, ExtractConfig, ImageSelector, ItemSelector, RuntimeConfig, SourceConfig,
};
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
    /// 当前生效的 config 文件路径（供设置持久化），缺省时无法回写。
    pub config_path: Option<PathBuf>,
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
        Self::with_config_path(cfg, None)
    }

    pub fn with_config_path(cfg: Config, config_path: Option<PathBuf>) -> Result<Self> {
        let runtime = RuntimeConfig::from_config(&cfg);
        std::fs::create_dir_all(&runtime.state_dir)?;
        let db_path = runtime.state_dir.join("reading-steiner.db");
        let db = Db::open(db_path)?;
        let notifier = match TelegramNotifier::new(&cfg.telegram, &runtime.timezone) {
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
            config_path,
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
        let now = Utc::now();
        let tz = self.runtime.timezone.clone();
        DaemonStatus {
            running: self.running.load(Ordering::Relaxed),
            version: env!("CARGO_PKG_VERSION").to_string(),
            sources: sources.len(),
            enabled_sources: enabled,
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
            last_tick_at: last_tick,
            engine_health,
            timezone: tz.clone(),
            server_time_utc: now,
            server_time_local: format_local_time(now, &tz),
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
                    failure_notified: false,
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
            let tags = db.list_tags().unwrap_or_default();
            let mut due: Vec<SourceConfig> = Vec::new();
            let sources = state.sources.lock().await;
            for source in sources.iter() {
                // 解析分组继承后的生效监控开关（分组关闭或自身关闭则跳过调度）。
                let (enabled, _, _) =
                    crate::config::resolve_effective_source(source, &tags, 0);
                if !enabled {
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
                    failure_notified: false,
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
            // 有界队列：每 tick 最多入队 queue_capacity 个任务，超出部分下个 tick 再处理。
            due.truncate(state.runtime.queue_capacity.max(1));
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
                        // 连续失败达到阈值时，发送一条失败通知（同一段失败连击只发一次）。
                        let threshold = state.runtime.failure_notify_threshold;
                        if threshold > 0
                            && sched.consecutive_failures >= threshold
                            && !sched.failure_notified
                        {
                            let chat_id = &state.cfg.telegram.default_chat_id;
                            if !chat_id.is_empty() {
                                let tz = state.runtime.timezone.clone();
                                let text = notifier::render_failure_message(
                                    &source.id,
                                    sched.consecutive_failures,
                                    threshold,
                                    &e.to_string(),
                                    &tz,
                                );
                                let _ = db.insert_system_notification(chat_id, &text);
                                sched.failure_notified = true;
                            }
                        }
                        let _ = db.upsert_schedule_state(&sched);
                    }
                }
            });
        }

        // Drain notification outbox periodically（含事件通知与系统告警）。
        if let Some(notifier) = state.notifier.clone() {
            let db = state.db.clone();
            let images = state.images.clone();
            if let Err(e) = notifier::process_outbox(&db, &images, &notifier, None).await {
                warn!(error = %e, "outbox processing failed");
            }
            // 按每个监控源保留条数限制清理历史（事件与快照）。
            // 解析分组继承后的生效保留条数：若分组配置了更严格的保留策略，
            // 则优先按分组的限制清理，否则使用全局限制。
            let db = db.lock().await;
            let tags = db.list_tags().unwrap_or_default();
            let global_limit = state.runtime.history_limit_per_source;
            // 快速路径：没有任何分组配置历史限制（全部跟随全局）时，用一次全表清理，
            // 避免对每个源逐条执行 DELETE 带来额外开销（与旧实现的单条 SQL 相当）。
            let any_group_limit = tags.iter().any(|t| t.history_limit > 0);
            let sources = state.sources.lock().await;
            if !any_group_limit {
                if global_limit > 0
                    && let Err(e) = db.prune_history(global_limit)
                {
                    warn!(error = %e, "history pruning failed");
                }
            } else {
                for source in sources.iter() {
                    let (_, _, history) =
                        crate::config::resolve_effective_source(source, &tags, global_limit);
                    if history > 0
                        && let Err(e) = db.prune_history_for_source(&source.id, history)
                    {
                        warn!(source = %source.id, error = %e, "history pruning failed");
                    }
                }
            }
            drop(db);
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
        db.upsert_schedule_state(&next_schedule(
            &source,
            0,
            None,
            false,
            prev.as_ref(),
            &state.runtime.timezone,
            &state.runtime.default_cron,
        ))?;
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
        db.upsert_schedule_state(&next_schedule(
            &source,
            0,
            Some(Utc::now()),
            false,
            prev.as_ref(),
            &state.runtime.timezone,
            &state.runtime.default_cron,
        ))?;
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
            db.upsert_schedule_state(&next_schedule(
                &source,
                0,
                Some(Utc::now()),
                true,
                Some(&sched),
                &state.runtime.timezone,
                &state.runtime.default_cron,
            ))?;
            debug!(source = %source.id, "duplicate change, suppressed");
            return Ok(());
        }
    }

    // 图片来源：若选用 `Changed`（只发变更元素相关图片），则根据 diff 计算变更元素图片。
    let mut image_urls = out.image_urls.clone();
    if matches!(
        extract_cfg,
        ExtractConfig::Items {
            images: Some(ImageSelector::Changed),
            ..
        }
    ) {
        // 本次变更中新增 / 更新的条目 stable_id。
        let changed_ids: std::collections::HashSet<String> = {
            let old_ids: std::collections::HashSet<&str> = diff_result
                .old_items
                .iter()
                .map(|i| i.stable_id.as_str())
                .collect();
            diff_result
                .new_items
                .iter()
                .filter(|i| {
                    !old_ids.contains(i.stable_id.as_str())
                        || old_items.iter().any(|o| {
                            o.stable_id == i.stable_id && o.fingerprint(&[]) != i.fingerprint(&[])
                        })
                })
                .map(|i| i.stable_id.clone())
                .collect()
        };
        if let ExtractConfig::Items {
            selector, fields, ..
        } = &extract_cfg
        {
            // Changed 模式依赖 CSS 定位 HTML 元素；JSONPath 源无法定位元素，
            // 会静默丢弃全部图片，此时回退到整页 image_urls，避免丢图。
            if matches!(selector, ItemSelector::Css { .. }) {
                image_urls =
                    pipeline::collect_changed_element_images(&doc, selector, fields, &changed_ids);
            }
        }
    }

    // camofox 源开启截图时，先把截图数据暂存（插入事件拿到 event_id 后再写文件，
    // 文件命名 `event-{id}.png`，落在 media_dir/screenshots/ 下）。
    // 写入失败时不引用不存在的文件（screenshot_path 置为 None）。
    let screenshot_data = if source.fetch.engine == "camofox"
        && source.fetch.screenshot
        && let Some(data) = &doc.screenshot
    {
        Some(data.clone())
    } else {
        None
    };

    let event = ChangeEvent {
        id: 0,
        watchpoint_id: source.id.clone(),
        change_type: diff_result.change_type.unwrap_or(ChangeType::Updated),
        old_items_json: serde_json::to_string(&diff_result.old_items)?,
        new_items_json: serde_json::to_string(&diff_result.new_items)?,
        diff_summary: diff_result.diff_summary,
        fingerprint: diff_result.fingerprint,
        dedupe_key: diff_result.dedupe_key,
        image_urls_json: serde_json::to_string(&image_urls)?,
        detected_at: Utc::now(),
        read: false,
        screenshot_path: None,
    };

    // 图片下载不阻塞检测：把挑选出的图片 URL 存入事件，
    // 由 notifier 在发送通知时按需下载/取缓存（见 process_outbox）。
    let event_id;
    {
        let db = state.db.lock().await;
        event_id = db.insert_change_event(&event)?;
        // 插入成功后再写截图：以 event_id 命名，写失败时事件不带截图，
        // 不存在残留临时文件，也不存在 DB 与文件名不一致的问题。
        if let Some(data) = &screenshot_data {
            let dir = state.runtime.media_dir.join("screenshots");
            match std::fs::create_dir_all(&dir) {
                Ok(()) => {
                    let fname = format!("event-{event_id}.png");
                    let path = dir.join(&fname);
                    match std::fs::write(&path, data) {
                        Ok(()) => {
                            if let Err(e) = db.update_event_screenshot(
                                event_id,
                                Some(&format!("screenshots/{fname}")),
                            ) {
                                // DB 更新失败：清理已写入的文件，避免引用不存在的文件。
                                tracing::warn!(
                                    error = %e, event_id,
                                    "failed to set screenshot path in db; removing file"
                                );
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e, event_id,
                                "failed to write screenshot; event will have no screenshot"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e, event_id,
                        "failed to create screenshots dir; event will have no screenshot"
                    );
                }
            }
        }
        // 仅当该源开启了通知（解析分组继承后的生效通知开关）且已配置 notifier
        // 与默认 chat 时才排队发送。
        let (_, effective_notify, _) = {
            let db = state.db.lock().await;
            let tags = db.list_tags().unwrap_or_default();
            crate::config::resolve_effective_source(&source, &tags, 0)
        };
        if effective_notify && state.notifier.is_some() {
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
        let mut sched = next_schedule(
            &source,
            0,
            Some(Utc::now()),
            true,
            prev.as_ref(),
            &state.runtime.timezone,
            &state.runtime.default_cron,
        );
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

/// 把标准 cron 的星期值（0-7，0/7=周日）转为 cron crate 的星期值（1-7，1=周日）。
/// 不支持命名形式（SUN/MON 等）——它们会原样透传，由 cron crate 解析。
fn map_cron_dow_num(v: i32) -> i32 {
    match v {
        0 | 7 => 1,  // 周日 → 1
        n if (1..=6).contains(&n) => n + 1, // 周一~周六 → 2~7
        _ => v,
    }
}

/// 在标准 cron 星期环（0-6，0/7=周日）上展开范围 `a-b`，返回标准 cron 星期值序列。
/// 支持跨周范围（如 `5-7` → [5,6,0]，`6-1` → [6,0,1]），`7` 视为周日（0）。
fn expand_std_dow_range(a: i32, b: i32) -> Vec<i32> {
    let norm = |v: i32| if v == 7 { 0 } else { v };
    let na = norm(a);
    let nb = norm(b);
    let mut out = Vec::new();
    if na <= nb {
        for v in na..=nb {
            out.push(v);
        }
    } else {
        // 跨周：a..6 再接 0..b
        for v in na..=6 {
            out.push(v);
        }
        for v in 0..=nb {
            out.push(v);
        }
    }
    out
}

/// 把标准 cron 星期值列表映射为 cron crate 星期值（1-7，1=周日）字符串列表。
fn map_dow_list(values: &[i32]) -> Vec<String> {
    values
        .iter()
        .map(|&v| map_cron_dow_num(v).to_string())
        .collect()
}

/// 把标准 cron 的星期字段（第 5 段）转为 cron crate 的星期字段。
/// 支持 `*`、`n`、`a-b`、`a-b/n`、`*/n`、`n/n`、逗号列表（含范围）。
/// 正确处理跨周范围（如 `5-7` 周五~周日、`6-1` 周六~周一）。
/// 命名（SUN/MON 等）原样透传，由 cron crate 自己解析。
fn convert_dow_field(field: &str) -> Result<String> {
    let field = field.trim();
    if field == "*" {
        return Ok("*".to_string());
    }
    // 命名形式（如 SUN、MON）直接透传，由 cron crate 自己解析（sun=1, mon=2...）。
    if field.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(field.to_string());
    }

    let mut out: Vec<String> = Vec::new();
    for item in field.split(',') {
        let item = item.trim();
        // 形如 a-b/n 或 a-b
        if let Some((range_part, step)) = item.split_once('/') {
            let step: i32 = step.parse().map_err(|_| {
                crate::error::Error::other(format!("无效的步进值: '{item}'"))
            })?;
            if step < 1 {
                return Err(crate::error::Error::other(format!(
                    "步进值必须为正: '{item}'"
                )));
            }
            if range_part == "*" {
                // 标准环 0-6 到 cron crate 1-7 是线性偏移，*/n 结构保持不变，直接透传。
                out.push(format!("*/{step}"));
            } else if let Some((a, b)) = range_part.split_once('-') {
                let a: i32 = a.trim().parse().map_err(|_| {
                    crate::error::Error::other(format!("无效的星期值: '{a}'"))
                })?;
                let b: i32 = b.trim().parse().map_err(|_| {
                    crate::error::Error::other(format!("无效的星期值: '{b}'"))
                })?;
                let vals = expand_std_dow_range(a, b);
                // 范围 + 步进：在展开后的序列上按 step 取样。
                let stepped: Vec<i32> = vals.iter().step_by(step as usize).copied().collect();
                out.push(map_dow_list(&stepped).join(","));
            } else {
                // 单个值 + 步进（Vixie 语义，如 1/2 = 周一/三/五）：
                // 从起点在标准环上每隔 step 取一个值。
                let v: i32 = range_part.trim().parse().map_err(|_| {
                    crate::error::Error::other(format!("无效的星期值: '{range_part}'"))
                })?;
                let start = if v == 7 { 0 } else { v };
                let mut vals = Vec::new();
                let mut cur = start;
                while cur <= 6 {
                    vals.push(cur);
                    cur += step;
                }
                out.push(map_dow_list(&vals).join(","));
            }
        } else if let Some((a, b)) = item.split_once('-') {
            let a: i32 = a.trim().parse().map_err(|_| {
                crate::error::Error::other(format!("无效的星期值: '{a}'"))
            })?;
            let b: i32 = b.trim().parse().map_err(|_| {
                crate::error::Error::other(format!("无效的星期值: '{b}'"))
            })?;
            if a == b {
                out.push(map_cron_dow_num(a).to_string());
            } else {
                let vals = expand_std_dow_range(a, b);
                out.push(map_dow_list(&vals).join(","));
            }
        } else {
            // 单个值
            let v: i32 = item.parse().map_err(|_| {
                crate::error::Error::other(format!("无效的星期值: '{item}'"))
            })?;
            out.push(map_cron_dow_num(v).to_string());
        }
    }
    Ok(out.join(","))
}

/// 把标准 5 段 cron 表达式（`分 时 日 月 周`）转为 cron crate 的 7 段格式
/// （`秒 分 时 日 月 周 年`），秒固定为 0、年不限。
/// 星期字段做标准 cron（0/7=周日）→ cron crate（1=周日）的映射。
/// 失败时返回描述性错误（原始表达式附在消息里）。
fn cron_5field_to_7field(expr: &str) -> Result<String> {
    let expr = expr.trim();
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(crate::error::Error::other(format!(
            "cron 表达式需要 5 段（分 时 日 月 周），实际得到 {} 段: '{expr}'",
            parts.len()
        )));
    }
    let dow = convert_dow_field(parts[4])?;
    // 标准 5 段 → 7 段：前插 0（秒），末尾追加 *（年不限）。
    Ok(format!("0 {} {} {} {} {} *", parts[0], parts[1], parts[2], parts[3], dow))
}

/// 按 cron 表达式计算下一次应触发的时间（配置时区的本地时间）。
/// `after` 为当前时间，返回严格晚于它的下一个匹配时刻。
/// 解析失败或没有下一次触发时返回 Err。
fn next_cron_due(expr: &str, tz: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let seven = cron_5field_to_7field(expr)?;
    let sched = CronSchedule::from_str(&seven).map_err(|e| {
        crate::error::Error::other(format!("cron 表达式解析失败 '{expr}': {e}"))
    })?;
    // 优先使用配置的 IANA 时区；解析失败时回退到系统本地时区。
    let next = match tz.parse::<chrono_tz::Tz>() {
        Ok(zone) => sched
            .after(&after.with_timezone(&zone))
            .take(1)
            .next()
            .map(|t| t.with_timezone(&Utc)),
        Err(_) => sched
            .after(&after.with_timezone(&Local))
            .take(1)
            .next()
            .map(|t| t.with_timezone(&Utc)),
    };
    next.ok_or_else(|| {
        crate::error::Error::other(format!("cron 表达式没有可用的下一次触发时间: '{expr}'"))
    })
}

/// 计算下一轮调度状态。
///
/// - `failures`：连续失败次数（成功时为 0，会清除失败计数与退避）。
/// - `last_success`：本次抓取是否成功（Some(时间)）——用于记录最后成功时间。
/// - `had_change`：本次抓取是否检测到内容变化。用于正确维护连续变化计数：
///   有变化时保留/递增；无变化时清零。**重复变化（被抑制）也视为"有变化"**，
///   避免连续变化计数被意外清零。
/// - `prev`：上一轮调度状态。用于**保留** `last_notified_*`，从而让
///   基于指纹的重复告警抑制跨轮生效。
/// - `tz`：cron 表达式使用的 IANA 时区名称。
///
/// 调度完全由源的 `schedule.cron` 表达式驱动：按 cron 精确计算下一次触发时间；
/// 表达式为空或无效时退化为 60s 短间隔重试，避免 daemon 因单个源卡死。
fn next_schedule(
    source: &crate::config::SourceConfig,
    failures: u32,
    last_success: Option<DateTime<Utc>>,
    had_change: bool,
    prev: Option<&ScheduleState>,
    tz: &str,
    default_cron: &str,
) -> ScheduleState {
    // 连续变化计数：本次有变化时保留/递增；无变化时清零。
    let consecutive_changes = if had_change {
        prev.map(|p| p.consecutive_changes.saturating_add(1))
            .unwrap_or(1)
    } else {
        0
    };

    // —— cron 表达式驱动：按表达式精确调度。 ——
    // 监控源未配置 cron 时使用全局默认 cron（default_cron）；
    // default_cron 也为空时回退到每小时（与 effective_cron() 保持一致）。
    let expr = source
        .schedule
        .cron
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let d = default_cron.trim();
            if d.is_empty() { None } else { Some(d) }
        })
        .unwrap_or("0 * * * *");
    let next_due_at = match next_cron_due(expr, tz, Utc::now()) {
        Ok(t) => t,
        Err(e) => {
            // 表达式缺失或无效时退化为短间隔重试，避免 daemon 因单个源卡死。
            warn!(source = %source.id, error = %e, "invalid cron, falling back to 60s retry");
            Utc::now() + chrono::Duration::seconds(60)
        }
    };

    ScheduleState {
        source_id: source.id.clone(),
        next_due_at,
        consecutive_failures: failures,
        consecutive_changes,
        backoff_until: None,
        last_success_at: last_success,
        last_notified_fingerprint: prev.and_then(|p| p.last_notified_fingerprint.clone()),
        last_notified_at: prev.and_then(|p| p.last_notified_at),
        // 成功（无失败）路径会将失败通知标记清零。
        failure_notified: failures == 0,
    }
}

/// 将 UTC 时间按指定 IANA 时区格式化为 `%Y-%m-%d %H:%M:%S` 的本地时间字符串。
/// 时区无法解析时退回 UTC。
pub fn format_local_time(t: DateTime<Utc>, tz: &str) -> String {
    match tz.parse::<chrono_tz::Tz>() {
        Ok(zone) => t
            .with_timezone(&zone)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => t.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_cron_5field_to_7field() {
        // 标准 1-5 = 周一~周五 → cron crate 2,3,4,5,6。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 1-5").unwrap(),
            "0 0 9 * * 2,3,4,5,6 *"
        );
        assert_eq!(cron_5field_to_7field("*/15 * * * *").unwrap(), "0 */15 * * * * *");
        // 标准 0,6 = 周日,周六 → cron crate 1,7。
        assert_eq!(cron_5field_to_7field("30 8,20 * * 0,6").unwrap(), "0 30 8,20 * * 1,7 *");
        // 7 也代表周日 → 1。
        assert_eq!(cron_5field_to_7field("0 9 * * 7").unwrap(), "0 0 9 * * 1 *");
    }

    #[test]
    fn test_cron_5field_to_7field_cross_week_range() {
        // 跨周范围 5-7：周五(5)周六(6)周日(0) → cron crate 6,7,1。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 5-7").unwrap(),
            "0 0 9 * * 6,7,1 *"
        );
        // 跨周范围 6-1：周六(6)周日(0)周一(1) → cron crate 7,1,2。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 6-1").unwrap(),
            "0 0 9 * * 7,1,2 *"
        );
        // 完整周 0-6 → cron crate 1..7。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 0-6").unwrap(),
            "0 0 9 * * 1,2,3,4,5,6,7 *"
        );
    }

    #[test]
    fn test_cron_5field_to_7field_steps() {
        // 单值步进 1/2（Vixie）：周一(1)/三(3)/五(5) → cron crate 2,4,6。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 1/2").unwrap(),
            "0 0 9 * * 2,4,6 *"
        );
        // 范围步进 1-5/2：1,3,5 → cron crate 2,4,6。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 1-5/2").unwrap(),
            "0 0 9 * * 2,4,6 *"
        );
        // 跨周范围步进 5-1/2：周五(5)周日(0) → cron crate 6,1。
        assert_eq!(
            cron_5field_to_7field("0 9 * * 5-1/2").unwrap(),
            "0 0 9 * * 6,1 *"
        );
    }

    #[test]
    fn test_cron_5field_to_7field_bad_arity() {
        assert!(cron_5field_to_7field("0 9 * *").is_err());
        assert!(cron_5field_to_7field("0 9 * * 1 2 3").is_err());
        assert!(cron_5field_to_7field("").is_err());
    }

    #[test]
    fn test_next_cron_due_daily() {
        // 每天 09:30 UTC，给定 after=2026-01-01 00:00，应得到 2026-01-01 09:30。
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let due = next_cron_due("30 9 * * *", "UTC", after).unwrap();
        assert_eq!(due, Utc.with_ymd_and_hms(2026, 1, 1, 9, 30, 0).unwrap());
    }

    #[test]
    fn test_next_cron_due_next_day_when_past() {
        // 每天 09:30，after 已过 09:30，应得到次日 09:30。
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let due = next_cron_due("30 9 * * *", "UTC", after).unwrap();
        assert_eq!(due, Utc.with_ymd_and_hms(2026, 1, 2, 9, 30, 0).unwrap());
    }

    #[test]
    fn test_next_cron_due_weekday() {
        // 2026-01-01 是周四。工作日（1-5）每天 09:00。
        // after = 周四 08:00 → 周四 09:00。
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        let due = next_cron_due("0 9 * * 1-5", "UTC", after).unwrap();
        assert_eq!(due, Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap());

        // after = 周五 10:00 → 下周一 09:00。
        let after2 = Utc.with_ymd_and_hms(2026, 1, 2, 10, 0, 0).unwrap();
        let due2 = next_cron_due("0 9 * * 1-5", "UTC", after2).unwrap();
        assert_eq!(due2, Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap());
    }

    #[test]
    fn test_next_cron_due_every_15_min() {
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 10, 7, 0).unwrap();
        let due = next_cron_due("*/15 * * * *", "UTC", after).unwrap();
        // 10:07 → 下一个 15 分钟边界为 10:15。
        assert_eq!(due, Utc.with_ymd_and_hms(2026, 1, 1, 10, 15, 0).unwrap());
    }

    #[test]
    fn test_next_cron_due_invalid_expr() {
        assert!(next_cron_due("61 * * * *", "UTC", Utc::now()).is_err());
        assert!(next_cron_due("bad", "UTC", Utc::now()).is_err());
    }
}
