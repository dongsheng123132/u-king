/**
 * 「我的 AI」运行面板 —— OpenCodex 左栏顶部。
 *
 * 已装的 AI 工具都列出来：未运行=正常色+「启动」，运行中=绿点高亮+「停止」（像 IDE 运行面板）。
 * 启动 = 新开一个工具型会话（右侧出终端，自动跑启动命令，并打 tool tag）。
 * 停止 = term_close 该工具所有运行中的 PTY 会话。运行状态来自轮询 list_running。
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Play, Square } from "lucide-react";
import { useWorkbench } from "./store";
import { useRunning } from "./useRunning";
import { useI18n } from "../i18n";

type ToolInfo = {
  id: string;
  name: string;
  installed: boolean;
  launch_cmd: string;
  launch_app: string;
};

/** 工具 id → 启动命令（openclaw 是 gateway 服务）。覆盖 tools.rs 的 launch_cmd 特例。
 *  gateway 必须带 --allow-unconfigured --port，否则裸 `openclaw gateway run` 会因缺 gateway.mode
 *  被判「suspicious config」直接卡死（对齐 apps.ts / ToolAppView 的正确命令）。 */
function startupCmd(t: ToolInfo): string {
  if (t.id === "openclaw") return "openclaw gateway run --allow-unconfigured --port 18789";
  return t.launch_cmd || t.id;
}

export function RunPanel() {
  const { t: tr } = useI18n();
  const { addToolSession } = useWorkbench();
  const running = useRunning();
  const [tools, setTools] = useState<ToolInfo[]>([]);

  useEffect(() => {
    invoke<ToolInfo[]>("list_tools")
      .then((all) => setTools(all.filter((t) => t.installed && (t.launch_cmd || t.launch_app))))
      .catch(() => {});
  }, []);

  // 终端类工具（有 launch_cmd），GUI 类（仅 launch_app）不在 OpenCodex 里跑
  const cliTools = tools.filter((t) => t.launch_cmd);
  if (cliTools.length === 0) return null;

  const runningTools = new Set(running.map((r) => r.tool));

  const start = (t: ToolInfo) => {
    addToolSession(t.id, t.name, startupCmd(t));
  };
  const stop = async (toolId: string) => {
    // 关掉该工具所有运行中的 PTY
    const sids = running.filter((r) => r.tool === toolId).map((r) => r.session_id);
    await Promise.all(sids.map((sid) => invoke("term_close", { sessionId: sid }).catch(() => {})));
  };

  return (
    <div className="px-2.5 py-2 border-b border-white/[0.06]">
      <div className="text-[10px] text-ink-5 px-1 pb-1.5">{tr("我的 AI")}</div>
      <div className="space-y-0.5">
        {cliTools.map((t) => {
          const on = runningTools.has(t.id);
          return (
            <div
              key={t.id}
              className={
                "group flex items-center gap-2 px-2 py-1.5 rounded-card text-[12.5px] " +
                (on ? "bg-success-500/[0.08]" : "hover:bg-white/[0.03]")
              }
            >
              <span className={"dot " + (on ? "dot-on" : "dot-off")} />
              <span className={"flex-1 min-w-0 truncate " + (on ? "text-ink-0" : "text-ink-2")}>{t.name}</span>
              {on ? (
                <button
                  onClick={() => void stop(t.id)}
                  title={tr("停止")}
                  className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-4 hover:text-danger-400 hover:bg-white/[0.06]"
                >
                  <Square size={12} />
                </button>
              ) : (
                <button
                  onClick={() => start(t)}
                  title={tr("启动")}
                  className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-4 hover:text-accent-400 hover:bg-white/[0.06] opacity-0 group-hover:opacity-100"
                >
                  <Play size={12} />
                </button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
