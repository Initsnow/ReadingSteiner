//! Web 控制台 HTTP API 与静态资源服务。
//!
//! daemon 内置一个轻量 HTTP 服务（axum），对外暴露 `/api/*` JSON 接口，
//! 并托管前端构建产物（默认 `web/dist`），供 React + shadcn.ui 控制台调用。
//! 监听地址可通过 config.yaml 的 `web.listen` 配置（默认 `127.0.0.1:8901`）。

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

use crate::config::{EditableSettings, SourceConfig};
use crate::control::{self, ControlRequest, ControlResponse};
use crate::error::Result;
use crate::scheduler::AppState;

/// 上传 zip 备份的最大允许体积（默认 4 GiB，覆盖绝大多数含 media 的备份）。
/// 由于上传会先在内存中缓冲（`field.bytes()` 后再落盘），此上限用于防止
/// 超大/恶意上传导致内存耗尽。
const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024 * 1024;

/// 启动 Web 控制台 HTTP 服务（阻塞直至监听器结束）。
pub async fn serve_web(state: Arc<AppState>) -> Result<()> {
    let app = build_router(state.clone());
    let listen = state.cfg.web.effective_listen();
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    info!(
        addr = %listen,
        static_dir = %state.cfg.web.static_dir().display(),
        "web console listening"
    );
    axum::serve(listener, app)
        .await
        .map_err(|e| crate::error::Error::other(format!("web server error: {e}")))?;
    Ok(())
}

/// 构建 axum 路由：`/api/*` JSON 接口 + 前端静态资源（SPA 路由回退到 index.html）。
fn build_router(state: Arc<AppState>) -> Router {
    let static_dir = state.cfg.web.static_dir();
    let index = static_dir.join("index.html");

    let api = Router::new()
        .route("/status", get(api_status))
        .route("/sources", get(api_list_sources).post(api_add_source))
        .route(
            "/sources/{id}",
            put(api_update_source).delete(api_delete_source),
        )
        .route("/sources/{id}/test", post(api_test_source))
        .route("/sources/preview", post(api_preview_source))
        .route("/sources/batch", post(api_batch_sources))
        .route("/events", get(api_list_events))
        .route("/events/{id}", get(api_get_event))
        .route("/check", post(api_check))
        .route("/history", get(api_history))
        .route("/notify-test", post(api_notify_test))
        .route("/settings", get(api_get_settings).put(api_update_settings))
        .route("/backup", post(api_backup))
        .route("/backups", get(api_list_backups))
        .route("/backups/{name}", delete(api_delete_backup))
        .route("/backups/{name}/download", get(api_download_backup))
        .route("/restore", post(api_restore))
        .route("/restore/upload", post(api_restore_upload))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(&static_dir).not_found_service(ServeFile::new(index)))
}

async fn json_response(resp: ControlResponse) -> (StatusCode, Json<Value>) {
    let code = if resp.ok {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        code,
        Json(json!({ "ok": resp.ok, "result": resp.result, "error": resp.error })),
    )
}

async fn api_status(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    json_response(control::handle_request(&state, ControlRequest::Status).await).await
}

async fn api_list_sources(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    json_response(control::handle_request(&state, ControlRequest::ListSources).await).await
}

async fn api_add_source(
    State(state): State<Arc<AppState>>,
    Json(source): Json<SourceConfig>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::SourcesAdd {
                source: Box::new(source),
            },
        )
        .await,
    )
    .await
}

async fn api_update_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(source): Json<SourceConfig>,
) -> (StatusCode, Json<Value>) {
    // Keep the URL-provided id authoritative so a rename doesn't silently split a source.
    let mut source = source;
    source.id = id;
    json_response(
        control::handle_request(
            &state,
            ControlRequest::SourcesUpdate {
                source: Box::new(source),
            },
        )
        .await,
    )
    .await
}

async fn api_delete_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(&state, ControlRequest::SourcesDelete { source_id: id }).await,
    )
    .await
}

/// 批量操作请求体：对多个监控源同时设置监控开关 / 通知开关。
/// 两个字段均为可选，前端可只改监控或只改通知（也可同时改）。
#[derive(Deserialize)]
struct BatchSourcesBody {
    /// 要批量更新的监控源 id 列表。
    source_ids: Vec<String>,
    /// 批量设置监控开关。缺省时不修改监控开关。
    enabled: Option<bool>,
    /// 批量设置通知开关。缺省时不修改通知开关。
    notify_enabled: Option<bool>,
}

async fn api_batch_sources(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchSourcesBody>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::SourcesSetFlags {
                source_ids: body.source_ids,
                enabled: body.enabled,
                notify_enabled: body.notify_enabled,
            },
        )
        .await,
    )
    .await
}

async fn api_test_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(&state, ControlRequest::TestSource { source_id: id }).await,
    )
    .await
}

/// 预览请求体：抓取 URL 并返回页面标题，用于添加监控源时自动填充名称。
#[derive(Deserialize)]
struct PreviewSourceBody {
    url: String,
    /// 抓取引擎，缺省为 http。
    #[serde(default)]
    engine: String,
}

async fn api_preview_source(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PreviewSourceBody>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::PreviewSource {
                url: body.url,
                engine: body.engine,
            },
        )
        .await,
    )
    .await
}

#[derive(Deserialize)]
struct ListEventsQuery {
    limit: Option<usize>,
}

async fn api_list_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListEventsQuery>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::ListEvents {
                limit: params.limit,
            },
        )
        .await,
    )
    .await
}

