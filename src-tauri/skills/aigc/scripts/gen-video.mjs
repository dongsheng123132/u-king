#!/usr/bin/env node
// U-King AIGC · 文生视频 / 图生视频 —— 直连虾盘云视频（异步：提交→轮询→下载）。零 npm 依赖。
// 用法见同目录 ../SKILL.md。输出：--json 出 {ok,file,...}；退出码 0 成功 / 1 运行错 / 2 参数错。
import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  closeSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, renameSync,
  rmSync, statSync, unlinkSync, writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import https from "node:https";

// ── 输出路径归一化：弱模型常给 git-bash 风格 `/c/Users/...`（node 会错解成 C:\c\Users），
// 或指向还不存在的子目录 → 转成 Windows 绝对路径 + 自动建父目录，挡掉 ENOENT/路径穿帮。
function normalizeOut(p) {
  let s = String(p);
  const m = s.match(/^\/([A-Za-z])\/(.*)$/);
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  const abs = resolve(s);
  try { mkdirSync(dirname(abs), { recursive: true }); } catch {}
  return abs;
}

const BASE = "https://api.u-claw.org.cn";

// ── 参数解析（--flag value；--json/--quiet 为布尔；--ref/--image 可重复成数组）──
const BOOL = new Set(["json", "quiet", "force-new"]);
const REPEAT = new Set(["ref", "image"]);
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

// ── Key 优先级：--key > 环境变量 XIAPAN_API_KEY > ~/.uking/device.json ──
function resolveKey(args) {
  if (args.key && args.key !== true) return String(args.key);
  if (process.env.XIAPAN_API_KEY) return process.env.XIAPAN_API_KEY;
  try {
    const j = JSON.parse(readFileSync(join(homedir(), ".uking", "device.json"), "utf8"));
    if (j && j.key) return j.key;
  } catch {}
  return "";
}

