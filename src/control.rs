use std::net::{IpAddr, Ipv6Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info};

use crate::config::{EditableSettings, FetchConfig, SourceConfig};
use crate::error::{Error, Result};
use crate::fetcher::{FetchSpec, create_fetcher};
use crate::models::TagConfig;
use crate::scheduler::{self, AppState};

/// 单次批量操作允许的最大监控源数量，避免持锁期间长时间执行 upsert 阻塞其他请求。
const MAX_BATCH_SIZE: usize = 100;

/// 校验全局可编辑设置的合法值，返回首个错误；合法时返回 `Ok(())`。
fn validate_settings(s: &EditableSettings) -> Result<()> {
    if s.concurrency == 0 {
        return Err(Error::config("concurrency 必须大于 0"));
    }
    if s.queue_capacity == 0 {
        return Err(Error::config("queue_capacity 必须大于 0"));
    }
    if s.default_timeout_secs == 0 {
        return Err(Error::config("default_timeout_secs 必须大于 0"));
    }
    if !s.default_cron.trim().is_empty()
        && s.default_cron.parse::<cron::Schedule>().is_err()
    {
        return Err(Error::config(format!(
            "default_cron 不是合法的 cron 表达式: {}",
            s.default_cron
        )));
    }
    if !s.timezone.trim().is_empty()
        && iana_time_zone::get_timezone().is_ok()
        && s.timezone != crate::config::system_local_timezone()
        && chrono_tz::Tz::from_str(&s.timezone).is_err()
    {
        return Err(Error::config(format!(
            "timezone 不是合法的 IANA 时区: {}",
            s.timezone
        )));
    }
    // 非空 telegram_url 必须可解析为合法通知目标，避免非法 URL 入库后
    // 在热更新重建 notifier 时静默失败、把通知功能整体关掉。
    if !s.telegram_url.trim().is_empty()
        && crate::config::parse_telegram_url(&s.telegram_url).is_err()
    {
        return Err(Error::config(format!(
            "telegram_url 不是合法的 tgram:// 通知目标: {}",
            s.telegram_url
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    ListSources,
    ListEvents {
        limit: Option<usize>,
    },
    Check {
        source_id: String,
    },
    SourcesAdd {
        source: Box<SourceConfig>,
    },
    SourcesUpdate {
        source: Box<SourceConfig>,
    },
    SourcesDelete {
        source_id: String,
    },
    SourcesSetFlags {
        /// 要批量更新的监控源 id 列表。
        source_ids: Vec<String>,
        /// 批量设置监控开关。None 表示不修改监控开关。
        enabled: Option<bool>,
        /// 批量设置通知开关。None 表示不修改通知开关。
        notify_enabled: Option<bool>,
    },
    TestSource {
        source_id: String,
    },
    /// 预览：抓取 URL 并返回页面标题，用于添加监控源时自动填充名称。
    PreviewSource {
        url: String,
        engine: String,
    },
    Diff {
        event_id: i64,
    },
    History {
        source_id: Option<String>,
        limit: Option<usize>,
    },
    /// 标记某个监控源的全部变更事件为已读。
    MarkSourceRead {
        source_id: String,
    },
    /// 列出全部分组（标签）设置。
    ListTags,
    /// 新增 / 更新一个分组（标签）设置。
    UpdateTag {
        tag: Box<TagConfig>,
    },
    /// 删除一个分组（标签）设置。
    DeleteTag {
        name: String,
    },
    /// 标记单个变更事件为已读。
    MarkEventRead {
        event_id: i64,
    },
    NotifyTest {
        chat_id: Option<String>,
    },
    GetSettings,
    UpdateSettings {
        settings: Box<EditableSettings>,
    },
    Backup,
    ListBackups,
    Restore {
        name: String,
    },
    DeleteBackup {
        name: String,
    },
    RestoreUpload {
        /// 上传的 zip 备份在本机临时路径。
        path: PathBuf,
    },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlResponse {
    pub fn ok(result: Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.into()),
        }
    }
}

#[cfg(unix)]
pub async fn serve_control(state: Arc<AppState>) -> Result<()> {
    let path = state.runtime.read().unwrap().socket_path.clone();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    info!(socket = %path.display(), "control socket listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(state, stream).await {
                error!(error = %e, "control connection error");
            }
        });
    }
}

