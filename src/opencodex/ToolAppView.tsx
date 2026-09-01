/**
 * 独立 TUI 应用视图 —— claude / openclaw / hermes 共用（与 OpenCodex 平级）。
 *
 * 进去是空终端（不自动跑命令）+ 顶栏提示词按钮（点了才启动）。右侧可收起的驱动配置（只切该工具）。
 * 常驻保活靠父级 App.tsx 的 display 切换（本组件不卸载 → PTY 续跑）；手动逐个 X 关终端才停。
 * 复用 TermPanel（终端引擎 + 提示词按钮）+ ProviderSwitch（per-tool 切驱动，targets 参数化）。
 *
 * OpenClaw 专属：顶栏「一键启动并打开 WebUI」—— runInActive 起 gateway + 延时开浏览器到 dashboard。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, CheckCircle2, Loader2, PanelRight, PanelRightClose, Play, RefreshCw, Rocket, SquareTerminal } from "lucide-react";
import type { TuiApp } from "./apps";
import { TermPanel, type TermPanelApi } from "./panels/TermPanel";
import { ProviderSwitch } from "../components/ProviderSwitch";
import { ToolIcon } from "../components/ToolIcon";
import type { DeviceKey, DriverStatus } from "../lib/types";
import { useI18n } from "../i18n";

/** 这个工具是否已被任何驱动接管（接管了就别自动回灌，尊重用户可能切到的官方直连）。 */
function targetConfigured(app: TuiApp, d: DriverStatus | null): boolean {
  if (!d) return false;
  const t = app.configTargets[0];
  // 用户显式选过的驱动（含「官方直连」official）一律算已接管 —— 绝不回灌覆盖。
  // 这条优先级最高：还原到 official 后 config.toml 可能被删（没了 model_provider），
  // 若只看下面的实时配置会误判成「没配过」→ 把虾盘云又写回去，造成「怎么都还原不了」。
  if (d.active?.[t]) return true;
  if (t === "claude") return !!d.claude_base;
  if (t === "codex") return !!d.codex_provider;
  if (t === "clawx") return !!d.clawx_model;
  if (t === "dsh") return !!d.dsh_model;
  // Cline：裸跑没配 provider 报 "Unauthorized ... re-authenticate"（2026-08-29 实测），
  // 用 active["cline"]（显式记录）+ providers.json 的 openai-compatible 槽位判断是否接管过。
  // active["cline"] 由后端 record_active_driver 写，用户显式切过（含 official）在上面 line 27 已挡。
  if (t === "cline") return !!d.active?.cline;
  // Hermes 特例：**不能**用 `!!d.hermes_model` 兜底 —— Hermes 首次运行会**自造**一个默认
  // 模型（alibaba/qwen3.7-max），config.yaml 里永远有 model.default，导致这里恒为 true →
  // 启动时的自动配置被跳过 → Hermes 用它自己的 qwen 默认 + 空 Key → HTTP 401（客户实锤，
  // 见截图 2026-07-08）。真正的「已配过」信号是 active["hermes"]（line 26 已处理，来自
  // 显式记录或 base_url 反推的已知 provider）。无该记录 = 从没被我们/用户配过 → 该自动配虾盘云。
  if (t === "hermes") return false;
  return false;
}

const OPENCLAW_PORT = 18789;
const OPENCLAW_GATEWAY_CMD = `openclaw gateway run --allow-unconfigured --port ${OPENCLAW_PORT}`;
// 网页版 URL 不再前端死写 token —— 真实 token 由后端 openclaw_webui_url 读 gateway 配置返回。
// 历史 bug：死写 #token=uclaw，但 `--allow-unconfigured` 起的 gateway 用的是随机 token（且装了
// ClawX 时 18789 是 ClawX 的网关 token=clawx-xxx）→ 都对不上 → 网页「认证不匹配」打不开。
const OPENCLAW_WEBUI_FALLBACK = `http://127.0.0.1:${OPENCLAW_PORT}/#token=uclaw`;
const DSH_PORT = 3080;
const DSH_WEB_URL = `http://127.0.0.1:${DSH_PORT}`;

