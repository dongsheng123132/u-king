/**
 * 输入框外壳 —— 借 WorkBuddy / Codex 桌面版那张「卡片 + 卡内底部工具条」的排版。
 *
 * **一份实现两处用**（宪法第 12 条）：轻助手（`Chat.tsx`）和 Claude/Codex（`panels/ChatPanel.tsx`）
 * 的输入框共用这一个外壳 —— 跟 `QuickPrompts` / `ComposerMenu` 同一个套路，别再各写一份。
 *
 * 为什么要改（客户原话「这个输入框是不是少了啊」）：原来两处都是「一行 textarea + 右边一个按钮」，
 * 旁边什么都没有 ——
 *  - 想带个文件进来：只能拖，或者手敲一条自己也记不准的路径（`@` 存在，但没有任何地方告诉你）
 *  - 想换模型 / 想知道 AI 能动多狠：藏在顶栏或压根没有
 *  - 一行高：写超过一句话就开始在一条缝里滚动
 * WorkBuddy 和 Codex 的答案是同一个：**把「按下发送之前该知道 / 该能改的东西」放进输入框自己**，
 * 左下角是能力（+ 附件 · 模型 · 权限），右下角是动作（清空 · 发送）。这里照抄这个结构。
 *
 * 三条不许回退的约定：
 * 1. **工具条只放真的东西**。麦克风/语音是 WorkBuddy 和 Codex 都有的，我们没有 ASR ——
 *    摆一个点了没反应的图标，比少一个图标伤得多（同 `ComposerMenu` 里「不做假技能菜单」那条）。
 * 2. **`+` 里的每一项都直接调宿主已有的 handler**，没有第二份实现。
 * 3. **权限档说的必须是这台机器上真发生的事**：轻助手是可选档（chat.rs 真按它审批），
 *    Claude Code 是写死的 bypassPermissions（`agent/claude.rs`），后者只能只读展示，不能做成下拉 ——
 *    给一个改不动的下拉 = 骗。
 */
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUp, Check, ChevronDown, ChevronRight, Cpu, FilePlus2, FolderOpen, FolderPlus, Plus, ShieldCheck, Slash as SlashIcon, Square, Users } from "lucide-react";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

/** 起手高度 ≈ 3 行（WorkBuddy 那张卡的手感），再长自增高到 MAX_H 后内部滚动。 */
const MIN_H = 68;
const MAX_H = 200;

/* ---- 卡片本体 ------------------------------------------------------------ */

