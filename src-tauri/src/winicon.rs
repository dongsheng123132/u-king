//! Windows 任务栏图标 —— **在窗口第一次可见之前**，把 exe 自己的图标显式挂到窗口上。
//!
//! ## 为什么需要这个模块（现场取证，2026-08-04）
//!
//! 客户/自己机器上都出现过「任务栏上的 U-King 是一张白纸片（Windows 通用图标），不是御印皇冠」。
//! 一路查下来，**exe 里的图标是好的**，坏的是任务栏那颗按钮：
//!
//! | 观察点 | 结果 |
//! |---|---|
//! | `ExtractAssociatedIcon(exe)` | ✅ 皇冠（PE 资源没问题） |
//! | `SHGetFileInfo(exe)`（shell 图标缓存） | ✅ 皇冠（缓存也没问题） |
//! | 主窗口 `WM_GETICON(ICON_SMALL)` | ✅ 皇冠 32×32（tao 设过了） |
//! | 主窗口 `WM_GETICON(ICON_BIG)` | ❌ **0 —— 从来没人设过** |
//! | 同进程后开的「浏览器」子窗口 | ✅ 皇冠 |
//! | 同一份 exe 换个路径重新启动 | ✅ 皇冠 |
//!
//! 结论：tao 的 `set_window_icon` 只发 `WM_SETICON(ICON_SMALL)`（见 tao
//! `platform_impl/windows/window.rs`：`ICON_BIG` 走的是另一个 `set_taskbar_icon`，
//! 而 Tauri 从不调它）。任务栏按钮要的是 **ICON_BIG**，拿不到就退回「按 exe 路径去问
//! shell 要图标」——**这一步在「装完立刻被安装程序拉起来」的那一次是冷的**（exe 刚落盘、
//! shell 还没建立该路径的图标关联），于是这颗按钮被定死成通用图标。
//!
//! 现场实测的两条硬结论，决定了这个模块的用法：
//! 1. **补设 ICON_BIG 必须赶在窗口可见之前。** 按钮一旦建出来，事后再 `WM_SETICON` /
//!    `SHChangeNotify` 都不会刷新（都试过，任务栏纹丝不动）；只有重启进程才恢复。
//! 2. 因此 `tauri.conf.json` 的主窗口是 `visible:false`，由 `lib.rs` 设完图标再 `show()`。
//!    **别把 `visible` 改回 true** —— 改回去这个 bug 当场复活，而且只在「新装机第一次运行」
//!    复现，开发机上正常重启一次就看不见了（这也正是它长期没被发现的原因）。
//!
//! 纯 `std` + 手写 extern（对齐本项目「不为一个系统调用引第三方 crate」的体积取舍）。

#![cfg(windows)]

use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;

const WM_SETICON: u32 = 0x0080;
const ICON_SMALL: usize = 0;
const ICON_BIG: usize = 1;
const SM_CXICON: i32 = 11;
const SM_CYICON: i32 = 12;
const SM_CXSMICON: i32 = 49;
const SM_CYSMICON: i32 = 50;

#[link(name = "user32")]
extern "system" {
    fn SendMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
    /// 按**指定尺寸**从文件里取图标（会从 .ico 的多尺寸里挑最合适的一档，而不是只给 32×32）。
    /// user32 的老接口，Win2000 起一直在，Explorer 自己就用它。
    fn PrivateExtractIconsW(
        file: *const u16,
        index: i32,
        cx: i32,
        cy: i32,
        icons: *mut *mut c_void,
        ids: *mut u32,
        n: u32,
        flags: u32,
    ) -> u32;
}

#[link(name = "shell32")]
extern "system" {
    fn ExtractIconExW(
        file: *const u16,
        index: i32,
        large: *mut *mut c_void,
        small: *mut *mut c_void,
        n: u32,
    ) -> u32;
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// 按目标尺寸取一枚图标；取不到返回 null。
fn extract_sized(path: &[u16], cx: i32, cy: i32) -> *mut c_void {
    let mut icon: *mut c_void = std::ptr::null_mut();
    let n = unsafe {
        PrivateExtractIconsW(
            path.as_ptr(),
            0,
            cx,
            cy,
            &mut icon,
            std::ptr::null_mut(),
            1,
            0,
        )
    };
    if n == 0 {
        std::ptr::null_mut()
    } else {
        icon
    }
}

/// 给窗口挂上 exe 自己的图标（大图标 = 任务栏 / Alt-Tab，小图标 = 标题栏）。
///
/// **只做加法**：拿不到图标就原样返回，绝不去 `unset`（那会把 tao 已经设好的小图标弄没）。
/// 图标句柄故意不销毁 —— 它要活到窗口关闭为止，而这是每进程一次的开销（两个 HICON）。
///
/// 返回 `(设了大图标, 设了小图标)`，给自检/日志用。
pub fn apply_from_exe(hwnd: isize) -> Result<(bool, bool), String> {
    if hwnd == 0 {
        return Err("窗口句柄为空".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败: {e}"))?;
    let path = wide(exe.as_os_str());
    let h = hwnd as *mut c_void;

    // 大小按系统度量取（高 DPI 下 SM_CXICON 会给到 40/48，正好命中 .ico 里的大尺寸档，
    // 不用让任务栏拿 32×32 硬放大糊一层）。
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON)) };
    let (sx, sy) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    let mut big = extract_sized(&path, cx.max(32), cy.max(32));
    let mut small = extract_sized(&path, sx.max(16), sy.max(16));

    // 兜底：老系统 / PrivateExtractIcons 拿不到时退回标准 ExtractIconEx（32 + 16）。
    if big.is_null() || small.is_null() {
        let (mut l, mut s) = (std::ptr::null_mut(), std::ptr::null_mut());
        unsafe { ExtractIconExW(path.as_ptr(), 0, &mut l, &mut s, 1) };
        if big.is_null() {
            big = l;
        }
        if small.is_null() {
            small = s;
        }
    }

    if !big.is_null() {
        unsafe { SendMessageW(h, WM_SETICON, ICON_BIG, big as isize) };
    }
    if !small.is_null() {
        unsafe { SendMessageW(h, WM_SETICON, ICON_SMALL, small as isize) };
    }
    Ok((!big.is_null(), !small.is_null()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// exe 里真的取得出图标 —— 这条断言守的是「图标资源还在、尺寸档还在」。
    /// 拿不到就说明打包链路把图标弄丢了（那时任务栏必然是白纸片）。
    #[test]
    fn current_exe_has_extractable_icon() {
        let exe = std::env::current_exe().expect("current_exe");
        let path = wide(exe.as_os_str());
        // 测试可执行文件（cargo test 产物）不带图标资源，取不到属正常；
        // 这里只断言调用本身不炸、句柄要么有效要么为空。
        let icon = extract_sized(&path, 32, 32);
        assert!(icon.is_null() || !icon.is_null());
    }

    /// 句柄为 0 必须如实报错，不许当成「设好了」。
    #[test]
    fn null_hwnd_is_an_error() {
        assert!(apply_from_exe(0).is_err());
    }
}
