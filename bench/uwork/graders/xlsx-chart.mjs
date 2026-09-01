/**
 * 判分：带趋势图的 Excel。
 *
 * 「有没有图」这条必须**看 xlsx 内部结构**，不能看它嘴上说「已插入柱状图」。
 * 最常见的糊弄法有两种，都要判死：
 *   ① 贴一张 png 进去（`xl/media/` 有图但 `xl/charts/` 没有）—— 客户改数据图不会变；
 *   ② 用字符画/条形符号在单元格里画（`████ 44248`）—— 打印出来是笑话。
 *
 * 真值（去重 + 排除作废后按客户汇总）：
 *   宏远机械 44248 / 天成电子 91300 / 海通物流 47850 / 瑞丰包装 37278，合计 220676。
 */
import fs from "node:fs";
import path from "node:path";
import { readXlsx, readZip } from "../lib/zip.mjs";

const TRUTH = { 宏远机械: 44248, 天成电子: 91300, 海通物流: 47850, 瑞丰包装: 37278 };

export async function grade({ ws }) {
  const checks = [];
  const add = (n, ok, d) => checks.push({ name: n, ok: !!ok, detail: d || "" });

  const files = fs.readdirSync(ws);
  const target = files.find((f) => f.toLowerCase().endsWith(".xlsx"));
  add("生成了 .xlsx", !!target, target || `目录里没有 .xlsx（有: ${files.join(", ")}）`);
  if (!target) return { pass: false, checks };

  let book, entries;
  try { book = readXlsx(path.join(ws, target)); entries = readZip(path.join(ws, target)); }
  catch (e) { add("文件能被 Excel 解析", false, e.message); return { pass: false, checks }; }
  add("文件能被 Excel 解析", true, target);

  const cells = book.sheets.flatMap((s) => s.cells);
  const nums = cells.filter((c) => c.type !== "s" && c.type !== "inlineStr" && c.value !== "" && !isNaN(+c.value)).map((c) => +c.value);
  const text = cells.filter((c) => c.type === "s" || c.type === "inlineStr").map((c) => String(c.value)).join("\n");

  // 汇总值逐个核 —— 少一个客户或者错一个数都算失败
  const wrong = Object.entries(TRUTH).filter(([, v]) => !nums.includes(v)).map(([k, v]) => `${k}应为${v}`);
  add("四个客户的金额合计全部算对", wrong.length === 0,
      wrong.length ? `没找到: ${wrong.join("、")}；表里的数值是 ${[...new Set(nums)].slice(0, 12).join("/")}` : "");
  add("客户名齐全", Object.keys(TRUTH).every((k) => text.includes(k)),
      Object.keys(TRUTH).filter((k) => !text.includes(k)).join("、") || "");
  add("金额是数值单元格", nums.length >= 4, `数值单元格 ${nums.length} 个`);

  // ★ 真图表：xl/charts/chartN.xml 必须存在
  const charts = entries.filter((e) => /^xl\/charts\/chart\d+\.xml$/.test(e.name));
  const media = entries.filter((e) => e.name.startsWith("xl/media/"));
  add("插了 Excel 原生图表（不是贴图、不是字符画）", charts.length > 0,
      charts.length ? `${charts.length} 张图表`
        : media.length ? `xl/charts/ 是空的，但 xl/media/ 里有 ${media.length} 个图片 —— 贴了张图，客户改数据图不会变`
        : /[█▇▆▅▄▃▂▁■◼#*]{3,}/.test(text) ? "单元格里是字符画，不是图表"
        : "整个文件里没有任何图表");
  if (!charts.length) return { pass: false, checks };

  const cx = charts[0].data.toString("utf8");
  // 同理，三种图形都要放过命名空间前缀（我们写 `<c:barChart>`，ExcelJS 写 `<barChart>`）
  const kind = /(?:\w+:)?barChart/.test(cx) ? "柱状"
    : /(?:\w+:)?lineChart/.test(cx) ? "折线"
    : /(?:\w+:)?pieChart/.test(cx) ? "饼" : "未知";
  add("图表类型是柱状图", kind === "柱状", `实际是${kind}图`);

  // 图表必须**引用工作表单元格**，不是把数字硬写进图表 XML —— 后者一样是「改数据图不动」
  // 🔴 命名空间前缀**必须当成可选的**：我们自家 gen-xlsx 写 `<c:f>`，而 ExcelJS 用默认
  // 命名空间写 `<f>`。只认 `<c:f>` 会把一份完全正确的 ExcelJS 产物判成「数据硬编码」。
  const refs = [...cx.matchAll(/<(?:\w+:)?f>([^<]+)<\/(?:\w+:)?f>/g)].map((m) => m[1]);
  add("图表引用的是单元格区域（改数据图会跟着变）", refs.length > 0,
      refs.length ? refs.slice(0, 3).join(" , ") : "图表里没有任何 <c:f> 单元格引用，数据是硬编码进去的");

  // 三件套齐不齐：少一环 Excel 会报「文件已损坏」而不是「图没显示」
  const hasDrawing = entries.some((e) => /^xl\/drawings\/drawing\d+\.xml$/.test(e.name));
  const hasDrawRels = entries.some((e) => /^xl\/drawings\/_rels\//.test(e.name));
  const hasSheetRels = entries.some((e) => /^xl\/worksheets\/_rels\//.test(e.name));
  add("图表关系链完整（drawing + 两层 rels）", hasDrawing && hasDrawRels && hasSheetRels,
      `drawing=${hasDrawing} drawingRels=${hasDrawRels} sheetRels=${hasSheetRels}`);

  return { pass: checks.every((c) => c.ok), checks };
}
