/**
 * 判分：脏 CSV → 能求和的 Excel。
 *
 * 真值是**独立算出来的**（不是从 CLI 的产物反推）：
 * 种子 销售明细.csv 有 13 行数据，其中 XS-2603 重复一次、XS-2608 状态为作废。
 * 去重 + 排除作废后 = 11 条有效记录，金额合计 = 220676。
 *
 * 🔴 合计算错**不给部分分**。这类错的特征是「格式完全正常、客户根本看不出来」——
 * 0.9.88 那次实测就栽在这儿（188790 写成 189790）。宁可判 fail 也不能让它蒙混过关。
 */
import fs from "node:fs";
import path from "node:path";
import { readXlsx } from "../lib/zip.mjs";

const TRUTH_TOTAL = 220676;
const TRUTH_ROWS = 11;

export async function grade({ ws }) {
  const checks = [];
  const add = (name, ok, detail) => checks.push({ name, ok: !!ok, detail: detail || "" });

  const xs = fs.readdirSync(ws).filter((f) => f.toLowerCase().endsWith(".xlsx"));
  const target = xs.find((f) => f.includes("汇总")) || xs[0];
  add("生成了 .xlsx", !!target, xs.length ? `目录里有: ${xs.join(", ")}` : "工作区里一个 .xlsx 都没有");
  if (!target) return { pass: false, checks };

  let book;
  try { book = readXlsx(path.join(ws, target)); }
  catch (e) { add("文件能被 Excel 解析", false, `解析失败: ${e.message}`); return { pass: false, checks }; }
  add("文件能被 Excel 解析", true, `${target}，${book.sheets.length} 张表`);

  const cells = book.sheets.flatMap((s) => s.cells);
  const nums = cells.filter((c) => c.type !== "s" && c.type !== "inlineStr" && c.value !== "" && !isNaN(+c.value));
  const numVals = nums.map((c) => +c.value);
  const texts = cells.filter((c) => c.type === "s" || c.type === "inlineStr").map((c) => String(c.value));
  const allText = texts.join("\n");

  // 数值型：金额那一列不能是文本。文本单元格在 Excel 里求和会得 0，客户拉一下鼠标就发现是废表。
  add("金额是数值单元格（能求和）", numVals.length >= TRUTH_ROWS,
      `数值单元格 ${numVals.length} 个（至少要有 ${TRUTH_ROWS} 条记录的金额）`);

  // 去重：XS-2603 只能出现一次
  const c2603 = (allText.match(/XS-2603/g) || []).length;
  add("重复订单已去重（XS-2603 只出现一次）", c2603 === 1, `出现 ${c2603} 次`);

  // 排除作废：XS-2608 不该在
  add("作废记录已排除（XS-2608 不在表里）", !/XS-2608/.test(allText), /XS-2608/.test(allText) ? "作废单还在表里" : "");

  // 有效记录条数
  const kept = ["XS-2601", "XS-2602", "XS-2603", "XS-2604", "XS-2605", "XS-2606", "XS-2607", "XS-2609", "XS-2610", "XS-2611", "XS-2612"];
  const missing = kept.filter((k) => !allText.includes(k));
  add(`保留了全部 ${TRUTH_ROWS} 条有效记录`, missing.length === 0, missing.length ? `缺: ${missing.join(",")}` : "");

  // ★ 合计。允许它写成公式（<f>SUM(...)</f> 且缓存值对）或常量，但**值必须精确等于真值**。
  const hasTotalLabel = /合计/.test(allText);
  add("有合计行", hasTotalLabel, hasTotalLabel ? "" : "找不到「合计」标签");
  const hit = numVals.includes(TRUTH_TOTAL);
  const near = numVals.filter((v) => v > TRUTH_TOTAL * 0.9 && v < TRUTH_TOTAL * 1.1 && v !== TRUTH_TOTAL);
  add(`合计精确等于 ${TRUTH_TOTAL}`, hit,
      hit ? "" : near.length ? `没找到 ${TRUTH_TOTAL}；最接近的是 ${near.join("/")} —— 算错了，客户从格式上看不出来` : `表里没有任何接近 ${TRUTH_TOTAL} 的数`);

  const formulaCells = cells.filter((c) => c.formula);
  add("（加分项）合计用公式而非手填", formulaCells.length > 0,
      formulaCells.length ? `${formulaCells.length} 个公式单元格` : "全是手填常量——客户改一行数据合计不会变");

  // 硬性项：加分项不参与 pass 判定
  const hard = checks.filter((c) => !c.name.startsWith("（加分项）"));
  return { pass: hard.every((c) => c.ok), checks };
}
