/** 监控源表单：配置对象 ↔ 表单状态的转换与校验。
 *
 * 与后端 `SourceConfig` 的 JSON 形状一一对应，转换集中在此，
 * 页面组件只负责渲染与事件。
 */
import type {
  ChangeEvent,
  ExtractConfig,
  ImageSelector,
  ItemField,
  ItemSelector,
  SourceConfig,
} from "@/lib/api"

export type ImageKind = "none" | "first_item" | "css" | "screenshot"

export interface FormState {
  id: string
  name: string
  enabled: boolean
  notify_enabled: boolean
  group_id: string
  url: string
  engine: string
  method: string
  cron: string
  cron_follow_global: boolean
  timeout_secs: number
  extractType: "text" | "json" | "items"
  textSelector: string
  jsonPath: string
  selectorKind: "css" | "json_path"
  selector: string
  fieldsJson: string
  dedupeKey: string
  imageKind: ImageKind
  imageSelector: string
  screenshot: boolean
  waitSelector: string
  evaluate: string
  headersJson: string
}

export function emptyForm(): FormState {
  return {
    id: "",
    name: "",
    enabled: true,
    notify_enabled: true,
    group_id: "",
    url: "",
    engine: "http",
    method: "GET",
    cron: "",
    cron_follow_global: true,
    timeout_secs: 0,
    extractType: "text",
    textSelector: "",
    jsonPath: "",
    selectorKind: "css",
    selector: "",
    fieldsJson: "[]",
    dedupeKey: "",
    imageKind: "none",
    imageSelector: "",
    screenshot: false,
    waitSelector: "",
    evaluate: "",
    headersJson: "{}",
  }
}

export function safeJsonParse<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T
  } catch {
    return null
  }
}

function imageToForm(image: ImageSelector | undefined) {
  if (!image || image.kind === "none") return { imageKind: "none" as const, imageSelector: "" }
  if (image.kind === "first_item") return { imageKind: "first_item" as const, imageSelector: "" }
  if (image.kind === "screenshot") return { imageKind: "screenshot" as const, imageSelector: "" }
  return { imageKind: "css" as const, imageSelector: image.selector }
}

function extractToForm(extract: ExtractConfig | undefined) {
  if (!extract || extract.type === "text") {
    return {
      extractType: "text" as const,
      textSelector: extract?.type === "text" ? extract.selector ?? "" : "",
      jsonPath: "",
      selectorKind: "css" as const,
      selector: "",
      fieldsJson: "[]",
      dedupeKey: "",
      ...imageToForm(extract?.type === "text" ? extract.images : undefined),
    }
  }
  if (extract.type === "json") {
    return {
      extractType: "json" as const,
      textSelector: "",
      jsonPath: extract.json_path ?? "",
      selectorKind: "json_path" as const,
      selector: "",
      fieldsJson: "[]",
      dedupeKey: "",
      ...imageToForm(extract.images),
    }
  }
  const s = extract.selector
  return {
    extractType: "items" as const,
    textSelector: "",
    jsonPath: "",
    selectorKind: s.kind,
    selector: s.kind === "css" ? s.selector : (s as { path: string }).path,
    fieldsJson: JSON.stringify(extract.fields ?? [], null, 2),
    dedupeKey: extract.dedupe_key ?? "",
    ...imageToForm(extract.images),
  }
}

export function sourceToForm(s: SourceConfig): FormState {
  const f: FormState = {
    id: s.id,
    name: s.name,
    enabled: s.enabled,
    notify_enabled: s.notify_enabled ?? true,
    group_id: s.group_id ?? "",
    url: s.fetch.url,
    engine: s.fetch.engine,
    method: s.fetch.method,
    cron: s.schedule.cron ?? "",
    cron_follow_global: !s.schedule.cron,
    timeout_secs: s.fetch.timeout_secs ?? 0,
    ...extractToForm(s.extract),
    screenshot: s.fetch.screenshot ?? false,
    waitSelector: s.fetch.wait?.selector ?? "",
    evaluate: s.fetch.evaluate ?? "",
    headersJson: JSON.stringify(s.fetch.headers ?? {}, null, 2),
  }
  if (s.fetch.screenshot && f.imageKind === "none") {
    f.imageKind = "screenshot"
  }
  return f
}

