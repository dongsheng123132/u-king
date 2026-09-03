/**
 * DSH 插件 —— 一键打开 DeepSeek Harness，并把插件装进去。
 *
 * 为什么是这一页而不是「小程序市场」（2026-08-18 定，理由写在 `docs/需求榜.md` E-2）：
 * 客户自己给了判据 ——「有什么不是 AI 对话不能搞定的呢？除了小游戏……他自己做一个到桌面
 * 不行吗？」顺着筛，小程序真正成立的只有「输入/输出不是语言」那一类，品类窄到撑不起一个市场。
 * **而 DSH 那边已经有供给侧**：我们内置了 DSH，装机清单里本来就在跑
 * `dsh plugin --profile web add …` 给它装我们自己的两个插件。
 * 所以「插件生态」该投的是**帮用户往 DSH 里装**，不是自己开一个空货架。
 *
 * 🔴 **不叫「DSH 小程序」**：小程序跑在我们进程里、受限 web 包、我们负责安全；
 * DSH 插件跑在 DSH 进程里、全特权 Node、DSH 负责。名字混一起会让人以为能互换。
 * 侧栏那一区仍叫「小程序」（就是本机这几个），这一页单独叫「DSH 插件」。
 *
 * 精选清单**故意很短**：我们只放自己跑过的。其余交给下面那条动态搜
 * （`runtime.hire.search` 的 `keywords:dsh-plugin`）—— 同 `hire.rs` 那条边界：
 * **技能市场是一张会漂的地图，现搜才是看地形**。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Blocks, Bot, ExternalLink, Loader2, Play, Puzzle } from "lucide-react";
import { ACTION, createTauriActionClient } from "./generated/action-client";
import { HireSearch } from "./opencodex/HireSearch";
import { useI18n } from "./i18n";
import { cn } from "./lib/cn";

const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

/**
 * 🔴 **不做「精选清单」**（2026-08-18 用户否掉我的第一版）。
 *
 * 我第一版把**我们自己写的那两个插件**摆成主推。用户原话：「我的插件一般般，
 * 我写的插件一帮帮，不要做主推」「github 上 awesome dsh 插件的仓库有知名的，
 * 你用他们的更好一些」。他说得对，而且理由比「谦虚」硬：
 * **我们没有能力维护一份 DSH 插件的评审清单** —— 那要持续跟进上游、逐个验、逐个更新。
 * 摆一份我们不维护的货架，跟自建一个没人上架的市场是同一个毛病。
 *
 * 社区那份有真实信号（star 数、每日自动抓取 + 人工核实），而且**一直有人维护**。
 * 我们该做的是**把人送过去 + 把装的那一步变简单**，不是替他们重排一遍。
 *
 * 下面这两个是我们装机时**确实装过**的，所以列出来 —— 标的是「U-King 自带」这个事实，
 * 不是「推荐」这个评价。事实我们担得起，评价担不起。
 */
const BUNDLED: { spec: string; name: string; what: string; profile?: string }[] = [
  {
    spec: "github:dongsheng123132/dsh-cache-prefix",
    name: "缓存前缀稳定",
    what: "让 DSH Web 的请求前缀稳定，缓存命中率更高。装机时默认装了这个。",
  },
  {
    spec: "github:dongsheng123132/dsh-terminal",
    name: "持续对话终端",
    what: "终端里连续对话不掉上下文，另带缓存统计。装机时默认装了这个。",
    profile: "terminal",
  },
];

/** 社区维护的插件清单。**只放一个** —— 放三个「精选仓库」等于又开始替人排序了。 */
const AWESOME = {
  url: "https://github.com/awesome-dsh-plugin/awesome-dsh-plugin",
  name: "awesome-dsh-plugin",
  what: "社区维护的 DSH 插件清单（8.6k star，每日自动抓取 + 人工核实）。挑好之后，复制它的仓库地址回来装。",
};

