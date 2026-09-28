/**
 * 「我的 AI」—— 已装工具卡片网格：点开就用，换模型不用跳页。
 *
 * 定位区别（跟侧栏另一个入口「装机 · 体检」`MyAI`(App.tsx) 分工——2026-09-29 首页改版，
 * 两边都改了名，id 和页面实现都没动）：
 *  · `myai`（侧栏「装机 · 体检」）是**装机漏斗**——引导装、体检、卸载，给刚接触 U-King 的人看。
 *  · 这里（`toolhub`，侧栏现在也叫「我的 AI」）是**日常启动台**——已经装好工具的人天天回来的
 *    落脚点：看一眼装了什么、换个模型、点一下启动。也是全站默认落地页（见 App.tsx
 *    `refresh()` 里的落点逻辑：首次打开和回访统一落这里）。
 * 两者共用同一批数据源（`tools`/`driver`/`deviceKey`）和同一条业务通路
 * （`launchTool`/`openTool`/`apply_provider`，均由 App.tsx 传入，本组件不重新实现）。
 *
 * 布局（2026-09-29 改版，去掉了「选工具 → 选模型 → 启动」那套两步交互）：
 * 已安装卡片网格在上、可安装紧凑 tile 在下；换模型收进每张已装卡片自己的下拉
 * （cc-switch 式：点了就应用，不再有「先选中、勾选『修改模型配置』、点启动才生效」的中间态，
 * 也不再有独立的右侧选模型面板和底部 sticky 操作条）。
 *
 * 可插拔：整个模块收在 `src/toolhub/` 下，App.tsx 只在两处接它——侧栏 `CORE` 数组的一条
 * `NavItem` 和主区 `tab === "toolhub"` 的一个分支——删掉这个目录、去掉这两处即可完整移除
 * （宪法「模块独立可插拔」条）。
 *
 * ActionParity：本组件不写任何新业务逻辑，纯粹是对已有 Tauri 命令（`list_providers` /
 * `apply_provider`）和已有回调（`onLaunch`/`onOpen`）的又一层界面，跟 `ProviderSwitch.tsx`
 * 走的是同一条后端路径，`apply_provider` 的调用形状（provider/apiKey/model/targets 四个
 * 字段）照抄 `ProviderSwitch.doSwitch`，没有引入新的写法。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  ChevronDown,
  Cpu,
  Download,
  LayoutGrid,
  Play,
  Plus,
  RefreshCw,
  Rocket,
  Settings2,
  SquareTerminal,
  Wallet,
  Sparkles,
} from "lucide-react";
import { LAB_TOOLS, toolTargets, currentModelFor, discoveryNameFor, type ToolInfo } from "../App";
import type { DeviceKey, DriverStatus } from "../lib/types";
import type { ProviderPreset } from "../Wizard";
import { ToolIcon } from "../components/ToolIcon";
import { AnchoredMenu } from "../components/AnchoredMenu";
import { providerKeyFor } from "../components/ProviderSwitch";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";
import { getLaunchPref, setLaunchPref, type LaunchPref } from "./launchPref";
import { buildProviderRepairPrompt, type ProviderRepairPromptInput } from "../lib/providerRepairPrompt";

type Category = "all" | "cli" | "gui" | "agent" | "lab";

/**
 * 分类映射——前端 `ToolInfo` 目前只暴露 `kind`("deep"/"standalone") 和 `launch_app`/
 * `launch_cmd` 两个非空判定，没有直接给 `LaunchMode`（那是后端 `tools::TOOL_SPECS` 内部字段，
 * 没有经 `discover_tools`/`ToolInfo` 序列化过来）。用现有字段能拼出一个够用的四分类：
 *  · 实验室：命中 `LAB_TOOLS`（跟侧栏「实验室」折叠组同一份名单）；
 *  · 智能体：`kind === "deep"`（深度接入的对话/编码代理，如 Claude Code / Codex / Hermes）；
 *  · 桌面应用：`launch_app` 非空（有独立可执行 GUI 程序，如 ClawX / DSH）；
 *  · 命令行：其余（`standalone` 且没有 GUI 启动项，走终端命令）。
 * 这是按现有数据做的合理映射，不是从后端拿到的权威分类——如果以后 `ToolInfo` 补上
 * `launch_mode` 字段，这里应该直接切过去读它，不用再猜。
 */
