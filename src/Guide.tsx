/**
 * 接入指南 —— 告诉用户怎么把「虾盘云」内置 Key 配到任意第三方平台/工具。
 *
 * 一个 Key 调遍全球大模型：API 地址 + Key + 热门模型 + 各平台接入示例。
 * Key 一键复制（纯 Key / ClawX 导入串）。所有信息基于实测（2026-06-16）。
 */
import { useState } from "react";
import { Check, CheckCircle2, ClipboardCopy, Copy, ExternalLink, FileText, Globe, KeyRound, Sparkles, Wallet } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "./i18n";
import type { DeviceKey } from "./lib/types";
import { WalletCard } from "./components/WalletCard";
import { XIAPAN_MODELS, IMAGE_MODELS, VIDEO_MODELS } from "./lib/models";

// 国内镜像：裸 api.u-claw.org 国内 GFW SNI reset，客户机连不上（见 providers.rs/device.rs）。
const BASE_OPENAI = "https://api.u-claw.org.cn/v1";
const BASE_ANTHROPIC = "https://api.u-claw.org.cn";
const RECHARGE = "https://u-claw.org.cn/recharge";

/** 热门模型（2026-08-01 逐个真调 `/v1/chat/completions` 验过在线，不是照抄各家发布稿）。
 *
 *  ⚠️ 两条只放这里、别凭「官网发了新版」就改的规矩：
 *  ① **只收裸 id 且真有直连渠道的**。虾盘云 `/v1/models` 同时列裸名（直连）和 `anthropic/…`
 *     这类中转前缀名，两者不等价 —— 实测 `claude-opus-5` 裸名 503（无渠道）、
 *     `anthropic/claude-opus-5` 才 200。写错客户复制过去只会收到一句看不懂的报错。
 *  ② **故意不放 gpt-5.5 / gpt-5.6-***。那是 `models.ts::priceyModelHint` 专门警告的一档
 *     （有客户拿它一天烧掉 ¥169），而这排 chip 是「点一下就复制」，没有地方挂成本警告。
 *     要用前沿档的，从下面「展开全部模型清单」里挑，那里有 desc 说清楚多贵。 */
const HOT_MODELS: { id: string; tag: string }[] = [
  { id: "deepseek-v4-flash", tag: "最快最省" },
  { id: "claude-sonnet-5", tag: "编程之王" },
  { id: "gpt-5.4", tag: "OpenAI 旗舰" },
  { id: "gemini-3.1-pro-preview", tag: "谷歌" },
  { id: "kimi-k3", tag: "超长上下文" },
  { id: "gpt-image-2", tag: "画图" },
];

/** 「复制配置文档」生成的 markdown —— 整段粘给任意 AI（ClawX/Claude/Codex/任意聊天框），
 *  它就能照着把自己配好。含：双格式地址 + Key + 全部模型 id 清单 + /v1/models 自发现 + 各工具示例。
 *  这是中转站的通行做法：把模型 id 都列出来，AI/用户照抄即可，不用猜。 */
