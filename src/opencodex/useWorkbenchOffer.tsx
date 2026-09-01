/**
 * 「这个空文件夹要不要布置成工作台？」—— 接在「新建项目·选文件夹」之后的一问。
 *
 * ## 为什么接在这儿，而不是另开一个页
 * 客户选完一个空文件夹，现在得到的是**一个空文件夹** —— 他还得自己想「素材放哪、稿子放哪」，
 * AI 进来也不知道任何约定。这一问就发生在他最需要答案的那一秒。
 * 另开一个「工作台模板」页的话，需要它的人恰好不知道它存在。
 *
 * ## 三条自律
 * 1. **只在空文件夹上问**。已有内容的目录是他的项目，我们一个字都不该说
 *    （后端的闸门也会拒，这里只是不打扰）。
 * 2. **先给看再给装**：列出真会建的目录，那份清单是后端 `inspect` 返回的**安装计划本身**，
 *    不是前端另写的一段文案 —— 文案会跟真实行为漂开，而漂开的那次正好是出事那次。
 * 3. **「先空着」永远在**，且不是次要按钮。不想要模板是完全正常的选择。
 *
 * ## 🔴 颜色只许用 `text-ink-*`，不许用 `text-white/*`
 * 这个文件曾经用 12 处 `text-white/xx` 写死深色 —— 而 App **默认是浅色主题**
 * （`App.tsx` 的 `uking.theme` 缺省落 light），浅色下弹窗底色 `bg-bg-2` 是**纯白**，
 * 于是路径、一句话简介、「会建这些目录」、「它没有什么」、连整个「先空着」按钮的文字
 * 全都白底白字看不见 —— 客户截图里那是一个**空白框**。
 * `globals.css` 的浅色兼容层映射了 `border-white/*` `bg-white/[0.0x]`，唯独没有 `text-white/*`。
 * 现在 `scripts/check-theme-tokens.mjs` 会在构建期拦住它，别再写回去。
 *
 * 一份实现多处用（同 `Composer` / `ExpertGallery`）：谁有「新建项目」入口谁就调 `offer(dir)`
 * 并渲染 `node`，别复制第二份弹窗。
 */
import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FolderPlus, Check, X, Loader2, AlertTriangle } from "lucide-react";
import { useI18n } from "../i18n";

type AnyObj = Record<string, any>;

async function runAction(actionId: string, input: AnyObj = {}, confirmed = false): Promise<AnyObj> {
  const res: any = await invoke("action_parity_call", {
    request: { action_id: actionId, input, confirmed },
  });
  if (!res?.ok) throw new Error(res?.error?.message || `${actionId} 失败`);
  return res.result || {};
}

interface Step {
  kind: string;
  path: string;
  verdict: string;
}
interface Template {
  id: string;
  name: string;
  one_liner: string;
  for_whom: string;
  dirs: number;
  skills: string[];
  not_included: string[];
}
/**
 * 装完的结果。**存后端返回的字段，不存前端拼好的一句话** ——
 * 上一版把 `next` 扔了自己另写了一句「AI 进这个文件夹会先读 WORKBENCH.md」，
 * 而 `workbench.rs` 的 ENTRYPOINTS 注释白纸黑字写着：没有任何 AI CLI 会自动读这个文件名，
 * 真入口是 `AGENTS.md` / `CLAUDE.md`。同一份事实抄第二遍，抄错的那遍就是客户看到的那遍。
 */
interface Outcome {
  ok: boolean;
  created: number;
  /** 后端算出来的「接下来干什么」原话 */
  next: string;
  /** 后端装完回头量真实世界发现的问题（入口文件是客户自己的 = AI 读不到约定） */
  warnings: string[];
  /** 失败时后端拒绝的理由原文 */
  error: string;
}

/**
 * 去掉 markdown 记号。模板 JSON 里的 `**加粗**`、后端 warnings 里的 `` `CLAUDE.md` ``
 * 都是写给 WORKBENCH.md 那份 markdown 看的；弹窗按纯文本渲染，
 * 不去掉客户就看到一对星号和一对反引号（这两处都是真出现过的，不是假想）。
 */
