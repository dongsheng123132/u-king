/**
 * 夜班助手 —— 「AI 白天陪我们干活，晚上替我们值班」。
 *
 * ## 这一版**只做记录，不执行**（2026-08-04 产品决策）
 *
 * 上半页是**真的**：本机行为时间轴（谁在什么时候干了什么），后端 `journal.rs`，
 * 影核动作 `runtime.journal.inspect`。
 * 下半页是**规划中的能力**，全部标「暂未开放」，点了只解释、不干活。
 *
 * 为什么这么排：**记录是后面一切的地基**。熔断要判断「烧得异常」，得先知道正常是多少；
 * 护栏要拦「失控行为」，得先有失控的样子；交班报告的每一行都从时间轴来。
 * 反过来，先上执行、后补记录，等于让 AI 半夜在一台没有黑匣子的机器上动手 ——
 * 出了事只能靠客户口述。所以先把黑匣子装上，这一步零风险且立刻可用。
 *
 * ## 「暂未开放」不是占位符
 * 每张卡片都写清**这件事具体是什么、为什么现在还不能开**。给一个没有说明的灰按钮
 * 比不给更糟：客户会以为是坏了。按钮点下去必须有回应（一句人话），不能什么都不发生。
 *
 * ## 绑定核对（宪法第 14 条）
 * 只有**真的连着影核动作**的控件才挂 `data-action-id`。规划中的按钮**一律不挂** ——
 * 挂一个动作表里不存在的 id，`action bindings` 会判 `stale` 直接失败（这是对的：
 * 那意味着自动化点它会点空）。
 *
 * **整块可插拔**：纯前端，只靠 `onToast` prop。删本页 = 删这一个文件 +
 * App.tsx 去 import/tab + Sidebar 去 LAB 一行。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Moon, Loader2, RefreshCw, User, Bot, FileEdit, ShieldAlert, Lock,
  Trash2, EyeOff, Coffee, Gauge, GitBranch, ClipboardList, MonitorOff, Info,
} from "lucide-react";
import { useI18n } from "./i18n";
import { askConfirm } from "./lib/confirm";

/** `useI18n()` 的 `t`，传给子组件时用它 —— 签名必须和 i18n 那边**一字不差**，
 *  放宽成 `Record<string, unknown>` 会当场编译不过（逆变）。 */
type Translate = (zh: string, vars?: Record<string, string | number>) => string;

type Ev = {
  at: number;
  actor: "human" | "ai" | "system";
  via?: string;
  agent?: string;
  kind: string;
  name: string;
  target?: string;
  ok: boolean;
  ms?: number;
  err?: string | null;
  note?: string;
};
type Summary = {
  total: number;
  human: number;
  ai: number;
  system: number;
  failed: number;
  file_writes: number;
  top_actions: { name: string; count: number }[];
  touched_files: { name: string; count: number }[];
};
type Status = {
  ready: boolean;
  blockers: string[];
  enabled: boolean;
  keep_days: number;
  dir: string;
  records_only_uking_actions: boolean;
  uploads: boolean;
  summary: Summary;
  recent: Ev[];
};

