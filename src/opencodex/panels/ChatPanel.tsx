/**
 * 任务对话面板 —— Codex 式结构化对话。Claude Code 走 stream-json，渲染成卡片而非裸终端。
 *
 * 后端 agent/claude.rs 发统一事件（kind: session/text/text_done/tool_start/tool_input/tool_end/usage/done），
 * 经 claude_send 的 Channel 流回。前端据此拼消息流：用户气泡 + assistant 文本 + 工具卡片 + 内联 diff + 用量。
 *
 * 会话历史存组件内 state；面板靠 PanelArea 的 display 切换保活（切任务不丢历史）。
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke, convertFileSrc, Channel } from "@tauri-apps/api/core";
import { FileEdit, Terminal as TermIcon, Eye, Globe, Wrench, RotateCcw, AlertTriangle, Copy, Check, X, ChevronDown, ChevronRight, Info, Play, Loader2, FolderOpen, Cpu } from "lucide-react";
import { DiffView } from "./DiffView";
import { copyToClipboard } from "../../lib/clipboard";
import { useDropZone, pathsToText } from "../../lib/fileDrop";
import { describeImages, fileLabel, isImageFile } from "../../lib/vision";
import { MiniMd } from "../../lib/miniMd";
import { QuickPrompts, type Best } from "../QuickPrompts";
import { useComposerMenu } from "../ComposerMenu";
import { AttachButton, Composer, ComposerSelect } from "../Composer";
import { XIAPAN_MODELS, priceyModelHint } from "../../lib/models";
import { AnchoredMenu } from "../../components/AnchoredMenu";
// 「哪些扩展名 redline 渲染得了」的唯一真相源，别在本文件再抄一份
import { REDLINE_EXTS } from "../../vendor/redline-core";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/cn";
import { useViewport } from "../../lib/useViewport";

/* ---- 输入框工具条上的「模型」和「权限」---------------------------------------
 * 两样都必须说这台机器上**真发生的事**，不能照着别家 UI 摆好看的：
 *
 * 模型：`claude_send` / `codex_send` 本来就收 `model: Option<String>`（agent/claude.rs 转
 *   `--model`、agent/codex.rs 转 `-m`），以前前端一直传 null —— 能力在，只是没有入口。
 *   空串 = 不传 = 跟着驱动配置走（虾盘云 preset 的 deepseek-v4-flash）。
 *
 *   Claude 侧直接复用 `lib/models.ts` 那份清单：Manager 的「换模型」写的就是 ANTHROPIC_MODEL，
 *   跟 `--model` 是同一个口径，别在这儿再抄一份（宪法第 8 条）。
 *   Codex 侧**不能**用那份：它走 /v1/responses，只有挂了 type=1 直连渠道的 `-codex` 名调得通，
 *   国产裸名会 500 convert_request_failed（见 components/ProviderSwitch.tsx 那段实测注释）。
 *   所以这里只收两个有实测背书的。
 *
 * 权限：Claude Code 在工作台里是 `--permission-mode bypassPermissions` 写死的
 *   （agent/claude.rs），**做成下拉就是骗** —— 只读展示，并说清楚想逐条确认该去哪。
 *   Codex 那侧我们一个 sandbox/approval flag 都没传，事实就是「跟它自己的默认走」，照实说。
 * -------------------------------------------------------------------------- */
const CODEX_MODELS: { id: string; label: string }[] = [
  { id: "deepseek-v4-flash-codex", label: "DeepSeek V4 Flash · 最快最省（默认）" },
  { id: "gpt-5.3-codex", label: "GPT-5.3 Codex · 更强（约 6 倍价）" },
];
const MODEL_KEY = "uking.chatpanel.model.";

