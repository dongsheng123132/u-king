//! 「一键装到本地」—— 把 U 盘上的 U-King 复制到本机，建桌面快捷方式。
//!
//! ## 为什么要装到本地
//!
//! U 盘 IO 慢、容易被拔走、每次双击都要找盘符。装到本地后：
//! - 复制到 `%USERPROFILE%\.uking`（中文用户名也安全，走环境变量）
//! - 桌面建 `U-King.lnk` 快捷方式（双击直接开管理界面）
//! - 之后不插 U 盘也能用
//!
//! ## 设计取舍（对齐简化版定位）
//!
//! - **只装管家本身**（exe + 资源），AI 工具按需在界面里再装 —— 用户选的「管理器为主」
//! - 纯 std 实现，不引第三方 crate（体积优先）
//! - 复制策略：整目录递归 copy（管家本体就几 MB，不必做增量）

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 安装结果（回前端展示）。
#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    /// 安装目标根目录，如 `C:\Users\xxx\.uking`
    pub install_dir: String,
    /// 桌面快捷方式路径（建失败则为 None）
    pub shortcut: Option<String>,
    /// 复制的文件数
    pub files_copied: u64,
    /// 复制总字节
    pub bytes: u64,
    /// 本机已装过（这次是覆盖更新）
    pub was_update: bool,
}

/// 安装目录：`%USERPROFILE%\.uking`。
pub fn install_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".uking")
}

/// 安装复制的源目录。
/// Windows：exe 所在目录（U 盘上的 U-King/ 或本地安装目录）。
/// macOS：整个 .app bundle（exe 在 U-King.app/Contents/MacOS/ 里，只拷 MacOS 目录会拷出废包）。
fn current_app_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败: {e}"))?;
    #[cfg(target_os = "macos")]
    {
        if let Some(bundle) = exe
            .ancestors()
            .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        {
            return Ok(bundle.to_path_buf());
        }
    }
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "当前程序无父目录".into())
}

/// 是否已经在本地安装目录里运行（避免自己复制自己）。
pub fn running_from_install_dir() -> bool {
    match current_app_dir() {
        Ok(dir) => {
            let target = install_dir();
            // 比对规范化后的路径前缀
            let a = dir.canonicalize().ok();
            let b = target.canonicalize().ok();
            match (a, b) {
                (Some(a), Some(b)) => a.starts_with(&b),
                _ => false,
            }
        }
        Err(_) => false,
    }
}

/// 执行安装：把当前 app 目录（U 盘上的 U-King）递归复制到 `~/.uking`。
///
/// `on_progress(rel_path, copied_so_far)` 流式回调，前端可显示进度。
pub fn install_to_local(mut on_progress: impl FnMut(&str, u64)) -> Result<InstallResult, String> {
    let src = current_app_dir()?;
    let dst = install_dir();
    let was_update = dst.exists();
    // 「装到本地」是从 U 盘整包复制，客户机上最常见的失败是盘满 / 文件被占（旧 exe 正在跑）/
    // 杀软拦截。日志记住是首装还是覆盖更新、源和目标在哪 —— 覆盖更新失败尤其危险（可能留下半套）。
    crate::ulog::section(
        "install-local",
        &format!("{} src={} dst={}", if was_update { "覆盖更新" } else { "首次安装" }, src.display(), dst.display()),
    );
    let mut on_progress = move |m: &str, p: u64| {
        crate::ulog::write("install-local", m);
        on_progress(m, p);
    };

    // macOS：源是 .app bundle，目标保留包结构 ~/.uking/U-King.app
    let copy_dst = if src.extension().map(|e| e == "app").unwrap_or(false) {
        dst.join(src.file_name().unwrap_or_default())
    } else {
        dst.clone()
    };

    std::fs::create_dir_all(&copy_dst)
        .map_err(|e| format!("创建安装目录失败 {}: {e}", copy_dst.display()))?;

    let mut files_copied = 0u64;
    let mut bytes = 0u64;
    copy_dir_recursive(&src, &copy_dst, &src, &mut |rel, n| {
        files_copied += 1;
        bytes += n;
        on_progress(rel, files_copied);
    })?;

    // 桌面快捷方式
    let shortcut = create_desktop_shortcut(&dst).ok();

    // U 盘护符：装到本地即授权本机永久可用（之后不插 U 盘也能跑）。
    // 下载版（未带 usb-guard）内部空转、不留文件。
    crate::guard::mark_local_activation();

    Ok(InstallResult {
        install_dir: dst.display().to_string(),
        shortcut: shortcut.map(|p| p.display().to_string()),
        files_copied,
        bytes,
        was_update,
    })
}

