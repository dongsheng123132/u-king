#!/usr/bin/env node
/**
 * U-King 多 AI 协同 —— 统一调用本机其它 AI CLI（带超时 / 输出捕获 / 失败兜底 / 防嵌套死循环）。
 * 无任何第三方依赖（只用 Node 内置模块）。
 *
 * 用法:
 *   node call-agent.mjs --agent <claude|codex|openclaw|hermes> --prompt "..." [--timeout 180] [--model id] [--json]
 *
 * 退出码: 0=成功; 1=队友返回失败/超时; 2=参数错误; 3=检测到嵌套调用(被拦)。
 *
 * ────────────────────────────────────────────────────────────────────────
 * ## 这一版修的三件事（0.9.98 评测实测出来的）
 *
 * ### ① 提示词不再经过 shell —— 「问 A 答 B」的真凶
 * 老实现 `spawn(bin, [..., prompt], { shell: true })`：Windows 上这等于把提示词拼进
 * 一条 cmd 命令行再让 cmd 重新解析。带引号 / 换行 / `&` `|` `%` 的提示词会被切断或改写，
 * 队友收到的是**残缺的另一个问题** —— 实测出现过「返回了一段与提问无关的回答」。
 * 这不是模型答错，是我们根本没把问题完整送到。
 *
 * 现在按**每个 CLI 实测支持的方式**送（口径写在 `AGENTS` 里，不是照文档猜的）：
 *  - `claude` / `codex`：提示词走 **stdin**，argv 里一个字用户文本都没有 → 注入和截断从源头消失。
 *    （`codex exec --help` 原文：不给 PROMPT 参数时 instructions are read from stdin。）
 *  - `hermes`：本机解析到的是**真 .exe**（不是 .cmd 壳），可直接 spawn，压根不经过 cmd。
 *  - `openclaw`：只有 .cmd 壳且不吃 stdin，提示词只能进 argv。**送不安全就明着拒**，
 *    见 `assertArgvSafe` —— 宁可报错让人换个队友，也不把一个被 cmd 改写过的问题发出去。
 *
 * ### ② 超时杀的是**整棵树**，不是那层壳
 * 老实现 `child.kill("SIGKILL")`：Windows 上 SIGKILL 只干掉最外面那个壳，
 * claude 自己和它拉起来的 MCP 服务器全都活着 —— 实测超时后要人工去任务管理器清。
 * 现在 Windows 走 `taskkill /T /F`（整棵树），POSIX 走进程组（`detached` + `kill(-pid)`）。
 *
 * ### ③ 成功退出后也要扫一遍后代
 * 队友自己正常退出，**不代表它拉起来的 MCP 子进程也退了**（这正是实测遗留的那批）。
 * 所以两条路径都做一次 `sweepDescendants`：按 ppid 求闭包，且**只杀本次运行之后创建的**
 * （创建时间早于我们启动的一律不碰 —— pid 会复用，杀错别人的进程比留几个孤儿糟得多）。
 *
 * ### 顺带：部分输出不再被丢掉
 * 老实现超时后只报一句「超时」，那一路收到的 stdout 全扔了。现在超时也把已收到的
 * `output` 一并返回，并记录跑到哪一步（`stage`）—— 排查「卡在哪」全靠它。
 */
import { spawn, spawnSync } from "node:child_process";

const IS_WIN = process.platform === "win32";

// 防「队友再调队友」的网状递归：被另一个 call-agent 起来时，本进程拒绝再往下调。
if (Number(process.env.UKING_TEAMWORK_DEPTH || "0") >= 1) {
  console.error("call-agent: 已在子 agent 内，禁止再嵌套调用（防死循环 / 防烧爆 token）。");
  process.exit(3);
}

function parseArgs(argv) {
  const a = { timeout: 180 };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--agent") a.agent = argv[++i];
    else if (k === "--prompt") a.prompt = argv[++i];
    else if (k === "--timeout") a.timeout = parseInt(argv[++i], 10) || 180;
    else if (k === "--model") a.model = argv[++i];
    else if (k === "--json") a.json = true;
  }
  return a;
}

/**
 * 每个队友怎么调 —— **实测口径**，不是照文档抄的。
 *
 * `how: "stdin"` = 提示词从标准输入送，argv 只放开关（最安全，argv 里没有用户文本）。
 * `how: "argv"`  = 提示词只能当参数送（该 CLI 不吃 stdin）。
 */
const AGENTS = {
  claude: {
    bin: "claude",
    how: "stdin",
    args: (model) => ["-p", ...(model ? ["--model", model] : [])],
  },
  codex: {
    // `codex exec` 不给 PROMPT 时从 stdin 读 —— 见 `codex exec --help`。
    bin: "codex",
    how: "stdin",
    args: (model) => ["exec", ...(model ? ["-m", model] : [])],
  },
  openclaw: {
    bin: "openclaw",
    how: "argv",
    args: (model, prompt) => ["agent", "--local", "-m", prompt, ...(model ? ["--model", model] : [])],
  },
  hermes: {
    // 本机解析到 .exe，直接 spawn 不经 cmd → argv 天然安全。
    bin: "hermes",
    how: "argv",
    args: (model, prompt) => ["-z", prompt, ...(model ? ["-m", model] : [])],
  },
};

