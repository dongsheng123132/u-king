#!/usr/bin/env node
/**
 * open-url.mjs —— 用系统默认浏览器打开网址 / 用默认程序打开本地文件（预览产物）。
 *
 * 为什么单独一个脚本而不是让 AI 直接敲 `start`：
 *   - Windows 的 `start` 是 cmd 内建命令，直接 spawn 会失败；且第一个参数会被当窗口标题吃掉
 *     （`start "https://x"` 只开一个空窗口）—— 这是模型反复踩的坑，封一层就不用它记。
 *   - 三个平台三条命令（start / open / xdg-open），封一层跨平台。
 *   - 顺手拦掉明显危险的协议（file:// 之外的本地执行、javascript: 等）。
 *
 * 用法：
 *   node open-url.mjs https://u-king.org --json
 *   node open-url.mjs "C:/…/图纸.预览.svg" --json      # 打开本地产物给客户看
 */
import { argv, exit, platform } from "node:process";
import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const args = argv.slice(2);
const asJson = args.includes("--json");
const target = args.find((a) => !a.startsWith("--"));
function out(o) { if (asJson) console.log(JSON.stringify(o)); else console.log(o.ok ? o.opened : "失败: " + o.error); exit(o.ok ? 0 : 1); }
if (!target) out({ ok: false, error: "用法: node open-url.mjs <网址或本地文件路径> [--json]" });

let opened = target;
if (/^https?:\/\//i.test(target)) {
  // 网址：原样打开
} else if (/^[a-z][a-z0-9+.-]*:/i.test(target) && !/^file:/i.test(target)) {
  out({ ok: false, error: `不打开 ${target.split(":")[0]}: 协议 —— 只支持 http/https 和本地文件路径` });
} else {
  const p = path.resolve(target.replace(/^file:\/\//i, ""));
  if (!fs.existsSync(p)) out({ ok: false, error: `文件不存在: ${p}` });
  opened = p;
}

const done = (err) => out(err ? { ok: false, error: String(err.message || err) } : { ok: true, opened });
if (platform === "win32") execFile("cmd", ["/c", "start", "", opened], done);
else if (platform === "darwin") execFile("open", [opened], done);
else execFile("xdg-open", [opened], done);
