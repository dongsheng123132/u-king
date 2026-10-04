/**
 * English backfill for the remaining literal i18n keys found on 2026-08-20.
 *
 * Keep this separate from the older machine-generated backfill so the batch is
 * easy to audit or move into its owning module later. Module dictionaries are
 * spread after this file and may intentionally override individual wording.
 */
export const backfill20260820: Record<string, string> = {
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
  "；没配：{list}": "; not configured: {list}",
  "一个都没配上 —— 详情见下": "Nothing was configured — see details below",
  "一键导入": "Import all",

  "已保存": "Saved",
  "已暂停": "Paused",
  "上次失败": "Last run failed",
  "选一个工作文件夹": "Choose a working folder",
  "叫什么": "Name",
  "几点": "Time",
  "用哪个大脑": "AI model",
  "选": "Choose",


  "已让 Claude Code 用中文回答（下次开新会话生效；可在「进阶 → 安全卸载」里撤销）":
    "Claude Code will answer in Chinese in new sessions. You can undo this in “Advanced → Safe Uninstall.”",
  "设置失败: {e}": "Setup failed: {e}",
  "进程已退出，点这里重开": "Process exited — click to restart",
  "中文小抄": "Chinese quick guide",
  "往 ~/.claude/CLAUDE.md 追加一行「用简体中文回答」（只增不删，可在「进阶 → 安全卸载」里撤销）":
    "Append “Respond in Simplified Chinese” to ~/.claude/CLAUDE.md (adds one line only; reversible in “Advanced → Safe Uninstall”)",
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
  "关闭整个项目下的会话（会先问你，不动磁盘文件夹）":
    "Close all sessions in this project (asks first; does not touch the folder on disk)",
  "双击可重命名": "Double-click to rename",
  "关闭会话（聊过的会先问你，不会删除磁盘文件夹）":
    "Close session (asks first if it has messages; does not delete the folder on disk)",


  "重新显示新手引导": "Show onboarding again",
  "终端配色": "Terminal colors",
};
