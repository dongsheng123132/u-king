/**
 * 英文覆盖字典总入口 —— 合并各模块的 `en/*.ts`（每个模块独占一个文件，避免并行改动冲突）。
 * 新增一个模块的翻译：在 `en/` 下建 `<模块>.ts` 导出一个 `Record<string,string>`，再在这里 import + spread。
 * key = 组件里的中文原文；value = English。缺失的 key 在运行时自动回退中文。
 *
 * 重复 key：多个模块出现同一句中文时后 spread 的覆盖前者 —— 术语表已统一，同一中文应映射同一英文。
 */
import { backfill } from "./en/backfill";
import { backfill20260820 } from "./en/backfill-20260820";
import { sidebar } from "./en/sidebar";
import { app } from "./en/app";
import { onboarding } from "./en/onboarding";
import { settings } from "./en/settings";
import { media } from "./en/media";
import { tools } from "./en/tools";
import { codex } from "./en/codex";
import { misc } from "./en/misc";
import { workbench } from "./en/workbench";
import { panels } from "./en/panels";
import { redline } from "./en/redline";
import { rtk } from "./en/rtk";
import { feedback } from "./en/feedback";
import { identity } from "./en/identity";
import { localllm } from "./en/localllm";
import { deviceWallet } from "./en/device-wallet";
import { englishUi } from "./en/english-ui";
import { reel } from "./en/reel";
import { team } from "./en/team";

export const EN: Record<string, string> = {
  // 🔴 backfill 必须**第一个** spread：它是机器批量补的历史欠账，
  // 后面那些按模块手工维护的字典要能覆盖它（同 key 后者胜）。
  ...backfill,
  ...backfill20260820,
  ...sidebar,
  ...app,
  ...onboarding,
  ...settings,
  ...media,
  ...tools,
  ...codex,
  ...misc,
  ...workbench,
  ...panels,
  ...redline,
  ...rtk,
  ...localllm,
  ...feedback,
  ...identity,
  ...deviceWallet,
  ...reel,
  ...team,
  // Dynamic data-table strings and compact English copy intentionally win last.
  ...englishUi,
};
