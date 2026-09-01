#!/usr/bin/env node
// U-King AIGC · 文生配乐 —— 提交→轮询→下载 MiniMax Music MP3。零 npm 依赖。
// 上游提交会同步生成约 1~2 分钟，提交超时必须给 200 秒；轮询最多 10 分钟。
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, statSync, mkdtempSync, rmSync, mkdirSync, unlinkSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";

const BASE = "https://api.u-claw.org.cn";
const DEFAULT_MODEL = "minimax-music-v2.6";
const SUBMIT_TIMEOUT_MS = 200000;
const POLL_TIMEOUT_MS = 10 * 60 * 1000;
const POLL_EVERY_MS = 10000;
const BOOL = new Set(["json", "quiet"]);

function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      const k = a.slice(2);
      if (BOOL.has(k)) { out[k] = true; continue; }
      out[k] = i + 1 < argv.length && !argv[i + 1].startsWith("--") ? argv[++i] : true;
    } else out._.push(a);
  }
  return out;
}
function resolveKey(args) {
  if (args.key && args.key !== true) return String(args.key);
  if (process.env.XIAPAN_API_KEY) return process.env.XIAPAN_API_KEY;
  try { const j = JSON.parse(readFileSync(join(homedir(), ".uking", "device.json"), "utf8")); if (j?.key) return j.key; } catch {}
  return "";
}
let QUIET = false, JSONMODE = false;
function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
function done(obj, code = 0) {
  if (JSONMODE) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (obj.ok) process.stdout.write((obj.file || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) { done({ ok: false, error: String((msg && msg.message) || msg) }, code); }
function normalizeOut(p) {
  let s = String(p); const m = s.match(/^\/([A-Za-z])\/(.*)$/);
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  const abs = resolve(s); try { mkdirSync(dirname(abs), { recursive: true }); } catch {}
  return abs;
}
function curlJson(args, timeoutMs, label) {
  const r = spawnSync("curl", ["-sS", "-m", String(Math.ceil(timeoutMs / 1000)), ...args],
    { timeout: timeoutMs + 5000, encoding: "utf8", maxBuffer: 4 * 1024 * 1024 });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`${label}连接失败（curl 退出码 ${r.status}）：${String(r.stderr || "").trim().slice(0, 240)}`);
  try { return JSON.parse(String(r.stdout || "")); }
  catch { throw new Error(`${label}响应不是 JSON：${String(r.stdout || "").slice(0, 240)}`); }
}
function errOf(v) { return v?.error?.message || (v?.error ? JSON.stringify(v.error) : ""); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function downloadContent(id, key, out) {
  const r = spawnSync("curl", ["-sS", "-L", "-m", "120", "-H", `Authorization: Bearer ${key}`,
    "-o", out, "-w", "%{http_code}\n%{content_type}", `${BASE}/v1/music/generations/${encodeURIComponent(id)}/content`],
  { timeout: 125000, encoding: "utf8" });
  if (r.error) throw r.error;
  const [status = "", type = ""] = String(r.stdout || "").trim().split(/\r?\n/);
  let bytes = 0; try { bytes = statSync(out).size; } catch {}
  if (r.status !== 0 || !/^2\d\d$/.test(status) || bytes <= 1024) {
    let detail = ""; try { detail = readFileSync(out, "utf8").slice(0, 240); } catch {}
    try { unlinkSync(out); } catch {}
    throw new Error(`配乐下载失败（HTTP ${status || "?"}，${type || "未知类型"}，${bytes} 字节）：${detail}`);
  }
  return bytes;
}

async function main() {
  const args = parseArgs(process.argv.slice(2)); QUIET = !!args.quiet; JSONMODE = !!args.json;
  const prompt = typeof args.prompt === "string" ? args.prompt.trim() : "";
  if (!prompt) fail("缺少 --prompt（描述想要的配乐）", 2);
  const key = resolveKey(args); if (!key) fail("找不到 API Key（--key / XIAPAN_API_KEY / ~/.uking/device.json）", 2);
  const model = (typeof args.model === "string" && args.model) || DEFAULT_MODEL;
  const out = normalizeOut((typeof args.out === "string" && args.out) || `./uking-bgm-${Date.now()}.mp3`);
  const dir = mkdtempSync(join(tmpdir(), "uking-bgm-")); const bodyPath = join(dir, "body.json");
  const t0 = Date.now();
  try {
    const idempotencyKey = `${Date.now()}-${process.pid}`;
    writeFileSync(bodyPath, JSON.stringify({ model, prompt, idempotency_key: idempotencyKey }));
    logE(`提交配乐：${model}（上游同步生成约 1~2 分钟）…`);
    const submitted = curlJson(["-X", "POST", `${BASE}/v1/music/generations`, "-H", `Authorization: Bearer ${key}`,
      "-H", "Content-Type: application/json", "--data", `@${bodyPath}`], SUBMIT_TIMEOUT_MS, "提交配乐");
    const submitErr = errOf(submitted); if (submitErr) throw new Error("配乐生成失败：" + submitErr);
    const id = submitted?.id || submitted?.task_id;
    if (!id) throw new Error("配乐提交响应缺少 id：" + JSON.stringify(submitted).slice(0, 240));
    const deadline = Date.now() + POLL_TIMEOUT_MS;
    let state = submitted;
    while (String(state?.status || "").toLowerCase() !== "completed") {
      const status = String(state?.status || "").toLowerCase();
      const stateErr = errOf(state);
      if (stateErr || ["failed", "error", "cancelled", "canceled"].includes(status))
        throw new Error("配乐生成失败：" + (stateErr || state?.message || status));
      if (Date.now() >= deadline) throw new Error("配乐生成轮询超时（10 分钟），任务仍未完成：" + id);
      logE(`  任务 ${id}：${state?.status || "处理中"}，10 秒后查询…`);
      await sleep(POLL_EVERY_MS);
      state = curlJson([`${BASE}/v1/music/generations/${encodeURIComponent(id)}`, "-H", `Authorization: Bearer ${key}`], 30000, "查询配乐任务");
    }
    const bytes = downloadContent(id, key, out);
    done({ ok: true, file: resolve(out), id, model, bytes, elapsed: ((Date.now() - t0) / 1000).toFixed(1) + "s" });
  } catch (e) { fail(e); } finally { rmSync(dir, { recursive: true, force: true }); }
}
main();
