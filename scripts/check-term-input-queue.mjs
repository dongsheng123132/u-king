/**
 * U-CLI 输入链路回归：建连前不吞、并发不乱序、失败不静默、重开不串台。
 * 用法：node scripts/check-term-input-queue.mjs
 */
import { readFileSync } from "node:fs";
import { transformWithEsbuild } from "vite";

const source = readFileSync("src/opencodex/term/inputQueue.ts", "utf8");
const built = await transformWithEsbuild(source, "inputQueue.ts", { loader: "ts", format: "esm" });
const mod = await import(`data:text/javascript;base64,${Buffer.from(built.code).toString("base64")}`);
const { createTermInputQueue } = mod;

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const fails = [];

console.log("[1/4] PTY 建好前打的字不能被吞…");
{
  const writes = [];
  const q = createTermInputQueue({ write: async (sid, data) => writes.push([sid, data]), onError: () => {} });
  q.push("Ctrl+");
  q.push("L");
  if (writes.length !== 0 || q.pendingChars() !== 6) fails.push("未连接时输入没有完整留在缓存");
  q.connect("sid-a");
  await tick();
  const text = writes.map((x) => x[1]).join("");
  if (text !== "Ctrl+L") fails.push(`建连后实收 ${JSON.stringify(text)}，预期 \"Ctrl+L\"`);
  else console.log("     ✓ 建连前 6 个字符在建连后完整送达");
}

console.log("[2/4] 快速输入时 IPC 必须严格单飞、原序到达…");
{
  const writes = [];
  let inFlight = 0;
  let maxInFlight = 0;
  const q = createTermInputQueue({
    write: async (_sid, data) => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise((resolve) => setTimeout(resolve, data === "a" ? 20 : 1));
      writes.push(data);
      inFlight -= 1;
    },
    onError: () => {},
  });
  q.connect("sid-order");
  q.push("a");
  q.push("b");
  q.push("c");
  await new Promise((resolve) => setTimeout(resolve, 60));
  if (maxInFlight !== 1) fails.push(`同时飞了 ${maxInFlight} 个 term_write，仍可能乱序`);
  if (writes.join("") !== "abc") fails.push(`快速输入到达顺序是 ${JSON.stringify(writes)}`);
  if (maxInFlight === 1 && writes.join("") === "abc") console.log("     ✓ 最大并发 1，实收 abc");
}

console.log("[3/4] 写入失败必须可见，且不能冒险重试造成双字…");
{
  let attempts = 0;
  const errors = [];
  const q = createTermInputQueue({
    write: async () => {
      attempts += 1;
      throw new Error("session gone");
    },
    onError: (e) => errors.push(String(e)),
  });
  q.connect("sid-dead");
  q.push("x");
  await tick();
  q.push("y");
  await tick();
  if (errors.length !== 1) fails.push(`写入失败提示了 ${errors.length} 次（应恰好一次）`);
  if (attempts !== 1) fails.push(`失败后又尝试了 ${attempts} 次，可能重复发送`);
  if (errors.length === 1 && attempts === 1) console.log("     ✓ 失败提示一次、没有自动重发");
}

console.log("[4/4] 旧 PTY 的残留输入不能灌进重开的新 PTY…");
{
  const writes = [];
  const q = createTermInputQueue({ write: async (sid, data) => writes.push(`${sid}:${data}`), onError: () => {} });
  q.push("old");
  q.disconnect();
  q.connect("sid-new");
  q.push("new");
  await tick();
  if (writes.join("|") !== "sid-new:new") fails.push(`发生串台：${JSON.stringify(writes)}`);
  else console.log("     ✓ 新会话只收到 new");
}

if (fails.length) {
  console.error(`\n❌ ${fails.length} 条不达标：`);
  for (const f of fails) console.error(`  - ${f}`);
  process.exit(1);
}
console.log("\n✅ 输入 FIFO：不吞启动输入、严格保序、失败可见、重开不串台");
