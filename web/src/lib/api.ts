// ReadingSteiner Web 控制台 API 客户端。
// 对应 daemon 内置的 axum HTTP/JSON 接口（/api/*）。
// 每个接口返回 `{ ok, result, error }`。

export interface ApiEnvelope<T = unknown> {
  ok: boolean
  result: T | null
  error: string | null
}

export interface DaemonStatus {
  running: boolean
  version: string
  sources: number
  enabled_sources: number
  queue_depth: number
  last_tick_at: string | null
  engine_health: Record<string, boolean>
}

export interface SourceConfig {
  id: string
  name: string
  enabled: boolean
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
    interval_secs: number
    jitter_secs?: number
  }
  priority: number
  pipeline: string
  compare: {
    mode: string
    stable_id?: string
    ignore_fields?: string[]
    notify_on?: string[]
    confirm_count?: number
    cooldown_secs?: number
  }
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
  pipeline?: string
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
  status: () => request<DaemonStatus>("/api/status"),

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

  testSource: (id: string) =>
    request<TestSourceResult>(`/api/sources/${encodeURIComponent(id)}/test`, {
      method: "POST",
    }),

  listEvents: (limit = 50) =>
    request<ChangeEvent[]>(`/api/events?limit=${limit}`),

  getEvent: (id: number) => request<ChangeEvent>(`/api/events/${id}`),

  check: (sourceId: string) =>
    request<{ source_id: string; checked: boolean }>("/api/check", {
      method: "POST",
      body: JSON.stringify({ source_id: sourceId }),
    }),

  testPipeline: (sourceId: string) =>
    request<{ source_id: string }>("/api/test-pipeline", {
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
}
