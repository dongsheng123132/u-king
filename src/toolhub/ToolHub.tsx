/**
 * 「我的 AI」—— 已装工具 Launchpad 式 logo 墙：软件是主角,不是启动按钮。
 *
 * 定位区别（跟侧栏另一个入口「装机 · 体检」`MyAI`(App.tsx) 分工——2026-09-29 首页改版，
 * 两边都改了名，id 和页面实现都没动）：
 *  · `myai`（侧栏「装机 · 体检」）是**装机漏斗**——引导装、体检、卸载，给刚接触 U-King 的人看。
 *  · 这里（`toolhub`，侧栏现在也叫「我的 AI」）是**日常启动台**——已经装好工具的人天天回来的
 *    落脚点：看一眼装了什么、换个模型、点一下启动。也是全站默认落地页（见 App.tsx
 *    `refresh()` 里的落点逻辑：首次打开和回访统一落这里）。
 * 两者共用同一批数据源（`tools`/`driver`/`deviceKey`）和同一条业务通路
 * （`launchTool`/`openTool`/`apply_provider`/`uninstallTool`，均由 App.tsx 传入，本组件不
 * 重新实现）。
 *
 * 布局（2026-09-29 二次改版——去掉「一屏全是启动按钮」的大卡片网格）：
 * 已装/可装两个分区都是 logo 瓷砖网格（`TileGrid`，Launchpad 观感：大图标 + 名字，瓷砖本身
 * 不放任何按钮）。点一下瓷砖，在**它所在那一整行**的下方插入一条全宽详情条（Steam/Netflix
 * 那种在所选行下面展开的详情），里面才有换模型、启动、卸载这些操作；再点一下同一块瓷砖，或按
 * Esc，详情收起。双击已装瓷砖＝直接启动（跳过详情，给熟手抄近路；可装瓷砖目前没有双击直装，
 * 只有单击展开详情里的「安装」按钮——复审 medium 修复：旧注释说「双击瓷砖＝直接启动/安装」，
 * 但可装瓷砖从来没绑过 `onDoubleClick`，注释与实现对不上，这里改成如实描述）。已装瓷砖的单击
 * 选中会先等一拍浏览器判定"这是不是双击"的窗口再真正生效（见 `commitTileClick`），不是点哪个
 * 都立刻挪动布局——双击一个未选中的瓷砖时，浏览器会连续派发 click→click→dblclick，如果每次
 * click 都立刻挪详情条，会出现"详情闪一下又收起，中途还可能把详情条挪到别的瓷砖上方，
 * 让第二下 click/dblclick 落在挪位后新出现的别的按钮上"（复审 medium：确认属实的竞态，
 * 曾经拿分体按钮"体检修复"这类次要按钮举例）。已装/可装两个网格共享
 * 「同一时刻只展开一个」——因为一个工具只可能在其中一个网格里。
 * 详情条的水平位置和插入点都是算出来的，不是猜的：`TileGrid` 用 `ResizeObserver` 量自己的
 * 容器宽度换算每行能摆几列，据此定位「该在第几个瓷砖后面插详情条」以及「小三角该落在哪个
 * 横坐标」——视口一变、换行了，这两个数字跟着重算，详情条永远贴着被点的那块瓷砖。
 *
 * 可插拔：整个模块收在 `src/toolhub/` 下，App.tsx 只在两处接它——侧栏 `CORE` 数组的一条
 * `NavItem` 和主区 `tab === "toolhub"` 的一个分支——删掉这个目录、去掉这两处即可完整移除
 * （宪法「模块独立可插拔」条）。
 *
 * ActionParity：本组件不写任何新业务逻辑，纯粹是对已有 Tauri 命令（`list_providers` /
 * `apply_provider` / `uninstall_ai_tool`）、已有只读动作（`runtime.tool.inspect`——只用来读
 * 每个工具的 `LaunchMode`，判断详情条该显示单按钮还是分体按钮，不重新判断"能不能启动"，
 * 那件事仍然只有 `runtime.tool.launch` 说了算）和已有回调（`onLaunch`/`onOpen`/`onUninstall`）
 * 的又一层界面，换模型那段逻辑（`applyProvider`/`openModelMenu`/`providersByTarget` 缓存）跟
 * `ProviderSwitch.tsx` 走的是同一条后端路径，原样照搬，没有引入新的写法——这次改版只是把它
 * 从「常驻在每张卡片上」挪进「点开才看到的详情条」。
 */
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ChevronDown,
  Cpu,
  Download,
  LayoutGrid,
  Play,
  Plus,
  RefreshCw,
  Rocket,
  Settings2,
  Sparkles,
  Stethoscope,
  Trash2,
  Wallet,
} from "lucide-react";
import {
  LAB_TOOLS,
  toolTargets,
  currentModelFor,
  modelReadbackState,
  needsEffectiveReadback,
  discoveryNameFor,
  canUninstallTool,
  type ToolInfo,
} from "../App";
import type { DeviceKey, DriverStatus, EffectiveConfig } from "../lib/types";
import type { ProviderPreset } from "../Wizard";
import { ToolIcon } from "../components/ToolIcon";
import { AnchoredMenu } from "../components/AnchoredMenu";
import { providerKeyFor } from "../components/ProviderSwitch";
import type { LaunchPlan } from "../components/LaunchBlocked";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";
import { getLaunchPref, setLaunchPref, type LaunchPref } from "./launchPref";
import { buildProviderRepairPrompt, type ProviderRepairPromptInput } from "../lib/providerRepairPrompt";
import { ACTION, createTauriActionClient } from "../generated/action-client";

const callAction = createTauriActionClient(invoke, { surface: "gui" });

type Category = "all" | "cli" | "gui" | "agent" | "lab";

/** 「上次启动的工具」——首次渲染默认展开哪个详情条就靠它（见组件里的默认展开 effect）。
 *  只在这一个文件读写，别处不要另开一份同名 key（宪法第 8 条）。 */
