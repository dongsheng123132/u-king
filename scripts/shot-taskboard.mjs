/**
 * 任务看板取证 —— 拿**真后端的真数据**喂给**真组件**，在真 Chromium 里量出来。
 *
 * 为什么不直接开 exe：客户的 U-King 常开着，单实例锁会把第二个实例挡回去；强开/强杀还会往
 * crashlog 写一份假的「上次异常退出」（`--crash-test` 盯的正是它）。
 * 要在真 Tauri 窗口里看，用**绿色测试版**（换 identifier，见 `scripts/build-green-test.sh`）。
 *
 * 这跑道验的是什么、不验什么，说清楚：
 *  - ✅ **验**：影核动作 `runtime.ai_tasks.inspect` 的真实返回，经 TaskBoard 真组件渲染后 ——
 *    发现卡片有没有先问一句、勾选后卡片有没有进板、来源筛选有没有生效、选择跨刷新有没有存住、
 *    有没有 React key 撞车或运行时报错。数据是**当场跑 exe 拿的**，不是手捏的 fixture。
 *  - ❌ **不验**：Tauri webview 里的真实表现（字体/DPI/滚动惯性）。那得在真 exe 上看。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/shot-taskboard.mjs`
 */
import { chromium } from "playwright";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

const URL = "http://localhost:1430/";
const OUT = path.join(process.env.TEMP || "/tmp", "uking-taskboard");
const EXE = "src-tauri/target/debug/u-king-mini.exe";
const PREF_KEY = "uking.board.ai_sources.v1";

// —— 真数据：当场调影核动作 ——
console.log("[1/5] 跑 runtime.ai_tasks.inspect 拿真数据…");
const aiTasks = JSON.parse(
  execFileSync(EXE, ["action", "run", "runtime.ai_tasks.inspect", "--json"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "ignore"],
  }),
);
const detected = aiTasks.sources.filter((s) => s.readable && s.count > 0);
console.log(
  `      tasks=${aiTasks.tasks.length} running=${aiTasks.counts.running} 探测到=${detected.map((s) => `${s.tool}:${s.count}`).join(",")}`,
);

// 定时任务：读真文件（没有就空着——空态也得能看）
let automations = { jobs: [], running_id: null };
const autoFile = path.join(homedir(), ".uking", "automations.json");
if (existsSync(autoFile)) {
  try {
    const raw = JSON.parse(readFileSync(autoFile, "utf8"));
    const jobs = Array.isArray(raw) ? raw : raw.jobs || [];
    automations = {
      jobs: jobs.map((j) => ({
        id: j.id, name: j.name, enabled: !!j.enabled,
        next_run_at: j.next_run_at || 0, last_ok: j.last_ok ?? null, last_message: j.last_message || "",
      })),
      running_id: null,
    };
  } catch { /* 读不了就空着 */ }
}
console.log(`[2/5] 定时任务 ${automations.jobs.length} 条`);

/** 本工作台的会话：造两条**两种状态**的，为的是验「自己的卡 vs 别家的卡」同列共存。 */
const OWN_TASKS = [
  { id: "sess-a", name: "u-king简化版-u盘版本", dir: "C:\\Users\\me\\Desktop\\claude", status: "running", source: "manual", assignee: null, external_ref: null, last_opened_at: Date.now(), created_at: Date.now(), kind: "task", project: "p1" },
  { id: "sess-b", name: "本境协议", dir: "D:\\uking编程\\本境协议", status: "error", source: "manual", assignee: null, external_ref: null, last_opened_at: Date.now(), created_at: Date.now(), kind: "task", project: "p2" },
];

const SHIM = (data) => {
  const fake = (cmd, args) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\me", installed: true, opened_dir: null };
    if (cmd === "list_tools") return [];
    if (cmd === "list_tasks") return data.own;
    if (cmd === "upsert_task") return args?.task ?? null;
    if (cmd === "list_automations") return data.automations;
    if (cmd === "list_ai_tasks") return data.aiTasks;
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false };
    if (cmd?.startsWith("plugin:event|")) return 1;
    return null;
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => Promise.resolve(fake(cmd, args)),
    transformCallback: (cb) => { const id = Math.floor(Math.random() * 1e9); window[`_${id}`] = cb; return id; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
};

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
// React 的 key 撞车、setState 警告只走 console.error，不是 pageerror —— 两条都得收。
page.on("console", (m) => { if (m.type() === "error") errors.push("console: " + m.text().slice(0, 200)); });

