/**
 * AI 海报 + 真二维码合成 —— AI 画的二维码扫不出来（微信/支付宝二维码是平台颁发的，AI 画不出真的），
 * 这里让你拿一张背景海报（AI 现生成 / 上传已有的），贴上一个真二维码（上传截图 / 网址文字现生成），
 * 拖拽调整位置大小角度，合成导出一张成品图。跟「AI 作图」（Draw.tsx）分开：作图是创作，
 * 这里是「精确拼接一张必须能扫的真图」，两回事。
 *
 * 交互层用 react-konva 的 Transformer（拖拽/缩放/旋转手柄），不手写坐标数学。
 * 背景图和二维码分成两个独立 Konva.Layer（各自一块 canvas），这样二维码那层能单独关平滑
 * （imageSmoothingEnabled 是 Layer 级方法，不是 Image 节点级）——背景保留平滑更好看，
 * 二维码关平滑保清晰（硬边色块，平滑会在模块边缘产生灰阶，拉低对比度，伤扫描率）。
 * Transformer 单独放第三层，导出/扫码自检时直接分别栅格化背景层+二维码层再手动叠图，
 * 完全不碰第三层，天然不会把选中手柄画进成品图。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Konva from "konva";
import { Stage, Layer, Image as KonvaImage, Rect, Transformer } from "react-konva";
import QRCode from "qrcode";
import jsQR from "jsqr";
import ReactCrop, { centerCrop, makeAspectCrop, type Crop, type PixelCrop } from "react-image-crop";
import "react-image-crop/dist/ReactCrop.css";
import {
  AlertTriangle,
  CheckCircle2,
  Crop as CropIcon,
  Download,
  Image as ImageIcon,
  ImagePlus,
  Loader2,
  QrCode as QrCodeIcon,
  RotateCcw,
  Sparkles,
  Upload,
  Wallet,
  X,
} from "lucide-react";
import { useDropZone } from "./lib/fileDrop";
import type { DeviceKey } from "./lib/types";
import { IMAGE_MODELS, IMAGE_SIZES } from "./lib/models";
import { ModelMenu } from "./components/ModelMenu";
import { CopyKey } from "./components/CopyKey";
import { cn } from "./lib/cn";
import { useI18n } from "./i18n";

/** 翻译函数类型（模块级 helper 里透传，组件内用 useI18n 拿）。 */
type TFn = (zh: string, vars?: Record<string, string | number>) => string;

// ---------------- 类型 ----------------

type ImgAsset = { dataUrl: string; el: HTMLImageElement; naturalW: number; naturalH: number };
/** 二维码图层状态，全部是背景图的自然像素坐标（不是预览缩放坐标）——Stage 整体 scale，
 *  节点坐标不用换算，预览和导出共用一套数字。 */
type QrLayer = { cx: number; cy: number; w: number; h: number; rotation: number };
type VerifyState = "unknown" | "checking" | "ok" | "fail";
type PasteTarget = "bg" | "qr";
type DrawHistItem = { id: number; src?: string | null };
type BgPanel = "upload" | "generate" | "history" | null;
type QrPanel = "upload" | "generate" | null;

// ---------------- 常量 ----------------

const QUIET_ZONE_RATIO = 0.1; // 二维码每边额外留白 = 二维码宽度 × 该比例，纯启发式，只加不减
const MIN_SIZE_PCT = 8; // 二维码最小占背景短边的百分比
const MAX_SIZE_PCT = 90;
const DEFAULT_SIZE_PCT = 20;
const NUDGE_PCT = 1; // 方向按钮每次挪动占短边的百分比
const VERIFY_MAX_DIM = 1200; // 扫码自检前降采样到的长边上限，保证防抖后仍然跑得快

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}

// ---------------- 纯函数 helper ----------------

/** File → data URL，与 Draw.tsx 的同名 helper 一致。 */
function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const fr = new FileReader();
    fr.onload = () => resolve(String(fr.result));
    fr.onerror = () => reject(fr.error);
    fr.readAsDataURL(file);
  });
}

function loadImage(dataUrl: string, t: TFn): Promise<ImgAsset> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      if (!img.naturalWidth || !img.naturalHeight) {
        reject(new Error(t("图片解析失败，请换一张")));
        return;
      }
      resolve({ dataUrl, el: img, naturalW: img.naturalWidth, naturalH: img.naturalHeight });
    };
    img.onerror = () => reject(new Error(t("图片解析失败，请换一张")));
    img.src = dataUrl;
  });
}

function defaultLayer(bg: ImgAsset, qr: ImgAsset): QrLayer {
  const shortSide = Math.min(bg.naturalW, bg.naturalH);
  const w = shortSide * (DEFAULT_SIZE_PCT / 100);
  const h = w * (qr.naturalH / qr.naturalW);
  return { cx: bg.naturalW / 2, cy: bg.naturalH / 2, w, h, rotation: 0 };
}

