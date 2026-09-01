#!/usr/bin/env node
/**
 * fetch-page.mjs —— 纯 std（零 npm 依赖）抓一个网页并转成 Markdown 正文。
 *
 * 解决的是：「只会文字的模型看不见网页」。整页 HTML 丢进上下文既贵又常常放不下
 * （一个普通新闻页 200KB HTML ≈ 5 万 token，正文其实只有 2 千）——这里先在本地
 * 剥掉 script/style/nav/footer，再转 Markdown，通常只剩 3~8%。
 *
 * 用法：
 *   node fetch-page.mjs https://example.com --json
 *   node fetch-page.mjs https://example.com --max-chars 8000
 *   node fetch-page.mjs https://example.com --links        # 额外列出页面链接
 *   node fetch-page.mjs https://example.com --raw          # 不转换，出原始 HTML（调试用）
 *
 * 输出：`{"ok":true,"url":"…","title":"…","text":"…","chars":N,"truncated":false}`
 */
import { argv, exit } from "node:process";

function parseArgs(a) {
  const o = { _: [] };
  for (let i = 0; i < a.length; i++) {
    const t = a[i];
    if (t.startsWith("--")) { const k = t.slice(2); o[k] = a[i + 1] && !a[i + 1].startsWith("--") ? a[++i] : true; }
    else o._.push(t);
  }
  return o;
}
const args = parseArgs(argv.slice(2));
const asJson = !!args.json;
function fail(m) { if (asJson) console.log(JSON.stringify({ ok: false, error: String(m) })); else console.error("[fetch-page] 失败:", m); exit(1); }

const url = args._[0] || args.url;
if (!url) fail("用法: node fetch-page.mjs <网址> [--json] [--max-chars N] [--links]");
if (!/^https?:\/\//i.test(String(url))) fail("网址要带 http:// 或 https://");

const maxChars = Number(args["max-chars"] || 12000);
const timeoutMs = Number(args.timeout || 30000);

const ctl = new AbortController();
const timer = setTimeout(() => ctl.abort(), timeoutMs);
let res, html;
try {
  res = await fetch(String(url), {
    signal: ctl.signal,
    redirect: "follow",
    headers: {
      // 不伪装成别的产品，但也别用默认 UA —— 很多站直接 403 掉没有 UA 的请求
      "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) U-King/1.0 (+https://u-king.org)",
      "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
      "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
    },
  });
  html = await res.text();
} catch (e) {
  clearTimeout(timer);
  fail(String(e && e.name === "AbortError" ? `超时 ${timeoutMs}ms 没拿到页面` : e.message || e));
}
clearTimeout(timer);
if (!res.ok) fail(`HTTP ${res.status} ${res.statusText}`);

if (args.raw) { console.log(html); exit(0); }

// ---------- 标题 ----------
const titleM = html.match(/<title[^>]*>([\s\S]*?)<\/title>/i);
const decode = (s) => String(s)
  .replace(/&nbsp;/gi, " ").replace(/&amp;/gi, "&").replace(/&lt;/gi, "<").replace(/&gt;/gi, ">")
  .replace(/&quot;/gi, '"').replace(/&#39;|&apos;/gi, "'").replace(/&ldquo;|&rdquo;/gi, '"')
  .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(+d))
  .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)));
const title = titleM ? decode(titleM[1]).trim().replace(/\s+/g, " ") : "";

// ---------- 剥壳：先删整块噪音，再转 Markdown ----------
let s = html;
for (const tag of ["script", "style", "noscript", "svg", "iframe", "nav", "footer", "form", "aside"]) {
  s = s.replace(new RegExp(`<${tag}[\\s\\S]*?<\\/${tag}>`, "gi"), " ");
}
s = s.replace(/<!--[\s\S]*?-->/g, " ");
// 正文优先：有 <article> / <main> 就只留它，命中率高且省得多
const art = s.match(/<article[^>]*>([\s\S]*?)<\/article>/i) || s.match(/<main[^>]*>([\s\S]*?)<\/main>/i);
if (art && art[1].length > 500) s = art[1];

// 链接（在剥标签之前抽）
let links = [];
if (args.links) {
  const seen = new Set();
  for (const m of s.matchAll(/<a[^>]+href=["']([^"']+)["'][^>]*>([\s\S]*?)<\/a>/gi)) {
    const text = decode(m[2].replace(/<[^>]+>/g, "")).trim().replace(/\s+/g, " ");
    if (!text || seen.has(m[1])) continue;
    seen.add(m[1]);
    try { links.push({ text: text.slice(0, 80), href: new URL(m[1], String(url)).href }); } catch {}
    if (links.length >= 100) break;
  }
}

s = s
  .replace(/<h1[^>]*>([\s\S]*?)<\/h1>/gi, (_, t) => `\n\n# ${t}\n\n`)
  .replace(/<h2[^>]*>([\s\S]*?)<\/h2>/gi, (_, t) => `\n\n## ${t}\n\n`)
  .replace(/<h3[^>]*>([\s\S]*?)<\/h3>/gi, (_, t) => `\n\n### ${t}\n\n`)
  .replace(/<h[456][^>]*>([\s\S]*?)<\/h[456]>/gi, (_, t) => `\n\n#### ${t}\n\n`)
  .replace(/<li[^>]*>/gi, "\n- ").replace(/<\/li>/gi, "")
  .replace(/<br\s*\/?>/gi, "\n")
  .replace(/<\/(p|div|tr|section|h[1-6])>/gi, "\n\n")
  .replace(/<\/t[dh]>/gi, " | ")
  .replace(/<[^>]+>/g, "");
s = decode(s)
  .replace(/[ \t ]+/g, " ")
  .split("\n").map((l) => l.trim()).join("\n")
  .replace(/\n{3,}/g, "\n\n")
  .trim();

const truncated = s.length > maxChars;
if (truncated) s = s.slice(0, maxChars) + `\n\n…（正文超过 ${maxChars} 字已截断，要看全文加 --max-chars）`;

if (asJson) console.log(JSON.stringify({ ok: true, url: res.url, status: res.status, title, text: s, chars: s.length, truncated, links: args.links ? links : undefined }));
else {
  if (title) console.log(`# ${title}\n`);
  console.log(s);
  if (args.links && links.length) { console.log("\n---\n链接："); for (const l of links) console.log(`- [${l.text}](${l.href})`); }
}
