/** 英文覆盖字典 · Token 压缩机（TokenSqueezer / RTK）。「Token 压缩机」标题本身在 sidebar.ts 已译。 */
export const rtk: Record<string, string> = {
  "安装失败：": "Install failed: ",
  "切换失败：": "Toggle failed: ",
  "卸载失败：": "Uninstall failed: ",
  "（重启 Claude Code 后生效）": " (takes effect after restarting Claude Code)",
  "AI 编程时把啰嗦的命令输出压扁，省 token 不降智 · 基于开源 RTK":
    "Squeezes verbose command output while coding — saves tokens with no quality loss · powered by open-source RTK",
  "已开启": "On",
  "你的 AI 编程时会反复跑 git / 测试 / 构建等命令，那些啰嗦的输出被一遍遍喂给大模型，白烧 token。开启后由 RTK 把这些输出压扁再喂——实测综合省 40~70%，重度测试场景 80~90%。":
    "While coding, your AI repeatedly runs git / test / build commands whose verbose output gets fed to the model over and over, burning tokens. Once on, RTK compresses that output first — measured 40–70% overall, 80–90% on test-heavy sessions.",
  "git status −50%": "git status −50%",
  "测试 −90%": "tests −90%",
  "ls / 读文件 −70%": "ls / file reads −70%",
  "已紧凑的不硬压 · 不丢报错": "won't over-compress tight output · never drops errors",
  "安装中…": "Installing…",
  "安装 Token 压缩机（约 4MB）": "Install Token Squeezer (~4 MB)",
  "已开启 · 正在为 Claude Code 省 token": "On · saving tokens for Claude Code",
  "已安装，未开启": "Installed, not enabled",
  "Claude Code 跑命令时自动压缩输出 · 只砍噪音，报错/diff 全留 · 改动配置后需重启 Claude Code 生效":
    "Auto-compresses command output in Claude Code · cuts only noise, keeps all errors/diffs · restart Claude Code to apply",
  "点右侧开关开启（会往 ~/.claude 加一条 hook，卸载可完全清除）":
    "Flip the switch to enable (adds one hook to ~/.claude; fully removed on uninstall)",
  "点击关闭": "Click to turn off",
  "点击开启": "Click to turn on",
  "你的真实战绩（数据来自 rtk gain，不是估算）": "Your real stats (from rtk gain, not an estimate)",
  "累计已省": "Saved so far",
  "平均压缩率": "Avg compression",
  "已优化命令": "Commands optimized",
  "条": "",
  "刚开启，还没积累数据 —— 用 Claude Code 跑几条命令后，这里显示真实省了多少。":
    "Just enabled, no data yet — run a few commands in Claude Code and your real savings will show here.",
  "先开启，用一阵后这里会显示真实省了多少 token。":
    "Enable it first; after a while your real token savings will show here.",
  "≈ 少充 ¥{y}（按虾盘云 ¥1≈50 万 token 估；用贵模型省得更多）":
    "≈ ¥{y} less to top up (at Xiapan ¥1 ≈ 500k tokens; pricier models save more)",
  "默认保守档：只砍重复日志、通过的测试列表、进度条等噪音；报错、失败、diff、告警一个不丢，所以省 token 不降智。目前接管 Claude Code。":
    "Conservative by default: cuts only repeated logs, passing-test lists, progress bars and other noise; keeps every error, failure, diff and warning — so you save tokens with no quality loss. Currently covers Claude Code.",
  "卸载（清除 rtk + hook）": "Uninstall (remove rtk + hook)",
};
