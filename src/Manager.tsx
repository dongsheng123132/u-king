/**
 * 驱动管理台 —— 日常使用界面（对齐 cc-switch 交互，市场已教育过用户）。
 *
 * 装机向导是「首次」，这里是「日常」：
 *  - 顶部工具 Tab（Claude/Codex/ClawX/Hermes），一次看一个工具的供应商列表
 *  - cc-switch 两步式：点行=选中(展开细节)，点「启用」按钮才真切（不误触）
 *  - 「+ 添加供应商」+ 自定义行 hover 出编辑/删除（内置预设不可删改）
 *  - 顶部余额卡（虾盘云内置 Key）+ 去充值 + 每日消耗迷你柱图（cc-switch 没有的）
 *  - 每行可「测试连通」（让模型真回一句话）
 */

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { openRecharge } from "./lib/recharge";
import {
  BarChart3,
  Blocks,
  BookOpen,
  Bot,
  IdCard,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Cpu,
  ExternalLink,
  Gift,
  Image as ImageIcon,
  KeyRound,
  Loader2,
  Pencil,
  Plug,
  Plus,
  Power,
  RefreshCw,
  ClipboardList,
  Lightbulb,
  Settings,
  Trash2,
  Wallet,
  X,
  XCircle,
  Zap,
} from "lucide-react";
import { cn } from "./lib/cn";
import { ToolIcon } from "./components/ToolIcon";
import type { ProviderPreset } from "./Wizard";
import { XIAPAN_MODELS, priceyModelHint, codexProtocolHint } from "./lib/models";
import { ShareButton } from "./components/ShareCard";
import { WalletCard } from "./components/WalletCard";
import { ToolCheckup } from "./components/ToolCheckup";
import { DoctorCard } from "./components/DoctorCard";
import { FreerouterCard } from "./components/FreerouterCard";
import { PROVIDER_TEMPLATES, type ProviderTemplate } from "./lib/providerTemplates";
import { FREE_GUIDE, type FreeGuide } from "./lib/freeGuide";
import { askConfirm } from "./lib/confirm";
import { buildProviderRepairPrompt } from "./lib/providerRepairPrompt";
import { useI18n } from "./i18n";
import type { DeviceKey, DrawRoute, DriverStatus } from "./lib/types";
import { ACTION, createTauriActionClient } from "./generated/action-client";

const callAction = createTauriActionClient(invoke, { surface: "gui" });

/**
 * 「那个工具**真的**会照着跑吗」—— 回读工具自己的配置得到的结论。
 * 后端 `providers::effective_config` 的镜像，字段含义以那边的文档为准。
 *
 * 🔴 `readable === false` 是**「不知道」**（我们没有这个工具的回读路径），
 * 不是「没配置」。这两种必须渲染成不同的东西 —— 把「没查」画成绿勾，
 * 就是这次要修的那类假绿的又一份。
 */
type EffectiveConfig = {
  target: string;
  readable: boolean;
  provider_key: string | null;
  base_url: string | null;
  model: string | null;
  /** 有别的文件压着我们写的那份 → 写入成功但不生效。值是那个文件的路径。 */
  overridden_by: string | null;
};

type TestResult = { ok: boolean; api: string; latency_ms: number; reply: string | null; error: string | null };

type DailyUsage = { date: string; tokens: number };
type UsageTrend = { daily: DailyUsage[]; today_tokens: number; week_tokens: number; samples: number };
type UsageBreakdownItem = { model: string; tool?: string; cny: number; count: number; input_tokens?: number; output_tokens?: number };
/** 一条本地算出来的省钱建议（后端 usage_local::build_tips，确定性算术，不烧 token）。 */
type UsageTip = { id: string; title: string; detail: string; saving_cny: number };

type UsageBreakdown = {
  days: number;
  items: UsageBreakdownItem[];
  total_cny?: number;
  total_calls?: number;
  total_input_tokens?: number;
  total_output_tokens?: number;
  source?: string;
  tips?: UsageTip[];
};

/**
 * AI 设置页是条件挂载：离开页面会卸载组件，重新进入时 useState 全部归零。
 * 旧实现每次进来都同步等五个后端查询，其中驱动体检要探测多款 CLI，缺失工具的
 * `--version` 最慢可达数秒，于是客户每次都看到一张空白配置页。
 *
 * 缓存只活在当前 WebView 进程内，不写 localStorage（ProviderPreset 含客户自己的 Key，
 * 不能再复制一份到浏览器持久化）。修改供应商/切模型后仍走 `refresh(true)` 强制回读磁盘；
 * 只有“离开又回来”复用刚才已确认的快照。
 */
type ManagerSnapshot = {
  providers: ProviderPreset[];
  driver: DriverStatus | null;
  trend: UsageTrend | null;
  hidden: string[];
  addable: ProviderPreset[];
};

/** 免费路线正在接入的上下文。Key 只在 `editing` 的本机表单状态里，绝不进官网或 Registry。 */
type FreeRouteContext = {
  entry: FreeGuide["entries"][number];
  target: string;
  stage: "draft" | "added";
  savedId?: string;
};
const managerSnapshots = new Map<string, ManagerSnapshot>();

const fmtTok = (n: number, wan: string) =>
  n >= 10000 ? `${(n / 10000).toFixed(n >= 100000 ? 0 : 1)} ${wan}` : `${n}`;

/** token 数紧凑显示：亿 / 万 / 原数（用量看板 ↑input ↓output 用）。 */
const fmtTk = (n: number): string =>
  n >= 100_000_000 ? `${(n / 100_000_000).toFixed(1)}亿` : n >= 10_000 ? `${Math.round(n / 10_000)}万` : `${n}`;

/** 这俩预设已被「虾盘云 + 模型下拉」取代，管理台不再单列 */
const HIDDEN_PRESETS = new Set(["xiapan-claude", "xiapan-gpt"]);

/**
 * 内置驱动 id → 显示名，给「已移除：[+ 虾盘云]」那一行用。
 * 移除后后端列表里就没有这一项了，拿不到 name，只能在这里留一份短名。
 * 只有内置 id 需要（自定义是真删、不会出现在这行），漏了也不致命（回退显示 id）。
 */
const BUILTIN_LABELS: Record<string, string> = {
  xiapan: "虾盘云",
  deepseek: "DeepSeek 官方",
  glm: "智谱 GLM",
  kimi: "Kimi",
  ollama: "Ollama 本地",
  official: "官方直连（还原）",
};

/** 作图的内置默认供应商 id（后端 `providers::XIAPAN_ID` 的对侧）。
 *  界面上只用来判「要不要显示模型输入框」——虾盘云的模型真相源在「AI 作图」页那个下拉里。 */
const DRAW_BUILTIN_ID = "xiapan";

/** 自定义供应商表单的输入框统一样式。 */
const IPT =
  "w-full h-9 rounded-lg border border-white/[0.10] bg-bg-1 px-3 text-[12px] text-ink-1 outline-none focus:border-accent/50 placeholder:text-ink-4";

const TOOL_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  clawx: "ClawX / OpenClaw",
  hermes: "Hermes",
  dsh: "DeepSeek Harness",
  pi: "pi",
  opencode: "OpenCode",
  cline: "Cline",
};

/** 顶部工具 Tab 顺序 + 用哪个 ToolIcon 图标（cc-switch 式分页）。
 *  Claude / Codex：直接写配置即生效。
 *  ClawX：2026-07-19 加回 —— 老决策（2026-06-20）因「切了没反应」把它移去教程页，但那个坑
 *  的根因（运行中 ClawX 退出时用内存副本覆写磁盘配置）后来已被 `apply_clawx_managed`
 *  （关进程→写配置→自动重启）根治；托盘也在提示「ClawX 去 AI 设置切」，这里没 Tab 就成了
 *  断头路（客户实锤「找不到位置」）。switchOneTool 对 clawx 走托管命令，不走裸 apply_provider。
 *  Hermes：2026-07-22 加回 Tab —— Hermes 现在是 TUI（终端）结构、跟 Claude/Codex 一样是 CLI 驱动，
 *  就该在这里 per-tool 切驱动（后端 apply_provider 支持 hermes 目标，写 %LOCALAPPDATA%\hermes\.env）。
 *  不再当「桌面版/网页版」走教程页。switchOneTool 对 hermes 走通用 apply_provider（非 clawx 托管路径）。
 *  DSH / pi：2026-08-22 加 —— 后端 `providers.rs::APPLY_ALL_TARGETS` 早就能配这两个驱动
 *  （`apply_dsh` / `apply_pi`），但这一页从来没给它们开 Tab，客户装了 DSH/pi 也配不上
 *  （只能靠「一键配好全部」顺带碰一下）。两个都走通用 apply_provider 路径，不需要
 *  clawx 那种托管重启。 */
const TOOL_TABS: { target: string; label: string; icon: string }[] = [
  { target: "claude", label: "Claude Code", icon: "claude" },
  { target: "codex", label: "Codex", icon: "codex" },
  { target: "clawx", label: "ClawX", icon: "clawx" },
  { target: "hermes", label: "Hermes", icon: "hermes" },
  { target: "dsh", label: "DeepSeek Harness", icon: "dsh" },
  // pi 没有官方 logo/专属图标，ToolIcon 对未知 id 会退到「品牌色首字母方块」兜底
  // （见 ToolIcon.tsx 的 FALLBACK_BG 分支）——这是已有的中性图标路径，不是新造的 key。
  { target: "pi", label: "pi", icon: "pi" },
  // OpenCode：2026-08-24 加 —— 后端 `apply_opencode` 从 2026-08-03 就在、`APPLY_ALL_TARGETS`
  // 里也一直有它，但这一页没 Tab、`tools.rs` 里还 `hidden: true`，于是它是「藏在『一键配好
  // 全部』里的隐藏项」：客户既看不见也配不准（用户 2026-08-24 点名要）。cc-switch 和
  // EchoBird 都把 opencode 当一等公民列着，我们不列纯粹是自己藏自己。
  { target: "opencode", label: "OpenCode", icon: "opencode" },
  // Cline：2026-08-29 上架 —— apply_cline 写 ~/.cline 的 openai-compatible 槽位，
  // 走通用 apply_provider 路径（同 pi/opencode，不需要托管重启）。
  { target: "cline", label: "Cline", icon: "cline" },
];

/** 配置目标 → 「我的 AI」里的工具 id。跟 `App.tsx::MANAGER_TARGET_TOOL_ID` 是同一张表；
 *  那边负责「装/起哪个」，这边只负责「装没装」，两处都只读不写，不构成第二份实现。 */
const TARGET_TOOL_ID: Record<string, string> = {
  claude: "claude-code",
  codex: "codex",
  clawx: "clawx",
  hermes: "hermes",
  dsh: "dsh",
  pi: "pi",
  opencode: "opencode",
  cline: "cline",
};

/**
 * 这个配置目标**真的**装了没。
 *
 * 🔴 **不要用 `toolInstalledOf`** —— 它对 `claude` / `codex` 是**写死的 true**
 * （见那个函数里的 `return true`）。那在老布局下无所谓：那排卡片只是选「配哪个」，
 * 安装态只影响一个图标灰不灰。但「左装右选」要靠它决定**主按钮是「装它」还是「启动」**，
 * 恒真就意味着没装的工具也会显示「启动」，点了什么都不会发生 ——
 * ★ 又是「报告是对的、世界是坏的」那一类。所以这里读 `list_tools` 的真实结果。
 *
 * 拿不到清单（还没加载完）时返回 `null` = **不知道**，界面据此显示中性态，
 * 不假装知道（同 readiness 那条：不知道就说不知道，别猜成已装）。
 */
function realInstalled(tools: { id: string; installed: boolean }[] | undefined, target: string): boolean | null {
  if (!tools?.length) return null;
  const id = TARGET_TOOL_ID[target];
  const hit = tools.find((x) => x.id === id);
  return hit ? hit.installed : null;
}

/**
 * 某工具当前生效的 provider id（per-tool）。
 * **只读后端 `active` 表**（对齐 cc-switch 的 is_current：切一次记一笔，回显读这一笔）——
 * 不再前端各自反推。Hermes 老 bug（「有 model 就当虾盘云」→ 切官方后还显示使用中）根治于此。
 */
/**
 * 回验结论的那一行小字。
 *
 * 🔴 **三种状态必须视觉可分，尤其「不知道」不许长得像「没问题」。**
 *   · 读到了 → 灰色 mono，如实写出工具会用的 `provider · model`（跟上面那行一对比就知道对不对）
 *   · 被别的文件压着 → 橙色警告 + **点名那个文件**（客户拿着路径就能自己去改）
 *   · 没有回读路径 → 中性「未回读」，**不给任何勾**
 *
 * 空结果有两义（没查 / 查了没有）—— 把「没查」渲染成绿勾，就是这次要修的那类假绿的又一份。
 */
function EffectiveLine({ eff, t }: { eff?: EffectiveConfig; t: (s: string, v?: Record<string, string>) => string }) {
  if (!eff) return null;
  if (!eff.readable) {
    return (
      <div className="text-[10px] text-ink-5 truncate mt-0.5" title={eff.overridden_by ?? undefined}>
        {eff.overridden_by
          ? t("未回读 · {f} 挡住了视线（带注释，读不了）", { f: shortPath(eff.overridden_by) })
          : t("未回读 · 这个工具还没有回读通道")}
      </div>
    );
  }
  const runs = [eff.provider_key, eff.model].filter(Boolean).join(" · ");
  if (!runs) {
    return <div className="text-[10px] text-ink-5 truncate mt-0.5">{t("实际：还没配任何驱动")}</div>;
  }
  if (eff.overridden_by) {
    return (
      <div className="text-[10px] text-warning-600 truncate mt-0.5" title={eff.overridden_by}>
        {t("⚠ {f} 压着它 → 实际跑 {runs}", { f: shortPath(eff.overridden_by), runs })}
      </div>
    );
  }
  return (
    <div className="text-[10px] text-ink-4 font-mono truncate mt-0.5" title={eff.base_url ?? undefined}>
      {t("实际：{runs}", { runs })}
    </div>
  );
}

/**
 * 供应商行上那个端点小字：`api.u-claw.org.cn/v1`。
 *
 * 只留 host + path，**去掉 scheme** —— `https://` 占 8 个字符却没有任何区分度
 * （这里全是 https），而这一行真正要回答的是「这条打的是谁家」。
 */
