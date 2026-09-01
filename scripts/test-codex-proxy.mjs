/**
 * codex-deepseek-proxy 的端到端回归测试（`node scripts/test-codex-proxy.mjs`）。
 *
 * 为什么要它：省钱路由的 502「upstream 400: insufficient tool messages following
 * tool_calls message」是客户实际撞上的故障，根因在 responses→chat 的历史翻译丢了
 * 「带 tool_calls 的 assistant 必须紧跟等量 tool 消息」这条上游硬约束。这个测试起一个
 * **会照着这条约束验参**的假上游（DeepSeek 怎么拒，它就怎么拒），再把**真的**代理脚本
 * 拉起来打过去 —— 不是拿副本测，也不是拿截图猜。
 *
 * 退出码 0=全绿，1=有用例没过。不联外网、不读用户文件、不碰 ~/.codex。
 */
import http from "node:http";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
// 默认测仓库里的真脚本；UKING_PROXY_SCRIPT 可指向别的副本（用来验「改之前确实是红的」）
const PROXY_SCRIPT =
  process.env.UKING_PROXY_SCRIPT || path.join(HERE, "..", "src-tauri", "resources", "codex-deepseek-proxy.mjs");
const UPSTREAM_PORT = 15798; // 避开真代理的 15722，别打扰正在用 codex 的人
const PROXY_PORT = 15799;

// ———————————————— 假上游：照 OpenAI/DeepSeek 的约束验参 ————————————————
let lastMessages = null;

/** 返回违约说明；合法则返回 null。 */
function validate(messages) {
  for (let i = 0; i < messages.length; i++) {
    const m = messages[i];
    if (m.role === "assistant" && m.tool_calls?.length) {
      const ids = m.tool_calls.map((t) => t.id);
      const got = [];
      for (let j = i + 1; j < messages.length && messages[j].role === "tool"; j++) got.push(messages[j].tool_call_id);
      if (got.length < ids.length) {
        return "An assistant message with 'tool_calls' must be followed by tool messages responding to each 'tool_call_id'. (insufficient tool messages following tool_calls message)";
      }
      for (const id of ids) if (!got.includes(id)) return `missing tool message for tool_call_id ${id}`;
    }
    if (m.role === "tool") {
      const prev = messages[i - 1];
      const ok = prev && (prev.role === "tool" || (prev.role === "assistant" && prev.tool_calls?.length));
      if (!ok) return "tool message must follow an assistant message with tool_calls";
      if (!m.tool_call_id) return "tool message requires tool_call_id";
    }
  }
  return null;
}

