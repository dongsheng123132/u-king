/**
 * 任务看板（长程办公总览）—— **这台电脑上所有 AI** 的活「谁在跑 / 谁跑完 / 谁挂了」一屏看全。
 *
 * ## 数据源（都是已有事实，不造第二份）
 *  - **本工作台会话** = `useWorkbench().state.tasks`。`status` 是**运行时真值** —— Chat 的
 *    `onStatus` → `setTaskStatus` 喂进来的，不是另外存的一份状态。
 *  - **本机其它 AI 的任务** = `list_ai_tasks`（薄壳 → 影核动作 `runtime.ai_tasks.inspect`，
 *    aitasks.rs）：Claude Code / Codex CLI / Hermes 各自写的会话记录 + AI 用 uking-board
 *    技能包自己登记的任务。**只读它们自己写的文件**，不联网、不改任何东西。
 *  - **定时任务条** = `list_automations`（automation.rs，和 AutomationPanel 读同一份 JSON）。
 *
 * ## 为什么要看别家 AI 的会话
 * 客户机上真正在干活的 AI 远不止我们这一个入口 —— 他在另一个终端开着 Claude Code、
 * 编辑器里挂着 Codex。以前这块板只看得见自己工作台里的会话，「我这台电脑上的 AI 都在忙什么」
 * 根本答不了。
 *
 * ## 状态是怎么来的（别把推断说成事实）
 * 本工作台的会话状态是**真值**（我们自己在跑，知道成没成）。外部 AI 的会话记录里**没有**
 * 「这次成没成」这种字段，所以只按一条能证明的事实判：几分钟内那个会话文件还在被写 = 还在跑。
 * 卡片上因此标了来源，底部横条也写明口径。**外部会话永远不进「出错」列** —— 没有判据就不猜。
 *
 * 🔴 **列名只许说这块板能证明的事**（见下面 `COLUMNS`）。这四个来源里只有「AI 任务看板」
 * 是任务语义，其余全是会话语义 —— 一个「已完成」列会把「你今天正在干的活」画成「干完了」，
 * 而且看着完全正常。跑道 `shot-taskboard.mjs` 现在会拦住列名漂回任务语义。
 *
 * ## 为什么定时任务横排在板上方，不塞进成败列
 * 会话是「做一件事，然后结束」的生命周期，能进五列；定时任务是「每到一个点就再跑一次」
 * 的**排期**生命周期，塞进「已完成/出错」是硬造。
 *
 * ## 交互
 * 点**本工作台**的卡片 = 切到那个会话（会话一直 display 保活，不是重开）。
 * 点**外部 AI** 的卡片 = 用它的工作目录在工作台开/复用一个会话（接着那个活干）；
 * 没有工作目录的（Hermes 的会话记录里就没有这个字段）不可点。
 * 看板上的按钮全是 UI 导航，**不触发影核动作，也不挂 `data-action-id`** ——
 * 挂了动作表里没有的 id，`action bindings` 会判 stale。
 *
 * ## 窄屏 / 矮屏（见 `lib/useViewport.ts`，全应用唯一口径）
 * 这块板原来一处适配都没有。1280×640 CSS（1920×1008 物理 @150%，客户常见）实测：
 * 板子可用 626px 里 **184px 被顶部三条横幅吃掉**，卡片区只剩 404px；五列各 156px，
 * 卡片标题全成一个省略号 —— 「挤 + 大片空白」同时发生，纵向压根没做分配。
 *
 * 收的全是**装饰**，一个字的信息都不少（宪法：空间紧不是少说话的理由）：
 *  - 副标题矮屏进 `title`（同 Sidebar 的 slogan）
 *  - 定时任务**空态**从一个独立的框收成标题右边的一行字（那个框只为说一句话占了 59px）
 *  - 内边距 / 行距 / 卡片 padding 收一档
 *  - 窄屏卡片标题改两行钳位 —— 这是**多**给信息，不是少给
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Check, ClipboardList, Info, Plus, RefreshCw, User, Wrench } from "lucide-react";
import { useWorkbench } from "./store";
import { ToolIcon } from "../components/ToolIcon";
import { useI18n } from "../i18n";
import { useViewport } from "../lib/useViewport";
import { cn } from "../lib/cn";
import { dirBasename } from "./types";
import type { Task, TaskStatus } from "./types";

/** 定时任务（只读画看板条用；写操作都走自动化面板的影核动作，不在这）。 */
type AutoJob = {
  id: string;
  name: string;
  enabled: boolean;
  next_run_at: number;
  last_ok: boolean | null;
  last_message: string;
};