function buildImageSelector(f: FormState): ImageSelector | undefined {
  if (f.imageKind === "none") return undefined
  if (f.imageKind === "first_item") return { kind: "first_item" }
  if (f.imageKind === "screenshot") return { kind: "screenshot" }
  return { kind: "css", selector: f.imageSelector.trim() }
}

export function buildSelector(f: FormState): ItemSelector {
  return f.selectorKind === "css"
    ? { kind: "css", selector: f.selector.trim() }
    : { kind: "json_path", path: f.selector.trim() }
}

export function buildExtract(f: FormState): ExtractConfig {
  if (f.extractType === "text") {
    return {
      type: "text",
      selector: f.textSelector.trim() || undefined,
      images: buildImageSelector(f),
    }
  }
  if (f.extractType === "json") {
    return {
      type: "json",
      json_path: f.jsonPath.trim() || undefined,
      images: buildImageSelector(f),
    }
  }
  return {
    type: "items",
    selector: buildSelector(f),
    fields: safeJsonParse<ItemField[]>(f.fieldsJson) ?? [],
    dedupe_key: f.dedupeKey.trim() || undefined,
    images: buildImageSelector(f),
  }
}

export function formToSource(f: FormState, existing?: SourceConfig): SourceConfig {
  const base = existing ?? ({} as SourceConfig)
  const headers = safeJsonParse<Record<string, string>>(f.headersJson) ?? undefined
  return {
    ...base,
    id: f.id.trim(),
    name: f.name.trim(),
    enabled: f.enabled,
    notify_enabled: f.notify_enabled,
    group_id: f.group_id.trim() || undefined,
    fetch: {
      ...base.fetch,
      engine: f.engine,
      url: f.url.trim(),
      method: f.method || "GET",
      headers,
      timeout_secs: f.timeout_secs > 0 ? f.timeout_secs : undefined,
      wait: f.waitSelector.trim() ? { selector: f.waitSelector.trim() } : undefined,
      evaluate: f.evaluate.trim() || undefined,
      screenshot: f.imageKind === "screenshot" || f.screenshot,
    },
    schedule: {
      ...base.schedule,
      cron: f.cron_follow_global ? undefined : f.cron.trim() || undefined,
    },
    extract: buildExtract(f),
  }
}

/**
 * 校验表单，返回首个错误；合法时返回 null。
 */
export function validateForm(
  f: FormState,
  validateCron: (expr: string) => string | null,
  isEditing: boolean,
): string | null {
  if (!f.url.trim()) return "URL 为必填项"
  if (!isEditing && !f.id.trim() && !f.url.trim()) return "URL 为必填项"
  if (!f.cron_follow_global && !f.cron.trim()) {
    return "取消“跟随预设/全局”后，需要填写 cron 表达式"
  }
  if (!f.cron_follow_global) {
    const cronErr = validateCron(f.cron)
    if (cronErr) return cronErr
  }
  if (f.extractType === "items" && !f.selector.trim()) {
    return "条目提取需要填写选择器 (CSS / JSONPath)"
  }
  if (f.extractType === "items" && f.fieldsJson.trim() && !safeJsonParse(f.fieldsJson)) {
    return "字段配置不是合法的 JSON，请检查语法"
  }
  return null
}

/** 解析变更事件的旧/新条目，供 diff 展示。 */
export function parseChangeEvent(e: ChangeEvent) {
  return {
    oldItems: safeJsonParse<Record<string, unknown>[]>(e.old_items_json) ?? [],
    newItems: safeJsonParse<Record<string, unknown>[]>(e.new_items_json) ?? [],
  }
}

export function itemText(it: Record<string, unknown>): string {
  const text = it.text
  return typeof text === "string" && text ? text : JSON.stringify(it)
}