const tabBtnClass = (active: boolean) =>
  cn(
    "flex-1 h-7 rounded-lg border text-[11px] transition-colors",
    active ? "border-accent bg-accent/[0.10] text-ink-0" : "border-white/[0.10] text-ink-3 hover:bg-white/[0.04]",
  );

const nudgeBtnClass =
  "h-7 rounded border border-white/[0.10] text-ink-2 text-[11px] hover:bg-white/[0.06] grid place-items-center";

function VerifyBadge({ state }: { state: VerifyState }) {
  const { t } = useI18n();
  if (state === "checking")
    return (
      <span className="inline-flex items-center gap-1.5 text-[12px] text-ink-4">
        <Loader2 size={13} className="animate-spin" /> {t("检测中…")}
      </span>
    );
  if (state === "ok")
    return (
      <span className="inline-flex items-center gap-1.5 text-[12px] text-emerald-400">
        <CheckCircle2 size={14} /> {t("已验证可扫")}
      </span>
    );
  if (state === "fail")
    return (
      <span
        title={t("本地快速自检，不代表 100% 准确；实际能不能扫建议用手机再确认一次")}
        className="inline-flex items-center gap-1.5 text-[12px] text-amber-400"
      >
        <AlertTriangle size={14} /> {t("未检测到，试试调大二维码或减少旋转角度")}
      </span>
    );
  return null;
}

// ---------------- 组件 ----------------

