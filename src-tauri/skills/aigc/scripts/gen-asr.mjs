#!/usr/bin/env node
// U-King AIGC · 语音转文字（ASR）—— 音频/视频转写、口播稿整理、会议录音摘要的第一步。
// 零 npm 依赖（node 内置 + 系统 curl + ffmpeg）。--json 出 {ok,text,...}；退出码 0 成功 / 1 运行错 / 2 参数错。
//
// 用法：
//   node scripts/gen-asr.mjs --in 录音.mp3 --json
//   node scripts/gen-asr.mjs --in 视频.mp4 --out 转写.txt --json     # 视频自动抽音轨
//   node scripts/gen-asr.mjs --in 会议.m4a --prompt "这是产品评审会，注意人名和术语" --json
//
// 🔴 **虾盘云没有独立的 ASR 端点**（2026-08-16 实测）：`/v1/audio/transcriptions` 路由存在，
//    但全站 8 个音频模型全是 TTS，一个 ASR 模型都没挂。所以这里走的是**跟 gen-tts 同一条路**：
//    Omni 模型的 chat/completions，把音频当 `input_audio` 传进去。好处是零新渠道、零新模型。
//
// ⚠️ 它给的是**纯文本，没有逐句时间戳**（Omni 是理解模型不是对齐模型）。
//    要做漫剧字幕别用它 —— 字幕应该来自剧本（你本来就知道每句话是什么、配音多长），
//    从 ASR 反推时间戳是把确定的事变成不确定的。ASR 的位置是「转写别人给的音视频」。
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, statSync, mkdtempSync, rmSync, mkdirSync, existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve, dirname, extname } from "node:path";

const BASE = "https://api.u-claw.org.cn";
const CHAT_URL = BASE + "/v1/chat/completions";
const REQ_TIMEOUT_MS = 240000;
const DEFAULT_MODEL = "qwen3-omni-flash";
// 单段上限（秒）。太长会让 base64 请求体过大、上游也更容易截断。超长自动切段、逐段转写再拼。
const SEG_SECONDS = 120;
// 直接能当 input_audio 传的容器；其余（含所有视频）一律先用 ffmpeg 抽成 mp3。
const AUDIO_OK = new Set([".mp3", ".wav", ".m4a", ".aac", ".ogg", ".flac"]);

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
  try {
    const j = JSON.parse(readFileSync(join(homedir(), ".uking", "device.json"), "utf8"));
    if (j && j.key) return j.key;
  } catch {}
  return "";
}

