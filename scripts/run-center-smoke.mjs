import { readFile, access } from "node:fs/promises";
import { constants } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const candidates = [join(root, "otel-traces", "traces.otlp.jsonl"), join(root, "otel-traces", "replay-traces.otlp.jsonl")];
let source;
// 显式按优先级确认，保持 smoke 在没有 collector 时也可跑。
for (const file of candidates) { try { await access(file, constants.F_OK); source = file; break; } catch { /* next */ } }
const fallback = JSON.stringify({ resourceSpans: [{ resource: { attributes: [{ key: "service.name", value: { stringValue: "run-center-smoke" } }] }, scopeSpans: [{ spans: [{ traceId: "smoke-trace", spanId: "smoke-span", name: "gen_ai.chat", kind: "SPAN_KIND_INTERNAL", startTimeUnixNano: "1000000000", endTimeUnixNano: "1063000000", attributes: [{ key: "gen_ai.ttft_ms", value: { doubleValue: 42 } }, { key: "gen_ai.usage.total_tokens", value: { doubleValue: 120 } }] }] }] }] });
const raw = source ? await readFile(source, "utf8") : `${fallback}\n`;
function attr(items = []) { return Object.fromEntries(items.map(({ key, value }) => [key, value?.stringValue ?? value?.doubleValue ?? value?.intValue ?? value?.boolValue])); }
const spans = raw.split(/\r?\n/).flatMap((line) => { try { const payload = JSON.parse(line); if (payload.traceId && payload.name) return [{ ...payload, attributes: attr(payload.attributes), resource: attr(payload.resource?.attributes) }]; return (payload.resourceSpans || []).flatMap((resourceSpan) => (resourceSpan.scopeSpans || []).flatMap((scope) => (scope.spans || []).map((span) => ({ ...span, attributes: attr(span.attributes), resource: attr(resourceSpan.resource?.attributes) })))); } catch { return []; } });
const tokens = spans.reduce((sum, span) => sum + Number(span.attributes["gen_ai.usage.total_tokens"] || 0), 0);
const ttft = spans.map((span) => Number(span.attributes["gen_ai.ttft_ms"])).filter(Number.isFinite).sort((a, b) => a - b);
const p50 = ttft.length ? ttft[Math.floor((ttft.length - 1) * 0.5)] : 0;
const sample = spans[0];
const duration = sample ? (Number(sample.endTimeUnixNano) - Number(sample.startTimeUnixNano)) / 1_000_000 : 0;
console.log(`run-center source=${source || "constructed fixture"}`);
console.log(`metrics tasks=${new Set(spans.map((span) => span.traceId)).size} ttft_p50=${p50}ms tokens=${tokens} cost=$${(tokens * 0.002 / 1000).toFixed(6)} spans=${spans.length}`);
console.log(`waterfall sample trace=${sample?.traceId || "—"} name=${sample?.name || "—"} duration=${duration.toFixed(1)}ms`);
if (!spans.length) process.exitCode = 1;
