import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type ActionEnvelope = { ok: boolean; result?: Record<string, unknown>; error?: { message?: string } };
type WalletStatus = {
  masked_key: string; balance: { text?: string } | null; charged: boolean; low_balance: boolean;
  wallet_id: string; legacy_balance_unrecoverable: boolean;
};
type PortableContext = { portable: boolean; package_root?: string; data_root?: string; openclaw_root?: string };

const ACTION = {
  inspect: "runtime.openclaw2.inspect", prepare: "runtime.openclaw2.prepare",
  configure: "runtime.openclaw2.configure_model_no_probe", launch: "runtime.openclaw2.launch",
  stop: "runtime.openclaw2.stop", dashboard: "runtime.openclaw2.open_dashboard",
  walletStatus: "runtime.device.wallet.status", walletRecharge: "runtime.device.wallet.recharge",
  walletCopyBackup: "runtime.device.wallet.copy_backup", walletAdopt: "runtime.device.key_adopt",
  walletRotate: "runtime.device.key_rotate",
} as const;

/** A narrow portable appliance. Wallet credentials never enter WebView state:
 * status is masked and backup copy is performed in the Action Core. */
export function PortableApp() {
  const [wallet, setWallet] = useState<WalletStatus | null>(null);
  const [walletError, setWalletError] = useState("");
  const [runtime, setRuntime] = useState<Record<string, unknown> | null>(null);
  const [context, setContext] = useState<PortableContext | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState("正在读取便携包状态…");
  const [existingKey, setExistingKey] = useState("");
  const walletFlight = useRef<Promise<void> | null>(null);

  const refreshRuntime = async () => {
    const result = await call(ACTION.inspect, {}, false);
    if (!result.ok) throw new Error(result.error?.message || "读取 OpenClaw 状态失败");
    setRuntime(result.result || null);
  };

  // Wallet convergence can wait on the network. Never make runtime control wait
  // for it: offline wallet issuance must not disable local Start or Configure.
  const refreshWallet = () => {
    if (walletFlight.current) return walletFlight.current;
    const work = call(ACTION.walletStatus, {}, false)
      .then((result) => {
        if (!result.ok) throw new Error(result.error?.message || "读取设备钱包失败");
        setWallet(result.result as WalletStatus);
        setWalletError("");
      })
      .catch((error) => setWalletError(String(error)))
      .finally(() => { walletFlight.current = null; });
    walletFlight.current = work;
    return work;
  };

  useEffect(() => {
    void invoke<PortableContext>("portable_context_status").then(setContext).catch(() => {});
    void refreshRuntime()
      .then(() => setNotice("零余额也可完成配置、启动和进入；模型调用前请先充值。"))
      .catch((error) => setNotice(`读取运行状态失败：${String(error)}`));
    void refreshWallet();
  }, []);

  const run = async (label: string, action: string, input: Record<string, unknown> = {}) => {
    setBusy(label);
    try {
      const result = await call(action, input, true);
      if (!result.ok) throw new Error(result.error?.message || "操作失败");
      await refreshRuntime();
      void refreshWallet();
      setNotice(`${label}完成。`);
    } catch (error) {
      setNotice(`${label}失败：${String(error)}`);
    } finally {
      setBusy(null);
    }
  };

  const configure = async () => {
    setBusy("配置");
    try {
      const prepared = await call(ACTION.prepare, {}, true);
      if (!prepared.ok) throw new Error(prepared.error?.message || "准备私有配置失败");
      const configured = await call(ACTION.configure, { provider_id: "xiapan" }, true);
      if (!configured.ok) throw new Error(configured.error?.message || "配置失败");
      await refreshRuntime();
      void refreshWallet();
      setNotice("配置完成，未执行模型探针或扣费调用。");
    } catch (error) {
      setNotice(`配置失败：${String(error)}`);
    } finally {
      setBusy(null);
    }
  };

  const adoptExistingKey = async () => {
    const key = existingKey.trim();
    if (!key) { setNotice("请先粘贴已有密钥。"); return; }
    if (!window.confirm("这会替换本机当前设备钱包。请确认你已备份当前密钥；填错的密钥不会保存。")) return;
    await run("恢复已有密钥", ACTION.walletAdopt, { key });
    setExistingKey("");
  };
  const rotateKey = async () => {
    if (!window.confirm("确定换一把密钥？旧密钥会立刻失效，余额不受影响；其它设备或脚本中的旧密钥也需要更新。")) return;
    await run("更换密钥", ACTION.walletRotate);
  };

  const installed = runtime?.installed === true;
  const prepared = runtime?.prepared === true;
  const running = runtime?.running === true;
  const disabled = busy !== null;

  return <main className="min-h-screen bg-slate-950 px-6 py-10 text-slate-100">
    <section className="mx-auto max-w-2xl rounded-3xl border border-emerald-400/30 bg-slate-900 p-8 shadow-2xl">
      <p className="text-sm font-medium text-emerald-300">U-King · OpenClaw 绿色版</p>
      <h1 className="mt-2 text-3xl font-semibold">随包运行的 OpenClaw</h1>
      <p className="mt-3 text-sm leading-6 text-slate-300">配置、钱包、运行状态和 WebView 数据都保存在当前绿色包内。充值会打开系统浏览器，不会自动支付。</p>

      <div className="mt-6 grid gap-3 sm:grid-cols-3">
        <Status label="运行时" value={installed ? "已就绪" : "待检查"} good={installed} />
        <Status label="配置" value={prepared ? "已准备" : "启动时自动准备"} good={prepared} />
        <Status label="网关" value={running ? "运行中" : "未启动"} good={running} />
      </div>

      <div className="mt-6 rounded-2xl bg-slate-800/80 p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <p className="font-medium">设备钱包</p>
            <p className="mt-1 text-sm text-slate-300">密钥：{wallet?.masked_key || "正在安全读取…"}</p>
            <p className="mt-1 text-sm text-slate-300">{wallet?.balance?.text || (walletError ? "余额暂不可用" : "正在读取余额…")}</p>
          </div>
          <button className="rounded-lg border border-emerald-300/60 px-4 py-2 text-sm text-emerald-200 disabled:opacity-50" disabled={disabled} onClick={() => void run("充值", ACTION.walletRecharge)} data-action-id="runtime.device.wallet.recharge">一键充值</button>
        </div>
        {walletError && <p className="mt-3 text-sm text-warning-700 dark:text-warning-400">钱包暂时不可用：{walletError}。本地配置与启动仍可继续。</p>}
        <p className="mt-4 text-xs leading-5 text-slate-400">密钥就是你的钱包，请自行备份；复制后请妥善保存，勿发送给陌生人。</p>
        <div className="mt-3 flex flex-wrap gap-2">
          <ActionButton disabled={disabled} onClick={() => void run("复制备份", ACTION.walletCopyBackup)} id={ACTION.walletCopyBackup}>复制密钥备份</ActionButton>
          <ActionButton disabled={disabled} onClick={() => void rotateKey()} id={ACTION.walletRotate}>换一把密钥</ActionButton>
        </div>
        <div className="mt-4 rounded-xl border border-slate-700 p-3">
          <label className="text-sm font-medium" htmlFor="portable-existing-key">恢复已有密钥</label>
          <p className="mt-1 text-xs text-slate-400">换电脑、重装或从备份找回时粘贴。会先验证，再替换本机钱包。</p>
          <div className="mt-2 flex gap-2">
            <input id="portable-existing-key" type="password" autoComplete="off" value={existingKey} onChange={(event) => setExistingKey(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-slate-600 bg-slate-950 px-3 py-2 text-sm" placeholder="sk-…" />
            <ActionButton disabled={disabled} onClick={() => void adoptExistingKey()} id={ACTION.walletAdopt}>启用</ActionButton>
          </div>
        </div>
      </div>

      <div className="mt-6 rounded-2xl border border-slate-700 bg-slate-900/60 p-4 text-xs leading-5 text-slate-300">
        <p>程序路径：<span className="break-all text-slate-100">{context?.package_root || "正在读取…"}</span></p>
        <p className="mt-1">OpenClaw 数据：<span className="break-all text-slate-100">{context?.openclaw_root || "正在读取…"}</span></p>
        <p className="mt-1">U-King 数据：<span className="break-all text-slate-100">{context?.data_root || "正在读取…"}</span></p>
      </div>

      <div className="mt-6 grid gap-3 sm:grid-cols-2">
        <ActionButton disabled={disabled || !installed} onClick={() => void configure()} id={ACTION.configure}>一键配置</ActionButton>
        <ActionButton disabled={disabled || !installed} onClick={() => void run("启动", ACTION.launch)} id={ACTION.launch}>一键启动</ActionButton>
        <ActionButton disabled={disabled || !running} onClick={() => void run("进入", ACTION.dashboard)} id={ACTION.dashboard}>进入 OpenClaw</ActionButton>
        <ActionButton disabled={disabled || !running} onClick={() => void run("停止", ACTION.stop)} id={ACTION.stop}>停止</ActionButton>
      </div>
      <p className="mt-6 min-h-12 rounded-xl bg-slate-800 px-4 py-3 text-sm leading-6 text-slate-200" aria-live="polite">{notice}</p>
    </section>
  </main>;
}

async function call(action_id: string, input: Record<string, unknown>, confirmed: boolean): Promise<ActionEnvelope> {
  return invoke<ActionEnvelope>("action_parity_call", { request: { action_id, input, confirmed, surface: "desktop" } });
}
function Status({ label, value, good }: { label: string; value: string; good: boolean }) {
  return <div className="rounded-xl bg-slate-800 p-4"><p className="text-xs text-slate-400">{label}</p><p className={good ? "mt-1 font-medium text-emerald-300" : "mt-1 font-medium text-warning-700 dark:text-warning-400"}>{value}</p></div>;
}
function ActionButton({ children, disabled, onClick, id }: { children: string; disabled: boolean; onClick: () => void; id: string }) {
  return <button className="rounded-xl bg-emerald-500 px-4 py-3 font-medium text-slate-950 transition hover:bg-emerald-400 disabled:cursor-not-allowed disabled:opacity-40" disabled={disabled} onClick={onClick} data-action-id={id}>{children}</button>;
}
