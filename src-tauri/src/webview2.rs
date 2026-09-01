//! WebView2 运行库自检 —— **没有它，U-King 是一具活着的空壳**。
//!
//! ## 为什么这段代码必须跑在 `tauri::Builder` 之前
//!
//! 2026-08-04 在一台干净的 Windows Server 2022 上实测：没装 WebView2 时，U-King
//! **不是崩溃，是静默假死** —— 进程起得来、`Responding=True`、任务栏有它，但
//! `Builder::setup` 过不去创建窗口那步，于是 setup 里排的一长串 `thread::spawn`
//! （防火墙放行 / 设备 Key 激活 / **定时任务调度线程** / codex 代理自愈 / metrics 快照）
//! **一个都没执行**。`crash.log` 是空的，日志里一个字都没有。
//!
//! 客户看到的是「双击了没反应」「定时任务到点不跑」，而我们这边查无实据。
//! 认它的硬指标：进程**线程数 5**（正常 10+）、6 分钟只烧 0.2s CPU。
//!
//! 所以自检**不能放进 setup** —— 那时候已经晚了，setup 根本轮不到执行。必须在
//! `run()` 里、`Builder::default()` 之前，用不依赖 WebView 的原生 MessageBox 说话。
//!
//! ## 边界
//! - Windows 11 / 新 Win10 大多自带，本模块对它们是**零成本**（一次目录 stat 就返回）。
//! - 非 Windows 平台恒为「已装」——WebView2 是 Windows 专属概念，Mac 用系统 WKWebView。
//! - **绝不静默替客户装东西**：先弹框说清楚是什么、为什么，点了「是」才下载。
//!   （宪法第 10 条：不碰用户真实状态；装运行库是改机器，得他点头。）
//!
//! 纯 std，零新依赖。

#![allow(dead_code)]

/// 微软官方 evergreen bootstrapper 短链（长期有效，不随版本变）。
pub const BOOTSTRAPPER_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
/// 装不上时让客户自己去的页面。
pub const DOWNLOAD_PAGE: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

// ───────────────────────── 探测 ─────────────────────────

/// WebView2 运行库装没装。
///
/// 先查安装目录（纯文件系统，零子进程，启动路径上不能拖时间），再用 `reg query` 兜底
/// （只在文件系统没查到时才付这个 ~50ms 的代价）。
#[cfg(windows)]
pub fn installed() -> bool {
    if app_dir_has_version() {
        return true;
    }
    reg_has_version()
}

#[cfg(not(windows))]
pub fn installed() -> bool {
    true // 非 Windows 没有 WebView2 这个概念
}

/// `…\Microsoft\EdgeWebView\Application\<版本号>\` 存在即已装。
/// 三处都查：系统级 x86（绝大多数）、系统级 x64、用户级（per-user 安装）。
#[cfg(windows)]
fn app_dir_has_version() -> bool {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(var) {
            if !base.is_empty() {
                roots.push(std::path::PathBuf::from(base).join("Microsoft\\EdgeWebView\\Application"));
            }
        }
    }
    roots.iter().any(|dir| {
        std::fs::read_dir(dir)
            .map(|it| {
                it.filter_map(|e| e.ok())
                    // 版本目录形如 151.0.4129.59 —— 要求首字符是数字，
                    // 免得把同目录下的 Installer 之类当成「已装」
                    .any(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_digit())
                            && e.path().is_dir()
                    })
            })
            .unwrap_or(false)
    })
}

/// 注册表兜底。`pv` 非空且不是全 0 才算装了（卸载残留会留下 `pv=0.0.0.0`）。
#[cfg(windows)]
fn reg_has_version() -> bool {
    const CLIENT: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let keys = [
        format!("HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{CLIENT}"),
        format!("HKLM\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{CLIENT}"),
        format!("HKCU\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{CLIENT}"),
    ];
    keys.iter().any(|k| {
        let out = no_window(std::process::Command::new("reg").args(["query", k, "/v", "pv"]))
            .output()
            .ok();
        match out {
            Some(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout);
                s.lines()
                    .find(|l| l.trim_start().starts_with("pv"))
                    .and_then(|l| l.split_whitespace().last())
                    .is_some_and(|v| !v.is_empty() && v.chars().any(|c| c.is_ascii_digit() && c != '0'))
            }
            _ => false,
        }
    })
}

/// 子进程不弹黑窗（对齐项目里所有 Command 的用法）。
#[cfg(windows)]
fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
}

// ───────────────────────── 交互 + 安装 ─────────────────────────

/// 自检结果。给 `lib.rs` 决定是继续启动还是退出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 已装（或非 Windows）——照常启动
    Ready,
    /// 刚装好 —— 照常启动
    Installed,
    /// 没装且客户不让装 —— 已经给他开了下载页，本进程该退出
    Declined,
    /// 想装但装失败 —— 已经告知并开了下载页，本进程该退出
    Failed,
}

