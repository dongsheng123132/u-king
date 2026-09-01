import { useEffect } from "react";
import type { ViewerProps } from "./types";
import { singleUnit, useObjectUrl } from "./util";
import { useI18n } from "../../../i18n";

/** 图片预览 —— 从现有 opencodex FilesPanel 的 image 分支迁移。 */
export default function ImageViewer({ bytes, srcUrl, onUnitsResolved }: ViewerProps) {
  const { t } = useI18n();
  const url = useObjectUrl(bytes, srcUrl, "image/*");

  useEffect(() => {
    onUnitsResolved([singleUnit(t("图片"))]);
  }, [onUnitsResolved, t]);

  return (
    <div className="flex-1 overflow-auto p-4 flex items-center justify-center bg-black/20">
      <img src={url} alt="" className="block max-w-full max-h-full object-contain" />
    </div>
  );
}
