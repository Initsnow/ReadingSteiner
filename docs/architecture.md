# 架构

ReadingSteiner 是单进程 Rust daemon：内置调度器、抓取器、SQLite 存储与 Web 控制台。

## 分层

```text
       ┌─────────────────────────────────────────────┐
       │  传输层：web (HTTP/JSON)  ·  control (socket) │
       └───────────────────┬─────────────────────────┘
                           │  调用
       ┌───────────────────▼─────────────────────────┐
       │  api/：领域服务（业务唯一实现）                │
       └───────────────────┬─────────────────────────┘
                           │
       ┌───────────────────▼─────────────────────────┐
       │  scheduler · db · fetcher · pipeline ·       │
       │  differ · notifier · images · backup         │
       └─────────────────────────────────────────────┘
```

**业务规则只在 `src/api/` 实现一次。** Web 控制台与 CLI 都只是它的调用入口，
因此不会出现「Web 能改但 CLI 改不了」或两边行为漂移。新增一个能力只需：

1. 在 `src/api/<领域>.rs` 加一个返回领域类型的 `async fn`；
2. 在 `src/web.rs` 加一条路由（handler 只做参数解析 + 序列化）；
3. 在 `src/control.rs` 的 `ControlRequest` 加一个变体并转发。

## 模块

| 模块 | 职责 |
|---|---|
| `api/` | 领域服务：监控源、变更事件、分组、设置、备份。业务唯一实现 |
| `web/` | axum 路由、Bearer 鉴权、静态资源托管；统一 `{ok,result,error}` 信封 |
| `control/` | Unix socket 行协议（每行一个 JSON），只做传输与请求分发 |
| `scheduler/` | 调度主循环、单次检测流程、持有 `AppState` |
| `config/` | 配置模型（启动引导 + SQLite 可编辑设置）与解析 |
| `db/` | SQLite 访问层（含 schema 迁移） |
| `fetcher/` | 抓取引擎：`http` 与 `camofox`（可选） |
| `pipeline/` | 内容提取：整页文本 / 结构化条目（CSS / JSONPath） |
| `differ/` | 新旧条目比对，识别新增 / 更新 / 移除 |
| `notifier/` | Telegram 通知发送与发件箱排空 |
| `cron_expr/` | 标准 5 段 cron 解析与下次触发时刻计算 |
| `net_guard/` | 出站请求 SSRF 防护（私网 / 环回地址拦截） |
| `backup/` | 备份打包与在线恢复 |
| `images/` | 通知图片下载与缓存 |

## 数据流：一次检测

```text
调度主循环（每 500ms）
  → 捞到期源（跳过已禁用 / 退避中），按 queue_capacity 截断
  → 并发任务（信号量按 concurrency 限流）
      → fetcher 抓取（http / camofox）
      → pipeline 提取（整页文本 or 结构化条目）
      → differ 比对上一轮快照
      → 有变化：落库 change_event + 排队通知
      → 推进调度状态（cron 计算下次触发）
  → 排空通知发件箱
  → 每 60s 清理历史
```

## 状态与并发

- `AppState` 通过 `Arc` 共享。`db` 是 `tokio::sync::Mutex<Db>`（SQLite 连接非 `Send`），
  `sources` 是内存视图（SQLite 为持久层）。
- **锁顺序统一为 `db` → `sources`**，避免死锁。
- 由于 `Db` 非 `Sync`，不要把 `&Db` 带到 `.await` 之后：需要读库时先同步取完数据再 `drop`。
- 热更新：`settings` / `runtime` 用 `RwLock` 保护，保存设置后即时刷新；
  并发数由调度主循环每轮调整信号量许可数。

## 前端

`web/` 是 Vite + React 控制台，按功能划分 `src/features/{sources,settings}/`：
页面组件负责编排与状态，子组件负责渲染，`lib/` 放表单转换与格式化等纯逻辑。
详见 [web/README.md](../web/README.md)。
