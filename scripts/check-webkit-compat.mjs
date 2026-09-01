/**
 * 「Mac 上打得开吗」的静态跑道 —— 禁掉老 WebKit 会当场语法错的正则写法。
 *
 * ## 为什么必须单开一条
 * 2026-08-16：`fileLinks.ts` 里两条 `(?<!…)` lookbehind 把 **0.9.99 / 0.9.100 的 Mac 版整个打白屏**。
 * Windows 的 WebView2 是 Chromium（Chrome 62 起支持 lookbehind），一点事没有；
 * Mac 的 WKWebView 是 JavaScriptCore，**Safari 16.4（macOS 13.3，2023-03）才支持**，
 * 之前的版本连**解析**都过不去：`SyntaxError: Invalid regular expression: invalid group specifier name`。
 * 那两条正则又是**模块顶层求值**、模块又进了主入口 chunk → 不是「终端路径点不动」，是**首屏起不来**。
 *
 * **现有跑道结构性照不到这一类**：
 *  - `check-term-file-links.mjs` 两层跑的是 Node 22 + Playwright **Chromium**；
 *  - `tsc` / `vite build` 也不管 —— esbuild 只对**正则字面量**做目标检查，
 *    而我们是 `new RegExp("…")` 拼字符串，它看不见。
 * 所以只有「读源码找字符串」这一招能守。判据是静态的、毫秒级、零假绿。
 *
 * ## 只禁真会炸的
 * lookbehind `(?<=` / `(?<!`。命名捕获组 `(?<name>` 是 Safari 11.1 就有的，不禁。 webkit-compat-ok
 * 正则 `d` 标志（match indices，同样 16.4）静态判不准，不做 —— 宁可漏也不制造假红。
 *
 * 用法：`node scripts/check-webkit-compat.mjs`（`pnpm build` 里自动跑，构建完连 dist 一起查）
 */
import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { join } from "node:path";

/** `(?<` 后面跟 `=` 或 `!` 才是 lookbehind；跟标识符是命名组，放行。 */
const LOOKBEHIND = /\(\?<[=!]/g;
const SRC_EXT = /\.(ts|tsx|js|jsx|mjs|cjs)$/;

/**
 * 唯一的豁免口子：同一行写上这个词就放行。
 *
 * 故意**不去剥注释** —— 剥注释的失败方向是**假阴性**（字符串里藏一段像注释的东西就漏掉），
 * 而这条守卫漏一次的代价是 Mac 版整个白屏。留一个显式的、grep 得出来的口子，
 * 给「文档里要引用这个写法」这类场合用（本文件和 fileLinks.ts 的说明就靠它）。
 */
const ALLOW = "webkit-compat-ok";

const hits = [];

function scanFile(path, rel) {
  const text = readFileSync(path, "utf8");
  const lineStarts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") lineStarts.push(i + 1);

  LOOKBEHIND.lastIndex = 0;
  let m;
  while ((m = LOOKBEHIND.exec(text)) !== null) {
    let lo = 0;
    let hi = lineStarts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (lineStarts[mid] <= m.index) lo = mid;
      else hi = mid - 1;
    }
    const lineNo = lo + 1;
    const lineEnd = lineStarts[lo + 1] ?? text.length;
    const lineText = text.slice(lineStarts[lo], lineEnd);
    if (lineText.includes(ALLOW)) continue;
    // 压缩产物一行几万字符 —— 片段取**命中点前后**，不是行首（取行首只会印到无关的东西）
    const snippet = text.slice(Math.max(0, m.index - 40), m.index + 60).replace(/\n/g, "⏎");
    hits.push({ where: `${rel}:${lineNo}`, snippet });
  }
}

function walk(dir, rel, accept) {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === ".git") continue;
    const full = join(dir, name);
    const r = rel ? `${rel}/${name}` : name;
    if (statSync(full).isDirectory()) walk(full, r, accept);
    else if (accept(name)) scanFile(full, r);
  }
}

// ① 我们自己的源码 —— 一条都不许有
walk("src", "src", (n) => SRC_EXT.test(n));

// ①.5 index.html 的内联兜底脚本 —— 它是给「新语法把主包干掉了」兜底的那一段，
//      自己用了老引擎不认的写法就一起死，比主包里的更不能出错
scanFile("index.html", "index.html");

// ② 构建产物 —— 顺手把依赖带进来的也照出来（dist 不在时跳过：build 前跑就只有第①层）
const distAssets = join("dist", "assets");
const scannedDist = existsSync(distAssets);
if (scannedDist) walk(distAssets, "dist/assets", (n) => n.endsWith(".js"));

if (hits.length) {
  console.error(`\n❌ ${hits.length} 处正则 lookbehind —— 老 WebKit（macOS < 13.3）会白屏：`);
  for (const h of hits) console.error(`  - ${h.where}\n      ${h.snippet}`);
  console.error(`\n改法：把「前面那个字符」吃进来当捕获组，起始下标 = m.index + m[1].length。`);
  console.error(`      范例见 src/opencodex/term/fileLinks.ts 的 PATTERNS。\n`);
  process.exit(1);
}

console.log(`✓ 无正则 lookbehind（src${scannedDist ? " + dist/assets" : "，dist 未构建，跳过"}）`);
