/**
 * 「预览里的文字能不能选中复制」的跑道 —— **真组件 + 真 CSS + 真鼠标拖选**，不截屏。
 *
 * ## 为什么值得单开一条
 * 客户原话：「预览里的文字无法选中复制」。这个 bug 的成因**没有一个能被 tsc / 单测 /
 * conformance 看见**，它们全都长在 CSS 与命中测试上：
 *  1. `globals.css` 给 body 设了 `user-select:none`（防误选整个界面），预览正文跟着被禁；
 *  2. PDF 那条更彻底：canvas 上的字是**像素**，DOM 里一个字都没有，怎么选都选不中。
 * 两条都表现为「界面看着完全正常、就是选不动」——只能靠真拖一次来证。
 *
 * > 历史：这里原本还有第三条成因「各 viewer 铺的标注层容器吃掉鼠标事件」，以及一条配套的
 * > 反向断言「标注工具条必须仍然点得动」。标注功能已在 1.0.3 整个移除（它导出的锚点是假的），
 * > 那条成因和那条反向断言随之作废 —— **一并删掉，不留一个永远绿的空断言**。
 *
 * ## 真在哪儿、假在哪儿（不含糊）
 *  - **真**：`RedlinePanel` / 各 viewer 本体（不复制一份）、真 `globals.css`、
 *    真 Chromium、真 `mouse.down/move/up` 拖选、真 `window.getSelection()`。
 *  - **假**：只有 `host.readFileBytes` 这一层 —— 浏览器里没有 Tauri，字节由跑道现造
 *    （txt 是一段中英混排；md 是一小段带标题的 markdown；pdf 是手写的最小合法 PDF）。
 *  - **验不到**：Ctrl+C 之后系统剪贴板里到底有没有（那是 OS 的事）。所以断言停在
 *    「选区文本 = 期望的那段字」——剪贴板那半边由 `lib/clipboard.ts` 自己的路负责。
 *
 * ## markdown 那一步同时是 MdViewer 的闸门
 * `.md` 从 1.0.3 起走 MdViewer（渲染器由宿主注入）。第 2 步除了验「选得中」，还**反向断言
 * 页面上没有原始 `#` 标题符号** —— 否则说明 renderMarkdown 没接上、退化成了直出源码，
 * 而那种情况下文字照样选得中，只验选中会绿着骗人。
 *
 * 用法：先 `pnpm dev`，再 `node scripts/check-preview-select.mjs`（换端口用 UKING_DEV_URL=）。
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync } from "node:fs";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";

/** 纯文本 fixture —— 中英混排，且**不含空格**的一段用于精确断言选区。 */
const TXT = "U-King 预览选中测试：ABCDEFG-可选中-1234567890";

/** markdown fixture —— 标题必须被渲染掉（页面上不该再出现行首的 `#`）。 */
const MD_HEADING = "MdViewerHeading42";
const MD_BODY = "MdViewerBody-可选中-98765";
const MD = `# ${MD_HEADING}\n\n${MD_BODY}\n`;

/**
 * 最小合法 PDF（未压缩、内置 Helvetica），正文一行已知文字。
 * 手写而不是塞个二进制 fixture：跑道要能被人读懂、也不该往仓库里加二进制。
 * 偏移量用占位后回填，省得手算 xref。
 */
function makePdf(text) {
  const objs = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 150] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
    null, // 4 = 内容流，下面单独拼
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ];
  const stream = `BT /F1 18 Tf 20 80 Td (${text}) Tj ET`;
  objs[3] = `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`;

  let out = "%PDF-1.4\n";
  const offsets = [];
  objs.forEach((body, i) => {
    offsets.push(out.length);
    out += `${i + 1} 0 obj\n${body}\nendobj\n`;
  });
  const xref = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`;
  for (const off of offsets) out += `${String(off).padStart(10, "0")} 00000 n \n`;
  out += `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return out;
}

/** PDF 里只放 ASCII —— PDF 的默认编码不认中文，塞中文会渲染成空白，跑道就在验一个假目标。 */
const PDF_TEXT = "SELECTABLE-PDF-TEXT-42";
const PDF_SRC = makePdf(PDF_TEXT);

const PROBE_NAME = "__preview-select-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>preview select probe</title></head>
<body><div id="root"></div><script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import "/src/globals.css";
import { I18nProvider } from "/src/i18n";
import { RedlinePanel } from "/src/vendor/redline-core/index";
// 真 MiniMd —— markdown 那一步验的就是「宿主注入的渲染器接上了没」，用假的等于没验
import { MiniMd } from "/src/lib/miniMd";

const TXT = ${JSON.stringify(TXT)};
const MD = ${JSON.stringify(MD)};
const PDF_SRC = ${JSON.stringify(PDF_SRC)};
const enc = (s) => { const a = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i) & 0xff; return a.buffer; };

