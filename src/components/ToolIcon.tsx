/**
 * 工具/模型品牌图标 —— 用官方 logo（SVG 资源，Vite 打包为 URL，<img> 渲染）。
 *
 * 着色规则：彩色 = 已装/可用；灰色 = 未装（CSS grayscale + 降透明度，无需备两套素材）。
 * OpenClaw 没有现成官方 SVG，用内联龙虾（品牌红）。其余用官方 logo。
 *
 * 覆盖：claude / codex(LobeHub 官方标记) / gemini / deepseek / kimi(moonshot) / glm(zhipu) /
 *       minimax / hermes(Nous Research 官方吉祥物) / openclaw(clawx) / xiapan(自家品牌盘)。
 *       未知 → 首字母方块兜底。
 */
import claudeLogo from "../assets/logos/claude.svg";
import openaiLogo from "../assets/logos/openai.svg";
// Codex 改内联单色标记（CodexMark，fill=currentColor），不再用硬编码白 fill 的 svg 资源
// —— 白图标在浅色主题浅底上发暗/看不见（客户反馈「codex 图标是暗的」）。
import geminiLogo from "../assets/logos/gemini.svg";
import deepseekLogo from "../assets/logos/deepseek.svg";
import kimiLogo from "../assets/logos/kimi.svg";
import zhipuLogo from "../assets/logos/zhipu.svg";
import minimaxLogo from "../assets/logos/minimax.svg";
import hermesLogo from "../assets/logos/hermes.png"; // Nous Research 官方吉祥物头像

type Props = { tool: string; size?: number; active?: boolean; className?: string };

/** tool/provider id → 官方 logo URL。 */
const LOGO: Record<string, string> = {
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
};

/** 首字母方块兜底色（仅未知 id 用；xiapan/openclaw 已有专属图标，不在此列）。 */
const FALLBACK_BG: Record<string, string> = {
  "uu-remote": "#16a34a", // UU远程（网易）品牌绿，首字母 U 方块
  open365: "#2f7d4f", // Open365 电脑管家品牌绿盾，首字母 O 方块
};

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

/** tool/provider id 归一（兼容 ToolInfo.id、TUI tag、驱动 id、中文名 几套命名）。 */
function normTool(tool: string): string {
  const t = tool.toLowerCase();
  if (t.includes("claude")) return "claude";
  if (t.includes("codex") || t === "openai") return "codex";
  if (t.includes("openclaw") || t === "clawx" || t.includes("claw")) return "openclaw";
  if (t.includes("hermes")) return "hermes";
  if (t === "dsh" || t.includes("deepseek-harness")) return "deepseek";
  if (t.includes("gemini")) return "gemini";
  if (t.includes("deepseek")) return "deepseek";
  if (t.includes("kimi") || t.includes("moonshot")) return "kimi";
  if (t.includes("glm") || t.includes("zhipu") || t.includes("智谱")) return "glm";
  if (t.includes("minimax")) return "minimax";
  if (t.includes("xiapan") || t.includes("虾盘")) return "xiapan";
  return t;
}

/** 一个工具/模型图标。active=false（未装）→ 灰显。 */
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
  } else if (LOGO[id]) {
    inner = <img src={LOGO[id]} width={size} height={size} alt={id} style={{ width: size, height: size }} />;
  } else {
    // 兜底：品牌色首字母方块
    const bg = FALLBACK_BG[id] ?? "#5e6ad2";
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
      style={active ? undefined : { filter: "grayscale(1)", opacity: 0.45 }}
      title={tool}
    >
      {inner}
    </span>
  );
}
