import { NavLink, Outlet, useLocation } from "react-router-dom"
import {
  Radio,
  Boxes,
  Settings as SettingsIcon,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { Badge } from "@/components/ui/badge"

const nav = [
  { to: "/sources", label: "监控源", icon: Radio },
  { to: "/settings", label: "设置", icon: SettingsIcon },
]

export function Layout() {
  const location = useLocation()
  const current = nav.find((n) => location.pathname.startsWith(n.to))

  return (
    <div className="flex min-h-screen">
      <aside className="w-60 border-r bg-muted/40 p-4">
        <div className="mb-6 flex items-center gap-2 px-2">
          <img
            src="/favicon-32x32.png"
            alt="ReadingSteiner"
            className="h-7 w-7 rounded-lg"
          />
          <div>
            <div className="text-sm font-semibold leading-none">
              ReadingSteiner
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              Web 控制台
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
                    "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                    isActive
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:bg-accent hover:text-foreground"
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {item.label}
              </NavLink>
            )
          })}
        </nav>
        <div className="mt-6 flex items-center gap-2 px-2 text-xs text-muted-foreground">
          <Boxes className="h-3.5 w-3.5" />
          <span>v0.1.0</span>
        </div>
      </aside>

      <main className="flex-1 p-6">
        <div className="mb-4 flex items-center gap-2">
          <Badge variant="outline">{current?.label ?? ""}</Badge>
        </div>
        <Outlet />
      </main>
    </div>
  )
}