/** 每个工具的「这是啥 + 怎么用」一句话（启动遮罩上给小白看的）。 */
const APP_BLURB: Record<string, string> = {
  claude: "全球最强的编程 AI 助手，能写代码、改 bug、跑命令。点下面的按钮就开始对话。",
  codex: "OpenAI 的编程助手，会读你的代码、自动改文件。点下面的按钮就开始。",
  openclaw: "开源 AI 智能体（龙虾）。一键启动后会打开网页版控制台，能聊天、自动办事。",
  hermes: "Hermes 适合聊天、写方案和轻量工具任务。点启动会弹出独立终端窗口进入对话（默认已接好虾盘云），显示区域更大；浏览器接管需要单独体检。",
  dsh: "DeepSeek 官方智能体框架。可选 Web 工作台或持续对话终端；两种模式共用同一份模型、工具和权限。U-King 会用本机虾盘云 Key 一键配好，无需再申请 DeepSeek API Key。",
  cline: "开源编程 Agent，擅长自动化：定时任务、多项目并行、批量改代码。点启动自动配好虾盘云，直接对话或让它跑任务。",
};

type HermesBrowserStatus = {
  hermes_installed: boolean;
  browser_ready: boolean;
  agent_browser: { found: boolean; version?: string | null };
  config_dir: string;
  cloud_provider?: string | null;
  browser_use_key: boolean;
  browserbase_key: boolean;
  browserbase_project: boolean;
  firecrawl_key: boolean;
  cdp_url: boolean;
  message: string;
  suggestions: string[];
};

