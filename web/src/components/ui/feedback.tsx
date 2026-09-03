import { Loader2 } from "lucide-react"

/** 统一的错误提示条。 */
export function ErrorText({ children }: { children: React.ReactNode }) {
  if (!children) return null
  return (
    <p className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {children}
    </p>
  )
}

/** 统一的成功提示条。 */
export function NoticeText({ children }: { children: React.ReactNode }) {
  if (!children) return null
  return <p className="text-sm text-green-600">{children}</p>
}

/** 加载态：旋转图标 + 文案。 */
export function Loading({ text }: { text: string }) {
  return (
    <div className="flex items-center gap-2 text-muted-foreground">
      <Loader2 className="h-4 w-4 animate-spin" /> {text}
    </div>
  )
}

/** 空态占位卡片。 */
export function EmptyState({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-md border py-10 text-center text-sm text-muted-foreground">
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
      <span className="text-xs text-muted-foreground">{label}：</span>
      {options.map((opt) => (
        <button
          key={opt.label}
          onClick={() => onChange(opt.value)}
          className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
            value === opt.value
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground hover:bg-muted/70"
          }`}
        >
          {opt.label}
        </button>
      ))}
    </>
  )
}
