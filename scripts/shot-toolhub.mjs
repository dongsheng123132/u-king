/**
 * 给首页新改版的「我的 AI」页（src/toolhub/ToolHub.tsx，tab id "toolhub"）出图。
 *
 * 🔴 跟 shot-manager-split.mjs 同一个隐私铁律：只用 Playwright 无头 Chromium 的
 *    `page.screenshot` 截**页面**，绝不截桌面/屏幕（不许 CopyFromScreen / PowerShell 截屏）。
 *
 * 🔴 也别截用户正在跑的那个 U-King —— 连的是独立 dev server（vite :1430），
 *    Tauri 调用全部走本文件下面的 shim 喂假数据，只证明布局和交互，不证明真实后端行为。
 *
 * 套路照抄 shot-manager-split.mjs：chromium.launch() → page.addInitScript 注入
 * window.__TAURI_INTERNALS__ shim 顶替 invoke，未知命令回 null/空数组而不是抛错，
 * metadata 补全（否则 getCurrentWindow() 炸出故障边界页）。
 *
 * 跟 shot-manager-split.mjs 不同的是命令名对不对得上——读过 src/App.tsx 的
 * `refresh()`（约第 334-364 行）确认首屏真正调用的是 `get_driver_status`，
 * 不是 `driver_status`（shot-manager-split.mjs 那个 case 分支其实从未命中过，
 * 它能截出图全靠 default 分支兜底返回 null，Manager 页面对 driver=null 更容忍；
 * ToolHub 的「换模型」下拉要读 `driver.active`/`driver.*_model`，必须真给对命令名）。
 *
 * 用法：pnpm dev 起在 1430，然后 node scripts/shot-toolhub.mjs
 *      换端口：UKING_DEV_URL=http://localhost:5173/ node scripts/shot-toolhub.mjs
 */
