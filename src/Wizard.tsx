/**
 * 对话式安装向导 —— 聊天气泡式引导：
 *
 *   体检 → 选工具 → 流式安装（自动验证/修复）→ 选驱动 → 填 Key（或去充值）
 *   → 写入底层配置（cc-switch 式）→ 实测连通（模型真实回一句话）→ 完成
 *
 * 全部动作落在 Rust 命令上；安装日志经 `uking:wizard` 事件流式进气泡。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { openRecharge } from "./lib/recharge";
import { useI18n } from "./i18n";
import {
  Bot,
  CheckCircle2,
  ExternalLink,
  KeyRound,
  Loader2,
  RefreshCw,
  Send,
  Stethoscope,
  Wand2,
  XCircle,
} from "lucide-react";
import { cn } from "./lib/cn";
import { ACTION, createTauriActionClient } from "./generated/action-client";
import type { DeviceKey } from "./lib/types";

/** 收尾自检走影核动作 —— 和 CLI / MCP 同一条路，不另写一份判据。 */
const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

/* ---------------- 与 Rust 对齐的类型 ---------------- */

type CmdProbe = { found: boolean; version: string | null };
export type StackDetect = {
  node: CmdProbe;
  npm: CmdProbe;
  claude: CmdProbe;
  codex: CmdProbe;
  git: CmdProbe;
  claude_desktop: boolean;
  codex_app: boolean;
  portable_node: boolean;
  system_proxy: string | null;
};

type InstallToolResult = {
  ok: boolean;
  tool: string;
  version: string | null;
  attempts: number;
  error: string | null;
};

export type ProviderPreset = {
  id: string;
  name: string;
  summary: string;
  openai_base: string;
  anthropic_base: string | null;
  model: string;
  small_model: string;
  codex_model?: string;
  codex_wire_api?: string;
  key_url: string;
  key_hint: string;
  builtin_recharge: boolean;
  recommended: boolean;
  /** 内置预置（不可删/改）；自定义 provider = false */
  builtin?: boolean;
  /** 自定义 provider 自带的 API Key（切到它时一起传给 apply_provider） */
  api_key?: string;
};

type TestResult = {
  ok: boolean;
  api: string;
  latency_ms: number;
  reply: string | null;
  error: string | null;
};

type Balance = { tokens: number; cny?: number; text: string };

type Diagnosis = { diagnosis: string; commands: string[] };

const short = (k: string) => (k.length > 16 ? `${k.slice(0, 10)}…${k.slice(-4)}` : k);

/* ---------------- 消息模型 ---------------- */

type Choice = { label: string; value: string; tone?: "gold" | "plain" };

type Msg = {
  id: number;
  role: "uking" | "user";
  text?: string;
  /** 流式安装日志（持续追加） */
  log?: string[];
  logDone?: boolean;
  logOk?: boolean;
  /** 体检结果卡 */
  detect?: StackDetect;
  /** 实测结果卡 */
  tests?: { label: string; r: TestResult }[];
  balance?: Balance | null;
};

let nextId = 1;

// 队列里的每一步都用 `t(TOOL_NAMES[tool])` 做文案，**漏一个就显示 "undefined 安装成功"**。
// 所以这里必须覆盖所有可能进队列或被工具市场点进来的 id，不只是默认装的三件套。
const TOOL_NAMES: Record<string, string> = {
  "claude-code": "Claude Code CLI",
  pi: "pi",
  hermes: "Hermes Agent",
  dsh: "DeepSeek Harness",
  "harness-doctor": "Harness Doctor",
  codex: "Codex CLI",
  "codex-app": "Codex 桌面版",
  openclaw: "OpenClaw CLI（原版）",
  "qwen-code": "Qwen Code",
  crush: "Crush",
  opencode: "OpenCode",
  cline: "Cline",
};

/** 写驱动配置的目标（apply_provider 的 targets）展示名。
 *  以前这行文案是 `tg === "claude" ? "Claude Code" : "Codex"` —— 装了 ClawX / Hermes 的机器
 *  会被写成「Claude Code + Codex + Codex」，说的和实际写的不是一回事。 */
const TARGET_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  clawx: "ClawX",
  hermes: "Hermes",
  dsh: "DeepSeek Harness",
};

/** 环境地基的展示名（id 对应 toolbox.rs 里的 CapabilityTool）。 */
const ENV_NAMES: Record<string, string> = {
  "windows-terminal": "Windows Terminal（治终端乱码）",
  pwsh: "PowerShell 7",
  git: "Git",
  ffmpeg: "ffmpeg（视频拼接/成片要它）",
  libreoffice: "LibreOffice（导出 PDF、看老 .doc 要它，包较大请耐心）",
  markitdown: "MarkItDown（读客户拿来的 Word/Excel/PDF）",
};

/** Codex 桌面版目前只有 Windows 一键装（微软商店 MSIX；Mac 用户走官网 DMG） */
const IS_WINDOWS = navigator.userAgent.includes("Windows");

/** Codex 桌面版手动安装入口（自动装不上时的兜底，全是稳定可达的官方/镜像直链）。 */
const CODEX_APP_LINKS = {
  /** 微软商店 App 内页协议（直接拉起商店 App 到 Codex 页，点「获取」即可装） */
  msStoreApp: "ms-windows-store://pdp/?productid=9PLM9XGG6VKS",
  /** 微软商店网页版（商店 App 打不开时退而用浏览器，仍是官方页面） */
  msStoreWeb: "https://apps.microsoft.com/detail/9PLM9XGG6VKS",
  /** 国内镜像 MSIX 直链（约 664MB，浏览器下载后双击安装；商店渠道全不通时用） */
  msixMirror: "https://codexapp.agentsmirror.com/latest/win",
};

/* ---------------- 主组件 ---------------- */

