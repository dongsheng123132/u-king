/**
 * useTermGroup —— 一组终端会话的管理引擎（从 Terminal.tsx 抽出，复用）。
 *
 * 一个 group = 一个 host 容器里多个 xterm 标签，每个标签一个独立 PTY 会话。
 * 独立终端页（TerminalPage）、工作台每个任务的终端面板（TermPanel）、TUI 应用页各持有一个 group，
 * 互不污染。切标签/隐藏 group 只切 display，不杀 PTY（openclaw gateway 等长跑进程续命）。
 *
 * 与原 Terminal.tsx 的两点差异（计划要求）：
 *  1. ensurePty 传 cwd —— 工作台按任务文件夹开终端（cwd 为空 → 后端回落 home，抽屉行为不变）
 *  2. 终端序号 seq 改成实例内 useRef —— 多任务/多 group 不再共用模块级全局，避免 key 撞
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";
import { registerDropZone, pathsToText } from "../../lib/fileDrop";
import { claimSession, releaseSession } from "./registry";
import { registerFileLinks } from "./fileLinks";
import { anchorImeToCursor } from "./imeAnchor";
import { createTermInputQueue, type TermInputQueue } from "./inputQueue";
import { createPaintRepair, type PaintRepair } from "./paintRepair";
// 右键菜单是原生 DOM 建的，够不到 React context —— 用非 hook 版翻译
import { translate } from "../../i18n";

/**
 * 终端配色主题 —— 三套预设 + 完整 16 色 ANSI 调色板。
 *
 * 为什么补 16 色：老主题只有 6 个色值，ls/git 的彩色输出全落成灰白，终端看起来
 * 就是一片死黑（小白看不懂）。补上后「绿色=成功 / 红色=报错」一眼可辨。
 * 为什么多套：黑底对一部分人就是劝退，给个浅色护眼 + 复古绿，标签栏调色板按钮随时切。
 * 模块级管理（不经过 React state 传递）：终端页 / 工作台面板 / TUI 应用页共用一套主题，
 * 切了全局生效；已建 xterm 通过 subscribeTermTheme 即时换肤。
 */
export const TERM_THEMES = {
  dark: {
    background: "#0d0d0f",
    foreground: "#f7f8f8",
    cursor: "#5e6ad2",
    cursorAccent: "#0d0d0f",
    selectionBackground: "rgba(255,255,255,0.10)",
    black: "#1b1b1f",
    red: "#ff6b6b",
    green: "#7ee787",
    yellow: "#e3b341",
    blue: "#79b8ff",
    magenta: "#d2a8ff",
    cyan: "#76e3ea",
    white: "#e3e4e6",
    brightBlack: "#6b7280",
    brightRed: "#ff8f8f",
    brightGreen: "#9ff0a8",
    brightYellow: "#ffd76e",
    brightBlue: "#a5c8ff",
    brightMagenta: "#e4c4ff",
    brightCyan: "#a8f0f4",
    brightWhite: "#f7f8f8",
  },
  light: {
    background: "#f7f7f5",
    foreground: "#24292f",
    cursor: "#5e6ad2",
    cursorAccent: "#f7f7f5",
    selectionBackground: "rgba(94,106,210,0.18)",
    black: "#24292f",
    red: "#cf222e",
    green: "#116329",
    yellow: "#9a6700",
    blue: "#0969da",
    magenta: "#8250df",
    cyan: "#1b7c83",
    white: "#6e7781",
    brightBlack: "#57606a",
    brightRed: "#a40e26",
    brightGreen: "#1a7f37",
    brightYellow: "#bf8700",
    brightBlue: "#218bff",
    brightMagenta: "#a475f9",
    brightCyan: "#3192aa",
    brightWhite: "#6e7781",
  },
  green: {
    background: "#001400",
    foreground: "#00ff66",
    cursor: "#00ff66",
    cursorAccent: "#001400",
    selectionBackground: "rgba(0,255,102,0.25)",
    black: "#001400",
    red: "#ff5555",
    green: "#00ff66",
    yellow: "#ffcc00",
    blue: "#66ccff",
    magenta: "#cc88ff",
    cyan: "#66ffff",
    white: "#c8d8c8",
    brightBlack: "#005f00",
    brightRed: "#ff8888",
    brightGreen: "#66ff99",
    brightYellow: "#ffee66",
    brightBlue: "#99ddff",
    brightMagenta: "#ddbbff",
    brightCyan: "#99ffff",
    brightWhite: "#ffffff",
  },
} as const;

export type TermThemeId = keyof typeof TERM_THEMES;

const TERM_THEME_KEY = "uking.termTheme";
function loadThemeId(): TermThemeId {
  const v = localStorage.getItem(TERM_THEME_KEY);
  return v === "light" || v === "green" || v === "dark" ? v : "dark";
}
let currentTermTheme: TermThemeId = loadThemeId();
const themeListeners = new Set<() => void>();

export function getTermThemeId(): TermThemeId {
  return currentTermTheme;
}
export function getGlobalTermTheme(): (typeof TERM_THEMES)[TermThemeId] {
  return TERM_THEMES[currentTermTheme];
}
/** 全局切主题：所有 useTermGroup 实例的已建 xterm 即时换肤 + 新开的终端用新主题。 */
export function setGlobalTermTheme(id: TermThemeId) {
  currentTermTheme = id;
  try {
    localStorage.setItem(TERM_THEME_KEY, id);
  } catch {
    /* ignore */
  }
  themeListeners.forEach((fn) => fn());
}
function subscribeTermTheme(fn: () => void): () => void {
  themeListeners.add(fn);
  return () => {
    themeListeners.delete(fn);
  };
}

type TermSession = {
  key: number;
  title: string;
  term: XTerm;
  fit: FitAddon;
  sessionId: string | null; // 后端 PTY id（懒启动后填）
  /** 正在飞的 term_open —— 并发调用共用它，避免同一个 xterm 开出两个 PTY（见 ensurePty） */
  pending: Promise<string | null> | null;
  /** 对面进程已退出（收到后端的 EOF 空帧）。标签置灰 + 出「重开」，不再往里写。 */
  dead: boolean;
  /** 标签已被用户关掉，xterm 已 dispose —— 后端随后发来的收尾帧不能再往里写。 */
  closed: boolean;
  el: HTMLDivElement; // 该终端容器（一直存在，靠 display 切换）
  /** GPU 渲染器（拿不到 WebGL 上下文时为 undefined）。**拆终端时要先单独收它**，见 disposeTerm。 */
  webgl?: WebglAddon;
  /** 文件路径链接的反注册（link provider + contextmenu 监听） */
  unlink?: () => void;
  /** 输入法候选条锚定的反注册（见 imeAnchor.ts） */
  unanchor?: () => void;
  /** xterm → PTY 的前端 FIFO：建连前缓存、建连后串行、失败不再静默吞字。 */
  input: TermInputQueue;
  /** Windows DOM 渲染器在输出停顿后用缓冲区全屏校正，清掉缺笔/叠字。 */
  paintRepair?: PaintRepair;
  /** 上一次真发给后端的尺寸 —— 用来把「没变也发」的 resize 掐掉（见 fitActive） */
  sentSize?: { cols: number; rows: number };
  /** 每个标签自己的启动环境；普通 group 沿用默认值，升级恢复可逐条覆盖。 */
  cwd?: string | null;
  tool?: string | null;
  initialCmd?: string | null;
  restartCmd?: string | null;
  /** 升级快照的稳定条目号；重试时复用同一个 xterm，绝不把已成功项再开一遍。 */
  restoreKey?: number;
};

