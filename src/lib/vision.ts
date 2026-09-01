/** 图片附件统一走 ActionParity 的 media.image.describe。
 *
 * 主对话（DeepSeek / Claude / Codex）只接收本文件构造的文字，不会收到原图路径、base64 或 image_url。
 */
import { invoke } from "@tauri-apps/api/core";

const EXT = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "heic", "heif"]);

export function isImageFile(path: string): boolean {
  const ext = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
  return EXT.has(ext);
}

export function fileLabel(path: string): string {
  return path.split(/[\\/]/).pop() || "图片";
}

type VisionResult = { text: string; model: string; source: string; fallback_from?: string; cached?: boolean };

/** 用户选择附件即为一次明确同意；CLI/MCP 仍须由其各自的确认门通过。 */
export async function describeImages(paths: string[], question: string): Promise<string> {
  const blocks: string[] = [];
  for (const image of paths) {
    const requestId = globalThis.crypto?.randomUUID?.() ?? `vision-${Date.now()}-${Math.random()}`;
    const response: any = await invoke("action_parity_call", {
      request: {
        action_id: "media.image.describe",
        input: { image, question, request_id: requestId },
        confirmed: true,
        surface: "desktop",
      },
    });
    if (!response?.ok) throw new Error(response?.error?.message || response?.error || "图片识别失败");
    const result = response.result as VisionResult;
    if (!result?.text || !result?.model) throw new Error("图片识别没有返回文字");
    // source 来自受控后端的文件名；这里也不回传完整本地路径。
    blocks.push(`【图片识别（${result.model}，${result.source || fileLabel(image)}）】\n${result.text}`);
  }
  return blocks.join("\n\n");
}
