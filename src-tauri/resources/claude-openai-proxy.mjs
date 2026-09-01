/**
 * Claude Code ↔ OpenAI 兼容端点 本地翻译代理（U-King 内置，便携 Node 跑）。
 *
 * 为什么要它：Claude Code 只会说 **Anthropic Messages API**（POST /v1/messages，SSE），
 * 而客户自带的中转站里有一大批**只有 OpenAI 格式**（/v1/chat/completions）。以前我们只能
 * 如实告诉他「这个供应商驱动不了 Claude Code」（issue #359 / #322）—— 说的是实话，但客户
 * 要的是能用。本代理在本机把 messages 请求翻成 chat，再把 chat 的 SSE 翻回 Anthropic 事件流。
 *
 * 跟 `codex-deepseek-proxy.mjs` 是**同一个套路的另一个方向**（那个是 responses↔chat），
 * 两边共用的经验：纯 node:http + fetch、零 npm 依赖、上游出错原样透传、
 * **tool_call 配对是硬约束**（见 `messagesToChat` 的注释）。
 *
 * env：
 *   UKING_CLAUDE_PROXY_PORT   监听端口（默认 15723，跟 codex 代理的 15722 岔开）
 *   UKING_CLAUDE_UPSTREAM     上游 chat 端点全 URL
 *   UKING_CLAUDE_PROXY_KEY    上游 Key。**留空则转发客户端自己带的 key**（见下）
 *   UKING_CLAUDE_PROXY_MODEL  强制模型名；留空则原样透传客户端传来的 model
 *
 * 🔴 Key 默认不进 env：Claude Code 本来就会把 `ANTHROPIC_AUTH_TOKEN` 放进请求头，
 * 我们直接转发那一份就行 —— 少一份 Key 的副本，就少一处泄漏面（不进环境变量、
 * 不进命令行、不进进程列表）。只有确实需要「客户端不带 Key」时才配 UKING_CLAUDE_PROXY_KEY。
 *
 * 自检：`node claude-openai-proxy.selftest.mjs`（起一个假上游，不联网、不烧 token）。
 */
import http from "node:http";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

const PORT = Number(process.env.UKING_CLAUDE_PROXY_PORT) || 15723;
const UPSTREAM = process.env.UKING_CLAUDE_UPSTREAM || "https://api.u-claw.org.cn/v1/chat/completions";
const ENV_KEY = process.env.UKING_CLAUDE_PROXY_KEY || "";
const FORCE_MODEL = process.env.UKING_CLAUDE_PROXY_MODEL || "";

/** 工具结果丢了时的占位。宁可明说「结果没留下」，也不能让这一轮的 tool_call 没有回复。 */
const LOST_OUTPUT = "[工具结果在上下文压缩时被裁掉了]";

const now = () => Math.floor(Date.now() / 1000);
let _n = 0;
const uid = (p) => `${p}_${Date.now().toString(36)}${(_n++).toString(36)}`;

// ————————————————————————— 请求方向：Anthropic → OpenAI chat —————————————————————————

/** Anthropic 的 system 可以是字符串，也可以是一组 text block。 */
function systemText(system) {
  if (!system) return "";
  if (typeof system === "string") return system;
  if (Array.isArray(system)) {
    return system.filter((b) => b && b.type === "text").map((b) => b.text || "").join("\n");
  }
  return "";
}

/** tool_result 的 content 可以是字符串，也可以是一组 block。压成纯文本给 OpenAI 的 tool 消息。 */
function resultText(content) {
  if (content == null) return "";
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((b) => {
        if (typeof b === "string") return b;
        if (b?.type === "text") return b.text || "";
        // 工具返回图片：OpenAI 的 tool 消息只收文本，如实说明有一张图而不是静默丢掉。
        if (b?.type === "image") return "[图片]";
        return "";
      })
      .filter(Boolean)
      .join("\n");
  }
  return String(content);
}

