//! 一键备份 / 还原 v2 —— 内容寻址（content-addressed）增量快照到 U 盘。
//!
//! 覆盖 6 个应用的用户数据：**Claude Code CLI / Claude 桌面版 / Codex / ClawX / OpenClaw / Obsidian**。
//! 换台电脑（家 ↔ 办公室）插上 U 盘一键拉回，接着干活。
//!
//! ## 为什么是「内容寻址」而不是「每次全量拷一份时间戳目录」
//!
//! 用户要「保留多份全量时间戳快照」，但 U 盘多是 **exFAT/FAT32（不支持硬链接）**，6 个应用必备数据
//! 加起来 ~1.3GB，每份快照实打实全量拷一遍 → 堆 10 份就 13GB，且慢。借鉴 restic/borg 的做法：
//!
//! - `blobs/<aa>/<blake3>`：每个**唯一文件内容只存一次**（跨快照、跨应用去重）。blob 名就是内容的
//!   blake3 → 天然支持完整性校验。**注**：当前还原只校验引用的 blob **在位**（缺一即拒）；
//!   逐块重算 blake3（发现坏块/半写）是 blob 命名白送的能力，但 ~1.3GB 重算有成本，默认未开。
//! - `snapshots/<unix秒>.json`：一份快照 = 只记 `路径 → blob 哈希` 指针的小清单（KB 级）。
//!   每份快照逻辑上都是**全量、可独立还原 / 独立删除**，但相同文件不重复占盘。
//! - `files-cache.json`：本机 `绝对路径 → (size, mtime, hash)`，下次备份未变的文件**根本不重新 hash**
//!   （borg files-cache 快路径）。这是「重复备份从几分钟变几秒」的关键。
//!
//! ## 安全模型（绝不静默丢数据，沿用 v1 的硬约束）
//!
//! 还原前：① 先把**当前机器**状态也快照一份（安全网，可回滚）→ ② 关掉会锁库的 Electron 应用
//! （ClawX / Claude 桌面版 / Obsidian）→ ③ 校验目标快照引用的 blob **全部在位**（缺一即拒绝还原，
//! 绝不铺半套）。随后**两阶段换上位**，保证多应用「要么全换好、要么全回原样」：
//! **阶段 1** 把每个应用都重建到各自**同盘临时目录(staging)**，全程不碰任何真实目录——
//! blob 拷贝失败（盘满 / 坏块）只发生在这里，清掉 staging 即原数据分毫未动；
//! **阶段 2** 全部 staging 就绪后逐个原子换上位（旧目录**改名留底** `<dir>.uking-bak-<ts>` →
//! staging 改名上位），中途任一应用失败即把**已换好的全部回滚**到留底（绝不留半套）。
//!
//! ## 密钥
//!
//! 按用户选择：`auth.json` / `clawx-providers.json` / `~/.claude.json` 里的 key **一起明文备份**
//! （换机零配置）。⚠️ U 盘丢失即明文泄露——这是显式取舍，不是默认。
//!
//! 纯 std + serde + blake3(pure)，无重型依赖。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// U 盘上的同步根目录名（在所选盘符/目录下）。
const SYNC_DIR: &str = "U-King-Sync";
/// 当前快照清单格式版本。
const MANIFEST_FORMAT: u32 = 2;
/// 还原留底（`<name>.uking-bak-<ts>`）每个目标保留份数。
const KEEP_BAKS: usize = 3;
/// 默认保留多少份时间戳快照（去重后很省，故给得大方）。可用 `UKING_KEEP_SNAPSHOTS` 调。
const DEFAULT_KEEP_SNAPSHOTS: usize = 20;

fn keep_snapshots() -> usize {
    std::env::var("UKING_KEEP_SNAPSHOTS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(DEFAULT_KEEP_SNAPSHOTS)
}

// ============================================================
// 应用清单（要备份哪些应用的哪些目录 / 跳过什么）
//
// 路径策展思路借鉴 mackup（lra/mackup）的「应用 → 配置路径」数据库，但**只取路径这一事实、
// 用自己的数据结构重写**（mackup 是 GPL，不照搬其代码/配置文件）。Windows 路径自行映射。
// ============================================================

/// 一个备份源 = 一段要整体带走的目录（带跳过规则）。Obsidian 会动态展开成多个（每个 vault 一个）。
#[derive(Clone)]
struct Source {
    /// 落进 manifest 的稳定标识（也用于还原时反推目标路径）。如 `claude-cli` / `vault:MyNotes`。
    name: String,
    /// 给人看的中文名。
    label: String,
    /// 源目录绝对路径（本机此刻的位置）。
    root: PathBuf,
    /// 跳过：目录名（任意层级命中即整枝跳过，缓存/可再生类）。
    skip_dirs: Vec<String>,
    /// 跳过：精确相对路径（正斜杠），用于 `.obsidian/workspace.json` 这种单文件。
    skip_rel: Vec<String>,
}

fn home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
fn appdata() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn s(v: &str) -> String {
    v.to_string()
}
fn vec_s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// 5 个「固定家目录」应用（路径可由环境变量在任意机器重算 → 跨机还原稳）。
/// 返回的都是「目录确实存在」的源；不存在的略过。
fn fixed_sources() -> Vec<Source> {
    let mut out = Vec::new();

    // 1) Claude Code CLI —— ~/.claude（会话/记忆/配置）。跳过可再生的大目录。
    if let Some(h) = home() {
        let r = h.join(".claude");
        if r.is_dir() {
            out.push(Source {
                name: s("claude-cli"),
                label: s("Claude Code CLI"),
                root: r,
                skip_dirs: vec_s(&["plugins", "file-history", "cache", "statsig", "tmp", ".cache"]),
                skip_rel: vec![],
            });
        }
    }

    // 2) Claude 桌面版 —— %APPDATA%\Claude（Electron）。跳过 Chromium 缓存类。
    if let Some(a) = appdata() {
        let r = a.join("Claude");
        if r.is_dir() {
            out.push(Source {
                name: s("claude-desktop"),
                label: s("Claude 桌面版"),
                root: r,
                skip_dirs: vec_s(&[
                    "Cache", "Code Cache", "GPUCache", "DawnGraphiteCache", "DawnWebGPUCache",
                    "blob_storage", "logs", "Crashpad", "Dictionaries", "Shared Dictionary",
                    "component_crx_cache", "GrShaderCache", "ShaderCache",
                ]),
                skip_rel: vec![],
            });
        }
    }

    // 3) Codex CLI —— ~/.codex（logs_2.sqlite + sessions 是对话历史核心）。跳过可重下/可再生。
    if let Some(h) = home() {
        let r = h.join(".codex");
        if r.is_dir() {
            out.push(Source {
                name: s("codex"),
                label: s("Codex CLI"),
                root: r,
                skip_dirs: vec_s(&["plugins", "cache", ".cache", "vendor_imports", "generated_images", "tmp"]),
                skip_rel: vec![],
            });
        }
    }

    // 4) ClawX —— %APPDATA%\ClawX（providers/settings/Local Storage）。跳过日志/缓存。
    if let Some(a) = appdata() {
        let r = a.join("ClawX");
        if r.is_dir() {
            out.push(Source {
                name: s("clawx"),
                label: s("ClawX 对话与设置"),
                root: r,
                skip_dirs: vec_s(&[
                    "logs", "Cache", "Code Cache", "GPUCache", "blob_storage",
                    "DawnGraphiteCache", "DawnWebGPUCache", "Dictionaries", "Shared Dictionary", "Crashpad",
                ]),
                skip_rel: vec![],
            });
        }
    }

    // 5) OpenClaw —— ~/.openclaw（workspace/tasks/memory/state/state.db/skills）。跳过日志/大媒体。
    if let Some(h) = home() {
        let r = h.join(".openclaw");
        if r.is_dir() {
            out.push(Source {
                name: s("openclaw"),
                label: s("OpenClaw 龙虾工作区"),
                root: r,
                skip_dirs: vec_s(&["logs", "media", "browser", "node_modules", ".git", "target", ".cache"]),
                skip_rel: vec![],
            });
        }
    }

    out
}

/// Obsidian 专属：`%APPDATA%\obsidian\obsidian.json` 里列了所有 vault 的真实磁盘路径。
/// 备：① 全局配置目录（去缓存）② 每个 vault 整库（排除会冲突/可再生的 workspace.json、.trash 等）。
fn obsidian_sources() -> Vec<Source> {
    let mut out = Vec::new();
    let Some(a) = appdata() else { return out };
    let cfg_dir = a.join("obsidian");

    // ① 全局配置（含 obsidian.json 这张 vault 指针表）。
    if cfg_dir.is_dir() {
        out.push(Source {
            name: s("obsidian-config"),
            label: s("Obsidian 全局配置"),
            root: cfg_dir.clone(),
            skip_dirs: vec_s(&["Cache", "Code Cache", "GPUCache", "blob_storage", "logs"]),
            skip_rel: vec![],
        });
    }

    // ② 解析 obsidian.json → 每个 vault 一个源。
    let cfg = cfg_dir.join("obsidian.json");
    let Ok(text) = std::fs::read_to_string(&cfg) else { return out };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return out };
    let Some(vaults) = json.get("vaults").and_then(|v| v.as_object()) else { return out };

    for (_id, v) in vaults {
        let Some(p) = v.get("path").and_then(|x| x.as_str()) else { continue };
        let vp = PathBuf::from(p);
        if !vp.is_dir() {
            continue; // vault 可能已删/换盘，略过
        }
        let base = vp
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| s("vault"));
        out.push(Source {
            name: format!("vault:{base}"),
            label: format!("Obsidian 笔记库「{base}」"),
            root: vp,
            // .obsidian/workspace*.json 几乎一直在变、是跨机同步头号冲突源（obsidian-git 社区共识）。
            skip_dirs: vec_s(&[".trash", ".git"]),
            skip_rel: vec_s(&[
                ".obsidian/workspace.json",
                ".obsidian/workspace-mobile.json",
                ".obsidian/workspace",
            ]),
        });
    }

    out
}