const LAST_TOOL_KEY = "uking.toolhub.lastTool";

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

/** 详情条的 DOM id——瓷砖 `aria-controls` 和详情条外壳的 `id` 各写一次，用同一个函数拼，
 *  不让两处各自拼字符串漂开（宪法第 8 条）。已装/可装两个网格是分开渲染的、一个工具只会
 *  出现在其中一个网格里，不会有两个详情条抢同一个 id 的情况。 */
function detailIdFor(toolId: string): string {
  return `toolhub-detail-${toolId}`;
}

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

/**
 * 读回 pi / opencode 这类 `DriverStatus` 没有 *_model 字段的工具「当前真正会跑的模型」——
 * 复用只读动作 `runtime.provider.effective`（回读工具**自己的**配置文件，不新增任何后端能力）。
 * 不带 `target` = 一次把它认的全部目标回读回来（都是读几个小配置文件，比按 target 各调一次省往返）。
 *
 * 🔴 `wanted` 里没被回读到的 target（动作的 target enum 不含它，如 qwen / crush）、动作整个失败、
 * 返回 `ok:false`，都落成 `readable:false` = 「不知道」。**绝不留空位**（空位在界面上是「还在读」，
 * 会永远停在「读取中…」），也绝不编一个 model。
 */
async function readEffective(wanted: string[]): Promise<Record<string, EffectiveConfig>> {
  const out: Record<string, EffectiveConfig> = {};
  try {
    const env = await callAction(ACTION.RUNTIME_PROVIDER_EFFECTIVE, {});
    if (env.ok) {
      const list = (env.result as unknown as { targets?: EffectiveConfig[] }).targets ?? [];
      for (const e of list) out[e.target] = e;
    }
  } catch {
    /* 回读失败不该影响页面——下面统一把 wanted 补成「不知道」 */
  }
  for (const target of wanted) {
    if (!out[target]) {
      out[target] = { target, readable: false, provider_key: null, base_url: null, model: null, overridden_by: null };
    }
  }
  return out;
}

