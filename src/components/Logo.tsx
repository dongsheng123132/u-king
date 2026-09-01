/**
 * U-King 极简标记 —— 中性圆角方块 + accent「U」字。
 * 去掉黑金皇冠，走主流商务风（参考 Linear / cc-switch）。
 */
export function Logo({ size = 24 }: { size?: number }) {
  return (
    <svg viewBox="0 0 32 32" width={size} height={size} aria-label="U-King">
      <rect
        x="1"
        y="1"
        width="30"
        height="30"
        rx="7"
        fill="rgb(var(--color-bg-2))"
        stroke="rgb(var(--color-bg-4))"
        strokeWidth="1"
      />
      <path
        d="M11 10 L11 18 Q11 22 16 22 Q21 22 21 18 L21 10"
        fill="none"
        stroke="rgb(var(--color-accent))"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
