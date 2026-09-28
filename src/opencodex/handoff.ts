/**
 * 护照交接的**落点** —— 从一张任务护照到一个真实会话之间的那一跳。
 *
 * ## 为什么需要这么个东西
 * 交接以前只写剪贴板：用户点完「交接」，任务去了哪里**没有任何人回答**。
 * 剪贴板成功了看不见、失败了退化成一句普通提示，两种结局在界面上长得一样。
 * 用户的原话是「点完不知道任务去了哪儿」—— 那不是文案问题，是**这一跳根本不存在**。
 *
 * 现在这一跳是真的：建/复用一个会话 → 把护照上下文发进去 → 会话自己回一声「收到了」。
 *
 * ## 为什么是一个模块级信箱，而不是 props
 * 交接发生时**目标 Chat 还没挂载**（会话是刚建的，React 下一帧才渲染）。
 * 把一个一次性的值顺着 `UWorkspace → Chat` 串成 state，等于为一次投递维护一份长期状态；
 * 而按 sessionId 存一封信、收件人挂载时自取，形状跟这件事本身一样。
 *
 * ## 送达是**回执**，不是我们自己说的
 * `deliver()` 只有在 Chat 真的把它发出去之后才会被调用。界面上那句「已送达」
 * 因此是收件人签的字，不是发件人的一厢情愿 —— 这正是旧实现缺的那一半：
 * 它连剪贴板写没写成都不敢说，更别提任务到没到。
 *
 * 超时没等到回执就如实说「没送达」并把原因摆出来。**不许把「不知道」画成成功。**
 */
import type { Engine } from "./types";

/** 一次交接：把哪张护照、用哪个大脑、发什么内容，交给哪个会话。 */
export type Handoff = {
  /** 护照 id，回执和界面都用它对账。 */
  passportId: string;
  /** 接手的大脑（对应 Chat 顶栏那个下拉，不另造一套 AI 目标概念）。 */
  engine: Engine;
  /** 真正发给 AI 的那段话（护照编译出来的上下文 + 接手指令）。 */
  prompt: string;
};

/** 收件箱：sessionId → 待取的那封信。取走即删（一次性）。 */
const inbox = new Map<string, Handoff>();

/** 回执：sessionId → 已送达的护照 id。 */
const delivered = new Map<string, string>();

type Listener = () => void;
const listeners = new Set<Listener>();

function notify() {
  for (const fn of listeners) fn();
}

/** 订阅回执变化（护照页用它把「投递中」翻成「已送达」）。 */
export function onHandoffChange(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** 投一封信给某个会话。会话挂载后自取。 */
export function queueHandoff(sessionId: string, h: Handoff) {
  inbox.set(sessionId, h);
  delivered.delete(sessionId);
  notify();
}

/**
 * 收件人自取。**取走即删** —— 否则会话每次重挂载（切视图、刷新）都会把同一段护照再发一遍，
 * 那比不投递更糟：用户会看到 AI 反复接手同一个任务。
 */
export function takeHandoff(sessionId: string): Handoff | null {
  const h = inbox.get(sessionId);
  if (!h) return null;
  inbox.delete(sessionId);
  return h;
}

/** 收件人签字：这封信我真的发出去了。 */
export function deliver(sessionId: string, passportId: string) {
  delivered.set(sessionId, passportId);
  notify();
}

/** 这个会话签收过哪张护照（没有则 null）。 */
export function deliveredPassport(sessionId: string): string | null {
  return delivered.get(sessionId) ?? null;
}

/**
 * 交接要发的那段话。
 *
 * `compiled` 是**后端**（`origin.rs::compile_context`）编译出来的状态摘要 ——
 * 前端不再自己拼一段「护照大意」：拼第二份的那一刻，它就会跟真状态漂开，
 * 而漂开的那次正好是出事那次。前端只负责在它前面加一句「你要干什么」。
 *
 * 拿不到 compiled（后端读盘失败）时**只发护照号**，并明说上下文没带上 ——
 * 宁可让接手方自己去读，也不能让它以为「我拿到全部状态了」。
 */
export function buildHandoffPrompt(passportId: string, compiled: string | null): string {
  const head =
    `请接手任务护照 ${passportId}：先读取当前状态与下一步，` +
    `只继承已验证事实，不继承上一位 AI 的聊天记录。`;
  if (!compiled || !compiled.trim()) {
    return (
      head +
      `\n\n（注意：这次没能读出护照正文，上面只有护照号。` +
      `动手前请先自己把 ${passportId} 的状态读出来，不要按猜测干活。）`
    );
  }
  return `${head}\n\n${compiled.trim()}`;
}
