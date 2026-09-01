/**
 * 竞技场（Arena）—— 六个 CLI 同任务横向比「谁干活利索」。
 *
 * ## 评分口径（需求榜 A 条已定死）
 * 系统**只出可观测量**：耗时 / 退出码 / 有没有真产出 / stdout 尾部。质量那一栏
 * **只由人打星** —— 让系统判质量 = 重演 cli2work 那次自建判分器把自己的实现
 * 偏好变成评分标准的教训（查裸串「1202」惩罚了写「1,202万元」的，n=1 不可信）。
 *
 * ## 交互
 * 勾选参赛 CLI（默认全选）→ 塞同一个任务 → 选工作副本根目录（每个参赛者一个独立
 * 子目录，互不踩文件）→ 点「开赛」。一跑就烧 token，所以开赛按钮就是确认，
 * 不做第二次弹窗。结果回来后每行手动打星。
 *
 * 不开赛时看到的是「打分口径」说明 —— 不解释清楚，人很容易把「第一个跑完的」
 * 当成「最好的」，那是把系统可观测量错当质量排序。
 */
import { useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Play, Star } from "lucide-react";
import { useI18n } from "../i18n";

/** 竞技场参赛名单 —— 和后端 arena.rs 的 ARENA_TOOLS 同序同内容，别另写一份。
 *  加了新工具要两边一起改（同 `apps.ts` 那份三处共用的注册表规矩）。 */
const TOOLS: { id: string; name: string }[] = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex" },
  { id: "hermes", name: "Hermes" },
  { id: "pi", name: "Pi" },
  { id: "qwen", name: "Qwen Code" },
  { id: "crush", name: "Crush" },
];

/** 后端 ArenaResult 的可观测量字段（与 arena.rs 的 ArenaResult 一一对应）。 */
type ArenaResult = {
  tool: string;
  installed: boolean;
  ran: boolean;
  timed_out: boolean;
  exit_code: number | null;
  ms: number;
  produced: boolean;
  stdout_tail: string;
  note: string;
};