/** 本机别家 AI 的一条任务（形状对齐 aitasks.rs 的 `AiTask`）。 */
type AiTask = {
  id: string;
  /** 那家工具自己认的会话 id（不带 `<tool>:` 前缀）。 */
  session_id: string;
  tool: string;
  tool_label: string;
  title: string;
  dir: string;
  project: string;
  model: string;
  status: TaskStatus;
  /**
   * `mtime` = 我们从文件活跃度推的；`declared` = AI 自己写的；
   * `declared_stale` = AI 写过但那句话太老，已被后端降到 `idle`（理由在 `note` 里）。
   */
  status_from: string;
  started_at: number;
  updated_at: number;
  note: string;
  /**
   * 在这台机器上怎么接着干这一件事（`claude --resume <sid>` 之类）。
   * **空串 = 这家工具没有我们能保证跑得通的续接写法** —— 后端不编，前端也不许补。
   */
  resume_cmd: string;
};

type AiSource = {
  tool: string;
  label: string;
  path: string;
  present: boolean;
  readable: boolean;
  files_in_window: number;
  files_scanned: number;
  /** 从这个来源实际得到几条（影核观测记账：样本量必须报）。 */
  count: number;
  note: string;
};

type AiReport = {
  days: number;
  active_window_secs: number;
  tasks: AiTask[];
  sources: AiSource[];
  truncated: boolean;
  notes: string[];
};

/**
 * 五列。
 *
 * 🔴 **列名只许说这块板能证明的事。** 原来叫「待办 / 进行中 / 已完成」——那是**任务**语义，
 * 而这里 226 条数据里有 190 条是**会话**语义（会话记录里根本没有「这活成没成」这种字段）。
 * 名字和数据不是一回事，结果就是用户当天正在干的四件活
 * （讨论服务器迁移 / 调试额度耗尽 / 查计费扣款 / 检查发布准备）**全躺在「已完成」列里** ——
 * 它们一件都没做完，只是那个终端切走了五分钟。这不是数据错了，是列名替数据许了一个它兑现不了的承诺。
 *
 *  - `idle` 「没在跑」：登记过但当前没有会话在跑（含陈旧的 AI 声明，见 `DECLARED_FRESH_SECS`）
 *  - `done` 「已结束」：**那一次会话结束了，不代表事情做完了**
 */
const COLUMNS: { status: TaskStatus; label: string; dot: string; empty: string }[] = [
  { status: "idle", label: "没在跑", dot: "dot-off", empty: "没有闲着的" },
  { status: "running", label: "在跑", dot: "dot-on", empty: "没有在跑的" },
  { status: "waiting_input", label: "等待输入", dot: "dot-standby", empty: "没有等待输入的" },
  { status: "done", label: "已结束", dot: "dot-standby", empty: "没有已结束的" },
  { status: "error", label: "出错", dot: "dot-error", empty: "没有出错的" },
];

/** 每列先显示多少张，其余折起来。已完成列在真实机器上实测有 190+ 条，全铺出来只会把板压垮。 */
const VISIBLE_PER_COL = 20;

/** 本工作台自己的会话在筛选条里也是「一个来源」，且**永远在板上**（那本来就是这块板的东西）。 */
const WORKBENCH = "workbench";

/**
 * 用户勾了哪些**别家 AI** 的来源。存前端就够 —— 这是「这块板显示什么」的偏好，
 * 不是机器状态，没必要多开一个后端文件和一个动作。
 *
 * `null`（键不存在）= **还没问过他**，跟「问过了，他一个都不要」是两回事：
 * 前者要弹发现卡片，后者必须闭嘴。分不开的话，选了「先不用」的人每次进来都被问一遍。
 */
const PREF_KEY = "uking.board.ai_sources.v1";

function loadPref(): string[] | null {
  try {
    const raw = localStorage.getItem(PREF_KEY);
    if (raw == null) return null;
    const v = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : null;
  } catch {
    return null; // 存坏了当没问过，重新问一次总比崩了强
  }
}

function savePref(list: string[]) {
  try {
    localStorage.setItem(PREF_KEY, JSON.stringify(list));
  } catch {
    /* 隐私模式/配额满：存不下就这次会话内有效，不值得打扰用户 */
  }
}

/** 下次执行时间的人话说法（相对当前时间）。0/空 = 没排上。 */
function formatNext(ts: number): string {
  if (!ts || ts <= 0) return "—";
  const diff = ts - Date.now();
  if (diff <= 0) return "马上";
  const m = Math.round(diff / 60000);
  if (m < 60) return `${m} 分钟后`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} 小时后`;
  return `${Math.round(h / 24)} 天后`;
}

/** 过去时间的人话说法。 */
function formatAgo(ts: number): string {
  if (!ts || ts <= 0) return "";
  const diff = Date.now() - ts;
  if (diff < 60_000) return "刚刚";
  const m = Math.round(diff / 60000);
  if (m < 60) return `${m} 分钟前`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} 小时前`;
  return `${Math.round(h / 24)} 天前`;
}

