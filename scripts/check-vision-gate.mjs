/**
 * 「别把图发给纯文本模型」这道闸门的跑道 —— 吃 `see-image.mjs` 的真源码，不另抄一份实现。
 *
 * ## 它盯的是什么
 * 纯文本模型收下 `image_url` **不报错**：HTTP 200、`choices` 齐全，正文是一个**编出来的答案**
 * （2026-08-16 实测：问合成营业执照上的法定代表人，正解「张示例」，`qwen-turbo` / `qwen-plus`
 * 都答「张三」）。这类失败没有任何异常信号，下游拿去当识图结果用，比报错难查得多。
 *
 * 闸门有两源，这条跑道分别钉死：
 *  ① 本脚本内置的手写清单（正则）—— 覆盖 catalog 里压根没有的 `qwen-plus` / `qwen-turbo`；
 *  ② dsh 的 `@earendil-works/pi-ai` catalog（运行时查）—— 覆盖手写清单想不到的那 190 多个。
 *
 * ## 为什么必须两档跑（这就是本跑道的变异验证）
 * 同一份源码跑两遍，只改「catalog 在不在」：
 *  - `catalog-on` ：catalog 独有的纯文本模型**必须被拦**；
 *  - `catalog-off`：同一批**必须放行**（回到只剩手写清单的状态）。
 * 两档结果若相同，说明第二源根本没生效 —— 那 `catalog-on` 那档的绿是假的。
 *
 * ## 三条不许违反的（对应 see-image.mjs 里那三条铁律）
 *  1. **catalog 只用来加拦，不用来放行**：我们的默认 `qwen3.7-flash` 压根不在 catalog 里，
 *     拿「不在里面」当纯文本的证据会把主力路径当场拦死。两档都断言它放行。
 *  2. **同一裸名有矛盾条目时放行**：559 个裸名里 5 个自相矛盾（如 `qwen3.6-plus` img6/txt1）。
 *  3. **catalog 出任何问题都当「不知道」**：dsh 没装（= 本跑道的 catalog-off 档）必须照常工作。
 *
 * **验不到**：模型实际会不会编答案 —— 那要真花钱调用，且答案随问法变。这条只钉「该拦的拦住了」。
 *
 * 用法：`node scripts/check-vision-gate.mjs`（不联网、不花额度、毫秒级）
 */
