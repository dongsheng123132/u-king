/**
 * magpie 路由面板 —— 「AI 设置 → 路由」分区（1.4.12，设计稿 docs/HANDOFF-20261105-magpie-route.md §5）。
 *
 * 解决的事：U-King 的「我的 AI」里能装 magpie（本地模型管家），但装完在 U-King 里
 * **看不见它的路由组**，客户得离开 U-King 去 magpie 窗口里点。这一版把「哪个 AI 走哪一路」
 * 搬进 AI 设置。
 *
 * 数据全在本机、不出网：
 *  - `magpie_route_state` 读一次：装没装 / 网关在不在跑 / 有哪些路由组 / 每个 AI 当前走哪一路；
 *  - `magpie_route_apply` / `magpie_route_reset` 写完**直接返回新的 RouteState**，
 *    本组件拿它替换本地状态 —— 不再多发一次 state 调用（少一次 CLI 往返，也不会出现
 *    「刚切成功、界面又刷回旧值」那种自相矛盾）。
 *
 * 🔴 诚实地画「不知道」：
 *  - `readable === false` = 后端读不到这个 AI 的模型配置，**不是「没配」**。这一行单独文案，
 *    不给勾、不写「未配置」，也不把它当失败（可能是那个工具压根没装）。
 *  - `installed === false` 时整块只给一张说明卡：不知道路由就不假装有路由。
 *
 * 换一路是**写别人家的配置文件**（magpie 替你写），所以底下一句话必须写清
 * 「切完要重启那个 AI 工具才看得到新模型」—— 不写，客户会以为没生效然后再切一次。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Loader2,
  RefreshCw,
  RotateCcw,
  Waypoints,
  XCircle,
} from "lucide-react";

import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

/** 一路可选的路由（对应 Rust `RouteOption`；字段名就是后端序列化的原名）。 */
type RouteOption = {
  /** "group/smart-split" | "auto/deepseek-v4.1-flash" —— 交给 magpie CLI 的就是这个字符串。 */
  model: string;
  /** 人话名（display_name，缺省是 id）。 */
  name: string;
  /** "group"（自家路由组）| "auto"（同一模型多家 provider 兜底）。 */
  kind: string;
  /** reasoning_levels：有的模型才有档位。 */
  levels: string[];
  /** providers.json 里带得来成员；网关来源为空。 */
  members: string[];
};

/** 一个 AI 工具当前走哪一路（对应 Rust `ToolRoute`）。 */
type ToolRoute = {
  /** claude / codex / dsh / pi / hermes —— 也是 apply/reset 的入参。 */
  target: string;
  /** Claude Code / Codex / …（后端给，不在前端再造一份表）。 */
  label: string;
  /** magpie 那边的 agent id。 */
  agent: string;
  /** CLI 回读到的原始模型名（已剥 `magpie/`）。 */
  model: string | null;
  /** 是路由就是 `group/x` / `auto/x`，否则 null。 */
  route: string | null;
  /** "cli" | "config" | "none" —— 这个值从哪读来的。 */
  source: string;
  /** 🔴 false = **不知道**（不是「没配」）。界面必须单独文案，不许渲染成结论。 */
  readable: boolean;
};

/** `magpie_route_state` 的整体返回（对应 Rust `RouteState`）。 */
type RouteState = {
  installed: boolean;
  running: boolean;
  /** 运行态网关地址，如 http://127.0.0.1:3425（只在 tooltip 里露，不当装饰画上去）。 */
  gateway: string;
  routes: RouteOption[];
  tools: ToolRoute[];
  /** "gateway" | "config" | "none" —— 路由组从哪读来的（离线兜底要如实说）。 */
  routes_source: string;
  /** 后端读的时候遇到的事（可空）。有就如实展示，不吞。 */
  error: string | null;
};

/**
 * 配置目标 → 「我的 AI」里的工具 id，只为读安装态画「未安装」徽章。
 * 故意只在本文件写一份小的：Manager 里那张 `TARGET_TOOL_ID` 是页面内的，跨组件 import
 * 会绕成环（Manager → 本组件），而「装没装」只用来加一行提示，不值得为它抽公共层。
 * 对不上就返回 null = 不知道，界面不标徽章（同 Manager 的 `realInstalled` 口径）。
 */
