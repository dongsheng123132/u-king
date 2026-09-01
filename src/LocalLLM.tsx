/**
 * 本地大模型 —— 在自己电脑上跑开源模型。**离线 · 免费 · 数据不出本机。**
 *
 * 2026-08-11「简化第三刀」把这一页删了（那时只有 Ollama 一个引擎）。现在按 EchoBird
 * 的形态恢复成**四引擎货架**：
 *
 *   Ollama · llama.cpp · vLLM · SGLang
 *
 * 为什么是货架不是一个：能跑什么由客户那台机器决定。8G 内存无独显的笔记本和一台
 * 带 4090 的台式机，正确答案不是同一个；我们替他选一个，就必然在另一半机器上失灵。
 * 所以四个并排摆着，每个都自己回答「你这台能不能用我」——
 * `blockers` 说人话地讲卡在哪，`unsupported_here` 直接置灰（vLLM/SGLang 只有 Linux+N 卡）。
 *
 * ## 2026-08-19 补齐的三件（客户拿 EchoBird 对比着提的，三条都成立）
 *
 * 1. **右边的模型面板**（`LocalLLMStore`）：在它之前这一页对普通客户是条死路 ——
 *    界面让他「把 .gguf 放进某某目录」，他既不知道 GGUF 是什么也不知道去哪弄。
 *    引擎装得再顺，最后一步写着「请自行获取模型」，等于没做。
 * 2. **运行参数**：上下文 / 算力 / 端口 / 线程。之前上下文写死在引擎默认（llama.cpp 是
 *    4096），客户拿它读一份长文档必被截断，而界面上**什么都不会说**。
 * 3. **标准输出**：模型加载要几十秒到几分钟，中间没有任何反馈 = 客户以为点了没反应。
 *    日志原来藏在一个「日志」按钮后面，现在是常驻的一块。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  CheckCircle2,
  Cpu,
  Download,
  FolderPlus,
  HardDrive,
  Loader2,
  Play,
  RefreshCw,
  Square,
  Terminal,
  Upload,
} from "lucide-react";
import { useI18n } from "./i18n";
import { cn } from "./lib/cn";
import { LocalLLMStore, type LocalModel } from "./LocalLLMStore";

type Engine = {
  id: string;
  label: string;
  blurb: string;
  installed: boolean;
  ready: boolean;
  blockers: string[];
  version: string | null;
  endpoint: string | null;
  running_pid: number | null;
  running_model: string | null;
  models: string[];
  unsupported_here: boolean;
};

type RunSettings = { port: number; ctx: number; gpu_layers: number; threads: number };

type Inspect = {
  engines?: Engine[];
  model_dirs?: string[];
  download_dir?: string;
  local_models?: LocalModel[];
  settings?: Record<string, RunSettings>;
  error?: string;
};

type HardwareInfo = {
  ram_total_mb: number;
  gpu_name: string | null;
  gpu_vram_mb: number | null;
  gpu_accelerated: boolean;
  recommend?: { model_label: string; size_gb: number; note: string; can_run_decent: boolean };
};

const CTX_CHOICES = [2048, 4096, 8192, 16384, 32768, 65536, 131072];

/** 路径只留最后两段 —— 模型路径动辄很长，全铺出来一行就没了。 */
function shortPath(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts.length <= 2 ? p : `…/${parts.slice(-2).join("/")}`;
}

