/**
 * Dynamic UI strings that are passed to `t()` through data tables rather than
 * literal calls. The static missing-key scanner cannot discover these, so the
 * Chromium English UI smoke test is their regression gate.
 */
export const englishUi: Record<string, string> = {
  // Compact copy: this button is only 28px tall in the 190px session rail.
  "新建项目（选文件夹）": "New project",

  // U-Workspace view table (SessionList.NAV).
  "任务护照：一件事做到哪了，交给 Claude / DeepSeek / Codex 接着干":
    "Task passports: record progress and hand work to Claude, DeepSeek, or Codex",
  "这台电脑上所有 AI 的会话「谁在跑 / 谁跑完 / 谁挂了」+ 定时任务":
    "All AI sessions on this PC—running, ended, or failed—plus scheduled jobs",
  "挑个专家，当场在这里开会话干活": "Pick an expert and start a ready-to-use session here",
  "定时任务：到点了让 AI 自己把活干了": "Scheduled jobs: let AI work automatically on time",

  // Task-board source table.
  "本工作台": "This workbench",
  "点一下隐藏这个来源": "Click to hide this source",
  "点一下显示这个来源": "Click to show this source",

  // Automation template table.
  "每天学一招": "Learn one skill daily",
  "每天一条文案": "Daily copy",
  "每周周报": "Weekly report",
  "每天出一张图": "Create one image daily",

  // Expert skill labels.
  "AI 作图 / 视频": "AI images / video",
  "文生图、图生图、文生视频、配音、批量出片":
    "Text-to-image, image-to-image, text-to-video, voiceover, and batch creation",
  "建站": "Website builder",
  "单文件出完整网页，顶栏可直接预览": "Build a complete single-file webpage with instant preview",
  "做 PPT": "Create PowerPoint",
  "出有设计感的真 .pptx（内置主题 + 5 种版式）":
    "Create a polished .pptx with built-in themes and five layouts",
  "写 Word": "Create Word documents",
  "出可直接打开的 .docx 文档": "Create a ready-to-open .docx document",
  "出可直接打开的 .xlsx 表格": "Create a ready-to-open .xlsx workbook",
  "读文档": "Read documents",
  "看懂客户手上的 Word/Excel/PPT/PDF，先摘要点再回答":
    "Read Word, Excel, PowerPoint, and PDF files; summarize before answering",
  "改文档": "Edit documents",
  "在客户原文件上改字，页眉页脚/样式/图片一个字节都不动":
    "Edit text in the original file while preserving headers, footers, styles, and images",

  // Built-in experts. Persona text is sent to the model, not rendered in UI.
  "AI 优化专家": "AI Optimization Expert",
  "本机 AI 环境医生": "Local AI Environment Doctor",
  "小优": "Opti",
  "看这台电脑的 AI 为什么慢 / 贵 / 不稳，再动手":
    "Find out why AI is slow, costly, or unstable on this PC before changing anything",
  "先跑只读体检拿事实（PATH、代理、编码、抢占进程、token 用量），再逐条说清「哪儿不对、改了会怎样」，你点头才动手。":
    "Runs read-only checks first (PATH, proxy, encoding, process conflicts, and token usage), explains each issue and impact, and changes nothing without your approval.",
  "体检一下": "Run a checkup",
  "为什么这么贵": "Why does it cost so much?",
  "命令跑不通": "Command will not run",
  "省钱专家": "Cost-Saving Expert",
  "token 成本分析师": "Token Cost Analyst",
  "小算": "Tally",
  "看 token 花在哪，给能落地的省法": "See where tokens go and get practical ways to save",
  "先拿只读用量数据（各 AI / 各模型 / 各项目 / 近 7 天 / 月预测），再指出最大的几笔和可执行的省法，连代价一起说。":
    "Reads usage by AI, model, project, recent seven days, and monthly forecast, then identifies the biggest costs and practical savings with their tradeoffs.",
  "钱花哪了": "Where did the money go?",
  "这个月要花多少": "What will this month cost?",
  "小站": "Webby",
  "奶茶店落地页": "Bubble tea landing page",
  "个人作品集": "Personal portfolio",
  "产品官网首页": "Product homepage",
  "本本": "Docsy",
  "先出大纲再逐页逐段填充，PPT 默认出可直接打开的真 .pptx、Word 出真 .docx（也能出 HTML/Markdown）。":
    "Starts with an outline, then builds each page or section. Produces real .pptx and .docx files by default, with HTML or Markdown when needed.",
  "路演 PPT": "Pitch deck",
  "周报": "Weekly report",
  "产品介绍": "Product introduction",
  "小格": "Tabby",
  "销售报表": "Sales report",
  "记账台账": "Accounting ledger",
  "清单整理": "Organize a list",
  "小美": "Muse",
  "活动海报": "Event poster",
  "公众号封面": "WeChat article cover",
  "短视频片头": "Short-video intro",
  "小笔": "Pixel",
  "戴墨镜橘猫": "Orange cat in sunglasses",
  "中国风山水": "Chinese ink landscape",
  "赛博朋克城市": "Cyberpunk city",
  "小影": "Reel",
  "猫在月球": "Cat on the Moon",
  "产品展示": "Product showcase",
  "小简": "CV Pro",
  "写简历": "Write a résumé",
  "优化经历": "Improve work experience",
  "文文": "Copy",
  "小红书笔记": "RedNote post",
  "公众号推文": "WeChat article",
  "朋友圈文案": "WeChat Moments copy",
  "阿译": "Lingua",
  "中译英": "Chinese to English",
  "英译中": "English to Chinese",
  "环境体检": "Environment checkup",
  "省 token": "Save tokens",
  "排障": "Troubleshooting",
  "用量分析": "Usage analysis",
  "成本": "Cost",
};
