/**
 * 左侧边栏导航 —— 主流商务风（参考 Linear / cc-switch）。
 * 管家功能区（我的 AI / 装机向导 / Codex 专区 / AI 设置）+ 底部 OpenCodex 工作台入口
 * + 一排「我的 AI」快捷启动图标（Dock）+ 终端抽屉开关。
 */
import { useState } from "react";
// 注：Clapperboard 曾是 T-King 影爆的图标，该项 0.9.85 从导航摘掉（见 LAB 注释），
// 图标随之从 import 里去掉（noUnusedLocals 会拦）。放回来时记得连它一起加回来。
import { ArrowUpCircle, ChevronDown, Cpu, FlaskConical, Gauge, Globe, HardDrive, History, Languages, Layers, LifeBuoy, MessageSquare, Moon, MoreHorizontal, Palette, PanelLeftClose, PanelLeftOpen, PanelTopClose, Sparkles, Sun, Wallet, Wand2, Wrench } from "lucide-react";
import { Logo } from "./Logo";
import { cn } from "../lib/cn";
import { SidebarMiniApps } from "./SidebarMiniApps";
import { useViewport } from "../lib/useViewport";
import { useI18n } from "../i18n";

import type { TuiAppId } from "../opencodex/apps";

// TUI 应用（claude/codex-cli/openclaw/hermes）+ 工作台 + 终端页 + 管家页面 + AI 作图
export type TabId =
  | TuiAppId
  | "workbench"
  | "dshplugins"
  | "terminal"
  | "setup"
  | "chat"
  | "create"
  | "experts"
  | "myai"
  | "xiapan"
  | "skills"
  | "codex"
  | "manage"
  | "draw"
  | "qrmerge"
  | "video"
  | "reel"
  | "tasks"
  | "geo"
  | "skillpack"
  | "toolbox"
  | "backup"
  | "advanced"
  | "feedback"
  | "guide"
  | "airuntime"
  | "meter"
  | "identity"
  | "nightshift"
  | "localllm"
  | "rtk"
  | "teamspace"
  | "runcenter";

type NavItem = { id: TabId; label: string; sub: string; icon: typeof Wand2 };

/** 核心 4 项 —— 按「客户每天真正干什么」排，不是按「我们做了什么」排。
 *
 * 0.9.83 依测试报告 #010 重排：原来 U-Workspace 排第三、「AI 设置」占着核心位。
 * 那是**开发视角**的排法：配置在我们眼里重要，但客户装完只配一次，之后天天开的是工作台。
 * 把一个一次性动作放在天天用的动作前面，等于每天都收一次「你还没配好」的税。
 * 当时据此把「AI 设置」下沉进了「更多」。
 *
 * 🔴 **2026-08-31 用户拍板：AI 设置升回核心第 4 位**（原话：把 AI 设置放在左侧的前面
 * 4 个里边，默认露出在外面，放在我的 AI 下面，这个功能比较主要）。理由：
 * 这页已经不是「配一次就走」的设置页了 —— 它装着一键体检（doctor）/ 一键升级
 * （CLI 全部更到最新 + 本体升级），是**健康维护的常驻入口**，跟「装好就用」是同一频次的事。
 *
 *   ① U-Workspace —— 主交互面（对话 + 终端 + 作图预览），干活入口，必须第一
 *   ② AI 创作     —— 作图/视频/海报，第二高频
 *   ③ 首页·我的AI  —— 装机→一键配好→充值→启动的漏斗 + 启动台
 *   ④ AI 设置     —— 换模型/余额/一键体检/一键升级（维护入口，常驻）
 *
 * **首页故意留在核心**：首页是小白唯一的装机漏斗，摘掉等于新客户开机即无路可走。
 */
const CORE: NavItem[] = [
  // U-Workspace（AI 工作台）= 主交互面：对话 + 终端 + 作图预览，全都在 U-King 内，交互尽量不外跳。
  //
  // 🔴 **命名约定（2026-08-16 定，别再混着叫）**：
  //   · **U-Workspace** = 这个「工作台」整体（左栏会话 + 中间 + 右侧面板）。**只指这一层**，
  //     不要拿它称呼里面的对话框或终端 —— 以前 bug 单里「U-Workspace 排版糟糕」到底指哪块，
  //     得回头翻截图才知道（issue #379）。
  //   · **U-Chat** = 工作台里那个 GUI 对话框（`opencodex/Chat.tsx` + `panels/ChatPanel.tsx`）。
  //   · **U-CLI**  = 工作台里那个终端界面（`panels/TermPanel.tsx` + `term/useTermGroup.ts`）。
  // 面向客户的文案保留「对话 / 终端」这类人话，代号只用来**指认是哪一块**。
  { id: "chat", label: "U-Workspace", sub: "对话 · 终端 · 作图出片，一站干活", icon: MessageSquare },
  // 「AI 创作」2026-08-23 **回到核心位**（用户拍板：「客户希望留」）。
  //
  // 它 08-21 被 `6bb9409` 摘掉，理由写的是「它和 U-Chat 是同一件事的两个入口，收进
  // U-Workspace 右侧的『创作』面板」。🔴 **那个论证成立的是「实现该合并」，推不出「入口该消失」**
  // —— CLAUDE.md 自己写着「能力和入口是两个开关，别一起拉」，08-21 那刀是反向拉错了：
  // 合了能力，顺手把入口也拔了。客户看到的就是「作图功能没了」。
  //
  // 本次同时**摘掉了工作台右侧那个「创作」面板**（`opencodex/Chat.tsx`），不是两边都留 ——
  // 同一份 `Create.tsx` 挂两处会有两份互不相通的历史和保活状态，客户在这边生成的图
  // 在那边看不到，这种不一致比少一个入口更难解释。**一个能力一个入口。**
  //
  // 放 CORE 不放 MORE：本组排序注释（下面）原本就写着「② AI 创作 —— 第二高频」；
  // 且 MORE 里清一色是配置/维护向，把一个「客户天天用来产出东西」的功能放进去等于再藏一次。
  { id: "create", label: "AI 创作", sub: "作图 · 视频 · 海报二维码", icon: Palette },
  // 「首页 · 我的 AI」→「我的 AI」（用户 2026-08-25：它就在侧栏上，「首页」两个字多余）。
  { id: "myai", label: "我的 AI", sub: "装机 · 一键配好 · 启动", icon: Sparkles },
  // 「AI 设置」2026-08-31 升回核心第 4 位（用户拍板，见上方 CORE 组注释）。
  // 0.9.83 下沉时它是「配一次就走」的设置页；现在装着一键体检/一键升级，成了
  // 健康维护的常驻入口 —— 客户隔着几天就要来看一眼余额/有没有新版/各 AI 配没配好。
  { id: "manage", label: "AI 设置", sub: "换模型 · 余额 · 一键体检升级", icon: Cpu },
];

