/** AI 设置供应商连通故障 → u-chat 的纯文本交接提示词（不落盘，不带 API Key）。 */
export type ProviderRepairPromptInput = {
  providerName: string;
  baseUrl: string;
  model: string;
  target?: string;
  error: string;
};

const MAX_LENGTH = 1500;
const PATH_WITH_USERNAME = /C:\\Users\\[^\\\s"'`]+/gi;
const API_KEY = /sk-[A-Za-z0-9]{8,}/g;

function redact(value: string): string {
  return value.replace(PATH_WITH_USERNAME, "~").replace(API_KEY, "sk-****");
}

function shorten(value: string, maxLength: number): string {
  if (value.length <= maxLength) return value;
  return maxLength <= 1 ? "…".slice(0, maxLength) : `${value.slice(0, maxLength - 1)}…`;
}

/** 将一条已脱敏的连通失败压成可直接投递给 u-chat 的修复请求。 */
export function buildProviderRepairPrompt(input: ProviderRepairPromptInput): string {
  const facts = [
    `供应商名：${redact(input.providerName)}`,
    `baseUrl：${redact(input.baseUrl)}`,
    `模型：${redact(input.model)}`,
    input.target ? `目标工具：${redact(input.target)}` : "",
    `报错：${redact(input.error)}`,
  ].filter(Boolean);
  const instructions = [
    "这是 U-King「AI 设置」里用户自配供应商后的连通故障，请诊断并给出修复。",
    ...facts,
    "Key 已在本机该供应商下保存（掩码），不要向用户索要 Key，也不要让用户把 Key 发到对话里。",
    "先调只读动作 runtime.checkup.inspect（工具配置体检）与 runtime.provider.effective（回读该工具实际生效的 base_url/model）。",
    "诊断出根因后，如需改配置，调 runtime.provider.save（保存供应商）→ runtime.driver.apply（接管到目标工具）；两者都是写动作会自动弹确认框，向用户解释每一步再执行。",
    "404/Not Found 类报错优先查 baseUrl 路径层（少/多 /v1、域名错），不是模型 id 或 Key。",
    "请先给一句结论（哪里错了），再动手修。",
  ];
  return shorten(instructions.join("\n"), MAX_LENGTH);
}
