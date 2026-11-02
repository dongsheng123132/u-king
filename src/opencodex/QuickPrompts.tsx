/**
 * 起手词 —— 对话为空时给客户「我该问什么」的示范。
 *
 * # 2026-11-02：从「4 场景 × 27 条 + 场景 tab」砍到 **3 条扁平**
 *
 * 会审判据（老板口径 U-King = 360 软件管家：帮装、帮充值、快速使用，不做复杂）：
 * **教客户怎么用原版，最多一条引导文案，不做交互。** 起手词的场景 tab + 展开/收起
 * 是一套小型引导系统，占两行高度、还要记 localStorage 上次选了哪个场景 ——
 * 对一个「装完就想问一句」的客户，这是纯负担。
 *
 * 现在只留 3 条，覆盖管家最核心的三个动作：**装（装 AI 工具）/ 写（写周报）/ 做（做幻灯片）**。
 * 词库本身**没删**（`SCENES` 仍在）—— 输入框 `/` 指令面板还从这里取全表，
 * 那是客户主动去翻的地方，不算打扰。
 *
 * # 两条不许回退的约定
 *
 * 1. **只在对话为空时出现** —— 它是教学不是常驻工具条；聊起来了还占地方就纯干扰。
 * 2. **点了只填输入框，绝不自动发送** —— 每条都以「：」收尾，后半句必须由用户自己写完。
 *    替他发出去 = 替他编需求，出来的东西不是他要的，比不给更伤信任。
 *
 * 词库来源：`quickPrompts` 数组即真相源，改词只动这个文件，两处输入框同时生效。
 * 能力边界跟着这个对话框真有的工具走（画图 / 视频 / 文档 / 表格 / 网页 / 读写文件 / 跑命令），
 * **不许写超纲的活**（订机票、发微信、连第三方账号）——点了做不出来就是骗。
 */
import {
  Bug, Code2, Film, FileCode, FileSearch, FileText, Languages, LayoutTemplate,
  Image as ImageIcon, PenLine, Presentation, Save, ScrollText, Smile, Sparkles,
  Table, Terminal, Wand2, ListChecks, UserRound,
  Download, Wrench, Stethoscope, RefreshCw, HardDriveDownload,
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
const BEST_WORKBENCH: Best = { engine: "claude", why: "装了 workbench 技能，会先只读盘点再问清楚，不乱建目录" };

/**
 * 词库全表 —— 场景分组仍然保留，但**不再有 tab UI**。
 *
 * 它现在的唯一消费者是输入框的 `/` 指令面板（`ALL_QUICK` 拍平后给它）：
 * 客户主动敲 `/` 是在找「还能干什么」，那才是发现感的正确位置；
 * 而空对话时铺一大堆，是把发现感变成了入场税。
 */
const SCENES: { scene: string; items: Quick[] }[] = [
  {
    scene: "日常办公",
    items: [
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
  {
    // 每一条都对应 `uking_action` 里真实存在的动作（体检/安装/修复），不是随口一句客服话术：
    // 装 → runtime.aitool.install，排查 → runtime.tool.inspect + runtime.optimizer.apply，
    // 体检 → runtime.diagnostics.collect，环境补件 → runtime.env.install_tools，
    // 卸载重装 → runtime.aitool.uninstall + runtime.aitool.install。都以「：」收尾，
    // 具体哪个工具/什么现象仍由用户自己写完，不替他编。
    scene: "安装与修复",
    items: [
      { label: "装 AI 工具", template: "帮我安装这个 AI 工具：", icon: Download },
      { label: "排查打不开", template: "这个 AI 工具打不开了，帮我排查：", icon: Wrench },
      { label: "环境体检", template: "给我的电脑做个 AI 环境体检：", icon: Stethoscope },
      { label: "补齐环境", template: "帮我检查一下电脑上缺不缺 Node / Git 这些运行环境，缺的话装上：", icon: HardDriveDownload },
      { label: "卸载重装", template: "帮我把这个 AI 工具卸载再重新装一遍：", icon: RefreshCw },
    ],
  },
];

/** 拍平的全表 —— 给输入框 `/` 指令面板用（同一份词库，不复制第二份）。 */
export const ALL_QUICK: { scene: string; label: string; template: string; best?: Best }[] = SCENES.flatMap((s) =>
  s.items.map(({ label, template, best }) => ({ scene: s.scene, label, template, best })),
);

/**
 * 空对话时铺出来的 3 条。**只有 3 条，且不分组、不折叠、不记忆上次选择。**
 *
 * 挑法是按「管家最核心的三个动作」：装 / 写 / 做 ——
 * 装（装 AI 工具）：这是 U-King 存在的第一理由；
 * 写（写周报）：不知道问什么的人，第一次最常问的就是这个；
 * 做（做幻灯片）：能拿到真文件，是「这软件值不值」最直观的一次交付。
 */
const PICKED: Quick[] = [
  { label: "装 AI 工具", template: "帮我安装这个 AI 工具：", icon: Download },
  { label: "写周报", template: "帮我写这周的周报，我的工作是：", icon: FileText },
  { label: "做幻灯片", template: "把这份内容做成幻灯片：", icon: Presentation, best: BEST_OFFICE },
];

export function QuickPrompts({ onPick, className }: {
  /** 点了填哪句话；`best` = 这活哪个大脑拿手（没有就是「当前这个就行」）。 */
  onPick: (template: string, best?: Best) => void;
  className?: string;
}) {
  const { t } = useI18n();
  // 矮屏（见 lib/useViewport.ts）：这三个胶囊在 1366×768 上也不是零成本，跟着收高度。
  const { short } = useViewport();

  return (
    /* 靠左（2026-08-18 客户改口：「起手词还是靠左边吧」）。
       输入框本身是一整块卡片、左边缘就是内容起点，起手词靠左才跟它同一条基线。 */
    <div className={cn(short ? "space-y-1" : "space-y-2", className)}>
      <div className="flex items-center gap-1.5 flex-wrap">
        {PICKED.map(({ label, template, icon: Icon, best }) => (
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
      </div>
    </div>
  );
}
