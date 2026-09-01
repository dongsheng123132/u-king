#!/usr/bin/env node
/**
 * 生成「文档理解」跑道的合成 PDF 夹具。
 *
 *   node bench/gen-doc-fixtures.mjs
 *
 * 和图片跑道同一套纪律：全合成、零隐私、ground truth 由我们精确控制。
 *
 * 为什么表格要单独测：图片跑道的判分是「这串字有没有出现」，
 * 但表格的信息**不在字里，在行列关系里**。
 * `第3行第4列=8640` 这个事实，PDF 抽出来的纯文本流里一个字节都没有 ——
 * 数字全在、关系全丢，而「字都在」会让任何 needle 判分显示满分。
 * 所以这里的题目全是**必须知道行列归属才能答对**的。
 */
import { chromium } from 'playwright';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(here, 'fixtures');
fs.mkdirSync(outDir, { recursive: true });
const CJK = `"Microsoft YaHei","PingFang SC","Noto Sans CJK SC",sans-serif`;

// 四个客户 × 四个季度。数字互不相同，答错一眼看得出来。
const ROWS = [
  { name: '示例客户甲', q: [1200, 1850, 2400, 3190], region: '华东' },
  { name: '示例客户乙', q: [980, 1120, 8640, 1450], region: '华东' },
  { name: '示例客户丙', q: [3300, 2750, 1980, 2210], region: '华南' },
  { name: '示例客户丁', q: [450, 7720, 610, 880], region: '华南' },
];
const sum = (a) => a.reduce((x, y) => x + y, 0);

const report = `<!doctype html><meta charset="utf-8">
<style>
  body{font-family:${CJK};font-size:12pt;color:#111;margin:0}
  table{border-collapse:collapse;width:100%;font-size:10.5pt}
  th,td{border:1px solid #444;padding:5px 8px}
  th{background:#eee}
  td.num{text-align:right}
  .two-col{column-count:2;column-gap:28px;font-size:10.5pt;line-height:1.75;text-align:justify}
  h1{font-size:19pt;margin:0 0 4px} h2{font-size:13pt;margin:20px 0 8px}
</style>
<body>
<h1>示例年度经营分析报告（合成数据）</h1>
<div style="color:#666;font-size:10pt">文档编号 DOC-REF-3f81c5 · 编制日期 2024年11月05日</div>

<h2>一、分客户分季度收入（单位：元）</h2>
<table>
  <tr><th rowspan="2">大区</th><th rowspan="2">客户</th><th colspan="4">季度收入</th><th rowspan="2">全年合计</th></tr>
  <tr><th>Q1</th><th>Q2</th><th>Q3</th><th>Q4</th></tr>
  ${(() => {
    let html = ''; let i = 0;
    for (const region of ['华东', '华南']) {
      const rs = ROWS.filter((r) => r.region === region);
      rs.forEach((r, k) => {
        html += `<tr>${k === 0 ? `<td rowspan="${rs.length}">${region}</td>` : ''}` +
          `<td>${r.name}</td>` + r.q.map((v) => `<td class="num">${v.toLocaleString()}</td>`).join('') +
          `<td class="num">${sum(r.q).toLocaleString()}</td></tr>`;
        i++;
      });
    }
    return html;
  })()}
  <tr><th colspan="2">合计</th>
    ${[0, 1, 2, 3].map((c) => `<td class="num">${ROWS.reduce((s, r) => s + r.q[c], 0).toLocaleString()}</td>`).join('')}
    <td class="num">${ROWS.reduce((s, r) => s + sum(r.q), 0).toLocaleString()}</td></tr>
</table>

<h2>二、经营说明（双栏排版）</h2>
<div class="two-col">
<p>本节为双栏排版，用于检验版面还原是否会把左右两栏的句子交叉串行。左栏第一句的锚点是 COL-LEFT-9d2e。报告期内，收入结构较上一周期发生变化，季度之间的波动主要来自单笔大额订单的确认时点差异，而非客户数量的增减。</p>
<p>成本端保持稳定，毛利率的季度差异主要由产品组合决定。右栏的锚点是 COL-RIGHT-6b71。若版面还原正确，这两个锚点应分别出现在各自的段落里，且左栏整段文字不应被右栏的句子打断。</p>
</div>

<h2>三、核对项</h2>
<p style="font-size:10.5pt;line-height:1.9">
☑ 已核对分客户金额　　☐ 已核对分区域金额　　☑ 已核对全年合计　　☐ 已提交审计
</p>
<div style="margin-top:26px;font-size:9pt;color:#666;border-top:1px solid #ccc;padding-top:6px">
  页脚校验：PAGE-FOOT-c4a903　　第 1 页 / 共 1 页
</div>
</body>`;

