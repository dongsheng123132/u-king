/**
 * Codex 工作站 —— Codex 桌面版装机 + 一键接虾盘云驱动 + 模板交付 + computer use 用法教程。
 *
 * 设计取舍（2026-06-20，按「让代码抗变化、别让你和用户抗」精简）：
 * 早期这里做 computer use「体检」，深挖 Codex 私有运行时路径（runtimes/cua_node/<hash>/…）判断
 * 能不能用 —— 那是 OpenAI 内部结构，每次升级都可能改，代码追着上游跑、还常误判。**已删除**。
 * 现在只保留两个稳的状态（装了没 / 驱动接管了没），computer use 怎么用改成**图文教程兜底**：
 * 装好 → 接虾盘云驱动 → 在 Codex 里「@电脑 / 自动化」发任务，首次它自己下运行时。不再探测下结论。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Bot,
  CheckCircle2,
  Clipboard,
  Cpu,
  Download,
  ExternalLink,
  FileText,
  Info,
  KeyRound,
  ListChecks,
  Loader2,
  MonitorCog,
  RefreshCw,
  ShieldCheck,
  Wand2,
} from "lucide-react";
import { cn } from "./lib/cn";
import { useI18n } from "./i18n";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

/** 翻译函数类型（供 module-scope 助手接收，默认恒等，避免未传时崩）。 */
type TFn = (zh: string, vars?: Record<string, string | number>) => string;

/** Codex 桌面版自动装不上时的图文教程网页（内嵌 exe，离线可用；含商店/下载/重启识别说明）。 */
async function openCodexGuide() {
  await invoke("open_codex_guide").catch(() => {});
}

/** Codex 高级配置教程：本地模型（--oss/Ollama）+ 自定义云驱动（model_providers），进阶选配。 */
async function openCodexLocalGuide() {
  await invoke("open_codex_local_guide").catch(() => {});
}

type ToolInfo = {
  id: string;
  name: string;
  summary: string;
  kind: "deep" | "standalone";
  installed: boolean;
  action: "install" | "url";
  target: string;
  launch_cmd: string;
  launch_app: string;
};

type DriverStatus = {
  claude_base: string | null;
  claude_model: string | null;
  codex_provider: string | null;
  codex_model: string | null;
};

/** 轻量状态（后端 codex_status；只保留稳的来源，不探上游私有路径）。 */
type CodexStatus = {
  app_installed: boolean;
  app_version: string | null;
  driver_managed: boolean;
};

type InstallToolResult = {
  ok: boolean;
  tool: string;
  version: string | null;
  attempts: number;
  error: string | null;
};

type TestResult = {
  ok: boolean;
  api: string;
  latency_ms: number;
  reply: string | null;
  error: string | null;
};

/**
 * Codex 任务模板 —— 富结构：三个引导问题 + 一个填空槽位 + 完整提示词（{{FILL}} 处替换用户输入）。
 * questions 是给小白的「先想清楚这三点」，fill 是把「说不清」变成结构化任务的那段话。
 * 任务生成器（TaskComposer）据此把用户一句话拼成 Codex 能直接执行的标准提示词。
 */
type CodexTemplate = {
  id: string;
  category: string;
  title: string;
  summary: string;
  /** 填之前先想清楚的三个问题（Coach 式引导） */
  questions: string[];
  /** 填空段的小标题，如「我的目标」 */
  fillHeading: string;
  /** 填空框的占位提示 */
  fillPlaceholder: string;
  /** 完整提示词，{{FILL}} 会被用户输入替换（留空则用 fillPlaceholder 兜底） */
  prompt: string;
};

