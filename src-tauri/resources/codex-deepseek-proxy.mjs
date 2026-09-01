/**
 * Codex ↔ DeepSeek 本地翻译代理（U-King 内置，Node 便携版跑）。
 *
 * 为什么要它：Codex CLI/App 只认 OpenAI **Responses API**（POST /v1/responses，SSE），
 * 而 DeepSeek 只会 **Chat Completions**（/v1/chat/completions）。虾盘云服务端不做转换
 * （实测 responses+deepseek 回 "not implemented"）。本代理在本机把 responses 请求转成 chat
 * 发给虾盘云的 deepseek，再把 chat 的 SSE 转回 responses 事件流吐回 Codex。
 *
 * 启停由 Rust codex_proxy.rs 管：启动=Codex 走本地代理→DeepSeek（省）；关闭=Codex 直连虾盘云
 * gpt-5.x-codex（贵几十倍）。纯 std http，无 npm 依赖。
 *
 * env：UKING_CODEX_PROXY_PORT / UKING_CODEX_UPSTREAM(chat端点) / UKING_CODEX_KEY / UKING_CODEX_MODEL
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const PORT = Number(process.env.UKING_CODEX_PROXY_PORT) || 15722;
const UPSTREAM = process.env.UKING_CODEX_UPSTREAM || "https://api.u-claw.org.cn/v1/chat/completions";
const API_KEY = process.env.UKING_CODEX_KEY || "";
const MODEL = process.env.UKING_CODEX_MODEL || "deepseek-v4-flash";

// —— 本地日志 ——
// Rust 起本进程时 stdout/stderr 都定向到 null（CREATE_NO_WINDOW，不能弹黑窗），所以 console
// 打什么都进虚空：客户报「codex 又坏了」时我们手上零线索。改成自己写文件，反馈页的诊断采集
// 会带上尾部。**只记角色序列和上游状态，绝不记 content** —— 那里面是用户的代码和 prompt。
const LOG_FILE = path.join(process.env.USERPROFILE || process.env.HOME || ".", ".uking", "codex-proxy.log");
function log(line) {
  try {
    if (fs.existsSync(LOG_FILE) && fs.statSync(LOG_FILE).size > 256 * 1024) fs.writeFileSync(LOG_FILE, "");
    fs.appendFileSync(LOG_FILE, `[${new Date().toISOString()}] ${line}\n`);
  } catch {}
}
/// 消息序列的紧凑摘要（排障够用，不含任何正文）。
const shape = (msgs) =>
  msgs.map((m) => (m.tool_calls ? `assistant<tc×${m.tool_calls.length}>` : m.role)).join(",");

const now = () => Math.floor(Date.now() / 1000);
const rid = () => "resp_" + Math.random().toString(36).slice(2, 14);
const mid = () => "msg_" + Math.random().toString(36).slice(2, 14);

// —— responses 请求 → chat 请求 ——

/// 工具结果丢了时给上游的占位。写清楚原因，让模型知道该重新调用而不是照着空气编结果。
const LOST_OUTPUT = "[工具结果不可用：上下文被压缩或该调用被中断，请在需要时重新调用]";

/// responses 的 `function_call_output.output` → chat 的 tool.content（字符串）。
const outText = (o) => (typeof o === "string" ? o : JSON.stringify(o ?? ""));

/// 普通 message 项 → chat 消息。
/// codex 用 OpenAI 的 developer 角色（responses API），DeepSeek chat 只认
/// system/user/assistant/tool → 归一。
function messageItem(it) {
  let role = it.role || "user";
  if (role === "developer") role = "system";
  if (!["system", "user", "assistant", "tool"].includes(role)) role = "user";
  // tool 角色只能由 function_call_output 产生（必须带 tool_call_id），
  // 裸的 tool message 会让上游 400 → 降级成 user。
  if (role === "tool") role = "user";
  let content = "";
  if (typeof it.content === "string") content = it.content;
  else if (Array.isArray(it.content)) content = it.content.map((c) => c.text ?? c.input_text ?? c.output_text ?? "").join("");
  return { role, content };
}

/// 把 responses 的 input 数组翻成 chat messages，**并强制满足上游的配对硬约束**：
/// 带 tool_calls 的 assistant 消息后面必须紧跟等量、call_id 一一对应的 tool 消息。
///
/// 逐项 1:1 翻译满足不了这条（这就是「502 upstream 400: insufficient tool messages
/// following tool_calls message」的根因），三种情况都会炸：
///   ① **并行工具调用**（MCP 场景常见）：N 个 function_call 被拆成 N 条挨着的 assistant，
///      中间没有 tool 消息 —— 什么都没丢也会炸，这条最狠；
///   ② **上下文自动压缩**：Codex 把 function_call_output 裁掉了，只剩落单的 call；
///   ③ **用户打断 / 拒绝审批**：call 发出去了，永远等不到 output。
/// 所以改成按「调用轮次」成组翻译：连续的 function_call 合并成一条 assistant，紧随其后的
/// output 按 call_id 配对，配不上的补占位。返回 { msgs, repairs } —— repairs 只用于写日志。
function inputToMessages(body) {
  const msgs = [];
  const repairs = [];
  if (body.instructions) msgs.push({ role: "system", content: String(body.instructions) });
  const input = body.input;
  if (typeof input === "string") {
    msgs.push({ role: "user", content: input });
    return { msgs, repairs };
  }
  if (!Array.isArray(input)) return { msgs, repairs };

  for (let i = 0; i < input.length; ) {
    const it = input[i];

    if (it.type === "function_call") {
      // ① 收下连续的一批 function_call（并行调用）→ 合并成**一条** assistant
      const calls = [];
      while (i < input.length && input[i].type === "function_call") {
        const c = input[i];
        calls.push({
          id: c.call_id || c.id,
          type: "function",
          function: { name: c.name, arguments: c.arguments || "{}" },
        });
        i++;
      }
      // ② 紧随其后的一批 output 收进 map，按 call_id 配对（顺序可以不一致）
      const outs = new Map();
      while (i < input.length && input[i].type === "function_call_output") {
        outs.set(input[i].call_id, outText(input[i].output));
        i++;
      }
      msgs.push({ role: "assistant", content: null, tool_calls: calls });
      // ③ 每个 call 必须有一条 tool 回复，缺的补占位
      for (const c of calls) {
        const hit = outs.has(c.id);
        if (!hit) repairs.push(`补占位:${c.function.name}`);
        msgs.push({ role: "tool", tool_call_id: c.id, content: hit ? outs.get(c.id) : LOST_OUTPUT });
        outs.delete(c.id);
      }
      // ④ 对不上任何 call 的 output（call 那头被压缩裁掉了）→ 不能当 tool 发，降级成文本保住信息
      for (const [, text] of outs) {
        repairs.push("孤儿输出降级");
        msgs.push({ role: "user", content: `[工具执行结果] ${text}` });
      }
      if (calls.length > 1) repairs.push(`并行合并×${calls.length}`);
      continue;
    }

    if (it.type === "function_call_output") {
      // 落单的 output（前面的 function_call 已被裁掉）—— 同 ④
      repairs.push("孤儿输出降级");
      msgs.push({ role: "user", content: `[工具执行结果] ${outText(it.output)}` });
      i++;
      continue;
    }

    msgs.push(messageItem(it));
    i++;
  }
  return { msgs, repairs };
}
function toolsToChat(tools) {
  if (!Array.isArray(tools)) return undefined;
  const out = [];
  for (const t of tools) {
    if (t.type === "function") {
      // responses 里 function 可能扁平（name/description/parameters）或嵌 function:{}
      const fn = t.function || t;
      out.push({ type: "function", function: { name: fn.name, description: fn.description || "", parameters: fn.parameters || { type: "object", properties: {} } } });
    }
  }
  return out.length ? out : undefined;
}

function sse(res, event, obj) {
  res.write(`event: ${event}\n`);
  res.write(`data: ${JSON.stringify(obj)}\n\n`);
}

// —— 把 chat 的 SSE 流转成 responses 事件流写回 res ——
async function pipeChatToResponses(upstreamRes, res, model) {
  const response_id = rid();
  const item_id = mid();
  let seq = 0;
  const base = { id: response_id, object: "response", created_at: now(), model, status: "in_progress" };
  sse(res, "response.created", { type: "response.created", response: base });
  sse(res, "response.in_progress", { type: "response.in_progress", response: base });
  // 文本消息 item
  sse(res, "response.output_item.added", { type: "response.output_item.added", output_index: 0, item: { id: item_id, type: "message", role: "assistant", status: "in_progress", content: [] } });
  sse(res, "response.content_part.added", { type: "response.content_part.added", item_id, output_index: 0, content_index: 0, part: { type: "output_text", text: "" } });

  let textBuf = "";
  const toolAcc = new Map(); // index -> {id,name,args}
  let usage = null;
  let buf = "";

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
      try { v = JSON.parse(data); } catch { continue; }
      if (v.usage) usage = v.usage;
      const delta = v.choices?.[0]?.delta;
      if (!delta) continue;
      if (delta.content) {
        textBuf += delta.content;
        sse(res, "response.output_text.delta", { type: "response.output_text.delta", item_id, output_index: 0, content_index: 0, delta: delta.content, sequence_number: seq++ });
      }
      if (Array.isArray(delta.tool_calls)) {
        for (const tc of delta.tool_calls) {
          const idx = tc.index ?? 0;
          if (!toolAcc.has(idx)) toolAcc.set(idx, { id: tc.id || "call_" + mid(), name: "", args: "" });
          const slot = toolAcc.get(idx);
          if (tc.id) slot.id = tc.id;
          if (tc.function?.name) slot.name = tc.function.name;
          if (tc.function?.arguments) slot.args += tc.function.arguments;
        }
      }
    }
  }

  // 收尾文本
  sse(res, "response.output_text.done", { type: "response.output_text.done", item_id, output_index: 0, content_index: 0, text: textBuf });
  sse(res, "response.content_part.done", { type: "response.content_part.done", item_id, output_index: 0, content_index: 0, part: { type: "output_text", text: textBuf } });
  sse(res, "response.output_item.done", { type: "response.output_item.done", output_index: 0, item: { id: item_id, type: "message", role: "assistant", status: "completed", content: [{ type: "output_text", text: textBuf }] } });

  // 工具调用 item（每个一条 function_call）
  const outputs = [{ id: item_id, type: "message", role: "assistant", status: "completed", content: [{ type: "output_text", text: textBuf }] }];
  let oi = 1;
  for (const [, slot] of toolAcc) {
    const fcId = "fc_" + mid();
    const fcItem = { id: fcId, type: "function_call", status: "completed", name: slot.name, arguments: slot.args || "{}", call_id: slot.id };
    sse(res, "response.output_item.added", { type: "response.output_item.added", output_index: oi, item: { ...fcItem, status: "in_progress" } });
    sse(res, "response.function_call_arguments.delta", { type: "response.function_call_arguments.delta", item_id: fcId, output_index: oi, delta: slot.args || "{}" });
    sse(res, "response.function_call_arguments.done", { type: "response.function_call_arguments.done", item_id: fcId, output_index: oi, arguments: slot.args || "{}" });
    sse(res, "response.output_item.done", { type: "response.output_item.done", output_index: oi, item: fcItem });
    outputs.push(fcItem);
    oi++;
  }

  const final = { ...base, status: "completed", output: outputs, usage: usage ? { input_tokens: usage.prompt_tokens ?? 0, output_tokens: usage.completion_tokens ?? 0, total_tokens: usage.total_tokens ?? 0 } : undefined };
  sse(res, "response.completed", { type: "response.completed", response: final });
  res.end();
}

const server = http.createServer(async (req, res) => {
  if (req.method === "GET" && req.url === "/health") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: true, model: MODEL, upstream: UPSTREAM }));
    return;
  }
  if (req.method !== "POST" || !/\/responses$/.test((req.url || "").split("?")[0])) {
    res.writeHead(404).end("not found");
    return;
  }
  let raw = "";
  req.on("data", (c) => (raw += c));
  req.on("end", async () => {
    let body;
    try { body = JSON.parse(raw || "{}"); } catch { res.writeHead(400).end("bad json"); return; }
    const { msgs, repairs } = inputToMessages(body);
    if (repairs.length) log(`修补对话历史: ${repairs.join(" ")} | ${shape(msgs)}`);
    const chatReq = {
      model: MODEL,
      messages: msgs,
      stream: true,
      stream_options: { include_usage: true },
    };
    const tools = toolsToChat(body.tools);
    if (tools) { chatReq.tools = tools; chatReq.tool_choice = body.tool_choice || "auto"; }
    try {
      const up = await fetch(UPSTREAM, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: "Bearer " + API_KEY },
        body: JSON.stringify(chatReq),
      });
      if (!up.ok || !up.body) {
        const t = await up.text().catch(() => "");
        log(`上游 ${up.status} model=${MODEL} | ${shape(msgs)} | ${t.slice(0, 200)}`);
        res.writeHead(502, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: { message: "upstream " + up.status + ": " + t.slice(0, 300) } }));
        return;
      }
      res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache", connection: "keep-alive" });
      await pipeChatToResponses(up, res, MODEL);
    } catch (e) {
      log(`请求异常 model=${MODEL}: ${String(e).slice(0, 200)}`);
      try { res.writeHead(502, { "content-type": "application/json" }); res.end(JSON.stringify({ error: { message: String(e) } })); } catch {}
    }
  });
});

server.listen(PORT, "127.0.0.1", () => {
  log(`启动 127.0.0.1:${PORT} → ${MODEL} @ ${UPSTREAM}`);
  console.log(`[codex-deepseek-proxy] listening 127.0.0.1:${PORT} → ${MODEL} @ ${UPSTREAM}`);
});
server.on("error", (e) => log(`监听失败 :${PORT} ${String(e).slice(0, 200)}`));
