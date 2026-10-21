/**
 * 维护助手 —— 极简 AI 维护对话页（UChat/UCLI 收敛后的唯一内置对话入口）。
 *
 * 【范围】只做「装机/工具故障排查」这一类维护任务：把失败现状说给模型，让它诊断、
 * 走只读/写动作（写动作一律弹确认）。**不复用** `opencodex/Chat.tsx`（那套会把终端/
 * 预览/作图面板整包拽回来）；不接第三方下载，不宣称 magpie/OpenCodex 已集成。
 *
 * 【对话内核】直接用 Rust `agent/chat.rs` 的 `chat_send` + Channel 事件流，
 * 审批走 `chat_approve`，中断走 `chat_interrupt` —— 与工作台同一套后端，零新后端。
 * 模型固定：虾盘云 + `deepseek-v4-flash`（与 providers.rs 虾盘云 preset 一致）。
 * Key 只从 `get_device_key` 拿，**绝不**进提示词 / 剪贴板 / localStorage / 命令行。
 *
 * 【余额】**只在真正发起 AI 任务时**查一次；判据是 `balance.tokens` / `balance.cny`
 * / `balance.text` 等实际字段，**不能拿 `charged` 代替余额**。查询失败 = 明确显示
 * 「查询失败 + 重查」，绝不误称「没余额」。普通安装流程不被余额门禁。
 *
 * 【交接】「复制维护任务」产出统一任务包（`lib/maintenanceTask.ts` 集中脱敏），
 * 剪贴板里无 Key。`pendingChatPrompt` 只填草稿，用户自己按发送。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Check,
  Copy,
  Loader2,
  MessageSquare,
  Plus,
  Send,
  ShieldCheck,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { cn } from "./lib/cn";
import { copyToClipboard } from "./lib/clipboard";
import { buildMaintenanceTask } from "./lib/maintenanceTask";
import type { Balance, DeviceKey } from "./lib/types";
import { useI18n } from "./i18n";

/** 与 providers.rs 虾盘云 preset 的 `model` 一致（客户装好在用的就是它）。 */
const DEFAULT_MODEL = "deepseek-v4-flash";

type TextItem = { type: "text"; role: "user" | "assistant"; content: string };
type ToolItem = {
  type: "tool";
  name: string;
  phase: "running" | "done" | "error";
  prompt?: string;
  path?: string;
  command?: string;
  output?: string;
  message?: string;
};
type ApprovalItem = {
  type: "approval";
  id: string;
  tool: string;
  action?: string;
  inputKeys?: string[];
  path?: string;
  bytes?: number;
  command?: string;
  decided?: "approved" | "rejected";
};
type Item = TextItem | ToolItem | ApprovalItem;

/** 余额判定：只看实际字段。charged 是派生量，不能当判据（需求榜 2026-10-19）。 */
type BalanceGate =
  | { state: "ok"; text: string }
  | { state: "empty"; text: string }
  | { state: "unknown"; text: string } // 未充值或查询无结果 —— 不说「没余额」
  | { state: "error"; message: string }; // 查询本身失败 —— 明确显示 + 重查

function judgeBalance(dk: DeviceKey | null): BalanceGate {
  if (!dk) return { state: "error", message: "get_device_key 无返回" };
  const b: Balance | null = dk.balance;
  if (!b) {
    // balance = null：后端语义是「还没充值 / 查询失败」，两者不可区分 —— 如实说，不误称没余额。
    return { state: "unknown", text: "还没查到余额（可能未充值，也可能查询没返回）" };
  }
  const tokens = Number(b.tokens);
  const cny = typeof b.cny === "number" ? b.cny : null;
  const emptyByTokens = Number.isFinite(tokens) && tokens <= 0;
  const emptyByCny = cny !== null && cny <= 0;
  if (emptyByTokens && (cny === null || emptyByCny)) {
    return { state: "empty", text: b.text || "余额不足" };
  }
  return { state: "ok", text: b.text || "可用" };
}

const toolLabel: Record<string, string> = {
  list_dir: "列目录",
  read_file: "读文件",
  write_file: "写文件",
  edit_file: "改文件",
  run_command: "跑命令",
  uking_action: "调动作",
  generate_image: "作图",
  generate_video: "出片",
};

