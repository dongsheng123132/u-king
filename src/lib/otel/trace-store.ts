export type TraceValue = string | number | boolean | undefined;
export type FlatSpan = { traceId: string; spanId: string; name: string; kind: string; startTimeUnixNano: string; endTimeUnixNano: string; attributes: Record<string, TraceValue>; events: { name: string; timeUnixNano?: string; attributes?: Record<string, TraceValue> }[]; status?: { code?: string; message?: string }; resource: Record<string, TraceValue> };
export type TraceMetrics = { spanCount: number; totalTokens: number; totalCost: number; p50TtftMs: number };
export type TraceLoad = { spans: FlatSpan[]; source: string | null; warning?: string };

function valueOf(value: Record<string, unknown> | undefined): TraceValue {
  if (!value) return undefined;
  for (const key of ["stringValue", "doubleValue", "intValue", "boolValue"]) if (key in value) return value[key] as TraceValue;
  return undefined;
}
function attributes(input: { key?: string; value?: Record<string, unknown> }[] | undefined) { return Object.fromEntries((input || []).map((item) => [item.key || "", valueOf(item.value)]).filter(([key]) => key)); }
export function parseTraceJsonl(raw: string): FlatSpan[] {
  const output: FlatSpan[] = [];
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    try {
      const payload = JSON.parse(line) as { resourceSpans?: { resource?: { attributes?: { key?: string; value?: Record<string, unknown> }[] }; scopeSpans?: { spans?: Record<string, unknown>[] }[] }[] };
      const flat = payload as unknown as Record<string, unknown>;
      if (flat.name && flat.traceId) {
        output.push({ traceId: String(flat.traceId), spanId: String(flat.spanId || ""), name: String(flat.name), kind: String(flat.kind || "SPAN_KIND_INTERNAL"), startTimeUnixNano: String(flat.startTimeUnixNano || "0"), endTimeUnixNano: String(flat.endTimeUnixNano || "0"), attributes: attributes(flat.attributes as { key?: string; value?: Record<string, unknown> }[]), events: ((flat.events as Record<string, unknown>[] || []).map((event) => ({ name: String(event.name || "event"), timeUnixNano: event.timeUnixNano ? String(event.timeUnixNano) : undefined, attributes: attributes(event.attributes as { key?: string; value?: Record<string, unknown> }[]) }))), status: flat.status as FlatSpan["status"], resource: attributes((flat.resource as { attributes?: { key?: string; value?: Record<string, unknown> }[] } | undefined)?.attributes) });
        continue;
      }
      for (const resourceSpan of payload.resourceSpans || []) for (const scope of resourceSpan.scopeSpans || []) for (const span of scope.spans || []) {
        output.push({ traceId: String(span.traceId || ""), spanId: String(span.spanId || ""), name: String(span.name || "unnamed"), kind: String(span.kind || "SPAN_KIND_INTERNAL"), startTimeUnixNano: String(span.startTimeUnixNano || "0"), endTimeUnixNano: String(span.endTimeUnixNano || "0"), attributes: attributes(span.attributes as { key?: string; value?: Record<string, unknown> }[]), events: ((span.events as Record<string, unknown>[] || []).map((event) => ({ name: String(event.name || "event"), timeUnixNano: event.timeUnixNano ? String(event.timeUnixNano) : undefined, attributes: attributes(event.attributes as { key?: string; value?: Record<string, unknown> }[]) }))), status: span.status as FlatSpan["status"], resource: attributes(resourceSpan.resource?.attributes) });
      }
    } catch { /* 收集器尾部半行或非 OTLP 行不影响已完成的记录 */ }
  }
  return output.sort((a, b) => Number(a.startTimeUnixNano) - Number(b.startTimeUnixNano));
}
function percentile(values: number[], p: number) { if (!values.length) return 0; const sorted = [...values].sort((a, b) => a - b); return sorted[Math.floor((sorted.length - 1) * p)]; }
export function aggregateTraceMetrics(spans: FlatSpan[]): TraceMetrics {
  const tokens = spans.reduce((sum, span) => sum + Number(span.attributes["gen_ai.usage.total_tokens"] || 0), 0);
  return { spanCount: spans.length, totalTokens: tokens, totalCost: tokens * 0.002 / 1000, p50TtftMs: percentile(spans.map((span) => Number(span.attributes["gen_ai.ttft_ms"])).filter(Number.isFinite), 0.5) };
}
export function spanDurationMs(span: FlatSpan) { return Math.max(0, (Number(span.endTimeUnixNano) - Number(span.startTimeUnixNano)) / 1_000_000); }
/** Vite 开发服务器会把仓库根目录作为可读根；生产包不含运行时 trace 文件，故返回明确空态。 */
export async function loadLocalTraces(): Promise<TraceLoad> {
  for (const source of ["/otel-traces/traces.otlp.jsonl", "/otel-traces/replay-traces.otlp.jsonl"]) {
    try { const response = await fetch(source, { cache: "no-store" }); if (response.ok) return { spans: parseTraceJsonl(await response.text()), source }; } catch { /* 尝试回放文件 */ }
  }
  return { spans: [], source: null, warning: "未找到 traces.otlp.jsonl；打包版不会直接读取开发机文件。" };
}
