#!/usr/bin/env node
/**
 * 泄漏闸门（offline leak gate）— Node.js 18+，零依赖。
 * 用法：
 *   node scripts/check-leak.mjs <扫描根目录>          # 扫描整个源码树
 *   git log --format=%s | node scripts/check-leak.mjs --stdin   # 扫 commit message
 * 有命中退出码 1 并逐条列出；无命中打印 LEAK CHECK PASSED (0 hits)。
 */
import fs from 'node:fs';
import path from 'node:path';

const CODE_EXTS = new Set(['.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.mts', '.json', '.md', '.sh', '.ps1', '.yml', '.yaml', '.toml', '.html', '.css', '.txt', '.xml']);

const RULES = [
  // 真实 API key 形态（sk- 后 16 位以上连续字母数字）
  { name: 'api-key', pattern: /\bsk-[A-Za-z0-9]{16,}\b/g, severity: 'critical', exts: null, isFalsePositive: (m) => /^sk-abc123def456ghij$/i.test(m[0]) },
  { name: 'github-token', pattern: /\b(?:gho_|ghp_|github_pat_)[A-Za-z0-9_]{16,}\b/g, severity: 'critical', exts: null },
  // 客户机编号（内部工单里的 pc-XXXX 已全部替换为 pc-***，只匹配纯数字形态）
  { name: 'customer-device-id', pattern: /\bpc-\d{3,5}\b/gi, severity: 'high', exts: null },
  // 内部私有仓（u-king-mini 是本产品二进制名，公开安全，不列）
  { name: 'internal-repo-name', pattern: /\bu-king-reports\b|\bpay-server\b/gi, severity: 'high', exts: null },
  { name: 'production-deploy-path', pattern: /\/data\/(?:uking-site|website|uking-bug)\b|\/var\/www\/web-releases/g, severity: 'high', exts: null },
  { name: 'ssh-host-alias', pattern: /\b(?:guangzhou|macmini)\b|\bsen@|\bdeploy@\d/g, severity: 'medium', exts: CODE_EXTS },
  { name: 'ops-tooling', pattern: /\bpm2 (?:start|list|logs|restart)\b|\buking-bug\b/gi, severity: 'medium', exts: CODE_EXTS },
  {
    // Windows/Mac 用户主目录真实路径；白名单 = 明显的测试占位符
    name: 'real-user-path',
    pattern: /(?:[A-Za-z]:[\\/]+|\/)(?:Users|users)[\\/]+([^\\/\r\n"'`\s<>\[\]%\uFF08\uFF09\u3002\uFF0C\uFF1A\uFF1B\u3001\uFF01\uFF1F]+)/g,
    severity: 'high', exts: null,
    isFalsePositive: (m) => /^(?:test|tester|testing|demo|example|examples|public|default|username|user|me|my|x|y|z|foo|bar|alice|bob|zhangsan|lisi|wangwu|张三|李四|王五|波|你|自己|对方|dev|developer|LI|admin|admin1|user1|user2|secret|aaa|bbb|<u>|<user>|<客户名>|<用户名>)$/i.test(m[1]) || m[1] === '你的用户名' || m[1] === 'xxx' || /^<.*>$/.test(m[1]) || m[1].includes('…') || /\.\.\./.test(m[1]) || m[1] === '*' || m[1] === '*' || /^\*+/.test(m[1]),
      skipLine: (line) => line.includes('%USERPROFILE%') || line.includes('%USERNAME%') || line.includes('$HOME')
    },
  { name: 'private-ipv4', pattern: /\b(?:10|192\.168|100\.(?:6[4-9]|[7-9]\d|1[0-2]\d))\.\d{1,3}\.\d{1,3}\b/g, severity: 'medium', exts: CODE_EXTS, isFalsePositive: (m) => !isValidIp(m[0]) },
  { name: 'public-ipv4', pattern: /\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b/g, severity: 'high', exts: CODE_EXTS, isFalsePositive: (m) => !isValidIp(m[0]) || ['127.0.0.1', '0.0.0.0', '255.255.255.255'].includes(m[0]) || m[0].endsWith('.0') || /^1\.\d/.test(m[0]) || m[0].startsWith('203.0.113.') || m[0].split('.').some((o) => Number(o) > 255) === false && m[0].split('.').filter((o) => Number(o) > 199).length >= 2 , skipLine: (line) => line.includes('d="M') },
];

function isValidIp(v) {
  const parts = v.split('.');
  if (parts.length !== 4) return false;
  return parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255);
}

const SKIP_DIRS = new Set(['node_modules', '.git', 'target', 'dist', 'dist-usb', 'gen']);
const BINARY_EXTENSIONS = new Set(['.7z', '.avi', '.bin', '.bmp', '.class', '.dll', '.dmg', '.doc', '.docx', '.eot', '.exe', '.gif', '.gz', '.ico', '.icns', '.jar', '.jpeg', '.jpg', '.lock', '.map', '.mp3', '.mp4', '.otf', '.pdf', '.png', '.rar', '.so', '.tar', '.ttf', '.wasm', '.webm', '.webp', '.woff', '.woff2', '.xls', '.xlsx', '.zip', '.svg', '.d.ts']);
const MAX_BYTES = 5 * 1024 * 1024;

function trunc(line) { return line.length > 120 ? line.slice(0, 117) + '...' : line; }

let hits = 0;
function scanText(source, text) {
  const lines = text.split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const rule of RULES) {
      if (rule.exts && source.includes('.') && !rule.exts.has(path.extname(source).toLowerCase())) continue;
      rule.pattern.lastIndex = 0;
      let m;
      while ((m = rule.pattern.exec(line)) !== null) {
        if (rule.skipLine && rule.skipLine(line)) continue;
        if (rule.isFalsePositive && rule.isFalsePositive(m)) continue;
        console.log(`${source}:${index + 1}: [${rule.severity}] ${rule.name}: ${trunc(line.trim())}`);
        hits += 1;
        break;
      }
    }
  });
}

function walk(root) {
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) walk(full);
    } else if (entry.isFile()) {
      const ext = path.extname(entry.name).toLowerCase();
      if (BINARY_EXTENSIONS.has(ext)) continue;
      if (entry.name === 'check-leak.mjs') continue; // 闸门不扫自己（规则字面量自匹配）
      const stat = fs.statSync(full);
      if (stat.size > MAX_BYTES || stat.size === 0) continue;
      const data = fs.readFileSync(full);
      if (data.includes(0)) continue;
      scanText(full, data.toString('utf8'));
    }
  }
}

const args = process.argv.slice(2);
const stdinMode = args.includes('--stdin');
const root = args.find((a) => a !== '--stdin');
if (stdinMode) {
  // --stdin 模式扫管道输入（commit message），不需要 root 参数
  const chunks = [];
  process.stdin.on('data', (c) => chunks.push(c));
  process.stdin.on('end', () => {
    scanText('<stdin>', Buffer.concat(chunks).toString('utf8'));
    if (hits === 0) console.log('LEAK CHECK PASSED (0 hits)');
    process.exitCode = hits ? 1 : 0;
  });
} else if (!root) {
  console.error('Usage: node check-leak.mjs <root> [--stdin]');
  process.exitCode = 2;
} else {
  try {
    const stat = fs.statSync(root);
    if (!stat.isDirectory()) throw new Error('scan root must be a directory');
    walk(path.resolve(root));
    if (hits === 0) console.log('LEAK CHECK PASSED (0 hits)');
    process.exitCode = hits ? 1 : 0;
  } catch (e) {
    console.error('check-leak: ' + e.message);
    process.exitCode = 2;
  }
}
