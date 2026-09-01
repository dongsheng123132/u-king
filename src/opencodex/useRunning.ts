/**
 * 轮询后端 list_running —— 返回当前正在跑的工具集合（运行面板据此显示绿点 + 停止）。
 * 2s 一次，够实时又不费。
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type RunningTool = { tool: string; session_id: string };

export function useRunning(intervalMs = 2000) {
  const [running, setRunning] = useState<RunningTool[]>([]);

  useEffect(() => {
    let alive = true;
    const tick = () => {
      invoke<RunningTool[]>("list_running")
        .then((r) => {
          if (alive) setRunning(r);
        })
        .catch(() => {});
    };
    tick();
    const t = setInterval(tick, intervalMs);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [intervalMs]);

  return running;
}
