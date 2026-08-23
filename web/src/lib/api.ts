// ReadingSteiner Web 控制台 API 客户端。
// 对应 daemon 内置的 axum HTTP/JSON 接口（/api/*）。
// 每个接口返回 `{ ok, result, error }`。

export interface ApiEnvelope<T = unknown> {
  ok: boolean
  result: T | null
  error: string | null
}

export interface SourceConfig {
  id: string
  name: string
  /** 是否启用监控（调度检查）。false 时该源不会被调度器抓取检测。 */
  enabled: boolean
  /** 是否发送变更通知。false 时仍正常监控检测，但变更不推送 Telegram 通知。 */
  notify_enabled: boolean
  /** 是否跟随所属分组（标签）的设置；false 时使用自身开关（自覆盖）。 */
  follow_group?: boolean
  tags: string[]
  fetch: {
    engine: string
    url: string
    method: string
    headers?: Record<string, string>
    max_body_bytes?: number
    timeout_secs?: number
    wait?: { selector?: string; timeout?: number }
    tab_policy?: string
    evaluate?: string
    screenshot?: boolean
  }
  schedule: {
    /** cron 表达式（标准 5 段：分 时 日 月 周）。 */
    cron?: string
  }
  // 内容提取：决定「把抓到的内容变成什么拿来比对」
  extract: ExtractConfig
}

/** 监控源列表项：在 SourceConfig 基础上附加展示用元信息。 */
export interface SourceMeta extends SourceConfig {
  /** 最近一次检查时间。 */
  last_check_at: string | null
  /** 最近一次检测到变更的时间。 */
  last_change_at: string | null
  /** 未读变更事件数。 */
  unread_count: number
  /** 是否处于错误状态（连续失败次数 > 0）。 */
  has_error: boolean
  /** 最近一次错误信息（失败时记录，成功后清空）。 */
  last_error?: string | null
}

// 图片选择器：如何挑选要随变更通知附带的图片。
export type ImageSelector =
  | { kind: "none" }
  | { kind: "items" }
  | { kind: "css"; selector: string }
  | { kind: "changed" }

// 内容提取方式
export type ExtractConfig =
  | { type: "text"; images?: ImageSelector }
  | {
      type: "items"
      selector: ItemSelector
      fields?: ItemField[]
      dedupe_key?: string
      images?: ImageSelector
    }

export type ItemSelector =
  | { kind: "css"; selector: string }
  | { kind: "json_path"; path: string }

export interface ItemField {
  name: string
  selector?: string
  attr?: string
  path?: string
}

export interface TestSourceResult {
  source_id: string
  status?: number
  final_url?: string
  duration_ms?: number
  engine?: string
  content_sha256?: string
  fingerprint?: string
  text_len?: number
  items?: Array<{ stable_id: string; fields: Record<string, string>; image_urls: string[]; text: string }>
  not_modified?: boolean
}

export interface ChangeEvent {
  id: number
  watchpoint_id: string
  change_type: string
  old_items_json: string
  new_items_json: string
  diff_summary: string
  fingerprint: string
  dedupe_key: string
  detected_at: string
  /** 是否已读。 */
  read: boolean
  /** camofox 截图路径（相对 media_dir）。 */
  screenshot_path: string | null
}

export interface DaemonStatus {
  running: boolean
  version: string
  sources: number
  enabled_sources: number
  queue_depth: number
  last_tick_at: string | null
  engine_health: Record<string, boolean>
  timezone: string
  server_time_utc: string
  server_time_local: string
}

export interface TagConfig {
  /** 分组（标签）名称。 */
  name: string
  /** 该分组下每个监控源最多保留的变更历史条数（0 表示不限制，跟随全局）。 */
  history_limit: number
  /** 分组默认的内容提取配置；未配置则不覆盖，监控源沿用自身/全局设置。 */
  extract?: ExtractConfig | null
  /** 分组的 Telegram 通知目标（tgram://bottoken/ChatID1/ChatID2）；留空沿用全局。 */
  notify_url: string
}

