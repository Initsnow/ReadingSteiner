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
cd web && pnpm install && pnpm build && cd ..

# 启动 daemon（默认读取 config.yaml）
./target/release/reading-steiner serve

# 打开 Web 控制台
# 浏览器访问 http://127.0.0.1:8901 （可在 config.yaml 的 web.listen 修改）

# 命令行查看状态
./target/release/reading-steiner status
```

## 文档

| 文档 | 说明 |
|---|---|
| [配置](./docs/configuration.md) | config.yaml 引导项 + SQLite 中的全局设置 |
| [Web 控制台](./docs/web-console.md) | 监控源 / 设置 / 分组 / 备份 |
| [CLI](./docs/cli.md) | 命令行用法 |
| [监控源配置](./docs/sources.md) | fetch / extract / schedule 写法 |
| [图片通知](./docs/sources.md#图片通知可选) | 变更通知附带图片 |
| [备份与恢复](./docs/backup.md) | 备份 / 恢复 / zip 迁移 |
| [camofox 接入](./docs/camofox.md) | 反检测浏览器引擎 |
| [部署](./docs/deployment.md) | NixOS flake 部署 |
| [测试](./docs/testing.md) | 单元 / 集成 / 压测 |

## 目录结构

```text
src/           Rust 后端（config / db / fetcher / pipeline / scheduler / web / ...）
web/           React Web 控制台（Vite + shadcn/ui）
tests/         集成测试
nixos/         NixOS 模块与集成测试
config.yaml    启动引导配置（可编辑的全局设置已存 SQLite）
```
