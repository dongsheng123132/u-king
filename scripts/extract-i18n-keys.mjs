/**
 * 抽出某个组件里所有 `t("…")` 的中文 key，用来补 `src/i18n/en/*.ts`。
 *
 * 用法：`node scripts/extract-i18n-keys.mjs src/Identity.tsx`
 *
 * 本项目 i18n 是「中文即 key」：漏翻的会在运行时静默回退中文 —— 好处是永远不崩，
 * 坏处是**漏了没人知道**。加新页面时跑一下这个，对着输出补 en 字典。
 */
import fs from "fs";

const file = process.argv[2];
if (!file) {
  console.error("用法: node scripts/extract-i18n-keys.mjs <组件路径>");
  process.exit(2);
}

const src = fs.readFileSync(file, "utf8");
const keys = new Set();
// 只抓双引号形式的 t("…")；本项目没有用单引号或模板串调 t 的写法。
const re = /\bt\(\s*"([^"]*)"/g;
let m;
while ((m = re.exec(src))) keys.add(m[1]);

for (const k of keys) console.log(JSON.stringify(k) + ": " + JSON.stringify(k) + ",");
console.error(`--- ${file}: ${keys.size} 条 ---`);
