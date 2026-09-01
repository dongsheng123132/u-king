#!/usr/bin/env node
/**
 * gen-xlsx.mjs —— 纯 std（零 npm 依赖）把 CSV / 结构化数据生成**真 .xlsx**（Excel/WPS 能开）。
 *
 * 客户机只有便携 Node、装不了 openpyxl/exceljs，所以手搓 SpreadsheetML + ZIP(STORE)。
 * 不含任何 Key、不联网，纯本地把表格数据写成 Excel。
 *
 * 用法（AI 经 run_command 调）：
 *   node gen-xlsx.mjs --csv data.csv --out 表格.xlsx --json          # 单表，从 CSV
 *   node gen-xlsx.mjs --in book.json --out 报表.xlsx --json          # 多表，见下
 *
 * book.json 结构（--in）：
 *   { "sheets": [
 *       { "name": "Sheet1", "rows": [ ["表头A","表头B"], [1, 2], ["文本", 3.14] ] },
 *       { "name": "汇总",   "rows": [ ["月份","销量"], ["1月", 120] ] } ] }
 *   —— 首行自动加粗当表头；数字自动识别为数值单元格（可参与 Excel 求和）。
 *
 * 输出：成功打印 .xlsx 绝对路径；--json 时打印 {"ok":true,"file":"..."}。
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
function fail(m) { if (asJson) console.log(JSON.stringify({ ok: false, error: String(m) })); else console.error("[gen-xlsx] 失败:", m); process.exit(1); }

// ---------- 简易 CSV 解析（支持双引号包裹字段、字段内逗号/换行） ----------
function parseCsv(text) {
  const rows = []; let row = [], field = "", inq = false;
  const s = text.replace(/\r\n/g, "\n");
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (inq) {
      if (c === '"') { if (s[i + 1] === '"') { field += '"'; i++; } else inq = false; }
      else field += c;
    } else if (c === '"') inq = true;
    else if (c === ",") { row.push(field); field = ""; }
    else if (c === "\n") { row.push(field); rows.push(row); row = []; field = ""; }
    else field += c;
  }
  if (field.length || row.length) { row.push(field); rows.push(row); }
  return rows.filter((r) => !(r.length === 1 && r[0] === ""));
}

// ---------- 取数据 ----------
let book;
try {
  if (args.csv) book = { sheets: [{ name: args.name || "Sheet1", rows: parseCsv(fs.readFileSync(String(args.csv), "utf8")) }] };
  else if (args.in) book = JSON.parse(fs.readFileSync(String(args.in), "utf8"));
  else fail("需要 --csv <file.csv> 或 --in <book.json>");
} catch (e) { fail("读数据失败: " + e.message); }
if (!book || !Array.isArray(book.sheets) || !book.sheets.length) fail("没有 sheets");

// 🔴 先剥 XML 1.0 非法控制字符，再转义。转义只管 `& < > "`，管不了 0x0B/0x1F 这类字节 ——
// 它们没有实体写法，进了 sharedStrings.xml 就是非法 XML，Excel 直接报「文件已损坏」，
// 而脚本照样返回 `ok:true`。OCR 结果 / 从 PDF 复制的文本 / 旧系统导出的 CSV 里很常见。
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

// ---------- 列字母（0→A, 26→AA） ----------
function colLetter(n) { let s = ""; n++; while (n > 0) { const m = (n - 1) % 26; s = String.fromCharCode(65 + m) + s; n = Math.floor((n - 1) / 26); } return s; }
/**
 * 「这个值该写成数值单元格吗」。
 *
 * 🔴 只判「长得像数字」是不够的 —— 写成数值等于让 IEEE754 双精度**重新表达**这个值，
 * 有两类值会被悄悄改写，而它们恰恰是国内表格里最常见的字段：
 *
 *   007            → 7                      工号/邮编/编号的前导零没了
 *   身份证 18 位    → 末几位变 000           双精度只有 15~17 位有效数字
 *   银行卡/长订单号 → 同上
 *
 * 「悄悄」是这个 bug 最坏的地方：没有报错、没有告警，表格看着正常，
 * 直到有人拿这份表去对账。宁可这一列不能求和，也不能算错。
 */
const isNum = (v) => {
  if (typeof v === "number") return Number.isFinite(v);
  const s = String(v).trim();
  if (!/^-?\d+(\.\d+)?$/.test(s)) return false;
  const body = s.replace(/^-/, "");
  if (/^0\d/.test(body)) return false; // 前导零有信息量（007 / 010000），写成数值就没了
  return body.replace(".", "").replace(/^0+/, "").length <= 15; // 有效位超 15 位存不下
};

