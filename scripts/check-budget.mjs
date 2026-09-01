/**
 * 预算闸门（棘轮式）—— 让 CLAUDE.md 里那张预算表**能阻断**，而不只是一句抱怨。
 *
 * ## 为什么要有这东西
 * 2026-08-11 定了预算表，当天砍完「简化第三刀」（净删约 2,900 行）。到 2026-08-16 盘点：
 *
 *     git diff af9e72c..HEAD  →  108 files changed, 7687 insertions(+), 500 deletions(-)
 *
 * **5 天加回来的是砍掉的 2.6 倍**，同期预算表 5 项全红、红着又涨。
 * 结论不是「大家不自觉」，是 **一个不阻断构建的预算不是预算**。
 *
 * ## 为什么是棘轮，不是硬上限
 * 所有指标**当下就已经超预算**（exe 10.24MB vs 6MB、动作 89 vs 40、开关 61 vs 15）。
 * 一个第一天就红的闸门，第二天就会被 `--no-verify` 绕过或直接删掉 —— 那还不如没有。
 *
 * 所以这里判的是**涨没涨**，不是超没超：
 *  - 比基线涨 → ❌ 红。想加就得先还，「新东西进来必须有旧东西出去」从口号变成机制。
 *  - 比基线降 → ✅ 绿，并**自动把基线收紧**（棘轮只能往一个方向转，省得有人偷偷涨回去）。
 *  - 离目标还有多远一并打出来，别让「不涨」被误当成「达标」。
 *
 * 真要涨（有意识的决策，不是漂移）：`node scripts/check-budget.mjs --accept "理由"`，
 * 理由会连同新基线一起写进 budget.json 并进 git —— **涨可以，但要留名**。
 *
 * ## 量的是哪个产物（这条踩过坑）
 * CLAUDE.md：「量 exe 体积只认 `pnpm tauri build` 的产物」——`cargo build --release` 出的那个
 * **少约 1.5MB**（实测同源码 9,168,896 vs 10,736,640），因为 tauri CLI 之后还要 patch bundle 信息。
 * 两者**落在同一个路径**，没法靠路径区分。所以这里量的是 **NSIS 安装包**：它只可能由
 * `pnpm tauri build` 产出，存在本身即自证。裸 exe 若比安装包新（= 之后又跑过 cargo build），
 * 直接拒绝采信并说明原因 —— 宁可报「量不到」，不报一个错的数。
 *
 * 用法：
 *   node scripts/check-budget.mjs              # 判涨没涨
 *   node scripts/check-budget.mjs --strict     # 量不到的指标也算失败（发版前用）
 *   node scripts/check-budget.mjs --accept "为了 X 接受 exe +200KB"
 */
import { readFileSync, writeFileSync, existsSync, statSync, readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";

// 基线含内部工作日志，维护在私有 ops 仓；公开 CI 跳过本检查。
const BASELINE = process.env.UKING_BUDGET_BASELINE || "UKING-OPS-budget.json.private";
const argv = process.argv.slice(2);
const STRICT = argv.includes("--strict");
const ACCEPT = argv.includes("--accept") ? argv[argv.indexOf("--accept") + 1] : null;

/** 递归数行：只数真正算「我们写的代码」的那些。 */
function countLines(dir, exts, skip = []) {
  let n = 0;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (skip.some((s) => p.replace(/\\/g, "/").includes(s))) continue;
    if (e.isDirectory()) n += countLines(p, exts, skip);
    else if (exts.some((x) => e.name.endsWith(x))) n += readFileSync(p, "utf8").split("\n").length;
  }
  return n;
}

/** 量 exe：只认 NSIS 安装包（见文件头）。返回 {value, note} 或 {skip, why}。 */
function measureInstaller() {
  const dir = "src-tauri/target/release/bundle/nsis";
  if (!existsSync(dir)) return { skip: true, why: "没有 NSIS 产物 —— 跑一次 `pnpm tauri build` 再量" };
  const pkgs = readdirSync(dir)
    .filter((f) => f.endsWith(".exe"))
    .map((f) => ({ f, ...statSync(join(dir, f)) }))
    .sort((a, b) => b.mtimeMs - a.mtimeMs);
  if (!pkgs.length) return { skip: true, why: "NSIS 目录里没有 .exe" };
  const newest = pkgs[0];
  const raw = "src-tauri/target/release/u-king-mini.exe";
  if (existsSync(raw) && statSync(raw).mtimeMs > newest.mtimeMs + 60_000)
    return {
      skip: true,
      why: `裸 exe 比安装包新 —— 之后跑过 cargo build，那个产物少约 1.5MB，采信它会低报（CLAUDE.md 记过这个坑）`,
    };
  return { value: newest.size, note: newest.f };
}

