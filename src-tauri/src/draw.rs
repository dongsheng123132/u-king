//! AI 作图历史持久化 —— 关 app 也不丢（修「记录不见了」）。
//!
//! 图片本体（base64 PNG 可能几 MB）单独落 `~/.uking/draw/<id>.png`，
//! 历史索引 `~/.uking/draw/history.json` 只存元数据 + 文件名，避免一份巨型 JSON。
//! 读历史时把 PNG 重新读成 data URL 回前端（前端 <img src> 直接用）。
//!
//! 纯 std + serde_json，照抄 tasks.rs 的落盘范式，零新依赖。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 一条作图记录（成功有 file，失败有 error）。前端 DrawItem 的持久化镜像。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawRecord {
    pub id: i64,
    pub prompt: String,
    pub model: String,
    pub size: String,
    /// 成功：图片文件名（draw/ 下的 <id>.png）。失败为 None。
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub revised: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub ts: i64,
}

/// 回前端用：把 file 换成可直接渲染的 data URL（src 字段），不暴露磁盘路径。
#[derive(Debug, Clone, Serialize)]
pub struct DrawItemOut {
    pub id: i64,
    pub prompt: String,
    pub model: String,
    pub size: String,
    pub src: Option<String>,
    pub revised: Option<String>,
    pub error: Option<String>,
    pub ts: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    items: Vec<DrawRecord>,
}

const MAX_ITEMS: usize = 30;

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

fn draw_dir() -> PathBuf {
    uking_home().join("draw")
}

fn history_path() -> PathBuf {
    draw_dir().join("history.json")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_file() -> HistoryFile {
    std::fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str::<HistoryFile>(&s).ok())
        .unwrap_or(HistoryFile {
            version: 1,
            items: Vec::new(),
        })
}

fn write_file(f: &HistoryFile) -> Result<(), String> {
    let _ = std::fs::create_dir_all(draw_dir());
    let s = serde_json::to_string_pretty(f).map_err(|e| format!("序列化作图历史失败: {e}"))?;
    std::fs::write(history_path(), s).map_err(|e| format!("写入作图历史失败: {e}"))
}

/// 把一条 base64 PNG（或失败记录）存进历史。成功时把 b64 落成 <id>.png。
/// 返回这条记录的 id（前端用它做 key / 下载文件名）。
pub fn save_record(
    prompt: &str,
    model: &str,
    size: &str,
    b64: Option<&str>,
    revised: Option<&str>,
    error: Option<&str>,
) -> Result<i64, String> {
    let _ = std::fs::create_dir_all(draw_dir());
    let id = now_ms();

    let file = match b64 {
        Some(data) if !data.is_empty() => {
            let bytes = b64_decode(data)?;
            let name = format!("{id}.png");
            std::fs::write(draw_dir().join(&name), bytes)
                .map_err(|e| format!("写图片文件失败: {e}"))?;
            Some(name)
        }
        _ => None,
    };

    let mut f = read_file();
    f.items.insert(
        0,
        DrawRecord {
            id,
            prompt: prompt.to_string(),
            model: model.to_string(),
            size: size.to_string(),
            file,
            revised: revised.map(String::from),
            error: error.map(String::from),
            ts: id,
        },
    );
    // 超出上限的尾部记录连同其图片文件一起删（防止 draw/ 无限膨胀）
    while f.items.len() > MAX_ITEMS {
        if let Some(old) = f.items.pop() {
            if let Some(name) = old.file {
                let _ = std::fs::remove_file(draw_dir().join(name));
            }
        }
    }
    f.version = 1;
    write_file(&f)?;
    Ok(id)
}

/// 读全部历史（最近在前），图片读回 data URL 给前端 <img>。
#[tauri::command]
pub fn list_draw_history() -> Vec<DrawItemOut> {
    read_file()
        .items
        .into_iter()
        .map(|r| {
            let src = r.file.as_ref().and_then(|name| {
                std::fs::read(draw_dir().join(name))
                    .ok()
                    .map(|bytes| format!("data:image/png;base64,{}", b64_encode(&bytes)))
            });
            DrawItemOut {
                id: r.id,
                prompt: r.prompt,
                model: r.model,
                size: r.size,
                src,
                revised: r.revised,
                error: r.error,
                ts: r.ts,
            }
        })
        .collect()
}

/// 某条作图记录的磁盘 PNG 路径（导出/另存为用）。不存在返回 None。
pub fn draw_file_path(id: i64) -> Option<PathBuf> {
    read_file()
        .items
        .into_iter()
        .find(|r| r.id == id)
        .and_then(|r| r.file)
        .map(|name| draw_dir().join(name))
        .filter(|p| p.exists())
}

/// 清空作图历史（删 history.json + draw/ 下所有 png）。
#[tauri::command]
pub fn clear_draw_history() -> Result<(), String> {
    let f = read_file();
    for r in f.items {
        if let Some(name) = r.file {
            let _ = std::fs::remove_file(draw_dir().join(name));
        }
    }
    let _ = std::fs::remove_file(history_path());
    Ok(())
}

// ── base64（纯 std，不引 crate）──────────────────────────────
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    // 反查表
    let mut rev = [255u8; 256];
    for (i, &c) in B64.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let mut buf = 0u32;
    let mut bits = 0;
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = rev[c as usize];
        if v == 255 {
            return Err("base64 含非法字符".into());
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}
