//! Codex 专区后端 —— **只做稳的检测，不追上游内部结构**。
//!
//! 安装复用 `installer.rs`，驱动切换复用 `providers.rs::apply_provider`，这里只补一块
//! 轻量状态：Codex 桌面版装了没 + U-King 驱动接管了没。
//!
//! 设计取舍（2026-06-20，按「让代码抗变化」原则瘦身）：早期这里做过 computer use 体检，
//! 深挖 Codex 的私有运行时路径（`runtimes/cua_node/<hash>/bin/node_modules/@oai/sky/…`）和
//! 扩展宿主路径来判断「能不能用」。这些是 OpenAI 的**内部结构**，每次升级都可能改 → 代码追着
//! 上游跑，维护成本高、还经常误判。现已删除：computer use 怎么用改成前端**图文教程兜底**
//! （装好 → 接虾盘云驱动 → 在 Codex 里 @电脑 发任务，首次自动下运行时），不再靠探测下结论。

use serde::Serialize;
use std::path::PathBuf;

/// Codex 专区状态（只保留稳的两项 + 版本号）。
#[derive(Debug, Clone, Serialize)]
pub struct CodexStatus {
    /// Codex 桌面版是否已装（复用 installer 的多路探测，稳）
    pub app_installed: bool,
    /// Codex App 版本（取到才有；取不到不影响主流程）
    pub app_version: Option<String>,
    /// 当前 Codex 驱动是否被 U-King 接管（config.toml 含我们的 managed 标记，读自己写的，稳）
    pub driver_managed: bool,
}

fn home_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
}

/// 读 Codex App 版本（PowerShell 查 Appx 包，失败返回 None）。
#[cfg(windows)]
fn codex_app_version() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new(crate::installer::system_tool("powershell"))
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-AppxPackage -Name OpenAI.Codex).Version",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

#[cfg(target_os = "macos")]
fn codex_app_version() -> Option<String> {
    let mut plist_paths = vec![PathBuf::from("/Applications/Codex.app/Contents/Info.plist")];
    if let Ok(home) = std::env::var("HOME") {
        plist_paths.push(PathBuf::from(home).join("Applications/Codex.app/Contents/Info.plist"));
    }
    for plist in plist_paths {
        if !plist.exists() {
            continue;
        }
        let plist_arg = plist.to_string_lossy().to_string();
        let out = std::process::Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg("Print :CFBundleShortVersionString")
            .arg(&plist_arg)
            .output()
            .ok()?;
        let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn codex_app_version() -> Option<String> {
    None
}

/// 当前 Codex 驱动是否被 U-King 接管（config.toml 含 managed 标记）。
/// 与 providers.rs::apply_codex 写入的 "# managed by U-King" 标记对齐。
fn driver_managed() -> bool {
    let cfg = home_dir().join(".codex").join("config.toml");
    std::fs::read_to_string(&cfg)
        .map(|s| s.contains("managed by U-King"))
        .unwrap_or(false)
}

/// Codex 专区状态：装了没 + 驱动接管了没（都读稳的来源，不探上游私有路径）。
pub fn codex_status() -> CodexStatus {
    let app_installed = crate::installer::codex_app_installed();
    let app_version = if app_installed { codex_app_version() } else { None };
    CodexStatus {
        app_installed,
        app_version,
        driver_managed: driver_managed(),
    }
}
