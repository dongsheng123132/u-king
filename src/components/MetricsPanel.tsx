// 「数据」面板 —— AI 优化大师的数据支撑（metrics.rs + envfp.rs 的 GUI 投影）。
//
// 设计前提：**这一页不上传也有用**。它是用户愿意让我们继续采集的唯一理由，
// 所以默认呈现的是「你这台机器的实际情况」，上传是额外的、显式的、可关的。
//
// 三条出数纪律，界面必须原样执行（判断在核心，界面只投影 —— 宪法第 15 条）：
//   ① 样本不足时**不显示大字结论**，改显示后端给的 notes
//   ② 后端给 null 的百分比显示「—」，绝不渲染成 0%
//   ③ 变好变坏都要显示，变坏标红 —— 只报变好的一眼就是营销
//
// 铁律：只靠 props 通信；删除本面板只动 AiRuntime.tsx 一处 import。

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type ModelRow = {
  tool: string;
  model: string;
  calls: number;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  days: number;
};
type DayRow = { day: string; calls: number; tokens: number; errors: number };
type ErrorRow = { sig: string; kind: string; tool: string; count: number; last_msg: string };
type CompareRow = {
  tool: string;
  errors_per_day_before: number;
  errors_per_day_after: number;
  tokens_per_call_before: number;
  tokens_per_call_after: number;
  errors_delta_pct: number | null;
  tokens_delta_pct: number | null;
};
type Compare = {
  anchor_ts: number;
  recipes: string[];
  before_days: number;
  after_days: number;
  sufficient: boolean;
  rows: CompareRow[];
};
type MetricsAdvice = {
  id: string;
  severity: "high" | "medium" | "low";
  title: string;
  detail: string;
  evidence: string;
  action: string | null;
};
type Report = {
  schema: number;
  days: number;
  events: number;
  first_ts: number | null;
  models: ModelRow[];
  daily: DayRow[];
  errors: ErrorRow[];
  compare: Compare | null;
  advice: MetricsAdvice[];
  notes: string[];
  upload_consent: boolean;
};