/**
 * 「让 AI 帮你挑」的动态提示词 —— 把「去哪找、怎么挑、怎么装」一次讲清，交给 AI 去现搜现推荐。
 *
 * 为什么这条是「动态」的而不是一份精选清单：技能市场是一张会漂的地图，现搜才是看地形
 * （同 hire.rs 那条边界）。这份提示词给的是**去哪找**，不是**有什么** —— 让 AI 自己去现看。
 *
 * 🔴 只推荐不装：装外部插件是不可撤回的动作（DSH 插件 = 进程内全权限 Node 代码），
 * 装不装由人在 DSH 里签字。提示词里明确写着「只推荐，不要真的装」。
 */
const HIRE_PROMPT =
  "帮我在 DSH（DeepSeek Harness）的插件生态里挑几个真正好用的插件。\n\n" +
  "去哪找（按顺序）：\n" +
  "1. 跑本机只读搜索（不会装任何东西）：\n" +
  "   u-king-mini action run runtime.hire.search --input '{\"query\":\"keywords:dsh-plugin\"}' --json\n" +
  "   （Windows 上命令是 u-king-mini.exe；找不到就先看 ~/.uking/llms.txt）\n" +
  "2. GitHub 社区清单 github.com/awesome-dsh-plugin/awesome-dsh-plugin（8.6k star，每日自动抓取 + 人工核实）\n" +
  "3. npm 直接搜 npmjs.com/search?q=dsh-plugin\n\n" +
  "怎么挑：周下载量 / star 高的优先；描述能看出「解决真问题」的才推；刚出、没人用的别推。\n\n" +
  "输出 3~5 个：插件名 + 它解决什么 + 怎么装（在 DSH 里跑 dsh plugin --profile web add <仓库或包名>）。\n" +
  "只推荐，不要真的装。";

/** 卡片上的几条起手词（照 AI 专家 quickPrompts 的形状）—— 点一条开一个对话，AI 带着提示词去找。 */
const HIRE_QUICK: { label: string; prompt: string }[] = [
  { label: "挑几个好用的", prompt: HIRE_PROMPT },
  { label: "找作图/视频的", prompt: HIRE_PROMPT + "\n重点找作图、视频、海报、浏览器操作这一类。" },
  { label: "找办公/微信/飞书的", prompt: HIRE_PROMPT + "\n重点找微信、公众号、飞书、办公自动化这一类。" },
];

