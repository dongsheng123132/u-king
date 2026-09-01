// 一搜商答 / 1so —— CLI 调度。
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { parseArgs, setQuiet, logE, done, fail } from "./util.mjs";
import { projectPaths } from "./config.mjs";
import { cmdScan } from "./commands/scan.mjs";

// 🔴 **会调模型的命令一律动态加载，不再静态 import**（2026-08-24）。
//
// 起因：`llm.mjs` 会**自己**去读 `~/.uking/device.json` 里的虾盘云设备钱包 Key
// （见 llm.mjs 的 apiKey 解析）—— 也就是说它不需要谁来注入密钥。只要客户机装了 U-King，
// 任何人在命令行敲 `node ~/.uking/skills/1so-geo/bin/1so.mjs aicheck --provider openai`
// 就能拿着**我们的**额度烧 token。**摘掉 GUI 按钮和 Tauri command 完全挡不住这一条。**
//
// 所以真正的闸门是「不把这些文件发出去」：`geo.rs::SKILL_FILES` 只发离线自查那条链，
// 会调模型的命令连同 `llm.mjs` 一起不进客户端。这里改成动态 import，是为了让缺文件时
// **优雅地说人话**，而不是整个 CLI 在加载阶段就崩掉（静态 import 缺一个文件 = 连 scan 都跑不了）。
//
// 🔴 判据是**文件在不在**，不是某个布尔开关 —— 开关可以被改，缺的文件改不出来。
// 我们自己（人工给客户出报告）用的是仓库里这份完整的技能包，不是客户机上那份。
const LLM_COMMANDS = {
  inspect: () => import("./commands/inspect.mjs").then((m) => m.cmdInspect),
  questions: () => import("./commands/questions.mjs").then((m) => m.cmdQuestions),
  detect: () => import("./commands/detect.mjs").then((m) => m.cmdDetect),
  aicheck: () => import("./commands/aicheck.mjs").then((m) => m.cmdAicheck),
  ingest: () => import("./commands/ingest.mjs").then((m) => m.cmdIngest),
  generate: () => import("./commands/generate.mjs").then((m) => m.cmdGenerate),
  optimize: () => import("./commands/optimize.mjs").then((m) => m.cmdOptimize),
};

const NOT_SHIPPED = "这个命令没有随 U-King 客户端发布（它会消耗内置 AI 额度）。\n"
  + "需要《AI 可见度报告》请加微信 hecare888，我们人工出具。";
const NOT_SHIPPED_CODE = 3;

/** 取 llm.mjs；客户端没发它就返回 null（调用方负责说人话）。 */
async function loadLlm(jsonMode) {
  const m = await import("./llm.mjs").catch(() => null);
  if (!m) { fail(jsonMode, NOT_SHIPPED, NOT_SHIPPED_CODE); return null; }
  return m;
}

const HELP = `一搜商答 / 1so —— 让 AI 知道真实商家（本地 GEO 工具）

用法：1so <命令> [选项]

命令：
  scan       一键"搜全网看自己"可视化体检面板         --name "公司名" [--region] [--auto][--proxy URL]（40渠道/客户自查+自动粗测）
  inspect    网页「AI 友好度」诊断→100分体检+生成llms.txt/JSON-LD  --url https://你的站 [--name][--keyword][--proxy]（纯离线不烧token）
  detect     检测「公司在 AI 眼里的样子」，产出报告      --name "公司名" [--region] [--industry] [--keywords "a,b"]
  aicheck    对接各大模型跑一遍问答→各家分数+总分+可视化测试报告  --name [--models "a,b"][--render]（OpenRouter/BYOK扣客户费用）
  ingest     读本地资料 → 提炼结构化知识卡              [目录] 或 --project <目录>（默认读 <项目>/materials/）
  generate   知识卡 → AI 可读答案页(HTML+JSON-LD+地图)  [--project <目录>]
  optimize   对比 AI 认知×真实资料 → 补内容清单          [--project <目录>]（需先 ingest + detect）
  questions  生成行业高频问答(大家最关心/AI问得多)→起草答案  [--industry X][--region][--n 12][--merge]
  run        一条龙：ingest → scan → detect → generate → optimize → 预览
  preview    浏览器打开生成的答案页
  doctor     自检 LLM 后端是否可用

通用选项：
  --project <dir>   项目目录（默认当前目录）。materials/ 放资料，产物落 .1so/ 与 site/
  --provider <p>    LLM 后端：uking（调本机 claude/codex）| openrouter（一key通全网模型）
                              | openai（虾盘云等兼容端点）| bl（本机百炼，默认）
  --model <id>      指定模型（openrouter 例：openai/gpt-4o-mini、anthropic/claude-3.5-sonnet）
  --key <k>         API key（也可环境变量：OPENROUTER_API_KEY / SO_API_KEY）；--base 换端点
  --json            机器可读输出（stdout 出契约 JSON）
  --quiet           压制进度日志
  --no-preview      run 时不自动打开浏览器

示例：
  1so run --project examples/demo
  1so detect --name "贺去病AI工作室" --region "深圳宝安" --industry "AI培训"
  1so ingest ./我的资料 && 1so generate && 1so preview
`;

