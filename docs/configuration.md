# 配置

ReadingSteiner 的配置分两层：

1. **`config.yaml`（启动引导）**——只放 daemon 启动时所需的引导项（目录、监听、camofox、telegram api_base 等）。
2. **SQLite `settings` 表（全局可编辑设置）**——运行时并发数、默认超时、cron、UA、历史保留、失败通知阈值、时区、通知模板、通知目标、单事件图片数等，通过 **Web 控制台「设置」页**直接读写，**不再写回 `config.yaml`**。

## config.yaml

见仓库根目录 [`config.yaml`](../config.yaml)。核心字段：

```yaml
state_dir: state          # 状态与数据库目录
media_dir: state/media    # 图片等媒体缓存目录

daemon:
  socket_path: state/daemon.sock   # CLI 与 daemon 通信的 socket
  log_level: info

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

### daemon 段中已迁移到 SQLite 的字段

以下在 `daemon` 段配置的**可编辑运行参数**已迁移到 SQLite，启动时以 SQLite 中的值为准（若未配置则回退到 `config.yaml` / 默认值）：

- `concurrency`——抓取并发数。
- `queue_capacity`——队列容量。
- `default_timeout_secs`——全局默认请求超时秒数（单源可覆盖）。
- `default_cron`——全局默认 cron 表达式。
- `default_user_agent`——默认 User-Agent。
- `history_limit_per_source`——每个监控源保留历史条数（0 不限制）。
- `failure_notify_threshold`——连续失败告警阈值（0 禁用）。
- `timezone`——调度器/展示时区（IANA 名称）。

### telegram 段中已迁移到 SQLite 的字段

- `url`——全局通知目标（`tgram://bottoken/ChatID...`）。
- `max_images_per_event`——单事件最多附带图片数。
- `template`——变更通知模板。

> 这些字段现在请通过 Web 控制台「设置」页修改。除**并发数 / 队列容量**外，其余字段在保存后**即时（或下次任务）生效**，无需重启 daemon。

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

### 生效档位（热更新）

保存设置后按「生效档位」处理，Web 控制台会以徽标标注每项字段：

| 档位 | 含义 | 字段 |
|---|---|---|
| **即时生效** | 保存后立刻生效，无需重启 | `telegram_url`、`template`、`max_images_per_event`、`failure_notify_threshold`、`history_limit_per_source` |
| **下次任务生效** | 下一次调度 / 下一次建源时读取，无需重启 daemon | `timezone`、`default_cron`、`default_user_agent`、`default_timeout_secs` |
| **需重启** | 启动时一次性分配线程池 / 队列，改动需重启 daemon 生效 | `concurrency`、`queue_capacity` |

即时生效字段由 notifier / runtime 在每次使用时读取；「下次任务生效」字段由调度器在下一轮调度或新建监控源时装载。并发数 / 队列容量因涉及工作线程池与有界队列的启动期分配，强行热改收益低且易引入竞态，故保留重启生效。

### 通知目标（tgram://）

通知配置统一使用 `tgram://` 形式，一个 URL 同时携带 bot token 与接收者 chat id：

```
tgram://bottoken/ChatID
tgram://bottoken/ChatID1/ChatID2/ChatIDN
```

- 全局在 Web「设置」页的「Telegram 通知目标」中配置。
- **分组**可在「分组管理」里为某个分组单独配置 `notify_url`；留空则沿用全局。
- 多 chat id 会把同一条通知推送到所有列出的 chat。
