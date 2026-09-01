#!/usr/bin/env node
/**
 * check-skill-scripts.mjs —— 技能包生成器的「产物真的能用吗」闸门。
 *
 * 为什么单开一条跑道（写不出理由的跑道不该存在）：
 * 这批脚本（gen-docx / gen-xlsx / gen-pptx / gen-eml / edit-office）**手搓 ZIP + XML**，
 * 它们唯一的失败模式是 `action conformance`、`cargo check`、`pnpm build` 全都看不见的那一种 ——
 *
 *   🔴 **返回 `{"ok":true}` + exit 0，产物却是坏的。**
 *
 * 2026-08-17 在客户机上一次黑盒测试同时撞出三处，全是这个形状：
 *   1. 输入含 XML 1.0 非法控制字符（0x0B/0x1F，OCR 结果和 PDF 复制里很常见）→
 *      docx/xlsx/pptx 三个生成器共用的 `esc()` 照写不误 → Word/Excel/PPT 报「文件已损坏」，
 *      而脚本报成功。用户交付出去才发现打不开。
 *   2. `gen-eml` 收件人/主题里的 CRLF 原样写进邮件头 → 凭空多一个**真实生效**的 `Bcc:`，
 *      用户在 To 栏里看不见，点发送就抄给了第三方。
 *   3. `gen-xlsx` 把 `007` 存成 7、18 位身份证末几位改成 0（双精度只有 15~17 位有效数字）——
 *      **静默改数据**，表格看着正常，直到有人拿它去对账。
 *
 * 这三件事的共同点：进程退出码是 0，JSON 里写着 ok:true，任何「跑起来没报错」的检查都是绿的。
 * 只有**解开产物、看里面的字节**才照得到。所以这条跑道量的是产物，不是退出码。
 *
 * 跑：`node scripts/check-skill-scripts.mjs`（已挂进 `pnpm build`）。全在系统临时目录里跑，
 * 不碰仓库、不碰用户文件。有红退出码 1。
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// 默认量仓库里的源；`UKING_SKILLS_DIR=<别的 skills 目录>` 可指向另一份做**变异验证** ——
// 一条只会绿的跑道证明不了任何事。修完这批 bug 时就是拿 `src-tauri/target/release/skills`
// 里那份未修改的旧脚本当对照组跑的：同样 20 条，旧的红 10 条、新的全绿。
const S = process.env.UKING_SKILLS_DIR || path.join(ROOT, "src-tauri", "skills");
const D = fs.mkdtempSync(path.join(os.tmpdir(), "uking-skillio-"));
// 0x0B 垂直制表符 —— 最常见的那个 XML 非法控制字符。用码点构造而不是写转义序列：
// 🔴 这一行被 `perl -i -pe` 吃掉过一次反斜杠，变成字面量 "000B"，于是输入里根本没有控制字符，
//    整条跑道当场变成恒绿的假绿。下面那句 assert 就是防这个的 —— 闸门自己也得有闸门。
const VT = String.fromCharCode(0x0b);
if (VT.length !== 1 || VT.charCodeAt(0) !== 0x0b) throw new Error("VT 不是 0x0B，跑道会假绿");

let fails = 0;
const ok = (name, cond, detail = "") => {
  if (!cond) fails++;
  console.log(`${cond ? "  ok " : "FAIL "} ${name}${cond || !detail ? "" : `\n       ${detail}`}`);
};

function run(script, args) {
  try {
    return { code: 0, out: execFileSync(process.execPath, [path.join(S, script), ...args], { encoding: "utf8" }).trim() };
  } catch (e) {
    return { code: e.status ?? -1, out: String(e.stdout || e.message || "").trim() };
  }
}

/** 最小 STORE-zip 解包 —— 这批生成器一律 method=0（不压缩），够用且零依赖。 */
function unzip(file) {
  const b = fs.readFileSync(file);
  const out = {};
  let i = 0;
  while (i + 30 <= b.length && b.readUInt32LE(i) === 0x04034b50) {
    const csize = b.readUInt32LE(i + 18);
    const nlen = b.readUInt16LE(i + 26), elen = b.readUInt16LE(i + 28);
    const name = b.subarray(i + 30, i + 30 + nlen).toString("utf8");
    const s = i + 30 + nlen + elen;
    out[name] = b.subarray(s, s + csize).toString("utf8");
    i = s + csize;
  }
  return out;
}
/** XML 1.0 只准 \t \n \r 这三个 C0 字符；其余进了部件就是「文件已损坏」。 */
const badXml = (s) => /[\x00-\x08\x0B\x0C\x0E-\x1F]/.test(s || "");
const w = (f, o) => (fs.writeFileSync(path.join(D, f), typeof o === "string" ? o : JSON.stringify(o), "utf8"), path.join(D, f));

