//! DeepSeek Harness —— Windows 官方桌面版：装、检测、启动、写底层模型配置。
//!
//! **为什么从「npm 装 `@deepseek-ai/dsh` + `dsh web` iframe」改走官方桌面版（Windows only，
//! 2026-09-25）**：npm 那条路要装 Node 环境、跑 `dsh web` 起本地 server、再用 iframe 嵌一个网页
//! 工作台——环节多、任何一环（Node 版本、端口占用、iframe 兼容）都能让「装了却打不开」发生。
//! DeepSeek 官方已经发了 Windows 桌面版（Electron，NSIS 打包），装完是一个独立窗口程序，
//! 不需要我们管 Node/端口/iframe。**Mac/Linux 保持现状不动**（仍走清单里的 npm 路线）——
//! 官方桌面版目前只发了 Windows 包。
//!
//! **可插拔模块**（遵本项目「模块独立」铁律，参照 `uuswitch.rs` 的写法）：只暴露纯函数，
//! `#[tauri::command]` 一律写在 `lib.rs` 转调；进度用 `|msg|` 回调传出，`lib.rs` 再 `emit`。
//! 删掉本模块要动：`lib.rs`（去 `mod dshdesk` + 1 个 command + generate_handler 1 行）、
//! `installer.rs`（去 `install_tool` 开头那段 dsh 特判）、`tools.rs`（TOOL_SPECS/list_tools/
//! launch_app_inner 里 dsh 的 Windows 分支改回 npm 语义）、`providers.rs`（`dsh_installed()`
//! 判据改回单纯 CLI 探测——这几处不是本模块自己的代码，是别处对本模块的引用点）。
//!
//! ## 实测事实（本机 2026-09-25，Windows，直连国内网络）
//! - 官方更新源（腾讯 COS，国内直连可达）：
//!   `https://download.deepseek.com/dsh-desk/feeds/win-x64/nightly.yml`，YAML 形如：
//!   ```yaml
//!   version: 0.1.7-rc.2
//!   files:
//!     - url: https://download.deepseek.com/dsh-desk/bin/win-x64/deepseek-harness-0.1.7-rc.2-win-x64.exe
//!       sha512: <base64>
//!       size: 288245480
//!   path: https://download.deepseek.com/dsh-desk/bin/win-x64/deepseek-harness-0.1.7-rc.2-win-x64.exe
//!   sha512: <base64（与 files[0] 相同）>
//!   releaseDate: '2026-09-24T14:11:01.715Z'
//!   ```
//!   取 `files[0]` 的 `url`/`sha512`/`size`，缺了回退顶层 `path`/`sha512`/`size`。
//!   `url` 必须落在 `https://download.deepseek.com/` 之下，否则拒绝（防 feed 被劫持后改指向别处）。
//! - 安装包是 electron-builder 出的 NSIS，签名方 Hangzhou DeepSeek。`<exe> /S /currentuser`
//!   **静默安装可用**：免管理员、不改 PATH、会建桌面 + 开始菜单快捷方式。本机实测装了 10 分钟以上
//!   （约 1.1 万文件，体积和 LibreOffice 级别接近），所以等待要给足（本模块给 30 分钟），
//!   且期间持续报进度（每 30 秒一次），否则界面会像卡死。
//! - 装完后 `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\1bf39983-50d0-5fe0-9ef4-cece76f67c5e`
//!   下有 `InstallLocation`（安装目录，取其 + `DeepSeek Harness.exe`，注意文件名带空格）、
//!   `DisplayVersion`、`QuietUninstallString`（静默卸载命令，未来若要做卸载支持可以用它，
//!   本轮任务不含卸载能力，没实现）。默认安装目录本机未单独验证过，兜底候选见 [`find_exe`]。
//! - 桌面版和 CLI 共用同一份 `$DSH_HOME`（默认 `~/.dsh`，尊重 `DSH_HOME` 环境变量）下的
//!   `settings.yaml` / `.credentials.yaml`——`providers.rs` 的 `apply_dsh`/`reset_dsh` 不用改
//!   写入目标，继续对两种形态都生效。
//!
//! ## 本模块不管什么
//! 不碰 `settings.yaml`/`.credentials.yaml`（那是 `providers.rs` 的地盘，单一真相源在那边）；
//! 不做卸载/清理（`cleanup.rs` 的「npm 全局包 dsh」探测是查 CLI 残留，跟桌面版是两件事，
//! 本轮没有要求做桌面版的卸载支持）。

