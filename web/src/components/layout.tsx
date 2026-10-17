import { NavLink, Outlet, useLocation } from "react-router-dom"
import { Activity, Layers, Radio, Settings as SettingsIcon } from "lucide-react"
import { cn } from "@/lib/utils"

declare const __APP_VERSION__: string

const nav = [
  { to: "/sources", label: "监控源", icon: Radio },
  { to: "/groups", label: "预设分组", icon: Layers },
  { to: "/events", label: "变更事件", icon: Activity },
  { to: "/settings", label: "系统设置", icon: SettingsIcon },
]

export function Layout() {
  const location = useLocation()

  return (
    <div className="flex min-h-screen bg-zinc-950 text-zinc-100 font-sans antialiased">
      {/* 侧边栏 */}
      <aside className="flex w-56 flex-col border-r border-zinc-800/80 bg-zinc-900/60 p-4 backdrop-blur-md">
        <div className="mb-6 flex items-center gap-2.5 px-2">
          <img
            src="/favicon-32x32.png"
            alt="ReadingSteiner"
            className="h-7 w-7 rounded-lg ring-1 ring-zinc-700/60"
          />
          <div>
            <div className="text-sm font-semibold tracking-wide text-zinc-100">
              ReadingSteiner
            </div>
            <div className="text-[10px] font-mono text-zinc-400">
              Change Watchdog
            </div>
          </div>
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
                    "flex items-center gap-3 rounded-md px-3 py-2 text-xs font-medium transition-all",
                    isActive
                      ? "bg-zinc-800 text-zinc-100 shadow-xs border border-zinc-700/60 font-semibold"
                      : "text-zinc-400 hover:bg-zinc-800/40 hover:text-zinc-200",
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {item.label}
              </NavLink>
            )
          })}
        </nav>

        {/* 底部紧凑版本信息 */}
        <div className="mt-auto pt-4 border-t border-zinc-800/60">
          <div className="px-2 text-[11px] font-mono text-zinc-500">
            v{typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "0.1.0"}
          </div>
        </div>
      </aside>

      {/* 主工作区 */}
      <div className="flex-1 flex flex-col min-w-0">
        <header className="sticky top-0 z-30 flex h-14 items-center justify-between border-b border-zinc-800/80 bg-zinc-950/80 px-6 backdrop-blur-md">
          <div className="flex items-center gap-3">
            <span className="text-xs font-mono text-zinc-400">
              {location.pathname === "/sources" && "监控源列表与控制"}
              {location.pathname === "/groups" && "预设模板与站点配置"}
              {location.pathname === "/events" && "变更事件流与差异比对"}
              {location.pathname === "/settings" && "全局运行参数与备份恢复"}
            </span>
          </div>
        </header>

        <main className="flex-1 p-6 overflow-y-auto">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
