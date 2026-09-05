/** 英文覆盖字典 · App 壳（App.tsx）。key = 中文原文，value = English。 */
export const app: Record<string, string> = {
  "发现新版本 v{ver}": "New version v{ver} found",
  "暂时检查不到更新（网络？稍后再试）": "Unable to check for updates right now (network?) — please try again later",
  // ---- App: toasts / flashes ----
  "已升级到最新版 v{v} ✓": "Upgraded to the latest v{v} ✓",
  "U-King · AI 管家 · v{v}": "U-King · AI Butler · v{v}",
  "已刷新余额：{text}": "Balance refreshed: {text}",
  "可用": "Available",
  "还没查到充值到账，稍等几秒再刷新": "Top-up not detected yet — wait a few seconds and refresh",
  "刷新余额失败：": "Failed to refresh balance: ",
  "已打开充值页；付款后回到 U-King 会自动刷新，也可以点「刷新余额」":
    "Top-up page opened. Your balance refreshes automatically when you return to U-King, or click \"Refresh balance\".",
  "已更新到本地最新版": "Updated to the latest local version",
  "安装完成！桌面已生成快捷方式": "Installed! A desktop shortcut has been created",
  "安装失败：": "Install failed: ",
  "已在桌面创建 U-King 快捷方式，以后双击桌面图标即可打开":
    "A U-King shortcut has been created on the desktop — just double-click it to open from now on",
  "固定到桌面失败：": "Failed to pin to desktop: ",
  "正在下载新版…": "Downloading the new version…",
  "无法确认终端运行状态，已暂停本次升级以保护运行中的终端，请稍后再试":
    "Unable to confirm terminal activity. This upgrade was paused to protect running terminals; please try again later.",
  "有 {n} 个终端正在运行，升级会关闭它们。是否立即升级？取消则暂不执行。":
    "{n} terminal(s) are running. Upgrading will close them. Upgrade now? Cancel to postpone.",
  "有 {n} 个终端正在运行，重装会关闭它们。是否立即重装？取消则暂不执行。":
    "{n} terminal(s) are running. Reinstalling will close them. Reinstall now? Cancel to postpone.",
  "升级已延后，终端关闭后可再来升级":
    "Upgrade postponed. You can try again after closing the terminals.",
  "重装已延后，终端关闭后可再来重装":
    "Reinstall postponed. You can try again after closing the terminals.",
  "上次升级时有 {n} 个终端": "There were {n} terminal(s) when you last upgraded",
  "含 {n} 个可续接会话": "Includes {n} resumable session(s)",
  "可重开同样目录和命令的终端；原来的屏幕内容和运行现场不会回来。":
    "Reopen terminals with the same folders and commands; their old screen contents and running state cannot be restored.",
  "一键重开": "Reopen all",
  "不再提醒": "Don't remind me",
  "正在重开…": "Reopening…",
  "有 {n} 个终端重开失败，快照已保留，可重试":
    "Failed to reopen {n} terminal(s). The snapshot was kept; you can retry.",
  "{n} 条重开失败": "{n} terminal(s) failed to reopen",
  "快照已保留；不会自动重试，请确认后手动重试。":
    "The snapshot was kept. It will not retry automatically; retry manually when ready.",
  "新版已下载完成，正在自动替换并重启 —— 窗口会消失几秒，请稍候，会自动打开新版（无需重新安装）":
    "The new version is downloaded and is now replacing and restarting — the window will disappear for a few seconds; please wait, the new version will open automatically (no reinstall needed)",
  "自动升级未成功：": "Auto-upgrade failed: ",
  " —— 可再点一次「一键升级」重试；多次失败可去下载页手动获取。":
    " — click \"One-click upgrade\" to retry; if it keeps failing, get it manually from the download page.",
  "正在打开 {name}…": "Opening {name}…",
  "ClawX 已安装，请从开始菜单 / 桌面图标打开": "ClawX is installed — open it from the Start menu or desktop icon",
  "正在检测 ClawX…": "Checking ClawX…",
  "已检测到 ClawX，正在打开…": "ClawX detected — opening…",
  "开始下载 ClawX（约 210 MB）…": "Downloading ClawX (about 210 MB)…",
  "（装好后可在顶部「接入虾盘云」一键配置）":
    " (once installed, use \"Connect Xiapan Cloud\" at the top to configure in one click)",
  "自动安装未成：": "Auto-install failed: ",
  "打开链接失败": "Failed to open link",
  "没找到 ClawX，先帮你安装…": "ClawX not found — installing it for you…",
  "{name} 还没装好，请先在工具中心安装": "{name} isn't ready yet — please install it from the tool center first",
  "该工具从开始菜单 / 应用列表打开": "Open this tool from the Start menu or app list",
  "已为 {name} 打开独立终端": "Opened a standalone terminal for {name}",
  "；作图/视频技能包已装进 {tools}": "; the Image/Video Skill Pack has been installed into {tools}",
  "；作图/视频技能包已导出到 {dir}": "; the Image/Video Skill Pack has been exported to {dir}",
  "；作图/视频技能包可到「AI 技能包」页一键安装":
    "; you can install the Image/Video Skill Pack in one click from the \"AI Skill Pack\" page",
  "AI 工具": "AI tools",
  "已接入虾盘云：{who} 现在国内直连": "Connected to Xiapan Cloud: {who} now use a direct China connection",
  "，ClawX 请重启生效": " — restart ClawX to apply",
  "接入失败：": "Connection failed: ",

  // ---- App: Feed titles ----
  "最新动态": "What's New",
  "新功能 · 活动 · 公告": "New features · Events · Announcements",
  "AI 学院": "AI Academy",
  "教程 · 玩法 · 进阶课": "Tutorials · Tips · Advanced courses",

  // ---- App: ClawX network-access hint overlay ----
  "马上打开 ClawX · 请放行网络": "Opening ClawX · Please allow network access",
  "ClawX 第一次打开时，Windows 可能弹出一个": "The first time ClawX opens, Windows may pop up a ",
  "「Windows 安全中心警报 / 是否允许访问」": "\"Windows Security Alert / Allow access?\"",
  "的窗口。": " window.",
  "请一定点": "Be sure to click",
  "允许访问": "Allow access",
  "，并把「专用网络 / 公用网络」都勾上。": ", and check both \"Private\" and \"Public\" networks.",
  "如果点了「取消」，ClawX 连不上 AI，会一直转圈或报错。":
    "If you click \"Cancel\", ClawX can't reach the AI and will keep spinning or show errors.",
  "（U-King 已尽量帮你提前放行，这条提示只出现一次。）":
    "(U-King has tried to allow access in advance; this notice appears only once.)",
  "取消": "Cancel",
  "知道了，打开 ClawX": "Got it, open ClawX",

  // ---- StatusLine ----
  "新版本": "New version",
  "当前": "Current",
  "升级中 {pct}%": "Upgrading {pct}%",
  "升级中…": "Upgrading…",
  "一键升级": "One-click upgrade",
  "稍后": "Later",
  "开始装机": "Start setup",
  "去配驱动": "Configure provider",
  "立即充值": "Top up now",
  "检测到已装": "Detected",
  "，还没接入虾盘云驱动 —— 接入后国内直连、用内置 Key，无需自己填":
    " but not connected to the Xiapan Cloud provider yet — once connected it uses a direct China connection with the built-in key, no manual entry needed",
  "接入虾盘云": "Connect Xiapan Cloud",
  "不用了": "No thanks",
  "让你的 AI 会「画图 / 做视频」—— 一键把": "Give your AI image and video skills — install",
  "AI 作图能力": "AI image generation",
  "装给 Claude / ClawX，装完直接说「帮我画张图」": " into Claude / ClawX; then just say \"draw me a picture\"",
  "去装作图能力": "Add image powers",
  "从右键菜单打开：": "Opened from context menu: ",

  // ---- TitleBar ----
  "隐藏到右下角托盘": "Minimize to system tray",
  "缩到托盘": "To tray",

  // ---- DriverBar ----
  "虾盘云（内置）": "Xiapan Cloud (built-in)",
  "智谱 GLM": "Zhipu GLM",
  "AI 装机 · 软件免费，用 AI 才充值": "Install AI · Software is free; you only top up to use AI",
  "第一次用点「一键全安装」最省事，工具全部免费，真正用 AI 时才消耗余额。":
    "For your first time, \"Install everything\" is easiest. All tools are free; balance is only used when you actually use AI.",
  "某个工具装不上？看教程": "A tool won't install? See the guide",
  "当前驱动": "Current provider",
  "官方默认 / 未配置": "Official default / Not configured",
  "余额偏低，Codex 可能不够一次请求": "Low balance — may not cover a single Codex request",
  "虾盘云已开通": "Xiapan Cloud activated",
  "余额不足，请充值": "Insufficient balance — please top up",
  "内置 Key 检测中…": "Checking built-in key…",
  "充值开通虾盘云": "Top up to activate Xiapan Cloud",
  "补充余额": "Add balance",
  "充值开通": "Top up to activate",
  "一键全安装": "Install everything",
  "逐个选装": "Install individually",

  // ---- XiapanGuide ----
  "免费装工具": "Install tools for free",
  "ClawX / Claude Code / Codex，一键装到电脑": "ClawX / Claude Code / Codex — install to your PC in one click",
  "去装机": "Go to setup",
  "自动配模型": "Auto-configure models",
  "把这台电脑的专属 Key 写进已装工具": "Write this PC's dedicated key into installed tools",
  "一键配好": "Configure in one click",
  "充值开通 AI": "Top up to activate AI",
  "¥20起充 · 到账后点刷新余额确认 · 不用不扣": "From ¥20 · click Refresh balance once it arrives · pay only for what you use",
  "充值后即可聊天、写代码、画图": "After topping up you can chat, write code, and generate images",
  "去充值": "Top up",
  "开始用 AI 还差几步": "A few steps to start using AI",
  "装工具、配模型全免费；充值只用于调用 AI，余额永久有效、不用不扣。":
    "Installing tools and configuring models is completely free. Top-ups are only for calling AI; your balance never expires and you pay only for what you use.",
  "已完成": "Done",
  "上一步完成后解锁": "Unlocks after the previous step",

  // ---- MyAI ----
  "主力推荐": "Recommended",
  "干活利落·推荐": "Best for work · Recommended",
  "启动 Hermes 终端（推荐）": "Launch Hermes terminal (recommended)",
  "小白首选": "Best for beginners",
  "干活最利落": "Best for work",
  "装好就是聊天窗口，不用敲命令": "Once installed it's just a chat window — no commands",
  "Hermes 终端 AI 助手": "Hermes terminal AI assistant",
  "命令行智能体，复杂活干得更利落": "A command-line agent — handles complex work more cleanly",
  "打开 Hermes": "Open Hermes",
  "一键安装 Hermes": "One-click install Hermes",
  "ClawX 图形版 AI 助手": "ClawX graphical AI assistant",
  "最适合小白：装好就是聊天窗口，不用敲命令": "Best for beginners: once installed it's just a chat window — no commands needed",
  "打开 ClawX": "Open ClawX",
  "一键安装 ClawX": "One-click install ClawX",
  "我装好的 AI 工具": "My installed AI tools",
  // 「重新安装 / 修复」改名（2026-08-20）：它一直**同时就是升级**，只是没人猜得到
  "升级 / 修复（重装到最新版）": "Upgrade / repair (reinstall latest)",
  "重新走一遍安装。装机清单里除 DSH 外都不锁版本，所以这一下同时就是**升级到最新版**；用不了、装坏了、或明明卸载了却还显示「已安装」时也点这里":
    "Runs the install again. Nothing in the install manifest is version-pinned except DSH, so this doubles as **upgrading to the latest**; also use it when a tool is broken, unusable, or still shows as “installed” after you removed it.",
  "复制内置 Key": "Copy built-in key",
  "正在生成体检报告…": "Generating health report…",
  "体检报告已生成到桌面：AI体检报告.txt（可截图发售后微信）":
    "Health report saved to desktop: AI体检报告.txt (screenshot it to send to support on WeChat)",
  "生成体检报告失败：": "Failed to generate health report: ",
  "生成一份「AI 体检报告」到桌面，装没装 / 驱动 / 余额一目了然，方便发给售后排查":
    "Generate an \"AI health report\" on the desktop showing install status / provider / balance at a glance — easy to send to support",
  "体检报告": "Health report",
  "在桌面创建 U-King 快捷方式": "Create a U-King shortcut on the desktop",
  "固定到桌面": "Pin to desktop",
  "还没装任何 AI 工具": "No AI tools installed yet",
  "点「一键全安装」自动装好全部工具 + 接好虾盘云，开箱即用":
    "Click \"Install everything\" to auto-install all tools and connect Xiapan Cloud — ready out of the box",
  "已安装": "Installed",
  "还没配模型": "No model configured yet",
  "打开应用": "Open app",
  "打开网页版": "Open web version",
  "打开终端": "Open terminal",
  "从开始菜单打开": "Open from Start menu",
  "单独给这个工具换模型（高级）": "Switch model for this tool only (advanced)",
  "还能装这些": "You can also install",

  // 首页最底的实验室折叠区（2026-07-27 做减法）
  "实验室 · 还在测试的工具": "Labs · tools still in testing",
  "这些不在「装好你的 AI」这条主线上，还在打磨。能用，但别当主力。":
    "These aren't part of the core \"get your AI set up\" flow and are still being polished. Usable, but don't rely on them.",
  "去看看": "Take a look",
  "一键安装": "One-click install",

  // ---- ToolMarket ----
  "AI 工具市场": "AI Tool Market",
  "按需安装，装好点「打开」即可用。": "Install what you need; once installed, click \"Open\" to use it.",
  "可一键安装": "One-click install",
  "官网指引": "Official site",
  "重新安装 / 更新": "Reinstall / Update",
  "已安装（从应用列表打开）": "Installed (open from app list)",
  "了解 / 安装": "Learn more / Install",

  // 「下载的绿色版点了没反应」= 单实例交棒，静默才是 bug（2026-08-18 客户反馈）
  "U-King 本来就开着（在托盘里），已经帮你切回来了。":
    "U-King was already running (in the tray) — switched you back to it.",
  "U-King 本来就开着（在托盘里），已经帮你切回来了 —— 你双击的那份「{name}」不会再开一个窗口。":
    "U-King was already running (in the tray) — switched you back to it. The copy you double-clicked ({name}) will not open a second window.",
  // ---- App: 启动 toast / 便携徽标（2026-09-06 重打：09-05 版随 rm -rf 事故丢失）----
  "{name} 启动失败：{msg}": "{name} failed to start: {msg}",
  "拉出终端窗口失败：{msg}": "Could not open the terminal window: {msg}",
  "已启动 {name}": "{name} started",
  "暂时无法启动": "Unable to start right now",
  "便携": "Portable",

  // 「并行调试实例」常驻条（--allow-multi-instance 起第二个 U-King 时）。见 src-tauri/src/instance.rs
  "这是并行调试实例（第二个 U-King）—— 界面、终端、工作目录跟第一个完全一样，但定时任务、技能包同步、Codex 代理自愈都由第一个负责，这里不重复跑；这里新建的任务和对话续接不会保存。":
    "This is a parallel debug instance (a second U-King). The UI, terminals and working folder are identical to the first one, but scheduled automations, skill-pack sync and Codex proxy self-heal are owned by the first instance and are not run twice here. Tasks and chat-resume ids created here are not saved.",
};
