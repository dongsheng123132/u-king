/**
 * AI 视频 —— 独立侧栏板块（与 AI 作图同级，代码完全解耦）。
 *
 * 视频是**异步任务**：提交 → 后端轮询出片 → 下载落盘。后端一条命令 `generate_video` 走完全程，
 * 进度走事件 `uking:video_progress`（{id,phase,progress}）。出图中状态放模块级（切走再回来还在）。
 * 历史落盘 `~/.uking/video/`（video.rs），列表不带视频字节，点「播放」才按需 `read_video` 取磁盘路径
 * 转 asset 协议直接流播（大文件不走 base64 data URL，避免卡 WebView2）。
 * app 中途关：running 记录留痕，重开本页自动 `resume_video` 续跑。
 *
 * 纯前端组件，只靠 props 通信（deviceKey/onToast/onRecharge），符合模块解耦铁律。
 */
import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useDropZone } from "./lib/fileDrop";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Clapperboard, Download, ImagePlus, Loader2, Package, Play, RotateCcw, Sparkles, Trash2, Wallet, X } from "lucide-react";
import type { DeviceKey } from "./lib/types";
import { VIDEO_MODELS } from "./lib/models";
import { CopyKey } from "./components/CopyKey";
import { classifyError } from "./lib/errorKind";
import { ModelMenu } from "./components/ModelMenu";
import { useI18n } from "./i18n";

/** 翻译函数类型（模块级 helper 里透传，组件内用 useI18n 拿）。 */
type TFn = (zh: string, vars?: Record<string, string | number>) => string;

/** 一条视频记录（后端 VideoItemOut 镜像）。status: running | ready | done | failed。 */
type VideoItem = {
  id: number;
  prompt: string;
  model: string;
  task_id: string;
  status: string;
  have_video: boolean;
  error?: string | null;
  ts: number;
};

/* ---------------- 模块级 store（进行中状态 + 草稿，历史以磁盘为准） ---------------- */

const videoState: {
  busy: boolean;
  pendingPrompt: string;
  pendingRef: string | null;
  progress: string;
  prompt: string;
  model: string;
  ref: string | null;
} = {
  busy: false,
  pendingPrompt: "",
  pendingRef: null,
  progress: "",
  prompt: "",
  model: VIDEO_MODELS[0].id,
  ref: null,
};
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((f) => f());

/** File → data URL（首帧图，后端归一成 image_url）。 */
function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const fr = new FileReader();
    fr.onload = () => resolve(String(fr.result));
    fr.onerror = () => reject(fr.error);
    fr.readAsDataURL(file);
  });
}

/** 设首帧图（data URL）+ 按模型能力自动切到支持 i2v 的模型。
 *  图生视频(i2v)实测只有 Veo 这条上游渠道可用：选了不支持首帧图的模型(如万相)自动切到 Veo。 */
function applyRef(dataUrl: string, onToast: (s: string) => void, t: TFn) {
  videoState.ref = dataUrl;
  const cur = VIDEO_MODELS.find((m) => m.id === videoState.model);
  if (cur && !cur.i2v) {
    const fb = VIDEO_MODELS.find((m) => m.i2v);
    if (fb) {
      videoState.model = fb.id;
      onToast(t("已切到{name}做图生视频（所选模型不支持首帧图）", { name: fb.label.split("·")[0].trim() }));
    }
  }
  notify();
}

/** 取第一张图片当首帧（图生视频只用一张首帧）。文件选择/剪贴板粘贴走这条。 */
async function setRef(files: FileList | File[], onToast: (s: string) => void, t: TFn) {
  const img = Array.from(files).find((f) => f.type.startsWith("image/"));
  if (!img) {
    onToast(t("只能用图片当首帧"));
    return;
  }
  try {
    applyRef(await fileToDataUrl(img), onToast, t);
  } catch {
    onToast(t("读取图片失败"));
  }
}

