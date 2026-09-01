// 1so questions —— 生成"你这个行业，大家最关心 / AI 被问得最多"的高频问题，
// 并基于老板的真实资料起草答案（资料没覆盖的标"待补充"，提示老板去表达）。多答多表达 = 多 GEO 表面。
// --merge：把答好的问答并进 cards.faqs → 重跑 generate，企业主页内容就变多了（回应"还能增加吗"）。
import { projectPaths } from "../config.mjs";
import { chatJson } from "../llm.mjs";
import { readJson, writeJson, parseJsonLoose, writeText, logE, warn, done, fail, today } from "../util.mjs";

const SYSTEM = `你是"一搜商答"的行业问答策划。给定一个行业（和这家商家的真实资料），列出这个行业里**客户/潜在客户最常问、AI 里被问得最多**的真实问题——买家决策前真会问的那种，不要泛泛而谈、不要关键词堆砌。
然后对每个问题，**只用商家资料里真实存在的信息**起草这家商家的答案；资料没覆盖到的，答案写"（待老板补充）"并在 need 里说明缺什么。
铁律：不编造资质/数据/案例；鼓励老板用真实经验、真实差异化来答。`;

export async function cmdQuestions(args, llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || ".");
  const cards = readJson(P.cards, null);

  const industry = (args.industry && args.industry !== true) ? String(args.industry) : cards?.company?.industry;
  if (!industry) return fail(jsonMode, "缺少行业。用 --industry \"行业\" 指定，或先 1so ingest 从资料提炼。", 2);
  const region = (args.region && args.region !== true) ? String(args.region) : cards?.company?.region || "";
  const n = Math.min(30, Math.max(5, parseInt(args.n, 10) || 12));

  const cardsBrief = cards ? JSON.stringify({ company: cards.company, services: cards.services, products: cards.products, cases: cards.cases, opinions: cards.opinions }).slice(0, 8000) : "（暂无资料，答案先留待补充）";
  logE(`生成「${industry}」行业 ${n} 个高频问题 + 起草答案 …`);

  const user = `行业：${industry}${region ? "（地区：" + region + "）" : ""}
商家真实资料：${cardsBrief}

输出 JSON：{"questions":[{"q":"客户真会问的高频问题","a":"基于资料起草的答案，或'（待老板补充）'","need":"若待补充，说明缺什么信息（否则空）","hot":"高/中"}]}
给 ${n} 个，按热度从高到低。`;

  let plan;
  try { plan = parseJsonLoose(await chatJson(SYSTEM, user, { ...llmOpts, maxTokens: 3500 })); }
  catch (e) { return fail(jsonMode, "生成失败：" + e.message); }
  const qs = (plan.questions || []).filter((x) => x && x.q);
  if (!qs.length) return fail(jsonMode, "没有生成到问题，换个说法或补充行业信息重试。");

  const out = P.report.replace("报告-AI眼里的你.md", "行业问答.md");
  writeText(out, renderMd(industry, region, qs));
  logE(`✓ 行业问答：${out}`);

  // --merge：把已答好的（非待补充）并进 cards.faqs
  let merged = 0;
  if (args.merge && cards) {
    cards.faqs ||= [];
    const exist = new Set(cards.faqs.map((f) => f.q));
    for (const x of qs) {
      if (x.a && !x.a.includes("待") && !exist.has(x.q)) { cards.faqs.push({ q: x.q, a: x.a }); merged++; }
    }
    if (merged) { writeJson(P.cards, cards); logE(`✓ 已并入知识卡 ${merged} 条问答（重跑 1so generate 主页即更新）。`); }
  }

  const todo = qs.filter((x) => x.a && x.a.includes("待")).length;
  if (todo) warn(`有 ${todo} 个问题资料没覆盖，标了"待老板补充"——这些正是你该去多表达的点。`);
  return done(jsonMode, { ok: true, file: out, count: qs.length, merged, todo }, out);
}

function renderMd(industry, region, qs) {
  return `# 「${industry}」行业高频问答${region ? "（" + region + "）" : ""}

> 客户最常问、AI 被问得最多的问题 + 基于你资料起草的答案。
> 标"（待老板补充）"的，是你该去多表达的空白点。答得越多、越真，AI 越懂你。
> 生成日期：${today()}

${qs.map((x, i) => `## ${i + 1}. ${x.q}　${x.hot === "高" ? "🔥" : ""}
${x.a || "（待老板补充）"}
${x.need ? `\n> 待补充：${x.need}` : ""}`).join("\n\n")}

---
> 补/改完答案后：把满意的问答加进 \`.1so/cards.json\` 的 faqs（或重跑 \`1so questions --merge\`），再 \`1so generate\`，这些问答就进你的企业主页、被 AI 收录。
`;
}