async fn api_get_event(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> (StatusCode, Json<Value>) {
    json_response(control::handle_request(&state, ControlRequest::Diff { event_id: id }).await)
        .await
}

#[derive(Deserialize)]
struct SourceIdBody {
    source_id: String,
}

async fn api_check(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SourceIdBody>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::Check {
                source_id: body.source_id,
            },
        )
        .await,
    )
    .await
}

#[derive(Deserialize)]
struct HistoryQuery {
    source_id: Option<String>,
    limit: Option<usize>,
}

async fn api_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::History {
                source_id: params.source_id,
                limit: params.limit,
            },
        )
        .await,
    )
    .await
}

#[derive(Deserialize)]
struct NotifyTestBody {
    chat_id: Option<String>,
}

async fn api_notify_test(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NotifyTestBody>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::NotifyTest {
                chat_id: body.chat_id,
            },
        )
        .await,
    )
    .await
}

async fn api_get_settings(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    json_response(control::handle_request(&state, ControlRequest::GetSettings).await).await
}

async fn api_update_settings(
    State(state): State<Arc<AppState>>,
    Json(settings): Json<EditableSettings>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::UpdateSettings {
                settings: Box::new(settings),
            },
        )
        .await,
    )
    .await
}

async fn api_backup(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    json_response(control::handle_request(&state, ControlRequest::Backup).await).await
}

async fn api_list_backups(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    json_response(control::handle_request(&state, ControlRequest::ListBackups).await).await
}

/// 下载指定备份的 zip 包。若 zip 不存在则尝试现场打包后返回。
async fn api_download_backup(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> std::result::Result<axum::response::Response, (StatusCode, Json<Value>)> {
    use axum::body::Body;
    use axum::http::header;

    // 备份名固定为 `YYYYMMDD-HHMMSS` 时间戳，仅允许数字与连字符，
    // 杜绝 `../` 等路径遍历（URL 编码后仍可能被解码拼接进文件路径）。
    if !crate::backup::is_valid_backup_name(&name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid backup name" })),
        ));
    }

    let state_dir = state.runtime.state_dir.clone();
    let dir = state_dir.join("backups").join(&name);
    if !dir.join("reading-steiner.db").exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "backup not found" })),
        ));
    }
    // 若 zip 尚未生成，则现场打包。
    let zip_path = state_dir.join("backups").join(format!("{name}.zip"));
    if !zip_path.exists()
        && let Err(e) = crate::backup::pack_zip(&dir, &zip_path)
    {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ));
    }
    let file = match tokio::fs::File::open(&zip_path).await {
        Ok(f) => f,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": e.to_string() })),
            ));
        }
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let mut builder = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}.zip\""),
        );
    if let Ok(meta) = std::fs::metadata(&zip_path) {
        builder = builder.header(header::CONTENT_LENGTH, meta.len().to_string());
    }
    match builder.body(body) {
        Ok(resp) => Ok(resp),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize)]
struct RestoreBody {
    name: String,
}

async fn api_restore(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RestoreBody>,
) -> (StatusCode, Json<Value>) {
    // 备份名同样校验，避免通过 restore 接口注入路径遍历。
    if !crate::backup::is_valid_backup_name(&body.name) {
        return json_response(ControlResponse::err("invalid backup name")).await;
    }
    json_response(
        control::handle_request(&state, ControlRequest::Restore { name: body.name }).await,
    )
    .await
}

async fn api_delete_backup(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> (StatusCode, Json<Value>) {
    // 备份名固定为时间戳，仅允许数字与连字符，杜绝路径遍历。
    if !crate::backup::is_valid_backup_name(&name) {
        return json_response(ControlResponse::err("invalid backup name")).await;
    }
    json_response(control::handle_request(&state, ControlRequest::DeleteBackup { name }).await)
        .await
}

/// 处理上传 zip 备份并恢复。
///
/// multipart 表单字段名固定为 `file`。先把上传内容落盘到临时文件，
/// 再交给 control 层（持有 DB 锁）完成解压与在线恢复。
async fn api_restore_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<Value>) {
    // 从 multipart 中取出第一个 file 字段。
    let mut bytes: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "ok": false, "error": format!("multipart 解析失败: {e}") })),
                );
            }
        };
        if field.name() == Some("file") {
            match field.bytes().await {
                Ok(data) if !data.is_empty() => {
                    bytes = Some(data.to_vec());
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "ok": false, "error": format!("读取上传内容失败: {e}") })),
                    );
                }
            }
        }
    }
    let bytes = match bytes {
        Some(b) if !b.is_empty() => b,
        _ => {
            return json_response(ControlResponse::err("未收到 zip 文件")).await;
        }
    };

    // 防止超大/恶意上传耗尽内存（上传与后续解压均会先在内存中缓冲）。
    if bytes.len() > MAX_UPLOAD_BYTES {
        return json_response(ControlResponse::err(format!(
            "上传的 zip 超过大小上限 ({} MiB)",
            MAX_UPLOAD_BYTES / (1024 * 1024)
        )))
        .await;
    }

    // 校验是合法 zip 头（PK\x03\x04 / PK\x05\x06 / PK\x07\x08）避免无意义落盘。
    if bytes.len() < 4 || &bytes[0..2] != b"PK" {
        return json_response(ControlResponse::err("上传的不是 zip 备份包")).await;
    }

    // 落盘到系统临时目录（用时间戳保证唯一，避免并发上传互相覆盖），交给 control 层恢复后清理。
    let uniq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = state
        .runtime
        .state_dir
        .join("backups")
        .join(format!("upload-restore-{uniq}.zip"));
    if let Some(parent) = tmp.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        return json_response(ControlResponse::err(format!("写入临时文件失败: {e}"))).await;
    }

    json_response(
        control::handle_request(&state, ControlRequest::RestoreUpload { path: tmp.clone() }).await,
    )
    .await
}