export function QrMerge({
  deviceKey,
  onToast,
  onRecharge,
}: {
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onRecharge: () => void;
}) {
  const { t } = useI18n();
  const [bg, setBg] = useState<ImgAsset | null>(null);
  const [qr, setQr] = useState<ImgAsset | null>(null);
  const [layer, setLayer] = useState<QrLayer | null>(null);
  const [quietZone, setQuietZone] = useState(true);
  const [verify, setVerify] = useState<VerifyState>("unknown");
  const [exporting, setExporting] = useState(false);

  const [bgPanel, setBgPanel] = useState<BgPanel>(null);
  const [bgPrompt, setBgPrompt] = useState("");
  const [bgModel, setBgModel] = useState(IMAGE_MODELS[0].id);
  const [bgSize, setBgSize] = useState(IMAGE_SIZES[0].id);
  const [bgBusy, setBgBusy] = useState(false);
  const [bgHistory, setBgHistory] = useState<DrawHistItem[] | null>(null);

  const [qrPanel, setQrPanel] = useState<QrPanel>(null);
  const [qrText, setQrText] = useState("");
  const [qrBusy, setQrBusy] = useState(false);

  // 上传的二维码先走裁剪确认步骤（去掉截图里多余的边框/文字/头像），qrCropSrc 非空时弹裁剪层
  const [qrCropSrc, setQrCropSrc] = useState<string | null>(null);
  const [crop, setCrop] = useState<Crop>();
  const [completedCrop, setCompletedCrop] = useState<PixelCrop>();
  const cropImgRef = useRef<HTMLImageElement>(null);

  const stageRef = useRef<Konva.Stage>(null);
  const bgLayerRef = useRef<Konva.Layer>(null);
  const qrLayerRef = useRef<Konva.Layer>(null);
  const qrNodeRef = useRef<Konva.Image>(null);
  const trRef = useRef<Konva.Transformer>(null);
  const bgFileRef = useRef<HTMLInputElement>(null);
  const qrFileRef = useRef<HTMLInputElement>(null);
  const pasteTargetRef = useRef<PasteTarget>("bg");

  // 二维码那层单独关平滑（Layer 级方法，只在挂载时设一次，节点本身不用管）
  useEffect(() => {
    qrLayerRef.current?.imageSmoothingEnabled(false);
  }, []);

  // 预览区实际可用尺寸——用 ResizeObserver 量出来，不猜一个固定值：全屏/窄屏/多显示器下
  // 可用空间差异很大，猜的常量装不下时 Stage 会撑破容器、盖到左边卡片上。wrapRef 常驻挂载
  // （不随 bg 有没有条件卸载），避免 effect 只跑一次时 ref 还没指到真实节点上。
  const previewWrapRef = useRef<HTMLDivElement>(null);
  const [wrapSize, setWrapSize] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = previewWrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      setWrapSize({ w: width, h: height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const previewScale = useMemo(() => {
    if (!bg || !wrapSize.w || !wrapSize.h) return 0;
    return Math.min(1, wrapSize.w / bg.naturalW, wrapSize.h / bg.naturalH);
  }, [bg, wrapSize]);

  // ---- 背景 / 二维码来源统一入口 ----

  const applyBg = async (dataUrl: string) => {
    try {
      const asset = await loadImage(dataUrl, t);
      setBg(asset);
      setBgPanel(null);
      setQr((curQr) => {
        if (curQr) setLayer(defaultLayer(asset, curQr));
        return curQr;
      });
    } catch (e) {
      onToast(String((e as Error)?.message || e));
    }
  };

  const applyQr = async (dataUrl: string) => {
    try {
      const asset = await loadImage(dataUrl, t);
      setQr(asset);
      setQrPanel(null);
      setBg((curBg) => {
        setLayer((L) => {
          if (!curBg) return L;
          if (!L) return defaultLayer(curBg, asset);
          return { ...L, h: L.w * (asset.naturalH / asset.naturalW) };
        });
        return curBg;
      });
    } catch (e) {
      onToast(String((e as Error)?.message || e));
    }
  };

  const { ref: bgDropRef, over: bgOver } = useDropZone<HTMLDivElement>((paths) => void addFromPaths(paths, "bg"));
  const { ref: qrDropRef, over: qrOver } = useDropZone<HTMLDivElement>((paths) => void addFromPaths(paths, "qr"));

  async function addFromPaths(paths: string[], target: PasteTarget) {
    const p = paths.find((x) => /\.(png|jpe?g|webp|gif|bmp)$/i.test(x));
    if (!p) {
      onToast(t("只能拖入图片文件"));
      return;
    }
    try {
      const url = await invoke<string>("read_file_data_url", { path: p });
      // 二维码截图统一先过一遍裁剪确认（去掉多余边框/文字），背景图直接用
      if (target === "bg") await applyBg(url);
      else setQrCropSrc(url);
    } catch (e) {
      onToast(t("读取图片失败：") + String(e));
    }
  }

  async function addFromFile(file: File, target: PasteTarget) {
    if (!file.type.startsWith("image/")) {
      onToast(t("只能选择图片文件"));
      return;
    }
    const url = await fileToDataUrl(file);
    if (target === "bg") await applyBg(url);
    else setQrCropSrc(url);
  }

  // ---- 二维码裁剪：默认框一个居中的正方形，交互时也锁 1:1（二维码本来就是正方形，
  //      锁死能防止手滑框出长方形、贴上去还得再拉伸变形，伤扫描率）----
  function onCropImageLoad(e: React.SyntheticEvent<HTMLImageElement>) {
    const { width, height } = e.currentTarget;
    setCrop(centerCrop(makeAspectCrop({ unit: "%", width: 90 }, 1, width, height), width, height));
  }

  async function confirmCrop() {
    if (!qrCropSrc) return;
    const img = cropImgRef.current;
    if (!img || !completedCrop || !completedCrop.width || !completedCrop.height) {
      await applyQr(qrCropSrc); // 没框选（比如截图本来就是干净的二维码）就直接用原图
    } else {
      const scaleX = img.naturalWidth / img.width;
      const scaleY = img.naturalHeight / img.height;
      const canvas = document.createElement("canvas");
      canvas.width = Math.round(completedCrop.width * scaleX);
      canvas.height = Math.round(completedCrop.height * scaleY);
      const ctx = canvas.getContext("2d");
      if (!ctx) {
        onToast(t("裁剪失败，请重试"));
        return;
      }
      ctx.drawImage(
        img,
        completedCrop.x * scaleX,
        completedCrop.y * scaleY,
        canvas.width,
        canvas.height,
        0,
        0,
        canvas.width,
        canvas.height,
      );
      await applyQr(canvas.toDataURL("image/png"));
    }
    setQrCropSrc(null);
    setCrop(undefined);
    setCompletedCrop(undefined);
  }

  function skipCrop() {
    if (!qrCropSrc) return;
    void applyQr(qrCropSrc);
    setQrCropSrc(null);
    setCrop(undefined);
    setCompletedCrop(undefined);
  }

  function cancelCrop() {
    setQrCropSrc(null);
    setCrop(undefined);
    setCompletedCrop(undefined);
  }

  // 剪贴板粘贴：鼠标悬停在哪张卡片上就粘到哪（两个来源各自独立的拖放区，没有单一 focus 目标可用）
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const f = Array.from(e.clipboardData?.items ?? [])
        .filter((it) => it.type.startsWith("image/"))
        .map((it) => it.getAsFile())
        .find((x): x is File => !!x);
      if (!f) return;
      e.preventDefault();
      void addFromFile(f, pasteTargetRef.current);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, []);

  // ---- AI 现场生成背景 ----
  async function handleGenerateBg() {
    const prompt = bgPrompt.trim();
    if (!prompt) {
      onToast(t("请先输入要画的内容"));
      return;
    }
    setBgBusy(true);
    try {
      const r = await invoke<{ b64: string | null; url: string | null }>("generate_image", {
        prompt,
        model: bgModel,
        size: bgSize,
      });
      // ⚠️ r.b64 是不带 data: 前缀的裸 base64，要自己拼；r.url 已是可直接用的完整地址
      const src = r.b64 ? `data:image/png;base64,${r.b64}` : r.url;
      if (!src) {
        onToast(t("未拿到图片"));
        return;
      }
      await applyBg(src);
      onToast(t("背景图生成成功"));
    } catch (e) {
      onToast(t("生成失败：") + String(e));
    } finally {
      setBgBusy(false);
    }
  }

  async function openHistory() {
    setBgPanel("history");
    setBgHistory(null);
    try {
      const list = await invoke<DrawHistItem[]>("list_draw_history");
      setBgHistory(list.filter((it) => it.src).slice(0, 12));
    } catch (e) {
      onToast(t("读取作图历史失败：") + String(e));
      setBgHistory([]);
    }
  }

  // ---- 生成二维码（网址/文字） ----
  async function handleGenerateQr() {
    const text = qrText.trim();
    if (!text) {
      onToast(t("请输入网址或文字"));
      return;
    }
    setQrBusy(true);
    try {
      const dataUrl = await QRCode.toDataURL(text, { margin: 2, width: 512, errorCorrectionLevel: "M" });
      await applyQr(dataUrl);
      onToast(t("二维码已生成"));
    } catch (e) {
      onToast(t("生成二维码失败：") + String(e));
    } finally {
      setQrBusy(false);
    }
  }

  // ---- Transformer 接线（官方标准写法：ref 挂节点，effect 里 nodes([...]) ）----
  useEffect(() => {
    if (trRef.current && qrNodeRef.current) {
      trRef.current.nodes([qrNodeRef.current]);
      trRef.current.getLayer()?.batchDraw();
    } else if (trRef.current) {
      trRef.current.nodes([]);
    }
  }, [qr]);

  function onQrDragEnd(e: Konva.KonvaEventObject<DragEvent>) {
    const node = e.target;
    setLayer((L) => (L ? { ...L, cx: node.x(), cy: node.y() } : L));
  }

  function onQrTransformEnd() {
    const node = qrNodeRef.current;
    if (!node || !bg) return;
    const scaleX = node.scaleX();
    const scaleY = node.scaleY();
    let w = Math.max(1, node.width() * scaleX);
    let h = Math.max(1, node.height() * scaleY);
    const shortSide = Math.min(bg.naturalW, bg.naturalH);
    const minW = shortSide * (MIN_SIZE_PCT / 100);
    const maxW = shortSide * (MAX_SIZE_PCT / 100);
    if (w < minW) {
      const r = minW / w;
      w *= r;
      h *= r;
    } else if (w > maxW) {
      const r = maxW / w;
      w *= r;
      h *= r;
    }
    node.scaleX(1);
    node.scaleY(1);
    setLayer({ cx: node.x(), cy: node.y(), w, h, rotation: node.rotation() });
  }

  // ---- 数值精调 ----
  const sizePct = bg && layer ? Math.round((layer.w / Math.min(bg.naturalW, bg.naturalH)) * 100) : DEFAULT_SIZE_PCT;

  function setSizePct(pct: number) {
    if (!bg || !qr || !layer) return;
    const shortSide = Math.min(bg.naturalW, bg.naturalH);
    const w = shortSide * (clamp(pct, MIN_SIZE_PCT, MAX_SIZE_PCT) / 100);
    const h = w * (qr.naturalH / qr.naturalW);
    setLayer({ ...layer, w, h });
  }

  function nudge(dx: number, dy: number) {
    if (!bg || !layer) return;
    const shortSide = Math.min(bg.naturalW, bg.naturalH);
    const step = shortSide * (NUDGE_PCT / 100);
    setLayer({
      ...layer,
      cx: clamp(layer.cx + dx * step, 0, bg.naturalW),
      cy: clamp(layer.cy + dy * step, 0, bg.naturalH),
    });
  }

  // ---- 合成：背景层 + 二维码层各自栅格化再手动叠图，完全不碰 Transformer 所在的第三层 ----
  function exportPixelRatio(targetMaxDim: number | null): number {
    if (!bg) return 1;
    const natMax = Math.max(bg.naturalW, bg.naturalH);
    const capRatio = targetMaxDim ? Math.min(1, targetMaxDim / natMax) : 1;
    return capRatio / previewScale;
  }

  function captureCanvas(targetMaxDim: number | null): HTMLCanvasElement | null {
    if (!bg || !previewScale || !bgLayerRef.current) return null;
    const pr = exportPixelRatio(targetMaxDim);
    const bgCanvas = bgLayerRef.current.toCanvas({ pixelRatio: pr });
    const out = document.createElement("canvas");
    out.width = bgCanvas.width;
    out.height = bgCanvas.height;
    const ctx = out.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(bgCanvas, 0, 0);
    if (qr && qrLayerRef.current) {
      const qrCanvas = qrLayerRef.current.toCanvas({ pixelRatio: pr });
      ctx.drawImage(qrCanvas, 0, 0);
    }
    return out;
  }

  // ---- 扫码自检（防抖 300ms，只提示不阻塞导出）----
  useEffect(() => {
    if (!bg || !qr || !layer) {
      setVerify("unknown");
      return;
    }
    setVerify("checking");
    const t = window.setTimeout(() => {
      try {
        const canvas = captureCanvas(VERIFY_MAX_DIM);
        const ctx = canvas?.getContext("2d");
        if (!canvas || !ctx) {
          setVerify("unknown");
          return;
        }
        const { data, width, height } = ctx.getImageData(0, 0, canvas.width, canvas.height);
        setVerify(jsQR(data, width, height) ? "ok" : "fail");
      } catch {
        setVerify("unknown");
      }
    }, 300);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bg, qr, layer, quietZone]);

  // ---- 导出 ----
  async function handleExport() {
    if (!bg || !qr) {
      onToast(t("先放好背景图和二维码"));
      return;
    }
    setExporting(true);
    try {
      const canvas = captureCanvas(null);
      if (!canvas) throw new Error(t("合成失败"));
      const pngBase64 = canvas.toDataURL("image/png");
      const filename = `uking-qrmerge-${Date.now()}.png`;
      const saved = await invoke<string | null>("export_qr_merge", { pngBase64, filename });
      if (saved) onToast(t("已保存到：") + saved);
    } catch (e) {
      onToast(t("导出失败：") + String(e));
    } finally {
      setExporting(false);
    }
  }

  const stageW = bg ? bg.naturalW * previewScale : 0;
  const stageH = bg ? bg.naturalH * previewScale : 0;
  const quietW = layer ? layer.w * (1 + 2 * QUIET_ZONE_RATIO) : 0;
  const quietH = layer ? layer.h * (1 + 2 * QUIET_ZONE_RATIO) : 0;

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* 顶栏 */}
      <section className="flex flex-wrap items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-3 shrink-0">
        <QrCodeIcon size={18} className="text-accent shrink-0" />
        <div className="min-w-0">
          <div className="text-[15px] font-semibold text-ink-0">{t("AI 海报二维码")}</div>
          <div className="text-[12px] text-ink-3">{t("AI 出图配的假二维码扫不出？拿真二维码换上去，拖一拖、合成导出")}</div>
        </div>
        <div className="ml-auto flex items-center gap-2">
          {deviceKey?.key && <CopyKey apiKey={deviceKey.key} onToast={onToast} />}
          <button
            onClick={onRecharge}
            className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-ink-1 text-[12px] hover:bg-white/[0.04]"
          >
            <Wallet size={13} /> {t("充值")}
          </button>
        </div>
      </section>

      {/* 隐藏文件选择器（点选浏览，拖放/粘贴走各自的 dropzone） */}
      <input
        ref={bgFileRef}
        type="file"
        accept="image/*"
        className="hidden"
        onChange={(e) => {
          if (e.target.files?.[0]) void addFromFile(e.target.files[0], "bg");
          e.target.value = "";
        }}
      />
      <input
        ref={qrFileRef}
        type="file"
        accept="image/*"
        className="hidden"
        onChange={(e) => {
          if (e.target.files?.[0]) void addFromFile(e.target.files[0], "qr");
          e.target.value = "";
        }}
      />

      <div className="flex-1 min-h-0 flex gap-4 mt-4">
        {/* 左侧：来源 + 调整 */}
        <aside className="w-[320px] shrink-0 overflow-y-auto space-y-4 pr-1">
          {/* 背景图卡片：整块就是拖放区，drop 目标常驻不随面板折叠卸载，避免拖放注册失效 */}
          <section
            ref={bgDropRef}
            onMouseEnter={() => (pasteTargetRef.current = "bg")}
            className={cn(
              "rounded-card border p-3.5 space-y-2.5 transition-colors",
              bgOver ? "border-accent bg-accent/[0.05]" : "border-white/[0.06] bg-bg-2",
            )}
          >
            <div className="flex items-center justify-between">
              <span className="text-[12.5px] font-semibold text-ink-1 flex items-center gap-1.5">
                <ImageIcon size={14} className="text-accent" /> {t("背景图")}
              </span>
              {bg && (
                <span className="text-[10.5px] text-ink-5 font-mono">
                  {bg.naturalW}×{bg.naturalH}
                </span>
              )}
            </div>

            {bg && (
              <div className="relative rounded-lg overflow-hidden border border-white/[0.08]">
                <img src={bg.dataUrl} className="w-full h-20 object-cover" alt={t("背景预览")} />
                <button
                  onClick={() => setBgPanel(bgPanel ? null : "upload")}
                  className="absolute bottom-1 right-1 px-2 h-6 rounded bg-black/60 text-white text-[10.5px] hover:bg-black/80"
                >
                  {t("换一张")}
                </button>
              </div>
            )}

            {(!bg || bgPanel) && (
              <div className="space-y-2">
                <div className="flex gap-1.5">
                  <button
                    onClick={() => setBgPanel(bgPanel === "upload" ? (bg ? null : "upload") : "upload")}
                    className={tabBtnClass(bgPanel === "upload" || !bgPanel)}
                  >
                    {t("上传")}
                  </button>
                  <button onClick={() => setBgPanel("generate")} className={tabBtnClass(bgPanel === "generate")}>
                    {t("AI 生成")}
                  </button>
                  <button onClick={() => void openHistory()} className={tabBtnClass(bgPanel === "history")}>
                    {t("最近作图")}
                  </button>
                </div>

                {(bgPanel === "upload" || !bgPanel) && (
                  <div
                    onClick={() => bgFileRef.current?.click()}
                    className="rounded-lg border border-dashed border-white/[0.14] hover:border-white/[0.24] px-3 py-4 text-center cursor-pointer"
                  >
                    <ImagePlus size={18} className="mx-auto text-ink-4 mb-1" />
                    <div className="text-[11.5px] text-ink-3">{t("点击选择 / 拖入 / 粘贴一张背景图")}</div>
                  </div>
                )}

                {bgPanel === "generate" && (
                  <div className="space-y-2">
                    <textarea
                      value={bgPrompt}
                      onChange={(e) => setBgPrompt(e.target.value)}
                      placeholder={t("描述海报内容，比如：简约风格的加好友海报，浅色背景，留出下方空间")}
                      rows={3}
                      className="w-full px-2.5 py-2 rounded-lg bg-bg-3 border border-white/[0.08] text-[12px] text-ink-0 placeholder:text-ink-5 outline-none resize-none focus:border-accent/50"
                    />
                    <div className="flex items-center gap-1.5">
                      <ModelMenu models={IMAGE_MODELS} value={bgModel} onChange={setBgModel} title={t("作图模型")} className="flex-1" />
                      <ModelMenu
                        models={IMAGE_SIZES}
                        value={bgSize}
                        onChange={setBgSize}
                        title={t("出图尺寸")}
                        inputPlaceholder={t("手填尺寸，回车确认")}
                        className="flex-1"
                      />
                    </div>
                    <button
                      onClick={() => void handleGenerateBg()}
                      disabled={bgBusy}
                      className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600 disabled:opacity-50"
                    >
                      {bgBusy ? <Loader2 size={13} className="animate-spin" /> : <Sparkles size={13} />}
                      {bgBusy ? t("生成中…") : t("生成背景图")}
                    </button>
                  </div>
                )}

                {bgPanel === "history" && (
                  <div className="grid grid-cols-4 gap-1.5">
                    {bgHistory === null ? (
                      <div className="col-span-4 text-[11px] text-ink-4 py-2 text-center">{t("读取中…")}</div>
                    ) : bgHistory.length === 0 ? (
                      <div className="col-span-4 text-[11px] text-ink-4 py-2 text-center">{t("还没有作图记录，先去「AI 作图」画一张")}</div>
                    ) : (
                      bgHistory.map((it) => (
                        <button
                          key={it.id}
                          onClick={() => void applyBg(it.src!)}
                          className="aspect-square rounded overflow-hidden border border-white/[0.08] hover:border-accent/50"
                        >
                          <img src={it.src!} className="w-full h-full object-cover" alt={t("历史作图")} />
                        </button>
                      ))
                    )}
                  </div>
                )}
              </div>
            )}
          </section>

          {/* 二维码卡片 */}
          <section
            ref={qrDropRef}
            onMouseEnter={() => (pasteTargetRef.current = "qr")}
            className={cn(
              "rounded-card border p-3.5 space-y-2.5 transition-colors",
              qrOver ? "border-accent bg-accent/[0.05]" : "border-white/[0.06] bg-bg-2",
            )}
          >
            <span className="text-[12.5px] font-semibold text-ink-1 flex items-center gap-1.5">
              <QrCodeIcon size={14} className="text-accent" /> {t("真二维码")}
            </span>

            {qr && (
              <div className="relative rounded-lg overflow-hidden border border-white/[0.08] bg-white grid place-items-center h-20">
                <img src={qr.dataUrl} className="max-h-full max-w-full object-contain" alt={t("二维码预览")} />
                <div className="absolute bottom-1 right-1 flex gap-1">
                  <button
                    onClick={() => setQrCropSrc(qr.dataUrl)}
                    title={t("重新裁剪")}
                    className="px-2 h-6 rounded bg-black/60 text-white text-[10.5px] hover:bg-black/80 inline-flex items-center gap-1"
                  >
                    <CropIcon size={11} /> {t("裁剪")}
                  </button>
                  <button
                    onClick={() => setQrPanel(qrPanel ? null : "upload")}
                    className="px-2 h-6 rounded bg-black/60 text-white text-[10.5px] hover:bg-black/80"
                  >
                    {t("换一个")}
                  </button>
                </div>
              </div>
            )}

            {(!qr || qrPanel) && (
              <div className="space-y-2">
                <div className="flex gap-1.5">
                  <button
                    onClick={() => setQrPanel(qrPanel === "upload" ? (qr ? null : "upload") : "upload")}
                    className={tabBtnClass(qrPanel === "upload" || !qrPanel)}
                  >
                    {t("上传截图")}
                  </button>
                  <button onClick={() => setQrPanel("generate")} className={tabBtnClass(qrPanel === "generate")}>
                    {t("网址/文字生成")}
                  </button>
                </div>

                {(qrPanel === "upload" || !qrPanel) && (
                  <>
                    <div
                      onClick={() => qrFileRef.current?.click()}
                      className="rounded-lg border border-dashed border-white/[0.14] hover:border-white/[0.24] px-3 py-4 text-center cursor-pointer"
                    >
                      <Upload size={18} className="mx-auto text-ink-4 mb-1" />
                      <div className="text-[11.5px] text-ink-3">{t("点击选择 / 拖入 / 粘贴你的二维码截图")}</div>
                    </div>
                    <div className="text-[10.5px] text-ink-5 leading-snug">
                      {t("微信/支付宝「扫我加好友」二维码是平台颁发的，不能靠文字生成，请上传截图。")}
                    </div>
                  </>
                )}

                {qrPanel === "generate" && (
                  <div className="space-y-2">
                    <input
                      value={qrText}
                      onChange={(e) => setQrText(e.target.value)}
                      placeholder={t("输入网址或文字，比如 https://example.com")}
                      className="w-full px-2.5 h-8 rounded-lg bg-bg-3 border border-white/[0.08] text-[12px] text-ink-0 placeholder:text-ink-5 outline-none focus:border-accent/50"
                    />
                    <div className="text-[10.5px] text-ink-5">{t("仅适用于普通网址/文字二维码；微信/支付宝二维码请用「上传截图」。")}</div>
                    <button
                      onClick={() => void handleGenerateQr()}
                      disabled={qrBusy}
                      className="w-full inline-flex items-center justify-center gap-1.5 h-8 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600 disabled:opacity-50"
                    >
                      {qrBusy ? <Loader2 size={13} className="animate-spin" /> : <QrCodeIcon size={13} />}
                      {t("生成二维码")}
                    </button>
                  </div>
                )}
              </div>
            )}
          </section>

          {/* 调整卡片：背景和二维码都齐了才显示 */}
          {bg && qr && layer && (
            <section className="rounded-card border border-white/[0.06] bg-bg-2 p-3.5 space-y-3">
              <span className="text-[12.5px] font-semibold text-ink-1">{t("调整")}</span>

              <div className="space-y-1">
                <div className="flex items-center justify-between text-[11px] text-ink-3">
                  <span>{t("大小")}</span>
                  <span className="font-mono">{sizePct}%</span>
                </div>
                <input
                  type="range"
                  min={MIN_SIZE_PCT}
                  max={MAX_SIZE_PCT}
                  value={sizePct}
                  onChange={(e) => setSizePct(Number(e.target.value))}
                  className="w-full accent-accent"
                />
              </div>

              <div className="space-y-1">
                <div className="flex items-center justify-between text-[11px] text-ink-3">
                  <span>{t("旋转")}</span>
                  <span className="font-mono">{Math.round(layer.rotation)}°</span>
                </div>
                <input
                  type="range"
                  min={-180}
                  max={180}
                  value={layer.rotation}
                  onChange={(e) => setLayer({ ...layer, rotation: Number(e.target.value) })}
                  className="w-full accent-accent"
                />
              </div>

              <div className="grid grid-cols-3 gap-1 w-28 mx-auto">
                <div />
                <button onClick={() => nudge(0, -1)} className={nudgeBtnClass} title={t("上移")}>
                  ↑
                </button>
                <div />
                <button onClick={() => nudge(-1, 0)} className={nudgeBtnClass} title={t("左移")}>
                  ←
                </button>
                <button onClick={() => setLayer(defaultLayer(bg, qr))} className={nudgeBtnClass} title={t("重置位置")}>
                  <RotateCcw size={12} />
                </button>
                <button onClick={() => nudge(1, 0)} className={nudgeBtnClass} title={t("右移")}>
                  →
                </button>
                <div />
                <button onClick={() => nudge(0, 1)} className={nudgeBtnClass} title={t("下移")}>
                  ↓
                </button>
                <div />
              </div>

              <label className="flex items-center gap-2 text-[11.5px] text-ink-2 cursor-pointer">
                <input type="checkbox" checked={quietZone} onChange={(e) => setQuietZone(e.target.checked)} />
                {t("自动加白边留白（推荐开启，更容易扫出）")}
              </label>
            </section>
          )}
        </aside>

        {/* 右侧：预览 + 导出。previewWrapRef 常驻挂载（不随 bg 有没有条件卸载）+ ResizeObserver
            量实际可用空间来定 Stage 缩放，不猜固定值——全屏/窄屏下可用空间差很多，猜的常量
            装不下时会撑破容器、盖到左边卡片上。 */}
        <main className="flex-1 min-w-0 min-h-0 flex flex-col items-center gap-3">
          <div ref={previewWrapRef} className="flex-1 min-h-0 min-w-0 w-full flex items-center justify-center overflow-hidden">
            {!bg ? (
              <div className="text-center">
                <ImageIcon size={30} className="text-accent mx-auto mb-3 opacity-70" />
                <p className="text-[14px] text-ink-1">{t("先在左边选一张背景图")}</p>
                <p className="text-[12px] text-ink-4 mt-1">{t("上传已有的海报，或用 AI 现场生成一张")}</p>
              </div>
            ) : (
              previewScale > 0 && (
                <div className="rounded-lg overflow-hidden border border-white/[0.10] shadow-xl" style={{ width: stageW, height: stageH }}>
                  <Stage ref={stageRef} width={stageW} height={stageH} scaleX={previewScale} scaleY={previewScale}>
                    <Layer ref={bgLayerRef}>
                      <KonvaImage image={bg.el} width={bg.naturalW} height={bg.naturalH} listening={false} />
                    </Layer>
                    <Layer ref={qrLayerRef}>
                      {qr && layer && quietZone && (
                        <Rect
                          x={layer.cx}
                          y={layer.cy}
                          offsetX={quietW / 2}
                          offsetY={quietH / 2}
                          width={quietW}
                          height={quietH}
                          rotation={layer.rotation}
                          fill="#ffffff"
                          listening={false}
                        />
                      )}
                      {qr && layer && (
                        <KonvaImage
                          ref={qrNodeRef}
                          image={qr.el}
                          x={layer.cx}
                          y={layer.cy}
                          width={layer.w}
                          height={layer.h}
                          offsetX={layer.w / 2}
                          offsetY={layer.h / 2}
                          rotation={layer.rotation}
                          draggable
                          onDragEnd={onQrDragEnd}
                          onTransformEnd={onQrTransformEnd}
                        />
                      )}
                    </Layer>
                    <Layer>
                      {qr && (
                        <Transformer
                          ref={trRef}
                          keepRatio
                          enabledAnchors={["top-left", "top-right", "bottom-left", "bottom-right"]}
                          rotateEnabled
                          anchorSize={10}
                          borderStroke="#6d5efc"
                          anchorStroke="#6d5efc"
                        />
                      )}
                    </Layer>
                  </Stage>
                </div>
              )
            )}
          </div>

          {bg && qr && (
            <div className="shrink-0 flex items-center gap-4">
              <VerifyBadge state={verify} />
              <button
                onClick={() => void handleExport()}
                disabled={exporting}
                className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-50"
              >
                {exporting ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
                {exporting ? t("导出中") : t("导出成品图")}
              </button>
            </div>
          )}
          {bg && !qr && <p className="shrink-0 text-[12px] text-ink-4">{t("再在左边放一个真二维码，就能拖到图上了")}</p>}
        </main>
      </div>

      {/* 二维码裁剪层：上传/重新裁剪时弹出，去掉截图里多余的边框/文字/头像；1:1 锁死比例 */}
      {qrCropSrc && (
        <div className="fixed inset-0 z-[100] bg-black/85 backdrop-blur-sm flex flex-col items-center justify-center gap-4 p-6">
          <div className="text-[13px] text-ink-1">{t("框选二维码所在区域，去掉多余的边框/文字/头像")}</div>
          <div className="max-w-[82vw] max-h-[70vh] overflow-auto rounded-lg bg-bg-2 p-2">
            <ReactCrop crop={crop} onChange={(_, pc) => setCrop(pc)} onComplete={(c) => setCompletedCrop(c)} aspect={1} keepSelection>
              <img ref={cropImgRef} src={qrCropSrc} alt={t("裁剪二维码")} onLoad={onCropImageLoad} className="max-w-full max-h-[65vh]" />
            </ReactCrop>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={cancelCrop}
              className="inline-flex items-center gap-1.5 px-3 h-9 rounded-lg border border-white/[0.14] text-ink-2 text-[12.5px] hover:bg-white/[0.06]"
            >
              <X size={13} /> {t("取消")}
            </button>
            <button
              onClick={skipCrop}
              className="inline-flex items-center gap-1.5 px-3 h-9 rounded-lg border border-white/[0.14] text-ink-2 text-[12.5px] hover:bg-white/[0.06]"
            >
              {t("跳过裁剪，直接用原图")}
            </button>
            <button
              onClick={() => void confirmCrop()}
              className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[12.5px] font-semibold hover:bg-accent-600"
            >
              <CropIcon size={13} /> {t("确认裁剪")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
