/**
 * 本地大模型 · 右侧模型面板：**本地** 有什么 / **商店** 能下什么。
 *
 * ## 为什么必须有这一面板
 *
 * 在它之前，这一页对一个普通客户是条死路：界面说「找不到 GGUF 模型文件，把 .gguf 放进
 * 某某目录」—— 可他既不知道 GGUF 是什么，也不知道去哪弄。引擎装得再顺，最后一步是
 * 「请自行获取模型」，等于没做。所以货架不是抄 EchoBird 的样子，是补这条链的断点。
 *
 * ## 两条数据的来源不一样，这是故意的
 *
 * - **货架**（有哪些模型值得下）：我们自己的清单，线上热下发 + 内嵌兜底。半年变一次。
 * - **量化清单 / 体积**（这个模型有哪些档、每档多大）：**现问模型站**，一个字都不缓存。
 *   上游随时重传、补档；写死在我们这儿的数字隔天就是错的，而错的方向是
 *   「界面写 5.7 GB、实际下 6.9 GB」，客户的 C 盘为此爆掉。
 *
 * 所以展开一张卡片会有一次网络请求 —— 慢一点，但说的是真话。
 *
 * ## 组件边界
 *
 * 只靠 props 通信（模块独立四铁律第 4 条）：父级给「本机有多少内存」和「现在选的是谁」，
 * 本组件回吐「用户选了这个」和「盘上的东西变了，你重扫一遍」。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  FolderOpen,
  HardDrive,
  Loader2,
  RefreshCw,
  Store,
  X,
} from "lucide-react";
import { useI18n } from "./i18n";
import { cn } from "./lib/cn";

export type LocalModel = { path: string; name: string; size_bytes: number; engine: string };

type CatalogModel = {
  id: string;
  label: string;
  publisher: string;
  params: string;
  tier: string;
  approx_gb: number;
  min_ram_gb: number;
  tags: string[];
  blurb: string;
  repo: string;
  engines: string[];
  downloaded: string[];
};

type QuantOption = { quant: string; files: string[]; size_bytes: number; local: boolean };

function gb(bytes: number): string {
  return `${(bytes / 1e9).toFixed(bytes < 1e10 ? 2 : 1)} GB`;
}

export function LocalLLMStore({
  localModels,
  downloadDir,
  ramGb,
  current,
  onPick,
  onChanged,
  onToast,
}: {
  localModels: LocalModel[];
  downloadDir: string;
  /** 本机物理内存（GB）。0 = 没探到，那就一条「跑不跑得动」的判断都别下 */
  ramGb: number;
  current: string;
  onPick: (path: string) => void;
  onChanged: () => void;
  onToast?: (msg: string) => void;
}) {
  const { t } = useI18n();
  const [tab, setTab] = useState<"local" | "store">("local");
  const [models, setModels] = useState<CatalogModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");
  const [open, setOpen] = useState<string>("");
  const [quants, setQuants] = useState<Record<string, QuantOption[] | { error: string }>>({});
  const [busyQuant, setBusyQuant] = useState("");
  /** 下载中的那一条：`<模型id>:<量化>`，配合下面的进度文本 */
  const [downloading, setDownloading] = useState("");
  const [progress, setProgress] = useState("");
  const seenStore = useRef(false);

  const loadCatalog = useCallback(
    async (refresh: boolean) => {
      setLoading(true);
      setErr("");
      try {
        const v = await invoke<{ models?: CatalogModel[]; error?: string }>("localllm_catalog", { refresh });
        if (v?.error) setErr(v.error);
        setModels(v?.models || []);
      } catch (e) {
        setErr(String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  // 货架只在第一次切到「商店」时拉 —— 大多数人开这一页是来启动已有模型的，
  // 不该为此先等一次网络。
  useEffect(() => {
    if (tab === "store" && !seenStore.current) {
      seenStore.current = true;
      void loadCatalog(false);
    }
  }, [tab, loadCatalog]);

  useEffect(() => {
    const un = listen<string>("uking:localllm_progress", (e) => setProgress(String(e.payload || "")));
    return () => {
      void un.then((f) => f());
    };
  }, []);

  const toggle = useCallback(
    async (id: string) => {
      if (open === id) {
        setOpen("");
        return;
      }
      setOpen(id);
      if (quants[id]) return;
      setBusyQuant(id);
      try {
        const v = await invoke<{ quants?: QuantOption[] }>("localllm_catalog", { modelId: id });
        setQuants((q) => ({ ...q, [id]: v?.quants || [] }));
      } catch (e) {
        setQuants((q) => ({ ...q, [id]: { error: String(e) } }));
      } finally {
        setBusyQuant("");
      }
    },
    [open, quants],
  );

  const download = useCallback(
    async (m: CatalogModel, q: QuantOption) => {
      const key = `${m.id}:${q.quant}`;
      setDownloading(key);
      setProgress(t("正在准备…"));
      try {
        await invoke("localllm_download", { modelId: m.id, quant: q.quant });
        onToast?.(t("{label} 已下载完成", { label: `${m.label} ${q.quant}` }));
        // 下完了盘上多了东西：让父级重扫，并把这个量化标记成本地已有
        setQuants((s) => {
          const cur = s[m.id];
          if (!Array.isArray(cur)) return s;
          return { ...s, [m.id]: cur.map((x) => (x.quant === q.quant ? { ...x, local: true } : x)) };
        });
        onChanged();
      } catch (e) {
        onToast?.(String(e));
      } finally {
        setDownloading("");
        setProgress("");
      }
    },
    [onChanged, onToast, t],
  );

  const pickDir = useCallback(async () => {
    const dir = await openDialog({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    try {
      await invoke("localllm_set_download_dir", { dir });
      onToast?.(t("下载位置已改到 {dir}", { dir }));
      onChanged();
    } catch (e) {
      onToast?.(String(e));
    }
  }, [onChanged, onToast, t]);

  /** 这台机器跑不跑得动。内存探不到就**什么都不说** —— 猜一个绿勾比不说更坏。 */
  const fitOf = (m: CatalogModel): { tone: string; text: string } | null => {
    if (!ramGb || !m.min_ram_gb) return null;
    if (ramGb >= m.min_ram_gb) return { tone: "text-success-400", text: t("这台跑得动") };
    if (ramGb >= m.min_ram_gb * 0.75) return { tone: "text-warn-500", text: t("勉强，会很慢") };
    return { tone: "text-ink-4", text: t("内存不够（要 {n} GB）", { n: m.min_ram_gb }) };
  };

  return (
    <div className="rounded-card border border-white/[0.06] bg-bg-2/70 flex flex-col max-h-[calc(100vh-140px)]">
      {/* 页签 */}
      <div className="flex items-center gap-1 px-3 pt-3">
        {(["local", "store"] as const).map((k) => (
          <button
            key={k}
            onClick={() => setTab(k)}
            className={cn(
              "rounded-card px-3 py-1.5 text-[12.5px]",
              tab === k ? "bg-accent/15 text-accent" : "text-ink-3 hover:bg-white/[0.05]",
            )}
          >
            {k === "local" ? `${t("本地")}（${localModels.length}）` : t("商店")}
          </button>
        ))}
        {tab === "store" && (
          <button
            onClick={() => void loadCatalog(true)}
            title={t("刷新货架")}
            className="ml-auto rounded-card p-1.5 text-ink-4 hover:bg-white/[0.05] hover:text-ink-2"
          >
            <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          </button>
        )}
      </div>

      {/* 下载位置 —— 模型动辄十几 GB，C 盘装不下是常态，所以这行摆在最显眼处 */}
      <div className="mx-3 mt-2.5 flex items-center gap-1.5 rounded-card border border-white/[0.06] px-2.5 py-2 text-[11.5px] text-ink-3">
        <HardDrive size={12} className="shrink-0 text-ink-4" />
        <span className="shrink-0 text-ink-4">{t("下载位置")}</span>
        <span className="truncate font-mono" title={downloadDir}>
          {downloadDir}
        </span>
        <button
          onClick={() => void openPath(downloadDir).catch((e) => onToast?.(String(e)))}
          title={t("打开这个文件夹")}
          className="ml-auto shrink-0 rounded p-1 text-ink-4 hover:bg-white/[0.05] hover:text-ink-2"
        >
          <FolderOpen size={12} />
        </button>
        <button onClick={() => void pickDir()} className="shrink-0 rounded px-1.5 py-0.5 text-accent hover:bg-accent/10">
          {t("改")}
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3 space-y-2">
        {tab === "local" ? (
          localModels.length === 0 ? (
            <p className="px-1 py-6 text-center text-[12px] leading-relaxed text-ink-4">
              {t("这台机器上还没有模型。去「商店」下一个，或把已有的 .gguf 放进上面那个文件夹。")}
            </p>
          ) : (
            localModels.map((m) => (
              <button
                key={m.path}
                onClick={() => onPick(m.path)}
                className={cn(
                  "w-full rounded-card border px-3 py-2.5 text-left",
                  current === m.path
                    ? "border-accent/40 bg-accent/[0.08]"
                    : "border-white/[0.06] hover:bg-white/[0.03]",
                )}
              >
                <div className="flex items-center gap-2">
                  <span className="truncate text-[12.5px] text-ink-1" title={m.name}>
                    {m.name}
                  </span>
                  {current === m.path && <Check size={12} className="shrink-0 text-accent" />}
                </div>
                <div className="mt-0.5 flex items-center gap-2 text-[11px] text-ink-4">
                  {m.size_bytes > 0 && <span>{gb(m.size_bytes)}</span>}
                  <span className="font-mono">{m.engine === "llamacpp" ? "llama.cpp" : "vLLM / SGLang"}</span>
                </div>
              </button>
            ))
          )
        ) : (
          <>
            {err && (
              <div className="flex items-start gap-1.5 rounded-card border border-warn-500/30 bg-warn-500/[0.08] px-2.5 py-2 text-[11.5px] leading-relaxed text-warn-500">
                <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                <span>{err}</span>
              </div>
            )}
            {loading && models.length === 0 && (
              <p className="px-1 py-6 text-center text-[12px] text-ink-4">{t("正在取货架…")}</p>
            )}
            {models.map((m) => {
              const fit = fitOf(m);
              const expanded = open === m.id;
              const qs = quants[m.id];
              return (
                <section
                  key={m.id}
                  className={cn(
                    "rounded-card border",
                    expanded ? "border-accent/25 bg-white/[0.02]" : "border-white/[0.06]",
                  )}
                >
                  <button
                    onClick={() => void toggle(m.id)}
                    className="flex w-full items-start gap-2 px-3 py-2.5 text-left hover:bg-white/[0.03]"
                  >
                    {expanded ? (
                      <ChevronDown size={13} className="mt-1 shrink-0 text-ink-4" />
                    ) : (
                      <ChevronRight size={13} className="mt-1 shrink-0 text-ink-4" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-[13px] font-semibold text-ink-0">{m.label}</span>
                        {m.downloaded.length > 0 && (
                          <span className="shrink-0 rounded-full bg-success-500/15 px-1.5 py-0.5 text-[10px] text-success-400">
                            {t("已下载")}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-ink-4">
                        <span>{m.publisher}</span>
                        <span className="font-mono">{m.params}</span>
                        <span>约 {m.approx_gb} GB</span>
                        {fit && <span className={fit.tone}>{fit.text}</span>}
                      </div>
                      <p className="mt-1 text-[11.5px] leading-relaxed text-ink-3">{m.blurb}</p>
                    </div>
                  </button>

                  {expanded && (
                    <div className="border-t border-white/[0.05] px-3 py-2 space-y-1.5">
                      {busyQuant === m.id && (
                        <p className="flex items-center gap-1.5 text-[11.5px] text-ink-4">
                          <Loader2 size={12} className="animate-spin" /> {t("正在问模型站有哪些量化…")}
                        </p>
                      )}
                      {qs && !Array.isArray(qs) && (
                        <p className="text-[11.5px] leading-relaxed text-warn-500">{qs.error}</p>
                      )}
                      {Array.isArray(qs) &&
                        qs.map((q) => {
                          const key = `${m.id}:${q.quant}`;
                          const busy = downloading === key;
                          return (
                            <div key={q.quant} className="rounded-lg bg-white/[0.02] px-2.5 py-1.5">
                              <div className="flex items-center gap-2">
                                <span className="font-mono text-[11.5px] text-ink-2">{q.quant}</span>
                                <span className="text-[11px] text-ink-4">{gb(q.size_bytes)}</span>
                                {q.files.length > 1 && (
                                  <span className="text-[10.5px] text-ink-5">
                                    {t("{n} 个分片", { n: q.files.length })}
                                  </span>
                                )}
                                {q.local ? (
                                  <span className="ml-auto inline-flex items-center gap-1 text-[11px] text-success-400">
                                    <Check size={11} /> {t("已在本地")}
                                  </span>
                                ) : busy ? (
                                  <button
                                    onClick={() =>
                                      void invoke<string>("localllm_download_cancel").then((s) => onToast?.(s))
                                    }
                                    className="ml-auto inline-flex items-center gap-1 rounded-card border border-white/[0.08] px-2 py-1 text-[11px] text-ink-2 hover:bg-white/[0.05]"
                                  >
                                    <X size={11} /> {t("取消")}
                                  </button>
                                ) : (
                                  <button
                                    disabled={!!downloading}
                                    data-action-id="runtime.localllm.download"
                                    onClick={() => void download(m, q)}
                                    className="ml-auto inline-flex items-center gap-1 rounded-card bg-accent/15 px-2 py-1 text-[11px] text-accent hover:bg-accent/25 disabled:opacity-40"
                                  >
                                    <Download size={11} /> {t("下载")}
                                  </button>
                                )}
                              </div>
                              {busy && (
                                <p className="mt-1 text-[11px] leading-relaxed text-ink-3">
                                  {progress || t("正在准备…")}
                                </p>
                              )}
                            </div>
                          );
                        })}
                      {Array.isArray(qs) && qs.length > 0 && (
                        <p className="pt-0.5 text-[10.5px] leading-relaxed text-ink-5">
                          {t("量化档越小越省内存、答得越糙。不知道选哪个就挑 Q4 那一档 —— 它是公认的平衡点。")}
                        </p>
                      )}
                    </div>
                  )}
                </section>
              );
            })}
            {!loading && !err && models.length === 0 && (
              <p className="px-1 py-6 text-center text-[12px] text-ink-4">{t("货架是空的")}</p>
            )}
          </>
        )}
      </div>

      <div className="flex items-center gap-1.5 border-t border-white/[0.05] px-3 py-2 text-[10.5px] leading-relaxed text-ink-5">
        <Store size={11} className="shrink-0" />
        <span>{t("模型来自魔搭（国内直连），下载走断点续传，断了再点一次接着下。")}</span>
      </div>
    </div>
  );
}
