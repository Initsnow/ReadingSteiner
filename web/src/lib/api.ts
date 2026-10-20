// ReadingSteiner Web 控制台 API 客户端。
// 对应 daemon 内置的 axum HTTP/JSON 接口（/api/*）。
// 每个接口返回 `{ ok, result, error }`。

export interface ApiEnvelope<T = unknown> {
  ok: boolean
  result: T | null
  error: string | null
}

export interface BrowserConfig {
  enabled: boolean
  cdp_url: string
  timeout_secs: number
}

export interface SourceConfig {
  id: string
  name: string
  /** 所属分组 ID（可选）。继承分组的预设规则与通知目标。 */
  group_id?: string
  /** 是否启用监控（调度检查）。false 时该源不会被调度器抓取检测。 */
  enabled: boolean
  /** 是否发送变更通知。false 时仍正常监控检测，但变更不推送 Telegram 通知。 */
  notify_enabled: boolean
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
  /** 内容提取：决定「把抓到的内容变成什么拿来比对」 */
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
  | { kind: "first_item" }
  | { kind: "css"; selector: string }
  | { kind: "screenshot" }

// 内容提取方式：3 种模式
export type ExtractConfig =
  | { type: "text"; selector?: string; images?: ImageSelector }
  | { type: "json"; json_path?: string; images?: ImageSelector }
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
  source_id?: string
  status?: number
  final_url?: string
  duration_ms?: number
  engine?: string
  content_sha256?: string
  fingerprint?: string
  text_len?: number
  items?: Array<{ stable_id: string; fields: Record<string, string>; image_urls: string[]; text: string }>
  image_urls?: string[]
  screenshot_base64?: string
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
  /** 截图文件路径（相对 media_dir）。 */
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

export interface GroupConfig {
  /** 分组唯一标识 ID（只读/新建时指定，如 news、github-releases）。 */
  id: string
  /** 分组展示名称。 */
  name: string
  /** 分组默认 cron 表达式。 */
  default_cron?: string
  /** 分组默认请求头。 */
  default_headers?: Record<string, string>
  /** 分组默认浏览器引擎参数。 */
  default_browser_config?: BrowserConfig
  /** 分组默认提取模式。 */
  default_extract?: ExtractConfig
  /** 分组的 Telegram 通知目标（tgram://bottoken/ChatID1/ChatID2）；留空沿用全局。 */
  default_notify_url?: string
  created_at?: string
  updated_at?: string
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
  /** 外部密钥文件路径（如通过 sops-nix 挂载） */
  telegram_url_file?: string
  max_images_per_event: number
  /** 是否启用 Chrome CDP 渲染引擎 */
  browser_enabled: boolean
  /** Chrome CDP 远程调试地址（如 http://127.0.0.1:9222 或 ws://127.0.0.1:9222/...） */
  browser_cdp_url: string
  browser_timeout_secs: number
  /** 配置文件是否只读（如部署在 NixOS /nix/store 或只读文件系统） */
  readonly: boolean
  config_path?: string
}

// ---- Web 控制台鉴权（Bearer Token）----
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

export async function checkAuthRequired(): Promise<boolean> {
  const res = await fetch("/api/status")
  return res.status === 401
}

export async function verifyToken(token: string): Promise<boolean> {
  try {
    const res = await fetch("/api/status", {
      headers: { Authorization: `Bearer ${token.trim()}` },
    })
    return res.status !== 401
  } catch {
    throw new Error("cannot reach daemon")
  }
}

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

  /** 对已保存的监控源触发实时抓取测试。 */
  testSource: (id: string) =>
    request<TestSourceResult>(`/api/sources/${encodeURIComponent(id)}/test`, {
      method: "POST",
    }),

  /** 对未保存的监控源草稿进行 Live Dry-run 测试（Live Inspector）。 */
  testSourceDryRun: (source: SourceConfig) =>
    request<TestSourceResult>("/api/sources/test", {
      method: "POST",
      body: JSON.stringify(source),
    }),

  /** 预览：抓取 URL 并返回页面标题，用于添加监控源时自动填充名称。 */
  previewSource: (url: string, engine = "http") =>
    request<{ url: string; title: string }>("/api/sources/preview", {
      method: "POST",
      body: JSON.stringify({ url, engine }),
    }),

  markEventRead: (id: number) =>
    request<{ updated: number }>(`/api/events/${id}/read`, { method: "POST" }),

  markAllEventsRead: () =>
    request<{ updated: number }>("/api/events/read-all", { method: "POST" }),

  markSourceRead: (sourceId: string) =>
    request<{ updated: number }>(`/api/sources/${encodeURIComponent(sourceId)}/read`, {
      method: "POST",
    }),

  // ---- 分组预设 (Groups) ----
  listGroups: () => request<GroupConfig[]>("/api/groups"),

  getGroup: (id: string) => request<GroupConfig>(`/api/groups/${encodeURIComponent(id)}`),

  upsertGroup: (group: GroupConfig) =>
    request<{ source_id: string }>("/api/groups", {
      method: "POST",
      body: JSON.stringify(group),
    }),

  deleteGroup: (id: string) =>
    request<{ source_id: string }>(`/api/groups/${encodeURIComponent(id)}`, {
      method: "DELETE",
    }),

  // 立即触发检查
  check: (sourceId: string) =>
    request<{ source_id: string }>(`/api/sources/${encodeURIComponent(sourceId)}/check`, {
      method: "POST",
    }),

  history: (sourceId?: string, limit = 50) => {
    const qs = new URLSearchParams()
    if (sourceId) qs.set("source_id", sourceId)
    qs.set("limit", String(limit))
    return request<ChangeEvent[]>(`/api/history?${qs.toString()}`)
  },

  listEvents: (limit = 50) => {
    const qs = new URLSearchParams()
    qs.set("limit", String(limit))
    return request<ChangeEvent[]>(`/api/events?${qs.toString()}`)
  },

  getEvent: (id: number) => request<ChangeEvent>(`/api/events/${id}`),

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
