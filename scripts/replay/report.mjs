#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const dateParts = Object.fromEntries(new Intl.DateTimeFormat("en", { timeZone: "Asia/Shanghai", year: "numeric", month: "2-digit", day: "2-digit" })
  .formatToParts(new Date()).filter((part) => part.type !== "literal").map((part) => [part.type, part.value]));
const today = `${dateParts.year}${dateParts.month}${dateParts.day}`;
const [baseline, cached] = await Promise.all(["baseline", "cached"].map(async (name) => JSON.parse(await readFile(resolve(`scripts/replay/results/result-${name}.json`), "utf8"))));
const percent = (value) => `${(value * 100).toFixed(1)}%`;
const change = (before, after) => before === 0 ? "—" : `${((after - before) / before * 100).toFixed(1)}%`;
const rows = baseline.cases.map((item, index) => `| ${item.id} | ${item.success ? "通过" : "失败"} | ${item.tokens} | ${cached.cases[index].tokens} | ${item.ttft_ms} | ${cached.cases[index].ttft_ms} | ${cached.cases[index].cached ? "命中" : "未命中"} |`).join("\n");
const text = `# AgentLoop 回测飞轮 A/B 报告（${today}）

本报告是一次可复跑的闭环实证样例：冻结任务集 → 标准 GenAI OTLP span → 机器判据评估 → 响应缓存优化 → A/B 回测。它不是统计显著性结论。

| 指标 | baseline | cached | 变化 |
| --- | ---: | ---: | ---: |
| success_rate | ${percent(baseline.summary.success_rate)} | ${percent(cached.summary.success_rate)} | ${change(baseline.summary.success_rate, cached.summary.success_rate)} |
| total_tokens | ${baseline.summary.total_tokens} | ${cached.summary.total_tokens} | ${change(baseline.summary.total_tokens, cached.summary.total_tokens)} |
| total_cost | $${baseline.summary.total_cost.toFixed(6)} | $${cached.summary.total_cost.toFixed(6)} | ${change(baseline.summary.total_cost, cached.summary.total_cost)} |
| p50_ttft_ms | ${baseline.summary.p50_ttft_ms} | ${cached.summary.p50_ttft_ms} | ${change(baseline.summary.p50_ttft_ms, cached.summary.p50_ttft_ms)} |

缓存配置对相同输入的后续请求只记 10% token、TTFT 固定为 5ms。本次结果显示：成功率保持 ${percent(cached.summary.success_rate)}，总 token ${change(baseline.summary.total_tokens, cached.summary.total_tokens)}，总成本 ${change(baseline.summary.total_cost, cached.summary.total_cost)}，P50 TTFT ${change(baseline.summary.p50_ttft_ms, cached.summary.p50_ttft_ms)}。这证明该模拟优化在这套冻结任务上转过一圈；上线前仍需用真实流量、缓存失效与质量回归继续验证。

## 逐例（极简）

| id | 判据 | baseline tokens | cached tokens | baseline TTFT | cached TTFT | 缓存 |
| --- | --- | ---: | ---: | ---: | ---: | --- |
${rows}
`;
const output = resolve(`docs/replay-report-${today}.md`);
await writeFile(output, text, "utf8");
console.log(output);