/** 量动作：真注册表为准（exe），拿不到就明说，不猜。 */
function measureActions() {
  const exe = "src-tauri/target/release/u-king-mini.exe";
  if (!existsSync(exe)) return { skip: true, why: "没有 exe，数不了真实注册表" };
  try {
    const out = execFileSync(exe, ["action", "list", "--json"], { encoding: "utf8", timeout: 30_000 });
    const list = JSON.parse(out);
    const covered = JSON.parse(readFileSync("src/generated/action-parity.json", "utf8")).actions.length;
    return { value: list.length, extra: { outside_contract: list.length - covered } };
  } catch (e) {
    return { skip: true, why: `跑 action list 失败：${String(e.message).slice(0, 80)}` };
  }
}

/**
 * 动作核心只能由组合根和四个已知适配层触达。这里量「文件数」而不是命中次数：
 * 同一适配层内多用一次不扩大依赖面；多出一个文件才是新的越权入口。
 */
function measureActionModuleAccess() {
  // 这里是基线**文件集合**，不是「正好四个文件」：替换掉其中一位、再塞进一个新
  // 越权入口，数量仍为 4，但依赖面已经变了，必须报红。
  const baselineFiles = new Set([
    "src-tauri/src/agent/chat.rs",
    "src-tauri/src/identity.rs",
    "src-tauri/src/mcp_serve.rs",
    "src-tauri/src/miniapp.rs",
  ]);
  try {
    const files = execFileSync("rg", [
      "-l",
      "(crate|super)::actions\\b|^\\s*use crate::actions;",
      "src-tauri/src",
      "-g", "*.rs",
      "-g", "!actions.rs",
      "-g", "!lib.rs",
    ], { encoding: "utf8" })
      .split(/\r?\n/)
      .map((f) => f.trim().replaceAll("\\", "/"))
      .filter(Boolean)
      .sort();
    return {
      value: files.length,
      extra: {
        files,
        unexpected_actions_files: files.filter((f) => !baselineFiles.has(f)),
        missing_baseline_actions_files: [...baselineFiles].filter((f) => !files.includes(f)),
      },
    };
  } catch (e) {
    return { skip: true, why: `rg actions 模块访问失败：${String(e.message).slice(0, 80)}` };
  }
}

const MB = (v) => `${(v / 1024 / 1024).toFixed(2)} MB`;

