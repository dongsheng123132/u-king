//! 本地大模型 —— 四个引擎的探测 / 安装 / 启停 / 模型扫描。
//!
//! ## 四个引擎，两类人
//!
//! | 引擎 | 谁用 | 端点 | 模型格式 |
//! |---|---|---|---|
//! | **Ollama** | 小白默认。装完就有后台服务，`ollama run` 就能聊 | `:11434/v1` | 自带仓库 |
//! | **llama.cpp**（`llama-server`） | 想自己挑量化版本的人。CPU 也能跑 | 自选端口 `/v1` | GGUF 文件 |
//! | **vLLM** | 有 Linux + N 卡服务器的人。吞吐最高 | 自选端口 `/v1` | HuggingFace 目录 |
//! | **SGLang** | 同上，长上下文/结构化输出更强 | 自选端口 `/v1` | HuggingFace 目录 |
//!
//! 🔴 **vLLM / SGLang 在 Windows 上装不了**（官方只支持 Linux + CUDA/ROCm）。这不是我们
//! 没做，是上游如此。所以它们的 `blockers` 会直说这件事，而不是让客户点了 START 之后
//! 对着一个失败日志猜 —— U-King 的客户几乎全是 Windows 个人机，这两个引擎对他们的
//! 正确答案是「你这台跑不了」，说清楚比藏起来强。
//!
//! ## 为什么四个都要
//!
//! 2026-08-11「简化第三刀」把这一页连同 `ollama.rs` 一起删了（只留 `hardware.rs`，
//! 因为 AI 优化大师在用）。现在按 EchoBird 的形态恢复：**引擎不是一个，是一个货架** ——
//! 同一台机器上「能跑什么」由硬件决定，客户需要的是「告诉我我这台该用哪个」，
//! 而不是我们替他选一个然后在别的机器上失灵。`hardware.rs` 那半（能跑多大模型、
//! 推荐哪个尺寸）当年就是为这一页写的，一直留着。
//!
//! ## 约定
//!
//! - 纯 std + 子进程，不引第三方 crate（守体积红线）；子进程一律 `CREATE_NO_WINDOW`
//! - **停进程只按 PID，绝不按镜像名**（宪法：`taskkill /IM` 会误杀同名的别人）。
//!   杀之前先核对这个 PID 的镜像名跟我们启动的那个对得上，防 PID 复用误伤
//! - 启动信息落 `~/.uking/localllm/<engine>.json`，日志落同目录 `<engine>.log`
//!
//! ---
//! 以下 Ollama 部分沿用 0.9.94 前的 `ollama.rs`（`45aa948` 删掉的那份）。
//! 选 Ollama 因为：① 自带 `localhost:11434/v1` 的 OpenAI 兼容端点（能直接当 ClawX/Hermes
//! 的 provider）② CLI 友好，`ollama run <model>` 在内置终端就能聊 ③ 有模型仓库，一键 pull。
//!
//! 安装走「下 OllamaSetup.exe → Inno Setup 静默装」，与 tools.rs::install_clawx 同一套路
//! （下载带进度事件、静默失败回退常规界面）。模型 pull/run 交给内置 PTY 终端（term.rs 白名单
//! 已放行 `ollama`），这里只管「装引擎 + 探测 + 列已装模型」。
//!
//! ⚠️ 体积/磁盘：引擎 + 模型都装在本地硬盘（非 U 盘，IO 慢）。模型动辄数 GB，默认落
//! `%USERPROFILE%\.ollama\models`（C 盘）。UI 已对磁盘占用做提示，教程页讲了怎么用
//! `OLLAMA_MODELS` 改到别的盘。

use serde::Serialize;
use std::path::PathBuf;

/// OllamaSetup.exe 下载源（依次尝试，第一个把文件下到 >100MB 的算成功）。
/// **不走自己服务器** —— 官方直链 + 国内第三方 GitHub 加速代理混排：
///  ① 官方 ollama.com 直链（CDN，curl -L 跟随 302 到 GitHub objects）
///  ② 国内 ghproxy 系镜像（裸网客户直连 GitHub 慢/被墙，靠它加速；代理偶尔挂，多列几个轮）
///  ③ 官方 GitHub latest 兜底（开了代理的机器走这条最稳）
/// 代理 URL = 在 GitHub 直链前拼代理前缀（ghproxy 系约定）。代理会换/会挂，下面是当前
/// 实测较稳的几个；某个失效不影响整体（loop 自动跳下一个）。
#[cfg(windows)]
const OLLAMA_SETUP_URLS: &[&str] = &[
    "https://ollama.com/download/OllamaSetup.exe",
    "https://ghfast.top/https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe",
    "https://gh-proxy.com/https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe",
    "https://ghproxy.net/https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe",
    "https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe",
];

#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    /// 引擎是否已安装
    pub installed: bool,
    /// 本地服务（11434）是否在跑
    pub serving: bool,
    /// 版本（探到才有）
    pub version: Option<String>,
    /// 已 pull 到本地的模型 id 列表
    pub models: Vec<String>,
    /// **端到端真的能用吗**（引擎在跑 + 至少有一个模型）。
    pub ready: bool,
    /// ready=false 时说人话：卡在哪。
    pub blockers: Vec<String>,
}

/// Ollama 可执行路径（Windows 安装在 %LOCALAPPDATA%\Programs\Ollama）。
#[cfg(windows)]
fn ollama_exe() -> Option<PathBuf> {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("Programs").join("Ollama").join("ollama.exe");
        if p.exists() {
            return Some(p);
        }
    }
    // PATH 兜底（用户可能装在别处 / 老版本路径）
    crate::installer::search_paths(None)
        .into_iter()
        .map(|d| d.join("ollama.exe"))
        .find(|p| p.exists())
}

