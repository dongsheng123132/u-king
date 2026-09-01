/**
 * 自动化（定时任务）—— 「到点了，把这句话交给 AI 去干」。
 *
 * 布局借鉴 WorkBuddy 的「自动化」页：模板起手 → 一张卡一条任务 → 开关 / 立即运行 / 看结果。
 * 后端是 `src-tauri/src/automation.rs`（独立可插拔）；增删改走影核动作
 * `runtime.automation.save|remove|set_enabled`，按钮上挂 `data-action-id` 供 `action bindings` 核对。
 *
 * **这一页卖的是可信**，所以三件事必须摆在明面上，不许藏：
 *  0. **合上盖子照样会睡，任务照样不跑**（`keep_awake.prevents_lid_close = false`）。
 *     我们只挡得住「人走了但机器开着」的空闲休眠。这句紧挨着上一句说 —— 客户看完
 *     「U-King 开着就行」，下一个念头必然是「那我合盖走人」，那正好是治不了的那种。
 *  1. 只有 U-King 开着（含缩在托盘）才会到点跑 —— 后端把这条当数据发（`runs_only_while_app_open`），
 *     不是这里编的文案。
 *  2. ready / blockers 健康条：拿不到设备 Key = 配了也白配，绝不绿着糊弄。
 *  3. 填了工作文件夹 = 授权它无人值守地读写那个文件夹、跑命令。不填就只有作图/视频。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle, CheckCircle2, Clock, FolderOpen, Loader2, Pencil, Play, Plus, Trash2, X, Zap,
} from "lucide-react";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";
import { askConfirm } from "../lib/confirm";
import { MiniMd } from "../lib/miniMd";

type Schedule = { kind: "interval" | "daily" | "weekly"; minutes: number; at: string; weekdays: number[] };

type Job = {
  id: string;
  name: string;
  prompt: string;
  engine: string;
  dir: string;
  schedule: Schedule;
  enabled: boolean;
  created_at: number;
  next_run_at: number;
  last_run_at: number;
  last_ok: boolean | null;
  last_message: string;
  last_run_file: string;
  runs: number;
  use_memory: boolean;
};

type Status = {
  ready: boolean;
  blockers: string[];
  count: number;
  enabled: number;
  max: number;
  runs_only_while_app_open: boolean;
  /** 休眠抑制：能不能用 · 现在开没开 · **它治不了什么**。后端当数据发，这里只渲染不重写口径。 */
  keep_awake?: {
    supported: boolean;
    on: boolean;
    prevents_idle_sleep: boolean;
    prevents_lid_close: boolean;
    prevents_manual_sleep: boolean;
    note: string;
  } | null;
  scheduler_started: boolean;
  running_id: string | null;
  jobs: Job[];
  error?: string;
};

const WEEK = ["日", "一", "二", "三", "四", "五", "六"];

const ENGINES: { id: string; label: string; hint: string }[] = [
  { id: "uking", label: "U-King 助手", hint: "自家大脑，直连虾盘云，装了就能用（推荐）" },
  { id: "claude", label: "Claude Code", hint: "跑 claude -p，需要这台机器已装 Claude Code" },
  { id: "codex", label: "Codex", hint: "跑 codex exec，需要这台机器已装 Codex CLI" },
];

