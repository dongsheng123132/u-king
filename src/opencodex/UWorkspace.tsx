/**
 * U-Workspace（工作台）—— 复用基座的会话管理（SessionList + store）+ 我们增强的对话（Chat）。
 *
 * 2026-10-21 用户拍板**回到原样但做减法**：
 * - 保留：左侧项目/会话列表 + 右侧 U-Chat（底座 Claude Code）+ 右侧可切终端。
 * - 去掉：任务护照面板、文件树/预览/浏览器/创作等右侧入口（见 Chat.tsx 的 RIGHT_META 收敛），
 *   这些在 10-19/10-20 两个批次里已经被判为冗余，不再回到界面。
 * - 维护助手不在这一层：它退到维护场景（装机失败 / 体检异常 / 「更多」），见 App.tsx。
 *
 * 会话是 display 保活的：切会话不杀 PTY、不丢对话；切走再回来接着看。
 */
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WorkbenchProvider, useWorkbench } from "./store";
import { dirBasename, type Task, type Engine } from "./types";
import { SessionList } from "./SessionList";
import { Chat } from "./Chat";
import { queueHandoff } from "./handoff";
import { useI18n } from "../i18n";

export function UWorkspace({ onToast, pendingChatPrompt, onConsumedChat, onInstallClaude, paneMode }: {
  onToast?: (m: string) => void;
  /** 一条待投递的对话（别的页面「让 AI 帮我…」）—— 来了就开会话、把它发给 AI。 */
  pendingChatPrompt?: { prompt: string; engine?: Engine; passportId?: string } | null;
  onConsumedChat?: () => void;
  onInstallClaude?: () => void;
  /** 侧栏两个入口共用同一个 UWorkspace 实例时，决定每个 Chat 默认停在对话态还是终端态。 */
  paneMode?: "chat" | "cli";
}) {
  return (
    <WorkbenchProvider>
      <Inner onToast={onToast} pendingChatPrompt={pendingChatPrompt} onConsumedChat={onConsumedChat} onInstallClaude={onInstallClaude} paneMode={paneMode} />
    </WorkbenchProvider>
  );
}

function Inner({ onToast, pendingChatPrompt, onConsumedChat, onInstallClaude, paneMode }: {
  onToast?: (m: string) => void;
  pendingChatPrompt?: { prompt: string; engine?: Engine; passportId?: string } | null;
  onConsumedChat?: () => void;
  onInstallClaude?: () => void;
  paneMode?: "chat" | "cli";
}) {
  const { t: tr } = useI18n();
  const { state, addTask, setTaskStatus, activate, renameTask } = useWorkbench();
  const autoCreated = useRef(false);

  // 首次加载后一个会话都没有 → 用主目录自动建一个（不必先选文件夹就能聊，对齐 OpenCodex）
  useEffect(() => {
    if (!state.loaded || autoCreated.current || state.tasks.length > 0) return;
    autoCreated.current = true;
    invoke<{ home_dir?: string }>("get_env")
      .then((env) => { if (env?.home_dir) void addTask(env.home_dir, "manual", true); })
      .catch(() => {});
  }, [state.loaded, state.tasks.length, addTask]);

  // 「发一句话」handoff（AI 优化大师等页面的「让 AI 帮你…」）——
  // 建一个会话 → 把提示词投进信箱 → 激活它（Chat 挂载时自取并自动发给 AI）。
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
    })();
    onConsumedChat?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.loaded, pendingChatPrompt, onConsumedChat]);

  return (
    <div className="flex h-full min-h-[420px] rounded-card border border-white/[0.08] overflow-hidden bg-bg-2">
      {/* 左：SessionList（项目分组 + 会话）。护照/工作树入口已随本轮精简摘掉。 */}
      <SessionList />
      {/* 右：每个任务(项目)一个 Chat 实例常驻，display 切换保活（切会话不杀 PTY / 不丢对话） */}
      <div className="flex-1 min-w-0 min-h-0 relative">
        {state.tasks.length === 0 ? (
          <div className="absolute inset-0 grid place-items-center text-center px-8">
            <div className="text-ink-3 text-[13px]">{tr("左侧「新建项目」选个文件夹，开始干活")}</div>
          </div>
        ) : (
          state.tasks.map((t: Task) => (
            <div key={t.id} className="absolute inset-0" style={{ display: t.id === state.activeId ? "block" : "none" }}>
              {/* onStatus：会话跑起来/跑完/跑挂了都染左侧那个小圆点。
                  会话是 display 保活的 —— 你在看会话 A 时 B 照样在跑。 */}
              {/* 🔴 常驻挂载的会话不该有导航权：不往 Chat 传任何「跳去别的页」的回调。 */}
              <Chat sessionId={t.id} initialWorkspace={t.dir} onToast={onToast} onInstallClaude={onInstallClaude} taskName={t.name}
                paneMode={paneMode}
                onStatus={(s) => setTaskStatus(t.id, s)}
                /* 自动命名接线：Chat 首条消息会调 onTitle 当会话标题；只改首条，用户手动改过的不覆盖。 */
                onTitle={(title) => { if (t.name === dirBasename(t.dir) || t.name === tr("新对话")) void renameTask(t.id, title); }} />
            </div>
          ))
        )}
      </div>
    </div>
  );
}
