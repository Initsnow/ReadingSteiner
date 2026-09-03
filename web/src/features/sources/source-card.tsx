import {
  AlertCircle,
  ChevronDown,
  ChevronRight,
  Eye,
  History as HistoryIcon,
  Loader2,
  Pencil,
  Play,
  TestTube2,
  Trash2,
} from "lucide-react"
import type { ChangeEvent, SourceMeta } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { formatDateTime, formatRelativeTime } from "@/lib/format"
import { EventRow } from "./event-row"

/** 单个监控源卡片：状态徽标、操作按钮、可展开的变更历史。 */
export function SourceCard({
  source,
  selected,
  busy,
  testing,
  expanded,
  history,
  historyLoading,
  onToggleSelect,
  onToggleHistory,
  onMarkRead,
  onCheck,
  onTest,
  onEdit,
  onDelete,
}: {
  source: SourceMeta
  selected: boolean
  busy: boolean
  testing: boolean
  expanded: boolean
  history?: ChangeEvent[]
  historyLoading: boolean
  onToggleSelect: () => void
  onToggleHistory: () => void
  onMarkRead: () => void
  onCheck: () => void
  onTest: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  const unread = source.unread_count ?? 0
  const hasScreenshot = source.fetch.engine === "camofox" && source.fetch.screenshot

  return (
    <div
      className={`rounded-xl border bg-card shadow ${
        selected ? "ring-2 ring-primary/60" : undefined
      }`}
    >
      <div className="px-4 py-3">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <input
            type="checkbox"
            className="h-4 w-4 shrink-0 accent-primary"
            checked={selected}
            onChange={onToggleSelect}
          />
          <button
            className="shrink-0 text-muted-foreground hover:text-foreground"
            onClick={onToggleHistory}
            title={expanded ? "收起历史" : "展开历史"}
          >
            {historyLoading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : expanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
          </button>

          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="truncate text-sm font-medium">
                {source.name || source.id}
              </span>
              {unread > 0 && (
                <Badge variant="warning" className="px-1.5 py-0 text-[10px]">
                  {unread} 未读
                </Badge>
              )}
              {source.has_error && (
                <Badge
                  variant="destructive"
                  className="px-1.5 py-0 text-[10px]"
                  title={source.last_error ?? "连续失败，请检查配置或网络"}
                >
                  <AlertCircle className="h-2.5 w-2.5" /> 错误
                </Badge>
              )}
              <Badge
                variant={source.enabled ? "success" : "secondary"}
                className="px-1.5 py-0 text-[10px]"
              >
                {source.enabled ? "监控中" : "已暂停监控"}
              </Badge>
              <Badge
                variant={source.notify_enabled ? "success" : "secondary"}
                className="px-1.5 py-0 text-[10px]"
              >
                {source.notify_enabled ? "通知开" : "已暂停通知"}
              </Badge>
              {hasScreenshot && (
                <Badge variant="outline" className="px-1.5 py-0 text-[10px]">
                  <Eye className="h-2.5 w-2.5" /> 截图
                </Badge>
              )}
              {source.tags.map((t) => (
                <Badge key={t} variant="outline" className="px-1.5 py-0 text-[10px]">
                  {t}
                </Badge>
              ))}
            </div>

            <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
              <span className="truncate">{source.fetch.url}</span>
              <span className="shrink-0">
                最近检查:{" "}
                <span title={formatDateTime(source.last_check_at)}>
                  {formatRelativeTime(source.last_check_at)}
                </span>
              </span>
              <span className="shrink-0">
                最近变更:{" "}
                <span title={formatDateTime(source.last_change_at)}>
                  {formatRelativeTime(source.last_change_at)}
                </span>
              </span>
            </div>

            {source.has_error && source.last_error && (
              <div className="mt-1 flex items-start gap-1 rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-xs text-destructive">
                <AlertCircle className="mt-0.5 h-3 w-3 shrink-0" />
                <span className="break-all">{source.last_error}</span>
              </div>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-1">
            {unread > 0 && (
              <Button
                size="sm"
                variant="ghost"
                className="h-7 px-2 text-xs"
                onClick={onMarkRead}
                title="标记已读"
              >
                <Eye className="h-3.5 w-3.5" /> 已读
              </Button>
            )}
            <IconButton
              title="立即检测"
              busy={busy}
              onClick={onCheck}
              icon={<Play className="h-3.5 w-3.5" />}
            />
            <IconButton
              title="测试"
              busy={testing}
              onClick={onTest}
              icon={<TestTube2 className="h-3.5 w-3.5" />}
            />
            <Button
              size="sm"
              variant="ghost"
              className="h-7 px-2 text-xs"
              onClick={onEdit}
            >
              <Pencil className="h-3.5 w-3.5" /> 编辑
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 px-2 text-xs text-destructive hover:text-destructive"
              disabled={busy}
              onClick={onDelete}
            >
              <Trash2 className="h-3.5 w-3.5" /> 删除
            </Button>
          </div>
        </div>

        {expanded && (
          <div className="mt-3 border-t pt-3">
            <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <HistoryIcon className="h-3.5 w-3.5" /> 变更历史
              {history && (
                <span className="text-[10px] text-muted-foreground">
                  共 {history.length} 条
                </span>
              )}
            </div>
            {historyLoading ? (
              <div className="flex items-center gap-2 py-4 text-xs text-muted-foreground">
                <Loader2 className="h-3.5 w-3.5 animate-spin" /> 加载历史…
              </div>
            ) : history && history.length > 0 ? (
              <div className="space-y-2">
                {history.map((ev) => (
                  <EventRow key={ev.id} event={ev} />
                ))}
              </div>
            ) : (
              <p className="py-2 text-xs text-muted-foreground">暂无变更记录。</p>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

/** 图标按钮：busy 时显示旋转指示器并禁用。 */
function IconButton({
  title,
  busy,
  onClick,
  icon,
}: {
  title: string
  busy: boolean
  onClick: () => void
  icon: React.ReactNode
}) {
  return (
    <Button
      size="sm"
      variant="ghost"
      className="h-7 px-2 text-xs"
      disabled={busy}
      onClick={onClick}
      title={title}
    >
      {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : icon}
    </Button>
  )
}
