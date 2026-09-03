import { readFileSync } from "node:fs"
import { fileURLToPath, URL } from "node:url"

import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"),
) as { version?: string }

export default defineConfig({
  plugins: [react()],
  // 版本号注入前端，避免侧栏硬编码与 package.json 漂移。
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version ?? "0.0.0"),
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8901",
        changeOrigin: true,
      },
    },
  },
})
