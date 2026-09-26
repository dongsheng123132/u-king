/**
 * 启动终端偏好 —— 「命令行工具落地要不要塞进 U-CLI」。
 *
 * 背景：CLI 工具（`LaunchMode::EmbeddedPty`）原来一律 `open_terminal_window`，
 * 拉出的其实是 U-King 自己那套 xterm（"U-CLI"），不是系统原生控制台。
 * 有客户就想要一个干净的系统终端窗口（哪怕丢掉 U-CLI 的高亮/快捷键），
 * 后端 `term::term_open_external` 早就实现了这条路（`LaunchMode::ExternalTerm`
 * 那类工具已经在用），这里只是把选择权交给用户，别处不需要跟着改。
 *
 * 纯前端偏好，落 localStorage，读写都在这一个文件里 —— 全站任何地方要用
 * 「用户想用哪种终端启动」都从这里取，不要各自读 localStorage 的 key 字面量
 * （宪法第 8 条：同一份事实只认一个真相源）。
 */

const KEY = "uking.launchIn";

export type LaunchPref = "system" | "ucli";

/** 未设置过 → 默认 "system"（系统终端，跟客户机上其它软件的观感一致）。 */
export function getLaunchPref(): LaunchPref {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "system" || v === "ucli") return v;
  } catch {
    /* localStorage 不可用（隐私模式等）时静默回退默认值 */
  }
  return "system";
}

export function setLaunchPref(v: LaunchPref): void {
  try {
    localStorage.setItem(KEY, v);
  } catch {
    /* ignore：写不进去就只影响这一次会话的偏好，不阻断功能 */
  }
}
