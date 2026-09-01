import { useEffect, useState } from "react";
import type { ViewerProps } from "./types";
import type { RedlineUnit } from "../document-model";
import { useI18n } from "../../../i18n";

/**
 * PPTX（以及旧版 .ppt/.doc/.xls 二进制格式）大纲提取。
 *
 * 不做逐页所见即所得渲染——纯前端没有成熟方案（调研过 LibreOffice headless/OnlyOffice
 * 都要引入服务端依赖，不符合"不用太深入"），退化成"抽文字大纲 + 用默认程序打开"兜底。
 * pptx 本质是 zip，直接解出每页 slideN.xml 里 <a:t> 文本节点拼大纲，不引入完整 OOXML 解析器。
 */
export default function OfficeOutlineViewer({
  doc,
  bytes,
  activeUnitId,
  onUnitsResolved,
  openExternal,
}: ViewerProps) {
  const { t } = useI18n();
  const [err, setErr] = useState<string | null>(null);
  const [slides, setSlides] = useState<{ label: string; text: string }[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    const isPptx = doc.sourcePath.toLowerCase().endsWith(".pptx");
    if (!isPptx) {
      // .ppt/.doc/.xls 旧二进制格式没有轻量解析路径，直接给"用默认程序打开"兜底
      onUnitsResolved([{ id: "0", index: 0, label: t("旧版二进制格式") }]);
      setSlides([]);
      return;
    }
    (async () => {
      try {
        const { unzipSync, strFromU8 } = await import("fflate");
        const files = unzipSync(new Uint8Array(bytes));
        const slideNames = Object.keys(files)
          .filter((n) => /^ppt\/slides\/slide\d+\.xml$/.test(n))
          .sort((a, b) => {
            const na = Number(a.match(/(\d+)/)?.[1] ?? 0);
            const nb = Number(b.match(/(\d+)/)?.[1] ?? 0);
            return na - nb;
          });
        if (cancelled) return;
        const list = slideNames.map((name, i) => {
          const xml = strFromU8(files[name]);
          const text = Array.from(xml.matchAll(/<a:t>([^<]*)<\/a:t>/g))
            .map((m) => m[1])
            .join(" ")
            .trim();
          return { label: t("第 {n} 页", { n: i + 1 }), text: text || t("（无文字内容）") };
        });
        setSlides(list);
        const units: RedlineUnit[] = list.map((s, i) => ({
          id: String(i),
          index: i,
          label: s.label,
          text: s.text,
        }));
        onUnitsResolved(units);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [doc, bytes, onUnitsResolved, t]);

  if (err) {
    return (
      <div className="p-4 text-red-400 text-[12px]">{t("大纲提取失败：{err}（建议\"用默认程序打开\"）", { err })}</div>
    );
  }

  if (slides && slides.length === 0) {
    return (
      <div className="p-4 flex flex-col gap-2 text-[12px]">
        <div className="text-gray-400">{t("旧版二进制格式（.ppt/.doc/.xls）暂不支持预览。")}</div>
        {openExternal && (
          <button
            onClick={() => void openExternal()}
            className="self-start px-3 py-1.5 rounded border border-current hover:bg-white/10"
          >
            {t("用默认程序打开")}
          </button>
        )}
      </div>
    );
  }

  const activeIndex = Number(activeUnitId || 0);
  const active = slides?.[activeIndex];

  return (
    <div className="flex-1 overflow-auto min-h-0 p-4">
      <div className="text-[11px] opacity-60 mb-2">
        {t("仅提取文字大纲，未逐页渲染排版；需要看真实版式请\"用默认程序打开\"。")}
      </div>
      {active && (
        <div className="whitespace-pre-wrap text-[13px] leading-relaxed">
          <div className="font-medium mb-1">{active.label}</div>
          {active.text}
        </div>
      )}
    </div>
  );
}
