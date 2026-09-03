use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, error, info, warn};

use crate::config::{
    ChangeType, Config, DEFAULT_CRON, EditableSettings, ExtractConfig, ImageSelector, ItemSelector,
    RuntimeConfig, SourceConfig,
};
use crate::db::Db;
use crate::differ;
use crate::error::{Error, Result};
use crate::fetcher::{self, FetchSpec};
use crate::images::ImageDownloader;
use crate::models::{ChangeEvent, DaemonStatus, Item, ScheduleState, SnapshotRecord};
use crate::notifier::{self, TelegramNotifier};
use crate::pipeline;

pub struct AppState {
    pub cfg: Config,
    /// 可热更新的运行时设置（全部字段在保存后即时刷新，详见
    /// `reload_settings`）。线程安全地供 daemon 循环每轮重新读取。
    pub runtime: Arc<RwLock<RuntimeConfig>>,
    /// 全局可编辑设置的内存视图，作为热更新 / 校验的单一来源；启动时从 SQLite 装载，
    /// 保存时由 `reload_settings` 刷新。
    pub settings: Arc<RwLock<EditableSettings>>,
    /// 当前生效的 config 文件路径（供设置持久化），缺省时无法回写。
    pub config_path: Option<PathBuf>,
    pub db: Arc<Mutex<Db>>,
    /// Live monitoring sources. Solely backed by the SQLite `sources` table,
    /// kept in sync as sources are added / edited / deleted via the Web/CLI.
    pub sources: Mutex<Vec<SourceConfig>>,
    /// 通知器（可热更新：url / 模板 / 图片数 / 时区变更后重建）。
    pub notifier: Arc<RwLock<Option<Arc<TelegramNotifier>>>>,
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
        // SQLite 是运行参数的唯一来源：config.yaml 只提供启动引导项（目录、socket、camofox、
        // telegram api_base 等），可编辑设置从 SQLite `settings` 表读取（迁移时已 seed 默认值）。
        std::fs::create_dir_all(&cfg.state_dir)?;
        let db_path = cfg.state_dir.join("reading-steiner.db");
        let db = Db::open(db_path)?;
        let settings = db
            .get_settings()?
            .ok_or_else(|| Error::other("settings not seeded"))?;
        let runtime = RuntimeConfig::from_parts(&cfg, &settings);
        // notifier 的 url / 模板 / 图片数以 SQLite 设置为准，api_base 等取自启动项。
        let telegram = cfg.telegram.clone().with_overrides(&settings);
        let notifier = match TelegramNotifier::new(&telegram, &runtime.timezone) {
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
            runtime: Arc::new(RwLock::new(runtime)),
            settings: Arc::new(RwLock::new(settings)),
            config_path,
            db: Arc::new(Mutex::new(db)),
            sources: Mutex::new(sources),
            notifier: Arc::new(RwLock::new(notifier)),
            images,
            running: AtomicBool::new(false),
            queue_depth: AtomicUsize::new(0),
            last_tick_at: Mutex::new(None),
            engine_health: Mutex::new(HashMap::new()),
        })
    }

    /// 运行时配置快照（热更新后即时反映新值）。
    fn runtime_snapshot(&self) -> RuntimeConfig {
        self.runtime.read().unwrap().clone()
    }

    /// 全局可编辑设置快照（fetcher 的 UA / 超时从这里读）。
    pub(crate) fn settings_snapshot(&self) -> EditableSettings {
        self.settings.read().unwrap().clone()
    }

    /// 当前生效的时区（IANA 名称）。
    pub(crate) fn timezone(&self) -> String {
        self.runtime.read().unwrap().timezone.clone()
    }

    /// 状态目录（备份等落盘位置）。
    pub(crate) fn state_dir(&self) -> PathBuf {
        self.runtime.read().unwrap().state_dir.clone()
    }

    /// 媒体目录（图片缓存 / 截图）。
    pub(crate) fn media_dir(&self) -> PathBuf {
        self.runtime.read().unwrap().media_dir.clone()
    }

    /// 当前生效的全局 Telegram 通知目标（`tgram://` URL，支持热更新）。
    pub(crate) fn telegram_url(&self) -> String {
        self.settings.read().unwrap().telegram_url.clone()
    }

    /// 全部分组（标签）配置。
    pub(crate) async fn tags(&self) -> Vec<crate::models::TagConfig> {
        self.db.lock().await.list_tags().unwrap_or_default()
    }

    pub async fn status(&self) -> DaemonStatus {
        let sources = self.sources.lock().await;
        let enabled = sources.iter().filter(|s| s.enabled).count();
        let last_tick = *self.last_tick_at.lock().await;
        let engine_health = self.engine_health.lock().await.clone();
        let now = Utc::now();
        let tz = self.timezone();
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
            server_time_local: crate::cron_expr::format_local(now, &tz),
        }
    }

    /// 设置保存后调用：把新设置写入内存视图，并全部热更新到 runtime / notifier。
    ///
    /// 并发数由 daemon 调度循环在每轮按 runtime.concurrency 动态调整信号量；
    /// 队列容量在每轮入队时读取，因此同样即时生效。
    pub fn reload_settings(&self, settings: &EditableSettings) {
        // 更新内存设置视图（fetcher 的 UA / 超时从这里读）。
        *self.settings.write().unwrap() = settings.clone();
        // 刷新 runtime 中全部可热更新字段（含并发数 / 队列容量）。
        {
            let mut rt = self.runtime.write().unwrap();
            rt.concurrency = settings.concurrency.max(1);
            rt.queue_capacity = settings.queue_capacity.max(1);
            rt.failure_notify_threshold = settings.failure_notify_threshold;
            rt.history_limit_per_source = settings.history_limit_per_source;
            rt.timezone = if settings.timezone.trim().is_empty() {
                crate::config::system_local_timezone()
            } else {
                settings.timezone.clone()
            };
            rt.default_cron = settings.default_cron.clone();
        }
        // 重建 notifier：url / 模板 / 图片数 / 时区变更即时生效。
        // 重建失败时保留旧 notifier，避免通知功能被非法配置整体关闭。
        let new_notifier = TelegramNotifier::new(
            &self.cfg.telegram.clone().with_overrides(settings),
            &self.runtime.read().unwrap().timezone,
        );
        match new_notifier {
            Ok(n) => {
                let mut guard = self.notifier.write().unwrap();
                *guard = Some(Arc::new(n));
                info!("global settings hot-reloaded (notifier rebuilt)");
            }
            Err(e) => {
                // 重建失败（如通知目标 url 为空或非法）：保留旧 notifier，避免通知被静默关闭。
                warn!(error = %e, "notifier hot-reload failed, keeping previous notifier");
            }
        }
    }
}

