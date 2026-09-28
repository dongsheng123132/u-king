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
  "装机 · 体检 →": "Install · Checkup →",
  "已安装（{n}）": "Installed ({n})",
  "可安装（{n}）": "Available ({n})",
  "使用工具自带账号": "Uses the tool's own account",
  "这里暂不能换它的模型": "Can't switch its model here yet",
  "还没装 AI 工具": "No AI tools installed yet",
  "一键装好推荐组合": "One-click install the recommended set",
  "或在下面挑一个装": "Or pick one to install below",
  "命令行工具打开方式：": "Open CLI tools in:",
  // 复审 high 修复（LAB_TOOLS 不再计入「已安装」主网格）：已装的实验室工具单收一条紧凑分区。
  "实验室（已装）": "Lab (installed)",
};
