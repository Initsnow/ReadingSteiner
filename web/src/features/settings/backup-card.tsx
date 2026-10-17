import { useCallback, useEffect, useRef, useState } from "react"
import { Archive, Download, History, Trash2, Upload } from "lucide-react"
import { api, fetchBlobUrl } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card"

interface Backup {
  name: string
  has_zip: boolean
}

/** 备份与恢复：一键备份、列表、zip 下载、上传恢复、删除。 */
export function BackupCard({
  backups,
  onReload,
  onError,
  onNotice,
}: {
  backups: Backup[]
  onReload: () => void
  onError: (message: string) => void
  onNotice: (message: string) => void
}) {
  const fileInputRef = useRef<HTMLInputElement>(null)

  async function run<T>(fn: () => Promise<T>, notice: string, reload = false) {
    onError("")
    onNotice("")
    try {
      await fn()
      if (notice) onNotice(notice)
      if (reload) onReload()
    } catch (e) {
      onError((e as Error).message)
    }
  }

  async function handleDownload(name: string) {
    try {
      const url = await fetchBlobUrl(`/api/backups/${encodeURIComponent(name)}/download`)
      const a = document.createElement("a")
      a.href = url
      a.download = `${name}.zip`
      document.body.appendChild(a)
      a.click()
      a.remove()
      URL.revokeObjectURL(url)
    } catch (e) {
      onError((e as Error).message)
    }
  }

  async function handleUploadZip(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    e.target.value = ""
    if (!file) return
    if (!file.name.toLowerCase().endsWith(".zip")) {
      onError("请选择 .zip 备份包文件")
      return
    }
    if (!window.confirm(`确定要从上传的 zip 备份（${file.name}）恢复吗？当前数据库与 media 将被覆盖。`))
      return
    await run(() => api.restoreFromZip(file), "已从上传的 zip 备份在线恢复。", true)
  }

  return (
    <Card className="border border-zinc-800 bg-zinc-900/60">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base text-zinc-100">
          <Archive className="h-4 w-4 text-primary" /> 数据备份与灾难恢复
        </CardTitle>
        <CardDescription className="text-xs text-zinc-400">
          生成包含 SQLite 数据库与 media 截图的完整快照，支持无缝冷热迁移。
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-wrap items-center gap-2.5">
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              run(
                async () => {
                  const res = await api.createBackup()
                  onNotice(`备份已创建：${res.name}`)
                },
                "",
                true,
              )
            }
          >
            <Archive className="h-3.5 w-3.5" /> 立即生成完整快照
          </Button>
          <Button variant="outline" size="sm" onClick={() => fileInputRef.current?.click()}>
            <Upload className="h-3.5 w-3.5" /> 上传 zip 恢复
          </Button>
          <input
            ref={fileInputRef}
            type="file"
            accept=".zip,application/zip"
            className="hidden"
            onChange={handleUploadZip}
          />
        </div>

        {backups.length === 0 ? (
          <div className="mt-4 rounded-lg border border-zinc-800/80 bg-zinc-950/40 p-4 text-center text-xs text-zinc-500">
            暂无备份文件。
          </div>
        ) : (
          <div className="mt-4 divide-y divide-zinc-800/80 rounded-lg border border-zinc-800 bg-zinc-950/60">
            {backups.map((b) => (
              <div
                key={b.name}
                className="flex items-center justify-between gap-3 px-3.5 py-2.5 text-xs"
              >
                <div className="flex items-center gap-2">
                  <Archive className="h-3.5 w-3.5 text-zinc-500" />
                  <span className="font-mono text-zinc-300">{b.name}</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-zinc-400 hover:text-zinc-100"
                    onClick={() => handleDownload(b.name)}
                    title="下载快照"
                  >
                    <Download className="h-3.5 w-3.5" /> 下载
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-amber-400 hover:text-amber-300"
                    onClick={() => {
                      if (!window.confirm(`确定要从备份 ${b.name} 恢复吗？当前数据库与 media 将被覆盖。`))
                        return
                      run(() => api.restoreBackup(b.name), `已从备份 ${b.name} 在线恢复。`)
                    }}
                    title="从快照恢复"
                  >
                    <History className="h-3.5 w-3.5" /> 恢复
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-rose-400 hover:text-rose-300"
                    onClick={() => {
                      if (!window.confirm(`确定要删除备份 ${b.name} 吗？此操作不可撤销。`)) return
                      run(() => api.deleteBackup(b.name), `已删除备份 ${b.name}。`, true)
                    }}
                    title="删除快照"
                  >
                    <Trash2 className="h-3.5 w-3.5" /> 删除
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

/** 拉取备份列表。 */
export function useBackups(
  onError: (message: string) => void,
): [Backup[], () => void] {
  const [backups, setBackups] = useState<Backup[]>([])
  const reload = useCallback(async () => {
    try {
      setBackups((await api.listBackups()).backups ?? [])
    } catch (e) {
      onError((e as Error).message)
    }
  }, [onError])

  useEffect(() => {
    reload()
  }, [reload])

  return [backups, reload]
}
