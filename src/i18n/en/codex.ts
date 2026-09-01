/** 英文覆盖字典 · Codex 工作站 + AI 优化大师（CodexZone/AiRuntime）。 */
export const codex: Record<string, string> = {
  // ── 通用（跨模块复用，本模块自带一份保证独立可用）─────────────
  复制: "Copy",
  "安装中…": "Installing…",
  已安装: "Installed",
  已装: "Installed",
  未装: "Not installed",
  刷新: "Refresh",
  检测中: "Checking",
  重试: "Retry",
  实测: "Measured",

  // ── CodexZone.tsx · 复制/剪贴板 ──────────────────────────────
  已复制到剪贴板: "Copied to clipboard",
  "复制失败，请手动选中文本复制": "Copy failed — please select the text and copy manually",

  // ── CodexZone.tsx · DeepSeek 省钱路由 ─────────────────────────
  "已关 DeepSeek 路由 · Codex 回到贵的 gpt-5.x-codex（重启 Codex 生效）":
    "DeepSeek route off · Codex is back on the pricey gpt-5.x-codex (restart Codex to take effect)",
  "已开 DeepSeek 省钱路由 · 重启 Codex 生效":
    "DeepSeek money-saving route on · restart Codex to take effect",
  "切换失败: {e}": "Switch failed: {e}",
  "DeepSeek 省钱路由": "DeepSeek money-saving route",
  "已开 · 省钱": "On · saves money",
  "已关 · 用贵的 GPT": "Off · using pricey GPT",
  "开 = Codex 走本地代理接 ": "On = Codex routes through a local proxy to ",
  "（": " (",
  便宜几十倍: "tens of times cheaper",
  "）；关 = Codex 直连海外 ": "); Off = Codex connects directly to overseas ",
  "贵约 200 倍": "~200× more expensive",
  "）。切换后": "). After switching, ",
  "重启 Codex": "restart Codex",
  " 生效。": " to take effect.",
  "原理：Codex 只认 responses API，本地代理把它转成 DeepSeek 的 chat。":
    "How it works: Codex only speaks the responses API, and the local proxy converts it into DeepSeek's chat API. ",
  "只有 Codex 需要": "Only Codex needs this",
  "——Claude / Hermes / U-King 助手本来就用便宜的 DeepSeek。":
    " — Claude / Hermes / the U-King assistant already use the cheap DeepSeek.",
  "关闭路由（回贵的）": "Turn off route (back to pricey)",
  开启省钱路由: "Turn on money-saving route",
  "配虾盘云时省钱路由": "When connecting Xiapan Cloud, the money-saving route is ",
  默认开启: "on by default",
  "。确需 GPT-5.3-Codex 等海外模型：在「AI 设置 → Codex」自选模型（按量计费，较贵），或联系客服微信 ":
    ". If you really need overseas models like GPT-5.3-Codex: pick the model yourself in “AI Settings → Codex” (pay-as-you-go, pricey), or contact support on WeChat ",
  " 单独对接。": " for a dedicated plan.",

  // ── CodexZone.tsx · 接虾盘云（toast/错误映射）─────────────────
  "配置已写入，但连通测试失败：{err}": "Config written, but the connectivity test failed: {err}",
  "请检查余额/网络": "please check balance / network",
  "已把 Codex 接到虾盘云，并通过 responses 实测":
    "Codex is now connected to Xiapan Cloud and verified live via responses",
  "接入失败：虾盘云余额不足，请去「② 虾盘云·充值」补充余额后再试":
    "Connection failed: Xiapan Cloud balance is too low — top up under “② Xiapan Cloud · Top up” and try again",
  "接入失败：Key 暂时不可用，请先充值后刷新；仍不行再重新接入虾盘云驱动":
    "Connection failed: the key is temporarily unavailable — top up, refresh, and if it still fails, reconnect the Xiapan Cloud driver",
  "接入失败：连接超时，检查下网络再试一次":
    "Connection failed: request timed out — check your network and try again",
  "接入失败：网络连不上，检查下网络再试一次":
    "Connection failed: network unreachable — check your network and try again",
  "接入失败，可去「AI 设置」页手动配置 Codex":
    "Connection failed — you can configure Codex manually on the “AI Settings” page",

  // ── CodexZone.tsx · 安装日志前缀 ─────────────────────────────
  修复: "Repair",
  验证: "Verify",
  安装: "Install",
  "{prefix}：{line}": "{prefix}: {line}",

  // ── CodexZone.tsx · 安装桌面版（toast）───────────────────────
  "Mac 版 Codex 请下载官方 DMG，拖到 Applications 后回 U-King 重新检测":
    "For the Mac version of Codex, download the official DMG, drag it into Applications, then return to U-King and re-detect",
  "开始安装 Codex 桌面版…": "Starting Codex Desktop installation…",
  "自动安装没有完成，请按手动教程安装": "Automatic installation didn't finish — please follow the manual guide",
  "Codex 自动安装未完成，已显示手动安装入口":
    "Codex auto-install didn't finish — the manual install option is now shown",
  "Codex 桌面版安装完成，并已自动接入虾盘云；余额不足时充值即可用":
    "Codex Desktop is installed and auto-connected to Xiapan Cloud; if the balance is low, just top up to use it",
  "Codex 自动安装失败，已显示手动安装入口":
    "Codex auto-install failed — the manual install option is now shown",

  // ── CodexZone.tsx · 就绪态 / 交付四步 ────────────────────────
  已就绪: "Ready",
  还差一步: "One step to go",
  待配置: "Not configured",
  "安装 Codex": "Install Codex",
  "已安装 {ver}": "Installed {ver}",
  已检测到桌面版: "Desktop app detected",
  一键安装或打开教程: "One-click install or open the guide",
  "打开 Codex": "Open Codex",
  一键安装: "One-click install",
  安装教程: "Install guide",
  接虾盘云: "Connect Xiapan Cloud",
  "已写入 U-King 管理配置": "Written to the U-King-managed config",
  "写入 Base URL、模型和设备 Key": "Writes the Base URL, model, and device key",
  换模型: "Switch model",
  一键接入: "One-click connect",
  验证能用: "Verify it works",
  "接入时已做 responses 连通测试": "A responses connectivity test ran during connection",
  接入后会自动实测一次: "A live test runs automatically after connecting",
  刷新状态: "Refresh status",
  复制模板开工: "Copy a template and start",
  "修 bug、做网页、多 Agent、交付自检": "Fix bugs, build web pages, multi-agent, delivery self-check",
  看模板: "View templates",

  // ── CodexZone.tsx · 首屏 / 状态卡 ────────────────────────────
  "Codex 工作站": "Codex Station",
  "给客户用的一站式 Codex 入口：安装桌面版、接入虾盘云、复制任务模板、查看 computer use 教程。":
    "A one-stop Codex hub for customers: install the desktop app, connect Xiapan Cloud, copy task templates, and view the computer-use guide.",
  "一键安装 Codex": "One-click install Codex",
  管理模型: "Manage models",
  一键接虾盘云: "One-click connect Xiapan Cloud",
  任务模板库: "Task template library",
  待安装: "Not installed",
  驱动: "Driver",
  已接入: "Connected",
  待接入: "Not connected",
  "Codex 状态": "Codex status",
  "Codex 桌面版": "Codex Desktop",
  "打开 Mac 安装教程": "Open the Mac install guide",
  "商店在后台慢慢装（可能要几分钟）。": "The Store installs it in the background (may take a few minutes). ",
  "DMG 拖进 Applications 后，": "After dragging the DMG into Applications, ",
  装好后: "once it's installed, ",
  "关掉 U-King 再打开": "close U-King and reopen it",
  "，这里就会变「已装」。": " and this will change to “Installed”.",
  "自动装不上？点这里手动安装": "Auto-install not working? Click here to install manually",
  自动安装没完成: "Auto-install didn't finish",
  打开手动安装教程: "Open the manual install guide",
  虾盘云驱动: "Xiapan Cloud driver",
  已接管: "Managed",
  未接管: "Not managed",
  "换模型 / 换驱动": "Switch model / driver",
  "接入中…": "Connecting…",

  // ── CodexZone.tsx · 交付流程 / 安全配置 ──────────────────────
  "Codex 交付流程": "Codex delivery workflow",
  "装机、接驱动、验证、开工都在这一页完成，原 U-King 其他入口继续保留。":
    "Install, connect the driver, verify, and start — all on this page; the other U-King entries remain available.",
  完成: "Done",
  待处理: "Pending",
  安全配置规则: "Security configuration rules",
  "默认用 U-King 设备 Key 接虾盘云，不让客户把私密 Key 发到聊天窗口。客户自有 Key 只在本机 AI 设置页输入。":
    "By default it connects to Xiapan Cloud with the U-King device key, so customers never send private keys into a chat window. A customer's own key is entered only on the local AI Settings page.",
  "客户只点 Codex 工作站，不需要找 bat 或配置文件。":
    "Customers only click Codex Station — no need to hunt for bat files or config files.",
  "先安装 Codex，再接虾盘云，最后用模板开工。":
    "Install Codex first, then connect Xiapan Cloud, then start with a template.",
  "API Key 只在本机配置页输入，不发微信、不发聊天窗口。":
    "Enter the API key only on the local config page — never over WeChat or a chat window.",
  "安装失败时留在当前页，点手动安装教程继续。":
    "If installation fails, stay on this page and continue via the manual install guide.",
  "自动安装不成功时，走手动安装最稳": "If auto-install fails, the manual install is the most reliable",
  "Windows 有些电脑没有 winget、商店后台注册很慢，或被杀毒/公司策略拦住；Mac 需要下载 DMG 后拖到 Applications。教程里给了官方入口和国内兜底，成功任意一条即可。":
    "Some Windows PCs lack winget, have slow Store background registration, or are blocked by antivirus / corporate policy; Mac requires downloading the DMG and dragging it into Applications. The guide provides both official and China-mainland fallback links — any one that works is enough.",

  // ── CodexZone.tsx · computer use 教程 ────────────────────────
  "computer use 怎么用（让 Codex 操作浏览器 / 电脑）":
    "How to use computer use (let Codex control the browser / computer)",
  "装好 ": "Install ",
  "（上方状态卡一键装），并 ": " (one-click via the status card above), and ",
  接虾盘云驱动: "connect the Xiapan Cloud driver",
  "（computer use 需要支持工具调用的上游，虾盘云支持）。":
    " (computer use needs an upstream that supports tool calling, which Xiapan Cloud does).",
  "打开 Codex，在左侧找 ": "Open Codex, find ",
  "「自动化」": "“Automations”",
  "（英文界面叫 Automations），选 ": " on the left (called Automations in English), pick ",
  "「@电脑」": "“@computer”",
  "（@computer）发一条任务；首次它会自动下载私有运行时（约几十 MB，需联网，稍等一会）。":
    " and send a task; the first time it auto-downloads a private runtime (tens of MB, needs internet, wait a moment).",
  下载完就能让它: "Once downloaded, you can have it ",
  "操作浏览器、点按键盘鼠标": "control the browser and click the keyboard and mouse",
  "了。装完没反应多半是运行时还在下，等一会或重发一次任务即可。":
    ". If nothing happens after install, the runtime is probably still downloading — wait a bit or resend the task.",
  "看完整图文教程（含商店安装 / 重启识别）":
    "See the full illustrated guide (incl. Store install / restart-to-detect)",

  // ── CodexZone.tsx · 任务生成器 / 模板库 ──────────────────────
  任务生成器: "Task generator",
  不会写提示词也能用: "Usable even if you can't write prompts",
  "选一个场景，回答几个问题、填一句话，自动生成 Codex 能直接执行的标准任务，一键复制粘贴进去即可。":
    "Pick a scenario, answer a few questions, fill in one sentence, and it auto-generates a standard task Codex can run directly — just copy and paste it in.",
  先想清楚这几点: "Think these through first",
  "生成的任务（复制进 Codex）": "Generated task (copy into Codex)",
  "上面还没填内容，现在复制会带示例占位；填一句你的真实需求会更准。":
    "You haven't filled anything in yet — copying now includes a sample placeholder; entering one real request makes it more accurate.",
  "{n} 个标准任务话术，重点是目标、约束和验收，不让 Codex 猜。":
    "{n} standard task scripts focused on goals, constraints, and acceptance — so Codex doesn't have to guess.",
  全部复制: "Copy all",
  用它生成: "Use this to generate",
  "安装 Codex 桌面版": "Install Codex Desktop",
  "图形界面，内置 computer use。Windows 一键安装成功后会自动接虾盘云驱动；Mac 走官方 DMG，装好后点一键接入即可。":
    "A graphical app with built-in computer use. On Windows, a successful one-click install auto-connects the Xiapan Cloud driver; on Mac, use the official DMG and click one-click connect after installing.",
  "装不上？手动安装教程": "Can't install? Manual install guide",
  可一键安装: "One-click installable",
  手动安装: "Manual install",
  打开应用: "Open app",
  打开终端: "Open terminal",
  "高级配置 · 本地模型 / 自定义驱动": "Advanced config · local models / custom driver",
  "用自己显卡离线跑开源模型（codex --oss），或接你自己的 OpenAI 兼容 API":
    "Run open-source models offline on your own GPU (codex --oss), or connect your own OpenAI-compatible API",

  // ── CodexZone.tsx · 模板分类 ────────────────────────────────
  全部: "All",
  代码: "Code",
  排错: "Debugging",
  测试: "Testing",
  前端: "Frontend",
  文档: "Docs",
  初始化: "Init",
  规划: "Planning",
  计划: "Plan",
  提示词: "Prompts",
  交付: "Delivery",

  // ── CodexZone.tsx · 模板标题 ────────────────────────────────
  接手陌生项目先分析: "Analyze an unfamiliar project first",
  实现一个功能: "Implement a feature",
  修复报错: "Fix an error",
  代码审查: "Code review",
  补测试用例: "Add test cases",
  优化界面可读性: "Improve UI readability",
  "README / 教程重写": "Rewrite README / guide",
  新项目初始化: "Initialize a new project",
  把想法变执行计划: "Turn an idea into a plan",
  "多 Agent 分工": "Multi-agent division of labor",
  把提示词改得更稳: "Make a prompt steadier",
  交付前检查: "Pre-delivery check",

  // ── CodexZone.tsx · 模板摘要 ────────────────────────────────
  "让 Codex 先读结构、启动方式、测试命令和风险，再动手，少误改。":
    "Have Codex first read the structure, how to run it, test commands, and risks before acting — fewer wrong edits.",
  "明确知道要加什么、但不确定改哪里时用。按最小改动执行。":
    "Use when you know what to add but not where to change it. Executes with minimal edits.",
  "把现象、复现步骤、日志交给 Codex，让它先定位根因再动手。":
    "Give Codex the symptom, repro steps, and logs so it finds the root cause before acting.",
  "按 bug、回归、安全、缺测试的顺序审查当前改动。":
    "Review the current changes in order of bugs, regressions, security, and missing tests.",
  "围绕正常路径、失败路径和最相关的回归风险补测试。":
    "Add tests around the happy path, failure paths, and the most relevant regression risks.",
  "按钮遮挡、文字太小、布局拥挤、小白看不懂时用。":
    "Use when buttons overlap, text is too small, the layout is cramped, or beginners can't follow it.",
  "把介绍、启动方式、交付步骤整理成客户看得懂的文档。":
    "Organize the intro, how to start, and delivery steps into docs customers can understand.",
  "从空文件夹建立结构、依赖、质量工具和说明。":
    "Build structure, dependencies, quality tools, and docs from an empty folder.",
  "把一句话需求拆成 P0/P1/P2 和今天能验收的最小任务。":
    "Break a one-line requirement into P0/P1/P2 and the smallest task you can ship today.",
  "大任务拆成互不冲突的并行工作，避免几个 Agent 改同一文件。":
    "Split a big task into non-conflicting parallel work so multiple agents don't edit the same file.",
  "把口语化、太长、太散的提示词整理成可复用模板。":
    "Turn a colloquial, overly long, or scattered prompt into a reusable template.",
  "发 U 盘或远程装机给客户前，让 Codex 帮忙查漏。":
    "Before shipping a USB drive or remote install to a customer, have Codex help catch gaps.",

  // ── CodexZone.tsx · 模板引导问题 ────────────────────────────
  "项目在哪个文件夹？": "Which folder is the project in?",
  "你最终想改成什么样？": "What do you ultimately want it to become?",
  "有没有绝对不能动的文件或功能？": "Are there files or features that must not be touched?",
  "用户从哪里进入这个功能？": "Where do users enter this feature?",
  "完成后界面或命令该出现什么？": "What should appear in the UI or command once it's done?",
  "怎么算成功？": "What counts as success?",
  "你点了哪里出的错？": "What did you click when the error occurred?",
  "实际报错是什么？": "What is the actual error message?",
  "以前正常还是一直不正常？": "Did it work before, or has it never worked?",
  "审查当前改动还是某个文件？": "Review the current changes or a specific file?",
  "重点看安全、性能还是界面？": "Focus on security, performance, or the UI?",
  "要不要给修复建议？": "Should it suggest fixes?",
  "要测哪个功能？": "Which feature should be tested?",
  "成功和失败场景分别是什么？": "What are the success and failure scenarios?",
  "现有测试命令是什么？": "What is the existing test command?",
  "哪一页看不懂？": "Which page is hard to understand?",
  "哪个按钮或文字有歧义？": "Which button or text is ambiguous?",
  "目标用户是谁？": "Who is the target user?",
  "文档给谁看？": "Who is the documentation for?",
  "用户第一步要做什么？": "What is the user's first step?",
  "哪些风险不能承诺？": "Which risks can't be promised away?",
  "项目类型是什么？": "What type of project is it?",
  "用什么技术栈？": "Which tech stack?",
  "需要哪些质量检查？": "Which quality checks are needed?",
  "你想做成什么？": "What do you want to build?",
  "给谁用？": "Who is it for?",
  "今天必须交付什么？": "What must be delivered today?",
  "总目标是什么？": "What is the overall goal?",
  "哪些文件可能被改？": "Which files might be changed?",
  "哪些子任务能并行？": "Which subtasks can run in parallel?",
  "原提示词是什么？": "What is the original prompt?",
  "要重复用在什么场景？": "In what scenarios will it be reused?",
  "希望输出什么格式？": "What output format do you want?",
  "交付给谁？": "Who is it delivered to?",
  "必须检查哪些入口？": "Which entry points must be checked?",
  "有没有不能对外承诺的话术？": "Any claims that shouldn't be promised externally?",

  // ── CodexZone.tsx · 模板填空小标题 ──────────────────────────
  我的目标: "My goal",
  功能目标: "Feature goal",
  现象: "Symptom",
  审查范围: "Review scope",
  测试目标: "Test goal",
  问题: "Problem",
  文档目标: "Docs goal",
  项目目标: "Project goal",
  我的想法: "My idea",
  目标: "Goal",
  原始提示词: "Original prompt",
  交付对象: "Delivery target",

  // ── CodexZone.tsx · 模板填空占位提示 ────────────────────────
  "写你接手这个项目后要完成的目标，例如：给这个后台加一个导出 Excel 的按钮。":
    "Write the goal you want to accomplish after taking over this project, e.g.: add an “Export to Excel” button to this admin panel.",
  "写清楚最终用户能做什么，例如：登录后能看到自己的订单列表并按时间排序。":
    "Spell out what the end user can do, e.g.: after logging in, see their own order list sorted by time.",
  "描述你看到了什么错误、在什么操作之后出现。日志里别带 API Key / 密码 / Token。":
    "Describe the error you saw and what action triggered it. Don't include API keys / passwords / tokens in logs.",
  "写要审查的分支、文件或功能，例如：这次提交对结算金额的改动。":
    "Write the branch, file, or feature to review, e.g.: this commit's changes to the settlement amount.",
  "写要覆盖的功能和风险，例如：优惠券金额计算，含过期和叠加两种边界。":
    "Write the feature and risks to cover, e.g.: coupon amount calculation, including the expiry and stacking edge cases.",
  "例如：设置页文字太小、按钮挤在一起、手机上重叠、重点不明显。":
    "E.g.: text on the settings page is too small, buttons are crowded together, they overlap on mobile, and the focus is unclear.",
  "写文档服务的对象和目的，例如：给不懂技术的客户看的上手说明。":
    "Write the audience and purpose of the docs, e.g.: getting-started instructions for non-technical customers.",
  "写要初始化的项目，例如：一个能记账并导出月报的本地小工具。":
    "Write the project to initialize, e.g.: a small local tool that tracks expenses and exports monthly reports.",
  "粘贴你原始的想法，越随意越好，Codex 会帮你理成计划。":
    "Paste your raw idea — the more casual the better; Codex will shape it into a plan.",
  "写这个大任务的最终结果。": "Write the end result of this big task.",
  "粘贴你现在用的提示词，哪怕很口语、很乱都行。":
    "Paste the prompt you currently use, even if it's colloquial and messy.",
  "写这次要交付的产品或目录，例如：给客户的 U 盘根目录。":
    "Write the product or directory to deliver, e.g.: the root of the customer's USB drive.",

  // ── AiRuntime.tsx · 仪表盘 / toast ──────────────────────────
  "AI 战斗力 / 100": "AI power / 100",
  "[环境组件] 检查 / 补装 Node、Git（含 Bash）与 PowerShell 7（首次需下载，请稍候）…":
    "[Environment] Checking / installing Node, Git (incl. Bash) and PowerShell 7 (first-time download, please wait)…",
  "[环境组件] 检查 / 补装 Node 与 Git（Apple 命令行开发者工具）…":
    "[Environment] Checking / installing Node and Git (Apple Command Line Developer Tools)…",
  "正在补装缺失的环境组件（Git / Node / PowerShell 7）…":
    "Installing missing environment components (Git / Node / PowerShell 7)…",
  "正在补装缺失的环境组件（Git / Node）…": "Installing missing environment components (Git / Node)…",
  "环境组件安装未完成：{e}": "Environment component installation didn't finish: {e}",
  "✔ 就绪": "✔ Ready",
  "— 跳过": "— Skipped",
  "[环境组件] ": "[Environment] ",
  "Node：{node}": "Node: {node}",
  "Git：{git}": "Git: {git}",
  "Git（含 Bash）：{git}": "Git (incl. Bash): {git}",
  "PowerShell 7：{pwsh}": "PowerShell 7: {pwsh}",
  "CLI 命令优先级：{guard}": "CLI command priority: {guard}",
  "优化完成：AI 战斗力 {before} → {after}（+{delta}）":
    "Optimization complete: AI power {before} → {after} (+{delta})",
  已是当前可优化的最佳状态: "Already at the best state that can be optimized",
  "优化失败：{e}": "Optimization failed: {e}",
  "已回滚最近一次优化（可连续点撤销更早的）":
    "Rolled back the most recent optimization (click again to undo earlier ones)",
  "回滚失败：{e}": "Rollback failed: {e}",
  "已加杀软白名单，npm/装机更快（可一键复原）":
    "Added to the antivirus allowlist — npm / installs are faster (one-click revert available)",
  "需要管理员权限：右键 U-King → 以管理员身份运行后重试":
    "Administrator rights required: right-click U-King → Run as administrator, then retry",
  "加白名单失败：{e}": "Adding to allowlist failed: {e}",
  "AI 优化大师": "AI Optimizer",
  优化引擎未就绪: "Optimization engine not ready",
  "正在扫描本机 AI 环境…（只读，不改任何配置）":
    "Scanning this machine's AI environment… (read-only, changes nothing)",
  "AI 时代的电脑优化大师 · 让 Claude / Codex 跑得更稳、更省":
    "The PC optimizer for the AI era · make Claude / Codex run steadier and cheaper",
  "已达标 ": "Met ",
  " 项 · 可优化 ": " items · optimizable ",
  " 项": " items",
  "· 还能再涨 ": "· can gain another ",
  "+{n} 分": "+{n} pts",
  "🏆 你的 AI 战斗力超过了全国 ": "🏆 Your AI power beats ",
  " 的电脑": " of PCs nationwide",
  "（基于已收集样本）": " (based on collected samples)",
  "正在优化…": "Optimizing…",
  已是最佳状态: "Already optimal",
  一键优化: "One-click optimize",
  "让 AI 给优化建议": "Ask AI for advice",
  "已把体检结果交给 AI，正在打开工作台": "Handed the health-check report to AI — opening the workspace",
  一键复原: "One-click revert",
  "把 AI 工具目录加进 Windows Defender 排除，npm/装机不再被实时扫描拖慢（需管理员，可一键复原）":
    "Add the AI tool directories to the Windows Defender exclusions so npm / installs are no longer slowed by real-time scanning (requires admin, one-click revert available)",
  "处理中…": "Processing…",
  "加杀软白名单·提速": "Add antivirus allowlist · speed up",
  重新体检: "Re-check",
  "🎉 优化完成！AI 战斗力 {from} → {to}": "🎉 Optimization complete! AI power {from} → {to}",
  "（+{delta} 分 · 修复 {fixed} 项）": " (+{delta} pts · fixed {fixed} items)",
  "配置改动已留底可「一键复原」；缺的 Git / Node 已自动补齐。还缺 Claude Code / Codex 的，用装机向导一键装。":
    "Config changes are backed up for one-click revert; missing Git / Node have been auto-installed. For missing Claude Code / Codex, use the install wizard's one-click install.",
  "配置改动已留底可「一键复原」；缺的 Node 已自动补齐，缺 Git 会弹 Apple「命令行开发者工具」安装窗。还缺 Claude Code / Codex 的，用装机向导一键装。":
    "Config changes are backed up for one-click revert; missing Node has been auto-installed, and a missing Git opens Apple's Command Line Developer Tools installer. For missing Claude Code / Codex, use the install wizard's one-click install.",

  // ── AiRuntime.tsx · 收益预估卡 ──────────────────────────────
  "省 Token 空间": "Token-saving room",
  "{n} 处浪费点": "{n} waste points",
  "权限往返 / 噪音输出 / 重复摸索": "Permission round-trips / noisy output / repeated fumbling",
  翻车风险: "Failure risk",
  "{n} 个隐患": "{n} hazards",
  "单次翻车 ≈ 上万 token + 几分钟等待": "One failure ≈ tens of thousands of tokens + a few minutes of waiting",
  修复方式: "How it's fixed",
  全部可回滚: "All reversible",
  "改前留底 · journal 记录 · 体检只读": "Backup before change · journal logging · read-only check",

  // ── AiRuntime.tsx · 可优化项 / 已达标 / 明细 ─────────────────
  "可优化项（{n}）": "Optimizable items ({n})",
  去装机向导安装: "Install via the wizard",
  "已达标（{n}）": "Met ({n})",
  本次改动明细: "This run's change details",
  "体检为只读操作 · 所有优化改前留底（~/.uking/{tool}/journal）· 「一键复原」按次回滚 · 引擎 {tool} v{ver}":
    "The check is read-only · every optimization is backed up before changes (~/.uking/{tool}/journal) · one-click revert rolls back per step · engine {tool} v{ver}",

  // ── AiRuntime.tsx · 收益标签（GAINS.tag）────────────────────
  省Token: "Save tokens",
  防翻车: "Prevent failures",
  提速: "Speed up",
  基础: "Basics",

  // ── AiRuntime.tsx · 收益说明（GAINS.note）───────────────────
  "实测：含 deprecated 依赖的 npm install 少喂 AI ~145 token/次（581 字节），大项目更多":
    "Measured: an npm install with deprecated deps feeds the AI ~145 fewer tokens per run (581 bytes), more for large projects",
  "实测：中文目录的 git status 转义多喂 ~70 token/次，且转义码 AI 读不懂→易重读":
    "Measured: git status in a Chinese directory feeds ~70 extra tokens per run from escaping, and the escape codes are unreadable to the AI → easy re-reads",
  "每拦 1 次权限确认 ≈ 多耗 1 轮 API 往返（数千 token · 估算）":
    "Each permission prompt ≈ one extra API round-trip (thousands of tokens · estimated)",
  "AI 免重复摸索环境，每个会话省数千 token（估算）":
    "The AI avoids re-probing the environment, saving thousands of tokens per session (estimated)",
  "长命令输出刷屏进上下文，单次省数百 token（估算）":
    "Long command output flooding the context; saves hundreds of tokens per run (estimated)",
  "GBK 乱码引发的重试，单次翻车 ≈ 上万 token + 3~10 分钟（估算）":
    "Retries caused by GBK garbling; one failure ≈ tens of thousands of tokens + 3–10 minutes (estimated)",
  "node_modules 深层路径翻车重试，单次 ≈ 上万 token（估算）":
    "Deep node_modules path failures and retries; one ≈ tens of thousands of tokens (estimated)",
  "Git 深路径报错 → AI 多轮试错（估算）": "Git deep-path errors → multiple AI trial-and-error rounds (estimated)",
  "pnpm/npm 建软链失败 → 安装类任务卡死重试（估算）":
    "pnpm/npm symlink creation fails → install tasks stall and retry (estimated)",
  "中文/空格用户目录是大量工具的翻车根源":
    "Chinese/space-containing user directories are a common failure root for many tools",
  "PATH 干净 = 命令解析更快更稳": "A clean PATH = faster, more stable command resolution",
  "PowerShell 7 的 UTF-8 支持远好于 5.1": "PowerShell 7's UTF-8 support is far better than 5.1",
  "Windows Terminal 渲染 UTF-8/emoji 不出豆腐块":
    "Windows Terminal renders UTF-8/emoji without tofu boxes",
  "Claude Code 必需组件": "A required component for Claude Code",
  "Claude Code 的 Bash 工具依赖": "A dependency of Claude Code's Bash tool",
  "AI CLI 工具运行时": "Runtime for AI CLI tools",
  用装机向导一键安装: "One-click install via the install wizard",

  // ── 模板正文（完整提示词，{{FILL}} 保留供替换）──────────────
  [`请先不要改文件。请阅读当前项目结构，找出技术栈、启动方式、关键目录、测试命令和潜在风险。

【我的目标】
{{FILL}}

请输出：
1. 项目结构和关键文件。
2. 运行、构建、测试命令。
3. 你建议先看的文件。
4. 实施前的风险和需要我确认的问题。
5. 信息足够的话，再给出下一步最小执行计划。`]: `Please don't modify any files yet. Read the current project structure and identify the tech stack, how to run it, key directories, test commands, and potential risks.

【My goal】
{{FILL}}

Please output:
1. The project structure and key files.
2. Run, build, and test commands.
3. Files you suggest reading first.
4. Risks before implementation and questions you need me to confirm.
5. If there's enough information, provide a minimal next-step execution plan.`,

  [`请在当前项目中实现这个功能。

【功能目标】
{{FILL}}

【验收标准】
1. 保持现有风格和交互一致。
2. 不改无关文件，不覆盖我未提交的改动。
3. 修改后运行相关测试；跑不了就说明原因。
4. 最后列出改了哪些文件、如何验证。

请先快速阅读相关代码，再直接实现并验证。`]: `Please implement this feature in the current project.

【Feature goal】
{{FILL}}

【Acceptance criteria】
1. Keep the existing style and interactions consistent.
2. Don't touch unrelated files or overwrite my uncommitted changes.
3. Run the relevant tests after changes; if they can't run, explain why.
4. Finally, list which files were changed and how to verify.

Please quickly read the relevant code first, then implement and verify directly.`,

  [`请帮我修复这个问题。

【现象】
{{FILL}}

【复现步骤】
1. 第一步。
2. 第二步。

请先定位根因，说明影响范围，再做最小必要修改并验证。不要粘贴或询问 API Key、密码、Token。`]: `Please help me fix this problem.

【Symptom】
{{FILL}}

【Repro steps】
1. Step one.
2. Step two.

Please locate the root cause first, explain the scope of impact, then make the minimal necessary change and verify. Don't paste or ask for API keys, passwords, or tokens.`,

  [`请用代码审查模式检查改动。

【审查范围】
{{FILL}}

请优先找：
1. bug 和行为回归。
2. 安全风险。
3. 缺少的测试。
4. 交互或文案可能误导用户的地方。

按严重程度排序，给出文件和行号。没问题也请明说，并列出剩余测试风险。`]: `Please review the changes in code-review mode.

【Review scope】
{{FILL}}

Please prioritize finding:
1. Bugs and behavior regressions.
2. Security risks.
3. Missing tests.
4. Interactions or wording that could mislead users.

Sort by severity and give file names and line numbers. If there are no issues, say so explicitly, and list the remaining testing risks.`,

  [`请为当前项目补充测试。

【测试目标】
{{FILL}}

请先查看现有测试框架和命令，再补最小必要测试，覆盖：
1. 正常路径。
2. 失败或边界路径。
3. 与本次改动最相关的回归风险。

完成后运行测试并说明结果。`]: `Please add tests to the current project.

【Test goal】
{{FILL}}

Please first check the existing test framework and commands, then add the minimal necessary tests covering:
1. The happy path.
2. Failure or edge paths.
3. The regression risks most relevant to this change.

Run the tests when done and report the results.`,

  [`请优化当前界面的可读性和操作路径。

【问题】
{{FILL}}

【要求】
1. 保持现有产品风格。
2. 不新增无关功能。
3. 让第一次用的小白知道下一步点哪。
4. 桌面和手机端布局都要正常。
5. 改完用截图或测试说明验证结果。`]: `Please improve the readability and interaction flow of the current UI.

【Problem】
{{FILL}}

【Requirements】
1. Keep the existing product style.
2. Don't add unrelated features.
3. Make sure a first-time beginner knows where to click next.
4. Both desktop and mobile layouts must work.
5. After changes, verify the result with a screenshot or test notes.`,

  [`请重写或补充项目文档。

【文档目标】
{{FILL}}

请输出适合客户或团队阅读的版本，包含：
1. 这个项目是什么。
2. 第一次怎么启动。
3. 常用功能怎么用。
4. 常见问题。
5. 不能承诺或需要注意的边界。

别写空泛宣传词，优先写能照着做的步骤。`]: `Please rewrite or extend the project documentation.

【Docs goal】
{{FILL}}

Please output a version suitable for customers or the team to read, including:
1. What this project is.
2. How to start it for the first time.
3. How to use common features.
4. FAQ.
5. Boundaries that can't be promised or need caution.

Don't write vague marketing copy — prioritize steps people can follow.`,

  [`请帮我初始化一个新项目。

【项目目标】
{{FILL}}

请先确认当前目录安全，再建立：
1. 清晰的目录结构。
2. 必要依赖和运行脚本。
3. 基础测试或质量检查。
4. README 启动说明。
5. 后续开发建议。

别引入过重技术栈，优先项目实际需要的最小方案。`]: `Please help me initialize a new project.

【Project goal】
{{FILL}}

Please confirm the current directory is safe first, then set up:
1. A clear directory structure.
2. Necessary dependencies and run scripts.
3. Basic tests or quality checks.
4. README startup instructions.
5. Suggestions for further development.

Don't bring in an overly heavy tech stack — prioritize the minimal solution the project actually needs.`,

  [`请把下面的想法整理成可执行计划。

【我的想法】
{{FILL}}

请输出：
1. 最终目标。
2. 今天必须完成的 P0。
3. 可后续做的 P1/P2。
4. 分阶段执行步骤。
5. 每阶段验收标准。
6. 风险和需要我确认的问题。

信息足够的话，请直接开始第一阶段。`]: `Please organize the idea below into an executable plan.

【My idea】
{{FILL}}

Please output:
1. The final goal.
2. The P0 that must be done today.
3. P1/P2 that can follow later.
4. Phased execution steps.
5. Acceptance criteria for each phase.
6. Risks and questions you need me to confirm.

If there's enough information, please start the first phase directly.`,

  [`这是一个较大的任务，请先判断是否适合多 Agent 并行。

【目标】
{{FILL}}

【约束】
1. 不改无关文件。
2. 每个 Agent 的写入范围必须互不重叠。
3. 先做探索和分工，再执行。

请输出：
1. 哪些子任务适合并行。
2. 每个 Agent 的职责和文件范围。
3. 主 Agent 需保留的关键路径工作。
4. 合并与验证步骤。`]: `This is a fairly large task; first judge whether it suits multi-agent parallelism.

【Goal】
{{FILL}}

【Constraints】
1. Don't touch unrelated files.
2. Each agent's write scope must not overlap.
3. Do exploration and division of labor first, then execute.

Please output:
1. Which subtasks suit parallel work.
2. Each agent's responsibilities and file scope.
3. The critical-path work the main agent should keep.
4. Merge and verification steps.`,

  [`请把下面的原始提示词精炼成可复用模板。

【原始提示词】
{{FILL}}

请输出：
1. 适用场景。
2. 精炼后的提示词。
3. 可替换的变量。
4. 使用示例。
5. 不适用或需人工确认的边界。

要让 Codex 拿到就能直接执行，不要变成空泛鸡汤。`]: `Please refine the raw prompt below into a reusable template.

【Original prompt】
{{FILL}}

Please output:
1. Applicable scenarios.
2. The refined prompt.
3. Replaceable variables.
4. A usage example.
5. Boundaries where it doesn't apply or needs human confirmation.

Make it so Codex can execute it directly on receipt — don't turn it into vague fluff.`,

  [`请做交付前检查。

【交付对象】
{{FILL}}

请检查：
1. 必需文件是否存在。
2. 是否有 API Key、密码、令牌被写进模板 / 文档 / 报告 / 日志。
3. 启动入口是否可用。
4. 自检脚本是否通过。
5. 客户首次使用步骤是否清楚。
6. 哪些功能不能对外承诺为已完成。

能直接修的先修，列出仍需人工确认的事项。`]: `Please run a pre-delivery check.

【Delivery target】
{{FILL}}

Please check:
1. Whether the required files exist.
2. Whether any API keys, passwords, or tokens were written into templates / docs / reports / logs.
3. Whether the launch entry points work.
4. Whether the self-check scripts pass.
5. Whether the customer's first-use steps are clear.
6. Which features can't be promised externally as complete.

Fix what you can directly, and list the items that still need human confirmation.`,
};