/** 用户/助手消息里的 content block → OpenAI 的 content（字符串或多模态数组）。 */
function blocksToChatContent(content) {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  const parts = [];
  for (const b of content) {
    if (!b) continue;
    if (b.type === "text") parts.push({ type: "text", text: b.text || "" });
    else if (b.type === "image" && b.source?.type === "base64") {
      parts.push({
        type: "image_url",
        image_url: { url: `data:${b.source.media_type || "image/png"};base64,${b.source.data || ""}` },
      });
    }
    // thinking / redacted_thinking：Anthropic 专有，OpenAI 侧没有对应物，丢掉。
    // 它们本来就不该回传给模型当输入（Anthropic 自己也只在同一轮内有意义）。
  }
  if (!parts.length) return "";
  // 全是文本就压成字符串 —— 有些中转对多模态数组支持不好，能简则简。
  if (parts.every((p) => p.type === "text")) return parts.map((p) => p.text).join("\n");
  return parts;
}

/**
 * Anthropic messages → OpenAI chat messages。
 *
 * 🔴 **tool_call 配对是硬约束**，不是风格问题：OpenAI 侧要求 assistant 消息里的每一个
 * `tool_calls[i].id`，都有一条紧随其后、`tool_call_id` 对得上的 `role:"tool"` 消息。
 * 少一条、或者出现对不上任何 call 的 tool 消息，上游直接 400/502。
 * 而 Claude Code 的历史被压缩过之后，**确实会**出现「有 tool_use 没有 tool_result」
 * （结果那半边被裁了）和「有 tool_result 没有 tool_use」（调用那半边被裁了）。
 * 所以这里必须修补，不能原样转发 —— 同 codex-deepseek-proxy 的做法，那个坑是真踩过的。
 */
export function messagesToChat(body) {
  const msgs = [];
  const repairs = [];
  const sys = systemText(body.system);
  if (sys) msgs.push({ role: "system", content: sys });

  for (const m of body.messages || []) {
    if (!m) continue;
    const content = m.content;

    if (m.role === "assistant") {
      const blocks = Array.isArray(content) ? content : [{ type: "text", text: String(content ?? "") }];
      const text = blocks.filter((b) => b?.type === "text").map((b) => b.text || "").join("");
      const uses = blocks.filter((b) => b?.type === "tool_use");
      if (uses.length) {
        msgs.push({
          role: "assistant",
          content: text || null,
          tool_calls: uses.map((u) => ({
            id: u.id,
            type: "function",
            function: { name: u.name, arguments: JSON.stringify(u.input ?? {}) },
          })),
        });
      } else if (text) {
        msgs.push({ role: "assistant", content: text });
      }
      continue;
    }

    // role === "user"：可能夹着一批 tool_result，也可能有正文
    const blocks = Array.isArray(content) ? content : [{ type: "text", text: String(content ?? "") }];
    const results = blocks.filter((b) => b?.type === "tool_result");
    const rest = blocks.filter((b) => b?.type !== "tool_result");

    if (results.length) {
      // 上一条 assistant 发起了哪些 call —— tool 消息必须**按它的顺序**紧跟其后
      const prev = msgs[msgs.length - 1];
      const pending = prev?.role === "assistant" && Array.isArray(prev.tool_calls) ? prev.tool_calls : [];
      const byId = new Map(results.map((r) => [r.tool_use_id, r]));

      for (const call of pending) {
        const hit = byId.get(call.id);
        if (!hit) repairs.push(`补占位:${call.function.name}`);
        msgs.push({
          role: "tool",
          tool_call_id: call.id,
          content: hit ? resultText(hit.content) || "(空)" : LOST_OUTPUT,
        });
        byId.delete(call.id);
      }
      // 对不上任何 call 的结果（调用那半边被压缩裁掉了）→ 不能当 tool 发，降级成文本保住信息
      for (const [, r] of byId) {
        repairs.push("孤儿结果降级");
        rest.unshift({ type: "text", text: `[工具执行结果] ${resultText(r.content)}` });
      }
    }

    const c = blocksToChatContent(rest);
    if (c && (typeof c !== "string" || c.trim())) msgs.push({ role: "user", content: c });
  }

  const req = {
    model: FORCE_MODEL || body.model,
    messages: msgs,
    stream: body.stream !== false,
  };
  if (req.stream) req.stream_options = { include_usage: true };
  if (Number.isFinite(body.max_tokens)) req.max_tokens = body.max_tokens;
  if (Number.isFinite(body.temperature)) req.temperature = body.temperature;
  if (Number.isFinite(body.top_p)) req.top_p = body.top_p;
  if (Array.isArray(body.stop_sequences) && body.stop_sequences.length) req.stop = body.stop_sequences;

  const tools = toolsToChat(body.tools);
  if (tools) {
    req.tools = tools;
    const tc = toolChoiceToChat(body.tool_choice);
    if (tc) req.tool_choice = tc;
  }
  return { req, repairs };
}

