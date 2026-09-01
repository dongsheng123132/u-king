/**
 * 「开这个会话时，往它的终端里贴这条命令」的**信箱**。
 *
 * ## 为什么需要它
 * 用户在任务看板上点一张 Claude Code 的卡片，想要的是**接着那个活干**。
 * 而「接着干」在这台机器上有一条确定的写法：`claude --resume <session_id>`。
 * 以前点卡片只做到「用它的工作目录开个会话」—— 目录对了，上文一个字没有，
 * 等于把用户送到正确的房间然后让他从头讲一遍。用户的原话是
 * 「点击任务，不能替我输入 claude resume」。
 *
 * ## 为什么是模块级信箱，不是 props
 * 跟 `handoff.ts` 同一个形状、同一个理由：**投递发生时收件人还没挂载**
 * （会话是刚 `addTask` 出来的，React 下一帧才渲染 `Chat`）。
 * 把一次性的值顺着 `UWorkspace → Chat → TermPanel` 串成 state，
 * 等于为一次投递维护一份长期状态。按 sessionId 存一封信、收件人挂载时自取，
 * 形状跟这件事本身一样。
 *
 * ## 🔴 只贴，不回车
 * 命令贴进终端后**不替用户按回车**，跟 `pasteToTerminal` 的既有手感一致。
 * `--resume` 会真的起一个 AI 会话、真的花钱、真的可能改文件 —— 那是「写」，
 * 得由人按下最后那一下（同影核「写动作必须 --yes」的纪律：确认权在人手里，
 * 换个入口不等于换掉这条规矩）。
 */

/** 收件箱：sessionId → 待贴的那条命令。取走即删（一次性）。 */
const inbox = new Map<string, string>();

/** 投一条命令给某个会话。会话挂载后自取。 */
export function queueTermCmd(sessionId: string, cmd: string) {
  const c = cmd.trim();
  if (!c) return; // 空命令不投递 —— 投了会在终端里贴个空行，看起来像「点了没反应」
  inbox.set(sessionId, c);
}

/**
 * 收件人自取。**取走即删** —— 否则会话每次重挂载（切视图、改窗口大小触发重渲染）
 * 都会把同一条 `--resume` 再贴一遍，终端里堆出一排命令。
 */
export function takeTermCmd(sessionId: string): string | null {
  const c = inbox.get(sessionId);
  if (!c) return null;
  inbox.delete(sessionId);
  return c;
}
