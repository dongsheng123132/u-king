/**
 * U-Workspace（AI 工作台）—— **直接复用 OpenCodex 的会话管理**（SessionList + store）+ 我们增强的对话（Chat）。
 *
 * 架构（回答"copy 还是调用"）：`src/opencodex/` 是开源 OpenCodex 的 vendored 拷贝（工作台基座逐字同步）；
 * U-Workspace 不再自造会话列表，而是**调用**基座的 `WorkbenchProvider`(store,持久化 tasks.json)
 * + `SessionList`(项目分组 + **拖拽排序** + worktree) —— 白拿这些能力；每个任务(项目文件夹)渲染一个
 * 我们的 `Chat`（大脑选择器 U-King助手/Claude Code/Codex/Hermes + 作图 + 右侧预览/终端/文件/浏览器）。
 * 虾盘云那层只在 Chat 里，绝不进开源基座。删掉只动 App.tsx 一行。
 *
 * ## 三个视图（借鉴 WorkBuddy 的左栏）
 * `chat`（默认，会话）/ `experts`（AI 专家墙）/ `automation`（定时任务）。
 * 后两个是**盖在会话之上的面板**，不是路由切换 —— 所有 Chat 实例照旧挂着、display 保活，
 * PTY 和对话一个都不掉。挑完专家、配完定时任务，点任意会话就回到原处。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WorkbenchProvider, useWorkbench } from "./store";
import { dirBasename, type Task, type WorkView, type Engine } from "./types";
import { SessionList } from "./SessionList";
import { Chat } from "./Chat";
import { ExpertGallery } from "./ExpertGallery";
import { AutomationPanel } from "./AutomationPanel";
import { TaskBoard } from "./TaskBoard";
import { PassportBoard } from "./PassportBoard";
import { Arena } from "./Arena";
import { queueHandoff, type Handoff } from "./handoff";
import { queueTermCmd } from "./termInbox";
import { findExpert, type Expert } from "./experts";
import { useI18n } from "../i18n";

export function UWorkspace({ onToast, pendingExpert, onConsumed, pendingChatPrompt, onConsumedChat, onInstallClaude, onGoCreate }: {
  onToast?: (m: string) => void;
  pendingExpert?: Expert | null;
  onConsumed?: () => void;
  /** 一条待投递的对话（DSH 插件页「让 AI 帮你挑」）—— 来了就开会话、把它发给 AI。 */
  pendingChatPrompt?: { prompt: string; engine?: Engine; passportId?: string } | null;
  onConsumedChat?: () => void;
  onInstallClaude?: () => void;
  /** 专家卡的 route 指向作图/视频时，切到侧栏「AI 创作」那一页（面板已撤，见 App.tsx 注释）。 */
  onGoCreate?: (sub: "draw" | "video") => void;
}) {
  return (
    <WorkbenchProvider>
      <Inner onToast={onToast} pendingExpert={pendingExpert} onConsumed={onConsumed} pendingChatPrompt={pendingChatPrompt} onConsumedChat={onConsumedChat} onInstallClaude={onInstallClaude} onGoCreate={onGoCreate} />
    </WorkbenchProvider>
  );
}

