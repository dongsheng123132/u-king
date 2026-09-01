// 网站 GEO 体检 —— 侧边栏功能页。**2026-08-24 起是「展示 + 转化」页，不是自助工具页。**
//
// 页面只做三件事：
//  ① 免费自查（geo_scan）：40+ 渠道体检面板，打开真实搜索页让客户自己看。
//     **纯离线、不需 Key、不花钱、没有失败面** —— 这一页唯一真跑的东西。
//  ② 样板报告：一份**虚构公司的演示报告**（静态 HTML，预渲染进技能包），
//     用来说明「我们出的报告长什么样」。不检测客户网站、不消耗额度。
//  ③ 加微信 hecare888：唯一的成交出口。
//
// 🔴 **为什么不在这儿真跑 AI 可见度测试**（原 `geo_aicheck` / `geo_inspect` 已删）：
//   · 它会调 6 个大模型烧我们的内置额度，而模型 id 会烂、网络会断，失败面大；
//   · 更要紧的是 `1so-geo/src/llm.mjs` 会**自己**读 `~/.uking/device.json` 拿虾盘云 Key，
//     所以只要那些脚本躺在客户机上，命令行就能调起来烧我们的钱 ——
//     **真正的闸门在 `geo.rs::SKILL_FILES` / `REMOVED_FILES`，不在这个文件里。摘按钮挡不住命令行。**
// 🔴 **也不留自助下单**：服务端没有「GEO 订单」概念，付款和普通 token 充值不可区分，
//   客户不写备注就静默丢单。成交动作统一收进微信，直到服务端补上那一环。
//
// 能力和代码一行没丢：完整技能包在仓库 `src-tauri/skills/1so-geo/`，我们人工出报告用它。
// 独立可插拔：删本功能只动 App.tsx（去 import + tab）与 lib.rs（去 geo 三命令）。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Gauge, Search, Loader2, ExternalLink, AlertTriangle, Compass, FileCheck2, MessageCircle, Rocket } from "lucide-react";
import { useI18n } from "./i18n";

type GeoScan = { panel: string; channels: number; auto_ran: boolean };

