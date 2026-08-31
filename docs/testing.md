# 测试

## 单元 / 集成测试

```bash
cargo test
```

包含：配置与提取、指纹比对、解析 tgram://、SQLite 读写（监控源 / 分组 / **全局设置** / 通知 / 快照）、备份与 zip 恢复、camofox 契约测试等。

## 压测

```bash
cargo run --release --bin loadgen -- 5000 256
```

该工具启动本地 HTTP 服务器，用真实 `HttpFetcher` 发起 5000 次请求，输出 p50/p95 与吞吐。

## 前端构建校验

```bash
cd web && pnpm install && pnpm build
```

## Nix

```bash
nix flake check   # fmt / clippy / unit 检查
```
