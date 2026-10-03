/**
 * 闸门：虾盘云模型目录（`xiapan-models.json`）的两份副本必须一致、目录本身必须合法、
 * 前端不许再手写第二份对话模型清单。
 *
 * ## 为什么要有它（2026-10-03，客户机实测 + 逐个真请求核对）
 * 「有哪些模型、谁能收图」以前硬编码在三十多个文件里，已经漂了：
 *   · DSH 配置漏声明 `input:["text","image"]` → 客户机上 DSH + 虾盘云发不了图；
 *   · 看图脚本把 deepseek-chat / deepseek-flash 标成纯文本（实测能看图）；
 *   · 「换模型」下拉里挂着上游已下线的 gemini-3.5-flash。
 * 现在只有一份数据，分成两个副本，跟装机清单同一套机制：
 *   · `src-tauri/models/xiapan-models.json` —— `include_str!` 编进 exe 的兜底（前端 import 的也是它）
 *   · `website/skills/xiapan-models.json`   —— 部署到服务器，热下发
 * 覆盖规则（`model_catalog.rs`）：**线上（或本地缓存）的 version 严格大于内嵌的才采用**。
 *
 * ## 判据（都可证伪）
 *   ① 两份除 `version` 外逐字节一致 —— 只改一边，下次构建/部署必有一份被悄悄回退；
 *   ② 线上那份 version ≥ 内嵌那份 —— 否则热下发永远不生效（本闸只比仓库里的两份，线上真值发版后从裸网核）；
 *   ③ schema 合法（与 Rust `validate` 同口径）：名字、version、分组/条目非空、id 唯一且无空白、
 *      `input` 非空且只含 text/image、default/strong 存在、作图模型 `edits` 必须是布尔值……
 *      Rust 端对坏文件是「整份丢弃回落内嵌」，所以这里放过一份坏文件 = 热下发那条路静默失效；
 *   ④ `src/lib/models.ts` 里没有再手写一份对话模型清单：`XIAPAN_MODELS` 必须由 `parseModelGroups(目录 JSON)` 生成，
 *      且文件里不能出现 `{ id: "<目录里的任何对话模型 id>" … }` 这种手写条目；
 *   ⑤ `model_catalog.rs` 的镜像地址顺序必须与 `installer.rs::SKILL_URLS` 一致（只差文件名）——
 *      国内直连 Vercel 经常不通，顺序是有意排的，漂了就是第二套顺序。
 *
 * 用法：node scripts/check-model-catalog-sync.mjs
 */
import { readFileSync, existsSync } from "node:fs";

const EMBEDDED = "src-tauri/models/xiapan-models.json";
const HOSTED = "website/skills/xiapan-models.json";
const MODELS_TS = "src/lib/models.ts";
const CATALOG_RS = "src-tauri/src/model_catalog.rs";
const INSTALLER_RS = "src-tauri/src/installer.rs";

const problems = [];
const fail = (msg) => problems.push(msg);

for (const f of [EMBEDDED, HOSTED, MODELS_TS, CATALOG_RS, INSTALLER_RS]) {
  if (!existsSync(f)) {
    console.error(`❌ 找不到 ${f}`);
    process.exit(1);
  }
}

const rawA = readFileSync(EMBEDDED, "utf8");
const rawB = readFileSync(HOSTED, "utf8");
let a, b;
try {
  a = JSON.parse(rawA);
} catch (e) {
  console.error(`❌ ${EMBEDDED} 不是合法 JSON：${e.message}`);
  process.exit(1);
}
try {
  b = JSON.parse(rawB);
} catch (e) {
  console.error(`❌ ${HOSTED} 不是合法 JSON：${e.message}`);
  process.exit(1);
}
console.log(`内嵌（编进 exe）: v${a.version}`);
console.log(`热下发（部署到服务器）: v${b.version}`);

// ── ② 版本方向 ──
if (!(b.version >= a.version)) {
  fail(
    `热下发那份版本更低（${b.version} < ${a.version}）—— 覆盖规则是「线上更大才覆盖」，` +
      `所以热下发这条路是断的。修法：把两份同步，并把 version 抬到比内嵌大（或相等后一起 +1）。`,
  );
}

