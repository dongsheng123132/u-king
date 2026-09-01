/**
 * 构建期断言：组件里不许再写 `text-white/xx`。
 *
 * ## 为什么值得拦在构建期
 * App **默认是浅色主题**（`src/App.tsx` 里 `uking.theme` 缺省落 light），
 * 浅色下面板底色 `bg-bg-2` 是**纯白**。写 `text-white/60` 就是白底白字。
 * v0.9.99 的「这个文件夹是空的，要布置一下吗？」弹窗（`useWorkbenchOffer.tsx`）
 * 一个文件里写了 12 处，结果客户看到的是：路径没了、简介没了、「会建这些目录」没了、
 * 「它没有什么」整段没了，连「先空着」那个按钮都只剩一个**空白框**。
 *
 * 这类 bug 三道防线全漏：
 *  - tsc 不管 className 里的字符串；
 *  - `scripts/check-workbench-offer.mjs` 用 `innerText` 断言 —— 白底白字它照样过；
 *  - 开发的人自己开着深色主题，肉眼也看不出来。
 * 所以只能在构建期按字符串拦。房子规矩是 `text-ink-*`（全仓 1600+ 处）。
 *
 * ## 白名单不是「这个文件豁免」，是「这几行有正当理由」
 * 白字压在**自己那块写死的深色底**上（accent 气泡、bg-black/60 遮罩、bg-neutral-900 浮层）
 * 跟主题无关，是对的写法。这些地方的容器同时挂了 `data-on-dark`，
 * `globals.css` 的浅色兜底会跳过它们 —— 两处必须同时在，改一处忘另一处就会出事。
 * 所以白名单精确到 **文件 + 行内容**，不写整目录豁免：整目录豁免等于把这条规矩关掉。
 *
 * 用法：`node scripts/check-theme-tokens.mjs`（`pnpm build` 里会先跑一遍）
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";

const SRC = path.resolve("src");
const NEEDLE = /text-white\/[0-9[]/;

/**
 * 第二条规矩：不许写 `text-amber-100/200/300`（可带 /xx 透明度）。
 *
 * 🔴 跟白底白字是**同一个病**，但上面那条正则一个都扫不到 —— 闸门自己有盲区。
 * 客户 2026-08-17 报「黄字体跟绿的混一起都看不到」：那行是 `text-amber-300/80`
 * （#fcd34d @80%）压在「使用中」绿卡上，浅色主题下实算对比度 **1.24:1**（AA 要 4.5）。
 * amber-100~300 整档都是照**深色底**调的，浅色主题（App 默认！）下必糊。
 * 全仓当时有 20 处，全部踩在同一块石头上。
 *
 * 房子规矩：`text-warning-700 dark:text-warning-400`（琥珀）/
 *          `text-danger-700 dark:text-danger-400`（红，留给「会花钱 / 不可逆」）。
 * 700 档是 2026-08-17 为浅色底补的，见 tailwind.config.js。
 *
 * 没有白名单：这一类没有「压在写死深色底上所以是对的」的正当例外
 * —— 真有写死深色底，那也该用 `dark:` 变体或 data-on-dark，而不是硬写深色专用色阶。
 */
const DARK_ONLY_AMBER = /text-amber-(100|200|300)(\/[0-9[]|\b)/;

/**
 * 允许保留的 `text-white/xx`。key = 相对 src 的路径，value = 该文件允许的条数 + 理由。
 * 🔴 加条目之前先问：这块底色是**写死的深色**，还是**跟着主题变的**？
 *    只有前者才该进白名单，且容器必须挂 `data-on-dark`。
 */
const ALLOW = {
  "Draw.tsx": { count: 1, why: "模型名压在 bg-accent/90 的提示词气泡里（容器已挂 data-on-dark）" },
  "Video.tsx": { count: 1, why: "同上，视频页的提示词气泡" },
  "components/ShareCard.tsx": { count: 1, why: "海报预览浮层 bg-black/70 上的说明文字（容器已挂 data-on-dark）" },
  "opencodex/panels/BrowserPanel.tsx": { count: 1, why: "「操作中…」压在 bg-black/20 遮罩上（容器已挂 data-on-dark）" },
};

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = path.join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (/\.tsx$/.test(name)) out.push(p);
  }
  return out;
}

const hits = new Map(); // rel -> [{line, text}]
const amberHits = []; // {rel, line, text}
for (const file of walk(SRC)) {
  const rel = path.relative(SRC, file).replace(/\\/g, "/");
  readFileSync(file, "utf8")
    .split(/\r?\n/)
    .forEach((text, i) => {
      if (NEEDLE.test(text)) {
        if (!hits.has(rel)) hits.set(rel, []);
        hits.get(rel).push({ line: i + 1, text: text.trim() });
      }
      if (DARK_ONLY_AMBER.test(text)) amberHits.push({ rel, line: i + 1, text: text.trim() });
    });
}

if (amberHits.length) {
  console.error("❌ 发现 text-amber-100/200/300 —— 深色底专用色阶，浅色主题（App 默认）下糊成一团\n");
  for (const h of amberHits) console.error(`   ${h.rel}:${h.line}  ${h.text.slice(0, 110)}`);
  console.error("\n   改用 text-warning-700 dark:text-warning-400（提示）");
  console.error("   或 text-danger-700 dark:text-danger-400（「会花钱 / 不可逆」才用红）。");
  process.exit(1);
}

const bad = [];
for (const [rel, lines] of hits) {
  const allowed = ALLOW[rel];
  if (!allowed) {
    bad.push({ rel, lines, reason: "不在白名单里" });
  } else if (lines.length > allowed.count) {
    // 只放行「原本就有的那几条」。新增的必须自己说清理由才能进白名单，
    // 否则白名单会慢慢变成一张「这个文件随便写」的通行证。
    bad.push({ rel, lines, reason: `白名单允许 ${allowed.count} 条，现在有 ${lines.length} 条` });
  }
}

if (bad.length) {
  console.error("❌ 发现 text-white/xx —— 浅色主题下这是白底白字（App 默认就是浅色）\n");
  for (const b of bad) {
    console.error(`   ${b.rel}（${b.reason}）`);
    for (const l of b.lines) console.error(`     :${l.line}  ${l.text.slice(0, 110)}`);
    console.error("");
  }
  console.error("   改用 text-ink-*：ink-1 正文 / ink-2 次要 / ink-3 提示（浅深两套主题都过 3:1）。");
  console.error("   若这块底色**确实是写死的深色**：给容器加 data-on-dark，并在本脚本 ALLOW 里写清理由。");
  process.exit(1);
}

const kept = [...hits].reduce((n, [, l]) => n + l.length, 0);
console.log(`✅ 主题令牌检查通过（${kept} 处 text-white/xx 全部在白名单内且压在 data-on-dark 底上）`);
