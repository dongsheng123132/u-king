// 1so generate —— 知识卡 → AI 可读的"答案页"（语义 HTML + JSON-LD + llms.txt）。
// 确定性渲染：页面 = 知识卡的忠实呈现，不二次生成文案（可溯源、不注水）。
// GEO 交付要点：纯静态、语义化、无 JS 墙、结构化数据满配。
import { projectPaths } from "../config.mjs";
import { readJson, writeText, ensureDir, logE, done, fail, today, esc } from "../util.mjs";

export async function cmdGenerate(args, _llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || ".");
  const cards = readJson(P.cards, null);
  if (!cards) return fail(jsonMode, "还没有知识卡。请先运行 1so ingest。", 2);

  ensureDir(P.site);
  const html = renderPage(cards);
  writeText(P.page, html);
  writeText(P.llms, renderLlms(cards));
  logE(`✓ 已生成答案页：${P.page}`);
  logE(`✓ 已生成 llms.txt：${P.llms}`);
  return done(jsonMode, { ok: true, page: P.page, llms: P.llms }, P.page);
}

function jsonLd(cards) {
  const c = cards.company || {};
  const graph = [];
  const biz = {
    "@context": "https://schema.org",
    "@type": "LocalBusiness",
    name: c.name || "",
    description: c.intro || "",
  };
  if (c.address) biz.address = c.address;
  if (c.region) biz.areaServed = c.region;
  if (c.contact) biz.telephone = c.contact;
  if ((cards.services || []).length)
    biz.makesOffer = cards.services.map((s) => ({ "@type": "Offer", itemOffered: { "@type": "Service", name: s.name, description: s.desc } }));
  graph.push(biz);

  const faqs = (cards.faqs || []).filter((f) => f.q && f.a);
  if (faqs.length) {
    graph.push({
      "@context": "https://schema.org",
      "@type": "FAQPage",
      mainEntity: faqs.map((f) => ({ "@type": "Question", name: f.q, acceptedAnswer: { "@type": "Answer", text: f.a } })),
    });
  }
  return graph.map((g) => `<script type="application/ld+json">\n${JSON.stringify(g, null, 2)}\n</script>`).join("\n");
}

// 地图/位置区块：三大地图搜索链接（用 公司名+地址/地区 定位）。
// 说明：无 key 的内嵌地图各家限制多，这里先给"在地图里找到我"的直达链接；
// 需要真正内嵌可视地图时，填各家 JS API key 后替换（见 README）。
function mapSection(c) {
  const q = [c.name, c.address || c.region].filter(Boolean).join(" ").trim();
  if (!q) return "";
  const eq = encodeURIComponent(q);
  const maps = [
    ["高德地图", `https://www.amap.com/search?query=${eq}`],
    ["百度地图", `https://map.baidu.com/search?wd=${eq}`],
    ["腾讯地图", `https://map.qq.com/?what=${eq}`],
  ];
  return `<section>
  <h2>位置 / 地图</h2>
  <p>${esc(c.address || c.region || "")}</p>
  <p class="maps">${maps.map(([n, u]) => `<a href="${esc(u)}" target="_blank" rel="noopener">在${n}打开 ↗</a>`).join("　")}</p>
</section>`;
}

function section(title, inner) { return inner ? `<section><h2>${esc(title)}</h2>${inner}</section>` : ""; }
function cardList(items, render) { return items && items.length ? `<div class="cards">${items.map(render).join("")}</div>` : ""; }

