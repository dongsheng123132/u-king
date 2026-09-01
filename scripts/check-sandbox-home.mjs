/**
 * 「会删东西的模块，家目录必须认沙箱」—— 棘轮式静态闸门。
 *
 * ## 立项理由（2026-08-18，当天真的炸了一次）
 * `skillpack.rs` 的 `home_dir()` 自己读 `USERPROFILE`、不认 `UKING_TEST_HOME`。
 * 该模块**只有装、没有拆**的那段时间里，这个缺陷完全潜伏：写进真实目录本来就是它的活。
 * 当天给它加了第一个 `remove_dir_all`（按包卸载技能）之后，一条**本该跑在沙箱里的单测**
 * 把开发机上真实的 `~/.claude|.codex|.agents/skills/uking-workbench` 三份全删了。
 *
 * 🔴 **教训不是「测试写错了」**，是：
 * > 一个不认沙箱的家目录实现，在模块只读/只写自己目录时是潜伏的，
 * > **加第一个删除操作时才引爆** —— 而那时炸的是开发者（或客户）的真实数据。
 *
 * `identity.rs:50` 记着一模一样的一笔（「别家 AI 记忆文件统统逃出沙箱，一次隔离测试
 * 能改到用户的真实 CLAUDE.md」）。同一个坑踩第二次，说明靠注释和记性不够，得有闸门。
 *
 * ## 判据
 * 一个 `.rs` 文件同时满足下面两条就算「危险」：
 *  1. 自己读 `USERPROFILE` / `HOME` 拼家目录（而不是走 `installer::user_home_dir()`）
 *  2. 文件里有 `remove_dir_all` / `remove_file`（**真删**）
 * 反过来，只要它引用了 `UKING_TEST_HOME` 或 `user_home_dir`，就认为它知道沙箱这回事，放行。
 *
 * ## 为什么是棘轮而不是硬闸
 * 立项当天存量就有 14 个模块命中。一个第一天就红的闸门，第二天就会被绕过
 * （`check-budget.mjs` 那条注释说的是同一件事）。所以：**存量记进基线不阻断，
 * 新增或变多即红**。修好一个就自动收紧，回不去。
 *
 * 用法：node scripts/check-sandbox-home.mjs        （超基线退出码 1）
 *      node scripts/check-sandbox-home.mjs --accept "理由"   （确实该涨时留名）
 */
