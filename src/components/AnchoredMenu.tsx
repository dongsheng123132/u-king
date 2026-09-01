/**
 * 挂在某个按钮下面的下拉菜单 —— **用 `fixed` 定位，不受祖先 `overflow-hidden` 影响**。
 *
 * 为什么要有这么个组件：项目里的浮层原来各写各的，一半用 `absolute`。`absolute` 的菜单
 * 是父级布局的一部分，只要链路上任何一个祖先有 `overflow-hidden` 就会被**整块切掉**，
 * 而且跟 z-index 无关 —— 调多高都没用。2026-08-17 客户碰到的就是这个：产物卡片上
 * 「打开方式 / 复制」点开后菜单在 ToolBubble 那张卡（`ChatPanel.tsx` 的 rounded-card
 * overflow-hidden）里被裁没了，看起来像"点了没反应"。
 *
 * `fixed` 脱离所有祖先的裁剪与定位上下文，只认视口，所以这类 bug 从根上不会再发生。
 * 项目里 TermPanel / FilesPanel 的右键菜单一直是这么写的，从没出过事 —— 这里只是把那套
 * 写法收成一个组件，顺带补上两件它们没做的：**按钮位置自动定位**、**下方放不下就往上翻**。
 *
 * 刻意不引 floating-ui / radix / portal：守 exe 体积红线，这点定位逻辑不值一个依赖。
 */
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/** 菜单四周至少离视口边缘这么远，别贴边。 */
const MARGIN = 8;

export interface AnchoredMenuProps {
  /** 锚点元素（一般就是触发按钮）。菜单贴着它的左下角展开。 */
  anchorRef: React.RefObject<HTMLElement | null>;
  /** 关闭回调 —— 点遮罩、按 Esc、选中某一项后都该调它。 */
  onClose: () => void;
  /** 菜单项。渲染成一列按钮，点了先关菜单再执行。传了 children 时可省略。 */
  items?: { label: string; run: () => void; danger?: boolean }[];
  /**
   * 任意浮层内容（表单、输入框……）。传了就渲染它而不是 items ——
   * 定位/裁剪那套逻辑对"菜单"和"小面板"是同一件事，不值得为后者再写一份。
   */
  children?: ReactNode;
  /** 可选：菜单顶部的一行小标题（例如文件名）。 */
  header?: ReactNode;
  /** 最小宽度，默认 168px。 */
  minWidth?: number;
  /** 对菜单项文案做 i18n；不传则原样显示。 */
  t?: (s: string) => string;
}

export function AnchoredMenu({ anchorRef, onClose, items, children, header, minWidth = 168, t }: AnchoredMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  // useLayoutEffect 而不是 useEffect：要在浏览器绘制**之前**算好位置。
  // 用 useEffect 的话菜单会先在 (0,0) 闪一帧再跳到按钮下面。
  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    const menu = menuRef.current;
    if (!anchor || !menu) return;
    const a = anchor.getBoundingClientRect();
    const m = menu.getBoundingClientRect();
    // 横向：跟按钮左对齐，右边超出视口就整体往左推
    const left = Math.max(MARGIN, Math.min(a.left, window.innerWidth - m.width - MARGIN));
    // 纵向：默认在按钮下方；下面放不下就翻到上方（上面也放不下才夹到视口里）
    const below = a.bottom + 4;
    const top =
      below + m.height + MARGIN <= window.innerHeight
        ? below
        : Math.max(MARGIN, a.top - 4 - m.height);
    setPos({ left, top });
  }, [anchorRef, items?.length, children]);

  return (
    <>
      {/* 遮罩：点任意空白处关闭。z 比菜单低一层。
          data-anchored-mask 是给跑道用的**稳定非视觉标识**（宪法 14）——
          跑道原来靠 `.fixed.inset-0.z-40` 这种样式类找遮罩，换个 z 值就失联。 */}
      <div
        data-anchored-mask=""
        className="fixed inset-0 z-[60]"
        onClick={onClose}
        onContextMenu={(e) => { e.preventDefault(); onClose(); }}
      />
      {/* 🔴 **不挂 role="menu" / role="menuitem"**。那对 role 按 ARIA 规范要配方向键导航
          （↑↓ 移动、Esc 关、焦点管理），我们没实现 —— 挂一个不兑现的 role 比不挂更坏：
          读屏器会宣告成菜单、然后用户按方向键什么都不会发生。
          实际代价还立刻发生了一次：`role="menuitem"` 会**覆盖 button 的隐式 role**，
          于是全项目的 `getByRole("button", …)` 断言和跑道当场全部选不中（本组件刚落地就被
          check-produced-file-card 抓到）。保持朴素 <button>，跟终端/文件树的右键菜单一致。 */}
      <div
        ref={menuRef}
        data-anchored-menu=""
        className={
          "fixed z-[61] rounded-lg bg-bg-1 border border-white/[0.10] shadow-lg " + (children ? "" : "py-1")
        }
        // 位置算出来之前先藏着（visibility 而非 display —— 得先有尺寸才量得到高度）
        style={{ minWidth, left: pos?.left ?? 0, top: pos?.top ?? 0, visibility: pos ? "visible" : "hidden" }}
      >
        {header != null && (
          <div className="px-3 py-1 text-[10px] text-ink-5 truncate max-w-[240px]">{header}</div>
        )}
        {children}
        {items?.map((it) => (
          <button
            key={it.label}
            onClick={() => {
              onClose();
              it.run();
            }}
            className={
              "block w-full text-left px-3 py-1.5 text-[12px] hover:bg-white/[0.06] " +
              (it.danger ? "text-red-400 hover:text-red-300" : "text-ink-2 hover:text-ink-0")
            }
          >
            {t ? t(it.label) : it.label}
          </button>
        ))}
      </div>
    </>
  );
}
