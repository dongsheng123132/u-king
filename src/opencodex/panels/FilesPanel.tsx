/**
 * 文件面板 —— 任务文件夹的文件树（懒加载）+ 通用文档预览/标注 + Codex 式文件管理。
 *
 * 树节点点开才请求 list_dir（单层），Map 缓存已加载层。双击文件交给 redline 渲染（图片/PDF/
 * Word/Excel/PPT/ZIP/文本…）并可圈选标注，把标注一键发给终端里的 AI。不引文件树库，纯递归组件 + Tailwind。
 *
 * 文件管理（对齐 Codex 桌面版）：
 *  - 头部「打开」下拉：把根目录在资源管理器 / VS Code / Cursor / 终端 / Git Bash 打开。
 *  - 树项右键菜单：在资源管理器中显示、用默认程序打开、用编辑器/终端/Git Bash 打开、复制路径。
 *
 * 预览走 asset 协议（convertFileSrc）。asset scope 默认只放行 ~/.uking/video/*，所以打开任意文件夹
 * 前必须先 `allow_fs_preview(root)` 把该目录放行，否则预览会 HTTP 403（历史 bug）。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  ChevronDown,
  ChevronRight,
  ExternalLink,
  File as FileIcon,
  Folder,
  FolderOpen,
  RefreshCw,
} from "lucide-react";
import { RedlinePanel } from "../../vendor/redline-core/index";
import { createTauriRedlineHost } from "../redline-host-tauri";
import { copyToClipboard } from "../../lib/clipboard";
import { isWindows } from "../../lib/platform";
import { AnchoredMenu } from "../../components/AnchoredMenu";
import { cn } from "../../lib/cn";
import { useI18n } from "../../i18n";

type Entry = { name: string; path: string; is_dir: boolean; size: number };

// 头部「打开」下拉：把根目录在外部应用打开（对齐 Codex 的「打开位置」）。
const ROOT_OPEN_APPS: { app: string; label: string }[] = [
  { app: "explorer", label: "资源管理器" },
  { app: "vscode", label: "VS Code" },
  { app: "cursor", label: "Cursor" },
  { app: "terminal", label: "终端" },
  { app: "gitbash", label: "Git Bash" },
];

export function FilesPanel({
  root,
  active,
  treeWidth = 280,
  activePath: activePathProp,
  onActivePathChange,
  onToast,
}: {
  root: string;
  active: boolean;
  /** 文件树那一列的宽度。终端右边那条窄栏要传小一点；280 是「文件」tab 的原始宽度。 */
  treeWidth?: number;
  /**
   * 受控的「当前预览哪个文件」。传了就由外部说了算 —— 终端里点一条文件路径要能驱动这里，
   * 而不是让终端去切走整个面板。不传则组件自己管（「文件」tab 的老行为）。
   */
  activePath?: string | null;
  onActivePathChange?: (path: string | null) => void;
  /** 复制路径 / 打开失败的轻提示。不传则静默。 */
  onToast?: (msg: string) => void;
}) {
  const { t } = useI18n();
  // path -> 该目录的子项（已加载）
  const [cache, setCache] = useState<Record<string, Entry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  // 当前在右侧预览的文件绝对路径（交给 redline 读字节 + 渲染）
  const [activePathSelf, setActivePathSelf] = useState<string | null>(null);
  const controlled = activePathProp !== undefined;
  const activePath = controlled ? activePathProp : activePathSelf;
  const [loadedRoot, setLoadedRoot] = useState(false);
  const [openMenu, setOpenMenu] = useState(false); // 头部「打开」下拉
  const openBtnRef = useRef<HTMLButtonElement>(null);
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; entry: Entry } | null>(null);
  // 已放行进 asset scope 的目录（去重，避免每次开文件重复 push 规则）
  const allowedRef = useRef<Set<string>>(new Set());
  const redlineHost = useMemo(() => createTauriRedlineHost(), []);

  /**
   * 窄了就上下分（树在上、预览在下），像 VS Code 的资源管理器。
   *
   * 为什么要有这一档：本组件在终端右边那条栏里复用时，栏宽被「终端保底 240px」夹着 ——
   * 实测 1280 宽的机器上整条栏只有 280px，左右分的话预览区只剩 80px，等于没有。
   * **自己测自己的宽度**而不是让宿主传 prop：这是组件自身的排版问题，宿主不该关心；
   * 换个宿主也自动成立。
   */
  const rootRef = useRef<HTMLDivElement>(null);
  const [stacked, setStacked] = useState(false);
  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setStacked(el.clientWidth > 0 && el.clientWidth < 460));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const setActivePath = useCallback(
    (p: string | null) => {
      if (!controlled) setActivePathSelf(p);
      onActivePathChange?.(p);
    },
    [controlled, onActivePathChange],
  );

  // 把目录（递归）放行进 asset 协议 scope —— 不放行则 redline 预览走 convertFileSrc 会 HTTP 403
  const ensureAllowed = useCallback(async (dir: string) => {
    if (allowedRef.current.has(dir)) return;
    allowedRef.current.add(dir);
    try {
      await invoke("allow_fs_preview", { path: dir });
    } catch {
      allowedRef.current.delete(dir); // 失败不缓存，下次重试
    }
  }, []);

  const load = useCallback(async (dir: string) => {
    try {
      const list = await invoke<Entry[]>("list_dir", { path: dir, showNoise: false });
      setCache((c) => ({ ...c, [dir]: list }));
    } catch {
      setCache((c) => ({ ...c, [dir]: [] }));
    }
  }, []);

  // 首次激活时加载根 + 放行根目录（预览用）
  useEffect(() => {
    if (active && !loadedRoot) {
      setLoadedRoot(true);
      void ensureAllowed(root);
      void load(root);
    }
  }, [active, loadedRoot, root, load, ensureAllowed]);

  // 受控切文件时（终端里点了一条路径），那个文件很可能**不在 root 底下** ——
  // 只放行了 root 的话预览会 403。这里补放行它自己所在的目录。
  useEffect(() => {
    if (!activePath) return;
    const dir = activePath.replace(/[\\/][^\\/]*$/, "");
    if (dir && dir !== activePath) void ensureAllowed(dir);
  }, [activePath, ensureAllowed]);

  const toggle = useCallback(
    (e: Entry) => {
      setExpanded((prev) => {
        const next = new Set(prev);
        if (next.has(e.path)) {
          next.delete(e.path);
        } else {
          next.add(e.path);
          if (!cache[e.path]) void load(e.path);
        }
        return next;
      });
    },
    [cache, load],
  );

  // 双击文件 = 交给 redline 渲染（先确保根目录已放行，否则预览 403）
  const openFile = useCallback(
    async (e: Entry) => {
      await ensureAllowed(root);
      setActivePath(e.path);
    },
    [ensureAllowed, root],
  );

  // 在外部应用打开文件/目录（资源管理器/编辑器/终端/Git Bash/reveal）
  const openExternal = useCallback(
    (path: string, app: string) => {
      invoke("open_dir_external", { path, app }).catch((err) =>
        onToast?.(t("打开失败: {e}", { e: String(err) })),
      );
    },
    [onToast, t],
  );

  // 用系统默认程序打开文件（复用 plugin-opener，零新增后端）
  const openDefault = useCallback(
    (path: string) => {
      openPath(path).catch((err) => onToast?.(t("打开失败: {e}", { e: String(err) })));
    },
    [onToast, t],
  );

  const copyPath = useCallback(
    (path: string) => {
      void copyToClipboard(path).then((ok) => onToast?.(ok ? t("已复制路径") : t("复制失败，请手动选中复制")));
    },
    [onToast, t],
  );

  const onContext = useCallback((e: React.MouseEvent, entry: Entry) => {
    e.preventDefault();
    e.stopPropagation();
    setOpenMenu(false);
    setCtxMenu({ x: e.clientX, y: e.clientY, entry });
  }, []);

  // 右键菜单项：文件夹 vs 文件给不同动作（对齐 Codex）
  const ctxItems = useMemo(() => {
    if (!ctxMenu) return [];
    const { entry } = ctxMenu;
    const p = entry.path;
    if (entry.is_dir) {
      return [
        { label: "在资源管理器打开", run: () => openExternal(p, "explorer") },
        { label: "在终端打开", run: () => openExternal(p, "terminal") },
        { label: "在 Git Bash 打开", run: () => openExternal(p, "gitbash") },
        { label: "用 VS Code 打开", run: () => openExternal(p, "vscode") },
        { label: "用 Cursor 打开", run: () => openExternal(p, "cursor") },
        { label: "复制路径", run: () => copyPath(p) },
      ];
    }
    return [
      { label: "预览", run: () => void openFile(entry) },
      { label: "用默认程序打开", run: () => openDefault(p) },
      // 和终端右键菜单同一套动作（同一个后端命令，不另写一份）——见 TermPanel 那处的理由
      ...(isWindows() ? [{ label: "用其他程序打开…", run: () => openExternal(p, "openas") }] : []),
      { label: "在资源管理器中显示", run: () => openExternal(p, "reveal") },
      { label: "用 VS Code 打开", run: () => openExternal(p, "vscode") },
      { label: "复制路径", run: () => copyPath(p) },
    ];
  }, [ctxMenu, openExternal, openDefault, openFile, copyPath]);

  return (
    <div ref={rootRef} className={cn("flex h-full min-h-0", stacked && "flex-col")}>
      {/* 树 */}
      <div
        style={stacked ? { height: "38%" } : { width: treeWidth }}
        className={cn(
          "shrink-0 flex flex-col min-h-0",
          stacked ? "border-b border-white/[0.06]" : "border-r border-white/[0.06]",
        )}
      >
        <div className="flex items-center gap-1 h-8 px-3 border-b border-white/[0.06] shrink-0">
          <span className="text-[11px] text-ink-3 truncate font-mono flex-1" title={root}>
            {root}
          </span>
          {/* 「打开」下拉：把根目录在外部应用打开。走 AnchoredMenu（fixed）——
              这一栏在终端右边时宽度只有两百来像素，absolute 的菜单会被它裁掉。 */}
          <div className="shrink-0">
            <button
              ref={openBtnRef}
              onClick={() => setOpenMenu((v) => !v)}
              className="inline-flex items-center gap-0.5 h-5 px-1.5 rounded text-[11px] text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]"
              title={t("在外部应用打开这个文件夹")}
            >
              <ExternalLink size={11} /> {t("打开")}
            </button>
            {openMenu && (
              <AnchoredMenu
                anchorRef={openBtnRef}
                onClose={() => setOpenMenu(false)}
                minWidth={140}
                t={t}
                items={ROOT_OPEN_APPS.map(({ app, label }) => ({ label, run: () => openExternal(root, app) }))}
              />
            )}
          </div>
          <button
            onClick={() => void load(root)}
            className="inline-flex items-center justify-center w-5 h-5 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] shrink-0"
            title={t("刷新")}
          >
            <RefreshCw size={12} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto py-1 text-[12.5px]">
          <Tree
            dir={root}
            depth={0}
            cache={cache}
            expanded={expanded}
            onToggle={toggle}
            onOpen={(e) => void openFile(e)}
            onContext={onContext}
            activePath={activePath}
          />
        </div>
      </div>

      {/* 预览：redline 通用文档渲染（key=path 换文件即重挂） */}
      <div className="flex-1 min-w-0 min-h-0 flex flex-col">
        {activePath ? (
          <RedlinePanel
            key={activePath}
            host={redlineHost}
            path={activePath}
            fileName={activePath.split(/[\\/]/).pop() ?? activePath}
          />
        ) : (
          <div className="h-full flex items-center justify-center text-ink-4 text-[12px] px-4 text-center">
            {t("双击文件预览（图片 / PDF / Word / Excel / PPT / ZIP / 文本 …）；右键更多操作")}
          </div>
        )}
      </div>

      {/* 右键菜单（fixed 到光标处；点空白/右键关） */}
      {ctxMenu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={() => setCtxMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setCtxMenu(null);
            }}
          />
          <div
            className="fixed z-50 min-w-[168px] rounded-lg bg-bg-1 border border-white/[0.10] shadow-lg py-1"
            style={{ left: Math.min(ctxMenu.x, window.innerWidth - 180), top: Math.min(ctxMenu.y, window.innerHeight - 220) }}
          >
            <div className="px-3 py-1 text-[10px] text-ink-5 truncate max-w-[220px]" title={ctxMenu.entry.name}>
              {ctxMenu.entry.name}
            </div>
            {ctxItems.map((it) => (
              <button
                key={it.label}
                onClick={() => {
                  setCtxMenu(null);
                  it.run();
                }}
                className="w-full text-left px-3 py-1.5 text-[12px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
              >
                {t(it.label)}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function Tree({
  dir,
  depth,
  cache,
  expanded,
  onToggle,
  onOpen,
  onContext,
  activePath,
}: {
  dir: string;
  depth: number;
  cache: Record<string, Entry[]>;
  expanded: Set<string>;
  onToggle: (e: Entry) => void;
  onOpen: (e: Entry) => void;
  onContext: (e: React.MouseEvent, entry: Entry) => void;
  activePath: string | null;
}) {
  const items = cache[dir];
  if (!items) return null;
  return (
    <>
      {items.map((e) => {
        const isOpen = expanded.has(e.path);
        const on = activePath === e.path;
        return (
          <div key={e.path}>
            <div
              onClick={() => (e.is_dir ? onToggle(e) : onOpen(e))}
              onDoubleClick={() => !e.is_dir && onOpen(e)}
              onContextMenu={(ev) => onContext(ev, e)}
              className={
                "flex items-center gap-1 h-6 pr-2 cursor-pointer rounded-sm " +
                (on ? "bg-accent/[0.12] text-ink-0" : "text-ink-2 hover:bg-white/[0.04]")
              }
              style={{ paddingLeft: 8 + depth * 12 }}
              title={e.name}
            >
              {e.is_dir ? (
                <>
                  {isOpen ? <ChevronDown size={12} className="shrink-0 text-ink-4" /> : <ChevronRight size={12} className="shrink-0 text-ink-4" />}
                  {isOpen ? <FolderOpen size={13} className="shrink-0 text-accent-400" /> : <Folder size={13} className="shrink-0 text-ink-3" />}
                </>
              ) : (
                <>
                  <span className="w-3 shrink-0" />
                  <FileIcon size={13} className="shrink-0 text-ink-4" />
                </>
              )}
              <span className="truncate">{e.name}</span>
            </div>
            {e.is_dir && isOpen && (
              <Tree
                dir={e.path}
                depth={depth + 1}
                cache={cache}
                expanded={expanded}
                onToggle={onToggle}
                onOpen={onOpen}
                onContext={onContext}
                activePath={activePath}
              />
            )}
          </div>
        );
      })}
    </>
  );
}