/// 调度主循环：每 500ms 扫一轮，把到期的监控源丢进并发任务执行。
///
/// 每轮做四件事：
/// 1. 按 runtime 最新并发数动态调整信号量（并发热更新）；
/// 2. 捞出到期且不在退避中的监控源，按队列容量截断后并发执行检测；
/// 3. 排空通知发件箱（变更通知 + 系统告警）；
/// 4. 每 60s 做一次历史清理（节流，避免高频全表 DELETE）。
pub async fn run_daemon(state: Arc<AppState>) -> Result<()> {
    state.running.store(true, Ordering::Relaxed);
    info!(sources = state.sources.lock().await.len(), "daemon started");
    init_schedule_states(&state).await?;

    let semaphore = Arc::new(Semaphore::new(state.runtime_snapshot().concurrency.max(1)));
    // 记录当前信号量的许可数，用于每轮动态增减（实现并发数热更新）。
    let mut current_concurrency = state.runtime_snapshot().concurrency.max(1);
    let mut last_prune = Instant::now();

    while state.running.load(Ordering::Relaxed) {
        *state.last_tick_at.lock().await = Some(Utc::now());

        current_concurrency = sync_concurrency(&semaphore, current_concurrency, &state);

        let Some(sched_map) = load_schedule_states(&state).await else {
            tokio::time::sleep(TICK_INTERVAL).await;
            continue;
        };

        let due = collect_due_sources(&state, &sched_map).await;
        state.queue_depth.store(due.len(), Ordering::Relaxed);
        for source in due {
            let state = state.clone();
            let semaphore = semaphore.clone();
            tokio::spawn(async move {
                let _permit = semaphore.acquire_owned().await;
                if let Err(e) = check_source(&state, &source.id).await {
                    record_failure(&state, &source.id, &e).await;
                }
            });
        }

        drain_outbox(&state).await;
        last_prune = maybe_prune_history(&state, last_prune).await;

        tokio::time::sleep(TICK_INTERVAL).await;
    }
    Ok(())
}

