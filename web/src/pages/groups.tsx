import { useEffect, useState } from "react"
import {
  Clock,
  Plus,
  Trash2,
  Edit2,
  Search,
  Send,
  Code2,
} from "lucide-react"
import { api, type GroupConfig, type ExtractConfig } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card"
import { Dialog } from "@/components/ui/dialog"
import { ErrorText, Loading, EmptyState } from "@/components/ui/feedback"
import { safeJsonParse } from "@/lib/source-form"

export function GroupsPage() {
  const [groups, setGroups] = useState<GroupConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [search, setSearch] = useState("")

  // Modal 编辑状态
  const [open, setOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [id, setId] = useState("")
  const [name, setName] = useState("")
  const [defaultCron, setDefaultCron] = useState("")
  const [defaultNotifyUrl, setDefaultNotifyUrl] = useState("")
  const [headersJson, setHeadersJson] = useState("{}")
  const [extractType, setExtractType] = useState<"none" | "text" | "json" | "items">("none")
  const [textSelector, setTextSelector] = useState("")
  const [jsonPath, setJsonPath] = useState("")
  const [itemsSelector, setItemsSelector] = useState("")
  const [saving, setSaving] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  async function load() {
    try {
      setLoading(true)
      const data = await api.listGroups()
      setGroups(data)
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

  function openCreate() {
    setEditingId(null)
    setId("")
    setName("")
    setDefaultCron("0 * * * *")
    setDefaultNotifyUrl("")
    setHeadersJson("{}")
    setExtractType("none")
    setTextSelector("")
    setJsonPath("")
    setItemsSelector("")
    setFormError(null)
    setOpen(true)
  }

  useEffect(() => {
    if (window.location.search.includes("new=1") || window.location.hash === "#new") {
      openCreate()
    }
  }, [])

  function openEdit(g: GroupConfig) {
    setEditingId(g.id)
    setId(g.id)
    setName(g.name)
    setDefaultCron(g.default_cron ?? "")
    setDefaultNotifyUrl(g.default_notify_url ?? "")
    setHeadersJson(JSON.stringify(g.default_headers ?? {}, null, 2))

    if (!g.default_extract) {
      setExtractType("none")
      setTextSelector("")
      setJsonPath("")
      setItemsSelector("")
    } else if (g.default_extract.type === "text") {
      setExtractType("text")
      setTextSelector(g.default_extract.selector ?? "")
    } else if (g.default_extract.type === "json") {
      setExtractType("json")
      setJsonPath(g.default_extract.json_path ?? "")
    } else {
      setExtractType("items")
      const s = g.default_extract.selector
      setItemsSelector(s.kind === "css" ? s.selector : (s as { path: string }).path)
    }

    setFormError(null)
    setOpen(true)
  }

  async function handleSave() {
    if (!id.trim()) {
      setFormError("分组 ID 为必填项（英文字符，如 news）")
      return
    }
    if (!name.trim()) {
      setFormError("分组名称为必填项")
      return
    }

    let headers: Record<string, string> = {}
    if (headersJson.trim()) {
      const parsed = safeJsonParse<Record<string, string>>(headersJson)
      if (!parsed) {
        setFormError("请求头必须为有效的 JSON 对象")
        return
      }
      headers = parsed
    }

    let default_extract: ExtractConfig | undefined
    if (extractType === "text") {
      default_extract = { type: "text", selector: textSelector.trim() || undefined }
    } else if (extractType === "json") {
      default_extract = { type: "json", json_path: jsonPath.trim() || undefined }
    } else if (extractType === "items") {
      default_extract = {
        type: "items",
        selector: { kind: "css", selector: itemsSelector.trim() || ".item" },
        fields: [],
      }
    }

    const payload: GroupConfig = {
      id: id.trim(),
      name: name.trim(),
      default_cron: defaultCron.trim() || undefined,
      default_headers: Object.keys(headers).length > 0 ? headers : undefined,
      default_extract,
      default_notify_url: defaultNotifyUrl.trim() || undefined,
    }

    setSaving(true)
    setFormError(null)
    try {
      await api.upsertGroup(payload)
      setOpen(false)
      await load()
    } catch (e) {
      setFormError((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete(gid: string) {
    if (!confirm(`确定要删除预设分组「${gid}」吗？`)) return
    try {
      await api.deleteGroup(gid)
      await load()
    } catch (e) {
      alert(`删除失败: ${(e as Error).message}`)
    }
  }

  const filtered = groups.filter(
    (g) =>
      g.name.toLowerCase().includes(search.toLowerCase()) ||
      g.id.toLowerCase().includes(search.toLowerCase()),
  )

  return (
    <div className="space-y-6 max-w-6xl">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-zinc-100">
            预设分组
          </h1>
          <p className="text-sm text-zinc-400">
            为相似站点定义通用规则模板（抓取调度、公共 Headers、提取模式与推送目标）。监控源指定分组后即可自动继承。
          </p>
        </div>

        <div className="flex items-center gap-3">
          <div className="relative">
            <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-zinc-400" />
            <input
              type="text"
              placeholder="搜索预设分组..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="h-9 w-60 rounded-md border border-zinc-800 bg-zinc-900/80 pl-9 pr-3 text-xs text-zinc-200 placeholder:text-zinc-500 focus:outline-none focus:ring-1 focus:ring-zinc-600"
            />
          </div>
          <Button onClick={openCreate} className="gap-2 h-9 text-xs">
            <Plus className="h-4 w-4" />
            新建预设分组
          </Button>
        </div>
      </div>

      {loading && <Loading text="加载分组..." />}
      {error && <ErrorText>{error}</ErrorText>}

      {!loading && !error && filtered.length === 0 && (
        <EmptyState
          title="暂无预设分组"
          description="点击上方「新建预设分组」，可为常用的网站类型或通知目标创建模板。"
        />
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {filtered.map((g) => (
          <Card
            key={g.id}
            className="border-zinc-800 bg-zinc-900/50 hover:border-zinc-700/80 transition-all flex flex-col justify-between"
          >
            <CardHeader className="pb-3">
              <div className="flex items-start justify-between gap-2">
                <div>
                  <div className="flex items-center gap-2">
                    <CardTitle className="text-base text-zinc-100 font-semibold">
                      {g.name}
                    </CardTitle>
                    <span className="rounded bg-zinc-800/90 px-1.5 py-0.5 font-mono text-[11px] text-zinc-300">
                      {g.id}
                    </span>
                  </div>
                  <CardDescription className="text-xs text-zinc-400 mt-1 line-clamp-1">
                    {g.default_notify_url ? (
                      <span className="flex items-center gap-1 text-emerald-400 font-mono text-[11px]">
                        <Send className="h-3 w-3" />
                        指定通知目标
                      </span>
                    ) : (
                      "沿用全局 Telegram 通知"
                    )}
                  </CardDescription>
                </div>

                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-zinc-400 hover:text-zinc-200"
                    onClick={() => openEdit(g)}
                    title="编辑"
                  >
                    <Edit2 className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-zinc-400 hover:text-red-400"
                    onClick={() => handleDelete(g.id)}
                    title="删除"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            </CardHeader>

            <CardContent className="space-y-2 pt-0 text-xs text-zinc-400">
              <div className="flex items-center justify-between border-t border-zinc-800/80 pt-2 font-mono">
                <span className="flex items-center gap-1.5 text-zinc-500">
                  <Clock className="h-3.5 w-3.5" />
                  调度周期:
                </span>
                <span className="text-zinc-300">
                  {g.default_cron || "跟随全局"}
                </span>
              </div>

              <div className="flex items-center justify-between font-mono">
                <span className="flex items-center gap-1.5 text-zinc-500">
                  <Code2 className="h-3.5 w-3.5" />
                  默认提取:
                </span>
                <span className="text-zinc-300 uppercase">
                  {g.default_extract ? g.default_extract.type : "未单独指定"}
                </span>
              </div>

              {g.default_headers && Object.keys(g.default_headers).length > 0 && (
                <div className="flex items-center justify-between font-mono">
                  <span className="text-zinc-500">请求头:</span>
                  <span className="text-zinc-300">
                    {Object.keys(g.default_headers).length} 项
                  </span>
                </div>
              )}
            </CardContent>
          </Card>
        ))}
      </div>

      {/* 新建/编辑预设模态框 */}
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title={editingId ? `编辑预设分组「${name}」` : "新建预设分组"}
        footer={
          <div className="flex items-center justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setOpen(false)}
              disabled={saving}
            >
              取消
            </Button>
            <Button size="sm" onClick={handleSave} disabled={saving}>
              {saving ? "保存中..." : "保存分组"}
            </Button>
          </div>
        }
      >
        <div className="space-y-4 py-2 text-xs">
          {formError && <ErrorText>{formError}</ErrorText>}

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-zinc-400 mb-1">
                分组 ID (唯一标识) *
              </label>
              <input
                type="text"
                disabled={!!editingId}
                value={id}
                onChange={(e) => setId(e.target.value)}
                placeholder="如 news、github"
                className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200 disabled:opacity-50 font-mono"
              />
            </div>
            <div>
              <label className="block text-zinc-400 mb-1">分组展示名称 *</label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="如 新闻资讯、源码发布"
                className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200"
              />
            </div>
          </div>

          <div>
            <label className="block text-zinc-400 mb-1">
              默认 Cron 调度周期 (留空表示沿用全局设置)
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={defaultCron}
                onChange={(e) => setDefaultCron(e.target.value)}
                placeholder="如 */30 * * * *"
                className="flex-1 rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200 font-mono"
              />
              <button
                type="button"
                onClick={() => setDefaultCron("*/15 * * * *")}
                className="rounded border border-zinc-800 bg-zinc-900 px-2 text-[11px] hover:bg-zinc-800"
              >
                15m
              </button>
              <button
                type="button"
                onClick={() => setDefaultCron("0 * * * *")}
                className="rounded border border-zinc-800 bg-zinc-900 px-2 text-[11px] hover:bg-zinc-800"
              >
                1h
              </button>
            </div>
          </div>

          <div>
            <label className="block text-zinc-400 mb-1">
              专属 Telegram 通知目标 (留空沿用全局)
            </label>
            <input
              type="text"
              value={defaultNotifyUrl}
              onChange={(e) => setDefaultNotifyUrl(e.target.value)}
              placeholder="tgram://bottoken/ChatID1/ChatID2"
              className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200 font-mono"
            />
          </div>

          <div>
            <label className="block text-zinc-400 mb-1">预设内容提取模式</label>
            <div className="grid grid-cols-4 gap-2 mb-2">
              {(["none", "text", "json", "items"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  onClick={() => setExtractType(mode)}
                  className={`rounded border py-1.5 text-center text-xs uppercase font-mono transition-all cursor-pointer ${
                    extractType === mode
                      ? "border-zinc-300 bg-zinc-800 text-zinc-100 font-semibold shadow-xs"
                      : "border-zinc-800 bg-zinc-950/60 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200"
                  }`}
                >
                  {mode === "none" ? "不预设" : mode}
                </button>
              ))}
            </div>

            {extractType === "text" && (
              <input
                type="text"
                value={textSelector}
                onChange={(e) => setTextSelector(e.target.value)}
                placeholder="可选：页面区域 CSS 选择器 (留空抓整页纯文本)"
                className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200 font-mono"
              />
            )}

            {extractType === "json" && (
              <input
                type="text"
                value={jsonPath}
                onChange={(e) => setJsonPath(e.target.value)}
                placeholder="可选：JSONPath (如 $.data.items, 留空取全量 JSON)"
                className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200 font-mono"
              />
            )}

            {extractType === "items" && (
              <input
                type="text"
                value={itemsSelector}
                onChange={(e) => setItemsSelector(e.target.value)}
                placeholder="条目容器 CSS 选择器 (如 .post, tr.item)"
                className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 px-3 py-1.5 text-xs text-zinc-200 font-mono"
              />
            )}
          </div>

          <div>
            <label className="block text-zinc-400 mb-1">
              默认公共请求头 (JSON 格式)
            </label>
            <textarea
              rows={3}
              value={headersJson}
              onChange={(e) => setHeadersJson(e.target.value)}
              placeholder='{"User-Agent": "MyCustomBot"}'
              className="w-full rounded-md border border-zinc-800 bg-zinc-900/80 p-2 font-mono text-xs text-zinc-200 focus:outline-none focus:ring-1 focus:ring-zinc-600"
            />
          </div>
        </div>
      </Dialog>
    </div>
  )
}
