import { useEffect, useState } from "react";
import type { ViewerProps } from "./types";
import type { RedlineUnit } from "../document-model";
import { useI18n } from "../../../i18n";

const MAX_ROWS = 500; // 大表格截断，不做虚拟滚动（不用太深入）

/** Excel 网格预览 —— 自己拼 <table>（React 转义单元格内容，不用 sheet_to_html 那种可能透传原值到 innerHTML 的路径）。 */
export default function SheetViewer({ bytes, activeUnitId, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [workbook, setWorkbook] = useState<any>(null);
  const [err, setErr] = useState<string | null>(null);
  const [rows, setRows] = useState<unknown[][]>([]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const XLSX = await import("xlsx");
        const wb = XLSX.read(bytes, { type: "array" });
        if (cancelled) return;
        const units: RedlineUnit[] = wb.SheetNames.map((name: string, i: number) => ({
          id: name,
          index: i,
          label: name,
        }));
        onUnitsResolved(units);
        setWorkbook(wb);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bytes, onUnitsResolved]);

  useEffect(() => {
    if (!workbook || !activeUnitId) return;
    let cancelled = false;
    (async () => {
      const XLSX = await import("xlsx");
      const sheet = workbook.Sheets[activeUnitId];
      const data = sheet
        ? (XLSX.utils.sheet_to_json(sheet, { header: 1, defval: "" }) as unknown[][])
        : [];
      if (cancelled) return;
      setRows(data.slice(0, MAX_ROWS));
    })();
    return () => {
      cancelled = true;
    };
  }, [workbook, activeUnitId]);

  if (err) {
    return <div className="p-4 text-red-400 text-[12px]">{t("Excel 解析失败：{err}", { err })}</div>;
  }

  return (
    <div className="flex-1 overflow-auto min-h-0 bg-white text-black text-[12px]">
      <table className="border-collapse">
        <tbody>
          {rows.map((row, ri) => (
            <tr key={ri}>
              {row.map((cell, ci) => (
                <td key={ci} className="border border-gray-300 px-2 py-1 whitespace-nowrap">
                  {String(cell)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
