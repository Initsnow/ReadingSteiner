import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// 校验标准 5 段 cron 表达式（分 时 日 月 周）。合法返回 null，否则返回错误信息。
// 与后端 cron crate 的解析约定对齐：5 段、支持 *、*/step、数字、a-b 范围、逗号列表、? 及命名星期/月份。
export function validateCron(expr: string): string | null {
  const trimmed = expr.trim()
  if (!trimmed) return "cron 表达式不能为空"
  const parts = trimmed.split(/\s+/)
  if (parts.length !== 5) {
    return `cron 表达式需要 5 段（分 时 日 月 周），当前 ${parts.length} 段`
  }
  // 各字段合法数值范围：分/时/日/月/周。
  const ranges: Array<[number, number]> = [
    [0, 59], // 分
    [0, 23], // 时
    [1, 31], // 日
    [1, 12], // 月
    [0, 7], // 周（0/7 均表示周日）
  ]
  for (let i = 0; i < 5; i++) {
    const field = parts[i]
    const [min, max] = ranges[i]
    const tokens = field.split(",")
    for (const tok of tokens) {
      if (!tok) return `第 ${i + 1} 段存在空的列表项`
      if (tok === "?" || tok === "*") continue
      // */step
      const stepMatch = tok.match(/^\*\/(\d+)$/)
      if (stepMatch) {
        if (parseInt(stepMatch[1], 10) < 1) return `第 ${i + 1} 段步长需 ≥ 1`
        continue
      }
      // 数字或 a-b 范围
      const numMatch = tok.match(/^(\d+)(?:-(\d+))?$/)
      if (numMatch) {
        const a = parseInt(numMatch[1], 10)
        const b = numMatch[2] ? parseInt(numMatch[2], 10) : a
        if (a < min || a > max || b < min || b > max) {
          return `第 ${i + 1} 段数值超出范围 ${min}-${max}`
        }
        if (b < a) return `第 ${i + 1} 段范围结束值不能小于起始值`
        continue
      }
      // 命名星期/月份（SUN、MON、JAN 等）由后端 cron crate 解析，宽松放行。
      if (/^[A-Za-z]{3}$/.test(tok)) continue
      return `第 ${i + 1} 段包含无法识别的 cron 语法："${tok}"`
    }
  }
  return null
}
