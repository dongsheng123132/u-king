/**
 * 「在哪个文件夹里打开 <工具>」的最近文件夹记忆 —— 纯函数 + 一层很薄的 localStorage 读写。
 *
 * 为什么要有它：命令行工具（Claude Code / Codex / pi / OpenCode / Hermes …）一启动就在
 * **当前目录**里干活——读代码、改文件、建文件都落在那儿。以前从「我的 AI」点启动，终端一律落在
 * 用户主目录（`C:\Users\xxx`），小白根本不知道 AI 正在他的「主目录」里翻东西。所以每次启动先问
 * 一句在哪个文件夹，并把最近用过的列出来，一键复用。
 *
 * 数据形态（localStorage 键 `uking.launchFolders`，带 `v` 版本号，以后改结构时按 `v` 迁移）：
 *   { v: 1,
 *     items:      [{ path, usedAt }]   // 全局最近用过的文件夹，最新在前，去重，≤ MAX_RECENT
 *     lastByTool: { [toolId]: path } } // 每个工具上次用的文件夹（弹窗默认高亮它）
 *
 * 本文件**不碰 DOM、不碰 Tauri**：判「这个目录还在不在」要问后端（`produced_file_info`），
 * 由调用方查完把「不在的」交给 `dropMissing`；存取只有 `loadState` / `saveState` 两个函数读写
 * localStorage，其余全是不可变的纯函数——可以直接用 node 跑断言
 * （`node scripts/check-recent-folders.mts`，见该脚本头注释）。
 *
 * ⚠️ 只用「可擦除」的 TS 语法（无 enum / 参数属性 / namespace），node 的类型剥离才能直接 import 它。
 */

export const RECENT_FOLDERS_KEY = "uking.launchFolders";
export const RECENT_FOLDERS_VERSION = 1;
/** 弹窗里最多列几个最近文件夹。 */
export const MAX_RECENT = 8;

export type RecentFolder = { path: string; usedAt: number };

export type RecentFoldersState = {
  v: typeof RECENT_FOLDERS_VERSION;
  items: RecentFolder[];
  lastByTool: Record<string, string>;
};

export function emptyState(): RecentFoldersState {
  return { v: RECENT_FOLDERS_VERSION, items: [], lastByTool: {} };
}

/** 比较路径用的归一化键：统一斜杠方向、去掉末尾斜杠（盘符根 `D:\` 除外）；
 *  Windows 风格路径（盘符 / UNC）大小写不敏感，其余（POSIX）区分大小写。 */
export function pathKey(path: string): string {
  let p = path.trim().replace(/\\/g, "/");
  const isRoot = /^[A-Za-z]:\/?$/.test(p) || p === "/";
  if (!isRoot) p = p.replace(/\/+$/, "");
  else if (/^[A-Za-z]:$/.test(p)) p += "/";
  const windowsLike = /^[A-Za-z]:\//.test(p) || p.startsWith("//");
  return windowsLike ? p.toLowerCase() : p;
}

export function samePath(a: string, b: string): boolean {
  return pathKey(a) === pathKey(b);
}

/** 文件夹显示名：路径最后一段；盘符根 / 根目录没有「最后一段」，原样显示路径。 */
export function folderName(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, "");
  const last = trimmed.split(/[\\/]/).pop() ?? "";
  if (!last || /^[A-Za-z]:$/.test(last)) return path.trim();
  return last;
}

/** 宽松解析：坏 JSON / 版本不认识 / 字段类型不对，一律当「没有记录」，绝不抛错
 *  （一个读不懂的旧缓存不该让「启动工具」弹窗打不开）。 */
