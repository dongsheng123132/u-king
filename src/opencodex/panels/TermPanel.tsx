/**
 * **U-CLI** —— 任务终端面板。在任务文件夹（cwd）里开终端，跑 claude / codex / hermes 等。
 *
 * 命名：U-Workspace（工作台整体）/ U-Chat（GUI 对话框）/ **U-CLI（就是本文件这一块）**。
 * 约定与理由见 `components/Sidebar.tsx` 的 CORE 注释 —— 一份真相源，这里只是指路。
 *
 * 复用 useTermGroup 引擎。每个任务一个 TermPanel 实例 = 一个独立终端 group。
 * PTY 保活：切任务时父级用 display:none 隐藏，本组件不卸载 → PTY 续跑；
 * 只有「关闭任务」父级才卸载本组件，useTermGroup 的卸载 effect 关掉本任务所有 PTY。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { ChevronDown, ChevronUp, Plus, RotateCw, X } from "lucide-react";
import { useTermGroup } from "../term/useTermGroup";
import { copyToClipboard } from "../../lib/clipboard";
import { isWindows } from "../../lib/platform";
import { AnchoredMenu } from "../../components/AnchoredMenu";
import { useI18n } from "../../i18n";
import "@xterm/xterm/css/xterm.css";

/** 终端顶部「一键启动 AI 工具」快捷按钮 —— **全部可增 / 删 / 自定义（内置默认也不例外）**。
 *  首次使用播种 claude / codex / hermes（**不含 openclaw cli**：产品线已收窄，龙虾走 ClawX 桌面版），
 *  之后完全由用户管理，落 localStorage、所有终端面板共享一份（工作台/各 TUI 应用都显示、都可编辑）。 */
type QuickItem = { label: string; cmd: string };
/** 托管列表存储键（v2）。v1（uking:term-quick-custom）只是用户自定义项，这里首次会迁移进来。 */
const QUICK_KEY = "uking:term-quick-v2";
const LEGACY_CUSTOM_KEY = "uking:term-quick-custom";
const QUICK_EVT = "uking:term-quick-changed";
const MAX_QUICK = 24;
/** 播种默认（老的 QUICK_TOOLS 去掉 openclaw cli）。用户删掉后不再回填（写回完整列表）。 */
const DEFAULT_QUICK: QuickItem[] = [
  { label: "claude", cmd: "claude" },
  { label: "codex", cmd: "codex" },
  { label: "hermes", cmd: "hermes" },
];

function sanitizeQuick(arr: unknown): QuickItem[] {
  if (!Array.isArray(arr)) return [];
  return arr
    .filter(
      (x): x is QuickItem =>
        !!x &&
        typeof (x as QuickItem).label === "string" &&
        typeof (x as QuickItem).cmd === "string" &&
        (x as QuickItem).label.trim() !== "",
    )
    .slice(0, MAX_QUICK)
    .map((x) => ({ label: String(x.label).slice(0, 24), cmd: String(x.cmd).slice(0, 200) }));
}

/** 读托管列表。首次（无 v2 键）= 播种默认 + 迁移旧「自定义」列表（按 label 去重）后落盘。 */
function loadQuick(): QuickItem[] {
  try {
    const raw = localStorage.getItem(QUICK_KEY);
    if (raw != null) return sanitizeQuick(JSON.parse(raw));
    const legacy = sanitizeQuick(JSON.parse(localStorage.getItem(LEGACY_CUSTOM_KEY) || "[]"));
    const seen = new Set<string>();
    const merged: QuickItem[] = [];
    for (const it of [...DEFAULT_QUICK, ...legacy]) {
      const k = it.label.toLowerCase();
      if (seen.has(k)) continue;
      seen.add(k);
      merged.push(it);
    }
    localStorage.setItem(QUICK_KEY, JSON.stringify(merged));
    return merged;
  } catch {
    return [...DEFAULT_QUICK];
  }
}

function saveQuick(items: QuickItem[]) {
  try {
    localStorage.setItem(QUICK_KEY, JSON.stringify(items));
  } catch {
    /* localStorage 不可用时静默 */
  }
  // 通知其它已挂载的终端面板同步（工作台切面板不卸载，靠事件刷新，避免「加/删了别处看不到」）
  try {
    window.dispatchEvent(new Event(QUICK_EVT));
  } catch {
    /* ignore */
  }
}