/** 小白起手式：点一下就有一条能跑的任务，不用面对空表单。 */
const TEMPLATES: { label: string; emoji: string; job: Partial<Job> }[] = [
  // ⚠️ 别加「整理今天的新闻/行情」这类模板：大脑**没有联网和搜索工具**（chat.rs 的 tools_spec
  // 只有作图/视频/文件/命令），让它「整理今天的 AI 动态」它只会一本正经地编 —— 实测跑出来的
  // 「早报」全是编的年份和产品名。演示时被客户当场戳穿，比没有这个模板坏得多。
  // 模板只放**生成型**的活（写、想、画），事实型的活必须由用户自己把资料放进工作文件夹。
  {
    label: "每天学一招",
    emoji: "💡",
    job: {
      name: "每天学一招",
      prompt: "教我一个今天就能用上的 AI 提效小技巧：先一句话说清适用场景，再给一段可以直接复制去用的提示词，最后说一句为什么这样写比随口问更好。",
      schedule: { kind: "daily", minutes: 0, at: "09:00", weekdays: [] },
    },
  },
  {
    label: "每天一条文案",
    emoji: "✍️",
    job: {
      name: "每天一条小红书文案",
      prompt: "帮我写一条今天可以发的小红书笔记：主题围绕「用 AI 提升工作效率」，要有吸睛标题、分点正文、话题标签和 emoji。",
      schedule: { kind: "daily", minutes: 0, at: "10:00", weekdays: [] },
    },
  },
  {
    label: "每周周报",
    emoji: "📊",
    job: {
      name: "每周周报提纲",
      prompt: "帮我生成一份本周工作周报的提纲：分「本周完成 / 下周计划 / 需要支持」三段，每段给 3 条可以直接往里填内容的要点。",
      schedule: { kind: "weekly", minutes: 0, at: "18:00", weekdays: [5] },
    },
  },
  {
    label: "每天出一张图",
    emoji: "🖼️",
    job: {
      name: "每天出一张配图",
      prompt: "画一张今天可以当公众号封面的图：科技感、明亮、留出放标题的空白区域。",
      schedule: { kind: "daily", minutes: 0, at: "08:30", weekdays: [] },
    },
  },
];

function blankJob(): Job {
  return {
    id: "",
    name: "",
    prompt: "",
    engine: "uking",
    dir: "",
    schedule: { kind: "daily", minutes: 60, at: "09:00", weekdays: [1] },
    enabled: true,
    created_at: 0,
    next_run_at: 0,
    last_run_at: 0,
    last_ok: null,
    last_message: "",
    last_run_file: "",
    runs: 0,
    use_memory: false,
  };
}

/** 排期的人话。**和后端 `automation::describe` 是同一套说法**（那边给 CLI / MCP 用，这边给眼睛用）。 */
function describe(s: Schedule): string {
  if (s.kind === "interval") return s.minutes % 60 === 0 ? `每 ${s.minutes / 60} 小时` : `每 ${s.minutes} 分钟`;
  if (s.kind === "daily") return `每天 ${s.at}`;
  return `每周${s.weekdays.map((d) => WEEK[d] ?? "?").join("、")} ${s.at}`;
}

