import { appendFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { createHash, randomBytes } from "node:crypto";

const localTraceFile = resolve("otel-traces/replay-traces.otlp.jsonl");
const attr = (key, value) => ({ key, value: typeof value === "number" ? { doubleValue: value } : { stringValue: String(value) } });
const nano = (ms) => `${Math.trunc(ms)}000000`;

/** Emit the same standard ExportTraceServiceRequest the WebView sends. */
export async function emitReplaySpan({ id, model, tokens, ttftMs, cached, startMs, endMs }) {
  const traceId = createHash("sha256").update(`${id}:${startMs}:${randomBytes(4).toString("hex")}`).digest("hex").slice(0, 32);
  const span = {
    traceId,
    spanId: randomBytes(8).toString("hex"),
    name: "gen_ai.chat",
    kind: "SPAN_KIND_INTERNAL",
    startTimeUnixNano: nano(startMs),
    endTimeUnixNano: nano(endMs),
    attributes: [
      attr("gen_ai.operation.name", "replay"), attr("gen_ai.request.model", model),
      attr("gen_ai.ttft_ms", ttftMs), attr("gen_ai.usage.total_tokens", tokens),
      attr("uking.replay.cached", String(cached)), attr("uking.replay.task_id", id),
    ],
    events: [{ timeUnixNano: nano(startMs + ttftMs), name: "gen_ai.first_token" }],
    status: { code: "STATUS_CODE_OK" },
  };
  const payload = { resourceSpans: [{ resource: { attributes: [attr("service.name", "u-king-mini.replay")] }, scopeSpans: [{ scope: { name: "u-king-mini.agentloop.replay", version: "1.1.0" }, spans: [span] }] }] };
  const endpoint = process.env.OTEL_ENDPOINT;
  if (endpoint) {
    const response = await fetch(endpoint, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(payload) });
    if (!response.ok) throw new Error(`OTLP collector returned HTTP ${response.status}`);
  } else {
    await mkdir(dirname(localTraceFile), { recursive: true });
    await appendFile(localTraceFile, `${JSON.stringify(payload)}\n`, "utf8");
  }
  return traceId;
}
