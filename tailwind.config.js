/** @type {import('tailwindcss').Config} */
// 中性灰 + 单一冷调色（Linear / cc-switch 风）。去掉黑金御印，主流商务风。
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      fontFamily: {
        sans: ['"Inter"', '"Noto Sans SC"', "system-ui", "sans-serif"],
        mono: ['"JetBrains Mono"', "ui-monospace", "monospace"],
      },
      colors: {
        // 窄值域中性面：canvas=bg-0 → 抬升=bg-4
        bg: {
          0: "rgb(var(--color-bg-0) / <alpha-value>)",
          1: "rgb(var(--color-bg-1) / <alpha-value>)",
          2: "rgb(var(--color-bg-2) / <alpha-value>)",
          3: "rgb(var(--color-bg-3) / <alpha-value>)",
          4: "rgb(var(--color-bg-4) / <alpha-value>)",
        },
        ink: {
          0: "rgb(var(--color-ink-0) / <alpha-value>)",
          1: "rgb(var(--color-ink-1) / <alpha-value>)",
          2: "rgb(var(--color-ink-2) / <alpha-value>)",
          3: "rgb(var(--color-ink-3) / <alpha-value>)",
          4: "rgb(var(--color-ink-4) / <alpha-value>)",
          5: "rgb(var(--color-ink-5) / <alpha-value>)",
          6: "rgb(var(--color-ink-6) / <alpha-value>)",
        },
        // 单一冷调色（Linear 靛蓝），仅用于选中 / 焦点 / 主按钮
        accent: {
          DEFAULT: "rgb(var(--color-accent) / <alpha-value>)",
          400: "rgb(var(--color-accent-400) / <alpha-value>)",
          500: "rgb(var(--color-accent) / <alpha-value>)",
          600: "rgb(var(--color-accent-600) / <alpha-value>)",
          700: "rgb(var(--color-accent-700) / <alpha-value>)",
        },
        // 🔴 400/500 这几档是照**深色底**调的，浅色主题（默认！）下小字会糊掉。
        //    客户 2026-08-17 报「黄字跟绿的混一起都看不到」：那行 `text-amber-300/80` 压在
        //    「使用中」绿卡（bg-success-500/[0.10]）上实算 **1.24:1** —— 不是「偏淡」，是几乎不存在。
        //    所以补一档 700 专供浅色底，写法固定 `text-{danger,warning}-700 dark:…-400`。
        //    实算（脚本见 git log 本次提交）：#a51f1f 5.98:1、#8a5a0c 5.31:1，均过 WCAG AA 4.5。
        //    别再直接用裸 Tailwind 的 amber-100~300，`scripts/check-theme-tokens.mjs` 会拦。
        danger: { 400: "#f07a7a", 500: "#eb5757", 600: "#d94343", 700: "#a51f1f" },
        success: { 400: "#46c46a", 500: "#27a644", 600: "#1f8f39" },
        // 琥珀警示（仅用于「谨慎/可能误删」这类提示，如安全卸载里帮你装的工具本体）
        warning: { 400: "#e0a53a", 500: "#d1912a", 600: "#b87d20", 700: "#8a5a0c" },
      },
      boxShadow: {
        card: "var(--shadow-card)",
        pop: "var(--shadow-pop)",
      },
      keyframes: {
        "fade-in": { "0%": { opacity: 0, transform: "translateY(4px)" }, "100%": { opacity: 1, transform: "translateY(0)" } },
      },
      animation: {
        "fade-in": "fade-in 0.16s ease-out both",
      },
      borderRadius: { card: "6px", pill: "2px" },
    },
  },
  plugins: [],
};
