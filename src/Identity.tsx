/**
 * 「让 AI 认识 U-King」—— 身份 + 给 AI 的说明书（llms.txt）+ 往别家记忆文件插指针的开关。
 * （1.0.3 从「我的 U-King」改名：那个名字没说出它对这台电脑做了什么，用户看不懂就不会去关它。）
 *
 * **这页解决的问题**：U-King 的能力早就是机器可读的了（影核 49 个动作），但装在同一台
 * 机器上的**别家 AI**（客户自己的 Claude Code / Codex / 任何东西）根本不知道我们存在。
 * 这页把动作表编译成 `~/.uking/llms.txt`，让任何 AI 一读就知道这台机器上能调什么。
 *
 * **这页卖的是信任**，所以三件事必须做到：
 *  1. `ready` / `blockers` 健康横幅 —— 「生成了」和「AI 真能发现」是两回事，别只报前者
 *  2. 说明书正文**可当场翻开看** —— 说「不会泄漏你的 Key」不如让他自己搜一遍
 *  3. 明文 / 私密**在界面上就分开**，不是只写在文档里
 *
 * 【独立可插拔】删掉只动 App.tsx（去 import + 分支）+ Sidebar.tsx（去 TabId + 入口）。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IdCard, Loader2, RefreshCw, FolderOpen, Eye, Check, AlertCircle, KeyRound, Trash2 } from "lucide-react";
import { useI18n } from "./i18n";
import { cn } from "./lib/cn";

type SecretSummary = { name: string; configured: boolean };
type Discovery = {
  id: string;
  label: string;
  path: string;
  /** 这台机器上装没装这个工具 */
  eligible: boolean;
  /** 指针挂上了没 */
  linked: boolean;
  reason: string | null;
};
type Status = {
  spec_version: string;
  ready: boolean;
  blockers: string[];
  discovery: Discovery[];
  linked_count: number;
  identity: { name: string; owner: string; role: string; traits: Record<string, unknown>; notes: string };
  files: { identity: string; secrets: string; llms: string; llms_full: string; logs_dir: string };
  published: boolean;
  secrets: SecretSummary[];
};