export function TaskBoard({ onOpenSession, onOpenAutomation, onOpenFolder, onResume }: {
  /** 点本工作台的卡片：切到那个会话（宿主负责 activate + 切 chat 视图）。 */
  onOpenSession: (id: string) => void;
  /** 点「去配置」：跳到自动化面板。 */
  onOpenAutomation: () => void;
  /** 点外部 AI 的卡片：用它的工作目录在工作台开/复用一个会话。 */
  onOpenFolder: (dir: string) => void;
  /** 「接着干」：开/复用该目录的会话，并把那条续接命令贴进它的终端。 */
  onResume: (dir: string, cmd: string) => void;
}) {
  const { t: tr } = useI18n();
  /** 窄=横向收内边距/间距、标题给两行；矮=纵向收横幅。判据只有这一个来源（宪法第 8 条）。 */
  const { narrow, short } = useViewport();
  const { state, addTask } = useWorkbench();
  const [jobs, setJobs] = useState<AutoJob[]>([]);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [loadingJobs, setLoadingJobs] = useState(true);
  const [report, setReport] = useState<AiReport | null>(null);
  const [days, setDays] = useState(7);
  const [scanning, setScanning] = useState(true);
  /**
   * 勾进板里的**别家 AI** 来源。`null` = 还没问过他（要弹发现卡片）。
   *
   * 🔴 **默认不是全开**：这台机器上实测有 200+ 条别家会话，一进来就把板铺满，
   * 客户第一反应是「这什么东西、我的会话呢」。探测到 ≠ 直接摆上去 ——
   * 先问一句，他勾了才算数。
   */
  const [included, setIncluded] = useState<string[] | null>(() => loadPref());
  const [expanded, setExpanded] = useState<Set<TaskStatus>>(new Set());

  // 定时任务条：轮询 automation.rs 的只读状态（和应用内其它地方读的是同一份 JSON）。
  // 调度在应用内线程跑，到点发生在任何时刻；这一步只是读个 JSON，成本忽略。
  useEffect(() => {
    let alive = true;
    const scan = () =>
      invoke<{ jobs?: AutoJob[]; running_id?: string | null }>("list_automations")
        .then((r) => {
          if (!alive) return;
          setJobs(r?.jobs ?? []);
          setRunningId(r?.running_id ?? null);
          setLoadingJobs(false);
        })
        .catch(() => { if (alive) setLoadingJobs(false); });
    void scan();
    const id = setInterval(scan, 30_000);
    return () => { alive = false; clearInterval(id); };
  }, []);

  /**
   * 扫本机各家 AI 的任务。
   *
   * 比定时任务那条慢得多（真机 200+ 会话实测 0.7 秒，要翻几百个文件），所以：
   * 轮询间隔放到 60 秒，并且给一个手动刷新 —— 客户盯着「进行中」那列看时，
   * 等一分钟才更新是难受的。
   */
  const rescan = useCallback(() => {
    setScanning(true);
    return invoke<AiReport>("list_ai_tasks", { days })
      .then((r) => setReport(r ?? null))
      .catch(() => setReport(null))
      .finally(() => setScanning(false));
  }, [days]);

  useEffect(() => {
    let alive = true;
    const run = () => { if (alive) void rescan(); };
    run();
    const id = setInterval(run, 60_000);
    return () => { alive = false; clearInterval(id); };
  }, [rescan]);

  const aiTasks = useMemo(() => report?.tasks ?? [], [report]);

  /**
   * 探测到的**别家 AI** 来源：读得动、且这台机器上真有东西的才算。
   * 装了但一条会话都没有的不摆出来 —— 一个恒为 0 的勾选框只会让人以为坏了。
   */
  const detected = useMemo(
    () => (report?.sources ?? []).filter((s) => s.readable && s.count > 0),
    [report],
  );

  /** 还没问过他，且确实探测到了东西 → 该弹发现卡片。 */
  const askToInclude = included === null && detected.length > 0;

  const isOn = useCallback(
    (tool: string) => tool === WORKBENCH || (included ?? []).includes(tool),
    [included],
  );

  /** 勾选任何一个来源都算「问过了」——之后不再弹发现卡片。 */
  const setSources = useCallback((list: string[]) => {
    setIncluded(list);
    savePref(list);
  }, []);

  const toggleChip = useCallback(
    (key: string) => {
      if (key === WORKBENCH) return; // 本工作台是这块板的本体，不给关
      const cur = included ?? [];
      setSources(cur.includes(key) ? cur.filter((k) => k !== key) : [...cur, key]);
    },
    [included, setSources],
  );

  /** 筛选条：本工作台 + 已经问过之后的每个探测到的来源。发现卡片还挂着时不重复摆一排。 */
  const chips = useMemo(() => {
    // 🔴 `note` 必须跟着 chip 走。后端对每个来源都说了实话
    // （「装了，但最近 7 天内没有新会话」「状态由 AI 自己声明，不是我们推的」），
    // 而这里原来只取 label+count 就丢掉了 —— 于是界面上 Hermes 显示一个「1」，
    // 客户没法知道那 1 条是哪来的、可不可信。**后端诚实了，前端把它咽回去，等于没诚实。**
    const list: { key: string; label: string; count: number; note?: string }[] = [
      { key: WORKBENCH, label: "本工作台", count: state.tasks.length },
    ];
    if (!askToInclude) {
      for (const s of detected) list.push({ key: s.tool, label: s.label, count: s.count, note: s.note });
    }
    return list;
  }, [detected, askToInclude, state.tasks.length]);

  /** 有话要说的来源 —— 渲染成可见文字（见下面那段注释）。 */
  const chipNotes = useMemo(
    () => chips.filter((c) => !!c.note && isOn(c.key)).map((c) => ({ key: c.key, label: c.label, note: c.note! })),
    [chips, isOn],
  );

  // 按状态分桶。本工作台的会话排在前面（那是我们自己在跑的，最可能要点开），
  // 外部 AI 的按最后活动时间由新到旧（后端已排好序）。
  const byStatus = useMemo(() => {
    const m: Record<TaskStatus, { own: Task[]; ai: AiTask[] }> = {
      idle: { own: [], ai: [] },
      running: { own: [], ai: [] },
      waiting_input: { own: [], ai: [] },
      done: { own: [], ai: [] },
      error: { own: [], ai: [] },
    };
    for (const t of state.tasks) m[t.status]?.own.push(t);
    for (const t of aiTasks) {
      if (!isOn(t.tool)) continue;
      m[t.status]?.ai.push(t);
    }
    return m;
  }, [state.tasks, aiTasks, isOn]);

  const pickFolder = useCallback(async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: tr("选择项目文件夹") });
    if (typeof dir === "string" && dir) await addTask(dir, "manual", false);
  }, [addTask, tr]);

  /** 读不动 / 没装的来源，如实摆在底部 —— 「没读到」和「没有」是两回事。 */
  const gaps = useMemo(
    () => (report?.sources ?? []).filter((s) => s.present && !s.readable),
    [report],
  );

  /** 矮屏 + 一条定时任务都没有 → 那句话挂在标题行里，不再单独画一个 59px 的框。 */
  const autoEmptyInline = short && !loadingJobs && jobs.length === 0;

  return (
    <div className="h-full flex flex-col bg-bg-2">
      {/* 头部。矮屏收掉副标题那一行 —— 话没少说，挪进标题的 tooltip（同 Sidebar 的 slogan）。 */}
      <header className={cn("flex items-start gap-3", short ? "px-4 pt-2.5 pb-1.5" : "px-5 pt-4 pb-3")}>
        <div className="min-w-0">
          <h2
            className="text-[15px] font-semibold text-ink-0"
            title={short ? tr("这台电脑上所有 AI 的会话：谁在跑、谁跑完、谁挂了。任务状态在左栏「护照」。") : undefined}
          >
            {tr("任务看板")}
          </h2>
          {!short && (
            <p className="text-[12px] text-ink-4 mt-0.5">
              {tr("这台电脑上所有 AI 的会话：谁在跑、谁跑完、谁挂了。任务状态在左栏「护照」。")}
            </p>
          )}
        </div>
        <div className="ml-auto flex items-center gap-1.5 shrink-0">
          {/* 回看窗口。默认 7 天 —— 看板问的是「最近在忙什么」，不是全部历史。 */}
          {[7, 30].map((d) => (
            <button
              key={d}
              onClick={() => setDays(d)}
              className={"text-[11.5px] px-2 py-1 rounded-lg border transition-colors " +
                (days === d
                  ? "border-accent/50 text-accent-400 bg-accent/[0.08]"
                  : "border-white/[0.08] text-ink-4 hover:text-ink-2 hover:border-white/[0.16]")}
            >
              {tr("{d} 天", { d: String(d) })}
            </button>
          ))}
          <button
            onClick={() => { void rescan(); }}
            disabled={scanning}
            title={tr("重新扫一遍本机各家 AI 的任务")}
            className="p-1.5 rounded-lg border border-white/[0.08] text-ink-4 hover:text-ink-2 hover:border-white/[0.16] transition-colors disabled:opacity-50"
          >
            <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
          </button>
        </div>
      </header>

      {/* 🔴 护照那条横条搬走了（→ `PassportBoard.tsx`，左栏「护照」一等入口）。
          它当年缩在这块板的页眉上，230px 宽的卡片里目标 / 已验证事实 / 下一步一个字都露不出来，
          只剩一串 id 和一个「复制交接口令」—— 客户点完不知道任务去了哪儿。
          会话看板答「谁在跑」，护照答「事情做到哪」，两个生命周期，两个入口。 */}

      {/* 发现卡片：探测到别家 AI，但**先问一句**再往板上放 */}
      {askToInclude && (
        <DiscoveryCard sources={detected} onConfirm={setSources} tr={tr} />
      )}

      {/* 来源筛选条 */}
      <div className={cn("flex flex-wrap items-center gap-1.5", short ? "px-4 pb-1.5" : "px-5 pb-2.5")}>
        {chips.map((c) => {
          const on = isOn(c.key);
          return (
            <button
              key={c.key}
              onClick={() => toggleChip(c.key)}
              // 稳定的非视觉标识，给排版/回归跑道用。**不是** `data-action-id` ——
              // 筛选来源是界面动作（切视图、改本地筛选），不进动作表；挂了动作表里没有的 id，
              // `action bindings` 会判 stale 直接失败。
              data-testid={`board-chip-${c.key}`}
              // 有 note 就把它顶到 title 最前面 —— 「这个来源可不可信」比「点一下会怎样」重要得多
              title={c.note ? `${tr(c.note)}
——
${tr(on ? "点一下隐藏这个来源" : "点一下显示这个来源")}` : tr(on ? "点一下隐藏这个来源" : "点一下显示这个来源")}
              className={"flex items-center gap-1.5 text-[11.5px] px-2 py-1 rounded-lg border transition-colors " +
                (on
                  ? "border-white/[0.14] text-ink-1 bg-white/[0.04]"
                  : "border-white/[0.06] text-ink-4 opacity-55 hover:opacity-80")}
            >
              {c.key === WORKBENCH ? (
                <Wrench size={11} className="shrink-0" />
              ) : c.key === "board" ? (
                <ClipboardList size={11} className="shrink-0" />
              ) : (
                <ToolIcon tool={c.key} size={12} active={on} />
              )}
              <span>{tr(c.label)}</span>
              <span className="text-ink-4">{c.count}</span>
              {/* 来源有话要说时给个可见记号 —— 只放 tooltip 等于没说：没人会去悬停一个看起来正常的 chip */}
              {c.note && <Info size={10} className="text-warning-500/80 shrink-0" />}
            </button>
          );
        })}
      </div>

      {/* 🔴 来源说明**必须是可见文字，不能只挂 tooltip**。
          后端对每个来源都说了实话（「装了，但最近 7 天内没有新会话」「状态由 AI 自己声明」），
          第一版我把它塞进了 `title` —— 而没人会去悬停一个看起来正常的 chip，
          等于后端诚实了、前端把它咽了回去。探针也证实了：innerText 里根本搜不到。
          客户问的是「这个 1 是哪来的、可不可信」，那句话得在屏幕上。 */}
      {chipNotes.length > 0 && (
        <div className={cn("flex flex-col gap-0.5", short ? "px-4 pb-1.5" : "px-5 pb-2.5")}>
          {chipNotes.map((n) => (
            <div key={n.key} className="text-[10.5px] text-ink-4 flex items-start gap-1">
              <Info size={10} className="text-warning-500/70 shrink-0 mt-[2px]" />
              <span>
                <span className="text-ink-3">{tr(n.label)}</span>：{tr(n.note)}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* 定时任务横条。
          🔴 矮屏下的**空态**不再画那个框：它整块 59px 只为说一句话，而这块板上真正稀缺的
          就是卡片区的纵向像素。话原样保留，挪到标题右边同一行 —— 收的是框，不是信息。 */}
      <div className={cn(short ? "px-4 pb-1.5" : "px-5 pb-3")}>
        <div className={cn("flex items-center gap-2", !autoEmptyInline && "mb-1.5")}>
          <span className="text-[12px] font-medium text-ink-3 shrink-0">{tr("定时任务")}</span>
          {autoEmptyInline && (
            <span className="text-[11.5px] text-ink-4 truncate min-w-0">
              {tr("还没有定时任务 —— 到点让 AI 自己干活，见「自动化」")}
            </span>
          )}
          <button onClick={onOpenAutomation} className="ml-auto shrink-0 text-[12px] text-accent-400 hover:text-accent-300 transition-colors">
            {tr("去配置 →")}
          </button>
        </div>
        {loadingJobs || autoEmptyInline ? null : jobs.length === 0 ? (
          <div className="text-[12px] text-ink-4 bg-white/[0.03] border border-white/[0.06] rounded-xl px-3 py-2.5">
            {tr("还没有定时任务 —— 到点让 AI 自己干活，见「自动化」")}
          </div>
        ) : (
          <div className="flex gap-2 overflow-x-auto pb-1">
            {jobs.map((j) => (
              <div
                key={j.id}
                onClick={onOpenAutomation}
                title={j.last_message || j.name}
                className={"min-w-[190px] max-w-[230px] shrink-0 bg-bg-1 rounded-lg border p-2.5 cursor-pointer transition-colors " +
                  (j.id === runningId
                    ? "border-accent/50"
                    : j.enabled
                      ? "border-white/[0.08] hover:border-white/[0.16]"
                      : "border-white/[0.05] opacity-60")}
              >
                <div className="flex items-center gap-2 min-w-0">
                  <span className={"dot shrink-0 " + (j.id === runningId ? "dot-on" : j.last_ok === false ? "dot-error" : j.last_ok === true ? "dot-standby" : "dot-off")} />
                  <span className="text-[12px] font-medium text-ink-0 truncate">{j.name}</span>
                </div>
                <div className="mt-1 text-[11px] text-ink-4 truncate">
                  {tr(j.enabled ? "下次 {t}" : "已停用", { t: formatNext(j.next_run_at) })}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 五列看板。窄屏把左右内边距收一档 —— 这 16px 直接进列宽（156→160），
          比留白值钱；宽屏一个像素不变。 */}
      <div className={cn("flex-1 min-h-0 overflow-x-auto", narrow ? "px-3" : "px-5")}>
        {/* 🔴 `min-w` 是**地板不是固定宽**：窗口宽时列会自己撑开，窄时才收到这个下限。
            原来写死 860px —— 实测真窗口（物理 1418px @125% DPI = CSS 1134px）里，侧栏 + 会话栏
            吃掉之后板子只剩 **722px**，于是 860 撑出 138px 横向溢出，**「出错」那列被挤到屏幕外**。
            偏偏那是最该一眼看见的一列，而横向滚动条在满屏卡片里根本没人注意到。
            660 能让五列在 722px 里都露出来（约 134px/列，卡片本来就 truncate）。
            宁可五列都挤一点，也不能让人以为「没有出错的」。 */}
        <div className={cn("grid grid-cols-5 h-full min-w-[660px]", narrow ? "gap-2" : "gap-3")}>
          {COLUMNS.map((col) => {
            const { own, ai } = byStatus[col.status];
            const total = own.length + ai.length;
            const isOpen = expanded.has(col.status);
            // 折叠只砍外部那批：本工作台的会话是用户自己开的，一共也没几个，永远全显示。
            const shownAi = isOpen ? ai : ai.slice(0, Math.max(0, VISIBLE_PER_COL - own.length));
            const restAi = ai.length - shownAi.length;
            return (
              // 🔴 `min-w-0` 不能少：grid 子项默认 min-width:auto，会被卡片里的文字撑到
              // min-content 宽度 —— 于是 5 列实际要 1060px 而不是声明的 860px，在 1418px 窗口上
              // （侧栏 + 会话栏吃掉 380px）**「出错」那列被挤到屏幕外**，只能横向滚才看得到。
              // 偏偏那是最该一眼看见的一列。加上它，truncate 才真的生效。
              <div key={col.status} className="flex flex-col min-h-0 min-w-0 bg-white/[0.03] border border-white/[0.06] rounded-xl">
                <header className="px-3 py-2 flex items-center gap-2 border-b border-white/[0.05]">
                  <span className={"dot " + col.dot} />
                  <span className="text-[12.5px] font-medium text-ink-0">{tr(col.label)}</span>
                  <span className="ml-auto text-[11px] text-ink-4">{total}</span>
                  {col.status === "idle" && (
                    <button onClick={pickFolder} title={tr("新建项目")} className="text-ink-4 hover:text-accent-400 transition-colors">
                      <Plus size={14} />
                    </button>
                  )}
                </header>
                {/* 空列的那句话竖直居中 —— 顶在最上面、底下吊着 350px 空白，看着像没渲染完。 */}
                <div className={cn("flex-1 overflow-y-auto p-1.5", total === 0 ? "grid place-items-center" : "space-y-1.5")}>
                  {total === 0 ? (
                    <div className="text-[11px] text-ink-4 px-2 text-center">{tr(col.empty)}</div>
                  ) : (
                    <>
                      {own.map((t) => (
                        <button
                          key={t.id}
                          onClick={() => onOpenSession(t.id)}
                          title={t.dir || undefined}
                          className={cn(
                            "w-full text-left bg-bg-1 hover:bg-white/[0.05] border border-white/[0.06] hover:border-accent/40 rounded-lg transition-colors",
                            narrow ? "p-2" : "p-2.5",
                          )}
                        >
                          <div className={cn("flex gap-1.5 min-w-0", narrow ? "items-start" : "items-center")}>
                            {t.expert ? <User size={12} className={cn("text-accent-400 shrink-0", narrow && "mt-0.5")} /> : t.kind === "tool" ? <Wrench size={12} className={cn("text-ink-4 shrink-0", narrow && "mt-0.5")} /> : null}
                            {/* 窄屏给两行：156px 的列里 truncate 只剩「帮我写一篇公众…」，
                                两行钳位能把标题看完 —— 这是**多**给信息，不是省地方。 */}
                            <span className={cn("text-[12.5px] font-medium text-ink-0 min-w-0 flex-1", narrow ? "line-clamp-2 leading-snug" : "truncate")}>{t.name}</span>
                          </div>
                          <div className="mt-1 text-[11px] text-ink-4 truncate">
                            {t.dir ? dirBasename(t.dir) : t.tool ?? tr("无工作文件夹")}
                          </div>
                        </button>
                      ))}
                      {shownAi.map((t) => (
                        <AiCard key={t.id} task={t} onOpenFolder={onOpenFolder} onResume={onResume} tr={tr} narrow={narrow} />
                      ))}
                      {restAi > 0 && (
                        <button
                          onClick={() => setExpanded((p) => new Set(p).add(col.status))}
                          className="w-full text-[11px] text-ink-4 hover:text-accent-400 py-1.5 transition-colors"
                        >
                          {tr("还有 {n} 条 ↓", { n: String(restAi) })}
                        </button>
                      )}
                    </>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* 口径与缺口。**这条不能省**：外部 AI 的状态是推的不是真值，读不到的来源也得说出来，
          不然一块「什么都看得见」的板会让人以为这就是全部。 */}
      <footer className={cn("text-[11px] text-ink-4", short ? "px-4 py-1.5 leading-snug" : "px-5 py-2.5 leading-relaxed")}>
        {report ? (
          <>
            {/* 🔴 这句是这块板最该说清的一件事。用户的原话是「看板和我在做的事不大一致」——
                因为他当天在干的活全在「已结束」列里。列名改完还得把口径写出来，
                否则下一个人还是会把「已结束」读成「已完成」。 */}
            {tr(
              "「已结束」= 那个会话文件近 {s} 秒没被写，不代表事情做完了；「没在跑」= 登记过但当前没有会话在跑。外部 AI 的记录里没有「这次成没成」这种字段，所以永远不进「出错」列 —— 只有本工作台的状态是真值。",
              { s: String(report.active_window_secs) },
            )}
            {gaps.map((g) => (
              <span key={g.tool} className="block mt-0.5">
                {tr("{label}：{note}", { label: g.label, note: g.note })}
              </span>
            ))}
            {report.truncated && (
              <span className="block mt-0.5">{tr("会话文件太多，只取了最新的那批 —— 更早的没扫。")}</span>
            )}
          </>
        ) : scanning ? (
          tr("正在扫本机各家 AI 的任务记录…")
        ) : (
          tr("没能读到本机其它 AI 的任务记录（下面只显示本工作台的会话）。")
        )}
      </footer>
    </div>
  );
}

/**
 * 「在这台电脑上还发现了这些 AI 的任务，要一起看吗」。
 *
 * **探测到 ≠ 直接摆上去**。这台机器实测有 200+ 条别家会话，不问一声就铺满，
 * 客户第一反应是「这什么东西、我自己的会话呢」。所以：默认一条都不加，
 * 勾好了点「加到看板」才进去；点「先不用」也算答过了，之后不再问。
 */
function DiscoveryCard({ sources, onConfirm, tr }: {
  sources: AiSource[];
  onConfirm: (list: string[]) => void;
  tr: (s: string, v?: Record<string, string>) => string;
}) {
  // 卡片里默认全勾上 —— 让「都要」是一次点击，而不是勾三下。
  // 但**没点确认之前板上一条都不加**，勾选状态只活在这张卡里。
  const [picked, setPicked] = useState<string[]>(() => sources.map((s) => s.tool));
  const toggle = (tool: string) =>
    setPicked((p) => (p.includes(tool) ? p.filter((t) => t !== tool) : [...p, tool]));
  const total = sources.reduce((a, s) => a + s.count, 0);

  return (
    <div className="mx-5 mb-3 rounded-xl border border-accent/25 bg-accent/[0.05] px-3.5 py-3">
      <div className="text-[12.5px] text-ink-1">
        {tr("这台电脑上还发现了 {n} 条别的 AI 的任务，要一起显示在看板上吗？", { n: String(total) })}
      </div>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {sources.map((s) => {
          const on = picked.includes(s.tool);
          return (
            <button
              key={s.tool}
              onClick={() => toggle(s.tool)}
              data-testid={`board-discover-${s.tool}`}
              className={"flex items-center gap-1.5 text-[11.5px] px-2 py-1 rounded-lg border transition-colors " +
                (on
                  ? "border-accent/50 bg-accent/[0.10] text-ink-0"
                  : "border-white/[0.10] text-ink-4 hover:text-ink-2")}
            >
              <span className={"w-3 h-3 rounded-[4px] border grid place-items-center shrink-0 " +
                (on ? "border-accent/60 bg-accent/30" : "border-white/20")}>
                {on ? <Check size={9} /> : null}
              </span>
              {s.tool === "board" ? <ClipboardList size={11} /> : <ToolIcon tool={s.tool} size={12} active={on} />}
              <span>{tr(s.label)}</span>
              <span className="text-ink-4">{s.count}</span>
            </button>
          );
        })}
      </div>
      <div className="mt-2.5 flex items-center gap-2">
        <button
          onClick={() => onConfirm(picked)}
          disabled={picked.length === 0}
          data-testid="board-discover-confirm"
          className="text-[12px] px-2.5 py-1 rounded-lg bg-accent/80 hover:bg-accent text-white transition-colors disabled:opacity-40"
        >
          {tr("加到看板")}
        </button>
        {/* 「先不用」必须落盘成一个**空选择**，不能什么都不存 ——
            不然下次进来又问一遍，那就成了骚扰。 */}
        <button
          onClick={() => onConfirm([])}
          data-testid="board-discover-skip"
          className="text-[12px] px-2.5 py-1 rounded-lg border border-white/[0.10] text-ink-3 hover:text-ink-1 transition-colors"
        >
          {tr("先不用")}
        </button>
        <span className="text-[11px] text-ink-4">{tr("之后随时能在上面那排来源里改")}</span>
      </div>
    </div>
  );
}

/**
 * 外部 AI 的一张卡。跟本工作台的卡片长得不一样是**故意的** —— 状态来源不同，别让人以为一样可信。
 *
 * ## 点它会发生什么（这里是「接着干」和「只开文件夹」的分界）
 * - `resume_cmd` 非空（Claude Code / Codex）→ **接着这一次会话干**：
 *   用它的工作目录开/复用一个会话，并把 `claude --resume <sid>` 贴进那个会话的终端。
 *   **不替用户回车** —— 见 `termInbox.ts`。
 * - `resume_cmd` 是空的（Hermes / OpenClaw / 看板任务）→ 退回**只开文件夹**，
 *   并且卡片上如实写着「只开文件夹」。后端拿不出可靠的续接写法时前端不补一条，
 *   补出来的命令会开一个新会话、丢掉全部上文，而界面上跟真续上了长得一模一样。
 */
function AiCard({ task, onOpenFolder, onResume, tr, narrow }: {
  task: AiTask;
  onOpenFolder: (dir: string) => void;
  onResume: (dir: string, cmd: string) => void;
  tr: (s: string, v?: Record<string, string>) => string;
  /** 窄屏：内边距收一档 + 标题两行钳位（口径来自宿主的 `useViewport`，这里不自己再判一次）。 */
  narrow: boolean;
}) {
  const canResume = !!task.resume_cmd && !!task.dir;
  const clickable = !!task.dir;
  const sub = [task.project || task.model, formatAgo(task.updated_at)].filter(Boolean).join(" · ");
  return (
    <button
      onClick={
        canResume ? () => onResume(task.dir, task.resume_cmd)
        : clickable ? () => onOpenFolder(task.dir)
        : undefined
      }
      disabled={!clickable}
      title={
        (task.note ? task.note + "\n" : "") +
        (task.dir || tr("这条记录里没有工作目录")) +
        "\n" +
        (canResume
          ? tr("点它：在这个文件夹开个会话，并把「{cmd}」贴进终端（不替你回车）", { cmd: task.resume_cmd })
          : tr("这家工具没有可靠的续接命令，点它只开文件夹")) +
        "\n" +
        (task.status_from === "declared"
          ? tr("状态是 AI 自己写在看板上的")
          : task.status_from === "declared_stale"
            // AI 说过它在做，但那句话太老了。降级的理由后端已经写进 `note`（在第一行），
            // 这里只解释「为什么它不在『在跑』那一列」——不写的话看着像任务丢了。
            ? tr("AI 声明过状态，但太久没更新，已经不能当「现在」用了")
            : tr("状态按会话文件还在不在被写推的"))
      }
      className={cn(
        "w-full text-left bg-bg-1 border border-white/[0.06] rounded-lg transition-colors",
        narrow ? "p-2" : "p-2.5",
        clickable ? "hover:bg-white/[0.05] hover:border-accent/40 cursor-pointer" : "opacity-80 cursor-default",
      )}
    >
      <div className={cn("flex gap-1.5 min-w-0", narrow ? "items-start" : "items-center")}>
        {task.tool === "board" ? (
          <ClipboardList size={12} className={cn("text-ink-4 shrink-0", narrow && "mt-0.5")} />
        ) : (
          <span className={cn("shrink-0", narrow && "mt-0.5")}><ToolIcon tool={task.tool} size={12} /></span>
        )}
        <span className={cn("text-[12.5px] text-ink-1 min-w-0 flex-1", narrow ? "line-clamp-2 leading-snug" : "truncate")}>{task.title}</span>
      </div>
      <div className="mt-1 flex items-center gap-1.5 min-w-0">
        <span className="text-[10.5px] text-ink-4 shrink-0 px-1 rounded bg-white/[0.05]">{task.tool_label}</span>
        <span className="text-[11px] text-ink-4 truncate flex-1">{sub}</span>
        {/* 点下去会发生什么，写在卡片上而不是只写在 title 里 —— 悬停才看得见的说明，
            对「我点了怎么没接着上文」这个具体困惑帮不上忙。 */}
        {clickable && (
          <span className={cn("text-[10.5px] shrink-0", canResume ? "text-accent-400" : "text-ink-4")}>
            {canResume ? tr("接着干 ↩") : tr("只开文件夹")}
          </span>
        )}
      </div>
    </button>
  );
}
