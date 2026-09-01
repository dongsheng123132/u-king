#!/usr/bin/env node
/**
 * U-Workspace 办公能力跑道（uwork bench）
 * ==========================================
 * 问的是一个问题：**「把一件真办公活交给它，它交得出能用的产物吗？」**
 *
 * 跟 `推广物料/…/benchmark-v2` 那套的区别：那套是给公众号文章做**多 CLI 公平横评**
 * （关技能、关扩展、锁同一模型比裸壳）。这套相反 —— 它测的是**我们配好的这台机器**：
 * 技能包开着、驱动配好、路径都通，模拟客户拿到 U-King 之后的真实状态。
 *
 * 三条不许破的规矩（跟数据基台同源，见 CLAUDE.md）：
 *   ① **判分只看产物**，不看 CLI 在 stdout 里怎么自述。它说「已完成」不算数。
 *   ② **算错就是错**。格式漂亮但合计差 1000 块，客户看不出来，但那是事故 —— 判 fail，不给部分分。
 *   ③ **报失败**。全绿的报告一眼就是营销；跑挂了、超时了、判分器自己崩了都如实写进 report.json。
 *
 * 用法：
 *   node bench/uwork/run.mjs                          # 全部任务，引擎 pi
 *   node bench/uwork/run.mjs --engine claude          # 换引擎（pi | claude | hermes | codex）
 *   node bench/uwork/run.mjs --only cad,mail          # 只跑某几个
 *   node bench/uwork/run.mjs --keep                   # 跑完保留工作区（默认也保留，此项仅为显式）
 *   node bench/uwork/run.mjs --json                   # stdout 只出 JSON（给上层脚本用）
 *
 * 产物：`bench/uwork/out/<引擎>-<时间戳>/`
 *   ├── report.json          汇总（含每条 check 的通过与否 + 失败原因）
 *   ├── <任务id>/            该任务的工作区（种子文件 + CLI 生成的产物，可直接双击查验）
 *   └── <任务id>.stdout.txt  CLI 原始输出（判分不看它，但排障要看）
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TASKS_DIR = path.join(HERE, "tasks");
const SEEDS_DIR = path.join(HERE, "seeds");
const GRADERS_DIR = path.join(HERE, "graders");

// ---------------- 参数 ----------------
function parseArgs(a) {
  const o = {};
  for (let i = 0; i < a.length; i++) {
    const t = a[i];
    if (t.startsWith("--")) { const k = t.slice(2); o[k] = a[i + 1] && !a[i + 1].startsWith("--") ? a[++i] : true; }
  }
  return o;
}
const args = parseArgs(process.argv.slice(2));
const asJson = !!args.json;
const engineId = String(args.engine || "pi");
const only = args.only ? String(args.only).split(",").map((s) => s.trim()) : null;
const log = (...m) => { if (!asJson) console.log(...m); };

// ---------------- 引擎适配 ----------------
// 每个引擎给出 [可执行文件, argv]。**故意不走 shell** —— 提示词里有换行、中文、引号，
// 一进 shell 就得跟三套转义规则打架（cmd / PowerShell / bash 各不相同），
// 而 Node 的 spawn 直接传 argv 数组不会碰这些。.cmd 只能经 cmd.exe，单独一条分支处理。
const WIN = process.platform === "win32";
/** PATH 上**所有**同名候选（不是第一个）。同一个命令在一台机器上常常有好几份。 */
function whichAll(names) {
  const dirs = (process.env.PATH || "").split(WIN ? ";" : ":");
  const exts = WIN ? [".exe", ".cmd", ".bat", ""] : [""];
  const out = [];
  for (const n of names) for (const d of dirs) for (const e of exts) {
    const p = path.join(d, n + e);
    try { if (fs.statSync(p).isFile() && !out.includes(p)) out.push(p); } catch {}
  }
  return out;
}
/**
 * 解析成一条**不经 cmd.exe** 的 argv 前缀。
 * 🔴 关键是「扫遍 PATH 挑能解开的那一个」而不是「取第一个」：本机 `~/bin/claude.cmd`
 * 是原生安装器写的转发器（路径里全是 `%APPDATA%` / `%latest%` 变量，解不开），
 * 而 `%APPDATA%\npm\claude.cmd` 里明明白白写着 claude.exe 的绝对路径。
 * 取第一个 = 退回 cmd.exe = 多行提示词被截断 = 得出冤枉结论。
 */
