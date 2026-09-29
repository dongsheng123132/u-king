/**
 * 给「我的 AI」页（src/toolhub/ToolHub.tsx，tab id "toolhub"）出图——
 * 2026-09-29 三次改版：Launchpad 式 logo 墙 + 行内展开详情条（不再是每张卡一个「启动」
 * 大按钮的网格）。这份脚本跟着改版重写了场景和判据，套路仍照抄 shot-manager-split.mjs：
 *
 * 🔴 跟 shot-manager-split.mjs 同一个隐私铁律：只用 Playwright 无头 Chromium 的
 *    `page.screenshot` 截**页面**，绝不截桌面/屏幕（不许 CopyFromScreen / PowerShell 截屏）。
 *
 * 🔴 也别截用户正在跑的那个 U-King —— 连的是独立 dev server（vite :1430），
 *    Tauri 调用全部走本文件下面的 shim 喂假数据，只证明布局和交互，不证明真实后端行为。
 *
 * 跟 shot-manager-split.mjs 不同的是命令名对不对得上——读过 src/App.tsx 的
 * `refresh()`（约第 334-364 行）确认首屏真正调用的是 `get_driver_status`，
 * 不是 `driver_status`。
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
 * `installed` 不在这张表里——那是每个场景自己决定的演示状态，不是抄来的。
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

/**
 * 每个工具真正的启动方式——照抄 `src-tauri/src/tools.rs::TOOL_SPECS` 各条目的 `launch_mode`
 * 字面量（Windows 分支：dsh 是 `GuiApp`，跟上面 `TOOL_DEFS` 里 dsh 用 `launch_app="dsh-desktop"`
 * 编的是同一个平台假设，两处必须对得上）。2026-09-29 复审 medium 修复：ToolHub 的详情条不再
 * 只用 `launch_app` 是否非空二分"桌面应用/走 launchPref 分体按钮"，改成读 `runtime.tool.inspect`
 * 回来的真实 `mode`（`RouteTab`/`ExternalTerm` 各自是单按钮，不听 `launchPref`）——这份表就是
 * 喂给下面 shim 里 `action_parity_call` 分支的假数据源，不建这张表的话 hermes/harness-doctor
 * 这两个场景会因为拿不到 `mode` 而退回"未知"分支（单按钮但看不出问题），没法验证到分体 ▾
 * 只在 `embedded_pty` 工具（claude-code/codex/pi/opencode/crush）上出现这条真实行为。
 */
const LAUNCH_MODE_BY_ID = {
  "claude-code": "embedded_pty",
  codex: "embedded_pty",
  openclaw: "route_tab",
  "qwen-code": "embedded_pty",
  pi: "embedded_pty",
  opencode: "embedded_pty",
  crush: "embedded_pty",
  clawx: "gui_app",
  hermes: "route_tab",
  dsh: "gui_app",
  "harness-doctor": "external_term",
  obsidian: "none",
  "uu-remote": "none",
  doubao: "none",
  qwenwork: "none",
  workbuddy: "none",
  "codex-app": "gui_app",
  open365: "gui_app",
  "hermes-app": "gui_app",
  "uu-switch": "gui_app",
};

/** 拼 `runtime.tool.inspect` action 的假返回——ToolHub 只读 `tool_id`/`mode` 这两个字段，
 *  没有必要连 `LaunchPlan` 剩下那些字段（`installed`/`blockers`/`route`…）也照真实形状填全。 */
function buildLaunchPlans() {
  return TOOL_DEFS.map((d) => ({ tool_id: d.id, mode: LAUNCH_MODE_BY_ID[d.id] ?? "none" }));
}

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
    // ("pi")→"pi" —— 版本号只是零成本展示项，不影响本次要验证的默认展开/详情条定位判据。
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
 * 注入到页面里的 shim——替换 `window.__TAURI_INTERNALS__.invoke`，喂假数据，并按场景需要
 * 预置 `localStorage.uking.toolhub.lastTool`（ToolHub 默认展开「上次启动的工具」就读这个
 * key，见 `src/toolhub/ToolHub.tsx` 的默认展开 effect）。命令名照读 src/App.tsx `refresh()`
 * （约 334-364 行）与 src/toolhub/ToolHub.tsx 的 `useEffect`（约 178-199 行）核对过，是真正
 * 被调用的那一组：get_env / list_tools / get_driver_status / get_device_key /
 * get_setup_state / term_snapshot_pending / check_update / take_update_flag /
 * instance_role / list_providers / action_parity_call。没在这张表里的命令一律落 default
 * 分支——返回 null（.catch/可选链已经把这些路径包住了，见 App.tsx 对应 useEffect 的
 * `.catch(() => ...)`），不是漏做，是那些命令这次用例走不到。
 * 🔴 2026-09-29 复审 medium 修复新增 `action_parity_call`：ToolHub 详情条改成读
 * `runtime.tool.inspect`（走 `generated/action-client.ts::createTauriActionClient`，底层命令
 * 就是这个 `action_parity_call`，`{request:{action_id,...}}` 这个参数形状照抄
 * `createTauriActionClient` 源码）判定每个工具真正的启动方式。不喂这条的话，`ToolHub` 里
 * `callAction(...).then(env=>{ if(!env.ok) ... })` 会在 `env` 落回 `default` 分支的 `null` 时
 * 读 `.ok` 抛错——虽然外层 `.catch` 接住了不会崩页面，但 `launchModes` 会一直是空表，
 * 分体 ▾ 只在明确判定为 `embedded_pty` 时才出现这条改动就验证不到（见 `shotSplit`）。
 */
