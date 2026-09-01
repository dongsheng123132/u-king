#!/usr/bin/env node
/**
 * gen-dxf.mjs —— 纯 std（零 npm 依赖）出**真 .dxf**（AutoCAD / 浩辰 / 中望 / CAD看图王 / LibreCAD 能开）。
 *
 * 为什么手搓：客户机只有便携 Node，装不了 ezdxf（要 Python + pip）。DXF R12(AC1009) 是
 * 纯文本格式、所有 CAD 都认，手写反而最稳。
 *
 * ★ 默认**同时出一张同源预览 SVG**（`<out>.预览.svg`）—— 客户机上九成没装 CAD，
 *   只给 .dxf 等于「做完了但看不见」。预览和 dxf 由同一份 spec 渲染，不会两张图对不上。
 *
 * 用法：
 *   node gen-dxf.mjs --in 图.json --out 图纸.dxf --json
 *   node gen-dxf.mjs --in 图.json --out 图纸.dxf --no-preview      # 只要 dxf
 *   node gen-dxf.mjs --in 图.json --out 图纸.dxf --encoding utf8   # 中文按原文写（见下）
 *
 * spec（--in 的 JSON）：
 * {
 *   "title": "示例",
 *   "layers": [{"name":"轮廓","color":7}, {"name":"标注","color":1}],
 *   "entities": [
 *     {"type":"line",     "layer":"轮廓", "from":[0,0], "to":[100,0]},
 *     {"type":"rect",     "layer":"轮廓", "at":[0,0], "w":100, "h":60},
 *     {"type":"circle",   "layer":"轮廓", "center":[50,30], "r":10},
 *     {"type":"arc",      "layer":"轮廓", "center":[0,0], "r":20, "start":0, "end":90},
 *     {"type":"polyline", "layer":"轮廓", "points":[[0,0],[10,0],[10,10]], "closed":true},
 *     {"type":"text",     "layer":"标注", "at":[10,10], "text":"客厅", "height":5, "rotation":0},
 *     {"type":"dim",      "layer":"标注", "from":[0,0], "to":[100,0], "offset":-8, "text":"100"}
 *   ]
 * }
 *
 * 坐标单位就是 CAD 图形单位（画平面图时按毫米记，1 单位 = 1mm 最省事）。
 * Y 轴向上（数学坐标系），跟 CAD 一致。
 *
 * ⚠ 中文：DXF R12 本体是 ASCII 年代的格式。默认 `--encoding escape` 把非 ASCII 字符写成
 * `\U+XXXX`（AutoCAD/ODA 系读得对，文件全 ASCII 不会乱码）；若你的看图软件不认，
 * 改用 `--encoding utf8` 写原文（多数现代看图软件也能读）。预览 SVG 永远是 UTF-8 原文。
 *
 * 输出：`{"ok":true,"file":"…dxf","preview":"…svg","entities":N}`（--json）。
 */
import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const a = {};
  for (let i = 0; i < argv.length; i++) {
    const t = argv[i];
    if (t.startsWith("--")) { const k = t.slice(2); a[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true; }
  }
  return a;
}
const args = parseArgs(process.argv.slice(2));
const asJson = !!args.json;
function fail(m) { if (asJson) console.log(JSON.stringify({ ok: false, error: String(m) })); else console.error("[gen-dxf] 失败:", m); process.exit(1); }

// ---------- 读 spec ----------
let spec;
try {
  if (args.in) spec = JSON.parse(fs.readFileSync(String(args.in), "utf8"));
  else fail("需要 --in <spec.json>");
} catch (e) { fail("读 spec 失败: " + e.message); }
if (!spec || !Array.isArray(spec.entities) || !spec.entities.length) fail("spec.entities 为空——没有要画的东西");
const out = String(args.out || "图纸.dxf");
// 文件**永远按 UTF-8 写**。曾经默认 latin1 写盘想让文件保持纯 ASCII，结果中文图层名
// 被逐字节截断成垃圾——文字实体转义得再干净，图层名一样会毁掉整个文件。
// `escape` 只决定「文字实体的内容要不要写成 \U+XXXX」，跟文件编码是两件事。
const encMode = String(args.encoding || "utf8");

