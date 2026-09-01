/**
 * Windows WebView2 + xterm DOM 渲染器的残字校正。
 *
 * 已知症状：Codex/Hermes 这类 TUI 高频局部重画后，屏幕上会留下缺笔、叠字或空洞；
 * PTY 缓冲区内容本身是对的，拖一下终端宽度触发全屏重画就立刻恢复。这里不碰 PTY、
 * 不伪造 resize，只在一批输出解析完后要求 xterm 用自己的缓冲区完整刷新一次。
 *
 * quietMs 收掉一阵输出里的重复刷新；maxWaitMs 保证持续流式输出时也会周期性自愈。
 */

export type PaintRepair = {
  /** 一块输出已被 xterm 解析；安排一次合并后的全屏校正。 */
  afterWrite: () => void;
  /** 终端销毁时取消计时器，不能再碰已 dispose 的 xterm。 */
  close: () => void;
};

// 这段代码只跑在 WebView 里，计时器 ID 就是浏览器的 number。
// 用 ReturnType<typeof setTimeout> 会被 @types/node 解成 NodeJS.Timeout，
// 而 macOS 构建时 DOM/Node 重载又会让默认 setTimeout 返回 number | Timeout。
type TimerId = number;

export function createPaintRepair(opts: {
  refresh: () => void;
  quietMs?: number;
  maxWaitMs?: number;
  setTimer?: (fn: () => void, ms: number) => TimerId;
  clearTimer?: (id: TimerId) => void;
}): PaintRepair {
  const quietMs = opts.quietMs ?? 80;
  const maxWaitMs = opts.maxWaitMs ?? 400;
  const setTimer = opts.setTimer ?? ((fn: () => void, ms: number) => globalThis.setTimeout(fn, ms) as unknown as number);
  const clearTimer = opts.clearTimer ?? ((id: number) => globalThis.clearTimeout(id));
  let quietTimer: TimerId | null = null;
  let maxTimer: TimerId | null = null;
  let closed = false;

  const cancel = () => {
    if (quietTimer != null) clearTimer(quietTimer);
    if (maxTimer != null) clearTimer(maxTimer);
    quietTimer = null;
    maxTimer = null;
  };

  const repair = () => {
    if (closed) return;
    cancel();
    try {
      opts.refresh();
    } catch {
      // 终端可能正好在 React 卸载；校正失败不能把整棵界面带走。
    }
  };

  return {
    afterWrite() {
      if (closed) return;
      if (quietTimer != null) clearTimer(quietTimer);
      quietTimer = setTimer(repair, quietMs);
      if (maxTimer == null) maxTimer = setTimer(repair, maxWaitMs);
    },
    close() {
      if (closed) return;
      closed = true;
      cancel();
    },
  };
}
