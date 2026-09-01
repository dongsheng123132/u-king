#!/usr/bin/env node
/** AI 设置供应商修复提示词跑道：验证故障事实、脱敏、动作引导与长度上限。 */
import { buildProviderRepairPrompt } from "../src/lib/providerRepairPrompt.ts";

const fails = [];
const ok = (name, condition, detail = "") => {
  if (condition) {
    process.stderr.write(`  ✓ ${name}\n`);
  } else {
    fails.push(`${name}${detail ? ` —— ${detail}` : ""}`);
    process.stderr.write(`  ✗ ${name} ${detail}\n`);
  }
};

const prompt = buildProviderRepairPrompt({
  providerName: "SiliconFlow",
  baseUrl: "https://api.siliconflow.cn/v1",
  model: "deepseek-ai/DeepSeek-V3",
  target: "codex",
  error: "响应不是 JSON： Not Found；日志在 C:\\Users\\张三\\AppData\\Local；key=sk-abc12345",
});

ok("包含端点与报错", prompt.includes("https://api.siliconflow.cn/v1") && prompt.includes("响应不是 JSON： Not Found"));
ok("API Key 已脱敏", !prompt.includes("sk-abc12345") && prompt.includes("sk-****"));
ok("客户用户名路径已脱敏", !prompt.includes("张三") && prompt.includes("~\\AppData\\Local"));
ok("包含供应商写入与接管动作引导", prompt.includes("runtime.provider.save") && prompt.includes("runtime.driver.apply") && prompt.includes("runtime.provider.effective"));
ok("提示词长度受控", prompt.length < 1500, `实际 ${prompt.length}`);
ok("明确禁止索要 Key", prompt.includes("不要向用户索要 Key"));

if (fails.length) {
  console.log(JSON.stringify({ ok: false, failed: fails }, null, 1));
  process.exit(1);
}
console.log(JSON.stringify({ ok: true, checks: 6 }));
