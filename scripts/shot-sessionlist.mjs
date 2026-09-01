/**
 * 给 U-Workspace 左栏（项目 / 会话列表）出图 —— 客户 2026-08-20：
 * 「文件夹和下面的任务对话名重复，关键是**区分度不够，一排在一起，不好点**」。
 *
 * 🔴 不截用户在跑的那个 U-King，连独立 dev 实例；`list_tasks` 喂假数据，
 *    故意造出**项目名和会话名一样**的情形（客户说的就是这个）。
 *
 * 用法：pnpm vite --port 1467，然后 UKING_DEV_URL=http://localhost:1467/ node scripts/shot-sessionlist.mjs
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = "shots/sessionlist";
mkdirSync(OUT, { recursive: true });

/**
 * 🔴 字段是 `name` 不是 `title` —— 第一版喂了 `title`，于是全部回落成
 * `dirBasename(t.dir)`，我差点把「三条全叫 uking-mini」报成产品 bug。**是跑道喂错了。**
 *
 * 而真实默认**恰恰也是文件夹名**（`types.ts`：`name: string; // 显示名（默认文件夹名）`），
 * 所以客户说的重名是真的：会话默认名 = 项目组头名。下面按真实默认造。
 */
const TASKS = [
  { id: "t1", name: "uking-mini", tool: "claude", kind: "tool", dir: "D:/claude/uking-mini", status: "idle" },
  { id: "t2", name: "uking-mini", tool: "codex", kind: "tool", dir: "D:/claude/uking-mini", status: "running" },
  { id: "t3", name: "改左栏区分度", kind: "task", dir: "D:/claude/uking-mini", status: "idle" },
  { id: "t4", name: "2origin", tool: "claude", kind: "tool", dir: "C:/Users/user1/Documents/ukingkaifa/2origin", status: "idle" },
  { id: "t5", name: "2origin", tool: "hermes", kind: "tool", dir: "C:/Users/user1/Documents/ukingkaifa/2origin", status: "idle" },
  { id: "t6", name: "俄罗斯方块游戏", tool: "claude", kind: "tool", dir: "C:/Users/user1/Documents/俄罗斯方块游戏", status: "idle" },
];

const SHIM = (tasks) => {
  const fake = (cmd) => {
    if (cmd === "list_tasks") return tasks;
    if (cmd === "list_tools") return [];
    if (cmd === "list_providers") return [];
    if (cmd === "driver_status") return { active: {} };
    if (cmd === "get_device_key") return { key: "sk-demo", balance_cny: 1.74, recharge_url: "" };
    if (cmd === "check_update") return { has_update: false };
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

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
await page.addInitScript(SHIM, TASKS);
await page.goto(URL, { waitUntil: "networkidle" });
await page.getByText("U-Workspace", { exact: false }).first().click().catch(() => {});
await page.waitForTimeout(1200);

if (await page.getByText("界面已停止").count()) {
  const txt = await page.textContent("body");
  throw new Error(`崩在故障边界，没截到界面：\n${(txt || "").slice(0, 300)}`);
}

// 整屏 + 左栏特写（左栏才是要看的东西，整屏里它太小）
await page.screenshot({ path: `${OUT}/1-整屏.png` });
const panel = page.locator("text=已打开的项目").locator("xpath=ancestor::div[3]").first();
await panel.screenshot({ path: `${OUT}/2-左栏特写.png` }).catch(async () => {
  await page.screenshot({ path: `${OUT}/2-左栏特写.png`, clip: { x: 0, y: 0, width: 460, height: 900 } });
});
console.log(`  📸 ${OUT}/1-整屏.png`);
console.log(`  📸 ${OUT}/2-左栏特写.png`);
await browser.close();