#[cfg(not(windows))]
fn ollama_exe() -> Option<PathBuf> {
    for p in ["/usr/local/bin/ollama", "/opt/homebrew/bin/ollama"] {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    crate::installer::search_paths(None)
        .into_iter()
        .map(|d| d.join("ollama"))
        .find(|p| p.exists())
}

/// 是否已装。
pub fn ollama_installed() -> bool {
    ollama_exe().is_some() || crate::installer::tool_installed("ollama")
}

/// 跑 ollama 子命令收 stdout（CREATE_NO_WINDOW；失败返回 None）。
fn run_ollama(args: &[&str]) -> Option<String> {
    let exe = ollama_exe()?;
    let mut c = std::process::Command::new(exe);
    c.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 已 pull 的本地模型列表（`ollama list` 首列）。
fn local_models() -> Vec<String> {
    let Some(out) = run_ollama(&["list"]) else {
        return Vec::new();
    };
    out.lines()
        .skip(1) // 表头 NAME ID SIZE MODIFIED
        .filter_map(|l| l.split_whitespace().next())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// 导入本地 GGUF 模型文件（测试报告 #022「不支持自定义导入」）。
///
/// ## 为什么必须是后端命令，不能像 pull 那样丢给内置终端
/// `term_open_external` 有命令白名单：参数只允许 `[A-Za-z0-9-_=./:]`，**反斜杠不在里面**。
/// 而用户挑出来的 GGUF 路径长这样：`C:\models\qwen.gguf` —— 一定带反斜杠，一定被拦。
/// 那个白名单是防 shell 注入的，不该为了这个功能开个口子；把这条路做成后端命令，
/// 路径根本不进命令行解析，反而更安全。
///
/// 做法是 Ollama 官方的标准姿势：写一个只有一行 `FROM <绝对路径>` 的 Modelfile，
/// 再 `ollama create <name> -f <Modelfile>`。Modelfile 落在 `~/.uking/ollama/`，
/// 不往用户的模型目录里塞东西。
pub fn import_gguf(path: &str, name: &str) -> Result<String, String> {
    let src = PathBuf::from(path);
    if !src.is_file() {
        return Err(format!("找不到这个文件：{path}"));
    }
    if !src
        .extension()
        .map(|e| e.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false)
    {
        return Err("只认 .gguf 文件（Ollama 能直接吃的就是这种）".into());
    }
    // 模型名会变成 `ollama run <name>` 的参数，卡死在保守字符集里 ——
    // 用户随手起的中文名/带空格的名字，后面每次 run 都会踩坑，不如现在就拦下来。
    let ok_name = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c));
    if !ok_name {
        return Err("模型名只能用字母、数字和 - _ . :（这个名字之后要当命令参数用）".into());
    }

    let exe = ollama_exe().ok_or("没找到 ollama，先装引擎")?;
    // 各模块自己算 ~/.uking（同 video.rs / installer.rs 的做法）——
    // 模块要能整块拔插，就不为一行路径去 import 别人。
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(home).join(".uking").join("ollama");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败：{e}"))?;
    let mf = dir.join(format!("Modelfile-{name}"));
    std::fs::write(&mf, format!("FROM {}\n", src.display())).map_err(|e| format!("写 Modelfile 失败：{e}"))?;

    let mut c = std::process::Command::new(exe);
    c.args(["create", name, "-f"])
        .arg(&mf)
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().map_err(|e| format!("起 ollama 失败：{e}"))?;
    if out.status.success() {
        Ok(format!("已导入：{name}（现在可以在「已下载」里直接运行它）"))
    } else {
        // stderr 才是 ollama 说人话的地方，原样带回去给用户看（截断按**字符**切，
        // 按字节切会把中文切成半个字 → 整个应用当场消失，0.9.82 刚被这个坑过）
        let err = String::from_utf8_lossy(&out.stderr);
        let err: String = err.trim().chars().take(600).collect();
        Err(if err.is_empty() { "导入失败（ollama 没给原因）".into() } else { err })
    }
}

/// 本地服务是否在跑（`ollama list` 能通就说明 serve 活着）。
fn serving() -> bool {
    // `ollama list` 在 serve 没起时会报错/空；用它探活最省事（不另起 http 依赖）
    run_ollama(&["list"]).map(|o| o.contains("NAME") || !o.trim().is_empty()).unwrap_or(false)
}

/// 完整状态（前端板块用）。
pub fn status() -> OllamaStatus {
    if !ollama_installed() {
        return OllamaStatus {
            installed: false,
            serving: false,
            version: None,
            models: Vec::new(),
            ready: false,
            blockers: vec!["还没安装 Ollama 引擎".into()],
        };
    }
    let version = run_ollama(&["--version"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let serving = serving();
    let models = local_models();
    // 「装了」和「能用」是两回事：引擎没在跑、或一个模型都没拉，
    // 用户点「开始对话」照样是空的。如实说清楚卡在哪。
    let mut blockers = Vec::new();
    if !serving {
        blockers.push("引擎没在跑（本地 11434 端口没人服务）".to_string());
    } else if models.is_empty() {
        blockers.push("引擎在跑，但一个模型都没下载".to_string());
    }
    OllamaStatus {
        installed: true,
        serving,
        ready: serving && !models.is_empty(),
        blockers,
        version,
        models,
    }
}

/// 下载 + 静默安装 Ollama 引擎。进度走回调（前端 toast 实时显示）。
/// 流程对齐 install_clawx：下 exe（国内镜像优先）→ Inno Setup 静默装 → 轮询装好。
#[cfg(windows)]
pub fn install_ollama(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if ollama_installed() {
        return Ok("Ollama 已安装。".into());
    }
    // 700MB 的下载 + 静默安装，在客户的慢网/杀软环境下最容易半路死。
    // 同 toolbox：包一层进度回调，界面看到什么日志里就有什么（含试了第几个下载源、下到多少 MB）。
    crate::ulog::section("ollama", "开始安装 Ollama 引擎");
    let notify = on_progress;
    let on_progress = |m: &str| {
        crate::ulog::write("ollama", m);
        notify(m);
    };

    let tmp = std::env::temp_dir().join("OllamaSetup-uking.exe");
    let _ = std::fs::remove_file(&tmp);

    // 依次尝试各下载源，第一个把文件下到 >100MB 的算成功（OllamaSetup ~700MB-1GB）。
    // curl 策略：连不上(15s)/龟速(30s 内 <30KB/s)就快速放弃跳下一个源，避免卡在死镜像上；
    // 健康但慢的源用 -m 1800（30min）兜底，留足时间下完大包。
    on_progress("开始下载 Ollama 引擎（约 700 MB，首次较久）…");
    let total = OLLAMA_SETUP_URLS.len();
    let mut ok = false;
    for (i, url) in OLLAMA_SETUP_URLS.iter().enumerate() {
        on_progress(&format!("尝试下载源 {}/{}…", i + 1, total));
        let _ = std::fs::remove_file(&tmp);
        let mut child = match std::process::Command::new(crate::installer::system_tool("curl"))
            .args([
                "-sSL",
                "--connect-timeout", "15",
                "--speed-time", "30",
                "--speed-limit", "30000",
                "-m", "1800",
                "-A", "Mozilla/5.0 U-King",
                "-o", &tmp.to_string_lossy(),
                url,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue,
        };
        const EST_TOTAL_MB: u64 = 700;
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(700));
            let mb = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) / 1_000_000;
            if mb > 0 {
                let pct = ((mb * 100) / EST_TOTAL_MB).min(99);
                on_progress(&format!("正在下载 Ollama… {mb} MB（约 {pct}%）"));
            }
        }
        if std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) > 100_000_000 {
            ok = true;
            break;
        }
    }
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        return Err("Ollama 下载失败。可在板块里点「打开官网」手动下 OllamaSetup.exe 安装。".into());
    }
    on_progress("下载完成，正在安装 Ollama 引擎…");

    // Inno Setup 静默安装：/VERYSILENT。Ollama 是**每用户**安装（装到 %LOCALAPPDATA%，免管理员），
    // 装完会自动起后台服务（系统托盘）。静默被拦 → 回退常规安装界面。
    let silent = std::process::Command::new(&tmp)
        .args(["/VERYSILENT", "/NORESTART", "/SUPPRESSMSGBOXES"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match silent {
        Ok(mut child) => {
            for _ in 0..45 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if ollama_installed() {
                    let _ = std::fs::remove_file(&tmp);
                    on_progress("Ollama 安装完成，正在启动本地服务…");
                    return Ok("Ollama 引擎已安装完成，接下来可以下载模型了。".into());
                }
                if let Ok(Some(_)) = child.try_wait() {
                    // 进程退出但还没探到，再等几轮
                }
            }
            on_progress("自动安装较慢，已打开安装界面，请按提示点「Install」…");
            let _ = std::process::Command::new(&tmp).spawn();
            Ok("已为你打开 Ollama 安装程序，按提示装完即可。".into())
        }
        Err(_) => {
            std::process::Command::new(&tmp)
                .spawn()
                .map_err(|e| format!("启动 Ollama 安装程序失败: {e}"))?;
            Ok("已打开 Ollama 安装程序，按提示安装即可。".into())
        }
    }
}

#[cfg(not(windows))]
pub fn install_ollama(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请到板块里点「打开官网」下载 Ollama（macOS 版 .app）安装。".into())
}

// ============================================================
// 多引擎层（llama.cpp / vLLM / SGLang，+ 上面那套 Ollama）
// ============================================================

/// 一个引擎的完整状态。**回答「能不能用」，不是「装没装」**（影核 readiness 约定）。
#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    /// 稳定 id：ollama / llamacpp / vllm / sglang
    pub id: String,
    pub label: String,
    /// 一句话说明这个引擎适合谁
    pub blurb: String,
    pub installed: bool,
    /// 端到端真的能用（引擎在 + 有模型 / 服务在跑）
    pub ready: bool,
    /// ready=false 时说人话：卡在哪
    pub blockers: Vec<String>,
    pub version: Option<String>,
    /// OpenAI 兼容端点（在跑才有）
    pub endpoint: Option<String>,
    /// 正在跑的进程（我们启动的那个）
    pub running_pid: Option<u32>,
    /// 正在跑的模型
    pub running_model: Option<String>,
    /// 这个引擎能用的本地模型
    pub models: Vec<String>,
    /// 这台机器压根跑不了（平台不支持）—— UI 该置灰而不是让人点了再失败
    pub unsupported_here: bool,
}

pub const ENGINE_IDS: [&str; 4] = ["ollama", "llamacpp", "vllm", "sglang"];

fn uking_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking")
}

fn localllm_dir() -> PathBuf {
    uking_dir().join("localllm")
}

fn state_path(engine: &str) -> PathBuf {
    localllm_dir().join(format!("{engine}.json"))
}

fn log_path(engine: &str) -> PathBuf {
    localllm_dir().join(format!("{engine}.log"))
}

