//! Token 压缩机 —— 集成 RTK (Rust Token Killer) 给客户的 AI 编程省 token。
//!
//! RTK 是开源 Rust 单二进制 CLI(Apache-2.0, github.com/rtk-ai/rtk):拦截 AI 编程时跑的命令
//! (git/cargo/test/lint/ls/grep…),把啰嗦输出压缩后再喂给大模型,实测省 40~90% token。
//! **不降智**:默认档只砍重复日志/噪音/通过的测试列表,报错/失败/diff/告警一个不丢
//! (真正有损的"只留函数签名"是 rtk 的 opt-in 激进档,本模块默认不开)。
//!
//! 本模块只负责:装 rtk.exe、开/关 Claude Code 的 hook、查状态、读 rtk gain 战绩。纯函数,
//! `#[tauri::command]` 在 lib.rs 转调,进度用 `|msg|` 回调传出。支持 `UKING_TEST_HOME` 沙箱。
//!
//! ## 关键设计取舍(2026-07-22 实机踩坑后定)
//! - **Claude hook 我们自己 JSON 合并/删键写 `~/.claude/settings.json`**(与 U-King 管 `env` 块同一套),
//!   **绝不调 `rtk init` / `rtk init --uninstall`**——实测它卸载会留一个空的 `"hooks":{"PreToolUse":[]}`
//!   壳、还会用当前(带 hook 的)内容**覆盖用户的 .bak**。我们自己管:删时若数组空就连 key 一起删,
//!   首改前备份 `settings.json.uking-bak`(不覆盖已存在的备份),规避这两个坑。
//! - hook 命令走 **我们自己的 exe 当包装器**(`"<绝对路径>/U-King.exe" rtk-hook`),
//!   由它调 rtk 并把改写结果里的裸 `rtk …` 换成绝对路径 —— 见下面「为什么要包装一层」。
//! - v1 只接管 **Claude Code**(强制 hook、省得最实、最好卸干净)。Codex(awareness 文档、省得少)、
//!   Hermes(插件)留作后续,不在本模块耦合。

use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// rtk 版本 + 下载源。三平台产物已镜像到阿里云 OSS 深圳(国内直连快、不被墙),GitHub 直链作兜底。
/// 换 rtk 版本时:先把新产物 `ossutil cp` 传到 `uking/runtimes/rtk/` 再改这里(对齐 Node/ollama 镜像做法)。
const RTK_VERSION: &str = "0.43.0";
const RTK_OSS_BASE: &str = "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/runtimes/rtk";
const RTK_GH_BASE: &str = "https://github.com/rtk-ai/rtk/releases/download/v0.43.0";

// ── 路径(支持 UKING_TEST_HOME 沙箱,与 providers.rs::config_home 同语义)──────────────

fn config_home() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
}

fn rtk_dir() -> PathBuf {
    config_home().join(".uking").join("tools").join("rtk")
}

fn rtk_exe() -> PathBuf {
    rtk_dir().join(if cfg!(windows) { "rtk.exe" } else { "rtk" })
}

fn claude_settings_path() -> PathBuf {
    config_home().join(".claude").join("settings.json")
}

/// U-King 自己的 exe（hook 包装器要靠它）。优先「装到本地」那份，路径最稳；
/// 没装过（U 盘 / 绿色版直接跑）才用当前 exe。
fn uking_exe() -> PathBuf {
    let local = config_home()
        .join(".uking")
        .join(if cfg!(windows) { "U-King.exe" } else { "U-King" });
    if local.exists() {
        return local;
    }
    std::env::current_exe().unwrap_or(local)
}

/// 命令行里安全引用一个绝对路径。
///
/// **一律转正斜杠**：真正执行被改写命令的是 Claude Code 的 Bash 工具，
/// 它在 Windows 上是 Git Bash —— 双引号里的反斜杠在 bash 里语义含混，
/// 而 `C:/Users/…/rtk.exe` 两个平台都认。
fn quoted(p: &std::path::Path) -> String {
    format!("\"{}\"", p.display().to_string().replace('\\', "/"))
}

/// hook 里写的命令：`"<绝对路径>/U-King.exe" rtk-hook`。
///
/// ## 为什么要包装一层（2026-08-10，客户机 macOS 0.9.94 实锤）
///
/// 老写法是 `"<绝对路径>/rtk" hook claude` —— 调 rtk 这一步确实不依赖 PATH，
/// **但 rtk 吐出来的 `updatedInput.command` 是裸名 `rtk ls -la`**，
/// 而那一条才是真正要被执行的。裸名在 PATH 上找不到 → 退出码 127 →
/// 客户机上 **每一条 Bash 命令都失败**，ls / git / grep 全废。
///
/// 老实现试图靠「把 shim 目录塞进用户 PATH」补救，但 `prepend_user_path` 是
/// PowerShell 实现 = Windows 专属，`#[cfg(not(windows))]` 分支压根没调它 ——
/// 于是 Mac 上开一次开关 = 那台机器的 AI 当场变废，而 status 只会说
/// 「点一次开关即可自动修复」，点了还是修不好。
///
/// 现在改成我们自己接管这一步：PATH 彻底出局，跨平台一份实现，且**失败时吐空**
/// （= 按原命令跑）。失败模式从 fail-closed 变成 fail-open —— 最坏只是没省到。
fn hook_command() -> String {
    format!("{} rtk-hook", quoted(&uking_exe()))
}

/// settings.json 里那条 hook 是哪种写法。
#[derive(PartialEq, Clone, Copy, Debug)]
enum HookKind {
    /// 没挂。**故意不叫 `None`** —— 跟 `Option::None` 在模式位置太容易看混。
    Absent,
    /// 老写法：直接调 `rtk hook claude`，改写出来的裸 `rtk …` 要靠 PATH
    Legacy,
    /// 新写法：走 U-King 包装器，不依赖 PATH
    Wrapper,
}

/// 判断某条 hook command 是不是我们(或 rtk)装的。
///
/// 两种写法都要认：`rtk-hook` 是新包装器，`hook claude` 是老写法 ——
/// 认不出老的，`remove_hook` 就摘不干净、自愈也无从谈起。
fn is_our_hook(cmd: &str) -> bool {
    hook_kind_of(cmd) != HookKind::Absent
}

fn hook_kind_of(cmd: &str) -> HookKind {
    // 老写法先认：`hook claude` 是 rtk 的专属子命令，够特异。
    if cmd.contains("hook claude") {
        return HookKind::Legacy;
    }
    // 包装器：`rtk-hook` 必须是**独立的一段参数**，不能只是「路径里恰好含这几个字」。
    // 🔴 第一版写的是 `cmd.contains("rtk-hook")`，被沙箱目录 `uking-test-rtk-hook-heal`
    // 当场骗过去 —— 老式 hook 被认成新式，于是自愈直接跳过、客户机继续废着。
    // 客户机上同样可能踩到（任何含 `rtk-hook` 的目录名）。
    if cmd
        .split_whitespace()
        .any(|t| t.trim_matches('"') == "rtk-hook")
    {
        return HookKind::Wrapper;
    }
    HookKind::Absent
}

// ── 原子写(同 providers.rs::atomic_write)──────────────────────────────────────────

fn atomic_write(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("cfg");
    let tmp = path.with_file_name(format!(".{fname}.uking-tmp.{pid}.{stamp}"));
    std::fs::write(&tmp, data).map_err(|e| format!("写临时文件失败: {e}"))?;
    #[cfg(windows)]
    {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("原子替换失败: {e}")
    })
}

// ── 跑 rtk.exe(注入 CREATE_NO_WINDOW 防闪黑窗)──────────────────────────────────────

