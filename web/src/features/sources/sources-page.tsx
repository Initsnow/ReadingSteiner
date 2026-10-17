import { useCallback, useEffect, useMemo, useState } from "react"
import {
  Clock,
  ExternalLink,
  LayoutGrid,
  List,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react"
import {
  api,
  type GroupConfig,
  type SourceMeta,
} from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { ErrorText, EmptyState, Loading } from "@/components/ui/feedback"
import {
  type FormState,
  formToSource,
  validateForm,
} from "@/lib/source-form"
import {
  SourceFormDialog,
  initialForm,
} from "./source-form-dialog"

export function SourcesPage() {
  const [sources, setSources] = useState<SourceMeta[]>([])
  const [groups, setGroups] = useState<GroupConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  // 视图模式：Table (高密度表格) vs Cards (卡片)
  const [viewMode, setViewMode] = useState<"table" | "cards">("table")

  // 多选与批量操作
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [batchBusy, setBatchBusy] = useState(false)

  // 筛选器
  const [search, setSearch] = useState("")
  const [groupFilter, setGroupFilter] = useState<string | null>(null)
  const [statusFilter, setStatusFilter] = useState<
    "all" | "healthy" | "error" | "disabled"
  >("all")

  // 编辑 / 新建模态框
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<SourceMeta | null>(null)
  const [form, setForm] = useState<FormState>(initialForm(null))
  const [saving, setSaving] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)


  const load = useCallback(async () => {
    try {
      setLoading(true)
      const [srcs, grps] = await Promise.all([
        api.listSources(),
        api.listGroups(),
      ])
      setSources(srcs)
      setGroups(grps)
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

  // 过滤后的源
  const filteredSources = useMemo(() => {
    return sources.filter((s) => {
      // 搜索
      if (
        search.trim() &&
        !s.name.toLowerCase().includes(search.toLowerCase()) &&
        !s.id.toLowerCase().includes(search.toLowerCase()) &&
        !s.fetch.url.toLowerCase().includes(search.toLowerCase())
      ) {
        return false
      }
      // 分组
      if (groupFilter !== null) {
        if (groupFilter === "__none__") {
          if (s.group_id) return false
        } else if (s.group_id !== groupFilter) {
          return false
        }
      }
      // 状态
      if (statusFilter === "disabled" && s.enabled) return false
      if (statusFilter === "healthy" && (!s.enabled || s.has_error)) return false
      if (statusFilter === "error" && (!s.enabled || !s.has_error)) return false

      return true
    })
  }, [sources, search, groupFilter, statusFilter])

  // 单源立即检测
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

  // 批量操作
  async function handleBatch(enable: boolean) {
    if (selected.size === 0) return
    setBatchBusy(true)
    try {
      await api.batchSetFlags(Array.from(selected), { enabled: enable })
      await load()
      setSelected(new Set())
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBatchBusy(false)
    }
  }

  async function handleBatchCheck() {
    if (selected.size === 0) return
    setBatchBusy(true)
    try {
      for (const id of selected) {
        await api.check(id)
      }
      await load()
      setSelected(new Set())
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBatchBusy(false)
    }
  }

  function toggleSelectAll() {
    if (selected.size === filteredSources.length) {
      setSelected(new Set())
    } else {
      setSelected(new Set(filteredSources.map((s) => s.id)))
    }
  }

  function toggleSelect(id: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  function openAdd() {
    setEditing(null)
    setForm(initialForm(null))
    setFormError(null)
    setModalOpen(true)
  }

  useEffect(() => {
    if (window.location.search.includes("new=1") || window.location.hash === "#new") {
      openAdd()
    }
  }, [])

  function openEdit(s: SourceMeta) {
    setEditing(s)
    setForm(initialForm(s))
    setFormError(null)
    setModalOpen(true)
  }

  async function handleDelete(id: string) {
    if (!confirm(`确定要删除监控源「${id}」吗？`)) return
    try {
      await api.deleteSource(id)
      await load()
    } catch (e) {
      alert(`删除失败: ${(e as Error).message}`)
    }
  }

  async function handleSave() {
    const err = validateForm(form, validateCron, !!editing)
    if (err) {
      setFormError(err)
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

  return (
    <div className="space-y-6 max-w-7xl">
      {/* 标题栏与快捷操作 */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-zinc-100 flex items-center gap-2">
            监控源管理
          </h1>
          <p className="text-sm text-zinc-400">
            配置与调度目标 HTTP、JSON 与网页监控项，支持实时抓取测试、多步规则与分组继承。
          </p>
        </div>

        <div className="flex items-center gap-2.5">
          <Button
            variant="outline"
            size="sm"
            onClick={load}
            className="gap-2 h-9 text-xs border-zinc-800 bg-zinc-900"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            刷新
          </Button>

          <Button onClick={openAdd} className="gap-2 h-9 text-xs">
            <Plus className="h-4 w-4" />
            新建监控源
          </Button>
        </div>
      </div>

      {/* 搜索与工具栏 */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-zinc-800 bg-zinc-900/60 p-3 text-xs">
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative">
            <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-zinc-400" />
            <input
              type="text"
              placeholder="搜索名称 / ID / URL..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="h-8 w-60 rounded border border-zinc-800 bg-zinc-950 pl-8 pr-2.5 text-xs text-zinc-200 placeholder:text-zinc-500 font-mono focus:outline-none focus:ring-1 focus:ring-zinc-600"
            />
          </div>

          {/* 分组筛选 */}
          <select
            value={groupFilter ?? ""}
            onChange={(e) =>
              setGroupFilter(e.target.value ? e.target.value : null)
            }
            className="h-8 rounded border border-zinc-800 bg-zinc-950 px-2.5 text-xs text-zinc-300 focus:outline-none"
          >
            <option value="">全部分组 ({sources.length})</option>
            <option value="__none__">
              未分组 ({sources.filter((s) => !s.group_id).length})
            </option>
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name} ({sources.filter((s) => s.group_id === g.id).length})
              </option>
            ))}
          </select>

          {/* 状态筛选 */}
          <div className="flex items-center rounded border border-zinc-800 bg-zinc-950 p-0.5">
            {(
              [
                ["all", "全部"],
                ["healthy", "正常"],
                ["error", "异常"],
                ["disabled", "已禁用"],
              ] as const
            ).map(([val, label]) => (
              <button
                key={val}
                type="button"
                onClick={() => setStatusFilter(val)}
                className={`px-2.5 py-1 rounded text-[11px] transition-colors ${
                  statusFilter === val
                    ? "bg-zinc-800 text-zinc-100 font-semibold shadow-xs"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {/* 视图模式切换与批量控制 */}
        <div className="flex items-center gap-2">
          {selected.size > 0 && (
            <div className="flex items-center gap-1.5 border-r border-zinc-800 pr-3 mr-1">
              <span className="text-[11px] text-zinc-400 font-mono">
                已选 {selected.size} 项:
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={() => handleBatch(true)}
                disabled={batchBusy}
                className="h-7 text-[11px] border-zinc-800 bg-zinc-900"
              >
                启用
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => handleBatch(false)}
                disabled={batchBusy}
                className="h-7 text-[11px] border-zinc-800 bg-zinc-900"
              >
                禁用
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={handleBatchCheck}
                disabled={batchBusy}
                className="h-7 text-[11px] border-zinc-800 bg-zinc-900 text-primary"
              >
                全部检测
              </Button>
            </div>
          )}

          <div className="flex items-center rounded border border-zinc-800 bg-zinc-950 p-0.5">
            <button
              type="button"
              onClick={() => setViewMode("table")}
              title="表格视图"
              className={`p-1.5 rounded transition-colors ${
                viewMode === "table"
                  ? "bg-zinc-800 text-zinc-100"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <List className="h-4 w-4" />
            </button>
            <button
              type="button"
              onClick={() => setViewMode("cards")}
              title="卡片视图"
              className={`p-1.5 rounded transition-colors ${
                viewMode === "cards"
                  ? "bg-zinc-800 text-zinc-100"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <LayoutGrid className="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      {loading && <Loading />}
      {error && <ErrorText>{error}</ErrorText>}

      {!loading && !error && filteredSources.length === 0 && (
        <EmptyState
          title="未找到匹配的监控源"
          description="当前筛选条件下没有监控源。可以清除筛选条件或点击右上角「新建监控源」。"
        />
      )}

      {/* 高密度数据表格视图 (Table View) */}
      {!loading && viewMode === "table" && filteredSources.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
          <table className="w-full text-left text-xs">
            <thead className="border-b border-zinc-800 bg-zinc-900/80 text-zinc-400 font-mono">
              <tr>
                <th className="w-10 px-3 py-2.5">
                  <input
                    type="checkbox"
                    checked={
                      selected.size === filteredSources.length &&
                      filteredSources.length > 0
                    }
                    onChange={toggleSelectAll}
                    className="rounded border-zinc-700 bg-zinc-950"
                  />
                </th>
                <th className="px-3 py-2.5">监控源 / ID</th>
                <th className="px-3 py-2.5">所属预设</th>
                <th className="px-3 py-2.5">提取模式</th>
                <th className="px-3 py-2.5">调度 Cron</th>
                <th className="px-3 py-2.5">检查 / 变更状态</th>
                <th className="px-3 py-2.5 text-right">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60 font-mono">
              {filteredSources.map((s) => {
                const group = groups.find((g) => g.id === s.group_id)
                const isSelected = selected.has(s.id)

                return (
                  <tr
                    key={s.id}
                    className={`hover:bg-zinc-800/30 transition-colors ${
                      isSelected ? "bg-zinc-800/20" : ""
                    }`}
                  >
                    <td className="px-3 py-2.5">
                      <input
                        type="checkbox"
                        checked={isSelected}
                        onChange={() => toggleSelect(s.id)}
                        className="rounded border-zinc-700 bg-zinc-950"
                      />
                    </td>

                    <td className="px-3 py-2.5 max-w-xs">
                      <div className="flex items-center gap-2">
                        <span
                          className={`h-2 w-2 rounded-full shrink-0 ${
                            !s.enabled
                              ? "bg-zinc-600"
                              : s.has_error
                                ? "bg-rose-500"
                                : "bg-emerald-500"
                          }`}
                        />
                        <span className="font-sans font-semibold text-zinc-100 truncate">
                          {s.name || s.id}
                        </span>
                        {s.unread_count > 0 && (
                          <Badge
                            variant="secondary"
                            className="bg-primary/20 text-primary text-[10px] px-1.5 py-0"
                          >
                            {s.unread_count} 未读
                          </Badge>
                        )}
                      </div>
                      <div className="text-[11px] text-zinc-500 truncate flex items-center gap-1.5 mt-0.5">
                        <span className="text-zinc-400">{s.id}</span>
                        <span>•</span>
                        <a
                          href={s.fetch.url}
                          target="_blank"
                          rel="noreferrer"
                          className="hover:underline flex items-center gap-0.5 truncate text-zinc-500 hover:text-zinc-300"
                        >
                          {s.fetch.url}
                          <ExternalLink className="h-2.5 w-2.5 shrink-0" />
                        </a>
                      </div>
                    </td>

                    <td className="px-3 py-2.5">
                      {group ? (
                        <span className="rounded bg-zinc-800 px-2 py-0.5 text-[11px] text-zinc-300 font-sans">
                          {group.name}
                        </span>
                      ) : (
                        <span className="text-zinc-600 italic font-sans">
                          独立源
                        </span>
                      )}
                    </td>

                    <td className="px-3 py-2.5">
                      <span className="uppercase text-[11px] text-zinc-300">
                        {s.extract.type}
                      </span>
                      {s.fetch.engine === "browser" && (
                        <Badge
                          variant="outline"
                          className="ml-1.5 border-amber-900/60 bg-amber-950/20 text-[10px] text-amber-400 py-0"
                        >
                          CDP
                        </Badge>
                      )}
                    </td>

                    <td className="px-3 py-2.5 text-zinc-300 text-[11px]">
                      {s.schedule.cron || (
                        <span className="text-zinc-500">跟随全局</span>
                      )}
                    </td>

                    <td className="px-3 py-2.5">
                      <div className="space-y-0.5">
                        <div className="flex items-center gap-1.5 text-[11px] text-zinc-400">
                          <Clock className="h-3 w-3 text-zinc-500" />
                          <span>{s.last_check_at || "尚未检查"}</span>
                        </div>
                        {s.has_error && (
                          <div
                            className="text-rose-400 text-[10px] truncate max-w-xs"
                            title={s.last_error || "连续检查失败"}
                          >
                            {s.last_error || "连续失败"}
                          </div>
                        )}
                      </div>
                    </td>

                    <td className="px-3 py-2.5 text-right whitespace-nowrap">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => runCheck(s.id)}
                          disabled={busyId === s.id}
                          className="h-7 text-xs text-zinc-400 hover:text-emerald-400"
                          title="立即检测"
                        >
                          {busyId === s.id ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                          ) : (
                            <Play className="h-3 w-3 fill-current" />
                          )}
                        </Button>

                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => openEdit(s)}
                          className="h-7 text-xs text-zinc-400 hover:text-zinc-200 font-sans"
                        >
                          配置 / 测试
                        </Button>

                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleDelete(s.id)}
                          className="h-7 text-xs text-zinc-500 hover:text-rose-400"
                          title="删除"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* 卡片视图 (Cards View) */}
      {!loading && viewMode === "cards" && filteredSources.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {filteredSources.map((s) => {
            const group = groups.find((g) => g.id === s.group_id)
            return (
              <Card
                key={s.id}
                className="border-zinc-800 bg-zinc-900/50 hover:border-zinc-700/80 transition-all flex flex-col justify-between"
              >
                <div className="p-4 space-y-3">
                  <div className="flex items-start justify-between gap-2">
                    <div>
                      <div className="flex items-center gap-2">
                        <span
                          className={`h-2.5 w-2.5 rounded-full ${
                            !s.enabled
                              ? "bg-zinc-600"
                              : s.has_error
                                ? "bg-rose-500"
                                : "bg-emerald-500"
                          }`}
                        />
                        <span className="font-semibold text-zinc-100 text-sm">
                          {s.name || s.id}
                        </span>
                      </div>
                      <div className="text-[11px] font-mono text-zinc-500 mt-0.5">
                        {s.id}
                      </div>
                    </div>

                    <div className="flex items-center gap-1">
                      {group && (
                        <span className="rounded bg-zinc-800 px-2 py-0.5 text-[11px] text-zinc-300">
                          {group.name}
                        </span>
                      )}
                      <span className="rounded border border-zinc-800 bg-zinc-950 px-1.5 py-0.5 text-[10px] font-mono uppercase text-zinc-400">
                        {s.extract.type}
                      </span>
                    </div>
                  </div>

                  <div className="truncate text-xs font-mono text-zinc-400 flex items-center gap-1">
                    <a
                      href={s.fetch.url}
                      target="_blank"
                      rel="noreferrer"
                      className="hover:underline truncate text-zinc-400 hover:text-zinc-200"
                    >
                      {s.fetch.url}
                    </a>
                  </div>

                  <div className="border-t border-zinc-800/80 pt-2 text-[11px] text-zinc-500 font-mono space-y-1">
                    <div>调度: {s.schedule.cron || "跟随全局"}</div>
                    <div>最近检查: {s.last_check_at || "尚未检查"}</div>
                  </div>
                </div>

                <div className="border-t border-zinc-800 bg-zinc-950/40 p-2 flex items-center justify-between">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => runCheck(s.id)}
                    disabled={busyId === s.id}
                    className="h-7 text-xs text-zinc-400 hover:text-emerald-400 gap-1.5"
                  >
                    {busyId === s.id ? (
                      <Loader2 className="h-3 w-3 animate-spin" />
                    ) : (
                      <Play className="h-3 w-3 fill-current" />
                    )}
                    检查
                  </Button>

                  <div className="flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => openEdit(s)}
                      className="h-7 text-xs text-zinc-400 hover:text-zinc-200"
                    >
                      配置 / 测试
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleDelete(s.id)}
                      className="h-7 text-xs text-zinc-500 hover:text-rose-400"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              </Card>
            )
          })}
        </div>
      )}

      {/* 新增 / 编辑监控源抽屉面板 (集成 Live Inspector) */}
      <SourceFormDialog
        open={modalOpen}
        editing={editing}
        form={form}
        groups={groups}
        saving={saving}
        error={formError}
        onChange={(patch) => setForm((prev) => ({ ...prev, ...patch }))}
        onClose={() => setModalOpen(false)}
        onSave={handleSave}
      />

    </div>
  )
}