let QUIET = false, JSONMODE = false;
function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
function done(obj, code = 0) {
  if (JSONMODE) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (obj.ok) process.stdout.write((obj.text || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) { done({ ok: false, error: String((msg && msg.message) || msg) }, code); }

// 探针参数按各家来：curl 只认 `--version`，ffmpeg/ffprobe 认 `-version`。
function has(bin) {
  const flag = bin === "curl" ? "--version" : "-version";
  try { return spawnSync(bin, [flag], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}

function normalizePath(p) {
  let s = String(p);
  const m = s.match(/^\/([A-Za-z])\/(.*)$/);
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  return resolve(s);
}

function durationOf(f) {
  const r = spawnSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f], { encoding: "utf8" });
  if (r.status !== 0) return null;
  const d = parseFloat(String(r.stdout).trim());
  return Number.isFinite(d) ? d : null;
}

// 抽音轨 / 转码成统一 mp3（单声道 16k 足够转写，且能把请求体压小一个量级）。
function toMp3(src, outPath) {
  const r = spawnSync("ffmpeg", ["-y", "-i", src, "-vn", "-ac", "1", "-ar", "16000", "-c:a", "libmp3lame", "-q:a", "6", outPath],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
  if (r.status !== 0)
    throw new Error("ffmpeg 抽音轨失败：" + String(r.stderr || "").split(/\r?\n/).filter(Boolean).slice(-3).join(" | ").slice(0, 300));
}

// 切成 ≤SEG_SECONDS 的片段，返回文件路径数组。
function split(src, dir, total) {
  if (total == null || total <= SEG_SECONDS) return [src];
  const files = [];
  for (let i = 0, t = 0; t < total; i++, t += SEG_SECONDS) {
    const f = join(dir, `seg${i}.mp3`);
    const r = spawnSync("ffmpeg", ["-y", "-ss", String(t), "-t", String(SEG_SECONDS), "-i", src, "-c", "copy", f],
      { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
    if (r.status !== 0 || !existsSync(f)) break;
    let sz = 0; try { sz = statSync(f).size; } catch {}
    if (sz < 512) break;
    files.push(f);
  }
  return files.length ? files : [src];
}

// 一段音频 → 文本。跟 gen-tts 同一条路（Omni chat），只是方向反过来。
function transcribeOne(mp3, { key, model, prompt }) {
  const dir = mkdtempSync(join(tmpdir(), "uking-asr-"));
  const bf = join(dir, "body.json"), rf = join(dir, "resp.txt");
  const b64 = readFileSync(mp3).toString("base64");
  const ask = "把这段音频里说的话一字不差转写成文字。只输出转写结果本身，不要加任何解释、标题或说明。"
    + (prompt ? `\n补充背景（帮助你写对人名/术语）：${prompt}` : "");
  writeFileSync(bf, JSON.stringify({
    model, stream: true,
    messages: [{
      role: "user",
      content: [
        { type: "input_audio", input_audio: { data: `data:audio/mp3;base64,${b64}`, format: "mp3" } },
        { type: "text", text: ask },
      ],
    }],
  }));
  try {
    const r = spawnSync("curl",
      ["-sS", "-m", String(Math.ceil(REQ_TIMEOUT_MS / 1000)), "-X", "POST", CHAT_URL,
        "-H", `Authorization: Bearer ${key}`, "-H", "Content-Type: application/json",
        "--data", `@${bf}`, "-o", rf],
      { timeout: REQ_TIMEOUT_MS + 5000, encoding: "utf8" });
    if (r.error) throw r.error;
    // curl 非 0 退出时 -o 只留空文件 —— 必须看退出码，否则「没连上」会被误报成「转写为空」。
    if (r.status !== 0) {
      const err = String(r.stderr || "").trim().slice(0, 200);
      throw new Error(`转写服务连接失败（curl 退出码 ${r.status}）：${err || "无法连接 api.u-claw.org.cn，检查网络/代理后重试"}`);
    }
    const body = readFileSync(rf, "utf8");
    let text = "", errMsg = "";
    for (const line of body.split(/\r?\n/)) {
      if (!line.startsWith("data:")) continue;
      const payload = line.slice(5).trim();
      if (!payload || payload === "[DONE]") continue;
      let j = null;
      try { j = JSON.parse(payload); } catch { continue; }
      if (j && j.error) { errMsg = j.error.message || JSON.stringify(j.error); continue; }
      const c = j && j.choices && j.choices[0] && j.choices[0].delta && j.choices[0].delta.content;
      if (typeof c === "string") text += c;
    }
    if (!text.trim()) {
      if (errMsg) throw new Error("转写失败：" + errMsg.slice(0, 300));
      throw new Error("转写失败：响应里没有文本 —— " + body.replace(/\s+/g, " ").slice(0, 250));
    }
    return text.trim();
  } finally { rmSync(dir, { recursive: true, force: true }); }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;

  const inRaw = (typeof args.in === "string" && args.in) || (typeof args.input === "string" && args.input) || args._[0];
  if (!inRaw) fail("缺少输入（--in 录音.mp3 / 视频.mp4）", 2);
  const src = normalizePath(inRaw);
  if (!existsSync(src)) fail(`找不到输入文件：${src}`, 2);

  const key = resolveKey(args);
  if (!key) fail("找不到 API Key（--key / XIAPAN_API_KEY / ~/.uking/device.json）", 2);
  const model = (typeof args.model === "string" && args.model) || DEFAULT_MODEL;
  const prompt = typeof args.prompt === "string" ? args.prompt : "";
  if (!has("curl")) fail("转写需要系统 curl（Win10+ 自带）。", 1);
  if (!has("ffmpeg")) fail("转写需要 ffmpeg（抽音轨/切段）。Windows：`winget install Gyan.FFmpeg`；Mac：`brew install ffmpeg`。", 1);

  const t0 = Date.now();
  const work = mkdtempSync(join(tmpdir(), "uking-asr-work-"));
  // 🔴 done() 里是 process.exit()，**finally 不会执行** —— 所以结果先算出来，
  // 清理放 finally，done() 留到最外面调。以前在 try 里直接 done() 会漏一堆临时目录。
  let result = null, errored = null;
  let segments = 0, text = "";
  try {
    // 统一先转成 16k 单声道 mp3：视频要抽音轨，音频也要压体积（base64 会再胀 1/3）。
    const norm = join(work, "audio.mp3");
    const ext = extname(src).toLowerCase();
    logE(AUDIO_OK.has(ext) ? "转码音频…" : "从视频抽音轨…");
    toMp3(src, norm);

    const total = durationOf(norm);
    const segs = split(norm, work, total);
    segments = segs.length;
    logE(`转写：模型 ${model}，时长 ${total == null ? "?" : total.toFixed(1) + "s"}${segs.length > 1 ? `（分 ${segs.length} 段）` : ""}…`);

    const outs = [];
    for (let i = 0; i < segs.length; i++) {
      outs.push(transcribeOne(segs[i], { key, model, prompt }));
      if (segs.length > 1) logE(`  段 ${i + 1}/${segs.length} ✓`);
    }
    text = outs.join("\n");

    result = { ok: true, text, model, segments, chars: text.length, elapsed: ((Date.now() - t0) / 1000).toFixed(1) + "s" };
    if (typeof args.out === "string") {
      const out = normalizePath(args.out);
      try { mkdirSync(dirname(out), { recursive: true }); } catch {}
      writeFileSync(out, text, "utf8");
      result.file = out;
    }
  } catch (e) {
    errored = e;
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  if (errored) fail(errored);
  done(result);
}
main();
