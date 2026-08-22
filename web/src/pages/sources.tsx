import { useEffect, useMemo, useState } from "react"
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
  ChevronDown,
  ChevronRight,
  Eye,
  History as HistoryIcon,
} from "lucide-react"
import {
  api,
  type SourceConfig,
  type SourceMeta,
  type ChangeEvent,
  type TestSourceResult,
  type ExtractConfig,
  type ItemField,
  type ImageSelector,
  type TagConfig,
} from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription } from "@/components/ui/card"

// ---- editable field model for the add/edit form ----
interface FormState {
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
  // 内容提取
  extractType: "text" | "items"
  selectorKind: "css" | "json_path"
  selector: string
  fieldsJson: string
  // 图片选择器
  imageKind: "none" | "items" | "css" | "changed"
  imageSelector: string
  // camofox 截图
  screenshot: boolean
}

function emptyForm(): FormState {
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
    follow_group: s.follow_group ?? true,
    url: s.fetch.url,
    engine: s.fetch.engine,
    method: s.fetch.method,
    cron: s.schedule.cron ?? "",
    cron_follow_global: !s.schedule.cron,
    timeout_secs: s.fetch.timeout_secs ?? 0,
    tags: (s.tags ?? []).join(","),
    ...ex,
    screenshot: s.fetch.screenshot ?? false,
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
    name: f.name.trim(),
    enabled: f.enabled,
    notify_enabled: f.notify_enabled,
    follow_group: f.follow_group,
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
      screenshot: f.screenshot,
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

function formatRelativeTime(ts: string | null): string {
  if (!ts) return "—"
  const d = new Date(ts)
  if (isNaN(d.getTime())) return "—"
  const diff = Date.now() - d.getTime()
  if (diff < 0) return "刚刚"
  const sec = Math.floor(diff / 1000)
  if (sec < 60) return `${sec} 秒前`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min} 分钟前`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr} 小时前`
  const day = Math.floor(hr / 24)
  if (day < 30) return `${day} 天前`
  return d.toLocaleDateString()
}

function formatDateTime(ts: string | null): string {
  if (!ts) return "—"
  const d = new Date(ts)
  if (isNaN(d.getTime())) return "—"
  return d.toLocaleString()
}

function parseChangeEvent(e: ChangeEvent) {
  const oldItems = safeJsonParse<any[]>(e.old_items_json) ?? []
  const newItems = safeJsonParse<any[]>(e.new_items_json) ?? []
  return { oldItems, newItems }
}

