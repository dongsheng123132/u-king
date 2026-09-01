//! ClawX 桌面版进程生命周期（关 → 等退出 → 重启）。仅 Windows 有实义。
//!
//! 为什么需要：ClawX 运行时在内存里持有一份配置，**它没退出就改磁盘配置、等它退出又会用
//! 旧内存覆写回去** —— 这是「U-King 配了 ClawX 没反应」的根因。所以托管式配置必须
//! 先关 ClawX、确认退出、再写配置、最后重启（让新 ClawX 读到我们写的配置）。
//!
//! 模块边界（遵守 CLAUDE.md 的独立性铁律）：本模块只暴露纯函数、不碰 `AppHandle`；
//! 进度由 `lib.rs` 的命令负责 emit。配置「写哪些文件」在 `providers.rs`，不在这里。
//! 重启复用 `tools::launch_app`（依赖方向：新模块 → 老的公共助手，允许）。

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// ClawX 是否在运行。
pub fn is_running() -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new(crate::installer::system_tool("tasklist"))
            .args(["/FI", "IMAGENAME eq ClawX.exe", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("ClawX.exe"))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("pgrep")
            .args(["-x", "ClawX"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false)
    }
}

/// 优雅关闭：taskkill **不带 /F**（等同点窗口关闭，给 ClawX 落盘 leveldb/state.db 的机会）。
pub fn graceful_close() {
    // 托管式配置是「关进程→写配置→重启」的时序活，任一步歪掉客户看到的都只是
    // 「配置没生效」。把每一步都记下来，远程才分得清是没关掉、还是写了又被覆盖、还是没拉起来。
    crate::ulog::write("clawx", "请求优雅关闭 ClawX");
    #[cfg(windows)]
    {
        let _ = std::process::Command::new(crate::installer::system_tool("taskkill"))
            .args(["/IM", "ClawX.exe", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-x", "ClawX"])
            .output();
    }
}

/// 强杀兜底（优雅关超时后用）。
pub fn force_kill() {
    // 走到这一步说明优雅关超时了 —— 这条本身就是有价值的信号（ClawX 卡死 / 有未保存对话弹窗）。
    crate::ulog::write("clawx", "优雅关超时，强制结束 ClawX");
    #[cfg(windows)]
    {
        let _ = std::process::Command::new(crate::installer::system_tool("taskkill"))
            .args(["/F", "/IM", "ClawX.exe", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-9", "-x", "ClawX"])
            .output();
    }
}

/// 轮询等 ClawX 完全退出，最多 `timeout_ms`。返回是否已退出。
pub fn wait_exited(timeout_ms: u64) -> bool {
    let step = 250u64;
    let mut waited = 0u64;
    while waited < timeout_ms {
        if !is_running() {
            crate::ulog::write("clawx", &format!("ClawX 已退出（等待 {waited}ms）"));
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(step));
        waited += step;
    }
    let exited = !is_running();
    crate::ulog::write(
        "clawx",
        &format!("等待退出超时 {timeout_ms}ms，最终{}退出", if exited { "已" } else { "未" }),
    );
    exited
}

/// 重启 ClawX（复用 tools 的启动逻辑：找到 exe 拉起）。
pub fn relaunch() -> Result<(), String> {
    let r = crate::tools::launch_app("clawx");
    match &r {
        Ok(_) => crate::ulog::write("clawx", "已重新拉起 ClawX"),
        // 没拉起来最坑：配置其实写对了，但客户看到 ClawX 没了，只会说「把我软件搞没了」。
        Err(e) => crate::ulog::write("clawx", &format!("重启 ClawX 失败：{e}")),
    }
    r
}