// ── ① 除 version 外逐字节一致 ──
const VERSION_RE = /("version"\s*:\s*)\d+/;
const normA = rawA.replace(VERSION_RE, "$1<V>");
const normB = rawB.replace(VERSION_RE, "$1<V>");
if (normA !== normB) {
  // 指出具体哪里不一样，不能只说「不一致」
  const diffs = [];
  const walk = (x, y, path) => {
    if (JSON.stringify(x) === JSON.stringify(y)) return;
    if (x && y && typeof x === "object" && typeof y === "object" && Array.isArray(x) === Array.isArray(y)) {
      for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) walk(x[k], y[k], `${path}.${k}`);
    } else {
      diffs.push(`${path}：内嵌=${JSON.stringify(x)} / 热下发=${JSON.stringify(y)}`);
    }
  };
  const { version: _va, ...restA } = a;
  const { version: _vb, ...restB } = b;
  walk(restA, restB, "$");
  fail(
    `两份除 version 外不一致${diffs.length ? "：\n   - " + diffs.slice(0, 8).join("\n   - ") : "（结构相同，差异在空白/键序，逐字节比对自查）"}` +
      `\n   只改一边 = 下次构建或部署时有一份会被悄悄回退。改目录请两份一起改。`,
  );
}

// ── ③ schema（与 model_catalog.rs::validate 同口径）──
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const badId = (id) => typeof id !== "string" || !id || id !== id.trim() || /[\s\u0000-\u001f]/.test(id);
function validate(c, label) {
  const err = (m) => fail(`[${label}] ${m}`);
  if (c.catalog !== "xiapan-models") err(`catalog 应为 "xiapan-models"，实际是 ${JSON.stringify(c.catalog)}`);
  if (!Number.isInteger(c.version) || c.version < 1) err("version 必须是 ≥1 的整数");
  if (typeof c.updated !== "string" || !DATE_RE.test(c.updated)) err(`updated 应为 YYYY-MM-DD，实际是 ${JSON.stringify(c.updated)}`);
  if (!Array.isArray(c.groups) || c.groups.length === 0) {
    err("groups 为空");
    return new Set();
  }
  const seen = new Set();
  for (const g of c.groups) {
    if (typeof g.group !== "string" || !g.group.trim()) err("存在空名字的分组");
    if (g.pricey !== undefined && typeof g.pricey !== "boolean") err(`分组「${g.group}」的 pricey 必须是布尔值`);
    if (!Array.isArray(g.items) || g.items.length === 0) {
      err(`分组「${g.group}」没有任何条目`);
      continue;
    }
    for (const it of g.items) {
      if (badId(it.id)) {
        err(`非法的模型 id ${JSON.stringify(it.id)}（空、带空白或控制字符）`);
        continue;
      }
      if (seen.has(it.id)) err(`模型 id 重复：${it.id}`);
      seen.add(it.id);
      if (typeof it.label !== "string" || !it.label.trim()) err(`${it.id} 缺 label`);
      if (it.recommend !== undefined && typeof it.recommend !== "boolean") err(`${it.id} 的 recommend 必须是布尔值`);
      if (!Array.isArray(it.input) || it.input.length === 0) err(`${it.id} 的 input 为空或缺失`);
      else for (const m of it.input) if (m !== "text" && m !== "image") err(`${it.id} 的 input 含不认识的模态 ${JSON.stringify(m)}（只许 text / image）`);
      // 对话模型的 input 必须是逐个真请求核实过的 —— 没有核实日期就等于没核实
      if (typeof it.verified !== "string" || !DATE_RE.test(it.verified)) err(`${it.id} 缺 verified（YYYY-MM-DD）—— 没核实过的不许进目录`);
    }
  }
  for (const k of ["default", "strong"]) if (!seen.has(c[k])) err(`${k} 指向的模型 ${JSON.stringify(c[k])} 不在条目里`);
  const seenImg = new Set();
  for (const m of c.image_models ?? []) {
    if (badId(m.id)) {
      err(`非法的作图模型 id ${JSON.stringify(m.id)}`);
      continue;
    }
    if (seenImg.has(m.id)) err(`作图模型 id 重复：${m.id}`);
    seenImg.add(m.id);
    if (typeof m.label !== "string" || !m.label.trim()) err(`作图模型 ${m.id} 缺 label`);
    if (typeof m.edits !== "boolean") err(`作图模型 ${m.id} 的 edits 必须显式写 true/false（没实测过不许默认成 true）`);
    if (m.verified !== undefined && m.verified !== "" && !DATE_RE.test(m.verified)) err(`作图模型 ${m.id} 的 verified 要么留空要么 YYYY-MM-DD`);
  }
  return seen;
}
const idsA = validate(a, "内嵌");
validate(b, "热下发");