import { readFileSync, existsSync, readdirSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { homedir, tmpdir } from "node:os";

const SEE = "src-tauri/skills/vision/scripts/see-image.mjs";
const fails = [];

// ── 从真源码里切出闸门那一段 ──
// 抄一份到跑道里最省事，但那样跑道验的是**副本**，源码改了副本不会跟着改 —— 等于没验。
const src = readFileSync(SEE, "utf8");
const from = src.indexOf("const TEXT_ONLY");
const toLine = src.indexOf("const textOnlyModel");
if (from < 0 || toLine < 0) {
  console.error("❌ 在 see-image.mjs 里找不到闸门那一段（`const TEXT_ONLY` … `const textOnlyModel`）——");
  console.error("   跑道过期了，去看那个文件是不是重构过。**不要**把断言注释掉了事。");
  process.exit(1);
}
const section = src.slice(from, src.indexOf("\n", toLine));

/** 把那一段放进给定环境里跑起来，返回闸门函数。`fakeHome` 用来制造「dsh 没装」。 */
function gateWith(fakeHome) {
  const factory = new Function(
    "existsSync", "readdirSync", "readFileSync", "join", "homedir",
    `${section}\nreturn { textOnlyModel, textOnlyRegex, catalogSaysTextOnly, loadCatalog };`,
  );
  return factory(existsSync, readdirSync, readFileSync, join, () => fakeHome);
}

const REAL_HOME = homedir();
const EMPTY_HOME = mkdtempSync(join(tmpdir(), "uking-no-dsh-"));

const on = gateWith(REAL_HOME);
const off = gateWith(EMPTY_HOME);

const catalogSize = on.loadCatalog().size;
console.log(`catalog: ${catalogSize} 个裸名${catalogSize ? "" : "（🔴 本机没装 dsh —— 第 ② 源这次验不到，见末尾）"}`);

// ── ① 手写清单：两档都必须拦（它不依赖 catalog）──
const BY_REGEX = ["qwen-plus", "qwen-turbo", "qwen3.7-max", "qwen3-coder-plus",
  "deepseek-v3.2", "deepseek-chat", "glm-5", "glm-5.1", "glm-5.2", "minimax-m1", "minimax-m2"];
console.log("\n[1/4] 手写清单点名的，两档都必须拦…");
for (const id of BY_REGEX) {
  if (!on.textOnlyModel(id)) fails.push(`[清单] ${id} 没被拦（catalog-on）`);
  if (!off.textOnlyModel(id)) fails.push(`[清单] ${id} 没被拦（catalog-off）—— 手写清单不该依赖 catalog`);
}
if (!fails.length) console.log(`     ✓ ${BY_REGEX.length} 个全拦住，且不依赖 catalog`);

// ── ② 会看图的：两档都必须放行（拦错 = 主力路径当场死）──
const VISION = ["qwen3.7-flash", "qwen3.7-plus", "qwen3-vl-flash", "qwen3-vl-plus", "qwen-vl-max",
  "kimi-k3", "z-ai/glm-5v-turbo", "gemini-3.5-flash", "claude-sonnet-5", "gpt-5.4", "deepseek-ocr"];
console.log("[2/4] 会看图的，两档都必须放行…");
{
  const before = fails.length;
  for (const id of VISION) {
    if (on.textOnlyModel(id)) fails.push(`🔴 [误拦] ${id} 被当成纯文本拦掉了（catalog-on）—— 这会让看图整个用不了`);
    if (off.textOnlyModel(id)) fails.push(`🔴 [误拦] ${id} 被拦（catalog-off）`);
  }
  if (fails.length === before) console.log(`     ✓ ${VISION.length} 个全放行（含默认 qwen3.7-flash：它不在 catalog 里，"不在里面" 没被当成证据）`);
}

// ── ③ catalog 独有的：on 必须拦、off 必须放行 ← 这一对就是变异验证 ──
console.log("[3/4] 变异验证：catalog 独有的纯文本模型，开 catalog 必拦 / 关 catalog 必放…");
if (!catalogSize) {
  console.log("     ⏭ 本机没装 dsh，跳过（末尾会算作「没验到」，不算绿）");
} else {
  // 从真 catalog 里现挑：手写清单拦不住、但 catalog 说它每一条都不收图的
  const idx = on.loadCatalog();
  const picks = [];
  for (const [id, v] of idx) {
    if (picks.length >= 8) break;
    if (v.txt > 0 && v.img === 0 && !on.textOnlyRegex(id) && id.length > 6) picks.push(id);
  }
  if (!picks.length) {
    fails.push("[catalog] 一个「catalog 独有」的样本都挑不出来 —— 第 ② 源等于没接上，或 catalog 结构变了");
  } else {
    const before = fails.length;
    for (const id of picks) {
      if (!on.textOnlyModel(id)) fails.push(`[catalog] ${id} 开着 catalog 也没拦住 —— 第 ② 源没生效`);
      if (off.textOnlyModel(id)) fails.push(`[catalog] ${id} 关了 catalog 还被拦 —— 那它其实是被手写清单拦的，这条样本不算数`);
    }
    if (fails.length === before)
      console.log(`     ✓ ${picks.length} 个（如 ${picks.slice(0, 3).join(", ")}）：开 catalog 拦住、关 catalog 放行 —— 两档不同，第 ② 源确实在干活`);
  }
}

// ── ④ 铁律 2：裸名自相矛盾时必须放行（宁可漏不可误杀）──
console.log("[4/4] 同一裸名有矛盾条目时必须放行…");
if (!catalogSize) {
  console.log("     ⏭ 同上，跳过");
} else {
  const idx = on.loadCatalog();
  const conflicts = [...idx].filter(([id, v]) => v.img > 0 && v.txt > 0 && !on.textOnlyRegex(id));
  const before = fails.length;
  for (const [id] of conflicts.slice(0, 10)) {
    if (on.catalogSaysTextOnly(id)) fails.push(`[矛盾] ${id} 有条目说收图却仍被 catalog 判成纯文本 —— 违反「宁可漏不可误杀」`);
  }
  if (fails.length === before)
    console.log(`     ✓ ${conflicts.length} 个矛盾裸名全部放行${conflicts.length ? `（如 ${conflicts.slice(0, 2).map(([i]) => i).join(", ")}）` : ""}`);
}

if (fails.length) {
  console.error(`\n❌ ${fails.length} 条不达标：`);
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
if (!catalogSize) {
  console.error("\n❌ 本机没装 dsh，第 ② 源（catalog）这次一条都没验到 —— 「跳过」不是「通过」。");
  console.error("   要验它：装 dsh，或在装了的机器上跑一遍。");
  process.exit(1);
}
console.log("\n✅ 闸门两源都在干活：手写清单不依赖 catalog，catalog 确实多拦了一批，且两条「宁可漏不可误杀」都守住了");