/// stderr 单独一个文件：PowerShell 的 Start-Process **不允许 stdout 和 stderr 重定向到
/// 同一个文件**（会直接报错），而模型加载的关键信息恰恰大多走 stderr。
fn err_log_path(engine: &str) -> PathBuf {
    localllm_dir().join(format!("{engine}.err.log"))
}

/// 我们启动的那个进程的名片。**exe 名要留着** —— 停的时候拿它跟 PID 现在的镜像名核对，
/// 防 PID 复用把别人的进程当成自己的关掉。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RunRecord {
    pub pid: u32,
    pub exe_name: String,
    pub port: u16,
    pub model: String,
}

fn load_run(engine: &str) -> Option<RunRecord> {
    let s = std::fs::read_to_string(state_path(engine)).ok()?;
    serde_json::from_str(&s).ok()
}

fn save_run(engine: &str, r: &RunRecord) {
    let _ = std::fs::create_dir_all(localllm_dir());
    if let Ok(s) = serde_json::to_string_pretty(r) {
        let _ = std::fs::write(state_path(engine), s);
    }
}

fn clear_run(engine: &str) {
    let _ = std::fs::remove_file(state_path(engine));
}

/// 给 Command 加「不弹黑窗」。GUI 下起子进程必用（与 installer.rs 同口径）。
fn hidden(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd;
}

/// 起一个**常驻服务**并拿到它的 pid。日志写进 `out_log` / `err_log`。
///
/// ## 为什么不能直接 `Command::spawn`
///
/// 常驻服务活得比调用方久。Windows 上 `CreateProcess` 是 `bInheritHandles=TRUE` 起的，
/// 于是服务顺手继承了调用方的**管道写端**：`action run runtime.localllm.start | jq`
/// 里 jq 永远等不到 EOF —— **结果早打出来了，看起来却是卡死**（2026-08-19 实测：
/// JSON 已输出，管道一直到 llama-server 被 stop 掉才闭合）。
///
/// 试过并**证伪**的两条路：`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`（照样继承）、
/// `cmd /c start /b`（照样继承）。真正切得断的是 PowerShell 的 `Start-Process` ——
/// 它建进程时不把调用方的句柄传下去（实测 1.3s 返回）。
///
/// PowerShell 在客户机上可能被杀软掏空（1.0.1 就修过这个），所以**失败退回直接 spawn**：
/// 那种情况下服务照常跑，只是管道消费者收不到 EOF —— 退化，不是坏掉。
fn spawn_service(
    exe: &std::path::Path,
    args: &[String],
    out_log: &std::path::Path,
    err_log: &std::path::Path,
) -> Result<u32, String> {
    #[cfg(windows)]
    {
        // -PassThru 让它把进程对象吐回来，我们只取 Id。参数逐个用单引号包（PowerShell
        // 的单引号里只有 '' 需要转义），路径含空格/中文都安全。
        let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let arg_list = if args.is_empty() {
            String::new()
        } else {
            format!(
                " -ArgumentList @({})",
                args.iter().map(|a| q(a)).collect::<Vec<_>>().join(",")
            )
        };
        let script = format!(
            "$p = Start-Process -FilePath {} {} -WindowStyle Hidden -PassThru \
             -RedirectStandardOutput {} -RedirectStandardError {}; $p.Id",
            q(&exe.to_string_lossy()),
            arg_list,
            q(&out_log.to_string_lossy()),
            q(&err_log.to_string_lossy()),
        );
        let mut c = std::process::Command::new(crate::installer::system_tool("powershell"));
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::null());
        hidden(&mut c);
        if let Ok(out) = c.output() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(pid) = s.split_whitespace().last().and_then(|x| x.parse::<u32>().ok()) {
                return Ok(pid);
            }
            crate::ulog::write(
                "localllm",
                &format!(
                    "Start-Process 没给出 pid（退回直接 spawn）：{}",
                    String::from_utf8_lossy(&out.stderr).trim().chars().take(200).collect::<String>()
                ),
            );
        }
    }
    // 退路（非 Windows 平台走的也是这条：那边不存在句柄继承这回事）
    let log = std::fs::File::create(out_log).map_err(|e| format!("建日志失败: {e}"))?;
    let err = std::fs::File::create(err_log).map_err(|e| format!("建日志失败: {e}"))?;
    let mut c = std::process::Command::new(exe);
    c.args(args)
        .stdout(log)
        .stderr(err)
        .stdin(std::process::Stdio::null());
    detached(&mut c);
    let child = c.spawn().map_err(|e| format!("启动失败: {e}"))?;
    Ok(child.id())
}

/// 起**常驻服务**用的：隐窗 + 脱离父进程 + 自己一个进程组。
///
/// 🔴 为什么不能跟 `hidden` 用同一套：常驻服务活得比调用方久。CLI 下
/// `action run runtime.localllm.start | jq` 那条管道，只要服务进程还攥着从我们这儿
/// 继承过去的管道写端，`jq` 就永远等不到 EOF —— **结果早就打出来了，看起来却是卡死**。
/// 2026-08-19 实测：`start` 的 JSON 已经输出，管道却一直到 llama-server 被 stop 掉
/// 才闭合。DETACHED_PROCESS(0x8) 让它不再挂在调用方的控制台上，
/// CREATE_NEW_PROCESS_GROUP(0x200) 让调用方那边一个 Ctrl+C 不会顺手把服务也带走。
fn detached(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | 0x0000_0008 | 0x0000_0200);
    }
    let _ = cmd;
}

/// 这个 PID 还活着吗？活着的话镜像名是什么？
///
/// 只用来**核对**，不用来找进程 —— 按名字找进程正是宪法里明令禁止的那件事。
fn image_name_of(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new(crate::installer::system_tool("tasklist"));
        c.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
        hidden(&mut c);
        let out = c.output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let first = s.lines().next()?.trim().to_string();
        if first.is_empty() || first.contains("No tasks") {
            return None;
        }
        return first
            .trim_start_matches('"')
            .split('"')
            .next()
            .map(|x| x.to_string());
    }
    #[cfg(not(windows))]
    {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

/// 本地模型目录。默认 `~/.uking/models`，客户可以再加别的盘
/// （模型动辄几 GB，C 盘放不下是常态）。
pub fn model_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![uking_dir().join("models")];
    if let Ok(s) = std::fs::read_to_string(localllm_dir().join("dirs.txt")) {
        for line in s.lines() {
            let t = line.trim();
            let p = PathBuf::from(t);
            if !t.is_empty() && p.is_dir() {
                dirs.push(p);
            }
        }
    }
    dirs
}

/// 追加一个模型目录（去重、只收真实存在的目录）。
pub fn add_model_dir(dir: &str) -> Result<Vec<String>, String> {
    let p = PathBuf::from(dir.trim());
    if !p.is_dir() {
        return Err(format!("目录不存在：{dir}"));
    }
    let _ = std::fs::create_dir_all(localllm_dir());
    let f = localllm_dir().join("dirs.txt");
    let mut cur: Vec<String> = std::fs::read_to_string(&f)
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let norm = p.to_string_lossy().to_string();
    if !cur.iter().any(|c| c == &norm) {
        cur.push(norm);
    }
    std::fs::write(&f, cur.join("\n")).map_err(|e| format!("写目录清单失败: {e}"))?;
    Ok(cur)
}

/// 扫 GGUF（llama.cpp 吃这个）。只扫两层，别在客户几 TB 的盘上递归到天荒地老。
fn scan_gguf() -> Vec<String> {
    let mut out = Vec::new();
    for dir in model_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "gguf").unwrap_or(false) {
                out.push(p.to_string_lossy().to_string());
            } else if p.is_dir() {
                if let Ok(sub) = std::fs::read_dir(&p) {
                    for e2 in sub.flatten() {
                        let p2 = e2.path();
                        if p2.extension().map(|x| x == "gguf").unwrap_or(false) {
                            out.push(p2.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// 扫 HuggingFace 模型目录（vLLM / SGLang 吃这个）：有 config.json 且有权重文件的目录。
fn scan_hf() -> Vec<String> {
    let mut out = Vec::new();
    for dir in model_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() || !p.join("config.json").exists() {
                continue;
            }
            let has_weights = std::fs::read_dir(&p)
                .map(|r| {
                    r.flatten().any(|f| {
                        let n = f.file_name().to_string_lossy().to_string();
                        n.ends_with(".safetensors") || n.ends_with(".bin")
                    })
                })
                .unwrap_or(false);
            if has_weights {
                out.push(p.to_string_lossy().to_string());
            }
        }
    }
    out.sort();
    out
}

/// 找 llama-server 可执行文件：`~/.uking/llama-server/bin/` → PATH。
fn llama_server_exe() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let bundled = uking_dir().join("llama-server").join("bin").join(name);
    if bundled.exists() {
        return Some(bundled);
    }
    crate::installer::search_paths(None)
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| p.exists())
}

/// python 解释器（vLLM / SGLang 都是 python 包）。
fn python_exe() -> Option<PathBuf> {
    for name in ["python3", "python"] {
        let n = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        };
        if let Some(p) = crate::installer::search_paths(None)
            .into_iter()
            .map(|d| d.join(&n))
            .find(|p| p.exists())
        {
            return Some(p);
        }
    }
    None
}