import { chromium } from "playwright";
import { mkdirSync, writeFileSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = "shots/toolhub";
mkdirSync(OUT, { recursive: true });

/**
 * 工具目录——id/name/kind/launch_cmd/launch_app/hidden 照抄
 * `src-tauri/src/tools.rs::list_tools()` 里的真实字面量（2026-09-29 读源码核对）。
 * `installed` 不在这张表里——那是每个场景（mixed/empty）自己决定的演示状态，不是抄来的。
 * `open365` 后端 `installed: true` 是硬编码常量（按需下载设计，见 tools.rs 同名注释），
 * 这里同样硬编码，不随场景切换——这正是 `LAB_TOOLS` 要把它从主「已装」网格摘出去、
 * 单独放"实验室（已装）"分区的原因（ToolHub.tsx `installed` 那段注释）。
 */
const TOOL_DEFS = [
  { id: "claude-code", name: "Claude Code CLI", kind: "standalone", launch_cmd: "claude", launch_app: "", hidden: false },
  { id: "codex", name: "Codex CLI", kind: "standalone", launch_cmd: "codex", launch_app: "", hidden: false },
  { id: "openclaw", name: "OpenClaw CLI（龙虾）", kind: "deep", launch_cmd: "openclaw", launch_app: "", hidden: true },
  { id: "qwen-code", name: "Qwen Code", kind: "standalone", launch_cmd: "qwen", launch_app: "", hidden: true },
  { id: "pi", name: "pi", kind: "standalone", launch_cmd: "pi", launch_app: "", hidden: false },
  { id: "opencode", name: "OpenCode", kind: "standalone", launch_cmd: "opencode", launch_app: "", hidden: false },
  { id: "crush", name: "Crush", kind: "standalone", launch_cmd: "crush", launch_app: "", hidden: true },
  { id: "clawx", name: "OpenClaw 桌面版（ClawX）", kind: "deep", launch_cmd: "", launch_app: "clawx", hidden: false },
  { id: "hermes", name: "Hermes Agent（Nous 官方）", kind: "deep", launch_cmd: "hermes", launch_app: "", hidden: false },
  // Windows 分支（cfg!(windows)）：官方桌面版，launch_app="dsh-desktop"、launch_cmd=""。
  { id: "dsh", name: "DeepSeek Harness（官方桌面版）", kind: "deep", launch_cmd: "", launch_app: "dsh-desktop", hidden: false },
  { id: "harness-doctor", name: "Harness Doctor（AI 工具体检）", kind: "utility", launch_cmd: "harness-doctor --target all --no-ports", launch_app: "", hidden: false },
  { id: "obsidian", name: "Obsidian 知识库", kind: "standalone", launch_cmd: "", launch_app: "", hidden: false },
  { id: "uu-remote", name: "UU远程（手机控电脑）", kind: "standalone", launch_cmd: "", launch_app: "", hidden: false },
  { id: "doubao", name: "豆包工作台", kind: "standalone", launch_cmd: "", launch_app: "", hidden: false },
  { id: "qwenwork", name: "千问办公", kind: "standalone", launch_cmd: "", launch_app: "", hidden: false },
  { id: "workbuddy", name: "WorkBuddy", kind: "standalone", launch_cmd: "", launch_app: "", hidden: false },
  // Windows/macOS 分支：Codex 桌面版，插在数组下标 2（这里顺序不影响 ToolHub 渲染，
  // ToolHub 不依赖 list_tools 的原始顺序做任何跨工具比较）。
  { id: "codex-app", name: "Codex 桌面版", kind: "standalone", launch_cmd: "", launch_app: "codex-app", hidden: false },
  { id: "open365", name: "Open365 电脑管家（开源）", kind: "standalone", launch_cmd: "", launch_app: "open365", hidden: false, alwaysInstalled: true },
  { id: "hermes-app", name: "Hermes 桌面版（Nous 官方）", kind: "deep", launch_cmd: "", launch_app: "hermes-app", hidden: true },
  { id: "uu-switch", name: "uu-switch 模型切换器", kind: "standalone", launch_cmd: "", launch_app: "uu-switch", hidden: false },
];

/** 供应商列表——照抄 shot-manager-split.mjs 的虾盘云/官方/DeepSeek 三条，字段形状一致。 */
const PROVIDERS = [
  { id: "xiapan", name: "虾盘云", summary: "内置 Key，开箱即用", openai_base: "https://api.u-claw.org.cn/v1", anthropic_base: null, model: "deepseek-v4-pro", small_model: "deepseek-v4-flash", key_url: "", key_hint: "API Key", builtin_recharge: true, recommended: true, builtin: true, api_key: "sk-***" },
  { id: "official", name: "官方直连", summary: "用你自己的 Key", openai_base: "", anthropic_base: null, model: "", small_model: "", key_url: "https://console.anthropic.com", key_hint: "API Key", builtin_recharge: false, recommended: false, builtin: true, api_key: "" },
  { id: "deepseek", name: "DeepSeek 官方", summary: "官方直连，自备 Key", openai_base: "https://api.deepseek.com/v1", anthropic_base: "https://api.deepseek.com/anthropic", model: "deepseek-chat", small_model: "deepseek-chat", key_url: "https://platform.deepseek.com", key_hint: "API Key", builtin_recharge: false, recommended: false, builtin: true, api_key: "" },
];

/** 按场景装/不装的 id 集合，拼出 `list_tools()` 形状的假数据（`ToolInfo`，见 App.tsx 定义）。 */
function buildTools(installedIds) {
  const set = new Set(installedIds);
  return TOOL_DEFS.map((d) => ({
    id: d.id,
    name: d.name,
    summary: "",
    kind: d.kind,
    installed: d.alwaysInstalled ? true : set.has(d.id),
    action: "install",
    target: "",
    launch_cmd: d.launch_cmd,
    launch_app: d.launch_app,
    hidden: d.hidden,
  }));
}

/** 按场景拼 `get_driver_status` 形状的假数据（`DriverStatus`，见 src/lib/types.ts 定义）。 */
function buildDriver({ withModels }) {
  if (!withModels) {
    return {
      claude_base: null,
      claude_model: null,
      codex_provider: null,
      codex_model: null,
      clawx_model: null,
      clawx_installed: false,
      hermes_model: null,
      hermes_installed: false,
      dsh_model: null,
      dsh_installed: false,
      active: {},
      discovered: [],
    };
  }
  return {
    claude_base: "https://api.u-claw.org.cn/v1",
    claude_model: "deepseek-v4-pro",
    codex_provider: "official",
    codex_model: "gpt-5.1-codex-max",
    clawx_model: null,
    clawx_installed: false,
    hermes_model: "deepseek-v4-flash",
    hermes_installed: true,
    dsh_model: null,
    dsh_installed: false,
    active: { claude: "xiapan", codex: "official", hermes: "xiapan" },
    // discoveryNameFor("claude-code")→"claude"、("codex")→"codex"（不在它的两条特判里，原样返回）、
    // ("pi")→"pi" —— 版本号只是零成本展示项，不影响本次要验证的换模型/空态判据。
    discovered: [
      { name: "claude", path: "C:/Users/demo/AppData/Roaming/npm/claude.cmd", source: "machine", version: "1.2.3", configured: true },
      { name: "codex", path: "C:/Users/demo/AppData/Roaming/npm/codex.cmd", source: "machine", version: "0.45.0", configured: true },
      { name: "hermes", path: "C:/Users/demo/.local/bin/hermes.exe", source: "portable", version: "2.1.0", configured: true },
      { name: "pi", path: "C:/Users/demo/AppData/Roaming/npm/pi.cmd", source: "machine", version: "0.9.0", configured: true },
    ],
  };
}

const DEVICE_KEY = {
  key: "sk-xp-demo",
  recharge_url: "https://u-claw.org.cn/recharge",
  balance: { tokens: 128500, cny: 128.5, text: "¥128.50" },
  charged: true,
};

const CHECK_UPDATE = { current: "1.0.0", latest: "1.0.0", has_update: false, checked_ok: true, notes: "", download_url: "" };

/**
 * 注入到页面里的 shim——替换 `window.__TAURI_INTERNALS__.invoke`，喂假数据。
 * 命令名照读 src/App.tsx `refresh()`（约 334-364 行）与 src/toolhub/ToolHub.tsx
 * 的 `useEffect`（约 178-199 行）核对过，是真正被调用的那一组：
 * get_env / list_tools / get_driver_status / get_device_key / get_setup_state /
 * term_snapshot_pending / check_update / take_update_flag / instance_role / list_providers。
 * 没在这张表里的命令一律落 default 分支——返回 null（.catch/可选链已经把这些路径包住了，
 * 见 App.tsx 对应 useEffect 的 `.catch(() => ...)`），不是漏做，是那些命令这次用例走不到。
 */
const SHIM = ({ tools, driver, deviceKey, checkUpdate, providers }) => {
  const fake = (cmd, _args) => {
    switch (cmd) {
      case "list_tools":
        return tools;
      case "get_driver_status":
        return driver;
      case "get_device_key":
        return deviceKey;
      case "list_providers":
        return providers;
      case "check_update":
        return checkUpdate;
      case "get_env":
        return {
          running_from_local: true,
          install_dir: "C:/Users/demo/AppData/Local/u-king",
          context_menu_registered: false,
          opened_dir: null,
          platform: "windows",
          home_dir: "C:/Users/demo",
        };
      case "get_setup_state":
        return { has_tool: true, has_driver: true, charged: true, clawx_needs_xiapan: false, next_step: "", hint: "" };
      case "term_snapshot_pending":
        return null;
      case "take_update_flag":
        return false;
      case "instance_role":
        return { role: "primary" };
      default:
        // event 插件那一族要返回订阅 id，返回 null 会让监听注册失败（同 shot-manager-split.mjs）。
        if (cmd?.startsWith("plugin:event|")) return 1;
        return null;
    }
  };
  // 🔴 `metadata` 少了会在 `getCurrentWindow()` 里炸（同 shot-manager-split.mjs 的教训）。
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => Promise.resolve(fake(cmd, args)),
    convertFileSrc: (p) => "https://asset.localhost/" + encodeURIComponent(p),
    transformCallback: (cb) => {
      const id = Math.floor(Math.random() * 1e9);
      window[`_${id}`] = cb;
      return id;
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
};

const browser = await chromium.launch();
const problems = [];
const report = {};

async function loadToolhub(page, { installedIds, withModels }) {
  const consoleErrors = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });
  page.on("pageerror", (err) => consoleErrors.push(String(err)));

  await page.addInitScript(SHIM, {
    tools: buildTools(installedIds),
    driver: buildDriver({ withModels }),
    deviceKey: DEVICE_KEY,
    checkUpdate: CHECK_UPDATE,
    providers: PROVIDERS,
  });
  await page.goto(URL, { waitUntil: "networkidle" });

  // 🔴 截图前先确认截的是界面，不是故障边界页（同 shot-manager-split.mjs 的教训）。
  const boom = await page.getByText("界面已停止").count().catch(() => 0);
  if (boom) {
    const txt = await page.textContent("body").catch(() => "");
    throw new Error(`页面崩在故障边界上，没截到界面：\n${(txt || "").slice(0, 400)}`);
  }

  // 首屏落地是无条件 setTab("toolhub")（App.tsx refresh() 里的落点逻辑），
  // 用标题「我的 AI」当就绪信号，比固定 waitForTimeout 更不容易抖动。
  const h1 = page.locator("h1", { hasText: "我的 AI" });
  await h1.waitFor({ state: "visible", timeout: 15000 });

  return consoleErrors;
}