export function Arena({ workspace, onToast }: { workspace?: string; onToast?: (m: string) => void }) {
  const { t: tr } = useI18n();
  const [task, setTask] = useState("");
  const [root, setRoot] = useState(workspace ?? "");
  const [picked, setPicked] = useState<string[]>(TOOLS.map((t) => t.id));
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const [results, setResults] = useState<ArenaResult[] | null>(null);
  /** 人打星：tool id → 1..5。这是竞技场唯一的「质量」字段，系统不给。 */
  const [stars, setStars] = useState<Record<string, number>>({});

  const toggle = useCallback((id: string) => {
    setPicked((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  }, []);

  const pickRoot = useCallback(async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: tr("选工作副本根目录") });
    if (typeof dir === "string" && dir) setRoot(dir);
  }, [tr]);

  const start = useCallback(async () => {
    if (!task.trim() || running) return;
    setRunning(true);
    setResults(null);
    setLog([]);
    setStars({});
    // 每个已勾选的工具逐个跑（后端按名单遍历，前端只负责收进度）。后端一跑就烧 token。
    const un = await listen<string>("uking:arena_progress", (e) => {
      setLog((prev) => [...prev.slice(-80), e.payload]);
    });
    try {
      const res = await invoke<ArenaResult[]>("arena_run", {
        task: task.trim(),
        workspace: root.trim() || ".",
        only: picked.length === TOOLS.length ? null : picked.join(","),
      });
      setResults(res);
      onToast?.(tr("竞技场跑完，看结果打星"));
    } catch (e) {
      onToast?.(`${tr("竞技场失败")}: ${e}`);
    } finally {
      un();
      setRunning(false);
    }
  }, [task, root, picked, running, onToast, tr]);

  // 结果按后端顺序排，没勾选的置灰
  const ordered = useMemo(() => {
    if (!results) return [];
    return TOOLS.map((t) => results.find((r) => r.tool === t.id)).filter(Boolean) as ArenaResult[];
  }, [results]);

  return (
    <div className="h-full flex flex-col bg-bg-2">
      <header className="px-5 pt-4 pb-3">
        <h2 className="text-[15px] font-semibold text-ink-0">{tr("竞技场")}</h2>
        <p className="text-[12px] text-ink-4 mt-0.5">
          {tr("六个 CLI 同任务横向比 —— 系统只出可观测量（耗时 / 退出码 / 有没有产出），质量由你打星")}
        </p>
      </header>

      <div className="flex-1 min-h-0 flex gap-4 px-5 pb-4">
        {/* 左：设置 */}
        <div className="w-[300px] shrink-0 flex flex-col gap-3">
          <div>
            <div className="text-[12px] font-medium text-ink-3 mb-1.5">{tr("参赛选手")}</div>
            <div className="grid grid-cols-2 gap-1.5">
              {TOOLS.map((t) => {
                const on = picked.includes(t.id);
                return (
                  <button
                    key={t.id}
                    onClick={() => toggle(t.id)}
                    className={"text-left px-2.5 py-2 rounded-lg border text-[12.5px] transition-colors " +
                      (on
                        ? "bg-accent/10 border-accent/40 text-ink-0"
                        : "bg-white/[0.03] border-white/[0.06] text-ink-4 hover:border-white/[0.14]")}
                  >
                    {t.name}
                  </button>
                );
              })}
            </div>
          </div>

          <div>
            <div className="text-[12px] font-medium text-ink-3 mb-1.5">{tr("同一个任务")}</div>
            <textarea
              value={task}
              onChange={(e) => setTask(e.target.value)}
              placeholder={tr("给所有参赛者的同一个任务，例如：把这个目录的 README 翻译成英文")}
              className="w-full h-[120px] resize-none bg-bg-1 border border-white/[0.08] rounded-lg px-3 py-2 text-[12.5px] text-ink-0 placeholder:text-ink-5 outline-none focus:border-accent/50"
            />
          </div>

          <div>
            <div className="text-[12px] font-medium text-ink-3 mb-1.5">{tr("工作副本根目录")}</div>
            <div className="flex gap-1.5">
              <input
                value={root}
                onChange={(e) => setRoot(e.target.value)}
                placeholder={tr("每个参赛者一个独立子目录，互不踩文件")}
                className="flex-1 min-w-0 bg-bg-1 border border-white/[0.08] rounded-lg px-2.5 py-2 text-[12px] text-ink-0 placeholder:text-ink-5 outline-none focus:border-accent/50"
              />
              <button
                onClick={pickRoot}
                className="shrink-0 px-3 py-2 rounded-lg border border-white/[0.1] text-[12px] text-ink-3 hover:border-white/[0.2] transition-colors"
              >
                {tr("选目录")}
              </button>
            </div>
            <p className="mt-1 text-[11px] text-ink-5">{tr("留空用当前工作目录。每个参赛者各开一个子目录，不直接共享。")}</p>
          </div>

          <button
            onClick={start}
            disabled={!task.trim() || running}
            className="mt-1 inline-flex items-center justify-center gap-1.5 px-4 py-2.5 rounded-lg bg-accent text-ink-0 text-[13px] font-medium disabled:opacity-40 transition-opacity"
          >
            <Play size={14} />
            {running ? tr("开赛中…") : tr("开赛（会烧 token）")}
          </button>

          <div className="text-[11px] text-ink-5 leading-relaxed">
            {tr("打星口径：跑得快不等于干得好。先看有没有真产出、退出码是否干净，再自己核对 stdout，最后打星。")}
          </div>
        </div>

        {/* 右：进度 + 结果 */}
        <div className="flex-1 min-w-0 min-h-0 flex flex-col gap-3">
          {running && log.length > 0 && (
            <div className="bg-white/[0.03] border border-white/[0.06] rounded-lg px-3 py-2 max-h-[140px] overflow-y-auto">
              {log.map((l, i) => (
                <div key={i} className="text-[11.5px] text-ink-4 font-mono whitespace-pre-wrap">{l}</div>
              ))}
            </div>
          )}

          {ordered.length === 0 ? (
            <div className="flex-1 grid place-items-center">
              <div className="text-center px-8">
                <div className="text-[13px] text-ink-3">{tr("勾选参赛者，塞同一个任务，点开赛")}</div>
                <div className="mt-1.5 text-[12px] text-ink-5">{tr("比的是同一个任务谁干活利索 —— 结果只列可观测量，质量靠人打星")}</div>
              </div>
            </div>
          ) : (
            <div className="flex-1 min-h-0 overflow-y-auto">
              <table className="w-full text-[12px]">
                <thead className="sticky top-0 bg-bg-2">
                  <tr className="text-ink-4 text-left">
                    <th className="py-2 pr-3 font-medium">{tr("选手")}</th>
                    <th className="py-2 pr-3 font-medium">{tr("耗时")}</th>
                    <th className="py-2 pr-3 font-medium">{tr("退出码")}</th>
                    <th className="py-2 pr-3 font-medium">{tr("产出")}</th>
                    <th className="py-2 pr-3 font-medium">{tr("stdout 尾部")}</th>
                    <th className="py-2 font-medium">{tr("打星")}</th>
                  </tr>
                </thead>
                <tbody>
                  {ordered.map((r) => (
                    <tr key={r.tool} className="border-t border-white/[0.05] align-top">
                      <td className="py-2.5 pr-3 whitespace-nowrap">
                        <div className="text-ink-0 font-medium">{TOOLS.find((t) => t.id === r.tool)?.name ?? r.tool}</div>
                        {!r.installed && <div className="text-[11px] text-ink-5 mt-0.5">{tr("未安装")}</div>}
                      </td>
                      <td className="py-2.5 pr-3 whitespace-nowrap text-ink-3">
                        {r.ran ? (r.timed_out ? tr("超时") : `${r.ms}ms`) : "—"}
                      </td>
                      <td className="py-2.5 pr-3 whitespace-nowrap text-ink-3">
                        {r.exit_code === 0 ? <span className="text-green-400">0</span> : r.exit_code ?? "—"}
                      </td>
                      <td className="py-2.5 pr-3 whitespace-nowrap">
                        {r.ran ? (
                          r.produced ? (
                            <span className="text-green-400">{tr("有")}</span>
                          ) : (
                            <span className="text-red-400">{tr("空")}</span>
                          )
                        ) : "—"}
                      </td>
                      <td className="py-2.5 pr-3 max-w-[280px]">
                        <div className="text-ink-4 font-mono text-[11px] leading-snug line-clamp-3 whitespace-pre-wrap" title={r.stdout_tail}>
                          {r.stdout_tail || r.note || "—"}
                        </div>
                      </td>
                      <td className="py-2.5 whitespace-nowrap">
                        <Stars value={stars[r.tool] ?? 0} onChange={(n) => setStars((s) => ({ ...s, [r.tool]: n }))} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** 1..5 星（人打星 —— 竞技场里唯一的「质量」输入）。点已选中的星归零。 */
function Stars({ value, onChange }: { value: number; onChange: (n: number) => void }) {
  return (
    <div className="flex gap-0.5">
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          onClick={() => onChange(value === n ? 0 : n)}
          className="p-0.5 text-ink-5 hover:text-yellow-400 transition-colors"
          title={`${n} 星`}
        >
          <Star size={13} className={n <= value ? "text-yellow-400 fill-yellow-400" : ""} />
        </button>
      ))}
    </div>
  );
}