const CODEX_TEMPLATES: CodexTemplate[] = [
  {
    id: "first-codebase",
    category: "代码",
    title: "接手陌生项目先分析",
    summary: "让 Codex 先读结构、启动方式、测试命令和风险，再动手，少误改。",
    questions: ["项目在哪个文件夹？", "你最终想改成什么样？", "有没有绝对不能动的文件或功能？"],
    fillHeading: "我的目标",
    fillPlaceholder: "写你接手这个项目后要完成的目标，例如：给这个后台加一个导出 Excel 的按钮。",
    prompt: `请先不要改文件。请阅读当前项目结构，找出技术栈、启动方式、关键目录、测试命令和潜在风险。

【我的目标】
{{FILL}}

请输出：
1. 项目结构和关键文件。
2. 运行、构建、测试命令。
3. 你建议先看的文件。
4. 实施前的风险和需要我确认的问题。
5. 信息足够的话，再给出下一步最小执行计划。`,
  },
  {
    id: "implement-feature",
    category: "代码",
    title: "实现一个功能",
    summary: "明确知道要加什么、但不确定改哪里时用。按最小改动执行。",
    questions: ["用户从哪里进入这个功能？", "完成后界面或命令该出现什么？", "怎么算成功？"],
    fillHeading: "功能目标",
    fillPlaceholder: "写清楚最终用户能做什么，例如：登录后能看到自己的订单列表并按时间排序。",
    prompt: `请在当前项目中实现这个功能。

【功能目标】
{{FILL}}

【验收标准】
1. 保持现有风格和交互一致。
2. 不改无关文件，不覆盖我未提交的改动。
3. 修改后运行相关测试；跑不了就说明原因。
4. 最后列出改了哪些文件、如何验证。

请先快速阅读相关代码，再直接实现并验证。`,
  },
  {
    id: "fix-bug",
    category: "排错",
    title: "修复报错",
    summary: "把现象、复现步骤、日志交给 Codex，让它先定位根因再动手。",
    questions: ["你点了哪里出的错？", "实际报错是什么？", "以前正常还是一直不正常？"],
    fillHeading: "现象",
    fillPlaceholder: "描述你看到了什么错误、在什么操作之后出现。日志里别带 API Key / 密码 / Token。",
    prompt: `请帮我修复这个问题。

【现象】
{{FILL}}

【复现步骤】
1. 第一步。
2. 第二步。

请先定位根因，说明影响范围，再做最小必要修改并验证。不要粘贴或询问 API Key、密码、Token。`,
  },
  {
    id: "review-changes",
    category: "代码",
    title: "代码审查",
    summary: "按 bug、回归、安全、缺测试的顺序审查当前改动。",
    questions: ["审查当前改动还是某个文件？", "重点看安全、性能还是界面？", "要不要给修复建议？"],
    fillHeading: "审查范围",
    fillPlaceholder: "写要审查的分支、文件或功能，例如：这次提交对结算金额的改动。",
    prompt: `请用代码审查模式检查改动。

【审查范围】
{{FILL}}

请优先找：
1. bug 和行为回归。
2. 安全风险。
3. 缺少的测试。
4. 交互或文案可能误导用户的地方。

按严重程度排序，给出文件和行号。没问题也请明说，并列出剩余测试风险。`,
  },
  {
    id: "generate-tests",
    category: "测试",
    title: "补测试用例",
    summary: "围绕正常路径、失败路径和最相关的回归风险补测试。",
    questions: ["要测哪个功能？", "成功和失败场景分别是什么？", "现有测试命令是什么？"],
    fillHeading: "测试目标",
    fillPlaceholder: "写要覆盖的功能和风险，例如：优惠券金额计算，含过期和叠加两种边界。",
    prompt: `请为当前项目补充测试。

【测试目标】
{{FILL}}

请先查看现有测试框架和命令，再补最小必要测试，覆盖：
1. 正常路径。
2. 失败或边界路径。
3. 与本次改动最相关的回归风险。

完成后运行测试并说明结果。`,
  },
  {
    id: "ui-polish",
    category: "前端",
    title: "优化界面可读性",
    summary: "按钮遮挡、文字太小、布局拥挤、小白看不懂时用。",
    questions: ["哪一页看不懂？", "哪个按钮或文字有歧义？", "目标用户是谁？"],
    fillHeading: "问题",
    fillPlaceholder: "例如：设置页文字太小、按钮挤在一起、手机上重叠、重点不明显。",
    prompt: `请优化当前界面的可读性和操作路径。

【问题】
{{FILL}}

【要求】
1. 保持现有产品风格。
2. 不新增无关功能。
3. 让第一次用的小白知道下一步点哪。
4. 桌面和手机端布局都要正常。
5. 改完用截图或测试说明验证结果。`,
  },
  {
    id: "docs-readme",
    category: "文档",
    title: "README / 教程重写",
    summary: "把介绍、启动方式、交付步骤整理成客户看得懂的文档。",
    questions: ["文档给谁看？", "用户第一步要做什么？", "哪些风险不能承诺？"],
    fillHeading: "文档目标",
    fillPlaceholder: "写文档服务的对象和目的，例如：给不懂技术的客户看的上手说明。",
    prompt: `请重写或补充项目文档。

【文档目标】
{{FILL}}

请输出适合客户或团队阅读的版本，包含：
1. 这个项目是什么。
2. 第一次怎么启动。
3. 常用功能怎么用。
4. 常见问题。
5. 不能承诺或需要注意的边界。

别写空泛宣传词，优先写能照着做的步骤。`,
  },
  {
    id: "project-init",
    category: "初始化",
    title: "新项目初始化",
    summary: "从空文件夹建立结构、依赖、质量工具和说明。",
    questions: ["项目类型是什么？", "用什么技术栈？", "需要哪些质量检查？"],
    fillHeading: "项目目标",
    fillPlaceholder: "写要初始化的项目，例如：一个能记账并导出月报的本地小工具。",
    prompt: `请帮我初始化一个新项目。

【项目目标】
{{FILL}}

请先确认当前目录安全，再建立：
1. 清晰的目录结构。
2. 必要依赖和运行脚本。
3. 基础测试或质量检查。
4. README 启动说明。
5. 后续开发建议。

别引入过重技术栈，优先项目实际需要的最小方案。`,
  },
  {
    id: "idea-to-plan",
    category: "规划",
    title: "把想法变执行计划",
    summary: "把一句话需求拆成 P0/P1/P2 和今天能验收的最小任务。",
    questions: ["你想做成什么？", "给谁用？", "今天必须交付什么？"],
    fillHeading: "我的想法",
    fillPlaceholder: "粘贴你原始的想法，越随意越好，Codex 会帮你理成计划。",
    prompt: `请把下面的想法整理成可执行计划。

【我的想法】
{{FILL}}

请输出：
1. 最终目标。
2. 今天必须完成的 P0。
3. 可后续做的 P1/P2。
4. 分阶段执行步骤。
5. 每阶段验收标准。
6. 风险和需要我确认的问题。

信息足够的话，请直接开始第一阶段。`,
  },
  {
    id: "multi-agent-plan",
    category: "计划",
    title: "多 Agent 分工",
    summary: "大任务拆成互不冲突的并行工作，避免几个 Agent 改同一文件。",
    questions: ["总目标是什么？", "哪些文件可能被改？", "哪些子任务能并行？"],
    fillHeading: "目标",
    fillPlaceholder: "写这个大任务的最终结果。",
    prompt: `这是一个较大的任务，请先判断是否适合多 Agent 并行。

【目标】
{{FILL}}

【约束】
1. 不改无关文件。
2. 每个 Agent 的写入范围必须互不重叠。
3. 先做探索和分工，再执行。

请输出：
1. 哪些子任务适合并行。
2. 每个 Agent 的职责和文件范围。
3. 主 Agent 需保留的关键路径工作。
4. 合并与验证步骤。`,
  },
  {
    id: "prompt-refine",
    category: "提示词",
    title: "把提示词改得更稳",
    summary: "把口语化、太长、太散的提示词整理成可复用模板。",
    questions: ["原提示词是什么？", "要重复用在什么场景？", "希望输出什么格式？"],
    fillHeading: "原始提示词",
    fillPlaceholder: "粘贴你现在用的提示词，哪怕很口语、很乱都行。",
    prompt: `请把下面的原始提示词精炼成可复用模板。

【原始提示词】
{{FILL}}

请输出：
1. 适用场景。
2. 精炼后的提示词。
3. 可替换的变量。
4. 使用示例。
5. 不适用或需人工确认的边界。

要让 Codex 拿到就能直接执行，不要变成空泛鸡汤。`,
  },
  {
    id: "safe-release",
    category: "交付",
    title: "交付前检查",
    summary: "发 U 盘或远程装机给客户前，让 Codex 帮忙查漏。",
    questions: ["交付给谁？", "必须检查哪些入口？", "有没有不能对外承诺的话术？"],
    fillHeading: "交付对象",
    fillPlaceholder: "写这次要交付的产品或目录，例如：给客户的 U 盘根目录。",
    prompt: `请做交付前检查。

【交付对象】
{{FILL}}

请检查：
1. 必需文件是否存在。
2. 是否有 API Key、密码、令牌被写进模板 / 文档 / 报告 / 日志。
3. 启动入口是否可用。
4. 自检脚本是否通过。
5. 客户首次使用步骤是否清楚。
6. 哪些功能不能对外承诺为已完成。

能直接修的先修，列出仍需人工确认的事项。`,
  },
];

/** 把模板 + 用户填的内容拼成完整提示词（空输入用占位提示兜底，避免生成半截 prompt）。 */
function composePrompt(tpl: CodexTemplate, fill: string, t: TFn = (s) => s): string {
  const v = fill.trim() || t(tpl.fillPlaceholder);
  return t(tpl.prompt).replace("{{FILL}}", v);
}

const TEMPLATE_CATEGORIES = ["全部", ...Array.from(new Set(CODEX_TEMPLATES.map((tpl) => tpl.category)))];

const CUSTOMER_HANDOFF_CHECKS = [
  "客户只点 Codex 工作站，不需要找 bat 或配置文件。",
  "先安装 Codex，再接虾盘云，最后用模板开工。",
  "API Key 只在本机配置页输入，不发微信、不发聊天窗口。",
  "安装失败时留在当前页，点手动安装教程继续。",
];

async function copyText(text: string, onToast?: (s: string) => void, t: TFn = (s) => s) {
  try {
    await navigator.clipboard.writeText(text);
    onToast?.(t("已复制到剪贴板"));
  } catch {
    onToast?.(t("复制失败，请手动选中文本复制"));
  }
}

/** Codex 模型路由候选 —— 全部经本地代理走虾盘云（responses→chat 翻译，我们赚）。
 *  国产=省钱推荐；海外=能用但按量贵，打「💰」标（对齐「切国外提醒贵」）。改这里内容不动后端。 */
