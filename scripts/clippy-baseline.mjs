#!/usr/bin/env node
// clippy baseline 工具 —— 「冻结存量、禁止新增」的门禁实现。
//
// 背景：`cargo clippy --all-targets -- -D warnings` 一次性收紧会让 main 长红
// （2026-11-02 存量 334 条 = lib 171 + lib test 163）。所以先把存量截图冻结成 baseline，
// CI 只断言「不许比 baseline 多」，存量在清理过程中一路往下掉，掉完（B4）再撤掉 baseline
// 换回 `-D warnings`。详见 docs/ci-gates.md。
//
// 前提：clippy JSON 必须与当前源码树同源（item 名是扫源码算出来的）。
// 改了代码就重跑 clippy，别拿旧的 clippy.json 来比——脚本会在归位靠猜时提醒。
//
// 指纹 = lint 名 + 相对仓库根的路径 + item 名，**不含行号**：
// 行号会随任何无关改动漂移，进了指纹 baseline 就变成「每次 rebase 都要 --update」的噪音源。
// item 名取「包裹这条诊断的最内层 item」（fn/struct/enum/impl/const/...），
// 靠扫描源码得到，不是靠 rustc 给的 span 文本。
//
// 用法（在仓库根执行）：
//   # 1. 跑 clippy 存 JSON（`|| true` 不需要：单纯 warning 时 cargo 退出码是 0）
//   (cd src-tauri && cargo clippy --all-targets --message-format=json > ../clippy.json)
//
//   # 2. 生成 / 更新 baseline —— 只在「清理 PR」里手工执行，CI 绝不调用
//   node scripts/clippy-baseline.mjs generate --input clippy.json
//
//   # 3. 门禁比对（CI 用）：新增即失败，存量放行
//   node scripts/clippy-baseline.mjs check --input clippy.json
//
//   # 4. 断言 baseline 这次改动只减不增（CI 用）
//   node scripts/clippy-baseline.mjs verify-shrink --base-ref origin/main
//
// 退出码：0 = 通过；1 = 门禁失败；2 = 用法/环境错误。

import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '..');
const DEFAULT_BASELINE = path.join(REPO_ROOT, 'clippy-baseline.json');

// ── 命令行 ──────────────────────────────────────────────────────────────
function parseArgs(argv) {
  const [raw, ...rest] = argv;
  const cmd = raw === '-h' || raw === '--help' ? 'help' : raw;
  const opts = { cmd, input: null, out: DEFAULT_BASELINE, baseRef: null };
  for (let i = 0; i < rest.length; i += 1) {
    const a = rest[i];
    if (a === '--input') opts.input = path.resolve(rest[++i]);
    else if (a === '--out') opts.out = path.resolve(rest[++i]);
    else if (a === '--base-ref') opts.baseRef = rest[++i];
    else if (a === '--update') opts.cmd = 'generate'; // 兼容 `check --update`：等价于 generate
    else if (a === '-h' || a === '--help') opts.cmd = 'help';
    else fail(2, `未知参数：${a}`);
  }
  return opts;
}

function fail(code, msg) {
  console.error(msg);
  process.exit(code);
}

const USAGE = `clippy baseline 工具

  generate      从 clippy JSON 生成/更新 clippy-baseline.json（清理 PR 手工执行；CI 不调用）
                --input <clippy.json> [--out clippy-baseline.json]
                「check --update」等价于 generate（但 CI 永不调用——只减不增由 verify-shrink 兜住）
  check         门禁比对：新增 warning / 同类计数上涨 → 退出码 1
                --input <clippy.json>
  verify-shrink 断言 baseline 的改动只减不增（读 git 里的旧版本对比）
                [--base-ref origin/main] [--out clippy-baseline.json]
`;

