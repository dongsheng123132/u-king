import { useEffect, useState } from "react";
import type { ViewerProps } from "./types";
import { singleUnit } from "./util";
import { useI18n } from "../../../i18n";

/**
 * Word 只读预览 —— mammoth.js 把 docx 转成语义化 html（mammoth 自己生成标签，
 * 不透传文件里的原始标记），用 dangerouslySetInnerHTML 直接渲染风险很低，
 * 不像 sheet_to_html 那样可能把单元格原值透传进 DOM（那个用 React 转义，见 SheetViewer）。
 */
export default function DocxViewer({ bytes, onUnitsResolved, openExternal }: ViewerProps) {
  const { t } = useI18n();
  const [html, setHtml] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const mammoth = await import("mammoth/mammoth.browser");
        // mammoth 默认只映射 `heading 1..6`，**不认 `Title`** —— 于是整篇文档的大标题被降级成
        // 一个粗体段落，预览一打开就矮人一截。这三条补的是「客户第一眼看到的那行字」。
        // 追加映射不覆盖默认表（mammoth 的 styleMap 是叠加的），所以 h1~h6 照旧。
        const result = await mammoth.convertToHtml(
          { arrayBuffer: bytes },
          {
            styleMap: [
              "p[style-name='Title'] => h1.rl-title:fresh",
              "p[style-name='Subtitle'] => p.rl-subtitle:fresh",
              "p[style-name='Quote'] => blockquote:fresh",
            ],
          },
        );
        if (cancelled) return;
        setHtml(result.value);
        const text = result.value.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
        onUnitsResolved([singleUnit(t("Word 文档"), text)]);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bytes, onUnitsResolved, t]);

  if (err) {
    return (
      <div className="p-4 flex flex-col gap-2 text-[12px]">
        <div className="text-red-400">{t("Word 解析失败：{err}", { err })}</div>
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

  return (
    <div className="flex-1 overflow-auto min-h-0 bg-white text-black">
      <div
        className="max-w-[820px] mx-auto p-8 text-[13px] leading-relaxed [&_h1]:text-xl [&_h1]:font-bold [&_h2]:text-lg [&_h2]:font-bold [&_table]:border-collapse [&_td]:border [&_td]:border-gray-300 [&_td]:px-2 [&_td]:py-1"
        dangerouslySetInnerHTML={{ __html: html ?? "" }}
      />
    </div>
  );
}