export function LocalLLM({ onToast }: { onToast?: (msg: string) => void }) {
  const { t } = useI18n();
  const [data, setData] = useState<Inspect>({});
  const [hw, setHw] = useState<HardwareInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string>("");
  const [picked, setPicked] = useState<Record<string, string>>({});
  /** 当前操作的引擎（EchoBird 那个「运行时」下拉）。默认挑一个这台机器用得上的 */
  const [engineId, setEngineId] = useState<string>("");
  const [cfg, setCfg] = useState<RunSettings>({ port: 18820, ctx: 8192, gpu_layers: -1, threads: 0 });
  const [logText, setLogText] = useState<string>("");
  const logBox = useRef<HTMLPreElement | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const v = await invoke<Inspect>("localllm_inspect");
      setData(v || {});
    } catch (e) {
      setData({ error: String(e) });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    // 硬件体检是另一个动作（AI 优化大师也在用同一个）—— 拿它回答「这台能跑多大的」。
    invoke<HardwareInfo>("detect_hardware")
      .then(setHw)
      .catch(() => setHw(null));
  }, [refresh]);

  const engines = useMemo(() => data.engines || [], [data.engines]);

  // 默认引擎：优先「正在跑的」→「能用的」→「装了的」→ llama.cpp。
  // 别默认挑一个这台机器根本跑不了的（vLLM 在 Windows 上永远是灰的）。
  useEffect(() => {
    if (engineId || engines.length === 0) return;
    const pick =
      engines.find((e) => e.running_pid != null) ||
      engines.find((e) => e.ready) ||
      engines.find((e) => e.installed && !e.unsupported_here) ||
      engines.find((e) => e.id === "llamacpp");
    if (pick) setEngineId(pick.id);
  }, [engines, engineId]);

  useEffect(() => {
    if (engineId && data.settings?.[engineId]) setCfg(data.settings[engineId]);
  }, [engineId, data.settings]);

  const engine = engines.find((e) => e.id === engineId);
  const running = engine?.running_pid != null;
  const model = (engineId && picked[engineId]) || engine?.running_model || engine?.models[0] || "";

  // 标准输出：起服务那几十秒里，这是客户唯一能看见的动静。跑着就一直跟。
  useEffect(() => {
    if (!engineId) return;
    let alive = true;
    const tick = () => {
      invoke<string>("localllm_logs", { engine: engineId, lines: 200 })
        .then((s) => {
          if (alive) setLogText(s);
        })
        .catch(() => {});
    };
    tick();
    const id = window.setInterval(tick, running || busy ? 1500 : 6000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [engineId, running, busy]);

  useEffect(() => {
    const el = logBox.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [logText]);

  const run = useCallback(
    async (key: string, fn: () => Promise<unknown>, okMsg: string) => {
      setBusy(key);
      try {
        await fn();
        onToast?.(okMsg);
        await refresh();
      } catch (e) {
        onToast?.(String(e));
      } finally {
        setBusy("");
      }
    },
    [onToast, refresh],
  );

  /** 参数改一下存一下 —— 别等到点「启动」才落盘，客户调完直接关窗是常态。 */
  const saveCfg = useCallback(
    (next: RunSettings) => {
      setCfg(next);
      if (!engineId) return;
      invoke("localllm_save_settings", {
        engine: engineId,
        port: next.port,
        ctx: next.ctx,
        gpuLayers: next.gpu_layers,
        threads: next.threads,
      }).catch((e) => onToast?.(String(e)));
    },
    [engineId, onToast],
  );

  const ramGb = hw ? Math.round(hw.ram_total_mb / 1024) : 0;

  return (
    <div className="mx-auto flex max-w-[1400px] flex-col gap-4 px-6 py-5 xl:flex-row">
      {/* ───────── 左：运行时 + 参数 + 标准输出 + 引擎货架 ───────── */}
      <div className="min-w-0 flex-1 space-y-4">
        <header className="flex items-center gap-2">
          <Cpu size={18} className="text-accent" />
          <h2 className="text-[16px] font-semibold text-ink-0">{t("本地大模型")}</h2>
          <span className="text-[12px] text-ink-4">{t("离线 · 免费 · 数据不出本机")}</span>
          <button
            onClick={() => void refresh()}
            data-action-id="runtime.localllm.inspect"
            className="ml-auto inline-flex items-center gap-1.5 rounded-card border border-white/[0.08] px-2.5 py-1.5 text-[12px] text-ink-2 hover:bg-white/[0.05]"
          >
            <RefreshCw size={13} className={loading ? "animate-spin" : ""} /> {t("重新检测")}
          </button>
        </header>

        {/* 这台机器能跑多大的 —— 先管理预期，再让人点启动 */}
        {hw && (
          <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-3.5">
            <div className="flex flex-wrap items-center gap-x-5 gap-y-1.5 text-[12px] text-ink-2">
              <span className="inline-flex items-center gap-1.5">
                <HardDrive size={13} className="text-ink-4" />
                {t("内存")} {ramGb} GB
              </span>
              <span>{hw.gpu_name || t("没探到独立显卡")}</span>
              {hw.gpu_vram_mb ? <span className="text-ink-4">{t("显存")} {Math.round(hw.gpu_vram_mb / 1024)} GB</span> : null}
              {!hw.gpu_accelerated && (
                <span className="text-ink-4">{t("没有能加速推理的显卡，速度会明显慢")}</span>
              )}
            </div>
            {hw.recommend && (
              <p
                className={cn(
                  "mt-1.5 text-[12px] leading-relaxed",
                  hw.recommend.can_run_decent ? "text-ink-2" : "text-warn-500",
                )}
              >
                {t("这台机器合适的档位")}：{hw.recommend.model_label}（{hw.recommend.size_gb} GB）—— {hw.recommend.note}
              </p>
            )}
          </section>
        )}

        {data.error && (
          <div className="rounded-card border border-danger-500/30 bg-danger-500/[0.08] px-4 py-3 text-[12.5px] text-danger-400">
            {data.error}
          </div>
        )}

        {/* ── 控制台：选模型 + 参数 + 启停 ── */}
        <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
          <div className="flex flex-wrap items-center gap-2 text-[12px]">
            <span className="text-ink-4">{t("当前模型")}</span>
            <span
              className={cn("truncate font-mono text-[12px]", model ? "text-ink-1" : "text-ink-4")}
              title={model}
            >
              {model ? shortPath(model) : t("从右边选一个（本地没有就去「商店」下）")}
            </span>
          </div>

          <div className="flex flex-wrap items-end gap-3">
            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-ink-4">{t("运行时")}</span>
              <select
                value={engineId}
                onChange={(e) => setEngineId(e.target.value)}
                className="rounded-lg border border-white/[0.08] bg-white/[0.03] px-2.5 py-1.5 text-[12px] text-ink-1"
              >
                {engines.map((e) => (
                  <option key={e.id} value={e.id}>
                    {e.label}
                    {e.unsupported_here ? t("（这台跑不了）") : e.installed ? "" : t("（没装）")}
                  </option>
                ))}
              </select>
            </label>

            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-ink-4">{t("算力")}</span>
              <select
                value={String(cfg.gpu_layers)}
                onChange={(e) => saveCfg({ ...cfg, gpu_layers: Number(e.target.value) })}
                className="rounded-lg border border-white/[0.08] bg-white/[0.03] px-2.5 py-1.5 text-[12px] text-ink-1"
              >
                <option value="-1">{t("自动")}</option>
                <option value="999">{t("全部交给显卡")}</option>
                <option value="0">{t("只用 CPU")}</option>
              </select>
            </label>

            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-ink-4">{t("上下文")}</span>
              <select
                value={String(cfg.ctx)}
                onChange={(e) => saveCfg({ ...cfg, ctx: Number(e.target.value) })}
                className="rounded-lg border border-white/[0.08] bg-white/[0.03] px-2.5 py-1.5 text-[12px] text-ink-1"
              >
                {CTX_CHOICES.map((c) => (
                  <option key={c} value={c}>
                    {c / 1024}K
                  </option>
                ))}
              </select>
            </label>

            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-ink-4">{t("端口")}</span>
              <input
                type="number"
                value={cfg.port}
                disabled={engineId === "ollama"}
                onChange={(e) => saveCfg({ ...cfg, port: Number(e.target.value) || 18820 })}
                className="w-[92px] rounded-lg border border-white/[0.08] bg-white/[0.03] px-2.5 py-1.5 text-[12px] text-ink-1 disabled:opacity-50"
              />
            </label>

            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-ink-4">{t("线程")}</span>
              <input
                type="number"
                value={cfg.threads}
                onChange={(e) => saveCfg({ ...cfg, threads: Math.max(0, Number(e.target.value) || 0) })}
                className="w-[76px] rounded-lg border border-white/[0.08] bg-white/[0.03] px-2.5 py-1.5 text-[12px] text-ink-1"
                title={t("0 = 引擎自己按核心数定")}
              />
            </label>
          </div>

          {/* 上下文越大越吃内存，这句得说在他调之前 */}
          <p className="text-[11px] leading-relaxed text-ink-5">
            {t("上下文调大 = 一次能读更长的东西，但还没开口就先吃掉更多内存；显存不够时「全部交给显卡」会让引擎直接退出，拿不准就留「自动」。")}
          </p>

          <div className="flex flex-wrap items-center gap-2 pt-0.5">
            {engine && !engine.installed && !engine.unsupported_here && (
              <button
                disabled={!!busy}
                data-action-id="runtime.localllm.install"
                onClick={() =>
                  void run(
                    `${engine.id}:install`,
                    () => invoke("localllm_install", { engine: engine.id }),
                    t("{name} 已安装", { name: engine.label }),
                  )
                }
                className="inline-flex items-center gap-1.5 rounded-card bg-accent/15 px-4 py-2 text-[12.5px] text-accent hover:bg-accent/25 disabled:opacity-50"
              >
                {busy === `${engine.id}:install` ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <Download size={14} />
                )}
                {t("安装引擎")}
              </button>
            )}

            {running ? (
              <button
                disabled={!!busy}
                data-action-id="runtime.localllm.stop"
                onClick={() =>
                  void run(`${engineId}:stop`, () => invoke("localllm_stop", { engine: engineId }), t("已停止"))
                }
                className="inline-flex items-center gap-1.5 rounded-card border border-white/[0.08] px-4 py-2 text-[12.5px] text-ink-2 hover:bg-white/[0.05] disabled:opacity-50"
              >
                {busy === `${engineId}:stop` ? <Loader2 size={14} className="animate-spin" /> : <Square size={14} />}
                {t("停止")}
              </button>
            ) : (
              <button
                disabled={!!busy || !engine || engine.unsupported_here || !engine.installed}
                data-action-id="runtime.localllm.start"
                onClick={() =>
                  void run(
                    `${engineId}:start`,
                    () =>
                      invoke("localllm_start", {
                        engine: engineId,
                        model,
                        port: cfg.port,
                        ctx: cfg.ctx,
                        gpuLayers: cfg.gpu_layers,
                        threads: cfg.threads,
                      }),
                    t("已启动，端点已就绪"),
                  )
                }
                className="inline-flex items-center gap-1.5 rounded-card bg-accent/15 px-4 py-2 text-[12.5px] text-accent hover:bg-accent/25 disabled:opacity-40"
              >
                {busy === `${engineId}:start` ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
                {t("启动")}
              </button>
            )}

            {engine?.endpoint && (
              <span className="inline-flex items-center gap-1.5 text-[11.5px] text-success-400">
                <CheckCircle2 size={12} />
                <span className="font-mono">{engine.endpoint}</span>
                <span className="text-ink-4">{t("已加进「AI 设置」当驱动")}</span>
              </span>
            )}
          </div>

          {engine && engine.blockers.length > 0 && !running && (
            <div className="flex items-start gap-1.5 text-[11.5px] leading-relaxed text-warn-500">
              <AlertTriangle size={12} className="mt-0.5 shrink-0" />
              <span>{engine.blockers.join("；")}</span>
            </div>
          )}
        </section>

        {/* ── 标准输出：加载慢/失败时，客户唯一能看的东西 ── */}
        <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-3.5">
          <div className="mb-2 flex items-center gap-2">
            <Terminal size={14} className="text-ink-4" />
            <h3 className="text-[13px] font-semibold text-ink-1">{t("标准输出")}</h3>
            <span className="text-[11px] text-ink-5">{engineId}</span>
          </div>
          <pre
            ref={logBox}
            className="max-h-[220px] min-h-[96px] overflow-auto whitespace-pre-wrap break-all rounded-lg bg-black/30 px-3 py-2 text-[11px] leading-relaxed text-ink-3"
          >
            {logText || t("（还没有日志。模型第一次加载要几十秒到几分钟，进度会在这里滚。）")}
          </pre>
        </section>

        {/* ── 引擎货架：每个自己回答「你这台能不能用我」 ── */}
        <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
          {engines.map((e) => {
            const isRunning = e.running_pid != null;
            return (
              <button
                key={e.id}
                onClick={() => setEngineId(e.id)}
                className={cn(
                  "rounded-card border px-4 py-3 text-left",
                  e.unsupported_here
                    ? "border-white/[0.05] bg-bg-2/40 opacity-70"
                    : engineId === e.id
                      ? "border-accent/30 bg-accent/[0.06]"
                      : "border-white/[0.06] bg-bg-2/70 hover:bg-white/[0.03]",
                )}
              >
                <div className="flex items-center gap-2">
                  <h3 className="text-[13.5px] font-semibold text-ink-0">{e.label}</h3>
                  {isRunning ? (
                    <span className="inline-flex items-center gap-1 rounded-full bg-success-500/15 px-2 py-0.5 text-[10.5px] text-success-400">
                      <CheckCircle2 size={11} /> {t("正在跑")}
                    </span>
                  ) : e.ready ? (
                    <span className="rounded-full bg-white/[0.06] px-2 py-0.5 text-[10.5px] text-ink-3">
                      {t("可以用")}
                    </span>
                  ) : e.unsupported_here ? (
                    <span className="rounded-full bg-white/[0.06] px-2 py-0.5 text-[10.5px] text-ink-4">
                      {t("这台跑不了")}
                    </span>
                  ) : (
                    <span className="rounded-full bg-white/[0.06] px-2 py-0.5 text-[10.5px] text-ink-4">
                      {e.installed ? t("差一步") : t("还没装")}
                    </span>
                  )}
                  {e.version && <span className="font-mono text-[10.5px] text-ink-5">{e.version}</span>}
                </div>
                <p className="mt-1 text-[11.5px] leading-relaxed text-ink-4">{e.blurb}</p>
                {/* 卡在哪：说人话，且**不藏**。这是这一页最重要的一行 */}
                {e.blockers.length > 0 && (
                  <div className="mt-1 flex items-start gap-1.5 text-[11.5px] leading-relaxed text-warn-500">
                    <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                    <span>{e.blockers.join("；")}</span>
                  </div>
                )}
              </button>
            );
          })}
        </div>

        {/* 协议现实：别让人以为配好本地模型 Claude Code 就能用 */}
        <p className="px-1 text-[11.5px] leading-relaxed text-ink-4">
          {t("这些引擎给的都是 OpenAI 兼容端点。Claude Code 和新版 Codex 认的是另外两种协议，接不了本地模型；要用本地模型请配进 ClawX 或 Hermes。")}
        </p>

        {/* 模型目录：扫这些地方找模型（下载落点在右边那块单独设） */}
        <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-3.5 space-y-2">
          <div className="flex items-center gap-2">
            <FolderPlus size={14} className="text-ink-4" />
            <h3 className="text-[13px] font-semibold text-ink-1">{t("模型目录")}</h3>
            <button
              data-action-id="runtime.localllm.model_add"
              onClick={async () => {
                const dir = await openDialog({ directory: true, multiple: false });
                if (typeof dir === "string") {
                  await run(
                    "dirs:add",
                    () => invoke("localllm_model_add", { kind: "dir", path: dir }),
                    t("已添加模型目录"),
                  );
                }
              }}
              className="ml-auto inline-flex items-center gap-1.5 rounded-card border border-white/[0.08] px-2.5 py-1.5 text-[11.5px] text-ink-2 hover:bg-white/[0.05]"
            >
              <FolderPlus size={12} /> {t("添加目录")}
            </button>
            <button
              data-action-id="runtime.localllm.model_add"
              onClick={async () => {
                const f = await openDialog({
                  multiple: false,
                  filters: [{ name: "GGUF", extensions: ["gguf"] }],
                });
                if (typeof f === "string") {
                  const base = f.split(/[\\/]/).pop() || "";
                  const name = base.replace(/\.gguf$/i, "").replace(/[^A-Za-z0-9\-_.:]/g, "-").slice(0, 64);
                  await run(
                    "dirs:gguf",
                    () => invoke("localllm_model_add", { kind: "gguf", path: f, name }),
                    t("已导入到 Ollama"),
                  );
                }
              }}
              className="inline-flex items-center gap-1.5 rounded-card border border-white/[0.08] px-2.5 py-1.5 text-[11.5px] text-ink-2 hover:bg-white/[0.05]"
            >
              <Upload size={12} /> {t("导入 GGUF 到 Ollama")}
            </button>
          </div>
          <div className="space-y-1">
            {(data.model_dirs || []).map((d) => (
              <button
                key={d}
                onClick={() => void openPath(d).catch((e) => onToast?.(String(e)))}
                className="block w-full break-all text-left font-mono text-[11.5px] text-ink-4 hover:text-ink-2"
              >
                {d}
              </button>
            ))}
          </div>
        </section>
      </div>

      {/* ───────── 右：本地有什么 / 商店能下什么 ───────── */}
      <aside className="w-full shrink-0 xl:w-[380px]">
        <LocalLLMStore
          localModels={data.local_models || []}
          downloadDir={data.download_dir || ""}
          ramGb={ramGb}
          current={model}
          onPick={(p) => setPicked((s) => ({ ...s, [engineId || "llamacpp"]: p }))}
          onChanged={() => void refresh()}
          onToast={onToast}
        />
      </aside>
    </div>
  );
}
