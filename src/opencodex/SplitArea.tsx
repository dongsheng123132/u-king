/**
 * OpenCodex 两区布局的「中对话」区 —— Codex 一比一。
 *
 * Codex 桌面版是「左任务列表 + 一个大对话面」两区：终端/文件/浏览器不是常驻右栏，
 * 而是对话顶部一排按钮，点了才从右侧滑出（slide-over）覆盖在对话上，收起回到全宽对话。
 *
 * ChatColumn：全宽主对话（ChatPanel 独占）+ 顶栏（任务名 + 切模型 + 终端/文件/浏览器开关）。
 * SidePanel：右侧滑出层，承载终端/文件/浏览器，绝对定位浮在对话右半边，可拖宽、可关。
 * 都靠父级 display 切换保活（切会话不杀 PTY/历史）。
 */
import { useCallback, useRef, useState } from "react";
import { Cpu, FolderTree, Globe, SquareTerminal, Terminal as TermIcon, X } from "lucide-react";
import type { RightKind, Task } from "./types";
import { useWorkbench } from "./store";
import { SplitContainer } from "./term/SplitContainer";
import { ChatPanel } from "./panels/ChatPanel";
import { FilesPanel } from "./panels/FilesPanel";
import { BrowserPanel } from "./panels/BrowserPanel";
import { ProviderSwitch } from "../components/ProviderSwitch";
import { PanelBoundary } from "../components/PanelBoundary";
import { useI18n } from "../i18n";

/**
 * `lab: true` = **实验室档**：功能在、入口在，但明着标「测试中」。
 *
 * 浏览器为什么降级（2026-08-09 用户真机实测点出来的）：后端全通 ——
 * `browser.open/stream/snapshot` 逐条验过，直播流 5 秒推 16 条消息 ——
 * 但**面板在真窗口里没能用起来**，而在此之前这一半从来没有人点过
 * （记忆原话：「GUI 面板 WebSocket 渲染仍需 RDP 手点」）。
 *
 * 命中的是实验室标准里那条「**依赖外部环境成功率不稳**」：agent-browser 要从
 * googlechromelabs.github.io 下浏览器内核，国内裸网间歇被重置，daemon 还会僵死。
 * 不是「不好用就藏起来」—— 是**不能让用户以为这是条稳的路**，
 * 点进去一片空白会让人怀疑整个产品，而不是怀疑这一个面板。
 */
const RIGHT_META: { kind: RightKind; label: string; icon: typeof TermIcon; lab?: boolean }[] = [
  { kind: "terminal", label: "终端", icon: TermIcon },
  { kind: "files", label: "文件", icon: FolderTree },
  { kind: "browser", label: "浏览器", icon: Globe, lab: true },
];

/**
 * 主区：全宽对话 + 顶栏开关 + 右侧滑出面板。
 * 一个组件搞定两区里的「右区」全部职责（取代旧 ChatColumn + RightColumn + AuxBar 三块）。
 */
