# 配置

`config.yaml` 是 ReadingSteiner 的单一真理源（SSOT）。daemon 启动时读取该文件，Web 控制台和
`reading-steiner settings` 也在允许写入时直接修改它并即时热更新。

## config.yaml

仓库根目录的 [`config.yaml`](../config.yaml) 提供完整示例：

```yaml
state_dir: state
media_dir: state/media

daemon:
  log_level: info
  concurrency: 16
  queue_capacity: 1024
  default_timeout_secs: 120
  default_cron: 0 12 * * *
  default_user_agent: Mozilla/5.0 ...
  history_limit_per_source: 0
  failure_notify_threshold: 0
  timezone: Asia/Shanghai

web:
  listen: 127.0.0.1:8901
  static_dir: web/dist

telegram:
  url: ''
  api_base: https://api.telegram.org
  max_images_per_event: 10
  image_bytes_budget: 10485760
  digest_window_secs: 30
  template: ''

browser:
  enabled: true
  cdp_url: ws://127.0.0.1:9222/devtools/browser
  timeout_secs: 120
```

敏感值可使用文件路径间接加载：

- `telegram.url_file` 指向 Telegram URL 密钥文件。
- `web.auth_token_file` 指向 Web 控制台 Bearer Token 文件。

文件路径会保留在 `config.yaml` 中，密钥内容本身无需写入配置。

## 运行参数

| 设置 | 说明 |
|---|---|
| `daemon.concurrency` | 最大并发抓取任务数（默认 16） |
| `daemon.queue_capacity` | 调度队列容量（默认 1024） |
| `daemon.default_timeout_secs` | 默认请求超时秒数（默认 120） |
| `daemon.default_cron` | 新建源未指定周期时使用的 cron（默认每天 12:00；留空也回退到该值） |
| `daemon.default_user_agent` | 默认 User-Agent |
| `daemon.history_limit_per_source` | 每源快照保留上限，0 表示不限制 |
| `daemon.failure_notify_threshold` | 连续失败告警阈值，0 表示禁用 |
| `daemon.timezone` | 调度与通知使用的 IANA 时区 |
| `telegram.template` | 变更通知模板，空值使用内置模板 |
| `telegram.url` | 全局通知目标（`tgram://`） |
| `telegram.max_images_per_event` | 单事件最多附带图片数（默认 10） |
| `browser.enabled` | 是否启用 CDP 浏览器引擎 |
| `browser.cdp_url` | Chrome/Chromium CDP 地址 |

### 热更新

通过 Web 控制台保存后，配置会原子写回 `config.yaml` 并即时应用到运行时：

- `concurrency` 和 `queue_capacity` 由调度循环在下一轮读取。
- `timezone`、`default_cron`、`default_timeout_secs`、`default_user_agent` 等字段在后续任务中即时使用。
- Telegram 目标和模板变更后会重建 notifier，无需重启 daemon。

### 只读部署

ReadingSteiner 会根据配置文件的实际写权限返回 `readonly`。只读模式下：

- Web 控制台禁用可编辑字段并隐藏保存按钮。
- `PUT /api/settings` 和服务端 `Config::save` 都会拒绝写入。

NixOS 模块将声明式配置安装为 `root:<service-group> 0440`，daemon 用户只能读取。因此参数应通过
`services.reading-steiner` 修改，而不是在 WebUI 中编辑。

## 通知目标（tgram://）

一个 URL 同时携带 bot token 与接收者 chat id：

```text
tgram://bottoken/ChatID
tgram://bottoken/ChatID1/ChatID2/ChatIDN
```

- 全局目标通过 `telegram.url` 或 Web「系统设置」配置。
- 分组可覆盖 `notify_url`；留空时沿用全局目标。
- 多个 chat id 会把同一条通知推送到所有接收者。
