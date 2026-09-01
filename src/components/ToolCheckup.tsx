import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CheckCircle2, Loader2 } from "lucide-react";
import { useI18n } from "../i18n";
import type { DeviceKey } from "../lib/types";

type AiCheckupItem = {
  target: string;
  label: string;
  installed: boolean;
  state: "ready" | "idle" | "self-managed" | "absent";
  model: string | null;
  can_auto_fix: boolean;
};

type AiCheckupReport = { charged: boolean; items: AiCheckupItem[] };
type TestResult = { ok: boolean; error: string | null };
type FixState = "ready" | "fixing" | "failed";

function failureMessage(error: string | null | undefined, t: (key: string) => string) {
  const value = error?.toLowerCase() ?? "";
  if (value.includes("401") || value.includes("unauthorized")) return t("Key 校验没过");
  if (value.includes("timeout") || value.includes("timed out")) return t("网络不通");
  return t("暂时没配上，可以稍后再试");
}

/** 应用后立刻体检回读：确认托管配置真的落上了（sol 终审要求逐目标校验）。 */
async function readbackReady(target: string): Promise<boolean> {
  try {
    const report = await invoke<AiCheckupReport>("ai_checkup");
    return report.items.some((i) => i.target === target && i.state === "ready");
  } catch {
    return false;
  }
}

