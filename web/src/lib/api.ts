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
  default_chat_id: string
  max_images_per_event: number
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...init,
  })
  const body = (await res.json()) as ApiEnvelope<T>
  if (!res.ok || !body.ok) {
    throw new Error(body.error ?? `HTTP ${res.status}`)
  }
  return body.result as T
}

export const api = {
  listSources: () => request<SourceConfig[]>("/api/sources"),

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

  listEvents: (limit = 50) =>
    request<ChangeEvent[]>(`/api/events?limit=${limit}`),

  getEvent: (id: number) => request<ChangeEvent>(`/api/events/${id}`),

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
    request<{ saved: boolean; restart_required: boolean; config: string }>(
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

  downloadBackup: (name: string) => `/api/backups/${encodeURIComponent(name)}/download`,

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
    const res = await fetch("/api/restore/upload", { method: "POST", body: form })
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
