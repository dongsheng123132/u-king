/**
 * 闸门：配置目标（config_target）在三处必须一致 ——
 *   ① `tools.rs::TOOL_SPECS[].config_target`（工具目录唯一真相源）
 *   ② `providers.rs::apply_provider` 的分派表（真正会写配置的 match 臂）
 *   ③ `Manager.tsx::TOOL_TABS`（AI 设置页的 tab；不该另起第二套工具清单）
 *
 * 为什么要有：`providers.rs` 的契约用例自己承认有个缺口——`APPLY_ALL_TARGETS`
 * 常量跟真正的 `apply_provider` 分派表没有断言相连，「常量写对了、循环体没跟着写」
 * 一类的漂移 cargo test 全绿。这里用源码断言把三条线钉在一起：
 *   · TOOL_SPECS 里每个 config_target 都必须在 apply_provider 里有对应 match 臂
 *     （少一个 = 配置目标对外声明了、真去配时报「未知目标」）；
 *   · APPLY_ALL_TARGETS 必须仍从 TOOL_SPECS 派生（`apply_all_targets_from_specs`），
 *     不许退回手抄字面量；
 *   · Manager 的 TOOL_TABS 每个 target 必须能在 TOOL_SPECS 里找到（tab 不许无中生有），
 *     且 TOOL_LABELS（CustomProviderModal）里有显示名。
 *
 * 顺带钉住 WorkBuddy 的产品定案文案：配置成功的 flash 必须是
 * 「已添加到模型列表，请在WorkBuddy选择」（不许前端另编一套“已切换”话术）。
 *
 * 用法：node scripts/check-provider-config-targets.mjs
 */
import { readFileSync } from "node:fs";

const TOOLS_RS = "src-tauri/src/tools.rs";
const PROVIDERS_RS = "src-tauri/src/providers.rs";
const MANAGER_TSX = "src/Manager.tsx";
const MODAL_TSX = "src/components/CustomProviderModal.tsx";

let bad = 0;
const fail = (msg) => {
  console.error(`\n❌ ${msg}`);
  bad++;
};

const toolsSrc = readFileSync(TOOLS_RS, "utf8");
const providersSrc = readFileSync(PROVIDERS_RS, "utf8");
const managerSrc = readFileSync(MANAGER_TSX, "utf8");
const modalSrc = readFileSync(MODAL_TSX, "utf8");

// ① TOOL_SPECS 的 config_target 全集（含重复目标去重后的集合）。
const specTargets = [...toolsSrc.matchAll(/config_target:\s*Some\("([^"]+)"\)/g)].map((m) => m[1]);
const specTargetSet = new Set(specTargets);
if (specTargetSet.size === 0) fail(`${TOOLS_RS} 里没解析出任何 config_target`);

// ② apply_provider 分派表的 match 臂（`"target" => {` / `"target" =>` 字面量）。
// 只切这一个函数体（到 ApplyAllResult 定义为止），免得把后面 effective_config 等
// match 的臂混进来当成分派表。
const applyStart = providersSrc.indexOf("pub fn apply_provider");
const applyEnd = providersSrc.indexOf("pub struct ApplyAllResult");
const applyFn = providersSrc.slice(applyStart, applyEnd > applyStart ? applyEnd : undefined);
const dispatch = [...applyFn.matchAll(/^\s*"([a-z0-9-]+)"\s*=>/gm)].map((m) => m[1]);
const dispatchSet = new Set(dispatch);
for (const target of specTargetSet) {
  if (!dispatchSet.has(target)) {
    fail(`TOOL_SPECS 声明的 config_target="${target}" 在 apply_provider 分派表里没有 match 臂`);
  }
}

// ③ APPLY_ALL_TARGETS 仍从 TOOL_SPECS 派生（宪法第 8 条：别再手抄一份）。
if (!providersSrc.includes("apply_all_targets_from_specs")) {
  fail(`${PROVIDERS_RS} 的 APPLY_ALL_TARGETS 不再从 TOOL_SPECS 派生（apply_all_targets_from_specs 不见了）`);
}
if (!providersSrc.includes("pub const APPLY_ALL_TARGETS: &[&str] = &apply_all_targets_from_specs()")) {
  fail("APPLY_ALL_TARGETS 的绑定变了，请同步本闸门并确认仍是「TOOL_SPECS 派生」");
}

// ④ Manager TOOL_TABS：每个 tab target 必须来自 TOOL_SPECS，且有 TOOL_LABELS 显示名。
const tabSection = managerSrc.slice(
  managerSrc.indexOf("const TOOL_TABS"),
  managerSrc.indexOf("const TARGET_TOOL_ID"),
);
const tabTargets = [...tabSection.matchAll(/target:\s*"([a-z0-9-]+)"/g)].map((m) => m[1]);
if (tabTargets.length === 0) fail(`${MANAGER_TSX} 里没解析出 TOOL_TABS`);
for (const target of tabTargets) {
  if (!specTargetSet.has(target)) {
    fail(`Manager TOOL_TABS 的 target="${target}" 在 TOOL_SPECS 里找不到 config_target —— 前端不许另起第二套工具清单`);
  }
}
const labelSection = modalSrc.slice(
  modalSrc.indexOf("export const TOOL_LABELS"),
  modalSrc.indexOf("export type TestResult"),
);
const labelTargets = [...labelSection.matchAll(/^\s*([a-z0-9-]+):/gm)].map((m) => m[1]);
for (const target of tabTargets) {
  if (!labelTargets.includes(target)) {
    fail(`TOOL_LABELS（CustomProviderModal.tsx）缺 "${target}" 的显示名`);
  }
}

// ⑤ WorkBuddy 产品定案文案（配置成功 = 加入模型列表，不是「已切换会话」）。
const WORKBUDDY_NOTE = "已添加到模型列表，请在WorkBuddy选择";
for (const [file, src] of [[PROVIDERS_RS, providersSrc], [MANAGER_TSX, managerSrc]]) {
  if (!src.includes(WORKBUDDY_NOTE)) {
    fail(`${file} 缺 WorkBuddy 定案文案「${WORKBUDDY_NOTE}」`);
  }
}
if (providersSrc.includes('r.workbuddy = Some("已切到')) {
  fail("WorkBuddy 的 ApplyResult 不许写「已切到」——模型由用户在 WorkBuddy 按任务选");
}

if (bad) {
  console.error(`\n共 ${bad} 处不一致。`);
  process.exit(1);
}
console.log(
  `✅ config_target 三处一致：TOOL_SPECS ${specTargetSet.size} 个目标 / apply_provider ${dispatchSet.size} 个臂 / Manager ${tabTargets.length} 个 tab；WorkBuddy 文案已钉住。`,
);
