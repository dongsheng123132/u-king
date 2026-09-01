/**
 * 「首屏起不来时，兜底页真的把 bug 发出去了吗」的跑道 —— 真浏览器 + 真 XHR，但**不碰线上接口**。
 *
 * ## 为什么值得单开一条
 * 2026-08-16：两条正则 lookbehind 把 0.9.99/0.9.100 的 Mac 版整个打白屏，
 * 而 bug 库里**一条记录都没有** —— 因为「首屏起不来」恰好是唯一采集不到的崩溃类型：
 * report.rs 管的是安装/AI/panic，ErrorBoundary 要 React 先挂上，兜底页当时只显示不上报。
 *
 * 补上上报之后，这条链路有个要命的性质：**它坏了是静默的**。
 * 版本号没替换（`x-uking-version: %APP_VERSION%`）→ 服务端 400 → 我们什么都看不到，
 * 界面照样显示得好好的。这正是「功能看着有，只是键错」那一类（自动去重曾这样空转过）。
 * 所以判据必须落在**真发出去的那个请求**上，不是「代码里写了」。
 *
 * ## 怎么做的
 * 起一个本地 http server 顶替 dist：`/` 发**真的 dist/index.html**（只把主模块换成会抛错的 boom.js），
 * 于是 #root 一直是空的 → 10s 看门狗触发 → 兜底页出现并发上报。
 * 上报用 Playwright 的 route 拦下来，**不放行到 u-claw.org.cn**，所以不会真的建 Issue。
 *
 * ## 真在哪儿、假在哪儿
 *  - **真**：dist/index.html 本体（构建产物，含版本号替换）、真 XMLHttpRequest、真 10s 看门狗。
 *  - **假**：服务端 —— 契约靠断言比对 `website/api/bug.js` 的入口校验（版本头正则、必填字段）。
 *
 * 用法：`node scripts/check-boot-report.mjs`（要先 `pnpm build`）
 */
import { chromium } from "playwright";
import { readFileSync, existsSync } from "node:fs";
import { createServer } from "node:http";

const fails = [];

if (!existsSync("dist/index.html")) {
  console.error("❌ 没有 dist/index.html —— 先跑 pnpm build");
  process.exit(1);
}

// 真产物，只把主模块指向一个会在顶层抛错的桩（模拟「某个 import 顶层抛错」）
const FAKE_USER_PATH = "/Users/zhangsan/U-King.app/Contents/Resources/app.js";
const html = readFileSync("dist/index.html", "utf8").replace(
  /<script type="module"[^>]*src="[^"]*"[^>]*><\/script>/,
  '<script type="module" src="./boom.js"></script>',
);
if (!html.includes("./boom.js")) fails.push("跑道自己先坏了：没能把 dist/index.html 的主模块换成桩");

// 🔴 前缀不许去掉。原来这里一字不差地写着真 WebKit 的那句
// `Invalid regular expression: invalid group specifier name` —— 于是这条跑道产出的截图
// 跟**真的 Mac 白屏**长得完全一样。2026-08-16 就为了判一张这样的截图是真崩溃还是赝品，
// 翻了 main、拆了 Mac 包、扫了二进制、ssh 上了 Mac mini。
// 测试用的假数据必须一眼能认出是假的，否则它迟早会被当成证据。
const BOOM = `throw new Error(${JSON.stringify(
  `[check-boot-report fixture] Invalid regular expression: invalid group specifier name (at ${FAKE_USER_PATH})`,
)});`;