/// 本机所有备份源（固定 5 应用 + Obsidian 动态展开）。只含存在的目录。
fn all_sources() -> Vec<Source> {
    let mut v = fixed_sources();
    v.extend(obsidian_sources());
    v
}

/// 还原时把某个源名反推成本机目标目录。
/// 固定应用按环境变量重算（跨机稳）；vault 用快照里存的绝对路径，父目录不存在则落到桌面兜底。
fn restore_target(name: &str, stored_root: &str) -> Option<PathBuf> {
    match name {
        "claude-cli" => home().map(|h| h.join(".claude")),
        "claude-desktop" => appdata().map(|a| a.join("Claude")),
        "codex" => home().map(|h| h.join(".codex")),
        "clawx" => appdata().map(|a| a.join("ClawX")),
        "openclaw" => home().map(|h| h.join(".openclaw")),
        "obsidian-config" => appdata().map(|a| a.join("obsidian")),
        _ => {
            // vault:<name> —— 优先还原回原路径；原盘/父目录不在，就落桌面 <name>。
            let stored = PathBuf::from(stored_root);
            if stored.parent().map(|p| p.is_dir()).unwrap_or(false) {
                return Some(stored);
            }
            let base = stored
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .or_else(|| name.strip_prefix("vault:").map(|x| x.to_string()))
                .unwrap_or_else(|| s("vault"));
            home().map(|h| h.join("Desktop").join(base))
        }
    }
}

// ============================================================
// 对前端的数据结构（**对外形状与 v1 完全一致**，前端零改动）
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct ItemStat {
    pub name: String,
    pub label: String,
    pub files: u64,
    pub bytes: u64,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupResult {
    /// 快照清单文件绝对路径（snapshots/<ts>.json）。
    pub dir: String,
    pub machine: String,
    pub created_at: i64,
    pub items: Vec<ItemStat>,
    pub total_bytes: u64,
}

/// list 返回：一条历史快照（前端展示用的摘要）。
#[derive(Debug, Clone, Serialize)]
pub struct BackupEntry {
    /// 还原时回传的引用（v2=snapshots/<ts>.json 文件；v1=旧快照目录）。
    pub dir: String,
    pub machine: String,
    pub user: String,
    pub created_at: i64,
    pub app_version: String,
    pub items: Vec<ManifestItem>,
    pub total_bytes: u64,
    pub is_this_machine: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreResult {
    pub items: Vec<ItemStat>,
    /// 还原前自动备份当前状态的目录（安全网，可回滚）。
    pub pre_backup_dir: Option<String>,
    /// 被改名留底的旧目录。
    pub archived: Vec<String>,
}

/// 摘要项（BackupEntry 用；也是 v1 manifest 的形状）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestItem {
    pub name: String,
    pub label: String,
    pub files: u64,
    pub bytes: u64,
}

// ---- 落盘的 v2 清单 ----

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
    /// 相对源根目录的路径（正斜杠）。
    rel: String,
    /// 内容 blake3（十六进制）。
    hash: String,
    size: u64,
    /// 修改时间（unix 秒）。
    mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppManifest {
    name: String,
    label: String,
    /// 备份时源根目录绝对路径（vault 还原反推用）。
    root: String,
    bytes: u64,
    files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestV2 {
    format: u32,
    machine: String,
    user: String,
    created_at: i64,
    app_version: String,
    apps: Vec<AppManifest>,
}

/// 旧 v1 清单（仅用于 list/restore 兼容读，绝不主动改写用户已有的旧快照）。
#[derive(Debug, Clone, Deserialize)]
struct ManifestV1 {
    machine: String,
    #[serde(default)]
    user: String,
    created_at: i64,
    #[serde(default)]
    app_version: String,
    #[serde(default)]
    items: Vec<ManifestItem>,
}

// ============================================================
// 基础工具
// ============================================================

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
/// 文件修改时间（unix **毫秒**）。用毫秒而非秒：files-cache 比对的是同一本地盘(NTFS)上源文件
/// 自身前后两次的 mtime，秒级粒度会漏掉「同一秒内、大小不变」的修改。
fn file_mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 本机名（文件系统安全）。
pub fn machine_name() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("NAME"))
        .unwrap_or_else(|_| "this-pc".into());
    let safe: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if safe.is_empty() { "this-pc".into() } else { safe }
}

