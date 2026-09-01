/**
 * 找出 `t("…")` 里有、但 `src/i18n/en/*.ts` 没翻的中文 key。
 *
 * 为什么要有这个：本项目 i18n 是「中文即 key」，漏翻**静默回退中文** ——
 * 好处是永远不崩，坏处是**漏了没人知道**：tsc 全绿、build 全绿、界面也不报错，
 * 只有把语言切成 English 的那位客户会看到半屏中文。
 * `extract-i18n-keys.mjs` 只负责「列出某个文件的 key」，还得靠人去对；这个直接给差集。
 *
 * 用法：
 *   node scripts/check-i18n-missing.mjs                 # 全量扫 src/
 *   node scripts/check-i18n-missing.mjs src/opencodex   # 只扫某个目录/文件
 *
 * 退出码：0 = 没漏；1 = 有漏（可进 CI / 发版前检查）。
 *
 * ⚠️ **只认字面量 `t("…")`，它的绿灯≠翻全了。** `t(label)` 这种把变量传进去的
 * （起手词 21 组、驱动名、模型清单、`+` 菜单项都是这样）**一条都抓不到** ——
 * 那些 key 的真身在数据表里（QuickPrompts 的 SCENES / ENGINES / MODES / XIAPAN_MODELS …）。
 * 2026-08-04 就是这样：本脚本报「opencodex 没有漏翻」，而英文界面里起手词那两排
 * 仍是整排中文。
 *
 * ⇒ **配套的动态检查**（真正兜底的那半）：把组件挂进探针、`localStorage.uking.lang="en"`，
 *   然后用 **TreeWalker 扫 text node** 找残留汉字，外加 `option/optgroup` 的 label 和
 *   `[title]/[placeholder]`：
 *     const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
 *   🔴 **别按元素过滤**（`el.children.length === 0` 那种）：带 `<svg>` 图标的按钮，
 *   文字是按钮自己的 text node、`children.length` 不为 0，整条会被跳过 —— 第一版就是
 *   这么把 7 个起手词漏报成「干净」的。还要**逐个 tab / 展开每个菜单**再扫，
 *   没渲染出来的那一屏永远是「0 残留」。
 */
import fs from "fs";
import path from "path";

const ROOT = process.argv[2] || "src";
const EN_DIR = "src/i18n/en";

/** 递归收集 .ts/.tsx（跳过 i18n 自己和 vendor 的第三方内核）。 */
function walk(p, out = []) {
  const st = fs.statSync(p);
  if (st.isFile()) {
    if (/\.tsx?$/.test(p) && !p.includes(`i18n${path.sep}`)) out.push(p);
    return out;
  }
  for (const name of fs.readdirSync(p)) {
    if (name === "i18n" || name === "vendor" || name === "generated") continue;
    walk(path.join(p, name), out);
  }
  return out;
}

// 词典整体当文本读：只要 key 以 `"…":` 的形式出现过就算翻过。
// 不 import 是因为这些是 .ts，node 直接跑不了，而为一次检查引 ts-node 不值当。
const dict = fs
  .readdirSync(EN_DIR)
  .filter((f) => f.endsWith(".ts"))
  .map((f) => fs.readFileSync(path.join(EN_DIR, f), "utf8"))
  .join("\n");

const missing = new Map(); // key -> [文件…]
for (const file of walk(ROOT)) {
  const src = fs.readFileSync(file, "utf8");
  // 🔴 别只认 `t(`。有些文件把它改了名：`const { t: tr } = useI18n()`（TermPanel.tsx 就是），
  // 而 `\bt\(` 匹配不到 `tr(` —— 于是**整个文件对这条闸门隐形**，它照样报绿。
  // 2026-08-16 实测：TermPanel 里那批 `tr("…")` 从来没被扫过。这里按文件自动认出别名。
  const aliases = new Set(["t"]);
  for (const a of src.matchAll(/\{\s*t\s*:\s*([A-Za-z_$][\w$]*)\s*[,}]/g)) aliases.add(a[1]);
  const re = new RegExp(`\\b(?:${[...aliases].join("|")})\\(\\s*"((?:[^"\\\\]|\\\\.)*)"`, "g");
  let m;
  while ((m = re.exec(src))) {
    const key = m[1];
    if (!key || !/[一-龥]/.test(key)) continue; // 没有汉字的不用翻
    if (dict.includes(`"${key}":`)) continue;
    if (!missing.has(key)) missing.set(key, []);
    const list = missing.get(key);
    if (!list.includes(file)) list.push(file);
  }
}

if (missing.size === 0) {
  console.error(`--- ${ROOT}: 字面量 t("…") 没有漏翻 ---`);
  process.exit(0);
}
// stdout 只出可直接粘进 en/*.ts 的行；文件归属走 stderr，别污染管道
for (const [key] of missing) console.log(JSON.stringify(key) + ": " + JSON.stringify(key) + ",");
console.error(`--- ${ROOT}: 漏翻 ${missing.size} 条 ---`);
for (const [key, files] of missing) console.error(`  ${files.join(", ")}  ←  ${key.slice(0, 40)}`);

/**
 * 棘轮：判**涨没涨**，不判有没有。
 *
 * 2026-08-16 把别名（`const { t: tr }`）也扫进来之后，一次性冒出 163 条**存量**漏翻 ——
 * 它们一直都在，只是这条闸门看不见。这时候直接判红会把所有人的 build 一起打死，
 * 而「先记着以后再说」又等于没有闸门。所以照 `check-budget.mjs` 的老办法：
 * 存量记在基线里、不阻断；**新增一条就红**；修好了自动收紧基线（只降不升）。
 * 基线只能靠**真去翻**来降 —— 不许手改。
 */
const BASELINE_FILE = "scripts/i18n-missing-baseline.json";
// 🔴 **只有全量扫描才准碰基线。** 加参数跑（`… src/LocalLLM.tsx`）数的是那一个文件，
// 拿它去跟全仓基线比毫无意义 —— 2026-08-19 实测：给单个新文件跑了一次，22 < 161，
// 棘轮当成「修好了 139 条」把基线收紧到 22；下一次 `pnpm build` 全量一扫立刻
// 「22 → 161 漏翻涨了」，红得莫名其妙。而这种红最自然的「修法」是手抬基线，
// 一抬就把真实的存量债洗掉了 —— 一条会自己骗自己的闸门比没有闸门更坏。
const FULL_SCAN = ROOT === "src";
let baseline = 0;
try {
  baseline = JSON.parse(fs.readFileSync(BASELINE_FILE, "utf8")).missing ?? 0;
} catch {
  /* 没基线就当 0：第一次跑必然要么全绿要么留下基线 */
}
if (!FULL_SCAN) {
  console.error(
    `\n（只扫了 ${ROOT}：${missing.size} 条漏翻。基线是全仓口径，这次不比也不改 —— 要判红绿请不带参数跑全量。）`,
  );
  process.exit(0);
}
if (missing.size > baseline) {
  console.error(`\n❌ 漏翻涨了：${baseline} → ${missing.size}。新加的文案请补进 src/i18n/en/*.ts`);
  process.exit(1);
}
if (missing.size < baseline) {
  fs.writeFileSync(BASELINE_FILE, JSON.stringify({ missing: missing.size }, null, 2) + "\n");
  console.error(`\n✅ 漏翻降了：${baseline} → ${missing.size}，基线已收紧`);
} else {
  console.error(`\n⚠️ ${missing.size} 条存量漏翻（= 基线，不阻断）。这些是别名扫描补上后才露出来的历史欠账。`);
}
process.exit(0);
