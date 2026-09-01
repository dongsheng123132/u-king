#!/usr/bin/env node
// U-King AIGC · 文字转语音（TTS）—— 直连 POST /v1/audio/speech，零 npm 依赖。
// 请求体：{model:"minimax-speech-2.8-turbo",input,voice}；同步返回 MP3 字节。
// 计费字符：汉字×2、其他字符×1，单次最多 5000；超过即参数错误，绝不发请求。
// 成功响应必须是 MP3（ID3 或 0xFF FB/0xFF F3/0xFF F2）；即使 HTTP 200，JSON 错误体也算失败。
//
// 用法：
//   node scripts/gen-tts.mjs --text "你好，欢迎来到虾盘云" --voice Cherry --out hello.mp3 --json
//   node scripts/gen-tts.mjs --list-voices --json
//   echo "很长的旁白…" | node scripts/gen-tts.mjs --out narration.mp3
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, statSync, mkdtempSync, rmSync, mkdirSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";

const BASE = "https://api.u-claw.org.cn";
const SPEECH_URL = BASE + "/v1/audio/speech";
const DEFAULT_MODEL = "minimax-speech-2.8-turbo";
const DEFAULT_VOICE = "female-tianmei";
const MAX_BILLABLE_CHARS = 5000;
const GEN_TIMEOUT_MS = 180000;

// 兼容旧 gen-reel / 旧脚本调用。未识别音色一律回落到已生产验证的女声。
const VOICE_ALIASES = { Cherry: "female-tianmei", Marcus: "male-qn-qingse" };
const VERIFIED_VOICES = ["female-tianmei", "male-qn-qingse"];
// MiniMax 官方音色名；除上面两项外尚未在本通道逐一验过，不能向调用方承诺可用。
const UNVERIFIED_OFFICIAL_VOICES = ["female-shaonv", "female-yujie", "female-chengshu", "female-mengwa", "male-qn-jingying", "male-qn-badao", "male-qn-daxuesheng"];

