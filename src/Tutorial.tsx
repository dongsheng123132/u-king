/**
 * 使用教程 —— 给完全不懂的小白看的「这是啥 / 点哪 / 怎么开始」图文上手。
 *
 * 放在「使用教程」tab 第一屏（接入指南 Key 配置在它下方）。纯静态图文，离线可看。
 * 文案口语化，避免任何「命令行 / 配置 / API」黑话。三步走 + 常见问题。
 */
import {
  MessageSquare,
  MousePointerClick,
  Play,
  Sparkles,
  Wallet,
  FileText,
  Code2,
  Table2,
  Languages,
  Image as ImageIcon,
  Search,
} from "lucide-react";
import { useI18n } from "./i18n";

/** 一步卡片。 */
function Step({
  n,
  icon: Icon,
  title,
  children,
}: {
  n: number;
  icon: typeof Play;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex gap-3.5">
      <div className="flex flex-col items-center shrink-0">
        <div className="flex h-9 w-9 items-center justify-center rounded-full bg-accent text-white text-[15px] font-bold">
          {n}
        </div>
        <div className="w-px flex-1 bg-white/[0.08] my-1" />
      </div>
      <div className="pb-5 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <Icon size={15} className="text-accent" />
          <h3 className="text-[14px] font-semibold text-ink-0">{title}</h3>
        </div>
        <div className="text-[12.5px] leading-relaxed text-ink-2">{children}</div>
      </div>
    </div>
  );
}

/** 常见问题一条。 */
function Faq({ q, a }: { q: string; a: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-white/[0.06] bg-bg-2/60 px-4 py-3">
      <div className="text-[12.5px] font-medium text-ink-1 mb-1">{q}</div>
      <div className="text-[12px] leading-relaxed text-ink-3">{a}</div>
    </div>
  );
}

