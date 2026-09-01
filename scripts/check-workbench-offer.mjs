/**
 * 「空文件夹要不要布置成工作台」那一问的跑道 —— **真数据喂真组件**，不截屏。
 *
 * ## 为什么值得单开一条
 * 这一问的判断只有三行（`installable && empty && !is_workbench`），但两个方向都很贵：
 *  - 写松了 → **每个项目文件夹都弹一次窗**，客户每天被打断，还会以为我们要动他的目录；
 *  - 写紧了 / 字段名打错 → **永远不弹，且没有任何报错**。tsc 拦不住（那些字段都是 any），
 *    单测也拦不住（判断在 React 里）。这类「静默不发生」的 bug 只能靠真点一遍。
 * 而「点了确认却没传 confirmed」更隐蔽：动作会被确认门禁挡回来，按钮看着能点、什么都没发生。
 *
 * ## 真在哪儿、假在哪儿（不含糊）
 *  - **真**：`useWorkbenchOffer` 本体（不复制一份判断）、真 Chromium、真 React 渲染；
 *    三种目录状态的返回**是真 exe 现算的**（`runtime.workbench.inspect`），不是我手写的假 JSON。
 *  - **假**：只有 `invoke` 这一层被替换成「照着上面那三份真返回作答」，因为浏览器里没有 Tauri。
 *  - **验不到**：原生选目录对话框（那是系统的）。所以跑道从「已经选好了一个目录」开始。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-workbench-offer.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { execFileSync } from "node:child_process";
import { writeFileSync, unlinkSync, mkdirSync, rmSync, existsSync } from "node:fs";
import path from "node:path";
import os from "node:os";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const EXE = path.resolve("src-tauri/target/release/u-king-mini.exe");
const BASE = path.join(os.tmpdir(), "wb-offer-probe");

function action(id, input, yes = false) {
  const args = ["action", "run", id, "--json", "--input", JSON.stringify(input)];
  if (yes) args.push("--yes");
  const out = execFileSync(EXE, args, { encoding: "utf8", windowsHide: true, stdio: ["ignore", "pipe", "ignore"] });
  const j = JSON.parse(out);
  return j.result ?? j;
}

console.log("[1/6] 用真 exe 现算三种目录状态…");
if (!existsSync(EXE)) {
  console.error("❌ 没有 release exe，先 cargo build --release");
  process.exit(1);
}
rmSync(BASE, { recursive: true, force: true });
mkdirSync(path.join(BASE, "empty"), { recursive: true });
mkdirSync(path.join(BASE, "foreign"), { recursive: true });
writeFileSync(path.join(BASE, "foreign", "别人的稿子.docx"), "x");
action("runtime.workbench.install", { path: path.join(BASE, "installed") }, true);

const CASES = ["empty", "foreign", "installed"];
const responses = {};
for (const k of CASES) {
  const p = path.join(BASE, k);
  responses[p.replace(/\\/g, "/")] = action("runtime.workbench.inspect", { path: p });
}
const emptyPath = path.join(BASE, "empty").replace(/\\/g, "/");
const realDirs = (responses[emptyPath].target.plan || [])
  .filter((s) => s.kind === "dir" && s.verdict === "create" && s.path !== ".")
  .map((s) => s.path);
console.log("     空目录会建：" + realDirs.join(" / "));
if (realDirs.length < 3) {
  console.error("❌ 真 exe 给的计划里目录太少，跑道自己先坏了");
  process.exit(1);
}

const SHIM = ({ responses }) => {
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      window.__calls.push({ cmd, args: JSON.parse(JSON.stringify(args || {})) });
      if (cmd === "action_parity_call") {
        const req = args?.request || {};
        if (req.action_id === "runtime.workbench.inspect") {
          const key = String(req.input?.path || "").replace(/\\/g, "/");
          const r = responses[key];
          return Promise.resolve(r ? { ok: true, result: r } : { ok: false, error: { message: "no fixture " + key } });
        }
        if (req.action_id === "runtime.workbench.install") {
          // 影核核心的门禁：没 confirmed 就该被挡回来。跑道照抄这条规矩，
          // 否则「按钮忘了传 confirmed」在这里会显示成成功。
          if (!req.confirmed) {
            return Promise.resolve({ ok: false, error: { message: "confirmation_required" } });
          }
          // `next` / `warnings` 必须在假数据里也有 —— 前端曾经把 next 扔掉自己另写了一句
          // 「AI 进这个文件夹会先读 WORKBENCH.md」，而后端的 ENTRYPOINTS 注释写着没有任何
          // CLI 会自动读那个名字。假数据里不摆这两个字段，跑道就永远验不到这条。
          const withWarn = window.__wantWarnings === true;
          return Promise.resolve({
            ok: true,
            result: {
              ok: true,
              created: 13,
              updated: 0,
              skipped: 0,
              next: "直接在这个文件夹里开工就行 —— AI 进来会自动读 AGENTS.md / CLAUDE.md，被指到 WORKBENCH.md",
              warnings: withWarn
                ? ["`CLAUDE.md`（Claude Code 进这个文件夹时读的那份）里没提 WORKBENCH.md —— AI 读不到本工作台的约定"]
                : [],
            },
          });
        }
      }
      if (cmd?.startsWith("plugin:event|")) return Promise.resolve(1);
      return Promise.resolve(null);
    },
    transformCallback: (cb) => { const id = Math.floor(Math.random() * 1e9); window[`_${id}`] = cb; return id; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
};

/**
 * 在浏览器里量弹窗每一处文字的**实际**对比度。
 *
 * 为什么要有这条：这条跑道原本全部用 `innerText` / `text=` 断言 —— 而 `innerText`
 * **不看颜色**。v0.9.99 的弹窗在浅色主题下整页白底白字（12 处 `text-white/xx` 压在纯白
 * `bg-bg-2` 上），跑道照样全绿，客户那边是一个空白框。文字「在 DOM 里」和「人能读到」
 * 是两回事，这里量的是后者。
 *
 * 算法就是 WCAG 那套：把元素自己和所有祖先的背景按 alpha 叠出实际底色，
 * 再和 computed color 算相对亮度比。门槛取 3:1 —— 不是 AA 的 4.5，
 * 是因为房子里 ink-3 这类提示文字本来就贴着 4.5 走，而白底白字是 1:1，
 * 3:1 足够把「看不见」和「淡但读得清」分开，且不会天天误报。
 */
