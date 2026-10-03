/**
 * 会话 id 生成 + 「哪些会话要落盘」的判据 —— 纯函数，不碰 Tauri / React，可被 node 直接跑断言
 * （`scripts/check-session-id.mts`）。
 *
 * ## 为什么 id 不能再用 `++计数器`
 * 原来 store 里是 `useRef(0)` 的计数器，**每次启动从 0 开始**，而 `load` 只读回旧会话、不推进它。
 * 客户机实测：U-King 自动更新重启后，新建的对话拿到跟旧会话**一模一样**的 id
 * （`sess-<folderhash>-1`），于是——
 *  · 后端 `tasks.rs::upsert_task` 按 id 去重 → 新会话**覆盖**旧会话那一行；
 *  · `uking.chatpanel.<id>-<engine>`、`~/.uking/chats/<id>.jsonl`、`agent-threads.json`
 *    全是按 id 串的 → 新会话读到（或冲掉）旧会话的历史，甚至 resume 到旧的 Claude 会话。
 * 所以后缀必须**跨重启唯一**：毫秒时间戳（base36）+ 4 位随机（base36）。
 *
 * 旧格式 id（`sess-<hash>-<n>` / `sess-tool-<tool>-<n>`）不迁移，原样可用：它们只是不透明字符串，
 * 谁也不解析数字后缀（`restoreTask` 里曾按数字后缀推进计数器，计数器没了，那段也就没了）。
 */
import type { Task } from "./types";

/**
 * 后端 `chatstore.rs::archive_path` 把 sessionId 直接拼进文件名，只放行 `[A-Za-z0-9._-]`
 * 且长度 ≤ 120、不以 `.` 开头。生成侧自己守住这条，别等落盘时才被拒。
 */
const UNSAFE_ID_CHARS = /[^A-Za-z0-9._-]/g;
const MAX_PREFIX_LEN = 80;

/** 4 位 base36 = 36^4 ≈ 168 万种。 */
const RAND_SPACE = 36 ** 4;

let lastTs = -1;
let lastRand = 0;

/**
 * id 后缀：`<毫秒时间戳 base36><4 位随机 base36>`（现在是 12 位，如 `mgabc123x9z1`）。
 * `now` / `rand` 可注入，仅为让测试能钉死「同一毫秒也不撞」。
 *
 * 跨进程（跨重启）靠时间戳拉开，同一毫秒启动的两个进程再靠随机段；同一进程内，同一毫秒里
 * 连发的第 2 个起，随机段改成上一个 +1 —— 不靠运气，进程内严格不重复（代价就这几行，而撞了
 * 的后果是上面那串「覆盖旧会话」）。
 */
export function newIdSuffix(now: number = Date.now(), rand: () => number = Math.random): string {
  let r = Math.min(RAND_SPACE - 1, Math.floor(rand() * RAND_SPACE));
  if (now === lastTs) r = (lastRand + 1) % RAND_SPACE;
  lastTs = now;
  lastRand = r;
  return now.toString(36) + r.toString(36).padStart(4, "0");
}

/** `<prefix>-<后缀>`。prefix 里不在白名单内的字符（比如工具名带空格/斜杠）换成 `_`，并限长。 */
export function newSessionId(prefix: string): string {
  const safe = prefix.replace(UNSAFE_ID_CHARS, "_").slice(0, MAX_PREFIX_LEN);
  return `${safe}-${newIdSuffix()}`;
}

/**
 * **这个会话要不要写进 `tasks.json`** —— 「对话会话」与「临时工具会话」的唯一判据，别在别处另判。
 *
 * 有文件夹 = 项目里的一个对话（右边是 Chat 视图，客户当它是正经会话），重启后必须还在；
 * 没文件夹 = 「我的 AI」运行面板起的工具实例（如 openclaw gateway），跑完即弃，不落盘。
 * 这条与后端一致：`upsert_task` 本来就拒绝空 dir。
 * 不按 `tool` 名字判：Chat 根本不读 `tool`（它只影响列表上的标签/图标），claude/codex/hermes
 * 在这点上没有区别。
 */
export function isDurableSession(t: Pick<Task, "dir">): boolean {
  return (t.dir ?? "").trim() !== "";
}

/**
 * 写盘用的副本：`running` 落成 `idle`。
 * 运行状态只活在内存里（重启后必然不成立，写进去下次开机就是一屋子「进行中」而一个都没在跑）。
 * 工具型会话建出来就硬写 `running`（含义是「终端开着」），所以它们走落盘路径（建会话 / 改名）时
 * 必须过这一道，否则 `running` 会顺着 `{ ...cur }` 漏进 tasks.json。
 */
export function diskTask(t: Task): Task {
  return t.status === "running" ? { ...t, status: "idle" } : t;
}
