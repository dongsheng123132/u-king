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
  "定时任务：到点了让 AI 自己把活干了": "Scheduled jobs: let AI work automatically on time",

  // Task-board source table.
  "本工作台": "This workbench",
  "点一下隐藏这个来源": "Click to hide this source",
  "点一下显示这个来源": "Click to show this source",

  // Automation template table.

  // 技能标签（原专家墙用，仍被其它页引用）。
  "读文档": "Read documents",
  "改文档": "Edit documents",

  "公众号封面": "WeChat article cover",
  "环境体检": "Environment checkup",
};
