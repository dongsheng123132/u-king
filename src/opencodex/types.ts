/** 工作台数据类型 —— 与 Rust src-tauri/src/tasks.rs 的 Task 字段一一对应。 */

/**
 * 对话大脑。定义放在 types 而不是 `Chat.tsx`，是为了让 `QuickPrompts` 也能用它标 `best`
 * ——起手词是 Chat 的**子组件**，反过来 import Chat 会成环。
 * uking=自家虾盘云直连（轻快·作图）；claude/codex=驱动真身 CLI（结构化卡片 + 技能包）；
 * claude-cli/hermes=在 U-CLI 里开真身 TUI（我们一个字都不翻译，你看到的就是它自己的界面）。
 *
 * 🔴 `claude` 和 `claude-cli` **是同一个 Claude Code 的两种壳，不是两个工具**：
 * 前者我们用 `-p --output-format stream-json` 驱动它、把结果渲染成卡片（好读、能预览产物）；
 * 后者直接把它的 TUI 摆出来（原味、有 `/` 指令和计划模式，老手要的是这个）。
 * 以前只有 Hermes 有「直接进终端」这条路，于是「想用原味 Claude Code」的人在工作台里无路可走。
 */
export type Engine = "uking" | "claude" | "codex" | "claude-cli" | "hermes";

/** 走 TUI 那条路的大脑 → 进终端要敲的命令。**不能拿 engine id 当命令**（`claude-cli` 不是命令名）。 */
export const ENGINE_TUI_CMD: Partial<Record<Engine, string>> = {
  "claude-cli": "claude",
  hermes: "hermes",
  // 🔴 **DSH 不进这个下拉**（2026-08-18 当天先加后撤）。
  // 先加是因为客户说「agent 选择下拉里边要有 dsh」；同一轮他又说清楚了
  // 「dsh 就不做终端形态的啊，目前体验也不好，就是 dsh web」。
  // 这两句不矛盾 —— 他要的是**够得着 DSH**，不是「在终端里开 DSH」。
  // 而这个下拉的语义是「这一轮用哪个大脑跑」，塞一个「点了会弹浏览器」的条目进去，
  // 是把**启动器**混进**选择器**：选完这一轮并不会用它跑。
  // DSH 的入口在「首页 · 我的 AI」（已前移到主推之后），点开就是它的 Web 工作台。
};

export type PanelKind = "terminal" | "files" | "browser" | "chat";
export type TaskStatus = "idle" | "running" | "waiting_input" | "done" | "error";
export type TaskSource = "manual" | "context_menu" | "im";

/** 右侧区可显示的面板（中间 ChatPanel 永远独占，不在此集合）。 */
export type RightKind = "terminal" | "browser" | "files";

/**
 * U-Workspace 右侧主区当前显示什么。
 * `chat` = 会话（默认，也是从别处回来的落点）；其余是左栏点进来的功能面板。
 * `kanban` = 任务看板（会话生命周期总览 + 定时任务条）—— 答「**谁在跑**」。
 * `passports` = 任务护照（长程任务状态 + 跨 AI 交接）—— 答「**事情做到哪**」。
 *
 * 🔴 后两个是**两个一等入口，不是一个页里的两块**。会话是「做一件事然后结束」的
 * 生命周期，护照是「跨会话、跨 AI、跨天地活着」的生命周期。以前护照挤在看板页眉上
 * 那条 230px 的横条里，目标 / 已验证事实 / 下一步一个字都露不出来，只剩一串 id ——
 * 摆在别人家的页眉上，它就永远只能是装饰。
 *
 * 切到面板时**不卸载任何会话**（Chat 实例照旧 display:none 保活，PTY 不断）。
 */
export type WorkView = "chat" | "experts" | "automation" | "kanban" | "passports" | "arena";

/** 每个会话的三区布局（运行时内存，不落盘）。中=对话恒在；右=可切+收起；下=终端抽屉。 */
export interface PanelLayout {
  rightKind: RightKind; // 右侧区当前显示
  rightOpen: boolean; // 右侧区展开/收起（收起则中对话独占全宽）
  rightRatio: number; // 中:右 宽度比 0.35~0.85
  drawerOpen: boolean; // 底部终端抽屉
  drawerHeight: number; // 抽屉高度 px
}

export interface Task {
  id: string; // 任务唯一 id（前端按文件夹生成）
  name: string; // 显示名（默认文件夹名）
  dir: string; // 绑定文件夹绝对路径
  status: TaskStatus; // 气泡染色 + IM 查询
  source: TaskSource; // 来源
  assignee: string | null; // IM 预留：指派给谁
  external_ref: string | null; // IM 预留：外部消息/会话 id
  last_opened_at: number;
  created_at: number;
  // —— Phase 7：工具型会话（从「我的 AI」运行面板启动的工具实例）——
  tool?: string | null; // claude/codex/openclaw/hermes…；任务型为空
  startup_cmd?: string | null; // 启动命令（如 "openclaw gateway run"）
  kind?: "task" | "tool"; // 默认 task
  // —— Phase 8：项目分组键（=规范化 dir）。同一文件夹可开多个会话，左侧按 project 分组 ——
  project?: string | null;
  // —— worktree：此任务是 git worktree 时设置。worktree_repo = 主仓库路径；与主任务同一 project 分组 ——
  worktree_repo?: string | null;
  worktree_branch?: string | null;
  // —— AI 专家：此会话由某专家「召唤」而来，绑定其 id（persona/引擎/技能由 experts.ts 恢复）——
  expert?: string | null;
}

/** 规范化目录路径（去尾斜杠 + 小写），作项目分组键。 */
export function normDir(dir: string): string {
  return (dir ?? "").replace(/[\\/]+$/, "").toLowerCase();
}

export interface ChatMsg {
  role: "user" | "assistant" | "system";
  content: string;
}

/** 任务名取文件夹名（路径末段）。 */
export function dirBasename(dir: string): string {
  const parts = (dir ?? "").replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || dir;
}

/** 用文件夹路径生成稳定 id（同一文件夹 → 同一任务，避免重复）。 */
export function taskIdFromDir(dir: string): string {
  const norm = (dir ?? "").replace(/[\\/]+$/, "").toLowerCase();
  let h = 0;
  for (let i = 0; i < norm.length; i++) {
    h = (h * 31 + norm.charCodeAt(i)) | 0;
  }
  return "t" + (h >>> 0).toString(36);
}
