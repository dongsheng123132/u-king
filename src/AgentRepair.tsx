/**
 * 「AI 工具专项修复」—— AI 优化大师里的一块，专治**装着却用不了**。
 *
 * ## 为什么要有这一块（0.9.83）
 * 「Codex 工作站」下架后，它身上真正有价值的那半边（这个工具装好没、配对没、跑得起来没、
 * 跑不起来怎么办）得有地方接。接在「AI 优化大师」下面最顺：那一页本来就是「让 AI 工具
 * 跑得更稳」，只不过以前只体检**环境**（node/git/PATH），不体检**具体某个 agent**。
 *
 * ## 它和 detect_stack 的分工（别合并）
 * `detect_stack` 回答**装没装**。这一块回答**能不能用** —— 两者可以同时为真和为假：
 * 0.9.83 修的那个 P0 就是活证据：claude 装着、`--version` 正常，但只要提示词带换行就
 * 一个字都跑不出来（批处理壳拒收含换行的参数），多行提问和所有 AI 专家全废。
 * 所以「起不起得来」是独立一行，由后端 `agent_launch_probe` 真起一次进程来判（烧 0 token）。
 *
 * 这正是本仓库那条 readiness 约定的意思：**报告是对的，世界是坏的** —— 只报「installed:true」
 * 的功能，客户可以开着两天一点用不上而我们全绿。
 *
 * ## 独立可插拔
 * 删本模块只动 2 处：`AiRuntime.tsx`（去 import + 一行 <AgentRepair/>）和
 * `lib.rs`（去 `agent_launch_probe` 命令）。不被任何别的组件 import，只靠 props 通信。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, CheckCircle2, Loader2, RefreshCw, Stethoscope, Wrench } from "lucide-react";
import { useI18n } from "./i18n";

/** 后端 `agent_launch_probe` 的一条结果。 */
type Probe = {
  program: string;
  resolved: string;
  /** exe=原生二进制 / node=node+脚本 / shim=没解开的批处理壳 / missing=没找到 */
  via: "exe" | "node" | "shim" | "missing";
  found: boolean;
  multiline_ok: boolean;
  error: string | null;
};

type Driver = { claude_base?: string | null; codex_provider?: string | null };
type Stack = Record<string, { found?: boolean; version?: string | null } | undefined>;

const TOOLS: { id: string; name: string; blurb: string }[] = [
  { id: "claude", name: "Claude Code", blurb: "U-Workspace 的默认大脑" },
  { id: "codex", name: "Codex CLI", blurb: "OpenAI 的编程 agent" },
];

/** 一条体检结论。ok=绿 / warn=黄（能用但不理想）/ bad=红（真用不了）。 */
type Row = { level: "ok" | "warn" | "bad"; title: string; detail: string; fix?: string };