const TARGET_TOOL_ID: Record<string, string> = {
  claude: "claude-code",
  codex: "codex",
  dsh: "dsh",
  pi: "pi",
  hermes: "hermes",
};

/** 路由组 / 自动路由的一个小卡片。模块级组件 —— 写在渲染里当匿名组件会每次重挂，丢展开态。 */
function RouteOptionCard({ option }: { option: RouteOption }) {
  return (
    <div className="rounded-lg border border-white/[0.08] bg-bg-0/40 px-3 py-2.5">
      <div className="flex items-baseline gap-2 min-w-0">
        <span className="text-[12.5px] font-medium text-ink-1 truncate" title={option.name || option.model}>
          {option.name || option.model}
        </span>
        <span className="ml-auto shrink-0 max-w-[55%] truncate font-mono text-[10.5px] text-ink-4" title={option.model}>
          {option.model}
        </span>
      </div>
      {option.levels.length > 0 && (
        <div className="mt-1.5 flex flex-wrap gap-1">
          {option.levels.map((lv) => (
            <span
              key={lv}
              className="px-1.5 py-0.5 rounded-md bg-white/[0.04] text-[10.5px] font-mono text-ink-3"
            >
              {lv}
            </span>
          ))}
        </div>
      )}
      {option.members.length > 0 && (
        <div className="mt-1 text-[10.5px] text-ink-4 leading-snug line-clamp-2" title={option.members.join(" · ")}>
          {option.members.join(" · ")}
        </div>
      )}
    </div>
  );
}

