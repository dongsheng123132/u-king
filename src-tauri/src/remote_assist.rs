//! 远程协助 —— 客户在「技术支持」页一键开启，作者就能连上来看现场。
//!
//! ## 为什么要有它
//!
//! 以前客户报 bug，我们得让他「打开 PowerShell，粘贴 `irm https://u-claw.org.cn/agent.ps1 | iex`，
//! 输 y 回车，把屏幕上那串 pc-XXXX 发我」。小白卡在第一步的比例极高，于是大量问题最后
//! 靠截图猜。这个模块把那套流程收进一个按钮。
//!
//! ## 边界（这不是「后门」，是有闸门的协助通道）
//!
//! - **默认关**。必须客户在界面上主动点，且点之前那段文案明写「开启后作者可以在你电脑上执行命令」。
//! - **可随时停**。界面上有「停止协助」，不依赖超时。
//! - **会自动过期**。`SESSION_MAX_SECS` 到点自杀 —— 注意 2 小时超时是**官方 agent.ps1 那层外壳**
//!   做的，`agent.exe` 自己没有这个逻辑。我们直接起进程就绕过了那层外壳，**必须自己看门狗**，
//!   否则客户点一次就永久开着。
//! - **有审计**。agent 每执行一条命令都写 `%TEMP%\uclaw\sessions\<日期>.log`，界面上给按钮直接打开。
//!
//! ## 模块独立性（对齐 CLAUDE.md 的四条铁律）
//!
//! 只暴露纯函数，`#[tauri::command]` 全在 `lib.rs` 转调，本模块不碰 `AppHandle`；
//! 不 import 任何其它功能模块，公共能力（系统 curl）复用 `installer::curl`。
//! 删这个功能只需动 `lib.rs`（去 mod + 3 个 command）和 `Feedback.tsx`（去一个区块）。

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 中转服务器（自签 TLS）。与 `agent.ps1` 同源，改这里要连它一起改。
const RELAY_SERVER: &str = "wss://relay.u-claw.org:8900";
/// agent 侧 token —— **公开值**，本来就烤在 agent.exe 里，不是密钥。
/// 真正的机密是运维端的 controller token，那个只在运维机器上，客户端一个字节都不该有。
const AGENT_TOKEN: &str = "uclaw-agent-pub";
/// 下载地址走 `.cn`：`.org` 子域在国内部分网络被 GFW SNI 阻断（CLAUDE.md 铁律）。
/// 2026-07-28 在裸网客户机 pc-*** 实测 200 / 8.6MB。
const AGENT_URL: &str = "https://u-claw.org.cn/downloads/agent.exe";
/// 一次协助最长存活（秒）。与官方 agent.ps1 的 `$TIMEOUT_HOURS = 2` 对齐。
const SESSION_MAX_SECS: u64 = 2 * 3600;

/// 正在跑的协助会话。
struct Session {
    child: Child,
    device_id: String,
    /// 起始时间（Unix 秒），用于算剩余时长和看门狗。
    started_unix: u64,
}

fn slot() -> &'static Mutex<Option<Session>> {
    static SLOT: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 给前端的状态。字段名与 `Feedback.tsx` 里的 `AssistStatus` 一一对应。
#[derive(serde::Serialize, Clone, Default)]
pub struct AssistStatus {
    pub running: bool,
    /// 运行中才有：`pc-XXXX`，客户要把它发给作者。
    pub device_id: Option<String>,
    /// 运行中才有：还剩多少秒自动断开。
    pub remaining_secs: Option<u64>,
    /// 审计日志路径（不管开没开都给，方便客户事后查我们做过什么）。
    pub audit_log: String,
    /// 本平台支不支持（agent.exe 只有 Windows 版）。
    pub supported: bool,
}

/// agent 的工作目录 —— **刻意复用官方 `agent.ps1` 的 `%TEMP%\uclaw`**，不另存一份到 `~/.uking`。
/// 理由是宪法第 8 条：同一个 agent.exe 存两份就会漂移两份，而且官方脚本的 `-Uninstall`
/// 只认这个路径，另存一份等于留个它清不掉的残留。
fn agent_dir() -> PathBuf {
    std::env::temp_dir().join("uclaw")
}

fn agent_path() -> PathBuf {
    agent_dir().join("agent.exe")
}

/// 审计日志：agent 每执行一条远程命令都会往这里追加。按天分文件。
pub fn audit_log_path() -> PathBuf {
    // 只拼当天的文件名；agent 自己建目录。日期用本地时间会引入时区依赖，
    // 这里跟 agent 一致按本地日期拼——拿不到就退回目录本身（前端打开目录也够用）。
    agent_dir().join("sessions")
}

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".uking")
}

