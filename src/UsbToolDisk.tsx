/**
 * U 盘工具盘 —— 便携 AI 的唯一管理面。
 *
 * P1 的 UI 接“发现 + 验证 + 启动已验证实例 + 移除随盘凭据（安全侧减法）”。
 * 制作只允许 credential_ref=none；「随盘凭据写入（official_device）」在真盘验收
 * 清单结清前按发布闸门禁止开放；P1 固定单一 runtime 版本，界面不提供更新入口。
 * 不能为了有按钮而把半成品写操作交给用户。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { CheckCircle2, HardDrive, KeyRound, Play, RefreshCw, ShieldAlert, TriangleAlert } from "lucide-react";
import { ACTION, createTauriActionClient } from "./generated/action-client";
import { askConfirm } from "./lib/confirm";

const callAction = createTauriActionClient(async (command, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(command, args);
}, { surface: "desktop:usb-tool-disk" });

type Target = {
  target_id: string; target_root: string; display_name: string; volume_label: string; filesystem: string;
  total_bytes: number; free_bytes: number; read_only: boolean; installed: boolean;
  picoclaw_version?: string | null; target_state_version: string;
};
type Inspection = { ready: boolean; blockers: string[]; targets: Target[]; inventory_state_version?: string; launched_from_target_id?: string | null };
type Verification = { ok: boolean; blockers: string[] };

function text(error: unknown) { return error instanceof Error ? error.message : String(error); }

export function UsbToolDisk({ onToast }: { onToast?: (message: string) => void }) {
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [root, setRoot] = useState<string | null>(null);
  const [verification, setVerification] = useState<Verification | null>(null);
  const [busy, setBusy] = useState<"inspect" | "deploy" | "verify" | "launch" | "credential_remove" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const selected = useMemo(() => inspection?.targets.find((item) => item.target_root === root) ?? null, [inspection, root]);

  const inspect = useCallback(async () => {
    setBusy("inspect");
    try {
      const envelope = await callAction(ACTION.RUNTIME_USB_GENIE_INSPECT, {});
      if (!envelope.ok) throw new Error(envelope.error?.message ?? "读取 U 盘状态失败");
      const next = envelope.result as unknown as Inspection;
      setInspection(next);
      setRoot((old) => {
        if (next.targets.some((item) => item.target_root === old)) return old;
        return next.targets.find((item) => item.target_id === next.launched_from_target_id)?.target_root ?? next.targets[0]?.target_root ?? null;
      });
      setVerification(null);
      setError(null);
    } catch (cause) {
      setInspection(null); setRoot(null); setError(text(cause));
    } finally { setBusy(null); }
  }, []);

  useEffect(() => { void inspect(); }, [inspect]);

  const verify = useCallback(async () => {
    if (!selected) return;
    setBusy("verify");
    try {
      const envelope = await callAction(ACTION.RUNTIME_USB_GENIE_VERIFY, { target_id: selected.target_id, target_root: selected.target_root });
      if (!envelope.ok) throw new Error(envelope.error?.message ?? "检查失败");
      const result = envelope.result as unknown as Verification;
      setVerification(result); setError(null);
      onToast?.(result.ok ? "此 U 盘 AI 精灵已验证" : "发现需要修复的项目");
    } catch (cause) { setError(text(cause)); }
    finally { setBusy(null); }
  }, [onToast, selected]);

  const deploy = useCallback(async () => {
    if (!selected || selected.installed) return;
    const accepted = await askConfirm(`将下载约 22 MB 的已固定 PicoClaw 运行时，并制作到 ${selected.target_root}。\n\n只会写入 U-King\\AI-Genie 和“启动 AI 精灵.cmd”；若发现同名但不属于 U-King 的内容会拒绝覆盖。此次不保存 AI 账号。`, "制作到此 U盘");
    if (!accepted) return;
    setBusy("deploy");
    try {
      const envelope = await callAction(ACTION.RUNTIME_USB_GENIE_DEPLOY, {
        target_id: selected.target_id, target_root: selected.target_root, credential_ref: "none",
        expected_state_version: inspection?.inventory_state_version,
      }, { confirmed: true });
      if (!envelope.ok) throw new Error(envelope.error?.message ?? "制作失败");
      onToast?.(`已制作 ${selected.target_root} 的 AI 精灵；请先检查再打开`);
      await inspect();
    } catch (cause) { setError(text(cause)); }
    finally { setBusy(null); }
  }, [inspection?.inventory_state_version, inspect, onToast, selected]);

  const launch = useCallback(async () => {
    if (!selected) return;
    const accepted = await askConfirm(`将打开 ${selected.target_root} 上的 AI 精灵。\n\n程序、资料和会话都留在这块 U 盘；退出 AI 后再拔盘。`, "打开 U盘 AI 精灵");
    if (!accepted) return;
    setBusy("launch");
    try {
      const envelope = await callAction(ACTION.RUNTIME_USB_GENIE_LAUNCH, { target_id: selected.target_id, target_root: selected.target_root }, { confirmed: true });
      if (!envelope.ok) throw new Error(envelope.error?.message ?? "启动失败");
      const result = envelope.result as { already_running?: boolean } | null;
      setError(null);
      onToast?.(result?.already_running ? "此 U 盘 AI 精灵已在运行" : `已打开 ${selected.target_root} 的 AI 精灵`);
    } catch (cause) { setError(text(cause)); }
    finally { setBusy(null); }
  }, [onToast, selected]);

  const removeCredential = useCallback(async () => {
    if (!selected) return;
    const accepted = await askConfirm(`将删除 ${selected.target_root} 上 AI 精灵的随盘凭据文件（data\\.security.yml）。\n\n这只删除本盘上的凭据文件，不能吊销已被复制的 key；如果 U 盘丢失，请到 U-King 轮换原 key。`, "移除此盘凭据");
    if (!accepted) return;
    setBusy("credential_remove");
    try {
      const envelope = await callAction(ACTION.RUNTIME_USB_GENIE_CREDENTIAL_REMOVE, { target_id: selected.target_id, target_root: selected.target_root }, { confirmed: true });
      if (!envelope.ok) throw new Error(envelope.error?.message ?? "移除失败");
      const result = envelope.result as { removed?: boolean } | null;
      setError(null);
      onToast?.(result?.removed ? "已移除此盘凭据" : "此盘没有随盘凭据，无需移除");
    } catch (cause) { setError(text(cause)); }
    finally { setBusy(null); }
  }, [onToast, selected]);

  return <div className="max-w-5xl mx-auto space-y-5">
    <section className="rounded-card border border-accent/20 bg-gradient-to-br from-accent/[0.12] to-bg-2 px-5 py-5 shadow-card">
      <div className="flex items-start gap-3"><span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent/15"><HardDrive size={20} className="text-accent" /></span><div className="min-w-0 flex-1"><h1 className="text-[18px] font-semibold text-ink-0">U 盘工具盘</h1><p className="mt-1 text-[13px] leading-relaxed text-ink-3">在这里确认这块 U 盘上的随身 AI。PicoClaw 的程序、资料、会话和日志均属于所选 U 盘；本机 Claude Code 只是快捷调用，不会被复制或改写。</p></div><button onClick={() => void inspect()} disabled={busy !== null} className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-white/[0.10] px-3 text-[12px] text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><RefreshCw size={14} className={busy === "inspect" ? "animate-spin" : ""} />刷新</button></div>
    </section>
    {error && <div className="flex gap-2 rounded-lg border border-danger-500/30 bg-danger-500/[0.08] px-4 py-3 text-[12px] text-danger-300"><TriangleAlert size={16} className="shrink-0" />{error}</div>}
    <section className="rounded-card border border-white/[0.08] bg-bg-2 p-5"><div className="flex items-center gap-2"><HardDrive size={16} className="text-accent" /><h2 className="text-[15px] font-semibold text-ink-1">这次要使用哪一份 AI</h2></div>{!inspection ? <p className="mt-4 text-[13px] text-ink-4">正在检测可移动磁盘…</p> : inspection.targets.length === 0 ? <div className="mt-4 rounded-lg border border-dashed border-white/[0.12] px-4 py-6 text-center"><p className="text-[13px] text-ink-2">未检测到可移动 U 盘</p><p className="mt-1 text-[11.5px] text-ink-4">插入 U 盘后点“刷新”。本机 AI、Claude Code 和它们的账号不会被误当成 U 盘 AI。</p></div> : <div className="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">{inspection.targets.map((item) => <button key={item.target_root} onClick={() => { setRoot(item.target_root); setVerification(null); }} className={`rounded-xl border p-4 text-left transition-colors ${item.target_root === root ? "border-accent bg-accent/[0.09]" : "border-white/[0.08] hover:bg-white/[0.04]"}`}><div className="flex items-center justify-between gap-2"><span className="min-w-0 truncate text-[14px] font-semibold text-ink-1">{item.volume_label || item.target_root}</span><span className={item.installed ? "shrink-0 text-success-400 text-[11px]" : "shrink-0 text-ink-4 text-[11px]"}>{item.installed ? "AI 精灵已检测" : "尚未制作"}</span></div><p className="mt-1 font-mono text-[11px] text-ink-4">{item.target_root} · {item.filesystem || "未知格式"} · 剩余 {formatBytes(item.free_bytes)}</p><p className="mt-1 text-[11.5px] text-ink-4">{item.installed ? `PicoClaw ${item.picoclaw_version ?? "已安装"} · 资料在此盘` : "尚无便携 AI 运行时"}</p></button>)}</div>}</section>
    {selected && <section className="rounded-card border border-white/[0.08] bg-bg-2 p-5"><div className="flex flex-wrap items-start justify-between gap-3"><div><p className="text-[11px] text-ink-4">当前选择</p><h2 className="mt-0.5 text-[16px] font-semibold text-ink-0">{selected.target_root} · U盘 AI 精灵</h2></div><span className={selected.installed ? "inline-flex items-center gap-1 rounded-full bg-success-500/15 px-2.5 py-1 text-[11px] text-success-400" : "inline-flex items-center gap-1 rounded-full bg-white/[0.06] px-2.5 py-1 text-[11px] text-ink-3"}>{selected.installed ? <CheckCircle2 size={13} /> : null}{selected.installed ? "已检测" : "未制作"}</span></div><div className="mt-4 grid gap-2 text-[12px] sm:grid-cols-3"><Info label="程序" value={`${selected.target_root}U-King\\AI-Genie`} /><Info label="资料" value={`${selected.target_root}U-King\\AI-Genie\\data`} /><Info label="AI 账号" value="首次制作不保存；随盘凭据尚未开放" /></div>{verification && <div className={`mt-4 flex gap-2 rounded-lg px-3 py-2.5 text-[12px] ${verification.ok ? "bg-success-500/[0.10] text-success-400" : "bg-danger-500/[0.08] text-danger-300"}`}>{verification.ok ? <CheckCircle2 size={16} className="shrink-0" /> : <ShieldAlert size={16} className="shrink-0" />}{verification.ok ? "已验证：固定 runtime、启动器和配置均属于此 U 盘。" : verification.blockers.join("；")}</div>}<div className="mt-5 flex flex-wrap gap-2">{!selected.installed && <button data-action-id="runtime.usb_genie.deploy" onClick={() => void deploy()} disabled={busy !== null || selected.read_only || selected.filesystem.toUpperCase() === "FAT32"} className="inline-flex h-10 items-center gap-1.5 rounded-lg bg-accent px-4 text-[13px] font-semibold text-white hover:bg-accent-600 disabled:cursor-not-allowed disabled:opacity-50"><HardDrive size={15} />{busy === "deploy" ? "正在制作…" : `制作到此 U盘（${selected.target_root}）`}</button>}{selected.installed && <button data-action-id="runtime.usb_genie.launch" onClick={() => void launch()} disabled={busy !== null} className="inline-flex h-10 items-center gap-1.5 rounded-lg bg-accent px-4 text-[13px] font-semibold text-white hover:bg-accent-600 disabled:cursor-not-allowed disabled:opacity-50"><Play size={15} />{busy === "launch" ? "正在打开…" : `打开 U盘 AI 精灵（${selected.target_root}）`}</button>}<button data-action-id="runtime.usb_genie.verify" onClick={() => void verify()} disabled={busy !== null || !selected.installed} className="inline-flex h-10 items-center gap-1.5 rounded-lg border border-white/[0.10] px-3 text-[12px] text-ink-2 hover:bg-white/[0.06] disabled:cursor-not-allowed disabled:opacity-50"><ShieldAlert size={15} />{busy === "verify" ? "正在检查…" : "检查此 U 盘"}</button><button data-action-id="runtime.usb_genie.credential_remove" onClick={() => void removeCredential()} disabled={busy !== null} className="inline-flex h-10 items-center gap-1.5 rounded-lg border border-danger-500/30 px-3 text-[12px] text-danger-300 hover:bg-danger-500/[0.08] disabled:cursor-not-allowed disabled:opacity-50"><KeyRound size={15} />{busy === "credential_remove" ? "正在移除…" : "移除此盘凭据"}</button></div><p className="mt-3 text-[11.5px] leading-relaxed text-ink-4">{selected.filesystem.toUpperCase() === "FAT32" ? "此盘为 FAT32，尚未通过原子提交验收；请换用 NTFS 或 exFAT，当前未写入。" : "首次制作会先下载并校验固定 runtime，再写入此盘的受管目录；不会格式化或扫描其它文件。P1 界面不提供更新入口（固定单一 runtime 版本）；随盘凭据写入待真盘验收结清后开放。"}</p></section>}
    <section className="rounded-card border border-white/[0.08] bg-bg-2 px-5 py-4"><h2 className="text-[13px] font-semibold text-ink-2">后续运行时</h2><p className="mt-1 text-[12px] leading-relaxed text-ink-4">OpenClaw 与 ClawX 将使用同一目标选择、资料归属、验证和启动交互；各自没有完成便携落盘取证、配置重定向与真实 U 盘验收前，不显示为“可用”。</p></section>
  </div>;
}

function Info({ label, value }: { label: string; value: string }) { return <div className="rounded-lg border border-white/[0.06] bg-bg-0/40 px-3 py-2"><p className="text-[10px] text-ink-5">{label}</p><p className="mt-0.5 break-all font-mono text-[10.5px] text-ink-3">{value}</p></div>; }
function formatBytes(value: number) { return value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(1)} GB` : `${Math.round(value / 1024 ** 2)} MB`; }
