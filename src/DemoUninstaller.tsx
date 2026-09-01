import { ShieldAlert, Sparkles, Trash2 } from "lucide-react";
import { SafeUninstall } from "./Advanced";

/**
 * 独立绿色版的唯一页面。
 *
 * 它复用 Advanced 里的同一套扫描/官方卸载协议，不复制任何删除逻辑；这样主程序新增了
 * 可清理项目后，这个录演示用的小工具也会自动拥有相同的覆盖范围。
 */
export function DemoUninstaller({ onToast }: { onToast: (message: string) => void }) {
  return (
    <div className="h-full overflow-y-auto bg-bg-0">
      <main className="mx-auto w-full max-w-4xl px-6 py-8 sm:px-9">
        <header className="mb-6 rounded-card border border-danger-500/25 bg-gradient-to-br from-danger-500/[0.12] to-transparent px-6 py-5 shadow-card">
          <div className="flex items-start gap-3">
            <span className="grid h-11 w-11 shrink-0 place-items-center rounded-xl bg-danger-500/15 text-danger-400">
              <Trash2 size={21} />
            </span>
            <div>
              <div className="flex flex-wrap items-center gap-2">
                <h1 className="text-[19px] font-semibold text-ink-0">U-King 演示卸载工具</h1>
                <span className="rounded-full border border-danger-500/25 bg-danger-500/10 px-2 py-0.5 text-[10.5px] font-medium text-danger-400">
                  绿色版 · 录制专用
                </span>
              </div>
              <p className="mt-1.5 max-w-2xl text-[13px] leading-relaxed text-ink-3">
                用于录制“安装 → 卸载 → 再安装”。它会扫描本机由 U-King 管理或展示过的工具，默认全部选中；你可以取消任何不想动的项目。
              </p>
            </div>
          </div>
        </header>

        <div className="mb-5 flex items-start gap-2.5 rounded-card border border-warning-400/25 bg-warning-400/[0.07] px-4 py-3 text-[12px] leading-relaxed text-ink-2">
          <ShieldAlert size={16} className="mt-0.5 shrink-0 text-warning-400" />
          <p>
            工具本体会调用各软件自己的卸载程序；Claude / Codex / ClawX / Hermes 的配置只撤销 U-King 写入的部分。若这台不是演示机，请先取消不属于本次演示的项目。
          </p>
        </div>

        <SafeUninstall demo onToast={onToast} />

        <p className="mt-5 flex items-center justify-center gap-1.5 text-[11px] text-ink-4">
          <Sparkles size={12} /> 完成后重新运行 U-King，即可从干净状态开始下一次安装演示。
        </p>
      </main>
    </div>
  );
}