let QUIET = false, JSONMODE = false;
function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
// stdout 只出结果（--json 出契约 JSON，否则出文件路径）；进度/错误走 stderr + 退出码。
function done(obj, code = 0) {
  if (JSONMODE) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (obj.ok) process.stdout.write((obj.file || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) { done({ ok: false, error: String((msg && msg.message) || msg) }, code); }

// ── HTTP：主路径 spawn 系统 curl（对齐后端踩坑）；缺 curl 时 JSON 调用退 node:https ──
function hasCurl() {
  try { return spawnSync("curl", ["--version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}
const CURL = hasCurl();

function curlText(args, timeoutMs) {
  const r = spawnSync("curl", args, { timeout: timeoutMs + 5000, maxBuffer: 64 * 1024 * 1024, encoding: "utf8" });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`curl 退出码 ${r.status}：${String(r.stderr || "").slice(0, 200)}`);
  return r.stdout || "";
}
function parseJsonOr(txt, label) {
  try { return JSON.parse(txt); }
  catch { throw new Error(`${label}响应不是 JSON：${String(txt).slice(0, 200)}`); }
}
function httpsRequest(method, url, { auth, json } = {}, timeoutMs = 60000) {
  return new Promise((res, rej) => {
    const u = new URL(url);
    const data = json ? Buffer.from(JSON.stringify(json)) : null;
    const req = https.request(
      {
        method, hostname: u.hostname, port: 443, path: u.pathname + u.search,
        headers: {
          ...(auth ? { Authorization: `Bearer ${auth}` } : {}),
          ...(data ? { "Content-Type": "application/json", "Content-Length": data.length } : {}),
        },
        timeout: timeoutMs,
      },
      (r) => {
        const chunks = [];
        r.on("data", (c) => chunks.push(c));
        r.on("end", () => {
          const txt = Buffer.concat(chunks).toString("utf8");
          try { res(JSON.parse(txt)); } catch { rej(new Error("响应不是 JSON：" + txt.slice(0, 200))); }
        });
      }
    );
    req.on("error", rej);
    req.on("timeout", () => req.destroy(new Error("请求超时")));
    if (data) req.write(data);
    req.end();
  });
}
async function postJson(path, key, bodyObj, timeoutMs) {
  if (CURL) {
    const dir = mkdtempSync(join(tmpdir(), "uking-"));
    const bf = join(dir, "body.json");
    writeFileSync(bf, JSON.stringify(bodyObj)); // body 落临时文件 + --data @file，绕中文/引号
    try {
      const txt = curlText(
        ["-sS", "-m", String(Math.ceil(timeoutMs / 1000)), "-X", "POST", BASE + path,
          "-H", `Authorization: Bearer ${key}`, "-H", "Content-Type: application/json", "--data", `@${bf}`],
        timeoutMs
      );
      return parseJsonOr(txt, "接口");
    } finally { rmSync(dir, { recursive: true, force: true }); }
  }
  return httpsRequest("POST", BASE + path, { auth: key, json: bodyObj }, timeoutMs);
}
async function getJson(path, key, timeoutMs) {
  if (CURL) {
    const txt = curlText(["-sS", "-m", String(Math.ceil(timeoutMs / 1000)), BASE + path, "-H", `Authorization: Bearer ${key}`], timeoutMs);
    return parseJsonOr(txt, "接口");
  }
  return httpsRequest("GET", BASE + path, { auth: key }, timeoutMs);
}
// 下载视频 → 落盘 + 校验。result_url 域名 .org→.org.cn，--ssl-no-revoke 防 CDN 吊销坑（对齐 video.rs::download）。
function download(url, key, outPath, timeoutMs) {
  if (!CURL) throw new Error("下载视频需要系统 curl（本机无 curl）。请按 SKILL.md 的 curl 文档手动下载 result_url。");
  const u = url.replace("://api.u-claw.org/", "://api.u-claw.org.cn/");
  const args = ["-sS", "-m", String(Math.ceil(timeoutMs / 1000)), "-L", "--ssl-no-revoke"];
  if (key) args.push("-H", `Authorization: Bearer ${key}`);
  args.push("-o", outPath, "-w", "%{http_code}\n%{content_type}\n%{size_download}", u);
  const r = spawnSync("curl", args, { timeout: timeoutMs + 5000, encoding: "utf8" });
  if (r.error) throw r.error;
  const [httpCode = "", contentType = ""] = String(r.stdout || "").trim().split(/\r?\n/);
  let sz = 0; try { sz = statSync(outPath).size; } catch {}
  let head = Buffer.alloc(0);
  try { head = readFileSync(outPath).subarray(0, 64); } catch {}
  // MP4 的 ftyp box 应出现在文件头；只看大小会把 CDN 返回的 28KB JSON/HTML 错当视频。
  const hasMp4Magic = head.indexOf(Buffer.from("ftyp")) >= 0;
  const contentLooksWrong = /(?:json|html|text\/plain)/i.test(contentType);
  const ok = r.status === 0 && /^2\d\d$/.test(httpCode) && sz >= 1024 && hasMp4Magic && !contentLooksWrong;
  if (!ok) {
    try { unlinkSync(outPath); } catch {}
    throw new Error(`视频下载校验失败（HTTP ${httpCode || "?"}，${contentType || "未知类型"}，${sz} 字节）；已保留原任务，重跑同一命令不会重复扣费`);
  }
}
// 抠出上游错误文案（兼容 {error:{message}} 与 {code,message}）。
function errOf(v) {
  if (v && v.error != null) return v.error.message || JSON.stringify(v.error);
  if (v && typeof v.code === "string" && v.code !== "success" && v.code !== "") return v.message || v.code;
  return null;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── 按次扣费任务的本地事务日志 ─────────────────────────────
// 一定要在 POST 前先落 request_id：进程若死在“服务端已收单/扣费，但 task_id 响应还没
// 回本机”的缝里，下次同命令会拿同一 idempotency_key 重放，服务端只返回原任务、不再扣费。
// 不保存 prompt / 首帧图内容，只存不可逆指纹、task_id、输出路径和状态。
const JOB_DIR = join(homedir(), ".uking", "video-jobs");

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}
function readJob(path) {
  try { return JSON.parse(readFileSync(path, "utf8")); } catch { return null; }
}
function writeJob(path, job) {
  mkdirSync(JOB_DIR, { recursive: true });
  const next = { ...job, version: 1, updated_at: new Date().toISOString() };
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, JSON.stringify(next, null, 2));
  renameSync(tmp, path);
  return next;
}
function jobIdentity({ model, prompt, duration, resolution, img, key }) {
  let imageHash = "";
  if (img) imageHash = sha256(readFileSync(img));
  return sha256(JSON.stringify({ model, prompt, duration, resolution, imageHash, keyHash: sha256(key) }));
}
function jobPaths(fingerprint) {
  mkdirSync(JOB_DIR, { recursive: true });
  return { state: join(JOB_DIR, `${fingerprint}.json`), lock: join(JOB_DIR, `${fingerprint}.lock`) };
}
async function acquireSubmitLock(lockPath) {
  for (let i = 0; i < 180; i++) {
    try {
      const fd = openSync(lockPath, "wx");
      writeFileSync(fd, JSON.stringify({ pid: process.pid, at: new Date().toISOString() }));
      closeSync(fd);
      return;
    } catch (e) {
      if (e && e.code !== "EEXIST") throw e;
      // 提交最长 120s；锁超过 6 分钟说明持锁进程已死。删的是当前指纹的单文件锁，
      // 状态文件仍保留 request_id，接管者重放也不会二次扣费。
      try {
        if (Date.now() - statSync(lockPath).mtimeMs > 6 * 60 * 1000) {
          unlinkSync(lockPath);
          continue;
        }
      } catch {}
      if (i === 0) logE("检测到相同视频正在提交，等待取得原任务（不会重复扣费）…");
      await sleep(1000);
    }
  }
  throw new Error("相同视频的提交锁等待超时；请稍后重试（本次未新建任务）");
}
function releaseSubmitLock(lockPath) {
  try { unlinkSync(lockPath); } catch {}
}
function resumable(job, out) {
  if (!job || !job.request_id) return false;
  if (["submitting", "queued", "running", "ready", "download_failed"].includes(job.status)) return true;
  // 批量父进程被杀后重跑同一 jobs.json：子任务可能其实已经交付到同一路径，直接复用。
  return job.status === "downloaded" && job.out === out && existsSync(out);
}

// 火山引擎 Ark Seedance 计价（2026-07-06 上线，真金验收过）：5s/480p 为基准价，
// 时长按 5s 档比例、720p 分辨率 ×1.5。这里只做「大概估算」给调用方提前掂量成本，
// **不是精确账单**——实扣以余额变化为准，服务器随时可能调价（对齐 models.ts 里
// 「价格不写死在客户端」的原则，这个估算只在本技能的开发者/Agent 场景里提示用，
// 不出现在 U-King 客户端 GUI 上）。
const BASE_PRICE_CNY = { "doubao-seedance-2-0-mini-260615": 2.9, "doubao-seedance-2-0-fast-260128": 4.9, "doubao-seedance-2-0-260128": 6.9 };
function estimateCost(model, durationSec, resolution) {
  const base = BASE_PRICE_CNY[model];
  if (!base) return null;
  const durMult = Math.max(1, durationSec / 5);
  // 480p 基准；720p ×1.5（实测定价）；1080p 更贵，这里给个粗略 ×2.5 提示（非精确账单，实扣以余额为准）。
  const resMult = resolution === "1080p" ? 2.5 : resolution === "720p" ? 1.5 : 1;
  return Math.round(base * durMult * resMult * 100) / 100;
}

// 带「可重试」标记的错误：上游本次生成失败/超时=可重试；鉴权/余额/参数=不可重试（重试无益）。
function vErr(msg, retriable, keepTask = false) {
  const e = new Error(String(msg));
  e.retriable = !!retriable;
  e.keepTask = !!keepTask;
  return e;
}

// 只有「重试确实无益」的才判死：鉴权 / 余额 / 模型或渠道不存在 / 参数非法。
// 其余（上游抖动、限流、5xx、超时）一律当可重试 —— **默认可重试**，因为提交失败上游会退费，
// 多试两次几乎零成本，而漏判成永久错误会让整条成片直接失败。
// 实测（2026-07-28 客户机 pc-***）：同一模型同一提示词，「火山视频任务创建失败，已自动退回本次
// 扣费。」16:24 失败、16:27 成功 —— 这条以前被一刀切成 retriable=false，一次都不重试，是
// T-King 影爆「一键成片全灭」的直接原因。
const PERMANENT_RE =
  /unauthorized|invalid[_\s-]*api[_\s-]*key|invalid[_\s-]*token|permission|forbidden|\b401\b|\b403\b|无权限|鉴权|未授权|令牌|余额不足|insufficient|quota|欠费|has no access to model|model[_\s-]*not[_\s-]*found|无可用渠道|没有可用渠道|模型不存在|invalid[_\s-]*request|参数错误|invalid[_\s-]*parameter/i;
function submitRetriable(msg) { return !PERMANENT_RE.test(String(msg || "")); }
// 把笼统的上游失败翻成人话（上游 Veo 常只回 "task failed" 无细节）。
function humanize(msg) {
  const s = String(msg || "");
  if (/task\s*failed|生成失败|^failed$/i.test(s)) return "上游模型本次生成失败（常见于内容审核命中或临时波动），多次重试仍未成功，请换个提示词或稍后再试";
  return s;
}

async function submitTask(body, key) {
  // 图生视频要上传较大的 base64 图，提交本身可能慢 —— 给 120s。
  let sub;
  try { sub = await postJson("/v1/video/generations", key, body, 120000); }
  catch (e) { throw vErr("提交视频任务失败：" + (e && e.message || e), true); }
  // 提交即报错：按内容分类。鉴权/余额/模型/参数 —— 重试无益；上游抖动/限流/5xx —— 重试多半就好。
  const se = errOf(sub); if (se) throw vErr(se, submitRetriable(se));
  const taskId = sub.task_id || sub.id;
  if (!taskId) throw vErr("提交响应缺少 task_id：" + JSON.stringify(sub).slice(0, 200), false);
  return taskId;
}

// 轮询已有任务。提交和轮询分开，才能在 POST 前持久化幂等键、拿到 task_id 后立即落盘并释放锁。
async function waitForTask(taskId, key, onState) {
  logE(`task_id=${taskId}，开始轮询（每 5s，最多 20 分钟）…`);
  // 轮询（对齐 video.rs：5s × 240，status 归一）
  for (let i = 0; i < 240; i++) {
    await sleep(5000);
    let v;
    try { v = await getJson(`/v1/video/generations/${taskId}`, key, 30000); }
    catch { continue; } // 单次轮询网络抖动不致命，继续重试
    const pe = errOf(v); if (pe) throw vErr(pe, submitRetriable(pe)); // 轮询期报错同样分类，别把上游抖动判死
    const dd = (v && v.data) || v || {};
    const raw = String(dd.status || "").toUpperCase();
    const prog = dd.progress != null ? String(dd.progress) : "";
    if (/SUCCESS|SUCCEED|COMPLET/.test(raw)) {
      onState?.("ready", prog);
      return { resultUrl: dd.result_url, taskId };
    }
    if (/FAIL|ERROR|CANCEL/.test(raw)) throw vErr(dd.fail_reason || "task failed", true); // 上游本次失败 —— 可重试
    onState?.("running", prog);
    logE(`生成中 ${prog}…`);
  }
  // 没拿到终态 ≠ 上游失败。原任务可能仍在生成，绝不能自动另开一条再次扣费。
  throw vErr("视频任务 20 分钟仍未返回终态", false, true);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;
  const prompt = typeof args.prompt === "string" ? args.prompt : "";
  if (!prompt) fail("缺少 --prompt（视频画面描述；图生视频也要写希望首帧怎么动）", 2);
  const img = typeof args.image === "string" ? args.image : Array.isArray(args.image) ? args.image[0] : "";
  // 默认模型：字节 Seedance mini（快·省），三档都支持文生/图生视频，同一个模型族只是质量档位不同。
  const model = (typeof args.model === "string" && args.model) || "doubao-seedance-2-0-mini-260615";
  // 秒，默认 5s；服务器按 5~15s 钳位（clampVideoDuration），这里同步钳位，避免估价算出
  // 服务器实际不会用的时长（比如填 3s，服务器会当 5s 收费，本地估算却按 3s 算，对不上）。
  const duration = Math.min(15, Math.max(5, parseInt(args.duration, 10) || 5));
  // 480p 基准 / 720p ×1.5 / 1080p 更贵（上游 Seedance 已实测接受并出片）。非白名单一律回落 480p。
  const resolution = ["720p", "1080p"].includes(args.resolution) ? args.resolution : "480p";
  const out = normalizeOut((typeof args.out === "string" && args.out) || `./uking-video-${Date.now()}.mp4`);
  // 默认重试 2 次：上游偶发「跑到 100% 才判 failed」的间歇性失败很常见，重提一次多半就好。
  const rp = parseInt(args.retries, 10);
  const retries = Math.max(0, Math.min(5, Number.isFinite(rp) ? rp : 2));
  const key = resolveKey(args);
  if (!key) fail("找不到 API Key（--key / 环境变量 XIAPAN_API_KEY / ~/.uking/device.json）", 2);

  const fingerprint = jobIdentity({ model, prompt, duration, resolution, img, key });
  const paths = jobPaths(fingerprint);

  const est = estimateCost(model, duration, resolution);
  if (est != null) logE(`预计费用（估算，非精确账单）：约 ¥${est}`);

  const t0 = Date.now();
  // 提交体（图生视频：--image 读文件转 data url 塞 image_url）
  const body = { model, prompt, duration, resolution };
  if (img) {
    const buf = readFileSync(img);
    const mime = buf[0] === 0xff && buf[1] === 0xd8 ? "image/jpeg" : "image/png";
    body.image_url = `data:${mime};base64,${buf.toString("base64")}`;
  }

  let state = null;
  let taskId = null;
  let wasResumed = false;
  let reusedFile = false;
  await acquireSubmitLock(paths.lock);
  try {
    state = readJob(paths.state);
    if (!args["force-new"] && resumable(state, out)) {
      if (state.status === "downloaded" && state.out === out && existsSync(out)) {
        logE(`已找到同一批次交付文件，直接复用：${out}`);
        reusedFile = true;
      } else {
        wasResumed = true;
        taskId = state.task_id || null;
        logE(taskId ? `恢复未交付任务 ${taskId}（不会重新扣费）` : "恢复提交中的任务（使用原幂等键，不会重复扣费）");
      }
    } else {
      state = writeJob(paths.state, {
        fingerprint, request_id: `ukv1-${randomUUID()}`, task_id: "", status: "submitting",
        model, duration, resolution, out, attempts: 1, created_at: new Date().toISOString(),
      });
    }
    if (!reusedFile && !taskId) {
      body.idempotency_key = state.request_id;
      taskId = await submitTask(body, key);
      state = writeJob(paths.state, { ...state, task_id: taskId, status: "queued" });
    }
  } finally {
    releaseSubmitLock(paths.lock);
  }
  if (reusedFile) {
    done({ ok: true, file: resolve(out), model, task_id: state.task_id, attempts: state.attempts || 1, resumed: true, elapsed: "0s" });
  }

  let resultUrl = null, usedAttempts = Number(state.attempts || 1), lastErr = null;
  for (let attempt = 0; attempt <= retries; attempt++) {
    if (attempt) {
      logE(`上游本次失败，自动重试 ${attempt}/${retries}…`);
      await acquireSubmitLock(paths.lock);
      try {
        state = writeJob(paths.state, {
          ...state, request_id: `ukv1-${randomUUID()}`, task_id: "", status: "submitting",
          attempts: usedAttempts + 1, error: "",
        });
        body.idempotency_key = state.request_id;
        taskId = await submitTask(body, key);
        usedAttempts += 1;
        state = writeJob(paths.state, { ...state, task_id: taskId, status: "queued", attempts: usedAttempts });
      } finally {
        releaseSubmitLock(paths.lock);
      }
    }
    logE(`${attempt ? "重新提交" : "处理"}视频任务：模型 ${model}，${duration}s / ${resolution}…`);
    try {
      const r = await waitForTask(taskId, key, (status, progress) => {
        state = writeJob(paths.state, { ...state, status, progress });
      });
      resultUrl = r.resultUrl;
      state = writeJob(paths.state, { ...state, status: "ready", result_url: resultUrl || "" });
      break;
    } catch (e) {
      lastErr = e;
      if (e.keepTask) {
        state = writeJob(paths.state, { ...state, status: "running", error: humanize(e.message) });
        fail("视频仍在服务端生成，任务已保留；稍后运行同一命令会继续查询和下载，不会重新提交或扣费。");
      }
      state = writeJob(paths.state, { ...state, status: "failed", error: humanize(e.message) });
      if (!e.retriable) fail(humanize(e.message)); // 鉴权/余额/参数 —— 直接报错，不重试
      logE(`失败：${e.message}`);
    }
  }
  if (!resultUrl) fail(humanize((lastErr && lastErr.message) || "视频生成失败"));

  // 下载（域名改写 + auth + --ssl-no-revoke + 校验 >1KB）
  logE("下载视频…");
  try {
    download(resultUrl, key, out, 300000);
  } catch (e) {
    state = writeJob(paths.state, { ...state, status: "download_failed", error: String(e && e.message || e) });
    fail(`视频已生成，但下载暂时失败：${e && e.message || e}。任务已保留；下次运行同一命令会继续下载，不会重新扣费。`);
  }
  state = writeJob(paths.state, { ...state, status: "downloaded", out: resolve(out), error: "", result_url: "" });
  done({
    ok: true, file: resolve(out), model, task_id: taskId, attempts: usedAttempts,
    resumed: wasResumed,
    elapsed: Math.round((Date.now() - t0) / 1000) + "s",
  });
}
main().catch((err) => fail(err));
