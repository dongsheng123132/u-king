/**
 * 闸门：装机清单的两份副本必须一致，且热下发那条路必须真的能覆盖。
 *
 * ## 为什么要有它（2026-08-20 实测，靠肉眼才发现的）
 * 清单有**两份**，是设计如此：
 *   · `src-tauri/skills/install-windows.json` —— `include_str!` **编进 exe** 的兜底
 *   · `website/skills/install-windows.json`   —— 部署到服务器，**热下发**
 * 覆盖规则（`installer.rs`）：**线上的 version 更大才覆盖内嵌的**。
 *
 * 🔴 当时实际状态：内嵌 **v43**、线上 **v42** —— 于是**热下发这条路整个失效**，
 *    线上那份永远赢不了。而 CLAUDE.md 的策略明写着「优先改 skill 清单热下发（不发版）」，
 *    也就是说：**我们以为有一条不用发版的快速通道，实际上它是断的。**
 *    ★ 跟「bug 采集反代到已冻结的 Vercel、改了 4 次 63 天没上线」同一个形状：
 *      管道自己坏了，而每一次改动都看起来成功了。
 *
 * ## 判据（两条，都可证伪）
 *   ① 两份**逐字节相同** —— 除了 version 允许线上 ≥ 内嵌，其余不许有差异。
 *      不同就说明有人只改了一边，下次构建/部署必有一份被悄悄回退。
 *   ② 线上那份的 version **≥** 内嵌那份 —— 否则热下发永远不生效。
 *      （本闸只比仓库里的两份；线上真值要发版后从裸网核，见 deploy.sh。）
 *
 * 用法：node scripts/check-skill-manifest-sync.mjs
 */
import { readFileSync, existsSync, readdirSync } from "node:fs";

const EMBEDDED = "src-tauri/skills/install-windows.json";
const HOSTED = "website/skills/install-windows.json";

for (const f of [EMBEDDED, HOSTED]) {
  if (!existsSync(f)) {
    console.error(`❌ 找不到 ${f}`);
    process.exit(1);
  }
}

const a = readFileSync(EMBEDDED, "utf8");
const b = readFileSync(HOSTED, "utf8");
const va = JSON.parse(a).version;
const vb = JSON.parse(b).version;

console.log(`内嵌（编进 exe）: v${va}`);
console.log(`热下发（部署到服务器）: v${vb}`);

let bad = 0;

// 🔴 AIGC 客户机实际拿的是 skillpack.rs 里的 include_str! 表，不是仓库的源目录。
// 新增 gen-*.mjs 却忘记进表时，开发机脚本仍能跑、客户机调用必 ENOENT（gen-bgm 曾真实漏过）。
// 这条进 pnpm build，直接比较源目录与 AIGC include 清单，不让遗漏等到客户机才暴露。
const skillpack = readFileSync("src-tauri/src/skillpack.rs", "utf8");
const generatorFiles = readdirSync("src-tauri/skills/aigc/scripts")
  .filter((name) => /^gen-.*\.mjs$/.test(name))
  .map((name) => `scripts/${name}`)
  .sort();
const includedGenerators = [...skillpack.matchAll(/\("(scripts\/gen-[^"]+\.mjs)",\s*include_str!\("\.\.\/skills\/aigc\/scripts\/[^"\n]+"\)\)/g)]
  .map((match) => match[1])
  .sort();
if (JSON.stringify(generatorFiles) !== JSON.stringify(includedGenerators)) {
  console.error(`\n❌ AIGC 的 gen-*.mjs 源目录与 skillpack include 清单不一致。`);
  console.error(`   源目录: ${generatorFiles.join(", ")}`);
  console.error(`   清单:   ${includedGenerators.join(", ")}`);
  bad++;
}

if (vb < va) {
  console.error(
    `\n❌ 热下发那份版本更低（${vb} < ${va}）—— 覆盖规则是「线上更大才覆盖」，` +
      `所以**热下发这条路是断的**：改了线上也不会生效。\n` +
      `   修法：把两份同步，并把 version 抬到比内嵌大（或相等后一起 +1）。`,
  );
  bad++;
}

if (a !== b) {
  const ja = JSON.parse(a);
  const jb = JSON.parse(b);
  const tools = new Set([...Object.keys(ja.tools || {}), ...Object.keys(jb.tools || {})]);
  const diff = [...tools].filter((k) => JSON.stringify(ja.tools?.[k]) !== JSON.stringify(jb.tools?.[k]));
  console.error(`\n❌ 两份不一致${diff.length ? "，这些工具的步骤不同：" + diff.join(", ") : "（差异不在 tools 里，逐字节比对自查）"}`);
  console.error(`   只改一边 = 下次构建或部署时有一份会被悄悄回退。改清单请两份一起改。`);
  bad++;
}

// DSH 的 plugin 子命令不是自包含的：它会直接 spawn 外部 `pnpm`。只装 Node/npm 与
// @deepseek-ai/dsh 时，Windows 干净机会在第一条插件命令稳定失败。正常安装与 repair
// 两条路径都必须先把固定版本 pnpm 装进 U-King 的 Node prefix，且顺序不能晚于插件命令。
const manifest = JSON.parse(a);
const pinnedDshPluginUrls = [
  "https://codeload.github.com/dongsheng123132/dsh-cache-stabilizer/tar.gz/7d7394a1421289a8e891e994d779843d93ff7893",
  "https://codeload.github.com/dongsheng123132/dsh-terminal/tar.gz/251e63689d34082bea15831b44c746e0cc50a5f5",
];
for (const bucket of ["steps", "repair"]) {
  const steps = manifest.tools?.dsh?.[bucket] || [];
  const pnpmIndex = steps.findIndex(
    (step) => step.type === "npm_install" && step.package === "pnpm@10.33.0",
  );
  const pluginIndex = steps.findIndex(
    (step) => step.type === "run" && /^dsh plugin\b/.test(step.cmd || ""),
  );
  if (pnpmIndex < 0 || pluginIndex < 0 || pnpmIndex >= pluginIndex) {
    console.error(
      `\n❌ dsh.${bucket} 必须在第一条 dsh plugin 前安装 pnpm@10.33.0；` +
        `当前 pnpmIndex=${pnpmIndex}, pluginIndex=${pluginIndex}`,
    );
    bad++;
  }
  const pluginCommands = steps
    .filter((step) => step.type === "run" && /^dsh plugin\b/.test(step.cmd || ""))
    .map((step) => step.cmd)
    .join("\n");
  if (/\b(?:github:|git\+)|\.git(?:#|\b)/.test(pluginCommands)) {
    console.error(`\n❌ dsh.${bucket} 仍含 git-hosted 插件规格；Windows 干净机没有 Git`);
    bad++;
  }
  for (const url of pinnedDshPluginUrls) {
    if (!pluginCommands.includes(url)) {
      console.error(`\n❌ dsh.${bucket} 缺少钉 SHA 的插件 tarball：${url}`);
      bad++;
    }
  }
}

if (bad) process.exit(1);
console.log("\n✅ 两份一致，且热下发能覆盖内嵌。");