// 第二份：**稀疏表**。这才是压平真正会死的地方 ——
// 空单元格在文本流里不留任何字符，于是「少了哪一列」无从判断，整行错位；
// 而下游模型会拿邻格的数当答案，**答得斩钉截铁**。report.pdf 那张密表
// 每行数值连续，强模型能重建，测不出差距。
const sparse = `<!doctype html><meta charset="utf-8">
<style>
  body{font-family:${CJK};font-size:12pt;margin:0}
  table{border-collapse:collapse;width:100%;font-size:10.5pt}
  th,td{border:1px solid #444;padding:5px 8px;vertical-align:top}
  th{background:#eee}
  td.num{text-align:right}
</style>
<body>
<h1 style="font-size:18pt;margin:0 0 10px">示例项目预算执行表（合成数据）</h1>
<table>
  <tr><th>项目</th><th>负责人</th><th>预算</th><th>已用</th><th>备注</th></tr>
  <tr><td>甲项目</td><td>张一</td><td class="num">12,000</td><td class="num"></td><td>未启动</td></tr>
  <tr><td>乙项目</td><td></td><td class="num">8,500</td><td class="num">3,200</td><td></td></tr>
  <tr><td>丙项目</td><td>王三</td><td class="num"></td><td class="num">4,100</td><td>需补预算</td></tr>
  <tr><td>丁项目</td><td>李四</td><td class="num">6,300</td><td class="num">6,300</td><td>已结项<br>超支风险解除</td></tr>
</table>
<p style="font-size:10pt;color:#666;margin-top:18px">注：空白表示该项尚未填报，不代表数值为零。校验码 SPARSE-2b7f40。</p>
</body>`;

const browser = await chromium.launch();
for (const [name, html] of [['report', report], ['report-sparse', sparse]]) {
  const page = await browser.newPage();
  await page.setContent(html);
  await page.pdf({ path: path.join(outDir, `${name}.pdf`), format: 'A4',
    margin: { top: '16mm', bottom: '14mm', left: '15mm', right: '15mm' }, printBackground: true });
  await page.close();
}
const file = path.join(outDir, 'report.pdf');
await browser.close();

// 把 ground truth 一并算出来打印，省得手抄错
console.log(`${file}  ${(fs.statSync(file).size / 1024).toFixed(0)}KB`);
console.log('\n== ground truth（结构相关）==');
console.log(`乙的Q3 = ${ROWS[1].q[2].toLocaleString()}   （纯文本流里这个数字在，但"是乙的Q3"这个事实不在）`);
console.log(`丁的Q2 = ${ROWS[3].q[1].toLocaleString()}`);
console.log(`丙全年合计 = ${sum(ROWS[2].q).toLocaleString()}`);
console.log(`Q4 列合计 = ${ROWS.reduce((s, r) => s + r.q[3], 0).toLocaleString()}`);
console.log(`总计 = ${ROWS.reduce((s, r) => s + sum(r.q), 0).toLocaleString()}`);
console.log(`华南大区包含 = ${ROWS.filter((r) => r.region === '华南').map((r) => r.name).join('、')}（靠 rowspan 合并单元格表达）`);