/** 「更多」—— 进阶/工具页，默认折叠，不让小白眼花。
 *  排序：使用向（虾盘云/教程/专家/Codex）在前，维护向（优化/备份/进阶）在后。 */
const MORE: NavItem[] = [
  { id: "runcenter", label: "运行中心", sub: "Trace · TTFT · 成本 · 回测", icon: Gauge },
  // 创作内的左侧功能栏已收纳「一键成片/任务中心」：同一能力不再在全局 More 重复入口。
  { id: "teamspace", label: "团队空间", sub: "项目协作 · 文件锁 · 人工审批", icon: Layers },
  // 「AI 设置」2026-08-31 升回核心第 4 位（用户拍板）—— 从 MORE 摘除，见 CORE 里那条。
  // 身份 + 给 AI 的说明书（llms.txt）+ 往别家记忆文件插指针的开关。**归「更多」不归「实验室」**：
  // 四条毕业标准全过（进了影核动作表 / 有 ready+blockers / 不依赖外部环境 / 正在主线上）。
  // 不进「核心」的理由和「AI 设置」完全一样 —— 配一次的东西不该天天占核心位。
  //
  // 🔴 1.0.3 改名（用户 2026-08-18 原话：「我的 uking 是啥没搞懂」）。
  // 原来叫「我的 U-King」+「起名 · 身份 · 给 AI 的说明书」—— 三个词没有一个说出它**对这台电脑做了什么**。
  // 而它做的事恰恰是用户当天抱怨过的那件：往 `~/.claude/CLAUDE.md` / `~/.codex/AGENTS.md`
  // 各插一段带标记的指针。**看不懂名字的开关，用户不会去关它，只会以为软件在乱改自己的文件。**
  // 名字里必须出现「CLAUDE.md」和「可撤」这两个词，否则改了等于没改。
  //
  // 「让 AI 认识 U-King」条目 2026-08-22 摘掉 —— **页面和路由原样**，入口收进
  // 「AI 设置 → 高级」的配置页卡片（Manager 的 onGoPage 那组）。它是「配一次就不再进」
  // 的页面，和本次一起摘的 rtk / dshplugins / localllm 同一批：侧栏挤不是因为东西该塞进
  // chat，是配置页太多（docs/需求榜.md 2026-08-22 批次 F1）。
  // 虾盘云专页保留可达（充值中心 + 接入指南）—— 首页状态卡已覆盖主流程，这里是深度入口
  { id: "xiapan", label: "虾盘云 · 充值", sub: "内置 Key · 充值 · 一键配好", icon: Wallet },
  // 🔴 「AI 专家」和「AI 技能 / 上手」两条 1.0.3 从侧栏**撤掉**（用户 2026-08-18：
  // 「左侧的专家和 uchat 里的专家合并，左侧无专家和 ai 技能了」）。
  //
  // 撤的理由不是它们没用，是**同一件事在两处各有一个入口**：专家墙
  // （`opencodex/ExpertGallery.tsx`）本来就常驻 U-Workspace 左栏，客户天天从工作台点它；
  // 侧栏那条只是套了个壳再挂一遍。按「舞台 / 演员」的口径，专家是**演员**——
  // 演员在舞台上招，不在设置菜单里招。
  //
  // **没有东西变得不可达**：技能页（SkillPack + 技能市场 + 教程）跟 `skillpack` tab
  // 是同一个渲染分支，而后者被作图 / 视频 / AI 创作的 `onGoSkillPack` 深链着。
  // skillhub.cn 入口已挪进 ExpertGallery 顶部。
  // 「Codex 工作站」2026-08-03 从这里下沉到「实验室」——**不是因为它不好用**，
  // 而是它服务的对象（Codex 桌面版 GUI）已经不在主线上了。见 LAB 里的条目。
  // 「AI 加速」0.9.34 起放出：ukrt.exe 已内嵌进本体（airuntime.rs include_bytes，
  // 首次使用自动释放到 ~/.uking/ukrt/），客户机不再依赖外部分发，报错卡问题不存在了。
  // 注：AI 优化大师规划做成「独立开源工具箱」，故暂不与厨具工具箱合并（保持互不污染，2026-07-23）。
  // 「省钱三件套」按 量 → 调 → 压 的顺序摆在一起：先有表看得见，调和压才有得对照。
  // 水电表是这条线的地基（原本只是「AI 设置」页里一个折叠小节，没人找得到）。
  { id: "meter", label: "Token 水电表", sub: "所有 AI 用了多少 token · 花在哪 · 怎么省", icon: Gauge },
  { id: "airuntime", label: "AI 优化大师", sub: "让 AI 工具跑得更稳、更省 token", icon: Wand2 },
  // 「Token 压缩机」2026-08-10 降到实验室（Issue #376，客户机 macOS 0.9.94 实锤）：
  // 它的 hook 把每条命令改写成裸 `rtk …`，而 Mac 上我们从没把 shim 接进过 PATH
  // → 那台机器上**每一条 Bash 命令**退出码 127，整台机器的 AI 变废。
  // 根因已修（hook 改走 U-King 包装器，绝对路径，见 rtk.rs::hook_command），
  // 但**它伤过客户，得在实验室待一版**再谈回来 —— 而且它本来就符合实验室标准：
  // 唯一一个会改写用户每一条命令的功能，风险最不对称。
  // 恢复：把下面这行加回 MORE、并从 LAB 摘掉即可（页面/路由/动作一个没删）。
  // 网站 GEO 体检 —— 2026-08-23 用户拍板从「实验室」升上来（`da7c565` 整块 revert 回来）。
  //
  // 🔴 **恢复它的理由不是「它进了三根柱子」** —— 它仍然不在「装 / 配 / 用」上，da7c565
  // 当初的删除理由到今天依然成立。恢复的理由是那个 commit 自己挂起、当时没人回答的问题：
  // 「这个页面带着**付费服务的下单入口**（支付宝下单帮你做 GEO+MEO 优化），是一条变现漏斗，
  // 不只是一个功能。删代码 ≠ 删这条生意。」—— 用户 08-23 给出的答案是：这条生意要留，
  // 那它就得有一个客户点得到的入口。
  //
  // 🔴 **证据状态要如实记着，别让后人以为这是需求驱动的**：08-23 全量翻过两个仓库
  // （两个工单仓共 460 条，open+closed），**客户询问 0 条**，
  // 真实使用只有 1 台设备 1 次免费体检（#376 的日志里）。唯一旁证是 #310 那位客户在用
  // 我们的作图功能做「淘宝本地化GEO运营」海报 —— 那是**画像证据，不是需求证据**。
  // 支撑这次恢复的是产品负责人在售后微信侧的判断，issue 渠道查不到。
  //
  // ⚠️ **漏斗本身有两个硬缺口，露出来之前该知道**（`1so-geo/src/pay.mjs`）：
  //   ① 服务端支付有单笔上限，最高档位超过上限不能直付；
  //   ② 服务端**没有「专项订单」这个概念** —— 付款在系统里就是一笔普通 token 充值，
  //      全靠客户在付款备注里写公司名+档位人工对账，不写就跟普通充值分不出来。
  // 这两条已进 docs/需求榜.md，不修的话入口露出来 = 客户点得到但我们收不明白。
  //
  // 命名：**不叫「GEO 优化大师」**。侧栏隔壁就是「AI 优化大师」（airuntime，管的是本机 AI 工具），
  // 两个「优化大师」对象完全不同却同名同姓，客户必点错。这里按**对象**命名不按头衔命名。
  //
  // 退出条件：90 天内付费下单为 0，则按 da7c565 的原理由再删一次 —— 且下次要连
  // `skills/experts/geo-optimizer/` 专家包一起删干净，别再留「专家在、工具不在」的半截。
  { id: "geo", label: "网站GEO体检", sub: "各家 AI 认不认识你 · 免费自查", icon: Globe },
  { id: "backup", label: "备份/同步", sub: "对话设置存 U 盘 · 多电脑切换", icon: HardDrive },
  // 「本地大模型」2026-08-25 从「AI 设置 → 高级」的入口卡**升回侧栏**（用户拍板：
  // 「本地大模型还是要调出来到左侧」）。页面/路由/动作原样 —— 它当初进 AI 设置的理由
  // （「配一次就不再进」）依然成立，但用户判断它是卖点级功能，藏一层等于没有。
  // 放「更多」末尾：不挤核心三件，但一展开就看得见。
  { id: "localllm", label: "本地大模型", sub: "Ollama · llama.cpp · 离线免费跑 AI", icon: Cpu },
  { id: "advanced", label: "进阶 / App 版", sub: "Hermes/ClawX 桌面版 · 手动配", icon: Layers },
  // 「AI 作图 / 海报二维码 / AI 视频」2026-07-19 合并进核心项「AI 创作」（Create.tsx 壳页），
  // 原独立路由（draw/qrmerge/video）保留可达（AI 专家页深链仍用），只是不再单独占导航位。
];