/// 启动前自检。**已装时零开销**（一次目录 stat 就返回 `Ready`）。
///
/// 没装时弹原生 MessageBox（不经过 WebView —— 那正是坏掉的东西）问客户是否自动安装。
pub fn ensure() -> Outcome {
    if installed() {
        return Outcome::Ready;
    }
    #[cfg(windows)]
    {
        let yes = ask_yes_no(
            "U-King 需要「Microsoft Edge WebView2 运行库」才能显示界面。\n\
             这台电脑上还没有它。\n\n\
             缺了它，U-King 会启动成一个看不见的空壳：进程在、任务栏也有，\n\
             但界面出不来，定时任务之类的后台功能也不会运行。\n\n\
             现在自动安装吗？（微软官方组件，约 1 分钟）",
            "U-King · 缺少运行库",
        );
        if !yes {
            open_url(DOWNLOAD_PAGE);
            return Outcome::Declined;
        }
        if install_runtime() && installed() {
            return Outcome::Installed;
        }
        alert(
            "WebView2 自动安装没成功。\n\n\
             已经帮你打开微软官方下载页，装好后重新打开 U-King 就行。",
            "U-King · 安装未完成",
        );
        open_url(DOWNLOAD_PAGE);
        Outcome::Failed
    }
    #[cfg(not(windows))]
    Outcome::Ready
}

/// 下载 evergreen bootstrapper 并静默安装。用系统 curl.exe（Win10+ 内置，
/// 与项目里其它 HTTP 调用同一条路），不引 HTTP crate。
#[cfg(windows)]
fn install_runtime() -> bool {
    let tmp = std::env::temp_dir().join("uking-webview2-setup.exe");
    let _ = std::fs::remove_file(&tmp);
    let ok = no_window(std::process::Command::new("curl.exe").args([
        "-L",
        "--connect-timeout",
        "20",
        "--max-time",
        "300",
        "-o",
        &tmp.to_string_lossy(),
        BOOTSTRAPPER_URL,
    ]))
    .status()
    .map(|s| s.success())
    .unwrap_or(false);
    if !ok || !tmp.exists() {
        return false;
    }
    let installed_ok = no_window(std::process::Command::new(&tmp).args(["/silent", "/install"]))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&tmp);
    installed_ok
}

// ───────────────────────── 原生对话框（不依赖 WebView）─────────────────────────

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// hwnd 用 `*mut c_void` 而不是 isize —— **必须和 guard.rs 里那份声明逐字一致**。
// 同一个 extern 符号在一个 crate 里声明两次且签名不同，rustc 会报
// `clashing_extern_declarations`；真正危险的是它只是个 warning，编得过、跑得可能也对，
// 直到某天 ABI 对不上才炸，而那时候没人会想到是两处声明打架。
#[cfg(windows)]
extern "system" {
    fn MessageBoxW(
        hwnd: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        utype: u32,
    ) -> i32;
    fn ShellExecuteW(
        hwnd: *mut core::ffi::c_void,
        op: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> isize;
}

#[cfg(windows)]
fn ask_yes_no(text: &str, caption: &str) -> bool {
    const MB_YESNO: u32 = 0x0000_0004;
    const MB_ICONWARNING: u32 = 0x0000_0030;
    const MB_SETFOREGROUND: u32 = 0x0001_0000;
    const IDYES: i32 = 6;
    let t = wide(text);
    let c = wide(caption);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND,
        ) == IDYES
    }
}

#[cfg(windows)]
fn alert(text: &str, caption: &str) {
    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONERROR: u32 = 0x0000_0010;
    const MB_SETFOREGROUND: u32 = 0x0001_0000;
    let t = wide(text);
    let c = wide(caption);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(windows)]
fn open_url(url: &str) {
    let op = wide("open");
    let file = wide(url);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 探测不能 panic、不能挂 —— 它跑在启动路径最前面，挂了就是「双击没反应」本身。
    #[test]
    fn probe_is_safe_and_fast() {
        let t = std::time::Instant::now();
        let _ = installed();
        assert!(t.elapsed().as_secs() < 5, "WebView2 探测太慢，会拖慢启动");
    }

    /// 开发机（本机）必然装了 WebView2，否则这个项目自己都跑不起来。
    /// 这条断言的价值：**探测逻辑写错时会当场红**（比如目录拼错、条件写反），
    /// 而不是等到干净机上才发现「所有人都被当成没装、全都弹框」。
    #[cfg(windows)]
    #[test]
    fn dev_machine_is_detected_as_installed() {
        assert!(installed(), "本机明明能跑 GUI 却被判成没装 WebView2 —— 探测逻辑反了");
    }
}
