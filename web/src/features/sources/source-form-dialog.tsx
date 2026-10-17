import { useState } from "react"
import {
  ChevronDown,
  ChevronRight,
  Clock,
  Code2,
  Cpu,
  FileCode,
  Globe,
  ImageIcon,
  Loader2,
  Play,
  Sparkles,
  Zap,
} from "lucide-react"
import {
  api,
  type GroupConfig,
  type SourceConfig,
  type SourceMeta,
  type TestSourceResult,
} from "@/lib/api"
import { validateCron } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Dialog } from "@/components/ui/dialog"
import { ErrorText } from "@/components/ui/feedback"
import {
  type FormState,
  emptyForm,
  formToSource,
  sourceToForm,
} from "@/lib/source-form"

export function SourceFormDialog({
  open,
  editing,
  form,
  groups,
  saving,
  error,
  onChange,
  onClose,
  onSave,
}: {
  open: boolean
  editing: SourceMeta | null
  form: FormState
  groups: GroupConfig[]
  saving: boolean
  error: string | null
  onChange: (patch: Partial<FormState>) => void
  onClose: () => void
  onSave: () => void
}) {
  const [fetchingTitle, setFetchingTitle] = useState(false)
  const [activeTab, setActiveTab] = useState<"form" | "inspector">("form")
  const [showAdvancedHttp, setShowAdvancedHttp] = useState(
    Boolean(form.headersJson && form.headersJson !== "{}" && form.headersJson.trim())
  )

  // Live Inspector Dry Run 状态
  const [testing, setTesting] = useState(false)
  const [testResult, setTestResult] = useState<TestSourceResult | null>(null)
  const [testError, setTestError] = useState<string | null>(null)

  // 抓取页面标题填充名称
  async function fetchTitle() {
    if (!form.url.trim()) return
    try {
      setFetchingTitle(true)
      const res = await api.previewSource(form.url.trim(), form.engine)
      if (res.title) {
        onChange({ name: res.title })
      }
    } catch {
      // 忽略失败
    } finally {
      setFetchingTitle(false)
    }
  }

  // 切换预设分组时，自动预填模板设置
  function handleGroupChange(groupId: string) {
    const patch: Partial<FormState> = { group_id: groupId }
    if (groupId) {
      const g = groups.find((it) => it.id === groupId)
      if (g) {
        if (g.default_cron && form.cron_follow_global) {
          patch.cron = g.default_cron
          patch.cron_follow_global = false
        }
        if (g.default_headers && Object.keys(g.default_headers).length > 0) {
          patch.headersJson = JSON.stringify(g.default_headers, null, 2)
        }
        if (g.default_extract) {
          if (g.default_extract.type === "text") {
            patch.extractType = "text"
            patch.textSelector = g.default_extract.selector ?? ""
          } else if (g.default_extract.type === "json") {
            patch.extractType = "json"
            patch.jsonPath = g.default_extract.json_path ?? ""
          } else if (g.default_extract.type === "items") {
            patch.extractType = "items"
            const s = g.default_extract.selector
            patch.selectorKind = s.kind
            patch.selector = s.kind === "css" ? s.selector : (s as { path: string }).path
          }
        }
      }
    }
    onChange(patch)
  }

  // 运行 Live Inspector 实时草稿测试
  async function runDryRun() {
    if (!form.url.trim()) {
      setTestError("请先填写目标监控 URL")
      return
    }
    setTesting(true)
    setTestError(null)
    setActiveTab("inspector")
    try {
      const sourceDraft = formToSource(form, editing ?? undefined)
      const res = await api.testSourceDryRun(sourceDraft)
      setTestResult(res)
    } catch (e) {
      setTestError((e as Error).message)
      setTestResult(null)
    } finally {
      setTesting(false)
    }
  }

  return (
    <Dialog
      open={open}
      onClose={onClose}
      width="xl"
      title={
        <div className="flex items-center gap-2 text-zinc-100">
          <Globe className="h-4 w-4 text-emerald-400" />
          <span>{editing ? `编辑监控源: ${form.name || form.id}` : "新建监控源"}</span>
        </div>
      }
      footer={
        <div className="flex w-full items-center justify-between">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={runDryRun}
            disabled={testing || !form.url.trim()}
            className="gap-1.5 border-zinc-700 bg-zinc-850 hover:bg-zinc-800 text-xs"
          >
            {testing ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Play className="h-3.5 w-3.5 text-emerald-400 fill-emerald-400" />
            )}
            实时测试
          </Button>

          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={onClose}
              disabled={saving}
              className="text-xs"
            >
              取消
            </Button>
            <Button
              size="sm"
              onClick={onSave}
              disabled={saving}
              className="text-xs gap-1.5"
            >
              {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              保存配置
            </Button>
          </div>
        </div>
      }
    >
      {error && <div className="mb-3">{<ErrorText>{error}</ErrorText>}</div>}

      {/* 顶部选项卡：表单编辑 vs 实时检查器 */}
      <div className="mb-4 flex border-b border-zinc-800 text-xs font-medium">
        <button
          type="button"
          onClick={() => setActiveTab("form")}
          className={`flex items-center gap-2 border-b-2 px-4 py-2 transition-colors cursor-pointer ${
            activeTab === "form"
              ? "border-zinc-200 text-zinc-100 font-semibold"
              : "border-transparent text-zinc-400 hover:text-zinc-200"
          }`}
        >
          <Code2 className="h-3.5 w-3.5" />
          规则配置
        </button>
        <button
          type="button"
          onClick={() => setActiveTab("inspector")}
          className={`flex items-center gap-2 border-b-2 px-4 py-2 transition-colors cursor-pointer ${
            activeTab === "inspector"
              ? "border-zinc-200 text-zinc-100 font-semibold"
              : "border-transparent text-zinc-400 hover:text-zinc-200"
          }`}
        >
          <Sparkles className="h-3.5 w-3.5 text-amber-400" />
          Live Inspector 测试
          {testResult && (
            <span className="rounded-full bg-emerald-500/20 px-1.5 py-0.2 text-[10px] text-emerald-400 font-mono">
              {testResult.status} OK
            </span>
          )}
        </button>
      </div>

      {activeTab === "form" && (
        <div className="space-y-4 max-h-[68vh] overflow-y-auto pr-1 text-xs">
          {/* 1. 基础信息 */}
          <div className="space-y-3 rounded-lg border border-zinc-800/80 bg-zinc-950/60 p-3.5">
            <div className="font-semibold text-zinc-200 flex items-center gap-2 text-xs">
              <Globe className="h-3.5 w-3.5 text-emerald-400" />
              目标站点与分组
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              <div className="sm:col-span-2">
                <label className="block text-zinc-400 mb-1">监控 URL *</label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={form.url}
                    placeholder="https://example.com/api/releases"
                    onChange={(e) => onChange({ url: e.target.value })}
                    className="flex-1 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono placeholder:text-zinc-600 focus:border-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-600"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={fetchTitle}
                    disabled={fetchingTitle || !form.url.trim()}
                    className="h-8 border-zinc-800 text-[11px] whitespace-nowrap bg-zinc-900"
                  >
                    {fetchingTitle ? "抓取中..." : "抓取标题"}
                  </Button>
                </div>
              </div>

              <div>
                <label className="block text-zinc-400 mb-1">所属预设分组</label>
                <select
                  value={form.group_id}
                  onChange={(e) => handleGroupChange(e.target.value)}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 focus:border-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-600"
                >
                  <option value="">未分组（独立配置）</option>
                  {groups.map((g) => (
                    <option key={g.id} value={g.id}>
                      {g.name} ({g.id})
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div>
                <label className="block text-zinc-400 mb-1">
                  监控源 ID *
                </label>
                <input
                  type="text"
                  disabled={!!editing}
                  value={form.id}
                  placeholder="留空自动基于 URL 或名称生成"
                  onChange={(e) => onChange({ id: e.target.value })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono disabled:opacity-50"
                />
              </div>

              <div>
                <label className="block text-zinc-400 mb-1">展示名称</label>
                <input
                  type="text"
                  value={form.name}
                  placeholder="如: GitHub 最新发布"
                  onChange={(e) => onChange({ name: e.target.value })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 focus:border-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-600"
                />
              </div>
            </div>

            <div className="flex items-center gap-6 pt-1">
              <label className="flex items-center gap-2 cursor-pointer text-zinc-300">
                <input
                  type="checkbox"
                  checked={form.enabled}
                  onChange={(e) => onChange({ enabled: e.target.checked })}
                  className="h-4 w-4 rounded border-zinc-700 bg-zinc-900 accent-zinc-200"
                />
                启用监控
              </label>
              <label className="flex items-center gap-2 cursor-pointer text-zinc-300">
                <input
                  type="checkbox"
                  checked={form.notify_enabled}
                  onChange={(e) => onChange({ notify_enabled: e.target.checked })}
                  className="h-4 w-4 rounded border-zinc-700 bg-zinc-900 accent-zinc-200"
                />
                启用 Telegram 通知
              </label>
            </div>
          </div>

          {/* 2. 抓取引擎与参数 */}
          <div className="space-y-3 rounded-lg border border-zinc-800/80 bg-zinc-950/60 p-3.5">
            <div className="font-semibold text-zinc-200 flex items-center gap-2 text-xs">
              <Zap className="h-3.5 w-3.5 text-amber-400" />
              抓取引擎与 HTTP 行为
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              <div>
                <label className="block text-zinc-400 mb-1">抓取引擎</label>
                <select
                  value={form.engine}
                  onChange={(e) => onChange({ engine: e.target.value })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 focus:border-zinc-700 focus:outline-none"
                >
                  <option value="http">HTTP 模式（轻量 / API / 静态页）</option>
                  <option value="browser">Browser CDP 模式（无头 Chrome 动态渲染）</option>
                </select>
              </div>

              <div>
                <label className="block text-zinc-400 mb-1">请求方法</label>
                <select
                  value={form.method}
                  onChange={(e) => onChange({ method: e.target.value })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 focus:border-zinc-700 focus:outline-none"
                >
                  <option value="GET">GET</option>
                  <option value="POST">POST</option>
                  <option value="HEAD">HEAD</option>
                </select>
              </div>

              <div>
                <label className="block text-zinc-400 mb-1">超时秒数</label>
                <input
                  type="number"
                  min={0}
                  value={form.timeout_secs}
                  placeholder="0 为使用全局默认"
                  onChange={(e) => onChange({ timeout_secs: Number(e.target.value) })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                />
              </div>
            </div>

            {/* CDP 特殊选项 */}
            {form.engine === "browser" && (
              <div className="rounded-md border border-amber-900/30 bg-amber-950/15 p-3 space-y-3">
                <div className="text-[11px] text-amber-300 font-medium flex items-center gap-1.5">
                  <Cpu className="h-3.5 w-3.5 text-amber-400" />
                  CDP 浏览器渲染选项
                </div>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div>
                    {(() => {
                      const activeExtractSelector =
                        form.extractType === "text"
                          ? form.textSelector.trim()
                          : form.extractType === "items" && form.selectorKind === "css"
                          ? form.selector.trim()
                          : ""
                      return (
                        <>
                          <div className="flex items-center justify-between mb-1">
                            <label className="text-zinc-400">
                              等待元素 CSS Selector
                            </label>
                            {activeExtractSelector && !form.waitSelector && (
                              <span className="text-[10px] text-amber-400/80">
                                留空默认沿用
                              </span>
                            )}
                          </div>
                          <input
                            type="text"
                            value={form.waitSelector}
                            placeholder={
                              activeExtractSelector
                                ? `留空自动沿用: ${activeExtractSelector}`
                                : "留空默认沿用提取区域选择器"
                            }
                            onChange={(e) => onChange({ waitSelector: e.target.value })}
                            className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                          />
                        </>
                      )
                    })()}
                  </div>

                  <div>
                    <label className="block text-zinc-400 mb-1">
                      页面加载后执行 JavaScript
                    </label>
                    <input
                      type="text"
                      value={form.evaluate}
                      placeholder="window.scrollTo(0, 500)"
                      onChange={(e) => onChange({ evaluate: e.target.value })}
                      className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                    />
                  </div>
                </div>
              </div>
            )}

            {/* 可折叠的自定义请求头 */}
            <div className="border-t border-zinc-800/80 pt-2">
              <button
                type="button"
                onClick={() => setShowAdvancedHttp(!showAdvancedHttp)}
                className="flex items-center gap-1 text-[11px] text-zinc-400 hover:text-zinc-200 cursor-pointer transition-colors"
              >
                {showAdvancedHttp ? (
                  <ChevronDown className="h-3.5 w-3.5" />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5" />
                )}
                <span>自定义 HTTP 请求头</span>
              </button>

              {showAdvancedHttp && (
                <div className="mt-2.5 rounded border border-zinc-800 bg-zinc-900/60 p-3">
                  <label className="block text-zinc-400 mb-1 text-[11px]">
                    自定义 Headers (JSON 键值对对象)
                  </label>
                  <textarea
                    rows={3}
                    value={form.headersJson}
                    placeholder='{"Authorization": "Bearer ...", "User-Agent": "MyBot/1.0", "Cookie": "session=..."}'
                    onChange={(e) => onChange({ headersJson: e.target.value })}
                    className="w-full rounded border border-zinc-800 bg-zinc-950 p-2 text-xs font-mono text-zinc-200 focus:border-zinc-700 focus:outline-none"
                  />
                </div>
              )}
            </div>
          </div>

          {/* 3. 内容提取模式 */}
          <div className="space-y-3 rounded-lg border border-zinc-800/80 bg-zinc-950/60 p-3.5">
            <div className="font-semibold text-zinc-200 flex items-center gap-2 text-xs">
              <FileCode className="h-3.5 w-3.5 text-primary" />
              内容提取模式
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-2.5">
              <button
                type="button"
                onClick={() => onChange({ extractType: "text" })}
                className={`rounded-lg border p-2.5 text-left transition-all cursor-pointer ${
                  form.extractType === "text"
                    ? "border-zinc-300 bg-zinc-900 text-zinc-100 shadow-xs"
                    : "border-zinc-800 bg-zinc-900/40 text-zinc-400 hover:border-zinc-700 hover:text-zinc-300"
                }`}
              >
                <div className="text-xs font-semibold text-zinc-100">Text 文本提取</div>
                <div className="text-[10px] text-zinc-400 mt-0.5">
                  纯文本对比，可指定可选 CSS 区域
                </div>
              </button>

              <button
                type="button"
                onClick={() => onChange({ extractType: "json" })}
                className={`rounded-lg border p-2.5 text-left transition-all cursor-pointer ${
                  form.extractType === "json"
                    ? "border-zinc-300 bg-zinc-900 text-zinc-100 shadow-xs"
                    : "border-zinc-800 bg-zinc-900/40 text-zinc-400 hover:border-zinc-700 hover:text-zinc-300"
                }`}
              >
                <div className="text-xs font-semibold text-zinc-100">Json 接口提取</div>
                <div className="text-[10px] text-zinc-400 mt-0.5">
                  格式化 JSON 或指定 JSONPath
                </div>
              </button>

              <button
                type="button"
                onClick={() => onChange({ extractType: "items" })}
                className={`rounded-lg border p-2.5 text-left transition-all cursor-pointer ${
                  form.extractType === "items"
                    ? "border-zinc-300 bg-zinc-900 text-zinc-100 shadow-xs"
                    : "border-zinc-800 bg-zinc-900/40 text-zinc-400 hover:border-zinc-700 hover:text-zinc-300"
                }`}
              >
                <div className="text-xs font-semibold text-zinc-100">Items 结构化条目</div>
                <div className="text-[10px] text-zinc-400 mt-0.5">
                  列表多条目拆解，比对增删改
                </div>
              </button>
            </div>

            {/* 各模式详细字段 */}
            {form.extractType === "text" && (
              <div className="pt-1">
                <label className="block text-zinc-400 mb-1">
                  可选页面区域 CSS 选择器（留空监控整页文本）
                </label>
                <input
                  type="text"
                  value={form.textSelector}
                  placeholder="如 #content, article.post, .main-body"
                  onChange={(e) => onChange({ textSelector: e.target.value })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                />
              </div>
            )}

            {form.extractType === "json" && (
              <div className="pt-1">
                <label className="block text-zinc-400 mb-1">
                  可选 JSONPath 提取路径（留空格式化全量 JSON 比对）
                </label>
                <input
                  type="text"
                  value={form.jsonPath}
                  placeholder="如 $.data.version 或 $.items[*].name"
                  onChange={(e) => onChange({ jsonPath: e.target.value })}
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                />
              </div>
            )}

            {form.extractType === "items" && (
              <div className="space-y-3 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3 pt-2">
                <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                  <div>
                    <label className="block text-zinc-400 mb-1">选择器类型</label>
                    <select
                      value={form.selectorKind}
                      onChange={(e) =>
                        onChange({
                          selectorKind: e.target.value as "css" | "json_path",
                          selector: "",
                        })
                      }
                      className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 focus:border-zinc-700 focus:outline-none"
                    >
                      <option value="css">CSS 选择器（HTML）</option>
                      <option value="json_path">JSONPath（JSON 数组）</option>
                    </select>
                  </div>

                  <div className="sm:col-span-2">
                    <label className="block text-zinc-400 mb-1">条目选择器 *</label>
                    <input
                      type="text"
                      value={form.selector}
                      placeholder={
                        form.selectorKind === "css"
                          ? ".post-item, tr.row"
                          : "$.data.items[*]"
                      }
                      onChange={(e) => onChange({ selector: e.target.value })}
                      className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                    />
                  </div>
                </div>

                <div>
                  <label className="block text-zinc-400 mb-1">
                    条目字段提取映射（JSON 数组，可选）
                  </label>
                  <textarea
                    rows={2}
                    value={form.fieldsJson}
                    placeholder='[{"name":"title","selector":".title"},{"name":"id","attr":"data-id"}]'
                    onChange={(e) => onChange({ fieldsJson: e.target.value })}
                    className="w-full rounded-md border border-zinc-800 bg-zinc-900 p-2.5 text-xs font-mono text-zinc-200 focus:border-zinc-700 focus:outline-none"
                  />
                </div>
              </div>
            )}

            {/* 图片通知选择器 */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-2 border-t border-zinc-800/80">
              <div>
                <label className="block text-zinc-400 mb-1">
                  通知附带图片
                </label>
                <select
                  value={form.imageKind}
                  onChange={(e) =>
                    onChange({
                      imageKind: e.target.value as FormState["imageKind"],
                    })
                  }
                  className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 focus:border-zinc-700 focus:outline-none"
                >
                  <option value="none">不附带图片</option>
                  <option value="first_item">首个条目的图片</option>
                  <option value="css">CSS 选择器抽取页面图片</option>
                  {form.engine === "browser" && (
                    <option value="screenshot">CDP 页面截屏快照</option>
                  )}
                </select>
              </div>

              {form.imageKind === "css" && (
                <div>
                  <label className="block text-zinc-400 mb-1">
                    图片 CSS 选择器
                  </label>
                  <input
                    type="text"
                    value={form.imageSelector}
                    placeholder=".cover img, img.thumbnail"
                    onChange={(e) => onChange({ imageSelector: e.target.value })}
                    className="w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                  />
                </div>
              )}
            </div>
          </div>

          {/* 4. 调度周期 */}
          <div className="space-y-3 rounded-lg border border-zinc-800/80 bg-zinc-950/60 p-3.5">
            <div className="font-semibold text-zinc-200 flex items-center gap-2 text-xs">
              <Clock className="h-3.5 w-3.5 text-primary" />
              调度周期
            </div>

            <div className="space-y-2">
              <label className="flex items-center gap-2 cursor-pointer text-zinc-300">
                <input
                  type="checkbox"
                  checked={form.cron_follow_global}
                  onChange={(e) =>
                    onChange({ cron_follow_global: e.target.checked })
                  }
                  className="h-4 w-4 rounded border-zinc-700 bg-zinc-900 accent-zinc-200"
                />
                跟随预设分组或全局设置默认周期
              </label>

              {!form.cron_follow_global && (
                <div className="flex flex-wrap items-center gap-2 pt-1">
                  <input
                    type="text"
                    value={form.cron}
                    placeholder="0 * * * *"
                    onChange={(e) => onChange({ cron: e.target.value })}
                    className="flex-1 min-w-44 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-xs text-zinc-100 font-mono focus:border-zinc-700 focus:outline-none"
                  />
                  <button
                    type="button"
                    onClick={() => onChange({ cron: "*/10 * * * *" })}
                    className="rounded border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-[11px] text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
                  >
                    10m
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange({ cron: "*/30 * * * *" })}
                    className="rounded border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-[11px] text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
                  >
                    30m
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange({ cron: "0 * * * *" })}
                    className="rounded border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-[11px] text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
                  >
                    1h
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange({ cron: "0 9 * * 1-5" })}
                    className="rounded border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-[11px] text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
                  >
                    工作日9点
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Live Inspector 实时抓取测试面板 */}
      {activeTab === "inspector" && (
        <div className="space-y-4 max-h-[68vh] overflow-y-auto pr-1 text-xs">
          <div className="flex items-center justify-between rounded-lg border border-zinc-800 bg-zinc-950/60 p-3.5">
            <div className="space-y-0.5">
              <div className="font-semibold text-zinc-200 flex items-center gap-2">
                <Sparkles className="h-4 w-4 text-amber-400" />
                Live Inspector 测试
              </div>
              <p className="text-[11px] text-zinc-400">
                使用当前未保存的表单草稿模拟运行抓取与提取，即时排查选择器与数据。
              </p>
            </div>

            <Button
              size="sm"
              onClick={runDryRun}
              disabled={testing || !form.url.trim()}
              className="gap-1.5 h-8 text-xs"
            >
              {testing ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Play className="h-3.5 w-3.5 text-emerald-400 fill-emerald-400" />
              )}
              立即测试
            </Button>
          </div>

          {testing && (
            <div className="flex flex-col items-center justify-center p-8 text-zinc-400 space-y-2 rounded-lg border border-zinc-800/80 bg-zinc-950/40">
              <Loader2 className="h-6 w-6 animate-spin text-primary" />
              <span>正在连接目标站点并提取内容...</span>
            </div>
          )}

          {testError && <ErrorText>{testError}</ErrorText>}

          {testResult && (
            <div className="space-y-3">
              {/* 运行指标 */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5">
                <div className="rounded-lg border border-zinc-800 bg-zinc-950/80 p-2.5">
                  <div className="text-[10px] text-zinc-500 font-mono">状态码</div>
                  <div className="font-mono text-sm font-bold text-emerald-400 mt-0.5">
                    {testResult.status ?? 200} OK
                  </div>
                </div>

                <div className="rounded-lg border border-zinc-800 bg-zinc-950/80 p-2.5">
                  <div className="text-[10px] text-zinc-500 font-mono">抓取耗时</div>
                  <div className="font-mono text-sm font-bold text-zinc-200 mt-0.5">
                    {testResult.duration_ms} ms
                  </div>
                </div>

                <div className="rounded-lg border border-zinc-800 bg-zinc-950/80 p-2.5">
                  <div className="text-[10px] text-zinc-500 font-mono">提取条目数</div>
                  <div className="font-mono text-sm font-bold text-primary mt-0.5">
                    {testResult.items ? testResult.items.length : 1}
                  </div>
                </div>

                <div className="rounded-lg border border-zinc-800 bg-zinc-950/80 p-2.5 truncate">
                  <div className="text-[10px] text-zinc-500 font-mono">内容指纹</div>
                  <div className="font-mono text-xs font-semibold text-zinc-300 truncate mt-0.5">
                    {testResult.fingerprint?.slice(0, 10)}...
                  </div>
                </div>
              </div>

              {/* 截图预览 (若有) */}
              {testResult.screenshot_base64 && (
                <div className="rounded-lg border border-zinc-800 bg-zinc-950/80 p-3 space-y-1.5">
                  <div className="font-semibold text-zinc-300 flex items-center gap-1.5 text-xs">
                    <ImageIcon className="h-3.5 w-3.5 text-primary" />
                    CDP 渲染页面截屏快照:
                  </div>
                  <img
                    src={`data:image/png;base64,${testResult.screenshot_base64}`}
                    alt="Dry run screenshot"
                    className="max-h-56 rounded border border-zinc-800 object-contain mx-auto"
                  />
                </div>
              )}

              {/* 提取结果预览 */}
              <div className="rounded-lg border border-zinc-800 bg-zinc-950/80 p-3 space-y-2">
                <div className="font-semibold text-zinc-300 flex items-center justify-between text-xs">
                  <span>提取数据预览:</span>
                  {testResult.items && (
                    <span className="font-mono text-[11px] text-zinc-500">
                      共 {testResult.items.length} 个条目
                    </span>
                  )}
                </div>

                <div className="max-h-60 overflow-y-auto space-y-2 font-mono text-[11px]">
                  {testResult.items && testResult.items.length > 0 ? (
                    testResult.items.map((it, idx) => (
                      <div
                        key={idx}
                        className="rounded border border-zinc-800 bg-zinc-900/60 p-2.5 space-y-1.5"
                      >
                        <div className="text-zinc-400 font-semibold flex items-center justify-between">
                          <span>条目 #{idx + 1} ID: {it.stable_id}</span>
                          {it.image_urls.length > 0 && (
                            <span className="text-emerald-400 text-[10px]">
                              {it.image_urls.length} 张图片
                            </span>
                          )}
                        </div>
                        {it.text && (
                          <div className="text-zinc-200 whitespace-pre-wrap line-clamp-3">
                            {it.text}
                          </div>
                        )}
                        {Object.keys(it.fields).length > 0 && (
                          <div className="rounded bg-black/40 p-2 text-[10px] text-zinc-400 space-y-0.5">
                            {Object.entries(it.fields).map(([k, v]) => (
                              <div key={k}>
                                <span className="text-zinc-500">{k}:</span> {v}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    ))
                  ) : (
                    <div className="text-zinc-400 italic py-2">
                      无结构化条目；提取纯文本长度: {testResult.text_len ?? 0}
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </Dialog>
  )
}

export function initialForm(editing: SourceMeta | null): FormState {
  return editing ? sourceToForm(editing as SourceConfig) : emptyForm()
}

export { validateCron }
