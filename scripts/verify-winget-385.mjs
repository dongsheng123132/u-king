/**
 * 校验 #385 补丁：把补过的 PowerShell 片段**真跑一遍**，确认
 * ① 语法正确、② 在本机 winget 版本下选对了分支、③ 不会多出空参数。
 * 只跑版本探测那一段，**不真的装任何东西**。
 */
import fs from "node:fs";
import { spawnSync } from "node:child_process";

const json = JSON.parse(fs.readFileSync("src-tauri/skills/install-windows.json", "utf8"));
let cmd = null;
const walk = (o) => {
  if (Array.isArray(o)) return o.forEach(walk);
  if (o && typeof o === "object") {
    for (const k of Object.keys(o)) {
      if (typeof o[k] === "string" && o[k].includes("winget @wa")) cmd = o[k];
      else walk(o[k]);
    }
  }
};
walk(json);
if (!cmd) {
  console.error("找不到补过的 winget 步骤");
  process.exit(1);
}

// 抽出「建数组 + 探版本」那一段，把真正的 `winget @wa` 换成打印，避免真装。
const start = cmd.indexOf("$wa=@(");
const end = cmd.indexOf("winget @wa;");
if (start < 0 || end < 0) {
  console.error("片段边界找不到");
  process.exit(1);
}
const frag = cmd.slice(start, end) + "Write-Host ('ARGS: ' + ($wa -join ' '))";

console.log("--- 待验证片段 ---");
console.log(frag);
console.log("--- 实跑结果 ---");
const r = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", frag], {
  encoding: "utf8",
  windowsHide: true,
  timeout: 120000,
});
console.log("exit:", r.status);
console.log("stdout:", (r.stdout || "").trim());
if (r.stderr && r.stderr.trim()) console.log("stderr:", r.stderr.trim());

const out = (r.stdout || "").trim();
const wingetVer = spawnSync("winget", ["--version"], { encoding: "utf8", windowsHide: true, timeout: 60000 });
console.log("本机 winget --version:", (wingetVer.stdout || "").trim() || "(不可用)");

if (r.status !== 0) process.exit(1);
if (!out.includes("ARGS: install --id 9PLM9XGG6VKS")) {
  console.error("参数数组没建对");
  process.exit(1);
}
// 空参数会表现为连续两个空格
if (/ {2}/.test(out)) {
  console.error("参数里出现了空参数（连续空格）");
  process.exit(1);
}
console.log("✓ 片段语法正确、参数数组正常、无空参数");

// ── 反向用例：**客户机那台老 winget 上必须不加这个开关** ──────────────────
// #385 的现场就是 v1.2.10691。本机是新版，只验新版分支等于没验到出事的那一半。
// 把版本探测替换成客户机的字面版本，跑同一段判断逻辑。
for (const [ver, shouldHaveFlag] of [["v1.2.10691", false], ["v1.4.10173", true], ["(garbage)", false]]) {
  const probe = frag.replace("(winget --version)", `('${ver}')`);
  const rr = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", probe], {
    encoding: "utf8",
    windowsHide: true,
    timeout: 60000,
  });
  const got = (rr.stdout || "").includes("--disable-interactivity");
  const verdict = got === shouldHaveFlag ? "✓" : "✗";
  console.log(`${verdict} winget ${ver} → ${got ? "加" : "不加"} --disable-interactivity（应${shouldHaveFlag ? "加" : "不加"}）`);
  if (got !== shouldHaveFlag) process.exit(1);
}
console.log("✓ 版本分支全部正确（老 winget 不再收到它不认识的开关）");
