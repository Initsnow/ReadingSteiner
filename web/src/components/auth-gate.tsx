import { useEffect, useState, type ReactNode } from "react"
import { Lock } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  getAuthToken,
  setAuthToken,
  verifyToken,
  onUnauthorized,
  checkAuthRequired,
} from "@/lib/api"

const inputCls =
  "w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"

/**
 * 鉴权门：后端配置了 `web.auth_token` 时，未携带有效 token 的请求会被后端 401 拒绝。
 * 该组件在首次进入或收到 401 时展示 Token 输入页，校验通过后再渲染子应用。
 * 后端未启用鉴权（token 为空）时直接放行，不影响原有本地使用方式。
 */
export function AuthGate({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(() => !!getAuthToken())
  const [token, setToken] = useState("")
  const [error, setError] = useState("")
  const [checking, setChecking] = useState(false)

  // 首次进入且本地无 token 时，探测后端是否需要鉴权。
  // 后端未启用 auth_token 时直接放行，不展示解锁页。
  useEffect(() => {
    if (getAuthToken()) return
    checkAuthRequired()
      .then((required) => {
        if (!required) setReady(true)
      })
      .catch(() => {
        // 网络异常（daemon 未启动等）：交由实际请求报错，先放行避免误拦。
        setReady(true)
      })
  }, [])

  useEffect(() => {
    return onUnauthorized(() => {
      setError("")
      setToken("")
      setReady(false)
    })
  }, [])

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setChecking(true)
    setError("")
    try {
      const ok = await verifyToken(token)
      if (ok) {
        setAuthToken(token)
        setReady(true)
      } else {
        setError("鉴权 Token 无效，请重试")
      }
    } catch {
      setError("无法连接服务，请确认 daemon 已启动")
    } finally {
      setChecking(false)
    }
  }

  if (ready) {
    return <>{children}</>
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-muted/30 p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Lock className="h-4 w-4" />
            需要鉴权
          </CardTitle>
          <CardDescription>请输入访问 Token 解锁控制台。</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-3">
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="输入访问 Token"
              autoFocus
              className={inputCls}
            />
            {error && (
              <p className="text-sm text-destructive">{error}</p>
            )}
            <Button type="submit" className="w-full" disabled={checking || !token.trim()}>
              {checking ? "校验中…" : "解锁"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
