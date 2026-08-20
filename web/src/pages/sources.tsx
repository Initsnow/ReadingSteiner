import { useEffect, useState } from "react"
import {
  Play,
  FlaskConical,
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
import { api, type SourceConfig, type TestSourceResult } from "@/lib/api"
import type { PipelineConfig, ExtractConfig, FieldSelector } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

// ---- editable field model for the add/edit form ----
interface FormState {
  id: string
  name: string
  enabled: boolean
  url: string
  engine: string
  method: string
  interval_secs: number
  jitter_secs: number
  priority: number
  pipeline: string
  timeout_secs: number
  tags: string
  compareMode: string
  stable_id: string
  notify_on: string
  // inline pipeline (content selector)
  useInline: boolean
  extractType: string
  extractSelector: string
  extractFields: string
  normalizeJson: string
  filterJson: string
}

function emptyForm(): FormState {
  return {
    id: "",
    name: "",
    enabled: true,
    url: "",
    engine: "http",
    method: "GET",
    interval_secs: 60,
    jitter_secs: 5,
    priority: 0,
    pipeline: "default",
    timeout_secs: 30,
    tags: "",
    compareMode: "item_set",
    stable_id: "id",
    notify_on: "new,updated,removed",
    useInline: true,
    extractType: "auto_text",
    extractSelector: "",
    extractFields: "{}",
    normalizeJson: "[]",
    filterJson: "{}",
  }
}

const EXTRACT_LABELS: Record<string, string> = {
  auto_text: "auto_text（整页文本）",
  auto_images: "auto_images（页面图片）",
  camofox_images: "camofox_images（浏览器图片）",
  css_items: "css_items（CSS 选择器）",
  xpath: "xpath（XPath 选择器）",
  json_path: "json_path（JSON 路径）",
  regex: "regex（正则）",
}

function extractToForm(s: SourceConfig): { extractType: string; extractSelector: string; extractFields: string } {
  const ex = s.pipeline_config?.extract?.[0]
  if (!ex) {
    return { extractType: "auto_text", extractSelector: "", extractFields: "{}" }
  }
  switch (ex.type) {
    case "css_items":
    case "xpath":
      return {
        extractType: ex.type,
        extractSelector: (ex as { selector: string }).selector,
        extractFields: JSON.stringify((ex as { fields?: Record<string, unknown> }).fields ?? {}, null, 2),
      }
    case "json_path":
      return {
        extractType: ex.type,
        extractSelector: (ex as { path: string }).path,
        extractFields: JSON.stringify((ex as { fields?: Record<string, unknown> }).fields ?? {}, null, 2),
      }
    case "regex":
      return {
        extractType: ex.type,
        extractSelector: (ex as { pattern: string }).pattern,
        extractFields: JSON.stringify((ex as { fields?: Record<string, unknown> }).fields ?? {}, null, 2),
      }
    default:
      return { extractType: ex.type, extractSelector: "", extractFields: "{}" }
  }
}

function sourceToForm(s: SourceConfig): FormState {
  const ex = extractToForm(s)
  return {
    id: s.id,
    name: s.name,
    enabled: s.enabled,
    url: s.fetch.url,
    engine: s.fetch.engine,
    method: s.fetch.method,
    interval_secs: s.schedule.interval_secs,
    jitter_secs: s.schedule.jitter_secs ?? 5,
    priority: s.priority,
    pipeline: s.pipeline,
    timeout_secs: s.fetch.timeout_secs ?? 30,
    tags: (s.tags ?? []).join(","),
    compareMode: s.compare.mode,
    stable_id: s.compare.stable_id ?? "id",
    notify_on: (s.compare.notify_on ?? []).join(","),
    useInline: !!s.pipeline_config,
    extractType: ex.extractType,
    extractSelector: ex.extractSelector,
    extractFields: ex.extractFields,
    normalizeJson: JSON.stringify(s.pipeline_config?.normalize ?? [], null, 2),
    filterJson: JSON.stringify(s.pipeline_config?.filter ?? {}, null, 2),
  }
}

function buildInlinePipeline(f: FormState): PipelineConfig | null {
  let extract: ExtractConfig[]
  switch (f.extractType) {
    case "css_items":
    case "xpath": {
      const fields = safeJsonParse<Record<string, FieldSelector>>(f.extractFields) ?? {}
      extract = [{ type: f.extractType, selector: f.extractSelector, fields }]
      break
    }
    case "json_path": {
      const fields = safeJsonParse<Record<string, FieldSelector>>(f.extractFields) ?? {}
      extract = [{ type: "json_path", path: f.extractSelector, fields }]
      break
    }
    case "regex": {
      const fields = safeJsonParse<Record<string, FieldSelector>>(f.extractFields) ?? {}
      extract = [{ type: "regex", pattern: f.extractSelector, fields }]
      break
    }
    case "auto_images":
      extract = [{ type: "auto_images" }]
      break
    case "camofox_images":
      extract = [{ type: "camofox_images" }]
      break
    default:
      extract = [{ type: "auto_text" }]
  }
  return {
    extract,
    normalize: safeJsonParse(f.normalizeJson) ?? [],
    filter: safeJsonParse(f.filterJson) ?? {},
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
  const pipelineConfig = f.useInline ? buildInlinePipeline(f) : null
  const structured = f.useInline && f.extractType !== "auto_text" && f.extractType !== "auto_images" && f.extractType !== "camofox_images"
  return {
    ...base,
    id: f.id.trim(),
    name: f.name.trim() || f.id.trim(),
    enabled: f.enabled,
    tags: f.tags
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean),
    fetch: {
      ...base.fetch,
      engine: f.engine,
      url: f.url.trim(),
      method: f.method || "GET",
      timeout_secs: f.timeout_secs || 30,
    },
    schedule: {
      ...base.schedule,
      interval_secs: f.interval_secs || 60,
      jitter_secs: f.jitter_secs || 5,
    },
    priority: f.priority || 0,
    pipeline: f.pipeline || "default",
    pipeline_config: pipelineConfig,
    compare: {
      ...base.compare,
      // 结构化内容选择器 -> 强制 item_set 逐项比对；其余情况尊重用户选择的比较模式
      mode: f.useInline && structured ? "item_set" : f.compareMode,
      stable_id: f.stable_id,
      notify_on: f.notify_on
        .split(",")
        .map((x) => x.trim())
        .filter(Boolean),
    },
  }
}

const inputCls =
  "w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
const labelCls = "text-xs font-medium text-muted-foreground"

function Field({
  label,
  children,
  className,
}: {
  label: string
  children: React.ReactNode
  className?: string
}) {
  return (
    <div className={className}>
      <label className={labelCls}>{label}</label>
      <div className="mt-1">{children}</div>
    </div>
  )
}

export function SourcesPage() {
  const [sources, setSources] = useState<SourceConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  // add/edit modal state
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<SourceConfig | null>(null)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [saving, setSaving] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

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

  async function testPipeline(id: string) {
    setBusyId(id)
    try {
      await api.testPipeline(id)
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

  function validateInlineJson(): string | null {
    if (!form.useInline) return null
    const requiredFields: Array<[string, string]> = []
    if (
      form.extractType === "css_items" ||
      form.extractType === "xpath" ||
      form.extractType === "json_path" ||
      form.extractType === "regex"
    ) {
      requiredFields.push(["extractFields", form.extractFields])
    }
    requiredFields.push(["normalizeJson", form.normalizeJson])
    requiredFields.push(["filterJson", form.filterJson])
    for (const [name, value] of requiredFields) {
      if (!value.trim()) continue
      try {
        JSON.parse(value)
      } catch {
        return `${name} 不是合法的 JSON，请检查语法`
      }
    }
    return null
  }

  async function handleSave() {
    if (!form.id.trim() || !form.url.trim()) {
      setFormError("id 和 url 为必填项")
      return
    }
    const jsonErr = validateInlineJson()
    if (jsonErr) {
      setFormError(jsonErr)
      return
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
          {sources.map((s) => (
            <Card key={s.id}>
              <CardHeader className="flex flex-row items-start justify-between space-y-0">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    {s.name || s.id}
                    <Badge variant={s.enabled ? "success" : "secondary"}>
                      {s.enabled ? "enabled" : "disabled"}
                    </Badge>
                  </CardTitle>
                  <CardDescription className="mt-1 break-all">
                    {s.fetch.url}
                  </CardDescription>
                </div>
              </CardHeader>
              <CardContent>
                <div className="flex flex-wrap items-center gap-4 text-xs text-muted-foreground">
                  <span>engine: {s.fetch.engine}</span>
                  <span>interval: {s.schedule.interval_secs}s</span>
                  <span>pipeline: {s.pipeline}</span>
                  <span>compare: {s.compare.mode}</span>
                  <span>priority: {s.priority}</span>
                </div>
                {s.tags.length > 0 && (
                  <div className="mt-3 flex flex-wrap gap-1">
                    {s.tags.map((t) => (
                      <Badge key={t} variant="outline">
                        {t}
                      </Badge>
                    ))}
                  </div>
                )}
                <div className="mt-4 flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === s.id}
                    onClick={() => runCheck(s.id)}
                  >
                    {busyId === s.id ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Play className="h-4 w-4" />
                    )}
                    立即检测
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === s.id}
                    onClick={() => handleTest(s)}
                  >
                    {testingId === s.id ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <TestTube2 className="h-4 w-4" />
                    )}
                    测试
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busyId === s.id}
                    onClick={() => testPipeline(s.id)}
                  >
                    <FlaskConical className="h-4 w-4" />
                    试跑流水线
                  </Button>
                  <div className="flex-1" />
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => openEdit(s)}
                  >
                    <Pencil className="h-4 w-4" /> 编辑
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="text-destructive hover:text-destructive"
                    disabled={busyId === s.id}
                    onClick={() => handleDelete(s)}
                  >
                    <Trash2 className="h-4 w-4" /> 删除
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
              <Field label="ID（唯一标识）" className="col-span-1">
                <input
                  className={inputCls}
                  value={form.id}
                  disabled={!!editing}
                  placeholder="e.g. shop-list"
                  onChange={(e) => setForm({ ...form, id: e.target.value })}
                />
              </Field>
              <Field label="名称" className="col-span-1">
                <input
                  className={inputCls}
                  value={form.name}
                  placeholder="e.g. Shop List"
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </Field>

              <Field label="URL" className="col-span-2">
                <input
                  className={inputCls}
                  value={form.url}
                  placeholder="https://example.com/list"
                  onChange={(e) => setForm({ ...form, url: e.target.value })}
                />
              </Field>

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

              <Field label="检测间隔（秒）">
                <input
                  type="number"
                  className={inputCls}
                  value={form.interval_secs}
                  onChange={(e) =>
                    setForm({ ...form, interval_secs: Number(e.target.value) })
                  }
                />
              </Field>
              <Field label="抖动（秒）">
                <input
                  type="number"
                  className={inputCls}
                  value={form.jitter_secs}
                  onChange={(e) =>
                    setForm({ ...form, jitter_secs: Number(e.target.value) })
                  }
                />
              </Field>

              <Field label="流水线 (pipeline)" className="col-span-1">
                <input
                  className={inputCls}
                  value={form.pipeline}
                  disabled={form.useInline}
                  placeholder="default"
                  onChange={(e) => setForm({ ...form, pipeline: e.target.value })}
                />
              </Field>
              <Field label="内容选择器" className="col-span-1">
                <label className="flex h-9 items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={form.useInline}
                    onChange={(e) =>
                      setForm({ ...form, useInline: e.target.checked })
                    }
                  />
                  使用内联选择器（在本页直接配置提取规则）
                </label>
              </Field>

              {form.useInline && (
                <div className="col-span-2 space-y-4 rounded-md border bg-muted/30 p-3">
                  <div className="grid grid-cols-2 gap-4">
                    <Field label="提取类型 (extract type)">
                      <select
                        className={inputCls}
                        value={form.extractType}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            extractType: e.target.value,
                            extractSelector: "",
                          })
                        }
                      >
                        {Object.entries(EXTRACT_LABELS).map(([v, label]) => (
                          <option key={v} value={v}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field label={
                      form.extractType === "json_path"
                        ? "JSON 路径 (path)"
                        : form.extractType === "regex"
                          ? "正则 (pattern)"
                          : "CSS/XPath 选择器 (selector)"
                    }>
                      <input
                        className={inputCls}
                        value={form.extractSelector}
                        placeholder={
                          form.extractType === "json_path"
                            ? "$.data.items[*]"
                            : form.extractType === "regex"
                              ? "(?P<id>\\d+)"
                              : ".item"
                        }
                        onChange={(e) =>
                          setForm({ ...form, extractSelector: e.target.value })
                        }
                      />
                    </Field>
                  </div>

                  {(form.extractType === "css_items" ||
                    form.extractType === "xpath" ||
                    form.extractType === "json_path" ||
                    form.extractType === "regex") && (
                    <Field label={`提取字段 (fields，JSON，如 {"id":{"attr":"data-id"}})`}>
                      <textarea
                        rows={3}
                        className={`${inputCls} font-mono text-xs`}
                        value={form.extractFields}
                        placeholder='{"id":{"attr":"data-id"},"title":{"selector":".title"}}'
                        onChange={(e) =>
                          setForm({ ...form, extractFields: e.target.value })
                        }
                      />
                    </Field>
                  )}

                  <Field label={`normalize（JSON 数组，如 [{"type":"trim","field":"title"}]）`}>
                    <textarea
                      rows={2}
                      className={`${inputCls} font-mono text-xs`}
                      value={form.normalizeJson}
                      onChange={(e) =>
                        setForm({ ...form, normalizeJson: e.target.value })
                      }
                    />
                  </Field>
                  <Field label={`filter（JSON 对象，如 {"include":[{"op":"gt","field":"price","value":0}]}）`}>
                    <textarea
                      rows={2}
                      className={`${inputCls} font-mono text-xs`}
                      value={form.filterJson}
                      onChange={(e) =>
                        setForm({ ...form, filterJson: e.target.value })
                      }
                    />
                  </Field>
                  <p className="text-xs text-muted-foreground">
                    配置结构化选择器（css_items / xpath / json_path / regex）后，变更检测会自动按提取到的条目（item_set + stable_id）比对，不再需要手选比较模式。
                  </p>
                </div>
              )}
              <Field label="优先级 (priority)">
                <input
                  type="number"
                  className={inputCls}
                  value={form.priority}
                  onChange={(e) =>
                    setForm({ ...form, priority: Number(e.target.value) })
                  }
                />
              </Field>

              <Field label="超时（秒）">
                <input
                  type="number"
                  className={inputCls}
                  value={form.timeout_secs}
                  onChange={(e) =>
                    setForm({ ...form, timeout_secs: Number(e.target.value) })
                  }
                />
              </Field>
              <Field label="标签（逗号分隔）">
                <input
                  className={inputCls}
                  value={form.tags}
                  placeholder="a,b"
                  onChange={(e) => setForm({ ...form, tags: e.target.value })}
                />
              </Field>

              <Field label="比较模式 (compare mode)">
                <select
                  className={inputCls}
                  value={form.compareMode}
                  disabled={
                    form.useInline &&
                    form.extractType !== "auto_text" &&
                    form.extractType !== "auto_images" &&
                    form.extractType !== "camofox_images"
                  }
                  onChange={(e) =>
                    setForm({ ...form, compareMode: e.target.value })
                  }
                >
                  <option value="item_set">item_set（按提取条目比对）</option>
                  <option value="raw_digest">raw_digest（整页指纹）</option>
                  <option value="text_sim">text_sim（文本相似度）</option>
                </select>
              </Field>
              <Field label="稳定字段 (stable_id)">
                <input
                  className={inputCls}
                  value={form.stable_id}
                  placeholder="id"
                  onChange={(e) =>
                    setForm({ ...form, stable_id: e.target.value })
                  }
                />
              </Field>

              <Field label="通知事件 (notify_on)" className="col-span-2">
                <input
                  className={inputCls}
                  value={form.notify_on}
                  placeholder="new,updated,removed"
                  onChange={(e) =>
                    setForm({ ...form, notify_on: e.target.value })
                  }
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
                      未提取到条目（可能未配置 extract 规则，或页面为空）。
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
