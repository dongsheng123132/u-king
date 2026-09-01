/**
 * AI 作图 —— 对话框形式（像聊天一样出图）。文生图调虾盘云 generations，图生图（带参考图）
 * 调 edits 端点；都用设备内置 Key 计费。
 *
 * 形态：上方对话流（你的提示词气泡 → AI 出的图气泡，从上往下排），输入框钉在底部。
 * 参考图（图生图）：可**拖入 / 粘贴 / 点选**，缩略图挂在输入框上方，有图就走 generate_image_edit。
 * 放大：点任意图开 app 内灯箱（Lightbox），不走 WebView2 原生「放大图片」（那个对 data URL 卡死）。
 * 历史：**落盘** `~/.uking/draw/`（后端 draw.rs），关 app 也不丢。出图中状态放模块级（切走再回来还在）。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDropZone } from "./lib/fileDrop";
import {
  AlertCircle,
  ChevronDown,
  Copy,
  Download,
  Image as ImageIcon,
  ImagePlus,
  Loader2,
  Package,
  Sparkles,
  Trash2,
  Wallet,
  X,
} from "lucide-react";
import type { DeviceKey, DrawRoute } from "./lib/types";
import { IMAGE_MODELS, IMAGE_SIZES, DRAW_PRESETS, DRAW_PRESET_CATS, type DrawPreset } from "./lib/models";
import { CopyKey } from "./components/CopyKey";
import { Lightbox } from "./components/Lightbox";
import { ModelMenu } from "./components/ModelMenu";
import { classifyError } from "./lib/errorKind";
import { useI18n } from "./i18n";

/** 翻译函数类型（模块级 helper 里透传，组件内用 useI18n 拿）。 */
type TFn = (zh: string, vars?: Record<string, string | number>) => string;

type ImageResult = {
  b64: string | null;
  url: string | null;
  model: string;
  revised_prompt: string | null;
  /** 原模型被安全系统拒绝后，后端自动换模型重画出来的——记原模型友好名（如 "GPT Image 2"）。 */
  fallback_from?: string | null;
};

/** 一条作图记录（后端 DrawItemOut 镜像；成功有 src，失败有 error）。 */
type DrawItem = {
  id: number;
  prompt: string;
  model: string;
  size: string;
  src?: string | null;
  revised?: string | null;
  error?: string | null;
  ts: number;
};

/** 最多附几张参考图（gpt-image 系支持多图融合；UI 上限防误拖一堆撑爆请求）。 */
const MAX_REFS = 4;

/* ---------------- 模块级 store（只存「进行中」状态 + 草稿 + 已附参考图，历史以磁盘为准） ---------------- */

const drawState: {
  busy: boolean;
  pendingPrompt: string;
  pendingRefs: string[];
  prompt: string;
  model: string;
  size: string;
  /** 画质：medium=标准省钱（默认）；high=高清（约 4× 计费）。仅文生图透传给 gpt-image。 */
  quality: string;
  refs: string[];
  /** 已就「模型读不了网页」提醒过一次 —— 再点就照发（用户可能真要把网址画进图里）。 */
  urlWarned: boolean;
  /** 作图正走客户自己的供应商（非虾盘云）。放模块级是因为出错话术在 `runGenerate` 里挑，
   *  而那是个模块级函数、拿不到组件 state；组件加载完路由后回写这一个布尔就够。 */
  customRoute: boolean;
} = {
  busy: false,
  pendingPrompt: "",
  pendingRefs: [],
  prompt: "",
  model: IMAGE_MODELS[0].id,
  size: IMAGE_SIZES[0].id,
  quality: "medium",
  refs: [],
  urlWarned: false,
  customRoute: false,
};

/** 提示词里带没带网址。 */
function hasUrl(s: string): boolean {
  return /https?:\/\/\S+/i.test(s) || /\b[a-z0-9][a-z0-9-]*\.(com|cn|net|org|io|co|shop|top|xyz|vip|cc)(\.[a-z]{2})?\b/i.test(s);
}

/**
 * 「给个网址，帮我了解这家公司再出图」——**作图模型做不到这件事**。
 *
 * 这条链路上没有任何 LLM、也没有任何抓取：输入框里的字原样就是发给扩散模型的 prompt。
 * 模型既不能拒绝也不能查证，只会按域名字面脑补一套「典型企业内容」——
 * 真实事故：家具公司的官网，出来的图画的是太阳能光伏。
 * 所以带网址 + 带检索意图时先拦一次，别让客户花钱买废图。
 */
function looksLikeWebLookup(s: string): boolean {
  if (!hasUrl(s)) return false;
  return /梳理|介绍|简介|资料|信息|了解|查一下|查查|看看这个|根据.{0,6}(网站|官网|链接)|这(家|个)公司|他们公司|公司的?业务/.test(s);
}
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((f) => f());

/** 秒数 → mm:ss / Ns（出图中显示已等待多久，让 3~10 分钟的慢图不像「卡死」）。 */
function fmtElapsed(s: number): string {
  const m = Math.floor(s / 60);
  const ss = s % 60;
  return m > 0 ? `${m}:${String(ss).padStart(2, "0")}` : `${ss}s`;
}

/**
 * 任意 WebView 能解码的图片 → RGBA PNG data URL。
 *
 * Azure Image Edit 只收 PNG/JPG，而且会拒绝 GIF/BMP/WebP、索引色 PNG、灰度/CMYK 等
 * 非 RGB/RGBA mode。旧版把 FileReader 的原字节直接上传，文件选择器却写着 image/*，因此
 * 「选得进来、上游必报 Invalid file or mode」。统一过 canvas 后，格式和色彩模式一次归一。
 */
