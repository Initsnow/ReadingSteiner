import { useEffect, useState } from "react"
import { Play, FlaskConical, Loader2, RefreshCw } from "lucide-react"
import { api, type SourceConfig } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

export function SourcesPage() {
  const [sources, setSources] = useState<SourceConfig[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

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
      await load()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusyId(null)
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
        <Button variant="outline" size="sm" onClick={load}>
          <RefreshCw className="h-4 w-4" /> 刷新
        </Button>
      </div>

      {error && (
        <p className="text-sm text-destructive">加载失败：{error}</p>
      )}

      {sources.length === 0 ? (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            暂无监控源。请在 config.yaml 中配置 sources 后重启 daemon。
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
                    <Badge
                      variant={s.enabled ? "success" : "secondary"}
                    >
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
                  <span>
                    interval: {s.schedule.interval_secs}s
                  </span>
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
                <div className="mt-4 flex gap-2">
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
                    variant="ghost"
                    disabled={busyId === s.id}
                    onClick={() => testPipeline(s.id)}
                  >
                    <FlaskConical className="h-4 w-4" />
                    试跑流水线
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
