//! 图片预处理动作：图片只交给视觉模型，主对话模型只收到文字结果。
//!
//! 这不是另写一套识图实现，而是应用内复用 `uking-vision` 的 `see-image.mjs`。
//! 因此 GUI、Action CLI 和 MCP 的模型链、纯文本模型闸门、失败回退完全一致。

use serde::Serialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 20 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(180);
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
    let p = Path::new(path)
        .canonicalize()
        .map_err(|e| format!("找不到图片文件: {e}"))?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("读取图片属性失败: {e}"))?;
    if !meta.is_file() {
        return Err("图片路径必须是一个普通文件。".into());
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("图片超过 20MB（当前 {}MB），请先压缩后再发。", meta.len() / 1024 / 1024));
    }
    Ok(p)
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
}
