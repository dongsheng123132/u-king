// ⚠️ 必须排在所有 import 之前：垫片要在任何懒加载 chunk（pdf.js 等）跑起来之前装好。
// 老 WebView2（客户机常年停在 Chrome 120）缺 Promise.try / withResolvers，见 issue #291。
import "./lib/polyfills";
import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { App } from "./App";
import { TerminalWindow } from "./TerminalWindow";
import { I18nProvider } from "./i18n";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ownedSessions } from "./opencodex/term/registry";
import "./globals.css";

// 全局兜底：ErrorBoundary 只兜 React 渲染期的错；异步/事件回调/Promise 里抛的错它管不到。
// 这里把未捕获的 error + unhandledrejection 也静默上报（去重，best-effort），让现场黑屏/卡死
// 第一次就把真实原因送回来。绝不打扰用户、绝不因上报失败再抛。
const reportedErrors = new Set<string>();
function reportGlobal(kind: string, summary: string, detail: string) {
  const dedupe = `${kind}:${summary}`.slice(0, 200);
  if (reportedErrors.has(dedupe)) return;
  reportedErrors.add(dedupe);
  try {
    void invoke("report_bug", { kind, summary: summary.slice(0, 150), detail: detail.slice(0, 4000) });
  } catch {
    /* ignore */
  }
}
window.addEventListener("error", (e) => {
  const msg = e?.error?.stack || e?.message || String(e);
  reportGlobal("ui_error", `未捕获错误: ${(e?.message || "").slice(0, 120)}`, `UA=${navigator.userAgent}\n${msg}`);
});
window.addEventListener("unhandledrejection", (e) => {
  const r: unknown = e?.reason;
  const msg = (r as Error)?.stack || String(r);
  reportGlobal("ui_rejection", `未处理的 Promise: ${String((r as Error)?.message ?? r).slice(0, 120)}`, `UA=${navigator.userAgent}\n${msg}`);
});

/**
 * 关掉 WebView2 的默认网页右键菜单（客户反馈 2026-08-10，v0.9.94）。
 *
 * 客户在终端里右键，弹出来的是 **Edge 网页菜单**：返回 / 刷新 / 另存为 / 打印 /
 * **发送标签页到你的设备**。这不是终端一处的问题 —— 全应用**任何地方**右键都是这个菜单，
 * 只是终端里最刺眼。它要么无意义（另存为？打印？），要么有害（「刷新」会重载整个 webview），
 * 而且一眼就告诉客户「这不是个应用，是个网页」。
 *
 * 🔴 **不是一刀切**：可编辑区域（输入框 / textarea / contenteditable）和**选中了文字**时
 * 放行，因为那时候浏览器菜单里的复制/粘贴/全选是**真有用**的，而我们并没有在每个输入框上
 * 都做一个自己的菜单。收掉有害的，留下有用的 —— 别为了干净把客户的复制粘贴一起干掉。
 *
 * 终端不走这条：它在 `useTermGroup` 里**无条件**拦掉并弹自己的菜单（那儿有选区也不能放行，
 * 否则又回到 Edge 菜单）。这里用 `defaultPrevented` 判断，终端已经处理过的就不重复插手。
 */
window.addEventListener(
  "contextmenu",
  (e) => {
    if (e.defaultPrevented) return; // 终端等自带菜单的地方已经接管了
    const t = e.target as HTMLElement | null;
    // 🔴 终端整块不走下面「输入框放行」那条：xterm 在光标位置放着一个隐藏的
    //    `.xterm-helper-textarea`，右键正好落在光标上时 `closest("textarea")` 会命中它，
    //    于是又把 Edge 菜单放了进来 —— 而且只在特定落点复现，最难查的那种。
    //
    //    说实话：**今天这条够不到** —— `useTermGroup` 的监听挂在终端**容器**上，
    //    终端里任何位置的右键都会先冒泡到它并被拦掉。留着是因为哪天有人把那个监听
    //    收窄到某个子元素，漏出来的正好是我们刚修的这个客户 bug，而代价是两行。
    //    （跑道里用一个**合成的** `.xterm > textarea` 单独验这条，不然它就是没人验的防御。）
    if (t?.closest?.(".xterm")) {
      if (!e.defaultPrevented) e.preventDefault();
      return;
    }
    const editable =
      !!t?.closest?.("input, textarea, [contenteditable=''], [contenteditable='true']");
    // 🔴 判的是「**右键这个地方**有没有选中文字」，不是「页面上任何地方有没有选区」。
    //    只看 `getSelection().toString()` 的话，别处一段没清掉的旧选区会让**整个应用**
    //    的右键又变回 Edge 菜单 —— 而且时灵时不灵，最难查的那种。
    const sel = window.getSelection();
    const selectedHere = !!sel && !sel.isCollapsed && !!t && sel.containsNode(t, true);
    if (editable || selectedHere) return; // 这两种情况下系统菜单是真有用的
    e.preventDefault();
  },
  // 冒泡阶段：先让组件自己的 onContextMenu 跑（它们会 preventDefault），我们只兜没人管的
  false,
);

// 终端会话保活心跳：webview 活着就每 20s 报一次平安（term.rs term_ping）。
// 没有它，WebView2 渲染进程一崩，Rust 侧的 watchdog 就收不到「前端还活着」的信号，
// 长驻 PTY 会话会一路累积到把应用拖死（2026-08-06 实机 35 个泄漏会话的教训）。
// 心跳断了 = 前端没了 = Rust 侧 3 分钟内自动收尸。窗口缩托盘被节流到 ~1 次/分钟也安全。
//
// ★ 必须带上 alive（本前端还在用哪些会话 id）。只报「我还活着」不够：webview 刷新/崩溃
// 重建之后，上一轮遗留的孤儿会话没有任何人认领，却会被这条全局心跳一起刷活、永远回收不掉。
// 带上归属后，孤儿自然老化到 HEARTBEAT_TIMEOUT 被收走 —— 后端那条按「会话总数」乱杀的
// MAX_SESSIONS 兜底也就不需要了（它会误杀用户正开着的终端，见 term.rs::reap_stale）。
setInterval(() => {
  void invoke("term_ping", { alive: ownedSessions() }).catch(() => {});
}, 20_000);

/**
 * 🔴 **拉出去的终端窗口只渲染终端，不渲染 App**。
 *
 * 同一个进程里开第二个 webview 是可以的（单实例锁挡的是第二个**进程**），
 * 但两份 `<App/>` 同时活着 = 两套侧栏/路由/定时任务调度/升级检查各跑一遍 ——
 * 那就是「多开 U-King = 定时任务 N 倍烧 token」那条踩过的坑搬进同一个进程。
 * 所以在**入口**就分流，别让第二个 App 有机会挂载。
 */
const isTerminalWindow = new URLSearchParams(location.search).get("pane") === "terminal";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <I18nProvider>{isTerminalWindow ? <TerminalWindow /> : <App />}</I18nProvider>
    </ErrorBoundary>
  </React.StrictMode>,
);
