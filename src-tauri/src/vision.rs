//! 图片预处理动作：图片只交给视觉模型，主对话模型只收到文字结果。
//!
//! 这不是另写一套识图实现，而是应用内复用 `uking-vision` 的 `see-image.mjs`。
//! 因此 GUI、Action CLI 和 MCP 的模型链、纯文本模型闸门、失败回退完全一致。

use serde::Serialize;
use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 20 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(180);
/// 附件暂存目录（系统临时目录下）。粘贴图片的 `uking-paste` 由 `fs::save_pasted_image` 管，这里只认它是「自己人」。
const STAGE_DIR: &str = "uking-attach";
const PASTE_DIR: &str = "uking-paste";
/// 复制一张 ≤20MB 的图正常是毫秒级；超过这个数基本是 OneDrive「仅在线」文件在现下载。
const STAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// 暂存副本保留多久。对话里一张图从拖入到发送不会隔几天，3 天足够宽松又不堆积。
const STAGE_KEEP: Duration = Duration::from_secs(3 * 24 * 3600);
static STAGE_SEQ: AtomicU64 = AtomicU64::new(0);
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "heic", "heif"];

#[derive(Debug, Clone, Serialize)]
pub struct VisionResult {
    pub ok: bool,
    pub text: String,
    pub model: String,
    pub mode: String,
    pub elapsed: String,
    pub source: String,
    pub cached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_from: Option<String>,
}

#[derive(Clone)]
struct CachedResult { fingerprint: String, result: VisionResult }
static REQUEST_CACHE: OnceLock<Mutex<HashMap<String, CachedResult>>> = OnceLock::new();

/// 仅按扩展名作 UI 分流；真正的文件类型、大小和路径验证在 [`describe`] 内完成。
pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| IMAGE_EXTS.iter().any(|e| s.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

fn checked_image(path: &str) -> Result<PathBuf, String> {
    if !is_image_path(path) {
        return Err("只支持 PNG/JPG/WEBP/GIF/BMP/HEIC 图片；普通文件会按原样交给对话。".into());
    }
    let p = Path::new(path).canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            // 客户机实测：从截图工具窗口直接拖出的图是临时文件，过一阵会被系统清掉。
            // 不拼系统原文 —— 它跟着系统语言走（法语系统上是法语），既看不懂也没法归类；
            // `image_missing:` 前缀交给 actions::ERR_RULES 归成「用户侧、不可重试」。
            "image_missing: 图片文件已经不在原来的位置了（被移走、删除，或是截图工具自动清理掉的临时文件），请重新拖入这张图。".to_string()
        } else {
            format!("读取图片路径失败: {e}")
        }
    })?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("读取图片属性失败: {e}"))?;
    if !meta.is_file() {
        return Err("图片路径必须是一个普通文件。".into());
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("图片超过 20MB（当前 {}MB），请先压缩后再发。", meta.len() / 1024 / 1024));
    }
    Ok(p)
}

// ───────────────────────── 附件暂存 ─────────────────────────
//
// 拖入对话的图片原先只记下拖放时的路径，到发送那一刻才去读。客户机实测：从截图工具窗口直接
// 拖出来的是临时文件，用过几次后被系统清掉，此后每次发送都「找不到文件」，这一轮永远发不出去。
// 所以图片一拖入就复制一份到我们自己的目录，后面识图读的是副本，原文件之后怎么样都不相干。

/// `p`（须是已 canonicalize 的路径）是否落在 `base` 之下。
/// Windows 上 canonicalize 带 `\\?\` 前缀，所以 `base` 也要 canonicalize 才可比；`base` 不存在就当不在其下。
fn is_under(base: &Path, p: &Path) -> bool {
    base.canonicalize().map(|b| p.starts_with(b)).unwrap_or(false)
}

/// 把拖入/选择的图片复制成一份暂存副本，返回副本的路径（之后识图、展示都用它）。
/// 已经是我们自己的副本（暂存目录或粘贴图片目录里）就原样返回，不重复复制。
pub fn stage_image(path: &str) -> Result<String, String> {
    stage_image_in(&std::env::temp_dir().join(STAGE_DIR), path)
}