function Inner({ onToast, pendingExpert, onConsumed, pendingChatPrompt, onConsumedChat, onInstallClaude, onGoCreate }: {
  onToast?: (m: string) => void;
  pendingExpert?: Expert | null;
  onConsumed?: () => void;
  pendingChatPrompt?: { prompt: string; engine?: Engine; passportId?: string } | null;
  onConsumedChat?: () => void;
  onInstallClaude?: () => void;
  /** 专家卡的 route 指向作图/视频时，切到侧栏「AI 创作」那一页（面板已撤，见 App.tsx 注释）。 */
  onGoCreate?: (sub: "draw" | "video") => void;
}) {
  const { t: tr } = useI18n();
  const { state, addTask, addExpertTask, setTaskStatus, activate, renameTask } = useWorkbench();
  const autoCreated = useRef(false);
  const [view, setView] = useState<WorkView>("chat");
  const [failedJobs, setFailedJobs] = useState(0);

  /**
   * 定时任务跑挂了要有地方看 —— 这是「会话红点」的另一半。
   *
   * 会话跑挂时人至少在旁边（是他自己发的），定时任务**到点自己跑，人根本不在场**：
   * 早上九点那条挂了，除非他主动点开自动化面板，否则永远不知道。数据一直都在
   * （`last_ok`，automation.rs 写、AutomationPanel 也显示），缺的就是把它顶到入口上。
   *
   * 只数**开着的**任务：关掉的任务上次失败过是历史，不是待办 —— 拿它一直亮红点是骚扰。
   * 轮询而不是等事件：调度是应用内线程，失败发生在任何时刻，而这一下只是读个 JSON。
   */
  useEffect(() => {
    let alive = true;
    const scan = () =>
      invoke<{ jobs?: { enabled?: boolean; last_ok?: boolean | null }[] }>("list_automations")
        .then((r) => {
          if (!alive) return;
          setFailedJobs((r?.jobs ?? []).filter((j) => j.enabled && j.last_ok === false).length);
        })
        .catch(() => {});
    void scan();
    const id = setInterval(scan, 60_000);
    return () => { alive = false; clearInterval(id); };
  }, [view]);

  // 首次加载后一个会话都没有 → 用主目录自动建一个（不必先选文件夹就能聊，对齐 OpenCodex）
  useEffect(() => {
    if (!state.loaded || autoCreated.current || state.tasks.length > 0) return;
    autoCreated.current = true;
    invoke<{ home_dir?: string }>("get_env")
      .then((env) => { if (env?.home_dir) void addTask(env.home_dir, "manual", true); })
      .catch(() => {});
  }, [state.loaded, state.tasks.length, addTask]);

  /**
   * 护照交接的落点：在护照的工作目录里建/复用一个会话，把状态投进去。
   *
   * 为什么由宿主做而不是 `PassportBoard` 自己做：会话是 store 的东西，
   * 护照页不该认识 store（同 `ChatPanel` 那条自律：模块只靠 props 通信）。
   *
   * `reuse=true`：同一个目录反复交接不该堆出一排重复会话 —— 交接的是**同一件事**。
   * **不自动切视图**：用户点的是「交出去」，不是「我要去那边」。切走的话他就看不见
   * 那句「已交给谁、落在哪个会话」的回执了 —— 而这正是这次要修的东西。
   * 回执那一行自带「打开会话 →」，去不去他自己定。
   */
  const handoffToSession = async (dir: string, h: Handoff) => {
    const sessionId = await addTask(dir, "manual", true);
    queueHandoff(sessionId, h);
    const name = state.tasks.find((t) => t.id === sessionId)?.name ?? dirBasename(dir);
    return { sessionId, sessionName: name };
  };

  /** 召唤一个专家 = 开一个绑该专家的会话 + 回到对话视图。**页面版和工作台内的专家墙共用这一条路。** */
  const summon = (e: Expert) => {
    // 带 `route` 的专家（作图 / 视频）= 侧栏那一页的快捷入口，卡片上写的就是「打开」不是「召唤」。
    // 🔴 跳转只发生在**用户此刻点了那张卡**这一瞬，且不建会话。
    // 以前这条路挂在 Chat 的 effect 里，而工作台把每个会话都常驻挂载（display 保活）——
    // 于是「历史上召唤过一次作图专家」变成「以后每次进工作台都被弹回作图页」，
    // 客户两次反馈的「强制自动跳回这个界面」就是它。会话本身也是多余的：
    // 用户要的是那一页，不是一个绑着作图 persona 的对话框。
    // 宿主没传 onGoCreate 时才退回开会话 —— 宁可这条路绕一点，也不要点了没反应。
    if (e.route && onGoCreate) {
      onGoCreate(e.route === "video" ? "video" : "draw");
      return;
    }
    void invoke<{ home_dir?: string }>("get_env")
      .then((env) => addExpertTask(e, env?.home_dir || "."))
      .catch(() => addExpertTask(e, "."))
      .finally(() => {
        void invoke("install_skill_pack").catch(() => {}); // 技能落盘（best-effort）
        setView("chat");
      });
  };

  // 「召唤」handoff：AI 专家**页**点召唤 → App 存 pendingExpert + 切到本页 → 这里开会话。
  // （工作台内的专家墙不走这条 —— 它直接调 summon，少绕一圈 App 状态。）
  useEffect(() => {
    if (!state.loaded || !pendingExpert) return;
    summon(pendingExpert);
    onConsumed?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.loaded, pendingExpert, onConsumed]);

  // 「发一句话」handoff（DSH 插件页「让 AI 帮你挑」）：跟召唤专家同一条路 ——
  // 建一个会话 → 把提示词投进信箱 → 切到对话视图（Chat 挂载时自取并自动发给 AI）。
  // 走 `addTask`（普通任务型会话）而不是 addExpertTask：这条没有 persona，就是一段任务提示词。
  useEffect(() => {
    if (!state.loaded || !pendingChatPrompt) return;
    void (async () => {
      const env = await invoke<{ home_dir?: string }>("get_env").catch(() => null);
      const dir = env?.home_dir || ".";
      const id = await addTask(dir, "manual", false);
      queueHandoff(id, {
        passportId: pendingChatPrompt.passportId ?? "dsh-hire",
        engine: pendingChatPrompt.engine ?? "claude",
        prompt: pendingChatPrompt.prompt,
      });
      activate(id);
      setView("chat");
    })();
    onConsumedChat?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.loaded, pendingChatPrompt, onConsumedChat]);

  return (
    <div className="flex h-full min-h-[420px] rounded-card border border-white/[0.08] overflow-hidden bg-bg-2">
      {/* 左：真 SessionList —— 项目分组 + 拖拽排序 + worktree（复用基座，不自造）+ 专家/自动化入口 */}
      <SessionList view={view} onView={setView} navBadge={{ automation: failedJobs }} />
      {/* 右：每个任务(项目)一个 Chat 实例常驻，display 切换保活（切会话不杀 PTY / 不丢对话/预览） */}
      <div className="flex-1 min-w-0 min-h-0 relative">
        {state.tasks.length === 0 ? (
          <div className="absolute inset-0 grid place-items-center text-center px-8">
            <div className="text-ink-3 text-[13px]">{tr("左侧「新建项目」选个文件夹，开始干活")}</div>
          </div>
        ) : (
          state.tasks.map((t: Task) => (
            <div key={t.id} className="absolute inset-0" style={{ display: view === "chat" && t.id === state.activeId ? "block" : "none" }}>
              {/* onStatus：会话跑起来/跑完/跑挂了都染左侧那个小圆点。
                  会话是 display 保活的 —— 你在看会话 A 时 B 照样在跑，B 挂了以前**没有任何地方会说**
                  （轻助手那侧只弹一条会自己消失的 toast，Claude 那侧只在它自己的对话里贴一句）。 */}
              {/* 🔴 不再往 Chat 传 onGoCreate：常驻挂载的会话不该有导航权（见 summon 注释）。 */}
              <Chat sessionId={t.id} initialWorkspace={t.dir} onToast={onToast} expert={findExpert(t.expert)} onInstallClaude={onInstallClaude} taskName={t.name} onFindExpert={() => setView("experts")} onSummonExpert={summon}
                active={view === "chat" && t.id === state.activeId}
                onStatus={(s) => setTaskStatus(t.id, s)}
                /* 自动命名接线（2026-08-25）：Chat 首条消息会调 onTitle 当会话标题，
                   此前一直没人传这个 prop → 满屏「新对话」（fable5 架构评审揪出的死代码）。
                   只改首条：用户手动改过的名不覆盖（renameTask 内部按 id 落盘 tasks.json）。 */
                onTitle={(title) => { if (t.name === dirBasename(t.dir) || t.name === tr("新对话")) void renameTask(t.id, title); }} />
            </div>
          ))
        )}

        {/* 功能面板：盖在会话之上（会话没被卸载，只是不显示）。滚动条各自独立。 */}
        {view !== "chat" && (
          view === "passports" ? (
            // 护照页自带内边距和滚动（同看板），不再套外层 px/py。
            <div className="absolute inset-0 bg-bg-2">
              <PassportBoard
                onHandoff={handoffToSession}
                onOpenSession={(id) => { activate(id); setView("chat"); }}
                // 护照没写 scope 时的兜底：当前正开着的那个会话的目录。
                // 仍然可能是空的 —— 那时护照页会问用户选一个，**不静默挑**。
                fallbackDir={state.tasks.find((t) => t.id === state.activeId)?.dir}
              />
            </div>
          ) : view === "kanban" ? (
            // 看板自带内边距和滚动，不再套外层 px/py —— 双重内边距会把五列挤没。
            <div className="absolute inset-0 bg-bg-2">
              <TaskBoard
                onOpenSession={(id) => { activate(id); setView("chat"); }}
                onOpenAutomation={() => setView("automation")}
                // 点别家 AI 的卡片 = 拿它的工作目录在这儿开/复用一个会话。
                // reuse=true：同一个文件夹反复点不会堆出一排重复会话。
                onOpenFolder={(dir) => { void addTask(dir, "manual", true).then(() => setView("chat")); }}
                // 「接着干」= 上面那一步 + 把 `claude --resume <sid>` 投进这个会话的终端信箱。
                // 🔴 **投在 setView 之前**：`Chat` 一挂载就去信箱自取，晚投一步就取空了。
                // 不替用户回车（见 termInbox.ts）—— 起一次 AI 会话是花钱的写操作。
                onResume={(dir, cmd) => {
                  void addTask(dir, "manual", true).then((id) => {
                    queueTermCmd(id, cmd);
                    setView("chat");
                  });
                }}
              />
            </div>
          ) : view === "arena" ? (
            <div className="absolute inset-0 bg-bg-2">
              <Arena
                workspace={state.tasks.find((t) => t.id === state.activeId)?.dir}
                onToast={onToast}
              />
            </div>
          ) : (
            <div className="absolute inset-0 overflow-y-auto bg-bg-2 px-5 py-4">
              {view === "experts" ? (
                <div className="space-y-4">
                  <header>
                    <h2 className="text-[15px] font-semibold text-ink-0">{tr("AI 专家")}</h2>
                    <p className="text-[12px] text-ink-4 mt-0.5">
                      {tr("挑一个专家 → 当场在这个工作台开一个绑好它的会话，直接干活出成果")}
                    </p>
                  </header>
                  <ExpertGallery onSummon={summon} dense />
                </div>
              ) : (
                <AutomationPanel onToast={onToast} />
              )}
            </div>
          )
        )}
      </div>
    </div>
  );
}
