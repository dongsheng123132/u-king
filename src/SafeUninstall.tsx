/**
 * 安全卸载 / 逐项清理面板 —— 从 Advanced.tsx（进阶页，2026-10-19 已删）拆出的独立模块。
 *
 * DemoUninstaller 与曾经的「进阶」页共用同一套扫描/官方卸载协议（后端 cleanup.rs），
 * 不复制任何删除逻辑。自包含：只靠 onToast 与后端 command，删本段只动本文件 + DemoUninstaller。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Check, Loader2, RefreshCw, ShieldAlert, ShieldCheck, Trash2 } from "lucide-react";
import { useI18n } from "./i18n";
import { cn } from "./lib/cn";
import { askConfirm } from "./lib/confirm";

/** 一条足迹（后端 cleanup::FootprintItem 的镜像）。 */
type FootprintItem = {
  id: string;
  group: "core" | "config" | "aitool";
  name: string;
  detail: string;
  safe: boolean;
  warn: string;
};

/** 三档分组的展示元数据（标题 / 副标题 / 色调）。 */
const GROUP_META: Record<
  FootprintItem["group"],
  { title: string; hint: string; icon: typeof ShieldCheck; tone: "safe" | "warn" }
> = {
  core: { title: "U-King 自己装的", hint: "安全 · 默认清除", icon: ShieldCheck, tone: "safe" },
  config: {
    title: "U-King 改过的配置（清除 = 还原到改动前）",
    hint: "安全 · 默认清除 · 不会清空你的 ~/.claude 等目录",
    icon: RefreshCw,
    tone: "safe",
  },
  aitool: {
    title: "U-King 帮你装的 AI 工具 / 厨具",
    hint: "可能你之前就有 · 默认不删，逐个确认",
    icon: ShieldAlert,
    tone: "warn",
  },
};
const GROUP_ORDER: FootprintItem["group"][] = ["core", "config", "aitool"];

/**
 * 安全卸载 / 逐项清理面板 —— 诚实扫描本机上 U-King 的全部足迹（后端 cleanup.rs），
 * 分三档勾选：core/config 默认勾（安全，清除=还原），aitool 默认不勾（帮你装的工具本体，附警告）。
 * 支持「逐项清除」与「清除所选」。含 uking-home（U-King 本体）时 = 彻底卸载并关闭。
 * 自包含：只靠 onToast 与后端 command，删本段只动本文件。
 */
