/** Brand artwork is bundled locally. Sources and trademark attribution are in NOTICE. */
import { useId } from "react";
import claudeLogo from "../assets/logos/claude.svg";
import openaiLogo from "../assets/logos/openai.svg";
// Codex 改内联单色标记（CodexMark，fill=currentColor），不再用硬编码白 fill 的 svg 资源
// —— 白图标在浅色主题浅底上发暗/看不见（客户反馈「codex 图标是暗的」）。
import geminiLogo from "../assets/logos/gemini.svg";
import deepseekLogo from "../assets/logos/deepseek.svg";
import kimiLogo from "../assets/logos/kimi-code.png";
import zhipuLogo from "../assets/logos/zhipu.svg";
import minimaxLogo from "../assets/logos/minimax.svg";
import hermesLogo from "../assets/logos/hermes.png"; // Nous Research 官方吉祥物头像
import piLogo from "../assets/logos/pi.svg"; // pi.dev 官方标记 + 深色圆角底块（官方原件跟系统主题走，直接用会在浅底上消失）
import opencodeLogo from "../assets/logos/opencode.svg"; // 官方 mark，自带 #131010 方形深底（圆角由 ROUNDED_IMG 补）
import doubaoLogo from "../assets/logos/doubao.svg"; // lobe-icons 第三方重绘（MIT），不是豆包官方素材；见 NOTICE
import qwenworkLogo from "../assets/logos/qwenwork.svg";
import workbuddyLogo from "../assets/logos/workbuddy.svg";
import uuRemoteLogo from "../assets/logos/uu-remote.png"; // 官网只有 64px 位图；2x 屏略糊，有矢量源再换

import mimoLogo from "../assets/logos/mimo.svg";
import codebuddyLogo from "../assets/logos/codebuddy.svg";
import qoderLogo from "../assets/logos/qoder.svg";
import grokLogo from "../assets/logos/grok.svg";
import antigravityLogo from "../assets/logos/antigravity.svg";
import museLogo from "../assets/logos/muse.ico";
import obsidianLogo from "../assets/logos/obsidian.svg";
import clawxLogo from "../assets/logos/clawx.svg";

type Props = { tool: string; size?: number; active?: boolean; className?: string };

/** tool/provider id → 官方 logo URL。 */
const LOGO: Record<string, string> = {
  "mimo-code": mimoLogo,
  "codebuddy-code": codebuddyLogo,
  "qoder-cn": qoderLogo,
  "grok-build": grokLogo,
  "muse-code": museLogo,
  "antigravity-cli": antigravityLogo,
  obsidian: obsidianLogo,
  clawx: clawxLogo,
  claude: claudeLogo,
  openai: openaiLogo,
  gemini: geminiLogo,
  deepseek: deepseekLogo,
  kimi: kimiLogo,
  moonshot: kimiLogo,
  glm: zhipuLogo,
  zhipu: zhipuLogo,
  minimax: minimaxLogo,
  hermes: hermesLogo,
  pi: piLogo,
  opencode: opencodeLogo,
  doubao: doubaoLogo,
  qwenwork: qwenworkLogo,
  workbuddy: workbuddyLogo,
  "uu-remote": uuRemoteLogo,
};

/** 素材本身是直角方形底的 logo：用 CSS 补圆角，跟 Codex / pi 的 24% 圆角块保持一致。 */
const ROUNDED_IMG = new Set(["opencode"]);

/** 首字母方块兜底色（仅未知 id 用；有专属图标/logo 的不会走到这里）。 */
const FALLBACK_BG = "#5e6ad2";

/** 内联 SVG 里 <linearGradient> 的 id 必须页面内唯一：App 用 display:none 切页保活，
 *  重复 id 时 url(#id) 可能解析到隐藏页里的那份而不渲染。新增的自家字形用 useId 隔离。 */
function useGradId(prefix: string): string {
  return prefix + useId().replace(/[^a-zA-Z0-9_-]/g, "");
}

function CodexMark({ size }: { size: number }) {
  // Codex 官方标记（LobeHub），单色 fill=currentColor —— 由父层 text-ink-1 上色，
  // 浅色主题呈深色、深色主题呈浅色，两种主题都清晰可见（修「codex 图标发暗」）。
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" fillRule="evenodd" xmlns="http://www.w3.org/2000/svg">
      <path clipRule="evenodd" d="M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z" />
    </svg>
  );
}

