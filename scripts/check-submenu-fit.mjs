/**
 * `+` 菜单二级列表的**落点跑道** —— 断言子菜单整个在视口里，不被上下切掉。
 *
 * 为什么单开一条：这一处已经用两个方向的常量各错过一次，方向相反、症状对称 ——
 *  - `bottom-0`（往上长）：行离顶近时飘出顶栏；
 *  - `top-0`（往下长）：行离底近时下半截被切（客户实拍「这个右侧列表往下就盖住了」）。
 * 两次都是 `cargo check` / `pnpm build` / `action conformance` **全绿**发出去的 ——
 * 排版没有跑道就只能等客户截图，而客户只会在第三次踩到时说「还是不对」。
 *
 * 矮视口是关键档：`+` 菜单贴着输入框往上弹，视口越矮，「专家 / 模型」那两行离底边越近。
 *
 * 用法：node scripts/check-submenu-fit.mjs   （需先 pnpm dev；换端口用 UKING_DEV_URL=）
 * 退出码：0 = 全在视口内；1 = 有一档被切。
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
/** 想看图就 `UKING_SHOT_OUT=<目录>`；不设就只出数字。 */
const SHOT = process.env.UKING_SHOT_OUT || "";
if (SHOT) mkdirSync(SHOT, { recursive: true });
const PLUS_TITLE = "添加文件 / 选专家 / 换模型（也可以直接把文件拖进来）";

/** 视口不是屏幕分辨率：扣掉任务栏 + 原生标题栏才是网页拿到的高度。矮的那两档是本跑道的主场。 */
const CASES = [
  { name: "1280x640", w: 1280, h: 640 },        // 1920×1008 @150% —— 全场最紧
  { name: "1366x696", w: 1366, h: 768 - 40 - 32 },
  { name: "1920x1008", w: 1920, h: 1080 - 40 - 32 }, // 对照组：宽屏也不许飘出去
];

/** 最小 Tauri shim —— 只为让界面渲染出来，不模拟业务。 */
const TAURI_SHIM = () => {
  const WS = "C:\\demo\\uking-mini";
  const fake = (cmd) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\demo", installed: true, opened_dir: WS };
    if (cmd === "list_tools") return [];
    if (cmd === "list_tasks") return [];
    if (cmd === "list_automations") return { jobs: [] };
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false };
    if (cmd === "list_dir") return [];
    /* 选文件夹的弹窗直接给一个假路径 —— 没有工作目录，对话面板只渲染空态，`+` 根本不存在。 */
    if (cmd?.startsWith("plugin:dialog|open")) return WS;
    if (cmd?.startsWith("plugin:event|")) return 1;
    return null;
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd) => Promise.resolve(fake(cmd)),
    convertFileSrc: (p) => "https://asset.localhost/" + encodeURIComponent(p),
    transformCallback: (cb) => { const id = 1234567; window[`_${id}`] = cb; return id; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
};

/**
 * 把界面掰成**「已经聊过一句」**的排版 —— 客户截图就是这一档，也只有这一档会被切。
 *
 * 🔴 空会话的排版**结构上**出不了这个问题，两处都在把输入框往上顶：
 *  - 起手词（21 条那块）只在 `items.length === 0` 时渲染，占着输入框下面一百多像素；
 *  - 整列还带 `justify-center`，空态是**垂直居中**的，输入框离底边有 245px。
 * 聊过一句之后两样都没了，输入框贴着窗口底边，`+` 菜单的「专家 / 模型」两行离底只剩几十像素
 * —— 这才是子菜单往下长会被切的那一档。不掰这一下，本跑道就是对着一个不会出问题的布局报绿。
 *
 * 掰完必须验「`+` 真的往下走了」（下面那道 60px 的判据），否则选择器哪天失配，
 * 这段会静默变成空操作，而跑道照样全绿 —— 那比没有跑道更坏。
 */
