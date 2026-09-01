/**
 * 更新日志弹层 —— 点侧栏版本号 / 升级横幅弹出。
 *
 * 「翻翻历史」的完整入口在**官网 `website/changelog.html`**（纯 HTML+JS，React 组件共享不过去）。
 * 两边不共代码但共数据：都渲染服务器同一份 version.json 的 notes + history
 * （宪法第 8 条 —— 版本历史这份事实只存 version.json 一处，谁都不许抄一份进自己的文件）。
 *
 * 「更新了啥」以前一直是黑洞：服务器 version.json 里写了 notes，`check_update` 也拉回来了，
 * 但前端从来没渲染过，直接丢掉。这个弹层就是把那份数据接出来，再补上历史版本。
 *
 * 数据来自服务器（`check_update` → version.json 的 notes + history），**不编进 exe**：
 * 改一句话不用发一次版（同 Feed 的思路）。拉不到就如实说「需要联网」，不假装有历史。
 *
 * 独立可插拔：只靠 props 通信，不引任何后端。删它只动 2 个文件
 * （App.tsx 去 import + 一个 state，Sidebar.tsx 去 onShowChangelog 一个 prop）。
 */
import { X, Sparkles, History } from "lucide-react";
import { useI18n } from "./i18n";

export type VersionNote = { version: string; date?: string; notes?: string };

/** a 是否比 b 新（只取数字段，与后端 `installer.rs::semver_gt` 同一口径，别让两边判反）。 */
function newerThan(a: string, b: string): boolean {
  const parse = (s: string) => s.split(".").map((p) => parseInt(p, 10) || 0);
  const [va, vb] = [parse(a), parse(b)];
  for (let i = 0; i < Math.max(va.length, vb.length); i++) {
    const [x, y] = [va[i] ?? 0, vb[i] ?? 0];
    if (x !== y) return x > y;
  }
  return false;
}

export default function Changelog({
  current,
  latest,
  notes,
  history,
  onClose,
}: {
  current: string;
  latest?: string;
  notes?: string;
  history?: VersionNote[];
  onClose: () => void;
}) {
  const { t } = useI18n();
  const list = history ?? [];
  // 服务器已经在 history 里列了当前最新版时就别重复渲染顶部那块。
  const latestInHistory = !!latest && list.some((h) => h.version === latest);
  const showLatest = !!latest && !!notes && !latestInHistory;
  const isNewer = !!latest && latest !== current;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60" onClick={onClose}>
      <div
        className="w-full max-w-2xl max-h-[80vh] flex flex-col rounded-card bg-bg-1 border border-white/[0.08] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-white/[0.06]">
          <div className="flex items-center gap-2">
            <History size={16} className="text-accent" />
            <span className="text-[15px] font-semibold text-ink-1">{t("更新日志")}</span>
            <span className="text-[11px] font-mono text-ink-5">
              {t("本机")} v{current}
              {isNewer ? ` · ${t("最新")} v${latest}` : ""}
            </span>
          </div>
          <button
            onClick={onClose}
            className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
            title={t("关闭")}
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 overflow-auto px-5 py-4 space-y-4">
          {showLatest && (
            <section className="rounded-card border border-accent/30 bg-accent/[0.06] p-4">
              <div className="flex items-center gap-1.5 mb-2">
                <Sparkles size={13} className="text-accent" />
                <span className="text-[13px] font-semibold text-ink-1">
                  v{latest}
                  {isNewer ? ` · ${t("可升级")}` : ` · ${t("当前版本")}`}
                </span>
              </div>
              <p className="text-[12.5px] leading-relaxed text-ink-2 whitespace-pre-wrap">{notes}</p>
            </section>
          )}

          {list.map((h) => {
            // 比本机新 = 升上去才能拿到的。不标出来的话，一列版本号里根本看不出
            // 「哪些是我还没有的」—— 而这正是打开这个面板要回答的问题。
            const pending = isNewer && newerThan(h.version, current);
            return (
              <section
                key={h.version}
                className={
                  "rounded-card p-4 border " +
                  (pending ? "border-accent/30 bg-accent/[0.06]" : "border-white/[0.06]")
                }
              >
                <div className="flex items-baseline gap-2 mb-1.5 flex-wrap">
                  <span className="text-[13px] font-semibold text-ink-1">v{h.version}</span>
                  {h.date ? <span className="text-[11px] text-ink-5 font-mono">{h.date}</span> : null}
                  {h.version === current ? (
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-white/[0.08] text-ink-3">{t("本机")}</span>
                  ) : pending ? (
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-accent/15 text-accent font-medium">
                      {t("升级后获得")}
                    </span>
                  ) : null}
                </div>
                <p className={"text-[12.5px] leading-relaxed whitespace-pre-wrap " + (pending ? "text-ink-2" : "text-ink-3")}>
                  {h.notes || t("（无说明）")}
                </p>
              </section>
            );
          })}

          {!showLatest && list.length === 0 && (
            // 不编内置历史进 exe，所以离线时就是没有 —— 如实说，别摆个空壳假装。
            <div className="py-10 text-center text-[12.5px] text-ink-4">
              {t("暂时拿不到更新日志（需要联网）。")}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
