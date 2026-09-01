#!/usr/bin/env node
// U-King AIGC · gen-reel —— 一键成片编排器（短视频 / **漫剧** / 宣传片）。
// 分镜 → 逐镜出图 → 图生视频 → 对白配音 → 字幕 → 拼接成片。**确定性编排（Node 控制流程，
// 不依赖大模型照做多步骤）**，复用同目录 gen-image/gen-video/gen-tts/gen-stitch。零 npm 依赖。
//
// 用法：
//   # ① 内联分镜（--shot 可重复，格式 "画面::怎么动"，:: 后可省=用画面当运动）
//   node gen-reel.mjs --shot "赛博朋克城市夜景::镜头缓慢推进" --shot "发光的AI核心::核心旋转" \
//     --bgm-prompt "轻快温馨钢琴曲" --resolution 720p --out reel.mp4 --json
//   # ② 分镜脚本文件（漫剧走这个 —— 多角色对白必须用文件）
//   node gen-reel.mjs --storyboard sb.json --out reel.mp4 --json
//   # ③ 整段旁白（没有分角色对白时用）
//   node gen-reel.mjs --shot "..." --narration "欢迎来到未来之城……" --voice Cherry --out reel.mp4 --json
//
// storyboard.json（漫剧完整形态）：
// {
//   "style": "赛璐璐动画风，高饱和",
//   "resolution": "720p",
//   "subtitle": true,                                   // 烧字幕（对白/旁白自动生成时间轴）
//   "characters": {                                     // 角色表：一处定义，全片复用
//     "小雨": { "ref": "assets/xiaoyu.png", "voice": "Cherry" },
//     "老陈": { "ref": "assets/laochen.png", "voice": "Marcus" }
//   },
//   "shots": [
//     { "image": "老旧公寓走廊，昏黄灯光",
//       "motion": "镜头缓慢前推",
//       "cast": ["小雨"],                               // 本镜出场角色 → 自动带上他们的参考图
//       "lines": [ {"speaker":"小雨","text":"这扇门后面……到底是什么？"} ] }
//   ]
// }
//
// 🔴 **镜头长度由对白音频决定**，不是固定 5 秒。以前 `--duration 5` 写死，对白 8 秒就被硬切掉半句。
//    现在：先合成对白 → 量出时长 → 反推该镜时长（钳在 5~15s，上游的硬区间）。
// 🔴 **字幕来自剧本，不来自 ASR**。每句话是什么、配音多长本来就是已知的，从 ASR 反推
//    是把确定的事变回不确定。gen-asr 的位置是「转写别人给的音视频」，不是这里。
import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, statSync, mkdirSync, rmSync, copyFileSync, appendFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const NODE = process.execPath; // 用当前 node（便携版）去跑兄弟脚本

// 上游对单镜时长的硬区间。超出会被服务端钳，钳完时间轴就和字幕对不上 —— 所以我们自己先钳。
const MIN_SHOT = 5, MAX_SHOT = 15;
// 镜头数上限：漫剧一集常 20~40 镜。给上限是防手滑烧额度，不是技术限制。
const MAX_SHOTS = 40;
// 句与句之间留白（秒），不留会听起来像连读。
const LINE_GAP = 0.25;

// ── 参数解析（--flag value；布尔见 BOOL；--shot 可重复）──
const BOOL = new Set(["json", "quiet", "keep", "i2v", "subtitle", "no-subtitle", "dry-run"]);
const REPEAT = new Set(["shot"]);
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