export function toolsToChat(tools) {
  if (!Array.isArray(tools) || !tools.length) return undefined;
  const out = [];
  for (const t of tools) {
    if (!t?.name) continue; // Anthropic 的服务端工具（computer/text_editor 等）没有 input_schema，跳过
    out.push({
      type: "function",
      function: {
        name: t.name,
        description: t.description || "",
        parameters: t.input_schema || { type: "object", properties: {} },
      },
    });
  }
  return out.length ? out : undefined;
}

export function toolChoiceToChat(tc) {
  if (!tc) return undefined;
  if (tc.type === "auto") return "auto";
  if (tc.type === "any") return "required";
  if (tc.type === "none") return "none";
  if (tc.type === "tool" && tc.name) return { type: "function", function: { name: tc.name } };
  return undefined;
}

/** OpenAI finish_reason → Anthropic stop_reason。 */
export function mapStopReason(finish, sawTool) {
  if (sawTool) return "tool_use";
  switch (finish) {
    case "length":
      return "max_tokens";
    case "tool_calls":
    case "function_call":
      return "tool_use";
    case "stop":
      return "end_turn";
    default:
      return "end_turn";
  }
}

// ————————————————————————— 响应方向：OpenAI chat SSE → Anthropic SSE —————————————————————————

function sse(res, event, obj) {
  res.write(`event: ${event}\n`);
  res.write(`data: ${JSON.stringify(obj)}\n\n`);
}

/**
 * 把 chat 的 SSE 转成 Anthropic 的事件流。
 *
 * Anthropic 的块模型是「按 index 开-写-关」，而 OpenAI 是「文本和 tool_calls 混在 delta 里」。
 * 两边的关键差异：OpenAI 的 tool 参数是**逐段 JSON 字符串**拼出来的，Anthropic 也一样
 * （`input_json_delta.partial_json`），所以可以直接转发碎片，不必等拼完 —— 但**块必须先开**，
 * 而 OpenAI 是在第一个 delta 里才给出 name/id，所以 tool 块只能懒开。
 */