// ---------- sharedStrings（去重字符串表） ----------
const ssIndex = new Map(); const ssList = [];
function ssIdx(str) { if (ssIndex.has(str)) return ssIndex.get(str); const i = ssList.length; ssIndex.set(str, i); ssList.push(str); return i; }

// ---------- 图表 ----------
// 管理层要的报表基本都带一张趋势图；没有图的表交上去，客户第一句话就是「能加个图吗」。
// 手搓 DrawingML 而不是引 exceljs：客户机只有便携 Node，装不了 npm 包（全包一贯口径）。
// 出的是**可编辑的原生 Excel 图表**，不是贴一张图片 —— 客户改数据图会跟着变。
const CHART_NS = 'xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
/** `A2:A13` → `'销售'!$A$2:$A$13`（图表引用必须是带表名的绝对地址，相对地址 Excel 打不开）。 */
function absRef(sheetName, range) {
  const abs = String(range).replace(/\$/g, "").replace(/([A-Za-z]+)(\d+)/g, "$$$1$$$2");
  return `'${String(sheetName).replace(/'/g, "''")}'!${abs}`;
}
function chartXml(sh, chart) {
  const type = String(chart.type || "line").toLowerCase();
  const series = (chart.series || []).map((s, i) => {
    const cat = chart.categories
      ? `<c:cat><c:strRef><c:f>${esc(absRef(sh.name, chart.categories))}</c:f></c:strRef></c:cat>` : "";
    return `<c:ser><c:idx val="${i}"/><c:order val="${i}"/>` +
      (s.name ? `<c:tx><c:v>${esc(s.name)}</c:v></c:tx>` : "") +
      (type === "line" ? `<c:marker><c:symbol val="circle"/></c:marker>` : "") +
      cat +
      `<c:val><c:numRef><c:f>${esc(absRef(sh.name, s.values))}</c:f></c:numRef></c:val>` +
      `</c:ser>`;
  }).join("");
  // 饼图没有坐标轴；折线/柱状必须给成对的 axId，缺一个 Excel 报「文件已损坏」
  const axes = type === "pie" ? "" :
    `<c:axId val="111111111"/><c:axId val="222222222"/>`;
  const plot =
    type === "pie" ? `<c:pieChart><c:varyColors val="1"/>${series}</c:pieChart>` :
    type === "bar" ? `<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>${series}${axes}</c:barChart>` :
    `<c:lineChart><c:grouping val="standard"/><c:marker val="1"/>${series}${axes}</c:lineChart>`;
  const axParts = type === "pie" ? "" :
    `<c:catAx><c:axId val="111111111"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>` +
    `<c:axPos val="b"/><c:crossAx val="222222222"/></c:catAx>` +
    `<c:valAx><c:axId val="222222222"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>` +
    `<c:axPos val="l"/><c:crossAx val="111111111"/></c:valAx>`;
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
    `<c:chartSpace ${CHART_NS}><c:chart>` +
    (chart.title ? `<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>${esc(chart.title)}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>` : "") +
    `<c:plotArea><c:layout/>${plot}${axParts}</c:plotArea>` +
    `<c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend>` +
    `<c:plotVisOnly val="1"/></c:chart></c:chartSpace>`;
}
/** 图表在表上的落位。默认放数据右边一点，不压着数据。 */
function drawingXml(anchorCol, anchorRow) {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
    `<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">` +
    `<xdr:twoCellAnchor><xdr:from><xdr:col>${anchorCol}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>${anchorRow}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>` +
    `<xdr:to><xdr:col>${anchorCol + 8}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>${anchorRow + 16}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>` +
    `<xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>` +
    `<xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm>` +
    `<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId1"/></a:graphicData></a:graphic>` +
    `</xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>`;
}
/** `J2` → {col:9, row:1}（0 基）。给不出就落在数据右边两列。 */
function parseAnchor(a, cols) {
  const m = /^([A-Za-z]+)(\d+)$/.exec(String(a || ""));
  if (!m) return { col: cols + 1, row: 1 };
  let c = 0; for (const ch of m[1].toUpperCase()) c = c * 26 + (ch.charCodeAt(0) - 64);
  return { col: c - 1, row: +m[2] - 1 };
}
/** 有图的 sheet 的下标（每个 sheet 最多一张图，够用；要多张再说，别提前设计）。 */
const chartSheets = book.sheets.map((sh, i) => (sh.chart && sh.chart.series && sh.chart.series.length ? i : -1)).filter((i) => i >= 0);

