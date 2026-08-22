# 监控源配置

监控源（source）通过 **Web 控制台**或 **CLI**（`reading-steiner sources add <file>`）添加，存于 SQLite。每个源只需配置三件事：**抓什么**（fetch）、**提取什么**（extract）、**何时检测**（schedule）。变更检测完全自动。

核心字段（对应 `src/config.rs::SourceConfig`）：

| 字段 | 说明 |
|---|---|
| `id` | 唯一 ID（可留空，系统自动生成） |
| `name` | 名称 |
| `enabled` | 是否启用监控（调度检查） |
| `notify_enabled` | 是否发送变更通知 |
| `follow_group` | 是否跟随所属分组设置（默认 true） |
| `tags` | 标签 / 分组列表 |
| `fetch` | 抓取配置（engine / url / headers 等） |
| `schedule` | cron 调度 |
| `extract` | 内容提取（整页文本 / 结构化条目） |

```yaml
id: my-blog
name: My Blog
enabled: true
notify_enabled: true
follow_group: true
tags: [news]
fetch:
  engine: http
  url: https://example.com
schedule:
  cron: "*/15 * * * *"
extract:
  type: text
```

## 抓取（fetch）

```yaml
fetch:
  engine: http        # http（默认）或 camofox
  url: https://example.com
  method: GET
  headers:
    User-Agent: "Mozilla/5.0"
  max_body_bytes: 5242880
  timeout_secs: 30
  wait:               # camofox 引擎的等待配置（可选）
    selector: ".content"
    timeout: 10
  tab_policy: reuse
  evaluate: "..."     # camofox 引擎执行的自定义 JS（可选）
  screenshot: false
```

- HTTP 抓取会根据响应头 `Content-Type` 声明的 charset 解码（支持 GBK 等常见中文编码），避免非 UTF-8 页面乱码导致误判。
- `engine: camofox` 需要接入 camofox 浏览器引擎，见 [camofox 接入](./camofox.md)。

## 调度（schedule）

每个监控源使用 **cron 表达式**（标准 5 段：`分 时 日 月 周`）精确调度，在指定时刻触发。

```yaml
schedule:
  # 工作日每天 9:00 与 18:00 各检查一次
  cron: "0 9,18 * * 1-5"
```

支持的语法：`*`（任意）、`*/n`（步进）、`a,b,c`（列表）、`a-b`（范围）、`a-b/n`（范围步进）。周字段 `0-6` 对应周日到周六（0 或 7 均为周日）。时区跟随全局 `timezone`（缺省为系统本地时区）。

> cron 留空时使用全局默认 cron；全局默认 cron 也留空则回退到每小时（`0 * * * *`）。

## 内容提取（extract）

### 整页文本（text）

直接把页面（或接口返回的文本）作为比对内容，有任何变化即视为变更。适合文章、纯文本页面、整页指纹监控。

```yaml
extract:
  type: text
```

### 结构化条目（items）

从页面 / JSON 中按规则提取出若干「条目」，自动对比条目的新增 / 更新 / 移除，无需配置稳定字段。

```yaml
extract:
  type: items
  selector:
    kind: css         # 或 json_path
    selector: ".product"
  fields:
    - name: title
      selector: ".title"
  dedupe_key: "{{title}}"
```

`selector` 按内容类型自动区分：

- HTML 用 `kind: css`（CSS 选择器）或 XPath。
- JSON 用 `kind: json_path`，支持 `$.items[*].id`、`$.items[0].name` 等链式导航（`[*]` 通配、`[n]` 索引可出现在任意层级）。

## 图片通知（可选）

检测到内容变化并推送 Telegram 时，可让通知附带页面图片。在 source 的 `extract` 里配置 `images` 图片选择器：

```yaml
# 整页文本监控 + 用 CSS 选择器挑选图片
extract:
  type: text
  images:
    kind: css
    selector: ".cover img"   # 匹配 <img> 或其容器元素
```

```yaml
# 结构化条目监控 + 发送条目里提取到的图片
extract:
  type: items
  selector:
    kind: css
    selector: ".product"
  images:
    kind: changed   # 只发有变化的条目相关图片
```

`images` 取值：

- `kind: none`（默认）——不附带图片。
- `kind: items`——发送结构化条目提取时自动带出的图片（仅适用于 `type: items`）。
- `kind: changed`——**只发送发生变更的条目**相关图片：取其元素自身子树（子节点）的 `<img>`，以及父容器中紧邻的直接 `<img>` 兄弟（如缩略图）。不会把整页全部图片都发出去（仅适用于 `type: items`）。
- `kind: css` + `selector`——用 CSS 选择器从页面挑选图片元素（取其 `src`/`data-src`）。

同一事件最多发送的图片数由全局设置 `max_images_per_event` 控制（默认 10）。图片在本地 `media_dir` 缓存，重复图片自动去重。
