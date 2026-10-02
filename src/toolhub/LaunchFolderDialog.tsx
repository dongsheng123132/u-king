/**
 * 「在哪个文件夹里打开 <工具>？」—— 启动命令行工具前的选文件夹弹窗。
 *
 * 命令行 AI（Claude Code / Codex / pi / OpenCode / Hermes …）一启动就在「当前文件夹」里读写文件。
 * 以前从「我的 AI」点启动一律落在用户主目录，小白不知道 AI 正在他的主目录里翻东西。现在每次启动先
 * 问一句，并把**最近用过的文件夹**列出来，一键复用（记忆与排序规则见 `recentFolders.ts`）。
 *
 * 这是纯界面控件：不挂 `data-action-id`（它不是动作表里的业务动作），测试钩子用 `data-testid`。
 * 真正「开终端」仍由调用方（App.tsx::runLaunchAction）走 `term_open_external` / `open_terminal_window`，
 * 本组件只负责回答「在哪个文件夹」——选定 `onPick(path)`，取消 `onCancel()`（= 不启动）。
 *
 * 键盘：↑/↓ 换高亮项，Enter = 用高亮项，Esc = 取消。不存在的路径在弹出时自动剔除。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Folder, FolderOpen } from "lucide-react";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";
import {
  defaultFor,
  dropMissing,
  folderName,
  listRecent,
  loadState,
  recordUse,
  saveState,
  type RecentFoldersState,
} from "./recentFolders";

/** 这个路径还是不是一个存在的目录（借 fs.rs 的 `produced_file_info`，只读 metadata，不新增后端命令）。
 *  问不到（命令失败）按「还在」处理——宁可多列一个，也不因为一次探测失败把用户的最近文件夹清空。 */
async function isExistingDir(path: string): Promise<boolean> {
  try {
    const info = await invoke<{ is_dir?: boolean }>("produced_file_info", { path });
    return !!info?.is_dir;
  } catch {
    return true;
  }
}

