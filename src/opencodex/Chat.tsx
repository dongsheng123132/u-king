/**
 * **U-Chat** —— 工作台里的 GUI 对话框（直连虾盘云：后端 agent/chat.rs curl 流式 + 工具循环 + 审批模式 + Channel）。
 *
 * 命名：U-Workspace（工作台整体）/ **U-Chat（就是本文件这一块 + panels/ChatPanel.tsx）** / U-CLI（终端）。
 * 约定与理由见 `components/Sidebar.tsx` 的 CORE 注释 —— 一份真相源，这里只是指路。
 *
 * 【Codex 式工作台】中间对话 + 右侧可滑出「多终端 / 文件树 / 浏览器」。
 * 工具：作图 · 读/列文件（自动）· 写文件 · 跑命令（run_command，可跑 claude -p/codex exec 委派复杂编程）。
 * 审批模式（copy Codex）：每步确认 / 自动（写自动·命令问）/ 全授权（都不问，危险命令仍 Rust 硬拦）。
 *
 * 【U-Workspace 模块】本文件是 U-Workspace（AI 工作台）的对话引擎，随工作台一起收进 opencodex/ 独立演进。
 * 右侧面板直接复用同目录 panels（TermPanel/FilesPanel），display 切换保活（切面板不杀 PTY）。
 * 自包含，删掉只动 App.tsx + Sidebar.tsx。
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke, Channel, convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Bot, Check, ChevronRight, Copy, Eye, FileText, Film, FolderOpen, FolderTree, Globe, Image as ImageIcon, Loader2, Maximize2, MessageSquare, PanelLeftClose, PanelLeftOpen, Paperclip, RotateCcw, ShieldCheck, Terminal, User, X, ZoomIn, ZoomOut } from "lucide-react";
import { cn } from "../lib/cn";
import { trimHistoryForPayload } from "./historyTrim";
import { useViewport } from "../lib/useViewport";
import { XIAPAN_MODELS, priceyModelHint } from "../lib/models";
import { copyToClipboard } from "../lib/clipboard";
import { useDropZone, pathsToText } from "../lib/fileDrop";
import { describeImages, fileLabel, isImageFile } from "../lib/vision";
import { MiniMd } from "../lib/miniMd";
import { TermPanel, type TermPanelApi } from "./panels/TermPanel";
import { FilesPanel } from "./panels/FilesPanel";
import { ChatPanel, ProducedFile, deliverableExt, previewableExt, producedFiles } from "./panels/ChatPanel";
import { RedlinePanel } from "../vendor/redline-core";
import { createTauriRedlineHost } from "./redline-host-tauri";
// 相对路径 → 绝对路径只此一份实现（终端链接和这里用同一个，跨平台分隔符也在它里面判）
import { resolvePath } from "./term/fileLinks";
import { ENGINE_TUI_CMD, type Engine } from "./types";
import { DiffView } from "./panels/DiffView";
import { QuickPrompts, type Best } from "./QuickPrompts";
import { useComposerMenu } from "./ComposerMenu";
import { AttachButton, Composer, ComposerSelect } from "./Composer";
import { buildSystemPrompt, type Expert } from "./experts";
import { allExperts } from "./experts";
import { deliver, takeHandoff, type Handoff } from "./handoff";
import { takeTermCmd } from "./termInbox";
import { releaseYield, reportTermWidth } from "../lib/yieldChain";
import type { ProviderPreset } from "../Wizard";
import type { DeviceKey } from "../lib/types";
import { useI18n } from "../i18n";
import { initOtel, wrapGenAICall } from "../lib/otel/tracer";

// 对话大脑：uking=自家虾盘云直连(轻快·作图·小白)；claude=驱动真身 Claude Code(结构化卡片,agent/claude.rs)；
// codex/hermes=在中间开真身 CLI 的 TUI 终端(组合调用最强工具;需先在装 AI/虾盘云一键配好该工具)
// 类型本体挪到 `types.ts`（起手词也要用它标 best，从子组件反向 import 本文件会成环）；这里 re-export 保持既有引用不变。
export type { Engine };
// 成本提示：claude / codex 底层都走 deepseek-v4-flash(delegation_env 注入,省·免配置)，
// 模型真相源是 providers.rs 的虾盘云 preset，别在这儿另写一个。Codex 走的是
// `deepseek-v4-flash-codex`（/v1/responses 只有这条 type=1 直连渠道认，裸名 500）。
// 主力推 Claude Code：同样的钱，它的结构化卡片/工具循环最成熟。
export const ENGINES: { id: Engine; label: string }[] = [
  { id: "claude", label: "Claude Code（推荐·最强·已免配）" },
  { id: "uking", label: "U-King 轻助手（省钱兜底·作图快）" },
  { id: "codex", label: "Codex（已免配·换个脑子试试）" },
  // ★ 2026-08-16 补的第二条 Claude 路：以前「对话框底下就是终端」只有 Hermes 有，
  // 而 Hermes 不是大多数人真正想在终端里用的那个 —— 他们想要的是**原味 Claude Code**
  // （`/` 指令、计划模式、自己的审批流）。上面那条 `claude` 是我们代驱动的卡片壳，
  // 这条是把它本人摆出来。同一个工具、同一个 Key，差的只是壳（见 types.ts::Engine 的注释）。
  { id: "claude-cli", label: "Claude Code 终端（原味 TUI·老手推荐）" },
  { id: "hermes", label: "Hermes 终端（自带记忆）" },
];

type TextItem = { type: "text"; role: "user" | "assistant"; content: string };
type ToolItem = { type: "tool"; name: string; phase: "running" | "done" | "error"; prompt?: string; path?: string; command?: string; output?: string; b64?: string; url?: string; message?: string; oldStr?: string; newStr?: string; isNew?: boolean };
type ApprovalItem = { type: "approval"; id: string; tool: string; action?: string; inputKeys?: string[]; path?: string; bytes?: number; command?: string; decided?: "approved" | "rejected"; oldStr?: string; newStr?: string };
type Item = TextItem | ToolItem | ApprovalItem;
type RightKind = "terminal" | "files" | "preview";
// 右侧「预览/画布」内容：生成的图（可放大）/ 视频（可播放）/ 渲染的 HTML / 办公文档（交给 redline 内核）
type Preview =
  | { kind: "image"; src: string; caption?: string }
  | { kind: "video"; src: string; caption?: string }
  /**
   * 网页预览。`src`（asset 协议地址）优先，没有才退回 `html`（整段源码塞 srcDoc）。
   *
   * 🔴 为什么必须有 `src` 这一路：`srcDoc` 里的页面**没有 base URL**，
   * `<img src="pic.png">` / `<link href="style.css">` 这些相对路径一个都解析不到 ——
   * AI 生成的网页十有八九带同目录资源，客户看到的是一张裸骨架，还会以为是 AI 做坏了。
   * 走 asset 协议就是真·文件 URL，相对资源照常加载（前提：那个目录 allow_fs_preview 过）。
   */
  | { kind: "html"; html?: string; src?: string; path?: string; caption?: string }
  | { kind: "doc"; path: string; caption?: string }
  | null;

// 模型清单来自 `lib/models.ts`（Manager「换模型」/ ProviderSwitch 用的是同一份）——
// 这儿原先自己抄了 5 条，其中 `gpt-5.6-luna` 正是把客户余额打穿的那一档，而抄的那份
// 既没有「贵/慎用」提示也没有看图模型。同一事实两份实现，迟早漂移（宪法第 8 条）。
const DEFAULT_MODEL = "deepseek-v4-flash";
const MODES = [
  { id: "ask", label: "每步确认（最安全）" },
  { id: "auto", label: "自动（写文件自动·命令仍问）" },
  { id: "full", label: "全授权（都不问）" },
];
// 按钮上写人话（客户认「终端」不认「U-CLI」），tooltip 里带代号 —— 代号是给报 bug /
// 看文档时**指认是哪一块**用的，不是拿来教育客户的。命名约定见 Sidebar.tsx 的 CORE 注释。
const RIGHT_META: { kind: RightKind; label: string; title: string; icon: typeof Terminal; needsWs: boolean; lab?: boolean }[] = [
  { kind: "preview", label: "预览", title: "预览（图 / 网页 / 文档）", icon: Eye, needsWs: false },
  { kind: "terminal", label: "终端", title: "终端（U-CLI）", icon: Terminal, needsWs: true },
  { kind: "files", label: "文件", title: "文件树", icon: FolderTree, needsWs: true },
  // 「创作」面板 2026-08-23 撤掉 —— **能力一点没动**，`Create.tsx` 回到侧栏「AI 创作」独立页。
  //
  // 它 08-21 从侧栏搬进这里，理由是「和 U-Chat 是同一件事的两个入口」。撤回的理由是用户反馈
  // 「客户希望留在侧栏」，而**两边都挂同一个 `Create.tsx` 不是个选项**：它内部带落盘历史和
  // 「进行中」保活状态，挂两处就是两份互不相通的状态 —— 客户在这边出的图在那边看不到，
  // 这种不一致比少一个入口更难解释。**一个能力一个入口。**
  //
  // 上一版这段注释记着「搬而不是删」的原因，那条依然成立、也依然被遵守：U-Chat 自带的
  // `generate_image` 是残废版（模型写死 gpt-image-2、尺寸写死 1024x1024、没有图生图），
  // Draw/Video/QrMerge 那一整排行为（参考图多图融合、画质、模型选择、历史落盘、境外 CDN
  // 取不回时的指路）**一行都没删**，只是回到侧栏那一页去了。
  //
  // 专家卡 route 指向作图/视频时切侧栏 tab，那条路在 `UWorkspace.summon()`（不在本组件里 ——
  // 会话是常驻挂载的，让它有导航权就是「每次进工作台被弹回作图页」那个 bug）。
];

// 起手词已挪到 `QuickPrompts.tsx`（两级：场景 tab + 该场景 7 条），轻助手和 Claude/Codex 共用一份。

// 跨轮历史裁剪在 `historyTrim.ts`：12 万字符掐头留尾 + 硬截断上界，纯函数可直跑边界测试。
function toApiMessages(items: Item[], systemText: string) {
    const msgs: { role: string; content: string }[] = [];
    for (const it of items) if (it.type === "text" && it.content) msgs.push({ role: it.role, content: it.content });
    return trimHistoryForPayload(msgs, systemText);
}
const fileToolLabel: Record<string, string> = { list_dir: "列目录", read_file: "读文件", write_file: "写文件", edit_file: "改文件", run_command: "跑命令" };

/** 行数（折叠摘要用）。 */
const lineCount = (s?: string | null) => (s ? s.split("\n").length : 0);

/** 操作详情默认折叠（测试报告 #002：bash/edit/write 详情全量平铺，几步下来对话全是墙）。
 *  摘要一行常驻（干了什么/多少行），点开才见全文 —— 审批卡和完成卡共用这一个。 */
function ToolDetail({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="mt-1.5 min-w-0">
      <button onClick={() => setOpen((v) => !v)} className="inline-flex items-center gap-1 text-[11px] text-ink-4 hover:text-ink-1">
        <ChevronRight size={11} className={cn("transition-transform", open && "rotate-90")} /> {label}
      </button>
      {open && children}
    </div>
  );
}

// 对话历史持久化 —— **后端文件化为主，localStorage 只作旧数据迁移源**（2026-08-25）。
//
// 历史版本只写 localStorage，有三个结构性病（fable5 架构评审实锤）：
// ① 配额满静默丢历史（聊得越多越容易丢）；② 「新建对话」不落 tasks.json → 存档变孤儿
// 永不回收；③ 删除要反向扫全库找前缀。现在真相源是 `~/.uking/chats/<sid>.jsonl`
// （chatstore.rs：append 增量 / replace 全量 / load 读回 / delete 级联），localStorage
// 里的旧存档**首次读取时自动搬进后端并清掉**，一次迁移，之后 localStorage 不再增长。
//
// 裁剪规则不变：丢待批准项（重载后 approval id 失效）、去图片 b64（撑爆配额）、截超长输出。
const CHAT_STORE_PREFIX = "uking.chat.";
function sanitizeForArchive(items: Item[]) {
  return items
    .filter((it) => it.type !== "approval")
    .map((it) =>
      it.type === "tool"
        ? { ...it, b64: undefined, oldStr: undefined, newStr: undefined, output: it.output && it.output.length > 4000 ? it.output.slice(0, 4000) + "\n…（历史已截断）" : it.output }
        : it,
    );
}
/** 旧 localStorage 存档读出（不删 key——删除只在水合合并确认后做，见组件内水合 effect）。 */
function readLegacyArchive(sessionId: string): Item[] | null {
  try {
    const raw = localStorage.getItem(CHAT_STORE_PREFIX + sessionId);
    if (!raw) return null;
    const arr = JSON.parse(raw);
    return Array.isArray(arr) && arr.length > 0 ? (arr as Item[]) : null;
  } catch {
    return null;
  }
}
/** 删旧 localStorage 存档（只在后端已确认有数据/已写入后调，绝不先删后写）。 */
function dropLegacyArchive(sessionId: string) {
  try { localStorage.removeItem(CHAT_STORE_PREFIX + sessionId); } catch { /* ignore */ }
}
/**
 * 同步初值：只有旧 localStorage 有数据时才先用它（水合 effect 会把 legacy 合进后端数据
 * 再统一 replace 落盘 + 删 key）；没有就给空数组等水合。**绝不在加载完成前触发保存。**
 */
