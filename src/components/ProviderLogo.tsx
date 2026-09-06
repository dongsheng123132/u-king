/**
 * 供应商品牌图标 —— 只查本地枚举映射，绝不接受远程图片 URL（安全 + 离线可用）。
 *
 * 跟 `ToolIcon` 分开维护：`ToolIcon` 是「工具/客户端」图标（Claude/Codex/ClawX…），
 * 沿用旧的模糊名称匹配；这里是「供应商/接口品牌」图标，只吃 `providerPresentation.ts`
 * 解析出的 `ProviderLogoKey`，不再往 `ToolIcon` 里塞更多供应商名称匹配规则
 * （见设计评审：openai→Codex 误映射、智谱统一 GLM 分不清海外 Z.ai 都是这条路子的坑）。
 *
 * 彩色 SVG（多色/渐变品牌标志）直接 `<img>` 保色；单色描边标志（`mono: true`，取色
 * 依赖 currentColor 或本来就是白色描边）改用 CSS mask + `bg-current`，让图标跟随
 * `text-ink-1` 走深浅主题，不搞「统一反色」这种对所有品牌都套一层滤镜的做法。
 *
 * 未命中枚举（iFlow / B.ai / APIMart 当前没有可用的官方 SVG 源；未知远程模板）
 * 一律退到品牌色字母方块兜底，不硬造 Logo。
 */
import type { ProviderLogoKey } from "../lib/providerPresentation";

// 已有六个候选 SVG，直接复用 `src/assets/logos/`，不复制文件。
import openaiLogo from "../assets/logos/openai.svg";
import deepseekLogo from "../assets/logos/deepseek.svg";
import zhipuLogo from "../assets/logos/zhipu.svg";
import kimiLogo from "../assets/logos/kimi.svg";
import minimaxLogo from "../assets/logos/minimax.svg";
import geminiLogo from "../assets/logos/gemini.svg";

// 新增补齐的品牌素材，来源见各 SVG 文件头部注释（lobehub/lobe-icons，MIT）。
import xiaomiMimoLogo from "../assets/providers/xiaomi-mimo.svg";
import bailianLogo from "../assets/providers/bailian.svg";
import volcengineLogo from "../assets/providers/volcengine.svg";
import siliconflowLogo from "../assets/providers/siliconflow.svg";
import openrouterLogo from "../assets/providers/openrouter.svg";
import opencodeLogo from "../assets/providers/opencode.svg";
import hunyuanLogo from "../assets/providers/hunyuan.svg";
import zaiLogo from "../assets/providers/zai.svg";
import qianfanLogo from "../assets/providers/qianfan.svg";
import stepfunLogo from "../assets/providers/stepfun.svg";
import longcatLogo from "../assets/providers/longcat.svg";
import xaiLogo from "../assets/providers/xai.svg";
import mistralLogo from "../assets/providers/mistral.svg";
import groqLogo from "../assets/providers/groq.svg";
import modelscopeLogo from "../assets/providers/modelscope.svg";

type Props = { logo: ProviderLogoKey; label: string; size?: number; className?: string };

type LogoEntry = { src: string; mono?: boolean };

/** `mono: true` = 单色描边标志（源文件用 currentColor 或纯白），走 CSS mask 适配主题。 */
const LOGOS: Partial<Record<ProviderLogoKey, LogoEntry>> = {
  openai: { src: openaiLogo, mono: true },
  deepseek: { src: deepseekLogo },
  zhipu: { src: zhipuLogo },
  kimi: { src: kimiLogo, mono: true },
  "xiaomi-mimo": { src: xiaomiMimoLogo, mono: true },
  bailian: { src: bailianLogo },
  volcengine: { src: volcengineLogo },
  siliconflow: { src: siliconflowLogo },
  openrouter: { src: openrouterLogo },
  opencode: { src: opencodeLogo, mono: true },
  hunyuan: { src: hunyuanLogo },
  zai: { src: zaiLogo, mono: true },
  minimax: { src: minimaxLogo },
  qianfan: { src: qianfanLogo },
  stepfun: { src: stepfunLogo },
  longcat: { src: longcatLogo },
  gemini: { src: geminiLogo },
  xai: { src: xaiLogo, mono: true },
  mistral: { src: mistralLogo },
  groq: { src: groqLogo, mono: true },
  modelscope: { src: modelscopeLogo },
};

/** 字母兜底方块的背景色；iFlow/B.ai/APIMart 当前没有可用官方 SVG，未知远程模板一律落这里。 */
const FALLBACK_BG: Record<string, string> = {
  iflow: "#7c3aed",
  bai: "#0891b2",
  apimart: "#ea580c",
  unknown: "#5e6ad2",
};

/** 供应商品牌图标：只查本地枚举，未知值退字母兜底。 */
export function ProviderLogo({ logo, label, size = 24, className = "" }: Props) {
  const entry = LOGOS[logo];
  if (entry) {
    if (entry.mono) {
      return (
        <span
          title={label}
          className={"inline-block shrink-0 bg-current text-ink-1 " + className}
          style={{
            width: size,
            height: size,
            // 必须带双引号：Vite 会把小 SVG 内联成含单引号的 data URI，
            // 裸 url(...) 遇到引号即解析失败，mask 失效变实心色块（2026-09-06 真机复现）。
            WebkitMaskImage: `url("${entry.src}")`,
            maskImage: `url("${entry.src}")`,
            WebkitMaskSize: "contain",
            maskSize: "contain",
            WebkitMaskRepeat: "no-repeat",
            maskRepeat: "no-repeat",
            WebkitMaskPosition: "center",
            maskPosition: "center",
          }}
        />
      );
    }
    return (
      <img
        src={entry.src}
        width={size}
        height={size}
        alt={label}
        title={label}
        className={"shrink-0 " + className}
        style={{ width: size, height: size }}
      />
    );
  }
  const bg = FALLBACK_BG[logo] ?? FALLBACK_BG.unknown;
  return (
    <span
      title={label}
      style={{ width: size, height: size, background: bg }}
      className={"inline-flex items-center justify-center shrink-0 rounded-md text-white font-bold " + className}
    >
      <span style={{ fontSize: size * 0.5 }}>{(label[0] || "?").toUpperCase()}</span>
    </span>
  );
}