export function ToolHub({
  tools,
  driver,
  deviceKey,
  onLaunch,
  onOpen,
  onUninstall,
  onGoManage,
  onManageProviders,
  onRecharge,
  onRefreshTools,
  onToast,
  onAskAiToFix,
  onGoSetup,
  onGoDoctor,
  onGoChat,
  onGoTermWb,
}: {
  tools: ToolInfo[];
  driver: DriverStatus | null;
  deviceKey: DeviceKey | null;
  /** 已装工具的启动通路（=App.tsx 的 `launchTool`，含运行时判定 + U-CLI/系统终端分流）。 */
  onLaunch: (t: ToolInfo) => void;
  /** 未装工具的一键安装通路 / GUI 应用直开（=App.tsx 的 `openTool`）。 */
  onOpen: (t: ToolInfo) => void;
  /** 详情条「卸载」——彻底卸载某个 AI 工具（=App.tsx 的 `uninstallTool`，含残留清理 + 二次
   *  确认，那套破坏性弹窗逻辑一个字没动，本组件只负责在能卸载的工具详情条里露出这个入口，
   *  能不能卸载看 `canUninstallTool`，跟侧栏「装机 · 体检」`MyAI` 共用同一份名单）。 */
  onUninstall: (t: ToolInfo) => void;
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
   * 目前这条链接接的是每个详情条自己 try/catch 得到的那次失败：换模型驱动时
   * `apply_provider` 报错。
   */
  onAskAiToFix?: (prompt: string) => void;
  /** 空态引导「一键装好推荐组合」→ App.tsx 的 `startInstallAll`（装机向导，预选 "all"，
   *  真正排队装好 Claude Code + 必要环境）。**不是**单纯 `setTab("setup")`——那样进页面
   *  只会按「一个 AI 都没装」条件自动拉起「逐个选装」，跟按钮文案「一键」对不上
   *  （复审 medium #6：这是曾经的回归，2026-09-29 改回）。 */
  onGoSetup: () => void;
  /** 页头次要链接「装机 · 体检 →」、详情条「体检修复」→ App.tsx 的 `setTab("myai")`
   *  （装机漏斗 + 体检/卸载）。 */
  onGoDoctor: () => void;
  /** 详情条「想在 U-King 里用？」→「对话工作台」→ App.tsx 的 `setTab("chat")`。 */
  onGoChat: () => void;
  /** 同上，→「终端工作台」→ App.tsx 的 `setTab("termwb")`。 */
  onGoTermWb: () => void;
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

  // 「当前模型」里 DriverStatus 读不到的那批（有 target、但 DriverStatus 没有它的 *_model 字段——
  // 目前是 pi / opencode）：有这类工具已装才去调一次只读回读动作，按 target 缓存；一个都没装
  // 就一次都不调。`driver` 进依赖是因为换完模型后 `onRefreshTools()` 会换一份新 driver，借它
  // 触发重读（ToolHub 是 `tab === "toolhub"` 条件挂载的，离开再回来整个重新挂载，也会重读）。
  // 缓存里**没有**某个 target = 还在读（`modelReadbackState` 的 `pending`）；读不了落成
  // `readable:false`（`readEffective` 保证不留空位）。
  const [effectiveByTarget, setEffectiveByTarget] = useState<Record<string, EffectiveConfig>>({});
  const effectiveWantedKey = useMemo(
    () =>
      [...new Set(installed.filter(needsEffectiveReadback).map((t) => t.config_target as string))]
        .sort()
        .join(","),
    [installed],
  );
  useEffect(() => {
    if (!effectiveWantedKey) return;
    let alive = true;
    void readEffective(effectiveWantedKey.split(",")).then((m) => {
      if (alive) setEffectiveByTarget((prev) => ({ ...prev, ...m }));
    });
    return () => {
      alive = false;
    };
  }, [effectiveWantedKey, driver]);

  // 当前展开详情的工具 id——已装/可装两个网格共享这一个状态（同一时刻只展开一个，
  // 因为一个工具只可能出现在其中一个网格里）。分类 chip 切走后如果选中的可装工具不再在
  // `installableFiltered` 里，下面渲染时按「不在当前筛选结果里就不展开」处理，不必单独清理
  // 这份状态——用户切回原分类，详情会自己重新出现，比强制清空更顺手。
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const installableSelectedId = installableFiltered.some((t) => t.id === selectedId) ? selectedId : null;

  function toggleSelect(id: string) {
    setSelectedId((cur) => (cur === id ? null : id));
    // 切换选中的瓷砖时顺手关掉两个悬浮菜单——不这样做的话，`openMenu`/`splitMenuTool`
    // 还指着刚刚收起的那张详情条，它们的触发按钮已经从 DOM 里消失，`AnchoredMenu` 量不到
    // 锚点位置会一直停在 `visibility:hidden`，但它无条件渲染的遮罩层（`fixed inset-0`）
    // 还在吃全屏点击——一个看不见但挡点击的浮层，比崩溃更难查。
    closeMenu();
    setSplitMenuTool(null);
  }

  // 默认展开「上次启动的工具」（localStorage，`handleLaunch` 每次真正启动时写入）；没有记录
  // 就展开第一个已装工具；已装为 0 时不展开。只在「已装列表第一次变成非空」时跑一次
  // （`didDefaultExpand` 挡住后续 `installed` 变化——不然每次刷新工具列表都会把用户手动
  // 收起的详情条重新弹开）。用 `useLayoutEffect` 而不是 `useEffect`——放在 passive effect
  // 里的话，首帧会先画一遍"没有任何详情条展开"的窄布局，等这个 effect 跑完才在下一帧弹出
  // 默认详情条，肉眼可见跳一下（复审修复：确认属实的首帧闪动，跟下面 `TileGrid` 列数计算
  // 是同一类问题）。`useLayoutEffect` 在浏览器绘制前同步跑完、`setSelectedId` 触发的重渲染
  // 也会在同一次提交里做完，首帧看到的就是最终展开状态。
  const didDefaultExpand = useRef(false);
  useLayoutEffect(() => {
    if (didDefaultExpand.current) return;
    if (installed.length === 0) return;
    didDefaultExpand.current = true;
    let last: string | null = null;
    try {
      last = localStorage.getItem(LAST_TOOL_KEY);
    } catch {
      /* localStorage 不可用（隐私模式等）时静默回退默认值 */
    }
    setSelectedId(last && installed.some((t) => t.id === last) ? last : installed[0].id);
  }, [installed]);

  // 真正启动一个工具——记「上次启动」+ 调用真正的启动通路。瓷砖双击、详情条主按钮、
  // 分体按钮的两个子项都走这一个函数，不各写一份（默认展开要读的就是这里写的记录）。
  function handleLaunch(t: ToolInfo) {
    try {
      localStorage.setItem(LAST_TOOL_KEY, t.id);
    } catch {
      /* 写不进去就只影响下次默认展开，不阻断这次启动 */
    }
    onLaunch(t);
  }

  // 已装瓷砖单击/双击消歧——浏览器双击会先后派发 click→click→dblclick，如果每次 click 都
  // 立刻 `toggleSelect`，双击一个未选中的瓷砖就会变成「开详情→关详情→启动」，中途详情条还可能
  // 挪到别的瓷砖上方，让第二下 click/dblclick 落在挪位后冒出来的别的按钮上（复审 medium：确认
  // 属实的竞态）。这里把「真的挪动详情条」延后一个跟浏览器 dblclick 判定窗口对齐的计时器：
  // 这段时间内没等到第二次点击才真的 `toggleSelect`；等到了，`commitTileDoubleClick` 直接吃掉
  // 这次单击意图、原样启动——从头到尾没有过一次布局挪动，不存在"点错到挪位后别的按钮"的窗口。
  // 代价是单击展开详情会有一拍（跟浏览器 dblclick 阈值一致，通常 300-500ms）的观感延迟，
  // 换来双击场景不再有错位点击的正确性——只装了 `onDoubleClick` 的已装瓷砖需要这层消歧，
  // 可装瓷砖没有双击语义，`toggleSelect` 原样立即调用即可。
  const tileClickTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (tileClickTimer.current != null) window.clearTimeout(tileClickTimer.current);
    },
    [],
  );
  function commitTileClick(id: string) {
    if (tileClickTimer.current != null) window.clearTimeout(tileClickTimer.current);
    tileClickTimer.current = window.setTimeout(() => {
      tileClickTimer.current = null;
      toggleSelect(id);
    }, 300);
  }
  function commitTileDoubleClick(t: ToolInfo) {
    if (tileClickTimer.current != null) {
      window.clearTimeout(tileClickTimer.current);
      tileClickTimer.current = null;
    }
    handleLaunch(t);
  }

  // 每个详情条自己的「换模型」下拉——同一时刻只开一个，跟 AnchoredMenu 本身的假设一致。
  const [openMenu, setOpenMenu] = useState<{ tool: ToolInfo; target: string } | null>(null);
  const anchorMap = useRef<Record<string, HTMLButtonElement | null>>({});
  const closeMenu = () => setOpenMenu(null);

  // 供应商列表按 target 缓存、首次打开对应下拉时才拉——不是一进页面就把四个工具的
  // 供应商全拉一遍（早年 `list_providers` 会真的探测/读盘，白拉浪费）。这份缓存挂在
  // ToolHub 本身而不是详情条上——详情条随选中的工具换成换出（`key={t.id}` 效果），
  // 缓存留在这一层才能「关掉详情条再点开同一个工具」不用重新拉一次。
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

  // 应用中 / 报错——都是 per-tool 的（Record 而不是单个全局标记），因为不同详情条的
  // 换模型互不相干：一个应用中不该拦住另一个也能点。
  const [applyingIds, setApplyingIds] = useState<Set<string>>(new Set());
  const [applyFailures, setApplyFailures] = useState<Record<string, ProviderRepairPromptInput>>({});

  function openModelMenu(t: ToolInfo) {
    const target = toolTargets(t)[0];
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
      await invoke("apply_provider", { providerId: p.id, apiKey: key, model: null, targets: toolTargets(t) });
      // ClawX 不热重载配置文件（运行时持有内存副本，退出会覆写）——切完必须重启 ClawX 才生效。
      // 跟 `ProviderSwitch.tsx::doSwitch` 用同一句提示（`clawxHint`），别在这重新造一句漂移的文案。
      const clawxHint = toolTargets(t).includes("clawx") ? tr("，请重启 ClawX 生效") : "";
      // DSH 同理：U-King 写 ~/.dsh/settings.yaml，DSH 只在启动时导入，开着切要重启才生效（同 ProviderSwitch 的 dshHint）。
      const dshHint = toolTargets(t).includes("dsh") ? tr("，重启 DSH 后生效") : "";
      const restartHint = clawxHint + dshHint;
      onToast(
        p.id === "official"
          ? tr("已还原官方配置{hint}", { hint: restartHint })
          : tr("已切到 {name}{model}{hint}", { name: p.name, model: "", hint: restartHint }),
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
          model: currentModelFor(t, driver, effectiveByTarget) ?? "",
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

  // 命令行工具「启动方式」偏好（系统终端 / U-CLI）——纯前端全局偏好，见 `launchPref.ts`
  // 顶部注释。2026-09-29 改版把它的开关从页底常驻分段控件挪进详情条的分体按钮，读写逻辑
  // 一个字没动。
  const [launchPref, setLaunchPrefState] = useState<LaunchPref>(getLaunchPref);
  const setLaunchPrefAndPersist = (v: LaunchPref) => {
    setLaunchPrefState(v);
    setLaunchPref(v);
  };

  // 每个工具真正的启动方式（`tools::LaunchMode`，经 `runtime.tool.inspect` 序列化过来）——
  // 只有 `LaunchMode::EmbeddedPty` 才会读 `launchPref` 分流到系统终端/U-CLI；`RouteTab`
  // （如 hermes，跳 U-King 内的专属页）和 `ExternalTerm`（如 harness-doctor，后端无条件开
  // 系统终端）不受这个偏好影响，「打开方式」文案和分体 ▾ 只应该对 EmbeddedPty 那批工具出现
  // （复审 medium 修复：旧版只用 `t.launch_app` 是否非空二分"桌面应用/走 launchPref"，
  // 漏了 RouteTab/ExternalTerm 这两类既非 GUI、又不听 launchPref 的工具，详情条会说一句不真实
  // 的「打开方式」，分体 ▾ 里两项效果还完全一样、点了还顺手改掉全局偏好）。跟 `ToolAppView.tsx`
  // 同一个只读动作、同一种一次性拉全量再按 id 查表的写法，不新开一条判定逻辑；只在挂载时拉
  // 一次——`LaunchMode` 是编译期定死在 `TOOL_SPECS` 里的常量，不会随驱动/安装状态变化，没必要
  // 跟着 `tools`/`onRefreshTools` 重新拉。
  const [launchModes, setLaunchModes] = useState<Record<string, LaunchPlan["mode"]>>({});
  useEffect(() => {
    let alive = true;
    callAction(ACTION.RUNTIME_TOOL_INSPECT, {})
      .then((env) => {
        if (!alive || !env.ok) return;
        const list = (env.result as unknown as { tools: LaunchPlan[] }).tools ?? [];
        const map: Record<string, LaunchPlan["mode"]> = {};
        for (const p of list) map[p.tool_id] = p.mode;
        setLaunchModes(map);
      })
      .catch(() => {
        /* 拉不到就维持空表——下面渲染对"未知模式"有安全的兜底（只显示单按钮，不猜分体） */
      });
    return () => {
      alive = false;
    };
  }, []);

  // 分体按钮右侧 ▾ 弹出的「在系统终端打开 / 在 U-CLI 打开」——只有两项的静态菜单，
  // 不需要像换模型下拉那样跨工具缓存，存整个 ToolInfo 而不是 id，点了直接拿来启动。
  const [splitMenuTool, setSplitMenuTool] = useState<ToolInfo | null>(null);
  const splitAnchorRef = useRef<HTMLButtonElement | null>(null);

  // Esc——分两档，不是无条件收详情条：换模型下拉或分体 ▾ 菜单开着时，Esc 该做的是
  // 「先关掉浮在最上面那层」（跟大多数弹出菜单的 Esc 习惯一致），这时候如果连详情条也一起
  // 收掉，用户会觉得「按一下 Esc 页面整个塌了」——两个菜单都关着，Esc 才轮到收起详情条
  // （复审修复：确认属实，旧版不分层，菜单开着按 Esc 会连详情条一起收）。放在这个位置
  // （`openMenu`/`closeMenu`/`splitMenuTool` 都已声明之后）不是随手挪的——`deps` 数组是
  // `useEffect` 调用时立刻求值的字面量，不是延迟到事件触发才读，放在这几个 `const` 声明
  // 之前会在渲染期直接踩 TDZ（"Cannot access before initialization"），跟 `toggleSelect`/
  // 原 Esc effect 内部函数体里引用后声明的 `closeMenu` 不是一回事——那些是"函数体里引用"，
  // 真正执行时机在事件触发之后，组件函数早已跑完、所有 `const` 都已初始化。
  useEffect(() => {
    if (!selectedId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (openMenu || splitMenuTool) {
        closeMenu();
        setSplitMenuTool(null);
        return;
      }
      setSelectedId(null);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [selectedId, openMenu, splitMenuTool]);

  const refreshAll = async () => {
    setRefreshing(true);
    try {
      await onRefreshTools();
    } finally {
      setRefreshing(false);
    }
  };

  /** 已装瓷砖——图标 + 名字 + 一行极小灰字（当前模型 / 「桌面应用」）,不放任何按钮。 */
  function renderInstalledTile(t: ToolInfo, registerRef: (el: HTMLButtonElement | null) => void) {
    const isSelected = selectedId === t.id;
    const model = currentModelFor(t, driver, effectiveByTarget);
    const subtitle = model || (t.launch_app ? tr("桌面应用") : "");
    return (
      <button
        ref={registerRef}
        data-testid="toolhub-tile"
        data-tool-id={t.id}
        aria-expanded={isSelected}
        aria-controls={detailIdFor(t.id)}
        onClick={() => commitTileClick(t.id)}
        onDoubleClick={() => commitTileDoubleClick(t)}
        className={cn(
          "flex flex-col items-center gap-1.5 rounded-xl border p-2.5 text-center transition-colors",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
          isSelected ? "border-accent bg-accent/[0.10]" : "border-transparent hover:bg-white/[0.04] hover:border-white/[0.10]",
        )}
      >
        <span className="grid place-items-center w-16 h-16 rounded-2xl bg-bg-3">
          <ToolIcon tool={t.id} size={56} active />
        </span>
        <span className="w-full text-[12px] font-medium text-ink-0 truncate">{t.name}</span>
        <span className="w-full text-[10px] text-ink-5 truncate">{subtitle || " "}</span>
      </button>
    );
  }

  /** 可装瓷砖——图标去饱和（`ToolIcon active=false` 自带 grayscale+透明度，不用另写一套）
   *  + 右下角一个小下载徽标；下面一行极小灰字是分类名（跟旧版可装 tile 同一份展示，
   *  这次改版没人要求去掉它）。同样不放任何按钮。 */
  function renderInstallableTile(t: ToolInfo, registerRef: (el: HTMLButtonElement | null) => void) {
    const isSelected = selectedId === t.id;
    const catLabel = CATS.find((c) => c.id === categoryOf(t))?.label ?? "";
    return (
      <button
        ref={registerRef}
        data-testid="toolhub-tile"
        data-tool-id={t.id}
        aria-expanded={isSelected}
        aria-controls={detailIdFor(t.id)}
        onClick={() => toggleSelect(t.id)}
        className={cn(
          "flex flex-col items-center gap-1.5 rounded-xl border p-2.5 text-center transition-colors",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
          isSelected ? "border-accent bg-accent/[0.10]" : "border-transparent hover:bg-white/[0.04] hover:border-white/[0.10]",
        )}
      >
        <span className="relative grid place-items-center w-16 h-16 rounded-2xl bg-bg-3">
          <ToolIcon tool={t.id} size={56} active={false} />
          <span className="absolute -bottom-1 -right-1 grid place-items-center w-5 h-5 rounded-full bg-bg-1 border border-white/[0.14] text-ink-4">
            <Download size={10} />
          </span>
        </span>
        <span className="w-full text-[12px] font-medium text-ink-2 truncate">{t.name}</span>
        <span className="w-full text-[10px] text-ink-5 truncate">{tr(catLabel)}</span>
      </button>
    );
  }

  /** 详情条的外壳——边框卡片 + 顶部指向所选瓷砖水平中心的小三角。`caretLeft` 是相对于
   *  网格容器左边缘的像素值（由 `TileGrid` 量出来），为 `null` 时（还没测出位置的第一帧）
   *  不画三角，避免闪一下错误位置。 */
  function detailShell(t: ToolInfo, caretLeft: number | null, body: ReactNode) {
    return (
      <div className="relative pt-3">
        {caretLeft != null && (
          <div
            data-testid="toolhub-detail-caret"
            className="absolute top-[6px] h-3 w-3 rotate-45 border-l border-t border-ink-5/40 bg-bg-1"
            style={{ left: caretLeft, transform: "translate(-50%, 0) rotate(45deg)" }}
          />
        )}
        <div
          id={detailIdFor(t.id)}
          data-testid="toolhub-detail"
          data-tool-id={t.id}
          role="region"
          aria-label={tr("{name} 详情", { name: t.name })}
          className="rounded-card border border-ink-5/30 bg-bg-1 p-4 space-y-3"
        >
          {body}
        </div>
      </div>
    );
  }

  /** 已装工具详情条：左侧 logo/名称/版本/打开方式 + 换模型下拉（逻辑原样搬自旧版卡片）+
   *  报错横幅 + 启动/打开按钮（CLI 工具是分体按钮）+ 次要操作 + 「想在 U-King 里用？」引导。 */
  function renderInstalledDetail(t: ToolInfo) {
    const model = currentModelFor(t, driver, effectiveByTarget);
    // 当前模型为空时到底是「没配」还是「读不到」——见 `modelReadbackState`；读不到不能喊「还没配模型」。
    const readback = modelReadbackState(t, effectiveByTarget);
    const targets = toolTargets(t);
    const target = targets[0];
    // 读不到模型时（pi/opencode 配置损坏、被 jsonc 挡住等）退而求其次：已知「使用中」的供应商名
    // 就显示它（名字要等换模型下拉拉过 `list_providers` 才有，没有就不显示——宁缺勿编一个 id 出来）。
    const activeProviderId = target ? driver?.active?.[target] : undefined;
    const activeProviderName = activeProviderId
      ? providersByTarget[target]?.find((p) => p.id === activeProviderId)?.name
      : undefined;
    const version = primaryDiscoveryVersion(t, driver);
    const applying = applyingIds.has(t.id);
    const failure = applyFailures[t.id];
    // 真正的启动方式（`tools::LaunchMode`，见上面 `launchModes` 的注释）——只有它是
    // `embedded_pty` 才读 `launchPref`、显示分体 ▾；`route_tab`/`external_term` 各自固定一句
    // 真实文案、单按钮；还没拉到（`mode` 为 `undefined`）时按 `t.launch_app` 是否非空兜底判断
    // 桌面应用（这条兜底跟 `gui_app` 的现有数据口径一致，不会猜错），其余场景宁可先不显示
    // 「打开方式」小字、也不猜一句可能不真实的话。
    const mode = launchModes[t.id];
    const isGuiApp = mode ? mode === "gui_app" : !!t.launch_app;
    const isEmbeddedPty = mode === "embedded_pty";
    const openMethodText =
      mode === "gui_app"
        ? tr("桌面应用")
        : mode === "route_tab"
          ? tr("在 U-King 里打开")
          : mode === "external_term"
            ? tr("在系统终端打开")
            : mode === "embedded_pty"
              ? launchPref === "system"
                ? tr("在系统终端打开")
                : tr("在 U-CLI 打开")
              : isGuiApp
                ? tr("桌面应用")
                : "";

    return (
      <>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0">
            <span className="grid place-items-center w-16 h-16 rounded-2xl bg-bg-3 shrink-0">
              <ToolIcon tool={t.id} size={48} active />
            </span>
            <div className="min-w-0">
              <div className="text-[15px] font-semibold text-ink-0 truncate">{t.name}</div>
              {version && <div className="text-[11px] text-ink-5 font-mono truncate">v{version}</div>}
              {openMethodText && <div className="text-[10.5px] text-ink-4 truncate">{openMethodText}</div>}
            </div>
          </div>

          <div className="flex flex-col items-end gap-1 shrink-0">
            {isGuiApp ? (
              <button
                data-action-id="runtime.tool.launch"
                data-testid="toolhub-launch-main"
                data-tool-id={t.id}
                onClick={() => handleLaunch(t)}
                disabled={applying}
                className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-60"
              >
                <Play size={14} /> {applying ? tr("正在应用配置…") : tr("打开")}
              </button>
            ) : isEmbeddedPty ? (
              <>
                <div className="inline-flex rounded-lg overflow-hidden">
                  <button
                    data-action-id="runtime.tool.launch"
                    data-testid="toolhub-launch-main"
                    data-tool-id={t.id}
                    onClick={() => handleLaunch(t)}
                    disabled={applying}
                    className="inline-flex items-center gap-1.5 px-4 h-9 bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-60"
                  >
                    <Play size={14} /> {applying ? tr("正在应用配置…") : tr("启动")}
                  </button>
                  <button
                    ref={splitAnchorRef}
                    data-testid="toolhub-launch-split-trigger"
                    data-tool-id={t.id}
                    aria-haspopup="menu"
                    aria-expanded={splitMenuTool?.id === t.id}
                    onClick={() => setSplitMenuTool((cur) => (cur?.id === t.id ? null : t))}
                    disabled={applying}
                    className="inline-flex items-center justify-center w-7 h-9 bg-accent border-l border-white/20 text-white hover:bg-accent-600 disabled:opacity-60"
                  >
                    <ChevronDown size={13} />
                  </button>
                </div>
                <span className="text-[10px] text-ink-5">{openMethodText}</span>
              </>
            ) : (
              // `route_tab`（如 hermes）/`external_term`（如 harness-doctor）/`mode` 还没拉到：
              // 都不听 `launchPref`，只给一个单按钮，不装一个"选了也没区别、还会顺手改掉全局
              // 偏好"的假 ▾（复审 medium：确认属实）。
              <>
                <button
                  data-action-id="runtime.tool.launch"
                  data-testid="toolhub-launch-main"
                  data-tool-id={t.id}
                  onClick={() => handleLaunch(t)}
                  disabled={applying}
                  className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-60"
                >
                  <Play size={14} /> {applying ? tr("正在应用配置…") : tr("启动")}
                </button>
                {openMethodText && <span className="text-[10px] text-ink-5">{openMethodText}</span>}
              </>
            )}
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
            className="w-full flex items-center gap-1.5 rounded-lg border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] px-2 h-8 text-[11.5px] disabled:opacity-60"
          >
            <Cpu size={12} className="text-accent/70 shrink-0" />
            {model ? (
              <span className="flex-1 min-w-0 truncate text-ink-2 text-left">{model}</span>
            ) : readback === "readable" ? (
              <span className="flex-1 min-w-0 truncate text-warning-500 text-left">{tr("还没配模型")}</span>
            ) : readback === "pending" ? (
              <span className="flex-1 min-w-0 truncate text-ink-4 text-left">{tr("读取中…")}</span>
            ) : (
              // 读不到（不是没配）：不说「还没配模型」，也不编模型名。
              <span className="flex-1 min-w-0 truncate text-ink-4 text-left">
                {activeProviderName ? tr("使用中：{name}", { name: activeProviderName }) : tr("选择模型供应商")}
              </span>
            )}
            <ChevronDown size={12} className="text-ink-5 shrink-0" />
          </button>
        ) : model ? (
          // target 为空但 currentModelFor 有值（没有 config_target、却读得到模型的工具——
          // 目前后端表里没有这种，保留给以后的只读模型行）：显示只读模型行。
          <div className="flex items-center gap-1.5 px-2 h-8 text-[11.5px] text-ink-2">
            <Cpu size={12} className="text-accent/70 shrink-0" />
            <span className="flex-1 min-w-0 truncate text-left">{model}</span>
          </div>
        ) : (
          // target 为空、currentModelFor 也拿不到值：如实说「这里换不了」，给个「AI 设置」
          // 的出口，别把用户卡死在这条详情里。
          <div className="flex items-center gap-1.5 px-2 h-8 text-[11.5px] text-ink-5">
            <Cpu size={12} className="shrink-0" />
            <span className="flex-1 min-w-0 truncate text-left">{tr("这里暂不能换它的模型")}</span>
            <button data-testid="toolhub-go-manage" onClick={onGoManage} className="shrink-0 text-accent-400 hover:text-accent hover:underline">
              {tr("AI 设置")}
            </button>
          </div>
        )}

        {failure && (
          <div className="rounded-lg border border-danger-500/40 bg-danger-500/[0.10] px-2.5 py-2">
            <p className="text-[10.5px] leading-snug font-medium text-danger-700 dark:text-danger-400">{failure.error}</p>
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

        <div className="flex flex-wrap items-center gap-1.5 pt-2 border-t border-white/[0.06]">
          <button
            data-testid="toolhub-go-doctor-detail"
            data-tool-id={t.id}
            onClick={onGoDoctor}
            className="inline-flex items-center gap-1 px-2 h-7 rounded-md text-[11.5px] text-ink-3 hover:text-accent-400 hover:bg-white/[0.04]"
          >
            <Stethoscope size={12} /> {tr("体检修复")}
          </button>
          {canUninstallTool(t.id) && (
            <button
              data-testid="toolhub-uninstall"
              data-action-id="runtime.aitool.uninstall"
              data-tool-id={t.id}
              onClick={() => onUninstall(t)}
              title={tr("彻底卸载 {name}（含 U-King 相关残留清理）", { name: t.name })}
              className="inline-flex items-center gap-1 px-2 h-7 rounded-md text-[11.5px] text-ink-5 hover:text-red-400 hover:bg-red-500/[0.06]"
            >
              <Trash2 size={12} /> {tr("卸载")}
            </button>
          )}
          <button
            data-testid="toolhub-go-manage-detail"
            data-tool-id={t.id}
            onClick={onGoManage}
            className="inline-flex items-center gap-1 px-2 h-7 rounded-md text-[11.5px] text-ink-3 hover:text-accent-400 hover:bg-white/[0.04]"
          >
            <Settings2 size={12} /> {tr("更多模型设置")}
          </button>
        </div>

        <div className="text-[11px] text-ink-5">
          {tr("想在 U-King 里用？")}{" "}
          <button data-testid="toolhub-go-chat" onClick={onGoChat} className="text-accent-400 hover:underline">
            {tr("对话工作台")}
          </button>
          {" · "}
          <button data-testid="toolhub-go-termwb" onClick={onGoTermWb} className="text-accent-400 hover:underline">
            {tr("终端工作台")}
          </button>
        </div>
      </>
    );
  }

  /** 未装工具详情条：大 logo + 名称 + 分类名 + 一个「安装」主按钮。不挂 `data-action-id`
   *  ——这里只是跳进装机向导/URL（`onOpen`），真正的安装动作在向导页里才发生，跟旧版
   *  可装 tile 的「安装」按钮同一个结论。`ToolInfo` 目前没有官网字段，没有就不显示、
   *  不编一个假链接出来。 */
  function renderInstallableDetail(t: ToolInfo) {
    const catLabel = CATS.find((c) => c.id === categoryOf(t))?.label ?? "";
    return (
      <div className="flex flex-wrap items-center gap-4">
        <span className="grid place-items-center w-16 h-16 rounded-2xl bg-bg-3 shrink-0">
          <ToolIcon tool={t.id} size={48} active={false} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-semibold text-ink-0 truncate">{t.name}</div>
          <div className="text-[11.5px] text-ink-4 truncate">{tr(catLabel)}</div>
        </div>
        <button
          data-testid="toolhub-install-btn"
          data-tool-id={t.id}
          onClick={() => onOpen(t)}
          className="shrink-0 inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600"
        >
          <Download size={14} /> {tr("安装")}
        </button>
      </div>
    );
  }

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
          <TileGrid
            tiles={installed}
            selectedId={selectedId}
            renderTile={renderInstalledTile}
            renderDetail={(t, caretLeft) => detailShell(t, caretLeft, renderInstalledDetail(t))}
          />
        </section>
      )}

      {installedLab.length > 0 && (
        // Open365 等「实验室」工具已装时的落脚点——它们不进上面「已安装」主网格（不算进
        // 「一键装好你的全部 AI」这条主线，见 App.tsx `LAB_TOOLS` 注释），但也不该点了就
        // 从这页彻底消失，所以单独收一条紧凑分区。点击复用 MyAI 里 `LabTools` 同一套判断
        // （装了 GUI 应用就直接打开，否则走 onOpen 的兜底），不走上面瓷砖/详情条那套复杂
        // 交互——这些工具本来就不接 U-King 的模型切换，保留原逻辑，这次改版没有碰它。
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
        {installableFiltered.length === 0 ? (
          <div className="rounded-card border border-dashed border-white/[0.12] px-4 py-6 text-center text-[12px] text-ink-4">
            {tr("这个分类下暂时没有工具")}
          </div>
        ) : (
          <TileGrid
            tiles={installableFiltered}
            selectedId={installableSelectedId}
            renderTile={renderInstallableTile}
            renderDetail={(t, caretLeft) => detailShell(t, caretLeft, renderInstallableDetail(t))}
          />
        )}
      </section>

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

      {splitMenuTool && (
        <AnchoredMenu anchorRef={splitAnchorRef} onClose={() => setSplitMenuTool(null)} minWidth={180}>
          <div className="p-1 space-y-0.5">
            <button
              data-testid="toolhub-launch-pref-system"
              data-action-id="runtime.tool.launch"
              onClick={() => {
                const t = splitMenuTool;
                setSplitMenuTool(null);
                setLaunchPrefAndPersist("system");
                handleLaunch(t);
              }}
              className="w-full text-left px-2.5 py-2 rounded-md text-[12px] text-ink-2 hover:bg-white/[0.06]"
            >
              {tr("在系统终端打开")}
            </button>
            <button
              data-testid="toolhub-launch-pref-ucli"
              data-action-id="runtime.tool.launch"
              onClick={() => {
                const t = splitMenuTool;
                setSplitMenuTool(null);
                setLaunchPrefAndPersist("ucli");
                handleLaunch(t);
              }}
              className="w-full text-left px-2.5 py-2 rounded-md text-[12px] text-ink-2 hover:bg-white/[0.06]"
            >
              {tr("在 U-CLI 打开")}
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

/**
 * 瓷砖网格——已装/可装两个分区共用同一份「量容器宽度算列数 → 在所选瓷砖那一行后面插入
 * 一条 col-span-full 详情条 → 顺带算出详情条三角该落在哪个横坐标」的布局逻辑，不各写一遍
 * （宪法第 8 条）。纯布局壳，不知道"瓷砖"和"详情"具体长什么样——那是调用方通过
 * `renderTile`/`renderDetail` 传进来的。
 *
 * 列数不用 Tailwind 的响应式断点类（`sm:grid-cols-3` 那种）——断点是离散的、说不出
 * "现在几列"，没法用来算"该在第几个瓷砖后面插详情条"。改成用 `ResizeObserver` 量自己的
 * `clientWidth`，按固定的单瓷砖最小宽度换算出整数列数，再用内联 `gridTemplateColumns`
 * 落地：同一个数字既定了 CSS 布局，也定了 JS 要在哪插入详情条、三角落在哪——两边不会漂。
 */
function TileGrid({
  tiles,
  selectedId,
  renderTile,
  renderDetail,
  minTile = 112,
  gap = 12,
}: {
  tiles: ToolInfo[];
  selectedId: string | null;
  renderTile: (t: ToolInfo, registerRef: (el: HTMLButtonElement | null) => void) => ReactNode;
  renderDetail: (t: ToolInfo, caretLeft: number | null) => ReactNode;
  /** 单个瓷砖的最小宽度（px）——列数按「容器宽度能塞下几个这么宽的瓷砖」换算。 */
  minTile?: number;
  /** 瓷砖间距（px），须跟外层 `className="gap-3"` 的实际值（12px）保持一致。 */
  gap?: number;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const tileRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [cols, setCols] = useState(1);
  const [caretLeft, setCaretLeft] = useState<number | null>(null);

  // 列数：按容器宽度重算，视口一变就跟着变（宽屏一行摆更多瓷砖）。用 `useLayoutEffect`
  // 而不是 `useEffect`——`cols` 初值是 1，如果在 passive effect 里量，浏览器会先画一帧
  // "单列堆叠"的布局，量出真实列数、`setCols` 生效后才跳回多列，肉眼可见闪一下（复审修复：
  // 确认属实的首帧闪动）。`useLayoutEffect` 在浏览器绘制前同步跑完、拿到真实列数再
  // `setCols`，触发的重渲染会在同一次提交里做完再交给浏览器画，首帧看到的就是最终布局。
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const compute = () => {
      const w = el.clientWidth;
      setCols(Math.max(1, Math.floor((w + gap) / (minTile + gap))));
    };
    compute();
    const ro = new ResizeObserver(compute);
    ro.observe(el);
    return () => ro.disconnect();
  }, [minTile, gap]);

  const selectedIndex = selectedId ? tiles.findIndex((t) => t.id === selectedId) : -1;
  const hasSelected = selectedIndex >= 0;

  // 三角水平位置：量所选瓷砖相对网格容器左边缘的中心横坐标——不是猜的百分比。用
  // `getBoundingClientRect` 而不是百分比换算，是因为网格列之间有 `gap`，百分比算出来的
  // "第 n 列中心"跟真实渲染位置会有肉眼可见的偏差（列越多、gap 占比越大偏差越明显）。
  useLayoutEffect(() => {
    if (!hasSelected || !selectedId) {
      setCaretLeft(null);
      return;
    }
    const compute = () => {
      const tileEl = tileRefs.current[selectedId];
      const gridEl = containerRef.current;
      if (!tileEl || !gridEl) return;
      const tRect = tileEl.getBoundingClientRect();
      const gRect = gridEl.getBoundingClientRect();
      setCaretLeft(tRect.left - gRect.left + tRect.width / 2);
    };
    compute();
    const el = containerRef.current;
    const ro = el ? new ResizeObserver(compute) : null;
    if (el && ro) ro.observe(el);
    window.addEventListener("resize", compute);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", compute);
    };
    // `selectedIndex`/`tiles` 补进依赖——`compute` 是从 `tileRefs.current[selectedId]` 现场量
    // DOM，理论上总能拿到当下的真实位置，但前提是这个 effect 真的重跑了一次。`selectedId`
    // 没变、只是它在 `tiles` 里的下标变了（刷新工具列表后顺序变化，或同一分类下增删了别的
    // 工具导致该工具换到别的列/别的行）时，光靠 `[selectedId, hasSelected, cols]` 挡不住——
    // 三个值都没变，effect 不会重跑，`caretLeft` 就停在刷新前的旧坐标，直到下一次真正触发
    // 重算的事件（resize / 列数变化）才纠正过来（复审修复：确认属实，刷新后换列三角不跟走）。
  }, [selectedId, hasSelected, cols, selectedIndex, tiles]);

  if (tiles.length === 0) return null;

  // 该在哪个瓷砖后面插详情条——所选瓷砖所在行的最后一个下标（末尾不满一行时夹到
  // `tiles.length - 1`，不然会算出一个不存在的下标）。
  const rowEndForSelected = hasSelected
    ? Math.min((Math.floor(selectedIndex / cols) + 1) * cols - 1, tiles.length - 1)
    : -1;

  return (
    <div ref={containerRef} className="grid gap-3" style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}>
      {tiles.map((t, i) => (
        <Fragment key={t.id}>
          {renderTile(t, (el) => {
            tileRefs.current[t.id] = el;
          })}
          {hasSelected && i === rowEndForSelected && (
            <div className="col-span-full">{renderDetail(tiles[selectedIndex], caretLeft)}</div>
          )}
        </Fragment>
      ))}
    </div>
  );
}
