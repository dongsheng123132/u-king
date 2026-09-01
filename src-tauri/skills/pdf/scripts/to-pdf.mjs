#!/usr/bin/env node
/**
 * to-pdf.mjs —— 把 Markdown / HTML / Word / Excel / PPT 转成 **PDF**（客户拿去发出去的最终形态）。
 *
 * ## 两条引擎，按输入分流（这是本包最关键的设计）
 *
 * **① Chromium（Edge / Chrome）—— 默认，零安装**
 *    `msedge --headless --print-to-pdf`。**Windows 10/11 出厂自带 Edge，客户机 100% 有**，
 *    macOS 上找 Chrome/Edge。实测 0.39s 出一份中文完整、**文字可搜索**（不是图片）的 PDF。
 *    走这条：`.md` / `.html`。AI 写完的报告九成是 Markdown，这条覆盖了最高频的场景。
 *
 * **② LibreOffice —— 客户拿来的 Office 文件才需要**
 *    `soffice --headless --convert-to pdf`。带公司模板的 Word/PPT 要**版式一个像素不动**，
 *    那是 LibreOffice 花二十年做的活，我们手搓只会出「字都在但版式全错」的东西。
 *    走这条：`.docx/.doc/.xlsx/.pptx/.odt/...`
 *
 * ⚠️ **没有可用引擎就诚实报「转不了」，绝不静默降级**成「把文字抽出来重排一份 PDF」——
 * 那种产物客户打开才发现版式没了，比直接说做不到坏得多。
 *
 * 用法：
 *   node to-pdf.mjs 报告.md --json                    # 走 Edge，零安装
 *   node to-pdf.mjs 合同.docx --json                  # 走 LibreOffice
 *   node to-pdf.mjs 报表.xlsx --out D:/发货/报表.pdf --json
 *   node to-pdf.mjs 页面.html --engine chromium --json
 *   node to-pdf.mjs --check --json                    # 查这台机器有哪条引擎
 *
 * 输出：`{"ok":true,"file":"…pdf","size":N,"engine":"Edge (headless)","ms":390}`
 *      转不了时 `{"ok":false,"error":"…","how_to_fix":"…"}`（退出码 1）
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawnSync } from "node:child_process";

function parseArgs(a) {
  const o = { _: [] };
  for (let i = 0; i < a.length; i++) {
    const t = a[i];
    if (t.startsWith("--")) { const k = t.slice(2); o[k] = a[i + 1] && !a[i + 1].startsWith("--") ? a[++i] : true; }
    else o._.push(t);
  }
  return o;
}
const args = parseArgs(process.argv.slice(2));
const asJson = !!args.json;
// 🔴 「以管理员身份」这句不是客套：2026-08-04 本机实测，**非提权**会话里
// `winget install TheDocumentFoundation.LibreOffice --silent` 直接 1603 失败，
// 而且**UAC 提示压根不弹**（winget 在非提权控制台不会自动提权），客户只会看到「没装上」。
// 顺带试过 7z 解 MSI 绕开管理员：出来 19494 个扁平文件、缺 program/share，
// soffice 跑得起来但转不出任何东西（还是退出码 0）—— 这条路不通，别再试。
const FIX = "装 LibreOffice（**必须用管理员身份的终端**，普通终端会静默失败）：以管理员身份打开 PowerShell → `winget install TheDocumentFoundation.LibreOffice -e`；或直接下官网安装包双击装（会弹 UAC，点「是」）。macOS: `brew install --cask libreoffice`。装完不用重启 U-King。";
function out(o, code = 0) {
  if (asJson) console.log(JSON.stringify(o));
  else if (o.ok) console.log(o.file || o.engine);
  else { console.error("[to-pdf] " + o.error); if (o.how_to_fix) console.error("  → " + o.how_to_fix); }
  process.exit(code);
}

/** 找 soffice。跟 `src-tauri/src/officedoc.rs::soffice_path` 同一批候选路径，别各找各的。 */
function findSoffice() {
  const win = process.platform === "win32";
  const names = win ? ["soffice.exe"] : ["soffice"];
  const dirs = [
    ...(process.env.PATH || "").split(win ? ";" : ":"),
    ...(win ? [
      path.join(process.env["ProgramFiles"] || "C:/Program Files", "LibreOffice/program"),
      path.join(process.env["ProgramFiles(x86)"] || "C:/Program Files (x86)", "LibreOffice/program"),
      path.join(process.env.LOCALAPPDATA || "", "Programs/LibreOffice/program"),
      // per-user 安装有两种落点，**两个都列** —— 只列一个会在另一种装法的机器上误报「没装」
      path.join(process.env.LOCALAPPDATA || "", "LibreOffice/program"),
    ] : [
      "/Applications/LibreOffice.app/Contents/MacOS", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin",
    ]),
  ].filter(Boolean);
  for (const d of dirs) for (const n of names) {
    const p = path.join(d, n);
    try { if (fs.statSync(p).isFile()) return p; } catch {}
  }
  return null;
}

