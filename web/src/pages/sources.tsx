import { useEffect, useState } from "react"
import {
  Play,
  Loader2,
  RefreshCw,
  Plus,
  Pencil,
  Trash2,
  TestTube2,
  X,
  CheckCircle2,
  AlertCircle,
} from "lucide-react"
import {
  api,
  type SourceConfig,
  type TestSourceResult,
  type ExtractConfig,
  type ItemField,
  type ImageSelector,
} from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription } from "@/components/ui/card"

// ---- editable field model for the add/edit form ----
interface FormState {
  id: string
  name: string
  enabled: boolean
  notify_enabled: boolean
  url: string
  engine: string
  method: string
  cron: string
  cron_follow_global: boolean
  timeout_secs: number
  tags: string
  // 内容提取
  extractType: "text" | "items"
  selectorKind: "css" | "json_path"
  selector: string
  fieldsJson: string
  // 图片选择器
  imageKind: "none" | "items" | "css" | "changed"
  imageSelector: string
}

function emptyForm(): FormState {
  return {
    id: "",
    name: "",
    enabled: true,
    notify_enabled: true,
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
  }
}

function imageToForm(image: ImageSelector | undefined): {
  imageKind: "none" | "items" | "css" | "changed"
  imageSelector: string
} {
  if (!image || image.kind === "none") return { imageKind: "none", imageSelector: "" }
  if (image.kind === "items") return { imageKind: "items", imageSelector: "" }
  if (image.kind === "changed") return { imageKind: "changed", imageSelector: "" }
  return { imageKind: "css", imageSelector: image.selector }
}

function extractToForm(extract: ExtractConfig | undefined): Pick<
  FormState,
  "extractType" | "selectorKind" | "selector" | "fieldsJson" | "imageKind" | "imageSelector"
> {
  if (!extract || extract.type === "text") {
    return {
      extractType: "text",
      selectorKind: "css",
      selector: "",
      fieldsJson: "[]",
      ...imageToForm(extract?.type === "text" ? extract.images : undefined),
    }
  }
  const s = extract.selector
  return {
    extractType: "items",
    selectorKind: s.kind,
    selector: s.kind === "css" ? s.selector : (s as { path: string }).path,
    fieldsJson: JSON.stringify(extract.fields ?? [], null, 2),
    ...imageToForm(extract.images),
  }
}

function sourceToForm(s: SourceConfig): FormState {
  const ex = extractToForm(s.extract)
  return {
    id: s.id,
    name: s.name,
    enabled: s.enabled,
    notify_enabled: s.notify_enabled ?? true,
    url: s.fetch.url,
    engine: s.fetch.engine,
    method: s.fetch.method,
    cron: s.schedule.cron ?? "",
    cron_follow_global: !s.schedule.cron,
    timeout_secs: s.fetch.timeout_secs ?? 0,
    tags: (s.tags ?? []).join(","),
    ...ex,
  }
}

function buildImageSelector(f: FormState): ImageSelector | undefined {
  if (f.imageKind === "none") return undefined
  if (f.imageKind === "items") return { kind: "items" }
  if (f.imageKind === "changed") return { kind: "changed" }
  return { kind: "css", selector: f.imageSelector.trim() }
}

function buildExtract(f: FormState): ExtractConfig {
  if (f.extractType === "text") {
    return { type: "text", images: buildImageSelector(f) }
  }
  return {
    type: "items",
    selector:
      f.selectorKind === "css"
        ? { kind: "css", selector: f.selector.trim() }
        : { kind: "json_path", path: f.selector.trim() },
    fields: safeJsonParse<ItemField[]>(f.fieldsJson) ?? [],
    images: buildImageSelector(f),
  }
}

function safeJsonParse<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T
  } catch {
    return null
  }
}