fn user_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default()
}

/// 建议的默认备份盘 —— 当前 exe 所在盘根目录（U 盘版多半就是 U 盘本身）。
pub fn default_root() -> String {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(prefix) = exe.components().next() {
                let p = PathBuf::from(prefix.as_os_str()).join("\\");
                return p.display().to_string();
            }
        }
    }
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_else(|_| ".".into())
}

/// 还原前要关掉的应用镜像名。**只许放名字无歧义的**：任何可能与 AI CLI 撞名的
/// （`claude.exe` / `node.exe` / `codex.exe` …）都不准进这张表 —— `taskkill /IM` 不区分
/// 大小写且杀掉全部同名进程。`locking_image_names_never_shadow_ai_clis` 用例守着这条。
#[cfg(windows)]
const LOCKING_IMAGE_NAMES: &[&str] = &["ClawX.exe", "Obsidian.exe"];

/// 关掉会锁库的 Electron 应用（还原前必做，否则 leveldb/sqlite 被锁、改名覆盖失败或写半套）。
/// best-effort：没装 / 没跑无所谓。
///
/// **绝不按裸镜像名杀 Claude。** Claude Code CLI 的可执行文件同样叫 `claude.exe`
/// （`%APPDATA%\Claude\claude-code\<ver>\claude.exe`、`%APPDATA%\npm\...\bin\claude.exe`），
/// 而 `taskkill /IM` 不区分大小写 —— 老写法 `taskkill /IM Claude.exe /F /T` 会把用户正在跑的
/// 全部 Claude Code 会话一起端掉（`/IM` 杀同名全部 + `/T` 杀整树），开发机上跑 `cargo test`
/// 更是连发起测试的那个会话一起自杀，且 TerminateProcess 不留任何崩溃痕迹极难排查。
/// 桌面版改为只认安装路径（Windows `…\AnthropicClaude\…` / macOS `/Claude.app/`）逐 PID 杀，
/// 宁可漏杀（还原失败会走既有回滚）也不误杀用户的 AI 会话。
fn kill_locking_apps() {
    // 硬保证：测试构建下永不碰真实进程。`restore()` 有 3 个用例会走到这里，靠「记得设某个
    // 环境变量」是不够的（老版本正是这样把开发者自己的会话杀了）。
    if cfg!(test) {
        return;
    }
    // 沙箱（`UKING_TEST_HOME` 非空）同样跳过 —— 与 `apply_clawx_managed` 同一约定，
    // 让 `--selfcheck` 之类拿真 exe 跑的沙箱流程也打不到真机上的应用。
    if std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false) {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 名字无歧义的，仍按镜像名杀。
        for im in LOCKING_IMAGE_NAMES {
            let _ = std::process::Command::new(crate::installer::system_tool("taskkill"))
                .args(["/IM", im, "/F", "/T"])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        // Claude 桌面版：装在 `%LOCALAPPDATA%\AnthropicClaude\`，按可执行文件路径逐 PID 杀。
        // CLI 装在 `%APPDATA%\Claude\claude-code\` 与 `%APPDATA%\npm\`，不含该目录名，杀不到。
        let _ = std::process::Command::new(crate::installer::system_tool("powershell"))
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like '*\\AnthropicClaude\\*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }",
            ])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(target_os = "macos")]
    {
        // 同理：`pkill -f Claude` 会连命令行里带 `Claude` / `.claude` 的 CLI 会话一起杀，
        // 只按 `.app` 包路径匹配。
        for pat in ["/ClawX.app/", "/Claude.app/", "/Obsidian.app/"] {
            let _ = std::process::Command::new("pkill").args(["-f", pat]).status();
        }
    }
}

// ============================================================
// 遍历（带 skip 谓词）
// ============================================================

/// 递归收集 `root` 下的文件，产出 `(相对路径正斜杠, 绝对路径, metadata)`。
/// 跳过：目录名在 `skip_dirs`（任意层级）或相对路径在 `skip_rel`。符号链接不跟随。
fn walk(
    dir: &Path,
    root: &Path,
    skip_dirs: &[String],
    skip_rel: &[String],
    out: &mut Vec<(String, PathBuf, std::fs::Metadata)>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败 {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");

        let Ok(meta) = entry.metadata() else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }

        if meta.is_dir() {
            if skip_dirs.iter().any(|d| d == &name) || skip_rel.iter().any(|r| r == &rel) {
                continue;
            }
            walk(&path, root, skip_dirs, skip_rel, out)?;
        } else if meta.is_file() {
            if skip_rel.iter().any(|r| r == &rel) {
                continue;
            }
            out.push((rel, path, meta));
        }
    }
    Ok(())
}

// ============================================================
// blake3 + 内容寻址 blob 仓
// ============================================================