export interface EditableSettings {
  concurrency: number
  queue_capacity: number
  default_timeout_secs: number
  default_cron: string
  default_user_agent: string
  history_limit_per_source: number
  failure_notify_threshold: number
  timezone: string
  template: string
  /** 全局 Telegram 通知目标：tgram://bottoken/ChatID1/ChatID2 */
  telegram_url: string
  max_images_per_event: number
}

// ---- Web 控制台鉴权（Bearer Token）----
// token 存在 localStorage；`web.auth_token` 未配置时后端不校验，携带与否均可用。
// 401 时清除 token 并通知 UI 展示解锁界面。

const TOKEN_KEY = "reading-steiner.auth-token"

export function getAuthToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

export function setAuthToken(token: string): void {
  if (token.trim()) {
    localStorage.setItem(TOKEN_KEY, token.trim())
  } else {
    localStorage.removeItem(TOKEN_KEY)
  }
}

export function clearAuthToken(): void {
  localStorage.removeItem(TOKEN_KEY)
}

/** 401 订阅：后端启用鉴权且未授权时触发，由 UI 展示解锁界面。 */
type AuthSubscriber = () => void
const authSubscribers = new Set<AuthSubscriber>()

export function onUnauthorized(cb: AuthSubscriber): () => void {
  authSubscribers.add(cb)
  return () => authSubscribers.delete(cb)
}

function notifyUnauthorized(): void {
  clearAuthToken()
  authSubscribers.forEach((cb) => cb())
}

