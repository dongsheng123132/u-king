// 1so aicheck —— 对接各大模型跑一遍问答，给"各家 AI 一个分数 + 总分"，出可视化测试报告。
// 免费测试出分 → 报告底部是三档"收费方案"入口（支付宝直接付款，见 PAY）——卖的是「我们帮你做 GEO+MEO 优化」。
// 默认后端 = 虾盘云(--provider openai + device.json 设备 Key)，客户零配置、免费测试成本我们承担(几分钱)当引流钩子；
// 想让客户自带 key、我们零成本，跑 --provider openrouter（一 key 通全网，BYOK 扣客户费用）。
// --render：不调 API，从已有 aicheck.json 重新出报告（改了模板/文案后重渲染用）。
import { projectPaths } from "../config.mjs";
import { chat, chatJson } from "../llm.mjs";
import { readJson, parseJsonLoose, writeJson, writeText, logE, warn, done, fail, today, esc } from "../util.mjs";
import { renderPayBlock, payCss } from "../pay.mjs";

// 各家大模型清单，按后端(provider)选。默认走 U-King 内置的虾盘云(openai 兼容端点 + device.json 的设备 Key)——
// 客户零配置就能出分，免费测试成本我们承担(几分钱)当引流钩子。这些 id 全是产品「换模型」下拉在用的、
// 虾盘云确定托管的(见 src/lib/models.ts XIAPAN_MODELS)，且都挑了便宜/非纯推理档，稳且省。
// **国产为主**：GEO 面向国内商家，客户/客户的客户搜索用的是 DeepSeek/通义/智谱/MiniMax 这些国产 AI，
// 走虾盘云也更稳更省；海外只留 GPT/Gemini「能测就测」。全部 2026-07-22 用设备 Key 实测调通
// （kimi-k2.6 是纯推理档、短输出常返空，已剔除）。谁答上来算谁、判读自动回退，出个大差不差的分即可。
// 想让客户自带 key、我们零成本，跑 `--provider openrouter`（一 key 通全网模型，BYOK）。
const MODELS_BY_PROVIDER = {
  openai: ["deepseek-v4-flash", "qwen3.7-max", "glm-5", "MiniMax-M3", "gpt-5.4-mini", "gemini-2.5-flash"],
  openrouter: ["openai/gpt-4o-mini", "anthropic/claude-3.5-sonnet", "google/gemini-flash-1.5", "deepseek/deepseek-chat", "meta-llama/llama-3.3-70b-instruct"],
};
// 评分器改国产 deepseek-v4-flash（快/省/非纯推理/必在白名单，比 gpt-5.4-mini 更稳；失败仍回退到本轮答上来的模型）。
const JUDGE_BY_PROVIDER = { openai: "deepseek-v4-flash", openrouter: "openai/gpt-4o-mini" };

// 报告里把模型 id 显示成客户认得的品牌名（认得 GPT/Claude/Gemini/DeepSeek 比认得 gpt-5.4-mini 强）。
const MODEL_LABELS = {
  "gpt-5.4-mini": "GPT · OpenAI", "gpt-5.4": "GPT-5.4 · OpenAI",
  "claude-haiku-4-5": "Claude · Anthropic", "claude-sonnet-4-6": "Claude Sonnet · Anthropic",
  "gemini-2.5-flash": "Gemini · 谷歌", "gemini-3.1-pro-preview": "Gemini Pro · 谷歌",
  "deepseek-v4-flash": "DeepSeek · 深度求索", "deepseek-v4-pro": "DeepSeek · 深度求索",
  "qwen3.7-max": "通义千问 · 阿里", "glm-5": "智谱 GLM", "MiniMax-M3": "MiniMax · 海螺",
  "kimi-k2.6": "Kimi · 月之暗面", "grok-4.3": "Grok · xAI",
};
const modelLabel = (id) => MODEL_LABELS[id] || id;

// 收费方案（PRICING）+ 支付宝收款（PAY）+ payHrefFor + 定价区块渲染 已抽到 ../pay.mjs
// （单一真相源，aicheck / inspect 两份报告共用；改价改文案只动 pay.mjs）。