/** 升级快照的一条可重开记录；它描述的是新 PTY 的启动条件，不是旧会话的复活。 */
export type TermRestore = { cwd: string | null; cmd: string | null; tool: string | null; restoreKey?: number };

/** 在终端里右键某个文件路径时弹的菜单（坐标是视口坐标，调用方 fixed 定位）。 */
export type TermFileMenu = {
  x: number;
  y: number;
  /** 已经**验过存在**的那个路径（候选里第一个真的在磁盘上的）。都不在时退回第一候选。 */
  path: string;
  /** 一个都没找到 —— 调用方据此把菜单降级成「找不到这个文件」而不是给一排必然失败的按钮。 */
  missing?: boolean;
  /** 全部候选（诊断用：告诉用户我们都试了哪儿）。 */
  candidates: string[];
  /** 命中的是个目录 —— 菜单该出「打开文件夹」而不是「预览」。 */
  isDir?: boolean;
};

/**
 * 一串候选路径里，第一个**真的在磁盘上**的那个。全都不在就返回 null。
 *
 * 借 `produced_file_info`（fs.rs，只读 metadata），不新增后端命令。
 * 逐个串行问：候选最多 6 个，而第一个通常就中，没必要并发。
 */
async function firstExisting(candidates: string[]): Promise<{ path: string; isDir: boolean } | null> {
  for (const c of candidates.slice(0, 6)) {
    try {
      // `exists` 只算文件；目录走 `is_dir`（fs.rs 那个字段就是为这条加的）——
      // 终端里的链接现在也认目录了，只看 exists 会把每个文件夹判成「找不到」。
      const info = await invoke<{ exists: boolean; is_dir: boolean }>("produced_file_info", { path: c });
      if (info?.exists) return { path: c, isDir: false };
      if (info?.is_dir) return { path: c, isDir: true };
    } catch {
      /* 问不到就当不在，接着试下一个 */
    }
  }
  return null;
}

/**
 * 关掉后端 PTY。
 *
 * ★ 必须处理「`term_open` 还在飞」这一种：那会儿 `sessionId` 还是 null，老代码的
 * `if (s.sessionId) term_close(...)` 直接跳过 → PTY 随后才被建出来，成了永远没人认领的
 * 孤儿 shell 进程。触发窗口非常现成：**首次开终端要先下 106MB 的 PowerShell 7**，那几十秒里
 * 点一下标签上的 × 就必漏。（69f841c 修的是「并发开出两个 PTY」，这是同一类的另一条路：
 * 开到一半就关。）
 */
/**
 * 拆掉一个终端的前端部分（link provider → WebGL → xterm 本体）。**每一步都不许把异常放出去。**
 *
 * 🔴 这不是「防御性编程」凑数，是 2026-08-16 客户 + 本机实锤的崩溃（issue #402 / #403）：
 * `s.term.dispose()` 走到 xterm 的 `AddonManager.dispose()` → `WebglAddon.dispose()` 时抛
 * `Cannot read properties of undefined (reading '_isDisposed')`。而这两处调用点
 * （`closeTerm` 和卸载 cleanup）**都在 React 的生命周期里** —— effect cleanup 抛出来的错
 * React 会往上冒到 ErrorBoundary，于是整棵树被卸掉、满屏变成「U-King 遇到问题，界面已停止」。
 * 客户看到的就是「用着用着 U-King 自己重启了」，而他真正做的只是**切了一下大脑**
 * （`Chat.tsx` 里 engine 从 hermes 切到轻助手 = 中间那个 TermPanel 卸载 = 走到这里）。
 *
 * 为什么「吞掉」是对的而不是掩盖：这条路上的对象**正在被扔掉**，没有任何下游依赖它的返回值；
 * 而放它出去的代价是整个界面。收尾失败最坏是漏一点 GPU 资源，一次误伤是整个 App。
 *
 * WebGL 单独先收：xterm 的 AddonManager 是**一个 addon 抛了就中断整轮**，先把最容易抛的那个
 * 拿出来自己收，剩下的 addon（FitAddon 等）才收得干净。
 */
function disposeTerm(s: TermSession) {
  try {
    s.paintRepair?.close();
  } catch {
    /* 只是在撤计时器；收尾一律不许抛 */
  }
  try {
    s.unanchor?.();
  } catch {
    /* textarea 早随 xterm 没了 —— 同上，收尾一律不许抛 */
  }
  try {
    s.unlink?.();
  } catch {
    /* link provider 已经没了也无所谓 */
  }
  try {
    s.webgl?.dispose();
  } catch {
    /* 上下文早丢了 / 已被 onContextLoss 收过 —— 正是崩在这儿的那一步 */
  }
  s.webgl = undefined;
  try {
    s.term.dispose();
  } catch {
    /* xterm 内部收尾抛错，不能让它把整个界面带走 */
  }
}

function closeBackend(s: TermSession) {
  s.input.close();
  if (s.sessionId) {
    releaseSession(s.sessionId); // 心跳不再上报它 —— 万一 term_close 没送到，后端也会自己收尸
    invoke("term_close", { sessionId: s.sessionId }).catch(() => {});
    s.sessionId = null;
    return;
  }
  const pending = s.pending;
  if (pending) {
    void pending.then((sid) => {
      if (sid) {
        releaseSession(sid);
        invoke("term_close", { sessionId: sid }).catch(() => {});
      }
    });
  }
}

/**
 * 超过这么多字节的文本粘贴，不再直接灌进 PTY，改成落成 .txt 只把路径贴进去。
 *
 * 为什么：Windows 的 ConPTY 会**吃掉括号粘贴标记**（ESC[200~ / ESC[201~，实测标记必丢、
 * 正文一字节不丢），还把一次粘贴拆成 ~1KB 一块。于是 TUI 把一次粘贴当成几十次独立输入 ——
 * 例如 hermes 的粘贴片段上限是 32 片，超了就从**最早的**开始丢，客户看到的就是
 * 「粘进去的内容被截断」。落文件贴路径绕开整条 ConPTY 输入链，且 claude / codex / hermes
 * 都能直接读文件。阈值取 8KB：日常粘贴照旧原样进终端，只接管明显是「一大段」的那种。
 */
const PASTE_TO_FILE_BYTES = 8 * 1024;