/** 拖文件进来（Tauri 原生拖放给的是路径）取第一张图片当首帧 —— 读盘转 data URL。 */
async function setRefFromPaths(paths: string[], onToast: (s: string) => void, t: TFn) {
  const img = paths.find((p) => /\.(png|jpe?g|webp|gif|bmp)$/i.test(p));
  if (!img) {
    onToast(t("只能用图片当首帧"));
    return;
  }
  const url = await invoke<string>("read_file_data_url", { path: img }).catch(() => null);
  if (!url) {
    onToast(t("读取图片失败"));
    return;
  }
  applyRef(url, onToast, t);
}

/** 提交一次视频生成：后端跑完全程（提交→轮询→下载），完成后回调 reload 历史。
 *  成功时把新记录 id 传给 onDone —— 调用方拿它自动加载播放器（测试报告 #003：
 *  等了 1～3 分钟出的片还要再点一下「播放」才看得到，等于白等）。 */
async function runGenerate(onToast: (s: string) => void, onDone: (id?: number) => void, t: TFn) {
  if (videoState.busy) return;
  const prompt = videoState.prompt.trim();
  if (!prompt) {
    onToast(t("请先输入要生成的视频画面"));
    return;
  }
  const { ref } = videoState;
  let model = videoState.model;
  // 安全网：带首帧图但模型不支持 i2v（先选模型后拖图等路径）→ 静默切到 Veo，避免提交必失败
  if (ref) {
    const cur = VIDEO_MODELS.find((m) => m.id === model);
    if (cur && !cur.i2v) {
      const fb = VIDEO_MODELS.find((m) => m.i2v);
      if (fb) {
        model = fb.id;
        videoState.model = fb.id;
      }
    }
  }
  videoState.busy = true;
  videoState.pendingPrompt = prompt;
  videoState.pendingRef = ref;
  // 图生视频要把整张首帧图传到上游、上游同步处理后才回 task_id（实测可达 1–2 分钟），
  // 给明确的等待预期，免得客户以为卡死而反复重试（每次重试都白扣额度）。
  videoState.progress = ref ? t("上传首帧图并提交中…（图生视频较慢，请耐心等 1–3 分钟，勿重复点）") : t("提交中…");
  videoState.prompt = "";
  videoState.ref = null;
  notify();
  let doneId: number | undefined;
  try {
    doneId = await invoke<number>("generate_video", { prompt, model, image: ref });
    onToast(t("视频生成完成"));
  } catch (e) {
    const msg = String(e);
    // 余额/Key/网络/服务端过载/安全审核都不是 bug —— 判据见 lib/errorKind.ts（与作图共用一份）。
    const { actionable, hint } = classifyError(msg);
    onToast(actionable ? (hint ? t(hint) : msg) : t("视频生成失败：") + msg);
    if (!actionable) {
      invoke("report_bug", {
        kind: ref ? "image_video_failed" : "video_failed",
        summary: `${ref ? "图生视频" : "视频"}失败 model=${model}: ${msg}`.slice(0, 200),
        // 只留诊断需要的信号，不带用户实际输入的 prompt 内容（那是用户创作内容，不是软件 bug 数据）。
        detail: `promptLen=${prompt.length}\nmodel=${model}\ni2v=${!!ref}\nerror=${msg}`,
      }).catch(() => {});
    }
  } finally {
    videoState.busy = false;
    videoState.pendingPrompt = "";
    videoState.pendingRef = null;
    videoState.progress = "";
    notify();
    onDone(doneId);
  }
}

/* ---------------- 组件 ---------------- */