async function pipeChatToAnthropic(upstreamRes, res, model) {
  const msgId = uid("msg");
  let nextIndex = 0;
  let textIndex = -1; // 文本块的 index，-1 = 还没开
  const toolSlots = new Map(); // openai tool_calls index -> {aIndex, id, name, opened}
  let usage = null;
  let finish = null;
  let buf = "";

  sse(res, "message_start", {
    type: "message_start",
    message: {
      id: msgId,
      type: "message",
      role: "assistant",
      model,
      content: [],
      stop_reason: null,
      stop_sequence: null,
      usage: { input_tokens: 0, output_tokens: 0 },
    },
  });

  const openText = () => {
    if (textIndex >= 0) return;
    textIndex = nextIndex++;
    sse(res, "content_block_start", {
      type: "content_block_start",
      index: textIndex,
      content_block: { type: "text", text: "" },
    });
  };

  const decoder = new TextDecoder();
  for await (const chunk of upstreamRes.body) {
    buf += decoder.decode(chunk, { stream: true });
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line.startsWith("data:")) continue;
      const data = line.slice(5).trim();
      if (data === "[DONE]") continue;
      let v;
      try {
        v = JSON.parse(data);
      } catch {
        continue;
      }
      if (v.usage) usage = v.usage;
      const choice = v.choices?.[0];
      if (!choice) continue;
      if (choice.finish_reason) finish = choice.finish_reason;
      const delta = choice.delta;
      if (!delta) continue;

      if (delta.content) {
        openText();
        sse(res, "content_block_delta", {
          type: "content_block_delta",
          index: textIndex,
          delta: { type: "text_delta", text: delta.content },
        });
      }

      for (const tc of delta.tool_calls || []) {
        const k = tc.index ?? 0;
        if (!toolSlots.has(k)) toolSlots.set(k, { aIndex: -1, id: "", name: "", opened: false });
        const slot = toolSlots.get(k);
        if (tc.id) slot.id = tc.id;
        if (tc.function?.name) slot.name = tc.function.name;
        // 拿到 name 才能开块（Anthropic 的 content_block_start 里 name 是必填的）
        if (!slot.opened && slot.name) {
          // 文本块要先收掉：Anthropic 要求块是顺序开关的，不能交叉
          if (textIndex >= 0) {
            sse(res, "content_block_stop", { type: "content_block_stop", index: textIndex });
            textIndex = -2; // -2 = 已关，别再往里写
          }
          slot.aIndex = nextIndex++;
          slot.opened = true;
          sse(res, "content_block_start", {
            type: "content_block_start",
            index: slot.aIndex,
            content_block: { type: "tool_use", id: slot.id || uid("toolu"), name: slot.name, input: {} },
          });
        }
        const args = tc.function?.arguments;
        if (args && slot.opened) {
          sse(res, "content_block_delta", {
            type: "content_block_delta",
            index: slot.aIndex,
            delta: { type: "input_json_delta", partial_json: args },
          });
        }
      }
    }
  }

  if (textIndex >= 0) sse(res, "content_block_stop", { type: "content_block_stop", index: textIndex });
  for (const [, slot] of toolSlots) {
    if (slot.opened) sse(res, "content_block_stop", { type: "content_block_stop", index: slot.aIndex });
  }

  sse(res, "message_delta", {
    type: "message_delta",
    delta: { stop_reason: mapStopReason(finish, toolSlots.size > 0), stop_sequence: null },
    usage: { output_tokens: usage?.completion_tokens ?? 0 },
  });
  sse(res, "message_stop", { type: "message_stop" });
  res.end();
}

/** 非流式：chat 的一次性 JSON → Anthropic Message。 */
export function chatJsonToAnthropic(j, model) {
  const choice = j.choices?.[0] || {};
  const m = choice.message || {};
  const content = [];
  if (m.content) content.push({ type: "text", text: m.content });
  for (const tc of m.tool_calls || []) {
    let input = {};
    try {
      input = JSON.parse(tc.function?.arguments || "{}");
    } catch {
      input = {};
    }
    content.push({ type: "tool_use", id: tc.id || uid("toolu"), name: tc.function?.name || "", input });
  }
  return {
    id: j.id || uid("msg"),
    type: "message",
    role: "assistant",
    model: j.model || model,
    content,
    stop_reason: mapStopReason(choice.finish_reason, (m.tool_calls || []).length > 0),
    stop_sequence: null,
    usage: {
      input_tokens: j.usage?.prompt_tokens ?? 0,
      output_tokens: j.usage?.completion_tokens ?? 0,
    },
  };
}

/**
 * 估算 token 数给 /v1/messages/count_tokens。
 *
 * **这是估算，不是真数**。上游是 OpenAI 兼容端点，没有 Anthropic 的计数接口，我们也不想
 * 为了数个数就真发一次请求（那要花钱）。按「中文 ~1 字/token、其余 ~4 字符/token」粗估。
 * 不实现这个接口的话 Claude Code 那边会报错，所以宁可给个标了口径的估算。
 */
