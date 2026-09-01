/**
 * U-King 简化版 管理界面
 *
 * 一屏搞定 4 件事（对齐用户需求）：
 *  1. 一键安装到本地电脑（带进度）
 *  2. 注册/注销 右键目录菜单
 *  3. 说明右下角托盘（像 360 一样常驻）
 *  4. 工具市场 —— 可选安装 AI 工具
 *
 * 黑金御印视觉语言从复杂版 Dashboard 借用，但大幅精简：不依赖 UI 组件库，纯 Tailwind inline。
 */

import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { openRecharge } from "./lib/recharge";
import type { DeviceKey, DriverStatus } from "./lib/types";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cpu,
  Download,
  FlaskConical,
  FolderTree,
  LifeBuoy,
  PanelTopClose,
  PlugZap,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Terminal as TerminalIcon,
  Trash2,
  Wand2,
} from "lucide-react";
import { Wizard } from "./Wizard";
import { UWorkspace } from "./opencodex/UWorkspace";
import type { Expert } from "./opencodex/experts";
import { Sidebar, type TabId, type DockApp } from "./components/Sidebar";
import { ToolIcon } from "./components/ToolIcon";
import { TUI_APPS, VISIBLE_TUI_APPS, isTuiAppId } from "./opencodex/apps";
import type { Engine } from "./opencodex/types";
import { ProviderManager } from "./components/ProviderManager";
import { ApplyScopeDialog } from "./components/ApplyScopeDialog";
import { PanelBoundary } from "./components/PanelBoundary";

// 懒加载「条件渲染、非首屏」的页面 —— 首屏只解析外壳 + 当前页，其余按需拉各自 chunk。
// 治「打开慢」（原本一次性 eager import 全部页面塞进 1.6MB 主包），并缩小首屏解析面（降低
// Mac 内存压力/黑屏面）。命名导出映射成 default 给 React.lazy。首屏 myai（内联）/setup（Wizard）
// 与常驻保活的 UWorkspace 保持 eager，避免首屏 Suspense 闪烁。
const Manager = lazy(() => import("./Manager").then((m) => ({ default: m.Manager })));
const CodexZone = lazy(() => import("./CodexZone").then((m) => ({ default: m.CodexZone })));
const Create = lazy(() => import("./Create").then((m) => ({ default: m.Create })));
const Draw = lazy(() => import("./Draw").then((m) => ({ default: m.Draw })));
// 小程序图标条已收进实验室（2026-07-27 做减法），文件与 props 全保留，恢复解开这行即可。
const QrMerge = lazy(() => import("./QrMerge").then((m) => ({ default: m.QrMerge })));
const Video = lazy(() => import("./Video").then((m) => ({ default: m.Video })));
const Reel = lazy(() => import("./Reel").then((m) => ({ default: m.Reel })));
const MediaTasks = lazy(() => import("./Reel").then((m) => ({ default: m.MediaTasks })));
const Identity = lazy(() => import("./Identity").then((m) => ({ default: m.Identity })));
const Tutorial = lazy(() => import("./Tutorial").then((m) => ({ default: m.Tutorial })));
const Geo = lazy(() => import("./Geo").then((m) => ({ default: m.Geo })));
const SkillPack = lazy(() => import("./SkillPack").then((m) => ({ default: m.SkillPack })));
const Toolbox = lazy(() => import("./Toolbox").then((m) => ({ default: m.Toolbox })));
const AiRuntime = lazy(() => import("./AiRuntime").then((m) => ({ default: m.AiRuntime })));
const TokenSqueezer = lazy(() => import("./TokenSqueezer").then((m) => ({ default: m.TokenSqueezer })));
const Meter = lazy(() => import("./Meter").then((m) => ({ default: m.Meter })));
const NightShift = lazy(() => import("./NightShift").then((m) => ({ default: m.NightShift })));
const LocalLLM = lazy(() => import("./LocalLLM").then((m) => ({ default: m.LocalLLM })));
const Backup = lazy(() => import("./Backup").then((m) => ({ default: m.Backup })));
const Advanced = lazy(() => import("./Advanced").then((m) => ({ default: m.Advanced })));
const DemoUninstaller = lazy(() => import("./DemoUninstaller").then((m) => ({ default: m.DemoUninstaller })));
const Feedback = lazy(() => import("./Feedback").then((m) => ({ default: m.Feedback })));
const Guide = lazy(() => import("./Guide").then((m) => ({ default: m.Guide })));
const TerminalPage = lazy(() => import("./TerminalPage").then((m) => ({ default: m.TerminalPage })));
const DshPlugins = lazy(() => import("./DshPlugins").then((m) => ({ default: m.DshPlugins })));
const OpenCodex = lazy(() => import("./opencodex/OpenCodex").then((m) => ({ default: m.OpenCodex })));
const ToolAppView = lazy(() => import("./opencodex/ToolAppView").then((m) => ({ default: m.ToolAppView })));
const TeamSpace = lazy(() => import("./TeamSpace").then((m) => ({ default: m.TeamSpace })));
const RunCenter = lazy(() => import("./RunCenter").then((m) => ({ default: m.RunCenter })));
import { APP_VERSION } from "./version";
import Changelog from "./Changelog";
import { cn } from "./lib/cn";
import { useViewport } from "./lib/useViewport";
import { askConfirm } from "./lib/confirm";
import { startFileDrop } from "./lib/fileDrop";
import { useI18n } from "./i18n";

type AppEnv = {
  running_from_local: boolean;
  install_dir: string;
  context_menu_registered: boolean;
  opened_dir: string | null;
  platform: string;
  home_dir: string;
  demo_uninstaller?: boolean;
};

type InstallResult = {
  install_dir: string;
  shortcut: string | null;
  files_copied: number;
  bytes: number;
  was_update: boolean;
};

type TermSnapshotSession = { cwd: string | null; cmd: string | null; tool: string | null; resumeHint?: string | null; restoreKey?: number };
type TermSnapshotInfo = { sessions: TermSnapshotSession[] };

type ToolInfo = {
  id: string;
  name: string;
  summary: string;
  kind: "deep" | "standalone";
  installed: boolean;
  action: "install" | "url";
  target: string;
  launch_cmd: string;
  launch_app: string;
  // 后端标记的隐藏工具（Codex CLI / OpenClaw CLI）—— 前端统一过滤掉，不在市场/Dock 露出
  hidden?: boolean;
};

// 🔴 DriverStatus 从 `lib/types.ts` 来 —— 这里原本自己又定义了一份，两边已经漂了：
// lib 那份有 `active` / `dsh_model` / `dsh_installed` / `extra_installed`，本地这份没有；
// 本地这份有 `claude_own_key` / `codex_own_key`，lib 那份没有。
// 结果是「首页想显示 DSH 当前模型」时 tsc 报 dsh_model 不存在，而同一个字段在
// ProviderSwitch.tsx 里用得好好的。同一事实存在几份就会漂几份（宪法第 8 条）。


type ThemeMode = "light" | "dark";

/** 懒加载页面切换时的短暂占位（通常 <100ms）。中性 spinner，不依赖 i18n/主题文案。 */
function PageFallback() {
  return (
    <div className="flex-1 grid place-items-center py-16 text-ink-4">
      <div className="h-6 w-6 animate-spin rounded-full border-2 border-white/10 border-t-accent" />
    </div>
  );
}

