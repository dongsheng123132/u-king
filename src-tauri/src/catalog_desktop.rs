//! Magpie 便携 GUI 与腾讯国内 WorkBuddy 的官方安装核心。
//! 调用方必须放在已有安装 worker / spawn_blocking 中；网络进程均有硬超时。
//! 不配置模型、不执行未核验的安装包，UKING_TEST_HOME 下不启动系统安装器。
//!
//! ## 边界（模块耦合闸门只有 1 条边：tools → 本模块）
//! Magpie 的落点 / 收据 / PE 判据 / 启动全在本模块——它是 **U-King 自己落盘的便携程序**，
//! 不是卸载表里的桌面应用，状态归安装核心所有；工具目录（tools.rs）直接问本模块
//! （检测/版本/启动），这是本次新增的唯一一条模块耦合边。
//! WorkBuddy / 豆包消费版仍是注册表 / bundle 桌面应用，探测归 `desktop_app`。

#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};
use serde_json::Value;

const MAGPIE_API: &str = "https://api.github.com/repos/yetone/magpie/releases/latest";
const WORKBUDDY_API: &str = "https://www.workbuddy.cn/v2/update?platform=workbuddy-win32-x64-user";
const MIN_EXE_BYTES: u64 = 1_000_000;

#[derive(Debug)]
struct Download {
    version: String,
    url: String,
    size: Option<u64>,
    sha256: Option<String>,
}

fn sha256(value: &str) -> Result<String, String> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("官方更新信息中的 SHA-256 格式无效".into());
    }
    Ok(value.to_ascii_lowercase())
}

fn magpie_release(json: &str, arch: &str) -> Result<Download, String> {
    let name = match arch {
        "x86_64" => "magpie-windows-amd64.exe",
        "aarch64" => "magpie-windows-arm64.exe",
        _ => return Err("Magpie 暂不支持此 Windows 架构".into()),
    };
    let v: Value = serde_json::from_str(json).map_err(|e| format!("Magpie 官方更新信息无效: {e}"))?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Err("Magpie 官方更新不是正式发行版".into());
    }
    let version = v["tag_name"].as_str().filter(|s| !s.is_empty())
        .ok_or("Magpie 官方更新缺少版本号")?.to_string();
    let asset = v["assets"].as_array().and_then(|a| a.iter().find(|a| a["name"].as_str() == Some(name)))
        .ok_or("Magpie 官方发行版未提供当前架构的 GUI 程序（不能用 CLI 包代替）")?;
    let url = asset["browser_download_url"].as_str().ok_or("Magpie 官方更新缺少下载地址")?;
    if !url.starts_with("https://github.com/yetone/magpie/releases/download/") || !url.ends_with(&format!("/{name}")) {
        return Err("Magpie 下载地址不是指定官方仓库的 GUI 发行包".into());
    }
    let size = asset["size"].as_u64().filter(|n| *n >= MIN_EXE_BYTES)
        .ok_or("Magpie 官方更新缺少有效文件大小")?;
    let digest = match asset.get("digest").filter(|d| !d.is_null()) {
        None => None,
        Some(d) => {
            let d = d.as_str().ok_or("Magpie 官方 digest 格式无效")?;
            Some(sha256(d.strip_prefix("sha256:").ok_or("Magpie 官方 digest 不是 SHA-256")?)?)
        }
    };
    Ok(Download { version, url: url.into(), size: Some(size), sha256: digest })
}

fn workbuddy_release(json: &str) -> Result<Download, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("WorkBuddy 官方更新信息无效: {e}"))?;
    let version = v["productVersion"].as_str().or_else(|| v["version"].as_str())
        .filter(|s| !s.is_empty()).ok_or("WorkBuddy 官方更新缺少版本号")?.to_string();
    let url = v["url"].as_str().ok_or("WorkBuddy 官方更新缺少下载地址")?;
    if !url.starts_with("https://download.codebuddy.cn/workbuddy/") || !url.ends_with(".exe") {
        return Err("WorkBuddy 下载地址不是腾讯国内版官方安装包".into());
    }
    let hash = sha256(v["sha256hash"].as_str().ok_or("WorkBuddy 官方更新缺少 SHA-256")?)?;
    Ok(Download { version, url: url.into(), size: v["size"].as_u64(), sha256: Some(hash) })
}

