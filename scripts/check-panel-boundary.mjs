/**
 * 「一个面板炸 ≠ 整个产品白掉」这条的跑道 —— 真 PanelBoundary + 真 ErrorBoundary + 真卸载。
 *
 * ## 它盯的是什么
 * 2026-08-16 盘点：最痛的三条 bug 是同一个形状 —— **一个叶子模块的故障，半径是整个产品**。
 *  - #402/#403：切大脑 → 卸 TermPanel → xterm dispose 抛错 → 冒到根 ErrorBoundary → 整棵树被卸
 *    → 客户报「U-King 自己重启了」（进程一直活着，被换掉的是整个界面）。
 *  - 0.9.99/0.9.100 Mac 白屏：两条正则 lookbehind 顶层求值 → 整个前端起不来。
 *
 * 根因是逐个修的（#403 的根因已由 50d0dc6 收掉），**半径是一次性压的**。
 * `PanelBoundary` 就是那一次性的一刀：下一个还没出现的抛错，不该再享受「拆掉整个界面」的待遇。
 *
 * ## 判据（非视觉，可自动判）
 * 让面板里的组件**必抛**，分渲染期 / 卸载期两种时机，各跑两档：
 *  - `legacy` 档：**变异验证** —— 同样的树，但**不包** PanelBoundary。根 ErrorBoundary 必须接管
 *    （「界面已停止」出现、侧栏消失）。它证明这条跑道真的会红。
 *  - `fixed`  档：包上 PanelBoundary。侧栏和兄弟面板**必须还活着**，「界面已停止」**不许**出现。
 *
 * 没有 legacy 档，fixed 档绿了说明不了任何事 —— 一条永远不红的断言等于没有断言。
 * （同 `check-term-teardown.mjs`；那条盯的是「别抛」，这条盯的是「抛了也只炸一块」。两件事。）
 *
 * 另外两条顺带钉死的：
 *  - 上报带面板名 —— #403 出来时「无人认领」，掉进「多半是没装好/驱动没配对」那句写死的猜测里；
 *  - 「重试」只重挂这一块 —— 不是 location.reload()，其他面板的 PTY / 会话不受影响。
 *
 * **验不到**：真 WebView2 里的原生崩溃（渲染进程整个没了）—— 那不是 JS 错误，任何 JS 边界都接不住，
 * 归 term_ping 心跳 + Rust 侧收尸管。这条跑道只钉「JS 抛错的爆炸半径」。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-panel-boundary.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync, readFileSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";

const SHIM = () => {
  window.__reports = [];
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      if (cmd === "report_bug") window.__reports.push({ ...(args || {}) });
      if (cmd?.startsWith("plugin:event|")) return Promise.resolve(1);
      return Promise.resolve(null);
    },
    transformCallback: (cb) => {
      const id = Math.floor(Math.random() * 1e9);
      window[`_${id}`] = cb;
      return id;
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
};

const PROBE_NAME = "__panel-boundary-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>panel boundary probe</title></head>
<body style="margin:0"><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import { ErrorBoundary } from "/src/components/ErrorBoundary";
import { PanelBoundary } from "/src/components/PanelBoundary";

const q = new URLSearchParams(location.search);
const mode = q.get("mode") || "fixed";     // fixed = 包 PanelBoundary；legacy = 不包（变异档）
const when = q.get("when") || "render";    // render = 渲染期抛；unmount = 卸载 cleanup 抛

// 复刻 #403 的那个 TypeError，连消息都一样 —— 让跑道盯的是真实现场那一类错。
const BOOM = () => new TypeError("Cannot read properties of undefined (reading '_isDisposed')");

window.__boom = true;   // 「重试」档会把它关掉，用来验证重试真的重挂了子树

/** 渲染期抛：最常见的一类（数据形状变了、上游返回 null）。 */
function BoomOnRender() {
  if (window.__boom) throw BOOM();
  return React.createElement("div", { id: "panel-ok" }, "PANEL-OK");
}

/** 卸载期抛：#403 的真实形状 —— effect cleanup 里抛，React 一样会往上冒。 */
function BoomOnUnmount() {
  React.useEffect(() => () => {
    if (window.__boom) throw BOOM();
  }, []);
  return React.createElement("div", { id: "panel-ok" }, "PANEL-OK");
}

function Panel() {
  return React.createElement(when === "unmount" ? BoomOnUnmount : BoomOnRender);
}