type RouteModel = { id: string; label: string; desc?: string };
const CODEX_ROUTE_GROUPS: { group: string; pricey?: boolean; items: RouteModel[] }[] = [
  {
    group: "🇨🇳 国产 · 省钱推荐",
    items: [
      // flash 在前且是「默认推荐」—— Codex 链路装好在用的是 `deepseek-v4-flash-codex`
      //（/v1/responses 只有这条 type=1 直连渠道认）。原本把 pro 标成「默认推荐 · 最快最省」
      // 是错的：既不是默认，也不最省。
      { id: "deepseek-v4-flash", label: "DeepSeek V4 Flash", desc: "默认推荐 · 最快最省" },
      { id: "deepseek-v4-pro", label: "DeepSeek V4 Pro", desc: "满血推理 · 更慢更费" },
      { id: "kimi-k3", label: "Kimi K3", desc: "超长上下文" },
      { id: "glm-5.2", label: "GLM-5.2 · 智谱", desc: "中文顺手" },
      { id: "qwen3.7-max", label: "通义 Qwen3.7 Max", desc: "国产偏聪明" },
      { id: "MiniMax-M3", label: "MiniMax M3 · 海螺" },
    ],
  },
  {
    group: "🌍 海外旗舰 · 更贵",
    pricey: true,
    items: [
      { id: "claude-sonnet-5", label: "Claude Sonnet 5", desc: "编程之王 · 费额度" },
      { id: "gpt-5.4", label: "GPT-5.4 · OpenAI" },
      { id: "grok-4.5", label: "Grok 4.5 · xAI" },
      { id: "gemini-3.1-pro-preview", label: "Gemini 3.1 Pro · 谷歌" },
    ],
  },
];

type ProxyStatus = { running?: boolean; on_proxy?: boolean; model?: string; kind?: string; label?: string };

/** Codex 模型路由 —— 本地翻译代理（codex_proxy.rs）把 Codex 的 responses 转成 chat，接虾盘云任意模型或
 *  你自己的 OpenAI 兼容端点。默认虾盘云 DeepSeek（省）；可切国产/海外(贵)/自定义 key；关=直连贵的 gpt-5.x-codex。 */
