import { useEffect, useRef, useState } from "react"
import {
  Loader2,
  RefreshCw,
  Save,
  Archive,
  History,
  Trash2,
  Upload,
  Clock,
  Server,
  Boxes,
} from "lucide-react"
import { api, type EditableSettings, type TagConfig } from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
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
  const [serverTimeUtc, setServerTimeUtc] = useState<string | null>(null)
  const [serverTz, setServerTz] = useState("UTC")
  const [browserNow, setBrowserNow] = useState(new Date())
  const [backups, setBackups] = useState<{ name: string; has_zip: boolean }[]>([])
  // 分组（标签）管理
  const [tags, setTags] = useState<TagConfig[]>([])
  const [tagSaving, setTagSaving] = useState(false)
  const [tagNotice, setTagNotice] = useState<string | null>(null)
  const [newTagName, setNewTagName] = useState("")

  async function load() {
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
  }

  async function loadBackups() {
    try {
      const res = await api.listBackups()
      setBackups(res.backups ?? [])
    } catch (e) {
      setError((e as Error).message)
    }
  }

  async function loadTags() {
    try {
      setTags(await api.listTags())
    } catch (e) {
      setError((e as Error).message)
    }
  }

  useEffect(() => {
    load()
    loadBackups()
    loadTags()
  }, [])

  // 浏览器本地时间持续刷新
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

  function updateTagLocal(name: string, patch: Partial<TagConfig>) {
    setTags((prev) => prev.map((t) => (t.name === name ? { ...t, ...patch } : t)))
  }

  async function handleSaveTag(tag: TagConfig) {
    setTagSaving(true)
    setTagNotice(null)
    setError(null)
    try {
      await api.updateTag(tag.name, tag)
      setTagNotice(`已保存分组「${tag.name}」的设置。`)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setTagSaving(false)
    }
  }

  async function handleAddTag() {
    const name = newTagName.trim()
    if (!name) {
      setError("分组名称不能为空")
      return
    }
    if (tags.some((t) => t.name === name)) {
      setError(`分组「${name}」已存在`)
      return
    }
    setTagSaving(true)
    setTagNotice(null)
    setError(null)
    try {
      await api.updateTag(name, {
        name,
        history_limit: 0,
        notify_url: "",
        extract: null,
      })
      setTags((prev) => [
        ...prev,
        { name, history_limit: 0, notify_url: "", extract: null },
      ])
      setNewTagName("")
      setTagNotice(`已创建分组「${name}」。`)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setTagSaving(false)
    }
  }

  async function handleDeleteTag(name: string) {
    setTagNotice(null)
    setError(null)
    if (!window.confirm(`确定要删除分组「${name}」的设置吗？该标签对应的监控源将恢复为使用自身设置。`)) return
    try {
      await api.deleteTag(name)
      setTags((prev) => prev.filter((t) => t.name !== name))
      setTagNotice(`已删除分组「${name}」的设置。`)
    } catch (e) {
      setError((e as Error).message)
    }
  }

  async function handleBackup() {
    setNotice(null)
    setError(null)
    try {
      const res = await api.createBackup()
      setNotice(`备份已创建：${res.name}（${res.path}）`)
      loadBackups()
    } catch (e) {
      setError((e as Error).message)
    }
  }

  async function handleRestore(name: string) {
    setNotice(null)
    setError(null)
    if (!window.confirm(`确定要从备份 ${name} 恢复吗？当前数据库与 media 将被覆盖。`)) return
    try {
      const res = (await api.restoreBackup(name)) as { error?: string }
      if (res.error) {
        setError(res.error)
      } else {
        setNotice(`已从备份 ${name} 在线恢复。`)
      }
    } catch (e) {
      setError((e as Error).message)
    }
  }

  async function handleDeleteBackup(name: string) {
    setNotice(null)
    setError(null)
    if (!window.confirm(`确定要删除备份 ${name} 吗？此操作不可撤销。`)) return
    try {
      await api.deleteBackup(name)
      setNotice(`已删除备份 ${name}。`)
      loadBackups()
    } catch (e) {
      setError((e as Error).message)
    }
  }

  const fileInputRef = useRef<HTMLInputElement>(null)

  async function handleUploadZip(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    e.target.value = "" // 允许重复选择同一文件
    if (!file) return
    setNotice(null)
    setError(null)
    if (!file.name.toLowerCase().endsWith(".zip")) {
      setError("请选择 .zip 备份包文件")
      return
    }
    if (!window.confirm(`确定要从上传的 zip 备份（${file.name}）恢复吗？当前数据库与 media 将被覆盖。`)) return
    try {
      await api.restoreFromZip(file)
      setNotice("已从上传的 zip 备份在线恢复。")
      loadBackups()
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
              {serverTimeUtc ? (
                <span>{serverTimeUtc.replace("T", " ").slice(0, 19)} UTC</span>
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
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-4">
            <Field label="抓取并发数">
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
            <Field
              label="默认 cron 表达式"
              hint="新建监控源未单独配置时使用；留空回退到每小时（0 * * * *）"
            >
              <input
                className={inputCls}
                value={settings.default_cron}
                placeholder="0 * * * *"
                onChange={(e) =>
                  setSettings({ ...settings, default_cron: e.target.value })
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
            <Field label="调度器时区" hint="IANA 名称，如 Asia/Shanghai；留空使用系统本地时区">
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
            <Field
              label="Telegram 通知目标"
              hint="格式：tgram://bottoken/ChatID1/ChatID2，编码 bot token 与一个或多个接收 chat id"
              className="col-span-2"
            >
              <input
                className={inputCls}
                value={settings.telegram_url}
                placeholder="tgram://123456:ABC/12345/67890"
                onChange={(e) =>
                  setSettings({ ...settings, telegram_url: e.target.value })
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

      {/* 分组管理 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Boxes className="h-5 w-5" /> 分组管理
          </CardTitle>
        </CardHeader>
        <CardContent>
          {tagNotice && <p className="mb-3 text-sm text-green-600">{tagNotice}</p>}
          <div className="mb-4 flex max-w-md items-center gap-2">
            <input
              type="text"
              className={inputCls}
              placeholder="分组名称，如 news"
              value={newTagName}
              onChange={(e) => setNewTagName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleAddTag()
              }}
            />
            <Button size="sm" disabled={tagSaving} onClick={handleAddTag}>
              新建分组
            </Button>
          </div>
          {tags.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              暂无分组。可在监控源的“标签”字段中填写标签，然后回来为对应分组配置历史保留 / 默认提取 / 通知目标。
            </p>
          ) : (
            <div className="grid gap-3 md:grid-cols-2">
              {tags.map((tag) => (
                <div
                  key={tag.name}
                  className="rounded-lg border p-4"
                >
                  <div className="mb-3 flex items-center justify-between">
                    <span className="font-medium">{tag.name}</span>
                    <div className="flex items-center gap-1">
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={tagSaving}
                        onClick={() => handleSaveTag(tag)}
                      >
                        保存
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="text-destructive hover:text-destructive"
                        disabled={tagSaving}
                        onClick={() => handleDeleteTag(tag.name)}
                      >
                        <Trash2 className="h-3.5 w-3.5" /> 删除
                      </Button>
                    </div>
                  </div>
                  <div className="space-y-3">
                    <Field label="每个源保留历史条数" hint="0 表示不限制，使用全局设置">
                      <input
                        type="number"
                        min={0}
                        className="w-24 rounded-md border border-input bg-background px-2 py-1 text-sm"
                        value={tag.history_limit}
                        onChange={(e) =>
                          updateTagLocal(tag.name, {
                            history_limit: Number(e.target.value),
                          })
                        }
                      />
                    </Field>
                    <Field label="通知目标" hint="留空沿用全局通知目标">
                      <input
                        type="text"
                        className={`${inputCls} font-mono`}
                        value={tag.notify_url ?? ""}
                        placeholder="tgram://bot/ChatID"
                        onChange={(e) =>
                          updateTagLocal(tag.name, { notify_url: e.target.value })
                        }
                      />
                    </Field>
                    <Field label="默认内容提取" hint="分组下的监控源开启「跟随分组」时沿用">
                      <div className="space-y-2">
                        <select
                          className={inputCls}
                          value={tag.extract?.type ?? "inherit"}
                          onChange={(e) => {
                            const v = e.target.value as "inherit" | "text" | "items"
                            if (v === "inherit") {
                              updateTagLocal(tag.name, { extract: null })
                            } else if (v === "text") {
                              updateTagLocal(tag.name, { extract: { type: "text" } })
                            } else {
                              updateTagLocal(tag.name, {
                                extract: {
                                  type: "items",
                                  selector: { kind: "css", selector: "" },
                                },
                              })
                            }
                          }}
                        >
                          <option value="inherit">不覆盖，沿用源自身</option>
                          <option value="text">整页文本</option>
                          <option value="items">结构化提取</option>
                        </select>
                        {tag.extract?.type === "items" && (
                          <input
                            type="text"
                            className={`${inputCls} font-mono`}
                            value={
                              tag.extract?.selector &&
                              "selector" in tag.extract.selector
                                ? tag.extract.selector.selector
                                : tag.extract?.selector && "path" in tag.extract.selector
                                  ? tag.extract.selector.path
                                  : ""
                            }
                            placeholder="选择器，如 .product"
                            onChange={(e) => {
                              const ex = tag.extract
                              const isItems = !!ex && "selector" in ex
                              updateTagLocal(tag.name, {
                                extract: {
                                  type: "items",
                                  selector:
                                    isItems && ex.selector && "path" in ex.selector
                                      ? { kind: "json_path", path: e.target.value }
                                      : { kind: "css", selector: e.target.value },
                                },
                              })
                            }}
                          />
                        )}
                      </div>
                    </Field>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 备份与恢复 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Archive className="h-5 w-5" /> 备份与恢复
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" onClick={handleBackup}>
              <Archive className="h-4 w-4" /> 立即备份
            </Button>
            <Button
              variant="outline"
              onClick={() => fileInputRef.current?.click()}
            >
              <Upload className="h-4 w-4" /> 上传 zip 恢复
            </Button>
            <Button variant="ghost" size="sm" onClick={loadBackups}>
              <RefreshCw className="h-3.5 w-3.5" /> 刷新列表
            </Button>
            <input
              ref={fileInputRef}
              type="file"
              accept=".zip,application/zip"
              className="hidden"
              onChange={handleUploadZip}
            />
          </div>

          {backups.length > 0 && (
            <div className="mt-4 divide-y rounded-md border">
              {backups.map((b) => (
                <div
                  key={b.name}
                  className="flex items-center justify-between gap-2 px-3 py-2 text-sm"
                >
                  <span className="font-mono text-xs">{b.name}</span>
                  <div className="flex items-center gap-1">
                    <a href={api.downloadBackup(b.name)} download>
                      <Button variant="outline" size="sm">
                        下载 zip
                      </Button>
                    </a>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleRestore(b.name)}
                    >
                      <History className="h-3.5 w-3.5" /> 恢复
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-destructive hover:text-destructive"
                      onClick={() => handleDeleteBackup(b.name)}
                    >
                      <Trash2 className="h-3.5 w-3.5" /> 删除
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
          {backups.length === 0 && !loading && (
            <p className="mt-3 text-xs text-muted-foreground">暂无备份。</p>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
