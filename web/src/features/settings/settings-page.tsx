import { useCallback, useEffect, useState } from "react"
import { api, type EditableSettings } from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { ErrorText, Loading, NoticeText } from "@/components/ui/feedback"
import { BackupCard, useBackups } from "./backup-card"
import { GlobalSettingsCard } from "./global-settings-card"
import { ServerTimeCard } from "./server-time-card"

export function SettingsPage() {
  const [settings, setSettings] = useState<EditableSettings | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const [serverTime, setServerTime] = useState<string | null>(null)
  const [serverTimeUtc, setServerTimeUtc] = useState<string | null>(null)
  const [serverTz, setServerTz] = useState("UTC")
  const [browserNow, setBrowserNow] = useState(new Date())

  const [backups, reloadBackups] = useBackups(setError)

  const load = useCallback(async () => {
    try {
      const [s, st] = await Promise.all([api.getSettings(), api.status()])
      setSettings(s)
      setServerTz(st.timezone || "UTC")
      setServerTime(st.server_time_local)
      setServerTimeUtc(st.server_time_utc)
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  // 浏览器本地时间持续刷新，便于与服务器时间对照。
  useEffect(() => {
    const timer = setInterval(() => setBrowserNow(new Date()), 1000)
    return () => clearInterval(timer)
  }, [])

  async function handleSave() {
    if (!settings) return
    // 全局默认 cron 留空回退到每小时；非空时校验格式。
    if (settings.default_cron.trim()) {
      const cronErr = validateCron(settings.default_cron)
      if (cronErr) {
        setError(cronErr)
        return
      }
    }
    setSaving(true)
    setNotice(null)
    setError(null)
    try {
      await api.updateSettings(settings)
      setNotice("已保存并即时生效。")
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <Loading text="加载设置…" />
  if (!settings) return <ErrorText>加载失败：{error}</ErrorText>

  return (
    <div className="space-y-6 max-w-5xl">
      <div>
        <h1 className="text-xl font-bold tracking-tight text-zinc-100">
          系统设置
        </h1>
        <p className="text-sm text-zinc-400">
          配置全局并发队列、默认抓取周期与 UA、通知目标，以及数据备份与恢复。
        </p>
      </div>

      <ServerTimeCard
        serverTz={serverTz}
        serverTimeLocal={serverTime}
        serverTimeUtc={serverTimeUtc}
        browserNow={browserNow}
      />

      <ErrorText>{error}</ErrorText>
      <NoticeText>{notice}</NoticeText>

      <GlobalSettingsCard
        settings={settings}
        saving={saving}
        onChange={(patch) =>
          setSettings((prev) => (prev ? { ...prev, ...patch } : prev))
        }
        onSave={handleSave}
        onReload={load}
      />

      <BackupCard
        backups={backups}
        onReload={reloadBackups}
        onError={setError}
        onNotice={setNotice}
      />
    </div>
  )
}