const EMULATE_CHATTED = () => {
  const b = [...document.querySelectorAll("button")].find((e) => e.textContent.trim() === "日常办公");
  let quick = b;
  while (quick && !String(quick.className || "").includes("mt-2")) quick = quick.parentElement;
  if (quick) quick.style.display = "none";

  /* 🔴 认「整列」要同时看 `flex-col`：`+` 按钮自己的 class 里也有 `justify-center`
     （`w-7 justify-center px-0`），只按这一个词往上找，第一跳就停在按钮身上 —— 那一步
     什么都没改，而 `col:true` 照样为真。这条正是本跑道的判据差点被自己骗过去的地方。 */
  const plus = document.querySelector('button[title^="添加文件 / 选专家"]');
  let col = plus?.parentElement;
  while (col && !(String(col.className || "").includes("justify-center") && String(col.className || "").includes("flex-col"))) col = col.parentElement;
  if (col) col.style.justifyContent = "flex-end";
  return { quick: !!quick, col: !!col };
};

/** 量子菜单：它是那一行 `relative` 容器里唯一 `absolute` 且可滚的浮层。 */
const MEASURE = () => {
  const box = [...document.querySelectorAll("div.absolute.left-full")].find((el) => el.getBoundingClientRect().height > 0);
  if (!box) return null;
  const r = box.getBoundingClientRect();
  return {
    top: Math.round(r.top), bottom: Math.round(r.bottom), h: Math.round(r.height),
    vh: window.innerHeight,
    clippedTop: Math.round(Math.max(0, -r.top)),
    clippedBottom: Math.round(Math.max(0, r.bottom - window.innerHeight)),
    items: box.querySelectorAll("button").length,
  };
};

const browser = await chromium.launch();
const rows = [];
let bad = 0;

for (const c of CASES) {
  const page = await browser.newPage({ viewport: { width: c.w, height: c.h } });
  await page.addInitScript(TAURI_SHIM);
  await page.goto(URL, { waitUntil: "networkidle" }).catch(() => {});
  await page.waitForSelector("aside", { timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(1500);

  await page.getByRole("button", { name: "U-Workspace", exact: false }).first().click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(1500);
  /* 新建对话 → 选工作文件夹（走上面那个 dialog 假路径），走完才有输入框和 `+`。 */
  await page.getByRole("button", { name: "新建对话", exact: true }).first().click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "选工作文件夹", exact: true }).first().click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(1800);

  const plus = page.getByTitle(PLUS_TITLE).first();
  if (!(await plus.count())) { rows.push({ case: c.name, err: "找不到 + 按钮（界面没进到对话面板？）" }); bad++; await page.close(); continue; }

  const before = (await plus.boundingBox())?.y ?? 0;
  const hit = await page.evaluate(EMULATE_CHATTED);
  await page.waitForTimeout(200);
  const after = (await plus.boundingBox())?.y ?? 0;
  if (!hit.quick || !hit.col || after - before < 60) {
    rows.push({ case: c.name, err: `没掰成「聊过一句」的排版（+ 只挪了 ${Math.round(after - before)}px，命中 ${JSON.stringify(hit)}）—— 量的不是客户那一档`, before: Math.round(before), after: Math.round(after) });
    bad++; await page.close(); continue;
  }

  await plus.click();
  await page.waitForTimeout(300);

  /* 「专家」和「模型」都要验：它们在菜单里的行高不同，离底边的距离也不同。 */
  for (const label of ["专家", "模型"]) {
    const row = page.getByRole("button", { name: new RegExp(`^${label}`) }).first();
    if (!(await row.count())) { rows.push({ case: c.name, sub: label, err: "菜单里没有这一项" }); continue; }
    await row.hover();
    await page.waitForTimeout(250);
    const m = await page.evaluate(MEASURE);
    if (!m) { rows.push({ case: c.name, sub: label, err: "子菜单没展开" }); bad++; continue; }
    const ok = m.clippedTop === 0 && m.clippedBottom === 0;
    if (!ok) bad++;
    rows.push({ case: c.name, sub: label, ok, ...m });
    /* 数字说「没被切」，截图说「切在哪」—— 判据用前者，人看后者。默认不存，省得每跑一次留一堆图。 */
    if (SHOT) await page.screenshot({ path: `${SHOT}/${c.name}-${label}.png` });
  }
  await page.close();
}

await browser.close();
console.log(JSON.stringify({ ok: bad === 0, bad, rows }, null, 2));
if (bad) console.error(`✗ ${bad} 档子菜单被视口切掉`);
else console.log("✓ 二级菜单在所有档位都完整落在视口内");
process.exit(bad ? 1 : 0);
