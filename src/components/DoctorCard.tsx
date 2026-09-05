/**
 * AI 设置页「一键体检 / 一键升级」卡 —— 参考 `claude doctor` / `hermes doctor` 的产品定位。
 *
 * 客户只按一个按钮就能回答「这台机器现在能不能好好用 AI」：
 *   ① 本体版本 + 服务器有没有新版（doctor_report.update）
 *   ② 虾盘云钱包余额（doctor_report.wallet）
 *   ③ 运行环境：Node / npm / git / 便携 Node / 系统代理（doctor_report.stack）
 *   ④ 各 AI CLI 的配置状态（doctor_report.tools —— 与 ai_checkup 同一份实现）
 *
 * 升级 = 对每个已装 CLI 重跑安装流水线（`upgrade_cli_tool`）：npm 装 latest / pip 自带 -U，
 * 白拿安装器全套护栏。逐个串行跑，日志走 `uking:upgrade` 事件。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  ArrowUpCircle,
  CheckCircle2,
  ChevronDown,
  CircleDashed,
  Loader2,
  RefreshCw,
  Stethoscope,
  XCircle,
} from "lucide-react";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

import { isAllGreen } from "../lib/doctorHealth";
import type { AiCheckupItem, CmdProbe, DoctorReport } from "../lib/doctorHealth";
import { ToolFixButton } from "./ToolFixButton";

/** 同一轮体检的事实在各挂载点共用，避免切页时重复打版本/余额网络请求。 */
const DOCTOR_CACHE_MS = 5 * 60 * 1000;
let doctorCache: { report: DoctorReport; checkedAt: number } | null = null;
let doctorCheckInFlight: Promise<{ report: DoctorReport; checkedAt: number }> | null = null;

function loadDoctorReport(force = false) {
  if (force) doctorCache = null;
  if (doctorCache && Date.now() - doctorCache.checkedAt < DOCTOR_CACHE_MS) return Promise.resolve(doctorCache);
  if (!doctorCheckInFlight) {
    doctorCheckInFlight = invoke<DoctorReport>("doctor_report")
      .then((report) => {
        const result = { report, checkedAt: Date.now() };
        doctorCache = result;
        return result;
      })
      .finally(() => {
        doctorCheckInFlight = null;
      });
  }
  return doctorCheckInFlight;
}

function checkedTime(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hour12: false }).format(timestamp);
}

/** 单工具升级状态机：idle → running → ok / fail / skip（未安装）。 */
type UpState = "idle" | "running" | "ok" | "fail" | "skip";

/** 一键升级覆盖的 CLI 工具（skill 清单里的 npm/pip 工具；codex-app 是 GUI，不在此列）。 */
const UPGRADE_TOOLS = [
  "claude-code",
  "codex",
  "openclaw",
  "hermes",
  "qwen-code",
  "crush",
  "opencode",
  "dsh",
  "pi",
  "cline",
];

function Probe({ label, p }: { label: string; p: CmdProbe | undefined }) {
  const { t } = useI18n();
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[10.5px] font-mono",
        p?.found ? "bg-white/[0.05] text-ink-2" : "bg-white/[0.03] text-ink-5 line-through",
      )}
      title={p?.found ? p.version ?? "" : t("未检测到")}
    >
      {p?.found ? `${label} ${p.version?.split(" ").pop() ?? ""}` : label}
    </span>
  );
}

function StateBadge({ item }: { item: AiCheckupItem }) {
  const { t } = useI18n();
  const map: Record<AiCheckupItem["state"], { label: string; cls: string }> = {
    ready: { label: t("已配好"), cls: "text-success-400" },
    idle: { label: t("未接 AI"), cls: "text-amber-500" },
    "self-managed": { label: t("已自行配置"), cls: "text-ink-3" },
    absent: { label: t("未安装"), cls: "text-ink-5" },
  };
  const m = map[item.state];
  return (
    <span className={cn("inline-flex items-center gap-1 text-[11px] shrink-0", m.cls)}>
      {item.state === "ready" ? <CheckCircle2 size={12} /> : item.state === "absent" ? <CircleDashed size={12} /> : <AlertTriangle size={12} />}
      {m.label}
      {item.model ? <span className="font-mono text-ink-5 hidden md:inline">· {item.model}</span> : null}
    </span>
  );
}

