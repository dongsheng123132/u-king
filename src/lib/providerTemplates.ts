/**
 * 「添加供应商」弹窗的预设模板库（小型精选，对齐 cc-switch 的预设网格）。
 *
 * **纯静态数据 = 最低维护成本**：点一个模板就把 baseUrl / 官网 / Key 提示预填好，
 * 用户只需补 API Key。模型不写死复杂清单 —— 给个默认 id，存好后在列表里用「🔄 拉取」
 * 动态选真实模型（见 Manager 的 ModelPicker）。要加新中转，往这个数组追一项即可。
 *
 * 字段含义同 ProviderPreset：
 *  - openai_base：OpenAI 兼容端点（Codex / ClawX / Hermes / 拉模型都用它，多数以 /v1 结尾）
 *  - anthropic_base：Anthropic 原生端点（给 Claude Code 用；只有 OpenAI 兼容就留空）
 *  - model：默认模型 id（占位，存好后可动态换）
 *
 * 2026-08-22：从 11 个扩到 19 个（P3a，客户原话「右侧支持的还不够多、不够全」）。
 * 新加的 8 家（MiniMax 国内站 / 百度千帆 ERNIE / 阶跃星辰 Stepfun / 美团 LongCat /
 * Google Gemini / xAI Grok / Mistral / Groq）每条 base URL 都过了一遍官方文档核实
 * （当天 WebSearch），拿不准有没有原生 Anthropic 端点的一律不填 `anthropic_base` ——
 * 填错会让 Claude Code 那条路直接 404，宁可少一个字段也不要编一个假端点。
 *
 * 2026-08-23：19 → 20（加 OpenCode Zen），同时把 OpenRouter 的默认模型换成 Ox Alpha。
 * 两处都当天用 curl 打过真实接口核过 id，不是抄文档抄来的。
 * ⚠️ 改这里必须同步改 `src-tauri/skills/install-windows.json` 和
 * `website/skills/install-windows.json` 的 `provider_templates`（同一份数据抄了三遍），
 * 闸门 `pnpm check:provider-templates` 会拦。
 */
export type ProviderTemplate = {
  name: string;
  openai_base: string;
  anthropic_base?: string;
  model?: string;
  small_model?: string;
  key_url?: string;
  key_hint?: string;
  website?: string;
};

