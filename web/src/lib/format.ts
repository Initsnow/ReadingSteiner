/** 展示层格式化：相对时间、绝对时间。 */

export function formatRelativeTime(ts: string | null): string {
  if (!ts) return "—"
  const d = new Date(ts)
  if (isNaN(d.getTime())) return "—"
  const diff = Date.now() - d.getTime()
  if (diff < 0) return "刚刚"
  const sec = Math.floor(diff / 1000)
  if (sec < 60) return `${sec} 秒前`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min} 分钟前`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr} 小时前`
  const day = Math.floor(hr / 24)
  if (day < 30) return `${day} 天前`
  return d.toLocaleDateString()
}

export function formatDateTime(ts: string | null): string {
  if (!ts) return "—"
  const d = new Date(ts)
  return isNaN(d.getTime()) ? "—" : d.toLocaleString()
}
