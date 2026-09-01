#!/usr/bin/env node
// U-King AIGC · 列出虾盘云「当前可用」模型 —— 实时 GET /v1/models。
// 模型会更新（新增作图/视频/语音模型时），跑这个拿最新清单即可，**不必改技能包、不必发版**。
// 零 npm 依赖（node 内置 + 系统 curl 兜底）。--json 出 {ok,models,note}；退出码 0/1/2。
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import https from "node:https";

const BASE = "https://api.u-claw.org.cn";

// ── Key：--key > 环境变量 XIAPAN_API_KEY > ~/.uking/device.json（与 gen-*.mjs 一致）──
function resolveKey(argv) {
  const i = argv.indexOf("--key");
  if (i >= 0 && argv[i + 1]) return argv[i + 1];
  if (process.env.XIAPAN_API_KEY) return process.env.XIAPAN_API_KEY;
  try {
    const j = JSON.parse(readFileSync(join(homedir(), ".uking", "device.json"), "utf8"));
    if (j && j.key) return j.key;
  } catch {}
  return "";
}

function hasCurl() {
  try { return spawnSync("curl", ["--version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}
function getJson(path, key) {
  if (hasCurl()) {
    const r = spawnSync("curl", ["-sS", "-m", "40", BASE + path, "-H", `Authorization: Bearer ${key}`],
      { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
    if (r.status !== 0) throw new Error(`curl 退出码 ${r.status}：${String(r.stderr || "").slice(0, 160)}`);
    return JSON.parse(r.stdout || "");
  }
  return new Promise((res, rej) => {
    const u = new URL(BASE + path);
    https.request({ method: "GET", hostname: u.hostname, port: 443, path: u.pathname,
      headers: { Authorization: `Bearer ${key}` }, timeout: 40000 }, (r) => {
      const c = []; r.on("data", (x) => c.push(x));
      r.on("end", () => { try { res(JSON.parse(Buffer.concat(c).toString("utf8"))); } catch (e) { rej(e); } });
    }).on("error", rej).end();
  });
}

// 已验证/推荐模型的用法提示（其余只列 id，标「可试」）。
const KNOWN = {
  "gpt-image-2": "★作图默认 · 文生图/图生图 · gen-image.mjs（--n 1~4、--quality low/med/high/auto）",
  "doubao-seedance-2-0-mini-260615": "★视频默认 · 快·省 · 文生/图生视频 · gen-video.mjs",
  "doubao-seedance-2-0-fast-260128": "视频 · 质量/价格平衡档",
  "doubao-seedance-2-0-260128": "视频 · 最高清商用档",
  "gpt-4o-mini-tts": "★语音默认 · 文字转语音/配音 · gen-tts.mjs（音色 alloy/nova/onyx…）",
};

// 按 id 命名归类（虾盘云无每模型能力元数据，靠命名 + 已知表）。
function bucket(id) {
  const s = id.toLowerCase();
  if (/tts|cosyvoice|speech|-audio|qwen-tts|sovits|fish-audio|voice/.test(s)) return "tts";
  if (/whisper|\basr\b|sensevoice|paraformer|transcri/.test(s)) return "asr";
  if (/seedance|veo|dreamina|kling|sora|t2v|i2v|-video/.test(s)) return "video";
  if (/image|dall|seedream|gpt-image|flux|nano-banana|t2i|(gemini|wan[0-9x.]*)[-.]?.*image/.test(s)) return "image";
  return "chat";
}

async function main() {
  const argv = process.argv.slice(2);
  const JSONMODE = argv.includes("--json");
  const key = resolveKey(argv);
  if (!key) { process.stderr.write("错误：找不到 API Key（--key / XIAPAN_API_KEY / ~/.uking/device.json）\n"); process.exit(2); }

  let resp;
  try { resp = await getJson("/v1/models", key); }
  catch (e) { process.stderr.write("错误：拉取 /v1/models 失败：" + (e.message || e) + "\n"); process.exit(1); }

  const ids = (resp && Array.isArray(resp.data) ? resp.data : []).map((m) => m.id || m).filter(Boolean).sort();
  const groups = { image: [], video: [], tts: [], asr: [], chat: [] };
  for (const id of ids) groups[bucket(id)].push(id);

  if (JSONMODE) {
    process.stdout.write(JSON.stringify({
      ok: true,
      models: {
        image: groups.image, video: groups.video, tts: groups.tts, asr: groups.asr,
        chat_count: groups.chat.length,
      },
      note: groups.tts.length ? "有语音/TTS 模型可用，可加 gen-tts 用法" : "当前虾盘云暂无 TTS/语音模型（上了会自动出现在这里）",
    }) + "\n");
    process.exit(0);
  }

  const line = (id) => `  ${id}${KNOWN[id] ? "   — " + KNOWN[id] : "   （可试）"}`;
  const out = [];
  out.push(`虾盘云当前可用模型（实时 /v1/models，共 ${ids.length} 个）：`);
  out.push(`\n【作图 image · ${groups.image.length}】`); out.push(...groups.image.map(line));
  out.push(`\n【视频 video · ${groups.video.length}】`); out.push(...groups.video.map(line));
  out.push(`\n【语音 TTS · ${groups.tts.length}】`);
  out.push(groups.tts.length ? groups.tts.map(line).join("\n") : "  （暂无 —— 虾盘云上线 TTS 后会自动出现在这里）");
  if (groups.asr.length) { out.push(`\n【语音识别 ASR · ${groups.asr.length}】`); out.push(...groups.asr.map(line)); }
  out.push(`\n【对话/推理 chat · ${groups.chat.length}】（作图/视频用不到，略）`);
  out.push("\n提示：作图/视频请优先用带 ★ 的默认模型；其余「可试」的按需 --model 透传。");
  process.stdout.write(out.join("\n") + "\n");
}
main().catch((e) => { process.stderr.write("错误：" + (e.message || e) + "\n"); process.exit(1); });
