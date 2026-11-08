/**
 * Sync clifleet's left-rail projects into U-King's U-Workspace task list.
 *
 * Both sides descend from the same opencodex workspace module: clifleet's
 * `~/.clifleet/tasks.json` and U-King's `~/.uking/tasks.json` share the same
 * task schema, so this is a data-layer merge — no product code needs to change.
 *
 * Merge rules:
 *   - dedupe by normalized `dir` (path may differ in case / trailing slash);
 *     projects already in U-King are left untouched (their status/source/tool/
 *     startup_cmd/kind/expert/assignee/external_ref fields are preserved);
 *   - clifleet-only projects are appended with U-King defaults filled in.
 *
 * Privacy: the real task files are user data. This script NEVER prints the
 * project list, only counts. Override paths with CLIFLEET_JSON / UKING_JSON
 * to test against fixture data. Default is preview-only; pass --apply to write,
 * and a `.bak-<timestamp>` backup is always taken first.
 *
 * Run: node scripts/sync-clifleet-workspace.mjs [--apply]
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

const home = homedir();
const clifleetPath = process.env.CLIFLEET_JSON || path.join(home, ".clifleet", "tasks.json");
const ukingPath = process.env.UKING_JSON || path.join(home, ".uking", "tasks.json");
const apply = process.argv.includes("--apply");

const DEFAULTS = {
  status: "idle",
  source: "manual",
  assignee: null,
  external_ref: null,
  tool: null,
  startup_cmd: null,
  kind: "task",
  expert: null,
};

function readTasks(file) {
  if (!existsSync(file)) throw new Error(`找不到输入文件: ${file}`);
  const data = JSON.parse(readFileSync(file, "utf8"));
  if (!Array.isArray(data.tasks)) throw new Error(`${file} 里没有 tasks 数组`);
  return data;
}

const normDir = (dir) => String(dir || "").replace(/[/\\]+$/, "").toLowerCase();

function fromClifleet(task) {
  return {
    id: task.id,
    name: task.name,
    dir: task.dir,
    ...DEFAULTS,
    last_opened_at: task.last_opened_at ?? 0,
    created_at: task.created_at ?? 0,
    order: task.order ?? 0,
  };
}

const clifleet = readTasks(clifleetPath);
const uking = readTasks(ukingPath);

const existingDirs = new Set(uking.tasks.map((t) => normDir(t.dir)));
const existingIds = new Set(uking.tasks.map((t) => t.id));
const added = [];

for (const task of clifleet.tasks) {
  const key = normDir(task.dir);
  if (!key || existingDirs.has(key) || existingIds.has(task.id)) continue;
  uking.tasks.push(fromClifleet(task));
  existingDirs.add(key);
  existingIds.add(task.id);
  added.push(task.id);
}

console.log(`来源项目数: ${clifleet.tasks.length}`);
console.log(`U-King 原有: ${uking.tasks.length - added.length}`);
console.log(`本次新增并入: ${added.length}`);
console.log(`合并后总数: ${uking.tasks.length}`);

if (!apply) {
  console.log("预览模式（未写入）。确认后加 --apply 落盘。");
  process.exit(0);
}

if (added.length === 0) {
  console.log("没有需要新增的项目，跳过写入。");
  process.exit(0);
}

const stamp = new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14);
const backup = `${ukingPath}.bak-${stamp}`;
writeFileSync(backup, readFileSync(ukingPath));
writeFileSync(ukingPath, JSON.stringify(uking, null, 2) + "\n");
console.log(`已写入 ${ukingPath}`);
console.log(`备份: ${backup}`);