export function AgentRepair({
  onToast,
  onGoSetup,
  onGoManage,
}: {
  onToast?: (m: string) => void;
  /** 跳装机向导（装/重装这个工具） */
  onGoSetup?: () => void;
  /** 跳 AI 设置（配驱动） */
  onGoManage?: () => void;
}) {
  const { t } = useI18n();
  const [probes, setProbes] = useState<Probe[] | null>(null);
  const [stack, setStack] = useState<Stack>({});
  const [driver, setDriver] = useState<Driver>({});
  const [busy, setBusy] = useState(false);

  const scan = useCallback(async () => {
    setBusy(true);
    try {
      // 三个探测各管各的：任一个挂了不该把整块变成白屏（客户机上 detect_stack 卡住是有过的）。
      const [p, s, d] = await Promise.all([
        invoke<Probe[]>("agent_launch_probe").catch(() => [] as Probe[]),
        invoke<Stack>("detect_stack").catch(() => ({}) as Stack),
        invoke<Driver>("get_driver_status").catch(() => ({}) as Driver),
      ]);
      setProbes(p);
      setStack(s);
      setDriver(d);
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void scan();
  }, [scan]);

  /** 把三份探测拼成人话结论。**每条都要能回答「所以我该干嘛」**，否则就是又一个没人看的绿勾。 */
  function rowsFor(toolId: string): Row[] {
    const probe = probes?.find((x) => x.program === toolId);
    const inst = stack[toolId];
    const rows: Row[] = [];

    // ① 装没装
    if (inst?.found || probe?.found) {
      rows.push({
        level: "ok",
        title: t("已安装"),
        detail: inst?.version ? t("版本 {v}", { v: inst.version }) : t("在这台机器上找得到"),
      });
    } else {
      rows.push({
        level: "bad",
        title: t("没装"),
        detail: t("这台机器上找不到它"),
        fix: t("去装机向导装一下"),
      });
      return rows; // 没装就别往下报了，后面每条都会是废话
    }

    // ② 起不起得来（这一条是 detect_stack 回答不了的，见文件头）
    if (probe) {
      if (probe.via === "shim") {
        rows.push({
          level: "bad",
          title: t("起得来，但多行提问会失败"),
          detail: t("命令被解析成了批处理壳（{p}）——Windows 不允许给批处理传带换行的参数，于是「多行提问」和所有「AI 专家」都会报「启动失败」。", { p: probe.resolved }),
          fix: t("升级到 0.9.83 以上即可（本版已修）；若仍出现，多半是 PATH 上另有一份旧的壳在抢"),
        });
      } else if (!probe.multiline_ok) {
        rows.push({
          level: "bad",
          title: t("起不来"),
          detail: probe.error || t("进程无法启动"),
          fix: t("多半是被杀软拦了或文件损坏，去装机向导重装一次"),
        });
      } else {
        rows.push({
          level: "ok",
          title: t("能正常启动"),
          detail: t("走的是 {via} · {p}", {
            via: probe.via === "node" ? t("node 脚本") : t("原生程序"),
            p: probe.resolved,
          }),
        });
      }
    }

    // ③ 驱动配没配（配不对的表现是「能起来但一问就报错」，客户最容易误判成「AI 坏了」）
    const configured = toolId === "claude" ? !!driver.claude_base : !!driver.codex_provider;
    rows.push(
      configured
        ? {
            level: "ok",
            title: t("驱动已配"),
            detail: toolId === "claude" ? String(driver.claude_base) : String(driver.codex_provider),
          }
        : {
            level: "warn",
            title: t("还没配驱动"),
            detail: t("能启动，但一提问就会报鉴权错误——它还不知道该连哪。"),
            fix: t("去「AI 设置」一键配好"),
          },
    );

    return rows;
  }

  const ICON = {
    ok: <CheckCircle2 size={14} className="text-success-400 shrink-0 mt-0.5" />,
    warn: <AlertTriangle size={14} className="text-amber-500 shrink-0 mt-0.5" />,
    bad: <AlertTriangle size={14} className="text-danger-500 shrink-0 mt-0.5" />,
  };

  return (
    <div className="rounded-card border border-white/[0.06] bg-bg-2 shadow-card p-5 space-y-3">
      <header className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-[14px] font-semibold text-ink-0 flex items-center gap-2">
            <Stethoscope size={15} className="text-accent" /> {t("AI 工具专项修复")}
          </h3>
          <div className="text-[11px] text-ink-4 mt-0.5">
            {t("专治「装着却用不了」——装没装、起不起得来、配没配对，分三行说清楚")}
          </div>
        </div>
        <button
          onClick={() => void scan()}
          disabled={busy}
          className="inline-flex shrink-0 items-center gap-1.5 h-8 px-3 rounded-lg border border-white/[0.10] bg-bg-1 text-[12px] text-ink-1 hover:border-accent/50 disabled:opacity-50"
        >
          {busy ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
          {t("重新体检")}
        </button>
      </header>

      {probes === null ? (
        <div className="py-6 text-center text-[12px] text-ink-4">
          <Loader2 size={16} className="animate-spin inline mr-1.5" />
          {t("正在体检…")}
        </div>
      ) : (
        <div className="space-y-2.5">
          {TOOLS.map((tool) => {
            const rows = rowsFor(tool.id);
            const worst = rows.some((r) => r.level === "bad")
              ? "bad"
              : rows.some((r) => r.level === "warn")
                ? "warn"
                : "ok";
            return (
              <div
                key={tool.id}
                className={
                  "rounded-lg border p-3 " +
                  (worst === "bad"
                    ? "border-danger-500/30 bg-danger-500/[0.05]"
                    : worst === "warn"
                      ? "border-amber-500/30 bg-amber-500/[0.05]"
                      : "border-white/[0.07] bg-bg-1")
                }
              >
                <div className="flex items-center gap-2 mb-2">
                  <span className="text-[13px] font-semibold text-ink-0">{tool.name}</span>
                  <span className="text-[11px] text-ink-5">{t(tool.blurb)}</span>
                </div>
                <div className="space-y-1.5">
                  {rows.map((r, i) => (
                    <div key={i} className="flex items-start gap-2">
                      {ICON[r.level]}
                      <div className="min-w-0 flex-1">
                        <div className="text-[12px] text-ink-1">{r.title}</div>
                        <div className="text-[11px] text-ink-4 leading-relaxed break-all">{r.detail}</div>
                        {r.fix && <div className="text-[11px] text-accent mt-0.5">→ {r.fix}</div>}
                      </div>
                    </div>
                  ))}
                </div>
                {rows.some((r) => r.fix) && (
                  <div className="flex gap-2 mt-2.5">
                    <button
                      onClick={() => (onGoSetup ? onGoSetup() : onToast?.(t("请到「首页 · 我的 AI」装机")))}
                      className="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/[0.12] border border-accent/30 text-[11.5px] text-accent hover:bg-accent/[0.2]"
                    >
                      <Wrench size={12} /> {t("去装 / 重装")}
                    </button>
                    <button
                      onClick={() => (onGoManage ? onGoManage() : onToast?.(t("请到「AI 设置」配驱动")))}
                      className="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg border border-white/[0.10] bg-bg-2 text-[11.5px] text-ink-1 hover:border-accent/50"
                    >
                      {t("去配驱动")}
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      <div className="text-[10px] text-ink-5">
        {t("体检全程只读，不改你机器上任何配置；「起不起得来」那一项会真起一次进程再立刻关掉，不消耗 token。")}
      </div>
    </div>
  );
}
