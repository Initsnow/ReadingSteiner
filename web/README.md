# ReadingSteiner Web 控制台

React + TypeScript + Vite + Tailwind CSS + [shadcn/ui](https://ui.shadcn.com/) 实现的 Web 管理控制台，用于替代原 TUI。

## 页面

- **监控源**：列出 source，支持「立即检测」「测试」、按标签筛选、展开变更历史（自动标记已读）。
- **设置**：全局设置、分组（标签）管理与备份恢复。

## 开发

```bash
pnpm install
pnpm dev
```

Vite 会把 `/api` 代理到 `http://127.0.0.1:8901`（需先启动 daemon）。

## 构建

```bash
pnpm build
# 产物输出到 dist/，由 daemon 的 web 服务托管
```

## 技术栈

- [Vite](https://vitejs.dev/) + React 18 + TypeScript
- [Tailwind CSS](https://tailwindcss.com/) v3
- [shadcn/ui](https://ui.shadcn.com/) 组件（Radix UI 原语 + CVA）

## API 约定

所有请求返回 `{ ok, result, error }`：

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/status` | 运行状态 |
| GET | `/api/sources` | 监控源列表 |
| GET | `/api/events?limit=N` | 变更事件列表 |
| GET | `/api/events/:id` | 单个事件详情 |
| POST | `/api/check` | 立即检测 `{ source_id }` |
| GET | `/api/history?source_id=&limit=` | 变更历史 |
| POST | `/api/notify-test` | 发送测试通知 `{ chat_id? }` |