/** 大数字压成 k/M/B —— token 动辄几十亿，原样显示没人读得出量级。 */
function fmtNum(n: number): string {
  if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(1) + "B";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

/** 缓存命中率：缓存读 / 总输入。分母 0 时返回 null（不编数字）。 */
function cacheHit(m: ModelRow): number | null {
  const total = m.cache_read + m.cache_write + m.input;
  return total > 0 ? m.cache_read / total : null;
}

const SEV_STYLE: Record<string, { border: string; bg: string; text: string; icon: string }> = {
  high: { border: "border-danger-500/30", bg: "bg-danger-500/[0.06]", text: "text-danger-400", icon: "⚠" },
  medium: { border: "border-warning-500/30", bg: "bg-warning-500/[0.06]", text: "text-warning-400", icon: "!" },
  low: { border: "border-white/[0.06]", bg: "bg-bg-1/40", text: "text-ink-3", icon: "·" },
};

export function MetricsPanel({
  onToast,
  onRunAction,
}: {
  onToast?: (msg: string) => void;
  onRunAction?: (action: string) => void;
}) {
  const { t } = useI18n();
  const [rep, setRep] = useState<Report | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [days, setDays] = useState(30);

  const load = useCallback(
    async (d: number) => {
      try {
        // 先补一次当天快照，再取报告 —— 否则刚跑完 AI 的用量要等下次启动才看得到
        await invoke("metrics_rollup").catch(() => {});
        setRep(await invoke<Report>("metrics_report", { days: d }));
        setErr(null);
      } catch (e) {
        setErr(String(e));
      }
    },
    [],
  );

  useEffect(() => {
    void load(days);
  }, [load, days]);

  const toggleConsent = useCallback(async () => {
    if (!rep) return;
    const next = !rep.upload_consent;
    try {
      await invoke("metrics_set_consent", { upload: next });
      setRep({ ...rep, upload_consent: next });
      onToast?.(next ? t("已开启：今后会上传匿名统计（不含代码和对话）") : t("已关闭上传，数据只留在本机"));
    } catch (e) {
      onToast?.(t("设置失败：") + String(e));
    }
  }, [rep, onToast, t]);

  if (err) {
    return (
      <div className="rounded-card border border-danger-500/30 bg-danger-500/[0.06] p-4 text-[12px] text-ink-2">
        {t("读不到本地数据：")}
        {err}
      </div>
    );
  }
  if (!rep) {
    return <div className="rounded-card border border-white/[0.06] bg-bg-2 p-4 text-[12px] text-ink-4">{t("正在读本地数据…")}</div>;
  }

  const maxTokens = Math.max(1, ...rep.daily.map((d) => d.tokens));

  return (
    <div className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card p-4 space-y-4">
      <div className="flex items-center justify-between gap-2">
        <div className="text-[12px] font-medium text-ink-2 flex items-center gap-2">
          {t("使用数据")}
          <span className="text-[10px] text-ink-5">{t("· 只存在这台机器上")}</span>
        </div>
        <div className="flex items-center gap-1">
          {[7, 30, 90].map((d) => (
            <button
              key={d}
              onClick={() => setDays(d)}
              className={
                "text-[10.5px] px-2 py-1 rounded-lg border transition-colors " +
                (days === d
                  ? "border-accent-500/40 bg-accent-500/[0.12] text-accent-400"
                  : "border-white/[0.06] text-ink-4 hover:text-ink-2")
              }
            >
              {t("{n} 天", { n: d })}
            </button>
          ))}
        </div>
      </div>

      {/* ===== 建议（每条都带证据；给不出证据的后端根本不会返回）===== */}
      {rep.advice.length > 0 && (
        <div className="space-y-2">
          {rep.advice.map((a) => {
            const s = SEV_STYLE[a.severity] ?? SEV_STYLE.low;
            return (
              <div key={a.id} className={"rounded-lg border p-3 " + s.border + " " + s.bg}>
                <div className="flex items-start gap-2">
                  <span className={"mt-0.5 text-[12px] " + s.text}>{s.icon}</span>
                  <div className="min-w-0 flex-1">
                    <div className="text-[12.5px] font-medium text-ink-0">{a.title}</div>
                    <div className="text-[11px] text-ink-3 mt-1 leading-relaxed">{a.detail}</div>
                    {/* ★ 证据：凭什么这么说。没有它这就只是一句正确的废话 */}
                    <div className="text-[10.5px] text-ink-5 mt-1.5 font-mono">{a.evidence}</div>
                  </div>
                  {a.action && onRunAction && (
                    <button
                      onClick={() => onRunAction(a.action as string)}
                      className="shrink-0 text-[10.5px] px-2.5 py-1 rounded-lg border border-accent-500/40 bg-accent-500/[0.12] text-accent-400 hover:bg-accent-500/[0.18] transition-colors"
                    >
                      {t("去修复")}
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* ===== 优化前后对比 ===== */}
      {rep.compare && (
        <div className="rounded-lg border border-white/[0.06] bg-bg-1/40 p-3">
          <div className="text-[11.5px] font-medium text-ink-1 mb-2">
            {t("优化前后对比")}
            <span className="text-[10px] text-ink-5 ml-1.5">
              {t("· 前 {b} 天 / 后 {a} 天", { b: rep.compare.before_days, a: rep.compare.after_days })}
            </span>
          </div>
          {/* 纪律①：样本不足就不给大字结论，只显示后端的诚实说明 */}
          {!rep.compare.sufficient ? (
            <div className="text-[11px] text-ink-4 leading-relaxed">
              {t("样本还不够，先不给结论。继续用几天这里会自动出数。")}
            </div>
          ) : (
            <div className="space-y-1.5">
              {rep.compare.rows.map((r) => (
                <div key={r.tool} className="flex items-center gap-3 text-[11px]">
                  <span className="w-16 shrink-0 text-ink-2">{r.tool}</span>
                  <Delta label={t("报错")} pct={r.errors_delta_pct} lowerIsBetter />
                  <Delta label={t("每次 token")} pct={r.tokens_delta_pct} lowerIsBetter />
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ===== 分工具 / 分模型用量 ===== */}
      {rep.models.length > 0 && (
        <div>
          <div className="text-[11.5px] font-medium text-ink-1 mb-2">{t("用在哪了")}</div>
          <div className="space-y-1">
            {rep.models.slice(0, 8).map((m) => {
              const hit = cacheHit(m);
              return (
                <div key={m.tool + m.model} className="flex items-center gap-2 text-[11px] py-1">
                  <span className="w-14 shrink-0 text-ink-4">{m.tool}</span>
                  <span className="flex-1 min-w-0 truncate text-ink-1" title={m.model}>
                    {m.model}
                  </span>
                  <span className="w-16 text-right text-ink-3 tabular-nums">{t("{n} 次", { n: m.calls })}</span>
                  <span className="w-16 text-right text-ink-3 tabular-nums">{fmtNum(m.input + m.output)}</span>
                  {/* 缓存命中：低了就是在为同一段上下文反复付全价 */}
                  <span
                    className={
                      "w-14 text-right tabular-nums " +
                      (hit === null ? "text-ink-5" : hit >= 0.7 ? "text-success-400" : "text-warning-400")
                    }
                    title={t("上下文缓存命中率")}
                  >
                    {hit === null ? "—" : (hit * 100).toFixed(0) + "%"}
                  </span>
                </div>
              );
            })}
          </div>
          <div className="flex items-center gap-2 text-[9.5px] text-ink-5 mt-1.5">
            <span className="w-14 shrink-0" />
            <span className="flex-1" />
            <span className="w-16 text-right">{t("调用")}</span>
            <span className="w-16 text-right">{t("token")}</span>
            <span className="w-14 text-right">{t("缓存命中")}</span>
          </div>
        </div>
      )}

      {/* ===== 每日趋势（聚合表看不出「哪天开始变糟」）===== */}
      {rep.daily.length > 1 && (
        <div>
          <div className="text-[11.5px] font-medium text-ink-1 mb-2">{t("每日用量")}</div>
          <div className="flex items-end gap-[3px] h-14">
            {rep.daily.map((d) => (
              <div
                key={d.day}
                className="flex-1 min-w-[3px] rounded-sm bg-accent-500/40 relative group"
                style={{ height: `${Math.max(4, (d.tokens / maxTokens) * 100)}%` }}
                title={`${d.day} · ${fmtNum(d.tokens)} token · ${d.calls} 次${d.errors ? ` · ${d.errors} 个错` : ""}`}
              >
                {d.errors > 0 && <div className="absolute -top-1 left-0 right-0 h-1 rounded-sm bg-danger-500" />}
              </div>
            ))}
          </div>
          <div className="flex justify-between text-[9.5px] text-ink-5 mt-1">
            <span>{rep.daily[0]?.day}</span>
            <span>{rep.daily[rep.daily.length - 1]?.day}</span>
          </div>
        </div>
      )}

      {/* ===== 报错 top ===== */}
      {rep.errors.length > 0 && (
        <div>
          <div className="text-[11.5px] font-medium text-ink-1 mb-2">{t("最常见的报错")}</div>
          <div className="space-y-1">
            {rep.errors.slice(0, 5).map((e) => (
              <div key={e.sig} className="flex items-start gap-2 text-[11px]">
                <span className="shrink-0 text-danger-400 tabular-nums w-8 text-right">{e.count}×</span>
                <span className="min-w-0 flex-1 text-ink-3 truncate" title={e.last_msg}>
                  {e.last_msg || e.kind}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ===== 诚实说明（后端给什么显示什么，不美化）===== */}
      {rep.notes.length > 0 && (
        <div className="space-y-1">
          {rep.notes.map((n, i) => (
            <div key={i} className="text-[10.5px] text-ink-4 leading-relaxed">
              · {n}
            </div>
          ))}
        </div>
      )}

      {/* ===== 上传同意（默认关）===== */}
      <div className="rounded-lg border border-white/[0.06] bg-bg-1/40 p-3">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <div className="text-[11.5px] font-medium text-ink-1">{t("帮我们改进（可选）")}</div>
            <div className="text-[10.5px] text-ink-4 mt-1 leading-relaxed">
              {t(
                "开启后只上传匿名统计：机型、系统、工具版本、调用次数、token 数、报错类型。绝不上传你的代码、对话内容和文件路径。随时可关。",
              )}
            </div>
          </div>
          <button
            onClick={() => void toggleConsent()}
            className={
              "shrink-0 text-[10.5px] px-3 py-1.5 rounded-lg border transition-colors " +
              (rep.upload_consent
                ? "border-success-500/40 bg-success-500/[0.12] text-success-400"
                : "border-white/[0.06] text-ink-4 hover:text-ink-2")
            }
          >
            {rep.upload_consent ? t("已开启") : t("未开启")}
          </button>
        </div>
      </div>

      <div className="text-[9.5px] text-ink-5">
        {t("共 {n} 条记录 · 存放在 ~/.uking/metrics/ · 卸载 U-King 会一并删除", { n: rep.events })}
      </div>
    </div>
  );
}

/** 一个变化量。**null 显示「—」而不是 0%**（纪律②）；变坏标红、变好标绿（纪律③）。 */
function Delta({ label, pct, lowerIsBetter }: { label: string; pct: number | null; lowerIsBetter?: boolean }) {
  if (pct === null) {
    return (
      <span className="text-ink-5">
        {label} <span className="tabular-nums">—</span>
      </span>
    );
  }
  const better = lowerIsBetter ? pct < 0 : pct > 0;
  const flat = Math.abs(pct) < 1;
  const cls = flat ? "text-ink-4" : better ? "text-success-400" : "text-danger-400";
  return (
    <span className="text-ink-4">
      {label}{" "}
      <span className={"tabular-nums font-medium " + cls}>
        {pct > 0 ? "+" : ""}
        {pct.toFixed(0)}%
      </span>
    </span>
  );
}
