/**
 * 侧栏的「小程序」动态区 —— 装了才出现，一个没装就是零行。
 *
 * 为什么单开这一区（2026-08-18）：小程序运行时（`miniapp.rs`）一直活着，2026-08-11
 * 「第三刀」删掉的是**商店页**，不是能力。结果开发机上装着 4 个小程序、正往影核动作表里
 * 注册 4 个动作（`app.imagefix.*` / `app.idcard.*` / `app.resize.*`），而 GUI 里
 * **一个入口都没有** —— 用户既看不见也删不掉。这跟客户抱怨的「预制 skill 删不掉」
 * 是同一个病在另一层：**能力和入口是两个开关，只拉了一个。**
 *
 * 🔴 它和上面 CORE/更多/实验室 三组的区别不是「更低频」，是**这些不是我们的**。
 * 那三组是舞台基建（编译进 exe，删不掉也不该删），这一区是用户自己装的演员。
 * 混在一起，用户就没法判断「删了会不会把 U-King 弄坏」。
 *
 * 点击开的是**独立窗口**，不是浮层：主窗口在 http 源上，iframe 指向 `uking://` 属跨 scheme，
 * WebView2 直接不放行（0.9.72 就是这么全量翻车的，见 lib.rs::open_miniapp 的注释）。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, Loader2, Puzzle, RotateCcw, Trash2, TriangleAlert, Wand2 } from "lucide-react";
import { ACTION, createTauriActionClient } from "../generated/action-client";
import { useI18n } from "../i18n";
import { askConfirm } from "../lib/confirm";
import { cn } from "../lib/cn";
import { copyToClipboard } from "../lib/clipboard";

const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

type App = {
  id: string;
  name: string;
  version: string;
  accent?: string | null;
  enabled: boolean;
  dev: boolean;
  actions: string[];
};
type Broken = { dir: string; error: string };

/** 开放规范仓库（小程序格式是 Apache-2.0 的，谁都能做、能打包 .ukapp 发给别人）。 */
const SPEC_REPO = "https://github.com/dongsheng123132/uking-miniapp";
/** 从 0.9.94 删掉的商店页里原样取回（`MiniApps.tsx::AI_PROMPT`），一字未改。 */
const MAKE_ONE_PROMPT = `帮我做一个 U-King 小程序：__在这里说清楚要做什么，比如「批量给图片加水印」__

开放规范、可直接抄的示例都在 ${SPEC_REPO}
先读 skill/uking-miniapp/SKILL.md，照它做（里面有起骨架的命令和四个能跑的例子）。
做完用 scripts/install-local.mjs 装到我本机，我在 U-King 侧栏的「小程序」里就能看到。`;

