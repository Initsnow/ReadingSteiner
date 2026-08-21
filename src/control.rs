use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info};

use crate::config::{EditableSettings, SourceConfig};
use crate::error::{Error, Result};
use crate::scheduler::{self, AppState};

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
    TestSource {
        source_id: String,
    },
    Diff {
        event_id: i64,
    },
    History {
        source_id: Option<String>,
        limit: Option<usize>,
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
    let path = state.runtime.socket_path.clone();
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
            let sources = state.sources.lock().await.clone();
            ControlResponse::ok(serde_json::to_value(sources).unwrap_or(json!([])))
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
        ControlRequest::SourcesAdd { source } => {
            // 将存在性检查、DB 写入、内存 push 放进同一次 sources 锁内完成，
            // 避免并发添加相同 id 时因检查与 push 分处两次锁而产生重复条目（TOCTOU）。
            // 锁获取顺序统一为 db → sources，与调度器 run_daemon 保持一致以避免死锁。
            let db = state.db.lock().await;
            let mut sources = state.sources.lock().await;
            if sources.iter().any(|s| s.id == source.id) {
                return ControlResponse::err(format!("source {} already exists", source.id));
            }
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
        ControlRequest::NotifyTest { chat_id } => match &state.notifier {
            Some(n) => match n.send_test(chat_id.as_deref()).await {
                Ok(id) => ControlResponse::ok(json!({"message_id": id})),
                Err(e) => ControlResponse::err(e.to_string()),
            },
            None => ControlResponse::err("telegram notifier disabled"),
        },
        ControlRequest::GetSettings => {
            let s = EditableSettings::from_config(&state.cfg);
            match serde_json::to_value(s) {
                Ok(v) => ControlResponse::ok(v),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::UpdateSettings { settings } => {
            // 持久化到 config 文件；部分参数（并发数、超时、UA、时区、模板）需重启 daemon 生效。
            let Some(config_path) = state.config_path.clone() else {
                return ControlResponse::err(
                    "no config file path available; cannot persist settings",
                );
            };
            let mut cfg = state.cfg.clone();
            settings.apply_to(&mut cfg);
            if let Err(e) = cfg.save(&config_path) {
                return ControlResponse::err(format!("failed to save config: {e}"));
            }
            ControlResponse::ok(json!({
                "saved": true,
                "restart_required": true,
                "config": config_path.display().to_string(),
            }))
        }
        ControlRequest::Backup => {
            let db = state.db.lock().await;
            match crate::backup::backup(db.connection(), &state.cfg, state.config_path.as_deref()) {
                Ok(dir) => ControlResponse::ok(json!({
                    "path": dir.display().to_string(),
                    "name": dir.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                })),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::ListBackups => {
            match crate::backup::list_backups(&state.runtime.state_dir) {
                Ok(names) => ControlResponse::ok(json!({"backups": names})),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Restore { name } => {
            // 恢复需停止 daemon 后由 CLI 执行；这里仅校验备份存在并给出提示。
            let dir = state.runtime.state_dir.join("backups").join(&name);
            if !dir.join("reading-steiner.db").exists() {
                return ControlResponse::err(format!("backup {name} not found"));
            }
            ControlResponse::err(format!(
                "restore must be run with daemon stopped: reading-steiner restore {name}"
            ))
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
