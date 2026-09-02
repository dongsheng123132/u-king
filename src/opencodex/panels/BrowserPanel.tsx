/**
 * 浏览器面板 —— 直播共享浏览器：AI 可控、可感知，用户实时看到同一会话。
 *
 * 引擎是 agent-browser（无头 Chrome，`browser.*` 影核动作的后端）。AI 和这个面板
 * 走**同一套** `browser.*` 动作（宪法 13/14：业务动作只实现一次）：
 *   - 感知：`browser.snapshot` 返回带 @ref 的可访问性树 —— 面板的交互树就是 AI 看的那份；
 *   - 控制：`browser.open/click/clickat/type/press/scroll/back/forward/reload`；
 *   - 直播：面板连 agent-browser 的 WebSocket 流（`browser.stream` 给地址），帧自动推送，
 *     AI 一导航/点击，用户立刻看到画面变化。
 *
 * 交互以 @ref 树为主（和 AI 同一份感知、单条 CLI 快速确定）；直播画面坐标点击为辅
 * （browser.clickat，合成 move+down+up，略慢）。登录态仍是已知限制（独立 Chrome profile）。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft, ArrowRight, ArrowUpRight, CornerDownLeft, ExternalLink, Globe,
  History, Keyboard, Loader2, MousePointer2, RotateCw, ScrollText,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/cn";

/** 常查的本机开发端口。状态是**真探**出来的（preview_port_alive），不是猜的。 */
const QUICK_PORTS = [3000, 5173, 8080, 4200];
const HIST_KEY = "uking.browser.recent";
const HIST_MAX = 8;

type AnyObj = Record<string, unknown>;

/** 统一调影核动作。
 *
 * 🔴 确认位默认 **false**，和全项目其它调用方一致（generated client、useWorkbenchOffer 都是
 * false）。本面板用到的 `browser.*` 全是 confirmation=never，不需要确认位；真正带确认门的
 * `browser.submit`（发帖/下单/删除这类有外部副作用的提交）**不该被这里默认签字**——
 * 要用它就在调用处显式传 true，让「谁确认的」看得见。
 *
 * 以前这里默认 true，注释还写着「传 true 无副作用」。它有副作用：信封适配器当时不看动作
 * 要不要确认就往入参塞 `confirm`，而只读动作的 schema 是 additionalProperties:false，
 * 于是整块面板被自己的入参校验判成 `invalid_input: 未知字段 confirm`。适配器已修
 * （lib.rs::action_parity_call_inner），这里连默认值一起摆正。 */
/**
 * 影核动作错误 —— **带上 `code` / `retriable` / `hint`**。
 *
 * 🔴 原来这里 `throw new Error(message)`，把后端辛苦标好的归因**整个丢掉**。
 * 后果实测（2026-08-18 黑盒测试报告，`~/.uking/journal/*.jsonl`）：
 * `browser.snapshot` 在 playwright 没装时被调了 **8873 次**，其中 08-18 一天 6598 次、
 * `ok:true` 零次、平均间隔 3.38 秒、持续 6.2 小时。后端每一条都写着
 * `code=not_installed retriable=false`。
 *
 * **后端契约、单元测试（`actions.rs` 里那条「未知错绝不自动重试」）、错误归因全做对了，
 * 唯一的消费端一行都没读。** 而这套 `retriable` 契约的诞生理由就写在 `actions.rs:17`：
 * 「2026-07-28 T-King 出不了片，根因表面是一行 retriable 写反了」。
 * 补装 playwright 只消除症状 —— 任何别的组件报 not_installed 都会原样复现。
 */
class ActionFailed extends Error {
  code: string;
  retriable: boolean;
  hint: string;
  constructor(actionId: string, err: AnyObj | undefined) {
    super((err?.message as string) || `${actionId} 失败`);
    this.code = (err?.code as string) || "unknown";
    // 缺省按**可重试**处理：拿不到这个字段时停掉轮询，会把「一次网络抖动」变成「面板死了」。
    this.retriable = err?.retriable !== false;
    this.hint = (err?.hint as string) || "";
  }
}

async function runAction(actionId: string, input: AnyObj = {}, confirmed = false): Promise<AnyObj> {
  const res: any = await invoke("action_parity_call", {
    request: { action_id: actionId, input, confirmed },
  });
  if (!res?.ok) throw new ActionFailed(actionId, res?.error);
  return res.result;
}

