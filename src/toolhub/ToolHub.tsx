/**
 * 「AI 工具中心」—— 参照 EchoBird 应用管理器：一屏走完「选工具 → 选模型 → 启动」。
 *
 * 定位区别（跟侧栏另一个入口「我的 AI」`MyAI`(App.tsx) 分工）：
 *  · `myai` 是**装机漏斗**——引导装、体检、卸载，给刚接触 U-King 的人看。
 *  · 这里是**日常启动台**——给已经装好工具的人，天天回来的落脚点是「换个模型、点一下启动」。
 * 两者共用同一批数据源（`tools`/`driver`/`deviceKey`）和同一条业务通路
 * （`launchTool`/`openTool`/`apply_provider`，均由 App.tsx 传入，本组件不重新实现）。
 *
 * 可插拔：整个模块收在 `src/toolhub/` 下，App.tsx 只在两处接它——侧栏 `CORE` 数组的一条
 * `NavItem` 和主区 `tab === "toolhub"` 的一个分支——删掉这个目录、去掉这两处即可完整移除
 * （宪法「模块独立可插拔」条）。
 *
 * ActionParity：本组件不写任何新业务逻辑，纯粹是对已有 Tauri 命令（`list_providers` /
 * `apply_provider`）和已有回调（`onLaunch`/`onOpen`）的又一层界面，跟 `ProviderSwitch.tsx`
 * 走的是同一条后端路径。之所以没有直接嵌 `<ProviderSwitch>`：那个组件是「点了就立即切」的
 * 语义（cc-switch 风格），而这页要的是「先选、勾了『修改模型配置』才在点启动那一刻生效」——
 * 交互模型不同，但 apply_provider 的调用形状（provider/apiKey/model/targets 四个字段）
 * 照抄 `ProviderSwitch.doSwitch`，没有引入新的写法。
 */
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  Cpu,
  Download,
  LayoutGrid,
  Play,
  Plus,
  RefreshCw,
  Settings2,
  SquareTerminal,
  Wallet,
} from "lucide-react";
import { LAB_TOOLS, toolTargets, currentModelFor, discoveryNameFor, type ToolInfo } from "../App";
import type { DeviceKey, DriverStatus } from "../lib/types";
import type { ProviderPreset } from "../Wizard";
import { ToolIcon } from "../components/ToolIcon";
import { providerKeyFor } from "../components/ProviderSwitch";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";
import { getLaunchPref, setLaunchPref } from "./launchPref";

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

/** 「修改模型配置」勾选记忆 —— 默认勾上（用户在右侧选了一个模型，本身就是想用它；
 *  EchoBird 两个勾选框也是默认都开）；用户主动取消勾选后，记住这个选择，别每次都弹回默认。 */
