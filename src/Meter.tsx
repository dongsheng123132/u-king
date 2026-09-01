/**
 * Token 水电表 —— 这台电脑上**所有 AI 的 token 读数**，透明可视化。
 *
 * ## 这一页在回答什么
 * 「省 token」在没有表之前是句空话：省了多少、从哪省的、这个月会花多少，全靠感觉。
 * 这页就是那块表 —— 读数（今天/昨天/7天/窗口）、曲线（每天多少）、分账（哪个项目/工具/模型在耗）、
 * 缓存账（缓存到底帮你省了多少）、用得多快（按这个速度一个月多少、余额还能撑几天），最后给建议。
 *
 * 它是「省 token / 省费用」这条线的**地基**：压缩机、换便宜档模型、改 CLAUDE.md，
 * 都得在这块表上看得见变化才算数。
 *
 * ## 三条不许破的规矩
 * 1. **金额是折算，不是账单。** 本地日志只记 token 数，不记你实际付了多少。金额一律按
 *    **公开列表价**折 ¥ —— 包月订阅（Claude Max / ChatGPT Plus）用户看到的那个大数
 *    不是他付的钱，而是「同样的量走 API 要多少」。这条必须写在客户眼睛能看到的地方，
 *    不能只藏在 tooltip 里：一台重度开发机 30 天能折算出 ¥25 万，不解释清楚就是吓人。
 *    **所以「数据来源」里给了「我是包月订阅」的勾** —— 勾上那个工具就只报 token、金额记 0。
 * 2. **token 读数才是硬数字**，所以表盘第一行给 token、金额排第二行。
 * 3. **没算进来的要自己说。** 底部「数据来源」列的是本机探测到的**全部** AI 工具，
 *    算得到的给勾选框，算不到的（Gemini/Qwen 不写 token、CodeBuddy 在加密区、ChatGPT 账在云端）
 *    灰掉并逐条说明为什么。客户以为总数是全部、结果差一大截，比不给这张表更糟。
 *
 * 判断全在后端（`usage_local::meter`，确定性算术、离线可用、不烧 token）；
 * 这页只负责画，不在前端重算一遍口径（宪法第 15 条）。
 *
 * **整块可插拔**：纯前端，只靠 props（onToast / onGoto）。后端 = 影核动作
 * `runtime.usage_meter.inspect`（薄壳命令 `query_usage_meter`）。
 * 删本页只删这一个文件 + App.tsx 去 import/tab + Sidebar 去 NAV。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Gauge, Loader2, RefreshCw, TrendingUp, Database, Lightbulb, Info,
  FolderTree, Bot, Cpu, AlertTriangle, EyeOff, Wallet, Check, Minus, ListOrdered,
} from "lucide-react";
import { useI18n } from "./i18n";
import type { TabId } from "./components/Sidebar";

type Totals = { cny: number; calls: number; input_tokens: number; output_tokens: number; tokens: number };
type DayPoint = { date: string; cny: number; tokens: number; calls: number };
type ModelItem = { model: string; tool: string; cny: number; count: number; input_tokens: number; output_tokens: number };
type Named = { name: string; detail: string; cny: number; tokens: number; calls: number; share: number };
type Cache = { non_cached_input: number; cache_read: number; cache_creation: number; hit_rate: number; saved_cny: number };
type Pace = {
  daily_avg_cny: number;
  month_projection_cny: number;
  today_vs_avg: number;
  days_left: number | null;
  balance_cny: number | null;
};
type Tip = { id: string; title: string; detail: string; saving_cny: number };
/** 一路数据源。`countable` = 我们**读不读得到**它的账（跟装没装、勾没勾都无关）；
 *  `enabled` = 用户勾了要算；`subscription` = 用户标了它是包月（token 照算、金额记 0）。
 *  四个状态各说各的 —— 混成一句「没算进来」，客户不知道该去装、去开、还是它根本没账。 */
type Source = {
  tool: string;
  label: string;
  dir: string;
  exists: boolean;
  countable: boolean;
  enabled: boolean;
  subscription: boolean;
  covered: boolean;
  files: number;
  note: string;
};

/**
 * 一条流水 = 一次模型调用。**Hermes 那路例外**（`session_rollup`）：它的 `sessions` 表
 * 就是会话粒度，没有逐轮记录，所以一行可能是几十次调用的合计 —— 界面必须说出来。
 */
type UsageEvent = {
  ts: string;
  epoch: number;
  exact_time: boolean;
  tool: string;
  tool_label: string;
  model: string;
  project: string;
  project_dir: string;
  calls: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  tokens: number;
  cny: number;
  session_rollup: boolean;
};
type EventsMeta = { total: number; returned: number; truncated: number; returned_cny: number };

