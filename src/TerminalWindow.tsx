/**
 * 拉出去的独立终端窗口 —— 一个只装终端的 webview 窗口。
 *
 * 客户 2026-08-18：「终端里边的一个终端页面能拉出我们的 U-King 界面不，就是放到外面来，
 * 做对比之类的」。用途很实：一边看 AI 在工作台里干活，一边在外面盯终端输出/自己敲命令，
 * 不用来回切标签。
 *
 * 🔴 **只渲染终端，不渲染整个 App**（入口在 `main.tsx` 按 `?pane=terminal` 分流）。
 * 走 App 那条路的话，这个小窗里会跟着起一整套侧栏/路由/心跳/事件监听 ——
 * 两份 App 同时活着，会话列表、定时任务、升级横幅都会各跑一遍，
 * 那正是「多开 U-King = 定时任务 N 倍烧 token」那条踩过的坑（只不过这次在同一个进程里）。
 *
 * PTY 归属：`useTermGroup` 建的会话由**这个 webview** 认领并心跳（`ownedSessions`），
 * 关掉窗口 = 心跳停 = Rust 侧按老化收尸。所以关窗不会留下孤儿 PTY。
 */
import { useEffect, useState } from "react";
import { TerminalPage } from "./TerminalPage";

export function TerminalWindow() {
  const [cmd, setCmd] = useState<string | null>(null);

  useEffect(() => {
    // 起手命令由开窗方经 URL 带进来（如「在这个目录开个终端并跑 claude」）。
    // 只取一次：跑完就清，避免刷新窗口又跑一遍。
    const q = new URLSearchParams(location.search);
    const c = q.get("cmd");
    if (c) setCmd(c);
    const title = q.get("cwd");
    if (title) document.title = `${title} · U-CLI`;
  }, []);

  return (
    <div className="h-screen w-screen bg-bg-0 overflow-hidden">
      <TerminalPage active pendingCmd={cmd} onConsumedCmd={() => setCmd(null)} />
    </div>
  );
}
