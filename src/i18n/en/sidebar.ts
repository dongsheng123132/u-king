/** 英文覆盖字典 · 左侧边栏（Sidebar.tsx）。key = 中文原文，value = English。 */
export const sidebar: Record<string, string> = {
  // 品牌头
  "一键装好你的全部 AI": "Set up all your AI in one click",
  "收起侧栏（腾出空间给右侧终端/工作区）": "Collapse sidebar (free up space for the terminal / workspace)",
  "展开侧栏": "Expand sidebar",
  // 让步链（lib/yieldChain.ts）第二顺位：会话栏已收窄、终端仍排不开才轮到主侧栏
  "窗口太窄，已自动让位给终端 —— 点这里展开（窗口拉宽会自己还原）":
    "Window too narrow — collapsed to give the terminal room. Click to expand; widening the window restores it.",
  "展开侧栏，查看「更多」": "Expand sidebar to see “More”",

  // 核心 4 项 CORE（2026-07-19 信息架构梳理）
  "首页 · 我的 AI": "Home · My AI",
  "我的 AI": "My AI",
  "装机 · 一键配好 · 启动": "Install · One-click setup · Launch",
  "AI 创作": "AI Studio",
  "作图 · 视频 · 海报二维码": "Images · Video · Poster QR",
  "换模型 · 余额 · 用自己的 Key": "Switch models · Balance · Your own key",
  "换模型 · 余额 · 一键体检升级": "Models · Balance · One-click checkup & update",
  "虾盘云 · 充值": "Xiapan Cloud · Top up",
  "上手教程": "Getting Started",
  "技能市场 · 怎么用 AI": "Skill market · How to use AI",
  "挑个专家帮你干活": "Pick an expert to work for you",
  // 旧核心三步（标签已改版；词条保留兜底老引用）
  "① 装 AI": "① Install AI",
  "一键装好 · 主推 ClawX": "One-click install · ClawX recommended",
  "② 虾盘云·充值": "② Xiapan Cloud · Top up",
  "内置 Key · 充值 · 一键配好": "Built-in key · Top up · One-click setup",
  "③ 用 AI": "③ Use AI",
  "技能市场 · 上手教程": "Skill market · Getting-started guide",
  "对话 + 终端 + 作图，一站干活": "Chat + terminal + image gen, all in one",
  "U-Chat 对话 · U-CLI 终端 · 作图，一站干活": "U-Chat · U-CLI terminal · image gen, all in one",
  "AI 专家": "AI Experts",
  "挑个专家帮你干活 · 更多去 skillhub": "Pick an expert to work for you · more on skillhub",
  "Codex 工作站": "Codex Station",
  "安装 · 配置 · 模板": "Install · Configure · Templates",

  // 更多 MORE
  "更多": "More",

  // 实验室 LAB（2026-07-27 做减法：没毕业的功能收进这里）
  "实验室": "Labs",
  "测试中": "beta",
  "这些还没做完，可能不稳定、可能随时改。能用，但别当主力。":
    "These aren't finished — they may be unstable and may change without notice. Usable, but don't rely on them.",
  "AI 优化大师": "AI Optimizer",
  "让 AI 工具跑得更稳、更省 token": "Make AI tools run steadier and cheaper on tokens",
  "Token 压缩机": "Token Squeezer",
  "AI 编程省 token · 不降智 · 开源 RTK": "Save coding tokens · no quality loss · open-source RTK",
  "AI 设置": "AI Settings",
  // 「让 AI 认识 U-King」侧栏条目（页面正文的翻译在 en/identity.ts）
  "让 AI 认识 U-King": "Let AIs discover U-King",
  "往 CLAUDE.md 插一行指针 · 随时可撤": "Adds one pointer line to CLAUDE.md · undo anytime",
  "起名 · 身份 · 给 AI 的说明书": "Name · Identity · The manual for AIs",
  "换模型 · 余额 · 每个工具单独配": "Switch models · Balance · Configure each tool",
  "AI 作图": "AI Image",
  "虾盘云出图 · 输入即画 · 可拖参考图": "Generate via Xiapan Cloud · type to draw · drag a reference",
  "AI 海报二维码": "AI Poster QR",
  "AI 出图 + 换成你的真二维码 · 可扫": "AI art + your real QR code · scannable",
  "AI 视频": "AI Video",
  "文字生成视频 · 异步出片": "Text to video · async rendering",
  "网站GEO体检": "Website GEO Check",
  "各家 AI 认不认识你 · 免费自查 + AI 可见度测试": "Do AIs know you · free self-check + AI visibility test",
  "AI 技能包": "AI Skill Pack",
  "给 Claude/ClawX 装作图能力 · 装完说「画图」": "Add image skills to Claude/ClawX · then say “draw”",
  "厨具工具箱": "Toolbox",
  "给 AI 装 ffmpeg/Chrome 等能力工具": "Install ffmpeg/Chrome and other tools for AI",
  "本地大模型": "Local LLM",
  "离线免费 · 自己电脑跑 AI": "Offline & free · run AI on your own PC",
  "备份/同步": "Backup / Sync",
  "对话设置存 U 盘 · 多电脑切换": "Save chats & settings to USB · switch across PCs",
  "进阶 / App 版": "Advanced / App",
  "Hermes/ClawX 桌面版 · 手动配": "Hermes/ClawX desktop · manual setup",
  "最新动态": "What's New",
  "新功能 · 活动 · 公告": "Features · Events · Announcements",
  "AI 学院": "AI Academy",
  "教程 · 玩法 · 进阶课": "Tutorials · Tips · Advanced courses",

  // 升级入口
  "正在升级…": "Upgrading…",
  "有新版 v{ver}，点此升级": "New version v{ver} available — click to upgrade",
  "升级到 v{ver}": "Upgrade to v{ver}",
  "升级中 {pct}%": "Upgrading {pct}%",
  "升级中…": "Upgrading…",
  "有新版 · 一键升级": "Update · One-click upgrade",
  // 自动升级失败后的兜底入口（覆盖安装）
  "下载安装包重装": "Download installer & reinstall",
  "自动升级失败过 {n} 次，改用官网安装包覆盖安装（配置不会丢）":
    "Auto-update failed {n} time(s) — reinstall over the top with the official installer (your settings are kept)",
  "自动升级失败过 {n} 次 —— 点此下载官网安装包覆盖安装":
    "Auto-update failed {n} time(s) — click to download the official installer",
  "自动升级没换成功（{why}）。装新版不会丢配置和对话。":
    "Auto-update could not replace the app ({why}). Reinstalling keeps your settings and chats.",
  "原因未知": "reason unknown",

  // 主题 & 页脚
  "浅色模式": "Light mode",
  "深色模式": "Dark mode",
  "夜间开": "Night on",
  "白天": "Day",
  "打开官网": "Open website",
  "官网": "Website",

  // 语言切换
  "语言": "Language",
  "中文": "中文",
  "English": "English",

  // 技术支持（侧栏底部常驻入口）
  "技术支持": "Support",
  "技术支持 · 报告问题 · 加微信找我们": "Support · Report an issue · Reach us on WeChat",

  // 小程序动态区（用户自己装的，能删）
  "小程序": "Mini-apps",
  "你自己装的，能删。删掉它注册的动作也会一起从 AI 那儿消失。":
    "Yours to keep or remove. Deleting one also removes the actions it gave your AI.",
  "删掉这个小程序": "Remove this mini-app",
  "删除小程序": "Remove mini-app",
  "删掉小程序「{name}」？它注册的 {n} 个动作会同时从动作表里消失（AI 也就调不到了）。你的文件不会被删，随时可以「补装内置」装回来。":
    "Remove the mini-app “{name}”? The {n} action(s) it registers disappear from the action table too, so your AI can no longer call them. Your files are kept, and “Restore built-ins” brings it back any time.",
  "已删掉「{name}」": "Removed “{name}”",
  "装着但读不出：{err}": "Installed but unreadable: {err}",
  "补装内置（少了 {n} 个）": "Restore built-ins ({n} missing)",
  "补装了 {n} 个内置小程序": "Restored {n} built-in mini-app(s)",
  "内置小程序都在，没什么要补的": "All built-in mini-apps are present — nothing to restore",

  "做一个自己的小程序（复制给 AI）": "Make your own mini-app (copy for your AI)",
  "已复制，粘给 U-Chat 里的 AI 就行": "Copied — paste it to the AI in U-Chat",

  // DSH 插件页（2026-08-18 新增；不叫「小程序」——两者安全模型相反）
  "DSH 插件": "DSH plugins",
  "打开 DeepSeek Harness · 给它装插件": "Open DeepSeek Harness · install plugins into it",
  "DeepSeek Harness": "DeepSeek Harness",
  "DeepSeek 官方的 AI 工作台，我们已内置。默认接好虾盘云，打开就能用。":
    "DeepSeek's own AI workbench, bundled with U-King. Xiapan Cloud is wired up by default — just open it.",
  "打开 DSH": "Open DSH",
  "先去装 DSH": "Install DSH first",
  "插件": "Plugins",
  "装进 DSH，不是装进 U-King": "installed into DSH, not into U-King",
  "装到 DSH": "Install into DSH",
  "先装 DSH，再装它的插件": "Install DSH first, then its plugins",
  "装好了 —— 回 DSH 里就能用": "Installed — it's ready next time you open DSH",
  "看源码": "View source",
  "缓存前缀稳定": "Stable cache prefix",
  "让 DSH Web 的请求前缀稳定下来，缓存命中率更高 —— 直接省 token。装机时默认就装了这个。":
    "Keeps DSH Web's request prefix stable so caching hits more often — straight token savings. Installed by default during setup.",
  "持续对话终端": "Persistent chat terminal",
  "在终端里连续对话不掉上下文，另带缓存统计。":
    "Keeps context across turns in the terminal, plus cache statistics.",

  "去社区清单挑插件": "Browse the community plugin list",
  "社区维护的 DSH 插件清单（8.6k star，每日自动抓取 + 人工核实）。挑好之后，复制它的仓库地址回来装。":
    "A community-maintained list of DSH plugins (8.6k stars, auto-crawled daily and hand-checked). Pick one, then paste its repo address back here to install.",
  "U-King 装机时自带的两个：": "The two U-King installs by default:",
};
