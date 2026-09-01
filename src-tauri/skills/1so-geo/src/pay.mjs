// 一搜商答 / 1so —— 收费方案（GEO + MEO 人工优化服务）· 单一真相源。
// aicheck / inspect 两份报告都从这里取 PRICING + PAY + payHrefFor + 定价区块 HTML/CSS，
// 避免各写一份漂移（开发宪法 rule 8：同一事实只认单一真相源）。改价改文案只动这一处。
import { esc } from "./util.mjs";

// 收费方案（三档，随时改）。卖的是「我们帮你做 GEO + MEO 优化」的人工交付服务：
//   GEO = 让 AI（豆包/DeepSeek/ChatGPT…）能收录、认识、引用你；
//   MEO = 让高德/百度/腾讯地图能搜到、正确标注你的商家信息。
// price 是票面文案；amount 是下单金额（数字，用于拼 ?amount=）。
export const PRICING = [
  { name: "免费测试", price: "¥0", amount: 0, badge: "你正在看的", features: ["AI 可见度总分", "各大模型认识度评分", "网页 AI 友好度诊断", "一句话诊断"] },
  { name: "基础优化", price: "¥199", amount: 199, features: ["完整多模型 AI 可见度诊断报告", "我们帮你建 1 个 AI 可读企业主页（GEO）", "同步高德 / 百度 / 腾讯地图商家信息（MEO）", "补内容清单 + 40+ 渠道体检面板 + llms.txt / 结构化数据"] },
  { name: "持续优化", price: "¥1999 / 年", amount: 1999, features: ["企业主页 + 行业高频问答持续更新", "每月复测追踪 AI / 地图可见度分数", "GEO + MEO 优化建议持续迭代落地", "被各家 AI 引用 + 地图收录监测"] },
];

// 支付宝收款配置。默认**复用虾盘云支付宝收款页**（epay/支付宝官方，客户面 u-claw.org.cn/recharge），
// 页面支持 `?amount=` 预填金额，钱到你支付宝、客户同时得到等额 token 余额。
//   🔴 单笔上限 cap（服务端写死）——超 cap 的档位不能直付，走 overCapUrl。
//   overCapUrl 留空=回退充值页首页；想让超额档走别的入口（联系页/分期页），改这里。
// 若你有独立支付宝收款码/收款链接（不走充值、不限额、不给 token），填 alipayUrl 或 alipayQr，会**覆盖**上面充值页。
export const PAY = {
  rechargeBase: "https://u-claw.org.cn/recharge",
  cap: 500,
  overCapUrl: "",
  alipayUrl: "",
  alipayQr: "",
  note: "加微信 hecare888，把公司名和网址发我们，先免费出一份 AI 可见度诊断，再谈怎么做。",
  // 超 cap 档位（¥1999/年）的去处：加微信一对一。
  //
  // 🔴 2026-08-23 修：原来这一档照样渲染「立即开通（支付宝）」，而 `payHrefFor(1999)` 因为
  // 超 cap 且 `overCapUrl` 为空，回退到**充值页首页** —— 客户点「立即开通」，落到一个不知道
  // 该填多少、填了也不知道算不算数的输入框。**卖得最贵的那一档，是唯一一条断的路。**
  //
  // 而 GUI 那边（`src/Geo.tsx`）0.9.83 就想明白了：企业档按客户体量和站点数谈，不平铺标价，
  // 改留微信（测试报告 #021）。两份定价各活各的，于是同一个决定只落实了一半 ——
  // 报告 HTML 是客户真正会转发给老板看的那份，偏偏是没改的那份（宪法第 8 条）。
  contactWechat: "hecare888",
};

/**
 * 🔴 **总开关：现在所有档位一律走微信，不留任何自助下单入口**（2026-08-24 用户拍板）。
 *
 * 为什么连能付得起的 ¥199 也收掉：**服务端没有「GEO 订单」这个概念**。
 * 下单复用的是虾盘云充值页 —— 客户付了 ¥199，在系统里就是一笔普通 token 充值，
 * 全靠他在支付宝备注里写「公司名 + 档位」，我们再人工对账。**他不写备注，这单就静默丢了**：
 * 他以为下了单在等我们联系，我们这边零信号，钱变成了他账上的 token 余额。
 * 在服务端补上订单概念之前，「能自助付款」不是一个功能，是一个会闷声掉单的陷阱。
 *
 * 恢复自助下单的条件（三条都满足才把这里改回去）：
 *   ① 服务端能把一笔付款识别成 GEO 订单（而不是 token 充值）；
 *   ② 付款成功后我们这边有信号（能主动联系客户，不依赖他写备注）；
 *   ③ 超上限档位的收款路径存在（现在被单笔上限挡住）。
 */
const DIRECT_PAY_ENABLED = false;

/** 这一档能不能直付？总开关关着就一律否 —— 别渲染一个点了会掉单的按钮。 */
export function directPayable(amount) {
  if (!DIRECT_PAY_ENABLED) return false;
  if (PAY.alipayUrl || PAY.alipayQr) return true;
  if (amount <= 0) return false;
  return amount <= PAY.cap || !!PAY.overCapUrl;
}