let QUIET = false, JSONMODE = false, PROGRESS = "";
function logE(...m) {
  const s = m.join(" ");
  if (!QUIET) process.stderr.write(s + "\n");
  if (PROGRESS) { try { appendFileSync(PROGRESS, s + "\n"); } catch {} } // 进度落文件，便于无头监控
}
function done(obj, code = 0) {
  if (JSONMODE) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (obj.ok) process.stdout.write((obj.file || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) { done({ ok: false, error: String((msg && msg.message) || msg) }, code); }

// 归一化输出路径：/c/Users→C:\Users、自动建父目录（挡弱模型给的烂路径 + ENOENT）
function normalizeOut(p) {
  let s = String(p);
  const m = s.match(/^\/([A-Za-z])\/(.*)$/);
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  const abs = resolve(s);
  try { mkdirSync(dirname(abs), { recursive: true }); } catch {}
  return abs;
}

// 跑一个兄弟脚本，解析它 --json 的最后一行 JSON。总是 resolve（失败也给 {ok:false}）。
function runScript(script, args) {
  return new Promise((res) => {
    const p = spawn(NODE, [join(HERE, script), ...args, "--json"], { windowsHide: true });
    let out = "", err = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (err += d));
    p.on("close", (code) => {
      let j = null;
      const last = out.trim().split(/\r?\n/).filter(Boolean).pop() || "";
      try { j = JSON.parse(last); } catch {}
      if (j && typeof j.ok === "boolean") res(j);
      else res({ ok: false, error: (err.trim().slice(-180) || ("退出码 " + code)) });
    });
    p.on("error", (e) => res({ ok: false, error: String(e.message || e) }));
  });
}
let KEY_ARGS = [];
// 配音重试（R5 会审裁定）：仅当 gen-tts 标记 retriable=true 时重试，同参数不换音色；
// 最多 2 次重试（共 3 次尝试），退避 1s/3s。400/401/402/未知结果一律不重试。
function sleepMs(ms) { return new Promise((r) => setTimeout(r, ms)); }
async function runTtsWithRetry(text, voice, out) {
  let last = null;
  for (let attempt = 0; attempt <= 2; attempt++) {
    if (attempt > 0) logE(`  配音重试 ${attempt}/2（${["", "1s 后", "3s 后"][attempt]}）…`);
    const r = await runScript("gen-tts.mjs", ["--text", text, "--voice", voice, "--out", out, ...KEY_ARGS]);
    if (r.ok) return r;
    last = r;
    if (r.retriable !== true) break; // 明确不可重试或未知结果（可能已扣费）→ 不重试
    if (attempt < 2) await sleepMs(attempt === 0 ? 1000 : 3000);
  }
  return last;
}

// 简单并发池：items 过 worker，最多 n 个同时跑。
async function pool(items, n, worker) {
  const results = new Array(items.length);
  let idx = 0;
  async function run() { while (idx < items.length) { const i = idx++; results[i] = await worker(items[i], i); } }
  await Promise.all(Array.from({ length: Math.max(1, Math.min(n, items.length)) }, run));
  return results;
}

function ff(args, label) {
  const r = spawnSync("ffmpeg", ["-y", ...args], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
  if (r.status !== 0)
    throw new Error(`${label} 失败：` + String(r.stderr || "").split(/\r?\n/).filter(Boolean).slice(-3).join(" | ").slice(0, 300));
}
// 只在请求 BGM 且已有对白/旁白时调用。gen-stitch 的 --audio 是覆盖单轨，不能拿来
// 混音；这里先保留既有声音，再把循环 BGM 压到 0.3，避免盖住 TTS（1.0）。
function mixBgm(video, bgm, out) {
  // 只循环 BGM；视频必须只进一次（循环视频=无终态流，-shortest 失效会无限输出）。
  // -metadata 注入 AI 生成隐式标识（R5 合规分层：传输层 X-AI-* 头 + 资产层 comment 元数据；
  // 显式 drawtext 角标需重编码，与 -c:v copy 冲突，留 M1 客户端烘焙）。
  const tag = `AI-generated | uclaw | ${out.split(/[\\/\\\\]/).pop() || String(Date.now())}`;
  ff(["-i", video, "-stream_loop", "-1", "-i", bgm,
    "-filter_complex", "[0:a]volume=1.0[voice];[1:a]volume=0.3[music];[voice][music]amix=inputs=2:duration=first:dropout_transition=0[aout]",
    "-map", "0:v", "-map", "[aout]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
    "-metadata", `comment=${tag}`, "-shortest", out], "混入背景音乐");
}
function durationOf(f) {
  const r = spawnSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f], { encoding: "utf8" });
  if (r.status !== 0) return null;
  const d = parseFloat(String(r.stdout).trim());
  return Number.isFinite(d) ? d : null;
}
function hasBin(bin) {
  try { return spawnSync(bin, ["-version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}

// 秒 → SRT 时间戳 00:00:01,250
function srtTime(sec) {
  const s = Math.max(0, sec);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), ss = Math.floor(s % 60);
  const ms = Math.round((s - Math.floor(s)) * 1000);
  const p2 = (n) => String(n).padStart(2, "0");
  return `${p2(h)}:${p2(m)}:${p2(ss)},${String(ms).padStart(3, "0")}`;
}

function loadShots(args) {
  let cfg = {};
  if (typeof args.storyboard === "string") {
    const p = normalizeOut(args.storyboard);
    if (!existsSync(p)) fail(`找不到分镜脚本：${p}`, 2);
    try { cfg = JSON.parse(readFileSync(p, "utf8")); }
    catch (e) { fail(`分镜脚本不是合法 JSON：${e.message}`, 2); }
  }
  let shots = Array.isArray(cfg.shots) ? cfg.shots.slice() : [];
  if (Array.isArray(args.shot)) {
    for (const s of args.shot) {
      const [image, motion] = String(s).split("::");
      shots.push({ image: (image || "").trim(), motion: (motion || "").trim() });
    }
  }
  shots = shots.filter((s) => s && String(s.image || "").trim());
  return { cfg, shots };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;
  PROGRESS = typeof args.progress === "string" ? args.progress : "";

  const { cfg, shots } = loadShots(args);
  if (!shots.length) fail("没有分镜（--shot \"画面::运动\" 可重复，或 --storyboard sb.json）", 2);
  if (shots.length > MAX_SHOTS) fail(`镜头太多（${shots.length} > 上限 ${MAX_SHOTS}，避免手滑烧额度）`, 2);

  const style = (typeof args.style === "string" && args.style) || cfg.style || "";
  const imageModel = (typeof args["image-model"] === "string" && args["image-model"]) || cfg.image_model || "gpt-image-2";
  const videoModel = (typeof args["video-model"] === "string" && args["video-model"]) || cfg.video_model || "doubao-seedance-2-0-fast-260128";
  const resolution = (typeof args.resolution === "string" && args.resolution) || cfg.resolution || "720p";
  const baseDuration = String(args.duration || cfg.duration || MIN_SHOT);
  const size = (typeof args.size === "string" && args.size) || cfg.size || "1536x1024";
  const quality = (typeof args.quality === "string" && args.quality) || cfg.quality || "high";
  const globalRef = (typeof args.ref === "string" && args.ref) || cfg.ref || "";
  const narration = (typeof args.narration === "string" && args.narration) || cfg.narration || "";
  const voice = (typeof args.voice === "string" && args.voice) || cfg.voice || "Cherry";
  // --bgm 是既有的本地音频入口；--bgm-prompt 新增为一键生成入口。
  let bgm = (typeof args.bgm === "string" && args.bgm) || cfg.bgm || "";
  const bgmPrompt = (typeof args["bgm-prompt"] === "string" && args["bgm-prompt"]) || cfg.bgm_prompt || "";
  const concurrency = Math.max(1, Math.min(4, parseInt(args.concurrency, 10) || 2));
  const out = normalizeOut((typeof args.out === "string" && args.out) || cfg.out || `./uking-reel-${Date.now()}.mp4`);
  const keyArgs = typeof args.key === "string" ? ["--key", args.key] : [];
  KEY_ARGS = keyArgs;
  const allowSilentLines = !!args["allow-silent-lines"];
  let degraded = false;
  const warnings = [];

  // 角色表：{名字: {ref, voice}}。有对白的镜头靠它决定「用谁的脸、用谁的声音」。
  const characters = (cfg.characters && typeof cfg.characters === "object") ? cfg.characters : {};
  const hasDialogue = shots.some((s) => Array.isArray(s.lines) && s.lines.length);
  const dryRun = !!args["dry-run"];
  // 字幕默认：有对白/旁白就开（漫剧不带字幕不能发），--no-subtitle 可关。
  let wantSub = args["no-subtitle"] ? false
    : (args.subtitle || cfg.subtitle !== undefined ? (args.subtitle || cfg.subtitle) : (hasDialogue || !!narration));

  if (hasDialogue && !hasBin("ffmpeg"))
    fail("对白漫剧需要 ffmpeg（配音混流/字幕）。Windows：`winget install Gyan.FFmpeg`；Mac：`brew install ffmpeg`。", 1);

  // 校验角色表：对白里点名了但角色表里没有 → 早报错，别等出完图才炸。
  for (const [i, s] of shots.entries()) {
    for (const ln of (s.lines || [])) {
      if (ln.speaker && !characters[ln.speaker])
        fail(`镜 ${i + 1} 的对白点名了角色「${ln.speaker}」，但 storyboard.characters 里没有它。要么补上（可只写 voice），要么去掉 speaker。`, 2);
    }
  }

  const work = normalizeOut((typeof args.work === "string" && args.work) || join(tmpdir(), `uking-reel-${Date.now()}`));
  mkdirSync(work, { recursive: true });
  const t0 = Date.now();
  const persistent = typeof args.work === "string";
  const cleanup = () => { if (!args.keep && !persistent) { try { rmSync(work, { recursive: true, force: true }); } catch {} } };

  try {
    // ── Stage 0：先配音 ──
    // 顺序很关键：**对白决定镜长，镜长决定出视频的参数**，所以配音必须在出视频之前。
    // 以前是「出完视频再配音」，那时候镜长已经写死了，对白长了只能被切。
    const shotAudio = new Array(shots.length).fill(null);   // {file, dur, lines:[{text,start,dur}]}
    if (hasDialogue) {
      logE(`【1/5】合成对白（${shots.reduce((n, s) => n + (s.lines || []).length, 0)} 句）…`);
      for (const [i, s] of shots.entries()) {
        const lines = s.lines || [];
        if (!lines.length) continue;
        const clips = [];
        for (const [k, ln] of lines.entries()) {
          const v = (characters[ln.speaker] && characters[ln.speaker].voice) || voice;
          const mp3 = join(work, `s${i + 1}-line${k + 1}.mp3`);
          const r = await runTtsWithRetry(String(ln.text || ""), v, mp3);
          if (!r.ok) {
            if (allowSilentLines) { logE(`  镜${i + 1} 第${k + 1}句配音✗ ${r.error || ""}（--allow-silent-lines 静默降级）`); continue; }
            fail(`镜 ${i + 1} 第${k + 1}句配音失败（${r.error || ""}）。视频未生成，成本近 0——重试仍失败即硬停，避免无声半残品。确要静默降级加 --allow-silent-lines`, 1);
          }
          clips.push({ file: r.file || mp3, dur: r.duration || durationOf(r.file || mp3) || 0, text: String(ln.text || ""), speaker: ln.speaker || "" });
        }
        if (!clips.length) continue;
        // 拼成该镜整条音轨（句间插 LINE_GAP 静音），同时算出每句在镜内的起点 —— 字幕靠它。
        const listed = [];
        let cursor = 0;
        const parts = [];
        for (const [k, c] of clips.entries()) {
          if (k > 0) {
            const sil = join(work, `s${i + 1}-gap${k}.mp3`);
            ff(["-f", "lavfi", "-i", `anullsrc=r=24000:cl=mono`, "-t", String(LINE_GAP), "-c:a", "libmp3lame", "-q:a", "6", sil], "生成句间静音");
            parts.push(sil); cursor += LINE_GAP;
          }
          listed.push({ text: c.text, speaker: c.speaker, start: cursor, dur: c.dur });
          parts.push(c.file); cursor += c.dur;
        }
        const track = join(work, `s${i + 1}-voice.mp3`);
        const listf = join(work, `s${i + 1}-list.txt`);
        writeFileSync(listf, parts.map((f) => `file '${f.replace(/\\/g, "/").replace(/'/g, "'\\''")}'`).join("\n"));
        ff(["-f", "concat", "-safe", "0", "-i", listf, "-c:a", "libmp3lame", "-q:a", "4", track], "拼接对白音轨");
        shotAudio[i] = { file: track, dur: durationOf(track) || cursor, lines: listed };
        logE(`  镜${i + 1} 对白 ${clips.length} 句 / ${(shotAudio[i].dur).toFixed(1)}s ✓`);
      }
    }

    // ── Stage 1：逐镜出图（仅图生视频模式）──
    // 有 cast 的镜头隐含 i2v：要让角色长得一样，就必须先出图当首帧。
    const anyCast = shots.some((s) => Array.isArray(s.cast) && s.cast.length);
    const i2v = !!(args.i2v || globalRef || anyCast);
    let stills = shots.map((shot, i) => ({ i, shot, png: null }));

    if (dryRun) {
      logE(`【2/5】--dry-run：跳过出图`);
    } else if (i2v) {
      logE(`【2/5】出图 ${shots.length} 镜（${imageModel}，${size}，并发 ${concurrency}）…`);
      stills = (await pool(shots, concurrency, async (shot, i) => {
        const prompt = (style ? style + "，" : "") + shot.image;
        const png = join(work, `shot${i + 1}.png`);
        const a = ["--prompt", prompt, "--model", imageModel, "--size", size, "--out", png, ...keyArgs];
        if (quality) a.push("--quality", quality);
        // 本镜出场角色的参考图（可多张融合）；没有 cast 就退回全局 --ref。
        const refs = (shot.cast || []).map((n) => characters[n] && characters[n].ref).filter(Boolean);
        if (refs.length) for (const r of refs) a.push("--ref", r);
        else if (globalRef) a.push("--ref", globalRef);
        const r = await runScript("gen-image.mjs", a);
        if (r.ok) logE(`  镜${i + 1} 图✓`); else logE(`  镜${i + 1} 图✗ ${r.error || ""}`);
        return r.ok ? { i, shot, png: r.file || png } : null;
      })).filter(Boolean);
      if (!stills.length) fail("所有镜头出图都失败");
    } else {
      logE(`【2/5】文生视频模式（跳过出图，更快更稳；要图生视频加 --i2v 或给镜头写 cast）`);
    }

    // ── Stage 2：出视频 ──
    // 主档失败自动换模型再试。**兜底链必须跨厂商**：以前是 fast→mini，两个都是字节 Seedance
    // 走同一条火山 Ark 渠道 —— 那条渠道整体挂了时两个一起死，兜底等于没有。
    // 这里只能列出 `gen-video.mjs` 当前真实支持的统一异步协议模型。
    // Wan 需要 DashScope 专用 adapter；在 adapter 落地前把它伪装成 fallback 只会无谓重试。
    const DEFAULT_VIDEO_FALLBACKS = ["doubao-seedance-2-0-mini-260615"];
    const vfbRaw = (typeof args["video-fallback"] === "string" && args["video-fallback"]) || cfg.video_fallback || "";
    const vfbList = vfbRaw ? String(vfbRaw).split(",").map((s) => s.trim()).filter(Boolean) : DEFAULT_VIDEO_FALLBACKS;
    const videoModels = [videoModel];
    for (const m of vfbList) if (m && !videoModels.includes(m)) videoModels.push(m);

    // 每镜时长：有对白就由音频反推（+留白），否则用默认；一律钳进上游硬区间。
    const shotDur = shots.map((s, i) => {
      const explicit = s.duration != null ? Number(s.duration) : null;
      if (explicit) return Math.max(MIN_SHOT, Math.min(MAX_SHOT, Math.round(explicit)));
      const a = shotAudio[i];
      if (a && a.dur) return Math.max(MIN_SHOT, Math.min(MAX_SHOT, Math.ceil(a.dur + 0.6)));
      return Math.max(MIN_SHOT, Math.min(MAX_SHOT, Math.round(Number(baseDuration) || MIN_SHOT)));
    });
    for (const [i, a] of shotAudio.entries()) {
      if (a && a.dur > MAX_SHOT)
        logE(`  ⚠ 镜${i + 1} 对白 ${a.dur.toFixed(1)}s 超过单镜上限 ${MAX_SHOT}s —— 会被截断，建议把这镜拆成两镜`);
    }

    // --dry-run：不调视频接口，用本地色块顶上。**只花配音那点钱**，
    // 却能把「镜长对不对、字幕跟不跟得上、混流会不会炸」整条验完。
    // 这是「只校验不落地」的探法 —— 没有它，每验一次编排逻辑都要烧十几块视频钱。
    let clipsOverride = null;
    if (dryRun) {
      logE(`【3/5】--dry-run：用本地色块代替视频（时长 ${shotDur.join("/")}s）—— 只验编排，不烧视频额度`);
      const { w, h } = { "480p": { w: 854, h: 480 }, "720p": { w: 1280, h: 720 }, "1080p": { w: 1920, h: 1080 } }[resolution] || { w: 1280, h: 720 };
      const COLORS = ["navy", "darkgreen", "maroon", "purple", "teal", "olive"];
      clipsOverride = stills.map((item) => {
        const mp4 = join(work, `clip${item.i + 1}.mp4`);
        ff(["-f", "lavfi", "-i", `color=c=${COLORS[item.i % COLORS.length]}:s=${w}x${h}:d=${shotDur[item.i]}`,
          "-c:v", "libx264", "-pix_fmt", "yuv420p", "-r", "30", mp4], `镜${item.i + 1} 占位片`);
        return { i: item.i, mp4 };
      });
    } else {
      logE(`【3/5】${i2v ? "图生" : "文生"}视频 ${stills.length} 镜（${videoModels.join("→")}，${resolution}，时长 ${shotDur.join("/")}s）…`);
    }
    const clips = clipsOverride || await pool(stills, concurrency, async (item) => {
      const shot = item.shot;
      const mp4 = join(work, `clip${item.i + 1}.mp4`);
      const base = ["--duration", String(shotDur[item.i]), "--resolution", resolution, "--out", mp4, ...keyArgs];
      const promptArgs = (i2v && item.png)
        ? ["--prompt", String(shot.motion || shot.image).trim(), "--image", item.png]
        : ["--prompt", [style, shot.image, shot.motion].filter(Boolean).join("，")];
      let r = { ok: false };
      for (const m of videoModels) {
        r = await runScript("gen-video.mjs", [...promptArgs, "--model", m, ...base]);
        if (r.ok) { if (m !== videoModel) logE(`  镜${item.i + 1} 兜底 ${m} ✓`); else logE(`  镜${item.i + 1} 片✓`); break; }
        logE(`  镜${item.i + 1} ${m} ✗ ${r.error || ""}`);
      }
      return r.ok ? { i: item.i, mp4: r.file || mp4 } : null;
    });
    const okClips = clips.filter(Boolean).sort((a, b) => a.i - b.i);
    if (!okClips.length) fail("所有镜头出视频都失败");

    // ── Stage 3：把每镜的对白混进它自己的片段 + 按**实际**时长排字幕时间轴 ──
    // 🔴 用实际时长，不用请求时长：上游会把 duration 钳到它自己的档位，
    //    拿请求值排时间轴，字幕会整片越走越偏。
    let srtPath = "", subCount = 0;
    const srtItems = [];
    let timeline = 0;
    if (hasDialogue) {
      logE(`【4/5】混流对白 + 排字幕时间轴…`);
      for (const c of okClips) {
        const vdur = durationOf(c.mp4) || shotDur[c.i];
        const a = shotAudio[c.i];
        if (a) {
          const muxed = join(work, `clip${c.i + 1}-voiced.mp4`);
          // apad + shortest：对白比画面短就补静音到画面长度，比画面长就跟着画面截
          // （前面已按对白反推过镜长，正常不会长；超 15s 上限那种会在这里被截，上面已告警）。
          ff(["-i", c.mp4, "-i", a.file, "-filter_complex", "[1:a]apad[aout]",
            "-map", "0:v", "-map", "[aout]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest", muxed], `镜${c.i + 1} 混流`);
          c.mp4 = muxed;
          for (const ln of a.lines) {
            const st = timeline + ln.start, en = Math.min(timeline + vdur, st + ln.dur);
            if (en > st) srtItems.push({ start: st, end: en, text: (ln.speaker ? `${ln.speaker}：` : "") + ln.text });
          }
        }
        timeline += vdur;
      }
    } else if (narration && narration.trim()) {
      // 无分角色对白但有整段旁白：合成一条盖全片（老行为，保留）。
      logE(`【4/5】合成整段旁白（${voice}）…`);
      const mp3 = join(work, "narration.mp3");
      const r = await runTtsWithRetry(narration, voice, mp3);
      if (r.ok) {
        shotAudio.globalNarration = r.file || mp3;
        if (wantSub) {
          // 旁白没有逐句时间戳，整段按总时长均分到句 —— 粗，但比没有强，且不假装精确。
          const total = r.duration || durationOf(r.file || mp3) || 0;
          const sentences = narration.split(/(?<=[。！？!?\n])/).map((s) => s.trim()).filter(Boolean);
          const per = sentences.length ? total / sentences.length : 0;
          sentences.forEach((s, k) => srtItems.push({ start: k * per, end: (k + 1) * per, text: s }));
        }
      } else {
        logE(`  旁白✗（跳过，视频已生成不硬停）：${r.error || ""}`);
        degraded = true; warnings.push("narration_failed");
      }
    } else {
      logE(`【4/5】无对白/旁白，跳过配音`);
    }

    if (wantSub && srtItems.length) {
      srtPath = join(work, "lines.srt");
      writeFileSync(srtPath,
        srtItems.map((it, k) => `${k + 1}\n${srtTime(it.start)} --> ${srtTime(it.end)}\n${it.text}\n`).join("\n"), "utf8");
      subCount = srtItems.length;
    }

    // ── 可选 BGM：独立通道，不改图像/视频/对白的既有编排。 ──
    if (bgmPrompt) {
      logE("【BGM】生成背景音乐…");
      const bgmOut = join(work, "bgm.mp3");
      const r = await runScript("gen-bgm.mjs", ["--prompt", bgmPrompt, "--out", bgmOut, ...keyArgs]);
      if (!r.ok) fail("背景音乐生成失败：" + (r.error || "未知错误"));
      bgm = r.file || bgmOut;
      logE("  背景音乐 ✓");
    }

    // ── Stage 4：拼接成片 ──
    logE(`【5/5】拼接 ${okClips.length} 段成片${subCount ? `（含 ${subCount} 条字幕）` : ""}…`);
    let finalFile = out;
    const single = okClips.length === 1 && !srtPath && !hasDialogue && !shotAudio.globalNarration && !bgm;
    let stitched = true;
    // 有 TTS 时先完成原有拼接，再在 BGM 分支混音；无 TTS 时仍直接交给 gen-stitch 的 --audio。
    const needsBgmMix = !!bgm && (hasDialogue || !!shotAudio.globalNarration);
    const stitchOut = needsBgmMix ? join(work, "reel-with-voice.mp4") : out;
    if (single) {
      copyFileSync(okClips[0].mp4, out);
    } else {
      const a = [];
      for (const c of okClips) a.push("--in", c.mp4);
      a.push("--out", stitchOut, "--resolution", resolution);
      if (hasDialogue) a.push("--keep-audio");
      else if (shotAudio.globalNarration) a.push("--audio", shotAudio.globalNarration);
      else if (bgm) a.push("--audio", bgm);
      if (srtPath) a.push("--subtitle", srtPath);
      const r = await runScript("gen-stitch.mjs", a);
      if (!r.ok) {
        stitched = false;
        copyFileSync(okClips[0].mp4, out); // 拼接失败兜底：至少把第一段交付
        logE(`  拼接失败（${r.error || ""}），已用第一段兜底`);
      }
    }
    if (needsBgmMix && stitched) mixBgm(stitchOut, bgm, out);

    let bytes = 0; try { bytes = statSync(finalFile).size; } catch {}
    if (bytes < 1024) fail("成片文件异常（过小）");
    const res = {
      ok: true, file: resolve(finalFile), shots: shots.length, clips: okClips.length,
      image_model: imageModel, video_model: videoModel, resolution,
      dialogue_lines: srtItems.length, subtitles: subCount,
      duration: durationOf(finalFile), bytes,
      degraded, warnings,
      work: persistent || args.keep ? work : undefined,
      elapsed: Math.round((Date.now() - t0) / 1000) + "s",
    };
    cleanup();
    done(res);
  } catch (e) { cleanup(); fail(e); }
}

main().catch((e) => fail(e));