export function SourcesPage() {
  const [sources, setSources] = useState<SourceMeta[]>([])
  // 分组（标签）配置，用于判断源“跟随分组”时继承的分组内容提取等设置。
  const [tags, setTags] = useState<TagConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  // 多选：选中的监控源 id 集合（用于批量暂停监控 / 暂停通知）。
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [batchBusy, setBatchBusy] = useState(false)

  // 按 tag 筛选：null 表示全部，"__untagged__" 表示无标签的源。
  const [tagFilter, setTagFilter] = useState<string | null>(null)
  // 附加状态筛选："all" | "unread" | "error"
  const [statusFilter, setStatusFilter] = useState<"all" | "unread" | "error">("all")

  const allTags = useMemo(
    () => Array.from(new Set(sources.flatMap((s) => s.tags ?? []))).sort(),
    [sources],
  )

  const filteredSources = useMemo(() => {
    let list = sources.filter((s) => {
      const tags = s.tags ?? []
      if (tagFilter === null) return true
      if (tagFilter === "__untagged__") return tags.length === 0
      return tags.includes(tagFilter)
    })
    if (statusFilter === "unread") {
      list = list.filter((s) => (s.unread_count ?? 0) > 0)
    } else if (statusFilter === "error") {
      list = list.filter((s) => s.has_error)
    }
    return list
  }, [sources, tagFilter, statusFilter])

  const hasUntagged = useMemo(
    () => sources.some((s) => (s.tags ?? []).length === 0),
    [sources],
  )

  // 展开历史记录的源 id 集合
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  // 每个源的历史事件缓存
  const [historyMap, setHistoryMap] = useState<Record<string, ChangeEvent[]>>({})
  const [historyLoading, setHistoryLoading] = useState<Set<string>>(new Set())

  // 切换标签筛选时同步清理 selected，避免批量操作作用于不可见的源。
  function applyTagFilter(tag: string | null) {
    setTagFilter(tag)
    setSelected((prev) => {
      const next = new Set(prev)
      sources.forEach((s) => {
        const tags = s.tags ?? []
        const match =
          tag === null
            ? true
            : tag === "__untagged__"
              ? tags.length === 0
              : tags.includes(tag)
        if (!match) next.delete(s.id)
      })
      return next
    })
  }

  // add/edit modal state
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<SourceMeta | null>(null)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [saving, setSaving] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  // test result modal state
  const [testResult, setTestResult] = useState<TestSourceResult | null>(null)
  const [testOpen, setTestOpen] = useState(false)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testError, setTestError] = useState<string | null>(null)

  // 当前表单所属分组（编辑时按标签解析），用于判断“跟随分组”时继承的提取设置。
  const formTagNames = useMemo(
    () =>
      form.tags
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean),
    [form.tags],
  )
  const formGroups = useMemo(
    () => tags.filter((t) => formTagNames.includes(t.name)),
    [tags, formTagNames],
  )
  // 跟随分组时，源自身的“内容提取”是否会被分组的提取配置覆盖。
  // 需与后端 resolve_effective_extract 保持一致：取源跟随的、配置了提取的
  // 分组中按名称升序的第一个分组作为生效提取；仅当该分组是结构化提取(items)时
  // 源自身的提取设置才会被禁用。若生效分组是 text 或未配置提取，源仍可用自己的设置。
  const groupForcesItemsExtract = useMemo(() => {
    if (!form.follow_group) return null
    const withExtract = [...formGroups]
      .filter((t) => t.extract)
      .sort((a, b) => a.name.localeCompare(b.name))
    const effective = withExtract[0]
    if (!effective || effective.extract?.type !== "items") return null
    return effective.name
  }, [form.follow_group, formGroups])

  async function load() {
    try {
      const [srcs, tagList] = await Promise.all([api.listSources(), api.listTags()])
      setSources(srcs)
      setTags(tagList)
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

  function openEdit(s: SourceMeta) {
    setEditing(s)
    setForm(sourceToForm(s))
    setFormError(null)
    setModalOpen(true)
  }

  // 添加监控源时，若名称留空则自动从 URL 抓取 title 填入名称。
  async function handleSave() {
    if (!form.url.trim()) {
      setFormError("url 为必填项")
      return
    }
    // 未跟随全局时 cron 必须填写，避免 UI 显示“自定义”而实际静默回退为“跟随全局”。
    if (!form.cron_follow_global && !form.cron.trim()) {
      setFormError("取消“跟随全局”后，需要填写 cron 表达式")
      return
    }
    // 自定义 cron 时做前端格式校验（跟随全局时使用全局默认值，由设置页负责校验）。
    const cronErr = form.cron_follow_global ? null : validateCron(form.cron)
    if (cronErr) {
      setFormError(cronErr)
      return
    }
    // 分组强制结构化提取时，源自身的提取配置由分组接管，跳过源级校验。
    if (!groupForcesItemsExtract && form.extractType === "items" && !form.selector.trim()) {
      setFormError("结构化提取需要填写选择器")
      return
    }
    if (!groupForcesItemsExtract && form.extractType === "items" && form.fieldsJson.trim()) {
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
      let effectiveForm = form
      // 新增且名称留空时，自动抓取 URL 的 title 作为名称
      if (!editing && !form.name.trim()) {
        try {
          const res = await api.previewSource(form.url.trim(), form.engine)
          if (res.title) {
            effectiveForm = { ...form, name: res.title }
          }
        } catch {
          // 抓取标题失败不阻塞保存，名称留空则由后端回退到 id。
        }
      }

      if (editing) {
        await api.updateSource(editing.id, formToSource(effectiveForm, editing))
      } else {
        await api.addSource(formToSource(effectiveForm))
      }
      setModalOpen(false)
      await load()
    } catch (e) {
      setFormError((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete(s: SourceMeta) {
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

  async function handleTest(s: SourceMeta) {
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

  // ---- 历史记录展开 ----
  async function toggleHistory(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
    // 展开变更历史时自动把该监控源的未读变更标记为已读。
    const wasExpanded = expanded.has(id)
    if (!wasExpanded) {
      try {
        await api.markSourceRead(id)
      } catch {
        // 标记已读失败不阻塞历史展开
      }
      await load()
    }
    if (!historyMap[id]) {
      setHistoryLoading((prev) => new Set(prev).add(id))
      try {
        const events = await api.history(id, 20)
        setHistoryMap((prev) => ({ ...prev, [id]: events }))
      } catch {
        setError("加载历史记录失败")
      } finally {
        setHistoryLoading((prev) => {
          const next = new Set(prev)
          next.delete(id)
          return next
        })
      }
    }
  }

  // ---- 标记已读 ----
  async function handleMarkSourceRead(s: SourceMeta) {
    try {
      await api.markSourceRead(s.id)
      await load()
    } catch (e) {
      setError((e as Error).message)
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
                checked={selected.size > 0 && selected.size === filteredSources.length}
                ref={(el) => {
                  if (el) {
                    el.indeterminate =
                      selected.size > 0 && selected.size < filteredSources.length
                  }
                }}
                onChange={(e) =>
                  setSelected(e.target.checked ? new Set(filteredSources.map((s) => s.id)) : new Set())
                }
              />
              全选
            </label>
          </div>

          {/* 标签与状态筛选栏 */}
          <div className="flex flex-wrap items-center gap-2 px-1">
            <span className="text-xs text-muted-foreground">标签：</span>
            <button
              onClick={() => applyTagFilter(null)}
              className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
                tagFilter === null
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted text-muted-foreground hover:bg-muted/70"
              }`}
            >
              全部
            </button>
            {allTags.map((t) => (
              <button
                key={t}
                onClick={() => applyTagFilter(tagFilter === t ? null : t)}
                className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
                  tagFilter === t
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/70"
                }`}
              >
                {t}
              </button>
            ))}
            {hasUntagged && (
              <button
                onClick={() =>
                  applyTagFilter(
                    tagFilter === "__untagged__" ? null : "__untagged__",
                  )
                }
                className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
                  tagFilter === "__untagged__"
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/70"
                }`}
              >
                无标签
              </button>
            )}

            <span className="ml-2 text-xs text-muted-foreground">状态：</span>
            {(["all", "unread", "error"] as const).map((f) => (
              <button
                key={f}
                onClick={() => setStatusFilter(f)}
                className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
                  statusFilter === f
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/70"
                }`}
              >
                {f === "all" ? "全部" : f === "unread" ? "未读" : "错误"}
              </button>
            ))}
          </div>

          {filteredSources.length === 0 ? (
            <p className="px-1 text-sm text-muted-foreground">
              没有符合筛选条件的监控源。
            </p>
          ) : (
            filteredSources.map((s) => {
              const isExpanded = expanded.has(s.id)
              const events = historyMap[s.id]
              const isHistLoading = historyLoading.has(s.id)
              const isCamofox = s.fetch.engine === "camofox"
              const unread = s.unread_count ?? 0
              return (
                <Card
                  key={s.id}
                  className={
                    selected.has(s.id)
                      ? "ring-2 ring-primary/60"
                      : undefined
                  }
                >
                  <CardContent className="px-4 py-3">
                    {/* 主行 */}
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                      <input
                        type="checkbox"
                        className="h-4 w-4 shrink-0 accent-primary"
                        checked={selected.has(s.id)}
                        onChange={() => toggleSelect(s.id)}
                      />
                      <button
                        className="shrink-0 text-muted-foreground hover:text-foreground"
                        onClick={() => toggleHistory(s.id)}
                        title={isExpanded ? "收起历史" : "展开历史"}
                      >
                        {isHistLoading ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : isExpanded ? (
                          <ChevronDown className="h-4 w-4" />
                        ) : (
                          <ChevronRight className="h-4 w-4" />
                        )}
                      </button>
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="truncate text-sm font-medium">
                            {s.name || s.id}
                          </span>
                          {unread > 0 && (
                            <Badge variant="warning" className="px-1.5 py-0 text-[10px]">
                              {unread} 未读
                            </Badge>
                          )}
                          {s.has_error && (
                            <Badge
                              variant="destructive"
                              className="px-1.5 py-0 text-[10px]"
                              title={s.last_error ?? "连续失败，请检查监控源配置或网络"}
                            >
                              <AlertCircle className="h-2.5 w-2.5" /> 错误
                            </Badge>
                          )}
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
                          {isCamofox && s.fetch.screenshot && (
                            <Badge variant="outline" className="px-1.5 py-0 text-[10px]">
                              <Eye className="h-2.5 w-2.5" /> 截图
                            </Badge>
                          )}
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
                        <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                          <span className="truncate">{s.fetch.url}</span>
                          <span className="shrink-0">
                            最近检查: <span title={formatDateTime(s.last_check_at)}>{formatRelativeTime(s.last_check_at)}</span>
                          </span>
                          <span className="shrink-0">
                            最近变更: <span title={formatDateTime(s.last_change_at)}>{formatRelativeTime(s.last_change_at)}</span>
                          </span>
                        </div>
                        {s.has_error && s.last_error && (
                          <div className="mt-1 flex items-start gap-1 rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-xs text-destructive">
                            <AlertCircle className="mt-0.5 h-3 w-3 shrink-0" />
                            <span className="break-all">{s.last_error}</span>
                          </div>
                        )}
                      </div>
                      <div className="flex shrink-0 items-center gap-1">
                        {unread > 0 && (
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-7 px-2 text-xs"
                            onClick={() => handleMarkSourceRead(s)}
                            title="标记已读"
                          >
                            <Eye className="h-3.5 w-3.5" /> 已读
                          </Button>
                        )}
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
                    </div>

                    {/* 历史记录展开区 */}
                    {isExpanded && (
                      <div className="mt-3 border-t pt-3">
                        <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
                          <HistoryIcon className="h-3.5 w-3.5" /> 变更历史
                          {events && (
                            <span className="text-[10px] text-muted-foreground">
                              （共 {events.length} 条）
                            </span>
                          )}
                        </div>
                        {isHistLoading ? (
                          <div className="flex items-center gap-2 py-4 text-xs text-muted-foreground">
                            <Loader2 className="h-3.5 w-3.5 animate-spin" /> 加载历史…
                          </div>
                        ) : events && events.length > 0 ? (
                          <div className="space-y-2">
                            {events.map((ev) => (
                              <EventRow key={ev.id} event={ev} />
                            ))}
                          </div>
                        ) : (
                          <p className="py-2 text-xs text-muted-foreground">
                            暂无变更记录。
                          </p>
                        )}
                      </div>
                    )}
                  </CardContent>
                </Card>
              )
            })
          )}
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

              <Field
                label="名称"
                className="col-span-2"
                hint={!editing ? "留空会自动抓取网页标题" : undefined}
              >
                <input
                  className={inputCls}
                  value={form.name}
                  placeholder="留空自动获取网页标题"
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </Field>

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

              {/* 跟随分组：仅编辑已有源且源有分组归属时有意义；新建源无分组，隐藏以免误设。 */}
              {editing && formTagNames.length > 0 && (
                <div className="col-span-2 flex items-center justify-between rounded-md border bg-muted/30 p-3">
                  <div className="text-sm">
                    <div className="font-medium">跟随分组设置</div>
                    <div className="mt-0.5 text-xs text-muted-foreground">
                      开启后继承分组的历史保留 / 通知目标 / 内容提取设置
                    </div>
                  </div>
                  <label className="flex cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-primary"
                      checked={form.follow_group}
                      onChange={(e) =>
                        setForm({ ...form, follow_group: e.target.checked })
                      }
                    />
                    <span>跟随分组</span>
                  </label>
                </div>
              )}

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

              {/* camofox 截图开关（仅 camofox 引擎生效） */}
              {form.engine === "camofox" && (
                <Field label="camofox 截图" className="col-span-2">
                  <div className="rounded-md border bg-muted/30 p-3">
                    <label className="flex cursor-pointer items-center gap-2 text-sm">
                      <input
                        type="checkbox"
                        className="h-4 w-4 accent-primary"
                        checked={form.screenshot}
                        onChange={(e) =>
                          setForm({ ...form, screenshot: e.target.checked })
                        }
                      />
                      <span>检测到变更时截图（可在历史记录中查看）</span>
                    </label>
                  </div>
                </Field>
              )}

              <Field
                label="cron 表达式"
                className="col-span-2"
                hint="例：*/15 * * * * 每 15 分钟、0 9,18 * * 1-5 工作日 9:00/18:00"
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

              <Field
                label="内容提取"
                className="col-span-2"
                hint={
                  groupForcesItemsExtract
                    ? `由分组「${groupForcesItemsExtract}」提供结构化提取，此处设置被覆盖。关闭“跟随分组”后可用。`
                    : undefined
                }
              >
                <select
                  className={inputCls}
                  value={groupForcesItemsExtract ? "items" : form.extractType}
                  disabled={!!groupForcesItemsExtract}
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

              {form.extractType === "items" && !groupForcesItemsExtract && (
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
                  <Field label={`提取字段（可选 JSON 数组）`}>
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
              <Field label="标签">
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

// ---- 单条变更事件行 ----
function EventRow({ event }: { event: ChangeEvent }) {
  const [read, setRead] = useState(event.read)
  const [showDiff, setShowDiff] = useState(false)
  const { oldItems, newItems } = parseChangeEvent(event)
  const hasScreenshot = !!event.screenshot_path

  async function markRead() {
    if (read) return
    try {
      await api.markEventRead(event.id)
      setRead(true)
    } catch {
      // ignore
    }
  }

  return (
    <div
      className={`rounded-md border p-2 text-xs ${read ? "bg-background/40" : "bg-primary/5"}`}
      onClick={markRead}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-muted-foreground">#{event.id}</span>
        <Badge
          variant={
            event.change_type === "new"
              ? "success"
              : event.change_type === "updated"
                ? "warning"
                : "destructive"
          }
          className="px-1.5 py-0 text-[10px]"
        >
          {event.change_type}
        </Badge>
        {!read && (
          <span className="text-[10px] text-primary">● 未读</span>
        )}
        <span className="text-muted-foreground">
          {new Date(event.detected_at).toLocaleString()}
        </span>
        <div className="flex-1" />
        <button
          className="flex items-center gap-1 text-muted-foreground hover:text-foreground"
          onClick={(e) => {
            e.stopPropagation()
            setShowDiff((v) => !v)
          }}
        >
          {showDiff ? (
            <ChevronDown className="h-3 w-3" />
          ) : (
            <ChevronRight className="h-3 w-3" />
          )}
          查看变更
        </button>
      </div>
      {event.diff_summary && (
        <p className="mt-1 text-muted-foreground">{event.diff_summary}</p>
      )}

      {hasScreenshot && (
        <div className="mt-2">
          <img
            src={api.eventScreenshotUrl(event.id)}
            alt={`截图 #${event.id}`}
            className="max-h-48 rounded-md border object-contain"
            loading="lazy"
          />
        </div>
      )}

      {showDiff && (
        <div className="mt-2 grid gap-2 md:grid-cols-2">
          <div className="rounded bg-muted p-2">
            <div className="mb-1 font-medium text-muted-foreground">旧 ({oldItems.length} 条)</div>
            {oldItems.length > 0 ? (
              <pre className="max-h-40 overflow-auto font-mono text-[10px]">
                {oldItems.map((it) => it.text ?? JSON.stringify(it)).join("\n\n")}
              </pre>
            ) : (
              <span className="text-muted-foreground">—</span>
            )}
          </div>
          <div className="rounded bg-muted p-2">
            <div className="mb-1 font-medium text-muted-foreground">新 ({newItems.length} 条)</div>
            {newItems.length > 0 ? (
              <pre className="max-h-40 overflow-auto font-mono text-[10px]">
                {newItems.map((it) => it.text ?? JSON.stringify(it)).join("\n\n")}
              </pre>
            ) : (
              <span className="text-muted-foreground">—</span>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
