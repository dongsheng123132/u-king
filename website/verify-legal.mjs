// 法务三页的回归检查：真起浏览器渲染 + 语言切换切得动 + 内链不断 + 联系方式可点。
//
// 为什么留着不删：这三页还要再改至少一次 —— 账单描述符（refund.html 里那行 U-KING）
// 必须等 Waffo 给出真实字符串后回来替换。那行字写错，整块防拒付设计就白做，
// 而写错了页面照样渲染、照样"看起来没问题"。所以留一条能跑的跑道。
//
// 跑法：node website/verify-legal.mjs   （退出码 0=全过 / 1=有页面没过）

import { chromium } from 'playwright';
import { fileURLToPath } from 'url';
import path from 'path';

const dir = path.dirname(fileURLToPath(import.meta.url));
const PAGES = ['terms.html', 'privacy.html', 'refund.html'];
const CONTACT = 'hefangsheng@gmail.com'; // 唯一的对外支持渠道，Waffo AUP 要求必须有人看

let failed = 0;
const browser = await chromium.launch();

for (const page of PAGES) {
  // locale 给 en-US：模拟海外买家，验证默认落英文而不是中文
  const ctx = await browser.newContext({ locale: 'en-US' });
  const p = await ctx.newPage();
  const errs = [];
  p.on('pageerror', (e) => errs.push(String(e)));
  p.on('console', (m) => { if (m.type() === 'error') errs.push(m.text()); });

  await p.goto('file:///' + path.join(dir, page).replace(/\\/g, '/'));
  await p.waitForTimeout(250);

  const en = p.locator('div[data-lang="en"]').first();
  const zh = p.locator('div[data-lang="zh"]').first();

  const enDefault = await en.isVisible();
  const zhHidden = !(await zh.isVisible());

  await p.locator('.langsw button[data-set="zh"]').click();
  await p.waitForTimeout(120);
  const zhAfter = await zh.isVisible();
  const enAfter = !(await en.isVisible());

  // 样式真加载了，不是裸 HTML
  const styled = await p.evaluate(() =>
    getComputedStyle(document.querySelector('header')).borderBottomWidth !== '0px');

  // 内链指向的页面都存在
  const hrefs = await p.$$eval('a[href$=".html"]', (as) => as.map((a) => a.getAttribute('href')));
  const known = new Set([...PAGES, 'index.html']);
  const broken = [...new Set(hrefs)].filter((h) => !known.has(h));

  // 联系方式在中英两版都出现，且是可点的 mailto —— 买家找不到人 = 直接拒付
  const mails = await p.$$eval('a[href^="mailto:"]', (as) => as.map((a) => a.getAttribute('href')));
  const mailOk = mails.length >= 2 && mails.every((m) => m === 'mailto:' + CONTACT);

  const ok = enDefault && zhHidden && zhAfter && enAfter && styled && mailOk
    && !broken.length && !errs.length;
  if (!ok) failed++;

  console.log(
    `${ok ? 'PASS' : 'FAIL'}  ${page}` +
    `  en默认=${enDefault} zh隐藏=${zhHidden} 切中文=${zhAfter} 切后隐英=${enAfter}` +
    ` 样式=${styled} 邮箱可点=${mailOk}(${mails.length})` +
    (broken.length ? `  断链=${broken.join(',')}` : '') +
    (errs.length ? `  JS错误=${errs.join(' | ')}` : '')
  );
  await ctx.close();
}

await browser.close();
console.log(failed ? `\n${failed} 个页面没过` : '\n三页全过');
process.exit(failed ? 1 : 0);
