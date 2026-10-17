import { Loader2, RefreshCw, Save, ShieldAlert, Sliders, Bell, Cpu, Lock } from "lucide-react"
import type { EditableSettings } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card"
import { Field, Input } from "@/components/ui/field"

/** 全局设置表单：单一权威源为 config.yaml。 */
export function GlobalSettingsCard({
  settings,
  saving,
  onSave,
  onChange,
  onReload,
}: {
  settings: EditableSettings
  saving: boolean
  onSave: () => void
  onChange: (patch: Partial<EditableSettings>) => void
  onReload: () => void
}) {
  const isReadonly = settings.readonly

  return (
    <Card className="border border-zinc-800 bg-zinc-900/60">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2 text-base text-zinc-100">
            <Save className="h-4 w-4 text-emerald-400" /> 全局运行参数
          </CardTitle>
          {isReadonly && (
            <div className="flex items-center gap-1.5 rounded-full border border-amber-500/30 bg-amber-500/10 px-2.5 py-0.5 text-xs font-medium text-amber-300">
              <Lock className="h-3 w-3" /> 只读模式（NixOS / 声明式管理）
            </div>
          )}
        </div>
        <CardDescription className="text-xs text-zinc-400">
          {isReadonly
            ? "当前配置文件由 NixOS 或只读系统管理，以下参数仅供查看，请在声明式配置文件中修改。"
            : "参数直接持久化至 config.yaml 并即时热生效，无需重启后台守护进程。"}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        {/* 1. 抓取与并发 */}
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-zinc-300 border-b border-zinc-800/80 pb-1.5">
            <Sliders className="h-3.5 w-3.5 text-zinc-400" />
            并发队列与网络
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <NumberField
              label="抓取并发数"
              hint="后台同时执行的最大抓取任务数"
              value={settings.concurrency}
              disabled={isReadonly}
              onChange={(concurrency) => onChange({ concurrency })}
            />
            <NumberField
              label="队列容量"
              hint="调度任务等待缓冲上限"
              value={settings.queue_capacity}
              disabled={isReadonly}
              onChange={(queue_capacity) => onChange({ queue_capacity })}
            />
            <NumberField
              label="默认请求超时（秒）"
              hint="各监控源未单独设置时的超时"
              value={settings.default_timeout_secs}
              disabled={isReadonly}
              onChange={(default_timeout_secs) => onChange({ default_timeout_secs })}
            />
          </div>
        </div>

        {/* 2. Chrome CDP 浏览器渲染 */}
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-zinc-300 border-b border-zinc-800/80 pb-1.5">
            <Cpu className="h-3.5 w-3.5 text-amber-400" />
            Chrome CDP 浏览器渲染服务
          </div>
          <div className="space-y-3">
            <label className="flex items-center gap-2 cursor-pointer text-xs text-zinc-300">
              <input
                type="checkbox"
                checked={settings.browser_enabled}
                disabled={isReadonly}
                onChange={(e) => onChange({ browser_enabled: e.target.checked })}
                className="h-4 w-4 rounded border-zinc-700 bg-zinc-950 text-emerald-500 focus:ring-0 focus:ring-offset-0 disabled:opacity-50"
              />
              <span className="font-medium text-zinc-200">启用 Chrome CDP 渲染引擎</span>
            </label>

            <Field
              label="Chrome CDP 调试地址"
              hint="外部无头 Chrome 的远程调试 WebSocket 或 HTTP 地址（如 http://127.0.0.1:9222 或 ws://127.0.0.1:9222/...）"
            >
              <Input
                value={settings.browser_cdp_url ?? ""}
                placeholder="http://127.0.0.1:9222"
                className="font-mono text-xs"
                disabled={isReadonly}
                onChange={(e) => onChange({ browser_cdp_url: e.target.value })}
              />
            </Field>
          </div>
        </div>

        {/* 3. 调度与生命周期 */}
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-zinc-300 border-b border-zinc-800/80 pb-1.5">
            <ShieldAlert className="h-3.5 w-3.5 text-zinc-400" />
            调度周期与数据保留
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <Field
              label="默认 cron 表达式"
              hint="新建监控源未单独配置时使用；留空回退到每小时（0 * * * *）"
            >
              <Input
                value={settings.default_cron}
                placeholder="0 * * * *"
                className="font-mono text-xs"
                disabled={isReadonly}
                onChange={(e) => onChange({ default_cron: e.target.value })}
              />
            </Field>

            <Field
              label="调度器时区"
              hint="IANA 时区名称，如 Asia/Shanghai；留空使用系统本地时区"
            >
              <Input
                value={settings.timezone}
                placeholder="Asia/Shanghai 或 UTC"
                className="font-mono text-xs"
                disabled={isReadonly}
                onChange={(e) => onChange({ timezone: e.target.value })}
              />
            </Field>

            <Field label="默认 User-Agent" hint="留空使用内置默认值" className="sm:col-span-2">
              <Input
                value={settings.default_user_agent}
                placeholder="Mozilla/5.0 ..."
                className="font-mono text-xs"
                disabled={isReadonly}
                onChange={(e) => onChange({ default_user_agent: e.target.value })}
              />
            </Field>

            <NumberField
              label="历史条数保留上限"
              hint="0 表示不限制存储条数"
              value={settings.history_limit_per_source}
              disabled={isReadonly}
              onChange={(history_limit_per_source) =>
                onChange({ history_limit_per_source })
              }
            />

            <NumberField
              label="连续失败通知阈值"
              hint="连续报错达到该次数触发通知；0 表示禁用失败告警"
              value={settings.failure_notify_threshold}
              disabled={isReadonly}
              onChange={(failure_notify_threshold) =>
                onChange({ failure_notify_threshold })
              }
            />
          </div>
        </div>

        {/* 4. 通知与模板 */}
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-zinc-300 border-b border-zinc-800/80 pb-1.5">
            <Bell className="h-3.5 w-3.5 text-zinc-400" />
            Telegram 通知配置
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div className="sm:col-span-2">
              <Field
                label="Telegram 通知目标"
                hint={
                  settings.telegram_url_file
                    ? `已由外部密钥文件托管 (${settings.telegram_url_file})，敏感 Token 受保护。`
                    : "格式：tgram://bottoken/ChatID1/ChatID2"
                }
              >
                <Input
                  value={
                    settings.telegram_url_file
                      ? "••••••••••••••••••••••••"
                      : settings.telegram_url
                  }
                  placeholder="tgram://123456:ABC/12345/67890"
                  className="font-mono text-xs"
                  disabled={isReadonly || !!settings.telegram_url_file}
                  onChange={(e) => onChange({ telegram_url: e.target.value })}
                />
              </Field>
            </div>

            <div>
              <NumberField
                label="单事件最多附带图片数"
                hint="多图聚合发送上限"
                value={settings.max_images_per_event}
                disabled={isReadonly}
                onChange={(max_images_per_event) =>
                  onChange({ max_images_per_event })
                }
              />
            </div>

            <div className="sm:col-span-3">
              <Field
                label="变更通知 HTML 模板"
                hint="支持占位符：{label} {watch} {time} {tz} {summary} {items}"
              >
                <textarea
                  rows={4}
                  disabled={isReadonly}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-600 disabled:opacity-50"
                  value={settings.template}
                  placeholder="<b>ReadingSteiner</b> — {label}&#10;<b>{watch}</b>&#10;<i>{time} {tz}</i>&#10;{summary}&#10;{items}"
                  onChange={(e) => onChange({ template: e.target.value })}
                />
              </Field>
            </div>
          </div>
        </div>

        <div className="mt-4 flex items-center justify-end gap-2 border-t border-zinc-800/80 pt-4">
          <Button variant="ghost" onClick={onReload} className="text-xs">
            <RefreshCw className="h-3.5 w-3.5" /> 刷新参数
          </Button>
          {!isReadonly && (
            <Button onClick={onSave} disabled={saving} className="text-xs gap-1.5">
              {saving ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Save className="h-3.5 w-3.5" />
              )}
              保存配置
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

function NumberField({
  label,
  hint,
  value,
  disabled,
  onChange,
}: {
  label: string
  hint?: string
  value: number
  disabled?: boolean
  onChange: (value: number) => void
}) {
  return (
    <Field label={label} hint={hint}>
      <Input
        type="number"
        min={0}
        value={value}
        disabled={disabled}
        className="font-mono text-xs"
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </Field>
  )
}
