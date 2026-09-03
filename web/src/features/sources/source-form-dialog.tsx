import { useMemo } from "react"
import { Loader2 } from "lucide-react"
import type { SourceConfig, SourceMeta, TagConfig } from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Dialog } from "@/components/ui/dialog"
import { ErrorText } from "@/components/ui/feedback"
import {
  Checkbox,
  Field,
  FollowGlobal,
  Input,
  Select,
} from "@/components/ui/field"
import {
  type FormState,
  emptyForm,
  sourceToForm,
} from "@/lib/source-form"

/**
 * 新增 / 编辑监控源。
 *
 * 分组强制结构化提取时（源「跟随分组」且所属分组配置了 items 提取），
 * 源自身的提取设置由分组接管，此处只读展示。
 */
export function SourceFormDialog({
  open,
  editing,
  form,
  tags,
  saving,
  error,
  onChange,
  onClose,
  onSave,
}: {
  open: boolean
  editing: SourceMeta | null
  form: FormState
  tags: TagConfig[]
  saving: boolean
  error: string | null
  onChange: (patch: Partial<FormState>) => void
  onClose: () => void
  onSave: () => void
}) {
  // 跟随分组时，源自身的提取配置会被分组覆盖。需与后端
  // resolve_effective_extract 一致：取源跟随的、配置了提取的分组中
  // 按名称升序的第一个，仅当其为 items 时才禁用源级设置。
  const groupForcesItemsExtract = useMemo(
    () => forcingGroup(form, tags),
    [form, tags],
  )
  const formTagNames = useMemo(
    () => form.tags.split(",").map((t) => t.trim()).filter(Boolean),
    [form.tags],
  )

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={editing ? "编辑监控源" : "添加监控源"}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={saving}>
            取消
          </Button>
          <Button onClick={onSave} disabled={saving}>
            {saving && <Loader2 className="h-4 w-4 animate-spin" />}
            保存
          </Button>
        </>
      }
    >
      {error && <div className="mt-2">{<ErrorText>{error}</ErrorText>}</div>}

      <div className="mt-4 grid grid-cols-2 gap-4">
        <Field label="URL" className="col-span-2">
          <Input
            value={form.url}
            placeholder="https://example.com/list"
            onChange={(e) => onChange({ url: e.target.value })}
          />
        </Field>

        <Field
          label="名称"
          className="col-span-2"
          hint={editing ? undefined : "留空自动抓取网页标题"}
        >
          <Input
            value={form.name}
            placeholder="留空自动获取网页标题"
            onChange={(e) => onChange({ name: e.target.value })}
          />
        </Field>

        <div className="col-span-2 flex flex-wrap items-center gap-6 rounded-md border bg-muted/30 p-3">
          <Checkbox
            checked={form.enabled}
            onChange={(enabled) => onChange({ enabled })}
            label="启用监控"
          />
          <Checkbox
            checked={form.notify_enabled}
            onChange={(notify_enabled) => onChange({ notify_enabled })}
            label="启用通知"
          />
        </div>

        {/* 仅编辑且源有分组归属时「跟随分组」才有意义；新建时隐藏以免误设。 */}
        {editing && formTagNames.length > 0 && (
          <div className="col-span-2 flex items-center justify-between rounded-md border bg-muted/30 p-3">
            <div className="text-sm">
              <div className="font-medium">跟随分组设置</div>
              <div className="mt-0.5 text-xs text-muted-foreground">
                开启后继承分组的历史保留 / 通知目标 / 内容提取设置
              </div>
            </div>
            <Checkbox
              checked={form.follow_group}
              onChange={(follow_group) => onChange({ follow_group })}
              label="跟随分组"
            />
          </div>
        )}

        <Field label="引擎 (engine)">
          <Select
            value={form.engine}
            onChange={(e) => onChange({ engine: e.target.value })}
          >
            <option value="http">http</option>
            <option value="camofox">camofox</option>
          </Select>
        </Field>

        <Field label="请求方法 (method)">
          <Select
            value={form.method}
            onChange={(e) => onChange({ method: e.target.value })}
          >
            <option value="GET">GET</option>
            <option value="POST">POST</option>
            <option value="HEAD">HEAD</option>
          </Select>
        </Field>

        {form.engine === "camofox" && (
          <div className="col-span-2">
            <div className="rounded-md border bg-muted/30 p-3">
              <Checkbox
                checked={form.screenshot}
                onChange={(screenshot) => onChange({ screenshot })}
                label="检测到变更时截图（可在历史记录中查看）"
              />
            </div>
          </div>
        )}

        <Field
          label="cron 表达式"
          className="col-span-2"
          hint="例：*/15 * * * * 每 15 分钟、0 9,18 * * 1-5 工作日 9:00/18:00"
        >
          <FollowGlobal
            checked={form.cron_follow_global}
            onChange={(cron_follow_global) => onChange({ cron_follow_global })}
          >
            <Input
              className="flex-1"
              placeholder="0 * * * *"
              value={form.cron}
              onChange={(e) => onChange({ cron: e.target.value })}
            />
          </FollowGlobal>
        </Field>

        <Field
          label="内容提取"
          className="col-span-2"
          hint={
            groupForcesItemsExtract
              ? `由分组「${groupForcesItemsExtract}」提供结构化提取，此处设置被覆盖。关闭“跟随分组”后可用。`
              : undefined
          }
        >
          <Select
            value={groupForcesItemsExtract ? "items" : form.extractType}
            disabled={!!groupForcesItemsExtract}
            onChange={(e) =>
              onChange({ extractType: e.target.value as "text" | "items" })
            }
          >
            <option value="text">整页文本</option>
            <option value="items">结构化提取</option>
          </Select>
        </Field>

        {form.extractType === "items" && !groupForcesItemsExtract && (
          <div className="col-span-2 space-y-4 rounded-md border bg-muted/30 p-3">
            <div className="grid grid-cols-2 gap-4">
              <Field label="选择器类型">
                <Select
                  value={form.selectorKind}
                  onChange={(e) =>
                    onChange({
                      selectorKind: e.target.value as "css" | "json_path",
                      selector: "",
                    })
                  }
                >
                  <option value="css">CSS</option>
                  <option value="json_path">JSONPath</option>
                </Select>
              </Field>
              <Field
                label={
                  form.selectorKind === "json_path" ? "JSONPath 路径" : "CSS 选择器"
                }
              >
                <Input
                  value={form.selector}
                  placeholder={
                    form.selectorKind === "json_path" ? "$.data.items[*]" : ".product"
                  }
                  onChange={(e) => onChange({ selector: e.target.value })}
                />
              </Field>
            </div>
            <Field label="提取字段（可选 JSON 数组）">
              <textarea
                rows={3}
                className="w-full rounded-md border border-input bg-background px-3 py-1.5 font-mono text-xs shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                value={form.fieldsJson}
                placeholder='[{"name":"id","attr":"data-id"},{"name":"title","selector":".title"}]'
                onChange={(e) => onChange({ fieldsJson: e.target.value })}
              />
            </Field>
          </div>
        )}

        <Field label="通知附带图片" className="col-span-2">
          <div className="grid grid-cols-2 gap-4 rounded-md border bg-muted/30 p-3">
            <Field label="图片来源">
              <Select
                value={form.imageKind}
                onChange={(e) =>
                  onChange({
                    imageKind: e.target.value as FormState["imageKind"],
                  })
                }
              >
                <option value="none">不附带图片</option>
                {form.extractType === "items" && <option value="items">条目的图片</option>}
                {form.extractType === "items" && <option value="changed">变更元素的图片</option>}
                <option value="css">按 CSS 选择器</option>
              </Select>
            </Field>
            {form.imageKind === "css" && (
              <Field label="图片 CSS 选择器">
                <Input
                  value={form.imageSelector}
                  placeholder=".cover img 或 img.product-thumb"
                  onChange={(e) => onChange({ imageSelector: e.target.value })}
                />
              </Field>
            )}
          </div>
        </Field>

        <Field label="超时">
          <FollowGlobal
            checked={form.timeout_secs === 0}
            onChange={(follow) => onChange({ timeout_secs: follow ? 0 : 30 })}
          >
            <span className="flex items-center gap-1">
              <Input
                type="number"
                min={1}
                className="w-20"
                value={form.timeout_secs}
                onChange={(e) =>
                  onChange({ timeout_secs: Number(e.target.value) })
                }
              />
              <span className="text-xs text-muted-foreground">秒</span>
            </span>
          </FollowGlobal>
        </Field>

        <Field label="标签">
          <Input
            value={form.tags}
            placeholder="a,b"
            onChange={(e) => onChange({ tags: e.target.value })}
          />
        </Field>
      </div>
    </Dialog>
  )
}

/**
 * 找出接管该源内容提取的分组名；无接管时返回 null。
 * 与后端 `resolve_effective_extract` 的选取规则保持一致。
 */
export function forcingGroup(form: FormState, tags: TagConfig[]): string | null {
  if (!form.follow_group) return null
  const names = form.tags.split(",").map((t) => t.trim()).filter(Boolean)
  const effective = tags
    .filter((t) => names.includes(t.name) && t.extract)
    .sort((a, b) => a.name.localeCompare(b.name))[0]
  // 仅结构化提取会覆盖源自身设置；text 或未配置提取时源仍用自己的设置。
  if (!effective || effective.extract?.type !== "items") return null
  return effective.name
}

/** 表单初始值：编辑时取源配置，新增时为空表单。 */
export function initialForm(editing: SourceMeta | null): FormState {
  return editing ? sourceToForm(editing as SourceConfig) : emptyForm()
}

export { validateCron }
