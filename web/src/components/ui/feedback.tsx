import { Loader2 } from "lucide-react"

/** 统一的错误提示条。 */
export function ErrorText({ children }: { children: React.ReactNode }) {
  if (!children) return null
  return (
    <p className="rounded-md border border-rose-900/40 bg-rose-950/20 px-3 py-2 text-xs text-rose-400 font-medium">
      {children}
    </p>
  )
}

/** 统一的成功提示条。 */
export function NoticeText({ children }: { children: React.ReactNode }) {
  if (!children) return null
  return (
    <p className="rounded-md border border-emerald-900/40 bg-emerald-950/20 px-3 py-2 text-xs text-emerald-400 font-medium">
      {children}
    </p>
  )
}

/** 加载态：旋转图标 + 文案。 */
export function Loading({ text = "加载中..." }: { text?: string }) {
  return (
    <div className="flex items-center justify-center gap-2 text-zinc-400 py-6 text-xs">
      <Loader2 className="h-4 w-4 animate-spin text-primary" /> {text}
    </div>
  )
}

/** 空态占位卡片。 */
export function EmptyState({
  title,
  description,
  children,
}: {
  title?: string
  description?: string
  children?: React.ReactNode
}) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 py-12 px-4 text-center text-zinc-400">
      {title && <div className="text-sm font-semibold text-zinc-200">{title}</div>}
      {description && (
        <div className="text-xs text-zinc-500 mt-1 max-w-md mx-auto">
          {description}
        </div>
      )}
      {children}
    </div>
  )
}

/** 筛选用的胶囊按钮组。 */
export function FilterPills<T extends string | null>({
  label,
  options,
  value,
  onChange,
}: {
  label: string
  options: { value: T; label: string }[]
  value: T
  onChange: (value: T) => void
}) {
  return (
    <>
      <span className="text-xs text-zinc-400">{label}：</span>
      {options.map((opt) => (
        <button
          key={opt.label}
          onClick={() => onChange(opt.value)}
          className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
            value === opt.value
              ? "bg-primary text-primary-foreground font-medium"
              : "bg-zinc-800 text-zinc-400 hover:bg-zinc-700"
          }`}
        >
          {opt.label}
        </button>
      ))}
    </>
  )
}