function buildConfigBlob(key: string): string {
  const ids = XIAPAN_MODELS.flatMap((g) => g.items.map((m) => m.id));
  const imgIds = IMAGE_MODELS.map((m) => m.id);
  const videoIds = VIDEO_MODELS.map((m) => m.id);
  return [
    "请帮我把下面这个 AI API 接入当前工具（OpenAI 兼容，按量计费、余额永久有效）：",
    "",
    "## 接口地址",
    `- OpenAI 兼容 Base URL（多数工具 / ClawX / Codex）：${BASE_OPENAI}`,
    `- Anthropic 格式 Base URL（Claude Code 专用）：${BASE_ANTHROPIC}`,
    `- API Key：${key}`,
    "",
    `## 可用模型（直接填模型名；完整清单可 GET ${BASE_OPENAI}/models 拉取）`,
    `对话模型：${ids.join(", ")}`,
    `画图模型：${imgIds.join(", ")}`,
    `视频模型：${videoIds.join(", ")}`,
    "",
    "## 选哪个",
    "- 日常最快最省：deepseek-v4-flash",
    "- 写代码 / 改 bug：claude-sonnet-5",
    "- 综合最强：gpt-5.4",
    "- 作图默认：gpt-image-2",
    "- 文生视频：doubao-seedance-2-0-mini-260615（火山 Seedance，中文好、国内直连）",
    "- 图生视频 / 让图片动起来：也用 doubao-seedance-2-0-mini-260615（要更清晰换 doubao-seedance-2-0-fast-260128）",
    "",
    "## 接入示例",
    `- ClawX / 多数工具：添加「OpenAI 兼容」供应商 → Base URL=${BASE_OPENAI} → Key=上面的 Key → 模型填上面任一。`,
    `- Claude Code：设环境变量 ANTHROPIC_BASE_URL=${BASE_ANTHROPIC} 和 ANTHROPIC_AUTH_TOKEN=上面的 Key。`,
    "",
    "## 重要：作图 / 视频不是只填 API 就够",
    "如果要让 ClawX/OpenClaw 自己会「画图、改图、生成视频、图生视频」，需要安装 U-King AIGC 技能包（SKILL.md + scripts），不是只记住上面的模型名。",
    "在 U-King 里点「② 虾盘云·充值」→「自动配好」会自动释放技能包；也可以打开「AI 技能包」页点「一键装进已装的 AI 工具」。",
    "技能包常见位置：",
    "- 主位置：~/.uking/skills/uking-aigc/",
    "- ClawX / OpenClaw：~/.openclaw/skills/uking-aigc/",
    "安装后重启 ClawX，再说「用 U-King AIGC 技能包画张图」或「把这张图做成 5 秒视频」。",
    "",
    "技能包命令示例：",
    `- 文生图：node ~/.openclaw/skills/uking-aigc/scripts/gen-image.mjs --prompt "一只橘猫宇航员" --out cat.png --json`,
    `- 图生视频：node ~/.openclaw/skills/uking-aigc/scripts/gen-video.mjs --prompt "让这张图动起来" --image input.png --out out.mp4 --json`,
    "",
    "## SkillHub 图片技能（ch12893719743826428329324）",
    `- Base URL：${BASE_OPENAI}`,
    `- Model：gpt-image-2`,
    "- API Type：openai",
    "- 参考图：这个 SkillHub 技能走 OpenAI/GPT-Image 图片接口；视频不要用它，视频请用 U-King AIGC 的 gen-video.mjs。",
    "",
    `充值：${RECHARGE}`,
  ].join("\n");
}

/** 一段可复制的代码块。 */
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