function renderPage(cards) {
  const c = cards.company || {};
  const desc = (c.intro || `${c.name || ""} ${c.industry || ""}`).slice(0, 140);
  const services = cardList(cards.services, (s) => `<article class="card"><h3>${esc(s.name)}</h3><p>${esc(s.desc)}</p>${s.audience ? `<p class="meta">适合：${esc(s.audience)}</p>` : ""}${s.process ? `<p class="meta">流程：${esc(s.process)}</p>` : ""}${s.priceRange ? `<p class="meta">价格：${esc(s.priceRange)}</p>` : ""}</article>`);
  const products = cardList(cards.products, (p) => `<article class="card"><h3>${esc(p.name)}</h3><p>${esc(p.desc)}</p>${p.specs ? `<p class="meta">规格：${esc(p.specs)}</p>` : ""}</article>`);
  const cases = cardList(cards.cases, (x) => `<article class="card"><h3>${esc(x.title)}</h3>${x.problem ? `<p><b>问题：</b>${esc(x.problem)}</p>` : ""}${x.solution ? `<p><b>做法：</b>${esc(x.solution)}</p>` : ""}${x.result ? `<p><b>结果：</b>${esc(x.result)}</p>` : ""}</article>`);
  const faqs = (cards.faqs || []).filter((f) => f.q && f.a);
  const faqHtml = faqs.length ? `<dl class="faq">${faqs.map((f) => `<dt>${esc(f.q)}</dt><dd>${esc(f.a)}</dd>`).join("")}</dl>` : "";
  const opinions = cardList(cards.opinions, (o) => `<article class="card"><h3>${esc(o.topic)}</h3><p>${esc(o.view)}</p></article>`);
  const sources = (cards._meta?.sources || []);

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(c.name || "商家答案页")}${c.industry ? "｜" + esc(c.industry) : ""}${c.region ? "｜" + esc(c.region) : ""}</title>
<meta name="description" content="${esc(desc)}">
<link rel="canonical" href="">
${jsonLd(cards)}
<style>
  :root{--fg:#1a1a1a;--mut:#666;--line:#eee;--accent:#c8102e}
  *{box-sizing:border-box}
  body{font:16px/1.7 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;color:var(--fg);max-width:820px;margin:0 auto;padding:32px 20px}
  header{border-bottom:2px solid var(--accent);padding-bottom:16px;margin-bottom:8px}
  h1{font-size:28px;margin:0 0 6px}
  .tagline{color:var(--mut);margin:0}
  .who{color:var(--mut);font-size:14px;margin-top:8px}
  h2{font-size:20px;margin:32px 0 12px;padding-left:10px;border-left:4px solid var(--accent)}
  h3{font-size:17px;margin:0 0 6px}
  .cards{display:grid;gap:14px}
  .card{border:1px solid var(--line);border-radius:10px;padding:14px 16px}
  .meta{color:var(--mut);font-size:14px;margin:4px 0 0}
  .faq dt{font-weight:600;margin-top:14px}
  .faq dd{margin:4px 0 0;color:#333}
  .maps a{display:inline-block;margin:6px 8px 0 0;padding:6px 12px;border:1px solid var(--line);border-radius:8px;text-decoration:none;color:var(--accent)}
  footer{margin-top:40px;padding-top:16px;border-top:1px solid var(--line);color:var(--mut);font-size:13px}
</style>
</head>
<body>
<header>
  <h1>${esc(c.name || "")}</h1>
  ${c.intro ? `<p class="tagline">${esc(c.intro)}</p>` : ""}
  <p class="who">${[c.person && "负责人：" + c.person, c.region, c.industry].filter(Boolean).map(esc).join("　·　")}</p>
</header>

${section("我们提供什么服务", services)}
${section("产品", products)}
${section("做过的案例", cases)}
${section("常见问题", faqHtml)}
${section("我们的观点", opinions)}

<section>
  <h2>联系方式</h2>
  <p>${[c.contact && "联系：" + esc(c.contact), c.address && "地址：" + esc(c.address)].filter(Boolean).join("<br>") || "（资料未提供）"}</p>
</section>

${mapSection(c)}

<footer>
  本页由商家提供的资料整理生成，经商家确认。资料来源：${sources.length ? esc(sources.join("、")) : "商家提供"}。<br>
  最后更新：${today()}　·　由「一搜商答 / 1so」生成，专为 AI 检索与引用优化。
</footer>
</body>
</html>`;
}

// llms.txt：给 AI 的纯文本摘要（约定俗成的 AI 友好入口文件）
function renderLlms(cards) {
  const c = cards.company || {};
  const lines = [`# ${c.name || ""}`, ""];
  if (c.intro) lines.push(`> ${c.intro}`, "");
  if (c.region || c.industry) lines.push(`地区：${c.region || "—"}　行业：${c.industry || "—"}`, "");
  if ((cards.services || []).length) { lines.push("## 服务"); for (const s of cards.services) lines.push(`- ${s.name}：${s.desc}`); lines.push(""); }
  if ((cards.faqs || []).length) { lines.push("## 常见问题"); for (const f of cards.faqs.filter(x => x.q && x.a)) lines.push(`- 问：${f.q}\n  答：${f.a}`); lines.push(""); }
  if (c.contact || c.address) lines.push(`## 联系\n${c.contact || ""} ${c.address || ""}`.trim(), "");
  lines.push(`最后更新：${today()}　来源：一搜商答 / 1so`);
  return lines.join("\n");
}