function OpenClawLobster({ size }: { size: number }) {
  // 借自 ccswitch 版 claw.svg（龙虾），保留品牌红 + 青眼。
  return (
    <svg width={size} height={size} viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id="uk-lobster" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#ff4d4d" />
          <stop offset="100%" stopColor="#991b1b" />
        </linearGradient>
      </defs>
      <path
        d="M60 10 C30 10 15 35 15 55 C15 75 30 95 45 100 L45 110 L55 110 L55 100 C55 100 60 102 65 100 L65 110 L75 110 L75 100 C90 95 105 75 105 55 C105 35 90 10 60 10Z"
        fill="url(#uk-lobster)"
      />
      <path d="M20 45 C5 40 0 50 5 60 C10 70 20 65 25 55 C28 48 25 45 20 45Z" fill="url(#uk-lobster)" />
      <path d="M100 45 C115 40 120 50 115 60 C110 70 100 65 95 55 C92 48 95 45 100 45Z" fill="url(#uk-lobster)" />
      <path d="M45 15 Q35 5 30 8" stroke="#ff4d4d" strokeWidth="3" strokeLinecap="round" />
      <path d="M75 15 Q85 5 90 8" stroke="#ff4d4d" strokeWidth="3" strokeLinecap="round" />
      <circle cx="45" cy="35" r="6" fill="#050810" />
      <circle cx="75" cy="35" r="6" fill="#050810" />
      <circle cx="46" cy="34" r="2.5" fill="#00e5cc" />
      <circle cx="76" cy="34" r="2.5" fill="#00e5cc" />
    </svg>
  );
}

function XiapanDisk({ size }: { size: number }) {
  // 自家品牌「虾盘云」专属图标：青→蓝数据盘（呼应「盘」）+ 中心龙虾青眼（#00e5cc，与 OpenClawLobster 同色），
  // 自带填充色，深底/浅底都可见；未装时父层 grayscale 灰显。
  return (
    <svg width={size} height={size} viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id="uk-xiapan" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#22d3ee" />
          <stop offset="100%" stopColor="#2563eb" />
        </linearGradient>
      </defs>
      <circle cx="60" cy="60" r="50" fill="url(#uk-xiapan)" />
      <path d="M60 14 A46 46 0 0 1 106 60" stroke="#a5f3fc" strokeWidth="6" strokeLinecap="round" fill="none" opacity="0.85" />
      <circle cx="60" cy="60" r="15" fill="#050810" />
      <circle cx="60" cy="60" r="5.5" fill="#00e5cc" />
    </svg>
  );
}