// 某档位的「立即开通」去哪：独立收款链接/码优先；否则走虾盘云充值页（≤cap 预填金额，超 cap 走 overCapUrl）。
export function payHrefFor(amount) {
  if (PAY.alipayUrl) return PAY.alipayUrl;
  if (PAY.alipayQr) return "#geo-pay";
  if (PAY.rechargeBase) {
    return amount > 0 && amount <= PAY.cap
      ? `${PAY.rechargeBase}?amount=${amount}`
      : (PAY.overCapUrl || PAY.rechargeBase);
  }
  return "#";
}

// 定价区块相关的 CSS（两份报告共用，注入进各自 <style>。依赖 --accent/--green/--fg 变量，两报告 :root 都定义了同款调色板）。
export function payCss() {
  return `
  .pay{margin-top:34px;background:linear-gradient(135deg,#4f46e5,#7c3aed);border-radius:18px;padding:24px;color:#fff}
  .pay h2{color:#fff;border:0;padding:0;margin:0 0 4px;font-size:20px} .pay .psub{opacity:.9;font-size:14px;margin:0 0 16px}
  .prices{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:12px}
  .pc{background:#fff;color:var(--fg);border-radius:14px;padding:16px;position:relative}
  .pc.free{outline:2px solid #c7d2fe} .pb{position:absolute;top:-10px;right:12px;background:#16a34a;color:#fff;font-size:11px;padding:2px 8px;border-radius:999px}
  .pc h4{margin:0 0 2px;font-size:16px} .pp{font-size:22px;font-weight:800;color:var(--accent);margin-bottom:8px}
  .pc ul{list-style:none;padding:0;margin:0 0 12px;font-size:13px;color:#444} .pc li{padding:3px 0 3px 18px;position:relative}
  .pc li:before{content:"✓";position:absolute;left:0;color:var(--green)}
  .pcta{display:block;text-align:center;background:var(--accent);color:#fff;text-decoration:none;padding:9px;border-radius:9px;font-weight:600}
  .pcta.done{background:#e5e7eb;color:#16a34a}
  .pcta.alipay{background:#1677ff}
  .pcta.wx{background:#07c160;color:#fff;cursor:default}
  .pcontact{margin:14px 0 0;text-align:center;color:#fff;opacity:.95;font-size:14px}
  .paybox{margin:16px auto 0;max-width:440px;background:#fff;color:var(--fg);border-radius:14px;padding:16px;display:flex;gap:14px;align-items:center}
  .paybox .qr{width:122px;height:122px;flex:0 0 auto;border-radius:8px;object-fit:contain;background:#f6f7fb}
  .paybox .paytxt b{font-size:15px} .paybox .paytxt p{margin:6px 0 0;font-size:13px;color:#444;line-height:1.5}`;
}

// 定价区块 HTML（渐变卡 + 三档定价卡 + 收款说明）。intro 为上方引导语，两份报告各传各的语境。
export function renderPayBlock(intro) {
  const priceCards = PRICING.map((p, i) => {
    const href = payHrefFor(p.amount);
    const anchor = href.startsWith("#"); // 页内收款码锚点不开新标签
    return `<div class="pc ${i === 0 ? "free" : ""}">
      ${p.badge ? `<span class="pb">${esc(p.badge)}</span>` : ""}
      <h4>${esc(p.name)}</h4><div class="pp">${esc(p.price)}</div>
      <ul>${p.features.map((f) => `<li>${esc(f)}</li>`).join("")}</ul>
      ${i === 0
        ? '<div class="pcta done">✓ 已完成</div>'
        : directPayable(p.amount)
          ? `<a class="pcta alipay" href="${esc(href)}"${anchor ? "" : ' target="_blank" rel="noopener"'}>立即开通（支付宝）</a>`
          // 超 cap 档位：明说走微信一对一，不给一个点了会落空的「立即开通」。
          // 报价一对一谈，和 GUI 页（Geo.tsx 的企业档）保持同一个说法。
          : `<div class="pcta wx">加微信 ${esc(PAY.contactWechat)} 一对一</div>`}
    </div>`;
  }).join("");
  // 收款码面板：填了 alipayQr 才显示二维码；否则只显示付款说明（备注公司名）。
  const payPanel = PAY.alipayQr
    ? `<div id="geo-pay" class="paybox">
        <img class="qr" src="${esc(PAY.alipayQr)}" alt="支付宝收款码">
        <div class="paytxt"><b>支付宝扫码开通</b><p>${esc(PAY.note)}</p></div>
      </div>`
    : (PAY.note ? `<p class="pcontact">${esc(PAY.note)}</p>` : "");
  return `<div class="pay">
    <h2>让 AI 和地图都搜到你 —— 我们帮你做 GEO + MEO 优化</h2>
    <p class="psub">${esc(intro || "付费后由我们上手：把你变成 AI 读得懂的企业主页（GEO）、同步高德/百度/腾讯三大地图商家信息（MEO），并持续复测追踪分数：")}</p>
    <div class="prices">${priceCards}</div>
    ${payPanel}
  </div>`;
}
