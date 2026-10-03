/**
 * 会话 id 跨重启唯一 + 「哪些会话落盘」的判据 —— `src/opencodex/session.ts` 的纯逻辑断言，无 DOM、不联网。
 *
 *     node scripts/check-session-id.mts
 *
 * 本仓没有前端测试框架（见 check-yield-chain.mts 头注释），所以这是一条可被 node 直接跑的断言脚本：
 * Node >= 22.18 / 24 自带类型剥离，能直接 import `.ts`（session.ts 只用可擦除语法）；
 * 更老的 Node 用 `npx --yes tsx scripts/check-session-id.mts`。
 * 退出码：0 = 全过；1 = 有失败。手动跑，没挂进 `pnpm build`（理由同 check-recent-folders.mts）。
 *
 * ## 它盯的病
 * 旧 id 是 `${前缀}-${++内存计数器}`，计数器每次启动从 0 起 —— U-King 自动更新重启后新会话拿到跟旧会话
 * 相同的 id，后端按 id 去重 → 覆盖旧会话，localStorage / chats/*.jsonl / agent-threads.json 也全按 id 串。
 *
 * 第 6 节**不抄副本**：从 `SessionList.tsx` 的真源码里切出 `archiveOwner`（按 id 前缀把存档 key 配给会话）
 * 来跑，源码改了这条会跟着变。
 */
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import { diskTask, isDurableSession, newIdSuffix, newSessionId } from "../src/opencodex/session.ts";

let fail = 0;
let total = 0;
const eq = (name: string, got: unknown, want: unknown) => {
  total++;
  const g = JSON.stringify(got);
  const w = JSON.stringify(want);
  const ok = g === w;
  if (!ok) fail++;
  console.log(`${ok ? "  ok  " : "  FAIL"}  ${name}${ok ? "" : `\n         got =${g}\n         want=${w}`}`);
};
const yes = (name: string, cond: boolean) => eq(name, cond, true);

/** chatstore.rs::archive_path 的白名单：[A-Za-z0-9._-]、≤120、不以 . 开头。 */
const backendOk = (id: string) => id.length > 0 && id.length <= 120 && /^[A-Za-z0-9._-]+$/.test(id) && !id.startsWith(".");

console.log("\n1. 字符集 / 长度（后端 chatstore.rs 拿它拼文件名）");
{
  const samples = [
    newSessionId("sess-t1abc"),
    newSessionId("sess-tool-claude"),
    newSessionId("sess-tool-open claw/../x"), // 工具名带空格 / 斜杠 / 点点
    newSessionId("sess-tool-中文工具"),
    newSessionId("sess-tool-" + "x".repeat(500)), // 离谱的长工具名
  ];
  for (const id of samples) yes(`后端白名单接受: ${id.slice(0, 48)}${id.length > 48 ? "…" : ""} (${id.length})`, backendOk(id));
  eq("非法字符被换成 _，结构不变", samples[2].startsWith("sess-tool-open_claw_.._x-"), true);
  eq("后缀是 12 位 base36（8 位时间戳 + 4 位随机）", /^[0-9a-z]{12}$/.test(newIdSuffix(Date.UTC(2026, 9, 3), () => 0.5)), true);
}

console.log("\n2. 同一进程内连发不撞（含同一毫秒）");
{
  const burst = new Set<string>();
  for (let i = 0; i < 20000; i++) burst.add(newSessionId("sess-t1abc"));
  eq("20000 个真实 id 全部不同（现实时钟 + Math.random）", burst.size, 20000);

  // 把最坏情况钉死：时钟不动 + 随机数恒定。后缀必须靠顺延拉开，而不是死循环或重复。
  const frozen = new Set<string>();
  for (let i = 0; i < 2000; i++) frozen.add(newIdSuffix(1_800_000_000_000, () => 0.25));
  eq("时钟冻结 + 随机数恒定，2000 个后缀仍全部不同", frozen.size, 2000);
  eq("rand() 返回 1（越界）不崩、仍是 4 位随机段", /^[0-9a-z]{12}$/.test(newIdSuffix(1_800_000_000_001, () => 1)), true);
}

console.log("\n3. 跨重启：模拟「重启 = 内存状态归零」");
{
  // 旧方案：重启后计数器归零，第一个新会话的 id 必然等于上一轮第一个会话的 id。
  const oldScheme = (counter: { n: number }, prefix: string) => `${prefix}-${++counter.n}`;
  const runA = { n: 0 };
  const runB = { n: 0 };
  eq("【旧方案复现】两轮启动的第一个会话 id 相同 → 撞号", oldScheme(runA, "sess-t1abc") === oldScheme(runB, "sess-t1abc"), true);

  // 新方案：带不同 query 的 import 得到一份全新的模块实例（= 进程重启，模块级状态全部归零）。
  const runOld = await import("../src/opencodex/session.ts?run=before-restart");
  const runNew = await import("../src/opencodex/session.ts?run=after-restart");
  yes("两份模块实例确实是分开的（不是同一个缓存）", runOld.newIdSuffix !== runNew.newIdSuffix);

  const t = 1_800_000_000_000;
  // 最坏情况：随机数恒为 0（等于「随机段帮不上忙」）。1 秒后重启，靠时间戳拉开。
  yes("重启后第一个会话 id 与重启前不同（随机段最坏也靠时间戳拉开）", runOld.newIdSuffix(t, () => 0) !== runNew.newIdSuffix(t + 1000, () => 0));
  // 另一半：同一毫秒启动的两个进程，靠随机段拉开。
  yes("同一毫秒启动的两个进程，随机段不同 → 后缀不同", runOld.newIdSuffix(t + 5000, () => 0.1) !== runNew.newIdSuffix(t + 5000, () => 0.9));
}

