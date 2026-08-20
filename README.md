# ReadingSteiner

持续监控 HTTP/HTTPS 网页与数据接口，检测内容变化，并将变化推送到 Telegram。

> **设计理念：只有两件事——抓取内容，然后检测变化。**
> 不需要「流水线 / 比较模式 / 稳定字段」这些复杂概念。每个监控源只需配置：**抓什么**、**提取什么**、**多久检测一次**。

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
```

## 配置

见 [`config.yaml`](./config.yaml)。config.yaml 只负责**全局配置**（state_dir、telegram、camofox、web），**不包含监控源（sources）定义**——监控源统一存于 SQLite，通过 Web 控制台或 CLI 管理。

监控源通过 **Web 控制台**或 **CLI**（`reading-steiner sources add <file>`）添加，格式见 `src/config.rs::SourceConfig`。每个源只需配置：抓什么（fetch）、提取什么（extract，整页文本或结构化条目）、多久检测一次（schedule）。变更检测完全自动。

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
