import { useEffect, useState } from "react"
import { fetchBlobUrl } from "@/lib/api"

/**
 * 带鉴权头加载的图片：截图接口受 Bearer Token 保护，原生 `<img>` 无法附加
 * Authorization 头，故经 `fetchBlobUrl` 拉取 blob 后用 objectURL 渲染。
 * 组件卸载时自动 revoke，避免内存泄漏。
 */
export function AuthImage({ url, alt, className }: { url: string; alt: string; className?: string }) {
  const [src, setSrc] = useState<string | null>(null)

  useEffect(() => {
    let objectUrl: string | null = null
    let cancelled = false
    fetchBlobUrl(url)
      .then((blobUrl) => {
        if (cancelled) {
          URL.revokeObjectURL(blobUrl)
          return
        }
        objectUrl = blobUrl
        setSrc(blobUrl)
      })
      .catch(() => {
        // 鉴权失败 / 加载失败：留空由占位展示，不阻塞列表。
        if (!cancelled) setSrc(null)
      })
    return () => {
      cancelled = true
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [url])

  if (!src) return null
  return <img src={src} alt={alt} className={className} loading="lazy" />
}
