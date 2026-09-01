#!/usr/bin/env node
// U-King AIGC · 文生图 / 图生图 —— 直连虾盘云作图。零 npm 依赖（仅 node 内置 + 系统 curl）。
// 用法见同目录 ../SKILL.md。输出：--json 出 {ok,file,...}；退出码 0 成功 / 1 运行错 / 2 参数错。
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, statSync, mkdtempSync, rmSync, mkdirSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import https from "node:https";

const BASE = "https://api.u-claw.org.cn";
// 同步出图慢（文字多的海报上游要画 2~4 分钟），与后端 providers.rs::IMAGE_GEN_TIMEOUT_S 对齐到 600s。
const GEN_TIMEOUT_MS = 600000;

// ── 参数解析（--flag value；--json/--quiet 为布尔；--ref/--image 可重复成数组）──
const BOOL = new Set(["json", "quiet"]);
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

// ── 输出路径归一化：弱模型常给 git-bash 风格 `/c/Users/...`（node 会错解成 C:\c\Users），
// 或指向还不存在的子目录 → 转成 Windows 绝对路径 + 自动建父目录，挡掉 ENOENT/路径穿帮。
function normalizeOut(p) {
  let s = String(p);
  const m = s.match(/^\/([A-Za-z])\/(.*)$/); // /c/Users/... → C:\Users\...
  if (m) s = m[1].toUpperCase() + ":\\" + m[2].replace(/\//g, "\\");
  const abs = resolve(s);
  try { mkdirSync(dirname(abs), { recursive: true }); } catch {}
  return abs;
}

let QUIET = false, JSONMODE = false;
function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
// stdout 只出结果（--json 出契约 JSON，否则出文件路径）；错误走 stderr + 退出码。
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
function httpsRequest(method, url, { auth, json } = {}, timeoutMs = 185000) {
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
// 下载二进制（结果只回 url 时）→ 落盘 + 校验。域名 .org→.org.cn，--ssl-no-revoke 防 CDN 吊销坑（对齐后端 ensure_b64）。
function download(url, key, outPath, timeoutMs) {
  if (!CURL) throw new Error("下载图片需要系统 curl（本机无 curl）。请改用 gpt-image-2（直接回图、无需下载），或按 SKILL.md 手动下载。");
  const u = url.replace("://api.u-claw.org/", "://api.u-claw.org.cn/");
  const args = ["-sS", "-m", String(Math.ceil(timeoutMs / 1000)), "-L", "--ssl-no-revoke"];
  if (key) args.push("-H", `Authorization: Bearer ${key}`);
  args.push("-o", outPath, u);
  const r = spawnSync("curl", args, { timeout: timeoutMs + 5000 });
  if (r.error) throw r.error;
  let sz = 0; try { sz = statSync(outPath).size; } catch {}
  if (r.status !== 0 || sz < 512) throw new Error("图片下载失败或文件过小");
}
// 抠出上游错误文案（兼容 {error:{message}} 与 {code,message}）。
function errOf(v) {
  if (v && v.error != null) return v.error.message || JSON.stringify(v.error);
  if (v && typeof v.code === "string" && v.code !== "success" && v.code !== "") return v.message || v.code;
  return null;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;
  const prompt = typeof args.prompt === "string" ? args.prompt : "";
  if (!prompt) fail("缺少 --prompt（要画的内容）", 2);
  const model = (typeof args.model === "string" && args.model) || "gpt-image-2";
  const size = (typeof args.size === "string" && args.size) || "1024x1024";
  // --n：一次出几张（文生图，1~4；gpt-image-2 已实测支持）。图生图 edits 固定 1 张。
  const n = Math.max(1, Math.min(4, parseInt(args.n, 10) || 1));
  // --quality：low/medium/high/auto（gpt-image-2 已实测生效；low 最省最快，high 最精细）。
  const quality = typeof args.quality === "string" && ["low", "medium", "high", "auto"].includes(args.quality) ? args.quality : "";
  const refs = [...(args.ref || []), ...(args.image || [])].filter((x) => typeof x === "string");
  const out = normalizeOut((typeof args.out === "string" && args.out) || `./uking-image-${Date.now()}.png`);
  const key = resolveKey(args);
  if (!key) fail("找不到 API Key（--key / 环境变量 XIAPAN_API_KEY / ~/.uking/device.json）", 2);

  const t0 = Date.now();
  let resp;
  if (refs.length) {
    // 图生图：multipart edits（必须 curl）。prompt 落临时文件用 -F prompt=<file，避中文/特殊字符。
    // 注：edits 固定 1 张（多图输出未验证）；--n 只作用于文生图。
    if (!CURL) fail("图生图需要系统 curl（multipart 上传），本机未检测到 curl。请按 SKILL.md 的 curl 文档手调。", 1);
    if (n > 1) logE("提示：--n 只对文生图有效，图生图仍出 1 张。");
    logE(`图生图：${refs.length} 张参考图，模型 ${model}${quality ? "，质量 " + quality : ""}…`);
    const dir = mkdtempSync(join(tmpdir(), "uking-edit-"));
    const pf = join(dir, "prompt.txt");
    writeFileSync(pf, prompt);
    const a = ["-sS", "-m", String(Math.ceil(GEN_TIMEOUT_MS / 1000)), "-X", "POST", BASE + "/v1/images/edits", "-H", `Authorization: Bearer ${key}`,
      "-F", `model=${model}`, "-F", `prompt=<${pf}`, "-F", "n=1", "-F", `size=${size}`];
    if (quality) a.push("-F", `quality=${quality}`);
    const field = refs.length === 1 ? "image" : "image[]"; // 单图 image，多图 image[]（对齐后端）
    for (const p of refs) a.push("-F", `${field}=@${p}`); // ⚠️ 绝不传 response_format（gpt-image-2 edits 会 400）
    try { resp = parseJsonOr(curlText(a, GEN_TIMEOUT_MS + 5000), "图生图"); }
    finally { rmSync(dir, { recursive: true, force: true }); }
  } else {
    logE(`文生图：模型 ${model}，尺寸 ${size}${n > 1 ? "，" + n + " 张" : ""}${quality ? "，质量 " + quality : ""}…`);
    const body = { model, prompt, n, size, response_format: "b64_json" };
    if (quality) body.quality = quality;
    resp = await postJson("/v1/images/generations", key, body, GEN_TIMEOUT_MS);
  }

  const e = errOf(resp);
  if (e) fail(e);
  const items = (resp && Array.isArray(resp.data) ? resp.data : []).filter((x) => x && (x.b64_json || x.url));
  if (!items.length) fail("作图响应缺少 data：" + JSON.stringify(resp).slice(0, 200));

  // 多张时：第一张写 --out，其余在扩展名前插 -2/-3…（out.png → out-2.png）。
  const withSuffix = (p, i) => (i === 0 ? p : p.replace(/(\.[^./\\]+)?$/, (m) => `-${i + 1}${m || ""}`));
  const files = [];
  for (let i = 0; i < items.length; i++) {
    const p = withSuffix(out, i);
    if (items[i].b64_json) writeFileSync(p, Buffer.from(items[i].b64_json, "base64"));
    else download(items[i].url, "", p, 60000); // 只回 url（seedream/wanx）则下载转存
    files.push(resolve(p));
  }

  done({
    ok: true, file: files[0], files, count: files.length, model,
    revised_prompt: items[0].revised_prompt || null,
    elapsed: ((Date.now() - t0) / 1000).toFixed(1) + "s",
  });
}
main().catch((err) => fail(err));