/// 读 DOS 偏移指向的 PE 签名，不能只凭 MZ 两个字符把 HTML / 占位壳算作 exe。
fn pe_header(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || &bytes[..2] != b"MZ" { return false; }
    let offset = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]) as usize;
    offset >= 64 && offset.checked_add(24).is_some_and(|end| end <= bytes.len())
        && bytes.get(offset..offset + 4) == Some(b"PE\0\0".as_slice())
}

fn verify_bytes(bytes: &[u8], spec: &Download) -> Result<(), String> {
    let size = bytes.len() as u64;
    if size < MIN_EXE_BYTES || spec.size.is_some_and(|n| n != size) {
        return Err(format!("下载包大小不符（实得 {size} 字节，官方大小 {:?}），可能是错误页或截断文件", spec.size));
    }
    if !pe_header(bytes) { return Err("下载内容不是有效的 Windows PE 程序，拒绝执行".into()); }
    if let Some(expected) = &spec.sha256 {
        if !crate::installer::sha256_hex_bytes(bytes).eq_ignore_ascii_case(expected) {
            return Err("下载包 SHA-256 与官方更新信息不符，拒绝执行".into());
        }
    }
    Ok(())
}

pub fn magpie_exe() -> PathBuf {
    crate::installer::user_home_dir().join(".uking/apps/magpie/magpie.exe")
}

/// 高频检测只读头部和大小；完整 hash 在每次安装/替换及落盘回读时核验。
/// macOS 走 bundle 存在性探测（`magpie.app`，官方 Makefile 实证的落点）。
pub fn magpie_installed() -> bool {
    if cfg!(target_os = "macos") {
        return magpie_bundle().is_some();
    }
    use std::io::{Read, Seek, SeekFrom};
    let path = magpie_exe();
    let Ok(mut file) = std::fs::File::open(path) else { return false; };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    if size < MIN_EXE_BYTES { return false; }
    let mut dos = [0u8; 64];
    if file.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" { return false; }
    let offset = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    if offset < 64 || offset + 24 > size || file.seek(SeekFrom::Start(offset)).is_err() { return false; }
    let mut sig = [0u8; 4];
    file.read_exact(&mut sig).is_ok() && sig == *b"PE\0\0"
}

/// macOS：官方 Makefile 产出 `magpie.app/Contents/MacOS/magpie`；只做存在性探测，未证实不猜。
#[cfg(target_os = "macos")]
fn magpie_bundle() -> Option<PathBuf> {
    let roots = [
        PathBuf::from("/Applications"),
        crate::installer::user_home_dir().join("Applications"),
    ];
    roots
        .into_iter()
        .map(|root| root.join("magpie.app"))
        .find(|app| app.join("Contents/MacOS/magpie").is_file())
}

#[cfg(not(target_os = "macos"))]
fn magpie_bundle() -> Option<PathBuf> {
    None
}

pub fn magpie_version() -> Option<String> {
    let path = magpie_exe();
    let json: Value = serde_json::from_slice(&std::fs::read(path.with_extension("json")).ok()?).ok()?;
    if json["size"].as_u64()? != std::fs::metadata(path).ok()?.len() { return None; }
    json["version"].as_str().map(str::to_owned)
}

