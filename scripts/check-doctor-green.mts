/**
 * 体检全绿判定的确定性用例（npx tsx scripts/check-doctor-green.mts）。
 * 覆盖 first-principles 评审 2026-09-06 ②2 点名的输入类别：
 * BYOK 未充值、有新版/更新检查失败、可选工具缺席、必要运行时缺失、已装工具没接模型、零工具。
 */
import assert from "node:assert/strict";
import { isAllGreen, type DoctorReport } from "../src/lib/doctorHealth";

const probe = (found: boolean) => ({ found, version: found ? "1.0.0" : null });
const tool = (installed: boolean, state: "ready" | "idle" | "self-managed" | "absent") => ({
  target: "t",
  label: "t",
  installed,
  state,
  model: state === "ready" ? "m" : null,
  can_auto_fix: false,
});

const base: DoctorReport = {
  update: { current: "1.0", latest: "1.0", has_update: false, checked_ok: true },
  wallet: { charged: true, low_balance: false, balance: null, recharge_url: "" },
  stack: { node: probe(true), npm: probe(true), git: probe(true), portable_node: false, system_proxy: null },
  tools: [tool(true, "ready")],
};
const withOverride = (patch: Partial<DoctorReport>): DoctorReport => ({ ...base, ...patch });

// 健康机器全绿。
assert.equal(isAllGreen(base), true);
// BYOK：钱包没充值 / 余额低 / 钱包探测不到，都不该拉黄。
assert.equal(isAllGreen(withOverride({ wallet: { charged: false, low_balance: true, balance: null, recharge_url: "" } })), true);
assert.equal(isAllGreen(withOverride({ wallet: null })), true);
// 有新版 / 更新检查失败是独立信息，不算故障。
assert.equal(isAllGreen(withOverride({ update: { current: "1.0", latest: "2.0", has_update: true, checked_ok: true } })), true);
assert.equal(isAllGreen(withOverride({ update: { current: "1.0", latest: "", has_update: false, checked_ok: false, fail_reason: "net" } })), true);
// 可选工具没装不算故障；用户自管算配好。
assert.equal(isAllGreen(withOverride({ tools: [tool(true, "self-managed"), tool(false, "absent")] })), true);
// 已装工具没接模型 = 待处理。
assert.equal(isAllGreen(withOverride({ tools: [tool(true, "ready"), tool(true, "idle")] })), false);
// 一个能用的工具都没有 = 不能算「环境正常」。
assert.equal(isAllGreen(withOverride({ tools: [tool(false, "absent")] })), false);
// 必要运行时缺失（探测失败不涂成健康）。
assert.equal(isAllGreen(withOverride({ stack: { ...base.stack, node: probe(false) } })), false);
assert.equal(isAllGreen(withOverride({ stack: { ...base.stack, git: probe(false) } })), false);

console.log("check-doctor-green: 10 assertions ok");
