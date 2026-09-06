import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceKey } from "./lib/types";

type ActionEnvelope = {
  ok: boolean;
  result?: Record<string, unknown>;
  error?: { message?: string };
};

const ACTION = {
  inspect: "runtime.openclaw2.inspect",
  prepare: "runtime.openclaw2.prepare",
  configure: "runtime.openclaw2.configure_model_no_probe",
  launch: "runtime.openclaw2.launch",
  stop: "runtime.openclaw2.stop",
} as const;

/**
 * The portable preview deliberately mounts no desktop workbench.  Keeping the
 * five appliance operations here means App.tsx effects cannot write updates,
 * chat history, automation, or WebView state outside the package.
 */
export function PortableApp() {
  const [wallet, setWallet] = useState<DeviceKey | null>(null);
  const [runtime, setRuntime] = useState<Record<string, unknown> | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState("正在读取便携包状态…");

  const refresh = async () => {
    const [walletResult, runtimeResult] = await Promise.allSettled([
      invoke<DeviceKey>("get_device_key"),
      call(ACTION.inspect, {}, false),
    ]);
    if (walletResult.status === "fulfilled") setWallet(walletResult.value);
    if (runtimeResult.status === "rejected") throw runtimeResult.reason;
    if (!runtimeResult.value.ok) throw new Error(runtimeResult.value.error?.message || "读取 OpenClaw 状态失败");
    setRuntime(runtimeResult.value.result || null);
  };

  useEffect(() => {
    void refresh()
      .then(() => setNotice("零余额也可完成配置、启动和进入；模型调用前请先充值。"))
      .catch((error) => setNotice(String(error)));
  }, []);

  const run = async (label: string, action: string, input: Record<string, unknown> = {}) => {
    setBusy(label);
    try {
      const result = await call(action, input, true);
      if (!result.ok) throw new Error(result.error?.message || "操作失败");
      await refresh();
      setNotice(`${label}完成。`);
    } catch (error) {
      setNotice(`${label}失败：${String(error)}`);
    } finally {
      setBusy(null);
    }
  };

  const openRecharge = async () => {
    setBusy("充值");
    try {
      await invoke("open_recharge", { url: wallet?.recharge_url || "https://u-claw.org.cn/recharge" });
      setNotice("已在系统浏览器打开充值页面；支付在浏览器中完成。");
    } catch (error) {
      setNotice(`打开充值页失败：${String(error)}`);
    } finally {
      setBusy(null);
    }
  };

  const installed = runtime?.installed === true;
  const prepared = runtime?.prepared === true;
  const running = runtime?.running === true;
  const disabled = busy !== null;

  return (
    <main className="min-h-screen bg-slate-950 px-6 py-10 text-slate-100">
      <section className="mx-auto max-w-2xl rounded-3xl border border-emerald-400/30 bg-slate-900 p-8 shadow-2xl">
        <p className="text-sm font-medium text-emerald-300">U-King · OpenClaw 绿色预览</p>
        <h1 className="mt-2 text-3xl font-semibold">随包运行的 OpenClaw</h1>
        <p className="mt-3 text-sm leading-6 text-slate-300">配置、钱包、运行状态和 WebView 数据都保存在当前绿色包内。充值会打开系统浏览器，不会自动支付。</p>

        <div className="mt-6 grid gap-3 sm:grid-cols-3">
          <Status label="运行时" value={installed ? "已就绪" : "待检查"} good={installed} />
          <Status label="配置" value={prepared ? "已准备" : "未准备"} good={prepared} />
          <Status label="网关" value={running ? "运行中" : "未启动"} good={running} />
        </div>

        <div className="mt-6 rounded-2xl bg-slate-800/80 p-5">
          <div className="flex items-center justify-between gap-3">
            <div><p className="font-medium">设备钱包</p><p className="mt-1 text-sm text-slate-300">{wallet?.balance?.text || "余额暂不可用"}</p></div>
            <button className="rounded-lg border border-emerald-300/60 px-4 py-2 text-sm text-emerald-200 disabled:opacity-50" disabled={disabled} onClick={() => void openRecharge()} data-action-id="portable.recharge">一键充值</button>
          </div>
        </div>

        <div className="mt-6 grid gap-3 sm:grid-cols-2">
          <ActionButton disabled={disabled || !installed} onClick={() => void configure()} id={ACTION.configure}>一键配置</ActionButton>
          <ActionButton disabled={disabled || !prepared} onClick={() => void run("启动", ACTION.launch)} id={ACTION.launch}>一键启动</ActionButton>
          <ActionButton disabled={disabled || !running} onClick={() => void run("进入", "runtime.openclaw2.open_dashboard")} id="runtime.openclaw2.open_dashboard">进入 OpenClaw</ActionButton>
          <ActionButton disabled={disabled || !running} onClick={() => void run("停止", ACTION.stop)} id={ACTION.stop}>停止</ActionButton>
        </div>
        <p className="mt-6 min-h-12 rounded-xl bg-slate-800 px-4 py-3 text-sm leading-6 text-slate-200" aria-live="polite">{notice}</p>
      </section>
    </main>
  );

  async function configure() {
    setBusy("配置");
    try {
      const prepared = await call(ACTION.prepare, {}, true);
      if (!prepared.ok) throw new Error(prepared.error?.message || "准备私有配置失败");
      const configured = await call(ACTION.configure, { provider_id: "xiapan" }, true);
      if (!configured.ok) throw new Error(configured.error?.message || "配置失败");
      await refresh();
      setNotice("配置完成，未执行模型探针或扣费调用。");
    } catch (error) {
      setNotice(`配置失败：${String(error)}`);
    } finally {
      // Offline wallet issuance can fail after the private profile was already
      // prepared. Preserve that local fact so Start remains available.
      await refresh().catch(() => {});
      setBusy(null);
    }
  }
}

async function call(action_id: string, input: Record<string, unknown>, confirmed: boolean): Promise<ActionEnvelope> {
  return invoke<ActionEnvelope>("action_parity_call", {
    request: { action_id, input, confirmed, surface: "desktop" },
  });
}

function Status({ label, value, good }: { label: string; value: string; good: boolean }) {
  return <div className="rounded-xl bg-slate-800 p-4"><p className="text-xs text-slate-400">{label}</p><p className={good ? "mt-1 font-medium text-emerald-300" : "mt-1 font-medium text-warning-700 dark:text-warning-400"}>{value}</p></div>;
}

function ActionButton({ children, disabled, onClick, id }: { children: string; disabled: boolean; onClick: () => void; id: string }) {
  return <button className="rounded-xl bg-emerald-500 px-4 py-3 font-medium text-slate-950 transition hover:bg-emerald-400 disabled:cursor-not-allowed disabled:opacity-40" disabled={disabled} onClick={onClick} data-action-id={id}>{children}</button>;
}