/// python 包装没装（`python -c "import x"` 退出码为准，不解析文案）。
fn python_module_present(module: &str) -> bool {
    let Some(py) = python_exe() else {
        return false;
    };
    let mut c = std::process::Command::new(py);
    c.args(["-c", &format!("import {module}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hidden(&mut c);
    c.status().map(|s| s.success()).unwrap_or(false)
}

/// 端口上有没有人在服务（本地 TCP 连一下，最直接）。
fn port_alive(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(400)).is_ok()
}

/// 我们启动的那个进程还活着吗？活着返回它的名片。
///
/// 名片里的 exe 名跟 PID 现在的镜像名对不上 = PID 被系统复用了，
/// 当成「没在跑」并把陈旧名片清掉，绝不把别人的进程认领成自己的。
fn live_run(engine: &str) -> Option<RunRecord> {
    let r = load_run(engine)?;
    match image_name_of(r.pid) {
        Some(name) if name.eq_ignore_ascii_case(&r.exe_name) => Some(r),
        _ => {
            clear_run(engine);
            None
        }
    }
}

/// 四个引擎的状态一次给全（前端一页展示）。
pub fn inspect_all() -> Vec<EngineStatus> {
    ENGINE_IDS.iter().map(|id| inspect(id)).collect()
}

/// 单个引擎状态。
pub fn inspect(engine: &str) -> EngineStatus {
    match engine {
        "ollama" => {
            let s = status();
            let run = live_run("ollama");
            EngineStatus {
                id: "ollama".into(),
                label: "Ollama".into(),
                blurb: "小白默认。装完自带后台服务，模型一键拉".into(),
                installed: s.installed,
                ready: s.ready,
                blockers: s.blockers,
                version: s.version,
                endpoint: if s.serving {
                    Some("http://127.0.0.1:11434/v1".to_string())
                } else {
                    None
                },
                running_pid: run.as_ref().map(|r| r.pid),
                running_model: run.map(|r| r.model),
                models: s.models,
                unsupported_here: false,
            }
        }
        "llamacpp" => {
            let exe = llama_server_exe();
            let models = scan_gguf();
            let run = live_run("llamacpp");
            let mut blockers = Vec::new();
            if exe.is_none() {
                blockers.push("还没有 llama-server（llama.cpp 的服务端）".to_string());
            } else if models.is_empty() {
                blockers.push(format!(
                    "找不到 GGUF 模型文件。把 .gguf 放进 {} 或添加模型目录",
                    model_dirs()[0].display()
                ));
            }
            EngineStatus {
                id: "llamacpp".into(),
                label: "llama.cpp".into(),
                blurb: "自己挑量化版本，没有独显也能跑".into(),
                installed: exe.is_some(),
                ready: exe.is_some() && !models.is_empty(),
                blockers,
                version: None,
                endpoint: run.as_ref().map(|r| format!("http://127.0.0.1:{}/v1", r.port)),
                running_pid: run.as_ref().map(|r| r.pid),
                running_model: run.map(|r| r.model),
                models,
                unsupported_here: false,
            }
        }
        "vllm" | "sglang" => {
            let is_vllm = engine == "vllm";
            let module = if is_vllm { "vllm" } else { "sglang" };
            // 🔴 上游只支持 Linux + CUDA/ROCm。Windows/macOS 上说清楚跑不了，
            //    别让客户点了 START 之后对着 pip 的报错猜。
            let unsupported = cfg!(windows) || cfg!(target_os = "macos");
            let installed = !unsupported && python_module_present(module);
            let models = if unsupported { Vec::new() } else { scan_hf() };
            let run = live_run(engine);
            let mut blockers = Vec::new();
            if unsupported {
                blockers.push(format!(
                    "{} 只能跑在 Linux + N 卡（CUDA）上，这台机器跑不了 —— 个人电脑请用 Ollama 或 llama.cpp",
                    if is_vllm { "vLLM" } else { "SGLang" }
                ));
            } else if python_exe().is_none() {
                blockers.push("找不到 python3".to_string());
            } else if !installed {
                blockers.push(format!("python 里没装 {module}（pip install {module}）"));
            } else if models.is_empty() {
                blockers.push(
                    "没有 HuggingFace 格式的模型目录（要有 config.json + 权重文件）".to_string(),
                );
            }
            EngineStatus {
                id: engine.to_string(),
                label: if is_vllm {
                    "vLLM".to_string()
                } else {
                    "SGLang".to_string()
                },
                blurb: if is_vllm {
                    "有 Linux + N 卡服务器时吞吐最高".to_string()
                } else {
                    "长上下文 / 结构化输出更强，同样要 Linux + N 卡".to_string()
                },
                installed,
                ready: installed && !models.is_empty(),
                blockers,
                version: None,
                endpoint: run.as_ref().map(|r| format!("http://127.0.0.1:{}/v1", r.port)),
                running_pid: run.as_ref().map(|r| r.pid),
                running_model: run.map(|r| r.model),
                models,
                unsupported_here: unsupported,
            }
        }
        other => EngineStatus {
            id: other.to_string(),
            label: other.to_string(),
            blurb: String::new(),
            installed: false,
            ready: false,
            blockers: vec![format!("没有这个引擎：{other}")],
            version: None,
            endpoint: None,
            running_pid: None,
            running_model: None,
            models: Vec::new(),
            unsupported_here: true,
        },
    }
}

// ============================================================
// 运行参数（每个引擎一份，落盘）
// ============================================================

/// 起服务时的参数。**落盘**（`~/.uking/localllm/<engine>.settings.json`）——
/// 客户在界面上调好的上下文/端口，下次开 U-King 还得在，不然每次都要重调一遍。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RunSettings {
    /// 服务端口。Ollama 固定 11434（它的后台服务就在那儿），别的引擎默认 18820
    pub port: u16,
    /// 上下文长度（token）。0 = 用引擎自己的默认
    pub ctx: u32,
    /// 往显卡上放几层：**-1 = 自动**（引擎自己定）· 0 = 纯 CPU · 999 = 全部上 GPU。
    /// 显存不够却全塞 GPU，llama.cpp 会当场 OOM 退出 —— 所以默认是「自动」不是「全上」。
    pub gpu_layers: i32,
    /// CPU 线程数。0 = 引擎自己按核心数定
    pub threads: u32,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self { port: 18820, ctx: 8192, gpu_layers: -1, threads: 0 }
    }
}

fn settings_path(engine: &str) -> PathBuf {
    localllm_dir().join(format!("{engine}.settings.json"))
}

/// 读这个引擎存下来的参数（没存过给默认值）。
pub fn settings(engine: &str) -> RunSettings {
    let mut s = std::fs::read_to_string(settings_path(engine))
        .ok()
        .and_then(|t| serde_json::from_str::<RunSettings>(&t).ok())
        .unwrap_or_default();
    if engine == "ollama" {
        s.port = 11434; // 它的后台服务写死在这个端口，让人改只会改出一个连不上的配置
    }
    s
}

/// 存参数（顺手把明显不合法的值夹回合法区间，别把「起不来」留到点启动时才发现）。
pub fn save_settings(engine: &str, mut s: RunSettings) -> Result<RunSettings, String> {
    if !ENGINE_IDS.contains(&engine) {
        return Err(format!("没有这个引擎：{engine}"));
    }
    if engine == "ollama" {
        s.port = 11434;
    } else if s.port < 1024 {
        s.port = 18820;
    }
    s.ctx = s.ctx.min(1_048_576);
    s.gpu_layers = s.gpu_layers.clamp(-1, 999);
    s.threads = s.threads.min(256);
    let _ = std::fs::create_dir_all(localllm_dir());
    let text = serde_json::to_string_pretty(&s).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(settings_path(engine), text).map_err(|e| format!("写参数失败: {e}"))?;
    Ok(s)
}

