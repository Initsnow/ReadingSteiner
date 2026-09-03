import { useEffect, useState } from "react"
import { Boxes, Hash, Plus, Trash2 } from "lucide-react"
import type { ExtractConfig, TagConfig } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Field, Input, Select } from "@/components/ui/field"
import { NoticeText } from "@/components/ui/feedback"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

/** 分组（标签）管理：为每个分组配置历史保留 / 通知目标 / 默认提取。 */
export function TagsCard({
  tags,
  busy,
  notice,
  error,
  onChange,
  onSave,
  onAdd,
  onDelete,
}: {
  tags: TagConfig[]
  busy: boolean
  notice: string | null
  error: string | null
  onChange: (name: string, patch: Partial<TagConfig>) => void
  onSave: (tag: TagConfig) => void
  onAdd: (name: string) => void
  onDelete: (name: string) => void
}) {
  const [activeTag, setActiveTag] = useState("")
  const [newTagName, setNewTagName] = useState("")

  // 分组列表变化后，若当前激活项已被删除或尚未初始化，回退到第一个分组。
  useEffect(() => {
    setActiveTag((cur) =>
      cur && tags.some((t) => t.name === cur) ? cur : (tags[0]?.name ?? ""),
    )
  }, [tags])

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Boxes className="h-5 w-5" /> 分组管理
        </CardTitle>
      </CardHeader>
      <CardContent>
        <NoticeText>{notice}</NoticeText>

        <div className="mb-4 flex max-w-md items-center gap-2">
          <Input
            placeholder="分组名称，如 news"
            value={newTagName}
            onChange={(e) => setNewTagName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                onAdd(newTagName)
                setNewTagName("")
              }
            }}
          />
          <Button size="sm" disabled={busy} onClick={() => {
            onAdd(newTagName)
            setNewTagName("")
          }}>
            <Plus className="h-3.5 w-3.5" /> 新建
          </Button>
        </div>

        {error && <p className="mb-3 text-sm text-destructive">{error}</p>}

        {tags.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            暂无分组。在监控源的「标签」字段填写标签后保存，即可在此配置。
          </p>
        ) : (
          <Tabs value={activeTag} onValueChange={setActiveTag}>
            <div className="overflow-x-auto pb-1">
              <TabsList className="h-auto flex-wrap gap-1">
                {tags.map((tag) => (
                  <TabsTrigger
                    key={tag.name}
                    value={tag.name}
                    className="flex items-center gap-1.5 data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                  >
                    <Hash className="h-3.5 w-3.5 opacity-70" />
                    {tag.name}
                    {tagIsCustomized(tag) && (
                      <span className="ml-0.5 h-1.5 w-1.5 rounded-full bg-emerald-500" />
                    )}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>

            {tags.map((tag) => (
              <TabsContent key={tag.name} value={tag.name} className="mt-3">
                <TagEditor
                  tag={tag}
                  busy={busy}
                  onChange={(patch) => onChange(tag.name, patch)}
                  onSave={() => onSave(tag)}
                  onDelete={() => onDelete(tag.name)}
                />
              </TabsContent>
            ))}
          </Tabs>
        )}
      </CardContent>
    </Card>
  )
}

function tagIsCustomized(tag: TagConfig): boolean {
  return tag.history_limit > 0 || !!tag.notify_url || !!tag.extract
}

function TagEditor({
  tag,
  busy,
  onChange,
  onSave,
  onDelete,
}: {
  tag: TagConfig
  busy: boolean
  onChange: (patch: Partial<TagConfig>) => void
  onSave: () => void
  onDelete: () => void
}) {
  const customized = tagIsCustomized(tag)
  return (
    <>
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-medium">{tag.name}</span>
          {tag.history_limit > 0 && <Badge variant="secondary">历史保留</Badge>}
          {!!tag.notify_url && <Badge variant="secondary">独立通知</Badge>}
          {!!tag.extract && <Badge variant="secondary">内容提取</Badge>}
          {!customized && (
            <span className="text-xs text-muted-foreground">全部沿用全局设置</span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <Button size="sm" variant="outline" disabled={busy} onClick={onSave}>
            保存
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="text-destructive hover:text-destructive"
            disabled={busy}
            onClick={onDelete}
          >
            <Trash2 className="h-3.5 w-3.5" /> 删除
          </Button>
        </div>
      </div>

      <div className="grid max-w-3xl gap-4 md:grid-cols-2">
        <Field label="每个源保留历史条数" hint="0 表示不限制，使用全局设置">
          <Input
            type="number"
            min={0}
            className="w-24"
            value={tag.history_limit}
            onChange={(e) => onChange({ history_limit: Number(e.target.value) })}
          />
        </Field>
        <Field label="通知目标" hint="留空沿用全局通知目标">
          <Input
            className="font-mono"
            value={tag.notify_url ?? ""}
            placeholder="tgram://bot/ChatID"
            onChange={(e) => onChange({ notify_url: e.target.value })}
          />
        </Field>
        <Field
          label="默认内容提取"
          hint="分组下的监控源开启「跟随分组」时沿用"
          className="md:col-span-2"
        >
          <ExtractEditor
            extract={tag.extract}
            onChange={(extract) => onChange({ extract })}
          />
        </Field>
      </div>
    </>
  )
}

/** 分组的默认提取配置：不覆盖 / 整页文本 / 结构化提取。 */
function ExtractEditor({
  extract,
  onChange,
}: {
  extract: ExtractConfig | null | undefined
  onChange: (extract: ExtractConfig | null) => void
}) {
  const selectorText =
    extract?.type === "items" ? selectorOf(extract.selector) : ""

  return (
    <div className="space-y-2">
      <Select
        value={extract?.type ?? "inherit"}
        onChange={(e) => {
          const v = e.target.value
          if (v === "inherit") onChange(null)
          else if (v === "text") onChange({ type: "text" })
          else
            onChange({
              type: "items",
              selector: { kind: "css", selector: "" },
            })
        }}
      >
        <option value="inherit">不覆盖，沿用源自身</option>
        <option value="text">整页文本</option>
        <option value="items">结构化提取</option>
      </Select>
      {extract?.type === "items" && (
        <Input
          className="font-mono"
          value={selectorText}
          placeholder="选择器，如 .product"
          onChange={(e) =>
            onChange({
              type: "items",
              selector: { kind: "css", selector: e.target.value },
            })
          }
        />
      )}
    </div>
  )
}

function selectorOf(selector: { kind: string } & Record<string, unknown>): string {
  if (selector.kind === "json_path") return (selector.path as string) ?? ""
  return (selector.selector as string) ?? ""
}
