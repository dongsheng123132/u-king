/** 「我的 AI」(`src/toolhub/`，原「AI 工具中心」) 专用英文词条 —— 只放这个模块独有的新词，
 *  跟别处共用的（刷新/启动/安装/已安装/未安装/使用中/内置/加载中…等）复用中央字典已有条目，
 *  不在这里重复一遍（同一句话两处各翻一次容易漂）。
 *  🔴 2026-09-29 首页改版：去掉右侧选模型面板 + 底部操作条后，以下几条词条已不再被引用，
 *  留着不删（不影响任何检查、也是这个模块曾经的形态记录）：
 *  「AI 工具中心」「选工具 · 选模型 · 一键启动」「为此选择模型：{tool}」「（未选择工具）」
 *  「先在左边选一个工具」「这个工具不支持在这里切换模型」「修改模型配置」「在 U-CLI 终端中打开」。
 *  🔴 2026-09-29 复审修复：target 为空且拿不到 currentModelFor 时，原文案「使用工具自带账号」
 *  对 pi 这类后端其实能配模型、只是前端还没接上的工具不属实，改用「这里暂不能换它的模型」
 *  +「AI 设置」出口（后者复用中央字典已有的「AI 设置」键，这里不重复翻）。
 *  「使用工具自带账号」这一条从此不再被引用，同上一条一起留着不删。 */