function loadChatItemsSync(sessionId: string): Item[] {
  return readLegacyArchive(sessionId) ?? [];
}
async function fetchChatItems(sessionId: string): Promise<Item[]> {
  try {
    const r = await invoke<unknown[]>("chat_archive_load", { sessionId });
    return Array.isArray(r) ? (r as unknown as Item[]) : [];
  } catch {
    return [];
  }
}
function saveChatItems(sessionId: string, items: Item[]) {
  const trimmed = sanitizeForArchive(items);
  void invoke("chat_archive_replace", { sessionId, items: trimmed }).catch(() => {});
}

export function Chat({ onToast, sessionId = "native-chat", initialWorkspace = "", onTitle, expert, onInstallClaude, taskName, onStatus, onFindExpert, onSummonExpert }: { onToast?: (m: string) => void; sessionId?: string; initialWorkspace?: string; onTitle?: (t: string) => void; expert?: Expert; onInstallClaude?: () => void; taskName?: string; /** 点那排的「找专家」→ 切到左栏专家墙。 */ onFindExpert?: () => void;
  /** 点一位专家 → 带着他开一个会话（宿主负责建会话，本组件不自己造）。 */ onSummonExpert?: (e: Expert) => void;
  /** 这一轮跑起来了 / 跑完了 / 跑挂了 —— 宿主拿去染左侧列表那个小圆点。不传也照常能用。 */
  onStatus?: (s: "running" | "idle" | "error") => void }) {
  const { t } = useI18n();
  // 矮屏（见 lib/useViewport.ts）：顶栏和「按发送前该知道的两件事」那条在
  // 1366×768 上各自都还占着宽松间距，而对话正文只剩三四行。
  const { short } = useViewport();
  // 对话大脑：默认 Claude Code（世界级 agent，底层走虾盘云 deepseek·同计费·免配置）；专家按 enginePolicy 定，
  // 通用工作台无专家时也默认 claude。想省钱/纯作图可在下拉切「U-King 轻助手」。没装 Claude 时下方会引导一键装。
  const [engine, setEngine] = useState<Engine>(expert?.enginePolicy.default ?? "claude");
  // 专家的系统提示（persona + 可用技能）；无专家时是 base。engine 影响作图技能提示（原生工具 vs 跑脚本）。
  const systemText = useMemo(() => buildSystemPrompt(expert, engine), [expert, engine]);
  const [items, setItems] = useState<Item[]>(() => loadChatItemsSync(sessionId));
  // 后端水合 + 保存策略（2026-08-25，fable5 终审 FIX-FIRST 三项的修法）：
  //
  // 【水合】挂载/换会话时读 ~/.uking/chats/<sid>.jsonl。**合并式覆盖**：
  //   后端有数据 → 用后端的（并顺手把 legacy 迁进去：replace 合并结果 + 删旧 key）；
  //   后端为空但本地初值非空 → 说明是「localStorage 有、盘上还没有」的未迁移会话，
  //   此时才 replace 初值；两边都空 → 只开闸不写（空列表绝不落盘，防 Standby 假灯）。
  // 【竞态】loadedRef 是保存闸门：水合回来前用户发的消息先留在内存，水合到达时
  //   **只在 items 为空时整体采用**；若用户已经说了话（本地比后端新），保留本地、
  //   把后端历史接在前面 —— 两种顺序都不丢任何一侧。
  // 【写放大】保存走 500ms trailing 去抖：流式输出每条 delta 都换 items，
  //   全量 replace 若跟着 delta 跑就是每 token 重写整个档。去抖后一轮只落盘一两次。
  const loadedRef = useRef(false);
  const saveTimer = useRef<number | null>(null);
  useEffect(() => {
    loadedRef.current = false;
    let alive = true;
    fetchChatItems(sessionId).then((arr) => {
      if (!alive) return;
      setItems((prev) => {
        let next: Item[];
        if (arr.length > 0 && prev.length > 0) {
          // 两边都有：以「更长的一方」为准（粗略但安全的启发——正常流程二者应相等）
          next = arr.length >= prev.length ? arr : prev;
        } else if (arr.length > 0) {
          next = arr;
        } else {
          next = prev; // 后端空：保留本地（可能是 legacy 初值或全新会话）
        }
        if (next.length > 0) {
          // 确认性写入：迁移合并结果 / 首次建档都在这一步落定；空列表永远不写。
          void invoke("chat_archive_replace", { sessionId, items: sanitizeForArchive(next) }).catch(() => {});
        }
        dropLegacyArchive(sessionId); // 到这里后端已确认，legacy key 可以安全清掉
        loadedRef.current = true;
        return next;
      });
    });
    return () => { alive = false; };
  }, [sessionId]);
  // 每次消息变化落盘（去抖 500ms trailing）。加载完才允许写；卸载/换会话时冲掉挂起的定时器。
  useEffect(() => {
    if (!loadedRef.current) return;
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    if (items.length === 0) return; // 空列表不触发任何写
    saveTimer.current = window.setTimeout(() => {
      saveTimer.current = null;
      saveChatItems(sessionId, items);
    }, 500);
    return () => {
      if (saveTimer.current !== null) { window.clearTimeout(saveTimer.current); saveTimer.current = null; }
    };
  }, [sessionId, items]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [model, setModel] = useState(DEFAULT_MODEL);
  /**
   * Claude / Codex 那侧的模型覆盖。**从 `ChatPanel` 提上来的**（2026-08-18）——
   * 客户要「Claude Code 和 DeepSeek 模型选择合并在一起」，而大脑(engine) 的状态在这一层、
   * 模型的状态原来在 ChatPanel 里，**合并的前提是它们归同一个人管**。
   * 键沿用 ChatPanel 原来那个（`uking.chatpanel.model.<agent>`），老用户选过的不丢。
   */
  const [panelModel, setPanelModelRaw] = useState<Record<string, string>>(() => {
    const r: Record<string, string> = {};
    for (const a of ["claude", "codex"]) {
      try { r[a] = localStorage.getItem("uking.chatpanel.model." + a) ?? ""; } catch { r[a] = ""; }
    }
    return r;
  });
  const setPanelModel = useCallback((agent: string, m: string) => {
    setPanelModelRaw((cur) => ({ ...cur, [agent]: m }));
    try { localStorage.setItem("uking.chatpanel.model." + agent, m); } catch { /* 配额满：不落盘也能用 */ }
  }, []);

  /**
   * 「大脑 + 模型」合成**一个**选择器（2026-08-18，客户：「claudecode 和 deepseek 模型选择，
   * 合并在一起，放在目前的 deepseek 的位置」）。
   *
   * 🔴 为什么该合：拆成两个下拉是**把我们的实现细节漏给了用户** —— 在他脑子里
   * 「用 Claude Code 跑 DeepSeek Flash」是一件事，不是两件。分开还会造出一个
   * 他答不上来的问题：先选哪个？（选错顺序时模型列表还会变）。
   *
   * value 编码 `engine:model`。**冒号后面允许为空** = 「跟随驱动设置」，
   * 那是 Claude/Codex 的默认，不能丢（丢了等于强行替用户覆盖他在 AI 设置里配好的）。
   */

  /**
   * 框内只留**大脑**（5 项）。**模型进 `+`**（客户 2026-08-18：「太多了……把模型切换
   * 放到 + 里边去吧」）。
   *
   * 🔴 上一轮把大脑×模型做成笛卡尔积是错的：19 个模型 × 3 个大脑 = 61 项，
   * 一屏根本放不下（客户直接拍了张下拉盖住半个界面的图）。
   * **合并没错，错在合的维度** —— 「哪个脑子」是 5 选 1、是我们区别于 MiniMax/ClawX/
   * WorkBuddy 的东西（它们只有自己一个脑子，所以框内那个下拉就是模型）；
   * 「哪个模型」是 19 选 1、而且绝大多数人一辈子就用「跟随驱动设置」。
   * 常驻位给前者，抽屉给后者。
   */
  const brainSelect = (
    <ComposerSelect value={engine} onChange={(v) => setEngine(v as Engine)} icon={Bot} tone="accent"
      title={t("用哪个大脑干这活")}>
      {ENGINES.map((e) => (
        <option key={e.id} value={e.id}>{t(e.label).split("（")[0]}</option>
      ))}
    </ComposerSelect>
  );

  /** 当前大脑吃不吃模型覆盖。TUI 那两个跑真身，传什么都不生效 —— 不给假开关。 */
  const brainTakesModel = engine !== "claude-cli" && engine !== "hermes";
  /**
   * 工作台跟着**「AI 设置」那份供应商库**走，不再写死虾盘云（2026-08-21）。
   *
   * 🔴 以前这里三层全绑死：模型下拉只列 `XIAPAN_MODELS`、Key 固定取设备钱包、
   * 后端端点是个 const。客户自己有小米 TokenPlan / DeepSeek 官方套餐，在「AI 设置」
   * 里配好、四个 CLI 都切过去了，**唯独工作台这个对话框还在拿我们的 Key 打我们的端点**。
   * 那不是「默认值」，那是把自己写进了别人的必经之路 —— 要开源、要让人放心用，
   * 这条得先拆掉：虾盘云仍是开箱默认（不配也能立刻用），但它只是选项之一。
   *
   * 不带 `tool` = 取全局那份（`save_provider` 本来就存全局的 ~/.uking/providers.json）。
   */
  /** 设备钱包那把 Key（虾盘云专用）。别家供应商用它自己的 `api_key`，见下面 `effectiveKey`。 */
  const deviceKeyRef = useRef("");
  // 原来这里还存了一份完整的 DeviceKey 对象（`drawDeviceKey`）给右侧创作面板看余额/充值链接。
  // 面板 2026-08-23 撤回侧栏后没人要它了，一并删掉 —— 留着就是一个「谁在用？」的死状态。
  const [chatProviders, setChatProviders] = useState<ProviderPreset[]>([]);
  const [providerId, setProviderId] = useState("xiapan");
  useEffect(() => { invoke<ProviderPreset[]>("list_providers", {}).then((ps) => setChatProviders(ps ?? [])).catch(() => {}); }, []);
  const activeProvider = chatProviders.find((p) => p.id === providerId) ?? null;
  /** 虾盘云用设备钱包的 Key，别家用它自己填的那把。**绝不拿我们的 Key 去打别人的端点。** */
  const effectiveKey = activeProvider && !activeProvider.builtin_recharge
    ? (activeProvider.api_key ?? "")
    : deviceKeyRef.current;
  /** 虾盘云传空 = 后端走缺省端点（保持历史行为，也少一处能填错的地方）。 */
  const effectiveBase = activeProvider && !activeProvider.builtin_recharge
    ? (activeProvider.openai_base || null)
    : null;

  /**
   * 选项 id 编码 `供应商::模型`。
   *
   * 为什么还是**一个**下拉而不是加一个「选供应商」：上一轮把「大脑 × 模型」做成两个下拉，
   * 结论是「合并没错，错在合的维度」。同理，「用小米的 mimo-v2.5」在客户脑子里是一件事，
   * 不是「先选小米、再选 mimo」两件 —— 拆开还会造出「先选哪个」这种他答不上来的问题。
   */
  const encodeModel = (pid: string, m: string) => `${pid}::${m}`;
  const uk = engine === "uking";
  /** 虾盘云的模型清单来自共享的 `XIAPAN_MODELS`；别家就用它在「AI 设置」里填的那个默认模型。 */
  const xiapanId = chatProviders.find((p) => p.builtin_recharge)?.id ?? "xiapan";
  const commonModels = [
    ...XIAPAN_MODELS.filter((g) => !g.group.includes("全球旗舰"))
      .flatMap((g) => g.items)
      .map((m) => ({ id: encodeModel(xiapanId, m.id), label: t(m.label) })),
    // 客户自己配的那些家。没填 model 的不列 —— 列一个空模型只会打出一个看不懂的 400。
    ...chatProviders
      .filter((p) => !p.builtin_recharge && p.id !== "official" && p.model)
      .map((p) => ({ id: encodeModel(p.id, p.model), label: `${t(p.name)} · ${p.model}` })),
  ];
  const modelPicker = brainTakesModel
    ? {
        value: uk ? encodeModel(providerId, model) : (panelModel[engine] ?? ""),
        allowFollow: !uk,
        // 常用 = 前面那些组 + 客户自己的供应商；贵的 = 组名带「全球旗舰」的那组
        //（`lib/models.ts` 里本来就这么分的，我们不另立判据 —— 另立一份就会漂）。
        list: uk
          ? commonModels
          : XIAPAN_MODELS.filter((g) => !g.group.includes("全球旗舰")).flatMap((g) => g.items).map((m) => ({ id: m.id, label: t(m.label) })),
        pricey: XIAPAN_MODELS.filter((g) => g.group.includes("全球旗舰"))
          .flatMap((g) => g.items)
          .map((m) => ({ id: uk ? encodeModel(xiapanId, m.id) : m.id, label: t(m.label) })),
        onChange: (v: string) => {
          if (!uk) { setPanelModel(engine, v); return; }
          // 空 = 「跟随驱动设置」，uking 档不给这个选项，兜底回默认模型。
          const [pid, ...rest] = v.split("::");
          const m = rest.join("::");
          if (!v || !m) { setProviderId(xiapanId); setModel(DEFAULT_MODEL); return; }
          setProviderId(pid);
          setModel(m);
        },
      }
    : undefined;
  const [mode, setMode] = useState("ask");
  const [queue, setQueue] = useState<string[]>([]); // 提示词队列（忙时排队，空闲自动派发）
  const [workspace, setWorkspace] = useState(initialWorkspace);
  // 右侧面板：rightOpen 显隐；opened 记录挂载过的（保活）；ratio=对话列占比
  const [rightKind, setRightKind] = useState<RightKind>("preview");
  const [rightOpen, setRightOpen] = useState(false);
  const [opened, setOpened] = useState<Set<RightKind>>(new Set());
  const [ratio, setRatio] = useState(0.55);
  // 收起对话列：终端编程时把中间对话整列藏起，让右侧终端/预览全屏。只在右侧面板已打开时有意义。
  const [chatCollapsed, setChatCollapsed] = useState(false);
  /**
   * 终端右边那条文件栏（树 + 就地预览）。**跟「文件」tab 是同一个 FilesPanel**，
   * 不是第二份实现 —— 区别只在于它跟终端**并排**，不把终端顶掉。
   *
   * 为什么要它：右侧四个 tab 是互斥单选，在终端里干活时想看一眼刚产出的文件就得切走，
   * 终端连同正在跑的东西一起消失。客户的原话是「发到 ucli 里边好像啥都不是」。
   * 宽度记住 —— 会开这条栏的人多半要连着看好几个文件，每次重开都要拖一遍等于没给。
   */
  const [termFilesOpen, setTermFilesOpen] = useState(false);
  const [termFilePath, setTermFilePath] = useState<string | null>(null);
  const [termFilesWidth, setTermFilesWidth] = useState<number>(() => {
    const v = Number(localStorage.getItem("uking.term.filesWidth"));
    return v >= 260 && v <= 1200 ? v : 480;
  });
  // 右侧「预览/画布」：生成的图（可放大）/ 渲染的 HTML；zoom=图片缩放倍数
  const [preview, setPreview] = useState<Preview>(null);
  const [zoom, setZoom] = useState(1);
  // 对话正文字号（测试报告 #007：「字体大小固定」）。记住选择 —— 会调字号的人多半是眼睛累了，
  // 每次开页面再调一遍等于没给。范围卡在 12~22：再小读不清，再大一屏放不下几句话。
  const [chatFont, setChatFont] = useState<number>(() => {
    const v = Number(localStorage.getItem("uking.chat.font"));
    return v >= 12 && v <= 22 ? v : 13;
  });
  const bumpFont = (d: number) =>
    setChatFont((f) => {
      const n = Math.min(22, Math.max(12, f + d));
      localStorage.setItem("uking.chat.font", String(n));
      return n;
    });
  const bottomRef = useRef<HTMLDivElement>(null);
  // 右侧终端面板的命令式接口 + 待冲刷的标注（终端刚挂载还没 onReady 时先暂存）—— redline「发给终端」用
  const termApiRef = useRef<TermPanelApi | null>(null);
  const pendingPasteRef = useRef<string | null>(null);
  const rowRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  /** 终端 tab 的整块区域（终端列 + 文件栏），拖条按它的右边缘量宽度。 */
  const termAreaRef = useRef<HTMLDivElement>(null);
  /** **只是终端那一列**。让步链量的是它，不是整个右面板 —— 理由见下面那个 useEffect。 */
  const termColRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // 🔴 专家 `route`（作图 / 视频）的跳转**不在这里**了 —— 2026-08-25 第二次客户反馈
  // 「强制自动跳回作图页」。08-25 早上那版只稳定了回调身份，病根没动：
  // UWorkspace 把**每个会话都常驻挂载**（display 切换保活），所以只要历史上召唤过一次
  // 作图专家，那个会话就永远挂着；每次进「AI 工作台」它一挂载就 `onGoCreate` 一次 →
  // 用户被当场弹回作图页，等于工作台再也进不去。
  // **导航是「用户刚点了什么」的结果，不是「屏幕上挂着什么」的结果** —— 已挪到
  // `UWorkspace.summon()`（用户真点专家卡的那一刻，只跑一次）。
  // 把内容送进右侧预览并自动展开（生成图/点图放大/预览 HTML 都走这）
  const showPreview = useCallback((p: Preview) => {
    setPreview(p);
    setZoom(1);
    setOpened((s) => (s.has("preview") ? s : new Set(s).add("preview")));
    setRightKind("preview");
    setRightOpen(true);
  }, []);

  // 起手词落到输入框（不覆盖已输入内容），聚焦让用户接着写。**不自动发送**。
  //
  // `best` = 这活哪个大脑拿手 **+ 为什么**（作图/出片→轻助手；PPT/文档/表格→Claude 的技能包）。
  // 客户选的是「我要干什么」，不该是「我要用哪个 AI」—— 所以这里替他切。但三条底线：
  // 1. **说清楚为什么切**（不是偷偷换掉他选的东西）。理由**跟着条目走**，不写死在这句 toast 里——
  //    以前这里硬编码「自带出图/出片工具」，等办公类也标上 best，同一句话就成了错话。
  // 2. **可一键切回**（顶栏的大脑下拉一直在）
  // 3. 只在真有能力差时切；没标 best 的条目一律不动当前大脑
  const applyQuick = useCallback((tpl: string, _best?: Best) => {
    /* 🔴 **不再替用户切大脑**（2026-08-18 客户：「这个提示看起来很吓人，不要提示这个，
       就用 claudecode 就行了。没切换，就不要切换」）。
       原来点一条作图类起手词会自动 `setEngine(best.engine)` 并弹一条长 toast 解释理由。
       问题不在 toast 长 —— 在于**它替他做了一个他没要求的决定**，而且是在他刚点下去、
       注意力还在输入框上的时候。他要的是「把这句话填进来」，不是「顺便换个引擎」。
       `best` 保留在类型里（起手词元数据仍标着哪类活谁拿手），只是不再据此自动动手。 */
    setInput((v) => (v.trim() ? v : tpl));
    setTimeout(() => inputRef.current?.focus(), 0);
  }, []);

  // 拖文件/文件夹进对话 = 把真实路径贴进输入框（含空格自动加引号），跟内嵌终端一个套路。
  // dragDropEnabled:true 下 HTML5 拖放整体失效，必须走 Tauri 原生事件（useDropZone）。
  const [pendingImages, setPendingImages] = useState<string[]>([]);
  const insertPaths = useCallback((paths: string[]) => {
    const images = paths.filter(isImageFile);
    const ordinary = paths.filter((p) => !isImageFile(p));
    if (images.length) setPendingImages((old) => [...old, ...images.filter((p) => !old.includes(p))]);
    const labels = images.map((p) => `【已附图片：${fileLabel(p)}，发送时先识图】`).join(" ");
    setInput((v) => [v.trimEnd(), ordinary.length ? pathsToText(ordinary).trimEnd() : "", labels].filter(Boolean).join(" ") + " ");
    setTimeout(() => inputRef.current?.focus(), 0);
  }, []);
  const { ref: dropRef, over: dragOver } = useDropZone<HTMLDivElement>(insertPaths);

  /**
   * 把一个产出文件送进右侧预览。**轻助手和 Claude/Codex 共用这一个** ——
   * 以前预览只做在轻助手侧，而默认大脑是 Claude：客户让 Claude 做了个网页、出了张图，
   * 对话里只有一行 `Write: xxx.html ✓`，点不开、看不见。
   *
   * 路径可以是相对工作目录的（Claude 常给相对路径），也可以是绝对的（脚本产出常给绝对路径）。
   * 图片/视频/办公文档都走 asset 协议读字节，**必须先 `allow_fs_preview` 授权那个目录**，
   * 否则 403 白屏（踩过：asset scope 当初只放了 video）。
   *
   * **办公三件套（docx/xlsx/pptx/pdf…）交给 redline 内核**（`previewableExt` 判 `"doc"`）——
   * 那套 viewer 早就有，之前只接在文件树双击那一处。以前走到这儿只剩最后那行
   * `read_text_file`，对二进制不是报错就是满屏乱码：办公活做完了，客户在软件里看不到成果。
   */
  const previewFile = useCallback(async (p: string) => {
    // 🔴 兜底：网址永远不进文件预览。终端链接那条路已经在 fileLinks 里按 kind 分流了，
    // 但这个函数是**所有入口共用**的（工具卡、文件树、AI 回答里的产出行），谁喂一个
    // `https://…` 进来，下面就会拿它当路径去 read_text_file，客户看到的是
    // 「预览失败: 读取失败: 系统找不到指定的路径。(os error 3)」——2026-08-16 截图里那句。
    if (/^(https?|file):\/\//i.test(p)) {
      void openUrl(p).catch(() => onToast?.(t("打不开这个网址：{e}", { e: p })));
      return;
    }
    const isAbs = /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(p);
    if (!isAbs && !workspace) { onToast?.(t("先选一个工作文件夹才能定位这个文件")); return; }
    // 🔴 拼路径复用 `resolvePath`，别在这儿再写一份（宪法 12：公共能力复用不复制）。
    // 原来这行是 `(workspace + "\\" + p).replace(/\//g, "\\")` —— 在 Windows 上对，
    // 在 **Mac 上把绝对路径整个拧断**：`/Users/example/ws` + `\` + `out.md` 再把所有 `/` 换成 `\`
    // = `\Users\example\ws\out.md`，磁盘上不存在 → asset 协议 404。开发机是 Windows，
    // 这类错跟「优化大师念 PowerShell」是同一个盲区：只有 Mac 用户撞得到。
    const abs = resolvePath(p, workspace);
    // 先问一句「这个文件在不在」。不问的话，路径错了要等 redline 去 fetch 才炸，
    // 客户看到的是一句 **不带路径** 的 `读取文件失败: HTTP 404` —— 他没法判断是文件没生成、
    // 还是我们找错了地方。带上路径，至少能自己看出来。
    const info = await invoke<{ exists: boolean }>("produced_file_info", { path: abs }).catch(() => null);
    if (info && !info.exists) {
      onToast?.(t("找不到这个文件：{p}（AI 可能还没写出来，或路径不是相对这个工作文件夹）", { p: abs }));
      return;
    }
    const ext = (p.split(/[?#]/)[0].split(".").pop() ?? "").toLowerCase();
    const allowDir = () => invoke("allow_fs_preview", { path: abs.replace(/[\\/][^\\/]*$/, "") }).catch(() => {});
    try {
      if (["png", "jpg", "jpeg", "webp", "gif", "bmp", "mp4", "webm", "mov"].includes(ext)) {
        await allowDir();
        const kind = ["mp4", "webm", "mov"].includes(ext) ? "video" : "image";
        showPreview({ kind, src: convertFileSrc(abs), caption: p } as Preview);
        return;
      }
      if (previewableExt(p) === "doc") {
        await allowDir();
        showPreview({ kind: "doc", path: abs, caption: p });
        return;
      }
      // 网页：走 asset 协议**真文件地址**，相对路径的图片/样式才加载得到（见 Preview 类型的注释）。
      // 其余文本（.md/.log/日志…）没有相对资源可言，照旧读进来当整段 HTML 塞进去。
      if (["html", "htm", "svg"].includes(ext)) {
        await allowDir();
        showPreview({ kind: "html", src: convertFileSrc(abs), path: abs, caption: p });
        return;
      }
      // 🔴 **文本文件交给 redline，别当 HTML 塞进去**（2026-08-18 客户实拍：
      //    点终端里的 .md，预览出来是一坨没有换行的文字）。
      //    原来这里 `read_text_file` 读出来直接 `kind:"html"` —— 而 HTML 把换行全吃掉，
      //    整篇 markdown 挤成一段。**顺带还是个注入面**：.md/.log 里写了 `<script>` 会被执行。
      //    redline 那套按扩展名分流（`viewers/registry.ts`：md→MdViewer、其余→TextViewer），
      //    换行、标题、代码块都对，而且不执行内容。
      await allowDir();
      showPreview({ kind: "doc", path: abs, caption: p });
    } catch (e) {
      onToast?.(t("预览失败: {e}", { e: String(e) }));
    }
  }, [workspace, showPreview, onToast, t]);

  useEffect(() => {
    invoke<DeviceKey>("get_device_key")
      .then((dk) => { deviceKeyRef.current = dk?.key ?? ""; })
      .catch(() => {});
  }, []);
  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" }); }, [items, busy]);
  // 右侧面板收起后自动恢复对话列（避免下次开面板时对话仍是藏着的、让人以为丢了）
  useEffect(() => { if (!rightOpen) setChatCollapsed(false); }, [rightOpen]);


  const pickWorkspace = useCallback(async () => {
    const dir = await open({ directory: true, title: t("选择工作文件夹（AI 和终端都在这里面读写文件、跑命令）") }).catch(() => null);
    if (typeof dir === "string") setWorkspace(dir);
  }, [t]);

  /**
   * 输入框**正下方**那条极轻的上下文行：现在在哪个文件夹干活。
   * MiniMax Code / ClawX / WorkBuddy 三家都摆在这个位置 —— 它是**上下文**不是**能力**：
   * 要一眼看得见（不然「AI 会把文件写到哪」全靠猜），但不该跟 `+`、模型、发送抢工具条。
   * 🔴 上一轮我把它整个收进了 `+`，那是收过头了：`+` 是抽屉，抽屉里的东西默认不可见，
   * 而「在哪干活」是**每一句话都成立的前提**，不该要点开才知道。
   */
  /* 🔴 输入框下面那条工作目录**已删**（2026-08-18 客户：「下面的工作目录，不显示了吧，
     上面右侧有文件预览功能」）。理由成立：右侧文件面板就开在这个目录上，路径在那儿一直看得见，
     这条是第三处（还有一处在 `+` 菜单）。
     **会话的工作目录仍然锁死**（建会话时定，会话内不许改）—— 那是修 `--resume` 必炸的关键，
     跟显不显示是两件事，别一起回退。 */

  /* 「打开方式」已删（2026-08-18）：右侧文件预览面板里本来就有，
     同一个动作两个入口，改一个漏一个（客户点名要去掉 `+` 里那组）。 */
  const togglePanel = useCallback((kind: RightKind) => {
    const meta = RIGHT_META.find((m) => m.kind === kind)!;
    if (meta.needsWs && !workspace) { onToast?.(t("先选一个工作文件夹，AI 和终端都在里面干活")); return; }
    setRightOpen((wasOpen) => {
      if (wasOpen && rightKind === kind) return false; // 再点当前面板 = 收起
      setOpened((s) => (s.has(kind) ? s : new Set(s).add(kind)));
      setRightKind(kind);
      return true;
    });
  }, [workspace, rightKind, onToast, t]);

  // 输入框 `/` 指令面板列的是**这些**（宿主真有的动作），不是编出来的「技能」。
  // 每一条都直接调上面那几个已经存在的 handler —— 没有第二份实现，也就没有「菜单里有、点了没反应」的可能。
  const slashCommands = useMemo(
    () => [
      { label: "开终端", hint: "右侧开一个终端", run: () => togglePanel("terminal") },
      { label: "开文件面板", hint: "浏览工作文件夹", run: () => togglePanel("files") },
      { label: "开预览", hint: "看生成的图 / 网页", run: () => togglePanel("preview") },
      { label: "换工作文件夹", hint: "AI 在哪儿干活", run: () => void pickWorkspace() },
      { label: "清空对话", hint: "这个会话从头开始", run: () => setItems([]) },
    ],
    [togglePanel, pickWorkspace],
  );
  const composer = useComposerMenu({ value: input, setValue: setInput, textareaRef: inputRef, workspace, commands: slashCommands, onQuickPick: applyQuick });

  // 终端面板挂载回调：存接口，并冲刷等待中的文本（writeToActive 会自建 PTY，无需重试）
  const handleTermReady = useCallback((api: TermPanelApi) => {
    termApiRef.current = api;
    if (pendingPasteRef.current != null) {
      api.paste(pendingPasteRef.current);
      pendingPasteRef.current = null;
    }
  }, []);
  /**
   * 把一段文本贴进当前终端（**不回车**，用户自己决定要不要执行）并切到终端面板。
   * 用它的有：任务看板「接着干」（termInbox）、AI 回答里代码块的「贴进终端」（MiniMd）、
   * ChatPanel 的 onRunInTerminal、以及「装 Claude」那类引导按钮。
   */
  const pasteToTerminal = useCallback((text: string) => {
    setOpened((s) => (s.has("terminal") ? s : new Set(s).add("terminal")));
    setRightKind("terminal");
    setRightOpen(true);
    const api = termApiRef.current;
    if (api) api.paste(text);
    else pendingPasteRef.current = text; // 终端面板刚挂载，等 onReady 冲刷
  }, []);
  // redline 宿主（读字节 / asset URL / markdown 渲染 / 外部打开）。**和 FilesPanel 同一个工厂**。
  const redlineHost = useMemo(() => createTauriRedlineHost(), []);

  /**
   * 终端里点了一条文件路径。**跟别处的「预览」不是同一件事**：那些可以切走面板，
   * 这里不行 —— 终端一消失，客户正在跑的东西就看不见了。所以开右边那条文件栏就地看。
   *
   * 视频例外：redline 没有视频 viewer（会落到 TextViewer 报「二进制文件」），
   * 交回给通用 previewFile 走预览 tab 的 <video>。网址同理，它会 openUrl。
   */
  const openFromTerminal = useCallback(
    (p: string) => {
      const ext = (p.split(/[?#]/)[0].split(".").pop() ?? "").toLowerCase();
      if (/^(https?|file):\/\//i.test(p) || ["mp4", "webm", "mov"].includes(ext)) {
        void previewFile(p);
        return;
      }
      const isAbs = /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(p);
      if (!isAbs && !workspace) { onToast?.(t("先选一个工作文件夹才能定位这个文件")); return; }
      setTermFilePath(resolvePath(p, workspace));
      setTermFilesOpen(true);
    },
    [workspace, previewFile, onToast, t],
  );

  /**
   * 终端 ↔ 文件栏 的拖条。跟 startDrag 一样按住拖，但它夹的是**像素宽度**不是比例：
   * 比例会让文件栏在宽屏上宽得离谱、在窄屏上窄到没法看。给终端硬留 240px ——
   * TUI 按列排版，再窄就是错位（同让步链那条注释的理由）。
   */
  const startTermFilesDrag = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const el = termAreaRef.current;
    if (!el) return;
    const move = (ev: MouseEvent) => {
      const rect = el.getBoundingClientRect();
      const w = Math.round(rect.right - ev.clientX);
      setTermFilesWidth(Math.min(Math.max(260, w), Math.max(260, rect.width - 240)));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setTermFilesWidth((w) => {
        localStorage.setItem("uking.term.filesWidth", String(w));
        return w;
      });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }, []);

  /**
   * 顶栏「对话 ↔ 终端」主切换。
   *
   * 只是两个既有状态的组合，不新增第三种模式：
   *  - 终端态 = 右面板开着且停在 terminal
   *  - 对话态 = 右面板收起（对话列拿回整宽）
   * 两边都先 `setChatCollapsed(false)` —— 上次把对话列藏起来的人，点「对话」时要的就是它回来。
   */
  const cliMode = rightOpen && rightKind === "terminal";
  const showCliMode = useCallback(() => {
    setChatCollapsed(false);
    setOpened((s) => (s.has("terminal") ? s : new Set(s).add("terminal")));
    setRightKind("terminal");
    setRightOpen(true);
  }, []);
  const showChatMode = useCallback(() => {
    setChatCollapsed(false);
    setRightOpen(false);
  }, []);

  const startDrag = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const el = rowRef.current;
    if (!el) return;
    const move = (ev: MouseEvent) => {
      const rect = el.getBoundingClientRect();
      const r = (ev.clientX - rect.left) / rect.width;
      setRatio(Math.min(0.78, Math.max(0.3, r)));
    };
    const up = () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }, []);

  /**
   * 让步链：终端宽度不够就请两级导航栏让位（推导与规矩见 `lib/yieldChain.ts`）。
   *
   * **只有终端这一档报数。** 文件 / 浏览器 / 预览窄一点还能看，TUI 不行 —— 它按列排版，
   * 少几列就是错位。让步的理由必须是「终端饿了」，不能是「右侧面板窄」，
   * 否则看张图也要把客户的会话列表收掉。
   *
   * 🔴 用 ResizeObserver 而不是 window.resize：面板宽度还受**用户拖 ratio**、
   * **用户拖会话栏宽度**影响，这两下都不触发 window.resize，漏了就等于没做。
   * 🔴 rAF 合并同帧多次回调：让步会改布局、改完 observer 又回调一次，
   * 不合并就是一串同步 setState（同 `useTermGroup` 那个防抖 fit 的理由）。
   *
   * 🔴 量的是 `termColRef`（**终端那一列**）不是 `panelRef`（整个右面板）。
   * 自从终端右边能开文件栏，这两个宽度就不是一回事了：开栏时面板总宽一点没变、
   * 终端却被挤掉几百像素。量面板 = 终端已经饿到错位了，让步链还以为一切正常。
   */
  useEffect(() => {
    const el = termColRef.current;
    if (!el || !rightOpen || rightKind !== "terminal") {
      releaseYield(sessionId);
      return;
    }
    let raf = 0;
    const measure = () => {
      raf = 0;
      reportTermWidth(sessionId, el.getBoundingClientRect().width);
    };
    const ro = new ResizeObserver(() => { if (!raf) raf = requestAnimationFrame(measure); });
    ro.observe(el);
    measure();
    return () => {
      ro.disconnect();
      if (raf) cancelAnimationFrame(raf);
      releaseYield(sessionId);
    };
  }, [sessionId, rightOpen, rightKind]);

  const copyText = useCallback((s: string) => {
    void copyToClipboard(s).then((ok) => onToast?.(ok ? t("已复制") : t("复制失败，请手动选中复制")));
  }, [onToast, t]);

  const approve = useCallback((id: string, approved: boolean) => {
    invoke("chat_approve", { approvalId: id, approved }).catch(() => {});
    setItems((prev) => prev.map((it) => (it.type === "approval" && it.id === id ? { ...it, decided: approved ? "approved" : "rejected" } : it)));
  }, []);

  // 实际发一轮（text 已就绪）；send 和提示词队列都调它
  const runChat = useCallback(async (text: string) => {
    // 幂等初始化；collector 未启动时 span 导出只会 warn，绝不能影响客户这轮对话。
    void initOtel().catch((error) => console.warn("[otel] initialization failed", error));
    if (items.length === 0) onTitle?.(text.length > 18 ? text.slice(0, 18) + "…" : text); // 首条消息当会话标题
    const history: Item[] = [...items, { type: "text", role: "user", content: text }];
    setItems(history);
    setBusy(true);
    onStatus?.("running");
    const payload = toApiMessages(history, systemText);
    await wrapGenAICall({
      model,
      operation: "chat",
      input: text,
      attributes: { "gen_ai.system_prompt.length": systemText.length, "uking.agent": "chat_send" },
    }, async (otel) => {
    const ch = new Channel<any>();
    ch.onmessage = (ev: any) => {
      if (ev.kind === "delta" && ev.text) {
        otel.firstToken();
        setItems((prev) => { const n = [...prev]; const l = n[n.length - 1]; if (l?.type === "text" && l.role === "assistant") n[n.length - 1] = { ...l, content: l.content + ev.text }; else n.push({ type: "text", role: "assistant", content: ev.text }); return n; });
      } else if (ev.kind === "tool") {
        if (ev.phase === "result" || ev.phase === "error") otel.tool(ev.name || "unknown", ev.prompt ?? ev.command, ev.output ?? ev.message);
        setItems((prev) => {
          const n = [...prev];
          if (ev.phase === "start") { n.push({ type: "tool", name: ev.name, phase: "running", prompt: ev.prompt, path: ev.path, command: ev.command }); return n; }
          for (let i = n.length - 1; i >= 0; i--) {
            const it = n[i];
            if (it.type === "tool" && it.name === ev.name && it.phase === "running") {
              // 视频出片进度只显示最新百分比（替换，不累加成「10%20%30%」）；命令输出仍追加
              if (ev.phase === "output") { n[i] = { ...it, output: it.name === "generate_video" ? (ev.chunk ?? "") : (it.output ?? "") + (ev.chunk ?? "") }; return n; }
              if (ev.phase === "result") { n[i] = { ...it, phase: "done", b64: ev.b64, url: ev.url, path: ev.path ?? it.path, output: ev.output ?? it.output, oldStr: ev.old ?? it.oldStr, newStr: ev.new ?? it.newStr, isNew: ev.is_new ?? it.isNew }; return n; }
              n[i] = { ...it, phase: "error", message: ev.message }; return n;
            }
          }
          if (ev.phase !== "output") n.push({ type: "tool", name: ev.name, phase: ev.phase === "result" ? "done" : "error", path: ev.path, command: ev.command, output: ev.output, b64: ev.b64, url: ev.url, message: ev.message, oldStr: ev.old, newStr: ev.new, isNew: ev.is_new });
          return n;
        });
        // 生成的图自动进右侧「预览」大图（用户要「图片放右侧预览」）
        if (ev.name === "generate_image" && ev.phase === "result") {
          const src = ev.b64 ? `data:image/png;base64,${ev.b64}` : ev.url;
          if (src) showPreview({ kind: "image", src });
        }
        // 生成的视频自动进右侧「预览」播放（asset 协议流式读磁盘 mp4，不走 IPC）
        if (ev.name === "generate_video" && ev.phase === "result" && ev.path) {
          showPreview({ kind: "video", src: convertFileSrc(ev.path), caption: t("生成的视频") });
        }
      } else if (ev.kind === "approval") {
        setItems((prev) => [...prev, { type: "approval", id: ev.id, tool: ev.tool, action: ev.action, inputKeys: ev.input_keys, path: ev.path, bytes: ev.bytes, command: ev.command, oldStr: ev.old, newStr: ev.new }]);
      } else if (ev.kind === "usage") {
        const promptTokens = Number(ev.input_tokens);
        const completionTokens = Number(ev.output_tokens);
        otel.response({
          id: ev.request_id ?? ev.response_id ?? null,
          promptTokens: Number.isFinite(promptTokens) ? promptTokens : null,
          completionTokens: Number.isFinite(completionTokens) ? completionTokens : null,
          totalTokens: Number.isFinite(promptTokens) && Number.isFinite(completionTokens) ? promptTokens + completionTokens : null,
        });
      } else if (ev.kind === "done") {
        otel.response({ id: ev.request_id ?? ev.response_id ?? null });
        setBusy(false);
        // 左侧小圆点的真相源。以前只弹一条 toast —— toast 会自己消失，
        // 人不在这个会话上就等于没通知过，跑挂了没有任何地方留痕。
        onStatus?.(ev.status === "error" ? "error" : "idle");
        if (ev.status === "error") onToast?.(ev.message || t("对话失败"));
      }
    };
    try {
      await invoke("chat_send", { taskId: sessionId, messages: payload, model, apiKey: effectiveKey, baseUrl: effectiveBase, workspace: workspace || null, approvalMode: mode, onEvent: ch });
    } catch (e) { setBusy(false); onStatus?.("error"); onToast?.(t("对话启动失败: {e}", { e: String(e) })); }
    });
  }, [items, model, workspace, mode, onToast, showPreview, sessionId, onTitle, systemText, t, onStatus, effectiveKey, effectiveBase]);

  // 提示词队列（借 OpenGUI）：忙时回车/发送把提示词排队，本轮结束自动派发下一条。
  const send = useCallback(async () => {
    const text = input.trim();
    if (!text) return;
    // 没 Key 的两种情况要分开说 —— 「等一下」和「你得去填」是完全不同的两件事，
    // 混成一句话，用自定义供应商的客户会一直等一个永远不会到的东西。
    if (!effectiveKey) {
      return onToast?.(
        activeProvider && !activeProvider.builtin_recharge
          ? t("{name} 还没填 API Key —— 去「AI 设置 → 供应商库」补上", { name: t(activeProvider.name) })
          : t("还没拿到设备 Key，稍等一下再试"),
      );
    }
    const images = pendingImages;
    setInput("");
    setPendingImages([]);
    let prepared = text;
    try {
      if (images.length) prepared += `\n\n${await describeImages(images, text)}`;
    } catch (e) {
      setInput(text);
      setPendingImages(images);
      onToast?.(t("图片识别失败: {e}", { e: String(e) }));
      return;
    }
    if (busy) { setQueue((q) => [...q, prepared]); return; } // 忙 → 入队
    void runChat(prepared);
  }, [input, busy, runChat, onToast, t, pendingImages]);
  useEffect(() => {
    // 空闲且有排队 → 自动发下一条
    if (!busy && queue.length > 0) {
      const next = queue[0];
      setQueue((q) => q.slice(1));
      void runChat(next);
    }
  }, [busy, queue, runChat]);

  /**
   * 护照交接的收件：**这个会话是被一张任务护照点名建出来的**，进来先把状态读进去。
   *
   * 为什么不是「把提示词填进输入框让用户自己按回车」：那正是旧实现（写剪贴板）的病
   * ——它把「交接」降级成「用户自己再操作一次」，中间任何一步断了都没人知道。
   * 交接是用户已经点过的那一下，这里要做的是**兑现**它，不是再问一遍。
   *
   * 🔴 **发的路要跟着大脑走**：只有 `uking` 走本组件的 `runChat`(chat_send)；
   * claude / codex 的对话真身在 `ChatPanel`（`${agent}_send`）。一开始我把两条路
   * 当成一条，交接给 Claude 会静默发进一个当时根本没在用的通道 —— 界面上还会画成
   * 「已送达」。**送达回执必须由真正发出去的那条路签字**，所以 claude/codex 的回执
   * 走 `ChatPanel.onSeedSent`，不在这里提前签。
   */
  const [seed, setSeed] = useState<Handoff | null>(null);
  const handoffDone = useRef(false);
  useEffect(() => {
    if (handoffDone.current) return;
    const h = takeHandoff(sessionId);
    if (!h) return;
    handoffDone.current = true;
    setEngine(h.engine);
    setSeed(h);
  }, [sessionId]);

  /**
   * 任务看板点了「接着干」→ 把那条续接命令（`claude --resume <sid>`）贴进本会话的终端。
   *
   * 走 `pasteToTerminal`：它会把终端面板打开并切过去，终端还没挂载时先存着、
   * `onReady` 再冲刷 —— 这条路已经被「预览里划一段发给终端 Agent」验过，不另写一份。
   *
   * 🔴 **只贴不回车**。见 `termInbox.ts`：起一次 AI 会话是花钱的写操作，
   * 最后那一下由人按。
   */
  useEffect(() => {
    const cmd = takeTermCmd(sessionId);
    if (cmd) pasteToTerminal(cmd);
  }, [sessionId, pasteToTerminal]);

  // uking 大脑：本组件自己发。Key 是异步取的，没到手就发会撞「还没拿到 Key」——
  // 等它到位再发，而不是丢一句 toast 让用户自己重来（那又回到「交接得靠人补一刀」）。
  //
  // 🔴 但**必须有上限**：自定义供应商没填 Key 时，那把 Key 是永远不会到的，
  // 原来这个无限重试是靠「设备 Key 迟早会到」这个前提成立的，供应商可选之后这个前提没了。
  // 等到超时就说人话，别让人对着一个永远转不完的圈猜。
  useEffect(() => {
    if (!seed || seed.engine !== "uking") return;
    let alive = true;
    let tries = 0;
    const fire = () => {
      if (!alive) return;
      if (!effectiveKey) {
        if (++tries > 20) { // ≈6 秒：够等一次网络往返，不够等一个不存在的东西
          onToast?.(t("还没拿到可用的 API Key，先去「AI 设置」确认这家供应商配好了"));
          setSeed(null);
          return;
        }
        window.setTimeout(fire, 300);
        return;
      }
      void runChat(seed.prompt);
      deliver(sessionId, seed.passportId);
      setSeed(null);
    };
    fire();
    return () => { alive = false; };
  }, [seed, runChat, sessionId, effectiveKey, onToast, t]);

  const stop = useCallback(() => { invoke("chat_interrupt", { taskId: sessionId }).catch(() => {}); setBusy(false); onStatus?.("idle"); }, [sessionId, onStatus]);

  /** TUI 档（claude-cli / hermes）进终端要敲的命令。非 TUI 档取不到，下面的分支也走不到它。 */
  const tuiCmd = ENGINE_TUI_CMD[engine] ?? engine;

  return (
    <section ref={rowRef} className="flex gap-0 h-full min-h-0">
      {/* 左：对话列（面板收起时居中窄栏，展开时按比例分宽；chatCollapsed 时整列藏起让终端全屏） */}
      <div
        className="flex flex-col min-w-0 h-full"
        style={{
          width: rightOpen ? (chatCollapsed ? 0 : `${ratio * 100}%`) : "100%",
          display: rightOpen && chatCollapsed ? "none" : "flex",
        }}
      >
        <div className={cn("flex flex-col h-full min-h-0", !rightOpen && "max-w-[820px] mx-auto w-full")}>
          <header className={cn("flex items-center gap-2 border-b border-white/[0.06] flex-wrap", short ? "pb-1.5" : "pb-3 mb-1")}>
            {expert ? <span className="text-[16px]">{expert.emoji}</span> : <Bot size={16} className="text-accent" />}
            <span className="text-[14px] font-semibold text-ink-0">{expert ? expert.name : "U-Workspace"}</span>
            {/* 大脑选择器**已挪到输入框正下方那条**（和工作文件夹/打开方式同一条）——
                它跟「在哪儿干活」是同一类信息：按发送之前该知道的事。这里不留副本。 */}
            {/* 引擎升级提示 chip：专家默认引擎下，若声明了 escalate，一键切到更强引擎 */}
            {expert?.enginePolicy.escalate && engine === expert.enginePolicy.default && (
              <button onClick={() => setEngine(expert.enginePolicy.escalate!)} title={t("复杂/多文件任务，切到更强的引擎")} className="inline-flex items-center gap-1 h-7 px-2 rounded-lg bg-amber-500/[0.12] border border-amber-500/30 text-[11px] text-amber-500 hover:bg-amber-500/[0.2]">
                {t("任务较重？切 {name} 更强", { name: (ENGINES.find((x) => x.id === expert.enginePolicy.escalate)?.label ?? "").split("（")[0] })}
              </button>
            )}
            {/* 正文字号（测试报告 #007「字体大小固定」）。放在顶栏而不是设置页：会想调它的时候
                人正在读，跑去设置页调完再回来那一趟，等于让他放弃。
                🔴 客户 2026-08-18 问「这个放大缩小有用吗，没用就删除」——**有用**，
                它改的是对话正文的 fontSize（下面那个 `style={{ fontSize: chatFont }}`）。
                但它只对**对话**有用，终端/预览/浏览器态点了什么都不会变 ——
                一个在当前界面上不起作用的控件，摆着就是在骗人。所以只在对话态出现。 */}
            {!cliMode && (
            <div className="inline-flex items-center rounded-lg bg-bg-1 border border-white/[0.08] h-7">
              <button
                onClick={() => bumpFont(-1)}
                disabled={chatFont <= 12}
                title={t("字小一点")}
                className="w-6 h-7 grid place-items-center text-ink-3 hover:text-ink-0 disabled:opacity-40"
              >
                <ZoomOut size={12} />
              </button>
              <span className="text-[10.5px] text-ink-5 w-6 text-center tabular-nums">{chatFont}</span>
              <button
                onClick={() => bumpFont(1)}
                disabled={chatFont >= 22}
                title={t("字大一点")}
                className="w-6 h-7 grid place-items-center text-ink-3 hover:text-ink-0 disabled:opacity-40"
              >
                <ZoomIn size={12} />
              </button>
            </div>
            )}
            {/* 工作文件夹 / 打开方式 / 审批档**已挪到输入框正下方**（见下面的 WorkFooter）——
                它们是「按发送之前该知道的两件事」，不该藏在顶栏一排图标里。
                这里不留副本：同一状态两个入口，改一个漏一个。 */}
            <div className="ml-auto flex items-center gap-1.5">
              {/* ★ 主切换：对话 ↔ 终端。**这是我们跟 dsh / ClawX / Codex 那类工具的区别** ——
                  它们只有对话，我们的终端是同级的一等公民。所以给它一等的开关（分段控件），
                  而不是混在预览/文件/浏览器那排面板图标里当第四个。
                  按钮上写人话（客户认「终端」不认「U-CLI」），代号进 tooltip —— 代号是给报 bug /
                  看文档时指认是哪一块用的（CLAUDE.md 的命名约定）。

                  🔴 **这两颗不挂 data-action-id**：宪法第 13 条 ——「切标签、悬停、拖窗口、动画
                  是界面动作，不进核心」。它俩只是改本地 state（面板开不开、停在哪个 kind），
                  没有业务语义。挂了会被 `action bindings` 判成 stale（指向不存在的动作 =
                  自动化点过去是空的），而它当场就抓到了。 */}
              <div className="inline-flex items-center rounded-lg border border-white/[0.08] bg-bg-1 p-0.5 mr-1">
                <button
                  onClick={showChatMode}
                  title={t("U-Chat（对话）")}
                  className={cn("inline-flex items-center gap-1 h-6 px-2.5 rounded-md text-[11px] transition-colors",
                    cliMode ? "text-ink-3 hover:text-ink-0" : "bg-accent/[0.16] text-ink-0 font-medium")}>
                  <MessageSquare size={12} /> {t("对话")}
                </button>
                <button
                  onClick={showCliMode}
                  disabled={!workspace}
                  title={t("U-CLI（终端）")}
                  className={cn("inline-flex items-center gap-1 h-6 px-2.5 rounded-md text-[11px] transition-colors disabled:opacity-40",
                    cliMode ? "bg-accent/[0.16] text-ink-0 font-medium" : "text-ink-3 hover:text-ink-0")}>
                  <Terminal size={12} /> {t("终端")}
                </button>
              </div>
              {/* 🔴 「文件」开关**不在这条**（2026-08-18 移走）。这里是**对话列**的顶栏，
                  而收起对话列看终端全屏时这条整个消失 —— 开关跟着一起没了，文件栏却还开着，
                  于是「打开了没地方关」（客户实拍）。当时那颗按钮旁边还留着面板头的「文件」tab，
                  点它是把整个面板切走、终端消失，跟关旁栏完全是两回事。
                  **开关要跟着它控制的东西走**：终端在面板里，开关就该在面板头（见下方 panelRef 那段）。 */}
              {/* 辅助面板：预览 / 文件 / 浏览器。终端已提到上面的主切换里，这里不再重复一个入口。
                  🔴 终端态下**连「文件」也不显示**：那时面板头那颗「文件」已经是文件入口（在终端旁开一栏），
                  两颗同名按钮挨在一起、点了行为还不一样（一个开旁栏、一个把整个面板切走），
                  比没有更糟。同一时刻只该有一个「文件」。 */}
              {RIGHT_META.filter((m) => m.kind !== "terminal" && !(cliMode && m.kind === "files")).map(({ kind, label, title, icon: Icon, lab }) => (
                <button key={kind} onClick={() => togglePanel(kind)} title={lab ? t("{label}（测试中：依赖 agent-browser，国内网络下常起不来）", { label: t(title) }) : t(title)}
                  className={cn("inline-flex items-center gap-1 h-7 px-2 rounded-lg border text-[11px]",
                    rightOpen && rightKind === kind ? "bg-accent/15 border-accent/40 text-ink-0" : "bg-bg-1 border-white/[0.08] text-ink-2 hover:border-accent/30")}>
                  <Icon size={12} /><span className="hidden sm:inline">{t(label)}</span>
                  {lab && <span className="hidden lg:inline text-[9px] leading-none px-1 py-0.5 rounded bg-amber-400/15 text-amber-400/90">{t("测试中")}</span>}
                </button>
              ))}
              {/* 模型选择器也已挪进输入框卡片的底部工具条（对齐 Codex / WorkBuddy） */}
              {/* 收起对话列：右侧面板打开时才有意义 —— 一键把中间对话藏起，终端/预览全屏 */}
              {rightOpen && (
                <button onClick={() => setChatCollapsed(true)} title={t("收起对话列，终端/预览全屏")}
                  className="inline-flex items-center justify-center h-7 w-7 rounded-lg bg-bg-1 border border-white/[0.08] text-ink-2 hover:border-accent/40 hover:text-ink-0">
                  <PanelLeftClose size={13} />
                </button>
              )}
            </div>
          </header>

          {engine !== "uking" ? (
            /* 最强工具当大脑：claude 走结构化卡片(agent/claude.rs)，codex/hermes 走真身 TUI 终端。都需工作文件夹 */
            !workspace ? (
              <div className="flex-1 grid place-items-center text-center px-6">
                <div>
                  <Bot size={28} className="text-accent/60 mx-auto mb-2" />
                  <div className="text-[13px] text-ink-2">{t("{name} 大脑要在一个工作文件夹里干活", { name: t(ENGINES.find((e) => e.id === engine)?.label ?? "") })}</div>
                  <button onClick={pickWorkspace} className="mt-2 inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent text-white text-[12px]"><FolderOpen size={13} /> {t("选工作文件夹")}</button>
                </div>
              </div>
            ) : engine === "claude" || engine === "codex" ? (
              /* claude / codex：都走结构化卡片（ChatPanel + agent/claude.rs / agent/codex.rs 的 JSONL） */
              <ChatPanel key={engine} taskId={`${sessionId}-${engine}`} cwd={workspace} active agent={engine} title={taskName} system={expert ? systemText : undefined} onRunInTerminal={pasteToTerminal} onStatus={onStatus} onQuickPick={applyQuick} onPreview={previewFile}
                brainSlot={brainSelect}
                modelPicker={modelPicker}
                experts={{
                  value: expert?.id ?? "",
                  list: allExperts().map((e) => ({ id: e.id, label: `${e.emoji} ${t(e.name)}` })),
                  onChange: (id: string) => {
                    if (id === "__hire__") { onFindExpert?.(); return; }
                    const e = allExperts().find((x) => x.id === id);
                    if (e) onSummonExpert?.(e);
                  },
                }}
                seedPrompt={seed && seed.engine === engine ? seed.prompt : null}
                onSeedSent={() => { if (seed) { deliver(sessionId, seed.passportId); setSeed(null); } }}
                onGoManage={() => (engine === "claude" && onInstallClaude ? onInstallClaude() : onToast?.(t("请先在「① 装 AI」装好该工具并在「② 虾盘云」一键配好驱动")))} />
            ) : (
              /* claude-cli / hermes：不代驱动，直接在中间开 U-CLI 跑它本人的 TUI（需先一键配好）。
                 🔴 命令来自 ENGINE_TUI_CMD，**不是 engine id** —— `claude-cli` 不是命令名，
                 直接拿 id 当命令会开出一个「找不到 claude-cli」的空终端。 */
              <div className="flex-1 min-h-0 rounded-lg overflow-hidden border border-white/[0.06]">
                <TermPanel key={engine} cwd={workspace} active={true} initialCmd={tuiCmd} prompts={[{ label: tuiCmd, cmd: tuiCmd }]} onOpenFile={previewFile} onToast={onToast} />
              </div>
            )
          ) : (
          <div ref={dropRef} className={cn("relative flex-1 min-h-0 flex flex-col rounded-lg", dragOver && "ring-2 ring-inset ring-accent/60")}>
          {/* 拖放高亮遮罩（测试报告 #028：「拖拽文件时没有视觉反馈，不知道是否生效」）。
              原来只有一圈 1px 的 ring —— 在深色底上拖着文件根本注意不到，
              客户会以为拖放不支持。跟 AI 作图/视频那两页用同一种明确的遮罩说法。 */}
          {dragOver && (
            <div className="absolute inset-0 z-20 rounded-lg border-2 border-dashed border-accent/60 bg-accent/[0.06] grid place-items-center pointer-events-none">
              <div className="flex items-center gap-2 text-accent text-[13px] font-semibold">
                <Paperclip size={16} /> {t("松手把文件路径插进输入框")}
              </div>
            </div>
          )}
          {/* 正文字号跟着 chatFont 走（测试报告 #007：「字体大小固定」）。
              长文档/长回复在 13px 下读起来很吃力，而这是「阅览」场景的主要用途。
              只放大**正文**，工具卡片那些次要信息保持原尺寸 —— 全放大等于什么都没突出。 */}
          <div className="flex-1 overflow-y-auto space-y-3 py-2 select-text" style={{ fontSize: chatFont }}>
            {items.length === 0 && (
              /* 视觉规格跟 Claude/Codex 那侧的空态**保持一致**（图标徽章 + 17px 问句 + 说明），
                 两个大脑来回切时不该像换了个软件。 */
              <div className="h-full grid place-items-center text-center px-4"><div className="flex flex-col items-center gap-3">
                {expert ? (
                  <>
                    <div className="grid place-items-center w-14 h-14 rounded-2xl bg-accent/[0.10] border border-accent/20 text-[28px]">{expert.emoji}</div>
                    <div>
                      <div className="text-[17px] font-semibold text-ink-0">{t("{name} 已就位", { name: expert.name })}</div>
                      <div className="text-[12.5px] text-ink-2 mt-1 max-w-sm leading-relaxed">{expert.tagline}</div>
                    </div>
                    <div className="text-[11.5px] text-ink-3">{t("下面点一个「试试这样问我」，或直接说你的需求")}</div>
                  </>
                ) : (
                  <>
                    <div className="grid place-items-center w-14 h-14 rounded-2xl bg-accent/[0.10] border border-accent/20">
                      <Bot size={24} className="text-accent" />
                    </div>
                    <div className="min-w-0 max-w-full">
                      {/* 有工作文件夹就点它的名 —— 同 Codex 那句「要在 X 内开发什么」：
                          打字前真正要确认的是这句话会落在哪儿。没选文件夹时不硬凑，照实问。 */}
                      <div className="text-[17px] font-semibold text-ink-0">
                        {workspace ? t("要在「{dir}」里做点什么？", { dir: workspace.split(/[\\/]/).filter(Boolean).pop() || workspace }) : t("有什么可以帮你的？")}
                      </div>
                      {/* ink-4 不是 ink-5 —— 见 ChatPanel 同一处的注释：浅色主题下 ink-5 约 1.4:1，读不出来 */}
                      {workspace && <div className="mt-1 text-[11px] font-mono text-ink-3 truncate" title={workspace}>{workspace}</div>}
                    </div>
                    {/* 🔴 2026-08-16 减字：这里原本堆了 4 段文字（说明 + 按钮 + 又一段说明），
                        客户原话「太多字、复杂、一堆字」。空态是**第一屏**，它的任务只有一件：
                        让人知道现在该往输入框里打字。所以只留一句十个字以内的，
                        和那个「开终端跑 Claude Code」的按钮（那是另一条路的入口，不是解释）。
                        被删掉的两段解释（轻助手和 Claude Code 差在哪）不是错的，只是**不该在这一屏**
                        —— 谁真想知道，顶栏的大脑选择器点开就有。 */}
                    <div className="text-[12.5px] text-ink-2">
                      {workspace ? t("说人话就行，它会自己动手") : t("先选个工作文件夹，它才能读写文件")}
                    </div>
                    <button
                      onClick={() => pasteToTerminal("claude")}
                      className="inline-flex items-center gap-1.5 rounded-lg border border-accent/30 bg-accent/[0.10] px-2.5 py-1.5 text-[11.5px] text-ink-1 hover:bg-accent/[0.18]"
                    >
                      <Terminal size={12} className="text-accent/80" />
                      {t("开终端跑 Claude Code")}
                    </button>
                  </>
                )}
              </div></div>
            )}
            {items.map((it, i) => {
              if (it.type === "approval") {
                const isCmd = it.tool === "run_command";
                const isUking = it.tool === "uking_action";
                return (
                  <div key={i} className="flex gap-2.5 justify-start">
                    <span className="shrink-0 grid place-items-center w-7 h-7 rounded-full bg-amber-500/80 text-white mt-0.5">{isCmd ? <Terminal size={14} /> : isUking ? <ShieldCheck size={14} /> : <FileText size={14} />}</span>
                    <div className="max-w-[80%] min-w-0 rounded-2xl rounded-tl-md bg-amber-500/[0.08] border border-amber-500/30 px-3 py-2.5 text-[12px]">
                      <div className="text-ink-1">{isCmd ? t("AI 想跑命令：") : isUking ? t("AI 想操作 U-King：") : it.tool === "edit_file" ? t("AI 想改文件：") : t("AI 想写文件：")}<span className="font-mono text-ink-0 break-all">{isCmd ? it.command : isUking ? it.action : it.path}</span>{t("，是否允许？")}</div>
                      {isUking && it.inputKeys && it.inputKeys.length > 0 && <div className="mt-1 text-[11px] text-ink-3">{t("入参字段：{keys}", { keys: it.inputKeys.join(", ") })}</div>}
                      {/* 改动内容默认折叠成「±N 行」——批不批的关键信息（哪个文件/哪条命令）在上一行，
                          想细看再点开（#002）。命令审批不折叠：命令本身就是要审的全部内容。 */}
                      {(it.oldStr != null || it.newStr != null) && (
                        <ToolDetail label={t("查看改动（-{o} +{n} 行）", { o: lineCount(it.oldStr), n: lineCount(it.newStr) })}>
                          <div className="mt-1.5 rounded-lg overflow-hidden border border-white/[0.06] bg-bg-0/50"><DiffView path="" oldStr={it.oldStr ?? ""} newStr={it.newStr ?? ""} /></div>
                        </ToolDetail>
                      )}
                      {!it.decided ? (
                        <div className="flex gap-2 mt-2">
                          <button onClick={() => approve(it.id, true)} className="inline-flex items-center gap-1 h-7 px-3 rounded-lg bg-accent text-white text-[12px]"><Check size={12} /> {t("批准")}</button>
                          <button onClick={() => approve(it.id, false)} className="inline-flex items-center gap-1 h-7 px-3 rounded-lg bg-white/[0.06] text-ink-2 text-[12px]"><X size={12} /> {t("拒绝")}</button>
                        </div>
                      ) : (<div className={cn("mt-1.5 text-[11px]", it.decided === "approved" ? "text-success-400" : "text-ink-4")}>{it.decided === "approved" ? t("✅ 已批准") : t("已拒绝")}</div>)}
                    </div>
                  </div>
                );
              }
              if (it.type === "tool") {
                const dataUrl = it.b64 ? `data:image/png;base64,${it.b64}` : it.url;
                const isImg = it.name === "generate_image";
                const isVideo = it.name === "generate_video";
                const isCmd = it.name === "run_command";
                const Icon = isImg ? ImageIcon : isVideo ? Film : isCmd ? Terminal : FileText;
                return (
                  <div key={i} className="flex gap-2.5 justify-start">
                    <span className="shrink-0 grid place-items-center w-7 h-7 rounded-full bg-accent text-white mt-0.5"><Icon size={14} /></span>
                    <div className="max-w-[80%] rounded-2xl rounded-tl-md bg-bg-1/90 border border-white/[0.06] px-3 py-2.5 text-[12px] min-w-0">
                      {it.phase === "running" && (isCmd ? (
                        <div>
                          <div className="flex items-center gap-2 text-ink-2"><Loader2 size={13} className="animate-spin text-accent" /> {t("运行：")}<span className="font-mono break-all">{it.command}</span></div>
                          {it.output && <pre className="mt-1 text-[11px] text-ink-2 bg-bg-0/60 rounded-lg px-2 py-1.5 overflow-x-auto max-h-40 whitespace-pre-wrap">{it.output}</pre>}
                        </div>
                      ) : (<div className="flex items-center gap-2 text-ink-2"><Loader2 size={13} className="animate-spin text-accent" /> {isImg ? t("正在作图：{prompt}", { prompt: it.prompt ?? "" }) : isVideo ? t("正在出片，约 1-3 分钟…") + (it.output ? ` ${it.output}` : "") : t("处理中…")}</div>))}
                      {it.phase === "error" && <div className="text-danger-400 break-all">{t(fileToolLabel[it.name] || it.name)}：{it.message}</div>}
                      {it.phase === "done" && (
                        isImg && dataUrl ? (
                          <div>
                            <div className="text-ink-3 mb-1.5">{t("已生成：")}{it.prompt}</div>
                            <button onClick={() => showPreview({ kind: "image", src: dataUrl, caption: it.prompt })} className="group relative block" title={t("点击在右侧放大预览")}>
                              <img src={dataUrl} alt={it.prompt} className="rounded-lg max-w-[240px] max-h-56 border border-white/[0.06] group-hover:opacity-90" />
                              <span className="absolute inset-0 grid place-items-center opacity-0 group-hover:opacity-100 bg-black/35 rounded-lg text-white text-[12px] gap-1"><Maximize2 size={15} /> {t("点击放大")}</span>
                            </button>
                          </div>
                        )
                        : isVideo && it.path ? (
                          <div>
                            <div className="text-ink-3 mb-1.5">{t("已生成视频：")}{it.prompt}</div>
                            <button onClick={() => showPreview({ kind: "video", src: convertFileSrc(it.path!), caption: it.prompt })} className="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent/15 border border-accent/30 text-accent text-[12px] hover:bg-accent/25" title={t("在右侧预览播放")}>
                              <Film size={13} /> {t("在右侧播放")}
                            </button>
                          </div>
                        )
                        : isCmd ? (
                          <div>
                            <div className="text-ink-3 mb-1 font-mono break-all">$ {it.command}</div>
                            {/* 短输出直接看；长输出折叠成「N 行」摘要（#002：跑几步命令对话就成墙） */}
                            {it.output && (lineCount(it.output) <= 6 && it.output.length <= 400 ? (
                              <pre className="text-[11px] text-ink-2 bg-bg-0/60 rounded-lg px-2 py-1.5 overflow-x-auto max-h-40 whitespace-pre-wrap">{it.output}</pre>
                            ) : (
                              <ToolDetail label={t("查看输出（{n} 行）", { n: lineCount(it.output) })}>
                                <pre className="mt-1 text-[11px] text-ink-2 bg-bg-0/60 rounded-lg px-2 py-1.5 overflow-x-auto max-h-64 whitespace-pre-wrap">{it.output}</pre>
                              </ToolDetail>
                            ))}
                            {/* 跑脚本产出的成品（PPT/Excel/Word…）：认出来就给一条拿到手的路。
                                轻助手做办公活全靠 run_command 调技能包脚本，以前这里只有一段命令输出，
                                客户得自己从日志里把路径抠出来再去文件夹找 —— 而 Claude/Codex 那侧早就有这张卡了。 */}
                            {it.phase === "done" && producedFiles(it.output).length > 0 && (
                              <div className="-mx-3 mt-1.5">
                                {producedFiles(it.output).map((p) => (
                                  <ProducedFile key={p} path={p} onPreview={previewFile} />
                                ))}
                              </div>
                            )}
                          </div>
                        )
                        : (
                          <div className="text-ink-2 min-w-0">
                            <div className="flex items-center gap-2 flex-wrap">
                              <span>{t(fileToolLabel[it.name] || it.name)}：<span className="font-mono text-ink-1 break-all">{it.path}</span> ✓</span>
                            </div>
                            {/* 成品卡片（预览 / 打开 / 在文件夹中显示 + 体积）。以前这儿只有 `.html` 一个
                                「预览网页」按钮：AI 写出 .md/.csv/.svg/.pptx 全都点不开。同一个组件，
                                跟 Claude/Codex 那侧共用，不是第二份实现。 */}
                            {(it.name === "write_file" || it.name === "edit_file") && deliverableExt(it.path ?? "") && (
                              <div className="-mx-3 mt-1.5">
                                <ProducedFile path={it.path!} onPreview={previewFile} />
                              </div>
                            )}
                            {(it.name === "write_file" || it.name === "edit_file") && (it.oldStr != null || it.newStr != null) && (
                              <ToolDetail label={t("查看改动（-{o} +{n} 行）", { o: lineCount(it.oldStr), n: lineCount(it.newStr) })}>
                                <div className="mt-1.5 rounded-lg overflow-hidden border border-white/[0.06] bg-bg-0/50"><DiffView path="" oldStr={it.oldStr ?? ""} newStr={it.newStr ?? ""} /></div>
                              </ToolDetail>
                            )}
                          </div>
                        )
                      )}
                    </div>
                  </div>
                );
              }
              return (
                <div key={i} className={cn("group flex gap-2.5 items-end", it.role === "user" ? "justify-end" : "justify-start")}>
                  {it.role === "assistant" && (<span className="shrink-0 grid place-items-center w-7 h-7 rounded-full bg-accent text-white mt-0.5"><Bot size={14} /></span>)}
                  {/* AI 回复走 markdown 渲染（测试报告 #009）；用户自己敲的字原样回显 ——
                      替他解析星号是擅自改他的话。`whitespace-pre-wrap` 只留给用户那一侧，
                      AI 侧交给 MiniMd 自己排版（它会处理换行/列表/代码块）。 */}
                  {/* 不再写死 text-[13px]：字号由外层容器的 chatFont 决定（#007 可调） */}
                  <div className={cn("select-text max-w-[78%] rounded-2xl px-4 py-2.5 leading-relaxed break-words", it.role === "user" ? "rounded-br-md bg-accent/15 border border-white/[0.10] text-ink-0 whitespace-pre-wrap" : "rounded-tl-md bg-bg-1/90 border border-white/[0.06] text-ink-1")}>
                    {it.content
                      ? (it.role === "assistant" ? <MiniMd text={it.content} onRunInTerminal={pasteToTerminal} /> : it.content)
                      : (busy && i === items.length - 1 ? <Loader2 size={13} className="animate-spin text-accent" /> : "")}
                  </div>
                  {it.content && (
                    <button onClick={() => copyText(it.content)} title={t("复制这段")} className="shrink-0 opacity-0 group-hover:opacity-100 inline-flex items-center justify-center w-6 h-6 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] mb-0.5"><Copy size={12} /></button>
                  )}
                  {it.role === "user" && (<span className="shrink-0 grid place-items-center w-7 h-7 rounded-full bg-white/[0.08] text-ink-2 mt-0.5"><User size={14} /></span>)}
                </div>
              );
            })}
            <div ref={bottomRef} />
          </div>

          <div className="pt-2 mt-1 border-t border-white/[0.06]">
            {/* 「看命令」的诚实对照面：轻助手没有 CLI 可摆。宁可说清楚，也不给它编一条假命令
                —— 客户照着敲一条我们造的命令跑不通，比不给更伤信任。顺带把「换 Claude Code 就能看到」讲了。 */}
            {/* 🔴 这里原来切的是 `claude`（我们代驱动的卡片壳）——可这句话许诺的是
                「对话框底下就是终端」，而卡片壳恰恰**没有**终端。点完发现不是那回事，
                这条提示就从「帮你」变成了「骗你」。现在切到 claude-cli，说什么就给什么。 */}
            {items.length > 0 && (
              <button onClick={() => setEngine("claude-cli")} title={t("切到 Claude Code 终端（原味 TUI）")}
                className="flex items-start gap-1.5 mb-2 text-left text-[10px] text-ink-5 leading-relaxed hover:text-ink-3">
                <Terminal size={10} className="shrink-0 mt-[2px]" />
                <span>{t("轻助手是直连模型 API 跑的，这一轮没有命令行等价物。想要「对话框底下就是终端」——点这里切到 Claude Code 终端。")}</span>
              </button>
            )}
            {/* 快捷调用：专家会话用专家自己的「试试这样问我」（那才是它真会的活）；
                通用会话摆**专家条**（不再是起手词），且只在对话为空时出现 ——
                教学一次就够，聊起来了还占地方是干扰。
                🔴 起手词没删：`/` 指令面板仍从 `ALL_QUICK` 取全部词条，换掉的只是默认铺开的那一块。 */}
            {expert ? (
              <div className="flex items-center gap-1.5 mb-2 flex-wrap">
                {expert.quickPrompts.map(({ label, template }) => (
                  <button key={label} onClick={() => applyQuick(template)} className="inline-flex items-center gap-1 h-7 px-2.5 rounded-full bg-accent/[0.10] border border-accent/30 text-[11px] text-ink-1 hover:bg-accent/[0.16] hover:text-ink-0">
                    {label}
                  </button>
                ))}
              </div>
            ) : null}
            {/* 提示词队列：忙时排队，本轮完自动发下一条 */}
            {queue.length > 0 && (
              <div className="flex items-center gap-1.5 mb-2 flex-wrap">
                <span className="text-[10px] text-ink-5">{t("排队中（本轮完成自动发）:")}</span>
                {queue.map((q, i) => (
                  <span key={i} className="inline-flex items-center gap-1 h-6 px-2 rounded-full bg-amber-500/[0.12] border border-amber-500/30 text-[11px] text-ink-1 max-w-[170px]">
                    <span className="truncate">{q}</span>
                    <button onClick={() => setQueue((qq) => qq.filter((_, j) => j !== i))} className="text-ink-4 hover:text-ink-1 shrink-0"><X size={10} /></button>
                  </span>
                ))}
              </div>
            )}
            {/* Slogan —— 只在对话为空时（照 MiniMax Code / DSH 那张空屏）。
                🔴 **只放一句，不放副标题**：那块地方的作用是让人知道「这是什么、能干什么」，
                多一行就变成又一处要读的字，而这一屏的问题从头到尾都是字太多。
                聊起来了就撤 —— 它是开场白不是常驻标题。 */}
            {items.length === 0 && !busy && (
              <div className={cn("text-center", short ? "mb-3" : "mb-6")}>
                <div className={cn("font-bold text-ink-0 tracking-tight", short ? "text-[22px]" : "text-[30px]")}>
                  {t("U-King")}
                </div>
                <div className={cn("font-bold text-ink-1 tracking-tight mt-0.5", short ? "text-[16px]" : "text-[22px]")}>
                  {t("更多 AI，你来指挥")}
                </div>
              </div>
            )}
            {/* 输入框卡片（WorkBuddy / Codex 式）：左下角能力（+ 附件 · 模型 · 审批档），
                右下角动作（清空 · 发送）。跟 Claude/Codex 那侧是**同一个外壳组件**。
                忙的时候不禁用输入：这边有提示词队列，边跑边写下一条是它的用法。 */}
            <Composer
              value={input}
              onChange={setInput}
              onSend={() => void send()}
              onStop={stop}
              busy={busy}
              textareaRef={inputRef}
              menu={composer.menu}
              onKeyDown={composer.onKeyDown}
              onBlur={composer.onBlur}
              /* 🔴 **一句话，别再往里塞快捷键**（2026-08-18 按 DSH 收）。原来是
                 「让它读写文件、跑命令、画图… @ 引用文件，/ 调指令，Enter 发送」——
                 一个占位符同时教了四件事，而占位符是**打第一个字就消失**的东西：
                 真正需要这些提示的时刻（写到一半想引用文件），它已经不在了。
                 `@` / `/` 照常能用（打出来就弹菜单），拖文件进来也照常；
                 教学交给 `+` 菜单，那个是**随时点得开**的。 */
              placeholder={workspace ? t("告诉 U-King，你想完成什么…") : t("告诉 U-King 你想完成什么，或先选个工作文件夹")}
              left={
                <>
                  <AttachButton
                    onInsert={insertPaths}
                    onMention={() => { setInput((v) => (v && !/\s$/.test(v) ? v + " @" : v + "@")); setTimeout(() => inputRef.current?.focus(), 0); }}
                    onSlash={() => { setInput((v) => (v ? v : "/")); setTimeout(() => inputRef.current?.focus(), 0); }}
                    hasWorkspace={!!workspace}
                    approval={{ value: mode, onChange: setMode, options: MODES }}
                    model={modelPicker}
                    experts={{
                      value: expert?.id ?? "",
                      list: allExperts().map((e) => ({ id: e.id, label: `${e.emoji} ${t(e.name)}` })),
                      onChange: (id) => {
                        if (id === "__hire__") { onFindExpert?.(); return; }
                        const e = allExperts().find((x) => x.id === id);
                        if (e) onSummonExpert?.(e);
                      },
                    }}
                  />
                  {brainSelect}
                  {/* 🔴 审批档**已折进 `+` 菜单**（客户：「权限……最好是自动模式，也不要让客户选择，
                      或者折叠进设置」）。默认档没变，改的只是它常驻不常驻 ——
                      **没有默默改成全自动**：那是产品安全档，静默放宽比多一个下拉危险得多。
                      它仍然一点就到，只是不再占着工具条。 */}
                </>
              }
              right={
                <button onClick={() => setItems([])} disabled={items.length === 0} title={t("清空对话（这个会话从头开始）")}
                  className="inline-flex items-center justify-center w-7 h-7 rounded-lg text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] disabled:opacity-30">
                  <RotateCcw size={14} />
                </button>
              }
              hint={priceyModelHint(model) ? (
                <div className="mt-1.5 rounded-lg border border-danger-500/40 bg-danger-500/[0.10] px-2.5 py-1.5 text-[11px] leading-snug font-medium text-danger-700 dark:text-danger-400">{t(priceyModelHint(model)!)}</div>
              ) : null}
            />
            {/* 起手词在**输入框下面**（2026-08-18 按 MiniMax Code 的排法）。
                🔴 放上面时它挤在 slogan 和输入框中间，把「这是什么」和「在这儿打字」隔开了；
                放下面则是「先看到能打字，再看到可以打什么」—— 顺序对得上人的动作。
                只在对话为空时出现：它是教学，不是常驻工具条。 */}
            {items.length === 0 && !expert && (
              <QuickPrompts onPick={applyQuick} onFindExpert={onFindExpert} className="mt-2" />
            )}
          </div>
          </div>
          )}

          {/* 🔴 输入框**外面那一行已整行删除**（2026-08-18，客户：「下面的文件夹功能删除…
              通用助手删除，放到 + 里边…Claude Code 和 DeepSeek 模型选择合并在一起」）。
              它曾是「文件夹 / 打开方式 / 专家 / 大脑」四个胶囊，上一轮收成三格等宽 ——
              但**对齐只解决了「难看」，没解决「为什么要有」**：
                · 文件夹   —— 左栏「新建项目（选文件夹）」是入口、标题下面又印着当前路径，这是第三处
                · 专家     —— 低频，选一次管很久，不值得常驻
                · 大脑+模型 —— 本来就是一件事（「用哪个脑子的哪个模型」），
                              拆成两个下拉是把我们的实现细节漏给了用户
              现在：换文件夹 / 换专家进 `+`，大脑和模型合成**一个**选择器留在框内。
              照 MiniMax Code 的形状：框内只剩「+ · 脑子 · 发送」，**框外零控件**。 */}
        </div>
      </div>

      {/* 拖条（仅面板展开且对话列未收起时） */}
      {rightOpen && !chatCollapsed && <div onMouseDown={startDrag} className="w-1.5 shrink-0 cursor-col-resize bg-white/[0.06] hover:bg-accent/40 rounded-full mx-0.5" />}

      {/* 右：面板区（挂载过就一直在，靠 display 切换保 PTY / 浏览历史） */}
      {opened.size > 0 && (
        <div ref={panelRef} className="flex flex-col min-h-0 h-full bg-bg-1/40 rounded-xl border border-white/[0.06] overflow-hidden" style={{ width: rightOpen ? (chatCollapsed ? "100%" : `${(1 - ratio) * 100}%`) : 0, display: rightOpen ? "flex" : "none" }}>
          <div className="flex items-center gap-1 h-9 px-2 border-b border-white/[0.06] bg-bg-1 shrink-0">
            {/* 对话列收起时：面板头最左给一个「展开对话」按钮，随时回到聊天 */}
            {chatCollapsed && (
              <button onClick={() => setChatCollapsed(false)} title={t("展开对话列")}
                className="inline-flex items-center justify-center w-6 h-6 rounded text-ink-3 hover:text-ink-0 hover:bg-white/[0.06] mr-0.5 shrink-0">
                <PanelLeftOpen size={14} />
              </button>
            )}
            {/* 🔴 终端态下不显示「文件」tab —— 它旁边就是文件栏开关，两颗同名按钮点了行为还不一样
                （一个开旁栏、一个把整个面板切走、终端消失）。同一时刻只该有一个「文件」。 */}
            {RIGHT_META.filter((m) => (opened.has(m.kind) || m.kind === rightKind) && !(rightKind === "terminal" && m.kind === "files")).map(({ kind, label, icon: Icon, needsWs }) => (
              <button key={kind} onClick={() => togglePanel(kind)} disabled={needsWs && !workspace}
                className={cn("inline-flex items-center gap-1 h-6 px-2 rounded text-[12px]", rightKind === kind ? "bg-accent/[0.14] text-ink-0" : "text-ink-3 hover:bg-white/[0.05]")}>
                <Icon size={13} /> {t(label)}
              </button>
            ))}
            {/* 文件栏开关 —— **必须在这条**：它控制的是终端旁边那栏，而终端就在这个面板里。
                原来它在对话列顶栏，收起对话列（看终端全屏）时开关跟着消失，文件栏却还开着
                = 打开了没地方关（2026-08-18 客户实拍）。**开关跟着它控制的东西走，
                不跟着某个恰好也在的容器走。** */}
            {/* 拉出成独立窗口（客户 2026-08-18：「终端能拉出来不？做对比之类的」）。
                同进程第二个 webview，只挂终端不挂整个 App（见 `main.tsx` 的入口分流）。
                同一目录再点 = 把已开那个顶到前面，不会开出一堆一模一样、各带 PTY 的窗口。 */}
            {rightKind === "terminal" && (
              <button
                onClick={() => void invoke("open_terminal_window", { cwd: workspace || null, cmd: null })
                  .catch((e) => onToast?.(t("拉出终端失败：{e}", { e: String(e) })))}
                title={t("把终端拉成独立窗口（可以和工作台并排看）")}
                className="inline-flex items-center gap-1 h-6 px-2 rounded text-[12px] ml-1 text-ink-3 hover:bg-white/[0.05]">
                <Maximize2 size={13} /> {t("拉出")}
              </button>
            )}
            {rightKind === "terminal" && workspace && (
              <button
                onClick={() => setTermFilesOpen((v) => !v)}
                title={termFilesOpen ? t("关掉右边的文件栏") : t("在终端右边开一栏：文件树 + 预览")}
                className={cn("inline-flex items-center gap-1 h-6 px-2 rounded text-[12px] ml-1",
                  termFilesOpen ? "bg-accent/[0.14] text-ink-0" : "text-ink-3 hover:bg-white/[0.05]")}>
                <FolderTree size={13} /> {t("文件")}
              </button>
            )}
            <button onClick={() => setRightOpen(false)} title={t("收起面板")} className="ml-auto inline-flex items-center justify-center w-6 h-6 rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.08]"><X size={14} /></button>
          </div>
          <div className="flex-1 min-h-0 relative">
            {opened.has("terminal") && workspace && (
              /* 终端 tab = 终端列 + 可开关的文件栏（树 + 就地预览）。文件栏用的是**同一个
                 FilesPanel**，只是树窄一点。终端始终在场：点开文件不会把它顶掉。 */
              <div ref={termAreaRef} className="absolute inset-0 flex" style={{ display: rightKind === "terminal" ? "flex" : "none" }}>
                <div ref={termColRef} className="flex-1 min-w-0">
                  <TermPanel cwd={workspace} active={rightOpen && rightKind === "terminal"} onReady={handleTermReady}
                    onOpenFile={openFromTerminal} onToast={onToast} />
                </div>
                {termFilesOpen && (
                  <>
                    <div onMouseDown={startTermFilesDrag} className="w-1.5 shrink-0 cursor-col-resize bg-white/[0.06] hover:bg-accent/40" />
                    {/* 🔴 宽度必须在**渲染时**夹一次，不能只在拖条里夹。
                        原来只有 startTermFilesDrag 里有 `rect.width - 240` 的保底，初次打开时
                        直接吃 termFilesWidth(480) —— 实测 1280 宽的机器上终端只剩 34px、
                        1440 上剩 101px（scripts/shot-workspace-ui.mjs 拍出来的）。
                        TUI 按列排版，那个宽度等于废掉。用 CSS 夹：终端保底 240px。 */}
                    <div
                      style={{ width: `max(240px, min(${termFilesWidth}px, calc(100% - 240px)))` }}
                      className="shrink-0 min-w-0 border-l border-white/[0.06]"
                    >
                      <FilesPanel root={workspace} active={rightOpen && rightKind === "terminal"}
                        treeWidth={200} activePath={termFilePath} onActivePathChange={setTermFilePath} onToast={onToast} />
                    </div>
                  </>
                )}
              </div>
            )}
            {opened.has("files") && workspace && (
              <div className="absolute inset-0" style={{ display: rightKind === "files" ? "block" : "none" }}>
                <FilesPanel root={workspace} active={rightOpen && rightKind === "files"} onToast={onToast} />
              </div>
            )}
            {opened.has("preview") && (
              <div className="absolute inset-0 flex flex-col" style={{ display: rightKind === "preview" ? "flex" : "none" }}>
                {!preview ? (
                  <div className="flex-1 grid place-items-center text-center px-4">
                    <div>
                      <Eye size={30} className="text-ink-5 mx-auto mb-2" />
                      <div className="text-[13px] text-ink-3">{t("图片 / 视频 / 网页 / PPT · Word · Excel · PDF 都在这里预览")}</div>
                      <div className="text-[11px] text-ink-5 mt-1">{t("让它「画一张…」「做个 PPT…」「整理成表格…」，成果就出现在这里")}</div>
                    </div>
                  </div>
                ) : preview.kind === "image" ? (
                  <>
                    <div className="flex items-center gap-1.5 h-9 px-2 border-b border-white/[0.06] shrink-0">
                      <span className="text-[12px] text-ink-3 truncate flex-1">{preview.caption || t("预览")}</span>
                      <button onClick={() => setZoom((z) => Math.max(0.25, z - 0.25))} title={t("缩小")} className="w-6 h-6 grid place-items-center rounded text-ink-3 hover:bg-white/[0.06]"><ZoomOut size={14} /></button>
                      <span className="text-[11px] text-ink-4 w-10 text-center">{Math.round(zoom * 100)}%</span>
                      <button onClick={() => setZoom((z) => Math.min(4, z + 0.25))} title={t("放大")} className="w-6 h-6 grid place-items-center rounded text-ink-3 hover:bg-white/[0.06]"><ZoomIn size={14} /></button>
                      <button onClick={() => setZoom(1)} title={t("还原")} className="w-6 h-6 grid place-items-center rounded text-ink-3 hover:bg-white/[0.06]"><Maximize2 size={13} /></button>
                    </div>
                    <div className="flex-1 min-h-0 overflow-auto grid place-items-center bg-bg-0/40 p-3">
                      <img src={preview.src} alt={preview.caption || ""} style={{ transform: `scale(${zoom})`, transformOrigin: "center" }} className="max-w-full max-h-full object-contain rounded transition-transform" />
                    </div>
                  </>
                ) : preview.kind === "video" ? (
                  <>
                    <div className="flex items-center gap-1.5 h-9 px-2 border-b border-white/[0.06] shrink-0">
                      <Film size={13} className="text-ink-4" /><span className="text-[12px] text-ink-3 truncate flex-1">{preview.caption || t("视频预览")}</span>
                    </div>
                    <div className="flex-1 min-h-0 overflow-auto grid place-items-center bg-bg-0/40 p-3">
                      <video src={preview.src} controls autoPlay loop className="max-w-full max-h-full rounded" />
                    </div>
                  </>
                ) : preview.kind === "doc" ? (
                  /* 办公文档 / PDF / 压缩包 …：整块交给 redline 内核（它自带标题栏、格式识别、
                     懒加载 viewer、渲染失败兜底「用默认程序打开」）。**和文件树双击是同一个组件**，
                     不是第二份实现；key=path 换文件即重挂。 */
                  <RedlinePanel
                    key={preview.path}
                    host={redlineHost}
                    path={preview.path}
                    fileName={preview.path.split(/[\\/]/).pop() ?? preview.path}
                  />
                ) : (
                  <>
                    <div className="flex items-center gap-1.5 h-9 px-2 border-b border-white/[0.06] shrink-0">
                      <Globe size={13} className="text-ink-4" /><span className="text-[12px] text-ink-3 truncate flex-1">{preview.caption || t("网页预览")}</span>
                      {/* iframe 里的页面点链接不跳、有些脚本被 sandbox 拦 —— 交给系统浏览器打开，
                          能点链接、能登录，比内置浏览器更实用（也不再需要维护一整套浏览器面板）。
                          只对有真实路径的网页给；整段源码塞进来的那种（read_text_file 那条）没有文件可开。 */}
                      {preview.path && (
                        <button
                          onClick={() => {
                            // 本地文件交给系统默认程序（.html = 系统浏览器）。不能走 browser_nav
                            // external：它只放行 https/localhost，file:/// 会被校验拦下。
                            invoke("open_produced_file", { path: preview.path }).catch((e) =>
                              onToast?.(t("打开系统浏览器失败：{err}", { err: String(e) })),
                            );
                          }}
                          title={t("用系统浏览器打开（可点链接、可登录）")}
                          className="shrink-0 inline-flex items-center gap-1 h-6 px-2 rounded-md border border-white/[0.10] text-[11px] text-ink-3 hover:text-ink-0 hover:border-accent/40"
                        >
                          <Globe size={11} /> {t("用浏览器打开")}
                        </button>
                      )}
                    </div>
                    {/* src（真文件地址，相对资源能加载）优先；没有才退回整段源码。
                        sandbox 两条都留着 allow-scripts —— 生成的网页多半有交互，不给脚本等于给个死页面。 */}
                    {preview.src ? (
                      <iframe title="preview" src={preview.src} sandbox="allow-scripts allow-same-origin" className="flex-1 min-h-0 w-full bg-white" />
                    ) : (
                      <iframe title="preview" srcDoc={preview.html} sandbox="allow-scripts" className="flex-1 min-h-0 w-full bg-white" />
                    )}
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