export function Geo({ onToast }: { onToast: (s: string) => void }) {
  const { t } = useI18n();
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [name, setName] = useState("");
  const [region, setRegion] = useState("");
  const [industry, setIndustry] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<GeoScan | null>(null);
  const [busySample, setBusySample] = useState(false);

  useEffect(() => {
    invoke<boolean>("geo_installed")
      .then(setInstalled)
      .catch(() => setInstalled(false));
  }, []);

  async function runScan() {
    const nm = name.trim();
    if (!nm) {
      onToast(t("请先填公司名"));
      return;
    }
    setBusy(true);
    try {
      const r = await invoke<GeoScan>("geo_scan", {
        name: nm,
        region: region.trim() || null,
      });
      setResult(r);
      await invoke("geo_open_panel", { path: r.panel }).catch(() => {});
      onToast(t("体检面板已生成（{n} 个渠道），已在浏览器打开", { n: r.channels }));
    } catch (e) {
      onToast(typeof e === "string" ? e : t("体检失败，请重试"));
    } finally {
      setBusy(false);
    }
  }

  // 🔴 `runAicheck` / `runInspect` / `openPay` 2026-08-24 删除（用户拍板）。
  //
  // ① **不再在客户机上真跑**：aicheck 会调 6 个大模型、烧我们的内置额度，而且模型 id 会烂、
  //    网络会断 —— 失败面大、体验差。inspect 要抓客户任意网站，同属「乱做不如不做」。
  //    这两条命令连同 `llm.mjs` 已经不随客户端分发了（见 `geo.rs::REMOVED_FILES`），
  //    **真正的闸门在那儿，不在这个文件里** —— 摘按钮挡不住命令行。
  // ② **不再自助下单**：服务端没有「GEO 订单」这个概念，客户付了 ¥199 在系统里就是一笔
  //    普通 token 充值，靠他在支付宝备注写公司名人工对账 —— 不写就静默丢单，
  //    他以为下了单在等我们联系，我们这边零信号。所以成交动作统一收进微信。
  //
  // 现在这一页只做三件事：免费自查（真跑、离线、不花钱）· 看样板报告（静态、演示数据）·
  // 加微信谈。**能力和代码都在**（仓库 `src-tauri/skills/1so-geo/` 是完整的），
  // 我们收到客户微信后用它人工出报告。

  /** 打开样板报告 —— 静态文件、演示数据，不联网不调模型，永远不会翻车。 */
  async function openSample() {
    setBusySample(true);
    try {
      const path = await invoke<string>("geo_sample_report");
      await invoke("geo_open_panel", { path });
    } catch (e) {
      onToast(typeof e === "string" ? e : t("样板报告打开失败"));
    } finally {
      setBusySample(false);
    }
  }

  /** 把已填的公司信息拼成一句可直接粘给我们的话 —— 我们没有服务端收表单，
   *  所以「表单」的正确形态是让客户把内容带走，而不是假装我们收到了。 */
  function copyConsult() {
    const parts = [name.trim() && `公司：${name.trim()}`, region.trim() && `地区：${region.trim()}`, industry.trim() && `行业：${industry.trim()}`].filter(Boolean);
    const text = `你好，我想做 GEO（让 AI 认识我的公司）。${parts.join("，")}${parts.length ? "。" : ""}麻烦帮我出一份 AI 可见度报告。`;
    navigator.clipboard
      .writeText(text)
      .then(() => onToast(t("咨询内容已复制，加微信 hecare888 后直接粘贴发给我们")))
      .catch(() => onToast(t("请手动添加微信 hecare888，把公司名和网址发给我们")));
  }

  // 企业档不标价，改留微信（测试报告 #021）。复制失败也要把号码说出来 ——
  // 一个「复制失败」的 toast 等于把唯一的联系方式弄丢了。
  function copyWechat() {
    const id = "hecare888";
    navigator.clipboard
      .writeText(id)
      .then(() => onToast(t("微信号已复制：{id}，加我聊企业方案", { id })))
      .catch(() => onToast(t("请手动添加微信：{id}", { id })));
  }

  const disabledAll = installed === false;

  return (
    <div className="mx-auto flex max-w-[720px] flex-col gap-4 px-1 py-2">
      {/* 头部 */}
      <div className="flex items-start gap-3 rounded-card border border-white/[0.06] bg-bg-1 p-4">
        <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent/[0.12] text-accent">
          <Gauge size={20} />
        </div>
        <div className="min-w-0">
          <div className="text-[15px] font-semibold text-ink-0">{t("网站 GEO 体检")}</div>
          <div className="mt-0.5 text-[12px] leading-relaxed text-ink-3">
            {t("AI 时代，客户越来越多直接问豆包 / DeepSeek / ChatGPT「XX 这家公司靠谱吗」。这里两步看你在 AI 眼里的样子：先")}<span className="text-ink-1">{t("免费自查")}</span>{t("搜全网，再让")}<span className="text-ink-1">{t("各家大模型给你打分")}</span>{t("——出一份可直接发客户的《AI 可见度报告》。")}
          </div>
        </div>
      </div>

      {/* 研究背书条 */}
      <div className="flex items-center gap-2 rounded-lg border border-accent/15 bg-accent/[0.04] px-3 py-2 text-[11.5px] leading-relaxed text-ink-3">
        <span className="text-sm">📊</span>
        <span>{t("评分维度与优化建议基于 ")}<span className="font-medium text-ink-1">214,119 条中文 AI 引用</span>{t(" 与 ")}<span className="font-medium text-ink-1">23,745 条跨平台引用特征</span>{t(" 的公开实证研究（CN-GEO · 跨平台引用实验）。")}</span>
      </div>

      {/* 未装技能包提示 */}
      {installed === false && (
        <div className="flex items-start gap-2.5 rounded-lg border border-danger-500/25 bg-danger-500/[0.06] p-3 text-[12.5px] text-danger-400">
          <AlertTriangle size={16} className="mt-0.5 shrink-0" />
          <div>
            {t("没找到 GEO 体检技能包（")}<code className="text-ink-4">~/.uking/skills/1so-geo</code>{t("）。请双击更新到最新版 U-King，或在「AI 技能包」页安装后再来。")}
          </div>
        </div>
      )}

      {/* 输入表单（两步共用） */}
      <div className="flex flex-col gap-3 rounded-card border border-white/[0.06] bg-bg-1 p-4">
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium text-ink-2">{t("公司 / 品牌名 ")}<span className="text-danger-400">*</span></span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !busy && runScan()}
            placeholder={t("例：贺去病AI工作室")}
            className="rounded-lg border border-white/[0.08] bg-bg-2 px-3 py-2 text-[13.5px] text-ink-0 outline-none placeholder:text-ink-5 focus:border-accent/60"
          />
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-[12px] font-medium text-ink-2">{t("所在地区（可选）")}</span>
            <input
              value={region}
              onChange={(e) => setRegion(e.target.value)}
              placeholder={t("例：深圳宝安")}
              className="rounded-lg border border-white/[0.08] bg-bg-2 px-3 py-2 text-[13.5px] text-ink-0 outline-none placeholder:text-ink-5 focus:border-accent/60"
            />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[12px] font-medium text-ink-2">{t("所在行业（可选）")}</span>
            <input
              value={industry}
              onChange={(e) => setIndustry(e.target.value)}
              placeholder={t("例：AI培训")}
              className="rounded-lg border border-white/[0.08] bg-bg-2 px-3 py-2 text-[13.5px] text-ink-0 outline-none placeholder:text-ink-5 focus:border-accent/60"
            />
          </label>
        </div>
      </div>

      {/* ① 免费自查 */}
      <div className="flex flex-col gap-3 rounded-card border border-white/[0.06] bg-bg-1 p-4">
        <div className="flex items-center gap-2">
          <span className="grid h-6 w-6 place-items-center rounded-md bg-white/[0.06] text-[12px] font-bold text-ink-2">1</span>
          <div className="text-[13.5px] font-semibold text-ink-0">{t("免费自查 · 40+ 渠道体检面板")}</div>
          <span className="rounded-full bg-success-500/15 px-2 py-0.5 text-[10.5px] font-medium text-success-400">{t("免费 · 不需 Key")}</span>
        </div>
        <div className="text-[12px] leading-relaxed text-ink-3">
          {t("在 AI 搜索 / AI 对话 / 传统搜索 / 社交 / 视频 / 百科 / 地图 里搜你的公司，逐个点「去查↗」打开真实搜索页自己看——有没有你、AI 认不认你，实时算出「互联网可见度」。")}
        </div>
        <button
          onClick={runScan}
          disabled={busy || disabledAll}
          className="flex items-center justify-center gap-2 rounded-lg bg-white/[0.08] px-4 py-2.5 text-[13.5px] font-semibold text-ink-0 transition-colors hover:bg-white/[0.12] disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? (
            <><Loader2 size={16} className="animate-spin" /> {t("正在生成体检面板…")}</>
          ) : (
            <><Search size={16} /> {t("开始免费自查")}</>
          )}
        </button>
        {result && (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-success-500/25 bg-success-500/[0.05] px-3 py-2">
            <div className="flex items-center gap-2 text-[12.5px] text-success-400">
              <Compass size={15} /> {t("面板已生成 · 覆盖 {n} 个渠道", { n: result.channels })}
            </div>
            <button
              onClick={() => invoke("geo_open_panel", { path: result.panel }).catch(() => onToast(t("打开失败")))}
              className="flex shrink-0 items-center gap-1.5 rounded-md border border-white/[0.10] bg-bg-2 px-2.5 py-1 text-[12px] text-ink-1 transition-colors hover:border-accent/50 hover:text-ink-0"
            >
              <ExternalLink size={13} /> {t("重新打开")}
            </button>
          </div>
        )}
      </div>

      {/* ② 样板报告 —— 静态展示，不是对客户网站的检测 */}
      <div className="flex flex-col gap-3 rounded-card border border-accent/25 bg-accent/[0.05] p-4">
        <div className="flex items-center gap-2">
          <span className="grid h-6 w-6 place-items-center rounded-md bg-accent/20 text-[12px] font-bold text-accent">2</span>
          <div className="text-[13.5px] font-semibold text-ink-0">{t("看一份《AI 可见度报告》长什么样")}</div>
          <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[10.5px] font-medium text-amber-400">{t("演示样例")}</span>
        </div>
        {/* 🔴 诚实边界：这句必须说清「不是在检测你」。页面上任何让客户以为
            系统真的分析了他网站的暗示都是欺骗 —— 报告顶部还有一条更醒目的横幅。 */}
        <div className="text-[12px] leading-relaxed text-ink-3">
          {t("这是一份")}<span className="text-ink-1">{t("虚构公司的演示报告")}</span>
          {t("，用来说明我们出的报告包含什么：AI 可见度总分、6 家大模型分别怎么评价你、它们普遍缺哪几条关键信息、以及按影响力排序的改进清单。")}
          <span className="text-ink-4">{t("它不会检测你的网站，也不消耗任何额度。")}</span>
        </div>
        <button
          onClick={openSample}
          disabled={busySample || disabledAll}
          className="flex items-center justify-center gap-2 rounded-lg bg-accent px-4 py-2.5 text-[13.5px] font-semibold text-white transition-colors hover:bg-accent-600 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busySample ? (
            <><Loader2 size={16} className="animate-spin" /> {t("正在打开…")}</>
          ) : (
            <><FileCheck2 size={16} /> {t("打开样板报告")}</>
          )}
        </button>
      </div>

      {/* ③ 要一份自己的报告 —— 唯一的成交出口，走微信 */}
      <div className="flex flex-col gap-3 rounded-card border border-accent/30 bg-gradient-to-br from-accent/[0.10] to-accent/[0.03] p-4">
        <div className="flex items-center gap-2.5">
          <span className="grid h-9 w-9 shrink-0 place-items-center rounded-xl bg-accent/20 text-accent"><Rocket size={18} /></span>
          <div className="min-w-0">
            <div className="text-[14px] font-semibold text-ink-0">{t("想要一份针对你公司的真实报告？")}</div>
            <div className="text-[11.5px] leading-relaxed text-ink-3">{t("我们逐一实测 6 家大模型 + 人工判读后出具，再按结果谈怎么优化")}</div>
          </div>
        </div>
        {/* 为什么是人工而不是让客户自助跑：这是真话，不是话术 ——
            机器批量跑容易失真（模型有波动、会张冠李戴），报告要发给老板看的。 */}
        <div className="text-[12px] leading-relaxed text-ink-3">
          {t("AI 的回答有波动，同一个问题问两次结果可能不同，还常把同名公司搞混。所以我们不做一键批量跑，而是")}
          <span className="text-ink-1">{t("逐家实测 + 人工核对")}</span>
          {t("后出报告，附上能照着做的改进清单。")}
        </div>
        <div className="grid grid-cols-2 gap-2.5">
          <button
            onClick={copyConsult}
            className="flex flex-col items-start gap-0.5 rounded-lg border border-accent/40 bg-accent/[0.06] px-3 py-2.5 text-left transition-colors hover:border-accent/70"
          >
            <span className="text-[13px] font-semibold text-ink-0">{t("① 复制咨询内容")}</span>
            <span className="text-[11.5px] leading-snug text-ink-3">{t("把上面填的公司信息拼成一句话")}</span>
            <span className="text-[10.5px] leading-snug text-ink-4">{t("加微信后直接粘贴，省得重打一遍")}</span>
          </button>
          <button
            onClick={() => copyWechat()}
            className="flex flex-col items-start gap-0.5 rounded-lg border border-accent/40 bg-accent/[0.06] px-3 py-2.5 text-left transition-colors hover:border-accent/70"
          >
            <span className="text-[13px] font-semibold text-ink-0">{t("② 复制微信号")}</span>
            <span className="flex items-center gap-1 text-[15px] font-bold text-accent">
              <MessageCircle size={15} /> {t("hecare888")}
            </span>
            <span className="text-[10.5px] leading-snug text-ink-4">{t("点这里复制，去微信搜索添加")}</span>
          </button>
        </div>
        <div className="text-[10.5px] leading-relaxed text-ink-5">{t("报价按站点数和行业一对一给（我们帮你做：AI 可读企业主页 + llms.txt / 结构化数据部署 + 高德/百度/腾讯三大地图信息同步 + 每月复测追踪）。")}</div>
      </div>

      <p className="px-1 text-[11px] leading-relaxed text-ink-5">
        {t("提示：上面的「免费自查」打开的是各家 AI 和搜索引擎的真实页面，结果由你自己看 —— 搜不到 ≠ 不存在，只是还没被 AI 收录。样板报告为演示数据，不代表你公司的实际情况。")}
      </p>
    </div>
  );
}
