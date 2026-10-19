//! OpenCodex 桌面工作台（独立 Tauri 产品）的随包内嵌薄壳。
//!
//! 为什么内嵌：U-Chat/UCLI 已删，OpenCodex 是「文件夹式 CLI 终端工作台」的替代；用户拍板
//! 「打进安装包随包走」，于是像 ukrt.exe 一样 `include_bytes!` 打进 u-king-mini.exe，
//! 首启按需释放到 ~/.uking/opencodex/，便携 exe 与 NSIS 安装包两条路都生效。
//! 铁律：只暴露纯函数，不碰 AppHandle；#[tauri::command] 包装写在 lib.rs / tools.rs。
//! 体积代价 ~3MB（OpenCodex 0.5.0 release 单文件）。仅 Windows 随包——Mac 是 .app 目录，
//! 不能单文件内嵌，暂不随包。

use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 内嵌的 OpenCodex.exe（构建时打进 u-king-mini.exe，运行时按需释放到 ~/.uking/opencodex/）。
/// 单一真相源：resources/OpenCodex.exe = opencodex 仓库（../opencodex开源的codex工具/）release 构建产物。
#[cfg(windows)]
const OPENCODEX_EMBEDDED: &[u8] = include_bytes!("../resources/OpenCodex.exe");
/// OpenCodex 使用 MIT License；随 exe 一并释放，满足再分发时保留许可文本的要求。
#[cfg(windows)]
const OPENCODEX_LICENSE: &[u8] = include_bytes!("../resources/OpenCodex-LICENSE.txt");

/// 保证 OpenCodex.exe 可用（找不到就释放内嵌版本）：
/// 1) OPENCODEX_PATH 显式指定（开发调试）→ 尊重
/// 2) ~/.uking/opencodex/OpenCodex.exe 且大小与内嵌一致 → 用（不一致视为旧版/半包 → 重释放）
/// 3) 当前 exe 同目录（U 盘旁挂）→ 尊重
/// 4) 释放内嵌字节：写 .tmp 再 rename（原子防半包；被占用则报错不硬写）
#[cfg(windows)]
fn ensure_license(dir: &std::path::Path) -> Result<(), String> {
    let license = dir.join("OpenCodex-LICENSE.txt");
    if !license.exists() {
        std::fs::write(&license, OPENCODEX_LICENSE)
            .map_err(|e| format!("释放 OpenCodex 许可文本失败：{}", e))?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn ensure_opencodex() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("OPENCODEX_PATH") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Ok(pb);
        }
    }
    let home = std::env::var("USERPROFILE").map_err(|_| "读不到 USERPROFILE".to_string())?;
    let dir = PathBuf::from(&home).join(".uking").join("opencodex");
    let dst = dir.join("OpenCodex.exe");
    let same_size = std::fs::metadata(&dst)
        .map(|m| m.len() == OPENCODEX_EMBEDDED.len() as u64)
        .unwrap_or(false);
    if same_size {
        ensure_license(&dir)?;
        return Ok(dst);
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(d) = me.parent() {
            let pb = d.join("OpenCodex.exe");
            if pb.exists() {
                return Ok(pb);
            }
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 opencodex 目录失败：{}", e))?;
    let tmp = dir.join("OpenCodex.exe.tmp");
    std::fs::write(&tmp, OPENCODEX_EMBEDDED).map_err(|e| format!("释放 OpenCodex 失败：{}", e))?;
    std::fs::rename(&tmp, &dst).map_err(|e| format!("替换 OpenCodex 失败（可能被占用）：{}", e))?;
    ensure_license(&dir)?;
    Ok(dst)
}

#[cfg(not(windows))]
pub fn ensure_opencodex() -> Result<PathBuf, String> {
    Err("OpenCodex 随包仅 Windows 提供".into())
}

/// 启动 OpenCodex 桌面工作台（内嵌 exe，确保释放后拉起）。
#[cfg(windows)]
pub fn launch_opencodex() -> Result<(), String> {
    let exe = ensure_opencodex()?;
    let mut cmd = Command::new(&exe);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map_err(|e| format!("启动 OpenCodex 失败: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn launch_opencodex() -> Result<(), String> {
    Err("OpenCodex 随包仅 Windows 提供".into())
}
