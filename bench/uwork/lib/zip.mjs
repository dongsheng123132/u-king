/**
 * 最小 ZIP 读取器 —— .xlsx / .docx / .pptx 都是 ZIP，判分要看里面的 XML。
 * 只依赖 node:zlib，不引任何 npm 包（判分器要能在客户机上原地跑）。
 *
 * 故意自己解而不是调 `unzip`：Windows 客户机上没有 unzip，且我们要断言
 * **压缩方式和原始字节**（uking-office-edit 的「未改部件字节级相同」全靠这个）。
 */
import fs from "node:fs";
import zlib from "node:zlib";

/** 读出 ZIP 全部条目：{name, method, crc32, compressed, size, data(Buffer), raw(Buffer 压缩原文)} */
export function readZip(file) {
  const buf = fs.readFileSync(file);
  // End of Central Directory：从尾部往前找签名（注释最长 65535）
  let eo = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 65557); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) { eo = i; break; }
  }
  if (eo < 0) throw new Error("不是合法 ZIP（找不到 EOCD）——文件可能是空的或被截断");
  const count = buf.readUInt16LE(eo + 10);
  let off = buf.readUInt32LE(eo + 16);
  const out = [];
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(off) !== 0x02014b50) throw new Error("中央目录项签名不对，文件已损坏");
    const method = buf.readUInt16LE(off + 10);
    const crc32 = buf.readUInt32LE(off + 16);
    const compressed = buf.readUInt32LE(off + 20);
    const size = buf.readUInt32LE(off + 24);
    const nameLen = buf.readUInt16LE(off + 28);
    const extraLen = buf.readUInt16LE(off + 30);
    const cmtLen = buf.readUInt16LE(off + 32);
    const lho = buf.readUInt32LE(off + 42);
    const name = buf.toString("utf8", off + 46, off + 46 + nameLen);
    // 本地头：真正的数据起点要按本地头里的 name/extra 长度算，中央目录那份可能不同
    const lNameLen = buf.readUInt16LE(lho + 26);
    const lExtraLen = buf.readUInt16LE(lho + 28);
    const dataStart = lho + 30 + lNameLen + lExtraLen;
    const raw = buf.subarray(dataStart, dataStart + compressed);
    let data;
    try { data = method === 0 ? Buffer.from(raw) : zlib.inflateRawSync(raw); }
    catch (e) { data = Buffer.alloc(0); }
    out.push({ name, method, crc32, compressed, size, data, raw: Buffer.from(raw) });
    off += 46 + nameLen + extraLen + cmtLen;
  }
  return out;
}

/**
 * XML 实体还原。**数字字符引用那一半是被真 bug 逼出来的**：
 * ExcelJS 把中文写成 `&#23458;&#25143;` 而不是 `客户`，不解码的话判分器会说「客户名缺失」，
 * 把一份**完全正确**的产物判死。判分器只拿自家生成器的输出验过 = 只验了自己的假设。
 */
export function unxml(s) {
  return String(s)
    .replace(/&#x([0-9a-fA-F]+);/g, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(+d))
    .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'").replace(/&amp;/g, "&");
}

/** 取某个条目的文本内容（找不到返回 ""）。支持前缀匹配（`xl/worksheets/`）。 */
export function entryText(entries, nameOrPrefix) {
  const e = entries.find((x) => x.name === nameOrPrefix) || entries.find((x) => x.name.startsWith(nameOrPrefix));
  return e ? e.data.toString("utf8") : "";
}

/** 所有匹配前缀的条目文本拼起来（多张 sheet / 多页 slide 时用）。 */
export function allText(entries, prefix) {
  return entries.filter((x) => x.name.startsWith(prefix)).map((x) => x.data.toString("utf8")).join("\n");
}

/**
 * 从 .xlsx 里抽出每张表的单元格：[{sheet, cells:[{ref, type, value, formula}]}]。
 * 共享字符串（sharedStrings.xml）会被还原，否则文本单元格只能看到索引号。
 */
export function readXlsx(file) {
  const z = readZip(file);
  const shared = [];
  const ss = entryText(z, "xl/sharedStrings.xml");
  if (ss) for (const m of ss.matchAll(/<si>([\s\S]*?)<\/si>/g)) {
    shared.push(unxml([...m[1].matchAll(/<t[^>]*>([\s\S]*?)<\/t>/g)].map((t) => t[1]).join("")));
  }
  const sheets = [];
  const wb = entryText(z, "xl/workbook.xml");
  const names = [...wb.matchAll(/<sheet[^>]*name="([^"]*)"/g)].map((m) => m[1]);
  const files = z.filter((x) => /^xl\/worksheets\/sheet\d+\.xml$/.test(x.name))
    .sort((a, b) => (+a.name.match(/(\d+)/)[1]) - (+b.name.match(/(\d+)/)[1]));
  files.forEach((f, i) => {
    const xml = f.data.toString("utf8");
    const cells = [];
    for (const m of xml.matchAll(/<c\s([^>]*?)(?:\/>|>([\s\S]*?)<\/c>)/g)) {
      const attrs = m[1], inner = m[2] || "";
      const ref = (attrs.match(/r="([^"]*)"/) || [])[1] || "";
      const t = (attrs.match(/t="([^"]*)"/) || [])[1] || "n";
      const formula = (inner.match(/<f[^>]*>([\s\S]*?)<\/f>/) || [])[1] || null;
      let value = (inner.match(/<v>([\s\S]*?)<\/v>/) || [])[1];
      if (t === "s" && value !== undefined) value = shared[+value] ?? "";
      else if (t === "inlineStr") value = unxml([...inner.matchAll(/<t[^>]*>([\s\S]*?)<\/t>/g)].map((x) => x[1]).join(""));
      cells.push({ ref, type: t, value: value === undefined ? "" : value, formula });
    }
    sheets.push({ name: names[i] || `sheet${i + 1}`, cells });
  });
  return { sheets, entries: z };
}

/** .docx 全文（段落间加换行，便于按内容断言）。 */
export function docxText(file) {
  const z = readZip(file);
  const xml = entryText(z, "word/document.xml");
  // 走 unxml：`&#XXXX;` 这种数字字符引用也要还原（python-docx / ExcelJS 系都爱这么写中文）
  return unxml(xml.replace(/<\/w:p>/g, "\n").replace(/<[^>]+>/g, ""));
}

/** .pptx 每页文字：[{slide:1, text}]。 */
export function pptxSlides(file) {
  const z = readZip(file);
  return z.filter((x) => /^ppt\/slides\/slide\d+\.xml$/.test(x.name))
    .sort((a, b) => (+a.name.match(/(\d+)/)[1]) - (+b.name.match(/(\d+)/)[1]))
    .map((x, i) => ({
      slide: i + 1,
      text: x.data.toString("utf8").replace(/<\/a:p>/g, "\n").replace(/<[^>]+>/g, "").trim(),
    }));
}
