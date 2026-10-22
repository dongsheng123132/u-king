/**
 * 「装不上？让 AI 装」—— 把一次安装诉求压成统一维护任务包，交给轻助手（维护助手）。
 *
 * 与 Wizard 的装机失败交接、ToolHub 的「让 AI 帮我修」共用 `buildMaintenanceTask`
 * 同一份格式与脱敏规则（密钥 / Authorization / 充值 URL query / 用户路径都会抹掉）。
 * 这里只负责「安装」这一种场景的字段填法，不另写一套模板。
 */
import { buildMaintenanceTask } from "./maintenanceTask";

export type InstallTarget = { id: string; name: string; summary: string };

/** 用户在首页/软件库点「让 AI 装」时填进维护助手的草稿（不会自动发送，由用户按发送）。 */
export function buildAiInstallPrompt(t: InstallTarget): string {
  return buildMaintenanceTask({
    target: `安装 ${t.name}（${t.id}）`,
    failure:
      "用户在 U-King 里点了「让 AI 装」：自动安装这条路没走通（或用户希望由你直接装）。请先看这台机器现在是什么状态，再动手。",
    steps: [
      `用户已在 U-King 里选中「${t.name}」，工具 id=${t.id}`,
      `工具简介：${t.summary}`,
      "安装前先确认：它是否已经装过（可能只是没被检测到）；装的时候用官方渠道，别用不明来源的包。",
    ],
    verify: "命令行能打印版本号，或「我的 AI」里显示已安装、能启动/打开。",
    rollback: "只装用户点名的这一个；不要卸载或改写用户已有的配置，不要向用户索要 Key。",
  });
}

/** 已装工具「装着但用不了 / 想升级修不好」时，交给轻助手的诊断任务包。 */
export function buildAiRepairPrompt(
  t: InstallTarget & { version?: string | null },
): string {
  return buildMaintenanceTask({
    target: `检修 ${t.name}（${t.id}）`,
    failure:
      "用户在 U-King 里点了「让 AI 修」：这个工具已经装了，但可能启动不了、用不了或配置不对。请先看这台机器的现状，再决定怎么修。",
    steps: [
      `工具 id=${t.id}`,
      `当前识别到的版本：${t.version ? `v${t.version}` : "没读到（可能没装好，或不在 PATH 里）"}`,
      `工具简介：${t.summary}`,
    ],
    verify: "工具能正常启动 / 打印版本号，或在 U-King 里显示可用。",
    rollback: "只修这一个工具；不要改用户已有的模型配置和 Key，不要卸载用户没点名的东西。",
  });
}
