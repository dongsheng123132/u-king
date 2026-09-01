#!/usr/bin/env node
/**
 * 开发用：把 `src-tauri/skills/` 的技能包同步到本机各 AI 工具的 skills 目录。
 *
 * **产品侧的真相源是 `skillpack.rs::install_into_tools`**（跟着 exe 发货、开机自动跑）。
 * 这个脚本只是开发时的手动同步 —— 改完技能不重新构建 exe 也能立刻拿真 CLI 验。
 *
 * 🔴 为什么写它：改了 `src-tauri/skills/cad/scripts/gen-dxf.mjs` 之后直接跑评测，
 * 结果一模一样 —— 因为 pi 读的是 `~/.agents/skills/` 里那份**旧拷贝**。
 * 一份技能同时存在 5 个地方（仓库 + 4 个工具目录），不同步就是在验昨天的代码。
 *
 * 文件夹名**从 SKILL.md 的 `name:` 里读**，不在这儿手抄第二份映射表 ——
 * 抄一份就会跟 skillpack.rs 的 Pack.name 漂移。
 *
 *   node bench/uwork/sync-skills.mjs          # 同步
 *   node bench/uwork/sync-skills.mjs --check  # 只报差异，不写（有漂移退出码 1）
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const SRC = path.join(REPO, "src-tauri/skills");
const checkOnly = process.argv.includes("--check");
const HOME = os.homedir();

/** 各工具真正会扫的 skills 目录。父目录不存在 = 没装那个工具，跳过，不留垃圾目录。 */
const TARGETS = [
  { name: "U-King 自己", dir: path.join(HOME, ".uking/skills"), needParent: null },
  { name: "pi（Agent Skills 标准目录）", dir: path.join(HOME, ".agents/skills"), needParent: null },
  { name: "Claude Code", dir: path.join(HOME, ".claude/skills"), needParent: path.join(HOME, ".claude") },
  { name: "OpenClaw/ClawX", dir: path.join(HOME, ".openclaw/skills"), needParent: path.join(HOME, ".openclaw") },
];

/** 从 SKILL.md 的 YAML frontmatter 里读 `name:` —— 这就是技能的正式名，也是文件夹名。 */
function skillName(dir) {
  const md = path.join(dir, "SKILL.md");
  if (!fs.existsSync(md)) return null;
  const m = fs.readFileSync(md, "utf8").match(/^---\r?\n([\s\S]*?)\r?\n---/);
  return m ? (m[1].match(/^name:\s*(.+)$/m) || [])[1]?.trim() || null : null;
}

function walk(dir, base = dir) {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...walk(p, base));
    else out.push(path.relative(base, p));
  }
  return out;
}

// 同步哪些包，**以 skillpack.rs 的发货清单为准**，不是「skills/ 目录下有什么就同步什么」。
// 差别是真的：`1so-geo` 也躺在 skills/ 下，但它归 geo.rs 单独管、不在技能包里，
// 照目录同步就会把它塞进各工具的 skills 目录 —— 跟产品实际行为不一致，验出来的就不作数。
const RS = fs.readFileSync(path.join(REPO, "src-tauri/src/skillpack.rs"), "utf8");
const shipped = new Set([...RS.matchAll(/include_str!\("\.\.\/skills\/([^/]+)\//g)].map((m) => m[1]));
const packs = fs.readdirSync(SRC, { withFileTypes: true })
  .filter((e) => e.isDirectory() && shipped.has(e.name))
  .map((e) => ({ src: path.join(SRC, e.name), name: skillName(path.join(SRC, e.name)) }))
  .filter((p) => p.name);
if (!packs.length) { console.error("从 skillpack.rs 里一个包都没解出来 —— 清单格式变了？"); process.exit(2); }

let drift = 0, wrote = 0;
for (const t of TARGETS) {
  if (t.needParent && !fs.existsSync(t.needParent)) { console.log(`- ${t.name}：没装，跳过`); continue; }
  let changed = 0;
  for (const pk of packs) {
    const dst = path.join(t.dir, pk.name);
    for (const rel of walk(pk.src)) {
      const s = path.join(pk.src, rel), d = path.join(dst, rel);
      const same = fs.existsSync(d) && fs.readFileSync(d).equals(fs.readFileSync(s));
      if (same) continue;
      changed++; drift++;
      if (!checkOnly) {
        fs.mkdirSync(path.dirname(d), { recursive: true });
        fs.copyFileSync(s, d);
        wrote++;
      }
    }
  }
  console.log(`${changed === 0 ? "✓" : checkOnly ? "✗" : "→"} ${t.name}：${changed === 0 ? "已是最新" : `${changed} 个文件${checkOnly ? "有差异" : "已更新"}`}  (${t.dir})`);
}
console.log(`\n${packs.length} 个技能包：${packs.map((p) => p.name).join(", ")}`);
if (checkOnly && drift) { console.log(`\n🔴 有 ${drift} 处漂移 —— 现在跑评测验的是旧代码`); process.exit(1); }
if (!checkOnly) console.log(`同步完成，写了 ${wrote} 个文件`);