export function Identity({ onToast }: { onToast?: (m: string) => void }) {
  const { t } = useI18n();
  const [st, setSt] = useState<Status | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);

  // 表单本地态：不直接绑 st，否则每次 refresh 都会把用户正在打的字冲掉
  const [name, setName] = useState("");
  const [owner, setOwner] = useState("");
  const [role, setRole] = useState("");
  const [notes, setNotes] = useState("");
  const [dirty, setDirty] = useState(false);

  const [secretName, setSecretName] = useState("");
  const [secretValue, setSecretValue] = useState("");

  const [preview, setPreview] = useState<string | null>(null);
  const [previewFull, setPreviewFull] = useState(false);

  const refresh = useCallback(
    (syncForm: boolean) => {
      setLoading(true);
      invoke<Status>("identity_status")
        .then((d) => {
          setSt(d);
          // 只在用户没未保存改动时同步表单 —— 别把他正在写的东西吃掉
          if (syncForm && !dirty) {
            setName(d.identity.name === "U-King" ? "" : d.identity.name);
            setOwner(d.identity.owner ?? "");
            setRole(d.identity.role ?? "");
            setNotes(d.identity.notes ?? "");
          }
        })
        .catch((e) => onToast?.(t("读取身份失败: {e}", { e: String(e) })))
        .finally(() => setLoading(false));
    },
    [onToast, t, dirty],
  );

  useEffect(() => { refresh(true); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async () => {
    setBusy("save");
    try {
      await invoke("save_identity", { patch: { name, owner, role, notes } });
      setDirty(false);
      onToast?.(t("已保存，说明书同步更新了"));
      refresh(false);
    } catch (e) {
      onToast?.(t("保存失败: {e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  const publish = async () => {
    setBusy("publish");
    try {
      await invoke("publish_identity");
      onToast?.(t("说明书已重新生成"));
      refresh(false);
    } catch (e) {
      onToast?.(t("生成失败: {e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  const setSecret = async (n: string, v: string) => {
    if (!n.trim()) return;
    setBusy(`secret:${n}`);
    try {
      await invoke("set_identity_secret", { name: n, value: v });
      setSecretName("");
      setSecretValue("");
      onToast?.(v ? t("凭据已保存（值只存本机，不进说明书）") : t("凭据已删除"));
      refresh(false);
    } catch (e) {
      onToast?.(t("操作失败: {e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  const setLink = async (linked: boolean, targets: string[]) => {
    setBusy("link");
    try {
      await invoke("link_identity", { linked, targets });
      onToast?.(linked ? t("挂上了 —— 那些 AI 下次开会话就知道有个 U-King 能用") : t("已撤销，你自己的内容原样没动"));
      refresh(false);
    } catch (e) {
      onToast?.(t("操作失败: {e}", { e: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  const openPreview = async (full: boolean) => {
    try {
      const body = await invoke<string>("read_llms_doc", { full });
      setPreviewFull(full);
      setPreview(body);
    } catch (e) {
      onToast?.(t("读不到说明书，先点「生成说明书」: {e}", { e: String(e) }));
    }
  };

  const input = "w-full h-9 px-3 rounded-lg bg-bg-1 border border-white/[0.08] text-[13px] text-ink-1 outline-none focus:border-accent/60";

  return (
    <div className="p-6 space-y-5 max-w-[880px]">
      <header>
        <h2 className="text-[16px] font-semibold text-ink-0 flex items-center gap-2">
          <IdCard size={17} className="text-accent" /> {t("让 AI 认识 U-King")}
        </h2>
        <p className="text-[12px] text-ink-4 mt-0.5">
          {t("给这台机器上的 U-King 起个名字、定个职责，并生成一份「给 AI 看的说明书」—— 让客户自己装的 Claude Code、Codex 或任何别家 AI，都能发现并调用它。")}
        </p>
      </header>

      {loading && !st ? (
        <div className="flex items-center gap-2 text-[13px] text-ink-4 py-8 justify-center">
          <Loader2 size={16} className="animate-spin" /> {t("读取中…")}
        </div>
      ) : (
        <>
          {/* ── 健康横幅：「生成了」≠「AI 真能发现」，两件事分开说 ── */}
          <div
            className={cn(
              "rounded-card border p-4 flex items-start gap-3",
              st?.ready ? "border-accent/40 bg-accent/[0.05]" : "border-amber-500/30 bg-amber-500/[0.08]",
            )}
          >
            {st?.ready ? <Check size={18} className="text-accent shrink-0 mt-0.5" /> : <AlertCircle size={18} className="text-amber-500 shrink-0 mt-0.5" />}
            <div className="flex-1 min-w-0">
              <div className="text-[13px] text-ink-1 font-medium">
                {st?.ready ? t("说明书已就位，别的 AI 能发现这台机器上的 U-King") : t("说明书还没生成 —— 别的 AI 现在发现不了我们")}
              </div>
              {(st?.blockers ?? []).map((b) => (
                <div key={b} className="text-[12px] text-amber-400/90 mt-1">• {b}</div>
              ))}
              {st?.ready && (
                <div className="text-[12px] text-ink-4 mt-1 font-mono break-all">{st.files.llms}</div>
              )}
              <div className="flex items-center gap-2 mt-2.5 flex-wrap">
                <button
                  data-action-id="runtime.identity.publish"
                  onClick={publish}
                  disabled={busy === "publish"}
                  className="h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600 disabled:opacity-50 inline-flex items-center gap-1.5"
                >
                  {busy === "publish" ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
                  {st?.ready ? t("重新生成说明书") : t("生成说明书")}
                </button>
                <button onClick={() => openPreview(false)} className="h-8 px-3 rounded-lg bg-bg-1 border border-white/[0.08] text-ink-3 text-[12px] hover:text-ink-1 inline-flex items-center gap-1.5">
                  <Eye size={13} /> {t("看看 AI 会读到什么")}
                </button>
                <button onClick={() => openPreview(true)} className="h-8 px-3 rounded-lg bg-bg-1 border border-white/[0.08] text-ink-3 text-[12px] hover:text-ink-1">
                  {t("全量版")}
                </button>
                <button onClick={() => invoke("open_install_dir").catch(() => {})} className="h-8 px-3 rounded-lg bg-bg-1 border border-white/[0.08] text-ink-3 text-[12px] hover:text-ink-1 inline-flex items-center gap-1.5">
                  <FolderOpen size={13} /> {t("打开目录")}
                </button>
              </div>
              <div className="text-[11px] text-ink-5 mt-2">
                {t("说明书是从动作表现场编译出来的，不是手写文档 —— 升级 U-King 后点一下「重新生成」就跟上，永远不会和实际能力对不上。")}
              </div>
            </div>
          </div>

          {/* ── 让 AI 发现我 ──
              这段是整页的重点：生成说明书只是第一半，没人指向它就等于锁在抽屉里。 */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2/70 p-4 space-y-3">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="text-[13.5px] font-semibold text-ink-0">{t("让 AI 发现我")}</div>
                <div className="text-[11.5px] text-ink-5 mt-0.5">
                  {t("在这些工具的全局记忆文件里加一行指针，指向 ~/.uking/llms.txt —— 否则它们不会自己想到去读。我们只加带标记的一小块，你原有的内容一个字都不动，随时可撤销（首次改动会自动留一份 .uking-bak）。")}
                </div>
              </div>
              <span className={cn("text-[10px] px-1.5 py-0.5 rounded shrink-0", (st?.linked_count ?? 0) > 0 ? "bg-success-500/15 text-success-400" : "bg-white/[0.05] text-ink-5")}>
                {t("已挂 {n}", { n: String(st?.linked_count ?? 0) })}
              </span>
            </div>

            <div className="space-y-1.5">
              {(st?.discovery ?? []).map((d) => (
                <div key={d.id} className="flex items-center justify-between gap-3 px-3 py-2 rounded-lg bg-bg-1 border border-white/[0.06]">
                  <div className="min-w-0">
                    <div className="text-[12.5px] text-ink-1 flex items-center gap-1.5">
                      {d.label}
                      {d.linked && <span className="inline-flex items-center gap-0.5 text-[10px] text-accent"><Check size={11} /> {t("已挂")}</span>}
                    </div>
                    <div className="text-[11px] text-ink-5 font-mono truncate">{d.eligible ? d.path : d.reason}</div>
                  </div>
                  <button
                    data-action-id="runtime.identity.link"
                    onClick={() => setLink(!d.linked, [d.id])}
                    disabled={!d.eligible || busy === "link"}
                    className={cn(
                      "h-7 px-3 rounded-lg text-[12px] shrink-0 disabled:opacity-30 disabled:cursor-not-allowed",
                      d.linked ? "bg-bg-2 border border-white/[0.08] text-ink-3 hover:text-ink-1" : "bg-accent text-white hover:bg-accent-600",
                    )}
                  >
                    {d.linked ? t("撤销") : t("挂上")}
                  </button>
                </div>
              ))}
            </div>

            <div className="flex items-center gap-2">
              <button
                data-action-id="runtime.identity.link"
                onClick={() => setLink(true, [])}
                disabled={busy === "link" || !(st?.discovery ?? []).some((d) => d.eligible && !d.linked)}
                className="h-8 px-3 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600 disabled:opacity-40 inline-flex items-center gap-1.5"
              >
                {busy === "link" && <Loader2 size={13} className="animate-spin" />}
                {t("全部挂上")}
              </button>
              <span className="text-[11px] text-ink-5">
                {t("指针只有 3 行 —— 它会进每个会话的上下文，所以刻意写得很短，详细内容都在 llms.txt 里按需读。")}
              </span>
            </div>
          </section>

          {/* ── 身份（明文）── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2/70 p-4 space-y-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-[13.5px] font-semibold text-ink-0">{t("身份")}</div>
                <div className="text-[11.5px] text-ink-5 mt-0.5">
                  {t("明文保存在 identity.json，会原样写进说明书 —— 这是你对所有 AI 说话的地方。")}
                </div>
              </div>
              <span className="text-[10px] px-1.5 py-0.5 rounded bg-white/[0.05] text-ink-4 shrink-0">{t("明文")}</span>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <label className="block">
                <span className="text-[12px] text-ink-3">{t("它叫什么")}</span>
                <input className={cn(input, "mt-1")} value={name} placeholder="U-King" onChange={(e) => { setName(e.target.value); setDirty(true); }} />
              </label>
              <label className="block">
                <span className="text-[12px] text-ink-3">{t("怎么称呼你")}</span>
                <input className={cn(input, "mt-1")} value={owner} placeholder={t("比如：李工")} onChange={(e) => { setOwner(e.target.value); setDirty(true); }} />
              </label>
            </div>

            <label className="block">
              <span className="text-[12px] text-ink-3">{t("职责（一句话）")}</span>
              <input className={cn(input, "mt-1")} value={role} placeholder={t("比如：负责海事业务文档和数据整理")} onChange={(e) => { setRole(e.target.value); setDirty(true); }} />
            </label>

            <label className="block">
              <span className="text-[12px] text-ink-3">{t("对所有 AI 的补充说明")}</span>
              <textarea
                className="w-full mt-1 px-3 py-2 rounded-lg bg-bg-1 border border-white/[0.08] text-[13px] text-ink-1 outline-none focus:border-accent/60 min-h-[84px] resize-y"
                value={notes}
                placeholder={t("比如：我的项目都在 D:\\work，别动 C 盘；文档一律用中文。")}
                onChange={(e) => { setNotes(e.target.value); setDirty(true); }}
              />
            </label>

            <div className="flex items-center gap-2">
              <button
                data-action-id="runtime.identity.save"
                onClick={save}
                disabled={busy === "save" || !dirty}
                className="h-8 px-4 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600 disabled:opacity-40 inline-flex items-center gap-1.5"
              >
                {busy === "save" && <Loader2 size={13} className="animate-spin" />}
                {t("保存")}
              </button>
              {dirty && <span className="text-[11.5px] text-amber-400/90">{t("有未保存的修改")}</span>}
            </div>
          </section>

          {/* ── 凭据（私密）── */}
          <section className="rounded-card border border-white/[0.06] bg-bg-2/70 p-4 space-y-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-[13.5px] font-semibold text-ink-0 flex items-center gap-1.5">
                  <KeyRound size={14} className="text-ink-3" /> {t("凭据")}
                </div>
                <div className="text-[11.5px] text-ink-5 mt-0.5">
                  {t("值只存在本机的 secrets.json。说明书里只写「配了哪些 Key」，永远不写值 —— 你可以点上面的「看看 AI 会读到什么」自己搜一遍验证。")}
                </div>
              </div>
              <span className="text-[10px] px-1.5 py-0.5 rounded bg-danger-500/15 text-danger-400 shrink-0">{t("私密")}</span>
            </div>

            {(st?.secrets ?? []).length > 0 && (
              <div className="space-y-1.5">
                {st!.secrets.map((s) => (
                  <div key={s.name} className="flex items-center justify-between gap-2 px-3 h-9 rounded-lg bg-bg-1 border border-white/[0.06]">
                    <span className="text-[12.5px] text-ink-1 font-mono truncate">{s.name}</span>
                    <div className="flex items-center gap-2 shrink-0">
                      <span className={cn("text-[10px] px-1.5 py-0.5 rounded", s.configured ? "bg-success-500/15 text-success-400" : "bg-white/[0.05] text-ink-5")}>
                        {s.configured ? t("已配") : t("空")}
                      </span>
                      <button
                        data-action-id="runtime.identity.secret_set"
                        onClick={() => setSecret(s.name, "")}
                        disabled={busy === `secret:${s.name}`}
                        title={t("删除")}
                        className="text-ink-5 hover:text-danger-400 disabled:opacity-40"
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}

            <div className="flex items-end gap-2 flex-wrap">
              <label className="block flex-1 min-w-[140px]">
                <span className="text-[12px] text-ink-3">{t("名称")}</span>
                <input className={cn(input, "mt-1")} value={secretName} placeholder="openai" onChange={(e) => setSecretName(e.target.value)} />
              </label>
              <label className="block flex-[2] min-w-[180px]">
                <span className="text-[12px] text-ink-3">{t("值")}</span>
                <input type="password" className={cn(input, "mt-1")} value={secretValue} placeholder="sk-…" onChange={(e) => setSecretValue(e.target.value)} />
              </label>
              <button
                data-action-id="runtime.identity.secret_set"
                onClick={() => setSecret(secretName, secretValue)}
                disabled={!secretName.trim() || !secretValue.trim() || busy?.startsWith("secret:")}
                className="h-9 px-4 rounded-lg bg-bg-1 border border-white/[0.08] text-ink-1 text-[12px] hover:border-accent/50 disabled:opacity-40"
              >
                {t("添加")}
              </button>
            </div>
          </section>

          {/* ── 说明书预览 ── */}
          {preview !== null && (
            <section className="rounded-card border border-white/[0.06] bg-bg-2/70 p-4 space-y-2">
              <div className="flex items-center justify-between">
                <div className="text-[13px] font-semibold text-ink-0">
                  {previewFull ? t("llms-full.txt（全量版）") : t("llms.txt（AI 会读到的内容）")}
                </div>
                <button onClick={() => setPreview(null)} className="text-[12px] text-ink-4 hover:text-ink-1">{t("收起")}</button>
              </div>
              <pre className="text-[11.5px] leading-[1.55] text-ink-2 bg-bg-1 rounded-lg p-3 max-h-[420px] overflow-auto whitespace-pre-wrap break-words font-mono">
                {preview}
              </pre>
            </section>
          )}
        </>
      )}
    </div>
  );
}
