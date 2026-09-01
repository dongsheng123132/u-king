//! 办公文档 → PDF 真版式渲染（借 LibreOffice headless）。
//!
//! ## 这个模块补的是哪一格
//!
//! 内置预览对办公文档是**分档**的：
//!   - `.docx` 走 mammoth、`.xlsx` 走 SheetJS —— 纯前端、秒开、能选中文字，够用；
//!   - `.pptx` 只能从 zip 里抽 `<a:t>` **出一份文字大纲**（纯前端没有成熟的 pptx 渲染方案）；
//!   - `.doc` / `.ppt` 这些**老二进制格式**根本不是 ZIP，一个字都解不出来。
//!
//! 我们自己生成的 PPT 已经有同源的 `.预览.html` 兜住了，但**客户拿来的那一份**没有 ——
//! 而「帮我看看这份标书 / 这个方案」恰恰是客户最常拿来的活。这一档就是给它的：
//! 机器上装了 LibreOffice 就转成 PDF 交给已有的 PdfViewer（真版式、真分页），
//! 没装就什么都不做、静静退回原来的大纲档。
//!
//! ## 为什么是"可选增强"而不是必需依赖
//!
//! LibreOffice 装包 ~350MB。为了预览一份 PPT 就强制客户装它，是拿客户的磁盘补我们的短板。
//! 所以：**探得到就用，探不到就当没有这个功能**，绝不弹窗劝装、绝不因此报错。
//! 厨具工具箱里本来就有 `libreoffice` 这一项，想要的人自己会去装。
//!
//! ## 独立可插拔
//!
//! 只暴露纯函数，`#[tauri::command]` 写在 `lib.rs`；只 import `installer` 这个公共层
//! （`search_paths`），不碰任何其它功能模块。删掉本模块只需动 `lib.rs` + 前端两处。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 值得走这条路的扩展名。
///
/// **故意不含 `docx`/`xlsx`/`xls`**：那三个前端已经渲染得很好，而且是秒开、可选中文字的；
/// 换成 PDF 图像反而是降级 —— 还慢十几秒。只接管我们确实做不好的那几个。
const CONVERTIBLE: &[&str] = &["pptx", "ppt", "doc", "odp", "odt"];

/// LibreOffice 冷启动 + 转换的死线。首次跑要初始化用户配置，实测比后续慢得多，
/// 所以给得比一般子进程宽。到点仍未完成就杀掉 —— 预览可以没有，界面不能卡死。
const TIMEOUT: Duration = Duration::from_secs(90);

pub fn is_convertible(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    CONVERTIBLE.contains(&ext.as_str())
}

