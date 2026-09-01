//! advice.rs —— 云端「AI 工具最新提示」热下发（**只读检测 + 提示，零命令执行**）。
//!
//! 用途：Codex / Claude 再冒新坑（串口崩溃、TEMP 变慢…），改服务器 JSON + version+1
//! 即到全部客户机，**不发 exe**。ukrt / macopt 保持纯本地 doctor 不动，这里是叠在其上的
//! 「热更新提示层」，渲染进「AI 优化大师」页。
//!
//! 铁律（对齐开发宪法 + installer 下发链的安全约定）：
//! - 只暴露纯函数、不碰 AppHandle（`#[tauri::command]` 包在 lib.rs）。
//! - 依赖方向只向公共层：复用 `installer::curl`（系统 curl.exe），不 import 其它业务模块。
//! - **探针全白名单只读**：只判断「某工具是否在 PATH / 某文件是否存在」，
//!   绝不执行下发的任意命令 —— 下发链即便被篡改，最坏也只是弹一条误导性提示，不构成 RCE。
//! - 网络失败 / 服务器没上线 → 静默用内嵌兜底（离线也有内容）。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 依次尝试，第一个拉到「合法且 version 更大」的生效。只用国内可达域（cloud/api 子域部分网络被
/// GFW SNI reset，见 CLAUDE.md/Issue#18；u-claw.org.cn 是唯一全国内可达子域）。
const ADVICE_URLS: &[&str] = &[
    "https://u-claw.org.cn/uking/optimize-advice.json",
    "https://cloud.u-claw.org/uking/optimize-advice.json",
];

/// 内嵌兜底（构建时打进 exe）。单一真相源仍是服务器版；这份只在离线 / 服务器版更旧时兜底。
const EMBEDDED_ADVICE: &str = include_str!("../resources/optimize-advice.json");

#[derive(Deserialize)]
struct AdviceFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    advices: Vec<Advice>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Advice {
    pub id: String,
    /// 空 = 全平台；否则匹配 "windows" / "macos"。
    #[serde(default)]
    pub os: Vec<String>,
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub url: String,
    /// "info" | "warn"（前端配色用）。
    #[serde(default = "sev_info")]
    pub severity: String,
    /// 全部命中才显示；空 = 无条件显示（该 os 下）。
    #[serde(default)]
    pub when: Vec<Probe>,
}
fn sev_info() -> String {
    "info".into()
}

/// 白名单只读探针。新增探针请保持「只读、不接受可执行内容」的性质。
#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "kind")]
pub enum Probe {
    /// 某工具在 PATH 上（如 codex / claude）。
    #[serde(rename = "tool_installed")]
    ToolInstalled { name: String },
    /// 某工具不在 PATH 上。
    #[serde(rename = "tool_missing")]
    ToolMissing { name: String },
    /// 某文件 / 目录存在（支持 %VAR% 与 ~ 展开）。
    #[serde(rename = "file_exists")]
    FileExists { path: String },
}

fn this_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// 收集**当前机器命中**的提示：先按 os 过滤，再全部 when 探针通过才留下。
pub fn collect() -> Vec<Advice> {
    let os = this_os();
    load()
        .advices
        .into_iter()
        .filter(|a| a.os.is_empty() || a.os.iter().any(|o| o == os))
        .filter(|a| a.when.iter().all(probe_ok))
        .collect()
}

fn load() -> AdviceFile {
    let mut best: AdviceFile =
        serde_json::from_str(EMBEDDED_ADVICE).unwrap_or(AdviceFile { version: 0, advices: Vec::new() });
    if let Some(remote) = fetch_remote() {
        if remote.version > best.version {
            best = remote;
        }
    }
    best
}

fn fetch_remote() -> Option<AdviceFile> {
    for url in ADVICE_URLS {
        if let Ok(out) = crate::installer::curl(&["-sL", "-m", "6", url]) {
            if let Ok(f) = serde_json::from_str::<AdviceFile>(&out) {
                return Some(f);
            }
        }
    }
    None
}

fn probe_ok(p: &Probe) -> bool {
    match p {
        Probe::ToolInstalled { name } => tool_present(name),
        Probe::ToolMissing { name } => !tool_present(name),
        Probe::FileExists { path } => {
            let ep = expand(path);
            Path::new(&ep).symlink_metadata().is_ok() || Path::new(&ep).exists()
        }
    }
}

/// 工具名严格校验 + 只作为单一 arg 传给 where/which（非 shell），双保险防下发数据被当命令跑。
fn tool_present(name: &str) -> bool {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return false;
    }
    #[cfg(windows)]
    let prog = "where.exe";
    #[cfg(not(windows))]
    let prog = "which";
    let mut cmd = std::process::Command::new(prog);
    cmd.arg(name);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW，防闪黑窗
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// 展开 ~ 与 %VAR%（Windows 风格）。纯字符串替换，只用于 file_exists 的路径判断（只读）。
fn expand(p: &str) -> String {
    let mut s = p.to_string();
    if s.starts_with('~') {
        if let Ok(h) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            s = s.replacen('~', &h, 1);
        }
    }
    if s.contains('%') {
        for (k, v) in std::env::vars() {
            let pat = format!("%{}%", k);
            if s.contains(&pat) {
                s = s.replace(&pat, &v);
            }
        }
    }
    s
}