export async function main(argv) {
  const args = parseArgs(argv);
  setQuiet(args.quiet);
  const cmd = args._[0];
  const jsonMode = !!args.json;
  if (!cmd || args.help || cmd === "help") { process.stdout.write(HELP); process.exit(0); }

  try {
    // 离线那条链：不碰 llm.mjs、不读 device.json、不花一分钱，所以留在客户端。
    if (cmd === "scan") return await cmdScan(args, {});
    if (cmd === "preview") return preview(args, jsonMode);

    if (cmd in LLM_COMMANDS || cmd === "doctor" || cmd === "run") {
      const llm = await loadLlm(jsonMode);
      if (!llm) return NOT_SHIPPED_CODE; // 客户端没发 llm.mjs —— loadLlm 已经说过人话了
      const provider = llm.resolveProvider(args);
      const llmOpts = { provider, model: llm.resolveModel(args, provider), key: args.key, base: args.base };
      if (cmd === "doctor") return await doctor(llm, llmOpts, jsonMode);
      if (cmd === "run") return await run(args, llmOpts);
      const fn = await LLM_COMMANDS[cmd]().catch(() => null);
      if (!fn) return fail(jsonMode, NOT_SHIPPED, NOT_SHIPPED_CODE);
      return await fn(args, llmOpts);
    }
    return fail(jsonMode, `未知命令：${cmd}\n运行 1so help 看用法。`, 2);
  } catch (e) {
    return fail(jsonMode, e, 1);
  }
}

async function doctor(llm, llmOpts, jsonMode) {
  logE(`探测 LLM 后端：${llmOpts.provider} / ${llmOpts.model} …`);
  const r = await llm.ping(llmOpts);
  if (r.ok) { logE(`✓ 后端正常（${r.ms}ms）：${r.sample}`); return done(jsonMode, { ok: true, ...r, provider: llmOpts.provider, model: llmOpts.model }, "ok"); }
  return fail(jsonMode, `后端不可用：${r.error}`, 1);
}

function preview(args, jsonMode) {
  const P = projectPaths(args.project || ".");
  if (!existsSync(P.page)) return fail(jsonMode, `还没有答案页：${P.page}\n请先 1so generate。`, 2);
  openInBrowser(P.page);
  logE(`已在浏览器打开：${P.page}`);
  return done(jsonMode, { ok: true, page: P.page }, P.page);
}

function openInBrowser(file) {
  const plat = process.platform;
  try {
    if (plat === "win32") spawn("cmd", ["/c", "start", "", file], { detached: true, stdio: "ignore" }).unref();
    else if (plat === "darwin") spawn("open", [file], { detached: true, stdio: "ignore" }).unref();
    else spawn("xdg-open", [file], { detached: true, stdio: "ignore" }).unref();
  } catch {}
}

// 一条龙
async function run(args, llmOpts) {
  const jsonMode = !!args.json;
  // 复用各命令，但内部不让它们各自 process.exit —— 简单起见：串行调用，捕获它们通过 done() 触发的退出。
  // 由于 done() 会 exit，这里改为直接调用各命令的核心逻辑：用非 exit 版本不划算，故 run 采用子进程串联更干净。
  // 只用名字（下面走子进程串联），2026-08-24 起不再在这里引函数本体 ——
  // 那几个命令已改成动态加载，静态引用它们会在 run 被调用时 ReferenceError。
  const steps = [["ingest"], ["scan"], ["detect"], ["generate"], ["optimize"]];
  // 为避免 done()/fail() 的 process.exit 中断流水线，run 用「静默模式 + 直接函数返回」不可行（命令内部会 exit）。
  // 因此 run 走子进程串联，稳定可控。
  const self = fileURLToPath(new URL("../bin/1so.mjs", import.meta.url));
  const baseArgs = [];
  if (args.project) baseArgs.push("--project", String(args.project));
  if (args.provider) baseArgs.push("--provider", String(args.provider));
  if (args.model && args.model !== true) baseArgs.push("--model", String(args.model));
  baseArgs.push("--quiet");

  for (const [name] of steps) {
    logE(`\n▶ ${name} …`);
    const extra = [];
    if ((name === "detect" || name === "scan") && args.name) extra.push("--name", String(args.name));
    if (name === "scan" && args.auto) { extra.push("--auto"); if (args.proxy && args.proxy !== true) extra.push("--proxy", String(args.proxy)); }
    const code = await runChild(self, [name, ...baseArgs, ...extra]);
    if (code !== 0) return fail(jsonMode, `步骤 ${name} 失败（退出码 ${code}）。`, 1);
  }
  logE("\n✓ 全流程完成。");
  if (!args["no-preview"]) preview({ project: args.project }, false);
  const P = projectPaths(args.project || ".");
  return done(jsonMode, { ok: true, report: P.report, page: P.page, optimize: P.optimize },
    `完成：\n  报告 → ${P.report}\n  答案页 → ${P.page}\n  优化建议 → ${P.optimize}`);
}

function runChild(self, argv) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [self, ...argv], { stdio: ["ignore", "inherit", "inherit"] });
    child.on("close", (code) => resolve(code ?? 1));
    child.on("error", () => resolve(1));
  });
}