export function Wizard({
  preselect,
  onFinished,
  onGoWorkspace,
}: {
  /** 从工具市场点进来时预选的工具 id */
  preselect?: string | null;
  onFinished?: () => void;
  /** 装完把人送进 U-Workspace（0.9.85 起的落点：装完不再指向 ClawX）。
   *  没传就只显示文字引导，不给点了没反应的按钮。 */
  onGoWorkspace?: () => void;
}) {
  const { t } = useI18n();
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [choices, setChoices] = useState<Choice[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [showKeyInput, setShowKeyInput] = useState(false);
  // 当前选中驱动（渲染 Key 提示用），其余流程上下文都在 ctx
  const [activeProvider, setActiveProvider] = useState<ProviderPreset | null>(null);

  // 流程上下文
  const ctx = useRef<{
    detect: StackDetect | null;
    queue: string[]; // 待安装工具
    installed: string[]; // 本轮装成功 + 原本就有的
    provider: ProviderPreset | null;
    apiKey: string;
    deviceKey: DeviceKey | null; // 设备指纹内置 Key
    lastLog: string[]; // 全量安装日志（AI 诊断上下文）
    onChoice: ((v: string) => void) | null;
    onKey: ((k: string) => void) | null;
    logMsgId: number | null;
    installAllThenXiapan: boolean; // 「一键全安装」流程：队列装完后自动接虾盘云 + 送进 U-Workspace
  }>({
    detect: null,
    queue: [],
    installed: [],
    provider: null,
    apiKey: "",
    deviceKey: null,
    lastLog: [],
    onChoice: null,
    onKey: null,
    logMsgId: null,
    installAllThenXiapan: false,
  });

  const bottomRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [msgs, choices, showKeyInput]);

  const push = useCallback((m: Omit<Msg, "id">): number => {
    const id = nextId++;
    setMsgs((prev) => [...prev, { ...m, id }]);
    return id;
  }, []);

  const patch = useCallback((id: number, fn: (m: Msg) => Msg) => {
    setMsgs((prev) => prev.map((m) => (m.id === id ? fn(m) : m)));
  }, []);

  /** 等用户点选项 */
  const ask = useCallback((options: Choice[]): Promise<string> => {
    return new Promise((resolve) => {
      ctx.current.onChoice = (v) => {
        ctx.current.onChoice = null;
        setChoices(null);
        resolve(v);
      };
      setChoices(options);
    });
  }, []);

  /** 等用户输入 Key */
  const askKey = useCallback((): Promise<string> => {
    return new Promise((resolve) => {
      ctx.current.onKey = (k) => {
        ctx.current.onKey = null;
        setShowKeyInput(false);
        resolve(k);
      };
      setShowKeyInput(true);
    });
  }, []);

  /* ---------- 安装日志事件 ---------- */
  useEffect(() => {
    const un = listen<{ tool: string; phase: string; line: string }>("uking:wizard", (e) => {
      const { phase, line } = e.payload;
      // 全量留底给 AI 诊断
      ctx.current.lastLog.push(line);
      if (ctx.current.lastLog.length > 300) ctx.current.lastLog.splice(0, 100);
      const id = ctx.current.logMsgId;
      if (id == null) return;
      const text = phase === "step" ? `▶ ${line}` : phase === "verify" ? `✦ ${line}` : phase === "repair" ? `🔧 ${line}` : line;
      patch(id, (m) => ({ ...m, log: [...(m.log ?? []), text].slice(-400) }));
    });
    return () => {
      un.then((f) => f());
    };
  }, [patch]);

  /* ---------- 流程编排 ---------- */

  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    runFlow();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function runFlow() {
    push({
      role: "uking",
      text: t("你好，我是 U-King AI 管家 👋 我来帮你把 AI 编程工具装到这台电脑，并接上国内可用的大模型驱动。先给电脑做个体检…"),
    });
    setBusy(true);
    const detect = await invoke<StackDetect>("detect_stack").catch(() => null);
    setBusy(false);
    if (!detect) {
      push({ role: "uking", text: t("体检失败了，请重开窗口再试。") });
      return;
    }
    ctx.current.detect = detect;
    push({ role: "uking", detect });

    // 已装的直接记入
    if (detect.claude.found) ctx.current.installed.push("claude-code");
    if (detect.codex.found) ctx.current.installed.push("codex");
    if (detect.codex_app) ctx.current.installed.push("codex-app");

    await pickTools(detect);
  }

  async function pickTools(detect: StackDetect) {
    // 「一键全安装」：客户点首屏大按钮进来，不再逐个问，直接把所有可装工具排队装上。
    //
    // 顺序定调（0.9.85 改）：**Claude Code 排第一，ClawX 退出默认队列**。
    // 此前（v0.9.7）是反过来的 —— ClawX 当「主力工具」抢在最前面装 261MB。改的理由：
    //   ① 主交互面已经是 U-Workspace（对话 + 终端 + 作图都在 U-King 内），ClawX 是第二个
    //      对话壳，两个入口让小白二选一犯懵，还多一份配置面（clawx-providers.json）；
    //   ② ClawX 装完必须重启才吃配置、首启还要过一道「允许访问网络」，是老售后重灾区；
    //   ③ 261MB 卡在第一步，慢网客户还没见到任何能用的东西就先等十几分钟。
    // **不是删功能**：ClawX 的安装/检测/切驱动能力一个没动，「进阶 / App 版」页仍可手动装，
    // 存量已装的客户升级上来照常识别、照常切驱动。
    // ★ 2026-08-05 定案：**主推四件 = Claude Code / Hermes / Codex CLI / pi**，
    // ClawX 备选（不主推不主装，在「进阶 / App 版」页可装 —— 它的独有价值是
    // 图形界面 + 微信等 IM 接入）。qwen / crush / opencode / openclaw CLI 已撤出露出面
    // （能力全保留、已装的照配，只是不再推荐；判据见 apps.ts 各自的 hidden 注释）。
    //
    // 🔴 **「主推」≠「默认装」，别把这两个数对不上当成 bug**：主推四件说的是
    // Dock 里露出、点了能一键装；这里的默认装队列仍是**三件套**（Claude Code / pi / Hermes），
    // Codex 要用户自己点。理由是下面那条「能力不重叠」—— 默认装每多一件，客户就多等一程，
    // 而 Codex 是全场最慢的一个（实测 35~52s，pi 7.1s / claude 9.6s），不值得挡在首次体验前面。
    // ★ 2026-08-03：一键装从「5 个」收窄到**三件套**（Claude Code / pi / Hermes）。
    //
    // 收窄的判据不是"少就是好"，是**能力不重叠**：claude 是能力天花板、pi 同样的模型只花
    // 1/5 的钱（实测同任务上下文 5,000 vs 24,300 token）、Hermes 是唯一有内置记忆的
    //（MEMORY.md/USER.md 常开）。Codex / OpenClaw / Qwen / Crush / OpenCode 一个都没删，
    // 只是不再默认装 —— 工具市场里随时可装，切驱动、竞技场跑分照旧。
    //
    // 🔴 **claude-code 必须排第一**：客户等的是"能开始干活"，不是"全部装完"。第一个装完
    // 他就能进 U-Workspace 说话了，后面两个慢慢补。把最脆的（Hermes 是 pip 系，装机失败
    // 重灾区）排最后，正是为了不让它挡住前面已经能用的东西。
    if (preselect === "all") {
      const queue: string[] = [];
      if (!detect.claude.found) queue.push("claude-code");
      // ★ 环境地基（2026-08-03 定，`env:` 前缀分流到工具箱的 winget 安装）：
      //   · Windows Terminal —— pc-***「满屏乱码」的真因就是它没装（**不是** PowerShell 版本低）。
      //     装上这条客诉直接消失，比在建议引擎里提示他自己去装有效得多。
      //   · PowerShell 7 —— 只有 5.1 时默认 GBK 输出，AI 判断不了命令成没成就会反复重试，
      //     这笔 token 和时间的损耗不出现在任何报错统计里。
      //   · git —— 缺了它 Claude Code 的 Bash 工具基本就废了（pc-*** 实锤）。
      // 便携 Node / Python 不在这儿：它们由 skill 的 ensure_node / ensure_python 按需自动装，
      // 排进来会重复装一遍。排在两件套**后面**：环境是体验，能不能干活先解决。
      // Hermes **已移出默认队列**（见下方那段判据）。工具市场里随时可装。
      // 前三个是「终端能不能好好说话」，后两个是**技能包的硬依赖** ——
      // 不预装的话，llms.txt 里写着能做的事有两件当场就是空头支票：
      //   · LibreOffice → `uking-pdf`「导出 PDF」整条废，老 .doc 预览也看不了
      //   · ffmpeg      → `uking-aigc` 的视频拼接 / 一键成片废（出单条视频不受影响）
      // 体积确实大（LibreOffice ~350MB、ffmpeg ~100MB），所以**排在最后、且走的是
      // env: 那条不挡路的短路径**：装不上就跳过，客户该干的活一件都不耽误。
      //   · MarkItDown → `uking-office-read` 的首选内核。办公九成场景是「我有一份文件，帮我…」，
      //     读不了文件能力就断在第一步。**只装 [docx,pdf,pptx,xlsx] 四个转换器（≈65MB），
      //     不要 [all]**（会拖进 pandas+numpy 91MB 和 azure/youtube/语音转写，办公一个都用不上）。
      //     缺了不致命：read-doc.py 还有 pandoc 兜底，只是多付 33% token、表格差一截。
      // 🔴 LibreOffice **撤出默认装**（我几小时前刚把它加进来，理由已经不成立了）：
      // 当时的判据是「不装则 uking-pdf 整条废」。但 uking-pdf 随后改成了两条引擎 ——
      // `.md`/`.html` 走系统自带的 Edge headless（**零安装、0.39s、中文可搜索**），
      // 只有 Office 文档转 PDF 才要 LibreOffice，而技能自己就写着「AI 写完的报告九成是
      // Markdown，优先走零安装那条」。为一个低频场景给每台客户机装 400MB 不划算，
      // 何况客户机上大多已有 WPS/Office 能自己导 PDF。改成用到时再提示装（工具箱里一直都在）。
      /* 🔴 **默认队列的判据是「claude 跑得起来」，不是「什么都装上」**（2026-08-18 用户定：
         「ffmpeg 是必须的吗，markit 必须吗，以 claude 跑起来为基准才是对的」）。按这条重收：

           留下  env:git            —— Claude Code 的 Bash 工具没它基本就废（pc-*** 实锤）
           留下  env:windows-terminal / env:pwsh
                                    —— 治的是**乱码**（pc-*** 的真因就是 Terminal 没装）和
                                       PowerShell 5.1 的 GBK 输出让 AI 判断不了命令成没成、
                                       反复重试烧 token。都是 winget 轻量装，不是百 MB 级。
           **移出** env:ffmpeg      —— ~100MB，只服务 uking-aigc 的视频拼接
           **移出** env:markitdown  —— ~65MB，只服务 uking-office-read 的首选内核
           **移出** hermes          —— 第二个 AI，claude 跑起来不需要它

         移出的三个**不是删掉**：工具市场里一直都在，用到时再装（同 LibreOffice 那次的处理，
         理由一模一样 —— 为低频场景给每台客户机预装几百 MB 不划算）。
         🔴 移出 hermes 是我按这条判据推的，比用户点名的多一个，理由：它是 pip 系、
         装机失败重灾区（pc-*** 中文用户名那条就是它），而**装机失败占全部 bug 的 49%**。
         把它从「新机第一次」这条路上挪走，直接降低首装失败率。要它回来说一声。 */
      if (IS_WINDOWS) queue.push("env:windows-terminal", "env:pwsh", "env:git");
      push({
        role: "uking",
        text:
          t("好嘞，开始装 👇 只装「让 Claude Code 真能干活」必需的那几样：Claude Code 本体 + 终端环境。") +
          t("全程走国内加速 + 自动验证修复。装完自动接好虾盘云驱动，并当场自检一遍能不能用。"),
      });
      pushUser(t("一键全安装"));
      ctx.current.queue = queue;
      ctx.current.installAllThenXiapan = true;
      // 队列结束自动走 finishInstallAll 接虾盘云 + 送进 U-Workspace。
      return installQueue();
    }

    const needCodexCli = !detect.codex.found;
    const needCodexApp = IS_WINDOWS && !detect.codex_app;
    // Codex 一次装齐：命令行版（AI 互调 `codex exec` 靠它，跨平台）+ 桌面版（仅 Windows）。
    // 二者共用 ~/.codex 驱动配置，装完切一次驱动两边生效。
    const codexTools = [...(needCodexCli ? ["codex"] : []), ...(needCodexApp ? ["codex-app"] : [])];
    const codexLabel =
      codexTools.length === 2 ? t("装 Codex（命令行 + 桌面版）") : needCodexApp ? t("装 Codex 桌面版") : t("装 Codex 命令行版");

    const opts: Choice[] = [];
    if (!detect.claude.found) opts.push({ label: t("装 Claude Code"), value: "claude-code", tone: "gold" });
    if (codexTools.length) opts.push({ label: codexLabel, value: "codex-set" });
    if (!detect.claude.found && codexTools.length) opts.push({ label: t("两个都装"), value: "both", tone: "gold" });
    opts.push({ label: t("跳过安装，直接配驱动"), value: "skip", tone: "plain" });

    if (preselect && TOOL_NAMES[preselect]) {
      const has =
        preselect === "claude-code"
          ? detect.claude.found
          : preselect === "codex"
            ? detect.codex.found
            : false; // openclaw 等：没有专门探测位，直接让用户装
      if (!has) {
        push({ role: "uking", text: t("你在工具市场点了 {tool}，现在就装它？", { tool: t(TOOL_NAMES[preselect]) }) });
        const v = await ask([
          { label: t("安装 {tool}", { tool: t(TOOL_NAMES[preselect]) }), value: preselect, tone: "gold" },
          { label: t("我再想想，看看其他选项"), value: "menu", tone: "plain" },
        ]);
        if (v !== "menu") {
          pushUser(t("安装 {tool}", { tool: t(TOOL_NAMES[v]) }));
          ctx.current.queue = [v];
          return installQueue();
        }
      }
    }

    if (detect.claude.found && detect.codex.found && !needCodexApp) {
      push({ role: "uking", text: t("Claude Code 和 Codex 都已经在了，直接进入驱动配置（改底层 API 指向国内）。") });
      return pickDriver();
    }
    if (detect.claude.found && detect.codex.found && needCodexApp) {
      push({
        role: "uking",
        text: t("Claude Code 和 Codex CLI 都在了。还可以装 Codex 桌面版（图形界面，跟 CLI 共用同一份驱动配置），或直接配驱动。"),
      });
      const v0 = await ask([
        { label: t("装 Codex 桌面版"), value: "codex-app", tone: "gold" },
        { label: t("跳过，直接配驱动"), value: "skip", tone: "plain" },
      ]);
      if (v0 === "skip") {
        pushUser(t("直接配驱动"));
        return pickDriver();
      }
      pushUser(t("装 Codex 桌面版"));
      ctx.current.queue = ["codex-app"];
      return installQueue();
    }

    push({ role: "uking", text: t("想装哪个？我推荐 Claude Code（最强编程 agent），装好后用国内驱动直连，不用翻墙。") });
    const v = await ask(opts);
    if (v === "skip") {
      pushUser(t("跳过安装，直接配驱动"));
      return pickDriver();
    }
    ctx.current.queue =
      v === "both" ? ["claude-code", ...codexTools] : v === "codex-set" ? codexTools : [v];
    pushUser(v === "both" ? t("两个都装") : v === "codex-set" ? codexLabel : t("装 {tool}", { tool: t(TOOL_NAMES[v]) }));
    return installQueue();
  }

  /**
   * Codex 桌面版「自己去下载」兜底引导。
   *
   * 自动装（winget 商店 / 镜像 MSIX）在部分客户机上就是装不上：
   * 老 Windows 没装 App Installer（winget 缺失）、企业策略禁了旁加载、
   * 660MB MSIX 被杀软拦、或网络太慢。这时别让用户卡死在「重试/AI 修复」，
   * 直接给三条稳定的官方/镜像入口 + 文字步骤，让他自己点一下装上。
   *
   * 返回 true 表示用户表示「我已装好」（外层退出该工具的安装循环）。
   */
  async function manualCodexApp(): Promise<boolean> {
    // 先弹出图文教程网页（内嵌在 exe，离线可用；真浏览器里商店链接才能点）
    await invoke("open_codex_guide").catch(() => {});
    push({
      role: "uking",
      text: t(
        "没关系，我已经为你打开「Codex 手动安装教程」网页 📖，照着点几下就能装好。也可以直接用下面的按钮 👇\n\n【方式一·推荐】微软商店：点「打开微软商店」→ 在商店页点【获取 / 安装】，等进度跑完。\n【方式二】商店打不开就点「商店网页版」，用浏览器登录微软账号后点【获取】。\n【方式三】点「下载安装包」直接下安装包（约 664MB），下完在浏览器「下载」里双击它装。\n\n⚠️ 微软商店是后台慢慢装的：进度条跑到 100% 才算装好，刚点完可能这里还检测不到，属正常。",
      ),
    });
    while (true) {
      const v = await ask([
        { label: t("打开微软商店 ⭐"), value: "store", tone: "gold" },
        { label: t("商店网页版"), value: "web" },
        { label: t("下载安装包（664MB）"), value: "msix" },
        { label: t("看教程网页"), value: "guide" },
        { label: t("装好了，重新检测"), value: "recheck", tone: "plain" },
      ]);
      if (v === "store") {
        pushUser(t("打开微软商店"));
        // 先拉商店 App；它没装/协议不响应时浏览器开网页版兜底
        await openUrl(CODEX_APP_LINKS.msStoreApp).catch(async () => {
          await openUrl(CODEX_APP_LINKS.msStoreWeb).catch(() => {});
        });
        push({ role: "uking", text: t("已为你拉起微软商店。在商店页点【获取 / 安装】，等进度条 100% 装完再回来点「装好了，重新检测」。") });
        continue;
      }
      if (v === "web") {
        pushUser(t("商店网页版"));
        await openUrl(CODEX_APP_LINKS.msStoreWeb).catch(() => {});
        push({ role: "uking", text: t("已打开微软商店网页版，点【获取】即可。装完回来点「装好了，重新检测」。") });
        continue;
      }
      if (v === "msix") {
        pushUser(t("下载安装包"));
        await openUrl(CODEX_APP_LINKS.msixMirror).catch(() => {});
        push({
          role: "uking",
          text: t("已开始下载 Codex 安装包（.msix，约 664MB）。下完到浏览器「下载」里双击它，按提示装好后回来点「装好了，重新检测」。"),
        });
        continue;
      }
      if (v === "guide") {
        pushUser(t("看教程网页"));
        await invoke("open_codex_guide").catch(() => {});
        push({ role: "uking", text: t("已重新打开教程网页。") });
        continue;
      }
      // 「装好了，重新检测」：连测几次（商店后台装完有滞后）；还检测不到就引导重启 U-King。
      pushUser(t("装好了，重新检测"));
      setBusy(true);
      let found = false;
      for (let i = 0; i < 3 && !found; i++) {
        const det = await invoke<StackDetect>("detect_stack").catch(() => null);
        if (det?.codex_app) found = true;
        else if (i < 2) await new Promise((r) => setTimeout(r, 1500));
      }
      setBusy(false);
      if (found) {
        if (!ctx.current.installed.includes("codex-app")) ctx.current.installed.push("codex-app");
        push({ role: "uking", text: t("✅ 检测到 Codex 桌面版已装好，继续帮你配驱动。") });
        return true;
      }
      // 没检测到：最常见就是「商店刚装完，需重启 U-King 才认得」——明确告诉用户怎么做
      const v2 = await ask([
        { label: t("我再等等，重新检测"), value: "again", tone: "gold" },
        { label: t("知道了，先跳过"), value: "skip", tone: "plain" },
      ]);
      if (v2 === "skip") {
        pushUser(t("先跳过"));
        push({
          role: "uking",
          text: t(
            "好。如果商店里 Codex 已经装完了，这里还没认出来，多半是需要重启程序刷新 —— 把 U-King 整个关掉（右下角托盘图标 → 退出），再双击桌面/U盘里的 U-King.exe 重新打开，就能识别 Codex 了。",
          ),
        });
        return true;
      }
      pushUser(t("再等等，重新检测"));
      push({
        role: "uking",
        text: t(
          "💡 提示：请先确认商店里 Codex 的进度条已经 100%（商店里按钮变成「打开」就是装完了）。确认装完后还检测不到的话，把 U-King 彻底关掉重开一次即可识别。",
        ),
      });
      // 回到 while 顶部，用户可继续点「装好了，重新检测」
    }
  }

  /**
   * Codex CLI（命令行版）「自己照教程装」兜底引导。
   *
   * npm 装不上的真因通常是网络/代理/optionalDependency 被跳过；新版 @openai/codex
   * 已把平台二进制打进主包，不能再单独装旧 win32 包。这时弹出可复制命令的图文教程，
   * 让用户在终端里逐条敲，或直接走官方二进制兜底。返回 true 表示用户自报装好。
   */
  async function manualCodexCli(): Promise<boolean> {
    await invoke("open_codex_cli_guide").catch(() => {});
    push({
      role: "uking",
      text: t(
        "好，我已经打开「Codex CLI 手动安装教程」网页 📖（含可一键复制的命令，共 4 种方案）。\n\n最稳的做法：在 U-King 左侧栏打开「终端」，把教程里【方式一】的几条命令逐条粘贴回车。关键是主包安装时带上 `--include=optional`，避免平台二进制被 npm 跳过。\n\n💡 如果 npm 怎么都装不上，直接用教程最下面的【方式四 · 免 npm】—— 一条命令下官方现成程序，绕开 Node 和 npm 全部坑，最可靠。\n\n装好后命令行里 `codex --version` 能打印版本号就成了，回来点「装好了，重新检测」。",
      ),
    });
    while (true) {
      const v = await ask([
        { label: t("看教程网页 ⭐"), value: "guide", tone: "gold" },
        { label: t("装好了，重新检测"), value: "recheck", tone: "plain" },
        { label: t("先跳过"), value: "skip", tone: "plain" },
      ]);
      if (v === "guide") {
        pushUser(t("看教程网页"));
        await invoke("open_codex_cli_guide").catch(() => {});
        push({ role: "uking", text: t("已重新打开教程网页。") });
        continue;
      }
      if (v === "skip") {
        pushUser(t("先跳过"));
        return true;
      }
      pushUser(t("装好了，重新检测"));
      setBusy(true);
      const det = await invoke<StackDetect>("detect_stack").catch(() => null);
      setBusy(false);
      if (det?.codex.found) {
        if (!ctx.current.installed.includes("codex")) ctx.current.installed.push("codex");
        push({ role: "uking", text: t("✅ 检测到 Codex CLI 已装好（{ver}），继续帮你配驱动。", { ver: det.codex.version ?? "" }) });
        return true;
      }
      push({
        role: "uking",
        text: t(
          "还没检测到 Codex。请确认在命令行里 `codex --version` 能打印出版本号 —— 若提示找不到或闪退，先按教程重装 `@openai/codex --include=optional`，再不行直接走【免 npm】官方二进制兜底。",
        ),
      });
      // 回 while 顶部可再次重新检测
    }
  }

  /**
   * 通用「看手动安装教程」兜底（Claude Code / OpenClaw / Hermes）。
   *
   * 这几个工具装失败时，弹出对应的图文教程（含可复制命令 + 通用补救），
   * 让用户照着在终端里装。detect_stack 只跟踪 claude，所以 Claude Code 能自动复检；
   * openclaw/hermes 复检靠"重新跑一遍安装流"（install_tool 自带 verify），用户自报装好即重试。
   * 返回 true = 退出该工具的安装循环。
   */
  async function manualGuide(tool: string): Promise<boolean> {
    await invoke("open_install_guide", { tool }).catch(() => {});
    push({
      role: "uking",
      text: t(
        "好，我已打开「{tool} 手动安装教程」网页 📖（含可一键复制的命令）。\n\n建议在 U-King 左侧栏打开「终端」，把教程里的命令逐条粘贴回车。页面里还有「装不上怎么办」通用补救（清代理 / 装 Node / 换源 / 放开脚本策略）。\n\n装好后回来选下面的按钮 👇",
        { tool: t(TOOL_NAMES[tool] ?? tool) },
      ),
    });
    while (true) {
      const v = await ask([
        { label: t("看教程网页 ⭐"), value: "guide", tone: "gold" },
        { label: t("我装好了，重新检测"), value: "recheck", tone: "plain" },
        { label: t("先跳过"), value: "skip", tone: "plain" },
      ]);
      if (v === "guide") {
        pushUser(t("看教程网页"));
        await invoke("open_install_guide", { tool }).catch(() => {});
        push({ role: "uking", text: t("已重新打开教程网页。") });
        continue;
      }
      if (v === "skip") {
        pushUser(t("先跳过"));
        return true;
      }
      // 重新检测：Claude Code 能直接探；openclaw/hermes detect_stack 不跟踪，回安装循环重跑 verify。
      pushUser(t("我装好了，重新检测"));
      if (tool === "claude-code") {
        setBusy(true);
        const det = await invoke<StackDetect>("detect_stack").catch(() => null);
        setBusy(false);
        if (det?.claude.found) {
          if (!ctx.current.installed.includes("claude-code")) ctx.current.installed.push("claude-code");
          push({ role: "uking", text: t("✅ 检测到 Claude Code 已装好（{ver}），继续帮你配驱动。", { ver: det.claude.version ?? "" }) });
          return true;
        }
        push({
          role: "uking",
          text: t("还没检测到。请确认命令行里 `claude --version` 能打印版本号；若报「禁止运行脚本」，按教程放开一次 PowerShell 策略。"),
        });
        continue;
      }
      // openclaw / hermes：让外层安装循环重跑一遍（带 verify），相当于"重试"
      push({ role: "uking", text: t("好，我再跑一遍安装验证看看是否已就绪…") });
      return false; // false → 不退出循环，回到 while 顶部重装+验证
    }
  }

  async function installQueue() {
    for (const tool of ctx.current.queue) {
      // ★ `env:` 前缀 = 环境地基（Windows Terminal / PowerShell 7 / git），走工具箱的 winget 安装。
      // **单独一条短路径，不进下面那套「AI 诊断 → 修复 → 重试」循环**：这些是体验增强，
      // 装不上就明说、继续下一个。为一个 git 没装成把客户堵在向导里，比没装 git 更糟。
      if (tool.startsWith("env:")) {
        const id = tool.slice(4);
        push({ role: "uking", text: t("正在配环境：{tool}（走系统包管理器，装不上会自动跳过）…", { tool: ENV_NAMES[id] ?? id }) });
        setBusy(true);
        const r = await invoke<string>("install_capability_tool", { id }).catch((e) => `__ERR__${e}`);
        setBusy(false);
        push({
          role: "uking",
          text: r.startsWith("__ERR__")
            ? t("⚠️ {tool} 没装上（不影响使用，之后可在「厨具工具箱」里再装）：{err}", { tool: ENV_NAMES[id] ?? id, err: r.slice(7) })
            : t("✅ {tool} 就绪。", { tool: ENV_NAMES[id] ?? id }),
        });
        continue;
      }
      let ok = false;
      let aiRounds = 0;
      while (!ok) {
        push({
          role: "uking",
          text:
            tool === "codex-app"
              ? t("开始安装 Codex 桌面版（微软商店渠道，不通自动切国内镜像，装完自动验证）…")
              : t("开始安装 {tool}（走 npmmirror 国内加速，装完自动验证）…", { tool: t(TOOL_NAMES[tool]) }),
        });
        const logId = push({ role: "uking", log: [], logDone: false });
        ctx.current.logMsgId = logId;
        setBusy(true);
        const r = await invoke<InstallToolResult>("install_tool", { toolId: tool }).catch(
          (e): InstallToolResult => ({ ok: false, tool, version: null, attempts: 0, error: String(e) }),
        );
        setBusy(false);
        ctx.current.logMsgId = null;
        patch(logId, (m) => ({ ...m, logDone: true, logOk: r.ok }));

        if (r.ok) {
          ok = true;
          ctx.current.installed.push(tool);
          // 装完立即让 App 层的 Dock/工具列表刷新一次，不等整条向导流程（选驱动+连通测试）
          // 走到最后的 finish() 才刷新 —— 否则用户装完就切 tab / 中途放弃驱动配置，
          // 主界面会一直显示「未安装」直到重启 App（"装了但检测不到" 的真因）。
          onFinished?.();
          const detail = (r.version ?? t("已验证")) + (r.attempts > 1 ? t("，经过一轮自动修复") : "");
          push({
            role: "uking",
            text: t("✅ {tool} 安装成功（{detail}）。", { tool: t(TOOL_NAMES[tool]), detail }),
          });
          if (tool === "codex-app") {
            push({
              role: "uking",
              text: t("提示：Codex 桌面版默认英文。在它的 Settings → Language 选「简体中文」可切中文，但该功能受 OpenAI 灰度控制，部分账号暂时只能英文（这是 OpenAI 侧的问题，非装机失败）。"),
            });
          }
        } else {
          push({ role: "uking", text: t("❌ {tool} 没装上：{err}", { tool: t(TOOL_NAMES[tool]), err: r.error ?? t("未知错误") }) });
          const opts: Choice[] = [];
          // Codex 桌面版（商店/MSIX 渠道）失败时，「自己去下载」往往比 AI 修复更靠谱
          // （winget 缺失 / 旁加载被禁 / 杀软拦 MSIX，自动装绕不过去），所以排在最前并置顶推荐。
          if (tool === "codex-app") {
            opts.push({ label: t("我自己去下载装 ⭐"), value: "manual", tone: "gold" });
          }
          // Codex CLI 失败：平台二进制被镜像/代理跳过是常见死因，给可复制命令的图文教程兜底。
          if (tool === "codex") {
            opts.push({ label: t("照教程手动装 ⭐"), value: "manual-cli", tone: "gold" });
          }
          // 其它工具（Claude Code / OpenClaw / Hermes）失败：给对应的图文手动教程（含可复制命令 + 通用补救）。
          if (tool === "claude-code" || tool === "openclaw" || tool === "hermes") {
            opts.push({ label: t("看手动安装教程 ⭐"), value: "manual-guide", tone: "gold" });
          }
          if (aiRounds < 3) {
            opts.push({
              label: aiRounds === 0 ? t("AI 智能修复") : t("AI 再修一轮（{n}/3）", { n: aiRounds + 1 }),
              value: "ai",
              tone: tool === "codex-app" ? undefined : "gold",
            });
          }
          opts.push(
            { label: t("修复环境并重试"), value: "envfix" },
            { label: t("直接重试"), value: "retry" },
            { label: t("跳过它，继续后面的"), value: "skip", tone: "plain" },
          );
          const v = await ask(opts);
          if (v === "skip") {
            pushUser(t("跳过"));
            break;
          }
          if (v === "manual") {
            pushUser(t("我自己去下载装"));
            await manualCodexApp();
            break; // 手动引导给完即退出该工具循环，不再自动重试
          }
          if (v === "manual-cli") {
            pushUser(t("照教程手动装"));
            await manualCodexCli();
            break;
          }
          if (v === "manual-guide") {
            pushUser(t("看手动安装教程"));
            // 返回 true=用户自报装好/跳过→退出循环；false=回到顶部重跑安装+验证（openclaw/hermes）
            if (await manualGuide(tool)) break;
            else continue;
          }
          if (v === "envfix") {
            // 环境预检+免提权自动修（PATH 丢 System32 等），说清修了什么再自动重装
            pushUser(t("修复环境并重试"));
            const pre = await invoke<{
              ok: boolean;
              issues: string[];
              fixed: string[];
              warnings?: string[];
              repairable?: string[];
            }>("env_precheck").catch(() => ({
              ok: false,
              issues: [t("环境预检执行异常")],
              fixed: [] as string[],
              warnings: [] as string[],
              repairable: [] as string[],
            }));
            const warnings = pre.warnings ?? [];
            if (pre.fixed.length) push({ role: "uking", text: t("已自动修复：{list}", { list: pre.fixed.join("；") }) });
            if (pre.issues.length) push({ role: "uking", text: t("仍需注意：{list}", { list: pre.issues.join("；") }) });
            if (warnings.length) push({ role: "uking", text: t("环境注意：{list}", { list: warnings.join("；") }) });
            if (!pre.fixed.length && !pre.issues.length && !warnings.length)
              push({ role: "uking", text: t("环境检查没发现问题，直接重试安装。") });

            // 🔴 长路径没开是装机失败存量里的第 2 大桶（23 台）。**我们检测得到、也一直有能力修**
            //    （`airuntime_fix_elevated` = 一次 UAC 开长路径 + 开发者模式，已 journal 可回滚），
            //    以前却只吐一行「以管理员运行 reg add …」把活推回给客户 —— 等于没检测。
            //    这里不 Rust 侧互相 import（守四铁律），由前端把两个现成 command 组合起来。
            //    按后端给的稳定 id 判断，**不按文案匹配** —— 文案会被 i18n 翻掉。
            if ((pre.repairable ?? []).includes("long_paths")) {
              const go = await ask([
                { label: t("帮我开启长路径（需要管理员）"), value: "fixlp", tone: "gold" },
                { label: t("先不开，继续装"), value: "nolp", tone: "plain" },
              ]);
              if (go === "fixlp") {
                pushUser(t("帮我开启长路径"));
                setBusy(true);
                // 说清它到底改什么 —— 顺带开开发者模式是 ukrt fix 的既有行为，不能藏着。
                push({
                  role: "uking",
                  text: t("正在开启长路径支持（同时会开启开发者模式，两项都记进 journal 可回滚）。请在弹出的窗口点「是」…"),
                });
                const r = await invoke<string>("airuntime_fix_elevated").catch((e) => String(e));
                setBusy(false);
                push({ role: "uking", text: String(r) });
                // 只报「已开启」不够 —— 复检一次，把「真开了没」摆出来。
                const after = await invoke<{ repairable?: string[] }>("env_precheck").catch(() => ({
                  repairable: [] as string[],
                }));
                if ((after.repairable ?? []).includes("long_paths")) {
                  push({
                    role: "uking",
                    text: t("复检：长路径仍显示未开启 —— 可能是授权被取消，或该策略被公司域策略锁住。可右键 U-King 以管理员身份运行后重试。"),
                  });
                } else {
                  push({
                    role: "uking",
                    text: t("复检：长路径已开启。若装依赖仍报路径过长，重启一次电脑再装。"),
                  });
                }
              } else {
                pushUser(t("先不开"));
              }
            }
            // 落回 while 顶部自动重装验证
          } else if (v === "ai") {
            pushUser(t("AI 智能修复"));
            aiRounds += 1;
            await aiRepair(tool);
            // 修复后回到 while 顶部自动重装验证
          } else {
            pushUser(t("重试"));
          }
        }
      }
    }
    // 「一键全安装」：队列装完后自动接虾盘云 + 铺技能包，不再让用户手动选驱动。
    // （原注释写着「弹 ClawX 下载」—— 那是 0.9.85 之前的流程，ClawX 早已移出默认队列，
    //   finishInstallAll 里一个字都没有。留着会让人以为这里还在推 261MB 的下载。）
    if (ctx.current.installAllThenXiapan) {
      return finishInstallAll();
    }
    // 单独安装 DSH：安装完就把这台机器的虾盘云 Key 写进 DSH 原生
    // settings/credentials，Web + terminal 共用。这是用户主动点「安装 DSH」的收尾，
    // 不去顺手改 Claude/Codex 等其它工具。
    if (ctx.current.queue.length === 1 && ctx.current.queue[0] === "dsh") {
      push({ role: "uking", text: t("DeepSeek Harness 已装好，正在用本机专属 Key 接入虾盘云…") });
      setBusy(true);
      const dk = await invoke<DeviceKey>("get_device_key").catch(() => null);
      const error = dk
        ? await invoke("apply_provider", {
            providerId: "xiapan",
            apiKey: dk.key,
            model: null,
            targets: ["dsh"],
          }).then(() => null).catch((e) => String(e))
        : t("未能生成本机专属 Key");
      setBusy(false);
      if (dk) ctx.current.deviceKey = dk;
      push({
        role: "uking",
        text: error
          ? t("DSH 已安装，但虾盘云自动配置失败：{error}。可在「我的 AI → DeepSeek Harness」右侧重试。", { error })
          : t("✅ DSH 已接好虾盘云！Web 工作台和终端模式都能直接用，不需要再申请或填写 DeepSeek API Key。") +
            (dk?.charged
              ? t("（内置 Key 余额 {bal}）", { bal: dk.balance?.text ?? "" })
              : t("（内置 Key 余额为 0，使用前到「AI 设置」充值即可）")),
      });
      return finish();
    }
    // Harness Doctor 是只读诊断工具，不需要模型驱动；安装后回到首页即可生成更深的体检报告。
    if (ctx.current.queue.length === 1 && ctx.current.queue[0] === "harness-doctor") {
      push({ role: "uking", text: t("Harness Doctor 已装好。回到「我的 AI」点它可立即体检；之后生成 AI 体检报告时也会自动附上四个 Harness 的诊断摘要。") });
      return finish();
    }
    return pickDriver();
  }

  /* ClawX 的静默安装步骤（installClawXStep）已从「一键全安装」删除（0.9.85）——
   * 同一份能力在 App.tsx 的工具卡片点击路径里有更完整的一份（下载进度 + 失败回退图文教程 +
   * 装完不擅自切驱动），客户从「进阶 / App 版」或工具卡片点它照样能装。留两份只会漂移。 */

  /** 「一键全安装」收尾：用设备内置 Key 一次把虾盘云接进全部已装工具。
   *  `apply_xiapan_everywhere` 后端自探已装工具（不信前端列表）—— 所以客户如果**自己**装过
   *  ClawX，这一步照样会把它配上，跟我们默认装不装它无关。 */
  async function finishInstallAll(): Promise<void> {
    push({ role: "uking", text: t("工具都装好了，正在自动接入虾盘云驱动（用本机专属 Key，无需配置）…") });
    setBusy(true);
    // 一键配好全部：后端自己探测装了哪些工具（不信前端 installed 列表），把虾盘云 Key 一次
    // 写进全部。这是用户主动选「一键全安装」的收尾，属明确同意。
    await invoke("apply_xiapan_everywhere", { providerId: "xiapan", apiKey: null, model: null }).catch(() => {});
    // 打通「AI 之间互相调用」：把 uking-teamwork 技能包铺进各 AI 的 skills 目录（后端自探已装工具）。
    // 装好后 Hermes / Claude Code 等用同一个 Key 就能调 `claude -p`、`codex exec` 分工协作，
    // 不必客户自己去「AI 技能包」页手动点。best-effort，失败不影响主流程。
    await invoke("install_skill_pack").catch(() => {});
    const dk = await invoke<DeviceKey>("get_device_key").catch(() => null);
    setBusy(false);
    if (dk) ctx.current.deviceKey = dk;
    push({
      role: "uking",
      text:
        t("✅ 虾盘云已接好！Claude Code / Codex / Hermes / DSH 现在都国内直连，默认用 DeepSeek V4 Flash（最快最省）。") +
        (dk?.charged
          ? t("（内置 Key 余额 {bal}）", { bal: dk.balance?.text ?? "" })
          : t("（内置 Key 余额为 0，首次使用前去「AI 设置」充值即可，¥20 起充，¥1=50 万 token）")),
    });
    push({
      role: "uking",
      text: t(
        "🤝 已打通「AI 协同」：这些 AI 共用同一个 Key，可以互相调用分工 —— 比如让 Hermes 或 Claude Code 去调 `claude -p`、`codex exec` 把大任务拆开做、再汇总。在任意一个 AI 里说「用 U-King 多 AI 协同帮我…」，它就会照着技能包（uking-teamwork）分工。",
      ),
    });
    // 🔴 **收尾自检**（2026-08-18 用户要的那一步）。装机链路到这里为止说的全是
    //    「装了什么」；这一句回答的是**能不能用**。两件事差得远 —— CLAUDE.md 里的原话
    //    就是这个场景：形状全对、报告全绿，而客户开了两天一点没省。
    //    全绿只说一句；有问题就**逐条说清缺什么、怎么补**，别让他自己猜。
    await runReadinessCheck();
    return finish();
  }

  /** 跑一次「现在能不能用」并把结论说成人话。失败不阻断收尾 —— 自检本身挂了
   *  不该把已经装好的流程也拖住（但要说一声，静默跳过等于假装检查过了）。 */
  async function runReadinessCheck(): Promise<void> {
    setBusy(true);
    type Check = { name: string; ok: boolean; detail: string; fix: string };
    type Readiness = { ready?: boolean; checks?: Check[] };
    let r: Readiness | null = null;
    try {
      const env = await callAction(ACTION.RUNTIME_READINESS_INSPECT, {});
      r = (env.ok ? (env.result as unknown as Readiness) : null);
    } catch {
      r = null;
    }
    setBusy(false);
    if (!r?.checks?.length) {
      push({ role: "uking", text: t("（自检没跑成，不影响使用；到「首页 · 我的 AI」可以再点一次。）") });
      return;
    }
    const bad = r.checks.filter((c) => !c.ok);
    if (bad.length === 0) {
      push({ role: "uking", text: t("🔎 自检通过：Claude Code 真跑得起来、驱动已接、余额可用、技能包已就位 —— 可以去 U-Workspace 干活了。") });
      return;
    }
    push({
      role: "uking",
      text: [
        t("🔎 自检发现 {n} 件事还没到位（其余都好了）：", { n: bad.length }),
        ...bad.map((c) => `· ${t(c.name)}：${c.detail}\n  → ${c.fix}`),
      ].join("\n"),
    });
  }

  /** 装完的落点（0.9.85）：把人送进 U-Workspace，并把「怎么用终端」说成三步。
   *
   *  以前这里是「打开 ClawX 直接用」——等于把刚装好的客户推去另一个应用，U-King 自己反而
   *  成了装机器。现在主推顺序是：① 最好 → 工作台里开终端跑 `claude`（能力最全）；
   *  ② 兜底 → 就在 U-Chat 对话框里干活（不碰终端，同一个 Claude Code + 同一个 Key）。
   *  两条路的模型和计费完全一样，区别只是「敢不敢看见黑框」。 */
  async function offerWorkspace(): Promise<void> {
    push({
      role: "uking",
      text: t(
        "接下来去哪干活？推荐 U-Workspace（U-King 自带的工作台，对话 + 终端 + 作图都在一个界面里）：\n" +
          "· 想要最全的能力 → 工作台右上角点「终端」，输入 claude 回车，就是完整的 Claude Code；\n" +
          "· 不想碰黑乎乎的终端 → 直接在对话框里说人话就行，底下跑的是同一个 Claude Code、同一个 Key。",
      ),
    });
    if (!onGoWorkspace) return;
    const v = await ask([
      { label: t("进 U-Workspace 开始干活"), value: "go", tone: "gold" },
      { label: t("先不用，我自己逛逛"), value: "stay", tone: "plain" },
    ]);
    if (v === "stay") {
      pushUser(t("先这样"));
      return;
    }
    pushUser(t("进 U-Workspace"));
    onGoWorkspace();
  }

  /* ---------- AI 修复（虾盘云 API 直连大脑，不依赖已装工具） ---------- */

  /** AI 修复要烧 token：优先用户已填的 Key，否则用设备内置 Key（没充值就引导充值）。 */
  async function resolveRepairKey(): Promise<string | null> {
    if (ctx.current.apiKey) return ctx.current.apiKey;
    setBusy(true);
    const dk = await invoke<DeviceKey>("get_device_key").catch(() => null);
    setBusy(false);
    if (!dk) {
      push({ role: "uking", text: t("拿不到设备内置 Key，先跳过 AI 修复（可以选「直接重试」）。") });
      return null;
    }
    ctx.current.deviceKey = dk;
    if (dk.charged) return dk.key;

    push({
      role: "uking",
      text: t("AI 修复需要烧一点 token。U-King 已为这台电脑生成专属虾盘云 Key：{key}（硬件指纹，恒定不变），目前余额为 0 —— 充值即开通，¥20 起充，¥1 = 50 万 token，修一次只要几千 token。", { key: short(dk.key) }),
    });
    while (true) {
      const v = await ask([
        { label: t("去充值（打开充值页）"), value: "open", tone: "gold" },
        { label: t("我已充值，查余额"), value: "check" },
        { label: t("跳过 AI 修复"), value: "skip", tone: "plain" },
      ]);
      if (v === "skip") {
        pushUser(t("跳过 AI 修复"));
        return null;
      }
      if (v === "open") {
        pushUser(t("去充值"));
        await openRecharge(dk.recharge_url);
        continue;
      }
      pushUser(t("查余额"));
      setBusy(true);
      const dk2 = await invoke<DeviceKey>("get_device_key").catch(() => null);
      setBusy(false);
      if (dk2?.charged) {
        push({ role: "uking", text: t("到账了！余额 {bal}。", { bal: dk2.balance?.text ?? "" }) });
        ctx.current.deviceKey = dk2;
        return dk2.key;
      }
      push({ role: "uking", text: t("还没查到余额（到账一般几秒到几分钟），稍后再点「查余额」。") });
    }
  }

  async function aiRepair(tool: string): Promise<void> {
    const key = await resolveRepairKey();
    if (!key) return;

    push({ role: "uking", text: t("🩺 AI 诊断中（虾盘云直连，即使 Claude 没装好也能修）…") });
    setBusy(true);
    const context =
      `失败工具: ${TOOL_NAMES[tool]}\n` +
      `环境体检: ${JSON.stringify(ctx.current.detect)}\n` +
      `安装日志尾部:\n${ctx.current.lastLog.slice(-60).join("\n")}`;
    const d = await invoke<Diagnosis>("ai_diagnose", { apiKey: key, context }).catch((e) => {
      push({ role: "uking", text: t("AI 诊断失败：{err}", { err: String(e) }) });
      return null;
    });
    setBusy(false);
    if (!d) return;

    push({
      role: "uking",
      text:
        t("AI 诊断：{diag}", { diag: d.diagnosis }) +
        (d.commands.length
          ? t("\n\n建议执行 {n} 条修复命令（执行前请过目）：\n", { n: d.commands.length }) +
            d.commands.map((c, i) => `${i + 1}. ${c}`).join("\n")
          : t("\n\n（没有可自动执行的修复命令，请按上面说明手动处理后再重试）")),
    });
    if (!d.commands.length) return;

    const v = await ask([
      { label: t("执行这 {n} 条修复命令", { n: d.commands.length }), value: "go", tone: "gold" },
      { label: t("不执行"), value: "no", tone: "plain" },
    ]);
    if (v !== "go") {
      pushUser(t("不执行"));
      return;
    }
    pushUser(t("执行修复"));

    const logId = push({ role: "uking", log: [], logDone: false });
    ctx.current.logMsgId = logId;
    setBusy(true);
    let allOk = true;
    for (const cmd of d.commands) {
      const ran = await invoke("run_fix", { toolId: tool, cmd })
        .then(() => true)
        .catch((e) => {
          patch(logId, (m) => ({ ...m, log: [...(m.log ?? []), `✗ ${e}`] }));
          return false;
        });
      if (!ran) {
        allOk = false;
        break;
      }
    }
    setBusy(false);
    ctx.current.logMsgId = null;
    patch(logId, (m) => ({ ...m, logDone: true, logOk: allOk }));
    push({
      role: "uking",
      text: allOk ? t("修复命令执行完毕，自动重装验证…") : t("有修复命令执行失败（已停止），仍会重装验证一次…"),
    });
  }

  async function pickDriver(): Promise<void> {
    const ps = await invoke<ProviderPreset[]>("list_providers").catch(() => [] as ProviderPreset[]);
    // 用户可以把内置驱动（含虾盘云）从列表里移除 —— 移除了就别再推荐它，
    // 否则文案指着一个选项里根本没有的东西，等于我们嘴上说不抢、话术还在抢。
    const hasXiapan = ps.some((p) => p.id === "xiapan");
    push({
      role: "uking",
      text: hasXiapan
        ? t("现在选底层驱动（大模型 API）。推荐虾盘云：U-King 内置，国内直连、充值即用；也可以用你自己的 DeepSeek / GLM / Kimi Key。")
        : t("现在选底层驱动（大模型 API）。用你自己的 Key 即可；想用内置的虾盘云，可以到「AI 设置」把它加回列表。"),
    });
    const opts: Choice[] = ps
      .filter((p) => p.id !== "official")
      .map((p) => ({
        label: p.recommended ? `${p.name} ⭐` : p.name,
        value: p.id,
        tone: p.recommended ? "gold" : undefined,
      }));
    opts.push({ label: t("还原官方直连"), value: "official", tone: "plain" });
    const v = await ask(opts);
    const p = ps.find((x) => x.id === v)!;
    ctx.current.provider = p;
    setActiveProvider(p);
    pushUser(p.name);

    if (p.id === "official") {
      setBusy(true);
      const targets = targetsFromInstalled();
      // 别再「无论成没成都说已清除」——还原失败却报喜，用户回头发现照旧走我们的配置，
      // 而我们这边什么记录都没有。失败就说失败，并且上报。
      const err = await invoke("apply_provider", { providerId: "official", apiKey: "-", model: null, targets })
        .then(() => null)
        .catch((e) => String(e));
      setBusy(false);
      if (err) {
        push({ role: "uking", text: t("写配置失败：{err}", { err }) });
        invoke("report_bug", {
          kind: "driver_apply_failed",
          summary: `还原官方直连失败: ${err}`.slice(0, 200),
          detail: `provider=official\ntargets=${targets.join(",")}\nerror=${err}`,
        }).catch(() => {});
        return retryDriver();
      }
      push({ role: "uking", text: t("已清除 U-King 写入的驱动配置，Claude Code / Codex 还原为官方登录。") });
      return finish();
    }

    // 虾盘云：默认走设备内置 Key（不送 token，充值即开通）
    if (p.builtin_recharge) {
      setBusy(true);
      const dk = await invoke<DeviceKey>("get_device_key").catch(() => null);
      setBusy(false);
      if (dk) {
        ctx.current.deviceKey = dk;
        if (dk.charged) {
          push({
            role: "uking",
            text: t("这台电脑已有 U-King 专属 Key：{key}（硬件指纹生成，重装系统前恒定），余额 {bal}。直接用它？", { key: short(dk.key), bal: dk.balance?.text ?? "" }),
          });
          const v = await ask([
            { label: t("用内置 Key（余额 {bal}）", { bal: dk.balance?.text ?? "" }), value: "device", tone: "gold" },
            { label: t("换我自己的 Key"), value: "own", tone: "plain" },
          ]);
          if (v === "device") {
            pushUser(t("用内置 Key"));
            ctx.current.apiKey = dk.key;
            return applyAndTest();
          }
          pushUser(t("用自己的 Key"));
        } else {
          push({
            role: "uking",
            text: t("U-King 已为这台电脑生成专属虾盘云 Key：{key}（硬件指纹，无需注册，已保存在本机 ~/.uking/device.json，重装系统前可备份）。默认余额为 0，充值即开通 —— ¥20 起充，¥1 = 50 万 token，驱动是 DeepSeek-V4 Pro 满血版。", { key: short(dk.key) }),
          });
          let useOwn = false;
          while (!useOwn) {
            const v = await ask([
              { label: t("去充值（打开充值页，已带 Key）"), value: "open", tone: "gold" },
              { label: t("我已充值，查余额"), value: "check" },
              { label: t("用我自己的 Key"), value: "own", tone: "plain" },
            ]);
            if (v === "own") {
              pushUser(t("用自己的 Key"));
              useOwn = true;
              break;
            }
            if (v === "open") {
              pushUser(t("去充值"));
              await openRecharge(dk.recharge_url);
              continue;
            }
            pushUser(t("查余额"));
            setBusy(true);
            const dk2 = await invoke<DeviceKey>("get_device_key").catch(() => null);
            setBusy(false);
            if (dk2?.charged) {
              push({ role: "uking", text: t("到账！余额 {bal}。用内置 Key 继续。", { bal: dk2.balance?.text ?? "" }) });
              ctx.current.deviceKey = dk2;
              ctx.current.apiKey = dk2.key;
              return applyAndTest();
            }
            push({ role: "uking", text: t("还没查到余额（到账一般几秒到几分钟），充值完稍等再点「查余额」。") });
          }
        }
      }
    }

    push({
      role: "uking",
      text: p.builtin_recharge
        ? t("好，把你的虾盘云 Key 粘贴进来（{hint}）。", { hint: p.key_hint })
        : t("好，用 {name}。把你的 API Key 粘贴进来（{hint}）。没有的话点下面按钮去申请。", { name: p.name, hint: p.key_hint }),
    });
    const key = await askKey();
    ctx.current.apiKey = key;
    pushUser(key.length > 12 ? `${key.slice(0, 8)}…${key.slice(-4)}` : key);
    return applyAndTest();
  }

  function targetsFromInstalled(): string[] {
    const t: string[] = [];
    if (ctx.current.installed.includes("claude-code")) t.push("claude");
    // Codex CLI 和桌面版共用 ~/.codex 配置，装了任意一个都要写
    if (ctx.current.installed.includes("codex") || ctx.current.installed.includes("codex-app")) t.push("codex");
    if (ctx.current.installed.includes("openclaw") || ctx.current.installed.includes("clawx")) t.push("clawx");
    if (ctx.current.installed.includes("hermes")) t.push("hermes");
    if (ctx.current.installed.includes("dsh")) t.push("dsh");
    if (t.length === 0) t.push("claude"); // 都没装也先把 Claude 配置写好，装完即用
    return t;
  }

  async function applyAndTest(): Promise<void> {
    const p = ctx.current.provider!;
    const key = ctx.current.apiKey;
    const targets = targetsFromInstalled();

    // 纯 OpenAI 兼容的供应商配不了 Claude Code（它只认 Anthropic 接口）——**能力不匹配，
    // 不是失败**。后端多目标时会跳过 claude 把别的配好；这里同步口径，别再对着一个注定
    // 配不上的工具喊「正在写入」、也别去跑一条注定失败的 Anthropic 实测（issue #359/#322：
    // 客户挑了火山方舟 / 一个只有 OpenAI 端点的中转，整条向导卡在最后一米反复打转）。
    const claudeOk = Boolean(p.anthropic_base?.trim());
    const willWrite = targets.filter((tg) => tg !== "claude" || claudeOk);
    if (targets.includes("claude") && !claudeOk) {
      push({
        role: "uking",
        text: t("提醒：{name} 只提供 OpenAI 兼容接口，而 Claude Code 只认 Anthropic 接口 —— 这一个配不了，先跳过（想用 Claude Code 就换一个带 Anthropic 端点的驱动，比如虾盘云）。", { name: p.name }),
      });
    }
    // 一个都写不了（机器上只认 Claude Code、偏偏这个驱动配不了它）：说清楚 + 让用户换驱动。
    // **不上报 bug** —— 能力不匹配是事实，不是故障；报了只会把 bug 仓库灌满噪音。
    if (willWrite.length === 0) {
      push({
        role: "uking",
        text: t("这台机器目前只需要配 Claude Code，而它配不了 —— 换一个带 Anthropic 端点的驱动，或者先装上 Codex CLI 再回来用这个。"),
      });
      setBusy(false);
      return retryDriver();
    }

    setBusy(true);
    push({ role: "uking", text: t("写入底层配置（{list}）…", { list: willWrite.map((tg) => TARGET_LABELS[tg] ?? tg).join(" + ") }) });
    const applied = await invoke<{ claude: string | null; codex: string | null }>("apply_provider", {
      providerId: p.id,
      apiKey: key,
      model: null,
      // 传过滤后的 targets：已经告诉用户跳过了，就别再让后端去试一次
      targets: willWrite,
    }).catch((e) => {
      push({ role: "uking", text: t("写配置失败：{err}", { err: String(e) }) });
      // 必须上报：这一步是首装主路径的最后一米，挂在这里等于「装了用不了」。
      // 教训（0.9.70~0.9.72）—— 核心校验把 `model: null` 判成类型错，全量客户首装
      // 100% 卡在这句话上，而 bug 仓库里一条记录都没有：只在气泡里显示的错，
      // 对我们等于没发生，只能靠客户拍照片告诉我们。
      invoke("report_bug", {
        kind: "driver_apply_failed",
        summary: `写入驱动配置失败 provider=${p.id}: ${String(e)}`.slice(0, 200),
        // 报**实际尝试写入**的 targets（不是探测到的全集）—— 否则 triage 会对着一个我们
        // 压根没写的目标找原因（#359 就是这么读岔的：报 claude,codex，其实 claude 是能力不匹配）。
        detail: `provider=${p.id}\ntargets=${willWrite.join(",")}\nhasKey=${Boolean(key)}\nerror=${String(e)}`,
      }).catch(() => {});
      return null;
    });
    if (!applied) {
      setBusy(false);
      return retryDriver();
    }

    push({ role: "uking", text: t("配置已写入。现在实测连通 —— 让模型真实回一句话…") });
    const tests: { label: string; r: TestResult }[] = [];
    if (targets.includes("claude") && claudeOk) {
      const r = await invoke<TestResult>("test_provider", { providerId: p.id, apiKey: key, model: null, api: "anthropic" });
      tests.push({ label: t("Claude Code 链路（Anthropic 格式）"), r });
    }
    if (targets.includes("codex")) {
      const r = await invoke<TestResult>("test_provider", { providerId: p.id, apiKey: key, model: null, api: "openai" });
      tests.push({ label: t("Codex 链路（OpenAI 格式）"), r });
    }

    let balance: Balance | null = null;
    if (p.builtin_recharge) {
      balance = await invoke<Balance>("query_balance", { apiKey: key }).catch(() => null);
    }
    setBusy(false);
    push({ role: "uking", tests, balance });

    const allOk = tests.every((x) => x.r.ok);
    if (allOk) {
      push({ role: "uking", text: t("🎉 全部链路打通！驱动已生效（配置热更新，新开终端即可用）。") });
      return finish();
    }
    push({ role: "uking", text: t("有链路没通。常见原因：Key 没充值 / 模型名不对 / 网络波动。") });
    return retryDriver();
  }

  async function retryDriver(): Promise<void> {
    const v = await ask([
      { label: t("重新输入 Key"), value: "rekey", tone: "gold" },
      { label: t("换一个驱动"), value: "switch" },
      { label: t("先这样，稍后再说"), value: "done", tone: "plain" },
    ]);
    if (v === "rekey") {
      pushUser(t("重新输入 Key"));
      const key = await askKey();
      ctx.current.apiKey = key;
      pushUser(`${key.slice(0, 8)}…`);
      return applyAndTest();
    }
    if (v === "switch") {
      pushUser(t("换一个驱动"));
      return pickDriver();
    }
    pushUser(t("先这样"));
    return finish();
  }

  async function finish() {
    const det = await invoke<StackDetect>("detect_stack").catch(() => null);
    const lines: string[] = [];
    if (det?.claude.found) lines.push(t("· Claude Code：{ver}（终端输入 claude 即可用）", { ver: det.claude.version ?? "" }));
    if (det?.codex.found) lines.push(t("· Codex CLI：{ver}（终端输入 codex 即可用）", { ver: det.codex.version ?? "" }));
    if (det?.codex_app) lines.push(t("· Codex 桌面版：已安装（和 CLI 共用驱动配置，切一次两边生效）"));
    if (det?.portable_node) lines.push(t("· 便携 Node 已装到 ~/.uking/runtime（已写入 PATH，新终端生效）"));
    if (det?.system_proxy)
      lines.push(
        t("⚠️ 检测到系统代理（{proxy}）：claude/codex 会走它。如果上面实测是通的、工具却报连接错误，多半是代理节点失效 —— 把 api.u-claw.org 加进代理的直连名单，或暂时关闭系统代理再试。", { proxy: det.system_proxy }),
      );
    push({
      role: "uking",
      text:
        t("搞定！") +
        (lines.length ? "\n" + lines.join("\n") : "") +
        t("\n以后随时回到这里：换驱动、查余额、修复安装都行。"),
    });
    onFinished?.();
    // 落点引导放在 onFinished 之后：先让外面刷新「已装工具」，再问要不要进工作台，
    // 免得客户点了按钮过去、Dock 图标还是灰的。
    await offerWorkspace();
  }

  function pushUser(text: string) {
    push({ role: "user", text });
  }

  /* ---------- 渲染 ---------- */

  const p = activeProvider;
  return (
    <section className="rounded-card border border-white/[0.10] bg-bg-1/80 backdrop-blur-sm shadow-card flex flex-col overflow-hidden">
      <header className="flex items-center gap-2.5 px-5 h-12 border-b border-white/[0.08] bg-bg-1/70">
        <span className="grid place-items-center w-7 h-7 rounded-lg bg-accent/[0.12]">
          <Wand2 size={15} className="text-accent" />
        </span>
        <span className="text-[14px] font-semibold text-ink-0">{t("对话式安装向导")}</span>
        <span className="text-[10px] font-mono text-ink-4 uppercase tracking-widest">AI Setup</span>
        {busy && <Loader2 size={14} className="animate-spin text-accent ml-auto" />}
      </header>

      <div className="flex-1 overflow-y-auto px-5 py-5 space-y-4 max-h-[430px] min-h-[300px]">
        {msgs.map((m) => (
          <Bubble key={m.id} m={m} />
        ))}

        {/* 选项按钮 */}
        {choices && (
          <div className="flex flex-wrap gap-2.5 pl-12 animate-fade-in">
            {choices.map((c) => (
              <button
                key={c.value}
                onClick={() => ctx.current.onChoice?.(c.value)}
                className={cn(
                  "px-4 h-10 rounded-full text-[12px] font-medium transition-all",
                  c.tone === "gold"
                    ? "bg-accent text-white hover:bg-accent-600 shadow-sm"
                    : c.tone === "plain"
                      ? "border border-white/[0.08] text-ink-4 hover:border-white/[0.16] hover:bg-white/[0.04]"
                      : "border border-white/[0.10] text-ink-1 hover:bg-white/[0.04] hover:border-white/[0.16]",
                )}
              >
                {c.label}
              </button>
            ))}
          </div>
        )}

        {/* Key 输入 */}
        {showKeyInput && (
          <div className="pl-12 space-y-2 animate-fade-in">
            <div className="flex gap-2">
              <div className="flex-1 flex items-center gap-2.5 h-11 rounded-xl border border-white/[0.10] bg-bg-2 px-3.5 shadow-sm">
                <KeyRound size={15} className="text-accent shrink-0" />
                <input
                  value={keyInput}
                  onChange={(e) => setKeyInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && keyInput.trim()) {
                      ctx.current.onKey?.(keyInput.trim());
                      setKeyInput("");
                    }
                  }}
                  placeholder={p?.key_hint || t("粘贴 API Key")}
                  className="flex-1 bg-transparent outline-none text-[13px] text-ink-0 font-mono placeholder:text-ink-4"
                />
              </div>
              <button
                onClick={() => {
                  if (keyInput.trim()) {
                    ctx.current.onKey?.(keyInput.trim());
                    setKeyInput("");
                  }
                }}
                className="px-4 h-11 rounded-xl bg-accent text-white text-[13px] font-semibold inline-flex items-center gap-1.5 hover:bg-accent-600 shadow-sm"
              >
                <Send size={14} />
                {t("确定")}
              </button>
            </div>
            {p && (
              <button
                onClick={() => openUrl(p.key_url).catch(() => {})}
                className="inline-flex items-center gap-1.5 text-[11px] text-accent hover:text-accent-400 transition-colors"
              >
                <ExternalLink size={11} />
                {p.builtin_recharge ? t("打开虾盘云充值页（获取 / 充值 Key）") : t("去 {name} 申请 Key", { name: p.name })}
              </button>
            )}
          </div>
        )}

        <div ref={bottomRef} />
      </div>
    </section>
  );
}

