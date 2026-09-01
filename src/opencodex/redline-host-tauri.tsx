/**
 * Redline 的 Tauri 宿主实现（U-King 版）—— 把 redline-core 的「万能文档预览」接进工作台。
 * 全部复用 U-King 现成能力，零新增 Rust command：
 *   readFileBytes / getSrcUrl → asset 协议（Cargo.toml 已开 protocol-asset，Video.tsx 已在用 convertFileSrc）
 *   openExternal → plugin-opener 的 openPath（系统默认程序打开，渲染失败兜底）
 *   renderToPdf → officedoc.rs 借本机 LibreOffice
 *   renderMarkdown → 复用聊天气泡那套 MiniMd（内核自己不带 markdown 渲染器，理由见 host-adapter）
 *
 * 独立可插拔：本文件只 import redline-core + tauri 现成 API + 一个纯前端渲染器，
 * 不碰任何 U-King 后端模块。
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import type { RedlineHost } from "../vendor/redline-core/index";
import { MiniMd } from "../lib/miniMd";

/** 建一个 Tauri RedlineHost。 */
export function createTauriRedlineHost(): RedlineHost {
  return {
    async readFileBytes(path: string): Promise<ArrayBuffer> {
      const res = await fetch(convertFileSrc(path));
      if (!res.ok) {
        // 🔴 别只抛状态码。客户截图里就是一句光秃秃的 `读取文件失败: HTTP 404`：
        // 既不知道我们去找的是哪个路径，也不知道 404 和 403 是两回事 ——
        // 404 = 那个路径上没有这个文件；403 = 文件在，但这个目录没进 asset 白名单
        // （`allow_fs_preview` 漏调了）。两种的下一步动作完全不同。
        const why =
          res.status === 404
            ? "这个路径上没有这个文件"
            : res.status === 403
              ? "这个目录没被允许预览（allow_fs_preview 没调到）"
              : `HTTP ${res.status}`;
        throw new Error(`${why}：${path}`);
      }
      return res.arrayBuffer();
    },
    getSrcUrl(path: string): string {
      return convertFileSrc(path);
    },
    /** markdown 渲染复用聊天气泡那份 MiniMd —— 全站只有一套 md 语法口径，改一处两边都跟上。 */
    renderMarkdown(text: string) {
      return <MiniMd text={text} />;
    },
    async openExternal(path: string): Promise<void> {
      await openPath(path);
    },
    /** 办公文档 → PDF（后端 officedoc.rs 借 LibreOffice headless）。
     *  没装 LibreOffice / 格式不归它管都返回 null —— redline 会安静退回原来的档。 */
    async renderToPdf(path: string): Promise<string | null> {
      return (await invoke<string | null>("office_to_pdf", { path })) ?? null;
    },
  };
}
