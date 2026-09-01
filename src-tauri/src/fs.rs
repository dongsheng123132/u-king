//! 文件树 —— 工作台「文件」面板的后端。纯 std，不引 walkdir/ignore（守体积红线）。
//!
//! 单层懒加载：`list_dir(path)` 只读一层，前端点开目录再请求子层。目录在前、按名排序，
//! 过滤常见噪声目录（.git/node_modules/target），限条数防超大目录卡 UI。
//! `read_text_file` 给只读预览用，限大小防读进大二进制。

use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct DirEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
}

/* ---- 「成品」交付：打开 / 定位 ------------------------------------------------
 * 办公活（PPT/Word/Excel）做完了，右侧面板**渲染不了**这些格式，所以以前对话里
 * 一个按钮都没有 —— 文件躺在磁盘某处，客户不知道在哪、也打不开。这一对补的就是那一步。
 * ---------------------------------------------------------------------------- */

/// 允许交给系统默认程序打开的扩展名。**白名单，不是黑名单，这条不许改成黑名单。**
///
/// 因为这个路径是 **AI 说出来的**：模型完全可以在对话里声称「已生成 setup.exe」，
/// 客户点一下「打开」就等于替它执行了任意程序。黑名单永远漏
/// （.scr / .pif / .lnk / .hta / .msc / .reg / .vbs / .jar / .msi …），
/// 而白名单漏掉一个格式，最坏结果只是少一个按钮。
const OPENABLE_EXTS: &[&str] = &[
    // 办公三件套（这一批正是「强化办公能力」的成品形态）
    "docx", "doc", "pptx", "ppt", "xlsx", "xls", "csv", "rtf", "odt", "ods", "odp",
    // 文档 / 文本
    "pdf", "txt", "md", "json", "xml", "yaml", "yml", "log", "srt", "vtt",
    // 图像 / 音视频（右侧能预览的这里也允许「用本机程序打开」，客户常想用看图王打开）
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "svg", "ico", "mp4", "webm", "mov", "mp3", "wav",
    // 网页 + 打包件
    "html", "htm", "zip",
];

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// 这个路径存在吗（顺便回答它够不够格给一个「打开」按钮）。
///
/// 前端**渲染按钮之前**先问一次：AI 报的成品路径可能压根不存在（它说完就忘、或写错了目录）。
/// 「点了必失败的按钮比没有按钮更伤」是这个文件里既有的原则，这里只是把它落到办公产物上。
#[tauri::command]
pub fn produced_file_info(path: String) -> Result<serde_json::Value, String> {
    let p = Path::new(&path);
    let meta = std::fs::metadata(p).ok();
    let ext = ext_of(&path);
    Ok(serde_json::json!({
        // `exists` 只算**文件**（历史语义：调用方拿它决定要不要给「打开」按钮）。
        // 目录另给一个字段 —— 终端里的链接现在也认目录了（`demo\SUBMIT-xxx\` 这种），
        // 而目录在这里恒 `exists:false`，不单独说一声，调用方会把它当「找不到」。
        "exists": meta.as_ref().map(|m| m.is_file()).unwrap_or(false),
        "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
        "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
        "openable": OPENABLE_EXTS.contains(&ext.as_str()),
    }))
}

/// 用系统默认程序打开成品文件（.docx 交给 Word、.pptx 交给 PPT…）。
///
/// 直接 spawn argv、**不经 shell**：路径来自 AI，走 `cmd /c start` 就得自己处理引号和 `&`。
/// explorer/open 是 GUI 进程，不弹黑窗。
#[tauri::command]
pub fn open_produced_file(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_file() {
        return Err("这个文件不在了 —— AI 可能把路径说错了，或者它还没真正写出来".into());
    }
    let ext = ext_of(&path);
    if !OPENABLE_EXTS.contains(&ext.as_str()) {
        return Err(format!(
            "为安全起见不能直接打开 .{ext} 文件。点「在文件夹中显示」，确认清楚再自己打开"
        ));
    }
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(p).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    // ⚠️ Windows 的 explorer 打开文件成功时也常返回非 0 退出码，所以**只看 spawn 起没起来**，
    // 不去 wait 退出码 —— 拿退出码当判据会把成功报成失败。
    r.map(|_| ()).map_err(|e| format!("打开失败: {e}"))
}

/// 在资源管理器 / Finder 里**选中**这个文件（不是打开它所在的目录就完了）。
/// 不限扩展名：只是让人看见它在哪，不执行任何东西 —— 这正是遇到不可打开格式时的退路。
#[tauri::command]
pub fn reveal_produced_file(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err("这个文件不在了 —— AI 可能把路径说错了，或者它还没真正写出来".into());
    }
    #[cfg(windows)]
    let r = std::process::Command::new("explorer")
        .arg(format!("/select,{}", p.display()))
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").args(["-R", &p.display().to_string()]).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open")
        .arg(p.parent().unwrap_or(p))
        .spawn();
    r.map(|_| ()).map_err(|e| format!("定位失败: {e}"))
}

#[cfg(test)]
mod openable_tests {
    use super::*;

    /// 白名单绝不能放进可执行/脚本类 —— 那条路径是 AI 说出来的，放进去等于给它一条
    /// 「让客户点一下就替我执行」的通道。这条用例就是拿来挡「顺手加个 .exe 方便测试」的。
    #[test]
    fn never_openable_executables() {
        for bad in [
            "exe", "bat", "cmd", "com", "scr", "pif", "lnk", "msi", "msc", "hta", "reg", "vbs",
            "vbe", "js", "jse", "wsf", "ps1", "psm1", "sh", "jar", "app", "dll", "cpl", "url",
        ] {
            assert!(!OPENABLE_EXTS.contains(&bad), ".{bad} 不该在可直接打开的白名单里");
        }
    }