// 对话历史持久化：存 localStorage（跨刷新/重启不丢，治「刷新后对话全清空」）。key 用 taskId
// （= sessionId-engine，每会话每引擎一份）。工具输出可能很大，存档时截断防撑爆配额。
const CHAT_STORE_PREFIX = "uking.chatpanel.";
function loadChatItems(key: string): Item[] {
  try {
    const raw = localStorage.getItem(CHAT_STORE_PREFIX + key);
    const arr = raw ? JSON.parse(raw) : [];
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}
function saveChatItems(key: string, items: Item[]) {
  try {
    const trimmed = items.map((it) =>
      it.kind === "tool" && it.output && it.output.length > 4000
        ? { ...it, output: it.output.slice(0, 4000) + "\n…（历史已截断）" }
        : it,
    );
    localStorage.setItem(CHAT_STORE_PREFIX + key, JSON.stringify(trimmed));
  } catch {
    /* 配额满/隐私模式：放弃持久化，不影响使用 */
  }
}

/** 对话前置体检：claude 装没装 + 驱动配没配。 */
type ReadyState = {
  claudeFound: boolean;
  hasDriver: boolean;
  /** 这一轮**记在谁账上**：true = 客户自己的官方登录 / 自己的 Key，我们一个 env 都没注入。
   *  见 `providers.rs::delegation_env` —— 客户自己配过就绝不覆盖（产品红线：不许抢登录态）。
   *  只在报错时用得上，但正因为平时不显示，出事时它就是唯一能分清「谁家的 403」的依据。 */
  ownAccount: boolean;
} | null;

/** 「多久没新动静就该提醒一句」。后端的硬死线是 300s（agent/claude.rs::STALL_SECS），
 *  这里 90s 就先开口 —— 提醒在前、动手在后，别让人先干等五分钟再看到一句「已自动停止」。 */
const IDLE_HINT_SECS = 90;

/** 卡住的时候**它到底停在哪一步**。跟后端 `agent/mod.rs::TurnLog` 的阶段一一对应，
 *  但这边不需要后端多发事件 —— 事件流本身就够推：tool_start→工具、tool_end→等模型、text→流中。
 *
 *  为什么非要分：原先这行写死「多半卡在一条很慢的命令上」。pc-***（2026-08-03）那次静默
 *  21 分钟，**一条命令都没在跑**，它停在「等模型回话」上 —— 客户照着这句去查自己的电脑，
 *  查一整天也查不到。替人做判断还把判断说错，比不说更伤。 */
type Stage = "startup" | "tool" | "await" | "stream";

/** 每个阶段一句**只陈述事实**的话（都是从事件流直接读出来的，不含推测），外加下一步怎么办。 */
function stageHint(
  stage: Stage,
  tool: string | null,
  t: (s: string, v?: Record<string, string | number>) => string,
): string {
  switch (stage) {
    case "tool":
      return tool ? t("还在跑「{n}」这一步", { n: tool }) : t("还在跑一条命令");
    case "await":
      return t("上一步已经做完了，正在等 AI 回话");
    case "stream":
      return t("AI 回了一半停住了");
    default:
      return t("还没开始跑，卡在启动这一步");
  }
}

/** 秒 → 人话时长。**故意不显示毫秒/小数**：这行字是给等着的人看的，不是性能报表。 */
function fmtDur(sec: number): string {
  if (sec < 60) return `${sec} 秒`;
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return s ? `${m} 分 ${s} 秒` : `${m} 分钟`;
}

/** 「看命令」：后端 agent/cmdline.rs 每轮发来的真实命令 + 终端交互式等价写法。 */
type CmdInfo = { display: string; teach: string; program: string; prompt_inlined: boolean };

type ToolItem = {
  kind: "tool";
  id: string;
  name: string;
  input?: unknown;
  output?: string;
  isError?: boolean;
  done: boolean;
};
type TextItem = { kind: "text"; role: "user" | "assistant"; text: string };
/** 这一轮的账。`cny` 是**我们自己按水电表那份价表算的**，不是上游 CLI 报的 $ ——
 *  上游拿它认得的官方价算 deepseek，出来的数跟真实扣费无关（详见 lib.rs::chat_cost_cny）。 */
type UsageItem = { kind: "usage"; inTok: number; outTok: number; cacheRead: number; cacheWrite: number; costUsd: number; cny: number; ms: number };
type Item = TextItem | ToolItem | UsageItem;

/**
 * 工具名 → 人话。小白在对话里看到的一屏 `Bash` / `Glob` / `Grep` 是天书，
 * 而这些卡片**每一轮都出现** —— 翻译它的收益比任何一处文案都高。
 *
 * **认不出的原样返回**：MCP 工具、上游新加的工具名，瞎翻会把人带偏 ——
 * 一个看不懂的英文名比一个编出来的中文名诚实。
 */
const TOOL_LABELS: Record<string, string> = {
  Bash: "运行命令",
  BashOutput: "看后台输出",
  KillShell: "停掉后台命令",
  Read: "查看文件",
  Write: "新建文件",
  Edit: "修改文件",
  NotebookEdit: "改笔记本",
  Glob: "查找文件",
  Grep: "搜索内容",
  WebFetch: "打开网页",
  WebSearch: "上网搜索",
  Task: "派了个小助手",
  TodoWrite: "列执行计划",
  ExitPlanMode: "确认方案",
  SlashCommand: "执行指令",
  Wrench: "调用外部工具", // codex 的 mcp_tool_call 映射过来的名字
};

/**
 * 报错人话化 —— 返回 `[人话, 该怎么办]`，认不出就返回 `null` 让调用方走通用引导。
 *
 * 以前不管什么错都硬贴一句「多半是没接驱动 / 没装」。那是**一句写死的猜测**：
 * 真因是余额用完时，这句话会把客户支去重装驱动，越搞越远。
 * 认得出就给准话，认不出就说「认不出」——不许拿一个猜测冒充诊断。
 */
/** 人话化的结果：`[是什么, 怎么办, 文案里 {占位符} 的取值]`。
 *
 * 第三项是后加的：带数字的那句必须是**模板 + 参数**，不能把数字拼进字符串再喂给 `t()` ——
 * 拼出来的串在词典里永远匹配不到，等于这句话对英文用户直接消失。而漏翻闸门也照不到它：
 * 它只认参数是字符串字面量的那种调用，拼出来的串对它是隐形的，所以会一路报绿。 */
export type Humanized = [string, string, Record<string, string | number>?];

export function humanizeError(raw: string, ownAccount = false): Humanized | null {
  const s = (raw || "").toLowerCase();
  const has = (...keys: string[]) => keys.some((k) => s.includes(k));

  /* 🔴 「余额还有钱，却一条都发不出去」—— 必须排在 403 前面。
   *
   * 上游发请求前要**预扣**一笔（按模型价 × max_tokens 估的上限）。Claude Code 上下文大，
   * 单次预扣可能要 ¥0.36；余额剩 ¥0.34 就会被挡在门外，报的是 403：
   *
   *   403 token quota is not enough, token remain quota: ¥0.340274, need quota: ¥0.358240
   *
   * 它带着 403，所以会被下面那条 403 规则抢走，然后给出「换模型 / 渠道下架 / IP 被拦」——
   * 对这一种一条都不适用，客户换完模型照样发不出去。
   *
   * 这就是 2026-08-18「老用户不能用了」的真因：用久了余额烧到低位就撞这道门槛，
   * 而界面上明明还显示有钱。新用户余额高，永远碰不到 —— 所以看起来像「只有老用户坏了」。
   *
   * 把两个数字原样念给客户听，比任何解释都短。 */
  if (has("quota is not enough", "need quota")) {
    const remain = /remain quota[^0-9¥$]*[¥$]?\s*([0-9.]+)/i.exec(raw)?.[1];
    const need = /need quota[^0-9¥$]*[¥$]?\s*([0-9.]+)/i.exec(raw)?.[1];
    const what = "余额不够发起这一次请求（不是没钱，是不够垫这一次）";
    // 两句分开：取到数字就用带数字那句（最短的解释），取不到就退到通用句。
    // 不把数字拼进字符串 —— 拼出来的串在词典里匹配不到（见 Humanized 的注释）。
    return remain && need
      ? [
          what,
          "你的余额还剩 ¥{remain}，而这一次要预留 ¥{need}。发请求前上游要先按「最多可能用掉多少」冻结一笔，所以余额见底时会一条都发不出去。去「虾盘云」页充值，充 ¥1 就能解开；充完这条消息重发一次即可。",
          { remain: Number(remain).toFixed(2), need: Number(need).toFixed(2) },
        ]
      : [
          what,
          "发请求前上游要先按「最多可能用掉多少」冻结一笔，所以余额见底时会一条都发不出去。去「虾盘云」页充值，充 ¥1 就能解开；充完这条消息重发一次即可。",
        ];
  }

  if (has("credit balance", "insufficient_quota", "insufficient balance", "余额不足", "quota exceeded"))
    return ["账户余额用完了", "去「虾盘云」页充值，充完这条消息重发一次就行。"];
  if (has("invalid api key", "authentication_error", "invalid_api_key", "unauthorized", "401"))
    return ["密钥没对上", "多半是驱动没配好或配到了别家。去「AI 设置」重新一键配虾盘云。"];
  /* 403 以前**无人认领** —— 掉进最后那句「多半是没装好/驱动没配对，点『去配驱动』」，
   * 而那正是上面注释明令禁止的写死猜测：客户会被支去重装一遍驱动，一无所获。
   * 2026-08-16 客户就是这么报上来的「u-chat 里几个 403」。
   *
   * 🔴 **403 和 401 不是一回事**：401 是「你是谁没对上」（重配驱动确实管用），
   * 403 是「你是谁没问题，但这次不许你用」—— 模型/渠道没权限、上游中转商欠费下架、
   * 地区或出口 IP 被拦、风控。重配驱动对这几种一样都治不了。
   *
   * 🔴 **而且必须先说清「这一轮记在谁账上」**：客户自己配过 Claude Code / Codex 时
   * 我们一个 env 都不注入（`providers.rs::delegation_env`，产品红线），那条 403 来自**他自己那家**，
   * 跟虾盘云无关。不讲这句，他只会认定是我们坏了 —— 我自己排查这条时也先怀疑了虾盘云，
   * 全量探完 17 个模型 + /v1/messages + /v1/responses 才发现那台机器压根没走我们这儿。 */
  if (has("403", "forbidden", "permission_error", "request not allowed", "无权"))
    return ownAccount
      ? [
          "上游回了 403：认得出你是谁，但这一次不让用",
          "**这一轮走的是你自己的账号**（你在「AI 设置」里给它配过官方登录 / 自己的 Key，我们不会覆盖），所以这条 403 来自那一家、不是虾盘云。先去那家看看账号状态：模型有没有权限、有没有欠费或被限地区（挂代理的话换个出口再试）。想改走虾盘云就去「AI 设置」切一下。",
        ]
      : [
          "上游回了 403：认得出你是谁，但这一次不让用",
          "常见三种：这个模型你的档位用不了、上游渠道临时下架、或者出口 IP 被拦。先在顶栏「换模型」换成 DeepSeek Flash 重发一次；换了还是 403 就用「技术支持」把下面这段原文发给我们（里面有 request id，我们能直接查到是哪条渠道）。",
        ];
  if (has("rate limit", "429", "too many requests"))
    return ["请求太频繁，被上游限流了", "等一两分钟再发。反复出现就换个模型试试。"];
  if (has("enotfound", "econnrefused", "etimedout", "connection error", "network", "getaddrinfo", "timed out"))
    return ["连不上服务器", "先看看这台电脑能不能上网；开了代理/VPN 的话先关掉再试。"];
  if (has("model not found", "does not exist", "unknown model", "no such model"))
    return ["这个模型现在用不了", "在顶栏「换模型」里换一个（推荐 DeepSeek Flash），或去「AI 设置」重配驱动。"];
  if (has("command not found", "不是内部或外部命令", "enoent", "is not recognized"))
    return ["找不到这个程序", "多半是它没装好。去「装 AI」页重装一次，装完会自动验证。"];
  if (has("context", "too long", "maximum context", "token limit"))
    return ["这一轮聊太长了，超出了模型能记住的上限", "点输入框旁边的「清空对话」开个新会话，把关键信息重说一遍。"];
  return null;
}

// 工具图标
function toolIcon(name: string) {
  if (name === "Edit" || name === "Write" || name === "NotebookEdit") return FileEdit;
  if (name === "Bash") return TermIcon;
  if (name === "Read" || name === "Glob" || name === "Grep") return Eye;
  if (name === "WebFetch" || name === "WebSearch") return Globe;
  return Wrench;
}

export function ChatPanel({
  taskId,
  cwd,
  active,
  onGoManage,
  agent = "claude",
  system,
  onRunInTerminal,
  onStatus,
  onQuickPick,
  brainSlot,
  composerFooter,
  modelPicker,
  experts,
  onPreview,
  seedPrompt,
  onSeedSent,
}: {
  taskId: string;
  cwd: string;
  active: boolean;
  onGoManage?: () => void;
  /** 对话大脑 CLI：claude（agent/claude.rs）或 codex（agent/codex.rs）。命令走 `${agent}_send` 等。 */
  agent?: "claude" | "codex";
  /** 专家 persona（系统提示）：claude 走 --append-system-prompt，codex 首轮 prepend。 */
  system?: string;
  /** 会话显示名（用户自定义/工具名）—— 空态标题用它，别再写死「和 Claude Code 对话」。 */
  title?: string;
  /** 「看命令」的「在终端跑」：把命令贴进宿主的终端面板（**不回车**，让用户自己确认再跑）。
   *  不传则只显示「复制」—— 组件仍自包含可用（宪法：模块只靠 props 通信）。 */
  onRunInTerminal?: (cmd: string) => void;
  /** 这一轮跑起来了 / 跑完了 / 跑挂了 —— 交给宿主去染左侧列表那个小圆点。
   *  组件自己不认识 store（宪法：模块只靠 props 通信），不传也照常能用。 */
  onStatus?: (s: "running" | "idle" | "error") => void;
  /** 起手词点到了**别的大脑更拿手**的活（如作图/出片）时交回宿主 —— 本组件没有 engine 状态，
   *  切大脑是宿主的事。不传就退化成「填进自己的输入框」，组件仍自包含可用。 */
  onQuickPick?: (template: string, best: Best) => void;
  /** 宿主建好的「大脑 + 模型」合并选择器。不传则退回本地的模型下拉。 */
  brainSlot?: ReactNode;
  /** 输入框正下方那条轻行（工作文件夹）—— 由宿主建好传进来，两个分支一份实现。 */
  composerFooter?: ReactNode;
  /** 模型覆盖，收在 `+` 里（19 项，放不进工具条）。 */
  modelPicker?: { value: string; allowFollow: boolean; list: { id: string; label: string }[]; onChange: (m: string) => void };
  /* `onOpenWith` / `workspace` 已删（2026-08-18）：「在这个文件夹里打开」那组从 `+` 里去掉了
     —— 右侧文件预览面板本来就有这个功能，同一个动作两个入口，改一个漏一个。 */
  experts?: { value: string; list: { id: string; label: string }[]; onChange: (id: string) => void };
  /* 🔴 `onFindExpert` / `onSummonExpert` **已删**（2026-08-18 随专家 chips 一起）：
     选专家/找专家现在统一在输入框底下那条的「专家」下拉里（Chat.tsx），两个分支共用一份。
     组件里留着两个没人调的 prop = 下一个人会以为这儿还有个专家入口。 */
  /** 把一个产出文件送进宿主的右侧预览（网页/图片/视频）。
   *  本组件不拥有预览面板（那是 Chat 的），所以只发路径不自己渲染。不传则不显示预览按钮 ——
   *  **宁可没这个按钮，也不给一个点了没反应的**。 */
  onPreview?: (path: string) => void;
  /** 宿主投进来的第一轮（护照交接）。发一次就够 —— 见下方 `seedSent`。不传＝没人交接过。 */
  seedPrompt?: string | null;
  /** 上面那一轮真发出去了。宿主的「已送达」由它签字。 */
  onSeedSent?: () => void;
}) {
  const { t } = useI18n();
  /** 矮屏（1366×768 客户区 ~688px）：这一栏的固定占用（顶部留白 + 空态留白 + 起手词 + 输入框）
   *  加起来能吃掉一半高度，客户看到的就是「一边要滚一边大片空白」。见 lib/useViewport.ts。 */
  const { short } = useViewport();
  /** 这个面板驱动的是哪个大脑 —— 一切对客户说话的地方都用它，别再写死「Claude」
   *  （codex 引擎下显示「Claude 正在干活」是明确的错话，小白会照着它去排错 Claude）。 */
  const agentName = agent === "codex" ? "Codex" : "Claude";
  const [items, setItems] = useState<Item[]>(() => loadChatItems(taskId));
  // 每次消息变化落盘（跨刷新/重启保活）
  useEffect(() => {
    saveChatItems(taskId, items);
  }, [taskId, items]);
  // 单条移除（报错/无用消息可单独关，不必清空整段——治「报错只能靠刷新/清空消除」）
  const removeItem = useCallback((i: number) => setItems((prev) => prev.filter((_, j) => j !== i)), []);
  const [input, setInput] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  // 模型覆盖（空 = 跟着驱动配置走）。按大脑各记各的：claude 和 codex 认的模型名根本不是一套。
  /** 本会话累计花费（¥，我们自己的口径）。客户原话「10 元很快就用完了」——
   *  他缺的不是省钱手段，是**知道钱花在哪一轮**。 */
  const [spentCny, setSpentCny] = useState(0);
  const [model, setModel] = useState(() => {
    try {
      return localStorage.getItem(MODEL_KEY + agent) ?? "";
    } catch {
      return "";
    }
  });
  const pickModel = useCallback(
    (m: string) => {
      setModel(m);
      try {
        localStorage.setItem(MODEL_KEY + agent, m);
      } catch {
        /* 隐私模式/配额满：不持久化也照常能用 */
      }
    },
    [agent],
  );

  /* ── Codex 的模型清单必须跟着**当前供应商**走（2026-08-11，客户机 pc-*** 实锤）────────
   *
   * `CODEX_MODELS` 里那两个名字（`deepseek-v4-flash-codex` / `gpt-5.3-codex`）**只有虾盘云认**：
   * 前者是服务端专门建的 type=1 直连渠道。DeepSeek 官方只认裸名，给它 `-codex` 后缀直接 400：
   *   "The supported API model names are deepseek-v4-pro or deepseek-v4-flash,
   *    but you passed deepseek-v4-flash-codex."
   *
   * 🔴 而这个选择是**按大脑存 localStorage 的，跟供应商无关** —— 客户在虾盘云时选过一次
   * （标签还写着「（默认）」，很招手），之后把 Codex 切到 DeepSeek 官方，这条陈旧覆盖留了下来。
   * 它以 `-m` 传下去，**盖过 config.toml 里我们写对的那个模型**，于是每轮都 400。
   * 界面上更坑：下拉显示的是「DeepSeek V4 Flash · 最快最省」，看着完全正确。
   *
   * 所以：不是虾盘云就不提供这份清单（我们没有别家可用模型的实测背书，宁可不给也不瞎给），
   * 并把对不上的历史选择清掉，回落「跟随驱动设置」= 不传 `-m` = 用 U-King 写进配置的那个。 */
  const [codexProvider, setCodexProvider] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    if (agent !== "codex") return;
    let alive = true;
    invoke<{ codex_provider?: string | null }>("get_driver_status")
      .then((d) => alive && setCodexProvider(d?.codex_provider ?? null))
      .catch(() => alive && setCodexProvider(null));
    return () => {
      alive = false;
    };
  }, [agent]);
  const codexModels = useMemo(
    () => (codexProvider === "xiapan" ? CODEX_MODELS : []),
    [codexProvider],
  );
  useEffect(() => {
    // undefined = 还没问出来，别在这个空档里把用户的选择清掉
    if (agent !== "codex" || codexProvider === undefined) return;
    if (model && !codexModels.some((m) => m.id === model)) {
      setModel("");
      try {
        localStorage.removeItem(MODEL_KEY + agent);
      } catch {
        /* 同上，清不掉也不影响这一轮已经回落 */
      }
    }
  }, [agent, codexProvider, codexModels, model]);
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
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState<ReadyState>(null);
  /** `ready` 的现值，给挂在 Channel 上的回调读（见 applyEvent 里那条注释）。 */
  const readyRef = useRef<ReadyState>(null);
  useEffect(() => {
    readyRef.current = ready;
  }, [ready]);
  // 「看命令」：只留最近一轮（不落 localStorage —— 重启后那条命令的 --resume 会话早失效，
  // 摆一条跑不通的命令比不摆更坏）。首轮发送前不显示，因为确实还没有命令可摆。
  const [cmd, setCmd] = useState<CmdInfo | null>(null);
  /** 瞬时状态提示（网络重连等）。一轮结束就清 —— 它不是对话内容，不进 items。 */
  const [notice, setNotice] = useState<string | null>(null);
  /* ---- 「它还活着没」------------------------------------------------------
   * 这一轮从什么时候开始跑、最后一次收到事件是什么时候。**以前这两个数字一个都没有**：
   * 界面上只有一个转圈，客户没有任何依据判断「还在跑」和「已经死了」——
   * pc-*** 的客户就这么盯着转圈等了 25 分钟。
   * 用 ref 不用 state：每收一个流式片段就 setState 会把整条消息流重渲一遍。
   * ------------------------------------------------------------------------ */
  const startedAt = useRef(0);
  const lastBeatAt = useRef(0);
  /** 静默是从**哪个阶段**开始的（配 [`stageHint`]）。用 ref 同 lastBeatAt：每秒重渲那一行时现读。 */
  const stage = useRef<Stage>("startup");
  const stageTool = useRef<string | null>(null);
  const [tick, setTick] = useState(0); // 每秒 +1，只为让下面那行时长重渲
  useEffect(() => {
    if (!busy) return;
    const id = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [busy]);
  const elapsed = busy && startedAt.current ? Math.floor((Date.now() - startedAt.current) / 1000) : 0;
  const idle = busy && lastBeatAt.current ? Math.floor((Date.now() - lastBeatAt.current) / 1000) : 0;
  void tick; // tick 只用来触发重渲，值本身不参与计算（时长一律现算，切后台回来也准）
  const scrollRef = useRef<HTMLDivElement>(null);
  // 流式：当前正在累积的 assistant 文本下标
  const streamingIdx = useRef<number | null>(null);

  // 前置体检：claude 装没装 + 驱动配没配（决定是否拦在发送前给引导）
  const checkReady = useCallback(async () => {
    const [detect, driver] = await Promise.all([
      invoke<any>("detect_stack").catch(() => null),
      invoke<any>("get_driver_status").catch(() => null),
    ]);
    // `active.<tool>` 是后端 `runtime.driver.inspect` 给的「这台机器上它现在挂在谁身上」。
    // "official" = 客户自己的官方登录 / 自己的 Key（我们不注入），其余（xiapan 等）才是我们配的。
    const activeFor = agent === "codex" ? driver?.active?.codex : driver?.active?.claude;
    setReady({
      claudeFound: agent === "codex" ? !!detect?.codex?.found : !!detect?.claude?.found,
      hasDriver: agent === "codex" ? !!driver?.codex_provider : !!(driver?.claude_base || driver?.codex_provider),
      ownAccount: activeFor === "official",
    });
  }, [agent]);
  useEffect(() => {
    if (active) void checkReady();
  }, [active, checkReady]);

  const scrollToEnd = useCallback(() => {
    requestAnimationFrame(() => {
      const el = scrollRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    });
  }, []);

  // 把一个后端事件并进消息流
  const applyEvent = useCallback(
    (ev: any) => {
      const kind = ev?.kind as string;
      lastBeatAt.current = Date.now(); // 收到任何事件都算「那头还活着」
      // 顺手记下「接下来这段静默属于哪个阶段」—— 卡住时那行提示要靠它说实话
      if (kind === "tool_start" || kind === "tool_input") {
        stage.current = "tool";
        if (ev.name) stageTool.current = ev.name as string;
      } else if (kind === "tool_end") {
        stage.current = "await";
        stageTool.current = null;
      } else if (kind === "text" || kind === "text_done") {
        stage.current = "stream";
      } else if (kind === "session") {
        stage.current = "await";
      }
      // 「看命令」事件不进消息流（它描述的是这一轮怎么跑的，不是对话内容）
      if (kind === "command") {
        setCmd({ display: ev.display ?? "", teach: ev.teach ?? "", program: ev.program ?? agent, prompt_inlined: !!ev.prompt_inlined });
        return;
      }
      // 瞬时状态（网络重连）——同样不进消息流，只更新那一行提示
      if (kind === "notice") {
        setNotice(ev.text ?? "");
        return;
      }
      setItems((prev) => {
        const next = [...prev];
        if (kind === "text") {
          // 流式片段：追加到当前 assistant 文本（没有则新建）
          if (streamingIdx.current == null) {
            next.push({ kind: "text", role: "assistant", text: ev.text });
            streamingIdx.current = next.length - 1;
          } else {
            const it = next[streamingIdx.current];
            if (it && it.kind === "text") it.text += ev.text;
          }
        } else if (kind === "text_done") {
          // 完整文本：若没流式过则补一条（避免重复：流式已有就跳过）
          if (streamingIdx.current == null && ev.text) {
            next.push({ kind: "text", role: "assistant", text: ev.text });
          }
          streamingIdx.current = null;
        } else if (kind === "tool_start") {
          streamingIdx.current = null;
          next.push({ kind: "tool", id: ev.id, name: ev.name, done: false });
        } else if (kind === "tool_input") {
          const ti = next.find((x) => x.kind === "tool" && x.id === ev.id) as ToolItem | undefined;
          if (ti) ti.input = ev.input;
          else next.push({ kind: "tool", id: ev.id, name: ev.name, input: ev.input, done: false });
        } else if (kind === "tool_end") {
          const ti = next.find((x) => x.kind === "tool" && x.id === ev.id) as ToolItem | undefined;
          if (ti) {
            ti.output = ev.output;
            ti.isError = ev.is_error;
            ti.done = true;
          }
        } else if (kind === "usage") {
          streamingIdx.current = null;
          next.push({
            kind: "usage",
            inTok: ev.input_tokens ?? 0,
            outTok: ev.output_tokens ?? 0,
            cacheRead: ev.cache_read_input_tokens ?? ev.cache_read_tokens ?? 0,
            cacheWrite: ev.cache_creation_input_tokens ?? ev.cache_write_tokens ?? 0,
            costUsd: ev.cost_usd ?? 0,
            cny: 0, // 下面异步问后端按统一口径算，算回来再填
            ms: ev.duration_ms ?? 0,
          });
          // 按我们自己的口径折人民币（异步，算回来再填那一条 + 累进会话总账）
          const inTok = ev.input_tokens ?? 0, outTok = ev.output_tokens ?? 0;
          const cr = ev.cache_read_input_tokens ?? ev.cache_read_tokens ?? 0;
          const cw = ev.cache_creation_input_tokens ?? ev.cache_write_tokens ?? 0;
          const at = next.length - 1;
          void invoke<number>("chat_cost_cny", { model: model || agent, input: inTok, output: outTok, cacheRead: cr, cacheWrite: cw })
            .then((cny) => {
              if (!cny) return;
              setItems((cur) => cur.map((x, i) => (i === at && x.kind === "usage" ? { ...x, cny } : x)));
              setSpentCny((v) => v + cny);
            })
            .catch(() => {});
        }
        return next;
      });
      if (kind === "done") {
        streamingIdx.current = null;
        setBusy(false);
        setNotice(null); // 这一轮结束，瞬时状态清掉
        // 左侧小圆点的真相源：这一轮到底成没成。以前只在对话里贴一句「⚠️ 对话失败」，
        // 会话一多、或者人不在这个会话上，跑挂了就**没有任何地方会说**。
        // timeout 算「没成」——它确实没给出结果，拿绿点糊过去等于把故障藏起来。
        onStatus?.(ev.status === "error" || ev.status === "timeout" ? "error" : "idle");
        // 卡死收尾：说清楚「发生了什么 / 为什么 / 现在能怎么办」三件事。
        // 只说一句「超时」没有用 —— 客户既不知道是自己问错了还是软件坏了，也不知道下一步该干嘛。
        if (ev.status === "timeout") {
          const mins = Math.max(1, Math.round((ev.stall_secs ?? 300) / 60));
          setItems((prev) => [
            ...prev,
            {
              kind: "text",
              role: "assistant",
              text: t(
                "⏱️ 这一轮卡住了：整整 {mins} 分钟没有任何动静，已经自动帮你停下（它在后台起的命令也一并收掉了，不会继续占着你的电脑）。\n\n**最常见的原因**：它跑了一个不会自己结束的命令，比如启动一个服务器、或者一直在等你输入什么。\n\n**接下来可以试**：\n1. 把要做的事说得更具体一点，再发一次；\n2. 或者点上面「看命令」把这条命令复制到终端里自己跑一遍 —— 终端里能看到完整过程，也能随时按 Ctrl+C 停。",
                { mins },
              ),
            },
          ]);
        }
        // 出错收尾：把失败原因显式贴出来（不再默默卡住），并触发一次体检刷新引导横幅。
        // **认得出就给准话，认不出就承认认不出** —— 以前不管什么错都硬贴「多半是没接驱动/没装」，
        // 那是一句写死的猜测：真因是余额用完时，这句话会把客户支去重装驱动，越搞越远。
        if (ev.status === "error") {
          const raw = (ev.message && String(ev.message).trim()) || t("{agent} 退出码 {code}", { agent, code: ev.code ?? "?" });
          // 走 ref 不走闭包：`applyEvent` 被挂在 Channel 的 onmessage 上，一轮里不会重挂 ——
          // 直接闭 `ready` 会永远读到发这一轮时的旧值（首轮就是 null，403 会走错分支）。
          const known = humanizeError(raw, !!readyRef.current?.ownAccount);
          // 原始报错**永远保留**（折进代码块）：人话是给客户看的，原文是给排障看的，
          // 谁也不能替谁。截断留尾 —— 报错的关键信息通常在最后几行。
          const detail = "\n\n```\n" + (raw.length > 600 ? "…" + raw.slice(-600) : raw) + "\n```";
          const text = known
            ? t("⚠️ {what}\n\n**怎么办**：{how}", { what: t(known[0]), how: t(known[1], known[2]) }) + detail
            : t("⚠️ 这一轮没跑成，而且我没认出这是哪种问题。\n\n常见的两个方向：{agent} 没装好，或者驱动没配对 —— 可以点上方「去配驱动」一键修。还不行就用「技术支持」把下面这段发给我们。", { agent }) + detail;
          setItems((prev) => [...prev, { kind: "text", role: "assistant", text }]);
          void checkReady();
        }
      }
      scrollToEnd();
    },
    [scrollToEnd, checkReady, t, agent, onStatus],
  );

  /**
   * 真正发一轮。**从 `send` 里抽出来的**，为的是让「不是用户敲进输入框」的那一轮也能走同一条路——
   * 护照交接就是这种：会话是被一张任务护照点名建出来的，进来第一轮由机器发。
   * 抽出来而不是复制一份：复制的那份迟早跟这份漂开，而漂开的那次正好是出事那次。
   */
  const runTurn = useCallback(async (text: string) => {
    if (!text || busy) return;
    setItems((prev) => [...prev, { kind: "text", role: "user", text }]);
    setBusy(true);
    onStatus?.("running");
    startedAt.current = Date.now();
    lastBeatAt.current = Date.now();
    // 阶段跟着这一轮重置：不重置的话，上一轮停在「工具执行」，新一轮刚卡住就会报上一轮的工具名
    stage.current = "startup";
    stageTool.current = null;
    streamingIdx.current = null;
    scrollToEnd();
    const ch = new Channel<any>();
    ch.onmessage = applyEvent;
    try {
      // model 空 = 不覆盖，跟着驱动配置里的默认走（虾盘云 preset）。
      await invoke(`${agent}_send`, { taskId, prompt: text, cwd, model: model || null, system: system ?? null, onEvent: ch });
    } catch (e) {
      applyEvent({ kind: "text_done", text: t("\n[启动 {agent} 失败: {err}]", { agent, err: String(e) }) });
      setBusy(false);
      onStatus?.("error"); // 连起都没起来，同样是「跑挂了」
    }
  }, [busy, taskId, cwd, applyEvent, scrollToEnd, agent, system, model, t, onStatus]);

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || busy) return;
    const images = pendingImages;
    setInput("");
    setPendingImages([]);
    let prepared = text;
    try {
      if (images.length) prepared += `\n\n${await describeImages(images, text)}`;
    } catch (e) {
      setInput(text);
      setPendingImages(images);
      setItems((prev) => [...prev, { kind: "text", role: "assistant", text: t("⚠️ 图片识别失败：{e}。图片没有交给当前对话模型。", { e: String(e) }) }]);
      return;
    }
    await runTurn(prepared);
  }, [input, busy, runTurn, pendingImages, t]);

  /**
   * 护照交接的第一轮：宿主投进来一段状态，这里发一次就够。
   *
   * `seedSent` 认的是**内容**不是布尔：会话切来切去会重挂载，光用 `once` 布尔会在
   * 重挂载后清零、把同一张护照再发一遍 —— 用户会看到 AI 反复接手同一个任务。
   * `onSeedSent` 是**回执**，宿主那句「已送达」由它签字，不是宿主自己说的。
   */
  const seedSent = useRef<string | null>(null);
  useEffect(() => {
    const text = seedPrompt?.trim();
    if (!text || seedSent.current === text) return;
    seedSent.current = text;
    void runTurn(text).then(() => onSeedSent?.());
  }, [seedPrompt, runTurn, onSeedSent]);

  const interrupt = useCallback(() => {
    invoke(`${agent}_interrupt`, { taskId }).catch(() => {});
    setBusy(false);
    onStatus?.("idle"); // 人自己按的停，不是出错 —— 别拿红点吓他
  }, [taskId, agent, onStatus]);

  const reset = useCallback(() => {
    invoke(`${agent}_reset`, { taskId }).catch(() => {});
    setItems([]);
    streamingIdx.current = null;
  }, [taskId, agent]);

  // 输入框的 `@`（引用 cwd 里的文件）和 `/`（指令 + 起手词全表）。跟轻助手共用同一个 hook。
  // 这一侧的真指令只有「清空对话」—— 终端/文件面板归宿主管，这里编不出来就不列。
  const composer = useComposerMenu({
    value: input,
    setValue: setInput,
    textareaRef: inputRef,
    workspace: cwd,
    commands: useMemo(() => [{ label: "清空对话", hint: "这个会话从头开始", run: reset }], [reset]),
    onQuickPick,
  });

  // 只在「没装 claude」时拦一条引导（少干涉：原版 claude code 用户有自己的官方授权，
  // 不该催他配驱动）。装了但没配驱动 → 不弹横幅，让他自己用；真连不上时对话失败会提示。
  const notReady = ready && !ready.claudeFound;
  const notReadyMsg = t("还没装 Claude Code —— 这里的对话靠它驱动。先去装一下。");

  return (
    <div
      ref={dropRef}
      /* 🔴 **空态整块上下居中**（2026-08-18 客户指名照 MiniMax / WorkBuddy / Claude Cowork）。
         三家空屏的形状是同一个：品牌 + 输入框 + 起手词作为**一整块**浮在正中，
         上下留白对称。我们原来是「标题在中间、输入框钉在最底下」——
         中间那片空白把两者拉开，读起来是两个东西，不是一块。
         做法：空态时外层 `justify-center`、消息区不再 `flex-1` 抢高度、输入区去掉顶边框。
         有消息之后立刻恢复原样（输入框回到底部）——那时候钉底才是对的。 */
      className={cn(
        "relative flex flex-col h-full min-h-0 rounded-lg",
        items.length === 0 && !busy && "justify-center",
        dragOver && "ring-2 ring-inset ring-accent/60",
      )}
    >
      {/* 拖放高亮遮罩（测试报告 #028）：一圈 1px 的 ring 在深色底上看不见，
          客户拖着文件会以为不支持。与 Chat/作图/视频统一成明确的遮罩。 */}
      {dragOver && (
        <div className="absolute inset-0 z-20 rounded-lg border-2 border-dashed border-accent/60 bg-accent/[0.06] grid place-items-center pointer-events-none">
          <div className="text-accent text-[13px] font-semibold">{t("松手把文件路径插进输入框")}</div>
        </div>
      )}
      {notReady && (
        <div className="shrink-0 flex items-center gap-2.5 px-4 py-2 border-b border-amber-500/20 bg-amber-500/[0.08] text-[12.5px]">
          <AlertTriangle size={14} className="text-amber-400 shrink-0" />
          <span className="flex-1 text-ink-1">{notReadyMsg}</span>
          {onGoManage && (
            <button
              onClick={onGoManage}
              className="px-2.5 h-7 rounded-md bg-accent text-white text-[12px] font-medium hover:bg-accent-600 shrink-0"
            >
              {t("去装机")}
            </button>
          )}
        </div>
      )}
      {/* 消息流 —— Codex 式：居中单列文档流（不左右气泡）。select-text 覆盖全局 user-select:none，
          让对话内容能框选复制（WebView2 里默认禁选，客户反馈「U-Workspace 复制不了」的根因）。 */}
      <div ref={scrollRef} className={cn(
        "min-h-0 overflow-y-auto select-text",
        items.length === 0 && !busy ? "shrink-0" : "flex-1",
        short ? "px-3 py-2" : "px-4 py-5",
      )}>
        {/* 空态时把内层撑满可视高度并转 flex 列 —— 好让下面那块真的**垂直居中**。
            以前是 `pt-16` 把两行字钉在顶上，剩下大半屏是纯白（客户发来的截图里那片空白就是它）。
            **只在空态加这两个类**：有消息时保持原来的块级流，不动消息列的排版。 */}
        <div className="max-w-2xl mx-auto space-y-4">
          {items.length === 0 && (
            /* 借 Codex 桌面版那张空态：图标 + **点名工作文件夹**的一句问话 + 一行能力说明。
               「要在『X』里做点什么」比「和 Claude Code 对话」有用得多 —— 人在打字前真正
               需要确认的是**这句话会落在哪儿**，而那以前只是灰字里一条挤扁的长路径。
               **不摆 Codex 那四张引导卡**：起手词（QuickPrompts，21 条分 3 个场景）就是我们的
               等价物，已经摆在输入框正上方了；再加四张 = 同一件事两个入口，迟早漂移。 */
            /* 🔴 空态只剩「你在哪个文件夹」。
               它一路瘦过三版：①两段 51 字的说明 → ②一句 → ③现在只有文件夹名 + 路径。
               砍掉的都是**描述显而易见之事**的字：起手词就摆在正下方、输入框占位符里已经写着
               「@ 引用文件，/ 调指令」、diff 长什么样等它真出现时一眼就懂。连那个 56px 的大图标
               也去掉了 —— 它不承载任何信息，只是把真正有用的两行往下推了 80px。
               留下的这两行是唯一**看不出来**的事实：这句话会落到磁盘上的哪个地方。 */
            /* 空态主区 = **品牌两行**（2026-08-18 客户定的 slogan，形制照 WorkBuddy 那张
               「WorkBuddy / 你的职场超能力」）。
               🔴 原来这里是「会话名 + 一整条绝对路径」，客户原话「有点突兀」—— 说得对：
               那条路径是**上下文**不是**欢迎语**，它在 `+` 菜单的「在哪干活」和输入框
               下面那条轻行里都有，摆在这块最大的空白正中间等于让人先读一遍机器细节。
               这块地方该回答的是「这是什么、我能拿它干嘛」。 */
            <div className={cn("flex flex-col items-center justify-center text-center", short ? "py-1" : "py-2")}>
              <div className="min-w-0 max-w-full px-4">
                <div className={cn("font-bold text-ink-0 tracking-tight", short ? "text-[22px]" : "text-[30px]")}>
                  {t("U-King")}
                </div>
                <div className={cn("font-bold text-ink-1 tracking-tight mt-0.5", short ? "text-[16px]" : "text-[22px]")}>
                  {t("更多 AI，你来指挥")}
                </div>
              </div>
            </div>
          )}
          {items.map((it, i) => (
            <Bubble key={i} item={it} onDismiss={() => removeItem(i)} onPreview={onPreview} onRunInTerminal={onRunInTerminal} />
          ))}
        </div>
      </div>

      {/* 「看命令」：这一轮对话底下真实跑的就是这行 CLI —— 摆出来，别让人以为对话框是魔法 */}
      {cmd && <CommandStrip cmd={cmd} onRunInTerminal={onRunInTerminal} />}

      {/* 瞬时状态（目前只有网络重连）。**不进对话流**：它描述的是这一轮怎么跑的，不是内容。
          以前后端把 Codex 的 "Reconnecting... 2/5" 整个吞了，客户看到的就是「卡住不动」——
          那正是反馈里那个「5 次重连」的观感来源。现在摆出来：还在跑、跑到第几次了。 */}
      {notice && (
        <div className="shrink-0 px-4 py-1.5 border-t border-amber-400/15 bg-amber-400/[0.05] text-[11px] text-warning-700 dark:text-warning-400 flex items-center gap-1.5">
          <Loader2 size={11} className="animate-spin shrink-0" />
          {noticeText(notice, t)}
        </div>
      )}

      {/* 「它还活着没」—— 跑起来之后常驻的一行心跳。
          转圈本身不携带任何信息：跑了 5 秒和卡了 25 分钟长得一模一样。这行把两件事分开说 ——
          **已用多久**（还在动）和 **多久没动静**（可能挂了）。超过 IDLE_HINT_SECS 变琥珀色并
          给出下一步，别等后端那条 5 分钟死线到了才第一次开口。
          notice（网络重连）在的时候让位：那条已经说明「还在跑」了，两行挤一起只会更吵。 */}
      {busy && !notice && (
        /* 🔴 **跟消息列同一条基线**（2026-08-18 客户实拍：这条横条「超出了边界，和上下不一致」）。
           消息列和输入框都是 `max-w-2xl mx-auto` 居中，只有这条是满宽 —— 在宽屏上它比
           上下两块各宽出一大截，看起来像糊在外面的一条。底色/边框仍满宽（那是分隔线，
           该通到底），**只把内容收进同一个宽度**。 */
        <div
          className={
            "shrink-0 border-t " +
            (idle >= IDLE_HINT_SECS
              ? "border-amber-400/20 bg-amber-400/[0.06] text-warning-700 dark:text-warning-400"
              : "border-white/[0.06] bg-white/[0.02] text-ink-4")
          }
        >
        <div className="max-w-2xl mx-auto px-4 py-1.5 text-[11px] flex items-center gap-1.5">
          <Loader2 size={11} className="animate-spin shrink-0" />
          <span className="shrink-0">{t("正在干活 · 已用 {d}", { d: fmtDur(elapsed) })}</span>
          {idle >= IDLE_HINT_SECS && (
            <span className="truncate">
              {t("· 已经 {d} 没有新动静，{w}。等不及就点右边红色按钮停下。", {
                d: fmtDur(idle),
                w: stageHint(stage.current, stageTool.current, t),
              })}
            </span>
          )}
        </div>
        </div>
      )}

      {/* 输入区 —— Codex 式：居中、底部大输入框 */}
      <div className={cn("shrink-0", items.length === 0 && !busy ? "" : "border-t border-white/[0.06]", short ? "p-2" : "p-3")}>
        {/* 🔴 输入框上方那句小 slogan **删了**：空态主区已经是「U-King / 更多 AI，你来指挥」
            两行大字（照 WorkBuddy 那张），同一句话在一屏上说两遍不会更响，只会更挤。
            slogan 该有一个完整、够大的位置 —— 那个位置在上面，不是这儿。 */}
        {/* 本会话累计花费：只在真花过钱之后才出现，一行、极轻。
            客户原话「10 元很快就用完了」—— 他缺的不是省钱手段，是**看得见自己花到哪儿了**。 */}
        {spentCny > 0 && (
          <div className="max-w-2xl mx-auto mb-1 text-right text-[10.5px] text-ink-5 font-mono">
            {t("本会话累计 ≈¥{v}", { v: spentCny < 0.01 ? spentCny.toFixed(4) : spentCny.toFixed(2) })}
          </div>
        )}
        {/* 输入框卡片（WorkBuddy / Codex 式）：左下角是能力，右下角是动作。
            工具条上的每一项都必须能真改变这一轮怎么跑 —— 摆好看的一律不要。 */}
        <div className="max-w-2xl mx-auto">
          <Composer
            value={input}
            onChange={setInput}
            onSend={() => void send()}
            onStop={interrupt}
            busy={busy}
            disabled={busy}
            textareaRef={inputRef}
            menu={composer.menu}
            onKeyDown={composer.onKeyDown}
            onBlur={composer.onBlur}
            /* 🔴 **一句话**（2026-08-18 按 DSH 收）。原来一个占位符同时教了四件事
               （@ 引用文件 / 调指令 / Enter 发送 / Shift+Enter 换行），而占位符是**打第一个字
               就消失**的东西 —— 真正需要这些提示的时刻它已经不在了。教学挪进 `+` 菜单，
               那个随时点得开；`@` `/` 拖放照常能用。 */
            placeholder={
              busy
                ? t("{name} 正在干活…", { name: agentName })
                : t("告诉 U-King，你想完成什么…")
            }
            left={
              <>
                <AttachButton
                  onInsert={insertPaths}
                  onMention={() => {
                    // 「引用工作区里的文件」= 往输入框补一个 @ 让 ComposerMenu 接手，
                    // 不是第二套文件选择器（宪法第 12 条：公共能力复用不复制）
                    setInput((v) => (v && !/\s$/.test(v) ? v + " @" : v + "@"));
                    setTimeout(() => inputRef.current?.focus(), 0);
                  }}
                  onSlash={() => {
                    setInput((v) => (v ? v : "/"));
                    setTimeout(() => inputRef.current?.focus(), 0);
                  }}
                  hasWorkspace={!!cwd}
                  model={modelPicker}
                  experts={experts}
                />
                {/* 🔴 大脑/模型**不在左槽** —— 已挪到发送键旁的右槽（客户 2026-08-18：
                    「我们就把选择 agent 放到右侧如何」）。WorkBuddy / Claude Cowork / MiniMax
                    三家都是这个位置：左边 `+` 管「往这句话里加什么」，右边管「谁来跑」。
                    不传 `brainSlot` 时退回本地模型下拉 —— 别处直接用 ChatPanel 不受影响。 */}
                {brainSlot ? null : (
                  <ComposerSelect
                    value={model}
                    onChange={pickModel}
                    icon={Cpu}
                    title={t("这一轮用哪个模型（不选就跟着「虾盘云」里配的走）")}
                  >
                    <option value="">{t("模型：跟随驱动设置")}</option>
                    {agent === "codex"
                      ? codexModels.map((m) => (<option key={m.id} value={m.id}>{t(m.label)}</option>))
                      : XIAPAN_MODELS.map((g) => (
                          <optgroup key={g.group} label={t(g.group)}>
                            {g.items.map((m) => (<option key={m.id} value={m.id}>{t(m.label)}</option>))}
                          </optgroup>
                        ))}
                  </ComposerSelect>
                )}
                {/* 🔴 权限 chip **已折进 `+` 菜单**（客户：「权限……不要让客户选择，或者隐藏，
                    或者折叠进设置」）。它本来就是**只读事实**（agent/claude.rs 写死 bypassPermissions），
                    不是控件，摆在工具条上只是占地方还带个警告色。
                    **但没把这句话删掉** —— 改不动 ≠ 不用说，客户有权知道它不会逐条问他。 */}
              </>
            }
            right={
              <>
              {brainSlot}
              <button
                onClick={reset}
                title={t("清空对话（开新会话）")}
                className="inline-flex items-center justify-center w-7 h-7 rounded-lg text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
              >
                <RotateCcw size={14} />
              </button>
              </>
            }
            hint={
              // 「现金老虎」提醒：客户能在这儿选到最烧钱的一档，选中当场说，别等余额被打穿
              priceyModelHint(model) ? (
                <div className="mt-1.5 rounded-lg border border-danger-500/40 bg-danger-500/[0.10] px-2.5 py-1.5 text-[11px] leading-snug font-medium text-danger-700 dark:text-danger-400">
                  {t(priceyModelHint(model)!)}
                </div>
              ) : null
            }
            footer={composerFooter}
          />
          {/* 起手词在**输入框下面**（2026-08-18 按 MiniMax Code 的排法）：
              放上面会把 slogan 和输入框隔开，放下面才是「先看到能打字，再看到可以打什么」。 */}
          {/* 🔴 **两件都要做，不是二选一**（客户 2026-08-18：「点了几个没有反应」）。
              原来写的是 `onQuickPick ? onQuickPick(...) : setInput(...)` —— 而 `onQuickPick`
              指向宿主 `Chat.tsx::applyQuick`，它填的是**轻助手那个输入框**。默认大脑是 Claude，
              这一处渲染的是 ChatPanel 自己的输入框 → 字进了一个没渲染的 state，
              用户看到的就是「点了没反应」。
              宿主那半负责**该不该切大脑**（本组件没有 engine 状态），
              填字这半必须自己做 —— prop 注释里「不传就退化成填进自己的输入框」写的是
              兜底，被读成了非此即彼。 */}
          {items.length === 0 && !busy && (
            <QuickPrompts
              onPick={(tpl, best) => {
                onQuickPick?.(tpl, best!);
                setInput((v) => (v.trim() ? v : tpl));
                setTimeout(() => inputRef.current?.focus(), 0);
              }}
              className="mt-2"
            />
          )}
        </div>
        {!active && <div className="text-[10px] text-ink-5 mt-1">{t("（后台任务，切回查看）")}</div>}
      </div>
    </div>
  );
}

