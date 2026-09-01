/**
 * 进阶 / App 版 —— 桌面 App 工具（Hermes 桌面版 / ClawX）的「装机半自动 + 配置教程化」页。
 *
 * 产品取舍（2026-06-19 定）：GUI app 的模型配置**不做全自动**（实践证明 app 自动写配置坑太多
 * —— 配置文件格式/路径/内存副本各种时机问题，切了没反应是头号售后单）。改为：
 *  ① 安装 = 下一步下一步（installer 自己跑，我们只帮下载 + 拉起）；
 *  ② 配置 = 给「一键复制 Key / Base URL」+ 图文步骤，客户自己粘进 app 的模型设置（当场看见生效，可自查）。
 * 一键自动切换仍保留给 CLI 工具（在「AI 设置」页），那层可靠；GUI app 只到「复制 + 教程」为止。
 *
 * 自包含（符合「模块随时整块拔插」铁律）：本页自己 invoke 后端命令，删本页只需动 App.tsx + Sidebar。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  Check,
  Copy,
  Download,
  ExternalLink,
  Info,
  KeyRound,
  Link2,
  Loader2,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Trash2,
  Wand2,
} from "lucide-react";
import { ToolIcon } from "./components/ToolIcon";
import { WalletCard } from "./components/WalletCard";
import { useI18n } from "./i18n";
import { cn } from "./lib/cn";
import { askConfirm } from "./lib/confirm";
import { mergeModels } from "./lib/models";
import type { DeviceKey as DeviceKeyFull } from "./lib/types";

/** 虾盘云国内可达端点（OpenAI 兼容格式）。**必须用 .org.cn** —— 裸 api.u-claw.org 国内 GFW SNI reset。 */
const XIAPAN_BASE_URL = "https://api.u-claw.org.cn/v1";
/** 默认模型 —— 必须跟 providers.rs 虾盘云 preset 的 `model` 一致（客户装好在用的就是它）。
 *  这里曾经写 `deepseek-v4-pro`：客户从这页复制一段配置粘到第三方工具，拿到的就是个
 *  跟 U-King 里不一样的模型，账单也对不上。下面凭据块里仍可下拉换成 Claude/GPT 等再复制。 */
const XIAPAN_MODEL = "deepseek-v4-flash";

type ToolInfo = {
  id: string;
  name: string;
  summary: string;
  installed: boolean;
  launch_app: string;
};

// 本文件原来自己写了一份 `{ key, charged }` —— 加 legacy_balance_unrecoverable 时
// 当场 TS2339。同一事实存在几份就会漂几份（宪法第 8 条）。改成基于 lib/types 那份，
// 只在这里加上「可能还没拿到」的 null。
type DeviceKey = DeviceKeyFull | null;

/** 复用的「一键复制」按钮（带「已复制」反馈）。CopyField 与模型行共用。 */
function CopyBtn({ value, label, onToast }: { value: string; label: string; onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [done, setDone] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setDone(true);
      onToast(t("已复制{label}", { label: t(label) }));
      window.setTimeout(() => setDone(false), 1800);
    } catch {
      onToast(t("复制失败，请手动选中复制"));
    }
  };
  return (
    <button
      onClick={copy}
      disabled={!value}
      className={cn(
        "inline-flex items-center gap-1 px-2.5 h-7 rounded-md border text-[11.5px] transition-colors shrink-0",
        done
          ? "text-success-400 border-success-500/30"
          : "text-ink-1 border-white/[0.10] hover:bg-white/[0.04] disabled:opacity-40",
      )}
    >
      {done ? <Check size={12} /> : <Copy size={12} />}
      {done ? t("已复制") : t("复制")}
    </button>
  );
}

/** 通用「一键复制」行：左标签 + 单行值 + 复制按钮。Key/BaseURL 共用。 */
function CopyField({
  label,
  value,
  icon: Icon,
  onToast,
  mono = true,
}: {
  label: string;
  value: string;
  icon: typeof KeyRound;
  onToast: (s: string) => void;
  mono?: boolean;
}) {
  const { t } = useI18n();
  return (
    <div className="flex items-center gap-2 rounded-lg border border-white/[0.08] bg-bg-1/50 px-3 py-2">
      <Icon size={14} className="text-accent shrink-0" />
      <div className="min-w-0 flex-1">
        <div className="text-[10.5px] text-ink-4">{t(label)}</div>
        <div className={cn("truncate text-[12.5px] text-ink-1", mono && "font-mono")} title={value}>
          {value || t("（检测中…）")}
        </div>
      </div>
      <CopyBtn value={value} label={label} onToast={onToast} />
    </div>
  );
}

