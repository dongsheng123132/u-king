/**
 * 功能模块之间不许互相 import —— 「模块独立四铁律」第 2 条的闸门。
 *
 * ## 为什么要有它
 *
 * U-King 是个「360 式管家壳」：功能只会越放越多。这种形态唯一的活路是**每块能整块拔掉**，
 * 而拔不掉的原因从来不是「设计时没想过」，是**耦合是一行一行悄悄漏进去的**，
 * 而且**只有真去删的那天才会暴露**。
 *
 * 2026-08-11「简化第三刀」真去删了四个模块，当场发现宪法那句「删一个模块只动 2 个文件」
 * 三处不成立：`hardware.rs` 被 AiRuntime 咬住、`Feed.tsx` 被 Skills 咬住、`mcp.rs` 被
 * cleanup 咬住 —— 全是早就漏进去、只是从没人去删过所以没人知道。
 *
 * 所以判据不能是「删的时候再看」，得是**每次构建都看**。
 *
 * ## 判什么
 *
 * 功能模块 A 里出现 `crate::B::`，且 B 也是功能模块 → 记一条耦合。
 * 允许的方向只有「功能模块 → 公共层」（见 COMMON）。
 *
 * ## 为什么是棘轮不是红线
 *
 * 当下就有一批存量耦合（下面基线那个数）。第一天就红的闸门第二天就会被绕过 ——
 * 跟 `check-budget.mjs` / `check-i18n-missing.mjs` 同一个道理：**存量记账不阻断，
 * 新增一条就红，修好了自动收紧**。要涨得留名（--accept "理由"）。
 *
 * 用法：
 *   node scripts/check-module-coupling.mjs                 # 判红绿
 *   node scripts/check-module-coupling.mjs --list          # 列出每一条耦合（修的时候看）
 *   node scripts/check-module-coupling.mjs --accept "理由"  # 认下这次的涨幅并重设基线
 */
import fs from "fs";
import path from "path";

const SRC = "src-tauri/src";
const BASELINE_FILE = "scripts/module-coupling-baseline.json";

/**
 * 公共层：允许被任何功能模块依赖。
 *
 * 判据是「它是不是**能力**而不是**功能**」—— 公共层没有自己的界面、不代表某个业务板块，
 * 存在的意义就是被复用（宪法：公共能力复用不复制）。往这个名单里加东西前先问一句：
 * 它有没有自己的页面？有就不是公共层，是功能模块。
 */
const COMMON = new Set([
  "installer", // curl / search_paths / system_tool / CREATE_NO_WINDOW，唯一的进程与下载层
  "ulog", // 运行日志
  "actions", // 影核协议核心（零业务 import，本身不依赖任何功能模块）
  "fs", // 文件系统小工具
  "util", // 杂项助手
  "ulog_rotate",
  "paths",
  // 测试沙箱：只在 cfg(test) / 自检里用，本身没有业务。它被十几个模块依赖是**对的** ——
  // 「各模块各起一把锁 = 没锁」那次踩坑之后，这一层就是特意收拢出来的。
  "testsandbox",
]);

/** 组合根 + 入口：它们**本来就**该知道所有模块，不参与判定。 */
const ROOTS = new Set(["lib", "main", "mcp_serve"]);

const files = fs
  .readdirSync(SRC)
  .filter((f) => f.endsWith(".rs"))
  .map((f) => f.replace(/\.rs$/, ""));

/** 子目录形式的模块（agent/、term/ 这种）也算一个模块，按目录名。 */
for (const d of fs.readdirSync(SRC, { withFileTypes: true })) {
  if (d.isDirectory() && !files.includes(d.name)) files.push(d.name);
}

const modules = new Set(files);
const edges = []; // { from, to, line, text }