type MeterData = {
  days: number;
  ready: boolean;
  blockers: string[];
  window: Totals;
  today: Totals;
  yesterday: Totals;
  last7: Totals;
  daily: DayPoint[];
  by_model: ModelItem[];
  by_tool: Named[];
  by_project: Named[];
  cache: Cache;
  pace: Pace;
  tips: Tip[];
  sources: Source[];
  /** 逐条流水。只在请求时带了 detail 才有（后端 `#[serde(skip_serializing_if)]`）。 */
  events?: UsageEvent[];
  events_meta?: EventsMeta;
};

type T = (s: string, v?: Record<string, string | number>) => string;

/** 1.2万 / 3.4亿 —— token 数动辄上亿，原样打出来没人读得出量级。 */
function fmtTokens(n: number): string {
  if (n >= 1e8) return `${(n / 1e8).toFixed(2)}亿`;
  if (n >= 1e4) return `${(n / 1e4).toFixed(1)}万`;
  return String(n);
}

function fmtCny(n: number): string {
  if (n >= 10000) return `¥${(n / 10000).toFixed(2)}万`;
  return `¥${n.toFixed(2)}`;
}

/** 一格表盘。token 是硬数字放大字，折算金额放小字 —— 别让「估算」冒充账单。 */
function Dial({
  label,
  totals,
  hint,
  accent,
}: {
  label: string;
  totals: Totals;
  hint?: string;
  accent?: boolean;
}) {
  return (
    <div
      className={
        "rounded-card border p-4 " + (accent ? "border-accent/30 bg-accent/[0.06]" : "border-white/[0.06] bg-bg-2")
      }
    >
      <div className="text-[11.5px] text-ink-4">{label}</div>
      <div className="text-[20px] font-semibold text-ink-0 tabular-nums mt-1 leading-tight">
        {fmtTokens(totals.tokens)}
        <span className="text-[11.5px] text-ink-4 font-normal ml-1">token</span>
      </div>
      <div className="text-[12px] text-ink-3 tabular-nums mt-0.5">
        ≈ {fmtCny(totals.cny)}
        <span className="text-ink-5"> · {totals.calls} 次</span>
      </div>
      {hint ? <div className="text-[11px] text-ink-5 mt-1.5 leading-snug">{hint}</div> : null}
    </div>
  );
}

/**
 * 每日读数柱状图。
 *
 * ⚠️ 柱子的 `height: X%` 必须有一个**高度确定的直接父级**才解析得出来 ——
 * 这是 Token 压缩机那页踩过的坑（柱子塌成 0 高、只剩一行日期）。固定高度落在「柱槽」这一层。
 */