async function blobToEditPng(blob: Blob): Promise<string> {
  const bitmap = await createImageBitmap(blob);
  try {
    if (bitmap.width < 1 || bitmap.height < 1) throw new Error("empty image");
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas unavailable");
    ctx.drawImage(bitmap, 0, 0);
    return canvas.toDataURL("image/png");
  } finally {
    bitmap.close();
  }
}

function fileToEditPng(file: File): Promise<string> {
  return blobToEditPng(file);
}

async function dataUrlToEditPng(dataUrl: string): Promise<string> {
  const response = await fetch(dataUrl);
  if (!response.ok) throw new Error("image data unavailable");
  return blobToEditPng(await response.blob());
}

/** 把一批**文件路径**（Tauri 原生拖放给的）里的图片加进参考图 —— 读盘转 data URL（后端 read_file_data_url）。
 *  文件选择/剪贴板粘贴仍走 addRefs(File)；只有「拖文件进来」走这条（dragDropEnabled=true 后拖放拿到的是路径）。*/
async function addRefsFromPaths(paths: string[], onToast: (s: string) => void, t: TFn) {
  const imgs = paths.filter((p) => /\.(png|jpe?g|webp|gif|bmp)$/i.test(p));
  if (imgs.length === 0) {
    onToast(t("只能拖入图片文件"));
    return;
  }
  const room = MAX_REFS - drawState.refs.length;
  if (room <= 0) {
    onToast(t("最多 {n} 张参考图", { n: MAX_REFS }));
    return;
  }
  const picked = imgs.slice(0, room);
  const urls = (
    await Promise.all(
      picked.map(async (p) => {
        try {
          const raw = await invoke<string>("read_file_data_url", { path: p });
          return await dataUrlToEditPng(raw);
        } catch {
          return null;
        }
      }),
    )
  ).filter((u): u is string => !!u);
  if (urls.length === 0) {
    onToast(t("读取图片失败"));
    return;
  }
  drawState.refs = [...drawState.refs, ...urls].slice(0, MAX_REFS);
  notify();
  if (imgs.length > picked.length) onToast(t("已加 {n} 张（上限 {max}）", { n: picked.length, max: MAX_REFS }));
}

/** 把一批文件里的图片加进参考图（过滤非图片、去重撑满 MAX_REFS）。 */
async function addRefs(files: FileList | File[], onToast: (s: string) => void, t: TFn) {
  const imgs = Array.from(files).filter((f) => f.type.startsWith("image/"));
  if (imgs.length === 0) {
    onToast(t("只能拖入图片文件"));
    return;
  }
  const room = MAX_REFS - drawState.refs.length;
  if (room <= 0) {
    onToast(t("最多 {n} 张参考图", { n: MAX_REFS }));
    return;
  }
  const picked = imgs.slice(0, room);
  try {
    const urls = await Promise.all(picked.map(fileToEditPng));
    drawState.refs = [...drawState.refs, ...urls].slice(0, MAX_REFS);
    notify();
    if (imgs.length > picked.length) onToast(t("已加 {n} 张（上限 {max}）", { n: picked.length, max: MAX_REFS }));
  } catch {
    onToast(t("读取图片失败：请换一张能正常打开的 PNG/JPG 图片"));
  }
}

/**
 * 这次实际该用哪个模型 —— 挂了参考图（改图模式）时，把不支持 `/v1/images/edits` 的模型
 * 换成第一个支持的。防的是这条顺序：客户先选了「通义千问图片」（只能画新图），**之后**才拖进
 * 参考图，选择就卡在一个改不了图的模型上，点发送只会收到一句看不懂的上游报错。
 * 下拉的可选项和真正发出去的请求都过这一道，两处口径一致。
 */
function effectiveModel(model: string, hasRefs: boolean): string {
  if (!hasRefs) return model;
  const m = IMAGE_MODELS.find((x) => x.id === model);
  // 手填的模型（不在列表里）一律放行 —— 手填是老手的逃生口，不替他做主。
  if (!m || m.edits !== false) return model;
  return IMAGE_MODELS.find((x) => x.edits !== false)?.id ?? model;
}

/**
 * 出图失败后，这条该给什么建议？—— 得看**是哪种失败、用的哪个模型**。
 *
 * 老版本对所有非超时失败一律回一句「建议用 GPT Image 2，Seedream/万相存境外 CDN 下不回来」。
 * 2026-07-28 这句同时踩了三个错：
 *   ① gpt-image-2 的上游被 Cloudflare 限速（1015）全量 429 时，客户**正用着它**，被劝去用它；
 *   ② 那句话把唯一还能用的国产模型劝退了；
 *   ③ 「万相存境外 CDN」本身就不对 —— 阿里系（qwen-image/万相）回的是国内乌兰察布 OSS。
 *
 * 返回空串 = 不追加建议（超时类后端话术已经给全了，再补一句只会更吵）。
 */
