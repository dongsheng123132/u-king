/**
 * 「一键配好全部 AI」动手前的**知情与否决**弹窗（0.9.84）。
 *
 * ## 为什么要有它
 * 这个动作以前是无差别覆盖：后端探到哪些 AI 工具装了，就把虾盘云的 Key 一次性写进全部。
 * 它跟「不抢用户使用权」直接冲突 —— 客户可能就是不想让我们碰他的 Codex（那上面挂着他自己
 * 的 ChatGPT 登录），但界面从没给过他说「不」的机会，点下去才发现被改了。
 *
 * 现在动手前先摊开：**改谁、从什么改成什么、能不能不改**。默认全勾，小白多按一次确认即可；
 * 老手可以只勾他愿意给的那几个。
 *
 * ## 一个不能省的细节：已经在用自己 Key 的工具，默认不勾
 * `claude_own_key` / `codex_own_key` 是后端探到的「用户在用自己的 Key 或官方登录」。
 * 这种工具默认取消勾选并标注原因 —— 默认值就是我们的态度：**他自己配好的东西，
 * 默认不动**。他真想换过来，自己勾上就是了。
 *
 * ## 独立可插拔
 * 删本组件只动 2 处：`App.tsx`（去 import + 那段 state）和本文件。
 * 不被别的组件 import，只靠 props 通信；后端不需要任何改动（`targets` 是可选参数，
 * 不传就是老的全配行为）。
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, ShieldCheck, X } from "lucide-react";
import { ToolIcon } from "./ToolIcon";
import { useI18n } from "../i18n";

/**
 * 后端 `runtime.driver.apply_everywhere` 认的工具 id。
 *
 * 🔴 这里以前只有四个（claude/codex/clawx/hermes），而后端 `apply_xiapan_everywhere`
 * **早就会配八个**了。弹窗传上去的 `only` 名单里永远没有 pi/qwen/crush/opencode，
 * 而 `only` 只做减法 —— 于是「一键配好全部」对这四个工具**永远是空操作**：
 * 客户装了 pi，敲 `pi` 撞 google 的交互式登录，回头看界面上根本没有 pi 这一行。
 * 界面能勾的必须 ⊇ 后端会配的，否则「全部」这两个字是假的。
 */
type Target = "claude" | "codex" | "clawx" | "hermes" | "pi" | "qwen" | "crush" | "opencode" | "cline";

type Driver = {
  claude_base?: string | null;
  codex_provider?: string | null;
  clawx_installed?: boolean;
  clawx_model?: string | null;
  hermes_installed?: boolean;
  hermes_model?: string | null;
  claude_own_key?: boolean;
  codex_own_key?: boolean;
  /** 后上架那批 CLI 装没装（判据与后端 apply 用的是同一个 `tool_installed`）。 */
  extra_installed?: Record<string, boolean>;
  /** 每个工具当前生效的 provider id。 */
  active?: Record<string, string>;
} | null;

/**
 * 后上架的那批：id → 显示名 + 图标。
 *
 * 🔴 **不是**跟后端 `EXTRA_APPLY_TOOLS`（`qwen`/`crush`/`opencode` 三个）同一批 id ——
 * 这里多一个 `pi`。`pi` 已经从 `EXTRA_APPLY_TOOLS` 数组挪去了独立 `LIST_TOOLS`，但它仍靠
 * 后端 `driver_status()` 单独补的 `extra_installed`/`active` 记录出现在这个弹窗里（见
 * `providers.rs` 里 `EXTRA_APPLY_TOOLS` 上方注释）。这份列表真正对齐的是「弹窗要显示哪几行」，
 * 判据是后端 `extra_installed` 里有没有这个 key，不是某个常量数组逐字相同。
 */
const EXTRA_TOOLS: { id: Target; name: string; icon: string }[] = [
  { id: "pi", name: "pi", icon: "pi" },
  { id: "qwen", name: "Qwen Code", icon: "qwen" },
  { id: "crush", name: "Crush", icon: "crush" },
  { id: "opencode", name: "OpenCode", icon: "opencode" },
  { id: "cline", name: "Cline", icon: "cline" },
];

type Stack = Record<string, { found?: boolean } | undefined>;

type Row = {
  id: Target;
  name: string;
  icon: string;
  /** 当前是什么状态（人话，给用户看「会从什么变成什么」） */
  now: string;
  /** 默认不勾的原因；有值 = 用户自己配过，我们默认不碰 */
  ownReason?: string;
};