function categoryOf(t: ToolInfo): Category {
  if (LAB_TOOLS.has(t.id)) return "lab";
  if (t.kind === "deep") return "agent";
  if (t.launch_app) return "gui";
  return "cli";
}

const CATS: { id: Category; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "cli", label: "命令行" },
  { id: "gui", label: "桌面应用" },
  { id: "agent", label: "智能体" },
  { id: "lab", label: "实验室" },
];

/** 接口主机名，仅用于展示——跟 `CustomProviderModal.tsx`/`Manager.tsx` 里那个
 *  `new URL(url).host || url` 一样的一行兜底写法，没有必要为一行逻辑抽公共模块。 */
function hostOf(url: string | null | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

export function ToolHub({
  tools,
  driver,
  deviceKey,
  onLaunch,
  onOpen,
  onGoManage,
  onManageProviders,
  onRecharge,
  onRefreshTools,
  onToast,
  onAskAiToFix,
  onGoSetup,
  onGoDoctor,
}: {
  tools: ToolInfo[];
  driver: DriverStatus | null;
  deviceKey: DeviceKey | null;
  /** 已装工具的启动通路（=App.tsx 的 `launchTool`，含运行时判定 + U-CLI/系统终端分流）。 */
  onLaunch: (t: ToolInfo) => void;
  /** 未装工具的一键安装通路 / GUI 应用直开（=App.tsx 的 `openTool`）。 */
  onOpen: (t: ToolInfo) => void;
  onGoManage: () => void;
  onManageProviders: (editId?: string, tool?: string) => void;
  onRecharge: () => void;
  onRefreshTools: () => Promise<ToolInfo[] | void> | void;
  onToast: (s: string) => void;
  /**
   * 「让 AI 帮我修」——跟 `ai-settings-repair` / `airuntime-doctor` / DSH「让 AI 帮你挑」
   * 同一条修复按钮语义：走 App.tsx 的 `pendingChatPrompt` handoff，**自动发给 AI**，
   * 不是起手词那种「只填输入框」（那条规矩只管 `QuickPrompts.tsx` 的场景 chips）。
   *
   * 🔴 **只接得住这一页自己能观测到的失败**：`onLaunch`/`onOpen`（=App.tsx 的
   * `launchTool`/`openTool`）都是 fire-and-forget，内部失败自己 `flash` 掉，不会把
   * rejection 抛回这里——想让「装/启动失败」也点得出这个链接，得先把那两个函数的
   * 签名改成能报成功/失败（它们还被「装机 · 体检」`ToolMarket` 等好几处复用），
   * 那是比这次改动大一圈的事，先如实说明，不在这里悄悄扩大范围。
   * 目前这条链接接的是每张已装卡片自己 try/catch 得到的那次失败：换模型驱动时
   * `apply_provider` 报错。
   */
  onAskAiToFix?: (prompt: string) => void;
  /** 空态引导「一键装好推荐组合」→ App.tsx 的 `startInstallAll`（装机向导，预选 "all"，
   *  真正排队装好 Claude Code + 必要环境）。**不是**单纯 `setTab("setup")`——那样进页面
   *  只会按「一个 AI 都没装」条件自动拉起「逐个选装」，跟按钮文案「一键」对不上
   *  （复审 medium #6：这是曾经的回归，2026-09-29 改回）。 */
  onGoSetup: () => void;
  /** 页头次要链接「装机 · 体检 →」→ App.tsx 的 `setTab("myai")`（装机漏斗 + 体检/卸载）。 */
  onGoDoctor: () => void;
}) {
  const { t: tr } = useI18n();
  const [category, setCategory] = useState<Category>("all");
  const [refreshing, setRefreshing] = useState(false);

  const visibleTools = useMemo(() => tools.filter((t) => !t.hidden), [tools]);
  // 「已装」只数主线工具——`LAB_TOOLS`（Open365 等）不算。Open365 后端 `installed` 恒 true
  // （按需下载设计，见 App.tsx `LAB_TOOLS` 注释），不摘掉的话空态判断永远拿不到 0，
  // 全新用户第一次进来就看不到「还没装 AI 工具」引导（复审 high：确认属实的回归）。
  // 已装的实验室工具单独收进 `installedLab`，在下面一个小分区里露出，不是直接消失。
  const installed = useMemo(
    () => visibleTools.filter((t) => t.installed && !LAB_TOOLS.has(t.id)),
    [visibleTools],
  );
  const installedLab = useMemo(
    () => visibleTools.filter((t) => t.installed && LAB_TOOLS.has(t.id)),
    [visibleTools],
  );
  const installableAll = useMemo(() => visibleTools.filter((t) => !t.installed), [visibleTools]);
  // 分类 chip 只过滤「可安装」分区，已安装网格不受它影响——已装的工具本来就没几个，
  // 全部摆出来一眼看完比再筛一层更直接。
  const installableFiltered = useMemo(
    () => installableAll.filter((t) => category === "all" || categoryOf(t) === category),
    [installableAll, category],
  );

  // 每张已装卡片自己的「换模型」下拉——同一时刻只开一个，跟 AnchoredMenu 本身的假设一致。
  const [openMenu, setOpenMenu] = useState<{ tool: ToolInfo; target: string } | null>(null);
  const anchorMap = useRef<Record<string, HTMLButtonElement | null>>({});
  const closeMenu = () => setOpenMenu(null);

  // 供应商列表按 target 缓存、首次打开对应下拉时才拉——不是一进页面就把四个工具的
  // 供应商全拉一遍（早年 `list_providers` 会真的探测/读盘，白拉浪费）。
  const [providersByTarget, setProvidersByTarget] = useState<Record<string, ProviderPreset[]>>({});
  const [loadingTarget, setLoadingTarget] = useState<string | null>(null);
  const fetchedTargets = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!openMenu) return;
    const { target } = openMenu;
    if (fetchedTargets.current.has(target)) return;
    fetchedTargets.current.add(target);
    setLoadingTarget(target);
    // 🔴 曾经这里有个 `alive` 标记，在 effect cleanup（菜单一关就跑，不是组件卸载）里把它
    // 置 false，导致请求还没回来就关菜单时 then/catch/finally 全部跳过：`loadingTarget`
    // 卡在这个 target 上出不去，`fetchedTargets` 又已经标记过，下次再开同一个下拉只会
    // 一直显示「加载中…」（复审 medium：确认属实的竞态）。这些 state 挂在 ToolHub 本身，
    // 关菜单不等于组件卸载，去掉这层假保护、结果按 target 落地即可，不会有装错菜的风险。
    invoke<ProviderPreset[]>("list_providers", { tool: target })
      .then((ps) => {
        setProvidersByTarget((m) => ({ ...m, [target]: ps ?? [] }));
      })
      .catch(() => {
        // 失败不缓存：把 target 从 fetchedTargets 里摘掉，下次再开同一个下拉能重新拉一次，
        // 而不是从此永远显示这一份空列表（曾经失败一次就再也拉不动，见本 effect 上面的注释
        // ——同一处“一次性标记 + 不回滚”的坑）。
        fetchedTargets.current.delete(target);
        setProvidersByTarget((m) => ({ ...m, [target]: [] }));
      })
      .finally(() => {
        setLoadingTarget((cur) => (cur === target ? null : cur));
      });
  }, [openMenu]);

  // 虾盘云（内置充值渠道）钉最前——用户开箱唯一不用自己填 Key 就能用的一档，
  // 该被第一眼看到。纯前端展示排序，不改后端 `list_providers` 的顺序语义
  // （那份顺序是用户可拖拽调整的「偏好」，这里只是把内置渠道垫到最上面显示）。
  function sortedProvidersFor(target: string): ProviderPreset[] {
    return [...(providersByTarget[target] ?? [])].sort(
      (a, b) => Number(!!b.builtin_recharge) - Number(!!a.builtin_recharge),
    );
  }

  // 应用中 / 报错——都是 per-tool 的（Record 而不是单个全局标记），因为不同卡片的
  // 换模型互不相干：一张卡应用中不该拦住另一张卡也能点。
  const [applyingIds, setApplyingIds] = useState<Set<string>>(new Set());
  const [applyFailures, setApplyFailures] = useState<Record<string, ProviderRepairPromptInput>>({});

  function openModelMenu(t: ToolInfo) {
    const target = toolTargets(t.id)[0];
    if (!target) return;
    setOpenMenu({ tool: t, target });
  }

  async function applyProvider(t: ToolInfo, target: string, p: ProviderPreset) {
    // 点的就是当前「使用中」那家：不重新 apply。`model: null` 会让后端 `effective_model`
    // 落回 preset 默认模型（providers.rs），如果用户之前在这个供应商下手动换过非默认模型，
    // 点一下「使用中」条目就会被悄悄换回默认——旧版 ToolHub 用 `selectedProviderId !== activeId`
    // 挡过这个（复审 medium：确认属实的回归，这里恢复同等保护）。
    if (p.id === driver?.active?.[target]) {
      closeMenu();
      return;
    }
    const key = providerKeyFor(p, deviceKey);
    if (key === "") {
      closeMenu();
      onToast(tr("{name} 需要先在「AI 设置」填 Key", { name: p.name }));
      onGoManage();
      return;
    }
    closeMenu();
    setApplyingIds((s) => new Set(s).add(t.id));
    try {
      await invoke("apply_provider", { providerId: p.id, apiKey: key, model: null, targets: toolTargets(t.id) });
      // ClawX 不热重载配置文件（运行时持有内存副本，退出会覆写）——切完必须重启 ClawX 才生效。
      // 跟 `ProviderSwitch.tsx::doSwitch` 用同一句提示（`clawxHint`），别在这重新造一句漂移的文案。
      const clawxHint = toolTargets(t.id).includes("clawx") ? tr("，请重启 ClawX 生效") : "";
      // 🔴 这里原来写的是「已切到 {name}，正在启动 {tool}…」——这句文案是旧版「先应用再
      // 启动」流程留下的，这次改版只切模型、不启动工具，onLaunch 根本没被调用，用户会
      // 一直等一个不会出现的窗口（复审 medium：确认属实）。改用 `ProviderSwitch.tsx::doSwitch`
      // 「只切换」的同一句译文，别在这另造一句漂移的文案。
      onToast(
        p.id === "official"
          ? tr("已还原官方配置{hint}", { hint: clawxHint })
          : tr("已切到 {name}{model}{hint}", { name: p.name, model: "", hint: clawxHint }),
      );
      setApplyFailures((m) => {
        if (!(t.id in m)) return m;
        const n = { ...m };
        delete n[t.id];
        return n;
      });
      await onRefreshTools();
    } catch (e) {
      onToast(String(e));
      setApplyFailures((m) => ({
        ...m,
        [t.id]: {
          providerName: p.name,
          baseUrl: (target === "claude" ? p.anthropic_base || p.openai_base : p.openai_base || p.anthropic_base) || "",
          model: currentModelFor(t, driver) ?? "",
          target,
          error: String(e),
        },
      }));
    } finally {
      setApplyingIds((s) => {
        if (!s.has(t.id)) return s;
        const n = new Set(s);
        n.delete(t.id);
        return n;
      });
    }
  }

  const [launchPref, setLaunchPrefState] = useState<LaunchPref>(getLaunchPref);
  const setLaunchPrefAndPersist = (v: LaunchPref) => {
    setLaunchPrefState(v);
    setLaunchPref(v);
  };

  const refreshAll = async () => {
    setRefreshing(true);
    try {
      await onRefreshTools();
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div>
          <h1 className="text-[18px] font-semibold text-ink-0 flex items-center gap-2">
            <LayoutGrid size={18} className="text-accent" />
            {tr("我的 AI")}
          </h1>
          <p className="mt-0.5 text-[12px] text-ink-3">{tr("已装 {n} 个工具", { n: installed.length })}</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => void refreshAll()}
            disabled={refreshing}
            className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-[12.5px] text-ink-2 hover:bg-white/[0.05] disabled:opacity-60"
          >
            <RefreshCw size={13} className={refreshing ? "animate-spin" : ""} />
            {tr("刷新")}
          </button>
          <button
            data-testid="toolhub-go-doctor"
            onClick={onGoDoctor}
            className="inline-flex items-center gap-1 px-2 h-8 text-[12.5px] text-accent hover:text-accent-600"
          >
            {tr("装机 · 体检 →")}
          </button>
        </div>
      </div>

      {installed.length === 0 && (
        <div className="rounded-card border border-accent/30 bg-accent/[0.06] p-5">
          <div className="flex flex-col sm:flex-row sm:items-center gap-3">
            <div className="flex-1 text-[14px] font-semibold text-ink-0">{tr("还没装 AI 工具")}</div>
            <button
              data-testid="toolhub-go-setup"
              onClick={onGoSetup}
              className="inline-flex items-center gap-1.5 px-4 h-9 rounded-xl bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shadow-sm shrink-0"
            >
              <Rocket size={14} /> {tr("一键装好推荐组合")}
            </button>
          </div>
          <div className="mt-2 text-[11.5px] text-ink-4">{tr("或在下面挑一个装")}</div>
        </div>
      )}

      {installed.length > 0 && (
        <section className="space-y-2.5">
          <h2 className="text-[13px] font-semibold text-ink-1">{tr("已安装（{n}）", { n: installed.length })}</h2>
          <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-3">
            {installed.map((t) => {
              const model = currentModelFor(t, driver);
              const targets = toolTargets(t.id);
              const target = targets[0];
              const version = primaryDiscoveryVersion(t, driver);
              const applying = applyingIds.has(t.id);
              const failure = applyFailures[t.id];
              return (
                <div key={t.id} data-tool-id={t.id} className="rounded-card border border-ink-5/30 bg-bg-1 p-3 flex flex-col gap-2">
                  <div className="flex items-center gap-2.5">
                    <span className="grid place-items-center w-9 h-9 rounded-xl bg-bg-3 shrink-0">
                      <ToolIcon tool={t.id} size={22} active />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="text-[13.5px] font-semibold text-ink-0 truncate flex items-center gap-1">
                        <Check size={11} className="text-success-400 shrink-0" />
                        <span className="truncate">{t.name}</span>
                      </div>
                      {version && <div className="text-[10.5px] text-ink-5 font-mono truncate">v{version}</div>}
                    </div>
                  </div>

                  {target ? (
                    <button
                      ref={(el) => {
                        anchorMap.current[t.id] = el;
                      }}
                      data-testid="toolhub-model-trigger"
                      data-tool-id={t.id}
                      onClick={() => openModelMenu(t)}
                      disabled={applying}
                      className="w-full flex items-center gap-1.5 rounded-lg border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] px-2 h-7 text-[11px] disabled:opacity-60"
                    >
                      <Cpu size={11} className="text-accent/70 shrink-0" />
                      {model ? (
                        <span className="flex-1 min-w-0 truncate text-ink-2 text-left">{model}</span>
                      ) : (
                        <span className="flex-1 min-w-0 truncate text-warning-500 text-left">{tr("还没配模型")}</span>
                      )}
                      <ChevronDown size={11} className="text-ink-5 shrink-0" />
                    </button>
                  ) : model ? (
                    // target 为空但 currentModelFor 有值（如 dsh——模型由 U-King 配置，只是
                    // 走的不是 `toolTargets`/`apply_provider` 这条切换通路）：显示只读模型行，
                    // 不能说成「使用工具自带账号」——那句是给真正不接 U-King 配置的工具用的
                    // （复审 medium：确认属实，旧版对所有已装工具都会显示这一行，不分 target）。
                    <div className="flex items-center gap-1.5 px-2 h-7 text-[11px] text-ink-2">
                      <Cpu size={11} className="text-accent/70 shrink-0" />
                      <span className="flex-1 min-w-0 truncate text-left">{model}</span>
                    </div>
                  ) : (
                    // target 为空、currentModelFor 也拿不到值：这类工具后端其实可能能配模型
                    // （如 pi），只是前端还没接上这条通路——说成「使用工具自带账号」不属实。
                    // 如实说「这里换不了」，给个「AI 设置」的出口，别把用户卡死在这张卡片上。
                    <div className="flex items-center gap-1.5 px-2 h-7 text-[11px] text-ink-5">
                      <Cpu size={11} className="shrink-0" />
                      <span className="flex-1 min-w-0 truncate text-left">{tr("这里暂不能换它的模型")}</span>
                      <button
                        data-testid="toolhub-go-manage"
                        onClick={onGoManage}
                        className="shrink-0 text-accent-400 hover:text-accent hover:underline"
                      >
                        {tr("AI 设置")}
                      </button>
                    </div>
                  )}

                  {failure && (
                    <div className="rounded-lg border border-danger-500/40 bg-danger-500/[0.10] px-2 py-1.5">
                      <p className="text-[10px] leading-snug font-medium text-danger-700 dark:text-danger-400">{failure.error}</p>
                      {onAskAiToFix && (
                        <button
                          onClick={() => {
                            onAskAiToFix(buildProviderRepairPrompt(failure));
                            onToast(tr("已把故障交给 AI，正在打开工作台"));
                          }}
                          className="mt-1 inline-flex items-center gap-1 px-2 h-6 rounded-full border border-accent/30 bg-accent/[0.08] text-[10.5px] text-accent hover:bg-accent/[0.14]"
                        >
                          <Sparkles size={10} /> {tr("让 AI 帮我修")}
                        </button>
                      )}
                    </div>
                  )}

                  <button
                    data-action-id="runtime.tool.launch"
                    data-tool-id={t.id}
                    onClick={() => onLaunch(t)}
                    disabled={applying}
                    className="inline-flex items-center justify-center gap-1.5 h-8 rounded-lg bg-accent text-white text-[12.5px] font-semibold hover:bg-accent-600 disabled:opacity-60"
                  >
                    {applying ? (
                      tr("正在应用配置…")
                    ) : (
                      <>
                        <Play size={13} /> {tr("启动")}
                      </>
                    )}
                  </button>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {installedLab.length > 0 && (
        // Open365 等「实验室」工具已装时的落脚点——它们不进上面「已安装」主网格（不算进
        // 「一键装好你的全部 AI」这条主线，见 App.tsx `LAB_TOOLS` 注释），但也不该点了就
        // 从这页彻底消失，所以单独收一条紧凑分区。点击复用 MyAI 里 `LabTools` 同一套判断
        // （装了 GUI 应用就直接打开，否则走 onOpen 的兜底），不走上面「已装」卡片那套
        // 换模型/启动分离的复杂交互——这些工具本来就不接 U-King 的模型切换。
        <section className="space-y-2">
          <h2 className="text-[12px] font-medium text-ink-4">{tr("实验室（已装）")}</h2>
          <div className="flex flex-wrap gap-2">
            {installedLab.map((t) => (
              <button
                key={t.id}
                data-tool-id={t.id}
                onClick={() => (t.launch_app && t.installed ? onLaunch(t) : onOpen(t))}
                className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] text-[12px] text-ink-2"
              >
                <ToolIcon tool={t.id} size={14} active />
                {t.name}
              </button>
            ))}
          </div>
        </section>
      )}

      <section className="space-y-2.5">
        <div className="flex items-center justify-between gap-3 flex-wrap">
          <h2 className="text-[13px] font-semibold text-ink-1">{tr("可安装（{n}）", { n: installableAll.length })}</h2>
          <div className="flex items-center gap-1.5 flex-wrap">
            {CATS.map((c) => (
              <button
                key={c.id}
                onClick={() => setCategory(c.id)}
                className={cn(
                  "px-2.5 h-7 rounded-full text-[11.5px] font-medium transition-colors",
                  category === c.id
                    ? "bg-accent text-white"
                    : "bg-white/[0.03] text-ink-3 hover:bg-white/[0.06] hover:text-ink-1",
                )}
              >
                {tr(c.label)}
              </button>
            ))}
          </div>
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2.5">
          {installableFiltered.length === 0 && (
            <div className="col-span-full rounded-card border border-dashed border-white/[0.12] px-4 py-6 text-center text-[12px] text-ink-4">
              {tr("这个分类下暂时没有工具")}
            </div>
          )}
          {installableFiltered.map((t) => (
            <button
              key={t.id}
              data-tool-id={t.id}
              onClick={() => onOpen(t)}
              className="text-left rounded-card border border-ink-5/30 hover:border-white/[0.16] bg-bg-1 p-2.5 flex flex-col gap-1.5"
            >
              <div className="flex items-center gap-2">
                <span className="grid place-items-center w-7 h-7 rounded-lg bg-bg-3 shrink-0">
                  <ToolIcon tool={t.id} size={16} active={false} />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] font-medium text-ink-1 truncate">{t.name}</div>
                  <div className="text-[10.5px] text-ink-4 truncate">{tr(CATS.find((c) => c.id === categoryOf(t))?.label ?? "")}</div>
                </div>
              </div>
              <span className="inline-flex items-center justify-center gap-1 h-7 rounded-lg bg-white/[0.04] text-[11.5px] text-accent-400 hover:bg-accent/[0.10]">
                <Download size={12} /> {tr("安装")}
              </span>
            </button>
          ))}
        </div>
      </section>

      <div className="flex items-center gap-2 text-[11px] text-ink-4">
        <SquareTerminal size={12} className="text-ink-5 shrink-0" />
        <span>{tr("命令行工具打开方式：")}</span>
        <div className="inline-flex rounded-full border border-white/[0.10] p-0.5">
          <button
            data-testid="toolhub-launch-pref-system"
            onClick={() => setLaunchPrefAndPersist("system")}
            className={cn(
              "px-2.5 h-6 rounded-full text-[10.5px]",
              launchPref === "system" ? "bg-accent text-white" : "text-ink-3 hover:text-ink-1",
            )}
          >
            {tr("系统终端")}
          </button>
          <button
            data-testid="toolhub-launch-pref-ucli"
            onClick={() => setLaunchPrefAndPersist("ucli")}
            className={cn(
              "px-2.5 h-6 rounded-full text-[10.5px]",
              launchPref === "ucli" ? "bg-accent text-white" : "text-ink-3 hover:text-ink-1",
            )}
          >
            U-CLI
          </button>
        </div>
      </div>

      {openMenu && (
        <AnchoredMenu
          anchorRef={{ current: anchorMap.current[openMenu.tool.id] ?? null }}
          onClose={closeMenu}
          header={openMenu.tool.name}
          minWidth={220}
        >
          <div className="max-h-72 overflow-y-auto p-1 space-y-1">
            {loadingTarget === openMenu.target ? (
              <p className="px-2.5 py-2 text-[12px] text-ink-4">{tr("加载中…")}</p>
            ) : (
              sortedProvidersFor(openMenu.target).map((p) => {
                const on = p.id === driver?.active?.[openMenu.target];
                return (
                  // 🔴 原来这整行是一个 <button>，里面又嵌了「余额/充值」<button>——非法 HTML
                  // （React 报 "<button> cannot be a descendant of <button>"）。改成外层一个
                  // flex 容器（div）承载边框/高亮样式，「选择供应商」和「余额/充值」是它的两个
                  // 兄弟 <button>，不再互相嵌套；data-action-id 保留在「选择供应商」按钮上。
                  <div
                    key={p.id}
                    className={cn(
                      "w-full flex items-center gap-2 rounded-lg border transition-colors",
                      on
                        ? "border-accent bg-accent/[0.10] ring-1 ring-inset ring-accent"
                        : "border-white/[0.06] bg-white/[0.02] hover:bg-white/[0.05]",
                    )}
                  >
                    <button
                      data-action-id="runtime.driver.apply"
                      onClick={() => void applyProvider(openMenu.tool, openMenu.target, p)}
                      className="min-w-0 flex-1 flex items-center gap-2 pl-2.5 py-2 text-left"
                    >
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-1.5">
                          <span className="text-[12px] font-medium text-ink-1 truncate">{p.name}</span>
                          {on && (
                            <span className="text-[9px] px-1.5 h-[15px] inline-flex items-center rounded-full bg-accent text-white shrink-0">
                              {tr("使用中")}
                            </span>
                          )}
                          {p.builtin_recharge && (
                            <span className="text-[9px] text-accent-400 shrink-0">{tr("内置")}</span>
                          )}
                        </span>
                        <span className="block text-[10px] text-ink-4 font-mono truncate">
                          {hostOf(p.anthropic_base || p.openai_base)}
                        </span>
                      </span>
                    </button>
                    {p.builtin_recharge && (
                      <button
                        onClick={() => {
                          closeMenu();
                          onRecharge();
                        }}
                        className="shrink-0 inline-flex items-center gap-0.5 pr-2.5 py-2 text-[10px] text-accent-400 hover:text-accent"
                      >
                        <Wallet size={10} /> {tr("余额/充值")}
                      </button>
                    )}
                  </div>
                );
              })
            )}
            <button
              onClick={() => {
                closeMenu();
                onManageProviders(undefined, openMenu.target);
              }}
              className="w-full flex items-center gap-1.5 px-2.5 py-2 rounded-lg text-[11.5px] text-ink-4 hover:text-accent-400 hover:bg-white/[0.03]"
            >
              <Plus size={13} /> {tr("自定义供应商")}
            </button>
            <button
              onClick={() => {
                closeMenu();
                onGoManage();
              }}
              className="w-full flex items-center gap-1.5 px-2.5 py-2 rounded-lg text-[11.5px] text-ink-4 hover:text-accent-400 hover:bg-white/[0.03]"
            >
              <Settings2 size={13} /> {tr("更多模型设置")}
            </button>
          </div>
        </AnchoredMenu>
      )}
    </div>
  );
}

/** 卡片上「版本号」——只在零成本时有（`ToolDiscovery.version`，同目录/上级目录有
 *  package.json 才填），拿不到就不显示，不猜一个假版本号出来。 */
function primaryDiscoveryVersion(t: ToolInfo, driver: DriverStatus | null): string | null {
  const name = discoveryNameFor(t.id);
  return driver?.discovered?.find((d) => d.name === name)?.version ?? null;
}
