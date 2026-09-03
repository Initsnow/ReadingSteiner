//! ReadingSteiner：网页 / 数据接口变更检测与 Telegram 推送。
//!
//! # 分层
//!
//! ```text
//! web (HTTP/JSON)  ─┐
//!                   ├─► api（领域服务：业务唯一实现）──► scheduler / db / fetcher / notifier
//! control (socket) ─┘
//! ```
//!
//! - [`api`]：所有业务操作的实现，与传输方式无关。新增接口只需在这里写一次。
//! - [`web`]：axum 路由 + 鉴权中间件，负责 HTTP 语义（状态码、二进制响应）。
//! - [`control`]：Unix socket 行协议，只做请求分发与序列化。
//! - [`scheduler`]：调度主循环 + 单次检测流程，持有 [`scheduler::AppState`]。
//! - [`config`] / [`models`]：配置与领域模型定义。

pub mod api;
pub mod backup;
pub mod cli;
pub mod config;
pub mod control;
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
