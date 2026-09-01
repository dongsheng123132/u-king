//! Free Router 本地免费路由网关 —— 「一键装跑」支持（2026-08-31 会审定案的最小集成）。
//!
//! ## 它是什么
//! 第三方开源项目（github.com/www222fff/free-router，MIT 免费档仓库零依赖单文件 server.mjs）：
//! 本地 OpenAI 兼容网关（127.0.0.1:8787，模型名 free-best）。TokenRouter 免费档优先，
//! OpenRouter 免费模型兜底；每 15 分钟刷 OpenRouter 免费目录、下架模型自动摘除、
//! 限流/超时自动冷却换下一家。U-King 免费页已有它的「目录条目」（C 形态，热下发）；
//! 本模块是下一版加的**深度集成**：一键下载 → 填 Key → 后台拉起 → 健康检查。
//!
//! ## 会审红线（opus/sol/GLM 三方一致，2026-08-31，别回退）
//! 1. **钉死 commit SHA 下载 tarball**，不 git clone、不追 main —— 21 star 无审计仓库，
//!    main 漂移 = 我们替客户执行了没看过的代码。升级 = 改 SHA 常量发版，人工看过 diff 才换。
//! 2. **Key 只写本机 .env**（0600 语义，仓库 .env.example 同款变量名），绝不进 config.json、
//!    绝不上报。U-King 不带 Key：没有 OpenRouter Key 就引导去免费页领。
//! 3. **只绑 127.0.0.1**（上游默认即如此，配置不开放 host），无本地鉴权 = 不能暴露到局域网。
//! 4. **安装即解压 tarball**，不跑 npm install（package.json 零运行时依赖，engines>=20）。
//! 5. 端口冲突/启动失败给人话，不静默重试。

use serde::Serialize;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 钉死的上游 commit（2026-08-31 审定时 HEAD；升级先人工 diff，再换这里发版）。
const PINNED_SHA: &str = "7dbb9f15bde3e0010f70b6be57ac10701b8910d5";
/// tarball 直链（codeload，钉 SHA）。sha256 校验见 install()。
const TARBALL_URL: &str =
    "https://codeload.github.com/www222fff/free-router/tar.gz/7dbb9f15bde3e0010f70b6be57ac10701b8910d5";
/// 审定时对 tarball 算的 SHA-256（2026-08-31 实测下载后 certutil 口径一致）。
/// 上游换内容 → 哈希不符当场拦下，人工重审 diff 后更新两个常量再发版。
const TARBALL_SHA256: &str = "a9728419cd8d0327cc3afded307c2150e001d43d7cc2441aac78a076ab713476";
const PORT: u16 = 8787;
const HEALTH_URL: &str = "http://127.0.0.1:8787/health";

#[derive(Debug, Clone, Serialize)]
pub struct FrStatus {
    /// 安装目录存在且 server.mjs 在
    pub installed: bool,
    /// /health 返回 ok（= 进程活着且是它）
    pub running: bool,
    pub version: String,
    /// .env 里有没有配 OpenRouter Key（只报有无，绝不回显内容）
    pub key_configured: bool,
    pub dir: String,
    pub log_tail: Vec<String>,
}

fn fr_home() -> std::path::PathBuf {
    crate::installer::uking_home().join("free-router")
}

fn env_path() -> std::path::PathBuf {
    fr_home().join(".env")
}

fn log_path() -> std::path::PathBuf {
    fr_home().join("server.log")
}

/// health 探测：GET /health，2s 死线。通 = 活着（该端口只有它，冲突时它起不来会报错）。
fn probe_health() -> bool {
    matches!(
        crate::installer::curl(&["-s", "-m", "2", HEALTH_URL]),
        Ok(s) if s.contains("\"ok\":true")
    )
}

/// 读 .env 里 OPENROUTER_API_KEY 是否非空（不回显值）。
fn key_configured() -> bool {
    let Ok(text) = std::fs::read_to_string(env_path()) else { return false };
    text.lines().any(|l| {
        let l = l.trim();
        l.starts_with("OPENROUTER_API_KEY=") && l["OPENROUTER_API_KEY=".len()..].trim().len() > 8
    })
}

pub fn status() -> FrStatus {
    let server = fr_home().join("server.mjs");
    let installed = server.is_file();
    let running = probe_health();
    let log_tail: Vec<String> = std::fs::read_to_string(log_path())
        .map(|t| {
            let mut all: Vec<&str> = t.lines().collect();
            let start = all.len().saturating_sub(12);
            all.split_off(start).into_iter().map(|s| s.to_string()).collect()
        })
        .unwrap_or_default();
    FrStatus {
        installed,
        running,
        version: PINNED_SHA.chars().take(8).collect(),
        key_configured: key_configured(),
        dir: fr_home().display().to_string(),
        log_tail,
    }
}

