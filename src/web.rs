//! Web 控制台 HTTP 层：axum 路由 + Bearer 鉴权 + 静态资源托管。
//!
//! 这一层**只负责 HTTP 语义**：路由、参数解析、鉴权、状态码与响应序列化。
//! 业务逻辑全部委托给 [`crate::api`]，因此这里没有重复的领域规则。
//!
//! 响应统一为 `{ ok, result, error }` 信封，由 [`ApiError`] 统一转换错误。

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

use crate::api;
use crate::config::{Config, EditableSettings, SourceConfig};
use crate::error::{Error, Result};
use crate::models::TagConfig;
use crate::scheduler::AppState;

/// 上传 zip 备份的最大允许体积（默认 4 GiB）。
/// 上传内容会先在内存中缓冲再落盘，此上限防止超大/恶意上传耗尽内存。
const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024 * 1024;

/// 统一响应信封：所有 `/api/*` 接口都返回这个形状。
#[derive(Debug, Serialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    error: Option<String>,
}

impl<T: Serialize> Envelope<T> {
    fn ok(result: T) -> Json<Self> {
        Json(Self {
            ok: true,
            result: Some(result),
            error: None,
        })
    }
}

/// 错误响应：业务失败一律 400（客户端可修正），未找到映射为 404。
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(e: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: e.to_string(),
        }
    }
    fn not_found(e: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: e.to_string(),
        }
    }
    fn internal(e: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(Envelope::<()> {
            ok: false,
            result: None,
            error: Some(self.message),
        });
        (self.status, body).into_response()
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::Config(_) | Error::Other(_) | Error::Control(_) => Self::bad_request(e),
            Error::Io(_) if is_not_found(&e) => Self::not_found(e),
            _ => Self::internal(e),
        }
    }
}

fn is_not_found(e: &Error) -> bool {
    matches!(e, Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound)
}

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
        .map_err(|e| Error::other(format!("web server error: {e}")))?;
    Ok(())
}

/// 对 `/api/*` 请求做 Bearer Token 鉴权。
///
/// `web.auth_token` 为空时不启用鉴权（默认仅本地访问）；非空时要求
/// `Authorization: Bearer <token>`，否则 401。由于授权后的接口可修改监控源、
/// 设置、备份等敏感数据，鉴权失败必须硬拒绝，不提供降级路径。
async fn require_auth(State(cfg): State<Config>, req: Request, next: Next) -> Response {
    let expected = cfg.web.auth_token.trim();
    if expected.is_empty() {
        return next.run(req).await;
    }
    let supplied = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    if supplied.is_some_and(|s| constant_time_eq(s.as_bytes(), expected.as_bytes())) {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(Envelope::<()> {
                ok: false,
                result: None,
                error: Some("unauthorized: missing or invalid auth token".into()),
            }),
        )
            .into_response()
    }
}

/// 恒定时间字符串比较，避免时序侧信道泄露 token 长度 / 内容。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 构建路由：`/api/*` JSON 接口 + 前端静态资源（SPA 路由回退到 index.html）。
fn build_router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/sources", get(sources_list).post(source_add))
        .route("/sources/preview", post(source_preview))
        .route("/sources/batch", post(sources_batch))
        .route("/sources/{id}", put(source_update).delete(source_delete))
        .route("/sources/{id}/test", post(source_test))
        .route("/sources/{id}/read", post(source_mark_read))
        .route("/events", get(events_list))
        .route("/events/{id}", get(event_get))
        .route("/events/{id}/read", post(event_mark_read))
        .route("/events/{id}/screenshot", get(event_screenshot))
        .route("/tags", get(tags_list))
        .route("/tags/{name}", put(tag_update).delete(tag_delete))
        .route("/check", post(check))
        .route("/history", get(history))
        .route("/notify-test", post(notify_test))
        .route("/settings", get(settings_get).put(settings_update))
        .route("/backup", post(backup_create))
        .route("/backups", get(backups_list))
        .route("/backups/{name}", delete(backup_delete))
        .route("/backups/{name}/download", get(backup_download))
        .route("/restore", post(backup_restore))
        .route("/restore/upload", post(backup_restore_upload))
        // 放行上传体积限制到 MAX_UPLOAD_BYTES（默认 body 上限过小）。
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES));

    let static_dir = state.cfg.web.static_dir();
    // 鉴权中间件只需一份只读 Config（读 web.auth_token）。
    let auth_cfg = state.cfg.clone();

    Router::new()
        .nest(
            "/api",
            api
                // 未知 /api/* 路径返回 JSON 404，避免落进前端 SPA 回退
                // 而得到一个 HTML 页面（前端 JSON 解析会失败且难以定位）。
                .fallback(api_not_found)
                .layer(middleware::from_fn_with_state(auth_cfg, require_auth)),
        )
        .with_state(state)
        // 静态资源命中则直接返回；未命中（SPA 深链如 /sources、/settings）
        // 一律回退到 index.html，交给前端路由处理。
        .fallback_service(ServeDir::new(&static_dir).fallback(spa_fallback(static_dir)))
}