export class AuthError extends Error {
  constructor(message: string) {
    super(message)
    this.name = "AuthError"
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  headers.set("Content-Type", "application/json")
  const token = getAuthToken()
  if (token) headers.set("Authorization", `Bearer ${token}`)

  const res = await fetch(path, { ...init, headers })
  if (res.status === 401) {
    notifyUnauthorized()
    throw new AuthError("unauthorized: 需要鉴权 Token")
  }
  const body = (await res.json()) as ApiEnvelope<T>
  if (!res.ok || !body.ok) {
    throw new Error(body.error ?? `HTTP ${res.status}`)
  }
  return body.result as T
}

/** 探测后端是否需要鉴权。
 * 不带 token 请求 /api/status：返回 401 说明需要鉴权；其他状态码说明无需鉴权。
 * 网络异常时抛出错误，由调用方处理。 */
export async function checkAuthRequired(): Promise<boolean> {
  const res = await fetch("/api/status")
  return res.status === 401
}

/** 校验 token 是否有效：调用一个轻量只读接口（/api/status）。
 * 仅当后端明确返回 401 视为无效；其余错误（5xx / 网络异常等）由调用方区分，
 * 避免把服务端故障误判为“token 错误”而阻塞用户。 */
export async function verifyToken(token: string): Promise<boolean> {
  try {
    const res = await fetch("/api/status", {
      headers: { Authorization: `Bearer ${token.trim()}` },
    })
    return res.status !== 401
  } catch {
    // 网络不通（daemon 未启动等）：让调用方按“无法连接”处理。
    throw new Error("cannot reach daemon")
  }
}

/**
 * 带鉴权头拉取二进制资源并返回 objectURL。
 * 截图、备份下载等接口受 Bearer Token 保护，浏览器原生 `<img>`/`<a download>`
 * 无法附加 Authorization 头，必须经此 fetch 获取 blob。
 */
export async function fetchBlobUrl(url: string): Promise<string> {
  const token = getAuthToken()
  const headers = new Headers()
  if (token) headers.set("Authorization", `Bearer ${token}`)
  const res = await fetch(url, { headers })
  if (res.status === 401) {
    notifyUnauthorized()
    throw new AuthError("unauthorized: 需要鉴权 Token")
  }
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}`)
  }
  const blob = await res.blob()
  return URL.createObjectURL(blob)
}

export const api = {
  listSources: () => request<SourceMeta[]>("/api/sources"),

  addSource: (source: SourceConfig) =>
    request<{ source_id: string; added: boolean }>("/api/sources", {
      method: "POST",
      body: JSON.stringify(source),
    }),

  updateSource: (id: string, source: SourceConfig) =>
    request<{ source_id: string; updated: boolean }>(`/api/sources/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(source),
    }),

  deleteSource: (id: string) =>
    request<{ source_id: string; deleted: boolean }>(`/api/sources/${encodeURIComponent(id)}`, {
      method: "DELETE",
    }),

  /** 批量更新多个监控源的监控开关 / 通知开关。 */
  batchSetFlags: (sourceIds: string[], flags: { enabled?: boolean; notify_enabled?: boolean }) =>
    request<{ updated: number }>("/api/sources/batch", {
      method: "POST",
      body: JSON.stringify({ source_ids: sourceIds, ...flags }),
    }),

  testSource: (id: string) =>
    request<TestSourceResult>(`/api/sources/${encodeURIComponent(id)}/test`, {
      method: "POST",
    }),

  /** 预览：抓取 URL 并返回页面标题，用于添加监控源时自动填充名称。 */
  previewSource: (url: string, engine = "http") =>
    request<{ url: string; title: string }>("/api/sources/preview", {
      method: "POST",
      body: JSON.stringify({ url, engine }),
    }),

  markEventRead: (id: number) =>
    request<{ updated: number }>(`/api/events/${id}/read`, { method: "POST" }),

  markSourceRead: (sourceId: string) =>
    request<{ updated: number }>(`/api/sources/${encodeURIComponent(sourceId)}/read`, {
      method: "POST",
    }),

  listTags: () => request<TagConfig[]>("/api/tags"),

  updateTag: (name: string, tag: TagConfig) =>
    request<{ name: string; updated: boolean }>(`/api/tags/${encodeURIComponent(name)}`, {
      method: "PUT",
      body: JSON.stringify(tag),
    }),

  deleteTag: (name: string) =>
    request<{ name: string; deleted: boolean }>(`/api/tags/${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),

  check: (sourceId: string) =>
    request<{ source_id: string; checked: boolean }>("/api/check", {
      method: "POST",
      body: JSON.stringify({ source_id: sourceId }),
    }),

  history: (sourceId?: string, limit = 50) => {
    const qs = new URLSearchParams()
    if (sourceId) qs.set("source_id", sourceId)
    qs.set("limit", String(limit))
    return request<ChangeEvent[]>(`/api/history?${qs.toString()}`)
  },

  notifyTest: (chatId?: string) =>
    request<{ message_id?: number }>("/api/notify-test", {
      method: "POST",
      body: JSON.stringify({ chat_id: chatId ?? null }),
    }),

  status: () => request<DaemonStatus>("/api/status"),

  getSettings: () => request<EditableSettings>("/api/settings"),

  updateSettings: (settings: EditableSettings) =>
    request<{
      saved: boolean
      applied: boolean
      immediate: boolean
      restart_required: boolean
      config: string
    }>(
      "/api/settings",
      {
        method: "PUT",
        body: JSON.stringify(settings),
      },
    ),

  createBackup: () =>
    request<{ path: string; name: string; has_zip: boolean }>("/api/backup", { method: "POST" }),

  listBackups: () =>
    request<{ backups: { name: string; has_zip: boolean }[] }>("/api/backups"),

  restoreBackup: (name: string) =>
    request<unknown>("/api/restore", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),

  deleteBackup: (name: string) =>
    request<{ deleted: boolean; name: string }>(
      `/api/backups/${encodeURIComponent(name)}`,
      { method: "DELETE" },
    ),

  // 上传 zip 备份并恢复。multipart 字段名为 file，不能走默认 JSON 头。
  restoreFromZip: async (file: File) => {
    const form = new FormData()
    form.append("file", file)
    const headers = new Headers()
    const token = getAuthToken()
    if (token) headers.set("Authorization", `Bearer ${token}`)
    const res = await fetch("/api/restore/upload", {
      method: "POST",
      headers,
      body: form,
    })
    if (res.status === 401) {
      notifyUnauthorized()
      throw new AuthError("unauthorized: 需要鉴权 Token")
    }
    const body = (await res.json()) as ApiEnvelope<{
      restored: boolean
      name: string
      error?: string
    }>
    if (!res.ok || !body.ok) {
      throw new Error(body.error ?? `HTTP ${res.status}`)
    }
    return body.result
  },
}
