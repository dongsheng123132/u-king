#!/usr/bin/env node
/**
 * 文档理解跑道 —— 测的不是「字有没有抽出来」，是**下游模型能不能答对**。
 *
 *   node scripts/doc-bench.mjs [--pdf bench/fixtures/report.pdf] [--repeat 2] [--model deepseek-v4-flash]
 *
 * 为什么不能沿用图片跑道那套 needle 判分：表格的信息不在字里，在行列关系里。
 * 「乙的Q2=1120」这个事实，在压平的文本流里一个字节都没有 —— 数字全在、关系全丢，
 * 而 needle 判分会显示满分。所以这里的题目**全部必须知道行列归属才能答对**，
 * 而且答案数值互不相同，蒙不出来。
 *
 * 链路 = 真实使用姿势：read-pdf.py 抽取 → 把 Markdown 喂给 DeepSeek 本体 → 它答题。
 * 对照两种抽取：`--no-tables`（旧行为，纯文本流）vs 默认（表格按行列还原）。
 */
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '..');
const argOf = (f, d) => { const i = process.argv.indexOf(f); return i > -1 && process.argv[i + 1] ? process.argv[i + 1] : d; };

const PDF = path.resolve(argOf('--pdf', path.join(root, 'bench/fixtures/report.pdf')));
const REPEAT = Math.max(1, parseInt(argOf('--repeat', '2'), 10) || 1);
const MODEL = argOf('--model', 'deepseek-v4-flash');
const KEY = process.env.XIAPAN_API_KEY ||
  JSON.parse(fs.readFileSync(path.join(os.homedir(), '.uking', 'device.json'), 'utf8')).key;

const CASES = JSON.parse(fs.readFileSync(path.join(root, 'bench/doc-cases.json'), 'utf8'));
const QUESTIONS = CASES[path.basename(PDF)];
if (!QUESTIONS) {
  console.error(`bench/doc-cases.json 里没有 ${path.basename(PDF)} 的题库。已有：` +
    Object.keys(CASES).filter((k) => !k.startsWith('_')).join(', '));
  process.exit(2);
}

function extract(v) {
  // markitdown 走的是隔壁 uking-office-read 技能 —— 同一件事本仓库有两份实现，
  // 放进跑道对照，是为了让「该留哪一份」有数字可依，而不是靠印象。
  if (v.markitdown) {
    const p = path.join(os.homedir(), '.claude/skills/uking-office-read/scripts/read-doc.py');
    if (!fs.existsSync(p)) return null;
    const r = spawnSync('python', [p, PDF], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
    return r.status === 0 ? r.stdout : null;
  }
  const args = [path.join(root, 'scripts/read-pdf.py'), PDF];
  if (v.noTables) args.push('--no-tables');
  const r = spawnSync('python', args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  if (r.status !== 0) throw new Error('抽取失败：' + String(r.stderr || '').slice(0, 300));
  return r.stdout;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// 上游偶发 5xx / cpu overloaded / 空回复会把跑道数字搅浑 —— 那是基础设施抖动，
// 不是「抽取方式好不好」的证据。重试到拿到真回答为止，仍失败才记 ERR 并单列。
async function answer(docText, question, tries = 3) {
  for (let i = 0; i < tries; i++) {
    try {
      const res = await fetch('https://api.u-claw.org.cn/v1/chat/completions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${KEY}` },
        body: JSON.stringify({
          model: MODEL, max_tokens: 300, temperature: 0,
          messages: [{ role: 'user', content:
            `下面是一份文档的内容，请**只根据文档**回答问题，不要推测、不要计算文档里没写的东西。\n\n` +
            `<文档>\n${docText}\n</文档>\n\n问题：${question}` }],
        }),
        signal: AbortSignal.timeout(120000),
      });
      const j = await res.json();
      const txt = j.choices?.[0]?.message?.content || '';
      if (!j.error && txt.trim()) return txt;
      if (i === tries - 1) return 'ERR ' + ((j.error && j.error.message) || '空回复').slice(0, 80);
    } catch (e) {
      if (i === tries - 1) return 'ERR ' + e.message;
    }
    await sleep(1500 * (i + 1));
  }
  return 'ERR 重试耗尽';
}

const norm = (s) => (s || '').replace(/[,，\s]/g, '');
// must=全都要出现；must_any=任一即可（「未填报/空白/无」这类同义答法）；must_not=一个都不许出现
const judge = (ans, qq) => {
  const a = norm(ans);
  if (!a || a.startsWith('ERR')) return false;
  if ((qq.must || []).some((m) => !a.includes(norm(m)))) return false;
  if (qq.must_any && !qq.must_any.some((m) => a.includes(norm(m)))) return false;
  return !(qq.must_not || []).some((m) => a.includes(norm(m)));
};

const variants = [
  { name: '旧:纯文本流', noTables: true },
  { name: '新:表格还原', noTables: false },
  { name: 'markitdown', markitdown: true },
];

console.error(`文档：${PDF}\n下游模型：${MODEL}　题目 ${QUESTIONS.length} 道 × ${REPEAT} 遍\n`);
const score = {};
for (const v of variants) {
  const doc = extract(v);
  if (doc == null) { console.error(`— ${v.name}（不可用，跳过）`); continue; }
  console.error(`— ${v.name}（抽出 ${doc.length} 字符）`);
  score[v.name] = { hit: 0, total: 0, wrong: [] };
  for (let r = 0; r < REPEAT; r++) {
    for (const qq of QUESTIONS) {
      const ans = await answer(doc, qq.q);
      const ok = judge(ans, qq);
      score[v.name].total++;
      if (ok) score[v.name].hit++;
      else score[v.name].wrong.push(`${qq.q.slice(0, 16)}… → ${ans.replace(/\s+/g, ' ').slice(0, 46)}`);
      process.stderr.write(ok ? '.' : 'x');
    }
  }
  process.stderr.write('\n');
}

console.log('\n== 下游模型答对率（必须靠行列关系才能答对的题）==');
for (const v of variants) {
  const s = score[v.name];
  if (!s) continue;
  console.log(`${v.name.padEnd(14)} ${s.hit}/${s.total}  (${Math.round((s.hit / s.total) * 100)}%)`);
}
for (const v of variants) {
  const s = score[v.name];
  if (!s || !s.wrong.length) continue;
  console.log(`\n-- ${v.name} 答错的 --`);
  [...new Set(s.wrong)].forEach((w) => console.log('   ' + w));
}
