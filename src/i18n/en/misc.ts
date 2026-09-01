/** 英文覆盖字典 · 备份/进阶/连接器/动态/教程/GEO/专家（Backup/Advanced/Connectors/Feed/Tutorial/Geo/Experts）。 */
export const misc: Record<string, string> = {
  // ── Backup.tsx ─────────────────────────────────────────────────
  "选择备份位置（U 盘根目录）": "Choose a backup location (USB drive root)",
  "准备备份…": "Preparing backup…",
  "已备份 {items}（{size}）到 U 盘": "Backed up {items} ({size}) to USB drive",
  "本机暂无 ClawX / 龙虾数据可备份": "No ClawX / OpenClaw data on this PC to back up",
  "备份失败：": "Backup failed: ",
  "从「{machine}」{time} 的快照还原到本机？\n\n· 会先关闭 ClawX\n· 本机当前的 ClawX 对话/设置会被这份快照整份替换\n· 替换前会自动把本机当前状态也备份一份（可回滚），旧数据另存为 .uking-bak\n\n确定继续吗？":
    "Restore this PC from the snapshot of “{machine}” taken {time}?\n\n· ClawX will be closed first\n· This PC's current ClawX chats/settings will be fully replaced by this snapshot\n· Before replacing, this PC's current state is backed up too (rollback safe); old data is kept as .uking-bak\n\nContinue?",
  "准备还原…": "Preparing restore…",
  "已还原 {n} 项。请重新打开 ClawX 查看": "Restored {n} item(s). Reopen ClawX to see them",
  "（本机原状态已自动备份）": " (this PC's previous state was auto-backed up)",
  "这份快照里没有可还原的数据": "This snapshot has no restorable data",
  "还原失败：": "Restore failed: ",
  "备份 / 同步到 U 盘": "Backup / Sync to USB drive",
  "把 ClawX 的对话和设置（含龙虾工作区）存到 U 盘，回家插上一键还原，接着干活。":
    "Save ClawX chats and settings (incl. the OpenClaw workspace) to a USB drive; plug it in at home, restore in one click, and keep working.",
  "备份位置": "Backup location",
  "（探测中…）": "(detecting…)",
  "换位置": "Change location",
  "备份中…": "Backing up…",
  "立即备份到 U 盘": "Back up to USB drive now",
  "还原采用「整份替换 + 自动留底」：换电脑还原前会先把本机当前状态也备份一份，旧数据另存为":
    "Restore uses “full replace + auto-backup”: before restoring on another PC, this machine's current state is backed up first, and the old data is saved as",
  "，不会凭空丢。ClawX 的对话存在数据库里，无法逐条合并，故只能整份覆盖。":
    " — nothing is lost. ClawX chats live in a database and can't be merged item by item, so only a full overwrite is possible.",
  "U 盘上的备份": "Backups on the USB drive",
  "（{n}）": "({n})",
  "这个位置还没有备份。点上面「立即备份到 U 盘」创建第一份。":
    "No backups at this location yet. Click “Back up to USB drive now” above to create the first one.",
  "本机": "This PC",
  "（空）": "(empty)",
  "还原到本机": "Restore to this PC",
  "办公室 ↔ 家里 怎么用": "Office ↔ Home: how to use",
  "1. 办公室干完活 → 这里「立即备份到 U 盘」 → 拔盘带走":
    "1. Finish work at the office → “Back up to USB drive now” here → unplug and take it with you",
  "2. 回家插上 U 盘开 U-King → 找到办公室那条快照 → 「还原到本机」 → 打开 ClawX 接着用":
    "2. At home, plug in the USB and open U-King → find that office snapshot → “Restore to this PC” → open ClawX and carry on",
  "提示：快照里带的是本机虾盘云 Key，换机还原后会用同一个钱包计费 —— 单人多机正好省事。":
    "Tip: the snapshot carries this PC's Xiapan Cloud key, so after restoring on another machine it bills from the same wallet — handy for one person across multiple PCs.",

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
  "正在下载 Hermes 安装器…": "Downloading the Hermes installer…",
  "正在打开 Hermes…": "Opening Hermes…",
  "没找到 Hermes，请先安装，或从开始菜单打开": "Hermes not found — install it first, or open it from the Start menu",
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
  "Hermes 桌面版（Nous 官方）": "Hermes desktop (official Nous)",
  "✓ 已安装": "✓ Installed",
  "Nous Research 自进化 AI 智能体 · 官方图形版": "Nous Research self-evolving AI agent · official GUI",
  "打开 Hermes": "Open Hermes",
  "安装中…": "Installing…",
  "下载安装 Hermes": "Download & install Hermes",
  "装好后，照下面 4 步把虾盘云接进 Hermes（一次配好，永久生效）：":
    "Once installed, follow these 4 steps to connect Xiapan Cloud to Hermes (set up once, works forever):",
  "装好后点上方 ": "After installing, click ",
  "「打开 Hermes」": "“Open Hermes”",
  "，进入 Hermes 主界面。": " above to enter the Hermes main screen.",
  "在 Hermes 里打开 ": "In Hermes, open ",
  "设置（Settings）→ 供应商（Providers）": "Settings → Providers",
  "，新增一个 ": ", and add a new ",
  "OpenAI 兼容": "OpenAI-compatible",
  " 供应商。": " provider.",
  "把下面三项": "Copy-paste the three items below ",
  "复制粘贴": "into it",
  "进去（点右边「复制」按钮，再到 Hermes 对应输入框粘贴）：":
    " (click the “Copy” button on the right, then paste into the matching Hermes field):",
  "填好后": "Once filled in, ",
  "保存并选中该供应商/模型": "save and select this provider/model",
  "，回到对话框发一句话测试，能回话即成功。": ", go back to the chat, send a message to test — a reply means success.",
  "Hermes 官网": "Hermes website",
  "· 内置 Key 检测中，稍候即可复制": "· Detecting built-in key — ready to copy shortly",
  "· 内置 Key 未充值，去「AI 设置」充值后可用": "· Built-in key has no balance — top up in “AI Settings” to use it",
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
  "装好主力工具 ClawX（图形版 AI 助手）": "Install the main tool ClawX (GUI AI assistant)",
  "新手首选 ": "The top pick for beginners is ",
  "——图形界面，像微信一样点点就能用，最好上手。在「我的 AI」里点 ClawX，U-King 会帮你":
    " — a graphical interface you use with clicks, just like WeChat, easiest to get started. Click ClawX in “My AI” and U-King will ",
  "自动下载、安装、配好 AI": "automatically download, install, and set up the AI",
  "，你只要等它装完。": " for you — just wait for it to finish.",
  "去「我的 AI」": "Go to “My AI”",
  "打开 ClawX，记得点「允许访问网络」": "Open ClawX, and be sure to click “Allow network access”",
  "装好后点「打开 ClawX」。第一次打开时 Windows 可能弹一个「是否允许访问网络」的窗口，":
    "After installing, click “Open ClawX”. On first launch, Windows may pop up an “Allow network access?” dialog — ",
  "一定要点【允许访问】": "you must click [Allow access]",
  "，不然 AI 连不上。": ", otherwise the AI can't connect.",
  "（U-King 多数情况已帮你提前放行，不一定会弹。）": "(U-King usually clears this in advance, so it may not pop up.)",
  "第一次用，先充值开通（¥20 起，够聊很久）": "First time: top up to activate (from ¥20, enough for a long time)",
  "AI 是按量计费的，": "AI is billed by usage; ",
  "第一次使用前需要先充值开通": "you need to top up to activate before first use",
  "。在「我的 AI」或「接入指南」页右上角点": ". In the top-right of “My AI” or the “Setup Guide” page, click ",
  "「充值」": "“Top up”",
  "，会自动填好你这台电脑的专属 Key，微信扫码即可，": " — it auto-fills this PC's dedicated key; just scan with WeChat to pay. ",
  "¥20 起充，¥1 = 50 万 token": "From ¥20, ¥1 = 500,000 tokens",
  "，到账即时、余额永久有效、不用不扣。": " — credited instantly, balance never expires, no charge when unused.",
  "像聊天一样打字，回车发送": "Type like chatting, press Enter to send",
  "ClawX 打开后，在输入框直接打字，比如「": "Once ClawX is open, just type in the box, e.g. “",
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
  "——充值入口在「我的 AI」和「接入指南」页右上角，": " — the top-up entry is in the top-right of “My AI” and the “Setup Guide” page. ",
  "¥20 起充": "From ¥20",
  "，到账即时、余额永久有效、不用不扣，¥20 通常够聊很久。":
    " — credited instantly, balance never expires, no charge when unused; ¥20 usually lasts a long time.",
  "ClawX 图标是灰色的、点不动？": "The ClawX icon is grey and won't click?",
  "那是 ClawX 还没装。点一下灰色图标，U-King 会自动帮你下载安装（约 210MB，会显示进度），等它装完就变彩色、能打开了。":
    "That means ClawX isn't installed yet. Click the grey icon and U-King will download and install it automatically (about 210MB, with progress shown); once done it turns colored and opens.",
  "ClawX 一直转圈、连不上 AI？": "ClawX keeps spinning and can't connect to the AI?",
  "八成是第一次打开时那个「是否允许访问网络」的窗口被点了「取消」。解决：关掉 ClawX，回 U-King 重新点「打开 ClawX」，这次弹窗点【允许访问】即可。详见盘内《第一次打开 ClawX 必看》。":
    "Most likely the “Allow network access?” dialog on first launch was clicked “Cancel”. Fix: close ClawX, go back to U-King and click “Open ClawX” again, and this time click [Allow access] on the popup. See “Read Me First When Opening ClawX” on the drive.",
  "充了钱但还是说余额不足 / 连不上？": "Paid but it still says insufficient balance / can't connect?",
  "回到「AI 设置」点一下「测试连通 / 查询余额」刷新一下。还不行就看盘内《常见故障排查手册》，或按《远程协助看这里》联系我们远程帮你弄。":
    "Go back to “AI Settings” and click “Test connection / Check balance” to refresh. If it still fails, see “Troubleshooting Handbook” on the drive, or follow “Remote Help Here” to contact us for remote assistance.",
  "怎么看还剩多少额度？怎么充值？": "How do I see my remaining balance? How do I top up?",
  "在「我的 AI」首页右上角就显示余额（多少万 token）。要充值点旁边的「高级 / 余额」或「接入指南」里的充值按钮，会打开充值页、自动填好你的 Key，微信扫码即可，¥20 起充，到账即时、永久有效。":
    "The balance (in hundreds of thousands of tokens) is shown in the top-right of the “My AI” home page. To top up, click “Advanced / Balance” next to it or the top-up button in the “Setup Guide”; it opens the top-up page with your key pre-filled — scan with WeChat, from ¥20, credited instantly, never expires.",
  "家里 / 单位几台电脑能共用吗？": "Can several PCs at home / work share it?",
  "可以。同一个 Key（额度）能同时配到多台电脑、手机 App 上用，额度共享、一起扣。在「接入指南」里复制 Key，按上面的说明配到别的设备即可。":
    "Yes. The same key (quota) can be configured on multiple PCs and phone apps at once, sharing and drawing down the same quota. Copy the key in “Setup Guide” and set it up on other devices per the instructions above.",
  "关掉窗口 AI 就停了吗？": "Does closing the window stop the AI?",
  "U-King 点右上角「缩到托盘」是最小化到右下角，还在后台跑。ClawX 是独立程序，直接关它的窗口即可。点工具卡的「打开终端」会弹出独立的终端窗口，关掉 U-King 也不影响它。":
    "Clicking “Minimize to tray” in U-King's top-right minimizes it to the bottom-right and keeps it running in the background. ClawX is a standalone program — just close its window. Clicking “Open terminal” on a tool card pops up a separate terminal window that isn't affected by closing U-King.",
  "下面是给进阶用户看的「接入指南」——把这台电脑的 AI 额度配到手机 App、其它软件里用。新手可以先不看。":
    "Below is the “Setup Guide” for advanced users — how to use this PC's AI quota in phone apps and other software. Beginners can skip it for now.",

  // ── Geo.tsx ────────────────────────────────────────────────────
  "AI 几乎不认识你": "AI barely knows you",
  "少数 AI 模糊知道你": "A few AIs vaguely know you",
  "部分 AI 认识你，但不全面": "Some AIs know you, but not fully",
  "多数 AI 认识并会推荐你": "Most AIs know and will recommend you",
  "请先填公司名": "Please enter a company name first",
  "体检面板已生成（{n} 个渠道），已在浏览器打开": "Check panel generated ({n} channels) and opened in the browser",
  "体检失败，请重试": "Check failed, please try again",
  "AI 可见度 {score}/100 —— {label}，报告已在浏览器打开": "AI visibility {score}/100 — {label}. Report opened in the browser",
  "AI 可见度测试失败，请重试": "AI visibility test failed, please try again",
  // GEO 页 2026-08-24 改版：展示 + 转化微信，不在客户机上真跑 AI 可见度测试。
  "样板报告打开失败": "Could not open the sample report",
  "咨询内容已复制，加微信 hecare888 后直接粘贴发给我们": "Enquiry text copied. Add WeChat hecare888 and paste it to us",
  "请手动添加微信 hecare888，把公司名和网址发给我们": "Please add WeChat hecare888 manually and send us your company name and website",
  "看一份《AI 可见度报告》长什么样": "See what an “AI Visibility Report” looks like",
  "演示样例": "Sample",
  "这是一份": "This is a ",
  "虚构公司的演示报告": "demo report for a fictional company",
  "，用来说明我们出的报告包含什么：AI 可见度总分、6 家大模型分别怎么评价你、它们普遍缺哪几条关键信息、以及按影响力排序的改进清单。":
    ", showing what our reports contain: an overall AI visibility score, how each of 6 LLMs describes you, which key facts they all lack, and a fix list ordered by impact.",
  "它不会检测你的网站，也不消耗任何额度。": "It does not scan your website and uses no credits.",
  "正在打开…": "Opening…",
  "打开样板报告": "Open the sample report",
  "想要一份针对你公司的真实报告？": "Want a real report for your company?",
  "我们逐一实测 6 家大模型 + 人工判读后出具，再按结果谈怎么优化":
    "We test 6 LLMs one by one, review the answers by hand, then discuss what to fix",
  "AI 的回答有波动，同一个问题问两次结果可能不同，还常把同名公司搞混。所以我们不做一键批量跑，而是":
    "LLM answers fluctuate — ask twice and you may get different results, and they often confuse companies with similar names. So instead of a one-click batch run we do ",
  "逐家实测 + 人工核对": "per-model testing with human review",
  "后出报告，附上能照着做的改进清单。": ", then hand over the report with a fix list you can act on.",
  "① 复制咨询内容": "① Copy enquiry text",
  "把上面填的公司信息拼成一句话": "Turns what you typed above into one sentence",
  "加微信后直接粘贴，省得重打一遍": "Paste it after adding us on WeChat — no need to retype",
  "② 复制微信号": "② Copy WeChat ID",
  "点这里复制，去微信搜索添加": "Click to copy, then search for it in WeChat",
  "报价按站点数和行业一对一给（我们帮你做：AI 可读企业主页 + llms.txt / 结构化数据部署 + 高德/百度/腾讯三大地图信息同步 + 每月复测追踪）。":
    "Pricing is quoted one-to-one based on site count and industry (we build: an AI-readable company page, llms.txt / structured data deployment, listing sync across Amap / Baidu / Tencent maps, and monthly re-testing).",
  "提示：上面的「免费自查」打开的是各家 AI 和搜索引擎的真实页面，结果由你自己看 —— 搜不到 ≠ 不存在，只是还没被 AI 收录。样板报告为演示数据，不代表你公司的实际情况。":
    "Note: the free self-check opens the real AI and search pages for you to read yourself — not found ≠ does not exist, it just is not indexed by AI yet. The sample report uses demo data and does not reflect your company.",
  "网站 GEO 体检": "Website GEO Check",
  "AI 时代，客户越来越多直接问豆包 / DeepSeek / ChatGPT「XX 这家公司靠谱吗」。这里两步看你在 AI 眼里的样子：先":
    "In the AI era, more and more customers just ask Doubao / DeepSeek / ChatGPT “is company XX trustworthy?”. Two steps to see how you look in AI's eyes: first ",
  "免费自查": "free self-check",
  "搜全网，再让": " to search the whole web, then let ",
  "各家大模型给你打分": "the major LLMs score you",
  "——出一份可直接发客户的《AI 可见度报告》。": " — producing an “AI Visibility Report” you can send straight to customers.",
  "没找到 GEO 体检技能包（": "GEO check skill pack not found (",
  "）。请双击更新到最新版 U-King，或在「AI 技能包」页安装后再来。":
    "). Double-click to update to the latest U-King, or install it on the “AI Skill Pack” page and come back.",
  "公司 / 品牌名 ": "Company / brand name ",
  "例：贺去病AI工作室": "e.g. He Qubing AI Studio",
  "所在地区（可选）": "Region (optional)",
  "例：深圳宝安": "e.g. Bao'an, Shenzhen",
  "所在行业（可选）": "Industry (optional)",
  "例：AI培训": "e.g. AI training",
  "免费自查 · 40+ 渠道体检面板": "Free self-check · 40+ channel panel",
  "免费 · 不需 Key": "Free · no key needed",
  "在 AI 搜索 / AI 对话 / 传统搜索 / 社交 / 视频 / 百科 / 地图 里搜你的公司，逐个点「去查↗」打开真实搜索页自己看——有没有你、AI 认不认你，实时算出「互联网可见度」。":
    "Search your company across AI search / AI chat / traditional search / social / video / wiki / maps; click “Check ↗” on each to open the real search page and see for yourself — whether you're there, whether AI knows you — and get a live “web visibility” score.",
  "正在生成体检面板…": "Generating check panel…",
  "开始免费自查": "Start free self-check",
  "面板已生成 · 覆盖 {n} 个渠道": "Panel generated · covers {n} channels",
  "打开失败": "Failed to open",
  "重新打开": "Reopen",
  "AI 可见度测试 · 各家大模型给你打分": "AI visibility test · scored by the major LLMs",
  "免费出分": "Free score",
  "并行去问 GPT / Claude / Gemini / DeepSeek / 通义 等大模型「认不认识你公司、会不会推荐你」，聚合出一份可视化《AI 可见度报告》（总分 + 各家评分 + 普遍缺什么），可直接发客户。报告底部可":
    "Ask GPT / Claude / Gemini / DeepSeek / Tongyi and other LLMs in parallel whether they know your company and would recommend you, then aggregate a visual “AI Visibility Report” (overall score + per-model scores + what's commonly missing) you can send straight to customers. At the bottom of the report you can ",
  "支付宝下单，由我们帮你做 GEO + MEO 优化": "order via Alipay and have us do GEO + MEO optimization for you",
  "（让 AI 和地图都搜到你）。": " (so both AI and maps can find you).",
  "用 U-King 内置额度，一次约几分钱、无需自己配 Key。": "Uses U-King's built-in quota — a few cents per run, no key to configure yourself.",
  "正在问各家 AI（约 1 分钟）…": "Asking the AIs (about 1 minute)…",
  "测测各家 AI 认不认识你": "Test whether the AIs know you",
  "AI 可见度总分 {score}/100 · {label}": "AI visibility total {score}/100 · {label}",
  "报告已在浏览器打开——底部有「基础优化 / 持续优化」两档，可":
    "The report has opened in the browser — at the bottom there are two tiers, “Basic optimization / Ongoing optimization”, and you can ",
  "支付宝直接下单": "order directly via Alipay",
  "，由我们帮你做 GEO + MEO 优化（AI 可读企业主页 + 三大地图商家信息 + 每月复测追踪）。":
    ", and have us do GEO + MEO optimization for you (AI-readable company homepage + business listings on the three major maps + monthly re-test tracking).",
  "重新打开 AI 可见度报告": "Reopen AI visibility report",
  "提示：分数为本次抽样结果，AI 回答有波动，建议多次复测取趋势（搜不到 ≠ 不存在，只是还没被 AI 收录）。更多能力（生成 AI 可读企业主页、行业高频问答）在升级方案里。":
    "Note: the score is a sample from this run; AI answers vary, so re-test a few times to read the trend (not found ≠ nonexistent — just not indexed by AI yet). More capabilities (generating an AI-readable company homepage, industry FAQ) are in the upgrade plans.",

  // ── Experts.tsx ────────────────────────────────────────────────
  "AI 专家": "AI Experts",
  "精品技能，召唤即用 —— 点一个专家，带着它进 U-Workspace 直接干活出成果":
    "Curated skills, summon and go — pick an expert and take it into U-Workspace to get results",
  "技能市场 skillhub.cn": "Skill market skillhub.cn",
  "海量 AI 专家 / 技能包 —— 做视频、炒股看盘、写作排版、办公…注册后搜关键词一键装，装完回 U-Workspace 直接用":
    "Tons of AI experts / skill packs — video, stock watching, writing & layout, office work… sign up, search a keyword, install in one click, then use it back in U-Workspace",
  "精选场景": "Featured scenarios",
  "能力介绍": "What it does",
  "擅长领域": "Strengths",
  "试试这样问我": "Try asking me",
  "打开{name}": "Open {name}",
  "召唤 {role} → 进 U-Workspace 干活": "Summon {role} → work in U-Workspace",
  "全部": "All",
  "产品设计": "Product design",
  "内容创作": "Content creation",
  "效率办公": "Productivity",

  // ── opencodex/experts.ts（专家数据，Experts.tsx 渲染处 t() 包） ──
  // 专家名
  "网站设计专家": "Website Design Expert",
  "PPT·文档专家": "PPT·Docs Expert",
  "数据表格专家": "Data & Spreadsheet Expert",
  "海报·短视频专家": "Poster·Short-Video Expert",
  "AI 作图专家": "AI Image Expert",
  "AI 视频专家": "AI Video Expert",
  "简历专家": "Resume Expert",
  "文案专家": "Copywriting Expert",
  "翻译·润色专家": "Translation·Polishing Expert",
  // 职称
  "资深网页设计师": "Senior web designer",
  "PPT·文档顾问": "PPT·docs consultant",
  "数据分析师": "Data analyst",
  "视觉设计总监": "Visual design director",
  "AI 绘画师": "AI illustrator",
  "AI 视频师": "AI video maker",
  "求职顾问": "Career advisor",
  "新媒体文案": "Social media copywriter",
  "翻译专家": "Translation specialist",
  // 能力介绍 desc
  "澄清需求 → 信息架构 → 现代 Tailwind 风格的单页原型，hero 配图用 AI 生成，右侧实时预览，边看边改。":
    "Clarify needs → information architecture → a modern Tailwind-style single-page prototype, with AI-generated hero art and a live preview on the right so you can tweak as you go.",
  "先出大纲再逐页逐段填充，PPT 默认出**可直接打开的真 .pptx**、Word 出**真 .docx**（也能出 HTML/Markdown）。":
    "Outline first, then fill in page by page and paragraph by paragraph; by default PPT outputs a real, openable .pptx and Word a real .docx (HTML/Markdown also available).",
  "把杂乱数据整理成结构化表格，导出可直接打开、数字能求和的真 .xlsx；也能做多表报表、台账、清单。":
    "Turn messy data into structured tables, exported as a real, openable .xlsx where numbers can be summed; also does multi-sheet reports, ledgers, and lists.",
  "AI 出图做海报封面、AI 文生视频、AI 配音，成果自动进右侧预览。懂尺寸与提示词工作法。":
    "AI art for poster covers, AI text-to-video, AI voiceover — results flow into the right-side preview automatically. Knows sizing and prompt craft.",
  "把你的一句话扩成专业画面描述再出图，图进右侧预览可放大。懂提示词工作法。":
    "Expands your one-liner into a professional scene description before generating; images appear in the right preview and can be zoomed. Knows prompt craft.",
  "文字描述 → 视频，异步出片、落盘到工作区，成果可预览。":
    "Text description → video, rendered asynchronously and saved to the workspace, with a preview of the result.",
  "问清经历和目标岗位，产出结构清晰、有量化成果的简历，导出可直接投递的真 .docx。":
    "Clarify your background and target role, produce a clearly structured resume with quantified results, and export a real .docx ready to send.",
  "按平台调性写吸睛文案（标题/正文/话题标签/emoji），要配图就 AI 出图，成果进右侧预览。":
    "Write eye-catching copy in each platform's voice (title/body/hashtags/emoji); AI-generate images when needed, with results in the right-side preview.",
  "地道翻译（不生硬直译）、润色、改写、语气调整；长文可读文件、结果可存文件。":
    "Natural translation (no stiff word-for-word), polishing, rewriting, and tone adjustment; long text can be read from files and results saved to files.",
  // 一句话选它做什么 tagline
  "做网站 / 落地页 / H5，出可预览的成品": "Build websites / landing pages / H5, with previewable results",
  "做 PPT / 文档 / 报告，可预览可导出": "Make PPT / docs / reports, previewable and exportable",
  "整理数据 / 做报表，出真 Excel": "Organize data / build reports, output real Excel",
  "做海报 / 短视频素材 / 配音": "Make posters / short-video assets / voiceover",
  "专心作图，一句话出图": "Focused on images — one sentence, one picture",
  "文字生成视频，异步出片": "Text to video, rendered asynchronously",
  "帮你写简历，出真 Word": "Write your resume, output real Word",
  "小红书/公众号/朋友圈文案 + 配图": "RED / WeChat / Moments copy + images",
  "中英互译 / 润色 / 改写": "CN↔EN translation / polishing / rewriting",
  // 擅长领域 tags（含中文的）
  "网站设计": "Web design",
  "落地页": "Landing page",
  "文档": "Docs",
  "报告": "Report",
  "数据整理": "Data wrangling",
  "报表": "Reports",
  "台账": "Ledger",
  "海报": "Poster",
  "短视频": "Short video",
  "配音": "Voiceover",
  "封面": "Cover",
  "作图": "Image gen",
  "配图": "Illustration",
  "提示词": "Prompts",
  "视频": "Video",
  "文生视频": "Text-to-video",
  "简历": "Resume",
  "求职": "Job hunting",
  "小红书": "RED",
  "公众号": "WeChat OA",
  "文案": "Copy",
  "标题": "Headlines",
  "翻译": "Translation",
  "润色": "Polishing",
  "中英": "CN↔EN",
  // 试试这样问我 quickPrompts.template
  "帮我做一个奶茶店的落地页，清新风、有点单和门店位置":
    "Build me a bubble-tea shop landing page, fresh style, with a menu and store location",
  "帮我做一个极简的个人作品集单页，深色风": "Build me a minimal single-page personal portfolio, dark theme",
  "帮我做一个 SaaS 产品官网首页，含 hero、功能卡、定价、页脚":
    "Build me a SaaS product homepage with a hero, feature cards, pricing, and footer",
  "帮我做一份 8 页的创业路演 PPT，主题是 AI 办公助手":
    "Make me an 8-page startup pitch deck on the theme of an AI office assistant",
  "帮我把这周的工作整理成一份周报 Word 文档": "Turn this week's work into a weekly-report Word doc",
  "帮我做一份产品介绍 PPT，突出卖点和对比": "Make me a product intro deck highlighting selling points and comparisons",
  "帮我把这些销售数据整理成 Excel 报表：（把数据贴这里）":
    "Organize this sales data into an Excel report: (paste the data here)",
  "帮我做一个记账台账 Excel，含日期/项目/金额/分类": "Make me a bookkeeping ledger in Excel with date/item/amount/category",
  "帮我把这段文字里的信息整理成一张 Excel 表格：": "Organize the information in this text into an Excel table:",
  "帮我画一张周年庆活动海报，喜庆红金风": "Draw me an anniversary event poster in a festive red-and-gold style",
  "帮我做一张公众号封面图，科技风": "Make me a WeChat Official Account cover image, tech style",
  "帮我生成一段 5 秒的产品短视频片头": "Generate me a 5-second product short-video intro",
  "画一只戴墨镜的橘猫，卡通风格": "Draw an orange cat wearing sunglasses, cartoon style",
  "画一幅中国风水墨山水，留白，意境": "Paint a Chinese-style ink landscape, with negative space and mood",
  "画一座赛博朋克风格的夜晚城市，霓虹": "Draw a cyberpunk-style night city with neon",
  "生成一段小猫在月球上散步的视频": "Generate a video of a kitten walking on the moon",
  "生成一段咖啡杯在桌上冒热气的短视频": "Generate a short video of a coffee cup steaming on a table",
  "帮我写一份简历，目标岗位是（岗位），我的经历是：":
    "Write me a resume for the target role of (role); my background is:",
  "帮我优化这段工作经历，让它更有说服力：": "Improve this work experience to make it more persuasive:",
  "帮我写一篇小红书笔记，主题是（主题），带标题和话题标签":
    "Write me a RED post on the topic of (topic), with a title and hashtags",
  "帮我写一篇公众号推文，主题是：": "Write me a WeChat Official Account article on the topic:",
  "帮我写一条朋友圈文案，卖点是：": "Write me a Moments post; the selling point is:",
  "帮我把这段中文翻译成地道英文：": "Translate this Chinese into natural English:",
  "帮我把这段英文翻译成中文：": "Translate this English into Chinese:",
  "帮我润色这段文字，让它更专业通顺：": "Polish this text to make it more professional and fluent:",
};