/** 量页面状态——同一套判据给四张图共用，写进 report.json。
 *  🔴 每张已装卡片里外层 div + 「换模型」按钮 + 「启动」按钮三处都带同一个
 *  `data-tool-id`（ToolHub.tsx 卡片 JSX），`querySelectorAll("[data-tool-id]").length`
 *  数的是元素数不是卡片数——同一张卡会被数 2~3 遍。按 `data-tool-id` 的值去重后
 *  再计数，数的才是「有几张卡/几个 tile」。 */
async function measure(page) {
  return page.evaluate(() => {
    const sections = Array.from(document.querySelectorAll("section"));
    const findSection = (label) => sections.find((s) => s.querySelector("h2")?.textContent?.includes(label));
    const installedSection = findSection("已安装");
    const installableSection = findSection("可安装");
    const uniqueToolIds = (root) =>
      root ? new Set(Array.from(root.querySelectorAll("[data-tool-id]")).map((el) => el.getAttribute("data-tool-id"))).size : 0;
    return {
      installedCards: uniqueToolIds(installedSection),
      installableTiles: uniqueToolIds(installableSection),
      emptyStateShown: document.body.textContent?.includes("还没装 AI 工具") ?? false,
      overflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    };
  });
}

async function shotMixed(name, { width, height, openMenu }) {
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, {
    installedIds: ["claude-code", "codex", "hermes", "pi"],
    withModels: true,
  });

  if (openMenu) {
    const trigger = page.locator('[data-testid="toolhub-model-trigger"][data-tool-id="claude-code"]');
    const count = await trigger.count();
    if (count === 0) {
      problems.push(`${name}: 找不到 data-testid="toolhub-model-trigger" (data-tool-id="claude-code")`);
      await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: true });
      await page.close();
      return consoleErrors;
    }
    await trigger.first().click();
    await page.waitForTimeout(400);
    // 菜单没弹出来也要如实报告，不能悄悄截一张没有菜单的图充数。
    const menuVisible = await page.getByText("虾盘云", { exact: false }).first().isVisible().catch(() => false);
    if (!menuVisible) {
      problems.push(`${name}: 点了 toolhub-model-trigger，但没看到供应商菜单（"虾盘云"字样）`);
    }
  }

  const m = await measure(page);
  report[name] = m;
  const file = `${OUT}/${name}.png`;
  await page.screenshot({ path: file, fullPage: true });
  console.log(`  📸 ${file}  installedCards=${m.installedCards} installableTiles=${m.installableTiles} overflowX=${m.overflowX} consoleErrors=${consoleErrors.length}`);
  report[name].consoleErrors = consoleErrors;
  await page.close();
  return consoleErrors;
}

