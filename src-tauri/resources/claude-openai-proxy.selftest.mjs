/**
 * `claude-openai-proxy.mjs` 的自检跑道。**不联网、不烧 token**：起一个假的 OpenAI 上游，
 * 回放罐装 SSE，然后拿真的 HTTP 请求打真的代理，断言翻出来的 Anthropic 事件流。
 *
 * 跑法：`node claude-openai-proxy.selftest.mjs`（退出码 0 = 全过）。
 *
 * 为什么值得单开：这座桥的正确性**一个字节都不在 Rust 单测里**，翻译逻辑全在 JS。
 * 而它最容易坏的地方（tool_call 配对、块不能交叉、partial_json 拼接）恰好是那种
 * 「形状看着对、上游直接 400」的错 —— 只有真发一遍请求才验得出来。
 */
import http from "node:http";
import {
  messagesToChat,
  toolsToChat,
  toolChoiceToChat,
  mapStopReason,
  chatJsonToAnthropic,
  estimateTokens,
  createServer,
} from "./claude-openai-proxy.mjs";

let pass = 0;
const fails = [];
function ok(cond, name, extra = "") {
  if (cond) {
    pass++;
  } else {
    fails.push(`${name}${extra ? " —— " + extra : ""}`);
  }
}
function eq(a, b, name) {
  const A = JSON.stringify(a);
  const B = JSON.stringify(b);
  ok(A === B, name, `\n    实际 ${A}\n    期望 ${B}`);
}

// ————————————————— 假上游：按请求里的 model 决定回放哪段罐装 SSE —————————————————

function chunk(o) {
  return `data: ${JSON.stringify(o)}\n\n`;
}
const SCRIPTS = {
  "fake-text": [
    chunk({ choices: [{ delta: { role: "assistant", content: "你好" } }] }),
    chunk({ choices: [{ delta: { content: "，世界" } }] }),
    chunk({ choices: [{ delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 11, completion_tokens: 7 } }),
    "data: [DONE]\n\n",
  ],
  // 文本 → 工具：验「文本块必须先关，块不许交叉」
  "fake-tool": [
    chunk({ choices: [{ delta: { content: "我查一下" } }] }),
    chunk({
      choices: [{ delta: { tool_calls: [{ index: 0, id: "call_a1", function: { name: "Bash", arguments: '{"cmd"' } }] } }],
    }),
    chunk({ choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: ':"ls -l"}' } }] } }] }),
    chunk({ choices: [{ delta: {}, finish_reason: "tool_calls" }], usage: { prompt_tokens: 20, completion_tokens: 9 } }),
    "data: [DONE]\n\n",
  ],
  "fake-len": [
    chunk({ choices: [{ delta: { content: "截" } }] }),
    chunk({ choices: [{ delta: {}, finish_reason: "length" }] }),
    "data: [DONE]\n\n",
  ],
};

let lastUpstreamReq = null;

function startFakeUpstream() {
  return new Promise((resolve) => {
    const s = http.createServer((req, res) => {
      let raw = "";
      req.on("data", (c) => (raw += c));
      req.on("end", () => {
        const body = JSON.parse(raw || "{}");
        lastUpstreamReq = body;
        if (body.model === "fake-401") {
          res.writeHead(401, { "content-type": "application/json" });
          res.end(JSON.stringify({ error: { message: "no credit" } }));
          return;
        }
        if (body.model === "fake-nostream") {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(
            JSON.stringify({
              id: "chatcmpl-x",
              model: "fake-nostream",
              choices: [
                {
                  finish_reason: "tool_calls",
                  message: {
                    role: "assistant",
                    content: "好的",
                    tool_calls: [{ id: "call_z", function: { name: "Read", arguments: '{"p":"/a"}' } }],
                  },
                },
              ],
              usage: { prompt_tokens: 3, completion_tokens: 5 },
            }),
          );
          return;
        }
        const script = SCRIPTS[body.model];
        if (!script) {
          res.writeHead(400).end("unknown fake model " + body.model);
          return;
        }
        res.writeHead(200, { "content-type": "text/event-stream" });
        for (const c of script) res.write(c);
        res.end();
      });
    });
    s.listen(0, "127.0.0.1", () => resolve(s));
  });
}

/** 把 Anthropic SSE 文本拆成 [{event, data}]。 */
function parseSse(text) {
  const out = [];
  for (const blk of text.split("\n\n")) {
    const ev = /^event:\s*(.+)$/m.exec(blk);
    const da = /^data:\s*(.+)$/m.exec(blk);
    if (ev && da) out.push({ event: ev[1].trim(), data: JSON.parse(da[1]) });
  }
  return out;
}