export function Composer({
  value,
  onChange,
  onSend,
  onStop,
  busy,
  placeholder,
  textareaRef,
  menu,
  onKeyDown,
  onBlur,
  left,
  right,
  disabled,
  hint,
  footer,
}: {
  value: string;
  onChange: (v: string) => void;
  onSend: () => void;
  /** 忙的时候点右下角圆钮做什么。不传则忙时圆钮只是禁用（没有可中断的东西就别摆停止键）。 */
  onStop?: () => void;
  busy?: boolean;
  placeholder?: string;
  /** 宿主自己持有 ref（`@`/`/` 菜单、起手词填词、拖放插路径都要 focus 它）。不传则内部自管。 */
  textareaRef?: RefObject<HTMLTextAreaElement | null>;
  /** `@` / `/` 浮层（`useComposerMenu` 返回的 menu）。定位靠本组件最外层那个 relative。 */
  menu?: ReactNode;
  /** 返回 true = 这一下键已被菜单吃掉，别再当发送处理。 */
  onKeyDown?: (e: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  onBlur?: () => void;
  /** 卡内底部**左**槽：能力（+ 附件 / 模型 / 权限）。 */
  left?: ReactNode;
  /** 卡内底部**右**槽：动作（清空…）。发送/停止圆钮由本组件自己摆在最右。 */
  right?: ReactNode;
  /** 禁用输入（如 Claude 正在跑一轮）。 */
  disabled?: boolean;
  /** 卡片下方的一行小字（成本提醒之类）。 */
  hint?: ReactNode;
  /** 卡片**正下方**那条极轻的上下文行（工作文件夹之类）。
   *  MiniMax Code / ClawX / WorkBuddy 三家都把「工作空间」摆在这个位置 ——
   *  它是**上下文**不是**能力**：要一眼看得见，但不该跟 `+`、模型、发送抢工具条的位置。 */
  footer?: ReactNode;
}) {
  const { t } = useI18n();
  const innerRef = useRef<HTMLTextAreaElement>(null);
  const ref = textareaRef ?? innerRef;

  // 自增高：写长了自己往上撑，到 MAX_H 封顶转内部滚动。
  // 以前写死 rows=1 + max-h-32，结果超过一句话就是在一条缝里滚 —— 而这个对话框的用途
  // 恰恰是「把一大段需求说清楚」。
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(MAX_H, Math.max(MIN_H, el.scrollHeight))}px`;
  }, [value, ref]);

  const canSend = !!value.trim() && !disabled;

  return (
    <div className="relative">
      {menu}
      <div className="rounded-xl border border-white/[0.10] bg-bg-1 transition-colors focus-within:border-accent/40">
        <textarea
          ref={ref}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
          onKeyDown={(e) => {
            // 菜单开着时 Enter 归菜单（选中项），否则才是发送 —— 不然挑个文件把半截话发出去了
            if (onKeyDown?.(e)) return;
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (canSend) onSend();
            }
          }}
          rows={3}
          style={{ height: MIN_H }}
          placeholder={placeholder}
          disabled={disabled}
          className="block w-full resize-none bg-transparent px-3.5 pt-3 pb-1 text-[13px] leading-relaxed text-ink-0 placeholder:text-ink-4 outline-none disabled:opacity-60"
        />
        <div className="flex items-center gap-1.5 flex-wrap px-2 pb-2 pt-0.5">
          {left}
          <div className="ml-auto flex items-center gap-1.5">
            {right}
            {busy && onStop ? (
              <button
                onClick={onStop}
                title={t("停止这一轮")}
                className="inline-flex items-center justify-center w-8 h-8 rounded-full bg-danger-500 text-white hover:bg-danger-600"
              >
                <Square size={13} />
              </button>
            ) : (
              <button
                onClick={onSend}
                disabled={!canSend}
                title={t("发送（Enter 发送 · Shift+Enter 换行）")}
                className="inline-flex items-center justify-center w-8 h-8 rounded-full bg-accent text-white hover:bg-accent-600 disabled:opacity-40"
              >
                <ArrowUp size={16} />
              </button>
            )}
          </div>
        </div>
      </div>
      {footer}
      {hint}
    </div>
  );
}

/* ---- 工具条零件 ----------------------------------------------------------
 * 三种形态共用一套尺寸/配色，所以摆在一排不会大小不一（原来顶栏那几个 select 就是各写各的）。
 * -------------------------------------------------------------------------- */

const CHIP = "inline-flex items-center gap-1 h-7 rounded-lg border text-[11px] transition-colors";
const CHIP_IDLE = "bg-white/[0.04] border-white/[0.08] text-ink-2 hover:border-accent/30 hover:text-ink-0";

/** 下拉型 chip（模型 / 审批档 / 大脑）。用原生 select —— 键盘可达、不用自己写浮层。
 *  `tone="accent"` 给「大脑」这种主选择用；**别用 className 去覆盖底色** ——
 *  两个 arbitrary 变体（bg-white/[0.04] vs bg-accent/[0.10]）谁赢取决于生成的 CSS 顺序，不是写的顺序。 */
export function ComposerSelect({
  value,
  onChange,
  title,
  icon: Icon,
  children,
  tone = "idle",
  className,
  stretch,
}: {
  value: string;
  onChange: (v: string) => void;
  title?: string;
  icon?: typeof FolderOpen;
  children: ReactNode;
  tone?: "idle" | "accent";
  /** 外层额外类名（让宿主能把它排进等宽网格）。 */
  className?: string;
  /** 撑满外层给的宽度 —— 用于「一行几个控件长短要对齐」那种排版。
   *  不开时保持原来的内容自适应宽度（别的调用方不受影响）。 */
  stretch?: boolean;
}) {
  return (
    <span
      className={cn(
        CHIP,
        "relative pl-2 pr-1",
        tone === "accent" ? "bg-accent/[0.12] border-accent/30 text-ink-1 hover:border-accent/50" : CHIP_IDLE,
        stretch && "w-full min-w-0",
        className,
      )}
      title={title}
    >
      {Icon && <Icon size={12} className="shrink-0 text-ink-4" />}
      {/* ★ 上限从写死的 190px 改成随宽度自适应：190 截断的是「Claude Code（推荐 · 最强 · 已装）」
          这种正常长度的选项名，而它在 1920 上**一样**被截成「…已…」——那不是分辨率问题，
          是这个上限从一开始就比内容短。窄屏（≤1280）仍收在 190 防止这条 chip 把整行挤换行。 */}
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={cn(
          "cursor-pointer appearance-none truncate bg-transparent pr-4 text-[11px] text-ink-1 outline-none",
          stretch ? "w-full min-w-0" : "max-w-[190px] xl:max-w-[260px]",
        )}
      >
        {children}
      </select>
      <ChevronDown size={11} className="pointer-events-none absolute right-1.5 text-ink-4" />
    </span>
  );
}

/** 只读 chip —— 用来说**改不动但你该知道**的事实（如 Claude Code 在工作台里是全授权跑的）。 */
export function ComposerChip({
  icon: Icon,
  children,
  title,
  tone = "idle",
  onClick,
}: {
  icon?: typeof FolderOpen;
  children: ReactNode;
  title?: string;
  tone?: "idle" | "warn";
  onClick?: () => void;
}) {
  const cls = cn(
    CHIP,
    "px-2 max-w-[240px]",
    tone === "warn"
      ? "bg-warning-500/[0.12] border-warning-500/30 text-warning-400"
      : CHIP_IDLE,
    !onClick && "cursor-default",
  );
  const body = (
    <>
      {Icon && <Icon size={12} className="shrink-0" />}
      <span className="truncate">{children}</span>
    </>
  );
  return onClick ? (
    <button onClick={onClick} title={title} className={cls}>
      {body}
    </button>
  ) : (
    <span title={title} className={cls}>
      {body}
    </span>
  );
}

/* ---- `+` 附件菜单 --------------------------------------------------------- */
/*
 * 照 WorkBuddy / MiniMax Code / Claude Cowork 三家共同的那个形状（客户 2026-08-18 指名参考）：
 * **主菜单永远只有几行，重的东西挂在二级菜单上，鼠标往右一滑就展开**。
 *
 * 🔴 这解决的是我上一轮做错的那件事：我把专家/模型/权限**平铺**进 `+`，菜单越拉越长，
 * 客户原话「+ 里边东西有点多……应该是常用的、放到和鼠标位置接近的位置」。
 * 三家没有一个是靠「少放东西」解决的 —— 他们放得比我们还多（WorkBuddy 五组、
 * MiniMax 还有插件列表），靠的是**层级**：一级只列类目，二级才列内容。
 * 「尽量利用光标的移动、右侧列表来快速显示点中」——客户自己说的，就是这个。
 *
 * 展开靠 hover（鼠标扫过就开，不用点），同时保留点击 —— 触控板/键盘用户点得到。
 */

/**
 * 二级菜单的竖向落点 —— **先量再放**，不用常量。
 *
 * 🔴 这一处已经用两个方向的常量各错过一次，方向相反、症状对称：
 *  - `bottom-0`（从这一行的底边往上长）：行离顶只有一百多像素时，320px 的列表整片冲出顶栏；
 *  - `top-0`（从这一行的顶边往下长）：行离底只有一百多像素时，列表下半截被窗口切掉
 *    （客户实拍：专家列表「往下就盖住了」）。
 * 两个常量都只对半个屏幕成立，而 `+` 菜单本身是**贴着输入框往上弹**的 —— 它的行在视口里的
 * 高度随窗口大小、随菜单里有几项而变，没有哪个方向是恒对的。所以别再挑方向了：
 * 量这一行在视口里的位置 + 菜单自己的实际高度，夹回视口内 —— 下面够就往下、不够就往上顶。
 *
 * 用 ResizeObserver 而不是只在挂载时量一次：「更多（海外旗舰）」展开会让高度当场变一倍。
 */
function SubMenuBox({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<CSSProperties>({});

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const place = () => {
      /* offsetParent 就是那一行（`relative` 的容器）—— 二级菜单是它的绝对定位子元素。 */
      const row = (el.offsetParent ?? el.parentElement) as HTMLElement | null;
      if (!row) return;
      const anchor = row.getBoundingClientRect();
      const vh = window.innerHeight;
      const max = Math.min(260, vh - 16);
      const h = Math.min(el.scrollHeight, max);
      /* 想跟这一行顶部对齐（鼠标平移过去位移最短），放不下再往上挪，最多挪到离顶 8px。 */
      const top = Math.min(Math.max(8, anchor.top), Math.max(8, vh - h - 8));
      setStyle({ top: Math.round(top - anchor.top), maxHeight: max });
    };
    place();
    const ro = new ResizeObserver(place);
    ro.observe(el);
    window.addEventListener("resize", place);
    return () => { ro.disconnect(); window.removeEventListener("resize", place); };
  }, []);

  return (
    <div
      ref={ref}
      style={style}
      className="absolute left-full ml-0.5 w-56 overflow-y-auto rounded-lg border border-white/[0.10] bg-bg-1 py-1 shadow-pop"
    >
      {children}
    </div>
  );
}

type SubItem = { id: string; label: string; hint?: string; active?: boolean };
/** 二级列表里可以放一段「默认折叠」的尾巴 —— 用来把**贵的海外模型**藏在「更多」后面。 */
type SubMore = { label: string; items: SubItem[] };
type Group =
  | { kind: "action"; icon: typeof FilePlus2; label: string; hint?: string; run: () => void }
  | { kind: "sub"; icon: typeof FilePlus2; label: string; value?: string; items: SubItem[]; more?: SubMore; onPick: (id: string) => void };

export function AttachButton({
  onInsert,
  onMention,
  onSlash,
  hasWorkspace,
  approval,
  experts,
  model,
}: {
  /** 选中的真实路径（可多选）。宿主负责插进输入框 —— 和拖放共用同一个 handler。 */
  onInsert: (paths: string[]) => void;
  /** 「引用工作区里的文件」= 往输入框打一个 `@` 把 ComposerMenu 唤起来。 */
  onMention?: () => void;
  /** 「调指令」= 往输入框打一个 `/`。`/` 一直能用，但只在占位符里说过一次，而占位符打第一个字就没了。 */
  onSlash?: () => void;
  hasWorkspace?: boolean;
  /** 审批档（轻助手：真能改）。 */
  approval?: { value: string; onChange: (v: string) => void; options: readonly { id: string; label: string }[] };
  /** 用哪个专家。低频，进二级。 */
  experts?: { value: string; list: { id: string; label: string }[]; onChange: (id: string) => void };
  /** 模型覆盖。19 项，**只能**进二级 —— 平铺会把菜单撑成一屏。 */
  model?: {
    value: string;
    allowFollow: boolean;
    /** 常用（国产/便宜）—— 直接列出来。 */
    list: { id: string; label: string }[];
    /** 贵的（海外旗舰）—— 折在「更多」后面，别让人随手点到。 */
    pricey?: { id: string; label: string }[];
    onChange: (m: string) => void;
  };
}) {
  const { t } = useI18n();
  const [open_, setOpen] = useState(false);
  const [sub, setSub] = useState<string | null>(null);
  /** 「更多（海外旗舰）」展开没 —— 默认收着，别让人随手点到贵的。 */
  const [more, setMore] = useState(false);

  const pick = async (directory: boolean) => {
    setOpen(false);
    const r = await open({ multiple: !directory, directory, title: directory ? t("选一个文件夹") : t("选文件（可多选）") }).catch(() => null);
    const paths = Array.isArray(r) ? r : typeof r === "string" ? [r] : [];
    if (paths.length) onInsert(paths);
  };

  const groups: Group[] = [
    { kind: "action", icon: FilePlus2, label: "添加文件…", hint: t("也可以直接拖进来"), run: () => void pick(false) },
    { kind: "action", icon: FolderPlus, label: "添加文件夹…", run: () => void pick(true) },
  ];
  if (onMention && hasWorkspace) {
    groups.push({ kind: "action", icon: FolderOpen, label: "引用工作区里的文件", hint: "@", run: () => { setOpen(false); onMention(); } });
  }
  if (onSlash) {
    groups.push({ kind: "action", icon: SlashIcon, label: "调指令…", hint: "/", run: () => { setOpen(false); onSlash(); } });
  }
  if (experts) {
    groups.push({
      kind: "sub", icon: Users, label: "专家",
      value: experts.list.find((e) => e.id === experts.value)?.label ?? t("通用助手"),
      items: [
        { id: "", label: t("通用助手"), active: !experts.value },
        ...experts.list.map((e) => ({ id: e.id, label: e.label, active: e.id === experts.value })),
        { id: "__hire__", label: t("＋ 找专家 / 装技能…") },
      ],
      onPick: (id) => { setOpen(false); experts.onChange(id); },
    });
  }
  if (model) {
    /* 🔴 **海外贵的那批默认藏起来**（客户 2026-08-18：「尽量不要让他们碰到海外的模型，
       隐藏海外贵的模型，需要点击更多才展示之类的，避免他们碰到」）。
       判据不是我现编的：`lib/models.ts` 里第三组的组名本来就叫
       「全球旗舰（更聪明，更费额度）」—— 宿主按组切好再传进来，这里只负责折叠。
       **不编倍率数字**：记忆里「贵 6 倍」那次实算是输入 12.5 / 输出 50 倍，
       唯一写错的那份恰好是唯一给客户看的。说不准的数就不说，只说贵。 */
    groups.push({
      kind: "sub", icon: Cpu, label: "模型",
      value: [...model.list, ...(model.pricey ?? [])].find((m) => m.id === model.value)?.label ?? t("跟随驱动设置"),
      items: [
        ...(model.allowFollow ? [{ id: "", label: t("跟随驱动设置"), active: !model.value }] : []),
        ...model.list.map((m) => ({ id: m.id, label: m.label, active: m.id === model.value })),
      ],
      more: model.pricey?.length
        ? { label: t("更多（海外旗舰 · 更费额度）"), items: model.pricey.map((m) => ({ id: m.id, label: m.label, active: m.id === model.value })) }
        : undefined,
      onPick: (id) => { setOpen(false); model.onChange(id); },
    });
  }
  if (approval) {
    groups.push({
      kind: "sub", icon: ShieldCheck, label: "权限",
      value: t(approval.options.find((m) => m.id === approval.value)?.label ?? ""),
      items: approval.options.map((m) => ({ id: m.id, label: t(m.label), active: m.id === approval.value })),
      onPick: (id) => { setOpen(false); approval.onChange(id); },
    });
  }

  return (
    <span className="relative">
      <button
        onClick={() => { setOpen((v) => !v); setSub(null); }}
        title={t("添加文件 / 选专家 / 换模型（也可以直接把文件拖进来）")}
        className={cn(CHIP, CHIP_IDLE, "w-7 justify-center px-0")}
      >
        <Plus size={14} />
      </button>
      {open_ && (
        <>
          <div className="fixed inset-0 z-20" onClick={() => { setOpen(false); setSub(null); }} />
          {/* 往**上**弹：输入框在最底下，往下弹会掉出可视区。
              阴影用 `shadow-pop`（浅色主题下这是白菜单盖在白卡片上，1px 边框分不出层次）。 */}
          <div className="absolute bottom-9 left-0 z-30 w-56 rounded-lg border border-white/[0.10] bg-bg-1 py-1 shadow-pop">
            {groups.map((g) =>
              g.kind === "action" ? (
                <button
                  key={g.label}
                  onMouseEnter={() => setSub(null)}
                  onClick={g.run}
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] text-ink-2 hover:bg-white/[0.05] hover:text-ink-0"
                >
                  <g.icon size={13} className="shrink-0 text-ink-4" />
                  <span className="flex-1 truncate">{t(g.label)}</span>
                  {g.hint && <span className="shrink-0 text-[10px] text-ink-4">{g.hint}</span>}
                </button>
              ) : (
                <div key={g.label} className="relative" onMouseEnter={() => setSub(g.label)}>
                  <button
                    onClick={() => setSub((v) => (v === g.label ? null : g.label))}
                    className={cn(
                      "flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] hover:bg-white/[0.05] hover:text-ink-0",
                      sub === g.label ? "bg-white/[0.05] text-ink-0" : "text-ink-2",
                    )}
                  >
                    <g.icon size={13} className="shrink-0 text-ink-4" />
                    <span className="flex-1 truncate">{t(g.label)}</span>
                    {/* 二级菜单收起时也要看得见**当前选的是哪个** —— 否则得点开才知道，
                        那就把「一眼可见」换成了「多点一下」。 */}
                    {g.value && <span className="shrink-0 max-w-[86px] truncate text-[10px] text-ink-4">{g.value}</span>}
                    <ChevronRight size={12} className="shrink-0 text-ink-4" />
                  </button>
                  {sub === g.label && (
                    /* 落点交给 `SubMenuBox` 现量现算 —— 写死方向的两次都错过，见那边的注释。 */
                    <SubMenuBox>
                      {g.items.map((it) => (
                        <button
                          key={it.id || "__none__"}
                          onClick={() => g.onPick(it.id)}
                          className={cn(
                            "flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] hover:bg-white/[0.05] hover:text-ink-0",
                            it.active ? "text-accent" : "text-ink-2",
                          )}
                        >
                          <span className="flex-1 truncate">{it.label}</span>
                          {it.active && <Check size={12} className="shrink-0" />}
                        </button>
                      ))}
                      {g.more && (
                        <>
                          <button
                            onClick={() => setMore((v) => !v)}
                            className="flex w-full items-center gap-1.5 border-t border-white/[0.06] px-3 py-1.5 text-left text-[11px] text-ink-3 hover:bg-white/[0.05] hover:text-ink-1"
                          >
                            <ChevronDown size={11} className={cn("shrink-0 transition-transform", !more && "-rotate-90")} />
                            <span className="truncate">{g.more.label}</span>
                          </button>
                          {more &&
                            g.more.items.map((it) => (
                              <button
                                key={it.id}
                                onClick={() => g.onPick(it.id)}
                                className={cn(
                                  "flex w-full items-center gap-2 pl-5 pr-3 py-1.5 text-left text-[12px] hover:bg-white/[0.05] hover:text-ink-0",
                                  it.active ? "text-accent" : "text-ink-3",
                                )}
                              >
                                <span className="flex-1 truncate">{it.label}</span>
                                {it.active && <Check size={12} className="shrink-0" />}
                              </button>
                            ))}
                        </>
                      )}
                    </SubMenuBox>
                  )}
                </div>
              ),
            )}
            {/* 🔴 「这一轮：动手前不逐条问你」那行**已删**（2026-08-18 客户：
                「感觉这个都开源，删除，就是自动模式，默认，删除这个提示」）。
                它是改不动的事实，而**一条改不动、每次都在、还占一行的说明**，
                在菜单里只会被当噪音划过去 —— 说了等于没说，还让这个菜单看起来更乱。
                这个事实没有消失：它仍写在 `agent/claude.rs` 顶部、
                也仍是「AI 设置」页要讲的东西。菜单不是讲授权模型的地方。 */}
          </div>
        </>
      )}
    </span>
  );
}