export function App() {
  const { t: tr } = useI18n();
  const [env, setEnv] = useState<AppEnv | null>(null);
  const [tools, setTools] = useState<ToolInfo[]>([]);
  // openclaw CLI 是否已装 —— 它在 tools.rs 里 hidden=true 会被下面 `!x.hidden` 过滤掉，
  // 但「OpenClaw 网页版」Dock 图标要据此着色，故单独从未过滤的原始列表里取一次。
  const [openclawInstalled, setOpenclawInstalled] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  // ClawX 首次启动「允许访问网络」引导浮层：ClawX 是 Electron，第一次开会弹 Windows
  // 防火墙警报，小白点了「取消」就联网失败 = 主力工具打不开。这里在启动前弹一次醒目说明。
  const [clawxNetHint, setClawxNetHint] = useState<null | (() => void)>(null);
  // ClawX 下载/安装进度（210MB 大包，常驻进度条让小白安心，不以为卡死）。
  const [clawxProgress, setClawxProgress] = useState<string | null>(null);
  const [driver, setDriver] = useState<DriverStatus | null>(null);
  /** 「一键配好全部 AI」的勾选框开着没（动手前让用户看清改谁、能不改）。 */
  const [applyScope, setApplyScope] = useState(false);
  const [deviceKey, setDeviceKey] = useState<DeviceKey | null>(null);
  const rechargeRefreshTimers = useRef<number[]>([]);
  const lastRechargeOpenAt = useRef(0);
  const [theme, setTheme] = useState<ThemeMode>(() =>
    localStorage.getItem("uking.theme") === "dark" ? "dark" : "light",
  );
  // 对话式安装向导：runId 每次 +1 强制重开新会话
  const [wizard, setWizard] = useState<{ runId: number; preselect: string | null } | null>(null);
  // 左侧边栏：setup=装机向导 / myai=我的 AI（已装快捷启动）/ manage=AI 设置（切驱动+余额+用量）。
  const [tab, setTab] = useState<TabId>("setup");
  /**
   * 「自己管高度」的页面（测试报告 #005：AI 创作区出现两条滚动条）。
   *
   * Draw / Video / QrMerge 都是**内部自己滚**的三段式布局（顶栏 + 可滚内容 + 钉底输入框），
   * 高度写死成 `h-[calc(100vh-7rem)]`。可它们外面还套着一个 `overflow-y-auto` + `py-6` 的 main，
   * 再加上「AI 创作」自己的标签条 —— 加起来必然超出一屏，于是外层**又长出一条滚动条**：
   * 客户看到的就是右边缘并排两条，滚哪条都只动半个页面。
   *
   * 解法不是去调那个 7rem 魔法数（调了也只在这一种窗口尺寸下对），而是把高度链接通：
   * 这些页所在的 main 不滚、不留 py-6，页面本体改 `h-full` 从父级拿确定高度。
   */
  const selfHeightTab = tab === "create" || tab === "draw" || tab === "video" || tab === "reel" || tab === "qrmerge";
  // 小程序浮层：首页图标条和小程序页都往这里塞，容器只有一个
  // 「召唤」handoff：AI 专家页点召唤 → 存这里 + 切到 U-Workspace(chat) → UWorkspace 消费后清空
  const [pendingExpert, setPendingExpert] = useState<Expert | null>(null);
  // 「发一句话」handoff：DSH 插件页「让 AI 帮你挑」→ 存这里 + 切到 chat → UWorkspace 开会话把它发给 AI
  const [pendingChatPrompt, setPendingChatPrompt] = useState<{ prompt: string; engine?: Engine; passportId?: string } | null>(null);
  // 访问过的 TUI 应用（懒挂载，挂载后常驻 display 切换保活，不卸载 → PTY 续跑）
  const [mountedTui, setMountedTui] = useState<Set<string>>(new Set());
  useEffect(() => {
    if (isTuiAppId(tab)) setMountedTui((s) => (s.has(tab) ? s : new Set(s).add(tab)));
  }, [tab]);
  // 全站文件拖放：统一走 Tauri 原生拖放事件（拿真实文件路径），只装一次全局监听
  useEffect(() => {
    startFileDrop();
  }, []);
  // 终端页 + 待运行命令（点工具「打开终端」时塞进来）
  const [termMounted, setTermMounted] = useState(false);
  const [pendingCmd, setPendingCmd] = useState<string | null>(null);
  const [termSnapshot, setTermSnapshot] = useState<TermSnapshotInfo | null>(null);
  const [pendingTermRestores, setPendingTermRestores] = useState<TermSnapshotSession[] | null>(null);
  // 恢复失败项不再塞回 pending（那会触发 effect 立即自动重试）；只等用户点「重试」才重入队列。
  const [failedTermRestores, setFailedTermRestores] = useState<TermSnapshotSession[] | null>(null);
  const [recoveringTermSnapshot, setRecoveringTermSnapshot] = useState(false);
  // 进终端页就懒挂载，挂载后常驻保活（display 切换，PTY 续跑）
  useEffect(() => {
    if (tab === "terminal") setTermMounted(true);
  }, [tab]);
  // 自升级是硬退出，旧 PTY 无法存活；新版只在「我的 AI」放一次轻量重开入口。
  useEffect(() => {
    invoke<TermSnapshotInfo | null>("term_snapshot_pending").then(setTermSnapshot).catch(() => {});
  }, []);
  // 新版检测
  const [update, setUpdate] = useState<{
    current: string;
    latest: string;
    has_update: boolean;
    notes: string;
    download_url: string;
    history?: { version: string; date?: string; notes?: string }[];
    /** 这台机器自动升级到该版本失败过几次（后端本地账本）。≥1 → 界面改推「下载安装包重装」。 */
    failed_attempts?: number;
    fail_reason?: string;
    installer_url?: string;
  } | null>(null);
  const [clawxDismissed, setClawxDismissed] = useState(false);
  // 更新日志弹层（点侧栏版本号打开）。数据全来自已经拉好的 update，不额外请求。
  const [changelogOpen, setChangelogOpen] = useState(false);
  // 「给 AI 装作图能力」推荐条：点「去装」或「不用了」后持久化，不再重复弹。
  const [aigcDismissed, setAigcDismissed] = useState(() => localStorage.getItem("uking.aigcNudgeDone") === "1");
  const [updating, setUpdating] = useState(false);
  const [updatePct, setUpdatePct] = useState<number | null>(null);
  // provider 增删改弹层：null=关；{}=新建；{editId}=编辑该项
  const [providerMgr, setProviderMgr] = useState<{ editId?: string } | null>(null);
  // 半成品状态引导（装了工具没配驱动 / 配了没充值 等）
  const [setupState, setSetupState] = useState<{
    has_tool: boolean;
    has_driver: boolean;
    charged: boolean;
    clawx_needs_xiapan: boolean;
    next_step: string;
    hint: string;
  } | null>(null);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    localStorage.setItem("uking.theme", theme);
  }, [theme]);

  useEffect(() => {
    const run = () =>
      invoke<typeof update>("check_update")
        .then((u) => {
          setUpdate(u);
        })
        .catch(() => {});
    run();
    // 定时复查：check_update 原来只在挂载查一次，长期不关 App 的用户在发版后永远收不到新版提示
    //（客户实锤「没有自动升级提示」）。每 2 小时复查一次，让老会话也能自动发现新版。
    const timer = setInterval(run, 2 * 60 * 60 * 1000);
    return () => clearInterval(timer);
  }, []);

  // 自升级替换后首次启动：后端会留一个「已升级」标记，弹条确认让用户确信升成功、无需重装。
  // 但一句「升级成功 ✓」只回答了**成没成**，没回答**升了什么** —— 后者才是客户真正想知道的
  // （「你让我升级，又不告诉我升了有什么用」）。所以顺手把更新日志直接摆到脸前。
  const [justUpdated, setJustUpdated] = useState(false);
  useEffect(() => {
    invoke<boolean>("take_update_flag")
      .then((ok) => {
        if (ok) {
          flash(tr("已升级到最新版 v{v} ✓", { v: APP_VERSION }));
          setJustUpdated(true);
        }
      })
      .catch(() => {});
  }, []);
  // history/notes 是 check_update 异步拉回来的，早开一步会开出一个「暂时拿不到更新日志」的空壳。
  // 等数据到了再弹；真拉不到（离线）就安静地算了，不摆空壳糊弄人。
  useEffect(() => {
    if (!justUpdated || !update) return;
    setChangelogOpen(true);
    setJustUpdated(false);
  }, [justUpdated, update]);

  // 内置 Key + 余额（带网络查询，单独懒加载，不阻塞 refresh）
  useEffect(() => {
    invoke<DeviceKey>("get_device_key")
      .then(setDeviceKey)
      .catch(() => setDeviceKey(null));
  }, [wizard?.runId]);

  // 半成品状态：装了没配 / 配了没充值，启动时与每次驱动变化后检测
  useEffect(() => {
    invoke<typeof setupState>("get_setup_state").then(setSetupState).catch(() => {});
  }, [wizard?.runId, driver]);

  // 首屏落点只算一次：有已装工具 → 我的 AI，否则 → 装机向导。之后只听用户手动点。
  const landed = useRef(false);
  const refresh = useCallback(async () => {
    // 三个探测**并行**跑：彼此不依赖，串起来只是把等待时间相加。
    // `list_tools` 里每个工具的「装没装」是真起进程跑 `--version`（本机实测合计约 3.2s，
    // 光 hermes 就 2.3s），串行等于开机先白等三个探测之和 —— 首屏落点还卡在最后一个后面。
    const [e, raw, d] = await Promise.all([
      invoke<AppEnv>("get_env").catch(() => null),
      // 原始列表（含 hidden）—— openclaw 着色判据从这里取；展示用的 tools 再过滤掉 hidden。
      invoke<ToolInfo[]>("list_tools").catch(() => [] as ToolInfo[]),
      invoke<DriverStatus>("get_driver_status").catch(() => null),
    ]);
    setEnv(e);
    setOpenclawInstalled(raw.some((x) => x.id === "openclaw" && x.installed));
    // 过滤掉后端标记 hidden 的工具（Codex CLI / OpenClaw CLI）—— 全应用统一只见可见工具
    const t = raw.filter((x) => !x.hidden);
    setTools(t);
    setDriver(d);
    if (!landed.current) {
      landed.current = true;
      // 安装器定位：第一次打开就落「① 装 AI」——主力推荐 ClawX + 一键全安装就在眼前，
      // 不再落 Codex 工作站（那把新用户先带去 Codex，弱化了「这是个装机器」的第一印象）。
      if (localStorage.getItem("uking.seenGuide") !== "1") {
        localStorage.setItem("uking.seenGuide", "1");
        setTab("myai");
      } else {
        // 回访：装过工具 → 默认进 **U-Workspace**；没装过 → 装机向导。
        //
        // 🔴 2026-08-16 改：原来落「终端」。那是个裸黑框，客户原话「首页那个莫名其妙的终端」——
        // 对会敲命令的人它是首选，对我们真正的用户（不会敲命令、来这儿是为了让 AI 干活的人）
        // 它就是一堵墙：没有任何提示说下一步该干什么。首页只该是这两个之一 ——
        // **还没装好就带他装完**，**装好了就带他干活**。终端照旧在侧栏和工作台右侧，一点就有。
        // open365 是系统管家（非 AI 工具），U 盘随盘带会被判「已装」——排除它，
        // 否则只带了 Open365、没装任何 AI 的用户会被误带进 myai，跳过了装 AI 向导。
        const hasInstalled = t.some(
          (x) => x.installed && (x.launch_cmd || x.launch_app) && x.id !== "open365",
        );
        setTab(hasInstalled ? "chat" : "setup");
      }
    }
    // 返回原始（含 hidden）列表，让点击处理器能据「确认后的状态」决策（避免读到旧 state）。
    // 含 hidden 才能让 onLaunchDock 对 openclaw 这类隐藏工具做「装没装」再校验。
    return raw;
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 「这是不是并行调试实例」（后端 `--allow-multi-instance` 起的第二个 U-King）。
  //
  // **两条路都要，缺一不可**：`instance_role` 命令负责「进来先问一遍」（后端在 setup 里
  // emit 的时候前端还没挂载，那个事件正常情况下必然错过）；事件负责「刚好在场时立刻知道」。
  // 只留事件那半边的话，你打开窗口时看到的永远是一片正常 —— 而降权是静默的。
  const [isSidecar, setIsSidecar] = useState(false);
  useEffect(() => {
    let alive = true;
    invoke<{ role?: string }>("instance_role")
      .then((v) => {
        if (alive && v?.role === "sidecar") setIsSidecar(true);
      })
      .catch(() => {});
    const un = listen("uking:sidecar-mode", () => setIsSidecar(true));
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);

  // 又双击了一次 U-King（单实例把第二个进程挡回去了，只留这一个窗口）。
  // 必须重拉一次 env：那次点击如果是右键菜单「用 U-King 打开」，目录正暂存在后端等人来取，
  // 不取就悄悄丢了 —— 窗口被顶到前面、目录却没进工作台，比多开一个窗口更让人摸不着头脑。
  useEffect(() => {
    const un = listen("uking:reopen", () => void refresh());
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  // 🔴 「下载的绿色版点了没反应」的解释（客户 2026-08-18 反馈）。
  // 关窗口默认缩托盘 → U-King 几乎一直在跑 → 双击另一份 exe 时单实例锁让它交棒后退出，
  // 只把这个窗口顶到前面。**不许两个实例是对的，静默才是 bug** —— 这条把「没反应」
  // 变成一句能看懂的话，顺便告诉他那份在哪。
  useEffect(() => {
    const un = listen<{ other?: string }>("uking:second-instance", (e) => {
      const other = e.payload?.other ?? "";
      const name = other.split(/[\\/]/).pop() || other;
      // 直接用 setToast 不用 flash：`flash` 是每次渲染新建的普通函数，声明还在下面，
      // 放进依赖会让这个订阅每渲染重来一次。
      setToast(
        name
          ? tr("U-King 本来就开着（在托盘里），已经帮你切回来了 —— 你双击的那份「{name}」不会再开一个窗口。", { name })
          : tr("U-King 本来就开着（在托盘里），已经帮你切回来了。"),
      );
      window.setTimeout(() => setToast(null), 5200);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 安装进度事件（托盘菜单触发的「安装到本地」时用，仅打日志）
  useEffect(() => {
    const un = listen<{ rel: string; count: number }>("uking:install_progress", () => {});
    // 卸载进度：npm/pip/官方卸载程序可能跑十几秒，实时把进度 toast 出来，别让人以为卡死。
    const un2 = listen<string>("uking:uninstall_progress", (e) => setToast(e.payload));
    return () => {
      un.then((f) => f());
      un2.then((f) => f());
    };
  }, []);

  // 托盘菜单触发的动作
  useEffect(() => {
    const un = listen<string>("uking:tray_action", (e) => {
      if (e.payload === "install") doInstall();
      if (e.payload === "manage") setTab("manage");
      if (e.payload === "about") setToast(tr("U-King · AI 管家 · v{v}", { v: APP_VERSION }));
    });
    // 托盘驱动快捷切换的结果（后台线程切完 emit 过来；成功/失败都 toast 告知）
    const un2 = listen<string>("uking:tray_driver_result", (e) => setToast(e.payload));
    return () => {
      un.then((f) => f());
      un2.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 定时任务跑完的通知。**挂在 App 顶层而不是自动化面板里**：面板收起来就 unmount 了，
  // 而任务恰恰是在人没看着的时候跑的 —— 挂在面板里等于只有盯着它的人才收得到通知。
  // 后端是唯一发出点（automation::execute 里的 notifier），到点触发和「立即运行」同一条路。
  useEffect(() => {
    const un = listen<{ id: string; name: string; ok: boolean; summary: string }>(
      "uking:automation_done",
      (e) => {
        const { name, ok, summary } = e.payload;
        const brief = summary.length > 40 ? `${summary.slice(0, 40)}…` : summary;
        setToast(
          ok
            ? tr("自动化「{name}」跑完了：{s}", { name, s: brief })
            : tr("自动化「{name}」没跑成：{s}", { name, s: brief }),
        );
        // 失败停久一点 —— 成功可以一眼扫过，失败要让人来得及看清
        window.setTimeout(() => setToast(null), ok ? 6000 : 10000);
      },
    );
    return () => {
      un.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const flash = (msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 3200);
  };

  // 「正在刷新」的转圈现在归 WalletCard 自己管（它 await 这个 Promise）——
  // App 这里不再另存一份 refreshing 状态：两处各存一份，迟早有一处忘了清。
  const refreshDeviceKey = useCallback(async (silent = false) => {
    try {
      const dk = await invoke<DeviceKey>("get_device_key");
      setDeviceKey(dk);
      invoke<typeof setupState>("get_setup_state").then(setSetupState).catch(() => {});
      if (!silent) {
        flash(dk.charged ? tr("已刷新余额：{text}", { text: dk.balance?.text ?? tr("可用") }) : tr("还没查到充值到账，稍等几秒再刷新"));
      }
      return dk;
    } catch (e) {
      if (!silent) flash(tr("刷新余额失败：") + String(e));
      return null;
    }
  }, [tr]);

  const openRechargeAndWatch = useCallback(
    async (url?: string) => {
      lastRechargeOpenAt.current = Date.now();
      rechargeRefreshTimers.current.forEach((id) => window.clearTimeout(id));
      rechargeRefreshTimers.current = [];
      await openRecharge(url);
      flash(tr("已打开充值页；付款后回到 U-King 会自动刷新，也可以点「刷新余额」"));
      [6000, 18000, 45000].forEach((ms) => {
        const id = window.setTimeout(() => void refreshDeviceKey(true), ms);
        rechargeRefreshTimers.current.push(id);
      });
    },
    [refreshDeviceKey, tr],
  );

  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - lastRechargeOpenAt.current < 10 * 60 * 1000) {
        void refreshDeviceKey(true);
      }
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      rechargeRefreshTimers.current.forEach((id) => window.clearTimeout(id));
    };
  }, [refreshDeviceKey]);

  // 托盘菜单「安装到本地」触发（界面上不再露出此卡）
  const doInstall = async () => {
    if (installing) return;
    setInstalling(true);
    try {
      const r = await invoke<InstallResult>("install_local");
      flash(r.was_update ? tr("已更新到本地最新版") : tr("安装完成！桌面已生成快捷方式"));
      refresh();
    } catch (e) {
      flash(tr("安装失败：") + String(e));
    } finally {
      setInstalling(false);
    }
  };

  const confirmTerminalShutdown = async (action: "upgrade" | "reinstall"): Promise<number | null> => {
    let count: number;
    try {
      count = await invoke<number>("term_active_count");
    } catch {
      flash(tr("无法确认终端运行状态，已暂停本次升级以保护运行中的终端，请稍后再试"));
      return null;
    }
    if (count <= 0) return count;
    const message = action === "upgrade"
      ? tr("有 {n} 个终端正在运行，升级会关闭它们。是否立即升级？取消则暂不执行。", { n: count })
      : tr("有 {n} 个终端正在运行，重装会关闭它们。是否立即重装？取消则暂不执行。", { n: count });
    const ok = await askConfirm(message);
    if (!ok) flash(action === "upgrade" ? tr("升级已延后，终端关闭后可再来升级") : tr("重装已延后，终端关闭后可再来重装"));
    return ok ? count : null;
  };

  // 应用内一键升级：后台下新版 → 替换当前 app/exe → 自动重启。其它平台回退开下载页。
  const doSelfUpdate = async () => {
    if (updating) return;
    // 曾经这里是 `if (updating || !update) return` —— update 还没拉到时点按钮**什么都不发生**，
    // 连一句提示都没有，用户只能反复点。没拿到版本信息就如实说，别装作没点过。
    if (!update) {
      flash(tr("还没拿到服务器版本信息（可能网络不通），请稍等几秒再试"));
      return;
    }
    if (env?.platform && env.platform !== "windows" && env.platform !== "macos") {
      openUrl(update.download_url).catch(() => {});
      return;
    }
    const ackTerminalCount = await confirmTerminalShutdown("upgrade");
    if (ackTerminalCount == null) return;
    setUpdating(true);
    setUpdatePct(0);
    flash(tr("正在下载新版…"));
    // 监听后端下载/替换进度（uking:update_progress），把百分比显示到横幅按钮上，下载不再像「卡住」
    const un = await listen<{ phase: string; percent: number }>("uking:update_progress", (e) => {
      if (e.payload.phase === "download") setUpdatePct(e.payload.percent);
      else if (e.payload.phase === "swap") setUpdatePct(100);
    });
    try {
      await invoke("self_update", { ackTerminalCount });
      un();
      setUpdatePct(100);
      // 进程 ~0.8s 后退出，替换脚本覆盖旧 exe 并拉起新版。明确告知会闪一下、且无需重装。
      flash(tr("新版已下载完成，正在自动替换并重启 —— 窗口会消失几秒，请稍候，会自动打开新版（无需重新安装）"));
    } catch (e) {
      un();
      setUpdating(false);
      setUpdatePct(null);
      // ★ 失败后**重新拉一次 check_update**：后端刚把这次失败记进本地账本，重拉才能让
      // 侧栏那个按钮当场从「一键升级」改口成「下载安装包重装」。不重拉的话，界面会继续
      // 劝用户走同一条已经走不通的路 —— 这正是「老是有新版本、就是升不上去」的循环。
      invoke<typeof update>("check_update").then(setUpdate).catch(() => {});
      // 依旧【不】自动打开下载页/自动下安装包：重装是用户的决定，我们只把路指出来。
      flash(tr("自动升级未成功：") + String(e) + tr(" —— 左下角按钮已切换成「下载安装包重装」，点它即可（配置和对话不会丢）。"));
    }
  };

  // 自动升级走不通时的兜底：下载官网安装包 → 打开它 → U-King 退出让它覆盖安装。
  // 这条路和自动替换**不共用任何机制**：安装程序自己有权限模型和杀软信任度，
  // 不依赖我们那段替换脚本，所以自动替换失败的机器上它通常照样能装上。
  const doReinstall = async () => {
    if (updating) return;
    const ackTerminalCount = await confirmTerminalShutdown("reinstall");
    if (ackTerminalCount == null) return;
    setUpdating(true);
    setUpdatePct(0);
    flash(tr("正在下载官网安装包…"));
    const un = await listen<{ phase: string; percent: number }>("uking:update_progress", (e) => {
      if (e.payload.phase === "download") setUpdatePct(e.payload.percent);
    });
    try {
      const path = await invoke<string>("reinstall_latest", { ackTerminalCount });
      un();
      setUpdatePct(100);
      flash(
        tr("安装包已下载到 {p}，正在打开安装程序 —— U-King 会先退出，一路「下一步」装完会自动打开新版（配置和对话不会丢）", {
          p: path,
        }),
      );
    } catch (e) {
      un();
      setUpdating(false);
      setUpdatePct(null);
      // 连安装包都下不下来（多半是这台机器上不了网/被拦），那就只剩「自己去官网下」这一条路，
      // 直接把下载页开出来 —— 到这一步再让用户自己找网址就太不像话了。
      flash(tr("安装包下载失败：") + String(e) + tr(" —— 已为你打开官网下载页，手动下载安装即可"));
      openUrl(update?.installer_url || update?.download_url || "https://u-claw.org.cn/uking/").catch(() => {});
    }
  };

  // 仅启动 ClawX，失败也**不**回退安装 —— 给「检测到已装 / 刚装完」的路径用，
  // 杜绝「检测说装了、但 find_clawx_exe 找不到」时 launch⇄install 来回弹的死循环。
  const launchClawxSoft = (t: ToolInfo) => {
    invoke("launch_app", { app: t.launch_app })
      .then(() => flash(tr("正在打开 {name}…", { name: t.name })))
      .catch(() => flash(tr("ClawX 已安装，请从开始菜单 / 桌面图标打开")));
  };

  const openTool = async (t: ToolInfo) => {
    if (t.id === "clawx") {
      // ① 先「检测一次」：点击时拿一份最新探测结果，已装就直接打开 app —— 绝不因 state 旧了
      //    就盲目重下 210MB 重装（客户实测：明明装了 ClawX，却又被装一遍）。
      setClawxProgress("正在检测 ClawX…");
      const fresh = await refresh().catch(() => null);
      const cur = fresh?.find((x) => x.id === "clawx");
      if (cur?.installed) {
        setClawxProgress(null);
        flash(tr("已检测到 ClawX，正在打开…"));
        launchClawxSoft(cur);
        return;
      }
      // ② 确认没装才下载 + 静默安装（NSIS /S）。装完**不再自动接虾盘云**——用户主动装 ClawX
      // 不等于同意切驱动；装完 refresh 后 setupState.clawx_needs_xiapan 会触发顶部提示条，
      // 引导用户「自己点」接入（cc-switch 哲学，绝不静默改用户配置）。
      // 监听后端进度事件，实时显示「正在下载 12 MB / 安装中…」，不让人以为卡死。
      setClawxProgress("开始下载 ClawX（约 210 MB）…");
      const un = await listen<string>("uking:clawx_progress", (e) => setClawxProgress(e.payload));
      try {
        const msg = await invoke<string>("install_clawx");
        un();
        setClawxProgress(null);
        flash(msg + tr("（装好后可在顶部「接入虾盘云」一键配置）"));
        setClawxDismissed(false); // 重新装了 ClawX，允许提示条再次出现
        // ③ 装完再检测一次：确认已识别就直接打开 app（用户预期「装完就用」），
        //    没识别到（多半是后台还在装/非静默回退）就只刷新，不强行 launch 触发死循环。
        const after = await refresh().catch(() => null);
        if (after?.find((x) => x.id === "clawx")?.installed) {
          launchClawxSoft(t);
        }
      } catch (e) {
        un();
        setClawxProgress(null);
        // 静默装失败：回退打开图文教程（教程内含 Windows/Mac 直链和手动步骤）
        flash(tr("自动安装未成：") + String(e));
        await invoke("open_install_guide", { tool: "clawx" })
          .catch(async () => {
            const url = await invoke<string>("get_clawx_download_url").catch(() => t.target);
            await openUrl(url).catch(() => {});
          });
      }
      return;
    }
    if (t.id === "uu-switch") {
      // uu-switch（去广告版 cc-switch 模型切换器）= GUI 应用，走后端下载 + 静默安装（不进向导）。
      // 已装 → 直接打开；没装 → 下载安装（进度用 toast），装完自动打开。装完不改用户任何配置。
      const fresh = await refresh().catch(() => null);
      const cur = fresh?.find((x) => x.id === "uu-switch");
      if (cur?.installed) {
        flash(tr("已检测到 uu-switch，正在打开…"));
        doLaunchApp(cur);
        return;
      }
      flash(tr("开始下载并安装 uu-switch（约 12 MB）…"));
      const un = await listen<string>("uking:uuswitch_progress", (e) => flash(e.payload));
      try {
        const msg = await invoke<string>("install_uuswitch");
        un();
        flash(msg);
        const after = await refresh().catch(() => null);
        if (after?.find((x) => x.id === "uu-switch")?.installed) doLaunchApp(t);
      } catch (e) {
        un();
        flash(tr("自动安装未成：") + String(e) + tr("，可到工具卡点「打开下载页」手动装"));
        const url = await invoke<string>("get_uuswitch_download_url").catch(() => t.target);
        await openUrl(url).catch(() => {});
      }
      return;
    }
    if (t.action === "url") {
      await openUrl(t.target).catch(() => flash(tr("打开链接失败")));
    } else {
      // 进对话式向导，预选该工具。Wizard 只在「装机向导(setup)」页渲染 —— 必须切过去，
      // 否则在「我的 AI」页点一键安装会 setState 但看不到向导（实测「点了没反应」的真因）。
      setTab("setup");
      setWizard((w) => ({ runId: (w?.runId ?? 0) + 1, preselect: t.id }));
    }
  };

  // U-Workspace「没装 Claude Code 就装」：默认对话大脑是 Claude Code，客户没装时走和工具中心
  // 完全一致的一键安装（进装机向导预选 claude-code）。装完 agent/claude.rs 注入 delegation_env
  // 自动接虾盘云（deepseek-v4-flash·同计费·免配置），无需再单独配驱动 —— 装完即用。
  const installClaude = () => {
    const t = tools.find((x) => x.id === "claude-code");
    if (t) return openTool(t);
    setTab("setup");
    setWizard((w) => ({ runId: (w?.runId ?? 0) + 1, preselect: "claude-code" }));
  };

  // 真正拉起 GUI 应用。启动失败（多半是「还没装 / 没装好」）→ 不留死胡同，引导去安装。
  const doLaunchApp = (t: ToolInfo) => {
    invoke("launch_app", { app: t.launch_app })
      .then(() => flash(tr("正在打开 {name}…", { name: t.name })))
      .catch(() => {
        // ClawX：找不到 exe 八成是没装/没装全 → 直接转安装流，而不是甩个报错给小白。
        if (t.id === "clawx") {
          flash(tr("没找到 ClawX，先帮你安装…"));
          openTool(t);
        } else {
          flash(tr("{name} 还没装好，请先在工具中心安装", { name: t.name }));
        }
      });
  };

  // 启动工具：GUI 应用直接打开程序，CLI 工具进应用内终端自动运行
  const launchTool = (t: ToolInfo) => {
    // OpenClaw：卡片已 hidden（2026-07-07 起 ClawX = 唯一人类入口），正常不会走到这里；
    // 留着这条路由是为将来把 hidden 改回 false 时无需重接线。
    if (t.id === "openclaw") {
      setTab("openclaw");
      return;
    }
    // Hermes：点卡片进 Hermes app 页（ToolAppView）。大按钮「启动」先按需配好虾盘云
    // （ensureWebToolConfigured 传 model:null → 落 preset 默认 deepseek-v4-flash）再进终端 TUI
    // （2026-07-07 定：终端优先，网页版降为备选）。不走下面 term_open_external 裸跑 `hermes`：
    // 那条不配虾盘云，是当年客户「Hermes 还要手动配很多」的根因。claude 仍走终端：它本就
    // 命令行、且策略上不替用户切驱动（常有自己的 Pro/Max 订阅），所以不在此列。
    if (t.id === "hermes") {
      setTab("hermes");
      return;
    }
    // DeepSeek Harness：进入专属页，由 ToolAppView 启动 dsh web、等端口就绪并自动开浏览器。
    // 不能走通用外部终端，否则客户只看到 server 日志，却不知道工作台在 127.0.0.1:3080。
    if (t.id === "dsh") {
      setTab("dsh");
      return;
    }
    if (t.launch_app) {
      // ClawX 首次启动：先弹「允许访问网络」引导（只弹一次，记 localStorage）。
      const firstClawx =
        t.launch_app === "clawx" && localStorage.getItem("uking.clawxNetHintShown") !== "1";
      if (firstClawx) {
        setClawxNetHint(() => () => {
          localStorage.setItem("uking.clawxNetHintShown", "1");
          setClawxNetHint(null);
          doLaunchApp(t);
        });
        return;
      }
      doLaunchApp(t);
      return;
    }
    if (!t.launch_cmd) {
      flash(tr("该工具从开始菜单 / 应用列表打开"));
      return;
    }
    // 「打开终端」= 弹一个独立的系统终端窗口（带注入好的 PATH），就像打开一个独立 app。
    // 关掉 U-King 主窗口它照常活着（openclaw gateway 不被一起杀）。失败再回落到内嵌终端页。
    invoke("term_open_external", { cmd: t.launch_cmd })
      .then(() => flash(tr("已为 {name} 打开独立终端", { name: t.name })))
      .catch(() => {
        setPendingCmd(t.launch_cmd!);
        setTab("terminal");
      });
  };

  // 卸载一个 AI 工具：删本体 + 一切会被探测成"已装"的残留（修「删了还检测到、重装又冒出来」）。
  // 铁律：这是"帮你装的工具本体"，二次确认（若你之前自己装过会真删掉）。进度走事件 toast。
  const uninstallTool = async (t: ToolInfo) => {
    const ok = await askConfirm(
      tr(
        "确定卸载「{name}」吗？\n\n会删掉它本体，以及 U-King 相关残留（让它不再被检测成「已装」）。\n若你之前是自己装的、其它软件也在用，请勿卸载。",
        { name: t.name },
      ),
    );
    if (!ok) return;
    flash(tr("正在卸载 {name}…", { name: t.name }));
    try {
      const msg = await invoke<string>("uninstall_ai_tool", { toolId: t.id });
      flash(String(msg));
    } catch (e) {
      flash(tr("卸载失败：") + String(e));
    } finally {
      await refresh();
    }
  };

  // 底部 Dock：全部 TUI 应用（已装彩色 / 未装灰显）+ 纯 GUI 应用（外部启动）
  // 注：OpenCodex 工作台暂时隐藏 —— 先把「装机 + 使用 AI」打磨好。代码全留着，
  // 渲染靠 display:none 控制，tab 永远切不到 "workbench" 即不显示。要恢复：把下面
  // { id:"opencodex", name:"OpenCodex", kind:"workbench" } 加回数组首项即可。
  const tuiToolIds = new Set(TUI_APPS.map((a) => a.toolId));
  const dockApps: DockApp[] = [
    // 纯 GUI 应用（仅 launch_app，非 TUI）→ 外部启动，归「桌面应用」组。已装的都列；
    // ClawX 即使没装也列（灰显），点了跳官方下载（给客户更多选择 + 一键入口）。
    ...tools
      .filter(
        (t) => t.launch_app && !tuiToolIds.has(t.id) && (t.installed || t.id === "clawx"),
      )
      .map((t): DockApp => ({
        id: t.id,
        name: t.name,
        kind: "launch",
        tool: t.id,
        active: t.installed,
        group: "desktop",
      })),
    // 可见 TUI 应用（隐藏掉 codex-cli）→「命令行工具」组；installed → 彩色，否则灰显
    ...VISIBLE_TUI_APPS.map((a): DockApp => ({
      id: a.id,
      name: a.name,
      kind: "tui",
      tabId: a.id,
      tool: a.tool,
      // openclaw 的 ToolInfo 被 hidden 过滤出 tools，故单独用 openclawInstalled 判着色。
      // ⚠️ 2026-08-05 起 apps.ts 里 openclaw 也 hidden 了，VISIBLE_TUI_APPS 不再产出它，
      // 于是这个分支**当前命中不到**。保留不删：复活时把两处 hidden 一起去掉就能直接工作，
      // 现在删了将来还得重写一遍（且容易漏掉「它不在 tools 里」这个前提）。
      active:
        a.toolId === "openclaw"
          ? openclawInstalled
          : tools.some((t) => t.id === a.toolId && t.installed),
      group: a.group ?? "cli",
    })),
  ];
  const onLaunchDock = async (a: DockApp) => {
    if (a.kind === "workbench") return setTab("workbench");
    if (a.kind === "tui") {
      // 未装的灰色图标 → 先「检测一次」：可能是装好了但 state 还旧（如 Hermes 装完没刷新到）。
      // 真没装才进装机向导；已检测到就直接开 TUI 页，别再让用户白装一遍。
      if (!a.active) {
        const app = TUI_APPS.find((x) => x.id === a.tabId);
        const fresh = await refresh().catch(() => null);
        const ok = !!app && (fresh ?? tools).some((t) => t.id === app.toolId && t.installed);
        if (ok) return setTab(a.tabId);
        setTab("setup");
        setWizard((w) => ({ runId: (w?.runId ?? 0) + 1, preselect: app?.toolId ?? null }));
        return;
      }
      return setTab(a.tabId);
    }
    // launch：纯 GUI 应用。已装 → 启动；没装（如 ClawX 灰显）→ 走 openTool（url 跳下载）
    const t = tools.find((x) => x.id === a.id);
    if (!t) return;
    if (t.installed) launchTool(t);
    else openTool(t);
  };

  const startWizard = () => setWizard((w) => ({ runId: (w?.runId ?? 0) + 1, preselect: null }));
  // 一键全安装：进装机向导，预选 "all" —— Wizard 自动排队装全部工具 + 自动接虾盘云 + 弹 ClawX 下载
  const startInstallAll = () => {
    setTab("setup");
    setWizard((w) => ({ runId: (w?.runId ?? 0) + 1, preselect: "all" }));
  };

  // 一键接虾盘云：用设备内置 Key 把已装的 AI 工具接到虾盘云（国内直连）。
  // **动手前先弹勾选框**（0.9.84）：以前这里是无差别覆盖全部已装工具，客户没机会说
  // 「别碰我的 Codex」。现在先摊开改谁、从什么改成什么，默认全勾但他自己配过的默认不勾。
  const applyXiapan = useCallback(() => setApplyScope(true), []);

  /** 用户在勾选框里点了确认 —— 只配他勾的那几个。 */
  const doApplyXiapan = useCallback(async (targets: string[]) => {
    setApplyScope(false);
    // 一次把虾盘云内置 Key 写进选中的工具，并顺手释放 AIGC 技能包。只写 API 配置不等于会作图/视频：
    // ClawX/OpenClaw 需要 SKILL.md + scripts 落到 skills 目录，才能稳定调用 gen-image/gen-video。
    try {
      const r = await invoke<{ configured: string[]; skipped: string[]; clawx_needs_restart: boolean }>(
        "apply_xiapan_everywhere",
        { providerId: "xiapan", apiKey: null, model: null, targets },
      );
      let skillMsg = "";
      try {
        const info = await invoke<{ default_dir: string; installed: { tool: string; path: string }[] }>("install_skill_pack");
        const tools = info.installed.map((i) => i.tool);
        skillMsg = tools.length
          ? tr("；作图/视频技能包已装进 {tools}", { tools: tools.join(" / ") })
          : tr("；作图/视频技能包已导出到 {dir}", { dir: info.default_dir });
      } catch {
        skillMsg = tr("；作图/视频技能包可到「AI 技能包」页一键安装");
      }
      // 🔴 **跳过的必须报出来**（后端 `skipped` 里本来就带着原因，之前被整条丢掉了）：
      // 用户勾了 6 个、实际配上 4 个，只报那 4 个，读起来就是「全配好了」——
      // 而没配上的那两个恰恰是他等会儿要撞墙的地方（如 Codex 挂着自己的 ChatGPT 登录）。
      // 这跟「投影必须披露自己丢了什么」是同一条：**过滤了却不说，就是谎报**。
      const who = r.configured.length ? r.configured.join(" / ") : tr("AI 工具");
      const skipped = r.skipped?.length ? tr("；没配：{list}", { list: r.skipped.join("、") }) : "";
      flash(
        (r.configured.length
          ? tr("已接入虾盘云：{who} 现在国内直连", { who })
          : tr("一个都没配上 —— 详情见下")) +
          skillMsg +
          (r.clawx_needs_restart ? tr("，ClawX 请重启生效") : "") +
          skipped,
      );
      refresh();
    } catch (e) {
      flash(tr("接入失败：") + String(e));
    }
  }, [refresh, tr]);

  // 一键把「虾盘云（Claude + Codex）+ 你在用的工具配置」导入 uu-switch（写 ~/.cc-switch/config.json，
  // 非破坏式合并）。虾盘云用 U-King 设备 Key + 端点 + preset 默认模型（deepseek-v4-flash /
  // deepseek-v4-flash-codex），两侧切换等效 —— 模型名后端从 preset 读，别在这条注释里再写死一份。
  const importToUuswitch = useCallback(async () => {
    // 有库时后端要关 uu-switch→写库→重开，进度走事件（关闭/写入…）。
    const un = await listen<string>("uking:uuswitch_progress", (e) => flash(e.payload));
    try {
      flash(tr("正在导入到 uu-switch（虾盘云 + 你在用的配置）…"));
      const msg = await invoke<string>("import_to_uuswitch");
      flash(msg);
    } catch (e) {
      flash(tr("导入 uu-switch 失败：") + String(e));
    } finally {
      un();
    }
  }, [tr]);

  // ⚠️ 已彻底移除「打开即自动接虾盘云」逻辑（2026-06-17）。
  // 旧行为侵入性太强：U-King 一启动就把虾盘云写进 Claude Code / Codex / ClawX 的底层配置，
  // 把用户正在用的【原版官方直连】静默替换掉（has_driver 把「没设 BASE_URL = 官方直连」
  // 误判成「没配过驱动」，于是自动覆盖）。现改为**纯手动**：用户必须主动点
  // 「一键接入虾盘云」按钮（applyXiapan）才会写配置，绝不在启动时动用户现有配置。

  const hideToTray = () => invoke("hide_to_tray").catch(() => {});

  /** 矮屏（1366×768 客户区仅 ~688px）：收掉纯装饰性的纵向占用，把高度还给内容。
   *  见 lib/useViewport.ts 的阈值推导 —— 1080p@100% 不会命中，正常机器排版不变。 */
  const { short } = useViewport();

  // 用户在用自己的 AI（官方登录 / 自备 Key，非虾盘云）→ 前端不推虾盘云、不弹接管条、
  // 接入改成显式可还原入口。铁律（CLAUDE.md 第 10 条）：绝不抢、不挤占用户自己的 Key。
  const usingOwnKey = !!(driver?.claude_own_key || driver?.codex_own_key);

  const onStatusAction = () => {
    if (!setupState) return;
    if (setupState.next_step === "recharge") {
      void openRechargeAndWatch(deviceKey?.recharge_url);
      setTab("manage");
    } else if (setupState.next_step === "config_driver") {
      setTab("manage");
    } else if (setupState.next_step === "install_tool") {
      setTab("setup");
      startWizard();
    }
  };

  // 独立的「演示卸载工具」绿色版：同一个后端、单独的收敛界面。放在所有 hooks 之后，
  // 不会改变主程序的 hook 顺序；普通 U-King 永远不会走到这里。
  if (env?.demo_uninstaller) {
    return (
      <div className="h-full">
        <Suspense fallback={<PageFallback />}>
          <DemoUninstaller onToast={flash} />
        </Suspense>
        {toast && (
          <div className="fixed bottom-5 left-1/2 z-50 -translate-x-1/2 animate-fade-in">
            <div className="flex items-center gap-2 rounded-full border border-white/[0.10] bg-bg-3/95 px-4 py-2 text-[13px] text-ink-1 shadow-card backdrop-blur">
              <Sparkles size={14} className="text-accent" />
              {toast}
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      {/* ★ 矮屏不渲染这条自定义标题栏。
          `tauri.conf.json` 没设 `decorations` → 默认 true，**原生标题栏一直开着**
          （「U-King AI 管家」那条）。所以这 36px 是第二条标题栏，里面只有一个
          「缩到托盘」按钮 —— 在 1366×768 客户区仅 688px 的机器上，这是 5% 的高度
          换一个按钮。拖动窗口也不靠它（原生标题栏本来就能拖）。
          **按钮不是被删掉，是搬走了**：矮屏时它出现在侧栏页脚（`Sidebar` 的
          `onHideToTray`），侧栏在所有页面常驻，比这条更好找。 */}
      {!short && (
        <PanelBoundary name="titlebar" variant="chrome">
          <TitleBar onHide={hideToTray} />
        </PanelBoundary>
      )}
      {/* 🔴 「并行调试实例」常驻条。**不能做成 toast** —— 降权是整个会话都成立的事实，
          而一条 5 秒就消失的提示只能解释「刚才」。没有它，这个实例里定时任务不跑、
          技能包不同步、codex 代理不自愈、新建的任务和 AI 续接不落盘，从界面上跟
          「这些东西坏了」长得一模一样，排障的人会去查调度器、查配置、查上游，
          而真相只是「你开了两个」。
          只读那半边（任务 / 续接不保存）必须一起说 —— **丢状态不可怕，丢了还装作没丢才可怕**。
          用 warning 令牌而不是裸 amber-100~300：浅色主题是默认，那几档对比度只有
          1.24:1 = 根本看不见（`check-theme-tokens` 会拦）。这条是「提示」不是「会花钱」，
          所以 warning 不是 danger。 */}
      {isSidecar && (
        <div
          data-action-id="runtime.instance.inspect"
          className="shrink-0 border-b border-warning-500/40 bg-warning-500/15 px-3 py-1.5 text-[12.5px] text-warning-700 dark:text-warning-400"
        >
          {tr(
            "这是并行调试实例（第二个 U-King）—— 界面、终端、工作目录跟第一个完全一样，但定时任务、技能包同步、Codex 代理自愈都由第一个负责，这里不重复跑；这里新建的任务和对话续接不会保存。",
          )}
        </div>
      )}
      {changelogOpen && (
        <Changelog
          current={APP_VERSION}
          latest={update?.latest}
          notes={update?.notes}
          history={update?.history}
          onClose={() => setChangelogOpen(false)}
        />
      )}

      <div className="flex-1 flex min-h-0">
        {/* 🔴 侧栏必须单独包边界：它在每一页都常驻，而且渲染的是**动态数据**
            （已装小程序、dock、升级状态）—— 一个坏掉的小程序清单就能把整个界面白掉。
            边界在这里而不在 Sidebar 内部：卸载期抛的错只会冒给仍挂载着的上层。 */}
        <PanelBoundary name="sidebar" variant="chrome">
        <Sidebar
          active={tab}
          onSelect={setTab}
          version={APP_VERSION}
          onShowChangelog={() => setChangelogOpen(true)}
          platform={env?.platform}
          theme={theme}
          onToggleTheme={() => setTheme((m) => (m === "dark" ? "light" : "dark"))}
          // 官网 = 品牌域 u-king.org（2026-07 已从 Vercel 迁到自有服务器，与 u-claw.org.cn 同机，
                    // 根路径 serve 落地页）。⚠️ 境内裸网 SNI reset（2026-08 实测）→ 不直开品牌域，
                    // 先调 resolve_site_url 后端探测：u-claw.org.cn/uking/ 首选（境内 200），www.u-king.org 备选，
                    // 全挂 fallback 国内地址——点击必须能出来（多 AI 会审 2026-08-27 裁决 P0）。
                    onOpenSite={() => {
                      void (async () => {
                        let u = "https://u-claw.org.cn/uking/";
                        try { u = await invoke<string>("resolve_site_url"); } catch { /* 保持 fallback */ }
                        openUrl(u).catch(() => {});
                      })();
                    }}
          dockApps={dockApps}
          onLaunchDock={onLaunchDock}
          // 侧栏常驻升级入口：升级横幅只在管家页 StatusLine 露出，长期待在 U-Workspace 的用户
          // 看不到升级（客户实锤）。侧栏在所有页都在，保证随时点得到。不受 updateDismissed 影响
          //（那只收起大横幅；这个小入口有新版就一直在）。
          hasUpdate={!!update?.has_update}
          latestVersion={update?.latest}
          onUpdate={doSelfUpdate}
          updating={updating}
          updatePct={updatePct}
          // 自动升级在这台机器上失败过 → 侧栏按钮改推「下载安装包重装」（见 Sidebar 注释）
          updateFailed={update?.failed_attempts ?? 0}
          updateFailReason={update?.fail_reason}
          onReinstall={doReinstall}
          // 矮屏时自定义标题栏被撤（见上），「缩到托盘」搬到侧栏页脚 —— 传 null 表示这台
          // 机器不矮、标题栏还在，侧栏就别重复放一个（同一动作两个入口，迟早漂移）。
          onHideToTray={short ? hideToTray : undefined}
        />
        </PanelBoundary>

        {/* OpenCodex 工作台：现由 U-Workspace 取代（U-Workspace 复用同一套 store/SessionList）。
            OpenCodex tab 本就不可达，改成**仅 workbench 时才挂载**，避免两个 WorkbenchProvider
            共享同一 tasks.json 双写。要单独调试老 OpenCodex 时把 tab 切 workbench 即可。 */}
        {tab === "workbench" && (
          <main className={cn("flex-1 min-w-0 min-h-0", short ? "p-1.5" : "p-3")}>
            <PanelBoundary name="workbench">
              <Suspense fallback={<PageFallback />}>
                <OpenCodex openedDir={env?.opened_dir ?? null} homeDir={env?.home_dir ?? null} onToast={flash} onGoManage={() => setTab("manage")} />
              </Suspense>
            </PanelBoundary>
          </main>
        )}

        {/* U-Workspace（AI 工作台，opencodex 模块）：常驻渲染（display 切换保活，多会话/PTY/预览切走不丢，同 OpenCodex） */}
        <main
          className={cn("flex-1 min-w-0 min-h-0", short ? "p-1.5" : "p-3")}
          style={{ display: tab === "chat" ? undefined : "none" }}
        >
          {/* U-Workspace 是唯一 eager 挂载的页（保活），也是崩得最多的页 ——
              工作台里面还有 U-Chat / U-CLI / 文件 / 浏览器各自的边界（SplitArea.tsx），
              这一层兜的是工作台外壳自身（store / SessionList / 顶栏）。 */}
          <PanelBoundary name="U-Workspace">
            {/* onGoCreate：「AI 创作」2026-08-23 从工作台右侧面板搬回侧栏独立页（一个能力一个入口）。
                专家卡上「打开 AI 作图专家」那条 route 必须跟着改道到侧栏那一页，否则它又会变回
                一句不兑现的承诺 —— Chat.tsx 那段注释记着它以前就是死的。 */}
            <UWorkspace onToast={flash} pendingExpert={pendingExpert} onConsumed={() => setPendingExpert(null)} pendingChatPrompt={pendingChatPrompt} onConsumedChat={() => setPendingChatPrompt(null)} onInstallClaude={installClaude} onGoCreate={(sub) => setTab(sub === "video" ? "video" : "draw")} />
          </PanelBoundary>
        </main>

        {/* 所有 TUI 应用：访问过的常驻渲染，display 由 tab 控制（PTY 保活） */}
        {TUI_APPS.filter((a) => mountedTui.has(a.id)).map((a) => (
          <main
            key={a.id}
            className={cn("flex-1 min-w-0 min-h-0", short ? "p-1.5" : "p-3")}
            style={{ display: tab === a.id ? undefined : "none" }}
          >
            {/* 每个 TUI 应用一个边界：Hermes 的终端炸了不该带走 Claude 的那个 */}
            <PanelBoundary name={`tui:${a.id}`}>
              <Suspense fallback={<PageFallback />}>
                <ToolAppView
                  app={a}
                  active={tab === a.id}
                  deviceKey={deviceKey}
                  onToast={flash}
                  onGoManage={() => setTab("manage")}
                  onManageProviders={(editId) => setProviderMgr({ editId })}
                  onRefreshDriver={refresh}
                />
              </Suspense>
            </PanelBoundary>
          </main>
        ))}

        {/* 终端页：访问过即常驻渲染，display 由 tab 控制（PTY 保活，切走不杀终端） */}
        {termMounted && (
          <main
            className={cn("flex-1 min-w-0 min-h-0", short ? "p-1.5" : "p-3")}
            style={{ display: tab === "terminal" ? undefined : "none" }}
          >
            <PanelBoundary name="terminal">
              <Suspense fallback={<PageFallback />}>
                <TerminalPage
                  active={tab === "terminal"}
                  pendingCmd={pendingCmd}
                  onConsumedCmd={() => setPendingCmd(null)}
                  pendingRestores={pendingTermRestores}
                  onRestoreFailed={(failed) => {
                    setRecoveringTermSnapshot(false);
                    setPendingTermRestores(null);
                    setFailedTermRestores(failed);
                    flash(tr("有 {n} 个终端重开失败，快照已保留，可重试", { n: failed.length }));
                  }}
                  onConsumedRestores={() => {
                    setPendingTermRestores(null);
                    setFailedTermRestores(null);
                    setRecoveringTermSnapshot(false);
                    setTermSnapshot(null);
                    void invoke("term_snapshot_consume").catch(() => {});
                  }}
                />
              </Suspense>
            </PanelBoundary>
          </main>
        )}

        {/* 非 TUI 页面（manage/codex/myai/setup）：条件渲染，无 PTY 不需保活 */}
        {tab !== "workbench" && tab !== "terminal" && tab !== "chat" && !isTuiAppId(tab) && (
        <main className={cn(
          "flex-1 min-w-0",
          selfHeightTab ? "min-h-0 flex flex-col" : "overflow-y-auto",
          selfHeightTab ? (short ? "p-1.5" : "p-3") : (short ? "px-4 py-3" : "px-6 py-6"),
        )}>
          {/* 主体宽度 max-w-4xl(896px) → max-w-5xl(1024px)（测试报告 #012：「大块留白」）。
              1920 宽屏上 896px 的正文两侧各空 400 多像素，而侧栏才 208px ——
              客户看到的就是「导航占一条、内容挤中间、其余全是空」。
              不敢一步放到 6xl：这些页面的卡片是按窄栏排的，太宽会让每行字长到读不下去。 */}
          <div className={selfHeightTab ? "flex-1 min-h-0 flex flex-col" : "max-w-5xl mx-auto space-y-6"}>
            {/* 🔴 状态条也在边界外过：它渲染升级状态 / 装机引导 / 充值提醒，全是后端数据驱动，
                而它在每一页顶部常驻 —— 崩一次就是整屏。它跟下面的页边界必须分开：
                合在一起的话，状态条炸会把当前页一起吃掉，等于半径没压。 */}
            <PanelBoundary name="statusline" variant="chrome">
            <StatusLine
              // 工具中心(myai)是「日常用」页，动态/学院是「浏览」页 —— 都不催装机，隐藏装机引导横幅
              //（升级横幅仍保留）。装机引导只在装机向导(setup)/Codex专区(codex)/AI设置(manage)露出。
              // 例外：「该充值了」是开始使用前的最后一步 —— 一键安装完即落 myai，必须在这里也提醒，
              //（否则装完落地页吞掉充值入口，客户反馈「提醒不够」）。
              setupState={
                tab === "dshplugins" || tab === "toolbox" || tab === "localllm" || tab === "rtk" || tab === "backup" || tab === "advanced" || tab === "feedback" || tab === "xiapan" || tab === "skills" || tab === "experts" || tab === "identity" || tab === "create" || tab === "nightshift" || tab === "teamspace" || tab === "runcenter"
                  ? null
                  : tab === "myai"
                    ? setupState?.next_step === "recharge" || setupState?.clawx_needs_xiapan
                      ? setupState
                      : null
                    : setupState
              }
              // 升级入口已收敛到「侧栏底部（深浅色切换旁）」那一个常驻按钮（见 Sidebar）——
              // 它在所有页都在，包含第一页和 U-Workspace，一处即可、不再到处弹横幅（用户要求「别那么多地方显示」）。
              // 所以这里不再给 StatusLine 升级横幅（StatusLine 只剩装机引导/右键目录）。
              update={null}
              usingOwnKey={usingOwnKey}
              openedDir={env?.opened_dir ?? null}
              onAction={onStatusAction}
              onUpdate={doSelfUpdate}
              updating={updating}
              updatePct={updatePct}
              onDismissUpdate={() => {
                if (update) localStorage.setItem("uking.updateDismissedVersion", update.latest);
              }}
              onConnectClawx={() => {
                setClawxDismissed(true); // 点了就收起横幅；applyXiapan 会自探把 ClawX 一起接上
                void applyXiapan();
              }}
              onDismissClawx={() => setClawxDismissed(true)}
              clawxDismissed={clawxDismissed}
              // 全流程走完（装了工具+配了驱动+已充值）才推荐装作图能力；正在用作图/视频/技能包/浏览页时不弹，
              // 不打断当下的事。升级/装机/驱动/充值/ClawX 横幅优先级更高（StatusLine 里靠 next_step=done 天然让位）。
              aigcNudge={
                setupState?.next_step === "done" &&
                !aigcDismissed &&
                !["skillpack", "create", "draw", "qrmerge", "video", "reel", "tasks", "geo", "toolbox", "rtk", "backup", "advanced"].includes(tab)
              }
              onGoAigc={() => {
                localStorage.setItem("uking.aigcNudgeDone", "1");
                setAigcDismissed(true);
                setTab("skillpack");
              }}
              onDismissAigc={() => {
                localStorage.setItem("uking.aigcNudgeDone", "1");
                setAigcDismissed(true);
              }}
            />
            </PanelBoundary>

            {/* 普通页共用这一条 else-chain。边界带 key={tab}：每个页各拿一个干净边界，
                某页崩了切走再回来自动重置，不用重启整个 U-King。 */}
            <PanelBoundary key={tab} name={tab}>
            <Suspense fallback={<PageFallback />}>
            {tab === "teamspace" ? (
              <TeamSpace />
            ) : tab === "runcenter" ? (
              <RunCenter />
            ) : tab === "manage" ? (
              <Manager
                onGoCodex={() => setTab("codex")}
                onGoAdvanced={() => setTab("advanced")}
                onGoPage={(tb) => setTab(tb as TabId)}
                onDeviceKeyChange={setDeviceKey}
                onRecharge={(url) => openRechargeAndWatch(url)}
                onSelfUpdate={doSelfUpdate}
                onAskAI={(prompt) => { setPendingChatPrompt({ prompt, engine: "uking", passportId: "ai-settings-repair" }); setTab("chat"); }}
                // 「左装右选 → 合并启动」用的三件（2026-08-20）。**全是复用现成的**：
                // 装机走 `openTool`（跟「我的 AI」同一条），启动走 `launchTool`
                // （CLI 落进 U-CLI 会话 / GUI 应用外部弹出，也是同一条）。
                // Manager 自己不实现装和启动 —— 那两件在 App 这层已经各有唯一一份。
                tools={tools}
                onInstallTool={(target) => {
                  const t = tools.find((x) => x.id === MANAGER_TARGET_TOOL_ID[target]);
                  if (t) openTool(t);
                }}
                onLaunchTool={(target) => {
                  const t = tools.find((x) => x.id === MANAGER_TARGET_TOOL_ID[target]);
                  if (t) launchTool(t);
                }}
              />
            ) : tab === "create" ? (
              <Create deviceKey={deviceKey} onToast={flash} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} onGoSkillPack={() => setTab("skillpack")} />
            ) : tab === "draw" ? (
              <Draw deviceKey={deviceKey} onToast={flash} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} onGoSkillPack={() => setTab("skillpack")} />
            ) : tab === "qrmerge" ? (
              <QrMerge deviceKey={deviceKey} onToast={flash} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} />
            ) : tab === "video" ? (
              <Video deviceKey={deviceKey} onToast={flash} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} onGoSkillPack={() => setTab("skillpack")} />
            ) : tab === "reel" ? (
              <Reel deviceKey={deviceKey} onToast={flash} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} />
            ) : tab === "tasks" ? (
              <MediaTasks onGo={(next) => setTab(next)} />
            ) : tab === "geo" ? (
              <Geo onToast={flash} />
            ) : tab === "skillpack" ? (
              // 技能包页：一键装自带能力(作图/视频/协同) + 图文上手教程。
              //
              // 🔴 1.0.3 删掉了原来夹在中间的「技能市场」那一段（`Skills.tsx` + `Feed.tsx`）——
              // 用户 2026-08-18：「ai技能 删除吧，就是一个 skillhub，ai专家，不就是吗？合并留到 uchat」。
              // skillhub 入口和**逐包装/删清单**都搬到了 U-Workspace 左栏的「AI 专家」那一屏
              // （专家是人、技能包是这些人会的本事，摆一起才说得通）。
              // `Tutorial` 没跟着删：它是给完全不懂的小白看的图文上手，`Skills.tsx` 是它**唯一**的挂载点，
              // 一起删就成了静默移除新手引导 —— 所以直接挂在这里。
              <div className="space-y-6">
                <SkillPack deviceKey={deviceKey} onToast={flash} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} />
                <div className="border-t border-white/[0.06]" />
                <Tutorial onGoMyAI={() => setTab("myai")} />
              </div>
            ) : tab === "dshplugins" ? (
              <DshPlugins
                onToast={flash}
                onGoInstall={() => setTab("setup")}
                // 「打开 DSH」跟「我的 AI」点 DSH 卡片走同一条路（launchTool 里 t.id === "dsh"
                // 那支）。插件页不许自己起 `dsh web` —— 见 DshPlugins.tsx::openWeb 的注释。
                onGoDsh={() => setTab("dsh")}
                onGoChat={(prompt) => {
                  setPendingChatPrompt({ prompt, engine: "claude" });
                  setTab("chat");
                }}
              />
            ) : tab === "toolbox" ? (
              <Toolbox onToast={flash} />
            ) : tab === "airuntime" ? (
              <AiRuntime onToast={flash} onGoSetup={() => setTab("setup")} onAskAI={(prompt) => { setPendingChatPrompt({ prompt, engine: "uking", passportId: "airuntime-doctor" }); setTab("chat"); }} />
            ) : tab === "rtk" ? (
              <TokenSqueezer onToast={flash} />
            ) : tab === "meter" ? (
              <Meter onToast={flash} onGoto={setTab} />
            ) : tab === "nightshift" ? (
              <NightShift onToast={flash} />
            ) : tab === "localllm" ? (
              <LocalLLM onToast={flash} />
            ) : tab === "backup" ? (
              <Backup onToast={flash} />
            ) : tab === "advanced" ? (
              <Advanced
                deviceKey={deviceKey}
                onToast={flash}
                onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)}
                onDeviceKeyChange={setDeviceKey}
              />
            ) : tab === "feedback" ? (
              <Feedback version={APP_VERSION} onToast={flash} />
            ) : tab === "xiapan" ? (
              <Guide
                deviceKey={deviceKey}
                onToast={flash}
                onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)}
                onRefreshBalance={() => refreshDeviceKey(false)}
                onDeviceKeyChange={setDeviceKey}
                onApplyXiapan={applyXiapan}
                onUseOwnKey={() => setTab("manage")}
              />
            ) : tab === "identity" ? (
              <Identity onToast={flash} />
            ) : tab === "codex" ? (
              <CodexZone
                tools={tools}
                driver={driver}
                onOpen={openTool}
                onLaunch={launchTool}
                onGoManage={() => setTab("manage")}
                onToast={flash}
              />
            ) : tab === "myai" ? (
              <MyAI
                driver={driver}
                tools={tools}
                deviceKey={deviceKey}
                setupState={setupState}
                usingOwnKey={usingOwnKey}
                onLaunch={launchTool}
                onOpen={openTool}
                onUninstall={uninstallTool}
                onGoInstall={() => setTab("setup")}
                onInstallAll={startInstallAll}
                onGoManage={() => setTab("manage")}
                onApplyXiapan={applyXiapan}
                onImportXiapan={importToUuswitch}
                onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)}
                onManageProviders={(editId) => setProviderMgr({ editId })}
                onRefreshDriver={refresh}
                termSnapshot={termSnapshot}
                recoveringTermSnapshot={recoveringTermSnapshot}
                failedTermRestoreCount={failedTermRestores?.length ?? 0}
                onRestoreTermSnapshot={() => {
                  if (!termSnapshot) return;
                  setRecoveringTermSnapshot(true);
                  setPendingTermRestores(() => failedTermRestores?.length
                    ? failedTermRestores.map((session) => ({ ...session }))
                    : termSnapshot.sessions.map((session, restoreKey) => ({ ...session, restoreKey })));
                  setTab("terminal");
                }}
                onDismissTermSnapshot={() => {
                  void invoke("term_snapshot_consume").catch(() => {});
                  setPendingTermRestores(null);
                  setFailedTermRestores(null);
                  setTermSnapshot(null);
                }}
              />
            ) : (
              <>
                <DriverBar driver={driver} deviceKey={deviceKey} onStart={startWizard} onInstallAll={startInstallAll} onRecharge={() => openRechargeAndWatch(deviceKey?.recharge_url)} />
                {wizard && (
                  <Wizard
                    key={wizard.runId}
                    preselect={wizard.preselect}
                    onFinished={refresh}
                    onGoWorkspace={() => setTab("chat")}
                  />
                )}
                <ToolMarket tools={tools} onOpen={openTool} onLaunch={launchTool} onImportXiapan={importToUuswitch} />
              </>
            )}
            </Suspense>
            </PanelBoundary>
          </div>
        </main>
        )}
      </div>

      {/* 小程序开在独立窗口里（open_miniapp），不是浮层 ——
          主窗口在 http 源上，iframe 指向 uking:// 属跨 scheme，WebView2 不放行。 */}

      {providerMgr && (
        <ProviderManager
          editId={providerMgr.editId}
          onToast={flash}
          onClose={() => setProviderMgr(null)}
          onChanged={refresh}
        />
      )}

      {/* ClawX 首次启动：允许访问网络 引导浮层 */}
      {clawxNetHint && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in">
          <div className="w-[440px] max-w-[92vw] rounded-card border border-accent/30 bg-bg-2 shadow-card overflow-hidden">
            <div className="flex items-center gap-2.5 px-5 pt-5">
              <div className="flex h-9 w-9 items-center justify-center rounded-full bg-accent/15">
                <ShieldCheck size={18} className="text-accent" />
              </div>
              <h3 className="text-[15px] font-semibold text-ink-0">{tr("马上打开 ClawX · 请放行网络")}</h3>
            </div>
            <div className="px-5 py-4 space-y-3">
              <p className="text-[13px] leading-relaxed text-ink-2">
                {tr("ClawX 第一次打开时，Windows 可能弹出一个")}
                <span className="text-ink-0 font-medium">{tr("「Windows 安全中心警报 / 是否允许访问」")}</span>
                {tr("的窗口。")}
              </p>
              <div className="rounded-lg border border-accent/25 bg-accent/[0.07] px-4 py-3">
                <p className="text-[13px] leading-relaxed text-ink-1">
                  {tr("请一定点")}
                  <span className="mx-1 inline-flex items-center rounded bg-accent px-2 py-0.5 text-[12px] font-semibold text-white">
                    {tr("允许访问")}
                  </span>
                  {tr("，并把「专用网络 / 公用网络」都勾上。")}
                </p>
                <p className="mt-1.5 text-[12px] text-ink-3">
                  {tr("如果点了「取消」，ClawX 连不上 AI，会一直转圈或报错。")}
                </p>
              </div>
              <p className="text-[11.5px] text-ink-4">
                {tr("（U-King 已尽量帮你提前放行，这条提示只出现一次。）")}
              </p>
            </div>
            <div className="flex items-center justify-end gap-2 px-5 pb-5">
              <button
                onClick={() => setClawxNetHint(null)}
                className="px-3.5 h-9 rounded-lg border border-white/[0.10] text-[13px] text-ink-3 hover:text-ink-1"
              >
                {tr("取消")}
              </button>
              <button
                onClick={() => clawxNetHint()}
                className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600"
              >
                {tr("知道了，打开 ClawX")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ClawX 下载/安装进度条（常驻，比 toast 显眼，210MB 大包不让人以为卡死） */}
      {clawxProgress && (
        <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-[55] w-[360px] max-w-[92vw] animate-fade-in">
          <div className="rounded-xl border border-accent/30 bg-bg-3/95 backdrop-blur px-4 py-3 shadow-card">
            <div className="flex items-center gap-2 text-[12.5px] text-ink-1">
              <Download size={14} className="text-accent shrink-0 animate-pulse" />
              <span className="truncate">{tr(clawxProgress)}</span>
            </div>
            <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-white/[0.08]">
              <div
                className="h-full rounded-full bg-accent transition-all duration-500"
                style={{ width: `${clawxPct(clawxProgress)}%` }}
              />
            </div>
          </div>
        </div>
      )}

      {toast && (
        <div className="fixed bottom-5 left-1/2 -translate-x-1/2 z-50 animate-fade-in">
          <div className="flex items-center gap-2 rounded-full border border-white/[0.10] bg-bg-3/95 backdrop-blur px-4 py-2 text-[13px] text-ink-1 shadow-card">
            <Sparkles size={14} className="text-accent" />
            {toast}
          </div>
        </div>
      )}

      {/* 「一键配好全部 AI」动手前的知情与否决 —— 改谁、从什么改成什么、能不改。 */}
      {applyScope && (
        <ApplyScopeDialog
          driver={driver}
          onCancel={() => setApplyScope(false)}
          onConfirm={(targets) => void doApplyXiapan(targets)}
        />
      )}
    </div>
  );
}

/** 从 ClawX 进度文案里估算进度条百分比（解析「约 N%」，否则按阶段给估值）。 */
function clawxPct(msg: string): number {
  const m = msg.match(/约\s*(\d+)\s*%/);
  if (m) return Math.min(99, Math.max(3, parseInt(m[1], 10)));
  if (msg.includes("放行") || msg.includes("接入")) return 98;
  if (msg.includes("安装")) return 95;
  if (msg.includes("下载完成")) return 90;
  return 5; // 开始下载
}

/* ---------------- 状态行（合并三横幅，最多显示一条）---------------- */

type SetupState = { has_tool: boolean; has_driver: boolean; charged: boolean; clawx_needs_xiapan: boolean; next_step: string; hint: string };
type UpdateInfo = { current: string; latest: string; has_update: boolean; notes: string; download_url: string };

function StatusLine({
  setupState,
  update,
  usingOwnKey,
  updating,
  updatePct,
  openedDir,
  onAction,
  onUpdate,
  onDismissUpdate,
  onConnectClawx,
  onDismissClawx,
  clawxDismissed,
  aigcNudge,
  onGoAigc,
  onDismissAigc,
}: {
  setupState: SetupState | null;
  update: UpdateInfo | null;
  /** 用户在用自己的 Key → 不弹「去配驱动 / 接入虾盘云」这类推虾盘云的引导条（铁律：不抢用户 Key）。 */
  usingOwnKey?: boolean;
  updating: boolean;
  updatePct: number | null;
  openedDir: string | null;
  onAction: () => void;
  onUpdate: () => void;
  onDismissUpdate: () => void;
  onConnectClawx: () => void;
  onDismissClawx: () => void;
  clawxDismissed: boolean;
  /** 已装好工具+驱动+充值（next_step=done）后，主动推荐「给 AI 装作图/视频能力」。治「很多人不知道有技能包按钮」。 */
  aigcNudge?: boolean;
  onGoAigc: () => void;
  onDismissAigc: () => void;
}) {
  const { t } = useI18n();
  // 优先级：有新版（最重要，升级即修 bug）> 装机引导 > 右键打开的目录。
  // 注意：升级必须排在装机引导前 —— 否则处于「装了没配驱动/没充值」半成品态的客户
  // 永远被装机引导挡住、看不到升级提示（v0.8.3 实锤的真 bug，全量客户受影响）。
  if (update) {
    // 刻意做得比其它状态条更轻：细一圈、素色边框、小字号——「有新版」是个可选项，
    // 不该长得比「没配驱动连不上」还醒目。不升级也要能完全正常用。
    return (
      <div className="flex items-center gap-2.5 rounded-card border border-white/[0.08] bg-white/[0.02] px-3.5 py-1.5 text-[12px] animate-fade-in">
        <span className="flex-1 text-ink-3">
          {t("新版本")} <span className="text-ink-1 font-medium">v{update.latest}</span> {t("可用")}
          <span className="text-ink-4"> · {t("当前")} v{update.current}</span>
        </span>
        <button
          onClick={onUpdate}
          disabled={updating}
          className="px-2.5 h-6 rounded-md border border-accent/30 text-accent text-[11px] font-medium hover:bg-accent/[0.08] shrink-0 disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {updating ? (updatePct != null ? t("升级中 {pct}%", { pct: updatePct }) : t("升级中…")) : t("一键升级")}
        </button>
        {!updating && (
          <button onClick={onDismissUpdate} className="text-ink-4 hover:text-ink-2 text-[11px] shrink-0">
            {t("稍后")}
          </button>
        )}
      </div>
    );
  }

  // 用户在用自己的 Key 时，不弹「去配驱动」引导 —— 那步会把他们推去接虾盘云（覆盖自备 Key）。
  // 充值提醒仍保留：只有真配了虾盘云却没充值才会到 recharge 态，不误伤自备 Key 用户。
  const guiding =
    setupState &&
    setupState.next_step !== "done" &&
    setupState.next_step !== "none" &&
    !(usingOwnKey && setupState.next_step === "config_driver");
  if (guiding) {
    const urgent = setupState!.next_step === "config_driver";
    // 充值是「开始使用前最后一步」—— 用红色预警，不再像普通装机提示那样低调。
    const recharge = setupState!.next_step === "recharge";
    const actionLabel =
      setupState!.next_step === "install_tool" ? t("开始装机") : setupState!.next_step === "config_driver" ? t("去配驱动") : t("立即充值");
    return (
      <div
        className={cn(
          "flex items-center gap-3 rounded-card border px-4 py-2.5 text-[13px] animate-fade-in",
          recharge ? "border-red-400/30 bg-red-500/[0.10]" : "border-white/[0.06] bg-bg-2",
        )}
      >
        <span className={cn("dot", recharge || urgent ? "dot-warn" : "dot-on")} />
        <span className={cn("flex-1", recharge ? "text-ink-0" : "text-ink-1")}>{setupState!.hint}</span>
        <button
          onClick={onAction}
          className={cn(
            "px-3 h-7 rounded-md text-white text-[12px] font-medium shrink-0",
            recharge ? "bg-red-500 hover:bg-red-600" : "bg-accent hover:bg-accent-600",
          )}
        >
          {actionLabel}
        </button>
      </div>
    );
  }

  // ClawX 已装但没接虾盘云 —— 非侵入提示，引导用户「自己点」接入，绝不后台静默写。
  // 排在装机引导之后（那个更紧急：没配驱动 claude 直接连不上）；用户点「不用了」即本次隐藏。
  if (setupState?.clawx_needs_xiapan && !clawxDismissed && !usingOwnKey) {
    return (
      <div className="flex items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-4 py-2.5 text-[13px] animate-fade-in">
        <span className="dot dot-on" />
        <span className="flex-1 text-ink-1">
          {t("检测到已装")} <b className="text-ink-0">ClawX</b>{t("，还没接入虾盘云驱动 —— 接入后国内直连、用内置 Key，无需自己填")}
        </span>
        <button
          onClick={onConnectClawx}
          className="px-3 h-7 rounded-md bg-accent text-white text-[12px] font-medium hover:bg-accent-600 shrink-0"
        >
          {t("接入虾盘云")}
        </button>
        <button onClick={onDismissClawx} className="text-ink-4 hover:text-ink-2 text-[11px] shrink-0">
          {t("不用了")}
        </button>
      </div>
    );
  }

  // 全部就绪后：主动推荐「给 AI 装作图/视频能力」—— 技能包按钮埋在「更多」里没人点，
  // 这里在全流程走完时露一条，一键直达。排在装机/驱动/充值/ClawX 之后（那些更紧急），
  // 只在 next_step=done 时由 App 计算传入，故不会与引导横幅打架。用户点「去装」或「不用了」都不再弹。
  if (aigcNudge) {
    return (
      <div className="flex items-center gap-3 rounded-card border border-accent/25 bg-accent/[0.06] px-4 py-2.5 text-[13px] animate-fade-in">
        <Sparkles size={15} className="shrink-0 text-accent" />
        <span className="flex-1 text-ink-1">
          {t("让你的 AI 会「画图 / 做视频」—— 一键把")} <b className="text-ink-0">{t("AI 作图能力")}</b>{t("装给 Claude / ClawX，装完直接说「帮我画张图」")}
        </span>
        <button
          onClick={onGoAigc}
          className="px-3 h-7 rounded-md bg-accent text-white text-[12px] font-medium hover:bg-accent-600 shrink-0"
        >
          {t("去装作图能力")}
        </button>
        <button onClick={onDismissAigc} className="text-ink-4 hover:text-ink-2 text-[11px] shrink-0">
          {t("不用了")}
        </button>
      </div>
    );
  }

  if (openedDir) {
    return (
      <div className="flex items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-4 py-2.5 text-[13px] animate-fade-in">
        <FolderTree size={15} className="shrink-0 text-ink-3" />
        <span className="flex-1 text-ink-2">
          {t("从右键菜单打开：")}<span className="font-mono text-ink-1">{openedDir}</span>
        </span>
      </div>
    );
  }

  return null;
}

/* ---------------- 自定义标题栏 ---------------- */

function TitleBar({ onHide }: { onHide: () => void }) {
  const { t } = useI18n();
  return (
    <header
      data-tauri-drag-region
      className="h-9 shrink-0 flex items-center justify-end px-3 border-b border-white/[0.06] bg-bg-0"
    >
      <button
        onClick={onHide}
        title={t("隐藏到右下角托盘")}
        className="pointer-events-auto inline-flex items-center gap-1.5 px-2.5 h-6 rounded text-ink-3 text-[11px] hover:bg-white/[0.04] hover:text-ink-1 transition-colors"
      >
        <PanelTopClose size={13} />
        {t("缩到托盘")}
      </button>
    </header>
  );
}

/* ---------------- AI 装机 + 驱动状态条 ---------------- */

function DriverBar({
  driver,
  deviceKey,
  onStart,
  onInstallAll,
  onRecharge,
}: {
  driver: DriverStatus | null;
  deviceKey: DeviceKey | null;
  onStart: () => void;
  onInstallAll: () => void;
  onRecharge: () => void;
}) {
  const { t } = useI18n();
  const driverLabel = (() => {
    const b = driver?.claude_base ?? "";
    if (b.includes("u-claw.org")) return t("虾盘云（内置）");
    if (b.includes("deepseek")) return "DeepSeek";
    if (b.includes("bigmodel")) return t("智谱 GLM");
    if (b.includes("moonshot")) return "Kimi";
    if (b) return b.replace(/^https?:\/\//, "");
    return null;
  })();
  // 判据在后端（device.rs，带实测依据：客户实际被挡那次预扣要 ¥0.358）。
  // 这里原来是一个裸的 `cny < 0.5`，Manager.tsx 里还有一份一模一样的 —— 改门槛会漂两份。
  const lowBalance = !!deviceKey?.low_balance;

  return (
    <section className="flex flex-wrap items-center gap-4 rounded-card border border-white/[0.10] bg-bg-2 px-5 py-4 shadow-card animate-fade-in">
      <span className="grid place-items-center w-10 h-10 rounded-lg bg-accent text-white shrink-0">
        <Wand2 size={18} />
      </span>
      <div className="flex-1 min-w-[220px]">
        <h2 className="text-[15px] font-semibold text-ink-0">{t("AI 装机 · 软件免费，用 AI 才充值")}</h2>
        <p className="text-[12px] text-ink-3 mt-1">
          {t("第一次用点「一键全安装」最省事，工具全部免费，真正用 AI 时才消耗余额。")}
        </p>
        {/* 装不上的兜底入口：放在装机向导顶部（客户反复装、装失败都在这页），点开通用手动安装
            与排错教程（清代理 / 装 Node / 换源 / 放开脚本策略 + 各工具逐条命令）。 */}
        <button
          onClick={() => invoke("open_install_help").catch(() => {})}
          className="inline-flex items-center gap-1 mt-2 text-[12px] text-accent hover:text-accent-400 transition-colors"
        >
          <LifeBuoy size={12} />
          {t("某个工具装不上？看教程")}
        </button>
      </div>
      <div className="flex items-center gap-3">
        <div className="text-right text-[11px] leading-tight">
          <div className="flex items-center gap-1.5 justify-end text-ink-2">
            <Cpu size={11} className="text-accent" />
            {t("当前驱动")}
          </div>
          <div className="font-mono text-[11px] mt-0.5">
            {driverLabel ? (
              <span className="text-success-400">{driverLabel}{driver?.claude_model ? ` · ${driver.claude_model}` : ""}</span>
            ) : (
              <span className="text-ink-4">{t("官方默认 / 未配置")}</span>
            )}
          </div>
          <div className="font-mono text-[10px] mt-0.5">
            {deviceKey ? (
              lowBalance ? (
                <span className="text-red-300">{t("余额偏低，Codex 可能不够一次请求")}</span>
              ) : deviceKey.charged ? (
                <span className="text-accent">{t("虾盘云已开通")}</span>
              ) : (
                <span className="text-red-300">{t("余额不足，请充值")}</span>
              )
            ) : (
              <span className="text-ink-4">{t("内置 Key 检测中…")}</span>
            )}
          </div>
        </div>
        {/* 充值按钮恒显示，不绑 deviceKey —— 它一旦慢/失败也不能挡住充值（虾盘云核心便捷点）。
            有 deviceKey 用带 key 的 URL；没有就退通用充值页（openRecharge 内部兜底）。 */}
        <button
          onClick={onRecharge}
          className="inline-flex items-center gap-1.5 px-3.5 h-10 rounded-lg border border-white/[0.10] text-accent text-[12px] font-medium hover:bg-white/[0.04] transition-colors"
          title={t("充值开通虾盘云")}
        >
          {deviceKey?.charged ? t("补充余额") : t("充值开通")}
        </button>
        <button
          onClick={onInstallAll}
          className="inline-flex items-center gap-2 px-5 h-10 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 transition-shadow"
        >
          <Download size={14} />
          {t("一键全安装")}
        </button>
        <button
          onClick={onStart}
          className="inline-flex items-center gap-2 px-4 h-10 rounded-lg border border-white/[0.10] text-ink-1 text-[12px] font-medium hover:bg-white/[0.04] transition-colors"
        >
          <Sparkles size={13} />
          {t("逐个选装")}
        </button>
      </div>
    </section>
  );
}


/* ---------------- 虾盘云首次引导卡（领Key → 接驱动 → 充值，配好即消失）---------------- */

function XiapanGuide({
  setupState,
  deviceKey,
  hasTool,
  usingOwnKey,
  onApplyXiapan,
  onRecharge,
  onGoInstall,
}: {
  setupState: SetupState | null;
  deviceKey: DeviceKey | null;
  hasTool: boolean;
  usingOwnKey?: boolean;
  onApplyXiapan: () => void;
  onRecharge: () => void;
  onGoInstall: () => void;
}) {
  const { t } = useI18n();
  // 用户在用自己的 AI（官方登录 / 自备 Key）→ **什么都不显示**。
  //
  // 2026-08-21 删掉了原来那张「检测到你自己的 AI 配置」绿卡（用户：「这个我感觉应该也是不要的」）。
  // 它说的是实话，但那是一句**不需要说的实话** —— 客户自己配好了 AI 正在用，
  // 我们跳出来汇报「我发现了，我不会动它」，对他没有任何新信息，只是占掉首屏一整块。
  //
  // 🔴 但**必须在这里就 return，不能让它掉进下面的三步引导**。那条引导第二步会推
  // 「一键配好 → 把这台电脑的专属 Key 写进已装工具」，对一个自备 Key 的客户就是
  // 劝他把自己的配置换掉 —— 删一张卡片顺手把红线（绝不抢用户 Key，宪法第 10 条）
  // 也删了。想改用虾盘云的入口在「AI 设置」里，那是他主动去点的地方。
  if (usingOwnKey) return null;
  const hasDriver = !!setupState?.has_driver;
  const charged = !!setupState?.charged || !!deviceKey?.charged;
  // 全就绪（装了 + 接了驱动 + 充了）→ 不显示
  if (hasTool && hasDriver && charged) return null;

  const steps = [
    // 文案跟着主推走：上面首屏推的是 Claude Code + Hermes，这里还写 ClawX / Codex 就是
    // 同一个页面上两处互相打架的说法（真机截图查出来的，类型和构建都不会报）。
    { done: hasTool, label: t("免费装工具"), desc: t("Claude Code + Hermes，一键装到电脑"), action: hasTool ? null : { text: t("去装机"), fn: onGoInstall } },
    { done: hasDriver, label: t("自动配模型"), desc: t("把这台电脑的专属 Key 写进已装工具"), action: hasDriver || !hasTool ? null : { text: t("一键配好"), fn: onApplyXiapan } },
    { done: charged, label: t("充值开通 AI"), desc: deviceKey ? t("¥20起充 · 到账后点刷新余额确认 · 不用不扣") : t("充值后即可聊天、写代码、画图"), action: charged || !hasDriver ? null : { text: t("去充值"), fn: onRecharge } },
  ];
  const nextIdx = steps.findIndex((s) => !s.done);
  const next = nextIdx >= 0 ? steps[nextIdx] : null;

  return (
    <section className="rounded-card border border-accent/30 bg-gradient-to-br from-accent/[0.12] to-transparent px-5 py-5 shadow-card">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2.5">
            <span className="grid place-items-center w-9 h-9 rounded-xl bg-accent/[0.18] shrink-0">
              <PlugZap size={18} className="text-accent" />
            </span>
            <h2 className="text-[15px] font-semibold text-ink-0">{t("开始用 AI 还差几步")}</h2>
          </div>
          <p className="mt-1.5 text-[12px] text-ink-3">
            {t("装工具、配模型全免费；充值只用于调用 AI，余额永久有效、不用不扣。")}
          </p>
        </div>
        {next?.action && (
          <button
            onClick={next.action.fn}
            className="inline-flex h-10 items-center justify-center rounded-xl bg-accent px-5 text-[12.5px] font-semibold text-white hover:bg-accent-600 shadow-sm transition-colors"
          >
            {next.action.text}
          </button>
        )}
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {steps.map((s, i) => (
          <div
            key={i}
            className={cn(
              "rounded-xl border px-4 py-3.5 flex flex-col gap-2",
              s.done
                ? "border-success-500/25 bg-success-500/[0.06]"
                : i === nextIdx
                ? "border-accent/40 bg-accent/[0.08]"
                : "border-white/[0.06] bg-white/[0.02] opacity-70",
            )}
          >
            <div className="flex items-center gap-2.5">
              <span
                className={cn(
                  "grid place-items-center w-6 h-6 rounded-full text-[11px] font-bold shrink-0",
                  s.done ? "bg-success-500 text-white" : i === nextIdx ? "bg-accent text-white" : "bg-accent/20 text-accent-400",
                )}
              >
                {s.done ? <CheckCircle2 size={13} /> : i + 1}
              </span>
              <span className="text-[13px] font-semibold text-ink-0">{s.label}</span>
            </div>
            <div className="text-[11px] text-ink-3 leading-snug min-h-[28px]">{s.desc}</div>
            {s.action ? (
              <button
                onClick={s.action.fn}
                className="mt-auto inline-flex items-center justify-center h-9 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 transition-colors"
              >
                {s.action.text}
              </button>
            ) : (
              <div className="mt-auto h-9 inline-flex items-center text-[11px] text-ink-4">
                {s.done ? t("已完成") : t("上一步完成后解锁")}
              </div>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}

/* ---------------- 我的 AI（日常态：打开已装工具）---------------- */

/** 工具 id → apply_provider target（决定切驱动写哪个工具的底层配置）。 */
function toolTargets(id: string): string[] {
  switch (id) {
    case "claude-code":
      return ["claude"];
    case "codex":
    case "codex-app": // Codex 桌面版与 CLI 共用 ~/.codex/config.toml
      return ["codex"];
    case "openclaw":
    case "clawx": // OpenClaw 的 target 字符串是 "clawx"
      return ["clawx"];
    case "hermes":
      return ["hermes"];
    default:
      return [];
  }
}

/**
 * 「实验室」工具 —— 首页不推，收进最下面一个折叠区（2026-07-27 做减法）。
 *
 * 三个都不在「一键装好你的全部 AI」这条主线上：
 *   · open365  自家电脑管家，品类是安全卫士替代品，不是 AI 工具；而且它 `installed` 恒 true
 *              （按需下载的设计），于是永远霸占「我装好的 AI 工具」区最显眼的位置
 *   · obsidian / uu-remote  纯第三方，action:url 点了只是跳官网，我们既不装也不管
 *
 * 纯展示分组：后端的检测 / 启动 / 卸载能力一个没动，存量已装用户照常使用。
 */
const LAB_TOOLS = new Set(["open365", "obsidian", "uu-remote"]);

/** 支持「一键卸载」的工具 id —— 镜像后端 cleanup::uninstall_ai_tool 的 match（改一处同步另一处）。
 *  url 型第三方工具（Obsidian / UU远程）不由我们装，不给卸载入口。 */
/**
 * 「AI 设置」里那四个配置目标 → 「我的 AI」里对应的工具 id。
 *
 * 这两套 id **本来就不一样**（配置目标是 `claude`，工具是 `claude-code`），
 * 以前不用对齐是因为两个页面各管各的；「左装右选」把装机和配模型并进一屏之后，
 * 才需要一张明确的对照表。写在这里而不是 Manager 里：**Manager 不该知道工具 id**，
 * 它只说「我要装/起哪个配置目标」，由组合根翻译成具体的 ToolInfo（同四铁律的方向）。
 */
const MANAGER_TARGET_TOOL_ID: Record<string, string> = {
  claude: "claude-code",
  codex: "codex",
  clawx: "clawx",
  hermes: "hermes",
  // dsh / pi 是 2026-08-22 补的 Manager Tab（同 Manager.tsx::TARGET_TOOL_ID 那张表）——
  // 少了这两行，「左装右选」的装/启动按钮对这两个 Tab 会查不到 tools 里的条目、
  // 静默什么都不做（onInstallTool/onLaunchTool 里的 `tools.find` 会是 undefined）。
  dsh: "dsh",
  pi: "pi",
  // opencode 2026-08-24 同批加入（同上：少这一行，「左装右选」的装/启动按钮对
  // OpenCode 这个 Tab 会查不到 tools 条目、静默什么都不做）。
  opencode: "opencode",
  // cline 2026-08-29 上架同批加入（AI 设置页新 Tab 的「左装右选」装/启动按钮）。
  cline: "cline",
};

const UNINSTALLABLE = new Set([
  "claude-code",
  "codex",
  "codex-app",
  "clawx",
  "hermes",
  "hermes-app",
  "dsh",
  "harness-doctor",
  "openclaw",
  "cline",
  "ollama",
  "uu-switch",
  "open365",
]);

function MyAI({
  tools,
  driver,
  deviceKey,
  setupState,
  usingOwnKey,
  onLaunch,
  onOpen,
  onUninstall,
  onGoInstall,
  onInstallAll,
  onGoManage,
  onApplyXiapan,
  onImportXiapan,
  onRecharge,
  termSnapshot,
  recoveringTermSnapshot,
  failedTermRestoreCount,
  onRestoreTermSnapshot,
  onDismissTermSnapshot,
}: {
  tools: ToolInfo[];
  /** 每个工具当前配的模型 —— 卡片上要显示它（见下面 currentModel 那段注释）。 */
  driver: DriverStatus | null;
  deviceKey: DeviceKey | null;
  setupState: SetupState | null;
  /** 用户在用自己的 Key → XiapanGuide 换中性卡、不推虾盘云（铁律：不抢用户 Key）。 */
  usingOwnKey?: boolean;
  onLaunch: (t: ToolInfo) => void;
  onOpen: (t: ToolInfo) => void;
  /** 彻底卸载某个 AI 工具（含残留清理，修「删了还检测到」）。 */
  onUninstall: (t: ToolInfo) => void;
  onGoInstall: () => void;
  onInstallAll: () => void;
  onGoManage: () => void;
  onApplyXiapan: () => void;
  onImportXiapan: () => void;
  onRecharge: () => void;
  termSnapshot: TermSnapshotInfo | null;
  recoveringTermSnapshot: boolean;
  failedTermRestoreCount: number;
  onRestoreTermSnapshot: () => void;
  onDismissTermSnapshot: () => void;
  onManageProviders: (editId?: string) => void;
  onRefreshDriver: () => void;
}) {
  const { t: tr } = useI18n();
  // 实验室工具先摘出去：首页只留「一键装好你的全部 AI」这条主线上的东西。
  const mainline = tools.filter((t) => !LAB_TOOLS.has(t.id));
  const labTools = tools.filter((t) => LAB_TOOLS.has(t.id));
  // ★ 主推两件（2026-08-03 定稿）：**Claude Code + Hermes**。判据是「能不能把活干成」，
  // 不是「省不省钱」—— claude 干活最强、Hermes 是唯一有内置记忆的（MEMORY.md/USER.md 常开，
  // `hermes memory status` 实测）。一个把活干成，一个越用越懂你，两件就够开箱。
  //
  // 🔴 pi 从默认里撤下来了（曾短暂进过三件套）：它的长处是省钱（同任务上下文 5,000 vs
  // 24,300 token，实测），但**省钱是第二位的，先得能干活**。它照旧在下面「还能装这些」里，
  // 客户自己觉得好就装，装完点「一键接入虾盘云」也会自动配上（apply_xiapan_everywhere 已覆盖）。
  //
  // 其余 CLI（Codex/Qwen/Crush/OpenCode）**一个都没删** —— 只是不在首页推，
  // 仍在下方「还能装这些」里可装、可切驱动、可进竞技场跑分。ClawX 从这里撤下（GUI 与
  // U-Workspace 是同一生态位，主线改推自家工作台）。
  const CORE_TRIO: { id: string; badge: string; title: string; desc: string }[] = [
    { id: "claude-code", badge: "干活最强", title: "Claude Code", desc: "难活交给它，工具编排和技能生态最成熟" },
    { id: "hermes", badge: "越用越懂你", title: "Hermes", desc: "自带记忆，用得越久越了解你的习惯" },
  ];
  // 🔴 **装了就不再出大卡**（2026-08-18 客户实拍：「claude code 和 hermes 重复了 2 次，
  //    有点占位置，一页都没显示完整」）。
  //    这两张大卡是**装机漏斗**：没装时它们是「先装这两个」的引导，值得占半屏；
  //    装完之后下面「我装好的 AI 工具」网格里已经有同一个工具，而且那张卡还能换模型、
  //    重装、卸载 —— 功能是它的超集。留着大卡等于把最贵的屏幕位置花在一份更弱的副本上。
  const trio = CORE_TRIO
    .map((c) => ({ ...c, tool: tools.find((t) => t.id === c.id) }))
    .filter((c) => c.tool && !c.tool.installed);
  const coreIds = new Set(CORE_TRIO.map((c) => c.id));
  // 已装网格的排序：主推两件在最前，**DSH 紧随其后**（客户：「把 dsh 在我的 AI 里边的
  // 位置往前面移动」—— 我们内置了它，却排在一堆没装的后面，等于自己藏自己）。
  const RANK_AFTER_CORE: Record<string, number> = { dsh: CORE_TRIO.length };
  const rank = (t: ToolInfo) =>
    coreIds.has(t.id) ? CORE_TRIO.findIndex((c) => c.id === t.id) : (RANK_AFTER_CORE[t.id] ?? 99);
  const installed = mainline.filter((t) => t.installed).sort((a, b) => rank(a) - rank(b));
  const notYet = mainline.filter((t) => !t.installed).sort((a, b) => rank(a) - rank(b));
  const resumableSessionCount = termSnapshot?.sessions.filter((session) => !!session.resumeHint).length ?? 0;

  return (
    <div className="space-y-6 pb-2">
      {termSnapshot && termSnapshot.sessions.length > 0 && (
        <section className="flex flex-wrap items-center gap-3 rounded-card border border-accent/30 bg-accent/[0.07] px-4 py-3">
          <TerminalIcon size={18} className="text-accent shrink-0" />
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-semibold text-ink-0">
              {failedTermRestoreCount > 0 ? tr("{n} 条重开失败", { n: failedTermRestoreCount }) : tr("上次升级时有 {n} 个终端", { n: termSnapshot.sessions.length })}
              {resumableSessionCount > 0 && <span className="ml-1.5 text-[11px] font-normal text-ink-3">{tr("含 {n} 个可续接会话", { n: resumableSessionCount })}</span>}
            </div>
            <div className="mt-0.5 text-[11.5px] text-ink-3">{failedTermRestoreCount > 0 ? tr("快照已保留；不会自动重试，请确认后手动重试。") : tr("可重开同样目录和命令的终端；原来的屏幕内容和运行现场不会回来。")}</div>
          </div>
          <button
            onClick={onRestoreTermSnapshot}
            disabled={recoveringTermSnapshot}
            className="h-8 rounded-lg bg-accent px-3 text-[12px] font-semibold text-white hover:bg-accent-600 disabled:opacity-60"
          >
            {recoveringTermSnapshot ? tr("正在重开…") : failedTermRestoreCount > 0 ? tr("重试") : tr("一键重开")}
          </button>
          <button onClick={onDismissTermSnapshot} className="h-8 rounded-lg px-2.5 text-[12px] text-ink-3 hover:bg-white/[0.06] hover:text-ink-1">
            {tr("不再提醒")}
          </button>
        </section>
      )}
      {/* 🔴 「AI 设置」常驻入口（2026-08-25 用户拍板：「AI 设置放到我的 AI，就不隐藏了」）。
          侧栏里它仍收在「更多」折叠组（0.9.83 的下沉决定不变），但装机主流程的页面上
          必须有一张一眼看得见的卡 —— 换模型/余额/免费额度是配好能用的最后一公里，
          藏两层（更多 → AI 设置）等于让小白迷路。点击走现成的 onGoManage 深链。 */}
      <button
        onClick={onGoManage}
        className="w-full flex items-center gap-3 rounded-card border border-white/[0.10] bg-bg-1/80 px-4 py-3.5 text-left shadow-card hover:border-accent/40 hover:bg-bg-1 transition-colors"
      >
        <span className="grid place-items-center w-10 h-10 rounded-xl bg-accent/[0.14] shrink-0">
          <Cpu size={20} className="text-accent" />
        </span>
        <span className="flex-1 min-w-0">
          <span className="block text-[14px] font-semibold text-ink-0">{tr("AI 设置")}</span>
          <span className="block text-[11.5px] text-ink-3 truncate">
            {tr("换模型 · 余额 · 免费额度 · 用自己的 Key")}
          </span>
        </span>
        <ChevronRight size={16} className="text-ink-4 shrink-0" />
      </button>
            {/* ★ 主推三件套（2026-08-03 定，替换掉原「ClawX 图形版 + Hermes 终端」双入口）。
          数据驱动渲染而不是三段复制粘贴的 JSX：加/减一个只改 CORE_TRIO 数组。 */}
      <section className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        {trio.map((c) => (
          <div
            key={c.id}
            className="rounded-card border border-white/[0.08] bg-bg-1/80 px-5 py-5 shadow-card flex flex-col"
          >
            <div className="flex items-center gap-3 mb-3">
              <span className="grid h-11 w-11 place-items-center rounded-2xl bg-accent/[0.14] ring-1 ring-accent/25 shrink-0">
                <ToolIcon tool={c.id === "claude-code" ? "claude" : (c.id as any)} size={26} active />
              </span>
              <span className="inline-flex items-center rounded-full bg-accent px-2.5 py-0.5 text-[10.5px] font-semibold text-white shrink-0">
                {tr(c.badge)}
              </span>
            </div>
            <div className="text-[15px] font-semibold text-ink-0">{tr(c.title)}</div>
            <div className="mt-1 mb-4 text-[12px] text-ink-3 flex-1">{tr(c.desc)}</div>
            <button
              onClick={() => (c.tool!.installed ? onLaunch(c.tool!) : onOpen(c.tool!))}
              className="w-full inline-flex items-center justify-center gap-1.5 px-5 h-11 rounded-xl bg-accent text-white text-[14px] font-semibold hover:bg-accent-600 shadow-sm transition-colors"
            >
              <Sparkles size={16} />
              {c.tool!.installed ? tr("打开") : tr("一键安装")}
            </button>
          </div>
        ))}
      </section>

      {/* 「快速打开」小图标网格已移除 —— 它把工具又列了一遍，与下方「我装好的 AI 工具」
          + 「还能装这些」完全重复（同一批工具三处展示）。安装器只留「已装 / 可装」两段更清爽。
          dockApps/onLaunchDock 仍由 App 传入（其它页仍用），此处不再渲染。 */}

      <XiapanGuide
        setupState={setupState}
        deviceKey={deviceKey}
        hasTool={installed.length > 0}
        usingOwnKey={usingOwnKey}
        onApplyXiapan={onApplyXiapan}
        onRecharge={onRecharge}
        onGoInstall={onGoInstall}
      />
      {/* 设备钱包不在这儿了 —— 2026-08-22 F6：钱包是**虾盘云这个 provider 的一部分**，
          不是 U-King 的全局功能。删掉虾盘云它就该跟着走，否则留成一块没有归属的死砖。
          唯一实现是 `components/WalletCard.tsx`，挂在「AI 设置 → 供应商库 → 虾盘云卡片」
          （以及 Guide / Advanced 两处复用同一份）。这里原来那份 `DeviceWalletCard` 是
          1.0.5 独立长出来的第二份实现，已随本次合并删除。 */}
      {/* 已装的工具：大卡片 + 打开终端。
          标题行上原来挂着三颗小按钮 —— 2026-08-21 全部拿掉（用户：「很多小功能……跟我感觉不需要」）：
          · **复制内置 Key** → 挪去「设备钱包」。它是钱包的备份手段，不是装机页的功能；
            放在这里既找不着，又让人以为 Key 属于某个工具。
          · **体检报告** → 删。它服务的是售后排查，不是客户自己的日常，
            真要排查有 `--selfcheck` / bug 上报那条链，不该占首页视线。
          · **固定到桌面** → 删。「一键装到本地」本来就会自动建桌面快捷方式
            （`install.rs::create_shortcut`），而那才是我们推荐客户走的路。
            ⚠️ 代价说清楚：**绿色版（U 盘直跑）从此没有建快捷方式的界面入口了** ——
            它原来是那类用户唯一的一条。判断是这条路本身价值有限（快捷方式指着 U 盘，
            拔了盘就是死链），真要保留应该做进装机向导，而不是挂在首页标题行。
            动作 `runtime.desktop.pin` 仍在表里，CLI/MCP 调得到。
          🔴 删的是入口也是实现：三处对应的后端动作没动，CLI/MCP 照样调得到
          （能力和入口是两个开关，别一起拉）。*/}
      <section>
        <div className="flex items-center gap-2 mb-4">
          <span className="grid place-items-center w-7 h-7 rounded-lg bg-accent/[0.12]">
            <Sparkles size={15} className="text-accent" />
          </span>
          <h2 className="text-[15px] font-semibold text-ink-0">{tr("我装好的 AI 工具")}</h2>
        </div>

        {installed.length === 0 ? (
          <div className="rounded-card border border-dashed border-white/[0.12] bg-bg-1/50 px-6 py-12 text-center">
            <span className="grid place-items-center w-12 h-12 rounded-2xl bg-accent/[0.12] mx-auto mb-4">
              <Wand2 size={26} className="text-accent" />
            </span>
            <p className="text-[15px] font-medium text-ink-0 mb-1">{tr("还没装任何 AI 工具")}</p>
            <p className="text-[12px] text-ink-3 mb-5">{tr("点「一键全安装」自动装好全部工具 + 接好虾盘云，开箱即用")}</p>
            <div className="flex items-center justify-center gap-2">
              <button
                onClick={onInstallAll}
                className="inline-flex items-center gap-1.5 px-5 h-10 rounded-xl bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shadow-sm transition-colors"
              >
                <Download size={14} /> {tr("一键全安装")}
              </button>
              <button
                onClick={onGoInstall}
                className="inline-flex items-center gap-1.5 px-5 h-10 rounded-xl border border-white/[0.10] text-ink-1 text-[13px] font-medium hover:bg-white/[0.04] transition-colors"
              >
                {tr("逐个选装")}
              </button>
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
            {installed.map((t) => {
              const targets = toolTargets(t.id);
              // 工具 id → DriverStatus 里对应的字段。**不猜**：认不出的工具就不显示这一行，
              // 显示一个错的模型名比不显示更坏（客户会照着它去排查一个不存在的配置）。
              const currentModel =
                t.id === "claude-code" || t.id === "claude"
                  ? driver?.claude_model
                  : t.id === "codex" || t.id === "codex-cli"
                    ? driver?.codex_model
                    : t.id === "hermes"
                      ? driver?.hermes_model
                      : t.id === "dsh"
                        ? driver?.dsh_model
                        : t.id === "clawx"
                          ? driver?.clawx_model
                          : null;
              return (
                <div
                  key={t.id}
                  className="rounded-card border border-white/[0.08] bg-bg-1/70 hover:border-white/[0.14] hover:bg-bg-1 transition-colors overflow-hidden flex flex-col shadow-sm"
                >
                  {/* 卡头：图标 + 名 + 打开按钮 */}
                  <div className="flex items-center gap-3 px-4 py-4">
                    <span className="grid place-items-center w-12 h-12 rounded-xl bg-bg-3 shrink-0">
                      <ToolIcon tool={t.id} size={30} active />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="text-[14px] font-semibold text-ink-0 truncate">{t.name}</span>
                        {/* 徽章跟着首屏两卡的说法走，别一个页面上两套定位。 */}
                        {(t.id === "claude-code" || t.id === "hermes") && (
                          <span className="shrink-0 inline-flex items-center rounded-full bg-accent px-1.5 py-0.5 text-[9.5px] font-semibold text-white">
                            {tr(t.id === "hermes" ? "越用越懂你" : "干活最强")}
                          </span>
                        )}
                      </div>
                      <div className="text-[11px] text-success-400 inline-flex items-center gap-1">
                        <CheckCircle2 size={11} /> {tr("已安装")}
                      </div>
                      {/* 🔴 当前配的模型 —— 「枪 + 子弹」摆在同一行。
                          这张卡以前只写「已安装」：客户看得见**装了什么**，看不见**它现在用哪个模型**，
                          而后者才是「能不能干活」的那一半。换模型的入口还藏在卡底部的折叠项里，
                          于是「配好没有」这件事在首页上完全不可见（用户 2026-08-18：「我的 ai 安装配置…很乱」）。
                          参考 EchoBird 的模型中心：它的卡上直接写着 模型/来源/延迟，一眼知道通不通。
                          数据是现成的 —— `DriverStatus` 里每个工具的 *_model 一直都有，只是没人显示。 */}
                      {currentModel ? (
                        <div className="text-[11px] text-ink-3 flex items-center gap-1 mt-0.5" title={currentModel}>
                          <Cpu size={11} className="text-accent/70 shrink-0" />
                          <span className="truncate max-w-[200px]">{currentModel}</span>
                        </div>
                      ) : targets.length > 0 ? (
                        <div className="text-[11px] text-warning-600 dark:text-warning-400 flex items-center gap-1 mt-0.5">
                          <Cpu size={11} className="shrink-0" /> {tr("还没配模型")}
                        </div>
                      ) : null}
                    </div>
                    {t.launch_app ? (
                      <button
                        onClick={() => onLaunch(t)}
                        className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shrink-0 shadow-sm transition-colors"
                      >
                        <Sparkles size={14} /> {tr("打开应用")}
                      </button>
                    ) : t.launch_cmd ? (
                      <button
                        onClick={() => onLaunch(t)}
                        className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shrink-0 shadow-sm transition-colors"
                      >
                        {/* Hermes 已改主推终端版（2026-07-07）：点了进 app 页起 TUI 而非网页版，统一「打开终端」。 */}
                        <TerminalIcon size={14} /> {tr("打开终端")}
                      </button>
                    ) : (
                      <span className="text-[11px] text-ink-4 shrink-0 text-right">{tr("从开始菜单打开")}</span>
                    )}
                  </div>

                  {/* 卡身：给这个工具单独换模型 = 高级用法，跳到「AI 设置」高级层统一管，
                      不在卡片里就地内嵌（避免和 AI 设置页的「每工具单独配」重复成两套入口）。
                      小白走顶部「一键配好全部」即可，不用看这里。 */}
                  {targets.length > 0 && (
                    <button
                      onClick={onGoManage}
                      className="w-full flex items-center gap-1.5 border-t border-white/[0.06] bg-bg-0/60 px-3 py-2.5 text-[11.5px] text-ink-4 hover:text-ink-2 hover:bg-bg-1/80 transition-colors"
                    >
                      <Cpu size={12} />
                      {tr("单独给这个工具换模型（高级）")}
                      <ChevronRight size={12} className="ml-auto" />
                    </button>
                  )}
                  {/* uu-switch 专属：一键把「虾盘云(Claude+Codex) + 你在用的工具配置」写进它的驱动列表。 */}
                  {t.id === "uu-switch" && (
                    <button
                      onClick={onImportXiapan}
                      className="w-full flex items-center gap-1.5 border-t border-white/[0.06] bg-bg-0/60 px-3 py-2.5 text-[11.5px] text-accent hover:text-accent-600 hover:bg-bg-1/80 transition-colors"
                    >
                      <Download size={12} />
                      {tr("一键导入到 uu-switch（虾盘云 + 在用配置）")}
                      <ChevronRight size={12} className="ml-auto" />
                    </button>
                  )}
                  {/* 重新安装 / 修复：已装的卡片原来只有「打开」，一旦「已装」判错，
                      客户就被彻底困住 —— 卸载了还显示已装、点了只能打开、没有任何路子重装
                      （线上 issue #237）。检测再准也不该成为唯一出路：这里永远留一条重装口。 */}
                  {t.action === "install" && (
                    <button
                      onClick={() => onOpen(t)}
                      className="w-full flex items-center gap-1.5 border-t border-white/[0.06] bg-bg-0/60 px-3 py-2.5 text-[11.5px] text-ink-4 hover:text-accent hover:bg-bg-1/80 transition-colors"
                      title={tr("重新走一遍安装。装机清单里除 DSH 外都不锁版本，所以这一下同时就是**升级到最新版**；用不了、装坏了、或明明卸载了却还显示「已安装」时也点这里")}
                    >
                      <Download size={12} />
                      {tr("升级 / 修复（重装到最新版）")}
                      <ChevronRight size={12} className="ml-auto" />
                    </button>
                  )}
                  {/* 卸载：彻底删本体 + 残留清理（修「删了还检测到、重装又冒出来」）。二次确认在 onUninstall。
                      放卡底、默认灰、hover 才变红——是低频且破坏性操作，不该抢主操作的视觉。 */}
                  {UNINSTALLABLE.has(t.id) && (
                    <button
                      data-action-id="runtime.aitool.uninstall"
                      onClick={() => onUninstall(t)}
                      className="w-full flex items-center gap-1.5 border-t border-white/[0.06] px-3 py-2.5 text-[11.5px] text-ink-5 hover:text-red-400 hover:bg-red-500/[0.06] transition-colors"
                      title={tr("彻底卸载 {name}（含 U-King 相关残留清理）", { name: t.name })}
                    >
                      <Trash2 size={12} />
                      {tr("卸载")}
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* 可装的工具（快捷入口）*/}
      {notYet.length > 0 && (
        <section>
          <div className="flex items-center gap-2 mb-3">
            <span className="grid place-items-center w-6 h-6 rounded-md bg-accent/[0.12]">
              <Sparkles size={13} className="text-accent" />
            </span>
            <h2 className="text-[14px] font-semibold text-ink-1">{tr("还能装这些")}</h2>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {notYet.map((t) => (
              <button
                key={t.id}
                onClick={() => onOpen(t)}
                className="text-left rounded-xl border border-white/[0.06] bg-bg-1/40 hover:bg-bg-1 hover:border-white/[0.12] px-4 py-3.5 transition-colors group"
              >
                <div className="text-[13px] font-medium text-ink-0">{t.name}</div>
                <p className="mt-1 text-[11px] text-ink-3 leading-snug line-clamp-2">{t.summary}</p>
                <div className="mt-2.5 text-[11px] font-medium text-accent inline-flex items-center gap-1">
                  <Download size={11} /> {tr("一键安装")}
                </div>
              </button>
            ))}
          </div>
        </section>
      )}

      {/* 实验室工具：不在 AI 主线上的，收在最下面、默认折叠。
          「还能用、但别当主力」——把话说明白比偷偷藏起来诚实。 */}
      {labTools.length > 0 && <LabTools tools={labTools} onOpen={onOpen} onLaunch={onLaunch} />}
    </div>
  );
}

/** 首页最底的「实验室」折叠区 —— 装了也好、没装也好，统一按「点开才看得到」处理。 */
function LabTools({
  tools,
  onOpen,
  onLaunch,
}: {
  tools: ToolInfo[];
  onOpen: (t: ToolInfo) => void;
  onLaunch: (t: ToolInfo) => void;
}) {
  const { t: tr } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <section>
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-2 py-2 text-left text-ink-4 hover:text-ink-2 transition-colors"
      >
        <FlaskConical size={13} className="text-amber-400/70" />
        <span className="text-[12.5px] font-medium">{tr("实验室 · 还在测试的工具")}</span>
        <span className="text-[10px] text-ink-5">{tools.length}</span>
        <ChevronDown size={13} className={cn("transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <>
          <p className="mb-3 text-[11px] text-ink-5 leading-relaxed">
            {tr("这些不在「装好你的 AI」这条主线上，还在打磨。能用，但别当主力。")}
          </p>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {tools.map((t) => (
              <button
                key={t.id}
                onClick={() => (t.launch_app && t.installed ? onLaunch(t) : onOpen(t))}
                className="text-left rounded-xl border border-white/[0.06] bg-bg-1/40 hover:bg-bg-1 hover:border-white/[0.12] px-4 py-3.5 transition-colors"
              >
                <div className="text-[13px] font-medium text-ink-1">{t.name}</div>
                <p className="mt-1 text-[11px] text-ink-3 leading-snug line-clamp-2">{t.summary}</p>
                <div className="mt-2.5 text-[11px] font-medium text-ink-4 inline-flex items-center gap-1">
                  {t.launch_app && t.installed ? tr("打开") : tr("去看看")}
                </div>
              </button>
            ))}
          </div>
        </>
      )}
    </section>
  );
}

/* ---------------- 工具市场 ---------------- */

function ToolMarket({
  tools,
  onOpen,
  onLaunch,
  onImportXiapan,
}: {
  tools: ToolInfo[];
  onOpen: (t: ToolInfo) => void;
  onLaunch: (t: ToolInfo) => void;
  onImportXiapan: () => void;
}) {
  const { t: tr } = useI18n();
  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
      <header className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-[15px] font-semibold text-ink-0 flex items-center gap-2">
            <Sparkles size={16} className="text-accent" />
            {tr("AI 工具市场")}
          </h2>
          <p className="text-[12px] text-ink-3 mt-0.5">
            {tr("按需安装，装好点「打开」即可用。")}
          </p>
        </div>
      </header>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {tools.map((t) => (
          <div
            key={t.id}
            className="group flex flex-col rounded-lg border border-white/[0.06] bg-white/[0.02] hover:border-white/[0.10] px-4 py-3.5 transition-colors"
          >
            <div className="flex items-start justify-between gap-2">
              <span className="grid place-items-center w-9 h-9 rounded-md bg-bg-3">
                <ToolIcon tool={t.id} size={22} active={t.installed} />
              </span>
              <StatePill
                tone={t.installed ? "success" : t.action === "install" ? "warning" : "neutral"}
                label={t.installed ? tr("已安装") : t.action === "install" ? tr("可一键安装") : tr("官网指引")}
              />
            </div>
            <div className="mt-2.5 text-[13px] font-medium text-ink-1">{t.name}</div>
            <p className="mt-1 text-[11px] text-ink-3 leading-snug line-clamp-2 min-h-[30px]">{t.summary}</p>
            <div className="mt-3 flex gap-2">
              {t.installed && t.launch_app ? (
                // 已装的 GUI 应用（ClawX / Codex 桌面版）→ 打开应用。
                // uu-switch 额外给「导入虾盘云」——把虾盘云写进它的 Claude 驱动列表（两侧切换等效）。
                <>
                  <button
                    onClick={() => onLaunch(t)}
                    className="flex-1 inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
                  >
                    <Sparkles size={13} /> {tr("打开应用")}
                  </button>
                  {t.id === "uu-switch" && (
                    <button
                      onClick={onImportXiapan}
                      className="inline-flex items-center justify-center gap-1 px-2.5 h-8 rounded-md border border-white/[0.10] text-ink-2 text-[11px] hover:bg-white/[0.04]"
                      title={tr("一键把虾盘云(Claude+Codex) + 你在用的工具配置导入 uu-switch")}
                    >
                      <Download size={12} /> {tr("一键导入")}
                    </button>
                  )}
                </>
              ) : t.installed && t.launch_cmd ? (
                <>
                  <button
                    onClick={() => onLaunch(t)}
                    className="flex-1 inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
                  >
                    <TerminalIcon size={13} /> {tr("打开终端")}
                  </button>
                  <button
                    onClick={() => onOpen(t)}
                    className="inline-flex items-center justify-center px-3 h-8 rounded-md border border-white/[0.10] text-ink-3 text-[11px] hover:bg-white/[0.04]"
                    title={tr("重新安装 / 更新")}
                  >
                    <RefreshCw size={12} />
                  </button>
                </>
              ) : (
                <button
                  onClick={() => onOpen(t)}
                  className="flex-1 inline-flex items-center justify-center gap-1.5 h-8 rounded-md border border-white/[0.10] text-ink-1 text-[12px] font-medium hover:bg-white/[0.04]"
                >
                  {t.installed ? tr("已安装（从应用列表打开）") : t.action === "install" ? tr("一键安装") : tr("了解 / 安装")}
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

/* ---------------- 复用小件 ---------------- */


function StatePill({
  tone,
  label,
}: {
  tone: "success" | "warning" | "neutral";
  label: string;
}) {
  const cls: Record<string, string> = {
    success: "bg-success-500/10 text-success-400 border-success-500/25",
    warning: "bg-accent/15 text-accent border-white/[0.10]",
    neutral: "bg-white/[0.04] text-ink-2 border-white/[0.06]",
  };
  return (
    <span
      className={cn(
        "inline-flex items-center px-2 h-5 rounded text-[10px] font-mono uppercase tracking-wider border whitespace-nowrap",
        cls[tone],
      )}
    >
      {label}
    </span>
  );
}