export function estimateTokens(body) {
  let s = systemText(body.system);
  for (const m of body.messages || []) {
    const c = m?.content;
    if (typeof c === "string") s += c;
    else if (Array.isArray(c)) {
      for (const b of c) {
        if (b?.type === "text") s += b.text || "";
        else if (b?.type === "tool_result") s += resultText(b.content);
        else if (b?.type === "tool_use") s += JSON.stringify(b.input ?? {});
      }
    }
  }
  for (const t of body.tools || []) s += (t?.name || "") + (t?.description || "") + JSON.stringify(t?.input_schema ?? {});
  let cjk = 0;
  for (const ch of s) if (ch.charCodeAt(0) > 0x2e7f) cjk++;
  return Math.ceil(cjk + (s.length - cjk) / 4);
}

/** 上游错误 → Anthropic 形状的错误体（Claude Code 只认这个形状）。 */
function anthropicError(status, message) {
  const type =
    status === 401 || status === 403
      ? "authentication_error"
      : status === 429
        ? "rate_limit_error"
        : status >= 500
          ? "api_error"
          : "invalid_request_error";
  return { type: "error", error: { type, message } };
}

// ————————————————————————————————— HTTP 服务 —————————————————————————————————

function clientKey(req) {
  const xk = req.headers["x-api-key"];
  if (typeof xk === "string" && xk) return xk;
  const auth = req.headers["authorization"];
  if (typeof auth === "string" && auth) return auth.replace(/^Bearer\s+/i, "");
  return "";
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", () => resolve(raw));
    req.on("error", reject);
  });
}

export function createServer({ upstream = UPSTREAM, envKey = ENV_KEY } = {}) {
  return http.createServer(async (req, res) => {
    const path = (req.url || "").split("?")[0];

    if (req.method === "GET" && path === "/health") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, upstream, bridge: "anthropic->openai" }));
      return;
    }

    if (req.method !== "POST") {
      res.writeHead(404).end("not found");
      return;
    }

    let body;
    try {
      body = JSON.parse((await readBody(req)) || "{}");
    } catch {
      res.writeHead(400, { "content-type": "application/json" });
      res.end(JSON.stringify(anthropicError(400, "bad json")));
      return;
    }

    if (/\/count_tokens$/.test(path)) {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ input_tokens: estimateTokens(body) }));
      return;
    }

    if (!/\/messages$/.test(path)) {
      res.writeHead(404).end("not found");
      return;
    }

    const key = envKey || clientKey(req);
    const { req: chatReq } = messagesToChat(body);
    const model = chatReq.model;

    try {
      const up = await fetch(upstream, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: "Bearer " + key },
        body: JSON.stringify(chatReq),
      });

      if (!up.ok) {
        const t = await up.text().catch(() => "");
        res.writeHead(up.status, { "content-type": "application/json" });
        res.end(JSON.stringify(anthropicError(up.status, `上游 ${up.status}: ${t.slice(0, 300)}`)));
        return;
      }

      if (!chatReq.stream) {
        const j = await up.json();
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify(chatJsonToAnthropic(j, model)));
        return;
      }

      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
      });
      await pipeChatToAnthropic(up, res, model);
    } catch (e) {
      const msg = String(e).slice(0, 300);
      try {
        if (!res.headersSent) {
          res.writeHead(502, { "content-type": "application/json" });
          res.end(JSON.stringify(anthropicError(502, msg)));
        } else {
          res.end();
        }
      } catch {}
    }
  });
}

// ————————————————————————————————— 入口 —————————————————————————————————

// 只有被当入口跑时才监听；被自检 import 时不能自己占端口。
// 🔴 自检入口是 `node claude-openai-proxy.selftest.mjs`，**别改成在本模块里
// `await import(selftest)`** —— selftest 反过来 import 本模块，ESM 循环 + top-level
// await 会直接死锁，Node 只丢一句 "unsettled top-level await"，看着像卡住而不是报错。
const isMain = !!process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);

if (isMain) {
  const server = createServer();
  server.listen(PORT, "127.0.0.1", () => {
    console.log(`[claude-openai-proxy] listening 127.0.0.1:${PORT} -> ${UPSTREAM}`);
  });
  server.on("error", (e) => {
    console.error(`[claude-openai-proxy] listen failed :${PORT} ${String(e).slice(0, 200)}`);
    process.exit(1);
  });
}