const APPLY_MODEL_KEY = "uking.toolhub.applyModel";
function getApplyModelPref(): boolean {
  try {
    const v = localStorage.getItem(APPLY_MODEL_KEY);
    if (v === "0") return false;
    if (v === "1") return true;
  } catch {
    /* localStorage 不可用时按默认勾上处理 */
  }
  return true;
}
function setApplyModelPref(v: boolean): void {
  try {
    localStorage.setItem(APPLY_MODEL_KEY, v ? "1" : "0");
  } catch {
    /* ignore：写不进去只影响这一次会话的偏好 */
  }
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
}) {
  const { t: tr } = useI18n();
  const [category, setCategory] = useState<Category>("all");
  const [refreshing, setRefreshing] = useState(false);

  const visible = useMemo(
    () => tools.filter((t) => !t.hidden && (category === "all" || categoryOf(t) === category)),
    [tools, category],
  );

  const [selectedId, setSelectedId] = useState<string | null>(null);
  // 选中项跟着分类走：切分类后如果选中的工具不在当前视图里，自动挑视图里第一个
  // （优先已装的，跟「我的 AI」把已装排前面是同一个判断——先给能直接点启动的）。
  useEffect(() => {
    setSelectedId((cur) => {
      if (cur && visible.some((t) => t.id === cur)) return cur;
      const installed = visible.find((t) => t.installed);
      return (installed ?? visible[0])?.id ?? null;
    });
  }, [visible]);

  const selected = visible.find((t) => t.id === selectedId) ?? null;
  const targets = selected ? toolTargets(selected.id) : [];
  const listTool = targets[0];

  const [providers, setProviders] = useState<ProviderPreset[]>([]);
  const [loadingProviders, setLoadingProviders] = useState(false);
  useEffect(() => {
    if (!listTool) {
      setProviders([]);
      return;
    }
    let alive = true;
    setLoadingProviders(true);
    invoke<ProviderPreset[]>("list_providers", { tool: listTool })
      .then((ps) => alive && setProviders(ps ?? []))
      .catch(() => alive && setProviders([]))
      .finally(() => alive && setLoadingProviders(false));
    return () => {
      alive = false;
    };
  }, [listTool]);

  // 虾盘云（内置充值渠道）钉最前——用户开箱唯一不用自己填 Key 就能用的一档，
  // 该被第一眼看到。纯前端展示排序，不改后端 `list_providers` 的顺序语义
  // （那份顺序是用户可拖拽调整的「偏好」，这里只是把内置渠道垫到最上面显示）。
  const sortedProviders = useMemo(
    () => [...providers].sort((a, b) => Number(!!b.builtin_recharge) - Number(!!a.builtin_recharge)),
    [providers],
  );

  const activeId = (selected && driver?.active?.[targets[0]]) || null;
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
  useEffect(() => {
    setSelectedProviderId(activeId);
  }, [activeId, selected?.id]);

  const [applyModelOnLaunch, setApplyModelOnLaunch] = useState(getApplyModelPref);
  const [launchInUcli, setLaunchInUcli] = useState(() => getLaunchPref() === "ucli");
  const [applying, setApplying] = useState(false);

  const toggleApplyModelOnLaunch = (checked: boolean) => {
    setApplyModelOnLaunch(checked);
    setApplyModelPref(checked);
  };

  const toggleLaunchInUcli = (checked: boolean) => {
    setLaunchInUcli(checked);
    setLaunchPref(checked ? "ucli" : "system");
  };

  const refreshAll = async () => {
    setRefreshing(true);
    try {
      await onRefreshTools();
    } finally {
      setRefreshing(false);
    }
  };

  const handlePrimary = async () => {
    if (!selected) return;
    if (!selected.installed) {
      onOpen(selected);
      return;
    }
    if (applyModelOnLaunch && selectedProviderId && selectedProviderId !== activeId && targets.length) {
      const p = providers.find((x) => x.id === selectedProviderId);
      if (p) {
        const key = providerKeyFor(p, deviceKey);
        if (key === "") {
          onToast(tr("{name} 需要先在「AI 设置」填 Key", { name: p.name }));
          onGoManage();
          return;
        }
        setApplying(true);
        try {
          await invoke("apply_provider", { providerId: p.id, apiKey: key, model: null, targets });
          // ClawX 不热重载配置文件（运行时持有内存副本，退出会覆写）——切完必须重启 ClawX 才生效。
          // 跟 `ProviderSwitch.tsx::doSwitch` 用同一句提示（`clawxHint`），别在这重新造一句漂移的文案。
          const clawxHint = targets.includes("clawx") ? tr("，请重启 ClawX 生效") : "";
          onToast(
            (p.id === "official"
              ? tr("已还原官方配置{hint}", { hint: "" })
              : tr("已切到 {name}，正在启动 {tool}…", { name: p.name, tool: selected.name })) + clawxHint,
          );
          await onRefreshTools();
        } catch (e) {
          onToast(String(e));
          setApplying(false);
          return;
        }
        setApplying(false);
      }
    }
    onLaunch(selected);
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h1 className="text-[18px] font-semibold text-ink-0 flex items-center gap-2">
            <LayoutGrid size={18} className="text-accent" />
            {tr("AI 工具中心")}
          </h1>
          <p className="mt-0.5 text-[12px] text-ink-3">{tr("选工具 · 选模型 · 一键启动")}</p>
        </div>
        <button
          onClick={() => void refreshAll()}
          disabled={refreshing}
          className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-[12.5px] text-ink-2 hover:bg-white/[0.05] disabled:opacity-60"
        >
          <RefreshCw size={13} className={refreshing ? "animate-spin" : ""} />
          {tr("刷新")}
        </button>
      </div>

      {/* 分类标签 */}
      <div className="flex items-center gap-1.5 flex-wrap">
        {CATS.map((c) => (
          <button
            key={c.id}
            onClick={() => setCategory(c.id)}
            className={cn(
              "px-3 h-8 rounded-full text-[12.5px] font-medium transition-colors",
              category === c.id
                ? "bg-accent text-white"
                : "bg-white/[0.03] text-ink-3 hover:bg-white/[0.06] hover:text-ink-1",
            )}
          >
            {tr(c.label)}
          </button>
        ))}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_320px] gap-4 items-start">
        {/* 中间：工具网格 */}
        <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-3">
          {visible.length === 0 && (
            <div className="col-span-full rounded-card border border-dashed border-white/[0.12] px-4 py-8 text-center text-[12.5px] text-ink-4">
              {tr("这个分类下暂时没有工具")}
            </div>
          )}
          {visible.map((t) => {
            const model = currentModelFor(t, driver);
            const on = t.id === selectedId;
            return (
              <button
                key={t.id}
                onClick={() => setSelectedId(t.id)}
                className={cn(
                  "text-left rounded-card border p-3 transition-colors bg-bg-1",
                  on ? "border-accent ring-1 ring-inset ring-accent bg-accent/[0.06]" : "border-ink-5/30 hover:border-white/[0.16]",
                )}
              >
                <div className="flex items-center gap-2.5">
                  <span className="grid place-items-center w-9 h-9 rounded-xl bg-bg-3 shrink-0">
                    <ToolIcon tool={t.id} size={22} active={t.installed} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="text-[13.5px] font-semibold text-ink-0 truncate">{t.name}</div>
                    <div
                      className={cn(
                        "text-[11px] flex items-center gap-1 mt-0.5",
                        t.installed ? "text-success-400" : "text-ink-4",
                      )}
                    >
                      {t.installed ? (
                        <>
                          <Check size={10} className="shrink-0" />
                          <span className="shrink-0">{tr("已安装")}</span>
                        </>
                      ) : (
                        <span>{tr("未安装")}</span>
                      )}
                    </div>
                  </div>
                </div>
                {t.installed && (
                  <div className="mt-2 text-[11px] text-ink-3 truncate flex items-center gap-1" title={model ?? undefined}>
                    <Cpu size={10} className="text-accent/70 shrink-0" />
                    {model ? <span className="truncate">{model}</span> : <span className="text-warning-500">{tr("还没配模型")}</span>}
                  </div>
                )}
                {primaryDiscoveryVersion(t, driver) && (
                  <div className="mt-1 text-[10.5px] text-ink-5 font-mono truncate">
                    v{primaryDiscoveryVersion(t, driver)}
                  </div>
                )}
              </button>
            );
          })}
        </div>

        {/* 右侧：换模型面板 */}
        <div className="rounded-card border border-white/[0.08] bg-bg-1 p-3.5">
          <div className="text-[13px] font-semibold text-ink-0 mb-2.5">
            {tr("为此选择模型：{tool}", { tool: selected?.name ?? tr("（未选择工具）") })}
          </div>
          {!selected ? (
            <p className="text-[12px] text-ink-4">{tr("先在左边选一个工具")}</p>
          ) : !targets.length ? (
            <p className="text-[12px] text-ink-4">{tr("这个工具不支持在这里切换模型")}</p>
          ) : loadingProviders ? (
            <p className="text-[12px] text-ink-4">{tr("加载中…")}</p>
          ) : (
            <div className="space-y-1.5 max-h-72 overflow-y-auto pr-0.5">
              {sortedProviders.map((p) => {
                const on = p.id === selectedProviderId;
                return (
                  <button
                    key={p.id}
                    onClick={() => setSelectedProviderId(p.id)}
                    className={cn(
                      "w-full flex items-center gap-2 rounded-lg border px-2.5 py-2 text-left transition-colors",
                      on
                        ? "border-accent bg-accent/[0.10] ring-1 ring-inset ring-accent"
                        : "border-white/[0.06] bg-white/[0.02] hover:bg-white/[0.05]",
                    )}
                  >
                    <span
                      className={cn(
                        "w-3.5 h-3.5 rounded-full border grid place-items-center shrink-0",
                        on ? "border-accent" : "border-ink-5",
                      )}
                    >
                      {on && <span className="w-1.5 h-1.5 rounded-full bg-accent" />}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1.5">
                        <span className="text-[12.5px] font-medium text-ink-1 truncate">{p.name}</span>
                        {p.id === activeId && (
                          <span className="text-[9px] px-1.5 h-[15px] inline-flex items-center rounded-full bg-accent text-white shrink-0">
                            {tr("使用中")}
                          </span>
                        )}
                        {p.builtin_recharge && (
                          <span className="text-[9px] text-accent-400 shrink-0">{tr("内置")}</span>
                        )}
                      </span>
                      <span className="block text-[10.5px] text-ink-4 font-mono truncate">
                        {hostOf(p.anthropic_base || p.openai_base)}
                      </span>
                    </span>
                    {p.builtin_recharge && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          onRecharge();
                        }}
                        className="shrink-0 inline-flex items-center gap-0.5 text-[10.5px] text-accent-400 hover:text-accent"
                      >
                        <Wallet size={10} /> {tr("余额/充值")}
                      </button>
                    )}
                  </button>
                );
              })}
              <button
                onClick={() => onManageProviders(undefined, listTool)}
                className="w-full flex items-center gap-1.5 px-2.5 py-2 rounded-lg text-[12px] text-ink-4 hover:text-accent-400 hover:bg-white/[0.03]"
              >
                <Plus size={13} /> {tr("+ 自定义供应商")}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* 底部操作条 */}
      <div className="sticky bottom-0 rounded-card border border-white/[0.08] bg-bg-1/95 backdrop-blur px-4 py-3 flex flex-wrap items-center gap-4">
        <label className="inline-flex items-center gap-1.5 text-[12px] text-ink-2">
          <input
            type="checkbox"
            checked={applyModelOnLaunch}
            onChange={(e) => toggleApplyModelOnLaunch(e.target.checked)}
            disabled={!selected?.installed || !targets.length}
          />
          <Settings2 size={13} className="text-ink-4" />
          {tr("修改模型配置")}
        </label>
        <label className="inline-flex items-center gap-1.5 text-[12px] text-ink-2">
          <input
            type="checkbox"
            checked={launchInUcli}
            onChange={(e) => toggleLaunchInUcli(e.target.checked)}
          />
          <SquareTerminal size={13} className="text-ink-4" />
          {tr("在 U-CLI 终端中打开")}
        </label>
        <div className="flex-1" />
        <button
          onClick={() => void handlePrimary()}
          disabled={!selected || applying}
          className="inline-flex items-center gap-1.5 px-6 h-10 rounded-xl bg-accent text-white text-[13.5px] font-semibold hover:bg-accent-600 disabled:opacity-60 shadow-sm"
        >
          {selected?.installed ? <Play size={15} /> : <Download size={15} />}
          {applying
            ? tr("正在应用配置…")
            : selected?.installed
              ? tr("启动")
              : tr("一键安装")}
        </button>
      </div>
    </div>
  );
}

/** 卡片上「版本号」——只在零成本时有（`ToolDiscovery.version`，同目录/上级目录有
 *  package.json 才填），拿不到就不显示，不猜一个假版本号出来。 */
function primaryDiscoveryVersion(t: ToolInfo, driver: DriverStatus | null): string | null {
  const name = discoveryNameFor(t.id);
  return driver?.discovered?.find((d) => d.name === name)?.version ?? null;
}