export function DshPlugins({ onToast, onGoInstall, onGoDsh, onGoChat }: {
  onToast: (s: string) => void;
  onGoInstall: () => void;
  /** 去 DSH 专属页（`setTab("dsh")`）—— 打开 DSH 的**唯一**实现在那儿，本页只负责导航。 */
  onGoDsh: () => void;
  /** 把一条「动态提示词」交出去 —— 宿主负责开对话并把它发给 AI（像 AI 专家的召唤）。 */
  onGoChat?: (prompt: string) => void;
}) {
  const { t } = useI18n();
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    // 装没装：问工具清单，不自己猜。没装时整页的动作都换成「先去装」。
    invoke<{ id: string; installed: boolean }[]>("list_tools")
      .then((ts) => setInstalled(!!ts.find((x) => x.id === "dsh")?.installed))
      .catch(() => setInstalled(null));
  }, []);

  const openWeb = useCallback(() => {
    if (installed === false) {
      onGoInstall();
      return;
    }
    // 🔴 DSH **不走 `launch_app`**（我第一版就是这么写的，真机点下去报
    //    `invalid args 'app' for command 'launch_app'`）：参数名是 `app` 不是 `id`，且更根本的是
    //    DSH 压根不是「一个可执行文件」—— 它的 `launch_cmd` 是 `dsh web`（起个本地服务），
    //    跟 ClawX 那种 .exe 不同类。
    //
    // 🔴 **也不走 `term_open_external("dsh web")`**（第二版这么写的，测试机 pc-*** 实锤翻车）。
    //    我当时注释里写「复用『我的 AI』里同一条路」—— 那句是错的，「我的 AI」走的是
    //    `setTab("dsh")` → ToolAppView。外部终端那条只弹个黑窗把 server 跑起来，`dsh web`
    //    自己**没有开浏览器这一步**（`dsh web --help` 里连 `--open` 都没有），于是：
    //      · 服务确实起来了（127.0.0.1:3080 HTTP 200），客户却只看见一屏日志 → 判成「没打开」；
    //      · toast 还说「正在打开 Web 工作台…」→ 报告是对的，世界是坏的；
    //      · 没有端口复用闸，再点一次就是 `EADDRINUSE 127.0.0.1:3080` 当场崩一个新黑窗。
    //    App.tsx:693 那行注释早写明「不能走通用外部终端」，这里正是踩了它。
    //
    // 现在只做导航：等端口、复用已有实例、显示内嵌工作台全在 `ToolAppView.launchDshWebUI` 里，
    // 那是「打开 DSH」的唯一实现（宪法 13）。落地页还让客户在 Web / 终端两种模式里自己选 ——
    // 从这页直接替他挑一种，等于替他做了主。
    onGoDsh();
  }, [installed, onGoInstall, onGoDsh]);

  const install = useCallback(
    (spec: string, profile?: string) => {
      setBusy(spec);
      void callAction(ACTION.RUNTIME_DSH_PLUGIN_INSTALL, { spec, profile: profile ?? "web" }, { confirmed: true })
        .then((env) => {
          if (!env.ok) throw new Error(env.error?.message ?? "failed");
          onToast(t("装好了 —— 回 DSH 里就能用"));
        })
        .catch((e) => onToast(String(e)))
        .finally(() => setBusy(null));
    },
    [onToast, t],
  );

  return (
    <div className="space-y-5 pb-4">
      {/* ① 一键打开 —— 客户原话：「上面是一键打开 dsh，下面是安装插件」 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-1/60 px-5 py-4">
        <div className="flex items-center gap-2.5">
          <span className="grid place-items-center w-9 h-9 rounded-lg bg-accent/[0.12] text-accent shrink-0">
            <Blocks size={18} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="text-[14px] font-semibold text-ink-0">{t("DeepSeek Harness")}</h2>
            <p className="text-[11.5px] text-ink-3 mt-0.5">
              {t("DeepSeek 官方的 AI 工作台，我们已内置。默认接好虾盘云，打开就能用。")}
            </p>
          </div>
          <button
            onClick={openWeb}
            className="shrink-0 inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600"
          >
            <Play size={13} />
            {/* 🔴 这颗按钮说的都是**真话**：没装就送你去装机；装了就送你去 DSH 页 ——
                它自己不启动任何东西，所以也不该出现 spinner（旧版那个转圈只是在转，
                背后弹的黑窗永远不会变成一个打开的工作台）。 */}
            {installed === false ? t("先去装 DSH") : t("打开 DSH")}
          </button>
        </div>
      </section>

      {/* ①½ 让 AI 帮你挑 —— 「动态提示词」那一半。现搜那半（HireSearch）给的是名字+怎么装，
          这半把「去哪找、怎么挑」交给一个真的 AI 去执行：开对话 → 提示词喂进去 →
          AI 去 GitHub / npm 现找现推荐。像 AI 专家的召唤，不做第二份精选清单。 */}
      {onGoChat && (
        <section className="rounded-card border border-accent/25 bg-accent/[0.06] px-5 py-4">
          <div className="flex items-center gap-2.5">
            <span className="grid place-items-center w-9 h-9 rounded-lg bg-accent/[0.12] text-accent shrink-0">
              <Bot size={18} />
            </span>
            <div className="min-w-0 flex-1">
              <h3 className="text-[13px] font-semibold text-ink-1">{t("让 AI 帮你挑插件")}</h3>
              <p className="text-[11.5px] text-ink-3 mt-0.5">
                {t("开一个对话，AI 按提示词去 GitHub / npm 现找现推荐 —— 提示词里写清了去哪找、怎么挑、怎么装。只推荐不装。")}
              </p>
            </div>
          </div>
          <div className="mt-3 flex items-center gap-1.5 flex-wrap">
            {HIRE_QUICK.map((p) => (
              <button
                key={p.label}
                onClick={() => onGoChat(p.prompt)}
                className="h-7 px-3 rounded-full bg-accent/[0.14] border border-accent/30 text-[11.5px] text-accent hover:bg-accent/[0.22]"
              >
                {t(p.label)}
              </button>
            ))}
          </div>
        </section>
      )}

      {/* ② 插件：精选 + 动态搜 */}
      <section className="space-y-2">
        <div className="flex items-baseline gap-2">
          <h3 className="text-[13px] font-semibold text-ink-1">{t("插件")}</h3>
          <span className="text-[11px] text-ink-3">{t("装进 DSH，不是装进 U-King")}</span>
        </div>

        {/* 去社区那份清单挑 —— 我们不维护评审清单（维护不动，也就不该假装在维护）。 */}
        <button
          onClick={() => void openUrl(AWESOME.url).catch(() => {})}
          className="w-full text-left rounded-card border border-accent/25 bg-accent/[0.06] px-4 py-3 hover:border-accent/45"
        >
          <div className="flex items-center gap-2">
            <ExternalLink size={14} className="shrink-0 text-accent" />
            <span className="text-[12.5px] font-medium text-ink-1">{t("去社区清单挑插件")}</span>
            <span className="text-[10px] text-ink-3 font-mono truncate">{AWESOME.name}</span>
          </div>
          <p className="text-[11px] text-ink-3 mt-1 leading-relaxed">{t(AWESOME.what)}</p>
        </button>

        <div className="text-[11px] text-ink-3 pt-1">{t("U-King 装机时自带的两个：")}</div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
          {BUNDLED.map((p) => (
            <div key={p.spec} className="rounded-card border border-white/[0.06] bg-bg-1/40 px-4 py-3">
              <div className="flex items-start gap-2.5">
                <Puzzle size={15} className="shrink-0 mt-0.5 text-accent/80" />
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] font-medium text-ink-1">{t(p.name)}</div>
                  <p className="text-[11px] text-ink-3 mt-0.5 leading-relaxed">{t(p.what)}</p>
                  <div className="text-[10px] text-ink-4 mt-1 font-mono truncate">{p.spec}</div>
                </div>
              </div>
              <div className="mt-2.5 flex items-center gap-1.5">
                <button
                  onClick={() => install(p.spec, p.profile)}
                  disabled={!!busy || installed === false}
                  title={installed === false ? t("先装 DSH，再装它的插件") : undefined}
                  className={cn(
                    "inline-flex items-center gap-1 h-7 px-2.5 rounded-md text-[11.5px]",
                    "bg-accent/15 border border-accent/30 text-accent hover:bg-accent/25 disabled:opacity-40",
                  )}
                >
                  {busy === p.spec ? <Loader2 size={12} className="animate-spin" /> : null}
                  {t("装到 DSH")}
                </button>
                <button
                  onClick={() => void openUrl(p.spec.replace(/^github:/, "https://github.com/")).catch(() => {})}
                  className="inline-flex items-center gap-1 h-7 px-2 rounded-md border border-white/[0.10] text-[11px] text-ink-3 hover:text-ink-0"
                >
                  <ExternalLink size={11} /> {t("看源码")}
                </button>
              </div>
            </div>
          ))}
        </div>

        {/* 动态那半 —— 复用「去市场找人」那条（它搜的就是 npm / DSH 插件 / 技能包）。
            🔴 只搜不装：这里给的是名字 + 怎么装，装外部包由人签字（同 hire.rs 的边界）。
            defaultQuery：打开页面自动跑 `keywords:dsh-plugin`，不然这页默认只有一个空搜索框。 */}
        <HireSearch onToast={onToast} defaultQuery="keywords:dsh-plugin" />
      </section>
    </div>
  );
}
