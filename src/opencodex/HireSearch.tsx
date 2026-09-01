/**
 * 去市场现搜可招的人 —— **动态，不是我们维护的货架**。
 *
 * 用户 2026-08-18：「找专家安专家，默认就是去 github 和 skillhub 找相关最好的，
 * 这是动态词也是最好的，动态让大家装。」
 *
 * 后端 `runtime.hire.search`（`hire.rs`）早就写好了，**前端一次都没用过** ——
 * 它只读、只搜不装，每条结果带 `how_to_hire`（这东西到底怎么招进来）。
 * 那个模块开头解释了为什么不自建市场：生态里已经有 6 家在做，而且
 * 「让用户的 AI 自己去 GitHub / npm / SkillHub 上搜」跟自己开货架是冲突的 ——
 * **技能市场是一张会漂的地图，现搜才是看地形。**
 *
 * 🔴 只搜不装（跟后端同一条边界）：这里给的是名字 + 怎么装 + 原始链接，
 * **不替用户跑任何安装命令**。`expert.rs` 拒绝 `init` 字段是同一个理由 ——
 * 给外部内容一个「我来跑任意命令」的口子，等于把提示词注入面升级成任意代码执行。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, ExternalLink, Loader2, Search } from "lucide-react";
import { ACTION, createTauriActionClient } from "../generated/action-client";
import { useI18n } from "../i18n";
import { copyToClipboard } from "../lib/clipboard";

const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

type Hit = {
  name: string;
  version?: string;
  description?: string;
  how_to_hire?: string;
  url?: string;
  source?: string;
  shape?: string;
  weekly_downloads?: number;
};
type SourceStat = { source: string; reachable: boolean; hits: number; error?: string | null };

/** 默认预设起手词 —— 标签对人话、query 给后端。
 *  实测 npm 全文检索对中文/多词几乎全是噪声，只有 `keywords:dsh-plugin`
 *  能稳定返回真 DSH 插件（25 条全相关、按周下载量排）。其他几条是给人往下改的起点。 */
const DEFAULT_PRESETS: { label: string; query: string }[] = [
  { label: "最热 DSH 插件", query: "keywords:dsh-plugin" },
  { label: "飞书/办公", query: "dsh 飞书" },
  { label: "微信/公众号", query: "dsh 微信" },
  { label: "官方底层库", query: "@deepseek-ai" },
  { label: "CAD", query: "cad" },
];