function scan(file, modName) {
  const src = fs.readFileSync(file, "utf8");
  src.split(/\r?\n/).forEach((line, i) => {
    // 注释里提到 crate::x:: 不算 —— 文档里举例说明是好事，不该被判耦合。
    const code = line.replace(/^\s*\/\/.*$/, "");
    for (const m of code.matchAll(/crate::([a-z_][a-z0-9_]*)::/g)) {
      const to = m[1];
      if (to === modName || !modules.has(to) || COMMON.has(to) || ROOTS.has(to)) continue;
      edges.push({ from: modName, to, line: i + 1, text: line.trim().slice(0, 100) });
    }
  });
}

for (const name of files) {
  if (ROOTS.has(name) || COMMON.has(name)) continue;
  const single = path.join(SRC, `${name}.rs`);
  if (fs.existsSync(single)) scan(single, name);
  const dir = path.join(SRC, name);
  if (fs.existsSync(dir) && fs.statSync(dir).isDirectory()) {
    for (const f of fs.readdirSync(dir).filter((x) => x.endsWith(".rs"))) {
      scan(path.join(dir, f), name);
    }
  }
}

// 同一对模块之间多行只算一条边 —— 我们判的是「这两块粘上了没有」，不是粘了几行。
const pairs = new Map();
for (const e of edges) {
  const k = `${e.from} → ${e.to}`;
  if (!pairs.has(k)) pairs.set(k, []);
  pairs.get(k).push(e);
}

if (process.argv.includes("--list")) {
  for (const [k, list] of [...pairs].sort()) {
    console.log(`\n${k}  (${list.length} 处)`);
    for (const e of list.slice(0, 4)) console.log(`   ${e.from}.rs:${e.line}  ${e.text}`);
  }
  console.log(`\n共 ${pairs.size} 条模块间耦合`);
  process.exit(0);
}

let baseline = Infinity;
try {
  baseline = JSON.parse(fs.readFileSync(BASELINE_FILE, "utf8")).pairs ?? Infinity;
} catch {
  /* 没基线：第一次跑就把现状记下来 */
}

const acceptIdx = process.argv.indexOf("--accept");
const now = pairs.size;
console.log(`功能模块之间的耦合：${now} 条（基线 ${baseline === Infinity ? "—" : baseline}）`);

if (acceptIdx >= 0) {
  const why = process.argv[acceptIdx + 1] || "";
  if (!why.trim()) {
    console.error("--accept 必须带理由：涨可以，但要留名");
    process.exit(1);
  }
  fs.writeFileSync(
    BASELINE_FILE,
    JSON.stringify({ pairs: now, why, list: [...pairs.keys()].sort() }, null, 2) + "\n",
  );
  console.log(`✅ 已接受并重设基线，理由记在 ${BASELINE_FILE}：「${why}」`);
  process.exit(0);
}

if (now > baseline) {
  const prev = new Set(
    (() => {
      try {
        return JSON.parse(fs.readFileSync(BASELINE_FILE, "utf8")).list ?? [];
      } catch {
        return [];
      }
    })(),
  );
  const fresh = [...pairs.keys()].filter((k) => !prev.has(k));
  console.error(`\n❌ 新增了模块间耦合：${baseline} → ${now}`);
  for (const k of fresh) {
    const e = pairs.get(k)[0];
    console.error(`   ${k}   例：${e.from}.rs:${e.line}  ${e.text}`);
  }
  console.error(
    "\n改法：要共享就把那段能力下沉到公共层（installer.rs），或者让组合根 lib.rs 去问、" +
      "把结果当参数传进来。真要粘：node scripts/check-module-coupling.mjs --accept \"为什么值得\"",
  );
  process.exit(1);
}

if (now < baseline) {
  fs.writeFileSync(
    BASELINE_FILE,
    JSON.stringify(
      { pairs: now, why: "自动收紧（解开了耦合）", list: [...pairs.keys()].sort() },
      null,
      2,
    ) + "\n",
  );
  console.log(`✅ 耦合降了：${baseline} → ${now}，基线已收紧`);
  process.exit(0);
}

console.log("⚠️ 与基线持平（存量耦合，不阻断）。想看是哪些：--list");
process.exit(0);
