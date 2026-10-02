//! 闭源桌面应用（豆包工作 / 千问办公 / WorkBuddy / Obsidian / UU远程）的
//! 「装没装？主程序在哪？什么版本？」探测，以及「启动本地程序」。
//!
//! ## 为什么走卸载表，而不是猜目录
//! 这几个都是官网安装包装出来的闭源软件：U-King 既不装也不管它们的配置，只要**认得出它们在本机**、
//! 点「打开」能把本地程序拉起来。此前 `list_tools()` 里豆包 / 千问办公 / WorkBuddy 的 `installed`
//! 直接写死 `false`，于是明明装了、卡片还是「去官网下载」，点一下又把客户带去下载页。
//!
//! 它们的落点五花八门（`%LOCALAPPDATA%\DoubaoWork\Application`、`%LOCALAPPDATA%\Programs\QwenWorkCN`、
//! `C:\Program Files\WorkBuddy`），安装目录还能被用户改掉；唯一稳定的是 Windows 认定「装没装」的权威处
//! —— 卸载表（`...\CurrentVersion\Uninstall`）。判据思路取自 EchoBird 的 `scan_windows_registry`
//! （只学检测思路，不抄代码）：遍历三处卸载表，按**子键名精确匹配**或 **DisplayName 精确 / 前缀匹配**
//! 找到条目，再从 `DisplayIcon` → `InstallLocation` → `UninstallString` 父目录依次找主程序 exe。
//!
//! ## 🔴 踩过 / 实测过的形状（2026-10-02 本机只读实测，路径一律以占位写进测试夹具）
//! - 豆包工作：子键 `DoubaoWork`，`DisplayIcon` 是带引号的 `...\Application\icon.ico`（**是 .ico 不是 exe**），
//!   没有 `InstallLocation`；主程序在同目录 `DoubaoWork.exe`。消费版「豆包」是另一个子键 `Doubao` /
//!   `Doubao.exe`，DisplayName 只有「豆包」两个字——所以绝不能对「豆包」做前缀匹配（会把两个产品混成一个）。
//! - 千问办公：子键是 **GUID**（只能靠 DisplayName 认），`DisplayIcon` 是 `...\QwenWorkCN\uninstallerIcon.ico`
//!   （同样是 .ico）；主程序是根目录的 `Launcher.exe`（开始菜单快捷方式指向它，真正的 `QwenWorkCN.exe`
//!   在 `1.2.0-xxxx\` 这类带版本号的子目录里，随自动更新换目录）。**`DisplayVersion` 是装机时的版本，
//!   自动更新后不会回写**（本机注册表 1.0.4，磁盘上实际在跑 1.2.0），所以这里给出的版本号对它只能当参考。
//! - WorkBuddy：两个并存的产品——`WorkBuddy 5.7.3`（国内版）与 `WorkBuddy AI 5.6.2`（海外版），都是 GUID 子键、
//!   `DisplayName` 带版本号（要前缀匹配）、`DisplayIcon` 是 `C:\Program Files\...\X.exe,0`（带 `,0` 图标序号）。
//!   国内版优先（见 `find_in` 的 rank 规则）。
//! - Obsidian / UU远程：`DisplayIcon` 直接是主程序 exe（`Obsidian.exe,0` / `GameViewer.exe`）。
//!
//! ## 注册表输出是 GBK
//! `reg.exe` 在中文 Windows 上吐的是系统代码页（GBK），DisplayName 是中文——所以输出走
//! `installer::decode_console`（合法 UTF-8 原样，否则按系统 ANSI 页解），**不是** `from_utf8_lossy`。
//! 非中文系统上 reg.exe 会把中文输出成 `?`，此时 DisplayName 认不出，靠「子键名」和「固定落点」兜底。
//!
//! ## 代价控制
//! `list_tools()` 是同步高频调用（`plan_all` 甚至对每个工具各调一次），不能每次问五遍注册表：
//! 三处卸载表各 `reg query <hive> /s` 一次（并发），解析成条目快照，**10 秒内复用**；`reg.exe`
//! 单次最多等 5 秒（超时就杀掉，当作「这处卸载表读不到」）。快照只缓存「条目」，**exe 文件在不在
//! 每次都现查**——所以卸载后立刻显示未装；刚装好的最坏滞后一个 TTL，而默认落点另有固定路径兜底，
//! 即时生效。
//!
//! 纯 `std`、不引依赖。纯字符串解析的部分不套 `cfg(windows)`（夹具测试在任何平台都能跑）；
//! 路径一律按 Windows 写法的字符串处理（`std::path::Path` 在非 Windows 上不认 `\`）。

#![cfg_attr(not(windows), allow(dead_code))]

use std::path::PathBuf;

/// 一个桌面应用的识别规则。
pub struct AppRule {
    /// 稳定 id，等于 `tools::TOOL_SPECS` / `list_tools()` 里同一工具的 id，也是 `launch_app` 的键。
    pub id: &'static str,
    /// 给人看的名字（错误提示用）。
    pub label: &'static str,
    /// 卸载表子键名，精确匹配（忽略 ASCII 大小写）。
    pub key_names: &'static [&'static str],
    /// DisplayName 精确匹配（忽略 ASCII 大小写）。
    pub display_names: &'static [&'static str],
    /// DisplayName 前缀匹配（忽略 ASCII 大小写）——容忍名字里带版本号（`WorkBuddy 5.7.3`）。
    pub display_prefixes: &'static [&'static str],
    /// 约定的主程序文件名，**按优先级排**：`DisplayIcon` 不是 exe（.ico）时在候选目录里按这个顺序找；
    /// 多个条目同时命中时（WorkBuddy 国内版 + 海外版），也按它决定用哪一个。
    pub exe_names: &'static [&'static str],
    /// 注册表里查不到时的固定落点（`%VAR%` 模板，含 exe 文件名）。只在默认安装位置才命中，是兜底。
    pub fallback_exes: &'static [&'static str],
}