export function ChatColumn({
  task,
  active,
  onToast,
  onGoManage,
}: {
  task: Task;
  active: boolean;
  onToast: (s: string) => void;
  onGoManage: () => void;
}) {
  const { t } = useI18n();
  const { state, setRight, toggleRight, setRatio } = useWorkbench();
  const layout = state.panels[task.id];
  const rightKind = layout?.rightKind ?? "terminal";
  const rightOpen = layout?.rightOpen ?? false;
  const ratio = layout?.rightRatio ?? 0.5;

  const [modelOpen, setModelOpen] = useState(false);
  const rowRef = useRef<HTMLDivElement>(null);

  // 滑出面板宽度拖动（从分隔条往左拖加宽面板）
  const onDragRatio = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const el = rowRef.current;
      if (!el) return;
      const move = (ev: MouseEvent) => {
        const rect = el.getBoundingClientRect();
        // ratio = 对话占比；面板在右，鼠标越往左面板越宽
        setRatio(task.id, (ev.clientX - rect.left) / rect.width);
      };
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    },
    [task.id, setRatio],
  );

  // 点顶栏按钮：已开且同类 → 关；否则切到该类并展开
  const onPick = (k: RightKind) => {
    if (rightOpen && rightKind === k) toggleRight(task.id, false);
    else setRight(task.id, k);
  };

  return (
    <div className="flex flex-col h-full min-h-0 min-w-0">
      {/* 顶栏：任务名 + 目录 + 切模型 + 终端/文件/浏览器开关 */}
      <div className="flex items-center h-10 px-3 border-b border-white/[0.06] bg-bg-1 shrink-0 gap-2">
        <span className="text-[13px] font-medium text-ink-0 truncate">{task.name}</span>
        {task.tool && (
          <span className="text-[10px] px-1.5 py-0.5 rounded bg-accent/[0.14] text-accent-400 shrink-0">
            {task.tool}
          </span>
        )}
        <span className="text-ink-5 text-[11px] truncate max-w-[30%] font-mono hidden lg:inline" title={task.dir}>
          {task.dir}
        </span>
        <div className="flex-1" />

        {/* 切模型 */}
        <div className="relative">
          <button
            onClick={() => setModelOpen((o) => !o)}
            title={t("切换模型")}
            className={
              "inline-flex items-center gap-1.5 h-7 px-2.5 rounded text-[12px] transition-colors " +
              (modelOpen ? "bg-accent/[0.16] text-accent" : "text-ink-3 hover:bg-white/[0.05] hover:text-ink-1")
            }
          >
            <Cpu size={13} />
            <span className="hidden md:inline">{t("模型")}</span>
          </button>
          {modelOpen && (
            <div className="absolute right-0 top-9 z-30 w-60 rounded-card border border-white/[0.10] bg-bg-2 shadow-card p-2">
              <ProviderSwitch
                targets={["claude", "codex"]}
                onToast={onToast}
                onGoManage={() => {
                  onGoManage();
                  setModelOpen(false);
                }}
                onSwitched={() => setModelOpen(false)}
                compact
              />
            </div>
          )}
        </div>

        {/* 终端/文件/浏览器开关 —— 点了才滑出 */}
        <div className="flex items-center gap-0.5 ml-1 pl-2 border-l border-white/[0.08]">
          {RIGHT_META.map((p) => {
            const on = rightOpen && rightKind === p.kind;
            const Icon = p.icon;
            return (
              <button
                key={p.kind}
                onClick={() => onPick(p.kind)}
                title={p.lab ? t("{label}（测试中：依赖 agent-browser，国内网络下常起不来）", { label: t(p.label) }) : t(p.label)}
                className={
                  "inline-flex items-center gap-1.5 h-7 px-2.5 rounded text-[12px] transition-colors " +
                  (on ? "bg-accent/[0.12] text-ink-0" : "text-ink-3 hover:bg-white/[0.05] hover:text-ink-1")
                }
              >
                <Icon size={13} className={on ? "text-accent" : ""} />
                <span className="hidden md:inline">{t(p.label)}</span>
                {/* 「测试中」徽章 —— 跟侧栏实验室区同一套语义：功能在，但别当它是稳的。
                    不标的话，用户点进去一片空白会怀疑整个产品，而不是怀疑这一个面板。 */}
                {p.lab && (
                  <span className="hidden lg:inline text-[9px] leading-none px-1 py-0.5 rounded bg-amber-400/15 text-amber-400/90">
                    {t("测试中")}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* 对话 + 右侧滑出面板（同一行；面板用 flex-basis 控宽，关闭则对话独占全宽） */}
      <div ref={rowRef} className="flex flex-1 min-h-0 min-w-0">
        <div className="min-w-0 min-h-0" style={{ flexBasis: rightOpen ? `${ratio * 100}%` : "100%" }}>
          {/* U-Chat 炸了不该带走侧栏和终端 —— 边界挂在这里而不是 ChatPanel 内部，
              是因为**边界必须比它守的东西活得久**：卸载期抛的错只会冒给仍挂载着的上层边界。 */}
          <PanelBoundary name="U-Chat">
            <ChatPanel taskId={task.id} cwd={task.dir} active={active} onGoManage={onGoManage} />
          </PanelBoundary>
        </div>
        {rightOpen && (
          <>
            <div
              onMouseDown={onDragRatio}
              className="w-1 shrink-0 bg-white/[0.06] hover:bg-accent/60 cursor-col-resize transition-colors"
            />
            <div className="min-w-0 min-h-0" style={{ flexBasis: `${(1 - ratio) * 100}%` }}>
              <SidePanel task={task} active={active} kind={rightKind} onClose={() => toggleRight(task.id, false)} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** 右侧滑出面板内容：终端/文件/浏览器（终端常驻保活，文件/浏览器按需）。 */
function SidePanel({
  task,
  active,
  kind,
  onClose,
}: {
  task: Task;
  active: boolean;
  kind: RightKind;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const { addSession } = useWorkbench();
  const label = kind === "terminal" ? "终端" : kind === "files" ? "文件" : "浏览器";

  return (
    <div className="flex flex-col h-full min-h-0 min-w-0 border-l border-white/[0.06] bg-bg-2">
      <div className="flex items-center gap-2 h-9 px-3 border-b border-white/[0.06] bg-bg-1 shrink-0">
        <span className="text-[12px] font-medium text-ink-1">{t(label)}</span>
        <div className="flex-1" />
        {kind === "terminal" && (
          <button
            onClick={() => addSession(task.dir, "openclaw", "OpenClaw CLI", "openclaw")}
            title={t("在新终端打开 OpenClaw CLI")}
            className="flex items-center gap-1 h-7 px-2 rounded text-[11px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.04]"
          >
            <SquareTerminal size={13} />
            OpenClaw CLI
          </button>
        )}
        <button
          onClick={onClose}
          title={t("收起")}
          className="inline-flex items-center justify-center w-7 h-7 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
        >
          <X size={15} />
        </button>
      </div>
      <div className="flex-1 min-h-0 relative">
        {/* 终端常驻保活（display 切换）；文件/浏览器按需挂载 */}
        <div className="absolute inset-0" style={{ display: kind === "terminal" ? "block" : "none" }}>
          {/* ★ #402/#403 的现场：切大脑 → 卸 TermPanel → xterm dispose 抛错。根因已由 50d0dc6 收掉
              （dispose 逐步吞异常），这层管的是**下一个还没出现的抛错** —— 别再让它拆掉整个界面。
              边界挂在 SplitContainer 外、常驻 div 内：里面的 TermPanel 来去，它一直在，接得住卸载期的错。 */}
          <PanelBoundary name="U-CLI">
            <SplitContainer
              cwd={task.dir}
              active={active && kind === "terminal"}
              tool={task.tool ?? undefined}
              initialCmd={task.startup_cmd ?? undefined}
            />
          </PanelBoundary>
        </div>
        {/* 文件/浏览器：面板仍按需挂载（不改保活语义），但**定位容器和边界常驻** ——
            边界要比它守的东西活得久才接得住卸载期的错；容器常驻则保证兜底卡片落在
            `absolute inset-0` 里，出错时不会挤歪旁边那个终端层。 */}
        <div className="absolute inset-0" style={{ display: kind === "files" ? "block" : "none" }}>
          <PanelBoundary name="files">
            {kind === "files" && <FilesPanel root={task.dir} active={active} />}
          </PanelBoundary>
        </div>
        <div className="absolute inset-0" style={{ display: kind === "browser" ? "block" : "none" }}>
          <PanelBoundary name="browser">
            {kind === "browser" && <BrowserPanel taskId={task.id} />}
          </PanelBoundary>
        </div>
      </div>
    </div>
  );
}
