#!/usr/bin/env node
/**
 * gen-pptx.mjs —— 纯 std（零 npm 依赖）把结构化大纲生成**有设计感的真 .pptx**。
 *
 * 客户机只有便携 Node、装不了 pptxgenjs，所以手搓 OOXML(PresentationML) + ZIP(STORE)。
 * 不含任何 Key、不联网。内置**主题配色 + 版式系统**（封面/章节/内容/图文），出片专业不寒酸。
 *
 * 用法（AI 经 run_command 调；大纲用 --in 传文件避免 shell 转义大 JSON）：
 *   node gen-pptx.mjs --in deck.json --out 演示.pptx --json
 *
 * deck.json 结构：
 *   {
 *     "title": "整体标题(可选)",
 *     "accent": "4F46E5",            // 主题色 hex(可选;也可用命名: indigo/teal/rose/amber/emerald/slate)
 *     "slides": [
 *       { "type":"cover",   "title":"主标题", "subtitle":"副标题", "footer":"2026 · 团队" },
 *       { "type":"section", "title":"第一部分：背景", "number":"01" },
 *       { "type":"content", "title":"页标题", "bullets":["要点1","要点2","要点3"] },
 *       { "type":"content", "title":"图文页", "bullets":["左边文字"], "image":"hero.png" },
 *       { "type":"quote",   "text":"一句金句/结论", "by":"—— 出处" },
 *       { "type":"end",     "title":"谢谢观看", "subtitle":"Q & A" }
 *     ]
 *   }
 * type 可省略：第 0 页默认 cover；有 bullets=content；只有 subtitle=section；末页写 end。
 * 配图先用 generate_image / uking-aigc 出图，把绝对路径(Windows 风格 C:/…)填进 image。
 *
 * 输出：成功打印 .pptx 绝对路径；--json 时打印 {"ok":true,"file":"..."}。
 */
import fs from "node:fs";
import path from "node:path";