/**
 * 找到命令的**真实可执行文件**，并说清它是不是需要 cmd 代跑的批处理壳。
 * 优先 `.exe`：能直接 spawn 的就绝不绕 cmd（少一层解析就少一处能改写提示词的地方）。
 */
function resolveBin(name) {
  const finder = IS_WIN ? "where" : "which";
  const r = spawnSync(finder, [name], { encoding: "utf8", windowsHide: true });
  if (r.status !== 0) return null;
  const hits = r.stdout.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  if (!hits.length) return null;
  const exe = hits.find((p) => /\.exe$/i.test(p));
  if (exe) return { path: exe, shim: false };
  const cmd = hits.find((p) => /\.(cmd|bat)$/i.test(p));
  if (cmd) return { path: cmd, shim: true };
  return { path: hits[0], shim: !IS_WIN ? false : true };
}

/**
 * 这段文本能不能安全地当 argv 送进一个 **cmd 批处理壳**。
 *
 * cmd 会在参数里展开 `%VAR%`、并把 `& | < > ^` 当控制符 —— 引号也保不住全部情况。
 * 送出去的将是一个被改写过的问题，而**收到答案的人无从发现**。
 * 所以这里的选择是：**拒绝，并说清为什么**。静默送一个残缺的问题才是真正的错误。
 */
function assertArgvSafe(text, agent) {
  const bad = /[%&|<>^"\r\n]/.exec(text);
  if (!bad) return null;
  const ch = bad[0] === "\r" || bad[0] === "\n" ? "换行" : `「${bad[0]}」`;
  return (
    `提示词里有 ${ch}，而 ${agent} 在 Windows 上只能通过 .cmd 批处理壳接收参数，` +
    `cmd 会把它改写成另一段文字。已拒绝发送（宁可报错，也不送一个残缺的问题）。` +
    `改用 claude / codex（它们走 stdin，什么字符都能安全送），或把提示词简化成单行纯文本。`
  );
}

/** 求 pid 的后代闭包 —— 只认**本次运行之后创建的**（pid 会复用，别杀错别人的树）。 */
function descendantsOf(rootPid, notBefore) {
  if (!IS_WIN) return [];
  const ps = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CreationDate | ConvertTo-Json -Compress",
    ],
    { encoding: "utf8", windowsHide: true, timeout: 20_000 },
  );
  if (ps.status !== 0 || !ps.stdout.trim()) return [];
  let rows;
  try {
    rows = JSON.parse(ps.stdout);
  } catch {
    return [];
  }
  if (!Array.isArray(rows)) rows = [rows];
  const byParent = new Map();
  const born = new Map();
  for (const r of rows) {
    const pid = Number(r.ProcessId);
    const ppid = Number(r.ParentProcessId);
    if (!Number.isFinite(pid) || !Number.isFinite(ppid)) continue;
    if (!byParent.has(ppid)) byParent.set(ppid, []);
    byParent.get(ppid).push(pid);
    const t = Date.parse(r.CreationDate ?? "");
    born.set(pid, Number.isFinite(t) ? t : 0);
  }
  const out = [];
  const seen = new Set([rootPid]);
  const stack = [rootPid];
  while (stack.length) {
    for (const kid of byParent.get(stack.pop()) ?? []) {
      if (seen.has(kid)) continue;
      seen.add(kid);
      // 创建时间早于本次运行 = 不是我们拉起来的，pid 撞车而已 → 一个字节都不碰。
      if ((born.get(kid) ?? 0) < notBefore) continue;
      out.push(kid);
      stack.push(kid);
    }
  }
  return out;
}

/** 杀掉这棵树（含壳自身）。Windows 用 taskkill /T，POSIX 用进程组。 */
function killTree(child) {
  if (!child?.pid) return;
  if (IS_WIN) {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true, timeout: 15_000 });
  } else {
    try { process.kill(-child.pid, "SIGKILL"); } catch { /* 组已经没了 */ }
    try { child.kill("SIGKILL"); } catch { /* 已退出 */ }
  }
}

/**
 * 队友退出后再扫一遍它留下的后代（**成功路径也要扫**）。
 * claude 正常退出不代表它拉起的 MCP 服务器也退了 —— 那正是实测遗留下来的那批。
 */
function sweepDescendants(pid, notBefore) {
  const left = descendantsOf(pid, notBefore);
  for (const p of left) {
    spawnSync("taskkill", ["/PID", String(p), "/T", "/F"], { windowsHide: true, timeout: 10_000 });
  }
  return left;
}

const args = parseArgs(process.argv.slice(2));
if (!args.agent || !args.prompt) {
  console.error('用法: node call-agent.mjs --agent <claude|codex|openclaw|hermes> --prompt "..." [--timeout 180] [--model id] [--json]');
  process.exit(2);
}
const spec = AGENTS[args.agent];
if (!spec) {
  console.error("未知 agent: " + args.agent + "（可选 claude / codex / openclaw / hermes）");
  process.exit(2);
}