const BOOL = new Set(["json", "quiet", "list-voices"]);
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
  else if (obj.ok) process.stdout.write((obj.file || obj.voices?.join(" ") || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) {
  const m = (msg && msg.message) || msg || "";
  const out = { ok: false, error: String(m) };
  for (const k of ["http_status", "error_type", "retriable", "charge_state"]) {
    if (msg && msg[k] !== undefined) out[k] = msg[k];
  }
  done(out, code);
}

function normalizeOut(p) {
  let s = String(p);
  const m = s.match(/^\/([A-Za-z])\/(.*)$/);
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  const abs = resolve(s);
  try { mkdirSync(dirname(abs), { recursive: true }); } catch {}
  return abs;
}
function hasCurl() {
  try { return spawnSync("curl", ["--version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}
function isMp3(buf) {
  return buf.length >= 3 && (buf.subarray(0, 3).equals(Buffer.from("ID3"))
    || (buf[0] === 0xff && (buf[1] === 0xfb || buf[1] === 0xf3 || buf[1] === 0xf2)));
}
function errorFromBody(buf) {
  const text = buf.toString("utf8").trim();
  try {
    const j = JSON.parse(text);
    return j?.error?.message || j?.message || JSON.stringify(j);
  } catch { return text.replace(/\s+/g, " ").slice(0, 280) || "响应不是 MP3"; }
}
function billableChars(text) {
  let count = 0;
  for (const ch of text) count += /[\u3400-\u4DBF\u4E00-\u9FFF]/u.test(ch) ? 2 : 1;
  return count;
}
function durationOf(f) {
  const r = spawnSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f], { encoding: "utf8" });
  if (r.status !== 0) return null;
  const d = parseFloat(String(r.stdout).trim());
  return Number.isFinite(d) ? Math.round(d * 100) / 100 : null;
}

function synthesize({ key, model, input, voice, out }) {
  const dir = mkdtempSync(join(tmpdir(), "uking-tts-"));
  const body = join(dir, "body.json");
  try {
    writeFileSync(body, JSON.stringify({ model, input, voice }));
    const r = spawnSync("curl", ["-sS", "-L", "-m", String(Math.ceil(GEN_TIMEOUT_MS / 1000)),
      "-X", "POST", SPEECH_URL, "-H", `Authorization: Bearer ${key}`, "-H", "Content-Type: application/json",
      "--data", `@${body}`, "-o", out, "-w", "%{http_code}\n%{content_type}"],
    { timeout: GEN_TIMEOUT_MS + 5000, encoding: "utf8" });
    if (r.error) { const e = new Error(`语音服务进程异常：${r.error.message || r.error}`); e.error_type = "network"; e.retriable = true; e.charge_state = "unknown"; throw e; }
    const [status = "", contentType = ""] = String(r.stdout || "").trim().split(/\r?\n/);
    if (r.status !== 0) {
      const code = r.status;
      const e = new Error(`语音服务连接失败（curl 退出码 ${code}）：${String(r.stderr || "").trim().slice(0, 240)}`);
      e.http_status = 0;
      if ([5, 6, 7, 35].includes(code)) { e.error_type = "network"; e.retriable = true; e.charge_state = "not_charged"; }
      else if ([28, 52, 56].includes(code)) { e.error_type = "unknown"; e.retriable = false; e.charge_state = "unknown"; }
      else { e.error_type = "network"; e.retriable = false; e.charge_state = "unknown"; }
      throw e;
    }
    let bytes = Buffer.alloc(0);
    try { bytes = readFileSync(out); } catch {}
    if (!/^2\d\d$/.test(status) || !isMp3(bytes)) {
      const body = errorFromBody(bytes);
      const e = new Error(`语音合成失败（HTTP ${status || "?"}，${contentType || "未知类型"}）：${body}`);
      e.http_status = parseInt(status, 10) || 0;
      const sc = e.http_status;
      if (sc === 400) { e.error_type = "bad_request"; e.retriable = false; e.charge_state = "not_charged"; }
      else if (sc === 401 || sc === 402 || sc === 403) { e.error_type = sc === 402 ? "quota" : "auth"; e.retriable = false; e.charge_state = "not_charged"; }
      else if (sc >= 500 || /已退款|自动退回/i.test(body)) { e.error_type = "upstream_error"; e.retriable = true; e.charge_state = "charged_refunded"; }
      else { e.error_type = "upstream_error"; e.retriable = false; e.charge_state = "unknown"; }
      throw e;
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;
  if (args["list-voices"]) {
    done({ ok: true, model: DEFAULT_MODEL, voices: VERIFIED_VOICES,
      aliases: VOICE_ALIASES, unverified_official_voices: UNVERIFIED_OFFICIAL_VOICES });
  }

  let text = typeof args.text === "string" ? args.text : typeof args.input === "string" ? args.input : args._.join(" ");
  if ((!text || !text.trim()) && !process.stdin.isTTY) {
    try { text = readFileSync(0, "utf8"); } catch {}
  }
  text = (text || "").trim();
  if (!text) fail("缺少要合成的文字（--text \"...\"，或管道 stdin）", 2);
  const charged = billableChars(text);
  if (charged > MAX_BILLABLE_CHARS)
    fail(`文本计费字符超限（${charged} > ${MAX_BILLABLE_CHARS}；汉字按 2、其他按 1 计）`, 2);

  const key = resolveKey(args);
  if (!key) fail("找不到 API Key（--key / XIAPAN_API_KEY / ~/.uking/device.json）", 2);
  if (!hasCurl()) fail("语音合成需要系统 curl（Win10+ 自带）。装好 curl 后重试。", 1);
  const model = (typeof args.model === "string" && args.model) || DEFAULT_MODEL;
  const requestedVoice = (typeof args.voice === "string" && args.voice) || DEFAULT_VOICE;
  const voice = VOICE_ALIASES[requestedVoice] || (VERIFIED_VOICES.includes(requestedVoice) ? requestedVoice : DEFAULT_VOICE);
  if (voice !== requestedVoice) logE(`音色 ${requestedVoice} 未验证或为旧名，已映射/回退为 ${voice}`);
  const out = normalizeOut((typeof args.out === "string" && args.out) || `./uking-tts-${Date.now()}.mp3`);

  const t0 = Date.now();
  try { synthesize({ key, model, input: text, voice, out }); } catch (e) { fail(e); }
  let bytes = 0; try { bytes = statSync(out).size; } catch {}
  if (bytes < 3) fail("语音合成结果异常（文件为空）");
  done({ ok: true, file: resolve(out), model, voice, chars: text.length, billable_chars: charged,
    bytes, duration: durationOf(out), elapsed: ((Date.now() - t0) / 1000).toFixed(1) + "s" });
}
main();
