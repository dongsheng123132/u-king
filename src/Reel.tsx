/**
 * 客户端一键成片：状态壳复用 Video.tsx 的模块级 store + 历史按需取文件路径，
 * 生成本身绝不在前端重写，统一交给 Rust 壳调用成熟的 gen-reel.mjs。
 */
import { useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Clapperboard, Film, Loader2, Play, RotateCcw, Trash2, Wallet } from "lucide-react";
import type { DeviceKey } from "./lib/types";
import { useI18n } from "./i18n";
import { ACTION, createTauriActionClient } from "./generated/action-client";

type ReelItem = {
  id: number; prompt: string; shots: string[]; narration?: string | null; voice?: string | null;
  bgm_prompt?: string | null; resolution?: string | null; preset_id?: string | null;
  status: "running" | "done" | "failed" | "degraded" | string; have_video: boolean;
  error?: string | null; degraded: boolean; warnings: string[]; ts: number;
};
type ReelProgress = { id: number; phase: string; detail: string };
type ReelPreset = { schema_version: number; id: string; title: string; description: string };
type MediaTask = { id: number; prompt: string; status?: string; ts: number; error?: string | null; have_video?: boolean; src?: string | null };

const reelState = {
  busy: false, currentId: null as number | null, prompt: "", narration: "", bgm: false, bgmPrompt: "", resolution: "720p", presetId: null as string | null, progress: "",
};
const subscribers = new Set<() => void>();
const notify = () => subscribers.forEach((fn) => fn());
const actionClient = createTauriActionClient((command, args) => invoke(command, args), { surface: "gui:creator-reel" });

function fmt(ts: number) { return ts ? new Date(ts).toLocaleString() : "—"; }
function phaseLabel(phase: string, t: (s: string) => string) {
  return ({ dialogue: t("1/5 对白"), storyboard: t("2/5 分镜"), video: t("3/5 视频"), voice: t("4/5 配音"), stitch: t("5/5 拼接+BGM") } as Record<string, string>)[phase] || t("处理中");
}

