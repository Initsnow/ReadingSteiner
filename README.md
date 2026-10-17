# ReadingSteiner

持续监控 HTTP/HTTPS 网页与数据接口，检测内容变化，并将变化推送到 Telegram。

- **Rust** 高性能后端 + **React / shadcn/ui** 现代化深色 Web 控制台，CLI + Web 统一基于 HTTP RESTful API。
- 可选外部 Chrome/Chromium 浏览器引擎（通过 CDP 远程调试协议驱动，不内置二进制、按需连接）。
- 存储：SQLite（WAL 模式）+ 本地 media 目录。
- 预设分组（Groups）：站点模板化配置，统一管理 cron 调度、请求头、提取模式与通知目标。
- 部署：NixOS flake 直接引用与标准 systemd 部署。

## 快速开始

```bash
# 构建 Rust 后端
cargo build --release

# 构建 Web 控制台前端（可选；未构建时 daemon 仍可提供 /api 接口）
cd web && pnpm install && pnpm build && cd ..

# 启动 daemon（默认读取 config.yaml）
./target/release/reading-steiner serve

# 打开 Web 控制台
# 浏览器访问 http://127.0.0.1:8901 （可在 config.yaml 的 web.listen 修改）

# 命令行查看状态（CLI 作为 HTTP 客户端请求 /api/status）
./target/release/reading-steiner status
```

## 文档

| 文档 | 说明 |
|---|---|
| [架构](./docs/architecture.md) | 分层、模块职责、数据流与并发约定 |
| [配置](./docs/configuration.md) | config.yaml 作为唯一系统权威配置 + SQLite 存储监控源业务实体 |
| [Web 控制台](./docs/web-console.md) | 监控源 / 预设模板 / 设置 / 备份 |
| [CLI](./docs/cli.md) | 命令行用法 |
| [监控源配置](./docs/sources.md) | fetch / extract / schedule 写法 |
| [图片通知](./docs/sources.md#图片通知可选) | 变更通知附带图片 |
| [备份与恢复](./docs/backup.md) | 备份 / 恢复 / zip 迁移 |
| [部署](./docs/deployment.md) | NixOS flake 部署 |
| [测试](./docs/testing.md) | 单元 / 集成 / 压测 |

## 目录结构

```text
src/api/       领域服务层：业务核心实现（HTTP 控制台与 CLI 统一调用）
src/web/       HTTP 传输层（Axum 路由 + 鉴权 + 静态资源托管）
src/cli.rs     命令行客户端（基于 reqwest 请求 Axum /api/*）
src/fetcher/   抓取引擎（HTTP / 外部 CDP 浏览器）
src/           scheduler / db / pipeline / differ / notifier / ...
web/           现代化 Web 控制台（Vite + React + Tailwind + shadcn/ui）
tests/         集成测试
nixos/         NixOS 模块与集成测试
config.yaml    全局系统运行参数与基础设施配置的唯一权威来源（Single Source of Truth）
```

架构与模块职责见 [架构文档](./docs/architecture.md)。