/** 已安装 AI 工具的配置体检。老版本没有 ai_checkup 时整卡静默隐藏。 */
export function ToolCheckup() {
  const { t } = useI18n();
  const [report, setReport] = useState<AiCheckupReport | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [fixes, setFixes] = useState<Record<string, { state: FixState; error?: string }>>({});
  // 🔴 sol 终审：确认按钮要防重入——fixing 期间所有「一键配好」都禁用，防双击/多目标
  // 并发各起一条 apply 流水把同一批配置写花。
  const anyFixing = Object.values(fixes).some((f) => f.state === "fixing");

  useEffect(() => {
    let alive = true;
    invoke<AiCheckupReport>("ai_checkup")
      .then((next) => {
        if (alive) setReport(next);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  const items = report?.items.filter((item) => item.installed || item.state !== "absent") ?? [];
  if (!report || !items.length) return null;

  // 🔴 sol 终审：配好一项后顶部计数要跟着走——fix 成功的 target 从 idle 计数里排除。
  const locallyReady = new Set(
    Object.entries(fixes)
      .filter(([, f]) => f.state === "ready")
      .map(([target]) => target),
  );
  const idleCount = items.filter((item) => item.state === "idle" && !locallyReady.has(item.target)).length;

  async function configure(item: AiCheckupItem) {
    setConfirming(null);
    setFixes((current) => ({ ...current, [item.target]: { state: "fixing" } }));
    try {
      const device = await invoke<DeviceKey>("get_device_key");
      await invoke("apply_provider", {
        providerId: "xiapan",
        apiKey: device.key,
        model: null,
        targets: [item.target],
      });
      // 🔴 sol 终审：test_provider 只能证明「虾盘云活着」，配置是否真写上要靠体检回读。
      const wrote = await readbackReady(item.target);
      if (!wrote) {
        setFixes((c) => ({
          ...c,
          [item.target]: {
            state: "failed",
            error: t("配置好像没写进去，点「重试」再试一次"),
          },
        }));
        return;
      }
      const test = await invoke<TestResult>("test_provider", {
        providerId: "xiapan",
        apiKey: device.key,
        model: null,
        api: "openai",
      });
      setFixes((current) => ({
        ...current,
        [item.target]: test.ok
          ? { state: "ready" }
          : { state: "failed", error: failureMessage(test.error, t) },
      }));
    } catch (error) {
      setFixes((current) => ({
        ...current,
        [item.target]: { state: "failed", error: failureMessage(String(error), t) },
      }));
    }
  }

  return (
    <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
      <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
        <CheckCircle2 size={14} className="text-accent shrink-0" />
        <div className="min-w-0">
          <div className="text-[13px] font-medium text-ink-1">{t("工具体检")}</div>
          <div className="text-[11px] text-ink-3 mt-0.5">
            {idleCount ? t("{n} 个工具装好了还没接 AI", { n: idleCount }) : t("{n} 个 AI 助手全部就绪 ✅", { n: items.length })}
          </div>
        </div>
      </div>

      <div className="divide-y divide-white/[0.05]">
        {items.map((item) => {
          const fix = fixes[item.target];
          const ready = item.state === "ready" || fix?.state === "ready";
          return (
            <div key={item.target} className="px-4 py-3">
              <div className="flex items-center gap-2.5">
                <span
                  className={
                    "h-2 w-2 rounded-full shrink-0 " +
                    (ready ? "bg-success-400" : item.state === "self-managed" ? "bg-sky-400" : "bg-warning-400")
                  }
                />
                <span className="text-[12.5px] font-medium text-ink-1">{item.label}</span>
                {ready && item.model && <span className="text-[10.5px] text-ink-4 truncate">{item.model}</span>}
                {item.state === "self-managed" && <span className="text-[10.5px] text-sky-400">{t("已自行配置")}</span>}
                {fix?.state === "ready" && (
                  <span className="inline-flex items-center gap-1 text-[10.5px] text-success-400">
                    <CheckCircle2 size={12} /> {t("配好了，去终端试试")}
                  </span>
                )}
                {item.state === "idle" && !item.can_auto_fix && <span className="text-[10.5px] text-ink-4">{t("暂不支持自动配置")}</span>}
                {item.state === "idle" && item.can_auto_fix && !fix && (
                  <div className="ml-auto flex items-center gap-2 shrink-0">
                    <button
                      onClick={() => setConfirming(item.target)}
                      disabled={anyFixing}
                      className="h-7 px-2.5 rounded-md border border-accent/35 text-[11px] font-medium text-accent hover:bg-accent/[0.08] disabled:opacity-40 disabled:cursor-not-allowed"
                    >
                      {t("一键配好")}
                    </button>
                    <button onClick={() => setConfirming(null)} className="text-[10.5px] text-ink-4 hover:text-ink-2">
                      {t("稍后")}
                    </button>
                  </div>
                )}
                {fix?.state === "fixing" && <Loader2 size={14} className="ml-auto animate-spin text-accent" />}
              </div>

              {item.state === "idle" && item.can_auto_fix && !fix && (
                <div className="ml-[18px] mt-1 text-[10.5px] text-ink-3">
                  {t("检测到 {name} 装好还没接 AI，点一下用内置额度让它开口～", { name: item.label })}
                </div>
              )}
              {fix?.state === "failed" && (
                <div className="ml-[18px] mt-1 flex items-center gap-2">
                  {/* 🔴 sol 终审：失败不许把入口吞掉——失败可重试。重试=回确认气泡重来。 */}
                  <span className="text-[10.5px] text-danger-400">{fix.error}</span>
                  {!anyFixing && (
                    <button
                      onClick={() => setFixes((c) => {
                        const next = { ...c };
                        delete next[item.target];
                        return next;
                      })}
                      className="h-6 px-2 rounded-md border border-accent/35 text-[10.5px] font-medium text-accent hover:bg-accent/[0.08]"
                    >
                      {t("重试")}
                    </button>
                  )}
                </div>
              )}

              {confirming === item.target && (
                <div className="ml-[18px] mt-2 flex flex-wrap items-center gap-2 rounded-lg border border-accent/25 bg-accent/[0.06] px-3 py-2">
                  <span className="flex-1 min-w-[190px] text-[10.5px] leading-relaxed text-ink-2">
                    {/* 🔴 sol 终审：话术要如实——test 只证虾盘云可用，不逐工具烧 token。 */}
                    {t("用 U-King 内置虾盘云额度给它配好，并确认配置写上、虾盘云连通")}
                  </span>
                  <button onClick={() => configure(item)} className="h-7 px-2.5 rounded-md bg-accent text-[11px] font-medium text-white hover:bg-accent-600">
                    {t("一键配好")}
                  </button>
                  <button onClick={() => setConfirming(null)} className="h-7 px-1 text-[10.5px] text-ink-3 hover:text-ink-1">
                    {t("稍后")}
                  </button>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
