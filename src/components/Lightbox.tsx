/**
 * 图片放大灯箱 —— app 内自绘，**不依赖 WebView2 原生「放大图片」右键菜单**。
 *
 * 背景：作图的图是 `data:image/png;base64,...`（几 MB）。WebView2 原生图片查看器打开
 * data URL 会一直 loading（客户实测 bug）。正解不是改 Tauri fs scope，而是自己做灯箱：
 * 全屏遮罩 + 滚轮缩放 + 拖拽平移 + 双击复位 + Esc/点背景关 + 下载。健壮、离线、零协议依赖。
 *
 * 纯前端组件，只靠 props 通信（src + 可选下载名 + onClose），符合模块解耦铁律。
 */
import { useEffect, useRef, useState } from "react";
import { Download, RotateCcw, X, ZoomIn, ZoomOut } from "lucide-react";
import { useI18n } from "../i18n";

export function Lightbox({
  src,
  downloadName,
  onDownload,
  onClose,
}: {
  src: string;
  downloadName?: string;
  /** 提供则「下载」走它（后端原生另存为，Mac 才靠谱）；不提供回退 <a download>。 */
  onDownload?: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [scale, setScale] = useState(1);
  const [tx, setTx] = useState(0);
  const [ty, setTy] = useState(0);
  const drag = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  // Esc 关；挂 window 级，保证焦点在任意子元素也能关
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const reset = () => {
    setScale(1);
    setTx(0);
    setTy(0);
  };

  const zoom = (factor: number) => setScale((s) => clamp(s * factor, 0.2, 8));

  const btn =
    "inline-flex items-center justify-center w-9 h-9 rounded-lg border border-white/[0.10] text-ink-1 hover:bg-white/[0.08] hover:text-ink-0 transition-colors";

  const onWheel = (e: React.WheelEvent) => {
    // 遮罩盖满全屏、背后无可滚内容，直接按方向缩放即可
    zoom(e.deltaY < 0 ? 1.15 : 1 / 1.15);
  };

  const onMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    drag.current = { x: e.clientX, y: e.clientY, tx, ty };
    setDragging(true);
  };

  // 拖拽平移：移动/松开挂 window，鼠标移出图片也不丢拖拽
  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      setTx(d.tx + (e.clientX - d.x));
      setTy(d.ty + (e.clientY - d.y));
    };
    const onUp = () => {
      drag.current = null;
      setDragging(false);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [dragging]);

  return (
    <div
      className="fixed inset-0 z-[100] bg-black/85 backdrop-blur-sm flex flex-col"
      onMouseDown={(e) => {
        // 只在点到背景（非图片/工具栏）时关闭
        if (e.target === e.currentTarget) onClose();
      }}
    >
      {/* 工具栏 */}
      <div className="shrink-0 flex items-center justify-end gap-2 px-4 h-12 text-ink-1">
        <span className="mr-auto text-[12px] text-ink-4 select-none">
          {t("滚轮缩放 · 拖拽移动 · 双击复位 · Esc 关闭")}
        </span>
        <button onClick={() => zoom(1 / 1.25)} title={t("缩小")} className={btn}>
          <ZoomOut size={16} />
        </button>
        <span className="text-[12px] text-ink-3 font-mono w-12 text-center select-none">
          {Math.round(scale * 100)}%
        </span>
        <button onClick={() => zoom(1.25)} title={t("放大")} className={btn}>
          <ZoomIn size={16} />
        </button>
        <button onClick={reset} title={t("复位")} className={btn}>
          <RotateCcw size={16} />
        </button>
        {onDownload ? (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onDownload();
            }}
            title={t("下载")}
            className={btn}
          >
            <Download size={16} />
          </button>
        ) : (
          <a
            href={src}
            download={downloadName || "uking-image.png"}
            title={t("下载")}
            className={btn}
            onClick={(e) => e.stopPropagation()}
          >
            <Download size={16} />
          </a>
        )}
        <button onClick={onClose} title={t("关闭")} className={btn}>
          <X size={16} />
        </button>
      </div>

      {/* 画布区 */}
      <div
        className="flex-1 min-h-0 overflow-hidden grid place-items-center"
        onWheel={onWheel}
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        onDoubleClick={reset}
      >
        <img
          src={src}
          alt={t("放大查看")}
          draggable={false}
          onMouseDown={onMouseDown}
          onDoubleClick={(e) => {
            e.stopPropagation();
            setScale((s) => (s === 1 ? 2 : 1));
            if (scale !== 1) {
              setTx(0);
              setTy(0);
            }
          }}
          style={{
            transform: `translate(${tx}px, ${ty}px) scale(${scale})`,
            cursor: dragging ? "grabbing" : "grab",
          }}
          className="max-w-[92vw] max-h-[82vh] object-contain select-none transition-transform duration-75 will-change-transform"
        />
      </div>
    </div>
  );
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}
