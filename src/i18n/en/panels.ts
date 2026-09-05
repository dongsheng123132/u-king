/** 英文覆盖字典 · 工作台面板 + 终端页（opencodex/panels/*, term/*, TerminalPage）。 */
export const panels: Record<string, string> = {
  "回看较早输出（也可在终端内滚动鼠标滚轮）": "View earlier output (you can also scroll the mouse wheel in the terminal)",
  "回看较早输出": "View earlier output",
  "回到最新输出": "Jump to latest output",
  // ChatPanel.tsx —— 任务对话面板
  "{agent} 退出码 {code}": "{agent} exit code {code}",
  "⚠️ 对话失败：{msg}\n（多半是 {agent} 没接驱动 / 没装。点上方「去配驱动」一键修。）":
    "⚠️ Chat failed: {msg}\n(Likely {agent} has no driver configured or isn't installed. Click “Set up” above to fix it in one click.)",
  "\n[启动 {agent} 失败: {err}]": "\n[Failed to launch {agent}: {err}]",
  "还没装 Claude Code —— 这里的对话靠它驱动。先去装一下。":
    "Claude Code isn't installed yet — chat here runs on it. Install it first.",
  "去装机": "Set up",
  "和 {name} 对话": "Chat with {name}",
  "在 ": "Working in ",
  " 里干活。工具调用、文件改动会以卡片和内联 diff 展示。":
    ". Tool calls and file changes are shown as cards and inline diffs.",
  "{name} 正在干活…": "{name} is working…",
  "给 {name} 发指令（@ 引用文件，/ 调指令 · Enter 发送）":
    "Message {name} (@ for files, / for commands · Enter to send)",
  // 工具名人话化（对话里每一轮都出现，是最该翻的一批）
  "运行命令": "Run command",
  "看后台输出": "Check background output",
  "停掉后台命令": "Stop background command",
  "查看文件": "Read file",
  "新建文件": "Create file",
  "修改文件": "Edit file",
  "改笔记本": "Edit notebook",
  "查找文件": "Find files",
  "搜索内容": "Search content",
  "打开网页": "Fetch page",
  "上网搜索": "Web search",
  "派了个小助手": "Delegated to a sub-agent",
  "列执行计划": "Made a plan",
  "确认方案": "Confirm plan",
  "执行指令": "Run command",
  "调用外部工具": "Call external tool",

  // 报错人话化（认得出才给准话；认不出的走下面那条「我没认出」）
  "⚠️ {what}\n\n**怎么办**：{how}": "⚠️ {what}\n\n**What to do**: {how}",
  "⚠️ 这一轮没跑成，而且我没认出这是哪种问题。\n\n常见的两个方向：{agent} 没装好，或者驱动没配对 —— 可以点上方「去配驱动」一键修。还不行就用「技术支持」把下面这段发给我们。":
    "⚠️ This turn failed, and I couldn't identify the cause.\n\nTwo common directions: {agent} isn't installed properly, or the provider isn't configured — try “Set up” above. If that doesn't help, send us the block below via Feedback.",
  "账户余额用完了": "Your account balance is used up",
  // 余额还有钱、却一条都发不出去：上游按「最多可能用掉多少」预扣，见底时会被 403 挡回。
  "余额不够发起这一次请求（不是没钱，是不够垫这一次）":
    "Not enough balance to start this request (you have money — just not enough to cover this one)",
  "你的余额还剩 ¥{remain}，而这一次要预留 ¥{need}。发请求前上游要先按「最多可能用掉多少」冻结一笔，所以余额见底时会一条都发不出去。去「虾盘云」页充值，充 ¥1 就能解开；充完这条消息重发一次即可。":
    "You have ¥{remain} left, but this request needs ¥{need} held up front. The provider reserves the maximum a request could possibly cost before it runs, so a nearly-empty balance blocks every request. Top up on the 虾盘云 page — ¥1 is enough to unblock it — then resend this message.",
  "发请求前上游要先按「最多可能用掉多少」冻结一笔，所以余额见底时会一条都发不出去。去「虾盘云」页充值，充 ¥1 就能解开；充完这条消息重发一次即可。":
    "The provider reserves the maximum a request could possibly cost before it runs, so a nearly-empty balance blocks every request. Top up on the 虾盘云 page — ¥1 is enough to unblock it — then resend this message.",
  "去「虾盘云」页充值，充完这条消息重发一次就行。": "Top up on the 虾盘云 page, then resend this message.",
  "密钥没对上": "API key doesn't match",
  "多半是驱动没配好或配到了别家。去「AI 设置」重新一键配虾盘云。":
    "Most likely the provider isn't configured, or points elsewhere. Reconfigure in AI Settings.",
  "请求太频繁，被上游限流了": "Rate-limited upstream — too many requests",
  "等一两分钟再发。反复出现就换个模型试试。": "Wait a minute or two. If it keeps happening, switch models.",
  "连不上服务器": "Can't reach the server",
  "先看看这台电脑能不能上网；开了代理/VPN 的话先关掉再试。":
    "Check this machine's internet access; if a proxy/VPN is on, turn it off and retry.",
  "这个模型现在用不了": "This model isn't available right now",
  "在顶栏「换模型」里换一个（推荐 DeepSeek Flash），或去「AI 设置」重配驱动。":
    "Pick another in the top bar (DeepSeek Flash recommended), or reconfigure the provider in AI Settings.",
  "找不到这个程序": "Program not found",
  "多半是它没装好。去「装 AI」页重装一次，装完会自动验证。":
    "It's probably not installed correctly. Reinstall from the Install AI page — it verifies automatically.",
  "这一轮聊太长了，超出了模型能记住的上限": "This conversation exceeded the model's context limit",
  "点输入框旁边的「清空对话」开个新会话，把关键信息重说一遍。":
    "Use “Clear chat” next to the input box to start fresh, then restate the key points.",

  // 成品卡片（办公产物：右侧渲染不了，客户要的是「打开」）
  // 注意「打开」的翻译在下面「预览独立窗口」那节已有一条，别再加 —— 同名键会让 tsc 直接报 TS1117
  "用电脑上的默认程序打开": "Open with your default app",
  "在文件夹中显示": "Show in folder",
  // 心跳行 + 卡死收尾（后端 status:"timeout"）
  "正在干活 · 已用 {d}": "Working · {d} elapsed",
  "· 已经 {d} 没有新动静，多半卡在一条很慢的命令上。等不及就点右边红色按钮停下。":
    "· No activity for {d} — probably stuck on a slow command. Hit the red button to stop.",
  "⏱️ 这一轮卡住了：整整 {mins} 分钟没有任何动静，已经自动帮你停下（它在后台起的命令也一并收掉了，不会继续占着你的电脑）。\n\n**最常见的原因**：它跑了一个不会自己结束的命令，比如启动一个服务器、或者一直在等你输入什么。\n\n**接下来可以试**：\n1. 把要做的事说得更具体一点，再发一次；\n2. 或者点上面「看命令」把这条命令复制到终端里自己跑一遍 —— 终端里能看到完整过程，也能随时按 Ctrl+C 停。":
    "⏱️ This turn got stuck: no activity at all for {mins} minutes, so it was stopped automatically (background commands it started were cleaned up too — nothing is left running on your machine).\n\n**Most common cause**: it ran a command that never finishes — starting a server, or waiting for input.\n\n**What to try next**:\n1. Describe what you want more specifically and send again;\n2. Or use “Under the hood” above to copy the command into a terminal and run it yourself — you'll see the full output there and can press Ctrl+C anytime.",
  "清空对话（开新会话）": "Clear chat (start a new session)",
  "中断": "Stop",
  "（后台任务，切回查看）": "(Background task — switch back to view)",
  "复制这段": "Copy this",
  "移除这条（报错/无用消息可单独关，不影响其它对话）":
    "Remove this (dismiss error/unwanted messages individually without affecting the rest)",
  "你": "You",
  "工具": "Tool",
  "\n…（已截断）": "\n…(truncated)",

  // ChatPanel.tsx —— 「看命令」条（对话框底下真实跑的 CLI）
  "这一轮对话，底下真实跑的命令": "The command actually running under this turn",
  "底层命令": "Under the hood",
  "① 这一轮真实执行的（一字不差）": "① Actually executed this turn (verbatim)",
  "② 你在终端里可以这么敲（交互式）": "② What you'd type in a terminal (interactive)",
  "复制": "Copy",
  "在终端跑": "Run in terminal",
  "贴进右侧终端（不自动回车，你按回车才真跑）":
    "Paste into the terminal on the right (no auto-Enter — it only runs when you press Enter)",
  "和①不等价：去掉了只为把输出画成卡片而加的参数，改成交互模式（会问你要不要批准）。":
    "Not equivalent to ①: the flags that exist only to render cards are dropped, and it runs interactively (it will ask you to approve).",
  "这次的提示词太长/换行了，没并进命令 —— 敲完回车再把它粘进去。":
    "This prompt was too long or had line breaks, so it isn't inlined — press Enter first, then paste it in.",

  // lib/miniMd.tsx —— AI 回复里代码块的悬停操作条
  "贴到终端": "Paste to terminal",
  "贴进终端（不自动回车，你按回车才真跑）":
    "Paste into the terminal (no auto-Enter — it only runs when you press Enter)",

  // TermPanel.tsx / TerminalPage.tsx —— 终端面板 + 独立终端页（共享 key 自动去重）
  "松手把文件路径贴进终端": "Release to paste the file path into the terminal",
  "关闭此终端": "Close this terminal",
  "新建终端": "New terminal",
  "在终端运行：{cmd}": "Run in terminal: {cmd}",
  "已注入工具路径，openclaw / hermes / claude / codex 可直接运行":
    "Tool paths injected — openclaw / hermes / claude / codex run directly",
  // 自定义快捷词（+ 自定义）
  "自定义": "Custom",
  "添加自定义快捷词": "Add a custom shortcut",
  "在终端右边开/关文件树与预览": "Show/hide the file tree and preview beside the terminal",
  // 顶栏「对话 ↔ 终端」主切换。按钮上是人话，代号只进 tooltip（CLAUDE.md 的命名约定）
  "对话": "Chat",
  "U-Chat（对话）": "U-Chat (conversation)",
  "U-CLI（终端）": "U-CLI (terminal)",
  "{name} 会在这个文件夹里直接动手 —— 说一句你要什么。":
    "{name} works directly inside this folder — just say what you need.",
  "删除此快捷词": "Remove this shortcut",
  "添加快捷词（点按钮即发进终端执行）": "Add a shortcut (click it to run in the terminal)",
  "按钮文字（如 /model）": "Button text (e.g. /model)",
  "发送的命令（留空=同按钮文字）": "Command to send (blank = same as button text)",
  "添加": "Add",

  // FilesPanel.tsx —— 文件面板
  "刷新": "Refresh",
  "双击文件预览（图片 / PDF / Word / Excel / PPT / ZIP / 文本 …）":
    "Double-click a file to preview (image / PDF / Word / Excel / PPT / ZIP / text …)",

  // 预览独立窗口（未找到任何组件引用这批 key —— 核实过与工作台浏览面板无关，
  // 是更早遗留的死键，本次不顺手清，交回主会话判断是否要单独清理）
  "http://localhost:3000 或 https://…": "http://localhost:3000 or https://…",
  "打开": "Open",
  "在独立窗口打开预览页（localhost 也能开）":
    "Open the preview page in a separate window (localhost works too)",

  // SplitContainer.tsx —— 终端分屏
  "左右分屏": "Split left/right",
  "上下分屏": "Split top/bottom",
  "关闭此格": "Close this pane",

  // ChatPanel.tsx —— 空态 + 输入框工具条（2026-08 改版）
  "{name} 在这个文件夹里读写文件、跑命令、改代码 —— 工具调用和文件改动会以卡片和内联 diff 展示。": "{name} reads and writes files, runs commands and edits code in this folder — tool calls and file changes show up as cards with inline diffs.",
  "下面点一个起手词，或直接说你要什么。": "Pick a starter below, or just say what you want.",
  "给 {name} 发指令（@ 引用文件，/ 调指令 · Enter 发送，Shift+Enter 换行）": "Send {name} an instruction (@ to attach a file, / for commands · Enter to send, Shift+Enter for a new line)",
  "这一轮用哪个模型（不选就跟着「虾盘云」里配的走）": "Which model to use for this turn (leave unset to follow your Xiapan Cloud config)",
  "模型：跟随驱动设置": "Model: follow driver config",
  "我们没给 codex 传任何 sandbox / 审批参数，按它自己的默认来。要改就去改 Codex 的配置。": "We pass codex no sandbox or approval flags — it runs on its own defaults. Change it in Codex's own config.",
  "权限：跟随 Codex": "Access: Codex defaults",
  "Claude Code 在工作台里以 bypassPermissions 跑：改文件、跑命令都不会逐条问你。想要每步确认，去右边终端里自己敲 claude。": "Inside the workbench Claude Code runs with bypassPermissions: it won't ask before editing files or running commands. Want step-by-step confirmation? Run claude yourself in the terminal on the right.",
  "权限：完全访问": "Access: full",
  "网络不稳，正在重连（第 {n}/{max} 次）—— 还在跑，先别关": "Network is flaky, reconnecting (attempt {n}/{max}) — still running, don't close it",
  "网络不稳，正在重连 —— 还在跑，先别关": "Network is flaky, reconnecting — still running, don't close it",
  "预览视频": "Preview video",
  "看这张图": "View this image",
  "这个地址看不懂，检查一下拼写": "Can't parse this address — check the spelling",
  "本机 {port} 端口上没有服务在跑 —— 先让 AI 把开发服务器起起来（比如 npm run dev），起好了再点预览。": "Nothing is listening on port {port} — have the AI start the dev server first (e.g. npm run dev), then hit preview.",
  "打开中…": "Opening…",
  "窗口开着": "Window open",
  "窗口未开": "Window closed",
  "页面在独立窗口里显示（localhost 也能开）；上面这排按钮控制那个窗口。": "The page opens in its own window (localhost works too); the buttons above control that window.",

  // Chat.tsx —— 预览工具栏「用浏览器打开」按钮（改交给系统浏览器，2026-09-06）
  "用系统浏览器打开（可点链接、可登录）": "Open in your system browser (click links, sign in)",
  "打开系统浏览器失败：{err}": "Failed to open the system browser: {err}",

  // ChatPanel.tsx —— Codex 专用模型清单（同上，label 走变量）
  "DeepSeek V4 Flash · 最快最省（默认）": "DeepSeek V4 Flash · fastest & cheapest (default)",
  "GPT-5.3 Codex · 更强（约 6 倍价）": "GPT-5.3 Codex · stronger (~6× the price)",
};