function HarnessDoctorGlyph({ size }: { size: number }) {
  // 自家「Harness Doctor（AI 工具体检）」：青→蓝渐变圆角块（沿用虾盘云 XiapanDisk 的渐变，同属自家品牌族）
  // + 白色心电脉冲线（体检语义）。自带填充，深/浅底都可见；未装时父层 grayscale 灰显。
  const gid = useGradId("uk-hd-");
  return (
    <svg width={size} height={size} viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id={gid} x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#22d3ee" />
          <stop offset="100%" stopColor="#2563eb" />
        </linearGradient>
      </defs>
      <rect x="8" y="8" width="104" height="104" rx="28" fill={`url(#${gid})`} />
      <path
        d="M18 64 H38 L48 38 L64 92 L76 54 L82 64 H102"
        stroke="#ffffff"
        strokeWidth="8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function Open365Glyph({ size }: { size: number }) {
  // 自家「Open365 电脑管家」：品牌绿 #2f7d4f 渐变盾牌 + 白色对勾（安全护盾语义）。
  // 盾牌加一圈浅绿描边，深色底上轮廓也清楚。
  const gid = useGradId("uk-o365-");
  return (
    <svg width={size} height={size} viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id={gid} x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#4aa877" />
          <stop offset="100%" stopColor="#2f7d4f" />
        </linearGradient>
      </defs>
      <path
        d="M60 8 L102 22 V56 C102 82 84 102 60 112 C36 102 18 82 18 56 V22 Z"
        fill={`url(#${gid})`}
        stroke="#86e0ae"
        strokeOpacity="0.7"
        strokeWidth="3"
        strokeLinejoin="round"
      />
      <path d="M39 60 L54 75 L83 44" stroke="#ffffff" strokeWidth="9" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function UuSwitchGlyph({ size }: { size: number }) {
  // 自家「uu-switch 模型切换器」：靛蓝渐变圆角块 + 上右下左两根反向箭头（来回切换语义）。
  // 不借用上游 cc-switch 的 logo。
  const gid = useGradId("uk-uusw-");
  return (
    <svg width={size} height={size} viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id={gid} x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#818cf8" />
          <stop offset="100%" stopColor="#4f46e5" />
        </linearGradient>
      </defs>
      <rect x="8" y="8" width="104" height="104" rx="28" fill={`url(#${gid})`} />
      <g stroke="#ffffff" strokeWidth="8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M26 42 H92" />
        <path d="M77 28 L93 42 L77 56" />
        <path d="M94 78 H28" />
        <path d="M43 64 L27 78 L43 92" />
      </g>
    </svg>
  );
}

/** tool/provider id 归一（兼容 ToolInfo.id、TUI tag、驱动 id、中文名 几套命名）。 */
function normTool(tool: string): string {
  const t = tool.toLowerCase();
  if (t.includes("claude")) return "claude";
  if (t.includes("codex") || t === "openai") return "codex";
  if (t === "clawx") return "clawx";
  if (t.includes("openclaw") || t.includes("claw")) return "openclaw";
  if (t.includes("hermes")) return "hermes";
  if (t === "dsh" || t.includes("deepseek-harness")) return "deepseek";
  if (t.includes("gemini")) return "gemini";
  if (t.includes("deepseek")) return "deepseek";
  if (t.includes("kimi") || t.includes("moonshot")) return "kimi";
  if (t.includes("glm") || t.includes("zhipu") || t.includes("智谱")) return "glm";
  if (t.includes("minimax")) return "minimax";
  if (t.includes("xiapan") || t.includes("虾盘")) return "xiapan";
  // 以下是后加的工具：短/易撞的 id（pi、opencode、uu-*）用精确匹配 + 少量别名，不用裸 includes ——
  // 「pi」是任何含 pi 字样名字的子串，「opencode」带 code 字样，uu-remote 与 uu-switch 必须分开。
  // 它们都不会被上面的规则先吃掉（已逐个 id 核对）。
  if (t === "pi") return "pi";
  if (t === "opencode" || t === "open-code") return "opencode";
  if (t.includes("doubao") || t.includes("豆包")) return "doubao";
  if (t.includes("qwenwork") || t.includes("千问办公")) return "qwenwork";
  if (t.includes("workbuddy")) return "workbuddy";
  if (t === "uu-remote" || t === "uuremote" || t.includes("uu远程")) return "uu-remote";
  if (t === "uu-switch" || t === "uuswitch") return "uu-switch";
  if (t.includes("harness-doctor")) return "harness-doctor";
  if (t.includes("open365")) return "open365";
  return t;
}

/** 一个工具/模型图标。安装状态由卡片的下载标记表达，品牌素材始终保留原色。 */
export function ToolIcon({ tool, size = 24, active = true, className = "" }: Props) {
  const id = normTool(tool);
  let inner: React.ReactNode;
  if (id === "openclaw") {
    inner = <OpenClawLobster size={size} />;
  } else if (id === "codex") {
    // Codex/ChatGPT 桌面版本来就是「黑色圆角 app 图标 + 白色标记」的观感 —— 用深色圆角块托白 mark，
    // 一看就是那个 app，品牌一致、浅/深主题都好看（之前纯黑 mark 直贴浅底显突兀，用户反馈「怎么是黑的」）。
    inner = (
      <span
        className="inline-flex items-center justify-center rounded-[24%] bg-[#0d0d0f] text-white"
        style={{ width: size, height: size }}
      >
        <CodexMark size={Math.round(size * 0.6)} />
      </span>
    );
  } else if (id === "xiapan") {
    inner = <XiapanDisk size={size} />;
  } else if (id === "harness-doctor") {
    inner = <HarnessDoctorGlyph size={size} />;
  } else if (id === "open365") {
    inner = <Open365Glyph size={size} />;
  } else if (id === "uu-switch") {
    inner = <UuSwitchGlyph size={size} />;
  } else if (LOGO[id]) {
    inner = (
      <img
        src={LOGO[id]}
        width={size}
        height={size}
        alt={id}
        style={{ width: size, height: size, objectFit: "contain", borderRadius: ROUNDED_IMG.has(id) ? "24%" : undefined }}
      />
    );
  } else {
    // 兜底：品牌色首字母方块
    const bg = FALLBACK_BG;
    inner = (
      <span
        style={{ width: size, height: size, background: bg }}
        className="inline-flex items-center justify-center rounded-md text-white font-bold"
      >
        <span style={{ fontSize: size * 0.5 }}>{(tool[0] || "?").toUpperCase()}</span>
      </span>
    );
  }
  return (
    <span
      className={"inline-flex items-center justify-center shrink-0 transition-all " + className}
      data-tool-icon={id}
      data-icon-source={LOGO[id] ? "brand" : "inline"}
      data-installed={active}
      title={tool}
    >
      {inner}
    </span>
  );
}
