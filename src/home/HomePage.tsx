/**
 * 「我的 AI」新首页 —— 2026-10-22 用户拍板走「B 极简」样张：
 *   一句话状态 → 四个动作块 → 已装列表（点开就地展开操作）→ 推荐装 → 每轮底座账单。
 *
 * 设计口径（为什么长这样）：
 *  - 360 管家的**事**（装 / 升 / 修 / 省），老外软件的**脸**（浅色、单蓝、少盒子、留白）。
 *  - 页面上只有「动作块」有块面，列表全用 1px 细线分隔——盒子越少，越不像后台。
 *  - 已装工具的操作（启动 / 换模型 / 升级修复 / 让 AI 修 / 卸载）全部**点开才出现**，
 *    不在第一屏堆按钮。原来 TileGrid + 详情条那套（旧 ToolHub）已下线。
 *
 * 数据来源：全部沿用 App.tsx 现有的回调与 `list_tools` 结果，本组件不新开业务通路；
 * 「每轮底座账单」只读后端 `context_bill`（不估账、不谎报：本体给的是实测常量，
 * 技能/记忆给的是这台机器上的真实文件统计）。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  ArrowUpRight,
  ChevronDown,
  Coins,
  Download,
  Package,
  Play,
  RefreshCw,
  Settings2,
  Sparkles,
  Trash2,
  Wrench,
} from "lucide-react";
import type { TermSnapshotInfo, ToolInfo } from "../App";
import { RECOMMENDED_TOOLS, canUninstallTool } from "../App";
import { ToolIcon } from "../components/ToolIcon";
import { buildAiInstallPrompt, buildAiRepairPrompt } from "../lib/toolPrompt";
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

/** 大数字说人话：1.2 万 / 8,400。只用于展示，不参与任何判断。 */
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