/// 算文件内容 blake3（十六进制）。流式读，省内存。
fn hash_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("打开失败 {}: {e}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取失败 {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// blob 在仓里的路径：`<blobs>/<前2位>/<hash>`。
fn blob_path(blobs: &Path, hash: &str) -> PathBuf {
    blobs.join(&hash[0..2]).join(hash)
}

/// 把文件内容存进 blob 仓（已存在即跳过 = 去重）。原子：先写临时再 rename。
/// 返回 true 表示真写了新 blob（用于统计/测试）。
fn put_blob(blobs: &Path, hash: &str, src: &Path) -> Result<bool, String> {
    let dst = blob_path(blobs, hash);
    if dst.exists() {
        return Ok(false);
    }
    let sub = dst.parent().ok_or_else(|| format!("blob 路径无父目录: {}", dst.display()))?;
    std::fs::create_dir_all(sub).map_err(|e| format!("建 blob 目录失败 {}: {e}", sub.display()))?;
    let tmp = sub.join(format!("{hash}.{}.tmp", now_ms()));
    std::fs::copy(src, &tmp).map_err(|e| format!("写 blob 失败 {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &dst) {
        let _ = std::fs::remove_file(&tmp);
        // 并发下别人可能刚写好同名 blob —— 内容寻址下等价，视作成功。
        if !dst.exists() {
            return Err(format!("blob 落位失败 {}: {e}", dst.display()));
        }
    }
    Ok(true)
}

// ---- files-cache：绝对路径 → (size, mtime, hash)，跳过未变文件的重复 hash ----

type FilesCache = HashMap<String, (u64, i64, String)>;

fn load_files_cache(machine_root: &Path) -> FilesCache {
    let p = machine_root.join("files-cache.json");
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}
fn save_files_cache(machine_root: &Path, cache: &FilesCache) {
    let p = machine_root.join("files-cache.json");
    if let Ok(t) = serde_json::to_string(cache) {
        let _ = std::fs::write(p, t);
    }
}

/// 取文件 hash：cache 命中（源文件 size 与 mtime 都没变）就复用，否则真算并回填。
/// 比对的是本地盘上源文件自身的前后元数据，故 **精确比对毫秒级 mtime**（不容差——容差会漏掉
/// 同一秒内大小不变的修改）。
fn cached_hash(abs: &Path, meta: &std::fs::Metadata, cache: &mut FilesCache) -> Result<String, String> {
    let key = abs.to_string_lossy().to_string();
    let size = meta.len();
    let mtime = file_mtime_ms(meta);
    if let Some((cs, cm, ch)) = cache.get(&key) {
        if *cs == size && *cm == mtime {
            return Ok(ch.clone());
        }
    }
    let h = hash_file(abs)?;
    cache.insert(key, (size, mtime, h.clone()));
    Ok(h)
}

// ============================================================
// 备份
// ============================================================

/// 把本机数据增量快照到 `<dest_root>/U-King-Sync/<机器名>/`（内容寻址 blob + snapshots/<ts>.json）。
pub fn backup(
    dest_root: &str,
    app_version: &str,
    mut on_progress: impl FnMut(&str),
) -> Result<BackupResult, String> {
    // 备份失败（U 盘拔了 / 空间不够 / 文件被占）客户往往不看提示就走了，
    // 等到要还原才发现没备成。留痕才能事后判「那天到底备没备上」。
    crate::ulog::section("backup", &format!("开始备份到 {dest_root}"));
    let on_progress = move |m: &str| {
        crate::ulog::write("backup", m);
        on_progress(m);
    };
    backup_inner(dest_root, app_version, true, on_progress)
}

/// `do_prune=false` 用于「还原前的安全网备份」——此时绝不裁剪/GC，免得把正要还原的快照的 blob 删掉。
fn backup_inner(
    dest_root: &str,
    app_version: &str,
    do_prune: bool,
    mut on_progress: impl FnMut(&str),
) -> Result<BackupResult, String> {
    let root = PathBuf::from(dest_root);
    if !root.is_dir() {
        return Err(format!("备份位置不存在或不是文件夹：{dest_root}"));
    }

    let machine = machine_name();
    let created_at = now_ms();
    let machine_root = root.join(SYNC_DIR).join(&machine);
    let blobs = machine_root.join("blobs");
    let snaps = machine_root.join("snapshots");
    std::fs::create_dir_all(&blobs).map_err(|e| format!("创建 blobs 目录失败: {e}"))?;
    std::fs::create_dir_all(&snaps).map_err(|e| format!("创建 snapshots 目录失败: {e}"))?;

    let mut cache = load_files_cache(&machine_root);
    let mut stats: Vec<ItemStat> = Vec::new();
    let mut apps_manifest: Vec<AppManifest> = Vec::new();
    let mut total_bytes = 0u64;
    let mut new_blobs = 0u64;

    let sources = all_sources();
    on_progress(&format!("发现 {} 个可备份源…", sources.len()));

    for src in &sources {
        on_progress(&format!("正在备份「{}」…", src.label));
        let mut files: Vec<(String, PathBuf, std::fs::Metadata)> = Vec::new();
        if let Err(e) = walk(&src.root, &src.root, &src.skip_dirs, &src.skip_rel, &mut files) {
            on_progress(&format!("⚠️ 跳过「{}」：{e}", src.label));
            continue;
        }

        let mut entries: Vec<FileEntry> = Vec::with_capacity(files.len());
        let mut bytes = 0u64;
        for (rel, abs, meta) in &files {
            let hash = match cached_hash(abs, meta, &mut cache) {
                Ok(h) => h,
                Err(_) => continue, // 单文件读不了（被锁等）不致命，跳过
            };
            match put_blob(&blobs, &hash, abs) {
                Ok(wrote) => {
                    if wrote {
                        new_blobs += 1;
                    }
                }
                Err(_) => continue,
            }
            let size = meta.len();
            bytes += size;
            entries.push(FileEntry { rel: rel.clone(), hash, size, mtime: file_mtime_ms(meta) });
        }

        total_bytes += bytes;
        let present = !entries.is_empty();
        stats.push(ItemStat {
            name: src.name.clone(),
            label: src.label.clone(),
            files: entries.len() as u64,
            bytes,
            present,
        });
        if present {
            apps_manifest.push(AppManifest {
                name: src.name.clone(),
                label: src.label.clone(),
                root: src.root.display().to_string(),
                bytes,
                files: entries,
            });
        }
    }

    // 写快照清单
    let manifest = ManifestV2 {
        format: MANIFEST_FORMAT,
        machine: machine.clone(),
        user: user_name(),
        created_at,
        app_version: app_version.to_string(),
        apps: apps_manifest,
    };
    // 文件名用毫秒时间戳（= created_at），保证同一秒内连续两次备份也不撞名。
    let snap_file = snaps.join(format!("{}.json", created_at));
    std::fs::write(&snap_file, serde_json::to_string_pretty(&manifest).unwrap_or_default())
        .map_err(|e| format!("写快照清单失败: {e}"))?;

    save_files_cache(&machine_root, &cache);
    on_progress(&format!("备份完成（新增 {new_blobs} 个文件块，其余已去重）"));

    if do_prune {
        prune_snapshots(&machine_root);
    }

    Ok(BackupResult {
        dir: snap_file.display().to_string(),
        machine,
        created_at,
        items: stats,
        total_bytes,
    })
}

// ============================================================
// 列出已有快照（v2 snapshots/*.json + 兼容旧 v1 <ts>/manifest.json）
// ============================================================

pub fn list(root: &str) -> Vec<BackupEntry> {
    let base = PathBuf::from(root).join(SYNC_DIR);
    let mut out: Vec<BackupEntry> = Vec::new();
    let this_machine = machine_name();

    let Ok(machines) = std::fs::read_dir(&base) else {
        return out;
    };
    for m in machines.flatten() {
        if !m.path().is_dir() {
            continue;
        }

        // v2: <机器>/snapshots/<ts>.json
        let snaps = m.path().join("snapshots");
        if let Ok(rd) = std::fs::read_dir(&snaps) {
            for s in rd.flatten() {
                let p = s.path();
                if p.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                let Ok(man) = serde_json::from_str::<ManifestV2>(&text) else { continue };
                let items: Vec<ManifestItem> = man
                    .apps
                    .iter()
                    .map(|a| ManifestItem {
                        name: a.name.clone(),
                        label: a.label.clone(),
                        files: a.files.len() as u64,
                        bytes: a.bytes,
                    })
                    .collect();
                let total_bytes = items.iter().map(|i| i.bytes).sum();
                out.push(BackupEntry {
                    dir: p.display().to_string(),
                    is_this_machine: man.machine == this_machine,
                    machine: man.machine,
                    user: man.user,
                    created_at: man.created_at,
                    app_version: man.app_version,
                    items,
                    total_bytes,
                });
            }
        }

        // v1 兼容：<机器>/<ts>/manifest.json（老格式，只读不改）
        if let Ok(rd) = std::fs::read_dir(m.path()) {
            for s in rd.flatten() {
                let dir = s.path();
                if !dir.is_dir() {
                    continue;
                }
                let mf = dir.join("manifest.json");
                let Ok(text) = std::fs::read_to_string(&mf) else { continue };
                let Ok(man) = serde_json::from_str::<ManifestV1>(&text) else { continue };
                let total_bytes = man.items.iter().map(|i| i.bytes).sum();
                out.push(BackupEntry {
                    dir: dir.display().to_string(),
                    is_this_machine: man.machine == this_machine,
                    machine: man.machine,
                    user: man.user,
                    created_at: man.created_at,
                    app_version: man.app_version,
                    items: man.items,
                    total_bytes,
                });
            }
        }
    }

    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

// ============================================================
// 还原
// ============================================================

/// 从某份快照还原到本机（整份替换，含 v1/v2 自动识别）。
pub fn restore(backup_ref: &str, mut on_progress: impl FnMut(&str)) -> Result<RestoreResult, String> {
    // 还原是**整份替换**客户当前的对话和设置 —— 全项目破坏性最强的一个动作。
    // 出问题（还原了错的快照 / 中途失败留下半套数据）时，没有日志就只能听客户描述，
    // 而客户描述不出「它当时还原到哪一步」。
    crate::ulog::section("backup", &format!("开始还原快照 {backup_ref}"));
    let mut on_progress = move |m: &str| {
        crate::ulog::write("backup", m);
        on_progress(m);
    };
    let p = PathBuf::from(backup_ref);

    // v1 旧快照目录：<ts>/ 下有 manifest.json + 各应用子目录 → 走兼容还原。
    if p.is_dir() && p.join("manifest.json").is_file() {
        return restore_v1_dir(&p, on_progress);
    }
    if !p.is_file() {
        return Err(format!("快照不存在：{backup_ref}"));
    }

    let text = std::fs::read_to_string(&p).map_err(|e| format!("读取快照清单失败: {e}"))?;
    let manifest: ManifestV2 = serde_json::from_str(&text).map_err(|e| format!("快照清单解析失败: {e}"))?;
    // <机器>/snapshots/<ts>.json → blobs 在 <机器>/blobs
    let machine_root = p.parent().and_then(|d| d.parent()).ok_or("无法定位 blobs 目录")?.to_path_buf();
    let blobs = machine_root.join("blobs");

    // ③ 先校验：本快照引用的所有 blob 必须在位（防 U 盘坏块/半写后铺半套）。
    on_progress("校验快照完整性…");
    for app in &manifest.apps {
        for f in &app.files {
            if !blob_path(&blobs, &f.hash).is_file() {
                return Err(format!(
                    "快照不完整：缺少 {} 的文件块（{}…）。U 盘可能损坏，已中止还原，本机数据未改动。",
                    app.label,
                    &f.hash[..f.hash.len().min(12)]
                ));
            }
        }
    }

    // 先关会锁库的 app
    on_progress("正在关闭 ClawX / Claude 桌面版 / Obsidian（如在运行）… 不会动你正在跑的 AI 命令行会话");
    kill_locking_apps();
    std::thread::sleep(std::time::Duration::from_millis(800));

    // ① 还原前自动把当前状态快照一份（安全网）。不裁剪/GC，免删到本次要还原的 blob。
    let mut pre_backup_dir = None;
    if let Some(usb_root) = usb_root_of(&p) {
        on_progress("还原前先自动备份当前状态（安全网）…");
        match backup_inner(&usb_root, env!("CARGO_PKG_VERSION"), false, |m| on_progress(m)) {
            Ok(r) => pre_backup_dir = Some(r.dir),
            Err(e) => on_progress(&format!("⚠️ 还原前自动备份失败：{e}（继续，旧数据仍会改名留底以便手动回滚）")),
        }
    }

    let ts = now_ms();

    // ── 阶段 1：把每个应用都重建到各自同盘临时目录(staging)，全程不碰任何真实目录。 ──
    // blob 拷贝失败（目标盘写满 / U 盘读错 / 校验后被删的 TOCTOU）只会发生在这一阶段，此时**没有
    // 任何应用被换过**，清掉已建 staging 即原数据分毫未动 —— 这才让「未改动」承诺对所有应用都成立
    // （旧实现单循环边建边换，第 N 个失败时前 N-1 个已换上位，却仍报「未改动」= B-1）。
    struct Staged {
        target: PathBuf,
        staging: PathBuf,
        name: String,
        label: String,
        files: u64,
        bytes: u64,
    }
    // 失败时清掉已建的 staging（不碰真实目录）；extra = 当前正在建、还没入列表的那个。
    let cleanup = |list: &[Staged], extra: Option<&Path>| {
        for s in list {
            let _ = std::fs::remove_dir_all(&s.staging);
        }
        if let Some(p) = extra {
            let _ = std::fs::remove_dir_all(p);
        }
    };
    let mut staged: Vec<Staged> = Vec::new();

    for app in &manifest.apps {
        let Some(target) = restore_target(&app.name, &app.root) else { continue };
        on_progress(&format!("正在准备「{}」…", app.label));
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let staging = sibling_with_suffix(&target, &format!("uking-new-{ts}"));
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(e) = std::fs::create_dir_all(&staging) {
            cleanup(&staged, Some(&staging));
            return Err(format!("创建临时目录失败 {}: {e}（原数据未改动，可重试）", staging.display()));
        }

        let mut files = 0u64;
        let mut bytes = 0u64;
        let mut failed: Option<String> = None;
        for f in &app.files {
            let dst = staging.join(&f.rel);
            if let Some(parent) = dst.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    failed = Some(format!("建目录失败 {}: {e}", parent.display()));
                    break;
                }
            }
            let blob = blob_path(&blobs, &f.hash);
            match std::fs::copy(&blob, &dst) {
                Ok(n) => {
                    files += 1;
                    bytes += n;
                }
                Err(e) => {
                    failed = Some(format!("还原文件失败 {}: {e}", f.rel));
                    break;
                }
            }
        }
        if let Some(e) = failed {
            cleanup(&staged, Some(&staging));
            return Err(format!("还原「{}」时失败：{e}（原数据未改动，可重试）", app.label));
        }

        staged.push(Staged {
            target,
            staging,
            name: app.name.clone(),
            label: app.label.clone(),
            files,
            bytes,
        });
    }

    // ── 阶段 2：所有 staging 已就绪，逐个原子换上位（旧目录改名留底 → staging 改名上位）。 ──
    // 只剩快速 rename，极少失败；中途任一应用失败即把**已换好的全部回滚**到留底，保证「要么全换好、
    // 要么全回原样」。archived 仅在成功路径返回；失败一律 return Err 丢弃，故回滚分支无需维护它。
    let mut stats: Vec<ItemStat> = Vec::new();
    let mut archived: Vec<String> = Vec::new();
    // 已换好的 (真实目录, 其留底 bak)。回滚见 rollback_swaps（提成自由函数以便单测）。
    let mut committed: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();

    for (i, s) in staged.iter().enumerate() {
        on_progress(&format!("正在还原「{}」…", s.label));
        // ⑤ 旧目录改名留底
        let mut bak: Option<PathBuf> = None;
        if s.target.exists() {
            let b = sibling_with_suffix(&s.target, &format!("uking-bak-{ts}"));
            if let Err(e) = std::fs::rename(&s.target, &b) {
                // 本应用还没换；回滚之前已换好的，清掉本应用及之后尚未上位的 staging。
                rollback_swaps(&committed);
                for rest in &staged[i..] {
                    let _ = std::fs::remove_dir_all(&rest.staging);
                }
                return Err(format!(
                    "无法移开旧的「{}」（{}）：{e}。请先彻底退出对应应用再试。（已回滚，原数据未改动）",
                    s.label,
                    s.target.display()
                ));
            }
            archived.push(b.display().to_string());
            bak = Some(b);
        }
        // ⑥ staging 原子上位
        if let Err(e) = std::fs::rename(&s.staging, &s.target) {
            // 先把本应用的留底改回来，再回滚之前已换好的，清掉之后尚未上位的 staging。
            if let Some(b) = &bak {
                let _ = std::fs::rename(b, &s.target);
            }
            let _ = std::fs::remove_dir_all(&s.staging);
            rollback_swaps(&committed);
            for rest in &staged[i + 1..] {
                let _ = std::fs::remove_dir_all(&rest.staging);
            }
            return Err(format!("还原「{}」替换失败：{e}（已回滚，原数据未改动）", s.label));
        }
        committed.push((s.target.clone(), bak));
        stats.push(ItemStat {
            name: s.name.clone(),
            label: s.label.clone(),
            files: s.files,
            bytes: s.bytes,
            present: true,
        });
    }

    // 全部换好后再裁剪留底（避免回滚阶段误删要用的留底）。
    for (target, _) in &committed {
        prune_old_baks(target);
    }

    on_progress("还原完成");
    Ok(RestoreResult { items: stats, pre_backup_dir, archived })
}

/// 兼容：还原 v1 老快照目录（<ts>/<app>/...files...，直接整目录拷回）。
fn restore_v1_dir(snap: &Path, mut on_progress: impl FnMut(&str)) -> Result<RestoreResult, String> {
    on_progress("检测到旧版(v1)快照，按兼容方式还原…");
    kill_locking_apps();
    std::thread::sleep(std::time::Duration::from_millis(800));

    let ts = now_ms();
    let mut stats = Vec::new();
    let mut archived = Vec::new();
    // v1 只有 clawx / openclaw 两项，名字即子目录名。
    for (name, label) in [("clawx", "ClawX 对话与设置"), ("openclaw", "OpenClaw 龙虾工作区")] {
        let src = snap.join(name);
        if !src.is_dir() {
            continue;
        }
        let Some(target) = restore_target(name, "") else { continue };
        on_progress(&format!("正在还原「{label}」…"));
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let staging = sibling_with_suffix(&target, &format!("uking-new-{ts}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| format!("创建临时目录失败: {e}"))?;
        let mut files = 0u64;
        let mut bytes = 0u64;
        if let Err(e) = copy_tree(&src, &staging, &mut |n| {
            files += 1;
            bytes += n;
        }) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("还原「{label}」复制失败：{e}（原数据未改动）"));
        }
        if target.exists() {
            let bak = sibling_with_suffix(&target, &format!("uking-bak-{ts}"));
            match std::fs::rename(&target, &bak) {
                Ok(_) => archived.push(bak.display().to_string()),
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(format!("无法移开旧「{label}」：{e}（原数据未改动）"));
                }
            }
        }
        if let Err(e) = std::fs::rename(&staging, &target) {
            if let Some(bak) = archived.pop() {
                let _ = std::fs::rename(&bak, &target);
            }
            return Err(format!("还原「{label}」替换失败：{e}（已回滚）"));
        }
        prune_old_baks(&target);
        stats.push(ItemStat { name: name.into(), label: label.into(), files, bytes, present: true });
    }
    on_progress("还原完成");
    Ok(RestoreResult { items: stats, pre_backup_dir: None, archived })
}

