/**
 * 健壮复制到剪贴板 —— 零依赖，不引 Tauri clipboard 插件（守体积红线）。
 *
 * 为什么不能只用 navigator.clipboard：Tauri WebView（WebView2）里 navigator.clipboard 偶发
 * 不可用 / 无权限 / 非 focus 时静默失败，客户就会「点了复制没反应」。这里主用它，失败自动
 * 回退到隐藏 textarea + execCommand("copy") —— 老办法但在 WebView2 里稳、不挑安全上下文，
 * 保证「点了就复制成功」。全局 css 已给 textarea 放开 user-select，回退才能真选中。
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* 落到下面的兜底 */
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    ta.style.top = "0";
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}
