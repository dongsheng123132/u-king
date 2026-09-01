// 1so optimize —— 拿"AI 现在怎么看你"（detect）对比"你的真实资料"（cards），
// 给出让 AI 能找到你、正确描述你、在买家问题里推荐你的补内容清单。这就是"基于自己资料怎么优化"。
import { projectPaths } from "../config.mjs";
import { chatJson } from "../llm.mjs";
import { readJson, parseJsonLoose, writeText, logE, done, fail, today } from "../util.mjs";

const SYSTEM = `你是"一搜商答"的 GEO 优化顾问。你会拿到两份东西：
A. 这家商家的真实资料（结构化知识卡）。
B. 当前 AI 对这家商家的认知（可见度检测结果）。
目标：找出"AI 认知"和"真实情况"之间的差距，给出具体、可执行的补内容清单，让 AI 能正确认识并在买家问题里推荐这家商家。
铁律：所有建议只能基于 A 里真实存在的资料来"补充展示"，不能建议编造不存在的资质/案例/评价。反对关键词堆砌、反对批量城市页那种 SEO 垃圾。`;

export async function cmdOptimize(args, llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || ".");
  const cards = readJson(P.cards, null);
  const detect = readJson(P.detect, null);
  if (!cards) return fail(jsonMode, "缺少知识卡，请先 1so ingest。", 2);
  if (!detect) return fail(jsonMode, "缺少检测结果，请先 1so detect。", 2);

  const user = `A. 真实资料（知识卡）：
${JSON.stringify(cards, null, 1).slice(0, 12000)}

B. AI 当前认知（检测结果摘要）：
${JSON.stringify(detect.summary, null, 1)}

请输出 JSON：
{
  "diagnosis": "",                 // 一段话诊断：AI 认知 vs 真实情况的核心差距
  "actions": [                     // 按优先级排序的补内容清单（3~8 条）
    {"priority":"高/中/低","title":"要补什么内容/页面","why":"为什么补(对应哪个买家问题或哪条gap)","from":"依据资料里的哪部分"}
  ],
  "unanswered_questions": [],      // 买家会问、但目前你的内容还没答上的真实问题
  "avoid": []                      // 提醒不要做的事（如某类关键词页）
}`;

  logE("对比 AI 认知与真实资料，生成优化建议…");
  let plan;
  try { plan = parseJsonLoose(await chatJson(SYSTEM, user, { ...llmOpts, maxTokens: 2500 })); }
  catch (e) { return fail(jsonMode, "优化建议解析失败：" + e.message); }

  writeText(P.optimize, renderMd(cards, detect, plan));
  logE(`✓ 优化建议：${P.optimize}`);
  logE(`  ${(plan.actions || []).length} 条补内容建议，${(plan.unanswered_questions || []).length} 个待答买家问题`);
  return done(jsonMode, { ok: true, optimize: P.optimize, actions: plan.actions || [] }, P.optimize);
}

function renderMd(cards, detect, plan) {
  const name = cards.company?.name || detect.company || "";
  return `# 优化建议：让 AI 正确认识并推荐「${name}」

> 基于你的真实资料 × AI 当前认知（可见度 ${detect.summary?.score ?? "?"}/100，${detect.summary?.known ? "AI 知道你" : "AI 还不知道你"}）
> 生成日期：${today()}

## 诊断
${plan.diagnosis || ""}

## 该补什么（按优先级）
${(plan.actions || []).map((a, i) => `### ${i + 1}. 【${a.priority || "中"}】${a.title}
- **为什么**：${a.why || ""}
- **依据资料**：${a.from || ""}`).join("\n\n")}

## 买家在问、你还没答上的问题
${(plan.unanswered_questions || []).length ? plan.unanswered_questions.map((q) => "- " + q).join("\n") : "（暂无）"}

## 千万别做（反污染红线）
${(plan.avoid || []).length ? plan.avoid.map((q) => "- " + q).join("\n") : "- 不要堆砌关键词页；不要批量生成「XX 城市 + 行业哪家好」这类门页。"}

---
> 补完资料后，重新跑 \`1so ingest\` → \`1so generate\` 更新答案页，再 \`1so detect\` 复测 AI 可见度是否提升。
`;
}
