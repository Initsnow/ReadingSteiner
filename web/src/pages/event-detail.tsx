import { useEffect, useState } from "react"
import { useParams, Link } from "react-router-dom"
import { ArrowLeft, Loader2 } from "lucide-react"
import { api, type ChangeEvent } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

export function EventDetailPage() {
  const { id } = useParams<{ id: string }>()
  const [event, setEvent] = useState<ChangeEvent | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!id) return
    let active = true
    api
      .getEvent(Number(id))
      .then((e) => {
        if (active) setEvent(e)
      })
      .catch((e) => {
        if (active) setError((e as Error).message)
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [id])

  return (
    <div className="space-y-4">
      <Button variant="ghost" size="sm" asChild>
        <Link to="/events">
          <ArrowLeft className="h-4 w-4" /> 返回事件列表
        </Link>
      </Button>

      {loading && (
        <div className="flex items-center gap-2 text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" /> 加载事件…
        </div>
      )}

      {error && <p className="text-sm text-destructive">加载失败：{error}</p>}

      {event && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              事件 #{event.id}
              <Badge variant="outline">{event.change_type}</Badge>
            </CardTitle>
            <CardDescription>
              {event.watchpoint_id} ·{" "}
              {new Date(event.detected_at).toLocaleString()}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div>
              <div className="mb-1 text-sm font-medium">变更摘要</div>
              <p className="rounded-md bg-muted p-3 text-sm">
                {event.diff_summary || "（无摘要）"}
              </p>
            </div>

            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <div className="mb-1 text-sm font-medium">旧数据</div>
                <pre className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs">
                  {formatJson(event.old_items_json)}
                </pre>
              </div>
              <div>
                <div className="mb-1 text-sm font-medium">新数据</div>
                <pre className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs">
                  {formatJson(event.new_items_json)}
                </pre>
              </div>
            </div>

            <div className="flex flex-wrap gap-4 text-xs text-muted-foreground">
              <span>fingerprint: {truncate(event.fingerprint, 48)}</span>
              <span>dedupe_key: {truncate(event.dedupe_key, 48)}</span>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function formatJson(json: string) {
  try {
    return JSON.stringify(JSON.parse(json), null, 2)
  } catch {
    return json
  }
}

function truncate(s: string, n: number) {
  return s.length > n ? s.slice(0, n) + "…" : s
}
