import { useEffect, useMemo } from "react";
import type { ViewerProps } from "./types";
import { singleUnit } from "./util";
import { useI18n } from "../../../i18n";

const MAX_BYTES = 512 * 1024;

/**
 * Markdown 预览。
 *
 * **渲染器由宿主注入**（`host.renderMarkdown` → 这里的 `renderMarkdown` prop），内核自己不带
 * 一个 —— 理由见 `host-adapter.ts`：宿主基本都已经有 markdown 渲染器（U-King 里是聊天气泡
 * 那份 MiniMd），再塞一个进来既涨体积又要维护两套语法口径。
 *
 * 宿主没注入时退回纯文本直出源码 —— 跟加这个 viewer 之前一模一样，不是错误。
 */
export default function MdViewer({ bytes, onUnitsResolved, renderMarkdown }: ViewerProps) {
  const { t } = useI18n();

  const text = useMemo(() => {
    const view = new Uint8Array(bytes);
    const slice = view.subarray(0, Math.min(view.length, MAX_BYTES));
    if (slice.includes(0)) return null; // 含 NUL 视为二进制（.md 一般不会，但别相信扩展名）
    let s = new TextDecoder("utf-8", { fatal: false }).decode(slice);
    if (view.length > MAX_BYTES) s += "\n\n" + t("…（文件过大，仅显示前 512KB）");
    return s;
  }, [bytes, t]);

  useEffect(() => {
    onUnitsResolved([singleUnit(t("Markdown"), text ?? undefined)]);
  }, [onUnitsResolved, t, text]);

  if (text == null) {
    return <div className="p-4 text-gray-400 text-[12px]">{t("[无法预览: 二进制文件]")}</div>;
  }

  return (
    <div className="flex-1 overflow-auto min-h-0 bg-white text-black">
      <div className="max-w-[820px] mx-auto p-6 text-[13px] leading-relaxed">
        {renderMarkdown ? (
          renderMarkdown(text)
        ) : (
          <pre className="text-[12px] font-mono whitespace-pre-wrap">{text}</pre>
        )}
      </div>
    </div>
  );
}
