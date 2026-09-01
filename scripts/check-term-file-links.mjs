/**
 * 「终端里的文件路径点得动吗」的跑道 —— 真 xterm + 真鼠标，不截屏。
 *
 * ## 为什么值得单开一条
 * 客户原话：「终端里的文件，无法点击预览，右侧选择打开方式，连复制都没」。
 * AI 干完活最后一句往往是「已生成 D:\xx\报告.docx」——这行字在终端里原本是**死的**。
 * 这条链路一个字节都不在动作表里：它是「一行文本 → 认出路径 → xterm 链接 → 鼠标命中 → 回调」，
 * conformance / cargo test / tsc 全都看不见。而它坏起来是静默的：不画下划线、点了没反应。
 *
 * ## 两层，分开验
 *  - **第一层（纯函数，不开浏览器）**：`findPathsInLine` 对着真实 CLI 输出行断言。
 *    正例要认出来，**反例一个都不许认**——满屏英文单词全画下划线比不画还糟。
 *  - **第二层（真 xterm）**：把一行写进真终端，用真 `mouse.move/click/right-click`
 *    打在那个字符的像素位置上，断言回调拿到的是**拼好 cwd 的绝对路径**。
 *    这一层证的是「链接真的被 xterm 命中了」——正则再对，range 差一格也点不着。
 *
 * ## 真在哪儿、假在哪儿
 *  - **真**：`fileLinks.ts` 本体（两层用的是同一份，不复制）、真 `@xterm/xterm`、真鼠标事件。
 *  - **假**：没有 PTY —— 文本用 `term.write()` 直接灌，因为要验的是「认路径」不是「跑 shell」。
 *
 * 用法：`node --experimental-strip-types scripts/check-term-file-links.mjs`
 *       （第二层要 `pnpm dev`；只想跑第一层加 `--pure`，换端口用 UKING_DEV_URL=）
 */
import { chromium } from "playwright";
import { writeFileSync, unlinkSync, readFileSync } from "node:fs";
import { findPathsInLine, resolvePath, dirHintsFromLine, candidatePaths } from "../src/opencodex/term/fileLinks.ts";

const URL = process.env.UKING_DEV_URL || "http://localhost:1430/";
const PURE_ONLY = process.argv.includes("--pure");
const fails = [];

console.log("[1/2] 纯函数：真实 CLI 输出行认路径…");

/** [一行终端输出, 期望认出来的路径…] —— 全部抄自 claude / codex / hermes / git / npm 的真实输出。 */
const POSITIVE = [
  ["已生成 D:\\工作\\报告.docx", ["D:\\工作\\报告.docx"]],
  ["Wrote C:/Users/example/Desktop/out.md", ["C:/Users/example/Desktop/out.md"]],
  ["见 D:\\a\\b.txt。", ["D:\\a\\b.txt"]],
  ["(src/main.rs)", ["src/main.rs"]],
  ["  modified:   src/opencodex/term/useTermGroup.ts", ["src/opencodex/term/useTermGroup.ts"]],
  ["Updated .\\dist\\app.exe", [".\\dist\\app.exe"]],
  ['saved to "C:\\Program Files\\我的 文件.pdf"', ["C:\\Program Files\\我的 文件.pdf"]],
  ["cat /c/Users/example/.uking/llms.txt", ["/c/Users/example/.uking/llms.txt"]],
  ["报告.docx 已经写好了", ["报告.docx"]],
];

/** 反例：这些行里**一个路径都不许认**（认了 = 满屏下划线）。 */
const NEGATIVE = [
  "Installing dependencies, please wait...",
  "error: expected one of `,` or `}`, found `;`",
  "✓ built in 11.24s",
  "总共 89 个动作，68 通过 0 失败",
  "PS C:\\Users\\me>",  // 提示符本身：认了会把每行开头都画上下划线
  // 长得像路径但不在本机 —— 认了 = 用户点开必然 404（跟网址被当文件读同一类错）
  "diff --git a/src/x.ts b/src/x.ts",              // a/ b/ 是 git 编的前缀，磁盘上没有
  "git@github.com:dongsheng123132/u-king-mini.git",// 冒号右边在别人机器上
  "2origin-site.pages.dev 这个域名",                 // 裸域名不给链接（保守：宁可漏）
];

