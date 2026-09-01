#!/usr/bin/env node
// U-King AIGC · 批量 / 多进程出图出片 —— 把一批 job 并发交给 gen-image.mjs / gen-video.mjs
// 各自独立子进程（真·多进程），带并发上限。适合「一组提示词批量配图」「多镜头并发出视频」。
// 零 npm 依赖。用法见同目录 ../SKILL.md。--json 出 {ok,total,succeeded,failed,results}；
// 退出码 0 = 全部成功 / 1 = 有失败或运行错 / 2 = 参数错。
import { spawn } from "node:child_process";
import { readFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));

// ── 参数解析（--flag value；--json/--quiet 布尔；--prompt 可重复成数组）──
const BOOL = new Set(["json", "quiet"]);
const REPEAT = new Set(["prompt"]);
function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      const k = a.slice(2);
      if (BOOL.has(k)) { out[k] = true; continue; }
      const v = i + 1 < argv.length && !argv[i + 1].startsWith("--") ? argv[++i] : true;
      if (REPEAT.has(k)) (out[k] ??= []).push(v);
      else out[k] = v;
    } else out._.push(a);
  }
  return out;
}

let QUIET = false, JSONMODE = false;
function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
function done(obj, code = 0) {
  if (JSONMODE) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (obj.ok) process.stdout.write(`全部成功 ${obj.succeeded}/${obj.total}\n`);
  else process.stderr.write(`完成：成功 ${obj.succeeded || 0}/${obj.total || 0}，失败 ${obj.failed || 0}\n`);
  process.exit(code);
}
function fail(msg, code = 2) {
  if (JSONMODE) process.stdout.write(JSON.stringify({ ok: false, error: String((msg && msg.message) || msg) }) + "\n");
  else process.stderr.write("错误：" + String((msg && msg.message) || msg) + "\n");
  process.exit(code);
}

// 规整一条 job：{type:image|video, prompt, model?, size?, duration?, resolution?, out?, ref?/image?, key?, retries?}
function normJob(j, idx, outdir, fallbackKey) {
  const type = j.type === "video" ? "video" : "image";
  const prompt = typeof j.prompt === "string" ? j.prompt : "";
  const ext = type === "video" ? "mp4" : "png";
  const out = j.out ? resolve(j.out) : join(outdir, `${String(idx + 1).padStart(2, "0")}-${type}.${ext}`);
  // 参考图/首帧图：image job 用 --ref（可多张），video job 用 --image（首帧，取第一张）
  const imgs = []
    .concat(j.ref ?? [], j.image ?? [])
    .filter((x) => typeof x === "string" && x);
  return {
    type, prompt, model: j.model, size: j.size, n: j.n, quality: j.quality, duration: j.duration, resolution: j.resolution,
    out, imgs, key: j.key ?? fallbackKey, retries: j.retries,
  };
}

// 跑一条 job = spawn 一个子进程（gen-image.mjs / gen-video.mjs），收 stdout 最后一行 JSON。
function runJob(job, idx) {
  return new Promise((res) => {
    if (!job.prompt) return res({ idx, ok: false, type: job.type, out: job.out, error: "缺少 prompt" });
    const script = job.type === "video" ? "gen-video.mjs" : "gen-image.mjs";
    const args = [join(HERE, script), "--json", "--quiet", "--prompt", job.prompt, "--out", job.out];
    if (job.model) args.push("--model", String(job.model));
    if (job.size && job.type === "image") args.push("--size", String(job.size));
    if (job.quality && job.type === "image") args.push("--quality", String(job.quality));
    if (job.n != null && job.type === "image") args.push("--n", String(job.n));
    if (job.duration != null && job.type === "video") args.push("--duration", String(job.duration));
    if (job.resolution && job.type === "video") args.push("--resolution", String(job.resolution));
    if (job.key) args.push("--key", String(job.key));
    if (job.retries != null && job.type === "video") args.push("--retries", String(job.retries));
    if (job.type === "video") { if (job.imgs[0]) args.push("--image", job.imgs[0]); }
    else for (const r of job.imgs) args.push("--ref", r);

    logE(`[${idx + 1}] ${job.type} 启动：${job.prompt.slice(0, 40)}…`);
    const child = spawn(process.execPath, args, { windowsHide: true });
    let so = "", se = "";
    child.stdout.on("data", (d) => (so += d));
    child.stderr.on("data", (d) => (se += d));
    child.on("error", (e) => res({ idx, ok: false, type: job.type, out: job.out, error: "子进程启动失败：" + e.message }));
    child.on("close", (code) => {
      const line = so.trim().split(/\r?\n/).filter(Boolean).pop() || "";
      let r = null; try { r = JSON.parse(line); } catch {}
      if (r && r.ok) { logE(`[${idx + 1}] ✓ ${r.file}`); res({ idx, ok: true, type: job.type, file: r.file, model: r.model, elapsed: r.elapsed }); }
      else { const err = (r && r.error) || se.trim().split(/\r?\n/).pop() || `子进程退出码 ${code}`; logE(`[${idx + 1}] ✗ ${err}`); res({ idx, ok: false, type: job.type, out: job.out, error: err }); }
    });
  });
}

// 并发池：最多 k 个同时在跑，其余排队。保持结果顺序。
async function pool(items, k, fn) {
  const results = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (true) {
      const i = next++;
      if (i >= items.length) return;
      results[i] = await fn(items[i], i);
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(k, items.length)) }, worker));
  return results;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;

  const concurrency = Math.max(1, parseInt(args.concurrency, 10) || 3);
  const outdir = resolve(typeof args.outdir === "string" ? args.outdir : `./uking-batch-${Date.now()}`);
  try { mkdirSync(outdir, { recursive: true }); } catch {}

  // 两种喂法：① --jobs jobs.json（每条完整 job）；② --prompt 重复 + --type/--model（同款批量）
  let rawJobs = [];
  if (typeof args.jobs === "string") {
    let parsed; try { parsed = JSON.parse(readFileSync(args.jobs, "utf8")); }
    catch (e) { fail("读取 --jobs 文件失败或不是合法 JSON：" + e.message); }
    rawJobs = Array.isArray(parsed) ? parsed : Array.isArray(parsed.jobs) ? parsed.jobs : [];
  } else if (args.prompt) {
    const prompts = Array.isArray(args.prompt) ? args.prompt : [args.prompt];
    const type = args.type === "video" ? "video" : "image";
    rawJobs = prompts.map((p) => ({
      type, prompt: p, model: args.model, size: args.size, n: args.n, quality: args.quality, duration: args.duration, resolution: args.resolution,
      image: args.image, ref: args.ref,
    }));
  }
  if (!rawJobs.length) fail("没有任务。用 --jobs jobs.json，或 --prompt \"...\" --prompt \"...\"（可加 --type video）", 2);
  if (rawJobs.length > 64) fail("一次最多 64 个任务（避免把额度一次跑空）", 2);

  const jobs = rawJobs.map((j, i) => normJob(j, i, outdir, typeof args.key === "string" ? args.key : undefined));
  logE(`共 ${jobs.length} 个任务，并发 ${concurrency}，输出目录 ${outdir}`);

  pool(jobs, concurrency, runJob).then((results) => {
    const succeeded = results.filter((r) => r && r.ok).length;
    const failed = results.length - succeeded;
    done({ ok: failed === 0, total: results.length, succeeded, failed, outdir, results }, failed === 0 ? 0 : 1);
  });
}
main();
