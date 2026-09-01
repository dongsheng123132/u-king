/**
 * 闸门：「添加供应商」模板清单有两份手工维护的副本，构建期断言它们一致。
 *
 * ## 为什么要有它（2026-08-22 债务，08-23 补）
 *   · `src/lib/providerTemplates.ts` 的 `PROVIDER_TEMPLATES`（19 条静态表，前端首帧兜底）
 *   · `src-tauri/skills/install-windows.json` 与 `website/skills/install-windows.json`
 *     里的 `provider_templates` 字段（19 条，热下发路径）
 * 是**同一份数据抄了两遍**——今天一致纯属同一个人同一天写的。全仓没有任何东西断言
 * 它们一致；下次只改一份（比如债 1 那种模型 id 过期修复），首帧和离线路径就会画出
 * 两份不同的清单。
 *
 * 🔴 不把 TS 直接 import 那份 JSON——那会把整份 700 行装机清单（所有安装步骤、
 *    repair 脚本）打进前端 bundle，为 19 条模板拖进一堆前端用不到的东西。
 *    所以只用这条构建期比对脚本，两边各自维护，形状不一致就报错。
 *
 * ## 判据
 *   ① 条目数一致
 *   ② 三份文件逐条全字段比对（端点、模型、领 Key 链接/提示也不能漂移）
 *      不一致要指出具体哪一条哪个字段不同，不能只说「不一致」
 *
 * 用法：node scripts/check-provider-templates-sync.mjs
 */
import { readFileSync } from "node:fs";

const TS_FILE = "src/lib/providerTemplates.ts";
const JSON_FILE = "src-tauri/skills/install-windows.json";
const WEBSITE_JSON_FILE = "website/skills/install-windows.json";

const FIELDS = ["name", "openai_base", "anthropic_base", "model", "small_model", "key_url", "key_hint", "website"];

function parseTsTemplates(src) {
  const start = src.indexOf("export const PROVIDER_TEMPLATES");
  if (start < 0) {
    console.error(`❌ ${TS_FILE} 里找不到 PROVIDER_TEMPLATES`);
    process.exit(1);
  }
  const eqIdx = src.indexOf("=", start);
  const arrayStart = src.indexOf("[", eqIdx);
  // 找到与 arrayStart 配对的 ]（简单括号计数，数组里没有嵌套的 [] 字面量所以够用）
  let depth = 0;
  let arrayEnd = -1;
  for (let i = arrayStart; i < src.length; i++) {
    if (src[i] === "[") depth++;
    else if (src[i] === "]") {
      depth--;
      if (depth === 0) {
        arrayEnd = i;
        break;
      }
    }
  }
  if (arrayEnd < 0) {
    console.error(`❌ ${TS_FILE} 里 PROVIDER_TEMPLATES 数组没找到匹配的结尾`);
    process.exit(1);
  }
  const body = src.slice(arrayStart + 1, arrayEnd);

  // 逐个对象解析：按花括号配对切出每个 `{ ... }` 块，再从块里正则取字段。
  // 不用真的 TS/JS parser（不引依赖），字段都是简单字符串字面量，够稳。
  const objects = [];
  let objDepth = 0;
  let objStart = -1;
  for (let i = 0; i < body.length; i++) {
    if (body[i] === "{") {
      if (objDepth === 0) objStart = i;
      objDepth++;
    } else if (body[i] === "}") {
      objDepth--;
      if (objDepth === 0 && objStart >= 0) {
        objects.push(body.slice(objStart, i + 1));
        objStart = -1;
      }
    }
  }

  return objects.map((block) => {
    const entry = {};
    for (const field of FIELDS) {
      const m = block.match(new RegExp(`\\b${field}\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)"`));
      if (m) entry[field] = m[1];
    }
    return entry;
  });
}

function parseJsonTemplates(src) {
  const manifest = JSON.parse(src);
  const list = manifest.provider_templates;
  if (!Array.isArray(list)) {
    console.error(`❌ ${JSON_FILE} 里没有 provider_templates 数组`);
    process.exit(1);
  }
  return list.map((entry) => {
    const picked = {};
    for (const field of FIELDS) {
      if (entry[field] !== undefined) picked[field] = entry[field];
    }
    return picked;
  });
}

const tsSrc = readFileSync(TS_FILE, "utf8");
const jsonSrc = readFileSync(JSON_FILE, "utf8");
const websiteJsonSrc = readFileSync(WEBSITE_JSON_FILE, "utf8");

const tsTemplates = parseTsTemplates(tsSrc);
const jsonTemplates = parseJsonTemplates(jsonSrc);
const websiteJsonTemplates = parseJsonTemplates(websiteJsonSrc);

console.log(`${TS_FILE}: ${tsTemplates.length} 条`);
console.log(`${JSON_FILE} 的 provider_templates: ${jsonTemplates.length} 条`);
console.log(`${WEBSITE_JSON_FILE} 的 provider_templates: ${websiteJsonTemplates.length} 条`);

let bad = 0;

if (tsTemplates.length !== jsonTemplates.length) {
  console.error(
    `\n❌ 条目数不一致：TS ${tsTemplates.length} 条，JSON ${jsonTemplates.length} 条。` +
      `两份清单必须同步改。`,
  );
  bad++;
}
if (jsonTemplates.length !== websiteJsonTemplates.length) {
  console.error(`\n❌ 两份 skill 清单条目数不一致：内嵌 ${jsonTemplates.length} 条，网站 ${websiteJsonTemplates.length} 条。`);
  bad++;
}

const jsonByName = new Map(jsonTemplates.map((t) => [t.name, t]));
const tsNames = new Set(tsTemplates.map((t) => t.name));

for (const tsEntry of tsTemplates) {
  const jsonEntry = jsonByName.get(tsEntry.name);
  if (!jsonEntry) {
    console.error(`\n❌ 「${tsEntry.name}」只在 ${TS_FILE} 里有，${JSON_FILE} 缺这一条`);
    bad++;
    continue;
  }
  for (const field of FIELDS) {
    const a = tsEntry[field];
    const b = jsonEntry[field];
    if (a !== b) {
      console.error(
        `\n❌ 「${tsEntry.name}」的 ${field} 不一致：TS="${a ?? "(未设置)"}" ` +
          `JSON="${b ?? "(未设置)"}"`,
      );
      bad++;
    }
  }
}

for (const jsonEntry of jsonTemplates) {
  if (!tsNames.has(jsonEntry.name)) {
    console.error(`\n❌ 「${jsonEntry.name}」只在 ${JSON_FILE} 里有，${TS_FILE} 缺这一条`);
    bad++;
  }
}

const websiteByName = new Map(websiteJsonTemplates.map((t) => [t.name, t]));
for (const jsonEntry of jsonTemplates) {
  const websiteEntry = websiteByName.get(jsonEntry.name);
  if (!websiteEntry) {
    console.error(`\n❌ 「${jsonEntry.name}」只在 ${JSON_FILE} 里有，网站 skill 缺这一条`);
    bad++;
    continue;
  }
  for (const field of ["name", ...FIELDS]) {
    const a = jsonEntry[field];
    const b = websiteEntry[field];
    if (a !== b) {
      console.error(`\n❌ 「${jsonEntry.name}」的 ${field} 在两份 skill 不一致：内嵌="${a ?? "(未设置)"}" 网站="${b ?? "(未设置)"}"`);
      bad++;
    }
  }
}

if (bad) {
  console.error(`\n共 ${bad} 处不一致。改供应商模板时两份要一起改，别只改一边。`);
  process.exit(1);
}
console.log("\n✅ 两份供应商模板清单一致。");
