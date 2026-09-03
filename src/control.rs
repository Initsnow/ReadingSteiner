//! CLI 控制通道：Unix socket 行协议（每行一个 JSON 请求 / 响应）。
//!
//! 这一层**只做传输与分发**：解析请求、调用 [`crate::api`] 的领域服务、
//! 序列化响应。业务规则不在本文件实现，避免与 Web 控制台出现行为漂移。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info};

use crate::api;
use crate::config::{EditableSettings, SourceConfig};
use crate::error::{Error, Result};
use crate::models::TagConfig;
use crate::scheduler::AppState;

#[cfg(not(unix))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

/// CLI 可发起的控制请求。
///
/// 与 Web API 一一对应；两者都只是 [`crate::api`] 的调用入口，
/// 因此不存在「Web 能改但 CLI 改不了」的缺口。
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    ListSources,
    ListEvents {
        limit: Option<usize>,
    },
    Diff {
        event_id: i64,
    },
    History {
        source_id: Option<String>,
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
    /// 批量设置多个监控源的监控开关 / 通知开关（均可选，缺省表示不修改）。
    SourcesSetFlags {
        source_ids: Vec<String>,
        enabled: Option<bool>,
        notify_enabled: Option<bool>,
    },
    TestSource {
        source_id: String,
    },
    /// 抓取 URL 并返回页面标题，用于添加监控源时自动填充名称。
    PreviewSource {
        url: String,
        engine: String,
    },
    /// 标记某个监控源的全部变更事件为已读。
    MarkSourceRead {
        source_id: String,
    },
    /// 标记单个变更事件为已读。
    MarkEventRead {
        event_id: i64,
    },
    ListTags,
    UpdateTag {
        tag: Box<TagConfig>,
    },
    DeleteTag {
        name: String,
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
    /// 从已落盘的 zip 备份在线恢复。
    RestoreUpload {
        path: PathBuf,
    },
    Shutdown,
}

/// 控制通道响应。
#[derive(Debug, Serialize, Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlResponse {
    fn ok(result: impl Serialize) -> Self {
        Self {
            ok: true,
            result: Some(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
            error: None,
        }
    }
    pub fn err(msg: impl std::fmt::Display) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(msg.to_string()),
        }
    }
}

