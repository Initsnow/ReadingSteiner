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
  }
}

function sourceToForm(s: SourceConfig): FormState {
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
  }
}

function formToSource(f: FormState, existing?: SourceConfig): SourceConfig {
  const base = existing ?? ({} as SourceConfig)
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
    compare: {
      ...base.compare,
      mode: f.compareMode,
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

  async function handleSave() {
    if (!form.id.trim() || !form.url.trim()) {
      setFormError("id 和 url 为必填项")
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

              <Field label="流水线 (pipeline)">
                <input
                  className={inputCls}
                  value={form.pipeline}
                  onChange={(e) => setForm({ ...form, pipeline: e.target.value })}
                />
              </Field>
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
                  onChange={(e) =>
                    setForm({ ...form, compareMode: e.target.value })
                  }
                >
                  <option value="item_set">item_set</option>
                  <option value="raw_digest">raw_digest</option>
                  <option value="text_sim">text_sim</option>
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
