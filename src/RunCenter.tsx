import { useEffect, useMemo, useState } from "react";
import replayReport from "../docs/replay-report-20260828.md?raw";
import { aggregateTraceMetrics, loadLocalTraces, spanDurationMs, type FlatSpan } from "./lib/otel/trace-store";

type Replay = { baseline: Record<string, string>; cached: Record<string, string> } | null;
function readReplay(raw: string): Replay {
  const rows = raw.split("\n").filter((line) => line.startsWith("| "));
  const values = rows.slice(2, 6).map((line) => line.split("|").map((value) => value.trim()));
  if (values.length < 4) return null;
  return { baseline: Object.fromEntries(values.map((row) => [row[1], row[2]])), cached: Object.fromEntries(values.map((row) => [row[1], row[3]])) };
}
function tone(name: string) { return name.startsWith("origin.team.") ? "bg-violet-500" : name.startsWith("origin.") ? "bg-sky-500" : name.startsWith("gen_ai.") ? "bg-emerald-500" : "bg-slate-500"; }
function nano(value: string) { return Number(value) / 1_000_000; }
function details(value: string | number | boolean | undefined) { return value == null ? "—" : String(value); }

export function RunCenter() {
  const [spans, setSpans] = useState<FlatSpan[]>([]);
  const [source, setSource] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | undefined>();
  const [selected, setSelected] = useState<FlatSpan | null>(null);
  useEffect(() => { void loadLocalTraces().then((result) => { setSpans(result.spans); setSource(result.source); setWarning(result.warning); }); }, []);
  const metrics = useMemo(() => aggregateTraceMetrics(spans), [spans]);
  const waterfall = spans.slice(-20);
  const range = useMemo(() => { const starts = waterfall.map((span) => nano(span.startTimeUnixNano)); const ends = waterfall.map((span) => nano(span.endTimeUnixNano)); return { start: Math.min(...starts, 0), duration: Math.max(1, Math.max(...ends, 1) - Math.min(...starts, 0)) }; }, [waterfall]);
  const replay = readReplay(replayReport);
  return <div className="max-w-6xl mx-auto space-y-5 pb-8">
    <header className="rounded-card border border-ink-6 bg-bg-1 p-5 shadow-card"><p className="text-xs text-ink-3">OpenTelemetry · 本地可视化</p><h1 className="mt-1 text-xl font-semibold text-ink-0">运行中心</h1><p className="mt-2 text-xs text-ink-3">{source ? `数据源：${source}` : warning || "等待 trace 数据"}</p></header>
    <section className="grid gap-3 sm:grid-cols-4"><Metric label="TTFT P50" value={metrics.p50TtftMs ? `${metrics.p50TtftMs} ms` : "—"}/><Metric label="总 Tokens" value={metrics.totalTokens.toLocaleString()}/><Metric label="估算成本" value={`$${metrics.totalCost.toFixed(6)}`}/><Metric label="Span 数" value={String(metrics.spanCount)}/></section>
    {!spans.length ? <section className="rounded-card border border-dashed border-ink-5 bg-bg-1 p-10 text-center text-sm text-ink-3">启动本地 collector 后运行团队空间操作即可采集。开发态从仓库 <code>otel-traces/</code> 读取；打包版不会直接访问开发机文件。</section> : <section className="rounded-card border border-ink-6 bg-bg-1 p-5 shadow-card"><div className="flex items-center justify-between"><h2 className="font-semibold text-ink-0">Trace 瀑布</h2><span className="text-xs text-ink-3">最近 {waterfall.length} 个 span · 同 traceId 相邻成组</span></div><div className="mt-4 space-y-2">{waterfall.map((span, index) => { const left = ((nano(span.startTimeUnixNano) - range.start) / range.duration) * 100; const width = Math.max(1, spanDurationMs(span) / range.duration * 100); return <button key={`${span.spanId}-${index}`} onClick={() => setSelected(span)} className="grid w-full grid-cols-[120px_1fr_66px] items-center gap-3 text-left"><span className="truncate font-mono text-[11px] text-ink-3" title={span.traceId}>{span.traceId.slice(0, 10) || "无 trace"}</span><span className="relative h-6 rounded bg-bg-0"><span className={`absolute top-1 h-4 rounded ${tone(span.name)}`} style={{ left: `${left}%`, width: `${width}%` }} title={`${span.name} ${spanDurationMs(span).toFixed(1)}ms`}/><span className="absolute inset-y-0 left-2 flex items-center text-[11px] text-ink-1 mix-blend-screen">{span.name}</span></span><span className="text-right text-[11px] text-ink-3">{spanDurationMs(span).toFixed(1)}ms</span></button>; })}</div></section>}
    <section className="rounded-card border border-ink-6 bg-bg-1 p-5 shadow-card"><h2 className="font-semibold text-ink-0">A/B 回测对比</h2>{replay ? <div className="mt-4 grid gap-3 sm:grid-cols-4">{["success_rate", "total_tokens", "total_cost", "p50_ttft_ms"].map((key) => <div key={key} className="rounded-lg bg-bg-0 p-3"><p className="text-xs text-ink-3">{key}</p><p className="mt-2 text-sm text-ink-1">基线 {replay.baseline[key]}</p><p className="text-sm text-emerald-600 dark:text-emerald-400">优化 {replay.cached[key]}</p>{key === "total_cost" && <span className="mt-2 inline-block rounded-full bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-700 dark:text-emerald-300">-22.2% 成本</span>}</div>)}</div> : <p className="mt-3 text-sm text-ink-3">未找到回测结果。运行 <code>node scripts/replay/run.mjs</code> 两次生成 baseline/cached 结果。</p>}</section>
    {selected && <aside className="rounded-card border border-accent/30 bg-bg-1 p-5 shadow-card"><div className="flex items-center justify-between"><h2 className="font-semibold text-ink-0">Span 详情 · {selected.name}</h2><button onClick={() => setSelected(null)} className="text-xs text-ink-3 hover:text-ink-1">关闭</button></div><dl className="mt-3 grid gap-2 text-xs sm:grid-cols-2"><Detail label="kind" value={selected.kind}/><Detail label="status" value={selected.status?.code || "—"}/><Detail label="service.name" value={details(selected.resource["service.name"])}/><Detail label="events" value={selected.events.map((event) => event.name).join("、") || "—"}/></dl><Table title="attributes" values={selected.attributes}/><Table title="resource" values={selected.resource}/></aside>}
  </div>;
}
function Metric({ label, value }: { label: string; value: string }) { return <div className="rounded-card border border-ink-6 bg-bg-1 p-4 shadow-card"><p className="text-xs text-ink-3">{label}</p><p className="mt-1 font-semibold text-ink-0">{value}</p></div>; }
function Detail({ label, value }: { label: string; value: string }) { return <div><dt className="text-ink-3">{label}</dt><dd className="mt-0.5 break-all text-ink-1">{value}</dd></div>; }
function Table({ title, values }: { title: string; values: Record<string, string | number | boolean | undefined> }) { return <div className="mt-4"><h3 className="text-xs font-medium text-ink-2">{title}</h3><div className="mt-2 divide-y divide-ink-6 rounded border border-ink-6">{Object.entries(values).map(([key, value]) => <div key={key} className="grid grid-cols-2 gap-3 px-3 py-2 text-xs"><span className="break-all text-ink-3">{key}</span><span className="break-all text-ink-1">{details(value)}</span></div>)}</div></div>; }
