// 一搜商答 / 1so —— 自动探测层（"我们能查的就自动查"：抓真实页面 → 交 LLM 判断）。
// 只有服务端渲染、不强反爬的渠道能脚本抓到 HTML。实测能抓：Bing / 搜狗（通用网页搜索，覆盖国内外+微信搜一搜）。
// 抓到后抽成纯文本交 LLM 判断"公司是否作为真实主体出现"——比数关键词/找无结果标记可靠得多（搜索页会回显查询词，纯计数必假阳性）。
// 其余（百度/知乎/小红书/抖音/微博/地图…）是 JS 壳或反爬 → manual，交客户在面板自查。抓不到≠不存在。
import { spawnSync } from "node:child_process";
import { parseJsonLoose } from "./util.mjs";

export const AUTO_IDS = new Set(["bing", "sogou"]);
const UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36";

function curlGet(url, { proxy, timeoutSec = 14 }) {
  const a = ["-s", "-L", "-A", UA, "--max-time", String(timeoutSec)];
  if (proxy) a.push("--proxy", proxy);
  a.push(url);
  const r = spawnSync("curl", a, { encoding: "utf8", timeout: (timeoutSec + 4) * 1000, maxBuffer: 16 * 1024 * 1024 });
  if (r.error || r.status !== 0) return null;
  return r.stdout || "";
}

// HTML → 可读纯文本（去脚本/样式/标签，压空白，截断）。
function htmlToText(html, max = 6000) {
  if (!html) return "";
  let t = html
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/g, " ").replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"')
    .replace(/\s+/g, " ")
    .trim();
  return t.slice(0, max);
}

const JUDGE_SYS = `你是搜索结果判读员。下面是某搜索引擎对"公司名"的结果页纯文本（含查询回显、导航等噪声）。
判断：这家公司是否作为一个真实、具体的主体出现在结果里（有它的官网/主页/词条/相关报道/店铺等），而不是仅仅因为查询词被回显、或只是零散的无关同名片段。
诚实：不确定或只有噪声就判 found=false。`;

// 对能抓的渠道逐个：抓 HTML → 抽文本 → LLM 判读。返回 map: id -> {status:'hit'|'miss'|'manual', evidence}
export async function autoProbe(items, { name, region = "", proxy, llmOpts = {} } = {}) {
  const result = {};
  for (const it of items) {
    if (!AUTO_IDS.has(it.id)) continue;
    const html = curlGet(it.url, { proxy });
    const text = htmlToText(html);
    if (!text || text.length < 200) { result[it.id] = { status: "manual", hits: 0, evidence: "网络抓取失败/被拦" }; continue; }
    try {
      // llm.mjs 动态加载：客户端只发离线自查那条链，不发它（见 cli.mjs 顶部那段）。
      // autoProbe 只有 `scan --auto` 才会走到这里，而客户端的 GUI 不带 --auto。
      const { chatJson } = await import("./llm.mjs");
      const raw = await chatJson(
        JUDGE_SYS,
        `公司名：${name}${region ? "（地区：" + region + "）" : ""}\n渠道：${it.name}\n结果页文本：\n${text}\n\n输出 JSON：{"found":true/false,"evidence":"命中的最有力一条，或未命中的原因（≤40字）"}`,
        { ...llmOpts, maxTokens: 300, timeoutSec: 90 }
      );
      const j = parseJsonLoose(raw);
      result[it.id] = { status: j.found ? "hit" : "miss", evidence: String(j.evidence || "").slice(0, 60) };
    } catch (e) {
      result[it.id] = { status: "manual", evidence: "判读失败：" + (e.message || e) };
    }
  }
  return result;
}
