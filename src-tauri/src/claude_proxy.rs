//! Claude Code ↔ OpenAI 翻译桥的启停（独立可插拔模块）。
//!
//! 翻译逻辑全在 `resources/claude-openai-proxy.mjs`（自检 `…selftest.mjs`，45 条断言）；
//! 本模块只管**进程生命周期**：写脚本 → 便携 Node 起进程 → 健康检查 → 停。
//!
//! 跟 `codex_proxy.rs` 是同一套路的另一个方向（那个 responses↔chat 走 15722，
//! 这个 messages↔chat 走 15723），`find_node` / 端点归一都复用 `installer` 的公共实现。
//!
//! 模块纪律（宪法第 12 条）：只暴露纯函数，`#[tauri::command]` 全写在 `lib.rs` 转调；
//! 不 import 别的功能模块（`providers` / `codex_proxy` 一概不认），要共享就下沉到 `installer`。

use serde::Serialize;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// 监听端口。跟 codex 代理的 15722 岔开，两个桥可以同时开着。
pub const PROXY_PORT: u16 = 15723;

const PROXY_SCRIPT: &str = include_str!("../resources/claude-openai-proxy.mjs");

fn running() -> &'static Mutex<Option<Child>> {
    static R: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

/// 桥的状态。**带 `ready` + `blockers`**：回答「能不能用」而不是「装没装」
/// —— 这条约定是被真 bug 逼出来的（Token 压缩机 `installed:true` 形状全对、
/// conformance 全绿，可 hook 改写出的裸 `rtk` 不在 PATH 上，客户开了两天一点没省）。
#[derive(Serialize, Clone)]
pub struct BridgeStatus {
    pub running: bool,
    pub port: u16,
    /// 写进 Claude Code 的 `ANTHROPIC_BASE_URL` 就是这个值。
    pub base_url: String,
    /// 当前这座桥转发到哪儿（没起就是空串）。
    pub upstream: String,
    /// 找到的 node 路径；`None` = 这台机器上没 Node，桥起不来。
    pub node: Option<String>,
    pub ready: bool,
    pub blockers: Vec<String>,
    /// ★ **产品边界当数据发**：桥是 U-King 的子进程，**U-King 一退出桥就没了**，
    /// 那一刻 Claude Code 会立刻连不上。GUI 文案 / CLI / MCP 读的是同一句话，
    /// 不会三处各自跑偏（同 `automation` 的 `runs_only_while_app_open`）。
    pub runs_only_while_app_open: bool,
}

pub fn base_url() -> String {
    format!("http://127.0.0.1:{PROXY_PORT}")
}

fn script_path() -> PathBuf {
    crate::installer::uking_home().join("claude-openai-proxy.mjs")
}

/// 把供应商的 OpenAI base 归一成 chat/completions 全 URL。
///
/// **空了直接报错，不给默认值** —— 跟 codex 那条（空则回退虾盘云）故意不一样：
/// 这座桥是给「客户自带的中转」用的，猜一个端点等于把他的请求发去别人家。
pub fn upstream_for(openai_base: &str) -> Result<String, String> {
    crate::installer::to_chat_completions_url(openai_base)
        .ok_or_else(|| "这个供应商没填 OpenAI 端点，桥不知道该往哪儿转发".to_string())
}

/// 纯函数：算出「为什么用不了」。给 `status()` 和起桥前的预检共用同一份判据。
pub fn blockers_for(node: Option<&str>, running: bool) -> Vec<String> {
    let mut b = Vec::new();
    if node.is_none() {
        b.push("这台机器上没找到 Node（装任意一个 AI 工具会自动装便携版）".into());
    }
    if !running {
        b.push("桥没在跑（还没启动，或 U-King 重启过）".into());
    }
    b
}

/// 端口上到底有没有一个**我们的**桥在应答。
///
/// 只连端口不够 —— 15723 上蹲着别的程序也会连得上。这里真发一个 `GET /health`，
/// 认响应里的 `"bridge":"anthropic->openai"`：**别把「端口被占」当成「桥活着」**。
fn health_ok() -> bool {
    let addr = format!("127.0.0.1:{PROXY_PORT}");
    let Ok(sa) = addr.parse() else { return false };
    let Ok(mut s) = TcpStream::connect_timeout(&sa, Duration::from_millis(400)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(400)));
    if s.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.contains("anthropic->openai")
}

pub fn status() -> BridgeStatus {
    let node = crate::installer::find_node().map(|p| p.display().to_string());
    let running = health_ok();
    let blockers = blockers_for(node.as_deref(), running);
    BridgeStatus {
        running,
        port: PROXY_PORT,
        base_url: base_url(),
        upstream: String::new(),
        node,
        ready: blockers.is_empty(),
        blockers,
        runs_only_while_app_open: true,
    }
}