function resolveLauncher(name) {
  const cands = whichAll([name]);
  let fallback = null;
  for (const c of cands) {
    // Windows 上真正能直接 spawn 的只有 .exe；无扩展名的那份通常是 Git Bash 的
    // shell 脚本（本机 `~/bin/claude` 就是），Node spawn 它会秒退成「0.0s 交白卷」。
    if (WIN && !/\.(exe|cmd|bat)$/i.test(c)) continue;
    const [exe, pre] = viaCmd(c, []);
    if (exe !== "cmd.exe") return { exe, prefix: pre, from: c };
    fallback = fallback || { exe, prefix: pre, from: c };
  }
  if (fallback) log(`⚠ ${name} 只能经 cmd.exe 启动（${fallback.from}）—— 多行提示词可能被截断，结论不可信`);
  return fallback;
}
function which(names) {
  const dirs = (process.env.PATH || "").split(WIN ? ";" : ":");
  // 🔴 Windows 上扩展名顺序不能把 "" 放前面：`C:\Users\…\bin\claude`（无扩展名）常常是
  // Git Bash 的 shell 脚本，Node 直接 spawn 会秒退 —— 表现成「引擎 0.0s 交白卷」，
  // 看起来像它干砸了，其实是压根没启动。先找 .exe/.cmd。
  const exts = WIN ? [".exe", ".cmd", ".bat", ""] : [""];
  for (const n of names) for (const d of dirs) for (const e of exts) {
    const p = path.join(d, n + e);
    try { if (fs.statSync(p).isFile()) return p; } catch {}
  }
  return null;
}
/**
 * 把 npm 生成的 `.cmd` 壳解开，找出它真正要起的东西 —— 这是 `src-tauri/src/agent/launcher.rs`
 * 的移植版，**别改成别的做法**（同一个问题不许有两套解）。
 *
 * 🔴 为什么必须解：经 cmd.exe 传参会把**多行提示词截断**。实测这轮 claude 收到的
 * 「做以下修改：」后面是空的 —— 它老老实实反问「清单没传过来」，然后被我判成 FAIL。
 * 一条被跑道弄坏的提示词，能得出「Claude Code 干不了这活」这种完全错误的结论。
 *
 * 两种壳都存在：`claude.cmd → "…\bin\claude.exe" %*`、`codex.cmd → "%_prog%" "…\codex.js" %*`。
 * 共同点是目标一定在双引号里。跳过 `IF EXIST "%dp0%\node.exe"` 那种解释器探测行
 * （认成目标会起一个空 node 挂着等 stdin，表现成「卡死」比报错还难查）。
 */
function unwrapShim(shim) {
  let text;
  try { text = fs.readFileSync(shim, "utf8"); } catch { return null; }
  const dir = path.dirname(shim).replace(/[\\/]+$/, "");
  for (const raw of text.split('"')) {
    const low = raw.toLowerCase();
    const isExe = low.endsWith(".exe");
    const isJs = /\.(js|mjs|cjs)$/.test(low);
    if (!isExe && !isJs) continue;
    const expanded = raw.replace(/%~?dp0%?/gi, dir);
    if (expanded.includes("%")) continue; // 还留着 %_prog% 之类的未知变量，别猜
    if (isExe && /(^|[\\/])node\.exe$/i.test(expanded)) continue; // 解释器探测行，不是目标
    try { if (!fs.statSync(expanded).isFile()) continue; } catch { continue; }
    return isExe ? { exe: expanded, prefix: [] } : { exe: process.execPath, prefix: [expanded] };
  }
  return null;
}
/** 解析成一条能直接 spawn 的 [可执行文件, 前置参数]；解不开才退回 cmd.exe（单行参数仍可用）。 */
function viaCmd(exe, argv) {
  if (!exe || !/\.(cmd|bat)$/i.test(exe)) return [exe, argv];
  const u = unwrapShim(exe);
  if (u) return [u.exe, [...u.prefix, ...argv]];
  return ["cmd.exe", ["/d", "/s", "/c", exe, ...argv]];
}

