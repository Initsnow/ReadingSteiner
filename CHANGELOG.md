# Changelog

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
