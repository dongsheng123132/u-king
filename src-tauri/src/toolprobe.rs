//! 工具可用性实测跑道 —— 回答「配好的驱动，这个工具**真的能用吗**」。
//!
//! ## 为什么必须真跑，不能看配置文件
//! Crush 那个 bug（2026-08-04）：配置写了半年、`configured` 报得漂漂亮亮，而 crush 读的是
//! 另一个目录，一次都没生效。**形状对 ≠ 能用**。同类的还有 Hermes 的 `%LOCALAPPDATA%`、
//! Token 压缩机的「装了但不在 PATH 上」——报告是对的，世界是坏的。
//! 唯一能戳穿这类 bug 的手段就是拿配好的驱动真发一句话，看它回不回。
//!
//! ## 这条跑道给谁跑
//! **我们自己，发版前跑**，不在客户机上自动跑 —— 每跑一轮都在烧真金白银的 token。
//! 分工是清楚的：
//!   · 「配了能不能用」是**我们该测的**（标准配置下的事实，不需要客户数据）→ 本模块
//!   · 「有没有人用」才需要客户侧数据 → `metrics::log_tool_use`
//!
//! ## 独立可插拔
//! 纯函数，不碰 `AppHandle`、不 import 其它功能模块；进度用 `emit` 回调传出。
//! 删掉本模块只需动 lib.rs 两处（`mod` + `--toolstack-probe` 分支）。

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// 让模型原样回显的标记。取个不像自然语言的串，避免它出现在思考过程里造成误判。
const MARKER: &str = "UKPROBE7X";
const PROMPT: &str = "Reply with exactly: UKPROBE7X";

/// 单个工具跑一次的死线。推理型模型（deepseek-v4-pro）先烧 reasoning 再写正文，给足时间；
/// 到点仍没回 = 对客户而言就是不能用，如实记 `ok:false`，不给它「再等等也许行」的宽容。
const TIMEOUT: Duration = Duration::from_secs(150);

/// 各工具的无头入口。**每一条都是 2026-08-04 在本机实测过的写法**，不是照文档抄的。
/// 加新工具时：先手工把这条命令跑通，再往这儿加——否则跑道会把「我们命令写错了」
/// 报成「这个工具坏了」，然后据此下架一个其实好好的工具。
const PROBES: &[(&str, Option<&[&str]>)] = &[
    ("claude", Some(&["-p", PROMPT])),
    ("codex", Some(&["exec", PROMPT])),
    ("hermes", Some(&["-z", PROMPT])),
    ("pi", Some(&["-p", PROMPT])),
    ("qwen", Some(&["-p", PROMPT])),
    ("crush", Some(&["run", PROMPT])),
    ("opencode", Some(&["run", PROMPT])),
    // openclaw = None：**它没有可靠的无头一次性推理入口**，不是它坏了。
    // 实测（2026-08-04）：`infer model run` 走的是另一套模型目录（catalog），
    // 认不出 agent 侧注册的 provider —— 显式传 `--model custom-ukingxia/deepseek-v4-flash`
    // 直接 `Unknown model`，而 U-King 写的 openclaw.json / auth-profiles.json /
    // models.json 三份配置一个字段都没错。它的正常用法是 `gateway run` + 面板。
    //
    // 与其留一条会误报的跑道，不如如实说「测不了」：按本表开头那条规矩，
    // 把「我们命令用错了」报成「这个工具坏了」，会导致据此砍掉一个其实好好的工具。
    ("openclaw", None),
];

#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    pub tool: String,
    /// 命令在不在 PATH 上。`false` 时不跑，`ok` 也为 false，但 `note` 会写明是「没装」
    /// —— **没装不是坏**，这两件事在报告里绝不能混为一谈（客户没装 ≠ 我们的 bug）。
    pub installed: bool,
    /// **真跑过吗**。`false` = 没装、或这个工具压根没有无头入口 —— 这两种都不是
    /// 「坏」，报告里必须跟 `ok:false` 分开算，否则会把「我们测不了」说成「它不能用」。
    pub probed: bool,
    pub ok: bool,
    pub ms: u64,
    /// 失败原因（已截断，不含 Key）。成功时为空。
    pub note: String,
}