// 🔴 `t0` / `stage` 必须声明在**任何可能调 `fail()` 的代码之前**：
// 下面的「找不到命令」「argv 闸门」都会走 fail()，而 fail() 要报 stage。
// （第一版就栽在这儿：let 的暂时性死区让一次本该清晰的拒绝变成了 ReferenceError 崩溃。）
const t0 = Date.now();
/** 跑到哪一步了 —— 排查「卡在哪」全靠它，超时也会一并报出来。 */
let stage = "spawn";

const resolved = resolveBin(spec.bin);
if (!resolved) {
  fail(`找不到 ${spec.bin}（该 AI 可能未安装，或不在 PATH）`, "resolve");
}

const useStdin = spec.how === "stdin";
const binArgs = useStdin ? spec.args(args.model) : spec.args(args.model, args.prompt);

// argv 路线且要经过批处理壳 → 先确认这段文本送得过去，送不过去就明着拒。
if (!useStdin && resolved.shim) {
  const why = assertArgvSafe(args.prompt, args.agent);
  if (why) fail(why, "argv-guard");
}

// 🔴 **一律 shell:false**。批处理壳由我们显式交给 cmd 代跑（`/d` 跳过 autorun、
// `/s` 让引号按原样交给目标），提示词要么在 stdin、要么已过安全闸门。
const child = resolved.shim
  ? spawn(process.env.ComSpec || "cmd.exe", ["/d", "/s", "/c", resolved.path, ...binArgs], {
      windowsHide: true,
      env: { ...process.env, UKING_TEAMWORK_DEPTH: "1" },
      stdio: ["pipe", "pipe", "pipe"],
    })
  : spawn(resolved.path, binArgs, {
      windowsHide: true,
      env: { ...process.env, UKING_TEAMWORK_DEPTH: "1" },
      stdio: ["pipe", "pipe", "pipe"],
      // POSIX：自成进程组，超时才杀得掉整组（Windows 靠 taskkill /T，不需要）。
      detached: !IS_WIN,
    });

let out = "";
let err = "";
let done = false;
child.stdout.on("data", (d) => { out += d; stage = "streaming"; });
child.stderr.on("data", (d) => { err += d; });

// 提示词走 stdin 的：写完就关，让队友知道输入到此为止（不关它会一直等，看起来就是「卡住」）。
if (useStdin) {
  stage = "stdin-write";
  child.stdin.on("error", () => { /* 队友先退了：由 close 分支统一报，不在这儿重复报 */ });
  child.stdin.end(args.prompt);
} else {
  child.stdin.end();
}

const killer = setTimeout(() => {
  stage = `timeout@${stage}`;
  killTree(child);
  // 杀完壳再扫后代：留着的 MCP 进程会一直占内存/端口，实测要人工清。
  const swept = sweepDescendants(child.pid, t0);
  finish(false, `超时（${args.timeout}s）未返回，已强制结束整棵进程树${swept.length ? `（顺带清理 ${swept.length} 个残留子进程）` : ""}`);
}, args.timeout * 1000);

function fail(msg, at) {
  stage = at || stage;
  if (args.json) {
    process.stdout.write(JSON.stringify({ ok: false, agent: args.agent, ms: 0, stage, output: "", error: msg }) + "\n");
  } else {
    process.stderr.write("call-agent 失败（" + args.agent + "）: " + msg + "\n");
  }
  process.exit(1);
}

function finish(ok, errMsg) {
  if (done) return;
  done = true;
  clearTimeout(killer);
  const ms = Date.now() - t0;
  const output = out.trim();
  if (args.json) {
    // 🔴 失败时也带上 `output`：一路收到的部分结果是排查「卡在哪」最值钱的东西，
    // 老实现在超时分支把它整段丢了。
    process.stdout.write(
      JSON.stringify({ ok, agent: args.agent, ms, stage, output, error: ok ? null : (errMsg || err.trim() || null) }) + "\n",
    );
  } else if (ok) {
    process.stdout.write(output + "\n");
  } else {
    if (output) process.stdout.write(output + "\n"); // 部分输出照样给人看
    process.stderr.write("call-agent 失败（" + args.agent + "）: " + (errMsg || err.trim() || "未知错误") + "\n");
  }
  process.exit(ok ? 0 : 1);
}

child.on("error", (e) =>
  finish(false, "无法启动 " + resolved.path + "：" + e.message + "（该 AI 可能未安装，或不在 PATH）"),
);
child.on("close", (code) => {
  if (done) return;
  stage = "closed";
  // 正常退出也扫一遍：队友退了 ≠ 它拉起来的 MCP 子进程退了。
  sweepDescendants(child.pid, t0);
  // 有标准输出就算成功（部分 CLI 把进度写到 stderr 后正常退出）
  if (code === 0 || out.trim()) finish(true);
  else finish(false, err.trim() || "退出码 " + code);
});