pub const RULES: &[AppRule] = &[
    AppRule {
        id: "claude-app",
        label: "Claude 桌面版",
        key_names: &["AnthropicClaude"],
        display_names: &["Claude"],
        display_prefixes: &[],
        exe_names: &["Claude.exe", "claude.exe"],
        fallback_exes: &[
            "%LOCALAPPDATA%\\AnthropicClaude\\Claude.exe",
            "%LOCALAPPDATA%\\Programs\\Claude\\Claude.exe",
        ],
    },
    AppRule {
        id: "doubao",
        label: "豆包工作",
        key_names: &["DoubaoWork"],
        display_names: &["豆包工作"],
        display_prefixes: &[],
        exe_names: &["DoubaoWork.exe"],
        fallback_exes: &["%LOCALAPPDATA%\\DoubaoWork\\Application\\DoubaoWork.exe"],
    },
    AppRule {
        id: "qwenwork",
        label: "千问办公",
        key_names: &[],
        display_names: &["千问办公"],
        display_prefixes: &["千问办公"],
        exe_names: &["Launcher.exe", "QwenWorkCN.exe"],
        fallback_exes: &["%LOCALAPPDATA%\\Programs\\QwenWorkCN\\Launcher.exe"],
    },
    AppRule {
        id: "workbuddy",
        label: "WorkBuddy",
        key_names: &[],
        display_names: &[],
        // 「WorkBuddy 5.7.3」（国内版）与「WorkBuddy AI 5.6.2」（海外版）都以它开头，两个都认；
        // 谁优先由 exe_names 的顺序定（国内版 WorkBuddy.exe 在前）。
        display_prefixes: &["WorkBuddy"],
        exe_names: &["WorkBuddy.exe", "WorkBuddyAI.exe"],
        fallback_exes: &[
            "%ProgramFiles%\\WorkBuddy\\WorkBuddy.exe",
            "%ProgramFiles%\\WorkBuddyAI\\WorkBuddyAI.exe",
            // 以下两条是 electron-builder「仅当前用户」的默认落点，本机没有这种装法，未实测，只做存在性探测。
            "%LOCALAPPDATA%\\Programs\\WorkBuddy\\WorkBuddy.exe",
            "%LOCALAPPDATA%\\Programs\\WorkBuddyAI\\WorkBuddyAI.exe",
        ],
    },
    AppRule {
        id: "obsidian",
        label: "Obsidian",
        key_names: &[],
        // 精确匹配：别把「Obsidian Entertainment」之类带前缀的认成它。
        display_names: &["Obsidian"],
        display_prefixes: &[],
        exe_names: &["Obsidian.exe"],
        fallback_exes: &[
            // 前两条是旧 `obsidian_installed()` 一直在探的落点（现在并到这一份，不再各写一遍）。
            "%LOCALAPPDATA%\\Obsidian\\Obsidian.exe",
            "%ProgramFiles%\\Obsidian\\Obsidian.exe",
            "%LOCALAPPDATA%\\Programs\\Obsidian\\Obsidian.exe",
        ],
    },
    AppRule {
        id: "uu-remote",
        label: "UU远程",
        key_names: &["GameViewer", "UURemote"],
        display_names: &["UU远程", "网易UU远程"],
        display_prefixes: &[],
        exe_names: &["GameViewer.exe", "UURemote.exe"],
        fallback_exes: &[
            "%ProgramFiles%\\Netease\\GameViewer\\GameViewer.exe",
            "%ProgramFiles(x86)%\\Netease\\GameViewer\\GameViewer.exe",
            "%LOCALAPPDATA%\\Programs\\Netease\\GameViewer\\GameViewer.exe",
        ],
    },
];

/// 按 id 取规则。
pub fn rule_of(id: &str) -> Option<&'static AppRule> {
    RULES.iter().find(|r| r.id == id)
}

/// 探测结果：主程序路径 + 卸载表里的 `DisplayVersion`（固定落点兜底命中时没有）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundApp {
    pub exe: PathBuf,
    pub version: Option<String>,
}

/// 卸载表里一个直接子键的快照（只留我们要用的几个值）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UninstallEntry {
    /// 子键名（路径最后一段）。
    pub key: String,
    pub display_name: String,
    pub display_version: Option<String>,
    /// 原始 `DisplayIcon`（`reg_value` 只剥了首尾成对引号，`"path",0` 这种形状还没拆）。
    pub display_icon: Option<String>,
    pub install_location: Option<String>,
    pub uninstall_string: Option<String>,
}

// ───────────────────────── 纯字符串解析（可在任何平台测） ─────────────────────────

