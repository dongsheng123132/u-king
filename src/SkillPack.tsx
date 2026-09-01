/**
 * AI 技能包 —— 把「AI 作图 + AI 视频」打包成给任意 AI 工具（OpenClaw / ClawX / Hermes /
 * Claude Code / WorkBuddy …）CLI 调用的技能包。
 *
 * 两个一键动作：① 导出技能包文件夹（后端 export_skill_pack 释放内嵌的 SKILL.md + 脚本）；
 * ② 复制说明文档（含调用命令 + Key + 模型，粘给任意 AI 即可）。
 *
 * **整块可插拔**：纯前端，只靠 props（deviceKey/onToast/onRecharge）通信；内联自己的
 * CodeBlock / Chip 小组件，不依赖 Guide.tsx —— 删本页只删这一个文件。
 */
import { useState } from "react";
import { useI18n } from "./i18n";
import { invoke } from "@tauri-apps/api/core";
import { Check, Copy, Download, FileText, FolderOpen, Package, Sparkles, Wallet } from "lucide-react";
import type { DeviceKey } from "./lib/types";
import { CopyKey } from "./components/CopyKey";
import { IMAGE_MODELS, IMAGE_SIZES, VIDEO_MODELS } from "./lib/models";

// 国内裸网可达的 .cn 域名（见 providers.rs/device.rs）。
const BASE = "https://api.u-claw.org.cn";
const RECHARGE = "https://u-claw.org.cn/recharge";

/** 一段可复制的代码块（内联，不抽公共组件，保持本页自包含）。 */
function CodeBlock({ text, onToast }: { text: string; onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [done, setDone] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setDone(true);
      onToast(t("已复制"));
      window.setTimeout(() => setDone(false), 1600);
    } catch {
      onToast(t("复制失败"));
    }
  };
  return (
    <div className="relative group">
      <pre className="rounded-lg bg-bg-0 border border-white/[0.08] px-3 py-2.5 text-[11.5px] font-mono text-ink-2 overflow-x-auto whitespace-pre-wrap leading-relaxed">
        {text}
      </pre>
      <button
        onClick={copy}
        className="absolute top-2 right-2 inline-flex items-center gap-1 px-2 h-6 rounded border border-white/[0.10] bg-bg-2 text-[10.5px] text-ink-3 hover:text-ink-0 opacity-0 group-hover:opacity-100 transition-opacity"
      >
        {done ? <Check size={11} /> : <Copy size={11} />}
        {done ? t("已复制") : t("复制")}
      </button>
    </div>
  );
}

/** 点击即复制的模型/尺寸标签。 */
function Chip({ id, tag, onToast }: { id: string; tag?: string; onToast: (s: string) => void }) {
  const { t } = useI18n();
  return (
    <button
      onClick={() => navigator.clipboard.writeText(id).then(() => onToast(t("已复制 {id}", { id }))).catch(() => {})}
      className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-md border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] text-[11.5px]"
      title={t("点击复制")}
    >
      <span className="font-mono text-ink-1">{id}</span>
      {tag && <span className="text-[9.5px] text-accent-400">{tag}</span>}
    </button>
  );
}

/** 一段可整体粘给任意 AI 的说明（含 Key —— 满足「复制 apikey + 说明」；附勿公开提示）。
 *  dir = 技能包主位置绝对路径（来自后端 install/export 回显；未知时用 ~ 占位）。**必带绝对路径**
 *  —— 历史上 blob 不写路径导致 AI 瞎猜 skills 目录全扑空（见 memory: aigc-skillpack-path-discovery-gap）。 */
