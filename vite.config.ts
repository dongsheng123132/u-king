import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import pkg from "./package.json";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Tauri 期望固定端口；用相对 base 让打包后的 dist 能在 tauri://localhost 下直接加载。
export default defineConfig(async () => ({
  plugins: [
    react(),
    {
      // index.html 里的早期兜底脚本要把版本号发给 bug 接口（服务端校 x-uking-version）。
      // 那段脚本跑在主包之前，拿不到 __APP_VERSION__，所以在这儿做 HTML 层替换 ——
      // 版本号仍然只有 package.json 一个来源，不新增第六处要同步的地方。
      name: "uking-html-version",
      transformIndexHtml: (html: string) => html.replace(/%APP_VERSION%/g, pkg.version),
    },
  ],
  base: "./",
  // __APP_VERSION__：给 vendored 的 opencodex 组件（SessionList 页脚版本号）用，取 package.json 版本
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
