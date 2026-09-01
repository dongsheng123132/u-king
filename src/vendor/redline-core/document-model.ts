/**
 * 统一文档模型 —— 格式差异在这一层被抹平。
 *
 * PDF 的页、PPTX 的大纲条目、PSD 的图层、XLSX 的 sheet、ZIP 的条目、3D 模型的场景……
 * 对外都长成同一种单元（RedlineUnit）。viewer 只认这个模型，
 * 不关心具体格式怎么解析出来的。
 */

export type RedlineFormat =
  | "image"
  | "html"
  | "text"
  | "markdown"
  | "pdf"
  | "docx"
  | "xlsx"
  | "pptx-outline"
  | "zip"
  | "psd"
  | "model3d"
  | "cad2d"
  | "unsupported";

/** 文档内一个单元（页/图层/sheet/条目/场景……）。 */
export interface RedlineUnit {
  /** 单元在文档内的稳定标识，同一文档重开后要保持一致。 */
  id: string;
  /** 展示用序号，从 0 开始。 */
  index: number;
  /** 展示用标签，例如「第 3 页」「图层：背景」「Sheet1」。 */
  label: string;
  /** 该单元解析出的文字内容。目前由 OfficeOutlineViewer 直接渲染（pptx 每页的文字大纲）。 */
  text?: string;
}

/** 一份被 Redline 打开的文档。只读——Redline 从不回写 sourcePath 指向的文件。 */
export interface RedlineDocument {
  docId: string;
  sourcePath: string;
  format: RedlineFormat;
  /** 大部分格式只有一个 unit（图片/html/text/docx/markdown）；PDF/PSD/XLSX/ZIP/3D 是多 unit。 */
  units: RedlineUnit[];
  /** 解析/渲染中途的非致命提示，比如「STEP/IGES 暂不支持」「pptx 仅提取大纲，未逐页渲染」。 */
  notes?: string[];
}
