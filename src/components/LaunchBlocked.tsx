import { useI18n } from "../i18n";

/** 对应 Rust `tools::LaunchPlan`（`runtime.tool.inspect` / `runtime.tool.launch` 的输出形状）。 */
export type LaunchPlan = {
  tool_id: string;
  cmd: string;
  installed: boolean;
  resolved_path: string | null;
  source: string;
  on_terminal_path: boolean;
  mode: "gui_app" | "external_term" | "embedded_pty" | "route_tab" | "url" | "none";
  route: string | null;
  launch_cmd: string;
  cmd_allowed: boolean;
  status: "ready" | "not_installed" | "not_found_in_path" | "rejected_cmd" | "no_launcher";
  blockers: string[];
};

type LaunchBlockedProps = {
  plan: LaunchPlan;
  /** `not_installed` 时的「去安装」入口——外层用同一个 `openTool(t)` 接。 */
  onInstall?: () => void;
  /** 没有对应"修 PATH"入口时，兜底反馈渠道（比如打开反馈/工单）。 */
  onFeedback?: () => void;
};

/**
 * `runtime.tool.inspect`/`runtime.tool.launch` 判定为「不能直接启动」时的统一展示。
 * 只读展示 + 有限的几个动作入口，不在这里发起真正的启动。
 */
export function LaunchBlocked({ plan, onInstall, onFeedback }: LaunchBlockedProps) {
  const { t } = useI18n();
  const blocker = plan.blockers[0];

  if (plan.status === "not_installed") {
    return (
      <div className="rounded-lg border border-warning-400/30 bg-warning-400/[0.06] px-3 py-2.5 text-[12px] leading-relaxed">
        <div className="text-ink-2">{blocker ?? t("还没安装「{name}」，请先安装。", { name: plan.tool_id })}</div>
        {onInstall && (
          <button
            onClick={onInstall}
            className="mt-2 h-7 px-2.5 rounded-md bg-accent text-[11px] font-medium text-white hover:bg-accent-600"
          >
            {t("去安装")}
          </button>
        )}
      </div>
    );
  }

  if (plan.status === "not_found_in_path") {
    return (
      <div className="rounded-lg border border-warning-400/30 bg-warning-400/[0.06] px-3 py-2.5 text-[12px] leading-relaxed">
        <div className="text-ink-2">
          {plan.resolved_path
            ? t("已在 {path} 检测到，但终端里找不到它，需要重新装到默认位置 / 修复 PATH。", { path: plan.resolved_path })
            : t("检测到已安装，但终端里找不到它，需要重新装到默认位置 / 修复 PATH。")}
        </div>
        {onFeedback && (
          <button
            onClick={onFeedback}
            className="mt-2 h-7 px-2.5 rounded-md border border-accent/35 text-[11px] font-medium text-accent hover:bg-accent/[0.08]"
          >
            {t("反馈问题")}
          </button>
        )}
      </div>
    );
  }

  if (plan.status === "rejected_cmd") {
    return (
      <div className="rounded-lg border border-danger-400/30 bg-danger-400/[0.06] px-3 py-2.5 text-[12px] leading-relaxed">
        <div className="text-ink-2">
          {t("启动命令「{cmd}」被安全校验拒绝，这是我们的 bug，请反馈。", { cmd: plan.launch_cmd })}
        </div>
        {onFeedback && (
          <button
            onClick={onFeedback}
            className="mt-2 h-7 px-2.5 rounded-md border border-accent/35 text-[11px] font-medium text-accent hover:bg-accent/[0.08]"
          >
            {t("反馈问题")}
          </button>
        )}
      </div>
    );
  }

  if (plan.status === "no_launcher") {
    return (
      <div className="rounded-lg border border-white/[0.08] bg-bg-1/70 px-3 py-2.5 text-[12px] leading-relaxed text-ink-3">
        {t("从开始菜单 / 桌面图标打开，这里帮不了。")}
      </div>
    );
  }

  // status === "ready" 不该走到这个组件；给个兜底文案而不是空白。
  return (
    <div className="rounded-lg border border-white/[0.08] bg-bg-1/70 px-3 py-2.5 text-[12px] leading-relaxed text-ink-3">
      {blocker ?? t("暂时无法启动。")}
    </div>
  );
}