// ── 读 clippy JSON ──────────────────────────────────────────────────────
// `cargo clippy --message-format=json` 是 NDJSON：一行一条 cargo 消息。
// 只取 reason=compiler-message 且 level=warning 的顶层诊断；
// message.children 是建议/补充说明，不是独立诊断，不能重复计数。
function readDiagnostics(jsonPath) {
  if (!existsSync(jsonPath)) fail(2, `找不到 clippy JSON：${jsonPath}`);
  const raw = readFileSync(jsonPath, 'utf8');
  const diags = [];
  for (const line of raw.split('\n')) {
    const t = line.trim();
    if (!t.startsWith('{')) continue;
    let m;
    try {
      m = JSON.parse(t);
    } catch {
      continue; // 半行/截断的行（CI 日志混入）直接跳过
    }
    if (m.reason !== 'compiler-message' || !m.message) continue;
    const msg = m.message;
    if (msg.level !== 'warning') continue;
    const lint = (msg.code && msg.code.code) || 'unknown';
    const primary = (msg.spans || []).find((s) => s.is_primary) || (msg.spans || [])[0];
    if (!primary || !primary.file_name) continue;
    const file = normalizeFile(primary.file_name);
    if (!file) continue; // 依赖 crate（~/.cargo、target/）里的 warning 不算我们的账
    diags.push({ lint, file, line: primary.line_start || 0, message: (msg.message || '').split('\n')[0] });
  }
  return diags;
}

// cargo 报的 file_name 相对 `src-tauri/`（例如 `src/agent/chat.rs`）；
// 统一换成相对仓库根的路径，baseline 里一眼能看出是哪个文件。
function normalizeFile(fileName) {
  const p = fileName.replace(/\\/g, '/');
  if (p.startsWith('target/') || p.includes('/.cargo/') || p.includes('/registry/')) return null;
  if (path.isAbsolute(p)) {
    const rel = path.relative(REPO_ROOT, p).replace(/\\/g, '/');
    return rel.startsWith('..') ? null : rel;
  }
  return p.startsWith('src-tauri/') ? p : `src-tauri/${p}`;
}

// ── item 名提取 ─────────────────────────────────────────────────────────
// 目标：把一条诊断定位到「它所在的最内层 item」，且结果不含行号、只随代码结构变。
// 做法：把源码里的注释/字符串抹成空白（保持行数与花括号以外的字符位置），然后
//   ① 逐行记录 item 声明及其花括号区间，得到每一行「最内层 item」；
//   ② 诊断落在 doc 注释/属性行上时（clippy 的 doc_* 系列就是这样），改取它下方最近的 item。
const DECL_RES = [
  // fn foo / async fn foo / unsafe extern "C" fn foo
  [/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+([A-Za-z0-9_]+)/, 'fn'],
  [/^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum|union|trait|type|mod|const|static)\s+([A-Za-z0-9_]+)/, 2],
  [/^\s*(?:pub(?:\([^)]*\))?\s+)?(macro_rules)!\s+([A-Za-z0-9_]+)/, 2],
  [/^\s*(?:pub(?:\([^)]*\))?\s+)?(impl)\b\s*(.*?)\s*\{?\s*$/, 'impl'],
];