import { readFileSync, writeFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const SRC = "src-tauri/src";
const BASELINE = "scripts/sandbox-home-baseline.json";

/**
 * 直接从环境变量拼用户目录。
 *
 * 🔴 `LOCALAPPDATA` / `APPDATA` 是 2026-08-18 加的，代价是一次真删：
 * `skillpack.rs` 的 `home_dir()` 当天已经改成走公共层了，但同文件的
 * `legacy_skill_parents()` 还在直接读 `LOCALAPPDATA` —— 一次「沙箱内」的卸载
 * 把开发机上 `%LOCALAPPDATA%\hermes\skills\aigc\uking-ppt` 真删了。
 * **认沙箱不能只认一个 env。**
 */
const READS_HOME = /var\(\s*"(USERPROFILE|HOME|LOCALAPPDATA|APPDATA)"\s*\)/;
/** 真删。**只认删**：`fs::write` 那类覆盖也危险，但删是不可逆的，先把最贵的一档钉死。 */
const DESTRUCTIVE = /remove_dir_all|remove_file/;
/**
 * ~~知道沙箱这回事的证据~~ —— **这条豁免已经取消**。
 *
 * 它原本是「整份文件里只要出现过 UKING_TEST_HOME / user_home_dir 就放行」。
 * 那是按**文件**判的，而危险是按**读取点**发生的：`skillpack.rs` 修好 `home_dir()`
 * 之后整份文件就"合格"了，同文件里第二处直接读 LOCALAPPDATA 照样把真实目录删掉。
 *
 * 现在的规则更笨也更硬：**会删东西的文件里，一律不许直接从 env 拼用户目录**，
 * 走 `installer::user_home_dir()`（或显式先查 UKING_TEST_HOME）。
 * 存量全部进基线不阻断，新增即红。
 */

/**
 * 剥掉注释和字符串字面量再判。
 *
 * 🔴 不剥的话闸门会被**自己的注释**骗过去 —— 立项当天的变异验证当场抓到：
 * 把 `skillpack.rs` 改回自己读 USERPROFILE，它照样放行，因为该函数的文档注释里
 * 写着「它认 `UKING_TEST_HOME`」这几个字。**判据看的必须是代码，不是关于代码的话。**
 * （同 `check-i18n-missing` 那类扫描器的老问题：正则不懂语法，得先把非代码剔掉。）
 */
/**
 * 逐字符剥注释。**不能用正则** —— 2026-08-18 一份黑盒测试报告用变异测试把旧实现打穿了：
 *
 *   · 行注释规则 `//.*$` 会把 `"https://x"` 里的 `//` 当成注释起点，从那儿到行尾整段抹掉。
 *     实测 `format!("https://a/{}", std::env::var("USERPROFILE"))` 剥完只剩
 *     `let x = format!("https:` ——**那处裸读就此对闸门隐形，绿灯放行**。
 *   · 块注释规则更狠：字符串里任何一个 `/*`（glob "skills/*"、正则、路径通配）
 *     会一路吃到全文件下一个 `*` `/`，中间跨多少行、藏多少处裸读全没了。同样实测验证过。
 *
 * 所以得维护「现在在不在字符串里」这个状态，只有不在字符串里才认注释起点。
 * 认三种 Rust 字面量：普通串（带反斜杠转义）、裸串 r"..." / r#"..."#（没有转义，
 * 收尾是引号加等量的井号）、字符字面量。
 * 保留字符串内容不动 —— 判据本身就长在字符串里（env::var 的参数）。
 */
function stripComments(src) {
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    // 裸字符串 r"..." / r#"..."# / br#"..."#：内部无转义，只认「引号 + 等量 #」收尾
    if (c === "r" || c === "b") {
      let j = i;
      if (src[j] === "b") j++;
      if (src[j] === "r") {
        j++;
        let hashes = 0;
        while (src[j] === "#") {
          hashes++;
          j++;
        }
        if (src[j] === '"') {
          const close = '"' + "#".repeat(hashes);
          const end = src.indexOf(close, j + 1);
          const stop = end === -1 ? n : end + close.length;
          out += src.slice(i, stop);
          i = stop;
          continue;
        }
      }
    }
    if (c === '"') {
      let j = i + 1;
      while (j < n) {
        if (src[j] === "\\") {
          j += 2;
          continue;
        }
        if (src[j] === '"') {
          j++;
          break;
        }
        j++;
      }
      out += src.slice(i, j);
      i = j;
      continue;
    }
    // 字符字面量。**生命周期 'a 不是字面量** —— 它后面没有收尾引号，
    // 当字面量吃会一口气吃到下一个引号，中间的代码就没了。所以只认「短且有收尾引号」的。
    if (c === "'") {
      const m = /^'(\\.|[^\\'])'/.exec(src.slice(i));
      if (m) {
        out += m[0];
        i += m[0].length;
        continue;
      }
      out += c;
      i++;
      continue;
    }
    if (c === "/" && src[i + 1] === "/") {
      while (i < n && src[i] !== "\n") i++;
      out += " ";
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      i = end === -1 ? n : end + 2;
      out += " ";
      continue;
    }
    out += c;
    i++;
  }
  return out;
}
// 🔴 **只剥注释、不剥字符串**：判据本身就长在字符串里
// （`var("USERPROFILE")` / `var("UKING_TEST_HOME")`），剥了就两边都认不出来。

function walk(dir, out = []) {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (e.endsWith(".rs")) out.push(p);
  }
  return out;
}

