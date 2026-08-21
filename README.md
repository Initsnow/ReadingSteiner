# ReadingSteiner

持续监控 HTTP/HTTPS 网页与数据接口，检测内容变化，并将变化推送到 Telegram。

- **Rust** 后端 + **React / shadcn.ui** Web 控制台，CLI + Web。
- 可选 camofox 反检测浏览器引擎（通过 HTTP API 调用，不内置、不捆绑）。
- 存储：SQLite（WAL）+ 本地 media 目录。
- 部署：NixOS flake 直接引用。

## 快速开始

```bash
# 构建 Rust 后端
cargo build --release

# 构建 Web 控制台前端（可选；未构建时 daemon 仍可提供 /api 接口）
cd web && npm install && npm run build && cd ..

# 准备配置
cp config.yaml config.local.yaml
# 编辑 token / chat 后启动 daemon
./target/release/reading-steiner serve --config config.local.yaml

# 在另一终端添加监控源（YAML 文件，需 daemon 已运行）
./target/release/reading-steiner sources add source.yaml
./target/release/reading-steiner sources list

# 打开 Web 控制台
# 浏览器访问 http://127.0.0.1:8901 （默认地址，可在 config.yaml 的 web.listen 修改）

# 命令行查看状态
./target/release/reading-steiner status --config config.local.yaml
```

## Web 控制台

daemon 内置一个轻量 HTTP 服务（axum），监听地址与静态目录由 `config.yaml` 的 `web` 段配置：

```yaml
web:
  listen: 127.0.0.1:8901
  static_dir: web/dist
```

- 页面：**监控源**（添加、编辑、删除、测试、立即检测）、**变更事件**（列表与 diff 详情）。
- **监控源（source）只存在 SQLite（`state/reading-steiner.db` 的 `sources` 表）中**，是运行时唯一数据源。添加、编辑、删除监控源统一通过 Web 控制台或 CLI 操作，即时生效，无需重启 daemon。
- 技术栈：React + TypeScript + Vite + Tailwind CSS + shadcn/ui。
- 前端源码位于 [`web/`](./web/)，构建产物输出到 `web/dist`。
- 仅暴露在 `127.0.0.1` 时请勿将 `web.listen` 绑定到公网地址，或加一层鉴权反代。

## CLI

```text
reading-steiner serve                 # 前台跑 daemon（systemd 用），并启动 Web API + 静态资源
reading-steiner web                   # 打印 Web 控制台地址
reading-steiner status [--json]       # 运行状态
reading-steiner sources add <file>    # 添加监控源（YAML 文件，需先启动 daemon）
reading-steiner sources list          # 列出监控源（需先启动 daemon）
reading-steiner check <id>            # 立即检测一次
reading-steiner diff <event-id>       # 查看变更 diff
reading-steiner notify test           # 发测试 Telegram 消息
reading-steiner history <id>          # 变更历史
reading-steiner settings             # 查看全局设置
reading-steiner backup               # 备份（db + media + config）
reading-steiner backups              # 列出已有备份
reading-steiner restore <name>       # 从备份恢复（需 daemon 停止）
```

### 全局设置（Web「设置」页 / `reading-steiner settings`）

全局设置通过 config.yaml 的 `daemon` / `telegram` 段配置，可在 Web 控制台的「设置」页编辑（保存到 config 文件，部分需重启 daemon 生效）：

- `daemon.concurrency`——抓取工作线程数（并发检测数）。
- `daemon.default_timeout_secs`——全局默认请求超时秒数（单源可覆盖）。
- `daemon.default_user_agent`——默认 User-Agent（HTTP 抓取与图片下载）。
- `daemon.history_limit_per_source`——每个监控源保留的历史变更事件条数（0 不限制）。
- `daemon.failure_notify_threshold`——连续失败达到多少次后发送 Telegram 失败告警（0 禁用）。
- `daemon.timezone`——调度器/告警展示时区（IANA 名称，如 `Asia/Shanghai`）。
- `telegram.template`——变更通知模板，占位符：`{label}` `{watch}` `{time}` `{tz}` `{summary}` `{items}`。
- `telegram.max_images_per_event`、`telegram.default_chat_id` 等。

### 备份与恢复

```bash
# 备份（在线一致性快照，含 db + media + config）
reading-steiner backup --config config.local.yaml
# 列出备份
reading-steiner backups
# 恢复（先停止 daemon）
reading-steiner restore <备份名> --config config.local.yaml
```

