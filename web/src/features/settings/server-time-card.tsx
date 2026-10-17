import { Clock, Globe, Laptop } from "lucide-react"
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card"

/** 服务器 / 浏览器时间对照：排查 cron 时区问题时的第一手信息。 */
export function ServerTimeCard({
  serverTz,
  serverTimeLocal,
  serverTimeUtc,
  browserNow,
}: {
  serverTz: string
  serverTimeLocal: string | null
  serverTimeUtc: string | null
  browserNow: Date
}) {
  return (
    <Card className="border border-zinc-800 bg-zinc-900/60">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base text-zinc-100">
          <Clock className="h-4 w-4 text-primary" /> 时区与时钟对照
        </CardTitle>
        <CardDescription className="text-xs text-zinc-400">
          用于校验调度器 Cron 执行时刻与服务器所在时区的实际时间对齐情况。
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-3 sm:grid-cols-3">
        <TimeCell
          icon={<Globe className="h-3.5 w-3.5 text-emerald-400" />}
          label={`服务器本地 (${serverTz})`}
          value={serverTimeLocal}
        />
        <TimeCell
          icon={<Clock className="h-3.5 w-3.5 text-blue-400" />}
          label="服务器 UTC 标准时间"
          value={serverTimeUtc?.replace("T", " ").slice(0, 19).concat(" UTC")}
        />
        <TimeCell
          icon={<Laptop className="h-3.5 w-3.5 text-amber-400" />}
          label="浏览器本地时间"
          value={browserNow.toLocaleString()}
        />
      </CardContent>
    </Card>
  )
}

function TimeCell({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode
  label: string
  value?: string | null
}) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-950/60 p-3 text-xs">
      <div className="flex items-center gap-1.5 text-zinc-400 font-medium">
        {icon} {label}
      </div>
      <div className="mt-1.5 font-mono text-xs font-semibold text-zinc-200">
        {value || "-"}
      </div>
    </div>
  )
}