const offenders = [];
for (const f of walk(SRC)) {
  const s = stripComments(readFileSync(f, "utf8"));
  if (!READS_HOME.test(s)) continue;
  const hits = (s.match(new RegExp(DESTRUCTIVE, "g")) || []).length;
  if (hits === 0) continue;
  // 🔴 记**裸读次数**而不是只记「这个文件有问题」：按文件记的话，一个已经在基线里的文件
  // 再加一处裸读照样绿 —— skillpack.rs 第二处 LOCALAPPDATA 就是这么溜过去、当天真删了东西的。
  const reads = (s.match(new RegExp(READS_HOME, "g")) || []).length;
  offenders.push({ file: relative(SRC, f).replace(/\\/g, "/"), hits, reads });
}
offenders.sort((a, b) => a.file.localeCompare(b.file));

const accept = process.argv.indexOf("--accept");
const base = existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, "utf8")) : null;

if (accept >= 0) {
  const why = process.argv[accept + 1];
  if (!why) {
    console.error("--accept 后面要写理由（会连同新基线一起存进 " + BASELINE + "）");
    process.exit(2);
  }
  writeFileSync(
    BASELINE,
    JSON.stringify({ reads: Object.fromEntries(offenders.map((o) => [o.file, o.reads])), history: [...(base?.history ?? []), { accepted: why }] }, null, 2) + "\n",
  );
  console.log(`✅ 已接受并重设基线（${offenders.length} 个）：${why}`);
  process.exit(0);
}

if (!base) {
  writeFileSync(BASELINE, JSON.stringify({ reads: Object.fromEntries(offenders.map((o) => [o.file, o.reads])), history: [{ accepted: "首份基线：立项当天的存量" }] }, null, 2) + "\n");
  console.log(`📌 首份基线已写入（${offenders.length} 个存量）`);
  process.exit(0);
}

const known = base.reads ?? {};
// 「变差」= 冒出新文件，**或**老文件里的裸读变多了。
// 🔴 只按文件记会漏：skillpack.rs 已经在基线里，它第二处 LOCALAPPDATA 就是这么溜过去的，
// 当天真删了 %LOCALAPPDATA%/hermes/skills/aigc/uking-ppt。所以记的是**次数**。
const fresh = offenders.filter((o) => (known[o.file] ?? 0) < o.reads);
const improved = offenders.filter((o) => (known[o.file] ?? 0) > o.reads);
const gone = Object.keys(known).filter((f) => !offenders.some((o) => o.file === f));

for (const o of offenders) {
  const was = known[o.file];
  console.log(`  ${was === undefined || was < o.reads ? "🔴" : "·"} ${o.file}  （${o.reads} 处裸读 / ${o.hits} 处真删）`);
}

if (improved.length || gone.length) {
  writeFileSync(BASELINE, JSON.stringify({ reads: Object.fromEntries(offenders.map((o) => [o.file, o.reads])), history: base.history ?? [] }, null, 2) + "\n");
  console.log(`\n🎉 变好了，基线已自动收紧（改善 ${improved.length} 个 · 清零 ${gone.length} 个）`);
}

if (fresh.length) {
  console.error(`\n❌ ${fresh.length} 个「会真删东西」的文件，裸读用户目录变多了：`);
  for (const o of fresh) console.error(`  - ${o.file}：${known[o.file] ?? 0} → ${o.reads} 处`);
  console.error(
    "\n改法：家目录一律走 `crate::installer::user_home_dir()`（它认 UKING_TEST_HOME）；\n" +
      "LOCALAPPDATA/APPDATA 这类也要先查 UKING_TEST_HOME。\n" +
      "直接读 env 的模块**只读时看不出问题**，加第一个 remove_dir_all 的那天会删掉真实数据\n" +
      "（skillpack.rs 2026-08-18 连栽两次：home_dir 一次、legacy_skill_parents 一次）。",
  );
  process.exit(1);
}

console.log(`\n✅ 没有变差（${offenders.length} 个存量在基线内，改好一处收紧一处）`);
