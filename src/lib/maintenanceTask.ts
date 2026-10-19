/**
 * 维护任务包 —— 集中纯函数脱敏 + 统一任务格式（装机失败 / 工具故障 → 轻助手或外部 AI 交接）。
 *
 * 🔴 安全红线（需求榜 2026-10-19）：
 *  · 绝不把 Key 写进提示词 / 剪贴板 / localStorage / 命令行
 *  · 剪贴板里的内容必须已脱敏（sk- 密钥、Authorization 头、充值 URL query、用户路径）
 *  · 只收「目标 / 失败原文 / 已做步骤 / 验证与回退」，不复制用户历史全量数据
 *
 * 与 Wizard（装机失败）和 MaintenanceAssistant（复制维护任务）共用同一份格式，
 * 不允许各写一份 —— 同一事实存在几份就会漂移几份（宪法第 8 条）。
 */

const MAX_LENGTH = 4000;

/** 与 providerRepairPrompt / doctorPrompt 同口径的脱敏规则，集中在此一份。 */
const API_KEY = /sk-[A-Za-z0-9_-]{8,}/g;
const BEARER = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi;
const AUTH_HEADER = /\b(Authorization\s*[:=]\s*)([^\s\n\r]+)/gi;
const PATH_WITH_USERNAME = /C:\\Users\\[^\\\s"'`]+/gi;
const MAC_HOME = /\/(?:Users|home)\/[^/\s"'`]+/g;
/** 充值 URL 上可能带 key / token / sign / ticket 之类的 query —— 交接文本里全部抹掉。 */
const RECHARGE_QUERY = /([?&](?:key|token|sign|ticket|auth|api_key|apikey|sk)=)[^&\s"'`]+/gi;

/** 集中脱敏：密钥 / Authorization / 充值 URL query / 用户路径。纯函数，无 IO。 */
export function redactSecrets(value: string): string {
  return value
    .replace(RECHARGE_QUERY, "$1****")
    .replace(AUTH_HEADER, "$1****")
    .replace(BEARER, "Bearer ****")
    .replace(API_KEY, "sk-****")
    .replace(PATH_WITH_USERNAME, "~")
    .replace(MAC_HOME, "~");
}

function shorten(value: string, maxLength: number = MAX_LENGTH): string {
  const t = value.trim();
  if (t.length <= maxLength) return t;
  return maxLength <= 1 ? "…".slice(0, maxLength) : `${t.slice(0, maxLength - 1)}…`;
}

export type MaintenanceTaskInput = {
  /** 目标：装的是什么 / 修的是什么（工具名、组件名、一句话场景）。 */
  target: string;
  /** 失败原文：报错原文 / 向导里的失败描述（会脱敏）。 */
  failure: string;
  /** 已做步骤：用户或程序已经做过的动作（数组或整段文本）。 */
  steps?: string[] | string;
  /** 验证说明：怎么算修好了。 */
  verify?: string;
  /** 回退说明：万一更糟怎么退回。 */
  rollback?: string;
};

/**
 * 统一任务包文本。装机向导失败与维护助手「复制维护任务」都用它，
 * 交给轻助手（draft）或复制给外部 AI 时内容一致。
 */
export function buildMaintenanceTask(input: MaintenanceTaskInput): string {
  const steps = Array.isArray(input.steps)
    ? input.steps.map((s, i) => `${i + 1}. ${s}`).join("\n")
    : (input.steps ?? "").trim();
  const parts = [
    "【U-King 维护任务】",
    `目标：${redactSecrets(input.target)}`,
    `失败原文：${redactSecrets(input.failure)}`,
    steps ? `已做步骤：\n${redactSecrets(steps)}` : "",
    input.verify ? `验证说明：${redactSecrets(input.verify)}` : "",
    input.rollback ? `回退说明：${redactSecrets(input.rollback)}` : "",
    "说明：密钥与凭据已脱敏，不要向用户索要 Key，也不要把 Key 发到对话里。",
  ].filter(Boolean);
  return shorten(parts.join("\n"));
}