/// 拆 `DisplayIcon`：去掉包裹引号和 `,N` 图标序号，只留路径。
///
/// 形状全集（都是真机见过或规范允许的）：
/// `"C:\x\a.exe",0` / `C:\x\a.exe,0` / `C:\x\a.exe,-101`（负数 = 资源 id）/ `C:\x\icon.ico` /
/// `"C:\x\icon.ico"` / 带 `%VAR%` 的 REG_EXPAND_SZ / 空串。拆不出路径返回 `None`。
pub(crate) fn parse_display_icon(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let path = if let Some(rest) = t.strip_prefix('"') {
        // 引号包着路径，后面可能还跟 `,0`：取到下一个引号为止；没有收尾引号就取剩下的全部。
        match rest.find('"') {
            Some(i) => &rest[..i],
            None => rest,
        }
    } else {
        // 没引号：去掉末尾的 `,整数`。路径里本身带逗号（极少见）时，尾巴不是整数，不会被误砍。
        match t.rsplit_once(',') {
            Some((head, tail)) if tail.trim().parse::<i32>().is_ok() => head,
            _ => t,
        }
    };
    let path = path.trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// 从一条命令行（`UninstallString` 一类：`"C:\x\u.exe" /S`、`C:\Program Files\x\unins000.exe /SILENT`）
/// 里取出 exe 路径。优先认 `.exe` 的结尾——未加引号的路径里有空格也能取全。
pub(crate) fn exe_path_from_command(raw: &str) -> Option<String> {
    let t = raw.trim().trim_start_matches('"');
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let end = match lower.find(".exe") {
        Some(i) => i + 4,
        // 没有 .exe（msiexec 之类也带 .exe；真没有的多半是 .bat / 奇怪形状）：取到第一个引号或空白。
        None => t.find(['"', ' ']).unwrap_or(t.len()),
    };
    let p = t[..end].trim();
    (!p.is_empty()).then(|| p.to_string())
}

/// 展开 `%NAME%`。`lookup` 查不到的变量原样保留（别把半截模板吞成空串，那样会拼出指向根目录的路径）。
pub(crate) fn expand_env_with(s: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 => {
                let name = &after[..j];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            // `%%` 或没有收尾的 `%`：原样输出这个 `%`，继续往后找。
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn win_parent(p: &str) -> Option<&str> {
    let t = p.trim_end_matches(['\\', '/']);
    let i = t.rfind(['\\', '/'])?;
    let parent = &t[..i];
    (!parent.is_empty()).then_some(parent)
}

fn win_file_name(p: &str) -> &str {
    p.rsplit(['\\', '/']).next().unwrap_or(p)
}

fn win_join(dir: &str, name: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches(['\\', '/']), name)
}

/// 卸载程序 / 安装器的文件名（`Uninstall X.exe`、`unins000.exe`、`uninstall.exe`）——
/// `DisplayIcon` 偶尔直接指向它，那不是主程序，不能当主程序启动。
fn is_uninstaller_name(file_name: &str) -> bool {
    let l = file_name.to_ascii_lowercase();
    l.starts_with("unins") || l.contains("uninstall")
}

fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s.is_char_boundary(prefix.len())
        && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// 条目是不是这条规则要找的。
pub(crate) fn entry_matches(rule: &AppRule, e: &UninstallEntry) -> bool {
    rule.key_names.iter().any(|k| e.key.eq_ignore_ascii_case(k))
        || (!e.display_name.is_empty()
            && (rule.display_names.iter().any(|n| e.display_name.eq_ignore_ascii_case(n))
                || rule.display_prefixes.iter().any(|p| starts_with_ignore_ascii_case(&e.display_name, p))))
}

/// 一个条目可能的主程序路径，按优先级排（还没验证文件在不在）。
///
/// 顺序：`DisplayIcon` 本身是 exe（且不是卸载器）→ 图标所在目录 → `InstallLocation` →
/// `UninstallString` 的父目录 → 图标目录的上一级——后面这些目录里按 `rule.exe_names` 的顺序找约定的 exe 名。
pub(crate) fn candidate_exes(
    rule: &AppRule,
    e: &UninstallEntry,
    expand: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let mut exes: Vec<String> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    let mut push_dir = |d: &str| {
        let d = d.trim();
        if !d.is_empty() && !dirs.iter().any(|x| x.eq_ignore_ascii_case(d)) {
            dirs.push(d.to_string());
        }
    };

    let icon = e.display_icon.as_deref().and_then(parse_display_icon).map(|p| expand(&p));
    if let Some(icon) = &icon {
        if icon.to_ascii_lowercase().ends_with(".exe") && !is_uninstaller_name(win_file_name(icon)) {
            exes.push(icon.clone());
        }
        if let Some(d) = win_parent(icon) {
            push_dir(d);
        }
    }
    if let Some(loc) = e.install_location.as_deref() {
        push_dir(&expand(loc));
    }
    if let Some(u) = e.uninstall_string.as_deref().and_then(exe_path_from_command) {
        if let Some(d) = win_parent(&expand(&u)) {
            push_dir(d);
        }
    }
    if let Some(icon) = &icon {
        if let Some(pp) = win_parent(icon).and_then(win_parent) {
            push_dir(pp);
        }
    }

    for d in &dirs {
        for n in rule.exe_names {
            exes.push(win_join(d, n));
        }
    }
    exes
}

/// 在一批卸载表条目里找这条规则的主程序。`is_file` 注入，所以不碰真实磁盘就能测。
///
/// 多个条目命中（WorkBuddy 国内版 + 海外版并存）时，取主程序文件名在 `rule.exe_names` 里排得最靠前的；
/// 并列取先出现的（卸载表顺序是 HKCU → HKLM → WOW6432Node，用户级安装优先）。
/// 注册表里一个都解析不出来，再按 `rule.fallback_exes` 的固定落点兜底（此时没有版本号）。
pub(crate) fn find_in(
    rule: &AppRule,
    entries: &[UninstallEntry],
    expand: &dyn Fn(&str) -> String,
    is_file: &dyn Fn(&str) -> bool,
) -> Option<(String, Option<String>)> {
    let mut best: Option<(usize, String, Option<String>)> = None;
    for e in entries.iter().filter(|e| entry_matches(rule, e)) {
        let Some(exe) = candidate_exes(rule, e, expand).into_iter().find(|c| is_file(c)) else {
            continue;
        };
        let name = win_file_name(&exe);
        let rank = rule
            .exe_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(usize::MAX);
        if best.as_ref().is_none_or(|(r, _, _)| rank < *r) {
            best = Some((rank, exe, e.display_version.clone()));
        }
    }
    if let Some((_, exe, version)) = best {
        return Some((exe, version));
    }
    rule.fallback_exes
        .iter()
        .filter_map(|t| {
            let path = expand(t);
            let unresolved = t.split('%').enumerate().any(|(i, name)| i % 2 == 1 && path.contains(&format!("%{name}%")));
            if unresolved { None } else { Some(path) }
        })
        .find(|p| is_file(p))
        .map(|p| (p, None))
}

/// 解析 `reg query <hive>\...\Uninstall /s` 的整段输出，只留**直接子键**（`/s` 会把更深的子键一并列出来，
/// 那些不是安装条目）。块与块之间以 `HKEY_...` 头行分隔；取值复用 `installer::reg_value`（剥引号 / 前缀要求空白）。
pub(crate) fn parse_uninstall_dump(out: &str) -> Vec<UninstallEntry> {
    const MARK: &str = "\\CurrentVersion\\Uninstall\\";
    let mut entries = Vec::new();
    let mut header: Option<&str> = None;
    let mut start = 0usize; // 当前块第一行（头行之后）在 out 里的起点
    let mut pos = 0usize;
    let mut flush = |header: Option<&str>, body: &str| {
        let Some(h) = header else { return };
        let Some(i) = h.find(MARK) else { return };
        let key = h[i + MARK.len()..].trim();
        if key.is_empty() || key.contains('\\') {
            return; // 更深一层的子键，不是安装条目
        }
        entries.push(UninstallEntry {
            key: key.to_string(),
            display_name: crate::installer::reg_value(body, "DisplayName").unwrap_or_default(),
            display_version: crate::installer::reg_value(body, "DisplayVersion"),
            display_icon: crate::installer::reg_value(body, "DisplayIcon"),
            install_location: crate::installer::reg_value(body, "InstallLocation"),
            uninstall_string: crate::installer::reg_value(body, "UninstallString"),
        });
    };
    for line in out.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with("HKEY_") {
            flush(header, &out[start..pos]);
            header = Some(t);
            start = pos + line.len();
        }
        pos += line.len();
    }
    flush(header, &out[start..pos]);
    entries
}

// ───────────────────────── Windows：真读卸载表 / 启动 ─────────────────────────

#[cfg(windows)]
const UNINSTALL_HIVES: [&str; 3] = [
    "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

/// 快照 TTL：见模块头「代价控制」。
#[cfg(windows)]
const SNAPSHOT_TTL: std::time::Duration = std::time::Duration::from_secs(10);
/// 单次 `reg.exe` 最长等待。正常是几十毫秒，超过说明系统异常（杀软卡 reg.exe 之类）——杀掉，当作读不到。
#[cfg(windows)]
const REG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// `reg query <hive> /s`，**带超时**。键不存在 / 超时 / 起不来都是 `None`。
#[cfg(windows)]
fn reg_dump(hive: &str) -> Option<String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut child = Command::new(crate::installer::system_tool("reg"))
        .args(["query", hive, "/s"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // 另起线程读管道：输出有几百 KB，不边读边等会把子进程写死在满管道上。
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(REG_TIMEOUT) {
        Ok(bytes) => {
            let ok = child.wait().map(|s| s.success()).unwrap_or(false);
            ok.then(|| crate::installer::decode_console(&bytes))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(windows)]
type Snapshot = (std::time::Instant, std::sync::Arc<Vec<UninstallEntry>>);

#[cfg(windows)]
static SNAPSHOT: std::sync::Mutex<Option<Snapshot>> = std::sync::Mutex::new(None);
#[cfg(windows)]
static CLAUDE_MSIX_CACHE: std::sync::Mutex<Option<(std::time::Instant, Option<FoundApp>)>> = std::sync::Mutex::new(None);

/// 安装之后必须重新探测，不能复用安装之前的「未安装」快照。
pub fn invalidate() {
    #[cfg(windows)]
    {
        *SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *CLAUDE_MSIX_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// 三处卸载表合并后的条目快照（HKCU → HKLM → WOW6432Node），TTL 内复用。
#[cfg(windows)]
fn snapshot() -> std::sync::Arc<Vec<UninstallEntry>> {
    if let Some((at, v)) = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if at.elapsed() < SNAPSHOT_TTL {
            return v.clone();
        }
    }
    // 三处并发读，把「3 × reg.exe 启动」压成「1 ×」。
    let dumps: Vec<Option<String>> = std::thread::scope(|s| {
        let handles: Vec<_> = UNINSTALL_HIVES.iter().map(|h| s.spawn(move || reg_dump(h))).collect();
        handles.into_iter().map(|h| h.join().ok().flatten()).collect()
    });
    let mut entries = Vec::new();
    for d in dumps.into_iter().flatten() {
        entries.extend(parse_uninstall_dump(&d));
    }
    let arc = std::sync::Arc::new(entries);
    *SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), arc.clone()));
    arc
}

/// 只读查找桌面应用的主程序与版本；Windows 查注册表，macOS 查应用 bundle。
pub fn find(id: &str) -> Option<FoundApp> {
    #[cfg(windows)]
    {
        let rule = rule_of(id)?;
        let entries = snapshot();
        let expand = |s: &str| expand_env_with(s, &|n| std::env::var(n).ok());
        let is_file = |p: &str| std::path::Path::new(p).is_file();
        find_in(rule, &entries, &expand, &is_file).map(|(exe, version)| FoundApp { exe: PathBuf::from(exe), version })
            .or_else(|| if id == "claude-app" { find_claude_msix() } else { None })
    }
    #[cfg(target_os = "macos")]
    {
        let (bundle, binary) = match id {
            "claude-app" => ("Claude.app", "Claude"),
            "obsidian" => ("Obsidian.app", "Obsidian"),
            "uu-remote" => ("UURemote.app", "UURemote"),
            _ => return None,
        };
        let roots = [PathBuf::from("/Applications"), crate::installer::user_home_dir().join("Applications")];
        roots.into_iter().map(|root| root.join(bundle).join("Contents/MacOS").join(binary))
            .find(|exe| exe.is_file()).map(|exe| FoundApp { exe, version: None })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = id;
        None
    }
}

/// MSIX installations have no traditional uninstall entry. Read the current user's package;
/// cache both hits and misses so repeated list_tools calls don't repeatedly launch PowerShell.
#[cfg(windows)]
fn find_claude_msix() -> Option<FoundApp> {
    use std::os::windows::process::CommandExt;
    use std::time::Instant;
    let mut cache = CLAUDE_MSIX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, app)) = cache.as_ref() {
        if at.elapsed() < SNAPSHOT_TTL { return app.clone().filter(|a| a.exe.is_file()); }
    }
    let script = "$p=Get-AppxPackage -Name Claude | Select-Object -First 1; if($p){ @{dir=$p.InstallLocation; version=$p.Version.ToString()} | ConvertTo-Json -Compress }";
    let mut command = std::process::Command::new(crate::installer::system_tool("powershell"));
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]).creation_flags(0x0800_0000);
    let output = crate::installer::output_with_timeout(command, 8);
    let app = output.filter(|o| o.status.success())
        .and_then(|out| serde_json::from_str::<serde_json::Value>(&crate::installer::decode_console(&out.stdout)).ok()).and_then(|v| {
        let root = PathBuf::from(v.get("dir")?.as_str()?);
        [root.join("app/Claude.exe"), root.join("Claude.exe")].into_iter().find(|p| p.is_file())
            .map(|exe| FoundApp { exe, version: v.get("version").and_then(|v| v.as_str()).map(str::to_owned) })
    });
    *cache = Some((Instant::now(), app.clone()));
    app
}

