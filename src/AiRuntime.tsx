// 「AI 优化大师」—— AI 时代的 Windows 优化大师（ukrt.exe 薄壳）。
// 设计对标：360/优化大师的「仪表盘 + 一键优化 + 成就感时刻」，配色走主题 token（bg/ink/accent）。
// 数字纪律：实测的标「实测」，推算的标「估算」——没对照组不吹具体百分比（开发宪法）。
// 铁律：只靠 props 通信（onToast）；删除本板块只动 App.tsx / Sidebar / lib.rs。
// 「让 AI 给优化建议」把纯内联体检摘要交给 App 的 u-chat handoff，由工作台自取发送。

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "./i18n";
import { ShareButton } from "./components/ShareCard";
// 「AI 工具专项修复」：Codex 工作站下架后，「这个 agent 装好没 / 跑得起来没 / 配对没」
// 这半边能力接到这儿。独立组件，删它只动本文件 + lib.rs（去 agent_launch_probe）。
import { AgentRepair } from "./AgentRepair";
import { ACTION, createTauriActionClient } from "./generated/action-client";
// 使用数据面板（metrics.rs 的 GUI 投影）。同样是独立组件，删它只动本文件。
import { MetricsPanel } from "./components/MetricsPanel";
import { buildDoctorPrompt } from "./airuntime/doctorPrompt";

const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

type CheckItem = {
  id: string;
  category: string;
  name: string;
  status: "pass" | "warn" | "fail" | "info";
  points: number;
  earned: number;
  bonus: number;
  detail: string;
  fix_hint: string;
};

type Doctor = {
  tool: string;
  version: string;
  score: number;
  earned: number;
  total: number;
  checks: CheckItem[];
};

/** 云端下发的「AI 工具最新提示」（fetch_optimize_advice / advice.rs）。改服务器即热更新，不发 exe。 */
type Advice = { id: string; title: string; detail: string; url: string; severity: string };

/** 整机一览快照 —— 复用共享命令 hardware.rs::detect_hardware，只取展示字段。
 *  本模块自包含：不跨 import 别的功能页（守模块独立铁律），直接调后端命令即可。 */
type HwGlance = {
  ram_total_mb: number;
  gpu_name: string | null;
  gpu_vram_mb: number | null;
  gpu_accelerated: boolean;
  cpu_cores: number;
  recommend: { model_label: string; can_run_decent: boolean };
};

/** 影核协议动作 runtime.command_guard.inspect 的共享状态；GUI 只投影，不自行判断 PATH。 */
type CommandGuard = {
  platform: string;
  shims_dir: string;
  conflicts: number;
  commands: { name: string; preferred_path: string | null; resolved_path: string | null; shadowed: boolean }[];
};

/** 影核协议动作 runtime.network.inspect 的共享状态；只显示脱敏后的配置，不做联网探测。 */
type RuntimeNetwork = {
  platform: string;
  system_proxy: string | null;
  environment_proxies: { name: string; endpoint: string }[];
  wsl: { executable_found: boolean; config_found: boolean; auto_proxy: boolean | null; mirrored_networking: boolean | null };
  warnings: string[];
};

/** 每个体检项的收益标签与量化口径。measured=实测(benchmarks/token-ab.md)，其余=估算。 */
const GAINS: Record<string, { tag: "省Token" | "防翻车" | "提速" | "基础"; note: string; measured?: boolean }> = {
  npmrc: { tag: "省Token", measured: true, note: "实测：含 deprecated 依赖的 npm install 少喂 AI ~145 token/次（581 字节），大项目更多" },
  "git-quotepath": { tag: "省Token", measured: true, note: "实测：中文目录的 git status 转义多喂 ~70 token/次，且转义码 AI 读不懂→易重读" },
  "claude-perms": { tag: "省Token", note: "每拦 1 次权限确认 ≈ 多耗 1 轮 API 往返（数千 token · 估算）" },
  "claude-md": { tag: "省Token", note: "AI 免重复摸索环境，每个会话省数千 token（估算）" },
  "ps-quiet": { tag: "省Token", note: "长命令输出刷屏进上下文，单次省数百 token（估算）" },
  acp: { tag: "防翻车", note: "GBK 乱码引发的重试，单次翻车 ≈ 上万 token + 3~10 分钟（估算）" },
  longpaths: { tag: "防翻车", note: "node_modules 深层路径翻车重试，单次 ≈ 上万 token（估算）" },
  "git-longpaths": { tag: "防翻车", note: "Git 深路径报错 → AI 多轮试错（估算）" },
  devmode: { tag: "防翻车", note: "pnpm 关着开发者模式会用 junction 兜底，通常不影响；仅 Next.js standalone 等硬要真软链才需要（要满分才开）" },
  home: { tag: "防翻车", note: "中文/空格用户目录是大量工具的翻车根源" },
  path: { tag: "提速", note: "PATH 干净 = 命令解析更快更稳" },
  pwsh7: { tag: "提速", note: "PowerShell 7 的 UTF-8 支持远好于 5.1" },
  wt: { tag: "提速", note: "Windows Terminal 渲染 UTF-8/emoji 不出豆腐块" },
  git: { tag: "基础", note: "Claude Code 必需组件" },
  "git-bash": { tag: "基础", note: "Claude Code 的 Bash 工具依赖" },
  node: { tag: "基础", note: "AI CLI 工具运行时" },
  claude: { tag: "基础", note: "用装机向导一键安装" },
  codex: { tag: "基础", note: "用装机向导一键安装" },
};

