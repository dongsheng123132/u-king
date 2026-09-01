/**
 * Token 压缩机 —— 集成 RTK（Rust Token Killer，开源 Apache-2.0）给 AI 编程省 token：
 * 拦截 AI 跑的命令（git/cargo/test/lint/ls/grep…），把啰嗦输出压扁后再喂给大模型。
 * 默认保守档**不降智**（只砍重复日志/噪音/通过的测试列表，报错/失败/diff/告警全留）。
 * 默认关、用户自己开（省 token = 虾盘云少赚，当「高级省钱」卖点）。
 *
 * ## 这一页的设计目标是**信任**，不是功能
 * 压缩机最难卖的不是能力，是客户看不见它干了什么 —— 只给一个「省了 27%」的数字，凭什么信？
 * 所以这页把话讲到底：
 *  1. **能不能用要说实话**（`ready` / `blockers`）：装了 + 开了 ≠ 在省。老版本这里只显示
 *     绿色的「已开启」，客户开了两天一点没省也毫不知情 —— 后端早就诚实了，界面得跟上。
 *  2. **前后两个绝对值**，不只给百分比：91.3K → 66.9K 是可核对的，孤零零一个 27% 不是。
 *  3. **分项排行**：哪条命令值、哪条不值，客户自己判断值不值得开。
 *  4. **现场演示**：点一下当场跑真 rtk，压缩前后原文并排摆出来 —— 砍了什么、留了什么，
 *     肉眼可见。这是「省 token 不降智」唯一站得住的证明方式。
 *  5. **口径说实话**：真实记录是日常综合 20~35%，构建/测试才 60~90%。吹成「综合省 40~70%」
 *     客户自己一算就穿帮，砸的是招牌。
 *
 * **整块可插拔**：纯前端，只靠 props（onToast）。后端 rtk.rs + 5 个 command（rtk_status /
 * rtk_demo / rtk_install / rtk_set_enabled / rtk_uninstall）。删本页只删这一个文件 + App.tsx 去
 * import/tab + Sidebar 去 NAV + lib.rs 去 5 个 command。
 */
import { useEffect, useState } from "react";
import { useI18n } from "./i18n";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Zap, Download, Loader2, Check, Trash2, Info, Power, AlertTriangle,
  Wrench, FlaskConical, ExternalLink, ArrowRight, ChevronDown,
} from "lucide-react";
import { ShareButton } from "./components/ShareCard";

/** 一条命令的压缩战绩（来自 `rtk gain` 的 By Command 表）。 */
type GainCmd = { command: string; count: number; saved: number; pct: number };
/** 一天的压缩战绩（来自 `rtk gain --daily --format json`）。 */
type GainDay = { date: string; commands: number; before: number; after: number; saved: number; pct: number };

type RtkStatus = {
  installed: boolean;
  enabled: boolean;
  version: string | null;
  saved_tokens: number | null;
  saved_pct: number | null;
  commands: number | null;
  /** 压缩前原始 token / 压缩后真正进上下文的 token。 */
  before_tokens: number | null;
  after_tokens: number | null;
  top_commands: GainCmd[];
  daily: GainDay[];
  /** 端到端真的能用吗（装了+开了+rtk 在 PATH 上）。 */
  ready: boolean;
  /** ready=false 时说人话：卡在哪。 */
  blockers: string[];
};

/** 现场演示的一个案例：同一份输入，压缩前 vs 压缩后。 */
type DemoCase = {
  id: string;
  title: string;
  note: string;
  before: string;
  after: string;
  before_chars: number;
  after_chars: number;
  before_tokens: number;
  after_tokens: number;
  saved_pct: number;
};
type DemoResult = { ready: boolean; blockers: string[]; cases: DemoCase[] };

/** 翻译函数类型（对齐 i18n 的 useI18n().t，子组件按 props 传，保持纯展示）。 */
type T = (zh: string, vars?: Record<string, string | number>) => string;

