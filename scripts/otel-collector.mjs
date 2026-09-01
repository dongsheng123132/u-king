#!/usr/bin/env node
/** Lightweight OTLP/HTTP JSON collector for local AgentLoop evidence. */
import http from "node:http";
import { appendFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const port = Number(process.env.PORT || 4318);
const output = resolve("otel-traces/traces.otlp.jsonl");
let totalSpans = 0;
let pending = Promise.resolve();

function readBody(req) {
  return new Promise((resolveBody, reject) => {
    let size = 0;
    const chunks = [];
    req.on("data", (chunk) => {
      size += chunk.length;
      if (size > 10 * 1024 * 1024) { reject(new Error("OTLP body exceeds 10 MiB")); req.destroy(); return; }
      chunks.push(chunk);
    });
    req.on("end", () => resolveBody(Buffer.concat(chunks).toString("utf8")));
    req.on("error", reject);
  });
}

function serviceName(resource = {}) {
  const attr = resource.attributes?.find?.((item) => item.key === "service.name");
  return attr?.value?.stringValue || "unknown-service";
}

function normalize(payload) {
  const lines = [];
  for (const resourceSpans of payload.resourceSpans || []) {
    const resource = resourceSpans.resource || {};
    for (const scopeSpans of resourceSpans.scopeSpans || []) {
      for (const span of scopeSpans.spans || []) {
        lines.push({
          traceId: span.traceId,
          spanId: span.spanId,
          parentSpanId: span.parentSpanId || "",
          name: span.name,
          kind: span.kind,
          startTimeUnixNano: span.startTimeUnixNano,
          endTimeUnixNano: span.endTimeUnixNano,
          attributes: span.attributes || [],
          events: span.events || [],
          status: span.status || { code: "STATUS_CODE_UNSET" },
          resource,
          scope: scopeSpans.scope || {},
        });
      }
    }
  }
  return lines;
}

const server = http.createServer(async (req, res) => {
  // Tauri WebView 的 tauri:// 来源调用本地 collector 时也会走浏览器 CORS 校验。
  res.setHeader("access-control-allow-origin", "*");
  res.setHeader("access-control-allow-methods", "POST, GET, OPTIONS");
  res.setHeader("access-control-allow-headers", "content-type");
  if (req.method === "OPTIONS") { res.writeHead(204).end(); return; }
  if (req.method === "GET" && req.url === "/health") { res.writeHead(200, { "content-type": "text/plain" }).end("ok\n"); return; }
  if (req.method !== "POST" || req.url !== "/v1/traces") { res.writeHead(404).end(); return; }
  try {
    const payload = JSON.parse(await readBody(req));
    const lines = normalize(payload);
    pending = pending.then(async () => {
      if (!lines.length) return;
      await mkdir(dirname(output), { recursive: true });
      await appendFile(output, `${lines.map((line) => JSON.stringify(line)).join("\n")}\n`, "utf8");
    });
    await pending;
    totalSpans += lines.length;
    const first = lines[0];
    console.log(`[otel-collector] service=${serviceName(first?.resource)} spans=${lines.length} window=${first?.startTimeUnixNano || "-"}..${lines.at(-1)?.endTimeUnixNano || "-"}`);
    res.writeHead(200, { "content-type": "application/json" }).end("{}");
  } catch (error) {
    console.warn("[otel-collector] rejected trace payload:", error instanceof Error ? error.message : error);
    res.writeHead(400, { "content-type": "application/json" }).end(JSON.stringify({ error: "invalid OTLP/JSON payload" }));
  }
});

server.listen(port, "127.0.0.1", () => console.log(`[otel-collector] listening http://127.0.0.1:${port}/v1/traces -> ${output}`));
function stop() {
  server.close(async () => { await pending; console.log(`[otel-collector] flushed spans=${totalSpans}`); process.exit(0); });
}
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