const CONTRAST_PROBE = () => {
  const MIN = 3;
  const panel = document.querySelector(".fixed.inset-0 > div");
  if (!panel) return { error: "没找到弹窗面板", bad: [], checked: 0, min: MIN };

  const lin = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
  const lum = ({ r, g, b }) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  const parse = (s) => {
    const m = String(s).match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    const p = m[1].split(",").map((x) => parseFloat(x));
    return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
  };
  const over = (fg, bg) => ({
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1,
  });
  /** 从 body 往里把每一层非透明背景叠上去 —— 半透明遮罩、半透明按钮底都算数 */
  const effBg = (el) => {
    const chain = [];
    for (let n = el; n; n = n.parentElement) chain.push(n);
    let bg = parse(getComputedStyle(document.body).backgroundColor) || { r: 255, g: 255, b: 255, a: 1 };
    if (bg.a < 1) bg = over(bg, { r: 255, g: 255, b: 255, a: 1 });
    for (const n of chain.reverse()) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c && c.a > 0) bg = over(c, bg);
    }
    return bg;
  };

  const bad = [];
  let checked = 0;
  let worst = 21;
  for (const el of panel.querySelectorAll("*")) {
    // 只量**自己直接持有文字**的元素，避免把父容器重复算一遍
    const own = [...el.childNodes]
      .filter((n) => n.nodeType === 3)
      .map((n) => n.textContent.trim())
      .join(" ")
      .trim();
    if (!own) continue;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden" || parseFloat(cs.opacity) === 0) continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;

    const fgRaw = parse(cs.color);
    if (!fgRaw) continue;
    const bg = effBg(el);
    const fg = fgRaw.a < 1 ? over(fgRaw, bg) : fgRaw;
    const [l1, l2] = [lum(fg), lum(bg)].sort((a, b) => b - a);
    const ratio = Math.round(((l1 + 0.05) / (l2 + 0.05)) * 10) / 10;
    checked++;
    if (ratio < worst) worst = ratio;
    if (ratio < MIN) bad.push({ text: own.slice(0, 24), ratio, color: cs.color });
  }
  return { bad, checked, worst, min: MIN, error: "" };
};

