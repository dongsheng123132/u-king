/**
 * Free Router（本地免费路由网关）一键装跑卡 —— 2026-08-31 会审定案的深度集成。
 *
 * 定位（见 freerouter.rs 模块头）：第三方开源本地网关，把 OpenRouter 免费模型
 * 汇成一个本地 OpenAI 兼容接口（127.0.0.1:8787，模型名 free-best），限流/下架自动换。
 * 上游代码钉死 commit SHA + tarball SHA-256 双校验；Key 只写本机 .env，绝不回显。
 *
 * 形态纪律：这是「进阶工具」不是「新免费额度」——没有 OpenRouter Key 它什么都做不了，
 * 卡片文案必须先讲清这一点，别让小白以为点一下就白得模型。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowUpRight,
  CheckCircle2,
  Loader2,
  Play,
  Router,
  Square,
  Wrench,
} from "lucide-react";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

type FrStatus = {
  installed: boolean;
  running: boolean;
  version: string;
  key_configured: boolean;
  dir: string;
  log_tail: string[];
};

export function FreerouterCard({ onToast }: { onToast?: (msg: string) => void }) {
  const { t } = useI18n();
  const [st, setSt] = useState<FrStatus | null>(null);
  const [busy, setBusy] = useState<"" | "install" | "start" | "stop" | "key">("");
  const [showKey, setShowKey] = useState(false);
  const [keyInput, setKeyInput] = useState("");

  const refresh = useCallback(() => {
    invoke<FrStatus>("freerouter_status").then(setSt).catch(() => {});
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh]);

  async function doInstall() {
    setBusy("install");
    try {
      const logs = await invoke<string[]>("freerouter_install");
      onToast?.(t("Free Router 已安装"));
      logs.forEach(() => {}); // 日志进 toast 没意义，成功文案足够；失败走 catch
    } catch (e) {
      onToast?.(t("安装失败：") + String(e));
    } finally {
      setBusy("");
      refresh();
    }
  }

  async function doStart() {
    setBusy("start");
    try {
      await invoke("freerouter_start");
      onToast?.(t("Free Router 已在后台运行：") + "http://127.0.0.1:8787/v1");
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy("");
      refresh();
    }
  }

  async function doStop() {
    setBusy("stop");
    try {
      await invoke("freerouter_stop");
      onToast?.(t("已停止"));
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy("");
      refresh();
    }
  }

  async function saveKey() {
    setBusy("key");
    try {
      await invoke("freerouter_set_key", { key: keyInput.trim() });
      setKeyInput("");
      setShowKey(false);
      onToast?.(t("Key 已保存到本机 .env（不会上传）"));
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy("");
      refresh();
    }
  }

  const installed = !!st?.installed;
  const running = !!st?.running;

  return (
    <section className="mb-3 rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
      <div className="flex items-center gap-2.5 px-4 py-3">
        <span className="grid place-items-center w-7 h-7 rounded-lg bg-accent/[0.12] shrink-0">
          <Router size={14} className="text-accent" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-ink-1">
            {t("Free Router · 本地免费路由")}
            {running && (
              <span className="ml-2 inline-flex items-center gap-1 text-[10.5px] text-success-400">
                <CheckCircle2 size={11} /> {t("运行中")}
              </span>
            )}
          </div>
          <div className="text-[11px] text-ink-4 mt-0.5">
            {t("把 OpenRouter 免费模型汇成一个本地接口，限流/下架自动换下一家 —— 需要先有 OpenRouter Key")}
          </div>
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          {!installed ? (
            <button
              onClick={doInstall}
              disabled={busy !== ""}
              className="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-60"
            >
              {busy === "install" ? <Loader2 size={13} className="animate-spin" /> : <Wrench size={13} />}
              {t("一键安装")}
            </button>
          ) : running ? (
            <button
              onClick={doStop}
              disabled={busy !== ""}
              className="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg border border-white/[0.1] text-ink-2 text-[12px] hover:bg-white/[0.04] disabled:opacity-60"
            >
              {busy === "stop" ? <Loader2 size={13} className="animate-spin" /> : <Square size={12} />}
              {t("停止")}
            </button>
          ) : (
            <button
              onClick={doStart}
              disabled={busy !== ""}
              className="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-60"
            >
              {busy === "start" ? <Loader2 size={13} className="animate-spin" /> : <Play size={13} />}
              {t("启动")}
            </button>
          )}
        </div>
      </div>

      {/* 已装后的状态行 + Key 管理 */}
      {installed && (
        <div className="px-4 pb-3 space-y-2">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px]">
            <span className={cn("font-mono", st?.key_configured ? "text-success-400" : "text-amber-500")}>
              {st?.key_configured ? t("OpenRouter Key ✓ 已配置") : t("还没有配 OpenRouter Key")}
            </span>
            {!st?.key_configured && (
              <>
                <button
                  onClick={() => setShowKey((v) => !v)}
                  className="text-accent hover:underline"
                >
                  {t("填 Key")}
                </button>
                <button
                  onClick={() => openUrl("https://openrouter.ai/keys")}
                  className="inline-flex items-center gap-0.5 text-ink-3 hover:text-ink-1"
                >
                  {t("去领 Key")} <ArrowUpRight size={10} />
                </button>
              </>
            )}
            <span className="text-ink-5 font-mono" title={st?.dir}>
              {t("版本")} {st?.version}
            </span>
          </div>

          {/* Key 输入（收起态不占位；值不回显不落日志） */}
          {showKey && (
            <div className="flex items-center gap-2">
              <input
                type="password"
                value={keyInput}
                onChange={(e) => setKeyInput(e.target.value)}
                placeholder="sk-or-…"
                className="flex-1 h-8 px-3 rounded-lg bg-bg-1 border border-white/[0.1] text-[12px] font-mono text-ink-1 focus:outline-none focus:border-accent/50"
              />
              <button
                onClick={saveKey}
                disabled={busy !== "" || keyInput.trim().length < 20}
                className="h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-60"
              >
                {busy === "key" ? <Loader2 size={13} className="animate-spin" /> : t("保存")}
              </button>
            </div>
          )}

          {/* 运行中：给「怎么用」的一句话 + 停止入口已在标题行 */}
          {running && (
            <div className="text-[11px] text-ink-4 leading-relaxed">
              {t("本地接口")} <span className="font-mono text-ink-3">http://127.0.0.1:8787/v1</span>
              {" · "}{t("模型名")} <span className="font-mono text-ink-3">free-best</span>
              {" · "}
              {t(
                "在「供应商库」添加自定义供应商填这个地址即可把任何 OpenAI 兼容工具接上来（仅本机可访问）",
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
