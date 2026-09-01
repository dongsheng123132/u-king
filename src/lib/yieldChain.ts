/**
 * 让步链 —— 窗口不够宽时，**谁先让位给终端**。
 *
 * ## 为什么需要它
 *
 * U-CLI 里跑的不是我们自己的界面，是**别人的 TUI**（Claude Code / Codex / Hermes 真身）。
 * 那些界面按列数排版，重画不了也不该重画 —— 所以宿主的第一职责就是**给够列数**。
 *
 * 而现在给不够。2026-08-17 按源码算的账（`tauri.conf.json` 声明 `minWidth: 900`）：
 *
 *     主侧栏 208 + 会话栏 230 + 内边距 24 = 462 固定占用
 *     剩 438 按 ratio 0.55 分 → 对话 241 / **终端 197px ≈ 24 列**
 *
 * 拖条把 ratio 夹在 [0.3, 0.78]，所以「用力往左拖」也只到 307px（约 37 列）；
 * 连 `chatCollapsed`（把对话整列藏掉）拿满 438px 都只有约 53 列。
 * **在我们自己声明的最小窗口下，终端怎么拖都到不了 80 列。**
 *
 * 根因不在终端，在那 462px：它是**固定占用**，窗口缩到 900 时一个像素都不让。
 * `useViewport.ts` 其实已经把这笔账算过一遍了，也做了窄屏收窄会话栏 —— 但那是
 * **按窗口宽度**触发的，跟「终端现在饿不饿」无关。终端要多宽这件事，以前没有任何代码在关心。
 *
 * ## 让步链是什么
 *
 * 排好一个**顺序**，挤的时候按顺序让位，而不是所有人同比例一起缩小到都不能用：
 *
 *     level 0  不让步
 *     level 1  会话栏收成 44px 窄条  ← **到此为止**
 *
 * 原来还有个 level 2「再把主侧栏收成 52px 窄条」，2026-08-19 **取消**了：
 * 终端只活在 U-Workspace 里，一进去它就是窄的，于是那一级实际上等于
 * 「点 U-Workspace 就把主侧栏收掉」—— 而那正是客户上一轮要求删掉的行为。
 * 详见下面 `YIELD_MAX` 的注释。**这条链现在只动会话栏。**
 *
 * 两条硬规矩，抄自 DSH `@deepseek-ai/dsh-client-ui-layout` 的让步链：
 *
 * 🔴 **让步是「推导出来的渲染态」，绝不写用户的偏好。** 收起来的那两栏，
 * localStorage 里的 `collapsed` / `width` 一个字节都不动 —— 窗口一变宽，
 * 它们按用户自己拖过的宽度回来。消费方要渲染的是 `collapsed || yieldLevel >= k`，
 * **不是**把 `collapsed` 改成 true。改了偏好就再也回不去了，而用户根本没做过那个决定。
 *
 * 🔴 **窄条不是消失。** 收起的栏保留一条可点回来的窄轨（会话栏 44px 带在跑小圆点、
 * 主侧栏 52px 留图标）。两处 rail 早就实现好了，这里只是替用户按了那一下。
 *
 * ## 🔴 为什么「涨」和「缩」看的不是同一个量
 *
 * 这是本文件唯一容易写错的地方，写错的表现是布局在临界宽度上**反复横跳**。
 *
 * - **升级看实测**：终端面板真实渲染宽度不够 `TERM_FLOOR_PX` 就升一级。
 *   必须实测，因为它同时受用户拖的 ratio、用户拖的会话栏宽度影响，算不准。
 * - **降级看窗口宽度**：`window.innerWidth` 回到「当初升级时的窗口宽 + 余量」才降一级。
 *
 * 降级**不能**也看实测宽度：让步本身会把面板撑宽，于是「宽够了 → 降级 → 又不够了 → 升级」
 * 每帧一个来回。而 `window.innerWidth` 是**外生变量** —— 它不会因为我们收了一栏而变化，
 * 拿它当降级判据，反馈回路就断了。
 *
 * ## 用户随时可以推翻
 *
 * 手动点展开/收起 = `overrideYield()`，让步链就此让开，直到终端面板关掉才复位。
 * 这条跟 SessionList 里那句「用户手调过 —— 他的选择永远赢」是同一条，别在这里破例。
 */