/// 本机是否装了它（= 能找到主程序）。
pub fn installed(id: &str) -> bool {
    find(id).is_some()
}

/// 已装版本（卸载表的 `DisplayVersion`；自动更新型软件可能滞后，见模块头）。
pub fn version_of(id: &str) -> Option<String> {
    find(id).and_then(|a| a.version)
}

/// 启动本地主程序（不弹黑窗、不等它退出）。
#[cfg(windows)]
pub fn launch(id: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let rule = rule_of(id).ok_or_else(|| format!("未知桌面应用 {id}"))?;
    let app = find(id).ok_or_else(|| format!("未找到{}的主程序（可能已被卸载），请重新安装或去官网下载", rule.label))?;
    let dir = app.exe.parent().map(|p| p.to_path_buf());
    let mut c = std::process::Command::new(&app.exe);
    if let Some(d) = &dir {
        c.current_dir(d);
    }
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    match c.spawn() {
        Ok(_) => Ok(()),
        // 740 = 程序清单要求提升权限：CreateProcess 拉不起，要走 ShellExecute（弹 UAC）。
        // 这几个软件正常都不要管理员；留这条是防万一，不让客户点了没反应（Open365 就踩过）。
        Err(e) if e.raw_os_error() == Some(740) => shell_start(&app.exe, dir.as_deref())
            .map_err(|e2| format!("启动{}失败: {e2}", rule.label)),
        Err(e) => Err(format!("启动{}失败: {e}", rule.label)),
    }
}

