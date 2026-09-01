/**
 * 第三方包类型缺口的兜底声明。不是我们代码的问题，是这几个包自己没把 .d.ts
 * 挂对 exports 字段（TS moduleResolution:"bundler" 严格按 exports 找类型，找不到就报错）。
 * 只在这几个具体子路径上兜底，不放宽整个包，尽量不丢类型安全。
 */

// mammoth 只有 Node 入口（"mammoth"）有类型，浏览器构建 "mammoth/mammoth.browser" 没有。
// 第二个参数是 options（`styleMap` 用来补 mammoth 默认表没有的样式，如 Word 的 `Title`）。
declare module "mammoth/mammoth.browser" {
  export function convertToHtml(
    input: { arrayBuffer: ArrayBuffer },
    options?: { styleMap?: string[]; includeDefaultStyleMap?: boolean },
  ): Promise<{ value: string; messages: unknown[] }>;
}
