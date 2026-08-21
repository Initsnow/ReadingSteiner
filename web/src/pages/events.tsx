import { useEffect, useState } from "react"
import { Link } from "react-router-dom"
import { Loader2, RefreshCw } from "lucide-react"
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

const changeTypeVariant: Record<string, "success" | "warning" | "destructive" | "default"> = {
  new: "success",
  updated: "warning",
  removed: "destructive",
}

function formatTime(ts: string) {
  const d = new Date(ts)
  const local = d.toLocaleString()
  const utc = d.toISOString().replace("T", " ").slice(0, 19) + " UTC"
  return `${local} · ${utc}`
}

export function EventsPage() {
  const [events, setEvents] = useState<ChangeEvent[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  async function load() {
    try {
      setEvents(await api.listEvents(50))
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

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 加载事件…
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <CardDescription>共 {events.length} 条变更事件</CardDescription>
        <Button variant="outline" size="sm" onClick={load}>
          <RefreshCw className="h-4 w-4" /> 刷新
        </Button>
      </div>

      {error && <p className="text-sm text-destructive">加载失败：{error}</p>}

      {events.length === 0 ? (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            暂无变更事件。
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-2">
          {events.map((e) => (
            <Link key={e.id} to={`/events/${e.id}`}>
              <Card className="transition-colors hover:border-primary">
                <CardHeader className="flex flex-row items-center justify-between space-y-0">
                  <div className="flex items-center gap-2">
                    <span className="text-muted-foreground">#{e.id}</span>
                    <Badge variant={changeTypeVariant[e.change_type] ?? "default"}>
                      {e.change_type}
                    </Badge>
                    <CardTitle className="text-sm">
                      {e.watchpoint_id}
                    </CardTitle>
                  </div>
                  <CardDescription>{formatTime(e.detected_at)}</CardDescription>
                </CardHeader>
                <CardContent>
                  <p className="line-clamp-2 text-sm text-muted-foreground">
                    {e.diff_summary || "（无摘要）"}
                  </p>
                </CardContent>
              </Card>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
