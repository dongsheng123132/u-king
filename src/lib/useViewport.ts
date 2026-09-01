/**
 * 窗口尺寸档位 —— **全应用唯一的窄屏/矮屏口径**（宪法第 8 条：同一事实只留一份）。
 *
 * ## 为什么需要它
 *
 * 这个界面原本**没做窄屏适配**：全仓 57 处 Tailwind 断点、`window.innerWidth` / `matchMedia`
 * 零处，两级导航栏（主侧栏 208px + 会话栏 230px = **固定 438px**）无论窗口多小都原样占着。
 * 而 `tauri.conf.json` 声明 `minWidth: 900` —— 那时留给对话的只剩 438px。
 *
 * 客户机实测（2026-08-05，1920×1128 物理 ÷ 150% 缩放 = **1280×752 CSS**）同时炸三样：
 *   ① 挤 —— 底部「Claude Code（推荐·最强·已…」被截断；
 *   ② 溢出 —— 侧栏展开「更多」自己长出滚动条，对话区右边缘也有一条；
 *   ③ 大片空白 —— 溢出和留白**同时**存在，就是纵向压根没做分配的典型症状。
 *
 * ## 为什么不用 Tailwind 断点就完事
 *
 * `lg:` / `xl:` 能解决「显示/隐藏一段文字」，但解决不了**行为**：窄屏该自动折叠会话栏，
 * 而折叠态是组件自己的 state + localStorage 偏好。那必须拿到 JS 里的实际尺寸。
 * 纯 CSS 的另一个问题是没法「尊重用户手动选择」——见 SessionList 的 auto-collapse。
 *
 * ## 阈值怎么定的（别随手改）
 *
 * - `narrow: width ≤ 1280` —— **不是拍脑袋取的整数，是算出来的**：
 *   主侧栏 208 + 会话栏 230 + main 的 p-3 左右 24 + 对话内容上限 820（`Chat.tsx`）
 *   = **1282px**。窗口宽过它，对话区就够摆下全宽内容，横向什么都不用收；窄过它，
 *   固定占用才真的开始吃内容。取 1280 这个标准断点，正好落在 1282 下面。
 *   ⚠️ 由此，**1366×768 笔记本不算 narrow**（它有 904px 给对话，够）——
 *   那台机器的问题几乎全在纵向，别拿横向的收窄去"修"一个它没有的毛病。
 * - `short: height < 780` —— 关键是算**客户区**高度而不是屏幕高度：
 *     · 1366×768 屏：768 − 任务栏 40 − 标题栏 32 ≈ **696** → 命中
 *     · 1080p @150% 缩放：CSS 屏高 720，客户区 ≈ **648** → 命中
 *     · 1080p @100%：客户区 ≈ **1008** → 不命中（正常机器不该被降级成紧凑版）
 *     · 1080p @125%：客户区 ≈ **806** → 不命中，留了 26px 余量防止在临界值上反复横跳
 *
 * 用 `matchMedia` 而不是 resize 事件：浏览器只在**跨过阈值**时才通知，不会在拖动窗口
 * 的每一帧都触发 setState（这个应用里有常驻的 xterm PTY 和 ResizeObserver，高频重渲染
 * 会连累它们）。
 */
import { useEffect, useState } from "react";

/** 窄屏阈值（px）。到此值及以下，横向固定占用开始吃对话内容（推导见文件头）。 */
export const NARROW_MAX = 1280;
/** 矮屏阈值（px）。低于此值开始收纵向的固定占用（副标题、padding、空态留白）。 */
export const SHORT_MAX = 779;

const NARROW_Q = `(max-width: ${NARROW_MAX}px)`;
const SHORT_Q = `(max-height: ${SHORT_MAX}px)`;

export type Viewport = {
  /** 窗口窄：横向固定占用（两级导航栏）该让位给内容。 */
  narrow: boolean;
  /** 窗口矮：纵向固定占用（副标题、padding、空态留白）该收紧。 */
  short: boolean;
  /** 任一维度吃紧 —— 大多数「要不要用紧凑排版」的地方看这个就够。 */
  compact: boolean;
};

/** SSR / 极早期调用时 matchMedia 可能不在，一律按「宽松」兜底（宁可不降级，别误伤正常机器）。 */
function read(query: string): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia(query).matches;
}

/**
 * 订阅一条 media query。返回 unsubscribe。
 * 老 WebView 只有 `addListener`（`addEventListener` 是后加的）——这个应用跑在客户机的
 * WebView2 上，版本不可控，两条都挂上。
 */
function subscribe(query: string, cb: () => void): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => {};
  const mql = window.matchMedia(query);
  if (typeof mql.addEventListener === "function") {
    mql.addEventListener("change", cb);
    return () => mql.removeEventListener("change", cb);
  }
  // eslint-disable-next-line @typescript-eslint/no-deprecated
  mql.addListener(cb);
  // eslint-disable-next-line @typescript-eslint/no-deprecated
  return () => mql.removeListener(cb);
}

/** 当前窗口的尺寸档位，跨过阈值时才重渲染。 */
export function useViewport(): Viewport {
  const [vp, setVp] = useState<{ narrow: boolean; short: boolean }>(() => ({
    narrow: read(NARROW_Q),
    short: read(SHORT_Q),
  }));

  useEffect(() => {
    const sync = () => setVp({ narrow: read(NARROW_Q), short: read(SHORT_Q) });
    // 挂载后再同步一次：首帧的 useState initializer 可能跑在窗口拿到最终尺寸之前
    //（Tauri 起窗时 WebView 先按默认尺寸布一次，随后才 resize 到 tauri.conf 的值）。
    sync();
    const offN = subscribe(NARROW_Q, sync);
    const offS = subscribe(SHORT_Q, sync);
    return () => { offN(); offS(); };
  }, []);

  return { narrow: vp.narrow, short: vp.short, compact: vp.narrow || vp.short };
}