/// 分发一个控制请求到领域服务。Web 控制台与 CLI 共用这套逻辑。
pub async fn handle_request(state: &Arc<AppState>, req: ControlRequest) -> ControlResponse {
    let outcome = match req {
        ControlRequest::Status => Ok(ControlResponse::ok(api::daemon_status(state).await)),
        ControlRequest::ListSources => api::sources_list(state).await.map(ControlResponse::ok),
        ControlRequest::ListEvents { limit } => api::events_list(state, None, limit.unwrap_or(20))
            .await
            .map(ControlResponse::ok),
        ControlRequest::Diff { event_id } => api::event_get(state, event_id)
            .await
            .and_then(|event| {
                event.ok_or_else(|| Error::other(format!("event {event_id} not found")))
            })
            .map(ControlResponse::ok),
        ControlRequest::History { source_id, limit } => {
            api::events_list(state, source_id.as_deref(), limit.unwrap_or(20))
                .await
                .map(ControlResponse::ok)
        }
        ControlRequest::Check { source_id } => api::check_source(state, &source_id)
            .await
            .map(|()| ControlResponse::ok(Checked { source_id })),
        ControlRequest::SourcesAdd { source } => api::source_add(state, *source)
            .await
            .map(ControlResponse::ok),
        ControlRequest::SourcesUpdate { source } => {
            api::source_update(state, *source).await.map(|()| {
                ControlResponse::ok(Updated {
                    source_id: String::new(),
                })
            })
        }
        ControlRequest::SourcesDelete { source_id } => api::source_delete(state, &source_id)
            .await
            .map(|()| ControlResponse::ok(Updated { source_id })),
        ControlRequest::SourcesSetFlags {
            source_ids,
            enabled,
            notify_enabled,
        } => api::sources_set_flags(state, &source_ids, enabled, notify_enabled)
            .await
            .map(|updated| ControlResponse::ok(UpdatedCount { updated })),
        ControlRequest::TestSource { source_id } => api::test_source(state, &source_id)
            .await
            .map(ControlResponse::ok),
        ControlRequest::PreviewSource { url, engine } => {
            api::preview_source_title(state, &url, &engine)
                .await
                .map(ControlResponse::ok)
        }
        ControlRequest::MarkSourceRead { source_id } => api::source_mark_read(state, &source_id)
            .await
            .map(|updated| ControlResponse::ok(UpdatedCount { updated })),
        ControlRequest::MarkEventRead { event_id } => api::event_mark_read(state, event_id)
            .await
            .map(|updated| ControlResponse::ok(UpdatedCount { updated })),
        ControlRequest::ListTags => api::tags_list(state).await.map(ControlResponse::ok),
        ControlRequest::UpdateTag { tag } => {
            let name = tag.name.clone();
            api::tag_update(state, *tag)
                .await
                .map(|()| ControlResponse::ok(TagUpdated { name }))
        }
        ControlRequest::DeleteTag { name } => api::tag_delete(state, &name)
            .await
            .map(|deleted| ControlResponse::ok(TagDeleted { name, deleted })),
        ControlRequest::NotifyTest { chat_id } => notify_test(state, chat_id).await,
        ControlRequest::GetSettings => api::settings_get(state).await.map(ControlResponse::ok),
        ControlRequest::UpdateSettings { settings } => {
            api::settings_update(state, *settings).await.map(|()| {
                ControlResponse::ok(SettingsSaved {
                    saved: true,
                    applied: true,
                    immediate: true,
                    restart_required: false,
                    config: "SQLite (settings 表)",
                })
            })
        }
        ControlRequest::Backup => api::backup_create(state).await.map(ControlResponse::ok),
        ControlRequest::ListBackups => {
            api::backup_list(state).map(|backups| ControlResponse::ok(BackupList { backups }))
        }
        ControlRequest::Restore { name } => api::backup_restore(state, &name).await.map(|()| {
            ControlResponse::ok(Restored {
                restored: true,
                name,
            })
        }),
        ControlRequest::DeleteBackup { name } => api::backup_delete(state, &name)
            .map(|deleted| ControlResponse::ok(BackupDeleted { deleted, name })),
        ControlRequest::RestoreUpload { path } => {
            api::backup_restore_upload(state, &path).await.map(|name| {
                ControlResponse::ok(Restored {
                    restored: true,
                    name,
                })
            })
        }
        ControlRequest::Shutdown => {
            state
                .running
                .store(false, std::sync::atomic::Ordering::Relaxed);
            Ok(ControlResponse::ok(Shutdown { shutdown: true }))
        }
    };
    match outcome {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "control request failed");
            ControlResponse::err(e)
        }
    }
}

/// 发送测试通知。
async fn notify_test(
    state: &Arc<AppState>,
    chat_id: Option<String>,
) -> std::result::Result<ControlResponse, Error> {
    let notifier = state
        .notifier
        .read()
        .unwrap()
        .clone()
        .ok_or_else(|| Error::other("telegram notifier disabled"))?;
    let message_id = notifier.send_test(chat_id.as_deref()).await?;
    Ok(ControlResponse::ok(MessageSent { message_id }))
}

#[derive(Debug, Serialize)]
struct Checked {
    source_id: String,
}

#[derive(Debug, Serialize)]
struct Updated {
    source_id: String,
}

#[derive(Debug, Serialize)]
struct UpdatedCount {
    updated: usize,
}

#[derive(Debug, Serialize)]
struct TagUpdated {
    name: String,
}

#[derive(Debug, Serialize)]
struct TagDeleted {
    name: String,
    deleted: bool,
}

#[derive(Debug, Serialize)]
struct SettingsSaved {
    saved: bool,
    applied: bool,
    immediate: bool,
    restart_required: bool,
    config: &'static str,
}

#[derive(Debug, Serialize)]
struct BackupList {
    backups: Vec<api::BackupInfo>,
}

#[derive(Debug, Serialize)]
struct Restored {
    restored: bool,
    name: String,
}

#[derive(Debug, Serialize)]
struct BackupDeleted {
    deleted: bool,
    name: String,
}

#[derive(Debug, Serialize)]
struct MessageSent {
    message_id: i64,
}

#[derive(Debug, Serialize)]
struct Shutdown {
    shutdown: bool,
}

// ---- 传输层 ----

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
    // 非 Unix 平台的本地开发回退：仅绑定回环地址。
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

/// 逐行读取请求、分发、写回响应。
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
        let resp = match serde_json::from_str::<ControlRequest>(&line) {
            Ok(req) => handle_request(&state, req).await,
            Err(e) => ControlResponse::err(format!("invalid request: {e}")),
        };
        write_response(&mut writer, &resp).await?;
    }
    Ok(())
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