/**
 * 「U-King 实验室」（2026-07-27 做减法）—— 没毕业的功能收在这里，默认折叠。
 *
 * 收进来的标准**不是「不好用」**，而是「还不到能往小白脸上推的程度」，具体是下面任一条：
 *   · 没进影核动作表 / 没有 `ready`+`blockers`（回答不了「这台机器上它到底能不能用」）
 *   · 依赖外部环境（Node / Ollama / winget），客户机成功率不稳
 *   · 品类不在「一键装好你的全部 AI」这条主线上
 *
 * 毕业标准就是上面那三条的反面，组内提示里对用户如实写着。
 * 关键是**别再靠「排在最底下 + 挂个测试徽章」当隐藏** —— T-King 影爆之前就是这么"藏"的，
 * 而任何人点开「更多」都能看见它，等于没藏。
 *
 * 页面和路由全部保留，只是不再和成熟功能混在一起。
 */
const LAB: NavItem[] = [
  // 泊舟 AI 小程序（独立应用、自带更新）的入口就在这一页顶部 —— 写进 sub 里，
  // 否则客户根本猜不到「泊舟」藏在「小程序」底下。它按实验室标准归这儿：
  // 是独立发版的外部应用，不在「一键装好你的全部 AI」主线上。
  // 夜班助手（0.9.89 新增）。按本组标准归这儿 —— 它的**记录**那一半已经进影核动作表
  // （`runtime.journal.inspect`，带 ready/blockers），但**执行**那一半（熔断/护栏/快照/交班）
  // 还全是「暂未开放」。功能没做完就往主线上推，客户点开一排灰按钮只会觉得产品是坏的。
  // 毕业标准：执行类能力真正开放、且在真机跑过一整晚。
  { id: "nightshift", label: "夜班助手", sub: "AI 值班 · 行为记录 · 可追溯", icon: Moon },
  // 「AI 专家」已毕业到「更多」（0.9.83，见上）—— 别再在这里留第二份入口。
  // 「网站 GEO 体检」2026-08-23 从这里升到「更多」—— 它带付费下单漏斗，是变现口子，
  // 藏在默认折叠的实验室里等于没有。理由和退出条件写在 MORE 那条上面。
  { id: "toolbox", label: "厨具工具箱", sub: "给 AI 装 ffmpeg/Chrome 等能力工具", icon: Wrench },
  // 🔴 2026-08-22 摘掉四条（F1，docs/需求榜.md 当日批次）：rtk「Token 压缩机」·
  // codex「Codex 桌面版工作站」· dshplugins「DSH 插件」· localllm「本地大模型」。
  // **页面/路由/动作全部原样**，入口收进「AI 设置 → 高级」的配置页卡片
  // （Manager 的 onGoPage 那组；Codex 专区在那儿本来就有自己的入口卡）。
  // 理由：它们全是「配一次就不再进」的配置页，却各占一格侧栏 —— 对照智序 AI 的结论是
  // 「侧栏该展示你有什么，不是我们有什么」，配置归配置中心，侧栏的格子留给用户的东西。
  // 各条目原先的下沉理由（rtk 伤过客户 / codex 品类不在主线 / localllm 未到毕业标准）
  // 依然成立，只是「没毕业」的表达方式从「实验室垫底」换成「住在 AI 设置里」。
  // ⚠️ 2026-08-25 更新：localllm 已从「AI 设置 → 高级」升回侧栏「更多」（用户拍板），
  // 见 MORE 里那条；本组注释里关于它的部分只剩历史意义。
];