/// Device ID 落盘的位置 —— **同一台机器复用同一个 ID**。
/// 官方脚本每次随机生成，客户第二次求助时 ID 就变了，运维那边对不上历史。
fn device_id_file() -> PathBuf {
    uking_home().join("remote-assist-id.txt")
}

/// 取（或首次生成）本机的 `pc-XXXX`。沿用官方脚本的 4 位数格式，运维侧工具无需改。
fn ensure_device_id() -> String {
    let f = device_id_file();
    if let Ok(s) = std::fs::read_to_string(&f) {
        let s = s.trim().to_string();
        if s.starts_with("pc-") && s.len() >= 5 {
            return s;
        }
    }
    // 没有随机数 crate（体积优先），用纳秒时间 + pid 混一下取 4 位。
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0);
    let n = 1000 + ((nanos ^ std::process::id() as u64) % 9000);
    let id = format!("pc-{n}");
    if let Some(p) = f.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    // 🔴 首次生成必须**原子建档**，不能裸 `fs::write`：两个调用方并发首启时（GUI 一个线程、
    // CLI/托盘另一个线程，或 cargo test 的并行用例），各自算出不同的 4 位数再互相覆盖 ——
    // 先返回的那个和最终落盘的对不上，客户第二次求助时运维按 ID 查不到历史。
    // `create_new` 让只有一个调用方能建成，其余读回赢家写的那份，同机永远同一个 ID。
    // 判据：删掉 remote-assist-id.txt（= 干净机首次运行）后 `cargo test --lib remote_assist`
    // 必挂；文件存在时全绿 —— 典型的「开发机绿、干净机红」。
    use std::io::Write;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&f) {
        Ok(mut fh) => {
            let _ = fh.write_all(id.as_bytes());
            id
        }
        // 建不成 = 别人先建了（或建不了盘）。读回来用；读不出合法值才退回自己算的。
        Err(_) => std::fs::read_to_string(&f)
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| s.starts_with("pc-") && s.len() >= 5)
            .unwrap_or(id),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 本平台能不能用（agent.exe 是 Windows 专属构建）。
pub fn supported() -> bool {
    cfg!(target_os = "windows")
}

/// 查状态。顺手做两件收尾：进程已自己退出就清槽位；超时了就杀掉。
pub fn status() -> AssistStatus {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    let mut out = AssistStatus {
        running: false,
        device_id: None,
        remaining_secs: None,
        audit_log: audit_log_path().to_string_lossy().to_string(),
        supported: supported(),
    };
    let expired = if let Some(s) = g.as_mut() {
        match s.child.try_wait() {
            // 已退出（客户关机 / agent 自己崩了）→ 清槽位
            Ok(Some(_)) | Err(_) => {
                *g = None;
                false
            }
            Ok(None) => {
                let elapsed = now_unix().saturating_sub(s.started_unix);
                if elapsed >= SESSION_MAX_SECS {
                    true
                } else {
                    out.running = true;
                    out.device_id = Some(s.device_id.clone());
                    out.remaining_secs = Some(SESSION_MAX_SECS - elapsed);
                    false
                }
            }
        }
    } else {
        false
    };
    if expired {
        if let Some(mut s) = g.take() {
            let _ = s.child.kill();
            let _ = s.child.wait();
        }
    }
    out
}

