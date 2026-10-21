/** Brand artwork is bundled locally. Sources and trademark attribution are in NOTICE. */
import claudeLogo from "../assets/logos/claude.svg";
import openaiLogo from "../assets/logos/openai.svg";
// Codex（CLI / 桌面版）统一用 OpenAI 官方 blossom mark 的内联单色版（fill=currentColor）：
// 官方 OpenAI/Codex 桌面版图标就是「黑色 app tile + 白色 blossom」，浅/深主题都清晰。
import geminiLogo from "../assets/logos/gemini.svg";
import deepseekLogo from "../assets/logos/deepseek.svg";
import kimiLogo from "../assets/logos/kimi-code.png";
import zhipuLogo from "../assets/logos/zhipu.svg";
import minimaxLogo from "../assets/logos/minimax.svg";
import hermesLogo from "../assets/logos/hermes.png"; // Nous Research 官方 Nous girl 标记（Hermes Agent 官方 app icon master 白色版，配深色 tile）
import dshLogo from "../assets/logos/dsh.svg"; // DeepSeek Harness 官方 favicon（deepseek.com/harness/favicon.svg）
import piLogo from "../assets/logos/pi.svg"; // pi.dev 官方标记 + 深色圆角底块（官方原件跟系统主题走，直接用会在浅底上消失）
import opencodeLogo from "../assets/logos/opencode.svg"; // 官方 mark，自带 #131010 方形深底（圆角由 ROUNDED_IMG 补）
import uuRemoteLogo from "../assets/logos/uu-remote.png"; // 网易官方 app icon（512 缩到 256、白底抠透明）
import workbuddyLogo from "../assets/logos/workbuddy.png"; // WorkBuddy 官方 app icon（codebuddy.cn/work 页面 CDN 的 400px 原件）
import magpieLogo from "../assets/logos/magpie.png"; // yetone/magpie 仓库自带官方 icon-256.png（UI 隐藏，备用）

import mimoLogo from "../assets/logos/mimo.svg";
import codebuddyLogo from "../assets/logos/codebuddy.svg";
import qoderLogo from "../assets/logos/qoder.svg";
import clawxLogo from "../assets/logos/clawx.svg";

type Props = { tool: string; size?: number; active?: boolean; className?: string };

/** tool/provider id → 官方 logo URL。 */
const LOGO: Record<string, string> = {
  "mimo-code": mimoLogo,
  "codebuddy-code": codebuddyLogo,
  "qoder-cn": qoderLogo,
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
  dsh: dshLogo,
  pi: piLogo,
  opencode: opencodeLogo,
  "uu-remote": uuRemoteLogo,
  workbuddy: workbuddyLogo,
  magpie: magpieLogo,
};

/** 素材本身是直角方形底的 logo：用 CSS 补圆角，跟 Codex / pi 的 24% 圆角块保持一致。 */
const ROUNDED_IMG = new Set(["opencode"]);

/** 首字母方块兜底色（仅未知 id 用；有专属图标/logo 的不会走到这里）。 */
const FALLBACK_BG = "#5e6ad2";

/** 深色圆角 app tile 的底色 —— 跟官方 Codex / ChatGPT 桌面版图标同一个观感。 */
const APP_TILE_BG = "#0d0d0f";

function OpenAIMark({ size }: { size: number }) {
  // OpenAI 官方 blossom mark（单色 fill=currentColor）：由父层文字色上色，
  // 浅色主题呈深色、深色主题呈浅色 —— 官方 Codex/桌面版 app 图标里那枚就是它。
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
      <path d="M21.55 10.004a5.416 5.416 0 00-.478-4.501c-1.217-2.09-3.662-3.166-6.05-2.66A5.59 5.59 0 0010.831 1C8.39.995 6.224 2.546 5.473 4.838A5.553 5.553 0 001.76 7.496a5.487 5.487 0 00.691 6.5 5.416 5.416 0 00.477 4.502c1.217 2.09 3.662 3.165 6.05 2.66A5.586 5.586 0 0013.168 23c2.443.006 4.61-1.546 5.361-3.84a5.553 5.553 0 003.715-2.66 5.488 5.488 0 00-.693-6.497v.001zm-8.381 11.558a4.199 4.199 0 01-2.675-.954c.034-.018.093-.05.132-.074l4.44-2.53a.71.71 0 00.364-.623v-6.176l1.877 1.069c.02.01.033.029.036.05v5.115c-.003 2.274-1.87 4.118-4.174 4.123zM4.192 17.78a4.059 4.059 0 01-.498-2.763c.032.02.09.055.131.078l4.44 2.53c.225.13.504.13.73 0l5.42-3.088v2.138a.068.068 0 01-.027.057L9.9 19.288c-1.999 1.136-4.552.46-5.707-1.51h-.001zM3.023 8.216A4.15 4.15 0 015.198 6.41l-.002.151v5.06a.711.711 0 00.364.624l5.42 3.087-1.876 1.07a.067.067 0 01-.063.005l-4.489-2.559c-1.995-1.14-2.679-3.658-1.53-5.63h.001zm15.417 3.54l-5.42-3.088L14.896 7.6a.067.067 0 01.063-.006l4.489 2.557c1.998 1.14 2.683 3.662 1.529 5.633a4.163 4.163 0 01-2.174 1.807V12.38a.71.71 0 00-.363-.623zm1.867-2.773a6.04 6.04 0 00-.132-.078l-4.44-2.53a.731.731 0 00-.729 0l-5.42 3.088V7.325a.068.068 0 01.027-.057L14.1 4.713c2-1.137 4.555-.46 5.707 1.513.487.833.664 1.809.499 2.757h.001zm-11.741 3.81l-1.877-1.068a.065.065 0 01-.036-.051V6.559c.001-2.277 1.873-4.122 4.181-4.12.976 0 1.92.338 2.671.954-.034.018-.092.05-.131.073l-4.44 2.53a.71.71 0 00-.365.623l-.003 6.173v.002zm1.02-2.168L12 9.25l2.414 1.375v2.75L12 14.75l-2.415-1.375v-2.75z" />
    </svg>
  );
}

