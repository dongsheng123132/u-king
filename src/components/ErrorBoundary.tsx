/**
 * 全局错误边界 —— 把「白/黑屏 + 无任何信息」变成「可见的错误框 + 自动上报」。
 *
 * 背景（Mac 客户实锤「黑屏、没法用、退出重开还黑」）：整个前端**原本没有任何 ErrorBoundary/
 * window.onerror**，只要有一个组件渲染时抛错，React 会卸载整棵树 → 深色主题下就是一片「黑屏」，
 * 且不提示、不上报，完全没法定位。装上它后：
 *  - React 渲染崩溃被兜住 → 显示可读的错误 + 堆栈 + 「重新加载」，而不是黑屏；
 *  - 自动 report_bug（kind=ui_crash）→ 现场黑屏第一次就把真实堆栈送回来；
 *  - 同时是「JS 崩溃 vs 原生 WKWebView 黑屏」的决定性测试：装上后仍纯黑 = 排除 JS，转窗口/合成方向。
 *
 * 刻意用**内联样式**（不依赖 Tailwind/globals.css）—— 万一样式层本身也坏了，兜底界面仍要能显示。
 * 刻意**不依赖 i18n**（context 可能正是崩溃源）—— 直接中英双语静态文案。
 */
import { Component, type ErrorInfo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

type Props = { children: ReactNode };
type State = { error: Error | null; info: string };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, info: "" };
  private reported = false;

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const stack = `${error?.stack || error?.message || String(error)}\n--- component stack ---${info?.componentStack || ""}`;
    this.setState({ info: stack });
    if (!this.reported) {
      this.reported = true;
      try {
        void invoke("report_bug", {
          kind: "ui_crash",
          summary: `UI 渲染崩溃: ${(error?.message || String(error)).slice(0, 140)}`,
          detail: `UA=${navigator.userAgent}\n${stack}`,
        });
      } catch {
        /* 上报失败不致命，本地仍显示错误 */
      }
    }
  }

  render() {
    const { error, info } = this.state;
    if (!error) return this.props.children;

    const details = `${error.message || String(error)}\n\n${info}`.trim();
    return (
      <div
        style={{
          position: "fixed",
          inset: 0,
          background: "#0b0e16",
          color: "#e6e8ef",
          font: "13px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif",
          padding: "40px 32px",
          overflow: "auto",
          zIndex: 999999,
        }}
      >
        <div style={{ maxWidth: 720, margin: "0 auto" }}>
          <div style={{ fontSize: 18, fontWeight: 700, marginBottom: 8 }}>
            U-King 遇到问题，界面已停止 · Something went wrong
          </div>
          <div style={{ color: "#9aa0b4", marginBottom: 20 }}>
            已自动上报，你可以直接「重新加载」恢复。若反复出现，请把下方信息发给我们。
            <br />
            Auto-reported. Click “Reload” to recover; if it keeps happening, send us the details below.
          </div>
          <div style={{ display: "flex", gap: 10, marginBottom: 18 }}>
            <button
              onClick={() => location.reload()}
              style={{
                background: "#3b82f6",
                color: "#fff",
                border: "none",
                borderRadius: 8,
                padding: "8px 16px",
                cursor: "pointer",
                fontWeight: 600,
              }}
            >
              重新加载 · Reload
            </button>
            <button
              onClick={() => {
                try {
                  void navigator.clipboard.writeText(details);
                } catch {
                  /* ignore */
                }
              }}
              style={{
                background: "transparent",
                color: "#e6e8ef",
                border: "1px solid #2a2f3e",
                borderRadius: 8,
                padding: "8px 16px",
                cursor: "pointer",
              }}
            >
              复制诊断信息 · Copy details
            </button>
          </div>
          <pre
            style={{
              background: "#0f131d",
              border: "1px solid #1e2430",
              borderRadius: 8,
              padding: 14,
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              color: "#c3c8d8",
              maxHeight: "50vh",
              overflow: "auto",
            }}
          >
            {details}
          </pre>
        </div>
      </div>
    );
  }
}
