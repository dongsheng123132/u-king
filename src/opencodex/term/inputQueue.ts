/**
 * xterm 输入 → Tauri `term_write` 的单会话 FIFO。
 *
 * xterm 的 `onData` 是同步事件，但 Tauri `invoke` 返回 Promise。若每次按键都直接 fire-and-forget，
 * 后端的 mpsc 只能保证「到达后端以后」的顺序，管不到仍在 WebView IPC 里的并发请求；PTY 还没
 * 建好时更会被 `if (sessionId)` 直接丢掉。这里把两段空窗都收口：没连接先缓存，连接后一次只飞
 * 一个 invoke。失败不重试（响应丢失时重试会双发），而是停住并明确报错。
 */

export type TermInputQueue = {
  /** 追加输入；未连接 PTY 时先缓存。 */
  push: (data: string) => void;
  /** 绑定一个已创建的 PTY，并开始排空缓存。新 session 会解除上一次失败状态。 */
  connect: (sessionId: string) => void;
  /** PTY 已退出；丢掉尚未确认的输入，避免重开后灌进新 shell。 */
  disconnect: () => void;
  /** term_open 失败等场景下清空启动期间积下来的输入。 */
  clear: () => void;
  /** 前端终端永久关闭。 */
  close: () => void;
  /** 只给回归取证，不参与业务判断。 */
  pendingChars: () => number;
};

export function createTermInputQueue(opts: {
  write: (sessionId: string, data: string) => Promise<unknown>;
  onError: (error: unknown) => void;
  maxBufferedChars?: number;
}): TermInputQueue {
  const chunks: string[] = [];
  const maxBufferedChars = opts.maxBufferedChars ?? 64 * 1024;
  let bufferedChars = 0;
  let sessionId: string | null = null;
  let generation = 0;
  let draining = false;
  let failed = false;
  let closed = false;

  const clear = () => {
    chunks.length = 0;
    bufferedChars = 0;
  };

  const drain = () => {
    if (draining || closed || failed || !sessionId || chunks.length === 0) return;
    const sid = sessionId;
    const gen = generation;
    draining = true;
    void (async () => {
      while (!closed && !failed && sessionId === sid && generation === gen && chunks.length > 0) {
        const data = chunks.shift();
        if (data == null) break;
        bufferedChars -= data.length;
        try {
          await opts.write(sid, data);
        } catch (error) {
          // 会话已换代时，旧 invoke 的失败不该污染新会话；也不重放刚才那块，避免「其实写成了，
          // 只是回包丢了」时制造双字。
          if (closed || sessionId !== sid || generation !== gen) break;
          failed = true;
          clear();
          opts.onError(error);
          break;
        }
      }
    })().finally(() => {
      draining = false;
      // drain 期间来的按键已排在 chunks 后面；上一轮正常结束后继续。
      if (!closed && !failed && sessionId && chunks.length > 0) drain();
    });
  };

  const push = (data: string) => {
    if (!data || closed || failed) return;
    if (bufferedChars + data.length > maxBufferedChars) {
      failed = true;
      clear();
      opts.onError(new Error(`终端输入缓存超过 ${maxBufferedChars} 字符`));
      return;
    }
    // 快速打字时把还没起飞的小块合并，既保持字节顺序，也少做 IPC；已经在飞的那块不碰。
    const last = chunks.length - 1;
    if (last >= 0 && chunks[last].length + data.length <= 4096) chunks[last] += data;
    else chunks.push(data);
    bufferedChars += data.length;
    drain();
  };

  return {
    push,
    connect(nextSessionId) {
      if (closed) return;
      sessionId = nextSessionId;
      generation += 1;
      failed = false;
      drain();
    },
    disconnect() {
      sessionId = null;
      generation += 1;
      failed = false;
      clear();
    },
    clear,
    close() {
      closed = true;
      sessionId = null;
      generation += 1;
      clear();
    },
    pendingChars: () => bufferedChars,
  };
}