/* ---------------- 气泡 ---------------- */

function Bubble({ m }: { m: Msg }) {
  if (m.role === "user") {
    return (
      <div className="flex justify-end animate-fade-in">
        <div className="max-w-[72%] rounded-2xl rounded-br-lg bg-accent text-white px-4 py-2.5 text-[13px] shadow-sm">
          {m.text}
        </div>
      </div>
    );
  }
  return (
    <div className="flex gap-3 animate-fade-in">
      <span className="shrink-0 grid place-items-center w-8 h-8 rounded-full bg-accent text-white mt-0.5 shadow-sm">
        <Bot size={15} />
      </span>
      <div className="max-w-[82%] space-y-2.5">
        {m.text && (
          <div className="rounded-2xl rounded-tl-lg bg-bg-1 border border-white/[0.08] px-4 py-3 text-[13px] text-ink-1 leading-relaxed whitespace-pre-wrap shadow-sm">
            {m.text}
          </div>
        )}
        {m.detect && <DetectCard d={m.detect} />}
        {m.log && <LogCard m={m} />}
        {m.tests && <TestCard tests={m.tests} balance={m.balance ?? null} />}
      </div>
    </div>
  );
}

function DetectCard({ d }: { d: StackDetect }) {
  const { t } = useI18n();
  const Row = ({ name, probe, extra }: { name: string; probe?: CmdProbe; extra?: boolean }) => {
    const ok = probe ? probe.found : !!extra;
    return (
      <div className="flex items-center gap-2.5 text-[12px] py-0.5">
        {ok ? (
          <span className="grid place-items-center w-5 h-5 rounded-full bg-success-500/12 shrink-0">
            <CheckCircle2 size={12} className="text-success-400" />
          </span>
        ) : (
          <span className="grid place-items-center w-5 h-5 rounded-full bg-white/[0.04] shrink-0">
            <XCircle size={12} className="text-ink-4" />
          </span>
        )}
        <span className="text-ink-1 w-32">{name}</span>
        <span className="font-mono text-[11px] text-ink-3 truncate">
          {probe?.found ? probe.version : ok ? t("已安装") : t("未检测到")}
        </span>
      </div>
    );
  };
  return (
    <div className="rounded-xl border border-white/[0.08] bg-bg-1 px-4 py-3.5 space-y-2 shadow-sm">
      <div className="flex items-center gap-2 text-[11px] text-accent font-medium mb-1.5">
        <span className="grid place-items-center w-5 h-5 rounded-md bg-accent/[0.12]">
          <Stethoscope size={11} />
        </span>
        {t("电脑体检结果")}
      </div>
      <Row name="Node.js" probe={d.node} />
      <Row name="npm" probe={d.npm} />
      <Row name="Claude Code" probe={d.claude} />
      <Row name="Codex CLI" probe={d.codex} />
      <Row name={t("Codex 桌面版")} extra={d.codex_app} />
      <Row name="Git" probe={d.git} />
      <Row name={t("Claude 桌面版")} extra={d.claude_desktop} />
    </div>
  );
}