function Shell() {
  // unmount 档：先正常挂着，等探针喊 __unmount() 再拆 —— 边界比它守的东西活得久，才接得住卸载期的错
  const [mounted, setMounted] = React.useState(true);
  window.__unmount = () => setMounted(false);
  window.__ready = true;

  const panel = mounted ? React.createElement(Panel) : React.createElement("div", { id: "panel-gone" }, "GONE");
  const guarded =
    mode === "legacy"
      ? panel                                                    // 变异档：裸奔，错直冲根边界
      : React.createElement(PanelBoundary, { name: "probe-panel" }, panel);

  return React.createElement(
    "div",
    { id: "shell" },
    React.createElement("div", { id: "sidebar" }, "SIDEBAR-ALIVE"),
    React.createElement("div", { id: "slot", style: { height: "300px" } }, guarded),
    React.createElement("div", { id: "sibling" }, "SIBLING-ALIVE"),
  );
}

createRoot(document.getElementById("root")).render(
  React.createElement(ErrorBoundary, null, React.createElement(Shell)),
);
</script></body></html>`;

const fails = [];
writeFileSync(PROBE_NAME, PROBE_HTML);
process.on("exit", () => {
  try {
    unlinkSync(PROBE_NAME);
  } catch {
    /* ignore */
  }
});

const browser = await chromium.launch();

/** 跑一档：挂上 →（必要时）卸载 → 回答「谁接管了、侧栏还在不在」。 */
async function run(mode, when) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 700 } });
  await page.addInitScript(SHIM);
  await page.goto(`${URL}${PROBE_NAME}?mode=${mode}&when=${when}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(400);
  if (when === "unmount") {
    await page.waitForFunction(() => window.__ready === true, null, { timeout: 15000 }).catch(() => {});
    await page.evaluate(() => window.__unmount?.());
    await page.waitForTimeout(400);
  }
  const r = await page.evaluate(() => ({
    text: document.body.innerText,
    sidebar: !!document.getElementById("sidebar"),
    sibling: !!document.getElementById("sibling"),
    reports: window.__reports || [],
  }));
  return { page, ...r, wholeAppDown: r.text.includes("界面已停止"), panelCard: r.text.includes("这一块出问题了") };
}

for (const when of ["render", "unmount"]) {
  const label = when === "render" ? "渲染期抛错" : "卸载期抛错（#403 的形状）";

  console.log(`\n[${when}] 变异验证：不包边界时，${label} 必须打崩整个界面 —— 证明这条跑道会红…`);
  {
    const r = await run("legacy", when);
    if (!r.wholeAppDown)
      fails.push(`[${when}/legacy] 没包边界却也没崩 —— 故障没注进去，那么 fixed 档绿了不能证明任何事`);
    else if (r.sidebar)
      fails.push(`[${when}/legacy] 根边界接管了但侧栏还在？探针树写错了，对比不成立`);
    else console.log(`     ✓ 如期整屏崩掉（根 ErrorBoundary 接管，侧栏一起没了）`);
    await r.page.close();
  }

  console.log(`[${when}] 包上 PanelBoundary：只许炸那一块，外壳必须活着…`);
  {
    const r = await run("fixed", when);
    if (r.wholeAppDown) fails.push(`[${when}/fixed] 整个界面仍被根边界接管 —— 边界没兜住，等于没加`);
    if (!r.sidebar) fails.push(`[${when}/fixed] 侧栏没了 —— 外壳被一起拆掉`);
    if (!r.sibling) fails.push(`[${when}/fixed] 兄弟面板没了 —— 隔离不成立`);
    if (!r.panelCard) fails.push(`[${when}/fixed] 没渲染出面板级兜底卡片，用户会看到一块空白而无从判断`);
    const rep = r.reports.find((x) => x.kind === "ui_panel_crash");
    if (!rep) fails.push(`[${when}/fixed] 没上报 ui_panel_crash —— 崩了没人知道，等于回到 #403 之前`);
    else if (!String(rep.summary || "").includes("probe-panel"))
      fails.push(`[${when}/fixed] 上报里没有面板名（summary=${rep.summary}）—— #403「无人认领」的原病复发`);
    if (!fails.length || !fails.some((f) => f.includes(`[${when}/fixed]`)))
      console.log(`     ✓ 只炸一块（侧栏 + 兄弟面板都在，兜底卡片就位，上报带面板名 probe-panel）`);
    await r.page.close();
  }
}