const METRICS = {
  // ★ 两个都要量，别只留一个：CLAUDE.md 那条「6 MB」说的是**裸 exe**。
  // 只量 NSIS 会得到一个 4.94 MB ✅ 的假绿 —— 安装包是压缩过的，比裸 exe 小一半，
  // 拿它去比一条为裸 exe 写的上限，等于用一个不相干的数字宣布达标。
  exe_bytes: {
    label: "裸 exe（CLAUDE.md 那条 6MB 预算说的是这个）",
    target: 6 * 1024 * 1024,
    fmt: MB,
    measure: () => {
      const r = measureInstaller(); // 复用它的产物真伪判定：NSIS 在 = 这次是 tauri build
      if (r.skip) return { skip: true, why: `${r.why}（裸 exe 的可信度跟着安装包走）` };
      const raw = "src-tauri/target/release/u-king-mini.exe";
      if (!existsSync(raw)) return { skip: true, why: "没有裸 exe" };
      return { value: statSync(raw).size };
    },
  },
  installer_bytes: {
    label: "安装包体积（NSIS，下载版发出去的那个）",
    fmt: MB, // 故意不设 target —— 没人为它定过上限，凭空编一个只会制造假绿
    measure: measureInstaller,
  },
  rust_lines: {
    label: "Rust 行数",
    fmt: String,
    measure: () => ({ value: countLines("src-tauri/src", [".rs"]) }),
  },
  frontend_lines: {
    label: "前端行数（不含 generated）",
    fmt: String,
    measure: () => ({ value: countLines("src", [".ts", ".tsx"], ["src/generated"]) }),
  },
  lib_rs_lines: {
    label: "lib.rs 行数（组合根）",
    fmt: String,
    measure: () => ({ value: readFileSync("src-tauri/src/lib.rs", "utf8").split("\n").length }),
  },
  actions_module_access: {
    label: "actions 越权文件数（除组合根 / 既有 4 个适配层）",
    // 只接受这四个既有适配层；出现第五个文件名就是新的跨模块依赖，必须报红。
    target: 4,
    fmt: String,
    measure: measureActionModuleAccess,
  },
  actions: {
    label: "影核动作数",
    target: 40,
    fmt: String,
    measure: measureActions,
  },
  headless_flags: {
    label: "无头开关 --xxx",
    target: 15,
    fmt: String,
    measure: () => {
      const src = ["main.rs", "lib.rs"]
        .map((f) => join("src-tauri/src", f))
        .filter(existsSync)
        .map((f) => readFileSync(f, "utf8"))
        .join("\n");
      return { value: new Set(src.match(/"--[a-z0-9-]+"/g) || []).size };
    },
  },
  tauri_commands: {
    label: "Tauri command 注册数",
    fmt: String,
    measure: () => {
      const src = readFileSync("src-tauri/src/lib.rs", "utf8");
      const block = src.slice(src.indexOf("generate_handler!"));
      const end = block.indexOf("])");
      return { value: (block.slice(0, end).match(/^\s+[a-z_][a-z0-9_]*,?\s*$/gm) || []).length };
    },
  },
  claude_md_lines: {
    label: "CLAUDE.md 行数（每次会话全量进上下文）",
    target: 300,
    fmt: String,
    measure: () => ({ value: readFileSync("CLAUDE.md", "utf8").split("\n").length }),
  },
  // CLAUDE.md 的预算表里「活跃分支 5」这条，2026-08-19 之前**根本没人量** —— 于是它跟
  // 那张表刚立时的处境一模一样：写在文档里，不阻断任何东西，就是一句抱怨。
  // 实测当天：21 条分支、19 个 worktree、12 条分支上悬着 27 个 patch 没人合。
  // 代价不是磁盘（死 worktree 的 target 早清过了），是**「修好了」和「发出去了」脱节** ——
  // 那天发版就差点漏掉两笔已经修好的 bug，因为它们躺在一条没人合的分支上。
  //
  // 🔴 **必须用 `git cherry` 按 patch 判，不能用 `git branch --no-merged` / ahead 数字**：
  //    后者按祖先算，已经 cherry-pick 进 main 的 patch 仍会被算成「未合并」，
  //    于是清理过的分支永远显示有欠账 —— 一个永远红的指标等于没有指标。
  //    （需求榜 P0 那段盘点方式写的就是这条，这里只是把它变成机器执行。）
  unmerged_branches: {
    label: "活跃分支（相对 main 真有未合并 patch 的）",
    target: 5,
    fmt: String,
    measure: () => {
      const git = (...a) => execFileSync("git", a, { encoding: "utf8" });
      try {
        git("rev-parse", "--verify", "main");
      } catch {
        return { skip: true, why: "本仓库没有 main 分支，无从比对" };
      }
      const branches = git("for-each-ref", "--format=%(refname:short)", "refs/heads/")
        .split("\n")
        .map((s) => s.trim())
        .filter((s) => s && s !== "main");
      let live = 0;
      let patches = 0;
      for (const b of branches) {
        let n = 0;
        try {
          n = (git("cherry", "main", b).match(/^\+/gm) || []).length;
        } catch {
          continue; // 拿不到就当 0，宁可少报也别把一条读不到的分支说成有欠账
        }
        if (n > 0) {
          live++;
          patches += n;
        }
      }
      return { value: live, extra: { stranded_patches: patches, total_branches: branches.length + 1 } };
    },
  },
};

const base = existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, "utf8")) : { metrics: {}, history: [] };
const grew = [];
const shrank = [];
const skipped = [];
const rows = [];

for (const [key, m] of Object.entries(METRICS)) {
  const r = m.measure();
  if (r.skip) {
    skipped.push(`${m.label}：${r.why}`);
    continue;
  }
  const prev = m.baseline ?? base.metrics[key];
  const now = r.value;
  const delta = prev == null ? 0 : now - prev;
  rows.push({ key, label: m.label, now, prev, delta, target: m.target, warn_limit: m.warn_limit, fmt: m.fmt, extra: r.extra });
  if (!m.warn_only && prev != null && delta > 0) grew.push({ key, label: m.label, prev, now, delta, fmt: m.fmt });
  if (!m.warn_only && prev != null && delta < 0) shrank.push(key);
  if (r.extra?.unexpected_actions_files?.length || r.extra?.missing_baseline_actions_files?.length) {
    const changed = [
      ...(r.extra.unexpected_actions_files || []),
      ...(r.extra.missing_baseline_actions_files || []),
    ];
    grew.push({
      key,
      label: `${m.label}（命中文件集合变化：${changed.join(", ")}）`,
      prev: 0,
      now: changed.length,
      delta: changed.length,
      fmt: String,
    });
  }
  if (!m.fixed_baseline) base.metrics[key] = prev == null || now < prev || ACCEPT ? now : prev; // 棘轮：只往下收，除非明确 accept
  if (r.extra) base.metrics[`${key}_extra`] = r.extra;
}

