// 一搜商答 / 1so —— LLM 后端抽象（可插拔）。
//   provider = uking      → 调本机已装的 claude/codex（U-King 机器零服务器依赖）。
//   provider = openrouter → OpenRouter（一个 key 通全网模型，OpenAI 兼容）。key 读 OPENROUTER_API_KEY。
//   provider = openai     → 任意 OpenAI 兼容端点（如虾盘云文本 key）。key 读 device.json。
//   provider = bl         → 本机百炼 CLI（qwen3.6-plus），开发默认。
// 选择优先级：--provider > 环境 SO_PROVIDER > 默认 bl。
// 大段 prompt 走临时文件（bl）/请求体（openai/openrouter），不塞命令行，避开 Windows 长度限制。
import { spawnSync } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir, homedir } from "node:os";
import { readFileSync } from "node:fs";
import https from "node:https";
import { logE } from "./util.mjs";

const IS_WIN = process.platform === "win32";

export function resolveProvider(args) {
  return String(args.provider || process.env.SO_PROVIDER || "bl").toLowerCase();
}
const DEFAULT_MODEL = { openai: "deepseek-v4-flash", openrouter: "openai/gpt-4o-mini", bl: "qwen3.6-plus", uking: "" };
export function resolveModel(args, provider) {
  if (args.model && args.model !== true) return String(args.model);
  if (process.env.SO_MODEL) return process.env.SO_MODEL;
  return DEFAULT_MODEL[provider] ?? "qwen3.6-plus";
}

// 统一入口：messages=[{role,content}]，返回模型输出的纯文本。
export async function chat(messages, opts = {}) {
  const provider = opts.provider || "bl";
  const model = opts.model || DEFAULT_MODEL[provider] || "qwen3.6-plus";
  const maxTokens = opts.maxTokens || 3000;
  const timeoutMs = (opts.timeoutSec || 120) * 1000;
  if (provider === "openai" || provider === "openrouter")
    return chatOpenAI(messages, { provider, model, maxTokens, timeoutMs, ...opts });
  if (provider === "uking") return chatUking(messages, { timeoutMs, agent: opts.agent });
  return chatBl(messages, { model, maxTokens, timeoutMs });
}

// ── provider: uking（调本机已装的 AI CLI，U-King 机器零服务器依赖）──
// prompt 走 stdin，避开 Windows 命令行长度限制。默认 claude，可 --agent 换 codex。
function chatUking(messages, { timeoutMs, agent = "claude" }) {
  const prompt = messages.map((m) => (m.role === "system" ? "[系统指令]\n" + m.content : m.content)).join("\n\n");
  const spec = agent === "codex" ? ["codex", ["exec", "-"]] : ["claude", ["-p"]];
  const [bin, cliArgs] = spec;
  const r = spawnSync(bin, cliArgs, {
    input: prompt, encoding: "utf8", shell: IS_WIN,
    timeout: timeoutMs + 5000, maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, UKING_TEAMWORK_DEPTH: "1" }, // 防被 call-agent 起时无限套娃
  });
  if (r.error) throw new Error(`无法启动 ${bin}：${r.error.message}（该 AI CLI 未安装？）`);
  const out = String(r.stdout || "").trim();
  if (!out) throw new Error(`${bin} 返回空（stderr：${String(r.stderr || "").slice(0, 200)}）`);
  return out;
}

// ── provider: bl（百炼 CLI）──
function chatBl(messages, { model, maxTokens, timeoutMs }) {
  const mf = join(tmpdir(), `1so-msgs-${process.pid}-${Date.now()}-${Math.floor(Math.random() * 1e6)}.json`);
  writeFileSync(mf, JSON.stringify(messages));
  try {
    const r = spawnSync(
      "bl",
      ["text", "chat", "--messages-file", mf, "--quiet", "--max-tokens", String(maxTokens), "--model", model],
      { encoding: "utf8", shell: IS_WIN, timeout: timeoutMs + 5000, maxBuffer: 32 * 1024 * 1024 }
    );
    if (r.error) throw new Error("无法启动 bl：" + r.error.message + "（百炼 CLI 未安装？）");
    if (r.status !== 0) {
      const e = String(r.stderr || r.stdout || "").slice(0, 300);
      throw new Error("bl 调用失败(退出码 " + r.status + ")：" + e);
    }
    const out = String(r.stdout || "").trim();
    if (!out) throw new Error("bl 返回空内容");
    return out;
  } finally {
    try { unlinkSync(mf); } catch {}
  }
}