function scoreLabel(s) {
  if (s <= 20) return "AI 几乎不认识你";
  if (s <= 50) return "少数 AI 模糊知道你";
  if (s <= 75) return "部分 AI 认识你，但不全面";
  return "多数 AI 认识并会推荐你";
}

// 评分器全部调不通时的「本地启发式出分」——绝不让「聚合评分这一步」把整份报告干掉。
// 规则很粗但够用：模型明说「不了解 / 没查到」→ 低分（不认识）；给出实质介绍 → 按篇幅给个保守中分。
// 触发场景：pc-***「This token has no access to model gpt-5.4-mini」——评分器候选全挂时的最后兜底。
const UNKNOWN_RE = /不了解|不知道|没查到|未找到|没有(找到|听说|相关)|无法(确认|查到|提供|获取)|不清楚|查不到|抱歉|没听说过|暂无|not\s+(aware|familiar)|don'?t\s+(know|have)|no\s+(information|record|data)/i;
function heuristicSummary(answered, name) {
  const per = answered.map((r) => {
    const t = String(r.answer || "").trim();
    const known = t.length > 40 && !(UNKNOWN_RE.test(t) && t.length < 260);
    const score = known ? Math.min(60, 28 + Math.round(t.length / 24)) : 8;
    return { model: r.model, score, known, willRecommend: false, note: t.slice(0, 70) };
  });
  const overall = per.length ? Math.round(per.reduce((a, m) => a + m.score, 0) / per.length) : 0;
  return {
    perModel: per,
    overallScore: overall,
    consensus: `（评分器暂不可用，此为本地估算）多数 AI 对「${name}」了解有限。`,
    gaps: ["可被 AI 收录的企业主页 / 官网", "权威平台上的公司简介与资质", "行业高频问题的公开答案"],
    verdict: "AI 对你的了解有限，建议补全可被 AI 收录的资料以提升可见度。",
  };
}

