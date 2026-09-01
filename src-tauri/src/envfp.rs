//! 环境指纹 —— 这台机器长什么样（**非隐私**，可跨机器做相关性统计）。
//!
//! ## 为什么单独一个模块
//!
//! `report.rs` 现在上报的 `os` 是 `std::env::consts::OS`，值就是字符串 `"windows"` ——三个字。
//! 结果是仓库里 30 条 `install_failed`，好几条标题直接写着「系统盘空间不足：仅剩 416 MB」，
//! **规律就摆在眼前却统计不出来**，只能一条条人工看。本模块把「能解释失败的变量」补齐。
//!
//! ## 红线
//!
//! - **绝不记录路径原文 / 用户名 / Key**。中文用户名这种坑只记 `path_nonascii: true`，
//!   不记那个名字本身。字段表里每一项都要说得出「它解释哪个已知失败模式」。
//! - 探测要跑几个子进程（reg / PowerShell / node），**缓存 24 小时**，不拖启动。
//! - 纯 std + serde，不引第三方 crate（守体积红线，与 hardware.rs 同口径）。
//!
//! schema 定义见 `docs/metrics-schema.md`。改字段必须同步那份文档并升 `SCHEMA_VERSION`。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// schema 版本。加/改字段必须 +1 —— 旧数据靠它区分，不混算。
/// v2（2026-08-10）：加了四个 AI CLI 的版本号。
/// **升号不只是记账** —— `current()` 会因 `c.v != SCHEMA_VERSION` 丢弃旧缓存重新探。
/// 不升的话，老 `env.json` 里没有这几个字段，反序列化成空串，
/// 于是一台装着 claude 的机器会报 `claude_ver=""` —— 假阴性比没有这个字段更坏。
pub const SCHEMA_VERSION: u32 = 2;

/// 指纹缓存有效期（秒）。
const CACHE_TTL: u64 = 24 * 3600;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EnvFingerprint {
    pub v: u32,
    /// 产生这份指纹的 U-King 版本
    pub app: String,

    // —— 平台基础分组 ——
    pub os: String,
    pub os_ver: String,
    pub arch: String,

    // —— 硬件相关性（「Mac 快多少」只能靠它 + tool_ms 拆解回答）——
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub ram_mb: u64,
    pub gpu_vendor: String,

    // —— 磁盘：★ install_failed 的头号解释变量 ——
    pub sys_disk_free_mb: u64,
    pub sys_disk_total_mb: u64,

    // —— 工具链版本：安装失败的头号变量 ——
    pub node_ver: String,
    pub npm_ver: String,
    pub git_ver: String,
    pub pwsh_major: u32,

    // —— AI CLI 版本：★ 它们**自己会静默自动升级，而升级会改行为** ——
    //
    // 2026-08-10 逼出来的。客户报「任务老是中断 / 跑一段时间自动退出」，而且是「这两天」
    // 突然全线发作 —— 之前几十个终端 22 天不关机都没事。真因是 Claude Code 某个 2.1.2xx
    // 版本新增了「未知模型窗口强制」（本机比对 2.1.205 与 2.1.225 二进制：那套机制和那句
    // 提示在前者里一次都没出现）。它自动升级，所以全体客户在几天内先后中招。
    //
    // 🔴 而我们当时**一个字都看不见**：指纹里记的是 node/npm/git，诊断正文里记的是
    // `claude=true/false` —— 只知道「装没装」，不知道「装的是哪一版」。于是
    // 「全体客户的 claude 在 08-08 前后跳版本 + 错误同时暴涨」这条最强的信号，
    // 在我们的数据里根本不存在，只能靠人肉去对二进制。
    //
    // 这是**我们自己的**观测缺口（不是去改别人的行为），所以该补的是这里。
    pub claude_ver: String,
    pub codex_ver: String,
    pub openclaw_ver: String,
    pub hermes_ver: String,

    // —— 环境炸点：每一条都对应一个真实踩过的坑 ——
    /// ★ 家目录含非 ASCII（中文用户名）。**只记 true/false，绝不记路径原文**
    pub path_nonascii: bool,
    /// 家目录含空格
    pub path_space: bool,
    /// ★ 家目录被 OneDrive 重定向 —— 装机高频炸点
    pub home_onedrive: bool,
    /// Windows LongPathsEnabled（优化大师的配方项之一）
    pub long_paths: bool,
    /// ★ Defender 实时防护开着 —— 会让 npm install 慢数倍，不记就永远归因不到
    pub defender_rt: bool,
    /// 代理开着（影响下载成败与速度）
    pub proxy: bool,
    /// 语言环境（乱码类问题相关）
    pub locale: String,

    /// 本次指纹算出来的时间（Unix 秒），缓存判断用。不上传。
    #[serde(default)]
    pub cached_at: u64,
}