import { useSyncExternalStore } from "react";

/**
 * 终端能排开主流 TUI 的最低宽度（px）。
 *
 * 60 列 × 约 8.2px/列 ≈ 492 → 取 480。8.2 是从实测数据点「197px ≈ 24 列」反推的
 * （197 ÷ 24 = 8.208），字号/字体改了会有几列出入，不影响量级。
 * 取 60 列而不是经典的 80：80 列要 656px，在 1280 宽的机器上会逼出 level 2，
 * 而那类机器横向其实不紧（见 `useViewport.ts` 的推导）—— 门槛定太高等于天天让步。
 */
export const TERM_FLOOR_PX = 480;

/**
 * 降级余量（px）：窗口要比「当初升级时的宽度」再宽这么多才收回让步。
 *
 * 不能是 0：正好等于当初那个宽度时，降完必然又立刻不够。32px 够跨过一次
 * 拖动窗口的抖动，又不至于让用户觉得「明明拉宽了还不还我」。
 */
export const RELEASE_MARGIN_PX = 32;

/** 让步到第几级。0 = 没让步。 */
export type YieldLevel = 0 | 1;

/** 让步链里各栏的出场顺序。谁排前面谁先让。 */
export const YIELD_SESSION_BAR: YieldLevel = 1;

/**
 * 🔴 **主侧栏已退出让步链**（客户 2026-08-19，第二次提同一件事）。
 *
 * 「点击 U-Workspace 默认收起侧栏」那个**明确实现**上一轮就删了（见 `Sidebar.tsx` 里
 * 客户的原话）。但同样的效果换了条路长回来：终端只存在于 U-Workspace 里，进去时它
 * **一出生就窄**（两栏都还开着）→ `reportTermWidth` 报 < 480 → 让步链升到 2 级 →
 * 主侧栏照样收。★ **删掉的是实现，不是行为** —— 跟今天 DSH 那颗按钮、小程序墓碑同形。
 *
 * 当时留着 2 级的理由是「那是窗口尺寸逼出来的，拉宽会自己还原，跟『你点了个导航它就变形』
 * 不是一回事」。这句话在代码里成立，**在用户那边不成立**：触发它的不是拖窗口，是点了那一条。
 * 客户的裁决：「一概不碰主侧栏，人手工点好点，不突兀，好操作。」
 *
 * 所以升级**封顶在 1 级**（只收会话栏）。主侧栏此后只认用户自己点的 `collapsed` 偏好。
 */
const YIELD_MAX: YieldLevel = 1;

/** 让步链的全部状态。`level` 是**渲染态**，不是任何人的偏好。 */
type State = {
  level: YieldLevel;
  /** 升到第 N 级时的 `window.innerWidth`，降级拿它当判据。索引 1 有效。 */
  escalatedAt: [number, number];
  /** 用户手动动过折叠 —— 让步链让开，直到复位。 */
  overridden: boolean;
  /**
   * 当前**替谁**在让步（会话 id）。
   *
   * 工作台里每个会话一个 Chat 实例、全都常驻挂载（display 切换保 PTY），所以同时会有
   * 好几个「终端面板开着」的实例在报数 —— 但只有一个是用户正看着的。可见的那个量得到
   * 真实宽度，隐藏的量到 0。**谁量到非 0 谁就是当前的委托人**；委托人量到 0 就是它被切走了，
   * 让步随即复位。没有这一位，切到一个没开终端的会话时侧栏会一直收着不还。
   */
  owner: string | null;
};

