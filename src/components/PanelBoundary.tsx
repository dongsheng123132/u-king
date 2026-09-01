/**
 * 面板级错误边界 —— 把「一个面板炸 = 整个产品白掉」压成「一个面板炸 = 那一块显示错误卡」。
 *
 * 为什么根上那个 ErrorBoundary 不够（2026-08-16 盘点，三条最痛的 bug 是同一个形状）：
 *  - #402/#403 `_isDisposed`：切大脑 → 卸 TermPanel → xterm dispose 抛错 → 冒到**根**边界
 *    → 整棵树被卸掉 → 客户报「U-King 自己重启了」。进程一直活着，被换掉的是整个界面。
 *  - 0.9.99/0.9.100 Mac 白屏：两条正则 lookbehind 在主包顶层求值 → 整个前端起不来。
 *  - 共同点不是「模块太多」，是**一个叶子模块的故障，半径是整个产品**。
 *
 * CLAUDE.md 的「模块独立四铁律」守的是编译期耦合（删一个模块只动 2 个文件），
 * 一个字都没守运行时耦合。**删得干净 ≠ 炸不连坐。** 这个文件补的是后一半。
 *
 * 根因要不要修？要，而且优先修（#403 的根因已由 50d0dc6 收掉）。但根因是逐个修的，
 * 半径是一次性压的 —— 下一个还没出现的抛错，不该再享受「拆掉整个界面」这个待遇。
 *
 * 与根 ErrorBoundary 的分工：
 *  - 这里兜得住 → 侧栏/标签/其他面板全部继续可用，用户能切走、能继续干活、能自己重试；
 *  - 这里兜不住的（渲染树更上层、主包顶层求值）→ 照旧落到根边界的全屏兜底页。
 *
 * 刻意用**内联样式**（不依赖 Tailwind/globals.css）、刻意**不依赖 i18n**
 * —— 理由同根边界：样式层/context 本身可能正是崩溃源。
 *
 * ★ 上报带 `name`：#403 出来时「无人认领」，掉进「多半是没装好/驱动没配对」那句写死的猜测里。
 *   现在标题直接写明是哪个面板，症状名和代码位置之间少隔一层。
 */
import { Component, type ErrorInfo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

type Props = {
  /** 面板标识，进上报标题（如 `U-CLI` / `chat` / `draw`）。用稳定代号，别用会翻译的文案。 */
  name: string;
  /**
   * 兜底卡的形态。
   *  - `panel`（默认）：占满高度的居中大卡，给主内容区用。
   *  - `chrome`：一条窄横条，给**常驻框架件**用（标题栏 / 侧栏 / 状态条）。
   *
   * 🔴 为什么必须分两种：这三件常驻组件原本一个都没包边界（2026-08-19 查出），
   * 它们一抛错就直接冒到根边界 = 整屏被换掉，正是客户说的「U-King 自己重启了」。
   * 但如果给侧栏套上那张占满高度的大卡，208px 宽的栏里塞一张 560px 的卡片，
   * 结果是「没白屏，但也没法用」—— **兜底本身不能是第二种坏掉**。
   */
  variant?: "panel" | "chrome";
  children: ReactNode;
};
type State = { error: Error | null; info: string };

export class PanelBoundary extends Component<Props, State> {
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
          kind: "ui_panel_crash",
          summary: `面板崩溃[${this.props.name}]: ${(error?.message || String(error)).slice(0, 120)}`,
          detail: `panel=${this.props.name}\nUA=${navigator.userAgent}\n${stack}`,
        });
      } catch {
        /* 上报失败不致命，本地仍显示错误卡 */
      }
    }
  }

  /** 重试 = 重新挂载子树。比 location.reload() 温和得多：其他面板的 PTY / 会话全部不受影响。 */
  private retry = () => {
    this.reported = false;
    this.setState({ error: null, info: "" });
  };

  render() {
    const { error, info } = this.state;
    if (!error) return this.props.children;

    const details = `panel=${this.props.name}\n${error.message || String(error)}\n\n${info}`.trim();

    // 常驻框架件（标题栏/侧栏/状态条）：只占一条，别把它旁边的内容顶掉。
    // 关键是**留一个「重试」**：侧栏挂了等于导航没了，没有重试就只能重开 App。
    if (this.props.variant === "chrome") {
      return (
        <div
          style={{
            padding: "8px 10px",
            margin: 6,
            borderRadius: 8,
            background: "rgba(220,38,38,0.08)",
            border: "1px solid rgba(220,38,38,0.28)",
            font: "12px/1.5 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif",
            color: "inherit",
            display: "flex",
            alignItems: "center",
            gap: 8,
            flexWrap: "wrap",
            minWidth: 0,
          }}
        >
          <span style={{ opacity: 0.85, minWidth: 0, wordBreak: "break-word" }}>
            「{this.props.name}」出错了，已自动上报
          </span>
          <button
            onClick={this.retry}
            style={{
              background: "#3b82f6",
              color: "#fff",
              border: "none",
              borderRadius: 6,
              padding: "3px 10px",
              cursor: "pointer",
              fontWeight: 600,
              font: "inherit",
              flexShrink: 0,
            }}
          >
            重试
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
              color: "inherit",
              border: "1px solid rgba(127,127,127,0.34)",
              borderRadius: 6,
              padding: "3px 10px",
              cursor: "pointer",
              font: "inherit",
              flexShrink: 0,
            }}
          >
            复制诊断
          </button>
        </div>
      );
    }

    return (
      <div
        style={{
          height: "100%",
          minHeight: 180,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: 24,
          overflow: "auto",
          font: "13px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif",
        }}
      >
        <div
          style={{
            maxWidth: 560,
            width: "100%",
            background: "rgba(127,127,127,0.06)",
            border: "1px solid rgba(127,127,127,0.24)",
            borderRadius: 12,
            padding: 20,
            color: "inherit",
          }}
        >
          <div style={{ fontSize: 15, fontWeight: 700, marginBottom: 6 }}>
            这一块出问题了，其余功能仍可用
          </div>
          <div style={{ opacity: 0.7, marginBottom: 16 }}>
            出问题的是「{this.props.name}」，已自动上报。左侧其他页面照常使用；点「重试」可以只重开这一块，
            不影响正在跑的终端和会话。
            <br />
            This panel crashed and was auto-reported. The rest of the app keeps working.
          </div>
          <div style={{ display: "flex", gap: 10, marginBottom: 14, flexWrap: "wrap" }}>
            <button
              onClick={this.retry}
              style={{
                background: "#3b82f6",
                color: "#fff",
                border: "none",
                borderRadius: 8,
                padding: "7px 15px",
                cursor: "pointer",
                fontWeight: 600,
                font: "inherit",
              }}
            >
              重试 · Retry
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
                color: "inherit",
                border: "1px solid rgba(127,127,127,0.34)",
                borderRadius: 8,
                padding: "7px 15px",
                cursor: "pointer",
                font: "inherit",
              }}
            >
              复制诊断信息 · Copy details
            </button>
          </div>
          <pre
            style={{
              background: "rgba(127,127,127,0.10)",
              border: "1px solid rgba(127,127,127,0.18)",
              borderRadius: 8,
              padding: 12,
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              opacity: 0.85,
              maxHeight: "32vh",
              overflow: "auto",
              margin: 0,
              fontSize: 12,
            }}
          >
            {details}
          </pre>
        </div>
      </div>
    );
  }
}