/// 未知 API 路径：返回 JSON 404（而非 HTML）。
async fn api_not_found(req: Request) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(Envelope::<()> {
            ok: false,
            result: None,
            error: Some(format!("unknown api path: {}", req.uri().path())),
        }),
    )
        .into_response()
}

/// SPA 回退服务：任何未命中的静态路径都返回 `index.html`。
///
/// 用 `ServeDir::fallback` 而非 `not_found_service`：后者会透传 404 状态码，
/// 浏览器虽仍渲染页面，但健康检查 / 爬虫会误判为缺页。
fn spa_fallback(static_dir: std::path::PathBuf) -> ServeFile {
    ServeFile::new(static_dir.join("index.html"))
}

// ---- 状态 ----

async fn status(State(state): State<Arc<AppState>>) -> Json<Envelope<crate::models::DaemonStatus>> {
    Envelope::ok(api::daemon_status(&state).await)
}

// ---- 监控源 ----

async fn sources_list(
    State(state): State<Arc<AppState>>,
) -> std::result::Result<Json<Envelope<Vec<crate::models::SourceMeta>>>, ApiError> {
    Ok(Envelope::ok(api::sources_list(&state).await?))
}

async fn source_add(
    State(state): State<Arc<AppState>>,
    Json(source): Json<SourceConfig>,
) -> std::result::Result<Json<Envelope<api::AddedSource>>, ApiError> {
    Ok(Envelope::ok(api::source_add(&state, source).await?))
}

async fn source_update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut source): Json<SourceConfig>,
) -> std::result::Result<Json<Envelope<UpdatedId>>, ApiError> {
    // URL 中的 id 权威，避免改名时把源拆成两个。
    source.id = id.clone();
    api::source_update(&state, source).await?;
    Ok(Envelope::ok(UpdatedId { source_id: id }))
}

async fn source_delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Envelope<UpdatedId>>, ApiError> {
    api::source_delete(&state, &id).await?;
    Ok(Envelope::ok(UpdatedId { source_id: id }))
}

/// 批量操作请求体：对多个监控源同时设置监控开关 / 通知开关（均可选）。
#[derive(Deserialize)]
struct BatchSourcesBody {
    source_ids: Vec<String>,
    enabled: Option<bool>,
    notify_enabled: Option<bool>,
}

async fn sources_batch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchSourcesBody>,
) -> std::result::Result<Json<Envelope<UpdatedCount>>, ApiError> {
    let updated =
        api::sources_set_flags(&state, &body.source_ids, body.enabled, body.notify_enabled).await?;
    Ok(Envelope::ok(UpdatedCount { updated }))
}

async fn source_test(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Envelope<serde_json::Value>>, ApiError> {
    Ok(Envelope::ok(api::test_source(&state, &id).await?))
}

/// 预览请求体：抓取 URL 并返回页面标题，用于添加监控源时自动填充名称。
#[derive(Deserialize)]
struct PreviewBody {
    url: String,
    /// 抓取引擎，缺省为 http。
    #[serde(default)]
    engine: String,
}

async fn source_preview(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PreviewBody>,
) -> std::result::Result<Json<Envelope<api::PreviewedTitle>>, ApiError> {
    Ok(Envelope::ok(
        api::preview_source_title(&state, &body.url, &body.engine).await?,
    ))
}

#[derive(Debug, Serialize)]
struct UpdatedId {
    source_id: String,
}

#[derive(Debug, Serialize)]
struct UpdatedCount {
    updated: usize,
}

// ---- 变更事件 ----

async fn events_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
) -> std::result::Result<Json<Envelope<Vec<crate::models::ChangeEvent>>>, ApiError> {
    let events = api::events_list(&state, None, params.limit.unwrap_or(20)).await?;
    Ok(Envelope::ok(events))
}