/* ---- 「看命令」条 ---------------------------------------------------------
 * 目的：把「GUI 对话框 ↔ 终端」这层窗户纸捅破 —— 你点的发送，底下就是这行 claude/codex。
 *
 * 【绝不做的事】不伪造终端画面。`-p` 无头模式没有 TTY，没有终端输出可镜像；
 * 把 JSON 事件流渲染成"终端的样子"是演的，客户迟早发现，那比不做更亏。
 * 这里只摆两样都为真的东西：真实 argv，和一条他自己能敲、且我们说清了差异的等价命令。
 * -------------------------------------------------------------------------- */
/**
 * 把上游那句英文瞬时状态翻成客户能据此判断的人话。
 *
 * Codex 发的是 `Reconnecting... 2/5`（上游 `responses_retry.rs`）。5 是它的
 * `stream_max_retries` 默认值 —— 客户反馈里那个「5 次重连」说的就是这个数。
 * **匹配不上就原样显示**：上游哪天改了文案，最多是看到英文，不会显示成空白或崩掉。
 */
function noticeText(raw: string, t: (s: string, v?: Record<string, string | number>) => string): string {
  const m = raw.match(/(\d+)\s*\/\s*(\d+)/);
  if (/reconnect/i.test(raw)) {
    return m
      ? t("网络不稳，正在重连（第 {n}/{max} 次）—— 还在跑，先别关", { n: m[1], max: m[2] })
      : t("网络不稳，正在重连 —— 还在跑，先别关");
  }
  return raw;
}

