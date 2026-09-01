/**
 * 一键备份 / 还原 —— 把 ClawX 的对话 + 设置（含 OpenClaw 龙虾工作区）快照到 U 盘，
 * 换台电脑（家 ↔ 办公室）一键拉回接着干活。
 *
 * 为什么是「整份替换」不是「智能合并」：ClawX 对话落在 Electron leveldb / OpenClaw 的
 * state.db，数据库不能跨机按文件合并。所以还原 = 关 ClawX → 旧目录留底 → 铺快照。
 * 还原前后端会**自动把当前状态也备份一份**（安全网，可回滚）。
 *
 * 后端：backup.rs（backup_now / list_backups / restore_backup / backup_default_root）。
 * 进度走事件 `uking:backup_progress`。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { askConfirm } from "./lib/confirm";
import { useI18n } from "./i18n";
import {
  Archive,
  Check,
  FolderOpen,
  HardDrive,
  Laptop,
  Loader2,
  RotateCcw,
  ShieldCheck,
  Upload,
} from "lucide-react";

type ManifestItem = { name: string; label: string; files: number; bytes: number };
type BackupEntry = {
  dir: string;
  machine: string;
  user: string;
  created_at: number;
  app_version: string;
  items: ManifestItem[];
  total_bytes: number;
  is_this_machine: boolean;
};
type ItemStat = { name: string; label: string; files: number; bytes: number; present: boolean };
type BackupResult = { dir: string; machine: string; created_at: number; items: ItemStat[]; total_bytes: number };
type RestoreResult = { items: ItemStat[]; pre_backup_dir: string | null; archived: string[] };

function fmtBytes(n: number): string {
  if (n <= 0) return "0 B";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${u[i]}`;
}

function fmtTime(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  const p = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function Backup({ onToast }: { onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [root, setRoot] = useState<string>("");
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [busy, setBusy] = useState<null | "backup" | "restore">(null);
  const [progress, setProgress] = useState<string>("");
  const logRef = useRef<string>("");

  const refresh = useCallback(async (r: string) => {
    if (!r) return;
    const list = await invoke<BackupEntry[]>("list_backups", { root: r }).catch(() => [] as BackupEntry[]);
    setBackups(list);
  }, []);

  // 首次：拿默认盘 + 列已有快照
  useEffect(() => {
    let alive = true;
    (async () => {
      const def = await invoke<string>("backup_default_root").catch(() => "");
      if (!alive) return;
      setRoot(def);
      void refresh(def);
    })();
    return () => {
      alive = false;
    };
  }, [refresh]);

  // 进度事件
  useEffect(() => {
    const un = listen<string>("uking:backup_progress", (e) => setProgress(e.payload));
    return () => {
      void un.then((f) => f());
    };
  }, []);

  const pickFolder = useCallback(async () => {
    const picked = await openDialog({ directory: true, defaultPath: root || undefined, title: t("选择备份位置（U 盘根目录）") }).catch(
      () => null,
    );
    if (typeof picked === "string") {
      setRoot(picked);
      void refresh(picked);
    }
  }, [root, refresh, t]);

  const doBackup = useCallback(async () => {
    if (!root || busy) return;
    setBusy("backup");
    setProgress(t("准备备份…"));
    logRef.current = "";
    try {
      const r = await invoke<BackupResult>("backup_now", { destRoot: root });
      const kept = r.items.filter((i) => i.present);
      onToast(
        kept.length
          ? t("已备份 {items}（{size}）到 U 盘", { items: kept.map((i) => i.label).join("、"), size: fmtBytes(r.total_bytes) })
          : t("本机暂无 ClawX / 龙虾数据可备份"),
      );
      await refresh(root);
    } catch (e) {
      onToast(t("备份失败：") + String(e));
    } finally {
      setBusy(null);
      setProgress("");
    }
  }, [root, busy, onToast, refresh, t]);

  const doRestore = useCallback(
    async (entry: BackupEntry) => {
      if (busy) return;
      const ok = await askConfirm(
        t(
          "从「{machine}」{time} 的快照还原到本机？\n\n· 会先关闭 ClawX\n· 本机当前的 ClawX 对话/设置会被这份快照整份替换\n· 替换前会自动把本机当前状态也备份一份（可回滚），旧数据另存为 .uking-bak\n\n确定继续吗？",
          { machine: entry.machine, time: fmtTime(entry.created_at) },
        ),
      );
      if (!ok) return;
      setBusy("restore");
      setProgress(t("准备还原…"));
      try {
        const r = await invoke<RestoreResult>("restore_backup", { backupDir: entry.dir });
        const n = r.items.filter((i) => i.present).length;
        onToast(
          n
            ? t("已还原 {n} 项。请重新打开 ClawX 查看", { n }) + (r.pre_backup_dir ? t("（本机原状态已自动备份）") : "")
            : t("这份快照里没有可还原的数据"),
        );
      } catch (e) {
        onToast(t("还原失败：") + String(e));
      } finally {
        setBusy(null);
        setProgress("");
      }
    },
    [busy, onToast, t],
  );

  return (
    <div className="space-y-5">
      {/* 头 */}
      <div>
        <h1 className="text-[18px] font-semibold text-ink-0 flex items-center gap-2">
          <HardDrive size={20} className="text-accent" />
          {t("备份 / 同步到 U 盘")}
        </h1>
        <p className="text-[12px] text-ink-3 mt-1">
          {t("把 ClawX 的对话和设置（含龙虾工作区）存到 U 盘，回家插上一键还原，接着干活。")}
        </p>
      </div>

      {/* 备份位置 + 立即备份 */}
      <div className="rounded-card border border-white/[0.06] bg-bg-1 p-4 space-y-3">
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="text-[11px] text-ink-4">{t("备份位置")}</div>
            <div className="text-[13px] text-ink-1 font-mono truncate" title={root}>
              {root || t("（探测中…）")}
            </div>
          </div>
          <button
            onClick={pickFolder}
            disabled={!!busy}
            className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-card text-[12px] text-ink-2 border border-white/[0.08] hover:bg-white/[0.04] disabled:opacity-50"
          >
            <FolderOpen size={14} />
            {t("换位置")}
          </button>
        </div>

        <button
          data-action-id="runtime.backup.create"
          onClick={doBackup}
          disabled={!root || !!busy}
          className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-card text-[14px] font-medium bg-accent text-black hover:bg-accent-400 disabled:opacity-50 transition-colors"
        >
          {busy === "backup" ? <Loader2 size={16} className="animate-spin" /> : <Upload size={16} />}
          {busy === "backup" ? t("备份中…") : t("立即备份到 U 盘")}
        </button>

        {busy && progress && (
          <div className="text-[12px] text-ink-3 flex items-center gap-1.5">
            <Loader2 size={12} className="animate-spin" />
            {progress}
          </div>
        )}

        {/* 安全说明 */}
        <div className="flex items-start gap-2 text-[11px] text-ink-4 pt-1 border-t border-white/[0.06]">
          <ShieldCheck size={14} className="text-emerald-400/80 shrink-0 mt-0.5" />
          <span>
            {t("还原采用「整份替换 + 自动留底」：换电脑还原前会先把本机当前状态也备份一份，旧数据另存为")}
            <span className="font-mono"> .uking-bak</span>
            {t("，不会凭空丢。ClawX 的对话存在数据库里，无法逐条合并，故只能整份覆盖。")}
          </span>
        </div>
      </div>

      {/* 已有快照 */}
      <div>
        <div className="text-[13px] font-medium text-ink-1 flex items-center gap-2 mb-2">
          <Archive size={15} className="text-ink-3" />
          {t("U 盘上的备份")}
          <span className="text-[11px] text-ink-5">{t("（{n}）", { n: backups.length })}</span>
        </div>

        {backups.length === 0 ? (
          <div className="rounded-card border border-dashed border-white/[0.08] p-6 text-center text-[12px] text-ink-4">
            {t("这个位置还没有备份。点上面「立即备份到 U 盘」创建第一份。")}
          </div>
        ) : (
          <div className="space-y-2">
            {backups.map((b) => (
              <div
                key={b.dir}
                className="rounded-card border border-white/[0.06] bg-bg-1 p-3.5 flex items-center justify-between gap-3"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2 text-[13px] text-ink-1">
                    <Laptop size={14} className="text-ink-3 shrink-0" />
                    <span className="font-medium truncate">{b.machine}</span>
                    {b.is_this_machine && (
                      <span className="text-[10px] px-1.5 py-0.5 rounded bg-accent/[0.14] text-accent shrink-0">{t("本机")}</span>
                    )}
                  </div>
                  <div className="text-[11px] text-ink-4 mt-1">
                    {fmtTime(b.created_at)} · {fmtBytes(b.total_bytes)} ·{" "}
                    {b.items.map((i) => i.label).join("、") || t("（空）")}
                    {b.app_version ? ` · v${b.app_version}` : ""}
                  </div>
                </div>
                <button
                  data-action-id="runtime.backup.restore"
                  onClick={() => doRestore(b)}
                  disabled={!!busy}
                  className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-card text-[12px] font-medium text-ink-1 border border-white/[0.1] hover:bg-white/[0.05] disabled:opacity-50"
                >
                  {busy === "restore" ? <Loader2 size={14} className="animate-spin" /> : <RotateCcw size={14} />}
                  {t("还原到本机")}
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 跨机说明 */}
      <div className="rounded-card border border-white/[0.06] bg-white/[0.02] p-3.5 text-[11px] text-ink-4 space-y-1.5">
        <div className="text-ink-2 font-medium flex items-center gap-1.5">
          <Check size={13} className="text-accent" />
          {t("办公室 ↔ 家里 怎么用")}
        </div>
        <div>{t("1. 办公室干完活 → 这里「立即备份到 U 盘」 → 拔盘带走")}</div>
        <div>{t("2. 回家插上 U 盘开 U-King → 找到办公室那条快照 → 「还原到本机」 → 打开 ClawX 接着用")}</div>
        <div className="text-ink-5">
          {t("提示：快照里带的是本机虾盘云 Key，换机还原后会用同一个钱包计费 —— 单人多机正好省事。")}
        </div>
      </div>
    </div>
  );
}
