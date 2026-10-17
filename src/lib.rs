//! ReadingSteiner：网页 / 数据接口变更检测与 Telegram 推送。
//!
//! # 分层
//!
//! ```text
//! web (HTTP REST API) ──► api（领域服务：业务唯一实现）──► scheduler / db / fetcher / notifier
//! ```
//!
//! - [`api`]：所有业务操作的实现，与传输方式无关。
//! - [`web`]：axum 路由 + 鉴权中间件，负责 HTTP RESTful API 与前端静态资源。
//! - [`cli`]：轻量 HTTP 客户端与 `serve` 启动入口。
//! - [`scheduler`]：调度主循环 + 单次检测流程，持有 [`scheduler::AppState`]。
//! - [`config`] / [`models`]：配置与领域模型定义。

pub mod api;
pub mod backup;
pub mod cli;
pub mod config;
pub mod cron_expr;
pub mod db;
pub mod differ;
pub mod error;
pub mod fetcher;
pub mod images;
pub mod models;
pub mod net_guard;
pub mod notifier;
pub mod pipeline;
pub mod scheduler;
pub mod web;

pub use config::Config;
pub use error::Error;
