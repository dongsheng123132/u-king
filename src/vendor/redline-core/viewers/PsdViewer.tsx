import { useEffect, useState } from "react";
import type { ViewerProps } from "./types";
import type { RedlineUnit } from "../document-model";
import { useI18n } from "../../../i18n";

interface FlatLayer {
  name: string;
  canvas?: HTMLCanvasElement;
  hidden?: boolean;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function flattenLayers(children: any[] | undefined, out: FlatLayer[] = []): FlatLayer[] {
  for (const c of children ?? []) {
    if (c.children) flattenLayers(c.children, out);
    else out.push({ name: c.name || "", canvas: c.canvas, hidden: c.hidden });
  }
  return out;
}

/**
 * PSD 图层预览 —— ag-psd 纯 JS 解析，只读图层结构 + 已渲染像素（不改图层、不回写源文件）。
 * 分组图层拍平成一维列表展示，不做嵌套树（不用太深入）。
 */
export default function PsdViewer({ bytes, activeUnitId, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();
  const [layers, setLayers] = useState<FlatLayer[] | null>(null);
  const [composite, setComposite] = useState<HTMLCanvasElement | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { readPsd } = await import("ag-psd");
        const psd = readPsd(bytes);
        if (cancelled) return;
        const flat = flattenLayers(psd.children);
        setComposite(psd.canvas ?? null);
        setLayers(flat);
        const units: RedlineUnit[] = [
          { id: "composite", index: 0, label: t("合成预览") },
          ...flat.map((l, i) => ({
            id: `layer-${i}`,
            index: i + 1,
            label: `${t("图层")}：${l.name || t("未命名图层")}${l.hidden ? t("（隐藏）") : ""}`,
          })),
        ];
        onUnitsResolved(units);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bytes, onUnitsResolved, t]);

  if (err) {
    return <div className="p-4 text-red-400 text-[12px]">{t("PSD 解析失败：{err}", { err })}</div>;
  }

  const activeCanvas =
    activeUnitId === "composite" ? composite : layers?.[Number(activeUnitId?.replace("layer-", ""))]?.canvas;

  return (
    <div className="flex-1 overflow-auto p-4 flex items-start justify-center bg-black/20">
      <CanvasMount canvas={activeCanvas ?? null} />
    </div>
  );
}

/** 把 ag-psd 已经产出的原生 <canvas> 挂进 DOM（不重新拷贝像素）。 */
function CanvasMount({ canvas }: { canvas: HTMLCanvasElement | null }) {
  const { t } = useI18n();
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!host || !canvas) return;
    host.innerHTML = "";
    canvas.style.display = "block";
    canvas.style.maxWidth = "100%";
    canvas.style.maxHeight = "100%";
    host.appendChild(canvas);
  }, [host, canvas]);

  if (!canvas) {
    return <div className="text-gray-400 text-[12px] p-4">{t("该图层无像素内容（可能是调整图层/文字图层）")}</div>;
  }

  return <div ref={setHost} className="inline-block" />;
}
