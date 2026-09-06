/**
 * 终端工作台 —— 「项目 → 终端」直达入口：不经过对话会话，直接按项目文件夹开终端。
 *
 * 现状是纯终端只能藏在会话的引擎下拉（claude-cli/hermes 全屏 TermPanel）或右面板里，
 * 想「就是开个终端」的人得先建一个 AI 会话才摸得到。这里复用 `TermPanel` 全部能力
 * （多标签 / 自定义命令 / 文件链接），不新建第二套终端逻辑 —— 只是换了个入口。
 */
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Maximize2 } from "lucide-react";
import { TermPanel } from "./panels/TermPanel";
import { useWorkbench } from "./store";
import { dirBasename, normDir, type Task } from "./types";
import { useI18n } from "../i18n";

const LAST_PROJECT_KEY = "uking:termwb-project";

/** 项目列表：按 `project ?? normDir(dir)` 去重，显示名用 dirBasename。 */
function projectsOf(tasks: Task[]): { key: string; dir: string; name: string }[] {
  const seen = new Map<string, { key: string; dir: string; name: string }>();
  for (const t of tasks) {
    const key = t.project ?? normDir(t.dir);
    if (!key || seen.has(key)) continue;
    seen.set(key, { key, dir: t.dir, name: dirBasename(t.dir) });
  }
  return [...seen.values()];
}

export function TerminalWorkbench({ active, onToast }: { active: boolean; onToast: (msg: string) => void }) {
  const { t: tr } = useI18n();
  const { state } = useWorkbench();

  const projects = useMemo(() => projectsOf(state.tasks), [state.tasks]);

  const activeProjectKey = useMemo(() => {
    const activeTask = state.tasks.find((t) => t.id === state.activeId);
    return activeTask ? (activeTask.project ?? normDir(activeTask.dir)) : null;
  }, [state.tasks, state.activeId]);

  const [selected, setSelected] = useState<string | null>(() => {
    try {
      return localStorage.getItem(LAST_PROJECT_KEY);
    } catch {
      return null;
    }
  });
  // 打开过的项目（keep-alive 挂载，PTY 不能因换项目被杀）
  const [opened, setOpened] = useState<Set<string>>(() => new Set());

  // 选中项目落 localStorage，缺省 = 当前激活任务的项目，否则列表第一个。
  useEffect(() => {
    if (projects.length === 0) return;
    const stillValid = selected && projects.some((p) => p.key === selected);
    if (stillValid) return;
    const next = (activeProjectKey && projects.some((p) => p.key === activeProjectKey))
      ? activeProjectKey
      : projects[0].key;
    setSelected(next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projects, activeProjectKey]);

  useEffect(() => {
    if (!selected) return;
    setOpened((prev) => (prev.has(selected) ? prev : new Set(prev).add(selected)));
    try {
      localStorage.setItem(LAST_PROJECT_KEY, selected);
    } catch {
      /* 隐私模式/配额异常：不落盘也不影响使用 */
    }
  }, [selected]);

  const selectedProject = projects.find((p) => p.key === selected) ?? null;

  const pullOut = () => {
    if (!selectedProject) return;
    void invoke("open_terminal_window", { cwd: selectedProject.dir, cmd: null }).catch((e) =>
      onToast(tr("拉出终端失败：{e}", { e: String(e) })),
    );
  };

  if (projects.length === 0) {
    return (
      <div className="h-full flex items-center justify-center text-center px-8">
        <div className="text-ink-3 text-[13px]">{tr("先在左侧列表新建项目（选文件夹）")}</div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col min-h-0">
      <div className="flex items-center gap-2 h-10 shrink-0 px-3 border-b border-white/[0.06]">
        <select
          value={selected ?? ""}
          onChange={(e) => setSelected(e.target.value)}
          className="h-7 px-2 rounded text-[12px] bg-bg-1 border border-white/10 text-ink-1 min-w-0 max-w-[260px]"
          title={tr("选择项目")}
        >
          {projects.map((p) => (
            <option key={p.key} value={p.key}>
              {p.name}
            </option>
          ))}
        </select>
        <button
          onClick={pullOut}
          title={tr("把终端拉成独立窗口（可以和工作台并排看）")}
          className="inline-flex items-center gap-1 h-7 px-2 rounded text-[12px] text-ink-3 hover:bg-white/[0.05]"
        >
          <Maximize2 size={13} /> {tr("拉出")}
        </button>
      </div>
      <div className="flex-1 min-h-0 relative">
        {[...opened].map((key) => {
          const p = projects.find((x) => x.key === key);
          if (!p) return null;
          return (
            <div key={key} className="absolute inset-0" style={{ display: key === selected ? "block" : "none" }}>
              <TermPanel cwd={p.dir} active={active && key === selected} onToast={onToast} />
            </div>
          );
        })}
      </div>
    </div>
  );
}