/** 找 Chromium 系浏览器。**Edge 排第一**：Win10/11 出厂自带，客户机一定有。 */
function findChromium() {
  const win = process.platform === "win32";
  const cands = win ? [
    [process.env["ProgramFiles(x86)"], "Microsoft/Edge/Application/msedge.exe", "Edge"],
    [process.env["ProgramFiles"], "Microsoft/Edge/Application/msedge.exe", "Edge"],
    [process.env["ProgramFiles"], "Google/Chrome/Application/chrome.exe", "Chrome"],
    [process.env["ProgramFiles(x86)"], "Google/Chrome/Application/chrome.exe", "Chrome"],
    [process.env.LOCALAPPDATA, "Google/Chrome/Application/chrome.exe", "Chrome"],
  ] : [
    ["/Applications", "Google Chrome.app/Contents/MacOS/Google Chrome", "Chrome"],
    ["/Applications", "Microsoft Edge.app/Contents/MacOS/Microsoft Edge", "Edge"],
    ["/usr/bin", "chromium", "Chromium"], ["/usr/bin", "google-chrome", "Chrome"],
  ];
  for (const [base, rel, name] of cands) {
    if (!base) continue;
    const p = path.join(base, rel);
    try { if (fs.statSync(p).isFile()) return { path: p, name }; } catch {}
  }
  return null;
}

const soffice = findSoffice();
const chromium = findChromium();

if (args.check) {
  out({
    ok: !!(soffice || chromium),
    ready: !!(soffice || chromium),
    chromium: chromium ? { engine: `${chromium.name} (headless)`, path: chromium.path, handles: "md / html" } : null,
    libreoffice: soffice ? { path: soffice, handles: "docx / xlsx / pptx / doc / odt …" } : null,
    note: chromium && !soffice
      ? "Markdown/HTML 转 PDF 没问题；客户拿来的 Office 文件要保版式还得装 LibreOffice"
      : soffice && !chromium ? "Office 文件没问题；Markdown 走 LibreOffice 也能转" : null,
    how_to_fix: soffice ? undefined : FIX,
  }, (soffice || chromium) ? 0 : 1);
}

