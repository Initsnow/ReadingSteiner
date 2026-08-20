//! Web 控制台 HTTP API 与静态资源服务。
//!
//! daemon 内置一个轻量 HTTP 服务（axum），对外暴露 `/api/*` JSON 接口，
//! 并托管前端构建产物（默认 `web/dist`），供 React + shadcn.ui 控制台调用。
//! 监听地址可通过 config.yaml 的 `web.listen` 配置（默认 `127.0.0.1:8901`）。

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

use crate::control::{self, ControlRequest, ControlResponse};
use crate::error::Result;
use crate::scheduler::AppState;

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
        .route("/sources", get(api_list_sources))
        .route("/events", get(api_list_events))
        .route("/events/{id}", get(api_get_event))
        .route("/check", post(api_check))
        .route("/test-pipeline", post(api_test_pipeline))
        .route("/history", get(api_history))
        .route("/notify-test", post(api_notify_test))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback_service(
            ServeDir::new(&static_dir).not_found_service(ServeFile::new(index)),
        )
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

#[derive(Deserialize)]
struct ListEventsQuery {
    limit: Option<usize>,
}

async fn api_list_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListEventsQuery>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(&state, ControlRequest::ListEvents { limit: params.limit })
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
        control::handle_request(&state, ControlRequest::Check { source_id: body.source_id })
            .await,
    )
    .await
}

async fn api_test_pipeline(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SourceIdBody>,
) -> (StatusCode, Json<Value>) {
    json_response(
        control::handle_request(
            &state,
            ControlRequest::TestPipeline {
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
        control::handle_request(&state, ControlRequest::NotifyTest { chat_id: body.chat_id })
            .await,
    )
    .await
}