/** 快照行（`- link "Learn more" [ref=e2]`，2 空格缩进 = 层级）解析成树。 */
interface TreeRow {
  indent: number;
  role: string;
  label: string;
  ref?: string;
}
function parseSnapshot(text: string): TreeRow[] {
  const rows: TreeRow[] = [];
  for (const line of text.split("\n")) {
    const m = line.match(/^(\s*)[-•]?\s*(.*)$/);
    if (!m) continue;
    const body = m[2];
    if (!body) continue;
    const labelM = body.match(/"((?:[^"\\]|\\.)*)"/);
    const label = labelM ? labelM[1] : "";
    const role = labelM ? body.slice(0, body.indexOf(labelM[0])).trim() : body.trim();
    const refM = body.match(/\bref=([a-zA-Z0-9]+)/);
    if (!role && !label) continue;
    rows.push({
      indent: Math.floor(m[1].length / 2),
      role,
      label,
      ref: refM ? refM[1] : undefined,
    });
  }
  return rows;
}

const loadHist = (): string[] => {
  try {
    const a = JSON.parse(localStorage.getItem(HIST_KEY) || "[]");
    return Array.isArray(a) ? a.slice(0, HIST_MAX) : [];
  } catch {
    return [];
  }
};

export function BrowserPanel({ taskId, openUrl, active = true }: {
  taskId: string;
  /** 宿主要它打开的地址（换一个值就导航一次）。用于「预览里点『用浏览器打开』」这条路。 */
  openUrl?: string;
  /** 只有当前可见的浏览器面板才占用 agent-browser 流和快照轮询。 */
  active?: boolean;
}) {
  const { t } = useI18n();
  const [url, setUrl] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [installingRuntime, setInstallingRuntime] = useState(false);
  const [runtimeInstallNeeded, setRuntimeInstallNeeded] = useState(false);
  const [hist, setHist] = useState<string[]>(loadHist);
  const [ports, setPorts] = useState<Record<number, boolean>>({});

  // ── 直播流 ──
  const [wsReady, setWsReady] = useState(false); // ws 已连上且收到过帧/状态
  const [wsFailed, setWsFailed] = useState(false); // stream 拿不到/连不上（agent-browser 没装等）
  const [frame, setFrame] = useState<string | null>(null); // data:image/jpeg;base64,…
  const [viewport, setViewport] = useState({ width: 1280, height: 720 });
  const [current, setCurrent] = useState(""); // 当前页 URL（来自流里的 tabs）
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectsRef = useRef(0);
  // 每次隐藏/重开都是新一代连接。`browser.stream` 是异步 invoke，不能让旧一代
  // 在面板收起之后才回来偷偷开一个没有 owner 的 WebSocket。
  const streamEpochRef = useRef(0);
  const mountedRef = useRef(true);
  const activeRef = useRef(active);
  activeRef.current = active;
  const urlFocusRef = useRef(false);

  // ── 交互树（AI 同一份感知）──
  const [tree, setTree] = useState<TreeRow[]>([]);
  const [treeBusy, setTreeBusy] = useState(false);

  const remember = (u: string) => {
    setHist((prev) => {
      const next = [u, ...prev.filter((x) => x !== u)].slice(0, HIST_MAX);
      try {
        localStorage.setItem(HIST_KEY, JSON.stringify(next));
      } catch {
        /* 配额满：不记就是了 */
      }
      return next;
    });
  };

  /** 连接 agent-browser 直播流。agent-browser 没装时这里会抛 not_installed，显示安装引导。 */
  const connectStream = useCallback(async (epoch = streamEpochRef.current) => {
    if (!mountedRef.current || !activeRef.current) return;
    try {
      const r = await runAction("browser.stream", {}, false);
      if (!mountedRef.current || !activeRef.current || epoch !== streamEpochRef.current) return;
      const wsUrl = r.ws_url as string;
      if (!wsUrl) throw new Error("browser.stream 没返回 ws 地址");
      setWsFailed(false);
      setRuntimeInstallNeeded(false);
      // 同一代也只允许一条流；快速切换不会留下旧连接。
      if (wsRef.current) wsRef.current.close();
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;
      ws.onmessage = (e) => {
        if (!activeRef.current) return;
        try {
          const m = JSON.parse(e.data as string);
          if (m.type === "status") {
            setViewport({
              width: m.viewportWidth || 1280,
              height: m.viewportHeight || 720,
            });
            setWsReady(true);
          } else if (m.type === "tabs") {
            const active = (m.tabs as any[])?.find((x) => x.active);
            if (active?.url) {
              setCurrent(active.url);
              if (!urlFocusRef.current) setUrl(active.url);
            }
          } else if (m.type === "frame" && typeof m.data === "string") {
            setFrame("data:image/jpeg;base64," + m.data);
            setWsReady(true);
          }
          // 收到协议消息说明本轮流已恢复；之后再断线仍可获得三次受控重连机会。
          reconnectsRef.current = 0;
        } catch {
          /* 非 JSON 帧忽略 */
        }
      };
      ws.onclose = () => {
        // 断线后旧帧不能继续冒充「直播中」。最多重连三次，避免环境缺失时永久刷后台。
        if (wsRef.current !== ws || epoch !== streamEpochRef.current) return;
        wsRef.current = null;
        setWsReady(false);
        setFrame(null);
        if (!mountedRef.current || !activeRef.current) return;
        if (reconnectsRef.current >= 3) {
          setWsFailed(true);
          setErr(t("直播流已断开，请检查浏览器运行时后再试一次"));
          return;
        }
        reconnectsRef.current += 1;
        setWsFailed(true);
        setErr(t("直播流已断开，正在重连…"));
        reconnectTimerRef.current = setTimeout(() => {
          reconnectTimerRef.current = null;
          void connectStream(epoch);
        }, reconnectsRef.current * 1_000);
      };
      ws.onerror = () => {
        if (wsRef.current !== ws) return;
        setErr(t("直播流连接失败，正在重连…"));
        ws.close(); // onclose 统一清状态和决定是否重连
      };
    } catch (e) {
      if (!mountedRef.current || !activeRef.current || epoch !== streamEpochRef.current) return;
      setWsFailed(true);
      setRuntimeInstallNeeded(
        e instanceof ActionFailed && (e.code === "not_installed" || e.message.includes("version_mismatch")),
      );
      setErr(String(e));
    }
  }, [t]);

  // 🔴 **「连接中…」不许无限转下去。**
  // 原来只要 `browser.stream` 不抛错、WebSocket 又一直没推消息，顶栏就永远停在
  // 「连接中…」，用户看到的是一个假装在加载、其实永远不会好的面板 ——
  // 这跟今天治的那一类「零观测被当成成功」是同一个病：**没结果 ≠ 还在努力**。
  // 12 秒还没收到任何一条消息（status/tabs/frame 都算）就如实认输，让下面的
  // wsFailed 分支把原因和安装引导摆出来。daemon 冷启动实测 2~4 秒，12 秒足够宽。
  useEffect(() => {
    if (!active || wsReady || wsFailed) return;
    const id = setTimeout(() => {
      if (!mountedRef.current || wsReady) return;
      setWsFailed(true);
      setErr((prev) => prev ?? t("12 秒内没收到任何画面 —— agent-browser 多半没起来（国内网络下装内核常被重置）"));
    }, 12_000);
    return () => clearTimeout(id);
  }, [active, wsReady, wsFailed, t]);

  useEffect(() => {
    mountedRef.current = true;
    const epoch = ++streamEpochRef.current;
    if (active) void connectStream(epoch);
    return () => {
      mountedRef.current = false;
      streamEpochRef.current += 1;
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
      reconnectsRef.current = 0;
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      setWsReady(false);
      setFrame(null);
    };
  }, [active, connectStream]);

  // ── 交互树轮询：打开时每 3s 刷一次（AI 在别处操作也能跟上），用户动作后手动刷 ──
  // 用 ref 防重入（state 当 guard 会拖进 effect 依赖、让 interval 反复重建）
  const treeBusyRef = useRef(false);
  /** 不可重试的错（如内核没装）—— 停掉轮询，并把后端给的 hint 摆出来。 */
  const [treeStopped, setTreeStopped] = useState<{ message: string; hint: string } | null>(null);
  const refreshTree = useCallback(async () => {
    if (!activeRef.current || treeBusyRef.current) return;
    treeBusyRef.current = true;
    setTreeBusy(true);
    try {
      const r = await runAction("browser.snapshot", {}, false);
      if (!activeRef.current) return;
      setTree(parseSnapshot((r.snapshot as string) || ""));
      setTreeStopped(null);
    } catch (e) {
      // 🔴 **读 `retriable`，别把错整个吞掉**。吞掉的代价是实测出来的：
      // 每 3 秒一次、6 小时 6598 次、一次都不会成功 —— 而每一条后端都说了「别重试」。
      // 可重试的错（网络抖动）保留旧树、下一轮再来；不可重试的**停下来并说清楚**。
      const f = e as ActionFailed;
      if (f && f.retriable === false) {
        setTreeStopped({ message: f.message, hint: f.hint });
      }
    } finally {
      treeBusyRef.current = false;
      setTreeBusy(false);
    }
  }, []);

  useEffect(() => {
    // 已经判定「不用再试了」就不再起定时器 —— 重试的前提是「可能会成功」。
    if (!active || treeStopped) return;
    void refreshTree();
    const id = setInterval(() => void refreshTree(), 3000);
    return () => clearInterval(id);
  }, [active, refreshTree, treeStopped]);

  // ── 本机端口探测（沿用旧面板的真探逻辑）──
  const probePorts = useCallback(() => {
    QUICK_PORTS.forEach((p) => {
      invoke<boolean>("preview_port_alive", { port: p })
        .then((alive) => setPorts((m) => ({ ...m, [p]: alive })))
        .catch(() => {});
    });
  }, []);
  useEffect(() => {
    if (!active) return;
    probePorts();
    const id = setInterval(probePorts, 4000);
    return () => clearInterval(id);
  }, [active, probePorts]);

  /** 导航到新地址（地址栏 / 端口 / 历史）。 */
  const open = async (target: string) => {
    let u = target.trim();
    if (!u) return;
    // `file://` 也放行：AI 做出来的网页要能在**真浏览器**里打开（能点链接、跑 JS），
    // 而不是只在 iframe 里看一眼。不排除它的话，`file:///D:/x.html` 会被拼成
    // `http://file:///D:/x.html` —— 打开一个不存在的网站，还看不出为什么。
    // 后端 `browser.open` 本来就同时收 http(s):// 和 file://（browser.rs 的入参校验）。
    if (!/^(https?|file):\/\//.test(u)) u = "http://" + u;
    setErr(null);
    setBusy(true);
    try {
      await runAction("browser.open", { url: u });
      setUrl(u);
      setCurrent(u);
      remember(u);
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  // 宿主指定了地址就去开。**同一个地址不重复开**（openUrl 不变时 effect 不会再跑），
  // 否则每次父组件重渲染都会重新导航一遍，正在填的表单会被冲掉。
  const openRef = useRef(open);
  openRef.current = open;
  useEffect(() => {
    if (active && openUrl) void openRef.current(openUrl);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, openUrl]);

  const nav = async (action: "back" | "forward" | "reload") => {
    setErr(null);
    setBusy(true);
    try {
      await runAction(`browser.${action}`);
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 点交互树里的 @ref —— 和 AI 用同一个动作、同一份感知。 */
  const clickRef = async (rf: string) => {
    setErr(null);
    setBusy(true);
    try {
      // agent-browser 的快照引用必须是 @e1；裸 e1 会被当成 CSS selector。
      await runAction("browser.click", { ref: rf.startsWith("@") ? rf : `@${rf}` });
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 点直播画面 → 视口坐标 → browser.clickat（画面/视口比例换算，帧尺寸在比值中抵消）。 */
  const clickFrame = async (ev: React.MouseEvent<HTMLImageElement>) => {
    const img = ev.currentTarget;
    const rect = img.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const x = Math.round(((ev.clientX - rect.left) / rect.width) * viewport.width);
    const y = Math.round(((ev.clientY - rect.top) / rect.height) * viewport.height);
    setErr(null);
    setBusy(true);
    try {
      await runAction("browser.clickat", { x, y });
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 输入到当前聚焦元素（先点一下目标元素让 ta 聚焦，再在这输入）。 */
  const [typing, setTyping] = useState("");
  const doType = async () => {
    if (!typing.trim()) return;
    setErr(null);
    setBusy(true);
    try {
      await runAction("browser.type", { text: typing });
      setTyping("");
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const pressKey = async (key: string) => {
    setErr(null);
    setBusy(true);
    try {
      await runAction("browser.press", { key });
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const scroll = async (direction: string) => {
    setErr(null);
    setBusy(true);
    try {
      await runAction("browser.scroll", { direction, px: 300 });
      await refreshTree();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 在系统浏览器打开（登录/支付/复杂页面时跳出去）。 */
  const openExternal = () => {
    const u = current || url;
    if (!u) return;
    invoke("browser_nav", { label: `browser-live-${taskId}`, action: "external", url: u }).catch((e) =>
      setErr(String(e)),
    );
  };

  const retryAfterInstall = () => {
    setErr(null);
    setTreeStopped(null);
    reconnectsRef.current = 0;
    const epoch = ++streamEpochRef.current;
    if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
    reconnectTimerRef.current = null;
    if (wsRef.current) wsRef.current.close();
    wsRef.current = null;
    setWsReady(false);
    setWsFailed(false);
    setFrame(null);
    void connectStream(epoch);
  };

  /** 复用装机向导的受控安装流；完成后立刻用同一条 browser.* 动作验证 Chrome/直播。 */
  const installRuntime = async () => {
    if (installingRuntime) return;
    setInstallingRuntime(true);
    setErr(null);
    try {
      const result = await invoke<{ ok: boolean; error?: string }>("install_tool", { toolId: "agent-browser" });
      if (!result?.ok) throw new Error(result?.error || t("浏览器运行时安装没有完成"));

      // 不把 npm 成功当作可用：真实拉起 daemon 并抓一次快照，Chrome 未装/不能启动会在此显性失败。
      await runAction("browser.stream", {}, false);
      await refreshTree();
      retryAfterInstall();
    } catch (e) {
      setWsFailed(true);
      setErr(String(e));
    } finally {
      setInstallingRuntime(false);
    }
  };

  const NavBtn = ({
    icon: Icon,
    onClick,
    title,
    disabled,
  }: {
    icon: typeof ArrowLeft;
    onClick: () => void;
    title: string;
    disabled?: boolean;
  }) => (
    <button
      onClick={onClick}
      disabled={disabled}
      title={t(title)}
      className="w-7 h-7 grid place-items-center rounded-md text-ink-3 hover:text-ink-0 hover:bg-white/[0.06] disabled:opacity-30 disabled:hover:bg-transparent shrink-0"
    >
      <Icon size={14} />
    </button>
  );

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* 地址栏 */}
      <div className="flex items-center gap-2 h-10 px-3 border-b border-white/[0.06] shrink-0">
        <Globe size={14} className={cn("shrink-0", wsReady ? "text-accent" : "text-ink-3")} />
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onFocus={() => (urlFocusRef.current = true)}
          onBlur={() => {
            urlFocusRef.current = false;
            if (current) setUrl(current);
          }}
          onKeyDown={(e) => e.key === "Enter" && open(url)}
          placeholder={t("https://… 回车打开")}
          className="flex-1 h-7 rounded-md border border-white/[0.10] bg-bg-1 px-2.5 text-[12.5px] text-ink-1 placeholder:text-ink-4 outline-none focus:border-accent/50"
        />
        <button
          onClick={() => open(url)}
          disabled={busy}
          className="inline-flex items-center gap-1 h-7 px-3 rounded-md bg-accent hover:bg-accent-600 disabled:opacity-60 text-white text-[12px] shrink-0"
        >
          {busy ? <Loader2 size={13} className="animate-spin" /> : null}
          {busy ? t("操作中…") : t("打开")}
          {busy ? null : <ArrowUpRight size={13} />}
        </button>
      </div>

      {/* 导航条 */}
      <div className="flex items-center gap-0.5 h-9 px-2 border-b border-white/[0.06] shrink-0">
        <NavBtn icon={ArrowLeft} onClick={() => nav("back")} title="后退" disabled={busy} />
        <NavBtn icon={ArrowRight} onClick={() => nav("forward")} title="前进" disabled={busy} />
        <NavBtn icon={RotateCw} onClick={() => nav("reload")} title="刷新" disabled={busy} />
        <NavBtn icon={ExternalLink} onClick={openExternal} title="在系统浏览器打开（登录/支付时用）" disabled={!current && !url} />
        <div className="w-px h-4 bg-white/[0.08] mx-1" />
        <NavBtn icon={ScrollText} onClick={() => scroll("up")} title="向上滚动" disabled={busy} />
        <NavBtn icon={MousePointer2} onClick={() => scroll("down")} title="向下滚动" disabled={busy} />
        <div className="flex-1" />
        <span className={cn("text-[11px] shrink-0", wsReady ? "text-accent" : "text-ink-5")}>
          {wsReady ? t("直播中 · AI 和你在同一浏览器") : wsFailed ? t("连接失败") : t("连接中…")}
        </span>
      </div>

      {/* 直播画面 */}
      <div className="relative shrink-0 border-b border-white/[0.06] bg-black/30">
        {frame ? (
          <img
            src={frame}
            alt={t("浏览器实时画面")}
            onClick={clickFrame}
            className="w-full h-auto max-h-[38vh] object-contain cursor-pointer"
            title={t("点画面 = 在那个位置点击（先点一下目标元素，再在下方输入）")}
          />
        ) : (
          <div className="h-40 grid place-items-center text-ink-4 text-[12px] gap-2">
            {wsFailed ? (
              <Globe size={16} className="text-danger-400" />
            ) : (
              <Loader2 size={16} className="animate-spin" />
            )}
            {wsFailed ? (
              <>
                <span>{runtimeInstallNeeded ? t("浏览器会话没起来 —— 可一键安装并验证运行时") : t("浏览器会话暂时断开，可重新连接")}</span>
                <button
                  onClick={() => void (runtimeInstallNeeded ? installRuntime() : retryAfterInstall())}
                  disabled={installingRuntime}
                  className="h-7 px-2.5 rounded-md bg-accent hover:bg-accent-600 disabled:opacity-60 text-white text-[11px]"
                >
                  {installingRuntime ? t("正在安装…") : runtimeInstallNeeded ? t("安装浏览器运行时并验证") : t("重新连接")}
                </button>
                {runtimeInstallNeeded && <span className="text-[10.5px] text-ink-5">{t("固定版 agent-browser，复用本机 Chrome；不会下载浏览器内核")}</span>}
              </>
            ) : t("连接浏览器…")}
          </div>
        )}
        {busy && (
          <div
            data-on-dark
            className="absolute inset-0 grid place-items-center bg-black/20 text-white/90 text-[12px] backdrop-blur-[1px] pointer-events-none"
          >
            {t("操作中…")}
          </div>
        )}
      </div>

      {/* 交互树（AI 同一份感知） */}
      <div className="flex flex-col min-h-0 flex-1 border-b border-white/[0.06]">
        <div className="flex items-center gap-1.5 h-8 px-3 border-b border-white/[0.06] shrink-0">
          <MousePointer2 size={12} className="text-ink-4" />
          <span className="text-[11px] text-ink-4">{t("页面元素（点一下 = 点击它，AI 也看着这一份）")}</span>
          <span className="flex-1" />
          {treeBusy && <Loader2 size={11} className="animate-spin text-ink-5" />}
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto px-1 py-1">
          {/* 🔴 停下来必须**出声**。原来这条错被整个吞掉：面板一直显示「还没抓到页面元素」，
              而真相是它每 3 秒撞一次墙、撞了 6 小时。「什么都不说」比报错危险 ——
              客户会以为是自己没打开页面，而不是缺个内核。 */}
          {treeStopped ? (
            <div className="px-3 py-5 text-[11.5px] leading-relaxed">
              <div className="text-warning-700 dark:text-warning-400">{treeStopped.message}</div>
              {treeStopped.hint && <div className="text-ink-3 mt-1">{treeStopped.hint}</div>}
              <button
                onClick={() => void (runtimeInstallNeeded ? installRuntime() : retryAfterInstall())}
                disabled={installingRuntime}
                className="mt-2 h-6 px-2 rounded-md bg-white/[0.05] border border-white/[0.08] text-[11px] text-ink-2 hover:text-ink-0"
              >
                {installingRuntime ? t("正在安装…") : runtimeInstallNeeded ? t("安装浏览器运行时并验证") : t("重新连接")}
              </button>
            </div>
          ) : tree.length === 0 ? (
            <div className="text-center text-ink-5 text-[11.5px] py-6">
              {t("还没抓到页面元素 —— 打开一个页面就有了")}
            </div>
          ) : (
            tree.map((row, i) => (
              <button
                key={`${row.ref ?? row.role}-${i}`}
                onClick={() => row.ref && clickRef(row.ref)}
                disabled={!row.ref || busy}
                style={{ paddingLeft: 8 + row.indent * 12 }}
                title={row.ref ? t("点击 {ref}", { ref: row.ref }) : row.role}
                className={cn(
                  "w-full text-left px-2 py-[3px] rounded text-[11.5px] leading-snug truncate",
                  row.ref
                    ? "text-ink-2 hover:text-ink-0 hover:bg-accent/[0.12] disabled:opacity-40"
                    : "text-ink-5",
                )}
              >
                <span className={cn("mr-1.5", row.ref ? "text-accent/70 font-mono text-[10px]" : "")}>
                  {row.ref ? `@${row.ref}` : "·"}
                </span>
                <span className="opacity-70">{row.role}</span>
                {row.label ? <span> {row.label}</span> : null}
              </button>
            ))
          )}
        </div>
      </div>

      {/* 输入到聚焦元素 */}
      <div className="flex items-center gap-1.5 h-10 px-3 border-b border-white/[0.06] shrink-0">
        <Keyboard size={13} className="text-ink-4 shrink-0" />
        <input
          value={typing}
          onChange={(e) => setTyping(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && doType()}
          placeholder={t("先点一下页面里的输入框，再在这输入文字")}
          className="flex-1 h-7 rounded-md border border-white/[0.10] bg-bg-1 px-2.5 text-[12px] text-ink-1 placeholder:text-ink-4 outline-none focus:border-accent/50"
        />
        <button
          onClick={doType}
          disabled={busy || !typing.trim()}
          className="inline-flex items-center gap-1 h-7 px-2.5 rounded-md border border-white/[0.12] hover:bg-white/[0.06] text-ink-1 text-[11.5px] disabled:opacity-40 shrink-0"
        >
          {t("输入")}
        </button>
        <button
          onClick={() => pressKey("Enter")}
          disabled={busy}
          title={t("按回车")}
          className="inline-flex items-center gap-1 h-7 px-2 rounded-md border border-white/[0.12] hover:bg-white/[0.06] text-ink-1 text-[11.5px] disabled:opacity-40 shrink-0"
        >
          <CornerDownLeft size={12} />
        </button>
      </div>

      {/* 本机端口 + 最近打开 */}
      <div className="flex-1 min-h-0 overflow-y-auto px-4 py-3 space-y-4 shrink-0">
        <div>
          <div className="text-[11px] text-ink-5 mb-1.5">{t("本机开发端口")}</div>
          <div className="flex flex-wrap gap-2">
            {QUICK_PORTS.map((p) => {
              const alive = ports[p];
              return (
                <button
                  key={p}
                  onClick={() => open(`http://localhost:${p}`)}
                  disabled={busy}
                  title={alive ? t("在跑，点了直接开") : t("这个端口上没服务 —— 先让 AI 起开发服务器")}
                  className={cn(
                    "inline-flex items-center gap-1.5 h-8 px-3 rounded-full border text-[12px] font-mono disabled:opacity-50",
                    alive
                      ? "border-accent/40 bg-accent/[0.08] text-ink-1 hover:bg-accent/[0.15]"
                      : "border-white/[0.10] text-ink-4 hover:bg-white/[0.04]",
                  )}
                >
                  <span className={"dot " + (alive ? "dot-on" : "dot-off")} />
                  {p}
                </button>
              );
            })}
          </div>
        </div>

        {hist.length > 0 && (
          <div>
            <div className="text-[11px] text-ink-5 mb-1.5 flex items-center gap-1">
              <History size={11} /> {t("最近打开")}
            </div>
            <div className="space-y-0.5">
              {hist.map((h) => (
                <button
                  key={h}
                  onClick={() => open(h)}
                  disabled={busy}
                  className="w-full text-left px-2 py-1 rounded text-[11.5px] text-ink-3 hover:text-ink-0 hover:bg-white/[0.05] truncate font-mono disabled:opacity-50"
                  title={h}
                >
                  {h}
                </button>
              ))}
            </div>
          </div>
        )}

        {err && (
          <div className="text-danger-400 text-[12px] text-center bg-danger-400/[0.06] rounded-md px-3 py-2">
            {err}
          </div>
        )}
      </div>
    </div>
  );
}
