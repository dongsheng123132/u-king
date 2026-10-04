/** 英文覆盖字典 · 进阶/连接器/动态/教程（Advanced/Connectors/Feed/Tutorial）+ 一批通用词。 */
export const misc: Record<string, string> = {
  "（{n}）": "({n})",
  "本机": "This PC",

  // ── Advanced.tsx ───────────────────────────────────────────────
  "已复制{label}": "Copied {label}",
  "复制失败，请手动选中复制": "Copy failed — please select and copy manually",
  "已复制": "Copied",
  "复制": "Copy",
  "（检测中…）": "(detecting…)",
  "模型 Model（选一个再复制）": "Model (pick one, then copy)",
  "  ★推荐": "  ★Recommended",
  "接口地址 Base URL": "Base URL",
  "API Key（你的内置 Key）": "API Key (your built-in key)",
  // 设备凭证轮换。旧体系的 Key 是按硬件算出来的、换不掉，所以以前没有这组文案。
  "怀疑 Key 泄露了？可以随时换一把，余额跟着走。":
    "Think your key leaked? Rotate it any time — your balance comes along.",
  "更新 Key": "Rotate key",
  "更新中…": "Rotating…",
  // 「Key = 一张充值卡」这个模型的全部操作面：备份 / 搬走 / 换掉。没有恢复码、没有账号。
  "上面这把 Key 就是你的账户，复制下来存好。换电脑时填回去就能接着用；怀疑泄露了随时换一把，余额跟着走。":
    "The key above IS your account — copy it somewhere safe. Paste it back on a new computer to carry on; rotate it any time you suspect it leaked, and your balance follows.",
  "填入已有 Key": "Use existing key",
  "验证中…": "Verifying…",
  "把你已有的 Key 填进来（换了电脑、或者这台机器上的另一份 U-King，填同一把就能共用余额）：":
    "Paste a key you already have (new computer, or another copy of U-King on this machine — the same key shares one balance):",
  "用这把 Key 替换本机当前的 Key？\n当前这把如果还有余额且你没有备份，替换后将无法自动找回。":
    "Replace this machine's current key with that one?\nIf the current key still has a balance and you have no backup, it cannot be recovered afterwards.",
  "已启用这把密钥": "Key is now in use",
  "这把密钥用不了：": "That key doesn't work: ",
  "换一把新的 API Key？旧 Key 会立即失效，余额自动保留。\n如果你把旧 Key 配到过别的电脑或脚本里，那边需要重新填。":
    "Rotate to a new API key? The old key stops working immediately; your balance is kept.\nIf you configured the old key on another machine or in a script, you'll need to update it there.",
  "已更新访问密钥": "Access key rotated",
  "更新密钥失败：": "Key rotation failed: ",
  "检测到本机配置曾丢失，已重新生成一把 Key。原来的余额无法自动找回 —— 请凭充值订单号联系客服迁移。":
    "This machine's local config was lost at some point, so a new key was issued. The previous balance cannot be recovered automatically — contact support with your top-up order number to have it migrated.",
  "模型": "Model",
  "正在卸载，U-King 即将关闭并清理 ~/.uking…": "Uninstalling — U-King will close and clean up ~/.uking…",
  "卸载失败：{e}": "Uninstall failed: {e}",
  "正在打开 ClawX…": "Opening ClawX…",
  "打不开 ClawX —— 可能还没装。请到「我的 AI」→ 找到 ClawX → 一键安装":
    "Can't open ClawX — it may not be installed. Go to “My AI” → find ClawX → one-click install",
  "需要临时关闭 ClawX 来写入配置（对话已自动保存），完成后会自动重启。是否继续？":
    "ClawX needs to close briefly to write the config (chats are auto-saved); it will restart automatically when done. Continue?",
  "正在关闭 ClawX…": "Closing ClawX…",
  "正在写入配置…": "Writing config…",
  "已把虾盘云配进 ClawX，正在重启…": "Xiapan Cloud configured into ClawX — restarting…",
  "已把虾盘云配进 ClawX": "Xiapan Cloud configured into ClawX",
  "自动配置失败：": "Auto-config failed: ",
  "（可照下面手动配）": " (you can configure it manually below)",
  "进阶 · 桌面 App 版": "Advanced · Desktop apps",
  "给想用图形界面的高级用户": "For power users who prefer a GUI",
  "这里是 ": "Here are ",
  " 等桌面 App。装机我们帮你「下一步下一步」装好；":
    " and other desktop apps. We install them for you step-by-step; ",
  "模型配置请照下面教程，自己把 Key 复制进 App 的设置里":
    "for model config, follow the guide below and paste the Key into the app's settings yourself",
  "—— App 的自动配置坑多（切了常没反应），手动粘一次最稳，你也能当场看到生效。":
    " — the app's auto-config is flaky (switches often do nothing); pasting once by hand is most reliable, and you'll see it take effect on the spot.",
  "（命令行工具的「一键切换模型」仍在「AI 设置」页，那层可靠、不受此影响。）":
    "(One-click model switching for command-line tools is still on the “AI Settings” page — that layer is reliable and unaffected.)",
  "✓ 已安装": "✓ Installed",
  "安装中…": "Installing…",
  "OpenAI 兼容": "OpenAI-compatible",
  "复制粘贴": "into it",
  "ClawX（图形版 AI）· 复制 Key 接虾盘云（3 步）": "ClawX (GUI AI) · Copy the key to connect Xiapan Cloud (3 steps)",
  "在「我的 AI」可一键安装；装好后照这里把虾盘云填进去":
    "One-click install in “My AI”; once installed, follow the steps here to fill in Xiapan Cloud",
  "自动关闭 ClawX → 写入虾盘云配置 → 重启 ClawX": "Auto-close ClawX → write Xiapan Cloud config → restart ClawX",
  "配置中…": "Configuring…",
  "一键配好 ClawX": "Configure ClawX in one click",
  "打开": "Open",
  "推荐点上方 ": "We recommend clicking ",
  "「一键配好 ClawX」": "“Configure ClawX in one click”",
  "（自动关闭→写入→重启）；若没成功，照下面 3 步手动配：":
    " above (auto-close → write → restart); if it doesn't work, configure manually in the 3 steps below:",
  "想自己换模型时，照这 3 步把虾盘云接进 ClawX：":
    "When you want to switch models yourself, follow these 3 steps to connect Xiapan Cloud to ClawX:",
  "打开 ClawX，进 ": "Open ClawX, go to ",
  "设置（Settings）→ 模型 / 供应商（Models / Providers）": "Settings → Models / Providers",
  "，点「添加供应商（Add Provider）」。": ", and click “Add Provider”.",
  "接入类型选 ": "For the connection type, choose ",
  "OpenAI 兼容（OpenAI Compatible）": "OpenAI Compatible",
  "，把下面的接口地址、API Key 粘进去；模型填下面": ", paste the Base URL and API Key below; for the model enter ",
  "选好的那个": "the one you picked",
  "（不确定就用默认 ": " below (if unsure, use the default ",
  "）。": ").",
  "保存后在 ClawX 里": "After saving, ",
  "选中这个供应商": "select this provider",
  "即可对话。": " in ClawX to start chatting. ",
  "填完记得重启一次 ClawX": "Remember to restart ClawX once after filling it in",
  "——它只在启动时读取配置，不重启常常「切了没反应」。":
    " — it only reads the config at startup; without a restart, switches often “do nothing”.",
  "卸载 U-King": "Uninstall U-King",
  "仅删除 U-King 自己装的东西：便携运行时（Node / Git / Python）、技能包、作图 / 视频历史、桌面快捷方式、右键菜单。":
    "Removes only what U-King installed: the portable runtime (Node / Git / Python), skill packs, image / video history, desktop shortcut, and context menu.",
  "不会动": "It won't touch",
  "你的 Claude Code / Codex 等 AI 工具及其配置（": " your Claude Code / Codex and other AI tools and their configs (",
  "、": ", ",
  " 等一律保留）。": " and the like are all kept).",
  "卸载 U-King…": "Uninstall U-King…",
  "确认卸载？将删除 ~/.uking 并关闭 U-King。": "Confirm uninstall? This deletes ~/.uking and closes U-King.",
  "正在卸载…": "Uninstalling…",
  "确认卸载": "Confirm uninstall",
  "取消": "Cancel",

  // ── Connectors.tsx ─────────────────────────────────────────────
  "读取连接器失败: {e}": "Failed to load connectors: {e}",
  "{name}：{msg}": "{name}: {msg}",
  "选一个允许「{name}」访问的文件夹": "Pick a folder to allow “{name}” to access",
  "操作失败: {e}": "Operation failed: {e}",
  "AI 连接器": "AI Connectors",
  "给 ": "Give the ",
  "Claude Code 大脑": "Claude Code brain",
  "挂上外部能力 —— 让 AI 能读写文件、操控浏览器、记住你、想得更深。在 U-Workspace 把大脑切到 Claude Code 后生效。":
    " external abilities — let the AI read/write files, drive the browser, remember you, and think deeper. Takes effect after you switch the brain to Claude Code in U-Workspace.",
  "读取中…": "Loading…",
  "还没装 Claude Code": "Claude Code isn't installed yet",
  "连接器目前只支持 Claude Code 大脑。先去「① 装 AI」装好 Claude Code，再回来启用连接器。":
    "Connectors currently only support the Claude Code brain. Go to “① Install AI” to install Claude Code first, then come back to enable connectors.",
  "去装 Claude Code": "Install Claude Code",
  "已启用": "Enabled",
  "停用": "Disable",
  "选文件夹启用": "Pick folder & enable",
  "启用": "Enable",
  "连接器基于 MCP（Model Context Protocol），首次使用时 Claude Code 会拉起对应的小程序（需联网）。启用后在 Claude Code 里输入需求即可自动调用；停用即从 Claude 配置移除，随时可切。":
    "Connectors are built on MCP (Model Context Protocol); on first use Claude Code launches the matching helper program (network required). Once enabled, just type your request in Claude Code and it's called automatically; disabling removes it from the Claude config — switch anytime.",

  // ── Feed.tsx ───────────────────────────────────────────────────
  "刷新": "Refresh",
  "正在拉取最新内容…": "Fetching the latest content…",
  "在线专题暂时没加载出来": "The online section didn't load for now",
  "不影响装机、充值、作图和视频。网络恢复后点「刷新」即可看到最新内容。":
    "This doesn't affect install, top-up, image or video. Once the network is back, click “Refresh” to see the latest.",
  "暂时还没有内容，过段时间再来看看～": "No content yet — check back a bit later~",
  "点任意一条用浏览器打开详情": "Click any item to open its details in the browser",

  // ── Tutorial.tsx ───────────────────────────────────────────────
  "几步开始用 AI · 完全不用懂电脑": "Start using AI in a few steps · no computer skills needed",
  "U-King 是一个「AI 管家」。它已经帮你把全球最强的 AI 都装好、配好了，你只要照下面几步点一点，就能像微信聊天一样跟 AI 对话——让它写文章、写代码、做表格、查资料、画图，都行。":
    "U-King is an “AI butler”. It has already installed and configured the world's best AI for you — just follow the steps below and click a few times, and you can chat with AI like on WeChat: writing articles, code, spreadsheets, research, drawing, all of it.",
  "在「我的 AI」一键装好 Claude Code（最强编程 agent）+ 终端环境":
    "Install Claude Code (the strongest coding agent) + terminal environment in one click from “My AI”",
  "新手首选 ": "The top pick for beginners is ",
  "——全球公认最强的编程 AI，在「我的 AI」里点一键安装，U-King 会帮你":
    " — the coding AI widely regarded as the strongest in the world. Click one-click install in “My AI” and U-King will ",
  "自动下载、安装、配好 AI": "automatically download, install, and set up the AI",
  "，你只要等它装完。想要图形界面（像微信一样点点就能用）也可以额外装":
    " for you — just wait for it to finish. If you'd also like a GUI (click-based, just like WeChat), you can additionally install ",
  "去「我的 AI」": "Go to “My AI”",
  "，在「进阶 / App 版」页可装，非必须。": " — available on the “Advanced / App version” page, not required.",
  "打开工作台，跟 AI 说话": "Open the workspace and talk to the AI",
  "装好后点「进 U-Workspace 开始干活」。想要最全能力，在工作台右上角点「终端」，输入 ":
    "After installing, click “Enter U-Workspace to start working”. For full capability, click “Terminal” at the top-right of the workspace and type ",
  " 回车即可；不想碰终端，直接在对话框里说人话就行，底下跑的是同一个 Claude Code、同一个 Key。若额外装了 ClawX，第一次打开 Windows 可能弹「是否允许访问网络」，":
    " and press Enter; if you'd rather not touch the terminal, just type in plain language in the chat box — it runs the same Claude Code with the same key underneath. If you also installed ClawX, Windows may pop up an “Allow network access?” dialog on first launch — ",
  "一定要点【允许访问】": "you must click [Allow access]",
  "，不然 AI 连不上。": ", otherwise the AI can't connect.",
  "第一次用，先充值开通（¥20 起，够聊很久）": "First time: top up to activate (from ¥20, enough for a long time)",
  "AI 是按量计费的，": "AI is billed by usage; ",
  "第一次使用前需要先充值开通": "you need to top up to activate before first use",
  "。在「AI 设置 → 账号 · 充值」里点":
    ". In “AI Settings → Account & Top-up”, click ",
  "「充值」": "“Top up”",
  "，会自动填好你这台电脑的专属 Key，微信扫码即可，": " — it auto-fills this PC's dedicated key; just scan with WeChat to pay. ",
  "¥20 起充，¥1 = 50 万 token": "From ¥20, ¥1 = 500,000 tokens",
  "，到账即时、余额永久有效、不用不扣。": " — credited instantly, balance never expires, no charge when unused.",
  "像聊天一样打字，回车发送": "Type like chatting, press Enter to send",
  "在 U-Workspace 的对话框或终端里直接打字，比如「": "Type directly in U-Workspace's chat box or terminal, e.g. “",
  "帮我写一封请假邮件": "Write me a leave-request email",
  "」，按回车，AI 就会回你。想让它做啥就直说，说中文就行。":
    "”, press Enter, and the AI replies. Just say what you want — Chinese is fine.",
  "就这么简单。剩下的，问 AI 自己就好。": "That's it. For the rest, just ask the AI itself.",
  "AI 能帮你做什么？（直接打字问它就行）": "What can AI do for you? (just type and ask)",
  "写文章 / 邮件": "Write articles / emails",
  "「帮我写一份述职报告」": "“Write me a performance report”",
  "写代码 / 改 bug": "Write code / fix bugs",
  "「写个 Excel 自动改名脚本」": "“Write a script to auto-rename in Excel”",
  "做表格 / 整理数据": "Make tables / organize data",
  "「把这段文字整理成表格」": "“Turn this text into a table”",
  "翻译 / 润色": "Translate / polish",
  "「把这段翻成地道英文」": "“Translate this into natural English”",
  "查资料 / 出主意": "Research / brainstorm",
  "「给孩子起 10 个名字」": "“Suggest 10 names for a baby”",
  "AI 画图": "AI drawing",
  "在「AI 作图」里输入即可": "Just type in “AI Image”",
  "想要更聪明的回答？在「我的 AI」每个工具下点「单独给这个工具换模型（高级）」可":
    "Want smarter answers? Under each tool in “My AI”, click “Switch model for this tool (advanced)” to ",
  "换 AI": "change the AI",
  "——不确定就用": " — if unsure, use ",
  "「DeepSeek V4 Pro（推荐）」": "“DeepSeek V4 Pro (recommended)”",
  "，又快又省；要写代码 / 攻难题，换": ", fast and economical; for coding / hard problems, switch to ",
  " 系更强（更费额度）。每个选项下都有一句人话说明。":
    " for more power (uses more quota). Each option has a plain-language note.",
  "新手常见问题": "Beginner FAQ",
  "要花钱吗？怎么才能开始用？": "Does it cost money? How do I get started?",
  "用 AI 是按量计费的（说几句话花几分钱）。": "AI is billed by usage (a few sentences cost a few cents). ",
  "第一次使用需要先充值开通": "You need to top up to activate before first use",
  "——充值入口在「AI 设置 → 账号 · 充值」，":
    " — the top-up entry is in “AI Settings → Account & Top-up”. ",
  "¥20 起充": "From ¥20",
  "，到账即时、余额永久有效、不用不扣，¥20 通常够聊很久。":
    " — credited instantly, balance never expires, no charge when unused; ¥20 usually lasts a long time.",
  "Claude Code 图标是灰色的、点不动？": "The Claude Code icon is grey and won't click?",
  "那是还没装好。在「我的 AI」里点它的「安装」，U-King 会自动帮你下载安装（会显示进度），等它装完就能在工作台里用了。":
    "It isn't installed yet. Click its “Install” in “My AI” and U-King will download and install it for you (with progress); once done you can use it in the Workbench.",
  "装了 ClawX 但一直转圈、连不上 AI？": "Installed ClawX but it keeps spinning and can't connect to the AI?",
  "八成是第一次打开时那个「是否允许访问网络」的窗口被点了「取消」。解决：关掉 ClawX，回 U-King 重新点「打开 ClawX」，这次弹窗点【允许访问】即可。详见盘内《第一次打开 ClawX 必看》。ClawX 是可选的图形界面，不装它同样能用 Claude Code。":
    "Most likely the “Allow network access?” dialog on first launch was clicked “Cancel”. Fix: close ClawX, go back to U-King and click “Open ClawX” again, and this time click [Allow access] on the popup. See “Read Me First When Opening ClawX” on the drive. ClawX is an optional GUI — you can use Claude Code just fine without it.",
  "充了钱但还是说余额不足 / 连不上？": "Paid but it still says insufficient balance / can't connect?",
  "回到「AI 设置」点一下「测试连通 / 查询余额」刷新一下。还不行就看盘内《常见故障排查手册》，或按《远程协助看这里》联系我们远程帮你弄。":
    "Go back to “AI Settings” and click “Test connection / Check balance” to refresh. If it still fails, see “Troubleshooting Handbook” on the drive, or follow “Remote Help Here” to contact us for remote assistance.",
  "怎么看还剩多少额度？怎么充值？": "How do I see my remaining balance? How do I top up?",
  "余额显示在「AI 设置」页顶部（多少万 token）。要充值去「AI 设置 → 账号 · 充值」点充值按钮，会打开充值页、自动填好你的 Key，微信扫码即可，¥20 起充，到账即时、永久有效。":
    "Your balance (in tokens) is shown at the top of “AI Settings”. To top up, go to “AI Settings → Account & Top-up” and click the top-up button — it opens the top-up page with your key pre-filled; pay by WeChat QR, from ¥20, credited instantly and never expires.",
  "家里 / 单位几台电脑能共用吗？": "Can several PCs at home / work share it?",
  "可以。同一个 Key（额度）能同时配到多台电脑、手机 App 上用，额度共享、一起扣。在「接入指南」里复制 Key，按上面的说明配到别的设备即可。":
    "Yes. The same key (quota) can be configured on multiple PCs and phone apps at once, sharing and drawing down the same quota. Copy the key in “Setup Guide” and set it up on other devices per the instructions above.",
  "关掉窗口 AI 就停了吗？": "Does closing the window stop the AI?",
  "U-King 点右上角「缩到托盘」是最小化到右下角，还在后台跑。ClawX 是独立程序，直接关它的窗口即可。点工具卡的「打开终端」会弹出独立的终端窗口，关掉 U-King 也不影响它。":
    "Clicking “Minimize to tray” in U-King's top-right minimizes it to the bottom-right and keeps it running in the background. ClawX is a standalone program — just close its window. Clicking “Open terminal” on a tool card pops up a separate terminal window that isn't affected by closing U-King.",
  "下面是给进阶用户看的「接入指南」——把这台电脑的 AI 额度配到手机 App、其它软件里用。新手可以先不看。":
    "Below is the “Setup Guide” for advanced users — how to use this PC's AI quota in phone apps and other software. Beginners can skip it for now.",

  "正在打开…": "Opening…",
  "打开失败": "Failed to open",
  "重新打开": "Reopen",

  // ── 通用词（原 Experts.tsx / experts.ts 的标签；专家墙 2026-10-04 删除后仍被其它页引用，故保留） ──
  "全部": "All",
  "文档": "Docs",
  "作图": "Image gen",
  "提示词": "Prompts",
  "视频": "Video",
  "文生视频": "Text-to-video",
  "标题": "Headlines",
};
