// 1so inspect —— 网页「AI 友好度」诊断（page-level GEO audit）。
// 抓一个网址的 HTML（+ /robots.txt + /llms.txt 两个兄弟文件），用一套 100 分制、机器可离线自查的规则打分：
// 「AI 能不能抓到你 / 好不好解析你 / 愿不愿引用你 / 认不认识你这个品牌」，逐维给分 + 具体怎么改，
// 并**顺手生成**给客户网站的 llms.txt 与 结构化数据（JSON-LD）两个可直接用的修复文件。
//
// 规则来源（都用我们自己的话重写，不复制原文）：
//   · Auriti-Labs/geo-optimizer-skill (MIT) 的 8 类 100 分骨架；
//   · Princeton GEO (KDD'24) 实证：加数字 +33% / 引用来源 +27% / 加引述 +41% / 流畅度 +29%，堆关键词无效（不奖励）；
//   · GEORank (Apache-2.0) 的 schema/meta/content/citation 维度；CN-GEO 研究的长度/对题/结构结论。
// **差异化**：硬编码国产 AI 爬虫 UA（字节 Bytespider / 百度 Baiduspider / 华为 PetalBot / 神马 YisouSpider / 搜狗 / 360）
//   —— 上面那些海外开源全没覆盖国产引擎，这是我们最值钱的一块。
//
// 纯启发式、不调 LLM、不烧 token：只联网抓页面本身（免费出分，和 scan 一样当引流钩子）。
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { projectPaths } from "../config.mjs";
import { writeText, logE, warn, done, fail, today, esc } from "../util.mjs";
import { renderPayBlock, payCss } from "../pay.mjs";

const UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36";

// ── HTTP（走系统 curl，带 http_code）──────────────────────────────
function httpGet(url, { proxy, timeoutSec = 20 } = {}) {
  const MARK = "__1SO_HTTP__";
  const a = ["-s", "-L", "-A", UA, "--max-time", String(timeoutSec), "-w", `\n${MARK}%{http_code}`];
  if (proxy) a.push("--proxy", proxy);
  a.push(url);
  const r = spawnSync("curl", a, { encoding: "utf8", timeout: (timeoutSec + 5) * 1000, maxBuffer: 24 * 1024 * 1024 });
  if (r.error) return { ok: false, status: 0, body: "", error: String(r.error.message || r.error) };
  const out = r.stdout || "";
  const i = out.lastIndexOf(MARK);
  const status = i >= 0 ? parseInt(out.slice(i + MARK.length).trim(), 10) || 0 : 0;
  const body = i >= 0 ? out.slice(0, i) : out;
  return { ok: r.status === 0, status, body };
}

