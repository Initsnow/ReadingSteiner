import { useEffect, useState } from "react"
import {
  Activity,
  Check,
  CheckCheck,
  Clock,
  Filter,
  Image as ImageIcon,
  Loader2,
  RefreshCw,
  ZoomIn,
} from "lucide-react"
import { api, type ChangeEvent } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { Dialog } from "@/components/ui/dialog"
import { ErrorText, Loading, EmptyState } from "@/components/ui/feedback"
import { parseChangeEvent, itemText } from "@/lib/source-form"
import { AuthImage } from "@/components/auth-image"

export function EventsPage() {
  const [events, setEvents] = useState<ChangeEvent[]>([])
  const [loading, setLoading] = useState(true)
  const [markingAll, setMarkingAll] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [filterSource, setFilterSource] = useState<string>("")
  const [filterUnread, setFilterUnread] = useState(false)
  const [expandedDiff, setExpandedDiff] = useState<number | null>(null)
  const [lightboxUrl, setLightboxUrl] = useState<string | null>(null)

  async function load() {
    try {
      setLoading(true)
      const data = await api.listEvents(100)
      setEvents(data)
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

  async function markRead(id: number) {
    try {
      await api.markEventRead(id)
      setEvents((prev) =>
        prev.map((e) => (e.id === id ? { ...e, read: true } : e)),
      )
    } catch (e) {
      alert(`标为已读失败: ${(e as Error).message}`)
    }
  }

  async function markAllRead() {
    setMarkingAll(true)
    try {
      await api.markAllEventsRead()
      setEvents((prev) => prev.map((e) => ({ ...e, read: true })))
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setMarkingAll(false)
    }
  }

  const unreadCount = events.filter((e) => !e.read).length

  const filtered = events.filter((e) => {
    if (filterUnread && e.read) return false
    if (
      filterSource &&
      !e.watchpoint_id.toLowerCase().includes(filterSource.toLowerCase())
    )
      return false
    return true
  })

  return (
    <div className="space-y-6 max-w-6xl">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-zinc-100 flex items-center gap-2">
            <Activity className="h-5 w-5 text-primary" />
            变更事件流
          </h1>
          <p className="text-sm text-zinc-400">
            抓取检测到的内容变化记录。点击条目可展开详细的 Visual Diff 与 CDP 截屏比对。
          </p>
        </div>

        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={markAllRead}
            disabled={markingAll || unreadCount === 0}
            className="gap-2 h-9 text-xs border-zinc-800 bg-zinc-900"
          >
            {markingAll ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <CheckCheck className="h-3.5 w-3.5" />
            )}
            全部标为已读
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={load}
            className="gap-2 h-9 text-xs border-zinc-800 bg-zinc-900"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            刷新
          </Button>
        </div>
      </div>

      {/* 筛选条 */}
      <div className="flex flex-wrap items-center gap-3 rounded-lg border border-zinc-800 bg-zinc-900/60 p-3 text-xs">
        <div className="flex items-center gap-2 text-zinc-400">
          <Filter className="h-3.5 w-3.5" />
          <span>筛选:</span>
        </div>

        <input
          type="text"
          placeholder="按监控源 ID 过滤..."
          value={filterSource}
          onChange={(e) => setFilterSource(e.target.value)}
          className="h-8 w-48 rounded border border-zinc-800 bg-zinc-950 px-2.5 text-xs text-zinc-200 placeholder:text-zinc-500 font-mono"
        />

        <button
          type="button"
          onClick={() => setFilterUnread(!filterUnread)}
          className={`h-8 rounded border px-3 text-xs transition-colors ${
            filterUnread
              ? "border-primary bg-primary/20 text-primary font-medium"
              : "border-zinc-800 bg-zinc-950 text-zinc-400 hover:text-zinc-200"
          }`}
        >
          仅看未读 ({unreadCount})
        </button>
      </div>

      {loading && <Loading text="加载事件列表..." />}
      {error && <ErrorText>{error}</ErrorText>}

      {!loading && !error && filtered.length === 0 && (
        <EmptyState
          title="暂无变更事件"
          description="当监控源检测到内容更新时，相应的变更记录将自动显示在此处。"
        />
      )}

      {/* 事件时间线卡片列表 */}
      <div className="space-y-3">
        {filtered.map((e) => {
          const isExpanded = expandedDiff === e.id
          const { oldItems, newItems } = isExpanded
            ? parseChangeEvent(e)
            : { oldItems: [], newItems: [] }
          const screenshotUrl = e.screenshot_path
            ? `/api/events/${e.id}/screenshot`
            : null

          return (
            <Card
              key={e.id}
              className={`border transition-all ${
                e.read
                  ? "border-zinc-850 bg-zinc-900/40 opacity-90"
                  : "border-zinc-700/80 bg-zinc-900/80 shadow-md"
              }`}
            >
              <CardContent className="p-4 space-y-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-sm font-semibold text-zinc-100">
                        {e.watchpoint_id}
                      </span>
                      <Badge
                        variant="secondary"
                        className={`text-[10px] uppercase font-mono ${
                          e.change_type === "new"
                            ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/20"
                            : e.change_type === "updated"
                              ? "bg-amber-500/10 text-amber-400 border-amber-500/20"
                              : "bg-rose-500/10 text-rose-400 border-rose-500/20"
                        }`}
                      >
                        {e.change_type}
                      </Badge>
                      {!e.read && (
                        <span className="h-2 w-2 rounded-full bg-primary" />
                      )}
                    </div>

                    <div className="text-xs text-zinc-300 font-medium">
                      {e.diff_summary}
                    </div>

                    <div className="flex items-center gap-4 text-[11px] text-zinc-500 font-mono">
                      <span className="flex items-center gap-1">
                        <Clock className="h-3 w-3" />
                        {e.detected_at}
                      </span>
                      <span>指纹: {e.fingerprint.slice(0, 10)}...</span>
                      {e.screenshot_path && (
                        <span className="flex items-center gap-1 text-primary">
                          <ImageIcon className="h-3 w-3" />
                          附带截屏
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-8 text-xs border-zinc-800 bg-zinc-900 hover:bg-zinc-800"
                      onClick={() =>
                        setExpandedDiff(isExpanded ? null : e.id)
                      }
                    >
                      {isExpanded ? "收起对比" : "展开 Diff 比对"}
                    </Button>

                    {!e.read && (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-8 text-xs text-zinc-400 hover:text-zinc-200"
                        onClick={() => markRead(e.id)}
                        title="标为已读"
                      >
                        <Check className="h-3.5 w-3.5" />
                      </Button>
                    )}
                  </div>
                </div>

                {/* 展开的 Visual Diff & Screenshot 区域 */}
                {isExpanded && (
                  <div className="mt-4 pt-4 border-t border-zinc-800 space-y-4">
                    {screenshotUrl && (
                      <div className="space-y-1.5">
                        <div className="text-xs font-semibold text-zinc-300 flex items-center gap-1.5">
                          <ImageIcon className="h-3.5 w-3.5 text-primary" />
                          变更时刻 CDP 页面快照截屏:
                        </div>
                        <div
                          className="relative inline-block overflow-hidden rounded-md border border-zinc-800 group cursor-pointer"
                          onClick={() => setLightboxUrl(screenshotUrl)}
                        >
                          <AuthImage
                            url={screenshotUrl}
                            alt="变更截屏"
                            className="max-h-64 object-cover transition-transform group-hover:scale-[1.02]"
                          />
                          <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity text-white text-xs gap-1 font-medium">
                            <ZoomIn className="h-4 w-4" />
                            点击放大全图
                          </div>
                        </div>
                      </div>
                    )}

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-xs">
                      {/* 旧数据 */}
                      <div className="rounded-md border border-rose-950/60 bg-rose-950/20 p-3 space-y-2">
                        <div className="font-semibold text-rose-400 flex items-center justify-between">
                          <span>变更前 (Previous State)</span>
                          <span className="font-mono text-[11px] text-rose-500">
                            {oldItems.length} 条目
                          </span>
                        </div>
                        <div className="max-h-60 overflow-y-auto font-mono text-[11px] text-zinc-300 space-y-1.5">
                          {oldItems.length === 0 ? (
                            <span className="text-zinc-600 italic">空 (初次创建或文本变更)</span>
                          ) : (
                            oldItems.map((it, idx) => (
                              <div
                                key={idx}
                                className="rounded bg-zinc-950/60 p-2 border border-rose-900/30 whitespace-pre-wrap break-all"
                              >
                                {itemText(it)}
                              </div>
                            ))
                          )}
                        </div>
                      </div>

                      {/* 新数据 */}
                      <div className="rounded-md border border-emerald-950/60 bg-emerald-950/20 p-3 space-y-2">
                        <div className="font-semibold text-emerald-400 flex items-center justify-between">
                          <span>变更后 (Detected New State)</span>
                          <span className="font-mono text-[11px] text-emerald-500">
                            {newItems.length} 条目
                          </span>
                        </div>
                        <div className="max-h-60 overflow-y-auto font-mono text-[11px] text-zinc-300 space-y-1.5">
                          {newItems.length === 0 ? (
                            <span className="text-zinc-600 italic">空</span>
                          ) : (
                            newItems.map((it, idx) => (
                              <div
                                key={idx}
                                className="rounded bg-zinc-950/60 p-2 border border-emerald-900/30 whitespace-pre-wrap break-all"
                              >
                                {itemText(it)}
                              </div>
                            ))
                          )}
                        </div>
                      </div>
                    </div>
                  </div>
                )}
              </CardContent>
            </Card>
          )
        })}
      </div>

      {/* 图片 Lightbox 弹窗 */}
      <Dialog
        open={!!lightboxUrl}
        onClose={() => setLightboxUrl(null)}
        title="页面截屏快照全图"
        width="lg"
      >
        <div className="relative flex max-h-[75vh] items-center justify-center overflow-auto p-2">
          {lightboxUrl && (
            <AuthImage
              url={lightboxUrl}
              alt="截屏全图"
              className="max-h-full max-w-full rounded object-contain"
            />
          )}
        </div>
      </Dialog>
    </div>
  )
}