console.log("\n4. 旧格式 id 仍是合法 id（不迁移旧数据）");
{
  for (const id of ["sess-t1abc-1", "sess-t1abc-12", "sess-tool-claude-3", "sess-tool-openclaw-17", "native-chat"]) {
    yes(`旧 id 通过同一道后端白名单: ${id}`, backendOk(id));
  }
}

console.log("\n5. isDurableSession / diskTask");
{
  eq("有文件夹 → 落盘", isDurableSession({ dir: "D:/demo" }), true);
  eq("无文件夹（运行面板起的 gateway 之类）→ 不落盘", isDurableSession({ dir: "" }), false);
  eq("纯空白 dir → 不落盘", isDurableSession({ dir: "   " }), false);
  eq("dir 缺失（旧数据兜底）→ 不落盘", isDurableSession({ dir: undefined as unknown as string }), false);

  const base = {
    id: "sess-tool-claude-x", name: "新对话", dir: "D:/demo", status: "running", source: "manual",
    assignee: null, external_ref: null, last_opened_at: 1, created_at: 1,
    tool: "claude", startup_cmd: "claude", kind: "tool", project: "d:/demo",
  } as const;
  const running = { ...base };
  const out = diskTask(running as never);
  eq("running 落成 idle", out.status, "idle");
  eq("其余字段原样（tool/startup_cmd/kind/name/project 都在）", { ...out, status: "running" }, { ...base });
  eq("不改入参（内存里仍是 running）", running.status, "running");
  eq("error 原样保留", diskTask({ ...base, status: "error" } as never).status, "error");
  eq("idle 原样保留", diskTask({ ...base, status: "idle" } as never).status, "idle");
}

console.log("\n6. SessionList.archiveOwner 在新旧 id 混用下不误配（读真源码）");
{
  const src = readFileSync("src/opencodex/SessionList.tsx", "utf8");
  const pre = /const CHAT_STORE_PREFIXES = [\s\S]*?as const;/.exec(src);
  const fn = /function archiveOwner\([\s\S]*?\r?\n}\r?\n/.exec(src); // 工作区文件是 CRLF
  yes("从 SessionList.tsx 切到了 archiveOwner 与 CHAT_STORE_PREFIXES", !!pre && !!fn);
  if (pre && fn) {
    // 包进函数里再剥类型：顶层 return 在脚本里不合法。
    const js = stripTypeScriptTypes(`function __probe() {\n${pre[0]}\n${fn[0]}\nreturn archiveOwner;\n}`);
    const archiveOwner = new Function(`${js}\nreturn __probe();`)() as (key: string, ids: string[]) => string | null;

    const oldA = "sess-t1abc-1";
    const oldB = "sess-t1abc-12";
    eq("旧 id：a-1 的存档不被 a-12 吞", archiveOwner("uking.chatpanel.sess-t1abc-12-claude", [oldA, oldB]), oldB);
    eq("旧 id：a-1 自己的存档配给 a-1", archiveOwner("uking.chatpanel.sess-t1abc-1-claude", [oldB, oldA]), oldA);

    const newA = "sess-t1abc-mgabc123x9z1";
    const newB = "sess-t1abc-mgabc123x9z2"; // 只差最后一位
    const ids = [oldA, oldB, newA, newB];
    eq("新 id 的 chatpanel 存档配给自己", archiveOwner(`uking.chatpanel.${newA}-claude`, ids), newA);
    eq("新 id 只差最后一位也不串", archiveOwner(`uking.chatpanel.${newB}-claude`, ids), newB);
    eq("新 id 的引擎名带连字符（claude-cli）也配得对", archiveOwner(`uking.chatpanel.${newA}-claude-cli`, ids), newA);
    eq("新 id 的轻助手存档（无引擎后缀）", archiveOwner(`uking.chat.${newB}`, ids), newB);
    eq("工具会话新 id", archiveOwner("uking.chatpanel.sess-tool-claude-mgabc123x9z1-codex", ["sess-tool-claude-3", "sess-tool-claude-mgabc123x9z1"]), "sess-tool-claude-mgabc123x9z1");
    eq("旧工具会话 id 与新工具会话 id 互不吞", archiveOwner("uking.chatpanel.sess-tool-claude-3-claude", ["sess-tool-claude-mgabc123x9z1", "sess-tool-claude-3"]), "sess-tool-claude-3");
    eq("不认识的 key → null", archiveOwner("uking.chatpanel.other-claude", ids), null);
  }
}

console.log(`\n判决 ${total - fail}/${total}`);
process.exitCode = fail ? 1 : 0; // 不用 process.exit()：本脚本里它在 Windows 上会撞 libuv 断言（实测退出码 127），改 exitCode 就正常