export function HireSearch({ onToast, defaultQuery, presets }: {
  onToast?: (s: string) => void;
  /** 打开页面即自动跑的一条搜索 —— 修复「打开页没内容」。给了才跑，只跑一次。 */
  defaultQuery?: string;
  /** 预设起手词（带人话标签）。不传用默认。 */
  presets?: { label: string; query: string }[];
}) {
  const { t } = useI18n();
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [sources, setSources] = useState<SourceStat[]>([]);
  const seq = useRef(0);

  const run = useCallback(async (query: string) => {
    const kw = query.trim();
    if (!kw) {
      setHits(null);
      return;
    }
    const my = ++seq.current;
    setBusy(true);
    try {
      const env = await callAction(ACTION.RUNTIME_HIRE_SEARCH, { query: kw });
      // 只认最后一次请求的结果 —— 打字快时早发的请求可能后到，否则列表会跳回旧词的结果
      if (my !== seq.current) return;
      const r = (env.ok ? env.result : null) as { hits?: Hit[]; sources?: SourceStat[] } | null;
      setHits(r?.hits ?? []);
      setSources(r?.sources ?? []);
    } catch (e) {
      if (my === seq.current) {
        setHits([]);
        onToast?.(String(e));
      }
    } finally {
      if (my === seq.current) setBusy(false);
    }
  }, [onToast]);

  // 输入防抖：registry 搜索是真网络请求，每敲一个字就发一次既慢又没必要
  useEffect(() => {
    const id = setTimeout(() => void run(q), 450);
    return () => clearTimeout(id);
  }, [q, run]);

  // 打开即出一屏内容：给了 defaultQuery 就在挂载时自动跑一次（不然这页默认只有一个空搜索框，
  // 客户看到的永远是「没内容」）。只跑一次，之后用户自己搜，别每次重置回预设词。
  const autoRan = useRef(false);
  useEffect(() => {
    if (!defaultQuery || autoRan.current) return;
    autoRan.current = true;
    setQ(defaultQuery);
    void run(defaultQuery);
  }, [defaultQuery, run]);

  /** 「搜不到」和「没搜成」是两件事 —— 缺席不会自己发声，得替它说。 */
  const offline = sources.length > 0 && sources.every((s) => !s.reachable);

  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-4 py-3.5 space-y-3">
      <div>
        <h3 className="text-[13px] font-semibold text-ink-0">{t("去市场找人")}</h3>
        <p className="text-[11.5px] text-ink-4 mt-0.5">
          {t("现搜 npm / DSH 插件 / 技能包 —— 我们不自建货架，直接看生态里现在有什么。只搜不装。")}
        </p>
      </div>

      <div className="relative">
        <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-ink-5 pointer-events-none" />
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={t("搜「飞书」「公众号」「cad」，或 keywords:dsh-plugin")}
          className="w-full h-9 pl-9 pr-9 rounded-card bg-bg-1 border border-white/[0.08] text-[12.5px] text-ink-1 placeholder:text-ink-5 outline-none focus:border-accent/40"
        />
        {busy && <Loader2 size={14} className="absolute right-3 top-1/2 -translate-y-1/2 text-accent animate-spin" />}
      </div>

      {/* 预设起手词常驻（不只空态出现）：它们就是「动态提示词」—— 点一下跑一条搜索，
          让页面永远有东西可点、可看，而不是一个要自己想关键词的空框。 */}
      <div className="flex items-center gap-1.5 flex-wrap">
        {(presets ?? DEFAULT_PRESETS).map((p) => (
          <button
            key={p.query}
            onClick={() => {
              setQ(p.query);
              void run(p.query);
            }}
            title={p.query}
            className="h-6 px-2 rounded-full bg-bg-1 border border-white/[0.08] text-[11px] text-ink-3 hover:text-ink-1 hover:border-accent/30"
          >
            {t(p.label)}
          </button>
        ))}
      </div>

      {hits && (
        <div className="space-y-1">
          {offline ? (
            // 网络没通时**别说「没找到」** —— 那是把「没问到」说成了「不存在」
            <div className="text-[12px] text-warning-600 dark:text-warning-400 py-2">
              {t("没连上市场（网络或代理）—— 这不代表没有，只代表这次没问到。")}
            </div>
          ) : hits.length === 0 ? (
            <div className="text-[12px] text-ink-4 py-2">{t("没搜到匹配的。换个词试试，比如工具名或用途。")}</div>
          ) : (
            hits.slice(0, 12).map((h) => (
              <div key={h.name + (h.version ?? "")} className="flex items-start gap-2.5 py-2 border-b border-white/[0.04] last:border-0">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5 flex-wrap">
                    <span className="text-[12px] text-ink-1 font-mono truncate">{h.name}</span>
                    {h.version && <span className="text-[10px] text-ink-5">v{h.version}</span>}
                    {typeof h.weekly_downloads === "number" && (
                      // 周下载量 = 这东西有没有人在用。**唯一能一眼判断靠不靠谱的客观数**
                      <span className="text-[10px] px-1 rounded bg-white/[0.05] text-ink-4">
                        {t("周装 {n}", { n: h.weekly_downloads.toLocaleString() })}
                      </span>
                    )}
                  </div>
                  {h.description && <div className="text-[11px] text-ink-3 mt-0.5 line-clamp-2">{h.description}</div>}
                  {h.how_to_hire && (
                    <div className="text-[10.5px] text-ink-4 mt-1 leading-relaxed">
                      <span className="text-accent/80">{t("怎么招：")}</span>
                      {h.how_to_hire}
                    </div>
                  )}
                </div>
                <div className="flex flex-col gap-1 shrink-0">
                  {/* 🔴 给命令，**不替他跑**（同 hire.rs 的「只搜不装」边界）。
                      复制到终端由人按回车 —— 装外部包是不可撤回的动作，该由人签字。 */}
                  <button
                    onClick={() =>
                      void copyToClipboard(`npm i -g ${h.name}`).then((ok) =>
                        onToast?.(ok ? t("已复制安装命令，去终端粘贴执行") : t("复制失败，请手动选中复制")),
                      )
                    }
                    className="inline-flex items-center gap-1 h-6 px-2 rounded-md bg-accent/15 border border-accent/30 text-[10.5px] text-accent hover:bg-accent/25"
                  >
                    <Download size={11} /> {t("复制装法")}
                  </button>
                  {h.url && (
                    <button
                      onClick={() => void openUrl(h.url!).catch(() => {})}
                      className="inline-flex items-center gap-1 h-6 px-2 rounded-md border border-white/[0.10] text-[10.5px] text-ink-3 hover:text-ink-0"
                    >
                      <ExternalLink size={11} /> {t("看详情")}
                    </button>
                  )}
                </div>
              </div>
            ))
          )}
          {hits.length > 12 && (
            <div className="text-[11px] text-ink-5 pt-1">{t("还有 {n} 条没显示 —— 把关键词写具体一点", { n: hits.length - 12 })}</div>
          )}
        </div>
      )}
    </section>
  );
}
