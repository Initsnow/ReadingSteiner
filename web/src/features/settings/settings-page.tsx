import { useCallback, useEffect, useState } from "react"
import { api, type EditableSettings, type TagConfig } from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { ErrorText, Loading, NoticeText } from "@/components/ui/feedback"
import { BackupCard, useBackups } from "./backup-card"
import { GlobalSettingsCard } from "./global-settings-card"
import { ServerTimeCard } from "./server-time-card"
import { TagsCard } from "./tags-card"

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

  const [tags, setTags] = useState<TagConfig[]>([])
  const [tagSaving, setTagSaving] = useState(false)
  const [tagNotice, setTagNotice] = useState<string | null>(null)

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

  const loadTags = useCallback(async () => {
    try {
      setTags(await api.listTags())
    } catch (e) {
      setError((e as Error).message)
    }
  }, [])

  useEffect(() => {
    load()
    loadTags()
  }, [load, loadTags])

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

  function updateTagLocal(name: string, patch: Partial<TagConfig>) {
    setTags((prev) => prev.map((t) => (t.name === name ? { ...t, ...patch } : t)))
  }

  async function handleSaveTag(tag: TagConfig) {
    setTagSaving(true)
    setTagNotice(null)
    setError(null)
    try {
      await api.updateTag(tag.name, tag)
      setTagNotice(`已保存分组「${tag.name}」。`)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setTagSaving(false)
    }
  }

  async function handleAddTag(name: string) {
    const trimmed = name.trim()
    if (!trimmed) {
      setError("分组名称不能为空")
      return
    }
    if (tags.some((t) => t.name === trimmed)) {
      setError(`分组「${trimmed}」已存在`)
      return
    }
    setTagSaving(true)
    setTagNotice(null)
    setError(null)
    try {
      await api.updateTag(trimmed, {
        name: trimmed,
        history_limit: 0,
        notify_url: "",
        extract: null,
      })
      setTags((prev) => [
        ...prev,
        { name: trimmed, history_limit: 0, notify_url: "", extract: null },
      ])
      setTagNotice(`已创建分组「${trimmed}」。`)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setTagSaving(false)
    }
  }

  async function handleDeleteTag(name: string) {
    setTagNotice(null)
    setError(null)
    if (!window.confirm(`确定要删除分组「${name}」吗？该标签下的监控源将恢复为使用自身设置。`))
      return
    try {
      await api.deleteTag(name)
      setTags((prev) => prev.filter((t) => t.name !== name))
      setTagNotice(`已删除分组「${name}」。`)
    } catch (e) {
      setError((e as Error).message)
    }
  }

  if (loading) return <Loading text="加载设置…" />
  if (!settings) return <ErrorText>加载失败：{error}</ErrorText>

  return (
    <div className="space-y-4">
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
        onChange={(patch) => setSettings((prev) => (prev ? { ...prev, ...patch } : prev))}
        onSave={handleSave}
        onReload={load}
      />

      <TagsCard
        tags={tags}
        busy={tagSaving}
        notice={tagNotice}
        error={null}
        onChange={updateTagLocal}
        onSave={handleSaveTag}
        onAdd={handleAddTag}
        onDelete={handleDeleteTag}
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