// ============================================================
// 对外
// ============================================================

/// 取指纹（缓存优先，24h 过期自动重算）。
///
/// 首次/过期会跑几个子进程（约 1–3 秒），调用方自行放后台线程，别在 UI 线程直接调。
pub fn current() -> EnvFingerprint {
    if let Some(c) = load_cache() {
        if now_secs().saturating_sub(c.cached_at) < CACHE_TTL && c.v == SCHEMA_VERSION {
            return c;
        }
    }
    let fp = detect();
    save_cache(&fp);
    fp
}

/// 强制重算（`--envfp` 无头入口 / 优化后重新锚定用）。
pub fn detect() -> EnvFingerprint {
    let home = home_dir();
    let home_s = home.to_string_lossy().to_string();
    let (free, total) = sys_disk_mb();

    // 四个 AI CLI 的版本**并行探**，互不依赖。
    //
    // 串行实测冷启动 7.9s（`hermes --version` 一个就吃掉 2.3s），而
    // `collect_diagnostics` 是 UI 路径 —— 那就是我自己造一个几秒的卡顿
    // （本仓库为「同步 command 冻住界面」栽过一次，别再来一遍）。
    // 并行之后代价 ≈ 最慢的那一个。
    let ai_vers: Vec<String> = {
        let handles: Vec<_> = ["claude", "codex", "openclaw", "hermes"]
            .into_iter()
            .map(|c| std::thread::spawn(move || cli_version(c)))
            .collect();
        handles
            .into_iter()
            // 探测线程 panic 不该把整份指纹带走 —— 那一项留空即可
            .map(|h| h.join().unwrap_or_default())
            .collect()
    };

    EnvFingerprint {
        v: SCHEMA_VERSION,
        app: env!("CARGO_PKG_VERSION").to_string(),

        os: std::env::consts::OS.to_string(),
        os_ver: os_version(),
        arch: std::env::consts::ARCH.to_string(),

        cpu_model: cpu_model(),
        cpu_cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(0),
        ram_mb: ram_mb(),
        gpu_vendor: gpu_vendor(),

        sys_disk_free_mb: free,
        sys_disk_total_mb: total,

        claude_ver: ai_vers[0].clone(),
        codex_ver: ai_vers[1].clone(),
        openclaw_ver: ai_vers[2].clone(),
        hermes_ver: ai_vers[3].clone(),
        node_ver: tool_version("node", &["--version"]),
        npm_ver: tool_version(npm_bin(), &["--version"]),
        git_ver: tool_version("git", &["--version"]),
        pwsh_major: pwsh_major(),

        // 只取布尔特征，路径原文不出本函数
        path_nonascii: !home_s.is_ascii(),
        path_space: home_s.contains(' '),
        home_onedrive: home_onedrive(&home_s),
        long_paths: long_paths_enabled(),
        defender_rt: defender_realtime(),
        proxy: proxy_on(),
        locale: locale(),

        cached_at: now_secs(),
    }
}

// ============================================================
// 缓存
// ============================================================

/// 自持目录解析 —— **不 import metrics**。功能模块之间禁止互相依赖（设计取舍铁律②），
/// 否则删一个就得连着改另一个。与 device.rs / draw.rs / guard.rs 同一份写法。
fn metrics_dir() -> PathBuf {
    home_dir().join(".uking").join("metrics")
}

fn cache_path() -> PathBuf {
    metrics_dir().join("env.json")
}

fn load_cache() -> Option<EnvFingerprint> {
    let s = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&s).ok()
}

fn save_cache(fp: &EnvFingerprint) {
    let _ = std::fs::create_dir_all(metrics_dir());
    if let Ok(s) = serde_json::to_string(fp) {
        let _ = std::fs::write(cache_path(), s);
    }
}

// ============================================================
// 探测（每个都 fail-soft：探不到给默认值，绝不 panic、绝不阻塞主流程）
// ============================================================