// ── HTML 解析小工具（正则，容脏页；全 try/catch 兜底，release 是 panic=abort，热路径别崩）──
// 实体解码：数字实体（&#x27; / &#39;）+ 常见命名实体。数字型先解，避免 &amp;#39; 这类漏网。
function decodeEntities(s) {
  return String(s || "")
    .replace(/&#x([0-9a-f]+);/gi, (m, h) => { try { return String.fromCodePoint(parseInt(h, 16)); } catch { return m; } })
    .replace(/&#(\d+);/g, (m, d) => { try { return String.fromCodePoint(parseInt(d, 10)); } catch { return m; } })
    .replace(/&nbsp;/gi, " ").replace(/&quot;/gi, '"').replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, "<").replace(/&gt;/gi, ">").replace(/&amp;/gi, "&");
}
function textOf(html, max = 200000) {
  const stripped = String(html || "")
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<[^>]+>/g, " ");
  return decodeEntities(stripped).replace(/\s+/g, " ").trim().slice(0, max);
}
// 正文「字数」：中文按字、英文按词，混合站都算得过去。
function contentLen(text) {
  const cjk = (text.match(/[一-鿿]/g) || []).length;
  const latin = (text.match(/[a-zA-Z]{2,}/g) || []).length;
  return cjk + latin;
}
function firstTag(html, tag) {
  const m = new RegExp(`<${tag}[^>]*>([\\s\\S]*?)<\\/${tag}>`, "i").exec(html || "");
  return m ? textOf(m[1]) : "";
}
function countTag(html, tag) {
  return (String(html || "").match(new RegExp(`<${tag}[\\s>]`, "gi")) || []).length;
}
// 抽所有 <meta>，归一成 {name/property(lower) -> content}
function metaMap(html) {
  const out = {};
  const re = /<meta\s+([^>]*?)\/?>/gi; let m;
  while ((m = re.exec(html || ""))) {
    const attrs = m[1];
    const key = (/(?:name|property|itemprop)\s*=\s*["']([^"']+)["']/i.exec(attrs) || [])[1];
    const val = (/content\s*=\s*["']([^"']*)["']/i.exec(attrs) || [])[1];
    if (key) out[key.toLowerCase()] = val || "";
  }
  return out;
}
function headings(html) {
  const out = [];
  const re = /<(h[1-6])[^>]*>([\s\S]*?)<\/\1>/gi; let m;
  while ((m = re.exec(html || ""))) out.push({ level: parseInt(m[1][1], 10), text: textOf(m[2]) });
  return out;
}
function jsonLdBlocks(html) {
  const blocks = [];
  const re = /<script[^>]+type\s*=\s*["']application\/ld\+json["'][^>]*>([\s\S]*?)<\/script>/gi; let m;
  while ((m = re.exec(html || ""))) {
    try { blocks.push(JSON.parse(m[1].trim())); } catch { blocks.push(null); }
  }
  return blocks;
}
// 从 JSON-LD（含 @graph / 数组）里收集所有 @type + 最大属性数
function ldTypes(blocks) {
  const types = new Set(); let maxProps = 0;
  const walk = (node) => {
    if (!node || typeof node !== "object") return;
    if (Array.isArray(node)) return node.forEach(walk);
    if (node["@type"]) [].concat(node["@type"]).forEach((t) => types.add(String(t)));
    const props = Object.keys(node).filter((k) => k !== "@context" && k !== "@type").length;
    if (props > maxProps) maxProps = props;
    if (node["@graph"]) walk(node["@graph"]);
    for (const v of Object.values(node)) if (v && typeof v === "object") walk(v);
  };
  blocks.filter(Boolean).forEach(walk);
  return { types, maxProps };
}
function links(html, origin) {
  const inturl = [], ext = []; const re = /<a\s+[^>]*href\s*=\s*["']([^"'#]+)["']/gi; let m;
  while ((m = re.exec(html || ""))) {
    const href = m[1].trim();
    if (/^(mailto:|tel:|javascript:)/i.test(href)) continue;
    if (/^https?:\/\//i.test(href)) {
      try { (new URL(href).origin === origin ? inturl : ext).push(href); } catch {}
    } else if (href.startsWith("/") || !href.includes(":")) inturl.push(href);
  }
  return { internal: inturl, external: ext };
}

// 权威外链域名（海外 + 国内官媒/学术）——引用它们=可信信号（KDD：引用来源 +27%）。
const AUTHORITY_RE = /(\.gov(\.cn)?|\.edu(\.cn)?|\.org|wikipedia\.org|wikidata\.org|cnki\.net|xinhuanet\.com|people\.com\.cn|gov\.cn|who\.int|arxiv\.org)(\/|$|["'])/i;
// 品牌实体图链接（sameAs 该指向的地方）——建立「你是谁」的实体身份。
const ENTITY_RE = /(wikipedia\.org|wikidata\.org|linkedin\.com|crunchbase\.com|weibo\.com|zhihu\.com|baike\.baidu\.com|qcc\.com|tianyancha\.com)/i;

// 国产 + 海外 AI 爬虫 UA（robots.txt 该放行的对象）。国产是本工具相对海外开源的**差异化**。
const AI_BOTS = [
  { ua: "GPTBot", who: "ChatGPT 训练" }, { ua: "OAI-SearchBot", who: "ChatGPT 搜索" }, { ua: "ChatGPT-User", who: "ChatGPT 浏览" },
  { ua: "ClaudeBot", who: "Claude" }, { ua: "PerplexityBot", who: "Perplexity" }, { ua: "Google-Extended", who: "Gemini/AI 概览" },
  { ua: "CCBot", who: "Common Crawl(喂多数大模型)" }, { ua: "Applebot-Extended", who: "Apple AI" },
  { ua: "Bytespider", who: "字节·豆包" }, { ua: "Baiduspider", who: "百度·文心" }, { ua: "PetalBot", who: "华为·小艺" },
  { ua: "YisouSpider", who: "神马·UC" }, { ua: "Sogou web spider", who: "搜狗" }, { ua: "360Spider", who: "360" },
];

// robots.txt → 每个 bot 是否被 Disallow: /（自己的组优先，无组则看 * 组）
function robotsBlocks(robotsTxt) {
  const groups = {}; let cur = [];
  for (const raw of String(robotsTxt || "").split(/\r?\n/)) {
    const line = raw.replace(/#.*$/, "").trim(); if (!line) continue;
    const mm = /^user-agent\s*:\s*(.+)$/i.exec(line);
    if (mm) { const ua = mm[1].trim().toLowerCase(); cur = groups[ua] || (groups[ua] = []); continue; }
    const dm = /^disallow\s*:\s*(.*)$/i.exec(line);
    if (dm && cur) cur.push(dm[1].trim());
  }
  const star = groups["*"] || [];
  const blockedFor = (ua) => {
    const own = groups[ua.toLowerCase()];
    const rules = own && own.length ? own : star;
    return rules.some((d) => d === "/");
  };
  return { blockedFor, hasStar: !!groups["*"] };
}

// 分档着色：pct→level 供报告条形色用
function band(pct) { return pct >= 80 ? "ok" : pct >= 45 ? "warn" : "bad"; }
function clamp(n) { return Math.max(0, Math.min(100, Math.round(n))); }

// ── 主命令 ──────────────────────────────────────────────────────
export async function cmdInspect(args, _llmOpts) {
  const jsonMode = !!args.json;
  let url = (args.url && args.url !== true) ? String(args.url) : (typeof args._[1] === "string" ? args._[1] : "");
  url = url.trim();
  if (!url) return fail(jsonMode, "缺少网址。用 1so inspect --url https://你的网站", 2);
  if (!/^https?:\/\//i.test(url)) url = "https://" + url;
  const name = (args.name && args.name !== true) ? String(args.name) : "";
  const keyword = (args.keyword && args.keyword !== true) ? String(args.keyword) : name;
  const proxy = (args.proxy && args.proxy !== true) ? String(args.proxy) : undefined;
  const P = projectPaths(args.project || ".");

  warn(`抓取并诊断：${url}`);
  const page = httpGet(url, { proxy });
  if (!page.body || page.body.length < 80) {
    return fail(jsonMode, `抓不到页面内容（${page.status || "网络失败"}）。检查网址是否可公开访问、是否需要代理（--proxy）。`, 1);
  }
  const html = page.body;
  let origin = "", host = "";
  try { const u = new URL(url); origin = u.origin; host = u.hostname; } catch {}

  // 兄弟文件：/robots.txt、/llms.txt
  const robots = origin ? httpGet(origin + "/robots.txt", { proxy, timeoutSec: 12 }) : { status: 0, body: "" };
  const llms = origin ? httpGet(origin + "/llms.txt", { proxy, timeoutSec: 12 }) : { status: 0, body: "" };

  // ── 解析 ──
  const meta = metaMap(html);
  const title = firstTag(html, "title") || decodeEntities(meta["og:title"] || "");
  const desc = decodeEntities(meta["description"] || meta["og:description"] || "");
  const lang = (/<html[^>]*\slang\s*=\s*["']?([a-zA-Z-]+)/i.exec(html) || [])[1] || "";
  const hasCanonical = /<link[^>]+rel\s*=\s*["']?canonical/i.test(html);
  const hasViewport = !!meta["viewport"];
  const hasFavicon = /<link[^>]+rel\s*=\s*["'][^"']*icon/i.test(html);
  const hs = headings(html);
  const h1 = hs.filter((h) => h.level === 1);
  const h23 = hs.filter((h) => h.level === 2 || h.level === 3);
  const qHead = hs.filter((h) => /[?？]\s*$|^(如何|怎么|怎样|什么|为什么|哪些|是否|多少)/.test(h.text)).length;
  const { types: ldT, maxProps: ldProps } = ldTypes(jsonLdBlocks(html));
  const lk = links(html, origin);
  const authLinks = lk.external.filter((h) => AUTHORITY_RE.test(h));
  const entityLinks = [...lk.external, ...Object.values(meta)].filter((h) => ENTITY_RE.test(String(h)));
  const text = textOf(html);
  const words = contentLen(text);
  const statHits = (text.match(/\d+(?:\.\d+)?\s?%|[¥$]\s?\d|\d{4}\s?年|\d+(?:\.\d+)?\s?(?:万|亿|倍|个|名|家|款|次|元)|\bNo\.?\s?\d|第\s?\d+/gi) || []).length;
  const defHits = (text.match(/是指|是一(?:种|个|家|款)|指的是|定义为|称为|\bis a\b|\bmeans\b|\brefers to\b/gi) || []).length;
  const listCnt = countTag(html, "ul") + countTag(html, "ol");
  const tableCnt = countTag(html, "table");
  const quoteCnt = countTag(html, "blockquote") + (text.match(/[“"][^”"]{8,}[”"]/g) || []).length;
  const stepHits = (text.match(/第[一二三四五六七八九十\d]+步|步骤\s?[一二三\d]|首先[，、]|Step\s?\d|^\s*\d+[.)、]/gim) || []).length;
  const imgs = (html.match(/<img\b[^>]*>/gi) || []);
  const imgsAlt = imgs.filter((t) => /\balt\s*=\s*["'][^"']+["']/i.test(t)).length;
  const dateMod = meta["article:modified_time"] || (/"dateModified"\s*:\s*"([^"]+)"/i.exec(html) || [])[1] || "";

  // ── 逐维打分（每维 pct 0-100；overall = Σ weight×pct/100）──
  const D = [];
  const add = (key, label, group, weight, pct, detail, fixes = []) =>
    D.push({ key, label, group, weight, pct: clamp(pct), detail, fixes });

  // A 能不能抓到你
  {
    const noRobots = !(robots.body && /user-agent/i.test(robots.body));
    if (noRobots) {
      add("robots", "AI 爬虫放行（robots.txt）", "A 能不能抓到你", 12, 82,
        "没有 robots.txt = 默认全放行（AI 爬虫能进）。建议显式写一份，明确欢迎国产 + 海外 AI 爬虫。",
        ["加一份 robots.txt，对 GPTBot / Bytespider（豆包）/ Baiduspider（文心）/ PerplexityBot 等写 Allow: /"]);
    } else {
      const rb = robotsBlocks(robots.body);
      const blocked = AI_BOTS.filter((b) => rb.blockedFor(b.ua));
      const pct = Math.round(((AI_BOTS.length - blocked.length) / AI_BOTS.length) * 100);
      add("robots", "AI 爬虫放行（robots.txt）", "A 能不能抓到你", 12, pct,
        blocked.length ? `有 ${blocked.length} 个 AI 爬虫被挡在门外：${blocked.map((b) => b.who).join("、")}。它们进不来就永远不认识你。`
          : "robots.txt 对主流国产 + 海外 AI 爬虫都放行，很好。",
        blocked.length ? blocked.map((b) => `robots.txt 里放行 ${b.ua}（${b.who}）：删掉对它的 Disallow: /`) : []);
    }
  }
  {
    const body = llms.body || "";
    const present = llms.status === 200 && body.trim() && !/^\s*</.test(body) && /(^|\n)#\s+\S/.test(body);
    let pct = 0, detail = "没有 /llms.txt —— 这是给 AI 看的「网站说明书」，缺它 AI 要自己猜你是谁。",
      fixes = ["生成并上传 llms.txt 到网站根目录（本次已帮你生成一份草稿，见报告下方 / 已存到本地）"];
    if (present) {
      const hasSummary = /\n\s*>\s+\S/.test(body), hasH2 = /\n##\s+\S/.test(body), absLinks = /\]\(https?:\/\//.test(body);
      pct = 40 + (hasSummary ? 20 : 0) + (hasH2 ? 20 : 0) + (absLinks ? 20 : 0);
      detail = `已有 llms.txt。${hasSummary ? "" : "缺一句话摘要(blockquote)；"}${hasH2 ? "" : "缺 H2 分节列表；"}${absLinks ? "" : "链接应用绝对地址；"}`.replace(/；$/, "。") || "结构完整。";
      fixes = pct >= 100 ? [] : ["按规范补：单个 H1 + 一句 > 摘要 + 用 ## 分节列出核心页面（绝对链接）"];
    }
    add("llms", "AI 网站说明书（llms.txt）", "A 能不能抓到你", 12, pct, detail, fixes);
  }

  // B 好不好解析你
  {
    let pct = 0; const has = (t) => ldT.has(t);
    if (ldT.size) {
      pct = 40 + Math.min(30, ldProps >= 5 ? 30 : ldProps * 6);
      if (has("Organization") || has("LocalBusiness")) pct += 15;
      if (has("WebSite")) pct += 8;
      if (has("FAQPage") || has("Article") || has("BlogPosting") || has("Product") || has("BreadcrumbList")) pct += 7;
    }
    add("schema", "结构化数据（JSON-LD）", "B 好不好解析你", 14, pct,
      ldT.size ? `检测到结构化数据：${[...ldT].slice(0, 6).join("、")}（最多 ${ldProps} 个字段）。`
        : "没有 JSON-LD 结构化数据 —— AI 只能靠猜你是「谁、做什么、在哪」。这是性价比最高的一块。",
      ldT.size ? (has("Organization") || has("LocalBusiness") ? [] : ["补 Organization / LocalBusiness 结构化数据（本次已帮你生成骨架）"])
        : ["加 Organization + WebSite 结构化数据（本次已生成 schema.jsonld 骨架，填好贴进 <head>）"]);
  }
  {
    let pct = 0; const t = (title || "").length;
    if (title) pct += t >= 8 && t <= 64 ? 28 : 16;
    if (desc) pct += desc.length >= 40 && desc.length <= 170 ? 24 : 14;
    if (hasCanonical) pct += 16; if (lang) pct += 14; if (hasViewport) pct += 10; if (hasFavicon) pct += 8;
    const miss = [];
    if (!title) miss.push("<title>"); else if (t < 8 || t > 64) miss.push("title 长度建议 8~64 字");
    if (!desc) miss.push("meta description"); else if (desc.length < 40) miss.push("description 太短(建议 40~160)");
    if (!hasCanonical) miss.push("canonical"); if (!lang) miss.push("<html lang>"); if (!hasFavicon) miss.push("favicon");
    add("meta", "元信息 / 预览标签", "B 好不好解析你", 10, pct,
      miss.length ? `待补：${miss.join("、")}。` : "标题、描述、canonical、语言等齐全。",
      miss.length ? [`补齐 <head>：${miss.join("、")}`] : []);
  }
  {
    const og = ["og:title", "og:description", "og:image", "og:type"].filter((k) => meta[k]).length;
    const tw = meta["twitter:card"] ? 1 : 0;
    const pct = Math.round(((og + tw) / 5) * 100);
    add("og", "社交 / 富预览（OpenGraph）", "B 好不好解析你", 5, pct,
      `OpenGraph ${og}/4${tw ? " + Twitter Card" : ""}。被转发 / 被 AI 摘要时的封面与标题靠它。`,
      pct >= 80 ? [] : ["补 og:title / og:description / og:image / og:type，能被分享和 AI 卡片正确展示"]);
  }

  // C 愿不愿引用你（KDD 实证权重最高的一组）
  {
    const per = words ? statHits / (words / 150) : 0; // 每 ~150 字一个数字为满
    const pct = words < 60 ? 0 : Math.min(100, Math.round(per * 60 + (statHits ? 20 : 0)));
    add("stats", "数字 / 统计密度", "C 愿不愿引用你", 10, pct,
      `全文约 ${statHits} 处数字/统计。含数字的页面被 AI 引用影响力实测高 33%（AI 爱可核对的硬事实）。`,
      pct >= 70 ? [] : ["把「很多、很快、领先」换成具体数字（服务 3000+ 客户、3 年、提速 60%），每段尽量带一个数"]);
  }
  {
    const n = authLinks.length;
    const pct = n >= 2 ? 100 : n === 1 ? 55 : 0;
    add("cite", "引用权威来源", "C 愿不愿引用你", 9, pct,
      n ? `引用了 ${n} 个权威来源（.gov/.edu/官媒/知网/维基等）。` : "没有引用任何权威外链。引用来源的页面被 AI 采信度实测高 27%。",
      pct >= 100 ? [] : ["在内容里引 2+ 个权威来源（政府/行业协会/官媒/学术），并给出链接"]);
  }
  {
    const blocks = listCnt + tableCnt + Math.min(3, Math.floor(defHits / 2)) + Math.min(2, quoteCnt) + Math.min(2, Math.floor(stepHits / 2));
    const pct = Math.min(100, blocks * 16);
    const bits = [];
    if (listCnt) bits.push(`${listCnt} 个列表`); if (tableCnt) bits.push(`${tableCnt} 个表格`);
    if (defHits) bits.push(`${defHits} 处定义句`); if (quoteCnt) bits.push(`${quoteCnt} 处引述`); if (stepHits) bits.push("有步骤块");
    add("blocks", "可抽取答案块", "C 愿不愿引用你", 8, pct,
      bits.length ? `含 ${bits.join("、")}。AI 爱能整段抽走的「定义 / 列表 / 表格 / 步骤 / 引述」。` : "几乎没有可抽取的结构块（定义/列表/表格/步骤）。AI 很难从纯段落里抽答案。",
      pct >= 70 ? [] : ["把关键信息改成：1 句定义 +（2-5 个要点列表 / 一个对比表）+ 操作步骤，别堆成大段落"]);
  }
  {
    let pct = 60;
    if (h1.length === 1) pct += 15; else if (h1.length === 0) { pct -= 25; } else pct -= 12;
    if (h23.length >= 3) pct += 15; else pct -= (3 - h23.length) * 5;
    if (qHead >= 1) pct += 10;
    add("heading", "标题层级结构", "C 愿不愿引用你", 7, pct,
      `H1 ${h1.length} 个 · H2/H3 ${h23.length} 个${qHead ? ` · ${qHead} 个问题式小标题` : ""}。清晰的层级=AI 分块检索的锚点。`,
      [h1.length === 1 ? "" : "全页保留且只保留 1 个 H1（页面主题）", h23.length >= 3 ? "" : "用 H2/H3 把内容分成 6~10 个清晰小节",
        qHead ? "" : "把小标题写成用户会问的问题（如「XX 多少钱？」「如何选 XX？」）——命中 AI 问答"].filter(Boolean));
  }
  {
    const pct = words < 200 ? 20 : words < 500 ? 50 : words <= 4000 ? 100 : 82;
    add("length", "正文充分度", "C 愿不愿引用你", 5, pct,
      `正文约 ${words} 字/词。太薄(<300)几乎不被深度吸收；800~2500 区间最稳；一味堆长也无益。`,
      pct >= 80 ? [] : ["把内容补到 800+ 字，覆盖客户真正关心的多个子问题（价格/流程/资质/案例/常见问答）"]);
  }
  {
    const lead = text.slice(0, 160);
    const hasEntity = keyword ? lead.includes(keyword) : /[一-鿿]{2,}/.test(lead);
    const inTitle = keyword && title ? title.includes(keyword) : !!title;
    let pct = 40 + (hasEntity ? 30 : 0) + (inTitle ? 30 : 0);
    add("frontload", "前置 / 对题（开头就说清）", "C 愿不愿引用你", 4, pct,
      `${inTitle ? "标题点题" : "标题未点题"}；${hasEntity ? "开头就出现主体/关键词" : "开头没直接点主体"}。语义对题是 AI 引用最强预测因子(r=0.43)。`,
      pct >= 80 ? [] : ["标题带上核心词（品牌/服务/地区）；导语第一句直接给答案，别用一大段铺垫开场"]);
  }

  // D 认不认识你（品牌实体 + 信号）
  {
    let pct = 0;
    if (entityLinks.length) pct += 55;
    if (/"author"|itemprop=["']author|rel=["']author|作者[:：]/i.test(html)) pct += 20;
    if (dateMod) { const d = Date.parse(dateMod); if (d && (Date.now() - d) < 365 * 864e5) pct += 25; else pct += 8; }
    add("entity", "品牌实体 · 作者 · 新鲜度", "D 认不认识你", 4, pct,
      `${entityLinks.length ? "有实体图链接(维基/领英/知乎/企查查等)" : "缺实体图链接(sameAs)"}；${dateMod ? "有更新时间" : "无更新时间标记"}。这些帮 AI 确认「你是一个真实存在的主体」。`,
      pct >= 80 ? [] : ["在 Organization 的 sameAs 里链上你的维基/领英/知乎/企查查主页；正文标注作者与更新日期"]);
  }

  // ── 反面信号（隐藏文字 / 提示注入 / 关键词堆砌）：扣分，最多 -10 ──
  let penalty = 0; const penNotes = [];
  if (/style\s*=\s*["'][^"']*(display\s*:\s*none|visibility\s*:\s*hidden|font-size\s*:\s*0)/i.test(html)
      && textOf((/(<[^>]+display\s*:\s*none[\s\S]{0,600})/i.exec(html) || [])[1] || "").length > 120) {
    penalty += 5; penNotes.push("疑似隐藏文字（display:none 里塞了正文）—— 被判作弊会掉权重");
  }
  if (/<!--[\s\S]{0,400}(ignore\s+previous|as an ai|system\s*prompt|assistant\s*:|忽略(以上|之前)指令)[\s\S]{0,400}-->/i.test(html)) {
    penalty += 6; penNotes.push("HTML 注释里疑似提示注入内容 —— 会被 AI 平台拉黑");
  }
  penalty = Math.min(10, penalty);

  const raw = D.reduce((a, d) => a + d.weight * d.pct / 100, 0);
  const score = clamp(raw - penalty);

  // ── 生成修复文件：llms.txt + schema.jsonld（可直接给客户网站用）──
  const llmsDraft = genLlmsTxt({ title, desc, host, url, name });
  const schemaDraft = genSchema({ title, desc, url, origin, name });
  let llmsPath = "", schemaPath = "";
  try { llmsPath = writeText(P.llms, llmsDraft); } catch {}
  try { schemaPath = writeText(join(P.site, "结构化数据-schema.jsonld"), schemaDraft); } catch {}

  const result = {
    url, host, name, sampledAt: today(), score, penalty, penNotes,
    dims: D, meta: { title, desc, words, h1: h1.length, h23: h23.length },
    llmsExists: llms.status === 200, llmsPath, schemaPath, llmsDraft, schemaDraft,
  };
  const outHtml = join(P.site, "网页AI友好度诊断.html");
  let pagePath = "";
  try { pagePath = writeText(outHtml, renderHtml(result)); } catch (e) { return fail(jsonMode, "写报告失败：" + e.message, 1); }

  logE(`✓ AI 友好度 ${score}/100 → ${pagePath}`);
  return done(jsonMode, { ok: true, page: pagePath, score, url, llms: llmsPath, schema: schemaPath },
    `AI 友好度 ${score}/100 → ${pagePath}`);
}

// ── 生成 llms.txt（规范：单 H1 + 一句 > 摘要 + ## 分节绝对链接）──
function genLlmsTxt({ title, desc, host, url, name }) {
  const t = name || title || host || "本站";
  const summary = (desc || `${t} 的官方网站。`).replace(/\s+/g, " ").slice(0, 180);
  const base = url.replace(/\/$/, "");
  return `# ${t}

> ${summary}

## 核心页面

- [首页](${base}/): ${t} 概览
- [关于我们](${base}/about): 团队、资质与背景
- [产品 / 服务](${base}/services): 我们能为你做什么
- [联系方式](${base}/contact): 电话、地址与在线咨询

## 常见问题

- [常见问题](${base}/faq): 客户最常问的问题与解答

## Optional

- [新闻 / 博客](${base}/blog): 最新动态（可按需抓取）

<!-- 由「一搜商答 / 1so」自动生成的草稿。把上面链接改成你网站真实路径，删掉不存在的，传到网站根目录 /llms.txt。 -->
`;
}

// ── 生成 Organization + WebSite JSON-LD 骨架 ──
function genSchema({ title, desc, url, origin, name }) {
  const n = name || title || (origin ? origin.replace(/^https?:\/\//, "") : "你的公司");
  const obj = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "Organization",
        name: n,
        url: origin || url,
        description: (desc || `${n} 官方介绍`).slice(0, 200),
        logo: (origin || "") + "/logo.png",
        sameAs: ["https://你的知乎主页", "https://你的领英", "https://baike.baidu.com/item/你的品牌"],
        contactPoint: { "@type": "ContactPoint", telephone: "+86-000-0000000", contactType: "customer service", areaServed: "CN" },
      },
      { "@type": "WebSite", name: n, url: origin || url },
    ],
  };
  return JSON.stringify(obj, null, 2)
    + "\n\n<!-- 把它放进网页 <head>：<script type=\"application/ld+json\"> …上面 JSON… </script>。填好 sameAs / 电话 / logo 后生效。 -->\n";
}

// ── 报告 HTML ──────────────────────────────────────────────────
function scoreLabel(s) {
  if (s <= 35) return "AI 基本读不懂你（急需优化）";
  if (s <= 67) return "打了地基，还差临门一脚";
  if (s <= 85) return "对 AI 挺友好，可再精修";
  return "AI 友好度优秀";
}
function renderHtml(r) {
  const score = clamp(r.score);
  const groups = [...new Set(r.dims.map((d) => d.group))];
  // 优化建议按「影响力 = 权重 ×(100-得分)」降序，先改最亏的
  const fixes = r.dims.flatMap((d) => d.fixes.map((f) => ({ f, impact: d.weight * (100 - d.pct), label: d.label })))
    .sort((a, b) => b.impact - a.impact).slice(0, 8);

  const groupHtml = groups.map((g) => {
    const rows = r.dims.filter((d) => d.group === g).map((d) => `
      <div class="dim">
        <div class="dh"><span class="dn">${esc(d.label)}</span><span class="dv ${band(d.pct)}">${d.pct}<small>/100</small></span></div>
        <div class="track"><i class="${band(d.pct)}" style="width:${d.pct}%"></i></div>
        <p class="dd">${esc(d.detail)}</p>
      </div>`).join("");
    return `<h2>${esc(g)}</h2><div class="dims">${rows}</div>`;
  }).join("");

  return `<!DOCTYPE html>
<html lang="zh-CN"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>网页 AI 友好度诊断 · ${esc(r.host || r.url)}</title>
<style>
  :root{--fg:#16162a;--mut:#6b7280;--line:#ececf3;--accent:#4f46e5;--green:#16a34a;--red:#dc2626;--bg:#f6f7fb}
  *{box-sizing:border-box} body{font:15px/1.6 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;color:var(--fg);background:var(--bg);margin:0}
  .wrap{max-width:880px;margin:0 auto;padding:28px 18px 60px}
  .brand{color:var(--accent);font-weight:700;font-size:13px;letter-spacing:.5px}
  h1{font-size:22px;margin:6px 0 2px;word-break:break-all} .sub{color:var(--mut);font-size:13px;margin:0}
  .hero{display:flex;gap:24px;align-items:center;background:#fff;border:1px solid var(--line);border-radius:16px;padding:22px;margin:18px 0}
  .ring{width:130px;height:130px;border-radius:50%;flex:0 0 auto;display:grid;place-items:center;background:conic-gradient(var(--accent) calc(var(--v)*1%), #e5e7eb 0)}
  .ring b{background:#fff;width:104px;height:104px;border-radius:50%;display:grid;place-items:center;flex-direction:column;line-height:1}
  .ring .big{font-size:34px;color:var(--accent);font-weight:800} .ring small{color:var(--mut);font-size:11px;margin-top:3px}
  .hero .lab{font-size:19px;font-weight:700;margin:0 0 6px} .hero .vd{color:var(--mut);font-size:14px;margin:0}
  h2{font-size:15px;margin:24px 0 10px;color:var(--mut);font-weight:700;letter-spacing:.3px}
  .dims{display:grid;gap:10px} .dim{background:#fff;border:1px solid var(--line);border-radius:12px;padding:12px 15px}
  .dh{display:flex;justify-content:space-between;align-items:baseline} .dn{font-weight:600;font-size:14px} .dv{font-size:19px;font-weight:800} .dv small{font-size:11px;color:var(--mut);font-weight:600}
  .dv.ok{color:var(--green)} .dv.warn{color:#d97706} .dv.bad{color:var(--red)}
  .track{height:7px;background:#eef;border-radius:6px;overflow:hidden;margin:8px 0 6px} .track i{display:block;height:100%}
  .track i.ok{background:linear-gradient(90deg,#4ade80,#16a34a)} .track i.warn{background:linear-gradient(90deg,#fbbf24,#d97706)} .track i.bad{background:linear-gradient(90deg,#f87171,#dc2626)}
  .dd{color:#444;font-size:12.5px;margin:0}
  .fixes{background:#fff;border:1px solid var(--line);border-radius:12px;padding:16px 18px}
  .fixes ol{margin:8px 0 0;padding-left:20px} .fixes li{margin:7px 0;font-size:13.5px;line-height:1.5}
  .fixes .tag{display:inline-block;font-size:11px;color:var(--accent);background:#eef2ff;border-radius:6px;padding:1px 7px;margin-right:6px}
  .deliver{display:grid;grid-template-columns:1fr 1fr;gap:14px;margin-top:14px}
  @media(max-width:640px){.deliver{grid-template-columns:1fr}}
  .card{background:#fff;border:1px solid var(--line);border-radius:12px;padding:14px 16px}
  .card h3{margin:0 0 6px;font-size:14px} .card .hint{color:var(--mut);font-size:12px;margin:0 0 8px}
  pre{background:#0f172a;color:#e2e8f0;border-radius:9px;padding:12px;overflow:auto;font:12px/1.5 "SFMono-Regular",Consolas,monospace;max-height:280px;margin:0}
  .pen{background:#fff7ed;border:1px solid #fed7aa;color:#9a3412;border-radius:10px;padding:10px 14px;margin:14px 0;font-size:13px}
  .rband{display:flex;gap:10px;align-items:flex-start;background:#eef2ff;border:1px solid #c7d2fe;border-radius:10px;padding:10px 14px;margin:14px 0;font-size:13px;color:#3730a3}
  .rband b{color:#1e1b4b}
  .insights{background:#fff;border:1px solid var(--line);border-radius:12px;padding:16px 18px;margin-top:4px}
  .ins-lead{margin:0 0 12px;font-size:14px;color:#374151}
  .ins-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:12px}
  .ins{background:#f9fafb;border-radius:10px;padding:12px 14px}
  .ins b{display:block;font-size:13.5px;color:var(--accent);margin-bottom:4px}
  .ins p{margin:0;font-size:12.5px;color:#4b5563;line-height:1.55}
  .ins-foot{margin:12px 0 0;font-size:12px;color:var(--mut);line-height:1.5}
  footer{margin-top:26px;color:var(--mut);font-size:12px;text-align:center;line-height:1.7}
  footer a{color:var(--accent);text-decoration:none}${payCss()}
</style></head>
<body><div class="wrap">
  <div class="brand">🔎 一搜商答 · 网页 AI 友好度诊断（免费）</div>
  <h1>${esc(r.host || r.url)}</h1>
  <p class="sub">${esc(r.url)}　·　正文约 ${r.meta.words} 字　·　${esc(r.sampledAt)}</p>

  <div class="hero">
    <div class="ring" style="--v:${score}"><b><span class="big">${score}</span><small>/100 AI 友好度</small></b></div>
    <div><p class="lab">${esc(scoreLabel(score))}</p><p class="vd">这是「AI 能不能读懂、愿不愿引用你网页」的体检分，越高越容易被豆包 / DeepSeek / ChatGPT 收录引用。</p></div>
  </div>

  <div class="rband"><span>📊</span><p>评分维度融合了 <b>Auriti GEO（MIT）</b> 100 分体系、<b>普林斯顿 KDD'24</b> 实证（加数字 +33% / 引用来源 +27% / 加引述 +41%）、<b>214,119 条中文 AI 引用记录</b>与 <b>23,745 条跨平台引用特征</b>的公开实证研究（CN-GEO · 跨平台引用实验），并覆盖国产 AI 爬虫（字节·豆包 / 百度·文心 / 华为 / 神马 / 搜狗 / 360）。</p></div>

  ${r.penNotes.length ? `<div class="pen">⚠ 检测到 ${r.penalty ? `扣分项（-${r.penalty}）` : "风险"}：${r.penNotes.map(esc).join("；")}。</div>` : ""}

  ${groupHtml}

  <h2>优先改这些（按影响力排序）</h2>
  <div class="fixes">
    ${fixes.length ? `<ol>${fixes.map((x) => `<li><span class="tag">${esc(x.label)}</span>${esc(x.f)}</li>`).join("")}</ol>`
      : "<p>👍 主要维度都不错，保持并持续更新内容即可。</p>"}
  </div>

  <h2>已帮你生成的修复文件（拿去就能用）</h2>
  <div class="deliver">
    <div class="card"><h3>llms.txt（AI 网站说明书）</h3><p class="hint">存到本地：${esc(r.llmsPath || "（写入失败）")}<br>传到网站根目录 /llms.txt 即可。</p><pre>${esc(r.llmsDraft)}</pre></div>
    <div class="card"><h3>结构化数据 JSON-LD 骨架</h3><p class="hint">存到本地：${esc(r.schemaPath || "（写入失败）")}<br>填好后贴进网页 &lt;head&gt;。</p><pre>${esc(r.schemaDraft)}</pre></div>
  </div>

  <h2>被 AI 深度吸收的页面有哪些共性 · 研究洞察</h2>
  <div class="insights">
    <p class="ins-lead">基于 23,745 条被 AI 引用页面的特征分析，这些规律在国产 + 海外 AI 上普遍成立，可对照你的页面自查：</p>
    <div class="ins-grid">
      <div class="ins"><b>① 写成「证据页」不是「观点页」</b><p>含定义、数字、对比、操作步骤的页面，被 AI 引用影响力高 41%–62%。空泛总结很难被抽走。</p></div>
      <div class="ins"><b>② 先够长，再谈够好</b><p>1000+ 词、6–10 个清晰小节的页面显著优于短内容；&lt;170 词的页面几乎不被深度吸收。</p></div>
      <div class="ins"><b>③ 标题正文都要「对题」</b><p>页面与问题的语义贴合度是影响力最强预测因子（r=0.43）。关键词进标题、导语直接回答。</p></div>
      <div class="ins"><b>④ 发布位置是筛选门槛</b><p>官网+新闻+行业垂类占被引来源 79%–88%。发在 AI 很少看的站点，后面优化很吃力。</p></div>
      <div class="ins"><b>⑤ 同内容做平台分发版</b><p>ChatGPT 重单条深度，Google 重标题对齐，Perplexity 重广覆盖。一个页面不一定吃满三家。</p></div>
    </div>
    <p class="ins-foot">⚠ 反常识：纯 Q&amp;A 格式并未带来优势（-5.7%）；「短而精」不如「长而结构化」。完整研究见页底论文链接。</p>
  </div>

  ${renderPayBlock("以上是免费诊断 + 修复文件草稿。想省心？付费后由我们直接上手：改造成 AI 读得懂的企业主页、写好 llms.txt / 结构化数据并部署、同步三大地图，并每月复测追踪分数：")}

  <footer>由「一搜商答 / 1so」生成 · ${esc(r.sampledAt)}。诊断为对页面 HTML 的静态分析，仅供参考。<br>
    方法论：<a href="https://arxiv.org/abs/2607.15771" target="_blank" rel="noopener">CN-GEO 中文生成式搜索引用研究</a> · <a href="https://arxiv.org/abs/2604.25707" target="_blank" rel="noopener">跨平台引用选择与吸收测量框架</a> · <a href="https://dl.acm.org/doi/10.1145/3637528.3671900" target="_blank" rel="noopener">Princeton GEO (KDD'24)</a> · <a href="https://github.com/Auriti-Labs/geo-optimizer-skill" target="_blank" rel="noopener">Auriti GEO (MIT)</a></footer>
</div></body></html>`;
}
