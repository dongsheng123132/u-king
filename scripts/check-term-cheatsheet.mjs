/**
 * 「英文 TUI 的中文小抄」跑道 —— 真 TermPanel，真敲命令。
 *
 * ## 为什么值得单开一条
 * 客户原话：「很多人对 Claude Code 的英文提示不懂」。小抄这东西**错了比没有更糟**：
 *  - 该出的时候不出 → 白做；
 *  - **不该出的时候乱出** → 他在跑 `git log`，底下挂一条「按 1 同意 2 拒绝」，直接教错人。
 * 两个方向都只有「真跑一条命令再看那一行在不在」能证，tsc 和单测都看不见。
 *
 * ## 真在哪儿、假在哪儿
 *  - **真**：`TermPanel` + `useTermGroup` 本体、真 xterm、真 React 渲染、真 `runCmd`。
 *  - **假**：`invoke`（浏览器里没有 Tauri）。PTY 是假的 —— 但小抄的判据是**用户敲了什么**，
 *    不是屏幕上出现了什么，所以假 PTY 不影响这条跑道要证的事。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-term-cheatsheet.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";

const SHIM = () => {
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      window.__calls.push({ cmd, args: { ...(args || {}) } });
      if (cmd === "term_pty_info") return Promise.resolve({ backend: "conpty", buildNumber: 22631 });
      if (cmd === "term_open") return Promise.resolve("sid-cheat-1");
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

const PROBE_NAME = "__term-cheat-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>term cheatsheet probe</title></head>
<body style="margin:0"><div id="root" style="height:520px"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { I18nProvider } from "/src/i18n";
import { TermPanel } from "/src/opencodex/panels/TermPanel";

function Probe() {
  return React.createElement("div", { style: { height: "520px" } },
    React.createElement(TermPanel, {
      cwd: "D:\\\\probe",
      active: true,
      onReady: (api) => { window.__api = api; window.__ready = true; },
      onToast: (m) => { window.__toast = m; },
    }));
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

const strip = page.locator("text=中文小抄");

console.log("[1/4] 一开始不该有小抄（还没跑任何 TUI）…");
if ((await strip.count()) > 0) fails.push("什么都没跑就挂了一条小抄 —— 白占一行");
else console.log("     ✓ 没有");

console.log("[2/4] 跑 claude → 该出，且是 claude 那份…");
{
  await page.evaluate(() => window.__api.runCmd("claude"));
  await page.waitForTimeout(500);
  if ((await strip.count()) === 0) {
    fails.push("跑了 claude 却没有中文小抄 —— 客户还是只能看英文");
  } else {
    const line = await page.locator("text=中文小抄").locator("..").innerText();
    if (!line.includes("按数字选")) fails.push(`小抄出了，但不是 claude 那份：${line.replace(/\\s+/g, " ").slice(0, 60)}`);
    else console.log("     ✓ " + line.replace(/\s+/g, " ").slice(0, 56) + "…");
  }
}

console.log("[3/4] 「让 AI 说中文」真的落到那条动作上…");
{
  const btn = page.getByRole("button", { name: "让 AI 说中文" });
  if ((await btn.count()) === 0) {
    fails.push("小抄上没有「让 AI 说中文」按钮");
  } else {
    await btn.click();
    await page.waitForTimeout(300);
    const call = await page.evaluate(() => window.__calls.find((c) => c.cmd === "link_identity"));
    if (!call) fails.push("点了「让 AI 说中文」什么都没发生（按钮没接上动作）");
    else if (call.args?.linked !== true || !(call.args?.targets || []).includes("claude"))
      fails.push(`调了 link_identity 但参数不对：${JSON.stringify(call.args)}`);
    else console.log("     ✓ link_identity linked=true targets=[claude]");
  }
}

console.log("[4/4] 再跑一条普通命令 → 小抄必须撤掉…");
{
  await page.evaluate(() => window.__api.runCmd("git log --oneline"));
  await page.waitForTimeout(500);
  if ((await strip.count()) > 0) {
    fails.push("跑 git 的时候还挂着「按 1 同意 2 拒绝」—— 这是在教错人");
  } else {
    console.log("     ✓ 撤掉了");
  }
}

await browser.close();
if (errors.length) console.log("（页面错误：" + errors.slice(0, 3).join(" | ") + "）");
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 中文小抄：该出时出、出的是对的那份、按钮真接到动作、不该出时撤掉");
