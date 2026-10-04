/**
 * 1.3.7「界面收敛」的无头浏览器探针 —— 真 Chromium 里渲染收敛后的界面，用断言量出来，不靠肉眼。
 * 背景：docs/收敛方案-2026-10-03.md、CHANGELOG.md 的 1.3.7 段。
 *
 * 🔴 隐私铁律（同 shot-toolhub.mjs）：只用 Playwright 无头 Chromium 的 `page.screenshot` 截**页面**，
 *    绝不截桌面/屏幕（不许 CopyFromScreen / PowerShell 截屏 / computer-use）。
 * 🔴 不起 exe、不碰正在跑的 U-King：连的是独立 vite dev server（默认 :1471），
 *    所有 Tauri 调用走本文件下面的 shim 喂假数据 —— 只证明布局与交互，不证明真实后端行为。
 * 🔴 「找不到元素就跳过」= 假绿：一律 `problems.push`，最后 problems 非空则 exit 1。
 *
 * 量什么（每个视口：1280×640 = 真实客户机最紧的一档，1440×900）：
 *  1. 侧栏入口恰好 8 项（首屏 4 + 更多 4；「实验室」组 2026-10-04 起为空、整组不渲染）；文案/顺序以 src/components/Sidebar.tsx 为准
 *     （本脚本运行时解析它，并与下面 BRIEF 里的任务清单对照，不一致只报差异、不改源码）。
 *  2. 逐个点这 8 项：渲染出非空内容、没有 PanelBoundary/根 ErrorBoundary 的崩溃兜底文案、
 *     pageerror / console.error 为 0（原文全部收集）、没有 report_bug(ui_*) 上报。
 *  3. 我的 AI → 右上「体检 · 升级 →」进子页，「← 返回我的 AI」回来。
 *  4. AI 设置：「账号 · 充值」「用量账单」两个子 tab，用量账单里有「Token 水电表」那块。
 *  5. 工作台：页内「对话 ↔ 终端」切换；对话态无专家时，顶栏大脑处是静态标签「Claude Code」而不是 <select>。
 *  6. 每页横向溢出：`documentElement.scrollWidth > clientWidth`，外加**真正的滚动容器**（当前可见的 <main>
 *     与侧栏 <nav>）—— 后者才是 shot-toolhub.mjs 里记过教训的：页面内容裁在 <main overflow-y-auto> 上，
 *     只量 documentElement 永远量不出横向溢出。
 *
 * 用法：
 *   pnpm vite --port 1471 --strictPort        # 另开一个终端/后台；跑完关掉
 *   node scripts/shot-converged-ui.mjs        # 换地址：UKING_DEV_URL=http://localhost:1471/
 * 截图：%TEMP%/uking-shot-converged/<视口>-<场景>.png（换目录：UKING_SHOT_OUT=）
 * 退出码：0 = problems 为空；1 = 有 problems（或脚本自身崩了）。
 */
import { chromium } from "playwright";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const URL = process.env.UKING_DEV_URL || "http://localhost:1471/";
/**
 * 变异自检（让断言证明自己真会红）：UKING_PROBE_MUTATE=brain|sidebar|overflow 时，在**浏览器侧**对被测代码做一处
 * 破坏（不改磁盘上任何文件），对应断言必须变红、退出码必须是 1。输出目录自动加后缀，不覆盖正式截图。
 *   brain    拦截 Chat.tsx 的响应，把 PICKABLE_ENGINES 改成 ["claude","uking"] → 大脑区应变回 <select>
 *   sidebar  拦截 Sidebar.tsx 的响应，往实验室组塞一个假入口 → 入口应变成 9 项，且多出「实验室」开关
 *   overflow 每页量之前往可见 <main> 里塞一个 4000px 宽的块 → 横向溢出断言应变红
 */
const MUTATE = process.env.UKING_PROBE_MUTATE || "";
if (MUTATE && !["brain", "sidebar", "overflow"].includes(MUTATE)) throw new Error(`未知 UKING_PROBE_MUTATE=${MUTATE}`);
const OUT_BASE = process.env.UKING_SHOT_OUT || path.join(tmpdir(), "uking-shot-converged");
const OUT = MUTATE ? `${OUT_BASE}-mutate-${MUTATE}` : OUT_BASE;
mkdirSync(OUT, { recursive: true });

const VIEWPORTS = [
  { w: 1280, h: 640 },
  { w: 1440, h: 900 },
];

/* ------------------------------------------------------------------------------------------------
 * 一、期望值：从源码读，不凭印象硬编码
 * ---------------------------------------------------------------------------------------------- */

/** 任务单（调用方给的清单）。仅用于与源码对照、报差异；断言一律以源码解析结果为准。 */
const BRIEF = {
  CORE: ["我的 AI", "工作台", "AI 创作", "AI 设置"],
  MORE: ["U盘工具盘", "AI 优化大师", "本地大模型", "进阶"],
  LAB: [],
};