/**
 * 英文 TUI 的中文小抄 —— 客户原话「很多人对 Claude Code 的英文提示不懂」。
 *
 * 为什么是「小抄」而不是「把它的界面翻译成中文」：那些字是 Claude Code 自己画在终端里的，
 * 我们要覆盖就得逐行拦截+替换，而它每次升级都会改措辞 —— 翻错位比看不懂更糟，
 * 而且那是在跟上游赛跑。小抄贴在终端**外面**：不碰它一个字节，上游怎么改都不会失真。
 *
 * 只写**按键**，不写它界面上的句子：按键是稳定的，句子不是。
 */
const TUI_CHEATSHEET: Record<string, string> = {
  claude: "出现 1./2./3. 时按数字选 · esc=打断它 · ctrl+c 连按两次=退出 · /help=命令表 · @=挑文件 · #=让它记住这句",
  codex: "出现 y/n 时按字母 · esc=打断 · ctrl+c 连按两次=退出 · /help=命令表",
  hermes: "esc=打断 · ctrl+c 连按两次=退出 · /help=命令表 · /model=换模型",
  dsh: "esc=打断 · ctrl+c 连按两次=退出 · /help=命令表",
  qwen: "esc=打断 · ctrl+c 连按两次=退出 · /help=命令表",
  crush: "esc=打断 · ctrl+c 连按两次=退出 · /help=命令表",
  opencode: "esc=打断 · ctrl+c 连按两次=退出 · /help=命令表",
};
/** 关掉小抄记在本地（别每开一个终端都再问一次）。 */
const CHEAT_OFF_KEY = "uking:term-cheatsheet-off";

/** TermPanel 暴露给父级的命令式接口：runCmd=当前标签；runCmdNew=新标签；paste=贴文本不回车。 */
export type TermPanelApi = {
  runCmd: (cmd: string) => void;
  runCmdNew: (cmd: string) => void;
  paste: (text: string) => void;
};