console.log(`\n[retry] 「重试」只重挂这一块，不是 reload 整个 U-King…`);
{
  const r = await run("fixed", "render");
  await r.page.evaluate(() => {
    window.__boom = false;
  });
  const btn = r.page.getByRole("button", { name: /重试/ });
  if ((await btn.count()) === 0) fails.push("[retry] 兜底卡片上没有「重试」按钮");
  else {
    await btn.first().click();
    await r.page.waitForTimeout(300);
    const back = await r.page.evaluate(() => !!document.getElementById("panel-ok"));
    if (!back) fails.push("[retry] 点了重试面板没回来 —— 用户只能重启整个 U-King，等于没修");
    else console.log("     ✓ 面板自己回来了（没有 location.reload，其他面板的 PTY/会话不受影响）");
  }
  await r.page.close();
}

// —— 静态一刀：新加的页别再漏包边界 ——
// 运行期跑道只能证明「边界能兜」，证明不了「每个入口都包了」。这一条盯后者：
// 真正会复发的方式不是边界坏掉，是有人加了个新 tab 忘了包。
console.log(`\n[静态] 面板入口是否都包上了…`);
{
  const app = readFileSync("src/App.tsx", "utf8");
  const split = readFileSync("src/opencodex/SplitArea.tsx", "utf8");
  const want = [
    ["src/App.tsx", app, "UWorkspace", 1],
    ["src/App.tsx", app, "TerminalPage", 1],
    ["src/App.tsx", app, "ToolAppView", 1],
    ["src/opencodex/SplitArea.tsx", split, "ChatPanel", 1],
    ["src/opencodex/SplitArea.tsx", split, "SplitContainer", 1],
  ];
  for (const [file, src, comp] of want) {
    // 粗判：组件出现处往前 400 字符内要能看到 PanelBoundary 开标签
    const at = src.indexOf(`<${comp}`);
    if (at < 0) {
      fails.push(`[静态] ${file} 里找不到 <${comp}> —— 跑道过期了，去对一下这个文件`);
      continue;
    }
    if (!src.slice(Math.max(0, at - 400), at).includes("<PanelBoundary"))
      fails.push(`[静态] ${file} 的 <${comp}> 没包在 PanelBoundary 里 —— 它崩了会带走整个界面`);
  }
  const n = (app.match(/<PanelBoundary/g) || []).length;
  if (n < 8) fails.push(`[静态] App.tsx 只有 ${n} 处 PanelBoundary，少于已知的 8 个挂载点`);
  if (!fails.some((f) => f.startsWith("[静态]"))) console.log(`     ✓ App.tsx ${n} 处 + SplitArea.tsx 四个面板，入口都包上了`);
}

// —— 常驻框架件：标题栏 / 侧栏 / 状态条 ——
// 🔴 2026-08-19 查出：这三个在每一页都常驻，却一个都没包边界。上面那条「面板入口」
// 的检查天然照不到它们 —— 它只认「页」，而这三件不是页。于是「爆炸半径已压到一个面板」
// 这句结论在这三件上**从来不成立**，跑道还一直报绿。
//
// 半径为什么是整屏：它们在 tab 切换之外，抛错直接冒到 main.tsx 的根 ErrorBoundary
// → 整棵树被换成全屏兜底页 = 客户说的「U-King 自己重启了」。
// 尤其侧栏渲染的是动态数据（已装小程序、dock、升级状态），一份坏清单就能白掉整个界面。
console.log(`\n[静态] 常驻框架件（标题栏/侧栏/状态条）有没有各自的边界…`);
{
  const app = readFileSync("src/App.tsx", "utf8");
  for (const [comp, name] of [["TitleBar", "titlebar"], ["Sidebar", "sidebar"], ["StatusLine", "statusline"]]) {
    const at = app.indexOf(`<${comp}`);
    if (at < 0) {
      fails.push(`[常驻] src/App.tsx 里找不到 <${comp}> —— 跑道过期了，去对一下这个文件`);
      continue;
    }
    const before = app.slice(Math.max(0, at - 700), at);
    if (!before.includes(`<PanelBoundary name="${name}"`))
      fails.push(`[常驻] <${comp}> 没包在 name="${name}" 的边界里 —— 它崩了会带走整个界面`);
    // 形态也要对：常驻件套上占满高度的大卡，等于「没白屏但也没法用」——兜底本身不能是第二种坏掉
    else if (!before.includes('variant="chrome"'))
      fails.push(`[常驻] <${comp}> 的边界没用 variant="chrome"，大卡会把它旁边的内容顶掉`);
  }
  if (!fails.some((f) => f.startsWith("[常驻]"))) console.log(`     ✓ 三件常驻框架件各有一个 chrome 形态的边界`);
}

await browser.close();
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 爆炸半径已压到一个面板：不包必崩（变异验过），包上只炸一块，重试能自愈，入口无遗漏");