#[cfg(not(unix))]
pub async fn serve_control(state: Arc<AppState>) -> Result<()> {
    // Non-Unix fallback for local development: 127.0.0.1 only.
    let addr = "127.0.0.1:38765";
    let listener = TcpListener::bind(addr).await?;
    info!(addr = %addr, "control TCP fallback listening (non-unix dev)");
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(state, stream).await {
                error!(error = %e, "control connection error");
            }
        });
    }
}

async fn handle_connection<S>(state: Arc<AppState>, stream: S) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let req: ControlRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = ControlResponse::err(format!("invalid request: {e}"));
                write_response(&mut writer, &resp).await?;
                continue;
            }
        };
        let resp = handle_request(&state, req).await;
        write_response(&mut writer, &resp).await?;
    }
    Ok(())
}

pub(crate) async fn handle_request(state: &Arc<AppState>, req: ControlRequest) -> ControlResponse {
    match req {
        ControlRequest::Status => {
            let s = state.status().await;
            ControlResponse::ok(serde_json::to_value(s).unwrap_or(json!(null)))
        }
        ControlRequest::ListSources => {
            let db = state.db.lock().await;
            let sources = state.sources.lock().await.clone();
            match db.list_source_meta(&sources) {
                Ok(meta) => ControlResponse::ok(serde_json::to_value(meta).unwrap_or(json!([]))),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::ListEvents { limit } => {
            let db = state.db.lock().await;
            match db.list_change_events(None, limit.unwrap_or(20)) {
                Ok(events) => {
                    ControlResponse::ok(serde_json::to_value(events).unwrap_or(json!([])))
                }
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Check { source_id } => {
            match scheduler::check_source(state, &source_id).await {
                Ok(()) => ControlResponse::ok(json!({"source_id": source_id, "checked": true})),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::SourcesAdd { mut source } => {
            // 将存在性检查、DB 写入、内存 push 放进同一次 sources 锁内完成，
            // 避免并发添加相同 id 时因检查与 push 分处两次锁而产生重复条目（TOCTOU）。
            // 锁获取顺序统一为 db → sources，与调度器 run_daemon 保持一致以避免死锁。
            let db = state.db.lock().await;
            let mut sources = state.sources.lock().await;
            // ID 未填时自动生成：优先从名称生成可读 slug，否则回退到随机短 id。
            if source.id.trim().is_empty() {
                source.id = crate::config::generate_source_id(&source.name, &source.fetch.url);
            }
            // 名称未填时自动从 URL 主机名生成可读名称。
            if source.name.trim().is_empty() {
                if let Ok(u) = url::Url::parse(&source.fetch.url) {
                    source.name = u.host_str().unwrap_or("").to_string();
                }
            }
            let id = source.id.clone();
            if sources.iter().any(|s| s.id == id) {
                return ControlResponse::err(format!("source {} already exists", id));
            }
            // 自动登记源的新标签到分组表，使分组能出现在「分组管理」中供配置。
            let _ = db.ensure_tags(&source.tags);
            if let Err(e) = db.upsert_source(source.as_ref()) {
                return ControlResponse::err(e.to_string());
            }
            sources.push(source.as_ref().clone());
            ControlResponse::ok(json!({ "source_id": source.id, "added": true }))
        }
        ControlRequest::SourcesUpdate { source } => {
            // 先校验存在性再写库，避免更新不存在的 id 时 upsert_source 插入新行，
            // 导致 DB 被写入却返回 not found，DB 与内存 sources 列表不一致。
            let db = state.db.lock().await;
            let mut sources = state.sources.lock().await;
            if !sources.iter().any(|s| s.id == source.id) {
                return ControlResponse::err(format!("source {} not found", source.id));
            }
            // 自动登记源的新标签到分组表，使分组能出现在「分组管理」中供配置。
            let _ = db.ensure_tags(&source.tags);
            if let Err(e) = db.upsert_source(source.as_ref()) {
                return ControlResponse::err(e.to_string());
            }
            if let Some(s) = sources.iter_mut().find(|s| s.id == source.id) {
                *s = source.as_ref().clone();
            }
            ControlResponse::ok(json!({ "source_id": source.id, "updated": true }))
        }
        ControlRequest::SourcesDelete { source_id } => {
            let db = state.db.lock().await;
            let mut sources = state.sources.lock().await;
            if let Err(e) = db.delete_source(&source_id) {
                return ControlResponse::err(e.to_string());
            }
            sources.retain(|s| s.id != source_id);
            ControlResponse::ok(json!({ "source_id": source_id, "deleted": true }))
        }
        ControlRequest::SourcesSetFlags {
            source_ids,
            enabled,
            notify_enabled,
        } => {
            // 空 id 列表或两个开关均未指定时直接返回成功（幂等，避免前端空选/误报错，
            // 也避免对每个源做无意义的 upsert 写库）。
            if source_ids.is_empty() || (enabled.is_none() && notify_enabled.is_none()) {
                return ControlResponse::ok(json!({"updated": 0}));
            }
            // 限制单次批量数量，避免超大列表在持锁期间长时间执行 upsert 阻塞其他请求。
            if source_ids.len() > MAX_BATCH_SIZE {
                return ControlResponse::err(format!(
                    "batch size {} exceeds limit {MAX_BATCH_SIZE}",
                    source_ids.len()
                ));
            }
            // 同一把 db → sources 锁内完成校验、写库与内存更新，保证原子性。
            let db = state.db.lock().await;
            let mut sources = state.sources.lock().await;
            // 基于快照先写库成功后再写回内存，避免写库失败时内存已被修改导致
            // 内存与 DB 不一致；先收集全部修改后的快照并统一写库，中途失败则
            // 返回错误且内存保持原状（与原实现「逐个 upsert 留部分成功状态」相比更安全）。
            let mut snapshots: Vec<SourceConfig> = Vec::new();
            for sid in &source_ids {
                let Some(source) = sources.iter().find(|s| s.id == *sid) else {
                    continue;
                };
                let mut snapshot = source.clone();
                if let Some(e) = enabled {
                    snapshot.enabled = e;
                }
                if let Some(n) = notify_enabled {
                    snapshot.notify_enabled = n;
                }
                if let Err(e) = db.upsert_source(&snapshot) {
                    return ControlResponse::err(format!("failed to update {}: {e}", snapshot.id));
                }
                snapshots.push(snapshot);
            }
            // 全部写库成功后统一写回内存，保持内存与 DB 一致。
            let updated = snapshots.len();
            for snapshot in &snapshots {
                if let Some(s) = sources.iter_mut().find(|s| s.id == snapshot.id) {
                    *s = snapshot.clone();
                }
            }
            ControlResponse::ok(json!({"updated": updated}))
        }
        ControlRequest::TestSource { source_id } => {
            let source = match scheduler::get_live_source(state, &source_id).await {
                Ok(s) => s,
                Err(e) => return ControlResponse::err(e.to_string()),
            };
            match scheduler::test_source(state, &source).await {
                Ok(v) => ControlResponse::ok(v),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::PreviewSource { url, engine } => {
            match preview_url(state, &url, &engine).await {
                Ok(title) => ControlResponse::ok(json!({ "url": url, "title": title })),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Diff { event_id } => {
            let db = state.db.lock().await;
            match db.get_change_event(event_id) {
                Ok(Some(ev)) => {
                    ControlResponse::ok(serde_json::to_value(ev).unwrap_or(json!(null)))
                }
                Ok(None) => ControlResponse::err(format!("event {event_id} not found")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::History { source_id, limit } => {
            let db = state.db.lock().await;
            match db.list_change_events(source_id.as_deref(), limit.unwrap_or(20)) {
                Ok(events) => {
                    ControlResponse::ok(serde_json::to_value(events).unwrap_or(json!([])))
                }
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::MarkSourceRead { source_id } => {
            let db = state.db.lock().await;
            match db.mark_source_events_read(&source_id) {
                Ok(n) => ControlResponse::ok(json!({ "updated": n })),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::ListTags => {
            let db = state.db.lock().await;
            match db.list_tags() {
                Ok(tags) => ControlResponse::ok(serde_json::to_value(tags).unwrap_or(json!([]))),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::UpdateTag { tag } => {
            let name = tag.name.trim().to_string();
            if name.is_empty() {
                return ControlResponse::err("tag name is required");
            }
            let mut tag = tag.as_ref().clone();
            tag.name = name;
            let db = state.db.lock().await;
            match db.upsert_tag(&tag) {
                Ok(()) => ControlResponse::ok(json!({ "name": tag.name, "updated": true })),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::DeleteTag { name } => {
            if name.trim().is_empty() {
                return ControlResponse::err("tag name is required");
            }
            let db = state.db.lock().await;
            match db.delete_tag(name.trim()) {
                Ok(n) if n > 0 => {
                    ControlResponse::ok(json!({ "name": name.trim(), "deleted": true }))
                }
                Ok(_) => ControlResponse::err(format!("tag {} not found", name.trim())),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::MarkEventRead { event_id } => {
            let db = state.db.lock().await;
            match db.mark_event_read(event_id) {
                Ok(n) => ControlResponse::ok(json!({ "updated": n })),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::NotifyTest { chat_id } => {
            let notifier = state.notifier.read().unwrap().clone();
            match notifier {
                Some(n) => match n.send_test(chat_id.as_deref()).await {
                    Ok(id) => ControlResponse::ok(json!({"message_id": id})),
                    Err(e) => ControlResponse::err(e.to_string()),
                },
                None => ControlResponse::err("telegram notifier disabled"),
            }
        }
        ControlRequest::GetSettings => {
            // 全局可编辑设置以 SQLite 为唯一来源；未配置时为默认值。
            let s = {
                let db = state.db.lock().await;
                db.get_settings().ok().flatten().unwrap_or_default()
            };
            match serde_json::to_value(s) {
                Ok(v) => ControlResponse::ok(v),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::UpdateSettings { settings } => {
            // 保存前校验：非法值直接拒绝，避免坏值入库后反复影响运行。
            if let Err(e) = validate_settings(&settings) {
                return ControlResponse::err(e.to_string());
            }
            // 持久化到 SQLite（`settings` 表），不再写入 config.yaml。
            {
                let db = state.db.lock().await;
                if let Err(e) = db.set_settings(&settings) {
                    return ControlResponse::err(format!("failed to save settings: {e}"));
                }
            }
            // 热更新：全部字段立即刷新到 runtime / notifier。
            state.reload_settings(&settings);
            ControlResponse::ok(json!({
                "saved": true,
                "config": "SQLite (settings 表)",
                "applied": true,
                // 全部设置保存即生效。
                "restart_required": false,
                "immediate": true,
            }))
        }
        ControlRequest::Backup => {
            // 仅在线备份需要持有 DB 锁（SQLite 快照）；zip 打包放在释放锁之后，
            // 避免大库打包阻塞 daemon 的其它请求。
            let (name, dir) = {
                let db = state.db.lock().await;
                match crate::backup::backup(
                    db.connection(),
                    &state.cfg,
                    state.config_path.as_deref(),
                ) {
                    Ok(dir) => {
                        let name = dir
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        (name, dir)
                    }
                    Err(e) => return ControlResponse::err(e.to_string()),
                }
            };
            // 释放锁后异步打包 zip（阻塞 I/O 用 spawn_blocking，不阻塞 async 执行器）。
            let dir_for_zip = dir.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::backup::pack_backup_zip(&dir_for_zip) {
                    tracing::warn!(error = %e, "zip packing failed for backup");
                }
            })
            .await
            .unwrap_or(());
            let has_zip = state
                .runtime
                .read()
                .unwrap()
                .state_dir
                .join("backups")
                .join(format!("{name}.zip"))
                .exists();
            ControlResponse::ok(json!({
                "path": dir.display().to_string(),
                "name": name,
                "has_zip": has_zip,
            }))
        }
        ControlRequest::ListBackups => {
            match crate::backup::list_backups(&state.runtime.read().unwrap().state_dir) {
                Ok(names) => ControlResponse::ok(json!({"backups": names})),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Restore { name } => {
            // 校验备份名，防止路径遍历注入。
            if !crate::backup::is_valid_backup_name(&name) {
                return ControlResponse::err("invalid backup name");
            }
            let mut db = state.db.lock().await;
            let dir = state.runtime.read().unwrap().state_dir.join("backups").join(&name);
            if !dir.join("reading-steiner.db").exists() {
                return ControlResponse::err(format!("backup {name} not found"));
            }
            // 在线恢复：通过 SQLite 备份接口把备份库写进实时连接，并刷新内存中的监控源。
            match crate::backup::restore(&dir, &state.cfg, Some(db.connection_mut())) {
                Ok(()) => {
                    // 恢复后重新从数据库加载监控源到内存（同步完成，不在 await 期间持有 &Db）。
                    let sources = db.list_sources().unwrap_or_default();
                    *state.sources.lock().await = sources;
                    ControlResponse::ok(json!({
                        "restored": true,
                        "name": name,
                        "note": "数据库与 media 已在线恢复"
                    }))
                }
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::DeleteBackup { name } => {
            if !crate::backup::is_valid_backup_name(&name) {
                return ControlResponse::err("invalid backup name");
            }
            match crate::backup::delete_backup(&state.runtime.read().unwrap().state_dir, &name) {
                Ok(deleted) if deleted => ControlResponse::ok(json!({
                    "deleted": true,
                    "name": name,
                })),
                Ok(_) => ControlResponse::err(format!("backup {name} not found")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::RestoreUpload { path } => {
            let file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => return ControlResponse::err(format!("无法读取上传文件: {e}")),
            };
            // 先无锁解压：解压不涉及数据库，若在 DB 锁内执行大体积 media 解压
            // 会长时间阻塞 daemon 的其它数据库访问。
            let restored_dir = match crate::backup::extract_zip(file, &state.cfg) {
                Ok(dir) => dir,
                Err(e) => {
                    let _ = std::fs::remove_file(&path);
                    return ControlResponse::err(e.to_string());
                }
            };
            // 再单独持有 DB 锁执行恢复与刷新内存监控源，锁占用时间最小化。
            let mut db = state.db.lock().await;
            if let Err(e) = crate::backup::restore(&restored_dir, &state.cfg, Some(db.connection_mut()))
            {
                drop(db);
                // 恢复失败时清理解压残留，避免遗留无 zip 的“半成品”备份。
                let _ = crate::backup::cleanup_backup_dir(&restored_dir);
                let _ = std::fs::remove_file(&path);
                return ControlResponse::err(e.to_string());
            }
            // 恢复后重新从数据库加载监控源到内存（同步完成，不在 await 期间持有 &Db）。
            let sources = db.list_sources().unwrap_or_default();
            *state.sources.lock().await = sources;
            drop(db); // 释放 DB 锁后再打包 zip，避免大 media 压缩阻塞 daemon。

            // 补一个 zip（便于与其它备份一致地下载/管理）。失败仅记录，不影响恢复结果。
            let _ = crate::backup::pack_backup_zip(&restored_dir);
            // 清理上传产生的临时文件。
            let _ = std::fs::remove_file(&path);
            ControlResponse::ok(json!({
                "restored": true,
                "name": restored_dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string(),
                "note": "已从上传的 zip 备份在线恢复"
            }))
        }
        ControlRequest::Shutdown => {
            state
                .running
                .store(false, std::sync::atomic::Ordering::Relaxed);
            ControlResponse::ok(json!({"shutdown": true}))
        }
    }
}

async fn write_response<W>(writer: &mut W, resp: &ControlResponse) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let line = serde_json::to_string(resp)?;
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

/// 抓取 URL 并提取页面 `<title>`（JSON 接口则取 `title`/`name` 字段），
/// 用于添加监控源时自动填充名称。非 HTML/非 JSON 内容返回空标题。
async fn preview_url(state: &Arc<AppState>, url: &str, engine: &str) -> Result<String> {
    let url = url.trim();
    // SSRF 防护：仅允许 http/https，且目标不得为私网/环回/链路本地地址。
    assert_safe_preview_url(url)?;
    let engine = if engine.is_empty() { "http" } else { engine };
    let fetch = FetchConfig {
        engine: engine.to_string(),
        url: url.to_string(),
        ..FetchConfig::default()
    };
    let fetcher = create_fetcher(
        engine,
        &state.cfg,
        &state.settings.read().unwrap().clone(),
    )?;
    let doc = fetcher
        .fetch(&FetchSpec {
            fetch,
            etag: None,
            last_modified: None,
            source_id: String::new(),
        })
        .await?;

    // 提取标题：优先解析 HTML `<title>`，JSON 则取 title/name 字段。
    let text = doc.text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let is_json = doc
        .content_type
        .as_deref()
        .map(|ct| ct.contains("json"))
        .unwrap_or(false);
    if is_json {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            for key in ["title", "name", "Title", "Name"] {
                if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                    let s = s.trim();
                    if !s.is_empty() {
                        return Ok(s.to_string());
                    }
                }
            }
        }
        return Ok(String::new());
    }

    let doc_html = scraper::Html::parse_document(text);
    if let Ok(sel) = scraper::Selector::parse("title") {
        if let Some(el) = doc_html.select(&sel).next() {
            let title = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
            if !title.is_empty() {
                return Ok(title);
            }
        }
    }
    // 部分页面没有 `<title>`，回退到 `<h1>`。
    if let Ok(sel) = scraper::Selector::parse("h1") {
        if let Some(el) = doc_html.select(&sel).next() {
            let title = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
            if !title.is_empty() {
                return Ok(title);
            }
        }
    }
    Ok(String::new())
}

/// 校验 preview 抓取的 URL，防止 SSRF：仅允许 http/https 协议，且
/// 目标主机不得解析到私网 / 环回 / 链路本地 / 未指定等内网地址。
fn assert_safe_preview_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(|_| Error::config("invalid preview url"))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(Error::config(format!(
            "unsupported preview url scheme: {scheme}"
        )));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::config("preview url missing host"))?;

    // host 为 IP 字面量：直接校验；否则按域名解析后校验（尽力而为）。
    let ip = parsed
        .host()
        .and_then(|h| match h {
            url::Host::Ipv4(v) => Some(IpAddr::V4(v)),
            url::Host::Ipv6(v) => Some(IpAddr::V6(v)),
            _ => None,
        })
        .or_else(|| {
            // 域名：解析出所有 IP 逐一校验，任一命中内网地址即拒绝。
            (host, 0u16)
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next().map(|s| s.ip()))
        });

    if let Some(ip) = ip {
        if is_private_ip(ip) {
            return Err(Error::config(format!(
                "preview url target resolves to private/internal address: {ip}"
            )));
        }
    }
    Ok(())
}

/// 判断 IP 是否为私网 / 环回 / 链路本地 / 未指定等内网地址。
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_unspecified()
                || v.is_documentation()
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || is_v6_private(&v)
        }
    }
}

/// IPv6 链路本地 / 唯一本地 / 站点本地地址。
fn is_v6_private(v: &Ipv6Addr) -> bool {
    v.segments()[0] & 0xffc0 == 0xfe80   // fe80::/10 链路本地
        || v.segments()[0] & 0xfe00 == 0xfc00 // fc00::/7 唯一本地
        || v.segments()[0] == 0xfec0       // fec0::/10 站点本地
        || v.segments()[0] == 0
}

#[cfg(unix)]
pub async fn send_request(
    socket_path: impl AsRef<Path>,
    req: &ControlRequest,
) -> Result<ControlResponse> {
    let stream = UnixStream::connect(socket_path.as_ref()).await?;
    send_request_stream(stream, req).await
}

#[cfg(not(unix))]
pub async fn send_request(
    _socket_path: impl AsRef<Path>,
    req: &ControlRequest,
) -> Result<ControlResponse> {
    let stream = TcpStream::connect("127.0.0.1:38765").await?;
    send_request_stream(stream, req).await
}

async fn send_request_stream<S>(stream: S, req: &ControlRequest) -> Result<ControlResponse>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let line = serde_json::to_string(req)?;
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines
        .next_line()
        .await?
        .ok_or_else(|| Error::Control("empty response from daemon".into()))?;
    Ok(serde_json::from_str(&resp_line)?)
}

pub fn socket_path_from_config(cfg: &crate::config::Config) -> std::path::PathBuf {
    cfg.socket_path()
}
