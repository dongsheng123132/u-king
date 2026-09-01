/**
 * AgentLoop / LoongSuite 的 WebView 安全 OTel 入口。
 *
 * Tauri WebView 不能写本地文件，也不能依赖 Node SDK；这里直接发送 OTLP/HTTP
 * JSON（ExportTraceServiceRequest），因此 collector、Jaeger 或任一兼容 Collector
 * 都能作为接收端。Node 任务仍可用 jsonl/console 兼容模式。
 */
import { trace, type Attributes, type Tracer } from "@opentelemetry/api";

export type OtelExporter = "console" | "otlp" | "jsonl" | "none";
export type OtelConfig = { serviceName: string; tracesExporter: OtelExporter; otlpEndpoint: string; sampler: string };
export type GenAiCallOptions = { model: string; operation?: string; input?: string; inputSummary?: string; attributes?: Attributes };
export type GenAiResponse = { id?: string | null; promptTokens?: number | null; completionTokens?: number | null; totalTokens?: number | null; output?: string | null };
export type GenAiCallHooks = { firstToken(): void; tool(name: string, input?: unknown, output?: unknown): void; response(response: GenAiResponse): void; attribute(name: string, value: string | number | boolean): void };

type OtelValue = string | number | boolean;
type OtelAttribute = { key: string; value: Record<string, OtelValue> };
type OtelEvent = { timeUnixNano: string; name: string; attributes?: OtelAttribute[] };

let initPromise: Promise<void> | undefined;
let configured: OtelConfig | undefined;

function runtimeEnv(name: string): string | undefined {
  const root = globalThis as { process?: { env?: Record<string, string | undefined> }; __OTEL_ENV__?: Record<string, string | undefined> };
  // __OTEL_ENV__ 供 Tauri embedding 在运行时注入；Node 则走真实环境变量。
  return root.__OTEL_ENV__?.[name] || root.process?.env?.[name];
}
function isNodeRuntime(): boolean { return Boolean((globalThis as { process?: { versions?: { node?: string } } }).process?.versions?.node); }
type DynamicModule = Record<string, unknown>;
const dynamicImport = (specifier: string): Promise<DynamicModule> => Function("specifier", "return import(specifier)")(specifier) as Promise<DynamicModule>;
// 默认 none（2026-08-31 r3 会审 opus 条件3）：观测是可选基础设施，没显式 OTEL_* 配置就不发——
// 否则客户机每个 span 都要走一次 127.0.0.1:4318 连接失败路径。要开 trace：设 OTEL_TRACES_EXPORTER=otlp（+可选 ENDPOINT）。
function safeExporter(value: string | undefined): OtelExporter { return value === "console" || value === "otlp" || value === "jsonl" ? value : "none"; }
export function readOtelConfig(): OtelConfig {
  return { serviceName: runtimeEnv("OTEL_SERVICE_NAME") || "u-king-mini", tracesExporter: safeExporter(runtimeEnv("OTEL_TRACES_EXPORTER")), otlpEndpoint: runtimeEnv("OTEL_EXPORTER_OTLP_ENDPOINT") || "http://127.0.0.1:4318/v1/traces", sampler: runtimeEnv("OTEL_TRACES_SAMPLER") || "always_on" };
}
/** 保留标准 API 给其它 OTel 调用方；GenAI 样张由下方 OTLP/JSON 路径导出。 */
export function getTracer(): Tracer { return trace.getTracer("u-king-mini.agentloop", "1.1.0"); }
/** 初始化只读取一次配置，且永不阻塞业务。实际导出延后至 span 结束。 */
export function initOtel(): Promise<void> { if (!initPromise) { configured = readOtelConfig(); initPromise = Promise.resolve(); } return initPromise; }
/** 兼容短命 Node 调用方；fetch 请求在 span 结束时自行 catch，不需要额外 flush。 */
export function shutdownOtel(): Promise<void> { return Promise.resolve(); }

/** 非模型业务动作的最小 span（团队空间 Lease/Receipt 等可复用）。观测永不阻断业务。 */
export async function recordOtelSpan(name: string, attributes: Record<string, string | number | boolean> = {}): Promise<void> {
  await initOtel();
  const startedAt = Date.now();
  void exportSpan({
    traceId: randomHex(16), spanId: randomHex(8), name, kind: "SPAN_KIND_INTERNAL",
    startTimeUnixNano: unixNano(startedAt), endTimeUnixNano: unixNano(Date.now()),
    attributes: attrs(attributes), status: { code: "STATUS_CODE_OK" },
  });
}