/// 启动一个引擎的本地服务。已经在跑就原样返回（幂等）。
///
/// `opts` 给了就用它并**存下来**；没给就用上次存的（见 `settings`）。
///
/// 返回 OpenAI 兼容端点 —— 拿到它就能当成一个 provider 接进大脑下拉。
pub fn start(engine: &str, model: &str, opts: Option<RunSettings>) -> Result<String, String> {
    if let Some(r) = live_run(engine) {
        return Ok(format!("http://127.0.0.1:{}/v1", r.port));
    }
    let st = inspect(engine);
    if st.unsupported_here {
        return Err(st.blockers.join("；"));
    }
    let cfg = match opts {
        Some(s) => save_settings(engine, s)?,
        None => settings(engine),
    };
    let _ = std::fs::create_dir_all(localllm_dir());

    if engine == "ollama" {
        // Ollama 的服务是安装时就注册的后台服务，正常情况本来就在跑。
        // 没在跑时拉一把 `ollama serve`，别让客户去开始菜单里找。
        if port_alive(11434) {
            return Ok("http://127.0.0.1:11434/v1".to_string());
        }
        let exe = ollama_exe().ok_or("Ollama 还没安装")?;
        let pid = spawn_service(
            &exe,
            &["serve".to_string()],
            &log_path("ollama"),
            &err_log_path("ollama"),
        )?;
        save_run(
            "ollama",
            &RunRecord {
                pid,
                exe_name: exe
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                port: 11434,
                model: model.to_string(),
            },
        );
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if port_alive(11434) {
                return Ok("http://127.0.0.1:11434/v1".to_string());
            }
        }
        return Err("ollama serve 起来了但 11434 没响应，看日志".to_string());
    }

    if model.trim().is_empty() {
        return Err("请先选一个模型".to_string());
    }
    let port = if cfg.port < 1024 { 18820 } else { cfg.port };
    if port_alive(port) {
        return Err(format!("端口 {port} 已被占用，换一个"));
    }

    let (exe, args): (PathBuf, Vec<String>) = match engine {
        "llamacpp" => {
            let exe = llama_server_exe().ok_or("找不到 llama-server")?;
            let mut a = vec![
                "-m".to_string(),
                model.to_string(),
                "--port".to_string(),
                port.to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
            ];
            if cfg.ctx > 0 {
                a.push("-c".into());
                a.push(cfg.ctx.to_string());
            }
            // -1 是「自动」：**一个 -ngl 都不传**，让 llama.cpp 自己按显存决定。
            // 显存不够却传 -ngl 999 会当场 OOM 退出，那种失败对客户就是「点了没反应」。
            if cfg.gpu_layers >= 0 {
                a.push("-ngl".into());
                a.push(cfg.gpu_layers.to_string());
            }
            if cfg.threads > 0 {
                a.push("-t".into());
                a.push(cfg.threads.to_string());
            }
            (exe, a)
        }
        "vllm" => {
            let py = python_exe().ok_or("找不到 python3")?;
            let mut a = vec![
                "-m".to_string(),
                "vllm.entrypoints.openai.api_server".to_string(),
                "--model".to_string(),
                model.to_string(),
                "--port".to_string(),
                port.to_string(),
            ];
            if cfg.ctx > 0 {
                a.push("--max-model-len".into());
                a.push(cfg.ctx.to_string());
            }
            (py, a)
        }
        "sglang" => {
            let py = python_exe().ok_or("找不到 python3")?;
            let mut a = vec![
                "-m".to_string(),
                "sglang.launch_server".to_string(),
                "--model-path".to_string(),
                model.to_string(),
                "--port".to_string(),
                port.to_string(),
            ];
            if cfg.ctx > 0 {
                a.push("--context-length".into());
                a.push(cfg.ctx.to_string());
            }
            (py, a)
        }
        other => return Err(format!("没有这个引擎：{other}")),
    };

    let pid = spawn_service(&exe, &args, &log_path(engine), &err_log_path(engine))?;
    save_run(
        engine,
        &RunRecord {
            pid,
            exe_name: exe
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            port,
            model: model.to_string(),
        },
    );
    crate::ulog::write(
        "localllm",
        &format!("{engine} 启动 pid={pid} port={port} model={model}"),
    );

    // 大模型加载要时间（几十 GB 权重读盘）。这里只等到端口开始服务为止，最多 90 秒；
    // 超时不算失败 —— 进程还在跑、日志在写，前端可以继续看。
    for _ in 0..180 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if port_alive(port) {
            return Ok(format!("http://127.0.0.1:{port}/v1"));
        }
    }
    Ok(format!("http://127.0.0.1:{port}/v1"))
}

/// 停掉我们启动的那个进程。没在跑也算成功（幂等）。
///
/// 🔴 **只按 PID 关，且关之前核对镜像名**。宪法那条「绝不按裸镜像名结束进程」的由来：
/// `taskkill /IM` 会把同名的别人一起端了（`claude.exe` 桌面版 vs CLI 那次）。
pub fn stop(engine: &str) -> Result<String, String> {
    let Some(r) = live_run(engine) else {
        clear_run(engine);
        return Ok("没有在跑的进程".to_string());
    };
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new(crate::installer::system_tool("taskkill"));
        c.args(["/PID", &r.pid.to_string(), "/T", "/F"]);
        hidden(&mut c);
        let _ = c.output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .arg(r.pid.to_string())
            .output();
    }
    clear_run(engine);
    crate::ulog::write("localllm", &format!("{engine} 已停 pid={}", r.pid));
    Ok(format!("已停止（pid {}）", r.pid))
}

// ============================================================
// llama.cpp 一键装
// ============================================================

/// GitHub 直链前面拼的国内加速前缀（与上面 Ollama 那套同源，代理会换会挂所以多列几个）。
const GH_MIRRORS: &[&str] = &[
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
    "", // 最后裸连（开了代理的机器走这条最稳）
];

/// 从 llama.cpp 的 release 资产名里挑一个。
///
/// 🔴 **Windows 默认挑 Vulkan 而不是 CUDA**，哪怕这台是 N 卡。理由是下载量：
/// 实测 b10488 —— vulkan 35 MB，cuda 147 MB **而且还必须再下 391 MB 的 cudart
/// 运行时**（不下就是「装好了、一启动缺 DLL」）。538 MB vs 35 MB，对我们这批
/// 慢网客户不是一个量级的事，而 Vulkan 在 N/A/I 三家卡上都能用。
/// 想要 CUDA 的最快速度就显式传 `variant="cuda"` —— 让代价被看见地选，
/// 而不是替所有人默认承担。
fn pick_llama_asset(names: &[String], gpu_vendor: &str, variant: Option<&str>) -> Option<String> {
    let want: Vec<&str> = if cfg!(target_os = "macos") {
        if std::env::consts::ARCH == "aarch64" {
            vec!["bin-macos-arm64"]
        } else {
            vec!["bin-macos-x64"]
        }
    } else {
        match variant {
            Some("cuda") => vec!["bin-win-cuda", "bin-win-vulkan", "bin-win-cpu"],
            Some("cpu") => vec!["bin-win-cpu"],
            Some("vulkan") => vec!["bin-win-vulkan", "bin-win-cpu"],
            // 没显式指定：有能加速的显卡走 Vulkan，纯核显/没显卡走 CPU 版
            _ if matches!(gpu_vendor, "nvidia" | "amd" | "intel") => {
                vec!["bin-win-vulkan", "bin-win-cpu"]
            }
            _ => vec!["bin-win-cpu"],
        }
    };
    for key in want {
        // 同一个 key 可能命中多个（cuda 有 12.4 / 13.3 两版），取名字排序最大的那个
        // = 版本号更新的那个。arm64 的包不能给 x64 机器，先滤掉。
        let mut hit: Vec<&String> = names
            .iter()
            .filter(|n| n.contains(key) && !n.contains("arm64") && !n.starts_with("cudart-"))
            .collect();
        hit.sort();
        if let Some(n) = hit.last() {
            return Some((*n).clone());
        }
    }
    None
}

