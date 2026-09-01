/**
 * 验证 v40 闸门：从清单里**原样取出**守卫片段真跑一遍（不是手敲一份重写，那验的是我的记性）。
 * 两条跑道：阈值原样（本机 build 高 → 应放行）/ 阈值调到 99999（逼本机走「太旧」分支 → 应拦住）。
 * 只跑守卫本身，不跑后面的 winget / 667MB 下载。
 */
import fs from "fs";
import { execFileSync } from "child_process";

const c = JSON.parse(fs.readFileSync("website/skills/install-windows.json", "utf8")).tools["codex-app"];
const MARK = '-Command "';
const END = "exit 1 }; ";

let bad = 0;

for (const [where, step] of [["steps", c.steps[0]], ["repair", c.repair[0]]]) {
  const i = step.cmd.indexOf(MARK) + MARK.length;
  const end = step.cmd.indexOf(END) + END.length;
  const guard = step.cmd.slice(i, end);

  if (!guard.includes("19041")) throw new Error(`${where}: 守卫没注入`);
  if (/[^\x20-\x7e]/.test(guard)) throw new Error(`${where}: 守卫含非 ASCII，GBK 机器上会变乱码`);
  if (guard.includes("\\")) throw new Error(`${where}: 守卫含反斜杠，多层转义里会被吃掉`);

  for (const [name, ps, wantBlocked] of [
    ["放行（本机真实 build）", guard, false],
    ["拦截（阈值抬到 99999）", guard.replace(/19041/g, "99999"), true],
  ]) {
    // 🔴 只认 **stdout** 里的哨兵：PowerShell 的错误记录会把整条命令行原样回显到 stderr，
    // 里头就带着 `Write-Host 'GUARD-PASSED'` 这几个字 —— 拿 stdout+stderr 一起匹配，
    // 闸门明明拦住了也会被判成「放行」（第一版验证脚本就是这么自己骗自己的）。
    let stdout = "";
    let stderr = "";
    let code = 0;
    try {
      stdout = execFileSync(
        "powershell.exe",
        ["-NoProfile", "-NonInteractive", "-Command", ps + " Write-Host 'GUARD-PASSED'"],
        { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
      );
    } catch (e) {
      code = e.status ?? -1;
      stdout = e.stdout || "";
      stderr = e.stderr || "";
    }
    const out = stdout + stderr;
    const passed = stdout.includes("GUARD-PASSED");
    const blocked = !passed && code === 1;
    const ok = wantBlocked ? blocked : passed;
    if (!ok) bad++;
    console.log(`${ok ? "✓" : "✗"} ${where} / ${name}: exit=${code} ${blocked ? "已拦截" : passed ? "已放行" : "结果不明"}`);
    if (wantBlocked && blocked) {
      // installer.rs 只留 stderr 末尾 300 字符 —— 按同样口径断言那句人话真的**留在里面**，
      // 不然客户上报上来又是一条「看不出为什么失败」的 issue。
      const tail = stderr.trim().slice(-300);
      if (!/please use Codex CLI instead/.test(tail)) {
        bad++;
        console.log("    ✗ 末尾 300 字符里没有那句人话，上报会退化成噪音");
      }
      console.log(`    客户/上报看到的（stderr 末尾）: ${tail.replace(/\s+/g, " ").slice(0, 110)}`);
    }
  }
}

console.log(bad === 0 ? "\n全部通过" : `\n${bad} 条未通过`);
process.exit(bad === 0 ? 0 : 1);