export function TermPanel({
  cwd,
  active,
  tool,
  initialCmd,
  prompts,
  onReady,
  onOpenFile,
  onToast,
}: {
  cwd: string;
  active: boolean;
  tool?: string;
  initialCmd?: string;
  /** 点终端里的文件路径 → 交给宿主预览（工作台传 openFromTerminal）。不传则退回「用默认程序打开」。 */
  onOpenFile?: (path: string) => void;
  /** 复制/打开失败的轻提示。不传则静默。 */
  onToast?: (msg: string) => void;
  /** 应用专属的启动按钮（如 ToolAppView 传该 app 的 prompts）。这些**不可编辑**，
   *  与下方"可增删自定义的托管快捷列表"并排显示。不传则只显示托管列表。 */
  prompts?: { label: string; cmd: string }[];
  /** 挂载后回调，透出 runCmd 给父级（一键启动 WebUI 等场景）。 */
  onReady?: (api: TermPanelApi) => void;
}) {
  const { t: tr } = useI18n();
  // 拖文件进终端 = 贴路径到当前输入行（逻辑集中在 useTermGroup，dropOver 用于高亮）
  const { hostRef, tabs, activeKey, setActiveKey, newTerm, closeTerm, restartTerm, moveTab, runInActive, runInNew, writeToActive, scrollActive, dropOver, fileMenu, closeFileMenu, activeTui } = useTermGroup({
    open: active,
    cwd,
    tool,
    initialCmd,
    // 没有宿主预览时（独立终端页 / TUI 应用页）左键就用系统默认程序打开 —— 总比点了没反应强
    onOpenFile: onOpenFile ?? ((p) => void openPath(p).catch((e) => onToast?.(tr("打开失败: {e}", { e: String(e) })))),
  });

  /* ── 标签拖动排序 ──────────────────────────────────────────────────────────
   * 只改顺序（moveTab 同时重排渲染序和实例序），不碰 xterm 也不碰 PTY。
   *
   * 🔴 **必须用 pointer 事件，不能用 HTML5 draggable**：`tauri.conf.json` 开了
   * `dragDropEnabled: true`（为了拖文件进终端能拿到真实路径，见 `lib/fileDrop.ts` 的头注释），
   * 代价是 webview 的 HTML5 拖放整体失效 —— dragstart/dragover/drop 一个都不来。
   *
   * 激活标签也挪到 pointerup 里做（原来是 onClick）：拖完抬手浏览器照样补一次 click，
   * 留着 onClick 就会「拖一下顺手把标签切了」。
   */
  const stripRef = useRef<HTMLDivElement | null>(null);
  // 落点同时进 ref：pointerup 里要用的是**最后一次 move 算出来的**那个，不是某次渲染闭包里的
  const dragRef = useRef<{ key: number; x: number; y: number; moved: boolean; before: number | null } | null>(null);
  const [dragKey, setDragKey] = useState<number | null>(null);
  const [dropBefore, setDropBefore] = useState<number | null>(null);

  const onTabPointerDown = (e: React.PointerEvent, key: number) => {
    // 关闭 ×、重开 ⟳ 归它们自己的 onClick，别被拖动吃掉
    if ((e.target as HTMLElement).closest("button")) return;
    if (e.button !== 0) return;
    dragRef.current = { key, x: e.clientX, y: e.clientY, moved: false, before: null };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onTabPointerMove = (e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    // 阈值：手抖几个像素不算拖，否则每次点标签都进拖动态
    if (!d.moved && Math.abs(e.clientX - d.x) < 4 && Math.abs(e.clientY - d.y) < 4) return;
    if (!d.moved) {
      d.moved = true;
      setDragKey(d.key);
    }
    // 落点 = 第一个「鼠标还在它左半边」的标签；都不满足就是挪到最后
    const els = Array.from(stripRef.current?.querySelectorAll<HTMLElement>("[data-tab-key]") ?? []);
    let before: number | null = null;
    for (const el of els) {
      const k = Number(el.dataset.tabKey);
      if (k === d.key) continue;
      const r = el.getBoundingClientRect();
      if (e.clientX < r.left + r.width / 2) {
        before = k;
        break;
      }
    }
    d.before = before;
    setDropBefore(before);
  };

  const onTabPointerUp = (e: React.PointerEvent) => {
    const d = dragRef.current;
    dragRef.current = null;
    if (!d) return;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    if (d.moved) moveTab(d.key, d.before);
    else setActiveKey(d.key);
    setDragKey(null);
    setDropBefore(null);
  };

  // 右键终端里的文件路径 → 这几件事。和文件面板右键菜单同一套动作（同样的后端命令，不另写一份）。
  const fileMenuItems = useMemo(() => {
    if (!fileMenu) return [];
    const p = fileMenu.path;
    const ext = (app: string) => () => invoke("open_dir_external", { path: p, app }).catch((e) => onToast?.(tr("打开失败: {e}", { e: String(e) })));
    // 🔴 磁盘上根本没这个文件时，**别给一排必然失败的按钮**。
    // 客户 2026-08-16 撞的就是这个：终端那行只写了文件名、目录在上面另一行，
    // 按「终端当前目录」拼出来的路径不存在 —— 预览 404、默认程序打开报错、
    // 而「复制路径」还会不声不响地复制一条错路径。现在先说清楚都试过哪儿，
    // 只留两条**在字符串层面仍然有意义**的（复制文件名 / 贴到输入行让用户自己补目录）。
    if (fileMenu.missing) {
      const tried = fileMenu.candidates.join("\n");
      const name = p.split(/[\\/]/).pop() ?? p;
      return [
        { label: "⚠ 找不到这个文件 —— 看看都试过哪儿", run: () => onToast?.(tr("这些位置都没有「{name}」：\n{tried}", { name, tried })) },
        { label: "复制文件名", run: () => void copyToClipboard(name).then((ok) => onToast?.(ok ? tr("已复制文件名") : tr("复制失败，请手动选中复制"))) },
        { label: "贴到输入行", run: () => writeToActive(/\s/.test(name) ? `"${name}" ` : `${name} `) },
      ];
    }
    // 所在目录（右键的是文件时取它的父目录；是目录时就是它自己）
    const dir = fileMenu.isDir ? p.replace(/[\\/]+$/, "") : /[\\/]/.test(p) ? p.replace(/[\\/][^\\/]*$/, "") : p;
    // 右键的是**目录**（终端里 `demo\SUBMIT-xxx\` 这种）：给的是「进去」的动作，不是预览。
    // 客户原话：「比如打开对应的文件夹」。预览一个目录只会报错。
    if (fileMenu.isDir) {
      return [
        { label: "打开这个文件夹", run: ext("explorer") },
        { label: "切到这个目录（cd）", run: () => runInActive(/\s/.test(dir) ? `cd "${dir}"` : `cd ${dir}`) },
        { label: "在系统终端里打开这个目录", run: ext("terminal") },
        { label: "用 VS Code 打开", run: ext("vscode") },
        { label: "复制路径", run: () => void copyToClipboard(p).then((ok) => onToast?.(ok ? tr("已复制路径") : tr("复制失败，请手动选中复制"))) },
        { label: "贴到输入行", run: () => writeToActive(/\s/.test(p) ? `"${p}" ` : `${p} `) },
      ];
    }
    return [
      { label: "预览", run: () => (onOpenFile ? onOpenFile(p) : void openPath(p)) },
      { label: "用默认程序打开", run: () => void openPath(p).catch((e) => onToast?.(tr("打开失败: {e}", { e: String(e) }))) },
      // 「用其他程序打开…」：调出系统的**打开方式**选择框。
      // 上面那条只会用已注册的默认程序开；客户常想的是「这个 zip 用 7-Zip、这个 md 用 Typora」，
      // 而硬编码一个 VS Code 永远只覆盖我们想到的那几个 —— 让系统把他装的都列出来。
      // 🔴 「打开方式」对话框是 Windows 专属（`OpenAs_RunDLL`）。Mac 上后端只会报错 ——
      //    别把点了必然失败的按钮摆给他看（这次会话开头修的就是这一类）。
      ...(isWindows() ? [{ label: "用其他程序打开…", run: ext("openas") }] : []),
      { label: "在资源管理器中显示", run: ext("reveal") },
      { label: "用 VS Code 打开", run: ext("vscode") },
      // 「切到这个目录」：直接在**当前这个终端**里 cd 过去（客户原话「我们切换到目录里边」）。
      // 不新开窗口 —— 他正在这个终端里干活，换个 cwd 接着干才是他要的。
      { label: "切到这个目录（cd）", run: () => runInActive(/\s/.test(dir) ? `cd "${dir}"` : `cd ${dir}`) },
      { label: "在系统终端里打开这个目录", run: ext("terminal") },
      { label: "复制路径", run: () => void copyToClipboard(p).then((ok) => onToast?.(ok ? tr("已复制路径") : tr("复制失败，请手动选中复制"))) },
      { label: "贴到输入行", run: () => writeToActive(/\s/.test(p) ? `"${p}" ` : `${p} `) },
    ];
  }, [fileMenu, onOpenFile, onToast, tr, writeToActive, runInActive]);
  // 应用专属启动按钮（不可编辑）；不传则为空，只显示下方托管列表。
  const promptButtons = prompts ?? [];

  // 托管快捷列表（内置默认 + 用户增改，全部可删/可加）—— 全局落 localStorage，所有终端面板共享一份。
  const [quickItems, setQuickItems] = useState<QuickItem[]>(loadQuick);
  const [adding, setAdding] = useState(false);
  const addBtnRef = useRef<HTMLButtonElement>(null);
  const [newLabel, setNewLabel] = useState("");
  const [newCmd, setNewCmd] = useState("");

  // 中文小抄：跑起英文 TUI 才出现；关过一次就永久不出（记 localStorage）
  const [cheatOff, setCheatOff] = useState(() => {
    try {
      return localStorage.getItem(CHEAT_OFF_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [zhBusy, setZhBusy] = useState(false);
  const cheat = !cheatOff && activeTui ? TUI_CHEATSHEET[activeTui] : null;

  /** 「让 AI 说中文」= 把我们那一块指针写进各家记忆文件（里面第一句就是「用简体中文回答」）。
   *  复用「我的 U-King」页那个已有动作，不另写一条路 —— 也就能在那页原样撤销。 */
  const setZh = async () => {
    setZhBusy(true);
    try {
      await invoke("link_identity", { linked: true, targets: ["claude"] });
      onToast?.(tr("已让 Claude Code 用中文回答（下次开新会话生效；在「我的 U-King」可撤销）"));
    } catch (e) {
      onToast?.(tr("设置失败: {e}", { e: String(e) }));
    } finally {
      setZhBusy(false);
    }
  };

  // 别处新增/删除时同步本面板（保活面板不重挂，只能靠事件刷新）
  useEffect(() => {
    const sync = () => setQuickItems(loadQuick());
    window.addEventListener(QUICK_EVT, sync);
    return () => window.removeEventListener(QUICK_EVT, sync);
  }, []);

  const addQuick = () => {
    const label = newLabel.trim();
    if (!label) return;
    const cmd = newCmd.trim() || label; // 留空 = 同按钮文字
    const next = [...quickItems, { label: label.slice(0, 24), cmd: cmd.slice(0, 200) }].slice(0, MAX_QUICK);
    setQuickItems(next);
    saveQuick(next);
    setNewLabel("");
    setNewCmd("");
    setAdding(false);
  };

  const removeQuick = (i: number) => {
    const next = quickItems.filter((_, j) => j !== i);
    setQuickItems(next);
    saveQuick(next);
  };

  // 透出当前标签 / 新标签执行与粘贴给父级。
  useEffect(() => {
    onReady?.({ runCmd: runInActive, runCmdNew: runInNew, paste: writeToActive });
  }, [onReady, runInActive, runInNew, writeToActive]);

  return (
    <div className="relative flex flex-col h-full min-h-0">
      {/* 拖文件高亮 */}
      {dropOver && (
        <div className="absolute inset-0 z-30 rounded border-2 border-dashed border-accent/60 bg-accent/[0.06] grid place-items-center pointer-events-none">
          <div className="text-accent text-[13px] font-semibold">{tr("松手把文件路径贴进终端")}</div>
        </div>
      )}
      {/* 标签栏（relative z-20 让下拉的「自定义」浮层盖在下方 xterm 之上）
       *
       * 🔴 **必须能换行，且高度不能写死。** 这一条里塞着两组东西：左边是终端页签
       * （每个带「关闭此终端」✕）、右边是快捷词 chip（每个带「删除此快捷词」✕）。
       * 原来是 `h-8` 固定高 + 单行不换行，于是窗口一窄，两组控件直接**叠在一起**：
       *
       *   1120×720（tauri.conf.json 里的默认窗口）→ 3 处重叠，其中
       *     「关闭此终端」和「删除此快捷词」只差 4px —— 两个都是破坏性操作，点错就没了
       *   900×600（配置的最小窗口）→ 4 处重叠，另有 2 个控件被挤出右边界，根本点不到
       *
       * 左边那组本来有 `flex-1 min-w-0 overflow-x-auto`，看着是对的，但右边那组是
       * `shrink-0` 永不让位 —— 实测左边被压到 **14px**，页签的 ✕ 于是溢出到右边那组身上。
       *
       * 试过两条改不通的路，别再走：
       *   ① 只把右组的 `shrink-0` 换成 `min-w-0 overflow-x-auto` —— 重叠一处没少，
       *      反而把 3 个控件推出屏幕（右组内容 273px，压到 156px 也不肯裁）
       *   ② 给左组设 min-width —— 同上，右组照样溢出
       * 真正有效的是**让整条换行**：窄了就把快捷词组挪到第二行，各自拿到完整宽度。
       * 实测 1120 与 900 下重叠均归零。
       */}
      <div className="relative z-20 flex flex-wrap items-center min-h-8 px-2 py-0.5 gap-x-1 gap-y-1 border-b border-white/[0.06] bg-bg-1 shrink-0">
        <div ref={stripRef} className="flex items-center gap-1 flex-1 min-w-[132px] overflow-x-auto">
          {tabs.map((t) => (
            <div
              key={t.key}
              data-tab-key={t.key}
              onPointerDown={(e) => onTabPointerDown(e, t.key)}
              onPointerMove={onTabPointerMove}
              onPointerUp={onTabPointerUp}
              onPointerCancel={onTabPointerUp}
              className={
                "group flex items-center gap-1.5 h-6 pl-2.5 pr-1.5 rounded cursor-pointer text-[12px] shrink-0 touch-none " +
                (t.key === activeKey ? "bg-accent/[0.12] text-ink-0" : "text-ink-3 hover:bg-white/[0.04]") +
                (t.key === dragKey ? " opacity-40" : "") +
                (t.key === dropBefore && dragKey != null ? " shadow-[inset_2px_0_0_0_rgb(var(--color-accent))]" : "")
              }
            >
              {/* 进程退了要看得出来（绿点原来是写死的），并给一键重开 */}
              <span className={"dot " + (t.dead ? "dot-off" : "dot-on")} />
              <span className={"whitespace-nowrap" + (t.dead ? " text-ink-4 opacity-60" : "")}>{t.title}</span>
              {t.dead && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    restartTerm(t.key);
                  }}
                  className="inline-flex items-center justify-center h-4 px-1 rounded text-[10px] text-accent-400 hover:bg-accent/[0.14]"
                  title={tr("进程已退出，点这里重开")}
                >
                  <RotateCw size={10} />
                </button>
              )}
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  closeTerm(t.key);
                }}
                className="opacity-0 group-hover:opacity-100 inline-flex items-center justify-center w-4 h-4 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
                title={tr("关闭此终端")}
              >
                <X size={11} />
              </button>
            </div>
          ))}
          <button
            onClick={() => newTerm()}
            className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-3 hover:text-ink-1 hover:bg-white/[0.06] shrink-0"
            title={tr("新建终端")}
          >
            <Plus size={14} />
          </button>
        </div>
        {/* 不借用 ↑/↓：那是 Codex TUI 的输入历史 / 菜单键。这里滚的是 xterm 回滚缓冲；
            鼠标滚轮同样可回看。 */}
        <div className="flex items-center gap-0.5 shrink-0">
          <button
            type="button"
            onClick={() => scrollActive(-8)}
            className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]"
            title={tr("回看较早输出（也可在终端内滚动鼠标滚轮）")}
            aria-label={tr("回看较早输出")}
          >
            <ChevronUp size={14} />
          </button>
          <button
            type="button"
            onClick={() => scrollActive(Number.POSITIVE_INFINITY)}
            className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]"
            title={tr("回到最新输出")}
            aria-label={tr("回到最新输出")}
          >
            <ChevronDown size={14} />
          </button>
        </div>
        {/* 应用专属启动按钮（不可编辑）+ 可增删自定义的托管快捷列表（在当前终端跑）
         * 换到第二行时那条竖分隔线就没意义了（它是用来跟左边页签分栏的），靠 `[&:first-child]`
         * 判断不了，所以用 max-* 断点：窄屏本来就会换行，直接不画线。 */}
        <div className="relative flex flex-wrap items-center gap-1 min-w-0 pl-0 ml-0 border-l-0 sm:pl-2 sm:ml-1 sm:border-l sm:border-white/[0.06]">
          {/* 应用专属按钮（ToolAppView 传入，不可删） */}
          {promptButtons.map((q) => (
            <button
              key={`p-${q.label}`}
              onClick={() => runInActive(q.cmd)}
              title={tr("在终端运行：{cmd}", { cmd: q.cmd })}
              className="h-6 px-2 rounded text-[11px] text-ink-3 hover:text-ink-0 hover:bg-accent/[0.14] transition-colors shrink-0"
            >
              {q.label}
            </button>
          ))}
          {/* 托管快捷列表（内置默认 + 用户增改，悬停出删除叉；全部可删/可加） */}
          {quickItems.map((q, i) => (
            <span key={`q-${i}-${q.label}`} className="group/cq relative inline-flex items-center shrink-0">
              <button
                onClick={() => runInActive(q.cmd)}
                title={tr("在终端运行：{cmd}", { cmd: q.cmd })}
                className="h-6 pl-2 pr-2 rounded text-[11px] text-ink-3 hover:text-ink-0 hover:bg-accent/[0.14] transition-colors"
              >
                {q.label}
              </button>
              <button
                onClick={() => removeQuick(i)}
                title={tr("删除此快捷词")}
                className="opacity-0 group-hover/cq:opacity-100 inline-flex items-center justify-center w-3.5 h-3.5 -ml-0.5 mr-0.5 rounded text-ink-4 hover:text-red-400 hover:bg-white/[0.08]"
              >
                <X size={9} />
              </button>
            </span>
          ))}
          {/* + 自定义 */}
          <button
            ref={addBtnRef}
            onClick={() => setAdding((v) => !v)}
            title={tr("添加自定义快捷词")}
            className="inline-flex items-center gap-0.5 h-6 px-1.5 rounded text-[11px] text-ink-4 hover:text-accent-400 hover:bg-accent/[0.10] transition-colors shrink-0"
          >
            <Plus size={11} /> {tr("自定义")}
          </button>
          {adding && (
            /* 走 AnchoredMenu（fixed）而不是 absolute：终端右边开了文件栏之后这一列会变窄，
               264px 的浮层贴 right-0 很容易被右面板的 overflow-hidden 切掉半边。 */
            <AnchoredMenu anchorRef={addBtnRef} onClose={() => setAdding(false)} minWidth={264}>
              <div className="w-[264px] p-3">
                <div className="text-[11px] text-ink-4 mb-2">{tr("添加快捷词（点按钮即发进终端执行）")}</div>
                <input
                  autoFocus
                  value={newLabel}
                  maxLength={24}
                  onChange={(e) => setNewLabel(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addQuick()}
                  placeholder={tr("按钮文字（如 /model）")}
                  className="w-full h-8 px-2 mb-2 rounded bg-bg-0 border border-white/[0.10] text-[12px] text-ink-0 placeholder:text-ink-5 outline-none focus:border-accent/40"
                />
                <input
                  value={newCmd}
                  maxLength={200}
                  onChange={(e) => setNewCmd(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addQuick()}
                  placeholder={tr("发送的命令（留空=同按钮文字）")}
                  className="w-full h-8 px-2 mb-2.5 rounded bg-bg-0 border border-white/[0.10] text-[12px] text-ink-0 placeholder:text-ink-5 outline-none focus:border-accent/40"
                />
                <div className="flex items-center justify-end gap-2">
                  <button
                    onClick={() => setAdding(false)}
                    className="h-7 px-3 rounded text-[12px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.06]"
                  >
                    {tr("取消")}
                  </button>
                  <button
                    onClick={addQuick}
                    disabled={!newLabel.trim()}
                    className="h-7 px-3 rounded text-[12px] font-medium bg-accent text-white disabled:opacity-40 hover:bg-accent-600"
                  >
                    {tr("添加")}
                  </button>
                </div>
              </div>
            </AnchoredMenu>
          )}
        </div>
      </div>
      {/* 终端宿主 */}
      <div ref={hostRef} className="relative flex-1 min-h-0" />

      {/* 中文小抄：只有真跑起英文 TUI 时才出现，一行，可永久关掉 */}
      {cheat && (
        <div className="shrink-0 flex items-center gap-2 h-6 px-2 border-t border-white/[0.06] bg-bg-1/80 text-[10.5px] text-ink-3">
          <span className="shrink-0 text-accent/90">{tr("中文小抄")}</span>
          <span className="truncate" title={cheat}>{cheat}</span>
          <button
            onClick={() => void setZh()}
            disabled={zhBusy}
            title={tr("往 ~/.claude/CLAUDE.md 追加一行「用简体中文回答」（只增不删，可在「我的 U-King」里撤销）")}
            className="ml-auto shrink-0 h-5 px-1.5 rounded text-[10.5px] text-accent hover:bg-accent/[0.14] disabled:opacity-40"
          >
            {tr("让 AI 说中文")}
          </button>
          <button
            onClick={() => {
              setCheatOff(true);
              try {
                localStorage.setItem(CHEAT_OFF_KEY, "1");
              } catch {
                /* ignore */
              }
            }}
            title={tr("不再显示")}
            className="shrink-0 inline-flex items-center justify-center w-4 h-4 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"
          >
            <X size={10} />
          </button>
        </div>
      )}

      {/* 右键终端里的文件路径 → 预览 / 打开方式 / 复制路径（fixed 到光标处；点空白关） */}
      {fileMenu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={closeFileMenu}
            onContextMenu={(e) => {
              e.preventDefault();
              closeFileMenu();
            }}
          />
          <div
            className="fixed z-50 min-w-[176px] rounded-lg bg-bg-1 border border-white/[0.10] shadow-lg py-1"
            style={{
              left: Math.min(fileMenu.x, window.innerWidth - 190),
              top: Math.min(fileMenu.y, window.innerHeight - 220),
            }}
          >
            <div className="px-3 py-1 text-[10px] text-ink-5 truncate max-w-[240px]" title={fileMenu.path}>
              {fileMenu.path.split(/[\\/]/).pop()}
            </div>
            {fileMenuItems.map((it) => (
              <button
                key={it.label}
                onClick={() => {
                  closeFileMenu();
                  it.run();
                }}
                className="block w-full text-left px-3 py-1.5 text-[12px] text-ink-2 hover:bg-white/[0.06] hover:text-ink-0"
              >
                {tr(it.label)}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