const W = Math.max(...rows.map((r) => r.label.length));
console.log("指标".padEnd(W) + "  现在".padStart(14) + "  基线".padStart(14) + "  变化".padStart(12) + "   目标");
for (const r of rows) {
  const d = r.prev == null ? "（新记）" : r.delta === 0 ? "—" : (r.delta > 0 ? "▲ +" : "▼ ") + r.fmt(Math.abs(r.delta));
  const tgt = r.warn_limit != null
    ? r.now <= r.warn_limit ? `✅ ≤${r.fmt(r.warn_limit)}` : `⚠ 超 ${r.fmt(r.now - r.warn_limit)}（仅警告）`
    : r.target == null ? "" : r.now <= r.target ? `✅ ≤${r.fmt(r.target)}` : `❌ 超 ${r.fmt(r.now - r.target)}`;
  console.log(
    r.label.padEnd(W) + r.fmt(r.now).padStart(16) + (r.prev == null ? "-" : r.fmt(r.prev)).padStart(16) + d.padStart(14) + "   " + tgt,
  );
  if (r.extra?.stranded_patches != null)
    console.log(
      `  └ 这些分支上共有 ${r.extra.stranded_patches} 个 patch 还没进 main（全仓 ${r.extra.total_branches} 条分支）` +
        `。**「修好了」不等于「发出去了」** —— 发版从 main 构建，躺在分支上的修复等于没发（宪法 3）。` +
        `\n     逐条看：for b in $(git for-each-ref --format='%(refname:short)' refs/heads/); do echo "$b $(git cherry main $b | grep -c '^+')"; done`,
    );
  if (r.extra?.outside_contract != null)
    console.log(
      `  └ 其中 ${r.extra.outside_contract} 个不在生成的契约清单 src/generated/action-parity.json 里` +
        `（hostManifest 只投影 runtime.* 命名空间，doc./browser./app.* 没进清单）—— conformance 那道闸已全量覆盖（2026-08-19 去掉了 --only runtime.），这条说的是清单覆盖面不是执行覆盖面`,
    );
  if (r.extra?.unexpected_actions_files?.length)
    console.error(
      `  └ ❌ 新的 crate::actions:: 越权文件：${r.extra.unexpected_actions_files.join(", ")} ` +
        "（只允许 agent/chat.rs、identity.rs、mcp_serve.rs、miniapp.rs）",
    );
}

if (skipped.length) {
  console.log("\n⚠ 这些指标这次没量到（不是绿，是不知道）：");
  for (const s of skipped) console.log("  - " + s);
}

if (ACCEPT) {
  base.history = [...(base.history || []), { accepted: ACCEPT, metrics: { ...base.metrics } }].slice(-20);
  writeFileSync(BASELINE, JSON.stringify(base, null, 2) + "\n");
  console.log(`\n✅ 已接受本次变化并重设基线，理由记在 ${BASELINE}：「${ACCEPT}」`);
  process.exit(0);
}

if (grew.length) {
  console.error(`\n❌ ${grew.length} 项比基线涨了 —— 预算超支中，新东西进来必须有旧东西出去：`);
  for (const g of grew) console.error(`  - ${g.label}：${g.fmt(g.prev)} → ${g.fmt(g.now)}（+${g.fmt(g.delta)}）`);
  console.error(`\n真要涨就留个名：node scripts/check-budget.mjs --accept "为什么值得"`);
  process.exit(1);
}

if (shrank.length) {
  writeFileSync(BASELINE, JSON.stringify(base, null, 2) + "\n");
  console.log(`\n🔻 有 ${shrank.length} 项降了，基线已自动收紧（棘轮只往一个方向转）：${shrank.join(", ")}`);
} else if (!existsSync(BASELINE)) {
  writeFileSync(BASELINE, JSON.stringify(base, null, 2) + "\n");
  console.log(`\n📌 已写下第一份基线 ${BASELINE}`);
}

if (STRICT && skipped.length) {
  console.error(`\n❌ --strict：有 ${skipped.length} 项没量到，发版前不许「不知道」当绿`);
  process.exit(1);
}
console.log("\n✅ 没有比基线涨（注意：不涨 ≠ 达标，上面带 ❌ 的仍在超预算）");