function ProxyRouteCard({ onToast }: { onToast?: (s: string) => void }) {
  const { t } = useI18n();
  const [st, setSt] = useState<ProxyStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [showCustom, setShowCustom] = useState(false);
  const [cBase, setCBase] = useState("");
  const [cKey, setCKey] = useState("");
  const [cModel, setCModel] = useState("");
  const load = useCallback(() => {
    invoke<ProxyStatus>("codex_proxy_status").then(setSt).catch(() => {});
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  const on = !!st?.on_proxy;
  const curModel = st?.model ?? "";
  const curKind = st?.kind ?? "xiapan";

  const applyRoute = useCallback(
    async (route: { kind: string; model: string; label?: string; base_url?: string; key?: string }, msg: string) => {
      setBusy(true);
      try {
        await invoke("codex_proxy_start", { route });
        onToast?.(msg);
      } catch (e) {
        onToast?.(t("切换失败: {e}", { e: String(e) }));
      } finally {
        setBusy(false);
        setTimeout(load, 500);
      }
    },
    [onToast, load, t],
  );

  const pickXiapan = (m: RouteModel) =>
    void applyRoute(
      { kind: "xiapan", model: m.id, label: m.label },
      t("已切到 {m} · 重启 Codex 生效", { m: m.label }),
    );

  const applyCustom = () => {
    const model = cModel.trim();
    if (!model) {
      onToast?.(t("请填模型名（如 kimi-k2-0711-preview）"));
      return;
    }
    void applyRoute(
      { kind: "custom", model, label: model, base_url: cBase.trim(), key: cKey.trim() },
      t("已切到自定义端点 · 重启 Codex 生效"),
    );
  };

  const turnOff = () =>
    void (async () => {
      setBusy(true);
      try {
        await invoke("codex_proxy_stop");
        onToast?.(t("已关路由 · Codex 直连 gpt-5.x-codex（贵，重启 Codex 生效）"));
      } catch (e) {
        onToast?.(t("切换失败: {e}", { e: String(e) }));
      } finally {
        setBusy(false);
        setTimeout(load, 500);
      }
    })();

  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
      <div className="flex items-start justify-between gap-4">
        <div className="flex gap-3 min-w-0">
          <span className={cn("shrink-0 grid place-items-center w-9 h-9 rounded-lg", on ? "bg-success-500/15 text-success-400" : "bg-amber-500/15 text-amber-400")}>
            <Cpu size={18} />
          </span>
          <div className="min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-[14px] font-semibold text-ink-0">{t("Codex 模型路由")}</span>
              <span className={cn("text-[10px] px-1.5 py-0.5 rounded-full border", on ? "border-success-500/40 text-success-400 bg-success-500/10" : "border-amber-500/40 text-amber-400 bg-amber-500/10")}>
                {on ? t("当前：{m}", { m: st?.label || curModel }) : t("已关 · 直连贵的 GPT")}
              </span>
            </div>
            <p className="mt-1.5 text-[12px] text-ink-2 leading-relaxed">
              {t("Codex 只认 responses，本地代理把它转成 chat 接")}<b className="text-ink-0">{t("虾盘云任意模型")}</b>{t("。国产")}<b className="text-success-400">{t("省钱")}</b>{t("、海外旗舰能用但")}<b className="text-amber-400">{t("按量更贵")}</b>{t("。切换后")}<b className="text-ink-0">{t("重启 Codex")}</b>{t("生效。")}
            </p>
          </div>
        </div>
      </div>

      {/* 虾盘云模型（国产省 / 海外贵） */}
      <div className="mt-4 space-y-3">
        {CODEX_ROUTE_GROUPS.map((grp) => (
          <div key={grp.group}>
            <div className="text-[11px] font-medium text-ink-3 mb-1.5">{t(grp.group)}</div>
            <div className="flex flex-wrap gap-1.5">
              {grp.items.map((m) => {
                const active = on && curKind === "xiapan" && curModel === m.id;
                return (
                  <button
                    key={m.id}
                    onClick={() => pickXiapan(m)}
                    disabled={busy}
                    title={m.desc ? t(m.desc) : m.label}
                    className={cn(
                      "inline-flex items-center gap-1 h-8 px-3 rounded-lg text-[12px] font-medium border transition-colors disabled:opacity-50",
                      active
                        ? "border-accent bg-accent text-white"
                        : "border-white/[0.10] bg-white/[0.02] text-ink-1 hover:bg-white/[0.05]",
                    )}
                  >
                    {active && <CheckCircle2 size={12} />}
                    {m.label}
                    {grp.pricey && !active && <span className="text-amber-400">💰</span>}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      {/* 自定义端点（BYOK：粘自己的 key + 端点，如 Kimi 平台 / MiniMax / DeepSeek 官方 / 任意兼容） */}
      <div className="mt-3 rounded-lg border border-white/[0.06] bg-white/[0.015]">
        <button
          onClick={() => setShowCustom((v) => !v)}
          className={cn(
            "w-full flex items-center justify-between px-3.5 py-2.5 text-left",
            on && curKind === "custom" ? "text-accent" : "text-ink-2",
          )}
        >
          <span className="flex items-center gap-2 text-[12px] font-medium">
            <KeyRound size={13} />
            {t("用自己的订阅 / 自定义端点（粘 key + 地址）")}
            {on && curKind === "custom" && (
              <span className="text-[10px] px-1.5 py-0.5 rounded-full border border-accent/40 bg-accent/10">{t("当前：{m}", { m: st?.label || curModel })}</span>
            )}
          </span>
          <span className="text-[11px] text-ink-4">{showCustom ? t("收起") : t("展开")}</span>
        </button>
        {showCustom && (
          <div className="px-3.5 pb-3.5 pt-0.5 space-y-2">
            <p className="text-[11px] leading-relaxed text-ink-4">
              {t("适合 Kimi 平台 / MiniMax / DeepSeek 官方 / OpenRouter 等 OpenAI 兼容端点。key 只存本机、只用于你自己的账户计费。")}
            </p>
            <input
              value={cBase}
              onChange={(e) => setCBase(e.target.value)}
              placeholder={t("端点 Base URL（如 https://api.moonshot.cn/v1，留空=虾盘云）")}
              className="w-full h-8 rounded-md border border-white/[0.10] bg-bg-1 px-3 text-[12px] text-ink-1 placeholder:text-ink-4 focus:border-accent focus:outline-none"
            />
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
              <input
                value={cModel}
                onChange={(e) => setCModel(e.target.value)}
                placeholder={t("模型名（如 kimi-k2-0711-preview）")}
                className="h-8 rounded-md border border-white/[0.10] bg-bg-1 px-3 text-[12px] text-ink-1 placeholder:text-ink-4 focus:border-accent focus:outline-none"
              />
              <input
                value={cKey}
                onChange={(e) => setCKey(e.target.value)}
                type="password"
                placeholder={t("你的 API Key（留空=用虾盘云设备 Key）")}
                className="h-8 rounded-md border border-white/[0.10] bg-bg-1 px-3 text-[12px] text-ink-1 placeholder:text-ink-4 focus:border-accent focus:outline-none"
              />
            </div>
            <button
              onClick={applyCustom}
              disabled={busy}
              className="inline-flex items-center gap-1.5 h-8 px-4 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-50"
            >
              {busy ? <Loader2 size={13} className="animate-spin" /> : <Cpu size={13} />}
              {t("接入自定义端点")}
            </button>
          </div>
        )}
      </div>

      {/* 关闭路由 = 直连贵的 gpt-5.x-codex */}
      <div className="mt-3 flex items-center justify-between gap-3 flex-wrap">
        <p className="text-[11px] text-ink-5 leading-relaxed">
          {t("只有 Codex 需要这个代理——Claude / Hermes / U-King 助手本来就直连便宜模型。")}
        </p>
        <button
          onClick={turnOff}
          disabled={busy || !on}
          className="shrink-0 inline-flex items-center gap-1.5 h-8 px-3 rounded-lg text-[12px] font-medium border border-white/[0.10] text-ink-2 hover:bg-white/[0.06] disabled:opacity-40"
        >
          {t("关闭路由（直连贵 GPT）")}
        </button>
      </div>
    </section>
  );
}

export function CodexZone({
  tools,
  driver,
  onLaunch,
  onGoManage,
  onToast,
}: {
  tools: ToolInfo[];
  driver: DriverStatus | null;
  onOpen?: (t: ToolInfo) => void;
  onLaunch: (t: ToolInfo) => void;
  onGoManage: () => void;
  onToast?: (s: string) => void;
}) {
  const { t } = useI18n();
  const [status, setStatus] = useState<CodexStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [installLog, setInstallLog] = useState<string[]>([]);
  // 任务生成器（借鉴虾印 Coach）：选场景 + 填一句话 → 拼出结构化提示词。
  const [composerId, setComposerId] = useState<string>(CODEX_TEMPLATES[0].id);
  const [composerFill, setComposerFill] = useState("");
  const [templateCat, setTemplateCat] = useState<string>("全部");

  const loadStatus = useCallback(() => {
    setLoading(true);
    invoke<CodexStatus>("codex_status")
      .then(setStatus)
      .catch(() => setStatus(null))
      .finally(() => setLoading(false));
  }, []);

  // 一键把 Codex 接到虾盘云（responses / gpt-5.3-codex），用设备内置 Key 计费。
  // ⚠️ apiKey 必须是 dk.key，不能空串 —— codex target 空 Key 会被后端拒绝。
  const applyXiapanToCodex = useCallback(async () => {
    if (applying) return;
    setApplying(true);
    try {
      const dk = await invoke<{ key: string }>("get_device_key");
      await invoke("apply_provider", {
        providerId: "xiapan",
        apiKey: dk.key,
        model: null, // 后端取预设 codex_model=gpt-5.3-codex；wire_api=responses
        targets: ["codex"],
      });
      const test = await invoke<TestResult>("test_provider", {
        providerId: "xiapan",
        apiKey: dk.key,
        model: null,
        api: "openai",
      });
      if (!test.ok) {
        onToast?.(t("配置已写入，但连通测试失败：{err}", { err: test.error || t("请检查余额/网络") }));
      } else {
        onToast?.(t("已把 Codex 接到虾盘云，并通过 responses 实测"));
      }
      loadStatus();
    } catch (e) {
      // 把后端原始报错翻成人话，别让小白对着英文/状态码发懵
      const raw = String(e);
      const msg = /quota|not enough|insufficient|余额不足|billing|403/i.test(raw)
        ? t("接入失败：虾盘云余额不足，请去「② 虾盘云·充值」补充余额后再试")
        : /401|invalid|unauthorized|key/i.test(raw)
          ? t("接入失败：Key 暂时不可用，请先充值后刷新；仍不行再重新接入虾盘云驱动")
        : /timeout|timed out|超时/i.test(raw)
          ? t("接入失败：连接超时，检查下网络再试一次")
          : /network|连接|resolve|curl/i.test(raw)
            ? t("接入失败：网络连不上，检查下网络再试一次")
            : t("接入失败，可去「AI 设置」页手动配置 Codex");
      onToast?.(msg);
    } finally {
      setApplying(false);
    }
  }, [applying, onToast, loadStatus, t]);

  useEffect(() => {
    loadStatus();
  }, [loadStatus]);

  useEffect(() => {
    const un = listen<{ tool: string; phase: string; line: string }>("uking:wizard", (e) => {
      if (e.payload.tool !== "codex-app" && e.payload.tool !== "codex") return;
      const prefix =
        e.payload.phase === "repair" ? t("修复") : e.payload.phase === "verify" ? t("验证") : t("安装");
      setInstallLog((prev) => [...prev, t("{prefix}：{line}", { prefix, line: e.payload.line })].slice(-8));
    });
    return () => {
      un.then((f) => f());
    };
  }, [t]);

  const installCodexApp = useCallback(async () => {
    if (installing) return;
    if (!IS_WINDOWS) {
      onToast?.(t("Mac 版 Codex 请下载官方 DMG，拖到 Applications 后回 U-King 重新检测"));
      await openCodexGuide();
      return;
    }
    setInstalling(true);
    setInstallError(null);
    setInstallLog([t("开始安装 Codex 桌面版…")]);
    try {
      const r = await invoke<InstallToolResult>("install_tool", { toolId: "codex-app" });
      if (!r.ok) {
        const msg = r.error || t("自动安装没有完成，请按手动教程安装");
        setInstallError(msg);
        onToast?.(t("Codex 自动安装未完成，已显示手动安装入口"));
        return;
      }
      onToast?.(t("Codex 桌面版安装完成，并已自动接入虾盘云；余额不足时充值即可用"));
      loadStatus();
    } catch (e) {
      const msg = String(e);
      setInstallError(msg);
      onToast?.(t("Codex 自动安装失败，已显示手动安装入口"));
    } finally {
      setInstalling(false);
    }
  }, [installing, onToast, loadStatus, t]);

  // 产品只主推 Codex 桌面版；CLI 不再引导装（存量已装的照常切驱动/启动）。
  const codexApp = tools.find((tool) => tool.id === "codex-app");
  const codexTools = [codexApp].filter(Boolean) as ToolInfo[];

  const appInstalled = !!status?.app_installed;
  const driverOn =
    !!status?.driver_managed ||
    !!(driver?.codex_provider && driver.codex_provider.toLowerCase().includes("xiapan"));
  const readyCount = [appInstalled, driverOn].filter(Boolean).length;
  const stationReadyText =
    readyCount === 2 ? t("已就绪") : readyCount === 1 ? t("还差一步") : loading ? t("检测中") : t("待配置");
  const stationSteps = [
    {
      title: t("安装 Codex"),
      desc: appInstalled ? status?.app_version ? t("已安装 {ver}", { ver: status.app_version }) : t("已检测到桌面版") : t("一键安装或打开教程"),
      done: appInstalled,
      action: appInstalled ? t("打开 Codex") : IS_WINDOWS ? t("一键安装") : t("安装教程"),
      onClick: () => (appInstalled && codexApp ? onLaunch(codexApp) : installCodexApp()),
      busy: installing,
    },
    {
      title: t("接虾盘云"),
      desc: driverOn ? driver?.codex_model ?? t("已写入 U-King 管理配置") : t("写入 Base URL、模型和设备 Key"),
      done: driverOn,
      action: driverOn ? t("换模型") : t("一键接入"),
      onClick: () => (driverOn ? onGoManage() : applyXiapanToCodex()),
      busy: applying,
    },
    {
      title: t("验证能用"),
      desc: driverOn ? t("接入时已做 responses 连通测试") : t("接入后会自动实测一次"),
      done: driverOn,
      action: t("刷新状态"),
      onClick: loadStatus,
      busy: loading,
    },
    {
      title: t("复制模板开工"),
      desc: t("修 bug、做网页、多 Agent、交付自检"),
      done: appInstalled && driverOn,
      action: t("看模板"),
      onClick: () => document.getElementById("codex-template-library")?.scrollIntoView({ behavior: "smooth", block: "start" }),
      busy: false,
    },
  ];

  // 任务生成器派生值
  const composerTpl = CODEX_TEMPLATES.find((tpl) => tpl.id === composerId) ?? CODEX_TEMPLATES[0];
  const composedPrompt = composePrompt(composerTpl, composerFill, t);
  const filteredTemplates =
    templateCat === "全部" ? CODEX_TEMPLATES : CODEX_TEMPLATES.filter((tpl) => tpl.category === templateCat);
  /** 点模板卡「用它生成」：切到该模板并滚到生成器。 */
  const pickTemplate = (id: string) => {
    setComposerId(id);
    setComposerFill("");
    document.getElementById("codex-task-composer")?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="space-y-5">
      {/* DeepSeek 省钱路由：本地翻译代理，让 Codex 走便宜的 DeepSeek（默认关=贵的 gpt-5.x-codex）。 */}
      <ProxyRouteCard onToast={onToast} />

      {/* 工作站首屏：把安装、配置、开工压成一个明确入口。 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
          <div className="min-w-0">
            <div className="flex items-center gap-3">
              <span className="grid place-items-center w-11 h-11 rounded-xl bg-accent text-white">
                <Bot size={22} />
              </span>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="text-[18px] font-semibold text-ink-0">{t("Codex 工作站")}</h2>
                  <span
                    className={cn(
                      "inline-flex h-5 items-center rounded px-2 text-[10px] font-semibold border",
                      readyCount === 2
                        ? "border-success-500/25 bg-success-500/10 text-success-400"
                        : "border-accent/25 bg-accent/10 text-accent",
                    )}
                  >
                    {stationReadyText}
                  </span>
                </div>
                <p className="text-[11.5px] text-ink-3 mt-1 leading-relaxed">
                  {t("给客户用的一站式 Codex 入口：安装桌面版、接入虾盘云、复制任务模板、查看 computer use 教程。")}
                </p>
              </div>
            </div>

            <div className="mt-4 grid grid-cols-1 sm:grid-cols-3 gap-2.5">
              <button
                onClick={appInstalled && codexApp ? () => onLaunch(codexApp) : installCodexApp}
                disabled={installing}
                className="inline-flex h-9 items-center justify-center gap-1.5 rounded-lg bg-accent px-3 text-[12px] font-semibold text-white hover:bg-accent-600 disabled:opacity-50"
              >
                {installing ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
                {appInstalled ? t("打开 Codex") : IS_WINDOWS ? t("一键安装 Codex") : t("安装教程")}
              </button>
              <button
                onClick={driverOn ? onGoManage : applyXiapanToCodex}
                disabled={applying}
                className="inline-flex h-9 items-center justify-center gap-1.5 rounded-lg border border-white/[0.10] bg-white/[0.02] px-3 text-[12px] font-semibold text-ink-1 hover:bg-white/[0.05] disabled:opacity-50"
              >
                {applying ? <Loader2 size={14} className="animate-spin" /> : <KeyRound size={14} />}
                {driverOn ? t("管理模型") : t("一键接虾盘云")}
              </button>
              <button
                onClick={() => document.getElementById("codex-template-library")?.scrollIntoView({ behavior: "smooth", block: "start" })}
                className="inline-flex h-9 items-center justify-center gap-1.5 rounded-lg border border-white/[0.10] bg-white/[0.02] px-3 text-[12px] font-semibold text-ink-1 hover:bg-white/[0.05]"
              >
                <Clipboard size={14} />
                {t("任务模板库")}
              </button>
            </div>
          </div>

          <div className="grid min-w-[220px] grid-cols-2 gap-2 lg:w-[260px]">
            <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-3">
              <div className="flex items-center gap-1.5 text-[11px] text-ink-3">
                <MonitorCog size={13} className="text-accent" />
                Codex
              </div>
              <div className="mt-1 text-[13px] font-semibold text-ink-0">{appInstalled ? t("已安装") : t("待安装")}</div>
            </div>
            <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-3">
              <div className="flex items-center gap-1.5 text-[11px] text-ink-3">
                <ShieldCheck size={13} className="text-accent" />
                {t("驱动")}
              </div>
              <div className="mt-1 text-[13px] font-semibold text-ink-0">{driverOn ? t("已接入") : t("待接入")}</div>
            </div>
          </div>
        </div>
      </section>

      {/* 状态卡 —— 只看两个稳的：装了没 + 驱动接了没 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
        <header className="flex items-center justify-between mb-4">
          <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2">
            <MonitorCog size={15} className="text-accent" />
            {t("Codex 状态")}
          </h3>
          <button
            onClick={loadStatus}
            disabled={loading}
            className="inline-flex items-center gap-1.5 h-7 px-3 rounded-md border border-white/[0.10] text-ink-2 text-[11px] hover:bg-white/[0.04] disabled:opacity-50"
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
            {loading ? t("检测中") : t("刷新")}
          </button>
        </header>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {/* Codex 桌面版 */}
          <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-4 py-3.5">
            <div className="flex items-center gap-2 mb-2">
              <span className={cn("w-2 h-2 rounded-full", appInstalled ? "bg-success-400" : "bg-ink-4")} />
              <span className="text-[12.5px] font-medium text-ink-1">{t("Codex 桌面版")}</span>
              <span className="ml-auto text-[11px] text-ink-4 font-mono">
                {appInstalled ? status?.app_version ?? t("已装") : t("未装")}
              </span>
            </div>
            {appInstalled ? (
              codexApp && (
                <button
                  onClick={() => onLaunch(codexApp)}
                  className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-md border border-white/[0.10] text-ink-1 text-[12px] hover:bg-white/[0.04]"
                >
                  <Bot size={13} /> {t("打开 Codex")}
                </button>
              )
            ) : (
              <div className="flex flex-col gap-1.5">
                {codexApp && IS_WINDOWS && (
                  <button
                    onClick={installCodexApp}
                    disabled={installing}
                    className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-50"
                  >
                    {installing ? <Loader2 size={13} className="animate-spin" /> : <Download size={13} />}
                    {installing ? t("安装中…") : t("一键安装")}
                  </button>
                )}
                {!IS_WINDOWS && (
                  <button
                    onClick={openCodexGuide}
                    className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
                  >
                    <ExternalLink size={13} /> {t("打开 Mac 安装教程")}
                  </button>
                )}
                {/* 商店/DMG 装完这里可能还显示「未装」——明确告诉小白重启 U-King 才认得出来，
                    免得他以为失败又装一遍。 */}
                <div className="flex items-start gap-1.5 rounded-md bg-accent/[0.08] border border-accent/20 px-2.5 py-2 text-[11px] text-ink-2 leading-snug">
                  <Info size={13} className="text-accent shrink-0 mt-px" />
                  <span>
                    {IS_WINDOWS ? t("商店在后台慢慢装（可能要几分钟）。") : t("DMG 拖进 Applications 后，")}
                    {t("装好后")}<b className="text-ink-1">{t("关掉 U-King 再打开")}</b>{t("，这里就会变「已装」。")}
                  </span>
                </div>
                <button
                  onClick={openCodexGuide}
                  className="inline-flex items-center gap-1 text-[11px] text-accent hover:text-accent-400 w-fit"
                >
                  <ExternalLink size={11} /> {t("自动装不上？点这里手动安装")}
                </button>
                {(installing || installError || installLog.length > 1) && (
                  <div
                    className={cn(
                      "rounded-md border px-2.5 py-2 text-[10.5px] leading-relaxed",
                      installError ? "border-amber-400/25 bg-amber-400/[0.08] text-warning-700 dark:text-warning-400" : "border-white/[0.06] bg-white/[0.025] text-ink-3",
                    )}
                  >
                    {installError ? (
                      <>
                        <div className="font-semibold text-warning-700 dark:text-warning-400">{t("自动安装没完成")}</div>
                        <div className="mt-0.5 line-clamp-2">{installError}</div>
                        <button onClick={openCodexGuide} className="mt-1 text-accent hover:text-accent-400">
                          {t("打开手动安装教程")}
                        </button>
                      </>
                    ) : (
                      installLog.map((l, i) => <div key={i} className="truncate">{l}</div>)
                    )}
                  </div>
                )}
              </div>
            )}
          </div>

          {/* 虾盘云驱动 */}
          <div className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-4 py-3.5">
            <div className="flex items-center gap-2 mb-2">
              <span className={cn("w-2 h-2 rounded-full", driverOn ? "bg-success-400" : "bg-ink-4")} />
              <span className="text-[12.5px] font-medium text-ink-1">{t("虾盘云驱动")}</span>
              <span className="ml-auto text-[11px] text-ink-4 font-mono">
                {driverOn ? driver?.codex_model ?? t("已接管") : t("未接管")}
              </span>
            </div>
            {driverOn ? (
              <button
                onClick={onGoManage}
                className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-md border border-white/[0.10] text-ink-1 text-[12px] hover:bg-white/[0.04]"
              >
                <Cpu size={13} /> {t("换模型 / 换驱动")}
              </button>
            ) : (
              <button
                onClick={applyXiapanToCodex}
                disabled={applying}
                className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-50"
              >
                <Cpu size={13} /> {applying ? t("接入中…") : t("一键接虾盘云")}
              </button>
            )}
          </div>
        </div>
      </section>

      {/* 交付流程：客户照着四步走，售后也能看哪里卡住。 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
        <header className="flex items-center justify-between gap-3 mb-4">
          <div>
            <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2">
              <ListChecks size={15} className="text-accent" />
              {t("Codex 交付流程")}
            </h3>
            <p className="mt-1 text-[11px] text-ink-3">{t("装机、接驱动、验证、开工都在这一页完成，原 U-King 其他入口继续保留。")}</p>
          </div>
          <span className="shrink-0 text-[11px] font-mono text-ink-4">{readyCount}/2 ready</span>
        </header>
        <div className="grid grid-cols-1 md:grid-cols-4 gap-3">
          {stationSteps.map((step, i) => (
            <div key={step.title} className="rounded-lg border border-white/[0.06] bg-white/[0.02] px-3.5 py-3.5">
              <div className="flex items-center justify-between gap-2">
                <span
                  className={cn(
                    "grid h-6 w-6 place-items-center rounded-full text-[11px] font-bold",
                    step.done ? "bg-success-500/15 text-success-400" : "bg-accent/15 text-accent",
                  )}
                >
                  {step.done ? <CheckCircle2 size={14} /> : i + 1}
                </span>
                <span className={cn("text-[10px]", step.done ? "text-success-400" : "text-ink-4")}>
                  {step.done ? t("完成") : t("待处理")}
                </span>
              </div>
              <div className="mt-2.5 text-[13px] font-semibold text-ink-1">{step.title}</div>
              <p className="mt-1 min-h-[34px] text-[11px] leading-snug text-ink-3">{step.desc}</p>
              <button
                onClick={step.onClick}
                disabled={step.busy}
                className="mt-3 inline-flex h-7 w-full items-center justify-center gap-1.5 rounded-md border border-white/[0.10] text-[11px] font-medium text-ink-1 hover:bg-white/[0.04] disabled:opacity-50"
              >
                {step.busy && <Loader2 size={12} className="animate-spin" />}
                {step.action}
              </button>
            </div>
          ))}
        </div>
      </section>

      {/* 安全配置：把 Key 处理规则直接放在工作站里，减少销售/客户误操作。 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-4 shadow-card">
        <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
          <div className="flex items-start gap-3">
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-accent/15 text-accent">
              <ShieldCheck size={18} />
            </span>
            <div>
              <h3 className="text-[13px] font-semibold text-ink-0">{t("安全配置规则")}</h3>
              <p className="mt-1 text-[11.5px] leading-relaxed text-ink-3">
                {t("默认用 U-King 设备 Key 接虾盘云，不让客户把私密 Key 发到聊天窗口。客户自有 Key 只在本机 AI 设置页输入。")}
              </p>
            </div>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 lg:w-[430px]">
            {CUSTOMER_HANDOFF_CHECKS.map((item) => (
              <div key={item} className="flex items-start gap-2 rounded-md border border-white/[0.06] bg-white/[0.02] px-3 py-2 text-[11px] leading-snug text-ink-3">
                <CheckCircle2 size={13} className="mt-0.5 shrink-0 text-success-400" />
                <span>{t(item)}</span>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* 自动安装失败时的强兜底：保持在 Codex 专区，不跳通用装机向导。 */}
      <section className="rounded-card border border-accent/20 bg-accent/[0.06] px-5 py-4 shadow-card">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-start gap-3">
            <Info size={16} className="mt-0.5 text-accent shrink-0" />
            <div>
              <h3 className="text-[13px] font-semibold text-ink-0">{t("自动安装不成功时，走手动安装最稳")}</h3>
              <p className="mt-1 text-[11.5px] leading-relaxed text-ink-3">
                {t("Windows 有些电脑没有 winget、商店后台注册很慢，或被杀毒/公司策略拦住；Mac 需要下载 DMG 后拖到 Applications。教程里给了官方入口和国内兜底，成功任意一条即可。")}
              </p>
            </div>
          </div>
          <button
            onClick={openCodexGuide}
            className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-accent px-3 text-[12px] font-semibold text-white hover:bg-accent-600"
          >
            <ExternalLink size={12} />
            {t("打开手动安装教程")}
          </button>
        </div>
      </section>

      {/* computer use 用法教程（取代旧体检：不探测，直接教怎么用） */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
        <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2 mb-3">
          <Info size={15} className="text-accent" />
          {t("computer use 怎么用（让 Codex 操作浏览器 / 电脑）")}
        </h3>
        <ol className="space-y-2.5">
          {[
            <>{t("装好 ")}<b className="text-ink-1">{t("Codex 桌面版")}</b>{t("（上方状态卡一键装），并 ")}<b className="text-ink-1">{t("接虾盘云驱动")}</b>{t("（computer use 需要支持工具调用的上游，虾盘云支持）。")}</>,
            <>{t("打开 Codex，在左侧找 ")}<b className="text-ink-1">{t("「自动化」")}</b>{t("（英文界面叫 Automations），选 ")}<b className="text-ink-1">{t("「@电脑」")}</b>{t("（@computer）发一条任务；首次它会自动下载私有运行时（约几十 MB，需联网，稍等一会）。")}</>,
            <>{t("下载完就能让它")}<b className="text-ink-1">{t("操作浏览器、点按键盘鼠标")}</b>{t("了。装完没反应多半是运行时还在下，等一会或重发一次任务即可。")}</>,
          ].map((it, i) => (
            <li key={i} className="flex gap-2.5">
              <span className="grid place-items-center w-5 h-5 rounded-full bg-accent/15 text-accent text-[11px] font-bold shrink-0">
                {i + 1}
              </span>
              <div className="text-[12.5px] leading-relaxed text-ink-2 pt-0.5">{it}</div>
            </li>
          ))}
        </ol>
        <button
          onClick={openCodexGuide}
          className="mt-3 inline-flex items-center gap-1 text-[11.5px] text-accent hover:text-accent-400"
        >
          <ExternalLink size={12} /> {t("看完整图文教程（含商店安装 / 重启识别）")}
        </button>
      </section>

      {/* 任务生成器（Coach）：小白选场景 + 填一句话 → 拼出 Codex 能直接执行的结构化提示词。 */}
      <section
        id="codex-task-composer"
        className="rounded-card border border-accent/20 bg-accent/[0.05] px-5 py-5 shadow-card"
      >
        <header className="mb-4">
          <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2">
            <Wand2 size={15} className="text-accent" />
            {t("任务生成器")}
            <span className="inline-flex h-4 items-center rounded bg-accent/15 px-1.5 text-[9px] font-semibold text-accent">
              {t("不会写提示词也能用")}
            </span>
          </h3>
          <p className="mt-1 text-[11px] leading-relaxed text-ink-3">
            {t("选一个场景，回答几个问题、填一句话，自动生成 Codex 能直接执行的标准任务，一键复制粘贴进去即可。")}
          </p>
        </header>

        {/* ① 选场景 */}
        <div className="flex flex-wrap gap-1.5 mb-3">
          {CODEX_TEMPLATES.map((tpl) => (
            <button
              key={tpl.id}
              onClick={() => {
                setComposerId(tpl.id);
                setComposerFill("");
              }}
              className={cn(
                "inline-flex items-center gap-1 h-7 px-2.5 rounded-md text-[11px] font-medium border transition-colors",
                composerId === tpl.id
                  ? "border-accent bg-accent text-white"
                  : "border-white/[0.10] bg-white/[0.02] text-ink-2 hover:bg-white/[0.05]",
              )}
            >
              {t(tpl.title)}
            </button>
          ))}
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          {/* ② 引导三问 + 填空 */}
          <div className="rounded-lg border border-white/[0.06] bg-bg-1/60 px-4 py-3.5">
            <div className="text-[11px] font-semibold text-ink-2 mb-2 flex items-center gap-1.5">
              <ListChecks size={13} className="text-accent" /> {t("先想清楚这几点")}
            </div>
            <ul className="space-y-1 mb-3">
              {composerTpl.questions.map((q, i) => (
                <li key={i} className="flex gap-1.5 text-[11.5px] leading-snug text-ink-3">
                  <span className="text-accent">{i + 1}.</span>
                  <span>{t(q)}</span>
                </li>
              ))}
            </ul>
            <div className="text-[11px] font-semibold text-ink-2 mb-1.5">{t(composerTpl.fillHeading)}</div>
            <textarea
              value={composerFill}
              onChange={(e) => setComposerFill(e.target.value)}
              placeholder={t(composerTpl.fillPlaceholder)}
              rows={4}
              className="w-full resize-y rounded-md border border-white/[0.10] bg-bg-1 px-3 py-2 text-[12px] leading-relaxed text-ink-1 placeholder:text-ink-4 focus:border-accent focus:outline-none"
            />
          </div>

          {/* ③ 生成结果 */}
          <div className="flex flex-col rounded-lg border border-white/[0.06] bg-bg-1/60 px-4 py-3.5">
            <div className="flex items-center justify-between mb-2">
              <div className="text-[11px] font-semibold text-ink-2 flex items-center gap-1.5">
                <FileText size={13} className="text-accent" /> {t("生成的任务（复制进 Codex）")}
              </div>
              <button
                onClick={() => copyText(composedPrompt, onToast, t)}
                className="inline-flex h-7 items-center gap-1 rounded-md bg-accent px-2.5 text-[11px] font-semibold text-white hover:bg-accent-600"
              >
                <Clipboard size={12} /> {t("复制")}
              </button>
            </div>
            <pre className="flex-1 min-h-[150px] max-h-[220px] overflow-auto whitespace-pre-wrap rounded-md border border-white/[0.06] bg-bg-1 px-3 py-2 text-[10.5px] leading-relaxed text-ink-2">
              {composedPrompt}
            </pre>
            {!composerFill.trim() && (
              <p className="mt-1.5 text-[10px] text-ink-4">{t("上面还没填内容，现在复制会带示例占位；填一句你的真实需求会更准。")}</p>
            )}
          </div>
        </div>
      </section>

      {/* 任务模板库：全部标准模板，按分类找，可直接复制或送进生成器补细节。 */}
      <section
        id="codex-template-library"
        className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card"
      >
        <header className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2">
              <Clipboard size={15} className="text-accent" />
              {t("任务模板库")}
            </h3>
            <p className="mt-1 text-[11px] leading-relaxed text-ink-3">
              {t("{n} 个标准任务话术，重点是目标、约束和验收，不让 Codex 猜。", { n: CODEX_TEMPLATES.length })}
            </p>
          </div>
          <button
            onClick={() => copyText(CODEX_TEMPLATES.map((tpl) => `# ${t(tpl.title)}\n${composePrompt(tpl, "", t)}`).join("\n\n---\n\n"), onToast, t)}
            className="inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-md border border-white/[0.10] px-3 text-[12px] font-semibold text-ink-1 hover:bg-white/[0.04]"
          >
            <Clipboard size={13} />
            {t("全部复制")}
          </button>
        </header>

        {/* 分类筛选 */}
        <div className="flex flex-wrap gap-1.5 mb-3">
          {TEMPLATE_CATEGORIES.map((c) => (
            <button
              key={c}
              onClick={() => setTemplateCat(c)}
              className={cn(
                "h-6 px-2.5 rounded text-[11px] font-medium border transition-colors",
                templateCat === c
                  ? "border-accent bg-accent/15 text-accent"
                  : "border-white/[0.08] bg-white/[0.02] text-ink-3 hover:bg-white/[0.05]",
              )}
            >
              {t(c)}
            </button>
          ))}
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          {filteredTemplates.map((tpl) => (
            <div key={tpl.id} className="flex flex-col rounded-lg border border-white/[0.06] bg-white/[0.02] px-4 py-3.5">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="grid h-7 w-7 place-items-center rounded-md bg-accent/15 text-accent">
                      <FileText size={14} />
                    </span>
                    <div>
                      <div className="text-[13px] font-semibold text-ink-1">{t(tpl.title)}</div>
                      <div className="text-[10px] text-ink-4">{t(tpl.category)}</div>
                    </div>
                  </div>
                  <p className="mt-2 text-[11px] leading-snug text-ink-3">{t(tpl.summary)}</p>
                </div>
                <div className="flex shrink-0 flex-col gap-1.5">
                  <button
                    onClick={() => pickTemplate(tpl.id)}
                    className="inline-flex h-7 items-center gap-1 rounded-md bg-accent px-2.5 text-[11px] font-semibold text-white hover:bg-accent-600 whitespace-nowrap"
                  >
                    <Wand2 size={12} /> {t("用它生成")}
                  </button>
                  <button
                    onClick={() => copyText(composePrompt(tpl, "", t), onToast, t)}
                    className="inline-flex h-7 items-center gap-1 rounded-md border border-white/[0.10] px-2.5 text-[11px] font-medium text-ink-1 hover:bg-white/[0.04] whitespace-nowrap"
                  >
                    <Clipboard size={12} /> {t("复制")}
                  </button>
                </div>
              </div>
              <pre className="mt-3 max-h-[120px] overflow-hidden whitespace-pre-wrap rounded-md border border-white/[0.06] bg-bg-1 px-3 py-2 text-[10.5px] leading-relaxed text-ink-3">
                {composePrompt(tpl, "", t)}
              </pre>
            </div>
          ))}
        </div>
      </section>

      {/* Codex 装机卡片（复用工具市场动作） */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/80 backdrop-blur-sm px-5 py-5 shadow-card">
        <header className="mb-4 flex items-start justify-between gap-3">
          <div>
            <h3 className="text-[14px] font-semibold text-ink-0">{t("安装 Codex 桌面版")}</h3>
            <p className="text-[11px] text-ink-3 mt-0.5">
              {t("图形界面，内置 computer use。Windows 一键安装成功后会自动接虾盘云驱动；Mac 走官方 DMG，装好后点一键接入即可。")}
            </p>
          </div>
          {/* 常驻兜底：不管自动装机成功/失败/误判，都能点开图文手动教程（含商店链接 + 重启识别） */}
          <button
            onClick={openCodexGuide}
            className="shrink-0 inline-flex items-center gap-1 h-7 px-2.5 rounded-md border border-white/[0.10] text-[11px] text-ink-3 hover:text-accent hover:bg-white/[0.04] whitespace-nowrap"
          >
            <ExternalLink size={11} /> {t("装不上？手动安装教程")}
          </button>
        </header>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {codexTools.map((tool) => {
            const installed = tool.id === "codex-app" ? appInstalled || tool.installed : tool.installed;
            return (
                <div
                  key={tool.id}
                  className="flex flex-col rounded-lg border border-white/[0.06] bg-white/[0.02] hover:border-white/[0.10] px-4 py-3.5 transition-colors"
                >
                  <div className="flex items-start justify-between gap-2">
                    <span className="grid place-items-center w-9 h-9 rounded-md bg-accent text-white text-[14px] font-bold">
                      {tool.name.slice(0, 1)}
                    </span>
                    <span
                      className={cn(
                        "inline-flex items-center px-2 h-5 rounded text-[10px] font-mono uppercase tracking-wider border whitespace-nowrap",
                        installed
                          ? "bg-success-500/10 text-success-400 border-success-500/25"
                          : "bg-accent/15 text-accent border-white/[0.10]",
                      )}
                    >
                      {installed ? t("已安装") : IS_WINDOWS ? t("可一键安装") : t("手动安装")}
                    </span>
                  </div>
                  <div className="mt-2.5 text-[13px] font-medium text-ink-1">{tool.name}</div>
                  <p className="mt-1 text-[11px] text-ink-3 leading-snug line-clamp-2 min-h-[30px]">{tool.summary}</p>
                  <div className="mt-3 flex gap-2">
                    {installed && tool.launch_app ? (
                      <button
                        onClick={() => onLaunch(tool)}
                        className="flex-1 inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
                      >
                        <Bot size={13} /> {t("打开应用")}
                      </button>
                    ) : installed && tool.launch_cmd ? (
                      <button
                        onClick={() => onLaunch(tool)}
                        className="flex-1 inline-flex items-center justify-center gap-1.5 h-8 rounded-md bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
                      >
                        {t("打开终端")}
                      </button>
                    ) : (
                      <button
                        onClick={installCodexApp}
                        disabled={installing}
                        className="flex-1 inline-flex items-center justify-center gap-1.5 h-8 rounded-md border border-white/[0.10] text-ink-1 text-[12px] font-medium hover:bg-white/[0.04] disabled:opacity-50"
                      >
                        {IS_WINDOWS && installing ? <Loader2 size={13} className="animate-spin" /> : IS_WINDOWS ? <Download size={13} /> : <ExternalLink size={13} />}
                        {IS_WINDOWS ? (installing ? t("安装中…") : t("一键安装")) : t("安装教程")}
                      </button>
                    )}
                  </div>
                </div>
            );
          })}
        </div>
      </section>

      {/* 进阶选配入口：本地模型 / 自定义云驱动（默认虾盘云够用，这里给想折腾的人） */}
      <button
        onClick={openCodexLocalGuide}
        className="w-full inline-flex items-center justify-between px-4 py-3 rounded-lg border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] text-left"
      >
        <span className="flex items-center gap-2.5">
          <Cpu size={15} className="text-accent" />
          <span>
            <span className="text-[13px] font-medium text-ink-1">{t("高级配置 · 本地模型 / 自定义驱动")}</span>
            <span className="block text-[11px] text-ink-3 leading-snug">
              {t("用自己显卡离线跑开源模型（codex --oss），或接你自己的 OpenAI 兼容 API")}
            </span>
          </span>
        </span>
        <ExternalLink size={13} className="text-ink-3 shrink-0" />
      </button>
    </div>
  );
}