function failureHint(error: string, model: string, t: TFn): string {
  const e = error || "";
  // 超时：后端已给出完整可操作话术（精简描述 / 换方图 / 过会儿重试），别再叠加。
  if (/\(28\)|timed out|超时/i.test(e)) return "";
  // 「出图成功但图存在境外 CDN、下载不回来」——这一条**确实**该指路 gpt-image-2（它回 b64 不走 CDN）。
  if (/境外\s*CDN|下载失败/i.test(e)) {
    return t("这张图出出来了，但存在境外图床、你的网络下载不回来。换成「GPT Image 2」重试——它直接返回图片，不走图床。");
  }
  // 上游限流 / 服务端繁忙：**不是客户点太快**，等也没用，得换供应商。
  if (/太频繁|限流|rate.?limit|繁忙|忙不过来|\b429\b|overloaded/i.test(e)) {
    return model.startsWith("gpt-image")
      ? t("这是「GPT Image 2」的海外上游被限速了，不是你点太快，等多久都一样。点上面的模型下拉换成「Seedream 4.0」或「通义千问图片」（国产直连）就能出图。")
      : t("这个模型的上游这会儿被限速了。点上面的模型下拉换一个模型再试。");
  }
  // 其余非超时失败：给个中性的逃生口提示，别再钦定某个模型。
  return t("可以点上面的模型下拉换一个模型再试（不同模型的上游是独立的，一个挂了另一个通常还在）。");
}

/** 跑一次作图：有参考图走图生图（edits），否则文生图（generations）。完成后回调让组件 reload 历史。 */
async function runGenerate(onToast: (s: string) => void, onDone: () => void, t: TFn) {
  if (drawState.busy) return;
  const prompt = drawState.prompt.trim();
  const refs = drawState.refs.slice();
  if (!prompt) {
    onToast(refs.length ? t("请描述要怎么改这张图（例如：换成星空背景）") : t("请先输入要画的内容"));
    return;
  }
  // 拦一次「给个网址帮我了解这家公司再出图」——模型读不了网页，只会照域名编。
  // 只拦一次：再点就照发（有人确实想把网址画进图里，那是合法需求）。
  if (!drawState.urlWarned && looksLikeWebLookup(prompt)) {
    drawState.urlWarned = true;
    notify();
    onToast(t("画图模型打不开网页，看不到那个网站的任何内容——它只会照着域名编。请直接描述你要的画面（想按官网内容出图，就把公司业务、主色、要出现的文字打出来）。再点一次即按原文作画。"));
    return;
  }
  // 改图模式下把「只能画新图」的模型换掉（见 effectiveModel）。
  const { size, quality } = drawState;
  const model = effectiveModel(drawState.model, refs.length > 0);
  drawState.busy = true;
  drawState.pendingPrompt = prompt;
  drawState.pendingRefs = refs;
  drawState.prompt = "";
  drawState.refs = [];
  drawState.urlWarned = false; // 下一条提示词重新提醒
  notify();
  const isEdit = refs.length > 0;
  try {
    const r = isEdit
      ? await invoke<ImageResult>("generate_image_edit", { prompt, model, size, images: refs })
      : await invoke<ImageResult>("generate_image", { prompt, model, size, quality });
    const ok = r.b64 || r.url;
    if (ok && r.fallback_from) {
      // 后端自动换了模型重画（原模型被安全系统拒 / 原模型的上游挂了），得如实告诉客户换成了谁 ——
      // 别再写死「已自动换 Seedream」：兜底模型按失败类型分两种（见 providers.rs 两个 FALLBACK 常量），
      // 写死那句在换成通义千问时就是假话。`r.model` 是实际出图的模型，以它为准。
      onToast(t("{from} 这次没画成，已自动换 {to} 重画好了", { from: r.fallback_from, to: r.model }));
    } else {
      onToast(ok ? t("出图成功") : t("未拿到图片"));
    }
  } catch (e) {
    const msg = String(e);
    // 余额/Key/网络/服务端过载/安全审核都不是 bug —— 别污染 bug 仓库，给人话。
    // 判据下沉到 lib/errorKind.ts（原来这里和 Video.tsx 各有一份正则，漏的是同一批）。
    const { actionable, hint } = classifyError(msg);
    // 🔴 `hint` 那几句人话是**为虾盘云写的**（「去「虾盘云 · 充值」充一点」）。作图改走
    // 客户自己那家之后，钱不在虾盘云扣、Key 也不是我们发的 —— 照着念就是把人往反方向指。
    // 后端在这条路上已经按供应商翻好了话（providers.rs::friendlier_image_error），原样透出即可。
    // 不改 errorKind.ts 本身：那份规则还被视频侧共用，而视频仍是虾盘云独占（本次不动）。
    const canned = hint && !drawState.customRoute;
    onToast(actionable ? (canned ? t(hint) : msg) : t("出图失败：") + msg);
    if (!actionable) {
      invoke("report_bug", {
        kind: isEdit ? "image_edit_failed" : "draw_failed",
        summary: `${isEdit ? "图生图" : "作图"}失败 model=${model}: ${msg}`.slice(0, 200),
        // 只留诊断需要的信号，不带用户实际输入的 prompt 内容（那是用户创作内容，不是软件 bug 数据）。
        detail: `promptLen=${prompt.length}\nmodel=${model}\nsize=${size}\nrefs=${refs.length}\nerror=${msg}`,
      }).catch(() => {});
    }
  } finally {
    drawState.busy = false;
    drawState.pendingPrompt = "";
    drawState.pendingRefs = [];
    notify();
    onDone(); // 让组件重新从磁盘拉历史（拿到刚落盘的这条）
  }
}

/* ---------------- 组件 ---------------- */

