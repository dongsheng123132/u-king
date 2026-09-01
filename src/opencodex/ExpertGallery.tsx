/**
 * AI 专家墙 —— **一份实现，两个地方用**（宪法第 12 条：公共能力复用不复制）。
 *
 *  1. 侧栏「AI 专家」独立页（`src/Experts.tsx`，外面套技能市场入口）
 *  2. U-Workspace 左栏点「AI 专家」滑出的面板（`UWorkspace.tsx`）—— 这是主场：
 *     挑完专家**当场就在工作台开会话**，不用先跳出去再跳回来。
 *
 * 布局借鉴 WorkBuddy 的专家页：搜索 → 精选场景（按 scene 分组的成组卡）→ 分类 chips → 专家卡片墙。
 * 数据源仍是 `experts.ts` 的 EXPERTS（**不在这里复制一份专家定义**）。
 */
import { useMemo, useState, useEffect } from "react";
import { ChevronRight, ExternalLink, Search, Sparkles, Store, Wrench, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";
import { invoke } from "@tauri-apps/api/core";
import { ACTION, createTauriActionClient } from "../generated/action-client";
import { EXPERTS, allExperts, loadHiredExperts, skillLabel, type Expert, type HiredMeta } from "./experts";
import { SkillPackList } from "../components/SkillPackList";
import { HireSearch } from "./HireSearch";

/** 走通用影核通道，不为「招人」再开一个 tauri command。 */
const callAction = createTauriActionClient(invoke, {
  command: "action_parity_call",
  requestArgument: "request",
  surface: "desktop",
});

/** 按 id 稳定生成的双色渐变。**必须是纯函数** —— 用随机数的话每次渲染都换一张脸。 */
function avatarBg(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) % 360;
  return `linear-gradient(135deg, hsl(${h} 68% 62%), hsl(${(h + 42) % 360} 70% 52%))`;
}