export function HomePage({
  tools,
  onLaunch,
  onInstallSelection,
  onUninstall,
  onManageProviders,
  onGoDoctor,
  onGoLibrary,
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
  onGoLibrary: () => void;
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
  const [bill, setBill] = useState<ContextBill | null>(null);
  const [billOpen, setBillOpen] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const billRef = useRef<HTMLElement | null>(null);

  // 只读账单：打不开就整块不显示，不弹错、不挡首屏（目录不存在 / 后端没就绪都算正常）。
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

  // 推荐装：优先 App.tsx 那份推荐名单（已装的不再推），不够就按工具目录顺序补足 3 个。
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

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await onRefreshTools();
    } finally {
      setRefreshing(false);
    }
  }, [onRefreshTools]);

  const baseTokens = bill ? humanNum(bill.claude_base_tokens) : null;
  const skillCount = bill ? bill.claude_skills.count : null;
  const memoryBytes = bill
    ? bill.memory_files.reduce((sum, f) => sum + f.bytes, 0)
    : 0;

  function openBill() {
    setBillOpen(true);
    // 等展开渲染完再滚，不然量到的是收起高度。
    window.setTimeout(() => billRef.current?.scrollIntoView({ behavior: "smooth", block: "center" }), 60);
  }

  function detailsFor(x: ToolInfo) {
    return (
      <div className="grid gap-1.5 px-3 pb-3 pt-1 sm:grid-cols-2">
        <button
          onClick={() => onManageProviders(undefined, x.id)}
          className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
        >
          <Settings2 size={14} className="text-ink-4" />
          {t("换模型")}
        </button>
        <button
          onClick={() => onInstallSelection([x.id])}
          className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
        >
          <Download size={14} className="text-ink-4" />
          {t("升级 / 修复（重走安装）")}
        </button>
        <button
          onClick={() => onAskAiToFix(buildAiRepairPrompt(x))}
          className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
        >
          <Wrench size={14} className="text-ink-4" />
          {t("让 AI 修")}
        </button>
        {canUninstallTool(x.id) && (
          <button
            onClick={() => onUninstall(x)}
            className="inline-flex items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] text-danger-700 hover:bg-danger-500/[0.08] dark:text-danger-400"
          >
            <Trash2 size={14} />
            {t("卸载")}
          </button>
        )}
      </div>
    );
  }

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
            {installed.length > 0 ? t("都在这儿，点开就能用。") : t("先把要用的 AI 装上。")}
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
          onClick={onGoLibrary}
          className="group flex items-center gap-3 rounded-xl border border-white/[0.10] bg-bg-1 px-4 py-3.5 text-left transition-colors hover:border-accent/40 hover:bg-bg-2"
        >
          <Package size={18} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] font-semibold text-ink-0">{t("软件库")}</span>
            <span className="block text-[11.5px] text-ink-3">{t("再装一个，分类找")}</span>
          </span>
          <ArrowUpRight size={15} className="shrink-0 text-ink-4" />
        </button>

        <button
          onClick={openBill}
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

      {/* 还没有任何工具：给一条最短的路 */}
      {installed.length === 0 && (
        <section className="rounded-xl border border-accent/30 bg-accent/[0.06] p-4">
          <div className="text-[13.5px] font-semibold text-ink-0">{t("还没装 AI 工具")}</div>
          <p className="mt-1 text-[12px] text-ink-3">
            {t("去软件库挑一个装上，或者让 AI 帮你按这台机器的配置推荐。")}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              onClick={onGoLibrary}
              className="inline-flex h-9 items-center gap-1.5 rounded-lg bg-accent px-4 text-[13px] font-semibold text-white hover:bg-accent-600"
            >
              <Package size={14} />
              {t("去软件库挑")}
            </button>
            <button
              onClick={onGoSetup}
              className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-white/[0.12] px-4 text-[13px] text-ink-2 hover:bg-white/[0.05]"
            >
              <Sparkles size={14} />
              {t("一键装好推荐组合")}
            </button>
          </div>
        </section>
      )}

      {/* 已装：细线列表，点一下展开操作 */}
      {installed.length > 0 && (
        <section>
          <div className="flex items-baseline justify-between gap-3">
            <h2 className="text-[13px] font-semibold text-ink-2">{t("已装（{n}）", { n: installed.length })}</h2>
            <button
              onClick={onGoDoctor}
              className="text-[12px] text-ink-3 hover:text-accent"
            >
              {t("全部体检 / 升级 →")}
            </button>
          </div>
          <div className="mt-1 border-t border-white/[0.08]">
            {installed.map((x) => {
              const open = expanded === x.id;
              const hasTerminal = !!x.launch_cmd && !x.launch_app;
              return (
                <div key={x.id} className="border-b border-white/[0.08]">
                  <div className="flex items-center gap-3 px-1 py-2.5">
                    <ToolIcon tool={x.id} size={32} active />
                    <button
                      onClick={() => setExpanded(open ? null : x.id)}
                      className="min-w-0 flex-1 text-left"
                    >
                      <span className="flex items-center gap-2">
                        <span className="truncate text-[13.5px] font-medium text-ink-0">{x.name}</span>
                        <span className="inline-flex h-4 w-4 items-center justify-center">
                          <ChevronDown
                            size={13}
                            className={cn("text-ink-4 transition-transform", open && "rotate-180")}
                          />
                        </span>
                      </span>
                      <span className="mt-0.5 block truncate text-[11.5px] text-ink-4">
                        {[x.version ? `v${x.version}` : "", x.launch_app ? t("桌面版") : hasTerminal ? t("命令行") : ""]
                          .filter(Boolean)
                          .join(" · ") || x.summary}
                      </span>
                    </button>
                    <span className="hidden shrink-0 items-center gap-1.5 text-[11.5px] text-ink-3 sm:inline-flex">
                      <span className="h-1.5 w-1.5 rounded-full bg-success-500" />
                      {t("正常")}
                    </span>
                    <button
                      onClick={() => void onLaunch(x)}
                      className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg border border-white/[0.12] px-3 text-[12.5px] text-ink-2 hover:border-accent/40 hover:text-accent"
                    >
                      <Play size={12} />
                      {x.launch_app ? t("打开") : t("启动")}
                    </button>
                  </div>
                  {open && detailsFor(x)}
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* 推荐装 */}
      {recommended.length > 0 && (
        <section>
          <div className="flex items-baseline justify-between gap-3">
            <h2 className="text-[13px] font-semibold text-ink-2">{t("推荐装")}</h2>
            <button onClick={onGoLibrary} className="text-[12px] text-ink-3 hover:text-accent">
              {t("全部软件 →")}
            </button>
          </div>
          <div className="mt-1 border-t border-white/[0.08]">
            {recommended.map((x) => (
              <div key={x.id} className="flex items-center gap-3 border-b border-white/[0.08] px-1 py-2.5">
                <ToolIcon tool={x.id} size={32} active={false} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13.5px] font-medium text-ink-0">{x.name}</div>
                  <div className="mt-0.5 truncate text-[11.5px] text-ink-4">{x.summary}</div>
                </div>
                <button
                  onClick={() => onAskAiToFix(buildAiInstallPrompt(x))}
                  className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg px-2.5 text-[12px] text-ink-3 hover:bg-white/[0.05] hover:text-ink-1"
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
              </div>
            ))}
          </div>
        </section>
      )}

      {/* 每轮底座账单（只读） */}
      <section ref={billRef} className="rounded-xl border border-white/[0.10] bg-bg-1">
        <button
          onClick={() => setBillOpen((v) => !v)}
          className="flex w-full items-center gap-2.5 px-4 py-3 text-left"
        >
          <Coins size={16} className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1 text-[12.5px] text-ink-2">
            {bill
              ? t("每轮底座：Claude Code 本体约 {n} tokens；技能 {s} 个、记忆 {m}", {
                  n: humanNum(bill.claude_base_tokens),
                  s: skillCount ?? 0,
                  m: humanBytes(memoryBytes),
                })
              : t("每轮底座账单（读取中…）")}
          </span>
          <ChevronDown size={14} className={cn("shrink-0 text-ink-4 transition-transform", billOpen && "rotate-180")} />
        </button>
        {billOpen && bill && (
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
                    n: bill.claude_skills.count,
                    c: humanChars(bill.claude_skills.chars),
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
              <p>{t("这是「输入」里的固定开销：聊得越久，历史还会另外叠加。")}</p>
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
                onClick={() => setBillOpen(false)}
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