function buildSkillPackBlob(key: string, dir: string): string {
  const img = IMAGE_MODELS.map((m) => m.id).join(", ");
  const vid = VIDEO_MODELS.map((m) => m.id).join(", ");
  return [
    "我本地装了「U-King AIGC」技能包，可作图和生成视频。需要画图/作图/文生图/图生图/生成视频/文生视频/图生视频时请调用它。",
    "",
    "## 技能包在哪（重要：先 cd 进去再运行脚本）",
    `主位置：${dir}`,
    "若你所在的 AI 工具有自己的 skills 目录，技能包可能也已自动装在：~/.claude/skills/uking-aigc、~/.openclaw/skills/uking-aigc、%LOCALAPPDATA%\\hermes\\skills\\aigc\\uking-aigc。任选其一 cd 进去即可。",
    "",
    "## 调用方式（重要：先把我的一句话补成专业提示词、按用途选好 --size，再调脚本——详见技能包 SKILL.md「提示词工作法」）",
    `文生图：node scripts/gen-image.mjs --prompt "补全后的专业描述" --size 1024x1024 --out out.png --json（抖音封面用 1536x2048、小红书 1024x1536、横图 1536x1024）`,
    `图生图/参考图：node scripts/gen-image.mjs --prompt "改成星空背景" --ref input.png --out out.png --json（--ref 可多张融合）`,
    `文生视频：node scripts/gen-video.mjs --prompt "描述" --out out.mp4 --json（默认 Seedance Mini，可加 --duration 5~15 --resolution 720p）`,
    `图生视频：node scripts/gen-video.mjs --prompt "让图动起来" --image first.png --out out.mp4 --json`,
    `批量/多进程（一次并发多张图或多条视频）：node scripts/gen-batch.mjs --prompt "春" --prompt "夏" --concurrency 4 --json（或 --jobs jobs.json 精细指定每条）`,
    "脚本输出 JSON（含 ok / file 绝对路径 / error），退出码 0 成功。",
    "",
    "## API Key（脚本默认自动读 ~/.uking/device.json，一般无需手动传）",
    `如需显式指定：加 --key ${key}`,
    "⚠️ 上面这把 Key 是你的专属充值卡，别发到公开群 / 公开仓库。",
    "",
    "## 可选模型（一般无需指定，用默认即可）",
    `作图：${img}（默认 gpt-image-2）`,
    `视频：${vid}（默认 Seedance Mini；三档 mini/fast/pro 均支持图生视频）`,
    "",
    "## 没有 Node 时可直接用 curl",
    `端点 ${BASE}（鉴权 Authorization: Bearer <KEY>）。作图 POST /v1/images/generations；图生图 POST /v1/images/edits；视频 POST /v1/video/generations（异步轮询，详见技能包 SKILL.md）。`,
    "",
    "## 已安装 SkillHub 图片技能时",
    "SkillHub 的 ch12893719743826428329324 是通用图片生成 skill，可这样接虾盘云：",
    `Base URL：${BASE}/v1`,
    "Model：gpt-image-2",
    "API Type：openai",
    "API Key：同上这把 Key。它只适合作图/参考图改图；视频生成请继续用本 U-King AIGC 技能包的 gen-video.mjs。",
    "",
    `余额 / 充值：${RECHARGE}`,
  ].join("\n");
}


