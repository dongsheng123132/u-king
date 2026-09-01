/**
 * 任务护照（长程任务状态）—— **一件事做到哪了**，以及把它交给下一个 AI 接着干。
 *
 * ## 为什么它从看板里搬出来单开一页
 * 会话看板回答「**谁在跑**」，护照回答「**事情做到哪**」。这是两个生命周期：
 * 会话是「做一件事然后结束」，护照是「这件事跨会话、跨 AI、跨天地活着」。
 * 以前护照是看板顶上一条横向滚动的窄条 —— 一个 230px 宽的卡片装不下目标、
 * 状态、已验证事实和下一步，于是护照最值钱的东西（**为什么这么定、还差什么**）
 * 一个字都露不出来，客户只能看见一串 id。摆在别人家的页眉上，它就永远只能是装饰。
 *
 * ## 三条自律
 * 1. **读不到 ≠ 没有**。inspect 失败时必须说「没读到」并给重试，
 *    绝不画成空列表 —— 一个「你还没有任务护照」的空态会让人去新建第二张，
 *    而原来那张其实好好躺在盘上。这条是本仓库反复在修的「统计者谎报」的同一形状。
 * 2. **交接必须有落点和回执**。见 `handoff.ts`：建/复用哪个会话、发了什么、
 *    对方签没签收，三件事都摆出来。旧实现只写剪贴板，三件事一件都答不了。
 * 3. **正文来自后端**（`origin.rs::compile_context`）。前端不自己拼第二份「护照大意」——
 *    拼了就会跟真状态漂开，而漂开的那次正好是出事那次。
 */
import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowRight,
  Check,
  ChevronLeft,
  ClipboardCopy,
  Loader2,
  RefreshCw,
  Send,
} from "lucide-react";
import { ToolIcon } from "../components/ToolIcon";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";
import { ACTION, createTauriActionClient } from "../generated/action-client";
import {
  buildHandoffPrompt,
  deliveredPassport,
  onHandoffChange,
  type Handoff,
} from "./handoff";
import type { Engine } from "./types";

const actionClient = createTauriActionClient(
  (command, args) => invoke(command, args),
  { surface: "gui:task-passport-board" },
);

/** 护照正文。形状对齐 `origin.rs::TaskOrigin`（后端序列化的全量对象）。 */
export type TaskPassport = {
  id: string;
  title: string;
  goal: string;
  version: number;
  harness: string;
  scope: string;
  updated_at: string;
  current_state: string;
  verification: string;
  next_steps: string[];
  facts: { claim: string; source: string; verified: boolean; when: string }[];
  decisions: { what: string; why: string; when: string }[];
  artifacts: string[];
  /** `compiled: true` 时后端附带的上下文块 —— 交接时原样发给接手方。 */
  compiled_context?: string;
};

/**
 * 可接手的大脑。**刻意与 Chat 顶栏那个下拉同一套 `Engine`** ——
 * 「交给另一个 AI」如果自造一份目标清单，用户就会看到两份不一致的「有哪些 AI」。
 *
 * `blocked` = 装了也不能自动交接，并且**必须说出为什么**。
 * Hermes 的对话真身是一个 TUI 终端（`Chat.tsx` 里 engine=hermes 渲染的是 TermPanel），
 * 往 TUI 里灌一段带换行的长状态在本项目是**已知会碎的**（0.9.83 那个 P0 就是批处理壳
 * 拒绝含换行的参数）。宁可在这儿画成灰的并写明原因，也不能发出去再画一个「已送达」——
 * 那就成了这次要修的那个病本身：把不知道画成成功。
 */
const TARGETS: { id: Engine; label: string; icon: string; why: string; blocked?: string }[] = [
  { id: "claude", label: "Claude Code", icon: "claude", why: "长程改代码最稳，技能包最全" },
  { id: "codex", label: "Codex", icon: "openai", why: "换个脑子看同一个问题" },
  { id: "uking", label: "U-King 轻助手", icon: "uking", why: "轻快，作图/短问答" },
  {
    id: "hermes",
    label: "Hermes",
    icon: "hermes",
    why: "省 token，适合体力活",
    blocked: "它的对话是终端 TUI，多行状态送不进去 —— 请复制护照号手动交接",
  },
];