export function Draw({
  deviceKey,
  onToast,
  onRecharge,
  onGoSkillPack,
}: {
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onRecharge: () => void;
  /** 跳到「AI 技能包」页 —— 把作图能力打包成可复制/导出的技能，给别的 AI 工具调用。 */
  onGoSkillPack: () => void;
}) {
  const { t } = useI18n();
  const [, force] = useState(0);
  const reRef = useRef(() => force((n) => n + 1));
  const [items, setItems] = useState<DrawItem[]>([]);
  // 拖文件进来 = 加参考图（统一走 Tauri 原生拖放，拿真实路径）；over 用于高亮遮罩
  const { ref: dropRef, over: dragOver } = useDropZone<HTMLDivElement>((paths) => void addRefsFromPaths(paths, onToast, t));
  const [lb, setLb] = useState<{ src: string; name: string; id?: number } | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const composingRef = useRef(false);
  const [presetCat, setPresetCat] = useState<string>(DRAW_PRESET_CATS[0]);
  // 场景预设条展开/收起（测试报告 #027）。默认展开——新客户靠它入门；收起过就一直收着。
  const [presetsOpen, setPresetsOpen] = useState(
    () => localStorage.getItem("uking.draw.presets") !== "0",
  );
  // 点了哪张场景预设 —— 用来在输入框上方给小白一条「下一步该干嘛」的引导条（需图 / 换占位 / 可直接生成）
  const [activePreset, setActivePreset] = useState<DrawPreset | null>(null);
  /**
   * 作图走哪家（后端 `~/.uking/draw-route.json` 的回显）。
   *
   * 🔴 客户在「AI 设置」里把作图改到自己那家之后，这一页**必须当场说出来**：
   * 底下那个模型下拉里列的是虾盘云的模型 id（gpt-image-2 / seedream-4-0 …），
   * 打到别人的端点上基本必 404 —— 一个选了必错的下拉，比没有下拉坏得多。
   * 所以自定义路由时：顶部挂 banner 说清走的谁，下面那个下拉整个收起来。
   * `null` = 还没拉到（首帧）→ 按默认渲染，别闪一下再变。
   */
  const [route, setRoute] = useState<DrawRoute | null>(null);
  const customRoute = route && !route.builtin ? route : null;

  const reloadHistory = () =>
    invoke<DrawItem[]>("list_draw_history")
      .then((list) => setItems(list))
      .catch(() => {});

  useEffect(() => {
    const f = reRef.current;
    listeners.add(f);
    void reloadHistory();
    // 每次进这一页都重读：路由是在**另一个页面**（AI 设置）改的，只在挂载时读一次
    // 就会出现「改完切回来还显示走虾盘云」——那正是客户会当成 bug 报上来的形状。
    invoke<DrawRoute>("get_draw_route")
      .then((r) => {
        setRoute(r);
        drawState.customRoute = !r.builtin;
      })
      .catch(() => {});
    return () => {
      listeners.delete(f);
    };
  }, []);

  const { busy, pendingPrompt, pendingRefs, prompt, model, size, quality, refs } = drawState;

  // 新内容出现时滚到底（对话框习惯）
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [items, busy]);

  // 出图中每秒计时：慢图（文字多的海报要 2~4 分钟）显示「已等待 1:23」，避免误以为卡死
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!busy) {
      setElapsed(0);
      return;
    }
    setElapsed(0);
    const t = window.setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => window.clearInterval(t);
  }, [busy]);

  // 输入框随内容自动长高：点「乐高小人」等长预设会填进 100+ 字提示词，rows=1 塞不下、看不全也难改。
  // 空时缩回 1 行、有长文时长高到最多 ~8 行（超过再内部滚动），既能看全长预设又不过度挤压对话区。
  //
  // 但自动只是**默认**，不是规矩（测试报告 #004：「无法放大缩小，长 prompt 时文字拥挤」）：
  // 写 300 字的分镜时 8 行还是不够看。所以放开了 `resize-y` 让人自己拖，
  // **一旦拖过，自动长高就让位** —— 否则下一次敲键盘会把用户拖出来的高度弹回去，
  // 那比不给拖更气人。
  const [manualH, setManualH] = useState<number | null>(null);
  useEffect(() => {
    const ta = taRef.current;
    if (!ta || manualH !== null) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 192) + "px";
  }, [prompt, manualH]);

  // 历史是「最近在前」，对话框要「老的在上、新的在下」→ 倒序渲染
  const ordered = [...items].reverse();

  const onSubmit = () => {
    setActivePreset(null); // 提交即收起引导条
    void runGenerate(onToast, reloadHistory, t);
  };
  const openImage = (src: string, name: string, id?: number) => setLb({ src, name, id });

  // 老手迭代①：把历史这条提示词回填输入框，改几个字或直接再点生成 → 出新一版
  const reusePrompt = (it: DrawItem) => {
    drawState.prompt = it.prompt;
    if (it.size) drawState.size = it.size;
    setActivePreset(null);
    notify();
    taRef.current?.focus();
    onToast(t("已填回提示词，改几个字或直接点生成"));
  };

  // 老手迭代②：把这张出图设为参考图（图生图），在原图基础上微调
  const editFromImage = (it: DrawItem) => {
    if (!it.src) return;
    drawState.refs = [it.src];
    drawState.prompt = "";
    setActivePreset(null);
    notify();
    taRef.current?.focus();
    onToast(t("已把这张设为参考图，描述要怎么改（如：换成星空背景、衣服改红色）"));
  };

  // 复制提示词（老手复用 / 存档）
  const copyPrompt = async (p: string) => {
    try {
      await navigator.clipboard.writeText(p);
      onToast(t("提示词已复制"));
    } catch {
      onToast(t("复制失败，请手动选中复制"));
    }
  };

  // 另存为：走后端原生「保存」对话框 + 拷磁盘文件（不依赖 <a download>，Mac 上才靠谱）
  const exportDraw = async (id: number) => {
    try {
      const p = await invoke<string | null>("export_draw", { id });
      if (p) onToast(t("已保存到：") + p);
    } catch (e) {
      onToast(t("保存失败：") + String(e));
    }
  };

  return (
    // 固定视口高度：让对话流可滚、输入框钉底（父 main 是整页滚动，h-full 拿不到高度）
    // 整块作为拖放区：拖图片进来 = 加参考图
    <div ref={dropRef} className="relative flex flex-col h-full min-h-0">
      {/* 拖放高亮遮罩 */}
      {dragOver && (
        <div className="absolute inset-0 z-20 rounded-card border-2 border-dashed border-accent/60 bg-accent/[0.06] grid place-items-center pointer-events-none">
          <div className="flex items-center gap-2 text-accent text-[14px] font-semibold">
            <ImagePlus size={18} /> {t("松手添加参考图（图生图）")}
          </div>
        </div>
      )}

      {/* 顶栏：标题 + 充值 */}
      <section className="flex flex-wrap items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-3 shrink-0">
        <ImageIcon size={18} className="text-accent shrink-0" />
        <div className="min-w-0">
          <div className="text-[15px] font-semibold text-ink-0">{t("AI 作图")}</div>
          <div className="text-[12px] text-ink-3">{t("输入文字即画 · 拖入参考图可改图 · 用内置 Key 计费")}</div>
        </div>
        <div className="ml-auto flex items-center gap-2">
          {/* 把作图能力打包给别的 AI 工具调用 —— 入口在这里（实际复制/导出在「AI 技能包」页） */}
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
                await invoke("clear_draw_history").catch(() => {});
                await reloadHistory();
                onToast(t("已清空作图记录"));
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

      {/* 作图走了别家 —— 说清打的是谁、用的哪个模型，以及去哪改回来。
          只有非默认路由才出现；绝大多数客户永远看不到这一条。 */}
      {customRoute && (
        <div className="mt-2 flex items-center gap-2 rounded-lg border border-accent/25 bg-accent/[0.06] px-3.5 py-2 text-[11.5px] text-ink-2 shrink-0">
          <Sparkles size={13} className="text-accent shrink-0" />
          <span className="min-w-0 truncate">
            {customRoute.model
              ? t("作图走 {name} · {model}（在「AI 设置 → 工具分配」里改）", {
                  name: customRoute.provider_name,
                  model: customRoute.model,
                })
              : t("作图走 {name}（在「AI 设置 → 工具分配」里改）", { name: customRoute.provider_name })}
          </span>
          <span className="ml-auto shrink-0 text-[10.5px] text-ink-5">{t("用这家自己的 Key 计费")}</span>
        </div>
      )}

      {/* 对话流（老在上、新在下；出图中显示占位气泡） */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto py-4 space-y-4">
        {ordered.length === 0 && !busy && (
          <div className="h-full grid place-items-center text-center">
            <div>
              <ImageIcon size={30} className="text-accent mx-auto mb-3 opacity-70" />
              <p className="text-[14px] text-ink-1 mb-1">{t("描述你想要的画面，回车即画")}</p>
              <p className="text-[12px] text-ink-4">
                {t("例如：一只穿宇航服的橘猫站在月球上，赛博朋克风，霓虹光")}
              </p>
              <p className="text-[12px] text-ink-5 mt-2">{t("想改一张已有的图？把它拖进来当参考图")}</p>
              <p className="text-[12px] text-accent/90 mt-3">{t("↓ 不知道画什么？点下方「场景模板」一键套用专业提示词")}</p>
            </div>
          </div>
        )}

        {ordered.map((it) => (
          <div key={it.id} className="space-y-2">
            {/* 你的提示词气泡（右对齐） */}
            <div className="flex justify-end">
              <div
                data-on-dark
                className="max-w-[78%] rounded-2xl rounded-br-sm bg-accent/90 text-white px-3.5 py-2 text-[13px] leading-snug"
              >
                {it.prompt}
                <span className="ml-2 text-[10px] text-white/70 font-mono">{it.model}</span>
              </div>
            </div>
            {/* AI 出的图气泡（左对齐） */}
            <div className="flex justify-start">
              {it.src ? (
                <div className="max-w-[78%] rounded-2xl rounded-bl-sm bg-bg-2 border border-white/[0.06] p-2 space-y-2">
                  <div className="rounded-lg overflow-hidden bg-bg-1 grid place-items-center">
                    <img
                      src={it.src}
                      alt={t("AI 出图")}
                      title={t("点击放大")}
                      onClick={() => openImage(it.src!, `uking-draw-${it.id}.png`, it.id)}
                      onContextMenu={(e) => {
                        // 屏蔽 WebView2 原生「放大图片」（对 data URL 卡死），改用 app 内灯箱
                        e.preventDefault();
                        openImage(it.src!, `uking-draw-${it.id}.png`, it.id);
                      }}
                      className="max-w-full max-h-[48vh] object-contain cursor-zoom-in"
                    />
                  </div>
                  <div className="flex items-center gap-1.5 flex-wrap">
                    <button
                      onClick={() => void exportDraw(it.id)}
                      className="inline-flex items-center gap-1.5 px-3 h-7 rounded-lg border border-white/[0.10] text-ink-1 text-[11px] hover:bg-white/[0.04]"
                    >
                      <Download size={12} /> {t("下载")}
                    </button>
                    <button
                      onClick={() => editFromImage(it)}
                      title={t("在这张的基础上改：把它设为参考图，再描述要怎么改")}
                      className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-lg border border-white/[0.10] text-ink-2 text-[11px] hover:text-accent hover:border-accent/40 hover:bg-accent/[0.06]"
                    >
                      <ImagePlus size={12} /> {t("用它改图")}
                    </button>
                    <button
                      onClick={() => reusePrompt(it)}
                      title={t("用同样的提示词再画一版（可先改几个字）")}
                      className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-lg border border-white/[0.10] text-ink-2 text-[11px] hover:text-accent hover:border-accent/40 hover:bg-accent/[0.06]"
                    >
                      <Sparkles size={12} /> {t("再画一版")}
                    </button>
                    <button
                      onClick={() => void copyPrompt(it.prompt)}
                      title={t("复制这条提示词")}
                      className="inline-flex items-center justify-center w-7 h-7 rounded-lg border border-white/[0.10] text-ink-3 hover:text-ink-1 hover:bg-white/[0.04]"
                    >
                      <Copy size={12} />
                    </button>
                    {it.revised && (
                      <span className="text-[10.5px] text-ink-4 truncate w-full">{t("改写：")}{it.revised}</span>
                    )}
                  </div>
                </div>
              ) : (
                <div className="max-w-[78%] flex items-start gap-2 rounded-2xl rounded-bl-sm bg-danger-500/[0.08] border border-danger-500/25 px-3.5 py-2.5 text-[11.5px] text-danger-400">
                  <AlertCircle size={14} className="shrink-0 mt-0.5" />
                  <div className="leading-snug">
                    {t("出图失败：")}{it.error}
                    {/* 建议按「哪种失败 + 用的哪个模型」给（failureHint），别再一律钦定 GPT Image 2 ——
                        它自己挂的时候，那句话正好把客户劝进死胡同。 */}
                    {(() => {
                      const hint = failureHint(it.error || "", it.model || "", t);
                      return hint ? <div className="text-ink-4 mt-1">{hint}</div> : null;
                    })()}
                  </div>
                </div>
              )}
            </div>
          </div>
        ))}

        {/* 出图中：提示词气泡（含参考图缩略） + 转圈气泡 */}
        {busy && (
          <div className="space-y-2">
            <div className="flex justify-end">
              <div className="max-w-[78%] rounded-2xl rounded-br-sm bg-accent/90 text-white px-3.5 py-2 text-[13px] leading-snug">
                {pendingRefs.length > 0 && (
                  <div className="flex gap-1.5 mb-1.5">
                    {pendingRefs.map((r, i) => (
                      <img
                        key={i}
                        src={r}
                        alt={t("参考图")}
                        className="w-10 h-10 rounded object-cover border border-white/30"
                      />
                    ))}
                  </div>
                )}
                {pendingPrompt}
              </div>
            </div>
            <div className="flex justify-start">
              <div className="rounded-2xl rounded-bl-sm bg-bg-2 border border-white/[0.06] px-4 py-3 flex items-start gap-2 text-[12px] text-ink-3">
                <Loader2 size={15} className="text-accent animate-spin mt-0.5 shrink-0" />
                <div className="leading-snug">
                  {pendingRefs.length > 0 ? t("正在按参考图改图…") : t("正在生成…")}
                  {elapsed > 0 && (
                    <span className="font-mono text-ink-4"> {t("已等待 {v}", { v: fmtElapsed(elapsed) })}</span>
                  )}
                  <div className="text-ink-5 mt-0.5">
                    {t("切到别的页面也不会中断")}
                    {elapsed >= 25 && " · " + t("文字多 / 要求细的图较慢，最长约 10 分钟，请耐心等")}
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 输入框钉底（回车发送，Shift+Enter 换行） */}
      <section className="shrink-0 rounded-card border border-white/[0.08] bg-bg-2/80 p-2.5 space-y-2">
        {/* 场景预设：点一下自动填**专业提示词** + 尺寸 —— 小白不会写提示词的解药（对齐 Fooocus 一键 style 库）。
            按分类切（爆款/商用/人像/实用），needRef 的是「改图」预设点了提示先拖图。改预设在 models.ts 不动后端。 */}
        <div className="space-y-1.5">
          {/* 收起开关（测试报告 #027：「工具栏固定显示，无法隐藏，占用界面空间」）。
              预设有四五十条，铺开要吃掉 2~3 行 —— 对第一次用的人是解药，对天天用的人是噪音。
              所以给个开关并**记住选择**：老用户收起一次就一直是收起的，不用每次开页面再收一遍。 */}
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => {
                const next = !presetsOpen;
                setPresetsOpen(next);
                localStorage.setItem("uking.draw.presets", next ? "1" : "0");
              }}
              className="inline-flex items-center gap-1 h-6 px-2 rounded-full text-[11px] text-ink-3 hover:text-ink-1 hover:bg-white/[0.04] transition-colors"
              title={presetsOpen ? t("收起场景预设，把空间让给对话") : t("展开场景预设")}
            >
              <ChevronDown size={12} className={presetsOpen ? "" : "-rotate-90"} />
              {t("场景预设")}
            </button>
            {!presetsOpen && (
              <span className="text-[10.5px] text-ink-5">{t("不会写提示词？点开挑一个")}</span>
            )}
          </div>
          {/* 分类标签 */}
          <div className={presetsOpen ? "flex items-center gap-1 flex-wrap" : "hidden"}>
            {DRAW_PRESET_CATS.map((c) => (
              <button
                key={c}
                onClick={() => setPresetCat(c)}
                className={
                  "h-6 px-2.5 rounded-full text-[11px] shrink-0 transition-colors border " +
                  (presetCat === c
                    ? "bg-accent/[0.14] text-accent border-accent/40 font-medium"
                    : "text-ink-3 hover:text-ink-1 border-transparent hover:bg-white/[0.04]")
                }
              >
                {t(c)}
              </button>
            ))}
          </div>
          {/* 当前分类的预设卡 */}
          <div className={presetsOpen ? "flex items-center gap-1.5 flex-wrap" : "hidden"}>
            {DRAW_PRESETS.filter((p) => p.cat === presetCat).map((p) => (
              <button
                key={p.id}
                onClick={() => {
                  drawState.prompt = p.prompt;
                  if (p.size) drawState.size = p.size;
                  setActivePreset(p); // 亮出「下一步」引导条
                  notify();
                  taRef.current?.focus();
                }}
                title={p.prompt.slice(0, 72) + "…"}
                className="inline-flex items-center gap-1 h-6 px-2 rounded-full border border-white/[0.10] text-[11px] text-ink-2 hover:text-accent hover:border-accent/40 hover:bg-accent/[0.06] shrink-0 transition-colors"
              >
                <span>{p.emoji}</span>
                <span className="whitespace-nowrap">{t(p.label)}</span>
                {p.needRef && <span className="text-[9px] text-ink-5">{t("需图")}</span>}
              </button>
            ))}
          </div>
        </div>

        {/* 选了预设后的「下一步」引导条 —— 小白点了模板不知道接着干嘛的解药 */}
        {activePreset && (
          <div className="flex items-start gap-2 rounded-lg border border-accent/25 bg-accent/[0.06] px-3 py-2 text-[11.5px] text-ink-2">
            <Sparkles size={13} className="text-accent shrink-0 mt-0.5" />
            <div className="leading-snug flex-1 min-w-0">
              {activePreset.needRef && refs.length === 0
                ? t("已选「{label}」——这是改图玩法：先点左下的 🖼 或把一张照片拖 / 粘进来，再点生成", { label: t(activePreset.label) })
                : activePreset.hint
                  ? t(activePreset.hint)
                  : activePreset.prompt.includes("「")
                    ? t("已套用「{label}」——把提示词里「」括起来的地方换成你自己的内容，再点生成", { label: t(activePreset.label) })
                    : t("已套用「{label}」——可直接点生成，或改几个字再生成", { label: t(activePreset.label) })}
            </div>
            <button
              onClick={() => setActivePreset(null)}
              title={t("知道了")}
              className="text-ink-5 hover:text-ink-2 shrink-0"
            >
              <X size={12} />
            </button>
          </div>
        )}

        {/* 已附参考图缩略条 */}
        {refs.length > 0 && (
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-[11px] text-ink-4">{t("参考图（图生图）：")}</span>
            {refs.map((r, i) => (
              <div key={i} className="relative group">
                <img
                  src={r}
                  alt={t("参考图")}
                  title={t("点击放大")}
                  onClick={() => openImage(r, `ref-${i + 1}.png`)}
                  className="w-12 h-12 rounded-lg object-cover border border-white/[0.12] cursor-zoom-in"
                />
                <button
                  onClick={() => {
                    drawState.refs = drawState.refs.filter((_, j) => j !== i);
                    notify();
                  }}
                  title={t("移除")}
                  className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-bg-1 border border-white/[0.2] grid place-items-center text-ink-2 hover:text-danger-400 hover:border-danger-500/40"
                >
                  <X size={11} />
                </button>
              </div>
            ))}
            <button
              onClick={() => {
                drawState.refs = [];
                notify();
              }}
              className="text-[11px] text-ink-5 hover:text-ink-2 ml-1"
            >
              {t("清空")}
            </button>
          </div>
        )}

        {/* 带网址就先说清楚：这条链路上没有 LLM、没有抓取，模型读不到那个站。
            不拦提交（有人就是要把网址画进图里），只是别让人以为「它会去看」。 */}
        {hasUrl(prompt) && (
          <div className="mb-2 flex items-start gap-2 rounded-lg border border-amber-400/25 bg-amber-400/[0.07] px-3 py-2 text-[12px] leading-relaxed text-warning-700 dark:text-warning-400">
            <span className="mt-px shrink-0">⚠️</span>
            <span>
              {t(
                "画图模型打不开网页，看不到这个网址里的任何内容——它只会照着域名编。想按官网内容出图，请把公司做什么、主色、要出现的文字直接写出来。（想分析网站请用「网站体检」页）",
              )}
            </span>
          </div>
        )}

        {/* flex-wrap（测试报告 #014「功能条固定死，不随窗口自适应」）：
            窗口一窄，模型选择 / 画质 / 尺寸 / 生成 会互相挤扁到点不准，
            甚至把输入框压成一条缝。允许换行 + 给输入框留最小宽度，让它整行独占。 */}
        <div className="flex items-end gap-2 flex-wrap">
          {/* 加参考图按钮（点选文件；拖入/粘贴也行） */}
          <button
            onClick={() => fileRef.current?.click()}
            title={t("添加参考图（也可直接拖入或粘贴）")}
            className="h-9 w-9 shrink-0 grid place-items-center rounded-lg border border-white/[0.10] text-ink-2 hover:text-accent hover:bg-white/[0.04]"
          >
            <ImagePlus size={16} />
          </button>
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            multiple
            className="hidden"
            onChange={(e) => {
              if (e.target.files?.length) void addRefs(e.target.files, onToast, t);
              e.target.value = ""; // 允许再次选同一文件
            }}
          />
          <textarea
            ref={taRef}
            value={prompt}
            onChange={(e) => {
              drawState.prompt = e.target.value;
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
              const imgs = Array.from(e.clipboardData.items)
                .filter((it) => it.type.startsWith("image/"))
                .map((it) => it.getAsFile())
                .filter((f): f is File => !!f);
              if (imgs.length) {
                e.preventDefault();
                void addRefs(imgs, onToast, t);
              }
            }}
            placeholder={
              refs.length > 0
                ? t("描述要怎么改这张图（回车发送）")
                : t("描述要画的画面，回车即画（Shift+Enter 换行；可拖/粘贴参考图）")
            }
            rows={1}
            // 拖过就记下来，从此这个框归你管（见上面 manualH 那段注释）
            onMouseUp={(e) => {
              const h = e.currentTarget.offsetHeight;
              if (manualH === null && Math.abs(h - e.currentTarget.scrollHeight) > 8) setManualH(h);
              else if (manualH !== null && h !== manualH) setManualH(h);
            }}
            style={manualH !== null ? { height: manualH } : undefined}
            className={
              "flex-1 min-w-[220px] px-3 py-2 rounded-lg bg-bg-3 border border-white/[0.08] text-[13px] text-ink-0 leading-snug placeholder:text-ink-5 focus:border-accent/50 outline-none resize-y overflow-y-auto " +
              (manualH === null ? "max-h-48" : "max-h-[70vh]")
            }
          />
          {/* 作图模型：富下拉（每个带优缺点 + 推荐），含手填兜底。
              **别再因为「只剩一个模型」把它藏起来** —— 2026-07-28 默认模型的上游被限速时，
              客户就是因为这个下拉不显示而完全没有逃生口（详见 lib/models.ts 顶部注释）。
              挂了参考图 = 改图模式，把不支持 edits 端点的模型滤掉，免得客户选中后收到看不懂的上游报错。 */}
          {/* 🔴 作图改走别家时**整个收起来**：IMAGE_MODELS 里是虾盘云的模型 id，
              打到客户自己的端点上基本必 404。模型改在「AI 设置」那张卡上填 ——
              一个选了必错的下拉，比没有下拉坏得多（客户会以为是模型坏了，不会想到是端点变了）。 */}
          {(() => {
            if (customRoute) return null;
            const pickable = refs.length > 0 ? IMAGE_MODELS.filter((m) => m.edits !== false) : IMAGE_MODELS;
            return pickable.length > 1 ? (
              <ModelMenu
                models={pickable}
                value={model}
                onChange={(id) => {
                  drawState.model = id;
                  notify();
                }}
                title={
                  refs.length > 0
                    ? t("改图模型（只列支持改图的；默认 GPT Image 2 最听话）")
                    : t("作图模型（看优缺点自己选；某个模型的上游挂了就换一个）")
                }
              />
            ) : null;
          })()}
          {/* 画质档：默认标准省钱，老手可切高清（约 4× 计费）。只对文生图生效——改图（有参考图）走标准，
              避免给未在真机验证过的 edits 端点透 quality 触发 400（保守，验证后再放开）。 */}
          {refs.length === 0 && (
            <div
              className="flex items-center rounded-lg border border-white/[0.10] overflow-hidden shrink-0 h-9"
              title={t("画质：标准省钱；高清更精细，但约 4 倍价钱")}
            >
              {[
                { id: "medium", label: t("标准") },
                { id: "high", label: t("高清") },
              ].map((q) => (
                <button
                  key={q.id}
                  onClick={() => {
                    drawState.quality = q.id;
                    notify();
                    if (q.id === "high")
                      onToast(t("已切高清：更精细，但约 4 倍价钱；通用配图用标准即可"));
                  }}
                  className={
                    "px-2.5 h-full text-[11px] transition-colors " +
                    (quality === q.id
                      ? "bg-accent/[0.16] text-accent font-medium"
                      : "text-ink-3 hover:text-ink-1 hover:bg-white/[0.04]")
                  }
                >
                  {q.label}
                </button>
              ))}
            </div>
          )}
          {/* 尺寸：富下拉 + 手填（不锁死——可填任意尺寸直透上游；选「自动」最稳）。复用 ModelMenu。 */}
          <ModelMenu
            models={IMAGE_SIZES}
            value={size}
            onChange={(id) => {
              drawState.size = id;
              notify();
            }}
            title={t("出图尺寸（可手填任意尺寸；拿不准就选「自动」）")}
            inputPlaceholder={t("手填尺寸，如 1024x1536，回车确认")}
          />
          <button
            onClick={onSubmit}
            disabled={busy}
            className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-50 shrink-0"
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />}
            {busy ? t("出图中") : refs.length > 0 ? t("改图") : t("生成")}
          </button>
        </div>
      </section>

      {/* 放大灯箱 */}
      {lb && (
        <Lightbox
          src={lb.src}
          downloadName={lb.name}
          onDownload={lb.id != null ? () => void exportDraw(lb.id!) : undefined}
          onClose={() => setLb(null)}
        />
      )}
    </div>
  );
}
