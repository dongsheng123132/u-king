/**
 * 「这台机器是什么系统」—— **只认后端 `get_env` 给的那个字段**，前端不自己算。
 *
 * 🔴 为什么单开这一份：2026-08-16 修过一整轮「平台分支只做到后端、界面层没跟上」的病 ——
 * 后端老老实实 `#[cfg(windows)]` 跳过了，界面照样把 Windows 专属的东西摆给 Mac 用户看
 * （「正在补装 PowerShell 7」、点了必然失败的按钮、查都没查就显示的绿勾）。
 * 根因是各处各判各的：有的翻 `env.platform`、有的靠 props 传、有的干脆没判。
 *
 * 用法（同步、可在 useMemo 里直接用）：
 *   import { isWindows } from "../../lib/platform";
 *   ...(isWindows() ? [{ label: "用其他程序打开…", run: ... }] : [])
 *
 * 拿不到就返回 false（**保守**）：宁可少一个按钮，也不要给 Mac 用户一个点了必报错的。
 * 首屏 `get_env` 是毫秒级的，而这些菜单都要用户右键才出得来，等得及。
 */
import { invoke } from "@tauri-apps/api/core";

let platform: string | null = null;

void invoke<{ platform?: string }>("get_env")
  .then((e) => {
    platform = e?.platform ?? null;
  })
  .catch(() => {
    /* 拿不到就一直是 null → isWindows() 恒 false，见上面的保守取向 */
  });

export function isWindows(): boolean {
  return platform === "windows";
}

export function isMac(): boolean {
  return platform === "macos";
}