function declName(line) {
  for (const [re, kind] of DECL_RES) {
    const m = line.match(re);
    if (!m) continue;
    if (kind === 'fn') return `fn ${m[1]}`;
    if (kind === 'impl') {
      const head = (m[2] || '').replace(/\s*\{$/, '').trim();
      return head ? `impl ${head}` : 'impl';
    }
    return `${m[1]} ${m[2]}`;
  }
  return null;
}

const itemCache = new Map();
const srcCache = new Map();

function sourceText(fileAbs) {
  if (srcCache.has(fileAbs)) return srcCache.get(fileAbs);
  let src = null;
  try {
    src = readFileSync(fileAbs, 'utf8');
  } catch {
    src = null;
  }
  srcCache.set(fileAbs, src);
  return src;
}

function itemIndexFor(fileAbs) {
  if (itemCache.has(fileAbs)) return itemCache.get(fileAbs);
  const src = sourceText(fileAbs);
  if (src === null) {
    itemCache.set(fileAbs, null);
    return null;
  }
  const lines = blankOutLiterals(src).split('\n');
  // 记录每个声明的 [起始行, 结束行]。
  //   块形（fn/struct/mod/impl…）：声明行（或多行签名结束处）的 `{` 开块，
  //   花括号回到开块前的深度时结束；
  //   `const`/`static`/`type` 这种没有块的：吃到行尾 `;` 为止（默认只占自己那一行），
  //   否则它会把后面所有代码都算成自己的「内部」，item 名就全错了。
  const decls = [];
  const stack = [];
  let depth = 0;
  let pending = -1; // 已声明、还没见到 `{` 或 `;` 的下标
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const name = declName(line);
    const opens = (line.match(/\{/g) || []).length;
    const closes = (line.match(/\}/g) || []).length;

    if (name) {
      decls.push({ line: i + 1, name, end: i + 1 });
      pending = decls.length - 1;
    }
    if (pending !== -1) {
      const decl = decls[pending];
      if (opens > closes) {
        // 本行开块（净开）→ 块形声明，等花括号回到原深度时结束
        stack.push({ idx: pending, depth: depth + opens - closes });
        pending = -1;
      } else if (opens > 0) {
        // 一行写完的块（`fn x() { … }`）→ 就到自己这行
        decl.end = i + 1;
        pending = -1;
      } else if (/;\s*$/.test(line)) {
        // 本行以 `;` 收尾 → 无块声明到此结束
        decl.end = i + 1;
        pending = -1;
      }
    }
    depth += opens - closes;
    while (stack.length && depth < stack[stack.length - 1].depth) {
      decls[stack.pop().idx].end = i + 1;
    }
  }
  while (stack.length) decls[stack.pop().idx].end = lines.length;
  if (pending !== -1 && decls[pending]) decls[pending].end = Math.min(decls[pending].line + 1, lines.length);
  itemCache.set(fileAbs, decls);
  return decls;
}

// 把一段文本抹成空格但**保留换行**：多行原始字符串/续行字符串一旦把 \n 吃掉，
// 后面所有行号会整体前移，item 归属就会错一大片（实测踩过：installer.rs 偏了 118 行）。
function blankChunk(text) {
  return text.replace(/[^\n]/g, ' ');
}

// 把字符串/字符/注释内容替换成空格（保留换行），避免 `"{}"` 之类干扰花括号计数。
function blankOutLiterals(src) {
  let out = '';
  let i = 0;
  const n = src.length;
  const blank = (c) => (c === '\n' ? '\n' : ' ');
  while (i < n) {
    // 行注释
    if (src[i] === '/' && src[i + 1] === '/') {
      while (i < n && src[i] !== '\n') { out += ' '; i += 1; }
      continue;
    }
    // 块注释（支持嵌套一层，够用）
    if (src[i] === '/' && src[i + 1] === '*') {
      let level = 0;
      while (i < n) {
        if (src[i] === '/' && src[i + 1] === '*') { level += 1; out += '  '; i += 2; continue; }
        if (src[i] === '*' && src[i + 1] === '/') { level -= 1; out += '  '; i += 2; if (level <= 0) break; continue; }
        out += blank(src[i]); i += 1;
      }
      continue;
    }
    // 原始字符串 r"..." / r#"..."#
    if (src[i] === 'r' && (src[i + 1] === '"' || src[i + 1] === '#')) {
      const m = /^r(#*)"([\s\S]*?)"\1/.exec(src.slice(i));
      if (m) { out += blankChunk(m[0]); i += m[0].length; continue; }
    }
    // 普通字符串
    if (src[i] === '"') {
      out += ' ';
      i += 1;
      while (i < n && src[i] !== '"') {
        if (src[i] === '\\') {
          out += blank(src[i]);
          out += i + 1 < n ? blank(src[i + 1]) : '';
          i += 2;
          continue;
        }
        out += blank(src[i]); i += 1;
      }
      if (i < n) { out += ' '; i += 1; }
      continue;
    }
    // 字符字面量（'a' / '\n'）——注意别把生命周期 'a 当字面量：
    // 只在匹配 `'x'` 或 `'\…'` 时吞掉。
    if (src[i] === "'") {
      const m = /^'(?:\\.|[^\\'])'/.exec(src.slice(i));
      if (m) { out += blankChunk(m[0]); i += m[0].length; continue; }
    }
    out += src[i];
    i += 1;
  }
  return out;
}