export async function cmdAicheck(args, _llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || ".");

  // --render：离线从已有 json 重新出报告
  if (args.render) {
    const r = readJson(P.aicheck, null);
    if (!r) return fail(jsonMode, "没有 aicheck.json 可重渲染，先正常跑一次 aicheck。", 2);
    writeReports(P, r);
    logE(`✓ 已从 aicheck.json 重新出报告：${P.aicheckPage}`);
    return done(jsonMode, { ok: true, page: P.aicheckPage }, P.aicheckPage);
  }

  const cards = readJson(P.cards, null);
  const name = (args.name && args.name !== true) ? String(args.name) : cards?.company?.name;
  if (!name) return fail(jsonMode, "缺少公司名。用 --name \"公司名\"，或先 1so ingest。", 2);
  const region = (args.region && args.region !== true) ? String(args.region) : cards?.company?.region || "";
  const industry = (args.industry && args.industry !== true) ? String(args.industry) : cards?.company?.industry || "";
  // 后端：默认虾盘云(openai 兼容 + device.json 设备 Key，客户零配置)；--provider openrouter 走 BYOK。
  const provider = (args.provider && args.provider !== true) ? String(args.provider).toLowerCase() : "openai";
  const byok = provider === "openrouter";
  const models = (args.models && args.models !== true)
    ? String(args.models).split(/[,，]/).map((s) => s.trim()).filter(Boolean)
    : (MODELS_BY_PROVIDER[provider] || MODELS_BY_PROVIDER.openai);
  const judgeModel = JUDGE_BY_PROVIDER[provider] || JUDGE_BY_PROVIDER.openai;
  const base = (args.base && args.base !== true) ? String(args.base) : undefined;
  const key = (args.key && args.key !== true) ? String(args.key) : undefined;

  warn(byok
    ? `将并行问 ${models.length} 个大模型，计费走你自己的 OpenRouter 账户（BYOK，扣客户费用）。`
    : `免费测试：并行问 ${models.length} 个大模型，用 U-King 内置额度（虾盘云），一次约几分钱。`);
  logE(`对接大模型：${models.map(modelLabel).join(" / ")}`);

  const buyerSys = `你是一个通用 AI 助手，一位普通顾客在向你提问。只根据你真实掌握的知识回答；不知道这家具体商家就直说"不了解/没查到"，绝不编造。回答简洁。`;
  const buyerQ = `我想了解${region ? region + "的" : ""}「${name}」${industry ? "（" + industry + "）" : ""}这家公司/商家：你知道它吗？如果知道，请介绍它、并说明你会不会把它推荐给有相关需求的人；如果不知道就直说。`;

  const results = await Promise.all(models.map(async (model) => {
    try {
      const a = await chat([{ role: "system", content: buyerSys }, { role: "user", content: buyerQ }],
        { provider, model, key, base, maxTokens: 500, timeoutSec: 90 });
      logE(`  ✓ ${model}`);
      return { model, answer: a, ok: true };
    } catch (e) {
      logE(`  ✗ ${model}：${String(e.message || e).slice(0, 60)}`);
      return { model, answer: "", ok: false, error: String(e.message || e).slice(0, 120) };
    }
  }));

  const answered = results.filter((r) => r.ok);
  if (!answered.length) {
    if (byok) return fail(jsonMode, "所有模型都没调通。检查 OpenRouter key（OPENROUTER_API_KEY 或 --key）与 --models 是否有效。", 1);
    // 分类失败原因，别再把「网络没通」误报成「余额不足」——客户会以为 token 没了（实测：挂代理→node
    // 不走代理→ENOTFOUND，token 其实好好的）。按各模型收集到的 error 判类型，给准话。
    const errs = results.map((r) => String(r.error || "")).join(" | ");
    const isNet = /ENOTFOUND|EAI_AGAIN|ECONNREFUSED|ETIMEDOUT|ECONNRESET|getaddrinfo|fetch failed|network|socket hang|time.?d? ?out|timeout/i.test(errs);
    const isAuth = /\b401\b|\b403\b|无权|no access|unauthorized|invalid api key/i.test(errs);
    const isQuota = /余额|quota|insufficient|欠费|balance|\b402\b|\b429\b/i.test(errs);
    const msg = isNet
      ? "网络没通，调不到模型（常见：挂了代理/VPN、或本机 DNS 异常）——这不是余额问题、token 没坏。关掉代理或换个网络重试；虾盘云国内直连免代理。"
      : isAuth
      ? "这个 Key 没权限调这些模型（token 白名单缺模型）——不是余额没了。联系客服补白名单，或去「AI 设置」换个驱动重试。"
      : isQuota
      ? "余额不足或被限流——去「AI 设置」查虾盘云余额、充值后重试。"
      : "所有模型都没调通——去「AI 设置」测一下虾盘云连通/余额，或稍后重试。";
    return fail(jsonMode, msg, 1);
  }

  const judgeSys = `你是"一搜商答"的 AI 可见度评分师。下面是多个大模型对同一家商家的回答。给每个模型打一个 0-100 的"认识度分"：
0=完全不知道这家；1-30=只识别名字或泛泛而谈；31-60=知道且描述大致正确；61-85=了解较全面且正面；86-100=非常了解且明确会推荐。
诚实：明说不知道/只会讲行业套话的必须低分。`;
  const judgeUser = `商家：${name}｜地区：${region || "未知"}｜行业：${industry || "未知"}
各模型回答：
${answered.map((r, i) => `【${i + 1}】模型 ${r.model}：\n${r.answer}`).join("\n\n")}

输出 JSON：
{
  "perModel": [{"model":"","score":0,"known":true/false,"willRecommend":true/false,"note":"它如何描述你/一句话"}],
  "overallScore": 0,
  "consensus": "多数模型对你的共识描述（没有就写'普遍不了解'）",
  "gaps": ["各模型普遍缺失的关键信息，3~5条"],
  "verdict": "一句话结论"
}`;
  // 评分器要稳：judgeModel 打头，后面接上「本轮真的答上来的」模型——它们对这个 token / 网络已证明可用，
  // 是最靠谱的兜底评分器（正解 pc-***「token 无权 gpt-5.4-mini」：判读回退到能用的模型即可，不必整份报废）。
  const judgeCandidates = [...new Set([judgeModel, ...answered.map((r) => r.model)])];
  let sum = null, lastErr = null;
  for (const jm of judgeCandidates) {
    try {
      sum = parseJsonLoose(await chatJson(judgeSys, judgeUser, { provider, model: jm, key, base, maxTokens: 1800 }));
      if (jm !== judgeModel) logE(`  评分器回退到 ${jm}`);
      break;
    } catch (e) { lastErr = e; sum = null; }
  }
  // 候选评分器全挂 → 本地启发式出分，绝不因「评分这一步」让整份报告崩掉（有回答就一定出得了报告）。
  if (!sum) {
    logE(`  聚合评分全部失败（${String(lastErr?.message || lastErr).slice(0, 60)}），改用本地启发式出分`);
    sum = heuristicSummary(answered, name);
  }

  // 兜底：总分缺失就用各家均分
  const per = sum.perModel || [];
  if (typeof sum.overallScore !== "number" && per.length)
    sum.overallScore = Math.round(per.reduce((a, m) => a + (Number(m.score) || 0), 0) / per.length);

  const result = { company: name, region, industry, models, sampledAt: today(), answers: results, summary: sum, byok };
  writeJson(P.aicheck, result);
  writeReports(P, result);

  const score = sum.overallScore || 0;
  logE(`✓ 测试报告：${P.aicheckPage}`);
  logE(`  AI 可见度总分 ${score}/100（${scoreLabel(score)}）｜${per.filter((m) => m.known).length}/${answered.length} 个模型认识你`);
  return done(jsonMode, { ok: true, page: P.aicheckPage, score, models: per.map((m) => ({ model: m.model, score: m.score })) },
    `AI 可见度 ${score}/100 → ${P.aicheckPage}`);
}