const SHIM = ({ tools, driver, deviceKey, checkUpdate, providers, lastTool, launchPlans }) => {
  if (lastTool) {
    try {
      window.localStorage.setItem("uking.toolhub.lastTool", lastTool);
    } catch {
      /* 无痕模式等拿不到 localStorage 时不影响 shim 其余部分 */
    }
  }
  const fake = (cmd, args) => {
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
      case "action_parity_call": {
        const actionId = args?.request?.action_id;
        if (actionId === "runtime.tool.inspect") {
          return {
            ok: true,
            version: 1,
            action_id: actionId,
            execution_id: "shim",
            result: { tools: launchPlans },
          };
        }
        // 这批场景这次只用到 runtime.tool.inspect 这一条 action，其余 action_id 不在用例里，
        // 落 default 分支即可（同文件顶部注释）。
        return null;
      }
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

async function loadToolhub(page, { installedIds, withModels, lastTool }) {
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
    lastTool: lastTool ?? null,
    launchPlans: buildLaunchPlans(),
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

  // ToolHub 挂载时另起一个 `runtime.tool.inspect` 只读请求算每个工具的真实启动方式
  // （见 ToolHub.tsx `launchModes` 那段 effect）——shim 里虽然是同步 `Promise.resolve`，
  // 但仍隔了一次微任务 + 一次 React 重渲染，给它一拍先稳定下来，不然下面几个场景一进来就
  // 摸按钮，可能摸到「mode 还没落地」的过渡态（分体 ▾ 该出现的时候还没出现）。
  await page.waitForTimeout(50);

  return consoleErrors;
}

/** 页面级粗量——瓷砖数、空态、横向溢出，四张图共用。「瓷砖数」直接数
 *  `data-testid="toolhub-tile"`：新版一个瓷砖只对应一个 DOM 元素（不像旧版大卡片那样
 *  外层 div + 换模型按钮 + 启动按钮三处共享同一个 `data-tool-id`，需要去重才能数对）。
 *  🔴 复审修复：横向溢出原来量的是 `document.documentElement`，但 toolhub 页真正的滚动容器
 *  是 App.tsx「非 TUI 页面」分支那个 `<main className="... overflow-y-auto ...">`（见
 *  App.tsx 约第 1200 行）——CSS 规则：一个元素的 `overflow-x`/`overflow-y` 只要有一个不是
 *  `visible`，另一个也会被隐式算成 `auto`，所以横向溢出实际被裁在这层 `<main>` 上，
 *  `documentElement` 几乎不可能出现 `scrollWidth > clientWidth`（外层没有另开滚动容器），
 *  量它等于永远测不出真实的横向溢出回归（确认属实：这条判据一直是摆设，量了个不会变化
 *  的对象，且从没有被拿去 `problems.push`，见 `finish()` 里新增的判定）。 */
async function measurePage(page) {
  return page.evaluate(() => {
    const sections = Array.from(document.querySelectorAll("section"));
    const findSection = (label) => sections.find((s) => s.querySelector("h2")?.textContent?.includes(label));
    const installedSection = findSection("已安装");
    const installableSection = findSection("可安装");
    const tileCount = (root) => (root ? root.querySelectorAll('[data-testid="toolhub-tile"]').length : 0);
    // App.tsx 里 chat/termwb 那个 `<main>` 是常驻挂载、只用 `style.display` 切走的（保活），
    // `querySelectorAll("main")` 在这个页面可能不止一个，挑当前真正可见（`display !== "none"`）
    // 的那个才是 toolhub 实际在用的滚动容器；找不到就如实返回 null，不能假装量到了 0。
    const mains = Array.from(document.querySelectorAll("main"));
    const scroller = mains.find((el) => getComputedStyle(el).display !== "none") ?? null;
    return {
      installedTiles: tileCount(installedSection),
      installableTiles: tileCount(installableSection),
      emptyStateShown: document.body.textContent?.includes("还没装 AI 工具") ?? false,
      scrollContainerFound: !!scroller,
      overflowX: scroller ? scroller.scrollWidth > scroller.clientWidth : null,
      scrollWidth: scroller ? scroller.scrollWidth : null,
      clientWidth: scroller ? scroller.clientWidth : null,
    };
  });
}

/**
 * 详情条定位判据——给定所选瓷砖的 `data-tool-id`，量：
 *  · 瓷砖/详情条都找得到（找不到就报 error，不是静默通过）；
 *  · 详情条紧跟在瓷砖所在行下面：`detailTop > tileBottom`，且瓷砖所在行的行底
 *    到详情条顶部之间，没有夹着另一整行瓷砖（`noOtherRowBetween`）——这是在验证
 *    `TileGrid` 真的按「所选瓷砖所在行」插入，不是插到了整个网格最后面；
 *  · 三角（`toolhub-detail-caret`）的水平中心与瓷砖水平中心的偏差像素——理论上应为 0，
 *    量出来的是真实渲染误差（字体度量/滚动条等），不是抠标准直接判等。
 */
async function measureDetail(page, sectionLabel, toolId) {
  return page.evaluate(
    ({ sectionLabel, toolId }) => {
      const sections = Array.from(document.querySelectorAll("section"));
      const section = sections.find((s) => s.querySelector("h2")?.textContent?.includes(sectionLabel));
      if (!section) return { error: `找不到 <section> 标题含「${sectionLabel}」` };
      const tile = section.querySelector(`[data-testid="toolhub-tile"][data-tool-id="${toolId}"]`);
      if (!tile) return { error: `找不到 data-testid="toolhub-tile" data-tool-id="${toolId}"` };
      const detail = section.querySelector(`[data-testid="toolhub-detail"][data-tool-id="${toolId}"]`);
      if (!detail) return { error: `找不到 data-testid="toolhub-detail" data-tool-id="${toolId}"` };
      const tileRect = tile.getBoundingClientRect();
      const detailRect = detail.getBoundingClientRect();
      const EPS = 2;
      const allTiles = Array.from(section.querySelectorAll('[data-testid="toolhub-tile"]')).map((el) => el.getBoundingClientRect());
      const sameRow = allTiles.filter((r) => Math.abs(r.top - tileRect.top) <= EPS);
      const rowBottom = Math.max(...sameRow.map((r) => r.bottom));
      const otherRowBetween = allTiles.some(
        (r) => Math.abs(r.top - tileRect.top) > EPS && r.top >= rowBottom - EPS && r.top < detailRect.top - EPS,
      );
      const caret = document.querySelector('[data-testid="toolhub-detail-caret"]');
      const caretRect = caret ? caret.getBoundingClientRect() : null;
      const tileCenterX = tileRect.left + tileRect.width / 2;
      const caretCenterX = caretRect ? caretRect.left + caretRect.width / 2 : null;
      return {
        tileTop: tileRect.top,
        tileBottom: tileRect.bottom,
        detailTop: detailRect.top,
        detailBelowTile: detailRect.top > tileRect.bottom - EPS,
        noOtherRowBetween: !otherRowBetween,
        caretFound: !!caretRect,
        caretDeviationPx: caretRect ? Math.abs(caretCenterX - tileCenterX) : null,
      };
    },
    { sectionLabel, toolId },
  );
}

/** 找一个渲染在「非第一行」的已装瓷砖——列数是运行时按容器宽度算出来的，不硬编码下标。
 *  拿不到（比如这个视口只够摆一行）就返回 null，调用方要如实报告，不能假装测过了。 */
async function pickSecondRowInstalledToolId(page) {
  return page.evaluate(() => {
    const sections = Array.from(document.querySelectorAll("section"));
    const section = sections.find((s) => s.querySelector("h2")?.textContent?.includes("已安装"));
    if (!section) return null;
    const tiles = Array.from(section.querySelectorAll('[data-testid="toolhub-tile"]'));
    if (tiles.length === 0) return null;
    const firstTop = tiles[0].getBoundingClientRect().top;
    const secondRow = tiles.find((el) => Math.abs(el.getBoundingClientRect().top - firstTop) > 2);
    return secondRow ? secondRow.getAttribute("data-tool-id") : null;
  });
}

async function finish(name, page, consoleErrors, extra) {
  const m = await measurePage(page);
  // 找不到真正的滚动容器 = 这条判据没法量，跟其余「找不到就记 problems」的判据（`measureDetail`/
  // `pickSecondRowInstalledToolId`）同一个原则，不能因为找不到就悄悄放过、0 退出（复审修复）。
  if (!m.scrollContainerFound) problems.push(`${name}: 找不到真正的滚动容器（当前可见的 <main>），横向溢出判据没法量`);
  else if (m.overflowX) problems.push(`${name}: 出现横向溢出（scrollWidth=${m.scrollWidth} > clientWidth=${m.clientWidth}）`);
  report[name] = { ...m, ...extra, consoleErrors };
  const file = `${OUT}/${name}.png`;
  await page.screenshot({ path: file, fullPage: true });
  console.log(
    `  📸 ${file}  installedTiles=${m.installedTiles} installableTiles=${m.installableTiles} overflowX=${m.overflowX} consoleErrors=${consoleErrors.length}`,
  );
  await page.close();
}

/** ① default.png —— mixed 数据，默认展开「上次启动的工具」= Claude Code（shim 里预置
 *  localStorage，不用点一下才展开）。 */
async function shotDefault() {
  const page = await browser.newPage({ viewport: { width: 1134, height: 800 }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, {
    installedIds: ["claude-code", "codex", "hermes", "pi"],
    withModels: true,
    lastTool: "claude-code",
  });
  const d = await measureDetail(page, "已安装", "claude-code");
  if (d.error) problems.push(`default: ${d.error}`);
  else {
    if (!d.detailBelowTile) problems.push(`default: 详情条没有出现在所选瓷砖下方（detailTop=${d.detailTop} tileBottom=${d.tileBottom}）`);
    if (!d.noOtherRowBetween) problems.push("default: 瓷砖行和详情条之间夹了另一整行瓷砖");
    if (!d.caretFound) problems.push("default: 没找到三角指示器（toolhub-detail-caret）");
    else if (d.caretDeviationPx > 3) problems.push(`default: 三角水平中心偏差 ${d.caretDeviationPx.toFixed(1)}px（应贴着瓷砖中心）`);
  }
  await finish("default", page, consoleErrors, { detail: d });
}

/** ② gui.png —— 展开一个有 `launch_app` 的已装 GUI 工具（ClawX），验证主按钮文案是
 *  「打开」、没有分体 ▾。 */
async function shotGui() {
  const page = await browser.newPage({ viewport: { width: 1134, height: 800 }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, {
    installedIds: ["claude-code", "codex", "hermes", "pi", "clawx"],
    withModels: true,
    lastTool: "clawx",
  });
  const d = await measureDetail(page, "已安装", "clawx");
  if (d.error) problems.push(`gui: ${d.error}`);
  else if (!d.detailBelowTile) problems.push(`gui: 详情条没有出现在所选瓷砖下方`);
  const splitTrigger = await page.locator('[data-testid="toolhub-launch-split-trigger"][data-tool-id="clawx"]').count();
  if (splitTrigger > 0) problems.push("gui: GUI 工具（有 launch_app）不该出现分体 ▾ 按钮");
  const mainBtn = await page.locator('[data-testid="toolhub-launch-main"][data-tool-id="clawx"]').count();
  if (mainBtn === 0) problems.push("gui: 找不到主启动按钮 toolhub-launch-main");
  await finish("gui", page, consoleErrors, { detail: d });
}

/** ③ install.png —— 展开一个未安装工具（opencode 不在 installedIds 里），验证主按钮
 *  是「安装」。 */
async function shotInstall() {
  const page = await browser.newPage({ viewport: { width: 1134, height: 800 }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, {
    installedIds: ["claude-code", "codex", "hermes", "pi"],
    withModels: true,
  });
  const tile = page.locator('[data-testid="toolhub-tile"][data-tool-id="opencode"]');
  if ((await tile.count()) === 0) {
    problems.push("install: 找不到未安装工具瓷砖 opencode");
  } else {
    await tile.first().click();
    await page.waitForTimeout(200);
  }
  const d = await measureDetail(page, "可安装", "opencode");
  if (d.error) problems.push(`install: ${d.error}`);
  else if (!d.detailBelowTile) problems.push("install: 详情条没有出现在所选瓷砖下方");
  const installBtn = await page.locator('[data-testid="toolhub-install-btn"][data-tool-id="opencode"]').count();
  if (installBtn === 0) problems.push("install: 找不到「安装」按钮 toolhub-install-btn");
  await finish("install", page, consoleErrors, { detail: d });
}

/** ④ split.png —— 命令行工具（Claude Code）详情条展开后，点启动按钮旁的 ▾，
 *  验证弹出「在系统终端打开」「在 U-CLI 打开」两项。 */
async function shotSplit() {
  const page = await browser.newPage({ viewport: { width: 1134, height: 800 }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, {
    installedIds: ["claude-code", "codex", "hermes", "pi"],
    withModels: true,
    lastTool: "claude-code",
  });
  const trigger = page.locator('[data-testid="toolhub-launch-split-trigger"][data-tool-id="claude-code"]');
  if ((await trigger.count()) === 0) {
    problems.push("split: 找不到分体按钮的 ▾ 触发器 toolhub-launch-split-trigger");
  } else {
    await trigger.first().click();
    await page.waitForTimeout(300);
    const systemItem = await page.getByTestId("toolhub-launch-pref-system").isVisible().catch(() => false);
    const ucliItem = await page.getByTestId("toolhub-launch-pref-ucli").isVisible().catch(() => false);
    if (!systemItem || !ucliItem) problems.push("split: 点了 ▾ 但没看到「在系统终端打开」/「在 U-CLI 打开」两项");
  }
  await finish("split", page, consoleErrors, {});
}

/** ⑤ empty.png —— 零安装：验证空态文案，且不应有任何详情条自动展开。 */
async function shotEmpty() {
  const page = await browser.newPage({ viewport: { width: 1134, height: 800 }, deviceScaleFactor: 2 });
  const consoleErrors = await loadToolhub(page, { installedIds: [], withModels: false });
  const m = await measurePage(page);
  if (!m.emptyStateShown) problems.push("empty: 没看到「还没装 AI 工具」空态文案");
  const anyDetail = await page.locator('[data-testid="toolhub-detail"]').count();
  if (anyDetail > 0) problems.push("empty: 已装为 0 时不应该有详情条展开");
  await finish("empty", page, consoleErrors, {});
}

/** ⑥ wide.png —— 1440×900，装够多工具让「已安装」网格出现第二行，展开第二行里的某个
 *  工具，验证三角指向与行内插入位置在宽屏下依然对得上（列数变多，插入点跟着变）。 */
async function shotWide() {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  const manyInstalled = [
    "claude-code", "codex", "pi", "opencode", "clawx", "hermes", "dsh",
    "harness-doctor", "obsidian", "uu-remote", "doubao", "qwenwork", "workbuddy",
    "codex-app", "uu-switch",
  ];
  const consoleErrors = await loadToolhub(page, { installedIds: manyInstalled, withModels: true });
  const targetId = await pickSecondRowInstalledToolId(page);
  let d = { error: "wide: 拿不到两行瓷砖，1440 宽度下已装网格仍然只有一行，没法验证跨行定位" };
  if (!targetId) {
    problems.push(d.error);
  } else {
    await page.locator(`[data-testid="toolhub-tile"][data-tool-id="${targetId}"]`).first().click();
    // 复审 medium 修复：已装瓷砖单击不再立刻 toggleSelect，要等 `commitTileClick` 那个
    // 跟浏览器 dblclick 判定窗口对齐的计时器（ToolHub.tsx，300ms）到期才真的挪详情条——
    // 这里的等待要盖过那个延迟，不然量到的是详情条还没挪过去的旧布局。
    await page.waitForTimeout(450);
    d = await measureDetail(page, "已安装", targetId);
    if (d.error) problems.push(`wide: ${d.error}`);
    else {
      if (!d.detailBelowTile) problems.push(`wide: 详情条没有出现在第二行瓷砖下方（detailTop=${d.detailTop} tileBottom=${d.tileBottom}）`);
      if (!d.noOtherRowBetween) problems.push("wide: 瓷砖行和详情条之间夹了另一整行瓷砖");
      if (!d.caretFound) problems.push("wide: 没找到三角指示器");
      else if (d.caretDeviationPx > 3) problems.push(`wide: 三角水平中心偏差 ${d.caretDeviationPx.toFixed(1)}px`);
    }
  }
  await finish("wide", page, consoleErrors, { targetId, detail: d });
}

console.log("给「我的 AI」（ToolHub）出图（独立 dev 实例，不碰你在跑的 U-King）：");
await shotDefault();
await shotGui();
await shotInstall();
await shotSplit();
await shotEmpty();
await shotWide();

await browser.close();

writeFileSync(`${OUT}/report.json`, JSON.stringify({ scenarios: report, problems }, null, 2));
console.log(`\n📄 ${OUT}/report.json`);

if (problems.length > 0) {
  console.error("\n❌ problems:");
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log("完成。");
