/**
 * 「AI 做完的东西，客户拿不拿得到手」的跑道 —— 真 ProducedFile 卡片。
 *
 * ## 为什么值得单开一条
 * 客户原话：「结果中的文件…是不是也支持多种方式打开，或者右侧浏览器预览…复制也要」。
 * 这张卡片是**交付的最后一米**：AI 说「已生成 report.html」之后，人能不能看见、打开、复制。
 * 它的失效方式全是静默的：
 *  - 文件其实不存在（AI 说完就忘/写错目录）却给了一排按钮 → 点了必失败，比没有更伤；
 *  - 菜单项摆了但没接到命令 → 看着能点，什么都不发生；
 *  - 给 .png 也摆「复制内容」→ 复制出来一堆乱码；
 *  - **菜单在 DOM 里齐活，屏幕上却被祖先的 `overflow-hidden` 整块裁掉** → 客户点了「没反应」。
 * 这几种 tsc 和单测都看不见。
 *
 * ## 这条跑道自己漏过一次，漏法值得记住（2026-08-17）
 * 最后那种（裁剪）当时**三条跑道全绿而功能全废**，两个原因叠在一起：
 *  1. 探针把 `ProducedFile` 裸渲染在一个普通 div 里，**没复现真实容器** ——
 *     真界面里它长在 `ToolBubble` 内部，而那是 `overflow-hidden`。碰不到的东西测不出来。
 *  2. 断言用的是 `getByRole(...).count()`，只问「在不在 DOM 里」。**被裁掉的元素照样在
 *     DOM 里、照样有 boundingBox、`isVisible()` 也照样是 true。**
 * 所以第 8 步两样都补上：真容器 + `elementFromPoint` 验「那个坐标上真的画的是它」。
 *
 * ## 真在哪儿、假在哪儿
 *  - **真**：`ProducedFile` / `ToolBubble` 组件本体、真 CSS（含 overflow-hidden）、真 React 渲染、真点击。
 *  - **假**：`invoke`（浏览器里没有 Tauri）。`produced_file_info` 按用例返回不同形状 ——
 *    「文件在不在、多大」正是这张卡片的输入，跑道要摆布的就是它。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-produced-file-card.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";

const SHIM = () => {
  window.__calls = [];
  const B = String.fromCharCode(92);
  window.__files = {
    ["D:" + B + "probe" + B + "画好的.png"]: { exists: true, size: 51200, openable: true },
    "D:\\probe\\report.html": { exists: true, size: 2048, openable: true },
    "D:\\probe\\成品.pptx": { exists: true, size: 40960, openable: true },
    "D:\\probe\\空的.docx": { exists: true, size: 0, openable: true },
    "D:\\probe\\不存在.png": { exists: false, size: 0, openable: false },
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      window.__calls.push({ cmd, args: { ...(args || {}) } });
      if (cmd === "produced_file_info") return Promise.resolve(window.__files[args.path] ?? { exists: false, size: 0, openable: false });
      if (cmd === "read_text_file") return Promise.resolve("<h1>hi</h1>");
      if (cmd?.startsWith("plugin:event|")) return Promise.resolve(1);
      return Promise.resolve(null);
    },
    // 图片卡片要它把本地路径变成能加载的地址；浏览器里没有 Tauri，给个形状对的假的就够
    convertFileSrc: (p) => "https://asset.localhost/" + encodeURIComponent(p),
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

const PROBE_NAME = "__produced-card-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>produced card probe</title></head>
<body style="margin:0"><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { I18nProvider } from "/src/i18n";
import { ProducedFile, ToolBubble } from "/src/opencodex/panels/ChatPanel";

const NAMES = ["report.html", "成品.pptx", "空的.docx", "不存在.png", "画好的.png"];
const PIC = "D:" + String.fromCharCode(92) + "probe" + String.fromCharCode(92) + "画好的.png";
const PATHS = ["D:\\\\probe\\\\report.html", "D:\\\\probe\\\\成品.pptx", "D:\\\\probe\\\\空的.docx", "D:\\\\probe\\\\不存在.png", PIC];
const OK_TOOL = { kind: "tool", name: "Bash", input: { command: "npm run build" }, output: "l1" + String.fromCharCode(10) + "l2" + String.fromCharCode(10) + "l3", done: true, isError: false };
const BAD_TOOL = { kind: "tool", name: "Bash", input: { command: "npm run oops" }, output: "boom 出错了", done: true, isError: true };
// 🔴 产物卡片在真实界面里是长在 ToolBubble **里面**的，而 ToolBubble 是 overflow-hidden。
// 上面那几张 data-card 是裸渲染的，永远碰不到裁剪 —— 2026-08-17 那个「菜单点开看不见」
// 的 bug 就是从这个缺口漏过去的：三条跑道全绿，功能全废。这一条专门复现那个容器。
const FILE_TOOL = { kind: "tool", name: "Bash", input: { command: "node gen.mjs" }, output: "D:\\\\probe\\\\report.html", done: true, isError: false };
function Probe() {
  React.useEffect(() => { window.__ready = true; }, []);
  return React.createElement("div", { style: { width: "620px" } },
    PATHS.map((p, i) => React.createElement("div", { key: p, "data-card": NAMES[i] },
      React.createElement(ProducedFile, { path: p, onPreview: (x) => { window.__previewed = x; } }))).concat([
      React.createElement("div", { key: "ok", "data-tool": "ok" }, React.createElement(ToolBubble, { item: OK_TOOL })),
      React.createElement("div", { key: "bad", "data-tool": "bad" }, React.createElement(ToolBubble, { item: BAD_TOOL })),
      React.createElement("div", { key: "file", "data-tool": "file" }, React.createElement(ToolBubble, { item: FILE_TOOL })),
    ]));
}
createRoot(document.getElementById("root")).render(
  React.createElement(I18nProvider, null, React.createElement(Probe)));
</script></body></html>`;

/** 图片用例的真实路径（单反斜杠）——选择器另用文件名，别拿它当 CSS 选择器 */
const PIC_PATH = "D:" + String.fromCharCode(92) + "probe" + String.fromCharCode(92) + "画好的.png";
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
const page = await browser.newPage({ viewport: { width: 800, height: 700 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
await page.addInitScript(SHIM);
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 20000 }).catch(() => {
  fails.push("探针没挂起来（`pnpm dev` 起了吗？）");
});
await page.waitForTimeout(600);

/** 用文件名当选择器 —— 路径里的反斜杠在 CSS 里是转义符，直接塞进属性选择器永远选不中。 */
const card = (p) => page.locator(`[data-card="${p.split("\\").pop()}"]`);

console.log("[1/8] 不存在 / 0 字节的，一个按钮都不许给…");
{
  for (const [p, why] of [["D:\\probe\\不存在.png", "文件不存在"], ["D:\\probe\\空的.docx", "0 字节 = 没做成"]]) {
    const html = (await card(p).innerHTML()).trim();
    if (html) fails.push(`${why}却渲染了卡片 —— 点了必失败的按钮比没有更伤（${p}）`);
  }
  if (!fails.length) console.log("     ✓ 两种都没渲染");
}

console.log("[2/8] 能预览的，主按钮就是「预览」…");
{
  const btn = card("D:\\probe\\report.html").getByRole("button", { name: /预览/ });
  if ((await btn.count()) === 0) {
    fails.push("html 成品没有预览按钮");
  } else {
    await btn.first().click();
    await page.waitForTimeout(200);
    const got = await page.evaluate(() => window.__previewed);
    if (got !== "D:\\probe\\report.html") fails.push(`点了预览但回调拿到的是「${got}」`);
    else console.log("     ✓ 预览 → 回调拿到正确路径");
  }
}

console.log("[3/8] 「打开方式 / 复制」菜单：该有的都在…");
{
  const more = card("D:\\probe\\report.html").getByRole("button", { name: /打开方式/ });
  if ((await more.count()) === 0) {
    fails.push("卡片上没有「打开方式 / 复制」菜单");
  } else {
    await more.first().click();
    await page.waitForTimeout(200);
    const want = ["用默认程序打开", "在资源管理器中显示", "用 VS Code 打开", "复制路径", "复制内容"];
    for (const w of want) {
      if ((await page.getByRole("button", { name: w, exact: true }).count()) === 0) fails.push(`菜单里少了「${w}」`);
    }
    if (!fails.length) console.log("     ✓ 五项齐（含复制路径 / 复制内容）");
  }
}

console.log("[4/8] 菜单项真接到后端命令（不是摆设）…");
{
  await page.getByRole("button", { name: "在资源管理器中显示", exact: true }).first().click();
  await page.waitForTimeout(250);
  const call = await page.evaluate(() => window.__calls.find((c) => c.cmd === "reveal_produced_file"));
  if (!call) fails.push("点了「在资源管理器中显示」什么都没发生");
  else if (call.args?.path !== "D:\\probe\\report.html") fails.push(`命令带的路径不对：${JSON.stringify(call.args)}`);
  else console.log("     ✓ reveal_produced_file(path=report.html)");
}

console.log("[5/8] 二进制成品不许出现「复制内容」…");
{
  const more = card("D:\\probe\\成品.pptx").getByRole("button", { name: /打开方式/ });
  await more.first().click();
  await page.waitForTimeout(200);
  const n = await page.getByRole("button", { name: "复制内容", exact: true }).count();
  if (n > 0) fails.push("给 .pptx 也摆了「复制内容」—— 复制出来是一堆乱码");
  else console.log("     ✓ pptx 菜单里没有「复制内容」");
}

/** 关掉可能开着的菜单 —— 它的遮罩是 `fixed inset-0`，会挡住后面所有点击。
 *  认 `data-anchored-mask`（AnchoredMenu 留的稳定标识）而不是样式类：
 *  原来写的是 `.fixed.inset-0.z-40`，z 值一改跑道就默默失联、遮罩再也关不掉。 */
const closeMenus = async () => {
  const mask = page.locator("[data-anchored-mask], .fixed.inset-0.z-40");
  while ((await mask.count()) > 0) {
    await mask.first().click({ force: true });
    await page.waitForTimeout(120);
  }
};

await closeMenus();
console.log("[6/8] 图片产物：卡片里就该是那张图…");
{
  const img = page.locator('[data-card="画好的.png"]').locator("img");
  if ((await img.count()) === 0) {
    fails.push("图片产物没有内联缩略图 —— 看一眼画成什么样还得再点两下");
  } else {
    const src = await img.first().getAttribute("src");
    if (!src) fails.push("img 有了但没有 src");
    else {
      await img.first().click();
      await page.waitForTimeout(200);
      const got = await page.evaluate(() => window.__previewed);
      if (got !== PIC_PATH) fails.push("点缩略图没有放大到右侧，拿到的是 " + got);
      else console.log("     ✓ 内联出图，点图进大图");
    }
  }
}

await closeMenus();
console.log("[7/8] 工具输出：成功折起、出错摊开…");
{
  const okOut = page.locator('[data-tool="ok"] pre');
  const badOut = page.locator('[data-tool="bad"] pre');
  if ((await okOut.count()) > 0) fails.push("成功的工具也把输出摊开了 —— 这就是「一堆字」的来源");
  else console.log("     ✓ 成功的折起来了");
  if ((await badOut.count()) === 0) fails.push("出错的工具把报错折起来了 —— 那正是人要读的东西");
  else console.log("     ✓ 出错的直接摊开");
  // 断言的是**意图**（折起来必须能展开），不是某句文案：开关已从卡片下方独立一行
  // 并进头部行（少一半行数、热区反而更大），入口现在是头部那颗「N 行」+ 折角。
  const toggle = page.locator('[data-tool="ok"] button').filter({ hasText: /\d+\s*行/ });
  if ((await toggle.count()) === 0) {
    fails.push("折起来了，但头部没有展开入口（应有「N 行」）—— 等于把输出藏没了");
  } else {
    await toggle.first().click();
    await page.waitForTimeout(200);
    if ((await okOut.count()) === 0) fails.push("点了头部的展开入口也没展开");
    else console.log("     ✓ 点头部能展开");
  }
}

await closeMenus();
console.log("[8/8] 卡片长在 ToolBubble 里时，菜单不许被 overflow-hidden 裁掉…");
{
  const more = page.locator('[data-tool="file"]').getByRole("button", { name: /打开方式/ });
  if ((await more.count()) === 0) {
    fails.push("ToolBubble 里没认出产物卡片 —— 这条跑道的 fixture 失效了，先修跑道");
  } else {
    await more.first().click();
    await page.waitForTimeout(250);
    const item = page.getByRole("button", { name: "复制路径", exact: true }).first();
    const box = (await item.count()) ? await item.boundingBox() : null;
    if (!box) {
      fails.push("菜单没出来（或菜单项没有位置）");
    } else if (box.x < 0 || box.y < 0 || box.x + box.width > 800 || box.y + box.height > 700) {
      fails.push(`菜单跑到视口外了（x=${Math.round(box.x)} y=${Math.round(box.y)} w=${Math.round(box.width)} h=${Math.round(box.height)}）—— 客户看不见`);
    } else {
      /* 🔴 **不能只验「菜单项存在」**。被 overflow-hidden 裁掉的元素照样在 DOM 里、
         照样有 boundingBox、`isVisible()` 也照样返回 true —— 这三样全绿而客户屏幕上
         什么都没有。唯一算数的证据是：那个坐标上**真正画出来的**就是它。
         用 closest('button') 而不是比 textContent：被裁时 elementFromPoint 返回的是外层
         容器，而容器的 textContent **包含**菜单项文字，比字符串会假绿。 */
      const painted = await page.evaluate(
        ([x, y]) => {
          const el = document.elementFromPoint(x, y);
          const btn = el && el.closest("button");
          return !!btn && (btn.textContent || "").includes("复制路径");
        },
        [box.x + box.width / 2, box.y + box.height / 2],
      );
      if (!painted) {
        fails.push("菜单项被裁掉/遮住了 —— 那个坐标上画的不是它（ToolBubble 的 overflow-hidden 切的；absolute 定位的菜单必然中招，要用 AnchoredMenu 的 fixed）");
      } else {
        console.log("     ✓ 在 overflow-hidden 的卡片里也完整画了出来（elementFromPoint 命中）");
      }
    }
  }
}

await browser.close();
if (errors.length) console.log("（页面错误：" + errors.slice(0, 3).join(" | ") + "）");
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 成品卡片：不存在的不给按钮 · 预览接对路径 · 打开方式/复制齐全且真接命令 · 二进制不给复制内容 · 菜单在 overflow-hidden 容器里不被裁");
