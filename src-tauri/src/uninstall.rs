//! 卸载 / 清理 —— **只删 U-King 自己装的东西**：`~/.uking`（便携 Node/Git/Python + skills + ukrt +
//! 作图/视频历史 + device.json 等）、用户 PATH 里我们加的目录、桌面快捷方式。右键菜单由 lib.rs
//! 转调 context_menu 注销。
//!
//! 铁律（开发宪法：不碰用户真实状态）：**绝不动用户自己的东西** —— `~/.claude`、`~/.codex`、
//! `~/.openclaw`、npm 全局装的 claude/codex 一律保留（那是用户的 AI 工具与配置，不是我们的）。
//! 删除前须前端强确认。`~/.uking` 可能正被本进程 / 便携 node（openclaw gateway）占用 → 用「等本
//! 进程退出 → 杀 ~/.uking 下残留进程 → 删目录 → 自删」的延迟脚本（隐藏窗口、脱离本进程），
//! 前端调用成功后紧接着退出 app。
//!
//! 独立可插拔：纯 std + 起系统脚本，不 import 其它业务模块；`#[tauri::command]` 包在 lib.rs（本模块不碰 AppHandle）。

use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

/// 卸载主流程（best-effort，逐步回调进度）。返回 Ok 表示「清理已就位」（含已安排退出后删 ~/.uking）。
/// 右键菜单注销不在此（HKCU，lib.rs 转调 context_menu）。调用方在本函数成功后应退出进程。
pub fn run(on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<(), String> {
    // 卸载是**不可逆**动作。客户事后说「你把我的 XX 删了」时，唯一能自证清白（或认错）的
    // 就是这份逐条记录 —— 到底动了哪些 PATH 项、哪个快捷方式、哪个目录。
    // 日志写在 ~/.uking/logs/ 里，会随 ~/.uking 一起被删；但删除是**延迟脚本**在本进程退出后才跑，
    // 客户此刻点「技术支持」仍采得到，正是最需要它的时候。
    crate::ulog::section("uninstall", "开始一键卸载 U-King");
    let notify = on_log;
    let on_log_wrapped = move |m: &str| {
        crate::ulog::write("uninstall", m);
        notify(m);
    };
    let on_log: &(dyn Fn(&str) + Send + Sync) = &on_log_wrapped;
    remove_user_path_entries(on_log);
    remove_shortcut(on_log);
    schedule_home_removal(on_log)?;
    crate::ulog::write("uninstall", "卸载流程已排定（目录删除由延迟脚本在退出后执行）");
    Ok(())
}

// ———————————————— Windows ————————————————

#[cfg(windows)]
fn ps_exe() -> String {
    std::env::var("SystemRoot")
        .map(|r| format!("{r}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"))
        .unwrap_or_else(|_| "powershell".into())
}

/// 从用户 PATH 摘掉所有指向 ~/.uking 的目录（便携 node/git/python）。只删我们加的，别的原样保留。
#[cfg(windows)]
pub fn remove_user_path_entries(on_log: &(dyn Fn(&str) + Send + Sync)) {
    let uking = uking_home().display().to_string().replace('\'', "''");
    let ps = format!(
        "$u='{uking}'; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
         if($p){{ $k=($p -split ';') | Where-Object {{ $_ -and -not $_.StartsWith($u,[StringComparison]::OrdinalIgnoreCase) }}; \
         [Environment]::SetEnvironmentVariable('Path', ($k -join ';'), 'User') }}"
    );
    let _ = Command::new(ps_exe())
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    on_log("已从用户 PATH 移除 U-King 便携工具目录");
}

/// 删桌面 U-King.lnk（只删我们建的这一个）。
#[cfg(windows)]
pub fn remove_shortcut(on_log: &(dyn Fn(&str) + Send + Sync)) {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let lnk = PathBuf::from(home).join("Desktop").join("U-King.lnk");
        if lnk.exists() {
            let _ = std::fs::remove_file(&lnk);
            on_log("已删除桌面快捷方式");
        }
    }
}

