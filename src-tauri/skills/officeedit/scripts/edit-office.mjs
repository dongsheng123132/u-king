#!/usr/bin/env node
/**
 * edit-office.mjs —— 在**客户自己那份 Office 文件上**改文字，格式一个字节都不动。
 * 支持 **.docx（Word）/ .pptx（PPT）/ .xlsx（Excel）**。
 *
 * ## 为什么必须有这个
 * 技能包原本只会「从零生成」：客户拿一份公司模板的文件让 AI 改两处，AI 只能读出来、
 * 重新生成一份 —— 页眉页脚、字体、编号、母版、图表**全丢**，而且是**静默降级**，
 * 客户拿到手才发现。真实样本实测：一份 39 段的 Word 带着 header1.xml / footer1.xml /
 * numbering.xml / styles.xml，那些才是"格式"。
 *
 * ## 怎么做到「格式不丢」
 * 未修改的部件**直接复制原始压缩字节**（连 CRC 和压缩方式都照抄），不解压也不重压 ——
 * 所以样式、母版、图片、图表是**字节级相同**，不是"看起来一样"。只有装文字的那几个
 * XML 部件被重写。
 *
 * ## 为什么不能简单字符串替换
 * 三种格式都会把一句话拆成很多个文本节点（拼写检查、格式标记、中文断字都会拆）。
 * 一份真实 Word 里平均每段 5.1 个 `<w:t>`，最碎的一段被拆成 **103 个** —— 直接在 XML 上
 * 搜"投标保证金"基本搜不到。所以按**段落**把文字拼起来再匹配，命中后写回该段第一个
 * 文本节点、其余清空（run 属性原样留着 = 格式还在）。
 *
 * 三种格式的「段落 → run」结构是同构的，所以是同一套逻辑、只换标签名：
 *
 * | 格式  | 改哪些部件                    | 段落    | 文本节点 |
 * |-------|-------------------------------|---------|----------|
 * | docx  | word/document.xml (+页眉页脚) | `<w:p>` | `<w:t>`  |
 * | pptx  | ppt/slides/slideN.xml (+备注) | `<a:p>` | `<a:t>`  |
 * | xlsx  | xl/sharedStrings.xml (+工作表)| `<si>`  | `<t>`    |
 *
 * ## 用法
 *   node edit-office.mjs 合同.docx --replace "甲方：张三=>甲方：李四" --out 合同-改.docx --json
 *   node edit-office.mjs 方案.pptx --replace "2025=>2026" --all-parts --json
 *   node edit-office.mjs 报价.xlsx --map 改动.json --json          # 改动多时用 JSON
 *   node edit-office.mjs 报告.docx --replace "旧=>新" --in-place    # 覆盖原文件（自动留 .bak）
 *
 * `--map` 的 JSON 形如 `[{"find":"旧文本","replace":"新文本"}, …]`。
 *
 * stdout 只出结果（`--json` 时是一行 JSON），日志走 stderr —— 可以直接接管道。
 * **没命中的会如实列在 `missed` 里**：一条都没命中时退出码 1（等于什么都没干，不能报成功）。
 *
 * 纯 std（只用 node:fs / node:zlib），零 npm 依赖 —— 客户机只有便携 Node。
 * 不含任何 Key、不联网。
 */
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

/* ---- 三种格式的差异，集中在这一张表 ---------------------------------------
 * 它们的「段落 → run」结构是同构的，所以下面的替换逻辑只有一份，这里只换标签名和部件名。
 * 加一种格式 = 这张表加一行（比如将来支持 .odt）。
 *
 * `main` 只改正文部件；`--all-parts` 才连页眉页脚 / 备注页 / 工作表内联字符串一起改 ——
 * 默认不改是因为那些地方常放模板信息（公司名、文件编号），批量替换容易误伤。
 * -------------------------------------------------------------------------- */
