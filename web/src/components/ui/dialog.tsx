import { useEffect, type ReactNode } from "react"
import { X } from "lucide-react"
import { Button } from "@/components/ui/button"

/**
 * 模态框：统一遮罩、Esc 关闭、标题栏与尺寸。
 * 原先「添加/编辑」与「测试结果」两处各写了一遍 fixed inset-0 样板。
 */
export function Dialog({
  open,
  onClose,
  title,
  width = "md",
  footer,
  children,
}: {
  open: boolean
  onClose: () => void
  title: ReactNode
  /** 内容最大宽度。 */
  width?: "md" | "lg"
  footer?: ReactNode
  children: ReactNode
}) {
  // Esc 关闭；仅当对话框打开时注册。
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-black/50 p-4"
      // 点击遮罩关闭；内容区的点击已在子层 stopPropagation。
      onClick={onClose}
    >
      <div
        className={`mt-8 w-full ${
          width === "lg" ? "max-w-3xl" : "max-w-2xl"
        } rounded-xl border bg-card p-6 shadow-lg`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">{title}</h2>
          <Button size="icon" variant="ghost" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </div>
        {children}
        {footer && (
          <div className="mt-6 flex items-center justify-end gap-2">
            {footer}
          </div>
        )}
      </div>
    </div>
  )
}