const state: State = { level: 0, escalatedAt: [0, 0], overridden: false, owner: null };

type Listener = () => void;
const listeners = new Set<Listener>();

function notify() {
  for (const fn of listeners) fn();
}

/** 订阅让步级别变化（给 `useSyncExternalStore`）。 */
export function onYieldChange(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** 当前该让到第几级。用户推翻过就一直是 0。 */
export function yieldLevel(): YieldLevel {
  return state.overridden ? 0 : state.level;
}

function winWidth(): number {
  return typeof window === "undefined" ? 0 : window.innerWidth;
}

function setLevel(next: YieldLevel) {
  if (state.level === next) return;
  state.level = next;
  notify();
}

/**
 * 终端面板报一次自己的实测宽度。宽度不够就升级，窗口够宽了就降级。
 *
 * `px <= 0` = 这个会话的面板正被 `display:none` 藏着（`getBoundingClientRect` 就是 0）。
 * 🔴 **它不表示「窄到极点」，表示「不是我在用」** —— 当成饥饿处理会让一屋子隐藏的会话
 * 一起把侧栏收掉。是委托人报的 0，才说明用户切走了，这时候才复位。
 */
export function reportTermWidth(owner: string, px: number) {
  if (px <= 0) {
    if (state.owner === owner) resetYield();
    return;
  }
  // 换了个会话在用终端：上一个的让步理由随它一起切走了，从头算（包括清掉它的 override）。
  if (state.owner !== owner) {
    state.owner = owner;
    state.level = 0;
    state.escalatedAt = [0, 0];
    state.overridden = false;
    notify();
  }
  if (state.overridden) return;
  const w = winWidth();

  if (px < TERM_FLOOR_PX && state.level < YIELD_MAX) {
    const next = (state.level + 1) as YieldLevel;
    state.escalatedAt[next] = w;
    setLevel(next);
    return;
  }

  // 降级一次只退一级：退完这一级如果还是不够，下一帧的实测会把它顶回来，
  // 而不是一口气退到 0 再从头升 —— 那个过程用户看得见（两栏一起闪一下）。
  if (state.level > 0 && w > state.escalatedAt[state.level] + RELEASE_MARGIN_PX) {
    setLevel((state.level - 1) as YieldLevel);
  }
}

/**
 * 用户手动动了折叠 —— 让步链让开。
 *
 * 不是「暂停一会儿」而是**一直让开到复位**：如果只压制几秒，用户展开会话栏之后
 * 它会自己再收回去，那比不做还糟 —— 界面在跟人较劲。
 */
export function overrideYield() {
  if (state.overridden) return;
  state.overridden = true;
  notify();
}

/**
 * 复位：终端面板关掉了 / 切走了，没有要让位的对象了。
 *
 * 同时清掉 `overridden` —— 下次再开终端是一次新的处境，用户上次那下不该一直生效。
 */
export function resetYield() {
  if (state.level === 0 && !state.overridden && state.owner === null) return;
  state.level = 0;
  state.escalatedAt = [0, 0];
  state.overridden = false;
  state.owner = null;
  notify();
}

/**
 * 某个会话不再需要让步了（终端面板关掉 / 切到别的面板 / Chat 卸载）。
 *
 * 只有**当前委托人**说话才作数 —— 否则一个后台会话关掉它自己的终端，会把前台
 * 正在让步的那个一起复位。
 */
export function releaseYield(owner: string) {
  if (state.owner === owner) resetYield();
}

/**
 * 订阅让步级别。
 *
 * 🔴 消费方要渲染的是 `collapsed || useYieldLevel() >= YIELD_XXX`，
 * **不是**把自己的 `collapsed` 偏好改成 true。让步是渲染态，偏好是用户的东西。
 */
export function useYieldLevel(): YieldLevel {
  return useSyncExternalStore(onYieldChange, yieldLevel, () => 0 as YieldLevel);
}