/// 起延迟脚本：等本进程退出 → 杀 ~/.uking 下残留进程（gateway 等便携 node）→ 删 ~/.uking → 自删。
#[cfg(windows)]
fn schedule_home_removal(on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<(), String> {
    use std::io::Write as _;
    let uking = uking_home().display().to_string().replace('\'', "''");
    let pid = std::process::id();
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'\r\n\
         while (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 500 }}\r\n\
         Get-CimInstance Win32_Process | Where-Object {{ $_.ExecutablePath -and $_.ExecutablePath.StartsWith('{uking}',[StringComparison]::OrdinalIgnoreCase) }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}\r\n\
         Start-Sleep -Milliseconds 800\r\n\
         for ($i=0; $i -lt 5; $i++) {{ Remove-Item -LiteralPath '{uking}' -Recurse -Force; if (-not (Test-Path -LiteralPath '{uking}')) {{ break }}; Start-Sleep -Milliseconds 700 }}\r\n\
         Remove-Item -LiteralPath $MyInvocation.MyCommand.Path -Force\r\n"
    );
    let script_file = std::env::temp_dir().join(format!("uking_uninstall_{pid}.ps1"));
    std::fs::File::create(&script_file)
        .and_then(|mut f| f.write_all(script.as_bytes()))
        .map_err(|e| format!("写卸载脚本失败: {e}"))?;
    Command::new(ps_exe())
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script_file)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动卸载脚本失败: {e}"))?;
    on_log("已安排：U-King 退出后自动清理 ~/.uking");
    Ok(())
}

// ———————————————— macOS ————————————————

/// 从 ~/.zshrc 摘掉我们加的 ~/.uking PATH 行；删桌面 U-King.app 软链。
#[cfg(not(windows))]
pub fn remove_user_path_entries(on_log: &(dyn Fn(&str) + Send + Sync)) {
    if let Ok(home) = std::env::var("HOME") {
        let rc = PathBuf::from(&home).join(".zshrc");
        if let Ok(txt) = std::fs::read_to_string(&rc) {
            let uking = PathBuf::from(&home).join(".uking");
            let uking_s = uking.display().to_string();
            let kept: Vec<&str> = txt.lines().filter(|l| !(l.contains("export PATH") && l.contains(&uking_s))).collect();
            let _ = std::fs::write(&rc, kept.join("\n") + "\n");
            on_log("已从 ~/.zshrc 移除 U-King 便携工具 PATH");
        }
    }
}

#[cfg(not(windows))]
pub fn remove_shortcut(on_log: &(dyn Fn(&str) + Send + Sync)) {
    if let Ok(home) = std::env::var("HOME") {
        let link = PathBuf::from(home).join("Desktop").join("U-King.app");
        if link.exists() || std::fs::symlink_metadata(&link).is_ok() {
            let _ = std::fs::remove_file(&link);
            on_log("已删除桌面 U-King.app 软链");
        }
    }
}

/// macOS：起延迟 sh 脚本，等本进程退出后删 ~/.uking。
#[cfg(not(windows))]
fn schedule_home_removal(on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;
    let uking = uking_home().display().to_string();
    let pid = std::process::id();
    let script = format!(
        "#!/bin/sh\n\
         while kill -0 {pid} 2>/dev/null; do sleep 0.5; done\n\
         rm -rf \"{uking}\"\n\
         rm -f \"$0\"\n"
    );
    let script_file = std::env::temp_dir().join(format!("uking_uninstall_{pid}.sh"));
    std::fs::File::create(&script_file)
        .and_then(|mut f| f.write_all(script.as_bytes()))
        .map_err(|e| format!("写卸载脚本失败: {e}"))?;
    let _ = std::fs::set_permissions(&script_file, std::fs::Permissions::from_mode(0o755));
    Command::new("sh")
        .arg(&script_file)
        .spawn()
        .map_err(|e| format!("启动卸载脚本失败: {e}"))?;
    on_log("已安排：U-King 退出后自动清理 ~/.uking");
    Ok(())
}
