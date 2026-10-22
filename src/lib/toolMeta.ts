/**
 * 工具分类 + 排序 —— 新首页（`home/HomePage`）和软件库（`library/LibraryPage`）共用这一份。
 *
 * 为什么不各写一份：分类口径一旦分叉，「我的 AI」说它是智能体、软件库说它是命令行，
 * 客户就会以为这是两个不同的软件（2026-10-22 用户口径：「软件要能加，能分类」）。
 *
 * 分类来源：后端 `ToolInfo` 目前只序列化 `kind` / `launch_app`，没有权威分类字段，
 * 所以这里按现有字段做展示级映射（口径与旧 ToolHub 一致）：
 *   - 智能体：`kind === "deep"`（深度接入的对话/编码代理，如 Claude Code / Codex / Hermes）；
 *   - 桌面应用：`launch_app` 非空（有独立 GUI 程序，如 ClawX / DSH / magpie）；
 *   - 命令行：其余。
 * 后端以后补 `launch_mode` 时应直接切过去读，不再猜。
 */
import type { ToolInfo } from "../App";

export type ToolCategory = "all" | "agent" | "gui" | "cli";

export const TOOL_CATEGORIES: { id: ToolCategory; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "agent", label: "智能体" },
  { id: "gui", label: "桌面应用" },
  { id: "cli", label: "命令行" },
];

export function categoryOf(t: Pick<ToolInfo, "kind" | "launch_app">): ToolCategory {
  if (t.kind === "deep") return "agent";
  if (t.launch_app) return "gui";
  return "cli";
}

export function categoryLabel(id: ToolCategory): string {
  return TOOL_CATEGORIES.find((c) => c.id === id)?.label ?? "";
}

/**
 * 软件库排序：推荐位在前（严格保持 `RECOMMENDED_TOOLS` 的顺序），其余按名字排。
 * 推荐名单是 App.tsx 里的一份常量——「推荐谁」不在这里另立一套。
 */
export function sortForLibrary(tools: ToolInfo[], recommended: readonly string[]): ToolInfo[] {
  const rank = new Map(recommended.map((id, index) => [id, index] as const));
  const fallback = recommended.length;
  return [...tools].sort((a, b) => {
    const ra = rank.get(a.id) ?? fallback;
    const rb = rank.get(b.id) ?? fallback;
    if (ra !== rb) return ra - rb;
    return a.name.localeCompare(b.name, "zh-Hans-CN");
  });
}