const ENGINES = {
  // pi：技能从 ~/.agents/skills 全局加载（Agent Skills 标准目录），无需额外参数。
  // --approve = 信任当前项目目录（无头模式不会弹信任提示，不给就读不到项目内资源）。
  pi: () => {
    // 🔴 不能拿 process.execPath 跑 pi：pi 要 Node ≥22.19，而跑道自己可能是被更老的
    // Node 启动的（本机系统 Node 是 22.14）。要用**跟 pi 装在同一个 prefix 里的那个 node**。
    const bin = which(["pi"]);
    const prefixes = [bin ? path.dirname(bin) : null, path.dirname(process.execPath)].filter(Boolean);
    for (const pre of prefixes) {
      const cli = path.join(pre, "node_modules/@earendil-works/pi-coding-agent/dist/cli.js");
      const node = path.join(pre, "node.exe");
      const ok = (p) => { try { return fs.statSync(p).isFile(); } catch { return false; } };
      if (ok(cli)) return { exe: ok(node) ? node : process.execPath, argv: (p) => [cli, "-p", "--approve", p], env: { PI_SKIP_VERSION_CHECK: "1" } };
    }
    if (!bin) return null;
    const [e, pre] = viaCmd(bin, []);
    return { exe: e, argv: (p) => [...pre, "-p", "--approve", p], env: { PI_SKIP_VERSION_CHECK: "1" } };
  },
  // Claude Code：跟 U-Workspace 委派时用的参数一致（bypassPermissions），否则会停在权限确认上。
  claude: () => {
    const l = resolveLauncher("claude");
    if (!l) return null;
    const pre = l.prefix;
    return { exe: l.exe, argv: (p) => [...pre, "-p", "--permission-mode", "bypassPermissions", p], env: {} };
  },
  hermes: () => {
    const l = resolveLauncher("hermes");
    if (!l) return null;
    const pre = l.prefix;
    return { exe: l.exe, argv: (p) => [...pre, "--oneshot", "--yolo", "--ignore-rules", p], env: {} };
  },
  codex: () => {
    const l = resolveLauncher("codex");
    if (!l) return null;
    const pre = l.prefix;
    return { exe: l.exe, argv: (p) => [...pre, "exec", "--dangerously-bypass-approvals-and-sandbox", p], env: {} };
  },
};

const mk = ENGINES[engineId];
if (!mk) { console.error(`不认识的引擎 "${engineId}"，可选：${Object.keys(ENGINES).join(" / ")}`); process.exit(2); }
const engine = mk();
if (!engine) { console.error(`引擎 "${engineId}" 在这台机器上找不到 —— 先装上再跑，别跑出一份「全 0 分」的假结论`); process.exit(2); }

// ---------------- 任务 ----------------
const tasks = fs.readdirSync(TASKS_DIR).filter((f) => f.endsWith(".json")).sort()
  .map((f) => JSON.parse(fs.readFileSync(path.join(TASKS_DIR, f), "utf8")))
  .filter((t) => !only || only.includes(t.id));
if (!tasks.length) { console.error("没有匹配的任务"); process.exit(2); }

const stamp = new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14);
const runDir = path.join(HERE, "out", `${engineId}-${stamp}`);
fs.mkdirSync(runDir, { recursive: true });

function copyDir(src, dst) {
  fs.mkdirSync(dst, { recursive: true });
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, e.name), d = path.join(dst, e.name);
    if (e.isDirectory()) copyDir(s, d); else fs.copyFileSync(s, d);
  }
}