// ---------- 把高层图元（rect / dim）摊平成 CAD 基本图元 ----------
// 🔴 尺寸标注的字高/箭头**必须跟图幅成比例**，不能写死。
// 早先固定 3.5 / 2.5 图形单位：在一张 12000mm 的平面图上那是一个看不见的点 ——
// 文件、图层、实体数全对，判分也过，客户打开一看「尺寸呢？」。
// 按 1:100 出图的习惯反推：3.5mm 打印字高 ≈ 图形单位 len/40，箭头取字高的 0.7。
// 显式给了 `height` 就听调用方的。
const dimTextHeight = (e, len) => +e.height || Math.max(len / 40, 0.5);
function flatten(list) {
  const flat = [];
  for (const e of list) {
    const L = e.layer || "0";
    switch (String(e.type || "").toLowerCase()) {
      case "rect": {
        const [x, y] = e.at || [0, 0], w = +e.w || 0, h = +e.h || 0;
        flat.push({ type: "polyline", layer: L, closed: true, points: [[x, y], [x + w, y], [x + w, y + h], [x, y + h]] });
        break;
      }
      case "dim": {
        // 「穷人版尺寸标注」：R12 的 DIMENSION 实体要配 BLOCK 才显示，各家实现还不一致。
        // 直接画成 尺寸线 + 两条界线 + 箭头 + 文字 —— 所有看图软件显示一致，也能被量取。
        const [x1, y1] = e.from, [x2, y2] = e.to;
        const off = e.offset === undefined ? -8 : +e.offset;
        const dx = x2 - x1, dy = y2 - y1, len = Math.hypot(dx, dy) || 1;
        const nx = -dy / len, ny = dx / len;            // 法向
        const ax = x1 + nx * off, ay = y1 + ny * off;
        const bx = x2 + nx * off, by = y2 + ny * off;
        flat.push({ type: "line", layer: L, from: [x1, y1], to: [ax, ay] });
        flat.push({ type: "line", layer: L, from: [x2, y2], to: [bx, by] });
        flat.push({ type: "line", layer: L, from: [ax, ay], to: [bx, by] });
        const h = dimTextHeight(e, len);
        const arrow = h * 0.7;
        const ux = dx / len, uy = dy / len;
        for (const [px, py, s] of [[ax, ay, 1], [bx, by, -1]]) {
          flat.push({ type: "polyline", layer: L, closed: true, points: [
            [px, py],
            [px + ux * arrow * s + nx * arrow * 0.3, py + uy * arrow * s + ny * arrow * 0.3],
            [px + ux * arrow * s - nx * arrow * 0.3, py + uy * arrow * s - ny * arrow * 0.3],
          ] });
        }
        const label = e.text !== undefined ? String(e.text) : String(Math.round(len * 100) / 100);
        const rot = Math.atan2(dy, dx) * 180 / Math.PI;
        flat.push({ type: "text", layer: L, at: [(ax + bx) / 2, (ay + by) / 2 + h * 0.4], text: label, height: h, rotation: rot, align: "center" });
        break;
      }
      default: flat.push({ ...e, layer: L });
    }
  }
  return flat;
}
const ents = flatten(spec.entities);

// ---------- 包围盒（写进 HEADER 的 $EXTMIN/$EXTMAX，CAD 打开自动缩放到图） ----------
let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
const grow = (x, y) => { if (x < minX) minX = x; if (y < minY) minY = y; if (x > maxX) maxX = x; if (y > maxY) maxY = y; };
for (const e of ents) {
  const t = String(e.type).toLowerCase();
  if (t === "line") { grow(...e.from); grow(...e.to); }
  else if (t === "circle") { const [cx, cy] = e.center, r = +e.r; grow(cx - r, cy - r); grow(cx + r, cy + r); }
  else if (t === "arc") { const [cx, cy] = e.center, r = +e.r; grow(cx - r, cy - r); grow(cx + r, cy + r); }
  else if (t === "polyline") for (const p of e.points) grow(p[0], p[1]);
  else if (t === "text") { const [x, y] = e.at, h = +e.height || 3.5; grow(x, y); grow(x + String(e.text).length * h * 0.7, y + h); }
}
if (!isFinite(minX)) fail("算不出图形范围——entities 里没有可识别的图元");

// ---------- DXF 拼装 ----------
const buf = [];
const g = (code, val) => { buf.push(String(code)); buf.push(String(val)); };
const num = (v) => (Math.round((+v) * 1e6) / 1e6).toFixed(6);