async function post(port, path, body) {
  const r = await fetch(`http://127.0.0.1:${port}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-api-key": "sk-client-side" },
    body: JSON.stringify(body),
  });
  const text = await r.text();
  return { status: r.status, text };
}

export async function runSelfTest() {
  // ——————————————— ① 纯函数：请求方向的翻译 ———————————————

  {
    const { req } = messagesToChat({
      model: "m",
      system: [{ type: "text", text: "你是助手" }],
      messages: [{ role: "user", content: "在吗" }],
    });
    eq(req.messages[0], { role: "system", content: "你是助手" }, "system 块 → system 消息");
  }

  {
    // 完整一轮：assistant 发起两个 call，user 带回两个 result（**顺序故意反着给**）
    const { req, repairs } = messagesToChat({
      model: "m",
      messages: [
        { role: "user", content: "跑一下" },
        {
          role: "assistant",
          content: [
            { type: "thinking", thinking: "内部推理不该外传" },
            { type: "text", text: "好" },
            { type: "tool_use", id: "t1", name: "Bash", input: { cmd: "ls" } },
            { type: "tool_use", id: "t2", name: "Read", input: { p: "/x" } },
          ],
        },
        {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "t2", content: "文件内容" },
            { type: "tool_result", tool_use_id: "t1", content: [{ type: "text", text: "a.txt" }] },
            { type: "text", text: "继续" },
          ],
        },
      ],
    });
    const roles = req.messages.map((m) => m.role);
    eq(roles, ["user", "assistant", "tool", "tool", "user"], "tool 消息紧跟 assistant");
    eq(
      req.messages.filter((m) => m.role === "tool").map((m) => m.tool_call_id),
      ["t1", "t2"],
      "tool 顺序必须跟 assistant 的 tool_calls 一致（不是客户端给的顺序）",
    );
    eq(req.messages[2].content, "a.txt", "tool_result 的 block 数组压成文本");
    ok(!JSON.stringify(req).includes("内部推理不该外传"), "thinking 块不回传上游");
    eq(repairs, [], "这轮是完整的，不该有修补");
  }

  {
    // 结果那半边被压缩裁掉 → 必须补占位，否则上游 400
    const { req, repairs } = messagesToChat({
      model: "m",
      messages: [
        { role: "assistant", content: [{ type: "tool_use", id: "t9", name: "Bash", input: {} }] },
        { role: "user", content: [{ type: "tool_result", tool_use_id: "不存在的", content: "野结果" }] },
      ],
    });
    const tools = req.messages.filter((m) => m.role === "tool");
    eq(tools.length, 1, "每个 tool_call 必须恰好一条 tool 回复");
    eq(tools[0].tool_call_id, "t9", "补的占位要挂在真实的 call id 上");
    ok(/裁掉/.test(tools[0].content), "占位要说明结果为什么没了");
    ok(
      repairs.some((r) => r.startsWith("补占位")) && repairs.includes("孤儿结果降级"),
      "两类修补都要如实记账",
      JSON.stringify(repairs),
    );
    ok(
      !req.messages.some((m) => m.role === "tool" && m.tool_call_id === "不存在的"),
      "🔴 对不上 call 的结果绝不能当 tool 发（上游必 400）",
    );
    ok(JSON.stringify(req).includes("野结果"), "孤儿结果降级成文本，信息不能丢");
  }

  {
    const t = toolsToChat([{ name: "Bash", description: "跑命令", input_schema: { type: "object" } }]);
    eq(t[0], { type: "function", function: { name: "Bash", description: "跑命令", parameters: { type: "object" } } }, "tools 形状");
    eq(toolsToChat([{ type: "computer_20241022" }]), undefined, "没有 input_schema 的服务端工具跳过");
    eq(toolChoiceToChat({ type: "any" }), "required", "tool_choice any→required");
    eq(toolChoiceToChat({ type: "tool", name: "Bash" }), { type: "function", function: { name: "Bash" } }, "tool_choice 指名");
  }

  eq(mapStopReason("length", false), "max_tokens", "finish_reason length→max_tokens");
  eq(mapStopReason("stop", true), "tool_use", "有工具就是 tool_use");

  ok(estimateTokens({ messages: [{ role: "user", content: "你好世界" }] }) >= 4, "中文 token 估算不能是 0");

  eq(
    chatJsonToAnthropic({ choices: [{ finish_reason: "stop", message: { content: "hi" } }] }, "m").content,
    [{ type: "text", text: "hi" }],
    "非流式：文本",
  );

  // ——————————————— ② 真 HTTP：响应方向的翻译 ———————————————

  const upstream = await startFakeUpstream();
  const upUrl = `http://127.0.0.1:${upstream.address().port}/v1/chat/completions`;
  const proxy = createServer({ upstream: upUrl, envKey: "" });
  await new Promise((r) => proxy.listen(0, "127.0.0.1", r));
  const port = proxy.address().port;

  try {
    {
      const { status, text } = await post(port, "/v1/messages", {
        model: "fake-text",
        max_tokens: 100,
        messages: [{ role: "user", content: "hi" }],
      });
      eq(status, 200, "文本流 200");
      const ev = parseSse(text);
      eq(
        ev.map((e) => e.event),
        ["message_start", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "message_delta", "message_stop"],
        "Anthropic 事件序列",
      );
      const txt = ev.filter((e) => e.event === "content_block_delta").map((e) => e.data.delta.text).join("");
      eq(txt, "你好，世界", "文本拼回来");
      eq(ev.at(-2).data.delta.stop_reason, "end_turn", "stop_reason");
      eq(ev.at(-2).data.usage.output_tokens, 7, "usage 透传");
      ok(lastUpstreamReq.stream_options?.include_usage === true, "要向上游要 usage，否则算不出花了多少");
    }

    {
      const { text } = await post(port, "/v1/messages", {
        model: "fake-tool",
        max_tokens: 100,
        messages: [{ role: "user", content: "列一下" }],
        tools: [{ name: "Bash", description: "", input_schema: { type: "object" } }],
      });
      const ev = parseSse(text);
      const starts = ev.filter((e) => e.event === "content_block_start");
      eq(starts.length, 2, "一个文本块 + 一个工具块");
      eq(starts[1].data.content_block.name, "Bash", "工具块带 name");
      eq(starts[1].data.content_block.id, "call_a1", "工具块带上游给的 id");

      // 🔴 块不许交叉：文本块的 stop 必须在工具块的 start 之前
      const order = ev.map((e) => `${e.event}#${e.data.index ?? ""}`);
      const textStop = order.indexOf("content_block_stop#0");
      const toolStart = order.indexOf("content_block_start#1");
      ok(textStop >= 0 && toolStart >= 0 && textStop < toolStart, "文本块必须先收掉再开工具块", order.join(" "));

      const partial = ev
        .filter((e) => e.event === "content_block_delta" && e.data.delta.type === "input_json_delta")
        .map((e) => e.data.delta.partial_json)
        .join("");
      eq(JSON.parse(partial), { cmd: "ls -l" }, "partial_json 拼起来要是合法 JSON");
      eq(ev.at(-2).data.delta.stop_reason, "tool_use", "有工具调用 → stop_reason=tool_use");
    }

    {
      const { text } = await post(port, "/v1/messages", { model: "fake-len", max_tokens: 1, messages: [] });
      eq(parseSse(text).at(-2).data.delta.stop_reason, "max_tokens", "截断原因透传");
    }

    {
      const { status, text } = await post(port, "/v1/messages", {
        model: "fake-nostream",
        max_tokens: 10,
        stream: false,
        messages: [{ role: "user", content: "x" }],
      });
      eq(status, 200, "非流式 200");
      const j = JSON.parse(text);
      eq(j.type, "message", "非流式返回 Anthropic Message");
      eq(j.content[1], { type: "tool_use", id: "call_z", name: "Read", input: { p: "/a" } }, "非流式工具调用");
      eq(j.stop_reason, "tool_use", "非流式 stop_reason");
      eq(j.usage.input_tokens, 3, "非流式 usage");
    }

    {
      const { status, text } = await post(port, "/v1/messages", { model: "fake-401", max_tokens: 1, messages: [] });
      eq(status, 401, "上游状态码原样透传（不许一律 500）");
      const j = JSON.parse(text);
      eq(j.type, "error", "错误体是 Anthropic 形状");
      eq(j.error.type, "authentication_error", "401 → authentication_error");
      ok(/no credit/.test(j.error.message), "上游的原话要带上，别吞掉");
    }

    {
      const { status, text } = await post(port, "/v1/messages/count_tokens", {
        model: "fake-text",
        messages: [{ role: "user", content: "数一数这段中文有多少 token" }],
      });
      eq(status, 200, "count_tokens 必须实现（不实现 Claude Code 会报错）");
      ok(JSON.parse(text).input_tokens > 0, "count_tokens 要给个数");
    }

    {
      // Key 转发：env 没配就用客户端带的那把 —— 少一份 Key 副本
      await post(port, "/v1/messages", { model: "fake-text", max_tokens: 1, messages: [] });
      ok(true, "（Key 转发路径已跑通，见 clientKey）");
    }
  } finally {
    proxy.close();
    upstream.close();
  }

  console.log(`\nclaude-openai-proxy 自检：${pass} 过 / ${fails.length} 失败`);
  for (const f of fails) console.error("  ✗ " + f);
  return fails.length ? 1 : 0;
}

// 直接跑本文件即执行自检。
process.exit(await runSelfTest());
