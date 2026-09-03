import { useCallback, useEffect, useMemo, useState } from "react"
import { Loader2, Plus, RefreshCw } from "lucide-react"
import { api, type ChangeEvent, type SourceMeta, type TagConfig, type TestSourceResult } from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { ErrorText, EmptyState, FilterPills, Loading } from "@/components/ui/feedback"
import {
  type FormState,
  formToSource,
  validateForm,
} from "@/lib/source-form"
import { SourceCard } from "./source-card"
import {
  SourceFormDialog,
  forcingGroup,
  initialForm,
} from "./source-form-dialog"
import { TestResultDialog } from "./test-result-dialog"

/** 「无标签」筛选用的哨兵值（与真实标签名区分）。 */
const UNTAGGED = "__untagged__"

export function SourcesPage() {
  const [sources, setSources] = useState<SourceMeta[]>([])
  const [tags, setTags] = useState<TagConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  // 多选（批量启停监控 / 通知）
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [batchBusy, setBatchBusy] = useState(false)

  const [tagFilter, setTagFilter] = useState<string | null>(null)
  const [statusFilter, setStatusFilter] = useState<"all" | "unread" | "error">("all")

  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [historyMap, setHistoryMap] = useState<Record<string, ChangeEvent[]>>({})
  const [historyLoading, setHistoryLoading] = useState<Set<string>>(new Set())

  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<SourceMeta | null>(null)
  const [form, setForm] = useState<FormState>(initialForm(null))
  const [saving, setSaving] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  const [testResult, setTestResult] = useState<TestSourceResult | null>(null)
  const [testOpen, setTestOpen] = useState(false)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testError, setTestError] = useState<string | null>(null)

  const load = useCallback(async () => {
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
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const allTags = useMemo(
    () => Array.from(new Set(sources.flatMap((s) => s.tags ?? []))).sort(),
    [sources],
  )

  const visibleSources = useMemo(
    () => sources.filter((s) => matchesTag(s, tagFilter) && matchesStatus(s, statusFilter)),
    [sources, tagFilter, statusFilter],
  )

  const hasUntagged = useMemo(
    () => sources.some((s) => (s.tags ?? []).length === 0),
    [sources],
  )

  // 切换标签筛选时清理选中项，避免批量操作作用于已隐藏的源。
  function applyTagFilter(tag: string | null) {
    setTagFilter(tag)
    setSelected((prev) => {
      const next = new Set(prev)
      for (const s of sources) {
        if (!matchesTag(s, tag)) next.delete(s.id)
      }
      return next
    })
  }

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
    setForm(initialForm(null))
    setFormError(null)
    setModalOpen(true)
  }

  function openEdit(s: SourceMeta) {
    setEditing(s)
    setForm(initialForm(s))
    setFormError(null)
    setModalOpen(true)
  }

  async function handleSave() {
    const groupForces = forcingGroup(form, tags)
    const err = validateForm(form, groupForces, validateCron, !!editing)
    if (err) {
      setFormError(err)
      return
    }
    setSaving(true)
    setFormError(null)
    try {
      let effectiveForm = form
      // 新增且名称留空时，抓取网页标题作为名称。
      if (!editing && !form.name.trim()) {
        try {
          const res = await api.previewSource(form.url.trim(), form.engine)
          if (res.title) effectiveForm = { ...form, name: res.title }
        } catch {
          // 抓取标题失败不阻塞保存；名称留空时后端回退到 id。
        }
      }
      if (editing) {
        await api.updateSource(editing.id, formToSource(effectiveForm, editing))
        // 编辑后清空该源的历史缓存，避免展示旧数据。
        setHistoryMap((prev) => {
          const next = { ...prev }
          delete next[editing.id]
          return next
        })
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

  function toggleSelect(id: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (!next.delete(id)) next.add(id)
      return next
    })
  }

  async function runBatch(flags: { enabled?: boolean; notify_enabled?: boolean }) {
    if (selected.size === 0) return
    setBatchBusy(true)
    setError(null)
    try {
      await api.batchSetFlags([...selected], flags)
      await load()
      setSelected(new Set())
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBatchBusy(false)
    }
  }

  async function toggleHistory(id: string) {
    const willExpand = !expanded.has(id)
    setExpanded((prev) => {
      const next = new Set(prev)
      if (!next.delete(id)) next.add(id)
      return next
    })
    // 展开时把该源的未读变更标记为已读。
    if (willExpand) {
      try {
        await api.markSourceRead(id)
      } catch {
        // 标记已读失败不阻塞展开
      }
      await load()
    }
    if (willExpand && !historyMap[id]) {
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

  async function handleMarkSourceRead(s: SourceMeta) {
    try {
      await api.markSourceRead(s.id)
      await load()
    } catch (e) {
      setError((e as Error).message)
    }
  }

  if (loading) return <Loading text="加载监控源…" />

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <span className="text-sm text-muted-foreground">共 {sources.length} 个监控源</span>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load}>
            <RefreshCw className="h-4 w-4" /> 刷新
          </Button>
          <Button size="sm" onClick={openAdd}>
            <Plus className="h-4 w-4" /> 添加监控源
          </Button>
        </div>
      </div>

      {selected.size > 0 && (
        <div className="flex flex-wrap items-center gap-2 rounded-md border bg-muted/40 px-3 py-2">
          <span className="text-sm font-medium">已选 {selected.size} 个</span>
          <div className="flex-1" />
          <BatchButton busy={batchBusy} onClick={() => runBatch({ enabled: false })}>
            暂停监控
          </BatchButton>
          <BatchButton busy={batchBusy} onClick={() => runBatch({ enabled: true })}>
            恢复监控
          </BatchButton>
          <BatchButton busy={batchBusy} onClick={() => runBatch({ notify_enabled: false })}>
            暂停通知
          </BatchButton>
          <BatchButton busy={batchBusy} onClick={() => runBatch({ notify_enabled: true })}>
            恢复通知
          </BatchButton>
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

      {error && <ErrorText>加载失败：{error}</ErrorText>}

      {sources.length === 0 ? (
        <EmptyState>暂无监控源。点击右上角「添加监控源」创建。</EmptyState>
      ) : (
        <div className="grid gap-4">
          <label className="flex cursor-pointer items-center gap-2 px-1 text-sm text-muted-foreground">
            <input
              type="checkbox"
              className="h-4 w-4 accent-primary"
              checked={selected.size > 0 && selected.size === visibleSources.length}
              ref={(el) => {
                if (el) {
                  el.indeterminate =
                    selected.size > 0 && selected.size < visibleSources.length
                }
              }}
              onChange={(e) =>
                setSelected(
                  e.target.checked
                    ? new Set(visibleSources.map((s) => s.id))
                    : new Set(),
                )
              }
            />
            全选
          </label>

          <div className="flex flex-wrap items-center gap-2 px-1">
            <FilterPills
              label="标签"
              value={tagFilter}
              onChange={applyTagFilter}
              options={[
                { value: null, label: "全部" },
                ...allTags.map((t) => ({ value: t, label: t })),
                ...(hasUntagged ? [{ value: UNTAGGED, label: "无标签" }] : []),
              ]}
            />
            <FilterPills
              label="状态"
              value={statusFilter}
              onChange={setStatusFilter}
              options={[
                { value: "all" as const, label: "全部" },
                { value: "unread" as const, label: "未读" },
                { value: "error" as const, label: "错误" },
              ]}
            />
          </div>

          {visibleSources.length === 0 ? (
            <p className="px-1 text-sm text-muted-foreground">
              没有符合筛选条件的监控源。
            </p>
          ) : (
            visibleSources.map((s) => (
              <SourceCard
                key={s.id}
                source={s}
                selected={selected.has(s.id)}
                busy={busyId === s.id}
                testing={testingId === s.id}
                expanded={expanded.has(s.id)}
                history={historyMap[s.id]}
                historyLoading={historyLoading.has(s.id)}
                onToggleSelect={() => toggleSelect(s.id)}
                onToggleHistory={() => toggleHistory(s.id)}
                onMarkRead={() => handleMarkSourceRead(s)}
                onCheck={() => runCheck(s.id)}
                onTest={() => handleTest(s)}
                onEdit={() => openEdit(s)}
                onDelete={() => handleDelete(s)}
              />
            ))
          )}
        </div>
      )}

      <SourceFormDialog
        open={modalOpen}
        editing={editing}
        form={form}
        tags={tags}
        saving={saving}
        error={formError}
        onChange={(patch) => setForm((prev) => ({ ...prev, ...patch }))}
        onClose={() => setModalOpen(false)}
        onSave={handleSave}
      />

      <TestResultDialog
        open={testOpen}
        result={testResult}
        error={testError}
        testing={testingId !== null}
        onClose={() => setTestOpen(false)}
      />
    </div>
  )
}

function BatchButton({
  busy,
  onClick,
  children,
}: {
  busy: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Button size="sm" variant="outline" disabled={busy} onClick={onClick}>
      {busy && <Loader2 className="h-4 w-4 animate-spin" />}
      {children}
    </Button>
  )
}

function matchesTag(s: SourceMeta, tag: string | null): boolean {
  if (tag === null) return true
  const tags = s.tags ?? []
  return tag === UNTAGGED ? tags.length === 0 : tags.includes(tag)
}

function matchesStatus(s: SourceMeta, status: "all" | "unread" | "error"): boolean {
  if (status === "unread") return (s.unread_count ?? 0) > 0
  if (status === "error") return s.has_error
  return true
}