/// 确保 agent.exe 在本地且看起来完整；缺了就下。`log` 用来把进度透给前端。
fn ensure_agent(log: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let dir = agent_dir();
    let exe = agent_path();
    // 已存在且大小合理就直接用（8MB 量级；<1MB 一律当成上次下坏了，重下）。
    if let Ok(md) = std::fs::metadata(&exe) {
        if md.len() > 1_000_000 {
            return Ok(exe);
        }
        log("检测到上次下载的文件不完整，重新下载…");
        let _ = std::fs::remove_file(&exe);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败: {e}"))?;
    log("正在下载协助程序（约 8MB）…");
    // 二进制必须用 curl 的 -o 落盘：走 stdout 会被 utf8 lossy 解码毁掉（providers.rs 同款教训）。
    let out = exe.to_string_lossy().to_string();
    crate::installer::curl(&["-fL", "--max-time", "180", "-o", &out, AGENT_URL])
        .map_err(|e| format!("下载协助程序失败：{e}\n（可以改用老办法：在 PowerShell 里跑 irm https://u-claw.org.cn/agent.ps1 | iex）"))?;
    let md = std::fs::metadata(&exe).map_err(|e| format!("下载后找不到文件: {e}"))?;
    if md.len() < 1_000_000 {
        let _ = std::fs::remove_file(&exe);
        return Err(format!(
            "下载的文件不完整（只有 {} 字节）。多半是网络被拦或杀软删了，请稍后重试。",
            md.len()
        ));
    }
    log("下载完成。");
    Ok(exe)
}

/// 开启协助。返回带 Device ID 的状态。
///
/// `log` 是进度回调（下载/连接），由 `lib.rs` 转成 `uking:remote_assist` 事件发给前端 ——
/// 模块本身不碰 `AppHandle`（模块独立铁律 ①）。
pub fn start(log: &dyn Fn(&str)) -> Result<AssistStatus, String> {
    if !supported() {
        return Err("远程协助目前只支持 Windows。Mac 上请把「复制脱敏诊断」的内容发给作者。".into());
    }
    // 已经在跑就别重复起（重复起会有两个 agent 抢同一个 ID）。
    let cur = status();
    if cur.running {
        return Ok(cur);
    }

    let exe = ensure_agent(log)?;
    let device_id = ensure_device_id();
    log("正在连接协助服务器…");

    let mut cmd = std::process::Command::new(&exe);
    cmd.args([
        "-server",
        RELAY_SERVER,
        "-token",
        AGENT_TOKEN,
        "-id",
        &device_id,
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：别在客户屏幕上闪黑窗
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("启动协助程序失败: {e}（可能被杀毒软件拦了，请在杀软里允许后重试）"))?;

    {
        let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
        *g = Some(Session {
            child,
            device_id: device_id.clone(),
            started_unix: now_unix(),
        });
    }

    // 看门狗：到点自动断开。**这层必须我们自己做** —— 2 小时超时原本在 agent.ps1 那层外壳里，
    // 我们直接起进程绕过了它，不补这条客户点一次就永久开着。
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let st = status(); // status() 内部就会处理超时并杀进程
        if !st.running {
            break;
        }
    });

    log(&format!("已连接。你的协助编号：{device_id}"));
    Ok(status())
}

/// 停止协助 —— **只杀我们自己起的那个 PID**。
///
/// 绝不用 `taskkill /IM agent.exe`：`agent` 是极其通用的进程名，`/IM` 会把全机器同名进程一起端掉
/// （宪法明令；`backup.rs` 那次 `taskkill /IM Claude.exe` 杀光客户全部 Claude Code 会话就是这么来的）。
pub fn stop() -> Result<(), String> {
    let mut g = slot().lock().unwrap_or_else(|e| e.into_inner());
    match g.take() {
        Some(mut s) => {
            s.child
                .kill()
                .map_err(|e| format!("停止协助失败: {e}"))?;
            let _ = s.child.wait();
            Ok(())
        }
        // 本来就没开着 —— 幂等，不报错。
        None => Ok(()),
    }
}

/// 打开审计日志所在目录，让客户自己看我们执行过哪些命令。
pub fn reveal_audit_dir() -> Result<(), String> {
    let dir = audit_log_path();
    // 目录可能还没建（从没连过）—— 建一个空的再打开，比弹「路径不存在」友好。
    let _ = std::fs::create_dir_all(&dir);
    open_dir(&dir)
}

#[cfg(target_os = "windows")]
fn open_dir(p: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer")
        .arg(p)
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开目录失败: {e}"))
}

#[cfg(not(target_os = "windows"))]
fn open_dir(p: &Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(p)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开目录失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Device ID 必须是运维侧工具认得的 `pc-XXXX` 四位格式。
    #[test]
    fn device_id_has_expected_shape() {
        let id = ensure_device_id();
        assert!(id.starts_with("pc-"), "device id 形状不对: {id}");
        let n: u32 = id.trim_start_matches("pc-").parse().expect("后缀应是数字");
        assert!((1000..10000).contains(&n), "应为 4 位数: {id}");
    }

    /// 同一台机器要复用同一个 ID —— 客户第二次求助时运维那边才对得上历史。
    #[test]
    fn device_id_is_stable_across_calls() {
        assert_eq!(ensure_device_id(), ensure_device_id());
    }

    /// 没开会话时 stop() 必须是幂等的（前端可能连点两下）。
    #[test]
    fn stop_is_idempotent_when_not_running() {
        assert!(stop().is_ok());
        assert!(stop().is_ok());
    }
}