export function SafeUninstall({ onToast, demo = false }: { onToast: (s: string) => void; demo?: boolean }) {
  const { t } = useI18n();
  const [items, setItems] = useState<FootprintItem[] | null>(null);
  const [sel, setSel] = useState<Record<string, boolean>>({});
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [confirm, setConfirm] = useState(false);
  // 独立演示版默认保留资料：先归档到“文档/U-King 演示保留数据”，再卸载，
  // 这样下一次安装仍是干净状态，不会因为旧目录被检测成“已安装”。
  const [preserveUserData, setPreserveUserData] = useState(demo);

  const scan = useRef(async () => {});
  scan.current = async () => {
    setScanning(true);
    try {
      const list = await invoke<FootprintItem[]>("cleanup_scan");
      setItems(list);
      // 主程序默认只勾安全项；独立演示清场版默认全选，仍会在真正执行前展示确认。
      const s: Record<string, boolean> = {};
      list.forEach((it) => (s[it.id] = demo || it.safe));
      setSel(s);
    } catch (e) {
      onToast(String(e));
      setItems([]);
    } finally {
      setScanning(false);
    }
  };
  useEffect(() => {
    scan.current();
    const p = listen<string>("uking:cleanup_progress", (e) =>
      setLogs((l) => [...l.slice(-120), e.payload]),
    );
    return () => {
      p.then((un) => un());
    };
  }, []);

  const toggle = (id: string) => setSel((s) => ({ ...s, [id]: !s[id] }));
  const selectedIds = items ? items.filter((it) => sel[it.id]).map((it) => it.id) : [];
  const selAiTools = items ? items.filter((it) => sel[it.id] && it.group === "aitool").length : 0;
  const homeSelected = selectedIds.includes("uking-home");

  const run = async (ids: string[]) => {
    if (!ids.length || busy) return;
    setBusy(true);
    setLogs([]);
    try {
      const willExit = await invoke<boolean>("cleanup_run", {
        ids,
        preserveUserData: demo && preserveUserData,
      });
      if (willExit) {
        onToast(t("正在彻底卸载并清理，U-King 即将关闭…"));
      } else {
        onToast(t("已清除所选项"));
        await scan.current(); // 刷新剩余足迹
      }
    } catch (e) {
      onToast(t("清理失败：{e}", { e: String(e) }));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  /** 逐项清除（单条）。aitool / uking-home 二次确认（会真卸载 / 会关闭）。 */
  const removeOne = async (it: FootprintItem) => {
    if (busy) return;
    if (it.id === "uking-home") {
      if (!(await askConfirm(t("这会删除 U-King 本体并关闭程序，确定？")))) return;
    } else if (it.group === "aitool") {
      if (
        !(await askConfirm(
          t("确定卸载「{name}」？若你之前自己装过，这会真的删掉它。", { name: it.name }),
        ))
      )
        return;
    }
    void run([it.id]);
  };

  const grouped = (g: FootprintItem["group"]) => (items ?? []).filter((it) => it.group === g);
  const empty = items !== null && items.length === 0;

  return (
    <section className="rounded-card border border-danger-500/25 bg-danger-500/[0.04] overflow-hidden">
      {/* 页头 */}
      <div className="flex items-center gap-2 px-5 py-3.5 border-b border-danger-500/15">
        <Trash2 size={16} className="text-danger-400 shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold text-ink-0">{t("安全卸载 / 逐项清理")}</div>
          <div className="text-[11px] text-ink-4">
            {demo
              ? t("演示清场模式：已默认选中检测到的所有工具与 U-King 足迹；请确认后执行。")
              : t("列出 U-King 在这台电脑上留下的全部东西，逐项可删、配置可还原到改动前，或一键彻底卸载。")}
          </div>
        </div>
        <button
          onClick={() => scan.current()}
          disabled={scanning || busy}
          className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-white/[0.10] text-[11.5px] text-ink-2 hover:bg-white/[0.04] disabled:opacity-50 shrink-0"
        >
          <RefreshCw size={12} className={scanning ? "animate-spin" : ""} /> {t("重新扫描")}
        </button>
      </div>

      <div className="px-5 py-4 space-y-4">
        {scanning && items === null ? (
          <div className="flex items-center gap-2 text-[12px] text-ink-3 py-4">
            <Loader2 size={14} className="animate-spin" /> {t("正在扫描本机足迹…")}
          </div>
        ) : empty ? (
          <div className="flex items-center gap-2 text-[12px] text-success-400 py-4">
            <Check size={14} /> {t("本机没有检测到 U-King 的任何足迹（干净）。")}
          </div>
        ) : (
          GROUP_ORDER.map((g) => {
            const list = grouped(g);
            if (!list.length) return null;
            const meta = GROUP_META[g];
            const Icon = meta.icon;
            return (
              <div key={g}>
                <div className="flex items-center gap-1.5 mb-1.5">
                  <Icon size={13} className={meta.tone === "warn" ? "text-warning-400" : "text-accent"} />
                  <span className="text-[12px] font-semibold text-ink-1">{t(meta.title)}</span>
                  <span className="text-[10.5px] text-ink-4">· {t(meta.hint)}</span>
                </div>
                <div className="space-y-1.5">
                  {list.map((it) => (
                    <label
                      key={it.id}
                      className={cn(
                        "flex items-start gap-2.5 rounded-lg border px-3 py-2 cursor-pointer transition-colors",
                        sel[it.id]
                          ? "border-accent/30 bg-accent/[0.05]"
                          : "border-white/[0.07] bg-bg-1/40 hover:bg-white/[0.03]",
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={!!sel[it.id]}
                        onChange={() => toggle(it.id)}
                        disabled={busy}
                        className="mt-0.5 accent-accent shrink-0"
                      />
                      <div className="min-w-0 flex-1">
                        <div className="text-[12.5px] text-ink-1 font-medium">{t(it.name)}</div>
                        <div className="text-[11px] text-ink-4 break-all">{it.detail}</div>
                        {it.warn && (
                          <div className="mt-0.5 inline-flex items-start gap-1 text-[10.5px] text-warning-400">
                            <AlertTriangle size={11} className="mt-[1px] shrink-0" /> {t(it.warn)}
                          </div>
                        )}
                      </div>
                      <button
                        onClick={(e) => {
                          e.preventDefault();
                          void removeOne(it);
                        }}
                        disabled={busy}
                        title={t("单独清除这一项")}
                        className="inline-flex items-center gap-1 px-2 h-6 rounded-md border border-white/[0.10] text-[10.5px] text-ink-3 hover:text-danger-400 hover:border-danger-500/40 disabled:opacity-40 shrink-0"
                      >
                        <Trash2 size={11} /> {t("清除")}
                      </button>
                    </label>
                  ))}
                </div>
              </div>
            );
          })
        )}

        {demo && !empty && (
          <label className="flex items-start gap-2.5 rounded-lg border border-success-500/25 bg-success-500/[0.06] px-3.5 py-3 cursor-pointer">
            <input
              type="checkbox"
              checked={preserveUserData}
              onChange={() => setPreserveUserData((v) => !v)}
              disabled={busy}
              className="mt-0.5 accent-success-500 shrink-0"
            />
            <span className="min-w-0">
              <span className="block text-[12.5px] font-medium text-ink-1">保留用户历史数据（推荐）</span>
              <span className="block mt-0.5 text-[11px] leading-relaxed text-ink-3">
                清场前会备份 U-King 的任务、作图和视频记录，以及 OpenClaw / ClawX / Hermes / Codex 桌面版的本地数据到“文档 / U-King 演示保留数据”。重新安装仍从干净状态开始。
              </span>
            </span>
          </label>
        )}

        {/* 进度日志 */}
        {logs.length > 0 && (
          <div className="rounded-lg border border-white/[0.08] bg-bg-1/60 px-3 py-2 max-h-32 overflow-auto">
            {logs.map((l, i) => (
              <div key={i} className="text-[11px] font-mono text-ink-3 leading-relaxed break-all">
                {l}
              </div>
            ))}
          </div>
        )}

        {/* 底部操作条 */}
        {!empty && items !== null && (
          <div className="pt-1 border-t border-white/[0.06]">
            {!confirm ? (
              <div className="flex flex-wrap items-center gap-2 pt-3">
                <button
                  onClick={() => setConfirm(true)}
                  disabled={busy || selectedIds.length === 0}
                  className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-card bg-danger-500 text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-40"
                >
                  {busy ? <Loader2 size={13} className="animate-spin" /> : <Trash2 size={13} />}
                  {homeSelected
                    ? demo
                      ? t("一键清场并关闭")
                      : t("彻底卸载并关闭")
                    : t("清除所选（{n} 项）", { n: String(selectedIds.length) })}
                </button>
                <span className="text-[11px] text-ink-4">
                  {selAiTools > 0 && (
                    <span className="text-warning-400">
                      {t("含 {n} 个 AI 工具本体", { n: String(selAiTools) })} ·{" "}
                    </span>
                  )}
                  {demo ? t("可取消不想处理的项目；配置项只还原 U-King 的改动") : t("勾选后清除；配置项会还原到改动前")}
                </span>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2 pt-3">
                <span className="text-[12px] text-ink-2">
                  {homeSelected
                    ? demo
                      ? t("确认演示清场？将卸载勾选的工具、删除 ~/.uking，并关闭本工具。")
                      : t("确认彻底卸载？将删除 ~/.uking 并关闭 U-King。")
                    : t("确认清除所选 {n} 项？", { n: String(selectedIds.length) })}
                  {selAiTools > 0 && (
                    <b className="text-warning-400">
                      {t("（含 {n} 个会真卸载的 AI 工具/厨具）", { n: String(selAiTools) })}
                    </b>
                  )}
                  {demo && preserveUserData && (
                    <span className="text-success-400">{t("（会先保留用户历史数据）")}</span>
                  )}
                </span>
                <button
                  data-action-id="runtime.footprint.remove"
                  onClick={() => void run(selectedIds)}
                  disabled={busy}
                  className="px-3 py-1.5 rounded-card bg-danger-500 text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-50"
                >
                  {busy ? t("正在清理…") : t("确认清除")}
                </button>
                <button
                  onClick={() => setConfirm(false)}
                  disabled={busy}
                  className="px-3 py-1.5 rounded-card border border-white/10 text-[12px] text-ink-3 hover:bg-white/[0.04] disabled:opacity-50"
                >
                  {t("取消")}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