/// 朴素递归拷贝（v1 兼容还原用）。
fn copy_tree(src: &Path, dst: &Path, on_file: &mut impl FnMut(u64)) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("建目录失败 {}: {e}", dst.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读目录失败 {}: {e}", src.display()))?.flatten() {
        let p = entry.path();
        let d = dst.join(entry.file_name());
        if p.is_dir() {
            copy_tree(&p, &d, on_file)?;
        } else if p.is_file() {
            match std::fs::copy(&p, &d) {
                Ok(n) => on_file(n),
                Err(e) => {
                    if !d.exists() {
                        return Err(format!("拷贝 {} 失败: {e}", p.display()));
                    }
                }
            }
        }
    }
    Ok(())
}

// ============================================================
// 裁剪 / GC
// ============================================================

/// 从快照清单文件往上找用户选的 U 盘根：`<root>/U-King-Sync/<机器>/snapshots/<ts>.json` → `<root>`。
fn usb_root_of(snap_file: &Path) -> Option<String> {
    // snapshots/<ts>.json → snapshots → <机器> → U-King-Sync → <root>
    let machine = snap_file.parent()?.parent()?; // <机器>
    let sync = machine.parent()?; // U-King-Sync
    if sync.file_name().map(|n| n == SYNC_DIR).unwrap_or(false) {
        return sync.parent().map(|r| r.display().to_string());
    }
    None
}