function run(exe, argv, cwd, env, timeoutMs) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    // 🔴 `stdio[0]` 必须是 ignore，不能留默认的 pipe。
    // 实测（2026-08-04）：`pi -p` 在 stdin 是**打开着的管道**时永久卡死 —— 7 分钟 stdout
    // 一个字节都没有；同一条命令把 stdin 关掉 12.9s 正常返回。非交互模式下 CLI 会把
    // 「stdin 不是 TTY」当成「还有输入要来」，于是等一个永远不会关的管道。
    // 这跟客户报的「u-chat 转圈转到天荒地老」是同一类症状，排查时先看这里。
    const ch = spawn(exe, argv, { cwd, env: { ...process.env, ...env }, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    let out = "", err = "", killed = false, done = false;
    const finish = (code) => { if (done) return; done = true; clearTimeout(timer); clearTimeout(hardTimer); resolve({ code, out, err, ms: Date.now() - t0, killed }); };
    let hardTimer = null;
    // 🔴 Windows 上 `ch.kill()` 只杀得掉直接子进程。引擎是 .cmd 时直接子进程是 cmd.exe，
    // 底下真正干活的 claude.exe / node.exe 活得好好的，还攥着 stdout 管道 ——
    // 于是 `close` 事件**永远不会触发**，跑道自己被吊死在超时之后（实测卡了 10 分钟以上）。
    // 必须 taskkill /T 按进程树杀；再加一道 5s 硬闸，管道万一还没关也照样往下走。
    // （按 PID 树杀，绝不按镜像名 —— 这台机器上还有别的 claude.exe 在给人干活。）
    const timer = setTimeout(() => {
      killed = true;
      try {
        if (WIN) spawn("taskkill", ["/PID", String(ch.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
        else ch.kill("SIGKILL");
      } catch {}
      hardTimer = setTimeout(() => finish(-2), 5000);
    }, timeoutMs);
    ch.stdout.on("data", (d) => { out += d; if (out.length > 8e6) out = out.slice(-4e6); });
    ch.stderr.on("data", (d) => { err += d; if (err.length > 2e6) err = err.slice(-1e6); });
    ch.on("error", (e) => { err += "\n[spawn error] " + e.message; finish(-1); });
    ch.on("close", (code) => finish(code));
  });
}

// ---------------- 主流程 ----------------
const results = [];
log(`引擎 ${engineId}  →  ${engine.exe}`);
log(`工作区 ${runDir}\n`);

for (const t of tasks) {
  const ws = path.join(runDir, t.id);
  fs.mkdirSync(ws, { recursive: true });
  for (const s of t.seeds || []) {
    const src = path.join(SEEDS_DIR, s);
    if (!fs.existsSync(src)) { console.error(`种子文件缺失: ${s}`); process.exit(2); }
    fs.statSync(src).isDirectory() ? copyDir(src, path.join(ws, s)) : fs.copyFileSync(src, path.join(ws, s));
  }

  log(`▶ ${t.id}  ${t.title}`);
  const timeoutMs = t.timeoutMs || 420000;
  const r = await run(engine.exe, engine.argv(t.prompt), ws, engine.env, timeoutMs);
  fs.writeFileSync(path.join(runDir, `${t.id}.stdout.txt`), r.out);
  if (r.err) fs.writeFileSync(path.join(runDir, `${t.id}.stderr.txt`), r.err);

  // 判分：只看工作区里的产物。CLI 退出码非 0 / 被超时杀掉也照样判 ——
  // 有的 harness 干完活才崩在收尾上，产物是好的；反过来退出码 0 也可能什么都没生成。
  // 「引擎没启动起来」跟「引擎干砸了」必须分开报。混在一起会得出
  // 「claude 0/5」这种冤枉结论 —— 实际上一个字节都没跑过。
  let grade = { pass: false, checks: [], error: null };
  if (r.code === -1 || (r.ms < 1500 && !r.out && /ENOENT|spawn error|不是内部或外部命令|is not recognized/i.test(r.err))) {
    grade = { pass: false, checks: [], error: `引擎没能启动（${r.ms}ms 就退了）：${(r.err || "无 stderr").trim().slice(0, 300)} —— 这不是它做不出来，是我们没把它拉起来` };
    results.push({ id: t.id, title: t.title, verdict: "ERROR", ms: r.ms, exitCode: r.code, timedOut: false, checks: [], error: grade.error, passed: 0, total: 0 });
    log(`  💥 ERROR  引擎没启动 —— ${grade.error.slice(0, 160)}\n`);
    continue;
  }
  try {
    const g = await import(pathToFileURL(path.join(GRADERS_DIR, t.grader)).href);
    grade = await g.grade({ ws, task: t, stdout: r.out });
  } catch (e) {
    grade = { pass: false, checks: [], error: `判分器自己崩了: ${e.message}` };
  }
  const passed = grade.checks.filter((c) => c.ok).length;
  const total = grade.checks.length;
  const verdict = grade.error ? "ERROR" : grade.pass ? "PASS" : "FAIL";
  results.push({
    id: t.id, title: t.title, verdict, ms: r.ms, exitCode: r.code, timedOut: r.killed,
    checks: grade.checks, error: grade.error, passed, total,
  });
  log(`  ${verdict === "PASS" ? "✅" : verdict === "ERROR" ? "💥" : "❌"} ${verdict}  ${(r.ms / 1000).toFixed(1)}s  ${passed}/${total} 项通过${r.killed ? "  ⏱超时被杀" : ""}`);
  for (const c of grade.checks) if (!c.ok) log(`     ✗ ${c.name}${c.detail ? " —— " + c.detail : ""}`);
  if (grade.error) log(`     💥 ${grade.error}`);
  log("");
}

const summary = {
  engine: engineId, engineExe: engine.exe, when: new Date().toISOString(),
  node: process.version, host: os.platform(),
  total: results.length,
  pass: results.filter((r) => r.verdict === "PASS").length,
  fail: results.filter((r) => r.verdict === "FAIL").length,
  error: results.filter((r) => r.verdict === "ERROR").length,
  totalMs: results.reduce((a, r) => a + r.ms, 0),
  results,
};
fs.writeFileSync(path.join(runDir, "report.json"), JSON.stringify(summary, null, 2));

if (asJson) console.log(JSON.stringify(summary));
else {
  log("─".repeat(64));
  log(`${engineId}：${summary.pass}/${summary.total} 通过` +
      (summary.error ? `（另有 ${summary.error} 个判分器出错）` : "") +
      `  合计 ${(summary.totalMs / 1000).toFixed(0)}s`);
  log(`报告 ${path.join(runDir, "report.json")}`);
}
process.exit(summary.pass === summary.total ? 0 : 1);