function writeReports(P, r) {
  writeText(P.aicheckReport, renderMd(r));
  writeText(P.aicheckPage, renderHtml(r));
}

function renderMd(r) {
  const s = r.summary, per = s.perModel || [], score = s.overallScore || 0;
  return `# AI 可见度测试报告

> 商家：**${r.company}**　地区：${r.region || "—"}　行业：${r.industry || "—"}
> 对接 ${r.models.length} 个大模型　抽样：${r.sampledAt}　计费：${r.byok ? "你的账户(BYOK)" : "U-King 内置额度(虾盘云)"}

## AI 可见度总分：${score}/100 —— ${scoreLabel(score)}
${s.verdict || ""}

## 各家 AI 的分数
| 大模型 | 分数 | 认识你 | 会推荐 | 它怎么说 |
|---|:--:|:--:|:--:|---|
${per.map((m) => `| ${modelLabel(m.model)} | ${m.score ?? "-"} | ${m.known ? "✅" : "❌"} | ${m.willRecommend ? "✅" : "—"} | ${(m.note || "").replace(/\|/g, "／")} |`).join("\n")}

## 多数模型的共识描述
${s.consensus || "（普遍不了解）"}

## 各模型普遍缺的关键信息
${(s.gaps || []).map((g, i) => `${i + 1}. ${g}`).join("\n") || "（无）"}
`;
}

