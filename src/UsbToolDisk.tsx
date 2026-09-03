/**
 * U 盘工具盘 —— 便携 AI 的唯一管理面。
 *
 * P1 的 UI 故意只接“发现 + 验证 + 启动已验证实例”。制作、更新和凭据写入须等
 * 目标身份、原子提交、失败回滚和凭据语义闭合后再开放；不能为了有按钮而把半成品
 * 写操作交给用户。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { CheckCircle2, HardDrive, Play, RefreshCw, ShieldAlert, TriangleAlert } from "lucide-react";
import { ACTION, createTauriActionClient } from "./generated/action-client";
import { askConfirm } from "./lib/confirm";

const callAction = createTauriActionClient(async (command, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(command, args);
}, { surface: "desktop:usb-tool-disk" });

type Target = { target_id: string; target_root: string; installed: boolean; picoclaw_version?: string | null; target_state_version: string };
type Inspection = { ready: boolean; blockers: string[]; targets: Target[]; launched_from_target_id?: string | null };
type Verification = { ok: boolean; blockers: string[] };

function text(error: unknown) { return error instanceof Error ? error.message : String(error); }

export function UsbToolDisk({ onToast }: { onToast?: (message: string) => void }) {
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [root, setRoot] = useState<string | null>(null);
  const [verification, setVerification] = useState<Verification | null>(null);
  const [busy, setBusy] = useState<"inspect" | "verify" | "launch" | null>(null);
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

  return <div className="max-w-5xl mx-auto space-y-5">
    <section className="rounded-card border border-accent/20 bg-gradient-to-br from-accent/[0.12] to-bg-2 px-5 py-5 shadow-card">
      <div className="flex items-start gap-3"><span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent/15"><HardDrive size={20} className="text-accent" /></span><div className="min-w-0 flex-1"><h1 className="text-[18px] font-semibold text-ink-0">U 盘工具盘</h1><p className="mt-1 text-[13px] leading-relaxed text-ink-3">在这里确认这块 U 盘上的随身 AI。PicoClaw 的程序、资料、会话和日志均属于所选 U 盘；本机 Claude Code 只是快捷调用，不会被复制或改写。</p></div><button onClick={() => void inspect()} disabled={busy !== null} className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-white/[0.10] px-3 text-[12px] text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><RefreshCw size={14} className={busy === "inspect" ? "animate-spin" : ""} />刷新</button></div>
    </section>
    {error && <div className="flex gap-2 rounded-lg border border-danger-500/30 bg-danger-500/[0.08] px-4 py-3 text-[12px] text-danger-300"><TriangleAlert size={16} className="shrink-0" />{error}</div>}
    <section className="rounded-card border border-white/[0.08] bg-bg-2 p-5"><div className="flex items-center gap-2"><HardDrive size={16} className="text-accent" /><h2 className="text-[15px] font-semibold text-ink-1">这次要使用哪一份 AI</h2></div>{!inspection ? <p className="mt-4 text-[13px] text-ink-4">正在检测可移动磁盘…</p> : inspection.targets.length === 0 ? <div className="mt-4 rounded-lg border border-dashed border-white/[0.12] px-4 py-6 text-center"><p className="text-[13px] text-ink-2">未检测到可移动 U 盘</p><p className="mt-1 text-[11.5px] text-ink-4">插入 U 盘后点“刷新”。本机 AI、Claude Code 和它们的账号不会被误当成 U 盘 AI。</p></div> : <div className="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">{inspection.targets.map((item) => <button key={item.target_root} onClick={() => { setRoot(item.target_root); setVerification(null); }} className={`rounded-xl border p-4 text-left transition-colors ${item.target_root === root ? "border-accent bg-accent/[0.09]" : "border-white/[0.08] hover:bg-white/[0.04]"}`}><div className="flex items-center justify-between gap-2"><span className="font-mono text-[14px] font-semibold text-ink-1">{item.target_root}</span><span className={item.installed ? "text-success-400 text-[11px]" : "text-ink-4 text-[11px]"}>{item.installed ? "AI 精灵已检测" : "尚未制作"}</span></div><p className="mt-1 text-[11.5px] text-ink-4">{item.installed ? `PicoClaw ${item.picoclaw_version ?? "已安装"} · 资料在此盘` : "尚无便携 AI 运行时"}</p></button>)}</div>}</section>
    {selected && <section className="rounded-card border border-white/[0.08] bg-bg-2 p-5"><div className="flex flex-wrap items-start justify-between gap-3"><div><p className="text-[11px] text-ink-4">当前选择</p><h2 className="mt-0.5 text-[16px] font-semibold text-ink-0">{selected.target_root} · U盘 AI 精灵</h2></div><span className={selected.installed ? "inline-flex items-center gap-1 rounded-full bg-success-500/15 px-2.5 py-1 text-[11px] text-success-400" : "inline-flex items-center gap-1 rounded-full bg-white/[0.06] px-2.5 py-1 text-[11px] text-ink-3"}>{selected.installed ? <CheckCircle2 size={13} /> : null}{selected.installed ? "已检测" : "未制作"}</span></div><div className="mt-4 grid gap-2 text-[12px] sm:grid-cols-3"><Info label="程序" value={`${selected.target_root}U-King\\AI-Genie`} /><Info label="资料" value={`${selected.target_root}U-King\\AI-Genie\\data`} /><Info label="AI 账号" value="状态将在凭据契约完成后显示；目前不猜测" /></div>{verification && <div className={`mt-4 flex gap-2 rounded-lg px-3 py-2.5 text-[12px] ${verification.ok ? "bg-success-500/[0.10] text-success-400" : "bg-danger-500/[0.08] text-danger-300"}`}>{verification.ok ? <CheckCircle2 size={16} className="shrink-0" /> : <ShieldAlert size={16} className="shrink-0" />}{verification.ok ? "已验证：固定 runtime、启动器和配置均属于此 U 盘。" : verification.blockers.join("；")}</div>}<div className="mt-5 flex flex-wrap gap-2">{selected.installed && <button data-action-id="runtime.usb_genie.launch" onClick={() => void launch()} disabled={busy !== null} className="inline-flex h-10 items-center gap-1.5 rounded-lg bg-accent px-4 text-[13px] font-semibold text-white hover:bg-accent-600 disabled:cursor-not-allowed disabled:opacity-50"><Play size={15} />{busy === "launch" ? "正在打开…" : `打开 U盘 AI 精灵（${selected.target_root}）`}</button>}<button data-action-id="runtime.usb_genie.verify" onClick={() => void verify()} disabled={busy !== null || !selected.installed} className="inline-flex h-10 items-center gap-1.5 rounded-lg border border-white/[0.10] px-3 text-[12px] text-ink-2 hover:bg-white/[0.06] disabled:cursor-not-allowed disabled:opacity-50"><ShieldAlert size={15} />{busy === "verify" ? "正在检查…" : "检查此 U 盘"}</button></div><p className="mt-3 text-[11.5px] leading-relaxed text-ink-4">可直接打开已验证的 AI 精灵。制作和凭据写入仍等待完整事务与回滚闸门通过；届时会在这张卡片补齐，不会新增第二套 UI。</p></section>}
    <section className="rounded-card border border-white/[0.08] bg-bg-2 px-5 py-4"><h2 className="text-[13px] font-semibold text-ink-2">后续运行时</h2><p className="mt-1 text-[12px] leading-relaxed text-ink-4">OpenClaw 与 ClawX 将使用同一目标选择、资料归属、验证和启动交互；各自没有完成便携落盘取证、配置重定向与真实 U 盘验收前，不显示为“可用”。</p></section>
  </div>;
}

function Info({ label, value }: { label: string; value: string }) { return <div className="rounded-lg border border-white/[0.06] bg-bg-0/40 px-3 py-2"><p className="text-[10px] text-ink-5">{label}</p><p className="mt-0.5 break-all font-mono text-[10.5px] text-ink-3">{value}</p></div>; }