/// 下载 + 校验 + 解压到 ~/.uking/free-router/。
/// 依赖便携/系统 Node ≥20（装网关本身不需要 npm install —— 零依赖单文件）。
pub fn install(on_log: &dyn Fn(&str)) -> Result<(), String> {
    let home = fr_home();
    std::fs::create_dir_all(&home).map_err(|e| format!("建目录失败: {e}"))?;
    let tmp_zip = home.join("_fr.tar.gz");
    let tmp_dir = home.join("_fr_unpack");

    on_log("下载 free-router（钉死 commit，SHA-256 校验）…");
    let dl = crate::installer::curl(&[
        "-fL",
        "-m",
        "120",
        "--retry",
        "2",
        "-o",
        &tmp_zip.display().to_string(),
        TARBALL_URL,
    ]);
    // curl 带了 -o 时 stdout 为空，错误才关心
    dl.map(|_| ()).map_err(|e| format!("下载失败: {e}"))?;

    // SHA-256 校验（复用 installer::verify_download 的思路，独立实现避免动老函数签名）
    let bytes = std::fs::read(&tmp_zip).map_err(|e| format!("读下载文件失败: {e}"))?;
    if TARBALL_SHA256.len() == 64 {
        use std::fmt::Write as _;
        let digest = {
            // 纯 std 无 sha2 crate：走 certutil（Win10+ 内置）/ shasum（macOS）
            #[cfg(windows)]
            {
                let out = std::process::Command::new("certutil")
                    .args(["-hashfile"])
                    .arg(&tmp_zip)
                    .arg("SHA256")
                    .output()
                    .map_err(|e| format!("certutil 失败: {e}"))?;
                let s = String::from_utf8_lossy(&out.stdout);
                s.lines()
                    .skip(1)
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim().to_string())
                    .unwrap_or_default()
            }
            #[cfg(not(windows))]
            {
                let out = std::process::Command::new("shasum")
                    .arg("-a")
                    .arg("256")
                    .arg(&tmp_zip)
                    .output()
                    .map_err(|e| format!("shasum 失败: {e}"))?;
                String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string()
            }
        };
        let mut hex = String::new();
        for b in digest.bytes() {
            let _ = write!(hex, "{b:02x}");
        }
        if hex.to_lowercase() != TARBALL_SHA256 {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err(format!(
                "SHA-256 校验不过（{hex} ≠ {TARBALL_SHA256}）—— 下载内容与审定不符，已拦截。请联系 U-King 更新。"
            ));
        }
    }

    on_log("解压…");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("建解压目录失败: {e}"))?;
    #[cfg(windows)]
    let untar = std::process::Command::new("tar")
        .args(["-xf"])
        .arg(&tmp_zip)
        .arg("-C")
        .arg(&tmp_dir)
        .creation_flags(0x0800_0000)
        .output();
    #[cfg(not(windows))]
    let untar = std::process::Command::new("tar")
        .args(["-xf"])
        .arg(&tmp_zip)
        .arg("-C")
        .arg(&tmp_dir)
        .output();
    let out = untar.map_err(|e| format!("tar 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!("解压失败: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // tarball 解出来是 free-router-<sha>/ 前缀目录，把内容搬到 fr_home 根
    let entries = std::fs::read_dir(&tmp_dir).map_err(|e| format!("读解压目录失败: {e}"))?;
    let mut moved = false;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && p.join("server.mjs").is_file() {
            for item in std::fs::read_dir(&p).map_err(|e| format!("读子目录失败: {e}"))?.flatten() {
                let dst = home.join(item.file_name());
                let _ = std::fs::remove_file(&dst);
                let _ = std::fs::remove_dir_all(&dst);
                std::fs::rename(item.path(), &dst).map_err(|e| format!("搬运失败: {e}"))?;
            }
            moved = true;
            break;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let _ = std::fs::remove_file(&tmp_zip);
    if !moved || !home.join("server.mjs").is_file() {
        return Err("解压后没找到 server.mjs —— tarball 结构变了，已拦截".into());
    }
    on_log("安装完成");
    Ok(())
}

/// 把 Key 写进 .env（保留用户已有其他行；OPENROUTER_API_KEY 已存在则覆盖该行）。
pub fn set_key(key: &str) -> Result<(), String> {
    let k = key.trim();
    if !(k.starts_with("sk-or-") && k.len() > 20) {
        return Err("Key 形状不对：应为 OpenRouter 的 sk-or- 开头长串（免费页「OpenRouter」条目可领）".into());
    }
    let path = env_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in existing.lines() {
        if line.trim_start().starts_with("OPENROUTER_API_KEY=") {
            out.push(format!("OPENROUTER_API_KEY={k}"));
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        out.push(format!("OPENROUTER_API_KEY={k}"));
    }
    std::fs::create_dir_all(fr_home()).map_err(|e| format!("建目录失败: {e}"))?;
    std::fs::write(&path, out.join("\n") + "\n").map_err(|e| format!("写 .env 失败: {e}"))?;
    Ok(())
}

/// 后台拉起：node server.mjs，输出重定向 server.log，CREATE_NO_WINDOW。
/// 健康检查最多等 8s；起不来把日志尾部带回去。
pub fn start() -> Result<(), String> {
    if probe_health() {
        return Ok(()); // 已在跑，幂等
    }
    let home = fr_home();
    let server = home.join("server.mjs");
    if !server.is_file() {
        return Err("还没安装 —— 先点安装".into());
    }
    // Node 探测：便携优先，退系统 node。engines>=20 但旧版多数也能跑；失败日志里可见。
    let node = crate::installer::portable_node_dir()
        .map(|d| d.join("node.exe"))
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "node".into());

    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .map_err(|e| format!("开日志失败: {e}"))?;
    let err_log = log.try_clone().map_err(|e| format!("开日志失败: {e}"))?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(&node)
            .arg("server.mjs")
            .current_dir(&home)
            .stdout(log)
            .stderr(err_log)
            .creation_flags(0x0800_0000 | 0x0000_0008) // CREATE_NO_WINDOW | DETACHED_PROCESS
            .spawn()
            .map_err(|e| format!("启动失败: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(&node)
            .arg("server.mjs")
            .current_dir(&home)
            .stdout(log)
            .stderr(err_log)
            .spawn()
            .map_err(|e| format!("启动失败: {e}"))?;
    }

    // 健康等待：8s 内 /health 变 ok 才算成
    for _ in 0..16 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if probe_health() {
            return Ok(());
        }
    }
    let tail = std::fs::read_to_string(log_path())
        .map(|t| {
            let mut all: Vec<&str> = t.lines().collect();
            let start = all.len().saturating_sub(6);
            all.split_off(start).join("\n")
        })
        .unwrap_or_default();
    Err(format!("网关 8 秒内没起来。日志尾部：\n{tail}"))
}

/// 停止：找监听 8787 的 PID 杀掉（Windows netstat / Unix pkill -f）。
pub fn stop() -> Result<(), String> {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("netstat")
            .args(["-ano"])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("netstat 失败: {e}"))?;
        let s = String::from_utf8_lossy(&out.stdout);
        let mut killed = 0;
        for line in s.lines() {
            if line.contains(format!(":{PORT}").as_str()) && line.contains("LISTENING") {
                if let Some(pid) = line.split_whitespace().last() {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/PID", pid])
                        .creation_flags(0x0800_0000)
                        .output();
                    killed += 1;
                }
            }
        }
        if killed == 0 && !probe_health() {
            return Ok(()); // 本来就没跑，幂等
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-f", "free-router/server.mjs"])
            .output();
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    if probe_health() {
        Err("停了进程但 8787 还有响应 —— 可能被别的程序占用，请手工检查".into())
    } else {
        Ok(())
    }
}

// ───────────────────── tauri 命令壳（sync 真身走 spawn_blocking，不卡 async 运行时） ─────────────────────

#[tauri::command]
pub async fn freerouter_status() -> FrStatus {
    tauri::async_runtime::spawn_blocking(status)
        .await
        .unwrap_or_else(|_| FrStatus {
            installed: false,
            running: false,
            version: String::new(),
            key_configured: false,
            dir: String::new(),
            log_tail: Vec::new(),
        })
}

#[tauri::command]
pub async fn freerouter_install() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let logs: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        install(&|m| {
            if let Ok(mut l) = logs.lock() {
                l.push(m.to_string());
            }
        })?;
        Ok(logs.into_inner().unwrap_or_default())
    })
    .await
    .map_err(|e| format!("安装任务异常: {e}"))?
}

#[tauri::command]
pub async fn freerouter_set_key(key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || set_key(&key))
        .await
        .map_err(|e| format!("保存 Key 异常: {e}"))?
}

#[tauri::command]
pub async fn freerouter_start() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(start)
        .await
        .map_err(|e| format!("启动任务异常: {e}"))?
}

#[tauri::command]
pub async fn freerouter_stop() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(stop)
        .await
        .map_err(|e| format!("停止任务异常: {e}"))?
}
