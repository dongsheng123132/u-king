/**
 * 自带技能包清单 —— **装哪些由用户自己定**。
 *
 * 🔴 立项理由（客户 2026-08-18）：「有人抱怨我们安装了太多预制 skill，还无法删除。」
 * 那时确实只有装没有拆：16 个包一次性铺进 `~/.claude/skills` 等落点，
 * 动作表里跟 skill 相关的只有一条 install，界面上一个逐包开关都没有。
 *
 * 🔴 **挂在「AI 专家」那一屏**（`opencodex/ExpertGallery.tsx`），不在独立的「AI 技能」页 ——
 * 用户原话：「ai技能 删除吧，就是一个 skillhub，ai专家，不就是吗？合并留到 uchat」。
 * 按「舞台 / 演员」的口径这是对的：技能是**演员带的本事**，跟演员在同一个招聘现场，
 * 不该另开一个页面（专家墙 + 技能清单 + skillhub 入口，三件事一屏看完）。
 *
 * 独立成文件是为了「一份实现」：以后哪里还要这张表，import 它，别再抄一份。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Download, Loader2, Trash2 } from "lucide-react";
import { ACTION, createTauriActionClient } from "../generated/action-client";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";

/** 走通用影核通道，不为技能包再开 tauri command。 */
const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

type PackRow = { name: string; what: string; installed: boolean; dirs: number };

/**
 * 自带技能包清单 —— **装哪些由用户自己定**。
 *
 * 🔴 立项理由（客户 2026-08-18）：「有人抱怨我们安装了太多预制 skill，还无法删除。」
 * 那时确实只有装没有拆：16 个包一次性铺进 `~/.claude/skills` 等落点，
 * 动作表里跟 skill 相关的只有一条 install，界面上一个逐包开关都没有。
 *
 * 这张表就是那句「能删能装就行，用户自己定」的落点：
 *  - `installed` 判据是**磁盘上真有那个目录**，不是「我们调过 install」——
 *    调过不等于装成了（install 是 best-effort，单个失败只记日志）
 *  - 删完立刻重扫，不做乐观更新：磁盘才是真相源
 */
export function SkillPackList({ onToast }: { onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [rows, setRows] = useState<PackRow[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const env = await callAction(ACTION.RUNTIME_SKILLPACK_INSPECT, {});
    setRows(env.ok ? ((env.result as { packs?: PackRow[] }).packs ?? []) : []);
  }, []);
  useEffect(() => void refresh(), [refresh]);

  const toggle = async (r: PackRow) => {
    setBusy(r.name);
    try {
      if (r.installed) {
        await callAction(ACTION.RUNTIME_SKILLPACK_UNINSTALL, { name: r.name }, { confirmed: true });
        onToast(t("已删除 {name}", { name: r.name }));
      } else {
        await callAction(ACTION.RUNTIME_SKILLPACK_INSTALL, { name: r.name }, { confirmed: true });
        onToast(t("已装上 {name}", { name: r.name }));
      }
      await refresh();
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(null);
    }
  };

  const on = rows?.filter((r) => r.installed).length ?? 0;
  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
      <div>
        <h3 className="text-[13px] font-semibold text-ink-0">
          {t("自带技能包")}
          {rows && <span className="ml-1.5 text-[11px] font-normal text-ink-4">{t("装了 {on}/{all}", { on: String(on), all: String(rows.length) })}</span>}
        </h3>
        <p className="text-[11.5px] text-ink-4 mt-0.5">
          {t("装哪些由你定 —— 每个包都是给 AI 的一份说明书 + 脚本，不用的可以删掉，随时能装回来。")}
        </p>
      </div>
      {!rows ? (
        <div className="text-[12px] text-ink-4">{t("正在查…")}</div>
      ) : (
        <div className="space-y-1">
          {rows.map((r) => (
            <div key={r.name} className="flex items-center gap-2.5 py-1.5 border-b border-white/[0.04] last:border-0">
              <span className={cn("w-1.5 h-1.5 rounded-full shrink-0", r.installed ? "bg-success-500" : "bg-ink-5/40")} />
              <div className="min-w-0 flex-1">
                <div className="text-[12px] text-ink-1 font-mono">{r.name}</div>
                <div className="text-[11px] text-ink-4 truncate" title={r.what}>{r.what}</div>
              </div>
              <button
                onClick={() => void toggle(r)}
                disabled={busy === r.name}
                data-action-id={r.installed ? "runtime.skillpack.uninstall" : "runtime.skillpack.install"}
                className={cn(
                  "inline-flex items-center gap-1 h-7 px-2.5 rounded-md text-[11px] shrink-0 disabled:opacity-50",
                  r.installed
                    ? "border border-white/[0.10] text-ink-3 hover:text-red-400 hover:border-red-400/40"
                    : "bg-accent text-white hover:bg-accent-600",
                )}
              >
                {busy === r.name ? <Loader2 size={12} className="animate-spin" /> : r.installed ? <Trash2 size={12} /> : <Download size={12} />}
                {busy === r.name ? t("处理中…") : r.installed ? t("删除") : t("装上")}
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