export const toolhub: Record<string, string> = {
  // 「正在启动…」= 工具中心启动按钮的行内状态（无工具名）；下面那条带 {tool} 的是终端里的
  // 「正在启动 mimo…」。两条文案不同、都要翻 —— 只加带 {tool} 的那条会让英文界面露出中文。
  "正在启动…": "Starting…",
  "正在启动 {tool}…": "Starting {tool}…",
  "启动仍在进行；如果一直没有输出，可点上方重开。": "Still starting. If no output appears, use Restart above.",
  "桌面版模型配置说明": "Desktop model configuration guide",
  "配置与登录说明": "Configuration and sign-in guide",
  "多选安装": "Install multiple",
  "取消多选": "Cancel selection",
  "安装所选（{n}）": "Install selected ({n})",
  "安装所选软件：{list}": "Install selected software: {list}",
  "所选软件安装流程已结束。请在「我的 AI」中启动；支持的工具可换模型或充值，需要账号的工具在首次启动时登录。": "Installation finished. Launch tools from My AI; supported tools offer model switching and recharge, while account-based tools require sign-in on first launch.",
  "官方登录": "Official sign-in",
  "自备 Key / 官方登录": "Your own key / official sign-in",
  "在 U-King 内置终端打开": "Open in U-King's terminal",
  "切换后 Claude Code 将使用 {name} 的模型和计费。可在「AI 设置」还原官方登录。": "Claude Code will use {name}'s models and billing. You can restore official sign-in in AI Settings.",
  "开始安装 Claude 桌面版（官方安装包，装完检测能否启动）…": "Installing Claude Desktop from the official installer and verifying the app…",

  "选择要在哪个文件夹里打开 {name}": "Choose a folder for {name}",
  "在哪个文件夹里打开 {name}？": "Where should {name} open?",
  "{name} 会在这个文件夹里读取、新建和修改文件。": "{name} will read, create, and edit files in this folder.",
  "正在读取最近用过的文件夹…": "Loading recent folders…",
  "还没有用过的文件夹，先选一个吧。": "Choose your first working folder.",
  "最近用过的文件夹": "Recent folders",
  "选择文件夹…": "Choose a folder…",
  "选择其它文件夹…": "Choose another folder…",
  "在这里打开": "Open here",
  "向下回看较新输出": "Scroll down to newer output",

  "AI 工具中心": "AI Tool Hub",
  "选工具 · 选模型 · 一键启动": "Pick a tool, pick a model, launch",
  "命令行": "CLI",
  "桌面应用": "Desktop app",
  "智能体": "Agent",
  "这个分类下暂时没有工具": "No tools in this category yet",
  "为此选择模型：{tool}": "Choose a model for: {tool}",
  "（未选择工具）": "(no tool selected)",
  "先在左边选一个工具": "Pick a tool on the left first",
  "这个工具不支持在这里切换模型": "This tool doesn't support switching models here",
  "余额/充值": "Balance / Top up",
  "自定义供应商": "Custom provider",
  "更多模型设置": "More model settings",
  "修改模型配置": "Apply model change",
  "在 U-CLI 终端中打开": "Open in U-CLI terminal",
  "正在应用配置…": "Applying config…",
  "已切到 {name}，正在启动 {tool}…": "Switched to {name}, launching {tool}…",
  "已为 {name} 打开系统终端": "Opened a system terminal for {name}",
  "打开系统终端失败（{msg}），已改用 U-CLI 终端窗口": "Failed to open a system terminal ({msg}); fell back to the U-CLI terminal window",
  // 「让 AI 帮我修」按钮文案 + 「已把故障交给 AI…」提示已经在 `en/settings.ts`
  // 翻过一次（`Manager.tsx` 同名按钮先用的这两句）——同一句话不在这重复翻第二遍，
  // 避免两份译文各改各的、悄悄漂开（宪法第 8 条）。

  // 2026-09-29 首页改版新增（已装/可装两段网格 + 每卡自带换模型下拉）
  "已装 {n} 个工具": "{n} tool(s) installed",
  // 2026-10-03 收敛 2b：页头链接「装机 · 体检 →」改名「体检 · 升级 →」（myai 降为本页子页）
  "体检 · 升级 →": "Checkup · Upgrade →",
  "已安装（{n}）": "Installed ({n})",
  "可安装（{n}）": "Available ({n})",
  "使用工具自带账号": "Uses the tool's own account",
  "这里暂不能换它的模型": "Can't switch its model here yet",
  "还没装 AI 工具": "No AI tools installed yet",
  "一键装好推荐组合": "One-click install the recommended set",
  "或在下面挑一个装": "Or pick one to install below",
  "命令行工具打开方式：": "Open CLI tools in:",
  // 复审 high 修复（LAB_TOOLS 不再计入「已安装」主网格）：已装的实验室工具单收一条紧凑分区。

  // 2026-09-29 三次改版（Launchpad 式 logo 墙 + 行内展开详情条）新增：
  // 分体按钮「启动」旁 ▾ 弹出的两项，以及详情条里描述当前打开方式的同一行小字——
  // 跟已经存在的「系统终端」「在 U-CLI 终端中打开」措辞不同，是这次新写的完整短句，
  // 不能复用旧 key（宪法第 8 条：同一句话只认一份翻译，但这不是同一句话）。
  "在系统终端打开": "Open in system terminal",
  "在 U-CLI 打开": "Open in U-CLI",
  // 复审 medium 修复：`route_tab` 类工具（如 hermes）跳的是 U-King 里的专属页，不是系统终端，
  // 也不听 `launchPref`——不能沿用上面两条 `embedded_pty` 专属的措辞（同一句话只认一份翻译，
  // 但这不是同一句话，宪法第 8 条）。
  "在 U-King 里打开": "Open inside U-King",
  // 详情条次要操作「体检修复」→ App.tsx 的 `setTab("myai")`（装机漏斗页）。
  "体检修复": "Checkup & repair",
  // 详情条底部「想在 U-King 里用？」+「在工作台对话」「在工作台开终端」两个链接引导。
  // 2026-10-03 收敛方案 2a：侧栏「对话工作台」「终端工作台」合成一个「工作台」，
  // 这两个链接改成动词短语，落点仍是同一个工作台的对话态 / 终端态。
  "想在 U-King 里用？": "Want to use it inside U-King?",
  "在工作台对话": "Chat in the Workbench",
  "在工作台开终端": "Open a terminal in the Workbench",

  // 换模型下拉按钮在「读不到当前模型」时的两种文案（pi/opencode 这类回读失败/被挡住；
  // 「读取中…」复用中央字典已有条目）。不说「还没配模型」——读不到不等于没配。
  "使用中：{name}": "In use: {name}",
  "选择模型供应商": "Choose a model provider",

  // 无障碍修复新增：详情条 `role="region"` 的 `aria-label`，读屏器用来播报这块区域是谁的详情，
  // 跟瓷砖 `aria-controls` 指向的是同一个元素（详情条 id 由 `detailIdFor()` 统一拼）。
  "{name} 详情": "{name} details",

  // 2026-10-03 收敛方案 §3.2：「可安装」区分三组。「推荐」复用中央字典已有条目
  // （tools.ts / onboarding.ts），这里只补两个带数量的折叠组标题。
  "更多 AI 工具（{n}）": "More AI tools ({n})",
  "日常软件（{n}）": "Everyday software ({n})",
};