function CopyBtn({ text, label }: { text: string; label: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      onClick={() =>
        void copyToClipboard(text).then((ok) => {
          if (ok) {
            setDone(true);
            window.setTimeout(() => setDone(false), 1500);
          }
        })
      }
      title={label}
      className="inline-flex items-center gap-1 h-6 px-2 rounded-md border border-white/[0.08] bg-bg-1 text-[11px] text-ink-3 hover:text-ink-0 hover:border-accent/40"
    >
      {done ? <Check size={11} className="text-success-400" /> : <Copy size={11} />} {label}
    </button>
  );
}

function CommandStrip({ cmd, onRunInTerminal }: { cmd: CmdInfo; onRunInTerminal?: (c: string) => void }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <div className="shrink-0 border-t border-white/[0.06] bg-bg-1/40">
      <div className="max-w-2xl mx-auto px-3 py-1.5">
        <button
          onClick={() => setOpen((v) => !v)}
          className="w-full flex items-center gap-1.5 text-left group"
          title={t("这一轮对话，底下真实跑的命令")}
        >
          {open ? <ChevronDown size={12} className="text-ink-4 shrink-0" /> : <ChevronRight size={12} className="text-ink-4 shrink-0" />}
          <TermIcon size={11} className="text-accent/70 shrink-0" />
          <span className="text-[10.5px] text-ink-4 shrink-0">{t("底层命令")}</span>
          {!open && (
            <span className="flex-1 min-w-0 truncate font-mono text-[10.5px] text-ink-5 group-hover:text-ink-3">{cmd.teach}</span>
          )}
        </button>

        {open && (
          <div className="mt-1.5 mb-1 space-y-2.5">
            {/* ① 真实执行的命令 —— 一字不差，含只为渲染卡片而加的参数和 exe 全路径 */}
            <div>
              <div className="flex items-center gap-2 mb-1">
                <span className="text-[10.5px] text-ink-4">{t("① 这一轮真实执行的（一字不差）")}</span>
                <div className="flex-1" />
                <CopyBtn text={cmd.display} label={t("复制")} />
              </div>
              <pre className="font-mono text-[10.5px] leading-relaxed text-ink-3 bg-bg-0/60 rounded-md px-2 py-1.5 whitespace-pre-wrap break-all max-h-24 overflow-y-auto select-text">
                {cmd.display}
              </pre>
            </div>

            {/* ② 终端里的等价写法 —— 可点即跑，这才是「培养终端习惯」真正起作用的地方 */}
            <div>
              <div className="flex items-center gap-2 mb-1">
                <span className="text-[10.5px] text-ink-4">{t("② 你在终端里可以这么敲（交互式）")}</span>
                <div className="flex-1" />
                <CopyBtn text={cmd.teach} label={t("复制")} />
                {onRunInTerminal && (
                  <button
                    onClick={() => onRunInTerminal(cmd.teach)}
                    title={t("贴进右侧终端（不自动回车，你按回车才真跑）")}
                    className="inline-flex items-center gap-1 h-6 px-2 rounded-md border border-accent/30 bg-accent/[0.12] text-[11px] text-accent hover:bg-accent/[0.2]"
                  >
                    <Play size={10} /> {t("在终端跑")}
                  </button>
                )}
              </div>
              <pre className="font-mono text-[10.5px] leading-relaxed text-ink-1 bg-bg-0/60 rounded-md px-2 py-1.5 whitespace-pre-wrap break-all select-text">
                {cmd.teach}
              </pre>
              <div className="flex items-start gap-1.5 mt-1 text-[10px] text-ink-5 leading-relaxed">
                <Info size={10} className="shrink-0 mt-[2px]" />
                <span>
                  {t("和①不等价：去掉了只为把输出画成卡片而加的参数，改成交互模式（会问你要不要批准）。")}
                  {!cmd.prompt_inlined && t("这次的提示词太长/换行了，没并进命令 —— 敲完回车再把它粘进去。")}
                </span>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/* ---- 悬停操作条：复制这段 + 移除这条（健壮复制走 clipboard 兜底，避免 WebView2 静默失败） ---- */
function MsgActions({ text, onDismiss }: { text: string; onDismiss?: () => void }) {
  const { t } = useI18n();
  const [done, setDone] = useState(false);
  return (
    <div className="absolute top-0 right-0 opacity-0 group-hover:opacity-100 flex items-center gap-0.5 transition-opacity">
      <button
        onClick={() =>
          void copyToClipboard(text).then((ok) => {
            if (ok) {
              setDone(true);
              window.setTimeout(() => setDone(false), 1500);
            }
          })
        }
        title={t("复制这段")}
        className="inline-flex items-center justify-center w-6 h-6 rounded-md bg-bg-2/85 border border-white/[0.08] text-ink-4 hover:text-ink-1"
      >
        {done ? <Check size={12} className="text-success-400" /> : <Copy size={12} />}
      </button>
      {onDismiss && (
        <button
          onClick={onDismiss}
          title={t("移除这条（报错/无用消息可单独关，不影响其它对话）")}
          className="inline-flex items-center justify-center w-6 h-6 rounded-md bg-bg-2/85 border border-white/[0.08] text-ink-4 hover:text-danger-400"
        >
          <X size={12} />
        </button>
      )}
    </div>
  );
}

/* ---- 单条消息渲染（Codex 式文档流，不左右气泡）---- */
function Bubble({ item, onDismiss, onPreview, onRunInTerminal }: { item: Item; onDismiss?: () => void; onPreview?: (path: string) => void; onRunInTerminal?: (cmd: string) => void }) {
  const { t } = useI18n();
  if (item.kind === "text") {
    const user = item.role === "user";
    // 用户：左侧细竖条 + 略强背景的块；助手：纯文档正文（无气泡）。都带悬停复制/移除。
    if (user) {
      return (
        <div className="group relative border-l-2 border-accent/60 pl-3 pr-12 py-0.5">
          <div className="text-[10px] text-ink-5 mb-0.5 uppercase tracking-wide">{t("你")}</div>
          <div className="text-[13.5px] leading-relaxed text-ink-0 whitespace-pre-wrap">{item.text}</div>
          <MsgActions text={item.text} onDismiss={onDismiss} />
        </div>
      );
    }
    // AI 回复走 markdown 渲染（测试报告 #009「Markdown 渲染异常」）。
    // 模型吐的本来就是 markdown，直出 whitespace-pre-wrap = 满屏 `##` 和 `**`，
    // 客户读到的是一堆星号，看着像功能没做完。用的是 AutomationPanel 已经在用的那份
    // MiniMd（一份实现多处用，不为这个再引 react-markdown —— 体积红线）。
    // **只渲染 AI 的**：用户自己敲的字要一字不差地回显，替他解析星号是擅自改他的话。
    return (
      <div className="group relative pr-12 text-[13.5px] leading-relaxed text-ink-1">
        <MiniMd text={item.text} onRunInTerminal={onRunInTerminal} />
        <MsgActions text={item.text} onDismiss={onDismiss} />
      </div>
    );
  }
  if (item.kind === "usage") {
    // 🔴 只显示**我们自己算的 ¥**，不显示上游的 $：客户走虾盘云时那个 $ 跟真实扣费无关
    //（拿 Anthropic 价目表算 deepseek），而一个对不上的数字会让他连带不信别的数。
    // 缓存读单列出来 —— 「为什么输入这么多」十有八九答案在这儿，藏起来他只会觉得我们乱扣。
    const cost = item.cny > 0 ? ` · ≈¥${item.cny < 0.01 ? item.cny.toFixed(4) : item.cny.toFixed(2)}` : "";
    const cache = item.cacheRead > 0 ? ` · 缓存读 ${item.cacheRead}` : "";
    const sec = item.ms > 0 ? ` · ${(item.ms / 1000).toFixed(1)}s` : "";
    return (
      <div className="text-center text-[10.5px] text-ink-5 font-mono py-1">
        ↑{item.inTok} ↓{item.outTok} tokens{cache}{cost}{sec}
      </div>
    );
  }
  // tool 卡片
  return <ToolBubble item={item} onPreview={onPreview} />;
}

/**
 * 工具卡片。**成功的默认折起来，出错的默认摊开。**
 *
 * 🔴 客户原话「太多字、复杂、一堆字」——对话里最大的一片字就是这儿：以前每个工具无条件
 * 把 output 摊出来（最多 2000 字、48 行高的滚动块），一轮下来七八块。而那些输出**绝大多数
 * 人不会看**：`Read` 读了什么、`Grep` 匹配到几行，人关心的是「干了什么、成没成」，
 * 只有出错时才想看细节。
 *
 * 所以判据是**成败**，不是长度：
 *  - 成功 → 折成一行「查看输出（N 行）」，想看点一下；
 *  - 出错 → 直接摊开，那正是他要读的东西，再让他多点一下就是折磨。
 * Edit/Write 的 diff 不动 —— 那是「改了什么」，是结论不是过程。
 */
export function ToolBubble({ item, onPreview }: { item: Extract<Item, { kind: "tool" }>; onPreview?: (path: string) => void }) {
  const { t } = useI18n();
  const Icon = toolIcon(item.name);
  const isEdit = item.name === "Edit" || item.name === "Write";
  const input = item.input as any;
  const [open, setOpen] = useState(false);
  // 出错的默认摊开 —— 但 isError 是流式过程中才置上的，不能只在 useState 初值里判
  useEffect(() => {
    if (item.isError) setOpen(true);
  }, [item.isError]);
  const lines = item.output ? item.output.split(/\r?\n/).length : 0;
  const canToggle = !isEdit && !!item.output;
  const showOut = canToggle && open;
  const hint = toolHint(item.name, input);
  const [dir, base] = hintIsPath(item.name) ? splitPath(hint) : ["", ""];
  return (
    <div className="rounded-card border border-white/[0.08] bg-bg-1 overflow-hidden">
      {/* 🔴 头部行**自己就是折叠开关**（抄 AI Elements 的 Tool 组件）。
          以前每张卡在头部之下另起一行「查看输出（N 行）」——一屏七八张卡就是七八行同样的灰字，
          而那行字承载的信息只有一个数字。并进头部后：少一半行数，点击热区反而变大了一整行。 */}
      <button
        type="button"
        disabled={!canToggle}
        onClick={canToggle ? () => setOpen((v) => !v) : undefined}
        className={
          "w-full flex items-center gap-2 px-3 py-2 text-[12px] text-left" +
          (canToggle ? " hover:bg-white/[0.03] cursor-pointer" : " cursor-default")
        }
      >
        <Icon size={13} className={"shrink-0 " + (item.isError ? "text-danger-400" : toolTone(item.name))} />
        {/* 🔴 `shrink-0 whitespace-nowrap` 不是样式偏好，是防「运 行 命 令」竖排。
            flex 子项默认 `min-width:auto` —— 右边那条长命令**拒绝收缩到内容宽度以下**，
            于是压力全转嫁给左边这个没设 nowrap 的中文标签，被挤成一列一个字。
            中文没有词边界，任何两字之间都是合法断点，所以中文标签是这套布局里最先塌的一环，
            而英文 `Run command` 有空格、塌得没那么难看 —— 开发机切英文界面看不出来。
            右边的 `min-w-0` 是让 `truncate` 真的生效（没有它 truncate 是装饰）。 */}
        <span className="font-medium text-ink-1 shrink-0 whitespace-nowrap">
          {item.name ? (TOOL_LABELS[item.name] ? t(TOOL_LABELS[item.name]) : item.name) : t("工具")}
        </span>
        {/* 路径：目录那截随便砍，文件名那截 shrink-0 永不收缩（见 splitPath 头注） */}
        {hintIsPath(item.name) && base ? (
          <span className="flex items-baseline min-w-0 flex-1 font-mono text-[11px]">
            <span className="truncate text-ink-5">{dir}</span>
            <span className="shrink-0 text-ink-3">{base}</span>
          </span>
        ) : (
          <span className="text-ink-4 truncate font-mono text-[11px] min-w-0 flex-1">{hint}</span>
        )}
        {canToggle && (
          <span className="shrink-0 flex items-center gap-0.5 text-[10.5px] text-ink-5">
            {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
            {t("{n} 行", { n: lines })}
          </span>
        )}
        <span className={"shrink-0 dot " + (item.done ? (item.isError ? "dot-warn" : "dot-on") : "dot-off")} />
      </button>
      {/* Edit/Write → 内联 diff */}
      {isEdit && input && (
        <DiffView
          path={input.file_path || input.path || ""}
          oldStr={input.old_string ?? ""}
          newStr={input.new_string ?? input.content ?? ""}
        />
      )}
      {/* 「看得见的产出」：干完活产出的文件，给一条真能拿到手的路。
          文件类工具看路径，Bash 看输出里认得出的产出文件（认不出就不给按钮）。 */}
      {item.done && !item.isError && (() => {
        const fromPath = input?.file_path || input?.path || "";
        const targets = deliverableExt(fromPath) ? [fromPath] : producedFiles(item.output);
        return targets.map((p) => <ProducedFile key={p} path={p} onPreview={onPreview} />);
      })()}
      {/* 输出本体。开关已并进头部行 —— 这里不再单独占一行（见头部那段注释） */}
      {showOut && item.output && (
        <pre className="px-3 pb-2 text-[11px] leading-relaxed text-ink-3 font-mono whitespace-pre-wrap max-h-48 overflow-y-auto border-t border-white/[0.05] pt-2">
          {item.output.length > 2000 ? item.output.slice(0, 2000) + t("\n…（已截断）") : item.output}
        </pre>
      )}
    </div>
  );
}

/* ---- 「看得见的产出」 ------------------------------------------------------
 * Claude/Codex 干完活以前只留一行 `Write: xxx.html ✓` —— 写出来的网页点不开、
 * 跑脚本出的图看不见，只有轻助手那侧有预览。这两个小函数就是把「产出」认出来。
 * ------------------------------------------------------------------------- */

/** 字节数 → 人话。顺带当「这文件是不是空的」的判据：0 字节的成品就是没做成。 */
function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * 成品卡片 —— 「做完了 → 这是成品 → 拿得到手」的最后一步。
 *
 * 办公活（PPT/Word/Excel）以前到这儿就断了：右侧面板渲染不了这些格式，于是一个按钮都没有，
 * 文件躺在磁盘某处，客户既不知道在哪也打不开。所以这里按格式给**不同的路**：
 * 能渲染的给「预览」（走宿主右侧面板），不能渲染的给「打开」（交给 Word/PPT），
 * 以及永远都有的「在文件夹中显示」—— 那是遇到任何意外时都成立的退路。
 *
 * **先问后端文件在不在，再决定给不给按钮**：AI 报的路径可能压根不存在（说完就忘、或写错目录）。
 * 「点了必失败的按钮比没有按钮更伤」是这一块既有的原则，办公产物同样适用。
 * 0 字节也当没做成 —— 一个打开就是空白的 PPT，比没有更让人恼火。
 */
export function ProducedFile({ path, onPreview }: { path: string; onPreview?: (p: string) => void }) {
  const { t } = useI18n();
  const [info, setInfo] = useState<{ exists: boolean; size: number; openable: boolean } | null>(null);
  const [err, setErr] = useState("");
  // 「⋯」菜单：**一个主按钮 + 其余收起来**。
  // 客户原话是「简化是要的，文件预览和打开是要的，复制也要」—— 这三件事互相打架：
  // 全平铺就是一排六个按钮（比现在还挤），全收起来又等于没有。所以主路径（预览）留在外面，
  // 其余（换个程序打开 / 复制路径 / 复制内容 / 在文件夹里找它）收进菜单，用的是
  // 文件面板右键菜单那套**同一批后端命令**，没有第二份实现。
  const [menu, setMenu] = useState(false);
  const menuBtnRef = useRef<HTMLButtonElement>(null);
  const [copied, setCopied] = useState("");
  /**
   * 图片产物直接**内联显示那张图**，不是「一张卡片 + 一个『看这张图』按钮」。
   *
   * AI 画完图，人第一件事是看它画成什么样 —— 让他为此点两下（点按钮 → 跳右侧面板）
   * 是把最高频的动作放到了第二层。Claude / Codex 那侧都是直接出图，我们这侧不该更绕。
   * 点缩略图仍然进右侧大图（放大/标注在那儿），所以没丢任何能力。
   *
   * asset 协议要先放行所在目录，否则 403 白图（同 FilesPanel 那条历史 bug）。
   */
  const [imgSrc, setImgSrc] = useState("");
  useEffect(() => {
    let alive = true;
    invoke<{ exists: boolean; size: number; openable: boolean }>("produced_file_info", { path })
      .then((r) => alive && setInfo(r))
      .catch(() => alive && setInfo({ exists: false, size: 0, openable: false }));
    return () => {
      alive = false;
    };
  }, [path]);

  // 图片：放行目录后拿 asset 地址（拿不到就退回原来的「按钮」形态，不至于什么都没有）
  useEffect(() => {
    if (previewableExt(path) !== "image") return;
    let alive = true;
    const dir = path.replace(/[\\/][^\\/]*$/, "");
    invoke("allow_fs_preview", { path: dir })
      .catch(() => {})
      .then(() => {
        if (alive) setImgSrc(convertFileSrc(path));
      });
    return () => {
      alive = false;
    };
  }, [path]);

  if (!info?.exists || info.size === 0) return null;
  const kind = previewableExt(path);
  const name = path.split(/[\\/]/).pop() || path;
  const ext = (path.split(/[?#]/)[0].split(".").pop() ?? "").toLowerCase();
  const act = (cmd: string) => invoke(cmd, { path }).catch((e) => setErr(String(e)));
  const flash = (m: string) => {
    setCopied(m);
    setTimeout(() => setCopied(""), 1600);
  };
  /** 文本类才给「复制内容」—— 对 .docx/.png 复制出来的是一堆乱码，那不叫功能。 */
  const textLike = ["txt", "md", "csv", "json", "log", "html", "htm", "svg", "srt", "yaml", "yml", "xml", "js", "ts", "py", "sh", "bat", "ps1"].includes(ext);

  const menuItems: { label: string; run: () => void }[] = [
    ...(info.openable ? [{ label: "用默认程序打开", run: () => void act("open_produced_file") }] : []),
    { label: "在资源管理器中显示", run: () => void act("reveal_produced_file") },
    { label: "用 VS Code 打开", run: () => void invoke("open_dir_external", { path, app: "vscode" }).catch((e) => setErr(String(e))) },
    { label: "复制路径", run: () => void copyToClipboard(path).then((ok) => flash(ok ? t("路径已复制") : t("复制失败"))) },
    ...(textLike
      ? [{
          label: "复制内容",
          run: () =>
            void invoke<string>("read_text_file", { path })
              .then((s) => copyToClipboard(s))
              .then((ok) => flash(ok ? t("内容已复制") : t("复制失败")))
              .catch((e) => setErr(String(e))),
        }]
      : []),
  ];

  return (
    <div className="mx-3 mb-2 -mt-0.5 rounded-lg border border-accent/25 bg-accent/[0.06] px-2.5 py-2">
      <div className="flex items-center gap-1.5 min-w-0">
        <FileEdit size={12} className="text-accent shrink-0" />
        <span className="text-[11.5px] text-ink-1 font-medium truncate" title={path}>
          {name}
        </span>
        <span className="text-[10.5px] text-ink-4 shrink-0">{fmtSize(info.size)}</span>
      </div>
      {/* 图就是图本身 —— 点它放大到右侧（那儿有缩放和标注） */}
      {imgSrc && (
        <img
          src={imgSrc}
          alt={name}
          onClick={() => onPreview?.(path)}
          className="mt-1.5 max-h-[180px] max-w-full rounded-md border border-white/[0.08] cursor-zoom-in object-contain"
        />
      )}
      <div className="flex items-center gap-1.5 flex-wrap mt-1.5">
        {kind && onPreview && (
          <button
            onClick={() => onPreview(path)}
            className="inline-flex items-center gap-1 h-6 px-2 rounded-md bg-accent/15 border border-accent/30 text-accent text-[11px] hover:bg-accent/25"
          >
            <Eye size={11} />{" "}
            {kind === "web"
              ? t("预览网页")
              : kind === "video"
                ? t("预览视频")
                : kind === "doc"
                  ? t("预览")
                  : t("看这张图")}
          </button>
        )}
        {/* 没有预览路（办公三件套之外的、渲染不了的）时，「打开」升级成主按钮 ——
            那种情况下它就是唯一能把成品拿到手的动作，不该躲在菜单里。 */}
        {!kind && info.openable && (
          <button
            onClick={() => act("open_produced_file")}
            className="inline-flex items-center gap-1 h-6 px-2 rounded-md bg-accent/15 border border-accent/30 text-accent text-[11px] hover:bg-accent/25"
            title={t("用电脑上的默认程序打开")}
          >
            <Play size={11} /> {t("打开")}
          </button>
        )}
        <div>
          {/* 🔴 菜单必须走 AnchoredMenu（fixed），**不能用 absolute**：这张卡片本身是
              `rounded-card … overflow-hidden`，absolute 的菜单会被整块裁掉，客户看到的是
              「点了没反应」。调 z-index 没用 —— overflow 裁剪跟 z 无关。 */}
          <button
            ref={menuBtnRef}
            onClick={() => setMenu((v) => !v)}
            className="inline-flex items-center gap-1 h-6 px-2 rounded-md border border-white/[0.10] text-ink-3 text-[11px] hover:text-ink-0 hover:border-accent/40"
            title={path}
          >
            <FolderOpen size={11} /> {t("打开方式 / 复制")}
            <ChevronDown size={10} />
          </button>
          {menu && (
            <AnchoredMenu anchorRef={menuBtnRef} onClose={() => setMenu(false)} items={menuItems} t={t} />
          )}
        </div>
        {copied && <span className="text-[10.5px] text-accent">{copied}</span>}
      </div>
      {err && <div className="mt-1 text-[10.5px] text-warning-700 dark:text-warning-400">{err}</div>}
    </div>
  );
}

/** 这个扩展名算不算「成品」—— 值得在对话里给一条**拿到手**的路。
 *
 *  比 `previewableExt` 宽一截，因为办公三件套（docx/pptx/xlsx）右侧面板根本渲染不了，
 *  可客户要的本来就不是预览而是**打开**。以前只按「能不能预览」发按钮，
 *  结果办公活做完了对话里一个按钮都没有 —— 文件躺在磁盘某处，客户不知道在哪。
 *  真正能不能打开由后端白名单说了算（`fs.rs::OPENABLE_EXTS`），这里只负责「值不值得认出来」。 */
export function deliverableExt(p: string): boolean {
  const ext = (p.split(/[?#]/)[0].split(".").pop() ?? "").toLowerCase();
  return (
    !!previewableExt(p) ||
    ["docx", "doc", "pptx", "ppt", "xlsx", "xls", "csv", "pdf", "md", "txt", "zip", "srt", "mp3", "wav"].includes(ext)
  );
}

/** 这个扩展名值不值得给一个「预览」按钮。**只列右侧面板真渲染得了的** ——
 *  挂一个点了只出乱码的按钮，比没有按钮更伤。
 *
 *  🔴 「doc」那一档直接用 redline 自己导出的 `REDLINE_EXTS`（registry 的 `EXT_TO_FORMAT` 键）。
 *  这里原本抄了一份清单，加格式时只改一边就会漂 —— 抄的那份到 2026-08-17 已经漏了
 *  `md`/`ico`/`avif`，而 viewer 明明渲染得了（宪法第 8 条）。 */
export function previewableExt(p: string): "" | "web" | "image" | "video" | "doc" {
  const ext = (p.split(/[?#]/)[0].split(".").pop() ?? "").toLowerCase();
  if (["html", "htm", "svg"].includes(ext)) return "web";
  if (["png", "jpg", "jpeg", "webp", "gif", "bmp"].includes(ext)) return "image";
  if (["mp4", "webm", "mov"].includes(ext)) return "video";
  if (REDLINE_EXTS.includes(ext)) return "doc";
  return "";
}

/** 从 Bash 输出里认出「刚生成的那个文件」。
 *
 *  只认两种**明确的**形态，绝不拿正则去扫整段输出碰运气：
 *   ① uking-aigc 那类脚本的 `--json` 收尾：`{"ok":true,"file":"D:/x/a.png"}`
 *   ② 独占一行、且整行就是一个带盘符/斜杠的可预览文件路径
 *  宽松匹配会从日志里挖出一堆不存在的路径，做成一排点了必失败的按钮 ——
 *  那比没有按钮更伤。取最后一个：一条命令可能产出多个，最后那个才是最终成品。 */
/**
 * 从 Bash 输出里认出「刚生成的那个文件」。
 *
 * 只认两种**明确的**形态，绝不拿正则扫整段输出碰运气：
 *  ① uking-aigc 那类脚本的 `--json` 收尾：`{"ok":true,"file":"D:\\x\\a.png"}`
 *  ② 独占一行、整行就是一个带分隔符的可预览文件路径
 * 宽松匹配会从日志里挖出一堆不存在的路径，做成一排点了必失败的按钮 —— 那比没有按钮更伤。
 * 多个产出取最后一个：一条命令可能产出中间件，最后那个才是成品。
 *
 * 下面三个正则用 String.raw 定义，别在字面量里手写反斜杠 ——
 * `[\/]` 在字符类里只等于 `/`（反斜杠被当转义吃掉），Windows 路径会全漏。
 */
const NEWLINE_RE = new RegExp(String.raw`\r?\n`);
const WS_RE = new RegExp(String.raw`\s`);
const SEP_RE = new RegExp(String.raw`[\\/]`);

export function producedFiles(output?: string, max = 3): string[] {
  if (!output) return [];
  const hits: string[] = [];
  // JSON 里的反斜杠是转义过的（"D:\\x\\a.png"），还原成真路径。
  // `html` 也认：`gen-pptx.mjs` 出的是**一对**产物 —— .pptx 是交付物、.预览.html 是能秒开的那份。
  // 只认最后一个的话，客户永远只拿到其中一个，另一个躺在磁盘上没人知道。
  for (const m of output.matchAll(/"(?:file|path|output|html|preview)"\s*:\s*"([^"]+)"/g)) {
    const p = m[1].replace(/\\\\/g, "\\");
    if (deliverableExt(p)) hits.push(p);
  }
  if (!hits.length) {
    for (const line of output.split(NEWLINE_RE)) {
      const s = line.trim().replace(/^["']|["'],?$/g, "");
      if (!s || WS_RE.test(s)) continue; // 带空格的多半是句子，不是路径
      if (!SEP_RE.test(s)) continue; // 没有任何路径分隔符 = 光一个文件名，没法定位
      if (deliverableExt(s)) hits.push(s);
    }
  }
  // 去重保序；超过上限只留**最后 max 个** —— 一条命令常先产中间件（分镜图/临时片段）再产成品，
  // 越靠后越可能是客户真正要的那个。全列出来会把成品淹在一排中间件里。
  const uniq = Array.from(new Set(hits));
  return uniq.length > max ? uniq.slice(-max) : uniq;
}

function toolHint(name: string, input: any): string {
  if (!input) return "";
  if (name === "Bash") return input.command ?? "";
  if (name === "Read" || name === "Edit" || name === "Write") return input.file_path ?? input.path ?? "";
  if (name === "Grep") return input.pattern ?? "";
  if (name === "Glob") return input.pattern ?? "";
  if (name === "WebFetch") return input.url ?? "";
  return "";
}

/** 这个提示是不是一条路径 —— 是的话要保住文件名（见 splitPath）。 */
function hintIsPath(name: string): boolean {
  return name === "Read" || name === "Edit" || name === "Write" || name === "NotebookEdit";
}

/**
 * 拆成「目录 / 文件名」两截。
 *
 * 🔴 为什么不能直接 `truncate`：CSS 砍的是**尾巴**，而路径的信息量全在尾巴上。
 * 客户看到的是 `/Users/<客户名>/Documents/GitHub/<项目>/src/lib/auto-chart/<文件名>…`
 * —— 一屏七八张卡，每张都以同样的前缀开头、都在同一个位置被砍断，
 * 于是「AI 到底动了哪个文件」这件事在界面上**不存在**。
 * 拆开之后目录那截可以随便砍（丢中间层级无所谓），文件名那截 `shrink-0` 永不收缩。
 */
function splitPath(p: string): [string, string] {
  const s = p || "";
  const i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\"));
  return i < 0 ? ["", s] : [s.slice(0, i + 1), s.slice(i + 1)];
}

/**
 * 工具分三档配色 —— 判据是**它对你的机器做了什么**，不是它属于哪个 SDK 分类。
 *
 * 一屏十几张卡以前全是同一个 accent 蓝，人只能逐字读标签才知道哪张要紧。
 * 现在：只读的退到中性色（看看而已，不该抢注意力）、写文件的用主色（有产出）、
 * 在你机器上执行命令的用警示色（最该多看一眼的那类）。
 */
function toolTone(name: string): string {
  if (name === "Write" || name === "Edit" || name === "NotebookEdit") return "text-accent-400";
  if (name === "Bash" || name === "KillShell" || name === "SlashCommand" || name === "Task") return "text-warning-400";
  return "text-ink-4";
}