/// 直接起落盘的便携 GUI（不经 PATH；`UKING_TEST_HOME` 时起的是沙箱里的那一份）。
/// macOS 按已探测到的 `magpie.app` 用 `open -a` 拉起。
pub fn launch_magpie() -> Result<(), String> {
    if !magpie_installed() {
        return Err("未找到 Magpie（可能已被移动或删除），请重新安装".into());
    }
    #[cfg(target_os = "macos")]
    {
        let bundle = magpie_bundle().ok_or("未找到 Magpie.app")?;
        std::process::Command::new("open")
            .arg("-a")
            .arg(bundle)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("启动 Magpie 失败: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let exe = magpie_exe();
        let dir = exe.parent().map(|p| p.to_path_buf());
        let mut c = std::process::Command::new(&exe);
        if let Some(d) = &dir {
            c.current_dir(d);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        c.spawn().map(|_| ()).map_err(|e| format!("启动 Magpie 失败: {e}"))
    }
}

fn unique_suffix() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!("{}-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// 同目录暂存，备份成功后直接 rename 覆盖；失败时不删除原文件。
fn replace_magpie(staged: &Path, target: &Path, spec: &Download) -> Result<(), String> {
    let bytes = std::fs::read(staged).map_err(|e| format!("读取 Magpie 下载包失败: {e}"))?;
    verify_bytes(&bytes, spec)?;
    if staged.parent() != target.parent() { return Err("Magpie 暂存文件必须与目标在同一目录".into()); }
    if target.exists() {
        let backup = target.with_file_name(format!("magpie.exe.uking-bak.{}", unique_suffix()));
        std::fs::copy(target, &backup).map_err(|e| format!("备份原 Magpie 失败，未替换: {e}"))?;
    }
    std::fs::OpenOptions::new().write(true).open(staged).and_then(|f| f.sync_all())
        .map_err(|e| format!("同步 Magpie 暂存文件失败: {e}"))?;
    std::fs::rename(staged, target).map_err(|e| format!("替换 Magpie 失败（请关闭正在运行的 Magpie 后重试）: {e}"))?;
    let back = std::fs::read(target).map_err(|e| format!("Magpie 落盘后无法读回: {e}"))?;
    verify_bytes(&back, spec)?;
    if back != bytes { return Err("Magpie 落盘回读不一致".into()); }
    let receipt = serde_json::to_vec(&serde_json::json!({"version": spec.version, "size": back.len()}))
        .map_err(|e| e.to_string())?;
    let receipt_tmp = target.with_file_name(format!(".magpie-receipt-{}", unique_suffix()));
    std::fs::write(&receipt_tmp, receipt).map_err(|e| format!("写 Magpie 版本记录失败: {e}"))?;
    std::fs::rename(&receipt_tmp, target.with_extension("json")).map_err(|e| format!("替换 Magpie 版本记录失败: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn official_curl(url: &str, destination: Option<&Path>) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(crate::installer::system_tool("curl"));
    c.args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--proto-redir", "=https",
        "--connect-timeout", "15", "--max-time", if destination.is_some() { "600" } else { "30" },
        "--user-agent", "U-King"]);
    if let Some(path) = destination { c.arg("--output").arg(path); }
    c.arg(url).creation_flags(0x0800_0000);
    let output = crate::installer::output_with_timeout(c, if destination.is_some() { 610 } else { 40 })
        .ok_or("官方下载超时或无法启动系统 curl")?;
    if !output.status.success() {
        // 不回显 URL / stderr：某些 CDN 的重定向地址会含临时凭据。
        return Err(format!("官方下载失败（curl 退出码 {:?}），请稍后重试或使用官网入口", output.status.code()));
    }
    String::from_utf8(output.stdout).map_err(|_| "官方更新信息不是 UTF-8".into())
}

/// Windows 安装/升级共用此入口。成功返回刚下载、校验、替换并读回的版本号；不会因旧 exe 存在跳过。
#[cfg(windows)]
pub fn install_magpie(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    on_log("step", "获取 Magpie 官方最新 GUI 发行版…");
    let spec = magpie_release(&official_curl(MAGPIE_API, None)?, std::env::consts::ARCH)?;
    let target = magpie_exe();
    let dir = target.parent().ok_or("Magpie 安装目录无效")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("创建 Magpie 目录失败: {e}"))?;
    let staged = dir.join(format!(".magpie-download-{}.exe", unique_suffix()));
    // create_new 避免并发调用共享临时文件。只会触及用户主动安装的便携目录。
    std::fs::OpenOptions::new().write(true).create_new(true).open(&staged)
        .map_err(|e| format!("创建 Magpie 暂存文件失败: {e}"))?;
    on_log("out", &format!("正在下载 Magpie {}（{} 字节）并校验官方摘要…", spec.version, spec.size.unwrap_or(0)));
    let result = official_curl(&spec.url, Some(&staged)).and_then(|_| replace_magpie(&staged, &target, &spec));
    let _ = std::fs::remove_file(&staged);
    result?;
    // 不需要清 desktop_app 快照：Magpie 检测不走卸载表缓存（每次现查磁盘 PE）。
    Ok(spec.version)
}

fn workbuddy_pending(_previous_version: Option<&str>, _requested_version: &str) -> Result<String, String> {
    // 打开安装界面既不证明安装完成，也不证明旧版本已经更新。
    Err("WorkBuddy 安装未完成：已打开腾讯官方安装界面，待用户操作。请按提示完成安装后刷新工具列表；模型按任务选择。".into())
}

/// 未实测静默参数，只打开经 hash/大小/PE 核验的官方安装器，始终返回待用户操作。
#[cfg(windows)]
pub fn install_workbuddy(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    if std::env::var("UKING_TEST_HOME").is_ok_and(|v| !v.trim().is_empty()) {
        return Err("沙箱禁止启动 WorkBuddy 系统安装器".into());
    }
    on_log("step", "获取腾讯国内 WorkBuddy 官方更新信息…");
    let spec = workbuddy_release(&official_curl(WORKBUDDY_API, None)?)?;
    let dir = crate::installer::user_home_dir().join(".uking/downloads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let staged = dir.join(format!("WorkBuddy-Setup-{}.exe", unique_suffix()));
    std::fs::OpenOptions::new().write(true).create_new(true).open(&staged).map_err(|e| e.to_string())?;
    let download = official_curl(&spec.url, Some(&staged)).and_then(|_| {
        let bytes = std::fs::read(&staged).map_err(|e| e.to_string())?;
        verify_bytes(&bytes, &spec)
    });
    if let Err(e) = download { let _ = std::fs::remove_file(&staged); return Err(e); }
    on_log("out", "官方安装包校验完成，正在打开安装界面，请按提示操作…");
    if let Err(e) = std::process::Command::new(&staged).creation_flags(0x0800_0000).spawn() {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("打开 WorkBuddy 官方安装器失败: {e}"));
    }
    // 不清 desktop_app 快照：卸载表快照 TTL 只有 10 秒，且 exe 存在性每次都现查；
    // 用户按提示装完（分钟级）后的刷新必然拿到新状态。这里绝不能拿「打开过安装器」当成功。
    // 安装进程需要继续读取安装包，不能在此删包。显式返回待完成，不以已有旧 exe 判成功。
    workbuddy_pending(None, &spec.version)
}

#[cfg(not(windows))]
pub fn install_magpie(_on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请到 Magpie 官方发行页下载并手动安装（装好后可一键打开）".into())
}

#[cfg(not(windows))]
pub fn install_workbuddy(_on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请到 WorkBuddy 官网下载并手动安装（检测到已装后可一键打开）".into())
}

// ───────────────────────── 统一安装分派入口（install/upgrade/verify/installed） ─────────────────────────
// installer.rs 的 install_tool_with_intent / InstallIntent 分派对这三个 id 各接一对
// install/upgrade + 一个 verify；签名与 dshdesk::{install,upgrade} 对齐，方便同一张分派表收口。

/// Upgrade 与 Install 同一实现：Magpie 每次都拉官方最新 GUI、校验后原子替换落盘，
/// **没有「旧 exe 还在就跳过」的路径**——旧文件永远不会被当成新安装完成。
pub fn upgrade_magpie(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    install_magpie(on_log)
}

/// 安装后验证：能定位到可启动的 Magpie（Windows 落盘 PE / macOS magpie.app）即通过。
pub fn verify_magpie() -> Result<String, String> {
    if !magpie_installed() {
        return Err("安装已结束，但没有找到可启动的 Magpie".into());
    }
    Ok(magpie_version().unwrap_or_else(|| "已找到 Magpie 程序".into()))
}

/// Upgrade 与 Install 同一实现：总是按官方更新信息打开交互安装界面并返回「待用户操作」，
/// **绝不因旧版已装而报升级/安装完成**。
pub fn upgrade_workbuddy(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    install_workbuddy(on_log)
}

/// WorkBuddy 装没装（注册表 / bundle 探测，经 installer 公共层问 desktop_app，不新增模块边）。
pub fn workbuddy_installed() -> bool {
    crate::installer::managed_desktop_installed("workbuddy")
}

/// 🔴 这里的 `Ok` 只证明「本机装着某个 WorkBuddy」（**可能是旧版**），绝不能当作
/// 「本次安装/升级完成」的凭据——安装完成与否只能由 `install_workbuddy` 的返回值表达
/// （它恒为「待用户操作」的 Err）。它只用于已装检测 / 版本展示 / 修复后的复查。
pub fn verify_workbuddy() -> Result<String, String> {
    if workbuddy_installed() {
        Ok(crate::installer::managed_desktop_version("workbuddy")
            .unwrap_or_else(|| "已找到 WorkBuddy 主程序".into()))
    } else {
        Err("安装未完成：尚未检测到 WorkBuddy 主程序（待用户在官方安装界面完成操作）".into())
    }
}

/// 豆包**消费桌面版**装没装（desktop_app 规则 id "doubao-desktop"；老 "doubao" 是豆包工作）。
pub fn doubao_desktop_installed() -> bool {
    crate::installer::managed_desktop_installed("doubao-desktop")
}

/// 豆包消费桌面版的下载直链尚未核实，不做假安装；统一安装分派收到它时给官网指引。
pub fn install_doubao_desktop(_on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    Err("豆包桌面版请到官网 https://www.doubao.com/download/desktop 下载安装（下载直链尚未核实，U-King 不代装）；检测到已装后可一键打开".into())
}

/// 与 install 同一实现（无代装能力，升级也走官网）。
pub fn upgrade_doubao_desktop(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    install_doubao_desktop(on_log)
}

/// 豆包消费桌面版安装后复查（已装检测口径与「豆包工作」严格分开）。
pub fn verify_doubao_desktop() -> Result<String, String> {
    if doubao_desktop_installed() {
        Ok(crate::installer::managed_desktop_version("doubao-desktop")
            .unwrap_or_else(|| "已找到豆包主程序".into()))
    } else {
        Err("未检测到豆包桌面版（如已下载安装包，请先完成官网安装）".into())
    }
}

/// 这条错误是不是**设计内结局**（沙箱拦截 / 当前平台不代装只指路 / 已打开官方安装界面
/// 待用户操作）——每次点都会走到，不是安装 bug。分派层拿它把这类 Err 报成
/// `precheck_blocked`（不建 issue），免得用户每点一次 WorkBuddy/豆包就自动建一条工单；
/// 真故障（下载失败、摘要不匹配、PE 核验不过）不命中这里，照旧走 `fail` 建 issue。
/// 前缀逐一对应本模块自己产出的错误字面量，改文案要两处一起改（见同名单测）。
pub fn is_design_outcome(err: &str) -> bool {
    const MARKS: [&str; 5] = [
        "沙箱禁止启动 WorkBuddy 系统安装器",
        "WorkBuddy 安装未完成",
        "豆包桌面版请到官网",
        "当前平台请到 Magpie 官方发行页",
        "当前平台请到 WorkBuddy 官网",
    ];
    MARKS.iter().any(|m| err.starts_with(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> Vec<u8> {
        let mut b = vec![0; MIN_EXE_BYTES as usize];
        b[..2].copy_from_slice(b"MZ");
        b[60..64].copy_from_slice(&64u32.to_le_bytes());
        b[64..68].copy_from_slice(b"PE\0\0");
        b
    }
    fn spec(bytes: &[u8]) -> Download {
        Download { version: "v0.0.2".into(), url: "https://example.com/demo.exe".into(),
            size: Some(bytes.len() as u64), sha256: Some(crate::installer::sha256_hex_bytes(bytes)) }
    }

    #[test]
    fn magpie_selects_gui_and_checks_official_metadata() {
        let json = serde_json::json!({"tag_name":"v0.0.2", "assets":[
            {"name":"magpie-cli-windows-amd64.exe", "size":1000000},
            {"name":"magpie-windows-amd64.exe", "size":56049152,
             "digest":format!("sha256:{}", "a".repeat(64)),
             "browser_download_url":"https://github.com/yetone/magpie/releases/download/v0.0.2/magpie-windows-amd64.exe"}]}).to_string();
        let release = magpie_release(&json, "x86_64").unwrap();
        assert_eq!(release.size, Some(56049152));
        assert!(!release.url.contains("cli"));
        assert!(magpie_release(&json.replace("sha256:", "md5:"), "x86_64").is_err());
        assert!(magpie_release(&json.replace("github.com/yetone", "example.com/yetone"), "x86_64").is_err());
        assert!(magpie_release(&json, "unsupported").is_err());
    }

    #[test]
    fn rejects_html_fake_pe_truncation_and_bad_digest() {
        let good = exe();
        let s = spec(&good);
        assert!(verify_bytes(&good, &s).is_ok());
        let mut fake = good.clone();
        // 伪造「下载到的是错误页」：头部换成 HTML（长度必须与目标切片一致，17 字节）
        fake[..17].copy_from_slice(b"<html>error</html");
        assert!(verify_bytes(&fake, &spec(&fake)).is_err());
        fake[..2].copy_from_slice(b"MZ");
        fake[64..68].fill(0);
        assert!(verify_bytes(&fake, &spec(&fake)).is_err());
        assert!(verify_bytes(&good[..100], &s).is_err());
        fake = good.clone();
        fake[100] = 1;
        assert!(verify_bytes(&fake, &s).is_err());
    }

    #[test]
    fn replaces_only_verified_download_and_keeps_old_backup() {
        let dir = std::env::temp_dir().join(format!("uking-catalog-fixture-{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("magpie.exe");
        let staged = dir.join(".download.exe");
        let old = b"old demo executable";
        std::fs::write(&target, old).unwrap();
        let bytes = exe();
        let s = spec(&bytes);
        std::fs::write(&staged, b"<html>not an installer</html>").unwrap();
        assert!(replace_magpie(&staged, &target, &s).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), old);
        std::fs::write(&staged, &bytes).unwrap();
        replace_magpie(&staged, &target, &s).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        let backups: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("magpie.exe.uking-bak.")).collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(backups[0].path()).unwrap(), old);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_workbuddy_cannot_prove_new_installation_completed() {
        assert!(workbuddy_pending(Some("0.0.1"), "0.0.2").unwrap_err().contains("待用户操作"));
        assert!(workbuddy_pending(Some("0.0.2"), "0.0.2").is_err());
        let json = serde_json::json!({"version":"0.0.2", "sha256hash":"a".repeat(64),
            "url":"https://download.codebuddy.cn/workbuddy/saas/win32-x64-user/demo.exe"}).to_string();
        assert!(workbuddy_release(&json).is_ok());
        assert!(workbuddy_release(&json.replace("download.codebuddy.cn", "example.com")).is_err());
        assert!(workbuddy_release(&json.replace("sha256hash", "hash")).is_err());
    }

    /// 设计内结局判据必须钉住本模块**实际产出**的错误字面量（前缀漂移会静默把
    /// 「待用户操作」漏进 fail → 每点一次建一条工单），且绝不误吞真故障。
    #[test]
    fn design_outcome_marks_match_real_messages_only() {
        // 真实产出的错误串（不是 MARKS 里抄的）必须命中
        assert!(is_design_outcome(&workbuddy_pending(Some("0.0.1"), "0.0.2").unwrap_err()));
        assert!(is_design_outcome("沙箱禁止启动 WorkBuddy 系统安装器"));
        assert!(is_design_outcome("豆包桌面版请到官网 https://www.doubao.com/download/desktop 下载安装（下载直链尚未核实，U-King 不代装）；检测到已装后可一键打开"));
        assert!(is_design_outcome("当前平台请到 Magpie 官方发行页下载并手动安装（装好后可一键打开）"));
        assert!(is_design_outcome("当前平台请到 WorkBuddy 官网下载并手动安装（检测到已装后可一键打开）"));
        // 真故障一律不命中（下载失败 / 摘要不匹配 / PE 核验不过）——这些要照旧建 issue
        // （字面量取自 official_curl / verify_bytes 的真实产出）
        assert!(!is_design_outcome("官方下载失败（curl 退出码 Some(22)），请稍后重试或使用官网入口"));
        assert!(!is_design_outcome("下载包 SHA-256 与官方更新信息不符，拒绝执行"));
        assert!(!is_design_outcome("下载内容不是有效的 Windows PE 程序，拒绝执行"));
    }
}
