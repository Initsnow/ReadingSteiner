import { useCallback, useEffect, useRef, useState } from "react"
import { Archive, History, Trash2, Upload } from "lucide-react"
import { api, fetchBlobUrl } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"

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
    // 清空 value，允许重复选择同一文件。
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
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Archive className="h-5 w-5" /> 备份与恢复
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
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
            <Archive className="h-4 w-4" /> 立即备份
          </Button>
          <Button variant="outline" onClick={() => fileInputRef.current?.click()}>
            <Upload className="h-4 w-4" /> 上传 zip 恢复
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
          <p className="mt-3 text-xs text-muted-foreground">暂无备份。</p>
        ) : (
          <div className="mt-4 divide-y rounded-md border">
            {backups.map((b) => (
              <div
                key={b.name}
                className="flex items-center justify-between gap-2 px-3 py-2 text-sm"
              >
                <span className="font-mono text-xs">{b.name}</span>
                <div className="flex items-center gap-1">
                  <Button variant="outline" size="sm" onClick={() => handleDownload(b.name)}>
                    下载 zip
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      if (!window.confirm(`确定要从备份 ${b.name} 恢复吗？当前数据库与 media 将被覆盖。`))
                        return
                      run(() => api.restoreBackup(b.name), `已从备份 ${b.name} 在线恢复。`)
                    }}
                  >
                    <History className="h-3.5 w-3.5" /> 恢复
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-destructive hover:text-destructive"
                    onClick={() => {
                      if (!window.confirm(`确定要删除备份 ${b.name} 吗？此操作不可撤销。`)) return
                      run(() => api.deleteBackup(b.name), `已删除备份 ${b.name}。`, true)
                    }}
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
