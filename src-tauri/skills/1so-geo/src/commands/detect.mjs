// 1so detect —— 检测「公司在 AI 眼里的样子」。杀手功能，产出成交利器报告。
//   第 1 步：扮演"顾客随手问的通用 AI"，如实回答一组买家问题（不知道就说不知道）。
//   第 2 步：分析这些问答 → AI 是否知道你 / 如何描述你 / 准不准 / 缺什么 / 提到哪些同行 / 打分。
// 现实诚实：各家 AI 无稳定"引用来源 API"，这是抽样探测，不是实时仪表盘（见规划文档 §7）。
import { projectPaths } from "../config.mjs";
import { chat, chatJson } from "../llm.mjs";
import { parseJsonLoose, readJson, writeJson, writeText, logE, warn, done, fail, today, esc } from "../util.mjs";

function buildQueries({ name, region, industry, keywords }) {
  const kw = (keywords || []).slice(0, 3);
  const qs = [
    `你知道${region ? region + "的" : ""}「${name}」这家${industry || "公司"}吗？请介绍一下它。`,
    `${region || ""}想找${industry || "这类服务"}，有哪些靠谱的商家或个人推荐？`,
    `「${name}」和同行相比有什么区别或优势？`,
  ];
  for (const k of kw) qs.push(`${region || ""}${k}找谁比较好？`);
  return qs;
}

export async function cmdDetect(args, llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || ".");
  const cards = readJson(P.cards, null);

  const name = (args.name && args.name !== true) ? String(args.name) : cards?.company?.name;
  if (!name) return fail(jsonMode, "缺少公司名。用 --name \"公司名\" 指定，或先 1so ingest 从资料里提炼。", 2);
  const region = (args.region && args.region !== true) ? String(args.region) : cards?.company?.region || "";
  const industry = (args.industry && args.industry !== true) ? String(args.industry) : cards?.company?.industry || "";
  const keywords = (args.keywords && args.keywords !== true) ? String(args.keywords).split(/[,，、]/).map(s => s.trim()).filter(Boolean)
    : (cards?.keywords || []);

  const engine = llmOpts.provider === "openai" ? `openai/${llmOpts.model}` : `百炼/${llmOpts.model}`;
  warn(`当前只探测 1 个 AI 引擎（${engine}）。多引擎（豆包/Kimi/DeepSeek/百度AI…）为后续版本，届时会逐个探测汇总。`);

  const queries = buildQueries({ name, region, industry, keywords });
  logE(`向 AI 抛出 ${queries.length} 个买家问题，看它怎么回答「${name}」…`);

  // 第 1 步：让 AI 如实作答
  const answerSys = `你是一个通用 AI 助手，一位普通顾客在向你提问。请只根据你真实掌握的知识回答。
如果你并不知道某家具体的公司/商家，就直白说"我没有查到 / 不了解这家"，绝不要编造它的业务、地址、评价。回答简洁真实。`;
  const answers = [];
  for (const q of queries) {
    let a = "";
    try { a = await chat([{ role: "system", content: answerSys }, { role: "user", content: q }], { ...llmOpts, maxTokens: 600, timeoutSec: 90 }); }
    catch (e) { a = "（探测失败：" + (e.message || e) + "）"; }
    answers.push({ q, a });
    logE(`  · ${q.slice(0, 28)}… ✓`);
  }

  // 第 2 步：分析
  const analyzeSys = `你是"一搜商答"的 AI 可见度分析师。下面是"顾客问 AI、AI 如实回答"的问答记录。
请客观分析这家商家在 AI 眼里的现状。诚实：如果 AI 明显不知道这家商家，就如实指出——这正是它需要做 GEO 的原因。`;
  const analyzeUser = `商家：${name}｜地区：${region || "未知"}｜行业：${industry || "未知"}
问答记录：
${answers.map((x, i) => `Q${i + 1}. ${x.q}\nA${i + 1}. ${x.a}`).join("\n\n")}

输出 JSON：
{
  "known": true/false,               // AI 是否真的知道这家商家
  "score": 0-100,                    // AI 可见度综合分（不知道=很低）
  "howDescribed": "",                // AI 目前如何描述这家（不知道就写"AI 对其一无所知"）
  "accurate": "",                    // 描述是否准确、有无偏差
  "competitors": [],                 // AI 在相关问题里提到了哪些同行/替代者
  "gaps": [],                        // AI 缺哪些关于这家的关键信息（3~6 条）
  "verdict": ""                      // 一句话总结现状
}`;
  let summary;
  try { summary = parseJsonLoose(await chatJson(analyzeSys, analyzeUser, { ...llmOpts, maxTokens: 1500 })); }
  catch (e) { return fail(jsonMode, "分析结果解析失败：" + e.message); }

  const result = { company: name, region, industry, keywords, engine, sampledAt: today(), queries: answers, summary };
  writeJson(P.detect, result);
  writeText(P.report, renderReport(result));
  logE(`✓ 检测报告：${P.report}`);
  logE(`  AI 是否知道你：${summary.known ? "是" : "否"}｜可见度评分：${summary.score}/100`);
  return done(jsonMode, { ok: true, detect: P.detect, report: P.report, known: summary.known, score: summary.score },
    `AI 可见度 ${summary.score}/100（${summary.known ? "AI 知道你" : "AI 还不知道你"}）→ ${P.report}`);
}

function renderReport(r) {
  const s = r.summary;
  const bar = "█".repeat(Math.round((s.score || 0) / 10)).padEnd(10, "░");
  return `# 你的公司在 AI 眼里是什么样？

> 商家：**${r.company}**　地区：${r.region || "—"}　行业：${r.industry || "—"}
> 探测引擎：${r.engine}　抽样日期：${r.sampledAt}
> （说明：AI 无稳定的"引用来源接口"，本报告为抽样探测结果，仅反映本次探测。）

## 一句话结论
${s.verdict || ""}

## AI 可见度评分
\`${bar}\` **${s.score}/100**　AI 是否知道你：**${s.known ? "✅ 知道" : "❌ 还不知道"}**

## AI 目前怎么描述你
${s.howDescribed || "（无）"}

准确性：${s.accurate || "—"}

## AI 提到的同行 / 替代者
${(s.competitors || []).length ? s.competitors.map((c) => "- " + c).join("\n") : "（本次探测未提到明显同行）"}

## AI 缺了关于你的哪些关键信息
${(s.gaps || []).length ? s.gaps.map((g, i) => `${i + 1}. ${g}`).join("\n") : "（无）"}

## 探测问答实录
${r.queries.map((x, i) => `**Q${i + 1}. ${x.q}**\n\n${x.a}\n`).join("\n")}

---
> 想让 AI 正确认识你、在这些问题里推荐你？下一步用 \`1so ingest\` 提炼你的资料，\`1so optimize\` 拿到补内容清单，\`1so generate\` 生成 AI 可读的答案页。
`;
}