/// 用 PowerShell `Start-Process`（= ShellExecute）拉起一个 exe，必要时会弹 UAC。
#[cfg(windows)]
fn shell_start(exe: &std::path::Path, work_dir: Option<&std::path::Path>) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let q = |p: &std::path::Path| p.to_string_lossy().replace('\'', "''");
    let mut ps = format!("Start-Process -FilePath '{}'", q(exe));
    if let Some(d) = work_dir {
        ps.push_str(&format!(" -WorkingDirectory '{}'", q(d)));
    }
    std::process::Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// macOS 按已检测到的应用 bundle 打开，避免同名应用歧义。
#[cfg(target_os = "macos")]
pub fn launch(id: &str) -> Result<(), String> {
    let app = find(id).ok_or_else(|| "未找到桌面应用，请先安装".to_string())?;
    let bundle = app.exe.ancestors().nth(3).ok_or_else(|| "应用目录无效".to_string())?;
    std::process::Command::new("open")
        .arg("-a").arg(bundle)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动桌面应用失败: {e}"))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn launch(_id: &str) -> Result<(), String> {
    Err("当前平台暂不支持直接打开此桌面应用".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // ── 夹具：形状取自 2026-10-02 本机只读实测，路径 / 用户名换成一眼假的占位（demo）。
    //    子键名、DisplayName、DisplayIcon 的「形状」（带引号 / 带 `,0` / .ico / GUID 键）是真的。
    const DUMP: &str = "\r\n\
HKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Doubao\r\n\
    DisplayName    REG_SZ    豆包\r\n\
    UninstallString    REG_SZ    \"C:\\Users\\demo\\AppData\\Local\\Doubao\\Application\\uninstall.exe\"\r\n\
    DisplayIcon    REG_SZ    \"C:\\Users\\demo\\AppData\\Local\\Doubao\\Application\\icon.ico\"\r\n\
    DisplayVersion    REG_SZ    1.77.8\r\n\
\r\n\
HKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\DoubaoWork\r\n\
    DisplayName    REG_SZ    豆包工作\r\n\
    UninstallString    REG_SZ    \"C:\\Users\\demo\\AppData\\Local\\DoubaoWork\\Application\\uninstall.exe\"\r\n\
    DisplayIcon    REG_SZ    \"C:\\Users\\demo\\AppData\\Local\\DoubaoWork\\Application\\icon.ico\"\r\n\
    DisplayVersion    REG_SZ    2.31.10\r\n\
\r\n\
HKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\DoubaoWork\\Components\r\n\
    DisplayName    REG_SZ    不是安装条目的更深子键\r\n\
\r\n\
HKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ffb0f51c-0000-0000-0000-000000000000\r\n\
    DisplayName    REG_SZ    千问办公\r\n\
    UninstallString    REG_SZ    \"C:\\Users\\demo\\AppData\\Local\\Programs\\QwenWorkCN\\Uninstall QwenWorkCN.exe\" /currentuser\r\n\
    QuietUninstallString    REG_SZ    \"C:\\Users\\demo\\AppData\\Local\\Programs\\QwenWorkCN\\Uninstall QwenWorkCN.exe\" /currentuser /S\r\n\
    DisplayVersion    REG_SZ    1.0.4\r\n\
    DisplayIcon    REG_SZ    C:\\Users\\demo\\AppData\\Local\\Programs\\QwenWorkCN\\uninstallerIcon.ico\r\n\
\r\n\
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\BFD312E9-0000-0000-0000-000000000000\r\n\
    DisplayName    REG_SZ    WorkBuddy 5.7.3\r\n\
    DisplayVersion    REG_SZ    5.7.3\r\n\
    DisplayIcon    REG_SZ    C:\\Program Files\\WorkBuddy\\WorkBuddy.exe,0\r\n\
\r\n\
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\C02C88CB-0000-0000-0000-000000000000\r\n\
    DisplayName    REG_SZ    WorkBuddy AI 5.6.2\r\n\
    DisplayVersion    REG_SZ    5.6.2\r\n\
    DisplayIcon    REG_SZ    C:\\Program Files\\WorkBuddyAI\\WorkBuddyAI.exe,0\r\n\
\r\n\
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\bd400747-0000-0000-0000-000000000000\r\n\
    DisplayName    REG_SZ    Obsidian\r\n\
    DisplayVersion    REG_SZ    1.12.7\r\n\
    DisplayIcon    REG_SZ    C:\\Program Files\\Obsidian\\Obsidian.exe,0\r\n\
\r\n\
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SomethingElse\r\n\
    DisplayName    REG_SZ    Obsidian Entertainment Launcher\r\n\
\r\n";

    fn entries() -> Vec<UninstallEntry> {
        parse_uninstall_dump(DUMP)
    }

    fn rule(id: &str) -> &'static AppRule {
        rule_of(id).unwrap_or_else(|| panic!("没有规则 {id}"))
    }

    fn no_expand(s: &str) -> String {
        s.to_string()
    }

    /// 假的磁盘：只有列出来的路径「存在」（忽略大小写）。
    fn disk<'a>(files: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |p: &str| files.iter().any(|f| f.eq_ignore_ascii_case(p))
    }

    #[test]
    fn display_icon_shapes() {
        // 带引号 + 图标序号（reg_value 不剥这种「引号在中间」的形状，由这里拆）
        assert_eq!(parse_display_icon(r#""C:\Program Files\X\x.exe",0"#).as_deref(), Some(r"C:\Program Files\X\x.exe"));
        // 不带引号 + 序号 / 负序号（资源 id）
        assert_eq!(parse_display_icon(r"C:\Program Files\WorkBuddy\WorkBuddy.exe,0").as_deref(), Some(r"C:\Program Files\WorkBuddy\WorkBuddy.exe"));
        assert_eq!(parse_display_icon(r"C:\x\a.exe,-101").as_deref(), Some(r"C:\x\a.exe"));
        // .ico：原样给出路径，后面由调用方在同目录找 exe
        assert_eq!(parse_display_icon(r"C:\x\icon.ico").as_deref(), Some(r"C:\x\icon.ico"));
        assert_eq!(parse_display_icon(r#""C:\x\icon.ico""#).as_deref(), Some(r"C:\x\icon.ico"));
        // REG_EXPAND_SZ 的未展开模板保持原样（展开是另一步）
        assert_eq!(parse_display_icon(r"%LOCALAPPDATA%\Doubao\Application\icon.ico,0").as_deref(), Some(r"%LOCALAPPDATA%\Doubao\Application\icon.ico"));
        // 路径本身带逗号，尾巴不是整数 → 不能误砍
        assert_eq!(parse_display_icon(r"C:\a,b\x.exe").as_deref(), Some(r"C:\a,b\x.exe"));
        // 没有收尾引号：取剩下的全部
        assert_eq!(parse_display_icon(r#""C:\x\a.exe"#).as_deref(), Some(r"C:\x\a.exe"));
        // 空 / 只有序号 / 只有引号 → 拆不出路径
        assert_eq!(parse_display_icon(""), None);
        assert_eq!(parse_display_icon("   "), None);
        assert_eq!(parse_display_icon(",0"), None);
        assert_eq!(parse_display_icon("\"\""), None);
    }

    #[test]
    fn exe_from_command_line() {
        assert_eq!(exe_path_from_command(r#""C:\App\Uninstall X.exe" /currentuser"#).as_deref(), Some(r"C:\App\Uninstall X.exe"));
        // 未加引号、路径里有空格：靠 .exe 结尾取全
        assert_eq!(exe_path_from_command(r"C:\Program Files\App\unins000.exe /SILENT").as_deref(), Some(r"C:\Program Files\App\unins000.exe"));
        // 被 reg_value 剥过头的 `"A" /x "y"` 形状（首尾都是引号）
        assert_eq!(exe_path_from_command("C:\\a\\u.exe\" /x \"y").as_deref(), Some(r"C:\a\u.exe"));
        assert_eq!(exe_path_from_command(""), None);
        assert_eq!(exe_path_from_command("\"\""), None);
    }

    #[test]
    fn expand_env_leaves_unknown_vars_alone() {
        let lookup = |n: &str| match n.to_ascii_lowercase().as_str() {
            "localappdata" => Some(r"C:\Users\demo\AppData\Local".to_string()),
            "programfiles(x86)" => Some(r"C:\Program Files (x86)".to_string()),
            _ => None,
        };
        assert_eq!(expand_env_with(r"%LOCALAPPDATA%\Doubao\icon.ico", &lookup), r"C:\Users\demo\AppData\Local\Doubao\icon.ico");
        assert_eq!(expand_env_with(r"%ProgramFiles(x86)%\X", &lookup), r"C:\Program Files (x86)\X");
        // 查不到的变量原样保留，不吞成空串
        assert_eq!(expand_env_with(r"%NOPE%\X", &lookup), r"%NOPE%\X");
        // 没有收尾的 % / 连续 %% / 没有变量
        assert_eq!(expand_env_with("100%", &lookup), "100%");
        assert_eq!(expand_env_with("a%%b", &lookup), "a%%b");
        assert_eq!(expand_env_with(r"C:\plain", &lookup), r"C:\plain");
    }

    #[test]
    fn dump_parsing_keeps_only_direct_children_and_decodes_values() {
        let es = entries();
        let keys: Vec<&str> = es.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            ["Doubao", "DoubaoWork", "ffb0f51c-0000-0000-0000-000000000000", "BFD312E9-0000-0000-0000-000000000000", "C02C88CB-0000-0000-0000-000000000000", "bd400747-0000-0000-0000-000000000000", "SomethingElse"],
            "更深一层的 DoubaoWork\\Components 不该被当成安装条目"
        );
        let dw = es.iter().find(|e| e.key == "DoubaoWork").unwrap();
        assert_eq!(dw.display_name, "豆包工作");
        assert_eq!(dw.display_version.as_deref(), Some("2.31.10"));
        // 首尾成对引号已被 reg_value 剥掉；没有 InstallLocation
        assert_eq!(dw.display_icon.as_deref(), Some(r"C:\Users\demo\AppData\Local\DoubaoWork\Application\icon.ico"));
        assert_eq!(dw.install_location, None);
        // UninstallString 不能被 QuietUninstallString 抢（千问的两个都有）
        let q = es.iter().find(|e| e.display_name == "千问办公").unwrap();
        assert!(q.uninstall_string.as_deref().unwrap().ends_with("/currentuser"), "{:?}", q.uninstall_string);
        assert!(!q.uninstall_string.as_deref().unwrap().ends_with("/S"));
        assert!(parse_uninstall_dump("").is_empty());
        assert!(parse_uninstall_dump("错误: 系统找不到指定的注册表项或值。\r\n").is_empty());
    }

    #[test]
    fn rule_matching_does_not_confuse_doubao_products() {
        let es = entries();
        let hits = |id: &str| -> Vec<String> {
            es.iter().filter(|e| entry_matches(rule(id), e)).map(|e| e.display_name.clone()).collect()
        };
        // 「豆包」（消费版）不能被「豆包工作」的规则认走，反过来也一样
        assert_eq!(hits("doubao"), ["豆包工作"]);
        // 千问办公是 GUID 键，只能靠 DisplayName
        assert_eq!(hits("qwenwork"), ["千问办公"]);
        // WorkBuddy 国内版与海外版（带 AI）都认
        assert_eq!(hits("workbuddy"), ["WorkBuddy 5.7.3", "WorkBuddy AI 5.6.2"]);
        // Obsidian 精确匹配，不认「Obsidian Entertainment Launcher」
        assert_eq!(hits("obsidian"), ["Obsidian"]);
        assert!(hits("uu-remote").is_empty());
        // 子键名匹配忽略大小写，且不需要 DisplayName
        let bare = UninstallEntry { key: "doubaowork".into(), ..Default::default() };
        assert!(entry_matches(rule("doubao"), &bare));
    }

    #[test]
    fn exe_is_found_next_to_an_ico_icon() {
        let es = entries();
        let dw = es.iter().find(|e| e.key == "DoubaoWork").unwrap();
        let cands = candidate_exes(rule("doubao"), dw, &no_expand);
        // DisplayIcon 是 .ico：不能把 .ico 当 exe，同目录的约定 exe 名排第一
        assert_eq!(cands[0], r"C:\Users\demo\AppData\Local\DoubaoWork\Application\DoubaoWork.exe");
        assert!(!cands.iter().any(|c| c.to_ascii_lowercase().ends_with(".ico")));

        let on = [r"C:\Users\demo\AppData\Local\DoubaoWork\Application\DoubaoWork.exe"];
        let found = find_in(rule("doubao"), &es, &no_expand, &disk(&on)).unwrap();
        assert_eq!(found, (on[0].to_string(), Some("2.31.10".to_string())));

        // 千问办公：.ico 在根目录，主程序是同目录的 Launcher.exe（排在 QwenWorkCN.exe 前面）
        let qw = [r"C:\Users\demo\AppData\Local\Programs\QwenWorkCN\Launcher.exe", r"C:\Users\demo\AppData\Local\Programs\QwenWorkCN\QwenWorkCN.exe"];
        let found = find_in(rule("qwenwork"), &es, &no_expand, &disk(&qw)).unwrap();
        assert_eq!(found.0, qw[0]);
        assert_eq!(found.1.as_deref(), Some("1.0.4"));
    }

    #[test]
    fn icon_that_is_an_exe_wins_and_uninstallers_never_do() {
        let es = entries();
        let ob = es.iter().find(|e| e.display_name == "Obsidian").unwrap();
        let cands = candidate_exes(rule("obsidian"), ob, &no_expand);
        assert_eq!(cands[0], r"C:\Program Files\Obsidian\Obsidian.exe", "`,0` 要被拆掉");

        // DisplayIcon 直接指向卸载器：不能当主程序，改走同目录约定名
        let e = UninstallEntry {
            key: "x".into(),
            display_icon: Some(r#""C:\Apps\Foo\Uninstall Foo.exe",0"#.into()),
            ..Default::default()
        };
        let cands = candidate_exes(rule("obsidian"), &e, &no_expand);
        assert_eq!(cands, [r"C:\Apps\Foo\Obsidian.exe", r"C:\Apps\Obsidian.exe"]);
        // Inno Setup 的 unins000.exe 同理
        assert!(is_uninstaller_name("unins000.exe") && is_uninstaller_name("Uninstall WorkBuddy.exe"));
        assert!(!is_uninstaller_name("WorkBuddy.exe"));
    }

    #[test]
    fn install_location_and_uninstall_string_are_fallback_dirs() {
        // 只有 InstallLocation（无 DisplayIcon）
        let e = UninstallEntry { key: "GameViewer".into(), install_location: Some(r"D:\Games\GameViewer".into()), ..Default::default() };
        let cands = candidate_exes(rule("uu-remote"), &e, &no_expand);
        assert_eq!(cands[0], r"D:\Games\GameViewer\GameViewer.exe");
        // 只有 UninstallString：取卸载器所在目录
        let e = UninstallEntry {
            key: "GameViewer".into(),
            uninstall_string: Some(r#""C:\Program Files\Netease\GameViewer\Uninstall.exe""#.into()),
            ..Default::default()
        };
        let cands = candidate_exes(rule("uu-remote"), &e, &no_expand);
        assert_eq!(cands[0], r"C:\Program Files\Netease\GameViewer\GameViewer.exe");
        // 三者都空：没有候选（不 panic）
        assert!(candidate_exes(rule("uu-remote"), &UninstallEntry::default(), &no_expand).is_empty());
    }

    #[test]
    fn env_templates_in_registry_values_are_expanded() {
        let e = UninstallEntry {
            key: "DoubaoWork".into(),
            display_icon: Some(r"%LOCALAPPDATA%\DoubaoWork\Application\icon.ico,0".into()),
            ..Default::default()
        };
        let expand = |s: &str| expand_env_with(s, &|n| (n == "LOCALAPPDATA").then(|| r"C:\Users\demo\AppData\Local".to_string()));
        let cands = candidate_exes(rule("doubao"), &e, &expand);
        assert_eq!(cands[0], r"C:\Users\demo\AppData\Local\DoubaoWork\Application\DoubaoWork.exe");
    }

    #[test]
    fn workbuddy_prefers_the_domestic_build_and_falls_back_to_overseas() {
        let es = entries();
        let both = [r"C:\Program Files\WorkBuddy\WorkBuddy.exe", r"C:\Program Files\WorkBuddyAI\WorkBuddyAI.exe"];
        let f = find_in(rule("workbuddy"), &es, &no_expand, &disk(&both)).unwrap();
        assert_eq!(f, (both[0].to_string(), Some("5.7.3".to_string())));
        // 国内版的文件没了（只剩海外版）→ 用海外版，版本跟着条目走
        let only_ai = [both[1]];
        let f = find_in(rule("workbuddy"), &es, &no_expand, &disk(&only_ai)).unwrap();
        assert_eq!(f, (both[1].to_string(), Some("5.6.2".to_string())));
        // 都没有 → None
        assert!(find_in(rule("workbuddy"), &es, &no_expand, &disk(&[])).is_none());
    }

    #[test]
    fn registry_hit_whose_exe_is_gone_is_not_installed() {
        // 卸载表还留着条目但文件被删了（常见：手动删目录）→ 必须判未装，不能报已装后点了没反应
        let es = entries();
        assert!(find_in(rule("doubao"), &es, &no_expand, &disk(&[])).is_none());
    }

    #[test]
    fn fixed_locations_back_up_an_empty_registry() {
        let expand = |s: &str| expand_env_with(s, &|n| match n {
            "LOCALAPPDATA" => Some(r"C:\Users\demo\AppData\Local".to_string()),
            "ProgramFiles" => Some(r"C:\Program Files".to_string()),
            _ => None,
        });
        let on = [r"C:\Users\demo\AppData\Local\DoubaoWork\Application\DoubaoWork.exe"];
        // 注册表一条都没有（新装未入表 / 非中文系统读不出中文名）：默认落点兜底，没有版本号
        let f = find_in(rule("doubao"), &[], &expand, &disk(&on)).unwrap();
        assert_eq!(f, (on[0].to_string(), None));
        // 模板里引用了展不开的变量：不会拼出奇怪路径命中别的东西
        assert!(find_in(rule("uu-remote"), &[], &expand, &disk(&[r"%ProgramFiles(x86)%\Netease\GameViewer\GameViewer.exe"])).is_some() == false);
    }

    #[test]
    fn rules_are_well_formed() {
        let mut ids = BTreeSet::new();
        for r in RULES {
            assert!(ids.insert(r.id), "规则 id 重复：{}", r.id);
            assert!(!r.exe_names.is_empty() && !r.fallback_exes.is_empty(), "{} 缺约定 exe 名 / 固定落点", r.id);
            assert!(
                !r.key_names.is_empty() || !r.display_names.is_empty() || !r.display_prefixes.is_empty(),
                "{} 没有任何匹配条件，永远认不出",
                r.id
            );
            for e in r.exe_names {
                assert!(e.to_ascii_lowercase().ends_with(".exe"), "{}: {e}", r.id);
            }
            for f in r.fallback_exes {
                assert!(f.contains('%') && f.to_ascii_lowercase().ends_with(".exe"), "{}: 固定落点要带 %VAR% 且以 .exe 结尾：{f}", r.id);
            }
        }
        assert!(rule_of("claude-code").is_none());
    }

    /// 非 Windows 恒「没装」，启动给人话（不 panic）。
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn non_windows_never_finds_anything() {
        for r in RULES {
            assert!(!installed(r.id));
        }
        assert!(launch("doubao").is_err());
    }

    /// 本机真实注册表**只读**实跑：验证 `reg.exe` 的 GBK 输出被正确解码、条目快照可用、
    /// 各应用的主程序与版本能解析出来。不断言具体值（别的机器上装法不同），只把结果打印出来：
    /// `cargo test --lib desktop_app::tests::real_registry_read_only_probe -- --ignored --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore = "读真实注册表（只读），结果依赖本机装了什么；手动跑并 --nocapture 看输出"]
    fn real_registry_read_only_probe() {
        let t0 = std::time::Instant::now();
        let snap = snapshot();
        eprintln!("快照：{} 个卸载条目，首次读取 {} ms", snap.len(), t0.elapsed().as_millis());
        let t1 = std::time::Instant::now();
        let _ = snapshot();
        eprintln!("缓存命中再取一次：{} ms", t1.elapsed().as_millis());
        for r in RULES {
            let hits: Vec<String> = snap
                .iter()
                .filter(|e| entry_matches(r, e))
                .map(|e| format!("{}（键 {}，图标 {:?}）", e.display_name, e.key, e.display_icon))
                .collect();
            eprintln!("[{}] 卸载表命中：{hits:?}", r.id);
            match find(r.id) {
                Some(a) => eprintln!("[{}] exe = {}，version = {:?}，exe 存在 = {}", r.id, a.exe.display(), a.version, a.exe.is_file()),
                None => eprintln!("[{}] 未装", r.id),
            }
        }
    }

    /// Windows 上 `find` / `installed` / `version_of` 三者口径一致，且不 panic（不依赖本机装了什么）。
    #[cfg(windows)]
    #[test]
    fn find_installed_and_version_agree() {
        for r in RULES {
            let f = find(r.id);
            assert_eq!(installed(r.id), f.is_some(), "{}", r.id);
            assert_eq!(version_of(r.id), f.and_then(|a| a.version), "{}", r.id);
        }
        assert!(find("no-such-app").is_none());
    }
}
