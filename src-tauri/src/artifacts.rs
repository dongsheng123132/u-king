//! 产出箱 —— 小程序（以及任何宿主能力）产出的东西的统一落点。
//!
//! 为什么要有它：无头调用产出的是**数据**，不是界面。
//! 一个 MCP 工具往终端里吐几 MB base64 是错的 —— Claude Code 里是一屏乱码，
//! Hermes 里同样，还白白撑爆上下文。正确做法是产物落盘、返回**引用**：
//!   { artifact: { id, kind, w, h, path }, message: "已去除水印 · 2000×1500" }
//! 文本客户端拿到一行人话 + 一个路径（Claude Code 可以直接 Read 那个路径看图），
//! GUI 侧则用 uking://localhost/artifact/<id> 取像素。两条路产出到同一个地方。
//!
//! **独立可插拔**：纯 std + serde_json，不 import 任何业务模块。
//! 落盘范式照抄 draw.rs（它是 AI 创作页的专用历史，两者互不干涉）。
//! 删它：lib.rs 去 `mod artifacts;` + handler 条目；miniapp.rs 去 artifact.emit 那一支。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 收件箱上限。超过就丢最旧的（同 draw.rs 的 30 条策略，只是产出可能更大所以给多一点）。
const MAX_ITEMS: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    /// image | text | json | file
    pub kind: String,
    /// 产出者：小程序 id，或宿主动作 id
    pub source: String,
    /// 产生它的动作 id（有的话）
    #[serde(default)]
    pub action: Option<String>,
    /// 盘上的文件名（artifacts/ 下）
    pub file: String,
    #[serde(default)]
    pub w: Option<u32>,
    #[serde(default)]
    pub h: Option<u32>,
    pub bytes: u64,
    /// 一行人话，直接给终端客户端看
    #[serde(default)]
    pub message: Option<String>,
    pub ts: i64,
    /// 用户看过了没 —— 角标靠它计数
    #[serde(default)]
    pub seen: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<Artifact>,
}

fn uking_home() -> PathBuf {
    if let Ok(p) = std::env::var("UKING_ARTIFACTS_ROOT") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

fn dir() -> PathBuf {
    uking_home().join("artifacts")
}
fn index_path() -> PathBuf {
    dir().join("index.json")
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_index() -> Index {
    std::fs::read_to_string(index_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_index(i: &Index) {
    let _ = std::fs::create_dir_all(dir());
    if let Ok(s) = serde_json::to_string_pretty(i) {
        let _ = std::fs::write(index_path(), s);
    }
}

/// 从 data URL 或裸 base64 里抠出字节 + 猜扩展名。
fn decode_payload(data: &str) -> Result<(Vec<u8>, &'static str), String> {
    let b64 = match data.find(";base64,") {
        Some(i) => &data[i + 8..],
        None => data,
    };
    let bytes = b64_decode(b64.trim())?;
    let ext = sniff_ext(&bytes);
    Ok((bytes, ext))
}

fn sniff_ext(b: &[u8]) -> &'static str {
    if b.len() > 8 && &b[..8] == b"\x89PNG\r\n\x1a\n" {
        "png"
    } else if b.len() > 3 && b[0] == 0xFF && b[1] == 0xD8 {
        "jpg"
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "webp"
    } else if b.len() > 4 && &b[..4] == b"%PDF" {
        "pdf"
    } else if b.len() > 3 && &b[..3] == b"GIF" {
        "gif"
    } else {
        "bin"
    }
}

/// PNG 的 IHDR 就在固定偏移，读宽高不需要解码整张图。
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[12..16] != b"IHDR" {
        return None;
    }
    let rd = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    Some((rd(16), rd(20)))
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut map = [255u8; 256];
    for (i, c) in T.iter().enumerate() {
        map[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut n = 0u32;
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = map[c as usize];
        if v == 255 {
            return Err("非法的 base64 字符".into());
        }
        acc = (acc << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    Ok(out)
}

/// 收一个产物。返回给调用方的引用（不含像素）。
pub fn emit(
    source: &str,
    action: Option<&str>,
    kind: &str,
    data: &str,
    message: Option<&str>,
) -> Result<Artifact, String> {
    let (bytes, ext) = decode_payload(data)?;
    if bytes.is_empty() {
        return Err("产物是空的".into());
    }
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("产物过大（上限 64MB）".into());
    }
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;

    let ts = now_ms();
    let id = format!("art_{ts:x}_{:04x}", bytes.len() as u16);
    let file = format!("{id}.{ext}");
    std::fs::write(dir().join(&file), &bytes).map_err(|e| e.to_string())?;

    let (w, h) = png_size(&bytes).map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
    let a = Artifact {
        id,
        kind: kind.to_string(),
        source: source.to_string(),
        action: action.map(String::from),
        file,
        w,
        h,
        bytes: bytes.len() as u64,
        message: message.map(String::from),
        ts,
        seen: false,
    };

    let mut idx = read_index();
    idx.version = 1;
    idx.items.insert(0, a.clone());
    // 超限的丢最旧，连文件一起删，免得收件箱无声膨胀
    while idx.items.len() > MAX_ITEMS {
        if let Some(old) = idx.items.pop() {
            let _ = std::fs::remove_file(dir().join(&old.file));
        }
    }
    write_index(&idx);
    Ok(a)
}

pub fn list() -> Vec<Artifact> {
    // 自愈：索引里有但文件没了的丢掉（用户可能手动删了目录）
    let mut idx = read_index();
    let before = idx.items.len();
    idx.items.retain(|a| dir().join(&a.file).exists());
    if idx.items.len() != before {
        write_index(&idx);
    }
    idx.items
}

pub fn unseen_count() -> usize {
    list().iter().filter(|a| !a.seen).count()
}

pub fn mark_seen(ids: &[String]) {
    let mut idx = read_index();
    for a in idx.items.iter_mut() {
        if ids.is_empty() || ids.contains(&a.id) {
            a.seen = true;
        }
    }
    write_index(&idx);
}

/// 盘上的绝对路径 —— 这就是回给终端客户端的那个 path。
pub fn path_of(id: &str) -> Option<PathBuf> {
    list()
        .into_iter()
        .find(|a| a.id == id)
        .map(|a| dir().join(a.file))
}

pub fn read_bytes(id: &str) -> Option<Vec<u8>> {
    path_of(id).and_then(|p| std::fs::read(p).ok())
}

pub fn clear() {
    let _ = std::fs::remove_dir_all(dir());
}