    /// 办公成品必须在白名单里 —— 少一个就等于那类活做完了客户还是打不开。
    #[test]
    fn office_products_are_openable() {
        for good in ["docx", "pptx", "xlsx", "pdf", "csv", "png", "mp4", "html"] {
            assert!(OPENABLE_EXTS.contains(&good), ".{good} 是成品格式，该能一键打开");
        }
    }

    /// 扩展名一律小写比较：AI 报 `报告.DOCX` 也得认（Windows 上大小写常见）。
    #[test]
    fn ext_is_case_insensitive() {
        assert_eq!(ext_of("D:/out/报告.DOCX"), "docx");
        assert_eq!(ext_of("D:/out/幻灯片.PptX"), "pptx");
        assert_eq!(ext_of("D:/out/no-ext"), "");
    }
}

/// 默认隐藏的噪声目录（前端可不展示）。文件树照常列出，但标记之。
fn is_noise(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | ".cache" | ".next" | "dist" | "__pycache__"
    )
}

/// 列一层目录。目录在前、各自按名（忽略大小写）排序，最多 2000 条。
#[tauri::command]
pub fn list_dir(path: String, show_noise: Option<bool>) -> Result<Vec<DirEntry>, String> {
    let p = Path::new(&path);
    if !p.is_dir() {
        return Err(format!("不是目录: {path}"));
    }
    let show_noise = show_noise.unwrap_or(false);
    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut files: Vec<DirEntry> = Vec::new();

    let rd = std::fs::read_dir(p).map_err(|e| format!("读取目录失败: {e}"))?;
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if !show_noise && is_noise(&name) {
            continue;
        }
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        let item = DirEntry {
            name,
            path: ent.path().to_string_lossy().to_string(),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
        };
        if is_dir {
            dirs.push(item);
        } else {
            files.push(item);
        }
        if dirs.len() + files.len() >= 2000 {
            break;
        }
    }

    let by_name = |a: &DirEntry, b: &DirEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    dirs.append(&mut files);
    Ok(dirs)
}

/// 读图片文件为 data URL（base64）—— 给「拖文件进来当参考图」用：Tauri 原生拖放给的是**路径**，
/// 而作图/视频要的是 data URL。mime 从扩展名推，限图片类型 + 20MB。纯 std 自带极小 base64（守体积）。
#[tauri::command]
pub fn read_file_data_url(path: String) -> Result<String, String> {
    let p = Path::new(&path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => return Err("只支持图片文件（png/jpg/jpeg/webp/gif/bmp）".into()),
    };
    let bytes = std::fs::read(&path).map_err(|e| format!("读取失败: {e}"))?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("图片过大（>20MB）".into());
    }
    Ok(format!("data:{mime};base64,{}", b64_encode(&bytes)))
}

/// 极小标准 base64 编码（带 padding）。纯 std，避免为一个功能引 crate。
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

/// 把剪贴板/内存里的图片字节存成临时文件，返回绝对路径 —— 给「终端粘贴图片」用：
/// 终端是纯文本流，Claude Code / Codex 只能读**文件路径**，所以把粘贴的图片先落盘，再把路径贴进终端
/// （与「拖文件进终端」殊途同归）。落在 系统临时目录/uking-paste/，顺手清掉 1 天前的旧文件防堆积。限 30MB。
#[tauri::command]
pub fn save_pasted_image(bytes: Vec<u8>, ext: Option<String>) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("空图片".into());
    }
    if bytes.len() > 30 * 1024 * 1024 {
        return Err("图片过大（>30MB）".into());
    }
    // 扩展名只留字母数字、限 5 字符，兜底 png（防路径注入/怪扩展名）。
    let ext: String = ext
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(5)
        .collect();
    let ext = if ext.is_empty() { "png".to_string() } else { ext };

    let dir = std::env::temp_dir().join("uking-paste");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建临时目录失败: {e}"))?;
    // 尽力清理 1 天前旧文件（失败忽略，不阻塞粘贴）。
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let day = std::time::Duration::from_secs(24 * 3600);
        for e in rd.flatten() {
            if let Ok(modt) = e.metadata().and_then(|m| m.modified()) {
                if modt.elapsed().map(|d| d > day).unwrap_or(false) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("paste-{}-{}.{}", stamp, std::process::id(), ext));
    std::fs::write(&path, &bytes).map_err(|e| format!("写图片失败: {e}"))?;
    Ok(path.display().to_string())
}

/// 读文本文件（只读预览）。限 max_bytes（默认 256KB），超出截断；疑似二进制（含 NUL）拒读。
#[tauri::command]
pub fn read_text_file(path: String, max_bytes: Option<usize>) -> Result<String, String> {
    let limit = max_bytes.unwrap_or(256 * 1024);
    let bytes = std::fs::read(&path).map_err(|e| format!("读取失败: {e}"))?;
    let truncated = bytes.len() > limit;
    let slice = &bytes[..bytes.len().min(limit)];
    // 含 NUL 视为二进制，不预览
    if slice.contains(&0) {
        return Err("二进制文件，不支持预览".into());
    }
    let mut s = String::from_utf8_lossy(slice).to_string();
    if truncated {
        s.push_str("\n\n…（文件过大，仅显示前 256KB）");
    }
    Ok(s)
}