/** 左侧「维护记录」列表的一条。消息本体在后端 archive（~/.uking/chats/<id>.jsonl），这里只存索引。 */
type SessionMeta = { id: string; title: string; ts: number };
const SESSIONS_KEY = "uking.maint.sessions";
const NEW_CHAT_TITLE = "新对话";

function loadSessionIndex(): SessionMeta[] {
  try {
    const raw = localStorage.getItem(SESSIONS_KEY);
    const list = raw ? (JSON.parse(raw) as SessionMeta[]) : [];
    return Array.isArray(list) ? list.filter((s) => s && typeof s.id === "string") : [];
  } catch {
    return [];
  }
}
function saveSessionIndex(list: SessionMeta[]) {
  try {
    localStorage.setItem(SESSIONS_KEY, JSON.stringify(list.slice(0, 50)));
  } catch {
    /* 配额满：不落盘也能用，历史只是丢一次 */
  }
}
export function MaintenanceAssistant({
  onToast,
  onRecharge,
  pendingPrompt,
  onConsumedPrompt,
}: {
  onToast?: (msg: string) => void;
  /** 一键充值（App 传入 `openRechargeAndWatch`，钱包轮询归 App 现存逻辑）。 */
  onRecharge?: () => void;
  /** 深链带来的草稿（AI 设置 / 优化大师 / 工具页 / 装机向导交接）——只填输入框，不自动发送。 */
  pendingPrompt?: { prompt: string } | null;
  onConsumedPrompt?: () => void;
}) {
  const { t } = useI18n();
  const [items, setItems] = useState<Item[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [key, setKey] = useState<string>("");
  // 页头的余额展示/按钮 2026-10-21 收起；门禁仍在发送前算一次，结果只用 toast 提示。
  const [, setBalanceGate] = useState<BalanceGate | null>(null);
  /** 当前会话 id —— 每次「新对话」换一个；历史记录在左侧列表里，落 ~/.uking/chats/<id>.jsonl。 */
  const [sessionId, setSessionId] = useState<string>(() => `maint-${Date.now().toString(36)}`);
  /** 左侧会话索引（id / 标题 / 时间）。消息本体由后端 archive 存，不塞 localStorage。 */
  const [sessions, setSessions] = useState<SessionMeta[]>(() => loadSessionIndex());
  /** 读历史时跳过「写回 archive」，否则会把刚加载的列表覆盖成空。 */
  const loadingRef = useRef(false);
  const persistTimer = useRef<number | null>(null);

  // 当前会话必须出现在左侧列表里（挂载那一刻登记一条）
  useEffect(() => {
    setSessions((prev) => (prev.some((s) => s.id === sessionId) ? prev : [{ id: sessionId, title: NEW_CHAT_TITLE, ts: Date.now() }, ...prev].slice(0, 50)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 会话索引落盘
  useEffect(() => {
    saveSessionIndex(sessions);
  }, [sessions]);

  // 消息变化 → 写后端 archive（防抖 400ms；加载历史时跳过）
  useEffect(() => {
    if (loadingRef.current || items.length === 0) return;
    if (persistTimer.current) window.clearTimeout(persistTimer.current);
    persistTimer.current = window.setTimeout(() => {
      void invoke("chat_archive_replace", { sessionId, items }).catch(() => {});
    }, 400);
    return () => {
      if (persistTimer.current) window.clearTimeout(persistTimer.current);
    };
  }, [items, sessionId]);

  // 首条用户消息当标题（只改还没起名的）
  useEffect(() => {
    const firstUser = items.find((it): it is TextItem => it.type === "text" && it.role === "user");
    if (!firstUser) return;
    const title = firstUser.content.replace(/\s+/g, " ").trim().slice(0, 24) || NEW_CHAT_TITLE;
    setSessions((prev) => prev.map((s) => (s.id === sessionId && (!s.title || s.title === NEW_CHAT_TITLE) ? { ...s, title, ts: Date.now() } : s)));
  }, [items, sessionId]);

  /** 新开一条对话（左侧留记录，旧的不删）。 */
  const newSession = useCallback(() => {
    const id = `maint-${Date.now().toString(36)}`;
    setSessionId(id);
    setItems([]);
    setInput("");
    setBusy(false);
    setBalanceGate(null);
    setSessions((prev) => [{ id, title: NEW_CHAT_TITLE, ts: Date.now() }, ...prev].slice(0, 50));
  }, []);

  /** 点左侧历史记录 → 读回那次对话。 */
  const openSession = useCallback(
    async (id: string) => {
      if (id === sessionId) return;
      loadingRef.current = true;
      setSessionId(id);
      setItems([]);
      setBusy(false);
      try {
        const loaded = await invoke<Item[]>("chat_archive_load", { sessionId: id });
        setItems(Array.isArray(loaded) ? loaded : []);
      } catch {
        setItems([]);
      }
      loadingRef.current = false;
    },
    [sessionId],
  );

  const deleteSession = useCallback(
    (id: string) => {
      invoke("chat_archive_delete", { sessionId: id }).catch(() => {});
      setSessions((prev) => {
        const rest = prev.filter((s) => s.id !== id);
        if (id === sessionId) {
          const next = rest[0]?.id ?? `maint-${Date.now().toString(36)}`;
          setSessionId(next);
          setItems([]);
        }
        return rest;
      });
    },
    [sessionId],
  );
  const listRef = useRef<HTMLDivElement | null>(null);

  // 草稿：来自深链/交接的提示词 —— 新开一条维护对话，并把提示词**预先填进输入框**，
  // 等用户自己按发送（不加载页面就发消息/付费）。用户口径（2026-10-21）：
  // 「点击这些让轻助手干活的时候，应该默认已经把提示词放进聊天框了」。
  useEffect(() => {
    if (!pendingPrompt?.prompt) return;
    newSession();
    setInput(pendingPrompt.prompt);
    onConsumedPrompt?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingPrompt?.prompt]);

  // 设备 Key：只在内存里引用，不落任何存储。加载时拿一次（拿不到先空着，发送时再查）。
  useEffect(() => {
    invoke<DeviceKey>("get_device_key")
      .then((dk) => setKey(dk?.key ?? ""))
      .catch(() => {});
  }, []);

  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [items]);


  const approve = useCallback((id: string, ok: boolean) => {
    invoke("chat_approve", { approvalId: id, approved: ok }).catch(() => {});
    setItems((prev) =>
      prev.map((it) => (it.type === "approval" && it.id === id ? { ...it, decided: ok ? "approved" : "rejected" } : it)),
    );
  }, []);

  const stop = useCallback(() => {
    invoke("chat_interrupt", { taskId: sessionId }).catch(() => {});
    setBusy(false);
  }, []);

  const copyTask = useCallback(async () => {
    // 交接包只收目标/失败原文/最近几条对话摘要，不复制全量历史；密钥集中脱敏。
    const lastTexts = items
      .filter((it): it is TextItem => it.type === "text")
      .slice(-6)
      .map((it) => `${it.role === "user" ? "用户" : "助手"}：${it.content}`)
      .join("\n");
    const pkg = buildMaintenanceTask({
      target: "U-King 维护助手对话交接",
      failure: input.trim() || lastTexts || "（用户未填写失败原文）",
      steps: lastTexts ? `最近对话：\n${lastTexts}` : [],
      verify: "对话中确认根因后，按建议步骤验证故障现象消失。",
      rollback: "涉及配置改动的，先备份原文件再改；不确定就先只诊断不动手。",
    });
    const ok = await copyToClipboard(pkg);
    onToast?.(ok ? t("维护任务已复制（已脱敏，不含 Key）") : t("复制失败，请手动选中复制"));
  }, [items, input, onToast, t]);

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || busy) return;
    // 发起 AI 任务前查余额 —— 只在这一步做，普通安装不被门禁。
    let dk: DeviceKey | null = null;
    try {
      dk = await invoke<DeviceKey>("get_device_key");
      setKey(dk?.key ?? "");
    } catch (e) {
      setBalanceGate({ state: "error", message: String(e) });
      onToast?.(t("余额查询失败，请重查后再发（不是没余额）"));
      return;
    }
    const gate = judgeBalance(dk);
    setBalanceGate(gate);
    if (gate.state === "error") {
      onToast?.(t("余额查询失败，请重查后再发（不是没余额）"));
      return;
    }
    if (gate.state === "empty") {
      onToast?.(t("余额不足，请先充值"));
      onRecharge?.();
      return;
    }
    if (gate.state === "unknown") {
      // 未充值或查询无结果：不硬拦（可能只是查询没返回），但要说清楚。
      onToast?.(t("还没查到余额（可能未充值）——可以先充值，或再试一次"));
    }
    const apiKey = dk?.key ?? key;
    if (!apiKey) {
      onToast?.(t("还没拿到设备 Key，稍等一下再试"));
      return;
    }

    const history: Item[] = [...items, { type: "text", role: "user", content: text }];
    setItems(history);
    setInput("");
    setBusy(true);
    const msgs = history
      .filter((it): it is TextItem => it.type === "text")
      .map((it) => ({ role: it.role, content: it.content }));

    const ch = new Channel<any>();
    ch.onmessage = (ev: any) => {
      if (ev.kind === "delta" && ev.text) {
        setItems((prev) => {
          const n = [...prev];
          const l = n[n.length - 1];
          if (l?.type === "text" && l.role === "assistant") n[n.length - 1] = { ...l, content: l.content + ev.text };
          else n.push({ type: "text", role: "assistant", content: ev.text });
          return n;
        });
      } else if (ev.kind === "tool") {
        setItems((prev) => {
          const n = [...prev];
          if (ev.phase === "start") {
            n.push({ type: "tool", name: ev.name, phase: "running", prompt: ev.prompt, path: ev.path, command: ev.command });
            return n;
          }
          for (let i = n.length - 1; i >= 0; i--) {
            const it = n[i];
            if (it.type === "tool" && it.name === ev.name && it.phase === "running") {
              if (ev.phase === "output") {
                n[i] = { ...it, output: (it.output ?? "") + (ev.chunk ?? "") };
                return n;
              }
              if (ev.phase === "result") {
                n[i] = { ...it, phase: "done", path: ev.path ?? it.path, output: ev.output ?? it.output };
                return n;
              }
              n[i] = { ...it, phase: "error", message: ev.message };
              return n;
            }
          }
          if (ev.phase !== "output") {
            n.push({
              type: "tool",
              name: ev.name,
              phase: ev.phase === "result" ? "done" : "error",
              path: ev.path,
              command: ev.command,
              output: ev.output,
              message: ev.message,
            });
          }
          return n;
        });
      } else if (ev.kind === "approval") {
        // 写动作一律弹确认 —— 后端 ask 模式 180s 超时会自动拒绝；这里绝不自动批准。
        setItems((prev) => [
          ...prev,
          {
            type: "approval",
            id: ev.id,
            tool: ev.tool,
            action: ev.action,
            inputKeys: ev.input_keys,
            path: ev.path,
            bytes: ev.bytes,
            command: ev.command,
          },
        ]);
      } else if (ev.kind === "done") {
        setBusy(false);
        if (ev.status === "error") onToast?.(ev.message || t("对话失败"));
      }
    };
    try {
      await invoke("chat_send", {
        taskId: sessionId,
        messages: msgs,
        model: DEFAULT_MODEL,
        apiKey,
        baseUrl: null, // 虾盘云默认端点（providers.rs preset）
        workspace: null,
        approvalMode: "ask", // 每步确认：写动作永不自动批准
        onEvent: ch,
      });
    } catch (e) {
      setBusy(false);
      onToast?.(t("对话启动失败: {e}", { e: String(e) }));
    }
  }, [items, input, busy, key, onToast, onRecharge, t]);

  return (
    <div className="flex h-full min-h-0 bg-bg-0">
      {/* 左：维护记录 —— 每次「新对话」新开一条，历史保留、可点回看（用户 2026-10-21 口径） */}
      <aside className="w-48 shrink-0 border-r border-white/[0.08] flex flex-col">
        <div className="p-2">
          <button
            onClick={newSession}
            className="w-full inline-flex items-center justify-center gap-1.5 rounded-md border border-white/[0.10] bg-bg-1/60 h-8 text-[12px] text-ink-1 hover:bg-white/[0.04]"
          >
            <Plus size={13} /> {t("新对话")}
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2 space-y-0.5">
          {sessions.map((s) => (
            <div key={s.id} className="group relative">
              <button
                onClick={() => void openSession(s.id)}
                className={cn(
                  "w-full text-left rounded-md px-2 py-1.5 pr-7 text-[12px] truncate transition-colors",
                  s.id === sessionId ? "bg-accent/[0.12] text-ink-0" : "text-ink-3 hover:bg-white/[0.04]",
                )}
                title={s.title}
              >
                <MessageSquare size={11} className="inline mr-1 -mt-0.5" />
                {s.title || NEW_CHAT_TITLE}
              </button>
              <button
                onClick={() => deleteSession(s.id)}
                title={t("删除这条记录")}
                className="absolute right-1 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 text-ink-4 hover:text-danger-400 p-1"
              >
                <Trash2 size={11} />
              </button>
            </div>
          ))}
        </div>
      </aside>
      {/* 右：对话区（原逻辑不变） */}
      <div className="flex h-full min-h-0 flex-1 flex-col">
      {/* 页头：固定虾盘云 DeepSeek Flash，余额只读展示 + 一键充值 */}
      <header className="flex items-center gap-2 border-b border-white/[0.08] px-4 py-2.5">
        <ShieldCheck size={16} className="text-accent shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold text-ink-0">{t("维护助手")}</div>
          <div className="text-[11px] text-ink-4">
            {t("虾盘云 · DeepSeek Flash · 只做装机/故障维护")} ·{" "}
            <span className="text-ink-3">{t("写操作每步确认，不会自动执行")}</span>
          </div>
        </div>
        {/* 2026-10-21 用户口径：页头的余额/充值按钮收起来 —— 余额门禁仍在「发送前」生效，
            失败时用 toast 说清楚，不占页头。 */}
      </header>

      {/* 消息流 */}
      <div ref={listRef} className="min-h-0 flex-1 overflow-y-auto px-4 py-3 space-y-2.5">
        {items.length === 0 && (
          <div className="mx-auto max-w-2xl rounded-card border border-white/[0.08] bg-bg-1/50 px-4 py-3 text-[12px] leading-relaxed text-ink-3">
            {t(
              "把装机失败或工具故障的现象贴进来（报错原文最好），我来诊断。写操作会先弹确认框，不会自动改你的电脑。",
            )}
            <div className="mt-2 text-[11px] text-ink-4">
              {t("独立编程工作台（OpenCodex）尚未接入本产品——可在「我的 AI」里找已安装的工作台，这里不提供假安装链接。")}
            </div>
          </div>
        )}
        {items.map((it, i) => {
          if (it.type === "text") {
            return (
              <div
                key={i}
                className={cn(
                  "max-w-[85%] whitespace-pre-wrap break-words rounded-card px-3 py-2 text-[12.5px] leading-relaxed",
                  it.role === "user" ? "ml-auto bg-accent/15 text-ink-0" : "bg-bg-1 text-ink-1",
                )}
              >
                {it.content}
              </div>
            );
          }
          if (it.type === "tool") {
            return (
              <div key={i} className="max-w-[85%] rounded-card border border-white/[0.08] bg-bg-1/60 px-3 py-2">
                <div className="flex items-center gap-1.5 text-[11.5px] text-ink-2">
                  {it.phase === "running" ? (
                    <Loader2 size={12} className="animate-spin text-accent" />
                  ) : it.phase === "done" ? (
                    <Check size={12} className="text-success-400" />
                  ) : (
                    <X size={12} className="text-danger-400" />
                  )}
                  <span className="font-medium">{t(toolLabel[it.name] ?? it.name)}</span>
                  {it.phase === "running" && <span className="text-ink-4">{t("执行中…")}</span>}
                  {it.phase === "error" && <span className="text-danger-400">{t("失败")}</span>}
                </div>
                {(it.command || it.path || it.prompt) && (
                  <div className="mt-1 break-all text-[11px] text-ink-4">{it.command ?? it.path ?? it.prompt}</div>
                )}
                {it.message && <div className="mt-1 break-all text-[11px] text-danger-400">{it.message}</div>}
                {it.output && (
                  <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-bg-0/60 px-2 py-1 text-[10.5px] text-ink-3">
                    {it.output.slice(0, 2000)}
                  </pre>
                )}
              </div>
            );
          }
          // approval
          return (
            <div key={i} className="max-w-[85%] rounded-card border border-warning-400/30 bg-warning-400/[0.06] px-3 py-2">
              <div className="flex items-center gap-1.5 text-[12px] font-medium text-ink-1">
                <AlertTriangle size={13} className="text-warning-400" />
                {t("确认写操作")}：{t(toolLabel[it.tool] ?? it.tool)}
                {it.action ? ` · ${it.action}` : ""}
              </div>
              <div className="mt-1 space-y-0.5 text-[11px] text-ink-3">
                {it.path && <div className="break-all">{t("文件")}：{it.path}</div>}
                {it.command && <div className="break-all">{t("命令")}：{it.command}</div>}
                {typeof it.bytes === "number" && <div>{t("约 {n} 字节", { n: String(it.bytes) })}</div>}
              </div>
              {it.decided ? (
                <div className="mt-1.5 text-[11px] text-ink-4">
                  {it.decided === "approved" ? t("已批准") : t("已拒绝")}
                </div>
              ) : (
                <div className="mt-2 flex items-center gap-2">
                  <button
                    onClick={() => approve(it.id, true)}
                    className="inline-flex items-center gap-1 rounded-md bg-accent px-3 h-7 text-[11.5px] font-medium text-white hover:opacity-90"
                  >
                    <Check size={12} /> {t("批准")}
                  </button>
                  <button
                    onClick={() => approve(it.id, false)}
                    className="inline-flex items-center gap-1 rounded-md border border-white/[0.12] px-3 h-7 text-[11.5px] text-ink-2 hover:bg-white/[0.04]"
                  >
                    <X size={12} /> {t("拒绝")}
                  </button>
                  <span className="text-[10.5px] text-ink-4">{t("不点则 3 分钟后自动拒绝")}</span>
                </div>
              )}
            </div>
          );
        })}
        {busy && (
          <div className="flex items-center gap-2 text-[11px] text-ink-4">
            <Loader2 size={12} className="animate-spin" /> {t("处理中…")}
            <button
              onClick={stop}
              className="inline-flex items-center gap-1 rounded-md border border-white/[0.12] px-2 h-6 text-[11px] text-ink-3 hover:bg-white/[0.04]"
            >
              <Square size={10} /> {t("中断")}
            </button>
          </div>
        )}
      </div>

      {/* 输入区 */}
      <div className="border-t border-white/[0.08] px-4 py-3">
        <div className="flex items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void send();
              }
            }}
            rows={3}
            placeholder={t("描述装机失败/工具故障现象，粘贴报错原文…（Enter 发送，Shift+Enter 换行）")}
            className="min-h-[64px] flex-1 resize-none rounded-card border border-white/[0.10] bg-bg-1/50 px-3 py-2 text-[12.5px] leading-relaxed text-ink-1 outline-none placeholder:text-ink-4 focus:border-accent/40"
          />
          <div className="flex flex-col gap-1.5">
            <button
              onClick={() => void send()}
              disabled={busy || !input.trim()}
              className="inline-flex items-center gap-1 rounded-md bg-accent px-3 h-8 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-40"
            >
              <Send size={13} /> {t("发送")}
            </button>
            <button
              onClick={() => void copyTask()}
              title={t("生成脱敏后的统一维护任务包，可交给外部 AI")}
              className="inline-flex items-center gap-1 rounded-md border border-white/[0.12] px-3 h-8 text-[11.5px] text-ink-2 hover:bg-white/[0.04]"
            >
              <Copy size={12} /> {t("复制维护任务")}
            </button>
          </div>
        </div>
      </div>
      </div>
    </div>
  );
}
