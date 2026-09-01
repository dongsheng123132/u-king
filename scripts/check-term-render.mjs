/**
 * 「终端跳来跳去 / 打字时字一闪一闪」这条老毛病的跑道 —— 真 useTermGroup + 真 xterm。
 *
 * ## 这条跑道证的是什么（说清楚边界）
 * 「闪不闪」最终要人眼看，但**它的两个已知成因是可测的**，这条跑道就钉这两个：
 *
 *  1. **没变也发 resize**：ConPTY 一收到 resize 就让对面 TUI 整屏重画。老代码 fit 完
 *     **无条件**发 `term_resize` —— 拖面板分隔条、切标签、开合右侧栏，每一帧都白白让
 *     Claude Code 重画一次。这条断言：容器尺寸变了但**行列数没变**时，一次 resize 都不许发。
 *  2. **渲染器**：真实 Windows WebView2 120 已截到 WebGL canvas 残字，且拖窗口重合成后变化。
 *     这条断言 Windows 默认不用 WebGL；正确显示优先于更快的重画。
 *
 * **验不到**：客户那台机器上「还闪不闪」。GPU、驱动、WebView2 版本都不一样，
 * 而且「闪」没有非视觉判据。所以这条跑道绿 ≠ 症状消失 —— 它只保证这两个成因已被拿掉。
 *
 * ## 真在哪儿、假在哪儿
 *  - **真**：`useTermGroup` 本体、真 xterm、真 ResizeObserver、Windows UA 分支。
 *  - **假**：`invoke` 这一层（浏览器里没有 Tauri）。term_open 返回假 id，
 *    resize/write 只记账 —— 记的正是这条跑道要数的东西。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-term-render.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";

const SHIM = () => {
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      window.__calls.push({ cmd, args: { ...(args || {}) }, t: Math.round(performance.now()) });
      if (cmd === "term_pty_info") return Promise.resolve({ backend: "conpty", buildNumber: 22631 });
      // 故意慢建连：下面会在 sid 返回前真敲三个键，老 `if (s.sessionId)` 写法会当场吞掉。
      if (cmd === "term_open") return new Promise((resolve) => setTimeout(() => resolve("sid-probe-1"), 500));
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

const PROBE_NAME = "__term-render-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>term render probe</title></head>
<body style="margin:0"><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { useTermGroup, isWebglRenderer } from "/src/opencodex/term/useTermGroup";

function Probe() {
  const g = useTermGroup({ open: true, cwd: "D:\\\\probe" });
  React.useEffect(() => {
    window.__webgl = isWebglRenderer;
    window.__ready = true;
  }, [g]);
  return React.createElement("div", {
    ref: g.hostRef,
    id: "host",
    style: { position: "relative", width: "800px", height: "400px" },
  });
}
createRoot(document.getElementById("root")).render(React.createElement(Probe));
window.__setHost = (w, h) => {
  const el = document.getElementById("host");
  el.style.width = w + "px";
  el.style.height = h + "px";
};
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

// 明确模拟 Windows WebView2 UA；旧跑道跑在 Linux UA 上，根本没走客户的真实平台分支。
const browser = await chromium.launch({ args: ["--use-gl=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({
  viewport: { width: 1100, height: 700 },
  userAgent:
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0",
});
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
await page.addInitScript(SHIM);
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 20000 }).catch(() => {
  fails.push("探针没挂起来（`pnpm dev` 起了吗？）");
});
// 等首个终端建出来（newTerm 里有 50ms 的 setTimeout）
await page
  .waitForFunction(() => (window.__calls || []).some((c) => c.cmd === "term_open"), null, { timeout: 15000 })
  .catch(() => fails.push("终端没建起来"));
console.log("[1/4] Windows 默认必须避开 WebGL canvas 残字路径…");
{
  const on = await page.evaluate(() => window.__webgl());
  if (on) fails.push("Windows 仍挂上了 WebGL —— 老 WebView2 上会出现 canvas 残字/拖窗口后变化");
  else console.log("     ✓ Windows 使用 DOM 渲染器（WebGL 已关闭）");
}

console.log("[2/4] PTY 尚未建好时真敲键 → 建连后必须完整送达…");
{
  await page.locator(".xterm-helper-textarea").focus();
  await page.keyboard.type("abc");
  const sent = await page
    .waitForFunction(
      () => (window.__calls || []).filter((c) => c.cmd === "term_write").map((c) => c.args.data).join(""),
      null,
      { timeout: 3000 },
    )
    .then((h) => h.jsonValue())
    .catch(() => "");
  if (sent !== "abc") fails.push(`建连前敲入 abc，建连后实收 ${JSON.stringify(sent)} —— 仍在吞启动输入`);
  else console.log("     ✓ 建连前敲入 abc，建连后完整实收 abc");
}

console.log("[3/4] 反复抖容器 → 不许出现「和上次一模一样」的 resize…");
{
  // 判据故意选「重复」而不是「次数」：容器抖 10px 有可能真的跨过一格边界，那次 resize 是**该发的**。
  // 真正的病是**同样的行列数一遍遍发** —— 每一次都让对面 TUI 整屏重画，而屏幕上什么都没变。
  for (let i = 1; i <= 10; i++) {
    await page.evaluate((n) => window.__setHost(800 + (n % 2 ? 2 : 0), 400 + (n % 2 ? 2 : 0)), i);
    await page.waitForTimeout(110);
  }
  const calls = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "term_resize").map((c) => `${c.args.cols}x${c.args.rows}`));
  const dup = calls.filter((v, i) => i > 0 && v === calls[i - 1]);
  if (dup.length) {
    fails.push(`发了 ${dup.length} 次和上一次一模一样的 resize（${dup.join(",")}）—— 屏幕没变，对面却整屏重画了 ${dup.length} 遍`);
  } else {
    console.log(`     ✓ 10 次抖动，0 次重复 resize（共发 ${calls.length} 次，全是真变化：${calls.join(" → ")}）`);
  }
}

console.log("[4/4] 真变大 → 必须发（别为了不闪把 resize 掐死）…");
{
  const before = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "term_resize").length);
  await page.evaluate(() => window.__setHost(1000, 620));
  await page.waitForTimeout(400);
  const calls = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "term_resize"));
  const added = calls.length - before;
  if (added < 1) fails.push("容器真的变大了却没通知后端 —— 对面 TUI 会按老尺寸排版（换行全乱）");
  else if (added > 1) fails.push(`一次变大发了 ${added} 次 resize（rAF 合并失效了）`);
  else console.log(`     ✓ 恰好 1 次：${calls.at(-1).args.cols}x${calls.at(-1).args.rows}`);
}

await browser.close();
if (errors.length) console.log("（页面错误：" + errors.slice(0, 3).join(" | ") + "）");
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 渲染与 resize：Windows 避开 WebGL 残字、无谓 resize 已掐掉、真变化照发");
