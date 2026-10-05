#!/usr/bin/env node
// U-King AIGC · 文生歌曲（Suno：带人声 / 歌词的整曲）—— 异步：提交→轮询→下载。零 npm 依赖。
// 带人声、要歌词、要「一首歌」用本脚本；只要无人声的背景配乐用同目录 gen-bgm.mjs。
//
// 三种模式（互斥）：
//   自定义（歌词）  --lyrics "<完整歌词，含 [Verse]/[Chorus]>" --tags "pop, female vocal" --title "标题"
//   描述（灵感）    --prompt "<只描述想要的歌，歌词/曲风由 Suno 自己写>"
//   纯音乐          --prompt "<描述>" --instrumental
// 另有 --gen-lyrics --prompt "<歌词主题>"：只写歌词（不出歌），落成 .txt。
//
// 协议：POST /suno/submit/music（或 /suno/submit/lyrics）→ 响应 data=任务号；
//       GET /suno/fetch/<任务号> 每 10s 轮询，最多 10 分钟；一次通常出 2 首（clips）。
// 输出：--json 出 {ok,task_id,files:[{path,title,duration,audio_url}],...}；否则 stdout 每行一个文件路径。
//       进度走 stderr。退出码 0 成功 / 1 运行错 / 2 参数错。
// 断点续跑：提交前先在 ~/.uking/music-jobs/ 落本地任务单，拿到任务号立刻写回。
//       同一条命令再跑 = 继续查询/下载原任务，不会重新提交（不会重复扣费）；--force-new 才强制新开一首。
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  closeSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, renameSync,
  rmSync, statSync, unlinkSync, writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import https from "node:https";

const BASE = "https://api.u-claw.org.cn";
const SUBMIT_PATH = { music: "/suno/submit/music", lyrics: "/suno/submit/lyrics" };
const DEFAULT_MV = "chirp-v5-5"; // 与画板默认一致，生产任务实测在用
const SUBMIT_TIMEOUT_MS = 60000;
const POLL_TIMEOUT_MS = 10 * 60 * 1000;
const POLL_EVERY_MS = 10000;
const DOWNLOAD_TRIES = 3;
// 单次价格（元）。只用来让调用方提前掂量成本，不是账单：服务器随时可能调价，实扣以余额变化为准。

const BOOL = new Set(["json", "quiet", "instrumental", "cover", "gen-lyrics", "force-new", "dry-run", "help"]);
function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "-h") { out.help = true; continue; }
    if (a.startsWith("--")) {
      const k = a.slice(2);
      if (BOOL.has(k)) { out[k] = true; continue; }
      out[k] = i + 1 < argv.length && !argv[i + 1].startsWith("--") ? argv[++i] : true;
    } else out._.push(a);
  }
  return out;
}