/**
 * [一行, 这一截**不许**出现在结果里]。用于「同一行里有的该认、有的不该认」。
 * scp 那行的 `out.zip` 是**本机**源文件，该认；冒号右边的 `/tmp/out.zip` 在**别人机器**上，不该认。
 * 一开始我把整行塞进 NEGATIVE，被跑道当场纠正 —— 反例写太宽会把对的一起判死。
 */
const PARTIAL_NEGATIVE = [
  ["scp out.zip deploy@example.com:/tmp/out.zip", "/tmp/out.zip"],
];

/**
 * 网址：[一行, 期望认出来的网址…]。全部抄自客户 2026-08-16 那张截图和本机真实输出。
 *
 * 🔴 这一组以前**一条都过不了**，而且是两种错法：
 *   - `U="https://…"` 被认成 `s://…`（`https` 的最后一个字母当了盘符），点了走文件预览
 *     报「系统找不到指定的路径 (os error 3)」；
 *   - 结尾是 `/` 或没扩展名的（`/docs/`、`localhost:1430/`、`issues/379`）被
 *     「必须带扩展名」那道闸挡掉，**一条都认不出来**。
 * 两种错法合起来就是客户说的「网址识别不对、不准、没法打开」。
 */
const URLS = [
  ['sleep 5; U="https://2origin-site.pages.dev"; for p in / /docs; do', ["https://2origin-site.pages.dev"]],
  ["https://2origin-site.pages.dev/docs/rfc-0000/", ["https://2origin-site.pages.dev/docs/rfc-0000/"]],
  ["  → 200  https://2origin-site.pages.dev/docs/bugscope/", ["https://2origin-site.pages.dev/docs/bugscope/"]],
  ["Local:   http://localhost:1430/", ["http://localhost:1430/"]],
  ["文档见 https://u-claw.org.cn/uking/ 里那页", ["https://u-claw.org.cn/uking/"]],
  ["see https://github.com/dongsheng123132/u-king-mini/issues/379 (已修)", ["https://github.com/dongsheng123132/u-king-mini/issues/379"]],
  ["Dashboard: http://127.0.0.1:18789/#token=uclaw", ["http://127.0.0.1:18789/#token=uclaw"]],
  ["报名 https://example.com/a?b=1&c=2 。", ["https://example.com/a?b=1&c=2"]],   // 尾部中文句号不算网址
  ["打开 file:///D:/work/out.html 看看", ["file:///D:/work/out.html"]],
  ["curl -sL https://api.u-claw.org.cn/v1 -o /dev/null", ["https://api.u-claw.org.cn/v1"]],
];

