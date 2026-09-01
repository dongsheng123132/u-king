import { confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";

/**
 * 二次确认框。**危险操作一律用这个，绝不要用 `window.confirm`。**
 *
 * 为什么：Tauri 的 dialog 插件会往 webview 注入一段脚本，把 `window.confirm`
 * 换成返回 Promise 的版本（tauri-plugin-dialog/src/init-iife.js）：
 *
 *     window.confirm = async function(m){ return await invoke("plugin:dialog|confirm", {message: m}) }
 *
 * 而 TypeScript 用的还是 DOM 的定义（返回 `boolean`），所以这种写法能过类型检查：
 *
 *     if (!window.confirm("确定删除？")) return;   // ❌ Promise 恒为真值
 *
 * `!Promise` 永远是 `false`，那条 `return` 永远不执行 —— 卸载 / 还原 / 删除
 * **根本没等用户回答就直接干了**，弹窗只是事后飘出来。而且 `dialog:default`
 * 权限集里不含 `allow-confirm`（只有 message/save/open），漏配时还会多一个
 * 未处理的 Promise 拒绝（线上 issue #227 报的就是它）。
 *
 * 失败一律当「用户没同意」（fail closed）：弹不出确认框时，宁可什么都不做，
 * 也绝不默认执行一个删除/替换。
 */
export async function askConfirm(message: string, title = "U-King"): Promise<boolean> {
  try {
    return await dialogConfirm(message, { title });
  } catch (e) {
    console.error("[confirm] 确认框弹不出来，按「未确认」处理：", e);
    return false;
  }
}
