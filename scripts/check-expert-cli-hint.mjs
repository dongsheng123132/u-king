#!/usr/bin/env node
/**
 * 闸门：专家 persona 里不许再出现**裸命令名** `u-king-mini`。
 *
 * 🔴 为什么需要一个闸门而不是「注意一下」：
 * U-King 自己的 exe **不在 PATH 上**（2026-08-22 实测：`command -v u-king-mini` 落空，
 * 带 `.exe` 也落空；`installer.rs::search_paths()` 注入的是 node/git/npm/python 的目录，
 * 从来不含我们自己）。可是 persona 是给 AI 的 system prompt，写了裸名，AI 的第一条命令
 * 就必然 command not found —— 而**开发机上永远看不到这件事**：我们自己那台装了 U-King，
 * 人一手就补上绝对路径，从没人意识到 AI 拿到的提示是错的。
 *
 * 这条错误在 `experts.ts` 里独立长出来过**三次**（AI 优化专家 / 省钱专家 / 装机医生），
 * 每一次都是照着上一个专家抄的。同一事实存三份就漂三份（宪法第 8 条）——
 * 现在只留一份 `UKING_CLI_HINT`，其余地方一律用占位符 `<UK>`。
 *
 * 真相源是 `~/.uking/llms.txt`（开机自动生成，第一段就是带引号的绝对路径）。
 * 🔴 **闸门故意不检查「路径写对没有」** —— 因为 persona 里根本不该出现任何路径：
 * 装到哪由安装器决定（下载版 / U 盘版 / Mac .app 各不同），写死一条就是第四份会漂的副本。
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const file = join(root, "src", "opencodex", "experts.ts");
const src = readFileSync(file, "utf8");

/** 允许出现裸名的唯一位置：那份共享常量自己（它正是用来解释「别用裸名」的）。 */
const HINT_START = src.indexOf("const UKING_CLI_HINT");
const HINT_END = HINT_START < 0 ? -1 : src.indexOf("\nconst BASE_SYSTEM", HINT_START);
if (HINT_START < 0 || HINT_END < 0) {
  console.error("✗ experts.ts 里找不到 UKING_CLI_HINT —— 它被删了或改名了。");
  console.error("  它是「怎么调本机 exe」这件事的唯一真相源，删掉等于让三个专家各自再抄一份。");
  process.exit(1);
}

const offenders = [];
const lines = src.split("\n");
let offset = 0;
for (let i = 0; i < lines.length; i++) {
  const line = lines[i];
  const at = offset;
  offset += line.length + 1;
  // 常量自己那一段跳过；注释行跳过（注释正是在解释这件事）。
  if (at >= HINT_START && at <= HINT_END) continue;
  const trimmed = line.trim();
  if (trimmed.startsWith("*") || trimmed.startsWith("//") || trimmed.startsWith("/*")) continue;
  // 裸名后面跟着 action/子命令 = 在教 AI 怎么调；`<UK>` 才是对的写法。
  if (/[`\s"']u-king-mini(\.exe)?[`\s"']*\s+action\b/.test(line)) {
    offenders.push({ n: i + 1, line: trimmed });
  }
}

if (offenders.length) {
  console.error("✗ 专家 persona 里出现了裸命令名 `u-king-mini` —— 它不在 PATH 上，AI 照着打必然 command not found。");
  console.error("  改法：用占位符 `<UK>`，并在 persona 开头拼上共享常量 UKING_CLI_HINT（它教 AI 去 ~/.uking/llms.txt 取绝对路径）。");
  for (const o of offenders) console.error(`  ${file}:${o.n}  ${o.line.slice(0, 110)}`);
  process.exit(1);
}

// 反向断言：用了 `<UK>` 占位符的 persona，必须真的把 UKING_CLI_HINT 拼进去，
// 否则 AI 拿到一个从没被解释过的 `<UK>` —— 比裸名更糟（它会当成字面量）。
const usesPlaceholder = (src.match(/<UK>/g) || []).length;
const includesHint = (src.match(/UKING_CLI_HINT/g) || []).length - 1; // 减去定义那一处
if (usesPlaceholder > 0 && includesHint < 1) {
  console.error("✗ persona 里用了 `<UK>` 占位符，却没有任何一个 persona 拼上 UKING_CLI_HINT。");
  console.error("  AI 会把 `<UK>` 当字面量原样敲进命令行。");
  process.exit(1);
}

console.log(`✓ 专家 persona 无裸命令名（${includesHint} 个 persona 挂了 UKING_CLI_HINT，${usesPlaceholder} 处占位符）`);