/** 极简 Markdown → HTML。够用就好：标题/加粗/斜体/行内码/列表/表格/引用/分隔线/链接。 */
function mdToHtml(md, title) {
  const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const inline = (s) => esc(s)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^*])\*([^*]+)\*/g, "$1<em>$2</em>")
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');
  const lines = String(md).replace(/\r\n/g, "\n").split("\n");
  const body = [];
  let list = null, inTable = false;
  const closeList = () => { if (list) { body.push(`</${list}>`); list = null; } };
  const closeTable = () => { if (inTable) { body.push("</tbody></table>"); inTable = false; } };
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    const h = /^(#{1,6})\s+(.*)$/.exec(l);
    const li = /^\s*[-*+]\s+(.*)$/.exec(l);
    const oli = /^\s*\d+[.)]\s+(.*)$/.exec(l);
    const row = /^\s*\|(.+)\|\s*$/.exec(l);
    if (h) { closeList(); closeTable(); body.push(`<h${h[1].length}>${inline(h[2])}</h${h[1].length}>`); continue; }
    if (/^\s*(---|\*\*\*|___)\s*$/.test(l)) { closeList(); closeTable(); body.push("<hr>"); continue; }
    if (row) {
      const cells = row[1].split("|").map((c) => c.trim());
      if (/^[\s:|-]+$/.test(row[1])) continue; // 分隔行
      if (!inTable) { closeList(); body.push("<table><tbody>"); inTable = true; body.push(`<tr>${cells.map((c) => `<th>${inline(c)}</th>`).join("")}</tr>`); continue; }
      body.push(`<tr>${cells.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`);
      continue;
    }
    closeTable();
    if (li || oli) {
      const want = li ? "ul" : "ol";
      if (list !== want) { closeList(); body.push(`<${want}>`); list = want; }
      body.push(`<li>${inline((li || oli)[1])}</li>`);
      continue;
    }
    closeList();
    if (/^\s*>\s?/.test(l)) { body.push(`<blockquote>${inline(l.replace(/^\s*>\s?/, ""))}</blockquote>`); continue; }
    if (l.trim()) body.push(`<p>${inline(l)}</p>`);
  }
  closeList(); closeTable();
  // 字体点名中文字族：Chromium headless 默认字体在部分客户机上会把中文渲染成方框
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>${esc(title)}</title><style>
@page{size:A4;margin:20mm 18mm}
body{font-family:"Microsoft YaHei","PingFang SC","Hiragino Sans GB",SimSun,sans-serif;font-size:11pt;line-height:1.75;color:#111}
h1{font-size:20pt;margin:0 0 14pt}h2{font-size:15pt;margin:18pt 0 8pt;border-bottom:1px solid #ddd;padding-bottom:4pt}
h3{font-size:12.5pt;margin:14pt 0 6pt}p{margin:6pt 0}li{margin:3pt 0}
table{border-collapse:collapse;width:100%;margin:10pt 0;font-size:10pt}
th,td{border:1px solid #bbb;padding:5pt 7pt;text-align:left}th{background:#f4f4f4;font-weight:600}
code{background:#f4f4f4;padding:1pt 4pt;border-radius:3px;font-family:Consolas,monospace;font-size:9.5pt}
blockquote{margin:8pt 0;padding:4pt 12pt;border-left:3px solid #ccc;color:#555}
hr{border:0;border-top:1px solid #ddd;margin:14pt 0}
</style></head><body>${body.join("\n")}</body></html>`;
}

const src = args._[0];
if (!src) out({ ok: false, error: "用法: node to-pdf.mjs <文件> [--out 输出.pdf] [--json]" }, 1);
const srcAbs = path.resolve(src);
if (!fs.existsSync(srcAbs)) out({ ok: false, error: `文件不存在: ${srcAbs}` }, 1);
const ext = path.extname(srcAbs).toLowerCase();
const WEB_EXT = [".md", ".markdown", ".html", ".htm"];
const OFFICE_EXT = [".docx", ".doc", ".odt", ".rtf", ".xlsx", ".xls", ".ods", ".csv", ".pptx", ".ppt", ".odp", ".txt"];
const OK_EXT = [...WEB_EXT, ...OFFICE_EXT];
if (!OK_EXT.includes(ext)) out({ ok: false, error: `不支持 ${ext}（支持: ${OK_EXT.join(" ")}）` }, 1);

// 引擎选择：md/html 走 Chromium（零安装），Office 文件走 LibreOffice（保版式）。
// `--engine chromium|libreoffice` 可强制。
const want = String(args.engine || "auto").toLowerCase();
const useChromium = want === "chromium" || (want === "auto" && WEB_EXT.includes(ext) && chromium);
if (useChromium && !chromium) out({ ok: false, error: "指定了 --engine chromium，但这台机器上找不到 Edge/Chrome" }, 1);
if (!useChromium && !soffice) {
  out({
    ok: false,
    error: WEB_EXT.includes(ext)
      ? "找不到 Edge/Chrome 也没装 LibreOffice，转不了 PDF"
      : `${ext} 是 Office 文件，要保住版式必须用 LibreOffice —— 这台机器没装。不会给你一份版式走样的替代品`,
    how_to_fix: WEB_EXT.includes(ext) ? FIX
      : `${FIX}\n（如果这份文件是 U-King 自己生成的，旁边通常有一份同源 .预览.html，直接转那个也行，版式一致且零安装）`,
  }, 1);
}

// ---------- 引擎 ①：Chromium headless ----------
if (useChromium) {
  const t0 = Date.now();
  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), "uking-pdf-"));
  let pageUrl;
  if (/\.(md|markdown)$/i.test(ext)) {
    const html = mdToHtml(fs.readFileSync(srcAbs, "utf8"), path.basename(srcAbs, ext));
    const htmlPath = path.join(workDir, "page.html");
    fs.writeFileSync(htmlPath, html, "utf8");
    pageUrl = "file:///" + htmlPath.replace(/\\/g, "/");
  } else {
    pageUrl = "file:///" + srcAbs.replace(/\\/g, "/");
  }
  const dest0 = path.resolve(String(args.out || srcAbs.replace(/\.[^.]+$/, "") + ".pdf"));
  fs.mkdirSync(path.dirname(dest0), { recursive: true });
  const r0 = spawnSync(chromium.path, [
    "--headless", "--disable-gpu", "--no-sandbox", "--no-pdf-header-footer",
    // 独立 user-data-dir：客户机上浏览器开着时，headless 会去复用已有实例然后什么都不打印
    `--user-data-dir=${path.join(workDir, "cud")}`,
    `--print-to-pdf=${dest0}`, pageUrl,
  ], { encoding: "utf8", timeout: Number(args.timeout || 120000), windowsHide: true });
  let size0 = 0;
  try { size0 = fs.statSync(dest0).size; } catch {}
  try { fs.rmSync(workDir, { recursive: true, force: true }); } catch {}
  if (!size0) {
    const why = (r0.stderr || r0.stdout || "").split(/\r?\n/).filter((l) => !/WSALookupServiceBegin|network_change_notifier/.test(l)).join(" ").trim().slice(0, 300);
    out({ ok: false, error: `${chromium.name} 没有产出 PDF${why ? "：" + why : ""}` }, 1);
  }
  if (size0 < 400) out({ ok: false, error: `产出的 PDF 只有 ${size0} 字节，基本是空的` }, 1);
  out({ ok: true, file: dest0, size: size0, engine: `${chromium.name} (headless)`, ms: Date.now() - t0 });
}

// soffice 只能指定输出**目录**、不能指定文件名，所以先转到临时目录再挪到目标位置。
// 顺带隔开：同目录直转会跟已存在的同名 pdf 打架。
const workDir = fs.mkdtempSync(path.join(os.tmpdir(), "uking-pdf-"));
const t0 = Date.now();
// -env:UserInstallation 给一个独立 profile：客户机上 LibreOffice 界面开着时，
// 第二个实例会直接退出（"another instance is running"），headless 转换随之失败。
const profile = "file:///" + path.join(workDir, "profile").replace(/\\/g, "/");
const r = spawnSync(soffice, [
  `-env:UserInstallation=${profile}`,
  "--headless", "--norestore", "--nolockcheck", "--nodefault",
  "--convert-to", "pdf", "--outdir", workDir, srcAbs,
], { encoding: "utf8", timeout: Number(args.timeout || 180000), windowsHide: true });

const produced = (() => {
  try { return fs.readdirSync(workDir).filter((f) => f.toLowerCase().endsWith(".pdf")); } catch { return []; }
})();
// 🔴 soffice 转换失败时**照样退出码 0**（officedoc.rs 里踩过同一个坑）。
// 唯一可信的判据是「文件真的出来了且非空」。
if (!produced.length) {
  const why = (r.stderr || r.stdout || "").trim().slice(0, 300);
  try { fs.rmSync(workDir, { recursive: true, force: true }); } catch {}
  out({ ok: false, error: `LibreOffice 没有产出 PDF${why ? "：" + why : "（退出码 0 但没有文件——它失败时也返回 0）"}` }, 1);
}
const tmpPdf = path.join(workDir, produced[0]);
const dest = path.resolve(String(args.out || srcAbs.replace(/\.[^.]+$/, "") + ".pdf"));
try {
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.copyFileSync(tmpPdf, dest);
} catch (e) {
  try { fs.rmSync(workDir, { recursive: true, force: true }); } catch {}
  out({ ok: false, error: "写目标文件失败: " + e.message }, 1);
}
const size = fs.statSync(dest).size;
try { fs.rmSync(workDir, { recursive: true, force: true }); } catch {}
if (size < 400) out({ ok: false, error: `产出的 PDF 只有 ${size} 字节，基本是空的 —— 源文件可能损坏或加了密` }, 1);

const ver = spawnSync(soffice, ["--version"], { encoding: "utf8", timeout: 30000, windowsHide: true });
out({ ok: true, file: dest, size, engine: (ver.stdout || "LibreOffice").trim().split(/\r?\n/)[0], ms: Date.now() - t0 });
