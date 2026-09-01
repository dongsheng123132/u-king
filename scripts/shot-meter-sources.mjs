/**
 * Token 水电表「数据来源」取证 —— 拿**真后端的真数据**喂给**真组件**，在真 Chromium 里点一遍。
 *
 * 为什么单开这条：新接了 OpenClaw / Hermes / pi 三路数据源，还把只读的「覆盖面」段落
 * 改成了**可勾选**的面板（勾工具、标包月）。数字对不对已经由 `usage_local` 的扫描器
 * 和独立重算逐字节比过了，但「勾了之后数字变没变、算不到的工具给没给假开关」这半边
 * **一个字节都不在动作表里** —— 不点一遍就只有编译级证据。
 *
 * 验的是什么、不验什么，说清楚：
 *  - ✅ **验**：影核动作 `runtime.usage_meter.inspect` 的真实返回经 Meter 真组件渲染后 ——
 *    本机探测到的工具是不是**全列**出来了、算不到的是不是灰掉且给了理由、
 *    算得到的有没有真勾选框、勾「包月」那一栏在没启用时不该出现、以及有没有 React 报错。
 *  - ❌ **不验**：勾选后真的落盘改数字（那条已由真 exe + 真 prefs 文件端到端验过，
 *    这里的 invoke 是 shim，写不进真文件）；也不验 Tauri webview 里的字体/DPI。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/shot-meter-sources.mjs`
 */
import { chromium } from "playwright";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, unlinkSync } from "node:fs";
import path from "node:path";

const URL = "http://localhost:1430/";
const OUT = path.join(process.env.TEMP || "/tmp", "uking-meter");
const EXE = "src-tauri/target/debug/u-king-mini.exe";

console.log("[1/4] 跑 runtime.usage_meter.inspect 拿真数据…");
const meter = JSON.parse(
  execFileSync(EXE, ["action", "run", "runtime.usage_meter.inspect", "--input", '{"days":30}'], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    // 🔴 开发机 shell 里可能残留 HERMES_HOME（指向对比实验目录），带着它跑会读到一份空库，
    // 然后这条跑道会「证明」Hermes 没有用量 —— 判别家工具的家目录必须走它自己的解析顺序。
    env: { ...process.env, HERMES_HOME: "" },
  }),
);
console.log(`      工具清单 ${meter.sources.length} 项，其中算得到 ${meter.sources.filter((s) => s.countable).length} 项`);

const SHIM = (data) => {
  const fake = (cmd) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\me", installed: true, opened_dir: null };
    if (cmd === "list_tools") return [];
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false, balance: null };
    if (cmd === "query_usage_meter") return data.meter;
    if (cmd === "set_usage_sources") return null;
    // 🔴 左栏（SessionList）在首屏就会渲染，这几条**必须给出正确形状** —— 给 null 会让
    // 整个 App 掉进错误边界，然后这条跑道报「找不到水电表入口」，看着像我的页面坏了。
    // 跑道骗人的方式之一就是：崩在别处，怪到你头上。
    // 🔴 这几条给的对象**必须带 `id`**。SessionList 里那句 `renaming?.id === t.id`，
    // 在 renaming 为 null（没在改名）且任务缺 id 时会算成 `undefined === undefined` = true，
    // 于是去读 `renaming.text` 当场崩，整个 App 掉进错误边界。
    // 真实数据里 id 总是有的，所以这是跑道的坑不是产品的坑 —— 但它值得记一笔：
    // 那句条件判断本身对「缺 id 的任务」是不设防的。
    if (cmd === "list_tasks") return [];
    if (cmd === "list_automations") return [{ id: "auto-stub", name: "stub", enabled: false }];
    if (cmd === "list_ai_tasks") return { days: 7, tasks: [], sources: [], counts: {}, notes: [] };
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

/**
 * **只挂 `<Meter/>` 这一个组件**，不启动整个 App。
 *
 * 为什么不走首页点进去（shot-taskboard 那种）：整个 App 首屏会渲染 `SessionList`，
 * 而它有一句 `renaming?.id === t.id` —— 没在改名时 `renaming` 是 null，若某个任务缺 `id`，
 * 这句就成了 `undefined === undefined` = true，接着读 `renaming.text` 当场崩，
 * 整个 App 掉进错误边界。真实数据里 id 总是有的，所以那是 stub 数据才触发的坑；
 * 但为了验我这一页而去猜别人组件要什么形状的假数据，本身就是在给跑道加不可信的部分。
 * 孤立挂载只依赖 Meter 自己的契约（`query_usage_meter` 的返回），验的正是它。
 */
