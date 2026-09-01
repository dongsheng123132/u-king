/**
 * 终端右键取证 —— 客户反馈 2026-08-10（v0.9.94）：在终端里右键弹的是 **Edge 网页菜单**
 * （返回 / 刷新 / 另存为 / 打印 / 发送标签页到你的设备），而且据报还会「同时粘贴」。
 *
 * ## 这条跑道验什么
 *  1. **浏览器默认菜单绝不出现** —— 判据是 `contextmenu` 事件的 `defaultPrevented`。
 *     这不是间接指标：`preventDefault()` 就是「浏览器别弹你的菜单」这件事本身的开关。
 *     Chromium 的原生菜单在自动化里看不见，但**这个开关看得见**，而且它坏了菜单必然回来。
 *  2. **弹的是终端自己的菜单**，且项都在（复制 / 粘贴 / 全选 / 清屏）。
 *  3. **右键单独不许粘贴** —— 剪贴板里放一段唯一标记，右键之后终端里不许出现它。
 *     这一条是客户报的第二个症状（他自己没复现出来），结构上把它钉死。
 *  4. **全局那半边的分寸**：空白处右键要拦（否则还是「发送标签页到你的设备」），
 *     但**输入框里右键必须放行** —— 我们没有给每个输入框做自己的菜单，一刀切等于
 *     把客户的复制粘贴一起干掉。
 *
 * ## 不验什么
 * 真 WebView2 里的表现（Edge 菜单长什么样、剪贴板权限给不给）。那得在真 exe 上看。
 * 这里跑的是 Chromium，验的是**我们这边的开关有没有拨对**。
 *
 * 用法：先 `pnpm dev`（本 worktree），再 `node scripts/check-term-contextmenu.mjs`
 *       换端口：`UKING_DEV_URL=http://localhost:1436/ node scripts/check-term-contextmenu.mjs`
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = "C:/Users/example/AppData/Local/Temp/uking-termmenu";
const MARK = "CLIPTEST_ZZZ_" + "918273"; // 固定值：脚本里不许用 Math.random（跑道要可重放）

/** 最小 Tauri shim。终端要能起来，但**不需要真 PTY** —— 我们验的是右键，不是 shell。 */
const SHIM = () => {
  let nextSession = 100;
  const fake = (cmd, args) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\demo", installed: true, opened_dir: null };
    if (cmd === "list_tools") return [];
    if (cmd === "list_tasks") return [];
    if (cmd === "upsert_task") return args?.task ?? null;
    if (cmd === "list_automations") return { jobs: [] };
    if (cmd === "list_ai_tasks") return { days: 7, active_window_secs: 300, tasks: [], sources: [], counts: {}, truncated: false, notes: [] };
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false };
    if (cmd === "term_open") return nextSession++;      // 假 PTY：给个 id 就够了
    if (cmd === "term_write" || cmd === "term_resize" || cmd === "term_close" || cmd === "term_ping") return null;
    if (cmd === "list_running") return [];
    if (cmd?.startsWith("plugin:event|")) return 1;
    return null;
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (c, a) => Promise.resolve(fake(c, a)),
    transformCallback: (cb) => { const id = 1e6 + Math.floor(performance.now()); window[`_${id}`] = cb; return id; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };

  /**
   * 右键一次，**返回这次事件最终有没有被拦**。
   *
   * 🔴 第一版是「挂个 window 监听记 defaultPrevented」，结果**全是假红**：
   * `addInitScript` 在文档一开始就跑，探针比 `main.tsx` 先注册，同在 window 冒泡阶段
   * 就先执行 —— 它读到的永远是「还没被拦」。派发完直接读事件对象就没有这个顺序问题。
   */
  window.__rightClick = (el, x, y) => {
    const ev = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: x, clientY: y });
    el.dispatchEvent(ev);
    return { prevented: ev.defaultPrevented, tag: el.tagName };
  };
};

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
const ctx = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  permissions: ["clipboard-read", "clipboard-write"],
});
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
page.on("console", (m) => { if (m.type() === "error") errors.push("console: " + m.text().slice(0, 200)); });
// 🔴 拦掉崩溃上报（与 check-boot-report.mjs 同款桩）：index.html 的兜底上报器会往
//    https://u-claw.org.cn/uking/bug 发 XHR，裸 Chromium 里预检被 CORS 拦 → console 冒
//    2 条 error，把这条跑道打成假红（app 本体毫发无伤 —— 上报失败本来就该静默）。
await page.route("**/uking/bug", async (route) => {
  const req = route.request();
  if (req.method() === "OPTIONS") {
    await route.fulfill({ status: 204, headers: {
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Headers": "*",
      "Access-Control-Allow-Methods": "POST, OPTIONS",
    }});
  } else {
    await route.fulfill({ status: 200, contentType: "application/json", body: '{"ok":true}' });
  }
});
await page.addInitScript(SHIM);