/** 非 ASCII → `\U+XXXX`（AutoCAD 文本解析器认这个转义，文件保持纯 ASCII 不会乱码）。 */
function dxfText(s) {
  const str = String(s);
  if (encMode === "utf8") return str;
  let o = "";
  for (const ch of str) {
    const cp = ch.codePointAt(0);
    o += cp < 128 ? ch : "\\U+" + cp.toString(16).toUpperCase().padStart(4, "0");
  }
  return o;
}

// 图层表：spec 里声明的 + entities 里用到但没声明的（自动补，颜色默认 7=白/黑）
const layerMap = new Map([["0", 7]]);
for (const l of spec.layers || []) layerMap.set(String(l.name), +l.color || 7);
for (const e of ents) if (!layerMap.has(String(e.layer))) layerMap.set(String(e.layer), 7);

g(0, "SECTION"); g(2, "HEADER");
g(9, "$ACADVER"); g(1, "AC1009");
g(9, "$INSBASE"); g(10, num(0)); g(20, num(0)); g(30, num(0));
g(9, "$EXTMIN"); g(10, num(minX)); g(20, num(minY)); g(30, num(0));
g(9, "$EXTMAX"); g(10, num(maxX)); g(20, num(maxY)); g(30, num(0));
g(0, "ENDSEC");

g(0, "SECTION"); g(2, "TABLES");
// LTYPE：CONTINUOUS 是必备项，缺了部分看图软件直接报文件损坏
g(0, "TABLE"); g(2, "LTYPE"); g(70, 1);
g(0, "LTYPE"); g(2, "CONTINUOUS"); g(70, 0); g(3, "Solid line"); g(72, 65); g(73, 0); g(40, num(0));
g(0, "ENDTAB");
// STYLE：TEXT 实体的 7 码指向它，缺了文字可能不显示
g(0, "TABLE"); g(2, "STYLE"); g(70, 1);
g(0, "STYLE"); g(2, "STANDARD"); g(70, 0); g(40, num(0)); g(41, num(1)); g(50, num(0));
g(71, 0); g(42, num(2.5)); g(3, "txt"); g(4, "");
g(0, "ENDTAB");
g(0, "TABLE"); g(2, "LAYER"); g(70, layerMap.size);
for (const [name, color] of layerMap) { g(0, "LAYER"); g(2, name); g(70, 0); g(62, color); g(6, "CONTINUOUS"); }
g(0, "ENDTAB");
g(0, "ENDSEC");

g(0, "SECTION"); g(2, "ENTITIES");
let drawn = 0;
for (const e of ents) {
  const L = String(e.layer);
  switch (String(e.type).toLowerCase()) {
    case "line":
      g(0, "LINE"); g(8, L);
      g(10, num(e.from[0])); g(20, num(e.from[1])); g(30, num(0));
      g(11, num(e.to[0])); g(21, num(e.to[1])); g(31, num(0));
      drawn++; break;
    case "circle":
      g(0, "CIRCLE"); g(8, L);
      g(10, num(e.center[0])); g(20, num(e.center[1])); g(30, num(0)); g(40, num(e.r));
      drawn++; break;
    case "arc":
      g(0, "ARC"); g(8, L);
      g(10, num(e.center[0])); g(20, num(e.center[1])); g(30, num(0)); g(40, num(e.r));
      g(50, num(e.start)); g(51, num(e.end));
      drawn++; break;
    case "polyline": {
      // R12 没有 LWPOLYLINE，必须 POLYLINE + VERTEX + SEQEND
      g(0, "POLYLINE"); g(8, L); g(66, 1);
      g(10, num(0)); g(20, num(0)); g(30, num(0)); g(70, e.closed ? 1 : 0);
      for (const p of e.points) { g(0, "VERTEX"); g(8, L); g(10, num(p[0])); g(20, num(p[1])); g(30, num(0)); }
      g(0, "SEQEND"); g(8, L);
      drawn++; break;
    }
    case "text": {
      const h = +e.height || 3.5;
      const just = e.align === "center" ? 1 : e.align === "right" ? 2 : 0;
      g(0, "TEXT"); g(8, L);
      g(10, num(e.at[0])); g(20, num(e.at[1])); g(30, num(0));
      g(40, num(h)); g(1, dxfText(e.text)); g(50, num(e.rotation || 0)); g(7, "STANDARD");
      if (just) { g(72, just); g(11, num(e.at[0])); g(21, num(e.at[1])); g(31, num(0)); }
      drawn++; break;
    }
    default: break; // 不认识的类型静默跳过，不让一个错图元废掉整张图
  }
}
g(0, "ENDSEC");
g(0, "EOF");