const server = createServer((req, res) => {
  const url = (req.url || "/").split("?")[0];
  if (url.endsWith("boom.js")) {
    res.writeHead(200, { "content-type": "text/javascript" });
    res.end(BOOM);
  } else if (url === "/" || url.endsWith("index.html")) {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(html);
  } else {
    res.writeHead(404).end();
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;

const browser = await chromium.launch();
const page = await browser.newPage();

let sent = null;
// ★ 拦死，绝不放行到线上 —— 跑道不许在真仓库里建 Issue
await page.route("**/uking/bug", async (route) => {
  const req = route.request();
  sent = { headers: req.headers(), body: req.postData() || "", method: req.method() };
  await route.fulfill({ status: 200, contentType: "application/json", body: '{"ok":true}' });
});

// 宿主注入的 __TAURI_INTERNALS__ —— 真 app 里由 Tauri 自己注入，**不经过我们那个已经加载失败的
// 前端包**，所以主包炸掉时它仍然在。这里照着 shim 一份，好验「下载最新版」真的调得到 opener。
await page.addInitScript(() => {
  window.__openerCalls = [];
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      window.__openerCalls.push({ cmd, args });
      return Promise.resolve(null);
    },
    transformCallback: (cb) => cb,
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
});

console.log("加载真 dist/index.html（主模块换成会抛错的桩），等 10s 看门狗…");
await page.goto(`http://127.0.0.1:${port}/`);
await page.waitForTimeout(13000);

// ① 兜底界面本身还在不在（上报不许把它挤掉）
const shown = await page.locator("text=U-King 界面未能加载").count();
if (!shown) fails.push("兜底界面没出来 —— 用户还是对着一片黑");

// ①.5 逃生路：白屏的 app 自己升不了级（升级按钮在没渲染出来的界面里），而**开机就语法错时
//      `location.reload()` 每次都会以完全相同的方式失败** —— 2026-08-16 之前兜底页唯一的按钮
//      就是这么一个可证明救不了它的按钮，拿到坏版本的人被永久卡死（0.9.99/0.9.100 的 Mac 包）。
{
  const btn = page.getByRole("button", { name: /下载最新版/ });
  if ((await btn.count()) === 0) {
    fails.push("🔴 兜底页没有「下载最新版」—— 用户唯一能点的是 reload，而它每次都会一样地失败");
  } else {
    await btn.first().click();
    await page.waitForTimeout(200);
    const calls = await page.evaluate(() => window.__openerCalls || []);
    const open = calls.find((c) => c.cmd === "plugin:opener|open_url");
    if (!open) fails.push("点了「下载最新版」但没调 plugin:opener|open_url —— 按钮是死的");
    else if (!String(open.args?.url || "").includes("u-claw.org.cn"))
      fails.push(`下载地址是 ${open.args?.url} —— 给客户端的 URL 必须走 u-claw.org.cn（国内可达性铁律）`);
    else console.log(`     ✓ 逃生路可用：opener → ${open.args.url}`);
    // opener 和剪贴板万一都不通，地址还得明写在页面上 —— 这是最后一条路，不能依赖任何东西
    const body = await page.evaluate(() => document.body.innerText);
    if (!body.includes("u-claw.org.cn"))
      fails.push("页面上没有明写下载地址 —— opener/剪贴板都不通时用户没法照着敲");
  }
}

// ② 请求真发出去了吗
if (!sent) {
  fails.push("🔴 首屏崩了但**一个上报请求都没发** —— 正是这次白屏零 bug 记录的那个洞");
} else {
  if (sent.method !== "POST") fails.push(`方法是 ${sent.method}，应为 POST`);

  // ③ 版本头 —— bug.js 第 25 行：不匹配 /^\d+\.\d+\.\d+$/ 直接 400，且我们看不到任何反馈
  const ver = sent.headers["x-uking-version"] || "";
  if (!/^\d+\.\d+\.\d+$/.test(ver)) {
    fails.push(`x-uking-version = "${ver}" 不合法（服务端会 400 静默丢弃；%APP_VERSION% 没替换？）`);
  } else {
    const pkgVer = JSON.parse(readFileSync("package.json", "utf8")).version;
    if (ver !== pkgVer) fails.push(`版本头 ${ver} ≠ package.json 的 ${pkgVer}`);
    else console.log(`     ✓ x-uking-version: ${ver}`);
  }

  // ④ 必填字段 —— bug.js 第 28 行
  let b = null;
  try {
    b = JSON.parse(sent.body);
  } catch {
    fails.push("body 不是合法 JSON");
  }
  if (b) {
    if (b.app !== "u-king-mini") fails.push(`app = ${JSON.stringify(b.app)}，服务端只认 "u-king-mini"`);
    if (!b.kind) fails.push("缺 kind");
    if (!b.summary) fails.push("缺 summary");
    if (!b.device) fails.push("缺 device —— 没它服务端按「kind|device|当天」去重会把全世界并成一条");

    // ⑤ 错误内容真的带上了（不然建出来的 Issue 是张白条）
    if (!/invalid group specifier name/.test(String(b.detail) + String(b.summary))) {
      fails.push("上报里没有原始错误信息 —— Issue 建出来也没法查");
    }

    // ⑥ 脱敏：用户名不许出门
    if (String(b.detail).includes("zhangsan")) fails.push("🔴 用户名 zhangsan 原样发出去了 —— 脱敏没生效");
    else if (!String(b.detail).includes("/Users/*/")) fails.push("脱敏后没看到 /Users/*/，正则可能没匹配上");
    else console.log("     ✓ 脱敏：/Users/zhangsan/ → /Users/*/");

    if (b.kind) console.log(`     ✓ kind=${b.kind} device=${b.device} os=${b.os}`);
  }
}

await browser.close();
server.close();

if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 首屏起不来时：兜底界面出得来，bug 也真的发得出去（且没发到线上）");
