/**
 * ProviderSwitch —— 每个 AI 工具卡片下面的 per-tool 驱动切换列表（cc-switch 式）。
 *
 * 取代旧 ModelSwitcher：真正支持切到「自定义 provider」和「官方还原」，且修掉了
 * 切虾盘云传空 Key 的 bug（claude/codex target 空 Key 会被后端拒绝）。
 *
 * targets 决定写哪个工具的底层配置（对齐 providers.rs apply_provider）：
 *   Claude Code → ["claude"]、Codex → ["codex"]、OpenClaw → ["clawx"]、Hermes → ["hermes"]、DSH → ["dsh"]。
 *
 * Key 来源：
 *   - 虾盘云系（builtin_recharge）→ 设备内置 Key（deviceKey.key）
 *   - 自定义 provider → provider 自带的 api_key
 *   - 官方还原（id==="official"）→ "-"（后端只删配置不需要 key）
 *   - 其它内置（DeepSeek/GLM/Kimi）→ 没填 Key 时引导去 AI 设置填
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, ChevronDown, Pencil, Trash2, Plus, Settings2 } from "lucide-react";
import type { ProviderPreset } from "../Wizard";
import type { DeviceKey, DriverStatus } from "../lib/types";
import { mergeModels, priceyModelHint, recommendedVisionModel, codexProtocolHint } from "../lib/models";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

export function ProviderSwitch({
  targets,
  deviceKey: deviceKeyProp,
  onToast,
  onGoManage,
  onManageProviders,
  onSwitched,
  compact,
}: {
  targets: string[];
  /** 设备内置 Key；不传则本组件自取（OpenCodex 等深层场景用） */
  deviceKey?: DeviceKey | null;
  onToast: (s: string) => void;
  onGoManage: () => void;
  /** 打开 provider 增删改弹层；带 editId 则直接编辑该项。不传则隐藏增/改入口 */
  onManageProviders?: (editId?: string) => void;
  onSwitched?: () => void;
  compact?: boolean;
}) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<ProviderPreset[]>([]);
  const [driver, setDriver] = useState<DriverStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [deviceKeySelf, setDeviceKeySelf] = useState<DeviceKey | null>(null);
  const deviceKey = deviceKeyProp ?? deviceKeySelf;

  /**
   * 服务端**实际有**哪些模型。`null` = 还没拉到 / 拉失败 = **不知道**（不是「没有」）。
   *
   * 🔴 为什么要拉：本地 `XIAPAN_MODELS` 是手工清单，2026-08-20 实测**服务端有 13 个它没收**
   * （kimi-k2.6 / glm-5 / glm-5.1 / qwen3.6-plus / deepseek-reasoner …）——
   * 客户充了钱、渠道也开了，就因为下拉里没有而用不上。新模型（客户问的 deepseek v5）
   * 按老做法得等我们改代码 + 发版才轮得到他。
   *
   * 拉失败**不做任何降级动作** —— `mergeModels(null)` 会原样退回本地清单，
   * 绝不会因为一次网络抖动让下拉变空。
   */
  const [liveModels, setLiveModels] = useState<string[] | null>(null);
  useEffect(() => {
    const key = deviceKey?.key;
    if (!key) return;
    let alive = true;
    invoke<string[]>("list_remote_models", { providerId: "xiapan", apiKey: key })
      .then((ids) => alive && setLiveModels(Array.isArray(ids) && ids.length ? ids : null))
      .catch(() => alive && setLiveModels(null)); // 拉不到就是不知道，交给 mergeModels 兜
    return () => {
      alive = false;
    };
  }, [deviceKey?.key]);
  const modelGroups = mergeModels(liveModels);
  // 虾盘云类卡片选中的模型（key=provider id）
  const [modelSel, setModelSel] = useState<Record<string, string>>({});
  const [modelOpen, setModelOpen] = useState<string | null>(null);

  // 列表是 per-tool 的（每个 AI 各有一份）—— 这个组件只管 targets[0] 那一个工具，
  // 拉列表就得带上它，否则会显示别的 AI 才有的供应商。
  const listTool = targets[0];

  const refresh = useCallback(async () => {
    const [p, d] = await Promise.all([
      invoke<ProviderPreset[]>("list_providers", { tool: listTool }).catch(() => []),
      invoke<DriverStatus>("get_driver_status").catch(() => null),
    ]);
    setProviders(p);
    setDriver(d);
  }, [listTool]);

  useEffect(() => {
    void refresh();
    // 没传 deviceKey 时自取（虾盘云切换要用内置 Key）
    if (!deviceKeyProp) {
      invoke<DeviceKey>("get_device_key").then(setDeviceKeySelf).catch(() => {});
    }
  }, [refresh, deviceKeyProp]);

  // 当前生效的模型（按 targets 取对应工具的回显）
  const isClawx = targets.includes("clawx");
  const isHermes = targets.includes("hermes");
  const isCodex = targets.includes("codex");
  const isDsh = targets.includes("dsh");
  const activeModel = isClawx
    ? driver?.clawx_model
    : isHermes
      ? driver?.hermes_model
      : isCodex
        ? driver?.codex_model
        : isDsh
          ? driver?.dsh_model
          : driver?.claude_model;

  /**
   * 该 provider 当前是否生效。**只读后端 `active` 表**（对齐 cc-switch is_current：
   * 切一次记一笔，回显读这一笔），不再按 model/base 反推——这才是 Hermes「切官方后
   * 还显示使用中」的根治（每个 ProviderSwitch 实例只切 targets[0] 一个工具）。
   */
  const activeId = driver?.active?.[targets[0]] ?? null;
  const isActive = (p: ProviderPreset): boolean => activeId === p.id;

  /** Codex 横幅：跟着**当前生效的**供应商走。还没切过（activeId 为空）时回落到虾盘云那条，
   *  因为那就是开箱默认。没匹配到供应商就不弹 —— 宁可少一条，也别再对 DeepSeek 官方
   *  的客户喊「别用裸名」（那正是他唯一能用的）。 */
  const codexBanner = isCodex
    ? codexProtocolHint(providers.find((p) => p.id === activeId) ?? { id: "xiapan", builtin_recharge: true })
    : null;

  const keyFor = (p: ProviderPreset): string | null => {
    if (p.id === "official") return "-";
    if (p.builtin_recharge) return deviceKey?.key || "";
    if (!p.builtin) return p.api_key || ""; // 自定义 provider 自带 key
    return ""; // 其它内置（DeepSeek/GLM/Kimi）需用户在 AI 设置填
  };

  const doSwitch = useCallback(
    async (p: ProviderPreset, modelOverride?: string) => {
      if (busy) return;
      const key = keyFor(p);
      if (key === "") {
        onToast(t("{name} 需要先在「AI 设置」填 Key", { name: p.name }));
        onGoManage();
        return;
      }
      const model = (p.builtin_recharge && (modelOverride || modelSel[p.id])) || null;
      setBusy(p.id);
      try {
        await invoke("apply_provider", {
          providerId: p.id,
          apiKey: key,
          model,
          targets,
        });
        // ClawX 不热重载配置文件（它运行时持有内存副本，退出会覆写）——切完必须重启 ClawX 才生效。
        // 这是 ClawX 和 U-King 抢同一个 openclaw.json 的真坑（实测：ClawX 开着切，切了不生效）。
        const clawxHint = targets.includes("clawx") ? t("，请重启 ClawX 生效") : "";
        const modelPart = model ? `（${model}）` : "";
        onToast(
          p.id === "official"
            ? t("已还原官方配置{hint}", { hint: clawxHint })
            : t("已切到 {name}{model}{hint}", { name: p.name, model: modelPart, hint: clawxHint }),
        );
        await refresh();
        onSwitched?.();
      } catch (e) {
        onToast(String(e));
      } finally {
        setBusy(null);
      }
    },
    [busy, deviceKey, modelSel, targets, onToast, onGoManage, refresh, onSwitched, t],
  );

  return (
    <div>
      {!compact && (
        <div className="px-1 pb-1.5 text-[11px] text-ink-4">
          {t("当前：")}<span className="text-ink-2 font-mono">{activeModel || t("未配置")}</span>
        </div>
      )}
      {/* Codex 该填哪个模型 —— 文案与判据都在 `lib/models.ts::codexProtocolHint`，这里只负责渲染。
          🔴 2026-08-11 这里按供应商分过一次文案，但同一句话在 `Manager.tsx` 还留着一份**没分**的，
             照旧对所有供应商喊「国产裸名会报 not implemented」（报错名也是错的，实际是
             500 convert_request_failed）。同一事实两份副本 = 漂移两份，所以收成一份（宪法第 8 条）。
          这里的横幅是「整列表一条」，拿不到具体供应商，只能按当前生效的那家给；
          逐张卡片的精确版在 Manager 的「换模型」框里。 */}
      {isCodex && codexBanner && (
        <div className="mb-1.5 rounded-lg border border-ink-6 bg-bg-3 px-2.5 py-1.5 text-[11px] leading-snug text-ink-2">
          {t(codexBanner)}
        </div>
      )}
      {/* ClawX 发图识别引导：DeepSeek 等纯文本模型收不了图，客户粘图进 ClawX 会失败也不知道该切模型
          （pc-*** 实锤）。给一条醒目提示 + 一键切到看图模型，把埋在「换模型→看图识图」里的路做成一步。 */}
      {isClawx && (
        <div className="mb-1.5 flex flex-wrap items-center gap-1.5 rounded-lg border border-accent/30 bg-accent/[0.07] px-2.5 py-1.5 text-[11px] leading-snug text-ink-2">
          <span>
            🖼️ {t("要识别图片 / 截图？")}
            <b className="text-ink-1">{t("DeepSeek 等纯文本模型看不了图")}</b>
            {t("，切到看图模型即可。")}
          </span>
          <button
            onClick={() => {
              const vision = recommendedVisionModel();
              if (!vision) return onToast(t("看图模型清单为空，请在「换模型」里手选"));
              const xp = providers.find((p) => p.builtin_recharge);
              if (xp) void doSwitch(xp, vision.id);
              else onToast(t("没找到虾盘云内置驱动，请先在「AI 设置」配置"));
            }}
            disabled={!!busy || !recommendedVisionModel()}
            className="ml-auto inline-flex items-center gap-1 px-2 h-6 rounded-md bg-accent text-white text-[10.5px] font-medium hover:bg-accent-600 disabled:opacity-50 shrink-0"
          >
            {/* 名字跟着 models.ts 的★走，别写死 —— 写死过一次，跑道换默认后按钮把客户切到了旧模型 */}
            {t("一键切 {name}（看图）", {
              name: (recommendedVisionModel()?.label || "").split(" · ")[0],
            })}
          </button>
        </div>
      )}
      <div className={cn("space-y-0.5", compact ? "max-h-60 overflow-y-auto" : "")}>
        {providers.map((p) => {
          const on = isActive(p);
          const selModel = modelSel[p.id] || (on && activeModel) || p.model;
          // 贵/慎用模型成本提示（只对虾盘云内置生效——它才可换成 gpt-5.6-* 这类前沿贵模型）
          const priceHint = p.builtin_recharge ? priceyModelHint(selModel) : null;
          return (
            <div
              key={p.id}
              className={cn(
                "rounded-lg border px-2 py-1.5 transition-colors",
                on
                  ? "border-accent bg-accent/[0.12] ring-1 ring-inset ring-accent"
                  : "border-white/[0.06] bg-white/[0.02] hover:bg-white/[0.04]",
              )}
            >
              <div className="flex items-center gap-2">
                <button
                  data-action-id="runtime.driver.apply"
                  onClick={() => doSwitch(p)}
                  disabled={busy === p.id}
                  className="flex-1 min-w-0 flex items-center gap-1.5 text-left disabled:opacity-50"
                >
                  {on ? (
                    <Check size={13} className="text-accent shrink-0" />
                  ) : (
                    <span className="w-[13px] shrink-0" />
                  )}
                  <span className={cn("text-[12.5px] truncate", on ? "font-semibold text-ink-0" : "text-ink-1")}>{p.name}</span>
                  {on && (
                    <span className="inline-flex items-center px-1.5 h-[16px] rounded-full text-[9px] font-bold bg-accent text-white shrink-0">
                      {t("使用中")}
                    </span>
                  )}
                  {p.builtin_recharge && (
                    <span className="text-[9px] text-accent-400 shrink-0">{t("内置")}</span>
                  )}
                  {!p.builtin && (
                    <span className="text-[9px] text-ink-5 shrink-0">{t("自定义")}</span>
                  )}
                </button>

                {/* 虾盘云类：换模型下拉 */}
                {p.builtin_recharge && (
                  <button
                    onClick={() => setModelOpen(modelOpen === p.id ? null : p.id)}
                    className="inline-flex items-center gap-0.5 px-1.5 h-6 rounded text-[10.5px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.05] shrink-0"
                    title={t("换模型")}
                  >
                    <span className="font-mono max-w-[88px] truncate">{selModel}</span>
                    <ChevronDown size={11} />
                  </button>
                )}

                {/* 自定义 provider：编辑 / 删除 */}
                {!p.builtin && onManageProviders && (
                  <>
                    <button
                      onClick={() => onManageProviders(p.id)}
                      className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.05] shrink-0"
                      title={t("编辑")}
                    >
                      <Pencil size={12} />
                    </button>
                    <button
                      data-action-id="runtime.provider.delete"
                      onClick={async () => {
                        try {
                          // ★ 只从**这个 AI** 的列表里拿走：别的 AI 照旧留着它，定义和 Key 也不销毁
                          // （要彻底删得去「AI 设置」里编辑它 → 彻底删除）。
                          await invoke("delete_provider", { id: p.id, tool: listTool });
                          onToast(t("已从这个 AI 的列表移除 {name}（其它 AI 保留）", { name: p.name }));
                          await refresh();
                        } catch (e) {
                          onToast(String(e));
                        }
                      }}
                      className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-4 hover:text-danger-400 hover:bg-white/[0.05] shrink-0"
                      title={t("从这个 AI 的列表移除（其它 AI 保留）")}
                    >
                      <Trash2 size={12} />
                    </button>
                  </>
                )}
              </div>

              {/* 贵模型成本护栏（只提醒不拦截）—— 用 danger 红，跟 Manager 那处保持同一套重量 */}
              {priceHint && (
                <p className="mt-1 text-[10px] leading-snug font-medium text-danger-700 dark:text-danger-400 rounded border border-danger-500/40 bg-danger-500/[0.10] px-1.5 py-1">
                  {t(priceHint)}
                </p>
              )}

              {/* 模型下拉展开 */}
              {modelOpen === p.id && p.builtin_recharge && (
                <div className="mt-1.5 max-h-48 overflow-y-auto rounded border border-white/[0.08] bg-bg-1 p-1">
                  {modelGroups.map((g) => (
                    <div key={g.group}>
                      <div className="px-1.5 pt-1 pb-0.5 text-[9.5px] text-ink-5">{g.group}</div>
                      {g.items.map((m) => (
                        <button
                          key={m.id}
                          onClick={() => {
                            setModelSel((s) => ({ ...s, [p.id]: m.id }));
                            setModelOpen(null);
                            // 已生效则立即热切换，否则只记选择（下次点切换用）
                            if (on) void doSwitch(p, m.id);
                          }}
                          className={cn(
                            "w-full flex items-start gap-1.5 px-1.5 py-1.5 rounded text-left hover:bg-white/[0.06]",
                            selModel === m.id ? "text-accent" : "text-ink-2",
                          )}
                        >
                          <span className="w-[11px] pt-0.5 shrink-0">
                            {selModel === m.id && <Check size={11} />}
                          </span>
                          <span className="min-w-0 flex-1">
                            <span className="flex items-center gap-1">
                              <span className="text-[11.5px] truncate">{m.label}</span>
                              {m.recommend && (
                                <span className="text-[8.5px] px-1 rounded bg-accent/20 text-accent-400 shrink-0">
                                  {t("推荐")}
                                </span>
                              )}
                            </span>
                            {m.desc && (
                              <span className="block text-[10px] leading-tight text-ink-4 mt-0.5">
                                {m.desc}
                              </span>
                            )}
                          </span>
                        </button>
                      ))}
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className="mt-2 flex items-center gap-2">
        {onManageProviders && (
          <>
            <button
              onClick={() => onManageProviders()}
              className="inline-flex items-center gap-1 text-[11px] text-ink-4 hover:text-accent-400"
            >
              <Plus size={12} /> {t("添加自定义")}
            </button>
            <span className="text-ink-6">·</span>
          </>
        )}
        <button
          onClick={onGoManage}
          className="inline-flex items-center gap-1 text-[11px] text-ink-4 hover:text-ink-2"
        >
          <Settings2 size={12} /> {t("余额 / 填 Key")}
        </button>
      </div>
    </div>
  );
}
