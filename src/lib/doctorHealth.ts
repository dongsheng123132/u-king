/**
 * 体检「全绿」判定 —— 提成纯函数（first-principles 评审 2026-09-06 ②2）。
 *
 * 健康只回答「这台机器现在能不能好好用 AI」：
 *   - 必要运行时（Node / npm / Git）在；
 *   - 已装的 AI 工具都接好了模型（ready 或用户自管），且至少有一个能用。
 *
 * 钱包没充值（BYOK 用户很正常）、可选工具没装、服务器有新版、更新检查失败，
 * 都是独立信息，在展开视图各自呈现，不算故障、不拉黄折叠条。
 * 探测失败（probe found=false）不涂成健康。
 */

export type CmdProbe = { found: boolean; version: string | null };

export type AiCheckupItem = {
  target: string;
  label: string;
  installed: boolean;
  state: "ready" | "idle" | "self-managed" | "absent";
  model: string | null;
  can_auto_fix: boolean;
};

export type DoctorReport = {
  update: {
    current: string;
    latest: string;
    has_update: boolean;
    checked_ok: boolean;
    fail_reason?: string;
    failed_attempts?: number;
  };
  wallet: {
    charged: boolean;
    low_balance: boolean;
    balance: { text?: string } | null;
    recharge_url: string;
  } | null;
  stack: {
    node: CmdProbe;
    npm: CmdProbe;
    git: CmdProbe;
    portable_node: boolean;
    system_proxy: string | null;
  };
  tools: AiCheckupItem[];
};

export function isAllGreen(report: DoctorReport): boolean {
  const installed = report.tools.filter((item) => item.installed);
  const toolsReady =
    installed.length > 0 &&
    installed.every((item) => item.state === "ready" || item.state === "self-managed");
  const stackReady =
    report.stack.node.found && report.stack.npm.found && report.stack.git.found;
  return stackReady && toolsReady;
}