/// 起桥。`openai_base` = 供应商的 OpenAI 端点；`key` 留空则由 Claude Code 自己带
/// （桥会转发请求头里的 `ANTHROPIC_AUTH_TOKEN`，少一份 Key 副本）。
///
/// **幂等**：已经在跑且上游没变就直接返回；换了上游要重起（env 只在进程启动时读一次）。
pub fn start(openai_base: &str, key: &str, model: &str) -> Result<BridgeStatus, String> {
    let upstream = upstream_for(openai_base)?;
    let node = crate::installer::find_node()
        .ok_or("没找到 Node（便携 Node 未装？装任意一个 AI 工具会自动装）")?;

    // 换上游必须重起，所以无条件先停干净，再等端口释放，避免 EADDRINUSE。
    stop()?;
    if health_ok() {
        std::thread::sleep(Duration::from_millis(300));
    }

    let script = script_path();
    if let Some(d) = script.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("建目录失败: {e}"))?;
    }
    std::fs::write(&script, PROXY_SCRIPT).map_err(|e| format!("写桥脚本失败: {e}"))?;

    let mut c = std::process::Command::new(&node);
    c.arg(&script)
        .env("UKING_CLAUDE_PROXY_PORT", PROXY_PORT.to_string())
        .env("UKING_CLAUDE_UPSTREAM", &upstream)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());
    // Key 只在客户端不带的时候才由我们注入 —— 默认不进 env（少一处泄漏面）。
    if !key.trim().is_empty() {
        c.env("UKING_CLAUDE_PROXY_KEY", key.trim());
    }
    if !model.trim().is_empty() {
        c.env("UKING_CLAUDE_PROXY_MODEL", model.trim());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = c.spawn().map_err(|e| format!("起桥进程失败: {e}"))?;
    if let Ok(mut g) = running().lock() {
        *g = Some(child);
    }

    // 等它真的应答再报成功 —— 「spawn 成功」不等于「桥能用」（端口被占、脚本报错都在这之后）。
    for _ in 0..40 {
        if health_ok() {
            let mut st = status();
            st.upstream = upstream;
            return Ok(st);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stop()?;
    Err(format!("桥起来了但 4 秒内没应答 127.0.0.1:{PROXY_PORT}/health（端口被占？）"))
}

/// 停桥。**按我们自己持有的子进程句柄杀**，绝不按镜像名 —— `taskkill /IM node.exe`
/// 会把客户机上所有 Node 进程一起端掉（宪法：绝不按裸镜像名结束进程）。
pub fn stop() -> Result<(), String> {
    let mut g = running().lock().map_err(|_| "锁失败".to_string())?;
    if let Some(mut ch) = g.take() {
        let _ = ch.kill();
        let _ = ch.wait();
    }
    Ok(())
}

// ———————————————— 前端入口 ————————————————
// 照 `codex_proxy` 的样子把 command 定义在模块内：这几个都不碰 `AppHandle`，
// 放这儿 lib.rs 只需登记三行，删这个模块也只动 lib.rs + 前端两处（宪法第 12 条）。

#[tauri::command]
pub fn claude_bridge_status() -> BridgeStatus {
    status()
}

#[tauri::command]
pub fn claude_bridge_start(
    openai_base: String,
    key: Option<String>,
    model: Option<String>,
) -> Result<BridgeStatus, String> {
    start(&openai_base, key.as_deref().unwrap_or(""), model.as_deref().unwrap_or(""))
}

#[tauri::command]
pub fn claude_bridge_stop() -> Result<(), String> {
    stop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_normalizes_and_refuses_to_guess() {
        assert_eq!(upstream_for("https://x.com/v1").unwrap(), "https://x.com/v1/chat/completions");
        assert_eq!(upstream_for("https://x.com").unwrap(), "https://x.com/v1/chat/completions");
        assert_eq!(upstream_for("https://x.com/v1/").unwrap(), "https://x.com/v1/chat/completions");
        assert_eq!(
            upstream_for("https://x.com/v1/chat/completions").unwrap(),
            "https://x.com/v1/chat/completions"
        );
        // 🔴 空端点必须报错而不是回退到某个默认 —— 这座桥是给客户自带中转用的，
        // 猜一个端点等于把他的请求发去别人家。
        assert!(upstream_for("").is_err());
        assert!(upstream_for("   ").is_err());
    }

    #[test]
    fn blockers_say_which_half_is_missing() {
        assert_eq!(blockers_for(Some("C:/node.exe"), true), Vec::<String>::new());
        assert_eq!(blockers_for(Some("C:/node.exe"), false).len(), 1);
        // 两个都缺就得两条都说，别只报第一条让客户修一次再撞一次
        assert_eq!(blockers_for(None, false).len(), 2);
    }

    /// 端口是常量，别跟 codex 那座桥撞（撞了后起的那个 EADDRINUSE，症状是「桥起不来」）。
    #[test]
    fn port_does_not_collide_with_codex_bridge() {
        assert_ne!(PROXY_PORT, crate::codex_proxy::PROXY_PORT);
        assert!(base_url().ends_with(&PROXY_PORT.to_string()));
    }
}
