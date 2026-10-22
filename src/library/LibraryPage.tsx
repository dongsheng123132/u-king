/**
 * 软件库 —— 2026-10-22 用户口径：「软件要能加，能分类；比如要加 magpie，能加上，能装；
 * 不行就丢提示词进 U-King 轻助手，让 AI 装」。
 *
 * 定位：这一页只解决「装什么」；已装工具的管理（启动/换模型/升级修复/卸载）在「我的 AI」首页，
 * 不在这里重复一份。分类口径与首页共用 `lib/toolMeta`，不许各写一套。
 *
 * 安装走的是既有装机向导通路（App.tsx `onInstallSelection` → 向导页真正执行安装），本页
 * 不新开一条安装实现。每个可装工具都带「让 AI 装」兜底：把一次脱敏维护任务包填进轻助手
 * 的草稿框，由用户按发送（`buildAiInstallPrompt`，与装机失败交接同一份格式）。
 */
import { useMemo, useState } from "react";
import { Download, Package, Play, RefreshCw, Search, Sparkles } from "lucide-react";
import type { ToolInfo } from "../App";
import { RECOMMENDED_TOOLS } from "../App";
import { ToolIcon } from "../components/ToolIcon";
import { buildAiInstallPrompt } from "../lib/toolPrompt";
import {
  TOOL_CATEGORIES,
  categoryLabel,
  categoryOf,
  sortForLibrary,
  type ToolCategory,
} from "../lib/toolMeta";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

export function LibraryPage({
  tools,
  onLaunch,
  onOpen,
  onInstallSelection,
  onAskAiToFix,
  onRefreshTools,
}: {
  tools: ToolInfo[];
  onLaunch: (t: ToolInfo) => void | Promise<void>;
  onOpen: (t: ToolInfo) => void;
  onInstallSelection: (ids: string[]) => void;
  onAskAiToFix: (prompt: string) => void;
  onRefreshTools: () => Promise<ToolInfo[] | void> | void;
}) {
  const { t } = useI18n();
  const [category, setCategory] = useState<ToolCategory>("all");
  const [query, setQuery] = useState("");
  const [refreshing, setRefreshing] = useState(false);

  const visible = useMemo(() => tools.filter((x) => !x.hidden), [tools]);
  const installedCount = useMemo(() => visible.filter((x) => x.installed).length, [visible]);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = visible.filter((x) => {
      if (category !== "all" && categoryOf(x) !== category) return false;
      if (!q) return true;
      return (
        x.name.toLowerCase().includes(q) ||
        x.id.toLowerCase().includes(q) ||
        x.summary.toLowerCase().includes(q)
      );
    });
    return sortForLibrary(filtered, RECOMMENDED_TOOLS);
  }, [visible, category, query]);

  async function refresh() {
    setRefreshing(true);
    try {
      await onRefreshTools();
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <div className="space-y-4">
      <header className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h1 className="flex items-center gap-2 text-[22px] font-semibold leading-tight text-ink-0">
            <Package size={20} className="text-accent" />
            {t("软件库")}
          </h1>
          <p className="mt-1 text-[13px] text-ink-3">
            {t("共 {n} 个软件 · 已装 {m} 个", { n: visible.length, m: installedCount })}
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

      {/* 搜索 + 分类：用户主动找东西时，什么都不藏 */}
      <div className="flex flex-col gap-2.5">
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
            <span className="ml-1 text-[11.5px] text-ink-4">
              {t("找到 {n} 个", { n: list.length })}
            </span>
          )}
        </div>
      </div>

      {/* 软件清单 */}
      <div className="border-t border-white/[0.08]">
        {list.map((x) => {
          const cat = categoryLabel(categoryOf(x));
          const canInstall = x.action === "install";
          return (
            <div key={x.id} className="flex items-center gap-3 border-b border-white/[0.08] px-1 py-3">
              <ToolIcon tool={x.id} size={36} active={x.installed} />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="truncate text-[13.5px] font-medium text-ink-0">{x.name}</span>
                  <span className="shrink-0 rounded-full border border-white/[0.10] px-1.5 text-[10.5px] leading-4 text-ink-3">
                    {t(cat)}
                  </span>
                  {x.installed && (
                    <span className="shrink-0 rounded-full bg-success-500/15 px-1.5 text-[10.5px] leading-4 text-success-600">
                      {t("已装")}
                    </span>
                  )}
                </div>
                <div className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-ink-4">
                  {x.summary}
                </div>
              </div>

              {x.installed ? (
                <button
                  onClick={() => void onLaunch(x)}
                  className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg border border-white/[0.12] px-3 text-[12.5px] text-ink-2 hover:border-accent/40 hover:text-accent"
                >
                  <Play size={12} />
                  {x.launch_app ? t("打开") : t("启动")}
                </button>
              ) : canInstall ? (
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
                  onClick={() => onOpen(x)}
                  className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg border border-white/[0.12] px-3 text-[12.5px] text-ink-2 hover:bg-white/[0.05]"
                >
                  {t("打开官网")}
                </button>
              )}
            </div>
          );
        })}

        {list.length === 0 && (
          <div className="px-2 py-10 text-center text-[12.5px] text-ink-4">
            {query.trim() || category !== "all"
              ? t("这个条件下没有软件；换个词或点「全部」。")
              : t("工具目录暂时是空的。")}
          </div>
        )}
      </div>

      <p className="text-[11.5px] leading-relaxed text-ink-4">
        {t("装不上的可以先点「让 AI 装」：它会把这台机器的现状交给 U-King 轻助手，由 AI 接着装（不会自动发送，你按发送才走）。")}
      </p>
    </div>
  );
}