// ── provider: openai 兼容（虾盘云 / OpenRouter / 任意兼容端点）──
function resolveOpenAIKey(args = {}, provider = "openai") {
  if (args.key && args.key !== true) return String(args.key);
  if (process.env.SO_API_KEY) return process.env.SO_API_KEY;
  if (provider === "openrouter") return process.env.OPENROUTER_API_KEY || "";
  if (process.env.XIAPAN_API_KEY) return process.env.XIAPAN_API_KEY;
  try {
    const j = JSON.parse(readFileSync(join(homedir(), ".uking", "device.json"), "utf8"));
    if (j && j.key) return j.key;
  } catch {}
  return "";
}
function chatOpenAI(messages, { provider = "openai", model, maxTokens, timeoutMs, base, ...args }) {
  const defaultBase = provider === "openrouter" ? "https://openrouter.ai/api/v1" : "https://api.u-claw.org.cn";
  const baseUrl = base || process.env.SO_BASE_URL || defaultBase;
  const key = resolveOpenAIKey(args, provider);
  if (!key && provider === "openrouter")
    return Promise.reject(new Error("缺少 OpenRouter key：设环境变量 OPENROUTER_API_KEY，或加 --key sk-or-..."));
  const body = JSON.stringify({ model, messages, max_tokens: maxTokens, temperature: 0.3 });
  return new Promise((resolve, reject) => {
    // 兼容 base 带不带 /v1（虾盘云给根域，OpenRouter 给 .../api/v1）
    const b = baseUrl.replace(/\/$/, "");
    const endpoint = /\/v\d+$/.test(b) ? b + "/chat/completions" : b + "/v1/chat/completions";
    const u = new URL(endpoint);
    const req = https.request(
      { method: "POST", hostname: u.hostname, port: 443, path: u.pathname,
        headers: { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(body),
          ...(provider === "openrouter" ? { "HTTP-Referer": "https://u-king.org", "X-Title": "1so-geo" } : {}),
          ...(key ? { Authorization: `Bearer ${key}` } : {}) } },
      (res) => {
        let d = ""; res.on("data", (c) => (d += c));
        res.on("end", () => {
          try {
            const j = JSON.parse(d);
            if (j.error) return reject(new Error("端点报错：" + (j.error.message || JSON.stringify(j.error))));
            const m = j?.choices?.[0]?.message || {};
            // content 为主；推理型模型(如 deepseek-v4-pro/glm)有时把输出塞进 reasoning_content 而 content 空 →
            // 回退读它，别丢内容（对齐 providers.rs::openai_chat_full 的同款兜底）。
            const txt = (m.content && String(m.content).trim()) || (m.reasoning_content && String(m.reasoning_content).trim());
            if (!txt) return reject(new Error("端点无有效返回：" + d.slice(0, 200)));
            resolve(String(txt).trim());
          } catch (e) { reject(new Error("端点响应非 JSON：" + d.slice(0, 200))); }
        });
      }
    );
    req.on("error", reject);
    req.setTimeout(timeoutMs, () => req.destroy(new Error("请求超时")));
    req.write(body); req.end();
  });
}

// 便捷封装：要求模型返回 JSON。system 里已强约束“只出 JSON”。
export async function chatJson(system, user, opts = {}) {
  const messages = [
    { role: "system", content: system + "\n\n只输出一个合法 JSON，不要解释，不要代码围栏。" },
    { role: "user", content: user },
  ];
  const raw = await chat(messages, opts);
  return raw;
}

// 自检：后端是否可用
export async function ping(opts = {}) {
  try {
    const t0 = Date.now();
    const r = await chat([{ role: "user", content: "只回两个字：在的" }], { ...opts, maxTokens: 20, timeoutSec: 40 });
    if (/fail|error|unauthor|authenticat|invalid|not found/i.test(r) && r.length < 60)
      return { ok: false, error: "后端返回疑似错误：" + r.slice(0, 80) };
    return { ok: true, ms: Date.now() - t0, sample: r.slice(0, 20) };
  } catch (e) {
    return { ok: false, error: String(e.message || e) };
  }
}