/// 主循环每轮的间隔。
const TICK_INTERVAL: Duration = Duration::from_millis(500);
/// 历史清理的节流间隔。
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
/// 连续失败退避的基数与上限指数（退避 = BASE * 2^min(失败数, MAX_EXP)）。
const BACKOFF_BASE_SECS: i64 = 30;
const BACKOFF_MAX_EXP: u32 = 5;

/// 首次启动时为所有监控源补一条调度状态，使其在第一轮即到期。
async fn init_schedule_states(state: &Arc<AppState>) -> Result<()> {
    let db = state.db.lock().await;
    let now = Utc::now();
    let sources = state.sources.lock().await;
    for source in sources.iter() {
        if db.get_schedule_state(&source.id)?.is_none() {
            db.upsert_schedule_state(&ScheduleState {
                source_id: source.id.clone(),
                next_due_at: now + chrono::Duration::seconds(1),
                ..ScheduleState::default()
            })?;
        }
    }
    Ok(())
}

/// 把信号量的许可数收敛到 runtime 的最新并发数，返回收敛后的值。
///
/// `forget_permits` 可能因仍有任务持有许可而未完全减少，故用其返回值修正计数，
/// 让后续循环继续向目标收敛。
fn sync_concurrency(semaphore: &Semaphore, current: usize, state: &AppState) -> usize {
    let target = state.runtime_snapshot().concurrency.max(1);
    if target == current {
        return current;
    }
    let next = if target > current {
        semaphore.add_permits(target - current);
        target
    } else {
        current - semaphore.forget_permits(current - target)
    };
    info!(
        old = current,
        new = next,
        target,
        "concurrency hot-reloaded"
    );
    next
}

/// 一次查出全部调度状态（避免每轮对每个源发一次查询的 N+1 问题）。
/// 失败时告警并返回 None，交由调用方跳过本轮。
async fn load_schedule_states(state: &Arc<AppState>) -> Option<HashMap<String, ScheduleState>> {
    match state.db.lock().await.list_schedule_states() {
        Ok(map) => Some(map),
        Err(e) => {
            warn!(error = %e, "failed to load schedule states; skipping tick");
            None
        }
    }
}

/// 收集本轮到期的监控源：启用、不在退避中、且已到下个触发时刻。
/// 最多取 `queue_capacity` 个（有界队列），其余留到下轮。
async fn collect_due_sources(
    state: &Arc<AppState>,
    sched_map: &HashMap<String, ScheduleState>,
) -> Vec<SourceConfig> {
    let now = Utc::now();
    let capacity = state.runtime_snapshot().queue_capacity.max(1);
    let sources = state.sources.lock().await;
    sources
        .iter()
        // 监控开关由监控源自身控制（分组不参与叠加）。
        .filter(|s| s.enabled)
        .filter(|s| {
            let sched = sched_map.get(&s.id);
            // 退避期内跳过；无调度状态时立即视为到期。
            let backed_off = sched
                .and_then(|s| s.backoff_until)
                .is_some_and(|until| until > now);
            let due_at = sched.map(|s| s.next_due_at).unwrap_or(now);
            !backed_off && due_at <= now
        })
        .take(capacity)
        .cloned()
        .collect()
}

/// 记录一次抓取失败：递增失败计数、设置指数退避、按需发送失败告警。
async fn record_failure(state: &Arc<AppState>, source_id: &str, error: &Error) {
    error!(source = %source_id, error = %error, "check_source failed");
    let db = state.db.lock().await;
    let Ok(Some(mut sched)) = db.get_schedule_state(source_id) else {
        return;
    };
    sched.consecutive_failures += 1;
    // 记录错误信息，供 Web 控制台展示失败原因。
    sched.last_error = Some(error.to_string());
    let exp = sched.consecutive_failures.min(BACKOFF_MAX_EXP);
    sched.backoff_until =
        Some(Utc::now() + chrono::Duration::seconds(BACKOFF_BASE_SECS * 2i64.pow(exp)));
    sched.next_due_at = Utc::now() + chrono::Duration::seconds(1);

    // 连续失败达到阈值时发一条告警，同一段失败连击只发一次。
    let threshold = state.runtime_snapshot().failure_notify_threshold;
    if threshold > 0 && sched.consecutive_failures >= threshold && !sched.failure_notified {
        try_queue_failure_alert(state, &db, source_id, &sched, error, threshold);
    }
    let _ = db.upsert_schedule_state(&sched);
}

