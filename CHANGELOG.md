# Changelog

## [Unreleased]

### Changed
- **Web 控制台替代 TUI**：移除 `src/tui.rs`、`tui` 子命令及 ratatui/crossterm 依赖。
- daemon 内置 HTTP/JSON API（axum），并提供前端静态资源托管；监听地址由 `config.yaml` 的 `web.listen` 配置（默认 `127.0.0.1:8901`）。
- 新增 React + TypeScript + Vite + Tailwind + shadcn/ui 控制台（`web/`），含仪表盘 / 监控源 / 变更事件页面。
- CLI 新增 `web` 命令用于打印控制台地址。

## [0.1.0] - 2026-08-16

### Added
- Rust CLI + daemon skeleton: `serve`, `tui`, `status`, `sources add/list`, `check`, `test-pipeline`, `diff`, `notify test`, `history`.
- YAML configuration model for sources, pipelines, compare, Telegram, camofox.
- SQLite WAL schema: sources, snapshots, change_events, media_cache, notifications, schedule_state.
- HTTP fetcher with connection pooling, conditional requests (ETag / Last-Modified / 304), retries, body limit, BLAKE3 hashing.
- Extraction pipeline: CSS, XPath, JSONPath, regex, auto_text, auto_images, camofox_images; normalize and filter stages.
- Differ: raw_digest, item_set (new/updated/removed), text_sim fallback.
- Scheduler with concurrency semaphore, schedule_state persistence, backoff, outbox drain.
- Telegram notifier: sendMessage, sendPhoto, sendMediaGroup, outbox retry, configurable API base for mocks.
- Camofox adapter: health, tabs, navigate, wait, snapshot pagination, images, evaluate, screenshot, close; Bearer auth; circuit breaker; mock contract test.
- Image pipeline: HTML/JSON/camofox image extraction, downloader, MIME/size validation, SSRF private-IP rejection, SHA-256 cache, average-hash pHash.
- Unix-socket control plane (TCP fallback on non-Unix dev), JSON-RPC style commands.
- TUI with status/sources/events view.
- NixOS flake + module + VM integration test scaffold.
- Load generator binary (`loadgen`) for HTTP fetch path.
- README, example config, CI workflow.
