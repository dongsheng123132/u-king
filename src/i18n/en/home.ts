/**
 * 英文覆盖字典 · 「我的 AI」首页 + 软件库（2026-10-22 B 极简改版）。
 *
 * 中文即 key；与 EN 里其它模块同名 key 的翻译必须保持同一个意思（后展布覆盖前展布，
 * 这里只放本版块新出现的文案 + 分类/记忆标签这类**变量传参**的 key——后者闸门扫不到）。
 */
export const home: Record<string, string> = {
  // ── 首页状态 / 动作块 ─────────────────────────────
  "都在这儿，点开就能用。": "All here — click one to use it.",
  "先把要用的 AI 装上。": "Install the AI tools you need first.",
  "已装 {n} 个 · 可装 {m} 个": "{n} installed · {m} available",
  "每轮底座约 {n} tokens": "about {n} tokens per turn",
  "体检一下": "Run a checkup",
  "看看哪块要修、哪个能升级": "See what needs fixing or upgrading",
  "软件库": "Software Library",
  "再装一个，分类找": "Install another, browse by category",
  "省额度": "Save tokens",
  "看看每轮都花在哪": "See where each turn's tokens go",
  "换模型 · 余额": "Model · Balance",
  "AI 设置里配": "Configure in AI Settings",
  "去软件库挑一个装上，或者让 AI 帮你按这台机器的配置推荐。":
    "Pick one in the Software Library, or let AI recommend based on this machine.",
  "去软件库挑": "Browse the library",
  "已装（{n}）": "Installed ({n})",
  "全部体检 / 升级 →": "Check up / upgrade all →",
  "桌面版": "Desktop app",
  "正常": "OK",
  "推荐装": "Recommended",
  "全部软件 →": "All software →",
  "让 AI 装": "Let AI install",
  "升级 / 修复（重走安装）": "Upgrade / repair (reinstall)",
  "让 AI 修": "Let AI fix it",
  "可以按原来的目录和命令把它们开回来（原来的屏幕内容不会回来）。":
    "Reopen them with the same folders and commands (the old screen contents will not come back).",

  // ── 每轮底座账单 ─────────────────────────────────
  "每轮底座：Claude Code 本体约 {n} tokens；技能 {s} 个、记忆 {m}":
    "Per-turn baseline: Claude Code itself ~{n} tokens; {s} skills; memory {m}",
  "每轮底座账单（读取中…）": "Per-turn token bill (loading…)",
  "Claude Code 本体（系统提示 + 工具定义）": "Claude Code itself (system prompt + tool definitions)",
  "约 {n} tokens": "~{n} tokens",
  "技能名字与简介（每轮都要读一遍）": "Skill names and descriptions (re-read every turn)",
  "{n} 个 · 说明合计 {c}字": "{n} skills · {c} chars of docs",
  "全局记忆文件（每轮都原样带上）": "Global memory files (re-sent every turn)",
  "这是「输入」里的固定开销：聊得越久，历史还会另外叠加。":
    "This is the fixed cost inside the input; the longer the chat, the more history adds on top.",
  "省法：不用的技能包卸掉；聊完一个话题就开新对话；全局记忆只留每次都必须在场的内容。":
    "Ways to save: remove skills you don't use, start a new chat when a topic is done, and keep only must-have content in global memory.",
  "去管技能包": "Manage skill packs",

  // ── 软件库 ───────────────────────────────────────
  "共 {n} 个软件 · 已装 {m} 个": "{n} apps · {m} installed",
  "搜软件名或用途，比如 magpie / 模型路由": "Search by name or purpose, e.g. magpie / model router",
  "找到 {n} 个": "{n} found",
  "把安装任务交给轻助手，由 AI 来装": "Hand the install task to the assistant and let AI install it",
  "这个条件下没有软件；换个词或点「全部」。": "No app matches. Try another keyword or tap All.",
  "工具目录暂时是空的。": "The app catalog is empty right now.",
  "装不上的可以先点「让 AI 装」：它会把这台机器的现状交给 U-King 轻助手，由 AI 接着装（不会自动发送，你按发送才走）。":
    "If an install fails, tap “Let AI install”: it hands this machine's current state to the U-King assistant (it will not send automatically — you press send).",

  // ── 2026-10-22 A 方案（软件库并回首页、已装图标墙）────
  "都在这儿，点一下就开始用。": "All here — click one to start using it.",
  "装软件": "Install apps",
  "分类挑一个，装不上让 AI 装": "Browse by category; hand failed installs to AI",
  "点一下启动": "Click to launch",
  "管理 {name}": "Manage {name}",
  "去「可安装」挑一个：分类挑、点安装；装不上就点「让 AI 装」。":
    "Pick one under Available: browse, then Install; if it fails, hand it to AI.",
  "去挑一个": "Browse apps",
  "这是「输入」里的固定开销：聊得越久，历史还会另外叠加（历史已压到 2.4 万字符）。":
    "This is the fixed cost inside the input; longer chats add more history (history is now capped at 24k chars).",

  // ── 动态 key（t(变量)，闸门扫不到，手写在这里）────
  "Claude 全局记忆": "Claude global memory",
  "Codex 全局记忆": "Codex global memory",
  "U-King 说明书": "U-King docs",
  "智能体": "Agents",
  "桌面应用": "Desktop apps",
  "命令行": "CLI tools",
  "分类找 · 一键装 · 让 AI 装": "Browse · one-click install · let AI install",
  "已装 · 体检 · 省额度": "Installed · checkup · save tokens",
};
