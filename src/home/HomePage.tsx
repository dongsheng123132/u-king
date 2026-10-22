/**
 * 「我的 AI」首页 —— 2026-10-22 用户拍板走「B 极简」样张，**同一天按 A 方案收回软件库栏目**：
 *   一句话状态 → 四个动作块 → 已装图标墙（点一下启动，右上角 ⋯ 展开管理）→ 可安装清单 → 每轮底座账单。
 *
 * 设计口径（为什么长这样）：
 *  - 360 管家的**事**（装 / 升 / 修 / 省），老外软件的**脸**（浅色、单蓝、少盒子、留白）。
 *  - 「装」和「管」在**同一页**：顶部两个切换（已装 / 可安装）。竞品 EchoBird 的「应用管理」
 *    也是这个形态 —— 多开一个「软件库」侧栏项只会把一件事拆成两页、核心导航从 4 项变 5 项。
 *  - 已装用**图标卡片**：一眼看到装了什么、点一下就开始用（这是原来瓷砖墙的优点，别为了极简丢掉）；
 *    换模型 / 升级修复 / 让 AI 修 / 卸载收进「⋯」里。
 *  - 可安装区带分类 + 搜索；装不上就点「让 AI 装」——把脱敏任务包填进轻助手，不自动发送。
 *
 * 数据来源：全部沿用 App.tsx 现有回调与 `list_tools`；「每轮底座账单」只读 `context_bill`，
 * 不估账、不猜账（本体是实测常量，技能/记忆是这台机器上的真实文件统计）。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  ArrowUpRight,
  ChevronDown,
  Coins,
  Download,
  MoreHorizontal,
  Package,
  Play,
  RefreshCw,
  Search,
  Settings2,
  Sparkles,
  Trash2,
  Wrench,
} from "lucide-react";
import type { TermSnapshotInfo, ToolInfo } from "../App";
import { RECOMMENDED_TOOLS, canUninstallTool } from "../App";
import { ToolIcon } from "../components/ToolIcon";
import { buildAiInstallPrompt, buildAiRepairPrompt } from "../lib/toolPrompt";
import {
  TOOL_CATEGORIES,
  categoryLabel,
  categoryOf,
  sortForLibrary,
  type ToolCategory,
} from "../lib/toolMeta";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

type SkillStat = { dir: string; count: number; chars: number };
type MemoryFile = { key: string; path: string; bytes: number };
/** 后端 `context_bill` 的返回形状（src-tauri/src/context_bill.rs）。 */
type ContextBill = {
  claude_base_tokens: number;
  claude_skills: SkillStat;
  claude_skills_off: SkillStat;
  uking_skills: SkillStat;
  memory_files: MemoryFile[];
};

/** 大数字说人话：2.07 万 / 8,400。只用于展示，不参与任何判断。 */
function humanNum(n: number): string {
  if (n >= 10000) {
    const v = (n / 10000).toFixed(2).replace(/\.?0+$/, "");
    return `${v} 万`;
  }
  return n.toLocaleString("zh-CN");
}

function humanBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

/** 字符数（技能说明合计）：2.5万字 这种紧凑写法。 */
function humanChars(n: number): string {
  if (n >= 10000) return `${(n / 10000).toFixed(1).replace(/\.0$/, "")}万`;
  return n.toLocaleString("zh-CN");
}

const MEMORY_LABEL: Record<string, string> = {
  claude_md: "Claude 全局记忆",
  agents_md: "Codex 全局记忆",
  llms_txt: "U-King 说明书",
};

type View = "installed" | "installable";

