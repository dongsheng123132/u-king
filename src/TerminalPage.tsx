/**
 * 独立终端页 —— 右侧主区一个整页，内部多标签多开（取代旧底部抽屉）。
 *
 * 与 TUI 应用页同级：侧栏点「终端」→ 这页在右侧主区展开，内部可 + 开很多终端、标签切换。
 * 复用 useTermGroup（与抽屉/工作台同一引擎），PATH 已注入便携工具，openclaw/hermes/claude/codex
 * 可直接跑。常驻保活由父级 App.tsx 的 display 切换负责（切走不杀 PTY）。
 */
import { useState } from "react";
import { useTermGroup, getTermThemeId, setGlobalTermTheme, TERM_THEMES, type TermRestore, type TermThemeId } from "./opencodex/term/useTermGroup";
import { Check, Lightbulb, Palette, Plus, RotateCw, X } from "lucide-react";
import { TermGuide } from "./components/TermGuide";
import { useI18n } from "./i18n";
import "@xterm/xterm/css/xterm.css";

const THEME_LABELS: Record<TermThemeId, string> = { dark: "暗黑（默认）", light: "浅色护眼", green: "复古绿" };

export function TerminalPage({
  active,
  pendingCmd,
  onConsumedCmd,
  pendingRestores,
  onRestoreFailed,
  onConsumedRestores,
}: {
  active: boolean;
  /** 待运行命令（点工具「打开终端」时塞进来），运行后回调清空 */
  pendingCmd: string | null;
  onConsumedCmd: () => void;
  /** 自升级后的会话快照：逐条打开成独立终端标签。 */
  pendingRestores?: TermRestore[] | null;
  onRestoreFailed?: (failed: TermRestore[]) => void;
  onConsumedRestores?: () => void;
}) {
  const { t: tr } = useI18n();
  const { hostRef, tabs, activeKey, setActiveKey, newTerm, closeTerm, restartTerm, runInActive, dropOver } = useTermGroup({
    open: active,
    pendingCmd,
    onConsumedCmd,
    pendingRestores,
    onRestoreFailed,
    onConsumedRestores,
  });

  // 新手引导卡：默认显示（只给小白），× 永久关闭，之后靠小灯泡重新展开
  const [guideHidden, setGuideHidden] = useState(() => {
    try {
      return localStorage.getItem("uking.termGuide") === "1";
    } catch {
      return false;
    }
  });
  const dismissGuide = () => {
    setGuideHidden(true);
    try {
      localStorage.setItem("uking.termGuide", "1");
    } catch {
      /* ignore */
    }
  };

  // 配色：三套预设，选完全局即时生效（所有终端页/工作台/应用页 xterm 一起换肤）
  const [themeId, setThemeId] = useState<TermThemeId>(() => getTermThemeId());
  const [themeOpen, setThemeOpen] = useState(false);
  const pickTheme = (id: TermThemeId) => {
    setGlobalTermTheme(id);
    setThemeId(id);
    setThemeOpen(false);
  };

  return (
    <div className="flex flex-col h-full min-h-0 rounded-card border border-white/[0.08] overflow-hidden bg-bg-2">
      {/* 标签栏 */}
      <div className="flex items-center h-9 px-2 gap-1 border-b border-white/[0.06] bg-bg-1 shrink-0">
        <div className="flex items-center gap-1 flex-1 min-w-0 overflow-x-auto">
          {tabs.map((t) => (
            <div
              key={t.key}
              onClick={() => setActiveKey(t.key)}
              className={
                "group flex items-center gap-1.5 h-7 pl-2.5 pr-1.5 rounded cursor-pointer text-[12px] shrink-0 " +
                (t.key === activeKey ? "bg-accent/[0.12] text-ink-0" : "text-ink-3 hover:bg-white/[0.04]")
              }
            >
              {/* 绿点曾经是写死的 —— 进程退了标签照样"在线"，敲什么都没反应也没提示 */}
              <span className={"dot " + (t.dead ? "dot-off" : "dot-on")} />
              <span className={"whitespace-nowrap" + (t.dead ? " text-ink-4 opacity-60" : "")}>{t.title}</span>
              {t.dead && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    restartTerm(t.key);
                  }}
                  className="inline-flex items-center justify-center h-4 px-1 rounded text-[10px] text-accent-400 hover:bg-accent/[0.14]"
                  title={tr("进程已退出，点这里重开")}
                >
                  <RotateCw size={10} />
                </button>
              )}
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  closeTerm(t.key);
                }}
                className="opacity-0 group-hover:opacity-100 inline-flex items-center justify-center w-4 h-4 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
                title={tr("关闭此终端")}
              >
                <X size={11} />
              </button>
            </div>
          ))}
          <button
            onClick={() => newTerm()}
            className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-3 hover:text-ink-1 hover:bg-white/[0.06] shrink-0"
            title={tr("新建终端")}
          >
            <Plus size={14} />
          </button>
        </div>
        <span className="text-ink-4 text-[10.5px] hidden md:inline shrink-0 mr-1">
          {tr("已注入工具路径，openclaw / hermes / claude / codex 可直接运行")}
        </span>
        {/* 新手引导开关：关闭后可在这里重新展开 */}
        {guideHidden && (
          <button
            onClick={() => setGuideHidden(false)}
            className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-3 hover:text-ink-1 hover:bg-white/[0.06] shrink-0"
            title={tr("重新显示新手引导")}
          >
            <Lightbulb size={13} />
          </button>
        )}
        {/* 配色切换：三套预设，选完全局即时生效 */}
        <div className="relative shrink-0">
          <button
            onClick={() => setThemeOpen((o) => !o)}
            className={"inline-flex items-center justify-center w-6 h-6 rounded shrink-0 " + (themeOpen ? "bg-accent/[0.14] text-accent" : "text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]")}
            title={tr("终端配色")}
          >
            <Palette size={13} />
          </button>
          {themeOpen && (
            <>
              <div className="fixed inset-0 z-30" onClick={() => setThemeOpen(false)} />
              <div className="absolute right-0 top-7 z-40 w-44 rounded-lg border border-white/[0.08] bg-bg-1 shadow-xl p-1.5 space-y-0.5">
                {(Object.keys(TERM_THEMES) as TermThemeId[]).map((id) => (
                  <button
                    key={id}
                    onClick={() => pickTheme(id)}
                    className="w-full flex items-center gap-2 px-2 py-1.5 rounded text-[12px] text-ink-2 hover:bg-white/[0.06]"
                  >
                    <span
                      className="w-3.5 h-3.5 rounded-full border border-white/20 shrink-0"
                      style={{ background: TERM_THEMES[id].background }}
                    />
                    <span className="flex-1 text-left">{THEME_LABELS[id]}</span>
                    {themeId === id && <Check size={12} className="text-accent" />}
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      </div>
      {/* 新手引导卡：默认显示（只给小白），× 永久关闭；不破坏终端，终端自动让位 */}
      {!guideHidden && <TermGuide onRun={runInActive} onClose={dismissGuide} />}
      {/* 终端宿主（各终端容器绝对定位叠在这里，靠 display 切换）*/}
      <div ref={hostRef} className="relative flex-1 min-h-0">
        {dropOver && (
          <div className="absolute inset-0 z-30 border-2 border-dashed border-accent/60 bg-accent/[0.06] grid place-items-center pointer-events-none">
            <div className="text-accent text-[13px] font-semibold">{tr("松手把文件路径贴进终端")}</div>
          </div>
        )}
      </div>
    </div>
  );
}
