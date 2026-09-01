//! Windows 右键菜单集成 —— 「右键目录 → 用 U-King 打开」。
//!
//! ## 思路
//!
//! 写 `HKEY_CURRENT_USER\Software\Classes\Directory\shell\U-King`（当前用户，免管理员）：
//! - `Directory\shell\U-King`            → 右键文件夹本身
//! - `Directory\Background\shell\U-King`  → 在文件夹空白处右键
//! - `Drive\shell\U-King`                 → 右键盘符（U 盘）
//!
//! 命令指向本地安装目录的 `U-King.exe --open-dir "%V"`，由 main 解析参数后
//! 让管理界面定位到该目录（v0.1 先把目录路径透传给前端展示）。
//!
//! ## 为什么用注册表而不是 .reg 文件
//!
//! 直接调 `reg add` / `reg delete`，安装/卸载都在程序内一键完成，
//! 用户不用手动双击 .reg、不弹 UAC（HKCU 不需要提权）。

#[cfg(windows)]
use std::path::Path;

/// 注册右键菜单。`exe` 是本地安装后的 U-King.exe 完整路径。
#[cfg(windows)]
pub fn register(exe: &Path) -> Result<(), String> {
    let exe = exe.display().to_string();
    let icon = format!("\"{exe}\",0");
    let command = format!("\"{exe}\" --open-dir \"%V\"");
    let label = "用 U-King 打开";

    // 三处挂载点
    let bases = [
        r"HKCU\Software\Classes\Directory\shell\U-King",
        r"HKCU\Software\Classes\Directory\Background\shell\U-King",
        r"HKCU\Software\Classes\Drive\shell\U-King",
    ];

    for base in bases {
        // 菜单标题
        reg_add(base, "", "REG_SZ", label)?;
        // 菜单图标
        reg_add(base, "Icon", "REG_SZ", &icon)?;
        // 命令
        let cmd_key = format!(r"{base}\command");
        reg_add(&cmd_key, "", "REG_SZ", &command)?;
    }
    Ok(())
}

/// 注销右键菜单。
#[cfg(windows)]
pub fn unregister() -> Result<(), String> {
    let bases = [
        r"HKCU\Software\Classes\Directory\shell\U-King",
        r"HKCU\Software\Classes\Directory\Background\shell\U-King",
        r"HKCU\Software\Classes\Drive\shell\U-King",
    ];
    for base in bases {
        // /f 静默，不存在也不报错（忽略返回）
        let _ = std::process::Command::new(crate::installer::system_tool("reg"))
            .args(["delete", base, "/f"])
            .status();
    }
    Ok(())
}

/// 查询右键菜单是否已注册。
#[cfg(windows)]
pub fn is_registered() -> bool {
    std::process::Command::new(crate::installer::system_tool("reg"))
        .args(["query", r"HKCU\Software\Classes\Directory\shell\U-King"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn reg_add(key: &str, value_name: &str, ty: &str, data: &str) -> Result<(), String> {
    let mut args = vec!["add", key];
    if value_name.is_empty() {
        args.push("/ve"); // 默认值
    } else {
        args.push("/v");
        args.push(value_name);
    }
    args.extend(["/t", ty, "/d", data, "/f"]);

    let status = std::process::Command::new(crate::installer::system_tool("reg"))
        .args(&args)
        .status()
        .map_err(|e| format!("调用 reg 失败: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg add {key} 返回非零"))
    }
}

// ---- 非 Windows 占位（Mac/Linux 暂不做右键集成）----

#[cfg(not(windows))]
pub fn register(_exe: &std::path::Path) -> Result<(), String> {
    Err("当前平台暂不支持右键菜单集成".into())
}

#[cfg(not(windows))]
pub fn unregister() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn is_registered() -> bool {
    false
}