function when(ts: number): string {
  if (!ts) return "—";
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, "0");
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return sameDay ? `今天 ${hm}` : `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
}

export function AutomationPanel({ onToast }: { onToast?: (m: string) => void }) {
  const { t: tr } = useI18n();
  const [status, setStatus] = useState<Status | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<Job | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [viewing, setViewing] = useState<{ job: Job; text: string } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const s = await invoke<Status>("list_automations");
      setStatus(s);
    } catch (e) {
      onToast?.(tr("读不到自动化列表：{e}", { e: String(e) }));
    } finally {
      setLoading(false);
    }
  }, [onToast, tr]);

  useEffect(() => {
    void refresh();
    // 12s 轮询：到点跑完后列表要能自己变（「上次结果」「下次时间」）。
    // 只有本页显示时才挂 —— 面板收起来就 unmount，不留后台定时器。
    const h = setInterval(() => void refresh(), 12_000);
    return () => clearInterval(h);
  }, [refresh]);

  const jobs = status?.jobs ?? [];
  const full = useMemo(() => jobs.length >= (status?.max ?? 30), [jobs.length, status?.max]);

  const save = async (job: Job) => {
    setBusy("save");
    try {
      await invoke("save_automation", { job });
      setEditing(null);
      await refresh();
      onToast?.(tr("已保存"));
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy(null);
    }
  };

  const toggle = async (job: Job) => {
    setBusy(job.id);
    try {
      await invoke("set_automation_enabled", { id: job.id, enabled: !job.enabled });
      await refresh();
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy(null);
    }
  };

  const del = async (job: Job) => {
    // 🔴 必须用 askConfirm，**绝不能用 window.confirm** —— Tauri 的 dialog 插件把它换成了
    // 返回 Promise 的版本，`!confirm(...)` 恒为 false，那条 return 永不执行 = 没问就删。
    // 实测过：点垃圾桶，确认框一次都没出现，任务直接没了（见 src/lib/confirm.ts 的原委）。
    if (!(await askConfirm(tr("删掉「{name}」？已经跑出来的结果留在磁盘上，不会删。", { name: job.name })))) return;
    setBusy(job.id);
    try {
      await invoke("remove_automation", { id: job.id });
      await refresh();
    } catch (e) {
      onToast?.(String(e));
    } finally {
      setBusy(null);
    }
  };

  const runNow = async (job: Job) => {
    setBusy(job.id);
    onToast?.(tr("「{name}」开跑了，出结果要等一会儿", { name: job.name }));
    try {
      const out = await invoke<string>("run_automation_now", { id: job.id });
      setViewing({ job, text: out });
    } catch (e) {
      onToast?.(tr("没跑成：{e}", { e: String(e) }));
    } finally {
      setBusy(null);
      void refresh();
    }
  };

  const openResult = async (job: Job) => {
    if (!job.last_run_file) return;
    try {
      setViewing({ job, text: await invoke<string>("read_automation_run", { file: job.last_run_file }) });
    } catch (e) {
      onToast?.(String(e));
    }
  };

  return (
    <div className="space-y-4">
      <header className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-[16px] font-semibold text-ink-0 flex items-center gap-1.5">
            <Zap size={16} className="text-accent" /> {tr("自动化")}
          </h2>
          <p className="text-[12px] text-ink-4 mt-0.5">
            {tr("到点了让 AI 自己把活干了 —— 每天的文案、配图、周报，不用你记着点一下")}
          </p>
        </div>
        <button
          onClick={() => setEditing(blankJob())}
          disabled={full}
          className="shrink-0 inline-flex items-center gap-1.5 h-8 px-3 rounded-card bg-accent text-white text-[12.5px] font-medium hover:bg-accent-600 disabled:opacity-40"
          title={full ? tr("已经到上限 {n} 条", { n: status?.max ?? 30 }) : ""}
        >
          <Plus size={14} /> {tr("新建自动化")}
        </button>
      </header>

      {/* 健康横幅：ready/blockers。「配了几条」和「能不能真跑起来」是两回事，别混。 */}
      {status && (
        <div
          className={cn(
            "rounded-card border px-3.5 py-2.5 text-[12px] leading-relaxed",
            status.ready
              ? "border-white/[0.08] bg-bg-2/60 text-ink-3"
              : "border-amber-500/30 bg-amber-500/[0.08] text-warning-700 dark:text-warning-400",
          )}
        >
          <div className="flex items-start gap-2">
            {status.ready ? (
              <CheckCircle2 size={14} className="mt-0.5 shrink-0 text-accent" />
            ) : (
              <AlertTriangle size={14} className="mt-0.5 shrink-0" />
            )}
            <div className="min-w-0">
              {status.ready ? (
                <span>
                  {tr("{n} 条自动化，{on} 条开着。", { n: status.count, on: status.enabled })}
                  {status.runs_only_while_app_open && (
                    <span className="text-ink-4"> {tr("注意：只有 U-King 开着（缩在托盘里也算）才会到点执行；关了电脑错过的班次不补跑。")}</span>
                  )}
                  {/* ★ 休眠这条**必须跟上一句挨着说**：客户看完「U-King 开着就行」，
                      下一个念头就是「那我合盖走人」—— 而合盖是挡不住的。
                      文案直接用后端那句（`note`），GUI / CLI / MCP 同一句话，不在这儿另写一版。 */}
                  {status.keep_awake?.supported && (
                    <span className="text-ink-4"> {tr(status.keep_awake.note)}</span>
                  )}
                </span>
              ) : (
                <>
                  <div className="font-medium">{tr("现在配了也不会真跑：")}</div>
                  <ul className="mt-0.5 space-y-0.5">
                    {status.blockers.map((b) => (
                      <li key={b}>· {b}</li>
                    ))}
                  </ul>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {/* 模板：空列表时最需要它 —— 别让小白面对一个空表单 */}
      {!editing && (
        <div className="flex flex-wrap gap-2">
          {TEMPLATES.map((t) => (
            <button
              key={t.label}
              onClick={() => setEditing({ ...blankJob(), ...t.job } as Job)}
              className="inline-flex items-center gap-1.5 h-8 px-3 rounded-card border border-white/[0.08] bg-bg-2/60 text-[12px] text-ink-2 hover:border-accent/40 hover:text-ink-0"
            >
              <span>{t.emoji}</span> {tr(t.label)}
            </button>
          ))}
        </div>
      )}

      {editing && <Editor job={editing} busy={busy === "save"} onCancel={() => setEditing(null)} onSave={save} />}

      {loading ? (
        <div className="py-10 text-center text-[12.5px] text-ink-4">
          <Loader2 size={16} className="inline animate-spin mr-1.5" />
          {tr("读取中…")}
        </div>
      ) : jobs.length === 0 ? (
        <div className="rounded-card border border-dashed border-white/[0.10] py-10 text-center">
          <div className="text-[13px] text-ink-2">{tr("还没有自动化")}</div>
          <div className="text-[12px] text-ink-4 mt-1">{tr("上面挑个模板，或者点「新建自动化」自己写一条")}</div>
        </div>
      ) : (
        <div className="space-y-2">
          {jobs.map((j) => {
            const running = status?.running_id === j.id || busy === j.id;
            return (
              <div
                key={j.id}
                className={cn(
                  "rounded-card border p-3.5",
                  j.enabled ? "border-white/[0.08] bg-bg-2/70" : "border-white/[0.05] bg-bg-2/30 opacity-70",
                )}
              >
                <div className="flex items-start gap-3">
                  <button
                    data-action-id="runtime.automation.set_enabled"
                    onClick={() => void toggle(j)}
                    disabled={!!busy}
                    title={j.enabled ? tr("点一下暂停") : tr("点一下启用")}
                    className={cn(
                      "mt-0.5 shrink-0 w-9 h-5 rounded-full transition-colors relative disabled:opacity-50",
                      j.enabled ? "bg-accent" : "bg-white/[0.14]",
                    )}
                  >
                    <span
                      className={cn(
                        "absolute top-0.5 w-4 h-4 rounded-full bg-white transition-all",
                        j.enabled ? "left-[18px]" : "left-0.5",
                      )}
                    />
                  </button>

                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-[13px] font-medium text-ink-0 truncate">{j.name}</span>
                      <span className="inline-flex items-center gap-1 text-[11px] px-1.5 py-0.5 rounded bg-white/[0.05] text-ink-3">
                        <Clock size={10} /> {describe(j.schedule)}
                      </span>
                      <span className="text-[11px] px-1.5 py-0.5 rounded bg-white/[0.05] text-ink-4">
                        {ENGINES.find((e) => e.id === j.engine)?.label ?? j.engine}
                      </span>
                      {j.dir && (
                        <span
                          className="inline-flex items-center gap-1 text-[11px] px-1.5 py-0.5 rounded bg-amber-500/[0.12] text-warning-700 dark:text-warning-400"
                          title={tr("已授权它无人值守地读写这个文件夹、在里面跑命令：{dir}", { dir: j.dir })}
                        >
                          <FolderOpen size={10} /> {tr("可动文件")}
                        </span>
                      )}
                    </div>
                    <p className="text-[12px] text-ink-3 mt-1 line-clamp-2 leading-relaxed">{j.prompt}</p>
                    <div className="text-[11px] text-ink-5 mt-1.5 flex items-center gap-3 flex-wrap">
                      <span>{tr("下次 {t}", { t: j.enabled ? when(j.next_run_at) : tr("已暂停") })}</span>
                      {j.runs > 0 && (
                        <button
                          onClick={() => void openResult(j)}
                          className={cn("hover:underline", j.last_ok === false ? "text-danger-500" : "text-ink-4")}
                          title={j.last_message}
                        >
                          {j.last_ok === false ? tr("上次失败") : tr("上次 {t} 成功", { t: when(j.last_run_at) })}
                          {j.last_run_file ? tr("（看结果）") : ""}
                        </button>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-1 shrink-0">
                    <button
                      onClick={() => void runNow(j)}
                      disabled={!!busy}
                      title={tr("现在就跑一次（不影响排期）")}
                      className="w-7 h-7 grid place-items-center rounded text-ink-3 hover:text-accent-400 hover:bg-white/[0.06] disabled:opacity-40"
                    >
                      {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
                    </button>
                    <button
                      onClick={() => setEditing(j)}
                      disabled={!!busy}
                      title={tr("编辑")}
                      className="w-7 h-7 grid place-items-center rounded text-ink-3 hover:text-ink-0 hover:bg-white/[0.06] disabled:opacity-40"
                    >
                      <Pencil size={13} />
                    </button>
                    <button
                      data-action-id="runtime.automation.remove"
                      onClick={() => void del(j)}
                      disabled={!!busy}
                      title={tr("删除")}
                      className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-danger-500 hover:bg-white/[0.06] disabled:opacity-40"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {viewing && (
        <div className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-4" onClick={() => setViewing(null)}>
          <div
            className="w-full max-w-2xl max-h-[80vh] flex flex-col rounded-card border border-white/[0.10] bg-bg-2 shadow-card"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
              <div className="min-w-0 flex-1">
                <div className="text-[13.5px] font-semibold text-ink-0 truncate">{viewing.job.name}</div>
                <div className="text-[11px] text-ink-5">{describe(viewing.job.schedule)}</div>
              </div>
              <button
                onClick={() => setViewing(null)}
                className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
              >
                <X size={16} />
              </button>
            </div>
            {/* 大模型吐的就是 markdown。直出 <pre> 会满屏 ** 和 ##，演示时看着像半成品 —— 渲染它。 */}
            <MiniMd
              text={viewing.text}
              className="flex-1 overflow-auto px-4 py-3 text-[12.5px] text-ink-2 break-words select-text"
            />
          </div>
        </div>
      )}
    </div>
  );
}

// ───────────────────────── 编辑表单 ─────────────────────────

function Editor({
  job,
  busy,
  onCancel,
  onSave,
}: {
  job: Job;
  busy: boolean;
  onCancel: () => void;
  onSave: (j: Job) => void;
}) {
  const { t: tr } = useI18n();
  const [draft, setDraft] = useState<Job>(job);
  const set = (patch: Partial<Job>) => setDraft((d) => ({ ...d, ...patch }));
  const setSch = (patch: Partial<Schedule>) => setDraft((d) => ({ ...d, schedule: { ...d.schedule, ...patch } }));

  const pickDir = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: tr("选一个工作文件夹") });
    if (typeof dir === "string" && dir) set({ dir });
  };

  const kinds: { id: Schedule["kind"]; label: string }[] = [
    { id: "daily", label: "每天" },
    { id: "weekly", label: "每周" },
    { id: "interval", label: "每隔一段" },
  ];

  return (
    <div className="rounded-card border border-accent/30 bg-bg-2/80 p-4 space-y-3">
      <div className="flex items-center justify-between">
        <span className="text-[13px] font-semibold text-ink-0">{job.id ? tr("编辑自动化") : tr("新建自动化")}</span>
        <button onClick={onCancel} className="w-6 h-6 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]">
          <X size={14} />
        </button>
      </div>

      <label className="block">
        <div className="text-[11px] text-ink-5 mb-1">{tr("叫什么")}</div>
        <input
          value={draft.name}
          onChange={(e) => set({ name: e.target.value })}
          placeholder={tr("每天早报")}
          className="w-full h-8 px-2.5 rounded-card bg-bg-1 border border-white/[0.08] text-[12.5px] text-ink-1 placeholder:text-ink-5 outline-none focus:border-accent/40"
        />
      </label>

      <label className="block">
        <div className="text-[11px] text-ink-5 mb-1">{tr("到点了让 AI 干什么（写清楚，它没法反问你）")}</div>
        <textarea
          value={draft.prompt}
          onChange={(e) => set({ prompt: e.target.value })}
          rows={3}
          placeholder={tr("把今天值得关注的 AI 动态整理成 5 条要点…")}
          className="w-full px-2.5 py-2 rounded-card bg-bg-1 border border-white/[0.08] text-[12.5px] text-ink-1 placeholder:text-ink-5 outline-none focus:border-accent/40 resize-y leading-relaxed"
        />
        <div className="text-[11px] text-ink-5 mt-1 leading-relaxed">
          {tr("它不会上网 —— 「整理今天的新闻/行情」这类活它只会编。要基于真实资料，就把资料放进下面的工作文件夹让它读。")}
        </div>
      </label>

      {/* 排期 */}
      <div>
        <div className="text-[11px] text-ink-5 mb-1.5">{tr("什么时候跑")}</div>
        <div className="flex items-center gap-1.5 mb-2 flex-wrap">
          {kinds.map((k) => (
            <button
              key={k.id}
              onClick={() => setSch({ kind: k.id })}
              className={cn(
                "h-7 px-3 rounded-full text-[12px]",
                draft.schedule.kind === k.id ? "bg-accent text-white" : "bg-bg-1 border border-white/[0.08] text-ink-3 hover:text-ink-1",
              )}
            >
              {tr(k.label)}
            </button>
          ))}
        </div>
        {draft.schedule.kind === "interval" ? (
          <div className="flex items-center gap-2 text-[12.5px] text-ink-2">
            {tr("每隔")}
            <input
              type="number"
              min={5}
              value={draft.schedule.minutes}
              onChange={(e) => setSch({ minutes: Number(e.target.value) || 5 })}
              className="w-20 h-8 px-2 rounded-card bg-bg-1 border border-white/[0.08] text-ink-1 outline-none focus:border-accent/40"
            />
            {tr("分钟（最少 5）")}
          </div>
        ) : (
          <div className="space-y-2">
            {draft.schedule.kind === "weekly" && (
              <div className="flex items-center gap-1">
                {WEEK.map((w, i) => {
                  const on = draft.schedule.weekdays.includes(i);
                  return (
                    <button
                      key={w}
                      onClick={() =>
                        setSch({
                          weekdays: on
                            ? draft.schedule.weekdays.filter((d) => d !== i)
                            : [...draft.schedule.weekdays, i].sort((a, b) => a - b),
                        })
                      }
                      className={cn(
                        "w-7 h-7 rounded text-[12px]",
                        on ? "bg-accent text-white" : "bg-bg-1 border border-white/[0.08] text-ink-3 hover:text-ink-1",
                      )}
                    >
                      {w}
                    </button>
                  );
                })}
              </div>
            )}
            <div className="flex items-center gap-2 text-[12.5px] text-ink-2">
              {tr("几点")}
              <input
                type="time"
                value={draft.schedule.at}
                onChange={(e) => setSch({ at: e.target.value })}
                className="h-8 px-2 rounded-card bg-bg-1 border border-white/[0.08] text-ink-1 outline-none focus:border-accent/40"
              />
              <span className="text-ink-5 text-[11px]">{tr("（本机时间）")}</span>
            </div>
          </div>
        )}
      </div>

      {/* 大脑 */}
      <label className="block">
        <div className="text-[11px] text-ink-5 mb-1">{tr("用哪个大脑")}</div>
        <select
          value={draft.engine}
          onChange={(e) => set({ engine: e.target.value })}
          className="w-full h-8 px-2 rounded-card bg-bg-1 border border-white/[0.08] text-[12.5px] text-ink-1 outline-none focus:border-accent/40"
        >
          {ENGINES.map((e) => (
            <option key={e.id} value={e.id}>
              {tr(e.label)}
            </option>
          ))}
        </select>
        <div className="text-[11px] text-ink-5 mt-1">{tr(ENGINES.find((e) => e.id === draft.engine)?.hint ?? "")}</div>
      </label>

      {/* 工作文件夹 —— 授权边界，必须说清楚 */}
      <div>
        <div className="text-[11px] text-ink-5 mb-1">{tr("工作文件夹（可不填）")}</div>
        <div className="flex items-center gap-2">
          <input
            value={draft.dir}
            onChange={(e) => set({ dir: e.target.value })}
            placeholder={tr("不填 = 只让它作图/生成视频，碰不到你的文件")}
            className="flex-1 min-w-0 h-8 px-2.5 rounded-card bg-bg-1 border border-white/[0.08] text-[12px] text-ink-1 placeholder:text-ink-5 outline-none focus:border-accent/40"
          />
          <button
            onClick={() => void pickDir()}
            className="shrink-0 inline-flex items-center gap-1.5 h-8 px-2.5 rounded-card border border-white/[0.08] text-[12px] text-ink-3 hover:text-ink-0"
          >
            <FolderOpen size={13} /> {tr("选")}
          </button>
          {draft.dir && (
            <button
              onClick={() => set({ dir: "" })}
              className="shrink-0 w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
              title={tr("清空")}
            >
              <X size={13} />
            </button>
          )}
        </div>
        {draft.dir && (
          <div className="mt-1.5 flex items-start gap-1.5 text-[11px] text-warning-700 dark:text-warning-400 leading-relaxed">
            <AlertTriangle size={12} className="mt-0.5 shrink-0" />
            {tr("填了文件夹 = 你允许它在没人盯着的情况下，读写这个文件夹里的文件、在里面跑命令。只填你放心的目录。")}
          </div>
        )}
      </div>

      {/* 长程记忆 —— 上下文一致。默认关：不开这条，行为一字不变。 */}
      <label className="flex items-start gap-2.5 cursor-pointer select-none pt-0.5">
        <input
          type="checkbox"
          checked={draft.use_memory}
          onChange={(e) => set({ use_memory: e.target.checked })}
          className="mt-0.5 shrink-0 w-3.5 h-3.5 accent-[var(--color-accent)]"
        />
        <span className="min-w-0">
          <span className="block text-[12.5px] font-medium text-ink-1">{tr("长程记忆：下一班接着上一班的进度干")}</span>
          <span className="block text-[11px] text-ink-5 mt-0.5 leading-relaxed">
            {tr("每班跑完把结论存进这份任务的记忆，下一班开头自动带上。适合「一个长活分几班推进」；独立的日更任务别开 —— 开了它会接着上次的写。记忆文件在 ~/.uking/automation/，随时能看。")}
          </span>
        </span>
      </label>

      <div className="flex items-center gap-2 pt-1">
        <button
          data-action-id="runtime.automation.save"
          onClick={() => onSave(draft)}
          disabled={busy || !draft.name.trim() || !draft.prompt.trim()}
          className="inline-flex items-center gap-1.5 h-8 px-4 rounded-card bg-accent text-white text-[12.5px] font-medium hover:bg-accent-600 disabled:opacity-40"
        >
          {busy && <Loader2 size={13} className="animate-spin" />}
          {tr("保存")}
        </button>
        <button
          onClick={onCancel}
          className="h-8 px-3 rounded-card text-[12.5px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.05]"
        >
          {tr("取消")}
        </button>
      </div>
    </div>
  );
}
