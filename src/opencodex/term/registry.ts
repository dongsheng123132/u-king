/**
 * 前端「我还在用哪些 PTY 会话」的登记表。
 *
 * ★ 为什么需要它：保活心跳（main.tsx）是**全局**的，而会话表 `sessionsRef` 挂在
 * useTermGroup 的每个实例上（分屏 / 多任务面板会有好几份），全局心跳根本看不到。
 * 后端 `term_ping` 拿不到归属，就只能把会话表**整张刷活** —— 于是 WebView2 崩溃或刷新后
 * 遗留的孤儿会话会被新前端的心跳一起续命，永远等不到 HEARTBEAT_TIMEOUT 回收。
 * 这就是 2026-08-06「实机 35 个泄漏会话」的根因。
 *
 * 当时的补救是在后端加 `MAX_SESSIONS` 硬上限「超了就回收最旧的」，但这是拿**会话总数**
 * 去近似「有没有泄漏」—— 用户合法开 17 个标签和漏了 17 个孤儿，在计数上毫无区别。
 * 更糟的是 `last_seen` 被全表刷成同一时刻后排序键全相等，「最旧」退化成 HashMap 的
 * 随机迭代顺序，实际效果是**随机杀掉一个活着的终端**（详见 term.rs::reap_stale）。
 *
 * 有了这张表，判据就从「数量」换成了准确的「还有没有人认领」，硬上限随之被删掉。
 */
const owned = new Set<string>();

/** PTY 开出来、前端确实持有了它 —— 登记。 */
export function claimSession(id: string): void {
  owned.add(id);
}

/** 标签关了 / 对面进程退了 —— 注销，让后端知道这个会话已经没人要了。 */
export function releaseSession(id: string): void {
  owned.delete(id);
}

/** 心跳要上报的「我还在用的会话」快照。 */
export function ownedSessions(): string[] {
  return Array.from(owned);
}