fn run_rtk(args: &[&str]) -> Result<String, String> {
    let exe = rtk_exe();
    if !exe.exists() {
        return Err("rtk 未安装".into());
    }
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("启动 rtk 失败: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// ── hook 包装器(`u-king-mini.exe rtk-hook`)─────────────────────────────────────────

/// 把 rtk 的 hook 输出改写成**不依赖 PATH**的形式。
///
/// 输入是 rtk 的 stdout 原文，输出是该打给 Claude Code 的 stdout（`None` = 什么都别打）。
/// rtk 的实际形状（0.43.0 实测）：
/// ```json
/// {"hookSpecificOutput":{"hookEventName":"PreToolUse",
///   "updatedInput":{"command":"rtk ls -la /tmp","description":"list"},
///   "permissionDecision":"allow"}}
/// ```
/// 不需要压缩的命令（`echo hi`）rtk **输出为空** —— 空 = 放行原命令，
/// 这正是我们所有 fail-open 分支借用的那条语义。
fn rewrite_hook_output(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None; // rtk 说这条不用改
    }
    // 解析不了就别吐 —— 吐一份我们没把握的 JSON 比不吐危险得多。
    let mut v: Value = serde_json::from_str(t).ok()?;
    let slot = v
        .get_mut("hookSpecificOutput")
        .and_then(|h| h.get_mut("updatedInput"))
        .and_then(|u| u.get_mut("command"));
    match slot {
        Some(slot) => {
            let cmd = slot.as_str()?.to_string();
            // 只认裸 `rtk ` 前缀。别的形状原样透传 —— 万一 rtk 将来自己改成绝对路径了，
            // 我们不该再插一手把它改坏。
            if let Some(rest) = cmd.strip_prefix("rtk ") {
                *slot = Value::String(format!("{} {}", quoted(&rtk_exe()), rest));
            }
            Some(v.to_string())
        }
        // 形状认不出（rtk 换了协议 / 输出的是别的决定）：原样透传，别自作主张。
        None => Some(v.to_string()),
    }
}

/// 把 hook 输入喂给 rtk，拿它的 stdout。超时/失败一律 `None`（= 放行原命令）。
fn rtk_hook_stdout(input: &str, timeout_ms: u64) -> Option<String> {
    use std::io::Write;
    let exe = rtk_exe();
    if !exe.exists() {
        return None;
    }
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["hook", "claude"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        // rtk 会往 stderr 打「No hook installed」之类的招呼语。我们换了 hook 写法之后
        // 它认不出自己被装上了，这行会**每条命令都打一遍** —— 直接丢掉。
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(input.as_bytes());
        // drop(si) 关掉管道，否则 rtk 等 EOF、我们等它退出 —— 死锁
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => return None,
        }
        if std::time::Instant::now() >= deadline {
            // 卡住了就放行。这个 hook 挂在**每一条** Bash 命令上，
            // 宁可不省，也绝不让它把客户的 AI 拖住。
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// `u-king-mini.exe rtk-hook` 的实现：读 stdin → 交给 rtk → 改写成绝对路径 → 出 stdout。
///
/// **任何一步出问题都吐空**（Claude Code 收到空输出 = 按原命令跑）。这是本模块最重要的
/// 一条不变量：老实现是 fail-closed（PATH 上没有 `rtk` → 每条命令退出码 127 → 客户的 AI
/// 整台变废，而他只会觉得「这 AI 怎么突然变傻了」），换成 fail-open 之后最坏只是这一轮没省到。
pub fn run_hook_wrapper() -> i32 {
    use std::io::Read;
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return 0;
    }
    if let Some(raw) = rtk_hook_stdout(&input, 5_000) {
        if let Some(out) = rewrite_hook_output(&raw) {
            print!("{out}");
        }
    }
    0
}

// ── 状态 ──────────────────────────────────────────────────────────────────────────

/// 一条命令的压缩战绩（`rtk gain` 文本表的 By Command 段）。
///
/// 为什么非要它不可：光给一个「平均省 27%」客户没法判断值不值。分项一摊开就一目了然
/// —— cargo test 省 98%、grep 只省 19%，而 grep 跑得最多。**这才是能拿去宣传的东西，
/// 也是客户自己能验证的东西**。
#[derive(Serialize, Clone)]
pub struct GainCmd {
    pub command: String,
    pub count: u64,
    pub saved: u64,
    pub pct: f64,
}

/// 一天的压缩战绩（`rtk gain --daily --format json`）。给前端画趋势条。
#[derive(Serialize, Clone)]
pub struct GainDay {
    pub date: String,
    pub commands: u64,
    /// 压缩前 token（喂给大模型之前的原始量）
    pub before: u64,
    /// 压缩后 token（真正进了上下文的量）
    pub after: u64,
    pub saved: u64,
    pub pct: f64,
}

#[derive(Serialize)]
pub struct RtkStatus {
    /// rtk.exe 是否已下载
    pub installed: bool,
    /// Claude settings.json 里是否挂着我们的 hook(= 已开启)
    pub enabled: bool,
    pub version: Option<String>,
    /// rtk gain 累计:省下的 token、平均压缩率(%)、已优化命令数
    pub saved_tokens: Option<u64>,
    pub saved_pct: Option<f64>,
    pub commands: Option<u64>,
    /// **压缩前**的原始 token 总量。有前后两个绝对值，「省了多少」才是可核对的，
    /// 而不是一个孤零零的百分比。
    pub before_tokens: Option<u64>,
    /// **压缩后**真正进入大模型上下文的 token 总量。
    pub after_tokens: Option<u64>,
    /// 按省下的量排序的命令排行（最多 8 条）。空 = 没数据或没解析出来。
    pub top_commands: Vec<GainCmd>,
    /// 每日战绩（最多留最近 14 天）。空 = 没数据。
    pub daily: Vec<GainDay>,
    /// **端到端真的能用吗**。装了 + 开了 ≠ 能用：hook 会把命令改写成裸 `rtk ...`，
    /// 那条命令要在 PATH 上解析得到才跑得起来。客户「开了两天一点没省」就是栽在这。
    pub ready: bool,
    /// ready=false 时说人话：到底卡在哪。空 = 没有已知阻塞。
    pub blockers: Vec<String>,
}

/// `rtk` 这个**裸命令**在 PATH 上解析得到吗？
///
/// 为什么非查不可：hook 自己是用绝对路径调的（所以 hook 一定跑得起来），但它吐出的
/// `updatedInput.command` 是 `rtk ls -la` 这种裸名 —— 真正要被执行的是那一条。
/// 「调 hook 不依赖 PATH」和「hook 改写出来的命令不依赖 PATH」是两件事，
/// 老注释只说对了前一半。
fn rtk_on_path() -> bool {
    let Ok(path) = std::env::var("PATH") else { return false };
    // **Windows 上只认无扩展名的 `rtk`。**
    //
    // 改写后的命令是被 Claude Code 的 shell 执行的，而它在 Windows 上走 **Git Bash**。
    // Git Bash 没有 PATHEXT 那套：裸 `rtk` **不会**匹配到 `rtk.cmd` / `rtk.bat`，
    // 只会找文件名恰好是 `rtk` 的可执行文件。
    //
    // 老版本这里把 rtk.cmd/.bat 也算「在 PATH 上」，于是 status 报 ready=true、
    // 而客户机上每条被改写的命令都 `rtk: command not found`（退出码 127）——
    // ls / find / grep / git status 全废，还看不出为什么。
    // 检测标准必须和真正执行它的那个 shell 一致，否则就是给自己发假绿灯。
    //
    // rtk.exe 仍然算数：Git Bash 认 .exe。
    let names: &[&str] = if cfg!(windows) { &["rtk", "rtk.exe"] } else { &["rtk"] };
    std::env::split_paths(&path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// 在 `~/.uking/shims` 放一个转发到真实 rtk 的小 `.cmd`，并把该目录置顶进用户 PATH。
///
/// 复用 installer 的 shims 约定和 PATH 助手（宪法第 12 条：公共能力复用不复制）。
/// 装完和每次开启时都调一遍 —— 已经装过的老客户不会为这个再点一次「安装」，
/// 自愈必须挂在他们真的会点的那个开关上。
pub fn ensure_on_path() -> Result<(), String> {
    if !rtk_exe().exists() {
        return Err("rtk 尚未安装".into());
    }
    let shims = config_home().join(".uking").join("shims");
    std::fs::create_dir_all(&shims).map_err(|e| format!("创建转发目录失败: {e}"))?;
    #[cfg(windows)]
    {
        // 两个 shim 都要写，缺一不可 —— 它们服务的是两个不同的 shell：
        //
        //   rtk.cmd  给 cmd.exe / PowerShell（用户自己在终端里敲 rtk）
        //   rtk      给 **Git Bash**，也就是 Claude Code 在 Windows 上真正执行
        //            被改写命令的那个 shell。Git Bash 没有 PATHEXT，裸 `rtk`
        //            不会匹配 rtk.cmd，只认文件名恰好是 `rtk` 的可执行文件。
        //
        // 只写 .cmd 是老版本的 bug：hook 把命令改写成裸 `rtk ls -la` 之后，
        // 客户机上每一条都 `rtk: command not found`（退出码 127），
        // ls / find / grep / git status 全部不可用，而 status 还报 ready=true。
        let cmd_shim = shims.join("rtk.cmd");
        let cmd_content = format!(
            "@echo off
rem U-King CLI command guard
\"{}\" %*
",
            rtk_exe().display()
        );
        std::fs::write(&cmd_shim, cmd_content)
            .map_err(|e| format!("写入 {} 失败: {e}", cmd_shim.display()))?;

        // Git Bash 用：带 shebang 的无扩展名转发脚本。
        // 路径转成正斜杠，免得 bash 把反斜杠当转义符。
        let sh_shim = shims.join("rtk");
        let sh_content = format!(
            "#!/usr/bin/env bash
# U-King CLI command guard (Git Bash / MSYS)
# Claude Code 在 Windows 上用 Git Bash 执行 hook 改写后的命令，
# 而 Git Bash 不认 rtk.cmd —— 这个无扩展名脚本才是那条路径上真正被执行的。
exec \"{}\" \"$@\"
",
            rtk_exe().display().to_string().replace('\\', "/")
        );
        std::fs::write(&sh_shim, sh_content)
            .map_err(|e| format!("写入 {} 失败: {e}", sh_shim.display()))?;

        crate::installer::prepend_user_path(&shims)?;
    }
    #[cfg(not(windows))]
    {
        let shim = shims.join("rtk");
        let _ = std::fs::remove_file(&shim);
        std::os::unix::fs::symlink(rtk_exe(), &shim)
            .map_err(|e| format!("建立 {} 链接失败: {e}", shim.display()))?;
    }
    // 本进程也立刻生效，免得「开了要重启 U-King 才看得到 ready」。
    crate::installer::prepend_process_path(&[shims]);
    Ok(())
}

/// 给无头跑道用：rtk 程序的绝对路径（没装则 `None`）。
pub fn probe_exe_path() -> Option<String> {
    let p = rtk_exe();
    p.exists().then(|| p.display().to_string())
}

/// 给无头跑道用：造一条 PreToolUse 探针报文（与闸门用的是同一份，不另写第二份）。
pub fn probe_input_json(command: &str) -> String {
    probe_input(command)
}

/// 造一条 Claude Code PreToolUse 的输入报文（给闸门和无头跑道当探针）。
fn probe_input(command: &str) -> String {
    json!({
        "session_id": "uking-probe",
        "cwd": ".",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command, "description": "uking probe" }
    })
    .to_string()
}

/// 真跑一遍改写链路，确认吐出来的命令**不依赖 PATH 也能执行**。
///
/// 这是「开关能不能开」的唯一判据。断言两条：
///   ① rtk 确实把命令改写了（形状没变、我们的前缀替换生效）
///   ② 替换进去的那个程序**在磁盘上真的存在** —— 只看字符串对不对是自欺欺人
///
/// 刻意**在进程内**跑（直接调 `rtk_hook_stdout` + `rewrite_hook_output`），而不是
/// spawn 自己：`cargo test` 里 `current_exe()` 是测试二进制，spawn 自己会假红。
/// 「真的 spawn 一次自己」那半边由 `--rtk-hook-test` 无头跑道盖住。
fn verify_pipeline() -> Result<(), String> {
    if !rtk_exe().exists() {
        return Err("Token 压缩机还没安装，先点「安装」".into());
    }
    let raw = rtk_hook_stdout(&probe_input("git status"), 8_000)
        .ok_or("压缩机自检没通过：rtk 跑不起来或没有响应，已阻止开启（开了会让所有命令失败）")?;
    let out = rewrite_hook_output(&raw)
        .ok_or("压缩机自检没通过：rtk 的输出看不懂，已阻止开启（开了会让所有命令失败）")?;
    let v: Value = serde_json::from_str(&out)
        .map_err(|e| format!("压缩机自检没通过：改写结果不是合法 JSON（{e}），已阻止开启"))?;
    let cmd = v
        .pointer("/hookSpecificOutput/updatedInput/command")
        .and_then(|c| c.as_str())
        .ok_or("压缩机自检没通过：改写结果里没有命令，已阻止开启")?;
    let want = quoted(&rtk_exe());
    if !cmd.starts_with(&want) {
        return Err(format!(
            "压缩机自检没通过：改写出来的命令是 `{cmd}`，没换成绝对路径 —— 这台机器上开了会让所有命令报 `rtk: command not found`，已阻止开启"
        ));
    }
    if !rtk_exe().exists() {
        return Err("压缩机自检没通过：rtk 程序文件不在了，已阻止开启".into());
    }
    Ok(())
}

/// 启动自愈：把老写法的 hook 升级成包装器；升不了就摘掉。
///
/// 为什么必须在启动时做，而不是等客户去点开关：**中招的人不会想到来关它**。
/// 他看到的现象是「Claude Code 突然什么命令都跑不了」，最可能的反应是卸载 Claude Code、
/// 重装、或者换个 AI —— 没有任何线索指向 U-King 的一个默认关着的省钱开关。
///
/// 返回 `Some(说明)` 表示动过手（给 ulog 和无头跑道看），`None` = 什么都没做。
pub fn heal_legacy_hook() -> Option<String> {
    if installed_hook_kind() != HookKind::Legacy {
        return None;
    }
    let path = claude_settings_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let mut root: Value = serde_json::from_str(&text).ok()?;
    let obj = root.as_object_mut()?;

    // 能升级就升级（客户本来就想要压缩，别默默把功能拿走）；rtk 都不在了才摘。
    let upgraded = verify_pipeline().is_ok();
    remove_hook(obj);
    if upgraded {
        add_hook(obj);
    }
    let pretty = serde_json::to_string_pretty(&root).ok()?;
    atomic_write(&path, pretty.as_bytes()).ok()?;

    let msg = if upgraded {
        "把旧版 Token 压缩机 hook 升级成绝对路径写法（旧写法会让所有命令报 rtk: command not found）"
    } else {
        "摘掉旧版 Token 压缩机 hook —— 自检没过，留着会让所有命令失败"
    };
    crate::ulog::write("rtk", msg);
    Some(msg.to_string())
}

/// 挂着的这条 hook **真的在干活吗**（不含「rtk 装没装」那一问）。
///
/// 新写法自带绝对路径，PATH 与它无关；老写法必须 rtk 在 PATH 上，否则不但不省，
/// 还会把每一条命令打成退出码 127。
fn hook_is_effective(kind: HookKind, on_path: bool) -> bool {
    match kind {
        HookKind::Absent => false,
        HookKind::Wrapper => true,
        HookKind::Legacy => on_path,
    }
}

/// 压缩机**此刻真的在省吗** —— 装了 + 挂上了 hook + 那条 hook 真的有效。
///
/// 和 `status()` 的区别：这里**一个子进程都不起**（status 要跑 `rtk --version` / `rtk gain`，
/// 实测 ~500ms）。给「用量分析」这种每次打开页面都要算一遍的地方用，别让一句判断拖慢整页。
pub fn is_active() -> bool {
    rtk_exe().exists() && claude_hook_present() && rtk_on_path()
}

pub fn status() -> RtkStatus {
    let installed = rtk_exe().exists();
    let version = if installed {
        run_rtk(&["--version"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    let kind = installed_hook_kind();
    let enabled = kind != HookKind::Absent;
    let gain = if installed { read_gain() } else { None };
    let on_path = rtk_on_path();
    let mut blockers = Vec::new();
    if !installed {
        blockers.push("还没安装 Token 压缩机".into());
    } else {
        if !enabled {
            blockers.push("开关是关的（~/.claude/settings.json 里没有我们的 hook）".into());
        }
        // **只有老写法才怕 PATH**。新写法（U-King 包装器）自己把裸 `rtk` 换成绝对路径，
        // rtk 在不在 PATH 上跟能不能省一点关系都没有 —— 这里再报 blocker 就是发假警报。
        if kind == HookKind::Legacy && !on_path {
            blockers.push(
                "旧版 hook + rtk 不在 PATH：被改写的命令会全部报 `rtk: command not found`。点一次开关即可换成新写法".into(),
            );
        }
    }
    RtkStatus {
        installed,
        enabled,
        version,
        saved_tokens: gain.as_ref().map(|g| g.saved),
        saved_pct: gain.as_ref().map(|g| g.pct),
        commands: gain.as_ref().map(|g| g.commands),
        before_tokens: gain.as_ref().map(|g| g.before),
        after_tokens: gain.as_ref().map(|g| g.after),
        top_commands: gain.as_ref().map(|g| g.top.clone()).unwrap_or_default(),
        daily: gain.as_ref().map(|g| g.daily.clone()).unwrap_or_default(),
        ready: installed && hook_is_effective(kind, on_path),
        blockers,
    }
}

/// settings.json 的 PreToolUse 里挂着的是**哪一种**我们的 hook。
///
/// 分新老两种，不是为了好看：老写法（`rtk hook claude`）改写出来的裸命令要靠 PATH，
/// 在没有 PATH 的机器上会让**每条命令**都失败；新写法（包装器）不会。
/// 只回一个 bool 就没法区分「开着且是好的」和「开着且正在毁掉这台机器」。
fn installed_hook_kind() -> HookKind {
    let Ok(s) = std::fs::read_to_string(claude_settings_path()) else {
        return HookKind::Absent;
    };
    let Ok(root) = serde_json::from_str::<Value>(&s) else {
        return HookKind::Absent;
    };
    let Some(arr) = root
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|p| p.as_array())
    else {
        return HookKind::Absent;
    };
    let mut found = HookKind::Absent;
    for entry in arr {
        let Some(hs) = entry.get("hooks").and_then(|h| h.as_array()) else {
            continue;
        };
        for h in hs {
            let Some(cmd) = h.get("command").and_then(|c| c.as_str()) else {
                continue;
            };
            match hook_kind_of(cmd) {
                // 老写法优先报出来：一台机器上同时有两条时，坏的那条才是要治的。
                HookKind::Legacy => return HookKind::Legacy,
                HookKind::Wrapper => found = HookKind::Wrapper,
                HookKind::Absent => {}
            }
        }
    }
    found
}

/// settings.json 的 PreToolUse 里有没有我们的 hook。
fn claude_hook_present() -> bool {
    installed_hook_kind() != HookKind::Absent
}

// ── 安装 rtk.exe ──────────────────────────────────────────────────────────────────

pub fn install(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let asset = rtk_asset().ok_or("Token 压缩机暂不支持当前系统(仅 Windows / macOS)")?;
    if rtk_exe().exists() {
        on_progress("rtk 已安装,跳过下载");
        return Ok(format!("rtk {RTK_VERSION} 已就绪"));
    }
    let dir = rtk_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let pkg = dir.join(asset);

    on_progress("正在下载 Token 压缩机(rtk ~4MB)…");
    download_rtk(asset, &pkg, on_progress)?;

    on_progress("正在解压…");
    extract_rtk(&pkg, &dir)?;
    let _ = std::fs::remove_file(&pkg);
    if !rtk_exe().exists() {
        return Err("解压后没找到 rtk 可执行文件,请重试".into());
    }
    // macOS/Unix:补可执行位(tar 一般已保留,保险起见)。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(rtk_exe(), std::fs::Permissions::from_mode(0o755));
    }
    // 装完立刻把 rtk 挂上 PATH：hook 改写出来的裸 `rtk` 命令要靠它才跑得起来。
    if let Err(e) = ensure_on_path() {
        on_progress(&format!("提示：把 rtk 加进 PATH 失败（{e}），压缩可能不生效"));
    }
    let ver = run_rtk(&["--version"]).unwrap_or_default();
    on_progress("安装完成");
    Ok(format!("已安装 {}", ver.trim()))
}

/// 本平台的 rtk 产物名(Windows zip / macOS tar.gz,按 CPU 架构)。None = 不支持的平台。
/// 用 `cfg!()`(编译期布尔)分支:universal2 的每个 slice 各自编译,`target_arch` 反映当前运行架构。
fn rtk_asset() -> Option<&'static str> {
    if cfg!(windows) {
        Some("rtk-x86_64-pc-windows-msvc.zip")
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Some("rtk-aarch64-apple-darwin.tar.gz")
        } else {
            Some("rtk-x86_64-apple-darwin.tar.gz")
        }
    } else {
        None
    }
}

/// 下载够大算成功(rtk 各平台压缩包都 >3MB)。
fn file_ok(p: &std::path::Path) -> bool {
    std::fs::metadata(p).map(|m| m.len() > 100_000).unwrap_or(false)
}

/// 把 URL 下到文件。每个源两招：curl 直连(有直连网时最快) → Windows PowerShell 写文件
/// (走 WinINET 系统/自动代理,救「只有代理、没直连网」的机器)。OSS 深圳镜像为主,GitHub 兜底。
///
/// **绝不用 `installer::curl` 的 dotnet 兜底下二进制**——那个兜底忽略 `-o`、把响应读成 UTF-8
/// 字符串返回,二进制根本不落盘(实测就是「无直连网机器 rtk 装不上」的真因:OSS 能下但文件是空的)。
fn download_rtk(
    asset: &str,
    out: &std::path::Path,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let sources = [
        (format!("{RTK_OSS_BASE}/{asset}"), "镜像"),
        (format!("{RTK_GH_BASE}/{asset}"), "备用源"),
    ];
    for (i, (url, label)) in sources.iter().enumerate() {
        if i > 0 {
            on_progress(&format!("换{label}重试…"));
        }
        // 1) curl 直连（--proxy "" 强制直连；有直连网的客户机最快，OSS 深圳国内直连不被墙）
        let _ = std::fs::remove_file(out);
        if curl_to_file(url, out) && file_ok(out) {
            return Ok(());
        }
        // 2) Windows：PowerShell 写文件（走系统/自动代理，救挂了代理但没直连网的机器）
        #[cfg(windows)]
        {
            let _ = std::fs::remove_file(out);
            if ps_download_to_file(url, out) && file_ok(out) {
                return Ok(());
            }
        }
    }
    let _ = std::fs::remove_file(out);
    Err("下载失败(直连/代理/备用源都试过了,检查网络后重试)".into())
}

/// curl 下到文件（进程成功退出才算；文件大小另由调用方 file_ok 兜底）。
fn curl_to_file(url: &str, out: &std::path::Path) -> bool {
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-fsSL", "--ssl-no-revoke", "--proxy", "", "-m", "180", "-o",
        &out.display().to_string(), url,
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Windows：PowerShell `Invoke-WebRequest -OutFile` 把 URL 二进制原样写到文件，
/// 走 WinINET 系统/自动代理 —— curl 直连失败但机器挂了代理时靠这条把文件下下来。
#[cfg(windows)]
fn ps_download_to_file(url: &str, out: &std::path::Path) -> bool {
    let dest = out.display().to_string().replace('\\', "\\\\");
    let script = format!(
        "[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13; \
         try {{ Invoke-WebRequest -Uri '{url}' -OutFile '{dest}' -TimeoutSec 180 -UseBasicParsing; exit 0 }} catch {{ exit 1 }}"
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// 解压:Windows 用 tar.exe 解 zip、macOS 用 tar 解 tar.gz(`-xf` 都能自动识别格式)。
/// 产物二进制在包根目录(rtk / rtk.exe);少数打包会多一层子目录,兜底找一下挪到根。
fn extract_rtk(pkg: &std::path::Path, dir: &std::path::Path) -> Result<(), String> {
    let tar = if cfg!(windows) { "tar.exe" } else { "tar" };
    let mut cmd = std::process::Command::new(tar);
    cmd.args(["-xf", &pkg.display().to_string(), "-C", &dir.display().to_string()]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("解压失败(缺 tar?): {e}"))?;
    if !rtk_exe().exists() {
        if let Some(found) = find_rtk_binary(dir) {
            let _ = std::fs::rename(&found, rtk_exe());
        }
    }
    if !rtk_exe().exists() {
        return Err(format!(
            "解压后没找到 rtk: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// 在解压目录的一层子目录里找 rtk 二进制(应对包内多一层的情况)。
fn find_rtk_binary(dir: &std::path::Path) -> Option<PathBuf> {
    let target = if cfg!(windows) { "rtk.exe" } else { "rtk" };
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let cand = p.join(target);
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    None
}

// ── 开/关 Claude hook(自己管 JSON,不调 rtk init)────────────────────────────────

pub fn set_enabled(enabled: bool) -> Result<String, String> {
    if enabled && !rtk_exe().exists() {
        return Err("请先安装 Token 压缩机".into());
    }
    // 这里改的是**用户自己的** `~/.claude/settings.json`（加/删 hook）。
    // 改别人的配置文件必须留痕：一旦 Claude Code 出怪毛病，第一件要排除的就是「是不是我们写坏了」。
    crate::ulog::write("rtk", if enabled { "开启 Token 压缩机 hook（写 ~/.claude/settings.json）" } else { "关闭 Token 压缩机 hook" });
    let path = claude_settings_path();
    // 读现有 settings.json(不存在则空对象);顶层必须是 object。
    let mut root: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    if !root.is_object() {
        return Err("~/.claude/settings.json 顶层不是对象,拒绝改动(避免破坏你的配置)".into());
    }

    // 首次改动前备份一次(不覆盖已有备份 —— 规避 rtk 覆盖用户 .bak 的坑)。
    if path.exists() {
        let bak = path.with_extension("json.uking-bak");
        if !bak.exists() {
            if let Ok(orig) = std::fs::read(&path) {
                let _ = std::fs::write(&bak, orig);
            }
        }
    }

    let obj = root.as_object_mut().unwrap();

    if enabled {
        // ★ 闸门：端到端验不过就**不许开**。
        //
        // 老实现这里写的是「失败不拦开启流程（PATH 写不进去是环境问题，hook 本身仍然
        // 有意义）」—— 那句话的假设是「没接上 PATH 顶多没省到」。真相是：没接上 =
        // 客户机上每一条 Bash 命令退出码 127，整台机器的 AI 变废。假设错了，
        // 所以放行的那个决定也就错了。宁可开不了，绝不半开着把机器毁掉。
        verify_pipeline()?;
        // 顺手把 rtk 挂上 PATH，纯粹是方便客户自己在终端敲 `rtk gain`。
        // 压缩能不能生效**已经不取决于它**（包装器自带绝对路径），所以失败不拦。
        if let Err(e) = ensure_on_path() {
            eprintln!("[rtk] ensure_on_path 失败（不影响压缩生效）: {e}");
        }
        add_hook(obj);
    } else {
        remove_hook(obj);
    }

    atomic_write(&path, serde_json::to_string_pretty(&root).unwrap_or_default().as_bytes())?;
    Ok(if enabled { "已开启 Token 压缩机" } else { "已关闭 Token 压缩机" }.into())
}

/// 往 settings.json 的 hooks.PreToolUse 加我们的 Bash hook(已存在则不重复加,保留用户其它 hook)。
fn add_hook(obj: &mut serde_json::Map<String, Value>) {
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let pre = hooks
        .as_object_mut()
        .unwrap()
        .entry("PreToolUse")
        .or_insert_with(|| json!([]));
    if !pre.is_array() {
        *pre = json!([]);
    }
    let arr = pre.as_array_mut().unwrap();
    let already = arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hs| hs.iter().any(|h| h.get("command").and_then(|c| c.as_str()).map(is_our_hook).unwrap_or(false)))
            .unwrap_or(false)
    });
    if !already {
        arr.push(json!({
            "matcher": "Bash",
            "hooks": [ { "type": "command", "command": hook_command() } ]
        }));
    }
}

/// 从 settings.json 删掉我们的 hook,并把空掉的容器一并删干净(不留 rtk 那种空壳)。
fn remove_hook(obj: &mut serde_json::Map<String, Value>) {
    let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };
    if let Some(pre) = hooks.get_mut("PreToolUse").and_then(|p| p.as_array_mut()) {
        // 每个 entry 内部先剔除我们的命令,再把空 entry 整个删掉。
        for entry in pre.iter_mut() {
            if let Some(hs) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                hs.retain(|h| !h.get("command").and_then(|c| c.as_str()).map(is_our_hook).unwrap_or(false));
            }
        }
        pre.retain(|entry| {
            entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|hs| !hs.is_empty())
                .unwrap_or(true)
        });
        if pre.is_empty() {
            hooks.remove("PreToolUse");
        }
    }
    if hooks.is_empty() {
        obj.remove("hooks");
    }
}

// ── 卸载(关 hook + 删 rtk.exe)────────────────────────────────────────────────────

pub fn uninstall() -> Result<String, String> {
    crate::ulog::section("rtk", "卸载 Token 压缩机（先摘 hook 再删目录）");
    let _ = set_enabled(false);
    let dir = rtk_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .inspect_err(|e| crate::ulog::write("rtk", &format!("删除 rtk 目录失败：{e}")))
            .map_err(|e| format!("删除 rtk 失败: {e}"))?;
    }
    Ok("已卸载 Token 压缩机(rtk.exe 删除、Claude hook 移除)".into())
}

// ── rtk gain 战绩解析 ─────────────────────────────────────────────────────────────

/// 完整战绩。前后两个绝对值 + 分项，全部来自 rtk 自己的记账，我们一个数都不编。
struct Gain {
    /// 压缩前原始 token
    before: u64,
    /// 压缩后进上下文的 token
    after: u64,
    saved: u64,
    pct: f64,
    commands: u64,
    top: Vec<GainCmd>,
    daily: Vec<GainDay>,
}

/// 读 rtk 的累计战绩。
///
/// **主路径走 `rtk gain --daily --format json`** —— 结构化、带每日序列，不用跟文本表格较劲。
/// 老版本 rtk 没有 `--format` 时退回解析文本汇总（只丢掉每日/分项，主数字仍在）。
/// 命令排行只有文本表有（JSON 不含 By Command），解析不出来就留空，不影响其余部分。
fn read_gain() -> Option<Gain> {
    let (mut g, json_ok) = match run_rtk(&["gain", "--daily", "--format", "json"])
        .ok()
        .and_then(|s| parse_gain_json(&s))
    {
        Some(g) => (g, true),
        None => (parse_gain_text(&run_rtk(&["gain"]).ok()?)?, false),
    };
    // 命令排行只能从文本表拿。JSON 路径下要多跑一次 `rtk gain`（毫秒级，可接受）。
    if json_ok {
        if let Ok(text) = run_rtk(&["gain"]) {
            g.top = parse_top_commands(&text);
        }
    }
    Some(g)
}

/// 解析 `rtk gain --daily --format json`。
fn parse_gain_json(s: &str) -> Option<Gain> {
    let v: Value = serde_json::from_str(s).ok()?;
    let sum = v.get("summary")?;
    let u = |k: &str| sum.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let before = u("total_input");
    let saved = u("total_saved");
    // 一条命令都没跑过时全是 0 —— 那是「暂无数据」，不是「省了 0」，别画个空面板。
    if u("total_commands") == 0 && before == 0 {
        return None;
    }
    let mut daily: Vec<GainDay> = v
        .get("daily")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let g = |k: &str| d.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    Some(GainDay {
                        date: d.get("date")?.as_str()?.to_string(),
                        commands: g("commands"),
                        before: g("input_tokens"),
                        after: g("output_tokens"),
                        saved: g("saved_tokens"),
                        pct: d.get("savings_pct").and_then(|x| x.as_f64()).unwrap_or(0.0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // 只留最近 14 天（面板画不下更多，也没人关心三个月前哪天省了多少）。
    if daily.len() > 14 {
        daily.drain(..daily.len() - 14);
    }
    Some(Gain {
        before,
        after: u("total_output"),
        saved,
        pct: sum.get("avg_savings_pct").and_then(|x| x.as_f64()).unwrap_or(0.0),
        commands: u("total_commands"),
        top: Vec::new(),
        daily,
    })
}

/// 兜底：解析 `rtk gain` 的文本汇总段（老版本 rtk 没有 `--format json`）。
fn parse_gain_text(out: &str) -> Option<Gain> {
    if out.contains("No tracking data") {
        return None;
    }
    let mut before: Option<u64> = None;
    let mut after: Option<u64> = None;
    let mut saved: Option<u64> = None;
    let mut pct: Option<f64> = None;
    let mut cmds: Option<u64> = None;
    for line in out.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Total commands:") {
            cmds = rest.trim().parse().ok();
        } else if let Some(rest) = l.strip_prefix("Input tokens:") {
            before = parse_abbrev(rest.trim());
        } else if let Some(rest) = l.strip_prefix("Output tokens:") {
            after = parse_abbrev(rest.trim());
        } else if let Some(rest) = l.strip_prefix("Tokens saved:") {
            // 形如 "1.6K (40.6%)"
            let rest = rest.trim();
            if let Some(num) = rest.split_whitespace().next() {
                saved = parse_abbrev(num);
            }
            if let Some(start) = rest.find('(') {
                if let Some(end) = rest[start..].find('%') {
                    pct = rest[start + 1..start + end].trim().parse().ok();
                }
            }
        }
    }
    let saved = saved?;
    Some(Gain {
        before: before.unwrap_or(0),
        after: after.unwrap_or(0),
        saved,
        pct: pct.unwrap_or(0.0),
        commands: cmds.unwrap_or(0),
        top: parse_top_commands(out),
        daily: Vec::new(),
    })
}

/// 解析 `rtk gain` 的 By Command 表，取省得最多的前 8 条。
///
/// 行长这样（列宽不固定，命令名里有空格，所以**从右往左**认字段）：
/// ```text
///  1.  rtk grep                    111   6.5K   19.1%    48ms  ██████████
/// ```
/// 从右往左依次是：影响力条 / 耗时 / 压缩率 / 省下量 / 次数，剩下的全是命令名。
/// 解析不出来就跳过该行 —— 排行是锦上添花，绝不能让它把整个面板拖垮。
fn parse_top_commands(out: &str) -> Vec<GainCmd> {
    let mut rows = Vec::new();
    for line in out.lines() {
        let l = line.trim();
        // 只认 "序号." 开头的数据行，跳过表头/分隔线/汇总段。
        let Some(rest) = l.split_once('.').and_then(|(n, r)| n.trim().parse::<u32>().ok().map(|_| r)) else {
            continue;
        };
        let mut fields: Vec<&str> = rest.split_whitespace().collect();
        // 末尾的影响力条（█/░）不是数据，丢掉。
        if fields.last().map(|f| f.chars().all(|c| c == '█' || c == '░')).unwrap_or(false) {
            fields.pop();
        }
        if fields.len() < 5 {
            continue;
        }
        let pct = fields[fields.len() - 2].trim_end_matches('%').parse::<f64>().ok();
        let saved = parse_abbrev(fields[fields.len() - 3]);
        let count = fields[fields.len() - 4].parse::<u64>().ok();
        let (Some(pct), Some(saved), Some(count)) = (pct, saved, count) else {
            continue;
        };
        let command = fields[..fields.len() - 4].join(" ");
        if command.is_empty() {
            continue;
        }
        rows.push(GainCmd { command, count, saved, pct });
    }
    rows.sort_by(|a, b| b.saved.cmp(&a.saved));
    rows.truncate(8);
    rows
}

// ── 现场演示：把「省的原理」当场跑给客户看 ────────────────────────────────────────
//
// 为什么要有这一段：Token 压缩机最难卖的不是功能，是**信任**——客户看不见它干了什么，
// 只看到一个「省了 27%」的数字，凭什么信？所以这里内嵌两段真实形态的日志，
// 点一下**当场跑真的 rtk**，把压缩前后原文并排摆出来。看得见砍了什么、留下了什么，
// 才有人信「省 token 不降智」这句话。截图还能直接拿去宣传（样例是内嵌的，不含任何隐私）。
//
// 刻意的选择：
// - **用内嵌样例，不扫用户的真文件** —— 确定性、离线可跑、截图可公开、零隐私顾虑。
// - **真跑 rtk，不预存结果** —— 预存等于我们自己写答案，那就又变成"信我"了。
//   客户机上 rtk 版本变了、行为变了，这里当场就会变，这才是可核对的。

/// 演示样例：构建日志。噪音密集（一堆 Compiling），但夹着 1 个 error + 1 个 warning。
const SAMPLE_BUILD: &str = r#"   Compiling proc-macro2 v1.0.86
   Compiling unicode-ident v1.0.12
   Compiling quote v1.0.36
   Compiling syn v2.0.72
   Compiling serde v1.0.204
   Compiling serde_json v1.0.120
   Compiling tokio v1.39.2
   Compiling hyper v1.4.1
warning: unused variable: `cfg`
  --> src/main.rs:42:9
   |
42 |     let cfg = load();
   |         ^^^ help: if this is intentional, prefix it with an underscore
   |
   = note: `#[warn(unused_variables)]` on by default
   Compiling reqwest v0.12.5
   Compiling tower v0.4.13
error[E0308]: mismatched types
  --> src/api.rs:88:22
   |
88 |     send_request(port)
   |                  ^^^^ expected `&str`, found `u16`
"#;

/// 演示样例：测试输出。23 条通过 + 1 条失败 —— 通过的那些对大模型毫无信息量。
const SAMPLE_TEST: &str = r#"running 24 tests
test config::tests::parse_empty ... ok
test config::tests::parse_basic ... ok
test config::tests::parse_nested ... ok
test config::tests::merge_defaults ... ok
test net::tests::retry_on_timeout ... ok
test net::tests::retry_gives_up ... ok
test store::tests::atomic_write ... ok
test store::tests::atomic_write_replaces ... ok
test store::tests::rollback_on_error ... FAILED

failures:

---- store::tests::rollback_on_error stdout ----
thread 'store::tests::rollback_on_error' panicked at src/store.rs:214:9:
assertion `left == right` failed
  left: 3
 right: 0

test result: FAILED. 23 passed; 1 failed; 0 ignored
"#;

/// 一个演示案例的前后对比。
#[derive(Serialize)]
pub struct DemoCase {
    pub id: &'static str,
    pub title: &'static str,
    /// 一句话说清这个案例砍了什么、留了什么。
    pub note: &'static str,
    pub before: String,
    pub after: String,
    pub before_chars: usize,
    pub after_chars: usize,
    /// 估算 token（见 `est_tokens`，仅供直观对比）。
    pub before_tokens: usize,
    pub after_tokens: usize,
    /// 省下的比例（按字符算，0~100）。
    pub saved_pct: f64,
}

/// 粗估 token 数：中日韩字符按 1 个字 ≈ 1 token，其余按 4 字符 ≈ 1 token。
///
/// **只用于演示面板的直观对比**，不参与任何计费。真实数字看战绩面板（那是 rtk 自己记的账）。
fn est_tokens(s: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for c in s.chars() {
        if matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}') {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

/// 当场跑一遍压缩，返回前后对比。**只读**：写的是我们自己目录下的临时样例文件，跑完就删。
pub fn demo() -> Result<Vec<DemoCase>, String> {
    if !rtk_exe().exists() {
        return Err("请先安装 Token 压缩机".into());
    }
    let dir = rtk_dir().join("demo");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建演示目录失败: {e}"))?;

    let specs: [(&'static str, &'static str, &'static str, &'static str); 2] = [
        (
            "build",
            "构建日志（cargo build）",
            "十几行 Compiling 噪音被折叠，报错和告警连行号一起原样留下。",
            SAMPLE_BUILD,
        ),
        (
            "test",
            "测试输出（cargo test）",
            "23 条通过的测试对大模型毫无信息量，折叠；唯一的失败连 panic 位置一起留下。",
            SAMPLE_TEST,
        ),
    ];

    let mut cases = Vec::new();
    for (id, title, note, sample) in specs {
        let path = dir.join(format!("{id}.log"));
        std::fs::write(&path, sample).map_err(|e| format!("写演示样例失败: {e}"))?;
        let after = run_rtk(&["log", &path.display().to_string()])?;
        let _ = std::fs::remove_file(&path);
        let after = after.trim_end().to_string();
        // rtk 没吐东西 = 这台机器上跑不起来，别拿个空框骗人。
        if after.trim().is_empty() {
            return Err(format!("演示失败：rtk 没有返回压缩结果（{title}）"));
        }
        let before = sample.trim_end().to_string();
        let before_chars = before.chars().count();
        let after_chars = after.chars().count();
        let saved_pct = if before_chars > 0 {
            ((before_chars.saturating_sub(after_chars)) as f64 / before_chars as f64 * 1000.0).round() / 10.0
        } else {
            0.0
        };
        cases.push(DemoCase {
            id,
            title,
            note,
            before_tokens: est_tokens(&before),
            after_tokens: est_tokens(&after),
            before,
            after,
            before_chars,
            after_chars,
            saved_pct,
        });
    }
    let _ = std::fs::remove_dir(&dir);
    Ok(cases)
}

/// "1.6K" → 1600, "2.3M" → 2_300_000, "512" → 512。
fn parse_abbrev(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix('K').or_else(|| s.strip_suffix('k')) {
        (n, 1_000.0)
    } else if let Some(n) = s.strip_suffix('M').or_else(|| s.strip_suffix('m')) {
        (n, 1_000_000.0)
    } else {
        (s, 1.0)
    };
    num.trim().parse::<f64>().ok().map(|v| (v * mult) as u64)
}

// ── 测试：战绩解析（纯函数，不碰真机）──────────────────────────────────────────────
//
// 为什么只测解析：装 / 开关 / 下载都要真机真网，测了也只是测 mock。而**解析是最脆的一环**
// —— rtk 换个版本把表格列宽一改、把 "Tokens saved" 改个措辞，面板就会静默显示 0。
// 这几条夹具就是从真机 `rtk gain` 原样拷下来的。
#[cfg(test)]
mod tests {
    use super::*;

    /// 真机 `rtk gain --daily --format json` 的原样输出（截短了 daily）。
    const REAL_JSON: &str = r#"{
      "summary": {
        "total_commands": 362,
        "total_input": 92874,
        "total_output": 67928,
        "total_saved": 25034,
        "avg_savings_pct": 26.954798974955317,
        "total_time_ms": 1336210,
        "avg_time_ms": 3691
      },
      "daily": [
        {"date":"2026-07-21","commands":4,"input_tokens":4010,"output_tokens":2383,"saved_tokens":1627,"savings_pct":40.57},
        {"date":"2026-07-26","commands":356,"input_tokens":87631,"output_tokens":64670,"saved_tokens":23049,"savings_pct":26.30}
      ]
    }"#;

    /// 真机 `rtk gain` 的 By Command 段（注意：命令名带空格、还会被 rtk 截断成 `...`）。
    const REAL_TABLE: &str = "
By Command
───────────────────────────────────────────────────────────────────────
  #  Command                   Count  Saved    Avg%    Time  Impact
───────────────────────────────────────────────────────────────────────
 1.  rtk grep                    111   6.5K   19.1%    48ms  ██████████
 2.  rtk read                     18   5.1K   24.6%     2ms  ████████░░
 3.  rtk ls -la C:/Users/Z...      1   2.5K   49.2%    1.6s  ████░░░░░░
 4.  rtk cargo build --mes...      9   2.0K   91.3%   13.0s  ███░░░░░░░
───────────────────────────────────────────────────────────────────────
";

    #[test]
    fn json_gain_gives_before_and_after() {
        let g = parse_gain_json(REAL_JSON).expect("应能解析真机 JSON");
        assert_eq!(g.before, 92874, "压缩前");
        assert_eq!(g.after, 67928, "压缩后");
        assert_eq!(g.saved, 25034);
        assert_eq!(g.commands, 362);
        assert_eq!(g.daily.len(), 2);
        assert_eq!(g.daily[0].date, "2026-07-21");
        assert_eq!(g.daily[1].saved, 23049);
    }

    /// 一条命令都没跑过 ≠ 「省了 0」。得让面板显示「暂无数据」，不是画一堆 0。
    #[test]
    fn empty_stats_are_no_data_not_zero() {
        let empty = r#"{"summary":{"total_commands":0,"total_input":0,"total_output":0,"total_saved":0,"avg_savings_pct":0.0}}"#;
        assert!(parse_gain_json(empty).is_none());
    }

    #[test]
    fn command_table_parses_names_with_spaces() {
        let rows = parse_top_commands(REAL_TABLE);
        assert_eq!(rows.len(), 4, "四条数据行，表头/分隔线不算");
        // 按省下的量降序
        assert_eq!(rows[0].command, "rtk grep");
        assert_eq!(rows[0].count, 111);
        assert_eq!(rows[0].saved, 6500);
        assert!((rows[0].pct - 19.1).abs() < 0.01);
        // 命令名带空格、带 rtk 自己截断的省略号，都要原样留住
        assert_eq!(rows[2].command, "rtk ls -la C:/Users/Z...");
        assert_eq!(rows[3].command, "rtk cargo build --mes...");
        assert!((rows[3].pct - 91.3).abs() < 0.01);
    }

    /// 表头行长得跟数据行很像（也有一串词），必须靠「序号.」把它挡在外面。
    #[test]
    fn table_header_is_not_a_row() {
        let rows = parse_top_commands("  #  Command                   Count  Saved    Avg%    Time  Impact");
        assert!(rows.is_empty());
    }

    /// 老版本 rtk 没有 `--format json` 时的兜底路径：主数字仍要在。
    #[test]
    fn text_fallback_still_gives_the_numbers() {
        let text = "\
Total commands:    362
Input tokens:      92.8K
Output tokens:     67.9K
Tokens saved:      25.0K (26.9%)
";
        let g = parse_gain_text(text).expect("兜底解析应成功");
        assert_eq!(g.commands, 362);
        assert_eq!(g.before, 92800);
        assert_eq!(g.after, 67900);
        assert_eq!(g.saved, 25000);
        assert!((g.pct - 26.9).abs() < 0.01);
    }

    #[test]
    fn abbrev_units() {
        assert_eq!(parse_abbrev("1.6K"), Some(1600));
        assert_eq!(parse_abbrev("2.3M"), Some(2_300_000));
        assert_eq!(parse_abbrev("512"), Some(512));
        assert_eq!(parse_abbrev("abc"), None);
    }

    /// 演示样例得真的**有东西可压** —— 样例本身太干净的话，演示出来就没说服力。
    #[test]
    fn demo_samples_are_worth_compressing() {
        assert!(SAMPLE_BUILD.lines().filter(|l| l.contains("Compiling")).count() >= 8);
        assert!(SAMPLE_BUILD.contains("error[E0308]"), "样例必须含报错，才能演示「报错不丢」");
        assert!(SAMPLE_TEST.lines().filter(|l| l.ends_with("... ok")).count() >= 8);
        assert!(SAMPLE_TEST.contains("FAILED"), "样例必须含失败，才能演示「失败不丢」");
    }

    /// rtk 0.43.0 的真实 hook 输出（`echo '{...}' | rtk hook claude` 原样抄回）。
    const REAL_HOOK_OUT: &str = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecisionReason":"RTK auto-rewrite","updatedInput":{"command":"rtk ls -la /tmp","description":"list"},"permissionDecision":"allow"}}"#;

    /// 裸 `rtk` 必须被换成绝对路径 —— 这一条就是客户机上「所有命令都失败」的分水岭。
    #[test]
    fn bare_rtk_is_rewritten_to_absolute_path() {
        // 🔴 **必须进沙箱**（2026-08-11 修）：这条测试两次解析家目录 —— `rewrite_hook_output`
        // 内部算一次 `rtk_exe()`，下面的断言又算一次。裸跑时另一个测试会在这中间把
        // `UKING_TEST_HOME` / `USERPROFILE` 指到它自己的沙箱，两次算出不同的路径，于是：
        //
        //     改写后必须以 rtk 绝对路径打头，实际是 "C:/Users/<user>/.uking/tools/rtk/rtk.exe" ls -la /tmp
        //
        // —— 报错里那个「实际值」**看着完全正确**，正是这条 flake 最坑的地方：它诱使你去
        // 怀疑改写逻辑（那是客户机上真出过事的地方），而真正对不上的是**断言里的期望值**。
        // 实测 12 跑 2 红。走 `testsandbox` 那把全进程唯一的锁，env 在这一段独占且稳定。
        let _sb = crate::testsandbox::enter("rtk-bare-rewrite", &[]);
        let out = rewrite_hook_output(REAL_HOOK_OUT).expect("真实输出必须能改写");
        let v: Value = serde_json::from_str(&out).unwrap();
        let cmd = v.pointer("/hookSpecificOutput/updatedInput/command").unwrap().as_str().unwrap();
        assert!(
            cmd.starts_with(&quoted(&rtk_exe())),
            "改写后必须以 rtk 绝对路径打头，实际是 {cmd}"
        );
        assert!(cmd.ends_with(" ls -la /tmp"), "参数必须原样保留，实际是 {cmd}");
        // 别的字段一个都不许动：permissionDecision 丢了 Claude Code 会改走确认流程。
        assert_eq!(v.pointer("/hookSpecificOutput/permissionDecision").unwrap(), "allow");
        assert_eq!(v.pointer("/hookSpecificOutput/updatedInput/description").unwrap(), "list");
    }

    /// **fail-open 的四种入口**：任何一种都必须「不吐东西」而不是吐半份 JSON。
    /// 空输出 = Claude Code 按原命令跑 = 最坏只是没省到；吐坏 JSON = 那一轮直接报错。
    #[test]
    fn unusable_output_falls_open_instead_of_breaking_the_command() {
        assert_eq!(rewrite_hook_output(""), None, "空输入（rtk 说这条不用改）");
        assert_eq!(rewrite_hook_output("   \n"), None, "只有空白");
        assert_eq!(rewrite_hook_output("not json at all"), None, "解析不了就别吐");
        assert_eq!(
            rewrite_hook_output("[rtk] /!\\ No hook installed\n{\"a\":1}"),
            None,
            "招呼语混进 stdout 时也不许吐半份"
        );
    }

    /// 认不出的形状要**原样透传**，不能自作主张改坏（万一 rtk 自己改成绝对路径了）。
    #[test]
    fn unknown_shapes_pass_through_untouched() {
        // 已经是绝对路径：不再插一手
        let already = r#"{"hookSpecificOutput":{"updatedInput":{"command":"/opt/rtk ls"}}}"#;
        let out = rewrite_hook_output(already).unwrap();
        assert!(out.contains("/opt/rtk ls"), "已是绝对路径的命令不该被改：{out}");
        // 根本没有 updatedInput：透传
        let other = r#"{"hookSpecificOutput":{"permissionDecision":"deny"}}"#;
        let out = rewrite_hook_output(other).unwrap();
        assert!(out.contains("deny"));
    }

    /// 新老两种 hook 写法都要认得出来，且能分辨。
    ///
    /// 认不出老写法 = `remove_hook` 摘不干净、自愈无从谈起；分辨不了 =
    /// status 没法区分「开着且好用」和「开着且正在毁掉这台机器」。
    #[test]
    fn both_hook_spellings_are_recognized_and_told_apart() {
        let legacy = "\"C:/Users/x/.uking/tools/rtk/rtk.exe\" hook claude";
        let wrapper = "\"C:/Users/x/.uking/U-King.exe\" rtk-hook";
        assert!(is_our_hook(legacy) && is_our_hook(wrapper));
        assert_eq!(hook_kind_of(legacy), HookKind::Legacy);
        assert_eq!(hook_kind_of(wrapper), HookKind::Wrapper);
        assert_eq!(hook_kind_of("echo hi"), HookKind::Absent);
        assert!(!is_our_hook("echo hi"));
        // 🔴 路径里**恰好含** `rtk-hook` 不算新写法。第一版的子串匹配就栽在这：
        // 老式 hook 被误判成新式 → 自愈跳过 → 客户机继续每条命令都失败。
        assert_eq!(
            hook_kind_of("\"C:/tmp/rtk-hook-heal/tools/rtk/rtk.exe\" hook claude"),
            HookKind::Legacy,
            "目录名含 rtk-hook 不能把老写法伪装成新写法"
        );
        // 我们**当下**写出去的必须是新写法
        assert_eq!(hook_kind_of(&hook_command()), HookKind::Wrapper);
    }

    /// `ready` 的判据要跟着写法走：老写法怕 PATH，新写法不怕。
    /// 给新写法报「rtk 不在 PATH」是发假警报，客户会去修一个根本不存在的问题。
    #[test]
    fn only_legacy_hook_depends_on_path() {
        assert!(hook_is_effective(HookKind::Wrapper, false), "新写法不该被 PATH 判死");
        assert!(hook_is_effective(HookKind::Legacy, true));
        assert!(!hook_is_effective(HookKind::Legacy, false), "老写法 + 没 PATH = 每条命令都废");
        assert!(!hook_is_effective(HookKind::Absent, true));
    }

    /// 自愈 + 摘除的整条链路。
    ///
    /// 🔴 必须合成**一个** `#[test]`：`UKING_TEST_HOME` 是进程级环境变量，
    /// 拆成多个用例会互相抢（同 automation.rs / aitasks.rs 的约定）。
    #[test]
    fn legacy_hook_is_healed_and_user_hooks_survive() {
        let sb = crate::testsandbox::enter("rtk-hook-heal", &[".claude"]);
        let settings = sb.root().join(".claude").join("settings.json");

        // 客户机现场：一条老式 rtk hook + 一条用户自己的 hook
        let legacy = format!(
            r#"{{"env":{{"ANTHROPIC_MODEL":"deepseek-v4-flash"}},"hooks":{{"PreToolUse":[
                {{"matcher":"Bash","hooks":[{{"type":"command","command":"\"{}\" hook claude"}}]}},
                {{"matcher":"Write","hooks":[{{"type":"command","command":"my-own-guard.sh"}}]}}
            ]}}}}"#,
            rtk_exe().display().to_string().replace('\\', "/")
        );
        std::fs::write(&settings, legacy).unwrap();
        assert_eq!(installed_hook_kind(), HookKind::Legacy, "先得认得出老写法");

        // 沙箱里没有 rtk.exe → 自检过不去 → 应当**摘掉**而不是留着害人
        let acted = heal_legacy_hook().expect("老写法必须被处理");
        assert!(acted.contains("摘掉"), "rtk 不在时应摘除，实际：{acted}");
        assert_eq!(installed_hook_kind(), HookKind::Absent);

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        // 用户自己的 hook 一个字节都不许动
        let pre = after.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre.len(), 1, "只该剩用户自己那条");
        assert_eq!(pre[0].pointer("/hooks/0/command").unwrap(), "my-own-guard.sh");
        // 与 hook 无关的配置也不许动
        assert_eq!(after.pointer("/env/ANTHROPIC_MODEL").unwrap(), "deepseek-v4-flash");

        // 没有我们的 hook 时，自愈必须是 no-op（别每次启动都重写用户的配置文件）
        assert_eq!(heal_legacy_hook(), None);

        // 闸门：沙箱里没装 rtk，开关必须开不起来，且**不许写进 settings.json**
        let err = set_enabled(true).expect_err("没装 rtk 时不该开得起来");
        assert!(err.contains("安装"), "错误得说人话，实际：{err}");
        assert_eq!(installed_hook_kind(), HookKind::Absent, "闸门拦下后不许留下半条 hook");
    }
}