/// 跑一遍。
///
/// `only=Some("pi")` 只跑一个；`emit` 收进度（CLI 打到 stderr）。
///
/// `sandbox` 决定**这一跑测的是什么**，两种含义别混：
///  · `None` —— 用客户机真实配置跑，回答「**这台机器现在**能不能用」（排障用）
///  · `Some(dir)` —— 把各 CLI 的 home 全指到沙箱，回答「**U-King 刚配出来的**能不能用」
///    （发版回归用）。只有后者能戳穿 Crush 那类 bug：它要的是「我们写的配置，
///    工具读到了吗」，跟客户机上原本是什么状态无关。
pub fn probe_all(only: Option<&str>, sandbox: Option<&std::path::Path>, emit: &dyn Fn(&str)) -> Vec<ProbeResult> {
    let mut out = Vec::new();
    for (tool, args) in PROBES {
        if let Some(o) = only {
            if o != *tool {
                continue;
            }
        }
        // 没有无头入口的工具：如实报「测不了」，不计入 broken（见 PROBES 表注释）
        let Some(args) = args else {
            let installed = crate::installer::tool_installed(tool);
            emit(&format!("{tool}: 无无头入口，本跑道测不了"));
            out.push(ProbeResult {
                tool: (*tool).into(),
                installed,
                probed: false,
                ok: false,
                ms: 0,
                note: "无可靠的无头入口（正常用法是 gateway + 面板），本跑道测不了".into(),
            });
            continue;
        };
        if !crate::installer::tool_installed(tool) {
            emit(&format!("{tool}: 未安装，跳过"));
            out.push(ProbeResult {
                tool: (*tool).into(),
                installed: false,
                probed: false,
                ok: false,
                ms: 0,
                note: "未安装（不是故障）".into(),
            });
            continue;
        }
        emit(&format!("{tool}: 实测中…"));
        let started = Instant::now();
        let r = run_with_timeout(tool, args, sandbox);
        let ms = started.elapsed().as_millis() as u64;
        let (ok, note) = match r {
            Ok(stdout) if stdout.contains(MARKER) => (true, String::new()),
            // 跑起来了但没回出标记：可能是驱动没配上、余额不足、模型不认这个请求形状。
            // 一律算不可用 —— 客户看到的就是「问了没有用的回答」。
            Ok(stdout) => (false, format!("跑通但没回出标记：{}", tail(&stdout, 140))),
            Err(e) => (false, e),
        };
        emit(&format!(
            "{tool}: {} ({}ms){}",
            if ok { "✓" } else { "✗" },
            ms,
            if note.is_empty() { String::new() } else { format!(" {note}") }
        ));
        out.push(ProbeResult { tool: (*tool).into(), installed: true, probed: true, ok, ms, note });
    }
    out
}

