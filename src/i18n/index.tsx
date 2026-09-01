/**
 * 轻量 i18n 核心 —— 「中文即 key + 英文覆盖字典 + 缺失回退中文」。
 *
 * 设计取舍（见 CLAUDE.md 开发宪法「单一真相源 / 体积优先」）：
 *  - 组件里保留中文原文，渲染时 `t("中文")` 包一层；英文放中央字典 `en.ts`（`{中文: English}`）。
 *  - 未翻译的串自动回退中文 —— 永不空白/崩，可增量翻译。
 *  - 自研 ~50 行，不引 react-i18next（省体积）。
 *  - 默认中文（中文优先产品），localStorage 记住用户选择，跨会话保留。
 *
 * 用法：组件内 `const { t, lang, setLang } = useI18n();` 然后 `{t("中文")}`。
 * 插值：`t("升级到 v{ver}", { ver: "0.9.54" })` —— 占位符 `{name}` 由 vars 替换。
 */
import { createContext, useCallback, useContext, useState, type ReactNode } from "react";
import { EN } from "./en";

export type Lang = "zh" | "en";

const STORE_KEY = "uking.lang";

/** 读持久化的语言选择；默认中文（中文优先产品）。 */
function detectDefault(): Lang {
  try {
    const saved = localStorage.getItem(STORE_KEY);
    if (saved === "zh" || saved === "en") return saved;
  } catch {
    /* localStorage 不可用时静默回退 */
  }
  return "zh";
}

/** 把 `{name}` 占位符替换成 vars 里的值；vars 缺失则原样保留占位符。 */
function interpolate(s: string, vars?: Record<string, string | number>): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m));
}

/** 查表 + 插值。**Provider 的 `t` 和非 React 的 `translate` 共用这一份**，别各写一遍。 */
function lookup(lang: Lang, zh: string, vars?: Record<string, string | number>): string {
  return interpolate(lang === "en" ? EN[zh] ?? zh : zh, vars);
}

/**
 * 非 React 环境用的翻译 —— 给**原生 DOM 建出来的东西**用（目前只有终端右键菜单：
 * 它由 `useTermGroup` 里那段命令式代码创建，够不到 context）。
 *
 * 语言取 localStorage，**跟 Provider 读的是同一份**（`setLang` 先写 localStorage 再 setState），
 * 只是不响应式 —— 菜单每次右键现建现销，够了。
 * 🔴 **组件里别用它**：组件要用 `useI18n()`，否则切语言时那块不会重渲染。
 */
export function translate(zh: string, vars?: Record<string, string | number>): string {
  return lookup(detectDefault(), zh, vars);
}

type Ctx = {
  lang: Lang;
  setLang: (l: Lang) => void;
  /** 翻译：英文态查 EN 字典（缺失回退中文），中文态直接返回原文；支持 {name} 插值。 */
  t: (zh: string, vars?: Record<string, string | number>) => string;
};

const I18nCtx = createContext<Ctx | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectDefault);

  const setLang = useCallback((l: Lang) => {
    setLangState(l);
    try {
      localStorage.setItem(STORE_KEY, l);
    } catch {
      /* ignore */
    }
    try {
      document.documentElement.lang = l === "en" ? "en" : "zh-CN";
    } catch {
      /* ignore */
    }
  }, []);

  const t = useCallback(
    (zh: string, vars?: Record<string, string | number>) => lookup(lang, zh, vars),
    [lang],
  );

  return <I18nCtx.Provider value={{ lang, setLang, t }}>{children}</I18nCtx.Provider>;
}

export function useI18n(): Ctx {
  const ctx = useContext(I18nCtx);
  if (!ctx) throw new Error("useI18n 必须在 <I18nProvider> 内使用");
  return ctx;
}