await page.addInitScript(SHIM, { aiTasks, automations, own: OWN_TASKS });
// 每次都从**没问过**的状态起跑，否则上一轮存下的选择会让发现卡片不出现（跑道又骗人）。
//
// 🔴 `addInitScript` 在**每一次导航**都会重跑，包括 `reload()` —— 第一版就是无条件
// `removeItem`，于是「刷新后选择还在吗」这条永远是假红：不是产品没存住，是跑道自己擦掉了。
// 用 sessionStorage 当一次性哨兵（它跨 reload 存活、跨用例不存活）。
await page.addInitScript((k) => {
  try {
    if (!sessionStorage.getItem("__harness_pref_cleared")) {
      localStorage.removeItem(k);
      sessionStorage.setItem("__harness_pref_cleared", "1");
    }
  } catch {}
}, PREF_KEY);

/** 从任意状态点进看板。**刷新之后也必须走这一整条** —— 页面重载会退回首页那个 tab，
 *  只点「看板」是点不到的（那次少点一步，量出来 0 条，差点被当成「勾选没存住」的产品 bug）。 */
const enterBoard = async () => {
  await page.getByRole("button", { name: "U-Workspace", exact: false }).first().click({ timeout: 5000 });
  await page.waitForTimeout(1000);
  await page.getByRole("button", { name: "看板", exact: false }).first().click({ timeout: 5000 });
  await page.waitForTimeout(2500); // 等 list_ai_tasks 回来 + 渲染
};

const gotoBoard = async () => {
  await page.goto(URL, { waitUntil: "networkidle" });
  await page.waitForTimeout(1800);
  await enterBoard();
};

const measure = () =>
  page.evaluate(() => {
    const cols = [...document.querySelectorAll(".grid.grid-cols-5 > div")].map((c) => {
      const head = c.querySelector("header");
      return {
        label: head?.querySelector("span:nth-child(2)")?.textContent?.trim() ?? "?",
        count: Number(head?.querySelector("span.ml-auto")?.textContent?.trim() ?? 0),
        cards: c.querySelectorAll(".overflow-y-auto > button").length,
      };
    });
    return {
      cols,
      total: cols.reduce((a, c) => a + c.count, 0),
      hasDiscovery: !!document.querySelector('[data-testid="board-discover-confirm"]'),
      discoverOptions: [...document.querySelectorAll('[data-testid^="board-discover-"]')]
        .map((b) => b.getAttribute("data-testid"))
        .filter((t) => t !== "board-discover-confirm" && t !== "board-discover-skip"),
      chips: [...document.querySelectorAll('[data-testid^="board-chip-"]')].map((b) => b.getAttribute("data-testid")),
      hasFooterCaveat: document.body.innerText.includes("永远不进「出错」列"),
      // 🔴 这块板量的是**会话**，不是任务。「已完成 / 待办」是任务语义，它兑现不了：
      //    用户当天在干的四件活全躺在原来那个「已完成」列里 —— 一件都没做完，
      //    只是终端切走了五分钟。列名一旦漂回任务语义，这条就该红。
      taskSemanticLabels: cols.map((c) => c.label).filter((l) => ["已完成", "待办", "进行中"].includes(l)),
      hasDoneCaveat: document.body.innerText.includes("不代表事情做完了"),
    };
  });

console.log("[3/5] 第一次进板：应该**先问一句**，而不是直接铺满…");
await gotoBoard();
const fresh = await measure();
await page.screenshot({ path: `${OUT}/board-1-discovery.png` });

console.log("[4/5] 勾上全部 → 加到看板…");
if (fresh.hasDiscovery) {
  await page.locator('[data-testid="board-discover-confirm"]').click();
  await page.waitForTimeout(800);
}
const included = await measure();
await page.screenshot({ path: `${OUT}/board-2-included.png` });

