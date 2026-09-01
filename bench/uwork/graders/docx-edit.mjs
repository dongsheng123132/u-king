/**
 * 判分：改客户已有的 Word。
 *
 * 这条是 U-King 跟「豆包/通用聊天机器人」拉开差距的地方，也是最容易**静默降级**的地方：
 * 模型很容易「读出来 → 重新生成一份新 docx」，文字全对、客户打开一看
 * 页眉页脚样式编号全没了 —— 而且要等他打开那一刻才发现。
 *
 * 所以判分不止看文字，还要**逐个 ZIP 部件比对 CRC32**：没被改动的部件必须跟原件字节级相同。
 * 只要 styles.xml 之类变了，就是重画的，判 fail。
 */
import fs from "node:fs";
import path from "node:path";
import { readZip, docxText } from "../lib/zip.mjs";

const SEED = "设备采购合同-原件.docx";
const OLD_NAME = "天成电子科技有限公司";
const NEW_NAME = "天成智能科技（杭州）有限公司";

export async function grade({ ws }) {
  const checks = [];
  const add = (n, ok, d) => checks.push({ name: n, ok: !!ok, detail: d || "" });

  const files = fs.readdirSync(ws);
  const out = files.find((f) => f.endsWith(".docx") && f !== SEED);
  add("生成了修订版 .docx", !!out, out || `除原件外没有新的 .docx（有: ${files.join(", ")}）`);
  if (!out) return { pass: false, checks };

  let text, outZip, seedZip;
  try {
    text = docxText(path.join(ws, out));
    outZip = readZip(path.join(ws, out));
    seedZip = readZip(path.join(ws, SEED));
  } catch (e) { add("修订版能被 Word 解析", false, e.message); return { pass: false, checks }; }
  add("修订版能被 Word 解析", true, `${out}，${outZip.length} 个部件`);

  add("甲方已改成新名称", text.includes(NEW_NAME), text.includes(NEW_NAME) ? "" : "找不到新公司名");
  add("旧名称已全部替换干净", !text.includes(OLD_NAME), text.includes(OLD_NAME) ? "旧名还在文里——改了一处漏了一处" : "");
  add("签订日期已改成 4 月 8 日", /2026\s*年\s*4\s*月\s*8\s*日/.test(text), "");

  // 其它内容一个字都不该动
  for (const [label, needle] of [["乙方名称", "宏远机械制造有限公司"], ["合同总价", "32160"], ["违约金条款", "百分之五"], ["交付日期", "6 月 30 日"]]) {
    const ok = text.replace(/\s/g, "").includes(needle.replace(/\s/g, ""));
    add(`没动到无关内容（${label}）`, ok, ok ? "" : `${needle} 不见了`);
  }

  // ★ 格式没丢：未改动部件必须字节级相同
  const seedMap = new Map(seedZip.map((e) => [e.name, e]));
  const outMap = new Map(outZip.map((e) => [e.name, e]));
  const missing = [...seedMap.keys()].filter((k) => !outMap.has(k));
  add("部件一个没少", missing.length === 0, missing.length ? `丢了: ${missing.join(", ")}` : "");

  const shouldBeIdentical = [...seedMap.keys()].filter((k) => k !== "word/document.xml" && outMap.has(k));
  const changed = shouldBeIdentical.filter((k) => seedMap.get(k).crc32 !== outMap.get(k).crc32);
  add("未改动部件字节级相同（样式/排版没丢）", changed.length === 0,
      changed.length ? `这些部件被重写了: ${changed.join(", ")} —— 说明是重新生成的新文档，不是在原件上改` : `${shouldBeIdentical.length} 个部件 CRC 全部一致`);

  return { pass: checks.every((c) => c.ok), checks };
}
