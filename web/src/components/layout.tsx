import { NavLink, Outlet } from "react-router-dom"
import { Radio, Settings as SettingsIcon } from "lucide-react"
import { cn } from "@/lib/utils"

const nav = [
  { to: "/sources", label: "监控源", icon: Radio },
  { to: "/settings", label: "设置", icon: SettingsIcon },
]

/** 侧边栏 + 内容区骨架。当前页面已在侧栏高亮，内容区不再重复标题。 */
export function Layout() {
  return (
    <div className="flex min-h-screen">
      <aside className="flex w-52 flex-col border-r bg-muted/40 p-4">
        <div className="mb-6 flex items-center gap-2 px-2">
          <img
            src="/favicon-32x32.png"
            alt="ReadingSteiner"
            className="h-7 w-7 rounded-lg"
          />
          <div className="text-sm font-semibold leading-none">ReadingSteiner</div>
        </div>
        <nav className="space-y-1">
          {nav.map((item) => {
            const Icon = item.icon
            return (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                    isActive
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:bg-accent hover:text-foreground",
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {item.label}
              </NavLink>
            )
          })}
        </nav>
        <div className="mt-auto px-2 text-xs text-muted-foreground">
          v{__APP_VERSION__}
        </div>
      </aside>

      <main className="flex-1 p-6">
        <Outlet />
      </main>
    </div>
  )
}