export function HomePage({
  tools,
  onLaunch,
  onInstallSelection,
  onUninstall,
  onManageProviders,
  onGoDoctor,
  onGoManage,
  onGoSkillPack,
  onGoSetup,
  onRefreshTools,
  onAskAiToFix,
  termSnapshot,
  recoveringTermSnapshot,
  failedTermRestoreCount = 0,
  onRestoreTermSnapshot,
  onDismissTermSnapshot,
}: {
  tools: ToolInfo[];
  onLaunch: (t: ToolInfo) => void | Promise<void>;
  onInstallSelection: (ids: string[]) => void;
  onUninstall: (t: ToolInfo) => void;
  onManageProviders: (editId?: string, tool?: string) => void;
  onGoDoctor: () => void;
  onGoManage: () => void;
  onGoSkillPack: () => void;
  onGoSetup: () => void;
  onRefreshTools: () => Promise<ToolInfo[] | void> | void;
  onAskAiToFix: (prompt: string) => void;
  termSnapshot?: TermSnapshotInfo | null;
  recoveringTermSnapshot?: boolean;
  failedTermRestoreCount?: number;
  onRestoreTermSnapshot?: () => void;
  onDismissTermSnapshot?: () => void;
}) {
  const { t } = useI18n();
  const [view, setView] = useState<View>("installed");
  const [category, setCategory] = useState<ToolCategory>("all");
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [budgetOpen, setBudgetOpen] = useState(false);
  const [bill, setBill] = useState<ContextBill | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const installRef = useRef<HTMLElement | null>(null);
  const autoSwitched = useRef(false);

  // 只读账单：打不开就整块显示「读取中」，不弹错、不挡首屏。
  useEffect(() => {
    let alive = true;
    invoke<ContextBill>("context_bill")
      .then((b) => {
        if (alive) setBill(b);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  const visible = useMemo(() => tools.filter((x) => !x.hidden), [tools]);
  const installed = useMemo(() => visible.filter((x) => x.installed), [visible]);
  const installable = useMemo(() => visible.filter((x) => !x.installed), [visible]);

  // 一台还没装过任何 AI 的机器：默认停在「可安装」，别让小白对着一个空首页发呆。
  useEffect(() => {
    if (autoSwitched.current || tools.length === 0) return;
    autoSwitched.current = true;
    if (installed.length === 0) setView("installable");
  }, [tools.length, installed.length]);

  const recommended = useMemo(() => {
    const byId = new Map(visible.map((x) => [x.id, x] as const));
    const picks = RECOMMENDED_TOOLS.map((id) => byId.get(id)).filter(
      (x): x is ToolInfo => !!x && !x.installed,
    );
    for (const candidate of installable) {
      if (picks.length >= 3) break;
      if (!picks.some((p) => p.id === candidate.id)) picks.push(candidate);
    }
    return picks.slice(0, 3);
  }, [visible, installable]);

  const installList = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = installable.filter((x) => {
      if (category !== "all" && categoryOf(x) !== category) return false;
      if (!q) return true;
      return (
        x.name.toLowerCase().includes(q) ||
        x.id.toLowerCase().includes(q) ||
        x.summary.toLowerCase().includes(q)
      );
    });
    return sortForLibrary(filtered, RECOMMENDED_TOOLS);
  }, [installable, category, query]);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await onRefreshTools();
    } finally {
      setRefreshing(false);
    }
  }, [onRefreshTools]);

  const baseTokens = bill ? humanNum(bill.claude_base_tokens) : null;
  const memoryBytes = bill ? bill.memory_files.reduce((sum, f) => sum + f.bytes, 0) : 0;

  function showInstallable() {
    setView("installable");
    window.setTimeout(() => installRef.current?.scrollIntoView({ behavior: "smooth", block: "start" }), 60);
  }

  function openBudget() {
    setBudgetOpen(true);
    // 等展开渲染完再滚，不然量到的是收起高度。
    window.setTimeout(
      () => document.getElementById("uking-budget")?.scrollIntoView({ behavior: "smooth", block: "center" }),
      60,
    );
  }

  const expandedTool = expanded ? installed.find((x) => x.id === expanded) ?? null : null;

  return (
    <div className="space-y-5">
      {/* 终端快照重开条：自升级是硬退出，旧 PTY 活不过去；有快照才出现。 */}
      {termSnapshot && termSnapshot.sessions.length > 0 && (
        <section className="flex flex-wrap items-center gap-3 rounded-xl border border-accent/30 bg-accent/[0.07] px-4 py-3">
          <Activity size={17} className="shrink-0 text-accent" />
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium text-ink-0">
              {failedTermRestoreCount > 0
                ? t("{n} 条重开失败", { n: failedTermRestoreCount })
                : t("上次升级时有 {n} 个终端", { n: termSnapshot.sessions.length })}
            </div>
            <div className="mt-0.5 text-[11.5px] text-ink-3">
              {t("可以按原来的目录和命令把它们开回来（原来的屏幕内容不会回来）。")}
            </div>
          </div>
          <button
            onClick={onRestoreTermSnapshot}
            disabled={recoveringTermSnapshot}
            className="h-8 shrink-0 rounded-lg bg-accent px-3 text-[12px] font-semibold text-white hover:bg-accent-600 disabled:opacity-60"
          >
            {recoveringTermSnapshot ? t("正在重开…") : t("一键重开")}
          </button>
          <button
            onClick={onDismissTermSnapshot}
            className="h-8 shrink-0 rounded-lg px-2.5 text-[12px] text-ink-3 hover:bg-white/[0.06] hover:text-ink-1"
          >
            {t("不再提醒")}
          </button>
        </section>
      )}

      {/* 一句话状态 */}
      <header className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="text-[11.5px] text-ink-4">{t("我的 AI")}</div>
          <h1 className="mt-1 text-[24px] font-semibold leading-tight text-ink-0">
            {installed.length > 0 ? t("都在这儿，点一下就开始用。") : t("先把要用的 AI 装上。")}
          </h1>
          <p className="mt-1 text-[13px] text-ink-3">
            {t("已装 {n} 个 · 可装 {m} 个", { n: installed.length, m: installable.length })}
            {baseTokens ? ` · ${t("每轮底座约 {n} tokens", { n: baseTokens })}` : ""}
          </p>
        </div>
        <button
          onClick={() => void refresh()}
          disabled={refreshing}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg border border-white/[0.10] px-3 text-[12.5px] text-ink-2 hover:bg-white/[0.05] disabled:opacity-60"
        >
          <RefreshCw size={13} className={refreshing ? "animate-spin" : ""} />
          {t("刷新")}
        </button>
      </header>

      {/* 四个动作块 —— 页面上唯一有块面的地方 */}
      <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
        <button
          onClick={onGoDoctor}
          className="group flex items-center gap-3 rounded-xl bg-accent px-4 py-3.5 text-left text-white transition-colors hover:bg-accent-600"
        >
          <Activity size={18} className="shrink-0" />
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] font-semibold">{t("体检一下")}</span>
            <span className="block text-[11.5px] text-white opacity-80">{t("看看哪块要修、哪个能升级")}</span>
          </span>
          <ArrowUpRight size={15} className="shrink-0 text-white opacity-70" />
        </button>

        <button
          onClick={showInstallable}
          className="group flex items-center gap-3 rounded-xl border border-white/[0.10] bg-bg-1 px-4 py-3.5 text-left transition-colors hover:border-accent/40 hover:bg-bg-2"
        >
          <Package size={18} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] font-semibold text-ink-0">{t("装软件")}</span>
            <span className="block text-[11.5px] text-ink-3">{t("分类挑一个，装不上让 AI 装")}</span>
          </span>
          <ArrowUpRight size={15} className="shrink-0 text-ink-4" />
        </button>

        <button
          onClick={openBudget}
          className="group flex items-center gap-3 rounded-xl border border-white/[0.10] bg-bg-1 px-4 py-3.5 text-left transition-colors hover:border-accent/40 hover:bg-bg-2"
        >
          <Coins size={18} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] font-semibold text-ink-0">{t("省额度")}</span>
            <span className="block text-[11.5px] text-ink-3">
              {baseTokens ? t("每轮底座约 {n} tokens", { n: baseTokens }) : t("看看每轮都花在哪")}
            </span>
          </span>
          <ArrowUpRight size={15} className="shrink-0 text-ink-4" />
        </button>

        <button
          onClick={onGoManage}
          className="group flex items-center gap-3 rounded-xl border border-white/[0.10] bg-bg-1 px-4 py-3.5 text-left transition-colors hover:border-accent/40 hover:bg-bg-2"
        >
          <Settings2 size={18} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] font-semibold text-ink-0">{t("换模型 · 余额")}</span>
            <span className="block text-[11.5px] text-ink-3">{t("AI 设置里配")}</span>
          </span>
          <ArrowUpRight size={15} className="shrink-0 text-ink-4" />
        </button>
      </div>

      {/* 已装 / 可安装 切换 */}
      <section ref={installRef} className="scroll-mt-4">
        <div className="flex flex-wrap items-center gap-2">
          <div className="inline-flex rounded-lg border border-white/[0.10] bg-bg-1 p-0.5">
            <button
              onClick={() => setView("installed")}
              className={cn(
                "h-7 rounded-md px-3 text-[12.5px] transition-colors",
                view === "installed" ? "bg-accent text-white" : "text-ink-2 hover:text-ink-0",
              )}
            >
              {t("已装（{n}）", { n: installed.length })}
            </button>
            <button
              onClick={() => setView("installable")}
              className={cn(
                "h-7 rounded-md px-3 text-[12.5px] transition-colors",
                view === "installable" ? "bg-accent text-white" : "text-ink-2 hover:text-ink-0",
              )}
            >
              {t("可安装（{n}）", { n: installable.length })}
            </button>
          </div>
          {view === "installed" && installed.length > 0 && (
            <button onClick={onGoDoctor} className="ml-auto text-[12px] text-ink-3 hover:text-accent">
              {t("全部体检 / 升级 →")}
            </button>
          )}
        </div>

        {view === "installed" ? (
          installed.length > 0 ? (
            <>
              {/* 图标墙：一眼看到装了什么，点一下直接启动；管理收进右上角 ⋯ */}
              <div className="mt-2.5 grid grid-cols-2 gap-2.5 sm:grid-cols-3 xl:grid-cols-4">
                {installed.map((x) => (
                  <div
                    key={x.id}
                    className="group relative rounded-xl border border-white/[0.10] bg-bg-1 transition-colors hover:border-accent/40 hover:bg-bg-2"
                  >
                    <button
                      onClick={() => void onLaunch(x)}
                      className="flex w-full flex-col items-center gap-2 px-3 pb-3 pt-4 text-center"
                    >
                      <ToolIcon tool={x.id} size={40} active />
                      <span className="block w-full truncate text-[13px] font-medium text-ink-0">
                        {x.name}
                      </span>
                      <span className="block w-full truncate text-[11px] text-ink-4">
                        {[x.version ? `v${x.version}` : "", x.launch_app ? t("桌面版") : ""]
                          .filter(Boolean)
                          .join(" · ") || t("点一下启动")}
                      </span>
                    </button>
                    <button
                      aria-label={t("管理 {name}", { name: x.name })}
                      onClick={() => setExpanded(expanded === x.id ? null : x.id)}
                      className="absolute right-1.5 top-1.5 grid h-6 w-6 place-items-center rounded-md text-ink-4 opacity-100 hover:bg-white/[0.06] hover:text-ink-1 sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100"
                    >
                      <MoreHorizontal size={13} />
                    </button>
                  </div>
                ))}
              </div>

              {expandedTool && (
                <div className="mt-2 rounded-xl border border-accent/30 bg-bg-1 p-2.5">
                  <div className="flex items-center gap-2 px-1 pb-1.5">
                    <ToolIcon tool={expandedTool.id} size={20} active />
                    <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-ink-0">
                      {expandedTool.name}
                    </span>
                    <button
                      onClick={() => setExpanded(null)}
                      className="rounded-md px-2 py-0.5 text-[11.5px] text-ink-3 hover:bg-white/[0.05] hover:text-ink-1"
                    >
                      {t("收起")}
                    </button>
                  </div>
                  <div className="grid grid-cols-2 gap-1 sm:grid-cols-5">
                    <button
                      onClick={() => void onLaunch(expandedTool)}
                      className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
                    >
                      <Play size={14} className="text-ink-4" />
                      {expandedTool.launch_app ? t("打开") : t("启动")}
                    </button>
                    <button
                      onClick={() => onManageProviders(undefined, expandedTool.id)}
                      className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
                    >
                      <Settings2 size={14} className="text-ink-4" />
                      {t("换模型")}
                    </button>
                    <button
                      onClick={() => onInstallSelection([expandedTool.id])}
                      className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
                    >
                      <Download size={14} className="text-ink-4" />
                      {t("升级 / 修复")}
                    </button>
                    <button
                      onClick={() => onAskAiToFix(buildAiRepairPrompt(expandedTool))}
                      className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
                    >
                      <Wrench size={14} className="text-ink-4" />
                      {t("让 AI 修")}
                    </button>
                    {canUninstallTool(expandedTool.id) && (
                      <button
                        onClick={() => onUninstall(expandedTool)}
                        className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-danger-700 hover:bg-danger-500/[0.08] dark:text-danger-400"
                      >
                        <Trash2 size={14} />
                        {t("卸载")}
                      </button>
                    )}
                  </div>
                </div>
              )}
            </>
          ) : (
            <div className="mt-2.5 rounded-xl border border-accent/30 bg-accent/[0.06] p-4">
              <div className="text-[13.5px] font-semibold text-ink-0">{t("还没装 AI 工具")}</div>
              <p className="mt-1 text-[12px] text-ink-3">
                {t("去「可安装」挑一个：分类挑、点安装；装不上就点「让 AI 装」。")}
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                <button
                  onClick={showInstallable}
                  className="inline-flex h-9 items-center gap-1.5 rounded-lg bg-accent px-4 text-[13px] font-semibold text-white hover:bg-accent-600"
                >
                  <Package size={14} />
                  {t("去挑一个")}
                </button>
                <button
                  onClick={onGoSetup}
                  className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-white/[0.12] px-4 text-[13px] text-ink-2 hover:bg-white/[0.05]"
                >
                  <Sparkles size={14} />
                  {t("一键装好推荐组合")}
                </button>
              </div>
            </div>
          )
        ) : (
          <>
            {/* 可安装：搜索 + 分类，主动找东西时什么都不藏 */}
            <div className="mt-2.5 flex flex-col gap-2.5">
              <div className="flex h-9 items-center gap-2 rounded-lg border border-white/[0.10] bg-bg-1 px-3">
                <Search size={14} className="shrink-0 text-ink-4" />
                <input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={t("搜软件名或用途，比如 magpie / 模型路由")}
                  className="min-w-0 flex-1 bg-transparent text-[13px] text-ink-0 outline-none placeholder:text-ink-4"
                  spellCheck={false}
                />
                {query && (
                  <button
                    onClick={() => setQuery("")}
                    className="shrink-0 text-[11.5px] text-ink-3 hover:text-ink-1"
                  >
                    {t("清空")}
                  </button>
                )}
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                {TOOL_CATEGORIES.map((c) => (
                  <button
                    key={c.id}
                    onClick={() => setCategory(c.id)}
                    className={cn(
                      "h-7 rounded-full border px-3 text-[12px] transition-colors",
                      category === c.id
                        ? "border-accent/50 bg-accent/[0.10] text-accent"
                        : "border-white/[0.10] text-ink-3 hover:bg-white/[0.05] hover:text-ink-1",
                    )}
                  >
                    {t(c.label)}
                  </button>
                ))}
                {query.trim() && (
                  <span className="ml-1 text-[11.5px] text-ink-4">{t("找到 {n} 个", { n: installList.length })}</span>
                )}
              </div>
            </div>

            <div className="mt-1 border-t border-white/[0.08]">
              {installList.map((x) => {
                const canInstall = x.action === "install";
                const recommendedIds = new Set(recommended.map((r) => r.id));
                return (
                  <div key={x.id} className="flex items-center gap-3 border-b border-white/[0.08] px-1 py-3">
                    <ToolIcon tool={x.id} size={36} active={false} />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="truncate text-[13.5px] font-medium text-ink-0">{x.name}</span>
                        <span className="shrink-0 rounded-full border border-white/[0.10] px-1.5 text-[10.5px] leading-4 text-ink-3">
                          {t(categoryLabel(categoryOf(x)))}
                        </span>
                        {recommendedIds.has(x.id) && (
                          <span className="shrink-0 rounded-full bg-accent/[0.12] px-1.5 text-[10.5px] leading-4 text-accent">
                            {t("推荐")}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-ink-4">{x.summary}</div>
                    </div>
                    {canInstall ? (
                      <>
                        <button
                          onClick={() => onAskAiToFix(buildAiInstallPrompt(x))}
                          title={t("把安装任务交给轻助手，由 AI 来装")}
                          className="hidden h-8 shrink-0 items-center gap-1 rounded-lg px-2.5 text-[12px] text-ink-3 hover:bg-white/[0.05] hover:text-ink-1 sm:inline-flex"
                        >
                          <Sparkles size={12} />
                          {t("让 AI 装")}
                        </button>
                        <button
                          onClick={() => onInstallSelection([x.id])}
                          className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg bg-accent px-3 text-[12.5px] font-semibold text-white hover:bg-accent-600"
                        >
                          <Download size={12} />
                          {t("安装")}
                        </button>
                      </>
                    ) : (
                      <button
                        onClick={() => onAskAiToFix(buildAiInstallPrompt(x))}
                        className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg border border-white/[0.12] px-3 text-[12.5px] text-ink-2 hover:bg-white/[0.05]"
                      >
                        <Sparkles size={12} />
                        {t("让 AI 装")}
                      </button>
                    )}
                  </div>
                );
              })}
              {installList.length === 0 && (
                <div className="px-2 py-10 text-center text-[12.5px] text-ink-4">
                  {query.trim() || category !== "all"
                    ? t("这个条件下没有软件；换个词或点「全部」。")
                    : t("工具目录暂时是空的。")}
                </div>
              )}
            </div>

            <p className="mt-2 text-[11.5px] leading-relaxed text-ink-4">
              {t("装不上的可以先点「让 AI 装」：它会把这台机器的现状交给 U-King 轻助手，由 AI 接着装（不会自动发送，你按发送才走）。")}
            </p>
          </>
        )}
      </section>

      {/* 每轮底座账单（只读） */}
      <section id="uking-budget" className="scroll-mt-4 rounded-xl border border-white/[0.10] bg-bg-1">
        <button
          onClick={() => setBudgetOpen((v) => !v)}
          className="flex w-full items-center gap-2.5 px-4 py-3 text-left"
        >
          <Coins size={16} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1 text-[12.5px] text-ink-2">
            {bill
              ? t("每轮底座：Claude Code 本体约 {n} tokens；技能 {s} 个、记忆 {m}", {
                  n: humanNum(bill.claude_base_tokens),
                  s: bill.claude_skills.count + bill.uking_skills.count,
                  m: humanBytes(memoryBytes),
                })
              : t("每轮底座账单（读取中…）")}
          </span>
          <ChevronDown
            size={14}
            className={cn("shrink-0 text-ink-4 transition-transform", budgetOpen && "rotate-180")}
          />
        </button>
        {budgetOpen && bill && (
          <div className="border-t border-white/[0.08] px-4 py-3">
            <div className="space-y-1.5 text-[12.5px]">
              <div className="flex items-center justify-between gap-3">
                <span className="text-ink-2">{t("Claude Code 本体（系统提示 + 工具定义）")}</span>
                <span className="shrink-0 tabular-nums text-ink-0">
                  {t("约 {n} tokens", { n: humanNum(bill.claude_base_tokens) })}
                </span>
              </div>
              <div className="flex items-center justify-between gap-3">
                <span className="text-ink-2">{t("技能名字与简介（每轮都要读一遍）")}</span>
                <span className="shrink-0 tabular-nums text-ink-0">
                  {t("{n} 个 · 说明合计 {c}字", {
                    n: bill.claude_skills.count + bill.uking_skills.count,
                    c: humanChars(bill.claude_skills.chars + bill.uking_skills.chars),
                  })}
                </span>
              </div>
              {bill.memory_files.length > 0 && (
                <div className="flex items-start justify-between gap-3">
                  <span className="text-ink-2">{t("全局记忆文件（每轮都原样带上）")}</span>
                  <span className="shrink-0 text-right text-ink-0">
                    {bill.memory_files.map((f) => (
                      <span key={f.key} className="block tabular-nums">
                        {t(MEMORY_LABEL[f.key] ?? f.key)} · {humanBytes(f.bytes)}
                      </span>
                    ))}
                  </span>
                </div>
              )}
            </div>
            <div className="mt-3 rounded-lg bg-bg-3 px-3 py-2.5 text-[12px] leading-relaxed text-ink-2">
              <p>{t("这是「输入」里的固定开销：聊得越久，历史还会另外叠加（历史已压到 2.4 万字符）。")}</p>
              <p className="mt-1">
                {t("省法：不用的技能包卸掉；聊完一个话题就开新对话；全局记忆只留每次都必须在场的内容。")}
              </p>
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                onClick={onGoSkillPack}
                className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-white/[0.12] px-3 text-[12px] text-ink-2 hover:bg-white/[0.05]"
              >
                <Package size={13} />
                {t("去管技能包")}
              </button>
              <button
                onClick={() => setBudgetOpen(false)}
                className="inline-flex h-8 items-center rounded-lg px-3 text-[12px] text-ink-3 hover:bg-white/[0.05]"
              >
                {t("收起")}
              </button>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
