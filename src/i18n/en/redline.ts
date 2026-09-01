/** 英文覆盖字典 · redline-core 文件查看器（vendor/redline-core/*）。 */
export const redline: Record<string, string> = {
  // RedlinePanel.tsx
  "打开失败：{err}": "Failed to open: {err}",
  "加载中…": "Loading…",
  "渲染中…": "Rendering…",
  "正在渲染真实版式（借本机 LibreOffice，首次约十几秒）…":
    "Rendering the real layout via your local LibreOffice (the first run takes ~15s)…",
  "这台电脑没装 LibreOffice，只能看文字大纲。装上它（工具箱 →「厨具工具箱」）就能看到真实版式。":
    "LibreOffice isn't installed on this PC, so only a text outline is available. Install it (Toolbox → “Kitchen Toolbox”) to see the real layout.",

  // viewers/MdViewer.tsx
  Markdown: "Markdown",
  "…（文件过大，仅显示前 512KB）": "… (file too large; showing the first 512KB only)",

  // viewers/ModelViewer.tsx
  "3D 模型": "3D Model",
  "3D 模型解析失败：{err}": "Failed to parse 3D model: {err}",
  "CAD 图纸": "CAD Drawing",
  "DXF 解析失败：{err}": "Failed to parse DXF: {err}",

  // viewers/OfficeOutlineViewer.tsx
  "旧版二进制格式": "Legacy binary format",
  "（无文字内容）": "(no text content)",
  "大纲提取失败：{err}（建议\"用默认程序打开\"）":
    "Failed to extract outline: {err} (try \"Open with default app\")",
  "旧版二进制格式（.ppt/.doc/.xls）暂不支持预览。":
    "Legacy binary formats (.ppt/.doc/.xls) are not supported for preview yet.",
  "仅提取文字大纲，未逐页渲染排版；需要看真实版式请\"用默认程序打开\"。":
    "Only the text outline is extracted; the layout is not rendered page by page. To see the actual layout, use \"Open with default app\".",

  // viewers/PsdViewer.tsx
  "合成预览": "Composite preview",
  "图层": "Layer",
  "未命名图层": "Unnamed layer",
  "（隐藏）": "(hidden)",
  "PSD 解析失败：{err}": "Failed to parse PSD: {err}",
  "该图层无像素内容（可能是调整图层/文字图层）":
    "This layer has no pixel content (may be an adjustment or text layer)",

  // viewers/ZipViewer.tsx
  "ZIP（文件过大，未展开）": "ZIP (file too large, not expanded)",
  "[空文件]": "[empty file]",
  "ZIP（{n} 项）": "ZIP ({n} items)",
  "ZIP 解析失败：{err}（压缩包可能已损坏）":
    "Failed to parse ZIP: {err} (the archive may be corrupted)",
  "文件过大（>50MB），未展开列表预览。": "File too large (>50MB); list preview not expanded.",
  "发现 {n} 个空文件（0 字节），可能是打包时缺失":
    "Found {n} empty files (0 bytes), possibly missing during packaging",

  // viewers/DocxViewer.tsx
  "Word 文档": "Word Document",
  "Word 解析失败：{err}": "Failed to parse Word: {err}",

  // viewers/PdfViewer.tsx (第 {n} 页 shared with OfficeOutlineViewer)
  "第 {n} 页": "Page {n}",
  "PDF 解析失败：{err}": "Failed to parse PDF: {err}",

  // viewers/TextViewer.tsx
  "…（文件过大，仅显示前 256KB）": "…(file too large, showing first 256KB only)",
  "文本": "Text",
  "[无法预览: 二进制文件]": "[Cannot preview: binary file]",

  // viewers/UnsupportedViewer.tsx
  "不支持预览": "Unsupported preview",
  "Redline 暂不支持预览这个格式（{ext}）":
    "Redline does not support previewing this format yet ({ext})",

  // viewers/SheetViewer.tsx
  "Excel 解析失败：{err}": "Failed to parse Excel: {err}",

  // viewers/HtmlViewer.tsx
  "网页": "Web page",

  // viewers/ImageViewer.tsx
  "图片": "Image",

  // Shared: OfficeOutlineViewer / DocxViewer / UnsupportedViewer
  "用默认程序打开": "Open with default app",
};
