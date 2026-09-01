#!/usr/bin/env node
/**
 * gen-docx.mjs —— 纯 std（零 npm 依赖）把 Markdown / 结构化块生成**真 .docx**（Word/WPS 能开）。
 *
 * 客户机只有便携 Node、装不了 python-docx/docx 库，所以手搓 WordprocessingML + ZIP(STORE)。
 * 不含任何 Key、不联网，纯本地把文本写成 Word 文档。
 *
 * 用法（AI 经 run_command 调；正文用 --md 传 Markdown 文件最自然）：
 *   node gen-docx.mjs --md report.md --out 周报.docx --json
 *   node gen-docx.mjs --in doc.json --out 报告.docx --json          # 结构化块(见下)
 *
 * 支持的 Markdown 子集：# / ## / ### 标题、- 或 * 列表、普通段落、
 *   | a | b | 表格(带 |---| 分隔行)、![alt](图片路径)、**加粗** 行内。
 *
 * doc.json 结构（--in，等价于 md）：
 *   { "title": "标题(可选)", "blocks": [
 *       {"type":"heading","level":1,"text":"一级标题"},
 *       {"type":"paragraph","text":"正文，支持 **加粗**"},
 *       {"type":"bullets","items":["要点1","要点2"]},
 *       {"type":"table","rows":[["表头A","表头B"],["1","2"]]},
 *       {"type":"image","path":"chart.png"} ] }
 *
 * 输出：成功打印 .docx 绝对路径；--json 时打印 {"ok":true,"file":"..."}。
 * 配图先用 generate_image / uking-aigc 出图，再把绝对路径(Windows 风格 C:/…)填进来。
 */
import fs from "node:fs";
import path from "node:path";

