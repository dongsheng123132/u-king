/**
 * 供应商「展示层」元数据 —— logo 键 + 分组（模型厂商 / 模型平台）。
 *
 * 单一职责：只按**解析后的接口主机名白名单**判断品牌，不碰 `ProviderTemplate` 本身
 * （UI 字段不写进三份安装模板，见 `providerTemplates.ts` 头部注释里「同一份数据抄了三遍」
 * 的闸门）。本地模板与远程热下发模板经过同一个 `resolveProviderPresentation`，
 * 因此远程旧结构不会覆盖本地 UI 展示。
 *
 * 分组依据（2026-09-06 设计评审裁决）：Groq / 魔搭 ModelScope / SiliconFlow（硅基流动）/
 * OpenRouter / OpenCode Zen / iFlow / B.ai / APIMart 归「模型平台」（聚合/托管推理/中转）；
 * 其余官方直连品牌归「模型厂商」。未识别的主机名一律 `unknown`，不擅自归为官方，也不丢弃
 * （远程新增模板可能是还没来得及补规则的新家）。
 *
 * 只认主机名，不认用户可改的展示名称或模型 id —— 用户把「OpenAI 官方」改名成别的字符串，
 * 图标仍应正确（见 ⑥ 两处接入 Logo 的验收标准）。
 */

/**
 * 输入值只要求 `openai_base` / `anthropic_base` 两个字段，可选可空 —— 这样本地模板
 * （`ProviderTemplate`，`anthropic_base?: string`）和已保存供应商（`ProviderPreset`，
 * `anthropic_base: string | null`）可以共用同一个解析函数，不用互相转换类型。
 */
export type ProviderPresentationInput = {
  openai_base?: string | null;
  anthropic_base?: string | null;
};

/** 明确枚举：24 个模板对应的 24 个键，外加 `unknown` 兜底。 */
export type ProviderLogoKey =
  | "openai"
  | "deepseek"
  | "zhipu"
  | "kimi"
  | "xiaomi-mimo"
  | "bailian"
  | "volcengine"
  | "siliconflow"
  | "openrouter"
  | "opencode"
  | "hunyuan"
  | "zai"
  | "minimax"
  | "qianfan"
  | "stepfun"
  | "longcat"
  | "gemini"
  | "xai"
  | "mistral"
  | "groq"
  | "iflow"
  | "modelscope"
  | "bai"
  | "apimart"
  | "unknown";

export type ProviderGroup = "vendor" | "platform" | "unknown";

export type ProviderPresentation = {
  logo: ProviderLogoKey;
  group: ProviderGroup;
};

type HostRule = {
  /** 命中任一主机名即算这一条（同一家可能 openai_base / anthropic_base 用不同子域）。 */
  hosts: string[];
  logo: ProviderLogoKey;
  group: ProviderGroup;
};

/**
 * 24 条模板 → 主机名规则。行号参照 `providerTemplates.ts`（2026-09-06 版本）。
 * `token-plan-cn.xiaomimimo.com` 是小米 MiMo 当前模板用的域；`api.xiaomimimo.com` 是
 * key_hint 里提到的按量付费替换域，提前收进白名单，免得用户手改地址后图标掉回兜底。
 */
const HOST_RULES: HostRule[] = [
  { hosts: ["api.openai.com"], logo: "openai", group: "vendor" },
  { hosts: ["api.deepseek.com"], logo: "deepseek", group: "vendor" },
  { hosts: ["open.bigmodel.cn"], logo: "zhipu", group: "vendor" },
  { hosts: ["api.moonshot.cn"], logo: "kimi", group: "vendor" },
  { hosts: ["token-plan-cn.xiaomimimo.com", "api.xiaomimimo.com"], logo: "xiaomi-mimo", group: "vendor" },
  { hosts: ["dashscope.aliyuncs.com"], logo: "bailian", group: "vendor" },
  { hosts: ["ark.cn-beijing.volces.com"], logo: "volcengine", group: "vendor" },
  { hosts: ["api.siliconflow.cn"], logo: "siliconflow", group: "platform" },
  { hosts: ["openrouter.ai"], logo: "openrouter", group: "platform" },
  { hosts: ["opencode.ai"], logo: "opencode", group: "platform" },
  { hosts: ["tokenhub.tencentmaas.com", "tokenhub-intl.tencentmaas.com"], logo: "hunyuan", group: "vendor" },
  { hosts: ["api.z.ai"], logo: "zai", group: "vendor" },
  { hosts: ["api.minimaxi.com"], logo: "minimax", group: "vendor" },
  { hosts: ["qianfan.baidubce.com"], logo: "qianfan", group: "vendor" },
  { hosts: ["api.stepfun.com"], logo: "stepfun", group: "vendor" },
  { hosts: ["api.longcat.chat"], logo: "longcat", group: "vendor" },
  { hosts: ["generativelanguage.googleapis.com"], logo: "gemini", group: "vendor" },
  { hosts: ["api.x.ai"], logo: "xai", group: "vendor" },
  { hosts: ["api.mistral.ai"], logo: "mistral", group: "vendor" },
  { hosts: ["api.groq.com"], logo: "groq", group: "platform" },
  { hosts: ["apis.iflow.cn"], logo: "iflow", group: "platform" },
  { hosts: ["api-inference.modelscope.cn"], logo: "modelscope", group: "platform" },
  { hosts: ["api.b.ai"], logo: "bai", group: "platform" },
  { hosts: ["api.apimart.ai"], logo: "apimart", group: "platform" },
];

function extractHost(url?: string | null): string | null {
  if (!url) return null;
  try {
    return new URL(url).host.toLowerCase();
  } catch {
    return null;
  }
}

/** 按接口主机名解析展示元数据；未知主机名统一兜底为 `{ logo: "unknown", group: "unknown" }`。 */
export function resolveProviderPresentation(
  value: ProviderPresentationInput
): ProviderPresentation {
  const hosts = [extractHost(value.openai_base), extractHost(value.anthropic_base)].filter(
    (h): h is string => !!h
  );
  if (hosts.length > 0) {
    for (const rule of HOST_RULES) {
      if (hosts.some((h) => rule.hosts.includes(h))) {
        return { logo: rule.logo, group: rule.group };
      }
    }
  }
  return { logo: "unknown", group: "unknown" };
}