const FORMATS = {
  ".docx": {
    label: "Word",
    main: /^word\/document\.xml$/,
    extra: /^word\/(header|footer)\d*\.xml$/, // 页眉页脚：文件编号/日期常在这儿
    para: "w:p",
    text: "w:t",
    marker: "word/document.xml",
    hint: ".doc（老二进制格式）不是 ZIP —— 让客户先在 Word 里「另存为 .docx」",
  },
  ".pptx": {
    label: "PPT",
    main: /^ppt\/slides\/slide\d+\.xml$/,
    extra: /^ppt\/notesSlides\/notesSlide\d+\.xml$/, // 演讲者备注
    para: "a:p",
    text: "a:t",
    marker: "ppt/presentation.xml",
    hint: ".ppt（老格式）请先另存为 .pptx",
  },
  ".xlsx": {
    // Excel 把单元格里的文字集中放在 sharedStrings.xml（`<si>` 一项一个字符串，
    // 富文本会再拆成多个 `<r><t>`）。所以「段落」= `<si>`，「run」= `<t>`，跟前两种同构。
    // ⚠️ 改一处 = 所有引用该字符串的单元格一起变 —— 这是 Excel 的共享字符串机制，不是 bug。
    label: "Excel",
    main: /^xl\/sharedStrings\.xml$/,
    extra: /^xl\/worksheets\/sheet\d+\.xml$/, // 内联字符串（少见）
    para: "si",
    text: "t",
    marker: "xl/workbook.xml",
    hint: ".xls（老格式）请先另存为 .xlsx；公式和数字不在文本表里，改不了",
  },
};

/* ---- 参数 ---------------------------------------------------------------- */
function parseArgs(argv) {
  const a = { replace: [] };
  for (let i = 0; i < argv.length; i++) {
    const v = argv[i];
    if (v === "--out") a.out = argv[++i];
    else if (v === "--replace") a.replace.push(argv[++i]);
    else if (v === "--map") a.map = argv[++i];
    else if (v === "--in-place") a.inPlace = true;
    else if (v === "--json") a.json = true;
    else if (v === "--all-parts") a.allParts = true; // 连页眉页脚一起改
    else if (!v.startsWith("--") && !a.src) a.src = v;
  }
  return a;
}
function fail(msg, code = 2) {
  process.stderr.write(msg + "\n");
  process.exit(code);
}

/* ---- 极简 ZIP 读写（只用 node:zlib） --------------------------------------
 * 只认「中央目录」里的 csize/usize/crc —— 本地头在带 data descriptor 时那三个字段是 0，
 * 照本地头读会拿到零长度数据。这是 ZIP 解析最常见的坑。
 * -------------------------------------------------------------------------- */
function readZip(buf) {
  const eocd = buf.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (eocd < 0) fail("不是有效的 .docx（找不到 ZIP 结尾）—— 这个文件可能是 .doc 老格式或已损坏");
  const count = buf.readUInt16LE(eocd + 10);
  let p = buf.readUInt32LE(eocd + 16);
  const items = [];
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(p) !== 0x02014b50) fail("ZIP 中央目录损坏");
    const flags = buf.readUInt16LE(p + 8);
    const method = buf.readUInt16LE(p + 10);
    const mtime = buf.readUInt16LE(p + 12);
    const mdate = buf.readUInt16LE(p + 14);
    const crc = buf.readUInt32LE(p + 16);
    const csize = buf.readUInt32LE(p + 20);
    const usize = buf.readUInt32LE(p + 24);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const cmtLen = buf.readUInt16LE(p + 32);
    const lho = buf.readUInt32LE(p + 42);
    const name = buf.slice(p + 46, p + 46 + nameLen).toString("utf8");
    // 数据起点要按**本地头**里的 name/extra 长度算（extra 常和中央目录里的不一样）
    const lNameLen = buf.readUInt16LE(lho + 26);
    const lExtraLen = buf.readUInt16LE(lho + 28);
    const dataOff = lho + 30 + lNameLen + lExtraLen;
    items.push({ name, flags, method, mtime, mdate, crc, csize, usize, raw: buf.slice(dataOff, dataOff + csize) });
    p += 46 + nameLen + extraLen + cmtLen;
  }
  return items;
}