// 只有取字节这一层是假的：浏览器里没有 Tauri。
const host = {
  readFileBytes: async (p) =>
    p.endsWith(".pdf") ? enc(PDF_SRC)
    : p.endsWith(".md") ? new TextEncoder().encode(MD).buffer
    : new TextEncoder().encode(TXT).buffer,
  renderMarkdown: (text) => React.createElement(MiniMd, { text }),
};

function Probe() {
  const [path, setPath] = React.useState("/probe/sample.txt");
  React.useEffect(() => { window.__open = (p) => setPath(p); window.__ready = true; }, []);
  return React.createElement(
    "div",
    { style: { height: "600px", width: "900px" } },
    React.createElement(RedlinePanel, { key: path, host, path, fileName: path.split("/").pop() }),
  );
}
createRoot(document.getElementById("root")).render(React.createElement(I18nProvider, null, React.createElement(Probe)));
</script></body></html>`;

/** 在给定元素上真拖一遍鼠标，返回选中的文本。 */
async function dragSelect(page, box) {
  await page.mouse.move(box.x + 4, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 4, box.y + box.height / 2, { steps: 12 });
  await page.mouse.up();
  return await page.evaluate(() => String(window.getSelection() ?? ""));
}

const fails = [];
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1000, height: 700 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));

writeFileSync(PROBE_NAME, PROBE_HTML);
process.on("exit", () => {
  try {
    unlinkSync(PROBE_NAME);
  } catch {
    /* ignore */
  }
});

console.log("[1/4] 挂真 RedlinePanel（真 globals.css）…");
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 20000 }).catch(() => {
  fails.push("探针没挂起来（`pnpm dev` 起了吗？）");
});

console.log("[2/4] 文本预览：真拖一遍…");
{
  const pre = page.locator("pre").first();
  await pre.waitFor({ timeout: 15000 }).catch(() => fails.push("文本预览没渲染出来"));
  const box = await pre.boundingBox();
  if (!box) {
    fails.push("文本预览拿不到位置");
  } else {
    const sel = await dragSelect(page, box);
    if (!sel.trim()) {
      fails.push("文本预览：拖了一遍**一个字都没选中** —— 客户说的就是这个");
    } else if (!TXT.includes(sel.trim().slice(0, 6))) {
      fails.push(`文本预览：选到的不是正文（选到了「${sel.slice(0, 20)}」）`);
    } else {
      console.log(`     ✓ 选中了「${sel.trim().slice(0, 24)}…」`);
    }
  }
}

console.log("[3/4] markdown：要**渲染**出来（不是直出源码），且选得中…");
{
  await page.evaluate(() => window.__open("/probe/sample.md"));
  const heading = page.getByText(MD_HEADING, { exact: false }).first();
  await heading.waitFor({ timeout: 15000 }).catch(() => fails.push("markdown 预览没渲染出来"));
  // 反向断言：渲染器没接上时会退回 <pre> 直出源码 —— 那时行首的 `#` 还在，而文字照样选得中。
  // 只验「选得中」会绿着骗人，必须同时验「源码符号没了」。
  const raw = await page.evaluate(() => document.body.innerText || "");
  if (raw.includes("# " + MD_HEADING)) {
    fails.push("markdown 直出了源码（`# 标题` 原样显示）—— host.renderMarkdown 没接上");
  } else if (!raw.includes(MD_HEADING)) {
    fails.push("markdown 标题内容整个没出现");
  } else {
    console.log("     ✓ 标题被渲染（页面上没有原始 `#`）");
  }
  const box = await heading.boundingBox();
  if (box) {
    const sel = await dragSelect(page, box);
    if (!sel.trim()) fails.push("markdown：渲染出来了但拖不出选区");
    else console.log(`     ✓ 选中了「${sel.trim().slice(0, 24)}」`);
  }
}

console.log("[4/4] PDF 预览：文字层要在，且选得中…");
{
  await page.evaluate(() => window.__open("/probe/sample.pdf"));
  const spans = page.locator(".textLayer span");
  await spans.first().waitFor({ timeout: 25000 }).catch(() => {
    fails.push("PDF 没有文字层 —— canvas 上的字是像素，客户永远选不中");
  });
  const n = await spans.count();
  if (n > 0) {
    const box = await spans.first().boundingBox();
    const sel = box ? await dragSelect(page, box) : "";
    if (!sel.trim()) {
      fails.push("PDF：文字层在，但拖不出选区（标注层还压在上面？）");
    } else if (!PDF_TEXT.includes(sel.trim().slice(0, 5))) {
      fails.push(`PDF：选到的不是正文（选到了「${sel.slice(0, 20)}」）`);
    } else {
      console.log(`     ✓ ${n} 个文字 span，选中了「${sel.trim().slice(0, 24)}」`);
    }
  }
}

await browser.close();

if (errors.length) console.log("（页面错误：" + errors.slice(0, 3).join(" | ") + "）");
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 预览可选中复制：文本 / markdown / PDF 都能真拖出选区，且 markdown 是渲染的不是源码");