// ---------- CLI ----------
function parseArgs(argv) {
  const a = {};
  for (let i = 0; i < argv.length; i++) {
    const t = argv[i];
    if (t.startsWith("--")) {
      const k = t.slice(2);
      a[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
    }
  }
  return a;
}
const args = parseArgs(process.argv.slice(2));
const asJson = !!args.json;
function fail(m) {
  if (asJson) console.log(JSON.stringify({ ok: false, error: String(m) }));
  else console.error("[gen-docx] 失败:", m);
  process.exit(1);
}

// ---------- 取内容：--md 解析 Markdown / --in 读 JSON blocks ----------
function parseMarkdown(md) {
  const blocks = [];
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  let para = [];
  let table = null;
  const flushPara = () => {
    if (para.length) { blocks.push({ type: "paragraph", text: para.join(" ") }); para = []; }
  };
  const flushTable = () => {
    if (table && table.length) blocks.push({ type: "table", rows: table });
    table = null;
  };
  for (const raw of lines) {
    const line = raw.replace(/\s+$/, "");
    const t = line.trim();
    let m;
    if (!t) { flushPara(); flushTable(); continue; }
    if ((m = t.match(/^(#{1,6})\s+(.*)$/))) { flushPara(); flushTable(); blocks.push({ type: "heading", level: m[1].length, text: m[2] }); continue; }
    if ((m = t.match(/^!\[[^\]]*\]\(([^)]+)\)\s*$/))) { flushPara(); flushTable(); blocks.push({ type: "image", path: m[1].trim() }); continue; }
    if (/^[-*]\s+/.test(t)) {
      flushPara(); flushTable();
      const item = t.replace(/^[-*]\s+/, "");
      const last = blocks[blocks.length - 1];
      if (last && last.type === "bullets") last.items.push(item);
      else blocks.push({ type: "bullets", items: [item] });
      continue;
    }
    if (/^\|.*\|$/.test(t)) {
      flushPara();
      const cells = t.slice(1, -1).split("|").map((c) => c.trim());
      if (cells.every((c) => /^:?-{2,}:?$/.test(c) || c === "")) continue; // |---| 分隔行跳过
      (table = table || []).push(cells);
      continue;
    }
    flushTable();
    para.push(t);
  }
  flushPara(); flushTable();
  return blocks;
}

let doc;
try {
  if (args.md) doc = { title: args.title || "", blocks: parseMarkdown(fs.readFileSync(String(args.md), "utf8")) };
  else if (args.in) doc = JSON.parse(fs.readFileSync(String(args.in), "utf8"));
  else if (args.text) doc = { title: args.title || "", blocks: parseMarkdown(String(args.text)) };
  else fail("需要 --md <file.md> 或 --in <doc.json>");
} catch (e) { fail("读内容失败: " + e.message); }
if (!doc || !Array.isArray(doc.blocks)) fail("没有 blocks");
if (doc.title) doc.blocks.unshift({ type: "heading", level: 0, text: doc.title });

// ---------- XML 转义 ----------
// 🔴 先剥 XML 1.0 非法控制字符，再转义。转义只管 `& < > "`，管不了 0x0B/0x1F 这类字节 ——
// 它们没有实体写法，**任何**写法进了 document.xml 都是非法 XML，Word 直接报「文件已损坏」。
// 而它们很常见：OCR 结果、从 PDF 复制的文本、旧系统导出的 CSV 里都带。
// 以前的行为是照写不误、脚本还返回 `ok:true` + exit 0 —— 用户把文件交付出去才发现打不开，
// 这比报错坏得多。（XML 1.0 只准 \t \n \r 这三个 C0 字符）
const stripCtl = (s) => s.replace(/[\x00-\x08\x0B\x0C\x0E-\x1F]/g, "");
const esc = (s) => stripCtl(String(s == null ? "" : s)).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// ---------- CRC32 + ZIP(STORE) ----------
const CRC = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
function crc32(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = CRC[(c ^ b[i]) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; }
function zipStore(entries) {
  const parts = [], central = []; let off = 0;
  for (const e of entries) {
    const nb = Buffer.from(e.name, "utf8");
    const d = Buffer.isBuffer(e.data) ? e.data : Buffer.from(e.data, "utf8");
    const crc = crc32(d);
    const lh = Buffer.alloc(30);
    lh.writeUInt32LE(0x04034b50, 0); lh.writeUInt16LE(20, 4); lh.writeUInt16LE(0, 6); lh.writeUInt16LE(0, 8);
    lh.writeUInt16LE(0, 10); lh.writeUInt16LE(0x21, 12); lh.writeUInt32LE(crc, 14);
    lh.writeUInt32LE(d.length, 18); lh.writeUInt32LE(d.length, 22); lh.writeUInt16LE(nb.length, 26); lh.writeUInt16LE(0, 28);
    parts.push(lh, nb, d);
    const ch = Buffer.alloc(46);
    ch.writeUInt32LE(0x02014b50, 0); ch.writeUInt16LE(20, 4); ch.writeUInt16LE(20, 6); ch.writeUInt16LE(0, 8);
    ch.writeUInt16LE(0, 10); ch.writeUInt16LE(0, 12); ch.writeUInt16LE(0x21, 14); ch.writeUInt32LE(crc, 16);
    ch.writeUInt32LE(d.length, 20); ch.writeUInt32LE(d.length, 24); ch.writeUInt16LE(nb.length, 28);
    ch.writeUInt16LE(0, 30); ch.writeUInt16LE(0, 32); ch.writeUInt16LE(0, 34); ch.writeUInt16LE(0, 36);
    ch.writeUInt32LE(0, 38); ch.writeUInt32LE(off, 42);
    central.push(ch, nb); off += 30 + nb.length + d.length;
  }
  const cb = Buffer.concat(central);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0); eocd.writeUInt16LE(0, 4); eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8); eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(cb.length, 12); eocd.writeUInt32LE(off, 16); eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...parts, cb, eocd]);
}

// ---------- 图片尺寸(PNG/JPEG) ----------
function imageSize(buf) {
  if (buf.length > 24 && buf[0] === 0x89 && buf[1] === 0x50) return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
  if (buf[0] === 0xff && buf[1] === 0xd8) {
    let o = 2;
    while (o < buf.length) {
      if (buf[o] !== 0xff) { o++; continue; }
      const mk = buf[o + 1];
      if (mk >= 0xc0 && mk <= 0xcf && mk !== 0xc4 && mk !== 0xc8 && mk !== 0xcc) return { h: buf.readUInt16BE(o + 5), w: buf.readUInt16BE(o + 7) };
      o += 2 + buf.readUInt16BE(o + 2);
    }
  }
  return null;
}