for (const [line, want] of POSITIVE) {
  const got = findPathsInLine(line).map((h) => h.text);
  for (const w of want) {
    if (!got.includes(w)) fails.push(`认不出「${w}」← ${line}（认到的是 ${JSON.stringify(got)}）`);
  }
  // 下标必须对得上原文，否则 xterm 的 range 会画歪
  for (const h of findPathsInLine(line)) {
    if (line.slice(h.start, h.end) !== h.text) fails.push(`下标对不上：${h.text} @ ${h.start}-${h.end} ← ${line}`);
  }
}
for (const line of NEGATIVE) {
  const got = findPathsInLine(line).map((h) => h.text);
  if (got.length) fails.push(`不该认却认了 ${JSON.stringify(got)} ← ${line}`);
}
for (const [line, forbidden] of PARTIAL_NEGATIVE) {
  const got = findPathsInLine(line).map((h) => h.text);
  if (got.includes(forbidden)) fails.push(`不该认「${forbidden}」（不在本机）← ${line}`);
}
// —— resolvePath：终端链接和 U-Chat 预览**共用这一份**（Chat.tsx 以前自己写了一份，
//    在 Mac 上会把 `/Users/example/ws` + `out.md` 拧成 `\Users\example\ws\out.md` → asset 404）——
const RESOLVE = [
  ["out.md", "D:\\work", "D:\\work\\out.md", "Windows 相对路径"],
  [".\\out.md", "D:\\work", "D:\\work\\out.md", "Windows ./ 前缀"],
  ["D:\\a.txt", "D:\\work", "D:\\a.txt", "Windows 绝对路径不许被 cwd 污染"],
  ["out.md", "/Users/example/ws", "/Users/example/ws/out.md", "🔴 Mac 相对路径必须用 / 拼"],
  ["./out.md", "/Users/example/ws", "/Users/example/ws/out.md", "Mac ./ 前缀"],
  ["sub/out.md", "/Users/example/ws", "/Users/example/ws/sub/out.md", "Mac 多级相对路径"],
  ["/Users/example/a.md", "/Users/example/ws", "/Users/example/a.md", "Mac 绝对路径不许被 cwd 污染"],
  ["out.md", "/Users/example/ws/", "/Users/example/ws/out.md", "cwd 末尾斜杠不许拼出双斜杠"],
];
for (const [raw, cwd, want, why] of RESOLVE) {
  const got = resolvePath(raw, cwd);
  if (got !== want) fails.push(`${why}：resolvePath(${JSON.stringify(raw)}, ${JSON.stringify(cwd)}) = ${JSON.stringify(got)}，应为 ${JSON.stringify(want)}`);
}
// —— 目录线索 + 候选路径：治「终端那行只有文件名、目录写在上面另一行」——
// 客户 2026-08-16 实锤：右键那个 zip，六项菜单全废，其中「复制路径」还**不声不响复制了错路径**。
{
  // ⚠️ 反引号原始串**不能以反斜杠结尾**（它会把收尾的反引号转义掉，整个文件当场语法错）——
  //    所以目录行拼出来，别直接写在 String.raw 里。
  const DIR = String.raw`D:\工作项目\2origin本源计算机商业化+skill推广计划\GOAI初赛-第3次提交-20260816`;
  const DIRLINE = DIR + "\\";
  const NAME = "2origin本象计算机-AgentInfra初赛提交包-v3-20260816.zip";

  // ① 目录行要能被认出来（结尾带分隔符 = 目录本身）
  const hints = dirHintsFromLine(DIRLINE);
  if (hints[0] !== DIR) fails.push(`目录行没认出来：${JSON.stringify(hints)}`);
  // ② 带文件名的行 → 取上一级目录
  const h2 = dirHintsFromLine(String.raw`Wrote D:\a\b\x.txt`);
  if (h2[0] !== String.raw`D:\a\b`) fails.push(`没取上级目录：${JSON.stringify(h2)}`);
  // ③ Mac 侧同理
  const h3 = dirHintsFromLine("saved /Users/example/ws/out/report.pdf ok");
  if (h3[0] !== "/Users/example/ws/out") fails.push(`Mac 上级目录不对：${JSON.stringify(h3)}`);
  // ④ 没有绝对路径的行不许瞎猜
  if (dirHintsFromLine("Installing dependencies, please wait...").length) fails.push("普通英文行被当成目录线索了");

  // ⑤ 裸文件名 → 候选里必须**同时**有「终端当前目录」和「上文那个目录」，且顺序是前者优先
  const cands = candidatePaths(NAME, String.raw`D:\工作项目`, hints);
  if (cands[0] !== String.raw`D:\工作项目` + "\\" + NAME) fails.push(`第一候选应是终端 cwd 拼的：${cands[0]}`);
  if (!cands.includes(DIR + "\\" + NAME)) fails.push(`候选里缺了上文那个目录：${JSON.stringify(cands)}`);
  // ⑥ 文本自己带了路径就照它说的算，不许拿线索乱拼
  const c2 = candidatePaths(String.raw`sub\a.txt`, String.raw`D:\ws`, hints);
  if (c2.length !== 1) fails.push(`带路径的文本不该有多候选：${JSON.stringify(c2)}`);
}