function LogCard({ m }: { m: Msg }) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [m.log?.length]);
  return (
    <div
      className={cn(
        "rounded-xl border px-4 py-3 shadow-sm",
        m.logDone
          ? m.logOk
            ? "border-success-500/25 bg-success-500/[0.06]"
            : "border-danger-500/25 bg-danger-500/[0.06]"
          : "border-white/[0.08] bg-bg-0/80",
      )}
    >
      <div className="flex items-center gap-2 text-[11px] mb-2">
        {m.logDone ? (
          m.logOk ? (
            <span className="grid place-items-center w-5 h-5 rounded-full bg-success-500/12">
              <CheckCircle2 size={11} className="text-success-400" />
            </span>
          ) : (
            <span className="grid place-items-center w-5 h-5 rounded-full bg-danger-500/12">
              <XCircle size={11} className="text-danger-400" />
            </span>
          )
        ) : (
          <Loader2 size={14} className="animate-spin text-accent" />
        )}
        <span className="text-ink-2 font-medium">{m.logDone ? (m.logOk ? t("安装日志（成功）") : t("安装日志（失败）")) : t("正在安装…")}</span>
      </div>
      <div ref={ref} className="max-h-36 overflow-y-auto font-mono text-[11px] leading-relaxed text-ink-3 space-y-0.5">
        {(m.log ?? []).map((l, i) => (
          <div key={i} className={cn("truncate", l.startsWith("▶") && "text-accent", l.startsWith("✦") && "text-success-400")}>
            {l}
          </div>
        ))}
        {(m.log?.length ?? 0) === 0 && <div className="text-ink-4">{t("等待输出…")}</div>}
      </div>
    </div>
  );
}

