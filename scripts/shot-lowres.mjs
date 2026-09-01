/**
 * 低分辨率排版取证 —— 按真实客户机的**客户区**尺寸开视口，截图 + 量出关键元素的实际像素。
 *
 * 为什么不直接开 exe：起第二个 U-King 会往 crashlog 写第二份会话标记，
 * 强杀掉就留下一个「上次异常退出」的假信号（CLAUDE.md 里 --crash-test 那条盯的正是它）。
 * 布局是纯前端的事，用 dev server + 真 Chromium 量，一个字节的真实状态都不碰。
 *
 * 视口不是屏幕分辨率：1366×768 的笔记本，扣掉任务栏 40 + 原生标题栏 32 才是网页拿到的高度。
 *
 * 用法：node scripts/shot-lowres.mjs   （需先 pnpm dev）
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = "C:/Users/example/AppData/Local/Temp/uking-lowres";

/** 屏幕 → 网页实际拿到的客户区（扣任务栏 + 原生标题栏；原生标题栏开着，见 tauri.conf.json 没设 decorations）。 */
const CASES = [
  { name: "1366x768", w: 1366, h: 768 - 40 - 32 },   // 最常见的老笔记本 = 目标档位
  { name: "1280x752", w: 1280, h: 752 - 32 },        // 用户截图那台（1920×1200 @150%）
  // ★ 用户 2026-08-09 报「这个分辨率下好丑」那台：1920×1008 物理 @150% 缩放 = 1280×640 CSS，
  //   narrow 和 short **同时**命中，是全场最紧的一档。看板的断言就按它量。
  { name: "1280x640", w: 1280, h: 640, board: "tight" },
  { name: "1920x1080", w: 1920, h: 1080 - 40 - 32, board: "roomy" }, // 对照组：正常机器排版必须不变
  // ★ 基线对照：同样 1366 宽，但高度跨到 short 阈值(779)以上 → 走的就是**改动前**那套排版。
  //   拿它的 navContentH 和上面 1366×696 的比，就是这次改动省下的真实像素，不靠估。
  { name: "1366x800-baseline", w: 1366, h: 800 },
];

/**
 * 任务看板的纵横预算判据。
 *
 * 为什么要写死数字：这块板改之前在 1280×640 上把 **184px（可用高的 29%）** 花在顶部三条横幅上，
 * 卡片区只剩 404px、列宽 156px —— 而 `cargo check` / `pnpm build` / `action conformance` 对排版
 * **全绿**。排版没有跑道就只能等客户截图。
 *
 * `roomy` 这一档是**反向**断言：宽屏必须**仍然**厚（副标题、定时任务框、大行距都在）。
 * 少了它，哪天有人把矮屏那套收紧当成全局默认，这条跑道会照样报绿。
 */
const BOARD_BUDGET = {
  tight: { chromeMax: 115, colMin: 158 },
  roomy: { chromeMin: 175, colMin: 250 },
};

/**
 * 最小 Tauri shim —— 只为让界面**渲染出来**，不模拟业务。
 *
 * ⚠️ 边界说清楚：这层 fixture 决定的是「屏幕上有多少内容」，**不是布局规则本身**。
 * 布局规则（矮屏收哪些间距）在真机上跟这里一模一样。凡是依赖真实后端返回才成立的
 * 结论（比如"客户机上这一栏到底几项"）不能拿这个跑道下判断 —— 那得在真 exe 上看。
 */
const TAURI_SHIM = () => {
  const now = Date.now();
  /** 看板要有真卡片才量得出卡片排版；标题**故意取长的**，短标题量不出截断。 */
  const AI_TASKS = {
    days: 7, active_window_secs: 300, truncated: false, notes: [], counts: { running: 1 },
    sources: [{ tool: "claude", label: "Claude Code", path: "~/.claude", present: true, readable: true, files_in_window: 2, files_scanned: 9, count: 2, note: "" }],
    tasks: [
      { id: "a1", tool: "claude", tool_label: "Claude Code", title: "帮我写一篇公众号推文关于 AI 的", dir: "C:\\demo", project: "demo", model: "", status: "done", status_from: "mtime", started_at: now, updated_at: now, note: "" },
      { id: "a2", tool: "claude", tool_label: "Claude Code", title: "你好", dir: "C:\\demo", project: "demo", model: "", status: "running", status_from: "mtime", started_at: now, updated_at: now, note: "" },
    ],
  };
  // 已经「问过」了 —— 否则第一次进板弹的是发现卡片，顶部横幅高度量的就不是常态。
  try { localStorage.setItem("uking.board.ai_sources.v1", JSON.stringify(["claude"])); } catch { /* ignore */ }
  const fake = (cmd, args) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\user1", installed: true, opened_dir: null };
    if (cmd === "list_tools") return [];
    if (cmd === "list_tasks") return [];
    if (cmd === "list_ai_tasks") return AI_TASKS;
    // 后端会把落盘后的 Task 原样回传，store 直接存它 —— 返回 null 会让 SessionList 当场崩
    if (cmd === "upsert_task") return args?.task ?? null;
    if (cmd === "list_automations") return { jobs: [] };
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false };
    if (cmd?.startsWith("plugin:event|")) return 1;
    return null;
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => Promise.resolve(fake(cmd, args)),
    transformCallback: (cb) => {
      const id = Math.floor(Math.random() * 1e9);
      window[`_${id}`] = cb;
      return id;
    },
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main", windowLabel: "main" },
    },
    plugins: {},
  };
  // plugin-event 的 unlisten 走这条独立通道（不在 __TAURI_INTERNALS__ 里）
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
};

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
/** 攒下来一次报完 —— 一个档位红了不该挡住后面几个档位的取证。 */
const problems = [];

