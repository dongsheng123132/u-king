//! 浏览器运行时适配层 —— 影核 `browser.*` 动作的后端。
//!
//! 当前后端：**agent-browser** CLI（Vercel，Windows 原生，npm 全局安装）。
//! 2026-08-06 实测（Windows 11）：snapshot 0.45s 返回带 @ref 的交互树；
//! fill/select/check/click 每动作 0.4~0.5s；6 字段表单全流程 ~3s，字段值验证正确。
//! 实测坑：daemon 僵死时命令会挂 30~50s（10060）→ 本模块自带超时 kill；
//! `--profile` 复用 Chrome 登录态在 Windows 上实测未生效（cookie DPAPI 跨实例读不了），
//! 登录态方案留给 onboarding 迁移向导（见 docs/浏览器工作台-设计稿.md）。
//!
//! 后端可替换：未来 ego (lite)（登录态继承 + 语义快照）或自研 WebView 面板落地后，
//! 只改本文件的 `run_ab` 调度，`browser.*` 动作契约与 lib.rs 登记不动。
//!
//! 依赖方向（宪法）：本模块不认识 `actions.rs`，动作登记在组合根 `lib.rs::action_table()`。

use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

// ─────────────────────── 动作 ID（对外契约，别乱改） ───────────────────────

/// 打开 URL（导航）。read 类，无外部副作用。
pub const BROWSER_OPEN: &str = "browser.open";
/// 页面快照（交互树，带 @ref）。默认只取交互元素，省 token。
pub const BROWSER_SNAPSHOT: &str = "browser.snapshot";
/// 取页面值：文本 / 属性 / URL / 标题。
pub const BROWSER_GET: &str = "browser.get";
/// 点击元素（页面内交互：导航、展开、选择）。**有外部副作用的提交必须用 `browser.submit`**。
pub const BROWSER_CLICK: &str = "browser.click";
/// 提交表单 / 执行有外部副作用的操作（发帖、下单、删除）。确认门必开。
pub const BROWSER_SUBMIT: &str = "browser.submit";
/// 清空并填充输入框。
pub const BROWSER_FILL: &str = "browser.fill";
/// 选择下拉项。
pub const BROWSER_SELECT: &str = "browser.select";
/// 勾选复选框。
pub const BROWSER_CHECK: &str = "browser.check";
/// 截图（可指定保存路径）。
pub const BROWSER_SCREENSHOT: &str = "browser.screenshot";
/// 后退（页面内导航，无外部副作用）。
pub const BROWSER_BACK: &str = "browser.back";
/// 前进（页面内导航）。
pub const BROWSER_FORWARD: &str = "browser.forward";
/// 刷新当前页。
pub const BROWSER_RELOAD: &str = "browser.reload";
/// 底层鼠标操作（move/down/up/wheel）—— 画布/地图/无 @ref 的元素用。
pub const BROWSER_MOUSE: &str = "browser.mouse";
/// 在视口坐标 (x,y) 处点击（合成 move+down+up）。坐标映射由调用方（面板）负责。
pub const BROWSER_CLICKAT: &str = "browser.clickat";
/// 键盘输入文本（打到当前聚焦的元素）。
pub const BROWSER_TYPE: &str = "browser.type";
/// 按键（Enter/Tab/…）。
pub const BROWSER_PRESS: &str = "browser.press";
/// 滚动（up/down/left/right + px）。
pub const BROWSER_SCROLL: &str = "browser.scroll";
/// 列出标签页。
pub const BROWSER_TABS: &str = "browser.tabs";
/// 浏览器会话直播流信息（ws:// 地址）—— 面板连这个看实时画面。只读，无副作用。
pub const BROWSER_STREAM: &str = "browser.stream";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 定位 agent-browser 可执行文件。
/// Windows：npm 全局装在 `%APPDATA%\npm\node_modules\agent-browser\bin\agent-browser-win32-x64.exe`。
/// macOS：brew 装在 PATH（`agent-browser`）。
fn ab_exe() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let p = std::path::Path::new(&appdata)
                .join("npm/node_modules/agent-browser/bin/agent-browser-win32-x64.exe");
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = std::path::Path::new(&local)
                .join("npm/node_modules/agent-browser/bin/agent-browser-win32-x64.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    // 兜底：PATH 里找（macOS / 手动安装）
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join("agent-browser");
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    None
}

