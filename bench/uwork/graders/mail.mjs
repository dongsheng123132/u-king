/**
 * 判分：读已有 Excel → 写催款邮件。
 *
 * 这条任务真正在测的是**跨文件一致性**：金额必须从 3月对账单.xlsx 里读出来（188790），
 * 不是凭印象写。办公事故九成出在这儿 —— 邮件发出去了，附件里的数跟正文对不上。
 */
import fs from "node:fs";
import path from "node:path";

/** RFC2047 编码字还原（=?UTF-8?B?…?= / =?UTF-8?Q?…?=），顺带拼掉折行。 */
function decodeHeader(s) {
  return String(s).replace(/\?=\s*\r?\n\s+=\?/g, "?==?").replace(/\r?\n\s+/g, "")
    .replace(/=\?([^?]+)\?([BbQq])\?([^?]*)\?=/g, (_, cs, enc, data) => {
      try {
        if (enc.toUpperCase() === "B") return Buffer.from(data, "base64").toString("utf8");
        return Buffer.from(data.replace(/_/g, " ").replace(/=([0-9A-Fa-f]{2})/g, (_, h) => String.fromCharCode(parseInt(h, 16))), "binary").toString("utf8");
      } catch { return data; }
    });
}
/** 把 .eml 里所有 text/* 段解码拼起来（base64 / quoted-printable / 7bit 都认）。 */
function bodyText(raw) {
  const parts = raw.split(/\r?\n\r?\n/);
  let out = "";
  for (let i = 0; i < parts.length - 1; i++) {
    const head = parts[i], body = parts[i + 1];
    if (!/content-type:\s*text\//i.test(head) && i !== 0) continue;
    const enc = (head.match(/content-transfer-encoding:\s*(\S+)/i) || [])[1] || "7bit";
    try {
      if (/base64/i.test(enc)) out += Buffer.from(body.replace(/[\r\n]/g, "").split("--")[0], "base64").toString("utf8") + "\n";
      else if (/quoted-printable/i.test(enc)) out += body.replace(/=\r?\n/g, "").replace(/=([0-9A-Fa-f]{2})/g, (_, h) => String.fromCharCode(parseInt(h, 16))) + "\n";
      else out += body + "\n";
    } catch {}
  }
  return out + "\n" + raw; // 兜底：连原文一起给断言看，避免解码失败误杀
}

export async function grade({ ws }) {
  const checks = [];
  const add = (n, ok, d) => checks.push({ name: n, ok: !!ok, detail: d || "" });

  const files = fs.readdirSync(ws);
  const eml = files.find((f) => f.toLowerCase().endsWith(".eml"));
  add("生成了 .eml 邮件草稿", !!eml, eml || `目录里没有 .eml（有: ${files.join(", ")}）`);
  if (!eml) return { pass: false, checks };

  const raw = fs.readFileSync(path.join(ws, eml), "utf8");
  const hdr = (k) => decodeHeader((raw.match(new RegExp(`^${k}:\\s*([\\s\\S]*?)\\r?\\n(?![ \\t])`, "mi")) || [])[1] || "");
  const to = hdr("To"), cc = hdr("Cc"), subject = hdr("Subject");
  const body = bodyText(raw);

  add("收件人正确", /zhangjg@tiancheng-elec\.com/i.test(to), `To: ${to || "(空)"}`);
  add("抄送正确", /finance@hongyuan-mech\.com/i.test(cc), `Cc: ${cc || "(空)"}`);
  add("主题具体、能单独看懂", subject.length >= 8 && !/^(你好|hi|hello|无标题)$/i.test(subject.trim()), `Subject: ${subject || "(空)"}`);

  // ★ 金额必须跟对账单一致：188790 / 188,790 / 188 790 都算对
  const money = /188[,\s]?790/.test(body);
  const wrongMoney = (body.match(/1[89]\d[,\s]?\d{3}/g) || []).filter((m) => !/188[,\s]?790/.test(m));
  add("正文金额与对账单一致（188790）", money,
      money ? "" : wrongMoney.length ? `正文里出现的是 ${[...new Set(wrongMoney)].join("/")} —— 跟附件对不上，这封信发出去就是事故` : "正文里找不到合计金额");

  add("有明确付款期限", /(前|之前|以内|日内|截止|期限)/.test(body) && /\d/.test(body), "");
  add("落款完整（李静 / 财务部）", /李静/.test(body) && /财务/.test(body), "");

  const hasAtt = /content-disposition:\s*attachment/i.test(raw);
  const attName = decodeHeader((raw.match(/filename="?([^"\r\n;]+)"?/i) || [])[1] || "");
  add("带上了对账单附件", hasAtt && /对账单/.test(attName), hasAtt ? `附件: ${attName}` : "没有任何附件");

  // 邮件是**草稿**不是已发送：这条不通过说明有人接了 SMTP，产品红线上要拦
  add("只写草稿、没有自动发信", !/^Message-ID:.*smtp/im.test(raw), "");

  return { pass: checks.every((c) => c.ok), checks };
}
