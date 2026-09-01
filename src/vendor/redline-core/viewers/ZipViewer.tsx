import { useEffect, useState } from "react";
import type { ViewerProps } from "./types";
import { singleUnit } from "./util";
import { useI18n } from "../../../i18n";

interface ZipEntry {
  name: string;
  size: number;
  isDir: boolean;
  empty: boolean;
}

const SIZE_GUARD = 50 * 1024 * 1024; // 超过 50MB 不做完整解压诊断，只提示（不用太深入）

/** ZIP 条目树 + 缺失/异常诊断 —— fflate 懒加载，全量解压后列条目，标出 0 字节的可疑项。 */
export default function ZipViewer({ bytes, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<ZipEntry[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [tooBig, setTooBig] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (bytes.byteLength > SIZE_GUARD) {
      setTooBig(true);
      onUnitsResolved([singleUnit(t("ZIP（文件过大，未展开）"))]);
      return;
    }
    (async () => {
      try {
        const { unzipSync } = await import("fflate");
        const files = unzipSync(new Uint8Array(bytes));
        if (cancelled) return;
        const list: ZipEntry[] = Object.entries(files)
          .map(([name, data]) => ({
            name,
            size: data.byteLength,
            isDir: name.endsWith("/"),
            empty: !name.endsWith("/") && data.byteLength === 0,
          }))
          .sort((a, b) => a.name.localeCompare(b.name));
        setEntries(list);
        const text = list.map((e) => `${e.name}${e.empty ? " " + t("[空文件]") : ""}`).join("\n");
        onUnitsResolved([singleUnit(t("ZIP（{n} 项）", { n: list.length }), text)]);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bytes, onUnitsResolved, t]);

  if (err) {
    return <div className="p-4 text-red-400 text-[12px]">{t("ZIP 解析失败：{err}（压缩包可能已损坏）", { err })}</div>;
  }
  if (tooBig) {
    return <div className="p-4 text-gray-400 text-[12px]">{t("文件过大（>50MB），未展开列表预览。")}</div>;
  }

  const emptyCount = entries?.filter((e) => e.empty).length ?? 0;

  return (
    <div className="flex-1 overflow-auto min-h-0 text-[12px] font-mono">
      {emptyCount > 0 && (
        <div className="px-3 py-1.5 text-red-400 bg-red-400/10 border-b border-black/10">
          {t("发现 {n} 个空文件（0 字节），可能是打包时缺失", { n: emptyCount })}
        </div>
      )}
      <div className="p-2">
        {entries?.map((e) => (
          <div
            key={e.name}
            className={"flex items-center gap-2 px-2 py-1 " + (e.empty ? "text-red-400" : "")}
          >
            <span className="flex-1 truncate">{e.name}</span>
            <span className="opacity-60 shrink-0">{e.isDir ? "" : `${e.size} B`}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