export function LaunchFolderDialog({
  tool,
  onPick,
  onCancel,
}: {
  tool: { id: string; name: string };
  onPick: (path: string) => void;
  onCancel: () => void;
}) {
  const { t: tr } = useI18n();
  const [state, setState] = useState<RecentFoldersState | null>(null); // null = 还在核对哪些目录还在
  const [selected, setSelected] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  // 弹出时先核对一遍最近目录还在不在，不在的剔掉并写回——之后列出来的每一项都是点了必能进的。
  useEffect(() => {
    let alive = true;
    const loaded = loadState();
    void Promise.all(listRecent(loaded).map(async (it) => ((await isExistingDir(it.path)) ? null : it.path))).then(
      (gone) => {
        if (!alive) return;
        const missing = gone.filter((p): p is string => !!p);
        const pruned = dropMissing(loaded, missing);
        if (missing.length > 0) saveState(pruned);
        setState(pruned);
        setSelected(defaultFor(pruned, tool.id));
      },
    );
    return () => {
      alive = false;
    };
  }, [tool.id]);

  useEffect(() => {
    rootRef.current?.focus();
  }, []);

  const items = state ? listRecent(state) : [];

  const commit = (path: string) => {
    saveState(recordUse(loadState(), tool.id, path, Date.now()));
    onPick(path);
  };

  const browse = async () => {
    if (browsing) return;
    setBrowsing(true);
    try {
      const dir = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: selected ?? undefined,
        title: tr("选择要在哪个文件夹里打开 {name}", { name: tool.name }),
      });
      if (typeof dir === "string" && dir) commit(dir);
    } catch {
      /* 系统选择框打不开：留在弹窗里，用户还能点最近文件夹或取消 */
    } finally {
      setBrowsing(false);
    }
  };

  // 键盘：Esc 取消（捕获阶段拦下，免得同时触发「我的 AI」页自己的 Esc 收起详情条）；
  // ↑/↓ 换高亮；Enter 只在焦点不在别的按钮上时才算「用高亮项」，免得吃掉「取消 / 选择其它」的回车。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        e.preventDefault();
        onCancel();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        if (items.length === 0) return;
        e.preventDefault();
        const cur = items.findIndex((it) => it.path === selected);
        const next = e.key === "ArrowDown" ? Math.min(items.length - 1, cur + 1) : Math.max(0, cur < 0 ? 0 : cur - 1);
        setSelected(items[next].path);
        return;
      }
      if (e.key === "Enter") {
        const el = e.target as HTMLElement | null;
        const onOtherButton = !!el?.closest("button") && !el.closest("[data-folder-row]");
        if (onOtherButton || !selected) return;
        e.preventDefault();
        commit(selected);
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, selected, onCancel]);

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
      data-testid="launch-folder-dialog"
      data-tool-id={tool.id}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        ref={rootRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={tr("在哪个文件夹里打开 {name}？", { name: tool.name })}
        className="w-[480px] max-w-[92vw] rounded-card border border-accent/30 bg-bg-2 shadow-card overflow-hidden outline-none"
      >
        <div className="px-5 pt-5">
          <h3 className="text-[15px] font-semibold text-ink-0">{tr("在哪个文件夹里打开 {name}？", { name: tool.name })}</h3>
          <p className="mt-1 text-[12px] leading-relaxed text-ink-3">
            {tr("{name} 会在这个文件夹里读取、新建和修改文件。", { name: tool.name })}
          </p>
        </div>

        <div className="px-5 py-3">
          {state === null ? (
            <p className="py-6 text-center text-[12px] text-ink-4">{tr("正在读取最近用过的文件夹…")}</p>
          ) : items.length === 0 ? (
            <div
              data-testid="launch-folder-empty"
              className="rounded-lg border border-dashed border-white/[0.12] px-4 py-5 text-center text-[12.5px] text-ink-3"
            >
              {tr("还没有用过的文件夹，先选一个吧。")}
            </div>
          ) : (
            <>
              <div className="mb-1.5 text-[11.5px] text-ink-4">{tr("最近用过的文件夹")}</div>
              <div role="listbox" className="max-h-[260px] overflow-y-auto space-y-1">
                {items.map((it) => {
                  const on = it.path === selected;
                  return (
                    <button
                      key={it.path}
                      role="option"
                      aria-selected={on}
                      data-folder-row
                      data-testid="launch-folder-row"
                      data-path={it.path}
                      onClick={() => setSelected(it.path)}
                      onDoubleClick={() => commit(it.path)}
                      className={cn(
                        "w-full flex items-center gap-2.5 rounded-lg border px-2.5 py-2 text-left transition-colors",
                        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
                        on
                          ? "border-accent bg-accent/[0.10] ring-1 ring-inset ring-accent"
                          : "border-white/[0.06] bg-white/[0.02] hover:bg-white/[0.05]",
                      )}
                    >
                      <Folder size={16} className={cn("shrink-0", on ? "text-accent" : "text-ink-4")} />
                      <span className="min-w-0 flex-1">
                        <span className="block text-[13px] font-medium text-ink-0 truncate">{folderName(it.path)}</span>
                        <span className="block text-[11px] text-ink-4 font-mono truncate" title={it.path}>
                          {it.path}
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </div>

        <div className="flex items-center gap-2 border-t border-white/[0.06] px-5 py-3.5">
          <button
            data-testid="launch-folder-browse"
            onClick={() => void browse()}
            disabled={browsing}
            className={cn(
              "inline-flex items-center gap-1.5 px-3 h-9 rounded-lg text-[12.5px] disabled:opacity-60",
              // 没有最近文件夹时它是唯一的出路 → 做成主按钮
              state !== null && items.length === 0
                ? "bg-accent text-white font-semibold hover:bg-accent-600"
                : "border border-white/[0.10] text-ink-2 hover:bg-white/[0.05]",
            )}
          >
            <FolderOpen size={14} /> {state !== null && items.length === 0 ? tr("选择文件夹…") : tr("选择其它文件夹…")}
          </button>
          <div className="flex-1" />
          <button
            data-testid="launch-folder-cancel"
            onClick={onCancel}
            className="px-3.5 h-9 rounded-lg border border-white/[0.10] text-[13px] text-ink-3 hover:text-ink-1"
          >
            {tr("取消")}
          </button>
          {items.length > 0 && (
            <button
              data-testid="launch-folder-confirm"
              onClick={() => selected && commit(selected)}
              disabled={!selected}
              className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-60"
            >
              {tr("在这里打开")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
