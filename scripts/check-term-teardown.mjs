/**
 * 「拆终端把整个界面拆没了」这条崩溃的跑道 —— 真 useTermGroup + 真 xterm + 真 ErrorBoundary。
 *
 * ## 它盯的是哪个 bug
 * 2026-08-16 客户 + 本机同时实锤（issue #402 / #403，`~/.uking/logs/.crash-history.json` 有栈）：
 *
 *     TypeError: Cannot read properties of undefined (reading '_isDisposed')
 *         at Pk.dispose            ← WebglAddon.dispose
 *         at AddonManager._wrappedAddonDispose
 *         at AddonManager.dispose  ← 来自 term.dispose()
 *
 * `term.dispose()` 只有两个调用点，都在 `useTermGroup.ts`，且**都在 React 生命周期里**
 * （关标签 / 卸载 cleanup）。effect cleanup 抛出来的错 React 会一路冒到 ErrorBoundary →
 * 整棵树被卸掉 → 满屏「U-King 遇到问题，界面已停止」。
 * 用户那边的说法是「**用着用着 U-King 自己重启了**」，而他实际做的只是把大脑从 Hermes
 * 切到轻助手（`Chat.tsx` 里那一切 = 中间的 TermPanel 卸载 = 走到这条路）。
 *
 * ## 判据（非视觉，可自动判）
 * 让 `WebglAddon.prototype.dispose` **必抛**（复刻上面那个 TypeError），然后卸载组件：
 *  - `fixed`  档：用非 Windows UA 让真 `useTermGroup` 仍挂 WebGL，再卸载 → ErrorBoundary
 *    **不许**出现，页面**不许**有 pageerror。Windows 现在默认 DOM，但 macOS/Linux 仍走这条。
 *  - `legacy` 档：**变异验证** —— 手写一份「老代码」（裸 `term.dispose()`，没有 try/catch），
 *    同样卸载 → ErrorBoundary **必须**出现。它证明这条跑道真的会红，而不是永远绿。
 *
 * 没有 legacy 档，`fixed` 档绿了也说明不了任何事 —— 一条永远不红的断言等于没有断言。
 *
 * **验不到**：真 WebView2 里 WebGL 上下文丢失的**时机**（切显卡 / 驱动重启 / 远程桌面）。
 * 这条跑道是把「抛了会怎样」钉死，不是去复现「什么时候会抛」。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-term-teardown.mjs`（换端口用 UKING_DEV_URL=）。
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
      if (cmd === "term_open") return Promise.resolve("sid-teardown-1");
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

const PROBE_NAME = "__term-teardown-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>term teardown probe</title></head>
<body style="margin:0"><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import { Terminal as XTerm } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import "/src/globals.css";
import { useTermGroup } from "/src/opencodex/term/useTermGroup";
import { ErrorBoundary } from "/src/components/ErrorBoundary";

// —— 注入故障：把崩溃现场那一步做成**必抛** ——
// patch 的是 WebglAddon 的原型，useTermGroup 和下面的 legacy 探针 import 的是同一个模块实例
// （Vite 会 dedupe），所以两档面对的是**同一个坏 addon**，对比才成立。
const realDispose = WebglAddon.prototype.dispose;
WebglAddon.prototype.dispose = function () {
  window.__webglDisposeCalls = (window.__webglDisposeCalls || 0) + 1;
  throw new TypeError("Cannot read properties of undefined (reading '_isDisposed')");
};
window.__realWebglDispose = realDispose;

/** fixed 档：真 useTermGroup（就是我们改过的那份）。 */
function RealTerm() {
  const g = useTermGroup({ open: true, cwd: "D:\\\\probe" });
  React.useEffect(() => { window.__ready = true; }, []);
  return React.createElement("div", {
    ref: g.hostRef,
    id: "host",
    style: { position: "relative", width: "800px", height: "400px" },
  });
}

/** legacy 档：手写一份 0.9.x 的老写法 —— 裸 term.dispose()，cleanup 里不设防。 */
function LegacyTerm() {
  const ref = React.useRef(null);
  React.useEffect(() => {
    const term = new XTerm({ fontSize: 13 });
    term.open(ref.current);
    try {
      const w = new WebglAddon();
      term.loadAddon(w);
    } catch { /* 拿不到上下文就算了，那种机器本来也不会崩 */ }
    window.__ready = true;
    return () => {
      term.dispose(); // ← 老代码原样：抛出去就冒到 ErrorBoundary
    };
  }, []);
  return React.createElement("div", { ref, id: "host", style: { position: "relative", width: "800px", height: "400px" } });
}