const TAG_STYLE: Record<string, string> = {
  省Token: "bg-success-500/15 text-success-400",
  防翻车: "bg-danger-500/15 text-danger-400",
  提速: "bg-accent/15 text-accent",
  基础: "bg-white/[0.06] text-ink-3",
};

function ScoreRing({ score, busy }: { score: number; busy: boolean }) {
  const { t } = useI18n();
  const r = 56;
  const c = 2 * Math.PI * r;
  const color = score >= 80 ? "#27a644" : score >= 50 ? "rgb(var(--color-accent))" : "#eb5757";
  return (
    <svg width="148" height="148" viewBox="0 0 148 148" className={busy ? "animate-pulse" : ""}>
      <circle cx="74" cy="74" r={r} fill="none" stroke="rgba(128,128,128,0.15)" strokeWidth="11" />
      <circle
        cx="74"
        cy="74"
        r={r}
        fill="none"
        stroke={color}
        strokeWidth="11"
        strokeLinecap="round"
        strokeDasharray={`${(Math.max(score, 2) / 100) * c} ${c}`}
        transform="rotate(-90 74 74)"
        style={{ transition: "stroke-dasharray 900ms cubic-bezier(.4,0,.2,1)" }}
      />
      <text x="74" y="72" textAnchor="middle" fill="currentColor" fontSize="38" fontWeight="700">
        {score}
      </text>
      <text x="74" y="94" textAnchor="middle" fill="currentColor" opacity="0.45" fontSize="11">
        {t("AI 战斗力 / 100")}
      </text>
    </svg>
  );
}

/** 分数 → 你超过了百分之几的电脑。锚点来自实测样本（裸机10 / 典型客户31 / 开发机未优化61 /
 *  优化后76 / 满配90+），分段线性插值。服务端有真实分布时以服务端返回为准。 */
function localPercentile(score: number): number {
  const pts: [number, number][] = [
    [0, 0], [10, 8], [30, 30], [50, 55], [65, 72], [76, 85], [90, 96], [100, 99],
  ];
  for (let i = 1; i < pts.length; i++) {
    const [x1, y1] = pts[i - 1];
    const [x2, y2] = pts[i];
    if (score <= x2) {
      const t = (score - x1) / (x2 - x1 || 1);
      return Math.round(y1 + t * (y2 - y1));
    }
  }
  return 99;
}

/** MB → "N GB"（整机一览用；本模块自包含不跨模块 import）。 */
function fmtGb(mb: number): string {
  return mb ? `${(mb / 1024).toFixed(0)} GB` : "—";
}

