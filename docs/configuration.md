# 配置

ReadingSteiner 的配置分两层，职责清晰、无重叠：

1. **`config.yaml`（启动引导）**——只放 daemon 启动时所需的引导项（目录、监听、socket、camofox、telegram api_base 等）。
2. **SQLite `settings` 表（全局可编辑设置）**——运行时并发数、默认超时、cron、UA、历史保留、失败通知阈值、时区、通知模板、通知目标、单事件图片数等，通过 **Web 控制台「设置」页**或 `reading-steiner settings` 命令直接读写，**不在 `config.yaml` 中配置**。

## config.yaml

见仓库根目录 [`config.yaml`](../config.yaml)。只包含启动引导项：

```yaml
state_dir: state          # 状态与数据库目录
media_dir: state/media    # 图片等媒体缓存目录

daemon:
  socket_path: state/daemon.sock   # CLI 与 daemon 通信的 socket
  log_level: info                  # 日志级别（实际以 RUST_LOG 环境变量优先）

web:
  listen: 127.0.0.1:8901   # Web 控制台监听地址
  static_dir: web/dist     # 前端构建产物目录

telegram:
  api_base: https://api.telegram.org
  image_bytes_budget: 10485760
  digest_window_secs: 30

camofox:
  enabled: false
  base_url: http://127.0.0.1:9377
  ...
```

`daemon` / `telegram` 段的**可编辑运行参数**（并发、队列、超时、cron、UA、历史保留、失败阈值、时区、通知目标、模板、图片数）**只存在于 SQLite**，config.yaml 中配置无效。保存后全部**即时生效**。

## 全局设置（SQLite）

全局设置存于 `state/reading-steiner.db` 的 `settings` 表，可通过 Web 控制台「设置」页或 `reading-steiner settings` 命令查看。

| 设置 | 说明 |
|---|---|
| `concurrency` | 抓取工作线程数 |
| `queue_capacity` | 队列容量 |
| `default_timeout_secs` | 全局默认请求超时（秒） |
| `default_cron` | 全局默认 cron（新建源未配置时使用） |
| `default_user_agent` | 默认 User-Agent |
| `history_limit_per_source` | 每源保留历史条数（0 不限制） |
| `failure_notify_threshold` | 连续失败告警阈值（0 禁用） |
| `timezone` | 时区（IANA 名称，留空用系统本地时区） |
| `template` | 变更通知模板 |
| `telegram_url` | 全局通知目标（`tgram://`） |
| `max_images_per_event` | 单事件最多附带图片数 |

### 热更新

所有全局设置保存后**即时生效**。

其中：

- `concurrency`（并发数）由 daemon 调度循环在每轮动态调整信号量许可数，即时生效。
- `queue_capacity`（队列容量）在每轮入队时读取，即时生效。
- 其余字段（`timezone`、`default_cron`、`default_user_agent`、`default_timeout_secs`、
  `telegram_url`、`template`、`max_images_per_event`、`failure_notify_threshold`、
  `history_limit_per_source`）由 runtime / notifier 在每次使用时读取，保存即生效。

### 通知目标（tgram://）

通知配置统一使用 `tgram://` 形式，一个 URL 同时携带 bot token 与接收者 chat id：

```
tgram://bottoken/ChatID
tgram://bottoken/ChatID1/ChatID2/ChatIDN
```

- 全局在 Web「设置」页的「Telegram 通知目标」中配置。
- **分组**可在「分组管理」里为某个分组单独配置 `notify_url`；留空则沿用全局。
- 多 chat id 会把同一条通知推送到所有列出的 chat。