const upstream = http.createServer((req, res) => {
  let raw = "";
  req.on("data", (c) => (raw += c));
  req.on("end", () => {
    const body = JSON.parse(raw || "{}");
    lastMessages = body.messages;
    const bad = validate(body.messages || []);
    if (bad) {
      res.writeHead(400, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { message: bad, type: "invalid_request_error", code: "invalid_request_error" } }));
      return;
    }
    res.writeHead(200, { "content-type": "text/event-stream" });
    res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: "好的" } }] })}\n\n`);
    res.write("data: [DONE]\n\n");
    res.end();
  });
});

// ———————————————— 用例：都是真实会发生的对话历史 ————————————————
const CASES = [
  {
    name: "单个工具调用（基线，本来就该过）",
    input: [
      { type: "message", role: "user", content: "看下 comp3" },
      { type: "function_call", call_id: "c1", name: "td_query", arguments: "{}" },
      { type: "function_call_output", call_id: "c1", output: "ok" },
    ],
  },
  {
    name: "并行工具调用 —— 什么都没丢也会炸的那条",
    input: [
      { type: "message", role: "user", content: "同时查 comp3 和截图" },
      { type: "function_call", call_id: "c1", name: "td_query", arguments: "{}" },
      { type: "function_call", call_id: "c2", name: "td_screenshot", arguments: "{}" },
      { type: "function_call_output", call_id: "c1", output: "ch0=0.20" },
      { type: "function_call_output", call_id: "c2", output: "shot.png" },
    ],
    expect: (m) => m.filter((x) => x.tool_calls).length === 1 && m.filter((x) => x.role === "tool").length === 2,
    why: "两个并行调用必须合成一条 assistant + 两条 tool",
  },
  {
    name: "并行调用但 output 顺序颠倒",
    input: [
      { type: "function_call", call_id: "c1", name: "a", arguments: "{}" },
      { type: "function_call", call_id: "c2", name: "b", arguments: "{}" },
      { type: "function_call_output", call_id: "c2", output: "second" },
      { type: "function_call_output", call_id: "c1", output: "first" },
    ],
    expect: (m) => {
      const t = m.filter((x) => x.role === "tool");
      return t.length === 2 && t[0].content === "first" && t[1].content === "second";
    },
    why: "按 call_id 配对，不能按位置配对",
  },
  {
    name: "上下文自动压缩后：output 被裁掉只剩 call（截图里那条）",
    input: [
      { type: "message", role: "user", content: "什么情况了" },
      { type: "function_call", call_id: "c9", name: "td_screenshot", arguments: "{}" },
    ],
    expect: (m) => m.some((x) => x.role === "tool" && x.content.includes("不可用")),
    why: "缺的 output 要补占位，而不是让 assistant 裸着",
  },
  {
    name: "用户中途打断 / 拒绝审批",
    input: [
      { type: "function_call", call_id: "c7", name: "shell", arguments: "{}" },
      { type: "message", role: "user", content: "别跑了" },
    ],
  },
  {
    name: "孤儿 output（call 那头被裁掉）",
    input: [
      { type: "function_call_output", call_id: "gone", output: "结果还在" },
      { type: "message", role: "user", content: "继续" },
    ],
    expect: (m) => m.some((x) => x.role === "user" && x.content.includes("结果还在")),
    why: "不能当 tool 发，降级成文本但要保住信息",
  },
  {
    name: "developer 角色归一 + 裸 tool 消息降级",
    input: [
      { type: "message", role: "developer", content: "你是助手" },
      { type: "message", role: "tool", content: "裸的" },
    ],
    // m[0] 是 instructions 自己那条 system，所以按内容找而不是按下标
    expect: (m) => m.some((x) => x.role === "system" && x.content === "你是助手") && m.some((x) => x.role === "user" && x.content === "裸的"),
    why: "DeepSeek 不认 developer；没有 tool_call_id 的 tool 消息一样 400",
  },
];

// ———————————————— 跑 ————————————————
const post = (body) =>
  new Promise((resolve, reject) => {
    const data = JSON.stringify(body);
    const req = http.request(
      { host: "127.0.0.1", port: PROXY_PORT, path: "/v1/responses", method: "POST", headers: { "content-type": "application/json" } },
      (res) => {
        let out = "";
        res.on("data", (c) => (out += c));
        res.on("end", () => resolve({ status: res.statusCode, body: out }));
      },
    );
    req.on("error", reject);
    req.end(data);
  });

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  await new Promise((r) => upstream.listen(UPSTREAM_PORT, "127.0.0.1", r));
  const proxy = spawn(process.execPath, [PROXY_SCRIPT], {
    env: {
      ...process.env,
      UKING_CODEX_PROXY_PORT: String(PROXY_PORT),
      UKING_CODEX_UPSTREAM: `http://127.0.0.1:${UPSTREAM_PORT}/v1/chat/completions`,
      UKING_CODEX_KEY: "test-key",
      UKING_CODEX_MODEL: "deepseek-v4-flash",
    },
    stdio: "ignore",
  });
  await wait(700);

  let failed = 0;
  for (const c of CASES) {
    lastMessages = null;
    let r;
    try {
      r = await post({ instructions: "你是 Codex", input: c.input });
    } catch (e) {
      console.log(`❌ ${c.name}\n   代理没响应: ${e.message}`);
      failed++;
      continue;
    }
    const shape = (lastMessages || []).map((m) => (m.tool_calls ? `assistant<tc×${m.tool_calls.length}>` : m.role)).join(",");
    if (r.status !== 200) {
      console.log(`❌ ${c.name}\n   HTTP ${r.status} ${r.body.slice(0, 160)}\n   发给上游的: ${shape}`);
      failed++;
      continue;
    }
    if (c.expect && !c.expect(lastMessages || [])) {
      console.log(`❌ ${c.name}\n   上游没 400，但形状不对（${c.why}）\n   发给上游的: ${shape}`);
      failed++;
      continue;
    }
    console.log(`✅ ${c.name}\n   → ${shape}`);
  }

  proxy.kill();
  upstream.close();
  console.log(failed ? `\n${failed}/${CASES.length} 个用例没过` : `\n全部 ${CASES.length} 个用例通过`);
  process.exit(failed ? 1 : 0);
}

main();