/**
 * ★ 伪终端信息（Windows 下必须告诉 xterm.js 对面是 ConPTY）。
 *
 * 不传会怎样：ConPTY 到行尾自己插换行、且不打 wrapped 标记，xterm.js 不知情就按自己那套
 * 再折一次 → 同一段文字前端占的行数比应用以为的多。TUI 应用（Claude Code 的多行输入框
 * 是典型）每次按键都发「光标上移 N 行、清掉、重画」，N 是它算的行数，清不到多出来的那几行
 * → 旧的留在屏上、新的画在下面 → **输入越长重复越多**，就是客户说的「老是重复」。
 *
 * 模块加载即发起，等用户点开终端时通常早已就绪；万一没赶上，newTerm 里会异步补写 options。
 * 后端探不到 buildNumber 时返回 null，这里就整个不传 —— 猜一个数字只是换一种错法。
 */
type PtyInfo = { backend: string | null; buildNumber: number | null };
let ptyInfo: PtyInfo | null = null;
const ptyInfoReady: Promise<void> = invoke<PtyInfo>("term_pty_info")
  .then((i) => {
    ptyInfo = i;
  })
  .catch(() => {
    ptyInfo = { backend: null, buildNumber: null };
  });

/** 最近一次建终端时 GPU 渲染器挂上了没 —— 排障用（跑道断言 / 客户机「为什么还是闪」取证）。 */
let webglOn = false;
export function isWebglRenderer(): boolean {
  return webglOn;
}

/**
 * Windows 的 WebView2 先走 xterm 自带 DOM 渲染器。
 *
 * 2026-08-20 在真实 U-King 1.0.2 + WebView2 120 上截到了 canvas 字符笔画残缺；拖动窗口
 * 触发 DWM 重合成后画面又变化，正是 GPU 合成层而不是 PTY 文本流。旧跑道只数 canvas，证明
 * 不了 canvas 里的像素完整。Windows 端先以正确为先；其它平台保留 WebGL，后续只有在真实
 * WebView2 像素回归能证明不丢字时才重新打开。
 */
function shouldUseWebglRenderer(): boolean {
  return !/Windows/i.test(navigator.userAgent);
}

function windowsPtyOption(): { backend: "conpty"; buildNumber: number } | undefined {
  if (ptyInfo?.backend !== "conpty" || !ptyInfo.buildNumber) return undefined;
  return { backend: "conpty", buildNumber: ptyInfo.buildNumber };
}

/**
 * 终端的**复制**。（粘贴不在这儿 —— 见下。）
 *
 * 🔴 为什么必须自己接：xterm 5.5 没有 `copyOnSelect` 这个选项（源码里根本没有），
 * 而 Ctrl+C 走的是通用「ctrl+字母 → 控制字符」那条路
 * （`e.keyCode>=65&&<=90 → String.fromCharCode(keyCode-64)`），Ctrl+C(67) 直接变 `\x03`。
 * 也就是说客户选中一段文字按 Ctrl+C，**不但没复制，还把正在跑的 AI 任务打断了** ——
 * 这比「不能复制」更糟，而且他多半不会把这两件事联系起来。
 *
 * 规则（对齐 Windows Terminal / VS Code 终端）：
 *  - `Ctrl+Shift+C` 永远复制（不跟信号抢）；
 *  - `Ctrl+C` **只在有选区时**当复制，没选区照旧发 `^C` —— 中断是终端的命根子，绝不能抢；
 *  - 复制完 `clearSelection()`，否则紧接着再按一次 Ctrl+C 又被当成复制，客户会发现
 *    「中断按不动了」；
 *  - 右键弹**终端自己的菜单**，任何情况下都不许落到 WebView2 的默认菜单上（见下）。
 *
 * ## 🔴 右键：一个字都不许漏给浏览器（客户反馈 2026-08-10，v0.9.94）
 * 客户在终端里右键，弹出来的是 **Edge 网页菜单**：返回 / 刷新 / 另存为 / 打印 /
 * **发送标签页到你的设备**。对终端来说这些要么无意义要么有害（「刷新」会重载 webview），
 * 而且它直接告诉客户「这不是个应用，是个网页」。
 *
 * 病根是这里原来写的 `if (!term.hasSelection()) return;` —— **没选区就直接放行**，
 * 于是落到 WebView2 默认菜单。所以现在**无条件 `preventDefault()`**：
 * 右键要么弹我们自己的菜单，要么什么都不弹，绝不会弹浏览器的。
 *
 * ## ⚠️ 菜单里的「粘贴」绝不许自己实现
 * 大段文本落 .txt 贴路径（见 `PASTE_TO_FILE_BYTES`，绕 ConPTY 吃括号粘贴标记那个坑）
 * 和贴图落盘，都挂在 host 的 `paste` DOM 事件上。要是在这儿 `readText()` + `term.paste()`，
 * 就**绕过了那整套**，等于把已经修好的截断 bug 放回来。
 *
 * 所以「粘贴」的做法是：读剪贴板 → 造一个 `paste` 事件 → **dispatch 到 xterm 自己的
 * textarea 上**。这样走的还是 Ctrl+V 那条路（host 的捕获监听先处理图片/超长文本，
 * 剩下的交给 xterm），**一份实现两个入口**。
 */
function attachClipboard(term: XTerm, el: HTMLElement) {
  const copySelection = () => {
    const text = term.getSelection();
    if (!text) return;
    void navigator.clipboard.writeText(text).catch(() => {
      /* WebView2 下极少失败；失败就当没复制，不弹东西打断用户 */
    });
    term.clearSelection();
  };

  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown") return true;
    if (!e.ctrlKey || e.altKey || e.metaKey) return true;
    if (e.key.toLowerCase() !== "c") return true;
    if (e.shiftKey) {
      copySelection();
      return false;
    }
    if (term.hasSelection()) {
      copySelection();
      return false; // 吃掉这次，别让 ^C 发下去
    }
    return true; // 没选区 → 放行，该中断就中断
  });

  /**
   * 粘贴 —— **不自己往终端写字**，而是把真实剪贴板内容包成一个 `paste` 事件，
   * dispatch 到 xterm 的 textarea 上，让 Ctrl+V 那条链原样跑一遍：
   * host 上的捕获监听先接（图片落盘 / 超长文本落 .txt），剩下的 xterk 自己处理
   * （含括号粘贴标记，那是 ConPTY 那个截断坑的修复所在）。
   *
   * 返回 false = 剪贴板读不出来。**这时必须告诉用户**，不能静默什么都不发生 ——
   * 一个点了没反应的菜单项比没有这一项更伤信任。
   */
  const pasteThroughRealPipeline = async (): Promise<boolean> => {
    const dt = new DataTransfer();
    try {
      // `read()` 才拿得到图片；老 WebView 只有 `readText()`，退一步只贴文本。
      if (typeof navigator.clipboard?.read === "function") {
        for (const item of await navigator.clipboard.read()) {
          for (const type of item.types) {
            const blob = await item.getType(type);
            if (type.startsWith("image/")) {
              dt.items.add(new File([blob], `clip.${type.split("/")[1] || "png"}`, { type }));
            } else if (type === "text/plain") {
              dt.setData("text/plain", await blob.text());
            }
          }
        }
      } else {
        dt.setData("text/plain", await navigator.clipboard.readText());
      }
    } catch {
      return false; // 权限被拒 / 剪贴板空 —— 交给调用方如实说
    }
    if (!dt.types.length) return false;
    // 🔴 必须 dispatch 到 textarea 而不是 host：xterm 的粘贴监听挂在它自己的 textarea 上，
    //    从 host 派发只会触发我们那半边，正常文本会一个字都进不去。
    const target: HTMLElement = term.textarea ?? el;
    target.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }),
    );
    term.focus();
    return true;
  };

  el.addEventListener("contextmenu", (e) => {
    // 🔴 **无条件拦**。漏一种情况就会落到 WebView2 的「刷新 / 另存为 / 发送标签页到你的设备」上。
    e.preventDefault();
    e.stopPropagation();
    const hasSel = term.hasSelection();
    openTermMenu(el, e as MouseEvent, [
      // 灰的项照样列出来 —— 「这儿本来有个复制，只是你还没选东西」比菜单少一项更好懂
      { label: translate("复制"), enabled: hasSel, run: copySelection },
      {
        label: translate("粘贴"),
        enabled: true,
        run: async (note) => {
          if (!(await pasteThroughRealPipeline())) {
            note(translate("读不到剪贴板 —— 请用 Ctrl+V 粘贴"));
          }
        },
      },
      { label: translate("全选"), enabled: true, run: () => term.selectAll() },
      // 清屏只清回滚，不动正在跑的进程（`clear` 就是这个语义）
      { label: translate("清屏"), enabled: true, run: () => term.clear() },
    ]);
  });
}