/** 解析 Sidebar.tsx 里 CORE/MORE/LAB 三个数组**未被注释**的条目（注释行以 `//` 开头，不会命中）。 */
function parseSidebarSource() {
  const src = readFileSync(path.join(ROOT, "src/components/Sidebar.tsx"), "utf8");
  const groups = { CORE: [], MORE: [], LAB: [] };
  let cur = null;
  for (const line of src.split(/\r?\n/)) {
    const head = line.match(/^const (CORE|MORE|LAB): NavItem\[\] = \[/);
    if (head) {
      cur = head[1];
      continue;
    }
    if (cur && /^\];/.test(line)) {
      cur = null;
      continue;
    }
    if (!cur) continue;
    const it = line.match(/^\s*\{ id: "(\w+)", label: "([^"]+)", sub: "([^"]*)"/);
    if (it) groups[cur].push({ id: it[1], label: it[2], sub: it[3] });
  }
  return groups;
}
const SRC = parseSidebarSource();
const SRC_ALL = [...SRC.CORE, ...SRC.MORE, ...SRC.LAB];

/** PanelBoundary（panel 形态 / chrome 形态）与根 ErrorBoundary 渲染的兜底文案，读自
 *  src/components/PanelBoundary.tsx 与 src/components/ErrorBoundary.tsx。 */
const CRASH_MARKERS = [
  "这一块出问题了，其余功能仍可用", // PanelBoundary panel 形态标题
  "This panel crashed and was auto-reported", // PanelBoundary panel 形态英文行
  "出错了，已自动上报", // PanelBoundary chrome 形态（「xxx」出错了，已自动上报）
  "界面已停止", // 根 ErrorBoundary（U-King 遇到问题，界面已停止）
];

/* ------------------------------------------------------------------------------------------------
 * 二、shim 的假数据（只为让界面渲染出来；形状照抄 shot-toolhub.mjs / src/lib/types.ts）
 * ---------------------------------------------------------------------------------------------- */

const DEMO_DIR = "C:\\demo\\uking-mini";

/** 工具目录。前面照抄 shot-toolhub.mjs（它核对过 tools.rs），后 5 个是 1.3.5/1.3.6 新增的，
 *  字段按同一形状补（只为让「我的 AI」的「更多 AI 工具」折叠组有东西可渲染）。 */
const TOOL_DEFS = [
  { id: "claude-code", name: "Claude Code CLI", kind: "standalone", launch_cmd: "claude", launch_app: "", hidden: false, config_target: "claude" },
  { id: "codex", name: "Codex CLI", kind: "standalone", launch_cmd: "codex", launch_app: "", hidden: false, config_target: "codex" },
  { id: "openclaw", name: "OpenClaw CLI（龙虾）", kind: "deep", launch_cmd: "openclaw", launch_app: "", hidden: true, config_target: "clawx" },
  { id: "pi", name: "pi", kind: "standalone", launch_cmd: "pi", launch_app: "", hidden: false, config_target: "pi" },
  { id: "opencode", name: "OpenCode", kind: "standalone", launch_cmd: "opencode", launch_app: "", hidden: false, config_target: "opencode" },
  { id: "clawx", name: "OpenClaw 桌面版（ClawX）", kind: "deep", launch_cmd: "", launch_app: "clawx", hidden: false, config_target: "clawx" },
  { id: "hermes", name: "Hermes Agent（Nous 官方）", kind: "deep", launch_cmd: "hermes", launch_app: "", hidden: false, config_target: "hermes" },
  { id: "dsh", name: "DeepSeek Harness（官方桌面版）", kind: "deep", launch_cmd: "", launch_app: "dsh-desktop", hidden: false, config_target: "dsh" },
  { id: "uu-remote", name: "UU远程（手机控电脑）", kind: "standalone", launch_cmd: "", launch_app: "", hidden: false, config_target: null },
  { id: "codex-app", name: "Codex 桌面版", kind: "standalone", launch_cmd: "", launch_app: "codex-app", hidden: false, config_target: "codex" },
  { id: "uu-switch", name: "uu-switch 模型切换器", kind: "standalone", launch_cmd: "", launch_app: "uu-switch", hidden: true, config_target: null },
  // 1.3.5 / 1.3.6 新增（src-tauri/src/tools.rs TOOL_SPECS 里的 id；name/launch 字段是示意值）
  { id: "mimo-code", name: "MiMo Code", kind: "standalone", launch_cmd: "mimo", launch_app: "", hidden: false, config_target: null },
  { id: "codebuddy-code", name: "CodeBuddy Code", kind: "standalone", launch_cmd: "codebuddy", launch_app: "", hidden: false, config_target: null },
  { id: "qoder-cn", name: "Qoder CN CLI", kind: "standalone", launch_cmd: "qodercn", launch_app: "", hidden: false, config_target: null },
  { id: "claude-app", name: "Claude 桌面版", kind: "standalone", launch_cmd: "", launch_app: "claude-app", hidden: false, config_target: null },
  { id: "kimi-code", name: "Kimi Code", kind: "standalone", launch_cmd: "kimi", launch_app: "", hidden: false, config_target: null },
];
const INSTALLED_IDS = new Set(["claude-code", "codex", "hermes", "pi", "dsh"]);
const LAUNCH_MODE_BY_ID = {
  "claude-code": "embedded_pty", codex: "embedded_pty", pi: "embedded_pty", opencode: "embedded_pty",
  clawx: "gui_app", hermes: "route_tab", dsh: "gui_app",
};
const TOOLS = TOOL_DEFS.map((d) => ({
  id: d.id, name: d.name, summary: "", kind: d.kind,
  installed: INSTALLED_IDS.has(d.id),
  action: "install", target: "", launch_cmd: d.launch_cmd, launch_app: d.launch_app,
  hidden: d.hidden, config_target: d.config_target,
}));

const PROVIDERS = [
  { id: "xiapan", name: "虾盘云", summary: "内置 Key，开箱即用", openai_base: "https://api.u-claw.org.cn/v1", anthropic_base: null, model: "deepseek-v4-pro", small_model: "deepseek-v4-flash", key_url: "", key_hint: "API Key", builtin_recharge: true, recommended: true, builtin: true, api_key: "sk-***" },
  { id: "official", name: "官方直连", summary: "用你自己的 Key", openai_base: "", anthropic_base: null, model: "", small_model: "", key_url: "https://console.anthropic.com", key_hint: "API Key", builtin_recharge: false, recommended: false, builtin: true, api_key: "" },
  { id: "deepseek", name: "DeepSeek 官方", summary: "官方直连，自备 Key", openai_base: "https://api.deepseek.com/v1", anthropic_base: "https://api.deepseek.com/anthropic", model: "deepseek-chat", small_model: "deepseek-chat", key_url: "https://platform.deepseek.com", key_hint: "API Key", builtin_recharge: false, recommended: false, builtin: true, api_key: "" },
];

const DRIVER = {
  claude_base: "https://api.u-claw.org.cn/v1", claude_model: "deepseek-v4-pro",
  codex_provider: "official", codex_model: "gpt-5.1-codex-max",
  clawx_model: null, clawx_installed: false, hermes_model: "deepseek-v4-flash", hermes_installed: true,
  dsh_model: null, dsh_installed: false,
  active: { claude: "xiapan", codex: "official", hermes: "xiapan" },
  discovered: [
    { name: "claude", path: "C:/Users/demo/AppData/Roaming/npm/claude.cmd", source: "machine", version: "1.2.3", configured: true },
    { name: "codex", path: "C:/Users/demo/AppData/Roaming/npm/codex.cmd", source: "machine", version: "0.45.0", configured: true },
    { name: "hermes", path: "C:/Users/demo/.local/bin/hermes.exe", source: "portable", version: "2.1.0", configured: true },
    { name: "pi", path: "C:/Users/demo/AppData/Roaming/npm/pi.cmd", source: "machine", version: "0.9.0", configured: true },
  ],
};

const DEVICE_KEY = {
  key: "sk-xp-demo", recharge_url: "https://u-claw.org.cn/recharge",
  balance: { tokens: 128500, cny: 128.5, text: "¥128.50" }, charged: true,
};

const ENV = {
  running_from_local: true, install_dir: "C:/Users/demo/AppData/Local/u-king",
  context_menu_registered: false, opened_dir: null, platform: "windows", home_dir: "C:/Users/demo",
};

/** 工作台里要有一个带 dir 的会话，ChatPanel 才会渲染（`!workspace` 时是「选工作文件夹」空态）。 */
const TASKS = [
  {
    id: "sess-demo-1", name: "uking-mini", dir: DEMO_DIR, status: "idle", source: "manual",
    assignee: null, external_ref: null, last_opened_at: 1760000000000, created_at: 1760000000000,
    kind: "task", project: DEMO_DIR.toLowerCase(),
  },
];

const today = new Date();
const daily = Array.from({ length: 14 }, (_, i) => {
  const d = new Date(today.getTime() - (13 - i) * 86400000);
  return { date: d.toISOString().slice(0, 10), tokens: 20000 + ((i * 7919) % 90000) };
});
const USAGE_TREND = { daily, today_tokens: 48200, week_tokens: 512300, samples: 14 };

const totals = (cny, calls, i, o) => ({ cny, calls, input_tokens: i, output_tokens: o, tokens: i + o });
const METER = {
  days: 7, ready: true, blockers: [],
  window: totals(12.6, 340, 4200000, 380000), today: totals(1.8, 41, 520000, 48000),
  yesterday: totals(2.1, 52, 610000, 55000), last7: totals(12.6, 340, 4200000, 380000),
  daily: daily.slice(-7).map((d) => ({ date: d.date, cny: d.tokens / 10000, tokens: d.tokens, calls: 20 })),
  by_model: [{ model: "deepseek-v4-pro", tool: "claude", cny: 9.8, count: 280, input_tokens: 3600000, output_tokens: 300000 }],
  by_tool: [{ name: "claude", detail: "Claude Code", cny: 9.8, tokens: 3900000, calls: 280, share: 0.78 }],
  by_project: [{ name: "uking-mini", detail: DEMO_DIR, cny: 9.8, tokens: 3900000, calls: 280, share: 0.78 }],
  cache: { non_cached_input: 900000, cache_read: 3000000, cache_creation: 300000, hit_rate: 0.71, saved_cny: 4.2 },
  pace: { daily_avg_cny: 1.8, month_projection_cny: 54, today_vs_avg: 1, days_left: 71, balance_cny: 128.5 },
  tips: [],
  sources: [
    { tool: "claude", label: "Claude Code", dir: "C:/Users/demo/.claude/projects", exists: true, countable: true, enabled: true, subscription: false, covered: true, files: 12, note: "" },
    { tool: "codex", label: "Codex CLI", dir: "C:/Users/demo/.codex/sessions", exists: true, countable: true, enabled: true, subscription: false, covered: true, files: 3, note: "" },
  ],
  events: [], events_meta: { total: 0, returned: 0, truncated: 0, returned_cny: 0 },
};

/** `airuntime_doctor` 真实后端返回的是 **JSON 字符串**（AiRuntime.tsx: `JSON.parse(await invoke<string>(...))`）。
 *  第一版 shim 没给它，落到 default 的 null → `JSON.parse(null)` 得 null → `reportScore(null)` 读 `d.score`
 *  抛未处理的 Promise rejection —— 那是 shim 缺数据造成的假错误（真后端永远返回对象），不是收敛引入的回归，
 *  所以补一份形状正确的假数据；这个事实也写进报告，免得被当成「错误被抹掉了」。 */
const AIRUNTIME_DOCTOR = JSON.stringify({
  tool: "ukrt", version: "1.0.0", score: 76, earned: 76, total: 100,
  checks: [
    { id: "git", category: "基础", name: "Git", status: "pass", points: 5, earned: 5, bonus: 0, detail: "2.45.0", fix_hint: "" },
    { id: "node", category: "基础", name: "Node.js", status: "pass", points: 5, earned: 5, bonus: 0, detail: "v22.1.0", fix_hint: "" },
    { id: "npmrc", category: "省Token", name: "npm 精简输出", status: "warn", points: 5, earned: 0, bonus: 0, detail: "未设置", fix_hint: "npm config set loglevel error" },
  ],
});

const SHIM_DATA = {
  airuntimeDoctor: AIRUNTIME_DOCTOR,
  tools: TOOLS, driver: DRIVER, providers: PROVIDERS, deviceKey: DEVICE_KEY, env: ENV, tasks: TASKS,
  usageTrend: USAGE_TREND, meter: METER,
  launchPlans: TOOL_DEFS.map((d) => ({ tool_id: d.id, mode: LAUNCH_MODE_BY_ID[d.id] ?? "none" })),
  checkUpdate: { current: "1.3.7", latest: "1.3.7", has_update: false, checked_ok: true, notes: "", download_url: "" },
  setupState: { has_tool: true, has_driver: true, charged: true, clawx_needs_xiapan: false, next_step: "done", hint: "" },
  detectStack: { claude: { found: true, version: "1.2.3", path: "C:/Users/demo/AppData/Roaming/npm/claude.cmd" }, codex: { found: true, version: "0.45.0" } },
};

/**
 * 注入页面的 shim —— 替换 `window.__TAURI_INTERNALS__.invoke`。
 * 每次导航都会重跑（addInitScript 的语义），所以计数器挂在 window 上、由 runner 在关页前读走。
 *  · window.__SHIM_CALLS      每个命令被调了几次
 *  · window.__SHIM_UNCOVERED  落到 default 分支（= 本 shim 没有给假数据）的命令，key 形如
 *                             `cmd` 或 `action_parity_call(<action_id>)`，值 = 次数
 *  · window.__SHIM_REPORTS    `report_bug` 的入参（PanelBoundary / 根边界 / 全局 error 都会上报）
 * `plugin:event|*` 视为 Tauri 事件基础设施（返回订阅 id），不算「未覆盖」；同 shot-toolhub.mjs。
 */
const SHIM = (D) => {
  const calls = (window.__SHIM_CALLS = {});
  const uncovered = (window.__SHIM_UNCOVERED = {});
  window.__SHIM_REPORTS = [];
  const bump = (m, k) => {
    m[k] = (m[k] || 0) + 1;
  };
  const NONE = { __none: true };
  const fake = (cmd, args) => {
    switch (cmd) {
      case "get_env": return D.env;
      case "list_tools": return D.tools;
      case "get_driver_status": return D.driver;
      case "get_device_key": return D.deviceKey;
      case "list_providers": return D.providers;
      case "check_update": return D.checkUpdate;
      case "get_setup_state": return D.setupState;
      case "term_snapshot_pending": return null;
      case "take_update_flag": return false;
      case "instance_role": return { role: "primary" };
      case "list_tasks": return D.tasks;
      case "upsert_task": return args?.task ?? null;
      case "detect_stack": return D.detectStack;
      case "get_usage_trend": return D.usageTrend;
      case "query_usage_meter": return D.meter;
      case "airuntime_doctor": return D.airuntimeDoctor;
      // lib.rs: `async fn fetch_optimize_advice() -> Vec<advice::Advice>` —— 真后端永远是数组（失败也返回空数组）。
      // 落到 default 的 null 会让 AiRuntime 的 `advice.length` 抛错、整页被 PanelBoundary 接走：那是 shim 的锅。
      case "fetch_optimize_advice": return [];
      case "report_bug":
        window.__SHIM_REPORTS.push({ kind: args?.kind, summary: args?.summary });
        return null;
      case "action_parity_call": {
        const id = args?.request?.action_id;
        if (id === "runtime.tool.inspect") {
          return { ok: true, version: 1, action_id: id, execution_id: "shim", result: { tools: D.launchPlans } };
        }
        return NONE;
      }
      default:
        if (cmd?.startsWith("plugin:event|")) return 1;
        return NONE;
    }
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      bump(calls, cmd);
      let v = fake(cmd, args);
      if (v === NONE) {
        const key = cmd === "action_parity_call" ? `action_parity_call(${args?.request?.action_id})` : cmd;
        bump(uncovered, key);
        // 「合理空值」：list_* / *_list 给空数组，其余给 null（同其他 shot-*.mjs）。
        v = /^list_|_list$/.test(cmd) ? [] : null;
      }
      return Promise.resolve(v);
    },
    convertFileSrc: (p) => "https://asset.localhost/" + encodeURIComponent(p),
    transformCallback: (cb) => {
      const id = Math.floor(Math.random() * 1e9);
      window[`_${id}`] = cb;
      return id;
    },
    // 🔴 `metadata` 少了会在 `getCurrentWindow()` 里炸（shot-manager-split.mjs 的教训）。
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
};

/* ------------------------------------------------------------------------------------------------
 * 三、页内量具（page.evaluate 里跑）
 * ---------------------------------------------------------------------------------------------- */

/** 扫侧栏 nav：条目（带 13px 标题 div 的按钮）与折叠组开关（没有该 div 的按钮）按 DOM 顺序平铺。 */
const SCAN_SIDEBAR = () => {
  const nav = document.querySelector("aside nav");
  if (!nav) return { error: "找不到 `aside nav`" };
  const navRect = nav.getBoundingClientRect();
  const seq = [];
  for (const b of nav.querySelectorAll("button")) {
    const labelEl = b.querySelector("div.text-\\[13px\\]");
    const r = b.getBoundingClientRect();
    const cs = getComputedStyle(b);
    const visible = r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.display !== "none";
    if (labelEl) {
      seq.push({
        type: "entry",
        label: (labelEl.textContent || "").trim(),
        sub: (b.querySelector("div.text-\\[12px\\]")?.textContent || "").trim() || null,
        active: b.className.includes("border-accent"),
        visible,
        inNavView: r.top >= navRect.top - 0.5 && r.bottom <= navRect.bottom + 0.5,
        belowNavBottomPx: Math.round((r.bottom - navRect.bottom) * 10) / 10,
      });
    } else {
      seq.push({ type: "toggle", text: (b.textContent || "").trim(), visible });
    }
  }
  return {
    seq,
    navClientH: nav.clientHeight,
    navScrollH: nav.scrollHeight,
    navScrollable: nav.scrollHeight > nav.clientHeight + 1,
    navOverflowX: nav.scrollWidth > nav.clientWidth,
    asideW: Math.round(nav.closest("aside").getBoundingClientRect().width),
    short: window.matchMedia("(max-height: 779px)").matches,
    narrow: window.matchMedia("(max-width: 1280px)").matches,
  };
};

/** 当前页状态：可见 <main> 的文字量、横向溢出（文档 + 真正的滚动容器）、崩溃兜底文案。 */
const PAGE_STATE = (markers) => {
  const mains = [...document.querySelectorAll("main")].filter((m) => getComputedStyle(m).display !== "none");
  const m = mains[0] ?? null;
  const de = document.documentElement;
  const nav = document.querySelector("aside nav");
  const body = document.body.innerText || "";
  return {
    visibleMains: mains.length,
    mainTextLen: m ? (m.innerText || "").replace(/\s+/g, "").length : 0,
    mainScrollW: m ? m.scrollWidth : null,
    mainClientW: m ? m.clientWidth : null,
    mainOverflowX: m ? m.scrollWidth > m.clientWidth : null,
    docScrollW: de.scrollWidth,
    docClientW: de.clientWidth,
    docOverflowX: de.scrollWidth > de.clientWidth,
    bodyOverflowX: document.body.scrollWidth > document.body.clientWidth,
    navOverflowX: nav ? nav.scrollWidth > nav.clientWidth : null,
    crashMarkers: markers.filter((s) => body.includes(s)),
    activeEntries: nav
      ? [...nav.querySelectorAll("button")]
          .filter((b) => b.querySelector("div.text-\\[13px\\]") && b.className.includes("border-accent"))
          .map((b) => (b.querySelector("div.text-\\[13px\\]").textContent || "").trim())
      : [],
  };
};

/** 工作台顶栏「大脑」区：所有可见的 title="用哪个大脑干这活" 元素。 */
const SCAN_BRAIN = () => {
  const els = [...document.querySelectorAll('[title="用哪个大脑干这活"]')].filter((e) => {
    const r = e.getBoundingClientRect();
    const cs = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && cs.visibility !== "hidden";
  });
  const main = [...document.querySelectorAll("main")].find((x) => getComputedStyle(x).display !== "none");
  return {
    labels: els.map((e) => {
      const region = e.closest("div.rounded-xl") || e.parentElement;
      return {
        tag: e.tagName,
        text: (e.textContent || "").trim(),
        selfIsSelect: e.tagName === "SELECT",
        hasSelectInside: !!e.querySelector("select"),
        regionHasSelect: !!region?.querySelector("select"),
        regionSelects: region ? region.querySelectorAll("select").length : null,
        outerHTML: e.outerHTML.slice(0, 260),
      };
    }),
    visibleSelectsInWorkspace: main ? [...main.querySelectorAll("select")].filter((s) => s.getBoundingClientRect().width > 0).length : null,
  };
};

/** 对话 / 终端 两颗分段按钮的当前态（激活 = class 含 bg-accent/[0.16]）+ xterm 是否挂载。 */
const SCAN_PANE = () => {
  const btn = (title) => {
    const b = [...document.querySelectorAll(`button[title="${title}"]`)].find((x) => {
      const r = x.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    });
    return b ? { found: true, active: b.className.includes("bg-accent/[0.16]"), disabled: b.disabled } : { found: false };
  };
  const xterms = [...document.querySelectorAll(".xterm")].filter((x) => x.getBoundingClientRect().width > 0);
  return { chat: btn("U-Chat（对话）"), cli: btn("U-CLI（终端）"), xtermVisible: xterms.length };
};

/* ------------------------------------------------------------------------------------------------
 * 四、runner
 * ---------------------------------------------------------------------------------------------- */

const problems = [];
const notes = [];
const shots = [];
const errorLog = []; // { vp, scenario, kind, text }
const uncoveredAll = new Map(); // key -> { count, vps:Set }
const callsAll = new Map(); // cmd -> count （全部视口合计）
const stepRows = []; // 给 report.json
let curVp = "";
let curScenario = "";

const log = (s) => console.log(s);
function check(cond, desc, detail = "", quiet = false) {
  const tag = `[${curVp}] ${curScenario}`;
  if (cond) {
    if (!quiet) log(`  ok    ${desc}`);
  } else {
    const line = `${tag}: ${desc}${detail ? ` :: ${detail}` : ""}`;
    problems.push(line);
    log(`  FAIL  ${desc}${detail ? ` :: ${detail}` : ""}`);
  }
  stepRows.push({ vp: curVp, scenario: curScenario, desc, ok: !!cond, detail });
  return !!cond;
}
function note(s) {
  notes.push(`[${curVp}] ${curScenario}: ${s}`);
  log(`  note  ${s}`);
}

/** 一个场景：设置当前场景名（错误归属用）；里面抛的任何异常（元素找不到、超时）都变成 problem，不中断后续场景。 */
async function scenario(name, fn) {
  curScenario = name;
  log(`- ${name}`);
  try {
    await fn();
  } catch (e) {
    const msg = String(e?.message || e).split("\n")[0];
    problems.push(`[${curVp}] ${name}: 场景抛异常 :: ${msg}`);
    log(`  FAIL  场景抛异常 :: ${msg}`);
    stepRows.push({ vp: curVp, scenario: name, desc: "场景抛异常", ok: false, detail: msg });
  }
}

async function shot(page, name) {
  const file = path.join(OUT, `${curVp}-${name}.png`);
  await page.screenshot({ path: file });
  shots.push(file);
}

/** 等「当前可见 <main>」里出现文字（懒加载页 Suspense 的转圈没有文字），再让异步数据落一拍。 */
async function settle(page, ms = 500) {
  await page
    .waitForFunction(
      () => {
        const m = [...document.querySelectorAll("main")].find((x) => getComputedStyle(x).display !== "none");
        return !!m && (m.innerText || "").trim().length > 0;
      },
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  await page.waitForTimeout(ms);
}

function entryLocator(page, label) {
  const esc = label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return page.locator("aside nav button").filter({
    has: page.locator("div.text-\\[13px\\]").filter({ hasText: new RegExp(`^${esc}$`) }),
  });
}
async function clickEntry(page, label) {
  const loc = entryLocator(page, label);
  const n = await loc.count();
  if (n !== 1) throw new Error(`侧栏入口「${label}」匹配到 ${n} 个按钮（应恰好 1 个）`);
  await loc.first().click();
}
async function expandGroup(page, text) {
  const loc = page.locator("aside nav button").filter({ hasText: text }).filter({ hasNot: page.locator("div.text-\\[13px\\]") });
  const n = await loc.count();
  if (n !== 1) throw new Error(`折叠组开关「${text}」匹配到 ${n} 个（应恰好 1 个）`);
  await loc.first().click();
  await page.waitForTimeout(150);
}

/** 对当前页做通用断言：非空 / 单一可见 main / 无崩溃文案 / 无横向溢出（文档与滚动容器）。 */
async function assertPageSane(page, what, st) {
  if (MUTATE === "overflow") {
    await page.evaluate(() => {
      const m = [...document.querySelectorAll("main")].find((x) => getComputedStyle(x).display !== "none");
      const d = document.createElement("div");
      d.style.cssText = "width:4000px;height:10px";
      m?.appendChild(d);
    });
  }
  st ??= await page.evaluate(PAGE_STATE, CRASH_MARKERS);
  // 通过的断言不逐条刷屏，合成一行「事实」（数字就是证据）；失败的照常逐条 FAIL。
  check(st.visibleMains === 1, `${what}: 恰好 1 个可见 <main>`, `实际 ${st.visibleMains}`, true);
  check(st.mainTextLen > 0, `${what}: 主区非空`, `可见文字 ${st.mainTextLen} 字符`, true);
  check(st.crashMarkers.length === 0, `${what}: 无崩溃兜底文案`, st.crashMarkers.join(" | "), true);
  check(!st.docOverflowX, `${what}: documentElement 无横向溢出`, `scrollWidth=${st.docScrollW} clientWidth=${st.docClientW}`, true);
  check(!st.bodyOverflowX, `${what}: body 无横向溢出`, "", true);
  check(st.mainOverflowX === false, `${what}: 可见 <main>（真正的滚动容器）无横向溢出`, `scrollWidth=${st.mainScrollW} clientWidth=${st.mainClientW}`, true);
  check(st.navOverflowX === false, `${what}: 侧栏 nav 无横向溢出`, "", true);
  log(`  sane  ${what}: 可见main=${st.visibleMains} 文字=${st.mainTextLen}字符 崩溃文案=${st.crashMarkers.length} 溢出[doc ${st.docScrollW}/${st.docClientW} · main ${st.mainScrollW}/${st.mainClientW} · nav ${st.navOverflowX ? "是" : "否"}]（scrollWidth/clientWidth）`);
  return st;
}

async function readShimCounters(page) {
  return page.evaluate(() => ({
    calls: window.__SHIM_CALLS || {},
    uncovered: window.__SHIM_UNCOVERED || {},
    reports: window.__SHIM_REPORTS || [],
  }));
}

async function newProbePage(browser, vp) {
  const ctx = await browser.newContext({ viewport: { width: vp.w, height: vp.h }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  page.setDefaultTimeout(10000);
  page.on("dialog", (d) => d.dismiss().catch(() => {}));
  await page.addInitScript(SHIM, SHIM_DATA);
  const mutateModule = (urlRe, from, to) =>
    page.route(urlRe, async (route) => {
      const res = await route.fetch();
      const body = await res.text();
      if (!body.includes(from)) throw new Error(`变异自检：在 ${route.request().url()} 里找不到要替换的片段 ${JSON.stringify(from)}`);
      await route.fulfill({ response: res, body: body.replace(from, to) });
    });
  if (MUTATE === "brain") await mutateModule(/\/src\/opencodex\/Chat\.tsx/, 'PICKABLE_ENGINES = ["claude"]', 'PICKABLE_ENGINES = ["claude", "uking"]');
  if (MUTATE === "sidebar") await mutateModule(/\/src\/components\/Sidebar\.tsx/, "const LAB = [", 'const LAB = [ { id: "backup", label: "备份/同步", sub: "x", icon: HardDrive },');
  return { ctx, page };
}

async function bootApp(page) {
  // vite dev 首次冷启动要现场转译几百个模块，goto / 首屏可能要 20s+（2026-10-04 实测），超时给足。
  await page.goto(URL, { waitUntil: "domcontentloaded", timeout: 120000 });
  // 首屏落点是无条件 setTab("toolhub")（App.tsx refresh()），以 h1「我的 AI」为就绪信号。
  await page.locator("h1", { hasText: "我的 AI" }).first().waitFor({ state: "visible", timeout: 90000 });
  await settle(page, 600);
}

/** 预热：把所有要点到的懒加载页先走一遍（结果全丢），免得 vite 首次发现依赖时 optimize + 整页重载打断正式测量。 */
async function warmUp(browser) {
  log("预热（结果丢弃）：让 vite 先把各懒加载页的依赖优化完 ...");
  const { ctx, page } = await newProbePage(browser, { w: 1440, h: 900 });
  page.on("pageerror", () => {});
  try {
    await bootApp(page);
    await expandGroup(page, "更多");
    for (const it of SRC_ALL) {
      await clickEntry(page, it.label).catch(() => {});
      await settle(page, 700);
    }
    await clickEntry(page, "AI 设置").catch(() => {});
    await settle(page, 500);
    for (const id of ["account", "usage", "providers", "free", "advanced", "tools"]) {
      await page.locator(`[data-testid="manager-subtab-${id}"]`).first().click().catch(() => {});
      await page.waitForTimeout(600);
    }
    await clickEntry(page, "工作台").catch(() => {});
    await settle(page, 800);
    await page.locator('button[title="U-CLI（终端）"]:visible').first().click().catch(() => {});
    await page.waitForTimeout(1500);
    await clickEntry(page, "我的 AI").catch(() => {});
    await page.locator('[data-testid="toolhub-go-doctor"]').first().click().catch(() => {});
    await page.waitForTimeout(1200);
  } catch (e) {
    log(`  （预热出错，忽略：${String(e?.message || e).split("\n")[0]}）`);
  } finally {
    await ctx.close();
  }
}

async function runViewport(browser, vp) {
  curVp = `${vp.w}x${vp.h}`;
  curScenario = "boot";
  log(`\n===== 视口 ${curVp} =====`);
  const { ctx, page } = await newProbePage(browser, vp);

  // —— 全程监听：console.error / pageerror 逐条留原文，归属到触发时的场景 ——
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      const loc = msg.location();
      errorLog.push({ vp: curVp, scenario: curScenario, kind: "console.error", text: msg.text() + (loc?.url ? `  @ ${loc.url}:${loc.lineNumber ?? "?"}` : "") });
    }
  });
  page.on("pageerror", (err) => {
    errorLog.push({ vp: curVp, scenario: curScenario, kind: "pageerror", text: String(err?.stack || err?.message || err) });
  });
  const netNotes = [];
  page.on("requestfailed", (req) => netNotes.push(`${curScenario}: requestfailed ${req.method()} ${req.url()} :: ${req.failure()?.errorText}`));
  page.on("response", (res) => {
    if (res.status() >= 400) netNotes.push(`${curScenario}: HTTP ${res.status()} ${res.url()}`);
  });
  let navCount = 0;
  page.on("framenavigated", (f) => {
    if (f === page.mainFrame()) navCount += 1;
  });

  try {
    /* ---- 启动 ---- */
    await scenario("boot", async () => {
      await bootApp(page);
      const st = await assertPageSane(page, "首屏(我的 AI)");
      check(st.activeEntries.length === 1 && st.activeEntries[0] === "我的 AI", "首屏侧栏高亮「我的 AI」", JSON.stringify(st.activeEntries));
      const c = await readShimCounters(page);
      note(`首屏 doctor_report 调用 ${c.calls.doctor_report ?? 0} 次（1.3.7 声明「首页不触发体检」）`);
    });

    /* ---- 1. 侧栏入口恰好 8 项 ---- */
    await scenario("sidebar", async () => {
      // 源码侧：数量与对照
      check(SRC.CORE.length === 4 && SRC.MORE.length === 4 && SRC.LAB.length === 0, "源码 Sidebar.tsx 解析：核心 4 + 更多 4 + 实验室 0（空组不渲染）",`核心 ${SRC.CORE.length} / 更多 ${SRC.MORE.length} / 实验室 ${SRC.LAB.length}`);
      for (const g of ["CORE", "MORE", "LAB"]) {
        const src = SRC[g].map((x) => x.label);
        if (JSON.stringify(src) !== JSON.stringify(BRIEF[g])) {
          note(`源码与任务清单不一致 ${g}: 源码=${JSON.stringify(src)} 任务清单=${JSON.stringify(BRIEF[g])}（断言以源码为准）`);
        }
      }
      const labelsOf = (scan) => scan.seq.filter((s) => s.type === "entry").map((s) => s.label);
      const want = (...gs) => gs.flatMap((g) => SRC[g].map((x) => x.label));

      let scan = await page.evaluate(SCAN_SIDEBAR);
      if (scan.error) throw new Error(scan.error);
      check(JSON.stringify(labelsOf(scan)) === JSON.stringify(want("CORE")), "首屏（未展开）可见入口 = 核心 4 项", JSON.stringify(labelsOf(scan)));
      const clippedAtFirstScreen = scan.seq.filter((s) => s.type === "entry" && !s.inNavView).map((s) => s.label);
      check(clippedAtFirstScreen.length === 0, "首屏（未展开）4 项都在 nav 可视区内，不需要滚动", `被裁掉: ${JSON.stringify(clippedAtFirstScreen)}`);
      const toggles = scan.seq.filter((s) => s.type === "toggle").map((s) => s.text);
      check(toggles.length === 1 && toggles[0] === "更多", "折叠组开关恰为「更多」（「实验室」组空，不渲染）", JSON.stringify(toggles));

      await expandGroup(page, "更多");
      scan = await page.evaluate(SCAN_SIDEBAR);
      const labels = labelsOf(scan);
      check(JSON.stringify(labels) === JSON.stringify(want("CORE", "MORE")), "展开「更多」后 = 核心 4 + 更多 4", JSON.stringify(labels));
      check(labels.length === 8, "全部展开后可见入口恰好 8 项", `实际 ${labels.length}: ${JSON.stringify(labels)}`);
      check(JSON.stringify(labels) === JSON.stringify(want("CORE", "MORE", "LAB")), "8 项文案与顺序 = 源码 CORE+MORE+LAB", JSON.stringify(labels));
      const seqShape = scan.seq.map((s) => (s.type === "entry" ? "E" : "T")).join("");
      check(seqShape === "EEEETEEEE", "DOM 次序 = 4 项 / 更多开关 / 4 项", seqShape);
      check(scan.seq.filter((s) => s.type === "entry").every((s) => s.visible), "8 项都有非零尺寸、非 hidden");
      // 「全部展开后 8 项是否都在 nav 可视区内」只记录、不判红：是否可接受由调用方判断，这里给数字。
      const outOfView = scan.seq.filter((s) => s.type === "entry" && !s.inNavView).map((s) => `${s.label}(底边超出 nav ${s.belowNavBottomPx}px)`);
      note(`全部展开后 nav clientH=${scan.navClientH} scrollH=${scan.navScrollH} 需滚动=${scan.navScrollable}；不在 nav 可视区内的条目=${JSON.stringify(outOfView)}；aside 宽 ${scan.asideW}px；short=${scan.short} narrow=${scan.narrow}`);
      check(scan.navOverflowX === false, "侧栏 nav 无横向溢出");
      await shot(page, "sidebar-expanded");
    });

    /* ---- 2. 逐个点 8 项 ---- */
    let i = 0;
    for (const it of SRC_ALL) {
      i += 1;
      await scenario(`nav-${String(i).padStart(2, "0")}-${it.id}（${it.label}）`, async () => {
        await clickEntry(page, it.label);
        await settle(page, 600);
        const st = await assertPageSane(page, it.label);
        check(st.activeEntries.length === 1 && st.activeEntries[0] === it.label, `点击后侧栏高亮「${it.label}」`, JSON.stringify(st.activeEntries));
        if (it.id === "toolhub") {
          check((await page.locator("h1", { hasText: "我的 AI" }).count()) >= 1, "「我的 AI」页有 h1 标题");
        }
        await shot(page, `nav-${String(i).padStart(2, "0")}-${it.id}`);
      });
    }

    /* ---- 3. 我的 AI ⇄ 体检 · 升级 ---- */
    await scenario("myai-doctor-subpage", async () => {
      await clickEntry(page, "我的 AI");
      await settle(page, 600);
      const link = page.locator('[data-testid="toolhub-go-doctor"]');
      check((await link.count()) === 1, "「我的 AI」页有 toolhub-go-doctor 链接", `匹配 ${await link.count()} 个`);
      const txt = ((await link.first().textContent()) || "").trim();
      check(txt.includes("体检 · 升级"), "链接文案含「体检 · 升级」", JSON.stringify(txt));
      const box = await link.first().boundingBox();
      check(!!box, "链接可见（有 boundingBox）");
      if (box) {
        const cx = box.x + box.width / 2;
        check(cx > vp.w * 0.5 && box.y < vp.h * 0.3, "链接在页面右上区域（中心 x > 50% 视口宽，y < 30% 视口高）", `box=${JSON.stringify(box)}`);
      }
      const before = (await readShimCounters(page)).calls.doctor_report ?? 0;
      await link.first().click();
      await page.locator('[data-testid="myai-back-to-hub"]').first().waitFor({ state: "visible", timeout: 10000 });
      await settle(page, 800);
      const st = await assertPageSane(page, "体检 · 升级 子页");
      check(st.activeEntries.length === 1 && st.activeEntries[0] === "我的 AI", "子页里侧栏仍高亮「我的 AI」", JSON.stringify(st.activeEntries));
      const back = (await page.locator('[data-testid="myai-back-to-hub"]').first().textContent())?.trim() ?? "";
      check(back.includes("返回我的 AI"), "子页有「← 返回我的 AI」", JSON.stringify(back));
      const after = (await readShimCounters(page)).calls.doctor_report ?? 0;
      note(`doctor_report 调用：进子页前 ${before} 次 → 进子页后 ${after} 次`);
      await shot(page, "myai-doctor");

      await page.locator('[data-testid="myai-back-to-hub"]').first().click();
      await page.locator("h1", { hasText: "我的 AI" }).first().waitFor({ state: "visible", timeout: 10000 });
      await settle(page, 500);
      check((await page.locator('[data-testid="toolhub-go-doctor"]').count()) === 1, "返回后回到「我的 AI」首页（toolhub-go-doctor 又出现）");
      check((await page.locator('[data-testid="myai-back-to-hub"]').count()) === 0, "返回后子页的返回链接已消失");
      await assertPageSane(page, "返回后的「我的 AI」");
      await shot(page, "myai-back");
    });

    /* ---- 4. AI 设置：账号 · 充值 / 用量账单 / Token 水电表 ---- */
    await scenario("manage-subtabs", async () => {
      await clickEntry(page, "AI 设置");
      await page.locator('[data-testid="manager-subtab-account"]').first().waitFor({ state: "visible", timeout: 10000 });
      await settle(page, 600);
      const acc = page.locator('[data-testid="manager-subtab-account"]');
      const use = page.locator('[data-testid="manager-subtab-usage"]');
      check((await acc.count()) === 1 && ((await acc.first().textContent()) || "").includes("账号 · 充值"), "存在子 tab「账号 · 充值」(manager-subtab-account)");
      check((await use.count()) === 1 && ((await use.first().textContent()) || "").includes("用量账单"), "存在子 tab「用量账单」(manager-subtab-usage)");
      await assertPageSane(page, "AI 设置(默认子 tab)");
      await shot(page, "manage-default");

      await acc.first().click();
      await settle(page, 900);
      const st1 = await assertPageSane(page, "AI 设置 → 账号 · 充值");
      check(st1.mainTextLen > 0, "账号 · 充值 子 tab 有内容");
      await shot(page, "manage-account");

      await use.first().click();
      await page.getByText("Token 水电表", { exact: false }).first().waitFor({ state: "visible", timeout: 10000 });
      await settle(page, 1200);
      await assertPageSane(page, "AI 设置 → 用量账单");
      const meterSummary = page.locator("details > summary", { hasText: "Token 水电表" });
      const billSummary = page.locator("details > summary", { hasText: "用量账单" });
      check((await meterSummary.count()) === 1, "用量账单子 tab 里有「Token 水电表」折叠块（源码文案「Token 水电表 · 所有 AI 工具」）", `匹配 ${await meterSummary.count()} 个`);
      check((await billSummary.count()) === 1, "用量账单子 tab 里有「用量账单 · 钱花在哪了」折叠块", `匹配 ${await billSummary.count()} 个`);
      if ((await meterSummary.count()) === 1) {
        const info = await meterSummary.first().evaluate((s) => {
          const d = s.parentElement;
          return {
            open: d.open,
            summaryText: (s.textContent || "").trim(),
            bodyTextLen: ((d.innerText || "").replace((s.innerText || ""), "")).replace(/\s+/g, "").length,
            loadingStill: (d.innerText || "").includes("加载中"),
          };
        });
        check(info.open, "Token 水电表块默认展开(details[open])");
        check(info.bodyTextLen > 0 && !info.loadingStill, "Token 水电表块有内容且不再停在「加载中…」", JSON.stringify(info));
      }
      const c = await readShimCounters(page);
      note(`query_usage_meter 调用 ${c.calls.query_usage_meter ?? 0} 次（水电表数据是 shim 里编的，只证明挂载/渲染，不证明口径）`);
      await shot(page, "manage-usage");
    });

    /* ---- 5. 工作台：对话 ⇄ 终端 + 大脑静态标签 ---- */
    await scenario("workspace", async () => {
      await clickEntry(page, "工作台");
      await page.locator('button[title="U-CLI（终端）"]:visible').first().waitFor({ state: "visible", timeout: 15000 });
      await settle(page, 1200);
      await assertPageSane(page, "工作台(对话态)");

      let pane = await page.evaluate(SCAN_PANE);
      check(pane.chat.found && pane.cli.found, "顶栏有「对话」「终端」两颗分段按钮", JSON.stringify(pane));
      check(pane.cli.found && pane.cli.disabled === false, "「终端」按钮可点（会话有 dir，未 disabled）");
      check(pane.chat.active === true && pane.cli.active === false, "初始是对话态（对话=激活，终端=未激活）", JSON.stringify(pane));

      // 「无专家」：Chat 顶栏标题在无专家时是 "U-Workspace"，有专家时是专家名（Chat.tsx: `expert ? expert.name : "U-Workspace"`）。
      const wsText = await page.evaluate(() => {
        const m = [...document.querySelectorAll("main")].find((x) => getComputedStyle(x).display !== "none");
        return m ? m.innerText : "";
      });
      check(wsText.includes("U-Workspace"), "当前会话是无专家会话（顶栏标题为 U-Workspace，不是专家名）");

      // 大脑区
      const brain = await page.evaluate(SCAN_BRAIN);
      check(brain.labels.length === 1, '对话态有且仅有 1 个可见 [title="用哪个大脑干这活"] 元素', `实际 ${brain.labels.length}`);
      const b = brain.labels[0];
      if (b) {
        check(b.tag !== "SELECT" && !b.selfIsSelect, "大脑元素本身不是 <select>", b.outerHTML);
        check(!b.hasSelectInside, "大脑元素内部没有 <select>", b.outerHTML);
        check(b.regionHasSelect === false, "大脑所在输入卡片区域里没有任何 <select>", `区域内 select 数 ${b.regionSelects}`);
        check(b.text.includes("Claude Code"), "大脑区文本含「Claude Code」", JSON.stringify(b.text));
        note(`大脑元素: <${b.tag.toLowerCase()}> 文本=${JSON.stringify(b.text)}；可见工作台内 <select> 总数=${brain.visibleSelectsInWorkspace}`);
      }
      await shot(page, "workspace-chat");

      // 对话 → 终端
      await page.locator('button[title="U-CLI（终端）"]:visible').first().click();
      await page.waitForTimeout(1500);
      pane = await page.evaluate(SCAN_PANE);
      check(pane.cli.active === true && pane.chat.active === false, "点「终端」后切到终端态（终端=激活，对话=未激活）", JSON.stringify(pane));
      check(pane.xtermVisible >= 1, "终端态下有可见的 .xterm 终端视图", `可见 .xterm ${pane.xtermVisible} 个`);
      await assertPageSane(page, "工作台(终端态)");
      await shot(page, "workspace-cli");

      // 终端 → 对话
      await page.locator('button[title="U-Chat（对话）"]:visible').first().click();
      await page.waitForTimeout(800);
      pane = await page.evaluate(SCAN_PANE);
      check(pane.chat.active === true && pane.cli.active === false, "点「对话」后切回对话态", JSON.stringify(pane));
      const brain2 = await page.evaluate(SCAN_BRAIN);
      check(brain2.labels.length === 1 && brain2.labels[0].tag !== "SELECT" && !brain2.labels[0].regionHasSelect && brain2.labels[0].text.includes("Claude Code"), "切回对话态后大脑区仍是静态「Claude Code」标签、无 <select>", JSON.stringify(brain2.labels.map((l) => ({ tag: l.tag, text: l.text, regionHasSelect: l.regionHasSelect }))));
      await assertPageSane(page, "工作台(切回对话态)");
      await shot(page, "workspace-chat-again");
    });

    /* ---- 收尾：整局没有被重载、没有 ui_* 上报 ---- */
    await scenario("teardown-checks", async () => {
      check(navCount === 1, "整个视口只做过 1 次主框架导航（中途没被 vite 重载）", `framenavigated 主框架 ${navCount} 次`);
      const c = await readShimCounters(page);
      const ui = c.reports.filter((r) => String(r.kind || "").startsWith("ui_"));
      check(ui.length === 0, "没有 report_bug(ui_*)（PanelBoundary/根边界/全局 error 的上报）", JSON.stringify(c.reports));
      for (const [k, n] of Object.entries(c.uncovered)) {
        const cur = uncoveredAll.get(k) ?? { count: 0, vps: new Set() };
        cur.count += n;
        cur.vps.add(curVp);
        uncoveredAll.set(k, cur);
      }
      for (const [k, n] of Object.entries(c.calls)) callsAll.set(k, (callsAll.get(k) ?? 0) + n);
    });

    // 错误按场景汇总成 problems（原文另行完整打印）
    const mine = errorLog.filter((e) => e.vp === curVp);
    const byScenario = new Map();
    for (const e of mine) byScenario.set(e.scenario, (byScenario.get(e.scenario) ?? 0) + 1);
    curScenario = "errors";
    check(mine.length === 0, `本视口 pageerror + console.error 总数为 0`, `实际 ${mine.length} 条；按场景：${JSON.stringify(Object.fromEntries(byScenario))}`);
    if (netNotes.length) {
      for (const n of netNotes.slice(0, 20)) note(`网络: ${n}`);
      if (netNotes.length > 20) note(`网络: 还有 ${netNotes.length - 20} 条省略`);
    }
  } finally {
    await ctx.close();
  }
}

/* ------------------------------------------------------------------------------------------------
 * 五、主流程与报告
 * ---------------------------------------------------------------------------------------------- */

log("1.3.7 界面收敛探针（独立 vite dev 实例 + 假 Tauri shim，不碰在跑的 U-King）");
if (MUTATE) log(`*** 变异自检模式 UKING_PROBE_MUTATE=${MUTATE}：被测代码在浏览器侧被故意破坏，下面应当出现 FAIL、退出码应当是 1 ***`);
log(`目标: ${URL}`);
log(`截图目录: ${OUT}`);
log(`Sidebar.tsx 解析: 核心=${JSON.stringify(SRC.CORE.map((x) => `${x.id}:${x.label}`))} 更多=${JSON.stringify(SRC.MORE.map((x) => `${x.id}:${x.label}`))} 实验室=${JSON.stringify(SRC.LAB.map((x) => `${x.id}:${x.label}`))}`);

const browser = await chromium.launch();
let fatal = null;
try {
  await warmUp(browser);
  for (const vp of VIEWPORTS) await runViewport(browser, vp);
} catch (e) {
  fatal = e;
  problems.push(`脚本自身崩溃 :: ${String(e?.stack || e)}`);
} finally {
  await browser.close();
}

log("\n===== 每条 pageerror / console.error 原文 =====");
if (errorLog.length === 0) log("（无）");
errorLog.forEach((e, n) => log(`#${n + 1} [${e.vp}] [${e.scenario}] ${e.kind}\n${e.text}\n`));

log("===== shim 未覆盖的命令（返回了空值，页面拿到的是假数据）=====");
if (uncoveredAll.size === 0) log("（无）");
for (const [k, v] of [...uncoveredAll.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
  log(`  ${k}  x${v.count}  视口: ${[...v.vps].join(", ")}  -> ${/^list_|_list$/.test(k) ? "[]" : "null"}`);
}
log("===== shim 给了假数据的命令（页面拿到的是编的值）=====");
log("  get_env list_tools get_driver_status get_device_key list_providers check_update get_setup_state term_snapshot_pending");
log("  take_update_flag instance_role list_tasks upsert_task detect_stack get_usage_trend query_usage_meter");
log("  airuntime_doctor fetch_optimize_advice report_bug action_parity_call(runtime.tool.inspect)  plugin:event|*（订阅 id）");

log("\n===== 观察记录（非断言）=====");
notes.forEach((n) => log(`  ${n}`));

log("\n===== 截图 =====");
shots.forEach((s) => log(`  ${s}`));

writeFileSync(
  path.join(OUT, "report.json"),
  JSON.stringify(
    {
      url: URL,
      viewports: VIEWPORTS,
      sidebarSource: SRC,
      brief: BRIEF,
      problems,
      notes,
      errors: errorLog,
      uncovered: Object.fromEntries([...uncoveredAll.entries()].map(([k, v]) => [k, { count: v.count, viewports: [...v.vps] }])),
      calls: Object.fromEntries(callsAll),
      steps: stepRows,
      shots,
    },
    null,
    2,
  ),
);
log(`\nreport: ${path.join(OUT, "report.json")}`);

if (problems.length > 0) {
  console.error(`\nproblems (${problems.length}):`);
  problems.forEach((p, n) => console.error(`  ${n + 1}. ${p}`));
  process.exit(1);
}
log("\nproblems: 0");
if (fatal) process.exit(1);