// ── ④ 前端不许再手写一份对话模型清单 ──
const ts = readFileSync(MODELS_TS, "utf8");
const tsCode = ts.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, ""); // 去掉注释再看代码
if (!/import\s+catalogJson\s+from\s+"\.\.\/\.\.\/src-tauri\/models\/xiapan-models\.json"/.test(tsCode)) {
  fail(`${MODELS_TS} 没有 import 内嵌目录 JSON（"../../src-tauri/models/xiapan-models.json"）—— XIAPAN_MODELS 的来源变了？`);
}
const decl = tsCode.match(/export const XIAPAN_MODELS[^=]*=\s*([^;]*);/);
if (!decl) {
  fail(`${MODELS_TS} 里找不到 \`export const XIAPAN_MODELS = …;\``);
} else if (!/^parseModelGroups\(/.test(decl[1].trim()) || /\bid\s*:/.test(decl[1])) {
  fail(`${MODELS_TS} 的 XIAPAN_MODELS 必须由 parseModelGroups(目录 JSON) 生成，不许手写条目。当前：${decl[1].trim().slice(0, 100)}`);
}
for (const id of idsA) {
  const hand = new RegExp(`\\bid\\s*:\\s*["'\`]${id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}["'\`]`);
  if (hand.test(tsCode)) fail(`${MODELS_TS} 里出现了手写的模型条目 { id: "${id}" … } —— 对话模型清单只能来自目录 JSON`);
}

// ── ⑤ 镜像顺序与 installer.rs 一致 ──
const rs = readFileSync(CATALOG_RS, "utf8");
const inst = readFileSync(INSTALLER_RS, "utf8");
const arrayOf = (src, name) => {
  const m = src.match(new RegExp(`const ${name}:\\s*&\\[&str\\]\\s*=\\s*&\\[([\\s\\S]*?)\\];`));
  return m ? [...m[1].matchAll(/"(https?:\/\/[^"]+)"/g)].map((x) => x[1]) : null;
};
const catalogUrls = arrayOf(rs, "CATALOG_URLS");
const skillUrls = arrayOf(inst, "SKILL_URLS");
if (!catalogUrls || !skillUrls) {
  fail("读不出 model_catalog.rs::CATALOG_URLS 或 installer.rs::SKILL_URLS —— 常量改名了？");
} else {
  const expected = skillUrls.map((u) => u.replace(/install-windows\.json$/, "xiapan-models.json"));
  if (JSON.stringify(expected) !== JSON.stringify(catalogUrls)) {
    fail(
      `镜像地址与 SKILL_URLS 不一致（顺序/域名/路径）。\n   期望：${expected.join(" , ")}\n   实际：${catalogUrls.join(" , ")}`,
    );
  }
}

if (problems.length) {
  console.error(`\n❌ ${problems.length} 条不达标：`);
  for (const p of problems) console.error("  - " + p);
  process.exit(1);
}
console.log(`\n✅ 目录两份一致（除 version）、热下发 ≥ 内嵌、schema 合法（${idsA.size} 个对话模型 + ${(a.image_models ?? []).length} 个作图模型）、前端没有手写清单、镜像顺序与装机清单一致。`);
