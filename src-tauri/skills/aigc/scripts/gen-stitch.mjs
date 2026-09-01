#!/usr/bin/env node
// U-King AIGC · 视频拼接 —— 把多段视频按顺序拼成一条成片（多镜头 / 漫剧 / 短视频合成）。
// 需系统 ffmpeg（没有会给安装指引）。零 npm 依赖。--json 出 {ok,file,...}；退出码 0 成功 / 1 运行错 / 2 参数错。
//
// 用法：
//   node scripts/gen-stitch.mjs --in a.mp4 --in b.mp4 --in c.mp4 --out final.mp4 --json
//   node scripts/gen-stitch.mjs a.mp4 b.mp4 --out final.mp4 --resolution 1080p --audio bgm.mp3
//   # 漫剧：每段自带对白音轨，保留它们（而不是盖一条全局音轨）+ 烧中文字幕
//   node scripts/gen-stitch.mjs --in s1.mp4 --in s2.mp4 --keep-audio --subtitle lines.srt --out final.mp4
import { spawnSync } from "node:child_process";
import { existsSync, statSync, mkdtempSync, rmSync } from "node:fs";
import { resolve, join } from "node:path";
import { tmpdir } from "node:os";

// ── 参数解析（--in 可重复；布尔见 BOOL；位置参数也当输入文件）──
const BOOL = new Set(["json", "quiet", "keep-audio"]);
const REPEAT = new Set(["in"]);
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
  else if (obj.ok) process.stdout.write((obj.file || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) { done({ ok: false, error: String((msg && msg.message) || msg) }, code); }

function hasFfmpeg() {
  try { return spawnSync("ffmpeg", ["-version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}
// libass 是可选编译项。没有它 `subtitles` 滤镜根本不存在，硬跑只会得一句
// "No such filter" —— 提前探，给人话。
function hasSubtitleFilter() {
  const r = spawnSync("ffmpeg", ["-hide_banner", "-filters"], { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  return r.status === 0 && /^\s*\S+\s+subtitles\s/m.test(String(r.stdout || ""));
}
function hasAudioStream(f) {
  const r = spawnSync("ffprobe", ["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0", f],
    { encoding: "utf8" });
  return r.status === 0 && String(r.stdout).trim().length > 0;
}

// 分辨率关键字 → 宽x高（也接受直接的 WxH）。
function resolveSize(r) {
  const map = { "480p": "854x480", "720p": "1280x720", "1080p": "1920x1080" };
  const s = map[r] || (typeof r === "string" && /^\d+x\d+$/.test(r) ? r : "1280x720");
  const [w, h] = s.split("x").map(Number);
  return { w, h };
}

// 🔴 `subtitles=` 滤镜里的路径要过两层解析（filtergraph + libass）。Windows 的
// `C:\a\b.srt` 直接塞进去必炸：反斜杠被当转义、盘符冒号被当参数分隔符。
// 规矩：反斜杠→正斜杠，冒号转义成 `\:`，单引号也转义。
function escapeForFilter(p) {
  return String(p).replace(/\\/g, "/").replace(/:/g, "\\:").replace(/'/g, "\\'");
}

// 给没有音轨的片段补一条静音，让 concat 的 a=1 不至于因为「有的段没音轨」整条失败。
// AI 生成的片子经常无音轨；漫剧里也可能某镜没对白。
function ensureAudio(files, dir) {
  return files.map((f, i) => {
    if (hasAudioStream(f)) return f;
    const out = join(dir, `withaudio${i}.mp4`);
    const r = spawnSync("ffmpeg", ["-y", "-i", f, "-f", "lavfi", "-i", "anullsrc=r=44100:cl=stereo",
      "-c:v", "copy", "-c:a", "aac", "-b:a", "128k", "-shortest", out],
      { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
    if (r.status !== 0 || !existsSync(out)) {
      logE(`  ⚠ 段 ${i + 1} 补静音失败，按原样使用`);
      return f;
    }
    return out;
  });
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;

  const inputs0 = [...(args.in || []), ...args._].filter((x) => typeof x === "string");
  if (inputs0.length < 1) fail("至少要 1 段视频（--in a.mp4 --in b.mp4 …，或直接列文件名）", 2);
  for (const f of inputs0) if (!existsSync(f)) fail(`找不到输入文件：${f}`, 2);
  if (!hasFfmpeg())
    fail("视频拼接需要系统 ffmpeg（未检测到）。Windows 装法：`winget install Gyan.FFmpeg`；或到 https://www.gyan.dev/ffmpeg/builds/ 下载解压后把 bin 加进 PATH。装好后重开终端再试。", 1);

  const out = (typeof args.out === "string" && args.out) || `./uking-stitch-${Date.now()}.mp4`;
  const { w, h } = resolveSize(args.resolution);
  const fps = Math.max(1, Math.min(60, parseInt(args.fps, 10) || 30));
  const audio = typeof args.audio === "string" ? args.audio : "";
  if (audio && !existsSync(audio)) fail(`找不到背景音频：${audio}`, 2);
  const keepAudio = !!args["keep-audio"];
  if (keepAudio && audio)
    fail("--keep-audio 和 --audio 互斥：前者保留每段自带音轨（漫剧对白），后者盖一条全局音轨（旁白/BGM）。", 2);

  const srt = typeof args.subtitle === "string" ? args.subtitle : "";
  if (srt && !existsSync(srt)) fail(`找不到字幕文件：${srt}`, 2);
  if (srt && !hasSubtitleFilter())
    fail("这份 ffmpeg 没编 libass，烧不了字幕（`ffmpeg -filters` 里查不到 subtitles）。换官方完整版：Windows `winget install Gyan.FFmpeg`；Mac `brew install ffmpeg`。或先去掉 --subtitle 出无字幕成片。", 1);
  const font = (typeof args.font === "string" && args.font) || "Microsoft YaHei";
  const fontSize = Math.max(8, Math.min(96, parseInt(args["font-size"], 10) || Math.round(h / 22)));

  const work = mkdtempSync(join(tmpdir(), "uking-stitch-"));
  let result = null, errored = null;
  try {
    const inputs = keepAudio ? ensureAudio(inputs0, work) : inputs0;

    // 逐段缩放+补边到统一 WxH、统一 fps（各段尺寸/帧率不同也能拼）。
    // 音轨：--keep-audio 时每段重采样到统一参数再一起 concat；否则只拼视频（a=0），
    // 各段自带音轨忽略 —— 配音/BGM 用 --audio 单独盖一条。
    const vParts = inputs.map((_, i) =>
      `[${i}:v]scale=${w}:${h}:force_original_aspect_ratio=decrease,pad=${w}:${h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps=${fps}[v${i}]`
    );
    const aParts = keepAudio
      ? inputs.map((_, i) => `[${i}:a]aresample=44100,aformat=sample_fmts=fltp:channel_layouts=stereo[a${i}]`)
      : [];
    const concatIn = inputs.map((_, i) => (keepAudio ? `[v${i}][a${i}]` : `[v${i}]`)).join("");
    const concatOut = keepAudio ? "[vcat][acat]" : "[vcat]";
    const chain = [...vParts, ...aParts,
      `${concatIn}concat=n=${inputs.length}:v=1:a=${keepAudio ? 1 : 0}${concatOut}`];

    // 字幕接在 concat 之后（整片一条时间轴），不是逐段烧。
    let vLabel = "[vcat]";
    if (srt) {
      chain.push(`[vcat]subtitles='${escapeForFilter(resolve(srt))}':force_style='FontName=${font},FontSize=${fontSize},PrimaryColour=&H00FFFFFF,OutlineColour=&H80000000,BorderStyle=3,Outline=1,Shadow=0,MarginV=${Math.round(h / 18)}'[vsub]`);
      vLabel = "[vsub]";
    }

    const cmd = ["-y"];
    for (const f of inputs) cmd.push("-i", f);
    if (audio) cmd.push("-i", audio);
    cmd.push("-filter_complex", chain.join(";"), "-map", vLabel);
    if (keepAudio) cmd.push("-map", "[acat]", "-c:a", "aac", "-b:a", "192k");
    else if (audio) cmd.push("-map", `${inputs.length}:a`, "-shortest", "-c:a", "aac", "-b:a", "192k");
    cmd.push("-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "veryfast", "-crf", "20", out);

    logE(`拼接 ${inputs.length} 段 → ${w}x${h}@${fps}fps${keepAudio ? "，保留各段音轨" : audio ? "，配音 " + audio : ""}${srt ? "，烧字幕" : ""}…`);
    const t0 = Date.now();
    const r = spawnSync("ffmpeg", cmd, { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
    if (r.error) throw new Error("ffmpeg 启动失败：" + r.error.message);
    let sz = 0; try { sz = statSync(out).size; } catch {}
    if (r.status !== 0 || sz < 1024)
      throw new Error("拼接失败：" + String(r.stderr || "").split(/\r?\n/).filter(Boolean).slice(-4).join(" | ").slice(0, 500));

    result = {
      ok: true, file: resolve(out), inputs: inputs.length, resolution: `${w}x${h}`,
      keep_audio: keepAudio, subtitle: srt ? resolve(srt) : null,
      elapsed: ((Date.now() - t0) / 1000).toFixed(1) + "s",
    };
  } catch (e) {
    errored = e;
  } finally {
    // done() 里是 process.exit()，finally 不会执行 —— 清理必须在调 done 之前。
    rmSync(work, { recursive: true, force: true });
  }
  if (errored) fail(errored);
  done(result);
}
main();
