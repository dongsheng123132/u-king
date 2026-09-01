/**
 * 宿主适配器 —— redline-core 对外唯一依赖的接口。
 *
 * 内核不假设自己活在 Tauri / Electron / Node 里，也不直接碰任何平台 API（文件系统、
 * 进程通信……）。这些事宿主各自实现一遍，内核只认这个接口，
 * 换宿主时（opencodex → MCP Server → 以后随便什么）viewer 代码一行都不用改。
 */
import type { ReactNode } from "react";

export interface RedlineHost {
  /**
   * 拿文件原始字节。Redline 从不自己决定"怎么访问文件系统"——
   * opencodex 里是 `fetch(convertFileSrc(path))`，MCP Server 里可以直接 `fs.readFile`。
   */
  readFileBytes(path: string): Promise<ArrayBuffer>;
  /**
   * 可选：把一段 markdown 渲染成节点。
   *
   * 内核**故意不自带 markdown 渲染器** —— 宿主基本都已经有一个（opencodex 里是
   * `src/lib/miniMd.tsx` 的 MiniMd，聊天气泡在用），再塞一个进来既涨体积又要维护两套
   * 语法口径。没注入就退化成纯文本直出源码，跟以前一样，不是错误。
   */
  renderMarkdown?: (text: string) => ReactNode;
  /** 可选：用系统默认程序打开文件——渲染失败/不支持的格式兜底用。 */
  openExternal?: (path: string) => Promise<void>;
  /**
   * 可选：给文件一个宿主原生可访问的 URL（比如 Tauri 的 convertFileSrc），
   * 图片/html 这类可以直接当 src 用，省一次「读 bytes 再拼 blob URL」的内存拷贝。
   * 没提供就退化成从 bytes 生成 blob URL，效果一样，只是稍慢。
   */
  getSrcUrl?: (path: string) => string;
  /**
   * 可选：把「我们渲染不好的办公格式」转成 PDF，返回 PDF 路径。
   *
   * 为什么要这么一条路：`.pptx` 纯前端只能抽出**文字大纲**（没有成熟的纯前端 pptx 渲染方案），
   * `.doc` 这类老二进制格式更是一个字都解不出来。宿主如果有本事（比如本机装了
   * LibreOffice）就把它转成 PDF，内核直接用已有的 PdfViewer 渲染出**真版式**。
   *
   * 约定：
   *  - 返回 `null` = **这台机器没这个本事，或这个格式不归它管** —— 不是错误，
   *    内核安静退回原来的档（大纲 / 用默认程序打开）。
   *  - 只有真出错（超时、文件损坏）才 reject。
   * 宿主没实现这个方法时，一切照旧 —— 这是纯增强，不是新依赖。
   */
  renderToPdf?: (path: string) => Promise<string | null>;
}