fn sibling_with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let parent = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
    parent.join(format!("{name}.{suffix}"))
}

/// 还原阶段 2 的回滚：把已换好的应用反向恢复——撤掉刚上位的新目录，再把留底改名回原位。
/// `done` = 已成功换上位的 `(真实目录, 其留底 bak)`；bak=None 表示该目录原本不存在（只需删掉新建的）。
/// 反向遍历，让最后换的最先撤回。
fn rollback_swaps(done: &[(PathBuf, Option<PathBuf>)]) {
    for (target, bak) in done.iter().rev() {
        let _ = std::fs::remove_dir_all(target);
        if let Some(b) = bak {
            let _ = std::fs::rename(b, target);
        }
    }
}

/// 裁剪到最近 `keep_snapshots()` 份 v2 快照，并 GC 掉没有任何留存快照引用的 blob。
fn prune_snapshots(machine_root: &Path) {
    let snaps_dir = machine_root.join("snapshots");
    let blobs = machine_root.join("blobs");

    // 收集 (ts, path)
    let Ok(rd) = std::fs::read_dir(&snaps_dir) else { return };
    let mut snaps: Vec<(u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                return None;
            }
            let ts = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            Some((ts, p))
        })
        .collect();

    let keep = keep_snapshots();
    if snaps.len() > keep {
        snaps.sort_by(|a, b| b.0.cmp(&a.0)); // 新→旧
        for (_, p) in snaps.iter().skip(keep) {
            let _ = std::fs::remove_file(p);
        }
        snaps.truncate(keep);
    }

    // GC：留存快照引用的 hash 集合
    let mut referenced: HashSet<String> = HashSet::new();
    for (_, p) in &snaps {
        if let Ok(text) = std::fs::read_to_string(p) {
            if let Ok(man) = serde_json::from_str::<ManifestV2>(&text) {
                for app in man.apps {
                    for f in app.files {
                        referenced.insert(f.hash);
                    }
                }
            }
        }
    }

    // 扫 blobs/<aa>/<hash>，删未被引用的
    let Ok(subs) = std::fs::read_dir(&blobs) else { return };
    for sub in subs.flatten() {
        if !sub.path().is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(sub.path()) else { continue };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if name.ends_with(".tmp") {
                let _ = std::fs::remove_file(f.path()); // 清理残留临时块
                continue;
            }
            if !referenced.contains(&name) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
}

