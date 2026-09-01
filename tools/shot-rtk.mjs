/**
 * 一次性排版取证：在浏览器里渲染 Token 压缩机页，mock 掉 Tauri 后端，截图看真实版式。
 * 只为「看得见才好改版式」，不是业务测试（业务对不对走 action conformance）。
 * 用法：node tools/shot-rtk.mjs [宽]
 */
import { chromium } from "playwright";

const W = Number(process.argv[2] || 1440);
const RTK_OK = {
  installed: true, enabled: true, version: "rtk 0.43.0",
  saved_tokens: 24400, saved_pct: 26.7, commands: 318,
  before_tokens: 91300, after_tokens: 66900,
  top_commands: [
    { command: "cargo test", count: 41, saved: 9800, pct: 78 },
    { command: "git status", count: 96, saved: 6100, pct: 62 },
    { command: "npm run build", count: 22, saved: 5200, pct: 71 },
    { command: "rg --files-with-matches", count: 84, saved: 2100, pct: 23 },
    { command: "git log --oneline -20", count: 75, saved: 1200, pct: 19 },
  ],
  daily: [
    { date: "2026-07-24", commands: 40, before: 12000, after: 9000, saved: 3000, pct: 25 },
    { date: "2026-07-25", commands: 62, before: 18000, after: 12800, saved: 5200, pct: 29 },
    { date: "2026-07-26", commands: 31, before: 9000, after: 7100, saved: 1900, pct: 21 },
    { date: "2026-07-27", commands: 55, before: 16000, after: 11400, saved: 4600, pct: 29 },
    { date: "2026-07-28", commands: 70, before: 21000, after: 14600, saved: 6400, pct: 30 },
    { date: "2026-07-29", commands: 60, before: 15300, after: 11970, saved: 3330, pct: 22 },
  ],
  ready: true, blockers: [],
};

// 三种状态都要看：正常 / 开着但没生效（唯一要用户动手的态）/ 没装
const STATES = {
  ok: RTK_OK,
  needfix: { ...RTK_OK, ready: false, blockers: ["rtk 不在 PATH 上，Claude Code 的 hook 调不到它", "settings.json 里的 hook 指向了一个已删除的路径"] },
  fresh: { installed: false, enabled: false, version: null, saved_tokens: null, saved_pct: null, commands: null, before_tokens: null, after_tokens: null, top_commands: [], daily: [], ready: false, blockers: [] },
};
const STATE = process.argv[3] || "ok";

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: W, height: 1200 }, deviceScaleFactor: 2 });
if (STATE === "update") await page.addInitScript(() => { window.__MOCK_HAS_UPDATE__ = true; });
await page.addInitScript((rtk) => {
  const resp = (cmd) => {
    if (cmd === "rtk_status") return rtk;
    if (cmd === "get_env") return { platform: "windows", is_usb: false, context_menu: false, open_dir: null };
    if (cmd === "check_update") {
      if (!window.__MOCK_HAS_UPDATE__) return { current: "0.9.81", latest: "0.9.81", has_update: false, notes: "", download_url: "", history: [] };
      return {
        current: "0.9.81", latest: "0.9.82", has_update: true, download_url: "https://u-claw.org.cn/uking/",
        notes: "① 修复中文用户名的电脑「点升级升不了」🔧——安装路径里带中文或全角括号时，替换脚本会整份乱码，升级无声失败还不留任何记录。现在改掉了，并且升级过程全程有日志，出问题点「意见反馈」就能带给作者。② Token 压缩机页面重排 📊——「每天省下的 token」趋势图之前根本没画出来，只剩一行日期；原理说明的标题被拦腰断字。都修好了。",
        history: [
          { version: "0.9.82", date: "2026-07-31", notes: "① 修复中文用户名的电脑「点升级升不了」🔧 ② Token 压缩机页面重排 📊" },
          { version: "0.9.81", date: "2026-07-30", notes: "① 工作台左栏大改，挑专家、配定时任务都不用离开工作台 🧭 ② 新增「自动化」定时任务 ⏰ ③ 泊舟 AI 小程序进了「实验室」🧪 ④ 崩溃终于能查了 🔍 ⑤ 修复老电脑打开文档预览就崩 📄" },
          { version: "0.9.80", date: "2026-07-29", notes: "① 工作台多了「看命令」🔍 ② 意见反馈加「屏幕协助」🖥️" },
          { version: "0.9.79", date: "2026-07-29", notes: "① 出问题更容易修好了 🔧 ② 修复「远程协助编号会变」的问题 🆔" },
        ],
      };
    }
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "detect_stack") return { node: null, npm: null, claude: null, codex: null, git: null };
    if (cmd === "get_device_key") return { key: "sk-demo", balance: null, recharge_url: "" };
    if (cmd.startsWith("list_") || cmd.startsWith("all_")) return [];
    return null;
  };
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd) => resp(cmd),
    transformCallback: (cb) => { const id = Math.floor(Math.random() * 1e9); window[`_cb${id}`] = cb; return id; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}, STATES[STATE]);

await page.goto("http://localhost:1430/", { waitUntil: "domcontentloaded" });
await page.waitForTimeout(2500);

// 侧栏「更多」可能是折叠的，先展开，再点 Token 压缩机
for (const label of ["更多", "Token 压缩机"]) {
  const el = page.locator(`text=${label}`).first();
  if (await el.count()) { await el.click().catch(() => {}); await page.waitForTimeout(600); }
}
// 「有新版」态：先拍侧栏，再点「看看这版改了什么」拍更新日志弹层
if (STATE === "update") {
  await page.screenshot({ path: `tools/rtk-update-sidebar.png` });
  const link = page.locator("text=改了什么").first();
  if (await link.count()) { await link.click(); await page.waitForTimeout(800); }
  else console.log("!! 没找到「看看这版改了什么」入口");
}
await page.waitForTimeout(1200);
await page.screenshot({ path: `tools/rtk-${STATE}-${W}.png`, fullPage: true });
console.log(`saved tools/rtk-${STATE}-${W}.png`);
await browser.close();