fn home_dir() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let h = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(h)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 跑命令收 stdout。CREATE_NO_WINDOW 防黑窗（与 hardware.rs 同口径）。
fn cmd_out(program: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(program);
    c.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 注册表读取已下沉到公共层（`installer::reg_query`）—— term.rs 也要读注册表拿 ConPTY
/// 版本号，与其复制第二份解析（「值里可能带空格」那个坑），不如共用一份。
/// 依赖方向合法：功能模块 → 老的公共助手。
#[cfg(windows)]
fn reg_query(key: &str, value: &str) -> Option<String> {
    crate::installer::reg_query(key, value)
}

fn npm_bin() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

/// `node --version` → `22.14.0`；`git version 2.49.0.windows.1` → `2.49.0`。
///
/// 只保留**纯数字点分**的前缀：`.windows.1` 这类平台后缀留着会让同一个 git
/// 在 Win/Mac 上分成两组，跨平台对比直接失真。
/// AI CLI 的版本号。找不到就返回空串，**且一个进程都不起**。
///
/// 两条都不是风格问题：
/// - **先在 `search_paths` 上找文件再起进程**：`hermes --version` 本机实测 2331ms
///   （见 `installer::tool_installed` 里那段实测数据）。没装这个工具的机器上白等几秒，
///   换不来任何信息。
/// - **用绝对路径起，而不是靠 PATH 解析**：GUI 启动的进程 PATH 很窄（macOS 尤其），
///   靠裸名会把「装了但没探到」记成「没装」—— 那比不记更坏，会让排障往错误方向走。
fn cli_version(cmd: &str) -> String {
    let exts: &[&str] = if cfg!(windows) {
        &["", ".cmd", ".exe", ".bat"]
    } else {
        &[""]
    };
    for dir in crate::installer::search_paths(None) {
        for ext in exts {
            let p = dir.join(format!("{cmd}{ext}"));
            if p.is_file() {
                let v = tool_version(&p.to_string_lossy(), &["--version"]);
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    // 兜底：走系统 PATH 解析一次。
    //
    // 🔴 **这一步不能省**，本机就是反例：claude 装在 `~/bin`（转发脚本）和原生安装目录里，
    // 两处都不在 `search_paths` 上 —— 只扫那份清单的话，一台**装着 claude 的机器**
    // 会报 `claude_ver=""`。字段在、值恒空，比没有这个字段更坏：排障的人会据此
    // 判定「客户没装」，往完全错误的方向查。判据必须和 `installer::tool_installed`
    // 一致（它也是「先扫 search_paths，再 probe 一次」）。
    //
    // 代价可控：工具不存在时进程根本起不来，立刻返回，不会有 `hermes --version`
    // 那 2.3 秒的等待 —— 那是**跑起来之后**才付的钱。
    tool_version(cmd, &["--version"])
}

fn tool_version(program: &str, args: &[&str]) -> String {
    let raw = match cmd_out(program, args) {
        Some(s) => s,
        None => return String::new(),
    };
    let first = raw.lines().next().unwrap_or("").trim();
    let tok = first
        .split_whitespace()
        .map(|t| t.trim_start_matches('v'))
        .find(|t| t.contains('.') && t.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .unwrap_or("");
    normalize_version(tok)
}

/// `2.49.0.windows.1` → `2.49.0`；`22.14.0` → `22.14.0`。
fn normalize_version(tok: &str) -> String {
    tok.split('.')
        .take_while(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join(".")
}

fn os_version() -> String {
    #[cfg(windows)]
    {
        // registry 比 WMI 快一个数量级
        let build = reg_query(
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "CurrentBuild",
        )
        .unwrap_or_default();
        if !build.is_empty() {
            return format!("10.0.{build}");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(s) = cmd_out("sw_vers", &["-productVersion"]) {
            return s.trim().to_string();
        }
    }
    String::new()
}

fn cpu_model() -> String {
    #[cfg(windows)]
    {
        // registry 路径不走 WMI，几十毫秒
        if let Some(s) = reg_query(
            r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "ProcessorNameString",
        ) {
            return s.trim().to_string();
        }
        // reg 的值里有空格时 last() 只拿到末段，退 WMI 拿全名
        if let Some(s) = cmd_out("wmic", &["cpu", "get", "name"]) {
            if let Some(l) = s.lines().map(str::trim).find(|l| !l.is_empty() && *l != "Name") {
                return l.to_string();
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(s) = cmd_out("sysctl", &["-n", "machdep.cpu.brand_string"]) {
            return s.trim().to_string();
        }
    }
    String::new()
}

fn ram_mb() -> u64 {
    #[cfg(windows)]
    {
        if let Some(s) = cmd_out(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ],
        ) {
            if let Ok(b) = s.trim().parse::<u64>() {
                return b / 1024 / 1024;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(s) = cmd_out("sysctl", &["-n", "hw.memsize"]) {
            if let Ok(b) = s.trim().parse::<u64>() {
                return b / 1024 / 1024;
            }
        }
    }
    0
}

fn gpu_vendor() -> String {
    #[cfg(target_os = "macos")]
    {
        // Apple Silicon 统一内存
        if std::env::consts::ARCH == "aarch64" {
            return "apple".into();
        }
    }
    #[cfg(windows)]
    {
        if cmd_out("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"]).is_some() {
            return "nvidia".into();
        }
        if let Some(s) = cmd_out("wmic", &["path", "win32_VideoController", "get", "name"]) {
            let l = s.to_ascii_lowercase();
            if l.contains("nvidia") {
                return "nvidia".into();
            }
            if l.contains("amd") || l.contains("radeon") {
                return "amd".into();
            }
            if l.contains("intel") {
                return "intel".into();
            }
        }
    }
    "unknown".into()
}

/// 系统盘（Windows=C:，Mac=/）的 (可用 MB, 总量 MB)。
fn sys_disk_mb() -> (u64, u64) {
    #[cfg(windows)]
    {
        if let Some(s) = cmd_out(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$d=Get-PSDrive C; \"$($d.Free) $($d.Used)\"",
            ],
        ) {
            let t: Vec<u64> = s.trim().split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if t.len() == 2 {
                return (t[0] / 1024 / 1024, (t[0] + t[1]) / 1024 / 1024);
            }
        }
    }
    #[cfg(not(windows))]
    {
        // df -k / → 第二行： Filesystem 1024-blocks Used Available ...
        if let Some(s) = cmd_out("df", &["-k", "/"]) {
            if let Some(line) = s.lines().nth(1) {
                let t: Vec<&str> = line.split_whitespace().collect();
                if t.len() >= 4 {
                    let total = t[1].parse::<u64>().unwrap_or(0) / 1024;
                    let avail = t[3].parse::<u64>().unwrap_or(0) / 1024;
                    return (avail, total);
                }
            }
        }
    }
    (0, 0)
}

fn pwsh_major() -> u32 {
    // pwsh = PowerShell 7+；没有就看老的 5.1
    if let Some(s) = cmd_out("pwsh", &["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"]) {
        if let Ok(n) = s.trim().parse::<u32>() {
            return n;
        }
    }
    #[cfg(windows)]
    {
        if let Some(s) = cmd_out(
            "powershell",
            &["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"],
        ) {
            if let Ok(n) = s.trim().parse::<u32>() {
                return n;
            }
        }
    }
    0
}

fn home_onedrive(home: &str) -> bool {
    let l = home.to_ascii_lowercase();
    if l.contains("onedrive") {
        return true;
    }
    // 家目录本身没被搬走，但桌面/文档被重定向进 OneDrive —— 同样会炸
    if let Ok(od) = std::env::var("OneDrive") {
        if !od.is_empty() && PathBuf::from(&od).join("Desktop").is_dir() {
            return true;
        }
    }
    false
}

fn long_paths_enabled() -> bool {
    #[cfg(windows)]
    {
        return reg_query(
            r"HKLM\SYSTEM\CurrentControlSet\Control\FileSystem",
            "LongPathsEnabled",
        )
        .map(|v| v.trim() == "0x1")
        .unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        true // 非 Windows 无此限制
    }
}

fn defender_realtime() -> bool {
    #[cfg(windows)]
    {
        return cmd_out(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-MpComputerStatus).RealTimeProtectionEnabled",
            ],
        )
        .map(|s| s.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn proxy_on() -> bool {
    for k in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY"] {
        if std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false) {
            return true;
        }
    }
    #[cfg(windows)]
    {
        if let Some(v) = reg_query(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "ProxyEnable",
        ) {
            return v.trim() == "0x1";
        }
    }
    false
}

fn locale() -> String {
    if let Ok(l) = std::env::var("LANG") {
        if !l.is_empty() {
            return l.split('.').next().unwrap_or(&l).to_string();
        }
    }
    #[cfg(windows)]
    {
        if let Some(s) = reg_query(r"HKCU\Control Panel\International", "LocaleName") {
            return s.trim().to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 指纹里**绝不能**出现路径原文 / 用户名 —— 这条塌了整个采集就不能上线。
    #[test]
    fn fingerprint_carries_no_path_text() {
        let fp = EnvFingerprint {
            v: SCHEMA_VERSION,
            path_nonascii: true,
            path_space: true,
            home_onedrive: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&fp).unwrap();
        // 只有布尔特征，没有任何路径分隔符携带的原文
        assert!(!json.contains("C:\\"), "指纹泄露了 Windows 路径: {json}");
        assert!(!json.contains("/Users/"), "指纹泄露了 Mac 路径: {json}");
        assert!(json.contains("\"path_nonascii\":true"), "特征位丢了: {json}");
    }

    /// 平台后缀不归一，同一个 git 在 Win/Mac 上会分成两组，跨平台对比直接失真。
    #[test]
    fn version_normalizes_to_numeric_prefix() {
        assert_eq!("2.49.0", normalize_version("2.49.0.windows.1"));
        assert_eq!("22.14.0", normalize_version("22.14.0"));
        assert_eq!("10.9.2", normalize_version("10.9.2"));
        assert_eq!("", normalize_version("unknown"));
    }
}
