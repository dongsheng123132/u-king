import { useEffect, useRef, useState } from "react";
import type { ViewerProps } from "./types";
import type { RedlineUnit } from "../document-model";
import { useI18n } from "../../../i18n";

/** 渲染倍率。文字层的 `--total-scale-factor` 必须跟它一致，否则字会错位。 */
const SCALE = 1.5;

/** PDF 逐页渲染。pdf.js 懒加载，worker 走 Vite `?url` 资产导入，不进主 bundle。 */
export default function PdfViewer({ bytes, activeUnitId, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [pdf, setPdf] = useState<any>(null);
  const [err, setErr] = useState<string | null>(null);
  const [canvasEl, setCanvasEl] = useState<HTMLCanvasElement | null>(null);
  // 透明文字层容器 —— canvas 只是像素，选不中也搜不到；真正能被鼠标选中的是这一层。
  const textLayerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setPdf(null);
    (async () => {
      try {
        const pdfjsLib = await import("pdfjs-dist");
        // 用标准 ESM `new URL(..., import.meta.url)` 取 worker 地址，不用 Vite 专属的 `?url`
        // 后缀语法——Vite/Rollup/大多数现代打包器都认这个写法，redline-core 不用为了这一行
        // 绑死某个打包器，纯 tsc 独立 typecheck 也能过。
        const workerUrl = new URL("pdfjs-dist/build/pdf.worker.mjs", import.meta.url).href;
        pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;
        const doc = await pdfjsLib.getDocument({ data: bytes.slice(0) }).promise;
        if (cancelled) return;
        const units: RedlineUnit[] = Array.from({ length: doc.numPages }, (_, i) => ({
          id: String(i + 1),
          index: i,
          label: t("第 {n} 页", { n: i + 1 }),
        }));
        onUnitsResolved(units);
        setPdf(doc);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bytes, onUnitsResolved, t]);

  useEffect(() => {
    let cancelled = false;
    if (!pdf || !canvasEl || !activeUnitId) return;
    (async () => {
      const page = await pdf.getPage(Number(activeUnitId));
      const viewport = page.getViewport({ scale: SCALE });
      canvasEl.width = viewport.width;
      canvasEl.height = viewport.height;
      const ctx = canvasEl.getContext("2d");
      if (!ctx) return;
      await page.render({ canvasContext: ctx, viewport }).promise;
      // 文字层：把 PDF 的字按原位铺一层**透明可选中**的 span（pdf.js 官方做法，
      // Chrome 的 PDF 阅读器也是这么干的）。没有它，客户在预览里怎么拖都选不中 ——
      // 屏幕上那些字只是 canvas 上的像素。渲染失败不挡看图，静默降级成「只能看」。
      const layer = textLayerRef.current;
      if (cancelled || !layer) return;
      try {
        const pdfjsLib = await import("pdfjs-dist");
        layer.replaceChildren();
        layer.style.width = `${viewport.width}px`;
        layer.style.height = `${viewport.height}px`;
        layer.style.setProperty("--total-scale-factor", String(SCALE));
        const tl = new pdfjsLib.TextLayer({
          textContentSource: page.streamTextContent(),
          container: layer,
          viewport,
        });
        await tl.render();
      } catch {
        /* 文字层失败 = 退回「只能看不能选」，不影响页面本身 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [pdf, canvasEl, activeUnitId]);

  if (err) {
    return <div className="p-4 text-red-400 text-[12px]">{t("PDF 解析失败：{err}", { err })}</div>;
  }

  return (
    <div className="flex-1 overflow-auto p-4 flex items-start justify-center bg-black/20">
      <div className="relative inline-block">
        <canvas ref={setCanvasEl} className="block shadow-lg" />
        {/* 文字层压在 canvas 上：字透明、可框选、可 Ctrl+C（样式见 globals.css 的 .textLayer） */}
        <div ref={textLayerRef} className="textLayer" />
      </div>
    </div>
  );
}
