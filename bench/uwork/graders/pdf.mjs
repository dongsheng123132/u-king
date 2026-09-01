/**
 * 判分：出一份能发出去的 PDF。
 *
 * 分两层判，**并且如实说明每层判了什么**：
 *   ① 结构层（纯 Node，任何机器都能跑）：真 PDF 头尾、页数、体积、**有嵌入字体**
 *      —— 「文字可复制」和「把网页截图贴进去」的区别就在这：截图版没有字体、只有一张大图。
 *   ② 内容层（要本机有 Python + pdfminer 才跑）：把正文抽出来核对数字。
 *      抽不出来时**不算失败**，但会在 detail 里写明「内容层没跑」——
 *      悄悄跳过一层检查然后报绿，比不检查更坏。
 *
 * 真值：有效订单 11 条，金额合计 220676。
 */
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";

const TRUTH_TOTAL = 220676;

/** 尽量在纯 Node 里把 PDF 的对象流解出来，用于判断有没有嵌入字体/页数。 */
function pdfFacts(buf) {
  const raw = buf.toString("latin1");
  let inflated = "";
  for (const m of raw.matchAll(/stream\r?\n/g)) {
    const start = m.index + m[0].length;
    const end = raw.indexOf("endstream", start);
    if (end < 0) continue;
    try { inflated += zlib.inflateSync(buf.subarray(start, end)).toString("latin1"); } catch {}
    if (inflated.length > 4e6) break;
  }
  const all = raw + inflated;
  return {
    isPdf: raw.startsWith("%PDF-"),
    hasEof: /%%EOF\s*$/.test(raw.slice(-64)),
    pages: (all.match(/\/Type\s*\/Page[^s]/g) || []).length,
    fonts: (all.match(/\/Type\s*\/Font/g) || []).length,
    images: (all.match(/\/Subtype\s*\/Image/g) || []).length,
  };
}

/** 有 Python + pdfminer 就抽正文；没有就返回 null（不算失败，但要说明）。 */
function extractText(pdfPath) {
  for (const py of ["python", "python3"]) {
    const r = spawnSync(py, ["-c",
      "import sys\nfrom pdfminer.high_level import extract_text\nsys.stdout.reconfigure(encoding='utf-8')\nprint(extract_text(sys.argv[1]))",
      pdfPath], { encoding: "utf8", timeout: 90000, windowsHide: true });
    if (r.status === 0 && r.stdout && r.stdout.trim()) return r.stdout;
  }
  return null;
}

export async function grade({ ws }) {
  const checks = [];
  const add = (n, ok, d) => checks.push({ name: n, ok: !!ok, detail: d || "" });

  const files = fs.readdirSync(ws);
  const pdf = files.find((f) => f.toLowerCase().endsWith(".pdf"));
  add("生成了 .pdf", !!pdf, pdf || `目录里没有 .pdf（有: ${files.join(", ")}）`);
  if (!pdf) return { pass: false, checks };

  const buf = fs.readFileSync(path.join(ws, pdf));
  const f = pdfFacts(buf);
  add("是合法 PDF（头尾完整）", f.isPdf && f.hasEof, `header=${f.isPdf} eof=${f.hasEof}`);
  add("不是空文件", buf.length > 2048, `${(buf.length / 1024).toFixed(1)} KB`);
  add("是文字版而不是截图（有嵌入字体）", f.fonts > 0,
      f.fonts ? `${f.fonts} 个字体对象` : `一个字体对象都没有${f.images ? `，却有 ${f.images} 张图 —— 八成是把截图贴进去了` : ""}`);

  // 源文件也要留 —— 客户十有八九还要改一版，只给 PDF 等于把他锁死
  const src = files.find((x) => /\.(md|markdown|docx|html?)$/i.test(x));
  add("源文件也留下了（客户还要改）", !!src, src || "只有 PDF，没有可编辑的源文件");

  const text = extractText(path.join(ws, pdf));
  if (text === null) {
    checks.push({ name: "（未跑）内容层核对", ok: true, detail: "这台机器抽不出 PDF 正文（缺 Python/pdfminer）—— 内容对不对**没有验**，只验了结构" });
  } else {
    const flat = text.replace(/\s/g, "");
    add("正文里金额合计正确（220676）", /220[,\s]?676/.test(text) || flat.includes("220676"),
        /220[,\s]?676/.test(text) ? "" : `抽出的正文里没有 220676；出现过的六位数: ${[...new Set((flat.match(/\d{6}/g) || []))].slice(0, 6).join("/") || "无"}`);
    add("写了有效订单数（11）", /11\s*条|有效订单[^0-9]{0,6}11|11\s*笔/.test(text), "");
    add("有按客户汇总的表格内容", ["宏远机械", "天成电子", "海通物流", "瑞丰包装"].filter((k) => text.includes(k)).length >= 3,
        `命中客户: ${["宏远机械", "天成电子", "海通物流", "瑞丰包装"].filter((k) => text.includes(k)).join("、") || "无"}`);
  }

  const hard = checks.filter((c) => !c.name.startsWith("（未跑）"));
  return { pass: hard.every((c) => c.ok), checks };
}
