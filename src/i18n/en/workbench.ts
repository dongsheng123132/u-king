/** 英文覆盖字典 · OpenCodex / U-Workspace 工作台（opencodex/*）。 */
export const workbench: Record<string, string> = {
  // 工作台不再绑死虾盘云（2026-08-21）：没 Key 的两种情况要分开说 ——
  // 「等一下」和「你得去填」是完全不同的两件事。
  "{name} 还没填 API Key —— 去「AI 设置 → 供应商库」补上":
    "{name} has no API key yet — add one under Settings → Providers",
  "还没拿到可用的 API Key，先去「AI 设置」确认这家供应商配好了":
    "No usable API key yet — check that this provider is set up under AI Settings",
  "「AI 技能包」在左侧栏里": "“AI Skill Pack” is in the left sidebar",

  // 卡住时那行提示：分阶段说实话，别拿「多半是慢命令」冒充诊断（pc-***）
  "· 已经 {d} 没有新动静，{w}。等不及就点右边红色按钮停下。":
    "· No new output for {d} — {w}. Hit the red button on the right to stop.",
  "还在跑「{n}」这一步": "still running “{n}”",
  "还在跑一条命令": "still running a command",
  "上一步已经做完了，正在等 AI 回话": "the last step finished; waiting on the AI to reply",
  "AI 回了一半停住了": "the AI stopped halfway through its reply",
  "还没开始跑，卡在启动这一步": "it never started — stuck at launch",
  // 通用 UI 词（跨多个工作台组件共用）
  "终端": "Terminal",
  "文件": "Files",
  "浏览器": "Browser",
  "预览": "Preview",
  // 右侧面板按钮的 tooltip（按钮上仍是上面那四个人话词，tooltip 才带代号）
  "终端（U-CLI）": "Terminal (U-CLI)",
  "预览（图 / 网页 / 文档）": "Preview (images / web pages / documents)",
  "文件树": "File tree",
  "内置浏览器": "Built-in browser",
  "模型": "Model",
  "收起": "Collapse",
  "停止": "Stop",
  "启动": "Start",
  "新建": "New",
  "发送": "Send",
  "驱动配置": "Driver settings",
  "切换模型": "Switch model",

  // useWorkbenchOffer.tsx —— 选完空文件夹之后那一问
  // 🔴 别留空串：i18n/index.tsx 用的是 `EN[zh] ?? zh`，空串**不会**回退中文，会渲染成空白
  "这个文件夹是空的，要布置一下吗？": "This folder is empty — want me to set it up?",
  "内置示例": "Built-in example",
  "给谁用：": "Who it's for: ",
  "会建这些目录": "It will create these folders",
  "外加每个目录一份说明，和 AGENTS.md / CLAUDE.md 两个入口文件 —— AI 进这个文件夹会自动读它们，就知道每个目录是干嘛的。":
    "Plus a README in each folder, and the AGENTS.md / CLAUDE.md entry files — AI auto-loads those when it opens this folder, so it knows what each folder is for.",
  "它没有什么": "What it does not do",
  "还有 {n} 条，装完写在 WORKBENCH.md 里。": "{n} more — written into WORKBENCH.md once installed.",
  "布置成工作台": "Set it up as a workbench",
  "先空着": "Leave it empty",
  "之后在这个文件夹上再新建一次项目，还会问你。": "Create a project on this folder again and I'll ask once more.",
  "好了。": "Done.",
  "装好了，但有一件事要你处理。": "Installed — but there's one thing you need to handle.",
  "没能布置：": "Could not set it up:",
  "建好了 {n} 项。": "Created {n} items.",
  "开始干活": "Get to work",

  // OpenCodex.tsx / SessionList.tsx 项目与会话
  "选择项目文件夹": "Select project folder",
  "新建一个项目，开始干活": "Create a project to get started",
  "选一个文件夹作为项目，和 Claude / Codex 对话。需要时从对话顶部开终端、看文件、开浏览器。":
    "Pick a folder as your project and chat with Claude / Codex. Open a terminal, browse files, or launch a browser from the top of the chat whenever you need.",
  "新建项目（选文件夹）": "New project (pick a folder)",
  "在新终端打开 OpenClaw CLI": "Open OpenClaw CLI in a new terminal",

  // SessionList.tsx
  "创建 worktree 失败: {e}": "Failed to create worktree: {e}",
  "新对话": "New chat",
  "展开会话栏": "Expand session bar",
  // 让步链（lib/yieldChain.ts）：终端排不开 TUI 时自动收起，窗口拉宽自己还原
  "窗口太窄，已自动让位给终端 —— 点这里展开（窗口拉宽会自己还原）":
    "Window too narrow — collapsed to give the terminal room. Click to expand; widening the window restores it.",
  "新建对话": "New chat",
  // 活动指示（会话行小圆点 / 项目组头汇总 / 收起态角标）
  "正在干活中": "Working right now",
  "在线 · 终端开着": "Online · terminal is open",
  "上一轮出错": "Last turn failed",
  "Standby · 聊过，点进去接着聊": "Standby · already chatted, click to pick it back up",
  "离线 · 还没开始": "Offline · not started yet",
  "这一轮已经跑了多久": "How long this turn has been running",
  "展开会话栏（{n} 个会话正在干活）": "Expand session bar ({n} session(s) working)",
  "这个项目下有 {n} 个会话正在干活": "{n} session(s) in this project are working",
  "这个项目下有 {n} 个会话上一轮出错": "{n} session(s) in this project failed on their last turn",
  "会话": "Sessions",
  "收起会话栏（把地方让给终端）": "Collapse session bar (give room to the terminal)",
  "选择文件夹新建项目": "Pick a folder to create a project",
  "已打开的项目": "Open projects",
  "换个视图看": "Switch view",
  "还没有项目。": "No projects yet.",
  "点「新建」选一个文件夹，": "Click “New” and pick a folder,",
  "在里面让多个 AI 一起干活。": "to let multiple AIs work together in it.",
  "未绑定文件夹": "No folder bound",
  "再点一次：删除该项目下全部会话（不会删除磁盘文件夹）":
    "Click again: delete all sessions in this project (won't delete the folder on disk)",
  "删除整个项目（移除其下所有会话，不动磁盘文件夹）":
    "Delete the whole project (removes all its sessions, leaves the disk folder untouched)",
  "新建 worktree（并行在另一个分支上工作）": "New worktree (work on another branch in parallel)",
  "在此项目新开一个 AI 会话": "Open a new AI session in this project",
  "分支名（回车确认）": "Branch name (Enter to confirm)",
  "再点一次确认关闭（不会删除磁盘文件夹）": "Click again to confirm closing (won't delete the disk folder)",
  "关闭会话": "Close session",
  "插件（即将上线）": "Plugins (coming soon)",
  "U-Workspace 版本": "U-Workspace version",
  "拖动调整会话栏宽度 · 双击收起": "Drag to resize the session bar · double-click to collapse",

  // RunPanel.tsx
  "我的 AI": "My AI",

  // UWorkspace.tsx
  "左侧「新建项目」选个文件夹，开始干活":
    "Click “New project” on the left, pick a folder, and get to work",

  // ToolAppView.tsx — 提示 / toast
  "{name} 已在独立终端窗口打开（显示区域更大）":
    "{name} opened in a separate terminal window (more display space)",
  "打开独立终端失败：{e}": "Failed to open a separate terminal: {e}",
  "{name} 已配好虾盘云": "{name} is now set up with Xiapan Cloud",
  "（ClawX 需重启）": " (ClawX needs a restart)",
  "终端还没就绪，请稍候再试": "The terminal isn't ready yet, please try again shortly",
  "检测到 ClawX 桌面版正在运行 —— 它就是完整的 OpenClaw，已为你打开，无需在这里重复启动":
    "Detected ClawX desktop is running — it is the full OpenClaw, opened for you; no need to launch it again here",
  "正在启动 OpenClaw 网页版，就绪后自动打开控制台…":
    "Starting the OpenClaw web version; the console will open automatically once it's ready…",
  "网页版启动较慢或失败，请看下方终端日志，或稍后点右上角「打开网页版」重试":
    "The web version is slow to start or failed. Check the terminal log below, or click “Open web version” in the top-right to retry later",
  "— 在独立终端窗口运行，显示区域更大；关那个窗口才停":
    "— runs in a separate terminal window with more space; close that window to stop",
  "— 点上方提示词启动；启动后常驻，关终端才停":
    "— click a prompt above to start; stays running afterward, close the terminal to stop",
  "起 gateway 并自动打开网页控制台": "Start the gateway and open the web console automatically",
  "打开网页版": "Open web version",
  "展开驱动配置": "Expand driver settings",
  "{name} 运行在独立终端窗口": "{name} runs in a separate terminal window",
  "为了给 {name} 更大的显示区域，它在一个独立的系统终端窗口里运行，不挤在 U-King 界面里。关掉那个终端窗口即停止；下面按钮可再次打开。":
    "To give {name} more display space, it runs in a separate system terminal window instead of being squeezed into the U-King interface. Close that terminal window to stop it; the buttons below can reopen it.",
  "全球最强的编程 AI 助手，能写代码、改 bug、跑命令。点下面的按钮就开始对话。":
    "The world's strongest coding AI assistant — writes code, fixes bugs, runs commands. Click the button below to start chatting.",
  "OpenAI 的编程助手，会读你的代码、自动改文件。点下面的按钮就开始。":
    "OpenAI's coding assistant — reads your code and edits files automatically. Click the button below to start.",
  "开源 AI 智能体（龙虾）。一键启动后会打开网页版控制台，能聊天、自动办事。":
    "Open-source AI agent (Lobster). One click launches the web console where you can chat and get things done automatically.",
  "Hermes 适合聊天、写方案和轻量工具任务。点启动会弹出独立终端窗口进入对话（默认已接好虾盘云），显示区域更大；浏览器接管需要单独体检。":
    "Hermes is great for chatting, drafting plans, and light tool tasks. Clicking start opens a separate terminal window for the conversation (Xiapan Cloud is preconfigured) with more display space; browser takeover needs a separate check.",
  "开源编程 Agent，擅长自动化：定时任务、多项目并行、批量改代码。点启动自动配好虾盘云，直接对话或让它跑任务。":
    "Open-source coding agent built for automation: scheduled tasks, parallel projects, and batch code changes. Clicking start preconfigures Xiapan Cloud — chat directly or hand it tasks.",
  "点下面的按钮开始使用。": "Click the button below to get started.",
  "正在体检 Hermes 浏览器能力...": "Checking Hermes browser capabilities...",
  "浏览器接管已就绪": "Browser takeover is ready",
  "浏览器接管未配置": "Browser takeover not configured",
  "正在读取 Hermes 配置和浏览器工具状态。": "Reading Hermes config and browser tool status.",
  "启动：直接进终端对话界面（和 Claude Code 一样），输入任务即可。":
    "Start: go straight into the terminal chat interface (just like Claude Code) and type your task.",
  "网页版聊天：备选入口，喜欢网页界面的可以用它。":
    "Web chat: an alternative entry for those who prefer a web interface.",
  "浏览器任务：未就绪时优先用 Codex 专区或 ClawX 做网页接管。":
    "Browser tasks: when not ready, prefer the Codex zone or ClawX for web takeover.",
  "重新体检 Hermes 浏览器能力": "Re-check Hermes browser capabilities",
  "一键启动并打开网页版": "Launch and open the web version in one click",
  "启动 {name}": "Start {name}",
  "启动后在独立终端窗口运行，关掉那个窗口才会停止":
    "Runs in a separate terminal window after launch; close that window to stop",
  "启动后会常驻运行，关掉终端标签才会停止": "Keeps running after launch; close the terminal tab to stop",
  "只切换 {name} 的驱动": "Switch driver for {name} only",

  // apps.ts 提示词按钮（数据文件的 key，在 ToolAppView / TermPanel 渲染处 t() 包）
  "继续上次": "Resume last",
  "启动网页版": "Start web version",
  "命令行版": "Command-line version",
  "网页版聊天": "Web chat",
  "重新初始化": "Re-initialize",
  "Web 工作台": "Web workspace",
  "终端模式": "Terminal mode",
  "DeepSeek Harness 终端模式已在新标签打开": "DeepSeek Harness terminal mode opened in a new tab",
  "在新终端标签启动 DeepSeek Harness 持续对话模式":
    "Start persistent DeepSeek Harness chat in a new terminal tab",

  // Chat.tsx —— 引擎 / 模型 / 审批模式
  "Claude Code（推荐·最强·已免配）": "Claude Code (recommended · strongest · preconfigured)",
  "U-King 轻助手（省钱兜底·作图快）": "U-King Lite (budget fallback · fast image gen)",
  "Codex（贵·需单独配置）": "Codex (pricey · needs separate setup)",
  // 上游 403 的人话（两档：这一轮是走客户自己的账号，还是走虾盘云）
  "上游回了 403：认得出你是谁，但这一次不让用": "Upstream returned 403: it knows who you are, but won't allow this request",
  "**这一轮走的是你自己的账号**（你在「AI 设置」里给它配过官方登录 / 自己的 Key，我们不会覆盖），所以这条 403 来自那一家、不是虾盘云。先去那家看看账号状态：模型有没有权限、有没有欠费或被限地区（挂代理的话换个出口再试）。想改走虾盘云就去「AI 设置」切一下。":
    "**This turn ran on your own account** — you configured an official login / your own key under AI Settings, and we never override that. So this 403 came from that provider, not from Xiapan Cloud. Check your account there: model access, unpaid balance, or a blocked region (if you use a proxy, try another exit node). To route through Xiapan Cloud instead, switch it in AI Settings.",
  "常见三种：这个模型你的档位用不了、上游渠道临时下架、或者出口 IP 被拦。先在顶栏「换模型」换成 DeepSeek Flash 重发一次；换了还是 403 就用「技术支持」把下面这段原文发给我们（里面有 request id，我们能直接查到是哪条渠道）。":
    "Three common causes: your tier can't use this model, the upstream channel is temporarily down, or your exit IP is blocked. First switch the model to DeepSeek Flash in the top bar and resend; if it's still 403, use Feedback to send us the raw text below (it contains a request id we can trace to the exact channel).",
  "Hermes（终端）": "Hermes (terminal)",
  "Hermes 终端（自带记忆）": "Hermes terminal (remembers you)",
  "Claude Code 终端（原味 TUI·老手推荐）": "Claude Code terminal (raw TUI · for power users)",
  "DeepSeek Flash（快·省）": "DeepSeek Flash (fast · cheap)",
  "DeepSeek Pro（强）": "DeepSeek Pro (strong)",
  "切到 Claude Code 大脑": "Switch to the Claude Code engine",
  "切到 Claude Code 终端（原味 TUI）": "Switch to the Claude Code terminal (raw TUI)",
  "轻助手是直连模型 API 跑的，这一轮没有命令行等价物。想看「对话框底下就是终端」——点这里切到 Claude Code 大脑。":
    "Lite talks to the model API directly, so this turn has no command-line equivalent. Want to see that the chat box IS a terminal underneath? Click here to switch to the Claude Code engine.",
  "轻助手是直连模型 API 跑的，这一轮没有命令行等价物。想要「对话框底下就是终端」——点这里切到 Claude Code 终端。":
    "Lite talks to the model API directly, so this turn has no command-line equivalent. Want the chat box to BE a terminal? Click here to switch to the Claude Code terminal.",
  "每步确认（最安全）": "Confirm each step (safest)",
  "自动（写文件自动·命令仍问）": "Auto (auto file writes · still asks for commands)",
  "全授权（都不问）": "Full access (never asks)",

  // Chat.tsx —— 快捷调用（标签 + 起手提示词模板）
  "作图": "Image",
  "帮我画一张：": "Draw me: ",
  "做PPT": "Make PPT",
  "帮我做一份 PPT，主题是：": "Make me a PPT on the topic: ",
  "写文档": "Write doc",
  "帮我写一份文档（Word/Markdown），内容是：": "Write me a document (Word/Markdown) about: ",
  "做网页": "Make webpage",
  "帮我做一个网页（HTML），要求：": "Make me a webpage (HTML) with these requirements: ",
  "写代码": "Write code",
  "帮我写代码：": "Write code for me: ",

  // Chat.tsx —— 工具标签
  "列目录": "List dir",
  "读文件": "Read file",
  "写文件": "Write file",
  "改文件": "Edit file",
  "跑命令": "Run command",

  // Chat.tsx —— toast / 提示
  "预览失败: {e}": "Preview failed: {e}",
  "找不到这个文件：{p}（AI 可能还没写出来，或路径不是相对这个工作文件夹）":
    "Can't find this file: {p} (the AI may not have written it yet, or the path isn't relative to this working folder)",

  "打不开这个网址：{e}": "Can't open this URL: {e}",
  "选择工作文件夹（AI 和终端都在这里面读写文件、跑命令）":
    "Select a working folder (the AI and terminal read/write files and run commands inside it)",
  "先选个工作文件夹": "Pick a working folder first",
  "打开失败: {e}": "Failed to open: {e}",
  "⚠ 找不到这个文件 —— 看看都试过哪儿": "⚠ File not found — see where we looked",
  "复制文件名": "Copy file name",
  "用其他程序打开…": "Open with another app…",
  "打开这个文件夹": "Open this folder",

  "切到这个目录（cd）": "cd into this folder",
  "在系统终端里打开这个目录": "Open this folder in the system terminal",

  "已复制文件名": "File name copied",
  "这些位置都没有「{name}」：\n{tried}": 'None of these locations has "{name}":\n{tried}',

  "先选一个工作文件夹，AI 和终端都在里面干活":
    "Pick a working folder first — the AI and terminal both work inside it",
  "已复制": "Copied",
  "复制失败，请手动选中复制": "Copy failed, please select and copy manually",
  "对话失败": "Chat failed",
  "对话启动失败: {e}": "Failed to start chat: {e}",
  "还没拿到设备 Key，稍等一下再试": "Device key not ready yet, please try again in a moment",

  // Chat.tsx —— 界面文案
  "对话大脑：自家助手 或 驱动 Claude Code 真身":
    "Chat brain: our own assistant or the real Claude Code driver",
  "复杂/多文件任务，切到更强的引擎": "For complex / multi-file tasks, switch to a stronger engine",
  "任务较重？切 {name} 更强": "Heavy task? Switch to {name} for more power",
  "选工作文件夹后 AI / 终端才能读写文件、跑命令":
    "Pick a working folder so the AI / terminal can read/write files and run commands",
  "选工作文件夹": "Pick working folder",
  "用外部应用打开这个文件夹": "Open this folder in an external app",
  "打开方式": "Open with",
  "资源管理器": "File Explorer",
  "系统终端": "System terminal",
  "审批模式": "Approval mode",
  "收起对话列，终端/预览全屏": "Collapse the chat column, full-screen the terminal/preview",
  "{name} 大脑要在一个工作文件夹里干活": "The {name} brain needs a working folder to operate in",
  "请先在「① 装 AI」装好该工具并在「② 虾盘云」一键配好驱动":
    "First install the tool under “① Install AI”, then set up the driver in one click under “② Xiapan Cloud”",
  "{name} 已就位": "{name} is ready",
  "下面点一个「试试这样问我」，或直接说你的需求":
    "Click one of the “try asking me” prompts below, or just tell me what you need",
  "有什么可以帮你的？": "How can I help?",
  "画图 · 读写文件 · 跑命令 · 右上角开终端/浏览器——选个工作文件夹让它动手":
    "Draw · read/write files · run commands · open a terminal/browser from the top-right — pick a working folder to let it act",
  "复杂活会组合调用最强工具：": "Complex work combines the strongest tools: ",
  "AI 想跑命令：": "AI wants to run a command: ",
  "AI 想改文件：": "AI wants to edit a file: ",
  "AI 想写文件：": "AI wants to write a file: ",
  "，是否允许？": " — allow?",
  "批准": "Approve",
  "拒绝": "Reject",
  "✅ 已批准": "✅ Approved",
  "已拒绝": "Rejected",
  "运行：": "Running: ",
  "正在作图：{prompt}": "Generating image: {prompt}",
  "处理中…": "Processing…",
  "正在出片，约 1-3 分钟…": "Rendering video, about 1-3 min…",
  "已生成视频：": "Video generated: ",
  "在右侧播放": "Play on the right",
  "在右侧预览播放": "Play in the preview on the right",
  "生成的视频": "Generated video",
  "视频预览": "Video preview",
  "已生成：": "Generated: ",
  "点击在右侧放大预览": "Click to enlarge preview on the right",
  "点击放大": "Click to enlarge",
  "预览网页": "Preview webpage",
  "排队中（本轮完成自动发）:": "Queued (auto-sends when this round finishes):",
  "让它读写文件、跑命令、画图、或聊天…": "Let it read/write files, run commands, draw, or chat…",
  "打字问点什么，或说「画一张…」…": "Type a question, or say “draw a…”…",
  "复制这段": "Copy this",
  "展开对话列": "Expand chat column",
  "收起面板": "Collapse panel",
  "返回对话": "Back to chat",
  "图片 / 视频 / 网页 / PPT · Word · Excel · PDF 都在这里预览":
    "Images / video / webpages / PPT · Word · Excel · PDF all preview here",
  "让它「画一张…」「做个 PPT…」「整理成表格…」，成果就出现在这里":
    "Say “draw a…”, “make a deck…” or “turn this into a table…” — the result shows up here",
  "缩小": "Zoom out",
  "放大": "Zoom in",
  "还原": "Reset",
  "网页预览": "Webpage preview",

  // FilesPanel.tsx —— 文件面板 / 文件管理（Codex 式）
  "刷新": "Refresh",
  "打开": "Open",
  "在外部应用打开这个文件夹": "Open this folder in an external app",
  "双击文件预览（图片 / PDF / Word / Excel / PPT / ZIP / 文本 …）；右键更多操作":
    "Double-click a file to preview (image / PDF / Word / Excel / PPT / ZIP / text …); right-click for more",
  "在资源管理器打开": "Open in File Explorer",
  "在资源管理器中显示": "Show in File Explorer",
  "在终端打开": "Open in Terminal",
  "在 Git Bash 打开": "Open in Git Bash",
  "用 VS Code 打开": "Open in VS Code",
  "用 Cursor 打开": "Open in Cursor",
  "用默认程序打开": "Open with default app",
  "复制路径": "Copy path",
  "已复制路径": "Path copied",

  // Composer.tsx / Chat.tsx —— 输入框外壳 + 轻助手空态（2026-08 改版）
  "这类活「{name}」更拿手（{why}）—— 已替你切过去，顶栏可切回": "“{name}” is better at this ({why}) — switched for you; you can switch back in the top bar.",
  "先选一个工作文件夹才能定位这个文件": "Pick a working folder first so this file can be located",
  "字小一点": "Smaller text",
  "字大一点": "Larger text",
  "松手把文件路径插进输入框": "Drop to insert the file path into the box",
  "要在「{dir}」里做点什么？": "What should we do in “{dir}”?",
  "画图 · 读写文件 · 跑命令 —— 说人话就行，它会自己动手。": "Draw · read/write files · run commands — just say it in plain words and it gets to work.",
  "画图 · 读写文件 · 跑命令 —— 选个工作文件夹（就在输入框下面）它才能动手。": "Draw · read/write files · run commands — pick a working folder (just below the box) before it can act.",
  "想要完整能力？开终端跑 Claude Code": "Want the full toolset? Open a terminal and run Claude Code",
  "不想碰终端也行 —— 直接在下面说人话。轻助手跟 Claude Code 用的是同一批模型、同一个 Key，差的是外壳。": "Rather not touch a terminal? Just type below. The light assistant uses the same models and the same key as Claude Code — only the shell differs.",
  "查看改动（-{o} +{n} 行）": "View changes (-{o} +{n} lines)",
  // ChatPanel 的折叠开关并进卡片头部后，那侧只剩一个行数
  "{n} 行": "{n} lines",
  // 🔴 别顺手删这条：Chat.tsx（轻助手）**另有一套**工具卡渲染，还在用完整文案。
  // 我删过一次，`check-i18n-missing` 当场把它捞了出来 —— 两套实现就是这么互相绊的。
  "查看输出（{n} 行）": "View output ({n} lines)",
  "让它读写文件、跑命令、画图… @ 引用文件，/ 调指令，Enter 发送": "Have it read/write files, run commands, draw… @ to attach a file, / for commands, Enter to send",
  "打字问点什么，或说「画一张…」；/ 调指令，Enter 发送": "Ask anything, or say “draw a…”; / for commands, Enter to send",
  "轻助手这一轮用哪个模型": "Which model the light assistant uses for this turn",
  "审批模式：AI 动手前问不问你": "Approval mode: whether the AI asks before acting",
  "权限：": "Access: ",
  "清空对话（这个会话从头开始）": "Clear conversation (start this session over)",
  "没选文件夹时只能聊天，不能读写文件 / 跑命令": "Without a folder it can only chat — no file access, no commands",
  "停止这一轮": "Stop this turn",
  "发送（Enter 发送 · Shift+Enter 换行）": "Send (Enter to send · Shift+Enter for a new line)",
  "选一个文件夹": "Pick a folder",
  "选文件（可多选）": "Pick files (multiple allowed)",
  "把真实路径插进输入框": "Inserts the real path",
  "整个目录交给它": "Hand it a whole folder",
  "等于打一个 @": "Same as typing @",
  "添加文件 / 文件夹（也可以直接拖进来）": "Add files / folders (you can also just drag them in)",
  "也可以直接把文件拖进对话框": "You can also drag files straight into the box",
  "这个文件夹读不到": "Can't read this folder",
  "先在上面选一个工作文件夹，@ 才能列出里面的文件": "Pick a working folder above first — then @ can list the files inside",

  // Composer.tsx —— `+` 附件菜单（label 是变量传进 t()，提取脚本抓不到，靠人核）
  "添加文件…": "Add files…",
  "添加文件夹…": "Add a folder…",
  "引用工作区里的文件": "Reference a file in the workspace",

  // QuickPrompts 场景 tab + ENGINES 大脑名（都是 t(变量)，提取脚本抓不到）
  "日常办公": "Everyday work",
  "代码开发": "Coding",
  "设计创意": "Design & ideas",
  "Codex（已免配·换个脑子试试）": "Codex (pre-configured · try a different brain)",

  // QuickPrompts.tsx —— 起手词 21 组（label + template，都走 t(变量)）
  // ⚠️ `scripts/check-i18n-missing.mjs` **扫不到这个文件的词**（它们是数组里的变量，不是
  //    JSX 里的字面量）。所以这一段漏了不会变红，加起手词的人得自己顺手补一条。
  // useTermGroup.ts —— 终端右键菜单（同样是数组里的变量，check-i18n-missing 扫不到）
  "复制": "Copy",
  "粘贴": "Paste",
  "全选": "Select all",
  "清屏": "Clear",
  "读不到剪贴板 —— 请用 Ctrl+V 粘贴": "Can't read the clipboard — use Ctrl+V to paste",
  "搭我的工作台": "Set up my workbench",
  "帮我搭一个我自己的工作台。先只读盘点一下这个文件夹再问我几句，别急着建目录：":
    "Set up a workbench for me. Take a read-only inventory of this folder and ask me a few questions first — don't create anything yet: ",
  "装了搭工作台的技能，会先盘点你的文件夹再动手":
    "Has the workbench skill — it inventories your folder before touching anything",
  "写周报": "Weekly report",
  "帮我写这周的周报，我的工作是：": "Write my weekly report. Here's what I worked on: ",
  "总结会议": "Summarize a meeting",
  "把这段会议记录总结成要点：": "Summarize these meeting notes into key points: ",
  "做表格": "Make a spreadsheet",
  "帮我做一个表格，统计这些数据：": "Build me a spreadsheet from this data: ",
  "做幻灯片": "Make slides",
  "把这份内容做成幻灯片：": "Turn this into a slide deck: ",
  "改简历": "Polish a résumé",
  "帮我改一下这份简历，我想：": "Revise this résumé for me. I want to: ",
  "写公众号": "Write an article",
  "帮我写一篇公众号文章，主题是：": "Write me an article. The topic is: ",
  "翻译文档": "Translate a document",
  "把这份文档翻译成中文：": "Translate this document into English: ",
  "改代码": "Edit code",
  "帮我写一段代码，功能是：": "Write me some code that: ",
  "帮我改一下这段代码，问题在于：": "Fix this code for me. The problem is: ",
  "找问题": "Find the bug",
  "这段代码跑不通，帮我找找原因：": "This code doesn't run — help me find out why: ",
  "帮我在终端里运行这个命令：": "Run this command in the terminal for me: ",
  "帮我看看这个文件的内容：": "Show me what's in this file: ",
  "存文件": "Save to a file",
  "把这串内容存成一个文件：": "Save this content into a file: ",
  "写脚本": "Write a script",
  "帮我写个小脚本，用来：": "Write me a small script that: ",
  "画张图": "Draw an image",
  "帮我画一张图，内容是：": "Draw me an image of: ",
  "改图片": "Edit an image",
  "帮我改一下这张图片，我想：": "Edit this image for me. I want to: ",
  "做海报": "Make a poster",
  "帮我做一张活动海报，主题是：": "Design an event poster. The theme is: ",
  "做视频": "Make a video",
  "帮我做一个视频，内容是关于：": "Make me a video about: ",
  "找配图": "Find an illustration",
  "帮我配一张插图，风格要：": "Create an illustration for me, in this style: ",
  "做封面": "Make a cover",
  "帮我设计一个封面，标题是：": "Design a cover for me. The title is: ",
  "做头像": "Make an avatar",
  "帮我画一个头像，我想要：": "Draw me an avatar. I want: ",
  // ── 任务看板（TaskBoard.tsx）+ 自动化·长程记忆（AutomationPanel.tsx）──────────
  看板: "Board",
  护照: "Passports",
  // 🔴 列名说的是**会话**不是任务：`Done` 会被读成「这活干完了」，而它只证明
  //    「那个会话文件没人写了」。中文侧同理（原来叫「已完成」）—— 见 TaskBoard.tsx 的 COLUMNS。
  没在跑: "Idle",
  在跑: "Running",
  等待输入: "Waiting",
  已结束: "Ended",
  出错: "Error",
  已完成: "Done", // App.tsx 的装机步骤还在用，那里「完成」是真值
  "任务看板": "Task board",
  "任务护照": "Task Passports",
  "一个任务一张护照，跨 AI 接力；会话运行状态仍在下方看板。":
    "One passport per task, carried across AI harnesses; live session status remains on the board below.",
  "可跨 AI 接力的任务": "Tasks that can move across AI harnesses",
  "项目可有多张护照；护照不等于聊天会话": "A project can have many passports; a passport is not a chat session",
  "还没有任务护照。对任意已接入 U-King 的 AI 说：为当前目标创建一张任务护照。":
    "No task passports yet. Ask any U-King-connected AI to create one for the current objective.",
  "复制交接口令": "Copy handoff prompt",
  交接: "Handoff",
  "上次：{h}": "Last: {h}",
  尚未标记接手方: "No receiving harness recorded",
  "请接手任务护照 {id}：先读取当前状态与下一步，只继承已验证事实，不继承上一位 AI 的聊天记录。":
    "Take over task passport {id}: read its current state and next steps first; inherit only verified facts, not the previous AI's chat transcript.",
  "会话谁在跑 / 谁跑完 / 谁挂了，一屏看全；点卡片就打开那个会话":
    "Who is running / who finished / who failed — all on one screen. Click a card to open that session.",
  "定时任务": "Scheduled tasks",
  "去配置 →": "Configure →",
  "还没有定时任务 —— 到点让 AI 自己干活，见「自动化」":
    "No scheduled tasks yet — set AI to work on its own, see “Automations”.",
  "新建项目": "New project",
  "无工作文件夹": "No working folder",
  "没有闲着的": "Nothing idle",
  "没有在跑的": "Nothing running",
  "没有等待输入的": "Nothing waiting",
  "没有已结束的": "Nothing ended",
  "没有出错的": "Nothing failed",
  "「已结束」= 那个会话文件近 {s} 秒没被写，不代表事情做完了；「没在跑」= 登记过但当前没有会话在跑。外部 AI 的记录里没有「这次成没成」这种字段，所以永远不进「出错」列 —— 只有本工作台的状态是真值。":
    "“Ended” means that session file hasn't been written for {s}s — it does NOT mean the work is done. “Idle” means registered but no session currently running. External AI records carry no “did this succeed” field, so they never enter the “Error” column — only this workbench's status is ground truth.",
  "状态是 AI 自己写在看板上的": "The AI declared this status on its own board",
  "AI 声明过状态，但太久没更新，已经不能当「现在」用了":
    "The AI declared a status, but it hasn't been updated in too long to describe the present",
  "状态按会话文件还在不在被写推的": "Status inferred from whether the session file is still being written",
  "下次 {t}": "Next {t}",
  已停用: "Disabled",
  马上: "now",
  "长程记忆：下一班接着上一班的进度干": "Long-run memory: continue from the last run's progress",
  "每班跑完把结论存进这份任务的记忆，下一班开头自动带上。适合「一个长活分几班推进」；独立的日更任务别开 —— 开了它会接着上次的写。记忆文件在 ~/.uking/automation/，随时能看。":
    "Each run saves its conclusions into this task's memory file, and the next run picks them up automatically. Use it when one long job advances across several runs; keep it OFF for independent daily tasks — with it on, each run continues the previous one. The memory file lives in ~/.uking/automation/ and you can read it anytime.",
  "已招": "Hired",
  // 1.0.3 专家墙：解聘 + 市场入口 + 起手词收行
  "解聘（删掉这个专家包）": "Dismiss (delete this expert pack)",
  "正在解聘…": "Dismissing…",
  "还缺人？去技能市场 skillhub.cn 找": "Need someone else? Find them on skillhub.cn",
  "找专家": "Find an expert",
  "找专家 / 装技能": "Experts / skills",
  // HireSearch —— 去市场现搜可招的人（动态，不自建货架）
  "去市场找人": "Hire from the ecosystem",
  "用哪个专家干这活（不挑就是通用助手）": "Which expert does this job (none = general assistant)",
  "通用助手": "General assistant",
  "现搜 npm / DSH 插件 / 技能包 —— 我们不自建货架，直接看生态里现在有什么。只搜不装。":
    "Search npm / DSH plugins / skill packs live — we don't run a storefront, we show what the ecosystem has right now. Search only, never installs.",
  "搜「飞书」「公众号」「cad」，或 keywords:dsh-plugin": "Try “feishu”, “wechat”, “cad”, or keywords:dsh-plugin",
  "没连上市场（网络或代理）—— 这不代表没有，只代表这次没问到。":
    "Couldn't reach the registry (network or proxy) — that means we didn't get an answer, not that nothing exists.",
  "没搜到匹配的。换个词试试，比如工具名或用途。": "No matches. Try another word — a tool name or what you want it to do.",
  "怎么招：": "How to hire: ",
  "周装 {n}": "{n}/wk",
  "复制装法": "Copy install",
  "看详情": "Details",
  "已复制安装命令，去终端粘贴执行": "Install command copied — paste it into the terminal",
  "还有 {n} 条没显示 —— 把关键词写具体一点": "{n} more not shown — try a more specific keyword",
  "也可以去 skillhub.cn 逛": "Or browse skillhub.cn",
  "还有 {n} 个": "{n} more",
  "缺技能包": "Missing skill pack",
  "缺工具": "Missing tool",

  // 输入框重排（2026-08-18 按 DSH 收：上面两三个等宽下拉、框内只留能力、框外零控件）
  "描述你要做的事…": "Describe what you want done…",
  "说点什么，或选个工作文件夹让它动手": "Say something — or pick a working folder so it can act",
  "{p}（点开可换文件夹 / 用外部应用打开）": "{p} (click to switch folder / open externally)",
  "换一个文件夹…": "Switch folder…",
  "用外部应用打开": "Open with",
  "＋ 找专家 / 装技能…": "+ Find an expert / add skills…",
  "关掉右边的文件栏": "Close the file column",
  "在终端右边开一栏：文件树 + 预览": "Open a column beside the terminal: file tree + preview",
  "等于打一个 /": "same as typing /",
  "动手前问不问你": "Asks before acting?",
  "也可以直接把文件拖进对话框 · Enter 发送，Shift+Enter 换行":
    "You can also drag files in · Enter sends, Shift+Enter for a new line",
  "我们没给 Codex 传任何 sandbox / 审批参数，按它自己的默认来。要改去改 Codex 的配置。":
    "We pass Codex no sandbox or approval flags — it uses its own defaults. Change them in Codex's own config.",
  "Claude Code 在工作台里全授权跑：改文件、跑命令都不会逐条问你。想每步确认，去右边终端里自己敲 claude。":
    "Claude Code runs fully authorized in the workbench: it edits files and runs commands without asking each time. Want per-step confirmation? Run claude yourself in the terminal on the right.",

  // 输入框二次收敛（2026-08-18）：框外零控件、大脑+模型合一、起手词下移、加 slogan
  "用哪个大脑的哪个模型干这活": "Which brain and which model handles this",
  "跟随驱动设置": "Follow driver settings",
  "U-King，让你的 AI 直接干活。": "U-King — put your AI to work.",
  "在哪干活": "Where it works",
  "还没选工作文件夹": "No working folder yet",
  "换文件夹…": "Switch folder…",
  "选文件夹…": "Pick a folder…",
  "谁来干": "Who does it",
  "用它自己的配置": "Uses its own settings",

  // 第三刀（2026-08-18）：品牌两行 + 大脑留框内、模型进 +、文件夹回到框下轻行
  "U-King": "U-King",
  "更多 AI，你来指挥": "More AI. You call the shots.",
  "告诉 U-King，你想完成什么…": "Tell U-King what you want done…",
  "告诉 U-King 你想完成什么，或先选个工作文件夹": "Tell U-King what you want done — or pick a working folder first",
  "用哪个大脑干这活": "Which brain handles this",
  "用哪个模型": "Which model",
  "选择文件夹": "Pick a folder",
  "选个工作文件夹，AI 才能读写文件、跑命令": "Pick a working folder so the AI can read/write files and run commands",
  // 工作目录在会话内锁死（2026-08-18）：中途换目录会让 claude --resume 必然找不到会话
  "这个会话还没绑工作文件夹": "This session has no working folder yet",
  "在这个文件夹里打开": "Open this folder in",
  // `+` 改成二级菜单（照 WorkBuddy / MiniMax / Claude Cowork）
  "也可以直接拖进来": "or just drag it in",
  "添加文件 / 选专家 / 换模型（也可以直接把文件拖进来）":
    "Add files / pick an expert / switch model (you can also drag files in)",
  "这一轮：动手前不逐条问你": "This run: won't ask before each action",
  "更多（海外旗舰 · 更费额度）": "More (overseas flagships · pricier)",
  "专家": "Expert",
  "权限": "Permissions",

  "拉出": "Pop out",
  "把终端拉成独立窗口（可以和工作台并排看）": "Pop the terminal into its own window (side-by-side with the workbench)",
  "拉出终端失败：{e}": "Could not pop out the terminal: {e}",
  "装好了，再试一次": "Installed it — try again",

  // 归档区（2026-08-25）：关会话 = 归档（可找回），彻底删除只在归档区
  "归档这个会话（聊天记录保留，可从底部归档区恢复）":
    "Archive this session (chat history is kept; restore from the archive at the bottom)",
  "归档整个项目下的会话（记录保留，可从底部归档区恢复）":
    "Archive all sessions in this project (history is kept; restore from the archive at the bottom)",
  "已归档会话": "Archived sessions",
  "已归档的会话。归档只是收起来，随时能恢复":
    "Archived sessions. Archiving only tucks them away — restore anytime",
  "{n} 条": "{n} msgs",
  "恢复到「{dir}」，聊天记录接上继续聊": "Restore to \"{dir}\" — chat history picks up where it left off",
  "恢复这个会话（原项目文件夹已不记得了，恢复为未绑定会话）":
    "Restore this session (its original project folder is unknown; restored as unbound)",
  "彻底删除（将无法找回）": "Delete forever (cannot be undone)",
  "彻底删除「{name}」？它的 {n} 条对话记录将无法找回。":
    "Permanently delete \"{name}\"? Its {n} message(s) cannot be recovered.",

  // 「更多」折叠（2026-08-25）：护照/看板/AI 专家/自动化收进一个折叠入口
  "更多": "More",
  "护照 / 看板 / AI 专家 / 自动化": "Passports / Board / AI experts / Automation",
  "当前不在这个视图上": "You're not on the chat view right now",
  "有上次没跑成的任务": "Some tasks failed last time",
  "图片识别失败: {e}": "Image recognition failed: {e}",
  "⚠️ 图片识别失败：{e}。图片没有交给当前对话模型。": "⚠️ Image recognition failed: {e}. The image was not sent to the current chat model.",
};
