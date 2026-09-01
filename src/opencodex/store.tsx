/**
 * 工作台状态 —— useReducer + Context（不引 zustand，守体积红线）。
 *
 * 持久化：任务列表落 ~/.opencodex/tasks.json（后端 tasks.rs）。面板布局只在内存，不落盘。
 * 任务来源（应用内选文件夹 / --open-dir 透传）统一经 addTask 写进同一份 tasks.json。
 */
import { createContext, useContext, useEffect, useReducer, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PanelLayout, RightKind, Task, TaskSource, TaskStatus } from "./types";
import { dirBasename, normDir, taskIdFromDir } from "./types";
import type { Expert } from "./experts";

type State = {
  tasks: Task[];
  activeId: string | null;
  panels: Record<string, PanelLayout>; // sessionId -> 三区布局
  loaded: boolean;
};

type Action =
  | { type: "load"; tasks: Task[] }
  | { type: "upsert"; task: Task }
  | { type: "status"; id: string; status: TaskStatus }
  | { type: "remove"; id: string }
  | { type: "removeMany"; ids: string[] }
  | { type: "reorder"; ids: string[] }
  | { type: "activate"; id: string }
  | { type: "setRight"; id: string; kind: RightKind }
  | { type: "toggleRight"; id: string; open?: boolean }
  | { type: "setRatio"; id: string; ratio: number }
  | { type: "toggleDrawer"; id: string; open?: boolean }
  | { type: "setDrawerHeight"; id: string; h: number };

const DEFAULT_LAYOUT: PanelLayout = {
  rightKind: "files", // 主区永远是终端；滑出层默认指向文件（点顶栏才滑出）
  rightOpen: false, // 默认全宽终端，文件/浏览器点顶栏才从右滑出
  rightRatio: 0.5,
  drawerOpen: false,
  drawerHeight: 280,
};

/** 终端已是主区，所有会话默认全宽终端，不自动滑出任何东西。 */
function layoutFor(_task: Task): PanelLayout {
  return { ...DEFAULT_LAYOUT };
}

/** 改某会话布局的某个字段（统一模式）。 */
function patchLayout(state: State, id: string, patch: Partial<PanelLayout>): State {
  const cur = state.panels[id] ?? DEFAULT_LAYOUT;
  return { ...state, panels: { ...state.panels, [id]: { ...cur, ...patch } } };
}

