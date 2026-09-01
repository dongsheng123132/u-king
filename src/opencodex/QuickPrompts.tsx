/**
 * 起手词 —— 借鉴 WorkBuddy 首页那两排：**场景 tab（日常办公 / 代码开发 / 设计创意）+ 该场景下的快捷卡**。
 *
 * **一份实现两处用**（宪法第 12 条）：轻助手（`Chat.tsx`）和 Claude/Codex（`panels/ChatPanel.tsx`）
 * 的输入框上方共用这一个组件 —— 以前只有轻助手那侧有 5 个扁平 chip，而**默认大脑是 Claude**，
 * 也就是绝大多数客户从来没见过起手词。
 *
 * 为什么要分两级：扁平一排的时候，写代码的人和做图的人各自只有一两个能用，其余全是噪声。
 * 分场景后每屏 7 条都跟你当下在干的事有关，且「原来还能干这个」的发现感留在了 tab 上。
 *
 * 两条不许回退的约定：
 * 1. **只在对话为空时出现** —— 它是教学不是常驻工具条；聊起来了还占两行就纯干扰。
 * 2. **点了只填输入框，绝不自动发送** —— 每条都以「：」收尾，后半句必须由用户自己写完。
 *    替他发出去 = 替他编需求，出来的东西不是他要的，比不给更伤信任。
 *
 * 词库来源：`quickPrompts` 数组即真相源，改词只动这个文件，两处输入框同时生效。
 * 能力边界跟着这个对话框真有的工具走（画图 / 视频 / 文档 / 表格 / 网页 / 读写文件 / 跑命令），
 * **不许写超纲的活**（订机票、发微信、连第三方账号）——点了做不出来就是骗。
 */
import { useEffect, useState } from "react";
import {
  Bug, Code2, Film, FileCode, FileSearch, FileText, Languages, LayoutTemplate,
  Image as ImageIcon, PenLine, Presentation, Save, ScrollText, Smile, Sparkles,
  Table, Terminal, Wand2, ListChecks, UserRound, ChevronDown, Users,
} from "lucide-react";
import type { Engine } from "./types";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";
import { useViewport } from "../lib/useViewport";

/**
 * `best` = 这活哪个大脑拿手 **+ 为什么**。**只在真有能力差的时候才标**，不是给每条都配一个。
 *
 * 两类真实的能力差（都不是"谁更聪明"——四个大脑底下跑的是同一批虾盘云模型，差的是外壳）：
 * - **作图/出片 → `uking`**：轻助手自带原生 `generate_image` / `generate_video`，一步出成果、
 *   直接进右侧预览。同样的活给 Claude Code 干，它得去跑 uking-aigc 脚本，慢一截还多一层会挂的环节。
 * - **办公产物（PPT/Word/Excel/网页）→ `claude`**：真文件靠 `uking-ppt` / `uking-docx` / `uking-xlsx`
 *   技能包，而技能包只装进 Claude Code / OpenClaw 的 skills 目录（`skillpack.rs::install_into_tools`）。
 *   轻助手**没有技能机制**，同一句"做个 PPT"它只能现编，出不来能打开的文件。
 *
 * 纯文字类（写周报、总结会议、翻译）**不标** —— 谁都干得了，切来切去反而添乱。
 *
 * `why` 必须跟着条目走，不能写死在 `Chat.tsx` 的 toast 里：原先那句写死的是"自带出图/出片工具"，
 * 一旦标了第二类活就会说错话。**替人做决定还把理由说错，比不替他做更伤信任。**
 *
 * 客户选的是「我要干什么」，不该是「我要用哪个 AI」。
 */
export type Best = { engine: Engine; why: string };
type Quick = { label: string; template: string; icon: typeof FileText; best?: Best };

/** 作图/出片：轻助手的原生工具一步到位。 */
const BEST_DRAW: Best = { engine: "uking", why: "自带出图/出片工具，一步出成果、直接进右边预览" };
/** 办公产物：靠 PPT/文档/表格技能包，只有 CLI 大脑装得上。 */
const BEST_OFFICE: Best = { engine: "claude", why: "装了 PPT/文档/表格技能包，能直接产出可打开的文件" };
/**
 * 搭工作台：**方法**在 `uking-workbench` 技能里（怎么只读盘点、该问哪几句、manifest 怎么写才合格），
 * 而技能只装进 CLI 大脑的 skills 目录 —— 轻助手没有技能机制。
 * 落盘那一步两边其实都够得着（都是影核动作 `runtime.workbench.install`），差的是「怎么搭」那半边。
 */