/// 找 soffice 可执行文件。找不到返回 None —— 这不是错误，是"这台机器没装"。
pub fn soffice_path() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "soffice.exe" } else { "soffice" };

    // ① 我们自己的搜索路径 + 系统 PATH（复用 installer 的公共实现，不另写一份）
    let mut dirs = crate::installer::search_paths(None);
    let sep = if cfg!(windows) { ';' } else { ':' };
    if let Ok(path) = std::env::var("PATH") {
        dirs.extend(path.split(sep).filter(|s| !s.is_empty()).map(PathBuf::from));
    }

    // ② LibreOffice 的默认安装位置。**必须显式列**：Windows 版装完不进 PATH，
    //    只靠 PATH 探会得出"没装"的错误结论（客户明明装了，我们说没有）。
    //
    //    ★ **两份实现必须列同一批路径**（另一份在 `skills/pdf/scripts/to-pdf.mjs::findSoffice`，
    //    那是给 AI 调的导出技能，跟本模块的预览是两个消费方、两个运行时，没法共享代码）。
    //    路径列表一旦漂移，客户就会看到「预览说没装、导出说装了」这种自相矛盾 ——
    //    改这里**务必同步改那边**。（本行不是废话：两边一度就差了 `Programs\`，见下。）
    #[cfg(windows)]
    {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(base) = std::env::var(var) {
                dirs.push(PathBuf::from(&base).join("LibreOffice").join("program"));
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            // 按用户装（winget 的 per-user 档）落在 `%LOCALAPPDATA%\Programs\…`，
            // 这是 Windows 上 per-user 安装的通行约定。**两个都列**：
            // 只列其中一个，就会在另一种装法的机器上误报「没装」。
            dirs.push(PathBuf::from(&local).join("Programs").join("LibreOffice").join("program"));
            dirs.push(PathBuf::from(&local).join("LibreOffice").join("program"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/snap/bin"));
    }

    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

/// 缓存键：路径 + 修改时间 + 大小。文件改过就自动换一个键，不会拿旧 PDF 骗人。
fn cache_key(src: &Path) -> String {
    let meta = std::fs::metadata(src).ok();
    let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let seed = format!("{}|{len}|{mtime}", src.display());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
    for b in seed.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

fn cache_root() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking").join("cache").join("officepdf")
}

/// 把办公文档转成 PDF，返回 PDF 绝对路径。
///
/// - 已转过（且原件没变）直接复用缓存，不重复烧十几秒；
/// - 没装 LibreOffice / 格式不在名单里 → `Ok(None)`，**不是错误**，调用方安静退回原来的档；
/// - 真出错（超时、转换失败）才 `Err`。
pub fn to_pdf(src_path: &str) -> Result<Option<String>, String> {
    if !is_convertible(src_path) {
        return Ok(None);
    }
    let src = PathBuf::from(src_path);
    if !src.is_file() {
        return Err(format!("文件不存在：{src_path}"));
    }
    let Some(soffice) = soffice_path() else {
        return Ok(None); // 没装 = 没这个功能，不是故障
    };

    let out_dir = cache_root().join(cache_key(&src));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");
    let pdf = out_dir.join(format!("{stem}.pdf"));
    if pdf.is_file() && std::fs::metadata(&pdf).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(Some(pdf.display().to_string()));
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("建缓存目录失败: {e}"))?;

    // 独立的 UserInstallation 配置目录：**客户开着 LibreOffice 时，共用默认配置的 headless
    // 进程会直接拒绝启动**（"another instance is running"）。给它一个自己的配置目录就绕开了 ——
    // 否则这个功能在"正好开着 Office 的人"那里 100% 不工作，而那恰恰是会用 LibreOffice 的人。
    let profile = cache_root().join("_profile");
    let _ = std::fs::create_dir_all(&profile);
    let profile_url = format!(
        "-env:UserInstallation=file:///{}",
        profile.display().to_string().replace('\\', "/").replace(' ', "%20")
    );

    let mut cmd = std::process::Command::new(&soffice);
    cmd.arg(&profile_url)
        .arg("--headless")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&out_dir)
        .arg(&src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：GUI 下别闪黑窗
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动 LibreOffice 失败: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > TIMEOUT {
                    let _ = child.kill();
                    return Err("LibreOffice 转换超时（90s）".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(format!("等待 LibreOffice 失败: {e}")),
        }
    }

    // 只认"文件真的出来了且非空"。soffice 转换失败时**照样退出码 0**，
    // 拿退出码当判据会得到"转成功了但没有文件"这种最难查的结论。
    if pdf.is_file() && std::fs::metadata(&pdf).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(Some(pdf.display().to_string()));
    }
    // 少数版本会按内部标题命名，兜底扫一下目录里的 pdf
    if let Ok(rd) = std::fs::read_dir(&out_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("pdf")) == Some(true)
                && std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
            {
                return Ok(Some(p.display().to_string()));
            }
        }
    }
    Err("LibreOffice 没有产出 PDF（文件可能已加密或损坏）".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_formats_we_render_badly_go_through_libreoffice() {
        // 该接管的：pptx 只有文字大纲、老二进制格式一个字都解不出来
        assert!(is_convertible("a.pptx"));
        assert!(is_convertible("A.PPT"));
        assert!(is_convertible("合同.doc"));
        // 不该接管的：这三个前端已经渲染得又快又好，换成 PDF 是降级
        assert!(!is_convertible("a.docx"));
        assert!(!is_convertible("a.xlsx"));
        assert!(!is_convertible("a.xls"));
        assert!(!is_convertible("a.png"));
        assert!(!is_convertible("noext"));
    }

    #[test]
    fn cache_key_is_stable_and_path_sensitive() {
        // 同一个不存在的路径要稳定出同一个键（否则每次预览都重转）
        assert_eq!(cache_key(Path::new("C:/x/a.pptx")), cache_key(Path::new("C:/x/a.pptx")));
        assert_ne!(cache_key(Path::new("C:/x/a.pptx")), cache_key(Path::new("C:/x/b.pptx")));
    }

    #[test]
    fn unconvertible_returns_none_not_error() {
        // 「这个格式不归我管」和「出错了」必须分开 —— 前端靠这个区别决定要不要退回大纲档
        assert_eq!(to_pdf("a.docx").unwrap(), None);
    }
}