export function DoctorCard({
  onRecharge,
  onSelfUpdate,
  collapsedByDefault,
}: {
  /** 打开充值页（沿用 Manager 那条路：独立 webview 子窗口）。 */
  onRecharge?: (url?: string | null) => void;
  /** 本体一键升级（复用 App 的 doSelfUpdate：进度/失败账本全在那一层管）。 */
  onSelfUpdate?: () => void;
  /** 首次挂载时是否折叠（2026-09-04，「我的 AI」页顶部用：全绿别占地方）。
   *  只影响**初始**折叠态，不影响下面 runCheck 里「体检完发现有问题就自动展开」那条既有逻辑——
   *  两者不冲突：collapsedByDefault=true 时若体检回来是全绿，`!isAllGreen` 算出 false，
   *  折叠态原样保持；若有问题，同一行逻辑会把它自动展开，这是期望行为。
   *  不传时保持原来的行为（默认展开），Manager 页以前的调用方式不受影响——虽然
   *  Manager 页那处挂载点本身已在本次改动中删除，这里仍保留「不传即展开」以防未来别处复用。 */
  collapsedByDefault?: boolean;
}) {
  const { t } = useI18n();
  const [report, setReport] = useState<DoctorReport | null>(null);
  const [checkedAt, setCheckedAt] = useState<number | null>(null);
  const [expanded, setExpanded] = useState(!collapsedByDefault);
  const [checking, setChecking] = useState(false);
  const [upgrading, setUpgrading] = useState(false);
  // 每个工具的升级状态 + 最近一行日志（事件来一条覆盖一条，够看清在干什么）
  const [upStates, setUpStates] = useState<Record<string, { state: UpState; note: string }>>({});
  // 🔴 sol 终审沿用（原 ToolCheckup）：一键配好期间所有目标都禁用，防双击/多目标
  // 并发各起一条 apply 流水把同一批配置写花。
  const [fixingAny, setFixingAny] = useState(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  async function runCheck(force = false) {
    setChecking(true);
    try {
      const result = await loadDoctorReport(force);
      if (alive.current) {
        setReport(result.report);
        setCheckedAt(result.checkedAt);
        setExpanded(!isAllGreen(result.report));
      }
    } catch {
      /* 体检失败保持旧报告；按钮恢复可点，用户可重试 */
    } finally {
      if (alive.current) setChecking(false);
    }
  }
  // 挂载即体检一次 —— doctor 的意义就是「进门先看一眼」，不该等用户点。
  useEffect(() => {
    runCheck();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** 一键升级：已装的 CLI 逐个串行跑（并发装 npm 全局目录会互锁）。 */
  async function runUpgrade() {
    if (upgrading) return;
    setUpgrading(true);
    try {
      for (const id of UPGRADE_TOOLS) {
        if (!alive.current) return;
        setUpStates((s) => ({ ...s, [id]: { state: "running", note: "" } }));
        const un = await listen<{ tool: string; phase: string; line: string }>("uking:upgrade", (e) => {
          if (e.payload.tool !== id) return;
          setUpStates((s) => ({ ...s, [id]: { state: "running", note: e.payload.line } }));
        });
        try {
          const r = await invoke<{ ok: boolean; version: string | null; error: string | null }>("upgrade_cli_tool", { toolId: id });
          un();
          if (alive.current) {
            setUpStates((s) => ({
              ...s,
              [id]: r.ok
                ? { state: "ok", note: r.version ?? "" }
                : { state: "fail", note: r.error ?? t("失败") },
            }));
          }
        } catch (e) {
          un();
          // 「未安装」不算失败 —— 一键升级对没装的工具如实说跳过，不算坏消息。
          const msg = String(e);
          if (alive.current) {
            setUpStates((s) => ({ ...s, [id]: { state: msg.includes("未安装") ? "skip" : "fail", note: msg } }));
          }
        }
      }
      // 升级会改工具版本/配置 → 重拉体检报告让状态行跟着新事实走。
      await runCheck(true);
    } finally {
      if (alive.current) setUpgrading(false);
    }
  }

  const installedTools = report?.tools.filter((x) => x.installed) ?? [];
  const readyCount = installedTools.filter((x) => x.state === "ready").length;
  const idleTools = installedTools.filter((x) => x.state === "idle");
  const upDone = Object.values(upStates).filter((u) => u.state === "ok").length;
  const upFail = Object.values(upStates).filter((u) => u.state === "fail").length;
  const allGreen = !!report && isAllGreen(report);

  // 折叠态按严重度分三档，条件从「全绿才折叠」放宽成「只要 expanded===false 就折」——
  // 因为折叠现在也可能是 collapsedByDefault 带来的初始态，而不是只有全绿才会出现。
  if (!expanded) {
    // 体检还没跑完（含 collapsedByDefault=true 时挂载即体检那一小段窗口）：中性 loading 态。
    if (!report) {
      return (
        <section className="mb-4 rounded-card border border-white/[0.08] bg-bg-1/90 shadow-card overflow-hidden">
          <button
            type="button"
            onClick={() => setExpanded(true)}
            title={t("点击查看体检详情")}
            className="w-full flex items-center gap-2.5 px-4 py-3 text-left hover:bg-white/[0.03]"
          >
            <Loader2 size={16} className="text-ink-4 shrink-0 animate-spin" />
            <span className="min-w-0 flex-1 text-[13px] font-medium text-ink-1">{t("体检中…")}</span>
            <ChevronDown size={15} className="text-ink-4 shrink-0" />
          </button>
        </section>
      );
    }
    if (allGreen) {
      return (
        <section className="mb-4 rounded-card border border-success-400/20 bg-bg-1/90 shadow-card overflow-hidden">
          <button
            type="button"
            onClick={() => setExpanded(true)}
            title={t("点击查看体检详情")}
            className="w-full flex items-center gap-2.5 px-4 py-3 text-left hover:bg-white/[0.03]"
          >
            <CheckCircle2 size={16} className="text-success-400 shrink-0" />
            <span className="min-w-0 flex-1 text-[13px] font-medium text-ink-1">
              {t("✅ 环境正常 · 刚检查 {time}", { time: checkedAt ? checkedTime(checkedAt) : "--:--" })}
            </span>
            <ChevronDown size={15} className="text-ink-4 shrink-0" />
          </button>
        </section>
      );
    }
    // 有问题但非「idle 缺件」（比如运行时缺件 / 一个已配好的工具都没有）：idleTools.length 可能是 0，
    // 这时用不点数字的通用文案，避免出现「0 个 AI 还没配好」这种误导。
    return (
      <section className="mb-4 rounded-card border border-amber-500/25 bg-bg-1/90 shadow-card overflow-hidden">
        <button
          type="button"
          onClick={() => setExpanded(true)}
          title={t("点击查看体检详情")}
          className="w-full flex items-center gap-2.5 px-4 py-3 text-left hover:bg-white/[0.03]"
        >
          <AlertTriangle size={16} className="text-amber-500 shrink-0" />
          <span className="min-w-0 flex-1 text-[13px] font-medium text-ink-1">
            {idleTools.length > 0
              ? t("⚠️ {n} 个 AI 还没配好", { n: idleTools.length })
              : t("⚠️ 环境有项待处理")}
          </span>
          <ChevronDown size={15} className="text-ink-4 shrink-0" />
        </button>
      </section>
    );
  }

  return (
    <section className="mb-4 rounded-card border border-white/[0.08] bg-bg-1/90 shadow-card overflow-hidden">
      {/* 标题行：一键体检 + 一键升级 */}
      <div className="flex items-center gap-2.5 px-4 py-3 border-b border-white/[0.06]">
        <span className="grid place-items-center w-7 h-7 rounded-lg bg-accent/[0.12] shrink-0">
          <Stethoscope size={14} className="text-accent" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-ink-1">{t("AI 一键体检")}</div>
          <div className="text-[11px] text-ink-4">
            {report
              ? t("{ready}/{total} 个 AI 已配好", { ready: readyCount, total: installedTools.length })
              : t("体检中…")}
          </div>
        </div>
        <button
          onClick={() => runCheck(true)}
          disabled={checking || upgrading}
          title={t("重新体检")}
          className="shrink-0 inline-flex items-center justify-center w-8 h-8 rounded-lg border border-white/[0.08] bg-bg-1 text-ink-3 hover:text-ink-1 hover:bg-white/[0.04] disabled:opacity-50"
        >
          {checking ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
        </button>
        <button
          onClick={runUpgrade}
          disabled={checking || upgrading}
          className="shrink-0 inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-60"
        >
          {upgrading ? <Loader2 size={13} className="animate-spin" /> : <ArrowUpCircle size={13} />}
          {upgrading ? t("升级中…") : t("一键升级全部 AI")}
        </button>
      </div>

      {/* 升级进度摘要（升级中/结束后显示） */}
      {upgrading || upDone + upFail > 0 ? (
        <div className="px-4 py-2 border-b border-white/[0.06] bg-white/[0.02]">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px]">
            {UPGRADE_TOOLS.map((id) => {
              const st = upStates[id];
              if (!st || st.state === "idle") return null;
              const isRunning = st.state === "running";
              return (
                <span
                  key={id}
                  title={st.note}
                  className={cn(
                    "inline-flex items-center gap-1",
                    st.state === "ok" && "text-success-400",
                    st.state === "fail" && "text-red-300",
                    st.state === "skip" && "text-ink-5",
                    isRunning && "text-accent",
                  )}
                >
                  {st.state === "ok" ? (
                    <CheckCircle2 size={11} />
                  ) : st.state === "fail" ? (
                    <XCircle size={11} />
                  ) : isRunning ? (
                    <Loader2 size={11} className="animate-spin" />
                  ) : (
                    <CircleDashed size={11} />
                  )}
                  {id}
                  {st.state === "ok" && st.note ? <span className="font-mono text-ink-5">{st.note}</span> : null}
                </span>
              );
            })}
          </div>
        </div>
      ) : null}

      {/* 体检结果：本体 / 钱包 / 环境 / 各 AI */}
      {report && (
        <div className="px-4 py-3 space-y-2">
          {/* ① 本体版本 */}
          <div className="flex items-center gap-2 text-[12px]">
            <span className="text-ink-3 w-20 shrink-0">{t("U-King 本体")}</span>
            <span className="font-mono text-ink-2">v{report.update.current}</span>
            {report.update.has_update ? (
              <span className="inline-flex items-center gap-1 text-amber-500 text-[11px]">
                <ArrowUpCircle size={12} />
                {t("有新版 v{ver}", { ver: report.update.latest })}
                {onSelfUpdate && (
                  <button
                    onClick={onSelfUpdate}
                    className="ml-1 px-2 h-6 rounded-md border border-amber-500/40 text-amber-500 hover:bg-amber-500/[0.08]"
                  >
                    {t("去升级")}
                  </button>
                )}
              </span>
            ) : report.update.checked_ok === false ? (
              <span className="text-ink-4 text-[11px]">{t("检查不到更新（网络？）")}</span>
            ) : (
              <span className="inline-flex items-center gap-1 text-success-400 text-[11px]">
                <CheckCircle2 size={12} />
                {t("已是最新版")}
              </span>
            )}
          </div>

          {/* ② 钱包 */}
          <div className="flex items-center gap-2 text-[12px]">
            <span className="text-ink-3 w-20 shrink-0">{t("虾盘云余额")}</span>
            {report.wallet ? (
              <>
                <span className={cn("font-mono", report.wallet.charged ? "text-ink-2" : "text-red-300")}>
                  {report.wallet.balance?.text ?? t("待充值")}
                </span>
                {report.wallet.low_balance && <span className="text-amber-500 text-[11px]">{t("余额偏低")}</span>}
                {!report.wallet.charged && onRecharge && (
                  <button
                    onClick={() => onRecharge(report.wallet?.recharge_url)}
                    className="px-2 h-6 rounded-md border border-accent/40 text-accent hover:bg-accent/[0.08]"
                  >
                    {t("去充值")}
                  </button>
                )}
              </>
            ) : (
              <span className="text-ink-5 text-[11px]">{t("钱包状态读取失败，请重试体检")}</span>
            )}
          </div>

          {/* ③ 运行环境 */}
          <div className="flex items-center gap-2 text-[12px] flex-wrap">
            <span className="text-ink-3 w-20 shrink-0">{t("运行环境")}</span>
            <Probe label="node" p={report.stack.node} />
            <Probe label="npm" p={report.stack.npm} />
            <Probe label="git" p={report.stack.git} />
            {report.stack.portable_node && (
              <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[10.5px] font-mono bg-white/[0.05] text-ink-2">
                {t("便携 Node ✓")}
              </span>
            )}
            {report.stack.system_proxy && (
              <span
                title={t("检测到系统代理 —— 代理节点失效时会出现「实测全绿、工具连不上」")}
                className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[10.5px] bg-amber-500/[0.10] text-amber-500"
              >
                <AlertTriangle size={10} />
                {t("系统代理 {p}", { p: report.stack.system_proxy })}
              </span>
            )}
          </div>

          {/* ④ 各 AI 配置状态 */}
          <div className="pt-1 space-y-1.5">
            {(report.tools.length ? report.tools : []).map((item) => (
              <div key={item.target} className="flex items-center gap-2 text-[12px] flex-wrap">
                <span className={cn("w-20 shrink-0 truncate", item.installed ? "text-ink-2" : "text-ink-5")}>
                  {item.label}
                </span>
                <StateBadge item={item} />
                {item.installed && item.state === "idle" && item.can_auto_fix && (
                  <ToolFixButton
                    item={item}
                    disabled={fixingAny}
                    onFixingChange={setFixingAny}
                    onFixed={() => runCheck(true)}
                  />
                )}
                {item.installed && item.state === "idle" && !item.can_auto_fix && (
                  <span className="ml-auto text-[10.5px] text-ink-4 shrink-0">{t("暂不支持自动配置")}</span>
                )}
              </div>
            ))}
            {idleTools.length > 0 && (
              <div className="text-[11px] text-ink-4 pt-0.5">
                {t("标黄的还没接 AI —— 用上方「一键配好」或到「免费算力」页接入")}
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