const BEST_WORKBENCH: Best = { engine: "claude", why: "装了搭工作台的技能，会先盘点你的文件夹再动手" };

const SCENES: { scene: string; items: Quick[] }[] = [
  {
    scene: "日常办公",
    items: [
      // ★ 「搭工作台」= 按客户自己的活给他配一套目录约定 + 说明书，之后任何 AI 进那个文件夹
      //   都知道该干什么。摆在第一条是因为它是**一次性的、且让后面每一条都变好用**的那件事。
      //   模板照旧以「：」收尾 —— 后半句（他那个文件夹在哪）必须他自己写，替他填就是替他编需求。
      {
        label: "搭我的工作台",
        template: "帮我搭一个我自己的工作台。先只读盘点一下这个文件夹再问我几句，别急着建目录：",
        icon: LayoutTemplate,
        best: BEST_WORKBENCH,
      },
      { label: "写周报", template: "帮我写这周的周报，我的工作是：", icon: FileText },
      { label: "总结会议", template: "把这段会议记录总结成要点：", icon: ListChecks },
      { label: "做表格", template: "帮我做一个表格，统计这些数据：", icon: Table, best: BEST_OFFICE },
      { label: "做幻灯片", template: "把这份内容做成幻灯片：", icon: Presentation, best: BEST_OFFICE },
      { label: "改简历", template: "帮我改一下这份简历，我想：", icon: UserRound, best: BEST_OFFICE },
      { label: "写公众号", template: "帮我写一篇公众号文章，主题是：", icon: PenLine },
      { label: "翻译文档", template: "把这份文档翻译成中文：", icon: Languages },
    ],
  },
  {
    scene: "代码开发",
    items: [
      { label: "写代码", template: "帮我写一段代码，功能是：", icon: Code2 },
      { label: "改代码", template: "帮我改一下这段代码，问题在于：", icon: FileCode },
      { label: "找问题", template: "这段代码跑不通，帮我找找原因：", icon: Bug },
      { label: "跑命令", template: "帮我在终端里运行这个命令：", icon: Terminal },
      { label: "读文件", template: "帮我看看这个文件的内容：", icon: FileSearch },
      { label: "存文件", template: "把这串内容存成一个文件：", icon: Save },
      { label: "写脚本", template: "帮我写个小脚本，用来：", icon: ScrollText },
    ],
  },
  {
    scene: "设计创意",
    items: [
      { label: "画张图", template: "帮我画一张图，内容是：", icon: ImageIcon, best: BEST_DRAW },
      { label: "改图片", template: "帮我改一下这张图片，我想：", icon: Wand2, best: BEST_DRAW },
      { label: "做海报", template: "帮我做一张活动海报，主题是：", icon: LayoutTemplate, best: BEST_DRAW },
      { label: "做视频", template: "帮我做一个视频，内容是关于：", icon: Film, best: BEST_DRAW },
      { label: "找配图", template: "帮我配一张插图，风格要：", icon: Sparkles, best: BEST_DRAW },
      { label: "做封面", template: "帮我设计一个封面，标题是：", icon: ImageIcon, best: BEST_DRAW },
      { label: "做头像", template: "帮我画一个头像，我想要：", icon: Smile, best: BEST_DRAW },
    ],
  },
];

/** 拍平的全表 —— 给输入框 `/` 指令面板用（同一份词库，不复制第二份）。 */
export const ALL_QUICK: { scene: string; label: string; template: string; best?: Best }[] = SCENES.flatMap((s) =>
  s.items.map(({ label, template, best }) => ({ scene: s.scene, label, template, best })),
);

/** 默认铺开几个起手词。5 个在 1280 宽上正好一行不换行（实测），再多就开始吃第二行。 */
const FIRST_ROW = 5;

const SCENE_KEY = "uking.chat.quickscene";