// ---------- 行内 **加粗** → runs ----------
function runs(text, base = {}) {
  const out = [];
  const re = /\*\*([^*]+)\*\*/g;
  let last = 0, m;
  const push = (t, bold) => { if (t) out.push({ t, b: bold || base.b, sz: base.sz, color: base.color }); };
  while ((m = re.exec(text))) { push(text.slice(last, m.index), false); push(m[1], true); last = re.lastIndex; }
  push(text.slice(last), false);
  return out.length ? out : [{ t: "", ...base }];
}
function runXml(r) {
  const rpr = [];
  if (r.b) rpr.push("<w:b/>");
  if (r.sz) rpr.push(`<w:sz w:val="${r.sz}"/><w:szCs w:val="${r.sz}"/>`);
  if (r.color) rpr.push(`<w:color w:val="${r.color}"/>`);
  return `<w:r>${rpr.length ? `<w:rPr>${rpr.join("")}</w:rPr>` : ""}<w:t xml:space="preserve">${esc(r.t)}</w:t></w:r>`;
}
function paraXml(rs, opts = {}) {
  const ppr = [];
  // ⚠ w:pPr 的子元素**必须按 schema 顺序**：pStyle → spacing → ind。写反了严格校验器会判文档无效
  // （Word 多半仍能打开，但这种"能开但不合法"的文件正是过一手 WPS/在线预览就崩的那种）。
  if (opts.style) ppr.push(`<w:pStyle w:val="${opts.style}"/>`);
  if (opts.spaceBefore) ppr.push(`<w:spacing w:before="${opts.spaceBefore}" w:after="120"/>`);
  if (opts.bullet) ppr.push(`<w:ind w:left="420" w:hanging="220"/>`);
  return `<w:p>${ppr.length ? `<w:pPr>${ppr.join("")}</w:pPr>` : ""}${rs.map(runXml).join("")}</w:p>`;
}

// ---------- 组装 body ----------
const media = []; // {name, data, ext}
const rels = []; // document.xml.rels 额外(图片)
const HEAD_SZ = { 0: 56, 1: 36, 2: 30, 3: 26, 4: 24, 5: 22, 6: 22 };
const bodyParts = [];

for (const b of doc.blocks) {
  if (b.type === "heading") {
    const sz = HEAD_SZ[b.level] ?? 26;
    // 挂上真正的标题样式（导航窗格 / 自动目录 / 大纲折叠全靠它），
    // 直接格式照旧保留 —— 样式丢了也还是长得像标题，两层保险。
    const style = b.level === 0 ? "Title" : `Heading${Math.min(6, Math.max(1, b.level))}`;
    bodyParts.push(paraXml([{ t: b.text || "", b: true, sz, color: b.level <= 1 ? "1F2937" : "374151" }], { style, spaceBefore: b.level <= 1 ? 240 : 160 }));
  } else if (b.type === "paragraph") {
    bodyParts.push(paraXml(runs(b.text || "", { sz: 22 })));
  } else if (b.type === "bullets") {
    for (const it of b.items || []) bodyParts.push(paraXml([{ t: "• ", sz: 22 }, ...runs(it, { sz: 22 })], { bullet: true }));
  } else if (b.type === "table") {
    const rows = b.rows || [];
    if (rows.length) {
      const cols = Math.max(...rows.map((r) => r.length));
      const grid = Array.from({ length: cols }, () => `<w:gridCol w:w="${Math.floor(9360 / cols)}"/>`).join("");
      const borders = "<w:tblBorders>" + ["top", "left", "bottom", "right", "insideH", "insideV"].map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/>`).join("") + "</w:tblBorders>";
      const trs = rows.map((r, ri) => {
        const tcs = Array.from({ length: cols }, (_, ci) => {
          const cell = r[ci] == null ? "" : r[ci];
          const rs = runs(String(cell), { sz: 20, b: ri === 0 });
          return `<w:tc><w:tcPr><w:tcW w:w="${Math.floor(9360 / cols)}" w:type="dxa"/>${ri === 0 ? '<w:shd w:val="clear" w:fill="F3F4F6"/>' : ""}</w:tcPr>${paraXml(rs)}</w:tc>`;
        }).join("");
        return `<w:tr>${tcs}</w:tr>`;
      }).join("");
      bodyParts.push(`<w:tbl><w:tblPr><w:tblW w:w="9360" w:type="dxa"/>${borders}</w:tblPr><w:tblGrid>${grid}</w:tblGrid>${trs}</w:tbl>`);
      bodyParts.push(paraXml([{ t: "", sz: 22 }])); // 表格后留空行
    }
  } else if (b.type === "image" && b.path) {
    try {
      const data = fs.readFileSync(String(b.path));
      const ext = (path.extname(String(b.path)).slice(1) || "png").toLowerCase();
      const e = ext === "jpg" ? "jpeg" : ext;
      const name = `image${media.length + 1}.${e}`;
      media.push({ name, data, ext: e });
      const rid = `rIdImg${media.length}`;
      rels.push({ id: rid, type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image", target: `media/${name}` });
      const dim = imageSize(data) || { w: 600, h: 400 };
      const maxW = 5943600; // 6.5in 内容宽
      let cx = dim.w * 9525, cy = dim.h * 9525;
      if (cx > maxW) { const s = maxW / cx; cx = Math.round(cx * s); cy = Math.round(cy * s); }
      const id = media.length + 100;
      bodyParts.push(
        `<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">` +
          `<wp:extent cx="${cx}" cy="${cy}"/><wp:docPr id="${id}" name="image${id}"/>` +
          `<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">` +
          `<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="${id}" name="image${id}"/><pic:cNvPicPr/></pic:nvPicPr>` +
          `<pic:blipFill><a:blip r:embed="${rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>` +
          `<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>` +
          `</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>`,
      );
    } catch { /* 图读不到跳过，不让整篇挂 */ }
  }
}

