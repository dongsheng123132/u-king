/**
 * 左侧列表 —— 按项目（文件夹）分组，每个项目下列多个 AI 会话（claude/codex/openclaw…）。
 * status 小圆点：idle 灰 / running 绿 / error 红。
 *
 * ## 「AI 专家」「自动化」为什么在这里（借鉴 WorkBuddy 的左栏信息架构）
 * WorkBuddy 把「专家·技能·连接器」「自动化」和会话列表放在**同一根左栏**里：挑个专家、
 * 配条定时任务，都不用离开工作台。我们照这个改 —— 以前「AI 专家」是侧栏另一个页，
 * 客户得跳出工作台挑完再被送回来，中间断一次。现在就在手边。
 *
 * 切这两个面板**不卸载任何会话**（右侧 Chat 实例照旧 display 保活，PTY 不断），
 * 点任意会话就回到 chat 视图。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { Archive, ArchiveRestore, ChevronsLeft, ChevronsRight, ChevronDown, ChevronUp, ClipboardList, FolderPlus, GitBranch, GripVertical, LayoutDashboard, MessageSquarePlus, Plus, Trash2, Users, X, Zap } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Task, WorkView } from "./types";
import { dirBasename, normDir } from "./types";
import { useWorkbench } from "./store";
import { useI18n } from "../i18n";
import { askConfirm } from "../lib/confirm";
import { overrideYield, useYieldLevel, YIELD_SESSION_BAR } from "../lib/yieldChain";
import { useWorkbenchOffer } from "./useWorkbenchOffer";

/**
 * 「这个会话此刻真的有 AI 在干活」。
 *
 * 🔴 不能只看 `status === "running"`：**终端型会话（`kind === "tool"`）建出来就硬写
 * `status: "running"` 且此后永不变**（见 store.tsx 的 `addSession`）—— 那个值的意思是
 * 「这个终端开着」，不是「里面有人在干活」。PTY 对面跑没跑我们根本探不到。
 * 拿它当活动指示 = 一个永远为真的东西在冒充活动信号，加上动效之后会**变本加厉**：
 * 一排永远呼吸的终端会话，把真正在跑的那个 AI 会话彻底淹掉。
 *
 * 所以活动指示只认 AI 会话 —— 它的 running 是 Chat / ChatPanel 的 `onStatus` 在每一轮
 * 开始/结束时真喂进来的。终端会话保持静态绿点（进程活着），语义不变。
 */
function isLive(t: Task): boolean {
  return t.status === "running" && t.kind !== "tool";
}

/**
 * 状态灯四态（0.9.83 依测试报告 #008 补 Standby；本次补「正在干活」的动效一档）。
 *
 * 老逻辑只有「跑着=绿 / 其余=灰」，于是**聊过一半、随时能接着聊**的会话跟一个空白新会话
 * 长得一模一样 —— 客户读到的是「离线」，实际它只是这一秒没在说话。这不是审美问题：
 * 灰色会让人以为得重开一个，于是同一个文件夹开出好几个会话。
 *
 *   干活中 dot-live    AI 正在跑这一轮 —— 呼吸（6px 的静态色差在余光里读不出来）
 *   在线   dot-on      终端型会话：进程开着
 *   Standby dot-standby 有对话历史、当前空闲 —— 点进去就接着聊（--resume 真的续得上）
 *   离线   dot-off     全新会话，一句话都没说过
 *   出错   dot-error   上一轮失败
 */
function statusDot(t: Task, hasHistory: boolean): string {
  if (isLive(t)) return "dot-live";
  if (t.status === "running") return "dot-on";
  if (t.status === "error") return "dot-error";
  return hasHistory ? "dot-standby" : "dot-off";
}

function statusTitle(t: Task, hasHistory: boolean): string {
  if (isLive(t)) return "正在干活中";
  if (t.status === "running") return "在线 · 终端开着";
  if (t.status === "error") return "上一轮出错";
  return hasHistory ? "Standby · 聊过，点进去接着聊" : "离线 · 还没开始";
}

/**
 * 「跑了多久」。光有个点只答得了「在不在跑」，答不了**「是不是卡住了」**——
 * 而后者才是客户真正想知道的（跑了 8 秒和跑了 8 分钟是两件事，pc-*** 那类「卡住」
 * 的第一手线索就是这个数）。
 */