export function QuickPrompts({ onPick, onFindExpert, className }: {
  /** 点了填哪句话；`best` = 这活哪个大脑拿手（没有就是「当前这个就行」）。 */
  onPick: (template: string, best?: Best) => void;
  /** 点「找专家」→ 打开左栏专家墙。不传就不显示这颗（独立终端页那类地方没有左栏）。 */
  onFindExpert?: () => void;
  className?: string;
}) {
  const { t } = useI18n();
  // 矮屏（见 lib/useViewport.ts）：这两排在 1366×768 上要占掉 ~72px 的对话高度。
  // **只收尺寸，一条词都不减** —— 起手词是教学，砍掉几条等于对这台机器上的客户说
  // 「你的电脑不配知道还能干这个」。
  const { short } = useViewport();
  // 记住上次选的场景：会用这个工作台的人多半长期在同一个场景里，每次回到「日常办公」是白让他多点一下
  const [scene, setScene] = useState(() => {
    const saved = localStorage.getItem(SCENE_KEY);
    return SCENES.some((s) => s.scene === saved) ? saved! : SCENES[0].scene;
  });
  const pickScene = (s: string) => {
    setScene(s);
    localStorage.setItem(SCENE_KEY, s);
  };
  const items = SCENES.find((s) => s.scene === scene)?.items ?? SCENES[0].items;
  // 换场景就收回一行 —— 不然点了「更多」之后再切场景，新场景也是摊开的，等于默认全铺
  const [expanded, setExpanded] = useState(false);
  useEffect(() => setExpanded(false), [scene]);

  return (
    /* 靠左（2026-08-18 客户改口：「起手词还是靠左边吧」）。
       我上一版按「和居中的输入框对齐」改成了居中 —— 但输入框本身是一整块卡片、
       左边缘就是内容的起点，起手词靠左才跟它同一条基线；居中反而像浮在中间。 */
    <div className={cn(short ? "space-y-1" : "space-y-2", className)}>
      <div className="flex items-center gap-1.5">
        {SCENES.map((s) => (
          <button
            key={s.scene}
            onClick={() => pickScene(s.scene)}
            className={cn(
              "px-3 rounded-full text-[12px] transition-colors",
              short ? "h-6" : "h-7",
              scene === s.scene
                ? "bg-accent text-white"
                : "bg-bg-1 border border-white/[0.08] text-ink-3 hover:text-ink-1 hover:border-accent/30",
            )}
          >
            {t(s.scene)}
          </button>
        ))}
      </div>
      {/* 🔴 默认**只铺一行**，其余折进「更多」。
          这不违反本文件开头那条「只收尺寸，一条词都不减」—— **一条都没删**，
          全部仍在一次点击之内。改的是默认铺开的量：原来 3 行（场景 tab + 8 个胶囊换行）
          在中间占掉近 90px，而 dsh / ClawX 那类工具这块是 0。
          用户 2026-08-18：「起手词的上面字太多了」+「专家可以替代起手词」。 */}
      <div className="flex items-center gap-1.5 flex-wrap">
        {(expanded ? items : items.slice(0, FIRST_ROW)).map(({ label, template, icon: Icon, best }) => (
          <button
            key={label}
            onClick={() => onPick(t(template), best)}
            title={t(template)}
            className={cn(
              "inline-flex items-center gap-1 px-2.5 rounded-full bg-bg-1 border border-white/[0.08] text-[11px] text-ink-2 hover:border-accent/40 hover:text-ink-0",
              short ? "h-6" : "h-7",
            )}
          >
            <Icon size={12} className="text-accent/80" /> {t(label)}
          </button>
        ))}
        {!expanded && items.length > FIRST_ROW && (
          <button
            onClick={() => setExpanded(true)}
            className={cn(
              "inline-flex items-center gap-0.5 px-2 rounded-full text-[11px] text-ink-4 hover:text-ink-1",
              short ? "h-6" : "h-7",
            )}
          >
            {t("还有 {n} 个", { n: items.length - FIRST_ROW })} <ChevronDown size={11} />
          </button>
        )}
        {/* 「找专家」—— 起手词是**一句话模板**，专家是**带 persona + 技能的一整套**。
            用户原话：「和起手词差不多，但重很多」。摆在同一行末尾，让人知道还有更重的一档。 */}
        {onFindExpert && (
          <button
            onClick={onFindExpert}
            className={cn(
              "inline-flex items-center gap-1 px-2.5 rounded-full border border-accent/30 bg-accent/[0.08] text-[11px] text-accent hover:bg-accent/[0.14]",
              short ? "h-6" : "h-7",
            )}
          >
            <Users size={12} /> {t("找专家")}
          </button>
        )}
      </div>
    </div>
  );
}