/** Dock 项分组：desktop=桌面应用（Codex 桌面版/ClawX）、cli=命令行工具（OpenClaw/Claude/Hermes）。 */
export type DockGroup = "desktop" | "cli";

/** Dock 快捷项：① workbench=OpenCodex ② tui=独立 TUI 应用（claude/codex/openclaw/hermes，切 tabId）
 *  ③ launch=纯 GUI 应用（Codex 桌面版/ClawX，外部启动）。
 *  tool=品牌图标 key（claude/codex/openclaw/hermes…）；active=已装/可用（着色，否则灰显）；
 *  group=左侧分桶。 */
export type DockApp =
  | { id: string; name: string; kind: "workbench"; group?: DockGroup }
  | { id: string; name: string; kind: "tui"; tabId: TabId; tool: string; active: boolean; group?: DockGroup }
  | { id: string; name: string; kind: "launch"; tool: string; active: boolean; group?: DockGroup };

export function Sidebar({
  active,
  onSelect,
  version,
  onShowChangelog,
  platform,
  onOpenSite,
  theme = "light",
  onToggleTheme,
  hasUpdate,
  latestVersion,
  onUpdate,
  updating,
  updatePct,
  updateFailed = 0,
  updateFailReason,
  onReinstall,
  onHideToTray,
}: {
  active: TabId;
  onSelect: (t: TabId) => void;
  version: string;
  /** 点版本号 → 打开更新日志（不传就是纯文本，不可点）。 */
  onShowChangelog?: () => void;
  /** get_env 探测到的操作系统（"windows"/"macos"/…）。用于隐藏平台专属入口，避免点开就报错。 */
  platform?: string;
  theme?: "light" | "dark";
  onToggleTheme?: () => void;
  /** 点底部「官网」打开官网（方便联系/找下载/充值）。极简：只放官网这一个入口。 */
  onOpenSite?: () => void;
  /** 有新版可升级——侧栏常驻升级入口。因为升级横幅只在「管家页」的 StatusLine 露出，
   *  用户长期待在 U-Workspace / 终端 / TUI 页时永远看不到升级（客户实锤）。侧栏在所有页
   *  都在，把升级入口放这里保证「随时点得到」。 */
  hasUpdate?: boolean;
  latestVersion?: string;
  onUpdate?: () => void;
  updating?: boolean;
  updatePct?: number | null;
  /** 这台机器自动升级到这一版**失败过几次**（后端本地账本）。≥1 就换条路走：
   *  不再劝「再点一次一键升级」（同一条路上的失败源基本都不是暂时性的），
   *  直接推「下载官网安装包覆盖安装」。 */
  updateFailed?: number;
  /** 上次失败原因（一句话，给用户一个交代，也方便客服）。 */
  updateFailReason?: string;
  /** 下载安装包并打开（覆盖安装）。 */
  onReinstall?: () => void;
  /** 缩到托盘。**只在矮屏由 App 传进来** —— 那时 App 撤掉了自定义标题栏（那条只为放这一个
   *  按钮却吃 36px，而 1366×768 客户区才 688px）。不矮的机器上标题栏还在，这里就不传，
   *  免得同一个动作在界面上有两个入口。 */
  onHideToTray?: () => void;
  // dockApps / onLaunchDock 仍由 App 传入但侧栏 Dock 已移除，故此处不再解构（保留可选以免 App 改动）
  dockApps?: DockApp[];
  onLaunchDock?: (a: DockApp) => void;
}) {
  const { t, lang, setLang } = useI18n();
  /** 矮屏（1366×768 笔记本客户区 ≈ 696px）：这一栏装不下自己。见下方 `short &&` 各处。 */
  const { short } = useViewport();
  // 「AI 优化大师」：Windows 走 ukrt.exe 薄壳，macOS 走原生 macopt（lib.rs 路由，同一份 UI）。
  // 两个平台都放出；其它平台（linux 等）暂无实现 → 仍隐藏，免得点开报错。
  const moreItems =
    platform && platform !== "windows" && platform !== "macos"
      ? MORE.filter((m) => m.id !== "airuntime")
      : MORE;

  // 「更多」默认折叠；若当前页在「更多」组里则自动展开，保证高亮可见。
  const activeInMore = moreItems.some((m) => m.id === active);
  const [moreOpen, setMoreOpen] = useState(activeInMore);
  const showMore = moreOpen || activeInMore;

  // 实验室同理：默认折叠，但当前页就在里面时必须自动展开 ——
  // 否则用户从别处（首页卡片、深链）跳进某个实验功能，侧栏会一个高亮都没有，像迷路。
  const activeInLab = LAB.some((m) => m.id === active);
  const [labOpen, setLabOpen] = useState(activeInLab);
  const showLab = labOpen || activeInLab;

  // 侧栏折叠：收成窄图标条，把横向空间全让给右侧（终端编程时尤其需要）。本地持久化（纯视图偏好）。
  const [collapsed, setCollapsed] = useState<boolean>(
    () => localStorage.getItem("uking.sidebar.collapsed") === "1",
  );
  /**
   * 🔴 **「进 U-Workspace 自动收成图标条」已删**（客户 2026-08-19：
   * 「点击 U-Workspace 默认收起侧栏这个动作去掉，需要用户自己点击才收缩，
   * 点击还是比较突兀，默认就这个界面，让用户自己点击收缩，好统一一点」）。
   *
   * 当初的理由（左边两栏在 1440 宽上吃掉 31%）没有错，但它替用户做了主：
   * 同一个「点侧栏条目」的动作，点别处什么都不动、点这一条侧栏当场缩掉一半 ——
   * 于是切页面这件小事有了两种表现，而用户并不知道界线在哪。省下的宽度是真的，
   * 突兀也是真的，客户选了后者。**要窄自己点，那一下点完还会记住（`collapsed` 偏好）。**
   *
   * 让步链（窗口太窄自动让位给终端）**保留** —— 那是窗口尺寸逼出来的、拉宽会自己还原，
   * 跟「你点了个导航它就变形」不是一回事。
   */
  // 🔴 **主侧栏只认用户自己点的偏好，不参与让步链**（2026-08-19，客户第二次提）。
  //    上一轮删掉的是「进 U-Workspace 自动收起」这个**明确实现**，但让步链的 level 2
  //    换了条路做出了一模一样的事：终端只活在 U-Workspace 里、一进去就窄 → 触发让步 → 侧栏收。
  //    ★ 删实现不等于删行为。客户裁决：「一概不碰主侧栏，人手工点好点。」
  //    让步链现在封顶在 1 级（只收会话栏），见 `yieldChain.ts::YIELD_MAX`。
  const railed = collapsed;
  const toggleCollapsed = () => {
    setCollapsed((c) => {
      const n = !c;
      try { localStorage.setItem("uking.sidebar.collapsed", n ? "1" : "0"); } catch { /* ignore */ }
      return n;
    });
  };

  // compact=true：只显标题不显小字说明（用于「更多」进阶项，去掉两行密度、更清爽）。
  // 核心 ①②③ funnel 仍带说明（小白引导刚需）……**但矮屏例外**：1366×768 上带说明的
  // 四个核心项 + 展开的「更多」根本装不下，侧栏自己长出滚动条（客户截图实证）。
  // 说明文字不是丢掉而是挪进 tooltip —— 空间不够时宁可少显示，不能少了那句话。
  const NavButton = (n: NavItem, compact = false) => {
    const on = active === n.id;
    const Icon = n.icon;
    return (
      <button
        key={n.id}
        onClick={() => onSelect(n.id)}
        // 副标题被藏起来时才挂 title：本来就看得见的东西再悬停一遍是噪声
        title={compact ? `${t(n.label)} · ${t(n.sub)}` : undefined}
        className={cn(
          "w-full flex items-center gap-3 px-3 rounded-card text-left transition-all border-l-2",
          compact ? (short ? "py-1.5" : "py-2") : "py-2.5",
          on
            ? "bg-accent/[0.12] border-accent text-ink-0"
            : "border-transparent text-ink-3 hover:bg-white/[0.035] hover:text-ink-1",
        )}
      >
        <Icon size={16} className={on ? "text-accent" : "text-ink-4"} />
        <div className="min-w-0">
          <div className={cn("text-[13px]", on ? "font-semibold" : "font-medium")}>{t(n.label)}</div>
          {!compact && <div className="text-[10px] text-ink-4 truncate">{t(n.sub)}</div>}
        </div>
      </button>
    );
  };

  // 折叠态：窄图标条 —— 只留 Logo + 核心 ①②③ 等图标 + 「更多」（点它展开侧栏）+ 主题/展开。
  // 把整条侧栏从 208px 压到 52px，右侧终端/工作区拿到最大空间。
  if (railed) {
    const RailButton = (n: NavItem) => {
      const on = active === n.id;
      const Icon = n.icon;
      return (
        <button
          key={n.id}
          onClick={() => onSelect(n.id)}
          title={`${t(n.label)} · ${t(n.sub)}`}
          className={cn(
            "w-10 h-10 grid place-items-center rounded-card border-l-2 transition-all",
            on
              ? "bg-accent/[0.14] border-accent text-accent shadow-sm"
              : "border-transparent text-ink-4 hover:bg-white/[0.04] hover:text-ink-1",
          )}
        >
          <Icon size={18} />
        </button>
      );
    };
    return (
      <aside className="w-[52px] shrink-0 flex flex-col items-center border-r border-white/[0.06] bg-bg-1">
        <button
          onClick={toggleCollapsed}
          // 这里只可能是用户自己收的（主侧栏不再参与让步链），所以不需要再解释「为什么它自己合上了」。
          title={t("展开侧栏")}
          className="h-14 w-full grid place-items-center border-b border-white/[0.06] group"
        >
          <span className="group-hover:hidden"><Logo size={24} /></span>
          <PanelLeftOpen size={18} className="hidden group-hover:block text-ink-2" />
        </button>
        <nav className="flex-1 overflow-y-auto py-3 flex flex-col items-center gap-1 w-full">
          {CORE.map(RailButton)}
          <button
            onClick={toggleCollapsed}
            title={t("展开侧栏，查看「更多」")}
            className="w-10 h-10 grid place-items-center rounded-card text-ink-4 hover:bg-white/[0.04] hover:text-ink-1"
          >
            <MoreHorizontal size={18} />
          </button>
        </nav>
        <div className="pb-3 flex flex-col items-center gap-1 w-full">
          {hasUpdate && (
            <button
              onClick={updateFailed > 0 ? onReinstall : onUpdate}
              disabled={updating}
              title={
                updating
                  ? t("正在升级…")
                  : updateFailed > 0
                    ? t("自动升级失败过 {n} 次 —— 点此下载官网安装包覆盖安装", { n: updateFailed })
                    : t("有新版 v{ver}，点此升级", { ver: latestVersion ?? "" })
              }
              className={cn(
                "w-10 h-10 grid place-items-center rounded-card transition-colors disabled:opacity-60",
                updateFailed > 0
                  ? "bg-amber-500/[0.12] text-amber-500 hover:bg-amber-500/[0.22]"
                  : "bg-accent/[0.12] text-accent hover:bg-accent/[0.22]",
              )}
            >
              <ArrowUpCircle size={18} className={updating ? "animate-pulse" : ""} />
            </button>
          )}
          <button
            onClick={() => onSelect("feedback")}
            title={t("技术支持 · 报告问题 · 加微信找我们")}
            className={cn(
              "w-10 h-10 grid place-items-center rounded-card transition-colors",
              active === "feedback"
                ? "bg-accent/[0.14] text-accent"
                : "text-ink-4 hover:bg-white/[0.04] hover:text-ink-1",
            )}
          >
            <LifeBuoy size={18} />
          </button>
          <button
            onClick={() => setLang(lang === "en" ? "zh" : "en")}
            title={`${t("语言")}: ${lang === "en" ? "English" : "中文"}`}
            className="w-10 h-10 grid place-items-center rounded-card text-ink-3 hover:bg-white/[0.04] hover:text-ink-1"
          >
            <span className="text-[10px] font-semibold">{lang === "en" ? "EN" : "中"}</span>
          </button>
          <button
            onClick={onToggleTheme}
            title={theme === "dark" ? t("浅色模式") : t("深色模式")}
            className="w-10 h-10 grid place-items-center rounded-card text-ink-3 hover:bg-white/[0.04] hover:text-ink-1"
          >
            {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
          </button>
          {/* 矮屏才有：自定义标题栏被撤，这是「缩到托盘」唯一的界面入口（见 onHideToTray 注释）。
              收起态也必须放 —— 否则客户一收侧栏，这个功能就在界面上消失了。 */}
          {onHideToTray && (
            <button
              onClick={onHideToTray}
              title={t("隐藏到右下角托盘")}
              className="w-10 h-10 grid place-items-center rounded-card text-ink-3 hover:bg-white/[0.04] hover:text-ink-1"
            >
              <PanelTopClose size={16} />
            </button>
          )}
        </div>
      </aside>
    );
  }

  return (
    <aside className="w-[208px] shrink-0 flex flex-col border-r border-white/[0.06] bg-bg-1">
      {/* 品牌头（标题栏已有「U-King AI 管家」，这里 Logo + 名 + 一句 slogan，跟下方 NAV 双行排版统一）。
          矮屏收成单行：slogan 在原生标题栏「U-King AI 管家」旁边已经是第三次说同一件事，
          而那 12px 在 696px 的预算里是真的要用的。 */}
      <div className={cn("flex items-center gap-2.5 px-4 border-b border-white/[0.06]", short ? "h-11" : "h-14")}>
        <Logo size={short ? 22 : 28} />
        <div className="min-w-0 leading-tight flex-1">
          <div className="text-[14px] font-semibold text-ink-0">U-King</div>
          {!short && <div className="text-[10px] text-ink-4 truncate">{t("一键装好你的全部 AI")}</div>}
        </div>
        <button
          onClick={toggleCollapsed}
          title={t("收起侧栏（腾出空间给右侧终端/工作区）")}
          className="shrink-0 inline-flex items-center justify-center w-7 h-7 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06] transition-colors"
        >
          <PanelLeftClose size={15} />
        </button>
      </div>

      {/* 导航 —— 核心三步置顶醒目，其余收进「更多」折叠组。
          外面这层 relative 只为托住底部那条渐隐：19 个导航项在 1366×768 / 1280×640 上装不下
          （既有行为，两个折叠组都展开时 1920 也滚），而**滚动条不出现在最后一行被切的地方** ——
          客户看到的是「实验室」被拦腰截断，像渲染坏了，而不是「下面还有」。 */}
      <div className="relative flex-1 min-h-0 flex flex-col">
      <nav className={cn("flex-1 overflow-y-auto px-2.5", short ? "py-1.5 space-y-0.5" : "py-3 space-y-1")}>
        {/* 矮屏下核心项也走 compact（不显副标题）—— 说明进了 tooltip，见 NavButton 注释 */}
        {CORE.map((n) => NavButton(n, short))}

        {/* 「更多」折叠组：进阶/工具页，默认收起，避免小白眼花 */}
        <div className={cn("border-t border-white/[0.06]", short ? "pt-1 mt-0.5" : "pt-2 mt-1")} />
        <button
          onClick={() => setMoreOpen((v) => !v)}
          className={cn(
            "w-full flex items-center gap-3 px-3 rounded-card text-left transition-colors",
            short ? "py-1.5" : "py-2",
            showMore ? "bg-white/[0.03] text-ink-1" : "text-ink-3 hover:bg-white/[0.03] hover:text-ink-1",
          )}
        >
          <MoreHorizontal size={16} className={showMore ? "text-accent" : "text-ink-4"} />
          <span className="text-[12px] font-medium flex-1">{t("更多")}</span>
          <ChevronDown
            size={14}
            className={cn("text-ink-4 transition-transform", showMore && "rotate-180")}
          />
        </button>
        {showMore && moreItems.map((m) => NavButton(m, true))}

        {/* 实验室折叠组：没毕业的功能。和「更多」分开，是因为「更多」里的是**成熟但低频**，
            这里的是**还不稳**——两者混在一起，用户就没法判断该不该信任一个功能。 */}
        <div className={cn("border-t border-white/[0.06]", short ? "pt-1 mt-0.5" : "pt-2 mt-1")} />
        <button
          onClick={() => setLabOpen((v) => !v)}
          className={cn(
            "w-full flex items-center gap-3 px-3 rounded-card text-left transition-colors",
            short ? "py-1.5" : "py-2",
            showLab ? "bg-white/[0.03] text-ink-1" : "text-ink-3 hover:bg-white/[0.03] hover:text-ink-1",
          )}
        >
          <FlaskConical size={16} className={showLab ? "text-amber-400" : "text-ink-4"} />
          <span className="text-[12px] font-medium flex-1">{t("实验室")}</span>
          <span className="text-[9.5px] px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-400">
            {t("测试中")}
          </span>
          <ChevronDown
            size={14}
            className={cn("text-ink-4 transition-transform", showLab && "rotate-180")}
          />
        </button>
        {showLab && (
          <>
            {/* 矮屏只收紧行距，**不删这句** —— 它是「别拿这些当主力」的风险告知，
                空间紧不是少说风险的理由（组头那个「测试中」徽章说不到「别当主力」这层）。
                🔴 2026-08-18 从 ink-5 改成 ink-3：实测浅色主题下 ink-5 只有 **1.48:1**，
                那一档是「几乎看不见」。上面这段注释一直在论证「必须保留这句」，而样式
                让它没人读得到 —— **留着但看不见 ≡ 删了，只是自己不知道**。留一句风险告知
                和让人真的读到，是两件事，前者不蕴含后者。 */}
            <p className={cn("px-3 text-[10.5px] text-ink-3", short ? "py-0.5 leading-snug" : "py-1.5 leading-relaxed")}>
              {t("这些还没做完，可能不稳定、可能随时改。能用，但别当主力。")}
            </p>
            {LAB.map((m) => NavButton(m, true))}
          </>
        )}

        {/* 小程序（用户自己装的）。放最后、且**装了才出现** —— 上面三组是舞台基建
            （编译进 exe，删不掉），这一区是演员（能装能删）。分开是为了让用户敢删：
            混在一起他没法判断「删了会不会把 U-King 弄坏」。 */}
        <SidebarMiniApps compact={short} />
      </nav>
        {/* 装得下时它盖在 bg-1 上、从 bg-1 渐隐到透明 = 看不见；装不下时才显出「下面还有」。
            纯 CSS，不测滚动高度 —— 少一个会跟折叠/窗口尺寸不同步的 state。 */}
        {/* 渐到 `bg-1/0` 而不是 `transparent` —— 同一个颜色只改透明度，任何浏览器都不会
            在中间插出一条灰边（`transparent` 在老的插值实现里是「透明的黑」）。 */}
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-5 bg-gradient-to-t from-bg-1 to-bg-1/0" />
      </div>

      {/* 「我的 AI」侧栏 Dock 已移除 —— 这些工具（Codex桌面版/OpenClaw桌面版/Claude CLI/OpenClaw龙虾/
          Hermes）右侧主区「我装好的 AI 工具」卡片里已经有了，侧栏再列一遍是重复，让小白眼花。
          连同「终端」一起去掉，侧栏更清爽。dockApps/onLaunchDock 仍由 App 传入（暂留），要恢复
          解开下面注释即可。 */}
      {/* <div className="px-2.5 pt-2 pb-1 border-t border-white/[0.06] overflow-y-auto">
        <div className="text-[10px] text-ink-5 px-1 pb-1.5">我的 AI</div>
        {(["desktop", "cli"] as const).map((g) => {
          const items = dockApps.filter((a) => (a.group ?? "cli") === g);
          if (items.length === 0) return null;
          return (
            <div key={g} className="mb-1.5">
              <div className="text-[10px] text-ink-6 px-1 pb-0.5">
                {g === "desktop" ? "桌面应用" : "命令行工具"}
              </div>
              <div className="space-y-0.5">
                {items.map((a) => {
                  const isOpenCodex = a.kind === "workbench";
                  const isView = a.kind === "workbench" || a.kind === "tui";
                  const on =
                    (a.kind === "workbench" && active === "workbench") ||
                    (a.kind === "tui" && active === a.tabId);
                  return (
                    <button
                      key={a.id}
                      onClick={() => onLaunchDock(a)}
                      title={isView ? `打开 ${a.name}` : `启动 ${a.name}`}
                      className={cn(
                        "w-full flex items-center gap-2.5 px-2 py-1.5 rounded-card text-left transition-colors border-l-2",
                        on
                          ? "bg-accent/[0.14] border-accent text-ink-0"
                          : "border-transparent text-ink-2 hover:bg-white/[0.04]",
                      )}
                    >
                      {isOpenCodex ? (
                        <span className="w-6 h-6 rounded-md bg-accent/[0.18] flex items-center justify-center shrink-0">
                          <LayoutGrid size={14} className="text-accent-400" />
                        </span>
                      ) : (
                        <ToolIcon tool={a.tool} size={22} active={a.active} className="w-6 h-6" />
                      )}
                      <span className={cn("text-[13px] truncate", isView ? "font-semibold" : "font-medium")}>
                        {a.name}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div> */}

      {/* 终端页入口已隐藏 —— 现在点工具卡「打开终端」直接弹独立系统终端窗口（term_open_external），
          内嵌终端整页对小白多余且易混淆（两个"终端"）。代码全留着，要恢复把下面这块解开注释即可。 */}
      {/* <div className="px-2.5 pt-1 pb-2">
        <button
          onClick={() => onSelect("terminal")}
          className={cn(
            "w-full flex items-center gap-3 px-3 py-2 rounded-card text-left transition-colors border-l-2",
            active === "terminal"
              ? "bg-accent/[0.10] border-accent text-ink-0"
              : "border-transparent text-ink-2 hover:bg-white/[0.03]",
          )}
        >
          <TerminalIcon size={16} className={active === "terminal" ? "text-accent" : "text-ink-3"} />
          <div className="text-[13px] font-medium">终端</div>
        </button>
      </div> */}

      {/* 有新版：常驻升级入口。升级横幅只在管家页 StatusLine 露出，用户长期待在 U-Workspace
          就永远看不到升级（客户实锤「没有升级按钮」）。侧栏在所有页都在，放这里保证随时点得到。 */}
      {hasUpdate && (
        <div className="px-2.5 pb-1">
          {/* ★ 自动升级失败过就换条路：主按钮改成「下载安装包重装」。
              别再劝「再试一次」—— 自动替换失败的原因（杀软锁 exe、目录不可写、路径含中文、
              替换脚本被拦）几乎都不是暂时性的，同一条路重试只是重复同一次失败，
              而客户的体感就是「老是有新版本，就是升不上去」。 */}
          <button
            onClick={updateFailed > 0 ? onReinstall : onUpdate}
            disabled={updating}
            title={
              updating
                ? t("正在升级…")
                : updateFailed > 0
                  ? t("自动升级失败过 {n} 次，改用官网安装包覆盖安装（配置不会丢）", { n: updateFailed })
                  : t("升级到 v{ver}", { ver: latestVersion ?? "" })
            }
            className={cn(
              "w-full flex items-center gap-3 px-3 py-2.5 rounded-card text-left transition-colors disabled:opacity-60 border",
              updateFailed > 0
                ? "bg-amber-500/[0.12] text-amber-500 hover:bg-amber-500/[0.2] border-amber-500/25"
                : "bg-accent/[0.12] text-accent hover:bg-accent/[0.2] border-accent/20",
            )}
          >
            <ArrowUpCircle size={16} className={cn("shrink-0", updating && "animate-pulse")} />
            {/* 版本号不挤在这一行 —— 侧栏窄（实测 ~205px）时「有新版 · 一键升级」+ v0.9.82
                放不下，标题会被拦腰折成两行。版本号挪到下面那行，反而更像一句话。 */}
            <div className="text-[13px] font-semibold whitespace-nowrap">
              {updating
                ? updatePct != null
                  ? t("升级中 {pct}%", { pct: updatePct })
                  : t("升级中…")
                : updateFailed > 0
                  ? t("下载安装包重装")
                  : t("有新版 · 一键升级")}
            </div>
          </button>
          {/* 失败了要说清楚「为什么」和「会不会丢东西」——不说，客户只会以为软件坏了。 */}
          {updateFailed > 0 && !updating && (
            <div className="mt-1 px-1 text-[11px] leading-snug text-ink-4">
              {t("自动升级没换成功（{why}）。装新版不会丢配置和对话。", {
                why: updateFailReason || t("原因未知"),
              })}
            </div>
          )}
          {/* 「升不升」是个决定，得先让人看到**升了有什么用**。这条信息一直存在
              （服务器 version.json 的 notes/history），但唯一入口是页脚那个 10px 的版本号，
              没人会想到去点它 —— 等于有等于没有。放在按钮正下方，看完再决定。 */}
          {!updating && (
            <button
              onClick={() => onShowChangelog?.()}
              className="w-full mt-1 py-0.5 text-[11px] text-ink-4 hover:text-accent transition-colors"
            >
              {latestVersion ? t("看看 v{ver} 改了什么", { ver: latestVersion }) : t("看看这版改了什么")}
            </button>
          )}
        </div>
      )}

      {/* 技术支持 —— 常驻底部（全页可达），把"报告问题/联系作者"放在最好找的位置（客户要求）。
          归属"页脚工具组"：顶部分隔线把它和上面的导航/升级分开，样式跟下方语言/主题/官网 一致
          的卡片底色（不再用 nav 的 border-l-2 —— 那会让它像一条漂在页脚上的游离导航项，客户反馈「突兀」）。 */}
      <div className={cn("px-2.5 border-t border-white/[0.06]", short ? "mt-0.5 pt-1 pb-0.5" : "mt-1 pt-2 pb-1")}>
        <button
          onClick={() => onSelect("feedback")}
          className={cn(
            "w-full flex items-center gap-2.5 px-3 rounded-card text-left transition-colors",
            short ? "py-1.5" : "py-2",
            active === "feedback"
              ? "bg-accent/[0.12] text-accent"
              : "bg-white/[0.02] text-ink-2 hover:bg-accent/[0.08] hover:text-ink-0",
          )}
          title={t("技术支持 · 报告问题 · 加微信找我们")}
        >
          <LifeBuoy size={15} className={active === "feedback" ? "text-accent" : "text-ink-4"} />
          <div className="text-[12px] font-medium">{t("技术支持")}</div>
        </button>
      </div>

      {/* 语言 + 主题合并成一行：原来是两整行（各 py-2）太占纵向空间（客户反馈「堆一块、占地方太多」）。
          合并后省一行高度，又保留标签+分段控件的可发现性（不退回当年那个没人发现的 24px 页脚小图标）。 */}
      <div className="px-2.5 pb-1 flex items-center gap-2">
        {/* 语言分段 */}
        <div className="flex items-center gap-1.5 flex-1 min-w-0 px-2 py-1.5 rounded-card bg-white/[0.02] text-ink-2">
          <Languages size={15} className="text-ink-4 shrink-0" />
          <div className="flex items-center gap-0.5 rounded-md bg-white/[0.04] p-0.5">
            {(["zh", "en"] as const).map((l) => (
              <button
                key={l}
                onClick={() => setLang(l)}
                title={t("语言")}
                className={cn(
                  "px-1.5 py-0.5 rounded text-[11px] font-medium transition-colors",
                  lang === l ? "bg-accent/[0.18] text-accent" : "text-ink-4 hover:text-ink-1",
                )}
              >
                {l === "zh" ? "中" : "EN"}
              </button>
            ))}
          </div>
        </div>
        {/* 主题切换：图标+当前状态短标签，tooltip 说明点了会切到哪。 */}
        <button
          onClick={onToggleTheme}
          title={theme === "dark" ? t("浅色模式") : t("深色模式")}
          className="flex items-center gap-1.5 shrink-0 px-2.5 py-1.5 rounded-card bg-white/[0.02] text-ink-3 hover:bg-accent/[0.08] hover:text-ink-0 transition-colors"
        >
          {theme === "dark" ? (
            <Moon size={15} className="text-ink-4" />
          ) : (
            <Sun size={15} className="text-ink-4" />
          )}
          <span className="text-[11px] font-medium">{theme === "dark" ? t("夜间开") : t("白天")}</span>
        </button>
        {/* 矮屏才有：自定义标题栏被撤，这是「缩到托盘」唯一的界面入口（见 onHideToTray 注释）。
            只放图标不放字 —— 这一行已经有语言和主题，再挤一个带标签的按钮会把它们压换行，
            那就把省下来的 36px 又还回去了。 */}
        {onHideToTray && (
          <button
            onClick={onHideToTray}
            title={t("隐藏到右下角托盘")}
            className="flex items-center shrink-0 px-2 py-1.5 rounded-card bg-white/[0.02] text-ink-3 hover:bg-accent/[0.08] hover:text-ink-0 transition-colors"
          >
            <PanelTopClose size={15} />
          </button>
        )}
      </div>

      {/* 版本 + 官网（极简，只放官网一个入口，方便联系/找下载/充值） */}
      <div className={cn("px-4 border-t border-white/[0.06] flex items-center justify-between gap-2 bg-white/[0.01]", short ? "py-1.5" : "py-3")}>
        {/* 不挂 data-action-id：打开更新日志面板是**界面动作**，按宪法不进动作核心
            （切标签/弹面板那一类）。挂了反而会让绑定核对指向一个不存在的动作。 */}
        {/* 加个图标 —— 光一串 10px 的灰色版本号，没人看得出它是可点的。 */}
        <button
          onClick={() => onShowChangelog?.()}
          className="flex items-center gap-1 text-[10px] text-ink-5 hover:text-accent transition-colors"
          title={t("查看更新日志")}
        >
          <History size={11} />
          <span className="font-mono">v{version}</span>
        </button>
        <button
          onClick={() => onOpenSite?.()}
          className="flex items-center gap-1 text-[10px] text-ink-4 hover:text-accent transition-colors"
          title={t("打开官网")}
        >
          <Globe size={11} /> {t("官网")}
        </button>
      </div>
    </aside>
  );
}