function boundedSummary(value: unknown): string {
  const raw = typeof value === "string" ? value : JSON.stringify(value ?? null) || "";
  const compact = raw.replace(/\s+/g, " ").trim();
  return compact.length > 120 ? `${compact.slice(0, 120)}…` : compact;
}
function summaryAttrs(prefix: string, value: unknown): Attributes {
  const raw = typeof value === "string" ? value : JSON.stringify(value ?? null) || "";
  return { [`${prefix}.length`]: raw.length, [`${prefix}.summary`]: boundedSummary(raw) };
}
function toAttribute(key: string, value: unknown): OtelAttribute | undefined {
  if (typeof value === "string") return { key, value: { stringValue: value } };
  if (typeof value === "boolean") return { key, value: { boolValue: value } };
  if (typeof value === "number" && Number.isFinite(value)) return { key, value: { doubleValue: value } };
  return undefined;
}
function attrs(values: Record<string, unknown>): OtelAttribute[] {
  return Object.entries(values).flatMap(([key, value]) => { const attribute = toAttribute(key, value); return attribute ? [attribute] : []; });
}
function unixNano(timeMs: number): string { return `${Math.trunc(timeMs)}000000`; }
function randomHex(bytes: number): string {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi?.getRandomValues) return Array.from(cryptoApi.getRandomValues(new Uint8Array(bytes)), (value) => value.toString(16).padStart(2, "0")).join("");
  return Array.from({ length: bytes }, () => Math.floor(Math.random() * 256).toString(16).padStart(2, "0")).join("");
}
function exporterConfig(): OtelConfig { return configured || readOtelConfig(); }
async function exportSpan(span: Record<string, unknown>): Promise<void> {
  const config = exporterConfig();
  if (config.tracesExporter === "none") return;
  if (config.tracesExporter === "console") { console.info("[otel]", JSON.stringify(span)); return; }
  const payload = { resourceSpans: [{ resource: { attributes: attrs({ "service.name": config.serviceName }) }, scopeSpans: [{ scope: { name: "u-king-mini.agentloop", version: "1.1.0" }, spans: [span] }] }] };
  if (config.tracesExporter === "jsonl") {
    if (!isNodeRuntime()) { console.warn("[otel] jsonl exporter is unavailable in WebView; use OTLP collector instead", span); return; }
    try {
      const [fs, os, path] = await Promise.all([dynamicImport("node:fs/promises"), dynamicImport("node:os"), dynamicImport("node:path")]);
      const home = (os.homedir as () => string)();
      const join = path.join as (...parts: string[]) => string;
      const folder = join(runtimeEnv("APPDATA") || join(home, "AppData", "Roaming"), "u-king-mini", "otel-traces");
      await (fs.mkdir as (target: string, options: { recursive: boolean }) => Promise<void>)(folder, { recursive: true });
      await (fs.appendFile as (target: string, data: string, encoding: string) => Promise<void>)(join(folder, "traces.otlp.jsonl"), `${JSON.stringify(payload)}\n`, "utf8");
    } catch (error) { console.warn("[otel] jsonl export failed; continuing without telemetry", error); }
    return;
  }
  try {
    const response = await fetch(config.otlpEndpoint, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(payload) });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
  } catch (error) {
    // collector 是可选基础设施：故障只能降级观测，不能影响聊天请求。
    console.warn("[otel] OTLP export failed; continuing without telemetry", error);
  }
}

/** 最小 GenAI span 封装；导出的 payload 是标准 OTLP/JSON，而非 SDK 的 toJSON 私有形态。 */
export async function wrapGenAICall<T>(options: GenAiCallOptions, call: (hooks: GenAiCallHooks) => Promise<T>): Promise<T> {
  await initOtel();
  const startedAt = Date.now();
  const traceId = randomHex(16);
  const spanId = randomHex(8);
  const spanAttrs: Record<string, unknown> = { "gen_ai.operation.name": options.operation || "chat", "gen_ai.request.model": options.model, ...(options.input ? summaryAttrs("gen_ai.input", options.input) : {}), ...(options.inputSummary ? summaryAttrs("gen_ai.input", options.inputSummary) : {}), ...options.attributes };
  const events: OtelEvent[] = [];
  let sawFirstToken = false;
  let status: { code: string; message?: string } = { code: "STATUS_CODE_UNSET" };
  const hooks: GenAiCallHooks = {
    firstToken() { if (!sawFirstToken) { sawFirstToken = true; spanAttrs["gen_ai.ttft_ms"] = Date.now() - startedAt; events.push({ timeUnixNano: unixNano(Date.now()), name: "gen_ai.first_token" }); } },
    tool(name, input, output) { events.push({ timeUnixNano: unixNano(Date.now()), name: "gen_ai.tool", attributes: attrs({ "gen_ai.tool.name": name, ...summaryAttrs("gen_ai.tool.input", input), ...summaryAttrs("gen_ai.tool.output", output) }) }); },
    response(response) {
      if (response.id) spanAttrs["gen_ai.response.id"] = response.id;
      if (response.promptTokens != null) spanAttrs["gen_ai.usage.prompt_tokens"] = response.promptTokens;
      if (response.completionTokens != null) spanAttrs["gen_ai.usage.completion_tokens"] = response.completionTokens;
      if (response.totalTokens != null) spanAttrs["gen_ai.usage.total_tokens"] = response.totalTokens;
      if (response.output != null) Object.assign(spanAttrs, summaryAttrs("gen_ai.output", response.output));
    },
    attribute(name, value) { spanAttrs[name] = value; },
  };
  try {
    const result = await call(hooks);
    if (!sawFirstToken) spanAttrs["gen_ai.total_duration_ms"] = Date.now() - startedAt;
    status = { code: "STATUS_CODE_OK" };
    return result;
  } catch (error) {
    status = { code: "STATUS_CODE_ERROR", message: String(error) };
    events.push({ timeUnixNano: unixNano(Date.now()), name: "exception", attributes: attrs({ "exception.message": String(error) }) });
    throw error;
  } finally {
    void exportSpan({ traceId, spanId, name: "gen_ai.chat", kind: "SPAN_KIND_INTERNAL", startTimeUnixNano: unixNano(startedAt), endTimeUnixNano: unixNano(Date.now()), attributes: attrs(spanAttrs), events, status });
  }
}
