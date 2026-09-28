/** 「AI 工具中心」(`src/toolhub/`) 专用英文词条 —— 只放这个模块独有的新词，
 *  跟别处共用的（刷新/启动/一键安装/已安装/未安装/使用中/内置/加载中…等）复用中央字典已有条目，
 *  不在这里重复一遍（同一句话两处各翻一次容易漂）。 */
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
  "+ 自定义供应商": "+ Custom provider",
  "修改模型配置": "Apply model change",
  "在 U-CLI 终端中打开": "Open in U-CLI terminal",
  "正在应用配置…": "Applying config…",
  "已切到 {name}，正在启动 {tool}…": "Switched to {name}, launching {tool}…",
  "已为 {name} 打开系统终端": "Opened a system terminal for {name}",
  "打开系统终端失败（{msg}），已改用 U-CLI 终端窗口": "Failed to open a system terminal ({msg}); fell back to the U-CLI terminal window",
  "让 AI 帮我修": "Ask AI to fix it",
  "「{tool}」切换模型驱动失败，帮我看看是怎么回事：\n{err}":
    "Switching the model driver for \"{tool}\" failed. Help me figure out why: \n{err}",
};
