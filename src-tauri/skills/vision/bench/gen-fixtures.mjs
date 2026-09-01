#!/usr/bin/env node
/**
 * 生成识图跑道的合成夹具（PNG）。
 *
 *   node bench/gen-fixtures.mjs
 *
 * 为什么是合成的：跑道要把图发给多家外部视觉模型，用真实文件（营业执照/账单/截图）
 * 等于把里面的身份证号、公司名、订单号送出去。夹具里全是编的数据，
 * 于是这份跑道可以随技能一起分发、谁都能复跑，ground truth 也由我们精确控制。
 *
 * 两个场景，对应两类真实痛点：
 *   license  —— 字段抽取（表单/证照/发票）：值必须一字不差
 *   checkout —— 大图小字（2400px 宽，关键文字 13px）：通用识图最容易整段丢
 */
import { chromium } from 'playwright';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(here, 'fixtures');
fs.mkdirSync(outDir, { recursive: true });

const CJK = `"Microsoft YaHei","PingFang SC","Noto Sans CJK SC",sans-serif`;

const license = `<!doctype html><meta charset="utf-8">
<body style="margin:0;width:1240px;height:880px;font-family:${CJK};background:#fdfbf3">
<div style="border:6px double #8a7a3a;margin:26px;height:800px;padding:28px 44px;position:relative">
  <h1 style="text-align:center;font-size:46px;letter-spacing:14px;color:#8a6d1f;margin:6px 0 2px">营业执照</h1>
  <div style="text-align:center;color:#8a6d1f;font-size:17px;margin-bottom:26px">（副本 2-1）</div>
  <table style="width:100%;font-size:19px;line-height:2.1;border-collapse:collapse">
    <tr><td style="width:150px;color:#333">统一社会信用代码</td><td style="font-weight:600">91330100MA2TESTX7Q</td>
        <td style="width:110px;color:#333">注册资本</td><td>人民币叁佰捌拾万元</td></tr>
    <tr><td style="color:#333">名&nbsp;&nbsp;&nbsp;&nbsp;称</td><td>测试样例贸易（示例市）有限公司</td>
        <td style="color:#333">成立日期</td><td>2024年07月19日</td></tr>
    <tr><td style="color:#333">类&nbsp;&nbsp;&nbsp;&nbsp;型</td><td>有限责任公司(自然人独资)</td>
        <td style="color:#333">登记机关</td><td>示例市市场监督管理局</td></tr>
    <tr><td style="color:#333">法定代表人</td><td>张示例</td>
        <td style="color:#333">经营期限</td><td>2024年07月19日 至 长期</td></tr>
    <tr><td style="color:#333;vertical-align:top">住&nbsp;&nbsp;&nbsp;&nbsp;所</td>
        <td colspan="3">示例省示例市样例区演示路417号B座1203室</td></tr>
    <tr><td style="color:#333;vertical-align:top">经营范围</td><td colspan="3" style="font-size:17px;line-height:1.75">
      一般项目：技术服务、技术开发、技术咨询；软件开发；信息系统集成服务；数据处理服务；
      企业管理咨询；广告设计、代理（除依法须经批准的项目外，凭营业执照依法自主开展经营活动）。</td></tr>
  </table>
  <div style="position:absolute;right:70px;bottom:70px;width:150px;height:150px;border:4px solid #c0392b;
       border-radius:50%;color:#c0392b;display:flex;align-items:center;justify-content:center;
       text-align:center;font-size:15px;line-height:1.4;opacity:.85">示例市<br>市场监督<br>管理局</div>
</div></body>`;

