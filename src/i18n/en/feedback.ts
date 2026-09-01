/** 英文覆盖字典 · 技术支持（Feedback.tsx）+ 首页卡片「卸载」（App.tsx）。key = 中文原文。 */
export const feedback: Record<string, string> = {
  // 远程协助（Feedback.tsx 底部区块）
  "远程协助（需要时再开）": "Remote assistance (only when you need it)",
  "两种方式：① 让作者跑命令排查（不用装东西，查配置/日志最快）；② 让作者看到你的屏幕（界面点不动、弹窗看不懂时用）。":
    "Two ways: ① let the author run commands (nothing to install — fastest for config/log issues); ② let the author see your screen (for when a button won't respond or a dialog makes no sense).",
  "① 让作者跑命令排查（U-King 自带）": "① Let the author run commands (built into U-King)",
  // 屏幕协助（UU远程）—— 官方无绿色版是硬事实，翻译里也别软化成 \"lightweight\"
  "② 让作者看到你的屏幕（UU远程）": "② Let the author see your screen (UU Remote)",
  "已安装": "Installed",
  "界面点不动、弹窗看不懂、装到一半卡住 —— 这类问题命令查不出来，得让作者直接看你的屏幕。用网易官方的 UU远程，我们帮你下好装上。":
    "A button that won't respond, a dialog you can't read, an install stuck halfway — commands can't diagnose these; the author needs to see your screen. We'll download and install NetEase's official UU Remote for you.",
  "· 官方没有免安装的绿色版，只有安装包（约 86 MB），所以需要装一次。":
    "· There is no portable/no-install build — the vendor ships an installer only (~86 MB), so it has to be installed once.",
  "· 装完打开 UU远程 →「远程协助」，把上面的 ID 和验证码发给作者就能连。你随时可以在它界面里断开。":
    "· After installing, open UU Remote → “Remote assistance”, then send the author the ID and verification code shown there. You can disconnect from its window at any time.",
  "帮我下载安装（约 86 MB）": "Download & install for me (~86 MB)",
  "正在下载安装…": "Downloading & installing…",
  "打开官网下载页": "Open the official download page",
  "安装失败：": "Install failed: ",
  "打开失败，请手动访问 {url}": "Couldn't open it — please visit {url} manually",
  "装不上、报错说不清、截图看不出问题时，可以让作者直接连上你的电脑排查，不用你再截图描述。":
    "When an install fails or an error is hard to describe, you can let the author connect to your computer and look directly — no more screenshots.",
  "开启后，作者可以在你这台电脑上执行命令、读取文件来排查问题。请只在你正在联系作者时开启。":
    "Once enabled, the author can run commands and read files on this computer to diagnose the problem. Only enable it while you are actively in touch with the author.",
  "· 你随时可以点「停止协助」立刻断开；{h} 小时后也会自动断开。":
    "· You can hit “Stop assistance” to disconnect instantly at any time; it also disconnects automatically after {h} hours.",
  "· 作者执行过的每条命令都会记进本机审计日志，你可以随时查看。":
    "· Every command the author runs is written to a local audit log you can inspect at any time.",
  "开启远程协助": "Enable remote assistance",
  "停止协助": "Stop assistance",
  "查看审计日志": "View audit log",
  "把这个编号发给作者：": "Send this code to the author:",
  "点此复制": "Click to copy",
  "复制": "Copy",
  "约 {m} 分钟后自动断开": "Disconnects automatically in about {m} min",
  "远程协助已开启，请把协助编号发给作者": "Remote assistance is on — send the code to the author",
  "已停止远程协助": "Remote assistance stopped",
  "已复制协助编号：{id}": "Copied assistance code: {id}",
  "开启失败：": "Failed to enable: ",
  "停止失败：": "Failed to stop: ",

  // Feedback 页
  "技术支持 · 报告问题": "Support · Report an issue",
  "直接找我们": "Reach us directly",
  "微信": "WeChat",
  "点此复制微信号": "Click to copy the WeChat ID",
  "（点微信号可复制）": "(click the ID to copy)",
  "扫码加好友": "Scan to add",

  "微信二维码": "WeChat QR code",
  "微信号": "WeChat ID",
  "点开看大图": "Click to enlarge",
  "微信扫一扫，加我为朋友": "Scan with WeChat to add me as a friend",
  "手机扫左边这张码，直接到「添加朋友」· 点码可看大图":
    "Scan the code on the left with your phone to land straight on \"Add friend\" · click it to enlarge",
  "关闭": "Close",

  "最快的一条路 · 点这里复制微信号，加好友直接说问题":
    "The fastest route · click to copy the WeChat ID, add us and just describe the problem",
  "微信号已复制：{id}，加我直接说问题": "WeChat ID copied: {id} — add us and just describe the problem",
  "请手动添加微信：{id}": "Please add us on WeChat manually: {id}",
  "遇到 bug、有建议，都可以在这里告诉我们。日志会脱敏后再发送。":
    "Found a bug or have a suggestion? Tell us here. Logs are redacted before sending.",
  "联系作者": "Contact the author",
  "点此复制邮箱": "Click to copy the email",
  "（点邮箱可复制）": "(click the email to copy)",
  "说说你遇到的问题或建议": "Describe the problem or suggestion",
  "例如：装 Claude 一直失败 / 作图点了没反应 / 希望能加某个功能…":
    "e.g. Claude keeps failing to install / image gen does nothing / please add a feature…",
  "附带脱敏诊断日志（版本 / 系统 / 装机状态 / 报错尾部，已抹掉 Key 与隐私）":
    "Attach redacted diagnostics (version / OS / install status / error tail — keys & privacy removed)",
  "采集中…": "Collecting…",
  "预览": "Preview",
  "刷新": "Refresh",
  "一键提交反馈": "Submit feedback",
  "发邮件给作者": "Email the author",
  "打开日志文件夹": "Open log folder",
  "复制脱敏诊断": "Copy redacted diagnostics",
  "「一键提交」会把反馈发给作者（自动带脱敏诊断）；也可「发邮件给作者」直接联系。日志需要时点「打开日志文件夹」，把里面的文件拖进邮件附件即可。":
    "“Submit” sends your feedback to the author (with redacted diagnostics); or use “Email the author” directly. For logs, click “Open log folder” and drag the files into the email.",

  // Feedback toasts
  "（诊断采集失败）": "(failed to collect diagnostics)",
  "请先写一句你遇到的问题或建议": "Please write the problem or suggestion first",
  "提交失败：": "Submit failed: ",
  "U-King 反馈 v{v}": "U-King feedback v{v}",
  "（如需附日志，请点页面「打开日志文件夹」把日志文件拖进邮件附件）":
    "(To attach logs, click “Open log folder” and drag the log files into the email)",
  "打开邮件失败，可手动发到 {email}": "Failed to open email — you can write to {email} manually",
  "已复制脱敏诊断，可贴到邮件/微信": "Redacted diagnostics copied — paste into email / WeChat",
  "复制失败，请手动选择复制": "Copy failed — please select and copy manually",
  "已复制邮箱：{email}": "Email copied: {email}",
  "已打开日志文件夹": "Opened the log folder",
  "打开失败：": "Failed to open: ",

  // 首页卡片「卸载」（App.tsx）
  "卸载": "Uninstall",
  "彻底卸载 {name}（含 U-King 相关残留清理）": "Fully uninstall {name} (incl. U-King leftover cleanup)",
  "确定卸载「{name}」吗？\n\n会删掉它本体，以及 U-King 相关残留（让它不再被检测成「已装」）。\n若你之前是自己装的、其它软件也在用，请勿卸载。":
    "Uninstall “{name}”?\n\nThis removes the tool itself and U-King's leftovers (so it's no longer detected as installed).\nIf you installed it yourself or other apps use it, do NOT uninstall.",
  "正在卸载 {name}…": "Uninstalling {name}…",
  "卸载失败：": "Uninstall failed: ",
};