export function Guide({
  deviceKey,
  onToast,
  onRecharge,
  onRefreshBalance,
  onDeviceKeyChange,
  onApplyXiapan,
  onUseOwnKey,
}: {
  deviceKey: DeviceKey | null;
  onToast: (s: string) => void;
  onRecharge: () => void;
  onRefreshBalance?: () => void;
  /** 钱包里换/填 Key 之后把新 DeviceKey 交回 App —— 否则整个界面还显示旧 Key。 */
  onDeviceKeyChange?: (dk: DeviceKey) => void;
  /** 「一键配好全部」：后端自探已装工具写入虾盘云 Key，并安装 AIGC 技能包。 */
  onApplyXiapan?: () => void;
  /** 「我有自己的 Key」：跳到 AI 设置页（换驱动/自定义中转）。虾盘云不稳时的应急出口。 */
  onUseOwnKey?: () => void;
}) {
  const { t } = useI18n();
  const key = deviceKey?.key ?? "你的Key";
  const charged = !!deviceKey?.charged;
  const [blobDone, setBlobDone] = useState(false);

  const copyBlob = async () => {
    try {
      await navigator.clipboard.writeText(buildConfigBlob(deviceKey?.key ?? key));
      setBlobDone(true);
      onToast(t("已复制配置串 —— 粘给任何 AI 即可"));
      window.setTimeout(() => setBlobDone(false), 1800);
    } catch {
      onToast(t("复制失败"));
    }
  };

  return (
    <div className="space-y-5">
      {/* 主力推荐 ClawX 卡已挪到「①装 AI」页第一屏（主推不该埋在第二步），此处不再重复。 */}

      {/* 头：开通状态 + Key + 充值 */}
      <section className="rounded-card border border-accent/30 bg-gradient-to-br from-accent/[0.12] to-bg-2 px-5 py-4 shadow-card">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2 mb-2">
              <Wallet size={18} className="text-accent" />
              <h2 className="text-[15px] font-semibold text-ink-0">{t("虾盘云充值 · 免费装工具，充值用 AI")}</h2>
            </div>
            <p className="text-[12px] text-ink-3 leading-relaxed max-w-2xl">
              {t("U-King 会给这台电脑生成专属 Key。你不用注册国外账号，也不用填信用卡；充值后这个 Key 就能在 ClawX、Claude Code、Codex 里调用大模型。")}
            </p>
          </div>
          <div className="w-full sm:w-auto sm:min-w-[180px] rounded-lg border border-white/[0.08] bg-bg-0/50 px-3 py-2.5">
            <div className="text-[10.5px] text-ink-4">{t("当前状态")}</div>
            <div className="mt-1 text-[15px] font-semibold text-ink-0">
              {deviceKey ? (charged ? t("已开通") : t("待充值开通")) : t("正在生成 Key")}
            </div>
            <div className="mt-1 text-[11px] text-ink-3">
              {charged ? t("余额 {bal}", { bal: deviceKey?.balance?.text ?? t("可用") }) : t("充值后即可使用 AI")}
            </div>
          </div>
        </div>

        <div className="mt-4 grid gap-2 sm:grid-cols-3">
          {[
            { done: !!deviceKey?.key, label: t("生成专属 Key"), desc: deviceKey?.key ? t("已绑定本机") : t("稍等几秒自动生成") },
            { done: charged, label: t("微信/支付宝充值"), desc: charged ? t("余额已可用") : t("¥20 起充，到账即时") },
            { done: charged, label: t("开始用 AI"), desc: t("聊天、写代码、画图、视频") },
          ].map((s, i) => (
            <div
              key={s.label}
              className="flex items-start gap-2 rounded-lg border border-white/[0.08] bg-bg-0/35 px-3 py-2.5"
            >
              <span className={`mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-full text-[11px] font-bold ${s.done ? "bg-success-500 text-white" : "bg-accent/20 text-accent-400"}`}>
                {s.done ? <CheckCircle2 size={13} /> : i + 1}
              </span>
              <span className="min-w-0">
                <span className="block text-[12px] font-semibold text-ink-0">{s.label}</span>
                <span className="block text-[10.5px] text-ink-3">{s.desc}</span>
              </span>
            </div>
          ))}
        </div>

        {/* 钱包（余额 / 充值 / 复制 Key / 换一把 / 用已有钱包）统一用 WalletCard。
            这里原来自己拼了「Key 芯片 + 复制 + 余额 + 充值 + 刷新」一整排，而「换一把 /
            填一把」只在进阶页有 —— 同一个钱包被切成两半摆在两页，客户在这页找不到换 Key。 */}
        <WalletCard
          className="mt-4"
          deviceKey={deviceKey}
          onDeviceKeyChange={onDeviceKeyChange}
          onRefresh={onRefreshBalance}
          onRecharge={onRecharge}
          onToast={onToast}
        />
      </section>

      {/* 「用自己的 Key」出口 —— 一等公民（2026-07-17 产品决策）：不锁死在虾盘云。
          有 DeepSeek/Kimi/GLM 官方 Key 或任意中转 Key 的用户，30 秒切走；
          虾盘云偶发不稳时这也是客户的自救通道（不用等我们修）。 */}
      {onUseOwnKey && (
        <button
          onClick={onUseOwnKey}
          className="w-full flex items-center gap-3 rounded-card border border-white/[0.10] bg-bg-2/60 px-4 py-3 text-left hover:bg-white/[0.05] hover:border-white/[0.20] transition-colors"
        >
          <span className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-white/[0.06]">
            <KeyRound size={15} className="text-ink-2" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-[13px] font-semibold text-ink-0">{t("我有自己的 API Key，不用虾盘云")}</span>
            <span className="block text-[11px] text-ink-3 leading-snug mt-0.5">
              {t("DeepSeek / Kimi / 智谱官方 Key，或任意中转站 Key 都行 —— 去「AI 设置」粘贴即用；虾盘云暂时不稳时也可先切过去应急。")}
            </span>
          </span>
          <span className="shrink-0 text-[11px] text-accent font-medium">{t("去配置 →")}</span>
        </button>
      )}

      {/* 两个一键大动作：① 自动配好已装工具 ② 复制一段文字粘给任意 AI。
          做显眼 + 标题让人一眼懂「这是两种配置方式，任选其一」。 */}
      <section className="space-y-2">
        <div className="flex items-center gap-2 px-0.5">
          <h3 className="text-[13px] font-semibold text-ink-0">{t("充值后怎么用")}</h3>
          <span className="text-[11px] text-ink-4">{t("小白点「自动配好」；会折腾的再看复制配置")}</span>
        </div>
        <div className="grid gap-3 lg:grid-cols-2">
          {onApplyXiapan && (
            <button
              data-action-id="runtime.driver.apply_everywhere"
              onClick={onApplyXiapan}
              className="group relative min-w-0 flex items-start gap-3 rounded-card border border-accent/50 bg-gradient-to-br from-accent/[0.14] to-transparent px-4 py-3.5 text-left shadow-[0_0_0_1px_rgba(0,0,0,0)] hover:from-accent/[0.20] hover:border-accent/70 transition-colors"
            >
              <span className="absolute top-2.5 right-3 hidden rounded-full bg-accent/20 px-2 py-0.5 text-[9.5px] font-semibold text-accent sm:inline-flex">{t("推荐")}</span>
              <span className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent/[0.20]">
                <Sparkles size={17} className="text-accent" />
              </span>
              <span className="min-w-0 sm:pr-8">
                <span className="flex min-w-0 flex-wrap items-center gap-1.5 text-[13.5px] font-semibold text-ink-0">
                  <span>{t("① 自动配好已装工具")}</span>
                  <span className="rounded-full bg-accent/20 px-1.5 py-0.5 text-[9.5px] font-semibold text-accent sm:hidden">{t("推荐")}</span>
                </span>
                <span className="block text-[11px] text-ink-3 leading-snug mt-0.5">
                  {t("自动探测已装的 Claude Code / Codex / ClawX / OpenClaw / Hermes，把 Key 和模型一次写好。")}
                </span>
              </span>
            </button>
          )}
          <button
            onClick={copyBlob}
            className="group relative min-w-0 flex items-start gap-3 rounded-card border border-white/[0.14] bg-bg-2/80 px-4 py-3.5 text-left hover:bg-white/[0.05] hover:border-white/[0.22] transition-colors"
          >
            <span className="absolute top-2.5 right-3 hidden items-center gap-1 rounded-full bg-white/[0.07] px-2 py-0.5 text-[9.5px] font-medium text-ink-3 sm:inline-flex">
              <FileText size={10} /> {t("一段文字")}
            </span>
            <span className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-white/[0.07]">
              {blobDone ? <Check size={17} className="text-accent" /> : <ClipboardCopy size={17} className="text-ink-2" />}
            </span>
            <span className="min-w-0 sm:pr-20">
              <span className="flex min-w-0 flex-wrap items-center gap-1.5 text-[13.5px] font-semibold text-ink-0">
                <span>{t("② 复制配置文档")}</span>
                <span className="inline-flex items-center gap-1 rounded-full bg-white/[0.07] px-1.5 py-0.5 text-[9.5px] font-medium text-ink-3 sm:hidden">
                  <FileText size={10} /> {t("进阶")}
                </span>
              </span>
              <span className="block text-[11px] text-ink-3 leading-snug mt-0.5">
                {t("点一下会")}<span className="text-ink-1 font-medium">{t("复制一段说明文字")}</span>{t("到剪贴板——里面是地址+Key+全部模型。粘到任意 AI 对话框（ClawX / 聊天框），它照着自己配。")}
              </span>
              {/* 给客户一眼看出"复制的是文本"：缩略示意，不是真要读的内容 */}
              <span className="mt-1.5 block rounded-md bg-bg-0/70 border border-white/[0.06] px-2 py-1 font-mono text-[9.5px] text-ink-4 leading-tight truncate">
                {t("# 虾盘云配置 · 地址 https://api.u-claw.org.cn/v1 · Key sk-… · 模型 deepseek-v4-flash …")}
              </span>
            </span>
          </button>
        </div>
      </section>

      <div className="flex items-center gap-3 pt-1">
        <div className="h-px flex-1 bg-white/[0.06]" />
        <span className="text-[11px] text-ink-5">{t("下面是进阶手动配置，普通用户不用看")}</span>
        <div className="h-px flex-1 bg-white/[0.06]" />
      </div>

      {/* 中转站定位：很多人不知道这台电脑已经能当「免 VPN 中转站」用——
          下面这套地址+Key 不是只给 U-King 认识的几个工具用的，任何支持
          「自定义模型 / OpenAI 兼容」的 AI 工具都能填进去直接用上外网大模型。 */}
      <section className="rounded-card border border-accent/25 bg-accent/[0.06] px-4 py-3">
        <div className="flex items-start gap-2.5">
          <Globe size={15} className="text-accent mt-0.5 shrink-0" />
          <div className="text-[11.5px] text-ink-2 leading-relaxed">
            <span className="text-ink-0 font-semibold">{t("这台电脑现在就是一个「中转站」")}</span>
            {t("——不用挂 VPN、不用注册国外账号。把下面的 ")}<span className="text-ink-1">{t("API 地址 + Key + 模型 ID")}</span>{" "}
            {t("复制到")}<span className="text-ink-1 font-medium">{t("任何支持「自定义模型 / OpenAI 兼容」的 AI 工具")}</span>
            {t("（不只是 Claude Code / Codex / ClawX，沉浸式翻译、Cursor 之类第三方 AI 应用同样能填），就能直接用上 GPT-5.4、Claude、Gemini 这些海外模型。")}
          </div>
        </div>
      </section>

      {/* API 地址 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
        <h3 className="text-[13px] font-semibold text-ink-0">{t("① API 地址（填到工具的 Base URL）")}</h3>
        <div className="grid sm:grid-cols-2 gap-3">
          <div className="space-y-1.5">
            <div className="text-[11px] text-ink-4">{t("通用地址 · OpenAI 标准（多数工具 / Codex / ClawX）")}</div>
            <CodeBlock text={BASE_OPENAI} onToast={onToast} />
          </div>
          <div className="space-y-1.5">
            <div className="text-[11px] text-ink-4">{t("Claude 专用地址（Claude Code 用）")}</div>
            <CodeBlock text={BASE_ANTHROPIC} onToast={onToast} />
          </div>
        </div>
      </section>

      {/* 热门模型 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-3">
        <h3 className="text-[13px] font-semibold text-ink-0">{t("② 热门模型（填到工具的「模型名」）")}</h3>
        <div className="flex flex-wrap gap-2">
          {HOT_MODELS.map((m) => (
            <button
              key={m.id}
              onClick={() => {
                navigator.clipboard.writeText(m.id).then(() => onToast(t("已复制模型名 {id}", { id: m.id }))).catch(() => {});
              }}
              className="inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] text-[12px]"
              title={t("点击复制模型名")}
            >
              <span className="font-mono text-ink-1">{m.id}</span>
              <span className="text-[10px] text-accent-400">{t(m.tag)}</span>
            </button>
          ))}
        </div>

        {/* 全部模型清单（按分组）：点任一即复制模型 id。一个 Key 调遍下面所有模型。 */}
        <details className="group rounded-lg border border-white/[0.06] bg-bg-0/40 px-3 py-2">
          <summary className="cursor-pointer list-none text-[12px] text-ink-3 hover:text-ink-1 select-none">
            {t("展开全部模型清单（聊天 + 画图，点击即复制 id）")}
          </summary>
          <div className="mt-2.5 space-y-3">
            {XIAPAN_MODELS.map((g) => (
              <div key={g.group} className="space-y-1.5">
                <div className="text-[11px] text-ink-4">{t(g.group)}</div>
                <div className="flex flex-wrap gap-1.5">
                  {g.items.map((m) => (
                    <button
                      key={m.id}
                      onClick={() => {
                        navigator.clipboard.writeText(m.id).then(() => onToast(t("已复制模型名 {id}", { id: m.id }))).catch(() => {});
                      }}
                      className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-md border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] text-[11.5px]"
                      title={m.desc || t("点击复制模型名")}
                    >
                      <span className="font-mono text-ink-1">{m.id}</span>
                      {m.recommend && <span className="text-[9.5px] text-accent-400">{t("推荐")}</span>}
                    </button>
                  ))}
                </div>
              </div>
            ))}
            <div className="space-y-1.5">
              <div className="text-[11px] text-ink-4">{t("画图模型（AI 作图用）")}</div>
              <div className="flex flex-wrap gap-1.5">
                {IMAGE_MODELS.map((m) => (
                  <button
                    key={m.id}
                    onClick={() => {
                      navigator.clipboard.writeText(m.id).then(() => onToast(t("已复制模型名 {id}", { id: m.id }))).catch(() => {});
                    }}
                    className="inline-flex items-center gap-1.5 px-2.5 h-7 rounded-md border border-white/[0.08] bg-white/[0.02] hover:bg-white/[0.05] text-[11.5px]"
                    title={t("点击复制模型名")}
                  >
                    <span className="font-mono text-ink-1">{m.id}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </details>
      </section>

      {/* 各平台接入示例 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4 space-y-4">
        <h3 className="text-[13px] font-semibold text-ink-0">{t("③ 各平台接入示例")}</h3>

        <div className="space-y-1.5">
          <div className="text-[12px] font-medium text-ink-1">{t("Claude Code（命令行）")}</div>
          <CodeBlock
            text={`export ANTHROPIC_BASE_URL=${BASE_ANTHROPIC}\nexport ANTHROPIC_AUTH_TOKEN=${key}`}
            onToast={onToast}
          />
          <div className="text-[11px] text-ink-4">{t("提示：U-King「虾盘云·充值」里点「自动配好」即可写入 ClawX / OpenClaw，不用手动填。")}</div>
        </div>

        <div className="space-y-1.5">
          <div className="text-[12px] font-medium text-ink-1">{t("Codex（~/.codex/config.toml）")}</div>
          <CodeBlock
            text={`model = "deepseek-v4-flash-codex"\nmodel_provider = "xiapan"\n\n[model_providers.xiapan]\nbase_url = "${BASE_OPENAI}"\nwire_api = "responses"`}
            onToast={onToast}
          />
        </div>

        <div className="space-y-1.5">
          <div className="text-[12px] font-medium text-ink-1">{t("ClawX / 其它 OpenAI 兼容工具")}</div>
          <div className="text-[11.5px] text-ink-3 leading-relaxed">
            {t("添加供应商 → 接入模式选「纯 API / OpenAI 兼容」→ Base URL 填")}{" "}
            <span className="font-mono text-ink-1">{BASE_OPENAI}</span>{" "}
            {t("→ API Key 填你的 Key（上方一键复制）→ 模型填上面任一热门模型 → 保存。")}
          </div>
        </div>
      </section>

      {/* 官网链接 —— 多端点探测（会审 2026-08-27：裸域 u-king.org 境内 SNI reset 点不动，
          改调 resolve_site_url：u-claw.org.cn/uking/ 首选、www.u-king.org 备选、全挂 fallback 国内地址） */}
      <section className="flex flex-wrap items-center gap-3 text-[12px]">
        <button
          onClick={() => {
            void (async () => {
              let u = "https://u-claw.org.cn/uking/";
              try { u = await invoke<string>("resolve_site_url"); } catch { /* 保持 fallback */ }
              openUrl(u).catch(() => {});
            })();
          }}
          className="inline-flex items-center gap-1.5 text-accent hover:text-ink-0"
        >
          <ExternalLink size={12} /> {t("全部模型与价格 · 官网")}
        </button>
        <span className="text-ink-5">·</span>
        <span className="text-ink-4">{t("余额永久有效，按量计费，不用不扣")}</span>
      </section>
    </div>
  );
}
