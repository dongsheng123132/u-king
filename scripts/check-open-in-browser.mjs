/**
 * 「AI 做的网页，能不能在真浏览器里打开」的跑道 —— 真 BrowserPanel。
 *
 * ## 为什么值得单开一条
 * iframe 预览看得见，但点链接不跳、登录态没有、部分脚本被 sandbox 拦。内置浏览器是真 Chrome
 * （CDP 驱动），这三件事它都行 —— 所以「用浏览器打开」这条路本身是产品能力的一部分。
 * 它的失效方式全是静默的，而且**已经踩到过一个**：
 *  - `open()` 里那句 `if (!/^https?:\/\//) u = "http://" + u` 会把 `file:///D:/x.html`
 *    拼成 `http://file:///D:/x.html` —— 打开一个不存在的网站，界面上什么错都不报。
 *  - 地址没变却反复导航 → 正在填的表单被冲掉。
 * 两条都只有「真挂上组件、真传一个 file:// 进去、看它最后调了什么」能证。
 *
 * ## 真在哪儿、假在哪儿
 *  - **真**：`BrowserPanel` 本体、真 React 渲染、真 effect。
 *  - **假**：`invoke`（浏览器里没有 Tauri）。`action_parity_call` 只记账不真开 Chrome ——
 *    这条跑道要证的是**发出去的 url 对不对**，不是 Chrome 能不能渲染。
 *  - **验不到**：真 Chrome 打开本地文件后页面长什么样（那要真机眼睛）。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-open-in-browser.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";

const SHIM = () => {
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      window.__calls.push({ cmd, args: JSON.parse(JSON.stringify(args || {})) });
      if (cmd === "action_parity_call") return Promise.resolve({ ok: true, result: { ok: true, title: "probe", url: args?.request?.input?.url } });
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

const FILE_URL = "file:///D:/probe/%E6%88%90%E5%93%81/report.html";
const PROBE_NAME = "__open-in-browser-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>open in browser probe</title></head>
<body style="margin:0"><div id="root" style="height:520px"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { I18nProvider } from "/src/i18n";
import { BrowserPanel } from "/src/opencodex/panels/BrowserPanel";

function Probe() {
  const [u, setU] = React.useState("");
  React.useEffect(() => { window.__go = (x) => setU(x); window.__ready = true; }, []);
  return React.createElement("div", { style: { height: "520px" } },
    React.createElement(BrowserPanel, { taskId: "probe", openUrl: u }));
}
createRoot(document.getElementById("root")).render(
  React.createElement(I18nProvider, null, React.createElement(Probe)));
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
const page = await browser.newPage({ viewport: { width: 1000, height: 620 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
await page.addInitScript(SHIM);
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 20000 }).catch(() => {
  fails.push("探针没挂起来（`pnpm dev` 起了吗？）");
});
await page.waitForTimeout(500);

/** 取所有发给 browser.open 的 url（按顺序）。 */
const opens = () =>
  page.evaluate(() =>
    window.__calls
      .filter((c) => c.cmd === "action_parity_call" && c.args?.request?.action_id === "browser.open")
      .map((c) => c.args.request.input.url),
  );

console.log("[1/3] 没人让它开时，不许自己乱开…");
{
  const n = (await opens()).length;
  if (n > 0) fails.push(`还没给地址就开了 ${n} 次`);
  else console.log("     ✓ 0 次");
}

console.log("[2/3] 给一个 file:// → 必须原样开，不许被拼成 http://file://…");
{
  await page.evaluate((u) => window.__go(u), FILE_URL);
  await page.waitForTimeout(600);
  const list = await opens();
  if (!list.length) {
    fails.push("传了 openUrl 却没导航 —— 「用浏览器打开」点了没反应");
  } else if (list[0] !== FILE_URL) {
    fails.push(`开的是「${list[0]}」，应为「${FILE_URL}」` + (list[0].startsWith("http://file") ? "（被当成域名拼了 http://）" : ""));
  } else {
    console.log("     ✓ " + list[0]);
  }
}

console.log("[3/3] 同一个地址不许反复导航（会冲掉正在填的表单）…");
{
  const before = (await opens()).length;
  await page.evaluate((u) => window.__go(u), FILE_URL); // 同值：React 不该再触发
  await page.waitForTimeout(500);
  const after = (await opens()).length;
  if (after > before) fails.push(`同一个地址又开了 ${after - before} 次`);
  else console.log("     ✓ 没有重复导航");
}

await browser.close();
if (errors.length) console.log("（页面错误：" + errors.slice(0, 3).join(" | ") + "）");
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 用浏览器打开：file:// 原样送到 browser.open · 不自己乱开 · 同址不重复导航");
