import { Clock, Server } from "lucide-react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"

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
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Clock className="h-5 w-5" /> 时间
        </CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4 md:grid-cols-3">
        <TimeCell
          icon={<Server className="h-3.5 w-3.5" />}
          label={`服务器本地时间（${serverTz}）`}
          value={serverTimeLocal}
        />
        <TimeCell
          icon={<Clock className="h-3.5 w-3.5" />}
          label="服务器 UTC 时间"
          value={serverTimeUtc?.replace("T", " ").slice(0, 19).concat(" UTC")}
        />
        <TimeCell
          icon={<Clock className="h-3.5 w-3.5" />}
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
    <div className="rounded-md border bg-muted/40 p-3 text-sm">
      <div className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
        {icon} {label}
      </div>
      <div className="mt-1 font-mono">{value || "-"}</div>
    </div>
  )
}