/// 把连续失败告警排入系统通知队列（失败告警走全局通知目标）。
fn try_queue_failure_alert(
    state: &Arc<AppState>,
    db: &Db,
    source_id: &str,
    sched: &ScheduleState,
    error: &Error,
    threshold: u32,
) {
    let Some(target) = state
        .notifier
        .read()
        .unwrap()
        .as_ref()
        .and_then(|n| n.global_target())
    else {
        return;
    };
    let target_json = serde_json::to_string(&crate::models::NotificationTarget {
        token: target.token.clone(),
        chat_ids: target.chat_ids.clone(),
    })
    .unwrap_or_default();
    let chat_id = target.chat_ids.first().cloned().unwrap_or_default();
    let text = notifier::render_failure_message(
        source_id,
        sched.consecutive_failures,
        threshold,
        &error.to_string(),
        &state.timezone(),
    );
    if db
        .insert_system_notification(&chat_id, &text, &target_json)
        .is_ok()
    {
        sched_mark_failure_notified(db, source_id);
    }
}

fn sched_mark_failure_notified(db: &Db, source_id: &str) {
    if let Ok(Some(mut s)) = db.get_schedule_state(source_id) {
        s.failure_notified = true;
        let _ = db.upsert_schedule_state(&s);
    }
}

/// 排空通知发件箱（变更通知 + 系统告警）。
async fn drain_outbox(state: &Arc<AppState>) {
    // 先 clone 出 Arc 再 await，避免跨 await 持有读写锁。
    let Some(notifier) = state.notifier.read().unwrap().clone() else {
        return;
    };
    if let Err(e) = notifier::process_outbox(&state.db, &state.images, &notifier, None).await {
        warn!(error = %e, "outbox processing failed");
    }
}