// 关掉「Claude Code」这个来源，验筛选真的把卡片摘掉了（不是只把 chip 变灰）。
//
// 🔴 按 role+name 找过一版，**静默匹配不到**：图标是 <img alt="claude">，无障碍名被拼成
// 「claude Claude Code 156」，`^Claude Code…$` 锚点全落空 → 点击没发生 → 前后数字当然一样 →
// 而当时那版把「找不到 chip」写成了跳过断言，于是**一个没跑过的检查报了绿**。
// 现在认稳定的 data-testid，且找不到就**直接判失败**：跑道骗人比功能坏更贵。
const claudeChip = page.locator('[data-testid="board-chip-claude"]');
const hadChip = (await claudeChip.count()) > 0;
if (hadChip) { await claudeChip.click(); await page.waitForTimeout(600); }
const filtered = await measure();
await page.screenshot({ path: `${OUT}/board-3-filtered.png` });

// ★ 点卡片能不能真的「跳进去继续干活」。
//
// 这是用户直接点名要验的一条，而本跑道此前**只验渲染和筛选、完全没点过卡片** ——
// 一块看得见但点不动的板，比没有更糟（用户会以为是自己不会用）。
// 两条路分开验，因为它们的语义不同：
//   ① 本工作台的卡 → activate(id) + 切 chat，是**切回已有会话**（PTY/对话都还活着）
//   ② 别家 AI 的卡 → 拿它的工作目录 addTask(reuse=true)，是**接着那个活开一个会话**
console.log("[6/6] 点卡片：能不能真跳进去继续干活…");
// 🔴 列名必须**精确**匹配，不能 includes：列名改成「在跑 / 没在跑」之后，
//    `"没在跑".includes("在跑")` 是 true —— 于是这条跑道会安安静静去点第一列，
//    「点『在跑』的卡能不能跳进去」从此再也没验过，而它照样报绿。
const clickFirstCardIn = async (colLabel) =>
  page.evaluate((l) => {
    const col = [...document.querySelectorAll(".grid.grid-cols-5 > div")].find(
      (c) => c.querySelector("header span:nth-child(2)")?.textContent?.trim() === l);
    const btn = col?.querySelector(".overflow-y-auto > button");
    if (!btn) return null;
    const title = btn.textContent?.trim().slice(0, 30) ?? "";
    btn.click();
    return title;
  }, colLabel);

// 先回看板（上一步刷新后停在看板），点「在跑」第一张
const clickedTitle = await clickFirstCardIn("在跑");
await page.waitForTimeout(1200);
const afterClick = await page.evaluate(() => ({
  // 切到 chat 视图 = 看板那五列不再挂着
  boardGone: !document.querySelector(".grid.grid-cols-5"),
  // 对话区的输入框在 = 真到了能干活的地方，而不是切了个空视图
  composerThere: !!document.querySelector("textarea"),
  bodyHead: document.body.innerText.slice(0, 120),
}));
console.log(`  点了「进行中」第一张（${clickedTitle ?? "没有卡片"}）→ 看板收起=${afterClick.boardGone} 输入框在=${afterClick.composerThere}`);

console.log("[5/6] 刷新页面：选择必须**存住**，不能又问一遍…");
await page.reload({ waitUntil: "networkidle" });
await page.waitForTimeout(1800);
await enterBoard();
const reloaded = await measure();

// ★ 窄窗口下五列必须全露出来。
// 这条是被真窗口逼出来的：物理 1418px @125% DPI = CSS 1134px，侧栏+会话栏吃掉后板子只剩 722px，
// 而 grid 的 min-w 原本写死 860 → 横向溢出 138px，**「出错」那列被挤到屏幕外**。
// 浏览器跑道当时开的是 1440 宽，完全看不到这个问题 —— 所以必须显式按真机 CSS 宽度量一遍。
await page.setViewportSize({ width: 1134, height: 758 });
await page.waitForTimeout(900);
const narrow = await page.evaluate(() => {
  const grid = document.querySelector(".grid.grid-cols-5");
  const scroller = grid?.parentElement;
  return {
    avail: scroller?.clientWidth ?? 0,
    need: grid?.scrollWidth ?? 0,
    cols: grid ? grid.children.length : 0,
  };
});
await page.screenshot({ path: `${OUT}/board-4-narrow.png` });