console.log("技能包生成器产物自检 —— 量的是解开后的字节，不是退出码\n");

// ── 1. XML 非法控制字符：三个生成器共用同一个 esc()，是同一个缺陷的三个出口 ──────────
{
  const md = w("in.md", `# 标题${VT}A\n\n正文${VT}文本\n`);
  let r = run("docx/scripts/gen-docx.mjs", ["--md", md, "--out", `${D}/t.docx`, "--json"]);
  ok("gen-docx 生成成功", r.code === 0 && r.out.includes('"ok":true'), r.out.slice(0, 200));
  ok("gen-docx: word/document.xml 无 XML 非法控制字符", !badXml(unzip(`${D}/t.docx`)["word/document.xml"]));

  const book = w("book.json", { sheets: [{ name: "S", rows: [["备注"], [`脏${VT}数据`]] }] });
  r = run("xlsx/scripts/gen-xlsx.mjs", ["--in", book, "--out", `${D}/t.xlsx`, "--json"]);
  ok("gen-xlsx 生成成功", r.code === 0 && r.out.includes('"ok":true'), r.out.slice(0, 200));
  ok("gen-xlsx: xl/sharedStrings.xml 无 XML 非法控制字符", !badXml(unzip(`${D}/t.xlsx`)["xl/sharedStrings.xml"]));

  const deck = w("deck.json", { title: `封面${VT}页`, slides: [{ title: `标题${VT}`, bullets: [`要点${VT}一`] }] });
  r = run("ppt/scripts/gen-pptx.mjs", ["--in", deck, "--out", `${D}/t.pptx`, "--json"]);
  ok("gen-pptx 生成成功", r.code === 0 && r.out.includes('"ok":true'), r.out.slice(0, 200));
  const z = unzip(`${D}/t.pptx`);
  const slides = Object.keys(z).filter((k) => /^ppt\/slides\/slide\d+\.xml$/.test(k));
  ok("gen-pptx: 至少有一个 slide 部件", slides.length > 0, Object.keys(z).join(","));
  ok("gen-pptx: ppt/slides/*.xml 无 XML 非法控制字符", slides.every((k) => !badXml(z[k])));
}

// ── 2. xlsx 数字保真：前导零和长号码不许被双精度悄悄改写 ────────────────────────────
{
  const book = w("num.json", {
    sheets: [{
      name: "S",
      rows: [["工号", "身份证", "金额"],
        ["007", "110101199003078888", "12.5"],
        ["0012", "12345678901234567890", "-3.25"]],
    }],
  });
  const r = run("xlsx/scripts/gen-xlsx.mjs", ["--in", book, "--out", `${D}/n.xlsx`, "--json"]);
  ok("gen-xlsx 数字用例生成成功", r.code === 0, r.out.slice(0, 200));
  const z = unzip(`${D}/n.xlsx`);
  const ss = [...(z["xl/sharedStrings.xml"] || "").matchAll(/<t[^>]*>(.*?)<\/t>/g)].map((m) => m[1]);
  const sheet = z["xl/worksheets/sheet1.xml"] || "";
  for (const v of ["007", "0012", "110101199003078888", "12345678901234567890"]) {
    ok(`gen-xlsx: ${v} 按文本存（不被改写）`, ss.includes(v), `sharedStrings = ${ss.join(" | ")}`);
  }
  // 反向：真数字必须还是数值，否则「修好了保真、顺手废掉了求和」
  ok("gen-xlsx: 12.5 仍是数值单元格（可求和）", /<c r="C2"[^>]*><v>12\.5<\/v><\/c>/.test(sheet), sheet.slice(0, 300));
  ok("gen-xlsx: -3.25 仍是数值单元格", /<c r="C3"[^>]*><v>-3\.25<\/v><\/c>/.test(sheet));
}

