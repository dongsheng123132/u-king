import { useEffect, useState } from "react";
import type { RedlineUnit } from "../document-model";

/** 单 unit 格式（图片/html/text/docx……）共用的 unit 构造。 */
export function singleUnit(label: string, text?: string): RedlineUnit {
  return { id: "0", index: 0, label, text };
}

/** 优先用宿主给的 srcUrl（比如 Tauri convertFileSrc，省一次拷贝），没有就从 bytes 生成 blob URL，卸载时自动释放。 */
export function useObjectUrl(bytes: ArrayBuffer, srcUrl: string | undefined, mime: string): string {
  const [url, setUrl] = useState(srcUrl ?? "");
  useEffect(() => {
    if (srcUrl) {
      setUrl(srcUrl);
      return;
    }
    const blobUrl = URL.createObjectURL(new Blob([bytes], { type: mime }));
    setUrl(blobUrl);
    return () => URL.revokeObjectURL(blobUrl);
  }, [bytes, srcUrl, mime]);
  return url;
}
