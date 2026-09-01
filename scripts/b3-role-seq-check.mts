// B3 验证：裁剪后消息角色序列（opus 指控：user/user 相邻 + 中段 system 可能 400）
// 场景 1：常规 12 万+ 字符交替会话（客户真实形态）
// 场景 2：超长 system（>119520 字符）挤压说明池 <48 的极端场景
import { trimHistoryForPayload, CHAT_HISTORY_CHAR_BUDGET } from "../src/opencodex/historyTrim.ts";

function check(name: string, msgs: { role: string; content: string }[], system: string) {
  const out = trimHistoryForPayload(msgs, system);
  const roles = out.map((m) => m.role);
  const total = out.reduce((n, m) => n + m.content.length, 0);
  // 断言 1：预算
  const over = total > CHAT_HISTORY_CHAR_BUDGET;
  // 断言 2：user/user 相邻（无 system 分隔的）
  let adj = false;
  for (let i = 1; i < out.length; i++) {
    if (out[i - 1].role === out[i].role && out[i].role !== "system") adj = true;
  }
  // 断言 3：中段 system 出现次数
  const midSystem = out.filter((m, i) => m.role === "system" && i > 0 && i < out.length - 1).length;
  console.log(`[${name}] 消息数=${msgs.length} 裁剪后=${out.length} 总字符=${total}(${total <= CHAT_HISTORY_CHAR_BUDGET ? "<=预算" : "🔥超预算"}) user/user相邻=${adj ? "🔥有" : "无"} 中段system=${midSystem}`);
  console.log(`  角色序列: ${roles.join(" → ")}`);
  if (over || adj) process.exitCode = 1;
}

// 场景 1：200 条交替消息，每条 800 字符 → 16 万字符 > 12 万
const big = [];
for (let i = 0; i < 200; i++) {
  big.push({ role: i % 2 === 0 ? "user" : "assistant", content: `第${i}条消息内容`.padEnd(800, "聊") });
}
check("常规超长交替会话", big, "你是 AI 助手。".padEnd(1500, "系"));

// 场景 1b：尾部最新几条 user 密集（客户实际最后一轮常是多条 user）
const big2 = [];
for (let i = 0; i < 195; i++) {
  big2.push({ role: i % 2 === 0 ? "user" : "assistant", content: `m${i}`.padEnd(800, "x") });
}
big2.push({ role: "user", content: "u195".padEnd(800, "y") });
big2.push({ role: "user", content: "u196".padEnd(800, "y") });
big2.push({ role: "user", content: "最后一条".padEnd(800, "z") });
check("尾部连续user", big2, "system 提示词".padEnd(2000, "s"));

// 场景 2：超长 system 120000 字符占满预算→分支 1；119600 字符→说明池 <48
const hugeSys = "超长系统提示词".padEnd(119_600, "系");
const big3 = [];
for (let i = 0; i < 20; i++) {
  big3.push({ role: i % 2 === 0 ? "user" : "assistant", content: `x${i}`.padEnd(500, "聊") });
}
check("超长system+拥挤预算", big3, hugeSys);

// 场景 2b：超长 system 且所有消息正好铺满（折叠=0、无 note、头尾衔接）
const big4 = [];
for (let i = 0; i < 8; i++) {
  big4.push({ role: i % 2 === 0 ? "user" : "assistant", content: `y${i}`.padEnd(46, "a") });
}
check("超长system+无缝头尾(理论死角)", big4, hugeSys);

console.log(process.exitCode ? "🔥 B3 发现角色序列问题" : "✅ B3 角色序列安全");