/// 下一个文件，按镜像依次试。第一个下到 `min_bytes` 以上的算成功。
fn download_with_mirrors(
    gh_url: &str,
    dest: &std::path::Path,
    min_bytes: u64,
    label: &str,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    for (i, m) in GH_MIRRORS.iter().enumerate() {
        let url = format!("{m}{gh_url}");
        on_progress(&format!("{label}：下载源 {}/{}…", i + 1, GH_MIRRORS.len()));
        let _ = std::fs::remove_file(dest);
        let mut c = std::process::Command::new(crate::installer::system_tool("curl"));
        c.args([
            "-sSL",
            "--connect-timeout",
            "15",
            "--speed-time",
            "30",
            "--speed-limit",
            "30000",
            "-m",
            "1800",
            "-o",
            &dest.to_string_lossy(),
            &url,
        ]);
        hidden(&mut c);
        let ok = c.status().map(|s| s.success()).unwrap_or(false);
        let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        if ok && size >= min_bytes {
            on_progress(&format!("{label}：已下载 {} MB", size / 1_000_000));
            return Ok(());
        }
    }
    let _ = std::fs::remove_file(dest);
    Err(format!("{label} 下载失败（几个源都没通）"))
}

/// 解压到目标目录。Win10+ 自带的 `tar.exe` 是 bsdtar，zip 和 tar.gz 都吃得下 ——
/// 不为解压引第三方 crate（体积红线），也不用 PowerShell 的 Expand-Archive
/// （客户机上 PowerShell 被杀软掏空过，见 1.0.1 那条）。
fn extract_archive(archive: &std::path::Path, into: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| format!("建目录失败: {e}"))?;
    let mut c = std::process::Command::new(crate::installer::system_tool("tar"));
    c.arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .stdin(std::process::Stdio::null());
    hidden(&mut c);
    let out = c.output().map_err(|e| format!("起 tar 失败: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("解压失败：{}", err.trim().chars().take(200).collect::<String>()));
    }
    Ok(())
}

/// 在解压出来的目录里找 llama-server（有的包在根目录，有的多包一层 build/bin）。
fn find_in_tree(dir: &std::path::Path, name: &str, depth: usize) -> Option<PathBuf> {
    let p = dir.join(name);
    if p.exists() {
        return Some(p);
    }
    if depth == 0 {
        return None;
    }
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let sub = e.path();
        if sub.is_dir() {
            if let Some(f) = find_in_tree(&sub, name, depth - 1) {
                return Some(f);
            }
        }
    }
    None
}

/// 一键装 llama-server：查最新 release → 按这台机器挑构建 → 下载 → 解压到
/// `~/.uking/llama-server/bin/`。幂等：已经有了直接返回。
///
/// `variant`：`vulkan` / `cuda` / `cpu`，不传则按 `gpu_vendor` 自动挑（见 `pick_llama_asset`）。
///
/// 🔴 `gpu_vendor` 是**传进来的**，不是本模块自己去探的。挡在这儿的是模块独立四铁律
/// 第 2 条：显卡探测住在 `hardware.rs`（另一个功能模块），本模块一旦 import 它，
/// 「删掉本模块只动 lib.rs + 前端」就不再成立 —— 08-11 那次真去删模块时，
/// `hardware.rs` / `Feed.tsx` / `mcp.rs` 三处都是这么先漏进去、再删不掉的。
/// 组合根 `lib.rs` 知道两边，让它去问，这里只收一个字符串。
pub fn install_llama_server(
    variant: Option<&str>,
    gpu_vendor: &str,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, String> {
    if llama_server_exe().is_some() {
        return Ok("llama-server 已经装好了。".into());
    }
    crate::ulog::section("localllm", "开始安装 llama-server");
    let notify = on_progress;
    let on_progress = |m: &str| {
        crate::ulog::write("localllm", m);
        notify(m);
    };

    on_progress("正在查 llama.cpp 的最新版本…");
    // release 清单只能问 GitHub API —— 资产名里带构建号（llama-b10488-…），
    // 没有「latest/download/固定名」那种直链可用。
    let api = "https://api.github.com/repos/ggml-org/llama.cpp/releases/latest";
    let mut body = String::new();
    for m in GH_MIRRORS {
        let url = format!("{m}{api}");
        if let Ok(s) = crate::installer::curl(&["-sSL", "-m", "40", "-A", "U-King", &url]) {
            if s.contains("\"assets\"") {
                body = s;
                break;
            }
        }
    }
    if body.is_empty() {
        return Err("查不到 llama.cpp 的版本清单（GitHub 没通）。可以自己去 github.com/ggml-org/llama.cpp/releases 下 win-vulkan-x64 的包，解压到 ~/.uking/llama-server/bin/ 一样能用。".into());
    }
    let rel: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("版本清单解析失败: {e}"))?;
    let tag = rel["tag_name"].as_str().unwrap_or("latest").to_string();
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = assets
        .iter()
        .filter_map(|a| a["name"].as_str().map(|s| s.to_string()))
        .collect();
    let pick = pick_llama_asset(&names, gpu_vendor, variant)
        .ok_or("这个 release 里没有适合本机的构建")?;
    let url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(pick.as_str()))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or("找不到下载地址")?
        .to_string();

    let tmp_dir = std::env::temp_dir().join("uking-llamacpp");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("建临时目录失败: {e}"))?;
    let archive = tmp_dir.join(&pick);
    on_progress(&format!("选中构建 {pick}（{tag}）"));
    download_with_mirrors(&url, &archive, 5_000_000, "llama-server", &on_progress)?;

    on_progress("正在解压…");
    let unpack = tmp_dir.join("unpack");
    extract_archive(&archive, &unpack)?;

    let exe_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let found = find_in_tree(&unpack, exe_name, 3)
        .ok_or("解压出来的包里没有 llama-server（构建名可能变了）")?;
    // 🔴 整个目录一起搬，不是只搬那个 exe —— llama-server 旁边那堆 ggml*.dll 是它的
    //    运行时依赖，只搬 exe 的话装完一启动就是「缺 DLL」。
    let src_dir = found.parent().ok_or("路径异常")?.to_path_buf();
    let bin = uking_dir().join("llama-server").join("bin");
    let _ = std::fs::remove_dir_all(&bin);
    std::fs::create_dir_all(&bin).map_err(|e| format!("建目录失败: {e}"))?;
    let mut n = 0usize;
    for e in std::fs::read_dir(&src_dir).map_err(|e| format!("读解压目录失败: {e}"))?.flatten() {
        let p = e.path();
        if p.is_file() {
            let _ = std::fs::copy(&p, bin.join(e.file_name()));
            n += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    if llama_server_exe().is_none() {
        return Err("装完却找不到 llama-server，可能是解压被杀软拦了".into());
    }
    on_progress("llama-server 装好了");
    Ok(format!("已安装 llama-server（{tag} · {pick} · {n} 个文件）。接下来把 .gguf 模型放进模型目录就能启动。"))
}

// ============================================================
// 模型商店：货架 + 下载
// ============================================================
//
// 🔴 为什么货架只登记「仓库」、不登记「有哪些量化、每个多大」
//
// 量化清单和体积是**上游随时会变的事实**：仓库今天补一个 UD-Q4_K_XL、明天换个量化脚本
// 重传，写死在我们这儿的数字第二天就是错的 —— 而错的方向是「界面上写着 5.7 GB，
// 实际下了 6.9 GB」，客户的 C 盘为此爆掉，我们还一脸无辜。所以体积和量化清单一律
// **下载前现问魔搭**，货架只管「推荐哪些模型」这种半年才变一次的事。
//
// 货架本身也会过时（模型月月出新），所以它跟 optimize-advice.json 同一套路：
// 线上优先 + 内嵌兜底，改货架不必发 exe。

/// 货架清单。只用国内可达域（api/cloud 子域部分网络被 SNI reset，见 CLAUDE.md）。
const CATALOG_URLS: &[&str] = &["https://u-claw.org.cn/uking/localllm-catalog.json"];

/// 内嵌兜底（构建时打进 exe）。线上拉不到 / 离线时用这份。
const EMBEDDED_CATALOG: &str = include_str!("../resources/localllm-catalog.json");

/// 魔搭（ModelScope）：国内直连稳定，GGUF 仓库都是官方同步过去的。
/// HuggingFace 在裸网国内基本连不上，所以不作为主源。
const MODELSCOPE_API: &str = "https://modelscope.cn/api/v1/models";
const MODELSCOPE_FILE: &str = "https://modelscope.cn/models";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub params: String,
    /// tiny / small / medium / large —— 界面按这个分组
    #[serde(default)]
    pub tier: String,
    /// Q4 档的量级（GB）。**会漂**，界面上写「约」；准确值由 `catalog_files` 现取
    #[serde(default)]
    pub approx_gb: f64,
    /// 跑得动它大概需要多少内存（GB）
    #[serde(default)]
    pub min_ram_gb: f64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub blurb: String,
    /// 魔搭仓库 id，如 `unsloth/Qwen3.5-9B-GGUF`
    pub repo: String,
    #[serde(default)]
    pub engines: Vec<String>,
    /// 本地已经下到的量化（后端扫出来的，清单里不写）
    #[serde(default, skip_deserializing)]
    pub downloaded: Vec<String>,
}