/// 带死线地跑一条命令收 stdout。
///
/// 不复用 `installer::run_capture_raw`：它用 `.output()` 直等到进程结束，**没有超时**。
/// 这些 CLI 恰恰有「跑完不退出」的毛病（pi 实测过），一挂就把整条跑道钉死在那儿。
fn run_with_timeout(program: &str, args: &[&str], sandbox: Option<&std::path::Path>) -> Result<String, String> {
    let exe = resolve_exe(program).ok_or_else(|| format!("PATH 上找不到 {program}"))?;
    let mut c = Command::new(exe);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", path_env());
    // 沙箱模式：把各 CLI 认 home 的那几个变量全指过去。**必须一次全设** ——
    // 少设一个，那个工具就会去读真实机器的配置，跑出来的绿是假的（实测踩过：
    // 只改 USERPROFILE 时 crush 仍读真实 %LOCALAPPDATA%，结论完全不作数）。
    if let Some(sb) = sandbox {
        let s = sb.to_string_lossy().to_string();
        c.env("USERPROFILE", &s)
            .env("HOME", &s)
            .env("LOCALAPPDATA", sb.join("LocalAppData"))
            .env("APPDATA", sb.join("AppData"))
            // openclaw 有自己的 home 解析，不跟 USERPROFILE 走（实测）
            .env("OPENCLAW_HOME", sb.join(".uking").join("openclaw"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = c.spawn().map_err(|e| format!("启动失败: {e}"))?;

    // stdout/stderr 必须并发读走：管道写满而没人读，子进程会阻塞在写上，
    // 于是它永远不退出、我们永远等不到 —— 那是把死锁误判成「这个工具很慢」。
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let h_out = std::thread::spawn(move || read_all(&mut so));
    let h_err = std::thread::spawn(move || read_all(&mut se));

    let started = Instant::now();
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) => {
                if started.elapsed() >= TIMEOUT {
                    break true;
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(e) => return Err(format!("等待失败: {e}")),
        }
    };
    if timed_out {
        kill_tree(&mut child);
    }
    let stdout = h_out.join().unwrap_or_default();
    let stderr = h_err.join().unwrap_or_default();
    if timed_out {
        return Err(format!("超时（>{}s）", TIMEOUT.as_secs()));
    }
    if stdout.contains(MARKER) {
        return Ok(stdout);
    }
    // 没回出标记时把 stderr 也带上——真正的原因（unauthorized / 余额 / 模型名）通常在那儿
    Ok(format!("{stdout}\n{stderr}"))
}

/// 杀掉整棵进程树。
///
/// **按 PID 杀，不按镜像名** —— `taskkill /IM` 会把本机所有同名进程一起端了，
/// 而这些 CLI 的镜像名恰恰高度撞车（claude.exe 既是 CLI 也是桌面版）。见宪法与
/// `backup.rs::LOCKING_IMAGE_NAMES` 那段教训。`/T` 是必须的：.cmd 包装底下的真进程
/// （node/python）是子进程，只杀父的话它会活着继续烧 token。
pub(crate) fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let mut k = Command::new("taskkill");
        k.args(["/PID", &pid, "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        use std::os::windows::process::CommandExt;
        k.creation_flags(0x0800_0000);
        let _ = k.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_all<R: Read>(r: &mut Option<R>) -> String {
    let Some(r) = r.as_mut() else {
        return String::new();
    };
    let mut buf = Vec::new();
    let _ = r.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

/// 在 PATH 上找出这个工具的**真实可执行文件**。
///
/// `pub(crate)`：竞技场（arena.rs）也要按工具找 exe，这是同一份能力 —— 找错扩展名
/// 会把「没装」报成「坏了」，宪法第 12 条复用不复制。
///
/// 不能图省事拼 `format!("{tool}.cmd")`：本机实测这几个的落点各不相同 ——
/// npm 装的是 `.cmd`（pi/qwen/crush/opencode/openclaw/codex）、claude 在 `~/bin/` 下没扩展名、
/// hermes 在便携 Python 的 `Scripts/` 里是 `.exe`。猜错扩展名会报成「没装」，
/// 而「没装」在报告里是要当成下架依据的 —— 猜错一次就可能砍掉一个好好的工具。
///
/// 也不能只靠 `Command::new("pi")` 让系统解析：Windows 的 CreateProcess **不查 PATHEXT**，
/// 无扩展名的 `.cmd` 一律找不到。
pub(crate) fn resolve_exe(tool: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &["cmd", "exe", "bat", ""] } else { &[""] };
    let mut dirs: Vec<PathBuf> = crate::installer::search_paths(None);
    if let Ok(p) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    for d in dirs {
        for ext in exts {
            let cand = if ext.is_empty() {
                d.join(tool)
            } else {
                d.join(format!("{tool}.{ext}"))
            };
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// 复用安装/验证时同一份 PATH（便携 Node、`%APPDATA%\npm`、便携 Python Scripts 都在里头）。
/// 跟内置终端同源 —— 否则会出现「终端里能跑、跑道说没装」这种自己骗自己的结果。
pub(crate) fn path_env() -> String {
    let mut dirs: Vec<PathBuf> = crate::installer::search_paths(None);
    if let Ok(p) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(dirs).map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn tail(s: &str, n: usize) -> String {
    let t = s.trim();
    let t: String = t.chars().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect();
    t.replace(['\n', '\r'], " ")
}