export function Video({
  deviceKey,
  onToast,
  onRecharge,
  onGoSkillPack,
}: {
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onRecharge: () => void;
  /** 跳到「AI 技能包」页 —— 把视频能力打包成可复制/导出的技能，给别的 AI 工具调用。 */
  onGoSkillPack: () => void;
}) {
  const { t } = useI18n();
  const [, force] = useState(0);
  const reRef = useRef(() => force((n) => n + 1));
  const [items, setItems] = useState<VideoItem[]>([]);
  const [players, setPlayers] = useState<Record<number, string>>({}); // id → asset URL（点播放/生成完自动加载）
  // 只给「最新加载的这条」autoPlay —— 老的 <video> 改这个 prop 不会打断已在播的
  const [autoId, setAutoId] = useState<number | null>(null);
  // 拖文件进来 = 设首帧图（统一走 Tauri 原生拖放，拿真实路径）；over 用于高亮遮罩
  const { ref: dropRef, over: dragOver } = useDropZone<HTMLDivElement>((paths) => void setRefFromPaths(paths, onToast, t));
  const scrollRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const composingRef = useRef(false);
  const resumedRef = useRef(false);

  const reloadHistory = () =>
    invoke<VideoItem[]>("list_video_history")
      .then((list) => {
        setItems(list);
        // 首次加载：把还在 running 的记录自动续跑（app 重开后接着轮询/下载）
        if (!resumedRef.current) {
          resumedRef.current = true;
          list
            .filter((it) => it.status === "running" || it.status === "ready")
            .forEach((it) =>
              invoke("resume_video", { id: it.id })
                .then(() => void autoLoad(it.id)) // 续跑出的片同样自动加载（#003）
                .catch(() => {})
                .finally(() => void reloadHistory())
            );
        }
      })
      .catch(() => {});

  useEffect(() => {
    const f = reRef.current;
    listeners.add(f);
    void reloadHistory();
    // 进度事件：更新进行中文案
    const un = listen<{ id: number; phase: string; progress: string }>("uking:video_progress", (e) => {
      const p = e.payload;
      const pct = p.progress || "";
      videoState.progress = p.phase === "running" ? t("生成中 {pct}", { pct }) : pct;
      notify();
    });
    return () => {
      listeners.delete(f);
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const { busy, pendingPrompt, pendingRef, progress, prompt, model, ref } = videoState;
  const selectedModel = VIDEO_MODELS.find((item) => item.id === model);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [items, busy]);

  const ordered = [...items].reverse();

  const play = async (id: number) => {
    if (players[id]) return;
    try {
      // read_video 返回磁盘 mp4 的绝对路径（不再是整档编码的 base64 data URL —— 视频常有
      // 几 MB～十几 MB，塞 data URL 会让 WebView2 长时间卡在解码上），转成 asset 协议 URL
      // 直接流播，不经过 IPC 序列化。
      const path = await invoke<string>("read_video", { id });
      setPlayers((m) => ({ ...m, [id]: convertFileSrc(path) }));
      setAutoId(id);
    } catch (e) {
      onToast(t("读取视频失败：") + String(e));
    }
  };

  // 生成/续跑完成 → 不等客户再点一下「播放」，直接把片装进播放器（测试报告 #003）。
  // 但**只有本页此刻可见才 autoPlay**：本组件是保活挂载的（切走只 display:none），
  // 客户在别的页面等片时突然出声，比「要多点一下」吓人得多 —— 那种情况只预加载，
  // 回来就是现成的播放器。加载失败不吭声：原来的「播放视频」按钮还在，点它兜底。
  const autoLoad = async (id: number) => {
    try {
      const path = await invoke<string>("read_video", { id });
      setPlayers((m) => ({ ...m, [id]: convertFileSrc(path) }));
      const visible = (scrollRef.current?.getBoundingClientRect().width ?? 0) > 0;
      if (visible) setAutoId(id);
    } catch {
      /* 按需加载路径仍可用 */
    }
  };

  const onSubmit = () =>
    void runGenerate(
      onToast,
      (id) => {
        void reloadHistory();
        if (id != null) void autoLoad(id);
      },
      t,
    );

  // 另存为：后端原生「保存」对话框 + 拷磁盘 mp4（不依赖 <a download>）
  const exportVideo = async (id: number) => {
    try {
      const p = await invoke<string | null>("export_video", { id });
      if (p) onToast(t("已保存到：") + p);
    } catch (e) {
      onToast(t("保存失败：") + String(e));
    }
  };

  const retryExisting = async (id: number) => {
    onToast(t("正在继续原任务，只重试查询和下载，不会重新生成或扣费"));
    try {
      await invoke("resume_video", { id });
      await reloadHistory();
      await autoLoad(id);
      onToast(t("视频已恢复并下载完成"));
    } catch (e) {
      await reloadHistory();
      onToast(t("原任务暂时还没下载下来：") + String(e));
    }
  };

  return (
    <div ref={dropRef} className="relative flex flex-col h-full min-h-0">
      {dragOver && (
        <div className="absolute inset-0 z-20 rounded-card border-2 border-dashed border-accent/60 bg-accent/[0.06] grid place-items-center pointer-events-none">
          <div className="flex items-center gap-2 text-accent text-[14px] font-semibold">
            <ImagePlus size={18} /> {t("松手设为首帧图（图生视频）")}
          </div>
        </div>
      )}

      {/* 顶栏：标题 + 充值 */}
      <section className="flex flex-wrap items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-3 shrink-0">
        <Clapperboard size={18} className="text-accent shrink-0" />
        <div className="min-w-0">
          <div className="text-[15px] font-semibold text-ink-0">{t("AI 视频")}</div>
          <div className="text-[12px] text-ink-3">{t("文字生成视频 · 拖入首帧图可图生视频 · 异步出片约 1～3 分钟")}</div>
        </div>
        <div className="ml-auto flex items-center gap-2">
          {/* 把视频能力打包给别的 AI 工具调用 —— 入口在这里（实际复制/导出在「AI 技能包」页） */}
          <button
            onClick={onGoSkillPack}
            title={t("把作图 / 视频能力打包成技能，复制说明或导出文件夹，给 OpenClaw / ClawX / Claude Code 等任意 AI 调用")}
            className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-accent/30 bg-accent/[0.06] text-accent text-[12px] font-medium hover:bg-accent/[0.12]"
          >
            <Package size={13} /> {t("打包给 AI 用")}
          </button>
          {deviceKey?.key && <CopyKey apiKey={deviceKey.key} onToast={onToast} />}
          {items.length > 0 && (
            <button
              onClick={async () => {
                await invoke("clear_video_history").catch(() => {});
                setPlayers({});
                await reloadHistory();
                onToast(t("已清空视频记录"));
              }}
              className="inline-flex items-center gap-1 px-2.5 h-8 rounded-lg border border-white/[0.10] text-ink-4 text-[11px] hover:text-ink-1 hover:bg-white/[0.04]"
            >
              <Trash2 size={12} /> {t("清空")}
            </button>
          )}
          <button
            onClick={onRecharge}
            className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-ink-1 text-[12px] hover:bg-white/[0.04]"
          >
            <Wallet size={13} /> {t("充值")}
          </button>
        </div>
      </section>

      {/* 对话流 */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto py-4 space-y-4">
        {ordered.length === 0 && !busy && (
          <div className="h-full grid place-items-center text-center">
            <div>
              <Clapperboard size={30} className="text-accent mx-auto mb-3 opacity-70" />
              <p className="text-[14px] text-ink-1 mb-1">{t("描述你想要的视频画面，回车即生成")}</p>
              <p className="text-[12px] text-ink-4">{t("例如：一只橘猫宇航员在月球上慢跑，电影感，星空背景")}</p>
              <p className="text-[12px] text-ink-5 mt-2">{t("视频较慢，出片约 1～3 分钟，切走也不中断")}</p>
            </div>
          </div>
        )}

        {ordered.map((it) => (
          <div key={it.id} className="space-y-2">
            {/* 提示词气泡 */}
            <div className="flex justify-end">
              <div
                data-on-dark
                className="max-w-[78%] rounded-2xl rounded-br-sm bg-accent/90 text-white px-3.5 py-2 text-[13px] leading-snug"
              >
                {it.prompt}
                <span className="ml-2 text-[10px] text-white/70 font-mono">{it.model.replace(/-generate-001$/, "")}</span>
              </div>
            </div>
            {/* 结果气泡 */}
            <div className="flex justify-start">
              {it.status === "done" && it.have_video ? (
                <div className="max-w-[78%] rounded-2xl rounded-bl-sm bg-bg-2 border border-white/[0.06] p-2 space-y-2">
                  {players[it.id] ? (
                    <video
                      src={players[it.id]}
                      controls
                      autoPlay={autoId === it.id}
                      className="max-w-full max-h-[52vh] rounded-lg bg-black"
                    />
                  ) : (
                    <button
                      onClick={() => void play(it.id)}
                      className="flex items-center gap-2 px-4 py-3 rounded-lg bg-bg-1 hover:bg-bg-3 text-ink-1 text-[13px]"
                    >
                      <Play size={16} className="text-accent" /> {t("播放视频")}
                    </button>
                  )}
                  <button
                    onClick={() => void exportVideo(it.id)}
                    className="inline-flex items-center gap-1.5 px-3 h-7 rounded-lg border border-white/[0.10] text-ink-1 text-[11px] hover:bg-white/[0.04]"
                  >
                    <Download size={12} /> {t("下载")}
                  </button>
                </div>
              ) : it.status === "running" ? (
                <div className="rounded-2xl rounded-bl-sm bg-bg-2 border border-white/[0.06] px-4 py-3 flex items-center gap-2 text-[12px] text-ink-3">
                  <Loader2 size={15} className="text-accent animate-spin" />
                  {t("生成中…（重开后已自动续跑，约 1～3 分钟）")}
                </div>
              ) : it.status === "ready" ? (
                <div className="max-w-[78%] rounded-2xl rounded-bl-sm bg-warning-500/[0.08] border border-warning-500/25 px-3.5 py-3 space-y-2 text-[11.5px] text-ink-2">
                  <div className="flex items-start gap-2 leading-snug">
                    <Download size={14} className="shrink-0 mt-0.5 text-warning-400" />
                    <span>{it.error || t("视频已经生成，正在等待下载到本机")}</span>
                  </div>
                  <button
                    onClick={() => void retryExisting(it.id)}
                    className="inline-flex items-center gap-1.5 px-3 h-7 rounded-lg border border-warning-500/30 text-warning-700 dark:text-warning-400 hover:bg-warning-500/[0.08]"
                  >
                    <RotateCcw size={12} /> {t("重试下载（不重新扣费）")}
                  </button>
                </div>
              ) : (
                <div className="max-w-[78%] flex items-start gap-2 rounded-2xl rounded-bl-sm bg-danger-500/[0.08] border border-danger-500/25 px-3.5 py-2.5 text-[11.5px] text-danger-400">
                  <AlertCircle size={14} className="shrink-0 mt-0.5" />
                  <div className="leading-snug">{t("视频生成失败：")}{it.error || t("未知错误")}</div>
                </div>
              )}
            </div>
          </div>
        ))}

        {/* 出图中 */}
        {busy && (
          <div className="space-y-2">
            <div className="flex justify-end">
              <div className="max-w-[78%] rounded-2xl rounded-br-sm bg-accent/90 text-white px-3.5 py-2 text-[13px] leading-snug">
                {pendingRef && (
                  <img
                    src={pendingRef}
                    alt={t("首帧")}
                    className="w-12 h-12 rounded object-cover border border-white/30 mb-1.5"
                  />
                )}
                {pendingPrompt}
              </div>
            </div>
            <div className="flex justify-start">
              <div className="rounded-2xl rounded-bl-sm bg-bg-2 border border-white/[0.06] px-4 py-3 flex items-center gap-2 text-[12px] text-ink-3">
                <Loader2 size={15} className="text-accent animate-spin" />
                {progress || t("生成中…")} · {t("视频较慢，切到别的页面也不会中断")}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 输入框钉底 */}
      <section className="shrink-0 rounded-card border border-white/[0.08] bg-bg-2/80 p-2.5 space-y-2">
        {/* 首帧图（图生视频） */}
        {ref && (
          <div className="flex items-center gap-2">
            <span className="text-[11px] text-ink-4">{t("首帧图（图生视频）：")}</span>
            <div className="relative">
              <img src={ref} alt={t("首帧")} className="w-12 h-12 rounded-lg object-cover border border-white/[0.12]" />
              <button
                onClick={() => {
                  videoState.ref = null;
                  notify();
                }}
                title={t("移除")}
                className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-bg-1 border border-white/[0.2] grid place-items-center text-ink-2 hover:text-danger-400 hover:border-danger-500/40"
              >
                <X size={11} />
              </button>
            </div>
          </div>
        )}

        {/* flex-wrap（测试报告 #014「功能条固定死，不随窗口自适应」）：
            窗口一窄，模型选择 / 画质 / 尺寸 / 生成 会互相挤扁到点不准，
            甚至把输入框压成一条缝。允许换行 + 给输入框留最小宽度，让它整行独占。 */}
        <div className="flex items-end gap-2 flex-wrap">
          {/* 加首帧图按钮（点选；拖入/粘贴也行） */}
          <button
            onClick={() => fileRef.current?.click()}
            title={t("设首帧图做图生视频（也可拖入或粘贴）")}
            className="h-9 w-9 shrink-0 grid place-items-center rounded-lg border border-white/[0.10] text-ink-2 hover:text-accent hover:bg-white/[0.04]"
          >
            <ImagePlus size={16} />
          </button>
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            className="hidden"
            onChange={(e) => {
              if (e.target.files?.length) void setRef(e.target.files, onToast, t);
              e.target.value = "";
            }}
          />
          <textarea
            value={prompt}
            onChange={(e) => {
              videoState.prompt = e.target.value;
              notify();
            }}
            onKeyDown={(e) => {
              if (composingRef.current || e.nativeEvent.isComposing) return;
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                onSubmit();
              }
            }}
            onCompositionStart={() => {
              composingRef.current = true;
            }}
            onCompositionEnd={() => {
              composingRef.current = false;
            }}
            onPaste={(e) => {
              const img = Array.from(e.clipboardData.items)
                .filter((it) => it.type.startsWith("image/"))
                .map((it) => it.getAsFile())
                .filter((f): f is File => !!f);
              if (img.length) {
                e.preventDefault();
                void setRef(img, onToast, t);
              }
            }}
            placeholder={
              ref
                ? t("描述首帧怎么动（回车生成图生视频）")
                : t("描述要生成的视频画面，回车即生成（可拖/粘贴首帧图）")
            }
            rows={1}
            // 可拖高（测试报告 #004）。这个框没有自动长高逻辑，所以放开 resize-y 就够，
            // 不像 Draw 那边还要处理「手动拖过之后别被自动长高弹回去」。
            className="flex-1 min-w-[220px] px-3 py-2 rounded-lg bg-bg-3 border border-white/[0.08] text-[13px] text-ink-0 placeholder:text-ink-5 focus:border-accent/50 outline-none resize-y max-h-[70vh] overflow-y-auto"
          />
          {/* 视频模型：富下拉（每个带优缺点 + 价格 + 推荐），含手填兜底 */}
          <ModelMenu
            models={VIDEO_MODELS}
            value={model}
            onChange={(id) => {
              videoState.model = id;
              notify();
            }}
            title={t("视频模型（看优缺点自己选）")}
          />
          {selectedModel?.price5s != null && (
            <span className="text-[11px] text-ink-4 shrink-0">
              {t("预估约 ¥{price} / 5 秒（按 480p，实扣以服务器为准）", { price: selectedModel.price5s.toFixed(1) })}
            </span>
          )}
          <button
            onClick={onSubmit}
            disabled={busy}
            className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-50 shrink-0"
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />}
            {busy ? t("生成中") : ref ? t("图生视频") : t("生成视频")}
          </button>
        </div>
      </section>
    </div>
  );
}
