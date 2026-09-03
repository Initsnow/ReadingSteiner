import { Loader2, RefreshCw, Save } from "lucide-react"
import type { EditableSettings } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Field, Input } from "@/components/ui/field"

/** 全局设置表单：保存即热更新，无需重启。 */
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
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Save className="h-5 w-5" /> 全局设置
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 gap-4">
          <NumberField
            label="抓取并发数"
            value={settings.concurrency}
            onChange={(concurrency) => onChange({ concurrency })}
          />
          <NumberField
            label="队列容量"
            value={settings.queue_capacity}
            onChange={(queue_capacity) => onChange({ queue_capacity })}
          />
          <NumberField
            label="默认请求超时（秒）"
            value={settings.default_timeout_secs}
            onChange={(default_timeout_secs) => onChange({ default_timeout_secs })}
          />
          <Field
            label="默认 cron 表达式"
            hint="新建监控源未单独配置时使用；留空回退到每小时（0 * * * *）"
          >
            <Input
              value={settings.default_cron}
              placeholder="0 * * * *"
              onChange={(e) => onChange({ default_cron: e.target.value })}
            />
          </Field>
          <Field label="默认 User-Agent" hint="留空使用内置默认值">
            <Input
              value={settings.default_user_agent}
              onChange={(e) => onChange({ default_user_agent: e.target.value })}
            />
          </Field>
          <NumberField
            label="每个监控源保留历史条数"
            hint="0 表示不限制"
            value={settings.history_limit_per_source}
            onChange={(history_limit_per_source) =>
              onChange({ history_limit_per_source })
            }
          />
          <NumberField
            label="连续失败通知阈值"
            hint="0 表示禁用失败通知"
            value={settings.failure_notify_threshold}
            onChange={(failure_notify_threshold) =>
              onChange({ failure_notify_threshold })
            }
          />
          <Field
            label="调度器时区"
            hint="IANA 名称，如 Asia/Shanghai；留空使用系统本地时区"
          >
            <Input
              value={settings.timezone}
              onChange={(e) => onChange({ timezone: e.target.value })}
            />
          </Field>
          <NumberField
            label="单事件最多附带图片数"
            value={settings.max_images_per_event}
            onChange={(max_images_per_event) => onChange({ max_images_per_event })}
          />
          <Field
            label="Telegram 通知目标"
            hint="格式：tgram://bottoken/ChatID1/ChatID2"
            className="col-span-2"
          >
            <Input
              value={settings.telegram_url}
              placeholder="tgram://123456:ABC/12345/67890"
              onChange={(e) => onChange({ telegram_url: e.target.value })}
            />
          </Field>
          <Field
            label="变更通知模板"
            hint="占位符：{label} {watch} {time} {tz} {summary} {items}"
            className="col-span-2"
          >
            <textarea
              rows={5}
              className="w-full rounded-md border border-input bg-background px-3 py-1.5 font-mono text-xs shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              value={settings.template}
              placeholder="<b>ReadingSteiner</b> — {label}&#10;<b>{watch}</b>&#10;<i>{time} {tz}</i>&#10;{summary}&#10;{items}"
              onChange={(e) => onChange({ template: e.target.value })}
            />
          </Field>
        </div>

        <div className="mt-4 flex items-center justify-end gap-2">
          <Button variant="ghost" onClick={onReload}>
            <RefreshCw className="h-4 w-4" /> 刷新
          </Button>
          <Button onClick={onSave} disabled={saving}>
            {saving && <Loader2 className="h-4 w-4 animate-spin" />}
            保存设置
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

function NumberField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string
  hint?: string
  value: number
  onChange: (value: number) => void
}) {
  return (
    <Field label={label} hint={hint}>
      <Input
        type="number"
        min={0}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </Field>
  )
}