#[derive(serde::Deserialize)]
struct CatalogFileJson {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    models: Vec<CatalogModel>,
}

/// 仓库名 → 模型文件名前缀（`unsloth/Qwen3.5-9B-GGUF` → `Qwen3.5-9B`）。
/// 拿它判断「本地是不是已经有这个模型」。
fn repo_base(repo: &str) -> String {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let lower = name.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix("-gguf") {
        name[..stripped.len()].to_string()
    } else {
        name.to_string()
    }
}

fn catalog_cache_path() -> PathBuf {
    localllm_dir().join("catalog.json")
}

/// 货架。线上优先（缓存 12 小时）→ 缓存 → 内嵌。**任何一步失败都不报错**：
/// 客户断网时该看到的是内嵌那份货架，不是一个红色的错误框。
pub fn catalog(refresh: bool) -> Vec<CatalogModel> {
    let embedded: CatalogFileJson =
        serde_json::from_str(EMBEDDED_CATALOG).unwrap_or(CatalogFileJson { version: 0, models: Vec::new() });

    let cache = catalog_cache_path();
    let cache_fresh = !refresh
        && std::fs::metadata(&cache)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|d| d.as_secs() < 12 * 3600).unwrap_or(false))
            .unwrap_or(false);

    let mut best = embedded;
    if cache_fresh {
        if let Some(c) = std::fs::read_to_string(&cache)
            .ok()
            .and_then(|s| serde_json::from_str::<CatalogFileJson>(&s).ok())
        {
            if c.version >= best.version && !c.models.is_empty() {
                best = c;
            }
        }
    } else {
        for url in CATALOG_URLS {
            let Ok(body) = crate::installer::curl(&["-sSL", "-m", "20", "-A", "U-King", url]) else {
                continue;
            };
            let Ok(c) = serde_json::from_str::<CatalogFileJson>(&body) else {
                continue;
            };
            if c.models.is_empty() || c.version < best.version {
                continue;
            }
            let _ = std::fs::create_dir_all(localllm_dir());
            let _ = std::fs::write(&cache, &body);
            best = c;
            break;
        }
    }

    // 本地已经有哪些量化 —— 用文件名前缀比对（GGUF 的命名就是 `<模型>-<量化>.gguf`）
    let local: Vec<String> = scan_gguf()
        .iter()
        .filter_map(|p| PathBuf::from(p).file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    let mut models = best.models;
    for m in &mut models {
        let base = repo_base(&m.repo).to_ascii_lowercase();
        m.downloaded = local
            .iter()
            .filter(|n| n.to_ascii_lowercase().starts_with(&format!("{base}-")))
            .cloned()
            .collect();
    }
    models
}

/// 商店里一个可下载的量化档。
#[derive(Debug, Clone, Serialize)]
pub struct QuantOption {
    /// 量化名，如 `Q4_K_M` / `UD-Q4_K_XL`
    pub quant: String,
    /// 仓库里的文件路径（分片模型会有多个，得一起下）
    pub files: Vec<String>,
    pub size_bytes: u64,
    /// 本地已经下全了
    pub local: bool,
}

/// 从文件名里剥出量化名：去掉 `-00001-of-00003` 分片后缀、去掉模型名前缀和 `.gguf`。
fn quant_of(file_name: &str, base: &str) -> (String, String) {
    let stem = file_name.trim_end_matches(".gguf").trim_end_matches(".GGUF");
    // 分片后缀 `-00001-of-00003`：手写扫描，不引正则 crate（体积红线）
    let group_stem = {
        let b = stem.as_bytes();
        let mut cut = stem.len();
        if b.len() > 15 {
            let tail = &stem[stem.len() - 15..];
            let t = tail.as_bytes();
            if t[0] == b'-'
                && t[6..10].eq_ignore_ascii_case(b"-of-")
                && t[1..6].iter().all(|c| c.is_ascii_digit())
                && t[10..].iter().all(|c| c.is_ascii_digit())
            {
                cut = stem.len() - 15;
            }
        }
        stem[..cut].to_string()
    };
    let q = if group_stem.to_ascii_lowercase().starts_with(&base.to_ascii_lowercase()) {
        group_stem[base.len()..].trim_start_matches('-').to_string()
    } else {
        group_stem.clone()
    };
    (if q.is_empty() { group_stem.clone() } else { q }, group_stem)
}