/// 每 [`PRUNE_INTERVAL`] 清理一次历史，返回更新后的计时起点。
///
/// 独立于 notifier：即使未配置通知器，历史清理也应照常执行。
async fn maybe_prune_history(state: &Arc<AppState>, last_prune: Instant) -> Instant {
    if last_prune.elapsed() < PRUNE_INTERVAL {
        return last_prune;
    }
    let db = state.db.lock().await;
    let tags = db.list_tags().unwrap_or_default();
    let global_limit = state.runtime_snapshot().history_limit_per_source;
    // 快速路径：没有任何分组配置历史限制时，一次全表清理即可，
    // 避免对每个源逐条 DELETE。
    if tags.iter().all(|t| t.history_limit == 0) {
        if global_limit > 0
            && let Err(e) = db.prune_history(global_limit)
        {
            warn!(error = %e, "history pruning failed");
        }
    } else {
        let sources = state.sources.lock().await;
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
    Instant::now()
}

/// 执行一次监控源检测：抓取 → 提取 → 比对 → 落库 → 按需排队通知。
///
/// 这是「检测」的核心流程，由调度主循环与 Web/CLI 的「立即检测」共用。
/// 失败时由调用方负责更新调度状态（退避 / 告警）。
pub async fn check_source(state: &Arc<AppState>, source_id: &str) -> Result<()> {
    let source = get_live_source(state, source_id).await?;
    let tags = state.tags().await;
    // 生效提取配置：跟随分组且分组配置了提取时，沿用分组设置。
    let extract_cfg = crate::config::resolve_effective_extract(&source, &tags);
    // 单次检查内用一致的时区 / 默认 cron 快照，避免中途热更新导致取值漂移。
    let tz = state.timezone();
    let default_cron = state.runtime_snapshot().default_cron;

    let previous = load_previous_snapshot(state, source_id).await?;
    let doc = fetch_document(state, &source, previous.etag, previous.last_modified).await?;

    // 304：内容未变，仅推进调度时间，不做后续比对。
    if doc.not_modified {
        debug!(source = %source.id, "304 not modified");
        advance_schedule(state, &source, &tz, &default_cron, None, false).await?;
        return Ok(());
    }

    let out = pipeline::run_pipeline(&doc, &extract_cfg)?;
    let old_items: Vec<Item> = previous
        .items_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    let diff = differ::diff(
        previous.fingerprint.as_deref().unwrap_or(""),
        &out.fingerprint,
        &old_items,
        &out.items,
    );

    save_snapshot(state, &source, &doc, &out).await?;
    state
        .engine_health
        .lock()
        .await
        .insert(source.fetch.engine.clone(), true);

    // 无变化：推进调度并结束。
    if !diff.changed {
        advance_schedule(state, &source, &tz, &default_cron, Some(Utc::now()), false).await?;
        debug!(source = %source.id, "no change");
        return Ok(());
    }

    // 指纹去重：同一内容指纹只通知一次。跨轮保留 last_notified_fingerprint，
    // 因此内容在多个指纹间振荡时也不会重复轰炸。
    {
        let db = state.db.lock().await;
        if let Some(sched) = db.get_schedule_state(&source.id)?
            && sched
                .last_notified_fingerprint
                .as_deref()
                .is_some_and(|fp| fp == diff.dedupe_key)
        {
            advance_schedule(state, &source, &tz, &default_cron, Some(Utc::now()), true).await?;
            debug!(source = %source.id, "duplicate change, suppressed");
            return Ok(());
        }
    }

    let image_urls = resolve_image_urls(&doc, &extract_cfg, &out, &diff, &old_items);
    record_change(
        state,
        &source,
        &tags,
        &doc,
        &diff,
        &image_urls,
        &tz,
        &default_cron,
    )
    .await
}

/// 上一轮快照中供比对的关键字段。
#[derive(Default)]
struct PreviousSnapshot {
    etag: Option<String>,
    last_modified: Option<String>,
    fingerprint: Option<String>,
    items_json: Option<String>,
}

async fn load_previous_snapshot(
    state: &Arc<AppState>,
    source_id: &str,
) -> Result<PreviousSnapshot> {
    let Some(snap) = state.db.lock().await.latest_snapshot(source_id)? else {
        return Ok(PreviousSnapshot::default());
    };
    Ok(PreviousSnapshot {
        etag: snap.etag,
        last_modified: snap.last_modified,
        fingerprint: Some(snap.normalized_fingerprint),
        items_json: Some(snap.items_json),
    })
}

/// 按监控源的引擎抓取文档。UA / 超时取自当前全局设置。
async fn fetch_document(
    state: &Arc<AppState>,
    source: &SourceConfig,
    etag: Option<String>,
    last_modified: Option<String>,
) -> Result<crate::models::FetchedDocument> {
    let fetcher =
        fetcher::create_fetcher(&source.fetch.engine, &state.cfg, &state.settings_snapshot())?;
    fetcher
        .fetch(&FetchSpec {
            fetch: source.fetch.clone(),
            etag,
            last_modified,
            source_id: source.id.clone(),
        })
        .await
}

/// 保存本轮快照（供下一轮比对）。
async fn save_snapshot(
    state: &Arc<AppState>,
    source: &SourceConfig,
    doc: &crate::models::FetchedDocument,
    out: &pipeline::PipelineOutput,
) -> Result<()> {
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
    state.db.lock().await.save_snapshot(&snapshot)?;
    Ok(())
}

/// 推进调度状态并落库。
///
/// `last_success` 为 `Some` 表示本次抓取成功；`had_change` 表示检测到变化
/// （被抑制的重复变化也算，避免连续变化计数被清零）。
async fn advance_schedule(
    state: &Arc<AppState>,
    source: &SourceConfig,
    tz: &str,
    default_cron: &str,
    last_success: Option<DateTime<Utc>>,
    had_change: bool,
) -> Result<()> {
    let db = state.db.lock().await;
    let prev = db.get_schedule_state(&source.id)?;
    let next = next_schedule(
        source,
        0,
        last_success,
        had_change,
        prev.as_ref(),
        tz,
        default_cron,
    );
    db.upsert_schedule_state(&next)
}

/// 确定本次变更要随通知附带的图片 URL。
///
/// `Changed` 模式只收集**发生变更的元素**相关图片，需要结合 diff 结果定位
/// HTML 元素，故在此按需重算；其余模式直接用提取阶段收集到的结果。
fn resolve_image_urls(
    doc: &crate::models::FetchedDocument,
    extract_cfg: &ExtractConfig,
    out: &pipeline::PipelineOutput,
    diff: &crate::models::DiffResult,
    old_items: &[Item],
) -> Vec<String> {
    let ExtractConfig::Items {
        selector,
        fields,
        images: Some(ImageSelector::Changed),
        ..
    } = extract_cfg
    else {
        return out.image_urls.clone();
    };
    // Changed 模式依赖 CSS 定位元素；JSONPath 源无法定位，回退整页图片避免丢图。
    if !matches!(selector, ItemSelector::Css { .. }) {
        return out.image_urls.clone();
    }
    pipeline::collect_changed_element_images(
        doc,
        selector,
        fields,
        &changed_item_ids(diff, old_items),
    )
}

/// 本次变更中新增 / 更新的条目 stable_id。
fn changed_item_ids(diff: &crate::models::DiffResult, old_items: &[Item]) -> HashSet<String> {
    let old_ids: HashSet<&str> = diff
        .old_items
        .iter()
        .map(|i| i.stable_id.as_str())
        .collect();
    diff.new_items
        .iter()
        .filter(|new| {
            !old_ids.contains(new.stable_id.as_str())
                || old_items.iter().any(|old| {
                    old.stable_id == new.stable_id && old.fingerprint(&[]) != new.fingerprint(&[])
                })
        })
        .map(|i| i.stable_id.clone())
        .collect()
}

/// 落库变更事件（含截图）、按需排队通知，并推进调度状态。
#[allow(clippy::too_many_arguments)]
async fn record_change(
    state: &Arc<AppState>,
    source: &SourceConfig,
    tags: &[crate::models::TagConfig],
    doc: &crate::models::FetchedDocument,
    diff: &crate::models::DiffResult,
    image_urls: &[String],
    tz: &str,
    default_cron: &str,
) -> Result<()> {
    let mut event = ChangeEvent {
        id: 0,
        watchpoint_id: source.id.clone(),
        change_type: diff.change_type.unwrap_or(ChangeType::Updated),
        old_items_json: serde_json::to_string(&diff.old_items)?,
        new_items_json: serde_json::to_string(&diff.new_items)?,
        diff_summary: diff.diff_summary.clone(),
        fingerprint: diff.fingerprint.clone(),
        dedupe_key: diff.dedupe_key.clone(),
        image_urls_json: serde_json::to_string(image_urls)?,
        detected_at: Utc::now(),
        read: false,
        screenshot_path: None,
    };

    let db = state.db.lock().await;
    let event_id = db.insert_change_event(&event)?;
    event.id = event_id;
    // 插入成功后再落盘截图（以 event_id 命名）；写失败时事件不带截图，
    // 不留残留文件，也不会出现 DB 与文件名不一致。
    store_screenshot(state, &db, event_id, doc, source);

    // 通知开关由监控源自身控制（分组不参与叠加）；仅开启时排队发送。
    let (_, notify_enabled, _) = crate::config::resolve_effective_source(source, tags, 0);
    if notify_enabled && state.notifier.read().unwrap().is_some() {
        // 目标解析：分组优先，回退全局（读热更新后的 tgram:// url）。
        let target = crate::config::resolve_notify_target(source, tags, &state.telegram_url())
            .or_else(|| {
                state
                    .notifier
                    .read()
                    .unwrap()
                    .as_ref()
                    .and_then(|n| n.global_target())
            });
        if let Some(target) = target {
            queue_notification(&db, event_id, &target)?;
        }
    }

    let prev = db.get_schedule_state(&source.id)?;
    let mut sched = next_schedule(
        source,
        0,
        Some(Utc::now()),
        true,
        prev.as_ref(),
        tz,
        default_cron,
    );
    sched.last_notified_fingerprint = Some(event.dedupe_key.clone());
    sched.last_notified_at = Some(Utc::now());
    db.upsert_schedule_state(&sched)?;

    info!(
        source = %source.id,
        event_id,
        change_type = ?event.change_type,
        summary = %event.diff_summary,
        "change detected"
    );
    Ok(())
}

/// 把 camofox 截图写入 `media_dir/screenshots/event-{id}.png` 并回填路径。
fn store_screenshot(
    state: &Arc<AppState>,
    db: &Db,
    event_id: i64,
    doc: &crate::models::FetchedDocument,
    source: &SourceConfig,
) {
    let Some(data) = doc.screenshot.as_deref() else {
        return;
    };
    if source.fetch.engine != "camofox" || !source.fetch.screenshot {
        return;
    }
    let dir = state.media_dir().join("screenshots");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(error = %e, event_id, "failed to create screenshots dir; event will have no screenshot");
        return;
    }
    let fname = format!("event-{event_id}.png");
    let path = dir.join(&fname);
    if let Err(e) = std::fs::write(&path, data) {
        warn!(error = %e, event_id, "failed to write screenshot; event will have no screenshot");
        return;
    }
    if let Err(e) = db.update_event_screenshot(event_id, Some(&format!("screenshots/{fname}"))) {
        // DB 回填失败：删掉文件，避免留下无人引用的孤儿文件。
        warn!(error = %e, event_id, "failed to set screenshot path in db; removing file");
        let _ = std::fs::remove_file(&path);
    }
}

