# ReadingSteiner

持续监控 HTTP/HTTPS 网页与数据接口，检测文本、结构化数据、图片 URL 的变化，并将变化（含图片本体）推送到 Telegram。

- **Rust** 后端 + **React / shadcn.ui** Web 控制台，CLI + Web（无 TUI）。
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

- 页面：**仪表盘**（运行状态 / 引擎健康）、**监控源**（添加、编辑、删除、测试、立即检测、试跑流水线）、**变更事件**（列表与 diff 详情）。
- **监控源（source）只存在 SQLite（`state/reading-steiner.db` 的 `sources` 表）中**，是运行时唯一数据源。config.yaml 中**不包含** sources 配置——添加、编辑、删除监控源统一通过 Web 控制台或 CLI 操作，即时生效，无需重启 daemon。
- 技术栈：React + TypeScript + Vite + Tailwind CSS + shadcn/ui。
- 前端源码位于 [`web/`](./web/)，构建产物输出到 `web/dist`。
- 开发调试：在 `web/` 下执行 `npm run dev`，Vite 会把 `/api` 代理到 `127.0.0.1:8901`。
- 仅暴露在 `127.0.0.1` 时请勿将 `web.listen` 绑定到公网地址，或加一层鉴权反代。

## CLI

```text
reading-steiner serve                 # 前台跑 daemon（systemd 用），并启动 Web API + 静态资源
reading-steiner web                   # 打印 Web 控制台地址
reading-steiner status [--json]       # 运行状态
reading-steiner sources add <file>    # 添加监控源（YAML 文件，需先启动 daemon）
reading-steiner sources list          # 列出监控源（需先启动 daemon）
reading-steiner check <id>            # 立即检测一次
reading-steiner test-pipeline <id>    # 用最近快照试跑筛选流水线
reading-steiner diff <event-id>       # 查看变更 diff
reading-steiner notify test           # 发测试 Telegram 消息
reading-steiner history <id>          # 变更历史
```

## 配置

见 [`config.yaml`](./config.yaml)。config.yaml 负责**全局配置**（state_dir、telegram、camofox、web）与**流水线模板**（pipelines），**不包含监控源（sources）定义**——监控源统一存于 SQLite，通过 Web 控制台或 CLI 管理。

核心结构：

```yaml
telegram:
  token_file: state/telegram_token   # 生产环境推荐文件方式
  default_chat_id: "-1001234567890"
  api_base: https://api.telegram.org # 可指向 mock，便于测试

camofox:
  enabled: false
  base_url: http://127.0.0.1:9377

pipelines:
  product_list:
    extract:
      - type: css_items
        selector: ".product"
        fields:
          id:    { attr: data-id }
          title: { selector: ".title" }
          price: { selector: ".price" }
    normalize:
      - type: trim
        field: title
    filter:
      include:
        - { op: gt, field: price, value: 0 }
```

### 监控源（source）

监控源通过 **Web 控制台**或 **CLI**（`reading-steiner sources add <file>`）添加，核心字段：

```yaml
id: shop-list          # 唯一标识
name: Shop List
fetch:
  engine: http         # 或 camofox
  url: https://example.com/list
schedule:
  interval_secs: 60
pipeline: product_list # 引用 config.yaml 中的 pipelines 模板
compare:
  mode: item_set       # 比较模式
  stable_id: id        # 稳定字段
  notify_on: [new, updated, removed]
```

### 比较模式（compare.mode）

`compare.mode` 决定 Differ 如何判定两轮快照之间的“变化”，支持三种：

| 模式 | 说明 | 适用场景 |
|---|---|---|
| `item_set` | 按提取后的 **Item 集合**比较。需配合 `stable_id`，逐项对比稳定字段值是否新增 / 更新 / 移除。 | **推荐**。结构化页面（商品列表、文章列表、JSON API 等），变更语义清晰、误报最少。 |
| `raw_digest` | 对原始抓取内容的 SHA-256 摘要做全量比较，内容有任何字节变化即视为“已变更”。 | 无需区分具体字段、只要知道“变没变”的整页监控。 |
| `text_sim` | 基于提取后全文做**相似度**比较，相似度低于阈值判定为变更。 | 文本型页面，内容存在小幅噪声（如时间戳、随机广告）时用于容忍轻微差异。 |

> 注意：`item_set` 是默认模式。结构化数据请优先用 `css_items` / `json_path` 提取字段并配合 `stable_id`，Differ 只会报告 Item 级差异，而不是 diff 原始 HTML。

### 稳定字段（compare.stable_id）

`stable_id` 指定 Item 中用于**跨轮次稳定识别同一条数据**的字段名（来自 pipeline 提取结果）。Differ 以 `stable_id` 的值作为 Item 的“主键”，在旧快照和新快照之间做集合配对：

- 新快照中出现旧快照没有的 `stable_id` 值 → 判定为 **新增（new）**。
- 旧快照中消失 → 判定为 **移除（removed）**。
- 相同 `stable_id` 但其余字段的指纹（排除 `ignore_fields`）不同 → 判定为 **更新（updated）**。

```yaml
compare:
  mode: item_set
  stable_id: id       # 每条商品/文章的稳定唯一 ID，对应 pipeline 中提取的字段
  ignore_fields: []   # 可选：比较时忽略的字段（如价格波动、点击量）
```

> 选错 `stable_id` 会直接导致误报：例如商品列表用 `title` 当稳定字段，一旦标题微调就会同时产生“移除+新增”两条事件。请选择一个内容不变但能唯一定位的字段。

### 试跑流水线（test-pipeline）

`reading-steiner test-pipeline <id>`（Web 控制台对应“试跑流水线”按钮）用于**在不抓取新内容、不产生变更事件的前提下**，用该监控源**最近一次快照**中的已提取 Item 重新跑一遍 pipeline 的 `filter` / `normalize` 阶段，返回最终落库前的 item 列表与指纹。

用途：
- 快速验证筛选规则（`filter`）写对了没有——比如 `price > 0` 是否真的过滤掉了不想要的条目。
- 验证 normalize 规则（`trim` / `strip` / `abs_url`）对历史数据的效果。
- 查看最终写入快照的指纹，用于排查“为什么这个 source 没有触发通知”。

> 注意：`test-pipeline` 用的是**已有快照**，不会重新抓取网页。若想刷新原始内容后试跑，先执行 `reading-steiner check <id>`（或 Web 控制台的“立即检测”）再试跑。

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