use serde::Deserialize;
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

/// 官方 Windows 更新源（nightly channel；DeepSeek 目前只发这一条 win-x64 feed）。
const FEED_URL: &str = "https://download.deepseek.com/dsh-desk/feeds/win-x64/nightly.yml";

/// 下载地址必须落在这个前缀下，否则拒绝——防 feed 被中间人/DNS 劫持后指向别处的可执行文件。
const TRUSTED_URL_PREFIX: &str = "https://download.deepseek.com/";

/// 实测的卸载注册表键（HKCU，免管理员）。GUID 是 electron-builder 按 app id 派生的固定值，
/// 不是每次安装随机生成的——本机反复重装验证过同一个键。
#[cfg(windows)]
const UNINSTALL_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\1bf39983-50d0-5fe0-9ef4-cece76f67c5e";

/// 主程序文件名（带空格，是官方安装器实际落的名字，不是我们起的）。
#[cfg(windows)]
const EXE_NAME: &str = "DeepSeek Harness.exe";

/// 一条可安装的发行信息，从更新源解析而来。
#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// 官方 feed 给的 sha512，base64 编码（不是十六进制）。
    pub sha512_b64: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct FeedFile {
    url: Option<String>,
    sha512: Option<String>,
    size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Feed {
    version: Option<String>,
    files: Option<Vec<FeedFile>>,
    path: Option<String>,
    sha512: Option<String>,
    size: Option<u64>,
}

/// 纯函数：把已经拿到手的 feed YAML 文本解析成 [`Release`]。拆出来单独测，不用真的发网络请求。
fn parse_feed(text: &str) -> Result<Release, String> {
    let feed: Feed = serde_yaml::from_str(text)
        .map_err(|e| format!("DeepSeek Harness 版本清单不是合法 YAML: {e}"))?;
    let version = feed.version.unwrap_or_default().trim().to_string();
    if version.is_empty() {
        return Err("DeepSeek Harness 版本清单缺少 version 字段".into());
    }
    let first = feed.files.as_ref().and_then(|f| f.first());
    let url = first
        .and_then(|f| f.url.clone())
        .or(feed.path)
        .ok_or("DeepSeek Harness 版本清单缺少下载地址")?
        .trim()
        .to_string();
    let sha512_b64 = first
        .and_then(|f| f.sha512.clone())
        .or(feed.sha512)
        .ok_or("DeepSeek Harness 版本清单缺少 sha512 校验值")?
        .trim()
        .to_string();
    let size = first
        .and_then(|f| f.size)
        .or(feed.size)
        .ok_or("DeepSeek Harness 版本清单缺少文件大小")?;
    if !url.starts_with(TRUSTED_URL_PREFIX) {
        return Err(format!(
            "DeepSeek Harness 下载地址不在受信任域名下，已拒绝安装：{url}"
        ));
    }
    Ok(Release { version, url, sha512_b64, size })
}

/// 拉取并解析当前最新发行信息。跨平台可编译（`installer::curl` 本身跨平台），
/// 但目前只有 Windows 的 [`install`] 会调用它。
pub fn latest() -> Result<Release, String> {
    let text = crate::installer::curl(&[
        "-fsSL",
        "-m",
        "20",
        "-A",
        "Mozilla/5.0 U-King",
        FEED_URL,
    ])
    .map_err(|e| format!("获取 DeepSeek Harness 版本信息失败: {e}"))?;
    parse_feed(&text)
}

/// 在已知落点找桌面版 exe（检测「已装」与「启动」共用）。
#[cfg(windows)]
pub fn find_exe() -> Option<PathBuf> {
    // ① 注册表 InstallLocation——最准，直接来自安装器实际写的路径。
    if let Some(dir) = crate::installer::reg_query(UNINSTALL_KEY, "InstallLocation") {
        let p = PathBuf::from(dir.trim()).join(EXE_NAME);
        if p.exists() {
            return Some(p);
        }
    }
    // ② 兜底候选（注册表没读到，或客户机上落点跟本机实测的不一样）。
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    for root in [Path::new(&local).join("Programs"), PathBuf::from(&pf)] {
        let p = root.join("DeepSeek Harness").join(EXE_NAME);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 已安装的 `DisplayVersion`（注册表读不到就 `None`，不是硬错误）。
#[cfg(windows)]
pub fn display_version() -> Option<String> {
    crate::installer::reg_query(UNINSTALL_KEY, "DisplayVersion")
}

/// DeepSeek Harness 桌面版是否已装。非 Windows 恒 `false`（官方目前只发 Windows 包）。
pub fn installed() -> bool {
    #[cfg(windows)]
    {
        find_exe().is_some()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 极小标准 base64 编码（带 padding）。纯 std，不跨模块复用别处的实现——「叶子工具不跨模块
/// 耦合」是本项目既有约定（见 `fs.rs`/`draw.rs`/`lib.rs` 里同款各自一份的小实现）。
fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// 流式算文件的 SHA-512，编成 base64（跟官方 feed 的 `sha512` 字段同一编码，可直接比较）。
/// 流式读（64KB 缓冲）不是一次性 `fs::read`——安装包接近 300MB，别整个吞进内存。
/// 只有 Windows 的 [`install`] 会调用它——`#[cfg(windows)]`，否则非 Windows 编译会报
/// dead_code（那边没有安装流程会去下载文件校验）。
#[cfg(windows)]
fn sha512_base64(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha512};
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("读取下载文件失败: {e}"))?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取下载文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(b64_encode(&hasher.finalize()))
}

/// 下载并**静默安装** DeepSeek Harness 桌面版（NSIS `/S /currentuser`，免管理员，约 280MB）。
/// 装完不碰任何模型配置——那是 `providers.rs::apply_dsh` 的活，由调用方（`installer.rs`）
/// 在「用户主动配置虾盘云」时另外触发，跟「装没装」分开。
///
/// 流程：幂等检查 → 磁盘预检 → 拉取版本信息 → 下载到临时目录 → 校验 size + sha512 →
/// `/S /currentuser` 静默装（最多等 30 分钟，期间每 30 秒报一次进度）→ 轮询确认装上了 →
/// 静默失败（被拦/取消/杀软）则回退拉起可视安装界面。
#[cfg(windows)]
pub fn install(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if installed() {
        on_progress("检测到 DeepSeek Harness 已安装，跳过重复安装。");
        return Ok("DeepSeek Harness 已安装（跳过重复安装）。".into());
    }

    // 磁盘预检：安装包本身约 280MB，解出约 1.1 万个文件后实际占用远超安装包体积。
    // 2000MB 是留足余量的保守线，不是精确算出来的下限——探测失败（None）不阻塞安装。
    if let Some(free_mb) = crate::installer::temp_disk_free_mb() {
        if free_mb < 2000 {
            return Err(format!(
                "系统盘空间不足：仅剩 {free_mb} MB，安装 DeepSeek Harness 建议至少预留 2000 MB。\
                 请清理磁盘（删除大文件/清空回收站）后重试。"
            ));
        }
    }

    on_progress("正在获取 DeepSeek Harness 最新版本信息…");
    let release = latest()?;
    on_progress(&format!(
        "开始下载 DeepSeek Harness {}（约 {} MB，网络好几分钟内能完成）…",
        release.version,
        release.size / 1024 / 1024
    ));

    let tmp = std::env::temp_dir().join(format!(
        "DeepSeek-Harness-Setup-uking-{}.exe",
        release.version
    ));
    let _ = std::fs::remove_file(&tmp);

    let status = std::process::Command::new(crate::installer::system_tool("curl"))
        .args([
            "-fsSL",
            "--retry",
            "2",
            "--retry-delay",
            "2",
            "-A",
            "Mozilla/5.0 U-King",
            "-m",
            "1800",
            "-o",
            &tmp.to_string_lossy(),
            &release.url,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let dl_ok = matches!(status, Ok(s) if s.success());
    let sz = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if !dl_ok {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "DeepSeek Harness 安装包下载失败（网络/代理/杀软所致，已下 {sz} 字节）。\
             请稍后重试，或到 DeepSeek 官网手动下载安装。"
        ));
    }
    if sz != release.size {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "DeepSeek Harness 安装包大小不符（期望 {} 字节，实际 {sz} 字节，\
             可能被杀软/代理截断）。请重试。",
            release.size
        ));
    }

    on_progress("下载完成，正在校验完整性…");
    let digest = sha512_base64(&tmp)?;
    if digest.trim() != release.sha512_b64.trim() {
        let _ = std::fs::remove_file(&tmp);
        return Err(
            "DeepSeek Harness 安装包校验失败（SHA-512 不匹配，可能被篡改或下载不完整）。请重试。"
                .into(),
        );
    }

    on_progress("校验通过，正在安装 DeepSeek Harness（首次安装文件较多，可能需要十几分钟）…");

    let visible_fallback = |reason: String| -> String {
        on_progress("静默安装未成，已打开安装界面，按提示点「下一步」即可…");
        let _ = std::process::Command::new(&tmp)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        format!("已打开 DeepSeek Harness 安装程序（{reason}），按提示装完即可。")
    };

    let spawned = std::process::Command::new(&tmp)
        .args(["/S", "/currentuser"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return Ok(visible_fallback(e.to_string())),
    };

    // 静默装等待时序：轮询 `try_wait`（不用阻塞的 `.status()`）才能在等待期间插空报进度。
    // 实测装了 10 分钟以上（约 1.1 万文件），30 分钟上限留足余量；每 30 秒报一次，
    // 不然界面在这十几分钟里看起来像卡死。
    let start = std::time::Instant::now();
    let max_wait = std::time::Duration::from_secs(30 * 60);
    let mut last_report = std::time::Instant::now();
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(e) => {
                crate::ulog::write("dshdesk", &format!("等待安装进程失败: {e}"));
                break None;
            }
        }
        if start.elapsed() > max_wait {
            let _ = child.kill();
            let _ = std::fs::remove_file(&tmp);
            return Err(
                "DeepSeek Harness 安装超过 30 分钟未完成，已放弃（可能是杀软拦截或磁盘很慢）。\
                 请稍后重试，或到官网手动安装。"
                    .into(),
            );
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        if last_report.elapsed() >= std::time::Duration::from_secs(30) {
            let mins = start.elapsed().as_secs() / 60;
            on_progress(&format!(
                "仍在安装 DeepSeek Harness，已 {mins} 分钟…（首次安装文件较多，请耐心等待）"
            ));
            last_report = std::time::Instant::now();
        }
    };

    match exit_status {
        Some(s) if s.success() => {
            // 装完轮询确认，最多约 10 秒（跟 uuswitch.rs 的口径一致）。
            for _ in 0..20 {
                if installed() {
                    let _ = std::fs::remove_file(&tmp);
                    return Ok(format!("DeepSeek Harness {} 已安装完成。", release.version));
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            let _ = std::fs::remove_file(&tmp);
            Ok(format!(
                "DeepSeek Harness {} 安装已完成（若列表未刷新，稍等片刻）。",
                release.version
            ))
        }
        Some(s) => Ok(visible_fallback(format!("安装码 {}", s.code().unwrap_or(-1)))),
        None => Ok(visible_fallback("安装进程状态未知".to_string())),
    }
}

#[cfg(not(windows))]
pub fn install(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("DeepSeek Harness 桌面版目前只提供 Windows 安装包，请使用命令行版".into())
}

/// 启动已装的桌面版（GUI 应用，找安装位置直接拉起，不进终端）。
#[cfg(windows)]
pub fn launch() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = find_exe().ok_or("未找到 DeepSeek Harness 桌面版（请先安装）")?;
    std::process::Command::new(&exe)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动 DeepSeek Harness 失败: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn launch() -> Result<(), String> {
    Err("当前平台不支持 DeepSeek Harness 桌面版，请使用命令行版".into())
}

#[cfg(test)]
mod feed_tests {
    use super::*;

    // 🔴 别用反斜杠续行来「缩进对齐」这段字面量——Rust 的 `\` + 换行会把下一行开头的空白
    // 一并吃掉，YAML 的缩进语义（`>-` 折叠标量靠缩进定界）会被悄悄抹平，解析当场报错。
    // 这里改用裸字符串字面量，YAML 该怎么缩进就怎么写，不迁就 Rust 源码的视觉对齐。
    const SAMPLE_FEED: &str = r#"version: 0.1.7-rc.2
files:
  - url: >-
      https://download.deepseek.com/dsh-desk/bin/win-x64/deepseek-harness-0.1.7-rc.2-win-x64.exe
    sha512: >-
      AY7f45dYQpz3o5nJx1v4y6c0z2b4n6q8s0u2w4y6a8c0e2g4i6k8m0o2q4s6u8w0y2a4c6e8g0i2k4m6o8q0s2u4w6y8a0
    size: 288245480
path: >-
  https://download.deepseek.com/dsh-desk/bin/win-x64/deepseek-harness-0.1.7-rc.2-win-x64.exe
sha512: >-
  AY7f45dYQpz3o5nJx1v4y6c0z2b4n6q8s0u2w4y6a8c0e2g4i6k8m0o2q4s6u8w0y2a4c6e8g0i2k4m6o8q0s2u4w6y8a0
releaseDate: '2026-09-24T14:11:01.715Z'
"#;

    /// 官方 feed 的真实形状：优先取 `files[0]`，跟顶层重复字段一致时也不出错。
    #[test]
    fn parses_files_entry() {
        let r = parse_feed(SAMPLE_FEED).expect("应能解析出真实 feed 形状");
        assert_eq!(r.version, "0.1.7-rc.2");
        assert_eq!(
            r.url,
            "https://download.deepseek.com/dsh-desk/bin/win-x64/deepseek-harness-0.1.7-rc.2-win-x64.exe"
        );
        assert_eq!(r.size, 288245480);
        assert!(r.sha512_b64.starts_with("AY7f45dY"));
    }

    /// `files` 缺失时回退顶层 `path`/`sha512`/`size`。
    #[test]
    fn falls_back_to_top_level_fields() {
        let text = "version: 9.9.9\npath: https://download.deepseek.com/x.exe\nsha512: AAAA==\nsize: 123\n";
        let r = parse_feed(text).unwrap();
        assert_eq!(r.url, "https://download.deepseek.com/x.exe");
        assert_eq!(r.sha512_b64, "AAAA==");
        assert_eq!(r.size, 123);
    }

    /// 下载地址不在受信任域名下——拒绝，防 feed 被劫持后指向任意可执行文件。
    #[test]
    fn rejects_untrusted_domain() {
        let text = "version: 1.0.0\npath: https://evil.example.com/x.exe\nsha512: AAAA==\nsize: 1\n";
        let err = parse_feed(text).unwrap_err();
        assert!(err.contains("不在受信任域名下"), "应明确拒绝: {err}");
    }

    /// 缺 version / 缺下载地址 / 缺 sha512 / 缺 size 都要拒绝，不能悄悄用空值继续。
    #[test]
    fn rejects_missing_required_fields() {
        assert!(parse_feed("files: []\n").is_err(), "缺 version 应拒绝");
        assert!(
            parse_feed("version: 1.0.0\nsha512: AAAA==\nsize: 1\n").is_err(),
            "缺下载地址应拒绝"
        );
        assert!(
            parse_feed("version: 1.0.0\npath: https://download.deepseek.com/x.exe\nsize: 1\n")
                .is_err(),
            "缺 sha512 应拒绝"
        );
        assert!(
            parse_feed(
                "version: 1.0.0\npath: https://download.deepseek.com/x.exe\nsha512: AAAA==\n"
            )
            .is_err(),
            "缺 size 应拒绝"
        );
    }

    /// base64 编码器对齐标准测试向量（RFC 4648），确保跟官方 sha512 字段的编码一致可比。
    #[test]
    fn b64_encode_matches_known_vectors() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }
}
