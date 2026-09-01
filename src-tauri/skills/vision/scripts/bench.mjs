#!/usr/bin/env node
/**
 * 识图模型跑道 —— 用带 ground truth 的真实图片给候选模型打分。
 *
 * 为什么要有这东西：模型名每隔几周就换一批（qwen3.5→3.6→3.7），
 * 「哪个更好」不能靠听说。这里给出可复跑的客观数字：命中率 + 耗时。
 *
 *   node scripts/bench.mjs                      # 跑默认 cases
 *   node scripts/bench.mjs --cases <path.json>  # 换一份用例
 *   node scripts/bench.mjs --models a,b,c       # 只跑这几个模型
 *   node scripts/bench.mjs --json               # 出机器可读结果
 *
 * 用例文件（默认 ~/.uking/vision-bench/cases.json，含隐私图片故不入库）：
 * {
 *   "cases": [
 *     { "name": "营业执照",
 *       "image": "C:/path/to.jpg",
 *       "ask": "抽取以下字段…",            // focused 模式的问法
 *       "needles": ["91441900MACCD11U53", …]  // ground truth，出现即算命中
 *     }
 *   ]
 * }
 *
 * 判分：needles 是「这张图里客观存在、且必须被读出来」的字符串。
 * 命中率 = 出现的 needle 数 / 总数。**不做模糊匹配**——OCR 对了就是对了。
 */
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const ENDPOINT = 'https://api.u-claw.org.cn/v1/chat/completions';
const DEFAULT_MODELS = [
  'qwen3-vl-flash',
  'qwen3.7-flash',
  'qwen3.5-ocr',
  'qwen-vl-plus',
  'qwen-vl-max',
  'MiniMax-M3',
];
// 两种问法的对照 —— 本跑道要证明的核心假设：
// 「丢内容」很大程度不是模型不行，是没告诉它你要找什么。
const GENERIC_PROMPT = '描述这张图片的内容，并把图中的文字读出来。';