export function Reel({ deviceKey, onToast, onRecharge }: { deviceKey: DeviceKey | null; onToast: (s: string) => void; onRecharge: () => void }) {
  const { t } = useI18n();
  const [, tick] = useState(0);
  const [items, setItems] = useState<ReelItem[]>([]);
  const [presets, setPresets] = useState<ReelPreset[]>([]);
  const [playing, setPlaying] = useState<string | null>(null);
  const render = () => tick((v) => v + 1);
  const reload = () => invoke<ReelItem[]>("list_reel_history").then(setItems).catch((e) => onToast(String(e)));
  useEffect(() => { subscribers.add(render); return () => { subscribers.delete(render); }; }, []);
  useEffect(() => { void reload(); }, []);
  useEffect(() => {
    void actionClient(ACTION.RUNTIME_CREATOR_REEL_PRESETS_INSPECT, {}).then((envelope) => {
      if (!envelope.ok) throw new Error(envelope.error.message);
      const list = (envelope.result as { presets?: unknown }).presets;
      // 目录来自后端，但仍防御性只展示已承诺的 v1 schema；提交时会再由 Rust 白名单校验。
      setPresets(Array.isArray(list) ? list.filter((preset): preset is ReelPreset => typeof preset === "object" && preset !== null && (preset as ReelPreset).schema_version === 1 && /^[a-z0-9-]{1,48}$/.test((preset as ReelPreset).id)) : []);
    }).catch((e) => onToast(String(e)));
  }, []);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<ReelProgress>("uking:reel_progress", (event) => {
      if (event.payload.id === reelState.currentId) { reelState.progress = event.payload.detail; notify(); }
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  const submit = async () => {
    if (reelState.busy) return;
    const prompt = reelState.prompt.trim();
    if (!prompt) return onToast(t("请先描述想做成片的内容"));
    reelState.busy = true; reelState.progress = t("正在提交一键成片任务…"); notify();
    try {
      const id = await invoke<number>("submit_reel", { params: {
        prompt, shots: [], narration: reelState.narration.trim() || null, voice: "Cherry",
        bgm_prompt: reelState.bgm ? reelState.bgmPrompt.trim() || prompt : null, resolution: reelState.resolution, preset_id: reelState.presetId,
      }});
      reelState.currentId = id; reelState.prompt = ""; reelState.narration = ""; onToast(t("一键成片完成"));
      await reload(); await play(id);
    } catch (e) { onToast(t("一键成片失败：") + String(e)); await reload(); }
    finally { reelState.busy = false; reelState.currentId = null; reelState.progress = ""; notify(); }
  };
  const play = async (id: number) => {
    try { setPlaying(convertFileSrc(await invoke<string>("read_reel_file", { id }))); }
    catch (e) { onToast(String(e)); }
  };
  const regenerate = async (id: number) => {
    if (!window.confirm(t("重新生成将产生新费用，是否继续？"))) return;
    reelState.busy = true; reelState.currentId = id; reelState.progress = t("正在按原参数重新生成（将产生新费用）…"); notify();
    try { await invoke("resume_reel", { id }); onToast(t("重新生成完成")); await reload(); await play(id); }
    catch (e) { onToast(t("重新生成失败：") + String(e)); await reload(); }
    finally { reelState.busy = false; reelState.currentId = null; reelState.progress = ""; notify(); }
  };
  const remove = async (id: number) => { await invoke("delete_reel", { id }); if (playing) setPlaying(null); await reload(); };
  const balance = deviceKey?.balance?.cny;
  const busy = reelState.busy;

  return <div className="flex h-full min-h-0 flex-col gap-3">
    <header className="flex shrink-0 flex-wrap items-center justify-between gap-3 rounded-card border border-ink-6 bg-bg-1 px-4 py-3 shadow-card">
      <div className="flex items-center gap-2"><Clapperboard size={19} className="text-accent" /><div><h1 className="font-semibold text-ink-0">{t("一键成片")}</h1><p className="text-xs text-ink-3">{t("分镜 · 视频 · 旁白 · BGM，一次完成")}</p></div></div>
      <div className="flex items-center gap-2 text-sm"><Wallet size={15} className="text-ink-3" /><span>{t("余额")} {balance == null ? "—" : `¥${balance}`}</span><button onClick={onRecharge} className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white">{t("充值")}</button></div>
    </header>
    <div className="min-h-0 flex-1 overflow-y-auto space-y-3 pr-1">
      {busy && <section className="rounded-card border border-accent/25 bg-accent/[0.06] p-4"><div className="flex items-center gap-2 text-sm font-medium text-ink-1"><Loader2 size={16} className="animate-spin text-accent" />{phaseLabel((reelState.progress.match(/【(\d)\/5/)?.[1] === "1" ? "dialogue" : reelState.progress.match(/【(\d)\/5/)?.[1] === "2" ? "storyboard" : reelState.progress.match(/【(\d)\/5/)?.[1] === "3" ? "video" : reelState.progress.match(/【(\d)\/5/)?.[1] === "4" ? "voice" : "stitch"), t)}</div><p className="mt-2 text-xs text-ink-3">{reelState.progress || t("处理中")}</p><p className="mt-2 text-xs text-ink-4">{t("切换页面不会中断；生成过程可能需要几分钟")}</p></section>}
      {playing && <section className="rounded-card border border-ink-6 bg-bg-1 p-3 shadow-card"><video src={playing} controls autoPlay className="max-h-[52vh] w-full rounded-lg bg-black" /></section>}
      <section className="rounded-card border border-ink-6 bg-bg-1 p-4 shadow-card"><div className="mb-3 flex items-center gap-2"><Film size={17} className="text-accent"/><h2 className="font-medium text-ink-0">{t("最近成片")}</h2></div>{items.length === 0 ? <p className="py-5 text-center text-sm text-ink-4">{t("还没有成片，先在下方写一个画面")}</p> : <div className="space-y-2">{items.map((item) => <article key={item.id} className="rounded-lg border border-ink-6 bg-bg-0 p-3"><div className="flex flex-wrap items-start gap-2"><span className={`rounded-full px-2 py-0.5 text-[11px] ${item.status === "done" ? "bg-emerald-500/10 text-emerald-600" : item.status === "degraded" ? "bg-warning-500/15 text-warning-700" : item.status === "failed" ? "bg-danger-500/10 text-danger-500" : "bg-accent/10 text-accent"}`}>{item.status === "degraded" ? t("已降级交付") : item.status === "done" ? t("已完成") : item.status === "failed" ? t("失败") : t("处理中")}</span><p className="min-w-0 flex-1 text-sm text-ink-1">{item.prompt || item.shots[0] || t("分镜成片")}</p><time className="text-[11px] text-ink-4">{fmt(item.ts)}</time></div>
        {(item.degraded || item.warnings.length > 0 || item.error) && <div className={`mt-2 flex gap-1.5 rounded-lg px-2.5 py-2 text-xs ${item.status === "failed" ? "bg-danger-500/[0.08] text-danger-600" : "bg-warning-500/[0.10] text-warning-700"}`}><AlertTriangle size={14} className="shrink-0"/><span>{item.error || (item.degraded ? t("旁白失败，成片无声") : item.warnings.join("；"))}{item.warnings.length > 0 && item.degraded ? ` · ${item.warnings.join("；")}` : ""}</span></div>}
        <div className="mt-3 flex flex-wrap gap-2"><button disabled={!item.have_video} onClick={() => void play(item.id)} className="inline-flex items-center gap-1 rounded border border-ink-5 px-2.5 py-1.5 text-xs text-ink-2 disabled:opacity-40"><Play size={13}/>{t("播放")}</button>{(item.status === "failed" || item.status === "running") && <button onClick={() => void regenerate(item.id)} className="inline-flex items-center gap-1 rounded border border-warning-500/30 px-2.5 py-1.5 text-xs text-warning-700"><RotateCcw size={13}/>{t("重新生成")}</button>}<button onClick={() => void remove(item.id)} className="ml-auto inline-flex items-center gap-1 rounded px-2 py-1.5 text-xs text-ink-4 hover:text-danger-500"><Trash2 size={13}/>{t("删除")}</button></div>
      </article>)}</div>}</section>
    </div>
    <section className="shrink-0 rounded-card border border-ink-6 bg-bg-1 p-3 shadow-card space-y-2"><textarea value={reelState.prompt} onChange={(e) => { reelState.prompt = e.target.value; notify(); }} placeholder={t("例如：赛博朋克城市夜景，霓虹灯牌，镜头缓慢推进")} rows={2} className="w-full resize-none rounded-lg border border-ink-6 bg-bg-0 px-3 py-2 text-sm text-ink-1 outline-none focus:border-accent/50"/>
      {presets.length > 0 && <div><p className="mb-1.5 text-xs font-medium text-ink-3">创作预设 <span className="font-normal text-ink-4">（可选；只定义视觉风格，BGM 仍须手动开启）</span></p><div className="grid gap-2 sm:grid-cols-3">{presets.map((preset) => <button key={preset.id} type="button" aria-pressed={reelState.presetId === preset.id} onClick={() => { reelState.presetId = reelState.presetId === preset.id ? null : preset.id; notify(); }} className={`rounded-lg border p-2 text-left transition-colors ${reelState.presetId === preset.id ? "border-accent/50 bg-accent/[0.08]" : "border-ink-6 bg-bg-0 hover:border-ink-5"}`}><span className="block text-xs font-medium text-ink-1">{preset.title}</span><span className="mt-0.5 block text-[11px] leading-4 text-ink-4">{preset.description}</span></button>)}</div></div>}
      <div className="grid gap-2 sm:grid-cols-3"><input value={reelState.narration} onChange={(e) => { reelState.narration = e.target.value; notify(); }} placeholder={t("可选旁白")} className="rounded-lg border border-ink-6 bg-bg-0 px-3 py-2 text-xs text-ink-1 outline-none"/><select value={reelState.resolution} onChange={(e) => { reelState.resolution = e.target.value; notify(); }} className="rounded-lg border border-ink-6 bg-bg-0 px-3 py-2 text-xs text-ink-1"><option value="480p">480p</option><option value="720p">720p</option></select><label className="flex items-center gap-2 rounded-lg border border-ink-6 bg-bg-0 px-3 text-xs text-ink-2"><input type="checkbox" checked={reelState.bgm} onChange={(e) => { reelState.bgm = e.target.checked; notify(); }}/>{t("添加 BGM")}</label></div>{reelState.bgm && <input value={reelState.bgmPrompt} onChange={(e) => { reelState.bgmPrompt = e.target.value; notify(); }} placeholder={t("BGM 描述，例如轻快电子乐")} className="w-full rounded-lg border border-ink-6 bg-bg-0 px-3 py-2 text-xs text-ink-1 outline-none"/>}<div className="flex items-center justify-between gap-3"><span className="text-xs text-ink-4">{t("预估费用以服务端实际计费为准")} · {balance == null ? t("请先查询余额") : `¥${balance}`}</span><button onClick={() => void submit()} disabled={busy} className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white disabled:opacity-50">{busy ? <Loader2 size={15} className="animate-spin"/> : <Clapperboard size={15}/>} {busy ? t("成片中") : t("一键成片")}</button></div></section>
  </div>;
}

/** M1 任务中心：只归一展示三份已存在的历史，不建立跨类任务状态或新的后端事实。 */
export function MediaTasks({ onGo }: { onGo: (tab: "draw" | "video" | "reel") => void }) {
  const { t } = useI18n(); const [tasks, setTasks] = useState<{ kind: "draw" | "video" | "reel"; item: MediaTask }[]>([]);
  useEffect(() => { void Promise.all([invoke<MediaTask[]>("list_draw_history"), invoke<MediaTask[]>("list_video_history"), invoke<MediaTask[]>("list_reel_history")]).then(([draw, video, reel]) => setTasks([...draw.map((item) => ({ kind: "draw" as const, item })), ...video.map((item) => ({ kind: "video" as const, item })), ...reel.map((item) => ({ kind: "reel" as const, item }))].sort((a,b) => b.item.ts-a.item.ts).slice(0, 24))); }, []);
  const label = (k: string) => k === "draw" ? t("作图") : k === "video" ? t("视频") : t("一键成片");
  return <div className="max-w-5xl mx-auto space-y-4"><header className="rounded-card border border-ink-6 bg-bg-1 p-5 shadow-card"><h1 className="text-xl font-semibold text-ink-0">{t("任务中心")}</h1><p className="mt-1 text-sm text-ink-3">{t("最近的作图、视频和一键成片；这里只展示，不改变原任务流程")}</p></header><section className="rounded-card border border-ink-6 bg-bg-1 p-4 shadow-card">{tasks.length === 0 ? <p className="py-10 text-center text-sm text-ink-4">{t("还没有创作任务")}</p> : <div className="divide-y divide-ink-6">{tasks.map(({kind,item}) => <div key={`${kind}-${item.id}`} className="flex flex-wrap items-center gap-3 py-3"><span className="rounded bg-accent/10 px-2 py-1 text-xs text-accent">{label(kind)}</span><span className="min-w-0 flex-1 truncate text-sm text-ink-1">{item.prompt || t("创作任务")}</span><span className="text-xs text-ink-4">{item.status || (item.src ? t("已完成") : item.error ? t("失败") : t("已完成"))}</span><time className="text-xs text-ink-4">{fmt(item.ts)}</time><button onClick={() => onGo(kind)} className="rounded border border-ink-5 px-2.5 py-1.5 text-xs text-ink-2">{t("打开")}</button></div>)}</div>}</section></div>;
}