function TestCard({ tests, balance }: { tests: { label: string; r: TestResult }[]; balance: Balance | null }) {
  const { t } = useI18n();
  return (
    <div className="rounded-xl border border-white/[0.08] bg-bg-1 px-4 py-3.5 space-y-3 shadow-sm">
      {tests.map((tc, i) => (
        <div key={i} className="space-y-1.5">
          <div className="flex items-center gap-2 text-[12px]">
            {tc.r.ok ? (
              <span className="grid place-items-center w-5 h-5 rounded-full bg-success-500/12 shrink-0">
                <CheckCircle2 size={11} className="text-success-400" />
              </span>
            ) : (
              <span className="grid place-items-center w-5 h-5 rounded-full bg-danger-500/12 shrink-0">
                <XCircle size={11} className="text-danger-400" />
              </span>
            )}
            <span className="text-ink-1">{tc.label}</span>
            <span className="font-mono text-[10px] text-ink-4 ml-auto">{tc.r.latency_ms}ms</span>
          </div>
          <div
            className={cn(
              "ml-7 text-[11.5px] leading-snug rounded-lg px-3 py-2 border",
              tc.r.ok ? "bg-success-500/[0.06] text-success-400 border-success-500/20" : "bg-danger-500/[0.06] text-danger-400 border-danger-500/20",
            )}
          >
            {tc.r.ok ? t("模型回话：「{reply}」", { reply: tc.r.reply ?? "" }) : tc.r.error}
          </div>
        </div>
      ))}
      {balance && (
        <div className="flex items-center gap-2 pt-2 border-t border-white/[0.06] text-[12px]">
          <RefreshCw size={12} className="text-accent" />
          <span className="text-ink-2">{t("虾盘云余额：")}</span>
          <span className="font-mono text-accent font-semibold">{balance.text}</span>
        </div>
      )}
    </div>
  );
}
