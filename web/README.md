# ReadingSteiner Web 控制台

React + TypeScript + Vite + Tailwind CSS + [shadcn/ui](https://ui.shadcn.com/) 实现的 Web 管理控制台，用于替代原 TUI。

## 页面

- **仪表盘**：运行状态、监控源数量、队列深度、引擎健康。
- **监控源**：列出 source，支持「立即检测」「试跑流水线」。
- **变更事件**：事件列表与 diff 详情（旧数据 / 新数据）。

## 开发

```bash
npm install
npm run dev
```

Vite 会把 `/api` 代理到 `http://127.0.0.1:8901`（需先启动 daemon）。

## 构建

```bash
npm run build
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
| POST | `/api/test-pipeline` | 试跑流水线 `{ source_id }` |
| GET | `/api/history?source_id=&limit=` | 变更历史 |
| POST | `/api/notify-test` | 发送测试通知 `{ chat_id? }` |
