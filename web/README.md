# ReadingSteiner Web 控制台

React + TypeScript + Vite + Tailwind CSS 实现的 Web 管理控制台。

## 页面

- **监控源**：添加 / 编辑 / 删除、立即检测、测试提取、按标签与状态筛选、
  多选批量启停监控与通知、展开变更历史（自动标记已读）。
- **设置**：服务器时间对照、全局设置、分组（标签）管理、备份与恢复。

## 目录结构

按功能划分，页面组件负责编排与状态，子组件负责渲染，纯逻辑放 `lib/`：

```text
src/
  features/
    sources/      监控源页：页面编排 + 卡片 + 事件行 + 表单弹窗 + 测试结果
    settings/    设置页：页面编排 + 时间卡 + 全局设置 + 分组 + 备份
  components/
    ui/          基础组件：button / card / badge / tabs / dialog / field / feedback
    layout.tsx   侧边栏骨架
    auth-*.tsx   鉴权门与带鉴权的图片加载
  lib/
    api.ts       API 客户端（对应后端 /api/*）
    source-form.ts  监控源表单 ↔ 配置转换与校验
    format.ts    展示层格式化
    ui.ts        共享样式类
    utils.ts     cn() 与 cron 校验
  pages/         路由入口（重导出 feature 页面）
```

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

- [Vite](https://vitejs.dev/) + React 19 + TypeScript
- [Tailwind CSS](https://tailwindcss.com/) v3
- [Radix UI](https://www.radix-ui.com/) 原语（tabs / separator / slot）+ CVA

> 表单使用原生控件，仅在需要无障碍交互处（Tabs）引入 Radix。

## API 约定

所有请求返回 `{ ok, result, error }`；业务失败为 `400`，未找到为 `404`，
鉴权失败为 `401`。后端未配置 `web.auth_token` 时不鉴权。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/status` | 运行状态 |
| GET | `/api/sources` | 监控源列表（含展示元信息） |
| POST | `/api/sources` | 新增监控源 |
| PUT | `/api/sources/:id` | 更新监控源 |
| DELETE | `/api/sources/:id` | 删除监控源 |
| POST | `/api/sources/batch` | 批量设置监控 / 通知开关 |
| POST | `/api/sources/:id/test` | 测试提取（不落库） |
| POST | `/api/sources/preview` | 抓取标题（新增时自动填名） |
| POST | `/api/sources/:id/read` | 标记该源全部事件已读 |
| GET | `/api/history?source_id=&limit=` | 变更历史 |
| GET | `/api/events?limit=N` | 变更事件列表 |
| GET | `/api/events/:id` | 单个事件详情 |
| POST | `/api/events/:id/read` | 标记单个事件已读 |
| GET | `/api/events/:id/screenshot` | 事件截图（二进制） |
| GET/PUT/DELETE | `/api/tags` · `/api/tags/:name` | 分组设置 |
| POST | `/api/check` | 立即检测 `{ source_id }` |
| POST | `/api/notify-test` | 发送测试通知 `{ chat_id? }` |
| GET/PUT | `/api/settings` | 全局设置读写 |
| POST | `/api/backup` | 创建备份 |
| GET | `/api/backups` | 备份列表 |
| GET | `/api/backups/:name/download` | 下载备份 zip |
| DELETE | `/api/backups/:name` | 删除备份 |
| POST | `/api/restore` | 在线恢复 `{ name }` |
| POST | `/api/restore/upload` | 上传 zip 恢复（multipart `file`） |

截图与备份下载受 Bearer 鉴权保护，浏览器原生 `<img>` / `<a download>` 无法附加
`Authorization` 头，故前端统一经 `fetchBlobUrl()` 取 blob 后用 objectURL 渲染 / 下载。