function argOf(flag, fallback) {
  const i = process.argv.indexOf(flag);
  return i > -1 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

function readKey() {
  if (process.env.XIAPAN_API_KEY) return process.env.XIAPAN_API_KEY;
  const p = path.join(os.homedir(), '.uking', 'device.json');
  return JSON.parse(fs.readFileSync(p, 'utf8')).key;
}

const MIME = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.gif': 'image/gif' };
function toDataUrl(file) {
  const ext = path.extname(file).toLowerCase();
  return `data:${MIME[ext] || 'image/png'};base64,` + fs.readFileSync(file).toString('base64');
}

async function ask(key, model, dataUrl, prompt) {
  const t = Date.now();
  try {
    const res = await fetch(ENDPOINT, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${key}` },
      body: JSON.stringify({
        model,
        max_tokens: 2000,
        messages: [{ role: 'user', content: [
          { type: 'text', text: prompt },
          { type: 'image_url', image_url: { url: dataUrl } },
        ] }],
      }),
      signal: AbortSignal.timeout(180000),
    });
    const j = await res.json();
    const elapsed = (Date.now() - t) / 1000;
    if (j.error) return { ok: false, err: (j.error.message || '').slice(0, 120), elapsed };
    return {
      ok: true,
      text: j.choices?.[0]?.message?.content || '',
      elapsed,
      usage: j.usage || null,
    };
  } catch (e) {
    return { ok: false, err: e.message, elapsed: (Date.now() - t) / 1000 };
  }
}

// needle 命中：全角/半角括号与空白差异不该算错，其余一字不让
const norm = (s) => (s || '').replace(/[（）]/g, (c) => (c === '（' ? '(' : ')')).replace(/\s+/g, '');
const hits = (text, needles) => needles.filter((n) => norm(text).includes(norm(n)));

async function pool(items, size, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(size, items.length) }, async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i], i);
    }
  }));
  return out;
}

const casesPath = argOf('--cases', path.join(os.homedir(), '.uking', 'vision-bench', 'cases.json'));
if (!fs.existsSync(casesPath)) {
  console.error(`找不到用例文件：${casesPath}\n用 --cases <path> 指定，或建一份（格式见本文件头注释）。`);
  process.exit(2);
}
let { cases } = JSON.parse(fs.readFileSync(casesPath, 'utf8'));
const only = argOf('--only', '').trim();
if (only) cases = cases.filter((c) => only.split(',').some((k) => c.name.includes(k.trim())));
// 相对路径按用例文件所在目录解析，跑道就能在任意 cwd 下跑
cases = cases.map((c) => ({ ...c, image: path.isAbsolute(c.image) ? c.image : path.resolve(path.dirname(casesPath), '..', c.image) }));
if (!cases.length) { console.error('没有匹配的用例'); process.exit(2); }
const models = argOf('--models', '').trim() ? argOf('--models', '').split(',').map((s) => s.trim()) : DEFAULT_MODELS;
const asJson = process.argv.includes('--json');
const key = readKey();

// 重复次数：识图是随机的，同一格子跑一次得出的排名不可信
// （实测 MiniMax-M3 同一图同一问法两次跑出 1/7 和 5/7）
const repeat = Math.max(1, parseInt(argOf('--repeat', '1'), 10) || 1);

const jobs = [];
for (const c of cases) {
  const dataUrl = toDataUrl(c.image);
  for (const model of models) {
    for (let i = 0; i < repeat; i++) {
      jobs.push({ case: c, model, mode: 'generic', prompt: GENERIC_PROMPT, dataUrl, rep: i });
      jobs.push({ case: c, model, mode: 'focused', prompt: c.ask, dataUrl, rep: i });
    }
  }
}

if (!asJson) console.error(`跑 ${jobs.length} 次调用（${cases.length} 图 × ${models.length} 模型 × 2 问法 × ${repeat} 遍）…`);

const results = await pool(jobs, 4, async (job) => {
  const r = await ask(key, job.model, job.dataUrl, job.prompt);
  const hit = r.ok ? hits(r.text, job.case.needles) : [];
  if (!asJson) {
    process.stderr.write(
      `  ${job.case.name}/${job.model}/${job.mode}: ` +
      (r.ok ? `${hit.length}/${job.case.needles.length} ${r.elapsed.toFixed(1)}s\n` : `✖ ${r.err}\n`)
    );
  }
  return {
    case: job.case.name, model: job.model, mode: job.mode,
    ok: r.ok, err: r.err || null,
    hit: hit.length, total: job.case.needles.length,
    missed: r.ok ? job.case.needles.filter((n) => !hit.includes(n)) : job.case.needles,
    elapsed: +r.elapsed.toFixed(1),
    usage: r.usage, text: r.text || '',
  };
});

if (asJson) { console.log(JSON.stringify({ cases: cases.map((c) => c.name), models, results }, null, 2)); process.exit(0); }

// 汇总表
const pad = (s, n) => String(s).padEnd(n);
// 一个格子 = 同一(图,模型,问法)的 repeat 次，报均值与跨度；跨度大本身就是结论
const cell = (m, cn, mode) => {
  const rows = results.filter((r) => r.model === m && r.case === cn && r.mode === mode);
  const ok = rows.filter((r) => r.ok);
  if (!ok.length) return { txt: '✖', hit: 0, total: rows[0]?.total || 0 };
  const hs = ok.map((r) => r.hit);
  const avg = hs.reduce((a, b) => a + b, 0) / hs.length;
  const lo = Math.min(...hs); const hi = Math.max(...hs);
  return {
    txt: `${avg.toFixed(1)}${lo === hi ? '' : `(${lo}~${hi})`}/${ok[0].total}`,
    hit: avg, total: ok[0].total,
  };
};

console.log('\n== 命中率（均值，越高越好）==');
console.log(pad('模型', 18) + cases.map((c) => pad(c.name.slice(0, 8), 26)).join('') + '合计');
for (const m of models) {
  let line = pad(m, 18); let th = 0; let tt = 0;
  for (const c of cases) {
    const g = cell(m, c.name, 'generic'); const f = cell(m, c.name, 'focused');
    th += g.hit + f.hit; tt += g.total + f.total;
    line += pad(`泛问${g.txt} 带意图${f.txt}`, 26);
  }
  console.log(line + `${tt ? Math.round((th / tt) * 100) : 0}%`);
}

console.log('\n== 耗时中位数 ==');
for (const m of models) {
  const es = results.filter((r) => r.model === m && r.ok).map((r) => r.elapsed).sort((a, b) => a - b);
  console.log(pad(m, 18) + (es.length ? `${es[Math.floor(es.length / 2)]}s  (n=${es.length})` : '全失败'));
}

// 问法对照 —— 本跑道的核心结论
const gh = results.filter((r) => r.mode === 'generic' && r.ok);
const fh = results.filter((r) => r.mode === 'focused' && r.ok);
const rate = (a) => (a.length ? Math.round((a.reduce((s, r) => s + r.hit, 0) / a.reduce((s, r) => s + r.total, 0)) * 100) : 0);
console.log(`\n== 问法对照 ==\n  泛问   ${rate(gh)}%   (n=${gh.length})\n  带意图 ${rate(fh)}%   (n=${fh.length})`);

console.log('\n== 漏了什么（带意图模式）==');
for (const c of cases) {
  const rows = results.filter((r) => r.case === c.name && r.mode === 'focused' && r.ok);
  const always = c.needles.filter((n) => rows.every((r) => r.missed.includes(n)));
  console.log(`  ${c.name}: ${always.length ? '全员漏 → ' + always.join(' / ') : '无全员漏项'}`);
}