export function parseState(raw: string | null | undefined): RecentFoldersState {
  if (!raw) return emptyState();
  try {
    const v = JSON.parse(raw) as Partial<RecentFoldersState> | null;
    if (!v || typeof v !== "object" || v.v !== RECENT_FOLDERS_VERSION) return emptyState();
    const items: RecentFolder[] = [];
    for (const it of Array.isArray(v.items) ? v.items : []) {
      if (it && typeof it.path === "string" && it.path.trim() && typeof it.usedAt === "number") {
        if (!items.some((x) => samePath(x.path, it.path))) items.push({ path: it.path, usedAt: it.usedAt });
      }
    }
    items.sort((a, b) => b.usedAt - a.usedAt);
    const lastByTool: Record<string, string> = {};
    if (v.lastByTool && typeof v.lastByTool === "object") {
      for (const [k, p] of Object.entries(v.lastByTool)) if (typeof p === "string" && p.trim()) lastByTool[k] = p;
    }
    return { v: RECENT_FOLDERS_VERSION, items: items.slice(0, MAX_RECENT), lastByTool };
  } catch {
    return emptyState();
  }
}

/** 记一次使用：该文件夹移到最前（已有则只更新时间，不重复），并记成该工具的「上次用的」。
 *  超过 `MAX_RECENT` 的最旧项被挤掉（连带清掉指向它的 `lastByTool`，免得留下列表里没有的默认项）。 */
export function recordUse(state: RecentFoldersState, toolId: string, path: string, now: number): RecentFoldersState {
  const p = path.trim();
  if (!p) return state;
  const rest = state.items.filter((x) => !samePath(x.path, p));
  const items = [{ path: p, usedAt: now }, ...rest].slice(0, MAX_RECENT);
  const lastByTool = { ...state.lastByTool, [toolId]: p };
  return { v: RECENT_FOLDERS_VERSION, items, lastByTool: pruneLast(items, lastByTool) };
}

/** 把调用方查出来「已经不存在」的路径从列表和各工具的默认项里剔掉。 */
export function dropMissing(state: RecentFoldersState, missing: readonly string[]): RecentFoldersState {
  if (missing.length === 0) return state;
  const gone = (p: string) => missing.some((m) => samePath(m, p));
  const items = state.items.filter((x) => !gone(x.path));
  const lastByTool: Record<string, string> = {};
  for (const [k, p] of Object.entries(state.lastByTool)) if (!gone(p)) lastByTool[k] = p;
  return { v: RECENT_FOLDERS_VERSION, items, lastByTool: pruneLast(items, lastByTool) };
}

function pruneLast(items: RecentFolder[], lastByTool: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, p] of Object.entries(lastByTool)) if (items.some((x) => samePath(x.path, p))) out[k] = p;
  return out;
}

/** 弹窗里默认高亮哪一项：该工具上次用的（仍在列表里）；没有就取全局最近一个；列表为空 → null。
 *  返回的是 `items` 里那一项的原始路径（不是归一化键）。 */
export function defaultFor(state: RecentFoldersState, toolId: string): string | null {
  const last = state.lastByTool[toolId];
  if (last) {
    const hit = state.items.find((x) => samePath(x.path, last));
    if (hit) return hit.path;
  }
  return state.items[0]?.path ?? null;
}

/** 按「最近使用」降序给弹窗列出的最多 `MAX_RECENT` 项。 */
export function listRecent(state: RecentFoldersState): RecentFolder[] {
  return [...state.items].sort((a, b) => b.usedAt - a.usedAt).slice(0, MAX_RECENT);
}

// ── localStorage 读写（唯一碰存储的两个函数）──────────────────────────────

export function loadState(): RecentFoldersState {
  try {
    return parseState(localStorage.getItem(RECENT_FOLDERS_KEY));
  } catch {
    return emptyState(); // localStorage 不可用（隐私模式等）：当没有记录
  }
}

export function saveState(state: RecentFoldersState): void {
  try {
    localStorage.setItem(RECENT_FOLDERS_KEY, JSON.stringify(state));
  } catch {
    /* 写不进去只影响「下次默认选哪个」，不阻断这次启动 */
  }
}