function inflate(item) {
  return item.method === 8 ? zlib.inflateRawSync(item.raw) : Buffer.from(item.raw);
}

const CRC_TABLE = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();
function crc32(b) {
  let c = 0xffffffff;
  for (let i = 0; i < b.length; i++) c = CRC_TABLE[(c ^ b[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function writeZip(items) {
  const locals = [];
  const centrals = [];
  let off = 0;
  for (const it of items) {
    const name = Buffer.from(it.name, "utf8");
    const lh = Buffer.alloc(30);
    lh.writeUInt32LE(0x04034b50, 0);
    lh.writeUInt16LE(20, 4);
    // 清掉 data descriptor 标志（bit 3）：我们把真实长度写进了本地头，不再需要它
    lh.writeUInt16LE(it.flags & ~0x08, 6);
    lh.writeUInt16LE(it.method, 8);
    lh.writeUInt16LE(it.mtime, 10);
    lh.writeUInt16LE(it.mdate, 12);
    lh.writeUInt32LE(it.crc, 14);
    lh.writeUInt32LE(it.raw.length, 18);
    lh.writeUInt32LE(it.usize, 22);
    lh.writeUInt16LE(name.length, 26);
    lh.writeUInt16LE(0, 28);
    locals.push(lh, name, it.raw);

    const ch = Buffer.alloc(46);
    ch.writeUInt32LE(0x02014b50, 0);
    ch.writeUInt16LE(20, 4);
    ch.writeUInt16LE(20, 6);
    ch.writeUInt16LE(it.flags & ~0x08, 8);
    ch.writeUInt16LE(it.method, 10);
    ch.writeUInt16LE(it.mtime, 12);
    ch.writeUInt16LE(it.mdate, 14);
    ch.writeUInt32LE(it.crc, 16);
    ch.writeUInt32LE(it.raw.length, 20);
    ch.writeUInt32LE(it.usize, 24);
    ch.writeUInt16LE(name.length, 28);
    ch.writeUInt32LE(off, 42);
    centrals.push(ch, name);
    off += 30 + name.length + it.raw.length;
  }
  const cd = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(items.length, 8);
  eocd.writeUInt16LE(items.length, 10);
  eocd.writeUInt32LE(cd.length, 12);
  eocd.writeUInt32LE(off, 16);
  return Buffer.concat([...locals, cd, eocd]);
}

/* ---- 段落级替换 ----------------------------------------------------------- */
// 🔴 与 gen-docx/xlsx/pptx 同一条闸：XML 1.0 非法的 C0 控制字符没有实体写法，
// 写进 document.xml 就是「Word 报文件已损坏」而不是「显示成乱码」。替换文本可能来自
// OCR / PDF 复制，先剥掉再转义。（XML 1.0 只准 \t \n \r）
const escapeXml = (s) =>
  s.replace(/[\x00-\x08\x0B\x0C\x0E-\x1F]/g, "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const unescapeXml = (s) =>
  s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");

/**
 * 在一份 document.xml 上做替换。返回 `{ xml, hits }`。
 *
 * 按 `<w:p>` 切段：把段内所有 `<w:t>…</w:t>` 的文字拼成整段（Word 会把一句话拆成几十个
 * run，不拼起来根本匹配不上），命中后把新整段写回**第一个** `<w:t>`、其余清空。
 * run 的属性（`<w:rPr>` 字体/字号/加粗）原封不动 —— 所以格式还在，只是文字换了。
 *
 * ⚠️ 副作用要说清楚：命中段落内部**原有的分段格式会被并成第一个 run 的格式**
 * （比如一段里前半句加粗、后半句不加粗，改完整段跟着前半句走）。这是「保住整篇模板」
 * 和「保住段内混排」之间的取舍，SKILL.md 里如实写了。
 */
export function replaceInXml(xml, pairs, fmt = FORMATS[".docx"]) {
  const hits = new Map(pairs.map((p) => [p.find, 0]));
  const P = fmt.para; // w:p / a:p / si
  const T = fmt.text; // w:t / a:t / t
  // 用段落开标签作为切分点：保留分隔符本身，改完能原样拼回去。
  // `[ >]` 而不是 `\b`：xlsx 的段落标签是裸 `<si>`，用 \b 会连 `<sheetData>` 一起吃掉。
  const parts = xml.split(new RegExp(`(<${P}[ >])`));
  for (let i = 0; i < parts.length; i++) {
    if (!new RegExp(`<${T}[ >]`).test(parts[i])) continue;
    const tRe = new RegExp(`(<${T}(?:\\s[^>]*)?>)([\\s\\S]*?)(</${T}>)`, "g");
    const runs = [];
    let m;
    while ((m = tRe.exec(parts[i]))) runs.push({ open: m[1], text: m[2], close: m[3], start: m.index, end: tRe.lastIndex });
    if (!runs.length) continue;
    const joined = unescapeXml(runs.map((r) => r.text).join(""));
    let next = joined;
    for (const p of pairs) {
      if (!p.find || !next.includes(p.find)) continue;
      const before = next;
      next = next.split(p.find).join(p.replace);
      if (next !== before) {
        const n = before.split(p.find).length - 1;
        hits.set(p.find, hits.get(p.find) + n);
      }
    }
    if (next === joined) continue;
    // 整段文字写回第一个文本节点，其余清空（保留标签和属性 = 保留格式）
    let out = "";
    let cursor = 0;
    runs.forEach((r, idx) => {
      out += parts[i].slice(cursor, r.start);
      const open = idx === 0 && !/xml:space=/.test(r.open) ? r.open.replace(/>$/, ' xml:space="preserve">') : r.open;
      out += open + (idx === 0 ? escapeXml(next) : "") + r.close;
      cursor = r.end;
    });
    out += parts[i].slice(cursor);
    parts[i] = out;
  }
  return { xml: parts.join(""), hits };
}

/* ---- 主流程 --------------------------------------------------------------- */
function main() {
  const a = parseArgs(process.argv.slice(2));
  if (!a.src) fail("用法: node edit-office.mjs <文件.docx|.pptx|.xlsx> --replace \"旧=>新\" [--out 输出文件] [--json]");
  if (!fs.existsSync(a.src)) fail(`文件不存在: ${a.src}`);

  const pairs = [];
  for (const r of a.replace) {
    const i = r.indexOf("=>");
    if (i < 0) fail(`--replace 要写成 "旧文本=>新文本"，收到的是: ${r}`);
    pairs.push({ find: r.slice(0, i), replace: r.slice(i + 2) });
  }
  if (a.map) {
    if (!fs.existsSync(a.map)) fail(`--map 文件不存在: ${a.map}`);
    let arr;
    try {
      arr = JSON.parse(fs.readFileSync(a.map, "utf8"));
    } catch (e) {
      fail(`--map 不是合法 JSON: ${e.message}`);
    }
    if (!Array.isArray(arr)) fail("--map 的内容要是数组：[{\"find\":\"…\",\"replace\":\"…\"}]");
    for (const o of arr) {
      if (!o || typeof o.find !== "string") fail("--map 每一项都要有 find 字段");
      pairs.push({ find: o.find, replace: String(o.replace ?? "") });
    }
  }
  if (!pairs.length) fail("没给任何替换内容（--replace 或 --map）");

  const ext = path.extname(a.src).toLowerCase();
  const fmt = FORMATS[ext];
  if (!fmt)
    fail(
      `只支持 .docx / .pptx / .xlsx，收到的是 ${ext || "(没有扩展名)"}\n` +
        "老格式（.doc/.ppt/.xls）不是 ZIP，请先在 Office 里「另存为」新格式。",
    );

  const out = a.inPlace ? a.src : a.out || a.src.slice(0, -ext.length) + "-改" + ext;
  const items = readZip(fs.readFileSync(a.src));
  if (!items.some((i) => i.name === fmt.marker))
    fail(`这不像 ${fmt.label} 文件（缺 ${fmt.marker}）—— ${fmt.hint}`);

  // 改哪些部件：默认只改正文；--all-parts 连页眉页脚 / 备注页 / 工作表一起
  const targets = new Set(items.filter((i) => fmt.main.test(i.name)).map((i) => i.name));
  if (a.allParts) for (const it of items) if (fmt.extra.test(it.name)) targets.add(it.name);
  if (!targets.size)
    fail(
      `${fmt.label} 文件里没找到装文字的部件` +
        (ext === ".xlsx" ? "\n（这份表里可能全是数字/公式 —— 那些不在文本表里，改不了）" : ""),
    );

  const total = new Map(pairs.map((p) => [p.find, 0]));
  let touched = 0;
  const next = items.map((it) => {
    if (!targets.has(it.name)) return it; // ★ 原样复制压缩字节：样式/图片/编号字节级不变
    const xml = inflate(it).toString("utf8");
    const { xml: xml2, hits } = replaceInXml(xml, pairs, fmt);
    for (const [k, v] of hits) total.set(k, total.get(k) + v);
    if (xml2 === xml) return it;
    touched++;
    const body = Buffer.from(xml2, "utf8");
    const deflated = zlib.deflateRawSync(body, { level: 9 });
    return { ...it, method: 8, crc: crc32(body), usize: body.length, raw: deflated };
  });

  const replaced = [...total].filter(([, n]) => n > 0).map(([find, count]) => ({ find, count }));
  const missed = [...total].filter(([, n]) => n === 0).map(([find]) => find);

  if (!replaced.length) {
    // 一条都没命中 = 什么都没干，绝不能报成功（也不写出文件，免得客户以为改好了）
    const msg = "一处都没找到，没有生成文件。没找到的是：\n  " + missed.join("\n  ") +
      "\n提示：文字要和文档里**完全一致**（包括空格和标点）；跨段落的文字匹配不了。" +
      "\n可以先用 uking-office-read 的 read-doc.py 把原文读出来，照着原文复制。";
    if (a.json) process.stdout.write(JSON.stringify({ ok: false, error: "no_match", missed }) + "\n");
    fail(msg, 1);
  }

  if (a.inPlace) {
    const bak = a.src + ".bak";
    if (!fs.existsSync(bak)) fs.copyFileSync(a.src, bak); // 覆盖原文件前先留底，且不覆盖已有的 .bak
  }
  fs.mkdirSync(path.dirname(path.resolve(out)), { recursive: true });
  fs.writeFileSync(out, writeZip(next));

  const preserved = items.length - touched;
  if (missed.length) process.stderr.write("⚠ 这些没找到（其余已改）：\n  " + missed.join("\n  ") + "\n");
  process.stderr.write(`已改 ${replaced.reduce((s, r) => s + r.count, 0)} 处，${preserved} 个部件原样保留（样式/页眉页脚/图片未动）\n`);
  if (a.json) process.stdout.write(JSON.stringify({ ok: true, file: path.resolve(out), replaced, missed, parts_preserved: preserved }) + "\n");
  else process.stdout.write(path.resolve(out) + "\n");
}

// 被 import 时（测试）不跑主流程
if (import.meta.url === `file://${process.argv[1].replace(/\\/g, "/")}` || process.argv[1]?.endsWith("edit-office.mjs")) main();