export function ApplyScopeDialog({
  driver,
  onCancel,
  onConfirm,
}: {
  driver: Driver;
  onCancel: () => void;
  /** 用户确认后回调，带上他勾选的工具（一定非空 —— 空了按钮是禁用的） */
  onConfirm: (targets: Target[]) => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState<Row[] | null>(null);
  const [picked, setPicked] = useState<Set<Target>>(new Set());

  useEffect(() => {
    let alive = true;
    // 「装没装」以后端为准（前端列表看走眼过：ClawX 装完才下载，列表里没它）。
    void invoke<Stack>("detect_stack")
      .catch(() => ({}) as Stack)
      .then((stack) => {
        if (!alive) return;
        const out: Row[] = [];
        if (stack.claude?.found) {
          out.push({
            id: "claude",
            name: "Claude Code",
            icon: "claude",
            now: driver?.claude_base ? t("已配：{v}", { v: driver.claude_base }) : t("未配置"),
            ownReason: driver?.claude_own_key ? t("你在用自己的 Key / 官方登录") : undefined,
          });
        }
        if (stack.codex?.found) {
          out.push({
            id: "codex",
            name: "Codex",
            icon: "openai",
            now: driver?.codex_provider ? t("已配：{v}", { v: driver.codex_provider }) : t("未配置"),
            ownReason: driver?.codex_own_key ? t("你在用自己的 Key / ChatGPT 登录") : undefined,
          });
        }
        if (driver?.clawx_installed) {
          out.push({
            id: "clawx",
            name: "ClawX / OpenClaw",
            icon: "openclaw",
            now: driver.clawx_model ? t("已配：{v}", { v: driver.clawx_model }) : t("未配置"),
          });
        }
        if (driver?.hermes_installed) {
          out.push({
            id: "hermes",
            name: "Hermes",
            icon: "hermes",
            now: driver.hermes_model ? t("已配：{v}", { v: driver.hermes_model }) : t("未配置"),
          });
        }
        // 后上架的四个。**只列真装了的** —— 一个恒为「未安装」的勾选框只会让人以为坏了
        // （同上面 ClawX/Hermes 的口径）。装没装以后端为准，跟它 apply 时用的是同一个判据。
        for (const x of EXTRA_TOOLS) {
          if (!driver?.extra_installed?.[x.id]) continue;
          const active = driver?.active?.[x.id];
          out.push({
            id: x.id,
            name: x.name,
            icon: x.icon,
            now: active ? t("已配：{v}", { v: active }) : t("未配置"),
          });
        }
        setRows(out);
        // 默认全勾，**但用户自己配过的默认不勾** —— 默认值就是态度。
        setPicked(new Set(out.filter((r) => !r.ownReason).map((r) => r.id)));
      });
    return () => {
      alive = false;
    };
  }, [driver, t]);

  const toggle = (id: Target) =>
    setPicked((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="fixed inset-0 z-[100] grid place-items-center bg-black/60 p-4" onClick={onCancel}>
      <div
        className="w-full max-w-md rounded-card border border-white/[0.10] bg-bg-2 shadow-card p-5 space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-3">
          <div>
            <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2">
              <ShieldCheck size={15} className="text-accent" /> {t("要配哪些？")}
            </h3>
            <p className="text-[11px] text-ink-4 mt-0.5">
              {t("勾上的会被写入虾盘云配置，没勾的一个字节都不动。")}
            </p>
          </div>
          <button onClick={onCancel} className="grid place-items-center w-7 h-7 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]">
            <X size={14} />
          </button>
        </header>

        {rows === null ? (
          <div className="py-6 text-center text-[12px] text-ink-4">
            <Loader2 size={15} className="animate-spin inline mr-1.5" />
            {t("正在看这台机器上装了哪些…")}
          </div>
        ) : rows.length === 0 ? (
          <div className="py-6 text-center text-[12px] text-ink-4">
            {t("这台机器上还没装 AI 工具 —— 先去装机向导装一个吧。")}
          </div>
        ) : (
          <div className="space-y-1.5">
            {rows.map((r) => (
              <label
                key={r.id}
                className="flex items-center gap-2.5 px-3 py-2.5 rounded-lg border border-white/[0.07] bg-bg-1 cursor-pointer hover:border-white/[0.14]"
              >
                <input
                  type="checkbox"
                  checked={picked.has(r.id)}
                  onChange={() => toggle(r.id)}
                  className="w-3.5 h-3.5 accent-accent shrink-0"
                />
                <ToolIcon tool={r.icon} size={17} active className="shrink-0 opacity-90" />
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] text-ink-1">{r.name}</div>
                  <div className="text-[10.5px] text-ink-5 truncate">{r.now}</div>
                </div>
                {r.ownReason && (
                  <span className="text-[10px] text-warning-700 dark:text-warning-400 shrink-0" title={t("默认不动你自己配好的")}>
                    {r.ownReason}
                  </span>
                )}
              </label>
            ))}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <button
            onClick={onCancel}
            className="h-8 px-3.5 rounded-lg border border-white/[0.10] bg-bg-1 text-[12px] text-ink-2 hover:border-white/20"
          >
            {t("取消")}
          </button>
          <button
            onClick={() => onConfirm([...picked])}
            disabled={picked.size === 0}
            className="h-8 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40"
          >
            {picked.size ? t("配置这 {n} 个", { n: picked.size }) : t("请至少勾一个")}
          </button>
        </div>
      </div>
    </div>
  );
}