function App() {
  const [mounted, setMounted] = React.useState(true);
  const mode = new URLSearchParams(location.search).get("mode") || "fixed";
  window.__unmount = () => setMounted(false);
  return React.createElement(
    ErrorBoundary,
    null,
    mounted ? React.createElement(mode === "legacy" ? LegacyTerm : RealTerm) : React.createElement("div", { id: "gone" }, "unmounted-ok"),
  );
}
createRoot(document.getElementById("root")).render(React.createElement(App));
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

const browser = await chromium.launch({ args: ["--use-gl=swiftshader", "--enable-unsafe-swiftshader"] });

/** 跑一档：挂上 → 等终端起来 → 卸载 → 回答「ErrorBoundary 出来了吗」。 */
async function run(mode) {
  const page = await browser.newPage({
    viewport: { width: 1100, height: 700 },
    // 这条回归盯的是「仍启用 WebGL 的非 Windows 平台」的卸载防崩。若沿用本机 Windows UA，
    // 新策略会正确回退 DOM，测试却因根本没创建 addon 而空转。
    userAgent:
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
  });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e).slice(0, 160)));
  await page.addInitScript(SHIM);
  await page.goto(`${URL}${PROBE_NAME}?mode=${mode}`, { waitUntil: "networkidle" });
  const up = await page
    .waitForFunction(() => window.__ready === true, null, { timeout: 20000 })
    .then(() => true)
    .catch(() => false);
  if (!up) {
    await page.close();
    return { up: false };
  }
  await page.waitForTimeout(700); // newTerm 里有 50ms setTimeout + fit
  await page.evaluate(() => window.__unmount());
  await page.waitForTimeout(500);
  // ErrorBoundary 的兜底界面是内联样式写死的中英双语标题，认它最稳（不依赖 Tailwind/i18n）
  const boom = await page.evaluate(() => document.body.innerText.includes("界面已停止"));
  const gone = (await page.locator("#gone").count()) > 0;
  const disposeCalls = await page.evaluate(() => window.__webglDisposeCalls || 0);
  await page.close();
  return { up: true, boom, gone, disposeCalls, errors };
}

console.log("[1/2] 变异验证：老写法（裸 term.dispose()）必须把界面打崩 —— 证明这条跑道会红…");
{
  const r = await run("legacy");
  if (!r.up) fails.push("legacy 探针没挂起来（`pnpm dev` 起了吗？）");
  else if (!r.boom)
    fails.push(
      "老写法卸载后 ErrorBoundary 没出现 —— 说明故障没注进去（WebglAddon 可能压根没挂上），" +
        "那么下面那档绿了也不能证明任何事",
    );
  else console.log(`     ✓ 老写法如期崩了（webgl.dispose 被调用 ${r.disposeCalls} 次，界面被 ErrorBoundary 接管）`);
}

console.log("[2/2] 真 useTermGroup 卸载：界面必须活着…");
{
  const r = await run("fixed");
  if (!r.up) fails.push("fixed 探针没挂起来");
  else {
    if (r.boom) fails.push("卸载真终端后界面被 ErrorBoundary 接管了 —— 客户看到的就是「U-King 自己重启了」");
    if (!r.gone) fails.push("卸载后没渲染出后续内容（组件树没能正常往下走）");
    if (r.disposeCalls === 0)
      fails.push("一次都没走到 webgl.dispose —— 这次卸载根本没碰到出事那条路，本档等于空转");
    if (r.errors?.length) fails.push(`卸载时有未捕获页面错误：${r.errors.slice(0, 2).join(" | ")}`);
    if (!fails.length) console.log(`     ✓ 卸载干净（webgl.dispose 抛了 ${r.disposeCalls} 次，全被吃掉，界面照常）`);
  }
}

await browser.close();
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 拆终端不再拆界面：故障注得进去（老写法会崩），改过的这份吃得下（新写法不崩）");
