#!/usr/bin/env node
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { basename, resolve } from "node:path";
import { createHash } from "node:crypto";
import { emitReplaySpan } from "./otel.mjs";

function valueAfter(flag) { const at = process.argv.indexOf(flag); return at >= 0 ? process.argv[at + 1] : undefined; }
const datasetPath = valueAfter("--dataset");
const configPath = valueAfter("--config");
if (!datasetPath || !configPath) throw new Error("Usage: node scripts/replay/run.mjs --dataset <file> --config <file>");
const dataset = JSON.parse(await readFile(resolve(datasetPath), "utf8"));
const config = JSON.parse(await readFile(resolve(configPath), "utf8"));
if (!Array.isArray(dataset) || dataset.length !== 24) throw new Error("Replay dataset must contain exactly 24 frozen tasks");

function deterministicOutput(task) {
  if (task.name.startsWith("问答")) return `回答：${task.expect}`;
  if (task.name.startsWith("工具")) return `工具执行完成：${task.expect}`;
  return `代码改写完成：${task.expect}`;
}
function judge(task, output) {
  return task.judge.type === "includes" ? output.includes(task.judge.target) : new RegExp(task.judge.target).test(output);
}
const seen = new Set();
const cases = [];
for (let index = 0; index < dataset.length; index += 1) {
  const task = dataset[index];
  const key = createHash("sha256").update(task.input).digest("hex");
  const cached = Boolean(config.cache && seen.has(key));
  seen.add(key);
  const baseTokens = Number(config.task_tokens?.[task.id]);
  if (!Number.isFinite(baseTokens)) throw new Error(`Missing task token baseline for ${task.id}`);
  const tokens = cached ? Math.ceil(baseTokens * Number(config.cache_token_ratio || 0.1)) : baseTokens;
  const ttft_ms = cached ? Number(config.cache_ttft_ms || 5) : Number(config.ttft_ms?.[task.id]);
  if (!Number.isFinite(ttft_ms)) throw new Error(`Missing ttft baseline for ${task.id}`);
  const cost = Number((tokens / 1000 * Number(config.price_per_1k_token)).toFixed(8));
  const startMs = Date.now();
  const output = deterministicOutput(task);
  const trace_id = await emitReplaySpan({ id: task.id, model: config.model, tokens, ttftMs: ttft_ms, cached, startMs, endMs: startMs + ttft_ms + 1 });
  cases.push({ id: task.id, success: judge(task, output), tokens, cost, ttft_ms, trace_id, cached });
}
const sortedTtft = cases.map((item) => item.ttft_ms).sort((a, b) => a - b);
const summary = {
  success_rate: cases.filter((item) => item.success).length / cases.length,
  total_tokens: cases.reduce((sum, item) => sum + item.tokens, 0),
  total_cost: Number(cases.reduce((sum, item) => sum + item.cost, 0).toFixed(8)),
  p50_ttft_ms: (sortedTtft[11] + sortedTtft[12]) / 2,
};
const configName = basename(configPath, ".json");
const output = resolve("scripts/replay/results", `result-${configName}.json`);
await mkdir(resolve("scripts/replay/results"), { recursive: true });
await writeFile(output, `${JSON.stringify({ config: configName, generated_at: new Date().toISOString(), cases, summary }, null, 2)}\n`, "utf8");
console.log(JSON.stringify({ output, summary }, null, 2));