function hostOf(p: ProviderPreset): string {
  const raw = (p.openai_base || p.anthropic_base || "").trim();
  if (!raw) return "";
  return raw.replace(/^https?:\/\//, "").replace(/\/+$/, "");
}

/**
 * 延迟徽标。色阶抄 EchoBird：<200ms 绿 / <500ms 黄 / 其余红 / **没测过灰**。
 *
 * 🔴 「没测过」必须是灰色空心，不许是绿色。这跟切换回验那条是同一条规矩：
 * 空结果有两义，把「没查」画成「没问题」就是在制造假绿。
 */
function LatencyBadge({
  tr,
  busy,
  t,
}: {
  tr?: TestResult;
  busy: boolean;
  t: (s: string, v?: Record<string, string | number>) => string;
}) {
  if (busy) return <Loader2 size={12} className="animate-spin text-ink-4 shrink-0" />;
  if (!tr) {
    return (
      <span
        className="shrink-0 text-[10px] font-mono text-ink-6 border border-white/[0.06] rounded-full px-1.5 h-[18px] inline-flex items-center"
        title={t("还没测过这一家")}
      >
        {t("未测")}
      </span>
    );
  }
  if (!tr.ok) {
    return (
      <span
        className="shrink-0 text-[10px] font-mono text-danger-400 bg-danger-500/[0.10] border border-danger-500/25 rounded-full px-1.5 h-[18px] inline-flex items-center"
        title={tr.error ?? undefined}
      >
        {t("不通")}
      </span>
    );
  }
  const ms = tr.latency_ms;
  const tone =
    ms < 200
      ? "text-success-400 bg-success-500/[0.10] border-success-500/25"
      : ms < 500
        ? "text-warning-600 bg-warning-500/[0.10] border-warning-500/25"
        : "text-danger-400 bg-danger-500/[0.10] border-danger-500/25";
  return (
    <span
      className={cn(
        "shrink-0 text-[10px] font-mono rounded-full px-1.5 h-[18px] inline-flex items-center border",
        tone,
      )}
      title={tr.reply ? `「${tr.reply}」` : undefined}
    >
      {ms}ms
    </span>
  );
}

/** 长路径只留最后两段 —— 侧栏 240px 宽，整条绝对路径进去只会把别的字挤没。完整路径挂 title。 */
function shortPath(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts.slice(-2).join("/");
}

function toolActiveOf(driver: DriverStatus | null, target: string): string | null {
  return driver?.active?.[target] ?? null;
}

/** 某工具当前生效的模型（per-tool）。 */
function toolModelOf(driver: DriverStatus | null, target: string): string | null {
  if (!driver) return null;
  switch (target) {
    case "claude":
      return driver.claude_model;
    case "codex":
      return driver.codex_model;
    case "clawx":
      return driver.clawx_model;
    case "hermes":
      return driver.hermes_model;
    case "dsh":
      return driver.dsh_model;
    // pi 没有专属的 xxx_model 字段（跟 qwen/crush/opencode 同口径），
    // 「已配：{provider 名}」照样从 toolActiveOf 拿得到，只是不带模型后缀。
    default:
      return null;
  }
}

/** 某工具是否已安装/可配。Claude 总可配（最常用，未装也可预配）。 */
function toolInstalledOf(driver: DriverStatus | null, target: string): boolean {
  if (!driver) return target === "claude";
  switch (target) {
    case "claude":
      return true;
    case "codex":
      return !!driver.codex_provider || true; // Codex 也总展示（CLI/桌面版常见）
    case "clawx":
      return !!driver.clawx_installed || !!driver.clawx_model;
    case "hermes":
      return !!driver.hermes_installed || !!driver.hermes_model;
    case "dsh":
      return !!driver.dsh_installed || !!driver.dsh_model;
    case "pi":
      return !!driver.extra_installed?.pi;
    // opencode 同 pi：它虽然进了 LIST_TOOLS（有自己的 Tab），装没装仍然只从
    // `extra_installed` 读 —— 后端 `driver_status()` 对 `PROMOTED_TO_LIST_TOOLS`
    // 那批照样填这张表（不填就会从「一键配好全部」弹窗里静默消失）。
    case "opencode":
      return !!driver.extra_installed?.opencode;
    // cline 同 pi/opencode：PROMOTED_TO_LIST_TOOLS 那批，装没装从 extra_installed 读。
    case "cline":
      return !!driver.extra_installed?.cline;
    default:
      return false;
  }
}

/**
 * 某供应商在某工具下的「模型候选」= 内置候选 + 动态拉到的真实清单（去重，内置在前）。
 * - 虾盘云：内置候选用 `XIAPAN_MODELS`（带人话说明）；Codex 链路再置顶 codex_model。
 * - 其它供应商：内置候选 = 预设默认 model；Codex 链路再置顶 codex_model（若有）。
 * UI 的输入框始终可手填任意 id，这里只是给「下拉提示」用。
 */
function modelOptionsFor(
  p: ProviderPreset,
  target: string,
  remote: string[],
  tr: (zh: string, vars?: Record<string, string | number>) => string,
): { id: string; label: string }[] {
  const out: { id: string; label: string }[] = [];
  const seen = new Set<string>();
  const add = (id: string, label: string) => {
    const t = id.trim();
    if (!t || seen.has(t)) return;
    seen.add(t);
    out.push({ id: t, label });
  };
  // Codex 链路：把该供应商的 codex_model 置顶（如虾盘云 gpt-5.3-codex）
  if (target === "codex" && p.codex_model)
    add(p.codex_model, tr("{model}（Codex 推荐）", { model: p.codex_model }));
  if (p.builtin_recharge) {
    for (const g of XIAPAN_MODELS) {
      // label 过一遍 tr()：这份清单是 lib/models.ts 的共享数据，工作台输入框的模型下拉
      // 也读它。以前这里直出中文，英文界面下整个「换模型」列表是中文的。
      for (const m of g.items) add(m.id, `${m.recommend ? "★ " : ""}${tr(m.label)}`);
    }
  } else if (p.model) {
    add(p.model, tr("{model}（预设默认）", { model: p.model }));
  }
  for (const id of remote) add(id, id);
  return out;
}

export function Manager({
  onGoCodex,
  onGoAdvanced,
  onGoPage,
  onDeviceKeyChange,
  onRecharge,
  onSelfUpdate,
  tools,
  onInstallTool,
  onLaunchTool,
  onAskAI,
}: {
  onGoCodex?: () => void;
  onGoAdvanced?: () => void;
  /** 跳到某个全屏配置页（本地大模型 / DSH 插件 / Token 压缩机 / 让 AI 认识 U-King）。
   *  2026-08-22 这批页面从侧栏摘掉、入口收进本页「高级」分区 —— 页面和路由原样保留，
   *  App 那边照 tab id 渲染，这里只负责跳（同 Codex 专区那条的手法）。 */
  onGoPage?: (tab: string) => void;
  onDeviceKeyChange?: (dk: DeviceKey) => void;
  onRecharge?: (url?: string) => void;
  /** 本体一键升级（App 的 doSelfUpdate，带进度/失败账本/重启流程 —— 本页不重造第二份）。 */
  onSelfUpdate?: () => void;
  /**
   * 「我的 AI」那份工具清单 —— 只为读**真实**安装态。
   *
   * 🔴 故意只声明用得到的两个字段，**不再抄一份 `ToolInfo`**：那个类型在 App / Advanced /
   * CodexZone 里已经各有一份本地定义（三份），再加第四份就是第四处会漂的地方（宪法 8）。
   * 本页只关心「装没装」，结构类型足够。
   */
  tools?: { id: string; installed: boolean }[];
  /** 装这个配置目标。由 App 翻成 ToolInfo 后走「我的 AI」同一条装机流，本页不自己实现。 */
  onInstallTool?: (target: string) => void;
  /** 起这个配置目标。CLI 落进 U-CLI 会话、GUI 应用外部弹出 —— 也是 App 那条唯一实现。 */
  onLaunchTool?: (target: string) => void;
  /** 将已脱敏的供应商故障交给 U-Chat；页面本身不读取或转交 API Key。 */
  onAskAI?: (prompt: string) => void;
}) {
  const { t } = useI18n();
  /**
   * 页内分区（2026-08-21）。原来这一页是**四段竖着排下来**：余额条 → 用量账单（折叠）
   * → 选择要配置的 AI → 更多设置（折叠）。客户的原话是「大体的功能都是有的，就是繁琐了，
   * 还容易弄错」—— 病不在缺功能，在于**低频的东西和高频的东西挤在同一条竖线上**，
   * 于是高频那件事（换个模型）要先滚过两个折叠块才够得着，而折叠块本身又在暗示
   * 「这里还有你没看的东西」。
   *
   * 改成分区后每次只呈现一件事。**不动任何一段的内部实现** —— 只是把它们分到 4 个 tab，
   * 所以这不是重写，是把已有的东西摆正（用户：「不要大改原来的」）。
   */
  const [settingsTab, setSettingsTab] = useState<"tools" | "providers" | "free" | "usage" | "advanced">("tools");
  const initialSnapshot = managerSnapshots.get("claude");
  const [providers, setProviders] = useState<ProviderPreset[]>(() => initialSnapshot?.providers ?? []);
  /** 被用户移出列表的内置驱动 id（决定底部「添加虾盘云」出不出现）。 */
  const [hidden, setHidden] = useState<string[]>(() => initialSnapshot?.hidden ?? []);
  /** 当前不在列表里、可一键加回的内置驱动（「添加供应商」弹窗顶部那一排）。
   *  默认就含 DeepSeek / GLM / Kimi / Ollama —— 它们不占列表，但点开「添加」就拿得到。 */
  const [addable, setAddable] = useState<ProviderPreset[]>(() => initialSnapshot?.addable ?? []);
  /**
   * 「添加供应商」画廊模板的远程覆盖（2026-08-22 P3b）——`null` = 还没拉到 / 拉不到，
   * 用静态 `PROVIDER_TEMPLATES` 兜底；非空 = 用 skill 清单热下发的那份（同一条通道，
   * `installer.rs::load_skill`，服务器 version 更大才覆盖内嵌）。
   *
   * 🔴 故意不挡渲染：这次 `invoke` 在 `useEffect` 里跑，静默失败就什么都不做——
   * 页面第一帧永远用编进 exe 的静态列表画完，远程数据到了才**悄悄换一次**，不出现加载态。
   * 网络最长可能等到 24s（4 个 URL 各 6s 超时，见 `fetch_remote_skill`），这也是不能让
   * 它挡住页面的原因（宪法第 9 条：网络必须异步 + 超时，不能拿它卡 UI）。
   */
  const [remoteTemplates, setRemoteTemplates] = useState<ProviderTemplate[] | null>(null);
  /** 官网同源、已人工核验的免费 Registry；断网时仍保留本地最后可信清单。 */
  const [remoteGuide, setRemoteGuide] = useState<FreeGuide | null>(null);
  useEffect(() => {
    let alive = true;
    invoke<{ provider_templates?: ProviderTemplate[] }>("load_skill")
      .then((skill) => {
        if (!alive) return;
        if (skill?.provider_templates?.length) setRemoteTemplates(skill.provider_templates);
      })
      .catch(() => {}); // 拉不到 / 版本没更新 / 老结构没这字段 —— 都留着用静态兜底，不报错
    return () => {
      alive = false;
    };
  }, []);
  const templates = remoteTemplates ?? PROVIDER_TEMPLATES;
  const freeGuide = remoteGuide ?? FREE_GUIDE;
  const [driver, setDriver] = useState<DriverStatus | null>(() => initialSnapshot?.driver ?? null);
  const [deviceKey, setDeviceKey] = useState<DeviceKey | null>(null);
  /** 供应商库里哪张卡展开了设备钱包。钱包是虾盘云这家供应商的一部分（余额/Key 都是它的），
   *  所以它长在卡片上、跟着卡片一起消失 —— 不做全屏 modal：那会让它看着又像个全局功能。 */
  const [walletOpen, setWalletOpen] = useState(false);
  const [trend, setTrend] = useState<UsageTrend | null>(() => initialSnapshot?.trend ?? null);
  const [breakdown, setBreakdown] = useState<UsageBreakdown | null>(null);
  // 用量看板：时间窗口（7/30 天，用 ref 让 fetch 保持稳定不吃闭包）+ 是否按工具分组视图
  const [usageDays, setUsageDays] = useState(30);
  const usageDaysRef = useRef(30);
  const [usageGroupByTool, setUsageGroupByTool] = useState(false);
  const [busy, setBusy] = useState<string | null>(null); // provider id 正在切换/测试
  /** 每个配置目标「真正会跑什么」的回读结果。键 = target。缺键 = 还没回读过。 */
  const [effective, setEffective] = useState<Record<string, EffectiveConfig>>({});
  const [pingingAll, setPingingAll] = useState(false);
  const [pingProgress, setPingProgress] = useState({ done: 0, total: 0 });
  const [testResult, setTestResult] = useState<Record<string, TestResult>>({});
  const [toast, setToast] = useState<string | null>(null);
  // 用户为某 provider 临时填的 key（虾盘云默认用内置）
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({});
  // 虾盘云卡片选中的模型（未动过则跟随当前生效/预设默认）
  const [modelSel, setModelSel] = useState<Record<string, string>>({});
  // 当前选中的工具 Tab（cc-switch 式：一次只看一个工具的 provider 列表）
  const [activeTab, setActiveTab] = useState<string>("claude");
  // cc-switch 式：点行=选中(高亮)，点「启用」才真切。null=未选中任何
  const [selected, setSelected] = useState<string | null>(null);
  // 自定义 provider 编辑弹窗（null=关闭，对象=新增/编辑）
  const [editing, setEditing] = useState<ProviderPreset | null>(null);
  /** 工具分配只引用已有定义；新建时才打开下面那张表单。 */
  const [providerPickerOpen, setProviderPickerOpen] = useState(false);
  /** 从引用选择器新建的供应商，保存后要立即引用回打开它的当前工具。 */
  const [addNewToActiveTool, setAddNewToActiveTool] = useState(false);
  const [freeRouteContext, setFreeRouteContext] = useState<FreeRouteContext | null>(null);
  const [freeEnabling, setFreeEnabling] = useState(false);
  // 动态拉取到的模型清单缓存（按 provider id；同一供应商的清单与工具无关）
  const [remoteModels, setRemoteModels] = useState<Record<string, string[]>>({});
  // 正在拉模型清单的 provider id
  const [fetchingModels, setFetchingModels] = useState<string | null>(null);

  /* ── 「AI 作图」走哪家 ───────────────────────────────────────────────
     和上面那四个 AI 并列摆着，但它**不是一个外部工具**：作图是 U-King 自己的能力，
     选完只是记一笔（`~/.uking/draw-route.json`），不往任何配置文件写字 ——
     所以没有「已接管 / 未接管」，也不走 apply_provider。
     供应商清单**故意用全局的**（`tool: null`）：作图不属于左栏那四个 AI 中的任何一个，
     跟着 activeTab 变的话，客户切个 Tab 会发现作图能选的家数变了，那是纯粹的误导。 */
  const [drawRoute, setDrawRoute] = useState<DrawRoute | null>(null);
  const [drawProviders, setDrawProviders] = useState<ProviderPreset[]>([]);
  const [drawPick, setDrawPick] = useState<string>("");
  const [drawModel, setDrawModel] = useState<string>("");
  const [drawBusy, setDrawBusy] = useState(false);
  /** 磁盘上那笔记录已经吃进输入框了 —— 只吃一次，理由见 refreshDrawRoute。 */
  const drawAdoptedRef = useRef(false);

  const flash = (m: string) => {
    setToast(m);
    window.setTimeout(() => setToast(null), 3000);
  };

  /** 按需查一次「钱花在哪了」——**本地实际用量**：读 Claude Code 会话日志按模型聚合，
   * 含客户自己的 Key（BYOK），不依赖我们服务器、纯本地只读（打开页面/点刷新才查）。 */
  const fetchBreakdown = useCallback((daysOverride?: number) => {
    const d = daysOverride ?? usageDaysRef.current;
    usageDaysRef.current = d;
    invoke<UsageBreakdown>("query_local_usage", { days: d })
      .then(setBreakdown)
      .catch(() => {});
  }, []);

  /**
   * 把用量整理成「AI 直接能读的正文 + 现成问法」拷到剪贴板。
   *
   * 为什么是复制而不是替客户去调模型：这类分析要跟着客户自己的上下文追问（他的预算、
   * 他在做什么项目、他能接受多慢），扔进他正在用的那个 AI 里聊最顺手 —— 我们再包一层
   * 对话框既多花他的钱，又比不上他自己的会话有上下文。确定性的结论上面已经算好了。
   *
   * **只带聚合后的元数据**（模型名 / 次数 / token 数），不含任何 prompt 内容、路径或 Key。
   */
  const copyUsageForAi = async (b: UsageBreakdown) => {
    const total = b.total_cny ?? b.items.reduce((s, it) => s + it.cny, 0);
    const rows = b.items
      .map((it) => `| ${it.tool ?? "-"} | ${it.model} | ${it.count} | ${it.input_tokens ?? 0} | ${it.output_tokens ?? 0} | ${it.cny.toFixed(2)} |`)
      .join("\n");
    const text = [
      t("# 我的 AI 用量（最近 {n} 天）", { n: b.days }),
      "",
      t("数据来自本机 Claude Code / Codex CLI 的会话日志，按模型聚合。花费是按各模型公开列表价折算的**估算值**，实际单价取决于我用的供应商（可能便宜不少）。"),
      "",
      t("合计：约 ¥{c} · {k} 次调用 · 输入 {i} token / 输出 {o} token", {
        c: total.toFixed(2),
        k: b.total_calls ?? b.items.reduce((s, it) => s + it.count, 0),
        i: b.total_input_tokens ?? 0,
        o: b.total_output_tokens ?? 0,
      }),
      "",
      `| ${t("工具")} | ${t("模型")} | ${t("调用次数")} | ${t("输入 token")} | ${t("输出 token")} | ${t("估算 ¥")} |`,
      "|---|---|---|---|---|---|",
      rows,
      "",
      t("请帮我看："),
      t("1. 钱主要花在哪、有没有明显浪费；"),
      t("2. 哪些活可以换更便宜的模型，怎么分工比较合理；"),
      t("3. 给我三件今天就能做的事，按性价比排序。"),
    ].join("\n");
    try {
      await navigator.clipboard.writeText(text);
      flash(t("已复制。粘给任意 AI 即可，它会看到完整用量表"));
    } catch {
      flash(t("复制失败，请手动选中复制"));
    }
  };

  /**
   * 统一解析某 provider 实际要用的 Key：
   *  用户手填 > 虾盘云内置设备 Key > provider 自带 api_key（自定义中转 / Ollama 占位）。
   * 三处共用（切换 / 测试 / 拉模型），避免各写一套口径不一。
   */
  const resolveKey = useCallback(
    (p: ProviderPreset): string => {
      let key = keyInputs[p.id]?.trim() || "";
      if (!key && p.builtin_recharge) key = deviceKey?.key || "";
      if (!key && p.api_key?.trim()) key = p.api_key.trim();
      return key;
    },
    [keyInputs, deviceKey],
  );

  /** 动态拉取某供应商真实可用的模型清单（对齐 cc-switch）。拉不到不致命，仍可手填。 */
  async function fetchModels(p: ProviderPreset) {
    const key = resolveKey(p);
    if (!key && p.id !== "ollama") {
      setSelected(p.id);
      flash(t("请先填入 Key，再拉取模型清单"));
      return;
    }
    setFetchingModels(p.id);
    try {
      const ids = await invoke<string[]>("list_remote_models", { providerId: p.id, apiKey: key });
      setRemoteModels((m) => ({ ...m, [p.id]: ids }));
      flash(t("已拉取 {n} 个可用模型 —— 下拉里选，或直接手填", { n: ids.length }));
    } catch (e) {
      flash(t("拉取失败：{e} —— 可直接手填模型 id", { e: String(e) }));
    } finally {
      setFetchingModels(null);
    }
  }

  /** 当前在配哪个 AI —— 列表是 per-tool 的，拉列表要带上它。用 ref 让 refresh 保持稳定引用。 */
  const activeTabRef = useRef(activeTab);
  activeTabRef.current = activeTab;

  const refresh = useCallback(async (force = true) => {
    // ★ 列表 / 已移除 / 可添加三者都是**这个 AI 自己的**（后端 per-tool 偏好）：
    // 在 Claude Code 那页删掉一个供应商，Hermes 那页照样留着。
    const tool = activeTabRef.current;
    if (!force) {
      const cached = managerSnapshots.get(tool);
      if (cached) {
        setProviders(cached.providers);
        setDriver(cached.driver);
        setTrend(cached.trend);
        setHidden(cached.hidden);
        setAddable(cached.addable);
        return;
      }
    }
    const [p, d, td, h, a] = await Promise.all([
      invoke<ProviderPreset[]>("list_providers", { tool }).catch(() => []),
      invoke<DriverStatus>("get_driver_status").catch(() => null),
      invoke<UsageTrend>("get_usage_trend", { days: 14 }).catch(() => null),
      invoke<string[]>("hidden_providers", { tool }).catch(() => [] as string[]),
      invoke<ProviderPreset[]>("addable_providers", { tool }).catch(() => [] as ProviderPreset[]),
    ]);
    // 🔴 `.catch(() => [])` 只接得住 **reject**，接不住 **resolve(null)**。
    // 后端哪天把某个命令改成返回 `Option<Vec<…>>`（序列化就是 `null`），
    // `providers` 就成了 null，而下面 `providers.filter(...)` 会把**整页**打崩
    // ——2026-08-18 实测：一个不返回该命令的桩就让「AI 设置」整页进了 PanelBoundary。
    // 数组类的返回一律再兜一次底：拿不到就当空表，别当 null 往下传。
    const snapshot: ManagerSnapshot = {
      providers: p ?? [],
      driver: d ?? null,
      trend: td ?? null,
      hidden: h ?? [],
      addable: a ?? [],
    };
    managerSnapshots.set(tool, snapshot);
    setProviders(snapshot.providers);
    setDriver(snapshot.driver);
    setTrend(snapshot.trend);
    setHidden(snapshot.hidden);
    setAddable(snapshot.addable);
  }, []);
  useEffect(() => {
    if (settingsTab !== "free") return;
    let alive = true;
    invoke<FreeGuide | null>("load_free_registry")
      .then((registry) => {
        if (alive && registry && registry.version >= FREE_GUIDE.version) setRemoteGuide(registry);
      })
      .catch(() => {});
    return () => { alive = false; };
  }, [settingsTab]);

  // 切到另一个 AI = 换一份列表，重拉（含它自己的「已移除 / 可添加」）
  useEffect(() => {
    void refresh(false);
  }, [activeTab, refresh]);

  /** 拉「作图走哪家」+ 能选哪几家。不跟 refresh 合并：那个是 per-tool 的，这个是全局的。 */
  const refreshDrawRoute = useCallback(async () => {
    const [r, all] = await Promise.all([
      invoke<DrawRoute>("get_draw_route").catch(() => null),
      invoke<ProviderPreset[]>("list_providers", { tool: null }).catch(() => [] as ProviderPreset[]),
    ]);
    if (r) {
      setDrawRoute(r);
      // 🔴 记录只吃进输入框**一次**。这个函数在客户加/删供应商之后也会重跑（清单得跟着变），
      // 那时再把两个输入框覆盖回磁盘上的旧值，等于把他刚填了一半的模型名冲掉 ——
      // 「输入框自己变回去了」是最像见了鬼、也最难让人相信不是 bug 的一类现象。
      if (!drawAdoptedRef.current) {
        drawAdoptedRef.current = true;
        setDrawPick(r.provider_id);
        setDrawModel(r.model);
      }
    }
    // 只留有 OpenAI 兼容地址的 —— 作图打的是 `/v1/images/*`，没这个地址后端也会拒（存前校验），
    // 但让一个存不进去的选项摆在下拉里，等于骗客户点一次再报错。
    const usable = (all ?? []).filter((p) => (p.openai_base ?? "").trim() !== "");
    // 当前正在用的那家必须在列表里，哪怕它已被移出「供应商列表」（列表主权归用户，
    // 但"我现在走的是谁"不能因为列表里没它就显示成别人）。
    if (r && !usable.some((p) => p.id === r.provider_id)) {
      usable.unshift({ id: r.provider_id, name: r.provider_name, openai_base: "" } as ProviderPreset);
    }
    setDrawProviders(usable);
  }, []);

  // 跟着 `providers` 走：客户在这一页新加一家供应商之后，作图那个下拉必须当场能选到它
  // （只在挂载时拉一次的话，他得关掉整页再进来 —— 而他不会，他会以为加失败了）。
  useEffect(() => {
    void refreshDrawRoute();
  }, [refreshDrawRoute, providers]);

  /** 记一笔「作图走哪家」。**没有「已接管」这一说** —— 它不改任何外部工具的配置。 */
  async function applyDrawRoute() {
    if (drawBusy || !drawPick) return;
    setDrawBusy(true);
    try {
      const r = await invoke<DrawRoute>("set_draw_route", {
        providerId: drawPick,
        // 虾盘云那条不传模型：作图页那个下拉才是它的真相源，两处都能填必然漂移
        model: drawPick === DRAW_BUILTIN_ID ? "" : drawModel.trim(),
      });
      setDrawRoute(r);
      setDrawModel(r.model);
      flash(
        r.builtin
          ? t("AI 作图走虾盘云（内置 Key 计费），模型在「AI 作图」页选")
          : t("AI 作图已改走 {name} —— 用它自己的 Key 计费", { name: r.provider_name }),
      );
    } catch (e) {
      flash(t("保存失败：{e}", { e: String(e) }));
    } finally {
      setDrawBusy(false);
    }
  }

  useEffect(() => {
    invoke<DeviceKey>("get_device_key")
      .then((dk) => {
        setDeviceKey(dk);
        onDeviceKeyChange?.(dk);
        // 查完余额后刷新趋势（record 已落盘）
        invoke<UsageTrend>("get_usage_trend", { days: 14 }).then(setTrend).catch(() => {});
        fetchBreakdown();
      })
      .catch(() => {});
  }, [refresh, fetchBreakdown]);

  /**
   * 回验：切完之后**回读工具自己的配置**，看它真会跑什么。
   *
   * 🔴 这不是「回读我们写了什么」。在它之前，「切换成功」的唯一凭据是 `atomic_write`
   * 之后逐字节比对 —— 那只证明「文件里是我写的内容」，证明不了**「那个工具会读这个字段」**。
   * 2026-08-24 一天撞到三条它必然放行的（pi 的 defaultProvider / opencode 的 jsonc 覆盖 /
   * codex 的 unknown field），客户看到的都是「设置不准」而我们这边零信号。
   */
  const refreshEffective = useCallback(async (target?: string) => {
    try {
      // 契约里 `target` 是 enum（由后端 `LIST_TOOLS` 生成），TS 侧收窄成联合类型。
      // 这里的 `as` 不是绕过校验：**后端仍然会真的拒掉不在 enum 里的值**
      // （`additionalProperties:false` + enum 都在 `validate_input` 里执行）。
      // 前端 target 来自 TOOL_TABS，本来就同源；写 `as` 只是让 TS 相信这一点。
      type Target = Parameters<typeof callAction<typeof ACTION.RUNTIME_PROVIDER_EFFECTIVE>>[1]["target"];
      const env = await callAction(
        ACTION.RUNTIME_PROVIDER_EFFECTIVE,
        target ? { target: target as Target } : {},
      );
      if (!env.ok) return;
      const list = (env.result as unknown as { targets: EffectiveConfig[] }).targets ?? [];
      setEffective((prev) => {
        const next = { ...prev };
        for (const e of list) next[e.target] = e;
        return next;
      });
    } catch {
      // 回验拿不到结果**不该影响切换本身** —— 切换已经成功了，这里只是少一条凭据。
      // 但也绝不能因此把上一次的结论留在界面上冒充这一次的（会变成陈旧的假绿）。
      if (target) setEffective((prev) => ({ ...prev, [target]: { ...prev[target], target, readable: false } as EffectiveConfig }));
    }
  }, []);

  useEffect(() => {
    void refreshEffective();
  }, [refreshEffective, driver]);

  /** 只切某一个工具（cc-switch 式 per-tool）。target ∈ claude|codex|clawx|hermes */
  /**
   * @param viaBridge 只对 Claude Code 有意义：这个供应商只有 OpenAI 端点，
   *   走本机翻译桥（`claude_bridge_enable`）而不是直连。
   */
  async function switchOneTool(target: string, providerId: string, model: string | null, viaBridge = false, providerOverride?: ProviderPreset): Promise<boolean> {
    if (busy) return false;
    const p = providerOverride ?? providers.find((x) => x.id === providerId);
    if (!p) return false;

    // 还原官方时，如果当前正走桥，必须**顺手把桥停掉**。
    // 只改配置不停桥 = 留一个客户看不见的常驻进程；只停桥不改配置 = Claude Code
    // 还指着一个没人监听的本地端口，下次用它撞一个莫名其妙的连接错误。两件事必须一起做。
    if (target === "claude" && providerId === "official" && driver?.claude_via_bridge) {
      setBusy(`${target}:${providerId}`);
      try {
        await invoke("claude_bridge_disable");
        flash(t("已关掉本地翻译桥，Claude Code 还原为官方直连"));
        setSelected(null);
        await refresh();
        return true;
      } catch (e) {
        flash(t("关闭失败：{e}", { e: String(e) }));
        return false;
      } finally {
        setBusy(null);
      }
    }

    if (viaBridge) {
      const key = resolveKey(p);
      if (!key) {
        flash(t("请先填入 {name} 的 API Key", { name: p.name }));
        return false;
      }
      setBusy(`${target}:${providerId}`);
      try {
        await invoke("claude_bridge_enable", { providerId, apiKey: key, model: model?.trim() || null });
        flash(t("已用本地翻译桥把 Claude Code 接到 {name} —— U-King 关掉这条就断", { name: p.name }));
        setSelected(null);
        await refresh();
        return true;
      } catch (e) {
        flash(t("桥接失败：{e}", { e: String(e) }));
        return false;
      } finally {
        setBusy(null);
      }
    }

    // 切换前是否已生效：是 → 这次是「热换模型」，不清选中（别收起面板）
    const wasActive = toolActiveOf(driver, target) === providerId;
    let key = resolveKey(p);
    if (providerId === "official") key = "-";
    if (!key && providerId !== "official" && providerId !== "ollama") {
      flash(t("请先填入 {name} 的 API Key", { name: p.name }));
      return false;
    }
    setBusy(`${target}:${providerId}`);
    try {
      const effModel = providerId === "official" ? null : model?.trim() || null;
      let restartHint = "";
      if (target === "clawx") {
        // ClawX 必须走托管命令（关进程→写配置→自动重启）——运行中的 ClawX 退出时会用
        // 内存副本覆写磁盘配置，裸 apply_provider 写完就被吞（「切了没反应」头号根因）。
        await invoke("apply_clawx_managed", { providerId, apiKey: key, model: effModel });
        restartHint = t("（已自动重启 ClawX 生效）");
      } else {
        await invoke("apply_provider", {
          providerId,
          apiKey: key,
          // 模型对所有供应商生效（不再只限虾盘云）：override 走后端 effective_model；
          // official 还原不带模型。
          model: effModel,
          targets: [target],
        });
        // Codex + 虾盘云 + 没显式选海外模型 → 后端已自动开 DeepSeek 省钱路由（2026-07-20 默认）
        if (target === "codex" && providerId === "xiapan" && (!effModel || effModel.toLowerCase().startsWith("deepseek"))) {
          restartHint = t("（已走 DeepSeek 省钱路由）");
        }
      }
      const modelPart = model ? `（${model}）` : "";
      flash(
        t("已把 {tool} 切到 {name}{model}{hint}", {
          tool: TOOL_LABELS[target] ?? target,
          name: p.name,
          model: modelPart,
          hint: restartHint,
        }),
      );
      if (!wasActive) setSelected(null); // 首次启用后清选中；热换模型则保持面板展开
      await refresh();
      // ★ 切完立刻回验。**放在 flash 之后**是故意的：切换本身已经成功，回验是追加的凭据，
      // 不该因为回验慢就让「已切到 X」这句话晚出来。回验的结论随后自己更新到卡面上。
      await refreshEffective(target);
      return true;
    } catch (e) {
      flash(t("切换失败：{e}", { e: String(e) }));
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function testProvider(p: ProviderPreset) {
    if (busy) return;
    const key = resolveKey(p);
    if (!key && p.id !== "ollama") {
      setSelected(p.id);
      flash(t("请先填入 Key 再测试"));
      return;
    }
    setBusy(p.id);
    try {
      // 测当前工具实际会走的协议，不能因为供应商“也有 Anthropic 端点”就拿它替 Codex/
      // OpenCode/DSH 的 OpenAI 链路报绿。否则客户切完才在真实工具里遇到 /responses 或 chat 失败。
      const api = activeTab === "claude" && p.anthropic_base
        ? "anthropic"
        : activeTab === "codex"
          ? "openai"
          : "openai-chat";
      // 测当前 Tab 这个工具选中的模型（per-tool key）；没选过则用预设默认（传 null）
      const model = modelSel[`${activeTab}:${p.id}`]?.trim() || null;
      const r = await invoke<TestResult>("test_provider", { providerId: p.id, apiKey: key, model, api });
      setTestResult((m) => ({ ...m, [p.id]: r }));
      flash(r.ok ? t("{name} 连通 ✓ {ms}ms", { name: p.name, ms: r.latency_ms }) : t("{name} 测试失败", { name: p.name }));
    } catch (e) {
      flash(t("测试异常：{e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  }

  /**
   * 依次测每一家（EchoBird 的 ping-all）。
   *
   * 🔴 **串行，不并发。** 每一次测试都是真的让模型回一句话 —— 是**要花钱的**。
   * 并发打过去等于让客户在一次点击里同时烧掉列表上所有家的额度，而且失败时
   * 分不清是哪家慢还是我们把自己打限流了。串行还有个好处：结果一条条出来，
   * 用户看到想要的那条就可以走了。
   *
   * 跳过没填 Key 的和「官方直连」那条 —— 它们不是「测不通」，是**没有可测的东西**，
   * 拿它们凑一条红色结果只会让列表看起来更糟而没有增加任何信息。
   */
  async function pingAll() {
    if (busy || pingingAll) return;
    const list = providers.filter(
      (p) => !HIDDEN_PRESETS.has(p.id) && p.id !== "official" && (!!resolveKey(p) || p.id === "ollama"),
    );
    if (!list.length) {
      flash(t("没有可测的供应商 —— 先填一个 Key"));
      return;
    }
    setPingingAll(true);
    setPingProgress({ done: 0, total: list.length });
    try {
      for (const [i, p] of list.entries()) {
        const key = resolveKey(p) || "-";
        const api = activeTab === "claude" && p.anthropic_base
          ? "anthropic"
          : activeTab === "codex"
            ? "openai"
            : "openai-chat";
        const model = modelSel[`${activeTab}:${p.id}`]?.trim() || null;
        try {
          const r = await invoke<TestResult>("test_provider", { providerId: p.id, apiKey: key, model, api });
          setTestResult((m) => ({ ...m, [p.id]: r }));
        } catch (e) {
          // 单家炸了不许中断整轮 —— 但也**不许静默跳过**：如实记一条失败，
          // 否则那一家会停在「未测」，看起来像我们没测，而其实是测了没通。
          setTestResult((m) => ({
            ...m,
            [p.id]: { ok: false, api, latency_ms: 0, reply: null, error: String(e) },
          }));
        }
        setPingProgress({ done: i + 1, total: list.length });
      }
      flash(t("测速完成 —— 绿色 <200ms、黄色 <500ms、红色更慢或不通"));
    } finally {
      setPingingAll(false);
    }
  }

  /** 保存自定义 provider（新增/编辑 upsert），成功后刷新列表。 */
  async function saveCustom(p: ProviderPreset): Promise<ProviderPreset | null> {
    // 地址是供应商实体的可读身份键：协议头/尾斜杠/大小写都不该让同一家悄悄变成两条定义。
    // 只在新增时提示；编辑原条目本来就是明确地改这一家，不能再把它拦回去。
    if (!p.id) {
      const normalizeBase = (base: string | null | undefined) =>
        (base ?? "").trim().replace(/^https?:\/\//i, "").replace(/\/+$/, "").toLowerCase();
      const candidateBases = [p.openai_base, p.anthropic_base].map(normalizeBase).filter(Boolean);
      const library = [...providers, ...addable.filter((a) => !providers.some((x) => x.id === a.id))];
      const existing = library.find((item) => {
        const bases = [item.openai_base, item.anthropic_base].map(normalizeBase).filter(Boolean);
        return candidateBases.some((base) => bases.includes(base));
      });
      if (existing) {
        if (existing.builtin) {
          const reference = await askConfirm(
            [
              t("「{name}」是内置驱动，无需新建 —— 直接引用它？", { name: existing.name }),
              t("确认：把内置的 {name} 加回当前 AI 的列表（Key 在列表行里填）。取消：返回修改。", { name: existing.name }),
            ].join("\n\n"),
          );
          if (reference) {
            await restoreProvider(existing.id, existing.name);
            setEditing(null);
          }
          return null;
        }
        const update = await askConfirm(
          t("供应商库里已有「{name}」使用相同地址。\n\n确认：更新它（保留原 Key，除非你改填）。\n取消：继续选择「仍新建（用于多账号）」或「取消」。", { name: existing.name }),
        );
        if (update) {
          // 新表单通常空 Key；更新既有定义时只有用户明确填了 Key 才覆盖它。
          p = {
            ...p,
            id: existing.id,
            api_key: p.api_key?.trim() ? p.api_key : existing.api_key,
          };
          setEditing(p);
        } else if (!(await askConfirm(
          t("仍新建「{name}」？仅在需要为多账号保留独立实例时使用。\n\n确认：仍新建（用于多账号）。\n取消：取消保存。", { name: p.name }),
        ))) {
          return null;
        }
      }
    }
    try {
      const saved = await invoke<ProviderPreset>("add_provider", { provider: p });
      flash(t("已保存「{name}」", { name: p.name }));
      await refresh();
      return saved;
    } catch (e) {
      flash(t("保存失败：{e}", { e: String(e) }));
      return null;
    }
  }

  /**
   * 从**当前这个 AI** 的列表里移除一个供应商 —— 内置也能移除（0.9.84「列表主权归用户」）。
   *
   * ★ per-tool（0.9.9x）：只从 `activeTab` 那份列表里拿走，另外三个 AI 一字不动，自定义
   * 供应商的定义和 Key 也留着。客户原话「Claude Code 的删除，Hermes 的留下来」——
   * 四个 AI 本来各配各的驱动，共用一份列表等于把「我不想让 Claude 用它」执行成「我不想再用它」。
   * 要连定义带 Key 一起删是另一条路：编辑弹窗里的「彻底删除」（见 purgeProvider）。
   *
   * 确认框必须说清两件事，否则用户会以为点了它机器就被改了：
   *  ① 只影响这一个 AI 的列表，需要时可在底部/「添加供应商」里加回来。
   *  ② **不会动已经配好的 AI 工具** —— 移除虾盘云 ≠ 把 Claude Code 还原成官方。
   *     替用户改他机器上已配好的东西，正是这次要根治的毛病，所以这里故意不连坐。
   */
  async function removeProvider(p: ProviderPreset) {
    const here = TOOL_LABELS[activeTab] ?? activeTab;
    const others = TOOL_TABS.filter((x) => x.target !== activeTab).map((x) => x.label).join(" / ");
    const inUseHere = toolActiveOf(driver, activeTab) === p.id;
    const lines = [
      t("把「{name}」从 {tool} 的列表里移除？", { name: p.name, tool: here }),
      t("只影响 {tool} —— {others} 的列表照旧留着它，需要时可在下方或「添加供应商」里加回来。", {
        tool: here,
        others,
      }),
      inUseHere
        ? t("⚠️ {tool} 正在用它。移除只影响这个列表，**不会改动它已配好的配置** —— 要换请另选一个供应商启用，或选「官方直连（还原）」。", { tool: here })
        : t("这只影响列表显示，不会改动你已经配好的任何 AI 工具。"),
    ];
    if (!(await askConfirm(lines.join("\n\n")))) return;
    try {
      await invoke("delete_provider", { id: p.id, tool: activeTab });
      flash(t("已从 {tool} 的列表移除「{name}」（其它 AI 保留）", { tool: here, name: p.name }));
      await refresh();
    } catch (e) {
      flash(t("移除失败：{e}", { e: String(e) }));
    }
  }

  /**
   * 彻底删除一个自定义供应商 —— 所有 AI 的列表都拿掉，**定义和已保存的 Key 一起销毁**。
   * 故意只放在编辑弹窗里：行里那个垃圾桶是高频的「这个 AI 不用它」，不该顺手把别的 AI
   * 还在用的东西也毁了；真要毁得多点两下、看清那句话。
   */
  async function purgeProvider(p: ProviderPreset) {
    const inUse = TOOL_TABS.filter((x) => toolActiveOf(driver, x.target) === p.id).map((x) => x.label);
    const lines = [
      t("彻底删除「{name}」？", { name: p.name }),
      t("会从**全部 4 个 AI** 的列表里拿掉，连同它的地址和已保存的 API Key 一起删除，之后只能重新填一次。"),
      inUse.length
        ? t("⚠️ {tools} 正在用它。删除只动这份列表，**不会改动它们已配好的配置**，但你在这里就换不回它了。", { tools: inUse.join(" / ") })
        : t("不会改动你已经配好的任何 AI 工具。"),
    ];
    if (!(await askConfirm(lines.join("\n\n")))) return;
    try {
      await invoke("delete_provider", { id: p.id }); // 不带 tool = 全部 AI + 删定义
      setEditing(null);
      flash(t("已彻底删除「{name}」", { name: p.name }));
      await refresh();
    } catch (e) {
      flash(t("删除失败：{e}", { e: String(e) }));
    }
  }

  /** 把被移除的驱动加回**当前这个 AI** 的列表（底部那个低调的「添加虾盘云」）。 */
  async function restoreProvider(id: string, label: string) {
    const here = TOOL_LABELS[activeTab] ?? activeTab;
    try {
      await invoke("restore_provider", { id, tool: activeTab });
      flash(t("已把「{name}」加回 {tool} 的列表", { name: label, tool: here }));
      await refresh();
    } catch (e) {
      flash(t("添加失败：{e}", { e: String(e) }));
    }
  }

  function openNewCustomProvider(addToCurrentTool = false) {
    setFreeRouteContext(null);
    setAddNewToActiveTool(addToCurrentTool);
    setEditing({
      id: "",
      name: "",
      summary: "",
      openai_base: "",
      anthropic_base: null,
      model: "",
      small_model: "",
      key_url: "",
      key_hint: "API Key",
      builtin_recharge: false,
      recommended: false,
      builtin: false,
      api_key: "",
    });
  }

  /**
   * 从模板直接开「添加供应商」弹窗，字段预填好（2026-08-22，「添加供应商」画廊合并的一半）。
   * 跟 `CustomProviderModal` 内部 `applyTemplate` 是同一套字段映射，只是那边要等用户先点开
   * 弹窗再点模板 chip，这里从外面的画廊卡片一步到位——省的是「点两次才到同一个地方」。
   * 只预填地址/模型/Key 提示，`api_key` 留空、`id` 留空（新增，后端生成 id）。
   */
  function openAddTemplate(tpl: ProviderTemplate) {
    setFreeRouteContext(null);
    setAddNewToActiveTool(false);
    setEditing({
      id: "",
      name: tpl.name,
      summary: "",
      openai_base: tpl.openai_base,
      anthropic_base: tpl.anthropic_base ?? null,
      model: tpl.model ?? "",
      small_model: tpl.small_model ?? tpl.model ?? "",
      key_url: tpl.key_url ?? "",
      key_hint: tpl.key_hint ?? "API Key",
      builtin_recharge: false,
      recommended: false,
      builtin: false,
      api_key: "",
    });
  }

  /** 免费路线入口只设置上下文，不切 tab、不遮住左侧清单。 */
  function openFreeRoute(entry: FreeGuide["entries"][number], tpl: ProviderTemplate) {
    setAddNewToActiveTool(false);
    setEditing({
      id: "", name: tpl.name, summary: "", openai_base: tpl.openai_base,
      anthropic_base: tpl.anthropic_base ?? null, model: tpl.model ?? "",
      small_model: tpl.small_model ?? tpl.model ?? "", key_url: tpl.key_url ?? "",
      key_hint: tpl.key_hint ?? "API Key", builtin_recharge: false, recommended: false,
      builtin: false, api_key: "",
    });
    setFreeRouteContext({ entry, target: entry.targets?.[0] ?? "pi", stage: "draft" });
  }

  async function saveFreeRoute(p: ProviderPreset) {
    const saved = await saveCustom(p);
    if (!saved) return;
    setEditing(saved);
    setFreeRouteContext((ctx) => ctx ? { ...ctx, stage: "added", savedId: saved.id } : ctx);
  }

  async function enableFreeRoute() {
    const ctx = freeRouteContext;
    const p = editing;
    if (!ctx || !p || !ctx.savedId) return;
    // 先让供应商真实回话；失败时还没有写任何 AI 配置，自然无需回滚，也绝不会借用虾盘钱包。
    setFreeEnabling(true);
    try {
      const r = await invoke<TestResult>("test_provider", { providerId: ctx.savedId, apiKey: p.api_key ?? "", model: p.model || null, api: "openai" });
      if (!r.ok) { flash(t("验证失败，尚未启用到任何 AI；不会扣虾盘余额")); return; }
      const applied = await switchOneTool(ctx.target, ctx.savedId, p.model || null, false, p);
      if (!applied) return;
      flash(t("已启用到 {tool}；真实请求验证成功，不使用虾盘钱包", { tool: TOOL_LABELS[ctx.target] ?? ctx.target }));
    } finally {
      setFreeEnabling(false);
    }
  }

  /**
   * 调整优先级（上移/下移）。排第一位的就是首选 —— 顺序由用户说了算，且**每个 AI 各排各的**。
   * 乐观更新本地顺序再落盘，失败就 refresh 拉回真相（顺序是纯偏好，错了也不伤机器）。
   */
  async function moveProvider(id: string, dir: -1 | 1) {
    const cur = [...providers];
    const i = cur.findIndex((p) => p.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= cur.length) return;
    [cur[i], cur[j]] = [cur[j], cur[i]];
    setProviders(cur);
    try {
      await invoke("set_provider_order", { ids: cur.map((p) => p.id), tool: activeTab });
    } catch {
      await refresh();
    }
  }

  const maxTok = Math.max(1, ...(trend?.daily?.map((d) => d.tokens) ?? [1]));
  // 判据在后端（device.rs，带实测依据：客户实际被挡那次预扣要 ¥0.358）。
  // 这里原来是一个裸的 `cny < 0.5`，Manager.tsx 里还有一份一模一样的 —— 改门槛会漂两份。
  const lowBalance = !!deviceKey?.low_balance;

  /**
   * 顶部「几个 AI」状态卡用：某工具当前接管的供应商名 + 模型（一眼看清各 AI 在用什么）。
   * 只读后端 `active` 表（对齐 cc-switch is_current），未切过 = 未接管。
   */
  const toolSummary = (target: string): { managed: boolean; sub: string } => {
    const actId = toolActiveOf(driver, target);
    const model = toolModelOf(driver, target);
    if (!actId) return { managed: false, sub: t("未配置 · 点此设置") };
    if (actId === "official") return { managed: true, sub: t("官方直连") + (model ? ` · ${model}` : "") };
    const p = providers.find((x) => x.id === actId);
    const name = p?.name ?? actId;
    return { managed: true, sub: model ? `${name} · ${model}` : name };
  };

  return (
    <div className="space-y-6 pb-2">
      {/* 1) 余额紧凑条 —— 从整张大卡压成一行：余额 + 今日/近7天 + 补充/刷新。把第一屏让给「换模型」。 */}
      <section className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-card border border-white/[0.08] bg-bg-1/90 px-4 py-3 shadow-card">
        <span className="grid place-items-center w-7 h-7 rounded-md bg-accent/[0.12] shrink-0">
          <Wallet size={14} className="text-accent" />
        </span>
        <div className="min-w-0">
          <div className="text-[10px] text-ink-4 leading-none mb-1">{t("虾盘云余额")}</div>
          <div className={cn("text-[19px] font-semibold leading-none font-mono tracking-tight", lowBalance || (deviceKey && !deviceKey.charged) ? "text-red-300" : "text-ink-0")}>
            {deviceKey?.balance ? deviceKey.balance.text : deviceKey ? t("待充值") : "…"}
          </div>
        </div>
        <div className={cn("text-[11px] min-w-0 hidden md:block", lowBalance || (deviceKey && !deviceKey.charged) ? "text-red-300" : "text-ink-4")}>
          {lowBalance
            ? t("余额偏低，Codex 大模型可能不够一次请求")
            : deviceKey?.charged
              ? t("AI 按量扣费，不用不扣")
              : t("余额不足，请充值后使用 AI")}
          <span className="ml-2 font-mono text-ink-5" title={deviceKey?.key}>
            {deviceKey ? `${deviceKey.key.slice(0, 8)}…${deviceKey.key.slice(-4)}` : ""}
          </span>
        </div>
        {/* 今日/近 7 天 迷你数字（明细在下方「用量账单」折叠面板） */}
        <div className="ml-auto flex items-center gap-2 text-[11px] font-mono">
          <span className="inline-flex items-center gap-1 px-2 py-1 rounded-md bg-white/[0.04] text-ink-3">
            {t("今日")} <span className="text-accent font-semibold">{fmtTok(trend?.today_tokens ?? 0, t("万"))}</span>
          </span>
          <span className="inline-flex items-center gap-1 px-2 py-1 rounded-md bg-white/[0.04] text-ink-3">
            {t("近 7 天")} <span className="text-success-400 font-semibold">{fmtTok(trend?.week_tokens ?? 0, t("万"))}</span>
          </span>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <button
            onClick={() => (onRecharge ? onRecharge(deviceKey?.recharge_url) : openRecharge(deviceKey?.recharge_url))}
            className="inline-flex items-center justify-center gap-1.5 h-9 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
          >
            <Zap size={13} /> {deviceKey?.charged ? t("补充余额") : t("充值开通")}
          </button>
          <button
            onClick={() => {
              invoke<DeviceKey>("get_device_key")
                .then((dk) => {
                  setDeviceKey(dk);
                  onDeviceKeyChange?.(dk);
                  fetchBreakdown();
                })
                .catch(() => {});
              invoke<UsageTrend>("get_usage_trend", { days: 14 }).then(setTrend).catch(() => {});
              flash(t("已刷新余额"));
            }}
            className="inline-flex items-center justify-center w-9 h-9 rounded-lg border border-white/[0.08] bg-bg-1 text-ink-3 hover:text-ink-1 hover:bg-white/[0.04]"
            title={t("刷新")}
          >
            <RefreshCw size={13} />
          </button>
        </div>
      </section>

      {/* 「一键体检 / 一键升级」（2026-08-31，参考 claude doctor / hermes doctor）：
          客户只按一个按钮就拿到「这台机器现在能不能好好用 AI」的完整判词 ——
          本体版本 / 钱包 / 环境 / 各 AI 配置状态一次看全；升级也是一键（CLI 逐个重装到 latest）。 */}
      <DoctorCard
        onRecharge={(url) => (onRecharge ? onRecharge(url ?? undefined) : openRecharge(url))}
        onSelfUpdate={onSelfUpdate}
      />

      {/* 分区切换 —— 每次只呈现一件事。
          顺序按**用的频率**排，不按功能亲缘：换模型（天天）→ 供应商库（偶尔）→
          用量（想起来才看）→ 高级（基本不看）。
          2026-08-25 用户：「这几个栏目有点低调，怕有人不好找，其实里边都比较重量级」——
          每个分区配图标 + 加高一档（h-9→h-10），选中态加重，让它们像五个正经入口而不是一行小字。 */}
      <div className="inline-flex p-1 gap-0.5 rounded-xl border border-white/[0.08] bg-bg-1/60">
        {([
          ["tools", t("工具分配"), t("哪个 AI 用哪家、用什么模型"), Plug],
          ["providers", t("供应商库"), t("增删改各家 API，所有 AI 共用一份"), KeyRound],
          ["free", t("免费算力"), t("国内稳定额度 + 海外/第三方免费路线"), Gift],
          ["usage", t("用量账单"), t("钱花在哪了"), BarChart3],
          ["advanced", t("高级"), t("桌面 App / Codex 专区"), Settings],
        ] as const).map(([id, label, hint, Icon]) => (
          <button
            key={id}
            onClick={() => setSettingsTab(id)}
            title={hint}
            className={cn(
              "inline-flex items-center gap-1.5 min-w-[104px] px-3.5 h-10 rounded-lg text-[12.5px] transition-colors",
              settingsTab === id
                ? "bg-accent/[0.14] text-accent font-semibold ring-1 ring-inset ring-accent/40"
                : "text-ink-2 hover:text-ink-0 hover:bg-white/[0.04]",
            )}
          >
            <Icon size={14} className={settingsTab === id ? "text-accent" : "text-ink-4"} />
            {label}
          </button>
        ))}
      </div>

      {/* 免费算力是人工核验的第三方路线清单，不是内置供应商、也不属于上面的模板画廊。
          条目仅按 template 名称引用端点；「我已有 Key，继续」进入保留路线上下文的右侧抽屉。
          官网只跳官方领 Key，Key 始终由用户在本机填写；Registry 下线只停止展示/推荐，绝不删除
          已保存的本机供应商或 Key。 */}
      {settingsTab === "free" && (
        <>
          {/* 暂保留：它不只是第二份状态展示，还承载“逐工具一键配好”的修复动作。
              迁入 DoctorCard 前不能为去重而删掉唯一可执行入口。 */}
          <ToolCheckup />
          {/* Free Router 一键装跑（2026-08-31 会审定案）：本地免费路由网关。
              放体检卡之后、虾盘云引导之前 —— 它是「进了免费页、想要更多免费模型」的进阶路，
              不是主漏斗。上游钉 SHA + tarball 哈希双校验，Key 只落本机 .env。 */}
          <FreerouterCard onToast={flash} />
          {/* 两类供给不能混账：虾盘云是 U-King 可负责的设备钱包余额；免费路线是客户
              自己在第三方领取的 Key。把两者放在同一入口，既让小白有一条稳定主路，
              又不把「注册送/试用/限流」说成 U-King 的免费 Token。 */}
          <section className="mb-3 rounded-card border border-accent/25 bg-accent/[0.055] shadow-card overflow-hidden">
            <div className="flex items-center gap-2 px-4 py-3">
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-ink-1">{t("国内稳定额度")}</div>
                <div className="text-[11px] text-ink-3 mt-0.5">
                  {t("虾盘云设备钱包：余额、充值和售后由 U-King 管；适合重要任务，不与下方第三方免费额度混算。")}
                </div>
              </div>
              <button
                onClick={() => setSettingsTab("providers")}
                className="shrink-0 inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-accent/35 text-accent text-[11px] font-medium hover:bg-accent/[0.10]"
              >
                {t("去配置")}
                <ChevronRight size={12} />
              </button>
            </div>
          </section>
          <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
          <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
            <Gift size={14} className="text-emerald-400 shrink-0" />
            <div className="flex-1 min-w-0">
              <div className="text-[13px] font-medium text-ink-1">{t("海外 / 第三方免费算力")}</div>
              <div className="text-[11px] text-ink-3">
                {t("下面是第三方当前公开的免费档或试用入口。U-King 不收 Key；你在官网领取后回到这里继续接入。")}
              </div>
            </div>
            <span className="text-[10.5px] text-ink-4 shrink-0">
              {t("核实于")} {freeGuide.checked}
              {remoteGuide ? ` · ${t("已更新")}` : ""}
            </span>
          </div>

          {/* 🔴 这一句不许删：免费条件会烂，而客户看到的是我们的界面。
              让他知道这份清单有可能过期，比让他按着过期教程走到一半撞墙强。 */}
          <div className="px-4 py-2 text-[11px] text-warning-700 dark:text-warning-400 bg-warning-500/[0.08] border-b border-white/[0.06]">
            {t("⚠️ 我们不承诺长期免费，也不自动上线新渠道。断网时只展示最后可信清单，可能已过期，不建议直接启用。")}
          </div>

          <div className="divide-y divide-white/[0.05]">
            {freeGuide.entries.map((e) => {
              // 逐字匹配模板名；对不上就只显示说明、不给按钮 —— 点了没反应比没按钮更坏
              const tpl = e.template ? templates.find((x) => x.name === e.template) : undefined;
              return (
                <div key={e.name} className="px-4 py-3">
                  <div className="flex items-start gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="text-[12.5px] font-medium text-ink-1">{e.name}</div>
                      <div className="text-[11.5px] text-ink-3 mt-0.5">{e.summary}</div>
                      <div className="mt-1 text-[10.5px] text-ink-4">
                        {[e.region, ...(e.targets ?? []).map((target) => TOOL_LABELS[target] ?? target)].filter(Boolean).join(" · ")}
                      </div>
                      {e.note && (
                        <div className="text-[11px] text-ink-4 mt-1 leading-relaxed">{e.note}</div>
                      )}
                    </div>
                    <div className="flex flex-col gap-1.5 shrink-0">
                      {e.key_url && (
                        <button
                          onClick={() => openUrl(e.key_url!)}
                          className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-white/[0.1] text-ink-2 text-[11px] hover:bg-white/[0.04]"
                        >
                          {t("去领 Key")}
                          <ExternalLink size={11} />
                        </button>
                      )}
                      {tpl && (
                        <button
                          onClick={() => openFreeRoute(e, tpl)}
                          className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-accent/30 text-accent text-[11px] font-medium hover:bg-accent/[0.08]"
                        >
                          <Plus size={11} />
                          {t("我已有 Key，继续")}
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>

          <div className="px-4 py-2.5 text-[11px] text-ink-4 border-t border-white/[0.06]">
            {t("没有你想要的那家？「供应商库」里有 20 家模板，或者自己手填地址也行。")}
          </div>
          </section>
        </>
      )}

      {/* 用量账单 —— 每日消耗 + 「钱花在哪了」明细。
          自己占一个分区后不再需要「默认折叠」（折叠是当初为了别霸占第一屏，现在第一屏归「工具分配」了），
          所以 `open` 常开：进到这个分区就是来看账单的，还要再点一下才展开是白设一道门。 */}
      {settingsTab === "usage" && (
      <details open className="group/panel rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
        <summary className="flex items-center gap-2 px-4 py-3 cursor-pointer select-none list-none text-[13px] font-medium text-ink-1 hover:bg-white/[0.02]">
          <BarChart3 size={14} className="text-accent" />
          {t("用量账单 · 钱花在哪了")}
          <span className="text-[10.5px] text-ink-4 font-normal">{t("每日消耗 + 各模型花费明细")}</span>
          <ChevronDown size={15} className="ml-auto text-ink-4 transition-transform group-open/panel:rotate-180" />
        </summary>

        {/* 用量趋势 */}
        <div className="px-5 pb-5 pt-2">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-2 text-[12px] font-medium text-ink-2">
              <span className="grid place-items-center w-6 h-6 rounded-md bg-accent/[0.12]">
                <BarChart3 size={13} className="text-accent" />
              </span>
              {t("每日消耗（最近 14 天）")}
            </div>
            <div className="flex gap-2 text-[11px] font-mono">
              <span className="inline-flex items-center gap-1.5 px-2 py-1 rounded-md bg-white/[0.04] text-ink-3">
                {t("今日")} <span className="text-accent font-semibold">{fmtTok(trend?.today_tokens ?? 0, t("万"))} token</span>
              </span>
              <span className="inline-flex items-center gap-1.5 px-2 py-1 rounded-md bg-white/[0.04] text-ink-3">
                {t("近 7 天")} <span className="text-success-400 font-semibold">{fmtTok(trend?.week_tokens ?? 0, t("万"))} token</span>
              </span>
            </div>
          </div>
          {trend && trend.daily?.length > 0 ? (
            <div className="flex items-end gap-2 h-[92px] px-1">
              {trend.daily.map((d) => (
                <div key={d.date} className="flex-1 flex flex-col items-center gap-1.5 group">
                  <div className="w-full flex items-end justify-center rounded-t-md bg-white/[0.03]" style={{ height: 74 }}>
                    <div
                      className="w-full max-w-[20px] rounded-t-md bg-accent/70 group-hover:bg-accent transition-all relative"
                      style={{ height: `${Math.max(2, (d.tokens / maxTok) * 74)}px` }}
                    >
                      <span className="absolute -top-5 left-1/2 -translate-x-1/2 text-[9px] font-mono text-accent opacity-0 group-hover:opacity-100 whitespace-nowrap">
                        {fmtTok(d.tokens, t("万"))}
                      </span>
                    </div>
                  </div>
                  <span className="text-[9px] font-mono text-ink-5">{d.date.slice(5)}</span>
                </div>
              ))}
            </div>
          ) : (
            <div className="h-[92px] grid place-items-center text-[12px] text-ink-4 bg-white/[0.02] rounded-lg">
              {t("暂无数据 —— 多用几天、多刷新几次余额就有曲线了")}
            </div>
          )}

          {/* 花在哪了：按模型分组的用量明细，回答「哪些行为花了多少钱」 */}
          {breakdown && breakdown.items.length > 0 && (
            <div className="mt-5 pt-4 border-t border-white/[0.06]">
              <div className="flex items-center justify-between gap-2 mb-3">
                <div className="min-w-0">
                  <div className="text-[11px] font-medium text-ink-2">{t("最近 {n} 天，钱花在哪了", { n: breakdown.days })}</div>
                  <div className="text-[10px] text-ink-4 mt-0.5">{t("本地实际用量（含你自己的 Key）· 花费按公开报价估算")}</div>
                </div>
                {(() => {
                  const total = breakdown.items.reduce((s, it) => s + it.cny, 0);
                  const totalCount = breakdown.items.reduce((s, it) => s + it.count, 0);
                  return (
                    <ShareButton
                      onToast={flash}
                      label={t("晒用量报告")}
                      className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-lg border border-accent/40 bg-accent/[0.10] text-[11px] font-medium text-accent hover:bg-accent/20"
                      spec={{
                        kind: "ai-bill",
                        badge: t("我的 AI 用量报告"),
                        title: t("最近 {n} 天，AI 一共花了", { n: breakdown.days }),
                        heroValue: "¥" + total.toFixed(2),
                        heroUnit: "",
                        heroSub: t("{n} 次调用", { n: totalCount }),
                        stats: [
                          { label: t("调用次数"), value: String(totalCount) },
                          { label: t("涉及模型"), value: String(breakdown.items.length) },
                        ],
                        footnote: t("最费：{m} · 国产模型更省钱", { m: breakdown.items[0]?.model ?? "-" }),
                      }}
                    />
                  );
                })()}
              </div>
              {/* 总计头：一眼看清最近 N 天总花费 / 调用 / 涉及模型（后端聚合，含 Claude + Codex）*/}
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-3 text-[11px]">
                <span className="text-ink-3">
                  {t("共花")}{" "}
                  <b className="text-accent font-semibold text-[13.5px]">
                    ¥{(breakdown.total_cny ?? breakdown.items.reduce((s, it) => s + it.cny, 0)).toFixed(2)}
                  </b>
                </span>
                <span className="text-ink-5">·</span>
                <span className="text-ink-4">{t("{n} 次调用", { n: breakdown.total_calls ?? breakdown.items.reduce((s, it) => s + it.count, 0) })}</span>
                <span className="text-ink-5">·</span>
                <span className="text-ink-4">{t("涉及 {n} 个模型", { n: breakdown.items.length })}</span>
              </div>
              {/* 控制：天数（7/30）+ 视图（按模型 / 按工具） */}
              <div className="flex items-center gap-2 mb-3">
                <div className="flex items-center rounded-lg border border-white/[0.08] overflow-hidden">
                  {[7, 30].map((d) => (
                    <button
                      key={d}
                      onClick={() => {
                        setUsageDays(d);
                        fetchBreakdown(d);
                      }}
                      className={cn(
                        "px-2.5 h-7 text-[11px] transition-colors",
                        usageDays === d ? "bg-accent/20 text-accent font-medium" : "text-ink-4 hover:text-ink-2",
                      )}
                    >
                      {t("{n} 天", { n: d })}
                    </button>
                  ))}
                </div>
                <div className="flex items-center rounded-lg border border-white/[0.08] overflow-hidden">
                  {([[false, t("按模型")], [true, t("按工具")]] as [boolean, string][]).map(([g, label]) => (
                    <button
                      key={label}
                      onClick={() => setUsageGroupByTool(g)}
                      className={cn(
                        "px-2.5 h-7 text-[11px] transition-colors",
                        usageGroupByTool === g ? "bg-accent/20 text-accent font-medium" : "text-ink-4 hover:text-ink-2",
                      )}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>

              {(() => {
                const maxCny = Math.max(0.01, ...breakdown.items.map((it) => it.cny));
                const toolChip = (tool?: string) =>
                  tool ? (
                    <span
                      className={cn(
                        "shrink-0 px-1 h-[15px] inline-flex items-center rounded text-[8.5px] font-semibold",
                        tool === "codex" ? "bg-sky-500/15 text-sky-300" : "bg-orange-500/15 text-orange-300",
                      )}
                    >
                      {tool === "codex" ? "Codex" : "Claude"}
                    </span>
                  ) : null;
                const renderRow = (it: UsageBreakdownItem) => (
                  <div key={`${it.tool ?? ""}:${it.model}`} className="flex items-center gap-3">
                    <div className="w-[150px] shrink-0 min-w-0">
                      <div className="flex items-center gap-1.5 min-w-0">
                        {toolChip(it.tool)}
                        <span className="truncate text-[11px] font-mono text-ink-1" title={it.model}>
                          {it.model}
                        </span>
                      </div>
                      <div className="text-[9.5px] text-ink-5 font-mono mt-0.5">
                        ↑{fmtTk(it.input_tokens ?? 0)} ↓{fmtTk(it.output_tokens ?? 0)}
                      </div>
                    </div>
                    <div className="flex-1 h-2.5 rounded-full bg-white/[0.05] overflow-hidden">
                      <div
                        className="h-full rounded-full bg-accent/70"
                        style={{ width: `${Math.max(3, (it.cny / maxCny) * 100)}%` }}
                      />
                    </div>
                    <div className="w-[72px] shrink-0 text-right">
                      <div className="text-[11px] font-mono text-ink-0">¥{it.cny.toFixed(2)}</div>
                      <div className="text-[9.5px] font-mono text-ink-4">{t("{n} 次", { n: it.count })}</div>
                    </div>
                  </div>
                );

                if (!usageGroupByTool) {
                  return <div className="space-y-2.5">{breakdown.items.slice(0, 8).map(renderRow)}</div>;
                }
                // 按工具分组：各工具一段，带小计
                const groups: Record<string, UsageBreakdownItem[]> = {};
                for (const it of breakdown.items) (groups[it.tool || "其它"] ||= []).push(it);
                const order = Object.entries(groups)
                  .map(([tool, items]) => ({ tool, items, sum: items.reduce((s, x) => s + x.cny, 0) }))
                  .sort((a, b) => b.sum - a.sum);
                const toolName = (tl: string) => (tl === "codex" ? "Codex" : tl === "claude" ? "Claude Code" : tl);
                return (
                  <div className="space-y-3.5">
                    {order.map((g) => (
                      <div key={g.tool}>
                        <div className="flex items-center gap-2 mb-1.5">
                          <span
                            className={cn(
                              "px-1.5 h-[16px] inline-flex items-center rounded text-[9px] font-semibold",
                              g.tool === "codex" ? "bg-sky-500/15 text-sky-300" : "bg-orange-500/15 text-orange-300",
                            )}
                          >
                            {toolName(g.tool)}
                          </span>
                          <span className="text-[10px] text-ink-4">
                            {t("小计 ¥{c} · {n} 次", {
                              c: g.sum.toFixed(2),
                              n: g.items.reduce((s, x) => s + x.count, 0),
                            })}
                          </span>
                        </div>
                        <div className="space-y-2.5">{g.items.slice(0, 6).map(renderRow)}</div>
                      </div>
                    ))}
                  </div>
                );
              })()}

              {/* 本地算出来的省钱建议 —— 确定性算术，毫秒级、离线、不烧一个 token。
                  AI 留给开放式追问（下面那个「复制给 AI 分析」）。 */}
              {(breakdown.tips ?? []).length > 0 && (
                <div className="mt-4 space-y-2">
                  {(breakdown.tips ?? []).map((tip) => (
                    <div
                      key={tip.id}
                      className="rounded-lg border border-white/[0.07] bg-white/[0.02] px-3 py-2.5"
                    >
                      <div className="flex items-start gap-2">
                        <Lightbulb size={13} className="text-amber-400 shrink-0 mt-0.5" />
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2 flex-wrap">
                            <span className="text-[11.5px] font-semibold text-ink-1">{tip.title}</span>
                            {tip.saving_cny > 0 && (
                              <span className="px-1.5 h-[16px] inline-flex items-center rounded bg-emerald-500/15 text-emerald-300 text-[9.5px] font-semibold tabular-nums">
                                {t("每月约省 ¥{c}", { c: tip.saving_cny.toFixed(0) })}
                              </span>
                            )}
                          </div>
                          <div className="text-[10.5px] text-ink-4 mt-1 leading-relaxed">{tip.detail}</div>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}

              {/* 复制给 AI 分析：把这份用量整理成 AI 能直接读的正文 + 现成问法。
                  只带聚合后的模型名/次数/token 数，**不含任何 prompt 内容和 Key**。 */}
              <div className="mt-3 flex items-center gap-2 flex-wrap">
                <button
                  onClick={() => copyUsageForAi(breakdown)}
                  className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-lg border border-white/[0.10] text-[11px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.05]"
                >
                  <ClipboardList size={12} />
                  {t("复制给 AI 分析")}
                </button>
                <span className="text-[10px] text-ink-5">
                  {t("只含模型名和 token 数，不含对话内容")}
                </span>
              </div>
            </div>
          )}
        </div>
      </details>
      )}

      {/* 「几个 AI」选择器 —— cc-switch 式：每个 AI 一张状态卡，直接显示各自当前接管的供应商/模型，
          一眼看清 4 个 AI 各在用什么、当前在配哪个（客户两次反馈「切换不明显」→ 从细文字 Tab
          升级成状态卡：强描边选中态 + 已接管/未接管彩色徽章 + 当前供应商·模型副标题）。 */}
      {settingsTab === "tools" && (
      <section>
        <div className="flex items-baseline gap-2 mb-3.5 flex-wrap">
          <h3 className="text-[14px] font-semibold text-ink-0">{t("选择要配置的 AI")}</h3>
          <span className="text-[11px] text-ink-4">
            {t("每个 AI 各自独立 —— 驱动、模型、连这份供应商列表都是分开的（在这里删掉，别的 AI 照旧留着）")}
          </span>
        </div>
        {/* 🔴 「左装右选」（2026-08-20，借鉴 EchoBird）。
            以前：装在「我的 AI」页、配模型在本页，**两页分离** —— 客户要先跳过去装、
            装完跳回来配，中间还得记住自己刚装了哪个。
            现在：左栏一列 AI（含未装的，就地能装），右栏是**跟着左边选中项走**的配置区，
            底下一颗启动。装 → 配 → 用压成一屏一条线。
            ★ 我们比 EchoBird 多一样东西：**启动落进 U-CLI 的会话**，不是弹个裸终端
            （见 App 传进来的 `onLaunchTool`）。EchoBird 只能弹窗，因为它没有壳。
            窄屏（<lg）自动退回上下堆叠，不硬挤两栏。 */}
        <div className="grid gap-4 lg:grid-cols-[minmax(210px,250px)_minmax(0,1fr)] items-start">
          {/* ① 左栏：装 */}
          <div className="grid grid-cols-2 lg:grid-cols-1 gap-2.5">
          {TOOL_TABS.map((tab) => {
            const on = activeTab === tab.target;
            const inst = toolInstalledOf(driver, tab.target);
            const sm = toolSummary(tab.target);
            return (
              <button
                key={tab.target}
                onClick={() => {
                  setActiveTab(tab.target);
                  setSelected(null);
                }}
                className={cn(
                  "relative flex items-center gap-2.5 px-3 py-3 rounded-xl border text-left transition-all",
                  on
                    // 选中（正在配置的 AI）：实心 accent 底 + 满描边 + 上移，跟其它卡拉开明显差距
                    ? "border-accent/80 bg-accent/[0.15] shadow-card -translate-y-0.5"
                    // 未选中：轻底、细边，hover 上浮
                    : "border-white/[0.06] bg-bg-1/60 hover:border-white/[0.14] hover:bg-white/[0.03] hover:-translate-y-0.5",
                )}
              >
                {/* 选中卡左侧 accent 竖条 —— 与下方供应商列表选中态同一视觉语言 */}
                {on && <span className="absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-full bg-accent" />}
                <ToolIcon tool={tab.icon} size={22} active={inst} className="shrink-0" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <span className={cn("text-[13px] font-semibold truncate", on ? "text-accent" : "text-ink-1")}>
                      {tab.label}
                    </span>
                    {/* 🔴 「未装」优先于「未接管」显示 —— 两件事，别混：
                        **未装** = 机器上没这个工具（该去装）；
                        **未接管** = 装了但还没被我们配驱动（该在右栏选一个）。
                        以前只有后者，于是没装的工具显示「未接管」，客户会去右栏找驱动，
                        选完还是用不了 —— 指错了地方比不指更费时间。 */}
                    {realInstalled(tools, tab.target) === false ? (
                      <span className="inline-flex items-center px-1.5 h-[16px] rounded-full text-[9px] font-semibold bg-warning-500/12 text-warning-600 border border-warning-500/30 shrink-0">
                        {t("未安装")}
                      </span>
                    ) : sm.managed ? (
                      <span className="inline-flex items-center gap-0.5 pl-0.5 pr-1.5 h-[16px] rounded-full text-[9px] font-bold bg-success-500/12 text-success-400 border border-success-500/25 shrink-0">
                        <CheckCircle2 size={9} /> {t("已接管")}
                      </span>
                    ) : (
                      <span className="inline-flex items-center px-1.5 h-[16px] rounded-full text-[9px] font-semibold bg-white/[0.04] text-ink-4 border border-white/[0.06] shrink-0">
                        {t("未接管")}
                      </span>
                    )}
                  </div>
                  <div className={cn("text-[10.5px] font-mono truncate mt-1", sm.managed ? "text-success-400" : "text-ink-4")}>
                    {sm.sub}
                  </div>
                  {/* ★ 回验行：**这一行说的是「那个工具会跑什么」，上面那行说的是「我们写了什么」。**
                      两行不一致就是本次要修的那类 bug 的现场 —— pi 那次这里会显示
                      `openrouter · moonshotai/kimi-k2.6`，而上面写着「虾盘云 · deepseek」。
                      🔴 三种状态必须长得不一样，尤其**「不知道」不许长得像「没问题」**。 */}
                  <EffectiveLine eff={effective[tab.target]} t={t} />
                </div>
                {/* 「配置中」脉冲标 —— 🔴 **竖排布局里去掉**（2026-08-20 截图实测）。
                    它是横排四卡时代的补偿：那时四张卡并排、选中态不够跳。改成左栏竖列之后，
                    选中项有 accent 描边 + 底色 + 左侧竖条，已经一眼可辨；而这个药丸在 240px
                    宽里会把标题挤成「Claude C···」——**为了强调「选中了」，代价是看不清选的是谁**。
                    ★ 这个只有出图才看得见，靠读代码看不出来。 */}
                {false && on && (
                  <span className="shrink-0 inline-flex items-center gap-1 text-[10px] font-semibold text-accent bg-accent/[0.12] px-1.5 h-[18px] rounded-full">
                    <span className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" />
                    {t("配置中")}
                  </span>
                )}
              </button>
            );
          })}
          </div>

          {/* ② 右栏：配（跟着左边选中的那个走） */}
          <div className="min-w-0">
        <div className="mb-4 flex items-center justify-between gap-2 flex-wrap">
          <button
            onClick={() => setProviderPickerOpen(true)}
            className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 shadow-sm"
          >
            <Plus size={14} /> {t("从供应商库添加")}
          </button>
          <div className="flex items-center gap-3">
            <button
              onClick={() => invoke("open_apikey_guide").catch(() => {})}
              className="inline-flex items-center gap-1.5 text-[11.5px] text-accent hover:text-accent-400"
            >
              <BookOpen size={12} /> {t("各模型获取 API Key 教程")}
            </button>
          </div>
        </div>

        {/* Codex 桌面版：**这里切一次，桌面版一起生效** —— 两者共用 `~/.codex/config.toml`
            （后端 `providers.rs::apply_provider` 那条 `tool_installed("codex") || codex_app_installed()`
            就是专门为此写的：只装桌面版没装 CLI 也照写）。
            🔴 为什么要单独说这一句：这个事实原来只写在**装机向导的总结页**和**工具市场卡片**上，
            一个「已经装了桌面版、但没走过装机向导」的存量用户，在这一页切完驱动后
            界面上没有任何信息告诉他桌面版也配好了 —— 不是误导，是一片空白，
            于是「codex app 没地方配置」成了普遍印象（客户原话）。 */}
        {activeTab === "codex" && tools?.some((x) => x.id === "codex-app" && x.installed) && (
          <p className="mb-2 text-[11px] text-ink-3 leading-snug">
            {t("检测到 Codex 桌面版：它和 CLI 共用同一份配置，这里切一次两边都生效，不用另外配。")}
          </p>
        )}

        {/* ★ 全部测速（2026-08-24）。以前只有每行 hover 出来的单条「测试连通」——
            要比较六七家谁快，得一条条 hover→点→等→记，实际上没人会做。
            EchoBird 的 ping-all 是把「哪家能用、哪家快」从一次调查变成一次点击。
            🔴 串行不并行：这些请求真的会花钱（每次让模型回一句话），
            并发打过去只会让客户在不知情的情况下同时烧七家的额度；串行还能中途看结果。 */}
        <div className="flex items-center justify-end mb-1.5">
          <button
            onClick={pingAll}
            disabled={!!busy || pingingAll}
            className="inline-flex items-center gap-1 text-[11px] text-ink-3 hover:text-ink-1 disabled:opacity-40 rounded-md px-2 h-7 hover:bg-white/[0.05]"
            title={t("依次让每一家真回一句话 —— 会消耗少量额度")}
          >
            {pingingAll ? <Loader2 size={12} className="animate-spin" /> : <Plug size={12} />}
            {pingingAll ? t("测速中 {done}/{total}", { done: pingProgress.done, total: pingProgress.total }) : t("全部测速")}
          </button>
        </div>

        <ToolProviderList
          target={activeTab}
          driver={driver}
          providers={providers.filter((p) => !HIDDEN_PRESETS.has(p.id))}
          busy={busy}
          testResult={testResult}
          keyInputs={keyInputs}
          modelSel={modelSel}
          remoteModels={remoteModels}
          fetchingModels={fetchingModels}
          selected={selected}
          onSelect={setSelected}
          onKeyInput={(id, v) => setKeyInputs((m) => ({ ...m, [id]: v }))}
          onModelSel={(id, v) => setModelSel((m) => ({ ...m, [id]: v }))}
          onFetchModels={fetchModels}
          onSwitch={switchOneTool}
          onTest={testProvider}
          onAskAI={onAskAI}
          onAskAIToast={() => flash(t("已把故障交给 AI，正在打开工作台"))}
          onOpenKeyUrl={(u) => openUrl(u).catch(() => {})}
          onEdit={(p) => setEditing(p)}
          onDelete={removeProvider}
          onMove={moveProvider}
        />

        {/* 引用供应商有两条严格隔离的路：内置项只加回当前 AI 的列表；模板只预填表单，
            要由用户自填 Key 后保存。免费活动只在「免费算力」页的专用抽屉，绝不出现在这里。 */}
        <div className="mt-4 pt-4 border-t border-white/[0.06]">
          <div className="text-[11.5px] font-medium text-ink-3 mb-2">
            {t("引用供应商")}
          </div>
          {hidden.length > 0 && <>
            <div className="mb-1 text-[10.5px] font-medium text-ink-4">{t("U-King 内置 · 一键加回当前 AI")}</div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {[...hidden]
              .sort((a, b) => (a === "xiapan" ? -1 : b === "xiapan" ? 1 : 0))
              .map((id) => {
                const label = addable.find((a) => a.id === id)?.name ?? BUILTIN_LABELS[id] ?? id;
                return (
                  <button
                    key={`builtin:${id}`}
                    onClick={() => restoreProvider(id, label)}
                    className="flex items-center gap-2 px-3 h-11 rounded-xl border border-white/[0.08] bg-bg-1/60 text-left hover:border-accent/40 hover:bg-accent/[0.06] transition-colors"
                  >
                    <span className="inline-flex items-center px-1.5 h-[16px] rounded-full text-[9px] font-semibold bg-accent/[0.14] text-accent shrink-0">
                      {t("内置")}
                    </span>
                    <span className="text-[12px] font-medium text-ink-1 truncate flex-1">{label}</span>
                    <Plus size={13} className="shrink-0 text-ink-4" />
                  </button>
                );
              })}
            </div>
          </>}
          <div className={cn(hidden.length > 0 && "mt-3")}>
            <div className="mb-1 text-[10.5px] font-medium text-ink-4">{t("预设供应商 · 预填接口，Key 由你自己填写")}</div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {templates.filter((tpl) => !providers.some((p) => p.openai_base === tpl.openai_base)).map(
              (tpl) => (
                <div
                  key={`tpl:${tpl.name}`}
                  className="flex items-center gap-2 px-3 h-11 rounded-xl border border-white/[0.08] bg-bg-1/60 hover:border-white/[0.16] transition-colors"
                >
                  <div className="min-w-0 flex-1">
                    <div className="text-[12px] font-medium text-ink-1 truncate">{tpl.name}</div>
                    <div className="text-[9.5px] font-mono text-ink-5 truncate" title={tpl.openai_base}>
                      {tpl.openai_base.replace(/^https?:\/\//, "")}
                    </div>
                  </div>
                  <button
                    onClick={() => openAddTemplate(tpl)}
                    title={t("添加：预填地址/模型，进弹窗只需补 Key")}
                    className="shrink-0 grid place-items-center w-8 h-8 rounded-lg border border-white/[0.10] text-ink-2 hover:text-accent hover:bg-white/[0.04]"
                  >
                    <Plus size={13} />
                  </button>
                  {tpl.key_url && (
                    <button
                      onClick={() => openUrl(tpl.key_url!).catch(() => {})}
                      title={t("申请 Key")}
                      className="shrink-0 grid place-items-center w-8 h-8 rounded-lg border border-white/[0.10] text-ink-3 hover:text-ink-1 hover:bg-white/[0.04]"
                    >
                      <KeyRound size={12} />
                    </button>
                  )}
                  {tpl.website && (
                    <button
                      onClick={() => openUrl(tpl.website!).catch(() => {})}
                      title={t("官网介绍")}
                      className="shrink-0 grid place-items-center w-8 h-8 rounded-lg border border-white/[0.10] text-ink-3 hover:text-ink-1 hover:bg-white/[0.04]"
                    >
                      <ExternalLink size={12} />
                    </button>
                  )}
                </div>
              ),
            )}
            </div>
          </div>
        </div>

            {/* ③ 合并启动 —— 「装 → 配 → 用」这条线的收口。
                🔴 **没有「保存」这一步**：`switchOneTool` 是选完立即生效的，
                硬造一颗「保存」按钮等于让界面撒谎（客户点了会以为刚才没生效）。
                所以这里只有「启动」。
                🔴 未装的显示「装好并启动」而不是「启动」—— 装没装读的是 `list_tools`
                的真实结果，不是 `toolInstalledOf`（那个对 claude/codex 恒真，
                会让没装的工具也显示「启动」，点了什么都不发生）。
                拿不到清单时（`null` = 不知道）按「已装」渲染但文案不承诺，见下。 */}
            {(() => {
              const inst = realInstalled(tools, activeTab);
              const label = TOOL_TABS.find((x) => x.target === activeTab)?.label ?? activeTab;
              const isGui = activeTab === "clawx";
              return (
                <div className="mt-5 pt-4 border-t border-white/[0.06] flex flex-wrap items-center gap-3">
                  <button
                    onClick={() => (inst === false ? onInstallTool?.(activeTab) : onLaunchTool?.(activeTab))}
                    disabled={!!busy}
                    data-action-id={inst === false ? undefined : "runtime.tool.launch"}
                    className="inline-flex items-center gap-2 h-10 px-5 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-50 shadow-sm"
                  >
                    <Power size={15} />
                    {inst === false
                      ? t("装好 {name} 并启动", { name: label })
                      : t("启动 {name}", { name: label })}
                  </button>
                  <span className="text-[11.5px] text-ink-4">
                    {inst === false
                      ? t("还没装 —— 会先走装机流程，装完再按上面选好的驱动启动")
                      : isGui
                        ? t("ClawX 是独立桌面应用，会单独开一个窗口")
                        : t("按上面选好的驱动，在 U-CLI 里开一个配好的会话（想拉出去有「拉出」按钮）")}
                  </span>
                </div>
              );
            })()}
          </div>
        </div>

        {/* 「AI 作图」方格 —— 本分区里第五件「要配供应商」的事。
            🔴 立项理由：作图原来把端点**写死**在 providers.rs 里（两个常量 + 固定用设备钱包
            Key），客户想用自己的图像 API 一点办法都没有，而这一页恰恰是他会来找的地方。
            现在虾盘云是**默认**不是唯一。
            为什么单独一张卡而不是塞进上面那列工具卡：那一列的每一项都是「机器上的一个外部
            程序」（要装、要启动、有「已接管」状态），作图三样都没有 —— 混进去会让「未安装」
            这类徽章对它失去意义，而徽章一旦有例外就没人再信它。 */}
        <div className="mt-5 rounded-card border border-white/[0.08] bg-bg-1/70 p-4">
          <div className="flex items-center gap-2 flex-wrap">
            <ImageIcon size={16} className="text-accent shrink-0" />
            <span className="text-[13px] font-semibold text-ink-0">{t("AI 作图")}</span>
            <span className="text-[11px] text-ink-4">{t("作图 / 图生图用哪家")}</span>
            {drawRoute && (
              <span
                className={cn(
                  "ml-auto inline-flex items-center px-2 h-[18px] rounded-full text-[9.5px] font-semibold shrink-0",
                  drawRoute.builtin
                    ? "bg-accent/[0.14] text-accent border border-accent/25"
                    : "bg-success-500/12 text-success-400 border border-success-500/25",
                )}
              >
                {drawRoute.builtin
                  ? t("内置（虾盘云钱包计费）")
                  : t("走 {name}", { name: drawRoute.provider_name })}
              </span>
            )}
          </div>

          <div className="mt-3 flex flex-wrap items-end gap-2.5">
            <label className="flex-1 min-w-[170px]">
              <span className="block mb-1 text-[10.5px] text-ink-5">{t("供应商")}</span>
              <select
                value={drawPick}
                onChange={(e) => setDrawPick(e.target.value)}
                className={IPT}
              >
                {drawProviders.map((p) => (
                  <option key={p.id} value={p.id}>
                    {t(p.name)}
                  </option>
                ))}
              </select>
            </label>
            {/* 🔴 虾盘云这条**故意不给模型框**：模型的真相源是「AI 作图」页那个富下拉
                （带每个模型的优缺点，且某家上游挂了能当场换）。这里再放一个能填的框，
                就是同一事实的第二份 —— 必然漂移，而且漂了以后没人说得清哪个说了算。 */}
            {drawPick === DRAW_BUILTIN_ID ? (
              <div className="flex-1 min-w-[190px] pb-2 text-[11px] text-ink-4 leading-snug">
                {t("模型在「AI 作图」页那个下拉里选（带优缺点说明，随时能换一家上游）")}
              </div>
            ) : (
              <label className="flex-1 min-w-[170px]">
                <span className="block mb-1 text-[10.5px] text-ink-5">{t("模型（必填）")}</span>
                <input
                  value={drawModel}
                  onChange={(e) => setDrawModel(e.target.value)}
                  placeholder={t("这家的作图模型 id，如 flux-pro / dall-e-3")}
                  className={IPT}
                />
              </label>
            )}
            <button
              onClick={() => void applyDrawRoute()}
              disabled={drawBusy || !drawPick || (drawPick !== DRAW_BUILTIN_ID && !drawModel.trim())}
              className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-50 shrink-0"
            >
              {drawBusy ? <Loader2 size={13} className="animate-spin" /> : <CheckCircle2 size={13} />}
              {t("应用")}
            </button>
          </div>

          {/* 回显「到底打谁」—— 客户报「作图不出图」时，这一行比任何截图都省事。 */}
          {drawRoute && (
            <div className="mt-2.5 space-y-1">
              <div className="text-[10px] text-ink-5 font-mono truncate" title={drawRoute.gen_url}>
                {drawRoute.gen_url}
              </div>
              <div className="text-[10.5px] text-ink-4">
                {drawRoute.builtin
                  ? t("默认：走内置虾盘云端点，用这台机器的钱包 Key 计费")
                  : t("用这家自己的 Key 计费（在「供应商库」里填），不再从虾盘云钱包扣钱")}
              </div>
            </div>
          )}
        </div>
      </section>
      )}

      {/* 供应商库 —— **一家 API 只登记一次，四个 AI 都从这儿引用**。
          原来「添加供应商」只藏在「工具分配」右栏顶部：客户得先随便选中一个 AI，才够得着
          那颗按钮，于是「加一家自己的 API」看起来像是「给这个 AI 加一家」。实际上
          `save_provider` 存的是 `~/.uking/providers.json`，本来就是全局的 ——
          界面把一件全局的事画成了局部的事，这正是「容易弄错」的来源之一。
          这里只做**呈现**：增删改仍走 CustomProviderModal / delete_provider 那份唯一实现。 */}
      {settingsTab === "providers" && (
        <section className="space-y-3">
          <div className="flex items-end justify-between gap-4 flex-wrap">
            <div>
              <h3 className="text-[14px] font-semibold text-ink-0">{t("统一供应商库")}</h3>
              <p className="mt-1 text-[11px] text-ink-4">
                {t("一处登记，所有 AI 共用，Key 只填一次。改一处，用到它的 AI 全都跟着变。")}
              </p>
            </div>
            <button
              onClick={() => openNewCustomProvider()}
              className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 shadow-sm shrink-0"
            >
              <Plus size={14} /> {t("添加供应商")}
            </button>
          </div>

          <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
            {[...providers, ...addable.filter((a) => !providers.some((p) => p.id === a.id))]
              .filter((p) => p.id !== "official")
              .map((p) => {
                // 「哪几个 AI 正在用它」—— 从 driver.active 反查。这是客户最想知道、
                // 而原来整页都答不上来的一件事：改这家之前，先看清会影响到谁。
                const usedBy = Object.entries(driver?.active ?? {})
                  .filter(([, id]) => id === p.id)
                  .map(([tool]) => TOOL_LABELS[tool] ?? tool);
                return (
                  <div
                    key={p.id}
                    className="rounded-card border border-white/[0.08] bg-bg-1/70 p-3.5 hover:border-white/[0.16] transition-colors"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-[12.5px] font-semibold text-ink-0 truncate">{t(p.name)}</span>
                      {p.builtin_recharge && (
                        <span className="text-[9px] px-1.5 py-0.5 rounded bg-accent/[0.16] text-accent shrink-0">{t("内置")}</span>
                      )}
                      {!p.builtin && (
                        <span className="text-[9px] px-1.5 py-0.5 rounded bg-white/[0.06] text-ink-4 shrink-0">{t("自定义")}</span>
                      )}
                      {!p.builtin && (
                        <button
                          onClick={() => setEditing(p)}
                          title={t("编辑")}
                          className="ml-auto grid place-items-center w-7 h-7 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] shrink-0"
                        >
                          <Pencil size={12} />
                        </button>
                      )}
                    </div>
                    <div className="mt-2.5 space-y-1 text-[10.5px] text-ink-4">
                      <div className="flex justify-between gap-2">
                        <span>{t("地址")}</span>
                        <span className="font-mono text-ink-3 truncate" title={p.openai_base || p.anthropic_base || ""}>
                          {(p.openai_base || p.anthropic_base || "—").replace(/^https?:\/\//, "")}
                        </span>
                      </div>
                      <div className="flex justify-between gap-2">
                        <span>{t("默认模型")}</span>
                        <span className="font-mono text-ink-3 truncate">{p.model || "—"}</span>
                      </div>
                    </div>
                    {/* 空着不写「没人用」——「当前没有 AI 引用它」和「我们没查出来」在界面上
                        长得一样，而后者会误导人去删掉正在用的东西。有才说，没有就不说。 */}
                    {usedBy.length > 0 && (
                      <div className="mt-2.5 pt-2.5 border-t border-white/[0.06] text-[10px] text-success-400">
                        {t("已在 {n}/{total} 个工具启用:{tools}", {
                          n: usedBy.length,
                          total: TOOL_TABS.length,
                          tools: usedBy.join(" · "),
                        })}
                      </div>
                    )}
                    {/* 设备钱包只挂在虾盘云（builtin_recharge）这张卡上：余额和内置 Key 是**这家**
                        供应商的东西，不是 U-King 的全局功能。客户把虾盘云删掉，这块跟着不见；
                        从「添加供应商」把它加回来，钱包也跟着回来（Key 在后端，不会因此丢）。 */}
                    {p.builtin_recharge && providers.some((x) => x.id === p.id) && (
                      <div className="mt-2.5 pt-2.5 border-t border-white/[0.06]">
                        <button
                          onClick={() => setWalletOpen((v) => !v)}
                          className="flex w-full items-center gap-1.5 text-[11px] text-ink-3 hover:text-ink-1"
                        >
                          <Wallet size={12} className="text-accent" />
                          {t("设备钱包")}
                          <span className="text-[10px] text-ink-5">{t("余额 · 充值 · 换一把 Key")}</span>
                          {walletOpen ? (
                            <ChevronUp size={13} className="ml-auto" />
                          ) : (
                            <ChevronDown size={13} className="ml-auto" />
                          )}
                        </button>
                        {walletOpen && (
                          <WalletCard
                            className="mt-2"
                            deviceKey={deviceKey}
                            onDeviceKeyChange={(dk) => {
                              setDeviceKey(dk);
                              onDeviceKeyChange?.(dk);
                            }}
                            onRecharge={() =>
                              onRecharge ? onRecharge(deviceKey?.recharge_url) : openRecharge(deviceKey?.recharge_url)
                            }
                            onToast={flash}
                          />
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
          </div>
        </section>
      )}

      {/* 高级 —— 桌面 App 状态 / Codex 专区，都是低频。「用自己的 Key」那块网格
          已在 P3a 合并进「工具分配」Tab 的画廊（见上面 2026-08-22 删的注释），
          这里的文案别再指着一个已经搬走的东西。
          同「用量账单」：自己占一个分区后 `open` 常开，不再让人多点一下。 */}
      {settingsTab === "advanced" && (
      <details open className="group rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
        <summary className="flex items-center gap-2 px-4 py-3 cursor-pointer select-none list-none text-[13px] font-medium text-ink-1 hover:bg-white/[0.02]">
          <Settings size={14} className="text-accent" />
          {t("更多设置")}
          <span className="text-[10.5px] text-ink-4 font-normal">{t("桌面 App · Codex 专区")}</span>
          <ChevronDown size={15} className="ml-auto text-ink-4 transition-transform group-open:rotate-180" />
        </summary>
        <div className="px-4 pb-4 pt-1 space-y-5">

      {/* 桌面 App（ClawX / Hermes）状态条 —— 小白在「AI 设置」一眼看到装没装。
          ClawX 已有上方 Tab 可一键切（apply_clawx_managed），「配置 →」直接跳那个 Tab；
          Hermes 桌面版仍走进阶页「复制 Key 到设置」图文教程。 */}
      {onGoAdvanced && (
        <section className="rounded-card border border-white/[0.08] bg-bg-1/60 px-5 py-4 shadow-card">
          <div className="flex items-center justify-between gap-2 mb-3">
            <h3 className="text-[14px] font-semibold text-ink-0">{t("桌面 App（图形版）")}</h3>
            <span className="text-[10.5px] text-ink-4">{t("ClawX / Hermes 都可在上方 Tab 一键切")}</span>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            {[
              { icon: "clawx", name: "ClawX", installed: !!driver?.clawx_installed, model: driver?.clawx_model, goTab: "clawx" },
              { icon: "hermes", name: "Hermes", installed: !!driver?.hermes_installed, model: driver?.hermes_model, goTab: "hermes" },
            ].map((app) => (
              <div
                key={app.icon}
                className="flex items-center gap-3 rounded-xl border border-white/[0.06] bg-bg-1/60 px-4 py-3 hover:border-white/[0.12] transition-colors"
              >
                <ToolIcon tool={app.icon} size={20} active={app.installed} className="shrink-0" />
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] font-medium text-ink-0 flex items-center gap-1.5">
                    {app.name}
                    {app.installed ? (
                      <span className="inline-flex items-center px-1.5 h-[16px] rounded-full text-[9px] font-semibold bg-success-500/12 text-success-400 border border-success-500/25">
                        {t("已装")}
                      </span>
                    ) : (
                      <span className="text-[10px] text-ink-4">{t("未装")}</span>
                    )}
                  </div>
                  <div className="text-[11px] font-mono text-ink-4 truncate">
                    {app.installed ? app.model || t("待配置模型") : t("去「我的 AI」安装")}
                  </div>
                </div>
                <button
                  onClick={() => {
                    // ClawX：设置页里就能切（上方 Tab），别再把人送去教程页绕路
                    if (app.goTab) {
                      setActiveTab(app.goTab);
                      setSelected(null);
                      window.scrollTo({ top: 0, behavior: "smooth" });
                    } else {
                      onGoAdvanced?.();
                    }
                  }}
                  className="shrink-0 inline-flex items-center gap-1 px-3 h-8 rounded-lg border border-white/[0.10] text-[11px] text-ink-2 hover:text-accent hover:bg-white/[0.04] transition-colors"
                >
                  {t("配置 →")}
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* 🔴 2026-08-22 删：「用自己的 Key？各模型官网与申请地址」这个独立网格（P3a）——
          它和「工具分配」Tab 里「已移除可加回」的 chips 是两套并列的添加入口，观感混乱
          （客户原话「右侧支持的还不够多、不够全」，而这里点了并不会真的把供应商加进列表，
          只给「申请 Key」/官网链接，添加还得回上面手动开弹窗选模板）。已合并成一个画廊，
          见 `settingsTab === "tools"` 里 `PROVIDER_TEMPLATES.filter(...)` 那块，点「+」
          就是 `openAddTemplate`——直接带着预填字段开弹窗，不用来回跳两个地方。 */}

      {/* Codex 专区入口（专区已并入 AI 设置，这里给个入口） */}
      {onGoCodex && (
        <section
          onClick={onGoCodex}
          className="flex items-center gap-3 rounded-card border border-white/[0.08] bg-bg-1/60 px-4 py-3.5 cursor-pointer hover:border-white/[0.14] hover:bg-white/[0.02] transition-colors shadow-sm"
        >
          <span className="grid place-items-center w-10 h-10 rounded-xl bg-accent/[0.12] shrink-0">
            <Bot size={18} className="text-accent" />
          </span>
          <div className="flex-1 min-w-0">
            <div className="text-[13px] font-semibold text-ink-0">{t("Codex 专区")}</div>
            <div className="text-[11px] text-ink-3">{t("Codex 桌面版装机 · 驱动接管 · computer use 教程")}</div>
          </div>
          <span className="text-[12px] font-medium text-accent shrink-0">{t("进入 →")}</span>
        </section>
      )}

      {/* 更多配置页（2026-08-22 从侧栏摘进来的四个）。「本地大模型」2026-08-25 升回
          侧栏「更多」（用户拍板），从这组网格里摘掉 —— 一个入口只在一处，别两边都摆。
          它们全是「配一次就不再进」的页面，
          却各占一格侧栏 —— 侧栏挤不是因为东西该塞进 chat，是配置页太多（智序对照的结论）。
          页面/路由/动作全部原样，这里只是入口；点进去还是原来那个全屏页。 */}
      {onGoPage && (
        <div className="grid gap-2 sm:grid-cols-2">
          {[
            { tab: "identity", icon: IdCard, label: t("让 AI 认识 U-King"), sub: t("往 CLAUDE.md 插一行指针 · 随时可撤") },
            { tab: "rtk", icon: Zap, label: t("Token 压缩机"), sub: t("AI 编程省 token · 不降智 · 开源 RTK") },
            { tab: "dshplugins", icon: Blocks, label: t("DSH 插件"), sub: t("打开 DeepSeek Harness · 给它装插件") },
          ].map(({ tab, icon: Icon, label, sub }) => (
            <section
              key={tab}
              onClick={() => onGoPage(tab)}
              className="flex items-center gap-3 rounded-card border border-white/[0.08] bg-bg-1/60 px-4 py-3 cursor-pointer hover:border-white/[0.14] hover:bg-white/[0.02] transition-colors shadow-sm"
            >
              <span className="grid place-items-center w-9 h-9 rounded-xl bg-white/[0.05] shrink-0">
                <Icon size={16} className="text-ink-2" />
              </span>
              <div className="flex-1 min-w-0">
                <div className="text-[12.5px] font-semibold text-ink-0">{label}</div>
                <div className="text-[10.5px] text-ink-4 truncate">{sub}</div>
              </div>
              <span className="text-[11.5px] font-medium text-accent shrink-0">{t("进入 →")}</span>
            </section>
          ))}
        </div>
      )}
        </div>
      </details>
      )}

      {providerPickerOpen && (
        <ProviderLibraryPicker
          providers={[...providers, ...addable.filter((a) => !providers.some((p) => p.id === a.id))]
            .filter((p) => p.id !== "official")}
          addableIds={new Set(addable.map((p) => p.id))}
          tool={TOOL_LABELS[activeTab] ?? activeTab}
          onRestore={(p) => restoreProvider(p.id, p.name)}
          onNew={() => {
            setProviderPickerOpen(false);
            openNewCustomProvider(true);
          }}
          onClose={() => setProviderPickerOpen(false)}
        />
      )}

      {editing && (
        <CustomProviderModal
          value={editing}
          onChange={setEditing}
          onSave={freeRouteContext ? saveFreeRoute : async (p) => {
            const saved = await saveCustom(p);
            if (!saved) return;
            if (addNewToActiveTool) await restoreProvider(saved.id, saved.name);
            setAddNewToActiveTool(false);
            setEditing(null);
          }}
          onClose={() => { setEditing(null); setFreeRouteContext(null); setAddNewToActiveTool(false); }}
          addable={addable}
          templates={templates}
          addingTo={TOOL_LABELS[activeTab] ?? activeTab}
          onAddBuiltin={async (p) => {
            setEditing(null);
            await restoreProvider(p.id, p.name);
          }}
          onPurge={purgeProvider}
          variant={freeRouteContext ? "drawer" : "modal"}
          freeRoute={freeRouteContext}
          onFreeTargetChange={(target) => setFreeRouteContext((ctx) => ctx ? { ...ctx, target } : ctx)}
          onFreeRouteDirty={() => setFreeRouteContext((ctx) => ctx?.stage === "added" ? { ...ctx, stage: "draft", savedId: undefined } : ctx)}
          onEnableFreeRoute={enableFreeRoute}
          enablingFreeRoute={freeEnabling}
        />
      )}

      {toast && (
        <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-50 animate-fade-in">
          <div className="flex items-center gap-2 rounded-full border border-white/[0.10] bg-bg-3/95 backdrop-blur px-4 py-2 text-[13px] text-ink-1 shadow-card">
            <CheckCircle2 size={14} className="text-success-400" />
            {toast}
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * 工具分配页的供应商库引用选择器。
 *
 * 这里不写 provider 定义：未勾选才调用既有 `restore_provider` 把同一 id 引回当前工具；
 * 已在列表中的项目灰显，避免把「可见」误画成一份可重复保存的配置。
 */
function ProviderLibraryPicker({
  providers,
  addableIds,
  tool,
  onRestore,
  onNew,
  onClose,
}: {
  providers: ProviderPreset[];
  addableIds: Set<string>;
  tool: string;
  onRestore: (p: ProviderPreset) => Promise<void>;
  onNew: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-black/60 backdrop-blur-sm p-4" onClick={onClose}>
      <div
        className="w-full max-w-[520px] rounded-card border border-white/[0.10] bg-bg-1 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-3.5 border-b border-white/[0.08] bg-bg-1/60">
          <div>
            <div className="text-[14px] font-semibold text-ink-0">{t("从供应商库添加")}</div>
            <p className="mt-0.5 text-[10.5px] text-ink-4">{t("勾选未在当前 AI 列表的供应商即可引用，Key 无需再次填写。")}</p>
          </div>
          <button
            onClick={onClose}
            className="grid place-items-center w-7 h-7 rounded-md text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]"
            title={t("关闭")}
          >
            <X size={15} />
          </button>
        </div>

        <div className="max-h-[55vh] overflow-y-auto p-3 space-y-1.5">
          {providers.length === 0 ? (
            <p className="px-2 py-6 text-center text-[12px] text-ink-4">{t("供应商库还没有供应商。先新建一家吧。")}</p>
          ) : providers.map((p) => {
            const alreadyVisible = !addableIds.has(p.id);
            return (
              <button
                key={p.id}
                disabled={alreadyVisible}
                onClick={() => void onRestore(p)}
                className={cn(
                  "flex w-full items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-colors",
                  alreadyVisible
                    ? "border-white/[0.06] bg-white/[0.02] text-ink-5 cursor-not-allowed"
                    : "border-white/[0.10] bg-bg-2/60 hover:border-accent/40 hover:bg-accent/[0.06]",
                )}
              >
                <span className={cn(
                  "grid place-items-center w-5 h-5 rounded-full border shrink-0",
                  alreadyVisible ? "border-success-500/40 text-success-400" : "border-white/[0.25] text-transparent",
                )}>
                  {alreadyVisible && <CheckCircle2 size={13} />}
                </span>
                <ToolIcon tool={p.builtin_recharge ? "deepseek" : p.id} size={18} active={!alreadyVisible} className="shrink-0" />
                <span className="min-w-0 flex-1">
                  <span className="block text-[12px] font-medium text-ink-1 truncate">{t(p.name)}</span>
                  <span className="block mt-0.5 text-[10px] font-mono text-ink-5 truncate">{(p.openai_base || p.anthropic_base || "—").replace(/^https?:\/\//, "")}</span>
                </span>
                <span className={cn("text-[10px] shrink-0", alreadyVisible ? "text-ink-5" : "text-accent")}>
                  {alreadyVisible ? t("已在当前 AI 列表") : t("引用到 {tool}", { tool })}
                </span>
              </button>
            );
          })}
        </div>

        <div className="flex items-center justify-between gap-3 px-5 py-3.5 border-t border-white/[0.08]">
          <span className="text-[10.5px] text-ink-4">{t("找不到要用的供应商？")}</span>
          <button
            onClick={onNew}
            className="inline-flex items-center gap-1.5 h-9 px-3.5 rounded-lg border border-accent/40 text-accent text-[12px] font-medium hover:bg-accent/[0.08]"
          >
            <Plus size={13} /> {t("新建供应商…")}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * 单个工具的 provider 紧凑行列表（cc-switch 式两步交互）。
 * 每个 provider = 一行：左品牌图标 + 名字/型号两行，生效行左侧绿条高亮。
 * **点行 = 选中（高亮 + 展开细节），点右侧「启用」按钮才真正切换**（对齐 cc-switch）。
 * 自定义供应商行 hover 出编辑 / 删除；右侧另有 测试 / 获取 Key 小图标。
 */
function ToolProviderList({
  target,
  driver,
  providers,
  busy,
  testResult,
  keyInputs,
  modelSel,
  remoteModels,
  fetchingModels,
  selected,
  onSelect,
  onKeyInput,
  onModelSel,
  onFetchModels,
  onSwitch,
  onTest,
  onAskAI,
  onAskAIToast,
  onOpenKeyUrl,
  onEdit,
  onDelete,
  onMove,
}: {
  target: string;
  driver: DriverStatus | null;
  providers: ProviderPreset[];
  busy: string | null;
  testResult: Record<string, TestResult>;
  keyInputs: Record<string, string>;
  modelSel: Record<string, string>;
  remoteModels: Record<string, string[]>;
  fetchingModels: string | null;
  selected: string | null;
  onSelect: (id: string | null) => void;
  onKeyInput: (id: string, v: string) => void;
  /** key 形如 `${target}:${providerId}` —— per-tool 记住各自选的模型 */
  onModelSel: (key: string, v: string) => void;
  onFetchModels: (p: ProviderPreset) => void;
  onSwitch: (target: string, providerId: string, model: string | null, viaBridge?: boolean) => void;
  onTest: (p: ProviderPreset) => void;
  onAskAI?: (prompt: string) => void;
  onAskAIToast: () => void;
  onOpenKeyUrl: (url: string) => void;
  onEdit: (p: ProviderPreset) => void;
  onDelete: (p: ProviderPreset) => void;
  /** 调优先级：dir=-1 上移 / 1 下移。第一位 = 首选。 */
  onMove: (id: string, dir: -1 | 1) => void;
}) {
  const { t } = useI18n();
  const installed = toolInstalledOf(driver, target);
  const activeId = toolActiveOf(driver, target);
  const activeModel = toolModelOf(driver, target);
  // 🔴 这里原来是 `const codexWarn = target === "codex"` —— 只看是不是 Codex、**不看哪家供应商**，
  //    于是每张卡片都弹同一句「国产裸名会报 not implemented」。文案挪去 lib/models.ts 按供应商分。
  const isCodex = target === "codex";

  /** 品牌图标 id（按 provider 推 ToolIcon 的 logo）。 */
  const iconOf = (p: ProviderPreset): string => {
    if (p.builtin_recharge) return "deepseek"; // 虾盘云默认 DeepSeek 系
    if (p.id === "official") return target === "codex" ? "openai" : "claude";
    return p.id; // deepseek / glm(zhipu) / kimi …
  };

  return (
    <div className="rounded-card border border-white/[0.08] bg-bg-1/60 overflow-hidden divide-y divide-white/[0.05] shadow-card">
      {!installed && (
        <div className="px-4 py-3 text-[12px] text-warning-700 dark:text-warning-400 bg-warning-500/[0.10] border-b border-warning-500/25">
          {/* 🔴 这句原来写的是「可先在**「我的 AI」**装好」—— 把人指去另一个页面。
              「左装右选」之后本页底部就有「装好 X 并启动」，那条指引成了**同一件事的第二个说法，
              而且是更差的那个**（要跳页、装完还得跳回来）。★ 加新入口时要回头看旧文案有没有
              还在指旧路 —— 出图才看见的：黄条和它正下方的按钮当场自相矛盾。 */}
          {TOOL_LABELS[target]} {t("还没安装 —— 先在这里选好驱动，再点下面的「装好并启动」，装完即按这套配置生效。")}
        </div>
      )}
      {providers.length === 0 && (
        <div className="px-4 py-6 text-center space-y-1.5">
          <div className="text-[12.5px] text-ink-2">
            {t("{tool} 的列表是空的 —— 你把它的供应商都移除了。", { tool: TOOL_LABELS[target] ?? target })}
          </div>
          <div className="text-[11px] text-ink-5">
            {t("点上面「+ 从供应商库添加」引用已有的，或在下方把移除掉的加回来。别的 AI 不受影响。")}
          </div>
        </div>
      )}
      {providers.map((p, idx, list) => {
        const active = activeId === p.id;
        const isSel = selected === p.id;
        const rowBusy = busy === `${target}:${p.id}`;
        const tr = testResult[p.id];
        // cc-switch 式：选中即展开细节（生效行也默认展开，方便看/换模型）
        const expanded = isSel || (active && selected === null);
        // 每个工具记住自己选的模型（per-tool key）；Codex 链路默认走 codex_model（如 gpt-5.3-codex）
        const mkey = `${target}:${p.id}`;
        const defaultModel = target === "codex" && p.codex_model ? p.codex_model : p.model;
        const curModel = modelSel[mkey] ?? ((active && activeModel) || defaultModel);
        // Claude Code 只说 Anthropic 协议：没有 Anthropic 端点的供应商（火山方舟等纯 OpenAI
        // 兼容中转）在这个 tab 下是配不上的。**点了才报错**等于让客户自己撞 —— 提前说清楚
        // （issue #359/#322 的另一半：向导那条路已修，per-tool 这条路也别留成暗坑）。
        const capMismatch = target === "claude" && p.id !== "official" && !p.anthropic_base?.trim();
        // 这一行正走本机翻译桥：链路里多了 U-King 自己，必须一直显示出来（不是开的时候提一次就完）。
        const viaBridge = target === "claude" && active && !!driver?.claude_via_bridge;
        const subtitle =
          p.id === "official"
            ? t("还原官方直连")
            : capMismatch
              ? t("只支持 OpenAI 格式 —— Claude Code 用不了，可在 Codex 等其它 tab 使用")
              : curModel || t("（未选模型）");
        const modelChanged = !!curModel.trim() && curModel.trim() !== (activeModel ?? "").trim();
        const canEdit = p.builtin === false; // 内置的定义由我们维护，改不了；自定义随便改
        // **谁都能移出列表**（0.9.84「列表主权归用户」），只有「官方直连（还原）」除外 ——
        // 它不是一个供应商，是「不用任何第三方」的出口；删了它用户就没地方还原官方登录了。
        const canDelete = p.id !== "official";
        const canMoveUp = idx > 0;
        const canMoveDown = idx < list.length - 1;

        // cc-switch 两步式：点行 = 选中（高亮 + 展开），不直接切；点「启用」才切。
        const onRow = () => {
          if (rowBusy || !installed) return;
          onSelect(isSel ? null : p.id);
        };

        return (
          <div
            key={p.id}
            className={cn(
              "relative transition-colors group",
              active
                ? "bg-success-500/[0.10] ring-1 ring-inset ring-success-500/50 z-10"
                : isSel
                  ? "bg-accent/[0.10] ring-1 ring-inset ring-accent/50 z-10"
                  : "hover:bg-white/[0.025]",
            )}
          >
            {/* 生效行左侧粗绿条 / 选中行左侧蓝条 —— cc-switch 式，整行描边一眼看清当前用哪个 */}
            {active ? (
              <span className="absolute left-0 top-0 bottom-0 w-[4px] bg-success-400" />
            ) : isSel ? (
              <span className="absolute left-0 top-0 bottom-0 w-[4px] bg-accent" />
            ) : null}

            {/* 主行 —— 点行=选中 */}
            <div
              role="button"
              tabIndex={0}
              onClick={onRow}
              onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onRow()}
              className={cn(
                "flex items-center gap-3 px-4 py-3.5 cursor-pointer select-none outline-none",
                (!installed || rowBusy) && "cursor-default opacity-60",
              )}
            >
              <ToolIcon tool={iconOf(p)} size={19} active={installed} className="shrink-0 opacity-90" />
              <div className="min-w-0 flex-1">
                <div className={cn("text-[13px] flex items-center gap-1.5", active ? "font-semibold text-ink-0" : "font-medium text-ink-1")}>
                  <span className="truncate">{p.name}</span>
                  {p.recommended && <span className="text-[10px] text-ink-4 shrink-0" title={t("推荐")}>★</span>}
                  {active && (
                    <span className="inline-flex items-center gap-0.5 pl-1 pr-1.5 h-[18px] rounded-full text-[9.5px] font-bold bg-success-500 text-white shrink-0">
                      <CheckCircle2 size={11} /> {t("使用中")}
                    </span>
                  )}
                </div>
                {/* ★ 第二行 = 端点 + 模型（2026-08-24 对齐 EchoBird 的信息密度）。
                    以前这行只有模型名，于是**两个同名中转分不出谁是谁** —— 客户自己加了
                    两条「DeepSeek」，一条打官方一条打中转，界面上一模一样。
                    端点是这一行里唯一能把它们区分开的东西，所以它排在模型前面。 */}
                <div className="text-[11px] font-mono text-ink-4 truncate">
                  {hostOf(p) && <span className="text-ink-5">{hostOf(p)}</span>}
                  {hostOf(p) && <span className="text-ink-6 mx-1">·</span>}
                  {subtitle}
                </div>
              </div>

              {/* ★ 延迟徽标：**常驻**，不跟着 hover 走。
                  以前测速结果只在展开面板里闪一下，于是「哪家快哪家慢」这件事
                  在列表层面完全不可见 —— 而这正是用户扫一眼列表最想知道的东西。
                  没测过 = 灰色空心，**不是绿色**（没测过不等于没问题，同回验那条）。 */}
              <LatencyBadge tr={tr} busy={rowBusy} t={t} />

              {/* 右侧操作：hover 出现（测试 / 获取 Key / 编辑 / 删除自定义） */}
              <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                {p.id !== "official" && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onTest(p);
                    }}
                    disabled={rowBusy}
                    title={t("测试连通")}
                    className="grid place-items-center w-8 h-8 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] disabled:opacity-40"
                  >
                    {rowBusy ? <Loader2 size={13} className="animate-spin" /> : <Plug size={12} />}
                  </button>
                )}
                {p.key_url && p.id !== "official" && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onOpenKeyUrl(p.key_url);
                    }}
                    title={p.builtin_recharge ? t("充值") : t("获取 Key")}
                    className="grid place-items-center w-8 h-8 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
                  >
                    <KeyRound size={12} />
                  </button>
                )}
                {/* 调优先级 —— 排第一位的就是首选。用上下箭头而不是拖拽：整行本身是可点的
                    （点行=选中展开），拖拽会跟它抢手势，箭头稳当且键盘可达。 */}
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onMove(p.id, -1);
                  }}
                  disabled={!canMoveUp}
                  title={t("上移（排前面 = 更优先）")}
                  className="grid place-items-center w-8 h-8 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] disabled:opacity-25 disabled:hover:bg-transparent"
                >
                  <ChevronUp size={13} />
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onMove(p.id, 1);
                  }}
                  disabled={!canMoveDown}
                  title={t("下移")}
                  className="grid place-items-center w-8 h-8 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] disabled:opacity-25 disabled:hover:bg-transparent"
                >
                  <ChevronDown size={13} />
                </button>
                {canEdit && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onEdit(p);
                    }}
                    title={t("编辑")}
                    className="grid place-items-center w-8 h-8 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
                  >
                    <Pencil size={12} />
                  </button>
                )}
                {canDelete && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onDelete(p);
                    }}
                    title={t("从 {tool} 的列表移除（其它 AI 保留）", { tool: TOOL_LABELS[target] ?? target })}
                    className="grid place-items-center w-8 h-8 rounded-md text-ink-4 hover:text-danger-400 hover:bg-danger-500/10"
                  >
                    <Trash2 size={12} />
                  </button>
                )}
              </div>
            </div>

            {/* 展开层：选中即展开（选模型：下拉真实清单 + 手填兜底 / 自填 Key / 启用按钮）*/}
            {expanded && (
              <div className="px-4 pb-4 -mt-0.5 space-y-3">
                {p.summary && <p className="text-[11px] text-ink-3 leading-snug bg-white/[0.02] rounded-lg px-3 py-2">{p.summary}</p>}

                {/* 选模型 —— 对所有供应商生效（official 除外）：
                    输入框可手填任意 model id，下拉=内置候选 + 点🔄拉到的真实清单。 */}
                {p.id !== "official" && (
                  <>
                    <ModelPicker
                      listId={`models-${mkey}`}
                      value={curModel}
                      options={modelOptionsFor(p, target, remoteModels[p.id] ?? [], t)}
                      fetching={fetchingModels === p.id}
                      disabled={rowBusy}
                      onChange={(v) => onModelSel(mkey, v)}
                      onFetch={() => onFetchModels(p)}
                    />
                    {/* 这条是**说明**不是警告：选错了当场报错、重选即可，没有不可逆代价。
                        所以不给它警示色、不给 ⚠️ —— 同一张卡上真正要人停一下的只有下面那条
                        「会花钱」。两条同重量并排 = 两条都没人读（客户原话：黄的跟绿的混一起看不见）。 */}
                    {/* ink-2 不是 ink-3：10.5px 小字用 ink-3 在这张绿卡上只有 4.27:1，差一点点不到 AA
                        的 4.5（ink-2 是 6.80:1）。「降成中性色」不该顺手降到读不清。 */}
                    {isCodex && codexProtocolHint(p) && (
                      <p className="text-[10.5px] text-ink-2 leading-snug px-0.5">{t(codexProtocolHint(p)!)}</p>
                    )}
                  </>
                )}

                {p.id !== "official" && (
                  <div className="flex items-center gap-2 h-9 rounded-lg border border-white/[0.10] bg-bg-1 px-2.5">
                    <KeyRound size={13} className="text-accent shrink-0" />
                    <input
                      value={keyInputs[p.id] ?? ""}
                      onClick={(e) => e.stopPropagation()}
                      onChange={(e) => onKeyInput(p.id, e.target.value)}
                      placeholder={p.builtin_recharge ? t("默认用内置 Key，可覆盖") : p.key_hint || "API Key"}
                      className="flex-1 bg-transparent outline-none text-[12px] text-ink-1 font-mono placeholder:text-ink-4"
                    />
                  </div>
                )}

                {/* cc-switch 式「启用」按钮：生效且模型没动=置灰「使用中」；
                    生效但改了模型=「应用新模型」；未生效=「启用 / 还原官方登录」。 */}
                {active && !(p.id !== "official" && modelChanged) ? (
                  <div className="space-y-1.5">
                    <button
                      disabled
                      className="w-full h-9 rounded-lg border border-success-500/30 bg-success-500/[0.08] text-success-400 text-[12px] font-semibold inline-flex items-center justify-center gap-1.5 cursor-default"
                    >
                      <CheckCircle2 size={14} /> {viaBridge ? t("使用中 · 本地翻译桥") : t("使用中")}
                    </button>
                    {/* 生效了也得一直看得见这个代价 —— 开的时候说过一次不算数，
                        客户回头看这一页时必须还能看到「为什么它依赖 U-King 开着」。 */}
                    {viaBridge && (
                      <p className="text-[10.5px] leading-snug text-warning-700 dark:text-warning-400 px-0.5">
                        {t("经本机翻译桥转发。U-King 关掉（含退出托盘）这条就断，Claude Code 会连不上。")}
                      </p>
                    )}
                  </div>
                ) : capMismatch ? (
                  // 直连配不上（只有 OpenAI 端点），但**有出路**：本机翻译桥。
                  // 🔴 代价摆在按钮正下方，不藏进 tooltip、不藏进帮助页 —— 桥跟着 U-King 活，
                  // 客户要是不知道这件事，等他哪天关掉 U-King，Claude Code 会在他毫不知情的
                  // 时候连不上，那比现在直说「配不了」还糟。
                  <div className="space-y-1.5">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        onSwitch(target, p.id, curModel, true);
                      }}
                      disabled={rowBusy || !installed}
                      title={t("Claude Code 只认 Anthropic 接口；本机起一座翻译桥，把它翻成这个供应商的 OpenAI 接口")}
                      className="w-full h-9 rounded-lg border border-accent/40 bg-accent/[0.10] text-accent text-[12px] font-semibold hover:bg-accent/[0.16] disabled:opacity-50 inline-flex items-center justify-center gap-1.5"
                    >
                      {rowBusy ? <Loader2 size={14} className="animate-spin" /> : <Power size={14} />}
                      {t("用本地翻译桥启用")}
                    </button>
                    <p className="text-[10.5px] leading-snug text-warning-700 dark:text-warning-400 px-0.5">
                      {t("这个供应商只有 OpenAI 接口，靠 U-King 在本机翻译。U-King 关掉（含退出托盘）这条就断。")}
                    </p>
                  </div>
                ) : (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onSwitch(target, p.id, p.id === "official" ? null : curModel);
                    }}
                    disabled={rowBusy || !installed}
                    className="w-full h-9 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-50 inline-flex items-center justify-center gap-1.5 shadow-sm"
                  >
                    {rowBusy ? <Loader2 size={14} className="animate-spin" /> : <Power size={14} />}
                    {p.id === "official" ? t("还原官方登录") : active ? t("应用新模型") : t("启用")}
                  </button>
                )}
              </div>
            )}

            {/* 测试结果（紧贴行底） */}
            {tr && (
              <div
                className={cn(
                  "mx-4 mb-3 text-[11px] leading-snug rounded-lg px-3 py-2 flex items-start gap-2",
                  tr.ok ? "bg-success-500/[0.08] text-success-400 border border-success-500/20" : "bg-danger-500/[0.08] text-danger-400 border border-danger-500/20",
                )}
              >
                {tr.ok ? <CheckCircle2 size={13} className="shrink-0 mt-px" /> : <XCircle size={13} className="shrink-0 mt-px" />}
                <span className="min-w-0 flex-1">{tr.ok ? `「${tr.reply}」· ${tr.latency_ms}ms` : tr.error}</span>
                {!tr.ok && onAskAI && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onAskAI(buildProviderRepairPrompt({
                        providerName: p.name,
                        baseUrl: target === "claude" ? p.anthropic_base || p.openai_base || "" : p.openai_base || p.anthropic_base || "",
                        model: curModel,
                        target,
                        error: tr.error ?? "",
                      }));
                      onAskAIToast();
                    }}
                    className="shrink-0 px-2.5 h-7 rounded-lg border border-accent/40 bg-accent/[0.08] text-[11px] font-medium text-accent hover:bg-accent/[0.16]"
                  >
                    {t("让 AI 帮我修")}
                  </button>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

/**
 * 选模型组合框（对齐 cc-switch）：一个可手填的输入框 + datalist 下拉（内置候选 + 动态拉到的
 * 真实清单）+ 右侧「🔄 拉取」按钮。手填永远可用 —— 拉不到清单也不挡用户切模型（健壮优先）。
 */
function ModelPicker({
  listId,
  value,
  options,
  fetching,
  disabled,
  onChange,
  onFetch,
}: {
  listId: string;
  value: string;
  options: { id: string; label: string }[];
  fetching: boolean;
  disabled: boolean;
  onChange: (v: string) => void;
  onFetch: () => void;
}) {
  const { t } = useI18n();
  // 贵/慎用模型成本提示（只提醒不拦截）——挡住「手填/拉取选到 gpt-5.6-sol 烧穿余额」的坑
  const pricey = priceyModelHint(value);
  return (
    <div className="space-y-1.5">
      <div className="flex items-center gap-2">
        <Cpu size={13} className="text-accent shrink-0" />
        <div className="flex-1 flex items-center h-9 rounded-lg border border-white/[0.10] bg-bg-1 px-2.5">
          <input
            list={listId}
            value={value}
            disabled={disabled}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => onChange(e.target.value)}
            placeholder={t("模型 id：下拉选，或直接手填")}
            className="flex-1 bg-transparent outline-none text-[12px] text-ink-1 font-mono placeholder:text-ink-4 disabled:opacity-50"
          />
          <datalist id={listId}>
            {options.map((o) => (
              <option key={o.id} value={o.id} label={o.label === o.id ? undefined : o.label} />
            ))}
          </datalist>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onFetch();
          }}
          disabled={fetching || disabled}
          title={t("拉取该供应商真实可用的模型清单")}
          className="shrink-0 grid place-items-center w-9 h-9 rounded-lg border border-white/[0.10] bg-bg-1 text-ink-3 hover:text-ink-1 hover:bg-white/[0.04] disabled:opacity-50"
        >
          {fetching ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
        </button>
      </div>
      {/* 「会花钱」用 danger 红、不用琥珀 —— 同一张卡上它是唯一有不可逆代价的一条（钱花掉了要不回来），
          得跟旁边那条纯说明拉开重量差。色阶必须带 dark: 变体：danger-400 是深色底专用，
          浅色主题（默认）下糊成一团，就是客户看到的那张截图。 */}
      {pricey && (
        <p className="text-[10.5px] leading-snug font-medium text-danger-700 dark:text-danger-400 rounded-md border border-danger-500/40 bg-danger-500/[0.10] px-2 py-1">
          {t(pricey)}
        </p>
      )}
    </div>
  );
}

/**
 * cc-switch 式「添加 / 编辑自定义供应商」弹窗。
 * 字段对齐 cc-switch 的自定义表单：名称 + 接口地址(base) + 模型 + API Key。
 * id 为空 = 新增（后端按 name 生成）；非空 = 编辑既有自定义项。
 */
function CustomProviderModal({
  value,
  onChange,
  onSave,
  onClose,
  addable = [],
  templates = PROVIDER_TEMPLATES,
  addingTo,
  onAddBuiltin,
  onPurge,
  variant = "modal",
  freeRoute,
  onFreeTargetChange,
  onFreeRouteDirty,
  onEnableFreeRoute,
  enablingFreeRoute = false,
}: {
  value: ProviderPreset;
  onChange: (p: ProviderPreset) => void;
  onSave: (p: ProviderPreset) => void;
  onClose: () => void;
  /** 当前这个 AI 的列表里没有、可一键加回的供应商（内置 + 被移出这个 AI 的自定义）。 */
  addable?: ProviderPreset[];
  /** 预设模板清单——调用方传的是「远程覆盖 ?? 静态兜底」（见 Manager 里的 `templates`）；
   *  不传就退回静态导入，保证这个组件单独测试/复用时不需要额外接线。 */
  templates?: ProviderTemplate[];
  /** 加到哪个 AI 的列表里（列表是 per-tool 的，得说清楚加的是谁的）。 */
  addingTo?: string;
  onAddBuiltin?: (p: ProviderPreset) => void;
  /** 彻底删除（全部 AI + 定义 + Key）。只在编辑既有自定义供应商时给。 */
  onPurge?: (p: ProviderPreset) => void;
  variant?: "modal" | "drawer";
  freeRoute?: FreeRouteContext | null;
  onFreeTargetChange?: (target: string) => void;
  /** 免费路线保存后又改了表单：旧 provider 不能再被直接启用，必须重新保存/试连。 */
  onFreeRouteDirty?: () => void;
  onEnableFreeRoute?: () => void;
  enablingFreeRoute?: boolean;
}) {
  const { t } = useI18n();
  const isEdit = !!value.id;
  /**
   * 存前试连 + 存前拉模型（2026-08-22，用户亲历「无法准确添加新的供应商」后重做）。
   *
   * 🔴 原来的流程是**盲存**：填完只能保存，Key 抄错一位 / base 少个 /v1 / 模型 id 打错，
   * 第一条报错出现在列表深处甚至切驱动失败时 —— 离「你填错的那一格」隔着三层。
   * 现在错误死在弹窗里：拉得到模型清单只证明端点可达；试连回话才证明 Key 和模型都对了。
   * 某些上游（如 OpenRouter）允许匿名读取 /models，不能把模型清单当作 Key 校验。
   */
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<TestResult | null>(null);
  const [fetchingList, setFetchingList] = useState(false);
  const [modelList, setModelList] = useState<string[]>([]);
  const [listErr, setListErr] = useState<string | null>(null);
  // 任何一格变了，上一次的试连结果就不再作数 —— 留着一个绿勾伴着改坏的表单，比没有更糟。
  const set = (patch: Partial<ProviderPreset>) => {
    setProbe(null);
    if (freeRoute?.stage === "added") onFreeRouteDirty?.();
    onChange({ ...value, ...patch });
  };

  const fetchModels = async () => {
    if (fetchingList) return;
    setFetchingList(true);
    setListErr(null);
    try {
      const ids = await invoke<string[]>("list_models_at_endpoint", {
        baseUrl: value.openai_base.trim(),
        apiKey: value.api_key ?? "",
      });
      setModelList(ids);
      // 模型还空着就替他填上第一个 —— 拉都拉到了，别让人再抄一遍
      if (!value.model.trim() && ids.length) set({ model: ids[0] });
    } catch (e) {
      setModelList([]);
      setListErr(String(e));
    } finally {
      setFetchingList(false);
    }
  };

  const runProbe = async () => {
    if (probing) return;
    setProbing(true);
    setProbe(null);
    const r = await invoke<TestResult>("probe_endpoint", {
      baseUrl: value.openai_base.trim(),
      apiKey: value.api_key ?? "",
      model: value.model.trim(),
    }).catch((e) => ({ ok: false, api: "openai", latency_ms: 0, reply: null, error: String(e) }) as TestResult);
    setProbe(r);
    setProbing(false);
  };

  /** 点预设模板 = 把 baseUrl/官网/Key 提示/默认模型一次填好（只补 Key）。null = 自定义清空。 */
  const applyTemplate = (t: ProviderTemplate | null) => {
    if (!t) {
      set({ name: "", openai_base: "", anthropic_base: null, model: "", small_model: "", key_url: "", key_hint: "API Key" });
      return;
    }
    set({
      name: t.name,
      openai_base: t.openai_base,
      anthropic_base: t.anthropic_base ?? null,
      model: t.model ?? "",
      small_model: t.small_model ?? "",
      key_url: t.key_url ?? "",
      key_hint: t.key_hint ?? "API Key",
    });
  };
  /** 当前表单的 baseUrl 命中哪个模板（高亮用）；都不命中 = 自定义。 */
  const activeTpl = templates.find((t) => t.openai_base === value.openai_base.trim());

  const canSave =
    value.name.trim().length > 0 &&
    value.openai_base.trim().length > 0 &&
    value.api_key !== undefined;

  const submit = () => {
    if (!canSave) return;
    // 新增：id 留空**交给后端生成**；编辑：保持原 id。
    // 🔴 这里以前自己算一份 slug（`name.replace(/[^a-z0-9]+/g,"-")`），中文名整串被替换成 "-"
    // → id 恒为 `custom--`，两个中文名供应商撞同一个 id、后加的静默覆盖先加的（issue #359
    // 客户机上就是 `custom--`）。判据只留后端一份（宪法第 8 条）。
    onSave({
      ...value,
      id: value.id,
      builtin: false,
      builtin_recharge: false,
      // anthropic_base 留空表示纯 OpenAI 兼容；填了则 Claude Code 走 Anthropic 格式
      anthropic_base: value.anthropic_base?.trim() ? value.anthropic_base.trim() : null,
      small_model: value.small_model?.trim() || value.model.trim(),
    });
  };

  const isDrawer = variant === "drawer";
  return (
    <div
      className={cn("fixed inset-0 z-[60]", isDrawer ? "pointer-events-none" : "grid place-items-center bg-black/60 backdrop-blur-sm p-4")}
      onClick={isDrawer ? undefined : onClose}
    >
      <div
        className={cn(
          "border border-white/[0.10] bg-bg-1 shadow-card",
          isDrawer ? "pointer-events-auto absolute right-0 top-0 h-full w-full max-w-[480px] rounded-l-card flex flex-col" : "w-full max-w-[440px] rounded-card",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 h-13 py-3.5 border-b border-white/[0.08] bg-bg-1/60">
          <div className="text-[14px] font-semibold text-ink-0">
            {isDrawer ? t("正在接入：{name}", { name: freeRoute?.entry.name ?? value.name }) : isEdit ? t("编辑供应商") : t("添加供应商")}
          </div>
          <button
            onClick={onClose}
            className="grid place-items-center w-7 h-7 rounded-md text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]"
          >
            <X size={15} />
          </button>
        </div>

        {isDrawer && freeRoute && (
          <div className="px-5 py-3 border-b border-white/[0.08] bg-accent/[0.045] text-[11px] leading-relaxed text-ink-3">
            <div className="flex items-center gap-2 text-ink-1 font-medium">
              <span className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-emerald-500">{t("免费档")}</span>
              <span>{freeRoute.entry.region ?? t("第三方")}</span>
              <select value={freeRoute.target} onChange={(e) => onFreeTargetChange?.(e.target.value)} className="ml-auto rounded border border-white/[0.12] bg-bg-2 px-1.5 py-1 text-ink-1">
                {(freeRoute.entry.targets ?? ["pi"]).map((target) => <option key={target} value={target}>{TOOL_LABELS[target] ?? target}</option>)}
              </select>
            </div>
            <div className="mt-1">{freeRoute.stage === "added" ? t("已添加：Key 和供应商已保存到本机，尚未启用给任何 AI。") : t("默认：仅此第三方来源；不使用虾盘钱包，不扣费。")}</div>
          </div>
        )}

        <div className={cn("px-5 py-4 space-y-3 overflow-y-auto", isDrawer ? "flex-1" : "max-h-[70vh]")}>
          {/* ★「U-King 内置 · 一键添加」——「添加」是用户主动伸手的时刻，摆在这里才不算抢。
              主列表默认只留虾盘云 + 官方直连，其余内置（DeepSeek/GLM/Kimi/Ollama）都在这一排；
              虾盘云被移除后，这里也是它**唯一**的常规回归入口（另一条是列表底部「已移除」那行，
              只在亲手删过之后才出现）。点一下即成 —— 内置的端点/Key 我们已经配好，不用填表。 */}
          {!isEdit && !isDrawer && addable.length > 0 && (
            <div>
              <div className="text-[12px] font-medium text-ink-1 mb-1.5 flex items-center gap-1.5">
                <Zap size={12} className="text-accent" />
                {addingTo ? t("一键加进 {tool} 的列表", { tool: addingTo }) : t("U-King 内置 · 一键添加")}
                <span className="text-[10.5px] font-normal text-ink-4">{t("免填表")}</span>
              </div>
              <div className="grid grid-cols-2 gap-1.5">
                {addable.map((p) => (
                  <button
                    key={p.id}
                    onClick={() => onAddBuiltin?.(p)}
                    title={p.summary}
                    className="flex items-center gap-2 px-2.5 h-11 rounded-lg border border-white/[0.10] bg-bg-2/60 text-left hover:border-accent/40 hover:bg-accent/[0.06] transition-colors"
                  >
                    <ToolIcon
                      tool={p.builtin_recharge ? "deepseek" : p.id}
                      size={17}
                      active
                      className="shrink-0 opacity-90"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="text-[11.5px] font-medium text-ink-1 truncate">{p.name}</div>
                      <div className="text-[10px] text-ink-4 truncate">
                        {p.builtin_recharge
                          ? t("内置 Key，免注册")
                          : p.key_hint || t("需自备 API Key")}
                      </div>
                    </div>
                    <Plus size={13} className="shrink-0 text-ink-4" />
                  </button>
                ))}
              </div>
              <div className="mt-2 border-t border-white/[0.06] pt-2.5 text-[10.5px] text-ink-4">
                {t("下面是自己填 —— 任何 OpenAI 兼容的中转 / 官方接口都能加。")}
              </div>
            </div>
          )}

          {/* 预设模板库（小型精选，对齐 cc-switch）：点一个自动填好地址，只需补 Key。仅新增时显示。 */}
          {!isEdit && !isDrawer && (
            <div>
              <div className="text-[12px] font-medium text-ink-1 mb-1.5">{t("预设供应商")}</div>
              <div className="flex flex-wrap gap-1.5">
                <button
                  onClick={() => applyTemplate(null)}
                  className={cn(
                    "px-2.5 h-7 rounded-md text-[11.5px] font-medium border transition-colors",
                    !activeTpl
                      ? "bg-accent text-white border-accent"
                      : "border-white/[0.10] text-ink-2 hover:bg-white/[0.04]",
                  )}
                >
                  {t("自定义")}
                </button>
                {templates.map((tpl) => {
                  const on = activeTpl?.name === tpl.name;
                  return (
                    <button
                      key={tpl.name}
                      onClick={() => applyTemplate(tpl)}
                      title={tpl.openai_base}
                      className={cn(
                        "px-2.5 h-7 rounded-md text-[11.5px] font-medium border transition-colors",
                        on
                          ? "bg-accent text-white border-accent"
                          : "border-white/[0.10] text-ink-2 hover:bg-white/[0.04]",
                      )}
                    >
                      {tpl.name}
                    </button>
                  );
                })}
              </div>
              <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">
                {t("💡 点一个自动填好接口地址，下方只需补 API Key；选「自定义」则全部手填。存好后可在列表里「🔄 拉取」选具体模型。")}
              </div>
            </div>
          )}

          <Field label={t("名称")} hint={t("给这个供应商起个名字，如「我的中转」")}>
            <input
              value={value.name}
              onChange={(e) => set({ name: e.target.value })}
              placeholder={t("我的供应商")}
              className={IPT}
            />
          </Field>

          <Field label={t("接口地址 (Base URL)")} hint={t("OpenAI 兼容接口，一般以 /v1 结尾")}>
            <input
              value={value.openai_base}
              onChange={(e) => set({ openai_base: e.target.value })}
              placeholder="https://api.example.com/v1"
              className={cn(IPT, "font-mono")}
            />
          </Field>

          <Field label="API Key">
            <input
              value={value.api_key ?? ""}
              onChange={(e) => set({ api_key: e.target.value })}
              placeholder="sk-..."
              className={cn(IPT, "font-mono")}
            />
          </Field>

          <Field label={t("模型")} hint={t("填好地址和 Key 后点「拉取」，从这家真实有的模型里选 —— 不用去官网抄")}>
            <div className="flex items-center gap-1.5">
              <input
                value={value.model}
                onChange={(e) => set({ model: e.target.value })}
                placeholder="gpt-4o / deepseek-v4-flash ..."
                list="add-provider-models"
                className={cn(IPT, "font-mono flex-1 min-w-0")}
              />
              <button
                onClick={fetchModels}
                disabled={fetchingList || !value.openai_base.trim()}
                title={t("从接口拉取真实模型清单；部分供应商允许匿名读取，Key 请用「测试连通」验证")}
                className="shrink-0 inline-flex items-center gap-1 h-9 px-2.5 rounded-lg border border-white/[0.10] text-ink-2 text-[11.5px] hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
              >
                {fetchingList ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
                {t("拉取")}
              </button>
            </div>
            <datalist id="add-provider-models">
              {modelList.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
            {modelList.length > 0 && (
              <p className="mt-1 text-[10.5px] text-success-400">
                {t("✓ 拉到 {n} 个模型 —— 接口地址可达；Key 请点「测试连通」确认。点输入框从清单里选", { n: modelList.length })}
              </p>
            )}
            {listErr && (
              <p className="mt-1 text-[10.5px] leading-snug text-danger-400 break-all">{listErr}</p>
            )}
          </Field>

          <details className="group">
            <summary className="cursor-pointer text-[11.5px] text-ink-3 hover:text-ink-1 list-none select-none">
              {t("＋ 高级（小模型 / Claude 格式地址，可不填）")}
            </summary>
            <div className="mt-2.5 space-y-3">
              <Field label="Small Model" hint={t("省 token 的轻量模型，留空 = 同上")}>
                <input
                  value={value.small_model}
                  onChange={(e) => set({ small_model: e.target.value })}
                  placeholder={t("留空则用上面的模型")}
                  className={cn(IPT, "font-mono")}
                />
              </Field>
              <Field label="Anthropic Base URL" hint={t("给 Claude Code 用的 Anthropic 格式地址；纯 OpenAI 接口留空")}>
                <input
                  value={value.anthropic_base ?? ""}
                  onChange={(e) => set({ anthropic_base: e.target.value })}
                  placeholder={t("留空 = 仅 OpenAI 兼容")}
                  className={cn(IPT, "font-mono")}
                />
              </Field>
            </div>
          </details>
        </div>

        {/* 试连结果 —— 紧贴按钮区，成败都说人话。绿 = 这套填法真能回话；红 = 原样透出上游报错，
            此刻表单还开着，改完再试，不用保存-失败-再回来。 */}
        {probe && (
          <div
            className={cn(
              "mx-5 mb-2.5 rounded-lg px-3 py-2 text-[11px] leading-snug flex items-start gap-2 border",
              probe.ok
                ? "bg-success-500/[0.08] text-success-400 border-success-500/20"
                : "bg-danger-500/[0.08] text-danger-400 border-danger-500/20",
            )}
          >
            {probe.ok ? <CheckCircle2 size={13} className="shrink-0 mt-px" /> : <XCircle size={13} className="shrink-0 mt-px" />}
            <span className="min-w-0 break-all">
              {probe.ok ? t("「{reply}」· {ms}ms · 可以保存了", { reply: probe.reply ?? "", ms: probe.latency_ms }) : probe.error}
            </span>
          </div>
        )}
        <div className="flex items-center justify-end gap-2 px-5 py-3.5 border-t border-white/[0.08]">
          {/* 「彻底删除」只在编辑既有自定义供应商时出现 —— 行里那个垃圾桶是高频的
              「这个 AI 不用它」（只移出当前列表），真要连定义带 Key 一起毁得进来这里点，
              免得顺手把别的 AI 还在用的东西也删了。 */}
          {isEdit && !value.builtin && onPurge && (
            <button
              data-action-id="runtime.provider.delete"
              onClick={() => onPurge(value)}
              title={t("从全部 AI 的列表里删掉，并销毁它的地址和已保存的 Key")}
              className="mr-auto inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-danger-500/25 text-danger-400 text-[12px] font-medium hover:bg-danger-500/10 transition-colors"
            >
              <Trash2 size={13} /> {t("彻底删除")}
            </button>
          )}
          <button
            onClick={runProbe}
            disabled={probing || !value.openai_base.trim()}
            title={t("用当前填的地址 / Key / 模型真发一条消息 —— 通了再保存，错了当场看到原因")}
            className="inline-flex items-center gap-1.5 h-9 px-3.5 rounded-lg border border-accent/40 text-accent text-[12px] font-medium hover:bg-accent/[0.08] disabled:opacity-40 transition-colors"
          >
            {probing ? <Loader2 size={13} className="animate-spin" /> : <Zap size={13} />}
            {t("测试连通")}
          </button>
          <button
            onClick={onClose}
            className="h-9 px-4 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] font-medium hover:bg-white/[0.04] transition-colors"
          >
            {t("取消")}
          </button>
          {freeRoute?.stage === "added" ? (
            <button onClick={onEnableFreeRoute} disabled={enablingFreeRoute} className="h-9 px-5 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40 shadow-sm transition-colors">
              {enablingFreeRoute ? t("验证并启用中…") : t("启用到 {tool}", { tool: TOOL_LABELS[freeRoute.target] ?? freeRoute.target })}
            </button>
          ) : (
            <button
              onClick={submit}
              disabled={!canSave}
              className="h-9 px-5 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40 shadow-sm transition-colors"
            >
              {isDrawer ? t("保存 Key 和供应商") : t("保存")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** 表单字段包裹（标签 + 说明 + 输入区）。 */
function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block">
      <div className="text-[12px] font-medium text-ink-0 mb-2">{label}</div>
      {children}
      {hint && <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">{hint}</div>}
    </label>
  );
}
