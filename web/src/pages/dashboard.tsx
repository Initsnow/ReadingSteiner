import { useEffect, useState } from "react"
import type { ReactNode } from "react"
import { CheckCircle2, Circle, Loader2, Radio, ScrollText } from "lucide-react"
import { api, type DaemonStatus } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

function StatCard({
  title,
  value,
  sub,
  icon,
}: {
  title: string
  value: ReactNode
  sub?: string
  icon: ReactNode
}) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        {icon}
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">{value}</div>
        {sub && <p className="text-xs text-muted-foreground">{sub}</p>}
      </CardContent>
    </Card>
  )
}

export function DashboardPage() {
  const [status, setStatus] = useState<DaemonStatus | null>(null)
  const [events, setEvents] = useState<number>(0)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let active = true
    async function load() {
      try {
        const s = await api.status()
        const evs = await api.listEvents(1)
        if (!active) return
        setStatus(s)
        setEvents(evs.length > 0 ? evs[0].id : 0)
        setError(null)
      } catch (e) {
        if (active) {
          setError((e as Error).message)
          setStatus(null)
        }
      } finally {
        if (active) setLoading(false)
      }
    }
    load()
    const t = setInterval(load, 5000)
    return () => {
      active = false
      clearInterval(t)
    }
  }, [])

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 正在连接 daemon…
      </div>
    )
  }

  if (error || !status) {
    return (
      <div className="space-y-4">
        <Card>
          <CardHeader>
            <CardTitle>无法连接 daemon</CardTitle>
            <CardDescription>
              请确认已启动后端服务：<code>reading-steiner serve</code>
            </CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-destructive">
              {error ?? "未获取到状态"}
            </p>
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          title="运行状态"
          value={
            status.running ? (
              <span className="flex items-center gap-2">
                <CheckCircle2 className="h-5 w-5 text-emerald-500" /> 运行中
              </span>
            ) : (
              <span className="flex items-center gap-2">
                <Circle className="h-5 w-5 text-muted-foreground" /> 未运行
              </span>
            )
          }
          sub={`版本 ${status.version}`}
          icon={<Radio className="h-4 w-4 text-muted-foreground" />}
        />
        <StatCard
          title="监控源"
          value={status.sources}
          sub={`已启用 ${status.enabled_sources}`}
          icon={<Radio className="h-4 w-4 text-muted-foreground" />}
        />
        <StatCard
          title="队列深度"
          value={status.queue_depth}
          sub="待调度任务数"
          icon={<ScrollText className="h-4 w-4 text-muted-foreground" />}
        />
        <StatCard
          title="最新事件 ID"
          value={events}
          sub="最近一次变更"
          icon={<ScrollText className="h-4 w-4 text-muted-foreground" />}
        />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>引擎健康</CardTitle>
          <CardDescription>各抓取引擎的连通状态</CardDescription>
        </CardHeader>
        <CardContent>
          {Object.keys(status.engine_health).length === 0 ? (
            <p className="text-sm text-muted-foreground">
              尚无引擎状态，等待首次检测…
            </p>
          ) : (
            <div className="flex flex-wrap gap-2">
              {Object.entries(status.engine_health).map(([engine, ok]) => (
                <Badge key={engine} variant={ok ? "success" : "destructive"}>
                  {engine}: {ok ? "healthy" : "down"}
                </Badge>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <div className="text-right">
        <Button
          variant="outline"
          onClick={() => window.location.reload()}
        >
          刷新
        </Button>
      </div>
    </div>
  )
}