export function SkillPack({
  deviceKey,
  onToast,
  onRecharge,
}: {
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onRecharge: () => void;
}) {
  const { t } = useI18n();
  const key = deviceKey?.key ?? "你的Key";
  const [blobDone, setBlobDone] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installInfo, setInstallInfo] = useState<{ default_dir: string; installed: { tool: string; path: string; experimental?: boolean }[] } | null>(null);
  const packDir = installInfo?.default_dir ?? "~/.uking/skills/uking-aigc";

  const copyBlob = async () => {
    try {
      await navigator.clipboard.writeText(buildSkillPackBlob(deviceKey?.key ?? key, packDir));
      setBlobDone(true);
      onToast(t("已复制说明文档 —— 粘给任意 AI 即可调用"));
      window.setTimeout(() => setBlobDone(false), 1800);
    } catch {
      onToast(t("复制失败"));
    }
  };

  // ① 推荐：释放到 ~/.uking + 探测已装工具自动拷进各自 skills 目录（治「AI 找不到脚本」）。
  const installPack = async () => {
    if (installing) return;
    setInstalling(true);
    try {
      const info = await invoke<{ default_dir: string; installed: { tool: string; path: string; experimental?: boolean }[] }>("install_skill_pack");
      setInstallInfo(info);
      const ready = info.installed.filter((i) => !i.experimental).map((i) => i.tool);
      const exp = info.installed.filter((i) => i.experimental).map((i) => i.tool);
      onToast(
        ready.length
          ? t("已装进 {ready}（共 {count} 个）", { ready: ready.join("、"), count: ready.length }) +
              (exp.length ? t("；{exp} 已拷入但未验证", { exp: exp.join("、") }) : "") +
              t("，并导出到 {dir}", { dir: info.default_dir })
          : t("已导出到 {dir}（未探测到已装的 Claude/OpenClaw，按下方路径手动拷即可）", { dir: info.default_dir }),
      );
    } catch (e) {
      onToast(t("安装失败：") + String(e));
    } finally {
      setInstalling(false);
    }
  };

  // 备选：导出到自选文件夹（弹原生选择框）。
  const exportPack = async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const root = await invoke<string>("export_skill_pack", { dest: null });
      onToast(t("已导出技能包到：") + root);
    } catch (e) {
      onToast(t("导出失败：") + String(e));
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="space-y-5">
      {/* 顶栏：标题 + 复制 Key + 充值 */}
      <section className="flex flex-wrap items-center gap-3 rounded-card border border-white/[0.06] bg-bg-2 px-5 py-3">
        <Package size={18} className="text-accent shrink-0" />
        <div className="min-w-0">
          <div className="text-[15px] font-semibold text-ink-0">{t("AI 技能包")}</div>
          <div className="text-[12px] text-ink-3">{t("把作图 / 视频打包给任意 AI 工具 CLI 调用 · OpenClaw / ClawX / Hermes / Claude Code")}</div>
        </div>
        <div className="ml-auto flex items-center gap-2">
          {deviceKey?.key && <CopyKey apiKey={deviceKey.key} onToast={onToast} />}
          <button
            onClick={onRecharge}
            className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-ink-1 text-[12px] hover:bg-white/[0.04]"
          >
            <Wallet size={13} /> {t("充值")}
          </button>
        </div>
      </section>

      {/* 一句话说明 */}
      <section className="rounded-card border border-accent/30 bg-gradient-to-br from-accent/[0.10] to-transparent px-5 py-4">
        <p className="text-[12.5px] text-ink-2 leading-relaxed">
          {t("这是一个")}<span className="text-ink-0 font-medium">{t("通用技能包")}</span>{t("（SKILL.md + 能跑的脚本）。导出后把")}{" "}
          <span className="font-mono text-ink-1">uking-aigc</span> {t("文件夹拷进任意 AI 工具的技能目录，那个 AI 就能自己「画图 / 生成视频」。Key 已内置（脚本自动读本机设备 Key），对方无需再配。")}
        </p>
      </section>

      {/* A 两个一键大动作 */}
      <section className="grid sm:grid-cols-2 gap-3">
        <button
          data-action-id="runtime.skillpack.install"
          onClick={installPack}
          disabled={installing}
          className="group relative flex items-start gap-3 rounded-card border border-accent/50 bg-gradient-to-br from-accent/[0.14] to-transparent px-4 py-3.5 text-left hover:from-accent/[0.20] hover:border-accent/70 transition-colors disabled:opacity-60"
        >
          <span className="absolute top-2.5 right-3 rounded-full bg-accent/20 px-2 py-0.5 text-[9.5px] font-semibold text-accent">{t("推荐")}</span>
          <span className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent/[0.20]">
            <Download size={17} className="text-accent" />
          </span>
          <span className="min-w-0 pr-8">
            <span className="block text-[13.5px] font-semibold text-ink-0">{t("① 一键装进已装的 AI 工具")}</span>
            <span className="block text-[11px] text-ink-3 leading-snug mt-0.5">
              {installing ? t("正在安装…") : t("自动探测 Claude / OpenClaw·ClawX / Hermes，把技能包拷进各自 skills 目录，并导出一份到 ~/.uking。装完直接对它说「画张图」。")}
            </span>
          </span>
        </button>
        <button
          onClick={copyBlob}
          className="group relative flex items-start gap-3 rounded-card border border-white/[0.14] bg-bg-2/80 px-4 py-3.5 text-left hover:bg-white/[0.05] hover:border-white/[0.22] transition-colors"
        >
          <span className="absolute top-2.5 right-3 inline-flex items-center gap-1 rounded-full bg-white/[0.07] px-2 py-0.5 text-[9.5px] font-medium text-ink-3">
            <FileText size={10} /> {t("一段文字")}
          </span>
          <span className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-white/[0.07]">
            {blobDone ? <Check size={17} className="text-accent" /> : <Sparkles size={17} className="text-ink-2" />}
          </span>
          <span className="min-w-0 pr-2">
            <span className="block text-[13.5px] font-semibold text-ink-0">{t("② 复制说明文档")}</span>
            <span className="block text-[11px] text-ink-3 leading-snug mt-0.5">
              {t("复制一段说明（含调用命令 + Key + 模型）到剪贴板，粘给任意 AI 对话框，它照着调用。")}
            </span>
          </span>
        </button>
      </section>

      {/* 备选导出 + 安装结果回显 */}
      <div className="-mt-2 flex flex-wrap items-center gap-3 text-[11px] text-ink-4">
        <button
          onClick={exportPack}
          disabled={exporting}
          className="underline decoration-dotted underline-offset-2 hover:text-ink-2 disabled:opacity-50"
        >
          {exporting ? t("导出中…") : t("或导出到我指定的文件夹")}
        </button>
        {installInfo && installInfo.installed.some((i) => !i.experimental) && (
          <span className="text-accent">{t("✓ 已装入：")}{installInfo.installed.filter((i) => !i.experimental).map((i) => i.tool).join("、")}</span>
        )}
        {installInfo && installInfo.installed.some((i) => i.experimental) && (
          <span className="text-ink-4">
            · {installInfo.installed.filter((i) => i.experimental).map((i) => i.tool).join("、")}{t("（实验性，已拷入但未验证是否生效）")}
          </span>
        )}
      </div>

      {/* A2 装完怎么用：给小白一句话示例，治「装了不知道怎么触发」 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-2.5">
        <div className="flex items-center gap-2">
          <Sparkles size={15} className="text-accent" />
          <h3 className="text-[13px] font-semibold text-ink-0">{t("装完怎么用？在 AI 里这样说（不用记命令）")}</h3>
        </div>
        <p className="text-[11.5px] text-ink-3 leading-relaxed">
          {t("装好后打开 Claude Code / ClawX（或对话框），直接用大白话说，它会自己调技能包出图 / 出片 —— AI 会替你把一句话补成专业提示词、按用途选好尺寸：")}
        </p>
        <div className="flex flex-wrap gap-1.5">
          {["帮我画一只橘猫宇航员", "做个抖音美食封面", "把这张图背景换成星空（图生图）", "生成一段海浪的短视频", "一次画春夏秋冬四张图"].map((s) => (
            <span
              key={s}
              className="inline-flex items-center px-2.5 h-7 rounded-md border border-white/[0.08] bg-white/[0.02] text-[11.5px] text-ink-2"
            >
              「{t(s)}」
            </span>
          ))}
        </div>
        <p className="text-[10.5px] text-ink-5 leading-relaxed">
          {t("想要别的比例 / 风格，直接追一句「换成竖版」「改成国潮风」即可 —— 以不变应万变，不用每次重敲上下文。")}
        </p>
      </section>

      {/* B 各工具 skills 目录 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
        <div className="flex items-center gap-2">
          <FolderOpen size={15} className="text-accent" />
          <h3 className="text-[13px] font-semibold text-ink-0">{t("没自动装上 / 用的是别的工具？手动把 uking-aigc 拷进对应目录（右上角复制路径）")}</h3>
        </div>
        <div className="space-y-2.5">
          <div className="space-y-1">
            <div className="text-[11px] text-ink-4">Claude Code</div>
            <CodeBlock text="~/.claude/skills/uking-aigc/" onToast={onToast} />
          </div>
          <div className="space-y-1">
            <div className="text-[11px] text-ink-4">{t("OpenClaw / ClawX（便携版在 exe 同级 data/.openclaw/skills/）")}</div>
            <CodeBlock text="~/.openclaw/skills/uking-aigc/" onToast={onToast} />
          </div>
          <div className="space-y-1">
            <div className="text-[11px] text-ink-4">Hermes <span className="text-ink-5">{t("（实验性，未验证是否识别；用不了请改用 Claude Code / ClawX）")}</span></div>
            <CodeBlock text={"%LOCALAPPDATA%\\hermes\\skills\\aigc\\uking-aigc\\"} onToast={onToast} />
          </div>
        </div>
        <p className="text-[11px] text-ink-5 leading-relaxed">
          {t("拷好后重启该工具，对它说「帮我画一张猫」「生成一段海浪视频」即可。其它工具放它自己的 skills 目录同理。")}
        </p>
      </section>

      {/* C 端点 + 模型 chip */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
        <h3 className="text-[13px] font-semibold text-ink-0">{t("接口与模型（点击即复制）")}</h3>
        <div className="space-y-1">
          <div className="text-[11px] text-ink-4">{t("API 端点（Base URL）")}</div>
          <CodeBlock text={BASE} onToast={onToast} />
        </div>
        <div className="space-y-1.5">
          <div className="text-[11px] text-ink-4">{t("作图模型")}</div>
          <div className="flex flex-wrap gap-1.5">
            {IMAGE_MODELS.map((m) => (
              <Chip key={m.id} id={m.id} tag={m.recommend ? t("推荐") : undefined} onToast={onToast} />
            ))}
          </div>
        </div>
        <div className="space-y-1.5">
          <div className="text-[11px] text-ink-4">{t("图片尺寸")}</div>
          <div className="flex flex-wrap gap-1.5">
            {IMAGE_SIZES.map((s) => (
              <Chip key={s.id} id={s.id} tag={s.label} onToast={onToast} />
            ))}
          </div>
        </div>
        <div className="space-y-1.5">
          <div className="text-[11px] text-ink-4">{t("视频模型")}</div>
          <div className="flex flex-wrap gap-1.5">
            {VIDEO_MODELS.map((m) => (
              <Chip key={m.id} id={m.id} tag={m.recommend ? t("推荐") : undefined} onToast={onToast} />
            ))}
          </div>
        </div>
      </section>

      {/* D 命令速查 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
        <h3 className="text-[13px] font-semibold text-ink-0">{t("命令速查（拷到终端即可验证）")}</h3>
        <CodeBlock
          text={[
            "# 文生图",
            'node scripts/gen-image.mjs --prompt "一只橘猫宇航员，电影感" --out cat.png --json',
            "# 图生图（改图）",
            'node scripts/gen-image.mjs --prompt "把背景换成星空" --ref cat.png --out cat2.png --json',
            "# 文生视频（默认 Seedance Mini）",
            'node scripts/gen-video.mjs --prompt "橘猫在月球慢跑，星空" --out cat.mp4 --json',
            "# 图生视频",
            'node scripts/gen-video.mjs --prompt "让这张图动起来" --image cat.png --out anim.mp4 --json',
          ].join("\n")}
          onToast={onToast}
        />
      </section>
    </div>
  );
}
