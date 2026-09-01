import { useEffect, useMemo } from "react";
import type { ViewerProps } from "./types";
import { singleUnit } from "./util";
import { useI18n } from "../../../i18n";

const MAX_BYTES = 256 * 1024;

/** 纯文本预览 —— 从现有 opencodex FilesPanel 的 text 分支迁移（含二进制拒读、256KB 截断）。 */
export default function TextViewer({ bytes, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();

  const text = useMemo(() => {
    const view = new Uint8Array(bytes);
    const slice = view.subarray(0, Math.min(view.length, MAX_BYTES));
    if (slice.includes(0)) return null; // 含 NUL 视为二进制，不预览
    let s = new TextDecoder("utf-8", { fatal: false }).decode(slice);
    if (view.length > MAX_BYTES) s += "\n\n" + t("…（文件过大，仅显示前 256KB）");
    return s;
  }, [bytes, t]);

  useEffect(() => {
    onUnitsResolved([singleUnit(t("文本"))]);
  }, [onUnitsResolved, t]);

  return (
    <div className="flex-1 overflow-auto min-h-0">
      <pre className="p-3 text-[12px] leading-relaxed font-mono whitespace-pre-wrap">
        {text ?? t("[无法预览: 二进制文件]")}
      </pre>
    </div>
  );
}
