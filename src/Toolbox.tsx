/**
 * 厨具工具箱 —— 给本机 AI 装「能力工具」（ffmpeg / Chrome / PowerShell 7 / Python …）。
 * 按用途分类、360 式可选一键装。让 AI「厨师」厨具备齐，去通做各种软件活。
 *
 * **整块可插拔**：纯前端，只靠 props（onToast）通信。后端 toolbox.rs + 两个 command。
 * 删本页只删这一个文件 + App.tsx 去 import/tab + lib.rs 去 command。
 */
import { useEffect, useState } from "react";
import { useI18n } from "./i18n";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Download, Wrench, Loader2, RefreshCw } from "lucide-react";
import { cn } from "./lib/cn";

type ToolStatus = {
  id: string;
  name: string;
  desc: string;
  category: string;
  uses: string;
  installed: boolean;
  can_install: boolean;
  manual_hint: string;
};

// 分类 → emoji（前端展示用；分类字符串由后端 toolbox.rs 决定）。
const CAT_EMOJI: Record<string, string> = {
  视频音频: "🎬",
  网页浏览: "🌐",
  终端环境: "🖥️",
  文档办公: "📄",
  开发工具: "🔧",
};

// 按用途的「推荐组合」—— 选一个用途，高亮该装的厨具 + 一键装齐缺的。
// toolIds 必须对上 toolbox.rs CATALOG 里的 id（改后端 id 时同步）。
const BUNDLES: { id: string; emoji: string; name: string; desc: string; toolIds: string[] }[] = [
  // 描述只说**客户得到什么**，不说我们内部怎么搭的（测试报告 #024：「实际效果描述过于冗杂」）。
  // 原文是「作图/视频/配音已在 AIGC 技能包，再补拼接 + 浏览器找素材」—— 那是讲给我们自己听的：
  // 客户不知道「AIGC 技能包」是什么，也不关心哪部分已经有了。
  { id: "manju", emoji: "🎨", name: "做漫剧 / 短视频", desc: "让 AI 把多段视频拼成一条成片，并能自己上网找素材", toolIds: ["ffmpeg", "chrome"] },
  { id: "video", emoji: "🎬", name: "视频剪辑 / 成片", desc: "拼接、转码、加音轨", toolIds: ["ffmpeg"] },
  { id: "ppt", emoji: "📊", name: "做 PPT / 教案", desc: "AI 生成/转换 PPT、Word、PDF", toolIds: ["python", "libreoffice", "pandoc"] },
  { id: "web", emoji: "🌐", name: "网页自动化", desc: "让 AI 控浏览器抓取/截图/填表", toolIds: ["chrome"] },
  { id: "dev", emoji: "🖥️", name: "终端 / 开发", desc: "更强终端 + 版本控制", toolIds: ["pwsh", "windows-terminal", "git"] },
];