export function SidebarMiniApps({
  compact,
  onToast,
}: {
  compact: boolean;
  onToast?: (s: string) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [apps, setApps] = useState<App[]>([]);
  const [broken, setBroken] = useState<Broken[]>([]);
  const [bundled, setBundled] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const env = await callAction(ACTION.RUNTIME_MINIAPP_INSPECT, {});
      const r = (env.ok ? env.result : null) as
        | { apps?: App[]; broken?: Broken[]; bundled?: number }
        | null;
      setApps(r?.apps ?? []);
      setBroken(r?.broken ?? []);
      setBundled(r?.bundled ?? 0);
    } catch {
      // 读不出来就当没有 —— 侧栏不该因为一个可选区块报错而整栏挂掉
      setApps([]);
      setBroken([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const remove = useCallback(
    async (a: App) => {
      const ok = await askConfirm(
        t("删掉小程序「{name}」？它注册的 {n} 个动作会同时从动作表里消失（AI 也就调不到了）。你的文件不会被删，随时可以「补装内置」装回来。", {
          name: a.name,
          n: a.actions.length,
        }),
        t("删除小程序"),
      );
      if (!ok) return;
      setBusy(a.id);
      try {
        // 签字走**信封层**（`confirmed`），不是塞进入参 —— 适配器再翻成核心要的 `confirm`。
        // 核心强制，不是 GUI 的礼貌：CLI / MCP / 远端影子进来一样被 confirmation_required 拦下。
        const env = await callAction(ACTION.RUNTIME_MINIAPP_UNINSTALL, { id: a.id }, { confirmed: true });
        if (!env.ok) throw new Error(env.error?.message ?? "failed");
        onToast?.(t("已删掉「{name}」", { name: a.name }));
        await load();
      } catch (e) {
        onToast?.(String(e));
      } finally {
        setBusy(null);
      }
    },
    [load, onToast, t],
  );

  const restore = useCallback(async () => {
    setBusy("__restore__");
    try {
      const env = await callAction(ACTION.RUNTIME_MINIAPP_RESTORE, {}, { confirmed: true });
      if (!env.ok) throw new Error(env.error?.message ?? "failed");
      const n = (env.result as { restored?: string[] } | null)?.restored?.length ?? 0;
      onToast?.(n > 0 ? t("补装了 {n} 个内置小程序", { n }) : t("内置小程序都在，没什么要补的"));
      await load();
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy(null);
    }
  }, [load, onToast, t]);

  // 装的比内置的少 = 用户删过内置的，给一条回头路。删除若是单向门，人就不敢删。
  const missing = Math.max(0, bundled - apps.filter((a) => !a.dev).length);

  // 一个都没有、也没坏的、也没得补 → 整区不渲染。**空分组是纯噪音**，
  // 而侧栏可见条目是有预算的（8 条），一个常驻的空标题就白占一条。
  if (apps.length === 0 && broken.length === 0 && missing === 0) return null;

  return (
    <>
      <div className={cn("border-t border-white/[0.06]", compact ? "pt-1 mt-0.5" : "pt-2 mt-1")} />
      <button
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "w-full flex items-center gap-3 px-3 rounded-card text-left transition-colors",
          compact ? "py-1.5" : "py-2",
          open ? "bg-white/[0.03] text-ink-1" : "text-ink-3 hover:bg-white/[0.03] hover:text-ink-1",
        )}
      >
        <Puzzle size={16} className={open ? "text-accent" : "text-ink-4"} />
        <span className="text-[12px] font-medium flex-1">{t("小程序")}</span>
        {broken.length > 0 && (
          <TriangleAlert size={12} className="text-warning-700 dark:text-warning-400" />
        )}
        <span className="text-[10px] text-ink-5">{apps.length}</span>
        <ChevronDown size={14} className={cn("text-ink-4 transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <>
          {/* 🔴 用 ink-3 不是 ink-5：这句是**后果告知**（删掉会连带失去哪些能力），
              不是装饰。实测浅色主题下 ink-5 对比度 **1.48:1** —— 那一档是「几乎看不见」，
              放在那儿等于没说，而「说了但没人看得见」比不说更坏：自己以为交代过了。
              旁边「实验室」那行风险告知犯的是同一个错（见 Sidebar.tsx）。 */}
          <p className={cn("px-3 text-[10.5px] text-ink-3", compact ? "py-0.5 leading-snug" : "py-1.5 leading-relaxed")}>
            {t("你自己装的，能删。删掉它注册的动作也会一起从 AI 那儿消失。")}
          </p>

          {apps.map((a) => (
            <div key={a.id} className="group flex items-center gap-2 pl-3 pr-1.5 rounded-card hover:bg-white/[0.03]">
              <button
                onClick={() => void invoke("open_miniapp", { id: a.id }).catch((e) => onToast?.(String(e)))}
                className={cn("flex items-center gap-2.5 flex-1 min-w-0 text-left", compact ? "py-1.5" : "py-2")}
                title={`${a.name} v${a.version}`}
              >
                {/* 图标用色点不用它自带的 icon.png —— 那文件在 uking:// 里，
                    主窗口跨 scheme 拿不到（和 iframe 那条是同一个限制） */}
                <span
                  className="w-2 h-2 rounded-full shrink-0"
                  style={{ background: a.accent || "#7c5cff", opacity: a.enabled ? 1 : 0.35 }}
                />
                <span className={cn("text-[12px] truncate", a.enabled ? "text-ink-2" : "text-ink-5 line-through")}>
                  {a.name}
                </span>
                {a.dev && <span className="text-[9px] px-1 rounded bg-white/[0.06] text-ink-5 shrink-0">dev</span>}
              </button>
              <button
                onClick={() => void remove(a)}
                disabled={busy === a.id}
                title={t("删掉这个小程序")}
                className="opacity-0 group-hover:opacity-100 focus:opacity-100 p-1 rounded text-ink-5 hover:text-danger-500 transition-opacity shrink-0"
              >
                {busy === a.id ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
              </button>
            </div>
          ))}

          {/* 目录在、清单读不出的。**不显示 = 用户永远查不出「为什么它的功能不见了」** ——
              界面上「装坏了」和「没装过」长得一模一样，这一行就是把它们区分开的唯一地方。 */}
          {broken.map((b) => (
            <div key={b.dir} className="px-3 py-1.5 text-[10.5px] text-warning-700 dark:text-warning-400 leading-relaxed">
              {t("装着但读不出：{err}", { err: b.error })}
            </div>
          ))}

          {/* 「做一个自己的小程序」——**从 0.9.94 删掉的商店页里捞回来的**
              （客户 2026-08-18：「里边原来还有小程序的构建方法，复制，进去 uchat 就能做一个出来」）。
              🔴 当初连页面一起删掉的时候，把这块也带走了 —— 而它跟商店页不是一回事：
              商店页是「装别人的」，这块是「做自己的」，后者不依赖任何货架。
              原文一字不改地从 git 历史取回（`MiniApps.tsx` 的 AI_PROMPT）。 */}
          <button
            onClick={() =>
              void copyToClipboard(MAKE_ONE_PROMPT).then((ok) =>
                onToast?.(ok ? t("已复制，粘给 U-Chat 里的 AI 就行") : t("复制失败，请手动选中复制")),
              )
            }
            className={cn("w-full flex items-center gap-2.5 px-3 rounded-card text-left text-ink-3 hover:text-ink-1 hover:bg-white/[0.03]", compact ? "py-1.5" : "py-2")}
          >
            <Wand2 size={13} className="shrink-0" />
            <span className="text-[11.5px]">{t("做一个自己的小程序（复制给 AI）")}</span>
          </button>

          {missing > 0 && (
            <button
              onClick={() => void restore()}
              disabled={busy === "__restore__"}
              className={cn("w-full flex items-center gap-2.5 px-3 rounded-card text-left text-ink-3 hover:text-ink-1 hover:bg-white/[0.03]", compact ? "py-1.5" : "py-2")}
            >
              {busy === "__restore__" ? (
                <Loader2 size={13} className="animate-spin shrink-0" />
              ) : (
                <RotateCcw size={13} className="shrink-0" />
              )}
              <span className="text-[11.5px]">{t("补装内置（少了 {n} 个）", { n: missing })}</span>
            </button>
          )}
        </>
      )}
    </>
  );
}