const problems = [];
const need = (ok, msg) => { if (!ok) problems.push(msg); };

await page.goto(URL, { waitUntil: "networkidle" }).catch(() => {});
await page.waitForSelector("aside", { timeout: 20000 }).catch(() => {});
await page.waitForTimeout(2000);

// —— 进终端。走的是客户那条路：U-Workspace → 会话顶栏「终端」开关滑出（侧栏那个整页入口
//    早就注释掉了，照它去点会一直点空，而"点空"在自动化里长得跟"功能坏了"一模一样）——
await page.getByRole("button", { name: "U-Workspace", exact: false }).first().click({ timeout: 8000 }).catch(() => {});
await page.waitForTimeout(1500);
// 🔴 必须精确匹配：侧栏那条「U-Workspace 对话 + 终端 + 作图，一站干活」也含「终端」，
//    `exact:false` + `.first()` 会点到它，然后终端永远不出现 —— 而"点空"在自动化里
//    长得跟"功能坏了"一模一样。
await page.getByRole("button", { name: "终端", exact: true }).first().click({ timeout: 8000 }).catch(() => {});
const term = page.locator(".xterm-screen").first();
const gotTerm = await term.waitFor({ timeout: 15000 }).then(() => true).catch(() => false);
need(gotTerm, "没能把终端渲染出来 —— 下面每一条结论都不作数（跑道量不到 ≠ 功能是对的）");

