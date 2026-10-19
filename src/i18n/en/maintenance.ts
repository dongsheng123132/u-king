/** 维护助手（MaintenanceAssistant）+ 装机失败交接（Wizard「交给轻助手/复制任务」）专用英文词条。
 *  2026-10-19 轻量维护助手批次。与别处共用的词（复制/取消/文件/余额不足等）复用中央字典，不重复翻。 */
export const maintenance: Record<string, string> = {
  // ── MaintenanceAssistant.tsx ──────────────────────────────────
  "维护任务已复制（已脱敏，不含 Key）": "Maintenance task copied (sanitized — no keys)",
  "余额查询失败，请重查后再发（不是没余额）":
    "Balance query failed — recheck before sending (this does NOT mean zero balance)",
  "余额不足，请先充值": "Insufficient balance — please recharge first",
  "还没查到余额（可能未充值）——可以先充值，或再试一次":
    "No balance result yet (may be unrecharged) — recharge, or try again",
  "维护助手": "Maintenance Assistant",
  "虾盘云 · DeepSeek Flash · 只做装机/故障维护": "Xiapan Cloud · DeepSeek Flash · install & troubleshooting only",
  "写操作每步确认，不会自动执行": "Write actions require confirmation each step — never auto-executed",
  "余额查询失败 · 重查": "Balance query failed · Retry",
  "余额 {text}": "Balance {text}",
  "余额不足": "Insufficient balance",
  "还没查到余额": "No balance result",
  "重新查询余额": "Recheck balance",
  "查余额": "Check balance",
  "把装机失败或工具故障的现象贴进来（报错原文最好），我来诊断。写操作会先弹确认框，不会自动改你的电脑。":
    "Paste the install failure or tool error here (raw error text preferred) and I'll diagnose. Write actions always ask first — nothing changes on your PC without confirmation.",
  "独立编程工作台（OpenCodex）尚未接入本产品——可在「我的 AI」里找已安装的工作台，这里不提供假安装链接。":
    "The standalone coding workbench (OpenCodex) is not integrated in this product — look in My AI for an installed workbench. No fake install links here.",
  "执行中…": "Running…",
  "确认写操作": "Confirm write action",
  "命令": "Command",
  "约 {n} 字节": "About {n} bytes",
  "已批准": "Approved",
  "已拒绝": "Rejected",
  "不点则 3 分钟后自动拒绝": "Auto-rejects after 3 minutes if unanswered",
  "描述装机失败/工具故障现象，粘贴报错原文…（Enter 发送，Shift+Enter 换行）":
    "Describe the install failure or tool error, paste the raw error… (Enter to send, Shift+Enter for newline)",
  "生成脱敏后的统一维护任务包，可交给外部 AI": "Build a sanitized maintenance task package for external AI",
  "复制维护任务": "Copy maintenance task",
  "处理中…": "Working…",
  "中断": "Interrupt",
  "发送": "Send",
  "余额": "Balance",
  "批准": "Approve",
  "拒绝": "Reject",
  "失败": "Failed",

  // ── ToolHub.tsx 详情条（工作台入口收敛为维护助手）──────────────
  "遇到装机/故障问题？": "Install or tool trouble?",
  "交给维护助手": "Hand off to Maintenance Assistant",
  "独立编程工作台（OpenCodex）可在「我的 AI」安装——实际尚未注册到工具目录":
    "The standalone coding workbench (OpenCodex) can be installed from My AI — not yet registered in the tool catalog",

  // ── Wizard.tsx 失败交接钩子 ──────────────────────────────────
  "交给轻助手（整理故障任务）": "Hand off to Assistant (package the failure)",
  "再整理一轮转交（{n}/3）": "Package & hand off again ({n}/3)",
  "复制任务（脱敏后给外部 AI）": "Copy task (sanitized, for external AI)",
  "复制任务": "Copy task",
  "重装后工具能正常启动即算修好。": "Fixed when the tool launches normally after reinstall.",
  "涉及配置改动的先备份原文件；不确定就先只诊断不动手。":
    "Back up config files before changing them; if unsure, diagnose only.",
  "已复制脱敏后的维护任务包（不含 Key），可粘给任意外部 AI 协助排查。":
    "Sanitized maintenance task copied (no keys) — paste it to any external AI for help.",
  "复制失败，请手动选中上方报错文本复制。": "Copy failed — select the error text above and copy manually.",
  "交给轻助手": "Hand off to Assistant",
  "已整理脱敏故障任务包（不含 Key），正在打开维护助手——确认内容后自己按发送即可。":
    "Sanitized failure package ready (no keys) — opening Maintenance Assistant. Review it, then send yourself.",
  "装好了！接下来去「我的 AI」一键启动工具。遇到装机/故障问题可以去「维护助手」描述现象（写操作每步确认）。":
    "All set! Launch tools from My AI. For install or tool trouble, describe it in Maintenance Assistant (write actions ask each step).",
  "去「我的 AI」启动工具": "Go to My AI to launch tools",
  "去我的 AI": "Go to My AI",
  "先这样": "Maybe later",
  "先不用，我自己逛逛": "No thanks, I'll look around",
  "跳过": "Skip",
};