function elapsedLabel(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}:${String(s % 60).padStart(2, "0")}`;
  return `${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`;
}

/**
 * 哪些会话有对话历史 —— 直接看 Chat/ChatPanel 落在 localStorage 的那份存档，
 * 不另存一份「有没有聊过」的标志位（同一事实存两份就会漂移，宪法第 8 条）。
 *
 * 前缀匹配是故意的：`uking.chat.<id>`（U-King 轻助手）和 `uking.chat.<id>-claude`
 * （切到 Claude/Codex 大脑）是两份存档，任一份非空都算聊过。
 */
/**
 * 对话存档一共有**两份前缀**，少扫一个就会把「聊过」判成「没聊过」：
 *  - `uking.chat.<id>`            —— U-King 轻助手（Chat.tsx）
 *  - `uking.chatpanel.<id>-<引擎>` —— Claude / Codex（panels/ChatPanel.tsx）
 *
 * 🔴 这里以前只扫第一个。而**默认大脑是 Claude** —— 纯 Claude 的会话在列表上一直是
 * 「离线·还没开始」，关它时也会被当成空会话。判据漏了一半，比没有判据更危险。
 */
const CHAT_STORE_PREFIXES = ["uking.chat.", "uking.chatpanel."] as const;

/** 这个 localStorage key 是不是属于某个会话（两种前缀 + `<id>` 或 `<id>-引擎`）。 */
function archiveOwner(key: string, taskIds: string[]): string | null {
  for (const pre of CHAT_STORE_PREFIXES) {
    if (!key.startsWith(pre)) continue;
    const suffix = key.slice(pre.length);
    for (const id of taskIds) {
      if (suffix === id || suffix.startsWith(id + "-")) return id;
    }
  }
  return null;
}

function chattedTaskIds(taskIds: string[]): Set<string> {
  const out = new Set<string>();
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (!k) continue;
      const raw = localStorage.getItem(k);
      // "[]" / null / "" 都算没聊过 —— 建了会话没说话不该亮 Standby
      if (!raw || raw.length < 3) continue;
      const owner = archiveOwner(k, taskIds);
      if (owner) out.add(owner);
    }
  } catch {
    /* 隐私模式/配额异常：退化成「都没聊过」，不影响使用 */
  }
  return out;
}

/**
 * 「聊过没」的真相源已搬到后端 `~/.uking/chats/`（2026-08-25 文件化）。
 * 这个 hook 把两边取并集：后端有档 = 聊过；localStorage 还有未迁移的旧档也算。
 * 返回 null 表示后端清单还没到（首帧），灯先按 localStorage 判断，到了再修正。
 */
function useBackendChatted(taskIds: string[]): Set<string> | null {
  const [ids, setIds] = useState<Set<string> | null>(null);
  useEffect(() => {
    let alive = true;
    invoke<string[]>("chat_archive_list", {})
      .then((arr) => { if (alive) setIds(new Set(Array.isArray(arr) ? arr : [])); })
      .catch(() => { if (alive) setIds(new Set()); });
    return () => { alive = false; };
  }, [taskIds.join("|")]);
  return ids;
}

/** 左栏功能入口（会话之外的三块）。一份数据驱动展开态和折叠 rail 两处渲染。 */
const NAV: { id: Exclude<WorkView, "chat">; label: string; hint: string; icon: typeof Users }[] = [
  // 🔴 这两条以前是**一条**：id=kanban 却叫「护照」，点进去是会话看板、护照缩在页眉一条横条里。
  // 一名两物 —— 客户点「护照」找不到护照，只能看见一块五列的会话板。现在各自一等：
  // 「护照」答**事情做到哪**（跨 AI 接力的状态），「看板」答**谁在跑**（会话生命周期）。
  { id: "passports", label: "护照", hint: "任务护照：一件事做到哪了，交给 Claude / DeepSeek / Codex 接着干", icon: ClipboardList },
  { id: "kanban", label: "看板", hint: "这台电脑上所有 AI 的会话「谁在跑 / 谁跑完 / 谁挂了」+ 定时任务", icon: LayoutDashboard },
  // 「竞技场」2026-08-11 从工作台导航摘掉（**按误点代价，不是按好不好用**）：
  // 点一次 = 六个 CLI 跑同一个任务 = **六倍 token**，而且非幂等（`arena.rs` 自己写着
  // 「一跑就烧 token 且非幂等 → 不进动作表」）。它是横向评测玩具，不是日常干活的东西 ——
  // 摆在天天要用的工作台第二格，是把全场最贵的按钮放在最顺手的位置。
  // 代码全留着（Arena.tsx + arena.rs + `arena` 视图 + --arena-test），从导航摘掉即不可达；
  // 要放回来把下面这行解开、并把 `Swords` 加回顶部 lucide-react 的 import。
  // { id: "arena", label: "竞技场", hint: "六个 CLI 同任务横向比，系统只出可观测量、质量由人打星", icon: Swords },
  { id: "experts", label: "AI 专家", hint: "挑个专家，当场在这里开会话干活", icon: Users },
  { id: "automation", label: "自动化", hint: "定时任务：到点了让 AI 自己把活干了", icon: Zap },
];

const ADD_TOOLS: { tool: string; name: string; cmd: string }[] = [
  { tool: "claude", name: "Claude Code", cmd: "claude" },
  { tool: "codex", name: "Codex", cmd: "codex" },
  { tool: "openclaw", name: "OpenClaw", cmd: "openclaw" },
  { tool: "hermes", name: "Hermes", cmd: "hermes" },
];

/**
 * 会话行**显示成什么**。
 *
 * 🔴 客户 2026-08-20：「文件夹和下面的任务对话名重复」。这不是巧合 ——
 * `types.ts` 里写着 `name: string; // 显示名（默认文件夹名）`，而项目组头显示的也是文件夹名，
 * 于是「项目 uking-mini」底下挂着三条「uking-mini」，**三行字一模一样，只能靠位置分辨**。
 *
 * 修法是**只改显示，不动存储的 `name`**：
 *   · 动存储要迁移，而且会覆盖掉用户自己改过的名字 —— 那是真正不可逆的破坏；
 *   · 重命名对话框里仍然给出原值，双击改名的行为一个字没变。
 * 判据只有一条：**这条会话的名字跟它所在项目的名字一样吗**。一样就说明它还是默认名、
 * 没携带任何信息，此时显示工具名（Claude Code / Codex …）更有用 —— 同一个项目下
 * 挂着 Claude 和 Codex 两条，这才是用户真正要区分的东西。
 * 用户一旦改过名（跟项目名不同），**永远原样显示**，不猜、不加工。
 */
function sessionLabel(t: { name?: string; tool?: string | null; dir?: string }, projName: string): string {
  const raw = t.name || dirBasename(t.dir || "");
  if (raw && raw !== projName) return raw; // 用户改过名 / 本来就不同 → 原样
  const tool = ADD_TOOLS.find((x) => x.tool === t.tool);
  return tool ? tool.name : raw;
}

/**
 * `view`/`onView` 可选：老的 OpenCodex 工作台（`workbench` tab）没有专家/自动化面板，
 * 不传就整块不渲染 —— 别为了共用组件，硬给一个点了没反应的入口。
 */
export function SessionList({ view = "chat", onView, navBadge }: {
  view?: WorkView;
  onView?: (v: WorkView) => void;
  /** 某个左栏入口上「有几件事要看」（红点数字）。**故意做成通用的**：
   *  SessionList 不该认识 automation —— 宿主算好了传进来，这里只负责画。
   *  0 / undefined = 不画。 */
  navBadge?: Partial<Record<WorkView, number>>;
} = {}) {
  const { t: tr } = useI18n();
  const { state, addTask, addSession, addWorktree, removeTask, removeProject, reorderTasks, renameTask, activate, restoreTask } =
    useWorkbench();
  // 「空文件夹要不要布置成工作台」那一问。一份实现多处用，别复制第二份弹窗。
  const { offer, node: workbenchOffer } = useWorkbenchOffer();
  // 正在重命名的会话 id + 输入框内容（测试报告 #016）。双击名字进入，回车/失焦保存，Esc 取消。
  const [renaming, setRenaming] = useState<{ id: string; text: string } | null>(null);
  const [addMenuFor, setAddMenuFor] = useState<string | null>(null);

  // 「更多」折叠（2026-08-25，学 OpenClaw 的渐进式披露）：护照/看板/专家/自动化四个低频
  // 入口默认收起，省出半栏给会话列表。正开着某个视图时收起态亮蓝点，防「状态丢了」的感觉。
  const [moreOpen, setMoreOpen] = useState(false);

  // Standby 灯的依据：哪些会话有对话历史。存档是边聊边写的，所以除了 tasks 变化，
  // 还挂在 activeId 上重算 —— 从某个会话切走时它多半刚聊过，切回列表就得亮起来。
  const legacyChatted = useMemo(
    () => chattedTaskIds(state.tasks.map((x) => x.id)),
    [state.tasks, state.activeId],
  );
  // 后端档清单（文件化后的真相源）。并集 = legacy 未迁移档 ∪ 后端现存档。
  // 后端清单没到之前（null）先按 legacy 判，到了立即修正 —— 不闪「离线」假灯。
  const backendChatted = useBackendChatted(state.tasks.map((x) => x.id));
  const chatted = useMemo(() => {
    const merged = new Set(legacyChatted);
    if (backendChatted) for (const id of backendChatted) merged.add(id);
    return merged;
  }, [legacyChatted, backendChatted]);

  /**
   * 正在干活的会话「从什么时候开始跑的」+ 每秒走一下表。
   *
   * 起点记在组件里而不是 store：它是**纯展示**（不落盘、别处不读、刷新即重置）。
   * 塞进 `Task` 会让所有读 tasks 的地方（看板、护照、后端 tasks.json）都多背一个跟
   * 业务无关的字段，为一行小灰字不值当。
   *
   * 计时器**只在真有会话在跑时才起**，一个都没跑就一秒都不 tick —— 侧栏是首屏常驻组件，
   * 让它无条件每秒重渲染是白烧电（U 盘版跑在客户的老笔记本上）。
   */
  const liveIds = useMemo(() => state.tasks.filter(isLive).map((t) => t.id), [state.tasks]);
  const liveKey = liveIds.join(",");
  const startedAt = useRef<Record<string, number>>({});
  const [, forceTick] = useState(0);
  useEffect(() => {
    const now = Date.now();
    const ids = liveKey ? liveKey.split(",") : [];
    for (const id of ids) if (!startedAt.current[id]) startedAt.current[id] = now;
    // 跑完的清掉：留着的话同一个会话下一轮会接着上一轮的秒数算，越跑越离谱
    for (const id of Object.keys(startedAt.current)) {
      if (!ids.includes(id)) delete startedAt.current[id];
    }
    if (ids.length === 0) return;
    const h = setInterval(() => forceTick((n) => n + 1), 1000);
    return () => clearInterval(h);
  }, [liveKey]);
  const now = Date.now();

  // git repo 检测缓存：projKey → boolean。首次渲染某个项目时异步检测，后续不重复。
  const [gitRepos, setGitRepos] = useState<Record<string, boolean>>({});
  const checkedDirsRef = useRef<Set<string>>(new Set());

  // worktree 输入状态
  const [worktreeInputFor, setWorktreeInputFor] = useState<string | null>(null);
  const [worktreeRepoRoot, setWorktreeRepoRoot] = useState<string | null>(null);
  const [worktreeBranch, setWorktreeBranch] = useState("");
  const [worktreeCreate, setWorktreeCreate] = useState(false);
  const [worktreeLoading, setWorktreeLoading] = useState(false);

  /** 归档单个会话。返回 true = 卡片可以移除（归档成功；或确实没聊过、没东西可归）。 */
  const archiveOne = async (id: string): Promise<boolean> => {
    const task = state.tasks.find((x) => x.id === id);
    try {
      await invoke("chat_session_archive", {
        sessionId: id,
        name: task?.name ?? "",
        dir: task?.dir || null,
      });
      return true;
    } catch (e) {
      if (String(e).includes("还没有聊天记录")) return true; // 空会话：没档可归不算失败
      // 真失败（磁盘满/权限/占用）：卡片**留在列表里** —— 历史还在活跃区，
      // 从 UI 上抹掉等于让用户以为已经归档了，实际哪都找不着。
      console.error("[archive] 归档失败，保留会话卡片:", e);
      return false;
    }
  };

  // 关闭会话 = **归档**（2026-08-25）。原来只有「留着 or 彻底删」两档，弹窗里那句
  // 「关掉就找不回来了」把不少客户吓得不敢点。现在关就是归档：
  //   · 消息档挪进 ~/.uking/chats/archived/，随时可从侧栏底部归档区恢复；
  //   · 非破坏动作，**不再弹确认** —— 弹窗只留给真正找不回来的事。
  const onDelClick = async (id: string) => {
    if (!(await archiveOne(id))) return;
    void removeTask(id);
    refreshArchived();
  };

  // 整组关闭：逐个归档，只移除归档成功的卡片。
  const onDelGroupClick = async (_projKey: string, ids: string[]) => {
    const removable: string[] = [];
    for (const id of ids) if (await archiveOne(id)) removable.push(id);
    if (removable.length > 0) void removeProject(removable);
    refreshArchived();
  };

  // ── 归档区 ─────────────────────────────────────────────────────────────
  // 后端 manifest 是真相源；archived_list 自带「文件没了的条目剔除」的对账。
  type ArchivedItem = { session_id: string; name: string; dir: string | null; message_count: number };
  const [archived, setArchived] = useState<ArchivedItem[] | null>(null);
  const [archOpen, setArchOpen] = useState(false);
  const refreshArchived = () => {
    invoke<ArchivedItem[]>("chat_archived_list", {})
      .then((arr) => setArchived(Array.isArray(arr) ? arr : []))
      .catch(() => setArchived([]));
  };
  useEffect(refreshArchived, []);

  /** 从归档区恢复：挪回活跃区 → 用原 id 原样重建任务卡片（历史自动接上）→ 刷新清单。
   *  恢复失败绝不装成功：不建卡、不切页，归档条目原样留着。 */
  const onRestoreClick = async (a: ArchivedItem) => {
    try {
      await invoke("chat_session_restore", { sessionId: a.session_id });
    } catch (e) {
      console.error("[archive] 恢复失败:", e);
      refreshArchived();
      return;
    }
    await restoreTask({ id: a.session_id, name: a.name, dir: a.dir });
    onView?.("chat");
    refreshArchived();
  };

  /** 彻底删除：唯一「找不回来」的入口，必须真问一次（fail-closed）。等后端落定再刷新 ——
   *  删成了条目消失，没删成条目还在原地，绝不提前把它从 UI 抹掉骗用户。 */
  const onPurgeClick = async (a: ArchivedItem) => {
    const okd = await askConfirm(
      tr("彻底删除「{name}」？它的 {n} 条对话记录将无法找回。", { name: a.name || a.session_id, n: a.message_count }),
    );
    if (!okd) return;
    try {
      await invoke("chat_session_purge", { sessionId: a.session_id });
    } catch (e) {
      console.error("[archive] 彻底删除失败:", e);
    }
    refreshArchived();
  };

  // 拖拽排序：dragRef 存当前拖的是「项目组」还是「组内会话」；over* 仅作落点高亮。
  // 用原生 HTML5 拖拽，不引第三方库（守体积红线）。
  const dragRef = useRef<{ kind: "group" | "session"; key: string; group?: string } | null>(null);
  const [overGroup, setOverGroup] = useState<string | null>(null);
  const [overSession, setOverSession] = useState<string | null>(null);
  const clearDrag = () => {
    dragRef.current = null;
    setOverGroup(null);
    setOverSession(null);
  };

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: tr("选择项目文件夹") });
    if (typeof dir === "string" && dir) {
      onView?.("chat");
      // 空文件夹先问一句要不要布置成工作台 —— 选完空目录什么都没有，正是他最需要答案的一秒。
      // 非空目录是他自己的项目，`offer` 会一个字都不说。
      await offer(dir);
      await addTask(dir, "manual", false);
    }
  };

  // 新建对话：在当前激活项目下开一个 claude 会话；没有激活项目则先选文件夹建项目
  const newChat = async () => {
    onView?.("chat");
    const active = state.tasks.find((t) => t.id === state.activeId);
    const dir = active?.dir;
    if (dir) {
      addSession(dir, "claude", tr("新对话"), "claude");
    } else {
      await pickFolder();
    }
  };

  /** 点会话 = 回到对话视图（否则点了半天没反应，因为专家/自动化面板盖在上面）。 */
  const openSession = (id: string) => {
    onView?.("chat");
    activate(id);
  };

  // 按项目（规范化 dir）分组；无 dir 的工具会话归到 "" 组（散会话）
  const groups = useMemo(() => {
    const m = new Map<string, Task[]>();
    for (const t of state.tasks) {
      const key = t.project ?? (t.dir ? normDir(t.dir) : "");
      const arr = m.get(key) ?? [];
      arr.push(t);
      m.set(key, arr);
    }
    return Array.from(m.entries());
  }, [state.tasks]);

  // 每当 groups 变化时，检测未查过的项目是否是 git repo
  useEffect(() => {
    for (const [, tasks] of groups) {
      const mainTask = tasks.find((t) => !t.worktree_repo);
      if (!mainTask) continue; // 全是 worktree 任务 → 已知是 git repo，跳过
      const key = normDir(mainTask.dir);
      if (checkedDirsRef.current.has(key)) continue;
      checkedDirsRef.current.add(key);
      invoke<boolean>("git_is_repo", { path: mainTask.dir })
        .then((result) => setGitRepos((prev) => ({ ...prev, [key]: result })))
        .catch(() => {});
    }
  }, [groups]);

  const doCreateWorktree = async () => {
    if (!worktreeBranch.trim() || !worktreeRepoRoot) return;
    setWorktreeLoading(true);
    try {
      await addWorktree(worktreeRepoRoot, worktreeBranch.trim(), worktreeCreate);
      setWorktreeInputFor(null);
      setWorktreeRepoRoot(null);
      setWorktreeBranch("");
    } catch (e) {
      alert(tr("创建 worktree 失败: {e}", { e: String(e) }));
    } finally {
      setWorktreeLoading(false);
    }
  };

  // 把 from 组整体挪到 to 组之前，重建扁平 id 顺序后落盘。
  const moveGroup = (fromKey: string, toKey: string) => {
    if (fromKey === toKey) return;
    const keys = groups.map(([k]) => k).filter((k) => k !== fromKey);
    const at = keys.indexOf(toKey);
    if (at < 0) return;
    keys.splice(at, 0, fromKey);
    const byKey = new Map(groups);
    const ids: string[] = [];
    for (const k of keys) for (const t of byKey.get(k) ?? []) ids.push(t.id);
    reorderTasks(ids);
  };

  // 组内把 from 会话挪到 to 会话之前；其它组顺序原样保留。
  const moveSession = (fromId: string, toId: string, projKey: string) => {
    if (fromId === toId) return;
    const ids: string[] = [];
    for (const [k, tasks] of groups) {
      if (k !== projKey) {
        for (const t of tasks) ids.push(t.id);
        continue;
      }
      const moved = tasks.find((t) => t.id === fromId);
      const arr = tasks.filter((t) => t.id !== fromId);
      const at = arr.findIndex((t) => t.id === toId);
      if (moved) arr.splice(at < 0 ? arr.length : at, 0, moved);
      for (const t of arr) ids.push(t.id);
    }
    reorderTasks(ids);
  };

  // 🔴 指针拖拽（不用 HTML5 draggable）：u-king 的 tauri.conf 设了 dragDropEnabled:true（为让拖 OS 文件
  // 进来拿真实路径给作图/视频/终端用），HTML5 拖拽被 Tauri OS 层拦截失效 → 排序改用 pointer 事件实现。
  // 阈值 5px 才算拖动，小位移当点击（会话行的 activate 不受影响）。落点靠 data-drop-* 属性 + elementFromPoint。
  const beginDrag = (e: React.PointerEvent, kind: "group" | "session", key: string, group?: string) => {
    const sx = e.clientX, sy = e.clientY;
    let active = false;
    const move = (ev: PointerEvent) => {
      if (!active) {
        if (Math.abs(ev.clientX - sx) + Math.abs(ev.clientY - sy) < 5) return;
        active = true;
        dragRef.current = { kind, key, group };
      }
      const el = document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null;
      if (kind === "group") {
        setOverGroup(el?.closest<HTMLElement>("[data-drop-group]")?.dataset.dropGroup ?? null);
      } else {
        const se = el?.closest<HTMLElement>("[data-drop-session]");
        setOverSession(se?.dataset.dropProjkey === group ? (se?.dataset.dropSession ?? null) : null);
      }
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (active) {
        const el = document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null;
        const d = dragRef.current;
        if (d?.kind === "group") {
          const to = el?.closest<HTMLElement>("[data-drop-group]")?.dataset.dropGroup;
          if (to) moveGroup(d.key, to);
        } else if (d?.kind === "session") {
          const se = el?.closest<HTMLElement>("[data-drop-session]");
          const to = se?.dataset.dropSession;
          if (to && se?.dataset.dropProjkey === d.group) moveSession(d.key, to, d.group!);
        }
      }
      clearDrag();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  // 会话栏宽度 + 折叠：本地持久化（不进 tasks.json，纯视图偏好）。拖右边缘调宽（180~460px），
  // 双击边缘或点收起按钮 = 折叠成窄条，把地方全让给右侧终端；再点展开恢复原宽。
  const MIN_W = 180, MAX_W = 460;
  const [width, setWidth] = useState<number>(() => {
    const v = parseInt(localStorage.getItem("uworkspace.sidebar.width") || "", 10);
    if (v >= MIN_W && v <= MAX_W) return v; // 用户手调过 —— 他的选择永远赢，别按窗口大小改他的偏好
    // ★ 窄屏（≤1280，见 useViewport 的推导）只改**默认值**：主侧栏 208 + 这栏 230 + 内边距 24
    // 已经固定吃掉 462px，1280 下留给对话的 818px 正好卡在内容上限 820 的擦边处。
    // **故意不做「窄屏自动折叠」**：折叠态会把会话列表整个藏起来，而在目标机型
    // 1366×768 上横向根本不紧（904px 够摆）—— 为一个不存在的问题藏掉客户的会话，
    // 代价比收益大。这里只是把默认从 230 收到 190，用户随时拖回去，且拖过一次就记住。
    return window.innerWidth <= 1280 ? 190 : 230;
  });
  const [collapsed, setCollapsed] = useState<boolean>(
    () => localStorage.getItem("uworkspace.sidebar.collapsed") === "1",
  );
  /**
   * 让步链的**第一顺位**：终端排不开 TUI 时，先收的是这一栏（见 `lib/yieldChain.ts`）。
   *
   * 🔴 只影响**渲染**，不碰上面那个 `collapsed` 偏好 —— 窗口一变宽，用户原来的展开态
   * 和他拖过的宽度原样回来。要是顺手把 `collapsed` 设成 true，用户就再也回不到
   * 他自己选的那个样子了，而他根本没做过那个决定。
   *
   * 前面那段注释说「**故意不做窄屏自动折叠**：横向根本不紧，为一个不存在的问题藏掉客户的会话」
   * —— 那条依然成立，这里也没有推翻它：**触发条件不是窗口变窄，是终端真的排不开了**。
   * 一个是猜，一个是量出来的。
   */
  const yieldLv = useYieldLevel();
  const railed = collapsed || yieldLv >= YIELD_SESSION_BAR;
  const widthRef = useRef(width);
  widthRef.current = width;
  const toggleCollapsed = () => {
    // 用户亲手点了 —— 让步链就此让开（同本文件「他的选择永远赢」那条）。
    const yieldOnly = !collapsed && yieldLv >= YIELD_SESSION_BAR;
    overrideYield();
    // 🔴 是让步把它收起来的（用户偏好本来就是展开）：推翻让步**就是**展开这一下，
    // 别再去翻他自己的偏好 —— 那会把 collapsed 翻成 true，按下「展开」反而收得更死。
    if (yieldOnly) return;
    setCollapsed((c) => {
      const n = !c;
      try { localStorage.setItem("uworkspace.sidebar.collapsed", n ? "1" : "0"); } catch { /* ignore */ }
      return n;
    });
  };
  // 右边缘拖拽调宽：pointer 事件（独立元素，和会话/项目排序拖拽 beginDrag 互不干扰）
  const beginResize = (e: React.PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = widthRef.current;
    const move = (ev: PointerEvent) => {
      setWidth(Math.min(MAX_W, Math.max(MIN_W, startW + (ev.clientX - startX))));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      try { localStorage.setItem("uworkspace.sidebar.width", String(widthRef.current)); } catch { /* ignore */ }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  // 折叠态：窄条 rail —— 只留展开 + 新建对话 + 新建项目三个图标，把空间全让给终端
  if (railed) {
    return (
      <aside className="w-11 shrink-0 flex flex-col items-center gap-1 py-2 border-r border-white/[0.06] bg-bg-1 min-h-0">
        <button
          onClick={toggleCollapsed}
          title={
            // 让步收起来的要说清是**为什么**收的 —— 不然用户只看到自己没点过的东西自己合上了。
            !collapsed && yieldLv >= YIELD_SESSION_BAR
              ? tr("窗口太窄，已自动让位给终端 —— 点这里展开（窗口拉宽会自己还原）")
              : liveIds.length
                ? tr("展开会话栏（{n} 个会话正在干活）", { n: liveIds.length })
                : tr("展开会话栏")
          }
          className="relative w-8 h-8 grid place-items-center rounded text-ink-3 hover:text-ink-0 hover:bg-white/[0.06]"
        >
          <ChevronsRight size={16} />
          {/* 🔴 收起态原本把会话状态**整个**藏掉：后台跑着的活、跑挂的活，在这条 44px 的窄栏上
              一个字都没有。而「把地方让给终端」正是长任务跑起来后最常按的一下 —— 恰恰是那时候
              最需要知道后面还有没有人在干活。这里只回答「有没有」，要看是谁点开就行了。 */}
          {/* 🔴 定位套在外层 span 上，不写成 `dot dot-live absolute`：`.dot-live` 自己要
              `position: relative`（脉冲环靠它当定位父），而它和 Tailwind 的 `absolute`
              同在 utilities 层、特异性相同 —— 谁赢取决于 Tailwind 把自定义 utilities 排在
              生成的 utilities 前面还是后面。**能不赌就不赌。** */}
          {liveIds.length > 0 && (
            <span className="absolute top-1 right-1">
              <span className="dot dot-live" />
            </span>
          )}
        </button>
        <button
          onClick={newChat}
          title={tr("新建对话")}
          className="w-8 h-8 grid place-items-center rounded text-accent-400 hover:bg-accent/[0.16]"
        >
          <MessageSquarePlus size={16} />
        </button>
        <button
          onClick={pickFolder}
          title={tr("新建项目（选文件夹）")}
          className="w-8 h-8 grid place-items-center rounded text-ink-3 hover:text-ink-0 hover:bg-white/[0.06]"
        >
          <FolderPlus size={15} />
        </button>
        {onView && <div className="w-6 h-px bg-white/[0.08] my-1" />}
        {onView && NAV.map((n) => (
          <button
            key={n.id}
            onClick={() => onView(n.id)}
            title={tr(n.label)}
            className={
              "relative w-8 h-8 grid place-items-center rounded " +
              (view === n.id ? "text-accent-400 bg-accent/[0.14]" : "text-ink-3 hover:text-ink-0 hover:bg-white/[0.06]")
            }
          >
            <n.icon size={15} />
            {/* 收起态只画一个点：这一栏就 32px 宽，数字画上去也看不清 */}
            {!!navBadge?.[n.id] && <span className="absolute top-1 right-1 w-1.5 h-1.5 rounded-full bg-danger-500" />}
          </button>
        ))}
        {/* 收起态照样能点「新建项目」，弹窗就得跟着渲染 —— 少挂一处 = 那条路上按钮点了没反应 */}
        {workbenchOffer}
      </aside>
    );
  }

  return (
    <aside
      style={{ width }}
      className="relative shrink-0 flex flex-col border-r border-white/[0.06] bg-bg-1 min-h-0"
    >
      {/* 顶部品牌条 + 收起按钮 */}
      <div className="px-3 pt-3 pb-1 shrink-0 flex items-center justify-between">
        <span className="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">{tr("会话")}</span>
        <button
          onClick={toggleCollapsed}
          title={tr("收起会话栏（把地方让给终端）")}
          className="inline-flex items-center justify-center w-5 h-5 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
        >
          <ChevronsLeft size={14} />
        </button>
      </div>

      {/* 新建对话（主）+ 新建项目（次）—— Codex 式入口 */}
      <div className="px-2.5 pt-2 pb-1.5 shrink-0 space-y-1">
        <button
          onClick={newChat}
          className="w-full inline-flex items-center gap-2 h-8 px-2.5 rounded-card bg-accent/[0.14] text-accent-400 hover:bg-accent/[0.20] text-[12.5px] font-medium"
        >
          <MessageSquarePlus size={14} />
          {tr("新建对话")}
        </button>
        <button
          onClick={pickFolder}
          className="w-full inline-flex items-center gap-2 h-7 px-2.5 rounded-card text-ink-3 hover:bg-white/[0.04] text-[12px]"
          title={tr("选择文件夹新建项目")}
        >
          <FolderPlus size={13} />
          {tr("新建项目（选文件夹）")}
        </button>
      </div>
      {/* AI 专家 / 自动化 —— 和会话同一根左栏（WorkBuddy 式）：挑专家、配定时任务都不用离开工作台 */}
      {onView && (
      <div className="px-2.5 pt-1 pb-2 shrink-0 space-y-0.5 border-t border-b border-white/[0.06] mt-1 mb-1">
        {/* 🔴 这一小行标题是分层用的，不是装饰。这一列里挤着**三种不同的东西**：
            上面是动作（新建对话 / 新建项目）、这里是别的视图（护照 / 看板 / 专家 / 自动化）、
            下面是状态（已打开的项目）。三种平铺成一串没有间隔的按钮时，
            人只能靠逐个点进去才知道哪个会**离开当前对话**——而「已打开的项目」那行早就有标题了，
            缺的只是中间这一层。上下各加一条分隔线，让三段各自成块。 */}
        {/* 「更多」渐进式披露（2026-08-25）：低频视图默认收起。开着非聊天视图时收起，
            用蓝点提示「你现在不在会话里」，别让切换状态悄悄丢在折叠里。 */}
        <button
          onClick={() => setMoreOpen((v) => !v)}
          className="w-full inline-flex items-center gap-2 h-7 px-2.5 rounded-card text-[12px] text-ink-3 hover:bg-white/[0.04] hover:text-ink-1"
          title={tr("护照 / 看板 / AI 专家 / 自动化")}
        >
          {moreOpen ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
          <span className="flex-1 text-left">{tr("更多")}</span>
          {!moreOpen && view !== "chat" && (
            <span className="w-1.5 h-1.5 rounded-full bg-accent-400" title={tr("当前不在这个视图上")} />
          )}
          {!moreOpen && !!navBadge && Object.values(navBadge).some(Boolean) && (
            <span
              className="min-w-[16px] h-4 px-1 grid place-items-center rounded-full bg-danger-500/90 text-white text-[10px] font-medium"
              title={tr("有上次没跑成的任务")}
            >
              {Object.values(navBadge).reduce<number>((s, v) => s + (v ?? 0), 0)}
            </span>
          )}
        </button>
        {moreOpen &&
          NAV.map((n) => (
          <button
            key={n.id}
            onClick={() => onView(view === n.id ? "chat" : n.id)}
            title={tr(n.hint)}
            className={
              "w-full inline-flex items-center gap-2 h-7 px-2.5 rounded-card text-[12px] " +
              (view === n.id
                ? "bg-accent/[0.14] text-accent-400 font-medium"
                : "text-ink-3 hover:bg-white/[0.04] hover:text-ink-1")
            }
          >
            <n.icon size={13} />
            {tr(n.label)}
            {/* 「有几个跑挂了」。展开态给数字：1 个和 5 个是两种严重程度 */}
            {!!navBadge?.[n.id] && (
              <span
                className="ml-auto min-w-[16px] h-4 px-1 grid place-items-center rounded-full bg-danger-500/90 text-white text-[10px] font-medium"
                title={tr("有 {n} 个上次没跑成", { n: navBadge[n.id] ?? 0 })}
              >
                {navBadge[n.id]}
              </span>
            )}
          </button>
        ))}
      </div>
      )}

      <div className="px-3 pb-1 shrink-0 text-[11px] text-ink-5">{tr("已打开的项目")}</div>

      <div className="flex-1 overflow-y-auto py-1.5 min-h-0">
        {state.tasks.length === 0 ? (
          <div className="px-3 py-6 text-center text-ink-4 text-[12px] leading-relaxed">
            {tr("还没有项目。")}
            <br />
            {tr("点「新建」选一个文件夹，")}
            <br />
            {tr("在里面让多个 AI 一起干活。")}
          </div>
        ) : (
          groups.map(([projKey, tasks]) => {
            // 找主仓库任务（无 worktree_repo）和任意一个 worktree 任务
            const nonWorktreeTask = tasks.find((t) => !t.worktree_repo);
            const anyWorktreeTask = tasks.find((t) => !!t.worktree_repo);
            // 用于显示项目名的 dir：优先主仓库 dir，其次从 worktree_repo 取
            const projDisplayDir = nonWorktreeTask?.dir ?? anyWorktreeTask?.worktree_repo ?? tasks[0].dir;
            const projName = projKey ? dirBasename(projDisplayDir) : tr("未绑定文件夹");
            // 用于创建新 worktree 的 git 根目录
            const repoRoot = nonWorktreeTask?.dir ?? anyWorktreeTask?.worktree_repo ?? null;
            // 有 worktree 任务 → 已知是 git repo；否则查检测缓存
            const isGitProject =
              !!anyWorktreeTask || (repoRoot ? !!gitRepos[normDir(repoRoot)] : false);

            /**
             * 这个**文件夹**里有没有人在干活 / 有没有跑挂的。
             *
             * 🔴 项目组头以前一个状态都不报。而客户扫左栏是**先看文件夹再看会话**的，
             * 文件夹一多（或某个组的会话被滚出视口）就等于没有提示 —— 「哪个项目在跑」
             * 这个最常问的问题，得靠一个个点开看。汇总放在组头上才答得了它。
             *
             * 跑挂的只在**没人在跑**时才报：正在跑的是当下、跑挂的是上一轮，同时亮两个灯
             * 会让人以为「一边跑一边在报错」。
             */
            const liveInGroup = tasks.filter(isLive).length;
            const errInGroup = tasks.filter((t) => t.status === "error").length;

            // 单会话项目并一行（学 ChatGPT/Codex，2026-08-25 客户拍板）：项目下只有一条会话、
            // 且显示名与项目名相同时，「文件夹头 + 同名子行」两行堆叠纯属冗余 —— 组头隐藏，
            // 会话行升级为项目行（加粗），组头的操作按钮原样挂在它 hover 区。
            const mergeSingle =
              !!projKey &&
              tasks.length === 1 &&
              !tasks[0].worktree_repo &&
              sessionLabel(tasks[0], projName) === projName;

            return (
              <div
                key={projKey || "_loose"}
                className="mb-1.5"
                data-drop-group={projKey}
              >
                {/* 项目组头：拖拽把手重排顺序（指针拖拽，见 beginDrag）；垃圾桶整组删除。
                    单会话合并行时整块不渲染 —— 名字和操作都已在那条会话行上。 */}
                {!mergeSingle && (
                <div
                  onPointerDown={(e) => beginDrag(e, "group", projKey)}
                  className={
                    // 🔴 层级原来是**反的**（2026-08-20 客户：「区分度不够，一排在一起，不好点」）：
                    //    项目组头 11px / text-ink-3（灰），底下的会话行却是 12px / text-ink-2（更深）——
                    //    **父级比子级还轻**，于是整栏读起来是一排平的，眼睛找不到分组的边。
                    //    改成：组头更大更深更粗（12.5px / ink-1 / semibold）+ 上方留白拉开组间距。
                    //    会话行不动（它已经够醒目了）—— 修的是「父级太轻」，不是「子级太重」。
                    "group flex items-center gap-1 px-2.5 pt-3 pb-1 mt-0.5 text-[12.5px] font-semibold text-ink-1 select-none cursor-grab active:cursor-grabbing border-t " +
                    (overGroup === projKey ? "border-accent" : "border-transparent")
                  }
                >
                  <GripVertical
                    size={11}
                    className="shrink-0 -ml-1 text-ink-5 opacity-0 group-hover:opacity-100 transition-opacity"
                  />
                  <span className="flex-1 min-w-0 truncate font-medium" title={projDisplayDir}>
                    {projName}
                  </span>
                  {/* 文件夹级活动指示。**常显**（不跟着 group-hover 走）—— 它是要在余光里被看到的，
                      藏在 hover 后面就等于没有。放在名字和那排操作按钮之间，按钮出现时不挤它。 */}
                  {liveInGroup > 0 ? (
                    <span
                      className="inline-flex items-center gap-1 shrink-0 text-[10px] text-success-400 tabular-nums"
                      title={tr("这个项目下有 {n} 个会话正在干活", { n: liveInGroup })}
                    >
                      <span className="dot dot-live" />
                      {liveInGroup > 1 && liveInGroup}
                    </span>
                  ) : errInGroup > 0 ? (
                    <span
                      className="inline-flex items-center gap-1 shrink-0 text-[10px] text-danger-400/90 tabular-nums"
                      title={tr("这个项目下有 {n} 个会话上一轮出错", { n: errInGroup })}
                    >
                      <span className="dot dot-error" />
                      {errInGroup > 1 && errInGroup}
                    </span>
                  ) : null}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onDelGroupClick(
                        projKey,
                        tasks.map((t) => t.id),
                      );
                    }}
                    className="inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all opacity-0 group-hover:opacity-100 text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
                    title={tr("归档整个项目下的会话（记录保留，可从底部归档区恢复）")}
                  >
                    <Trash2 size={12} />
                  </button>
                  {/* worktree 按钮：仅 git 项目显示 */}
                  {isGitProject && repoRoot && projKey && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        if (worktreeInputFor === projKey) {
                          setWorktreeInputFor(null);
                          setWorktreeRepoRoot(null);
                        } else {
                          setWorktreeInputFor(projKey);
                          setWorktreeRepoRoot(repoRoot);
                          setWorktreeBranch("");
                          setWorktreeCreate(false);
                        }
                      }}
                      className={
                        "inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all " +
                        (worktreeInputFor === projKey
                          ? "opacity-100 text-accent-400 bg-accent/[0.12]"
                          : "opacity-0 group-hover:opacity-100 text-ink-4 hover:text-accent-400 hover:bg-white/[0.06]")
                      }
                      title={tr("新建 worktree（并行在另一个分支上工作）")}
                    >
                      <GitBranch size={12} />
                    </button>
                  )}
                  {projKey && (
                    <div className="relative">
                      <button
                        onClick={() => setAddMenuFor(addMenuFor === projKey ? null : projKey)}
                        className="inline-flex items-center justify-center w-5 h-5 rounded text-ink-4 hover:text-accent-400 hover:bg-white/[0.06]"
                        title={tr("在此项目新开一个 AI 会话")}
                      >
                        <Plus size={12} />
                      </button>
                      {addMenuFor === projKey && (
                        <div className="absolute right-0 top-6 z-30 w-36 rounded-card border border-white/[0.10] bg-bg-2 shadow-card p-1">
                          {ADD_TOOLS.map((a) => (
                            <button
                              key={a.tool}
                              onClick={() => {
                                addSession(projDisplayDir, a.tool, a.name, a.cmd);
                                setAddMenuFor(null);
                              }}
                              className="w-full text-left px-2 py-1.5 rounded text-[12px] text-ink-2 hover:bg-white/[0.05]"
                            >
                              {a.name}
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
                )}
                {/* 单会话项目并一行（2026-08-25，学 ChatGPT/Codex）：项目下只有一个会话、
                    且显示名跟项目名相同时，不再渲染「文件夹头 + 同名子行」两行堆叠 ——
                    直接把会话行当项目行用。功能一个不少：组头那排操作按钮（归档/worktree/+）
                    挂在会话行的 hover 区；「+ 新开 AI 会话」的弹出菜单照常渲染；
                    worktree 输入行也不动。 */}
                {(() => {
                  if (tasks.length !== 1 || !projKey) return null;
                  const only = tasks[0];
                  if (only.worktree_repo) return null; // worktree 行有自己的 ⎇ 名字，不合
                  if (sessionLabel(only, projName) !== projName) return null;
                  const on = state.activeId === only.id;
                  const live = isLive(only);
                  return (
                    <div
                      data-drop-session={only.id}
                      data-drop-projkey={projKey}
                      onClick={() => openSession(only.id)}
                      onPointerDown={(e) => beginDrag(e, "session", only.id, projKey)}
                      className={
                        "group flex items-center gap-1 mx-1.5 mt-2 pl-3 pr-1.5 py-1.5 rounded-card cursor-pointer select-none " +
                        (on ? "bg-accent/[0.10]" : "hover:bg-white/[0.03]")
                      }
                      title={only.dir}
                    >
                      <span className={"dot shrink-0 " + statusDot(only, chatted.has(only.id))} title={tr(statusTitle(only, chatted.has(only.id)))} />
                      {renaming && renaming.id === only.id ? (
                        <input
                          autoFocus
                          value={renaming.text}
                          onClick={(e) => e.stopPropagation()}
                          onPointerDown={(e) => e.stopPropagation()}
                          onChange={(e) => setRenaming({ id: only.id, text: e.target.value })}
                          onBlur={() => {
                            void renameTask(only.id, renaming.text);
                            setRenaming(null);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              void renameTask(only.id, renaming.text);
                              setRenaming(null);
                            } else if (e.key === "Escape") {
                              setRenaming(null);
                            }
                          }}
                          className="flex-1 min-w-0 h-5 px-1 rounded bg-black/20 border border-accent/50 text-[12.5px] font-semibold text-ink-0 outline-none"
                        />
                      ) : (
                        <span
                          onDoubleClick={(e) => {
                            e.stopPropagation();
                            setRenaming({ id: only.id, text: only.name || dirBasename(only.dir) });
                          }}
                          title={tr("双击可重命名")}
                          className="flex-1 min-w-0 truncate text-[12.5px] font-semibold text-ink-1"
                        >
                          {projName}
                        </span>
                      )}
                      {live && (
                        <span className="shrink-0 text-[10px] text-success-400/90 tabular-nums" title={tr("这一轮已经跑了多久")}>
                          {elapsedLabel(now - (startedAt.current[only.id] ?? now))}
                        </span>
                      )}
                      {/* 组头的整组归档按钮：单会话时归这一个就是归全部，语义相同。
                          worktree / + 菜单原样挂上（+ 弹出菜单依赖 projDisplayDir）。 */}
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          onDelGroupClick(projKey, [only.id]);
                        }}
                        className="inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all opacity-0 group-hover:opacity-100 text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
                        title={tr("归档这个会话（聊天记录保留，可从底部归档区恢复）")}
                      >
                        <Trash2 size={12} />
                      </button>
                      {isGitProject && repoRoot && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            if (worktreeInputFor === projKey) {
                              setWorktreeInputFor(null);
                              setWorktreeRepoRoot(null);
                            } else {
                              setWorktreeInputFor(projKey);
                              setWorktreeRepoRoot(repoRoot);
                              setWorktreeBranch("");
                              setWorktreeCreate(false);
                            }
                          }}
                          className={
                            "inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all " +
                            (worktreeInputFor === projKey
                              ? "opacity-100 text-accent-400 bg-accent/[0.12]"
                              : "opacity-0 group-hover:opacity-100 text-ink-4 hover:text-accent-400 hover:bg-white/[0.06]")
                          }
                          title={tr("新建 worktree（并行在另一个分支上工作）")}
                        >
                          <GitBranch size={12} />
                        </button>
                      )}
                      <div className="relative">
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setAddMenuFor(addMenuFor === projKey ? null : projKey);
                          }}
                          className="inline-flex items-center justify-center w-5 h-5 rounded text-ink-4 hover:text-accent-400 hover:bg-white/[0.06]"
                          title={tr("在此项目新开一个 AI 会话")}
                        >
                          <Plus size={12} />
                        </button>
                        {addMenuFor === projKey && (
                          <div className="absolute right-0 top-6 z-30 w-36 rounded-card border border-white/[0.10] bg-bg-2 shadow-card p-1">
                            {ADD_TOOLS.map((a) => (
                              <button
                                key={a.tool}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  addSession(projDisplayDir, a.tool, a.name, a.cmd);
                                  setAddMenuFor(null);
                                }}
                                className="w-full text-left px-2 py-1.5 rounded text-[12px] text-ink-2 hover:bg-white/[0.05]"
                              >
                                {a.name}
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  );
                })()}
                {/* worktree 分支输入行（内联展开，不弹新窗口） */}
                {worktreeInputFor === projKey && (
                  <div className="mx-1.5 mb-1 flex items-center gap-1 px-2 py-1 rounded-card bg-white/[0.04] border border-white/[0.08]">
                    <GitBranch size={11} className="shrink-0 text-accent-400" />
                    <input
                      autoFocus
                      value={worktreeBranch}
                      onChange={(e) => setWorktreeBranch(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void doCreateWorktree();
                        if (e.key === "Escape") setWorktreeInputFor(null);
                      }}
                      placeholder={tr("分支名（回车确认）")}
                      className="flex-1 min-w-0 bg-transparent text-[11.5px] text-ink-1 placeholder:text-ink-5 outline-none"
                    />
                    <label className="flex items-center gap-1 text-[10.5px] text-ink-4 shrink-0 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={worktreeCreate}
                        onChange={(e) => setWorktreeCreate(e.target.checked)}
                        className="accent-accent-400"
                      />
                      {tr("新建")}
                    </label>
                    <button
                      onClick={() => void doCreateWorktree()}
                      disabled={!worktreeBranch.trim() || worktreeLoading}
                      className="text-[11px] px-1 text-accent-400 hover:text-accent-300 disabled:opacity-40 shrink-0"
                    >
                      {worktreeLoading ? "…" : "✓"}
                    </button>
                    <button
                      onClick={() => {
                        setWorktreeInputFor(null);
                        setWorktreeRepoRoot(null);
                      }}
                      className="inline-flex items-center justify-center w-4 h-4 rounded text-ink-4 hover:text-ink-2 shrink-0"
                    >
                      <X size={10} />
                    </button>
                  </div>
                )}

                {/* 该项目的会话。单会话合并行时跳过 —— 它已经以项目行的样子渲染在上面 */}
                {tasks.map((t) => {
                  if (mergeSingle) return null;
                  const on = state.activeId === t.id;
                  const live = isLive(t);
                  return (
                    <div
                      key={t.id}
                      data-drop-session={t.id}
                      data-drop-projkey={projKey}
                      onClick={() => openSession(t.id)}
                      onPointerDown={(e) => beginDrag(e, "session", t.id, projKey)}
                      className={
                        "group flex items-center gap-2 mx-1.5 mb-0.5 pl-3 pr-1.5 py-1.5 rounded-card cursor-pointer select-none border-l-2 border-t-2 " +
                        (overSession === t.id ? "border-t-accent " : "border-t-transparent ") +
                        (on ? "bg-accent/[0.10] border-l-accent" : "border-l-transparent hover:bg-white/[0.03]")
                      }
                      title={t.dir}
                    >
                      <span
                        className={"dot shrink-0 " + statusDot(t, chatted.has(t.id))}
                        title={tr(statusTitle(t, chatted.has(t.id)))}
                      />
                      {/* 🔴 必须先判 `renaming` 非空，不能只写 `renaming?.id === t.id`：
                          没在改名时 `renaming?.id` 是 undefined，万一某个任务的 `id` 也是
                          undefined，这句就成了 `undefined === undefined` = true，
                          接着读 `renaming.text` 直接 TypeError —— 整个 App 掉进错误边界，
                          左栏是首屏就渲染的，等于白屏。真实数据里 id 总在，所以这是颗
                          不响的雷；但「不响」靠的是别处的数据规矩，不是这里的判断本身。 */}
                      {renaming && renaming.id === t.id ? (
                        // 重命名输入框：拦掉 click/pointerdown，否则会触发选中会话和拖拽
                        <input
                          autoFocus
                          value={renaming.text}
                          onClick={(e) => e.stopPropagation()}
                          onPointerDown={(e) => e.stopPropagation()}
                          onChange={(e) => setRenaming({ id: t.id, text: e.target.value })}
                          onBlur={() => {
                            void renameTask(t.id, renaming.text);
                            setRenaming(null);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              void renameTask(t.id, renaming.text);
                              setRenaming(null);
                            } else if (e.key === "Escape") {
                              setRenaming(null); // 原名不动
                            }
                          }}
                          className="flex-1 min-w-0 h-5 px-1 rounded bg-black/20 border border-accent/50 text-[12.5px] text-ink-0 outline-none"
                        />
                      ) : (
                        <span
                          onDoubleClick={(e) => {
                            e.stopPropagation();
                            setRenaming({ id: t.id, text: t.name || dirBasename(t.dir) });
                          }}
                          title={tr("双击可重命名")}
                          className={"flex-1 min-w-0 truncate text-[12.5px] " + (on ? "text-ink-0" : "text-ink-1")}
                        >
                          {t.worktree_branch
                            ? `⎇ ${t.worktree_branch}`
                            : sessionLabel(t, projName)}
                        </span>
                      )}
                      {/* 跑了多久。**只在跑的时候占位**，闲下来就消失 —— 常驻一个 00s 会让
                          静止的列表看起来也在动，那正是我们要避免的噪声。 */}
                      {live && (
                        <span
                          className="shrink-0 text-[10px] text-success-400/90 tabular-nums"
                          title={tr("这一轮已经跑了多久")}
                        >
                          {elapsedLabel(now - (startedAt.current[t.id] ?? now))}
                        </span>
                      )}
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          onDelClick(t.id);
                        }}
                        className="inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all opacity-0 group-hover:opacity-100 text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
                        title={tr("归档这个会话（聊天记录保留，可从底部归档区恢复）")}
                      >
                        <X size={12} />
                      </button>
                    </div>
                  );
                })}
              </div>
            );
          })
        )}
      </div>

      {/* 归档区：关掉的会话都在这，可恢复、可彻底删。有存货才显示，默认折叠。 */}
      {(archived?.length ?? 0) > 0 && (
        <div className="shrink-0 border-t border-white/[0.06]">
          <button
            onClick={() => setArchOpen((v) => !v)}
            className="w-full flex items-center gap-1.5 px-3 py-2 text-[11px] text-ink-4 hover:text-ink-2 transition-colors"
            title={tr("已归档的会话。归档只是收起来，随时能恢复")}
          >
            {archOpen ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
            <Archive size={12} />
            <span className="flex-1 text-left">{tr("已归档会话")}</span>
            <span className="tabular-nums">{archived!.length}</span>
          </button>
          {archOpen && (
            <div className="pb-1.5 max-h-44 overflow-y-auto">
              {archived!.map((a) => (
                <div
                  key={a.session_id}
                  className="mx-2 mb-0.5 rounded px-2 py-1.5 flex items-center gap-2 hover:bg-white/[0.05]"
                >
                  <Archive size={11} className="shrink-0 text-ink-5" />
                  <span
                    className="flex-1 min-w-0 truncate text-[12px] text-ink-3"
                    title={a.dir || a.session_id}
                  >
                    {a.name || a.session_id}
                  </span>
                  <span className="shrink-0 text-[10px] text-ink-5 tabular-nums">
                    {tr("{n} 条", { n: a.message_count })}
                  </span>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void onRestoreClick(a);
                    }}
                    className="inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all text-ink-4 hover:text-accent-400 hover:bg-white/[0.08]"
                    title={
                      a.dir
                        ? tr("恢复到「{dir}」，聊天记录接上继续聊", { dir: dirBasename(a.dir) })
                        : tr("恢复这个会话（原项目文件夹已不记得了，恢复为未绑定会话）")
                    }
                  >
                    <ArchiveRestore size={12} />
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void onPurgeClick(a);
                    }}
                    className="inline-flex items-center justify-center w-5 h-5 rounded shrink-0 transition-all text-ink-4 hover:text-danger-400 hover:bg-white/[0.08]"
                    title={tr("彻底删除（将无法找回）")}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 底部：品牌 + 版本号 */}
      <div className="px-3 py-2 border-t border-white/[0.06] shrink-0 flex items-center justify-between">
        <div className="flex items-center gap-2 text-[11px] text-ink-5">
          <Zap size={12} />
          {tr("U-Workspace")}
        </div>
        <span className="text-[10px] font-mono text-ink-5 px-1.5 py-0.5 rounded bg-white/[0.04]" title={tr("U-Workspace 版本")}>
          v{__APP_VERSION__}
        </span>
      </div>

      {/* 右边缘拖拽条：拖动调宽，双击收起。骑在右边框上（往右探出一半便于抓取） */}
      <div
        onPointerDown={beginResize}
        onDoubleClick={toggleCollapsed}
        title={tr("拖动调整会话栏宽度 · 双击收起")}
        className="absolute top-0 right-0 bottom-0 w-1.5 translate-x-1/2 z-20 cursor-col-resize hover:bg-accent/50 transition-colors"
      />
      {workbenchOffer}
    </aside>
  );
}