// —— 目录也要能点（客户 2026-08-17：「比如打开对应的文件夹」）——
// 「必须带扩展名」那道闸原本把**所有目录**挡在外面。放宽判据是「结尾带分隔符」——
// 那是明写着的目录标记，而 PowerShell 提示符结尾是 `>`，够不着这条。
{
  const BS = String.fromCharCode(92);
  const cases = [
    ["格子那件 →demo" + BS + "SUBMIT-格子-20260816" + BS, "demo" + BS + "SUBMIT-格子-20260816" + BS, "箭头要削掉、目录要认出来"],
    ["打包到 D:" + BS + "工作" + BS + "交付-20260816" + BS, "D:" + BS + "工作" + BS + "交付-20260816" + BS, "Windows 绝对目录"],
    ["cd /usr/local/", "/usr/local/", "Unix 目录"],
  ];
  for (const [line, want, why] of cases) {
    const hits = findPathsInLine(line);
    const hit = hits.find((h) => h.text === want);
    if (!hit) fails.push(`${why}：认到的是 ${JSON.stringify(hits.map((h) => h.text))} ← ${line}`);
    else if (!hit.isDir) fails.push(`${why}：没标成目录（isDir），点了会掉进文件预览：${want}`);
  }
  // 提示符仍然不许认 —— 放宽目录不能把这条老红线一起放掉
  if (findPathsInLine("PS C:" + BS + "Users" + BS + "devuser>").length) {
    fails.push("放宽目录之后，PowerShell 提示符又被认成链接了");
  }
}

// —— 文件名里有空格：正则只切得到最后一段，靠**候选 + 磁盘裁决**兜底 ——
// `- AI4R_OPEN 格子.zip` 只会切出 `格子.zip`。空格在终端里是天然分隔符，
// 光看文本没法判断它是名字的一部分还是两个词 —— 所以不猜，两种都列成候选让磁盘裁决。
{
  const BS = String.fromCharCode(92);
  const L = "- AI4R_OPEN 格子.zip ←传这个";
  const cands = candidatePaths("格子.zip", "D:" + BS + "demo", [], L.slice(0, L.indexOf("格子.zip")));
  if (!cands.some((c) => c.endsWith("AI4R_OPEN 格子.zip"))) {
    fails.push(`候选里缺「AI4R_OPEN 格子.zip」（带空格的真名）：${JSON.stringify(cands)}`);
  }
  if (!cands.some((c) => c.endsWith(BS + "格子.zip"))) {
    fails.push(`候选里缺短的那个：${JSON.stringify(cands)}`);
  }
  // 中文叙述不许被当成文件名的一部分（只往左吃一个 ASCII 词）
  const L2 = "已生成 报告.docx";
  const c2 = candidatePaths("报告.docx", "D:" + BS + "demo", [], L2.slice(0, L2.indexOf("报告.docx")));
  if (c2.length !== 1) fails.push(`「已生成」被吃进文件名了：${JSON.stringify(c2)}`);
}