function plain(s: string): string {
  return String(s ?? "")
    .replace(/\*\*/g, "")
    .replace(/`/g, "");
}

export function useWorkbenchOffer() {
  const { t: tr } = useI18n();
  const [dir, setDir] = useState<string | null>(null);
  const [tpl, setTpl] = useState<Template | null>(null);
  const [plan, setPlan] = useState<Step[]>([]);
  const [busy, setBusy] = useState(false);
  /** 装完的结果。工作台里没有全局 toast —— 与其为一句话新造一套基建，不如让弹窗自己说完。 */
  const [result, setResult] = useState<Outcome | null>(null);
  /** offer() 的 resolve —— 调用方 `await offer(dir)` 之后才继续建项目，避免两个弹窗打架。 */
  const doneRef = useRef<(() => void) | null>(null);

  const close = useCallback(() => {
    setDir(null);
    setTpl(null);
    setPlan([]);
    setResult(null);
    doneRef.current?.();
    doneRef.current = null;
  }, []);

  /** 正在往硬盘上写的时候，遮罩和 X 都不许关 —— 关掉会 resolve 掉 offer() 并把结果丢了。 */
  const closeIfIdle = useCallback(() => {
    if (!busy) close();
  }, [busy, close]);

  const offer = useCallback(async (folder: string) => {
    try {
      const r = await runAction("runtime.workbench.inspect", { path: folder });
      const target = r.target || {};
      // 只在「空的、能装、还不是工作台」时开口。其余情况一个字都不说。
      if (!target.installable || !target.empty || target.is_workbench) return;
      const templates: Template[] = r.templates || [];
      const chosen = templates.find((x) => x.id === r.default_template) || templates[0];
      if (!chosen) return;
      setTpl(chosen);
      setPlan((target.plan || []).filter((s: Step) => s.verdict === "create"));
      setDir(folder);
      await new Promise<void>((resolve) => {
        doneRef.current = resolve;
      });
    } catch {
      // 探不出来就当没这回事 —— 这一问是锦上添花，不该挡住「新建项目」本身。
    }
  }, []);

  const install = useCallback(async () => {
    if (!dir || !tpl) return;
    setBusy(true);
    try {
      const r = await runAction("runtime.workbench.install", { path: dir, template: tpl.id }, true);
      setResult({
        ok: true,
        created: Number(r.created ?? 0),
        // 「接下来干什么」用后端的原话：它知道入口文件到底接上没有，前端不知道。
        next: typeof r.next === "string" ? r.next : "",
        // 🔴 warnings 必须露出来。后端装完会回头量真实世界，量出「入口文件是客户自己的、
        //    里面没提 WORKBENCH.md」时，这次「装好了」是假的：AI 进来读不到任何约定。
        //    只报一句「好了」就是 workbench.rs 那段注释警告的「报告对、世界坏」。
        warnings: Array.isArray(r.warnings) ? r.warnings.filter((w: any) => typeof w === "string") : [],
        error: "",
      });
    } catch (e: any) {
      // 🔴 失败要说清是什么失败了。后端拒绝的理由本身就是给人看的话（比如「这个目录不是空的」），
      //    在这儿替换成「操作失败」等于把唯一有用的信息扔掉。
      setResult({ ok: false, created: 0, next: "", warnings: [], error: e?.message || String(e) });
    } finally {
      setBusy(false);
    }
  }, [dir, tpl]);

  const dirsInPlan = plan.filter((s) => s.kind === "dir" && s.path !== ".");
  // 星号去掉再渲染；超出的条数如实说，别静默截断（截断了他两天后才发现，比一开始说清楚糟）
  const notIncluded = (tpl?.not_included ?? []).map(plain);
  const shownNotIncluded = notIncluded.slice(0, 3);
  const restNotIncluded = notIncluded.length - shownNotIncluded.length;
  const warned = (result?.warnings?.length ?? 0) > 0;

  const node =
    dir && tpl ? (
      <div className="fixed inset-0 z-[60] grid place-items-center bg-black/50 p-4" onClick={closeIfIdle}>
        <div
          className="w-full max-w-lg max-h-[82vh] flex flex-col rounded-card border border-white/[0.10] bg-bg-2 shadow-card"
          onClick={(e) => e.stopPropagation()}
        >
          <div className="flex items-start gap-3 px-4 py-3 border-b border-white/[0.06]">
            <FolderPlus size={18} className="mt-0.5 shrink-0 text-accent" />
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium text-ink-0">{tr("这个文件夹是空的，要布置一下吗？")}</div>
              <div className="mt-0.5 text-xs text-ink-3 truncate" title={dir}>
                {dir}
              </div>
            </div>
            <button
              className="p-1 rounded text-ink-3 hover:text-ink-1 hover:bg-white/[0.06] disabled:opacity-40"
              disabled={busy}
              onClick={closeIfIdle}
              title={tr("关闭")}
            >
              <X size={16} />
            </button>
          </div>

          {result ? (
            <>
              <div className="min-h-0 flex-1 overflow-auto px-4 py-4">
                <div
                  className={
                    // warning-600 而不是 500/amber-300：500 压在浅色白底上只有 2.9:1，
                    // 300 更是几乎看不见。600 两套主题都过 3:1（浅 3.5 / 深 5.2）。
                    result.ok && !warned ? "text-sm text-ink-1" : "text-sm text-warning-600"
                  }
                >
                  {!result.ok
                    ? tr("没能布置：")
                    : warned
                      ? tr("装好了，但有一件事要你处理。")
                      : tr("好了。")}
                </div>

                {result.ok ? (
                  <>
                    <div className="mt-1.5 text-xs text-ink-2 leading-relaxed">
                      {tr("建好了 {n} 项。", { n: result.created })}
                    </div>
                    {/* 「接下来干什么」是后端的原话 —— 它知道入口文件接没接上，前端不知道 */}
                    {result.next && (
                      <div className="mt-1.5 text-xs text-ink-3 leading-relaxed whitespace-pre-wrap">
                        {plain(result.next)}
                      </div>
                    )}
                    {warned && (
                      <div className="mt-3 rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2">
                        <div className="flex items-start gap-2">
                          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-warning-500" />
                          <div className="min-w-0 space-y-1">
                            {result.warnings.map((w, i) => (
                              <div key={i} className="text-xs text-ink-2 leading-relaxed whitespace-pre-wrap">
                                {plain(w)}
                              </div>
                            ))}
                          </div>
                        </div>
                      </div>
                    )}
                  </>
                ) : (
                  <div className="mt-1.5 text-xs text-ink-2 leading-relaxed whitespace-pre-wrap">{result.error}</div>
                )}
              </div>
              <div className="flex items-center gap-2 px-4 py-3 border-t border-white/[0.06]">
                <button
                  onClick={close}
                  className="h-8 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600"
                >
                  {tr("开始干活")}
                </button>
              </div>
            </>
          ) : (
          <>
          <div className="min-h-0 flex-1 overflow-auto px-4 py-3 space-y-3">
            <div>
              <div className="flex items-center gap-2">
                <div className="text-sm text-ink-0">{tpl.name}</div>
                {/* 内置模板目前只有一个，谁进来都是它 —— 那就别把它说成「为你挑的」 */}
                <span className="px-1.5 py-0.5 rounded bg-bg-3 text-[10px] text-ink-3 shrink-0">
                  {tr("内置示例")}
                </span>
              </div>
              <div className="mt-1 text-xs text-ink-2 leading-relaxed">{plain(tpl.one_liner)}</div>
              {/* 说清它是给谁的，他才判断得出合不合身；不合身就点「先空着」 */}
              {tpl.for_whom && (
                <div className="mt-1 text-xs text-ink-3 leading-relaxed">
                  {tr("给谁用：")}
                  {plain(tpl.for_whom)}
                </div>
              )}
            </div>

            {/* 这份清单是后端返回的安装计划本身 —— 上面写什么，点下去就真建什么 */}
            <div>
              <div className="text-xs text-ink-3 mb-1.5">{tr("会建这些目录")}</div>
              <div className="flex flex-wrap gap-1.5">
                {dirsInPlan.map((s) => (
                  <span key={s.path} className="px-2 py-0.5 rounded bg-bg-3 text-xs text-ink-2">
                    {s.path}
                  </span>
                ))}
              </div>
              <div className="mt-1.5 text-xs text-ink-3 leading-relaxed">
                {tr(
                  "外加每个目录一份说明，和 AGENTS.md / CLAUDE.md 两个入口文件 —— AI 进这个文件夹会自动读它们，就知道每个目录是干嘛的。",
                )}
              </div>
            </div>

            {notIncluded.length > 0 && (
              <div>
                {/* 一块「什么都看得见」的板会让人以为这就是全部 —— 所以「没有什么」也摆出来 */}
                <div className="text-xs text-ink-3 mb-1">{tr("它没有什么")}</div>
                <ul className="space-y-0.5">
                  {shownNotIncluded.map((n, i) => (
                    <li key={i} className="text-xs text-ink-3 leading-relaxed">
                      · {n}
                    </li>
                  ))}
                </ul>
                {restNotIncluded > 0 && (
                  <div className="mt-1 text-xs text-ink-3">
                    {tr("还有 {n} 条，装完写在 WORKBENCH.md 里。", { n: restNotIncluded })}
                  </div>
                )}
              </div>
            )}
          </div>

          <div className="flex items-center gap-2 px-4 py-3 border-t border-white/[0.06]">
            <button
              data-action-id="runtime.workbench.install"
              disabled={busy}
              onClick={install}
              className="inline-flex items-center gap-1.5 h-8 px-4 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40"
            >
              {busy ? <Loader2 size={14} className="animate-spin" /> : <Check size={14} />}
              {tr("布置成工作台")}
            </button>
            {/* 不想要模板是完全正常的选择，所以它不是一个灰按钮 */}
            <button
              disabled={busy}
              onClick={close}
              className="h-8 px-3.5 rounded-lg border border-white/[0.10] bg-bg-1 text-[12px] text-ink-2 hover:border-white/20 disabled:opacity-40"
            >
              {tr("先空着")}
            </button>
            <div className="ml-auto text-xs text-ink-3">
              {tr("之后在这个文件夹上再新建一次项目，还会问你。")}
            </div>
          </div>
          </>
          )}
        </div>
      </div>
    ) : null;

  return { offer, node };
}
