/**
 * 单工具「一键配好」按钮 —— 从 ToolCheckup 收拢来的唯一不可去重能力（评审
 * docs/first-principles-review-for-fable-2026-09-06.md ③）：ToolCheckup 整卡
 * 与 DoctorCard 展开视图的工具状态是重复展示，唯独这个修复动作 DoctorCard 原来没有。
 *
 * 只承载：确认气泡 → 调用 apply_provider 把虾盘云配上 → 体检回读确认真的写上了 →
 * test_provider 验证连通 → 展示 running/ok/fail，失败可重试。
 * 修好后**不在本地假装绿**——调用方必须传入 onFixed，由它强制重跑 doctor_report
 * 让状态从真实体检结果读回（不是只把这颗按钮的徽章改绿）。
 */
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CheckCircle2, Loader2 } from "lucide-react";
import { useI18n } from "../i18n";
import type { DeviceKey } from "../lib/types";
import type { AiCheckupItem } from "../lib/doctorHealth";

type TestResult = { ok: boolean; error: string | null };
type FixState = "fixing" | "ready" | "failed";

function failureMessage(error: string | null | undefined, t: (key: string) => string) {
  const value = error?.toLowerCase() ?? "";
  if (value.includes("401") || value.includes("unauthorized")) return t("Key 校验没过");
  if (value.includes("timeout") || value.includes("timed out")) return t("网络不通");
  return t("暂时没配上，可以稍后再试");
}

/** 应用后立刻体检回读：确认托管配置真的落上了（sol 终审要求逐目标校验）。 */
async function readbackReady(target: string): Promise<boolean> {
  try {
    const report = await invoke<{ items: AiCheckupItem[] }>("ai_checkup");
    return report.items.some((i) => i.target === target && i.state === "ready");
  } catch {
    return false;
  }
}

export function ToolFixButton({
  item,
  disabled,
  onFixingChange,
  onFixed,
}: {
  item: AiCheckupItem;
  /** 兄弟工具正在修的时候禁用当前这个——防并发多起一条 apply 流水把同一批配置写花。 */
  disabled: boolean;
  onFixingChange: (fixing: boolean) => void;
  /** 修复成功后调用方应强制重跑 doctor_report 让状态读回。 */
  onFixed: () => void;
}) {
  const { t } = useI18n();
  const [confirming, setConfirming] = useState(false);
  const [fix, setFix] = useState<{ state: FixState; error?: string } | undefined>();

  async function configure() {
    setConfirming(false);
    setFix({ state: "fixing" });
    onFixingChange(true);
    try {
      const device = await invoke<DeviceKey>("get_device_key");
      await invoke("apply_provider", {
        providerId: "xiapan",
        apiKey: device.key,
        model: null,
        targets: [item.target],
      });
      const wrote = await readbackReady(item.target);
      if (!wrote) {
        setFix({ state: "failed", error: t("配置好像没写进去，点「重试」再试一次") });
        return;
      }
      const test = await invoke<TestResult>("test_provider", {
        providerId: "xiapan",
        apiKey: device.key,
        model: null,
        api: "openai",
      });
      if (test.ok) {
        setFix({ state: "ready" });
        onFixed();
      } else {
        setFix({ state: "failed", error: failureMessage(test.error, t) });
      }
    } catch (error) {
      setFix({ state: "failed", error: failureMessage(String(error), t) });
    } finally {
      onFixingChange(false);
    }
  }

  if (fix?.state === "ready") {
    return (
      <span className="inline-flex items-center gap-1 text-[10.5px] text-success-400 shrink-0">
        <CheckCircle2 size={12} /> {t("配好了，去终端试试")}
      </span>
    );
  }

  if (fix?.state === "fixing") {
    return <Loader2 size={13} className="animate-spin text-accent shrink-0" />;
  }

  if (fix?.state === "failed") {
    return (
      <div className="flex items-center gap-2 shrink-0">
        <span className="text-[10.5px] text-danger-400">{fix.error}</span>
        {!disabled && (
          <button
            onClick={() => setFix(undefined)}
            className="h-6 px-2 rounded-md border border-accent/35 text-[10.5px] font-medium text-accent hover:bg-accent/[0.08]"
          >
            {t("重试")}
          </button>
        )}
      </div>
    );
  }

  if (confirming) {
    return (
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-accent/25 bg-accent/[0.06] px-3 py-2 shrink-0">
        <span className="flex-1 min-w-[190px] text-[10.5px] leading-relaxed text-ink-2">
          {t("用 U-King 内置虾盘云额度给它配好，并确认配置写上、虾盘云连通")}
        </span>
        <button
          onClick={configure}
          className="h-7 px-2.5 rounded-md bg-accent text-[11px] font-medium text-white hover:bg-accent-600"
        >
          {t("一键配好")}
        </button>
        <button onClick={() => setConfirming(false)} className="h-7 px-1 text-[10.5px] text-ink-3 hover:text-ink-1">
          {t("稍后")}
        </button>
      </div>
    );
  }

  return (
    <button
      onClick={() => setConfirming(true)}
      disabled={disabled}
      className="ml-auto h-7 px-2.5 rounded-md border border-accent/35 text-[11px] font-medium text-accent hover:bg-accent/[0.08] disabled:opacity-40 disabled:cursor-not-allowed shrink-0"
    >
      {t("一键配好")}
    </button>
  );
}
