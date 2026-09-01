/**
 * 终端新手引导卡 —— 只给生手小白看，点一下就在终端里跟 AI 干活。
 *
 * 设计约束（最小改动、不破坏终端）：
 *  - 正常流插在终端容器上方，终端 flex-1 自动让位，零侵入布局；
 *  - 右上角 × /「知道了」永久关闭（localStorage: uking.termGuide=1），
 *    之后靠终端页标签栏的小灯泡按钮重新展开；
 *  - 点击按钮 = runInActive 直接执行（走 term_write 写 PTY stdin，不经 term_open
 *    白名单，中文提示词不受限）；没装 claude/hermes 时终端会报 command not found，
 *    卡片底部有「去首页·我的AI 一键装」的引导文案兜底。
 *  - 词库对齐 QuickPrompts 的场景词（写周报/总结会议/做表格/写代码/找 Bug/翻译文档），
 *    只取最常用的 6 条，不超纲（能力边界跟着终端里真有的工具走）。
 */
import { Bot, Bug, Code2, FileText, Languages, ListChecks, Sparkles, Table, X } from "lucide-react";

type Props = {
  onRun: (cmd: string) => void;
  onClose: () => void;
};

const QUICK: { label: string; icon: typeof FileText; prompt: string }[] = [
  { label: "写周报", icon: FileText, prompt: "帮我写这周的周报，我的工作是：" },
  { label: "总结会议", icon: ListChecks, prompt: "把这段会议记录总结成要点：" },
  { label: "做表格", icon: Table, prompt: "帮我做一个表格，统计这些数据：" },
  { label: "写代码", icon: Code2, prompt: "帮我写一段代码，功能是：" },
  { label: "找 Bug", icon: Bug, prompt: "这段代码跑不通，帮我找找原因：" },
  { label: "翻译文档", icon: Languages, prompt: "把这份文档翻译成中文：" },
];

export function TermGuide({ onRun, onClose }: Props) {
  return (
    <div className="relative shrink-0 border-b border-white/[0.06] bg-gradient-to-b from-accent/[0.07] to-transparent px-4 pt-2.5 pb-2.5">
      <button
        onClick={onClose}
        className="absolute right-3 top-2.5 inline-flex items-center justify-center w-6 h-6 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
        title="关闭引导（以后可在标签栏小灯泡重新打开）"
      >
        <X size={13} />
      </button>

      <div className="flex items-center gap-2 mb-2 pr-8">
        <Sparkles size={13} className="text-accent shrink-0" />
        <span className="text-[12.5px] font-semibold text-ink-0">新手看这里：不用记命令，点一下就能跟 AI 干活</span>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {/* 大按钮：引导客户打开 claude / hermes —— 这是本卡的主使命 */}
        <button
          onClick={() => onRun("claude")}
          className="inline-flex items-center gap-1.5 rounded-lg px-3.5 py-2 bg-accent/15 hover:bg-accent/25 text-accent text-[13px] font-semibold transition-colors"
          title="在当前终端直接启动 Claude Code"
        >
          <Bot size={15} /> 跟 Claude 聊天
        </button>
        <button
          onClick={() => onRun("hermes")}
          className="inline-flex items-center gap-1.5 rounded-lg px-3.5 py-2 bg-accent/[0.07] hover:bg-accent/[0.16] text-ink-1 text-[13px] font-semibold transition-colors"
          title="在当前终端直接启动 Hermes"
        >
          <Sparkles size={15} /> 跟 Hermes 聊天
        </button>

        <span className="mx-1 h-4 w-px bg-white/[0.08] shrink-0" aria-hidden />

        {/* 快捷词：点一下直接跑 claude "提示词"（claude 未装时终端会提示 command not found） */}
        {QUICK.map((q) => (
          <button
            key={q.label}
            onClick={() => onRun(`claude "${q.prompt}"`)}
            className="inline-flex items-center gap-1.5 h-8 px-2.5 rounded-lg border border-white/[0.08] bg-white/[0.03] hover:bg-white/[0.08] text-ink-2 text-[12px] transition-colors"
            title={`跟 Claude 说：${q.prompt}…`}
          >
            <q.icon size={12} className="text-ink-3" /> {q.label}
          </button>
        ))}
      </div>

      <div className="mt-1.5 text-[11px] text-ink-4">
        没装 Claude？去「首页 · 我的 AI」一键装 · 点右上角 × 永久关闭本引导（以后可在标签栏小灯泡重新打开）
      </div>
    </div>
  );
}