const checkout = `<!doctype html><meta charset="utf-8">
<body style="margin:0;width:2400px;height:1180px;font-family:${CJK};background:#fff">
  <!-- 浏览器外壳：地址栏 13px，占整幅宽度的千分之几 —— 这就是「大图小字」 -->
  <div style="height:44px;background:#f1f3f4;display:flex;align-items:center;padding:0 18px;gap:14px;font-size:13px;color:#5f6368">
    <span>←</span><span>→</span><span>⟳</span>
    <div style="flex:1;background:#fff;border-radius:14px;padding:5px 14px;font-size:13px;color:#202124">
      https://demo-cashier.example.com/std/auth.htm?payOrderId=7f3a91c204be48d1a6e5</div>
  </div>
  <div style="height:26px;background:#6b6b6b;color:#eee;font-size:12px;display:flex;
       align-items:center;justify-content:flex-end;padding-right:60px">
    你好 dm***@example.com，欢迎使用示例支付！&nbsp;&nbsp;|&nbsp;&nbsp;常见问题</div>
  <div style="padding:26px 0 0 300px">
    <div style="font-size:26px;font-weight:700;color:#1677ff">示例支付 <span style="color:#999;font-weight:400;font-size:17px">| 我的收银台</span></div>
  </div>
  <div style="padding:26px 300px 0">
    <div style="font-size:14px;color:#666">正在使用即时到账交易 [?]</div>
    <div style="margin-top:9px;font-size:15px">
      <b>演示服务充值 ¥1</b>
      <span style="margin-left:34px;color:#333">收款方：样例网络（示例）科技有限公司</span>
      <span style="float:right;color:#ff5000;font-size:27px;font-weight:700">1.00 <span style="font-size:14px;color:#666">元</span></span>
    </div>
    <div style="margin-top:20px;border:1px solid #e6e6e6;height:600px;position:relative;background:#fff">
      <div style="text-align:center;padding-top:78px;font-size:14px;color:#666">扫一扫付款（元）</div>
      <div style="text-align:center;font-size:34px;color:#ff5000;font-weight:700">1.00</div>
      <div style="width:196px;height:196px;margin:22px auto;background:
           repeating-conic-gradient(#000 0 25%, #fff 0 50%) 0 0/22px 22px;border:9px solid #fff;outline:1px solid #ddd"></div>
      <div style="text-align:center;font-size:13px;color:#333;line-height:1.6">打开手机示例支付<br>扫一扫继续付款</div>
      <div style="width:470px;margin:26px auto 0;background:#f2fbe9;border:1px solid #d6ecc2;padding:13px 17px;
           font-size:13px;color:#333;line-height:1.7">
        <span style="color:#ff5000">620***@example.com</span> 已创建订单，请在手机示例支付上完成付款</div>
    </div>
    <div style="text-align:center;margin-top:22px;font-size:12px;color:#999">ICP证：示字B2-20240917</div>
  </div>
</body>`;

// 长截图：真实痛点里最狠的一类。总像素 2400×4600 ≈ 1100 万，
// 远超各家视觉模型的输入上限，必被强制降采样 —— 13px 的字降完就没了。
// 关键信息故意分散在顶 / 中 / 底，专治「只看了前半张」。
const longpage = `<!doctype html><meta charset="utf-8">
<body style="margin:0;width:2400px;font-family:${CJK};background:#fff;font-size:13px;color:#222">
  <div style="height:44px;background:#f1f3f4;display:flex;align-items:center;padding:0 18px">
    <div style="flex:1;background:#fff;border-radius:14px;padding:5px 14px">
      https://demo-console.example.com/orders/list?traceId=a83f5e17d94c</div></div>
  <div style="padding:20px 60px"><h2 style="font-size:22px">订单流水（演示数据）</h2>
  <table style="width:100%;border-collapse:collapse;font-size:13px">
    <tr style="background:#fafafa"><th style="border:1px solid #e5e5e5;padding:6px">序号</th>
      <th style="border:1px solid #e5e5e5">单号</th><th style="border:1px solid #e5e5e5">客户</th>
      <th style="border:1px solid #e5e5e5">金额</th><th style="border:1px solid #e5e5e5">备注</th></tr>
    ${Array.from({ length: 96 }, (_, i) => {
      const n = i + 1;
      const mark = n === 1 ? 'TOP-KEY-4f2a91' : n === 48 ? 'MID-KEY-7c3d08' : n === 96 ? 'END-KEY-b95e26' : '';
      return `<tr><td style="border:1px solid #e5e5e5;padding:5px;text-align:center">${n}</td>
        <td style="border:1px solid #e5e5e5;padding-left:8px">DEMO-2024-${String(1000 + n)}</td>
        <td style="border:1px solid #e5e5e5;padding-left:8px">示例客户${n}</td>
        <td style="border:1px solid #e5e5e5;text-align:right;padding-right:8px">${(n * 37.5).toFixed(2)}</td>
        <td style="border:1px solid #e5e5e5;padding-left:8px;color:${mark ? '#c0392b' : '#999'}">${mark || '—'}</td></tr>`;
    }).join('')}
  </table>
  <div style="margin-top:18px;color:#666">合计 96 笔，页脚校验码：FOOT-SUM-e71b40</div></div>
</body>`;

const browser = await chromium.launch();
for (const [name, html, size] of [
  ['license', license, { width: 1240, height: 880 }],
  ['checkout', checkout, { width: 2400, height: 1180 }],
  ['longpage', longpage, { width: 2400, height: 1200 }],
]) {
  const page = await browser.newPage({ viewport: size, deviceScaleFactor: 1 });
  await page.setContent(html);
  const file = path.join(outDir, `${name}.png`);
  // longpage 要整页（fullPage），其余按 viewport
  await page.screenshot({ path: file, fullPage: name === 'longpage' });
  const dim = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.scrollHeight]);
  await page.close();
  console.log(`${file}  ${name === 'longpage' ? dim.join('x') : `${size.width}x${size.height}`}  ${(fs.statSync(file).size / 1024).toFixed(0)}KB`);
}
await browser.close();
