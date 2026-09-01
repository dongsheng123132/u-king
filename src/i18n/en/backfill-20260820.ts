/**
 * English backfill for the remaining literal i18n keys found on 2026-08-20.
 *
 * Keep this separate from the older machine-generated backfill so the batch is
 * easy to audit or move into its owning module later. Module dictionaries are
 * spread after this file and may intentionally override individual wording.
 */
export const backfill20260820: Record<string, string> = {
  "自动化「{name}」跑完了：{s}": "Automation “{name}” finished: {s}",
  "自动化「{name}」没跑成：{s}": "Automation “{name}” failed: {s}",
  "还没拿到服务器版本信息（可能网络不通），请稍等几秒再试":
    "Version information is not available yet (the network may be offline). Wait a few seconds and try again.",
  " —— 左下角按钮已切换成「下载安装包重装」，点它即可（配置和对话不会丢）。":
    " — The button in the lower-left is now “Download installer and reinstall.” Click it to continue; your settings and chats will be kept.",
  "正在下载官网安装包…": "Downloading the official installer…",
  "安装包已下载到 {p}，正在打开安装程序 —— U-King 会先退出，一路「下一步」装完会自动打开新版（配置和对话不会丢）":
    "Installer downloaded to {p}. Opening it now — U-King will exit first. Continue through the installer and the new version will open automatically; your settings and chats will be kept.",
  "安装包下载失败：": "Installer download failed: ",
  " —— 已为你打开官网下载页，手动下载安装即可":
    " — The official download page is open. Download and install it manually.",
  "已检测到 uu-switch，正在打开…": "uu-switch detected. Opening it…",
  "开始下载并安装 uu-switch（约 12 MB）…": "Downloading and installing uu-switch (about 12 MB)…",
  "，可到工具卡点「打开下载页」手动装": ". You can use “Open download page” on the tool card to install it manually",
  "；没配：{list}": "; not configured: {list}",
  "一个都没配上 —— 详情见下": "Nothing was configured — see details below",
  "正在导入到 uu-switch（虾盘云 + 你在用的配置）…":
    "Importing into uu-switch (Xiapan Cloud + your active configurations)…",
  "导入 uu-switch 失败：": "Failed to import into uu-switch: ",
  "一键导入到 uu-switch（虾盘云 + 在用配置）":
    "Import into uu-switch (Xiapan Cloud + active configurations)",
  "一键把虾盘云(Claude+Codex) + 你在用的工具配置导入 uu-switch":
    "Import Xiapan Cloud (Claude + Codex) and your active tool configurations into uu-switch",
  "一键导入": "Import all",

  "选工作副本根目录": "Choose workspace-copy root",
  "竞技场跑完，看结果打星": "Arena finished — review and rate the results",
  "竞技场失败": "Arena failed",
  "竞技场": "Arena",
  "六个 CLI 同任务横向比 —— 系统只出可观测量（耗时 / 退出码 / 有没有产出），质量由你打星":
    "Run the same task across six CLIs. The system reports only observable facts (time, exit code, and output); you rate the quality.",
  "参赛选手": "Participants",
  "同一个任务": "Same task",
  "给所有参赛者的同一个任务，例如：把这个目录的 README 翻译成英文":
    "Give every participant the same task, for example: translate this folder’s README into English",
  "工作副本根目录": "Workspace-copy root",
  "每个参赛者一个独立子目录，互不踩文件":
    "Each participant gets a separate subfolder so their files never conflict",
  "选目录": "Choose folder",
  "留空用当前工作目录。每个参赛者各开一个子目录，不直接共享。":
    "Leave blank to use the current working folder. Each participant gets a separate subfolder; nothing is shared directly.",
  "开赛中…": "Starting…",
  "开赛（会烧 token）": "Start arena (uses tokens)",
  "打星口径：跑得快不等于干得好。先看有没有真产出、退出码是否干净，再自己核对 stdout，最后打星。":
    "Rating guide: fast is not the same as good. Check for real output and a clean exit code, inspect stdout yourself, then rate the result.",
  "勾选参赛者，塞同一个任务，点开赛": "Select participants, enter one task, then start the arena",
  "比的是同一个任务谁干活利索 —— 结果只列可观测量，质量靠人打星":
    "See who handles the same task best. Results show observable facts only; quality is rated by you.",
  "选手": "Participant",
  "耗时": "Time",
  "退出码": "Exit code",
  "产出": "Output",
  "stdout 尾部": "stdout tail",
  "打星": "Rating",
  "超时": "Timed out",
  "有": "Yes",

  "读不到自动化列表：{e}": "Could not load automations: {e}",
  "已保存": "Saved",
  "删掉「{name}」？已经跑出来的结果留在磁盘上，不会删。":
    "Delete “{name}”? Existing results will remain on disk.",
  "「{name}」开跑了，出结果要等一会儿": "“{name}” is running. Results may take a while.",
  "没跑成：{e}": "Failed: {e}",
  "自动化": "Automations",
  "到点了让 AI 自己把活干了 —— 每天的文案、配图、周报，不用你记着点一下":
    "Let AI do the work on schedule — daily copy, images, and reports without you remembering to click anything.",
  "已经到上限 {n} 条": "Limit reached ({n})",
  "新建自动化": "New automation",
  "{n} 条自动化，{on} 条开着。": "{n} automations, {on} enabled.",
  "注意：只有 U-King 开着（缩在托盘里也算）才会到点执行；关了电脑错过的班次不补跑。":
    "U-King must be running (the tray counts) for scheduled jobs to run. Jobs missed while the PC is off are not replayed.",
  "现在配了也不会真跑：": "It cannot run yet: ",
  "还没有自动化": "No automations yet",
  "上面挑个模板，或者点「新建自动化」自己写一条":
    "Choose a template above, or select “New automation” to create one.",
  "点一下暂停": "Click to pause",
  "点一下启用": "Click to enable",
  "已授权它无人值守地读写这个文件夹、在里面跑命令：{dir}":
    "Authorized for unattended file access and command execution in: {dir}",
  "可动文件": "Folder access",
  "已暂停": "Paused",
  "上次失败": "Last run failed",
  "上次 {t} 成功": "Last succeeded {t}",
  "（看结果）": "(view result)",
  "现在就跑一次（不影响排期）": "Run now (does not change schedule)",
  "选一个工作文件夹": "Choose a working folder",
  "编辑自动化": "Edit automation",
  "叫什么": "Name",
  "每天早报": "Daily briefing",
  "到点了让 AI 干什么（写清楚，它没法反问你）":
    "What should AI do on schedule? Be specific; it cannot ask follow-up questions.",
  "把今天值得关注的 AI 动态整理成 5 条要点…":
    "Summarize today’s notable AI developments in five points…",
  "它不会上网 —— 「整理今天的新闻/行情」这类活它只会编。要基于真实资料，就把资料放进下面的工作文件夹让它读。":
    "It cannot access the web. Tasks such as “summarize today’s news or markets” will be fabricated unless you place source material in the working folder below.",
  "什么时候跑": "Schedule",
  "每隔": "Every",
  "分钟（最少 5）": "minutes (minimum 5)",
  "几点": "Time",
  "（本机时间）": "(local time)",
  "用哪个大脑": "AI model",
  "工作文件夹（可不填）": "Working folder (optional)",
  "不填 = 只让它作图/生成视频，碰不到你的文件":
    "Leave blank to allow only image/video generation, with no file access",
  "选": "Choose",
  "填了文件夹 = 你允许它在没人盯着的情况下，读写这个文件夹里的文件、在里面跑命令。只填你放心的目录。":
    "Choosing a folder allows unattended file access and command execution inside it. Select only a folder you trust.",

  "召唤": "Hire",
  "搜专家名称、职称或擅长的活": "Search by expert name, role, or specialty",
  "热门": "Popular",
  "没搜到匹配的专家，换个词试试": "No matching experts. Try another search.",
  "自带技能 · 召唤后即可用": "Built-in skills · ready after hiring",
  "召唤 {role} → 开一个会话": "Hire {role} → start a session",

  "已让 Claude Code 用中文回答（下次开新会话生效；在「我的 U-King」可撤销）":
    "Claude Code will answer in Chinese in new sessions. You can undo this in “My U-King.”",
  "设置失败: {e}": "Setup failed: {e}",
  "进程已退出，点这里重开": "Process exited — click to restart",
  "中文小抄": "Chinese quick guide",
  "往 ~/.claude/CLAUDE.md 追加一行「用简体中文回答」（只增不删，可在「我的 U-King」里撤销）":
    "Append “Respond in Simplified Chinese” to ~/.claude/CLAUDE.md (adds one line only; reversible in “My U-King”)",
  "让 AI 说中文": "Ask AI to speak Chinese",
  "不再显示": "Do not show again",

  "这张护照没写工作目录 —— 选一个让接手方在哪儿干活":
    "This passport has no working folder. Choose where the next AI should work.",
  "交接失败：{e}": "Handoff failed: {e}",
  "一件事做到哪了，以及交给下一个 AI 接着干 —— 只传已验证事实，不传聊天记录。":
    "Record where a task stands and hand it to another AI. Only verified facts are passed on, never chat history.",
  "重新读一遍护照": "Reload passports",
  "这次没能读到任务护照 —— 下面显示的是上一次读到的，可能已经过期。":
    "Could not load task passports. The cached copy below may be outdated.",
  "正在读任务护照…": "Loading task passports…",
  "还没有任务护照。": "No task passports yet.",
  "对任意已接入 U-King 的 AI 说一句：为当前目标创建一张任务护照。":
    "Tell any AI connected to U-King: create a task passport for the current goal.",
  "护照存在 ~/.uking/origin/，不是聊天记录：换个 AI、换台会话、隔几天回来，接手方读到的是同一份「世界此刻是什么样」。":
    "Passports live in ~/.uking/origin/, separate from chat history. Switch AI, start another session, or return days later—the next AI reads the same current state.",
  "（这张护照没写目标）": "(no goal in this passport)",
  "尚未标记接手方": "No assignee yet",
  "{n} 条已验证事实": "{n} verified facts",
  "{n} 步待办": "{n} next steps",
  "已交给 {who} · 会话「{s}」": "Handed to {who} · session “{s}”",
  "正在送往 {who} · 会话「{s}」": "Sending to {who} · session “{s}”",
  "打开会话 →": "Open session →",
  "交给谁接着干？会在护照的工作目录里开一个会话，并把状态发进去。":
    "Who should continue? A session will open in the passport’s working folder and receive its current state.",
  "这台机器上还没装": "Not installed on this PC",
  "护照列表": "Passport list",
  "复制护照号": "Copy passport ID",
  " · 上次由 {h} 写入": " · last updated by {h}",
  "工作目录：{d}": "Working folder: {d}",
  "目标": "Goal",
  "（没写目标 —— 这张护照交接不出去）": "(no goal — this passport cannot be handed off)",
  "世界此刻": "Current state",
  "验证到哪了": "Verification",
  "下一步": "Next steps",
  "空的 —— 接手方拿到手还是得回头问人":
    "Empty — the next AI would still need to ask for context",
  "已知事实（✓=机器复验过，?=只是说法）": "Known facts (✓ machine-verified, ? unverified claim)",
  "出处": "Source",
  "已定的事（含理由，别重新纠结）": "Decisions made (with reasons; do not reopen)",
  "因为": "Because",
  "已产出": "Outputs",

  "关闭这个会话？它有 {n} 条对话记录，关掉就找不回来了。\n（磁盘上的文件夹不动，只是这个会话和它的聊天记录没了）":
    "Close this session? Its {n} chat messages cannot be recovered.\n(The folder on disk stays; only this session and its chat history are removed.)",
  "关闭这个项目下的 {c} 个会话？其中共有 {n} 条对话记录，关掉就找不回来了。\n（磁盘上的文件夹不动）":
    "Close {c} sessions in this project? Their {n} chat messages cannot be recovered.\n(The folder on disk stays.)",
  "关闭这个项目下的 {c} 个会话？（都还没聊过；磁盘上的文件夹不动）":
    "Close {c} sessions in this project? (They have no messages; the folder on disk stays.)",
  "有 {n} 个上次没跑成": "{n} failed last time",
  "关闭整个项目下的会话（会先问你，不动磁盘文件夹）":
    "Close all sessions in this project (asks first; does not touch the folder on disk)",
  "双击可重命名": "Double-click to rename",
  "关闭会话（聊过的会先问你，不会删除磁盘文件夹）":
    "Close session (asks first if it has messages; does not delete the folder on disk)",

  "这台电脑上所有 AI 的会话：谁在跑、谁跑完、谁挂了。任务状态在左栏「护照」。":
    "All AI sessions on this PC: what is running, ended, or failed. Task status is under “Passports” on the left.",
  "{d} 天": "{d} days",
  "重新扫一遍本机各家 AI 的任务": "Rescan local AI sessions",
  "还有 {n} 条 ↓": "{n} more ↓",
  "会话文件太多，只取了最新的那批 —— 更早的没扫。":
    "There are too many session files, so only the newest batch was scanned.",
  "正在扫本机各家 AI 的任务记录…": "Scanning local AI session records…",
  "没能读到本机其它 AI 的任务记录（下面只显示本工作台的会话）。":
    "Could not read other local AI session records. Only workbench sessions are shown below.",
  "这台电脑上还发现了 {n} 条别的 AI 的任务，要一起显示在看板上吗？":
    "Found {n} sessions from other AI tools on this PC. Show them on the board too?",
  "加到看板": "Add to board",
  "先不用": "Not now",
  "之后随时能在上面那排来源里改": "You can change this later using the source filters above",
  "这条记录里没有工作目录": "This record has no working folder",
  "点它：在这个文件夹开个会话，并把「{cmd}」贴进终端（不替你回车）":
    "Click to open a session in this folder and paste “{cmd}” into the terminal (it will not press Enter)",
  "这家工具没有可靠的续接命令，点它只开文件夹":
    "This tool has no reliable resume command; clicking only opens the folder",
  "接着干 ↩": "Resume ↩",
  "只开文件夹": "Open folder only",

  "挑一个专家 → 当场在这个工作台开一个绑好它的会话，直接干活出成果":
    "Pick an expert → open a ready-to-use session in this workbench and start producing results",
  "重新显示新手引导": "Show onboarding again",
  "终端配色": "Terminal colors",
};