/** 终端右键菜单的一项。`enabled` 在弹出那一刻算 —— 点了没反应的项比没有更糟。 */
type TermMenuItem = {
  label: string;
  enabled: boolean;
  /** `note` = 把一句话显示在菜单原地（用于「这次没成」），不弹全局 toast（终端这层没有）。 */
  run: (note: (msg: string) => void) => void | Promise<void>;
};

/**
 * 弹终端右键菜单。**原生 DOM，不走 React**。
 *
 * 为什么：`useTermGroup` 有三个消费方（TermPanel / TerminalPage / SplitContainer），
 * 走 React 就得挨个接线，而**少接一处，那条路上右键就还是浏览器菜单** ——
 * 恰好是最难发现的那种漏（它不报错，只是"没修好"）。菜单由建监听的这段代码自己造，
 * 谁挂了终端谁就自动有，忘不了。
 *
 * Tailwind 的类名写在 .ts 字面量里也会被扫到（`content: ./src/**\/*.{ts,tsx}`），主题一致。
 */
function openTermMenu(host: HTMLElement, e: MouseEvent, items: TermMenuItem[]) {
  host.querySelectorAll("[data-term-menu]").forEach((n) => n.remove());

  const menu = document.createElement("div");
  menu.setAttribute("data-term-menu", "");
  menu.className =
    "absolute z-50 min-w-[132px] py-1 rounded-lg border border-white/[0.12] bg-bg-2 shadow-lg text-[12.5px] select-none";
  const box = host.getBoundingClientRect();
  menu.style.left = `${Math.min(e.clientX - box.left, Math.max(0, box.width - 150))}px`;
  menu.style.top = `${Math.min(e.clientY - box.top, Math.max(0, box.height - 130))}px`;

  const close = () => {
    menu.remove();
    document.removeEventListener("pointerdown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
  };
  const onOutside = (ev: Event) => {
    if (!menu.contains(ev.target as Node)) close();
  };
  const onKey = (ev: KeyboardEvent) => {
    if (ev.key === "Escape") close();
  };
  const note = (msg: string) => {
    menu.textContent = "";
    const n = document.createElement("div");
    n.className = "px-3 py-1.5 text-[11.5px] text-ink-4 whitespace-nowrap";
    n.textContent = msg;
    menu.appendChild(n);
    setTimeout(close, 2600); // 说完自己走，别赖在屏幕上
  };

  for (const it of items) {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = it.label;
    b.disabled = !it.enabled;
    b.className = it.enabled
      ? "w-full text-left px-3 py-1 text-ink-1 hover:bg-accent/[0.18] hover:text-ink-0"
      : "w-full text-left px-3 py-1 text-ink-5 cursor-default";
    if (it.enabled) {
      b.addEventListener("click", () => {
        const r = it.run(note);
        // 同步项点完就关；异步项等它决定（可能要 note 一句「没成」）
        if (r instanceof Promise) void r.then(() => { if (menu.isConnected && menu.querySelector("button")) close(); });
        else close();
      });
    }
    menu.appendChild(b);
  }

  host.appendChild(menu);
  // 延后一帧再挂「点外面关掉」：否则**这次右键的 pointerdown 自己就会把它关掉**
  setTimeout(() => {
    document.addEventListener("pointerdown", onOutside, true);
    document.addEventListener("keydown", onKey, true);
  }, 0);
}

export type TermGroup = {
  /** 宿主容器 ref —— 调用方挂到面板/抽屉的终端区 */
  hostRef: React.RefObject<HTMLDivElement | null>;
  /** `dead` = 对面进程已退出，调用方据此把标签置灰并显示「重开」 */
  tabs: { key: number; title: string; dead: boolean }[];
  activeKey: number | null;
  setActiveKey: (k: number) => void;
  newTerm: () => TermSession | undefined;
  closeTerm: (key: number) => void;
  /** 在原标签里重新起一个 PTY（只对已退出的标签有效） */
  restartTerm: (key: number) => void;
  /** 把标签 `key` 拖到标签 `beforeKey` 之前（`beforeKey` 为 null = 挪到最后）。只动顺序，不碰 PTY */
  moveTab: (key: number, beforeKey: number | null) => void;
  /** 在当前激活终端（没有则新建）跑一条命令 */
  runInActive: (cmd: string) => void;
  /** 在全新的终端标签里跑一条命令（用于会占住前台的长驻模式） */
  runInNew: (cmd: string) => void;
  /** 往当前激活终端（没有则新建）**写入文本但不回车** —— 拖文件进来贴路径用 */
  writeToActive: (text: string) => void;
  /**
   * 移动当前终端的显示视口，不向 PTY 写任何按键。
   * Codex 等 TUI 自己会使用 ↑/↓（输入历史、选择菜单），所以宿主不能劫持它们翻回滚。
   */
  scrollActive: (lines: number) => void;
  /** 拖文件悬停在终端宿主上（调用方可据此显示高亮遮罩） */
  dropOver: boolean;
  /** 右键终端里的文件路径 → 该弹的菜单（null = 不弹）。调用方渲染，动作也由调用方接。 */
  fileMenu: TermFileMenu | null;
  closeFileMenu: () => void;
  /** 当前这个终端里跑起来的英文 TUI（claude / codex / …），null = 没有。调用方据此出中文小抄。 */
  activeTui: string | null;
};

/**
 * 界面是英文的那几个 CLI —— 客户原话「很多人对 Claude Code 的英文提示不懂」。
 *
 * 只按**用户敲下去的那条命令**认，不去猜屏幕上的内容：猜错了会在人家跑别的东西时
 * 挂一条驴唇不对马嘴的小抄。带参数（`claude -p …`）也算，管道/重定向里的不算。
 */
const ENGLISH_TUIS = ["claude", "codex", "hermes", "qwen", "crush", "opencode", "dsh", "pi", "cline"];
function tuiOf(cmd: string): string | null {
  const head = cmd.trim().split(/\s+/)[0]?.replace(/\.(exe|cmd|bat)$/i, "").toLowerCase() ?? "";
  return ENGLISH_TUIS.includes(head) ? head : null;
}

/**
 * @param open  group 是否可见（抽屉展开 / 任务激活）。用于 fit + 懒建首个终端。
 * @param cwd   终端工作目录（任务文件夹）；undefined → 后端回落 home。
 * @param pendingCmd 待运行命令（点工具「打开终端」塞进来）；运行后调 onConsumedCmd 清空。
 */
export function useTermGroup(opts: {
  open: boolean;
  cwd?: string;
  /** 给本 group 的 PTY 打工具 tag（claude/openclaw…），运行面板 list_running 据此识别 */
  tool?: string;
  /** 首个终端自动跑的启动命令（工具型会话用，如 "openclaw gateway run"）。过后端白名单。 */
  initialCmd?: string;
  pendingCmd?: string | null;
  onConsumedCmd?: () => void;
  /** 升级后的多终端重开队列；每条都在独立标签、各自的 cwd/tool 下启动。 */
  pendingRestores?: TermRestore[] | null;
  /** 有任一 PTY 未能创建时保留仅失败项；用户重试不会重复打开已成功终端。 */
  onRestoreFailed?: (failed: TermRestore[]) => void;
  onConsumedRestores?: () => void;
  /** 左键点终端里的文件路径 → 交给调用方（工作台传 previewFile，右侧直接出预览）。
   *  不传则不接管左键（链接仍会画出来，点了什么也不做）。 */
  onOpenFile?: (path: string) => void;
}): TermGroup {
  const { open, cwd, tool, initialCmd, pendingCmd, onConsumedCmd, pendingRestores, onRestoreFailed, onConsumedRestores, onOpenFile } = opts;
  const hostRef = useRef<HTMLDivElement>(null);
  const sessionsRef = useRef<TermSession[]>([]);
  const seqRef = useRef(0); // 实例内序号，不共用模块全局
  const cwdRef = useRef(cwd);
  cwdRef.current = cwd;
  const toolRef = useRef(tool);
  toolRef.current = tool;
  // 启动命令只在首个终端下发一次
  const initialCmdRef = useRef(initialCmd);
  const initialFiredRef = useRef(false);
  // 点路径的回调放 ref：newTerm 是 useCallback，不能让它随调用方每次渲染都重建（重建 = 终端重挂）
  const openFileRef = useRef(onOpenFile);
  openFileRef.current = onOpenFile;
  const [fileMenu, setFileMenu] = useState<TermFileMenu | null>(null);
  // 当前终端里跑着哪个英文 TUI（用于中文小抄）。按「敲下去的命令」认，见 tuiOf。
  const [activeTui, setActiveTui] = useState<string | null>(null);
  const [tabs, setTabs] = useState<{ key: number; title: string; dead: boolean }[]>([]);
  const [activeKey, setActiveKey] = useState<number | null>(null);
  const activeKeyRef = useRef<number | null>(null);
  activeKeyRef.current = activeKey;

  /**
   * 尺寸真变了才发 `term_resize`。
   *
   * 🔴 为什么非要这一层：ConPTY 收到 resize 会让对面的 TUI **整屏重画**（Claude Code / hermes
   * 这类满屏应用尤其明显）。老代码是「fit 完无条件发一次」，于是每一次 ResizeObserver 回调
   * ——拖面板分隔条、切标签、开合右侧栏、甚至只是同尺寸的重新观察——都白白让对面重画一次，
   * 表现就是终端「跳来跳去、字一闪一闪」。而 `fit.fit()` 在尺寸没变时对 xterm 自己是 no-op，
   * 唯一泄漏出去的就是这条 invoke。掐掉它，行为一字不变，重画次数从「每帧一次」降到「真变才有」。
   */
  const pushSize = (s: TermSession) => {
    if (!s.sessionId) return;
    const { cols, rows } = s.term;
    if (s.sentSize && s.sentSize.cols === cols && s.sentSize.rows === rows) return;
    s.sentSize = { cols, rows };
    invoke("term_resize", { sessionId: s.sessionId, cols, rows }).catch(() => {});
  };

  // 统一防抖 fit：activeKey 切换 / 容器尺寸变化 / open 切换都走这一个入口，
  // 用 rAF 合并同一帧内的多次触发 —— 杜绝「三处各自 setTimeout fit 叠加重算」的抖动。
  const rafRef = useRef<number | null>(null);
  const fitActive = useCallback(() => {
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      const s = sessionsRef.current.find((x) => x.key === activeKeyRef.current);
      if (!s) return;
      try {
        s.fit.fit();
        pushSize(s);
      } catch {
        /* ignore */
      }
    });
  }, []);

  // 起一个 PTY（懒启动）。返回 sessionId。
  // 并发安全：`term_open` 是 async，只判 `s.sessionId` 挡不住竞态 —— 第二个调用者在第一个
  // await 返回前进来时 sessionId 还是 null，就会给**同一个 xterm 开出第二个 PTY**：两个 shell
  // 的输出都写进同一块屏幕（提示符、回显全是双份），且先开的那个 sessionId 被覆盖后永远
  // 关不掉，成了泄漏的僵尸进程。触发路径现成的：open effect 建终端的同时 pendingCmd effect
  // 也在跑，或者用户连点两下工具按钮。改成共用同一个在飞的 promise。
  const ensurePty = useCallback(async (s: TermSession): Promise<string | null> => {
    if (s.sessionId) return s.sessionId;
    if (s.pending) return s.pending;
    s.pending = (async () => {
      try {
        const onData = new Channel<number[]>();
        onData.onmessage = (bytes) => {
          // 标签已关 → xterm 已 dispose，后端的收尾帧再往里写会抛（Channel 回调里抛 = 无人接）
          if (s.closed) return;
          // ★ 空帧 = 后端 reader 收尾发的 EOF 哨兵（正常输出读到 0 字节时后端是 break 不发送的，
          // 见 term.rs），表示对面进程已经退了。以前后端什么都不发、前端也就毫无感知：
          // 标签绿点照亮、敲什么都没反应，只能自己关掉重开。
          if (bytes.length === 0) {
            s.dead = true;
            s.input.disconnect();
            if (s.sessionId) releaseSession(s.sessionId); // 对面进程已退，别再给它报平安
            s.sessionId = null;
            setTabs((t) => t.map((x) => (x.key === s.key ? { ...x, dead: true } : x)));
            return;
          }
          // 回调发生在这块字节被 xterm 解析之后。Windows WebView2 的 DOM 局部重画偶尔会
          // 留下缺笔/叠字（拖宽度触发全屏重画就恢复），输出停顿后主动从正确缓冲区校正一次。
          s.term.write(new Uint8Array(bytes), () => s.paintRepair?.afterWrite());
        };
        // 每个标签都带自己的初始命令；普通 group 仍只会给首个标签拿到默认 initialCmd。
        const initCmd = s.initialCmd ?? null;
        if (initCmd) setActiveTui(tuiOf(initCmd));
        const sid = await invoke<string>("term_open", {
          cols: s.term.cols || 80,
          rows: s.term.rows || 24,
          onData,
          initialCmd: initCmd,
          cwd: s.cwd ?? cwdRef.current ?? null,
          tool: s.tool ?? toolRef.current ?? null,
        });
        // 仅在 PTY 确认建成后才消费重放命令。失败/超时后的复用标签仍要能带原命令重试。
        s.initialCmd = null;
        s.sessionId = sid;
        s.input.connect(sid);
        claimSession(sid); // 登记归属：心跳靠它告诉后端「这个会话还有人用」
        return sid;
      } catch (e) {
        s.input.clear();
        s.term.writeln(`\x1b[31m打开终端失败: ${String(e)}\x1b[0m`);
        return null;
      } finally {
        // 成功时 sessionId 已填，第一个 if 会短路；失败时清空以便后续重试
        s.pending = null;
      }
    })();
    return s.pending;
  }, []);

  // 新建一个终端标签
  const newTerm = useCallback((restore?: TermRestore): TermSession | undefined => {
    const host = hostRef.current;
    if (!host) return;
    const key = ++seqRef.current;
    const el = document.createElement("div");
    el.style.cssText = "position:absolute;inset:0;padding:6px 8px;";
    host.appendChild(el);

    const term = new XTerm({
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      theme: getGlobalTermTheme(),
      scrollback: 5000,
      windowsPty: windowsPtyOption(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    attachClipboard(term, el);
    term.open(el);
    // 输入法候选条锚定 —— 必须在 open() 之后：textarea 是 open() 里才建出来的
    const unanchor = anchorImeToCursor(term);
    // GPU 渲染器：非 Windows 保留；Windows 真实 WebView2 已抓到 canvas 残字，默认回退 DOM。
    // **必须能退**：老显卡 / 虚拟机 / 远程桌面下 WebGL 拿不到上下文，拿不到就静默回 DOM 渲染
    //（宁可慢，不能白屏）；上下文丢失（切换显卡、驱动重启）也当场卸掉退回去。
    let webgl: WebglAddon | undefined;
    if (!shouldUseWebglRenderer()) {
      webglOn = false;
    } else {
      try {
        const w = new WebglAddon();
        // 上下文丢失后这个 addon 已经废了：收掉它、**并且把引用清空**，
        // 否则拆终端时 disposeTerm 会去 dispose 一个已经 dispose 过的它（正是崩溃那条路）。
        w.onContextLoss(() => {
          try {
            w.dispose();
          } catch {
            /* ignore */
          }
          webgl = undefined;
          const cur = sessionsRef.current.find((x) => x.term === term);
          if (cur) cur.webgl = undefined;
        });
        term.loadAddon(w);
        webgl = w;
        webglOn = true;
      } catch {
        webglOn = false;
      }
    }
    // 探测还没回来就先建了终端（极少见：模块加载后立刻点开）→ 就绪后补上，
    // 否则这个终端会一直用错的折行口径，表现就是「只有第一个终端老是重复」。
    if (!windowsPtyOption()) {
      void ptyInfoReady.then(() => {
        const opt = windowsPtyOption();
        if (opt) term.options.windowsPty = opt;
      });
    }

    let s: TermSession;
    const input = createTermInputQueue({
      write: (sessionId, data) => invoke("term_write", { sessionId, data }),
      onError: (error) => {
        if (s.closed) return;
        s.term.writeln(`\r\n\x1b[31m[输入发送失败，请重开这个终端：${String(error)}]\x1b[0m`);
      },
    });
    const firstInitialCmd = !restore && initialCmdRef.current && !initialFiredRef.current
      ? initialCmdRef.current
      : null;
    if (firstInitialCmd) initialFiredRef.current = true;
    const launchCmd = restore?.cmd ?? firstInitialCmd;
    s = {
      key,
      title: `终端 ${key}`,
      term,
      fit,
      webgl,
      sessionId: null,
      pending: null,
      dead: false,
      closed: false,
      el,
      unanchor,
      input,
      cwd: restore?.cwd ?? cwdRef.current ?? null,
      tool: restore?.tool ?? toolRef.current ?? null,
      initialCmd: launchCmd,
      restartCmd: launchCmd,
      restoreKey: restore?.restoreKey,
    };
    if (!shouldUseWebglRenderer()) {
      s.paintRepair = createPaintRepair({
        refresh: () => {
          if (!s.closed && s.term.rows > 0) s.term.refresh(0, s.term.rows - 1);
        },
        // Codex TUI 的输入行也会高频局部重画；不能只在“AI 有输出”后修。
        // 24ms 合并一帧内的按键/回显，160ms 又确保持续输入不会一直保留缺字。
        quietMs: 24,
        maxWaitMs: 160,
      });
    }
    // 终端里的文件路径变成可点的：左键交给调用方预览，右键出「打开方式 / 复制路径」菜单。
    // AI 干完活最后那句「已生成 D:\xx\报告.docx」原本是死字，客户只能自己去资源管理器翻。
    s.unlink = registerFileLinks(term, el, {
      cwd: () => s.cwd ?? undefined,
      // 🔴 候选路径要**逐个问过磁盘**再往下传。终端里那行常常只有文件名，
      // 目录写在上面另一行 —— 只按「终端当前目录」拼就会拼到一个不存在的地方，
      // 于是预览 404、默认程序打开报错、而「复制路径」还会不声不响地复制一条错路径。
      // 目录 → 打开文件夹（塞进文件预览只会报错）；文件 → 交给宿主预览。
      onOpen: (p, cands) =>
        void firstExisting(cands).then((hit) => {
          if (hit?.isDir) {
            void invoke("open_dir_external", { path: hit.path, app: "explorer" }).catch(() => {});
            return;
          }
          openFileRef.current?.(hit?.path ?? p);
        }),
      // 网址交给系统默认浏览器（终端里点链接的通行预期）。走 plugin-opener：
      // plugin-shell 在生产构建里失效，见 App.tsx 那处同款注释。
      onOpenUrl: (u) => void openUrl(u).catch(() => {}),
      onMenu: (info) =>
        void firstExisting(info.candidates).then((hit) =>
          setFileMenu({ ...info, path: hit?.path ?? info.path, missing: !hit, isDir: hit?.isDir ?? false }),
        ),
    });
    term.onData((d) => {
      s.input.push(d);
      // WebView2 的 DOM 渲染器不只会把流式输出画残，Codex 本地回显输入行时也会。
      // 这只是从 xterm 正确的 buffer 重画，绝不向 PTY 伪造 ↑/↓ 或任何用户输入。
      s.paintRepair?.afterWrite();
    });

    sessionsRef.current.push(s);
    setTabs((t) => [...t, { key, title: s.title, dead: false }]);
    setActiveKey(key);
    setTimeout(() => {
      try {
        fit.fit();
      } catch {
        /* ignore */
      }
      ensurePty(s).then(() => pushSize(s));
      term.focus();
    }, 50);
    return s;
  }, [ensurePty]);

  // 关闭一个终端标签
  const closeTerm = useCallback((key: number) => {
    const arr = sessionsRef.current;
    const idx = arr.findIndex((x) => x.key === key);
    if (idx < 0) return;
    const s = arr[idx];
    s.closed = true; // 先立旗：后端收尾帧到达时不再往已 dispose 的 xterm 里写
    closeBackend(s);
    disposeTerm(s); // 收尾抛错不许冒到 React（见 disposeTerm 的注释）
    s.el.remove();
    arr.splice(idx, 1);
    setTabs((t) => t.filter((x) => x.key !== key));
    setActiveKey((cur) => {
      if (cur !== key) return cur;
      const next = arr[idx] ?? arr[idx - 1] ?? null;
      return next ? next.key : null;
    });
  }, []);

  /**
   * 拖动重排标签。**只动顺序，不碰 xterm、不碰 PTY** —— 终端实例挂在 hostRef 那一个容器里靠
   * display 切换（见下面的 activeKey effect），跟标签渲染顺序无关，所以重排不会重挂终端。
   *
   * 🔴 两份顺序必须一起动：`tabs` 是渲染序，`sessionsRef.current` 是实例序，而 `closeTerm` 靠
   * 后者的下标挑「关掉之后激活谁」（`arr[idx] ?? arr[idx-1]`）、`open` effect 又拿它的 [0] 当默认。
   * 只重排 tabs 的话，症状是「关掉当前标签，跳过去的不是视觉上的邻居」——看起来完全随机。
   */
  const moveTab = useCallback((key: number, beforeKey: number | null) => {
    if (key === beforeKey) return;
    const reorder = <T extends { key: number }>(arr: T[]): T[] | null => {
      const from = arr.findIndex((x) => x.key === key);
      if (from < 0) return null;
      const next = arr.slice();
      const [item] = next.splice(from, 1);
      // 目标下标要在**移除之后**的数组里重算，否则往右拖会差一位
      const to = beforeKey == null ? next.length : next.findIndex((x) => x.key === beforeKey);
      if (to < 0) return null;
      next.splice(to, 0, item);
      return next;
    };
    const nextSessions = reorder(sessionsRef.current);
    if (!nextSessions) return;
    sessionsRef.current = nextSessions;
    setTabs((t) => reorder(t) ?? t);
    // 🔴 拖完焦点会留在标签那个 div 上，敲键盘不进终端 —— 用户看到的是「终端卡死了」。
    // activeKey 没变所以下面那条 effect 不会重跑，这里补一次。
    sessionsRef.current.find((x) => x.key === activeKeyRef.current)?.term.focus();
  }, []);

  // 在原标签里重新起一个 PTY —— 只对「对面进程已退出」的标签有效。
  // 没有它的话，用户敲完 exit（或工具自己崩了）只能关掉标签再新建；工具型会话（ToolAppView
  // 传了 initialCmd）重开还意味着把那个工具重新拉起来，所以这里放开一次性的启动命令。
  const restartTerm = useCallback(
    (key: number) => {
      const s = sessionsRef.current.find((x) => x.key === key);
      if (!s || !s.dead || s.closed) return;
      s.dead = false;
      setTabs((t) => t.map((x) => (x.key === key ? { ...x, dead: false } : x)));
      s.term.reset();
      s.initialCmd = s.restartCmd ?? null;
      void ensurePty(s).then(() => {
        s.sentSize = undefined; // 重开的是新 PTY，它还不知道尺寸 —— 这一次必须真发
        pushSize(s);
        s.term.focus();
      });
    },
    [ensurePty],
  );

  // 切换显示哪个终端（display 切换，不动 PTY）→ 走统一防抖 fit
  useEffect(() => {
    for (const s of sessionsRef.current) {
      s.el.style.display = s.key === activeKey ? "block" : "none";
    }
    const s = sessionsRef.current.find((x) => x.key === activeKey);
    if (s && open) {
      fitActive();
      s.term.focus();
    }
  }, [activeKey, open, fitActive]);

  // 可见时：没有任何终端就新建一个；有就 fit 当前（统一防抖）
  useEffect(() => {
    if (!open) return;
    if (sessionsRef.current.length === 0 && !pendingRestores?.length) {
      newTerm();
    } else {
      fitActive();
      const s = sessionsRef.current.find((x) => x.key === activeKey) ?? sessionsRef.current[0];
      s?.term.focus();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // 升级恢复：逐条新建标签而非把命令灌进同一个 shell，保证 cwd/tool 不串。
  // 全批共享 60s 总超时；超时和拒绝都只留下失败项。重试会复用同 restoreKey 的 xterm/在飞 promise，
  // 因而不会把已成功项（也不会把尚未返回的同一请求）再开一遍。
  const consumedRestoresRef = useRef<TermRestore[] | null>(null);
  useEffect(() => {
    if (!open || !pendingRestores?.length || consumedRestoresRef.current === pendingRestores) return;
    consumedRestoresRef.current = pendingRestores;
    let cancelled = false;
    void (async () => {
      let timeoutId: ReturnType<typeof setTimeout> | undefined;
      const timedOut = new Promise<never>((_, reject) => {
        timeoutId = setTimeout(() => reject(new Error("恢复终端超时")), 60_000);
      });
      const restored = pendingRestores.map((restore, index) => {
        const restoreKey = restore.restoreKey ?? index;
        const normalized = restore.restoreKey === restoreKey ? restore : { ...restore, restoreKey };
        const existing = sessionsRef.current.find((s) => s.restoreKey === restoreKey && !s.closed);
        return { restore: normalized, session: existing ?? newTerm(normalized) };
      });
      const results = await Promise.allSettled(
        restored.map(({ session }) =>
          session ? Promise.race([ensurePty(session), timedOut]).then((sessionId) => {
            if (!sessionId) throw new Error("恢复终端失败");
            return sessionId;
          }) : Promise.reject(new Error("无法创建终端标签")),
        ),
      );
      if (timeoutId !== undefined) clearTimeout(timeoutId);
      if (cancelled) return;
      const failed = restored.filter((_, index) => results[index].status !== "fulfilled").map(({ restore }) => restore);
      if (failed.length > 0) {
        onRestoreFailed?.(failed);
        return;
      }
      onConsumedRestores?.();
    })();
    return () => {
      cancelled = true;
    };
  }, [open, pendingRestores, newTerm, ensurePty, onRestoreFailed, onConsumedRestores]);

  // 待运行命令：在当前（或新建）终端里跑
  useEffect(() => {
    if (!open || !pendingCmd) return;
    let cancelled = false;
    (async () => {
      let s = sessionsRef.current.find((x) => x.key === activeKey);
      if (!s) s = newTerm();
      if (!s) return;
      const sid = await ensurePty(s);
      if (cancelled || !sid) return;
      setActiveTui(tuiOf(pendingCmd));
      s.input.push(pendingCmd + "\r");
      onConsumedCmd?.();
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, pendingCmd]);

  // 在当前激活终端（没有则新建）跑一条命令 —— 终端顶部快捷按钮用
  const runInActive = useCallback(
    (cmd: string) => {
      void (async () => {
        let s = sessionsRef.current.find((x) => x.key === activeKeyRef.current);
        if (!s) s = newTerm();
        if (!s) return;
        const sid = await ensurePty(s);
        if (!sid) return;
        setActiveTui(tuiOf(cmd));
        s.input.push(cmd + "\r");
        s.term.focus();
      })();
    },
    [newTerm, ensurePty],
  );

  // 在全新的终端标签里跑命令。Web server / TUI 都会长期占住前台 shell；模式切换若复用
  // 当前标签，命令会被写进正在运行的程序而不是交给 shell。DSH 的 Web / 终端双入口用这条。
  const runInNew = useCallback(
    (cmd: string) => {
      void (async () => {
        const s = newTerm();
        if (!s) return;
        const sid = await ensurePty(s);
        if (!sid) return;
        setActiveTui(tuiOf(cmd));
        s.input.push(cmd + "\r");
        s.term.focus();
      })();
    },
    [newTerm, ensurePty],
  );

  // 往当前激活终端写入文本但**不回车**（拖文件进来贴路径用）—— 走 PTY，shell 会回显
  const writeToActive = useCallback(
    (text: string) => {
      void (async () => {
        let s = sessionsRef.current.find((x) => x.key === activeKeyRef.current);
        if (!s) s = newTerm();
        if (!s) return;
        const sid = await ensurePty(s);
        if (!sid) return;
        s.input.push(text);
        s.term.focus();
      })();
    },
    [newTerm, ensurePty],
  );

  // 只滚 xterm 的 viewport，不把控制字符送给正在运行的 Codex CLI。
  // `scrollToBottom` 比反复向下滚可靠：新输出或窗口尺寸变化后仍可精确回到实时位置。
  const scrollActive = useCallback((lines: number) => {
    const s = sessionsRef.current.find((x) => x.key === activeKeyRef.current);
    if (!s || s.closed) return;
    if (lines === Number.POSITIVE_INFINITY) s.term.scrollToBottom();
    else s.term.scrollLines(lines);
    s.term.focus();
  }, []);

  // 容器尺寸变化 → 统一防抖 fit（rAF 合并；拖拽时 ResizeObserver 会高频触发，
  // 合并后每帧最多 fit 一次，不再抖）
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const ro = new ResizeObserver(() => fitActive());
    ro.observe(host);
    return () => ro.disconnect();
  }, [fitActive]);

  // 全局主题切换 → 本 group 所有已建 xterm 即时换肤 + 宿主底色跟随
  // （宿主 padding 边缘不再露硬编码黑/白底；挂载即应用当前主题一次）
  useEffect(() => {
    const apply = () => {
      const theme = getGlobalTermTheme();
      for (const s of sessionsRef.current) {
        try {
          s.term.options.theme = theme;
        } catch {
          /* ignore */
        }
      }
      if (hostRef.current) hostRef.current.style.background = theme.background;
    };
    apply();
    return subscribeTermTheme(apply);
  }, []);

  // 拖文件进终端宿主 = 把路径贴到当前输入行（不回车）—— 所有用 useTermGroup 的终端
  // （面板 / 独立页 / 工作台）统一获得，逻辑只此一处。含空格路径加引号，末尾留空格便于接着敲。
  const [dropOver, setDropOver] = useState(false);
  const writeRef = useRef(writeToActive);
  writeRef.current = writeToActive;
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    return registerDropZone(
      host,
      (paths) => {
        writeRef.current(pathsToText(paths));
      },
      setDropOver,
    );
  }, []);

  // 粘贴图片进终端 = 把图片落成临时文件、再把路径贴到输入行 —— 终端是纯文本流，Claude/Codex 只能读
  // 文件路径（跟拖文件进终端一个套路）。capture 阶段先于 xterm，仅拦图片；文本粘贴不拦，交给 xterm。
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const onPaste = (e: ClipboardEvent) => {
      const dt = e.clipboardData;
      if (!dt) return;
      const img = Array.from(dt.items).find((it) => it.kind === "file" && it.type.startsWith("image/"));
      if (img) {
        const file = img.getAsFile();
        if (!file) return;
        e.preventDefault();
        e.stopPropagation();
        void (async () => {
          try {
            const buf = new Uint8Array(await file.arrayBuffer());
            const ext = (file.type.split("/")[1] || "png").toLowerCase();
            const path = await invoke<string>("save_pasted_image", { bytes: Array.from(buf), ext });
            const quoted = /\s/.test(path) ? `"${path}"` : path;
            writeRef.current(quoted + " ");
          } catch {
            /* 存图失败静默 —— 用户可改用「拖文件进终端」 */
          }
        })();
        return;
      }
      // 超长文本 → 落成 .txt 只贴路径（原因见 PASTE_TO_FILE_BYTES）。常规长度不拦，交给 xterm。
      const text = dt.getData("text/plain");
      if (!text) return;
      const bytes = new TextEncoder().encode(text);
      if (bytes.length <= PASTE_TO_FILE_BYTES) return;
      e.preventDefault();
      e.stopPropagation();
      void (async () => {
        try {
          // 复用存粘贴图那条落盘通道（同一个临时目录、同一套一天自动清理），不再造第二个命令
          const path = await invoke<string>("save_pasted_image", { bytes: Array.from(bytes), ext: "txt" });
          const quoted = /\s/.test(path) ? `"${path}"` : path;
          writeRef.current(quoted + " ");
        } catch {
          // 落盘失败就退回原样粘 —— 宁可碎，也不能把用户的内容整段吞掉
          writeRef.current(text);
        }
      })();
    };
    host.addEventListener("paste", onPaste, true);
    return () => host.removeEventListener("paste", onPaste, true);
  }, []);

  const closeFileMenu = useCallback(() => setFileMenu(null), []);

  // 卸载：关掉本 group 所有 PTY（仅在调用组件真正卸载时触发——
  // 工作台靠常驻 + display 切换保活，只有「关闭任务」才卸载 TermPanel）
  useEffect(() => {
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      for (const s of sessionsRef.current) {
        s.closed = true;
        closeBackend(s); // 同 closeTerm：在飞的 term_open 也要收，否则漏一个 shell 进程
        // 🔴 这里是**卸载 cleanup**：抛出去 = React 卸整棵树 = 客户眼里的「U-King 自己重启了」。
        // 而且一个终端抛了会让后面几个的 PTY 收不掉（漏 shell 进程），所以必须逐个吞。
        disposeTerm(s);
      }
      sessionsRef.current = [];
    };
  }, []);

  return {
    hostRef,
    tabs,
    activeKey,
    setActiveKey,
    newTerm,
    closeTerm,
    restartTerm,
    moveTab,
    runInActive,
    runInNew,
    writeToActive,
    scrollActive,
    dropOver,
    fileMenu,
    closeFileMenu,
    activeTui,
  };
}
