import { useEffect, useState } from "react"
import {
  Loader2,
  RefreshCw,
  Save,
  Archive,
  History,
  Clock,
  Server,
} from "lucide-react"
import { api, type EditableSettings } from "@/lib/api"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

const inputCls =
  "w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
const labelCls = "text-xs font-medium text-muted-foreground"

function Field({
  label,
  hint,
  className,
  children,
}: {
  label: string
  hint?: string
  className?: string
  children: React.ReactNode
}) {
  return (
    <div className={className}>
      <label className={labelCls}>{label}</label>
      <div className="mt-1">{children}</div>
      {hint && <p className="mt-1 text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}

export function SettingsPage() {
  const [settings, setSettings] = useState<EditableSettings | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  // 服务器时间显示
  const [serverTime, setServerTime] = useState<string | null>(null)
  const [serverTz, setServerTz] = useState("UTC")
  const [browserNow, setBrowserNow] = useState(new Date())

  async function load() {
    try {
      const [s, st] = await Promise.all([api.getSettings(), api.status()])
      setSettings(s)
      setServerTz(st.timezone || "UTC")
      setServerTime(st.server_time_local)
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

  // 浏览器本地时间持续刷新
  useEffect(() => {
    const timer = setInterval(() => setBrowserNow(new Date()), 1000)
    return () => clearInterval(timer)
  }, [])

  async function handleSave() {
    if (!settings) return
    setSaving(true)
    setNotice(null)
    setError(null)
    try {
      const res = await api.updateSettings(settings)
      setNotice(
        `已保存到 ${res.config}。${res.restart_required ? "部分设置需重启 daemon 后生效。" : ""}`,
      )
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  async function handleBackup() {
    setNotice(null)
    setError(null)
    try {
      const res = await api.createBackup()
      setNotice(`备份已创建：${res.name}（${res.path}）`)
    } catch (e) {
      setError((e as Error).message)
    }
  }

  async function handleRestore() {
    setNotice(null)
    setError(null)
    const name = window.prompt("输入要恢复的备份名称（仅允许在 daemon 停止时恢复）")
    if (!name) return
    try {
      const res = (await api.restoreBackup(name)) as { error?: string }
      if (res.error) {
        setError(res.error)
      } else {
        setNotice("恢复指令已提交。")
      }
    } catch (e) {
      setError((e as Error).message)
    }
  }

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 加载设置…
      </div>
    )
  }

  if (!settings) {
    return <p className="text-sm text-destructive">加载失败：{error}</p>
  }

  return (
    <div className="space-y-4">
      {/* 服务器 / 浏览器时间 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Clock className="h-5 w-5" /> 时间
          </CardTitle>
          <CardDescription>服务器时间与浏览器本地时间对照</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4 md:grid-cols-3">
          <div className="rounded-md border bg-muted/40 p-3 text-sm">
            <div className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
              <Server className="h-3.5 w-3.5" /> 服务器本地时间（{serverTz}）
            </div>
            <div className="mt-1 font-mono">{serverTime ?? "-"}</div>
          </div>
          <div className="rounded-md border bg-muted/40 p-3 text-sm">
            <div className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
              <Clock className="h-3.5 w-3.5" /> 服务器 UTC 时间
            </div>
            <div className="mt-1 font-mono">
              {serverTime ? (
                <span>{new Date(serverTime).toISOString().replace("T", " ").slice(0, 19)} UTC</span>
              ) : (
                "-"
              )}
            </div>
          </div>
          <div className="rounded-md border bg-muted/40 p-3 text-sm">
            <div className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
              <Clock className="h-3.5 w-3.5" /> 浏览器本地时间
            </div>
            <div className="mt-1 font-mono">{browserNow.toLocaleString()}</div>
          </div>
        </CardContent>
      </Card>

      {error && (
        <p className="text-sm text-destructive">操作失败：{error}</p>
      )}
      {notice && <p className="text-sm text-green-600">{notice}</p>}

      {/* 全局设置 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Save className="h-5 w-5" /> 全局设置
          </CardTitle>
          <CardDescription>
            抓取并发、超时、User-Agent、历史保留、失败阈值、时区与通知模板
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-4">
            <Field label="抓取工作线程数（并发）">
              <input
                type="number"
                className={inputCls}
                value={settings.concurrency}
                onChange={(e) =>
                  setSettings({ ...settings, concurrency: Number(e.target.value) })
                }
              />
            </Field>
            <Field label="队列容量">
              <input
                type="number"
                className={inputCls}
                value={settings.queue_capacity}
                onChange={(e) =>
                  setSettings({ ...settings, queue_capacity: Number(e.target.value) })
                }
              />
            </Field>
            <Field label="默认请求超时（秒）">
              <input
                type="number"
                className={inputCls}
                value={settings.default_timeout_secs}
                onChange={(e) =>
                  setSettings({ ...settings, default_timeout_secs: Number(e.target.value) })
                }
              />
            </Field>
            <Field label="默认 User-Agent" hint="留空使用内置默认值">
              <input
                className={inputCls}
                value={settings.default_user_agent}
                onChange={(e) =>
                  setSettings({ ...settings, default_user_agent: e.target.value })
                }
              />
            </Field>
            <Field label="每个监控源保留历史条数" hint="0 表示不限制">
              <input
                type="number"
                className={inputCls}
                value={settings.history_limit_per_source}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    history_limit_per_source: Number(e.target.value),
                  })
                }
              />
            </Field>
            <Field label="连续失败通知阈值" hint="0 表示禁用失败通知">
              <input
                type="number"
                className={inputCls}
                value={settings.failure_notify_threshold}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    failure_notify_threshold: Number(e.target.value),
                  })
                }
              />
            </Field>
            <Field label="调度器时区" hint="IANA 名称，如 Asia/Shanghai、UTC">
              <input
                className={inputCls}
                value={settings.timezone}
                onChange={(e) =>
                  setSettings({ ...settings, timezone: e.target.value })
                }
              />
            </Field>
            <Field label="单事件最多附带图片数">
              <input
                type="number"
                className={inputCls}
                value={settings.max_images_per_event}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    max_images_per_event: Number(e.target.value),
                  })
                }
              />
            </Field>
            <Field label="Telegram 默认 Chat ID" className="col-span-2">
              <input
                className={inputCls}
                value={settings.default_chat_id}
                onChange={(e) =>
                  setSettings({ ...settings, default_chat_id: e.target.value })
                }
              />
            </Field>
            <Field
              label="变更通知模板"
              hint="占位符：{label} {watch} {time} {tz} {summary} {items}"
              className="col-span-2"
            >
              <textarea
                rows={5}
                className={`${inputCls} font-mono text-xs`}
                value={settings.template}
                placeholder="<b>ReadingSteiner</b> — {label}&#10;<b>{watch}</b>&#10;<i>{time} {tz}</i>&#10;{summary}&#10;{items}"
                onChange={(e) =>
                  setSettings({ ...settings, template: e.target.value })
                }
              />
            </Field>
          </div>

          <div className="mt-4 flex items-center justify-end gap-2">
            <Button variant="ghost" onClick={load}>
              <RefreshCw className="h-4 w-4" /> 刷新
            </Button>
            <Button onClick={handleSave} disabled={saving}>
              {saving && <Loader2 className="h-4 w-4 animate-spin" />}
              保存设置
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* 备份与恢复 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Archive className="h-5 w-5" /> 备份与恢复
          </CardTitle>
          <CardDescription>
            备份包含数据库、media 与配置。恢复需先停止 daemon，用 CLI 执行。
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" onClick={handleBackup}>
              <Archive className="h-4 w-4" /> 立即备份
            </Button>
            <Button variant="outline" onClick={handleRestore}>
              <History className="h-4 w-4" /> 恢复…
            </Button>
          </div>
          <p className="mt-3 text-xs text-muted-foreground">
            备份保存在 <code>state/backups/&lt;时间戳&gt;/</code>。恢复方式：
            <code className="ml-1">reading-steiner restore &lt;名称&gt; --config config.yaml</code>
            （需 daemon 停止）。
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