function reducer(state: State, a: Action): State {
  switch (a.type) {
    case "load": {
      const panels = { ...state.panels };
      for (const t of a.tasks) if (!panels[t.id]) panels[t.id] = layoutFor(t);
      // 有持久化会话时自动选中第一个（最新），否则右侧对话区 activeId=null → 全 display:none
      // → 进 U-Workspace 是一片空白（客户「不好看」的元凶）。空列表交给 UWorkspace 的自动建会话。
      return { ...state, tasks: a.tasks, panels, loaded: true, activeId: state.activeId ?? a.tasks[0]?.id ?? null };
    }
    case "upsert": {
      // 已存在 → 原位替换（不打乱用户拖好的手动顺序）；新会话 → 置顶。
      const exists = state.tasks.some((t) => t.id === a.task.id);
      const tasks = exists
        ? state.tasks.map((t) => (t.id === a.task.id ? a.task : t))
        : [a.task, ...state.tasks];
      const panels = { ...state.panels };
      if (!panels[a.task.id]) panels[a.task.id] = layoutFor(a.task);
      return { ...state, tasks, activeId: a.task.id, panels };
    }
    // 只改状态：**故意不走 upsert** —— upsert 会顺手 activeId=该任务，
    // 后台会话跑完就把用户正在看的会话抢走了。也故意**不落盘**：
    // 「上一轮出错」重启后既没有上下文可看、也没法再点进去看，红点会变成永久噪声；
    // 「正在跑」落盘更荒唐（重启后什么都没在跑）。状态是运行时事实，只活在内存里。
    case "status": {
      if (!state.tasks.some((t) => t.id === a.id && t.status !== a.status)) return state;
      return { ...state, tasks: state.tasks.map((t) => (t.id === a.id ? { ...t, status: a.status } : t)) };
    }
    case "remove": {
      const tasks = state.tasks.filter((t) => t.id !== a.id);
      const panels = { ...state.panels };
      delete panels[a.id];
      const activeId = state.activeId === a.id ? (tasks[0]?.id ?? null) : state.activeId;
      return { ...state, tasks, panels, activeId };
    }
    case "removeMany": {
      const kill = new Set(a.ids);
      const tasks = state.tasks.filter((t) => !kill.has(t.id));
      const panels = { ...state.panels };
      for (const id of a.ids) delete panels[id];
      const activeId =
        state.activeId && kill.has(state.activeId) ? (tasks[0]?.id ?? null) : state.activeId;
      return { ...state, tasks, panels, activeId };
    }
    case "reorder": {
      // 按传入 id 顺序重排；漏掉的（防御）补到末尾，保证不丢会话。
      const byId = new Map(state.tasks.map((t) => [t.id, t]));
      const tasks: Task[] = [];
      for (const id of a.ids) {
        const t = byId.get(id);
        if (t) {
          tasks.push(t);
          byId.delete(id);
        }
      }
      for (const t of byId.values()) tasks.push(t);
      return { ...state, tasks };
    }
    case "activate":
      return { ...state, activeId: a.id };
    case "setRight":
      return patchLayout(state, a.id, { rightKind: a.kind, rightOpen: true });
    case "toggleRight": {
      const cur = state.panels[a.id] ?? DEFAULT_LAYOUT;
      return patchLayout(state, a.id, { rightOpen: a.open ?? !cur.rightOpen });
    }
    case "setRatio":
      return patchLayout(state, a.id, { rightRatio: Math.min(0.85, Math.max(0.35, a.ratio)) });
    case "toggleDrawer": {
      const cur = state.panels[a.id] ?? DEFAULT_LAYOUT;
      return patchLayout(state, a.id, { drawerOpen: a.open ?? !cur.drawerOpen });
    }
    case "setDrawerHeight":
      return patchLayout(state, a.id, { drawerHeight: Math.min(900, Math.max(120, a.h)) });
    default:
      return state;
  }
}

type Ctx = {
  state: State;
  /** 新建/激活一个任务（按文件夹）。reuse=true（右键/最近）同文件夹已有会话则激活不新建；
   *  reuse=false（手动新建）每次都新开一个会话。
   *
   *  **返回落到的那个会话 id**（新建的或复用的）。护照交接靠它回答「任务去了哪儿」——
   *  调用方不需要这个值时忽略即可，行为一个字节没变。 */
  addTask: (dir: string, source?: TaskSource, reuse?: boolean) => Promise<string>;
  /** 「召唤」一个 AI 专家：在 dir 下新开会话并绑定专家 id（persona/引擎/技能由 experts.ts 恢复）。 */
  addExpertTask: (expert: Expert, dir: string) => Promise<void>;
  /** 在某项目（dir）下新开一个绑工具的会话（claude/codex/openclaw…）。 */
  addSession: (dir: string, tool: string, name: string, startupCmd: string) => void;
  /** 启动一个工具型会话（无项目文件夹，从「我的 AI」运行面板点启动）。 */
  addToolSession: (tool: string, name: string, startupCmd: string, dir?: string) => void;
  removeTask: (id: string) => Promise<void>;
  /** 删除整个项目下的所有会话（一次性，二次确认在 UI 层）。仍不动磁盘文件夹。 */
  removeProject: (ids: string[]) => Promise<void>;
  /** 在 repoDir 下创建 worktree（新目录 + 分支），并建对应任务加入同一项目分组。 */
  addWorktree: (repoDir: string, branch: string, createBranch: boolean) => Promise<void>;
  /** 从归档区恢复一个会话：用**原 id** 原样重建任务卡片。历史档 <id>.jsonl 已被后端
   *  挪回活跃区，同 id 挂载的 Chat 一水合就接上全部对话 —— 是「恢复」，不是「新建」。 */
  restoreTask: (a: { id: string; name: string; dir: string | null }) => Promise<void>;
  /** 按给定 id 顺序重排左侧列表，并把任务型会话的顺序落盘。 */
  reorderTasks: (ids: string[]) => void;
  /** 改会话显示名（测试报告 #016：召唤同一个专家多次会得到一串同名会话，没法分辨谁是谁）。
   *  只改 `name`，不动 dir / expert / 布局 —— 重命名是纯标签操作。 */
  renameTask: (id: string, name: string) => Promise<void>;
  /** 会话跑没跑、跑挂没跑挂（左侧那个小圆点的真相源）。运行时内存，不落盘、不改 activeId。 */
  setTaskStatus: (id: string, status: TaskStatus) => void;
  activate: (id: string) => void;
  setRight: (id: string, kind: RightKind) => void;
  toggleRight: (id: string, open?: boolean) => void;
  setRatio: (id: string, ratio: number) => void;
  toggleDrawer: (id: string, open?: boolean) => void;
  setDrawerHeight: (id: string, h: number) => void;
};