export function MagpieRoutePanel({
  tools,
  onGoPage,
}: {
  /** 「我的 AI」那份工具清单，只用来给未安装的工具加提示徽章（不传 = 不知道，不标）。 */
  tools?: { id: string; installed: boolean }[];
  /** 跳全屏页：「我的 AI」的 tab id 是 "toolhub"。 */
  onGoPage?: (tab: string) => void;
}) {
  const { t } = useI18n();

  const [state, setState] = useState<RouteState | null>(null);
  /** 读失败的原因（raw string）。有它时 state 仍是 null —— 界面给「读取失败 + 重试」，不白屏。 */
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  /** 刷新 = 加一次 tick 重跑读取 effect（不另写第二份读逻辑）。 */
  const [tick, setTick] = useState(0);
  /** 正在 apply/reset 的那个 target（只锁一行，不锁整张表）。 */
  const [busy, setBusy] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  /** 每行下拉当前选的 model（还没点「切到这一路」之前只是草稿，不写任何配置）。 */
  const [picks, setPicks] = useState<Record<string, string>>({});
  const [autoOpen, setAutoOpen] = useState(false);
  const [toast, setToast] = useState<{ msg: string; kind: "success" | "error" } | null>(null);

  /** 卸载守卫：apply/reset/open 这几个事件回调是异步的，回来时组件可能已经不在了。 */
  const mountedRef = useRef(true);
  const toastTimerRef = useRef<number | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current);
    };
  }, []);

  /**
   * 读一次整份路由状态。`alive` 守卫是这里唯一的异步 setState 出口 ——
   * 切页/关窗时那次 invoke 还可能在路上，回来一律丢弃。
   */
  useEffect(() => {
    let alive = true;
    setLoading(true);
    invoke<RouteState>("magpie_route_state")
      .then((s) => {
        if (!alive) return;
        setState(s);
        setLoadErr(null);
      })
      .catch((e: unknown) => {
        if (!alive) return;
        setLoadErr(String(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [tick]);

  const flash = (msg: string, kind: "success" | "error") => {
    if (!mountedRef.current) return;
    setToast({ msg, kind });
    if (toastTimerRef.current !== null) window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => {
      if (mountedRef.current) setToast(null);
    }, 3000);
  };

  /**
   * 切到某一路。成功后**直接用返回的 RouteState 替换本地状态**（设计稿 §5.6）：
   * 后端已经写完并回读过了，再发一次 state 只会多一次 magpie CLI 往返，
   * 而且中间那段时间界面还是旧值，客户会以为没生效。
   */
  const apply = async (tool: ToolRoute, model: string) => {
    if (!model || busy) return;
    setBusy(tool.target);
    try {
      const next = await invoke<RouteState>("magpie_route_apply", { target: tool.target, model });
      if (!mountedRef.current) return;
      setState(next);
      setPicks((p) => ({ ...p, [tool.target]: "" }));
      flash(t("已切到 {name}", { name: labelOf(next, model) }), "success");
    } catch (e: unknown) {
      if (!mountedRef.current) return;
      flash(t("切换失败：") + String(e), "error");
    } finally {
      if (mountedRef.current) setBusy(null);
    }
  };

  /** 还原成这个 AI 工具自己原来的配置（`magpie <agent> default`）。同上，用返回值刷状态。 */
  const reset = async (tool: ToolRoute) => {
    if (busy) return;
    setBusy(tool.target);
    try {
      const next = await invoke<RouteState>("magpie_route_reset", { target: tool.target });
      if (!mountedRef.current) return;
      setState(next);
      setPicks((p) => ({ ...p, [tool.target]: "" }));
      flash(t("已还原"), "success");
    } catch (e: unknown) {
      if (!mountedRef.current) return;
      flash(t("还原失败：") + String(e), "error");
    } finally {
      if (mountedRef.current) setBusy(null);
    }
  };

  /** 直接把 magpie 自己的窗口拉起来（客户不用去开始菜单里找）。 */
  const openMagpie = async () => {
    if (opening) return;
    setOpening(true);
    try {
      await invoke("magpie_route_open");
      if (!mountedRef.current) return;
      flash(t("已请 magpie 打开窗口"), "success");
    } catch (e: unknown) {
      if (!mountedRef.current) return;
      flash(t("打开 magpie 失败：") + String(e), "error");
    } finally {
      if (mountedRef.current) setOpening(false);
    }
  };

  /** 路由 id → 人话名（对不上就如实显示 id，不编一个名字）。 */
  const labelOf = (s: RouteState, model: string) => {
    const hit = s.routes.find((r) => r.model === model);
    return hit?.name || model;
  };

  /** 这个 target 在「我的 AI」里装了没：null = 不知道（清单没到 / 表里没有）。 */
  const installedOf = (target: string): boolean | null => {
    const id = TARGET_TOOL_ID[target];
    if (!id || !tools?.length) return null;
    const hit = tools.find((x) => x.id === id);
    return hit ? hit.installed : null;
  };

  const refresh = () => setTick((x) => x + 1);

  const toastEl = toast ? (
    <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-50 animate-fade-in">
      <div className="flex items-center gap-2 rounded-full border border-white/[0.10] bg-bg-3/95 backdrop-blur px-4 py-2 text-[13px] text-ink-1 shadow-card">
        {toast.kind === "error" ? (
          <XCircle size={14} className="text-danger-400" />
        ) : (
          <CheckCircle2 size={14} className="text-success-400" />
        )}
        {toast.msg}
      </div>
    </div>
  ) : null;

  const iconBtn =
    "inline-flex items-center justify-center w-7 h-7 rounded-md border border-white/[0.10] text-ink-3 hover:text-ink-1 hover:bg-white/[0.04] disabled:opacity-50";

  /* ── 状态 1：还没读到（读失败时给原因 + 重试，不假装在加载） ───────────── */
  if (state === null) {
    return (
      <div className="space-y-3">
        {toastEl}
        <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card px-4 py-5">
          {loadErr ? (
            <div className="flex items-start gap-2">
              <AlertTriangle size={14} className="mt-0.5 shrink-0 text-warning-700 dark:text-warning-400" />
              <div className="min-w-0 flex-1">
                <div className="text-[12px] text-ink-2">
                  {t("读取路由失败：")}
                  <span className="font-mono text-[11.5px] text-ink-3 break-all">{loadErr}</span>
                </div>
                <button
                  type="button"
                  onClick={refresh}
                  className="mt-2 inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-white/[0.10] text-[11px] text-ink-2 hover:bg-white/[0.04]"
                >
                  <RefreshCw size={11} />
                  {t("重新读取")}
                </button>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2 text-[12px] text-ink-3">
              <Loader2 size={14} className="animate-spin text-accent" />
              {t("正在读取 magpie 路由…")}
            </div>
          )}
        </section>
      </div>
    );
  }

  /* ── 状态 2：没装 magpie —— 只解释 + 指路，下面整块不渲染 ───────────────── */
  if (!state.installed) {
    return (
      <div className="space-y-3">
        {toastEl}
        <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
          <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
            <Waypoints size={14} className="shrink-0 text-accent" />
            <div className="text-[13px] font-medium text-ink-1">{t("magpie 路由")}</div>
          </div>
          <div className="px-4 py-3.5 space-y-3">
            <div className="text-[12px] text-ink-2 leading-relaxed">
              {t("magpie 是本地模型管家：把多份订阅、多个 Key 合成「路由组」，AI 工具把它当成一个模型来用。")}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {onGoPage && (
                <button
                  type="button"
                  onClick={() => onGoPage("toolhub")}
                  className="inline-flex items-center gap-1 px-3 h-8 rounded-lg bg-accent text-white text-[11.5px] font-semibold hover:bg-accent-600"
                >
                  {t("先去「我的 AI」安装 magpie")}
                  <ChevronRight size={12} />
                </button>
              )}
              <span className="text-[11px] text-ink-4">{t("装好并启动 magpie 后，回这里就能选路由。")}</span>
            </div>
          </div>
        </section>
      </div>
    );
  }

  const routes = state.routes ?? [];
  const groupRoutes = routes.filter((r) => r.kind === "group");
  const autoRoutes = routes.filter((r) => r.kind !== "group");
  const toolRows = state.tools ?? [];

  /** 「当前」那一行的三种（加兜底）画法 —— 见本文件顶部 🔴 那条诚实纪律。 */
  const currentText = (tool: ToolRoute) => {
    if (tool.route) {
      return <span className="text-accent">→ {labelOf(state, tool.route)}</span>;
    }
    if (!tool.readable) {
      // 不知道就说不知道：既不是「未配置」，也不是失败。
      return <span className="text-warning-700 dark:text-warning-400">{t("读取不到当前模型")}</span>;
    }
    if (tool.model) {
      return <span>{t("当前：{model}（不是 magpie 路由）", { model: tool.model })}</span>;
    }
    return <span className="text-ink-4">{t("未配置")}</span>;
  };

  /* ── 状态 3/4/5：装了。开机头卡 → （没在跑就黄条）→ 路由组 → 工具表 ─────── */
  return (
    <div className="space-y-3">
      {toastEl}

      <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
          <Waypoints size={14} className="shrink-0 text-accent" />
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium text-ink-1">{t("magpie 路由")}</div>
            <div className="mt-0.5 text-[11px] text-ink-3">
              {t("magpie 把多份订阅合成一路，按任务自动挑模型；这里决定每个 AI 用哪一路。")}
            </div>
          </div>
          <span
            className={cn(
              "shrink-0 inline-flex items-center gap-1 px-2 h-6 rounded-md text-[10.5px]",
              state.running
                ? "bg-success-500/[0.10] text-success-600"
                : "bg-white/[0.05] text-ink-3",
            )}
            title={state.running ? state.gateway : undefined}
          >
            {state.running ? <CheckCircle2 size={11} /> : <XCircle size={11} />}
            {state.running ? t("运行中") : t("没在运行")}
          </span>
          <button
            type="button"
            onClick={refresh}
            disabled={loading}
            title={t("刷新")}
            className={iconBtn}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
          <button
            type="button"
            onClick={() => void openMagpie()}
            disabled={opening}
            className="shrink-0 inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-accent/30 text-accent text-[11px] font-medium hover:bg-accent/[0.08] disabled:opacity-50"
          >
            {opening ? <Loader2 size={11} className="animate-spin" /> : <Waypoints size={11} />}
            {t("打开 magpie")}
          </button>
        </div>

        {/* 状态 3：装了没在跑 —— 不是错误，但路由确实不会生效，必须说在明处。 */}
        {!state.running && (
          <div className="flex items-start gap-2 px-4 py-2 text-[11px] bg-warning-500/[0.08] border-b border-white/[0.06] text-warning-700 dark:text-warning-400">
            <AlertTriangle size={12} className="mt-0.5 shrink-0" />
            <span>{t("magpie 已安装但没在运行，路由要它开着才生效")}</span>
          </div>
        )}

        {/* 后端读的时候遇到的事：如实转述，不吞成「一切正常」。 */}
        {state.error && (
          <div className="flex items-start gap-2 px-4 py-2 text-[11px] bg-danger-500/[0.08] border-b border-white/[0.06] text-danger-700 dark:text-danger-400">
            <AlertTriangle size={12} className="mt-0.5 shrink-0" />
            <span className="break-all">
              {t("magpie 报了个错：")}
              <span className="font-mono">{state.error}</span>
            </span>
          </div>
        )}

        {/* 刷新失败：旧数据还在，但「这次没读到」必须说出来，不能静默留一份过期状态。 */}
        {loadErr && (
          <div className="flex items-start gap-2 px-4 py-2 text-[11px] bg-warning-500/[0.08] border-b border-white/[0.06] text-warning-700 dark:text-warning-400">
            <AlertTriangle size={12} className="mt-0.5 shrink-0" />
            <span className="break-all">
              {t("读取路由失败：")}
              <span className="font-mono">{loadErr}</span>
            </span>
          </div>
        )}
      </section>

      {/* 状态 4：装了、但没有路由组 —— 说清去哪里建，给两条路（打开 / 刷新）。 */}
      {routes.length === 0 ? (
        <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card px-4 py-4">
          <div className="text-[12px] text-ink-2 leading-relaxed">
            {t("magpie 里还没有路由组。到 magpie 的 Routing 面板建一个，回来点刷新。")}
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void openMagpie()}
              disabled={opening}
              className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-accent/30 text-accent text-[11px] font-medium hover:bg-accent/[0.08] disabled:opacity-50"
            >
              {opening ? <Loader2 size={11} className="animate-spin" /> : <Waypoints size={11} />}
              {t("打开 magpie")}
            </button>
            <button
              type="button"
              onClick={refresh}
              disabled={loading}
              className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-white/[0.10] text-[11px] text-ink-2 hover:bg-white/[0.04] disabled:opacity-50"
            >
              <RefreshCw size={11} className={loading ? "animate-spin" : ""} />
              {t("刷新")}
            </button>
          </div>
        </section>
      ) : (
        /* 状态 5：正常态 —— 路由组 + 自动路由（折叠）+ 五工具切换表 */
        <>
          <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
            <div className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-ink-1">{t("我的路由组")}</div>
                <div className="mt-0.5 text-[11px] text-ink-3">
                  {t("一组由你配的多家模型组成，AI 工具看到的只是这一组的名字。")}
                </div>
              </div>
              {state.routes_source === "config" && (
                <span className="shrink-0 text-[10.5px] text-ink-4">
                  {t("读自 magpie 的本机配置（没连上它的网关）")}
                </span>
              )}
            </div>

            <div className="px-4 py-3">
              {groupRoutes.length === 0 ? (
                <div className="text-[11.5px] text-ink-4">{t("还没有自己建的路由组。")}</div>
              ) : (
                <div className="grid gap-2 sm:grid-cols-2">
                  {groupRoutes.map((r) => (
                    <RouteOptionCard key={r.model} option={r} />
                  ))}
                </div>
              )}
            </div>

            {/* 自动路由可能几十条，默认收起（设计稿 §5 状态 5）—— 但必须点得开。 */}
            {autoRoutes.length > 0 && (
              <div className="border-t border-white/[0.06]">
                <button
                  type="button"
                  onClick={() => setAutoOpen((v) => !v)}
                  className="flex w-full items-center gap-2 px-4 py-2.5 text-left hover:bg-white/[0.03]"
                >
                  {autoOpen ? (
                    <ChevronDown size={12} className="shrink-0 text-ink-4" />
                  ) : (
                    <ChevronRight size={12} className="shrink-0 text-ink-4" />
                  )}
                  <span className="text-[12.5px] text-ink-1">{t("自动路由")}</span>
                  <span className="shrink-0 rounded bg-white/[0.05] px-1.5 py-0.5 font-mono text-[10.5px] text-ink-3">
                    {autoRoutes.length}
                  </span>
                  <span className="ml-auto min-w-0 truncate text-[10.5px] text-ink-4">
                    {t("同一个模型多家兜底，数量多，默认收起")}
                  </span>
                </button>
                {autoOpen && (
                  <div className="px-4 pb-3 grid gap-2 sm:grid-cols-2">
                    {autoRoutes.map((r) => (
                      <RouteOptionCard key={r.model} option={r} />
                    ))}
                  </div>
                )}
              </div>
            )}
          </section>

          <section className="rounded-card border border-white/[0.08] bg-bg-1/70 shadow-card overflow-hidden">
            <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-ink-1">{t("让哪个 AI 用哪一路")}</div>
                <div className="mt-0.5 text-[11px] text-ink-3">
                  {t("选一路再点「切到这一路」；想恢复原样点「还原」。")}
                </div>
              </div>
            </div>

            {toolRows.length === 0 ? (
              <div className="px-4 py-3 text-[11.5px] text-ink-4">{t("没读到可切换的 AI，点刷新再试。")}</div>
            ) : (
              <div className="divide-y divide-white/[0.05]">
                {toolRows.map((tool) => {
                  const notInstalled = installedOf(tool.target) === false;
                  const pick = picks[tool.target] ?? "";
                  const working = busy === tool.target;
                  return (
                    <div
                      key={tool.target}
                      className="flex flex-col gap-2 px-4 py-2.5 sm:flex-row sm:items-center"
                    >
                      <div className="min-w-0 sm:w-[160px] shrink-0">
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span className="truncate text-[12.5px] text-ink-1">{tool.label || tool.target}</span>
                          {/* 未安装只提示、不禁用：「先配好再装」也是合法路径（设计稿 §5）。 */}
                          {notInstalled && (
                            <span
                              title={t("没装也能先配好，装上后直接生效")}
                              className="shrink-0 rounded bg-white/[0.05] px-1.5 py-0.5 text-[10px] text-ink-4"
                            >
                              {t("未安装")}
                            </span>
                          )}
                        </div>
                        <div className="mt-0.5 truncate text-[10.5px] text-ink-3" title={tool.model ?? undefined}>
                          {currentText(tool)}
                        </div>
                      </div>

                      <select
                        value={pick}
                        onChange={(e) => setPicks((p) => ({ ...p, [tool.target]: e.target.value }))}
                        disabled={working}
                        className="h-7 min-w-0 flex-1 rounded-md border border-white/[0.10] bg-bg-0 px-2 text-[11.5px] text-ink-1 outline-none focus:border-accent/60 disabled:opacity-50"
                      >
                        <option value="">{t("选择一路…")}</option>
                        {groupRoutes.length > 0 && (
                          <optgroup label={t("我的路由组")}>
                            {groupRoutes.map((r) => (
                              <option key={r.model} value={r.model}>
                                {r.name || r.model}
                              </option>
                            ))}
                          </optgroup>
                        )}
                        {autoRoutes.length > 0 && (
                          <optgroup label={t("自动路由")}>
                            {autoRoutes.map((r) => (
                              <option key={r.model} value={r.model}>
                                {r.name || r.model}
                              </option>
                            ))}
                          </optgroup>
                        )}
                      </select>

                      <div className="flex shrink-0 items-center gap-1.5">
                        <button
                          type="button"
                          onClick={() => void apply(tool, pick)}
                          disabled={!pick || busy !== null}
                          className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-accent/30 text-accent text-[11px] font-medium hover:bg-accent/[0.08] disabled:opacity-40"
                        >
                          {working ? <Loader2 size={11} className="animate-spin" /> : null}
                          {t("切到这一路")}
                        </button>
                        {tool.route && (
                          <button
                            type="button"
                            onClick={() => void reset(tool)}
                            disabled={busy !== null}
                            title={t("还原成它原来的配置")}
                            className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-white/[0.10] text-[11px] text-ink-2 hover:bg-white/[0.04] disabled:opacity-40"
                          >
                            <RotateCcw size={11} />
                            {t("还原")}
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}

            <div className="px-4 py-2.5 text-[11px] text-ink-4 border-t border-white/[0.06]">
              {t("切换由 magpie 写进各 AI 自己的配置；切完要重启那个 AI 工具才看得到新模型。")}
            </div>
          </section>
        </>
      )}
    </div>
  );
}