let noItemMismatch = 0; // 归不到任何 item 的诊断条数（正常是模块级 `use` 之类）
const isCommentOrAttr = (line) => /^\s*(\/\/\/|\/\/!|#\[|#!\[)/.test(line);

function itemNameFor({ file, line }) {
  const fileAbs = path.join(REPO_ROOT, file);
  const decls = itemIndexFor(fileAbs);
  if (!decls) return '<file>';
  const enclosing = decls.filter((d) => d.line <= line && line <= d.end).pop();
  // doc/attr 行：真正被描述的是它下面的那个 item
  const text = ((sourceText(fileAbs) || '').split('\n')[line - 1]) || '';
  // `//!` 是**模块**的内部文档（描述整个文件），不属于它下面那个 item：
  // 否则任何人往文件顶部加一个 fn，模块文档的 warning 就会被重新归到这个新 item 上，
  // 变成「加了个函数 = 凭空多出几条新 warning」的假阳性。
  if (/^\s*\/\/!/.test(text)) return `mod ${path.basename(file, '.rs')}`;
  if (isCommentOrAttr(text)) {
    const forward = decls.find((d) => d.line >= line);
    if (forward) return forward.name;
  }
  if (enclosing) return enclosing.name;
  if (process.env.CB_DEBUG) {
    console.error(`[no-item] ${file}:${line} | ${JSON.stringify(text.slice(0, 60))}`);
  }
  // 归不到任何 item：模块级语句（`use` 之类）。记 <file>，稳定且一眼看得出是文件级。
  noItemMismatch += 1;
  return '<file>';
}

// ── 指纹 / 计数 ─────────────────────────────────────────────────────────
const fingerprint = (d) => `${d.lint}|${d.file}|${d.item}`;

function collect(diags) {
  const counts = new Map();
  const meta = new Map();
  for (const d of diags) {
    const item = itemNameFor(d);
    const fp = `${d.lint}|${d.file}|${item}`;
    counts.set(fp, (counts.get(fp) || 0) + 1);
    if (!meta.has(fp)) meta.set(fp, { lint: d.lint, file: d.file, item, sample: d.message });
  }
  return { counts, meta };
}

// ── baseline 读写（每行一条，git diff 干净）──────────────────────────────
function serializeBaseline(header, entries) {
  // 头部字段单列，entries 每条压成一行 —— git diff 里就只会看到「删掉的整行」
  const { entries: _omit, ...headOnly } = header;
  const head = JSON.stringify(headOnly, null, 2).replace(/\n}$/, '');
  const body = entries
    .map((e) => `    ${JSON.stringify({ fingerprint: e.fingerprint, lint: e.lint, file: e.file, item: e.item, count: e.count })}`)
    .join(',\n');
  return `${head},\n  "entries": [\n${body}\n  ]\n}\n`;
}

function loadBaseline(file) {
  if (!existsSync(file)) fail(2, `找不到 baseline：${file}`);
  const j = JSON.parse(readFileSync(file, 'utf8'));
  if (!Array.isArray(j.entries)) fail(2, `baseline 格式不对（缺 entries 数组）：${file}`);
  const counts = new Map();
  for (const e of j.entries) {
    if (!e.fingerprint) fail(2, `baseline 条目缺 fingerprint：${JSON.stringify(e)}`);
    counts.set(e.fingerprint, e.count);
  }
  return { json: j, counts };
}