const WorkbenchCtx = createContext<Ctx | null>(null);

export function WorkbenchProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, {
    tasks: [],
    activeId: null,
    panels: {},
    loaded: false,
  });

  const seq = useRef(0);

  // 启动：拉取持久化任务（旧 tasks.json 补 project/kind 默认，向后兼容）
  useEffect(() => {
    invoke<Task[]>("list_tasks")
      .then((tasks) =>
        dispatch({
          type: "load",
          tasks: tasks.map((t) => ({
            ...t,
            kind: t.kind ?? "task",
            project: t.project ?? normDir(t.dir),
          })),
        }),
      )
      .catch(() => dispatch({ type: "load", tasks: [] }));
  }, []);

  const addTask = useCallback(
    async (dir: string, source: TaskSource = "manual", reuse = false) => {
      const proj = normDir(dir);
      const now = Date.now();
      // reuse（右键/最近）：同文件夹已有任务型会话则激活不新建
      if (reuse) {
        const existing = state.tasks.find((t) => t.kind !== "tool" && normDir(t.dir) === proj);
        if (existing) {
          dispatch({ type: "activate", id: existing.id });
          return existing.id;
        }
      }
      const id = `sess-${taskIdFromDir(dir)}-${++seq.current}`;
      const task: Task = {
        id,
        name: dirBasename(dir),
        dir,
        status: "idle",
        source,
        assignee: null,
        external_ref: null,
        last_opened_at: now,
        created_at: now,
        kind: "task",
        project: proj,
      };
      try {
        const saved = await invoke<Task>("upsert_task", { task });
        dispatch({ type: "upsert", task: { ...saved, project: proj, kind: "task" } });
      } catch {
        dispatch({ type: "upsert", task }); // 落盘失败也先进内存
      }
      return id;
    },
    [state.tasks],
  );

  // 「召唤」专家：新开会话，name=专家名，绑 expert.id（Chat 据此注入 persona/引擎/技能）
  const addExpertTask = useCallback(async (expert: Expert, dir: string) => {
    const proj = normDir(dir);
    const now = Date.now();
    const id = `sess-${taskIdFromDir(dir)}-${++seq.current}`;
    const task: Task = {
      id, name: expert.name, dir, status: "idle", source: "manual",
      assignee: null, external_ref: null, last_opened_at: now, created_at: now,
      kind: "task", project: proj, expert: expert.id,
    };
    try {
      const saved = await invoke<Task>("upsert_task", { task });
      dispatch({ type: "upsert", task: { ...saved, project: proj, kind: "task", expert: expert.id } });
    } catch {
      dispatch({ type: "upsert", task });
    }
  }, []);

  // 在某项目（dir）下新开一个绑工具的会话（不落盘，运行时实例）
  const addSession = useCallback((dir: string, tool: string, name: string, startupCmd: string) => {
    const now = Date.now();
    const task: Task = {
      id: `sess-tool-${tool}-${++seq.current}`,
      name,
      dir,
      status: "running",
      source: "manual",
      assignee: null,
      external_ref: null,
      last_opened_at: now,
      created_at: now,
      tool,
      startup_cmd: startupCmd,
      kind: "tool",
      project: dir ? normDir(dir) : null,
    };
    dispatch({ type: "upsert", task });
  }, []);

  // 无项目文件夹的工具会话（运行面板点启动）
  const addToolSession = useCallback(
    (tool: string, name: string, startupCmd: string, dir?: string) => {
      addSession(dir ?? "", tool, name, startupCmd);
    },
    [addSession],
  );

  const addWorktree = useCallback(
    async (repoDir: string, branch: string, createBranch: boolean) => {
      const newPath = await invoke<string>("git_create_worktree", {
        repoRoot: repoDir,
        branch,
        createBranch,
      });
      const now = Date.now();
      const proj = normDir(repoDir); // 和主仓库任务同一项目分组
      const id = `sess-${taskIdFromDir(newPath)}-${++seq.current}`;
      const task: Task = {
        id,
        name: branch,
        dir: newPath,
        status: "idle",
        source: "manual",
        assignee: null,
        external_ref: null,
        last_opened_at: now,
        created_at: now,
        kind: "task",
        project: proj,
        worktree_repo: repoDir,
        worktree_branch: branch,
      };
      try {
        const saved = await invoke<Task>("upsert_task", { task });
        dispatch({
          type: "upsert",
          task: { ...saved, project: proj, kind: "task", worktree_repo: repoDir, worktree_branch: branch },
        });
      } catch {
        dispatch({ type: "upsert", task });
      }
    },
    [],
  );

  const removeTask = useCallback(async (id: string) => {
    dispatch({ type: "remove", id });
    await invoke("remove_task", { id }).catch(() => {});
  }, []);

  // 归档恢复（2026-08-25）：按归档清单里记下的原目录/原名重建任务。dir 为空（旧清单
  // 或未绑定文件夹的会话）只挪文件不建卡——前端归档区此时不显示恢复按钮。
  const restoreTask = useCallback(async (a: { id: string; name: string; dir: string | null }) => {
    // 恢复的是历史 id，可能比本轮 seq 大：推进去，防止之后新会话生成撞号。
    const m = /-(\d+)$/.exec(a.id);
    if (m) {
      const n = parseInt(m[1], 10);
      if (Number.isFinite(n) && n >= seq.current) seq.current = n + 1;
    }
    const dir = a.dir ?? "";
    const now = Date.now();
    const task: Task = {
      id: a.id,
      name: a.name || dirBasename(dir),
      dir,
      status: "idle",
      source: "manual",
      assignee: null,
      external_ref: null,
      last_opened_at: now,
      created_at: now,
      kind: "task",
      project: dir ? normDir(dir) : null,
    };
    try {
      const saved = await invoke<Task>("upsert_task", { task });
      dispatch({ type: "upsert", task: { ...saved, project: task.project, kind: "task" } });
    } catch {
      dispatch({ type: "upsert", task }); // 落盘失败也先进内存
    }
    dispatch({ type: "activate", id: a.id });
  }, []);

  // 整组删除：先一次性更新内存（避免逐条重渲染），再逐条落盘删除。
  const removeProject = useCallback(async (ids: string[]) => {
    if (ids.length === 0) return;
    dispatch({ type: "removeMany", ids });
    for (const id of ids) await invoke("remove_task", { id }).catch(() => {});
  }, []);

  // 重排：先动内存让 UI 立刻跟手，再把 id 顺序丢给后端落盘（工具会话 id 后端会自动忽略）。
  const reorderTasks = useCallback((ids: string[]) => {
    dispatch({ type: "reorder", ids });
    void invoke("reorder_tasks", { ids }).catch(() => {});
  }, []);

  /**
   * 会话状态。**`running` 只活在内存，其余落盘。**
   *
   * 🔴 原来这里只 `dispatch` 不落盘，而 `tasks.json` 里明明有 `status` 字段 ——
   * 于是本机磁盘上 47 条任务 **47 条全是 `idle`**，看板「待办」那一列成了「你加过的
   * 文件夹清单」，「出错」列每次重启清零：昨天挂掉的那个活，今天一点痕迹都没有。
   * （同一个文件里 `renameTask` 是落盘的 —— 漏的只有这一个。）
   *
   * 🔴 **`running` 故意不落盘**：它是「此刻这个 Chat 正在跑」的进程内真值，
   * 重启后必然不成立。写进去，下次开机会看到一屋子「进行中」而一个都没在跑 ——
   * 那正是我们刚在 board 那边修掉的病（`aitasks.rs::DECLARED_FRESH_SECS`），
   * 不能在自己家里再造一遍。`running` 结束时会喂 `idle`/`error`，那一下才落盘。
   */
  const setTaskStatus = useCallback((id: string, status: TaskStatus) => {
    dispatch({ type: "status", id, status });
    if (status === "running") return;
    const cur = state.tasks.find((x) => x.id === id);
    if (!cur || cur.status === status) return; // 没变就不写，省掉一次整份回写
    // 工具型会话后端不存（同 renameTask）；失败也不回滚内存 —— 状态是展示用的，
    // 下次重启顶多回到旧值，不值得为它闪一次 UI。
    void invoke("upsert_task", { task: { ...cur, status } }).catch(() => {});
  }, [state.tasks]);

  const renameTask = useCallback(
    async (id: string, name: string) => {
      const next = name.trim();
      if (!next) return;
      const cur = state.tasks.find((x) => x.id === id);
      if (!cur || cur.name === next) return;
      const task = { ...cur, name: next };
      dispatch({ type: "upsert", task });
      // 工具型会话不落盘（后端只存任务型），失败也不回滚内存：名字是纯展示，
      // 下次重启顶多回到旧名，不值得为它闪一次 UI。
      await invoke("upsert_task", { task }).catch(() => {});
    },
    [state.tasks],
  );

  const activate = useCallback((id: string) => dispatch({ type: "activate", id }), []);
  const setRight = useCallback((id: string, kind: RightKind) => dispatch({ type: "setRight", id, kind }), []);
  const toggleRight = useCallback((id: string, open?: boolean) => dispatch({ type: "toggleRight", id, open }), []);
  const setRatio = useCallback((id: string, ratio: number) => dispatch({ type: "setRatio", id, ratio }), []);
  const toggleDrawer = useCallback((id: string, open?: boolean) => dispatch({ type: "toggleDrawer", id, open }), []);
  const setDrawerHeight = useCallback((id: string, h: number) => dispatch({ type: "setDrawerHeight", id, h }), []);

  return (
    <WorkbenchCtx.Provider
      value={{
        state,
        addTask,
        addExpertTask,
        addSession,
        addToolSession,
        removeTask,
        removeProject,
        reorderTasks,
        renameTask,
        setTaskStatus,
        addWorktree,
        restoreTask,
        activate,
        setRight,
        toggleRight,
        setRatio,
        toggleDrawer,
        setDrawerHeight,
      }}
    >
      {children}
    </WorkbenchCtx.Provider>
  );
}

export function useWorkbench(): Ctx {
  const ctx = useContext(WorkbenchCtx);
  if (!ctx) throw new Error("useWorkbench 必须在 WorkbenchProvider 内使用");
  return ctx;
}
