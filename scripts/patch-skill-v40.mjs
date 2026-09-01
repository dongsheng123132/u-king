/**
 * 一次性清单补丁：codex-app 加 Windows 版本闸门（skill v39 → v40）。
 *
 * 背景（issue #357，同型号 #339 #335 #334 #328）：Codex 桌面版是 MSIX，MinVersion 写死
 * 10.0.19041.0。Windows 10 1809（build 17763，LTSC/政企机器常见）上微软商店和离线 MSIX
 * 两条路都必然失败，而我们的失败流是「步骤失败 → 自动 repair」，客户要先等一个 667MB
 * 的下载跑完，才看到一句 GBK 乱码的报错。
 *
 * 新客户端靠 `min_windows_build`（installer.rs 在 steps 之前判定，不进 repair）。
 * 老客户端（≤0.9.92）不认识那个字段，所以命令自己也带一份闸门，steps 和 repair 两头都拦。
 *
 * 🔴 两条硬约束：
 *  ① 提示文案必须**纯 ASCII** —— 子进程输出在中文 Windows 上是 GBK，我们按 UTF-8 lossy 解，
 *    中文会变成 `����`（#357 日志里那一片乱码就是这么来的）。
 *  ② 命令里**不带反斜杠** —— 用 [Environment]::OSVersion 而不是读注册表路径。
 *    第一版用了 'HKLM:\SOFTWARE\...'，在 JSON/shell/cmd 几层转义里反斜杠被吃掉，
 *    变成 'HKLM:SOFTWAREMicrosoft...' → Get-ItemProperty 报路径不存在 → **闸门静默失效**
 *    （试跑时打出 "BUG: guard did not fire" 才发现）。少一层转义就少一处能坏的地方。
 *
 * 另：报错用 [Console]::Error.WriteLine 而不是 Write-Error。Write-Error 吐的是 PowerShell
 * 错误记录 —— 把整条命令行连同 CategoryInfo / FullyQualifiedErrorId 一起打出来，而
 * installer.rs 只留 stderr 的**末尾 300 字符**，那句人话会被后面的噪音挤出去，上报上来又是
 * 一条「看不出为什么失败」的 issue。一行干净 stderr，客户和 triage 都直接看到原因。
 */
import fs from "fs";

const GUARD =
  "$b=0; try { $b=[int][Environment]::OSVersion.Version.Build } catch {}; " +
  "if ($b -gt 0 -and $b -lt 19041) { [Console]::Error.WriteLine('Codex desktop app requires Windows 10 build 19041 or newer; " +
  "this PC is build ' + $b + '. It cannot be installed on this system - please use Codex CLI instead " +
  "(same features, no OS version limit).'); exit 1 }; ";

const MARK = '-Command "';
const FILES = ["website/skills/install-windows.json", "src-tauri/skills/install-windows.json"];

for (const f of FILES) {
  const j = JSON.parse(fs.readFileSync(f, "utf8"));
  const c = j.tools["codex-app"];

  c.min_windows_build = 19041;

  for (const list of [c.steps, c.repair]) {
    const s = list[0];
    if (s.cmd.includes("19041")) continue; // 幂等：重跑不叠加
    const i = s.cmd.indexOf(MARK);
    if (i < 0) throw new Error(`找不到 -Command 插入点：${f}`);
    const at = i + MARK.length;
    s.cmd = s.cmd.slice(0, at) + GUARD + s.cmd.slice(at);
  }

  j.version = 40;
  j.updated = "2026-08-08";
  fs.writeFileSync(f, JSON.stringify(j, null, 2) + "\n");
  console.log(`patched ${f} -> v${j.version}`);
}