/** 本机时间的 `HH:MM:SS`。时间轴里 `at` 是 ms 时间戳，展示一律按**客户本地时区**。 */
function clock(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 同一天吗（本地时区）—— 时间轴按天分组用。 */
function dayKey(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function NightShift({ onToast }: { onToast: (msg: string) => void }) {
  const { t } = useI18n();
  const [data, setData] = useState<Status | null>(null);
  const [days, setDays] = useState(1);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    async (d: number) => {
      setBusy(true);
      try {
        setData((await invoke("journal_inspect", { days: d })) as Status);
      } catch (e) {
        onToast(t("读不到行为记录：") + String(e));
      } finally {
        setBusy(false);
      }
    },
    [onToast, t],
  );

  useEffect(() => {
    void load(days);
  }, [days, load]);

  const toggle = async (on: boolean) => {
    try {
      await invoke("journal_set_enabled", { enabled: on });
      onToast(on ? t("已开启行为记录") : t("已关闭 —— 此后的动作不再留痕"));
      void load(days);
    } catch (e) {
      onToast(String(e));
    }
  };

  const wipe = async () => {
    // 破坏性操作走 `askConfirm`，**绝不能用 `window.confirm`** ——
    // Tauri 的 dialog 插件把它换成返 Promise 的版本，`!Promise` 恒为 false，
    // 那条 return 永远不执行 = 点了「清空」根本没问就删了，弹窗只是事后飘出来。
    // 详见 lib/confirm.ts 的注释（线上 issue #227 就是这个）。
    if (!(await askConfirm(t("清空全部行为记录？删了就找不回来了。")))) return;
    try {
      await invoke("journal_clear");
      onToast(t("已清空"));
      void load(days);
    } catch (e) {
      onToast(String(e));
    }
  };

  const s = data?.summary;
  // 按天分组（新→旧），组内也是新→旧：时间轴要「最近的在最上面」。
  const grouped: [string, Ev[]][] = [];
  for (const e of [...(data?.recent ?? [])].reverse()) {
    const k = dayKey(e.at);
    const last = grouped[grouped.length - 1];
    if (last && last[0] === k) last[1].push(e);
    else grouped.push([k, [e]]);
  }

  return (
    <div className="space-y-5">
      {/* ── 顶栏 ───────────────────────────────────────────── */}
      <section className="flex items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-4">
        <div className="w-10 h-10 rounded-xl bg-accent/[0.12] grid place-items-center shrink-0">
          <Moon size={20} className="text-accent" />
        </div>
        <div className="min-w-0">
          <div className="text-[16px] font-semibold text-ink-0">{t("夜班助手")}</div>
          <div className="text-[12.5px] text-ink-3">
            {t("白天 AI 陪你干活，晚上替你值班 · 当前阶段：只记录，不执行")}
          </div>
        </div>
        <div className="ml-auto flex items-center gap-2 shrink-0">
          <div className="flex items-center gap-1 rounded-lg bg-white/[0.04] p-0.5">
            {[1, 7, 30].map((d) => (
              <button
                key={d}
                onClick={() => setDays(d)}
                className={
                  "px-2.5 h-7 rounded-md text-[12px] transition-colors " +
                  (days === d ? "bg-accent/20 text-accent font-medium" : "text-ink-4 hover:text-ink-2")
                }
              >
                {d === 1 ? t("今天") : t("{n} 天", { n: d })}
              </button>
            ))}
          </div>
          <button
            // 真的连着影核动作 —— `action bindings` 能查到它没点空。
            data-action-id="runtime.journal.inspect"
            onClick={() => void load(days)}
            disabled={busy}
            className="w-8 h-8 grid place-items-center rounded-lg bg-white/[0.06] text-ink-3 hover:text-ink-1 disabled:opacity-50"
            title={t("刷新")}
          >
            {busy ? <Loader2 size={15} className="animate-spin" /> : <RefreshCw size={15} />}
          </button>
        </div>
      </section>

      {/* ── 这一版做了什么、没做什么。放在最上面，别让客户自己猜 ────────── */}
      <section className="rounded-card border border-accent/20 bg-accent/[0.05] px-5 py-4 flex gap-3">
        <Info size={16} className="text-accent shrink-0 mt-0.5" />
        <div className="text-[12.5px] text-ink-2 leading-relaxed space-y-1.5">
          <p>
            {t("这一版先把「黑匣子」装上：把这台电脑上发生的动作记成一条可回看的时间轴 —— 哪些是你点的，哪些是 AI 自己干的。")}
          </p>
          <p className="text-ink-3">
            {t("让 AI 半夜真正动手的那些能力（熔断、护栏、快照回滚、交班报告）都还没开放，列在下方。先有记录再谈放权：没有时间轴就不知道正常是什么样，也就判断不了什么叫失控。")}
          </p>
        </div>
      </section>

      {!data ? (
        <div className="grid place-items-center py-16 text-ink-4">
          <Loader2 size={22} className="animate-spin" />
        </div>
      ) : (
        <>
          {/* ── 摘要 ────────────────────────────────────────── */}
          <section className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            <Stat icon={User} label={t("你干的")} value={s?.human ?? 0} />
            <Stat icon={Bot} label={t("AI 干的")} value={s?.ai ?? 0} />
            <Stat icon={FileEdit} label={t("AI 动过文件")} value={s?.file_writes ?? 0} />
            <Stat icon={ShieldAlert} label={t("失败")} value={s?.failed ?? 0} warn={(s?.failed ?? 0) > 0} />
          </section>

          {/* ── 时间轴 ──────────────────────────────────────── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 overflow-hidden">
            <div className="px-5 py-3 border-b border-white/[0.06] flex items-center gap-2">
              <ClipboardList size={15} className="text-ink-3" />
              <span className="text-[13px] font-medium text-ink-1">{t("行为时间轴")}</span>
              {!data.enabled && (
                <span className="ml-auto text-[11.5px] text-amber-400/90">{t("记录已关闭")}</span>
              )}
            </div>

            {!data.enabled ? (
              <div className="px-5 py-10 text-center space-y-2">
                <EyeOff size={22} className="text-ink-5 mx-auto" />
                <p className="text-[13px] text-ink-2">{t("行为记录已关闭")}</p>
                <p className="text-[12px] text-ink-4">
                  {t("关闭期间发生的事不会留痕。夜班的交班报告依赖这份记录。")}
                </p>
              </div>
            ) : grouped.length === 0 ? (
              // 空不是错误，是这台机器的事实 —— 说清为什么，别摆一堆 0 假装正常。
              <div className="px-5 py-10 text-center space-y-2">
                <Moon size={22} className="text-ink-5 mx-auto" />
                <p className="text-[13px] text-ink-2">{t("这个窗口里还没有记录")}</p>
                <p className="text-[12px] text-ink-4">
                  {t("在 U-King 里点几下、或让 AI 干点活，回来再看就有了。")}
                </p>
              </div>
            ) : (
              <div className="max-h-[420px] overflow-y-auto divide-y divide-white/[0.04]">
                {grouped.map(([day, evs]) => (
                  <div key={day}>
                    <div className="px-5 py-1.5 bg-white/[0.02] text-[11.5px] text-ink-4 sticky top-0 backdrop-blur">
                      {day} · {t("{n} 条", { n: evs.length })}
                    </div>
                    {evs.map((e, i) => (
                      <Row key={`${e.at}-${i}`} e={e} t={t} />
                    ))}
                  </div>
                ))}
              </div>
            )}
          </section>

          {/* ── AI 动过哪些文件（夜班最想一眼看到的） ─────────────── */}
          {(s?.touched_files.length ?? 0) > 0 && (
            <section className="rounded-card border border-white/[0.06] bg-bg-2 px-5 py-4">
              <div className="flex items-center gap-2 mb-3">
                <FileEdit size={15} className="text-ink-3" />
                <span className="text-[13px] font-medium text-ink-1">{t("AI 动过的文件")}</span>
              </div>
              <div className="flex flex-wrap gap-2">
                {s?.touched_files.map((f) => (
                  <span
                    key={f.name}
                    className="px-2.5 py-1 rounded-lg bg-white/[0.05] text-[12px] text-ink-2 font-mono"
                  >
                    {f.name}
                    {f.count > 1 && <span className="text-ink-4"> ×{f.count}</span>}
                  </span>
                ))}
              </div>
            </section>
          )}

          {/* ── 隐私：记什么、不记什么、存哪、怎么关 ───────────────── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2 px-5 py-4 space-y-3">
            <div className="flex items-center gap-2">
              <Lock size={15} className="text-ink-3" />
              <span className="text-[13px] font-medium text-ink-1">{t("这份记录里有什么")}</span>
            </div>
            <ul className="text-[12.5px] text-ink-3 leading-relaxed space-y-1 list-disc pl-4">
              <li>{t("只记 U-King 里发生的动作和 AI 调的工具。不记键盘鼠标、不记你开了什么软件、不记浏览器。")}</li>
              <li>{t("不记对话正文、不记文件内容、不记 Key。路径只留工作区内的相对位置，命令只留第一个词。")}</li>
              <li>{t("只存在你自己电脑上（{dir}），保留 {n} 天，不上传任何地方。", { dir: data.dir, n: data.keep_days })}</li>
            </ul>
            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={() => void toggle(!data.enabled)}
                className="px-3 h-8 rounded-lg bg-white/[0.06] hover:bg-white/[0.1] text-[12.5px] text-ink-2 transition-colors"
              >
                {data.enabled ? t("关闭记录") : t("开启记录")}
              </button>
              <button
                onClick={() => void wipe()}
                className="px-3 h-8 rounded-lg bg-white/[0.06] hover:bg-red-500/15 hover:text-red-300 text-[12.5px] text-ink-3 transition-colors inline-flex items-center gap-1.5"
              >
                <Trash2 size={13} />
                {t("清空记录")}
              </button>
            </div>
          </section>
        </>
      )}

      {/* ── 规划中的夜班能力（全部暂未开放） ─────────────────────── */}
      <section className="space-y-3">
        <div className="flex items-center gap-2 px-1">
          <Moon size={15} className="text-ink-4" />
          <span className="text-[13px] font-medium text-ink-2">{t("夜班能力 · 规划中")}</span>
          <span className="text-[11.5px] text-ink-5">{t("下面这些还没开放，点一下可以看它是什么")}</span>
        </div>
        <div className="grid sm:grid-cols-2 gap-3">
          {PLANNED.map((p) => (
            <Planned key={p.key} item={p} onToast={onToast} t={t} />
          ))}
        </div>
      </section>
    </div>
  );
}

/* ───────────────────────── 小组件 ───────────────────────── */

function Stat({
  icon: Icon,
  label,
  value,
  warn,
}: {
  icon: typeof User;
  label: string;
  value: number;
  warn?: boolean;
}) {
  return (
    <div className="rounded-card border border-white/[0.06] bg-bg-2 px-4 py-3">
      <div className="flex items-center gap-1.5 text-[11.5px] text-ink-4 mb-1">
        <Icon size={13} />
        {label}
      </div>
      <div className={"text-[20px] font-semibold " + (warn ? "text-amber-400" : "text-ink-0")}>{value}</div>
    </div>
  );
}

function Row({ e, t }: { e: Ev; t: Translate }) {
  const isAi = e.actor === "ai";
  const isSys = e.actor === "system";
  return (
    <div className="px-5 py-2 flex items-center gap-3 hover:bg-white/[0.02]">
      <span className="text-[11.5px] text-ink-5 font-mono shrink-0 w-[58px]">{clock(e.at)}</span>
      <span
        className={
          "shrink-0 w-[42px] text-[11px] px-1.5 py-0.5 rounded text-center " +
          (isSys
            ? "bg-white/[0.05] text-ink-4"
            : isAi
              ? "bg-violet-500/15 text-violet-300"
              : "bg-sky-500/15 text-sky-300")
        }
      >
        {isSys ? t("系统") : isAi ? t("AI") : t("你")}
      </span>
      <span className="text-[12.5px] text-ink-1 font-mono truncate">{e.name}</span>
      {e.target && <span className="text-[12px] text-ink-4 font-mono truncate">{e.target}</span>}
      {!e.ok && (
        <span className="ml-auto shrink-0 text-[11.5px] text-red-300/90">{e.err || t("失败")}</span>
      )}
      {e.ok && !!e.ms && e.ms > 0 && (
        <span className="ml-auto shrink-0 text-[11px] text-ink-5 font-mono">{e.ms}ms</span>
      )}
    </div>
  );
}

/** 规划中的一项能力。`why` 回答「为什么现在还不能开」—— 没有它，灰按钮读起来就像坏了。 */
type PlannedItem = {
  key: string;
  icon: typeof Coffee;
  title: string;
  what: string;
  why: string;
};

const PLANNED: PlannedItem[] = [
  {
    key: "awake",
    icon: Coffee,
    title: "别让电脑睡",
    what: "夜班期间抑制系统休眠（屏幕照常黑，机器不睡），到点自动解除。",
    why: "这是夜班的头号前置：现在定时任务活在应用进程里，你合盖走人后 Windows 一睡，一整晚一条都不会跑，而且不会报错。",
  },
  {
    key: "fuse",
    icon: Gauge,
    title: "Token 保险丝",
    what: "给一整个夜班班次设一个预算池（今晚最多花多少），触线即停并留证；单轮异常暴涨当场掐断。",
    why: "花费口径要用「跑前跑后查余额取差值」才准 —— 委派给 Claude / Codex 那条路我们只拿到输出，本地算不出它烧了多少。这条要先落地。",
  },
  {
    key: "guard",
    icon: ShieldAlert,
    title: "失控护栏",
    what: "拦住无人值守时不该发生的操作：git reset --hard / clean -fd / push --force、对外发布、超量删除。",
    why: "现有黑名单挡的是格盘、删系统这类一眼恶意的；真正会出事的是「合法命令 + 全授权 + 没人看着」，这批一条都没拦。",
  },
  {
    key: "snapshot",
    icon: GitBranch,
    title: "夜班前快照 · 一键回滚",
    what: "开工前给工作区打个快照，早上一个按钮回到夜班前。",
    why: "监控只能告诉你「它删了」，救不回来。这才是「AI 乱删文件」的解药 —— 而且工作区是 git 仓库时成本几乎为零。",
  },
  {
    key: "handover",
    icon: ClipboardList,
    title: "交班报告",
    what: "早上一张纸：今晚跑了什么、花了多少、动了哪些文件、有几次被拦下等你定夺。",
    why: "上面那条时间轴就是它的原料。等执行类能力开放后，这份报告才有内容可交。",
  },
  {
    key: "screen",
    icon: MonitorOff,
    title: "夜间免打扰",
    what: "夜班期间不弹提示、不抢焦点、不亮屏，托盘换个配色表示在值班。",
    why: "半夜被自己的电脑吵醒一次，客户就再也不会开夜班了。",
  },
];

function Planned({
  item,
  onToast,
  t,
}: {
  item: PlannedItem;
  onToast: (m: string) => void;
  t: Translate;
}) {
  const Icon = item.icon;
  return (
    <div className="rounded-card border border-white/[0.06] bg-bg-2 px-4 py-3.5 space-y-2">
      <div className="flex items-center gap-2">
        <Icon size={15} className="text-ink-4 shrink-0" />
        <span className="text-[13px] font-medium text-ink-1">{t(item.title)}</span>
        <span className="ml-auto shrink-0 text-[11px] px-1.5 py-0.5 rounded bg-white/[0.06] text-ink-4">
          {t("暂未开放")}
        </span>
      </div>
      <p className="text-[12.5px] text-ink-3 leading-relaxed">{t(item.what)}</p>
      <button
        // 规划中的能力**不挂 data-action-id** —— 动作表里没有它，挂了 `action bindings`
        // 会判 stale 直接失败（那正是对的：自动化点它会点空）。
        onClick={() => onToast(t(item.why))}
        className="text-[12px] text-ink-4 hover:text-ink-2 transition-colors underline decoration-dotted underline-offset-2"
      >
        {t("为什么还没开放？")}
      </button>
    </div>
  );
}