async fn event_get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> std::result::Result<Json<Envelope<crate::models::ChangeEvent>>, ApiError> {
    let event = api::event_get(&state, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("event {id} not found")))?;
    Ok(Envelope::ok(event))
}

async fn event_mark_read(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> std::result::Result<Json<Envelope<UpdatedCount>>, ApiError> {
    Ok(Envelope::ok(UpdatedCount {
        updated: api::event_mark_read(&state, id).await?,
    }))
}

async fn source_mark_read(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Envelope<UpdatedCount>>, ApiError> {
    Ok(Envelope::ok(UpdatedCount {
        updated: api::source_mark_read(&state, &id).await?,
    }))
}

/// 返回事件的 camofox 截图（流式读取，避免整图占内存）。
async fn event_screenshot(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> std::result::Result<Response, ApiError> {
    let path = api::event_screenshot_file(&state, id).await?;
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(ApiError::not_found)?;
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(axum::body::Body::from_stream(
            tokio_util::io::ReaderStream::new(file),
        ))
        .map_err(ApiError::internal)?
        .into_response())
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

// ---- 分组 ----

async fn tags_list(
    State(state): State<Arc<AppState>>,
) -> std::result::Result<Json<Envelope<Vec<TagConfig>>>, ApiError> {
    Ok(Envelope::ok(api::tags_list(&state).await?))
}

async fn tag_update(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(mut tag): Json<TagConfig>,
) -> std::result::Result<Json<Envelope<UpdatedName>>, ApiError> {
    // URL 路径中的分组名权威，避免重命名造成分组漂移。
    tag.name = name.clone();
    api::tag_update(&state, tag).await?;
    Ok(Envelope::ok(UpdatedName { name }))
}

async fn tag_delete(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> std::result::Result<Json<Envelope<UpdatedName>>, ApiError> {
    api::tag_delete(&state, &name).await?;
    Ok(Envelope::ok(UpdatedName { name }))
}

#[derive(Debug, Serialize)]
struct UpdatedName {
    name: String,
}

// ---- 检测 / 历史 / 通知测试 ----

#[derive(Deserialize)]
struct SourceIdBody {
    source_id: String,
}

async fn check(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SourceIdBody>,
) -> std::result::Result<Json<Envelope<UpdatedId>>, ApiError> {
    api::check_source(&state, &body.source_id).await?;
    Ok(Envelope::ok(UpdatedId {
        source_id: body.source_id,
    }))
}

#[derive(Deserialize)]
struct HistoryQuery {
    source_id: Option<String>,
    limit: Option<usize>,
}

async fn history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> std::result::Result<Json<Envelope<Vec<crate::models::ChangeEvent>>>, ApiError> {
    let events = api::events_list(
        &state,
        params.source_id.as_deref(),
        params.limit.unwrap_or(20),
    )
    .await?;
    Ok(Envelope::ok(events))
}

#[derive(Deserialize)]
struct NotifyTestBody {
    chat_id: Option<String>,
}

async fn notify_test(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NotifyTestBody>,
) -> std::result::Result<Json<Envelope<MessageSent>>, ApiError> {
    let notifier = state
        .notifier
        .read()
        .unwrap()
        .clone()
        .ok_or_else(|| ApiError::bad_request("telegram notifier disabled"))?;
    let message_id = notifier
        .send_test(body.chat_id.as_deref())
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Envelope::ok(MessageSent { message_id }))
}

#[derive(Debug, Serialize)]
struct MessageSent {
    message_id: i64,
}

// ---- 设置 ----

async fn settings_get(
    State(state): State<Arc<AppState>>,
) -> std::result::Result<Json<Envelope<EditableSettings>>, ApiError> {
    Ok(Envelope::ok(api::settings_get(&state).await?))
}

/// 设置保存结果：全部字段保存即生效，无需重启。
#[derive(Debug, Serialize)]
struct SettingsSaved {
    saved: bool,
    applied: bool,
    immediate: bool,
    restart_required: bool,
    config: &'static str,
}

async fn settings_update(
    State(state): State<Arc<AppState>>,
    Json(settings): Json<EditableSettings>,
) -> std::result::Result<Json<Envelope<SettingsSaved>>, ApiError> {
    api::settings_update(&state, settings).await?;
    Ok(Envelope::ok(SettingsSaved {
        saved: true,
        applied: true,
        immediate: true,
        restart_required: false,
        config: "SQLite (settings 表)",
    }))
}

// ---- 备份与恢复 ----

async fn backup_create(
    State(state): State<Arc<AppState>>,
) -> std::result::Result<Json<Envelope<api::BackupInfo>>, ApiError> {
    Ok(Envelope::ok(api::backup_create(&state).await?))
}

#[derive(Debug, Serialize)]
struct BackupList {
    backups: Vec<api::BackupInfo>,
}

async fn backups_list(State(state): State<Arc<AppState>>) -> Json<Envelope<BackupList>> {
    // 列出备份是只读的目录扫描：失败时返回空列表，不影响控制台其它功能。
    let backups = api::backup_list(&state).unwrap_or_default();
    Envelope::ok(BackupList { backups })
}

/// 下载指定备份的 zip 包；zip 不存在时现场打包后返回。
async fn backup_download(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> std::result::Result<Response, ApiError> {
    let zip_path = api::backup_zip_path(&state, &name)?;
    let file = tokio::fs::File::open(&zip_path)
        .await
        .map_err(ApiError::not_found)?;
    let meta = std::fs::metadata(&zip_path).ok();
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}.zip\""),
        );
    if let Some(meta) = meta {
        builder = builder.header(header::CONTENT_LENGTH, meta.len().to_string());
    }
    builder
        .body(axum::body::Body::from_stream(
            tokio_util::io::ReaderStream::new(file),
        ))
        .map(|r| r.into_response())
        .map_err(ApiError::internal)
}

#[derive(Deserialize)]
struct RestoreBody {
    name: String,
}

async fn backup_restore(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RestoreBody>,
) -> std::result::Result<Json<Envelope<Restored>>, ApiError> {
    api::backup_restore(&state, &body.name).await?;
    Ok(Envelope::ok(Restored {
        restored: true,
        name: body.name,
    }))
}

async fn backup_delete(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> std::result::Result<Json<Envelope<BackupDeleted>>, ApiError> {
    let deleted = api::backup_delete(&state, &name)?;
    if !deleted {
        return Err(ApiError::not_found(format!("backup {name} not found")));
    }
    Ok(Envelope::ok(BackupDeleted { deleted, name }))
}

/// 处理上传 zip 备份并在线恢复。multipart 字段名固定为 `file`。
async fn backup_restore_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> std::result::Result<Json<Envelope<Restored>>, ApiError> {
    let bytes = read_upload(&mut multipart).await?;
    // 校验 zip 头（PK），避免无意义的落盘与解压。
    if bytes.len() < 4 || &bytes[0..2] != b"PK" {
        return Err(ApiError::bad_request("上传的不是 zip 备份包"));
    }
    let tmp = temp_upload_path(&state);
    std::fs::write(&tmp, &bytes).map_err(ApiError::internal)?;
    let name = api::backup_restore_upload(&state, &tmp)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Envelope::ok(Restored {
        restored: true,
        name,
    }))
}

/// 从 multipart 中取出第一个 `file` 字段的内容。
async fn read_upload(multipart: &mut Multipart) -> std::result::Result<Vec<u8>, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart 解析失败: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(format!("读取上传内容失败: {e}")))?;
        if !data.is_empty() {
            if data.len() > MAX_UPLOAD_BYTES {
                return Err(ApiError::bad_request(format!(
                    "上传的 zip 超过大小上限 ({} MiB)",
                    MAX_UPLOAD_BYTES / (1024 * 1024)
                )));
            }
            return Ok(data.to_vec());
        }
    }
    Err(ApiError::bad_request("未收到 zip 文件"))
}

/// 上传临时文件路径：放在 backups 目录下（时间戳保证唯一，避免并发互相覆盖）。
fn temp_upload_path(state: &Arc<AppState>) -> std::path::PathBuf {
    let uniq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = state
        .state_dir()
        .join(crate::backup::BACKUP_SUBDIR)
        .join(format!("upload-restore-{uniq}.zip"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
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