// ---------- 每个 sheet 的 worksheet xml ----------
const sheetXmls = book.sheets.map((sh) => {
  const rows = Array.isArray(sh.rows) ? sh.rows : [];
  const rowXml = rows.map((r, ri) => {
    const cells = (Array.isArray(r) ? r : [r]).map((v, ci) => {
      const ref = `${colLetter(ci)}${ri + 1}`;
      const sAttr = ri === 0 ? ' s="1"' : ""; // 首行加粗
      if (v == null || v === "") return `<c r="${ref}"${sAttr}/>`;
      if (isNum(v)) return `<c r="${ref}"${sAttr}><v>${Number(v)}</v></c>`;
      return `<c r="${ref}"${sAttr} t="s"><v>${ssIdx(String(v))}</v></c>`;
    }).join("");
    return `<row r="${ri + 1}">${cells}</row>`;
  }).join("");
  // `<drawing>` 必须排在 `<sheetData>` 之后 —— schema 是有序的，放前面 Excel 直接报「已损坏」
  const hasChart = sh.chart && sh.chart.series && sh.chart.series.length;
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
    `<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">` +
    `<sheetData>${rowXml}</sheetData>` +
    (hasChart ? `<drawing r:id="rIdDr"/>` : "") +
    `</worksheet>`;
});

const sharedStrings =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="${ssList.length}" uniqueCount="${ssList.length}">` +
  ssList.map((s) => `<si><t xml:space="preserve">${esc(s)}</t></si>`).join("") + `</sst>`;

const styles =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">` +
  `<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts>` +
  `<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>` +
  `<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>` +
  `<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>` +
  `<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>` +
  `<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>` +
  `</styleSheet>`;

const workbook =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">` +
  `<sheets>` +
  book.sheets.map((sh, i) => `<sheet name="${esc(sh.name || `Sheet${i + 1}`).slice(0, 31)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join("") +
  `</sheets></workbook>`;

const workbookRels =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
  book.sheets.map((_, i) => `<Relationship Id="rId${i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`).join("") +
  `<Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>` +
  `<Relationship Id="rIdSS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>` +
  `</Relationships>`;

const rootRels =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
  `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>` +
  `</Relationships>`;

const contentTypes =
  `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
  `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
  `<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>` +
  `<Default Extension="xml" ContentType="application/xml"/>` +
  `<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>` +
  book.sheets.map((_, i) => `<Override PartName="/xl/worksheets/sheet${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`).join("") +
  `<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>` +
  `<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>` +
  chartSheets.map((i) =>
    `<Override PartName="/xl/drawings/drawing${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>` +
    `<Override PartName="/xl/charts/chart${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>`).join("") +
  `</Types>`;

const entries = [
  { name: "[Content_Types].xml", data: contentTypes },
  { name: "_rels/.rels", data: rootRels },
  { name: "xl/workbook.xml", data: workbook },
  { name: "xl/_rels/workbook.xml.rels", data: workbookRels },
  { name: "xl/styles.xml", data: styles },
  { name: "xl/sharedStrings.xml", data: sharedStrings },
];
sheetXmls.forEach((xml, i) => entries.push({ name: `xl/worksheets/sheet${i + 1}.xml`, data: xml }));

// 图表三件套：chart（数据引用）→ drawing（落在表上哪儿）→ 两层 rels 把它们串起来。
// 少任何一环 Excel 都报「文件已损坏」而不是「图没显示」——所以别只补一半。
for (const i of chartSheets) {
  const sh = book.sheets[i];
  const cols = Math.max(...(sh.rows || [[]]).map((r) => (Array.isArray(r) ? r.length : 1)), 1);
  const at = parseAnchor(sh.chart.anchor, cols);
  entries.push({ name: `xl/charts/chart${i + 1}.xml`, data: chartXml(sh, sh.chart) });
  entries.push({ name: `xl/drawings/drawing${i + 1}.xml`, data: drawingXml(at.col, at.row) });
  entries.push({
    name: `xl/drawings/_rels/drawing${i + 1}.xml.rels`,
    data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
      `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
      `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart${i + 1}.xml"/>` +
      `</Relationships>`,
  });
  entries.push({
    name: `xl/worksheets/_rels/sheet${i + 1}.xml.rels`,
    data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
      `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
      `<Relationship Id="rIdDr" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing${i + 1}.xml"/>` +
      `</Relationships>`,
  });
}

const outPath = path.resolve(String(args.out || "表格.xlsx"));
try { fs.mkdirSync(path.dirname(outPath), { recursive: true }); fs.writeFileSync(outPath, zipStore(entries)); }
catch (e) { fail("写 .xlsx 失败: " + e.message); }
const totalRows = book.sheets.reduce((a, s) => a + (s.rows ? s.rows.length : 0), 0);
if (asJson) console.log(JSON.stringify({ ok: true, file: outPath, sheets: book.sheets.length, rows: totalRows }));
else console.log(`已生成 Excel（${book.sheets.length} 表 / ${totalRows} 行）：${outPath}`);