const warnings = [];
if (encMode === "escape") {
  const bad = [...layerMap.keys()].filter((n) => /[^\x00-\x7F]/.test(n));
  if (bad.length) warnings.push(`--encoding escape 只能转义文字实体；图层名 ${bad.join("/")} 含中文，老版 AutoCAD 里可能显示为乱码（图形本身不受影响）。要么用默认 utf8，要么把图层名改成英文。`);
}
try {
  fs.mkdirSync(path.dirname(path.resolve(out)), { recursive: true });
  fs.writeFileSync(out, buf.join("\r\n") + "\r\n", "utf8");
} catch (e) { fail("写 dxf 失败: " + e.message); }

// ---------- 同源预览 SVG ----------
let previewPath = null;
if (!args["no-preview"]) {
  const pad = Math.max((maxX - minX), (maxY - minY)) * 0.06 + 5;
  const x0 = minX - pad, y0 = minY - pad, W = (maxX - minX) + pad * 2, H = (maxY - minY) + pad * 2;
  const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const CI = { 1: "#e11d48", 2: "#eab308", 3: "#22c55e", 4: "#06b6d4", 5: "#3b82f6", 6: "#d946ef", 7: "#111827" };
  const col = (L) => CI[layerMap.get(String(L))] || "#111827";
  const Y = (y) => (y0 + H) - (y - y0) + y0; // DXF 的 Y 向上 → SVG 向下
  const s = [];
  s.push(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="${num(x0)} ${num(y0)} ${num(W)} ${num(H)}" width="1100" style="background:#fff">`);
  s.push(`<g fill="none" stroke-width="${num(Math.max(W, H) / 700)}" vector-effect="non-scaling-stroke">`);
  for (const e of ents) {
    const c = col(e.layer);
    switch (String(e.type).toLowerCase()) {
      case "line":
        s.push(`<line x1="${num(e.from[0])}" y1="${num(Y(e.from[1]))}" x2="${num(e.to[0])}" y2="${num(Y(e.to[1]))}" stroke="${c}"/>`); break;
      case "circle":
        s.push(`<circle cx="${num(e.center[0])}" cy="${num(Y(e.center[1]))}" r="${num(e.r)}" stroke="${c}"/>`); break;
      case "arc": {
        const [cx, cy] = e.center, r = +e.r;
        const a0 = (+e.start) * Math.PI / 180, a1 = (+e.end) * Math.PI / 180;
        const sweep = ((+e.end) - (+e.start) + 360) % 360;
        const p0 = [cx + r * Math.cos(a0), Y(cy + r * Math.sin(a0))];
        const p1 = [cx + r * Math.cos(a1), Y(cy + r * Math.sin(a1))];
        s.push(`<path d="M ${num(p0[0])} ${num(p0[1])} A ${num(r)} ${num(r)} 0 ${sweep > 180 ? 1 : 0} 0 ${num(p1[0])} ${num(p1[1])}" stroke="${c}"/>`); break;
      }
      case "polyline":
        s.push(`<poly${e.closed ? "gon" : "line"} points="${e.points.map((p) => `${num(p[0])},${num(Y(p[1]))}`).join(" ")}" stroke="${c}" fill="none"/>`); break;
      case "text": {
        const h = +e.height || 3.5;
        const anchor = e.align === "center" ? "middle" : e.align === "right" ? "end" : "start";
        const rot = -(+e.rotation || 0);
        s.push(`<text x="${num(e.at[0])}" y="${num(Y(e.at[1]))}" font-size="${num(h)}" fill="${c}" stroke="none" text-anchor="${anchor}" font-family="Microsoft YaHei,SimSun,sans-serif" transform="rotate(${num(rot)} ${num(e.at[0])} ${num(Y(e.at[1]))})">${esc(e.text)}</text>`); break;
      }
    }
  }
  s.push("</g></svg>");
  previewPath = out.replace(/\.dxf$/i, "") + ".预览.svg";
  try { fs.writeFileSync(previewPath, s.join("\n"), "utf8"); } catch { previewPath = null; }
}

const abs = path.resolve(out);
if (asJson) console.log(JSON.stringify({ ok: true, file: abs, preview: previewPath ? path.resolve(previewPath) : null, entities: drawn, bbox: [minX, minY, maxX, maxY], warnings }));
else { console.log(abs); if (previewPath) console.log(path.resolve(previewPath)); }