/// 问魔搭：这个仓库里有哪些量化、每个多大、本地下过没有。
///
/// 🔴 这一步**必须实时问**，不能读我们自己写的表 —— 见本节开头那段。
pub fn catalog_files(model_id: &str) -> Result<Vec<QuantOption>, String> {
    let entry = catalog(false)
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("货架上没有这个模型：{model_id}"))?;
    let url = format!("{MODELSCOPE_API}/{}/repo/files?Revision=master&Root=", entry.repo);
    let body = crate::installer::curl(&["-sSL", "-m", "30", "-A", "U-King", &url])
        .map_err(|e| format!("连不上魔搭（模型仓库）：{e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "魔搭返回的不是 JSON（可能被网络劫持了）".to_string())?;
    if v["Code"].as_i64().unwrap_or(0) != 200 {
        return Err(format!(
            "魔搭说这个仓库取不到：{}",
            v["Message"].as_str().unwrap_or("未知原因")
        ));
    }
    let base = repo_base(&entry.repo);
    let dir = download_dir();
    let mut groups: Vec<QuantOption> = Vec::new();
    for f in v["Data"]["Files"].as_array().cloned().unwrap_or_default() {
        let name = f["Name"].as_str().unwrap_or_default().to_string();
        let path = f["Path"].as_str().unwrap_or(&name).to_string();
        let size = f["Size"].as_u64().unwrap_or(0);
        if !name.to_ascii_lowercase().ends_with(".gguf") {
            continue;
        }
        // mmproj = 视觉塔、mtp = 投机解码头：都是**配件**，单独下没法聊天。
        // 摆在量化列表里，客户会以为那是「最小的一档」然后下一个跑不起来的 100MB 文件。
        let low = name.to_ascii_lowercase();
        if low.starts_with("mmproj") || low.starts_with("mtp-") {
            continue;
        }
        let (quant, _stem) = quant_of(&name, &base);
        match groups.iter_mut().find(|g| g.quant == quant) {
            Some(g) => {
                g.size_bytes += size;
                g.files.push(path);
            }
            None => groups.push(QuantOption { quant, files: vec![path], size_bytes: size, local: false }),
        }
    }
    for g in &mut groups {
        g.files.sort();
        g.local = g.files.iter().all(|p| {
            let n = p.rsplit('/').next().unwrap_or(p);
            dir.join(n).is_file()
        });
    }
    groups.sort_by_key(|g| g.size_bytes);
    if groups.is_empty() {
        return Err("这个仓库根目录下没有可直接下载的 GGUF（可能是分目录存放的超大模型）".into());
    }
    Ok(groups)
}

/// 模型下载落点。默认 `~/.uking/models`，客户可以改到别的盘
/// （17 GB 一个模型，C 盘放不下是常态）。
pub fn download_dir() -> PathBuf {
    if let Ok(s) = std::fs::read_to_string(localllm_dir().join("download-dir.txt")) {
        let p = PathBuf::from(s.trim());
        if p.is_dir() {
            return p;
        }
    }
    let d = uking_dir().join("models");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// 改下载落点。顺手把它加进扫描目录 —— 不然下完了「本地」列表里看不见。
pub fn set_download_dir(dir: &str) -> Result<String, String> {
    let p = PathBuf::from(dir.trim());
    if !p.is_dir() {
        return Err(format!("目录不存在：{dir}"));
    }
    let _ = std::fs::create_dir_all(localllm_dir());
    std::fs::write(localllm_dir().join("download-dir.txt"), p.to_string_lossy().as_bytes())
        .map_err(|e| format!("写下载位置失败: {e}"))?;
    let _ = add_model_dir(dir);
    Ok(p.to_string_lossy().to_string())
}

/// 正在下的那个 curl 的 pid + 取消位。
/// **进程内状态**，故意不落盘：U-King 一关，下载本来就断了，落盘只会造出一条
/// 「上次好像在下东西」的假记忆。断点续传靠磁盘上的半截文件（curl -C -），不靠这个。
static DOWNLOAD: std::sync::OnceLock<std::sync::Mutex<(bool, Option<u32>)>> = std::sync::OnceLock::new();

fn dl_state() -> &'static std::sync::Mutex<(bool, Option<u32>)> {
    DOWNLOAD.get_or_init(|| std::sync::Mutex::new((false, None)))
}

/// 取消正在跑的下载。已经下的那半截**留着**（下次续传接上），不删。
pub fn download_cancel() -> String {
    let pid = {
        let mut g = dl_state().lock().unwrap_or_else(|e| e.into_inner());
        g.0 = true;
        g.1
    };
    if let Some(pid) = pid {
        #[cfg(windows)]
        {
            let mut c = std::process::Command::new(crate::installer::system_tool("taskkill"));
            c.args(["/PID", &pid.to_string(), "/T", "/F"]);
            hidden(&mut c);
            let _ = c.output();
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
        }
        return "已取消（下了一半的文件留着，下次接着下）".into();
    }
    "当前没有在下载".into()
}

/// 下一个模型的某个量化档。分片模型会把所有分片下齐。
///
/// - **断点续传**：`curl -C -`，客户 20 GB 下到一半断网不用从头来
/// - **按字节校验**：下完比对魔搭给的 Size，对不上算失败并保留半截文件（下次续）——
///   放过一个短了 3 GB 的文件，症状会是「llama-server 起来就退出」，谁也查不到这儿
/// - 已经下齐的直接跳过（幂等）
pub fn download_model(
    model_id: &str,
    quant: &str,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, String> {
    let entry = catalog(false)
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("货架上没有这个模型：{model_id}"))?;
    let opts = catalog_files(model_id)?;
    let opt = opts
        .iter()
        .find(|o| o.quant.eq_ignore_ascii_case(quant))
        .ok_or_else(|| format!("这个模型没有 {quant} 这一档"))?;

    // 每个分片的真实大小（校验用）
    let sizes: Vec<u64> = {
        let url = format!("{MODELSCOPE_API}/{}/repo/files?Revision=master&Root=", entry.repo);
        let body = crate::installer::curl(&["-sSL", "-m", "30", "-A", "U-King", &url]).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        opt.files
            .iter()
            .map(|p| {
                v["Data"]["Files"]
                    .as_array()
                    .and_then(|a| {
                        a.iter()
                            .find(|f| f["Path"].as_str() == Some(p.as_str()))
                            .and_then(|f| f["Size"].as_u64())
                    })
                    .unwrap_or(0)
            })
            .collect()
    };

    let dir = download_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("建下载目录失败: {e}"))?;
    crate::ulog::section("localllm", &format!("下载模型 {} {}", entry.label, opt.quant));
    {
        let mut g = dl_state().lock().unwrap_or_else(|e| e.into_inner());
        g.0 = false;
        g.1 = None;
    }

    let total = opt.files.len();
    let mut first: Option<PathBuf> = None;
    for (i, path) in opt.files.iter().enumerate() {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let dest = dir.join(&name);
        let want = sizes.get(i).copied().unwrap_or(0);
        if first.is_none() {
            first = Some(dest.clone());
        }
        let have = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        if want > 0 && have == want {
            on_progress(&format!("{name} 已经在本地，跳过"));
            continue;
        }
        let url = format!("{MODELSCOPE_FILE}/{}/resolve/master/{}", entry.repo, path);
        let label = if total > 1 {
            format!("{name}（第 {}/{} 片）", i + 1, total)
        } else {
            name.clone()
        };
        on_progress(&format!("开始下载 {label}…"));

        let mut c = std::process::Command::new(crate::installer::system_tool("curl"));
        c.args([
            "-sSL",
            "-C",
            "-", // 断点续传
            "--connect-timeout",
            "20",
            "--retry",
            "3",
            "-A",
            "U-King",
            "-o",
            &dest.to_string_lossy(),
            &url,
        ])
        .stdin(std::process::Stdio::null());
        hidden(&mut c);
        let mut child = c.spawn().map_err(|e| format!("起 curl 失败: {e}"))?;
        {
            let mut g = dl_state().lock().unwrap_or_else(|e| e.into_inner());
            g.1 = Some(child.id());
        }
        let mut last = String::new();
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            if dl_state().lock().map(|g| g.0).unwrap_or(false) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("已取消下载（下了一半的留着，下次接着下）".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(800));
            let got = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            let msg = if want > 0 {
                format!(
                    "正在下载 {label} … {} / {} MB（{}%）",
                    got / 1_000_000,
                    want / 1_000_000,
                    (got.saturating_mul(100) / want.max(1)).min(99)
                )
            } else {
                format!("正在下载 {label} … {} MB", got / 1_000_000)
            };
            if msg != last {
                on_progress(&msg);
                last = msg;
            }
        }
        {
            let mut g = dl_state().lock().unwrap_or_else(|e| e.into_inner());
            g.1 = None;
        }
        let got = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        if want > 0 && got != want {
            return Err(format!(
                "{name} 没下完（{} / {} MB）。网断了或者盘满了；再点一次会接着下，不用从头来。",
                got / 1_000_000,
                want / 1_000_000
            ));
        }
        if got == 0 {
            return Err(format!("{name} 一个字节都没下到（魔搭没通）"));
        }
    }

    let p = first.ok_or("没有要下的文件")?;
    crate::ulog::write("localllm", &format!("下载完成 {}", p.display()));
    Ok(p.to_string_lossy().to_string())
}

/// 本地已有的一个模型文件。
#[derive(Debug, Clone, Serialize)]
pub struct LocalModel {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    /// 哪个引擎吃得下：llamacpp（.gguf）/ vllm|sglang（HF 目录）
    pub engine: String,
}

/// 「本地」页签：磁盘上真有的模型，带体积。
///
/// （跟上面那个私有的 `local_models()` 不是一回事：那个是问 Ollama 要它自己仓库里的
/// 模型 id，这个是扫我们的模型目录看磁盘上真躺着什么文件。）
pub fn local_model_files() -> Vec<LocalModel> {
    let mut out: Vec<LocalModel> = Vec::new();
    for p in scan_gguf() {
        let pb = PathBuf::from(&p);
        out.push(LocalModel {
            name: pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            size_bytes: std::fs::metadata(&pb).map(|m| m.len()).unwrap_or(0),
            path: p,
            engine: "llamacpp".into(),
        });
    }
    for p in scan_hf() {
        let pb = PathBuf::from(&p);
        out.push(LocalModel {
            name: pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            size_bytes: 0, // 目录体积要递归算，值不大 —— 别为一个数字去遍历几十 GB
            path: p,
            engine: "vllm".into(),
        });
    }
    out
}

/// 日志尾巴（前端「看日志」）。模型加载慢/失败时，客户唯一能看的就是这个。
///
/// stdout 和 stderr 是两个文件（原因见 err_log_path），这里合起来给 —— 客户不该被要求
/// 知道哪半边写在哪个文件里。模型加载的关键信息大多在 stderr，所以它排在后面（更靠近尾巴）。
pub fn logs(engine: &str, lines: usize) -> String {
    let mut s = std::fs::read_to_string(log_path(engine)).unwrap_or_default();
    let e = std::fs::read_to_string(err_log_path(engine)).unwrap_or_default();
    if !e.trim().is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&e);
    }
    let all: Vec<&str> = s.lines().collect();
    let start = all.len().saturating_sub(lines.max(1));
    all[start..].join("\n")
}
