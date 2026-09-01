/**
 * 格式注册表 —— 按扩展名查格式、按格式懒加载对应 viewer。
 *
 * 所有 viewer 都走 React.lazy + 动态 import，用户真正打开某个格式时才拉对应库
 * （pdf.js/mammoth/xlsx/three.js……），不进主 bundle，守住宿主的体积红线。
 */
import { lazy, type ComponentType } from "react";
import type { RedlineFormat } from "../document-model";
import type { ViewerProps } from "./types";

const EXT_TO_FORMAT: Record<string, RedlineFormat> = {
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image",
  svg: "image", bmp: "image", ico: "image", avif: "image",
  html: "html", htm: "html",
  md: "markdown", markdown: "markdown",
  // csv 走纯文本而不是 SheetViewer：国内的 csv 常是 GBK，SheetJS 按 UTF-8 解会满屏乱码，
  // 而 TextViewer 至少能看清原文。要做真表格得先解决编码嗅探，那是另一件事。
  csv: "text",
  pdf: "pdf",
  docx: "docx", doc: "docx",
  xlsx: "xlsx", xls: "xlsx",
  pptx: "pptx-outline", ppt: "pptx-outline",
  zip: "zip",
  psd: "psd",
  stl: "model3d", obj: "model3d", gltf: "model3d", glb: "model3d",
  dxf: "cad2d",
};

/** 按文件名判断格式；未知扩展名一律先当文本试着预览（二进制由 TextViewer 自己拒读）。 */
export function detectFormat(fileName: string): RedlineFormat {
  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  return EXT_TO_FORMAT[ext] ?? "text";
}

/**
 * redline 有专门 viewer 的扩展名清单（就是上面那张表的键）。
 *
 * 宿主判断「这个产出文件值不值得挂一个预览按钮」用**这个**，别再抄一份到自己文件里：
 * 抄了的那份在 `ChatPanel.tsx` 躺了几个版本，加格式时只改一边就会漂（宪法第 8 条）。
 */
export const REDLINE_EXTS: readonly string[] = Object.keys(EXT_TO_FORMAT);

/** 未来变现分级预留位——v1 全部 free，先把口碑做出来。表结构已经能承载分级，不等于现在要收费。 */
export const FORMAT_TIERS: Record<RedlineFormat, "free" | "pro"> = {
  image: "free", html: "free", text: "free", markdown: "free", pdf: "free", docx: "free",
  xlsx: "free", "pptx-outline": "free", zip: "free", psd: "free",
  model3d: "free", cad2d: "free", unsupported: "free",
};

type ViewerLoader = () => Promise<{ default: ComponentType<ViewerProps> }>;

const LOADERS: Record<RedlineFormat, ViewerLoader> = {
  image: () => import("./ImageViewer"),
  html: () => import("./HtmlViewer"),
  text: () => import("./TextViewer"),
  markdown: () => import("./MdViewer"),
  pdf: () => import("./PdfViewer"),
  docx: () => import("./DocxViewer"),
  xlsx: () => import("./SheetViewer"),
  "pptx-outline": () => import("./OfficeOutlineViewer"),
  zip: () => import("./ZipViewer"),
  psd: () => import("./PsdViewer"),
  model3d: () => import("./ModelViewer"),
  cad2d: () => import("./ModelViewer"),
  unsupported: () => import("./UnsupportedViewer"),
};

const cache = new Map<RedlineFormat, ComponentType<ViewerProps>>();

/** 懒加载指定格式的 viewer 组件（React.lazy 包裹，用在 <Suspense> 下）。同格式只包一次。 */
export function loadViewer(format: RedlineFormat): ComponentType<ViewerProps> {
  const hit = cache.get(format);
  if (hit) return hit;
  const Comp = lazy(LOADERS[format]) as unknown as ComponentType<ViewerProps>;
  cache.set(format, Comp);
  return Comp;
}