// 🔴 探针页必须**真写进项目目录**让 vite 自己去服务 —— 用 Playwright 的 route 拦截伪造
// 一个 HTML 是不行的：那样 vite 从没见过这个页面，裸模块名（react / react-dom/client）
// 不会被改写成 dev 路径，浏览器直接 SyntaxError。跑完在 finally 里删掉，不留垃圾。
const PROBE_NAME = "__meter-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>meter probe</title></head>
<body><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { I18nProvider } from "/src/i18n";
import { Meter } from "/src/Meter";
createRoot(document.getElementById("root")).render(
  React.createElement(I18nProvider, null, React.createElement(Meter, { onToast: (m) => console.log("[toast]", m) }))
);
</script></body></html>`;

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
// React 的 key 撞车 / setState 警告只走 console.error，不是 pageerror —— 两条都得收。
page.on("console", (m) => { if (m.type() === "error") errors.push("console: " + m.text().slice(0, 200)); });

await page.addInitScript(SHIM, { meter });

console.log("[2/4] 挂载 Meter 组件…");
writeFileSync(PROBE_NAME, PROBE_HTML);
process.on("exit", () => { try { unlinkSync(PROBE_NAME); } catch {} });
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForTimeout(2500);
const body = await page.evaluate(() => document.body.innerText);
if (!body.includes("水电表")) {
  console.error("❌ Meter 组件没挂起来，页面正文：" + body.replace(/\s+/g, " ").slice(0, 300));
  await browser.close();
  process.exit(1);
}

console.log("[3/4] 展开「数据来源」…");
const toggle = page.getByRole("button", { name: /数据来源/ }).first();
if (!(await toggle.count())) { console.error("❌ 找不到「数据来源」折叠条"); await browser.close(); process.exit(1); }
await toggle.click();
await page.waitForTimeout(600);

const seen = await page.evaluate(() => {
  const out = [];
  for (const el of document.querySelectorAll("section div.rounded-lg.border")) {
    const txt = el.innerText || "";
    if (!txt.trim()) continue;
    const boxes = el.querySelectorAll("button, span.rounded");
    out.push({
      text: txt.replace(/\s+/g, " ").slice(0, 120),
      buttons: el.querySelectorAll("button").length,
      hasBoxes: boxes.length > 0,
    });
  }
  return out;
});

console.log("[4/4] 断言…");
let bad = 0;
const fail = (m) => { console.error("❌ " + m); bad++; };

// ① 本机探测到的工具要**全列**出来（少列一个，总数就在骗人）
const shown = seen.filter((s) => meter.sources.some((src) => s.text.includes(src.label)));
if (shown.length < meter.sources.length) {
  fail(`工具清单没列全：后端给了 ${meter.sources.length} 项，界面上只找到 ${shown.length} 项`);
} else {
  console.log(`   ✓ ${meter.sources.length} 个工具全部列出来了`);
}

// ② 算不到的工具必须给出**理由**，且不能给一个点了没用的勾
for (const src of meter.sources.filter((s) => !s.countable)) {
  const row = seen.find((s) => s.text.includes(src.label));
  if (!row) { fail(`${src.label}：算不到的工具没列出来`); continue; }
  if (!row.text.includes("读不到")) fail(`${src.label}：没标「读不到」`);
  if (row.buttons > 0) fail(`${src.label}：根本算不到，却给了 ${row.buttons} 个可点的开关（假开关）`);
}
if (!bad) console.log("   ✓ 算不到的工具都灰掉了、给了理由、没有假开关");

// ③ 算得到且已启用的工具要有勾选框 + 「包月」那一栏
for (const src of meter.sources.filter((s) => s.countable && s.enabled)) {
  const row = seen.find((s) => s.text.includes(src.label));
  if (!row) { fail(`${src.label}：算得到却没列出来`); continue; }
  if (row.buttons < 2) fail(`${src.label}：应有「算不算它」+「是不是包月」两个开关，实际 ${row.buttons} 个`);
}

// ④ 一条 React 报错都不许有
if (errors.length) { fail("页面有报错：\n   " + errors.slice(0, 5).join("\n   ")); }
else console.log("   ✓ 无 React / 运行时报错");

await page.screenshot({ path: path.join(OUT, "meter-sources.png"), fullPage: true });
console.log(`\n截图：${path.join(OUT, "meter-sources.png")}`);
await browser.close();
if (bad) { console.error(`\n❌ ${bad} 条断言没过`); process.exit(1); }
console.log("\n✅ 全过");
