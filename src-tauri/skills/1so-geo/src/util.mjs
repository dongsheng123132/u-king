// 一搜商答 / 1so —— 公共工具（零依赖，仅 node 内置）。
// 遵循 ai-cli-design：stdout 只出结果，日志/进度走 stderr，退出码 0/1/2。
import { readFileSync, writeFileSync, mkdirSync, existsSync, readdirSync, statSync } from "node:fs";
import { join, extname, basename } from "node:path";

// ── 参数解析：--flag value；--json/--quiet/--no-preview 为布尔 ──
const BOOL = new Set(["json", "quiet", "no-preview", "auto", "merge", "render", "help", "h"]);
export function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      const k = a.slice(2);
      if (BOOL.has(k)) { out[k] = true; continue; }
      const v = i + 1 < argv.length && !argv[i + 1].startsWith("--") ? argv[++i] : true;
      out[k] = v;
    } else if (a === "-h") {
      out.help = true;
    } else {
      out._.push(a);
    }
  }
  return out;
}

// ── 输出：QUIET 只压制进度日志（stderr），不影响结果（stdout）──
let QUIET = false;
export function setQuiet(q) { QUIET = !!q; }
export function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
export function warn(...m) { process.stderr.write("⚠ " + m.join(" ") + "\n"); }

// ── JSON 宽松解析：剥掉 ```json 围栏 / 前后杂文，取第一个完整 { } 或 [ ] ──
export function parseJsonLoose(text) {
  if (text == null) throw new Error("空响应");
  let s = String(text).trim();
  s = s.replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/i, "");
  // 直接能解析就返回
  try { return JSON.parse(s); } catch {}
  // 否则从第一个 { 或 [ 找到匹配的收尾
  const start = s.search(/[[{]/);
  if (start < 0) throw new Error("响应里找不到 JSON：" + s.slice(0, 160));
  const open = s[start], close = open === "{" ? "}" : "]";
  let depth = 0, inStr = false, esc = false;
  for (let i = start; i < s.length; i++) {
    const c = s[i];
    if (inStr) { if (esc) esc = false; else if (c === "\\") esc = true; else if (c === '"') inStr = false; continue; }
    if (c === '"') inStr = true;
    else if (c === open) depth++;
    else if (c === close) { depth--; if (depth === 0) { return JSON.parse(s.slice(start, i + 1)); } }
  }
  throw new Error("JSON 未闭合：" + s.slice(0, 160));
}

// ── 文件系统小工具 ──
export function ensureDir(d) { if (!existsSync(d)) mkdirSync(d, { recursive: true }); return d; }
export function readJson(p, fallback = undefined) {
  try { return JSON.parse(readFileSync(p, "utf8")); }
  catch (e) { if (fallback !== undefined) return fallback; throw e; }
}
export function writeJson(p, obj) { ensureDir(dirOf(p)); writeFileSync(p, JSON.stringify(obj, null, 2)); return p; }
export function writeText(p, txt) { ensureDir(dirOf(p)); writeFileSync(p, txt); return p; }
function dirOf(p) { const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")); return i < 0 ? "." : p.slice(0, i); }

// 读取一个目录里所有“文本类”资料（.txt/.md/.markdown/.json/.csv），返回 [{name, text}]。
// 非文本（pdf/docx/图片/视频）不静默跳过——返回到 skipped 里让上层告知用户。
const TEXT_EXT = new Set([".txt", ".md", ".markdown", ".json", ".csv", ".log", ".html", ".htm"]);
export function readMaterials(dir) {
  const files = [], skipped = [];
  if (!existsSync(dir)) return { files, skipped, missing: true };
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    let st; try { st = statSync(full); } catch { continue; }
    if (st.isDirectory()) continue;
    const ext = extname(name).toLowerCase();
    if (TEXT_EXT.has(ext)) {
      try { files.push({ name, text: readFileSync(full, "utf8") }); } catch { skipped.push(name + "(读取失败)"); }
    } else {
      skipped.push(name);
    }
  }
  return { files, skipped, missing: false };
}

// 标准结果输出：--json 出契约 JSON；否则出人读文本。错误走 stderr + 退出码。
export function done(jsonMode, obj, humanLine = "", code = 0) {
  if (jsonMode) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (humanLine) process.stdout.write(humanLine + "\n");
  process.exit(code);
}
export function fail(jsonMode, msg, code = 1) {
  const m = String((msg && msg.message) || msg);
  if (jsonMode) process.stdout.write(JSON.stringify({ ok: false, error: m }) + "\n");
  else process.stderr.write("错误：" + m + "\n");
  process.exit(code);
}

export function slug(s) {
  return String(s || "").trim().replace(/[\s\/\\]+/g, "-").replace(/[<>:"|?*]+/g, "").slice(0, 60) || "page";
}
export function today() {
  // 允许注入（测试可控）；默认取系统日期
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}
export function esc(s) {
  return String(s ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
