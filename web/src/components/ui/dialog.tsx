import { useEffect, type ReactNode } from "react"
import { createPortal } from "react-dom"
import { X } from "lucide-react"
import { Button } from "@/components/ui/button"

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
  width?: "md" | "lg" | "xl" | "2xl"
  footer?: ReactNode
  children: ReactNode
}) {
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!open) return null

  const widthClass =
    width === "2xl"
      ? "max-w-5xl"
      : width === "xl"
      ? "max-w-4xl"
      : width === "lg"
      ? "max-w-3xl"
      : "max-w-2xl"

  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center overflow-y-auto bg-black/85 backdrop-blur-sm p-4 sm:p-6"
      onClick={onClose}
    >
      <div
        className={`mt-4 w-full ${widthClass} rounded-xl border border-zinc-800 bg-zinc-900/95 text-zinc-100 shadow-2xl p-6 backdrop-blur-md`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-zinc-800 pb-3 mb-4">
          <h2 className="text-base font-semibold tracking-tight text-zinc-100">{title}</h2>
          <Button
            size="icon"
            variant="ghost"
            onClick={onClose}
            className="h-8 w-8 text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
        {children}
        {footer && (
          <div className="mt-5 flex items-center justify-end gap-2 border-t border-zinc-800 pt-4">
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  )
}