/// 还原留底（`<name>.uking-bak-<ts>`）每个目标只留最近 KEEP_BAKS 份。
fn prune_old_baks(target: &Path) {
    let Some(parent) = target.parent() else { return };
    let Some(name) = target.file_name().map(|n| n.to_string_lossy().to_string()) else { return };
    let prefix = format!("{name}.uking-bak-");
    let Ok(rd) = std::fs::read_dir(parent) else { return };
    let mut baks: Vec<(u64, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let fname = e.file_name().to_string_lossy().to_string();
            let ts = fname.strip_prefix(&prefix)?.parse::<u64>().unwrap_or(0);
            Some((ts, e.path()))
        })
        .collect();
    if baks.len() <= KEEP_BAKS {
        return;
    }
    baks.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in baks.into_iter().skip(KEEP_BAKS) {
        let _ = std::fs::remove_dir_all(&p);
    }
}

// ============================================================
// 测试（沙箱：把 APPDATA / USERPROFILE 指到临时目录，全程不碰真实数据）
// ============================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 借全进程唯一的那把沙箱锁 —— 这里曾经是**第三把**本地 `Mutex`（providers 一把、
    /// org 一把、这儿一把），各锁各的等于没锁：这几条用例改的 `USERPROFILE` 正是
    /// 别的模块在没有 `UKING_TEST_HOME` 时解析家目录要读的东西。
    ///
    /// 用 `enter_raw` 而不是 `enter`：这几条用例自己造 `USERPROFILE`/`APPDATA` 当家目录，
    /// 硬塞一个 `UKING_TEST_HOME` 会改掉它们的 home 解析结果。沙箱在这里只做两件事 ——
    /// 互斥，以及出作用域时把这些变量还原回去（老写法压根不还原）。
    fn env_sandbox(tag: &str) -> crate::testsandbox::Sandbox {
        crate::testsandbox::enter_raw(tag)
    }
    fn w(p: &Path, c: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, c).unwrap();
    }
    fn r(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }
    fn count_blobs(machine_root: &Path) -> usize {
        let blobs = machine_root.join("blobs");
        let mut n = 0;
        if let Ok(subs) = fs::read_dir(&blobs) {
            for sub in subs.flatten() {
                if sub.path().is_dir() {
                    if let Ok(fs2) = fs::read_dir(sub.path()) {
                        n += fs2.flatten().filter(|e| !e.file_name().to_string_lossy().ends_with(".tmp")).count();
                    }
                }
            }
        }
        n
    }
    fn count_snaps(machine_root: &Path) -> usize {
        fs::read_dir(machine_root.join("snapshots"))
            .map(|rd| rd.flatten().filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json")).count())
            .unwrap_or(0)
    }
    fn count_baks(parent: &Path, name: &str) -> usize {
        let prefix = format!("{name}.uking-bak-");
        fs::read_dir(parent)
            .map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&prefix)).count())
            .unwrap_or(0)
    }

    /// 回归：还原前的「关应用」名单里绝不能出现与 AI CLI 撞名的镜像名。
    ///
    /// 事故原样：`taskkill /IM Claude.exe /F /T`。Windows 镜像名不区分大小写，而 Claude Code
    /// CLI 的可执行文件就叫 `claude.exe`，于是 `/IM`（杀全部同名）+ `/T`（杀整树）把机器上
    /// 所有 Claude Code 会话一起端掉 —— 包括跑这个 `cargo test` 的那个会话本身。
    #[test]
    #[cfg(windows)]
    fn locking_image_names_never_shadow_ai_clis() {
        for im in LOCKING_IMAGE_NAMES {
            for cli in ["claude.exe", "node.exe", "codex.exe", "openclaw.exe", "hermes.exe"] {
                assert!(
                    !im.eq_ignore_ascii_case(cli),
                    "「{im}」会连 AI CLI 的同名进程一起杀；桌面版只能按安装路径逐 PID 关"
                );
            }
        }
    }

    /// 备份→去重→还原→裁剪/GC，一条龙在沙箱里跑。
    #[test]
    fn backup_dedup_restore_prune_gc() {
        let _g = env_sandbox("backup-env");
        std::env::set_var("UKING_KEEP_SNAPSHOTS", "3");
        let base = std::env::temp_dir().join(format!("uking-bk2-{}", now_ms()));
        let appdata = base.join("appdata");
        let home = base.join("home");
        let usb = match std::env::var("UKING_TEST_USB_ROOT") {
            Ok(p) if !p.is_empty() => PathBuf::from(p).join(format!("uking-bk2-{}", now_ms())),
            _ => base.join("usb"),
        };
        fs::create_dir_all(&usb).unwrap();
        std::env::set_var("APPDATA", &appdata);
        std::env::set_var("LOCALAPPDATA", appdata.parent().unwrap().join("Local"));
        std::env::set_var("USERPROFILE", &home);
        std::env::remove_var("HOME");

        // 源：claude-cli(含应跳的 plugins) + openclaw(含应跳的 logs) + clawx(含应跳的 Cache)
        let claude = home.join(".claude");
        w(&claude.join("settings.json"), "cfg-A");
        w(&claude.join("projects/p1/sess.jsonl"), "chat-A");
        w(&claude.join("plugins/big.bin"), "SHOULD-SKIP");
        let oc = home.join(".openclaw");
        w(&oc.join("state.db"), "db-A");
        w(&oc.join("logs/x.log"), "SKIP-LOG");
        let clawx = appdata.join("ClawX");
        w(&clawx.join("clawx-providers.json"), "prov-A");
        w(&clawx.join("Cache/c.bin"), "SKIP-CACHE");

        // 1) 备份
        let bk = backup(usb.to_str().unwrap(), "test", |_| {}).expect("backup 应成功");
        let machine_root = PathBuf::from(&bk.dir).parent().unwrap().parent().unwrap().to_path_buf();
        let blobs_after_1 = count_blobs(&machine_root);
        // 跳过项不应入快照：providers/settings/projects/state.db = 4 个唯一内容
        assert_eq!(blobs_after_1, 4, "应只有 4 个唯一文件块（跳过 plugins/logs/Cache），实际 {blobs_after_1}");
        assert_eq!(count_snaps(&machine_root), 1);

        // 2) 原样再备份一次 → 去重，blob 数不变，快照 +1
        std::thread::sleep(std::time::Duration::from_millis(3)); // 错开毫秒快照名
        let _ = backup(usb.to_str().unwrap(), "test", |_| {}).expect("二次 backup 应成功");
        assert_eq!(count_blobs(&machine_root), 4, "未变数据二次备份应零新增块（去重）");
        assert_eq!(count_snaps(&machine_root), 2);

        // 3) 改一个文件再备份 → 多 1 个块
        std::thread::sleep(std::time::Duration::from_millis(3));
        w(&claude.join("settings.json"), "cfg-B");
        let _ = backup(usb.to_str().unwrap(), "test", |_| {}).expect("三次 backup 应成功");
        assert_eq!(count_blobs(&machine_root), 5, "改 1 文件应只多 1 个块");
        assert_eq!(count_snaps(&machine_root), 3);

        // 4) 本机继续改，从第一份快照还原 → 内容回到 A 版
        w(&claude.join("settings.json"), "cfg-LOCAL-MODIFIED");
        w(&oc.join("state.db"), "db-MODIFIED");
        let entries = list(usb.to_str().unwrap());
        assert!(entries.len() >= 3);
        let oldest = entries.iter().min_by_key(|e| e.created_at).unwrap().clone();
        let rr = restore(&oldest.dir, |_| {}).expect("restore 应成功");
        assert_eq!(r(&claude.join("settings.json")), "cfg-A", "settings 应还原到 A");
        assert_eq!(r(&oc.join("state.db")), "db-A", "openclaw 应还原到 A");
        assert_eq!(r(&clawx.join("clawx-providers.json")), "prov-A", "clawx 应还原");
        assert!(!rr.archived.is_empty(), "旧数据应改名留底");
        assert!(rr.pre_backup_dir.is_some(), "还原前安全备份应成功");

        // 5) 裁剪：KEEP=3，多次备份后 v2 快照数 ≤ 3
        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(3)); // 错开毫秒快照名
            w(&claude.join("settings.json"), &format!("cfg-iter-{i}"));
            backup(usb.to_str().unwrap(), "test", |_| {}).expect("循环 backup 应成功");
        }
        assert!(count_snaps(&machine_root) <= 3, "v2 快照应裁剪到 ≤ 3，实际 {}", count_snaps(&machine_root));
        let nb = count_baks(&home.join(".claude"), ".claude");
        assert!(nb <= KEEP_BAKS, ".claude 留底应 ≤ {KEEP_BAKS}");

        std::env::remove_var("UKING_KEEP_SNAPSHOTS");
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&usb);
    }

    /// 完整性：blob 缺失（U 盘坏块）时，还原必须拒绝且原数据分毫不动。
    #[test]
    fn restore_missing_blob_keeps_target_intact() {
        let _g = env_sandbox("backup-env");
        let base = std::env::temp_dir().join(format!("uking-bk2fail-{}", now_ms() + 7));
        let appdata = base.join("appdata");
        let home = base.join("home");
        let usb = base.join("usb");
        fs::create_dir_all(&usb).unwrap();
        std::env::set_var("APPDATA", &appdata);
        std::env::set_var("LOCALAPPDATA", appdata.parent().unwrap().join("Local"));
        std::env::set_var("USERPROFILE", &home);
        std::env::remove_var("HOME");

        let oc = home.join(".openclaw");
        w(&oc.join("state.db"), "PRECIOUS-ORIGINAL");

        let bk = backup(usb.to_str().unwrap(), "test", |_| {}).expect("backup 应成功");
        let machine_root = PathBuf::from(&bk.dir).parent().unwrap().parent().unwrap().to_path_buf();

        // 本机改数据
        w(&oc.join("state.db"), "LOCAL-NOW");
        // 破坏 U 盘：删掉所有 blob（模拟坏块/半写）
        let blobs = machine_root.join("blobs");
        for sub in fs::read_dir(&blobs).unwrap().flatten() {
            if sub.path().is_dir() {
                for f in fs::read_dir(sub.path()).unwrap().flatten() {
                    fs::remove_file(f.path()).unwrap();
                }
            }
        }

        let res = restore(&bk.dir, |_| {});
        assert!(res.is_err(), "缺 blob 应拒绝还原");
        assert_eq!(r(&oc.join("state.db")), "LOCAL-NOW", "拒绝还原后本机数据必须分毫不动");
        assert!(count_baks(&home.join(".openclaw"), ".openclaw") == 0, "失败不应产生留底");

        let _ = fs::remove_dir_all(&base);
    }

    /// Obsidian：从 obsidian.json 解析 vault 路径并备份整库，排除 workspace.json。
    #[test]
    fn obsidian_vault_discovery_and_exclude() {
        let _g = env_sandbox("backup-env");
        let base = std::env::temp_dir().join(format!("uking-obs-{}", now_ms() + 11));
        let appdata = base.join("appdata");
        let home = base.join("home");
        let usb = base.join("usb");
        fs::create_dir_all(&usb).unwrap();
        std::env::set_var("APPDATA", &appdata);
        std::env::set_var("LOCALAPPDATA", appdata.parent().unwrap().join("Local"));
        std::env::set_var("USERPROFILE", &home);
        std::env::remove_var("HOME");

        // vault 在沙箱里
        let vault = home.join("MyNotes");
        w(&vault.join("note.md"), "# hello");
        w(&vault.join(".obsidian/app.json"), "{settings}");
        w(&vault.join(".obsidian/workspace.json"), "SHOULD-SKIP-CHURN");
        // obsidian.json 指针表（path 用正斜杠，serde_json 会原样读）
        let vpath = vault.display().to_string().replace('\\', "/");
        w(
            &appdata.join("obsidian/obsidian.json"),
            &format!(r#"{{"vaults":{{"abc":{{"path":"{vpath}","ts":1,"open":true}}}}}}"#),
        );

        let bk = backup(usb.to_str().unwrap(), "test", |_| {}).expect("backup 应成功");
        // vault 项应出现且包含 note.md / app.json，但不含 workspace.json
        let has_vault = bk.items.iter().any(|i| i.name.starts_with("vault:") && i.present);
        assert!(has_vault, "应发现并备份 vault");

        // 从快照清单里确认 workspace.json 被排除、note.md 在
        let snap_text = fs::read_to_string(&bk.dir).unwrap();
        assert!(snap_text.contains("note.md"), "vault 笔记应入快照");
        assert!(snap_text.contains(".obsidian/app.json"), "vault 配置应入快照");
        assert!(!snap_text.contains("workspace.json"), "workspace.json 应被排除");

        // 还原到新机（删掉本机 vault，看是否回到原路径）
        fs::remove_dir_all(&vault).unwrap();
        let _ = restore(&bk.dir, |_| {}).expect("restore 应成功");
        assert_eq!(r(&vault.join("note.md")), "# hello", "vault 应还原回原路径");
        assert!(!vault.join(".obsidian/workspace.json").exists(), "被排除项不应被还原");

        let _ = fs::remove_dir_all(&base);
    }

    /// 阶段 2 回滚原语：撤掉已换上的新目录、把留底改名回原位；bak=None 时只删不恢复。
    /// 这是「多应用还原中途失败 → 已换好的全部回滚」(B-1 修复) 最难的一段，单独确定性验证。
    #[test]
    fn rollback_swaps_restores_baks() {
        let base = std::env::temp_dir().join(format!("uking-rb-{}", now_ms() + 23));
        // 应用 A：原本存在（有留底）→ 回滚后应恢复 OLD-A、bak 改名回原位、刚换上的 NEW-A 被撤。
        let ta = base.join("a");
        let ba = base.join("a.uking-bak-X");
        w(&ta.join("f"), "NEW-A");
        w(&ba.join("f"), "OLD-A");
        // 应用 B：原本不存在（bak=None）→ 回滚后整目录应被删掉。
        let tb = base.join("b");
        w(&tb.join("f"), "NEW-B");

        rollback_swaps(&[(ta.clone(), Some(ba.clone())), (tb.clone(), None)]);

        assert_eq!(r(&ta.join("f")), "OLD-A", "A 应恢复到留底内容");
        assert!(!ba.exists(), "A 的留底应已改名回原位、不再独立存在");
        assert!(!tb.exists(), "B 原本不存在，回滚应整目录删除");

        let _ = fs::remove_dir_all(&base);
    }
}
