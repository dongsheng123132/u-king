import { useEffect } from "react";
import type { ViewerProps } from "./types";
import { singleUnit, useObjectUrl } from "./util";
import { useI18n } from "../../../i18n";

/** HTML 预览 —— 从现有 opencodex FilesPanel 的 html 分支迁移（sandbox iframe）。 */
export default function HtmlViewer({ bytes, srcUrl, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();
  const url = useObjectUrl(bytes, srcUrl, "text/html");

  useEffect(() => {
    onUnitsResolved([singleUnit(t("网页"))]);
  }, [onUnitsResolved, t]);

  return (
    <div className="flex-1 w-full min-h-0">
      <iframe
        src={url}
        title="html-preview"
        className="w-full h-full border-0 bg-white"
        sandbox="allow-scripts allow-same-origin allow-forms"
      />
    </div>
  );
}
