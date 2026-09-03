import { CheckCircle2, Loader2 } from "lucide-react"
import type { TestSourceResult } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Dialog } from "@/components/ui/dialog"
import { ErrorText } from "@/components/ui/feedback"

/** 测试监控源的结果：抓取概况 + 提取到的条目预览。 */
export function TestResultDialog({
  open,
  result,
  error,
  testing,
  onClose,
}: {
  open: boolean
  result: TestSourceResult | null
  error: string | null
  testing: boolean
  onClose: () => void
}) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      width="lg"
      title={
        <>
          测试监控源
          {testing && <Loader2 className="ml-2 inline h-4 w-4 animate-spin" />}
        </>
      }
      footer={
        <Button variant="ghost" onClick={onClose}>
          关闭
        </Button>
      }
    >
      {error && (
        <div className="mt-2">
          <ErrorText>{error}</ErrorText>
        </div>
      )}

      {result && (
        <div className="mt-4 space-y-4">
          <div className="flex flex-wrap items-center gap-4 rounded-md border bg-muted/40 p-3 text-sm">
            {result.status !== undefined && (
              <span
                className={
                  result.status < 400
                    ? "flex items-center gap-1 text-green-600"
                    : "flex items-center gap-1 text-destructive"
                }
              >
                <CheckCircle2 className="h-4 w-4" /> HTTP {result.status}
              </span>
            )}
            {result.engine && <span>engine: {result.engine}</span>}
            {result.duration_ms !== undefined && <span>{result.duration_ms}ms</span>}
            {result.text_len !== undefined && <span>text: {result.text_len} chars</span>}
            <span className="break-all">{result.final_url}</span>
          </div>

          <div>
            <div className="text-xs font-medium text-muted-foreground">
              fingerprint:{" "}
              <code className="break-all">{result.fingerprint ?? "-"}</code>
            </div>
            <div className="mt-2 text-xs font-medium text-muted-foreground">
              提取到 {result.items?.length ?? 0} 个条目
            </div>
            {result.items && result.items.length > 0 ? (
              <div className="mt-2 max-h-80 overflow-y-auto rounded-md border">
                <table className="w-full text-left text-xs">
                  <thead className="sticky top-0 bg-muted">
                    <tr>
                      <th className="px-3 py-2">stable_id</th>
                      <th className="px-3 py-2">fields</th>
                      <th className="px-3 py-2">text</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.items.map((item, i) => (
                      <tr key={i} className="border-t">
                        <td className="px-3 py-2 font-medium">{item.stable_id}</td>
                        <td className="break-all px-3 py-2">
                          {Object.entries(item.fields)
                            .map(([k, v]) => `${k}=${v}`)
                            .join(" | ") || "-"}
                        </td>
                        <td className="max-w-[200px] truncate px-3 py-2">
                          {item.text || "-"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="mt-2 text-xs text-muted-foreground">
                未提取到条目（可能未配置选择器，或页面为空）。
              </p>
            )}
          </div>
        </div>
      )}
    </Dialog>
  )
}