/** 深色 app tile + 白色官方 mark —— Codex 桌面版 / Hermes 用同一个外壳，跟官方桌面 app 图标同构。 */
function AppTile({ size, children }: { size: number; children: React.ReactNode }) {
  return (
    <span
      className="inline-flex items-center justify-center rounded-[24%] text-white"
      style={{ width: size, height: size, background: APP_TILE_BG }}
    >
      {children}
    </span>
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
  // Codex 桌面版单独一支：官方桌面 app 图标是「深色圆角 tile + 白色 OpenAI/Codex mark」，
  // CLI 用同一枚 mark 但不加 tile，两个入口一眼能区分「命令行 / 桌面版」。
  if (t === "codex-app") return "codex-app";
  if (t.includes("codex") || t === "openai") return "codex";
  if (t === "clawx") return "clawx";
  if (t.includes("openclaw") || t.includes("claw")) return "openclaw";
  if (t.includes("hermes")) return "hermes";
  // DSH = DeepSeek Harness，用产品自己的官方 favicon（同一只鲸鱼、官方品牌蓝），
  // 不再借用「DeepSeek 模型」那条 provider 图标（两者用途不同：dsh 是工具，deepseek 是供应商）。
  if (t === "dsh" || t.includes("deepseek-harness")) return "dsh";
  if (t.includes("gemini")) return "gemini";
  if (t.includes("deepseek")) return "deepseek";
  if (t.includes("kimi") || t.includes("moonshot")) return "kimi";
  if (t.includes("glm") || t.includes("zhipu") || t.includes("智谱")) return "glm";
  if (t.includes("minimax")) return "minimax";
  if (t.includes("xiapan") || t.includes("虾盘")) return "xiapan";
  // 以下是后加的工具：短/易撞的 id（pi、opencode、uu-remote）用精确匹配 + 少量别名，不用裸 includes ——
  // 「pi」是任何含 pi 字样名字的子串，「opencode」带 code 字样。
  // 它们都不会被上面的规则先吃掉（已逐个 id 核对）。
  if (t === "pi") return "pi";
  if (t === "opencode" || t === "open-code") return "opencode";
  if (t === "uu-remote" || t === "uuremote" || t.includes("uu远程")) return "uu-remote";
  return t;
}

/** 一个工具/模型图标。安装状态由卡片的下载标记表达，品牌素材始终保留原色。 */
export function ToolIcon({ tool, size = 24, active = true, className = "" }: Props) {
  const id = normTool(tool);
  let inner: React.ReactNode;
  if (id === "openclaw") {
    inner = <OpenClawLobster size={size} />;
  } else if (id === "codex") {
    // Codex CLI：官方 OpenAI/Codex mark 直接上色（fill=currentColor），浅/深主题都清晰。
    inner = <OpenAIMark size={size} />;
  } else if (id === "codex-app") {
    // Codex 桌面版：深色圆角 app tile + 白色官方 mark（= 官方桌面 app 图标观感）。
    inner = (
      <AppTile size={size}>
        <OpenAIMark size={Math.round(size * 0.6)} />
      </AppTile>
    );
  } else if (id === "hermes") {
    // Hermes：Nous Research 官方 Nous girl 标记（官方 app icon master 白色版）+ 深色 app tile，
    // 不用「NOUS 外框徽章」（40px 下糊成一团），也不在浅底上直贴黑色头像。
    inner = (
      <AppTile size={size}>
        <img
          src={hermesLogo}
          width={Math.round(size * 0.86)}
          height={Math.round(size * 0.86)}
          alt="hermes"
          style={{ width: Math.round(size * 0.86), height: Math.round(size * 0.86), objectFit: "contain" }}
        />
      </AppTile>
    );
  } else if (id === "xiapan") {
    inner = <XiapanDisk size={size} />;
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
