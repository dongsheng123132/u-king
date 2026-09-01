/**
 * 老 WebView2 兜底垫片 —— 必须在 `main.tsx` 里**第一个** import。
 *
 * ## 为什么需要（issue #291，客户 0.9.68 实锤）
 *
 * ```
 * TypeError: Promise.try is not a function
 *     at assets/pdf-B75MOh95.js
 * UA=… Chrome/120.0.0.0 … Edg/120.0.0.0
 * ```
 *
 * 我们不控制客户机上的 WebView2 运行时版本 —— 它跟着 Edge 走，客户不升级 Edge 就永远停在
 * 老版。这台是 **Chrome 120**，而 pdf.js 用了 `Promise.try`（**Chrome 128+** 才有），
 * 于是「打开文档预览」直接抛未捕获异常。
 *
 * ## 为什么是垫片而不是调构建目标
 *
 * Vite/esbuild 的 `build.target` 只降**语法**（箭头函数、可选链…），**不补内置方法**。
 * `Promise.try` / `Promise.withResolvers` 是运行时内置，降 target 一点忙帮不上，
 * 只能自己垫。而换掉 pdf.js 版本是拿一个大依赖的降级去换一个 5 行能解决的问题。
 *
 * ## 扫描口径（别只修被报上来的那一个）
 *
 * 对 `dist/assets/*.js` 全量扫过一遍现代内置：
 * - `Promise.try` 6 处（Chrome 128+）← 报上来的就是它
 * - `Promise.withResolvers` 26 处（Chrome 119+）← **没被报上来，但 Chrome ≤118 的客户必崩**
 * - `Object.groupBy` / `Map.groupBy` / `Array.fromAsync` / `toSorted` / `toReversed`：0 处
 * - `.findLast` 1 处（Chrome 97+，够老，不垫）
 *
 * 只修报上来的那个等于等着下一台老机器再崩一次 —— 报上来的是**样本**不是**全集**。
 *
 * 加新依赖后建议重扫一遍：
 * `grep -ro "Promise\.try\|Promise\.withResolvers\|Object\.groupBy" dist/assets/*.js | wc -l`
 */

type PromiseCtor = PromiseConstructor & {
  try?: <T>(fn: (...a: unknown[]) => T | PromiseLike<T>, ...args: unknown[]) => Promise<T>;
  withResolvers?: <T>() => { promise: Promise<T>; resolve: (v: T | PromiseLike<T>) => void; reject: (e?: unknown) => void };
};

const P = Promise as PromiseCtor;

// Promise.try(fn, ...args)：同步抛出的异常也要变成 rejected promise，而不是往上冒。
// 这正是调用方依赖的语义 —— 垫错了会把「同步抛错」漏成未捕获异常，比不垫还难查。
if (typeof P.try !== "function") {
  P.try = function <T>(fn: (...a: unknown[]) => T | PromiseLike<T>, ...args: unknown[]): Promise<T> {
    return new Promise<T>((resolve) => resolve(fn(...args)));
  };
}

// Promise.withResolvers()：把 resolve/reject 提到外面用。
if (typeof P.withResolvers !== "function") {
  P.withResolvers = function <T>() {
    let resolve!: (v: T | PromiseLike<T>) => void;
    let reject!: (e?: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  };
}

export {};
