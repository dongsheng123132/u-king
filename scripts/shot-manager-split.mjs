/**
 * 给「AI 设置」的「左装右选」新布局出图（2026-08-20）。
 *
 * 🔴 **不截用户正在跑的那个 U-King** —— 那台在干活，截它既不可靠也会打扰。
 *    这里连的是**独立的 dev server**，Tauri 调用全部走 shim 喂假数据，
 *    所以出的图只证明**布局和交互**，不证明后端行为（后端本来也一行没改）。
 *
 * 覆盖三种状态，因为它们走的是不同分支、看起来也该不一样：
 *   ① 已装 + 已接管   → 主按钮「启动 X」
 *   ② 未装            → 主按钮「装好 X 并启动」，左栏挂琥珀「未安装」
 *   ③ 窄屏 1100px     → 两栏退回上下堆叠（lg 断点以下）
 *
 * 用法：pnpm dev 起在 1430，然后 node scripts/shot-manager-split.mjs
 *      换端口：UKING_DEV_URL=http://localhost:5173/ node scripts/shot-manager-split.mjs
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = "shots/manager-split";
mkdirSync(OUT, { recursive: true });

/** 装了哪些工具 —— 用来切「已装/未装」两种形态。 */
const INSTALLED_ALL = ["claude-code", "codex", "clawx", "hermes"];
const INSTALLED_SOME = ["claude-code", "codex"]; // clawx / hermes 未装

const SHIM = (installed) => {
  const fake = (cmd) => {
    switch (cmd) {
      case "list_tools":
        return ["claude-code", "codex", "clawx", "hermes"].map((id) => ({
          id,
          name: id,
          installed: installed.includes(id),
          version: installed.includes(id) ? "1.2.3" : null,
          launch_cmd: id === "clawx" ? null : id,
          launch_app: id === "clawx" ? "clawx" : null,
          hidden: false,
        }));
      case "list_providers":
        return [
          { id: "xiapan", name: "虾盘云", summary: "内置 Key，开箱即用", openai_base: "https://api.u-claw.org.cn/v1", anthropic_base: null, model: "deepseek-v4-pro", small_model: "deepseek-v4-flash", key_url: "", key_hint: "API Key", builtin_recharge: true, recommended: true, builtin: true, api_key: "sk-***" },
          { id: "official", name: "官方直连", summary: "用你自己的 Key", openai_base: "", anthropic_base: null, model: "", small_model: "", key_url: "https://console.anthropic.com", key_hint: "API Key", builtin_recharge: false, recommended: false, builtin: true, api_key: "" },
          { id: "deepseek", name: "DeepSeek 官方", summary: "官方直连，自备 Key", openai_base: "https://api.deepseek.com/v1", anthropic_base: "https://api.deepseek.com/anthropic", model: "deepseek-chat", small_model: "deepseek-chat", key_url: "https://platform.deepseek.com", key_hint: "API Key", builtin_recharge: false, recommended: false, builtin: true, api_key: "" },
        ];
      case "driver_status":
        return {
          active: { claude: "xiapan", codex: "official" },
          claude_base: "https://api.u-claw.org.cn/v1",
          claude_model: "deepseek-v4-pro",
          codex_provider: "official",
          clawx_installed: installed.includes("clawx"),
          clawx_model: installed.includes("clawx") ? "deepseek-v4-flash" : null,
          hermes_installed: installed.includes("hermes"),
          hermes_model: null,
          claude_via_bridge: false,
        };
      case "get_device_key":
        return { key: "sk-xp-demo", balance_cny: 128.5, recharge_url: "https://u-claw.org.cn/recharge" };
      case "usage_trend":
        return { days: [], total_cny: 0 };
      case "usage_breakdown":
        return { rows: [] };
      default:
        // event 插件那一族要返回订阅 id，返回 null 会让监听注册失败
        if (cmd?.startsWith("plugin:event|")) return 1;
        if (cmd === "check_update") return { has_update: false };
        return null;
    }
  };
  // 🔴 `metadata` 少了会在 `getCurrentWindow()` 里炸（第一版就这么栽的：
  //    截出来的图是故障边界页，不是界面）。照 `shot-workspace-ui.mjs` 那份补全。
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => Promise.resolve(fake(cmd, args)),
    convertFileSrc: (p) => "https://asset.localhost/" + encodeURIComponent(p),
    transformCallback: (cb) => {
      const id = Math.floor(Math.random() * 1e9);
      window[`_${id}`] = cb;
      return id;
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
};

const browser = await chromium.launch();

async function shot(name, { installed, width, height, clickAi }) {
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 2 });
  await page.addInitScript(SHIM, installed);
  await page.goto(URL, { waitUntil: "networkidle" });
  // 进「AI 设置」：侧栏「更多」里那一条
  await page.getByText("更多", { exact: false }).first().click().catch(() => {});
  await page.waitForTimeout(300);
  await page.getByText("AI 设置", { exact: false }).first().click().catch(() => {});
  await page.waitForTimeout(900);
  // 模拟交互：点左栏某个 AI，验右栏和主按钮跟着变（这才是「左装右选」成不成立的判据）
  if (clickAi) {
    // 用 :has-text 而不是 getByRole({name}) —— 卡片的可及名字含图标+副标题，正则对不上（实测超时）
    await page.locator(`button:has-text("${clickAi}")`).first().click();
    await page.waitForTimeout(500);
  }
  // 🔴 截图前先确认**截的是界面**：第一版截出来的是故障边界页，而文件名照样叫「已装-宽屏」。
  //    一张名字对、内容错的图比没有图更坏 —— 它会被当成证据。
  const boom = await page.getByText("界面已停止").count().catch(() => 0);
  if (boom) {
    const txt = await page.textContent("body").catch(() => "");
    throw new Error(`页面崩在故障边界上，没截到界面：
${(txt || "").slice(0, 400)}`);
  }
  // 顺带量一下有没有横向溢出 —— 窄屏最常见的坏法是「没堆叠 + 出横向滚动条」
  const over = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  if (over > 2) console.log(`     ⚠ 横向溢出 ${over}px`);
  const file = `${OUT}/${name}.png`;
  await page.screenshot({ path: file, fullPage: false });
  console.log(`  📸 ${file}`);
  await page.close();
}

console.log("给「左装右选」出图（独立 dev 实例，不碰你在跑的 U-King）：");
await shot("1-已装-宽屏1440", { installed: INSTALLED_ALL, width: 1440, height: 900 });
await shot("2-有未装-宽屏1440", { installed: INSTALLED_SOME, width: 1440, height: 900 });
// 🔴 用例名别断言不成立的事：1100px **不会**堆叠（Tailwind lg 断点是 1024），
// 第一版起名「窄屏1100-应退回堆叠」，截出来是两栏 —— 名字对、内容错，比没图更坏。
await shot("3-中窄1100-仍是两栏", { installed: INSTALLED_SOME, width: 1100, height: 900 });
// 真窄：应用自己声明的最小宽度是 900，这一档必须堆叠且不横向溢出
await shot("3b-真窄940-应堆叠", { installed: INSTALLED_SOME, width: 940, height: 900 });
// ★ 交互：点一个**没装**的，主按钮必须从「启动 X」变成「装好 X 并启动」
await shot("4-点未装的ClawX", { installed: INSTALLED_SOME, width: 1440, height: 900, clickAi: "ClawX" });
await browser.close();
console.log("完成。");