const PROBE_NAME = "__wb-offer-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>wb offer probe</title></head>
<body><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { I18nProvider } from "/src/i18n";
import { useWorkbenchOffer } from "/src/opencodex/useWorkbenchOffer";
function Probe() {
  const { offer, node } = useWorkbenchOffer();
  React.useEffect(() => {
    window.__offer = (dir) => { window.__state = "pending"; return offer(dir).then(() => { window.__state = "resolved"; }); };
    window.__ready = true;
  }, [offer]);
  return React.createElement("div", null, node);
}
createRoot(document.getElementById("root")).render(React.createElement(I18nProvider, null, React.createElement(Probe)));
</script></body></html>`;

const fails = [];
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
page.on("console", (m) => { if (m.type() === "error") errors.push("console: " + m.text().slice(0, 200)); });
await page.addInitScript(SHIM, { responses });

console.log("[2/6] 挂载真 hook…");
writeFileSync(PROBE_NAME, PROBE_HTML);
process.on("exit", () => { try { unlinkSync(PROBE_NAME); } catch {} });
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 15000 }).catch(() => {
  fails.push("hook 没挂起来（dev server 起了吗？）");
});

const dialogVisible = () => page.locator("text=这个文件夹是空的，要布置一下吗？").count().then((n) => n > 0);

console.log("[3/6] 三种目录状态各点一遍…");
for (const [k, expect] of [["empty", true], ["foreign", false], ["installed", false]]) {
  const dir = path.join(BASE, k);
  await page.evaluate((d) => { window.__offer(d); }, dir); // 不 return promise：它要等弹窗关才 resolve
  await page.waitForTimeout(600);
  const shown = await dialogVisible();
  if (shown !== expect) {
    fails.push(`${k} 目录：弹窗=${shown}，应为 ${expect}` + (expect ? "（客户选了空文件夹却什么都没得到）" : "（在他自己的项目上打扰他）"));
  }
  if (!expect) {
    // 不弹的时候必须**立刻放行**，否则「新建项目」会卡在一个看不见的 await 上
    const st = await page.evaluate(() => window.__state);
    if (st !== "resolved") fails.push(`${k} 目录：不弹窗却没放行（新建项目会卡住）`);
  }
  if (shown) {
    const body = await page.evaluate(() => document.body.innerText);
    const missing = realDirs.filter((d) => !body.includes(d));
    if (missing.length) fails.push(`弹窗没列出真实会建的目录：${missing.join(",")}`);
    // 关掉，进下一个 case
    await page.locator("text=先空着").first().click();
    await page.waitForTimeout(300);
    if (await dialogVisible()) fails.push("点了「先空着」还关不掉");
    if ((await page.evaluate(() => window.__state)) !== "resolved") fails.push("拒绝之后没放行");
  }
}

console.log("[4/6] 点「布置成工作台」—— 确认位有没有真传出去…");
await page.evaluate((d) => { window.__offer(d); }, path.join(BASE, "empty"));
await page.waitForTimeout(600);
const btn = page.locator('[data-action-id="runtime.workbench.install"]');
if (!(await btn.count())) {
  fails.push("装按钮上没有 data-action-id —— 自动化点它只能靠猜像素（宪法第 14 条）");
} else {
  await btn.first().click();
  await page.waitForTimeout(800);
  const calls = await page.evaluate(() => window.__calls);
  const inst = calls.filter((c) => c.args?.request?.action_id === "runtime.workbench.install");
  if (!inst.length) fails.push("点了「布置成工作台」，一个 install 请求都没发出去");
  else if (!inst.at(-1).args.request.confirmed) {
    fails.push("install 没带 confirmed —— 会被确认门禁挡回来，按钮看着能点其实什么都没干");
  }
  const body = await page.evaluate(() => document.body.innerText);
  if (!body.includes("建好了")) fails.push("装完没有把结果告诉客户（他不知道成没成）");
  // 「接下来干什么」必须是后端那句原话。前端自己另写一句，就会写出
  //「AI 进这个文件夹会先读 WORKBENCH.md」这种和 ENTRYPOINTS 相反的话。
  if (!body.includes("AGENTS.md")) {
    fails.push("成功页没显示后端的 next —— 客户不知道接下来该干什么，或者前端又自己编了一句");
  }
  if (/先读\s*WORKBENCH\.md/.test(body)) {
    fails.push("成功页又出现「先读 WORKBENCH.md」—— 没有任何 AI CLI 会自动读这个文件名（见 workbench.rs 的 ENTRYPOINTS）");
  }
}

console.log("[5/6] 后端报了 warnings 时，成功页有没有如实说…");
await page.evaluate(() => { window.__wantWarnings = true; });
await page.locator("text=开始干活").first().click();
await page.waitForTimeout(300);
await page.evaluate((d) => { window.__offer(d); }, path.join(BASE, "empty"));
await page.waitForTimeout(600);
await page.locator('[data-action-id="runtime.workbench.install"]').first().click();
await page.waitForTimeout(800);
{
  const body = await page.evaluate(() => document.body.innerText);
  // 后端装完会回头量真实世界：入口文件是客户自己的 = AI 读不到约定，这次「装好了」是假的。
  // 只报一句「好了」就是 workbench.rs:516 那段注释说的「报告对、世界坏」。
  if (!body.includes("读不到本工作台的约定")) {
    fails.push("后端报了 warnings，弹窗却没说 —— 客户以为装好了，其实 AI 进去一问三不知");
  }
  if (/^\s*好了。/m.test(body)) {
    fails.push("有 warnings 还说「好了。」—— 报告对、世界坏");
  }
}
await page.evaluate(() => { window.__wantWarnings = false; });

console.log("[6/6] 两套主题各量一遍对比度 —— 白底白字 innerText 断言拦不住…");
// 回到「问」那一页再量：目录 chip、for_whom、「它没有什么」、两个按钮都在这一页上，
// 文字最多、也正是客户截图里全空掉的那一页。
await page.locator("text=开始干活").first().click();
await page.waitForTimeout(300);
await page.evaluate((d) => { window.__offer(d); }, path.join(BASE, "empty"));
await page.waitForTimeout(600);
for (const theme of ["light", "dark"]) {
  await page.evaluate((th) => {
    document.documentElement.classList.toggle("dark", th === "dark");
  }, theme);
  await page.waitForTimeout(200);
  const low = await page.evaluate(CONTRAST_PROBE);
  if (low.error) {
    fails.push(`${theme}：对比度探针没找到弹窗（${low.error}）`);
  } else if (low.bad.length) {
    const list = low.bad.slice(0, 6).map((b) => `「${b.text}」${b.ratio}:1`).join("，");
    fails.push(
      `${theme} 主题下有 ${low.bad.length} 处文字对比度低于 ${low.min}:1：${list}` +
        (theme === "light" ? "（浅色是默认主题，客户第一眼看到的就是它）" : ""),
    );
  } else {
    console.log(`     ${theme}：${low.checked} 处文字全部达标（最低 ${low.worst}:1）`);
  }
}
await page.evaluate(() => document.documentElement.classList.remove("dark"));

const realErrors = errors.filter((e) => !/favicon|ERR_/.test(e));
if (realErrors.length) fails.push("页面报错：" + realErrors.slice(0, 2).join(" | "));

await browser.close();
rmSync(BASE, { recursive: true, force: true });

if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不过：");
  for (const f of fails) console.error("   · " + f);
  process.exit(1);
}
console.log("\n✅ 全过：只在空文件夹上问 · 不弹时立刻放行 · 列的是真会建的目录 · 确认位真传出去了 · 结果有回话");
