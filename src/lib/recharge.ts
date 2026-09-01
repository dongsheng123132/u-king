import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as shellOpen } from "@tauri-apps/plugin-shell";

/** 通用充值页（不带 key）—— 设备 Key 还没拿到时也能让用户先打开充值。 */
const FALLBACK_RECHARGE_URL = "https://u-claw.org.cn/recharge";

/**
 * 打开充值页 —— 虾盘云的核心便捷点，必须「永远可点、永远能开」。
 *
 * 🔴 2026-06-23 pc-*** 实证：旧实现优先 `@tauri-apps/plugin-shell` 的 open，但它在
 * 生产构建里「完全没反应（连浏览器闪都没有）」—— tauri 2 已把「打开 URL」迁到
 * plugin-opener，plugin-shell 的 open 不再可靠。且旧实现把 webview 兜底的错误
 * `.catch(()=>{})` 吞掉，两条路都失败时彻底无反馈。
 *
 * 现策略：三重兜底，任一成功即止，每步留 console 痕（不再静默吞）：
 *  ① plugin-opener openUrl —— tauri 2 打开 URL 的正道，最稳（opener:default 已授权）。
 *  ② plugin-shell open —— 旧路径，兼容老环境。
 *  ③ 后端 open_recharge —— Rust 侧再调一次系统浏览器(opener)，仍唤不起才开 webview 子窗口兜底
 *     （⚠️ 支付宝收银台在内嵌 webview 里付不掉，故 webview 已降为最后退路，仅保人工充值入口可见）。
 */
export async function openRecharge(url?: string | null): Promise<void> {
  const target = url && url.trim() ? url : FALLBACK_RECHARGE_URL;
  try {
    await openUrl(target);
    return;
  } catch (e) {
    console.warn("[recharge] opener 打开失败，改试 shell", e);
  }
  try {
    await shellOpen(target);
    return;
  } catch (e) {
    console.warn("[recharge] shell 打开失败，改试 webview 兜底", e);
  }
  try {
    await invoke("open_recharge", { url: target });
    return;
  } catch (e) {
    console.error("[recharge] 三条路都失败，充值页打不开：", target, e);
  }
}
