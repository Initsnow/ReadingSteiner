# ReadingSteiner

持续监控 HTTP/HTTPS 网页与数据接口，检测文本、结构化数据、图片 URL 的变化，并将变化（含图片本体）推送到 Telegram。

- **Rust** 实现，CLI + TUI，无 Web。
- 可选 camofox 反检测浏览器引擎（通过 HTTP API 调用，不内置、不捆绑）。
- 存储：SQLite（WAL）+ 本地 media 目录。
- 部署：NixOS flake 直接引用。

## 快速开始

```bash
cargo build --release
# 准备配置
cp config.yaml config.local.yaml
# 编辑 token / chat / sources 后启动 daemon
./target/release/reading-steiner serve --config config.local.yaml
# 另一个终端查看状态 / TUI
./target/release/reading-steiner status --config config.local.yaml
./target/release/reading-steiner tui --config config.local.yaml
```

## CLI

```text
reading-steiner serve                 # 前台跑 daemon（systemd 用）
reading-steiner tui                   # 打开 TUI
reading-steiner status [--json]       # 运行状态
reading-steiner sources add <file>    # 添加监控项
reading-steiner sources list          # 列出配置中的监控项
reading-steiner check <id>            # 立即检测一次
reading-steiner test-pipeline <id>    # 用最近快照试跑筛选流水线
reading-steiner diff <event-id>       # 查看变更 diff
reading-steiner notify test           # 发测试 Telegram 消息
reading-steiner history <id>          # 变更历史
```

## 配置

见 [`config.yaml`](./config.yaml)。核心结构：

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

sources:
  - id: shop-list
    name: Shop List
    fetch:
      engine: http           # 或 camofox
      url: https://example.com/list
      wait:
        selector: ".item"
        timeout: 15000
      tab_policy: reuse
    schedule:
      interval_secs: 60
    pipeline: product_list
    compare:
      mode: item_set
      stable_id: id
      notify_on: [new, updated, removed]
```

> 重要：ReadingSteiner 不是“直接 diff 原始 HTML”。每个 source 通过 `pipeline` 声明提取规则（`css_items` / `xpath` / `json_path` / `regex` / `auto_text` 等），Differ 只在提取后的 `Item` 上做 `item_set` / `raw_digest` / `text_sim` 比较。`auto_text` 只是整页文本模式的便捷选择；结构化页面请配置 `css_items` 或 `json_path`。

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