for (const c of CASES) {
  const page = await browser.newPage({ viewport: { width: c.w, height: c.h } });
  await page.addInitScript(TAURI_SHIM);
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e).slice(0, 120)));
  await page.goto(URL, { waitUntil: "networkidle" }).catch(() => {});
  // 🔴 等侧栏真出现，别只等一个固定秒数：dev server 冷启动（首次预构建依赖）比 2.5 秒慢，
  //    第一个档位于是量出一整排 null —— 而 null 不触发任何断言，这条跑道就报了绿。
  //    「没量到」必须跟「量到了但不对」一样红。
  await page.waitForSelector("aside", { timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(2500); // 等懒加载的分块 + 首屏落点算完

  // 量真实像素，别靠肉眼看截图猜
  const measure = () => page.evaluate(() => {
    const q = (s) => document.querySelector(s);
    const box = (el) => (el ? { w: Math.round(el.getBoundingClientRect().width), h: Math.round(el.getBoundingClientRect().height) } : null);
    const aside = document.querySelector("aside");
    const nav = aside?.querySelector("nav");
    return {
      sidebarW: box(aside)?.w ?? null,
      // 导航**内容**高度：跟容器多高无关，可以跨视口直接比 → 这就是前后对比的量尺
      navContentH: nav?.scrollHeight ?? null,
      navViewH: nav?.clientHeight ?? null,
      // 侧栏自己滚不滚 = 装不装得下（客户第二张截图里那条滚动条）
      sidebarNavOverflow: nav ? Math.max(0, nav.scrollHeight - nav.clientHeight) : null,
      // 整页有没有溢出（双滚动条的来源）
      docOverflow: Math.max(0, document.documentElement.scrollHeight - window.innerHeight),
      titleBarPresent: !!q("header[data-tauri-drag-region]"),
      // 「缩到托盘」不管标题栏在不在，界面上必须始终找得到（不许因为省空间把功能弄没）
      trayEntry: !!document.querySelector('[title*="托盘"]'),
      viewport: { w: window.innerWidth, h: window.innerHeight },
    };
  });

  const click = async (name) => {
    await page.getByRole("button", { name, exact: false }).first().click({ timeout: 3000 }).catch(() => {});
    await page.waitForTimeout(250);
  };

  // 三种状态分开量 —— 「两个组都展开」是极端压力场景，不是客户的日常。
  // 客户第二张截图里就是 `moreOpen`：只展开了「更多」。拿极端场景下结论 = 把常态说坏。
  const closed = await measure();
  await click("更多");
  const moreOpen = await measure();
  await page.screenshot({ path: `${OUT}/${c.name}.png`, fullPage: false });
  await click("实验室");
  const bothOpen = await measure();
  await page.screenshot({ path: `${OUT}/${c.name}-both.png`, fullPage: false });

  // U-Workspace（客户原始截图那一页）：量对话区真正剩多少高度 —— 「一边溢出一边大片空白」在这。
  await page.getByRole("button", { name: "U-Workspace", exact: false }).first().click({ timeout: 3000 }).catch(() => {});
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${OUT}/${c.name}-workspace.png`, fullPage: false });
  const ws = await page.evaluate(() => {
    // 消息流那个滚动容器 = 客户真正用来看对话的地方，它剩多少就是这次改动的成果
    const scroller = [...document.querySelectorAll("div")].find((d) => d.className?.includes?.("overflow-y-auto") && d.className?.includes?.("select-text"));
    return { chatViewH: scroller ? Math.round(scroller.getBoundingClientRect().height) : null };
  });

  // ★ 任务看板（用户 2026-08-09 指着说「好丑」的那一屏）。只在标了 board 的档位上量，
  //   免得每个档位都多跑一次导航。
  let board = null;
  if (c.board) {
    await page.getByRole("button", { name: "看板", exact: false }).first().click({ timeout: 5000 }).catch(() => {});
    await page.waitForSelector(".grid.grid-cols-5", { timeout: 15000 }).catch(() => {});
    await page.waitForTimeout(1200);
    await page.screenshot({ path: `${OUT}/${c.name}-board.png`, fullPage: false });
    board = await page.evaluate(() => {
      const grid = document.querySelector(".grid.grid-cols-5");
      if (!grid) return null;
      const root = grid.closest(".h-full.flex.flex-col");
      const scroller = grid.parentElement;
      const foot = root?.querySelector("footer");
      const rootTop = root?.getBoundingClientRect().top ?? 0;
      return {
        // 顶部三条横幅一共吃掉多少 —— 这就是「看不到卡片」的直接原因
        chromeH: Math.round((scroller?.getBoundingClientRect().top ?? 0) - rootTop),
        cardsH: Math.round(scroller?.getBoundingClientRect().height ?? 0),
        footerH: Math.round(foot?.getBoundingClientRect().height ?? 0),
        colW: Math.round(grid.children[0]?.getBoundingClientRect().width ?? 0),
        cols: grid.children.length,
        // 五列必须全露出来（「出错」那列被挤出屏幕的老坑）
        overflowX: Math.max(0, grid.scrollWidth - (scroller?.clientWidth ?? 0)),
        // 口径说明一个字都不许因为省地方被删掉
        caveat: (root?.innerText || "").includes("永远不进「出错」列"),
      };
    });
  }

  console.log(JSON.stringify({
    case: c.name,
    ...ws,
    viewport: closed.viewport,
    titleBarPresent: closed.titleBarPresent,
    trayEntry: closed.trayEntry,
    docOverflow: closed.docOverflow,
    navScroll: {
      closed: closed.sidebarNavOverflow,
      moreOpen: moreOpen.sidebarNavOverflow,   // ← 客户截图里的状态
      bothOpen: bothOpen.sidebarNavOverflow,
    },
    navContentH: { closed: closed.navContentH, moreOpen: moreOpen.navContentH, bothOpen: bothOpen.navContentH },
    navViewH: closed.navViewH,
    board,
    pageErrors: errors.slice(0, 3),
  }));

  // —— 断言（跑道要能变红，否则等于没跑）——
  if (errors.length) problems.push(`${c.name}：运行时报错 ${errors.length} 条 — ${errors[0]}`);
  // 界面压根没渲染出来时，上面每一项都是 null，而 null 谁也不违反 —— 先把这条拦住
  if (closed.navContentH == null) problems.push(`${c.name}：界面没渲染出来（侧栏都没有），这一档一个结论都不作数`);
  if (c.board) {
    const b = BOARD_BUDGET[c.board];
    if (!board) problems.push(`${c.name}：没进到任务看板（五列没找到），下面的排版结论一条都不作数`);
    else {
      if (board.cols !== 5) problems.push(`${c.name}：看板只有 ${board.cols} 列，应为 5`);
      if (!board.caveat) problems.push(`${c.name}：底部口径说明不见了 —— 收装饰收到信息头上了`);
      if (board.overflowX > 0) problems.push(`${c.name}：五列横向溢出 ${board.overflowX}px —— 「出错」那列会被挤出屏幕`);
      if (board.colW < b.colMin) problems.push(`${c.name}：列宽只有 ${board.colW}px（下限 ${b.colMin}），卡片全成省略号`);
      if (b.chromeMax != null && board.chromeH > b.chromeMax) {
        problems.push(`${c.name}：顶部横幅吃掉 ${board.chromeH}px（上限 ${b.chromeMax}）—— 矮屏没收紧，卡片区被挤`);
      }
      // 反向断言：宽屏必须仍然厚。少了这条，把矮屏那套当全局默认也会报绿。
      if (b.chromeMin != null && board.chromeH < b.chromeMin) {
        problems.push(`${c.name}：顶部横幅只剩 ${board.chromeH}px（下限 ${b.chromeMin}）—— 宽屏不该走紧凑排版，副标题/定时任务框是不是被误删了`);
      }
    }
  }
  await page.close();
}

await browser.close();
console.log("screenshots ->", OUT);

if (problems.length) {
  console.error("\n❌ 不通过:\n - " + problems.join("\n - "));
  process.exit(1);
}
console.log("\n✅ 通过：矮屏看板顶部横幅已收紧 / 宽屏仍是厚排版 / 五列不溢出 / 口径说明还在 / 零运行时报错");