// 🔴 静态守卫：上面那组只能证明 resolvePath 自己对，证明不了**别处没再手搓一份**。
// Chat.tsx 曾经就有一份 `(workspace + "\\" + p).replace(/\//g,"\\")` —— 在 Windows 上一直是绿的，
// 在 Mac 上把绝对路径整个拧断。跑道跑在 Windows，所以这类错只能靠源码级断言拦。
{
  // 先剥注释再判 —— 第一版没剥，被**注释里引用的那段旧代码**自己判红了（守卫也会误报）
  const chat = readFileSync("src/opencodex/Chat.tsx", "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(^|[^:])\/\/.*$/gm, "$1");
  if (/workspace\s*\+\s*["'`]\\\\/.test(chat)) {
    fails.push("Chat.tsx 又自己拼路径了（写死反斜杠 = Mac 上必炸）—— 请复用 fileLinks 的 resolvePath");
  }
}

// —— 网址：认得出 + 认得准 + **分类对**（kind=url 才不会掉进文件预览）——
for (const [line, want] of URLS) {
  const hits = findPathsInLine(line);
  const got = hits.map((h) => h.text);
  for (const w of want) {
    if (!got.includes(w)) fails.push(`网址认不出「${w}」← ${line}（认到的是 ${JSON.stringify(got)}）`);
  }
  for (const h of hits) {
    if (line.slice(h.start, h.end) !== h.text) fails.push(`下标对不上：${h.text} @ ${h.start}-${h.end} ← ${line}`);
    // 🔴 关键断言：凡是带 scheme 的，kind 必须是 url。分错 = 走文件预览 = os error 3。
    if (/^(https?|file):\/\//i.test(h.text) && h.kind !== "url") {
      fails.push(`网址被分类成「${h.kind}」，点了会去读文件：${h.text} ← ${line}`);
    }
    // 反过来：一条命令行里，网址不许把旁边的真路径挤掉
    if (h.kind === "path" && /^[a-z]:\/\//i.test(h.text)) {
      fails.push(`把 scheme 认成盘符了：${h.text} ← ${line}`);
    }
  }
}
// 混排：同一行里网址和文件路径各归各的
{
  const line = "已生成 D:\\工作\\报告.docx，已上传 https://u-claw.org.cn/uking/";
  const hits = findPathsInLine(line);
  const url = hits.find((h) => h.kind === "url");
  const path = hits.find((h) => h.kind === "path");
  if (url?.text !== "https://u-claw.org.cn/uking/") fails.push(`混排行里网址不对：${JSON.stringify(hits)}`);
  if (path?.text !== "D:\\工作\\报告.docx") fails.push(`混排行里文件路径不对：${JSON.stringify(hits)}`);
}
// 文件路径的 kind 不许被我改坏
for (const [line, want] of POSITIVE) {
  for (const h of findPathsInLine(line)) {
    if (want.includes(h.text) && h.kind !== "path") fails.push(`文件路径被分类成「${h.kind}」：${h.text} ← ${line}`);
  }
}
if (!fails.length) {
  console.log(`     ✓ ${POSITIVE.length} 条正例全中、${NEGATIVE.length} 条反例零误报、${URLS.length} 条网址认对且分类对`);
}

if (PURE_ONLY || fails.length) {
  if (fails.length) {
    console.error("\n❌ " + fails.length + " 条不达标：");
    for (const f of fails) console.error("  - " + f);
    process.exit(1);
  }
  console.log("\n✅ 纯函数层通过（--pure，没验鼠标命中）");
  process.exit(0);
}

console.log("[2/2] 真 xterm：把一行写进去，真点一下…");

// 一行里三样东西：中文裸文件名、相对路径、网址 —— 全角字符 + 混排是最容易算歪列号的场景。
const LINE = "已生成 报告.docx 和 src/main.rs，见 https://u-claw.org.cn/uking/";
const PROBE_URL = "https://u-claw.org.cn/uking/";
const CWD = "D:\\工作台";
// 折行用的长中文路径。🔴 **必须走 ${JSON.stringify(...)} 插值**，不能把字面量写进探针页 ——
// 探针页本身是个 JS 模板字符串，`\u7f16` 这种转义会被**处理两次**：模板先吃一层，
// 浏览器再看到 `\uking` 就报 Invalid Unicode escape，整页模块起不来（踩过一次）。
const WRAP_LINE = "D:\\工作项目\\图书排版-交付项目\\报价-零部件测绘及成图技术\\发给客户-20260816-样例包.zip（40 MB）";
const WRAP_WANT = "D:\\工作项目\\图书排版-交付项目\\报价-零部件测绘及成图技术\\发给客户-20260816-样例包.zip";
const PROBE_NAME = "__term-links-probe.html";
const PROBE_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>term links probe</title></head>
<body style="margin:0"><div id="host" style="width:900px;height:300px"></div><script type="module">
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { registerFileLinks } from "/src/opencodex/term/fileLinks.ts";

const term = new Terminal({ fontSize: 14, cols: 80, rows: 12 });
term.open(document.getElementById("host"));
window.__opened = null;
window.__openedUrl = null;
window.__menu = null;
registerFileLinks(term, document.getElementById("host"), {
  cwd: () => ${JSON.stringify(CWD)},
  onOpen: (p) => { window.__opened = p; },
  onOpenUrl: (u) => { window.__openedUrl = u; },
  onMenu: (info) => { window.__menu = info; },
});
term.write(${JSON.stringify(LINE)});
// 量一格多大 —— 跑道要按像素去点，得知道字符格的尺寸
window.__geom = () => {
  const row = document.querySelector(".xterm-rows > div");
  const r = row.getBoundingClientRect();
  return { left: r.left, top: r.top, height: r.height, cell: r.width / term.cols };
};
/**
 * 「这段文字落在第几列」——**问 xterm 自己的 buffer**，不问被测代码的宽度函数。
 * 故意不共用 fileLinks 的 displayWidth：那样两边一起算错也会一起绿，
 * 而「全角字符算成一格」正是这条跑道要抓的那类错。
 */
window.__colsOf = (needle) => {
  const line = term.buffer.active.getLine(0);
  let text = "";
  const marks = [];
  for (let x = 0; x < term.cols; x++) {
    const ch = line.getCell(x)?.getChars() ?? "";
    if (!ch) continue;
    marks.push({ x, at: text.length });
    text += ch;
  }
  const i = text.indexOf(needle);
  if (i < 0) return null;
  const startCol = marks.find((m) => m.at === i)?.x ?? null;
  let lastCol = null;
  for (const m of marks) if (m.at <= i + needle.length - 1) lastCol = m.x;
  return { startCol, lastCol };
};
// —— 折行跑道：窄终端 + 长中文路径（客户 2026-08-16 那条 40MB 的 zip 就是这样折的）——
const wrapHost = document.createElement('div');
wrapHost.style.width = '520px'; wrapHost.style.height = '200px';
document.body.appendChild(wrapHost);
const wrapTerm = new Terminal({ fontSize: 13, cols: 40, rows: 8 });
wrapTerm.open(wrapHost);
// 拦 registerLinkProvider 把 provider 抓出来 —— 不碰 xterm 私有字段（版本一升就没了）
const origReg = wrapTerm.registerLinkProvider.bind(wrapTerm);
wrapTerm.registerLinkProvider = (p) => { window.__wrapProvider = p; return origReg(p); };
registerFileLinks(wrapTerm, wrapHost, { cwd: () => 'D:/somewhere', onOpen: () => {}, onMenu: () => {} });
wrapTerm.write(${JSON.stringify(WRAP_LINE)});
window.__wrapLinks = (row) =>
  new Promise((res) => {
    if (!window.__wrapProvider) return res(null);
    window.__wrapProvider.provideLinks(row, (links) =>
      res((links ?? []).map((l) => ({ text: l.text, y0: l.range.start.y, y1: l.range.end.y }))),
    );
  });
window.__ready = true;
</script></body></html>`;

writeFileSync(PROBE_NAME, PROBE_HTML);
process.on("exit", () => {
  try {
    unlinkSync(PROBE_NAME);
  } catch {
    /* ignore */
  }
});

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1000, height: 500 } });
const errors = [];
// 页面里抛的错**当场打出来**：以前只攒到最后打，而探针一挂后面每一步都是
// 「window.__xxx is not a function」，真正的原因被埋在噪音后面。
page.on("pageerror", (e) => {
  errors.push(String(e).slice(0, 200));
  console.error("     [探针页报错] " + String(e).slice(0, 300));
});
await page.goto(URL + PROBE_NAME, { waitUntil: "networkidle" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 20000 }).catch(() => {
  fails.push("探针没挂起来（`pnpm dev` 起了吗？）");
});
await page.waitForTimeout(400);

// 「报告.docx」在这一行的字符下标 —— 用真函数算，不手数
const hit = findPathsInLine(LINE).find((h) => h.text === "报告.docx");
const cols = await page.evaluate(() => window.__colsOf("报告.docx"));
if (!hit) {
  fails.push("跑道自己先坏了：这行里没认出 报告.docx");
} else if (!cols || cols.lastCol == null) {
  fails.push("跑道自己先坏了：xterm 的 buffer 里找不到那段文字");
} else {
  // ★ 点**最后一格**。这一行前面有中文（全角占两格），range 若按字符下标算就会整体左移，
  //   而左移之后路径的**头几格恰好还盖得住** —— 点头部会「凑巧通过」（第一版就是这么假绿的）。
  //   点尾部才验得出这个错。
  const g = await page.evaluate(() => window.__geom());
  const x = g.left + (cols.lastCol + 0.5) * g.cell;
  const y = g.top + g.height / 2;

  await page.mouse.move(x, y);
  await page.waitForTimeout(200);
  await page.mouse.click(x, y);
  await page.waitForTimeout(200);
  const opened = await page.evaluate(() => window.__opened);
  const want = CWD + "\\报告.docx";
  if (!opened) fails.push("左键点在路径上**没反应** —— 客户说的就是这个");
  else if (opened !== want) fails.push(`左键回调拿到「${opened}」，应为「${want}」（相对路径没拼 cwd？）`);
  else console.log(`     ✓ 左键 → ${opened}`);

  await page.mouse.move(x, y);
  await page.waitForTimeout(150);
  await page.mouse.click(x, y, { button: "right" });
  await page.waitForTimeout(200);
  const menu = await page.evaluate(() => window.__menu);
  if (!menu) fails.push("右键路径没出菜单 —— 「打开方式 / 复制路径」够不着");
  else if (menu.path !== want) fails.push(`右键菜单指向「${menu.path}」，应为「${want}」`);
  else console.log(`     ✓ 右键 → 菜单指向 ${menu.path}`);

  // ★ 网址：客户 2026-08-16 截图里那个动作。以前点它会掉进 onOpen 去**读文件**，
  //   报「读取失败：系统找不到指定的路径 (os error 3)」。这里同时验两件事：
  //   ① onOpenUrl 拿到完整网址；② onOpen 一个字都没收到（分流没漏）。
  const ucols = await page.evaluate((u) => window.__colsOf(u), PROBE_URL);
  if (!ucols || ucols.lastCol == null) {
    fails.push("跑道自己先坏了：xterm buffer 里找不到那个网址");
  } else {
    await page.evaluate(() => { window.__opened = null; window.__menu = null; });
    const ux = g.left + (ucols.lastCol + 0.5) * g.cell; // 同样点**最后一格**，验列号没算歪
    await page.mouse.move(ux, y);
    await page.waitForTimeout(200);
    await page.mouse.click(ux, y);
    await page.waitForTimeout(200);
    const openedUrl = await page.evaluate(() => window.__openedUrl);
    const leaked = await page.evaluate(() => window.__opened);
    if (!openedUrl) fails.push("左键点网址**没反应** —— 客户说的「网址没法打开」");
    else if (openedUrl !== PROBE_URL) fails.push(`网址回调拿到「${openedUrl}」，应为「${PROBE_URL}」`);
    else console.log(`     ✓ 左键网址 → ${openedUrl}`);
    if (leaked) fails.push(`网址漏进了文件预览：onOpen 收到「${leaked}」（这就是 os error 3 的来路）`);

    await page.mouse.move(ux, y);
    await page.waitForTimeout(150);
    await page.mouse.click(ux, y, { button: "right" });
    await page.waitForTimeout(200);
    const urlMenu = await page.evaluate(() => window.__menu);
    if (urlMenu) fails.push(`右键网址弹出了文件菜单，指向「${urlMenu.path}」——「打开方式」对网址没意义，且路径是上一个文件的`);
    else console.log("     ✓ 右键网址 → 不出文件菜单");
  }
}

// —— 折行：整条路径必须被拼回来，而且每一行都问得出它 ——
{
  const WANT = WRAP_WANT;
  for (const row of [1, 2, 3]) {
    const links = await page.evaluate((r) => window.__wrapLinks(r), row);
    if (!links) { fails.push('折行跑道：provider 没抓到'); break; }
    const texts = links.map((l) => l.text);
    if (!texts.includes(WANT)) fails.push(`折行第 ${row} 行没问出整条路径：${JSON.stringify(texts)}`);
    // 碎片假链接：只认到 `-taskpack.zip` 这种尾巴，点了必然打不开
    for (const t of texts) if (t !== WANT && WANT.endsWith(t)) fails.push(`折行第 ${row} 行认出了碎片「${t}」`);
  }
  if (!fails.length) console.log('     ✓ 折行长路径 → 三行都指向同一条完整路径，无碎片');
}

await browser.close();
if (errors.length) console.log("（页面错误：" + errors.slice(0, 3).join(" | ") + "）");
if (fails.length) {
  console.error("\n❌ " + fails.length + " 条不达标：");
  for (const f of fails) console.error("  - " + f);
  process.exit(1);
}
console.log("\n✅ 终端里的文件路径：认得出、点得中、右键有菜单");