/** 这台机器上装没装（`uking` 是我们自家云直连，永远可用）。 */
type Availability = Partial<Record<Engine, boolean>>;

/** 交接的落点 + 回执。`sessionId` 有值 = 会话真的建出来了。 */
type Landing = {
  passportId: string;
  engine: Engine;
  sessionId: string;
  sessionName: string;
  dir: string;
};

export function PassportBoard({ onHandoff, onOpenSession, fallbackDir }: {
  /**
   * 把一封信投给一个会话。宿主负责建/复用会话（它才拥有 store），
   * 回传落到哪个会话 —— 本组件不碰 store，只负责把落点如实画出来。
   */
  onHandoff: (dir: string, h: Handoff) => Promise<{ sessionId: string; sessionName: string }>;
  /** 点「打开会话」：切到那个会话。 */
  onOpenSession: (id: string) => void;
  /** 护照没写 scope 时的兜底工作目录（当前活动会话的目录）。仍可能为空。 */
  fallbackDir?: string;
}) {
  const { t: tr } = useI18n();
  const [passports, setPassports] = useState<TaskPassport[]>([]);
  /** `null` = 还没读完；字符串 = 这次**读失败了**（跟「读到了 0 张」是两件事）。 */
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [openId, setOpenId] = useState<string | null>(null);
  const [pickerFor, setPickerFor] = useState<string | null>(null);
  const [sending, setSending] = useState<string | null>(null);
  const [landings, setLandings] = useState<Record<string, Landing>>({});
  const [avail, setAvail] = useState<Availability>({ uking: true });
  const [copied, setCopied] = useState<string | null>(null);

  // 回执变化（会话签收了）→ 重画。订阅的是 handoff 模块，不是我们自己的猜测。
  // 返回值不参与渲染：它的作用就是「变了就重画」，签收状态由 `isDelivered()` 现读。
  // 快照是字符串，React 按值比较，同一状态重复渲染不会打转。
  useSyncExternalStore(onHandoffChange, () => deliveredSnapshot(landings));

  const load = useCallback(() => {
    setLoading(true);
    return actionClient(ACTION.RUNTIME_ORIGIN_INSPECT, { compiled: true })
      .then((envelope) => {
        if (!envelope.ok) throw new Error(envelope.error.message);
        const result = envelope.result as { tasks?: TaskPassport[] };
        setPassports(Array.isArray(result.tasks) ? result.tasks : []);
        setLoadError(null);
      })
      .catch((e) => {
        // 🔴 **不清空 passports**：上一次读到的仍然是这台机器上真实存在过的护照。
        // 把它抹成空列表，等于用一次读失败注销掉用户的任务。
        setLoadError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    let alive = true;
    const run = () => { if (alive) void load(); };
    run();
    const id = setInterval(run, 30_000);
    return () => { alive = false; clearInterval(id); };
  }, [load]);

  // 这台机器上哪些大脑真装了。装没装是**事实**，只能问后端。
  useEffect(() => {
    let alive = true;
    type Stack = Record<string, { found?: boolean } | undefined>;
    void Promise.all([
      invoke<Stack>("detect_stack").catch(() => ({}) as Stack),
      invoke<{ hermes_installed?: boolean }>("hermes_browser_status").catch(
        () => ({}) as { hermes_installed?: boolean },
      ),
    ]).then(([stack, hermes]) => {
      if (!alive) return;
      setAvail({
        uking: true,
        claude: !!stack?.claude?.found,
        codex: !!stack?.codex?.found,
        hermes: !!hermes?.hermes_installed,
      });
    });
    return () => { alive = false; };
  }, []);

  const open = useMemo(
    () => passports.find((p) => p.id === openId) ?? null,
    [passports, openId],
  );

  const doHandoff = useCallback(async (passport: TaskPassport, engine: Engine) => {
    setPickerFor(null);
    setSending(passport.id);
    try {
      // 工作目录：护照自己的 scope 优先；没有就用当前会话的；再没有才问用户。
      // **不许静默挑一个** —— 把任务丢进一个用户没想到的文件夹，比不交接更糟。
      let dir = passport.scope?.trim() || fallbackDir?.trim() || "";
      if (!dir) {
        const picked = await openDialog({
          directory: true,
          multiple: false,
          title: tr("这张护照没写工作目录 —— 选一个让接手方在哪儿干活"),
        });
        if (typeof picked !== "string" || !picked) return; // 用户取消 = 什么都没发生
        dir = picked;
      }
      const prompt = buildHandoffPrompt(passport.id, passport.compiled_context ?? null);
      const { sessionId, sessionName } = await onHandoff(dir, {
        passportId: passport.id,
        engine,
        prompt,
      });
      setLandings((m) => ({
        ...m,
        [passport.id]: { passportId: passport.id, engine, sessionId, sessionName, dir },
      }));
    } catch (e) {
      setLandings((m) => {
        const next = { ...m };
        delete next[passport.id];
        return next;
      });
      // 失败就说失败。旧实现在这里退化成「复制提示词」，把一次失败画成了一次成功。
      setLoadError(tr("交接失败：{e}", { e: e instanceof Error ? e.message : String(e) }));
    } finally {
      setSending(null);
    }
  }, [fallbackDir, onHandoff, tr]);

  const copyId = useCallback(async (id: string) => {
    try {
      await navigator.clipboard.writeText(id);
      setCopied(id);
      window.setTimeout(() => setCopied((c) => (c === id ? null : c)), 1500);
    } catch {
      /* 剪贴板被系统策略禁用：护照号本来就明文摆在卡片上，用户可手动选中复制 */
    }
  }, []);

  // ---------- 详情页 ----------
  if (open) {
    return (
      <PassportDetail
        passport={open}
        landing={landings[open.id] ?? null}
        delivered={isDelivered(landings[open.id])}
        avail={avail}
        sending={sending === open.id}
        onBack={() => setOpenId(null)}
        onHandoff={(engine) => void doHandoff(open, engine)}
        onOpenSession={onOpenSession}
        onCopyId={() => void copyId(open.id)}
        copied={copied === open.id}
        tr={tr}
      />
    );
  }

  // ---------- 列表页 ----------
  return (
    <div className="h-full flex flex-col bg-bg-2">
      <header className="flex items-start gap-3 px-5 pt-4 pb-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold text-ink-0">{tr("任务护照")}</h2>
          <p className="text-[12px] text-ink-4 mt-0.5">
            {tr("一件事做到哪了，以及交给下一个 AI 接着干 —— 只传已验证事实，不传聊天记录。")}
          </p>
        </div>
        <button
          onClick={() => void load()}
          disabled={loading}
          title={tr("重新读一遍护照")}
          className="ml-auto shrink-0 p-1.5 rounded-lg border border-white/[0.08] text-ink-4 hover:text-ink-2 hover:border-white/[0.16] transition-colors disabled:opacity-50"
        >
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
        </button>
      </header>

      {/* 🔴 读失败必须自己占一条，且**不吃掉下面已读到的护照** ——
          「这次没读到」和「你没有护照」是两句完全不同的话。 */}
      {loadError && (
        <div className="mx-5 mb-3 flex items-start gap-2 rounded-lg border border-danger-500/30 bg-danger-500/[0.07] px-3 py-2">
          <AlertTriangle size={13} className="text-danger-500 shrink-0 mt-0.5" />
          <div className="min-w-0 text-[11.5px] text-ink-2">
            <div>{tr("这次没能读到任务护照 —— 下面显示的是上一次读到的，可能已经过期。")}</div>
            <div className="text-ink-4 mt-0.5 break-all">{loadError}</div>
          </div>
          <button
            onClick={() => void load()}
            className="ml-auto shrink-0 text-[11.5px] text-accent-400 hover:text-accent-300"
          >
            {tr("重试")}
          </button>
        </div>
      )}

      <div className="flex-1 min-h-0 overflow-y-auto px-5 pb-5">
        {loading && passports.length === 0 && !loadError ? (
          <div className="py-10 text-center text-[12px] text-ink-4">
            <Loader2 size={15} className="animate-spin inline mr-1.5" />
            {tr("正在读任务护照…")}
          </div>
        ) : passports.length === 0 && !loadError ? (
          <div className="rounded-xl border border-dashed border-white/[0.10] px-4 py-8 text-center">
            <div className="text-[12.5px] text-ink-3">{tr("还没有任务护照。")}</div>
            <div className="text-[11.5px] text-ink-4 mt-1.5">
              {tr("对任意已接入 U-King 的 AI 说一句：为当前目标创建一张任务护照。")}
            </div>
          </div>
        ) : (
          <div className="grid gap-2.5 grid-cols-1 lg:grid-cols-2">
            {passports.map((p) => (
              <PassportCard
                key={p.id}
                passport={p}
                landing={landings[p.id] ?? null}
                delivered={isDelivered(landings[p.id])}
                sending={sending === p.id}
                pickerOpen={pickerFor === p.id}
                avail={avail}
                onOpen={() => setOpenId(p.id)}
                onTogglePicker={() => setPickerFor((c) => (c === p.id ? null : p.id))}
                onPick={(engine) => void doHandoff(p, engine)}
                onOpenSession={onOpenSession}
                tr={tr}
              />
            ))}
          </div>
        )}
      </div>

      <footer className="px-5 py-2.5 text-[11px] text-ink-4 leading-relaxed border-t border-white/[0.05]">
        {tr(
          "护照存在 ~/.uking/origin/，不是聊天记录：换个 AI、换台会话、隔几天回来，接手方读到的是同一份「世界此刻是什么样」。",
        )}
      </footer>
    </div>
  );
}

/** 交接过、且对方签收了。没签收就画「投递中」，不许提前说成功。 */
function isDelivered(l: Landing | null | undefined): boolean {
  if (!l) return false;
  return deliveredPassport(l.sessionId) === l.passportId;
}

/** 给 useSyncExternalStore 的快照：回执集合变了就重画。 */
function deliveredSnapshot(landings: Record<string, Landing>): string {
  return Object.values(landings)
    .map((l) => `${l.sessionId}:${deliveredPassport(l.sessionId) ?? ""}`)
    .join("|");
}

function PassportCard({
  passport: p, landing, delivered, sending, pickerOpen, avail,
  onOpen, onTogglePicker, onPick, onOpenSession, tr,
}: {
  passport: TaskPassport;
  landing: Landing | null;
  delivered: boolean;
  sending: boolean;
  pickerOpen: boolean;
  avail: Availability;
  onOpen: () => void;
  onTogglePicker: () => void;
  onPick: (engine: Engine) => void;
  onOpenSession: (id: string) => void;
  tr: (s: string, v?: Record<string, string>) => string;
}) {
  return (
    <article className="relative rounded-xl border border-white/[0.08] bg-bg-1 hover:border-white/[0.16] transition-colors">
      {/* 整张卡可点进详情 —— 客户反馈的第一条就是「卡片点不动」。
          交接按钮在上面单独一层，stopPropagation 免得点交接顺手翻页。 */}
      <button onClick={onOpen} className="w-full text-left px-3.5 py-3">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[13px] font-medium text-ink-0 truncate flex-1">
            {p.title || p.id}
          </span>
          <ArrowRight size={13} className="text-ink-5 shrink-0" />
        </div>
        <div className="mt-1 text-[11px] font-mono text-accent-300">
          {p.id} · v{p.version}
        </div>
        <div className="mt-1.5 text-[12px] text-ink-2 line-clamp-2 leading-snug">
          {p.goal || tr("（这张护照没写目标）")}
        </div>
        <div className="mt-1.5 flex items-center gap-2 text-[10.5px] text-ink-4">
          <span>{p.harness ? tr("上次：{h}", { h: p.harness }) : tr("尚未标记接手方")}</span>
          <span>·</span>
          <span>{tr("{n} 条已验证事实", { n: String(p.facts?.filter((f) => f.verified).length ?? 0) })}</span>
          <span>·</span>
          <span>{tr("{n} 步待办", { n: String(p.next_steps?.length ?? 0) })}</span>
        </div>
      </button>

      <div className="px-3.5 pb-3 -mt-1">
        {landing ? (
          <LandingLine landing={landing} delivered={delivered} onOpenSession={onOpenSession} tr={tr} />
        ) : (
          <button
            onClick={(e) => { e.stopPropagation(); onTogglePicker(); }}
            disabled={sending}
            data-testid={`passport-handoff-${p.id}`}
            className="inline-flex items-center gap-1.5 rounded-md border border-white/[0.10] px-2 py-1 text-[11px] text-ink-3 hover:text-accent-300 hover:border-accent/40 transition-colors disabled:opacity-50"
          >
            {sending ? <Loader2 size={11} className="animate-spin" /> : <Send size={11} />}
            {tr(sending ? "正在交接…" : "交给另一个 AI")}
          </button>
        )}
      </div>

      {pickerOpen && (
        <TargetPicker avail={avail} onPick={onPick} onClose={onTogglePicker} tr={tr} />
      )}
    </article>
  );
}

/** 落点那一行：**交给谁、落到哪个会话、签收没有**。三样缺一样这条就白写了。 */
function LandingLine({ landing, delivered, onOpenSession, tr }: {
  landing: Landing;
  delivered: boolean;
  onOpenSession: (id: string) => void;
  tr: (s: string, v?: Record<string, string>) => string;
}) {
  const label = TARGETS.find((x) => x.id === landing.engine)?.label ?? landing.engine;
  return (
    <div className="flex items-center gap-2 rounded-lg border border-accent/25 bg-accent/[0.06] px-2.5 py-1.5">
      {delivered ? (
        <Check size={12} className="text-accent-400 shrink-0" />
      ) : (
        <Loader2 size={12} className="text-ink-4 shrink-0 animate-spin" />
      )}
      <div className="min-w-0 text-[11px] text-ink-2 truncate">
        {delivered
          ? tr("已交给 {who} · 会话「{s}」", { who: label, s: landing.sessionName })
          : tr("正在送往 {who} · 会话「{s}」", { who: label, s: landing.sessionName })}
      </div>
      <button
        onClick={(e) => { e.stopPropagation(); onOpenSession(landing.sessionId); }}
        className="ml-auto shrink-0 text-[11px] text-accent-400 hover:text-accent-300"
      >
        {tr("打开会话 →")}
      </button>
    </div>
  );
}

/** 选接手方。没装的**摆出来但不可选并写明原因** —— 藏起来会让人以为我们不支持它。 */
function TargetPicker({ avail, onPick, onClose, tr }: {
  avail: Availability;
  onPick: (engine: Engine) => void;
  onClose: () => void;
  tr: (s: string, v?: Record<string, string>) => string;
}) {
  return (
    <>
      <div className="fixed inset-0 z-[60]" onClick={onClose} />
      <div className="absolute z-[61] left-3.5 right-3.5 bottom-2 rounded-xl border border-white/[0.12] bg-bg-2 shadow-card p-2">
        <div className="px-1.5 pb-1.5 text-[11px] text-ink-4">
          {tr("交给谁接着干？会在护照的工作目录里开一个会话，并把状态发进去。")}
        </div>
        {TARGETS.map((x) => {
          const installed = avail[x.id] !== false;
          // 「没装」和「装了但交接不过去」是两个不同的原因，得说不同的话 ——
          // 混成一句「不可用」，客户会去装一个装了也没用的东西。
          const reason = x.blocked ? tr(x.blocked) : installed ? tr(x.why) : tr("这台机器上还没装");
          const on = installed && !x.blocked;
          return (
            <button
              key={x.id}
              disabled={!on}
              onClick={() => onPick(x.id)}
              data-testid={`passport-target-${x.id}`}
              title={reason}
              className={cn(
                "w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-left transition-colors",
                on ? "hover:bg-white/[0.06]" : "opacity-45 cursor-not-allowed",
              )}
            >
              <ToolIcon tool={x.icon} size={14} active={on} className="shrink-0" />
              <span className="text-[12px] text-ink-1 shrink-0">{tr(x.label)}</span>
              <span className="text-[10.5px] text-ink-4 truncate">{reason}</span>
            </button>
          );
        })}
      </div>
    </>
  );
}

/** 护照详情：护照真正值钱的那些字段（已验证事实、决策理由、还差什么）终于有地方摆了。 */
function PassportDetail({
  passport: p, landing, delivered, avail, sending,
  onBack, onHandoff, onOpenSession, onCopyId, copied, tr,
}: {
  passport: TaskPassport;
  landing: Landing | null;
  delivered: boolean;
  avail: Availability;
  sending: boolean;
  onBack: () => void;
  onHandoff: (engine: Engine) => void;
  onOpenSession: (id: string) => void;
  onCopyId: () => void;
  copied: boolean;
  tr: (s: string, v?: Record<string, string>) => string;
}) {
  const [picker, setPicker] = useState(false);
  const verified = p.facts?.filter((f) => f.verified) ?? [];
  const unverified = p.facts?.filter((f) => !f.verified) ?? [];

  return (
    <div className="h-full flex flex-col bg-bg-2">
      <header className="flex items-center gap-2 px-5 pt-4 pb-3 shrink-0">
        <button
          onClick={onBack}
          className="shrink-0 inline-flex items-center gap-1 text-[12px] text-ink-3 hover:text-ink-0 transition-colors"
        >
          <ChevronLeft size={14} /> {tr("护照列表")}
        </button>
        <div className="ml-auto flex items-center gap-1.5 shrink-0">
          <button
            onClick={onCopyId}
            title={tr("复制护照号")}
            className="inline-flex items-center gap-1 rounded-md border border-white/[0.10] px-2 py-1 text-[11px] text-ink-3 hover:text-ink-0 transition-colors"
          >
            {copied ? <Check size={11} /> : <ClipboardCopy size={11} />}
            {tr(copied ? "已复制" : "复制护照号")}
          </button>
          <div className="relative">
            <button
              onClick={() => setPicker((v) => !v)}
              disabled={sending}
              data-testid="passport-detail-handoff"
              className="inline-flex items-center gap-1.5 rounded-md bg-accent px-2.5 py-1 text-[11.5px] font-semibold text-white hover:bg-accent-600 transition-colors disabled:opacity-50"
            >
              {sending ? <Loader2 size={11} className="animate-spin" /> : <Send size={11} />}
              {tr(sending ? "正在交接…" : "交给另一个 AI")}
            </button>
            {picker && (
              <div className="absolute right-0 top-full mt-1.5 w-[300px]">
                <TargetPicker
                  avail={avail}
                  onPick={(e) => { setPicker(false); onHandoff(e); }}
                  onClose={() => setPicker(false)}
                  tr={tr}
                />
              </div>
            )}
          </div>
        </div>
      </header>

      <div className="flex-1 min-h-0 overflow-y-auto px-5 pb-5 space-y-3.5">
        <div>
          <h2 className="text-[15px] font-semibold text-ink-0">{p.title || p.id}</h2>
          <div className="mt-1 text-[11px] font-mono text-accent-300">
            {p.id} · v{p.version}
            {p.harness ? tr(" · 上次由 {h} 写入", { h: p.harness }) : ""}
            {p.updated_at ? ` · ${p.updated_at}` : ""}
          </div>
          {p.scope && (
            <div className="mt-1 text-[11px] text-ink-4 break-all">{tr("工作目录：{d}", { d: p.scope })}</div>
          )}
        </div>

        {landing && (
          <LandingLine landing={landing} delivered={delivered} onOpenSession={onOpenSession} tr={tr} />
        )}

        <Section title={tr("目标")}>
          <p className="text-[12.5px] text-ink-1 leading-relaxed whitespace-pre-wrap">
            {p.goal || tr("（没写目标 —— 这张护照交接不出去）")}
          </p>
        </Section>

        {p.current_state && (
          <Section title={tr("世界此刻")}>
            <p className="text-[12.5px] text-ink-2 leading-relaxed whitespace-pre-wrap">{p.current_state}</p>
          </Section>
        )}

        {p.verification && (
          <Section title={tr("验证到哪了")}>
            <p className="text-[12.5px] text-ink-2 leading-relaxed whitespace-pre-wrap">{p.verification}</p>
          </Section>
        )}

        <Section
          title={tr("下一步")}
          hint={p.next_steps?.length ? undefined : tr("空的 —— 接手方拿到手还是得回头问人")}
        >
          <ol className="space-y-1.5">
            {(p.next_steps ?? []).map((s, i) => (
              <li key={i} className="flex gap-2 text-[12.5px] text-ink-1 leading-relaxed">
                <span className="shrink-0 text-ink-5 font-mono text-[11px] mt-0.5">{i + 1}.</span>
                <span className="min-w-0 whitespace-pre-wrap">{s}</span>
              </li>
            ))}
          </ol>
        </Section>

        {/* ✓ / ? 的区分**不能省**：把「只是说法」和「机器复验过」画成一样，
            接手方就会拿一句没验过的话当地基往下盖。 */}
        {(verified.length > 0 || unverified.length > 0) && (
          <Section title={tr("已知事实（✓=机器复验过，?=只是说法）")}>
            <ul className="space-y-1.5">
              {[...verified, ...unverified].map((f, i) => (
                <li key={i} className="flex gap-2 text-[12px] leading-relaxed">
                  <span className={cn("shrink-0 mt-0.5", f.verified ? "text-accent-400" : "text-amber-400")}>
                    {f.verified ? "✓" : "?"}
                  </span>
                  <span className="min-w-0">
                    <span className="text-ink-1">{f.claim}</span>
                    {f.source && <span className="text-ink-4"> （{tr("出处")}：{f.source}）</span>}
                  </span>
                </li>
              ))}
            </ul>
          </Section>
        )}

        {p.decisions?.length > 0 && (
          <Section title={tr("已定的事（含理由，别重新纠结）")}>
            <ul className="space-y-2">
              {p.decisions.map((d, i) => (
                <li key={i} className="text-[12px] leading-relaxed">
                  <div className="text-ink-1">{d.what}</div>
                  {d.why && <div className="text-ink-4 mt-0.5">{tr("因为")}：{d.why}</div>}
                </li>
              ))}
            </ul>
          </Section>
        )}

        {p.artifacts?.length > 0 && (
          <Section title={tr("已产出")}>
            <ul className="space-y-1">
              {p.artifacts.map((a, i) => (
                <li key={i} className="text-[11.5px] text-ink-3 break-all font-mono">{a}</li>
              ))}
            </ul>
          </Section>
        )}
      </div>
    </div>
  );
}

function Section({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-white/[0.06] bg-white/[0.02] px-3.5 py-3">
      <div className="flex items-center gap-2 mb-2">
        <h3 className="text-[12px] font-medium text-ink-3">{title}</h3>
        {hint && <span className="text-[11px] text-amber-400/90">{hint}</span>}
      </div>
      {children}
    </section>
  );
}
