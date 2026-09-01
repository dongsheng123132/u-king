import type { ReactNode } from "react";
import type { RedlineDocument, RedlineUnit } from "../document-model";

/**
 * 每个 viewer 的统一 props。viewer 只管"把 bytes 渲染成看得见的东西"——
 * 不叠任何覆盖层，也不需要上报自己的渲染像素尺寸。
 *
 * 这里原本还有一个必填的 `renderOverlay(unitId, renderSize)`，给标注层用。标注在
 * 1.0.3 整个删掉了（它产的锚点是假的：附的"该处内容"其实是全文前 200 字，跟框画在
 * 哪无关），连带这个字段一起摘掉 —— 留着它，等于让 11 个 viewer 永远替一个不存在的
 * 功能维护一套尺寸测量代码。将来若重做基于截图的标注，走的是"截 DOM 成 PNG"那条路，
 * 不需要 viewer 配合上报尺寸。
 */
export interface ViewerProps {
  doc: RedlineDocument;
  /** 文件原始字节，大部分格式解析要用。 */
  bytes: ArrayBuffer;
  /** 宿主提供的原生可访问 URL（比如 Tauri 的 convertFileSrc），能省一次内存拷贝；
   * 没有则 viewer 自己从 bytes 生成 blob URL 兜底。 */
  srcUrl?: string;
  activeUnitId: string;
  /** 多 unit 格式（PDF 页数、PSD 图层、ZIP 条目……）只有解析后才知道具体有哪些 unit，
   * 解析完必须调用一次，FilesPanel 才能画出 unit 切换 UI（页码/图层列表等）。 */
  onUnitsResolved: (units: RedlineUnit[]) => void;
  /** 把一段 markdown 渲染成节点（宿主注入，见 host-adapter 的 renderMarkdown）。没有则退回纯文本。 */
  renderMarkdown?: (text: string) => ReactNode;
  /** 用系统默认程序打开这份文档——渲染失败/不支持格式时兜底；宿主没实现就不展示按钮。 */
  openExternal?: () => Promise<void>;
}