export function Tutorial({ onGoMyAI }: { onGoMyAI: () => void }) {
  const { t } = useI18n();
  return (
    <div className="space-y-5">
      {/* 欢迎头 */}
      <section className="rounded-card border border-accent/30 bg-gradient-to-br from-accent/[0.12] to-transparent px-5 py-4">
        <div className="flex items-center gap-2 mb-1.5">
          <Sparkles size={18} className="text-accent" />
          <h2 className="text-[15px] font-semibold text-ink-0">{t("几步开始用 AI · 完全不用懂电脑")}</h2>
        </div>
        <p className="text-[12.5px] text-ink-3 leading-relaxed">
          {t("U-King 是一个「AI 管家」。它已经帮你把全球最强的 AI 都装好、配好了，你只要照下面几步点一点，就能像微信聊天一样跟 AI 对话——让它写文章、写代码、做表格、查资料、画图，都行。")}
        </p>
      </section>

      {/* 三步走 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-5">
        <Step n={1} icon={MousePointerClick} title={t("装好主力工具 ClawX（图形版 AI 助手）")}>
          {t("新手首选 ")}<b className="text-ink-1">🦞 ClawX</b>{t("——图形界面，像微信一样点点就能用，最好上手。在「我的 AI」里点 ClawX，U-King 会帮你")}<b className="text-ink-1">{t("自动下载、安装、配好 AI")}</b>{t("，你只要等它装完。")}
          <button
            onClick={onGoMyAI}
            className="ml-1 inline-flex items-center gap-1 text-accent hover:text-ink-0 underline underline-offset-2"
          >
            {t("去「我的 AI」")}
          </button>
        </Step>
        <Step n={2} icon={Play} title={t("打开 ClawX，记得点「允许访问网络」")}>
          {t("装好后点「打开 ClawX」。第一次打开时 Windows 可能弹一个「是否允许访问网络」的窗口，")}
          <b className="text-ink-1">{t("一定要点【允许访问】")}</b>{t("，不然 AI 连不上。")}
          <span className="text-ink-4">{t("（U-King 多数情况已帮你提前放行，不一定会弹。）")}</span>
        </Step>
        <Step n={3} icon={Wallet} title={t("第一次用，先充值开通（¥20 起，够聊很久）")}>
          {t("AI 是按量计费的，")}<b className="text-ink-1">{t("第一次使用前需要先充值开通")}</b>{t("。在「我的 AI」或「接入指南」页右上角点")}<b className="text-ink-1">{t("「充值」")}</b>{t("，会自动填好你这台电脑的专属 Key，微信扫码即可，")}
          <b className="text-accent-400">{t("¥20 起充，¥1 = 50 万 token")}</b>{t("，到账即时、余额永久有效、不用不扣。")}
        </Step>
        <Step n={4} icon={MessageSquare} title={t("像聊天一样打字，回车发送")}>
          {t("ClawX 打开后，在输入框直接打字，比如「")}<i className="text-ink-1">{t("帮我写一封请假邮件")}</i>{t("」，按回车，AI 就会回你。想让它做啥就直说，说中文就行。")}
        </Step>
        <div className="flex gap-3.5">
          <div className="flex flex-col items-center shrink-0">
            <div className="flex h-9 w-9 items-center justify-center rounded-full bg-accent/20 text-accent text-[15px]">
              ✓
            </div>
          </div>
          <div className="text-[12.5px] text-ink-2 pt-1.5">{t("就这么简单。剩下的，问 AI 自己就好。")}</div>
        </div>
      </section>

      {/* AI 能帮你做什么 */}
      <section className="rounded-card border border-white/[0.06] bg-bg-2/70 px-5 py-4">
        <h3 className="text-[13px] font-semibold text-ink-0 mb-3">{t("AI 能帮你做什么？（直接打字问它就行）")}</h3>
        <div className="grid grid-cols-2 sm:grid-cols-3 gap-2.5">
          {[
            { icon: FileText, t: "写文章 / 邮件", e: "「帮我写一份述职报告」" },
            { icon: Code2, t: "写代码 / 改 bug", e: "「写个 Excel 自动改名脚本」" },
            { icon: Table2, t: "做表格 / 整理数据", e: "「把这段文字整理成表格」" },
            { icon: Languages, t: "翻译 / 润色", e: "「把这段翻成地道英文」" },
            { icon: Search, t: "查资料 / 出主意", e: "「给孩子起 10 个名字」" },
            { icon: ImageIcon, t: "AI 画图", e: "在「AI 作图」里输入即可" },
          ].map((c) => (
            <div
              key={c.t}
              className="rounded-lg border border-white/[0.06] bg-bg-1/60 px-3 py-2.5"
            >
              <div className="flex items-center gap-1.5 mb-1">
                <c.icon size={13} className="text-accent shrink-0" />
                <span className="text-[12px] font-medium text-ink-1">{t(c.t)}</span>
              </div>
              <div className="text-[10.5px] leading-tight text-ink-4">{t(c.e)}</div>
            </div>
          ))}
        </div>
        <p className="mt-3 text-[11.5px] text-ink-3 leading-relaxed">
          {t("想要更聪明的回答？在「我的 AI」每个工具下点「单独给这个工具换模型（高级）」可")}<b className="text-ink-1">{t("换 AI")}</b>{t("——不确定就用")}<b className="text-accent-400">{t("「DeepSeek V4 Pro（推荐）」")}</b>{t("，又快又省；要写代码 / 攻难题，换")}<b className="text-ink-1">Claude</b>{t(" 系更强（更费额度）。每个选项下都有一句人话说明。")}
        </p>
      </section>

      {/* 常见问题 */}
      <section className="space-y-2.5">
        <h3 className="text-[13px] font-semibold text-ink-0 px-1">{t("新手常见问题")}</h3>
        <Faq
          q={t("要花钱吗？怎么才能开始用？")}
          a={
            <>
              {t("用 AI 是按量计费的（说几句话花几分钱）。")}<b className="text-ink-1">{t("第一次使用需要先充值开通")}</b>{t("——充值入口在「我的 AI」和「接入指南」页右上角，")}
              <span className="inline-flex items-center gap-0.5">
                <Wallet size={11} className="text-accent" /> {t("¥20 起充")}
              </span>
              {t("，到账即时、余额永久有效、不用不扣，¥20 通常够聊很久。")}
            </>
          }
        />
        <Faq q={t("ClawX 图标是灰色的、点不动？")} a={t("那是 ClawX 还没装。点一下灰色图标，U-King 会自动帮你下载安装（约 210MB，会显示进度），等它装完就变彩色、能打开了。")} />
        <Faq
          q={t("ClawX 一直转圈、连不上 AI？")}
          a={t("八成是第一次打开时那个「是否允许访问网络」的窗口被点了「取消」。解决：关掉 ClawX，回 U-King 重新点「打开 ClawX」，这次弹窗点【允许访问】即可。详见盘内《第一次打开 ClawX 必看》。")}
        />
        <Faq
          q={t("充了钱但还是说余额不足 / 连不上？")}
          a={t("回到「AI 设置」点一下「测试连通 / 查询余额」刷新一下。还不行就看盘内《常见故障排查手册》，或按《远程协助看这里》联系我们远程帮你弄。")}
        />
        <Faq
          q={t("怎么看还剩多少额度？怎么充值？")}
          a={t("在「我的 AI」首页右上角就显示余额（多少万 token）。要充值点旁边的「高级 / 余额」或「接入指南」里的充值按钮，会打开充值页、自动填好你的 Key，微信扫码即可，¥20 起充，到账即时、永久有效。")}
        />
        <Faq
          q={t("家里 / 单位几台电脑能共用吗？")}
          a={t("可以。同一个 Key（额度）能同时配到多台电脑、手机 App 上用，额度共享、一起扣。在「接入指南」里复制 Key，按上面的说明配到别的设备即可。")}
        />
        <Faq q={t("关掉窗口 AI 就停了吗？")} a={t("U-King 点右上角「缩到托盘」是最小化到右下角，还在后台跑。ClawX 是独立程序，直接关它的窗口即可。点工具卡的「打开终端」会弹出独立的终端窗口，关掉 U-King 也不影响它。")} />
      </section>

      <div className="text-[11px] text-ink-5 px-1">
        {t("下面是给进阶用户看的「接入指南」——把这台电脑的 AI 额度配到手机 App、其它软件里用。新手可以先不看。")}
      </div>
    </div>
  );
}
