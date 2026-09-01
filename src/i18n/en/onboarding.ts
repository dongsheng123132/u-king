/** 英文覆盖字典 · 新手引导 + 装机向导（Guide.tsx / Wizard.tsx）。 */
export const onboarding: Record<string, string> = {
  // ---------------- Guide.tsx ----------------

  // CodeBlock / copy toasts
  "已复制": "Copied",
  "复制失败": "Copy failed",
  "复制": "Copy",
  "已复制配置串 —— 粘给任何 AI 即可": "Config text copied — just paste it to any AI",

  // Top recharge section
  "虾盘云充值 · 免费装工具，充值用 AI": "Xiapan Cloud top-up · Install tools free, top up to use AI",
  "U-King 会给这台电脑生成专属 Key。你不用注册国外账号，也不用填信用卡；充值后这个 Key 就能在 ClawX、Claude Code、Codex 里调用大模型。":
    "U-King generates a dedicated key for this computer. No overseas account, no credit card needed; once topped up, this key can call large models in ClawX, Claude Code and Codex.",
  "当前状态": "Current status",
  "已开通": "Activated",
  "待充值开通": "Top up to activate",
  "正在生成 Key": "Generating key",
  "余额 {bal}": "Balance {bal}",
  "可用": "available",
  "充值后即可使用 AI": "Top up to start using AI",
  "生成专属 Key": "Generate dedicated key",
  "已绑定本机": "Bound to this device",
  "稍等几秒自动生成": "Auto-generated in a few seconds",
  "微信/支付宝充值": "Top up via WeChat / Alipay",
  "余额已可用": "Balance ready",
  "¥20 起充，到账即时": "From ¥20, credited instantly",
  "开始用 AI": "Start using AI",
  "聊天、写代码、画图、视频": "Chat, code, image generation, video",
  "检测中…": "Detecting…",
  "余额": "Balance",
  "继续充值": "Top up more",
  "去充值开通": "Top up to activate",
  "充值后点这里确认是否到账": "After topping up, click here to confirm it arrived",
  "刷新余额": "Refresh balance",

  // Two big actions
  "充值后怎么用": "How to use after topping up",
  "小白点「自动配好」；会折腾的再看复制配置": "Beginners: click “Auto-configure”; power users: see “Copy config”",
  "推荐": "Recommended",
  "① 自动配好已装工具": "① Auto-configure installed tools",
  "自动探测已装的 Claude Code / Codex / ClawX / OpenClaw / Hermes，把 Key 和模型一次写好。":
    "Auto-detects installed Claude Code / Codex / ClawX / OpenClaw / Hermes and writes the key and models in one go.",
  "一段文字": "A block of text",
  "② 复制配置文档": "② Copy config document",
  "进阶": "Advanced",
  "点一下会": "One click ",
  "复制一段说明文字": "copies a block of instructions",
  "到剪贴板——里面是地址+Key+全部模型。粘到任意 AI 对话框（ClawX / 聊天框），它照着自己配。":
    " to the clipboard — it contains the URL + key + all models. Paste it into any AI chat box (ClawX / a chat window) and it configures itself.",
  "# 虾盘云配置 · 地址 https://api.u-claw.org.cn/v1 · Key sk-… · 模型 deepseek-v4-flash …":
    "# Xiapan Cloud config · URL https://api.u-claw.org.cn/v1 · Key sk-… · Model deepseek-v4-flash …",

  // Relay-station section
  "下面是进阶手动配置，普通用户不用看": "Below is advanced manual setup — regular users can skip it",
  "这台电脑现在就是一个「中转站」": "This computer is now a “relay station”",
  "——不用挂 VPN、不用注册国外账号。把下面的 ": " — no VPN, no overseas account needed. Copy the ",
  "API 地址 + Key + 模型 ID": "API URL + key + model ID",
  "复制到": "into ",
  "任何支持「自定义模型 / OpenAI 兼容」的 AI 工具": "any AI tool that supports “custom model / OpenAI-compatible”",
  "（不只是 Claude Code / Codex / ClawX，沉浸式翻译、Cursor 之类第三方 AI 应用同样能填），就能直接用上 GPT-5.4、Claude、Gemini 这些海外模型。":
    " (not just Claude Code / Codex / ClawX — third-party AI apps like Immersive Translate or Cursor work too), and you can directly use overseas models like GPT-5.4, Claude and Gemini.",

  // API URL section
  "① API 地址（填到工具的 Base URL）": "① API URL (put in the tool’s Base URL)",
  "通用地址 · OpenAI 标准（多数工具 / Codex / ClawX）": "Universal URL · OpenAI standard (most tools / Codex / ClawX)",
  "Claude 专用地址（Claude Code 用）": "Claude-specific URL (for Claude Code)",

  // Hot models section
  "② 热门模型（填到工具的「模型名」）": "② Popular models (put in the tool’s “model name”)",
  "已复制模型名 {id}": "Copied model name {id}",
  "点击复制模型名": "Click to copy model name",
  "展开全部模型清单（聊天 + 画图，点击即复制 id）": "Expand the full model list (chat + image, click to copy id)",
  "画图模型（AI 作图用）": "Image models (for AI image generation)",
  // 热门模型标签（HOT_MODELS，Guide.tsx 本地数据，经 t(m.tag) 渲染）
  "最快最省": "Fastest & cheapest",
  "编程之王": "Best for coding",
  "OpenAI 旗舰": "OpenAI flagship",
  "谷歌": "Google",
  "超长上下文": "Ultra-long context",
  "画图": "Image gen",
  // 模型分组名（lib/models 数据，经 t(g.group) 渲染）
  "性价比之选": "Value picks",
  "全球旗舰（更聪明，更费额度）": "Global flagships (smarter, more tokens)",

  // Integration examples
  "③ 各平台接入示例": "③ Integration examples per platform",
  "Claude Code（命令行）": "Claude Code (command line)",
  "提示：U-King「虾盘云·充值」里点「自动配好」即可写入 ClawX / OpenClaw，不用手动填。":
    "Tip: in U-King’s “Xiapan Cloud · Top up”, click “Auto-configure” to write it into ClawX / OpenClaw — no manual entry needed.",
  "Codex（~/.codex/config.toml）": "Codex (~/.codex/config.toml)",
  "ClawX / 其它 OpenAI 兼容工具": "ClawX / other OpenAI-compatible tools",
  "添加供应商 → 接入模式选「纯 API / OpenAI 兼容」→ Base URL 填":
    "Add a provider → choose “API only / OpenAI-compatible” mode → set Base URL to",
  "→ API Key 填你的 Key（上方一键复制）→ 模型填上面任一热门模型 → 保存。":
    "→ set API Key to your key (one-click copy above) → set the model to any popular model above → Save.",
  "全部模型与价格 · 官网": "All models and pricing · Website",
  "余额永久有效，按量计费，不用不扣": "Balance never expires, pay-as-you-go, no usage no charge",

  // ---------------- Wizard.tsx ----------------

  // Tool names (TOOL_NAMES data, rendered via t())
  "Codex 桌面版": "Codex Desktop",
  "OpenClaw CLI（原版）": "OpenClaw CLI (original)",
  "Claude 桌面版": "Claude Desktop",

  // runFlow
  "你好，我是 U-King AI 管家 👋 我来帮你把 AI 编程工具装到这台电脑，并接上国内可用的大模型驱动。先给电脑做个体检…":
    "Hi, I’m the U-King AI butler 👋 I’ll help you install AI coding tools on this computer and hook up a large-model provider that works in China. Let me run a check first…",
  "体检失败了，请重开窗口再试。": "The check failed. Please reopen the window and try again.",

  // pickTools — one-click full install
  "好嘞，开始一键全安装 👇 ": "Alright, starting the one-click full install 👇 ",
  "先帮你装好主力工具「图形版 ClawX」（点开就能聊），再":
    "First I’ll install the main tool “ClawX (GUI)” (chat right away), then ",
  "依次": "then ",
  "依次装 Claude Code、Codex（命令行版 + 桌面版）、Hermes、OpenClaw，":
    "install Claude Code, Codex (CLI + desktop), Hermes and OpenClaw in turn, ",
  "全程走国内加速 + 自动验证修复。装完自动接好虾盘云驱动，并打通「AI 之间互相调用」，开箱即用。":
    "using domestic acceleration + auto verify & repair throughout. Once done it auto-connects the Xiapan Cloud provider and enables “AIs calling each other” — ready out of the box.",
  "一键全安装": "One-click full install",
  "装 Codex（命令行 + 桌面版）": "Install Codex (CLI + desktop)",
  "装 Codex 桌面版": "Install Codex Desktop",
  "装 Codex 命令行版": "Install Codex CLI",
  "装 Claude Code": "Install Claude Code",
  "两个都装": "Install both",
  "跳过安装，直接配驱动": "Skip install, configure provider",
  "你在工具市场点了 {tool}，现在就装它？": "You clicked {tool} in the tool market — install it now?",
  "安装 {tool}": "Install {tool}",
  "我再想想，看看其他选项": "Let me think — show other options",
  "Claude Code 和 Codex 都已经在了，直接进入驱动配置（改底层 API 指向国内）。":
    "Claude Code and Codex are both installed. Going straight to provider setup (point the underlying API to China).",
  "Claude Code 和 Codex CLI 都在了。还可以装 Codex 桌面版（图形界面，跟 CLI 共用同一份驱动配置），或直接配驱动。":
    "Claude Code and Codex CLI are both installed. You can also install Codex Desktop (GUI, shares the same provider config as the CLI), or configure the provider directly.",
  "跳过，直接配驱动": "Skip, configure provider",
  "直接配驱动": "Configure provider directly",
  "想装哪个？我推荐 Claude Code（最强编程 agent），装好后用国内驱动直连，不用翻墙。":
    "Which do you want to install? I recommend Claude Code (the strongest coding agent); once installed, connect directly via a domestic provider — no VPN needed.",
  "装 {tool}": "Install {tool}",

  // manualCodexApp
  "没关系，我已经为你打开「Codex 手动安装教程」网页 📖，照着点几下就能装好。也可以直接用下面的按钮 👇\n\n【方式一·推荐】微软商店：点「打开微软商店」→ 在商店页点【获取 / 安装】，等进度跑完。\n【方式二】商店打不开就点「商店网页版」，用浏览器登录微软账号后点【获取】。\n【方式三】点「下载安装包」直接下安装包（约 664MB），下完在浏览器「下载」里双击它装。\n\n⚠️ 微软商店是后台慢慢装的：进度条跑到 100% 才算装好，刚点完可能这里还检测不到，属正常。":
    "No worries — I’ve opened the “Codex manual install guide” page 📖 for you; a few clicks and it’s done. You can also use the buttons below 👇\n\n[Option 1 · Recommended] Microsoft Store: click “Open Microsoft Store” → on the store page click [Get / Install] and wait for it to finish.\n[Option 2] If the store won’t open, click “Store web version”, sign in to your Microsoft account in the browser, then click [Get].\n[Option 3] Click “Download installer” to download the package directly (about 664MB); when done, double-click it in the browser’s “Downloads” to install.\n\n⚠️ The Microsoft Store installs in the background: it’s only done when the progress bar hits 100%. Right after clicking it may not be detected here yet — that’s normal.",
  "打开微软商店 ⭐": "Open Microsoft Store ⭐",
  "商店网页版": "Store web version",
  "下载安装包（664MB）": "Download installer (664MB)",
  "看教程网页": "View guide page",
  "装好了，重新检测": "Installed — re-check",
  "打开微软商店": "Open Microsoft Store",
  "已为你拉起微软商店。在商店页点【获取 / 安装】，等进度条 100% 装完再回来点「装好了，重新检测」。":
    "Opened the Microsoft Store for you. On the store page click [Get / Install]; once the progress bar reaches 100%, come back and click “Installed — re-check”.",
  "已打开微软商店网页版，点【获取】即可。装完回来点「装好了，重新检测」。":
    "Opened the Microsoft Store web version — just click [Get]. When done, come back and click “Installed — re-check”.",
  "下载安装包": "Download installer",
  "已开始下载 Codex 安装包（.msix，约 664MB）。下完到浏览器「下载」里双击它，按提示装好后回来点「装好了，重新检测」。":
    "Started downloading the Codex installer (.msix, about 664MB). When done, double-click it in the browser’s “Downloads”, follow the prompts to install, then come back and click “Installed — re-check”.",
  "已重新打开教程网页。": "Reopened the guide page.",
  "✅ 检测到 Codex 桌面版已装好，继续帮你配驱动。":
    "✅ Detected Codex Desktop is installed. Continuing with provider setup.",
  "我再等等，重新检测": "Wait a bit more, re-check",
  "知道了，先跳过": "Got it, skip for now",
  "先跳过": "Skip for now",
  "好。如果商店里 Codex 已经装完了，这里还没认出来，多半是需要重启程序刷新 —— 把 U-King 整个关掉（右下角托盘图标 → 退出），再双击桌面/U盘里的 U-King.exe 重新打开，就能识别 Codex 了。":
    "OK. If Codex has finished installing in the store but isn’t recognized here yet, it usually just needs a restart to refresh — close U-King entirely (tray icon at the bottom-right → Exit), then double-click U-King.exe on the desktop / USB drive to reopen, and Codex will be recognized.",
  "再等等，重新检测": "Wait more, re-check",
  "💡 提示：请先确认商店里 Codex 的进度条已经 100%（商店里按钮变成「打开」就是装完了）。确认装完后还检测不到的话，把 U-King 彻底关掉重开一次即可识别。":
    "💡 Tip: first confirm the Codex progress bar in the store is at 100% (when the store button turns to “Open”, it’s done). If it’s still not detected after that, fully close and reopen U-King once to recognize it.",

  // manualCodexCli
  "好，我已经打开「Codex CLI 手动安装教程」网页 📖（含可一键复制的命令，共 4 种方案）。\n\n最稳的做法：在 U-King 左侧栏打开「终端」，把教程里【方式一】的几条命令逐条粘贴回车。关键是主包安装时带上 `--include=optional`，避免平台二进制被 npm 跳过。\n\n💡 如果 npm 怎么都装不上，直接用教程最下面的【方式四 · 免 npm】—— 一条命令下官方现成程序，绕开 Node 和 npm 全部坑，最可靠。\n\n装好后命令行里 `codex --version` 能打印版本号就成了，回来点「装好了，重新检测」。":
    "OK, I’ve opened the “Codex CLI manual install guide” page 📖 (with one-click copyable commands, 4 options total).\n\nMost reliable: open “Terminal” in U-King’s left sidebar and paste the commands from [Option 1] one by one, pressing Enter. The key is to add `--include=optional` when installing the main package, so npm doesn’t skip the platform binary.\n\n💡 If npm just won’t install, use [Option 4 · No npm] at the bottom of the guide — one command downloads the official ready-made program, bypassing all Node and npm pitfalls; it’s the most reliable.\n\nWhen `codex --version` prints a version number in the command line, you’re done — come back and click “Installed — re-check”.",
  "看教程网页 ⭐": "View guide page ⭐",
  "✅ 检测到 Codex CLI 已装好（{ver}），继续帮你配驱动。":
    "✅ Detected Codex CLI is installed ({ver}). Continuing with provider setup.",
  "还没检测到 Codex。请确认在命令行里 `codex --version` 能打印出版本号 —— 若提示找不到或闪退，先按教程重装 `@openai/codex --include=optional`，再不行直接走【免 npm】官方二进制兜底。":
    "Codex not detected yet. Please confirm that `codex --version` prints a version number in the command line — if it says not found or crashes, first reinstall `@openai/codex --include=optional` per the guide; if that still fails, use the [No npm] official binary fallback.",

  // manualGuide
  "好，我已打开「{tool} 手动安装教程」网页 📖（含可一键复制的命令）。\n\n建议在 U-King 左侧栏打开「终端」，把教程里的命令逐条粘贴回车。页面里还有「装不上怎么办」通用补救（清代理 / 装 Node / 换源 / 放开脚本策略）。\n\n装好后回来选下面的按钮 👇":
    "OK, I’ve opened the “{tool} manual install guide” page 📖 (with one-click copyable commands).\n\nI suggest opening “Terminal” in U-King’s left sidebar and pasting the commands one by one, pressing Enter. The page also has a general “What if it won’t install” fix (clear proxy / install Node / switch mirror / relax script policy).\n\nWhen done, come back and choose a button below 👇",
  "我装好了，重新检测": "I installed it — re-check",
  "✅ 检测到 Claude Code 已装好（{ver}），继续帮你配驱动。":
    "✅ Detected Claude Code is installed ({ver}). Continuing with provider setup.",
  "还没检测到。请确认命令行里 `claude --version` 能打印版本号；若报「禁止运行脚本」，按教程放开一次 PowerShell 策略。":
    "Not detected yet. Please confirm `claude --version` prints a version number in the command line; if it says “running scripts is disabled”, relax the PowerShell policy once per the guide.",
  "好，我再跑一遍安装验证看看是否已就绪…": "OK, I’ll run the install & verification again to see if it’s ready…",

  // installQueue
  "开始安装 Codex 桌面版（微软商店渠道，不通自动切国内镜像，装完自动验证）…":
    "Installing Codex Desktop (via Microsoft Store; auto-switches to a domestic mirror if unreachable; auto-verifies when done)…",
  "开始安装 {tool}（走 npmmirror 国内加速，装完自动验证）…":
    "Installing {tool} (via npmmirror domestic acceleration; auto-verifies when done)…",
  "已验证": "verified",
  "，经过一轮自动修复": ", after one round of auto-repair",
  "✅ {tool} 安装成功（{detail}）。": "✅ {tool} installed successfully ({detail}).",
  "提示：Codex 桌面版默认英文。在它的 Settings → Language 选「简体中文」可切中文，但该功能受 OpenAI 灰度控制，部分账号暂时只能英文（这是 OpenAI 侧的问题，非装机失败）。":
    "Tip: Codex Desktop defaults to English. You can switch to Chinese in its Settings → Language by choosing “简体中文”, but this feature is gated by OpenAI’s rollout, so some accounts are English-only for now (that’s an OpenAI-side issue, not an install failure).",
  "❌ {tool} 没装上：{err}": "❌ {tool} didn’t install: {err}",
  "未知错误": "Unknown error",
  "我自己去下载装 ⭐": "I’ll download and install it myself ⭐",
  "照教程手动装 ⭐": "Install manually per the guide ⭐",
  "看手动安装教程 ⭐": "View manual install guide ⭐",
  "AI 智能修复": "AI smart repair",
  "AI 再修一轮（{n}/3）": "AI repair another round ({n}/3)",
  "修复环境并重试": "Fix environment and retry",
  "直接重试": "Retry directly",
  "跳过它，继续后面的": "Skip it, continue with the rest",
  "跳过": "Skip",
  "我自己去下载装": "I’ll download and install it myself",
  "照教程手动装": "Install manually per the guide",
  "看手动安装教程": "View manual install guide",
  "环境预检执行异常": "Environment pre-check failed to run",
  "已自动修复：{list}": "Auto-fixed: {list}",
  "仍需注意：{list}": "Still to note: {list}",
  "环境检查没发现问题，直接重试安装。": "The environment check found no issues; retrying the install directly.",
  // 长路径修复（装机失败存量第 2 大桶，23 台）——以前只给客户一段 reg 命令，现在给一颗按钮。
  "帮我开启长路径（需要管理员）": "Enable long paths for me (needs administrator)",
  "先不开，继续装": "Skip for now, keep installing",
  "帮我开启长路径": "Enable long paths for me",
  "先不开": "Skip for now",
  "正在开启长路径支持（同时会开启开发者模式，两项都记进 journal 可回滚）。请在弹出的窗口点「是」…":
    "Enabling long-path support (this also turns on Developer Mode; both are journalled and reversible). Click “Yes” in the prompt that appears…",
  "复检：长路径仍显示未开启 —— 可能是授权被取消，或该策略被公司域策略锁住。可右键 U-King 以管理员身份运行后重试。":
    "Re-check: long paths still show as disabled — the prompt may have been cancelled, or a corporate domain policy is locking this setting. You can right-click U-King → Run as administrator and try again.",
  "复检：长路径已开启。若装依赖仍报路径过长，重启一次电脑再装。":
    "Re-check: long paths are now enabled. If dependency installs still report paths being too long, reboot once and install again.",
  "重试": "Retry",

  // installClawXStep
  "第一步：先为你下载安装主力工具「图形版 ClawX」（约 261MB，点开就能聊的对话界面）…":
    "Step 1: first download and install the main tool “ClawX (GUI)” (about 261MB, a chat interface you can use right away)…",
  " 打开 ClawX 即可用；第一次打开若弹「是否允许访问网络」，点【允许访问】。稍后会自动接好虾盘云。":
    " Open ClawX to use it; if it asks “allow network access” on first launch, click [Allow]. Xiapan Cloud will be connected automatically soon.",
  "ClawX 自动安装没成（可能被杀软拦了），已为你打开手动安装教程，下载后双击安装即可，装完它会自动接好虾盘云。继续帮你装其余工具…":
    "ClawX auto-install didn’t succeed (it may have been blocked by antivirus). I’ve opened the manual install guide for you — just download and double-click to install; once done it auto-connects Xiapan Cloud. Continuing to install the rest…",

  // finishInstallAll
  "工具都装好了，正在自动接入虾盘云驱动（用本机专属 Key，无需配置）…":
    "All tools installed. Auto-connecting the Xiapan Cloud provider (using this device’s dedicated key, no configuration needed)…",
  "✅ 虾盘云已接好！": "✅ Xiapan Cloud is connected!",
  " 现在都国内直连，打开 ClawX 或新开终端直接用。":
    " now all connect directly within China — open ClawX or a new terminal and use them right away.",
  "（内置 Key 余额 {bal}）": " (built-in key balance {bal})",
  "（内置 Key 余额为 0，首次使用前去「AI 设置」充值即可，¥20 起充，¥1=50 万 token）":
    " (built-in key balance is 0; just top up in “AI Settings” before first use — from ¥20, ¥1 = 500,000 tokens)",
  "🤝 已打通「AI 协同」：这些 AI 共用同一个 Key，可以互相调用分工 —— 比如让 Hermes 或 Claude Code 去调 `claude -p`、`codex exec` 把大任务拆开做、再汇总。在任意一个 AI 里说「用 U-King 多 AI 协同帮我…」，它就会照着技能包（uking-teamwork）分工。":
    "🤝 “AI collaboration” is enabled: these AIs share one key and can call each other to divide work — e.g. have Hermes or Claude Code call `claude -p` or `codex exec` to split a big task and then combine the results. In any AI, say “use U-King multi-AI collaboration to help me…” and it will divide the work per the skill pack (uking-teamwork).",

  // resolveRepairKey
  "拿不到设备内置 Key，先跳过 AI 修复（可以选「直接重试」）。":
    "Couldn’t get the device’s built-in key; skipping AI repair for now (you can choose “Retry directly”).",
  "AI 修复需要烧一点 token。U-King 已为这台电脑生成专属虾盘云 Key：{key}（硬件指纹，恒定不变），目前余额为 0 —— 充值即开通，¥20 起充，¥1 = 50 万 token，修一次只要几千 token。":
    "AI repair uses a bit of tokens. U-King has generated a dedicated Xiapan Cloud key for this computer: {key} (hardware fingerprint, always constant). The balance is currently 0 — top up to activate; from ¥20, ¥1 = 500,000 tokens, and one repair costs only a few thousand tokens.",
  "去充值（打开充值页）": "Top up (open top-up page)",
  "我已充值，查余额": "I’ve topped up — check balance",
  "跳过 AI 修复": "Skip AI repair",
  "去充值": "Top up",
  "查余额": "Check balance",
  "到账了！余额 {bal}。": "Received! Balance {bal}.",
  "还没查到余额（到账一般几秒到几分钟），稍后再点「查余额」。":
    "No balance found yet (it usually arrives in seconds to minutes); click “Check balance” again later.",

  // aiRepair
  "🩺 AI 诊断中（虾盘云直连，即使 Claude 没装好也能修）…":
    "🩺 AI diagnosing (direct via Xiapan Cloud — works even if Claude isn’t installed)…",
  "AI 诊断失败：{err}": "AI diagnosis failed: {err}",
  "AI 诊断：{diag}": "AI diagnosis: {diag}",
  "\n\n建议执行 {n} 条修复命令（执行前请过目）：\n":
    "\n\nSuggested {n} repair command(s) (please review before running):\n",
  "\n\n（没有可自动执行的修复命令，请按上面说明手动处理后再重试）":
    "\n\n(No commands can be run automatically; please handle it manually per the notes above, then retry.)",
  "执行这 {n} 条修复命令": "Run these {n} repair command(s)",
  "不执行": "Don’t run",
  "执行修复": "Run repair",
  "修复命令执行完毕，自动重装验证…": "Repair commands finished; auto-reinstalling and verifying…",
  "有修复命令执行失败（已停止），仍会重装验证一次…":
    "A repair command failed (stopped), but I’ll still reinstall and verify once…",

  // pickDriver
  "现在选底层驱动（大模型 API）。推荐虾盘云：U-King 内置，国内直连、充值即用；也可以用你自己的 DeepSeek / GLM / Kimi Key。":
    "Now choose the underlying provider (large-model API). I recommend Xiapan Cloud: built into U-King, direct connection in China, ready once topped up; you can also use your own DeepSeek / GLM / Kimi key.",
  "还原官方直连": "Restore official direct connection",
  "已清除 U-King 写入的驱动配置，Claude Code / Codex 还原为官方登录。":
    "Cleared the provider config U-King wrote; Claude Code / Codex are restored to official login.",
  "这台电脑已有 U-King 专属 Key：{key}（硬件指纹生成，重装系统前恒定），余额 {bal}。直接用它？":
    "This computer already has a U-King dedicated key: {key} (generated from a hardware fingerprint, constant until you reinstall the OS), balance {bal}. Use it directly?",
  "用内置 Key（余额 {bal}）": "Use built-in key (balance {bal})",
  "换我自己的 Key": "Use my own key instead",
  "用内置 Key": "Use built-in key",
  "用自己的 Key": "Use my own key",
  "U-King 已为这台电脑生成专属虾盘云 Key：{key}（硬件指纹，无需注册，已保存在本机 ~/.uking/device.json，重装系统前可备份）。默认余额为 0，充值即开通 —— ¥20 起充，¥1 = 50 万 token，驱动是 DeepSeek-V4 Pro 满血版。":
    "U-King has generated a dedicated Xiapan Cloud key for this computer: {key} (hardware fingerprint, no registration needed, saved locally at ~/.uking/device.json — back it up before reinstalling the OS). The default balance is 0; top up to activate — from ¥20, ¥1 = 500,000 tokens, powered by the full DeepSeek-V4 Pro.",
  "去充值（打开充值页，已带 Key）": "Top up (open top-up page, key included)",
  "用我自己的 Key": "Use my own key",
  "到账！余额 {bal}。用内置 Key 继续。": "Received! Balance {bal}. Continuing with the built-in key.",
  "还没查到余额（到账一般几秒到几分钟），充值完稍等再点「查余额」。":
    "No balance found yet (it usually arrives in seconds to minutes); after topping up, wait a moment and click “Check balance”.",
  "好，把你的虾盘云 Key 粘贴进来（{hint}）。": "OK, paste your Xiapan Cloud key here ({hint}).",
  "好，用 {name}。把你的 API Key 粘贴进来（{hint}）。没有的话点下面按钮去申请。":
    "OK, using {name}. Paste your API key here ({hint}). If you don’t have one, click the button below to get it.",

  // applyAndTest
  "写入底层配置（{list}）…": "Writing underlying config ({list})…",
  "写配置失败：{err}": "Failed to write config: {err}",
  "配置已写入。现在实测连通 —— 让模型真实回一句话…":
    "Config written. Now running a connectivity test — having the model actually reply…",
  "Claude Code 链路（Anthropic 格式）": "Claude Code channel (Anthropic format)",
  "Codex 链路（OpenAI 格式）": "Codex channel (OpenAI format)",
  "🎉 全部链路打通！驱动已生效（配置热更新，新开终端即可用）。":
    "🎉 All channels are working! The provider is active (config hot-reloads; open a new terminal to use it).",
  "有链路没通。常见原因：Key 没充值 / 模型名不对 / 网络波动。":
    "Some channels didn’t connect. Common causes: key not topped up / wrong model name / network fluctuation.",

  // retryDriver
  "重新输入 Key": "Re-enter key",
  "换一个驱动": "Switch provider",
  "先这样，稍后再说": "Leave it for now",
  "先这样": "Leave it for now",

  // finish
  "· Claude Code：{ver}（终端输入 claude 即可用）": "· Claude Code: {ver} (type claude in the terminal to use it)",
  "· Codex CLI：{ver}（终端输入 codex 即可用）": "· Codex CLI: {ver} (type codex in the terminal to use it)",
  "· Codex 桌面版：已安装（和 CLI 共用驱动配置，切一次两边生效）":
    "· Codex Desktop: installed (shares provider config with the CLI; switch once and both take effect)",
  "· 便携 Node 已装到 ~/.uking/runtime（已写入 PATH，新终端生效）":
    "· Portable Node installed at ~/.uking/runtime (added to PATH; effective in new terminals)",
  "⚠️ 检测到系统代理（{proxy}）：claude/codex 会走它。如果上面实测是通的、工具却报连接错误，多半是代理节点失效 —— 把 api.u-claw.org 加进代理的直连名单，或暂时关闭系统代理再试。":
    "⚠️ System proxy detected ({proxy}): claude/codex will route through it. If the test above passed but the tools report connection errors, the proxy node is probably down — add api.u-claw.org to the proxy’s direct list, or temporarily disable the system proxy and try again.",
  "搞定！": "All set!",
  "\n以后随时回到这里：换驱动、查余额、修复安装都行。":
    "\nCome back here anytime: switch providers, check balance, or repair the install.",

  // Render / header / key input
  "对话式安装向导": "Conversational setup wizard",
  "粘贴 API Key": "Paste API key",
  "确定": "Confirm",
  "打开虾盘云充值页（获取 / 充值 Key）": "Open the Xiapan Cloud top-up page (get / top up key)",
  "去 {name} 申请 Key": "Get a key from {name}",

  // DetectCard
  "已安装": "Installed",
  "未检测到": "Not detected",
  "电脑体检结果": "Computer check results",

  // LogCard
  "安装日志（成功）": "Install log (success)",
  "安装日志（失败）": "Install log (failed)",
  "正在安装…": "Installing…",
  "等待输出…": "Waiting for output…",

  // TestCard
  "模型回话：「{reply}」": "Model replied: “{reply}”",
  "虾盘云余额：": "Xiapan Cloud balance:",

  // 装机队列按「claude 跑得起来」收窄 + 收尾自检（2026-08-18）
  "好嘞，开始装 👇 只装「让 Claude Code 真能干活」必需的那几样：Claude Code 本体 + 终端环境。":
    "Here we go — installing only what Claude Code actually needs in order to work: Claude Code itself plus a sane terminal.",
  "全程走国内加速 + 自动验证修复。装完自动接好虾盘云驱动，并当场自检一遍能不能用。":
    "China mirrors throughout, with automatic verify-and-repair. When it finishes we wire up the built-in provider and immediately self-check that it really works.",
  "（自检没跑成，不影响使用；到「首页 · 我的 AI」可以再点一次。）":
    "(The self-check didn't run — harmless. You can run it again from Home.)",
  "🔎 自检通过：Claude Code 真跑得起来、驱动已接、余额可用、技能包已就位 —— 可以去 U-Workspace 干活了。":
    "🔎 Self-check passed: Claude Code really runs, the provider is wired up, there's balance, and the skill packs are in place — U-Workspace is ready.",
  "🔎 自检发现 {n} 件事还没到位（其余都好了）：":
    "🔎 Self-check found {n} thing(s) not ready yet (everything else is fine):",
  "Harness Doctor 已装好。回到「我的 AI」点它可立即体检；之后生成 AI 体检报告时也会自动附上四个 Harness 的诊断摘要。":
    "Harness Doctor is installed. Click it in \"My AI\" to run a checkup anytime; future AI health reports will automatically include diagnostics from all four harnesses.",
};