/// [`stage_image`] 的可测试内核：暂存根目录由参数给，测试传自建的临时目录
/// （不用环境变量覆盖 —— cargo test 并行跑，环境变量是进程级的会串）。
fn stage_image_in(root: &Path, path: &str) -> Result<String, String> {
    // 复用识图前的同一套校验：扩展名 / 存在 / 普通文件 / ≤20MB。
    let src = checked_image(path)?;
    if is_under(root, &src) || is_under(&std::env::temp_dir().join(PASTE_DIR), &src) {
        return Ok(path.to_string());
    }
    // 保留原文件名：界面标签和识图结果的 source 都靠它。
    let name: OsString = Path::new(path)
        .file_name()
        .or_else(|| src.file_name())
        .map(OsStr::to_os_string)
        .ok_or_else(|| "读取图片文件名失败".to_string())?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let seq = STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
    // 每张图独占一个子目录：不同来源的同名文件（常见的 image.png）不会互相覆盖。
    let dir = root.join(format!("{stamp}-{}-{seq}", std::process::id()));
    let dest = dir.join(&name);

    // 复制放线程里、这边带超时等：OneDrive「仅在线」文件一复制就会触发下载，能卡很久，
    // 不能让它把调用方（进而把界面里的发送按钮）一直拖着。
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let worker = {
        let (dir, root) = (dir.clone(), root.to_path_buf());
        thread::Builder::new().name("uking-stage-image".into()).spawn(move || {
            let r = copy_into(&src, &dir, &name);
            if tx.send(r).is_err() {
                // 调用方已经超时走人了，这份迟到的副本没人会用。
                let _ = std::fs::remove_dir_all(&dir);
            }
            prune_stale(&root, STAGE_KEEP);
        })
    };
    if let Err(e) = worker {
        return Err(format!("启动复制图片任务失败: {e}"));
    }
    match rx.recv_timeout(STAGE_TIMEOUT) {
        Ok(Ok(())) => Ok(dest.display().to_string()),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err("复制图片超过 60 秒还没完成（如果是 OneDrive「仅在线」文件，先在资源管理器里右键「始终保留在此设备上」再拖）。".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err("复制图片的任务意外中断，请重新拖入这张图。".into())
        }
    }
}

/// 复制到 `dir/name`；任何一步失败都尽力把整个唯一子目录删掉，不留半截文件。
fn copy_into(src: &Path, dir: &Path, name: &OsStr) -> Result<(), String> {
    let r = copy_verified(src, dir, name);
    if r.is_err() {
        let _ = std::fs::remove_dir_all(dir);
    }
    r
}

fn copy_verified(src: &Path, dir: &Path, name: &OsStr) -> Result<(), String> {
    let want = std::fs::metadata(src).map_err(|e| format!("读取图片属性失败: {e}"))?.len();
    std::fs::create_dir_all(dir).map_err(|e| format!("建暂存目录失败: {e}"))?;
    // 先写 `.part` 再 rename：正式文件名一旦出现，内容就一定是完整的。
    let mut part_name = OsString::from(".");
    part_name.push(name);
    part_name.push(".part");
    let part = dir.join(part_name);
    let copied = std::fs::copy(src, &part).map_err(|e| format!("复制图片失败: {e}"))?;
    let on_disk = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if copied != want || on_disk != want {
        return Err(format!("复制图片不完整（应为 {want} 字节，实际 {on_disk} 字节），请重新拖入这张图。"));
    }
    std::fs::rename(&part, dir.join(name)).map_err(|e| format!("保存图片副本失败: {e}"))
}

/// 尽力清掉 `root` 下修改时间超过 `max_age` 的暂存子目录；失败一律忽略，不阻塞任何人。
fn prune_stale(root: &Path, max_age: Duration) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let old = e
            .metadata()
            .ok()
            .filter(|m| m.is_dir())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d >= max_age)
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// 前端「拖入/粘贴图片」调用：把图片暂存成副本并返回副本路径。
/// 这是界面附件管道，不是业务动作，所以不进 ActionSpec 动作表。
///
/// **必须 async + spawn_blocking**：复制可能很慢（见 [`stage_image_in`]），同步 command 会跑在主线程上冻住界面
/// （同 lib.rs 里 `list_tools` 那段注释的道理）。
#[tauri::command]
pub async fn stage_image_attachment(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || stage_image(&path))
        .await
        .map_err(|e| format!("暂存图片任务失败: {e}"))?
}

fn temp_script_path() -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    std::env::temp_dir().join(format!("uking-see-image-{}-{nonce}.mjs", std::process::id()))
}

fn trim_error(raw: &str) -> String {
    let text = raw.trim();
    let tail = if text.len() > 700 { &text[text.len() - 700..] } else { text };
    // 防止上游在诊断里把 Bearer/设备 key 回显给 UI、日志或 MCP 调用者。
    tail.split_whitespace()
        .map(|part| if part.starts_with("sk-") { "[已隐藏访问密钥]" } else { part })
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect(mut pipe: impl Read + Send + 'static, into: Arc<Mutex<String>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = pipe.read_to_string(&mut buf);
        if let Ok(mut out) = into.lock() { *out = buf; }
    })
}