export const PROVIDER_TEMPLATES: ProviderTemplate[] = [
  {
    name: "OpenAI 官方",
    openai_base: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    key_url: "https://platform.openai.com/api-keys",
    key_hint: "sk- 开头",
    website: "https://platform.openai.com",
  },
  {
    name: "DeepSeek 官方",
    openai_base: "https://api.deepseek.com/v1",
    anthropic_base: "https://api.deepseek.com/anthropic",
    model: "deepseek-v4-flash",
    small_model: "deepseek-v4-flash",
    key_url: "https://platform.deepseek.com/api_keys",
    key_hint: "sk- 开头",
    website: "https://platform.deepseek.com",
  },
  {
    name: "智谱 GLM",
    openai_base: "https://open.bigmodel.cn/api/paas/v4",
    anthropic_base: "https://open.bigmodel.cn/api/anthropic",
    model: "glm-5",
    small_model: "glm-5-flash",
    key_url: "https://www.bigmodel.cn/usercenter/apikeys",
    key_hint: "在「API 密钥」页创建",
    website: "https://open.bigmodel.cn",
  },
  {
    name: "Kimi（月之暗面）",
    openai_base: "https://api.moonshot.cn/v1",
    anthropic_base: "https://api.moonshot.cn/anthropic",
    model: "kimi-k2.6",
    key_url: "https://platform.moonshot.cn/console/api-keys",
    key_hint: "sk- 开头",
    website: "https://platform.moonshot.cn",
  },
  {
    // pc-*** 客户手填的就是这家 —— 他填对了，说明有需求；模板库却没有，说明我们欠一项。
    // ⚠️ 小米的 Key 分两种，base 必须跟 Key 配对：Token Plan 套餐（tp- 开头）走 token-plan-cn；
    // 按量付费（sk- 开头）要把地址改成 https://api.xiaomimimo.com/v1。key_hint 里说清，别让人拿
    // tp- 的 Key 打按量的域名 —— 那个 401 和「Key 错了」长得一模一样。
    name: "小米 MiMo",
    openai_base: "https://token-plan-cn.xiaomimimo.com/v1",
    anthropic_base: "https://token-plan-cn.xiaomimimo.com/anthropic",
    model: "mimo-v2.5",
    key_url: "https://mimo.mi.com",
    key_hint: "Token Plan 套餐 tp- 开头；按量付费 sk- 开头需把地址换成 api.xiaomimimo.com/v1",
    website: "https://mimo.mi.com",
  },
  {
    name: "阿里百炼（通义）",
    openai_base: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    anthropic_base: "https://dashscope.aliyuncs.com/apps/anthropic",
    model: "qwen-plus",
    key_url: "https://bailian.console.aliyun.com/#/api-key",
    key_hint: "按量付费 API Key；Token/Coding Plan 套餐 Key 只能配其专用地址，混用会 401/403",
    website: "https://bailian.console.aliyun.com",
  },
  {
    name: "火山方舟（豆包）",
    openai_base: "https://ark.cn-beijing.volces.com/api/v3",
    model: "doubao-seed-1-6-250615",
    key_url: "https://console.volcengine.com/ark",
    key_hint: "在火山方舟控制台创建",
    website: "https://console.volcengine.com/ark",
  },
  {
    name: "SiliconFlow（硅基流动）",
    openai_base: "https://api.siliconflow.cn/v1",
    model: "deepseek-ai/DeepSeek-V3",
    key_url: "https://cloud.siliconflow.cn/account/ak",
    key_hint: "sk- 开头",
    website: "https://siliconflow.cn",
  },
  {
    // 不用 stealth/preview 模型作默认值：它们可能当天撤下。2026-08-30 无 Key 读取官方
    // /v1/models 后确认该稳定模型在册；仍建议保存后用“拉取模型”按账户权限选择。
    name: "OpenRouter",
    openai_base: "https://openrouter.ai/api/v1",
    anthropic_base: "https://openrouter.ai/api",
    model: "openai/gpt-5.4-mini",
    key_url: "https://openrouter.ai/keys",
    key_hint: "sk-or- 开头",
    website: "https://openrouter.ai",
  },
  {
    // OpenCode Zen（2026-08-23 加）：OpenCode 官方自己的模型网关，OpenAI 兼容。
    // base 取自官方文档的 Endpoints 表（`https://opencode.ai/zen/v1/chat/completions`），
    // 无 Key 直接 GET `/zen/v1/models` 能列出 64 个 id —— 拉取按钮对它天然可用。
    // ⚠️ Ox Alpha 在 Zen 这边**不叫 ox-alpha**，官方表里写的是 `x-preview-f-free`
    // （显示名 "Ox Alpha Free"）。照 OpenRouter 那个 id 填过来会 404。
    // 没查到原生 Anthropic 端点，所以不填 anthropic_base（同本文件规矩）。
    name: "OpenCode Zen",
    openai_base: "https://opencode.ai/zen/v1",
    model: "x-preview-f-free",
    key_url: "https://opencode.ai/auth",
    key_hint: "登录 opencode.ai 后在 Zen 页面复制",
    website: "https://opencode.ai/docs/zen/",
  },
  {
    name: "腾讯混元",
    // 旧混元平台将在 2026-09-30 停服；新购和新模型已迁 TokenHub。国内/国际 Key 不互通。
    openai_base: "https://tokenhub.tencentmaas.com/v1",
    anthropic_base: "https://tokenhub.tencentmaas.com/v1",
    model: "hy3",
    key_url: "https://tokenhub.tencentmaas.com",
    key_hint: "TokenHub 国内站 Key；国际站请改为 tokenhub-intl.tencentmaas.com/v1，二者 Key 不互通",
    website: "https://tokenhub.tencentmaas.com",
  },
  {
    name: "智谱 GLM 海外（z.ai）",
    openai_base: "https://api.z.ai/api/paas/v4",
    anthropic_base: "https://api.z.ai/api/anthropic",
    model: "glm-4.6",
    key_url: "https://z.ai/manage-apikey/apikey-list",
    key_hint: "在 z.ai 创建 API Key",
    website: "https://z.ai",
  },
  // 2026-08-22 补的 8 家（P3a：「右侧支持的还不够多」）。每条 openai_base / anthropic_base
  // 都过了一遍官方文档核实（WebSearch，当天）——**没查到原生 Anthropic 端点的一律不填
  // anthropic_base**，填错会让 Claude Code 那条路直接 404（同本文件其它条目的规矩）。
  // model 一律取官方 quickstart 示例里出现的那个 id，不是瞎猜的「最新」。
  {
    // 国内站用 minimaxi.com（比国际站 minimax.io 多一个 i），这是 MiniMax 自己反复强调的坑。
    name: "MiniMax（国内站）",
    openai_base: "https://api.minimaxi.com/v1",
    model: "MiniMax-M2.7",
    key_url: "https://platform.minimaxi.com",
    key_hint: "「接口密钥」页创建；国内站按量付费 Key，跟 Token Plan 订阅 Key 不通用",
    website: "https://platform.minimaxi.com",
  },
  {
    // V2 接口协议对齐 OpenAI，鉴权已升级成 bce-v3/ALTAK- 开头的 Bearer Token（不再是老的 AK/SK）。
    name: "百度千帆 ERNIE",
    openai_base: "https://qianfan.baidubce.com/v2",
    anthropic_base: "https://qianfan.baidubce.com/anthropic",
    model: "ernie-4.5-turbo-128k",
    key_url: "https://console.bce.baidu.com/qianfan/overview",
    key_hint: "bce-v3/ALTAK- 开头，控制台直接创建（V2 版，永久有效）",
    website: "https://qianfan.baidubce.com",
  },
  {
    name: "阶跃星辰 Stepfun",
    openai_base: "https://api.stepfun.com/v1",
    model: "step-3.5-flash",
    key_url: "https://platform.stepfun.com/interface-key",
    key_hint: "国内站需 +86 手机号验证",
    website: "https://platform.stepfun.com",
  },
  {
    // 官方 quickstart 的 Python 示例 base_url 就是不带 /v1 的 .../openai（网关自己处理版本号），
    // 照抄官方示例，别自己加一段。
    name: "美团 LongCat",
    openai_base: "https://api.longcat.chat/openai",
    anthropic_base: "https://api.longcat.chat/anthropic",
    model: "LongCat-2.0",
    key_url: "https://longcat.chat/platform/",
    key_hint: "注册后自动生成一把 default Key，同一把 Key 两种协议都能用",
    website: "https://longcat.chat/platform/",
  },
  {
    // 官方文档明确写着结尾的 /openai/ 斜杠不能删；只有 OpenAI 兼容层，没有原生 Anthropic 端点。
    name: "Google Gemini",
    openai_base: "https://generativelanguage.googleapis.com/v1beta/openai/",
    model: "gemini-3.5-flash",
    key_url: "https://aistudio.google.com/apikey",
    key_hint: "AI Studio 创建，免信用卡",
    website: "https://ai.google.dev/gemini-api/docs/openai",
  },
  {
    name: "xAI Grok",
    openai_base: "https://api.x.ai/v1",
    model: "grok-4.6",
    key_url: "https://console.x.ai",
    key_hint: "console.x.ai 创建（别跟 grok.com 聊天页搞混）",
    website: "https://x.ai/api",
  },
  {
    name: "Mistral",
    openai_base: "https://api.mistral.ai/v1",
    model: "mistral-large-latest",
    key_url: "https://console.mistral.ai",
    key_hint: "La Plateforme 控制台创建，有限量免费额度",
    website: "https://docs.mistral.ai",
  },
  {
    // LPU 硬件，主打快，没有原生 Anthropic 端点。
    name: "Groq",
    openai_base: "https://api.groq.com/openai/v1",
    model: "openai/gpt-oss-120b",
    key_url: "https://console.groq.com/keys",
    key_hint: "gsk_ 开头，免信用卡有免费额度",
    website: "https://console.groq.com",
  },
];