/// 把一条变更通知排入发件箱，由 notifier 异步发送。
fn queue_notification(
    db: &Db,
    event_id: i64,
    target: &crate::models::TelegramTarget,
) -> Result<()> {
    let target_json = serde_json::to_string(&crate::models::NotificationTarget {
        token: target.token.clone(),
        chat_ids: target.chat_ids.clone(),
    })?;
    db.insert_notification(&crate::models::NotificationRecord {
        id: 0,
        event_id,
        chat_id: target.chat_ids.first().cloned().unwrap_or_default(),
        target_json,
        message_ids_json: "[]".to_string(),
        status: "pending".to_string(),
        attempts: 0,
        next_retry_at: None,
    })?;
    Ok(())
}

/// 测试监控源：抓取并按配置提取，返回摘要，**不落库**（不写快照 / 不产生事件）。
pub async fn test_source(state: &Arc<AppState>, source_id: &str) -> Result<Value> {
    let source = get_live_source(state, source_id).await?;
    let extract_cfg = crate::config::resolve_effective_extract(&source, &state.tags().await);
    let doc = fetch_document(state, &source, None, None).await?;
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

/// 计算下一轮调度状态。
///
/// - `failures`：连续失败次数（成功路径传 0，会清除失败计数与退避）。
/// - `last_success`：本次抓取是否成功（成功则记录时间）。
/// - `had_change`：本次是否检测到内容变化。**重复变化（被抑制）也视为有变化**，
///   避免连续变化计数被意外清零。
/// - `prev`：上一轮状态，用于**保留** `last_notified_*`，让基于指纹的
///   重复告警抑制跨轮生效。
/// - `tz`：cron 表达式使用的 IANA 时区名。
///
/// 调度完全由源的 `schedule.cron` 驱动：按 cron 精确计算下一次触发时刻；
/// 表达式为空或无效时退化为 60s 短间隔重试，避免单个源卡死 daemon。
fn next_schedule(
    source: &SourceConfig,
    failures: u32,
    last_success: Option<DateTime<Utc>>,
    had_change: bool,
    prev: Option<&ScheduleState>,
    tz: &str,
    default_cron: &str,
) -> ScheduleState {
    let consecutive_changes = if had_change {
        prev.map(|p| p.consecutive_changes.saturating_add(1))
            .unwrap_or(1)
    } else {
        0
    };

    // 监控源未配置 cron 时用全局默认；两者都为空时回退到每小时。
    let expr = source
        .schedule
        .cron
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let d = default_cron.trim();
            (!d.is_empty()).then_some(d)
        })
        .unwrap_or(DEFAULT_CRON);
    let next_due_at = match crate::cron_expr::next_due(expr, tz, Utc::now()) {
        Ok(t) => t,
        Err(e) => {
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
        // 成功路径清空最近错误信息。
        last_error: None,
        last_notified_fingerprint: prev.and_then(|p| p.last_notified_fingerprint.clone()),
        last_notified_at: prev.and_then(|p| p.last_notified_at),
        failure_notified: failures == 0,
    }
}

/// 取内存中的监控源（SQLite 是持久层，内存列表是运行时视图）。
pub async fn get_live_source(state: &Arc<AppState>, source_id: &str) -> Result<SourceConfig> {
    state
        .sources
        .lock()
        .await
        .iter()
        .find(|s| s.id == source_id)
        .cloned()
        .ok_or_else(|| Error::other(format!("source not found: {source_id}")))
}