// ---------- 各 part ----------
const document =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" ` +
  `xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" ` +
  `xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">` +
  `<w:body>${bodyParts.join("")}` +
  `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>` +
  `</w:body></w:document>`;

/**
 * 标题样式 —— 让标题成为**真的标题**，而不是"大号粗体的普通段落"。
 *
 * 原先标题只有直接格式（加粗+字号+颜色），看着像标题，但对 Word 来说就是普通段落：
 * **导航窗格是空的、自动目录生成不出来、大纲视图没有层级、样式改不动**。
 * 客户是打开文件、点「引用 → 目录」那一刻才发现的 —— 而那时他已经把文件发出去了。
 *
 * 两个关键点，少一个都不成立：
 *  - `w:name` 必须是 Word 认的内置名 `heading 1`…（**小写 + 空格**），写成 `Heading1` 只会
 *    多出一个自定义样式，导航窗格照样空。
 *  - `w:outlineLvl` 才是「大纲级别」的真身。导航窗格 / 目录 / 折叠全看它，不看字号。
 */
const HEAD_STYLE = { 1: 36, 2: 30, 3: 26, 4: 24, 5: 22, 6: 22 };
const headingStyles = Object.entries(HEAD_STYLE)
  .map(([lv, sz]) => {
    const n = Number(lv);
    return (
      `<w:style w:type="paragraph" w:styleId="Heading${n}">` +
      `<w:name w:val="heading ${n}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/>` +
      `<w:pPr><w:keepNext/><w:outlineLvl w:val="${n - 1}"/><w:spacing w:before="${n <= 1 ? 240 : 160}" w:after="120"/></w:pPr>` +
      `<w:rPr><w:b/><w:sz w:val="${sz}"/><w:szCs w:val="${sz}"/><w:color w:val="${n <= 1 ? "1F2937" : "374151"}"/></w:rPr>` +
      `</w:style>`
    );
  })
  .join("");

const styles =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">` +
  `<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:eastAsia="Microsoft YaHei" w:hAnsi="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>` +
  `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>` +
  // 文档标题（level 0）：给 outlineLvl 0，导航窗格里能当根节点看到
  `<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/>` +
  `<w:pPr><w:keepNext/><w:outlineLvl w:val="0"/><w:spacing w:before="240" w:after="160"/></w:pPr>` +
  `<w:rPr><w:b/><w:sz w:val="56"/><w:szCs w:val="56"/><w:color w:val="1F2937"/></w:rPr></w:style>` +
  headingStyles +
  `</w:styles>`;

const imgExts = Array.from(new Set(media.map((m) => m.ext)));
const contentTypes =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
  `<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>` +
  `<Default Extension="xml" ContentType="application/xml"/>` +
  imgExts.map((e) => `<Default Extension="${e}" ContentType="image/${e}"/>`).join("") +
  `<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>` +
  `<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>` +
  `</Types>`;

const rootRels =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
  `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>` +
  `</Relationships>`;

const docRels =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
  `<Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>` +
  rels.map((r) => `<Relationship Id="${r.id}" Type="${r.type}" Target="${r.target}"/>`).join("") +
  `</Relationships>`;

const entries = [
  { name: "[Content_Types].xml", data: contentTypes },
  { name: "_rels/.rels", data: rootRels },
  { name: "word/document.xml", data: document },
  { name: "word/_rels/document.xml.rels", data: docRels },
  { name: "word/styles.xml", data: styles },
];
media.forEach((m) => entries.push({ name: `word/media/${m.name}`, data: m.data }));

const outPath = path.resolve(String(args.out || `${(doc.title || "文档").replace(/[\\/:*?"<>|]/g, "_")}.docx`));
try { fs.mkdirSync(path.dirname(outPath), { recursive: true }); fs.writeFileSync(outPath, zipStore(entries)); }
catch (e) { fail("写 .docx 失败: " + e.message); }
if (asJson) console.log(JSON.stringify({ ok: true, file: outPath, blocks: doc.blocks.length }));
else console.log(`已生成 Word 文档（${doc.blocks.length} 块）：${outPath}`);