export function ExpertGallery({
  onSummon,
  onToast,
  dense = false,
}: {
  onSummon: (e: Expert) => void;
  /** 装/删技能包的轻提示。不传则静默。 */
  onToast?: (s: string) => void;
  /** 嵌在工作台里时更紧凑（列窄、留白小）。 */
  dense?: boolean;
}) {
  const { t: tr } = useI18n();
  const [cat, setCat] = useState("全部");
  const [q, setQ] = useState("");
  const [detail, setDetail] = useState<Expert | null>(null);
  // 招进来的人异步进来。**内置那批先渲染，不等它**（数量以 EXPERTS 为准，别在注释里抄
  // 一个会过期的数字 —— 上一版写死「11 位」，加了装机医生就成了错的）—— 招人目录读不了
  // 不该让整页空白（同 disposeTerm 的取舍：旁路失败不许带走主干）。
  const [roster, setRoster] = useState<(Expert & HiredMeta)[]>(EXPERTS);
  useEffect(() => {
    let alive = true;
    void loadHiredExperts(async () => {
      const env = await callAction(ACTION.RUNTIME_EXPERT_INSPECT, {});
      return env.ok ? ((env.result as { packs?: [] }).packs ?? []) : [];
    }).then(() => {
      if (alive) setRoster(allExperts());
    });
    return () => {
      alive = false;
    };
  }, []);

  /** 正在解聘谁（按钮转圈用）。 */
  const [firing, setFiring] = useState<string | null>(null);
  /**
   * 解聘一个招进来的专家：删 `~/.uking/experts/<id>/`，然后**重新扫一遍**目录。
   * 不做本地乐观删除 —— 磁盘才是真相源，删失败了列表还留着才是对的
   * （比「界面上没了、下次开又回来」强）。
   */
  const dismiss = async (e: Expert) => {
    setFiring(e.id);
    try {
      // `confirmed` 走**第三个参数**（信封层），不是入参 —— 写动作的确认在核心里强制，
      // 塞进 input 会被 `additionalProperties:false` 判成 invalid_input。
      await callAction(ACTION.RUNTIME_EXPERT_DISMISS, { id: e.id }, { confirmed: true });
      await loadHiredExperts(
        async () => {
          const env = await callAction(ACTION.RUNTIME_EXPERT_INSPECT, {});
          return env.ok ? ((env.result as { packs?: [] }).packs ?? []) : [];
        },
        true, // force：刚删完，缓存必须作废
      );
      setRoster(allExperts());
      setDetail(null);
    } finally {
      setFiring(null);
    }
  };

  const cats = useMemo(() => ["全部", ...Array.from(new Set(roster.map((e) => e.category)))], [roster]);

  /** 精选场景：按 scene 把专家成组摆出来（WorkBuddy 那排大卡的做法）。 */
  const scenes = useMemo(() => {
    const m = new Map<string, Expert[]>();
    for (const e of roster) m.set(e.scene, [...(m.get(e.scene) ?? []), e]);
    return Array.from(m.entries());
  }, [roster]);

  // 搜索优先于分类：正在搜的时候还按分类过滤，会出现「明明有这个专家却搜不到」
  const list = useMemo(() => {
    const kw = q.trim().toLowerCase();
    if (kw) {
      return roster.filter((e) =>
        [e.name, e.role, e.tagline, e.desc, ...e.tags].some((s) => s.toLowerCase().includes(kw)),
      );
    }
    return cat === "全部" ? roster : roster.filter((e) => e.category === cat);
  }, [q, cat, roster]);

  const Card = (e: Expert) => (
    // 外层从 <button> 改成 div：卡片里要再放一个「召唤」按钮，button 套 button 是非法 HTML
    // （浏览器会把内层拆出去，点击行为变得没法预测）。role/tabIndex/键盘保留可达性。
    <div
      key={e.id}
      role="button"
      tabIndex={0}
      onClick={() => setDetail(e)}
      onKeyDown={(ev) => {
        if (ev.key === "Enter" || ev.key === " ") {
          ev.preventDefault();
          setDetail(e);
        }
      }}
      className="group relative text-left rounded-card border border-white/[0.06] bg-bg-2/70 hover:border-accent/40 hover:bg-bg-2 p-3.5 transition-colors cursor-pointer"
    >
      {/* 直接召唤（借 WorkBuddy 卡片右上那颗）：以前必须先点开详情弹窗才能召唤 ——
          对已经知道自己要谁的人，那一步纯属挡路。想先看看的照旧点卡片进详情。
          焦点可见（focus-visible）不能只靠 hover：键盘用户 hover 不出来。 */}
      <button
        onClick={(ev) => {
          ev.stopPropagation(); // 别顺带把详情弹窗也开了
          onSummon(e);
        }}
        className="absolute right-2.5 top-2.5 z-10 h-6 px-2 rounded-md bg-accent text-white text-[11px] opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-opacity hover:bg-accent-600"
      >
        {e.route ? tr("打开") : tr("召唤")}
      </button>
      <div className="flex items-center gap-2.5 mb-1.5">
        {/* 头像（2026-08-18 客户：「AI 专家里边多弄一点图片，参考 workbuddy」）。
            🔴 **不引图片资源** —— WorkBuddy 每位专家一张真人照，我们上这个要么打包进 exe
            （体积预算已经红着）、要么运行时去拉（多一条会挂的网络依赖，还把「谁的脸」这种
            事交给了服务器）。改用**按 id 稳定生成的双色渐变 + emoji**：
            一样是彩色圆头像、一眼能区分、认得住，而代价是 0 字节。
            色相取自 id 的字符和 —— 同一个专家在任何机器上永远同一个颜色（不能用随机数，
            那样每次渲染都换脸）。 */}
        <span
          className="grid place-items-center w-11 h-11 rounded-full text-[22px] leading-none shrink-0 ring-1 ring-white/[0.10]"
          style={{ background: avatarBg(e.id) }}
        >
          {e.emoji}
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold text-ink-0 truncate">{tr(e.name)}</div>
          {/* 职称 · 署名（借 WorkBuddy 的专家卡）。署名让它像个人而不是一张功能卡 ——
              「网站设计专家」记不住，「小站」记得住。没署名的就只显示职称，不留空占位。 */}
          <div className="text-[11px] text-ink-4 truncate">
            {tr(e.role)}
            {e.byline && <span className="text-ink-5"> · {tr(e.byline)}</span>}
            {/* 来源可见：招进来的人带标记，内置的没有 —— 用户永远看得出眼前这位是哪来的。 */}
            {(e as HiredMeta).hired && (
              <span className="ml-1 px-1 rounded bg-accent/15 text-accent">{tr("已招")}</span>
            )}
            {/* 依赖前置检查：缺技能包在卡片上就说，别等召唤之后才失败（readiness 原则）。 */}
            {!!(e as HiredMeta).missingSkills?.length && (
              <span className="ml-1 px-1 rounded bg-amber-500/15 text-amber-400">{tr("缺技能包")}</span>
            )}
            {/* 缺外部命令同样前置报。**只报不装** —— 专家包不许执行任意命令。 */}
            {!!(e as HiredMeta).missingTools?.length && (
              <span className="ml-1 px-1 rounded bg-amber-500/15 text-amber-400">{tr("缺工具")}</span>
            )}
          </div>
        </div>
        <ChevronRight size={14} className="shrink-0 text-ink-5 opacity-0 group-hover:opacity-100 transition-opacity" />
      </div>
      <p className="text-[11.5px] text-ink-3 leading-relaxed line-clamp-2">{tr(e.desc)}</p>
      <div className="flex flex-wrap gap-1 mt-2">
        {e.tags.slice(0, 3).map((t) => (
          <span key={t} className="text-[10px] px-1.5 py-0.5 rounded bg-white/[0.05] text-ink-4">
            {tr(t)}
          </span>
        ))}
      </div>
      {/* 自带技能（测试报告 #018）：技能是这个专家真会干的活，不该只活在给模型看的系统提示里。
          用 accent 色和上面的「擅长领域」标签区分开 —— 那是形容词，这是真本事。 */}
      {e.skills.length > 0 && (
        <div className="flex flex-wrap items-center gap-1 mt-1.5 pt-1.5 border-t border-white/[0.05]">
          <Wrench size={10} className="text-accent/70 shrink-0" />
          {e.skills.map((s) => (
            <span
              key={s}
              title={tr(skillLabel(s).what)}
              className="text-[10px] px-1.5 py-0.5 rounded bg-accent/[0.12] text-accent"
            >
              {tr(skillLabel(s).name)}
            </span>
          ))}
        </div>
      )}
    </div>
  );

  return (
    <div className="space-y-4">
      {/* 技能市场入口。**外链只留一条小的** —— 主位给下面那个「去市场找人」的现搜框：
          一个跳出去的按钮只能让人离开，现搜能直接告诉他生态里此刻有什么
          （`hire.rs` 那句「技能市场是一张会漂的地图，现搜才是看地形」）。 */}
      <button
        onClick={() => void openUrl("https://skillhub.cn").catch(() => {})}
        className="w-full text-left rounded-card border border-white/[0.10] px-3 py-1.5 flex items-center gap-2 hover:border-accent/40 transition-colors"
      >
        <Store size={13} className="text-ink-4 shrink-0" />
        <span className="text-[11.5px] text-ink-3 flex-1 min-w-0 truncate">{tr("也可以去 skillhub.cn 逛")}</span>
        <ExternalLink size={11} className="text-ink-5 shrink-0" />
      </button>

      {/* 搜索：专家一多就得靠它（WorkBuddy 右上角那个框） */}
      <div className="relative">
        <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-ink-5 pointer-events-none" />
        <input
          value={q}
          onChange={(ev) => setQ(ev.target.value)}
          placeholder={tr("搜专家名称、职称或擅长的活")}
          className="w-full h-9 pl-9 pr-8 rounded-card bg-bg-1 border border-white/[0.08] text-[12.5px] text-ink-1 placeholder:text-ink-5 outline-none focus:border-accent/40"
        />
        {q && (
          <button
            onClick={() => setQ("")}
            className="absolute right-2 top-1/2 -translate-y-1/2 w-5 h-5 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
          >
            <X size={12} />
          </button>
        )}
      </div>

      {/* 精选场景：一个场景一张卡，卡里直接列这个场景下的专家（点进去就是详情） */}
      {!q && scenes.length > 0 && (
        <div>
          <div className="text-[12px] text-ink-3 mb-2 flex items-center gap-1.5">
            <Sparkles size={13} className="text-accent" /> {tr("精选场景")}
          </div>
          <div className={cn("grid gap-3", dense ? "grid-cols-1 lg:grid-cols-2" : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3")}>
            {scenes.map(([scene, members]) => (
              <div key={scene} className="rounded-card border border-white/[0.06] bg-gradient-to-br from-accent/[0.08] to-transparent p-3.5">
                <div className="text-[13px] font-semibold text-ink-0 mb-2">{tr(scene)}</div>
                <div className="space-y-0.5">
                  {members.map((e) => (
                    <button
                      key={e.id}
                      onClick={() => setDetail(e)}
                      className="w-full flex items-center gap-2 px-1.5 py-1 rounded text-left hover:bg-white/[0.05]"
                    >
                      <span className="text-[15px] leading-none shrink-0">{e.emoji}</span>
                      <span className="text-[12px] text-ink-2 truncate">{tr(e.name)}</span>
                      {e.hot && (
                        <span className="ml-auto text-[9.5px] px-1 py-px rounded bg-accent/20 text-accent-400 shrink-0">
                          {tr("热门")}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      <div>
        {!q && (
          <div className="flex items-center gap-1.5 mb-3 flex-wrap">
            {cats.map((c) => (
              <button
                key={c}
                onClick={() => setCat(c)}
                className={cn(
                  "h-7 px-3 rounded-full text-[12px] transition-colors",
                  cat === c ? "bg-accent text-white" : "bg-bg-1 border border-white/[0.08] text-ink-3 hover:text-ink-1",
                )}
              >
                {tr(c)}
              </button>
            ))}
          </div>
        )}
        {list.length === 0 ? (
          <div className="py-10 text-center text-[12.5px] text-ink-4">{tr("没搜到匹配的专家，换个词试试")}</div>
        ) : (
          <div className={cn("grid gap-3", dense ? "grid-cols-1 lg:grid-cols-2" : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3")}>
            {list.map(Card)}
          </div>
        )}
      </div>

      {/* 技能包清单 —— 从被删掉的「AI 技能」页搬过来。
          专家是**人**，技能包是**这些人会的本事**：摆在同一屏才说得通
          （用户 2026-08-18：「ai技能 删除吧，就是一个 skillhub，ai专家，不就是吗？合并留到 uchat」）。 */}
      {/* 去市场现搜可招的人（动态，不是我们维护的货架）——用户要的「动态让大家装」 */}
      <HireSearch onToast={onToast} />

      <SkillPackList onToast={onToast ?? (() => {})} />

      {detail && (
        <div className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4" onClick={() => setDetail(null)}>
          <div
            className="w-full max-w-md rounded-card border border-white/[0.10] bg-bg-2 shadow-card p-5"
            onClick={(ev) => ev.stopPropagation()}
          >
            <div className="flex items-start gap-3">
              <span className="text-[34px] leading-none">{detail.emoji}</span>
              <div className="flex-1 min-w-0">
                <div className="text-[15px] font-semibold text-ink-0">{tr(detail.name)}</div>
                <div className="text-[12px] text-ink-4">
                  {tr(detail.role)}
                  {detail.byline && <span className="text-ink-5"> · {tr(detail.byline)}</span>}
                </div>
              </div>
              <button
                onClick={() => setDetail(null)}
                className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
              >
                <X size={16} />
              </button>
            </div>
            <div className="mt-4">
              <div className="text-[11px] text-ink-5 mb-1">{tr("能力介绍")}</div>
              <p className="text-[12.5px] text-ink-2 leading-relaxed">{tr(detail.desc)}</p>
            </div>
            <div className="mt-3">
              <div className="text-[11px] text-ink-5 mb-1.5">{tr("擅长领域")}</div>
              <div className="flex flex-wrap gap-1.5">
                {detail.tags.map((t) => (
                  <span key={t} className="text-[11px] px-2 py-0.5 rounded bg-white/[0.05] text-ink-3">
                    {tr(t)}
                  </span>
                ))}
              </div>
            </div>
            {/* 自带技能（测试报告 #018「AI技能与专家组分离」）：把技能**在专家详情里说清楚**，
                包括它具体能产出什么。客户判断「召唤谁」靠的就是这一段，不是那两行 desc 文案。 */}
            {detail.skills.length > 0 && (
              <div className="mt-3">
                <div className="text-[11px] text-ink-5 mb-1.5">{tr("自带技能 · 召唤后即可用")}</div>
                <div className="space-y-1.5">
                  {detail.skills.map((s) => {
                    const sk = skillLabel(s);
                    return (
                      <div key={s} className="flex items-start gap-2 px-3 py-2 rounded-lg bg-accent/[0.07] border border-accent/20">
                        <Wrench size={13} className="text-accent shrink-0 mt-0.5" />
                        <div className="min-w-0">
                          <div className="text-[12px] text-ink-1">{tr(sk.name)}</div>
                          {sk.what && <div className="text-[11px] text-ink-4 leading-relaxed">{tr(sk.what)}</div>}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
            <div className="mt-3">
              <div className="text-[11px] text-ink-5 mb-1.5">{tr("试试这样问我")}</div>
              <div className="space-y-1.5">
                {detail.quickPrompts.map((qp) => (
                  <div
                    key={qp.label}
                    className="flex items-center gap-2 px-3 py-2 rounded-lg bg-bg-1 border border-white/[0.06] text-[12px] text-ink-2"
                  >
                    <ChevronRight size={13} className="text-ink-5 shrink-0" />
                    <span className="truncate">{tr(qp.template)}</span>
                  </div>
                ))}
              </div>
            </div>
            <button
              onClick={() => {
                onSummon(detail);
                setDetail(null);
              }}
              className="mt-5 w-full h-10 rounded-lg bg-accent text-white text-[13px] font-medium hover:bg-accent-600"
            >
              {detail.route
                ? tr("打开{name}", { name: tr(detail.name) })
                : tr("召唤 {role} → 开一个会话", { role: tr(detail.role) })}
            </button>
            {/* 解聘 —— **只对招进来的人显示**。
                内置那 11 位是代码里的常量，磁盘上根本没有它们的文件夹，给个辞退按钮
                点了也只会返回 false，等于摆一个假开关（客户 2026-08-18 抱怨的正是
                「有但不管用」那一类）。招得进来就得辞得掉，否则那不叫舞台，叫住进来了。 */}
            {(detail as HiredMeta).hired && (
              <button
                onClick={() => void dismiss(detail)}
                disabled={firing === detail.id}
                data-action-id="runtime.expert.dismiss"
                className="mt-2 w-full h-9 rounded-lg border border-red-400/30 text-red-400 text-[12px] hover:bg-red-400/10 disabled:opacity-50"
              >
                {firing === detail.id ? tr("正在解聘…") : tr("解聘（删掉这个专家包）")}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