/**
 * RTK 开源仓库 —— 「可开源、可自己查」这句话得能点开验证。
 *
 * ★ 指**上游**，不是我们的 fork（`dongsheng123132/rtk`）。理由不是客气，是诚实：
 * 我们装给客户的 rtk.exe 就是上游 release 的二进制（见 rtk.rs 的 RTK_GH_BASE，v0.43.0），
 * fork 里一行代码都没改。这页整段话是「你可以自己去读它的代码，核对上面三条」——
 * 把链接指到一个零改动、零 star 的 fork，客户点开只会更怀疑我们魔改了什么。
 * 哪天我们真在自己的仓库里加了东西（见「多元混合省 token 方案」的规划），
 * 再在这里**并列**放第二个链接、写清「上游内核 / U-King 的集成层」各是什么，
 * 而不是把这一个悄悄换掉。
 */
const RTK_REPO = "https://github.com/rtk-ai/rtk";

/** 1600→"1.6K"、2_300_000→"2.3M"。 */
function fmtNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

export function TokenSqueezer({ onToast }: { onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [st, setSt] = useState<RtkStatus | null>(null);
  const [busy, setBusy] = useState<"install" | "toggle" | "uninstall" | "demo" | null>(null);
  const [progress, setProgress] = useState("");
  const [demo, setDemo] = useState<DemoCase[] | null>(null);

  const load = async () => {
    try {
      const s = await invoke<RtkStatus>("rtk_status");
      // 老后端没有这两个数组字段时别让 .map 炸掉整页。
      setSt({ ...s, top_commands: s.top_commands ?? [], daily: s.daily ?? [], blockers: s.blockers ?? [] });
    } catch (e) {
      onToast(String(e));
    }
  };

  useEffect(() => {
    void load();
    const un = listen<string>("uking:rtk_progress", (e) => setProgress(e.payload));
    return () => {
      void un.then((f) => f());
    };
  }, []);

  const install = async () => {
    setBusy("install");
    setProgress("");
    try {
      onToast(await invoke<string>("rtk_install"));
      await load();
    } catch (e) {
      onToast(t("安装失败：") + String(e));
    } finally {
      setBusy(null);
      setProgress("");
    }
  };

  const toggle = async () => {
    if (!st) return;
    setBusy("toggle");
    try {
      const m = await invoke<string>("rtk_set_enabled", { enabled: !st.enabled });
      onToast(m + t("（重启 Claude Code 后生效）"));
      await load();
    } catch (e) {
      onToast(t("切换失败：") + String(e));
    } finally {
      setBusy(null);
    }
  };

  /** 一键修复：重新开一次开关 —— 后端 set_enabled(true) 会顺带跑 ensure_on_path 自愈。 */
  const repair = async () => {
    setBusy("toggle");
    try {
      await invoke<string>("rtk_set_enabled", { enabled: true });
      await load();
      onToast(t("已重新挂好。重启 Claude Code 后生效"));
    } catch (e) {
      onToast(t("修复失败：") + String(e));
    } finally {
      setBusy(null);
    }
  };

  const runDemo = async () => {
    setBusy("demo");
    try {
      const r = await invoke<DemoResult>("rtk_demo");
      if (!r.ready) {
        onToast(r.blockers?.[0] ?? t("演示跑不起来"));
        return;
      }
      setDemo(r.cases);
    } catch (e) {
      onToast(t("演示失败：") + String(e));
    } finally {
      setBusy(null);
    }
  };

  const uninstall = async () => {
    setBusy("uninstall");
    try {
      onToast(await invoke<string>("rtk_uninstall"));
      setDemo(null);
      await load();
    } catch (e) {
      onToast(t("卸载失败：") + String(e));
    } finally {
      setBusy(null);
    }
  };

  const saved = st?.saved_tokens ?? null;
  const before = st?.before_tokens ?? null;
  const after = st?.after_tokens ?? null;
  const yuan = saved != null ? (saved / 500000).toFixed(2) : null; // ¥1≈50万token
  const hasGain = saved != null && saved > 0;
  /** 开着但没真正生效 —— 唯一需要用户动手的状态，控制台整块转成警示色。 */
  const needFix = !!st && st.enabled && !st.ready;

  return (
    <div className="space-y-5">
      {/* 顶栏 */}
      <section className="flex items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-4">
        <div className="w-10 h-10 rounded-xl bg-accent/[0.12] grid place-items-center shrink-0">
          <Zap size={20} className="text-accent" />
        </div>
        <div className="min-w-0">
          <div className="text-[16px] font-semibold text-ink-0">{t("Token 压缩机")}</div>
          <div className="text-[12.5px] text-ink-3">
            {t("AI 编程时把啰嗦的命令输出压扁，省 token 不降智 · 基于开源 RTK")}
          </div>
        </div>
        {st?.installed && (
          <span
            className={
              "ml-auto inline-flex items-center gap-1.5 px-2.5 h-7 rounded-full text-[12px] font-medium shrink-0 " +
              (st.ready
                ? "bg-emerald-500/[0.12] text-emerald-400"
                : st.enabled
                  ? "bg-amber-500/[0.12] text-amber-400"
                  : "bg-white/[0.06] text-ink-3")
            }
          >
            {st.ready ? <Check size={13} /> : st.enabled ? <AlertTriangle size={13} /> : <Power size={13} />}
            {st.ready ? t("正在省") : st.enabled ? t("开着但没生效") : t("未开启")}
          </span>
        )}
      </section>

      {/* 主体 */}
      {!st ? (
        <div className="grid place-items-center py-16 text-ink-4">
          <Loader2 size={22} className="animate-spin" />
        </div>
      ) : !st.installed ? (
        // ── 未安装：说清原理 + 真实口径 + 一键装 ──
        <>
          <section className="rounded-card border border-white/[0.06] bg-bg-2 p-6 space-y-4">
            <p className="text-[13.5px] text-ink-1 leading-relaxed">
              {t("你的 AI 编程时会反复跑 git / 测试 / 构建等命令，那些啰嗦的输出被一遍遍喂给大模型，白烧 token。开启后由 RTK 把这些输出压扁再喂——报错、失败、diff 一个不丢。")}
            </p>
            <RealNumbers t={t} />
            <button
              onClick={install}
              disabled={busy === "install"}
              className="inline-flex items-center gap-2 px-5 h-10 rounded-lg bg-accent text-white text-[13.5px] font-semibold hover:bg-accent-600 disabled:opacity-60"
            >
              {busy === "install" ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
              {busy === "install" ? progress || t("安装中…") : t("安装 Token 压缩机（约 4MB）")}
            </button>
          </section>
          <Principle t={t} />
        </>
      ) : (
        // ── 已安装：健康 → 开关 → 战绩 → 排行 → 现场演示 → 原理 ──
        <>
          {/*
            ── 控制台：状态 + 卡在哪 + 开关，合成一块 ──
            老版把同一件事说三遍：顶栏 badge、开关卡的标题、外加一条独立的琥珀横幅，
            三块层层堆下来，客户反而不知道该信哪个。合并后「装了+开了 ≠ 在省」这条硬信息
            （被真 bug 逼出来的，别删）挨着开关，正好是要动手的地方。
          */}
          <section
            className={
              "rounded-card border p-5 " +
              (needFix ? "border-amber-500/30 bg-amber-500/[0.07]" : "border-white/[0.06] bg-bg-2")
            }
          >
            <div className="flex items-start gap-4">
              {needFix && <AlertTriangle size={17} className="text-amber-400 shrink-0 mt-0.5" />}
              <div className="min-w-0 flex-1">
                <div className={"text-[14px] font-semibold " + (needFix ? "text-warning-700 dark:text-warning-400" : "text-ink-0")}>
                  {st.ready
                    ? t("已开启 · 正在为 Claude Code 省 token")
                    : needFix
                      ? t("开关是开的，但这台机器上其实没在省")
                      : t("已安装，未开启")}
                </div>
                <div className="text-[12px] text-ink-3 mt-0.5 leading-relaxed">
                  {/* needFix 下别再念「正在自动压缩输出」—— 它此刻恰恰没在压，自相矛盾。
                      这个状态下用户唯一想知道的是「那我该干嘛」。 */}
                  {needFix
                    ? t("点「一键修复」重新挂一次；修好后重启 Claude Code 才生效")
                    : st.enabled
                      ? t("Claude Code 跑命令时自动压缩输出 · 只砍噪音，报错/diff 全留 · 改动配置后需重启 Claude Code 生效")
                      : t("点右侧开关开启（会往 ~/.claude 加一条 hook，卸载可完全清除）")}
                </div>
                {needFix && st.blockers.length > 0 && (
                  <ul className="mt-2.5 space-y-1.5">
                    {st.blockers.map((b) => (
                      <li key={b} className="text-[12.5px] text-ink-2 leading-relaxed">· {b}</li>
                    ))}
                  </ul>
                )}
              </div>
              <div className="flex items-center gap-2 shrink-0">
                {needFix && (
                  <button
                    onClick={repair}
                    disabled={busy === "toggle"}
                    className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-amber-500 text-black text-[13px] font-semibold hover:bg-amber-400 disabled:opacity-60"
                  >
                    {busy === "toggle" ? <Loader2 size={15} className="animate-spin" /> : <Wrench size={15} />}
                    {t("一键修复")}
                  </button>
                )}
                <button
                  data-action-id="runtime.rtk.set_enabled"
                  onClick={toggle}
                  disabled={busy === "toggle"}
                  title={st.enabled ? t("点击关闭") : t("点击开启")}
                  className={
                    "inline-flex items-center gap-1.5 px-4 h-9 rounded-lg text-[13px] font-semibold shrink-0 transition-colors disabled:opacity-60 " +
                    (st.enabled
                      ? "border border-white/[0.14] text-ink-2 hover:text-ink-0 hover:bg-white/[0.05]"
                      : "bg-accent text-white hover:bg-accent-600")
                  }
                >
                  {busy === "toggle" ? (
                    <Loader2 size={15} className="animate-spin" />
                  ) : st.enabled ? (
                    <Power size={15} />
                  ) : (
                    <Zap size={15} />
                  )}
                  {busy === "toggle" ? t("处理中…") : st.enabled ? t("关闭") : t("开启压缩机")}
                </button>
              </div>
            </div>
          </section>

          {/* 战绩：前后对比条 + 三个数 */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
            <div className="flex items-center justify-between gap-2 mb-3">
              <div className="text-[12.5px] text-ink-3">
                {t("你的真实战绩（数字由 rtk 自己记账，我们一个都不编）")}
              </div>
              {hasGain && (
                <ShareButton
                  onToast={onToast}
                  label={t("晒省钱成绩")}
                  spec={{
                    kind: "token-saver",
                    badge: t("AI 省钱战绩"),
                    title: t("我用 U-King 省下"),
                    heroValue: fmtNum(saved!),
                    heroUnit: "token",
                    heroSub: yuan != null ? t("≈ 少充 ¥{y}", { y: yuan }) : undefined,
                    stats: [
                      { label: t("压缩前"), value: before ? fmtNum(before) : "-" },
                      { label: t("压缩后"), value: after ? fmtNum(after) : "-" },
                      { label: t("平均压缩率"), value: st.saved_pct != null ? st.saved_pct.toFixed(0) + "%" : "-" },
                    ],
                    footnote: t("Token 压缩机 · 基于开源 RTK · 省 token 不降智"),
                  }}
                />
              )}
            </div>

            {hasGain ? (
              <>
                {before != null && after != null && before > 0 && (
                  <BeforeAfter before={before} after={after} saved={saved!} t={t} />
                )}
                <div className="grid grid-cols-3 gap-3 mt-4">
                  <Stat label={t("累计已省")} value={fmtNum(saved!)} unit="token" accent />
                  <Stat label={t("平均压缩率")} value={st.saved_pct != null ? st.saved_pct.toFixed(0) : "-"} unit="%" />
                  <Stat label={t("已优化命令")} value={st.commands != null ? String(st.commands) : "-"} unit={t("条")} />
                </div>
                {yuan != null && (
                  <div className="text-[12px] text-ink-4 mt-3">
                    {t("≈ 少充 ¥{y}（按虾盘云 ¥1≈50 万 token 估；用贵模型省得更多）", { y: yuan })}
                  </div>
                )}
                {st.daily.length > 1 && <DailyStrip days={st.daily} t={t} />}
              </>
            ) : (
              <div className="text-[13px] text-ink-4 py-3">
                {st.enabled
                  ? t("刚开启，还没积累数据 —— 用 Claude Code 跑几条命令后，这里显示真实省了多少。")
                  : t("先开启，用一阵后这里会显示真实省了多少 token。")}
              </div>
            )}
          </section>

          {/* 分项排行：哪条命令值、哪条不值 */}
          {st.top_commands.length > 0 && <CmdRanking rows={st.top_commands} t={t} />}

          {/* 现场演示 —— 「原理可视」的核心 */}
          <LiveDemo
            cases={demo}
            busy={busy === "demo"}
            onRun={runDemo}
            onClear={() => setDemo(null)}
            t={t}
          />

          <Principle t={t} />

          <div className="flex items-center gap-3 text-[12px] text-ink-5">
            <button
              data-action-id="runtime.rtk.uninstall"
              onClick={uninstall}
              disabled={busy === "uninstall"}
              className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-ink-3 hover:text-danger-400 hover:border-danger-500/40 disabled:opacity-60"
            >
              {busy === "uninstall" ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
              {t("卸载（清除 rtk + hook）")}
            </button>
            {st.version && <span>rtk {st.version.replace(/^rtk\s*/, "")}</span>}
          </div>
        </>
      )}
    </div>
  );
}

/** 真实口径：分场景给数，不给一个糊弄人的平均值。 */
function RealNumbers({ t }: { t: T }) {
  const rows = [
    { k: t("测试输出（大量通过的用例）"), v: "60~90%" },
    { k: t("构建日志（重复编译行）"), v: "60~90%" },
    { k: t("git 状态 / 分支列表"), v: "40~90%" },
    { k: t("grep / 读文件（本来就短）"), v: "20~25%" },
  ];
  return (
    <div className="rounded-lg border border-white/[0.06] bg-bg-1 p-4">
      <div className="text-[12px] text-ink-3 mb-2.5">{t("按真实记录分场景（不是一个笼统的平均值）")}</div>
      <div className="space-y-1.5">
        {rows.map((r) => (
          <div key={r.k} className="flex items-center justify-between gap-3 text-[12.5px]">
            <span className="text-ink-2">{r.k}</span>
            <span className="text-accent font-semibold tabular-nums">{r.v}</span>
          </div>
        ))}
      </div>
      <div className="text-[11.5px] text-ink-4 mt-3 leading-relaxed">
        {t("日常混着用，综合下来通常是 20~35%。开着不亏——已经很紧凑的输出它不硬压。")}
      </div>
    </div>
  );
}

/**
 * 压缩前 → 压缩后 的可视对比条。两个绝对值摆出来，百分比才可核对。
 *
 * 一条槽说完两件事：**整条 = 压缩前的原始输出**，亮色段 = 真正喂给大模型的，剩下的就是省掉的。
 * 老写法画两条，其中「压缩前」是一条**永远满格**的灰条 —— 占掉一整行却不携带任何信息。
 */
function BeforeAfter({
  before, after, saved, t,
}: { before: number; after: number; saved: number; t: T }) {
  const afterPct = Math.max(4, Math.min(100, (after / before) * 100));
  return (
    <div>
      <div className="flex items-baseline justify-between text-[11.5px] mb-1.5">
        <span className="text-accent font-semibold">
          {t("压缩后真正喂进去")} <span className="tabular-nums">{fmtNum(after)}</span>
        </span>
        <span className="text-ink-4">
          {t("压缩前原始输出")} <span className="tabular-nums">{fmtNum(before)}</span>
        </span>
      </div>
      <div className="relative h-3.5 rounded-full bg-white/[0.07] overflow-hidden">
        <div className="absolute inset-y-0 left-0 bg-accent rounded-full transition-all" style={{ width: `${afterPct}%` }} />
      </div>
      <div className="text-[12px] text-emerald-400 font-medium mt-2">
        {t("↓ 少喂 {n} token", { n: fmtNum(saved) })}
      </div>
    </div>
  );
}

/**
 * 每日省了多少（趋势条）。
 *
 * ⚠️ 柱子的 `height: X%` **必须有一个高度确定的直接父级**才解析得出来。老写法把 `h-16`
 * 放在最外层 flex 容器上、柱子和日期标签同为它孙辈的 `flex-col` 子项 —— 子项高度是 auto，
 * 百分比无基准，整排柱子塌成 0 高：图表区一片空白，只剩一行日期（1440 宽截图实锤）。
 * 现在把固定高度落到「柱子槽」这一层，标签在槽外，百分比才有意义。
 */
function DailyStrip({ days, t }: { days: GainDay[]; t: T }) {
  const max = Math.max(...days.map((d) => d.saved), 1);
  return (
    <div className="mt-5 pt-4 border-t border-white/[0.06]">
      <div className="flex items-baseline justify-between mb-2.5">
        <span className="text-[11.5px] text-ink-4">{t("每天省下的 token")}</span>
        <span className="text-[11px] text-ink-5 tabular-nums">{t("最高 {n}", { n: fmtNum(max) })}</span>
      </div>
      <div className="flex items-end gap-1.5">
        {days.map((d) => (
          <div
            key={d.date}
            className="flex-1 min-w-0 flex flex-col items-center gap-1.5 group"
            title={`${d.date} · ${fmtNum(d.saved)} token · ${d.pct.toFixed(0)}%`}
          >
            {/* max-w 是给「只有几天数据」的新用户兜的：不限宽时 6 根柱子会各占 180px、
                比高度还宽，看着像色块不像趋势图。天数多了自然变窄，max-w 不起作用。 */}
            <div className="w-full h-20 flex items-end justify-center">
              <div
                className="w-full max-w-[44px] rounded-t bg-accent/70 group-hover:bg-accent transition-colors"
                style={{ height: `${Math.max(4, (d.saved / max) * 100)}%` }}
              />
            </div>
            <span className="text-[9.5px] text-ink-5 tabular-nums">{d.date.slice(5)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** 命令排行：哪条最值。客户据此判断值不值得开，也是最能拿去讲的一张表。 */
function CmdRanking({ rows, t }: { rows: GainCmd[]; t: T }) {
  const max = Math.max(...rows.map((r) => r.saved), 1);
  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
      <div className="text-[12.5px] text-ink-3 mb-3">{t("哪些命令最值（按省下的量排）")}</div>
      <div className="space-y-2">
        {rows.map((r) => (
          <div key={r.command} className="flex items-center gap-3">
            <code className="text-[12px] text-ink-1 font-mono truncate w-[30%] shrink-0">{r.command}</code>
            <div className="flex-1 h-2 rounded-full bg-white/[0.06] overflow-hidden">
              <div className="h-full bg-accent/80 rounded-full" style={{ width: `${(r.saved / max) * 100}%` }} />
            </div>
            <span className="text-[11.5px] text-ink-3 tabular-nums w-14 text-right shrink-0">{fmtNum(r.saved)}</span>
            <span className="text-[11.5px] text-accent tabular-nums w-12 text-right shrink-0 font-semibold">
              {r.pct.toFixed(0)}%
            </span>
            <span className="text-[11px] text-ink-5 tabular-nums w-12 text-right shrink-0">×{r.count}</span>
          </div>
        ))}
      </div>
      <div className="text-[11.5px] text-ink-4 mt-3 leading-relaxed">
        {t("压缩率低的不是没干活 —— 那类输出本来就紧凑，硬压才会丢信息。")}
      </div>
    </section>
  );
}

/**
 * 现场演示：点一下**当场跑真的 rtk**，把压缩前后原文并排摆出来。
 *
 * 为什么不预存结果：预存 = 我们自己写答案，那还是「信我」。当场跑，客户机上 rtk 什么行为
 * 这里就显示什么 —— 这才是可核对的。样例是内嵌的，截图可以直接拿去宣传，不含任何隐私。
 */
function LiveDemo({
  cases, busy, onRun, onClear, t,
}: {
  cases: DemoCase[] | null;
  busy: boolean;
  onRun: () => void;
  onClear: () => void;
  t: T;
}) {
  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5">
      <div className="flex items-start gap-3">
        <div className="w-9 h-9 rounded-lg bg-accent/[0.10] grid place-items-center shrink-0">
          <FlaskConical size={17} className="text-accent" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-semibold text-ink-0">{t("当场看它怎么省的")}</div>
          <div className="text-[12px] text-ink-3 mt-0.5 leading-relaxed">
            {t("拿两段真实形态的日志，现场跑一遍真的 rtk，压缩前后并排给你看：砍了什么、留了什么，一眼可见。")}
          </div>
        </div>
        <button
          onClick={cases ? onClear : onRun}
          disabled={busy}
          className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg border border-accent/40 bg-accent/[0.12] text-[13px] font-semibold text-accent hover:bg-accent/20 disabled:opacity-60 shrink-0"
        >
          {busy ? <Loader2 size={15} className="animate-spin" /> : cases ? <ChevronDown size={15} /> : <FlaskConical size={15} />}
          {busy ? t("跑着…") : cases ? t("收起") : t("现场跑一次")}
        </button>
      </div>

      {cases && (
        <div className="mt-4 space-y-5">
          {cases.map((c) => (
            <div key={c.id}>
              <div className="flex items-center gap-2 flex-wrap mb-1.5">
                <span className="text-[13px] font-semibold text-ink-0">{c.title}</span>
                <span className="px-2 h-6 inline-flex items-center rounded-full bg-emerald-500/[0.12] text-emerald-400 text-[11.5px] font-semibold tabular-nums">
                  {t("省 {p}%", { p: c.saved_pct.toFixed(0) })}
                </span>
                <span className="text-[11.5px] text-ink-4 tabular-nums">
                  {c.before_tokens} → {c.after_tokens} token（{t("估算")}）
                </span>
              </div>
              <div className="text-[12px] text-ink-3 mb-2">{c.note}</div>
              <div className="grid md:grid-cols-[1fr_auto_1fr] gap-2 items-center">
                <DemoPane label={t("压缩前")} chars={c.before_chars} text={c.before} t={t} />
                <ArrowRight size={16} className="text-ink-5 mx-auto hidden md:block shrink-0" />
                <DemoPane label={t("压缩后")} chars={c.after_chars} text={c.after} accent t={t} />
              </div>
            </div>
          ))}
          <div className="text-[11.5px] text-ink-4 leading-relaxed">
            {t("这是这台机器上当场跑出来的结果，不是我们预先写好的截图 —— 样例内置，不读你的任何文件。")}
          </div>
        </div>
      )}
    </section>
  );
}

function DemoPane({
  label, chars, text, accent, t,
}: { label: string; chars: number; text: string; accent?: boolean; t: T }) {
  return (
    <div className="min-w-0">
      <div className="flex items-center justify-between text-[11px] mb-1">
        <span className={accent ? "text-accent font-semibold" : "text-ink-4"}>{label}</span>
        <span className="text-ink-5 tabular-nums">{chars} {t("字符")}</span>
      </div>
      <pre
        className={
          "text-[10.5px] leading-[1.5] font-mono whitespace-pre-wrap break-all rounded-lg p-3 max-h-52 overflow-auto border " +
          (accent
            ? "bg-accent/[0.06] border-accent/25 text-ink-1"
            : "bg-bg-1 border-white/[0.06] text-ink-3")
        }
      >
        {text}
      </pre>
    </div>
  );
}

/** 省的原理 + 开源可查。把规则写死在界面上，客户能拿它去对我们的行为。 */
function Principle({ t }: { t: T }) {
  const rules = [
    {
      k: t("① 折叠重复"),
      v: t("一百行「Compiling xxx」、几十条通过的测试，只留计数和结论 —— 它们对大模型没有信息量。"),
    },
    {
      k: t("② 保住信号"),
      v: t("报错、失败、告警、diff 连同文件名行号原样保留。判断力来自这些行，一条都不能少。"),
    },
    {
      k: t("③ 不硬压"),
      v: t("输出本来就短、本来就紧凑的，原样透传。为了凑压缩率去砍，砍掉的就是你要的东西。"),
    },
  ];
  return (
    <section className="rounded-card border border-white/[0.06] bg-bg-2 p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Info size={15} className="text-ink-4" />
        <div className="text-[13.5px] font-semibold text-ink-0">{t("它到底怎么省的")}</div>
      </div>
      {/* 三条并排成三列：竖排时标签列固定 w-16，会把「折叠重复」拦腰断成「折叠重 / 复」
          （1440 宽截图实锤）；并排后标题独占一行、不断字，整块也从三行压到一行高。
          **不折叠** —— 这三条是这页的信任地基，藏起来等于把最该被看到的东西收走了。 */}
      <div className="grid gap-4 md:grid-cols-3">
        {rules.map((r) => (
          <div key={r.k} className="min-w-0">
            <div className="text-[12.5px] font-semibold text-accent mb-1">{r.k}</div>
            <div className="text-[12.5px] text-ink-2 leading-relaxed">{r.v}</div>
          </div>
        ))}
      </div>
      <div className="pt-3 border-t border-white/[0.06] space-y-2">
        <div className="text-[12px] text-ink-3 leading-relaxed">
          {t("压缩内核是开源的 RTK（Apache-2.0，Rust 单二进制）。我们只负责把它装好、管好 Claude Code 的 hook，不改它的压缩逻辑 —— 你可以自己去读它的代码，核对上面三条。")}
        </div>
        <button
          onClick={() => openUrl(RTK_REPO).catch(() => {})}
          className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.12] text-[12px] text-ink-2 hover:text-ink-0 hover:bg-white/[0.05]"
        >
          <ExternalLink size={13} />
          {t("查看压缩内核源码")}
        </button>
      </div>
    </section>
  );
}

function Stat({ label, value, unit, accent }: { label: string; value: string; unit: string; accent?: boolean }) {
  return (
    <div className="rounded-lg bg-bg-1 border border-white/[0.05] px-3 py-3 text-center">
      <div className="text-[11px] text-ink-4 mb-1">{label}</div>
      <div className={"text-[20px] font-bold leading-none " + (accent ? "text-accent" : "text-ink-0")}>
        {value}
        <span className="text-[11px] font-normal text-ink-4 ml-0.5">{unit}</span>
      </div>
    </div>
  );
}