console.log("");
console.log("  窄窗口(1134css):", `板子可用 ${narrow.avail}px，五列要 ${narrow.need}px，溢出 ${Math.max(0, narrow.need - narrow.avail)}px`);
console.log("  第一次进板   :", `总数=${fresh.total} 发现卡片=${fresh.hasDiscovery ? "有" : "无"} 可勾=${fresh.discoverOptions.length}`);
console.log("  勾上全部后   :", included.cols.map((c) => `${c.label}=${c.count}`).join("  "));
console.log("  关掉 Claude  :", filtered.cols.map((c) => `${c.label}=${c.count}`).join("  "));
console.log("  刷新后       :", `总数=${reloaded.total} 发现卡片=${reloaded.hasDiscovery ? "又问了一遍(坏)" : "没再问(对)"}`);
console.log("  来源 chip    :", included.chips.join(" | "));
console.log("  底部口径说明 :", included.hasFooterCaveat ? "有" : "缺");
console.log("  截图         :", OUT);

const own = OWN_TASKS.length;
const claudeCount = aiTasks.sources.find((s) => s.tool === "claude")?.count ?? 0;
const problems = [];
// 🔴 没进到看板时 cols 是空数组，total 就成了 0 —— 那不是「板上没东西」，是**没量到**。
// 不显式拦一下，任何一次导航失败都会伪装成一条产品 bug。
for (const [name, m] of [["第一次进板", fresh], ["勾选后", included], ["筛选后", filtered], ["刷新后", reloaded]]) {
  if (m.cols.length !== 5) problems.push(`${name}：没进到看板（只找到 ${m.cols.length} 列，应为 5）`);
}
if (errors.length) problems.push(`运行时报错 ${errors.length} 条: ${errors.slice(0, 3).join(" / ")}`);
// ★ 这条是这次改动的核心：探测到 ≠ 直接摆上去
if (!fresh.hasDiscovery) problems.push("第一次进板没弹发现卡片 —— 探测到别家 AI 却没问就用，正是要避免的「突兀」");
if (fresh.total !== own) problems.push(`没勾之前板上有 ${fresh.total} 条，应该只有本工作台的 ${own} 条`);
if (fresh.discoverOptions.length !== detected.length) {
  problems.push(`发现卡片给了 ${fresh.discoverOptions.length} 个勾选项，但后端探测到 ${detected.length} 个来源`);
}
if (included.total < aiTasks.tasks.length + own) {
  problems.push(`勾上全部后板上 ${included.total} 条 < 后端 ${aiTasks.tasks.length} + 本工作台 ${own}`);
}
if (!hadChip) problems.push("找不到 Claude Code 的来源 chip，筛选这条根本没验到");
else if (included.total - filtered.total !== claudeCount) {
  problems.push(`关掉 Claude 后少了 ${included.total - filtered.total} 条，但它自报 ${claudeCount} 条 —— 对不上`);
}
if (reloaded.hasDiscovery) problems.push("刷新后又弹了一次发现卡片 —— 选择没存住，会变成骚扰");
if (reloaded.total !== filtered.total) problems.push(`刷新后 ${reloaded.total} 条 ≠ 刷新前 ${filtered.total} 条 —— 勾选状态没存对`);
if (!included.hasFooterCaveat) problems.push("底部口径说明没渲染出来");
if (included.taskSemanticLabels.length)
  problems.push(`列名漂回任务语义：${included.taskSemanticLabels.join("/")} —— 这块板量的是会话，「已完成」它证明不了`);
if (!included.hasDoneCaveat)
  problems.push("底部没写「已结束 ≠ 已完成」—— 光改列名不写口径，下一个人还是会读成「做完了」");
if (clickedTitle === null) problems.push("「在跑」列一张卡都没有，跳转这条没验到");
else if (!afterClick.boardGone) problems.push("点了卡片但看板没收起 —— 没跳过去");
else if (!afterClick.composerThere) problems.push("跳过去了但没有输入框 —— 到不了能干活的地方");
if (narrow.need > narrow.avail) {
  problems.push(`窄窗口下横向溢出 ${narrow.need - narrow.avail}px —— 「出错」那列会被挤出屏幕，而横向滚动条没人会注意到`);
}

await browser.close();
if (problems.length) {
  console.error("\n❌ 不通过:\n - " + problems.join("\n - "));
  process.exit(1);
}
console.log("\n✅ 通过：先问后加 / 勾选生效 / 筛选准确 / 选择存住 / 零运行时报错");