function renderHtml(r) {
  const s = r.summary, per = s.perModel || [], score = Math.max(0, Math.min(100, s.overallScore || 0));
  const modelCards = per.map((m) => {
    const sc = Math.max(0, Math.min(100, Number(m.score) || 0));
    return `<div class="mc">
      <div class="mc-top"><span class="mn">${esc(modelLabel(m.model))}</span><span class="ms">${sc}</span></div>
      <div class="track"><i style="width:${sc}%"></i></div>
      <div class="chips">${m.known ? '<span class="ck ok">认识你</span>' : '<span class="ck no">不认识</span>'}${m.willRecommend ? '<span class="ck rec">会推荐</span>' : ""}</div>
      <p class="note">${esc(m.note || "")}</p>
    </div>`;
  }).join("");

  return `<!DOCTYPE html>
<html lang="zh-CN"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>AI 可见度测试报告 · ${esc(r.company)}</title>
<style>
  :root{--fg:#16162a;--mut:#6b7280;--line:#ececf3;--accent:#4f46e5;--green:#16a34a;--red:#dc2626;--bg:#f6f7fb}
  *{box-sizing:border-box} body{font:15px/1.6 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;color:var(--fg);background:var(--bg);margin:0}
  .wrap{max-width:860px;margin:0 auto;padding:28px 18px 60px}
  .brand{color:var(--accent);font-weight:700;font-size:13px;letter-spacing:.5px}
  h1{font-size:22px;margin:6px 0 2px} .sub{color:var(--mut);font-size:13px;margin:0}
  .hero{display:flex;gap:24px;align-items:center;background:#fff;border:1px solid var(--line);border-radius:16px;padding:22px;margin:18px 0}
  .ring{width:130px;height:130px;border-radius:50%;flex:0 0 auto;display:grid;place-items:center;
    background:conic-gradient(var(--accent) calc(var(--v)*1%), #e5e7eb 0)}
  .ring b{background:#fff;width:104px;height:104px;border-radius:50%;display:grid;place-items:center;flex-direction:column;line-height:1}
  .ring .big{font-size:34px;color:var(--accent);font-weight:800} .ring small{color:var(--mut);font-size:11px;margin-top:3px}
  .hero .lab{font-size:19px;font-weight:700;margin:0 0 6px} .hero .vd{color:var(--mut);font-size:14px;margin:0}
  h2{font-size:17px;margin:26px 0 12px;padding-left:10px;border-left:4px solid var(--accent)}
  .mc{background:#fff;border:1px solid var(--line);border-radius:12px;padding:14px 16px;margin-bottom:10px}
  .mc-top{display:flex;justify-content:space-between;align-items:baseline} .mn{font-weight:600} .ms{font-size:22px;font-weight:800;color:var(--accent)}
  .track{height:8px;background:#eef;border-radius:6px;overflow:hidden;margin:8px 0} .track i{display:block;height:100%;background:linear-gradient(90deg,#818cf8,#4f46e5)}
  .chips{display:flex;gap:6px;margin:2px 0 6px} .ck{font-size:11px;padding:2px 8px;border-radius:999px;background:#f1f1f7;color:var(--mut)}
  .ck.ok{background:#dcfce7;color:var(--green)} .ck.no{background:#fee2e2;color:var(--red)} .ck.rec{background:#e0e7ff;color:var(--accent)}
  .note{color:#444;font-size:13px;margin:0}
  .box{background:#fff;border:1px solid var(--line);border-radius:12px;padding:14px 16px}
  .gaps li{margin:4px 0}${payCss()}
  footer{margin-top:26px;color:var(--mut);font-size:12px;text-align:center}
  details{margin-top:8px} summary{cursor:pointer;color:var(--mut);font-size:13px}
  .rband{display:flex;gap:10px;align-items:flex-start;background:#eef2ff;border:1px solid #c7d2fe;border-radius:10px;padding:10px 14px;margin:14px 0;font-size:13px;color:#3730a3}
  .rband .rb-ic{font-size:18px;flex:0 0 auto;line-height:1.5}
  .rband p{margin:0;line-height:1.5}
  .rband b{color:#1e1b4b}
  .insights{background:#fff;border:1px solid var(--line);border-radius:12px;padding:16px 18px}
  .ins-lead{margin:0 0 12px;font-size:14px;color:#374151}
  .ins-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:12px}
  .ins{background:#f9fafb;border-radius:10px;padding:12px 14px}
  .ins b{display:block;font-size:13.5px;color:var(--accent);margin-bottom:4px}
  .ins p{margin:0;font-size:12.5px;color:#4b5563;line-height:1.55}
  .ins-foot{margin:12px 0 0;font-size:12px;color:var(--mut);line-height:1.5}
  footer a{color:var(--accent);text-decoration:none} footer a:hover{text-decoration:underline}
  .demo{background:#fef3c7;border:1px solid #f59e0b;color:#78350f;border-radius:12px;padding:12px 16px;margin:0 0 18px;font-size:13.5px;line-height:1.6}
  .demo b{color:#7c2d12}
  .demo-foot{margin:10px 0 0;font-size:12px;color:#92400e}
</style></head>
<body><div class="wrap">
  ${r.demo ? `<div class="demo">⚠️ <b>这是一份演示样例，不是对某家真实公司的检测结果。</b>
    公司名称、各家 AI 的分数与原话全部为虚构的示例数据，仅用于说明报告长什么样、包含哪些维度。
    <div class="demo-foot">想要一份针对你公司的真实报告？加微信 <b>hecare888</b>，我们逐一实测 6 家大模型后人工出具。</div></div>` : ""}
  <div class="brand">🔎 一搜商答 · AI 可见度测试${r.demo ? "（演示样例）" : "（免费）"}</div>
  <h1>${esc(r.company)}</h1>
  <p class="sub">${[r.region, r.industry].filter(Boolean).map(esc).join("　·　")}　·　对接 ${r.models.length} 个大模型　·　${esc(r.sampledAt)}</p>

  <div class="hero">
    <div class="ring" style="--v:${score}"><b><span class="big">${score}</span><small>/100 可见度</small></b></div>
    <div><p class="lab">${esc(scoreLabel(score))}</p><p class="vd">${esc(s.verdict || "")}</p></div>
  </div>

  <div class="rband">
    <span class="rb-ic">📊</span>
    <p>本报告的评分维度与优化建议，基于 <b>214,119 条中文 AI 引用记录</b> 与 <b>23,745 条跨平台引用特征</b> 的公开实证研究（CN-GEO · 跨平台引用实验）。</p>
  </div>

  <h2>各家 AI 的分数</h2>
  ${modelCards || "<p>（无数据）</p>"}

  <h2>它们普遍怎么看你 · 缺什么</h2>
  <div class="box">
    <p><b>共识描述：</b>${esc(s.consensus || "普遍不了解")}</p>
    <p><b>普遍缺的关键信息：</b></p>
    <ul class="gaps">${(s.gaps || []).map((g) => `<li>${esc(g)}</li>`).join("") || "<li>（无）</li>"}</ul>
    <details><summary>展开各模型原始回答</summary>
      ${r.answers.map((a) => `<p><b>${esc(modelLabel(a.model))}</b>${a.ok ? "" : "（未调通）"}<br>${esc(a.answer || a.error || "")}</p>`).join("")}
    </details>
  </div>

  <h2>为什么 AI 没把你排进去 · 研究洞察</h2>
  <div class="insights">
    <p class="ins-lead">基于 23,745 条被 AI 引用页面的特征分析，被 AI 深度吸收的页面有这些共性（你的页面可以对照自查）：</p>
    <div class="ins-grid">
      <div class="ins"><b>① 写成「证据页」不是「观点页」</b><p>含定义、数字、对比、操作步骤的页面，被 AI 引用影响力高 41%–62%。空泛总结很难被抽走。</p></div>
      <div class="ins"><b>② 先够长，再谈够好</b><p>1000+ 词、6–10 个清晰小节的页面显著优于短内容；&lt;170 词的页面几乎不被深度吸收。</p></div>
      <div class="ins"><b>③ 标题正文都要「对题」</b><p>页面与问题的语义贴合度是影响力最强预测因子（r=0.43）。关键词进标题、导语直接回答。</p></div>
      <div class="ins"><b>④ 发布位置是筛选门槛</b><p>官网+新闻+行业垂类占被引来源 79%–88%。发在 AI 很少看的站点，后面优化很吃力。</p></div>
      <div class="ins"><b>⑤ 同内容做平台分发版</b><p>ChatGPT 重单条深度，Google 重标题对齐，Perplexity 重广覆盖。一个页面不一定吃满三家。</p></div>
    </div>
    <p class="ins-foot">⚠ 反常识：纯 Q&amp;A 格式并未带来优势（-5.7%）；「短而精」不如「长而结构化」。完整研究见页底论文链接。</p>
  </div>

  ${renderPayBlock("以上是免费测试概览。付费后由我们上手：把你变成 AI 读得懂的企业主页（GEO）、同步高德/百度/腾讯三大地图商家信息（MEO），并持续复测追踪分数：")}

  <footer>由「一搜商答 / 1so」生成 · ${esc(r.sampledAt)}${r.byok ? " · 计费 BYOK（用你自己的 key）" : ""}。分数为本次抽样结果，AI 回答有波动，建议多次复测取趋势。<br>
    方法论依据：<a href="https://arxiv.org/abs/2607.15771" target="_blank" rel="noopener">CN-GEO 中文生成式搜索引用研究</a> · <a href="https://arxiv.org/abs/2604.25707" target="_blank" rel="noopener">跨平台引用选择与吸收测量框架</a></footer>
</div></body></html>`;
}
