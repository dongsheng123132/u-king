/**
 * English UI smoke test: render the real React app in Chromium, visit every
 * reachable U-Workspace view, then report visible Chinese fallback and hard
 * clipping at the two tight desktop viewport sizes used by layout regression.
 *
 * Run with `pnpm dev`, then `node scripts/check-english-ui.mjs`.
 */
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import path from "node:path";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const OUT = process.env.UKING_ENGLISH_SHOT_OUT || path.join(process.env.TEMP || ".", "uking-english-ui");
const CASES = [
  { name: "1280x640", width: 1280, height: 640 },
  { name: "1366x696", width: 1366, height: 696 },
];

const SHIM = () => {
  const now = Date.now();
  try {
    localStorage.setItem("uking.lang", "en");
    localStorage.removeItem("uworkspace.sidebar.width");
    localStorage.removeItem("uworkspace.sidebar.collapsed");
    localStorage.setItem("uking.board.ai_sources.v1", JSON.stringify(["claude"]));
  } catch { /* ignore */ }
  const task = {
    id: "english-layout-session",
    name: "English layout audit",
    dir: "C:\\demo\\english-layout-audit",
    status: "idle",
    source: "manual",
    assignee: null,
    external_ref: null,
    last_opened_at: now,
    created_at: now,
    kind: "task",
    project: "english-layout-audit",
  };
  const fake = (cmd, args) => {
    if (cmd === "get_env") return { platform: "windows", home_dir: "C:\\Users\\demo", installed: true, opened_dir: null };
    if (cmd === "list_tools") return [];
    if (cmd === "list_tasks") return [task];
    if (cmd === "upsert_task") return args?.task ?? task;
    if (cmd === "list_automations") return {
      jobs: [],
      running_id: null,
      ready: true,
      blockers: [],
      count: 0,
      enabled: 0,
      max: 30,
      runs_only_while_app_open: false,
    };
    if (cmd === "list_ai_tasks") return {
      days: 7,
      active_window_secs: 300,
      truncated: false,
      notes: [],
      counts: { idle: 1 },
      sources: [],
      tasks: [],
    };
    if (cmd === "get_setup_state") return { step: "done" };
    if (cmd === "get_driver_status") return {};
    if (cmd === "check_update") return { has_update: false };
    if (cmd === "get_device_key") return { key: "sk-demo", charged: false };
    if (cmd === "list_handoffs") return { handoffs: [] };
    if (cmd === "action_run") return { ok: true, data: { handoffs: [] } };
    if (cmd === "action_parity_call") return { ok: true, result: { handoffs: [], packs: [] } };
    if (cmd?.startsWith("plugin:event|")) return 1;
    return null;
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => Promise.resolve(fake(cmd, args)),
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

const AUDIT = () => {
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 &&
      r.top < innerHeight && r.left < innerWidth && s.display !== "none" &&
      s.visibility !== "hidden" && s.opacity !== "0";
  };
  const directText = (el) => [...el.childNodes]
    .filter((n) => n.nodeType === Node.TEXT_NODE)
    .map((n) => n.textContent || "")
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();
  const chinese = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const text = (n.textContent || "").replace(/\s+/g, " ").trim();
    const el = n.parentElement;
    // The language picker intentionally shows the target language in its own
    // script, so the single “中” option is not an untranslated UI string.
    if (el && text && text !== "中" && /[一-龥]/.test(text) && visible(el)) chinese.push(text.slice(0, 100));
  }
  for (const el of document.querySelectorAll("[title],[placeholder],option,optgroup")) {
    if (!visible(el)) continue;
    for (const value of [el.getAttribute("title"), el.getAttribute("placeholder"), el.getAttribute("label")]) {
      if (value && /[一-龥]/.test(value)) chinese.push(value.slice(0, 100));
    }
  }
  const clipped = [];
  for (const el of document.querySelectorAll("button,a,label,h1,h2,h3,p,span")) {
    if (!visible(el)) continue;
    const text = directText(el);
    if (!text || text.length < 2 || /^\p{Extended_Pictographic}\uFE0F?$/u.test(text)) continue;
    const s = getComputedStyle(el);
    const clipsX = el.scrollWidth > el.clientWidth + 1 && !["auto", "scroll"].includes(s.overflowX);
    const clipsY = el.scrollHeight > el.clientHeight + 1 && !["auto", "scroll"].includes(s.overflowY);
    if (!clipsX && !clipsY) continue;
    const intentional = el.classList.contains("truncate") ||
      [...el.classList].some((c) => c.startsWith("line-clamp-"));
    clipped.push({
      text: text.slice(0, 100),
      tag: el.tagName.toLowerCase(),
      x: clipsX,
      y: clipsY,
      intentional,
      box: `${el.clientWidth}x${el.clientHeight}`,
      scroll: `${el.scrollWidth}x${el.scrollHeight}`,
    });
  }
  return {
    chinese: [...new Set(chinese)],
    clipped,
    documentOverflowX: Math.max(0, document.documentElement.scrollWidth - innerWidth),
  };
};

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();
const report = [];

for (const viewport of CASES) {
  const page = await browser.newPage({ viewport: { width: viewport.width, height: viewport.height } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e).slice(0, 180)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(`console: ${m.text().slice(0, 180)}`); });
  await page.addInitScript(SHIM);
  await page.goto(URL, { waitUntil: "networkidle" });
  await page.waitForSelector("aside", { timeout: 20000 });
  await page.waitForTimeout(1200);

  const screens = [];
  const capture = async (name) => {
    await page.waitForTimeout(500);
    const audit = await page.evaluate(AUDIT);
    await page.screenshot({ path: path.join(OUT, `${viewport.name}-${name}.png`) });
    screens.push({ name, ...audit });
  };

  await capture("home");
  await page.getByRole("button", { name: /U-Workspace/i }).first().click();
  await capture("workspace");
  for (const name of ["Passports", "Board", "AI Experts", "Automations"]) {
    await page.getByRole("button", { name, exact: true }).first().click();
    await capture(name.toLowerCase().replace(/\s+/g, "-"));
  }
  report.push({ viewport: viewport.name, screens, errors });
  await page.close();
}

await browser.close();
const hardProblems = report.flatMap((v) => v.screens.flatMap((s) => [
  ...s.chinese.map((text) => ({ viewport: v.viewport, screen: s.name, kind: "chinese", text })),
  ...s.clipped.filter((c) => !c.intentional).map((c) => ({ viewport: v.viewport, screen: s.name, kind: "clip", ...c })),
  ...(s.documentOverflowX ? [{ viewport: v.viewport, screen: s.name, kind: "document-overflow", px: s.documentOverflowX }] : []),
]));
console.log(JSON.stringify({ out: OUT, hardProblems, report }, null, 2));
if (hardProblems.length || report.some((v) => v.errors.length)) process.exitCode = 1;