/// 递归复制目录。跳过 `node_modules` / `.git` 这类无关目录。
/// `pub(crate)`：tools.rs 复制 Open365 到本地时复用（公共能力不复制）。
pub(crate) fn copy_dir_recursive(
    dir: &Path,
    dst_root: &Path,
    src_root: &Path,
    on_file: &mut impl FnMut(&str, u64),
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("读取目录失败 {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("遍历目录项失败: {e}"))?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            // 跳过无关大目录
            if matches!(name_str.as_ref(), "node_modules" | ".git" | "target" | ".cache") {
                continue;
            }
            copy_dir_recursive(&path, dst_root, src_root, on_file)?;
        } else if path.is_file() {
            let rel = path.strip_prefix(src_root).unwrap_or(&path);
            let dst = dst_root.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败 {}: {e}", parent.display()))?;
            }
            // 跳过把自己（正在运行的 exe）复制到同一路径 —— Windows 会锁文件
            let same = dst.canonicalize().ok() == path.canonicalize().ok() && dst.exists();
            if same {
                continue;
            }
            match std::fs::copy(&path, &dst) {
                Ok(n) => on_file(&rel.to_string_lossy(), n),
                Err(e) => {
                    // exe 正在运行被占用是常见的；不致命，跳过即可（覆盖更新时本就有旧的）
                    if !dst.exists() {
                        return Err(format!("复制 {} 失败: {e}", path.display()));
                    }
                }
            }
        }
    }
    Ok(())
}

/// 在桌面创建指向 `install_dir\U-King.exe` 的快捷方式。
///
/// Windows: 用 PowerShell 的 WScript.Shell COM 建 .lnk（不引第三方 crate）。
/// 注：纯 Rust 手写 .lnk 二进制试过，但仅 LinkInfo 无 IDList 时 Shell 解析不出
/// TargetPath（实测 roundtrip 失败），格式正确成本高、收益低 —— 快捷方式失败本
/// 就非致命（install_to_local 用 .ok() 吞掉、装机照常完成），故仍用 PowerShell。
#[cfg(windows)]
fn create_desktop_shortcut(install_dir: &Path) -> Result<PathBuf, String> {
    let desktop = std::env::var("USERPROFILE")
        .map(|h| Path::new(&h).join("Desktop"))
        .map_err(|_| "找不到桌面目录".to_string())?;
    let lnk = desktop.join("U-King.lnk");
    let exe = install_dir.join("U-King.exe");

    let ps = format!(
        r#"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}');$s.TargetPath='{exe}';$s.WorkingDirectory='{wd}';$s.IconLocation='{exe}';$s.Description='U-King 个人 AI 操作系统';$s.Save()"#,
        lnk = lnk.display(),
        exe = exe.display(),
        wd = install_dir.display(),
    );

    let status = std::process::Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status()
        .map_err(|e| format!("调用 PowerShell 建快捷方式失败: {e}"))?;

    if status.success() {
        Ok(lnk)
    } else {
        Err("PowerShell 建快捷方式返回非零".into())
    }
}

/// macOS：桌面放一个指向已装 .app 的软链（Finder 双击即开）。
#[cfg(target_os = "macos")]
fn create_desktop_shortcut(install_dir: &Path) -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "找不到 HOME".to_string())?;
    let app = install_dir.join("U-King.app");
    if !app.exists() {
        return Err("安装目录里没有 U-King.app".into());
    }
    let link = Path::new(&home).join("Desktop").join("U-King.app");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&app, &link).map_err(|e| format!("建桌面软链失败: {e}"))?;
    Ok(link)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn create_desktop_shortcut(_install_dir: &Path) -> Result<PathBuf, String> {
    Err("当前平台暂不支持自动建快捷方式".into())
}

/// 给**当前正在运行的** exe 建桌面快捷方式（绿色版「固定到桌面」用）。
/// 绿色版是用户随手放的单 exe（U 盘/下载夹），固定后下次不用再翻文件夹找。
/// 指向 current_exe 本身，不复制、不安装 —— 跟「装到本地」区分（那是复制到 ~/.uking）。
#[cfg(windows)]
pub fn pin_current_exe_to_desktop() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败: {e}"))?;
    let wd = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let desktop = std::env::var("USERPROFILE")
        .map(|h| Path::new(&h).join("Desktop"))
        .map_err(|_| "找不到桌面目录".to_string())?;
    let lnk = desktop.join("U-King.lnk");
    let ps = format!(
        r#"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}');$s.TargetPath='{exe}';$s.WorkingDirectory='{wd}';$s.IconLocation='{exe}';$s.Description='U-King 个人 AI 操作系统';$s.Save()"#,
        lnk = lnk.display(),
        exe = exe.display(),
        wd = wd.display(),
    );
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW，防黑窗
        .status()
        .map_err(|e| format!("调用 PowerShell 建快捷方式失败: {e}"))?;
    if status.success() {
        Ok(lnk.display().to_string())
    } else {
        Err("建快捷方式失败".into())
    }
}

#[cfg(not(windows))]
pub fn pin_current_exe_to_desktop() -> Result<String, String> {
    Err("当前平台暂不支持「固定到桌面」".into())
}

/// 打开本地安装目录（资源管理器 / Finder）。
pub fn reveal_install_dir() -> Result<(), String> {
    let dir = install_dir();
    if !dir.exists() {
        return Err("尚未安装到本地".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("打开目录失败: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("打开目录失败: {e}"))?;
    }
    Ok(())
}