/// 没装 agent-browser 时给调用方的提示。
///
/// **只留真正必需的那半条命令。** 提示语属于对外契约的一部分：写多一步，
/// 客户就会照着跑多一步，而那一步恰好是会失败的那步。
const NOT_INSTALLED_HINT: &str = "not_installed: 没找到 agent-browser。装它：`npm install -g agent-browser`。\
它会直接用这台机器上已装的 Chrome，**不用再跑 `agent-browser install`** —— 那一步是从 Google 下载 \
Chrome for Testing（约 340MB），国内裸网常被重置，而装了 Chrome 的机器压根不需要它。\
没有 Chrome 就先装 Chrome（`runtime.toolbox.inspect` 里有这件厨具）。";

/// 跑一条 agent-browser 命令。自带超时 kill（daemon 僵死实测会挂 30~50s）。
fn run_ab(args: &[&str], timeout_ms: u64) -> Result<String, String> {
    let exe = ab_exe().ok_or_else(|| {
        // 🔴 **别再教人跑 `agent-browser install`。** 那一步是从
        // `googlechromelabs.github.io` 下 Chrome for Testing（~340MB），也正是
        // 「裸网国内机器间歇被重置（10054）」那条已知限制的唯一来源（需求榜第 8 条）。
        //
        // 而它**在已经装了 Chrome 的机器上根本不需要** —— agent-browser 会自动探测
        // 系统里的 Chrome/Brave/Playwright/Puppeteer 并直接用（上游 README 明写）。
        // 2026-08-16 干净 HOME 实测（macOS，系统 Chrome 151）：
        //   doctor → `Chrome pass … /Applications/Google Chrome.app` + `Launch test pass`
        //   open → get title → snapshot（带 @ref 树）→ close 全部 rc=0
        //   全程 `~/.agent-browser` 只有 24KB、`browsers/` 目录始终不存在 = 一个字节都没下
        // 所以提示语只留真正必需的那半条，把「装 Chrome」作为前提说清楚
        // （`runtime.toolbox.inspect` 本来就在管 Chrome 这件厨具）。
        NOT_INSTALLED_HINT.to_string()
    })?;
    let mut cmd = Command::new(&exe);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child =
        cmd.spawn().map_err(|e| format!("browser runtime 启动失败: {e}"))?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let _status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(e) => return Err(format!("wait 失败: {e}")),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timeout: agent-browser 无响应（daemon 僵死？重试或 agent-browser close）".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let out = child
        .wait_with_output()
        .map_err(|e| format!("读输出失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let joined = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    if !out.status.success() {
        // 原始信息透传，让 actions.rs 的 ERR_RULES 分类（timeout/network/not_installed…）
        return Err(format!("{}", joined.trim()));
    }
    Ok(joined)
}

/// 从快照/输出里提取标题（首行 "✓ xxx" 或 "xxx"）。
fn first_line(s: &str) -> String {
    s.lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('✓')
        .trim()
        .to_string()
}

/// 确保 agent-browser daemon 活着并返回直播流端口。
///
/// 面板连 `browser.stream` 拿这个 ws:// 地址看实时画面。任何 agent-browser 命令都会拉起
/// daemon，而 daemon 默认总是开着流（"Streaming is always enabled"），所以先读 `stream status`
/// 就行；读不到才 `stream enable`。刻意不用 `open` 当 kicker —— 那会把 AI 正在看的页面导航走。
fn ensure_stream() -> Result<u64, String> {
    // ① 读当前状态（daemon 没起时这条会失败，走 ② 拉起）
    if let Ok(status) = run_ab(&["stream", "status", "--json"], 15_000) {
        if let Ok(v) = serde_json::from_str::<Value>(&status) {
            if v["success"].as_bool().unwrap_or(false) {
                let port = v["data"]["port"].as_u64().unwrap_or(0);
                if port > 0 {
                    return Ok(port);
                }
            }
        }
    }
    // ② 拉起并显式开流
    let out = run_ab(&["stream", "enable", "--json"], 15_000)?;
    let v: Value = serde_json::from_str(&out)
        .map_err(|_| "browser.stream: 解析 stream enable 结果失败".to_string())?;
    let port = v["data"]["port"].as_u64().unwrap_or(0);
    if port == 0 {
        return Err("browser.stream: 浏览器会话未就绪".into());
    }
    Ok(port)
}

/// 影核 `browser.*` 动作统一分发（lib.rs 的 handler 全部指到这里）。
pub fn run(id: &str, input: Value, _p: &(dyn Fn(&str) + Send + Sync)) -> Result<Value, String> {
    let s = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let r = |k: &str| {
        let v = s(k);
        if v.is_empty() {
            return Err(format!("invalid_input: {k} 必填"));
        }
        Ok(v)
    };
    match id {
        BROWSER_OPEN => {
            let url = r("url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("file://") {
                return Err("invalid_input: url 必须是 http(s):// 或 file://".into());
            }
            let out = run_ab(&["open", &url], 60_000)?;
            Ok(json!({ "ok": true, "title": first_line(&out), "url": url }))
        }
        BROWSER_SNAPSHOT => {
            let interactive = input.get("interactive").and_then(|v| v.as_bool()).unwrap_or(true);
            let args = if interactive { vec!["snapshot", "-i"] } else { vec!["snapshot"] };
            let out = run_ab(&args, 30_000)?;
            Ok(json!({ "ok": true, "snapshot": out }))
        }
        BROWSER_GET => {
            let what = r("what")?;
            let selector = r("selector")?;
            let out = run_ab(&["get", &what, &selector], 30_000)?;
            Ok(json!({ "ok": true, "value": out.trim() }))
        }
        BROWSER_CLICK => {
            let rf = r("ref")?;
            let out = run_ab(&["click", &rf], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_SUBMIT => {
            // 语义：有外部副作用的提交。确认门由 lib.rs 的 confirmation="required" 强制。
            let rf = r("ref")?;
            let out = run_ab(&["click", &rf], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_FILL => {
            let rf = r("ref")?;
            let text = s("text");
            if text.is_empty() && input.get("text").is_none() {
                return Err("invalid_input: text 必填".into());
            }
            let out = run_ab(&["fill", &rf, &text], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_SELECT => {
            let rf = r("ref")?;
            let val = r("value")?;
            let out = run_ab(&["select", &rf, &val], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_CHECK => {
            let rf = r("ref")?;
            let out = run_ab(&["check", &rf], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_SCREENSHOT => {
            let path = s("path");
            let out = if path.is_empty() {
                run_ab(&["screenshot"], 30_000)?
            } else {
                run_ab(&["screenshot", &path], 30_000)?
            };
            Ok(json!({ "ok": true, "path": first_line(&out) }))
        }
        BROWSER_BACK => {
            let out = run_ab(&["back"], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_FORWARD => {
            let out = run_ab(&["forward"], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_RELOAD => {
            let out = run_ab(&["reload"], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_MOUSE => {
            let action = r("action")?;
            let x = input.get("x").and_then(|v| v.as_u64()).unwrap_or(0);
            let y = input.get("y").and_then(|v| v.as_u64()).unwrap_or(0);
            let dx = input.get("dx").and_then(|v| v.as_i64()).unwrap_or(0);
            let dy = input.get("dy").and_then(|v| v.as_i64()).unwrap_or(0);
            let x_s = x.to_string();
            let y_s = y.to_string();
            let dx_s = dx.to_string();
            let dy_s = dy.to_string();
            let args: Vec<&str> = match action.as_str() {
                "move" => vec!["mouse", "move", &x_s, &y_s],
                "down" => vec!["mouse", "down"],
                "up" => vec!["mouse", "up"],
                "wheel" => vec!["mouse", "wheel", &dy_s, &dx_s],
                _ => return Err("invalid_input: mouse action 必须是 move/down/up/wheel".into()),
            };
            let out = run_ab(&args, 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_CLICKAT => {
            let x = input
                .get("x")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "invalid_input: x 必填（视口坐标）".to_string())?;
            let y = input
                .get("y")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "invalid_input: y 必填（视口坐标）".to_string())?;
            let x_s = x.to_string();
            let y_s = y.to_string();
            // move → down → up 合成一次点击（agent-browser 没有坐标版 click）
            let _ = run_ab(&["mouse", "move", &x_s, &y_s], 30_000)?;
            let _ = run_ab(&["mouse", "down"], 30_000)?;
            let last = run_ab(&["mouse", "up"], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&last) }))
        }
        BROWSER_TYPE => {
            let text = r("text")?;
            let out = run_ab(&["keyboard", "type", &text], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_PRESS => {
            let key = r("key")?;
            let out = run_ab(&["press", &key], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_SCROLL => {
            let dir = r("direction")?;
            let px = input.get("px").and_then(|v| v.as_u64()).unwrap_or(100);
            let px_s = px.to_string();
            let out = run_ab(&["scroll", &dir, &px_s], 30_000)?;
            Ok(json!({ "ok": true, "result": first_line(&out) }))
        }
        BROWSER_TABS => {
            let out = run_ab(&["tab", "list"], 30_000)?;
            Ok(json!({ "ok": true, "tabs": out }))
        }
        BROWSER_STREAM => {
            let port = ensure_stream()?;
            Ok(json!({
                "ok": true,
                "ws_url": format!("ws://127.0.0.1:{}", port),
                "port": port,
            }))
        }
        _ => Err(format!("unknown_action: {id}")),
    }
}