// ---------- CLI ----------
function parseArgs(argv) { const a = {}; for (let i = 0; i < argv.length; i++) { const t = argv[i]; if (t.startsWith("--")) { const k = t.slice(2); a[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true; } } return a; }
const args = parseArgs(process.argv.slice(2));
const asJson = !!args.json;
function fail(m) { if (asJson) console.log(JSON.stringify({ ok: false, error: String(m) })); else console.error("[gen-pptx] 失败:", m); process.exit(1); }

let deck;
try {
  if (args.in) deck = JSON.parse(fs.readFileSync(String(args.in), "utf8"));
  else if (args.slides) deck = JSON.parse(String(args.slides));
  else fail("需要 --in <deck.json> 或 --slides '<json>'");
} catch (e) { fail("解析大纲 JSON 失败: " + e.message); }
// 宽容归一：AI 可能直接写成数组、或用 pages/list 键，都当 slides 处理
if (Array.isArray(deck)) deck = { slides: deck };
else if (deck && !Array.isArray(deck.slides)) deck.slides = deck.slides || deck.pages || deck.list;
if (!deck || !Array.isArray(deck.slides) || !deck.slides.length) fail("大纲里没有 slides（deck.json 需是 {slides:[…]} 或直接是一个数组）");
if (args.title) deck.title = args.title;
// 页脚标题：deck.title 缺省时取封面/首页标题，别露出「演示文稿」占位
const deckTitle = deck.title || (deck.slides.find((s) => s && (s.type === "cover" || !s.type)) || {}).title || (deck.slides[0] || {}).title || "演示文稿";

// ---------- 主题配色 ----------
const NAMED = { indigo: "4F46E5", teal: "0D9488", rose: "E11D48", amber: "D97706", emerald: "059669", slate: "334155", blue: "2563EB", violet: "7C3AED" };
// 主题色：--accent 命令行 > deck.accent；支持命名或 hex，默认 indigo
const accentIn = (typeof args.accent === "string" ? args.accent : "") || deck.accent || "";
const accent = (NAMED[String(accentIn).toLowerCase()] || (/^#?[0-9a-fA-F]{6}$/.test(String(accentIn)) ? String(accentIn).replace("#", "") : "4F46E5")).toUpperCase();
// 由 accent 派生一个更深的色（章节/封面背景用），简单按分量 ×0.7
const darken = (hex, f = 0.72) => hex.match(/../g).map((h) => Math.round(parseInt(h, 16) * f).toString(16).padStart(2, "0")).join("").toUpperCase();
const accentDark = darken(accent);
const C = { title: "1F2937", body: "374151", muted: "6B7280", faint: "9CA3AF", white: "FFFFFF", subOnAccent: "E5E7EB", footRule: "E5E7EB", cardBg: "F8FAFC" };
const FONT_EA = "Microsoft YaHei"; // 中文字体，好看不发虚
const FONT_LAT = "Segoe UI";

// ---------- XML 转义 ----------
// 🔴 先剥 XML 1.0 非法控制字符，再转义。转义只管 `& < > "`，管不了 0x0B/0x1F 这类字节 ——
// 它们没有实体写法，进了 slide1.xml 就是非法 XML，PowerPoint 直接报「文件已损坏」，
// 而脚本照样返回 `ok:true`。OCR 结果 / 从 PDF 复制的文本里很常见。
// （XML 1.0 只准 \t \n \r 这三个 C0 字符）
const stripCtl = (s) => s.replace(/[\x00-\x08\x0B\x0C\x0E-\x1F]/g, "");
const esc = (s) => stripCtl(String(s == null ? "" : s)).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// ---------- CRC32 + ZIP(STORE) ----------
const CRC = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
function crc32(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = CRC[(c ^ b[i]) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; }
function zipStore(entries) {
  const parts = [], central = []; let off = 0;
  for (const e of entries) {
    const nb = Buffer.from(e.name, "utf8"); const d = Buffer.isBuffer(e.data) ? e.data : Buffer.from(e.data, "utf8"); const crc = crc32(d);
    const lh = Buffer.alloc(30);
    lh.writeUInt32LE(0x04034b50, 0); lh.writeUInt16LE(20, 4); lh.writeUInt16LE(0, 6); lh.writeUInt16LE(0, 8); lh.writeUInt16LE(0, 10); lh.writeUInt16LE(0x21, 12);
    lh.writeUInt32LE(crc, 14); lh.writeUInt32LE(d.length, 18); lh.writeUInt32LE(d.length, 22); lh.writeUInt16LE(nb.length, 26); lh.writeUInt16LE(0, 28);
    parts.push(lh, nb, d);
    const ch = Buffer.alloc(46);
    ch.writeUInt32LE(0x02014b50, 0); ch.writeUInt16LE(20, 4); ch.writeUInt16LE(20, 6); ch.writeUInt16LE(0, 8); ch.writeUInt16LE(0, 10); ch.writeUInt16LE(0, 12); ch.writeUInt16LE(0x21, 14);
    ch.writeUInt32LE(crc, 16); ch.writeUInt32LE(d.length, 20); ch.writeUInt32LE(d.length, 24); ch.writeUInt16LE(nb.length, 28);
    ch.writeUInt16LE(0, 30); ch.writeUInt16LE(0, 32); ch.writeUInt16LE(0, 34); ch.writeUInt16LE(0, 36); ch.writeUInt32LE(0, 38); ch.writeUInt32LE(off, 42);
    central.push(ch, nb); off += 30 + nb.length + d.length;
  }
  const cb = Buffer.concat(central); const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0); eocd.writeUInt16LE(0, 4); eocd.writeUInt16LE(0, 6); eocd.writeUInt16LE(entries.length, 8); eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(cb.length, 12); eocd.writeUInt32LE(off, 16); eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...parts, cb, eocd]);
}

// ---------- 图片尺寸(PNG/JPEG) ----------
function imageSize(buf) {
  if (buf.length > 24 && buf[0] === 0x89 && buf[1] === 0x50) return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
  if (buf[0] === 0xff && buf[1] === 0xd8) { let o = 2; while (o < buf.length) { if (buf[o] !== 0xff) { o++; continue; } const mk = buf[o + 1]; if (mk >= 0xc0 && mk <= 0xcf && mk !== 0xc4 && mk !== 0xc8 && mk !== 0xcc) return { h: buf.readUInt16BE(o + 5), w: buf.readUInt16BE(o + 7) }; o += 2 + buf.readUInt16BE(o + 2); } }
  return null;
}

// ---------- 尺寸(EMU: 914400/in；16:9) ----------
const W = 12192000, H = 6858000, MX = 838200; // 左右边距 0.9in

const NS = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"';
const EMPTY_GROUP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';

// 一个纯色矩形（背景条/色块/分隔线）
function rect(id, x, y, cx, cy, fill, opts = {}) {
  const round = opts.round ? '<a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 8000"/></a:avLst></a:prstGeom>' : '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>';
  return `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="r${id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>` +
    `<p:spPr><a:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm>${round}` +
    `<a:solidFill><a:srgbClr val="${fill}"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr>` +
    `<p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>`;
}
// 文本框；paras: [{runs:[{t,sz,b,color}], bullet, bulletClr, algn, spcBef, lnPct}]
function textBox(id, name, x, y, cx, cy, paras, anchor = "t") {
  const body = paras.map((p) => {
    const pprBits = [];
    if (p.lnPct) pprBits.push(`<a:lnSpc><a:spcPct val="${p.lnPct}"/></a:lnSpc>`);
    if (p.spcBef) pprBits.push(`<a:spcBef><a:spcPts val="${p.spcBef}"/></a:spcBef>`);
    if (p.bullet) { pprBits.push(`<a:buClr><a:srgbClr val="${p.bulletClr || accent}"/></a:buClr><a:buFont typeface="Arial"/><a:buChar char="▪"/>`); }
    else pprBits.push('<a:buNone/>');
    const pPr = `<a:pPr${p.bullet ? ' marL="274320" indent="-274320"' : ""}${p.algn ? ` algn="${p.algn}"` : ""}>${pprBits.join("")}</a:pPr>`;
    const runs = (p.runs || []).map((r) => {
      const rp = [`lang="zh-CN"`, `sz="${r.sz || 1800}"`, `dirty="0"`];
      if (r.b) rp.push('b="1"');
      const fill = r.color ? `<a:solidFill><a:srgbClr val="${r.color}"/></a:solidFill>` : "";
      return `<a:r><a:rPr ${rp.join(" ")}>${fill}<a:latin typeface="${FONT_LAT}"/><a:ea typeface="${FONT_EA}"/></a:rPr><a:t>${esc(r.t)}</a:t></a:r>`;
    }).join("");
    return `<a:p>${pPr}${runs || '<a:endParaRPr lang="zh-CN"/>'}</a:p>`;
  }).join("");
  return `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="${esc(name)}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>` +
    `<p:spPr><a:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>` +
    `<p:txBody><a:bodyPr wrap="square" anchor="${anchor}"><a:normAutofit/></a:bodyPr><a:lstStyle/>${body}</p:txBody></p:sp>`;
}
function picBox(id, rEmbed, x, y, cx, cy) {
  return `<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="img${id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>` +
    `<p:blipFill><a:blip r:embed="${rEmbed}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>` +
    `<p:spPr><a:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 4000"/></a:avLst></a:prstGeom></p:spPr></p:pic>`;
}

// ---------- 组装 slides ----------
const media = [], slideXmls = [], slideRels = [];
let uid = 100;
function loadImage(spec, idx) {
  try {
    const data = fs.readFileSync(String(spec));
    const ext = (path.extname(String(spec)).slice(1) || "png").toLowerCase();
    const e = ext === "jpg" ? "jpeg" : ext;
    const name = `image${media.length + 1}.${e}`;
    media.push({ name, data, ext: e });
    const rid = `rIdImg${idx + 1}_${media.length}`;
    (slideRels[idx] = slideRels[idx] || []).push({ id: rid, type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image", target: `../media/${name}` });
    return { rid, dim: imageSize(data) };
  } catch { return null; }
}
// 按框 fit（保比例居中）
function fit(img, bx, by, bcx, bcy) {
  let cx = bcx, cy = bcy, x = bx, y = by;
  if (img.dim && img.dim.w > 0 && img.dim.h > 0) { const s = Math.min(bcx / img.dim.w, bcy / img.dim.h); cx = Math.round(img.dim.w * s); cy = Math.round(img.dim.h * s); x = bx + Math.round((bcx - cx) / 2); y = by + Math.round((bcy - cy) / 2); }
  return { x, y, cx, cy };
}

deck.slides.forEach((s, idx) => {
  const type = s.type || (idx === 0 ? "cover" : (s.bullets && s.bullets.length) ? "content" : s.text ? "quote" : s.subtitle ? "section" : "content");
  let bg = null; // <p:bg> 颜色
  const sh = [];
  const total = deck.slides.length;

  if (type === "cover") {
    bg = accentDark;
    sh.push(rect(uid++, 0, H - 220000, W, 220000, accent)); // 底部亮色条
    sh.push(rect(uid++, MX, 2540000, 900000, 70000, accent)); // 标题上的短色条
    sh.push(textBox(uid++, "Title", MX, 2680000, W - 2 * MX, 1500000, [{ runs: [{ t: s.title || deckTitle, sz: 5400, b: true, color: C.white }], algn: "l", lnPct: 105000 }], "t"));
    if (s.subtitle) sh.push(textBox(uid++, "Sub", MX, 4260000, W - 2 * MX, 800000, [{ runs: [{ t: s.subtitle, sz: 2200, color: C.subOnAccent }], algn: "l" }]));
    if (s.footer) sh.push(textBox(uid++, "Foot", MX, H - 620000, W - 2 * MX, 360000, [{ runs: [{ t: s.footer, sz: 1200, color: C.subOnAccent }], algn: "l" }]));
  } else if (type === "section") {
    bg = accent;
    sh.push(textBox(uid++, "Num", MX, 1700000, 4000000, 1600000, [{ runs: [{ t: s.number || String(idx).padStart(2, "0"), sz: 9600, b: true, color: accentDark }], algn: "l" }]));
    sh.push(rect(uid++, MX, 3720000, 760000, 60000, C.white));
    sh.push(textBox(uid++, "Title", MX, 3900000, W - 2 * MX, 1400000, [{ runs: [{ t: s.title || "", sz: 4000, b: true, color: C.white }], algn: "l", lnPct: 110000 }]));
  } else if (type === "quote") {
    bg = C.cardBg;
    sh.push(textBox(uid++, "Mark", MX, 1500000, 1200000, 1200000, [{ runs: [{ t: "“", sz: 9000, b: true, color: accent }], algn: "l" }]));
    sh.push(textBox(uid++, "Quote", MX, 2560000, W - 2 * MX, 2400000, [{ runs: [{ t: s.text || s.title || "", sz: 3200, b: true, color: C.title }], algn: "l", lnPct: 128000 }], "t"));
    if (s.by) sh.push(textBox(uid++, "By", MX, 5100000, W - 2 * MX, 500000, [{ runs: [{ t: s.by, sz: 1600, color: C.muted }], algn: "l" }]));
  } else if (type === "end") {
    bg = accentDark;
    sh.push(textBox(uid++, "Title", MX, 2680000, W - 2 * MX, 1400000, [{ runs: [{ t: s.title || "谢谢观看", sz: 5200, b: true, color: C.white }], algn: "ctr" }], "ctr"));
    if (s.subtitle) sh.push(textBox(uid++, "Sub", MX, 4200000, W - 2 * MX, 700000, [{ runs: [{ t: s.subtitle, sz: 2200, color: C.subOnAccent }], algn: "ctr" }]));
  } else {
    // content：标题 + 短色条 + 要点(+可选右图) + 页脚
    bg = C.white;
    const hasImg = !!s.image;
    let img = null, imgBox = null;
    if (hasImg) { img = loadImage(s.image, idx); if (img) { imgBox = fit(img, W - MX - 4700000, 1720000, 4700000, 3900000); } }
    sh.push(textBox(uid++, "Title", MX, 560000, W - 2 * MX, 900000, [{ runs: [{ t: s.title || "", sz: 3200, b: true, color: C.title }], algn: "l" }]));
    sh.push(rect(uid++, MX, 1500000, 620000, 60000, accent)); // 标题下短色条
    const bodyCx = imgBox ? (imgBox.x - MX - 300000) : (W - 2 * MX);
    const bullets = (s.bullets || []).map((b, i) => {
      const txt = typeof b === "string" ? b : (b && b.text) || "";
      return { runs: [{ t: txt, sz: 2000, color: C.body }], bullet: true, spcBef: i === 0 ? 0 : 900, lnPct: 118000 };
    });
    if (bullets.length) sh.push(textBox(uid++, "Body", MX, 1760000, bodyCx, 4400000, bullets, "t"));
    else if (!hasImg && s.subtitle) sh.push(textBox(uid++, "Body", MX, 1760000, bodyCx, 4400000, [{ runs: [{ t: s.subtitle, sz: 2200, color: C.body }], lnPct: 130000 }]));
    if (imgBox) sh.push(picBox(uid++, img.rid, imgBox.x, imgBox.y, imgBox.cx, imgBox.cy));
    // 页脚：细分隔线 + 页码 + 标题
    sh.push(rect(uid++, MX, 6320000, W - 2 * MX, 12000, C.footRule));
    sh.push(textBox(uid++, "FL", MX, 6380000, 6000000, 340000, [{ runs: [{ t: deckTitle, sz: 1000, color: C.faint }], algn: "l" }]));
    sh.push(textBox(uid++, "FR", W - MX - 2000000, 6380000, 2000000, 340000, [{ runs: [{ t: `${idx + 1} / ${total}`, sz: 1000, color: C.faint }], algn: "r" }]));
  }

  const bgXml = bg ? `<p:bg><p:bgPr><a:solidFill><a:srgbClr val="${bg}"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>` : "";
  slideXmls.push(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld ${NS}><p:cSld>${bgXml}<p:spTree>${EMPTY_GROUP}${sh.join("")}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`);
});

// ---------- 固定 part ----------
const theme1 =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office"><a:themeElements>` +
  `<a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="${accent}"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>` +
  `<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Segoe UI"/><a:ea typeface="Microsoft YaHei"/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Segoe UI"/><a:ea typeface="Microsoft YaHei"/><a:cs typeface=""/></a:minorFont></a:fontScheme>` +
  `<a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`;
const slideMaster1 = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster ${NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>${EMPTY_GROUP}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>`;
const slideLayout1 = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout ${NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>${EMPTY_GROUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>`;
const sldIds = deck.slides.map((_, i) => `<p:sldId id="${256 + i}" r:id="rId${i + 2}"/>`).join("");
const presentation = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation ${NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>${sldIds}</p:sldIdLst><p:sldSz cx="${W}" cy="${H}" type="screen16x9"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>`;
const presRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>` + deck.slides.map((_, i) => `<Relationship Id="rId${i + 2}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide${i + 1}.xml"/>`).join("") + `<Relationship Id="rIdTheme" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/></Relationships>`;
const masterRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>`;
const layoutRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>`;
const rootRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>`;
const imgExts = Array.from(new Set(media.map((m) => m.ext)));
const contentTypes = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>` + imgExts.map((e) => `<Default Extension="${e}" ContentType="image/${e}"/>`).join("") + `<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>` + deck.slides.map((_, i) => `<Override PartName="/ppt/slides/slide${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>`).join("") + `</Types>`;

const entries = [
  { name: "[Content_Types].xml", data: contentTypes },
  { name: "_rels/.rels", data: rootRels },
  { name: "ppt/presentation.xml", data: presentation },
  { name: "ppt/_rels/presentation.xml.rels", data: presRels },
  { name: "ppt/theme/theme1.xml", data: theme1 },
  { name: "ppt/slideMasters/slideMaster1.xml", data: slideMaster1 },
  { name: "ppt/slideMasters/_rels/slideMaster1.xml.rels", data: masterRels },
  { name: "ppt/slideLayouts/slideLayout1.xml", data: slideLayout1 },
  { name: "ppt/slideLayouts/_rels/slideLayout1.xml.rels", data: layoutRels },
];
slideXmls.forEach((xml, i) => {
  entries.push({ name: `ppt/slides/slide${i + 1}.xml`, data: xml });
  const rs = slideRels[i] || [];
  entries.push({ name: `ppt/slides/_rels/slide${i + 1}.xml.rels`, data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdL" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>` + rs.map((r) => `<Relationship Id="${r.id}" Type="${r.type}" Target="${r.target}"/>`).join("") + `</Relationships>` });
});
media.forEach((m) => entries.push({ name: `ppt/media/${m.name}`, data: m.data }));

const outPath = path.resolve(String(args.out || `${deckTitle.replace(/[\\/:*?"<>|]/g, "_")}.pptx`));
try { fs.mkdirSync(path.dirname(outPath), { recursive: true }); fs.writeFileSync(outPath, zipStore(entries)); } catch (e) { fail("写 .pptx 失败: " + e.message); }

/* ==========================================================================
 * 同源双产物：顺手渲一份 HTML 幻灯片
 *
 * **为什么必须有这一份**：.pptx 在软件里预览不出版式 —— 纯前端没有成熟的 pptx 渲染方案
 * （调研过 LibreOffice headless / OnlyOffice，都要拖一个服务端依赖进来），所以内置的
 * 预览只能从 zip 里抽 `<a:t>` 文字**出个大纲**。而客户点「做个 PPT」，想看的恰恰是
 * **它长什么样**：配色、版式、每页的疏密。给他一份纯文字大纲，等于什么都没给。
 *
 * 这份 HTML 和 .pptx **同一份 deck、同一套版式、同一个主题色**渲染出来，
 * 所以它不是"另一个东西的近似"，而是同一份设计的第二种载体：
 *   .pptx = 交付物（客户拿去改、去讲）
 *   .html = 秒开预览（软件里直接看，也能单独发给别人）
 *
 * 自包含：零 CDN、零外链、图片 base64 内联 —— 客户断网、发给同事、拷进 U 盘都能开。
 * （靠 CDN 的话，客户机上十次有一次拉不动，那一次就是"你做的 PPT 打开是白的"。）
 * ========================================================================== */
const hEsc = (s) => String(s == null ? "" : s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
// EMU → 幻灯片百分比：所有位置直接沿用 .pptx 的坐标，版式自然对齐，不用再手调一套
const pctX = (emu) => (emu / W) * 100, pctY = (emu) => (emu / H) * 100;
const MXP = pctX(MX);

/** 已经读进内存的配图 → data URI（和 .pptx 用的是同一份字节，不重复读盘） */
function dataUri(spec) {
  try {
    const buf = fs.readFileSync(String(spec));
    const ext = (path.extname(String(spec)).slice(1) || "png").toLowerCase();
    return `data:image/${ext === "jpg" ? "jpeg" : ext};base64,${buf.toString("base64")}`;
  } catch { return ""; }
}

function slideHtml(s, idx) {
  const type = s.type || (idx === 0 ? "cover" : (s.bullets && s.bullets.length) ? "content" : s.text ? "quote" : s.subtitle ? "section" : "content");
  const total = deck.slides.length;
  if (type === "cover") {
    return `<section class="sl" style="background:#${accentDark}">
      <i class="bar-bottom" style="background:#${accent}"></i>
      <i class="bar" style="left:${MXP}%;top:37%;width:7.4%;background:#${accent}"></i>
      <h1 class="cover-t">${hEsc(s.title || deckTitle)}</h1>
      ${s.subtitle ? `<p class="cover-s">${hEsc(s.subtitle)}</p>` : ""}
      ${s.footer ? `<p class="cover-f">${hEsc(s.footer)}</p>` : ""}</section>`;
  }
  if (type === "section") {
    return `<section class="sl" style="background:#${accent}">
      <div class="sec-n" style="color:#${accentDark}">${hEsc(s.number || String(idx).padStart(2, "0"))}</div>
      <i class="bar" style="left:${MXP}%;top:54%;width:6.2%;background:#fff"></i>
      <h2 class="sec-t">${hEsc(s.title || "")}</h2></section>`;
  }
  if (type === "quote") {
    return `<section class="sl" style="background:#${C.cardBg}">
      <div class="q-mark" style="color:#${accent}">&ldquo;</div>
      <blockquote class="q-t">${hEsc(s.text || s.title || "")}</blockquote>
      ${s.by ? `<p class="q-by">${hEsc(s.by)}</p>` : ""}</section>`;
  }
  if (type === "end") {
    return `<section class="sl end" style="background:#${accentDark}">
      <h1 class="end-t">${hEsc(s.title || "谢谢观看")}</h1>
      ${s.subtitle ? `<p class="end-s">${hEsc(s.subtitle)}</p>` : ""}</section>`;
  }
  const uri = s.image ? dataUri(s.image) : "";
  const bullets = (s.bullets || []).map((b) => hEsc(typeof b === "string" ? b : (b && b.text) || "")).filter(Boolean);
  return `<section class="sl" style="background:#fff">
    <h3 class="c-t">${hEsc(s.title || "")}</h3>
    <i class="bar" style="left:${MXP}%;top:${pctY(1500000)}%;width:5.1%;background:#${accent}"></i>
    <div class="c-row${uri ? " has-img" : ""}">
      <ul class="c-b">${bullets.map((b) => `<li><i style="background:#${accent}"></i><span>${b}</span></li>`).join("")}</ul>
      ${uri ? `<div class="c-img"><img src="${uri}" alt=""></div>` : ""}
    </div>
    <i class="foot-rule"></i>
    <p class="foot-l">${hEsc(deckTitle)}</p><p class="foot-r">${idx + 1} / ${total}</p></section>`;
}

const htmlDoc = `<!doctype html><html lang="zh-CN"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${hEsc(deckTitle)}</title><style>
*{box-sizing:border-box;margin:0;padding:0}
body{background:#111827;font:16px/1.5 "Microsoft YaHei","PingFang SC",system-ui,sans-serif;padding:24px 0}
.wrap{max-width:1100px;margin:0 auto;padding:0 16px}
.sl{position:relative;width:100%;aspect-ratio:16/9;margin:0 0 22px;border-radius:8px;overflow:hidden;box-shadow:0 6px 24px rgba(0,0,0,.45);container-type:inline-size}
/* 字号跟着幻灯片宽度走（cqw），缩放窗口时版式不散 —— 和 .pptx 的相对比例一致 */
.bar{position:absolute;height:.9cqw;border-radius:1px}
.bar-bottom{position:absolute;left:0;right:0;bottom:0;height:3.2cqw}
.cover-t{position:absolute;left:6.9%;top:39%;right:6.9%;color:#fff;font-size:5.4cqw;font-weight:700;line-height:1.15}
.cover-s{position:absolute;left:6.9%;top:62%;right:6.9%;color:#${C.subOnAccent};font-size:2.2cqw}
.cover-f{position:absolute;left:6.9%;bottom:5.5%;color:#${C.subOnAccent};font-size:1.2cqw;opacity:.85}
.sec-n{position:absolute;left:6.9%;top:23%;font-size:9.6cqw;font-weight:700;line-height:1}
.sec-t{position:absolute;left:6.9%;top:57%;right:6.9%;color:#fff;font-size:4cqw;font-weight:700;line-height:1.2}
.q-mark{position:absolute;left:6.9%;top:18%;font-size:9cqw;font-weight:700;line-height:1}
.q-t{position:absolute;left:6.9%;top:37%;right:6.9%;color:#${C.title};font-size:3.2cqw;font-weight:700;line-height:1.35}
.q-by{position:absolute;left:6.9%;top:74%;color:#${C.muted};font-size:1.6cqw}
.end{display:grid;place-content:center;text-align:center}
.end-t{color:#fff;font-size:5.2cqw;font-weight:700}
.end-s{color:#${C.subOnAccent};font-size:2.2cqw;margin-top:2cqw}
.c-t{position:absolute;left:6.9%;top:8.2%;right:6.9%;color:#${C.title};font-size:3.2cqw;font-weight:700}
.c-row{position:absolute;left:6.9%;right:6.9%;top:25.7%;bottom:12%;display:flex;gap:2.5cqw}
.c-b{list-style:none;flex:1;min-width:0}
.c-b li{display:flex;gap:1.1cqw;color:#${C.body};font-size:2cqw;line-height:1.5;margin-bottom:1.5cqw}
.c-b li i{flex:none;width:.75cqw;height:.75cqw;border-radius:50%;margin-top:.7cqw}
.c-img{flex:none;width:38.5%;display:grid;place-items:center}
.c-img img{max-width:100%;max-height:100%;object-fit:contain;border-radius:4px}
.foot-rule{position:absolute;left:6.9%;right:6.9%;top:92.2%;height:1px;background:#${C.footRule}}
.foot-l{position:absolute;left:6.9%;top:93.5%;color:#${C.faint};font-size:1cqw}
.foot-r{position:absolute;right:6.9%;top:93.5%;color:#${C.faint};font-size:1cqw}
.tip{color:#9ca3af;font-size:12px;text-align:center;padding:2px 0 16px}
@media print{body{background:#fff;padding:0}.wrap{max-width:none;padding:0}.sl{margin:0;border-radius:0;box-shadow:none;page-break-after:always}.tip{display:none}}
</style><div class="wrap">
<div class="tip">${hEsc(deckTitle)} · 共 ${deck.slides.length} 页 · ↑↓ / PgUp PgDn 翻页 · Ctrl+P 可直接印成 PDF</div>
${deck.slides.map((s, i) => slideHtml(s, i)).join("\n")}
</div><script>
// 翻页：把下一张滚到视口顶部。**只认方向键/翻页键**，不劫持普通滚轮 —— 想连着往下刷的人更多。
var sl=[].slice.call(document.querySelectorAll('.sl')),cur=0;
function go(d){cur=Math.max(0,Math.min(sl.length-1,cur+d));sl[cur].scrollIntoView({behavior:'smooth',block:'center'});}
addEventListener('keydown',function(e){
  if(e.key==='ArrowDown'||e.key==='PageDown'||e.key===' '){e.preventDefault();go(1);}
  else if(e.key==='ArrowUp'||e.key==='PageUp'){e.preventDefault();go(-1);}
});
</script></html>`;

// 默认就出这一份（`--no-html` 可关）。**不能等 AI 想起来加参数** —— 它十次有八次不会加，
// 那这个功能就等于不存在。多出一个文件的代价，远小于客户看不到成果的代价。
let htmlPath = "";
if (!args["no-html"]) {
  htmlPath = typeof args.html === "string" ? path.resolve(args.html) : outPath.replace(/\.pptx$/i, "") + ".预览.html";
  try { fs.writeFileSync(htmlPath, htmlDoc); } catch { htmlPath = ""; } // 预览失败不该让交付物跟着失败
}

if (asJson) console.log(JSON.stringify({ ok: true, file: outPath, html: htmlPath || undefined, slides: deck.slides.length, accent }));
else console.log(`已生成 ${deck.slides.length} 页 PPT（主题色 #${accent}）：${outPath}` + (htmlPath ? `\n可直接预览的网页版：${htmlPath}` : ""));