/** 一组「复制 Key / Base URL / 模型」—— Hermes 与 ClawX 教程共用。
 *  模型可**下拉换不同 id**（默认满血 deepseek-v4-pro，也可选 Claude/GPT 等）再复制。 */
function CredentialBlock({
  deviceKey,
  onToast,
  onRecharge,
  onDeviceKeyChange,
}: {
  deviceKey: DeviceKey;
  onToast: (s: string) => void;
  onRecharge: () => void;
  onDeviceKeyChange?: (dk: DeviceKeyFull) => void;
}) {
  // 同 ProviderSwitch：服务端有什么才列什么，拉不到就退回本地清单（见 mergeModels 注释）。
  const [liveModels, setLiveModels] = useState<string[] | null>(null);
  useEffect(() => {
    const key = deviceKey?.key;
    if (!key) return;
    let alive = true;
    invoke<string[]>("list_remote_models", { providerId: "xiapan", apiKey: key })
      .then((ids) => alive && setLiveModels(Array.isArray(ids) && ids.length ? ids : null))
      .catch(() => alive && setLiveModels(null));
    return () => { alive = false; };
  }, [deviceKey?.key]);
  const modelGroups = mergeModels(liveModels);
  const { t } = useI18n();
  const [model, setModel] = useState(XIAPAN_MODEL);

  return (
    <div className="space-y-2">
      {/* Key 本身、余额、换一把、填一把 —— 全部归 WalletCard（唯一一份实现）。
          本页原来自己写了一遍 rotate/adopt，且用的是 `window.confirm`：Tauri 把它换成了
          返回 Promise 的版本，`!Promise` 恒 false —— 那两处确认框等于没问就干。
          留在这里的只是「教程需要的那几串可复制文本」：Base URL 和模型 id。 */}
      <WalletCard
        deviceKey={deviceKey}
        onDeviceKeyChange={onDeviceKeyChange}
        onRecharge={onRecharge}
        onToast={onToast}
      />
      <CopyField label="接口地址 Base URL" value={XIAPAN_BASE_URL} icon={Link2} onToast={onToast} />
      {/* 模型：先在下拉里选一个，再点复制（不确定就用默认的 deepseek-v4-flash） */}
      <div className="flex items-center gap-2 rounded-lg border border-white/[0.08] bg-bg-1/50 px-3 py-2">
        <Sparkles size={14} className="text-accent shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="text-[10.5px] text-ink-4">{t("模型 Model（选一个再复制）")}</div>
          <select
            value={model}
            onChange={(e) => setModel(e.target.value)}
            className="w-full bg-transparent outline-none text-[12.5px] text-ink-1 font-mono cursor-pointer -ml-0.5"
          >
            {modelGroups.map((g) => (
              <optgroup key={g.group} label={g.group}>
                {g.items.map((m) => (
                  <option key={m.id} value={m.id} className="bg-bg-2">
                    {m.id}
                    {m.recommend ? t("  ★推荐") : ""}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </div>
        <CopyBtn value={model} label="模型" onToast={onToast} />
      </div>
    </div>
  );
}

/** 编号步骤列表。 */
function Steps({ items }: { items: React.ReactNode[] }) {
  return (
    <ol className="space-y-2">
      {items.map((it, i) => (
        <li key={i} className="flex gap-2.5">
          <span className="grid place-items-center w-5 h-5 rounded-full bg-accent/15 text-accent text-[11px] font-bold shrink-0">
            {i + 1}
          </span>
          <div className="text-[12.5px] leading-relaxed text-ink-2 pt-0.5">{it}</div>
        </li>
      ))}
    </ol>
  );
}

/** 一条足迹（后端 cleanup::FootprintItem 的镜像）。 */
type FootprintItem = {
  id: string;
  group: "core" | "config" | "aitool";
  name: string;
  detail: string;
  safe: boolean;
  warn: string;
};

/** 三档分组的展示元数据（标题 / 副标题 / 色调）。 */
const GROUP_META: Record<
  FootprintItem["group"],
  { title: string; hint: string; icon: typeof ShieldCheck; tone: "safe" | "warn" }
> = {
  core: { title: "U-King 自己装的", hint: "安全 · 默认清除", icon: ShieldCheck, tone: "safe" },
  config: {
    title: "U-King 改过的配置（清除 = 还原到改动前）",
    hint: "安全 · 默认清除 · 不会清空你的 ~/.claude 等目录",
    icon: RefreshCw,
    tone: "safe",
  },
  aitool: {
    title: "U-King 帮你装的 AI 工具 / 厨具",
    hint: "可能你之前就有 · 默认不删，逐个确认",
    icon: ShieldAlert,
    tone: "warn",
  },
};
const GROUP_ORDER: FootprintItem["group"][] = ["core", "config", "aitool"];

/**
 * 安全卸载 / 逐项清理面板 —— 诚实扫描本机上 U-King 的全部足迹（后端 cleanup.rs），
 * 分三档勾选：core/config 默认勾（安全，清除=还原），aitool 默认不勾（帮你装的工具本体，附警告）。
 * 支持「逐项清除」与「清除所选」。含 uking-home（U-King 本体）时 = 彻底卸载并关闭。
 * 自包含：只靠 onToast 与后端 command，删本段只动本文件。
 */
export function SafeUninstall({ onToast, demo = false }: { onToast: (s: string) => void; demo?: boolean }) {
  const { t } = useI18n();
  const [items, setItems] = useState<FootprintItem[] | null>(null);
  const [sel, setSel] = useState<Record<string, boolean>>({});
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [confirm, setConfirm] = useState(false);
  // 独立演示版默认保留资料：先归档到“文档/U-King 演示保留数据”，再卸载，
  // 这样下一次安装仍是干净状态，不会因为旧目录被检测成“已安装”。
  const [preserveUserData, setPreserveUserData] = useState(demo);

  const scan = useRef(async () => {});
  scan.current = async () => {
    setScanning(true);
    try {
      const list = await invoke<FootprintItem[]>("cleanup_scan");
      setItems(list);
      // 主程序默认只勾安全项；独立演示清场版默认全选，仍会在真正执行前展示确认。
      const s: Record<string, boolean> = {};
      list.forEach((it) => (s[it.id] = demo || it.safe));
      setSel(s);
    } catch (e) {
      onToast(String(e));
      setItems([]);
    } finally {
      setScanning(false);
    }
  };
  useEffect(() => {
    scan.current();
    const p = listen<string>("uking:cleanup_progress", (e) =>
      setLogs((l) => [...l.slice(-120), e.payload]),
    );
    return () => {
      p.then((un) => un());
    };
  }, []);

  const toggle = (id: string) => setSel((s) => ({ ...s, [id]: !s[id] }));
  const selectedIds = items ? items.filter((it) => sel[it.id]).map((it) => it.id) : [];
  const selAiTools = items ? items.filter((it) => sel[it.id] && it.group === "aitool").length : 0;
  const homeSelected = selectedIds.includes("uking-home");

  const run = async (ids: string[]) => {
    if (!ids.length || busy) return;
    setBusy(true);
    setLogs([]);
    try {
      const willExit = await invoke<boolean>("cleanup_run", {
        ids,
        preserveUserData: demo && preserveUserData,
      });
      if (willExit) {
        onToast(t("正在彻底卸载并清理，U-King 即将关闭…"));
      } else {
        onToast(t("已清除所选项"));
        await scan.current(); // 刷新剩余足迹
      }
    } catch (e) {
      onToast(t("清理失败：{e}", { e: String(e) }));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  /** 逐项清除（单条）。aitool / uking-home 二次确认（会真卸载 / 会关闭）。 */
  const removeOne = async (it: FootprintItem) => {
    if (busy) return;
    if (it.id === "uking-home") {
      if (!(await askConfirm(t("这会删除 U-King 本体并关闭程序，确定？")))) return;
    } else if (it.group === "aitool") {
      if (
        !(await askConfirm(
          t("确定卸载「{name}」？若你之前自己装过，这会真的删掉它。", { name: it.name }),
        ))
      )
        return;
    }
    void run([it.id]);
  };

  const grouped = (g: FootprintItem["group"]) => (items ?? []).filter((it) => it.group === g);
  const empty = items !== null && items.length === 0;

  return (
    <section className="rounded-card border border-danger-500/25 bg-danger-500/[0.04] overflow-hidden">
      {/* 页头 */}
      <div className="flex items-center gap-2 px-5 py-3.5 border-b border-danger-500/15">
        <Trash2 size={16} className="text-danger-400 shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold text-ink-0">{t("安全卸载 / 逐项清理")}</div>
          <div className="text-[11px] text-ink-4">
            {demo
              ? t("演示清场模式：已默认选中检测到的所有工具与 U-King 足迹；请确认后执行。")
              : t("列出 U-King 在这台电脑上留下的全部东西，逐项可删、配置可还原到改动前，或一键彻底卸载。")}
          </div>
        </div>
        <button
          onClick={() => scan.current()}
          disabled={scanning || busy}
          className="inline-flex items-center gap-1 px-2.5 h-7 rounded-md border border-white/[0.10] text-[11.5px] text-ink-2 hover:bg-white/[0.04] disabled:opacity-50 shrink-0"
        >
          <RefreshCw size={12} className={scanning ? "animate-spin" : ""} /> {t("重新扫描")}
        </button>
      </div>

      <div className="px-5 py-4 space-y-4">
        {scanning && items === null ? (
          <div className="flex items-center gap-2 text-[12px] text-ink-3 py-4">
            <Loader2 size={14} className="animate-spin" /> {t("正在扫描本机足迹…")}
          </div>
        ) : empty ? (
          <div className="flex items-center gap-2 text-[12px] text-success-400 py-4">
            <Check size={14} /> {t("本机没有检测到 U-King 的任何足迹（干净）。")}
          </div>
        ) : (
          GROUP_ORDER.map((g) => {
            const list = grouped(g);
            if (!list.length) return null;
            const meta = GROUP_META[g];
            const Icon = meta.icon;
            return (
              <div key={g}>
                <div className="flex items-center gap-1.5 mb-1.5">
                  <Icon size={13} className={meta.tone === "warn" ? "text-warning-400" : "text-accent"} />
                  <span className="text-[12px] font-semibold text-ink-1">{t(meta.title)}</span>
                  <span className="text-[10.5px] text-ink-4">· {t(meta.hint)}</span>
                </div>
                <div className="space-y-1.5">
                  {list.map((it) => (
                    <label
                      key={it.id}
                      className={cn(
                        "flex items-start gap-2.5 rounded-lg border px-3 py-2 cursor-pointer transition-colors",
                        sel[it.id]
                          ? "border-accent/30 bg-accent/[0.05]"
                          : "border-white/[0.07] bg-bg-1/40 hover:bg-white/[0.03]",
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={!!sel[it.id]}
                        onChange={() => toggle(it.id)}
                        disabled={busy}
                        className="mt-0.5 accent-accent shrink-0"
                      />
                      <div className="min-w-0 flex-1">
                        <div className="text-[12.5px] text-ink-1 font-medium">{t(it.name)}</div>
                        <div className="text-[11px] text-ink-4 break-all">{it.detail}</div>
                        {it.warn && (
                          <div className="mt-0.5 inline-flex items-start gap-1 text-[10.5px] text-warning-400">
                            <AlertTriangle size={11} className="mt-[1px] shrink-0" /> {t(it.warn)}
                          </div>
                        )}
                      </div>
                      <button
                        onClick={(e) => {
                          e.preventDefault();
                          void removeOne(it);
                        }}
                        disabled={busy}
                        title={t("单独清除这一项")}
                        className="inline-flex items-center gap-1 px-2 h-6 rounded-md border border-white/[0.10] text-[10.5px] text-ink-3 hover:text-danger-400 hover:border-danger-500/40 disabled:opacity-40 shrink-0"
                      >
                        <Trash2 size={11} /> {t("清除")}
                      </button>
                    </label>
                  ))}
                </div>
              </div>
            );
          })
        )}

        {demo && !empty && (
          <label className="flex items-start gap-2.5 rounded-lg border border-success-500/25 bg-success-500/[0.06] px-3.5 py-3 cursor-pointer">
            <input
              type="checkbox"
              checked={preserveUserData}
              onChange={() => setPreserveUserData((v) => !v)}
              disabled={busy}
              className="mt-0.5 accent-success-500 shrink-0"
            />
            <span className="min-w-0">
              <span className="block text-[12.5px] font-medium text-ink-1">保留用户历史数据（推荐）</span>
              <span className="block mt-0.5 text-[11px] leading-relaxed text-ink-3">
                清场前会备份 U-King 的任务、作图和视频记录，以及 OpenClaw / ClawX / Hermes / Codex 桌面版的本地数据到“文档 / U-King 演示保留数据”。重新安装仍从干净状态开始。
              </span>
            </span>
          </label>
        )}

        {/* 进度日志 */}
        {logs.length > 0 && (
          <div className="rounded-lg border border-white/[0.08] bg-bg-1/60 px-3 py-2 max-h-32 overflow-auto">
            {logs.map((l, i) => (
              <div key={i} className="text-[11px] font-mono text-ink-3 leading-relaxed break-all">
                {l}
              </div>
            ))}
          </div>
        )}

        {/* 底部操作条 */}
        {!empty && items !== null && (
          <div className="pt-1 border-t border-white/[0.06]">
            {!confirm ? (
              <div className="flex flex-wrap items-center gap-2 pt-3">
                <button
                  onClick={() => setConfirm(true)}
                  disabled={busy || selectedIds.length === 0}
                  className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-card bg-danger-500 text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-40"
                >
                  {busy ? <Loader2 size={13} className="animate-spin" /> : <Trash2 size={13} />}
                  {homeSelected
                    ? demo
                      ? t("一键清场并关闭")
                      : t("彻底卸载并关闭")
                    : t("清除所选（{n} 项）", { n: String(selectedIds.length) })}
                </button>
                <span className="text-[11px] text-ink-4">
                  {selAiTools > 0 && (
                    <span className="text-warning-400">
                      {t("含 {n} 个 AI 工具本体", { n: String(selAiTools) })} ·{" "}
                    </span>
                  )}
                  {demo ? t("可取消不想处理的项目；配置项只还原 U-King 的改动") : t("勾选后清除；配置项会还原到改动前")}
                </span>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2 pt-3">
                <span className="text-[12px] text-ink-2">
                  {homeSelected
                    ? demo
                      ? t("确认演示清场？将卸载勾选的工具、删除 ~/.uking，并关闭本工具。")
                      : t("确认彻底卸载？将删除 ~/.uking 并关闭 U-King。")
                    : t("确认清除所选 {n} 项？", { n: String(selectedIds.length) })}
                  {selAiTools > 0 && (
                    <b className="text-warning-400">
                      {t("（含 {n} 个会真卸载的 AI 工具/厨具）", { n: String(selAiTools) })}
                    </b>
                  )}
                  {demo && preserveUserData && (
                    <span className="text-success-400">{t("（会先保留用户历史数据）")}</span>
                  )}
                </span>
                <button
                  data-action-id="runtime.footprint.remove"
                  onClick={() => void run(selectedIds)}
                  disabled={busy}
                  className="px-3 py-1.5 rounded-card bg-danger-500 text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-50"
                >
                  {busy ? t("正在清理…") : t("确认清除")}
                </button>
                <button
                  onClick={() => setConfirm(false)}
                  disabled={busy}
                  className="px-3 py-1.5 rounded-card border border-white/10 text-[12px] text-ink-3 hover:bg-white/[0.04] disabled:opacity-50"
                >
                  {t("取消")}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </section>
  );
}

export function Advanced({
  deviceKey,
  onToast,
  onRecharge,
  onDeviceKeyChange,
}: {
  deviceKey: DeviceKey;
  onToast: (s: string) => void;
  /** 充值只有 App 那一条（开页 + 定时回查到账），本页不自己开 URL。 */
  onRecharge: () => void;
  /** 钱包换/填 Key 后把新 DeviceKey 交回 App，否则别的页面还显示旧的。 */
  onDeviceKeyChange?: (dk: DeviceKeyFull) => void;
}) {
  const { t } = useI18n();
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [hermesProgress, setHermesProgress] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [clawxConfig, setClawxConfig] = useState<string | null>(null);
  const [clawxBusy, setClawxBusy] = useState(false);

  // 「还没查完」和「查过了、没装」是两回事（测试报告 #011 的连带修正）。
  // list_tools 会真起 4 个进程跑 `--version`（本机实测 claude 272ms / codex 303ms /
  // openclaw 316ms / **hermes 2331ms**，合计约 3.2 秒）。它以前是同步 command，
  // 卡的是界面；改成异步后界面不卡了，但如果这段时间照着空数组渲染，
  // 客户会先看到「未安装」再突然变成「已安装」—— 那是在说一句我们当时还不知道的话。
  const [toolsLoading, setToolsLoading] = useState(true);
  const refresh = useRef(async () => {});
  refresh.current = async () => {
    setToolsLoading(true);
    try {
      const t = await invoke<ToolInfo[]>("list_tools").catch(() => []);
      setTools(t);
    } finally {
      setToolsLoading(false);
    }
  };
  useEffect(() => {
    refresh.current();
  }, []);

  const hermes = tools.find((t) => t.id === "hermes-app");
  const clawx = tools.find((t) => t.id === "clawx");
  const hermesInstalled = !!hermes?.installed;
  const clawxInstalled = !!clawx?.installed;

  const installHermes = async () => {
    if (installing) return;
    setInstalling(true);
    setHermesProgress(t("正在下载 Hermes 安装器…"));
    const un = await listen<string>("uking:hermes_progress", (e) => setHermesProgress(e.payload));
    try {
      const msg = await invoke<string>("install_hermes_app");
      onToast(msg);
      // 安装器是用户手动点下一步，装完时机不定 —— 稍候再刷新一次检测状态。
      window.setTimeout(() => refresh.current(), 4000);
    } catch (e) {
      onToast(String(e));
      // 下载失败（国际站慢/被墙）→ 回退打开官网下载页让用户自己下。
      const page = await invoke<string>("hermes_download_page").catch(
        () => "https://hermes-agent.nousresearch.com/",
      );
      await openUrl(page).catch(() => {});
    } finally {
      un();
      setHermesProgress(null);
      setInstalling(false);
    }
  };

  const launchHermes = () => {
    invoke("launch_app", { app: "hermes-app" })
      .then(() => onToast(t("正在打开 Hermes…")))
      .catch(() => onToast(t("没找到 Hermes，请先安装，或从开始菜单打开")));
  };
  const launchClawx = () => {
    invoke("launch_app", { app: "clawx" })
      .then(() => onToast(t("正在打开 ClawX…")))
      .catch(() => onToast(t("打不开 ClawX —— 可能还没装。请到「我的 AI」→ 找到 ClawX → 一键安装")));
  };

  /** 托管式一键配好 ClawX：（运行中先确认）关闭 → 写两层配置 → 重启。后端做了派生键根治，不再冒孤儿。 */
  const autoConfigClawx = async () => {
    if (clawxBusy) return;
    const running = await invoke<boolean>("clawx_running").catch(() => false);
    if (running) {
      const ok = await askConfirm(
        t("需要临时关闭 ClawX 来写入配置（对话已自动保存），完成后会自动重启。是否继续？"),
      );
      if (!ok) return;
    }
    setClawxBusy(true);
    setClawxConfig(running ? t("正在关闭 ClawX…") : t("正在写入配置…"));
    const un = await listen<string>("uking:clawx_config", (e) => setClawxConfig(e.payload));
    try {
      await invoke("apply_clawx_managed", {
        providerId: "xiapan",
        apiKey: deviceKey?.key ?? null,
        model: XIAPAN_MODEL,
      });
      onToast(running ? t("已把虾盘云配进 ClawX，正在重启…") : t("已把虾盘云配进 ClawX"));
      window.setTimeout(() => refresh.current(), 3000);
    } catch (e) {
      onToast(t("自动配置失败：") + String(e) + t("（可照下面手动配）"));
    } finally {
      un();
      setClawxConfig(null);
      setClawxBusy(false);
    }
  };

  return (
    <div className="space-y-5">
      {/* 页头 + 取舍说明 */}
      <section className="rounded-card border border-white/[0.08] bg-bg-2 px-5 py-4">
        <div className="flex items-center gap-2 mb-1.5">
          <Sparkles size={17} className="text-accent" />
          <h2 className="text-[15px] font-semibold text-ink-0">{t("进阶 · 桌面 App 版")}</h2>
          <span className="text-[11px] text-ink-4">{t("给想用图形界面的高级用户")}</span>
        </div>
        <p className="text-[12px] leading-relaxed text-ink-3">
          {t("这里是 ")}<b className="text-ink-1">Hermes / ClawX</b>{t(" 等桌面 App。装机我们帮你「下一步下一步」装好；")}
          <b className="text-ink-1">{t("模型配置请照下面教程，自己把 Key 复制进 App 的设置里")}</b>
          {t("—— App 的自动配置坑多（切了常没反应），手动粘一次最稳，你也能当场看到生效。")}
          <br />
          <span className="text-ink-4">{t("（命令行工具的「一键切换模型」仍在「AI 设置」页，那层可靠、不受此影响。）")}</span>
        </p>
      </section>

      {/* Hermes 桌面版入口已隐藏（2026-07-08 产品决策）：Hermes 改为只用 TUI —— 点「启动」即进
          终端对话（像 Claude Code）、自动配好虾盘云内部模型；桌面 App 装机坑多、与 TUI 能力重复，
          且客户会二选一犯难。代码整段保留，把下面的 false 改回 true 即可恢复桌面 App 入口。 */}
      {false && (
      <section className="rounded-card border border-white/[0.08] bg-bg-2/70 overflow-hidden">
        <div className="flex items-center gap-3 px-5 py-4 border-b border-white/[0.06]">
          <span className="grid place-items-center w-11 h-11 rounded-lg bg-bg-3 shrink-0">
            <ToolIcon tool="hermes" size={28} active={hermesInstalled} />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[14px] font-semibold text-ink-0">{t("Hermes 桌面版（Nous 官方）")}</div>
            <div className="text-[11.5px] text-ink-3">
              {toolsLoading ? (
                <span className="text-ink-4">{t("检测中…")}</span>
              ) : hermesInstalled ? (
                <span className="text-success-400">{t("✓ 已安装")}</span>
              ) : (
                t("Nous Research 自进化 AI 智能体 · 官方图形版")
              )}
            </div>
          </div>
          {hermesInstalled ? (
            <button
              onClick={launchHermes}
              className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shrink-0"
            >
              <Sparkles size={14} /> {t("打开 Hermes")}
            </button>
          ) : (
            <button
              onClick={installHermes}
              disabled={installing}
              className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shrink-0 disabled:opacity-60"
            >
              <Download size={14} /> {installing ? t("安装中…") : t("下载安装 Hermes")}
            </button>
          )}
        </div>

        {hermesProgress && (
          <div className="flex items-center gap-2 px-5 py-2.5 bg-accent/[0.06] text-[12px] text-ink-1 border-b border-white/[0.06]">
            <Download size={13} className="text-accent animate-pulse shrink-0" />
            <span className="truncate">{hermesProgress}</span>
          </div>
        )}

        <div className="px-5 py-4 space-y-3.5">
          <div className="flex items-center gap-1.5 text-[12px] text-ink-2">
            <Info size={13} className="text-accent" />
            {t("装好后，照下面 4 步把虾盘云接进 Hermes（一次配好，永久生效）：")}
          </div>
          <Steps
            items={[
              <>{t("装好后点上方 ")}<b className="text-ink-1">{t("「打开 Hermes」")}</b>{t("，进入 Hermes 主界面。")}</>,
              <>{t("在 Hermes 里打开 ")}<b className="text-ink-1">{t("设置（Settings）→ 供应商（Providers）")}</b>{t("，新增一个 ")}<b className="text-ink-1">{t("OpenAI 兼容")}</b>{t(" 供应商。")}</>,
              <>{t("把下面三项")}<b className="text-ink-1">{t("复制粘贴")}</b>{t("进去（点右边「复制」按钮，再到 Hermes 对应输入框粘贴）：")}</>,
              <>{t("填好后")}<b className="text-ink-1">{t("保存并选中该供应商/模型")}</b>{t("，回到对话框发一句话测试，能回话即成功。")}</>,
            ]}
          />
          <CredentialBlock deviceKey={deviceKey} onToast={onToast} onRecharge={onRecharge} onDeviceKeyChange={onDeviceKeyChange} />
          <div className="flex items-center gap-3 pt-0.5 text-[11px] text-ink-4">
            <button
              onClick={() => openUrl("https://hermes-agent.nousresearch.com/").catch(() => {})}
              className="inline-flex items-center gap-1 hover:text-ink-2"
            >
              <ExternalLink size={11} /> {t("Hermes 官网")}
            </button>
            {!deviceKey?.key && <span>{t("· 内置 Key 检测中，稍候即可复制")}</span>}
            {deviceKey?.charged === false && (
              <span className="text-accent">{t("· 内置 Key 未充值，去「AI 设置」充值后可用")}</span>
            )}
          </div>
        </div>
      </section>
      )}

      {/* ClawX 配置教程（ClawX 安装仍在「我的 AI」，这里只放配置教程） */}
      <section className="rounded-card border border-white/[0.08] bg-bg-2/70 overflow-hidden">
        <div className="flex items-center gap-3 px-5 py-4 border-b border-white/[0.06]">
          <span className="grid place-items-center w-11 h-11 rounded-lg bg-bg-3 shrink-0">
            <ToolIcon tool="clawx" size={28} active={clawxInstalled} />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[14px] font-semibold text-ink-0">{t("ClawX（图形版 AI）· 复制 Key 接虾盘云（3 步）")}</div>
            <div className="text-[11.5px] text-ink-3">
              {toolsLoading ? (
                <span className="text-ink-4">{t("检测中…")}</span>
              ) : clawxInstalled ? (
                <span className="text-success-400">{t("✓ 已安装")}</span>
              ) : (
                t("在「我的 AI」可一键安装；装好后照这里把虾盘云填进去")
              )}
            </div>
          </div>
          {clawxInstalled && (
            <div className="flex items-center gap-2 shrink-0">
              <button
                data-action-id="runtime.clawx.apply_managed"
                onClick={autoConfigClawx}
                disabled={clawxBusy}
                title={t("自动关闭 ClawX → 写入虾盘云配置 → 重启 ClawX")}
                className="inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 shrink-0 disabled:opacity-60"
              >
                {clawxBusy ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}
                {clawxBusy ? t("配置中…") : t("一键配好 ClawX")}
              </button>
              <button
                onClick={launchClawx}
                className="inline-flex items-center gap-1.5 px-3 h-9 rounded-lg border border-white/[0.10] text-ink-1 text-[13px] hover:bg-white/[0.04] shrink-0"
              >
                <Sparkles size={14} /> {t("打开")}
              </button>
            </div>
          )}
        </div>

        {clawxConfig && (
          <div className="flex items-center gap-2 px-5 py-2.5 bg-accent/[0.06] text-[12px] text-ink-1 border-b border-white/[0.06]">
            <Loader2 size={13} className="text-accent animate-spin shrink-0" />
            <span className="truncate">{clawxConfig}</span>
          </div>
        )}

        <div className="px-5 py-4 space-y-3.5">
          <div className="flex items-center gap-1.5 text-[12px] text-ink-2">
            <Info size={13} className="text-accent" />
            {clawxInstalled ? (
              <>{t("推荐点上方 ")}<b className="text-ink-1">{t("「一键配好 ClawX」")}</b>{t("（自动关闭→写入→重启）；若没成功，照下面 3 步手动配：")}</>
            ) : (
              <>{t("想自己换模型时，照这 3 步把虾盘云接进 ClawX：")}</>
            )}
          </div>
          <Steps
            items={[
              <>{t("打开 ClawX，进 ")}<b className="text-ink-1">{t("设置（Settings）→ 模型 / 供应商（Models / Providers）")}</b>{t("，点「添加供应商（Add Provider）」。")}</>,
              <>{t("接入类型选 ")}<b className="text-ink-1">{t("OpenAI 兼容（OpenAI Compatible）")}</b>{t("，把下面的接口地址、API Key 粘进去；模型填下面")}<b className="text-ink-1">{t("选好的那个")}</b>{t("（不确定就用默认 ")}<span className="font-mono text-ink-1">{XIAPAN_MODEL}</span>{t("）。")}</>,
              <>{t("保存后在 ClawX 里")}<b className="text-ink-1">{t("选中这个供应商")}</b>{t("即可对话。")}<b className="text-ink-1">{t("填完记得重启一次 ClawX")}</b>{t("——它只在启动时读取配置，不重启常常「切了没反应」。")}</>,
            ]}
          />
          <CredentialBlock deviceKey={deviceKey} onToast={onToast} onRecharge={onRecharge} onDeviceKeyChange={onDeviceKeyChange} />
        </div>
      </section>

      {/* 安全卸载 / 逐项清理 —— 诚实列出全部足迹，逐项可删、配置可还原，或彻底卸载 */}
      <SafeUninstall onToast={onToast} />
    </div>
  );
}