// ── 子命令 ──────────────────────────────────────────────────────────────
function rustcVersion() {
  try {
    return execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

function cmdGenerate(opts) {
  if (!opts.input) fail(2, 'generate 需要 --input <clippy.json>');
  const diags = readDiagnostics(opts.input);
  const { counts, meta } = collect(diags);
  const entries = [...counts.keys()]
    .sort()
    .map((fp) => ({ fingerprint: fp, ...meta.get(fp), count: counts.get(fp) }));
  const header = {
    schemaVersion: 1,
    tool: 'scripts/clippy-baseline.mjs',
    command: 'cargo clippy --all-targets --message-format=json',
    fingerprint: 'lint 名 + 相对仓库根路径 + 最内层 item 名（不含行号）',
    generatedAt: new Date().toISOString().slice(0, 10),
    // 平台必须记下来：Windows/macOS/Linux 的 cfg 不同，warning 集合会双向差几条
    // （本仓 Linux 比 macOS 多一批 dead_code，macOS 也有 10 条 Linux 没有的），
    // 所以账本以「门禁跑的那个平台」为准，见 docs/ci-gates.md。
    platform: process.platform,
    rustc: rustcVersion(),
    notes:
      'count 是该指纹在 lib 与 lib test 两次编译里出现的次数；同一处 warning 通常计 2。' +
      '本文件只允许变小：清理 PR 用 generate 重写后，CI 的 verify-shrink 会断言没有新增条目/计数。',
    total: diags.length,
    entryCount: entries.length,
    entries,
  };
  writeFileSync(opts.out, serializeBaseline(header, entries), 'utf8');
  console.log(`✓ baseline 已写入 ${path.relative(REPO_ROOT, opts.out)}`);
  console.log(`  诊断条数 ${diags.length} / 指纹 ${entries.length} 条（lint ${new Set(entries.map((e) => e.lint)).size} 种）`);
  return 0;
}

function cmdCheck(opts) {
  if (!opts.input) fail(2, 'check 需要 --input <clippy.json>');
  const diags = readDiagnostics(opts.input);
  const { counts, meta } = collect(diags);
  const base = loadBaseline(opts.out);

  const added = [];
  const increased = [];
  const decreased = [];
  for (const [fp, n] of counts) {
    const old = base.counts.get(fp);
    if (old === undefined) added.push({ fp, n, ...meta.get(fp) });
    else if (n > old) increased.push({ fp, n, old, ...meta.get(fp) });
    else if (n < old) decreased.push({ fp, n, old });
  }
  for (const fp of base.counts.keys()) if (!counts.has(fp)) decreased.push({ fp, n: 0, old: base.counts.get(fp) });

  const baseTotal = base.json.total ?? 0;
  const seededOn = base.json.platform || 'unknown';
  console.log('── clippy baseline 门禁 ─────────────────────────────');
  if (seededOn !== 'unknown' && seededOn !== process.platform) {
    console.log(
      `注意：baseline 是在 ${seededOn} 上生成的，本机是 ${process.platform}——` +
        'cfg 差异会让两边 warning 集合双向差几条（典型是 dead_code），' +
        '本机出现「平台专属」的新增属预期，以 CI 为准。详见 docs/ci-gates.md。',
    );
  }
  console.log(`baseline：${baseTotal} 条诊断 / ${base.counts.size} 条指纹（${base.json.generatedAt || '?'} 生成）`);
  console.log(`本次实测：${diags.length} 条诊断 / ${counts.size} 条指纹`);
  if (noItemMismatch > 8) {
    console.log(
      `\n注意：有 ${noItemMismatch} 条诊断归不到任何 item（记成 <file>）——通常说明这份 ` +
        'clippy.json 不是当前源码树跑出来的（改了代码却拿旧的 JSON 比对）。请重跑 clippy 再比。',
    );
  }
  console.log(`存量相比 baseline 减少 ${decreased.length} 条指纹${diags.length < baseTotal ? `（诊断 -${baseTotal - diags.length}）` : ''}`);

  if (added.length) {
    console.log(`\n✗ 新增 warning ${added.length} 条（baseline 里没有，门禁失败）：`);
    for (const a of added.slice(0, 40)) {
      console.log(`  + ${a.fp}  ×${a.n}`);
      if (a.sample) console.log(`      ${a.sample.slice(0, 110)}`);
    }
    if (added.length > 40) console.log(`  … 其余 ${added.length - 40} 条省略`);
  }
  if (increased.length) {
    console.log(`\n✗ 同一条 warning 数量上涨 ${increased.length} 条（门禁失败）：`);
    for (const a of increased.slice(0, 40)) console.log(`  + ${a.fp}  ${a.old} → ${a.n}`);
    if (increased.length > 40) console.log(`  … 其余 ${increased.length - 40} 条省略`);
  }
  if (decreased.length && !added.length && !increased.length) {
    console.log('\n提示：存量比 baseline 少了，可在清理 PR 里跑 generate 把 baseline 收紧（本次不拦）。');
  }
  if (added.length || increased.length) {
    console.log('\n✗ clippy baseline 门禁失败：新代码不要引入新 warning。');
    console.log('  - 修掉上面列出的 warning；');
    console.log('  - 确属无法避免的（例如平台相关），在代码里写带理由的 #[allow(...)]，');
    console.log('    被 allow 的条目不计入本 baseline（allow 全仓上限 20 条，见 docs/ci-gates.md）；');
    console.log('  - 不要为了让门禁变绿而 --update 抬高 baseline。');
    return 1;
  }
  console.log('\n✓ clippy baseline 门禁通过：没有新增 warning，存量未上涨。');
  return 0;
}

function cmdVerifyShrink(opts) {
  if (!opts.baseRef) fail(2, 'verify-shrink 需要 --base-ref <git ref>（例如 origin/main）');
  const rel = path.relative(REPO_ROOT, opts.out).replace(/\\/g, '/');
  let oldRaw;
  try {
    oldRaw = execFileSync('git', ['show', `${opts.baseRef}:${rel}`], {
      cwd: REPO_ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'], // git 的 "path ... does not exist" 由下面自己讲
    });
  } catch {
    console.log(`✓ base 里还没有 ${rel}（首个引入 baseline 的提交），跳过只减不增断言。`);
    return 0;
  }
  const oldJson = JSON.parse(oldRaw);
  const newJson = JSON.parse(readFileSync(opts.out, 'utf8'));
  const oldMap = new Map(oldJson.entries.map((e) => [e.fingerprint, e.count]));
  const newMap = new Map(newJson.entries.map((e) => [e.fingerprint, e.count]));

  const added = [...newMap.keys()].filter((k) => !oldMap.has(k));
  const bumped = [];
  for (const [k, v] of newMap) if (oldMap.has(k) && v > oldMap.get(k)) bumped.push({ k, old: oldMap.get(k), now: v });
  const removed = [...oldMap.keys()].filter((k) => !newMap.has(k));
  const shrunk = [...newMap.keys()].filter((k) => oldMap.has(k) && newMap.get(k) < oldMap.get(k));

  console.log('── baseline 只减不增断言 ─────────────────────────────');
  console.log(`${opts.baseRef}:${rel} → 工作区：${oldJson.total ?? '?'} 条 → ${newJson.total ?? '?'} 条诊断`);
  console.log(`删除指纹 ${removed.length} 条 / 计数下降 ${shrunk.length} 条`);

  if (added.length) {
    console.log(`\n✗ baseline 新增了 ${added.length} 条指纹（不允许：只许删，不许把新 warning 记进账）：`);
    for (const k of added.slice(0, 40)) console.log(`  + ${k}`);
    if (added.length > 40) console.log(`  … 其余 ${added.length - 40} 条省略`);
  }
  if (bumped.length) {
    console.log(`\n✗ baseline 里有 ${bumped.length} 条计数上涨：`);
    for (const b of bumped.slice(0, 40)) console.log(`  + ${b.k}  ${b.old} → ${b.now}`);
  }
  const newTotal = newJson.total ?? 0;
  const oldTotal = oldJson.total ?? 0;
  if (newTotal > oldTotal) {
    console.log(`\n✗ baseline 总数上涨：${oldTotal} → ${newTotal}`);
    return 1;
  }
  if (added.length || bumped.length) {
    console.log('\n✗ baseline 只能变小：请把新 warning 修掉，而不是记进 baseline。');
    return 1;
  }
  console.log(`\n✓ baseline 只减不增断言通过（${oldTotal} → ${newTotal}）。`);
  return 0;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (!opts.cmd || opts.cmd === 'help') {
    console.log(USAGE);
    return 0;
  }
  if (opts.cmd === 'generate') return cmdGenerate(opts);
  if (opts.cmd === 'check') return cmdCheck(opts);
  if (opts.cmd === 'verify-shrink') return cmdVerifyShrink(opts);
  fail(2, `未知子命令：${opts.cmd}\n\n${USAGE}`);
}

process.exit(main());