if (gotTerm) {
  await page.waitForTimeout(800);
  await page.evaluate((m) => navigator.clipboard.writeText(m), MARK);
  const before = await page.evaluate(() => document.querySelector(".xterm-screen")?.innerText || "");

  // ① 在终端里右键
  const clicked = await page.evaluate(() => {
    const el = document.querySelector(".xterm-screen");
    const b = el.getBoundingClientRect();
    return window.__rightClick(el, b.left + 60, b.top + 40);
  });
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${OUT}/term-rightclick.png` });

  const r = await page.evaluate(() => {
    const menu = document.querySelector("[data-term-menu]");
    return {
      menuPresent: !!menu,
      items: menu ? [...menu.querySelectorAll("button")].map((b) => b.textContent) : [],
      screen: document.querySelector(".xterm-screen")?.innerText || "",
    };
  });

  need(clicked.prevented === true,
    `终端里右键**没有**拦掉默认菜单（defaultPrevented=${clicked.prevented}）—— Edge 的「刷新 / 另存为 / 发送标签页到你的设备」会当场弹出来，这就是客户报的那个 bug`);
  need(r.menuPresent, "终端没弹出自己的右键菜单 —— 拦掉了默认菜单却不给替代品，比之前还糟");
  for (const label of ["复制", "粘贴", "全选", "清屏"]) {
    need(r.items.some((t) => (t || "").includes(label)), `终端菜单缺「${label}」，实际只有：${r.items.join(" / ") || "（空）"}`);
  }
  need(!r.screen.includes(MARK) && r.screen === before,
    "右键之后终端内容变了 / 出现了剪贴板标记 —— 右键单独绝不许触发粘贴（客户报的第二个症状）");

  // ★ 光标位置那个隐藏 textarea：xterm 把 `.xterm-helper-textarea` 放在光标上，
  //   右键正好落那儿时 `closest("textarea")` 会命中它 —— 全局那条「输入框放行」
  //   本来会把 Edge 菜单放回来。
  const onHelper = await page.evaluate(() => {
    const ta = document.querySelector(".xterm-helper-textarea");
    if (!ta) return { missing: true };
    const b = ta.getBoundingClientRect();
    return window.__rightClick(ta, b.left, b.top);
  });
  need(!onHelper.missing, "找不到 xterm 的光标 textarea —— 这条没验到");
  need(onHelper.prevented === true,
    `右键落在终端光标那个隐藏 textarea 上时没拦住（${JSON.stringify(onHelper)}）`);

  // ★ 上面那条今天是被**终端容器级监听**接住的，所以它验不到全局那条 `.xterm` 守卫。
  //   用一个**合成的** `.xterm > textarea`（不在任何真终端里）单独验全局规则本身 ——
  //   不然那两行守卫就是一段没人验的防御，而它防的正好是我们刚修的这个客户 bug。
  const synthetic = await page.evaluate(() => {
    const wrap = document.createElement("div");
    wrap.className = "xterm";
    wrap.style.cssText = "position:fixed;left:0;top:0;width:120px;height:60px;z-index:99999";
    const ta = document.createElement("textarea");
    wrap.appendChild(ta);
    document.body.appendChild(wrap);
    const r = window.__rightClick(ta, 5, 5);
    wrap.remove();
    return r;
  });
  need(synthetic.prevented === true,
    `全局规则把 .xterm 里的 textarea 当普通输入框放行了（${JSON.stringify(synthetic)}）—— 终端里的隐藏 textarea 会成为 Edge 菜单的后门`);

  // ② Esc 关掉菜单，别留在屏幕上
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  need(await page.evaluate(() => !document.querySelector("[data-term-menu]")), "Esc 关不掉终端右键菜单");
}

// —— 全局那半边：空白处要拦，输入框要放行 ——
const aside = await page.evaluate(() => {
  const el = document.querySelector("aside nav") || document.querySelector("aside");
  const b = el.getBoundingClientRect();
  return window.__rightClick(el, b.left + 20, b.top + 40);
});
need(aside?.prevented === true,
  `侧栏空白处右键没拦住（${JSON.stringify(aside)}）—— 客户会看到「发送标签页到你的设备」这种一眼就是网页的菜单`);

const inInput = await page.evaluate(() => {
  const ta = document.createElement("textarea");
  ta.id = "__probe_input";
  ta.style.cssText = "position:fixed;left:10px;top:10px;width:200px;height:60px;z-index:99999";
  document.body.appendChild(ta);
  const b = ta.getBoundingClientRect();
  const r = window.__rightClick(ta, b.left + 10, b.top + 10);
  ta.remove();
  return r;
});
need(inInput?.prevented === false,
  `输入框里右键被一起拦了（${JSON.stringify(inInput)}）—— 我们没给每个输入框做自己的菜单，拦掉等于把客户的复制粘贴干掉了`);

if (errors.length) problems.push(`运行时报错 ${errors.length} 条：${errors.slice(0, 2).join(" / ")}`);

await browser.close();
console.log("截图 ->", OUT);
if (problems.length) {
  console.error("\n❌ 不通过:\n - " + problems.join("\n - "));
  process.exit(1);
}
console.log("\n✅ 通过：终端右键不再落到浏览器菜单 / 弹的是终端自己的菜单 / 右键单独不粘贴 / 全局拦得住也放得开");
