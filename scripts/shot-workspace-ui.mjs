/**
 * U-Workspace 界面取证 —— 截图 + 量真实像素，给「字太多 / 侧栏占地方」这类判断提供依据。
 *
 * 为什么不开 exe：起第二个 U-King 会往 crashlog 写第二份会话标记，强杀就留一个「上次异常退出」
 * 的假信号（`--crash-test` 盯的正是它）。布局是纯前端的事，dev server + 真 Chromium 就够，
 * 一个字节的用户真实状态都不碰。
 *
 * 量什么（都是「字太多」的可证伪指标，不靠肉眼）：
 *  - 侧栏宽度 + 导航内容高度（收起后应当显著变窄）
 *  - 首屏**可见文字总字数**、最长一段的字数 —— 「字多」要能被数出来才好判断改没改动
 *  - 中间空态区的字数（那段说明是重灾区）
 *  - 横向溢出（ChatGPT 那份稿子点名的问题）
 *
 * 用法：node scripts/shot-workspace-ui.mjs   （需先 pnpm dev；换端口用 UKING_DEV_URL=）
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = process.env.UKING_SHOT_OUT || "C:/Users/example/AppData/Local/Temp/uking-ws-ui";

const CASES = [
  { name: "1280x720", w: 1280, h: 720 - 32 },
  { name: "1440x900", w: 1440, h: 900 - 40 - 32 },
  { name: "1920x1080", w: 1920, h: 1080 - 40 - 32 },
];

/** 最小 Tauri shim —— 只为让界面渲染出来，不模拟业务。 */
const TAURI_SHIM = () => {
  const WS = "C:\\demo\\uking-mini";
  const fake = (cmd, args) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\demo", installed: true, opened_dir: WS };
    if (cmd === "list_tools") return [];
    if (cmd === "list_tasks") return [];
    if (cmd === "list_ai_tasks") return { days: 7, active_window_secs: 300, truncated: false, notes: [], counts: {}, sources: [], tasks: [] };
    if (cmd === "upsert_task") return args?.task ?? null;
    if (cmd === "list_automations") return { jobs: [] };
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false };
    if (cmd === "list_dir") return [];
    if (cmd?.startsWith("plugin:event|")) return 1;
    return null;
  };
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

/** 首屏可见文字的可证伪指标 —— 「字太多」得能被数出来。 */
const TEXT_METRICS = () => {
  const vis = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return false;
    if (r.bottom < 0 || r.top > window.innerHeight || r.right < 0 || r.left > window.innerWidth) return false;
    const s = getComputedStyle(el);
    return s.visibility !== "hidden" && s.display !== "none" && s.opacity !== "0";
  };
  // 只数叶子节点的文字，避免父子重复计数
  const leaves = [...document.querySelectorAll("body *")].filter(
    (el) => el.children.length === 0 && el.textContent?.trim() && vis(el),
  );
  const texts = leaves.map((el) => el.textContent.trim());
  const chars = texts.reduce((n, s) => n + s.length, 0);
  const longest = texts.reduce((m, s) => (s.length > m.length ? s : m), "");
  const aside = document.querySelector("aside");
  const nav = aside?.querySelector("nav");
  return {
    visibleChars: chars,
    visibleNodes: texts.length,
    longestText: longest.slice(0, 60),
    longestLen: longest.length,
    sidebarW: aside ? Math.round(aside.getBoundingClientRect().width) : null,
    sidebarNavContentH: nav?.scrollHeight ?? null,
    sidebarNavOverflow: nav ? Math.max(0, nav.scrollHeight - nav.clientHeight) : null,
    docOverflowX: Math.max(0, document.documentElement.scrollWidth - window.innerWidth),
    docOverflowY: Math.max(0, document.documentElement.scrollHeight - window.innerHeight),
    viewport: { w: window.innerWidth, h: window.innerHeight },
  };
};

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
const report = [];

for (const c of CASES) {
  const page = await browser.newPage({ viewport: { width: c.w, height: c.h } });
  await page.addInitScript(TAURI_SHIM);
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e).slice(0, 160)));
  await page.goto(URL, { waitUntil: "networkidle" }).catch(() => {});
  await page.waitForSelector("aside", { timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(2000);

  await page.screenshot({ path: `${OUT}/${c.name}-1-home.png` });
  const home = await page.evaluate(TEXT_METRICS);

  // 进 U-Workspace（客户说「字太多」的那一屏）
  await page.getByRole("button", { name: "U-Workspace", exact: false }).first().click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(1800);
  await page.screenshot({ path: `${OUT}/${c.name}-2-workspace.png` });
  const ws = await page.evaluate(TEXT_METRICS);

  // ★ U-CLI（终端）+ 右边那条文件栏 —— 1.0.3 加的，只有真点一遍才知道摆没摆对
  let cli = null;
  await page.getByRole("button", { name: "终端", exact: true }).first().click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${OUT}/${c.name}-3-cli.png` });
  // 🔴 取**顶栏那颗**（终端态专属，在 [对话|终端] 旁边），别用 .last() ——
  // 面板 tab 里也有一颗同名的，点它会把整个面板切走、终端就没了（探针曾这么量出 termColW=0）。
  const filesBtn = page.getByRole("button", { name: "文件", exact: true }).first();
  await filesBtn.click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${OUT}/${c.name}-4-cli-files.png` });
  cli = await page.evaluate(() => {
    const t = (s) => document.querySelector(s);
    const w = (el) => (el ? Math.round(el.getBoundingClientRect().width) : null);
    // 终端列 = xterm 宿主往上找到那条 flex-1；文件栏 = 带 border-l 的固定宽兄弟
    const term = t(".xterm")?.closest(".flex-1") ?? null;
    // 🔴 量**渲染后的实际宽度**，别去 parse style 字符串：宽度现在是 CSS `max(…)` 表达式，
    // parseInt 会得 NaN → 量不到 → 断言恒真。「没量到」必须跟「量到了但不对」一样刺眼。
    const tree = [...document.querySelectorAll("div")].find(
      (d) => d.className?.includes?.("border-l") && d.getBoundingClientRect().width > 200 && d.querySelector(".overflow-y-auto"),
    );
    return {
      termColW: w(term),
      filesColW: w(tree ?? null),
      // 终端还在不在（点文件不许把终端顶掉，这是这次改动的全部意义）
      termStillMounted: !!document.querySelector(".xterm"),
    };
  });

  report.push({ case: c.name, home, workspace: ws, cli, errors: errors.slice(0, 3) });
  await page.close();
}

await browser.close();
console.log(JSON.stringify({ out: OUT, report }, null, 2));