export function ToolAppView({
  app,
  active,
  deviceKey,
  onToast,
  onGoManage,
  onManageProviders,
  onRefreshDriver,
}: {
  app: TuiApp;
  active: boolean;
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onGoManage: () => void;
  onManageProviders: (editId?: string) => void;
  onRefreshDriver: () => void;
}) {
  const { t } = useI18n();
  const [cfgOpen, setCfgOpen] = useState(true);
  // 是否已启动（点过启动按钮）—— 决定是否还盖着启动遮罩。一旦启动就长驻不再盖。
  const [launched, setLaunched] = useState(false);
  const [hermesStatus, setHermesStatus] = useState<HermesBrowserStatus | null>(null);
  const [checkingHermes, setCheckingHermes] = useState(false);
  const termApi = useRef<TermPanelApi | null>(null);
  /** DSH 正在等就绪（不可重入的闸门 + 顶栏那条进度）。 */
  const [dshWaiting, setDshWaiting] = useState(false);
  const [dshPhase, setDshPhase] = useState("");
  /** 卸载标记：等待循环要停得掉，否则组件没了它还在轮询 + setState。 */
  const dshAbort = useRef(false);
  useEffect(() => () => { dshAbort.current = true; }, []);

  const isOpenClaw = app.id === "openclaw";
  const isHermes = app.id === "hermes";
  const isDsh = app.id === "dsh";
  const hasProviderConfig = app.configTargets.length > 0;
  // external 应用（如 Hermes）：启动 = 弹独立系统终端窗口，不挤内嵌终端（显示区域太窄，客户反馈）。
  const isExternal = !!app.external;

  // 启动主命令 = prompts 第一个（claude / hermes / codex 直接跑；openclaw 走一键开 WebUI）。
  const startPrompt = app.prompts[0];

  // 在独立系统终端窗口跑命令（term_open_external 已注入便携工具 PATH + OPENCLAW_* + 过白名单）。
  const runExternal = async (cmd: string) => {
    try {
      await invoke("term_open_external", { cmd });
      onToast(t("{name} 已在独立终端窗口打开（显示区域更大）", { name: app.name }));
    } catch (e) {
      onToast(t("打开独立终端失败：{e}", { e: String(e) }));
    }
  };

  const refreshHermesStatus = async () => {
    if (!isHermes) return;
    setCheckingHermes(true);
    try {
      setHermesStatus(await invoke<HermesBrowserStatus>("hermes_browser_status"));
    } catch {
      setHermesStatus(null);
    } finally {
      setCheckingHermes(false);
    }
  };

  useEffect(() => {
    if (active && isHermes) {
      refreshHermesStatus();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, isHermes]);

  // 需免配置启动的工具（openclaw / hermes / dsh）在用户**点「启动」时**按需配好虾盘云 —— 是主动点的那一下，
  // 不是打开页面就偷偷写（对齐用户定的「无主动不切换、全部纯手动」）。且只在「还没被任何驱动
  // 接管」时配，用户已手动切过的（含官方直连 / 自有 Key）一律尊重、绝不覆盖。
  // claude / codex 不在此列：用户常有自己的官方登录（Claude Pro/Max、Codex ChatGPT），点「启动」
  // 只是跑 CLI、绝不替他切驱动；要接虾盘云请用右侧 ProviderSwitch 显式切（可一键还原）。
  const ensureWebToolConfigured = async () => {
    if (!deviceKey?.key) return;
    // cline（2026-08-29 上架）：裸跑没配 provider 报 "Unauthorized ... re-authenticate"
    // 看不懂（2026-08-29 实测），必须在启动前按需配好虾盘云。同 openclaw/hermes/dsh 的按需口径。
    if (app.id !== "openclaw" && app.id !== "hermes" && app.id !== "dsh" && app.id !== "cline") return;
    const d = await invoke<DriverStatus>("get_driver_status").catch(() => null);
    if (targetConfigured(app, d)) return; // 已配过（含官方直连）→ 尊重用户选择，不动
    try {
      await invoke("apply_provider", {
        providerId: "xiapan",
        apiKey: deviceKey.key,
        model: null,
        targets: app.configTargets,
      });
      onRefreshDriver();
      onToast(
        t("{name} 已配好虾盘云", { name: app.name }) + (app.id === "openclaw" ? t("（ClawX 需重启）") : ""),
      );
    } catch {
      /* 配失败不打断启动，右侧 ProviderSwitch 仍可手动切 */
    }
  };

  const handleStart = async () => {
    // external 应用（Hermes）：不依赖内嵌终端 —— 先按需配好虾盘云（规避裸跑 hermes → 401 老坑），
    // 再弹一个独立系统终端窗口跑，显示区域更大、不挤在 U-King 界面里。
    if (isExternal) {
      setLaunched(true);
      await ensureWebToolConfigured();
      if (startPrompt) await runExternal(startPrompt.cmd);
      return;
    }
    if (!termApi.current) {
      onToast(t("终端还没就绪，请稍候再试"));
      return;
    }
    setLaunched(true);
    // OpenClaw 且 ClawX 桌面版正在运行：ClawX 就是完整 OpenClaw（占着 18789）。此时 U-King
    // 啥都别碰 —— 不写它配置（免「全部覆盖了」客户自己的 ClawX 设置）、不抢端口、不开连不上的
    // 网页壳，直接把 ClawX 拉到前台让客户用。这是「装了 ClawX 必然打不开」的根因修复。
    if (isOpenClaw && (await invoke<boolean>("clawx_running").catch(() => false))) {
      onToast(
        t("检测到 ClawX 桌面版正在运行 —— 它就是完整的 OpenClaw，已为你打开，无需在这里重复启动"),
      );
      invoke("launch_app", { app: "clawx" }).catch(() => {});
      return;
    }
    await ensureWebToolConfigured(); // 仅 openclaw/hermes/dsh 且未配过时；claude/codex 不碰配置
    if (isOpenClaw) {
      launchOpenClawWebUI();
    } else if (isDsh) {
      launchDshWebUI();
    } else if (startPrompt) {
      termApi.current.runCmd(startPrompt.cmd);
    }
  };

  // 用 gateway 真实 token 开网页（后端读配置；读不到回退 uclaw）。不再前端死写 token。
  const openWebUI = async () => {
    const url = await invoke<string>("openclaw_webui_url").catch(() => OPENCLAW_WEBUI_FALLBACK);
    invoke("open_browser", { url, label: "browser-openclaw" }).catch((e) => onToast(String(e)));
  };

  // OpenClaw 一键起 gateway + 自动开 WebUI（傻瓜式）。三种情况分别处理 —— 历史「认证不匹配 /
  // 打不开」的根因正是没分清，对所有占用端口的网关都死写 #token=uclaw：
  //  ① 端口被 ClawX 占着：ClawX 桌面版就是完整 OpenClaw（它的 token=clawx-xxx，和我们对不上）——
  //     别再开一个连不上的网页壳，直接把 ClawX 拉到前台，让客户用它（功能完全一样，还更好用）。
  //  ② 端口被我们之前起的 gateway 占着：用它**真实** token（后端读配置）开网页。
  //  ③ 端口空闲：先把 token 钉成已知值（prepare_openclaw_home）再起 gateway，就绪后用真实 token 开。
  const launchOpenClawWebUI = async () => {
    if (!termApi.current) {
      onToast(t("终端还没就绪，请稍候再试"));
      return;
    }
    const already = await invoke<boolean>("wait_port", { port: OPENCLAW_PORT, timeoutMs: 0 }).catch(
      () => false,
    );
    if (already) {
      // 端口已被占 —— 先分清是不是 ClawX 桌面版的网关。是的话别抢端口，直接打开 ClawX。
      const clawx = await invoke<boolean>("clawx_running").catch(() => false);
      if (clawx) {
        onToast(
          t("检测到 ClawX 桌面版正在运行 —— 它就是完整的 OpenClaw，已为你打开，无需在这里重复启动"),
        );
        invoke("launch_app", { app: "clawx" }).catch(() => {});
        return;
      }
      // 是我们自己之前起的 gateway（切走又切回 / 之前点过）—— 用它真实 token 开网页。
      await openWebUI();
      return;
    }
    // 端口空闲：先钉好 token 再起 gateway（后端轮询 18789 就绪，通了立刻开网页，不固定等 6s）。
    await invoke("prepare_openclaw_home").catch(() => {});
    termApi.current.runCmd(OPENCLAW_GATEWAY_CMD);
    onToast(t("正在启动 OpenClaw 网页版，就绪后自动打开控制台…"));
    const ready = await invoke<boolean>("wait_port", { port: OPENCLAW_PORT, timeoutMs: 30000 }).catch(
      () => false,
    );
    if (!ready) {
      onToast(t("网页版启动较慢或失败，请看下方终端日志，或稍后点右上角「打开网页版」重试"));
      return;
    }
    await openWebUI();
  };

  // DeepSeek Harness 官方的人类入口是 Web UI。PTY 只承载 server 日志；按钮负责等待服务真正
  // 就绪再开浏览器，避免固定 sleep 在慢机上开早、快机上白等。端口已活时直接复用已有实例。
  //
  // 🔴 **60 秒不是「失败」，是「还没到」**（干净 Windows 实测）：
  //    冷启动 60 秒时没有监听、2 分 45 秒仍未就绪、但**随后可达**；热启动约 21.9 秒 → HTTP 200。
  //    旧实现等满 60 秒就弹「启动较慢或失败，请查看日志后重试」然后**停止探测** ——
  //    于是一次完全正常的首次初始化被画成失败，客户按提示去重试，重试又是一次冷启动。
  //    装成功了、却被自己的等待窗口判成装失败，这是本轮评测里最贵的一个误报。
  //
  // 为什么在前端轮询而不是把 `wait_port` 的超时调大：`term.rs::wait_port` 内部
  // `timeout_ms.min(60_000)` **硬顶 60 秒** —— 前端传 300000 也只会等 60 秒然后返回 false，
  // 「我要等 5 分钟」会被静默改成「等 1 分钟」。与其把那个上限改大（它是给所有调用方的护栏），
  // 不如在这儿用它的**单次探测**（timeoutMs:0）自己循环：还能顺带报出已等了多久。
  const DSH_HOT_MS = 60_000; // 热启动实测 ~22s；超过这条线就该改口说「首次初始化」
  const DSH_GIVEUP_MS = 10 * 60_000; // 到这儿仍不通才算真出事，且仍然只说事实、不叫用户重装
  const launchDshWebUI = async () => {
    if (!termApi.current) {
      onToast(t("终端还没就绪，请稍候再试"));
      return;
    }
    if (dshWaiting) {
      onToast(t("已经在等 DeepSeek Harness 就绪了，就绪后会自动打开"));
      return; // 不可重入：再点一次会再起一个 `dsh web`，两个实例抢同一个端口
    }
    const already = await invoke<boolean>("wait_port", { port: DSH_PORT, timeoutMs: 0 }).catch(() => false);
    if (!already) {
      termApi.current.runCmd("dsh web");
      setDshWaiting(true);
      setDshPhase(t("正在启动 DeepSeek Harness…"));
      const t0 = Date.now();
      try {
        for (;;) {
          if (dshAbort.current) return; // 组件卸载：停止轮询，不留一个写已卸载组件的循环
          const up = await invoke<boolean>("wait_port", { port: DSH_PORT, timeoutMs: 0 }).catch(() => false);
          if (up) break;
          const ms = Date.now() - t0;
          if (ms >= DSH_GIVEUP_MS) {
            // 仍然**只陈述观察到的事实**：等了多久、端口还没监听、日志在哪。
            // 不说「失败」——我们并不知道它失败了，我们知道的是它还没起来。
            setDshPhase("");
            setDshWaiting(false);
            onToast(
              t("等了 {m} 分钟，DeepSeek Harness 的 {p} 端口仍未监听 —— 下方终端里是它自己的日志，看那里能知道卡在哪。它可能仍在后台装组件，稍后点「打开工作台」会直接复用。", {
                m: String(Math.round(ms / 60_000)),
                p: String(DSH_PORT),
              }),
            );
            return;
          }
          // 阶段只报**能观察到的**：已等多久 + 这个时长意味着什么。
          // 「装到第几步了」我们看不见（那是 dsh 自己的进程），就不假装看得见。
          setDshPhase(
            ms < DSH_HOT_MS
              ? t("正在启动 DeepSeek Harness…（已等 {s} 秒）", { s: String(Math.round(ms / 1000)) })
              : t("首次启动要下载并初始化组件，通常 3–5 分钟。已等 {s} 秒，仍在探测，就绪后自动打开。", {
                  s: String(Math.round(ms / 1000)),
                }),
          );
          await new Promise((r) => setTimeout(r, 2000));
        }
      } finally {
        setDshWaiting(false);
        setDshPhase("");
      }
      if (dshAbort.current) return;
    }
    invoke("open_browser", { url: DSH_WEB_URL, label: "browser-dsh" }).catch((e) => onToast(String(e)));
  };

  // dsh 官方 headless 只跑一次任务；这里启动开源 dsh-terminal profile，在同一个真实
  // Agent/Session 上连续对话。每次用新 PTY 标签，避免 Web server 已占住当前 shell 时命令失效。
  const launchDshTerminal = () => {
    if (!termApi.current) {
      onToast(t("终端还没就绪，请稍候再试"));
      return;
    }
    setLaunched(true);
    termApi.current.runCmdNew("dsh --profile terminal");
    onToast(t("DeepSeek Harness 终端模式已在新标签打开"));
  };

  // 已移除「进页即 autoLaunch 自动启动」：它绕过用户点击 = 「打开页面就自动启动 / 自动接管」，
  // 与用户定的「无主动不切换、全部纯手动」冲突。现一律走遮罩大按钮 handleStart（点了才启动 + 按需配）。

  return (
    <div className="flex h-full min-h-0 rounded-card border border-white/[0.08] overflow-hidden bg-bg-2">
      {/* 主终端（空终端 + 提示词按钮，点了才启动） */}
      <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        <div className="flex items-center h-9 px-3 border-b border-white/[0.06] bg-bg-1 shrink-0 gap-2">
          <span className="text-[12.5px] font-medium text-ink-1">{app.name}</span>
          <span className="text-[10px] px-1.5 py-0.5 rounded bg-accent/[0.14] text-accent-400">{app.id}</span>
          <span className="text-[11px] text-ink-5 hidden md:inline">
            {isExternal ? t("— 在独立终端窗口运行，显示区域更大；关那个窗口才停") : t("— 点上方提示词启动；启动后常驻，关终端才停")}
          </span>
          <div className="flex-1" />
          {/* 🔴 等待中的那句话**必须常驻在界面上**，不能只发一条会自己消失的 toast：
              首次初始化要 3–5 分钟，客户在这几分钟里唯一能看到的东西就是这一行。
              没有它，「还在装」和「已经死了」在屏幕上长得一模一样 —— 那正是他会去点重试的时刻。 */}
          {isDsh && dshPhase && (
            <span className="text-[11px] text-accent-300 truncate max-w-[420px]" title={dshPhase}>
              {dshPhase}
            </span>
          )}
          {(isOpenClaw || isDsh) && (
            <button
              onClick={isDsh ? launchDshWebUI : launchOpenClawWebUI}
              disabled={isDsh && dshWaiting}
              title={isDsh ? t("启动并打开 DeepSeek Harness 工作台") : t("起 gateway 并自动打开网页控制台")}
              className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded bg-accent text-white text-[11.5px] font-semibold hover:bg-accent-600 disabled:opacity-60"
            >
              {isDsh && dshWaiting ? <Loader2 size={13} className="animate-spin" /> : <Rocket size={13} />}
              {isDsh ? t(dshWaiting ? "正在启动…" : "打开工作台") : t("打开网页版")}
            </button>
          )}
          {isDsh && (
            <button
              onClick={launchDshTerminal}
              title={t("在新终端标签启动 DeepSeek Harness 持续对话模式")}
              className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded border border-accent/40 text-accent-300 text-[11.5px] font-semibold hover:bg-accent/[0.12]"
            >
              <SquareTerminal size={13} />
              {t("终端模式")}
            </button>
          )}
          {hasProviderConfig && !cfgOpen && (
            <button
              onClick={() => setCfgOpen(true)}
              title={t("展开驱动配置")}
              className="inline-flex items-center gap-1 h-7 px-2 rounded text-[11px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.04]"
            >
              <PanelRight size={14} />
              {t("驱动配置")}
            </button>
          )}
        </div>
        <div className="flex-1 min-h-0 relative">
          {isExternal ? (
            /* external 应用（Hermes）：不嵌终端 —— 显示「运行在独立窗口」说明 + 各命令的再次打开按钮。
               点这些按钮/大按钮都走 term_open_external 弹独立系统终端，显示区域更大。 */
            <div className="h-full flex flex-col items-center justify-center gap-4 px-6 text-center">
              <ToolIcon tool={app.tool} size={48} active className="w-12 h-12" />
              <div className="text-[14px] font-medium text-ink-1">{t("{name} 运行在独立终端窗口", { name: app.name })}</div>
              <div className="max-w-[440px] text-[12px] leading-relaxed text-ink-4">
                {t("为了给 {name} 更大的显示区域，它在一个独立的系统终端窗口里运行，不挤在 U-King 界面里。关掉那个终端窗口即停止；下面按钮可再次打开。", { name: app.name })}
              </div>
              <div className="flex flex-wrap justify-center gap-2">
                {app.prompts.map((p) => (
                  <button
                    key={p.cmd}
                    onClick={() => void runExternal(p.cmd)}
                    className="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg border border-white/10 bg-white/[0.03] text-[12px] text-ink-2 hover:bg-white/[0.07] hover:text-ink-0"
                  >
                    <Play size={13} className="fill-current" />
                    {t(p.label)}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            /* 不传 initialCmd → 空终端；prompts = 顶栏提示词按钮；onReady 透出 runCmd */
            <TermPanel
              cwd=""
              active={active}
              tool={app.tool}
              prompts={app.prompts}
              onReady={(api) => (termApi.current = api)}
            />
          )}
          {/* 启动遮罩：还没点启动时盖住空终端，给小白一个明确的大按钮 + 一句话说明。
              点了就跑启动命令并隐藏。启动后想停就关终端标签，重进会重新盖（launched 重置）。 */}
          {!launched && (
            <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-5 bg-bg-0/92 backdrop-blur-[2px] px-6 text-center">
              <ToolIcon tool={app.tool} size={56} active className="w-14 h-14" />
              <div className="space-y-1.5 max-w-[420px]">
                <div className="text-[17px] font-semibold text-ink-0">{app.name}</div>
                <div className="text-[13px] leading-relaxed text-ink-3">
                  {t(APP_BLURB[app.tool] ?? "点下面的按钮开始使用。")}
                </div>
              </div>
              {isHermes && (
                <div className="w-full max-w-[520px] rounded-xl border border-white/[0.08] bg-bg-1/80 p-3 text-left">
                  <div className="flex items-start gap-2">
                    {hermesStatus?.browser_ready ? (
                      <CheckCircle2 size={16} className="mt-0.5 shrink-0 text-emerald-400" />
                    ) : (
                      <AlertTriangle size={16} className="mt-0.5 shrink-0 text-amber-400" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="text-[12px] font-semibold text-ink-1">
                        {checkingHermes
                          ? t("正在体检 Hermes 浏览器能力...")
                          : hermesStatus?.browser_ready
                            ? t("浏览器接管已就绪")
                            : t("浏览器接管未配置")}
                      </div>
                      <div className="mt-1 text-[11.5px] leading-relaxed text-ink-4">
                        {hermesStatus?.message ?? t("正在读取 Hermes 配置和浏览器工具状态。")}
                      </div>
                      <div className="mt-2 grid gap-1.5 text-[11px] text-ink-5">
                        <div>{t("启动：直接进终端对话界面（和 Claude Code 一样），输入任务即可。")}</div>
                        <div>{t("网页版聊天：备选入口，喜欢网页界面的可以用它。")}</div>
                        <div>{t("浏览器任务：未就绪时优先用 Codex 专区或 ClawX 做网页接管。")}</div>
                      </div>
                      {hermesStatus?.suggestions?.length ? (
                        <div className="mt-2 space-y-1 text-[11px] leading-relaxed text-ink-4">
                          {hermesStatus.suggestions.slice(0, 2).map((s) => (
                            <div key={s}>- {s}</div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                    <button
                      onClick={refreshHermesStatus}
                      title={t("重新体检 Hermes 浏览器能力")}
                      className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded text-ink-4 hover:bg-white/[0.06] hover:text-ink-1"
                    >
                      <RefreshCw size={14} className={checkingHermes ? "animate-spin" : ""} />
                    </button>
                  </div>
                </div>
              )}
              {isDsh ? (
                <div className="flex flex-wrap items-center justify-center gap-3">
                  <button
                    onClick={handleStart}
                    className="inline-flex items-center gap-2 h-12 px-7 rounded-xl bg-accent text-white text-[15px] font-semibold shadow-lg shadow-accent/30 hover:bg-accent-600 active:scale-[0.98] transition"
                  >
                    <Rocket size={18} />
                    {t("Web 工作台")}
                  </button>
                  <button
                    onClick={launchDshTerminal}
                    className="inline-flex items-center gap-2 h-12 px-7 rounded-xl border border-accent/50 bg-accent/[0.10] text-accent-200 text-[15px] font-semibold hover:bg-accent/[0.18] active:scale-[0.98] transition"
                  >
                    <SquareTerminal size={18} />
                    {t("终端模式")}
                  </button>
                </div>
              ) : (
                <button
                  onClick={handleStart}
                  className="inline-flex items-center gap-2 h-12 px-7 rounded-xl bg-accent text-white text-[15px] font-semibold shadow-lg shadow-accent/30 hover:bg-accent-600 active:scale-[0.98] transition"
                >
                  <Play size={18} className="fill-current" />
                  {isOpenClaw ? t("一键启动并打开网页版") : (app.launchLabel ? t(app.launchLabel) : t("启动 {name}", { name: app.name }))}
                </button>
              )}
              <div className="text-[11px] text-ink-5">
                {isExternal ? t("启动后在独立终端窗口运行，关掉那个窗口才会停止") : t("启动后会常驻运行，关掉终端标签才会停止")}
              </div>
            </div>
          )}
        </div>
      </div>

      {/* 右侧：驱动配置（可收起，只切该工具的驱动） */}
      {hasProviderConfig && cfgOpen && (
        <div className="w-[272px] shrink-0 flex flex-col border-l border-white/[0.06] bg-bg-1">
          <div className="flex items-center h-9 px-3 border-b border-white/[0.06] shrink-0">
            <span className="text-[12px] font-semibold text-ink-1 flex-1">{t("驱动配置")}</span>
            <button
              onClick={() => setCfgOpen(false)}
              title={t("收起")}
              className="inline-flex items-center justify-center w-7 h-7 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
            >
              <PanelRightClose size={15} />
            </button>
          </div>
          <div className="p-2 overflow-y-auto">
            <div className="text-[11px] text-ink-5 px-1 pb-1.5">{t("只切换 {name} 的驱动", { name: app.name })}</div>
            <ProviderSwitch
              targets={app.configTargets}
              deviceKey={deviceKey}
              onToast={onToast}
              onGoManage={onGoManage}
              onManageProviders={onManageProviders}
              onSwitched={onRefreshDriver}
            />
          </div>
        </div>
      )}
    </div>
  );
}