备份保存在 `state/backups/<时间戳>/`。Web 控制台「设置」页也提供一键备份与备份列表。

## 配置

见 [`config.yaml`](./config.yaml)。config.yaml 只负责**全局配置**（state_dir、telegram、camofox、web），**不包含监控源（sources）定义**——监控源统一存于 SQLite，通过 Web 控制台或 CLI 管理。

监控源通过 **Web 控制台**或 **CLI**（`reading-steiner sources add <file>`）添加，格式见 `src/config.rs::SourceConfig`。每个源只需配置：抓什么（fetch）、提取什么（extract，整页文本或结构化条目）、多久检测一次（schedule）。变更检测完全自动。

**调度与队列参数**：
- `schedule.interval_secs` + `schedule.jitter_secs`——每次调度在基础间隔上叠加随机抖动（均匀分布在 ±jitter/2），避免大量源同一瞬间同时唤醒抢锁。
- `priority`——到期的源按优先级从高到低依次检测（数值越大越先处理）。
- `daemon.queue_capacity`——每个轮询 tick 最多入队的检测任务数（有界队列，超出部分下个 tick 再处理），防止突发积压。
- `daemon.concurrency`——并发检测的最大任务数（信号量）。

**提取能力**：
- 结构化条目（`extract.type: items`）的 JSONPath 选择器支持 `$.items[*].id`、`$.items[0].name` 等链式导航（`[*]` 通配、`[n]` 索引可出现在任意层级）。
- HTTP 抓取会根据响应头 `Content-Type` 声明的 charset 解码（支持 GBK 等常见中文编码），避免非 UTF-8 页面乱码导致误判。
- CSS 图片选择器仅对 HTML 内容生效，对 JSON/纯文本接口不会误解析。

### 图片通知（可选）

检测到内容变化并推送 Telegram 时，可让通知附带页面图片。在 source 的 `extract` 里配置 `images` 图片选择器：

```yaml
# 整页文本监控 + 用 CSS 选择器挑选图片
id: blog
name: Blog
fetch:
  engine: http
  url: https://example.com
schedule:
  interval_secs: 300
extract:
  type: text
  images:
    kind: css
    selector: ".cover img"   # 匹配 <img> 或其容器元素
```

```yaml
# 结构化条目监控 + 发送条目里提取到的图片
id: shop
name: Shop
fetch:
  engine: http
  url: https://example.com/list
schedule:
  interval_secs: 60
extract:
  type: items
  selector:
    kind: css
    selector: ".product"
  images:
    kind: items
```

`images` 取值：

- `kind: none`（默认）——不附带图片。
- `kind: items`——发送结构化条目提取时自动带出的图片（仅适用于 `type: items`）。
- `kind: css` + `selector`——用 CSS 选择器从页面挑选图片元素（取其 `src`/`data-src`）。

同一事件最多发送的图片数由 `telegram.max_images_per_event` 控制（默认 10）。图片在本地 `media_dir` 缓存，重复图片自动去重。

## camofox 接入

1. 单独部署 camofox-browser（Docker / Node / 远程实例）。
2. 将 `camofox.enabled` 设为 `true`，填写 `base_url`、`access_key_file` / `api_key_file`。
3. 在 source 的 `fetch.engine: camofox`，按需配置 `wait`、`tab_policy`、`evaluate`、`screenshot`。
4. 契约测试基于仓库内 [`camofox-openapi.json`](./camofox-openapi.json) 与 mock server，见 `tests/integration.rs::test_camofox_contract_with_mock`。

## NixOS 部署

```nix
{
  inputs.reading-steiner.url = "github:you/ReadingSteiner";

  outputs = { nixpkgs, reading-steiner, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        reading-steiner.nixosModules.default
        {
          services.reading-steiner = {
            enable = true;
            configFile = ./config.yaml;
            settings = {
              stateDir = "/var/lib/reading-steiner";
              telegram.tokenFile = config.sops.secrets.telegram.path;
              camofox = {
                enabled = false;
                base_url = "http://127.0.0.1:9377";
              };
            };
          };
        }
      ];
    };
  };
}
```

`nix flake check` 会运行 fmt/clippy/unit 检查；NixOS 集成测试见 [`nixos/tests/reading-steiner.nix`](./nixos/tests/reading-steiner.nix)。

## 压测

```bash
cargo run --release --bin loadgen -- 5000 256
```

该工具启动本地 HTTP 服务器，用真实 `HttpFetcher` 发起 5000 次请求，输出 p50/p95 与吞吐。

## 测试

```bash
cargo test
```
