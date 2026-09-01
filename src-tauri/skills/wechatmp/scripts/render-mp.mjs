#!/usr/bin/env node
// markdown → 公众号可直接粘贴的内联样式 HTML。
// 用法: node render-mp.mjs <文章.md> [--theme default|grace|simple] [--out 文章.html]
// 只做排版，不联网、不上传、不需要 AppID/Secret —— 发布留给人在公众号后台点。
import { render, THEME_NAMES } from 'dsh-wechat-mp/render';
import { readFile, writeFile } from 'node:fs/promises';

const a = process.argv.slice(2);
const file = a.find(x => !x.startsWith('--'));
const pick = (k, d) => { const i = a.indexOf(k); return i >= 0 ? a[i + 1] : d; };
if (!file) { console.error('用法: render-mp.mjs <文章.md> [--theme grace] [--out 文章.html]'); process.exit(2); }

const theme = pick('--theme', 'default');
if (!THEME_NAMES.includes(theme)) {
  console.error(`没有主题 "${theme}"，可用: ${THEME_NAMES.join(', ')}`); process.exit(2);
}
const out = pick('--out', file.replace(/\.md$/i, '') + '.mp.html');
const res = await render(await readFile(file, 'utf8'), { theme });
const html = typeof res === 'string' ? res : (res.html ?? '');
if (!html) { console.error('渲染没产出 HTML'); process.exit(1); }
await writeFile(out, html, 'utf8');
console.log(JSON.stringify({ ok: true, out, theme, bytes: html.length, inline_css: /style="/.test(html) }));