function DailyChart({ days, mode, t }: { days: DayPoint[]; mode: "tokens" | "cny"; t: T }) {
  const val = (d: DayPoint) => (mode === "tokens" ? d.tokens : d.cny);
  const max = Math.max(...days.map(val), 1);
  const peak = days.reduce((a, b) => (val(b) > val(a) ? b : a), days[0]);
  // 天数多时标签会糊成一片，隔几根标一次。
  const step = days.length > 20 ? 5 : days.length > 10 ? 2 : 1;
  return (
    <div>
      <div className="flex items-baseline justify-between mb-2.5">
        <span className="text-[11.5px] text-ink-4">
          {mode === "tokens" ? t("每天用掉的 token") : t("每天折算花费")}
        </span>
        <span className="text-[11px] text-ink-5 tabular-nums">
          {t("最高 {d}：{v}", {
            d: peak?.date.slice(5) ?? "-",
            v: mode === "tokens" ? fmtTokens(max) : fmtCny(max),
          })}
        </span>
      </div>
      <div className="flex items-end gap-[3px]">
        {days.map((d, i) => (
          <div
            key={d.date}
            className="flex-1 min-w-0 flex flex-col items-center gap-1.5 group"
            title={`${d.date} · ${fmtTokens(d.tokens)} token · ${fmtCny(d.cny)} · ${d.calls} 次`}
          >
            <div className="w-full h-24 flex items-end justify-center">
              <div
                className={
                  "w-full max-w-[40px] rounded-t transition-colors " +
                  (val(d) > 0 ? "bg-accent/70 group-hover:bg-accent" : "bg-white/[0.06]")
                }
                // 0 的那天给一条 2px 的底线：既能看出「那天没用」，又不至于什么都没有。
                style={{ height: val(d) > 0 ? `${Math.max(4, (val(d) / max) * 100)}%` : "2px" }}
              />
            </div>
            <span className="text-[9px] text-ink-5 tabular-nums h-3">
              {i % step === 0 || i === days.length - 1 ? d.date.slice(5) : ""}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** 一次问表要多少条流水。见 `load()` 里为什么不要全量。 */
const LEDGER_ROWS = 200;

/**
 * 流水（逐条明细）。
 *
 * ## 这一块在回答什么
 * 上面那些表答的是「这个月花了 ¥x」「哪个项目在耗」；这一块答的是
 * **「这笔钱是哪一轮花的」** —— 客户盯着余额往下掉时想问的就是这个（#401 原话
 * 「写文章、生成代码，消耗 Token 数太快了，10 元很快就用完了」）。他缺的不是省钱手段，
 * 是看得见钱花在哪一轮。
 *
 * ## 两件必须说出来的事
 * 1. **只列了最近 N 条**（`truncated > 0` 时）—— 悄悄截断会让人把这几条的合计
 *    当成整个窗口的合计。
 * 2. **Hermes 的行是一整个会话**，不是一轮。不标出来，一条 ¥3.7 会被读成
 *    「这一轮花了 3.7 元」，而它可能是 40 轮的合计。
 */
function Ledger({
  events,
  meta,
  t,
}: {
  events: UsageEvent[];
  meta: EventsMeta | undefined;
  t: T;
}) {
  const [tool, setTool] = useState<string>("");
  const [project, setProject] = useState<string>("");

  // 过滤项从**这批数据自己**推出来，不另写一张工具清单 —— 写死清单在这个项目里
  // 已经炸过一次（pc-***：前端写死的供应商清单跟后端真值漂移）。
  const tools = Array.from(new Map(events.map((e) => [e.tool, e.tool_label])).entries());
  const projects = Array.from(new Set(events.map((e) => e.project).filter(Boolean)));

  const shown = events.filter((e) => (!tool || e.tool === tool) && (!project || e.project === project));
  const shownCny = shown.reduce((a, e) => a + e.cny, 0);

  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
      <div className="flex items-center gap-2 mb-1 flex-wrap">
        <ListOrdered size={15} className="text-accent" />
        <span className="text-[13px] font-semibold text-ink-0">{t("流水")}</span>
        <span className="text-[11.5px] text-ink-5">{t("每一次调用花了多少")}</span>
        <div className="ml-auto flex items-center gap-1.5">
          <select
            value={tool}
            onChange={(e) => setTool(e.target.value)}
            className="h-6 rounded border border-white/[0.10] bg-bg-3 text-[11px] text-ink-2 px-1.5 outline-none"
          >
            <option value="">{t("全部工具")}</option>
            {tools.map(([id, label]) => (
              <option key={id} value={id}>{label}</option>
            ))}
          </select>
          <select
            value={project}
            onChange={(e) => setProject(e.target.value)}
            className="h-6 max-w-[150px] rounded border border-white/[0.10] bg-bg-3 text-[11px] text-ink-2 px-1.5 outline-none"
          >
            <option value="">{t("全部项目")}</option>
            {projects.map((p) => (
              <option key={p} value={p}>{p}</option>
            ))}
          </select>
        </div>
      </div>

      {shown.length === 0 ? (
        <div className="text-[12.5px] text-ink-4 py-6 text-center">{t("这个窗口里没有记录")}</div>
      ) : (
        <div className="mt-3 divide-y divide-white/[0.05] max-h-[420px] overflow-y-auto">
          {shown.map((e, i) => (
            <div key={`${e.epoch}-${e.tool}-${i}`} className="flex items-start gap-3 py-1.5">
              <span className="shrink-0 w-[92px] text-[11px] text-ink-4 tabular-nums leading-5">
                {/* 只知道哪天的（pi 退回文件名那种）就只显示日期 —— 不编一个具体时刻 */}
                {e.exact_time ? e.ts.slice(5) : `${e.ts.slice(5)} --:--`}
              </span>
              <div className="min-w-0 flex-1">
                <div className="text-[12px] text-ink-1 truncate">
                  <span className="text-ink-3">{e.tool_label}</span>
                  <span className="text-ink-5"> · </span>
                  {e.model}
                  {e.project ? (
                    <>
                      <span className="text-ink-5"> · </span>
                      <span className="text-ink-3" title={e.project_dir}>{e.project}</span>
                    </>
                  ) : null}
                </div>
                <div className="text-[10.5px] text-ink-5 tabular-nums leading-snug">
                  ↑{fmtTokens(e.input_tokens)} ↓{fmtTokens(e.output_tokens)}
                  {e.cache_read_tokens > 0 ? ` · ${t("缓存读")} ${fmtTokens(e.cache_read_tokens)}` : ""}
                  {/* 🔴 一整个会话的合计，不是一轮。不说这句就会被当成单次花费读 */}
                  {e.session_rollup ? (
                    <span className="text-amber-400/90"> · {t("整段会话合计（{n} 次调用）", { n: e.calls })}</span>
                  ) : null}
                </div>
              </div>
              <span className="shrink-0 text-[12px] text-ink-2 tabular-nums leading-5">
                {e.cny > 0 ? `≈${fmtCny(e.cny)}` : <span className="text-ink-5">{t("包月")}</span>}
              </span>
            </div>
          ))}
        </div>
      )}

      <div className="text-[11px] text-ink-5 mt-3 leading-relaxed">
        {t("这 {n} 条合计 ≈{v}", { n: shown.length, v: fmtCny(shownCny) })}
        {/* 🔴 截断必须说。「最近 200 条的合计」和「这个月的合计」在界面上长得一模一样 */}
        {meta && meta.truncated > 0
          ? t("　·　窗口里一共 {total} 条，这里只列了最近 {shown} 条（其余 {cut} 条没列，但**已经算进上面的总数**）。", {
              total: meta.total,
              shown: meta.returned,
              cut: meta.truncated,
            })
          : ""}
      </div>
    </section>
  );
}

/** 一张分账表（按项目 / 按工具 / 按模型共用）。 */
function BreakdownRows({ rows, t }: { rows: { key: string; name: string; sub?: string; cny: number; tokens: number; calls: number; share: number }[]; t: T }) {
  if (rows.length === 0) {
    return <div className="text-[12.5px] text-ink-4 py-6 text-center">{t("这个窗口里没有记录")}</div>;
  }
  return (
    <div className="space-y-2">
      {rows.map((r) => (
        <div key={r.key} className="flex items-center gap-3">
          <div className="w-[34%] shrink-0 min-w-0">
            <div className="text-[12.5px] text-ink-1 truncate" title={r.sub || r.name}>
              {r.name}
            </div>
            {r.sub ? <div className="text-[10.5px] text-ink-5 truncate">{r.sub}</div> : null}
          </div>
          <div className="flex-1 h-2 rounded-full bg-white/[0.06] overflow-hidden">
            <div className="h-full bg-accent/80 rounded-full" style={{ width: `${Math.max(1, r.share * 100)}%` }} />
          </div>
          <span className="text-[11.5px] text-accent tabular-nums w-12 text-right shrink-0 font-semibold">
            {(r.share * 100).toFixed(0)}%
          </span>
          <span className="text-[11.5px] text-ink-3 tabular-nums w-20 text-right shrink-0">{fmtCny(r.cny)}</span>
          <span className="text-[11px] text-ink-5 tabular-nums w-16 text-right shrink-0">{fmtTokens(r.tokens)}</span>
        </div>
      ))}
    </div>
  );
}

export function Meter({ onToast, onGoto }: { onToast: (m: string) => void; onGoto?: (t: TabId) => void }) {
  const { t } = useI18n();
  const [data, setData] = useState<MeterData | null>(null);
  const [busy, setBusy] = useState(false);
  const [days, setDays] = useState(30);
  const [chart, setChart] = useState<"tokens" | "cny">("tokens");
  const [view, setView] = useState<"project" | "tool" | "model">("project");
  const [showSources, setShowSources] = useState(false);
  const [savingPrefs, setSavingPrefs] = useState(false);

  const load = useCallback(
    async (d: number) => {
      setBusy(true);
      try {
        // 余额是**可选**的：只有拿到了才算得出「还能用几天」，拿不到（BYOK / 断网）就不显示，
        // 绝不猜一个数。后端拿不到余额时同样返回 days_left:null —— 判断在核心，这里只是喂料。
        let balance: number | undefined;
        try {
          const dk = await invoke<{ balance: { cny?: number } | null }>("get_device_key");
          if (typeof dk?.balance?.cny === "number") balance = dk.balance.cny;
        } catch {
          /* 查不到余额不影响这张表的其它部分 */
        }
        // detail=200：要逐条流水。**故意不要全量** —— 30 天窗口可能几万条，
        // 一次全塞进 WebView 只会让这页卡住，而人也不会去翻第 3000 条。
        // 截掉多少由后端如实回报（`events_meta.truncated`），界面照实说。
        setData(await invoke<MeterData>("query_usage_meter", { days: d, balanceCny: balance, detail: LEDGER_ROWS }));
      } catch (e) {
        onToast(t("读表失败：") + String(e));
      } finally {
        setBusy(false);
      }
    },
    [onToast, t],
  );

  useEffect(() => {
    void load(days);
  }, [days, load]);

  /**
   * 改「算哪些工具 / 哪些是包月」，改完立刻重读一次表。
   *
   * **偏好的真相源在后端那份 json**，这里不留第二份状态：从 `data.sources` 现算出两份名单
   * 再整份提交，回来重读。少一步重读，客户就会看到「勾变了、数字没变」——那种画面比不给开关更糟。
   */
  const savePrefs = useCallback(
    async (next: { disabled: string[]; subscription: string[] }) => {
      setSavingPrefs(true);
      try {
        await invoke("set_usage_sources", next);
        await load(days);
      } catch (e) {
        onToast(t("保存失败：") + String(e));
      } finally {
        setSavingPrefs(false);
      }
    },
    [days, load, onToast, t],
  );

  /** 当前两份名单（从后端回来的 sources 现算，不另存一份 state）。 */
  const prefLists = () => {
    const src = data?.sources ?? [];
    return {
      disabled: src.filter((s) => s.countable && !s.enabled).map((s) => s.tool),
      subscription: src.filter((s) => s.subscription).map((s) => s.tool),
    };
  };

  const toggleTool = (tool: string, on: boolean) => {
    const cur = prefLists();
    const disabled = on ? cur.disabled.filter((x) => x !== tool) : [...new Set([...cur.disabled, tool])];
    // 关掉一个工具时把它的「包月」标记一并撤掉 —— 留着一个看不见的标记，
    // 下次打开会莫名其妙不折钱，而界面上根本没地方看出为什么。
    const subscription = on ? cur.subscription : cur.subscription.filter((x) => x !== tool);
    return savePrefs({ disabled, subscription });
  };

  const toggleSubscription = (tool: string, on: boolean) => {
    const cur = prefLists();
    return savePrefs({
      disabled: cur.disabled,
      subscription: on ? [...new Set([...cur.subscription, tool])] : cur.subscription.filter((x) => x !== tool),
    });
  };

  const rows = (() => {
    if (!data) return [];
    if (view === "project")
      return data.by_project.map((p) => ({ key: p.name, name: p.name, sub: p.detail, cny: p.cny, tokens: p.tokens, calls: p.calls, share: p.share }));
    if (view === "tool")
      return data.by_tool.map((p) => ({ key: p.name, name: p.name, cny: p.cny, tokens: p.tokens, calls: p.calls, share: p.share }));
    const total = data.window.cny || 1;
    return data.by_model.map((m) => ({
      key: `${m.tool}/${m.model}`,
      name: m.model,
      // 工具展示名从后端那份 sources 里取 —— 别在前端再抄一张表（以前这里写死
      // `claude ? "Claude Code" : "Codex CLI"`，接进 OpenClaw/Hermes/pi 后就会把它们
      // 统统标成「Codex CLI」）。
      sub: data.sources.find((s) => s.tool === m.tool)?.label ?? m.tool,
      cny: m.cny,
      tokens: m.input_tokens + m.output_tokens,
      calls: m.count,
      share: m.cny / total,
    }));
  })();

  const uncovered = (data?.sources ?? []).filter((s) => !s.covered && s.exists);

  return (
    <div className="space-y-5">
      {/* 顶栏 */}
      <section className="flex items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-4">
        <div className="w-10 h-10 rounded-xl bg-accent/[0.12] grid place-items-center shrink-0">
          <Gauge size={20} className="text-accent" />
        </div>
        <div className="min-w-0">
          <div className="text-[16px] font-semibold text-ink-0">{t("Token 水电表")}</div>
          <div className="text-[12.5px] text-ink-3">
            {t("这台电脑上所有 AI 的 token 读数 · 全本地统计，不上传")}
          </div>
        </div>
        <div className="ml-auto flex items-center gap-2 shrink-0">
          <div className="flex items-center gap-1 rounded-lg bg-white/[0.04] p-0.5">
            {[7, 30, 90].map((d) => (
              <button
                key={d}
                onClick={() => setDays(d)}
                className={
                  "px-2.5 h-7 rounded-md text-[12px] transition-colors " +
                  (days === d ? "bg-accent/20 text-accent font-medium" : "text-ink-4 hover:text-ink-2")
                }
              >
                {t("{n} 天", { n: d })}
              </button>
            ))}
          </div>
          <button
            // 绑定核对用：这个按钮背后就是影核动作，`action bindings` 能查到它没点空。
            data-action-id="runtime.usage_meter.inspect"
            onClick={() => void load(days)}
            disabled={busy}
            className="w-8 h-8 grid place-items-center rounded-lg bg-white/[0.06] text-ink-3 hover:text-ink-1 disabled:opacity-50"
            title={t("重新读表")}
          >
            {busy ? <Loader2 size={15} className="animate-spin" /> : <RefreshCw size={15} />}
          </button>
        </div>
      </section>

      {!data ? (
        <div className="grid place-items-center py-16 text-ink-4">
          <Loader2 size={22} className="animate-spin" />
        </div>
      ) : !data.ready ? (
        // 读不到日志不是错误，是这台机器的事实 —— 说清为什么，别摆一堆 0 假装正常。
        <section className="rounded-card border border-white/[0.06] bg-bg-2 p-8 text-center space-y-3">
          <Gauge size={26} className="text-ink-5 mx-auto" />
          <div className="text-[14px] text-ink-1 font-medium">{t("这块表还没有读数")}</div>
          {data.blockers.map((b) => (
            <p key={b} className="text-[12.5px] text-ink-3 leading-relaxed max-w-xl mx-auto">
              {b}
            </p>
          ))}
          <p className="text-[12px] text-ink-4">
            {t("用 Claude Code 或 Codex CLI 干点活，回来再读一次就有了。")}
          </p>
        </section>
      ) : (
        <>
          {/*
            ── 口径条：必须在表盘**上面**，不能塞进角落 ──
            本地日志只记 token，不记你实际付了多少。金额一律按公开列表价折算。
            包月订阅用户看到的那个大数不是账单，是「同样的量走 API 要花多少」——
            对他们来说这恰恰是「订阅帮我省了多少」。不解释清楚，一台重度机 30 天折出 ¥25 万会直接吓跑人。
          */}
          <section className="rounded-card border border-white/[0.06] bg-white/[0.02] px-4 py-3 flex gap-2.5">
            <Info size={14} className="text-ink-4 shrink-0 mt-0.5" />
            <p className="text-[12px] text-ink-3 leading-relaxed">
              {t("token 数是从各 AI 工具自己的会话日志里实测出来的，准。")}
              <b className="text-ink-2">{t("金额是按各模型「公开报价」折算的参考值，不是你的账单")}</b>
              {t("——本地日志不知道你走的是哪家、什么价。如果你用的是包月订阅（Claude Max / ChatGPT Plus 等），把这个金额读成「同样的量走 API 要花多少」，也就是订阅替你省下的部分。")}
            </p>
          </section>

          {/* ── 表盘 ── */}
          <section className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            <Dial
              label={t("今天")}
              totals={data.today}
              accent
              hint={
                data.pace.today_vs_avg > 0
                  ? t("是日均的 {n} 倍", { n: data.pace.today_vs_avg.toFixed(1) })
                  : undefined
              }
            />
            <Dial label={t("昨天")} totals={data.yesterday} />
            <Dial label={t("最近 7 天")} totals={data.last7} />
            <Dial label={t("最近 {n} 天合计", { n: data.days })} totals={data.window} />
          </section>

          {/* ── 用得多快 ── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
            <div className="flex items-center gap-2 mb-3">
              <TrendingUp size={15} className="text-accent" />
              <span className="text-[13px] font-semibold text-ink-0">{t("用得多快")}</span>
            </div>
            <div className="grid grid-cols-2 lg:grid-cols-3 gap-4">
              <div>
                <div className="text-[11.5px] text-ink-4">{t("日均")}</div>
                <div className="text-[17px] font-semibold text-ink-0 tabular-nums">{fmtCny(data.pace.daily_avg_cny)}</div>
              </div>
              <div>
                <div className="text-[11.5px] text-ink-4">{t("照这个速度，一个月")}</div>
                <div className="text-[17px] font-semibold text-ink-0 tabular-nums">
                  {fmtCny(data.pace.month_projection_cny)}
                </div>
              </div>
              {/* 没余额就整块不渲染 —— 宁可少一格，也不显示一个猜出来的天数。 */}
              {data.pace.days_left !== null && data.pace.balance_cny !== null ? (
                <div>
                  <div className="text-[11.5px] text-ink-4 flex items-center gap-1">
                    <Wallet size={11} />
                    {t("余额 {b} 还能用", { b: fmtCny(data.pace.balance_cny) })}
                  </div>
                  <div
                    className={
                      "text-[17px] font-semibold tabular-nums " +
                      (data.pace.days_left < 7 ? "text-amber-400" : "text-ink-0")
                    }
                  >
                    {t("约 {n} 天", { n: Math.max(0, Math.round(data.pace.days_left)) })}
                  </div>
                </div>
              ) : null}
            </div>
          </section>

          {/* ── 每日曲线 ── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
            <div className="flex items-center justify-between mb-1">
              <span className="text-[13px] font-semibold text-ink-0">{t("每日读数")}</span>
              <div className="flex items-center gap-1 rounded-lg bg-white/[0.04] p-0.5">
                {(["tokens", "cny"] as const).map((m) => (
                  <button
                    key={m}
                    onClick={() => setChart(m)}
                    className={
                      "px-2.5 h-6 rounded-md text-[11.5px] transition-colors " +
                      (chart === m ? "bg-accent/20 text-accent font-medium" : "text-ink-4 hover:text-ink-2")
                    }
                  >
                    {m === "tokens" ? t("token") : t("折算 ¥")}
                  </button>
                ))}
              </div>
            </div>
            <DailyChart days={data.daily} mode={chart} t={t} />
          </section>

          {/* ── 分账 ── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
            <div className="flex items-center justify-between mb-3.5">
              <span className="text-[13px] font-semibold text-ink-0">{t("花在哪了")}</span>
              <div className="flex items-center gap-1 rounded-lg bg-white/[0.04] p-0.5">
                {([
                  ["project", t("按项目"), FolderTree],
                  ["tool", t("按工具"), Bot],
                  ["model", t("按模型"), Cpu],
                ] as const).map(([k, label, Icon]) => (
                  <button
                    key={k}
                    onClick={() => setView(k)}
                    className={
                      "inline-flex items-center gap-1.5 px-2.5 h-6 rounded-md text-[11.5px] transition-colors " +
                      (view === k ? "bg-accent/20 text-accent font-medium" : "text-ink-4 hover:text-ink-2")
                    }
                  >
                    <Icon size={11} />
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <BreakdownRows rows={rows.slice(0, 12)} t={t} />
            {rows.length > 12 ? (
              // 截断了就说出来 —— 悄悄只画前 12 条，客户会以为那就是全部。
              <div className="text-[11px] text-ink-5 mt-3">
                {t("只列了花费最高的 12 项，还有 {n} 项没列。", { n: rows.length - 12 })}
              </div>
            ) : null}
          </section>

          {/* ── 流水（逐条明细）── 分账答「哪个项目在耗」，这块答「哪一轮花的」 */}
          {data.events && data.events.length > 0 && (
            <Ledger events={data.events} meta={data.events_meta} t={t} />
          )}

          {/* ── 缓存账 ── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
            <div className="flex items-center gap-2 mb-3">
              <Database size={15} className="text-accent" />
              <span className="text-[13px] font-semibold text-ink-0">{t("上下文缓存")}</span>
              <span
                className={
                  "ml-auto text-[12px] font-semibold tabular-nums " +
                  (data.cache.hit_rate >= 0.6 ? "text-emerald-400" : data.cache.hit_rate >= 0.35 ? "text-ink-2" : "text-amber-400")
                }
              >
                {t("命中率 {n}%", { n: (data.cache.hit_rate * 100).toFixed(0) })}
              </span>
            </div>
            <div className="h-2 rounded-full bg-white/[0.06] overflow-hidden mb-3">
              <div
                className={
                  "h-full rounded-full " + (data.cache.hit_rate >= 0.6 ? "bg-emerald-500/70" : "bg-amber-500/70")
                }
                style={{ width: `${Math.max(1, data.cache.hit_rate * 100)}%` }}
              />
            </div>
            <p className="text-[12px] text-ink-3 leading-relaxed">
              {t("命中缓存的输入只按原价约 1/10 收费。这个窗口里缓存已经替你省下约 ")}
              <b className="text-emerald-400">{fmtCny(data.cache.saved_cny)}</b>
              {t("（同样按公开报价折算）。命中率越高越省——一直重建缓存（每次开新会话、频繁改 CLAUDE.md、来回切模型）就是一直在多花钱。")}
            </p>
          </section>

          {/* ── 建议 ── */}
          {data.tips.length > 0 && (
            <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
              <div className="flex items-center gap-2 mb-3">
                <Lightbulb size={15} className="text-accent" />
                <span className="text-[13px] font-semibold text-ink-0">{t("怎么省")}</span>
                <span className="text-[11px] text-ink-5">{t("（本地算出来的，不烧 token）")}</span>
              </div>
              <div className="space-y-3">
                {data.tips.map((tip) => (
                  <div key={tip.id} className="rounded-lg border border-white/[0.06] bg-white/[0.02] p-3.5">
                    <div className="flex items-baseline gap-2 flex-wrap">
                      <span className="text-[12.5px] font-medium text-ink-1">{tip.title}</span>
                      {/* saving=0 是「算不准，就不给数」的信号，别渲染成 ¥0.00 */}
                      {tip.saving_cny > 0 ? (
                        <span className="text-[11px] px-1.5 py-0.5 rounded bg-emerald-500/[0.12] text-emerald-400 font-medium tabular-nums">
                          {t("每月约省 {v}", { v: fmtCny(tip.saving_cny) })}
                        </span>
                      ) : null}
                    </div>
                    <p className="text-[12px] text-ink-3 mt-1.5 leading-relaxed">{tip.detail}</p>
                    {tip.id === "enable_squeezer" && onGoto ? (
                      <button
                        onClick={() => onGoto("rtk")}
                        className="mt-2 text-[12px] text-accent hover:underline"
                      >
                        {t("去开 Token 压缩机 →")}
                      </button>
                    ) : null}
                    {tip.id === "switch_cheap_model" && onGoto ? (
                      <button
                        onClick={() => onGoto("manage")}
                        className="mt-2 text-[12px] text-accent hover:underline"
                      >
                        {t("去换模型 →")}
                      </button>
                    ) : null}
                  </div>
                ))}
              </div>
            </section>
          )}

          {/* ── 数据来源：算哪些工具，用户说了算；算不到的也如实列出来 ── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2">
            <button
              onClick={() => setShowSources((v) => !v)}
              className="w-full flex items-center gap-2 px-5 py-3.5 text-left"
            >
              {uncovered.length > 0 ? (
                <AlertTriangle size={14} className="text-amber-400 shrink-0" />
              ) : (
                <EyeOff size={14} className="text-ink-4 shrink-0" />
              )}
              <span className="text-[12.5px] text-ink-2">
                {uncovered.length > 0
                  ? t("数据来源 · 算哪些工具（有 {n} 个装了但没算进来）", { n: uncovered.length })
                  : t("数据来源 · 算哪些工具")}
              </span>
              <span className="ml-auto text-[11.5px] text-ink-5">{showSources ? t("收起") : t("展开")}</span>
            </button>
            {showSources && (
              <div className="px-5 pb-5 space-y-3">
                <p className="text-[11.5px] text-ink-4 leading-relaxed">
                  {t("下面是这台电脑上探测到的**全部** AI 工具。勾上的才算进上面的数字；灰掉的是「本机读不到它的账」，勾也没用 —— 原因逐条写在下面。")}
                </p>
                {data.sources.map((s) => (
                  <div
                    key={s.tool}
                    className={
                      "rounded-lg border px-3 py-2.5 " +
                      (s.countable ? "border-white/[0.07] bg-white/[0.015]" : "border-white/[0.04] bg-transparent")
                    }
                  >
                    <div className="flex items-start gap-2.5">
                      {/* 算得到的给真勾选框；算不到的给一个明确的「不可用」标记，不给假开关 */}
                      {s.countable ? (
                        <button
                          onClick={() => void toggleTool(s.tool, !s.enabled)}
                          disabled={savingPrefs}
                          title={s.enabled ? t("不算它") : t("算上它")}
                          className={
                            "mt-0.5 w-4 h-4 rounded shrink-0 grid place-items-center border transition-colors disabled:opacity-50 " +
                            (s.enabled ? "bg-accent border-accent text-white" : "border-white/25 hover:border-white/50")
                          }
                        >
                          {s.enabled && <Check size={11} />}
                        </button>
                      ) : (
                        <span
                          title={t("这台机器上读不到它的用量")}
                          className="mt-0.5 w-4 h-4 rounded shrink-0 grid place-items-center border border-white/10 text-ink-6"
                        >
                          <Minus size={10} />
                        </span>
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className={s.countable ? "text-ink-1 text-[12.5px]" : "text-ink-4 text-[12.5px]"}>
                            {s.label}
                          </span>
                          {s.covered && s.files > 0 && (
                            <span className="text-[10px] px-1.5 h-[16px] inline-flex items-center rounded-full bg-emerald-500/12 text-emerald-400 border border-emerald-500/20">
                              {t("已算入 · {n} 份记录", { n: s.files })}
                            </span>
                          )}
                          {!s.countable && (
                            <span className="text-[10px] px-1.5 h-[16px] inline-flex items-center rounded-full bg-white/[0.05] text-ink-5 border border-white/[0.06]">
                              {t("读不到")}
                            </span>
                          )}
                          {!s.exists && s.countable && (
                            <span className="text-[10px] px-1.5 h-[16px] inline-flex items-center rounded-full bg-white/[0.05] text-ink-5 border border-white/[0.06]">
                              {t("没用过")}
                            </span>
                          )}
                          <span className="text-ink-6 font-mono text-[10.5px] truncate">{s.dir}</span>
                        </div>
                        {s.note && <div className="text-ink-4 text-[11.5px] leading-relaxed mt-1">{s.note}</div>}
                        {/* 包月开关：只对算得到的工具给 —— 它决定「token 要不要折成钱」 */}
                        {s.countable && s.enabled && (
                          <button
                            onClick={() => void toggleSubscription(s.tool, !s.subscription)}
                            disabled={savingPrefs}
                            className={
                              "mt-1.5 inline-flex items-center gap-1.5 text-[11px] transition-colors disabled:opacity-50 " +
                              (s.subscription ? "text-warning-700 dark:text-warning-400" : "text-ink-5 hover:text-ink-3")
                            }
                          >
                            <span
                              className={
                                "w-3.5 h-3.5 rounded-sm grid place-items-center border " +
                                (s.subscription ? "bg-amber-400/80 border-amber-400 text-black" : "border-white/20")
                              }
                            >
                              {s.subscription && <Check size={9} />}
                            </span>
                            {t("我是包月订阅（如 Claude Pro / ChatGPT Plus）—— 只算 token，不折算成钱")}
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                ))}
                <p className="text-[11.5px] text-ink-5 pt-1 leading-relaxed">
                  {t("全部统计在你自己电脑上完成：只读各工具记录里的 token 数、模型名、时间和项目目录，从不读取对话内容，也从不上传。")}
                </p>
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}