/// 唯一业务实现。调用方拿到的只有视觉模型生成的文字，绝不返回 data-url/base64 给主模型。
pub fn describe(path: &str, ask: Option<&str>, mode: Option<&str>, request_id: Option<&str>) -> Result<VisionResult, String> {
    let image = checked_image(path)?;
    let mode = mode.unwrap_or("describe");
    if !matches!(mode, "describe" | "ocr") {
        return Err("mode 只支持 describe 或 ocr。".into());
    }
    let question = ask.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
    let fingerprint = format!("{}:{}:{:?}:{mode}:{question}", image.display(), std::fs::metadata(&image).map(|m| m.len()).unwrap_or(0), std::fs::metadata(&image).and_then(|m| m.modified()).ok());
    let request_id = request_id.map(str::trim).filter(|s| !s.is_empty());
    if let Some(id) = request_id {
        let cache = REQUEST_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(hit) = cache.lock().ok().and_then(|m| m.get(id).cloned()) {
            if hit.fingerprint != fingerprint { return Err("conflict: 同一个 request_id 不能换图片或问题重放。".into()); }
            let mut result = hit.result;
            result.cached = true;
            return Ok(result);
        }
    }
    let node = crate::installer::find_node().ok_or_else(|| "没有找到 Node.js；请先在 U-King 中完成环境安装。".to_string())?;
    let script = temp_script_path();
    std::fs::write(&script, include_str!("../skills/vision/scripts/see-image.mjs"))
        .map_err(|e| format!("准备视觉脚本失败: {e}"))?;

    let mut cmd = Command::new(node);
    cmd.arg(&script).arg(&image).arg("--json");
    if mode == "ocr" { cmd.arg("--ocr"); }
    if !question.is_empty() {
        // 用户问题是给视觉模型的，不进入 shell；限长防止把整个会话又塞进一次视觉请求。
        cmd.arg("--ask").arg(question.chars().take(4000).collect::<String>());
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("启动视觉模型失败: {e}"))?;
    let stdout = Arc::new(Mutex::new(String::new()));
    let stderr = Arc::new(Mutex::new(String::new()));
    let out_h = child.stdout.take().map(|p| collect(p, stdout.clone()));
    let err_h = child.stderr.take().map(|p| collect(p, stderr.clone()));
    let started = Instant::now();
    let mut timeout = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(120)),
            Ok(None) => { let _ = child.kill(); timeout = true; break; }
            Err(e) => { let _ = child.kill(); let _ = std::fs::remove_file(&script); return Err(format!("等待视觉模型失败: {e}")); }
        }
    }
    let status = child.wait().ok();
    if let Some(h) = out_h { let _ = h.join(); }
    if let Some(h) = err_h { let _ = h.join(); }
    let _ = std::fs::remove_file(&script);
    if timeout { return Err("图片识别超过 180 秒，已停止；请重试或换一张更小的图片。".into()); }
    let err = stderr.lock().map(|s| s.clone()).unwrap_or_default();
    if !status.map(|s| s.success()).unwrap_or(false) {
        return Err(format!("图片识别失败：{}", trim_error(&err)));
    }
    let out = stdout.lock().map(|s| s.clone()).unwrap_or_default();
    let raw: serde_json::Value = serde_json::from_str(out.trim())
        .map_err(|_| format!("图片识别返回格式异常：{}", trim_error(&out)))?;
    let text = raw.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
    let result_model = raw.get("model").and_then(|v| v.as_str()).unwrap_or("").trim();
    if text.is_empty() || result_model.is_empty() {
        return Err("图片识别没有返回可用文字。".into());
    }
    let result = VisionResult {
        ok: true,
        text: text.into(),
        model: result_model.into(),
        mode: raw.get("mode").and_then(|v| v.as_str()).unwrap_or(mode).into(),
        elapsed: raw.get("elapsed").and_then(|v| v.as_str()).unwrap_or("").into(),
        source: image.file_name().and_then(|s| s.to_str()).unwrap_or("image").into(),
        cached: false,
        fallback_from: raw.get("fallback_from").and_then(|v| v.as_str()).map(str::to_string),
    };
    if let Some(id) = request_id {
        if let Ok(mut cache) = REQUEST_CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
            cache.insert(id.to_string(), CachedResult { fingerprint, result: result.clone() });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_extensions_are_case_insensitive_and_narrow() {
        assert!(is_image_path(r"C:\\a b\\SCREENSHOT.PNG"));
        assert!(is_image_path("photo.heic"));
        assert!(!is_image_path("report.pdf"));
        assert!(!is_image_path("image.png.exe"));
    }

    #[test]
    fn error_redacts_key_like_tokens() {
        assert!(!trim_error("bad sk-secret-value").contains("sk-secret-value"));
    }

    /// 测试用的唯一临时目录，Drop 时清理（断言失败也会清）。
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let seq = STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("uking-vision-test-{tag}-{}-{nonce}-{seq}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("建临时目录失败 {}: {e}", dir.display()));
            Scratch(dir)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    const FAKE_PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-image-bytes";

    fn subdirs(root: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(root)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn stage_copies_keeps_name_and_survives_source_removal() {
        let tmp = Scratch::new("copy");
        let src_dir = tmp.path().join("src");
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&src_dir).unwrap();
        let name = "Capture d'écran 2026-10-03 175508.png";
        let src = src_dir.join(name);
        std::fs::write(&src, FAKE_PNG).unwrap();

        let staged = stage_image_in(&root, src.to_str().unwrap()).unwrap();
        assert!(!staged.starts_with(r"\\?\"), "返回的不应是 \\\\?\\ 形式：{staged}");
        assert_eq!(Path::new(&staged).file_name().and_then(|s| s.to_str()), Some(name));
        assert!(Path::new(&staged).starts_with(&root));
        assert_eq!(std::fs::read(&staged).unwrap(), FAKE_PNG);
        // 正式名出现时 .part 一定已经 rename 掉了。
        let dir = Path::new(&staged).parent().unwrap();
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1, "暂存目录里不该留 .part");

        // 这就是这次要修的场景：源文件被系统清掉之后，副本照样能过识图前的校验。
        std::fs::remove_file(&src).unwrap();
        assert!(checked_image(path_str(&src)).unwrap_err().starts_with("image_missing:"));
        assert!(checked_image(&staged).is_ok());
    }

    fn path_str(p: &Path) -> &str { p.to_str().unwrap() }

    #[test]
    fn stage_missing_source_is_image_missing() {
        let tmp = Scratch::new("missing");
        let root = tmp.path().join("root");
        let gone = tmp.path().join("gone.png");
        let err = stage_image_in(&root, path_str(&gone)).unwrap_err();
        assert!(err.starts_with("image_missing:"), "{err}");
        assert!(!err.contains("os error"), "不该拼系统原文：{err}");
        assert!(!root.exists(), "校验失败时不该建暂存目录");
    }

    #[test]
    fn stage_is_idempotent_for_files_already_under_root() {
        let tmp = Scratch::new("idem");
        let root = tmp.path().join("root");
        let src = tmp.path().join("a.png");
        std::fs::write(&src, FAKE_PNG).unwrap();

        let staged = stage_image_in(&root, path_str(&src)).unwrap();
        assert_eq!(subdirs(&root).len(), 1);
        let again = stage_image_in(&root, &staged).unwrap();
        assert_eq!(again, staged, "已是自己的副本就原样返回");
        assert_eq!(subdirs(&root).len(), 1, "不该再新增子目录");
    }

    #[test]
    fn stage_leaves_paste_copies_alone() {
        // 粘贴图片已经是 save_pasted_image 落的自家副本，不该再复制一遍。
        let paste = std::env::temp_dir().join(PASTE_DIR);
        let existed = paste.exists();
        std::fs::create_dir_all(&paste).unwrap();
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let file = paste.join(format!("test-stage-{}-{nonce}.png", std::process::id()));
        std::fs::write(&file, FAKE_PNG).unwrap();

        let tmp = Scratch::new("paste");
        let root = tmp.path().join("root");
        let r = stage_image_in(&root, path_str(&file));
        let _ = std::fs::remove_file(&file);
        if !existed { let _ = std::fs::remove_dir(&paste); }
        assert_eq!(r.unwrap(), path_str(&file));
        assert!(!root.exists());
    }

    #[test]
    fn stage_rejects_non_image_extension() {
        let tmp = Scratch::new("ext");
        let root = tmp.path().join("root");
        for name in ["notes.txt", "a.png.exe", "noext"] {
            let f = tmp.path().join(name);
            std::fs::write(&f, b"hello").unwrap();
            let err = stage_image_in(&root, path_str(&f)).unwrap_err();
            assert!(err.contains("只支持"), "{name}: {err}");
        }
        assert!(!root.exists());
    }

    #[test]
    fn prune_removes_only_expired_dirs() {
        let tmp = Scratch::new("prune");
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::write(root.join("one").join("a.png"), FAKE_PNG).unwrap();
        std::fs::write(root.join("stray.txt"), b"x").unwrap();

        prune_stale(&root, Duration::from_secs(3600));
        assert!(root.join("one").exists(), "没到期的不能动");
        prune_stale(&root, Duration::ZERO);
        assert!(!root.join("one").exists());
        assert!(root.join("stray.txt").exists(), "只清子目录，不碰散落文件");
    }
}
