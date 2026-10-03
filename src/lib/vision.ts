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

/** 待发送的图片附件。`path` 是暂存副本（识图读它），`source` 是用户拖入时的原路径（只用来去重）。 */
export type PendingImage = { path: string; source: string };

/** 拖入/粘贴时立刻把图片复制成我们自己的副本（后端 `vision::stage_image`），返回副本路径。
 *  不这么做的话，从截图工具窗口直接拖出的临时图片会在发送前被系统清掉，这一轮就永远发不出去。 */
export function stageImage(path: string): Promise<string> {
  return invoke<string>("stage_image_attachment", { path });
}

/** 输入框里代表一张图片的标签。增删都用这一个函数生成，两个面板的标签文字才永远对得上。 */
export function imageLabel(path: string): string {
  return `【已附图片：${fileLabel(path)}，发送时先识图】`;
}

/** 从输入框文字里去掉某张图的标签（连同它前面的一个空格，只去一处 —— 同名图片的标签是同一段文字）。 */
export function stripImageLabel(text: string, path: string): string {
  const label = imageLabel(path);
  const at = text.indexOf(label);
  if (at < 0) return text;
  const from = at > 0 && text[at - 1] === " " ? at - 1 : at;
  return text.slice(0, from) + text.slice(at + label.length);
}

/** 去掉后端协议错误的 `image_missing:` 这类 `小写_下划线:` 前缀，剩下的是写给人看的话。 */
export function cleanVisionError(msg: string): string {
  return msg.replace(/^\s*[a-z][a-z0-9]*(?:_[a-z0-9]+)+:\s*/, "").trim();
}

/** 任意错误值 → 给界面展示的一句话（Error 取 message，不带 "Error: " 前缀；去协议前缀）。 */
export function visionErrorText(e: unknown): string {
  return cleanVisionError(e instanceof Error ? e.message : String(e));
}

/** 识图失败。带上失败的是哪一张（`image`，即传给后端的路径）和后端错误码（`code`，
 *  如 `image_missing`），调用方据此决定是整体恢复还是只摘掉坏的那张。 */
export class ImageDescribeError extends Error {
  readonly image: string;
  readonly code: string;
  constructor(message: string, image: string, code: string) {
    super(message);
    this.name = "ImageDescribeError";
    this.image = image;
    this.code = code;
  }
}

type VisionResult = { text: string; model: string; source: string; fallback_from?: string; cached?: boolean };

/** 用户选择附件即为一次明确同意；CLI/MCP 仍须由其各自的确认门通过。 */
export async function describeImages(paths: string[], question: string): Promise<string> {
  const blocks: string[] = [];
  for (const image of paths) {
    const requestId = globalThis.crypto?.randomUUID?.() ?? `vision-${Date.now()}-${Math.random()}`;
    let response: any;
    try {
      response = await invoke("action_parity_call", {
        request: {
          action_id: "media.image.describe",
          input: { image, question, request_id: requestId },
          confirmed: true,
          surface: "desktop",
        },
      });
    } catch (e) {
      throw new ImageDescribeError(e instanceof Error ? e.message : String(e), image, "unknown");
    }
    if (!response?.ok) {
      const err = response?.error;
      throw new ImageDescribeError(String(err?.message || err || "图片识别失败"), image, String(err?.code ?? "unknown"));
    }
    const result = response.result as VisionResult;
    if (!result?.text || !result?.model) throw new ImageDescribeError("图片识别没有返回文字", image, "unknown");
    // source 来自受控后端的文件名；这里也不回传完整本地路径。
    blocks.push(`【图片识别（${result.model}，${result.source || fileLabel(image)}）】\n${result.text}`);
  }
  return blocks.join("\n\n");
}