// ── 3. 邮件头注入：CRLF 必须被拒，且拒的时候不许落盘 ────────────────────────────────
{
  const evil = w("evil.json", { to: ["victim@abc.com\r\nBcc: attacker@evil.com"], subject: "hi", text: "b" });
  let r = run("mail/scripts/gen-eml.mjs", ["--in", evil, "--out", `${D}/evil.eml`, "--json"]);
  ok("gen-eml: 收件人里的 CRLF 被拒", r.code === 1 && r.out.includes('"ok":false'), r.out.slice(0, 200));
  ok("gen-eml: 被拒时不落盘", !fs.existsSync(`${D}/evil.eml`));

  // 🔴 主题这条必须单独测：中文主题走 base64（CRLF 被编成惰性的 DQo，看着像「本来就安全」），
  //    纯 ASCII 主题**不走** base64 —— 同一个字段两条路径，只测一条等于没测。
  const evil2 = w("evil2.json", { to: ["a@b.com"], subject: "hi\r\nBcc: attacker@evil.com", text: "b" });
  r = run("mail/scripts/gen-eml.mjs", ["--in", evil2, "--out", `${D}/evil2.eml`, "--json"]);
  ok("gen-eml: 纯 ASCII 主题里的 CRLF 被拒", r.code === 1 && r.out.includes('"ok":false'), r.out.slice(0, 200));
}

// ── 4. 正常路径没被护栏误伤（只验坏输入会把好输入一起挡掉都发现不了）──────────────────
{
  const good = w("good.json", {
    from: "我 <me@corp.com>", to: ["张经理 <zhang@abc.com>", "b@c.com"], cc: "cc@abc.com",
    subject: "关于 3 月货款的付款提醒", text: "张经理您好：\n\n请查收。", html: "<p>你好</p>",
  });
  const r = run("mail/scripts/gen-eml.mjs", ["--in", good, "--out", `${D}/good.eml`, "--json"]);
  ok("gen-eml: 正常邮件仍能生成", r.code === 0 && r.out.includes('"ok":true'), r.out.slice(0, 200));
  const eml = fs.readFileSync(`${D}/good.eml`, "utf8");
  ok("gen-eml: 中文显示名/主题仍走 RFC2047 编码", eml.includes("=?UTF-8?B?") && /^To: .*<zhang@abc\.com>/m.test(eml),
    eml.split("\r\n").slice(0, 6).join(" / "));

  const md = w("ok.md", '# 标题\n\n正文 & <b> "引号"\n\n- 项一\n- 项二\n');
  run("docx/scripts/gen-docx.mjs", ["--md", md, "--out", `${D}/ok.docx`, "--json"]);
  const xml = unzip(`${D}/ok.docx`)["word/document.xml"] || "";
  ok("gen-docx: `&` `<` 仍正确转义且无双重转义", xml.includes("&amp;") && xml.includes("&lt;b&gt;") && !xml.includes("&amp;amp;"));
}

fs.rmSync(D, { recursive: true, force: true });
console.log(fails ? `\n技能包产物自检：${fails} 条红` : "\n技能包产物自检：全绿");
process.exit(fails ? 1 : 0);