// Key 优先级：--key > 环境变量 XIAPAN_API_KEY > ~/.uking/device.json
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
  else if (obj.ok) process.stdout.write((obj.files ? obj.files.map((f) => f.path).join("\n") : obj.file || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1, extra = {}) {
  done({ ok: false, error: String((msg && msg.message) || msg), ...extra }, code);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const str = (x) => (typeof x === "string" ? x.trim() : "");

// 路径归一化：git-bash 风格 /c/Users/... → Windows 绝对路径（纯计算，不碰磁盘）。
// 输出落盘前再单独 ensureParent 建父目录，这样 --dry-run 不会在磁盘上留下任何东西。
function normPath(p) {
  let s = String(p);
  const m = s.match(/^\/([A-Za-z])\/(.*)$/);
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  return resolve(s);
}
function ensureParent(abs) { try { mkdirSync(dirname(abs), { recursive: true }); } catch {} }

// ── HTTP：主路径 spawn 系统 curl；缺 curl 时 JSON 调用退 node:https（下载必须有 curl）──
function hasCurl() {
  try { return spawnSync("curl", ["--version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}
const CURL = hasCurl();

function curlText(args, timeoutMs) {
  const r = spawnSync("curl", args, { timeout: timeoutMs + 5000, maxBuffer: 16 * 1024 * 1024, encoding: "utf8" });
  if (r.error) throw r.error;
  if (r.status !== 0) {
    const e = new Error(`curl 退出码 ${r.status}：${String(r.stderr || "").trim().slice(0, 200)}`);
    e.notSent = [5, 6, 7, 35].includes(r.status); // 解析/连接/握手失败：请求根本没发出去
    throw e;
  }
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
    req.on("error", (e) => { e.notSent = ["ENOTFOUND", "ECONNREFUSED", "EAI_AGAIN"].includes(e.code); rej(e); });
    req.on("timeout", () => req.destroy(new Error("请求超时")));
    if (data) req.write(data);
    req.end();
  });
}
async function postJson(path, key, bodyObj, timeoutMs) {
  if (CURL) {
    const dir = mkdtempSync(join(tmpdir(), "uking-"));
    const bf = join(dir, "body.json");
    writeFileSync(bf, JSON.stringify(bodyObj)); // body 落临时文件 + --data @file，绕中文/引号/多行歌词
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
// 抠出错误文案（兼容 {error:{message}} 与 {code:"非 success",message}）。
function errOf(v) {
  if (v && v.error != null) return v.error.message || JSON.stringify(v.error);
  if (v && typeof v.code === "string" && v.code !== "success" && v.code !== "") return v.message || v.code;
  return null;
}
// 重试无益的错误（鉴权 / 余额 / 权限 / 参数）：轮询时遇到直接停，别空等 10 分钟。
const PERMANENT_RE =
  /unauthorized|invalid[_\s-]*api[_\s-]*key|invalid[_\s-]*token|permission|forbidden|\b401\b|\b403\b|无权限|鉴权|未授权|令牌|余额不足|insufficient|quota|欠费|has no access to model|model[_\s-]*not[_\s-]*found|无可用渠道|没有可用渠道|模型不存在|invalid[_\s-]*request|参数错误|invalid[_\s-]*parameter/i;

// ── 响应解析（兼容已验证客户端的解析口径：嵌套 data、clips 位置、各种 status 写法）──
const isOk = (s) => ["success", "succeeded", "complete", "completed"].includes(String(s).toLowerCase());
const isBad = (s) => ["failed", "failure", "error", "cancelled", "canceled"].includes(String(s).toLowerCase());

// 提交响应的任务号：{code:"success",data:"<id>"}；兼容 task_id / id / data.task_id 等写法。
function taskIdOf(p) {
  for (const c of [p?.task_id, p?.taskId, p?.id, p?.data, p?.data?.task_id, p?.data?.taskId, p?.data?.id,
    p?.data?.data?.task_id, p?.data?.data?.taskId, p?.data?.data?.id]) {
    if (typeof c === "string" && c.trim()) return c.trim();
  }
  return "";
}
function extractClips(p) {
  for (const c of [p?.data, p?.clips, p?.data?.clips, p?.data?.data, p?.data?.data?.data, p?.data?.data?.clips,
    p?.data?.items, p?.data?.data?.items, p?.items]) {
    if (Array.isArray(c)) return c;
  }
  return [];
}
// 歌词结果：沿 data 链往下找带 text/title/tags 的对象（最多 6 层）。
function extractLyrics(p) {
  let cur = p, fallback = null;
  for (let d = 0; d < 6; d++) {
    if (!cur || typeof cur !== "object" || Array.isArray(cur)) break;
    const text = str(cur.text), title = str(cur.title);
    const tags = Array.isArray(cur.tags) ? cur.tags.map(str).filter(Boolean).join(", ") : str(cur.tags);
    const status = str(cur.status) || str(cur.state);
    const err = str(cur.error_message) || str(cur.errorMessage);
    if (text || title || tags) return { text, title, tags, status, err };
    if (!fallback && (status || err)) fallback = { text, title, tags, status, err };
    cur = cur.data;
  }
  return fallback;
}
function failReasonOf(p, clips, lyr) {
  for (const c of [p?.fail_reason, p?.message, p?.error, p?.error_message, p?.data?.fail_reason, p?.data?.error_message,
    p?.data?.message, p?.data?.error, clips[0]?.metadata?.error_message, lyr?.err]) {
    if (typeof c === "string" && c.trim()) return c.trim();
  }
  return "";
}
// 把一次 fetch 响应归一成 {state: processing|completed|failed, progress, clips, lyrics, reason}
function readTask(v, kind) {
  const clips = kind === "music"
    ? extractClips(v).slice().sort((a, b) => (Number.isFinite(a.batch_index) ? a.batch_index : 1e9) - (Number.isFinite(b.batch_index) ? b.batch_index : 1e9))
    : [];
  const lyrics = kind === "lyrics" ? extractLyrics(v) : null;
  const itemSt = kind === "music"
    ? clips.map((c) => str(c.status) || str(c.state)).filter(Boolean)
    : (lyrics && lyrics.status ? [lyrics.status] : []);
  const taskSt = [v?.data?.data?.status, v?.data?.status, v?.status, v?.data?.data?.state, v?.data?.state, v?.state]
    .map(str).filter(Boolean);
  let state = "processing";
  if (itemSt.some(isBad)) state = "failed";
  else if (itemSt.length && itemSt.every(isOk)) state = "completed";
  else if (taskSt.some(isBad)) state = "failed";
  else if (taskSt.some(isOk)) state = "completed";
  const progress = String(v?.data?.progress ?? v?.progress ?? "");
  const reason = state === "failed" ? failReasonOf(v, clips, lyrics) : "";
  return { state, progress, clips, lyrics, reason, taskStatus: taskSt[0] || "" };
}

// ── 本地任务单（~/.uking/music-jobs）──────────────────────────
// POST 前先落单，拿到任务号立刻写回。进程若死在「服务端已收单，但任务号还没回到本机」的缝里，
// 状态会停在 submitting——Suno 提交没有幂等键，盲目重发会再扣一次，所以这种状态必须人工 --force-new 才重提。
// 不保存歌词/提示词原文，只存不可逆指纹、任务号、输出路径与状态。
const JOB_DIR = join(homedir(), ".uking", "music-jobs");
const sha256 = (v) => createHash("sha256").update(v).digest("hex");
function readJob(path) { try { return JSON.parse(readFileSync(path, "utf8")); } catch { return null; } }
function writeJob(path, job) {
  mkdirSync(JOB_DIR, { recursive: true });
  const next = { ...job, version: 1, updated_at: new Date().toISOString() };
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, JSON.stringify(next, null, 2));
  renameSync(tmp, path);
  return next;
}
function jobPaths(fingerprint) {
  mkdirSync(JOB_DIR, { recursive: true });
  return { state: join(JOB_DIR, `${fingerprint}.json`), lock: join(JOB_DIR, `${fingerprint}.lock`) };
}
async function acquireSubmitLock(lockPath) {
  for (let i = 0; i < 120; i++) {
    try {
      const fd = openSync(lockPath, "wx");
      writeFileSync(fd, JSON.stringify({ pid: process.pid, at: new Date().toISOString() }));
      closeSync(fd);
      return;
    } catch (e) {
      if (e && e.code !== "EEXIST") throw e;
      // 提交最长 60s；锁超过 3 分钟说明持锁进程已死。只删当前指纹的单文件锁，任务单仍在。
      try {
        if (Date.now() - statSync(lockPath).mtimeMs > 3 * 60 * 1000) { unlinkSync(lockPath); continue; }
      } catch {}
      if (i === 0) logE("检测到相同歌曲正在提交，等待取得原任务（不会重复扣费）…");
      await sleep(1000);
    }
  }
  throw new Error("相同歌曲的提交锁等待超时；请稍后重试（本次未新建任务）");
}
function releaseSubmitLock(lockPath) { try { unlinkSync(lockPath); } catch {} }

// kind: not_sent（请求没发出去）| rejected（服务端明确拒绝，没建任务）| unknown（不确定服务端收没收）
function submitErr(msg, kind) { const e = new Error(String(msg)); e.kind = kind; return e; }
async function submitTask(path, key, body) {
  let resp;
  try { resp = await postJson(path, key, body, SUBMIT_TIMEOUT_MS); }
  catch (e) { throw submitErr("提交失败：" + ((e && e.message) || e), e && e.notSent ? "not_sent" : "unknown"); }
  const se = errOf(resp);
  if (se) throw submitErr(se, "rejected");
  const id = taskIdOf(resp);
  if (!id) throw submitErr("提交响应缺少任务号：" + JSON.stringify(resp).slice(0, 200), "unknown");
  return id;
}

// ── 轮询 ─────────────────────────────────────────────────
function keepErr(msg) { const e = new Error(msg); e.keepTask = true; return e; }
async function waitForTask(taskId, key, kind, onState) {
  logE(`task_id=${taskId}，开始轮询（每 10s，最多 10 分钟）…`);
  const deadline = Date.now() + POLL_TIMEOUT_MS;
  let lastErr = "", grace = 0;
  for (;;) {
    let v = null;
    try { v = await getJson(`/suno/fetch/${encodeURIComponent(taskId)}`, key, 30000); }
    catch (e) { lastErr = (e && e.message) || String(e); } // 单次网络抖动不致命，继续
    if (v) {
      const pe = errOf(v);
      if (pe) {
        if (PERMANENT_RE.test(pe)) throw new Error(pe);
        lastErr = pe;
      } else {
        const t = readTask(v, kind);
        if (t.state === "failed") {
          const e = new Error(t.reason || (kind === "lyrics" ? "歌词生成失败" : "音乐生成失败"));
          e.upstreamFailed = true;
          throw e;
        }
        if (t.state === "completed") {
          if (kind === "lyrics") { if (t.lyrics && t.lyrics.text) return t; }
          else {
            const withAudio = t.clips.filter((c) => str(c.audio_url));
            // 每首都有 audio_url 才算齐；成功态下只有部分有地址时多等 3 轮，仍不齐就交付已有的。
            if (withAudio.length && (withAudio.length === t.clips.length || ++grace >= 3)) {
              return { ...t, clips: withAudio, missing: t.clips.length - withAudio.length };
            }
          }
        }
        onState?.(t.state === "completed" ? "ready" : "running", t.progress);
        logE(`生成中 ${t.progress || t.taskStatus || ""}…`);
      }
    }
    if (Date.now() >= deadline) break;
    await sleep(POLL_EVERY_MS);
  }
  throw keepErr("歌曲 10 分钟仍未返回终态" + (lastErr ? `（最近错误：${lastErr}）` : ""));
}

// ── 下载（最多 3 次；音频可能在自家域名也可能在第三方 CDN，给什么下什么，不改写域名）──
// 只有自家域名才带 Bearer：绝不把用户的 Key 发给第三方 CDN。
function curlDownload(url, key, outPath, minBytes) {
  const part = outPath + ".part";
  let own = false;
  try { own = new URL(url).hostname === new URL(BASE).hostname; } catch {}
  const args = ["-sS", "-m", "180", "-L", "--ssl-no-revoke"]; // --ssl-no-revoke：Windows 上 CDN 吊销检查常超时
  if (own && key) args.push("-H", `Authorization: Bearer ${key}`);
  args.push("-o", part, "-w", "%{http_code}\n%{content_type}", url);
  const r = spawnSync("curl", args, { timeout: 185000, encoding: "utf8" });
  const fin = (ok, why) => {
    if (ok) { renameSync(part, outPath); return; }
    let detail = ""; try { detail = readFileSync(part, "utf8").slice(0, 120).replace(/\s+/g, " "); } catch {}
    try { unlinkSync(part); } catch {}
    throw new Error(`HTTP ${why}${detail ? "：" + detail : ""}`);
  };
  if (r.error) throw r.error;
  const [httpCode = "", contentType = ""] = String(r.stdout || "").trim().split(/\r?\n/);
  let size = 0, head = Buffer.alloc(0);
  try { size = statSync(part).size; head = readFileSync(part).subarray(0, 8); } catch {}
  // 只看大小会把 CDN 回的 JSON/HTML 错误页当成歌：再核 Content-Type 与首字节。
  const looksText = /(?:json|html|text\/plain)/i.test(contentType) || head[0] === 0x7b || head[0] === 0x3c;
  fin(r.status === 0 && /^2\d\d$/.test(httpCode) && size >= minBytes && !looksText,
    `${httpCode || "?"}（curl 退出码 ${r.status}，${contentType || "未知类型"}，${size} 字节）`);
}
async function downloadWithRetry(url, key, outPath, minBytes, label) {
  let last = null;
  for (let i = 1; i <= DOWNLOAD_TRIES; i++) {
    try { curlDownload(url, key, outPath, minBytes); return; }
    catch (e) { last = e; logE(`  ${label}下载失败（${i}/${DOWNLOAD_TRIES}）：${e.message}`); if (i < DOWNLOAD_TRIES) await sleep(2000 * i); }
  }
  throw new Error(`${label}下载失败：${last && last.message}`);
}
function extOf(url, allowed, dflt) {
  try {
    const m = new URL(url).pathname.match(/\.([a-z0-9]{2,5})$/i);
    if (m && allowed.includes(m[1].toLowerCase())) return m[1].toLowerCase();
  } catch {}
  return dflt;
}


const HELP = `用法：node gen-music.mjs <模式> [选项]

模式（三选一；--gen-lyrics 另算）：
  自定义歌词   --lyrics "<完整歌词>" | --lyrics-file 歌词.txt   [--tags "pop, female vocal"] [--title 标题]
  描述         --prompt "<只描述想要的歌>"
  纯音乐       --prompt "<描述>" --instrumental
  只写歌词     --gen-lyrics --prompt "<歌词主题>"            （不出歌；--out 为 .txt 文件路径）

选项：
  --mv <版本>      Suno 模型版本，默认 ${DEFAULT_MV}（如 chirp-v4-5 / chirp-hawk）
  --out <前缀|目录>  歌曲输出前缀或目录，默认 ./uking-music-<时间>；实际文件为 <前缀>-1.mp3、<前缀>-2.mp3
  --cover          同时下载封面图（<前缀>-N.jpeg）
  --key <Key>      API Key（默认 环境变量 XIAPAN_API_KEY，再到 ~/.uking/device.json）
  --force-new      忽略本地任务单，强制新提交一首（会再次扣费）
  --dry-run        只打印将要发送的请求（不联网、不扣费、不需要 Key）
  --json           stdout 输出 JSON；--quiet 不打进度
  -h, --help       本帮助

退出码：0 成功 · 1 运行/网络/生成失败 · 2 参数错误
`;

async function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;
  if (args.help) { process.stdout.write(HELP); process.exit(0); }

  const genLyrics = !!args["gen-lyrics"];
  const prompt = str(args.prompt);
  let lyrics = str(args.lyrics);
  if (typeof args["lyrics-file"] === "string") {
    if (lyrics) fail("--lyrics 与 --lyrics-file 只能给一个", 2);
    try { lyrics = readFileSync(normPath(args["lyrics-file"]), "utf8"); if (lyrics.charCodeAt(0) === 0xFEFF) lyrics = lyrics.slice(1); lyrics = lyrics.trim(); }
    catch (e) { fail(`读不了歌词文件 ${args["lyrics-file"]}：${e.message}`, 2); }
    if (!lyrics) fail("歌词文件是空的", 2);
  }
  const tags = str(args.tags), title = str(args.title);
  const mv = str(args.mv) || DEFAULT_MV;
  const instrumental = !!args.instrumental;

  // ── 参数校验 + 组装请求体（字段名对齐上游 Suno 协议）──
  let kind, mode, body;
  if (genLyrics) {
    if (!prompt) fail("缺少 --prompt（歌词主题，如「写一首关于秋天和思念的短歌词」）", 2);
    if (lyrics || instrumental) fail("--gen-lyrics 只接 --prompt，不能和 --lyrics / --instrumental 一起用", 2);
    kind = "lyrics"; mode = "gen-lyrics"; body = { prompt };
  } else {
    kind = "music";
    if (lyrics && prompt) fail("--prompt（描述模式）与 --lyrics（自定义歌词模式）不能同时给；歌词放 --lyrics，风格放 --tags", 2);
    if (lyrics && instrumental) fail("纯音乐（--instrumental）不能带歌词；要人声请去掉 --instrumental", 2);
    if (!lyrics && !prompt) fail("缺少 --prompt（歌曲描述）或 --lyrics / --lyrics-file（完整歌词）", 2);
    if (lyrics) {
      mode = "custom";
      body = { prompt: lyrics, mv };
      if (tags) body.tags = tags;
      if (title) body.title = title;
    } else {
      // 描述模式：Suno 自己写歌词/定曲风，tags/title 在这个模式下不生效。
      if (tags || title) logE("提示：描述模式下 --tags / --title 不生效，已忽略（要指定风格/标题请用 --lyrics 自定义模式）");
      mode = instrumental ? "instrumental" : "description";
      body = { gpt_description_prompt: prompt, mv };
      if (instrumental) { body.prompt = ""; body.make_instrumental = true; }
    }
  }
  const path = SUBMIT_PATH[kind];

  // ── 输出路径（纯计算；--out 以 / 或 \ 结尾，或本身是已有目录 → 当目录用）──
  const ts = Date.now();
  const rawOut = typeof args.out === "string" ? args.out : "";
  const outAbs = rawOut ? normPath(rawOut) : "";
  const outIsDir = !!rawOut && (/[\\/]$/.test(rawOut) || (existsSync(outAbs) && statSync(outAbs).isDirectory()));
  let outPrefix = "", outFile = "";
  if (kind === "lyrics") {
    outFile = !rawOut ? resolve(`./uking-lyrics-${ts}.txt`) : outIsDir ? join(outAbs, `uking-lyrics-${ts}.txt`) : outAbs;
  } else {
    outPrefix = !rawOut ? resolve(`./uking-music-${ts}`)
      : outIsDir ? join(outAbs, `uking-music-${ts}`)
        : outAbs.replace(/\.(?:mp3|m4a|wav|ogg|flac|aac)$/i, "");
  }

  if (args["dry-run"]) {
    process.stdout.write(JSON.stringify({ ok: true, dry_run: true, mode, endpoint: BASE + path, body,
      ...(kind === "lyrics" ? { out_file: outFile } : { out_prefix: outPrefix }) }) + "\n");
    process.exit(0);
  }
  ensureParent(kind === "lyrics" ? outFile : outPrefix + "-1.mp3");

  const key = resolveKey(args);
  if (!key) fail("找不到 API Key（--key / 环境变量 XIAPAN_API_KEY / ~/.uking/device.json）", 2);
  if (!CURL) fail("下载歌曲需要系统 curl（Win10+ 自带）。装好 curl 后重试。", 1);

  const fingerprint = sha256(JSON.stringify({ path, body, keyHash: sha256(key) }));
  const paths = jobPaths(fingerprint);
  const t0 = Date.now();
  const label = kind === "lyrics" ? "歌词" : "歌曲";

  // ── 取得任务：复用已交付 / 续跑未交付 / 新提交 ──
  // 注意：锁内不许 done()/fail()（它们 process.exit，finally 不会跑，锁文件会留下挡住后续重跑）；
  // 结论先记到 early / subErr，出锁后再处理。
  let state = null, taskId = "", resumed = false, early = null, subErr = null;
  await acquireSubmitLock(paths.lock);
  try {
    state = readJob(paths.state);
    if (!args["force-new"] && state) {
      const delivered = kind === "lyrics"
        ? state.out_file === outFile && existsSync(outFile)
        : state.out_prefix === outPrefix && Array.isArray(state.files) && state.files.length > 0 && state.files.every((f) => existsSync(f.path));
      if (state.status === "downloaded" && delivered) {
        early = { res: kind === "lyrics"
          ? { ok: true, task_id: state.task_id, mode, file: outFile, ...state.result, resumed: true, elapsed: "0s" }
          : { ok: true, task_id: state.task_id, mode, mv, files: state.files, file: state.files[0].path, resumed: true, elapsed: "0s" } };
      } else if (["queued", "running", "ready", "download_failed"].includes(state.status) && state.task_id) {
        resumed = true; taskId = state.task_id;
        logE(`恢复未交付任务 ${taskId}（不会重新扣费）`);
      } else if (["submitting", "submit_unknown"].includes(state.status)) {
        early = { err: "上次提交被中断或结果不明：服务端可能已收单并扣费，但本机没拿到任务号。"
          + `请先核对账户余额有没有被扣；确认没有生成、想重新提交请加 --force-new（可能重复扣费）。任务单：${paths.state}` };
      }
    }
    if (!early && !taskId) {
      logE(`提交${label}：模式 ${mode}${kind === "music" ? `，版本 ${mv}` : ""}…`);
      state = writeJob(paths.state, { fingerprint, task_id: "", status: "submitting", kind, mode, mv, created_at: new Date().toISOString() });
      try { taskId = await submitTask(path, key, body); }
      catch (e) { subErr = e; state = writeJob(paths.state, { ...state, status: e.kind === "unknown" ? "submit_unknown" : "failed", error: e.message }); }
      if (!subErr) state = writeJob(paths.state, { ...state, task_id: taskId, status: "queued" });
    }
  } finally { releaseSubmitLock(paths.lock); }
  if (early && early.res) { logE(`已找到同一条命令交付过的${label}，直接复用（不会重复扣费）`); done(early.res); }
  if (early && early.err) fail(early.err, 1);
  if (subErr) {
    if (subErr.kind === "unknown") fail(`${subErr.message}。服务端是否已收单不确定，已记入任务单；重跑同一命令会先提示核对余额，不会盲目重复提交。`, 1);
    fail(subErr);
  }

  // ── 轮询 ──
  let result;
  try {
    result = await waitForTask(taskId, key, kind, (status, progress) => { state = writeJob(paths.state, { ...state, status, progress }); });
  } catch (e) {
    if (e.upstreamFailed) {
      // 上游明确判定这次生成失败：任务单置 failed，下次同命令才会重新提交。
      state = writeJob(paths.state, { ...state, status: "failed", error: e.message });
      fail(`${label}生成失败：${e.message}（常见于歌词/描述触发内容审核或临时波动，可调整内容后重试）`, 1, { task_id: taskId });
    }
    // 其余（超时 / 鉴权 / 查询报错）≠ 任务失败：原任务可能仍在生成，保留任务单，绝不自动另开一首。
    state = writeJob(paths.state, { ...state, status: "running", error: e.message });
    fail(e.keepTask
      ? `${label}仍在服务端生成，任务已保留；稍后运行同一命令会继续查询和下载，不会重新提交或扣费。（${e.message}）`
      : `${e.message}。任务已保留（task_id=${taskId}）；问题解决后重跑同一命令会继续，不会重新扣费；若确认任务已失效可加 --force-new 重新提交。`,
    1, { task_id: taskId });
  }
  state = writeJob(paths.state, { ...state, status: "ready" });

  // ── 落盘 ──
  if (kind === "lyrics") {
    const text = result.lyrics.text;
    writeFileSync(outFile, text + "\n", "utf8");
    const meta = { title: result.lyrics.title || "", tags: result.lyrics.tags || "", lyrics: text };
    writeJob(paths.state, { ...state, status: "downloaded", out_file: outFile, result: meta, error: "" });
    done({ ok: true, task_id: taskId, mode, file: outFile, ...meta, resumed, elapsed: Math.round((Date.now() - t0) / 1000) + "s" });
  }

  if (result.missing) logE(`注意：服务端另有 ${result.missing} 首暂未给出音频地址，本次只交付已就绪的；稍后可凭 task_id 手动查询（见 SKILL.md「纯 curl 用法」）。`);
  logE(`下载 ${result.clips.length} 首…`);
  const files = [];
  try {
    for (let i = 0; i < result.clips.length; i++) {
      const c = result.clips[i], n = i + 1;
      const audioUrl = str(c.audio_url);
      const audioPath = `${outPrefix}-${n}.${extOf(audioUrl, ["mp3", "m4a", "wav", "ogg", "flac", "aac", "opus"], "mp3")}`;
      await downloadWithRetry(audioUrl, key, audioPath, 1024, `第 ${n} 首`);
      const f = {
        path: audioPath, title: str(c.title), duration: c.duration ?? c.metadata?.duration ?? null,
        audio_url: audioUrl, clip_id: str(c.clip_id) || str(c.id),
      };
      const coverUrl = str(c.image_large_url) || str(c.image_url);
      if (args.cover && coverUrl) {
        const coverPath = `${outPrefix}-${n}.${extOf(coverUrl, ["jpg", "jpeg", "png", "webp"], "jpeg")}`;
        try { await downloadWithRetry(coverUrl, key, coverPath, 256, `第 ${n} 首封面`); f.cover = coverPath; }
        catch (e) { logE(`  封面跳过：${e.message}`); } // 封面是附带品，失败不拖垮整首歌
      }
      files.push(f);
    }
  } catch (e) {
    state = writeJob(paths.state, { ...state, status: "download_failed", error: e.message });
    fail(`${label}已生成，但下载失败：${e.message}。任务已保留；重跑同一命令会继续下载，不会重新扣费。`, 1,
      { task_id: taskId, clips: result.clips.map((c) => ({ title: str(c.title), audio_url: str(c.audio_url) })) });
  }
  writeJob(paths.state, { ...state, status: "downloaded", out_prefix: outPrefix, files, error: "" });
  done({ ok: true, task_id: taskId, mode, mv, files, file: files[0].path, ...(result.missing ? { missing_clips: result.missing } : {}),
    resumed, elapsed: Math.round((Date.now() - t0) / 1000) + "s" });
}
main().catch((err) => fail(err));
