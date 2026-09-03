import { useState } from "react"
import { ChevronDown, ChevronRight } from "lucide-react"
import { api, type ChangeEvent } from "@/lib/api"
import { AuthImage } from "@/components/auth-image"
import { Badge } from "@/components/ui/badge"
import { formatDateTime, formatRelativeTime } from "@/lib/format"
import { itemText, parseChangeEvent } from "@/lib/source-form"

const changeTypeVariant = {
  new: "success",
  updated: "warning",
  removed: "destructive",
} as const

/** 单条变更事件：摘要 + 可展开的旧/新对比 + 截图。 */
export function EventRow({ event }: { event: ChangeEvent }) {
  const [read, setRead] = useState(event.read)
  const [showDiff, setShowDiff] = useState(false)
  const { oldItems, newItems } = parseChangeEvent(event)

  async function markRead() {
    if (read) return
    try {
      await api.markEventRead(event.id)
      setRead(true)
    } catch {
      // 标记已读失败不阻塞查看
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
          variant={changeTypeVariant[event.change_type as keyof typeof changeTypeVariant] ?? "secondary"}
          className="px-1.5 py-0 text-[10px]"
        >
          {event.change_type}
        </Badge>
        {!read && <span className="text-[10px] text-primary">● 未读</span>}
        <span
          className="text-muted-foreground"
          title={formatDateTime(event.detected_at)}
        >
          {formatRelativeTime(event.detected_at)}
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

      {event.screenshot_path && (
        <div className="mt-2">
          <AuthImage
            url={`/api/events/${event.id}/screenshot`}
            alt={`截图 #${event.id}`}
            className="max-h-48 rounded-md border object-contain"
          />
        </div>
      )}

      {showDiff && (
        <div className="mt-2 grid gap-2 md:grid-cols-2">
          <DiffPane label="旧" items={oldItems} />
          <DiffPane label="新" items={newItems} />
        </div>
      )}
    </div>
  )
}

function DiffPane({
  label,
  items,
}: {
  label: string
  items: Record<string, unknown>[]
}) {
  return (
    <div className="rounded bg-muted p-2">
      <div className="mb-1 font-medium text-muted-foreground">
        {label} ({items.length} 条)
      </div>
      {items.length > 0 ? (
        <pre className="max-h-40 overflow-auto font-mono text-[10px]">
          {items.map(itemText).join("\n\n")}
        </pre>
      ) : (
        <span className="text-muted-foreground">—</span>
      )}
    </div>
  )
}
