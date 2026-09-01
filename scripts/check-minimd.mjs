#!/usr/bin/env node
/**
 * MiniMd 渲染跑道 —— 真渲染 + 真断言，不联网、不烧 token、不起 GUI。
 *
 * ## 为什么要有这条
 * `src/lib/miniMd.tsx` 是 U-Workspace 对话区**唯一**的 markdown 渲染器，客户每一句回话都经过它。
 * 而它的正确性一个字节都不在 Rust 单测里，`cargo check` / `pnpm build` 只能证明它**能编译**。
 * Issue #379 / #380（客户 0.9.95 实报「表格无法显示 / 排版很糟糕」）就是编译全绿、功能没有。
 *
 * ## 为什么不是截图
 * 截图要起 dev server + Chromium，还得人去看。这里只需要「结构对不对」，
 * `renderToStaticMarkup` 出 HTML 串直接断言即可 —— 毫秒级、确定性、能变红。
 * （像素好不好看归 `shot-lowres.mjs` 管，两回事。）
 *
 * ## 跑法
 *   node scripts/check-minimd.mjs
 * 退出码 0=全绿 / 1=有断言没过。
 */
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import { transformWithEsbuild } from "vite";
import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";

const SRC = "src/lib/miniMd.tsx";

// tsx → js。走 vite 转出来的 esbuild（vite 是直接 devDependency，pnpm 严格模式下
// 直接 `import "esbuild"` 解析不到 —— 它没被提升到顶层 node_modules）。不给仓库加新依赖。
const out = await transformWithEsbuild(readFileSync(SRC, "utf8"), SRC, {
  loader: "tsx",
  format: "esm",
  jsx: "automatic",
});
const dir = mkdtempSync(join(tmpdir(), "minimd-"));
const file = join(dir, "miniMd.mjs");
writeFileSync(file, out.code);
const { MiniMd } = await import(pathToFileURL(file).href);

const html = (text) => renderToStaticMarkup(createElement(MiniMd, { text }));

let failed = 0;
const check = (name, cond, detail = "") => {
  if (cond) {
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.log(`  ✗ ${name}${detail ? "\n      " + detail : ""}`);
  }
};

console.log("\n── 表格（Issue #379 / #380）──");
{
  const h = html("| 工具 | 状态 |\n|---|---|\n| Claude | 已装 |\n| Codex | 未装 |");
  check("生成了 <table>", h.includes("<table"), h.slice(0, 200));
  check("表头进 <th>", h.includes("<th") && h.includes("工具") && h.includes("状态"));
  check("数据进 <td>", h.includes("<td") && h.includes("Claude") && h.includes("已装"));
  check("两行数据都在", (h.match(/<tr/g) || []).length === 3, `实际 <tr> 数=${(h.match(/<tr/g) || []).length}`);
  check("没有裸竖线漏到正文", !h.replace(/<[^>]+>/g, "").includes("|"));
  check("外层可横向滚（宽表不撑破布局）", h.includes("overflow-x-auto"));
}

console.log("\n── 对齐 ──");
{
  const h = html("| 左 | 中 | 右 |\n|:---|:--:|---:|\n| a | b | c |");
  check("左对齐", h.includes("text-left"));
  check("居中", h.includes("text-center"));
  check("右对齐", h.includes("text-right"));
}

console.log("\n── 不该被当成表格的（回归护栏）──");
{
  const h = html("你可以用 A | B 二选一，随便挑。");
  check("只有竖线、没有分隔行 → 仍是段落", !h.includes("<table"), h.slice(0, 160));
  check("原文没被吃掉", h.includes("二选一"));
}
{
  // 🔴 这条是变异验证补出来的：上面那个单行用例**测不到 `isTableSep` 护栏** ——
  // 它只有一行，下一行是空串，列数天然对不上，被内层的「列数必须相等」兜住了。
  // 把 isTableSep 从条件里删掉，上面那条照样绿 = 护栏没被测到。
  // 真正的风险形状是**连续两行竖线数相同、但第二行不是分隔行**：
  const h = html("姓名 | 年龄\n张三 | 30");
  check("连续两行竖线数相同、但没有分隔行 → 仍是段落", !h.includes("<table"), h.slice(0, 200));
  check("两行原文都还在", h.includes("姓名") && h.includes("张三"));
}
{
  const h = html("---");
  check("单独的 --- 仍是分隔线不是表格", h.includes("<hr") && !h.includes("<table"));
}
{
  // 分隔行列数和表头对不上 → 不认（多半不是表格）
  const h = html("| a | b | c |\n|---|---|\n| 1 | 2 | 3 |");
  check("分隔行列数对不上 → 不当表格", !h.includes("<table"));
}

console.log("\n── 单元格里的行内标记 ──");
{
  const h = html("| 名 | 说明 |\n|---|---|\n| **粗** | `代码` |");
  check("单元格里的 **粗体** 生效", h.includes("<strong"));
  check("单元格里的 `代码` 生效", h.includes("<code"));
}

console.log("\n── 转义竖线 ──");
{
  const h = html("| 表达式 | 值 |\n|---|---|\n| a \\| b | 或 |");
  check("`\\|` 当内容不当分隔符", h.includes("a | b"), h.slice(0, 300));
}

console.log("\n── 缺格的行（模型常少写一格）──");
{
  const h = html("| a | b | c |\n|---|---|---|\n| 1 | 2 |");
  const tds = (h.match(/<td/g) || []).length;
  check("按表头补齐成 3 格，不整行错位", tds === 3, `实际 <td> 数=${tds}`);
}

console.log("\n── 原有能力没被表格改动碰坏 ──");
{
  check("## 标题仍渲染（那两条 issue 说它坏了，其实一直是好的）", html("## 标题").includes("text-[1.1em]"));
  check("**粗体**", html("**粗**").includes("<strong"));
  check("- 列表", html("- a\n- b").includes("<ul"));
  check("1. 有序列表", html("1. a\n2. b").includes("<ol"));
  check("> 引用", html("> q").includes("border-l-2"));
  check("``` 代码块", html("```\nx=1\n```").includes("<pre"));
  const h = html("| a | b |\n|---|---|\n| 1 | 2 |\n\n表格后面的正文");
  check("表格后面的正文没被吃掉", h.includes("表格后面的正文"));
}

console.log("\n── 安全：绝不透传 HTML ──");
{
  const h = html("| x |\n|---|\n| <script>alert(1)</script> |");
  check("单元格里的 <script> 被转义，不进 DOM", !h.includes("<script>"), h.slice(0, 300));
  check("原文仍看得见（转义成实体，不是吞掉）", h.includes("&lt;script&gt;"));
}

rmSync(dir, { recursive: true, force: true });

console.log(failed ? `\n❌ ${failed} 条断言没过\n` : "\n✅ 全部通过\n");
process.exit(failed ? 1 : 0);
