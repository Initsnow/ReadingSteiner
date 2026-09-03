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

export type ImageKind = "none" | "items" | "css" | "changed"

export interface FormState {
  id: string
  name: string
  enabled: boolean
  notify_enabled: boolean
  follow_group: boolean
  url: string
  engine: string
  method: string
  cron: string
  cron_follow_global: boolean
  timeout_secs: number
  tags: string
  extractType: "text" | "items"
  selectorKind: "css" | "json_path"
  selector: string
  fieldsJson: string
  imageKind: ImageKind
  imageSelector: string
  screenshot: boolean
}

export function emptyForm(): FormState {
  return {
    id: "",
    name: "",
    enabled: true,
    notify_enabled: true,
    follow_group: true,
    url: "",
    engine: "http",
    method: "GET",
    cron: "",
    cron_follow_global: true,
    timeout_secs: 0,
    tags: "",
    extractType: "items",
    selectorKind: "css",
    selector: "",
    fieldsJson: "[]",
    imageKind: "none",
    imageSelector: "",
    screenshot: false,
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
  if (image.kind === "items") return { imageKind: "items" as const, imageSelector: "" }
  if (image.kind === "changed") return { imageKind: "changed" as const, imageSelector: "" }
  return { imageKind: "css" as const, imageSelector: image.selector }
}

function extractToForm(extract: ExtractConfig | undefined) {
  if (!extract || extract.type === "text") {
    return {
      extractType: "text" as const,
      selectorKind: "css" as const,
      selector: "",
      fieldsJson: "[]",
      ...imageToForm(extract?.type === "text" ? extract.images : undefined),
    }
  }
  const s = extract.selector
  return {
    extractType: "items" as const,
    selectorKind: s.kind,
    selector: s.kind === "css" ? s.selector : (s as { path: string }).path,
    fieldsJson: JSON.stringify(extract.fields ?? [], null, 2),
    ...imageToForm(extract.images),
  }
}

export function sourceToForm(s: SourceConfig): FormState {
  return {
    id: s.id,
    name: s.name,
    enabled: s.enabled,
    notify_enabled: s.notify_enabled ?? true,
    follow_group: s.follow_group ?? true,
    url: s.fetch.url,
    engine: s.fetch.engine,
    method: s.fetch.method,
    cron: s.schedule.cron ?? "",
    cron_follow_global: !s.schedule.cron,
    timeout_secs: s.fetch.timeout_secs ?? 0,
    tags: (s.tags ?? []).join(","),
    ...extractToForm(s.extract),
    screenshot: s.fetch.screenshot ?? false,
  }
}

function buildImageSelector(f: FormState): ImageSelector | undefined {
  if (f.imageKind === "none") return undefined
  if (f.imageKind === "items") return { kind: "items" }
  if (f.imageKind === "changed") return { kind: "changed" }
  return { kind: "css", selector: f.imageSelector.trim() }
}

export function buildSelector(f: FormState): ItemSelector {
  return f.selectorKind === "css"
    ? { kind: "css", selector: f.selector.trim() }
    : { kind: "json_path", path: f.selector.trim() }
}

export function buildExtract(f: FormState): ExtractConfig {
  if (f.extractType === "text") {
    return { type: "text", images: buildImageSelector(f) }
  }
  return {
    type: "items",
    selector: buildSelector(f),
    fields: safeJsonParse<ItemField[]>(f.fieldsJson) ?? [],
    images: buildImageSelector(f),
  }
}

export function formToSource(f: FormState, existing?: SourceConfig): SourceConfig {
  const base = existing ?? ({} as SourceConfig)
  return {
    ...base,
    id: f.id.trim(),
    name: f.name.trim(),
    enabled: f.enabled,
    notify_enabled: f.notify_enabled,
    follow_group: f.follow_group,
    tags: f.tags.split(",").map((t) => t.trim()).filter(Boolean),
    fetch: {
      ...base.fetch,
      engine: f.engine,
      url: f.url.trim(),
      method: f.method || "GET",
      // 0 表示未单独设置，跟随全局默认（后端会兜底）。
      timeout_secs: f.timeout_secs > 0 ? f.timeout_secs : 0,
      screenshot: f.screenshot,
    },
    schedule: {
      ...base.schedule,
      // 跟随全局时 cron 留空，后端自动使用全局默认值。
      cron: f.cron_follow_global ? undefined : f.cron.trim() || undefined,
    },
    extract: buildExtract(f),
  }
}

/**
 * 校验表单，返回首个错误；合法时返回 null。
 *
 * `groupForcesItemsExtract` 非空表示分组已接管内容提取，此时跳过源级提取校验。
 */
export function validateForm(
  f: FormState,
  groupForcesItemsExtract: string | null,
  validateCron: (expr: string) => string | null,
  isEditing: boolean,
): string | null {
  if (!f.url.trim()) return "url 为必填项"
  // 未跟随全局时 cron 必须填写，避免 UI 显示「自定义」而实际静默回退为「跟随全局」。
  if (!f.cron_follow_global && !f.cron.trim()) {
    return "取消“跟随全局”后，需要填写 cron 表达式"
  }
  if (!isEditing && !f.id.trim() && !f.url.trim()) return "url 为必填项"
  // 自定义 cron 做格式校验；跟随全局时使用全局默认值，由设置页负责校验。
  if (!f.cron_follow_global) {
    const cronErr = validateCron(f.cron)
    if (cronErr) return cronErr
  }
  if (groupForcesItemsExtract) return null
  if (f.extractType === "items" && !f.selector.trim()) {
    return "结构化提取需要填写选择器"
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
