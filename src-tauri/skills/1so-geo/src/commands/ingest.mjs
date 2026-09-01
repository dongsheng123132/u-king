// 1so ingest —— 读本地资料 → LLM 提炼成结构化“知识卡”（cards.json）。
// 核心原则：提炼不是编造。要求模型只用资料里出现过的信息，缺就留空，绝不臆造。
import { projectPaths } from "../config.mjs";
import { chatJson } from "../llm.mjs";
import { readMaterials, parseJsonLoose, writeJson, logE, warn, done, fail } from "../util.mjs";

const MAX_CHARS = 40000; // 单次喂给模型的资料上限，超出截断并告知（不静默丢）。

const SYSTEM = `你是"一搜商答"的商家知识提炼引擎。任务：把老板提供的原始资料，提炼成结构化的商家知识卡。
铁律：
1. 只用资料里真实出现的信息。资料没提到的，字段留空字符串或空数组，绝不编造、绝不脑补行业套话。
2. 保留老板的真实表达、真实数字、真实案例，不要美化成广告腔。
3. 输出严格符合下面的 JSON 结构。`;

const SCHEMA_HINT = `输出 JSON 结构：
{
  "company": {"name":"","person":"","region":"","industry":"","intro":"","contact":"","address":""},
  "services": [{"name":"","desc":"","audience":"","process":"","priceRange":""}],
  "products": [{"name":"","desc":"","specs":""}],
  "cases": [{"title":"","problem":"","solution":"","result":""}],
  "faqs": [{"q":"","a":""}],
  "opinions": [{"topic":"","view":""}],
  "keywords": []
}
keywords：从资料里提炼 3~8 个"客户会拿去问 AI"的真实业务关键词。`;

export async function cmdIngest(args, llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || args._[0] || ".");
  const { files, skipped, missing } = readMaterials(P.materials);

  if (missing) return fail(jsonMode, `没找到资料目录：${P.materials}\n请把资料（.txt/.md 等）放进去，或用 --project 指定目录。`, 2);
  if (files.length === 0) return fail(jsonMode, `资料目录里没有可读的文本资料：${P.materials}`, 2);
  if (skipped.length) warn(`跳过 ${skipped.length} 个暂不支持的文件（v0.1 只读文本类）：${skipped.slice(0, 8).join("、")}${skipped.length > 8 ? " …" : ""}`);

  let corpus = files.map((f) => `# 资料：${f.name}\n${f.text}`).join("\n\n---\n\n");
  let truncated = false;
  if (corpus.length > MAX_CHARS) { corpus = corpus.slice(0, MAX_CHARS); truncated = true; warn(`资料较多，本次只提炼前 ${MAX_CHARS} 字（其余未纳入，后续可分批）。`); }

  logE(`读取 ${files.length} 份资料，共 ${corpus.length} 字，提炼中…`);
  const raw = await chatJson(SYSTEM, `${SCHEMA_HINT}\n\n=== 原始资料开始 ===\n${corpus}\n=== 原始资料结束 ===`, { ...llmOpts, maxTokens: 4000 });
  let cards;
  try { cards = parseJsonLoose(raw); } catch (e) { return fail(jsonMode, "提炼结果解析失败：" + e.message); }

  cards.company ||= {};
  cards.services ||= []; cards.products ||= []; cards.cases ||= []; cards.faqs ||= []; cards.opinions ||= []; cards.keywords ||= [];
  cards._meta = { sources: files.map((f) => f.name), truncated, generatedAt: new Date().toISOString() };
  writeJson(P.cards, cards);

  const n = { 服务: cards.services.length, 产品: cards.products.length, 案例: cards.cases.length, 问答: cards.faqs.length, 观点: cards.opinions.length };
  logE(`✓ 已生成知识卡：${P.cards}`);
  logE(`  公司：${cards.company.name || "（资料未明确）"}｜` + Object.entries(n).map(([k, v]) => `${k} ${v}`).join(" · "));
  return done(jsonMode, { ok: true, cards: P.cards, company: cards.company.name || "", counts: n, truncated }, P.cards);
}
