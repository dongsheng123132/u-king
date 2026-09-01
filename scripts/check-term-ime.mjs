/**
 * 「打中文时候选条贴不贴着光标」的跑道。
 *
 * ## 为什么必须单开一条
 * 候选条**不由我们画** —— 操作系统把它贴在聚焦元素的插入符处，而 xterm.js 的插入符是那个
 * `opacity:0` 的隐藏 `<textarea>`。所以「候选条在哪」这件事，在 DOM 里就是「textarea 在哪」，
 * 是个**几何量**，量得到、毫秒级、零假绿。
 *
 * 现有跑道结构性照不到：`action conformance` 是 Rust 动作层；`check-webkit-compat` 是静态扫源码；
 * `check-term-file-links` 虽然也起 Chromium，但它断言的是链接解析，从不看坐标。
 * 而这条链路坏掉是**看不出来的** —— 界面一切正常，只有打中文的人觉得别扭，
 * 且开发机上不装中文输入法就永远撞不上（跟 Mac 白屏同一类盲区）。
 *
 * ## 判据
 * 往回滚 N 行后开始组字，textarea 必须落在**看得见的**光标行上（`ybase+y-ydisp`），
 * 而不是 xterm 自己算的 `buffer.y`。
 *
 * ## 这条跑道自己要能红
 * 每个用例都跑两遍：不打补丁必须红、打了补丁必须绿。
 * 只验「打了补丁是绿的」证明不了补丁有用 —— 一条永远绿的跑道等于没有跑道。
 */
import { chromium } from "playwright";
// 走 vite 的公开 API 而不是直接 import esbuild：esbuild 是 vite 的传递依赖，
// 在 pnpm 的严格 node_modules 里从脚本这儿解析不到（第一版就是这么炸的）。
import { transformWithEsbuild } from "vite";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const XTERM_JS = read("node_modules/@xterm/xterm/lib/xterm.js");
const XTERM_CSS = read("node_modules/@xterm/xterm/css/xterm.css");
const FIT_JS = read("node_modules/@xterm/addon-fit/lib/addon-fit.js");
const WEBGL_JS = read("node_modules/@xterm/addon-webgl/lib/addon-webgl.js");

// 吃**真的**源码，不是这里再抄一份实现 —— 抄一份的话补丁被删了跑道照样绿
const ANCHOR_JS = (
  await transformWithEsbuild(read("src/opencodex/term/imeAnchor.ts"), "imeAnchor.ts", {
    loader: "ts",
    format: "iife",
    globalName: "ImeAnchor",
  })
).code;

const ROWS = 24;
const CURSOR_ROW = 18;
/** 允许半格误差：行高有小数，别让四舍五入制造假红 */
const TOLERANCE_ROWS = 0.6;

async function run(page, { scrollUp, patched }) {
  return page.evaluate(
    async ([rows, cursorRow, scrollUp, patched]) => {
      document.body.innerHTML = '<div style="padding:140px 0 0 220px"><div id="t" style="width:820px;height:420px"></div></div>';
      // 与 useTermGroup.ts::new XTerm({...}) 逐字段对齐
      const term = new window.Terminal({
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        fontSize: 13,
        lineHeight: 1.2,
        cursorBlink: true,
        scrollback: 5000,
        rows,
        cols: 80,
      });
      const fit = new window.FitAddon.FitAddon();
      term.loadAddon(fit);
      term.open(document.getElementById("t"));
      try {
        term.loadAddon(new window.WebglAddon.WebglAddon());
      } catch {
        /* 无 GPU 上下文就走 DOM 渲染 —— 与产品里同款兜底 */
      }
      if (patched) window.ImeAnchor.anchorImeToCursor(term);
      fit.fit();
      await new Promise((r) => setTimeout(r, 250));

      // 灌到有回滚（ybase>0）—— Claude Code 跑一会儿就是这个状态
      for (let i = 0; i < rows * 4; i++) term.write(`line ${i} ................\r\n`);
      term.write(`\x1b[${cursorRow + 1};11H`);
      await new Promise((r) => setTimeout(r, 250));

      // 往上翻着看输出
      if (scrollUp) term.scrollLines(-scrollUp);
      await new Promise((r) => setTimeout(r, 120));

      // 开始打中文。组字**不产生数据**，所以 xterm 的 scrollOnUserInput 在这里不会触发 ——
      // 这正是缺口所在，用真事件走真监听。
      const ta = document.querySelector(".xterm-helper-textarea");
      ta.focus();
      ta.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      ta.dispatchEvent(new CompositionEvent("compositionupdate", { data: "jiasu", bubbles: true }));
      await new Promise((r) => setTimeout(r, 250));

      const b = term.buffer.active;
      const scR = document.querySelector(".xterm-screen").getBoundingClientRect();
      const cellH = scR.height / term.rows;
      const visualRow = b.baseY + b.cursorY - b.viewportY;
      const taTop = ta.getBoundingClientRect().top;
      return {
        visualRow,
        cellH: +cellH.toFixed(2),
        offRows: +((taTop - (scR.top + visualRow * cellH)) / cellH).toFixed(2),
      };
    },
    [ROWS, CURSOR_ROW, scrollUp, patched],
  );
}

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 2, viewport: { width: 1100, height: 700 } });
await page.addStyleTag({ content: XTERM_CSS });
await page.addScriptTag({ content: XTERM_JS });
await page.addScriptTag({ content: FIT_JS });
await page.addScriptTag({ content: WEBGL_JS });
await page.addScriptTag({ content: ANCHOR_JS });

let failed = 0;
console.log("判据：组字时隐藏 textarea 必须落在看得见的光标行上（候选条就贴着它）\n");
for (const scrollUp of [0, 5, 12]) {
  const bare = await run(page, { scrollUp, patched: false });
  const fixed = await run(page, { scrollUp, patched: true });
  const ok = Math.abs(fixed.offRows) <= TOLERANCE_ROWS;
  // 回滚 0 时本来就不会飘，那一格不要求「不打补丁必须红」
  const provesItself = scrollUp === 0 || Math.abs(bare.offRows) > TOLERANCE_ROWS;
  if (!ok || !provesItself) failed++;
  console.log(
    `回滚 ${String(scrollUp).padEnd(2)} 行 → 不打补丁偏 ${String(bare.offRows).padStart(6)} 行` +
      ` · 打了补丁偏 ${String(fixed.offRows).padStart(6)} 行  ` +
      (ok ? "✓" : "🔴 补丁没兜住") +
      (provesItself ? "" : "  🔴 这一格不打补丁也不红 —— 跑道证明不了自己"),
  );
}
await browser.close();

if (failed) {
  console.error(`\n❌ ${failed} 组不合格`);
  process.exit(1); // 打完错误还 exit(0) 报绿，我们犯过
}
console.log("\n✅ 候选条钉在光标上；且去掉补丁这条跑道会红");
