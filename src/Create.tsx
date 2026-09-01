/**
 * AI 创作 —— 作图 / 视频 / 海报二维码 三合一入口。
 *
 * 信息架构梳理（2026-07-19）：这三个是老客户最高频的创作能力（也是烧 token 的主力），
 * 原先分散埋在侧栏「更多」折叠组里没人点。现合并成一个核心项「AI 创作」。
 *
 * 本页只是壳：内部标签切换 + 懒挂载保活（访问过的 tab 用 display 切换不卸载，
 * 作图历史/视频轮询等页内状态不丢）。三个原页面（Draw/Video/QrMerge）保持独立模块、
 * 原路由不动（AI 专家页等深链仍可直达）。删除本页只需动 App.tsx + Sidebar（铁律④）。
 */
import { lazy, Suspense, useState } from "react";
import { Clapperboard, History, Image as ImageIcon, QrCode } from "lucide-react";
import { cn } from "./lib/cn";
import { useI18n } from "./i18n";
import type { DeviceKey } from "./lib/types";

const Draw = lazy(() => import("./Draw").then((m) => ({ default: m.Draw })));
const Video = lazy(() => import("./Video").then((m) => ({ default: m.Video })));
const Reel = lazy(() => import("./Reel").then((m) => ({ default: m.Reel })));
const MediaTasks = lazy(() => import("./Reel").then((m) => ({ default: m.MediaTasks })));
const QrMerge = lazy(() => import("./QrMerge").then((m) => ({ default: m.QrMerge })));

/** 与 App.tsx 的 DeviceKey 同构（透传，不加工）。 */
type SubTab = "draw" | "video" | "reel" | "qrmerge" | "tasks";

const SUBS: { id: SubTab; label: string; icon: typeof ImageIcon }[] = [
  { id: "draw", label: "AI 作图", icon: ImageIcon },
  { id: "video", label: "AI 视频", icon: Clapperboard },
  { id: "reel", label: "一键成片", icon: Clapperboard },
  { id: "tasks", label: "任务中心", icon: History },
  { id: "qrmerge", label: "AI 海报二维码", icon: QrCode },
];

function Fallback() {
  return (
    <div className="flex-1 grid place-items-center py-16 text-ink-4">
      <div className="h-6 w-6 animate-spin rounded-full border-2 border-white/10 border-t-accent" />
    </div>
  );
}

export function Create({
  deviceKey,
  onToast,
  onRecharge,
  onGoSkillPack,
  initialSub = "draw",
}: {
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onRecharge: () => void;
  onGoSkillPack: () => void;
  /** 一进来落在哪个子页。给 U-Workspace 的创作面板用：召唤「AI 视频专家」该直接看到视频页，
   *  不该让人先看见作图再自己点一下 —— 那一步是我们的信息架构漏给用户的。 */
  initialSub?: SubTab;
}) {
  const { t } = useI18n();
  const [sub, setSub] = useState<SubTab>(initialSub);
  // 访问过的子页常驻渲染（display 切换保活）—— 与 App.tsx 的 TUI 保活同一手法
  const [mounted, setMounted] = useState<Set<SubTab>>(new Set([initialSub]));
  const go = (id: SubTab) => {
    setSub(id);
    setMounted((s) => (s.has(id) ? s : new Set(s).add(id)));
  };

  return (
    // 高度链的一环（测试报告 #005）：外层 main 已改成不滚的 flex 容器，
    // 这里必须把高度往下传，否则子页的 h-full 拿不到确定高度。
    <div className="flex flex-col flex-1 min-h-0 gap-4">
      {/* 创作功能栏只在这个唯一的创作入口出现；不在全局侧栏再放第二组 Reel/任务入口。 */}
      <div className="flex min-h-0 flex-1 flex-col gap-4 md:flex-row">
      <nav aria-label={t("AI 创作")} className="flex shrink-0 gap-1.5 overflow-x-auto md:w-36 md:flex-col md:overflow-visible">
        {SUBS.map((s) => {
          const Icon = s.icon;
          const on = sub === s.id;
          return (
            <button
              key={s.id}
              onClick={() => go(s.id)}
              className={cn(
                "inline-flex shrink-0 items-center gap-1.5 px-3.5 h-9 rounded-lg border text-[13px] font-medium transition-colors md:w-full",
                on
                  ? "border-accent/40 bg-accent/[0.10] text-accent"
                  : "border-white/[0.08] text-ink-2 hover:bg-white/[0.04]",
              )}
            >
              <Icon size={14} />
              {t(s.label)}
            </button>
          );
        })}
      </nav>

      {/* 子页内容：访问过即挂载，display 切换保活 */}
      <div className="flex min-h-0 flex-1 flex-col">
      {mounted.has("draw") && (
        <div className="flex-1 min-h-0" style={{ display: sub === "draw" ? undefined : "none" }}>
          <Suspense fallback={<Fallback />}>
            <Draw deviceKey={deviceKey} onToast={onToast} onRecharge={onRecharge} onGoSkillPack={onGoSkillPack} />
          </Suspense>
        </div>
      )}
      {mounted.has("video") && (
        <div className="flex-1 min-h-0" style={{ display: sub === "video" ? undefined : "none" }}>
          <Suspense fallback={<Fallback />}>
            <Video deviceKey={deviceKey} onToast={onToast} onRecharge={onRecharge} onGoSkillPack={onGoSkillPack} />
          </Suspense>
        </div>
      )}
      {mounted.has("reel") && (
        <div className="flex-1 min-h-0" style={{ display: sub === "reel" ? undefined : "none" }}>
          <Suspense fallback={<Fallback />}>
            <Reel deviceKey={deviceKey} onToast={onToast} onRecharge={onRecharge} />
          </Suspense>
        </div>
      )}
      {mounted.has("qrmerge") && (
        <div className="flex-1 min-h-0" style={{ display: sub === "qrmerge" ? undefined : "none" }}>
          <Suspense fallback={<Fallback />}>
            <QrMerge deviceKey={deviceKey} onToast={onToast} onRecharge={onRecharge} />
          </Suspense>
        </div>
      )}
      {mounted.has("tasks") && (
        <div className="flex-1 min-h-0 overflow-y-auto" style={{ display: sub === "tasks" ? undefined : "none" }}>
          <Suspense fallback={<Fallback />}><MediaTasks onGo={(next) => go(next)} /></Suspense>
        </div>
      )}
      </div>
      </div>
    </div>
  );
}