async function shotEmpty(name, { width, height }) {
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, { installedIds: [], withModels: false });
  const m = await measure(page);
  report[name] = m;
  report[name].consoleErrors = consoleErrors;
  if (!m.emptyStateShown) {
    problems.push(`${name}: 没看到「还没装 AI 工具」空态文案`);
  }
  const file = `${OUT}/${name}.png`;
  await page.screenshot({ path: file, fullPage: true });
  console.log(`  📸 ${file}  emptyStateShown=${m.emptyStateShown} installedCards=${m.installedCards} overflowX=${m.overflowX} consoleErrors=${consoleErrors.length}`);
  await page.close();
  return consoleErrors;
}

console.log("给「我的 AI」（ToolHub）出图（独立 dev 实例，不碰你在跑的 U-King）：");
await shotMixed("mixed", { width: 1134, height: 800 });
await shotEmpty("empty", { width: 1134, height: 800 });
await shotMixed("menu", { width: 1134, height: 800, openMenu: true });
await shotMixed("wide", { width: 1440, height: 900 });

await browser.close();

writeFileSync(`${OUT}/report.json`, JSON.stringify({ scenarios: report, problems }, null, 2));
console.log(`\n📄 ${OUT}/report.json`);

if (problems.length > 0) {
  console.error("\n❌ problems:");
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log("完成。");