function formToSource(f: FormState, existing?: SourceConfig): SourceConfig {
  const base = existing ?? ({} as SourceConfig)
  return {
    ...base,
    id: f.id.trim(),
    name: f.name.trim() || f.id.trim(),
    enabled: f.enabled,
    notify_enabled: f.notify_enabled,
    tags: f.tags
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean),
    fetch: {
      ...base.fetch,
      engine: f.engine,
      url: f.url.trim(),
      method: f.method || "GET",
      // 0 表示未单独设置，跟随全局默认（后端 effective_timeout 会兜底）。
      timeout_secs: f.timeout_secs > 0 ? f.timeout_secs : 0,
    },
    schedule: {
      ...base.schedule,
      // 仅 cron 表达式调度。
      // 跟随全局时 cron 留空，后端自动使用全局默认值。
      cron: f.cron_follow_global ? undefined : f.cron.trim() || undefined,
    },
    extract: buildExtract(f),
  }
}

const inputCls =
  "w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
const labelCls = "text-xs font-medium text-muted-foreground"

function Field({
  label,
  hint,
  children,
  className,
}: {
  label: string
  hint?: string
  children: React.ReactNode
  className?: string
}) {
  return (
    <div className={className}>
      <label className={labelCls}>{label}</label>
      <div className="mt-1">{children}</div>
      {hint && <p className="mt-1 text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}

export function SourcesPage() {
  const [sources, setSources] = useState<SourceConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  // 多选：选中的监控源 id 集合（用于批量暂停监控 / 暂停通知）。
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [batchBusy, setBatchBusy] = useState(false)

  // add/edit modal state
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<SourceConfig | null>(null)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [saving, setSaving] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)
  const [previewingTitle, setPreviewingTitle] = useState(false)

  // test result modal state
  const [testResult, setTestResult] = useState<TestSourceResult | null>(null)
  const [testOpen, setTestOpen] = useState(false)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testError, setTestError] = useState<string | null>(null)

  async function load() {
    try {
      setSources(await api.listSources())
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
  }, [])

  async function runCheck(id: string) {
    setBusyId(id)
    try {
      await api.check(id)
      await load()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusyId(null)
    }
  }

  function openAdd() {
    setEditing(null)
    setForm(emptyForm())
    setFormError(null)
    setModalOpen(true)
  }

  function openEdit(s: SourceConfig) {
    setEditing(s)
    setForm(sourceToForm(s))
    setFormError(null)
    setModalOpen(true)
  }

  // 抓取 URL 的页面标题，自动填充名称。
  async function fetchTitle() {
    const url = form.url.trim()
    if (!url) return
    setPreviewingTitle(true)
    try {
      const res = await api.previewSource(url, form.engine)
      if (res.title) {
        setForm((prev) => ({ ...prev, name: res.title }))
      } else {
        setFormError("未能自动获取标题（页面无 title 或非 HTML/JSON），请手动填写名称")
      }
    } catch {
      setFormError("获取标题失败，请检查 URL 或手动填写名称")
    } finally {
      setPreviewingTitle(false)
    }
  }

  async function handleSave() {
    if (!form.url.trim()) {
      setFormError("url 为必填项")
      return
    }
    if (form.extractType === "items" && !form.selector.trim()) {
      setFormError("结构化提取需要填写选择器")
      return
    }
    if (form.extractType === "items" && form.fieldsJson.trim()) {
      try {
        JSON.parse(form.fieldsJson)
      } catch {
        setFormError("字段配置不是合法的 JSON，请检查语法")
        return
      }
    }
    setSaving(true)
    setFormError(null)
    try {
      if (editing) {
        await api.updateSource(editing.id, formToSource(form, editing))
      } else {
        await api.addSource(formToSource(form))
      }
      setModalOpen(false)
      await load()
    } catch (e) {
      setFormError((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete(s: SourceConfig) {
    if (!window.confirm(`确定删除监控源「${s.name || s.id}」？此操作不可撤销。`)) return
    setBusyId(s.id)
    try {
      await api.deleteSource(s.id)
      await load()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusyId(null)
    }
  }

  async function handleTest(s: SourceConfig) {
    setTestingId(s.id)
    setTestError(null)
    setTestResult(null)
    setTestOpen(true)
    try {
      setTestResult(await api.testSource(s.id))
    } catch (e) {
      setTestError((e as Error).message)
    } finally {
      setTestingId(null)
    }
  }

  // ---- 多选批量操作 ----
  function toggleSelect(id: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  function toggleSelectAll(checked: boolean) {
    setSelected(checked ? new Set(sources.map((s) => s.id)) : new Set())
  }

  async function runBatch(
    flags: { enabled?: boolean; notify_enabled?: boolean },
    confirmText?: string,
  ) {
    const ids = [...selected]
    if (ids.length === 0) return
    if (confirmText && !window.confirm(confirmText)) return
    setBatchBusy(true)
    setError(null)
    try {
      await api.batchSetFlags(ids, flags)
      await load()
      setSelected(new Set())
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBatchBusy(false)
    }
  }

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 加载监控源…
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <CardDescription>共 {sources.length} 个监控源</CardDescription>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load}>
            <RefreshCw className="h-4 w-4" /> 刷新
          </Button>
          <Button size="sm" onClick={openAdd}>
            <Plus className="h-4 w-4" /> 添加监控源
          </Button>
        </div>
      </div>

      {/* 多选批量操作栏：选中任意监控源后出现 */}
      {selected.size > 0 && (
        <div className="flex flex-wrap items-center gap-3 rounded-md border bg-muted/40 px-3 py-2">
          <span className="text-sm font-medium">已选 {selected.size} 个监控源</span>
          <div className="flex-1" />
          <Button
            size="sm"
            variant="outline"
            disabled={batchBusy}
            onClick={() => runBatch({ enabled: false })}
          >
            {batchBusy && <Loader2 className="h-4 w-4 animate-spin" />}
            暂停监控
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={batchBusy}
            onClick={() => runBatch({ enabled: true })}
          >
            {batchBusy && <Loader2 className="h-4 w-4 animate-spin" />}
            恢复监控
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={batchBusy}
            onClick={() => runBatch({ notify_enabled: false })}
          >
            {batchBusy && <Loader2 className="h-4 w-4 animate-spin" />}
            暂停通知
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={batchBusy}
            onClick={() => runBatch({ notify_enabled: true })}
          >
            {batchBusy && <Loader2 className="h-4 w-4 animate-spin" />}
            恢复通知
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={batchBusy}
            onClick={() => setSelected(new Set())}
          >
            取消选择
          </Button>
        </div>
      )}

      {error && (
        <p className="text-sm text-destructive">加载失败：{error}</p>
      )}

      {sources.length === 0 ? (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            暂无监控源。点击右上角「添加监控源」创建。
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-4">
          <div className="flex items-center gap-2 px-1 text-sm text-muted-foreground">
            <label className="flex cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                className="h-4 w-4 accent-primary"
                checked={selected.size > 0 && selected.size === sources.length}
                ref={(el) => {
                  if (el) {
                    el.indeterminate =
                      selected.size > 0 && selected.size < sources.length
                  }
                }}
                onChange={(e) => toggleSelectAll(e.target.checked)}
              />
              全选
            </label>
          </div>
          {sources.map((s) => (
            <Card
              key={s.id}
              className={
                selected.has(s.id) ? "ring-2 ring-primary/60" : undefined
              }
            >
              <CardContent className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
                <input
                  type="checkbox"
                  className="h-4 w-4 shrink-0 accent-primary"
                  checked={selected.has(s.id)}
                  onChange={() => toggleSelect(s.id)}
                />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate text-sm font-medium">
                      {s.name || s.id}
                    </span>
                    <Badge
                      variant={s.enabled ? "success" : "secondary"}
                      className="px-1.5 py-0 text-[10px]"
                    >
                      {s.enabled ? "监控中" : "已暂停监控"}
                    </Badge>
                    <Badge
                      variant={s.notify_enabled ? "success" : "secondary"}
                      className="px-1.5 py-0 text-[10px]"
                    >
                      {s.notify_enabled ? "通知开" : "已暂停通知"}
                    </Badge>
                    {s.tags.map((t) => (
                      <Badge
                        key={t}
                        variant="outline"
                        className="px-1.5 py-0 text-[10px]"
                      >
                        {t}
                      </Badge>
                    ))}
                  </div>
                  <div className="mt-0.5 truncate text-xs text-muted-foreground">
                    {s.fetch.url}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 px-2 text-xs"
                    disabled={busyId === s.id}
                    onClick={() => runCheck(s.id)}
                    title="立即检测"
                  >
                    {busyId === s.id ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Play className="h-3.5 w-3.5" />
                    )}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 px-2 text-xs"
                    disabled={testingId === s.id}
                    onClick={() => handleTest(s)}
                    title="测试"
                  >
                    {testingId === s.id ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <TestTube2 className="h-3.5 w-3.5" />
                    )}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 px-2 text-xs"
                    onClick={() => openEdit(s)}
                  >
                    <Pencil className="h-3.5 w-3.5" /> 编辑
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 px-2 text-xs text-destructive hover:text-destructive"
                    disabled={busyId === s.id}
                    onClick={() => handleDelete(s)}
                  >
                    <Trash2 className="h-3.5 w-3.5" /> 删除
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Add / Edit modal */}
      {modalOpen && (
        <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-black/50 p-4">
          <div className="mt-8 w-full max-w-2xl rounded-xl border bg-card p-6 shadow-lg">
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold">
                {editing ? "编辑监控源" : "添加监控源"}
              </h2>
              <Button size="icon" variant="ghost" onClick={() => setModalOpen(false)}>
                <X className="h-4 w-4" />
              </Button>
            </div>

            {formError && (
              <p className="mt-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {formError}
              </p>
            )}

            <div className="mt-4 grid grid-cols-2 gap-4">
              <Field label="URL" className="col-span-2">
                <input
                  className={inputCls}
                  value={form.url}
                  placeholder="https://example.com/list"
                  onChange={(e) => setForm({ ...form, url: e.target.value })}
                />
              </Field>

              <Field label="名称" className="col-span-1">
                <input
                  className={inputCls}
                  value={form.name}
                  placeholder="留空自动获取网页标题"
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </Field>
              <div className="col-span-1 flex items-end">
                <Button
                  variant="outline"
                  className="w-full"
                  disabled={!form.url.trim() || previewingTitle}
                  onClick={fetchTitle}
                >
                  {previewingTitle && (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  )}
                  获取标题
                </Button>
              </div>

              <div className="col-span-2 flex flex-wrap items-center gap-6 rounded-md border bg-muted/30 p-3">
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    className="h-4 w-4 accent-primary"
                    checked={form.enabled}
                    onChange={(e) =>
                      setForm({ ...form, enabled: e.target.checked })
                    }
                  />
                  <span>启用监控</span>
                </label>
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    className="h-4 w-4 accent-primary"
                    checked={form.notify_enabled}
                    onChange={(e) =>
                      setForm({ ...form, notify_enabled: e.target.checked })
                    }
                  />
                  <span>启用通知</span>
                </label>
              </div>

              <Field label="引擎 (engine)">
                <select
                  className={inputCls}
                  value={form.engine}
                  onChange={(e) => setForm({ ...form, engine: e.target.value })}
                >
                  <option value="http">http</option>
                  <option value="camofox">camofox</option>
                </select>
              </Field>
              <Field label="请求方法 (method)">
                <select
                  className={inputCls}
                  value={form.method}
                  onChange={(e) => setForm({ ...form, method: e.target.value })}
                >
                  <option value="GET">GET</option>
                  <option value="POST">POST</option>
                  <option value="HEAD">HEAD</option>
                </select>
              </Field>

              <Field
                label="cron 表达式（分 时 日 月 周）"
                className="col-span-2"
                hint={"例：*/15 * * * *（每 15 分钟）、0 9,18 * * 1-5（工作日 9:00/18:00）。"}
              >
                <div className="flex items-center gap-3 rounded-md border bg-muted/30 px-3 py-2">
                  <label className="flex shrink-0 cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-primary"
                      checked={form.cron_follow_global}
                      onChange={(e) =>
                        setForm({ ...form, cron_follow_global: e.target.checked })
                      }
                    />
                    跟随全局
                  </label>
                  {!form.cron_follow_global && (
                    <input
                      type="text"
                      className={`${inputCls} flex-1`}
                      placeholder="0 * * * *"
                      value={form.cron}
                      onChange={(e) =>
                        setForm({ ...form, cron: e.target.value })
                      }
                    />
                  )}
                </div>
              </Field>

              <Field label="内容提取" className="col-span-2">
                <select
                  className={inputCls}
                  value={form.extractType}
                  onChange={(e) =>
                    setForm({
                      ...form,
                      extractType: e.target.value as "text" | "items",
                    })
                  }
                >
                  <option value="text">整页文本</option>
                  <option value="items">结构化提取</option>
                </select>
              </Field>

              {form.extractType === "items" && (
                <div className="col-span-2 space-y-4 rounded-md border bg-muted/30 p-3">
                  <div className="grid grid-cols-2 gap-4">
                    <Field label="选择器类型">
                      <select
                        className={inputCls}
                        value={form.selectorKind}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            selectorKind: e.target.value as "css" | "json_path",
                            selector: "",
                          })
                        }
                      >
                        <option value="css">CSS</option>
                        <option value="json_path">JSONPath</option>
                      </select>
                    </Field>
                    <Field
                      label={
                        form.selectorKind === "json_path"
                          ? "JSONPath 路径"
                          : "CSS 选择器"
                      }
                    >
                      <input
                        className={inputCls}
                        value={form.selector}
                        placeholder={
                          form.selectorKind === "json_path"
                            ? "$.data.items[*]"
                            : ".product"
                        }
                        onChange={(e) =>
                          setForm({ ...form, selector: e.target.value })
                        }
                      />
                    </Field>
                  </div>
                  <Field label={`提取字段（可选 JSON 数组，如 [{"name":"id","attr":"data-id"}]）`}>
                    <textarea
                      rows={3}
                      className={`${inputCls} font-mono text-xs`}
                      value={form.fieldsJson}
                      placeholder='[{"name":"id","attr":"data-id"},{"name":"title","selector":".title"}]'
                      onChange={(e) =>
                        setForm({ ...form, fieldsJson: e.target.value })
                      }
                    />
                  </Field>
                </div>
              )}

              {/* 图片通知：选择要随变更通知附带的图片 */}
              <Field label="通知附带图片" className="col-span-2">
                <div className="rounded-md border bg-muted/30 p-3">
                  <div className="grid grid-cols-2 gap-4">
                    <Field label="图片来源">
                      <select
                        className={inputCls}
                        value={form.imageKind}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            imageKind: e.target.value as
                              | "none"
                              | "items"
                              | "css"
                              | "changed",
                          })
                        }
                      >
                        <option value="none">不附带图片</option>
                        {form.extractType === "items" && (
                          <option value="items">条目的图片</option>
                        )}
                        {form.extractType === "items" && (
                          <option value="changed">变更元素的图片</option>
                        )}
                        <option value="css">按 CSS 选择器</option>
                      </select>
                    </Field>
                    {form.imageKind === "css" && (
                      <Field label="图片 CSS 选择器">
                        <input
                          className={inputCls}
                          value={form.imageSelector}
                          placeholder=".cover img 或 img.product-thumb"
                          onChange={(e) =>
                            setForm({ ...form, imageSelector: e.target.value })
                          }
                        />
                      </Field>
                    )}
                  </div>
                </div>
              </Field>

              <Field label="超时" className="col-span-1">
                <div className="flex items-center gap-3 rounded-md border bg-muted/30 px-3 py-2">
                  <label className="flex cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-primary"
                      checked={form.timeout_secs === 0}
                      onChange={(e) =>
                        setForm({
                          ...form,
                          timeout_secs: e.target.checked ? 0 : 30,
                        })
                      }
                    />
                    跟随全局
                  </label>
                  {form.timeout_secs !== 0 && (
                    <span className="flex items-center gap-1">
                      <input
                        type="number"
                        min={1}
                        className="w-20 rounded-md border border-input bg-background px-2 py-1 text-sm"
                        value={form.timeout_secs}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            timeout_secs: Number(e.target.value),
                          })
                        }
                      />
                      <span className="text-xs text-muted-foreground">秒</span>
                    </span>
                  )}
                </div>
              </Field>
              <Field label="标签（逗号分隔）">
                <input
                  className={inputCls}
                  value={form.tags}
                  placeholder="a,b"
                  onChange={(e) => setForm({ ...form, tags: e.target.value })}
                />
              </Field>
            </div>

            <div className="mt-6 flex items-center justify-end gap-2">
              <Button
                variant="ghost"
                onClick={() => setModalOpen(false)}
                disabled={saving}
              >
                取消
              </Button>
              <Button onClick={handleSave} disabled={saving}>
                {saving && <Loader2 className="h-4 w-4 animate-spin" />}
                保存
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Test result modal */}
      {testOpen && (
        <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-black/50 p-4">
          <div className="mt-8 w-full max-w-3xl rounded-xl border bg-card p-6 shadow-lg">
            <div className="flex items-center justify-between">
              <h2 className="flex items-center gap-2 text-lg font-semibold">
                测试监控源
                {testingId && <Loader2 className="h-4 w-4 animate-spin" />}
              </h2>
              <Button size="icon" variant="ghost" onClick={() => setTestOpen(false)}>
                <X className="h-4 w-4" />
              </Button>
            </div>

            {testError && (
              <p className="mt-2 flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                <AlertCircle className="h-4 w-4" /> {testError}
              </p>
            )}

            {testResult && (
              <div className="mt-4 space-y-4">
                <div className="flex flex-wrap items-center gap-4 rounded-md border bg-muted/40 p-3 text-sm">
                  {testResult.status !== undefined && (
                    <span
                      className={
                        testResult.status < 400
                          ? "flex items-center gap-1 text-green-600"
                          : "flex items-center gap-1 text-destructive"
                      }
                    >
                      <CheckCircle2 className="h-4 w-4" /> HTTP {testResult.status}
                    </span>
                  )}
                  {testResult.engine && <span>engine: {testResult.engine}</span>}
                  {testResult.duration_ms !== undefined && (
                    <span>{testResult.duration_ms}ms</span>
                  )}
                  {testResult.text_len !== undefined && (
                    <span>text: {testResult.text_len} chars</span>
                  )}
                  <span className="break-all">{testResult.final_url}</span>
                </div>

                <div>
                  <div className="text-xs font-medium text-muted-foreground">
                    fingerprint:{" "}
                    <code className="break-all">
                      {testResult.fingerprint ?? "-"}
                    </code>
                  </div>
                  <div className="mt-2 text-xs font-medium text-muted-foreground">
                    提取到 {testResult.items?.length ?? 0} 个条目
                  </div>
                  {testResult.items && testResult.items.length > 0 ? (
                    <div className="mt-2 max-h-80 overflow-y-auto rounded-md border">
                      <table className="w-full text-left text-xs">
                        <thead className="sticky top-0 bg-muted">
                          <tr>
                            <th className="px-3 py-2">stable_id</th>
                            <th className="px-3 py-2">fields</th>
                            <th className="px-3 py-2">text</th>
                          </tr>
                        </thead>
                        <tbody>
                          {testResult.items.map((item, i) => (
                            <tr key={i} className="border-t">
                              <td className="px-3 py-2 font-medium">
                                {item.stable_id}
                              </td>
                              <td className="px-3 py-2 break-all">
                                {Object.entries(item.fields)
                                  .map(([k, v]) => `${k}=${v}`)
                                  .join(" | ") || "-"}
                              </td>
                              <td className="max-w-[200px] truncate px-3 py-2">
                                {item.text || "-"}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  ) : (
                    <p className="mt-2 text-xs text-muted-foreground">
                      未提取到条目（可能未配置选择器，或页面为空）。
                    </p>
                  )}
                </div>
              </div>
            )}

            <div className="mt-6 flex justify-end">
              <Button variant="ghost" onClick={() => setTestOpen(false)}>
                关闭
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