export function AiRuntime({ onToast, onGoSetup, onAskAI }: { onToast?: (msg: string) => void; onGoSetup?: () => void; onAskAI?: (prompt: string) => void }) {
  const { t } = useI18n();
  const [report, setReport] = useState<Doctor | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [lastLog, setLastLog] = useState<string>("");
  const [delta, setDelta] = useState<{ from: number; to: number; fixed: number } | null>(null);
  const [percentile, setPercentile] = useState<number | null>(null);
  const [advice, setAdvice] = useState<Advice[]>([]);
  const [hw, setHw] = useState<HwGlance | null>(null);
  const [commandGuard, setCommandGuard] = useState<CommandGuard | null>(null);
  const [runtimeNetwork, setRuntimeNetwork] = useState<RuntimeNetwork | null>(null);

  /** 体检引擎 = 平台：Windows 走 ukrt.exe，macOS 走原生 macopt（lib.rs::airuntime_doctor_routed）。
   *  这一个字段就够判平台了 —— 别在前端另起一套 platform 判定，两份判定迟早会漂。 */
  const isMac = report?.tool === "macopt";

  /** 匿名上报分数（仅 score+version+os，无设备指纹）→ 拿服务端真实百分位；失败则用本地表。 */
  const reportScore = useCallback(async (d: Doctor) => {
    setPercentile(localPercentile(d.score)); // 先用本地表即时显示
    try {
      const p = await invoke<number | null>("airuntime_report_score", { score: d.score, version: d.version });
      if (typeof p === "number" && p >= 0) setPercentile(p); // 服务端有真实分布则覆盖
    } catch {
      /* 上报失败不影响使用（fire-and-forget） */
    }
  }, []);

  const doctor = useCallback(async (): Promise<Doctor | null> => {
    setBusy((b) => b ?? "doctor");
    try {
      const raw = await invoke<string>("airuntime_doctor");
      const d = JSON.parse(raw) as Doctor;
      setReport(d);
      setErr(null);
      void reportScore(d);
      return d;
    } catch (e) {
      setErr(String(e));
      return null;
    } finally {
      setBusy((b) => (b === "doctor" ? null : b));
    }
  }, [reportScore]);

  useEffect(() => {
    void doctor();
  }, [doctor]);

  // 云端「AI 工具最新提示」：只读探针命中的才返回；网络失败静默（回落内嵌兜底），不影响体检。
  useEffect(() => {
    invoke<Advice[]>("fetch_optimize_advice")
      .then(setAdvice)
      .catch(() => {});
  }, []);

  // 整机一览：一次性硬件快照（复用 detect_hardware）——给「打开就看到整机家底」的 Kudu 式一览。
  // 只读、失败静默，不影响体检主流程。
  useEffect(() => {
    invoke<HwGlance>("detect_hardware").then(setHw).catch(() => {});
  }, []);

  // 影核协议：桌面 GUI 与无头 CLI 只调用同一个 Action Core，不在前端复制 PATH 判定规则。
  useEffect(() => {
    callAction(ACTION.RUNTIME_COMMAND_GUARD_INSPECT, {})
      .then((envelope) => envelope.ok && setCommandGuard(envelope.result as unknown as CommandGuard))
      .catch(() => {});
  }, []);

  // 影核协议：不在 GUI 自己读环境变量；网络与 WSL 的状态与 CLI 来自同一只读 Action Core。
  useEffect(() => {
    callAction(ACTION.RUNTIME_NETWORK_INSPECT, {})
      .then((envelope) => envelope.ok && setRuntimeNetwork(envelope.result as unknown as RuntimeNetwork))
      .catch(() => {});
  }, []);

  /** 一键优化 = fix + optimize（改配置）+ 补装缺失环境基础件（便携 Node/Git）。
   *  配置改动各自 journal 一档，undo 可回滚；Node/Git 是便携安装到 ~/.uking/runtime（免管理员）。
   *  为什么要装：只「指出」缺 git/node 却让用户自己去别处装，一键优化就成了摆设——它指出的能修的就得修。 */
  const optimizeAll = async () => {
    const before = report?.score ?? 0;
    const beforeMiss = report?.checks.filter((c) => c.status === "warn" || c.status === "fail").length ?? 0;
    setBusy("optimize");
    setDelta(null);
    // 环境组件安装进度（便携 Git ~60MB，首次要下载）—— 实时贴到「本次改动明细」，别让用户以为卡死。
    const un = await listen<string>("uking:optimize_env", (e) => {
      setLastLog((prev) => (prev ? prev + "\n  " : "") + String(e.payload));
    });
    try {
      const log1 = await invoke<string>("airuntime_run", { action: "fix" });
      const log2 = await invoke<string>("airuntime_run", { action: "optimize" });
      setLastLog(
        log1 + "\n" + log2 + "\n\n" +
          (isMac
            ? t("[环境组件] 检查 / 补装 Node 与 Git（Apple 命令行开发者工具）…")
            : t("[环境组件] 检查 / 补装 Node、Git（含 Bash）与 PowerShell 7（首次需下载，请稍候）…"))
      );
      onToast?.(isMac ? t("正在补装缺失的环境组件（Git / Node）…") : t("正在补装缺失的环境组件（Git / Node / PowerShell 7）…"));
      let env: { node: string; git: string; pwsh: string; command_guard: string } = { node: "?", git: "?", pwsh: "?", command_guard: "?" };
      try {
        env = await invoke<{ node: string; git: string; pwsh: string; command_guard: string }>("optimize_env");
      } catch (e) {
        onToast?.(t("环境组件安装未完成：{e}", { e: String(e) }));
      }
      const tag = (s: string) => (s === "ok" ? t("✔ 就绪") : s === "skip" ? t("— 跳过") : s.replace(/^fail:\s*/, "✖ "));
      // 哪几件事做了，以**后端返回的 skip** 为准，不在前端重算一遍平台规则：
      // Mac 上 pwsh / command_guard 都是 Windows 专属，后端返回 skip → 这里干脆不提。
      // 以前一律照排，于是 Mac 用户在「AI 优化大师」里看到的是「PowerShell 7：— 跳过」，
      // 一句跟他机器毫无关系的话（用户原话：「好像不对，优化 PowerShell 去了」）。
      const envParts = [
        t("Node：{node}", { node: tag(env.node) }),
        isMac ? t("Git：{git}", { git: tag(env.git) }) : t("Git（含 Bash）：{git}", { git: tag(env.git) }),
      ];
      if (env.pwsh !== "skip") envParts.push(t("PowerShell 7：{pwsh}", { pwsh: tag(env.pwsh) }));
      if (env.command_guard !== "skip") envParts.push(t("CLI 命令优先级：{guard}", { guard: tag(env.command_guard) }));
      setLastLog((prev) => prev + "\n\n" + t("[环境组件] ") + envParts.join(" · "));
      const after = await doctor();
      if (after) {
        const afterMiss = after.checks.filter((c) => c.status === "warn" || c.status === "fail").length;
        setDelta({ from: before, to: after.score, fixed: Math.max(0, beforeMiss - afterMiss) });
        onToast?.(
          after.score > before
            ? t("优化完成：AI 战斗力 {before} → {after}（+{delta}）", { before, after: after.score, delta: after.score - before })
            : t("已是当前可优化的最佳状态")
        );
      }
    } catch (e) {
      setErr(String(e));
      onToast?.(t("优化失败：{e}", { e: String(e) }));
    } finally {
      un();
      setBusy(null);
    }
  };

  /** 以管理员拿满分：弹一次 UAC 跑 ukrt fix，开 longpaths + devmode 这两个 HKLM 项。
   *  这是「到 100 分」绕不开的一步——非管理员改不了 HKLM，不提权天花板约 87~96 分。
   *  改动复用 ukrt 的 journal 可回滚；UAC 被取消则后端返回 Err，这里 toast 提示不打断。 */
  const fixElevated = async () => {
    const before = report?.score ?? 0;
    const beforeMiss = report?.checks.filter((c) => c.status === "warn" || c.status === "fail").length ?? 0;
    setBusy("elevated");
    setDelta(null);
    try {
      const out = await invoke<string>("airuntime_fix_elevated");
      setLastLog(out);
      const after = await doctor();
      if (after) {
        const afterMiss = after.checks.filter((c) => c.status === "warn" || c.status === "fail").length;
        setDelta({ from: before, to: after.score, fixed: Math.max(0, beforeMiss - afterMiss) });
        onToast?.(
          after.score > before
            ? t("已提权修复：AI 战斗力 {before} → {after}（+{delta}）", { before, after: after.score, delta: after.score - before })
            : t("已处理（分数无变化）")
        );
      }
    } catch (e) {
      // 常见是用户在 UAC 弹窗点了「否」——后端返回中文说明，直接透传给用户。
      onToast?.(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(null);
    }
  };

  const undo = async () => {
    setBusy("undo");
    setDelta(null);
    try {
      const out = await invoke<string>("airuntime_run", { action: "undo" });
      setLastLog(out);
      await doctor();
      onToast?.(t("已回滚最近一次优化（可连续点撤销更早的）"));
    } catch (e) {
      onToast?.(t("回滚失败：{e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  /** 加杀软白名单（opt-in·提速）：AI 工具目录进 Defender 排除，需管理员。 */
  const addDefender = async () => {
    setBusy("defender");
    try {
      const out = await invoke<string>("airuntime_run", { action: "defender" });
      setLastLog(out);
      await doctor();
      onToast?.(out.includes("✔") ? t("已加杀软白名单，npm/装机更快（可一键复原）") : t("需要管理员权限：右键 U-King → 以管理员身份运行后重试"));
    } catch (e) {
      onToast?.(t("加白名单失败：{e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  // —— 错误卡：ukrt 缺失/崩溃只影响本板块（进程级隔离） ——
  if (err && !report) {
    return (
      <div className="p-6 max-w-3xl">
        <h2 className="text-[15px] font-semibold text-ink-0 mb-3">{t("AI 优化大师")}</h2>
        <div className="rounded-card border border-danger-500/30 bg-danger-500/[0.06] p-4 text-[13px] text-ink-1">
          <div className="font-medium mb-1">{t("优化引擎未就绪")}</div>
          <div className="text-ink-3 break-all">{String(err)}</div>
          <button className="mt-3 px-3 py-1.5 rounded-card border border-white/10 hover:bg-white/[0.04]" onClick={() => void doctor()}>
            {t("重试")}
          </button>
        </div>
      </div>
    );
  }

  if (!report) {
    return (
      <div className="p-8 flex items-center gap-3 text-[13px] text-ink-3">
        <span className="inline-block w-3.5 h-3.5 rounded-full border-2 border-accent border-t-transparent animate-spin" />
        {t("正在扫描本机 AI 环境…（只读，不改任何配置）")}
      </div>
    );
  }

  const misses = report.checks
    .filter((c) => c.status === "warn" || c.status === "fail")
    .sort((a, b) => b.bonus - a.bonus);
  const passes = report.checks.filter((c) => c.status === "pass");
  // 不计分提示（如中文用户名——无法自动修，但不再永久扣分把满分卡死；仍要露出来别藏着）
  const infos = report.checks.filter((c) => c.status === "info");
  const fixableBonus = misses.reduce((s, c) => s + c.bonus, 0);
  const nToken = misses.filter((c) => GAINS[c.id]?.tag === "省Token").length;
  const nCrash = misses.filter((c) => GAINS[c.id]?.tag === "防翻车").length;

  return (
    <div className="p-6 max-w-3xl space-y-4 overflow-y-auto h-full text-ink-0">
      {/* ===== 主仪表盘 ===== */}
      <div className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card p-5 flex items-center gap-6">
        <ScoreRing score={report.score} busy={busy !== null} />
        <div className="flex-1 min-w-0">
          <div className="text-[16px] font-semibold">{t("AI 优化大师")}</div>
          <div className="text-[11px] text-ink-4 mt-0.5">{t("AI 时代的电脑优化大师 · 让 Claude / Codex 跑得更稳、更省")}</div>
          <div className="text-[12px] text-ink-2 mt-2.5">
            {t("已达标 ")}<b className="text-ink-0">{passes.length}</b>{t(" 项 · 可优化 ")}<b className="text-ink-0">{misses.length}</b>{t(" 项")}
            {fixableBonus > 0 && (
              <span>
                {" "}
                {t("· 还能再涨 ")}<b className="text-accent">{t("+{n} 分", { n: fixableBonus })}</b>
              </span>
            )}
          </div>
          {percentile != null && (
            <div className="text-[11px] text-ink-4 mt-1">
              {t("🏆 你的 AI 战斗力超过了全国 ")}<b className="text-accent">{percentile}%</b>{t(" 的电脑")}
              <span className="text-ink-5">{t("（基于已收集样本）")}</span>
            </div>
          )}
          <div className="flex items-center gap-2.5 mt-3.5 flex-wrap">
            <button
              className="px-4 py-2 rounded-card bg-accent hover:bg-accent-600 text-white text-[13px] font-medium shadow-pop disabled:opacity-40 transition-colors"
              disabled={busy !== null || misses.length === 0}
              onClick={() => void optimizeAll()}
            >
              {busy === "optimize" ? t("正在优化…") : misses.length === 0 ? t("已是最佳状态") : t("一键优化")}
            </button>
            <button
              className="px-3.5 py-2 rounded-card border border-accent/40 bg-accent/[0.08] text-[12px] font-medium text-accent hover:bg-accent/[0.16] disabled:opacity-40"
              disabled={busy !== null}
              onClick={() => { onAskAI?.(buildDoctorPrompt(report)); onToast?.(t("已把体检结果交给 AI，正在打开工作台")); }}
            >
              {t("让 AI 给优化建议")}
            </button>
            {/* 到 100 分绕不开的一步：longpaths / devmode 是系统级 HKLM 注册表项，非管理员改不了。
                仅当这两项还红时才露出（免提权能修的都归「一键优化」，别无谓弹 UAC）。macOS 无 HKLM，隐藏。 */}
            {report.tool !== "macopt" &&
              report.checks.some((c) => (c.id === "longpaths" || c.id === "devmode") && (c.status === "warn" || c.status === "fail")) && (
                <button
                  className="px-3.5 py-2 rounded-card border border-accent/40 bg-accent/[0.08] text-[12px] font-medium text-accent hover:bg-accent/[0.16] disabled:opacity-40"
                  disabled={busy !== null}
                  title={t("长路径 / 开发者模式是系统级注册表项，需管理员才能开——这是到 100 分绕不开的一步。弹一次 UAC，改动记入 journal 可回滚。")}
                  onClick={() => void fixElevated()}
                >
                  {busy === "elevated" ? t("提权中…") : t("以管理员拿满分（弹一次 UAC）")}
                </button>
              )}
            <button
              className="px-3 py-2 rounded-card border border-white/10 text-[12px] text-ink-2 hover:bg-white/[0.04] disabled:opacity-40"
              disabled={busy !== null}
              onClick={() => void undo()}
            >
              {t("一键复原")}
            </button>
            {/* 杀软白名单是 Windows Defender 专属；macOS（tool=macopt）无此概念，隐藏 */}
            {report.tool !== "macopt" && (
              <button
                className="px-3 py-2 rounded-card border border-white/10 text-[12px] text-ink-2 hover:bg-white/[0.04] disabled:opacity-40"
                disabled={busy !== null}
                title={t("把 AI 工具目录加进 Windows Defender 排除，npm/装机不再被实时扫描拖慢（需管理员，可一键复原）")}
                onClick={() => void addDefender()}
              >
                {busy === "defender" ? t("处理中…") : t("加杀软白名单·提速")}
              </button>
            )}
            <button
              className="px-3 py-2 rounded-card text-[12px] text-ink-4 hover:text-ink-2 disabled:opacity-40"
              disabled={busy !== null}
              onClick={() => void doctor()}
            >
              {t("重新体检")}
            </button>
            <ShareButton
              onToast={(m) => onToast?.(m)}
              label={t("晒体检分")}
              className="px-3 py-2 rounded-card border border-accent/40 bg-accent/[0.08] text-[12px] font-medium text-accent hover:bg-accent/[0.16] inline-flex items-center gap-1.5 disabled:opacity-40"
              spec={{
                kind: "ai-score",
                badge: t("电脑 AI 就绪度"),
                title: t("我的电脑 AI 优化到"),
                heroValue: String(report.score),
                heroUnit: t("分"),
                heroSub:
                  percentile != null ? t("🏆 超过全国 {p}% 的电脑", { p: String(percentile) }) : undefined,
                stats: [
                  { label: t("已达标"), value: String(passes.length) },
                  { label: t("待优化"), value: String(misses.length) },
                ],
                footnote: t("AI 优化大师 · 一键优化全可回滚"),
              }}
            />
          </div>
        </div>
      </div>

      {/* ===== 使用数据（metrics.rs 的 GUI 投影）=====
          放在体检卡下面、整机一览上面：先看「该修什么」，再看「实际用得怎么样」。
          它自带加载态和空态，任何时候都能渲染，不需要外面兜条件。 */}
      <MetricsPanel
        onToast={onToast}
        onRunAction={(action) => {
          // 建议卡上的「去修复」→ 复用页面上已有的那几条真实路径，不另写一份
          if (action === "defender") void addDefender();
          else void optimizeAll();
        }}
      />

      {/* ===== 整机一览（借鉴 Kudu「打开就看到整机」；复用 detect_hardware 快照，AI 视角）=====
          现为静态家底（内存/显卡/CPU）+ 本地能跑多大的一句话。实时占用条（CPU%/磁盘）需后端
          sysinfo 采样，留作下一步（见记忆 uking-airuntime-kudu-machine-dashboard）。 */}
      {hw && (
        <div className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card p-4">
          <div className="text-[12px] font-medium text-ink-2 mb-2.5 flex items-center gap-2">
            {t("整机一览")}
            <span className="text-[10px] text-ink-5">{t("· 这台机器跑 AI 的家底")}</span>
          </div>
          <div className="grid grid-cols-3 gap-2.5">
            <div className="rounded-lg border border-white/[0.06] bg-bg-1/40 px-3 py-2.5">
              <div className="text-[10px] text-ink-4">💾 {t("内存")}</div>
              <div className="text-[15px] font-semibold text-ink-0 mt-0.5">{fmtGb(hw.ram_total_mb)}</div>
            </div>
            <div className="rounded-lg border border-white/[0.06] bg-bg-1/40 px-3 py-2.5">
              <div className="text-[10px] text-ink-4">🎮 {t("显卡")}</div>
              <div className={"text-[15px] font-semibold mt-0.5 " + (hw.gpu_accelerated ? "text-success-400" : "text-ink-0")}>
                {hw.gpu_accelerated ? (hw.gpu_vram_mb ? fmtGb(hw.gpu_vram_mb) : t("可加速")) : t("无独显")}
              </div>
              {hw.gpu_name && (
                <div className="text-[9.5px] text-ink-5 mt-0.5 truncate" title={hw.gpu_name}>{hw.gpu_name}</div>
              )}
            </div>
            <div className="rounded-lg border border-white/[0.06] bg-bg-1/40 px-3 py-2.5">
              <div className="text-[10px] text-ink-4">⚙️ CPU</div>
              <div className="text-[15px] font-semibold text-ink-0 mt-0.5">{t("{n} 核", { n: hw.cpu_cores })}</div>
            </div>
          </div>
          {/* AI 视角一句话：本地能跑多大 + 何时该用云端（承接「本地大模型」板块口径） */}
          <div className="text-[11px] text-ink-3 mt-2.5 leading-relaxed">
            {hw.recommend.can_run_decent
              ? t("本机本地可跑：{m}（想要满血效果仍建议用虾盘云）", { m: hw.recommend.model_label })
              : t("本机建议直接用「虾盘云」云端满血模型；本地大模型会偏慢")}
          </div>
        </div>
      )}

      {/* ===== CLI 命令优先级（影核协议 Action Core 的 GUI 投影） =====
          只在 Windows 出：`inspect_cli_command_guard` 的非 Windows 分支是个空壳
          （commands 恒为 []、conflicts 恒为 0），照渲染就会在 Mac 上显示
          「✓ 没有检测到启动器抢占」—— 一句**根本没查过**的绿话。宁可不出，不出错。 */}
      {commandGuard && commandGuard.platform === "windows" && (
        <div data-action-id={ACTION.RUNTIME_COMMAND_GUARD_INSPECT} className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card p-4">
          <div className="flex items-start gap-2.5">
            <span className={"mt-0.5 text-[13px] " + (commandGuard.conflicts ? "text-danger-400" : "text-success-400")}>
              {commandGuard.conflicts ? "⚠" : "✓"}
            </span>
            <div className="min-w-0">
              <div className="text-[13px] font-medium">{t("AI 命令优先级")}</div>
              <div className="text-[11px] text-ink-3 mt-0.5 leading-relaxed">
                {commandGuard.conflicts
                  ? t("发现 {n} 个命令被旧启动器抢占；点「一键优化」会建立 U-King 安全转发器，原文件不会删除。", { n: commandGuard.conflicts })
                  : t("Claude / Codex / OpenClaw / Hermes 当前没有检测到启动器抢占。")}
              </div>
              {commandGuard.conflicts > 0 && (
                <div className="text-[10px] text-ink-5 mt-1.5 break-all">
                  {commandGuard.commands.filter((c) => c.shadowed).map((c) => `${c.name}: ${c.resolved_path}`).join(" · ")}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ===== 网络 / WSL 代理体检（影核协议 Action Core 的 GUI 投影） =====
          同样只在 Windows 出：WSL 是 Windows 概念，且 `inspect_runtime_network` 的非 Windows
          分支全是 None/空 → Mac 上会显示「✓ 没有发现明显冲突」的假绿。Mac 的代理诊断由
          macopt::proxy_check() 出（会认小火箭/Clash 这些进程），落在下面的「环境提示」里。 */}
      {runtimeNetwork && runtimeNetwork.platform === "windows" && (
        <div data-action-id={ACTION.RUNTIME_NETWORK_INSPECT} className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card p-4">
          <div className="flex items-start gap-2.5">
            <span className={"mt-0.5 text-[13px] " + (runtimeNetwork.warnings.length ? "text-danger-400" : "text-success-400")}>
              {runtimeNetwork.warnings.length ? "⚠" : "✓"}
            </span>
            <div className="min-w-0">
              <div className="text-[13px] font-medium">{t("网络与 WSL 代理")}</div>
              <div className="text-[11px] text-ink-3 mt-0.5 leading-relaxed">
                {runtimeNetwork.warnings.length
                  ? t("发现 {n} 个可能让终端 AI 工具表现不一致的配置；当前只检查，不会改代理。", { n: runtimeNetwork.warnings.length })
                  : t("当前进程代理配置没有发现明显冲突。")}
              </div>
              <div className="text-[10px] text-ink-5 mt-1.5 leading-relaxed">
                {runtimeNetwork.system_proxy ? `${t("系统代理")}：${runtimeNetwork.system_proxy}` : t("系统代理：未开启")}
                {runtimeNetwork.environment_proxies.length > 0 && ` · ${t("进程代理")}：${runtimeNetwork.environment_proxies.map((p) => `${p.name}=${p.endpoint}`).join(" · ")}`}
                {runtimeNetwork.wsl.executable_found && ` · WSL：${runtimeNetwork.wsl.auto_proxy === true ? "autoProxy ✓" : runtimeNetwork.wsl.auto_proxy === false ? "autoProxy 已关闭" : "autoProxy 使用默认值"}`}
              </div>
              {runtimeNetwork.warnings.map((warning) => (
                <div key={warning} className="text-[10px] text-danger-400/90 mt-1.5 leading-relaxed">{warning}</div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ===== 云端「AI 工具新坑速报」（改服务器 JSON 即热更新，不发 exe；只读探针命中才显示） ===== */}
      {advice.length > 0 && (
        <div className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card">
          <div className="px-4 py-2.5 border-b border-white/[0.06] text-[12px] font-medium text-ink-2 flex items-center gap-2">
            {t("AI 工具新坑速报")}
            <span className="text-[10px] text-ink-5">{t("· 云端更新，无需升级 App")}</span>
          </div>
          <div className="divide-y divide-white/[0.04]">
            {advice.map((a) => (
              <div key={a.id} className="px-4 py-2.5 flex items-start gap-2.5">
                <span className={"mt-0.5 text-[13px] " + (a.severity === "warn" ? "text-danger-400" : "text-accent")}>
                  {a.severity === "warn" ? "⚠" : "💡"}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] font-medium">{a.title}</div>
                  <div className="text-[11px] text-ink-3 mt-0.5 whitespace-pre-wrap leading-relaxed">{a.detail}</div>
                  {a.url && (
                    <button
                      onClick={() => void openUrl(a.url).catch(() => {})}
                      className="mt-1.5 text-[11px] text-accent hover:underline"
                    >
                      {t("查看详情 →")}
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ===== 成就感时刻 ===== */}
      {delta && delta.to > delta.from && (
        <div className="rounded-card border border-success-500/30 bg-success-500/[0.07] p-4 animate-fade-in">
          <div className="text-[15px] font-semibold text-success-400">
            {t("🎉 优化完成！AI 战斗力 {from} → {to}", { from: delta.from, to: delta.to })}
            <span className="ml-1.5 text-[13px]">{t("（+{delta} 分 · 修复 {fixed} 项）", { delta: delta.to - delta.from, fixed: delta.fixed })}</span>
          </div>
          <div className="text-[12px] text-ink-2 mt-1">
            {isMac
              ? t("配置改动已留底可「一键复原」；缺的 Node 已自动补齐，缺 Git 会弹 Apple「命令行开发者工具」安装窗。还缺 Claude Code / Codex 的，用装机向导一键装。")
              : t("配置改动已留底可「一键复原」；缺的 Git / Node 已自动补齐。还缺 Claude Code / Codex 的，用装机向导一键装。")}
          </div>
        </div>
      )}

      {/* ===== 收益预估（诚实标注） ===== */}
      {(nToken > 0 || nCrash > 0) && !delta && (
        <div className="grid grid-cols-3 gap-2.5">
          <div className="rounded-card border border-white/[0.06] bg-bg-2 p-3">
            <div className="text-[11px] text-ink-4">{t("省 Token 空间")}</div>
            <div className="text-[15px] font-semibold text-success-400 mt-0.5">{t("{n} 处浪费点", { n: nToken })}</div>
            <div className="text-[10px] text-ink-5 mt-0.5">{t("权限往返 / 噪音输出 / 重复摸索")}</div>
          </div>
          <div className="rounded-card border border-white/[0.06] bg-bg-2 p-3">
            <div className="text-[11px] text-ink-4">{t("翻车风险")}</div>
            <div className="text-[15px] font-semibold text-danger-400 mt-0.5">{t("{n} 个隐患", { n: nCrash })}</div>
            <div className="text-[10px] text-ink-5 mt-0.5">{t("单次翻车 ≈ 上万 token + 几分钟等待")}</div>
          </div>
          <div className="rounded-card border border-white/[0.06] bg-bg-2 p-3">
            <div className="text-[11px] text-ink-4">{t("修复方式")}</div>
            <div className="text-[15px] font-semibold text-ink-0 mt-0.5">{t("全部可回滚")}</div>
            <div className="text-[10px] text-ink-5 mt-0.5">{t("改前留底 · journal 记录 · 体检只读")}</div>
          </div>
        </div>
      )}

      {/* ===== 可优化项 ===== */}
      {misses.length > 0 && (
        <div className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card">
          <div className="px-4 py-2.5 border-b border-white/[0.06] text-[12px] font-medium text-ink-2">
            {t("可优化项（{n}）", { n: misses.length })}
          </div>
          <div className="divide-y divide-white/[0.04]">
            {misses.map((c) => {
              const g = GAINS[c.id];
              return (
                <div key={c.id} className="px-4 py-2.5 flex items-start gap-3">
                  <span className={"mt-0.5 text-[13px] " + (c.status === "fail" ? "text-danger-400" : "text-ink-4")}>
                    {c.status === "fail" ? "✖" : "△"}
                  </span>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-[13px] font-medium">{c.name}</span>
                      <span className="text-[11px] text-accent font-medium">{t("+{n} 分", { n: c.bonus })}</span>
                      {g && <span className={"text-[10px] px-1.5 py-0.5 rounded " + TAG_STYLE[g.tag]}>{t(g.tag)}</span>}
                      {g?.measured && <span className="text-[10px] px-1.5 py-0.5 rounded bg-success-500/15 text-success-400">{t("实测")}</span>}
                    </div>
                    <div className="text-[11px] text-ink-3 mt-0.5">{c.detail}</div>
                    {g && <div className="text-[10px] text-ink-5 mt-0.5">{t(g.note)}</div>}
                    {/* 每个缺分项都亮出「怎么拿这分」——别让人对着分数发愁却不知道去哪修 */}
                    {c.fix_hint && <div className="text-[10px] text-accent/90 mt-1">{t("→ 怎么拿：")}{c.fix_hint}</div>}
                  </div>
                  {/* claude/codex 这类 AI 工具是重下载，交给装机向导（有进度 + 自动修复），
                      别在优化大师里静默硬装；git/node 已由「一键优化」直接补上。 */}
                  {(c.id === "claude" || c.id === "codex") && onGoSetup && (
                    <button
                      onClick={onGoSetup}
                      className="shrink-0 self-center px-2.5 py-1 rounded-card border border-accent/40 bg-accent/[0.08] text-[11px] text-accent hover:bg-accent/[0.16]"
                    >
                      {t("去装机向导安装")}
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* ===== 不计分提示（中文用户名等无法自动修的项：不拖累满分，但明确摆出来） ===== */}
      {infos.length > 0 && (
        <div className="rounded-card border border-accent/20 bg-accent/[0.04]">
          <div className="px-4 py-2.5 border-b border-white/[0.06] text-[12px] font-medium text-ink-2">
            {t("环境提示（不计分 · 不影响满分）")}
          </div>
          <div className="divide-y divide-white/[0.04]">
            {infos.map((c) => (
              <div key={c.id} className="px-4 py-2.5 flex items-start gap-2.5">
                <span className="mt-0.5 text-[12px] text-ink-4">·</span>
                <div className="min-w-0">
                  <div className="text-[12px] text-ink-2">{c.name}</div>
                  <div className="text-[11px] text-ink-4 mt-0.5">{c.detail}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ===== AI 工具专项修复（Codex 工作站下架后的去处） ===== */}
      <AgentRepair onToast={onToast} onGoSetup={onGoSetup} />

      {/* ===== 已达标（收起） ===== */}
      <details className="rounded-card border border-white/[0.06] bg-bg-2">
        <summary className="px-4 py-2.5 text-[12px] text-ink-3 cursor-pointer select-none">
          {t("已达标（{n}）", { n: passes.length })}
        </summary>
        <div className="px-4 pb-3 space-y-1">
          {passes.map((c) => (
            <div key={c.id} className="text-[11px] text-ink-4">
              <span className="text-success-400 mr-1.5">✔</span>
              {c.name} · {c.detail}
            </div>
          ))}
        </div>
      </details>

      {/* ===== 本次改动明细 ===== */}
      {lastLog && (
        <details className="rounded-card border border-white/[0.06] bg-bg-2" open>
          <summary className="px-4 py-2.5 text-[12px] text-ink-3 cursor-pointer select-none">{t("本次改动明细")}</summary>
          <pre className="px-4 pb-3 text-[11px] text-ink-3 whitespace-pre-wrap leading-relaxed">{lastLog}</pre>
        </details>
      )}

      <div className="text-[10px] text-ink-5 pb-2">
        {/* 引擎名和 journal 落点都随平台走：Mac 是 macopt，写死 ukrt 会让 Mac 用户照着去找一个
            根本不存在的目录（~/.uking/ukrt/journal），也让「引擎 ukrt」这个 Windows 名字凭空出现。 */}
        {t("体检为只读操作 · 所有优化改前留底（~/.uking/{tool}/journal）· 「一键复原」按次回滚 · 引擎 {tool} v{ver}", {
          tool: report.tool || "ukrt",
          ver: report.version,
        })}
      </div>
    </div>
  );
}