export function Toolbox({ onToast }: { onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [tools, setTools] = useState<ToolStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState<string | null>(null);
  const [progress, setProgress] = useState("");
  const [bundle, setBundle] = useState<string | null>(null);

  const load = async () => {
    try {
      const list = await invoke<ToolStatus[]>("list_capability_tools");
      setTools(list);
    } catch (e) {
      onToast(t("读取工具箱失败：") + String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    const un = listen<string>("uking:toolbox_progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const install = async (tool: ToolStatus) => {
    if (installing) return;
    setInstalling(tool.id);
    setProgress(t("准备安装 {name}…", { name: tool.name }));
    try {
      const msg = await invoke<string>("install_capability_tool", { id: tool.id });
      onToast(msg);
      await load();
    } catch (e) {
      onToast(t("安装失败：") + String(e));
    } finally {
      setInstalling(null);
      setProgress("");
    }
  };

  // 一键装齐某个用途组里「还没装」的厨具（逐个装，winget 一次一个）。
  const installBundle = async (b: (typeof BUNDLES)[number]) => {
    if (installing) return;
    const missing = tools.filter((tool) => b.toolIds.includes(tool.id) && !tool.installed && tool.can_install);
    if (!missing.length) { onToast(t("「{name}」需要的厨具都装好了 ✓", { name: t(b.name) })); return; }
    for (const tool of missing) {
      setInstalling(tool.id);
      setProgress(t("装齐「{bundle}」：{name}…", { bundle: t(b.name), name: tool.name }));
      try {
        const msg = await invoke<string>("install_capability_tool", { id: tool.id });
        onToast(msg);
      } catch (e) {
        onToast(t("安装失败：") + String(e));
        break;
      }
    }
    setInstalling(null);
    setProgress("");
    await load();
  };

  const activeBundle = BUNDLES.find((b) => b.id === bundle) ?? null;
  const bundleIds = activeBundle?.toolIds ?? [];
  const bundleMissing = activeBundle
    ? tools.filter((tool) => bundleIds.includes(tool.id) && !tool.installed).map((tool) => tool.name)
    : [];

  // 按 category 分组，保留后端顺序。
  const cats: { name: string; items: ToolStatus[] }[] = [];
  for (const tool of tools) {
    let g = cats.find((c) => c.name === tool.category);
    if (!g) { g = { name: tool.category, items: [] }; cats.push(g); }
    g.items.push(tool);
  }
  const installedCount = tools.filter((tool) => tool.installed).length;

  return (
    <div className="space-y-5">
      {/* 顶栏 */}
      <section className="flex flex-wrap items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-3">
        <Wrench size={18} className="text-accent shrink-0" />
        <div className="min-w-0">
          <div className="text-[15px] font-semibold text-ink-0">{t("厨具工具箱")}</div>
          <div className="text-[12px] text-ink-3">{t("给本机 AI 备齐「能力工具」—— 让它能剪视频、控浏览器、做 PPT、跑脚本")}</div>
        </div>
        <div className="ml-auto flex items-center gap-3">
          {!loading && <span className="text-[11.5px] text-ink-4">{t("已装 {installed}/{total}", { installed: installedCount, total: tools.length })}</span>}
          <button
            onClick={() => { setLoading(true); load(); }}
            className="inline-flex items-center gap-1 px-2.5 h-8 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] hover:bg-white/[0.04]"
          >
            <RefreshCw size={13} /> {t("刷新")}
          </button>
        </div>
      </section>

      {/* 立意说明 */}
      <section className="rounded-card border border-accent/25 bg-gradient-to-br from-accent/[0.08] to-transparent px-5 py-3.5">
        <p className="text-[12.5px] text-ink-2 leading-relaxed">
          {t("AI 时代很多活不用你自己开软件 —— ")}<span className="text-ink-0 font-medium">{t("让 AI 去做")}</span>{t("。但 AI 这个「厨师」得有「厨具」：剪视频要 ")}<span className="font-mono text-ink-1">ffmpeg</span>{t("、控网页要 ")}<span className="font-mono text-ink-1">Chrome</span>{t("、做 PPT 要 ")}<span className="font-mono text-ink-1">Python/LibreOffice</span>{t("。按用途一键装好，装完直接对 AI 说「帮我把这几段视频拼成一条」「做个 PPT」即可。")}
        </p>
      </section>

      {/* 按用途推荐组合 */}
      <section className="space-y-2">
        <div className="text-[12px] text-ink-3 px-1">{t("按用途装齐一组厨具（选一个，高亮该装的）：")}</div>
        <div className="flex flex-wrap gap-1.5">
          {BUNDLES.map((b) => (
            <button
              key={b.id}
              onClick={() => setBundle(bundle === b.id ? null : b.id)}
              className={cn(
                "inline-flex items-center gap-1.5 px-3 h-8 rounded-full border text-[12px] transition-colors",
                bundle === b.id ? "border-accent bg-accent/[0.12] text-ink-0" : "border-white/[0.10] text-ink-2 hover:bg-white/[0.04]",
              )}
            >
              <span>{b.emoji}</span> {t(b.name)}
            </button>
          ))}
        </div>
        {activeBundle && (
          <div className="rounded-card border border-accent/30 bg-accent/[0.06] px-4 py-2.5 flex flex-wrap items-center gap-2.5 text-[12px] animate-fade-in">
            <span className="text-ink-2 flex-1">
              <b className="text-ink-0">{activeBundle.emoji} {t(activeBundle.name)}</b> · {t(activeBundle.desc)}
              {bundleMissing.length ? (
                <>{t("—— 还差：")}<span className="text-accent-400">{bundleMissing.join("、")}</span></>
              ) : (
                <>{t("—— 都装好了 ✓")}</>
              )}
              {/* 「使用路径不清晰」（测试报告 #024）：装完之后干嘛，这一页从来没说过。
                  客户装完停在这里，以为还有下一步要做。 */}
              <span className="block text-[11px] text-ink-4 mt-0.5">
                {bundleMissing.length
                  ? t("装齐后不用做别的：回工作台直接叫 AI 干活，它自己会用这些工具。")
                  : t("直接回工作台叫 AI 干活就行，它自己会用这些工具，不用你再配置。")}
              </span>
            </span>
            {bundleMissing.length > 0 && (
              <button
                onClick={() => installBundle(activeBundle)}
                disabled={!!installing}
                className="inline-flex items-center gap-1 px-3 h-7 rounded-lg bg-accent text-white text-[12px] font-medium hover:bg-accent-600 disabled:opacity-50 shrink-0"
              >
                <Download size={13} /> {t("一键装齐缺的")}
              </button>
            )}
          </div>
        )}
      </section>

      {/* 安装进度条 */}
      {installing && (
        <section className="rounded-card border border-accent/30 bg-accent/[0.06] px-4 py-2.5 flex items-center gap-2.5 text-[12.5px] text-ink-1 animate-fade-in">
          <Loader2 size={15} className="text-accent animate-spin shrink-0" />
          <span className="flex-1">{progress || t("安装中…")}</span>
        </section>
      )}

      {loading ? (
        <div className="text-[13px] text-ink-4 px-1">{t("正在读取工具箱…")}</div>
      ) : (
        cats.map((cat) => (
          <section key={cat.name} className="space-y-2.5">
            <h3 className="text-[13px] font-semibold text-ink-1 px-1">
              {CAT_EMOJI[cat.name] ?? "📦"} {t(cat.name)}
            </h3>
            <div className="grid sm:grid-cols-2 gap-3">
              {cat.items.map((tool) => (
                <div
                  key={tool.id}
                  className={cn(
                    "flex items-start gap-3 rounded-card border bg-bg-2/70 px-4 py-3.5 transition-colors",
                    bundleIds.includes(tool.id) ? "border-accent/50 ring-1 ring-accent/40" : "border-white/[0.06]",
                  )}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-[13.5px] font-semibold text-ink-0">{tool.name}</span>
                      {tool.installed && (
                        <span className="inline-flex items-center gap-0.5 text-[10px] text-emerald-400">
                          <Check size={11} /> {t("已装")}
                        </span>
                      )}
                    </div>
                    <div className="text-[11.5px] text-ink-3 leading-snug mt-0.5">{tool.desc}</div>
                    <div className="text-[10px] text-accent-400 mt-1">{tool.uses}</div>
                    {!tool.installed && !tool.can_install && (
                      <div className="text-[10px] text-ink-5 mt-1 font-mono">{t("手动：")}{tool.manual_hint}</div>
                    )}
                  </div>
                  {tool.installed ? (
                    <span className="mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-emerald-500/[0.14]">
                      <Check size={16} className="text-emerald-400" />
                    </span>
                  ) : (
                    <button
                      onClick={() => install(tool)}
                      disabled={!!installing || !tool.can_install}
                      title={tool.can_install ? t("一键安装 {name}", { name: tool.name }) : t("本平台需手动安装（见左侧命令）")}
                      className="mt-0.5 inline-flex items-center gap-1 px-2.5 h-8 shrink-0 rounded-lg border border-accent/40 text-accent text-[11.5px] font-medium hover:bg-accent/[0.10] disabled:opacity-40 disabled:cursor-not-allowed"
                    >
                      {installing === tool.id ? <Loader2 size={13} className="animate-spin" /> : <Download size={13} />}
                      {installing === tool.id ? t("装中") : t("一键装")}
                    </button>
                  )}
                </div>
              ))}
            </div>
          </section>
        ))
      )}

      <p className="text-[11px] text-ink-5 leading-relaxed px-1">
        {t("安装走系统包管理器（Windows=winget · macOS=brew）。部分工具（如 Chrome/LibreOffice）安装时可能弹一次 UAC 授权，点「是」即可。装不上的用卡片里的手动命令，或到官网下载。")}
      </p>
    </div>
  );
}
