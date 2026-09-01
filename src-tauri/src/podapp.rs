//! 泊舟 AI 小程序（PodApp）—— 在 U-King 里下载、打开、更新它。
//!
//! ## 它是什么，以及 U-King 为什么只做「入口」
//! PodApp 是**独立的桌面应用**（`dongsheng123132/podapp`，产品名「泊舟 AI 小程序」），
//! 自己发版、自带 Tauri updater。U-King 内置的那几个小程序是试点，PodApp 才是主线。
//! 所以这里**不复制它的更新逻辑** —— 造第二套升级机制，两边版本判据迟早漂移
//! （宪法第 8 条：同一事实存在几份就会漂移几份）。U-King 只做三件事：
//! 装上 / 打开 / 告诉你有没有新版。真正的自动更新由 PodApp 自己完成。
//!
//! ## 🔴 国内可达性（这条是「自动升级」能不能成立的关键）
//! PodApp 的 updater 配了三个端点，但 **2026-07-30 实测国内三条全断**：
//!   ① `podapp.net/latest.json`           → Vercel 托管，国内裸网打不开
//!   ② `u-claw.org.cn/podapp/latest.json` → **404，从没部署过**
//!   ③ GitHub releases                     → 国内经常拉不动
//! ② 是唯一能救的一条（同机 nginx，`.cn` 域不在 GFW 名单）—— 发布时必须把
//! `latest.json` + 安装包一起放上去，PodApp 自带的更新才会在国内真的转起来。
//! 本模块下载时也按同样的优先级选源：国内可达的排最前，GitHub 垫底。
//!
//! ## 独立可插拔
//! 只暴露纯函数，不碰 `AppHandle`；进度用 `|msg|` 回调传出，`lib.rs` 再 emit。
//! 不 import 其它功能模块，只用 `installer` 的公共助手（curl / system_tool）。
//! 删本模块 = 去 `lib.rs` 的 mod+command+动作登记 + 前端那张卡片。

use std::path::PathBuf;

/// `latest.json` 的取源优先级：**国内可达的排最前**。
/// 与 PodApp 自己 updater 里的 endpoints 同源同序 —— 两边看到的「最新版」必须是同一个，
/// 否则 U-King 说有新版、PodApp 自己却查不到，客户就卡在中间。
const MANIFEST_URLS: &[&str] = &[
    "https://u-claw.org.cn/podapp/latest.json",
    "https://podapp.net/latest.json",
    "https://github.com/dongsheng123132/podapp/releases/latest/download/latest.json",
];

/// 产品名（NSIS 装出来的目录名 / 卸载表 DisplayName 都用它）。
const PRODUCT: &str = "泊舟 AI 小程序";
/// 下载页（装不上时让用户自己去下）。
pub const RELEASES_PAGE: &str = "https://github.com/dongsheng123132/podapp/releases/latest";

/// latest.json 里我们要的部分。
pub struct Manifest {
    pub version: String,
    pub url: String,
    pub notes: String,
    /// 实际取到清单的那个源（排障时要知道走的哪条路）
    pub source: String,
}

#[cfg(windows)]
fn curl_out(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new(crate::installer::system_tool("curl"))
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(not(windows))]
fn curl_out(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("curl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// 按优先级拉 `latest.json`。任一源拿到**且能解析出 windows 安装包 url** 才算数 ——
/// 只判 HTTP 200 会被落地页/错误页骗到（`u-claw.org.cn/podapp/` 现在就是 404 页）。
pub fn latest_manifest() -> Result<Manifest, String> {
    let mut last = String::new();
    for u in MANIFEST_URLS {
        // 🔴 **单源预算必须塞得进动作自报的 timeout_ms**（`runtime.podapp.inspect` 声明 20s）。
        // 这里一度是 `-m 25` —— 一个源就比整个动作的预算还长，三个源最坏 75s。
        // 干净机实测：没装泊舟、三个源都拉不动 → `ms=20007`，conformance 判 fail。
        // 而「拉不到清单」本来就不是错误（离线/被墙很正常，下面 `.ok()` 已经如实报 latest=null），
        // 它却因为超时把整个只读动作拖红了。5s 连接 + 6s 总时长 ×3 源 = 最坏 18s，留 2s 余量。
        let Some(body) = curl_out(&[
            "-sSL", "-A", "Mozilla/5.0 U-King",
            "--connect-timeout", "5", "-m", "6",
            u,
        ]) else {
            last = format!("{u} 拉不到");
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
            last = format!("{u} 返回的不是 JSON（多半是落地页/错误页）");
            continue;
        };
        let version = v.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
        // Tauri updater 的 target 键；U-King 目前只发 Windows 版 PodApp。
        let url = v
            .pointer("/platforms/windows-x86_64/url")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if version.is_empty() || url.is_empty() {
            last = format!("{u} 的清单里没有 windows 安装包");
            continue;
        }
        return Ok(Manifest {
            version,
            url,
            notes: v.get("notes").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            source: (*u).to_string(),
        });
    }
    Err(if last.is_empty() { "取不到 PodApp 版本清单".into() } else { last })
}

/// 已装的 PodApp（可执行文件路径 + 版本）。
///
/// 🔴 **别只按目录名猜**：UU远程 那次就是猜错一层目录（官方装在 `Netease\GameViewer`），
/// 导致装成功了却报「没装」、客户装两遍。所以这里**卸载表优先**（Windows 认定「装没装」
/// 的权威处，还顺带给出版本号），目录探测只当兜底。
#[cfg(windows)]
pub fn installed() -> Option<(PathBuf, String)> {
    // ① 卸载表：Tauri NSIS 的键名可能是 identifier 或 productName，两个都试；
    //    三个 hive 都扫（perMachine 装在 HKLM，currentUser 装在 HKCU）。
    for hive in [
        "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ] {
        for key in ["org.podapp.dock", PRODUCT] {
            let full = format!("{hive}\\{key}");
            let Some(out) = reg_query(&full) else { continue };
            let ver = reg_value(&out, "DisplayVersion").unwrap_or_default();
            let loc = reg_value(&out, "InstallLocation").unwrap_or_default();
            let exe = if loc.is_empty() {
                None
            } else {
                let p = PathBuf::from(&loc).join(format!("{PRODUCT}.exe"));
                p.is_file().then_some(p)
            };
            if let Some(e) = exe {
                return Some((e, ver));
            }
            // 注册表有登记但 InstallLocation 空/对不上 → 继续走目录兜底，但版本先记下
            if !ver.is_empty() {
                if let Some(p) = probe_dirs() {
                    return Some((p, ver));
                }
            }
        }
    }
    // ② 目录兜底（Tauri NSIS 默认：currentUser→%LOCALAPPDATA%\<产品名>，perMachine→Program Files）
    probe_dirs().map(|p| {
        let v = exe_version(&p).unwrap_or_default();
        (p, v)
    })
}

#[cfg(windows)]
fn probe_dirs() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let roots = [
        PathBuf::from(&local),
        PathBuf::from(&local).join("Programs"),
        PathBuf::from(&pf),
    ];
    for r in &roots {
        let p = r.join(PRODUCT).join(format!("{PRODUCT}.exe"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// `reg query <key>` 的原始输出（键不存在返回 None）。
#[cfg(windows)]
fn reg_query(key: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new(crate::installer::system_tool("reg"))
        .args(["query", key])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// 从 `reg query` 输出里取某个值（形如 `    DisplayVersion    REG_SZ    0.1.1`）。
///
/// 🔴 **必须剥掉包裹的双引号**：2026-07-30 干净机实测，PodApp 的 NSIS 把 `InstallLocation`
/// 写成了 `"C:\...\泊舟 AI 小程序"` —— **带引号存进注册表**。不剥的话 `PathBuf` 会拿到一个以
/// `"` 开头的非法路径，`is_file()` 恒 false → 装好了照样报「没装」→ 客户重复安装。
/// 这是 UU远程 那次「装了却报没装」的同款故障，换了个成因（那次是目录少一层，这次是引号）。
#[cfg(windows)]
fn reg_value(out: &str, name: &str) -> Option<String> {
    for line in out.lines() {
        let t = line.trim();
        // 用 `starts_with(name)` 会让 `Display` 误命中 `DisplayVersion`；要求后面紧跟空白。
        let Some(rest) = t.strip_prefix(name) else { continue };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        for ty in ["REG_EXPAND_SZ", "REG_SZ"] {
            if let Some(v) = rest.strip_prefix(ty) {
                let v = v.trim();
                // 剥掉成对的包裹引号（只在首尾都有时剥，别把值里的引号吃掉）
                let v = v
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                    .unwrap_or(v)
                    .trim()
                    .trim_end_matches('\\');
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// 读 exe 的文件版本（注册表没给版本时兜底）。
#[cfg(windows)]
fn exe_version(exe: &PathBuf) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let ps = format!(
        "(Get-Item -LiteralPath '{}').VersionInfo.FileVersion",
        exe.display().to_string().replace('\'', "''")
    );
    let out = std::process::Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(not(windows))]
pub fn installed() -> Option<(PathBuf, String)> {
    None
}

/// `a < b`？逐段比数字（版本号里非数字段按 0 处理，够用且不引 semver 依赖）。
fn version_lt(a: &str, b: &str) -> bool {
    let seg = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split(['.', '-', '+'])
            .map(|x| x.parse::<u32>().unwrap_or(0))
            .collect()
    };
    let (x, y) = (seg(a), seg(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p < q;
        }
    }
    false
}

/// 状态：装没装 / 已装版本 / 最新版 / 有没有新版 / 能不能用。
///
/// **`ready` 回答的是「现在能不能用泊舟小程序」= 装了就能**，不是「我们能不能替你装」
/// （后者是 `can_auto_install`，非 Windows 为 false）。这两件事分开，是 UU远程 那次
/// 定下的约定：混成一个字段会得出「Mac 用不了」这种错结论。
pub fn status() -> serde_json::Value {
    let inst = installed();
    let (path, cur) = match &inst {
        Some((p, v)) => (p.display().to_string(), v.clone()),
        None => (String::new(), String::new()),
    };
    // 取不到清单不是错误（离线/被墙也很正常）—— 如实报 latest=null，别把没网说成没新版。
    let m = latest_manifest().ok();
    let latest = m.as_ref().map(|x| x.version.clone());
    let update_available = match (&inst, &latest) {
        (Some((_, v)), Some(l)) => !v.is_empty() && version_lt(v, l),
        _ => false,
    };
    let mut blockers: Vec<String> = Vec::new();
    if inst.is_none() {
        blockers.push(if cfg!(windows) {
            "还没装泊舟 AI 小程序（点「装到本机」）".to_string()
        } else {
            "还没装泊舟 AI 小程序，且当前平台不支持一键装 —— 请到发布页自行下载".to_string()
        });
    }
    serde_json::json!({
        "installed": inst.is_some(),
        "ready": inst.is_some(),
        "blockers": blockers,
        "version": cur,
        "latest": latest,
        "update_available": update_available,
        "exe_path": path,
        "can_auto_install": cfg!(windows),
        "releases_page": RELEASES_PAGE,
        // 清单是从哪条源拿到的 —— 国内客户报「更新不了」时，这一条最能说明问题
        "manifest_source": m.as_ref().map(|x| x.source.clone()),
        "notes": m.as_ref().map(|x| x.notes.clone()).unwrap_or_default(),
    })
}

/// 下载 + 静默安装 PodApp（NSIS `/S`）。已是最新则跳过（幂等）。
///
/// 装完之后的**升级不归这里管** —— PodApp 自己的 updater 会做。这个函数只保证
/// 「从没有到有」和「手动点一下更到最新」，不常驻、不后台轮询。
#[cfg(windows)]
pub fn install(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let m = latest_manifest()?;
    on_progress(&format!("最新版 {}（源：{}）", m.version, m.source));

    // 幂等：已装且不比最新旧，就别再下一遍。
    if let Some((p, v)) = installed() {
        if !v.is_empty() && !version_lt(&v, &m.version) {
            on_progress(&format!("已是最新版 {v}，跳过下载。"));
            return Ok(format!("泊舟 AI 小程序已是最新版 {v}。在「打开」即可使用。({})", p.display()));
        }
        on_progress(&format!("已装 {v}，正在更新到 {}…", m.version));
    }

    let tmp = std::env::temp_dir().join("PodApp-Setup-uking.exe");
    let _ = std::fs::remove_file(&tmp);
    on_progress("开始下载泊舟 AI 小程序（约 2 MB）…");
    let st = std::process::Command::new(crate::installer::system_tool("curl"))
        .args([
            "-sSL", // GitHub 直链会 302 到 CDN，必须跟随
            "-A",
            "Mozilla/5.0 U-King",
            "-m",
            "300",
            "-o",
            &tmp.to_string_lossy(),
            &m.url,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("启动下载失败: {e}"))?;
    let sz = std::fs::metadata(&tmp).map(|x| x.len()).unwrap_or(0);
    // 下限 500KB：源被墙/代理拦时会回几 KB 的错误页，HTTP 200 但内容是垃圾，
    // 直接拿去执行就是最难查的「双击没反应」（video/UU远程 都踩过同一类）。
    if !st.success() || sz < 500_000 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "下载失败（只拿到 {sz} 字节，源 {}）。可以点「打开发布页」自己下。",
            m.source
        ));
    }
    on_progress("下载完成，正在安装…");

    // Tauri NSIS：`/S` 是标准静默旗标（bundle.targets=["nsis"] 实证）。
    // 但仍**只认「探测到装上了 + 版本对得上」**才算成功 —— 判据是机器状态，
    // 不是「我发了 /S 所以应该装上了」。
    match std::process::Command::new(&tmp).arg("/S").creation_flags(CREATE_NO_WINDOW).spawn() {
        Ok(mut child) => {
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if let Some((p, v)) = installed() {
                    if v.is_empty() || !version_lt(&v, &m.version) {
                        let _ = std::fs::remove_file(&tmp);
                        return Ok(format!(
                            "泊舟 AI 小程序 {} 已装好，点「打开」即可用。({})",
                            m.version,
                            p.display()
                        ));
                    }
                }
            }
            // 还没探到。**绝不在旧安装进程还活着时再拉一个**（install_clawx 修过的
            // 「装着装着又弹出一个安装窗」，UU远程 那次也是同款）。
            if matches!(child.try_wait(), Ok(None)) {
                on_progress("仍在后台安装（慢盘/杀软扫描会拖慢），装完在开始菜单能找到。");
                return Ok("泊舟 AI 小程序正在后台安装，稍等一会儿即可。".into());
            }
            if installed().is_some() {
                let _ = std::fs::remove_file(&tmp);
                return Ok(format!("泊舟 AI 小程序 {} 已装好，点「打开」即可用。", m.version));
            }
            on_progress("自动安装没成功，已打开官方安装界面，按提示点「下一步」即可。");
            let _ = std::process::Command::new(&tmp).spawn();
            Ok("已为你打开泊舟 AI 小程序安装程序，按提示装完即可。".into())
        }
        Err(_) => {
            std::process::Command::new(&tmp)
                .spawn()
                .map_err(|e| format!("启动安装程序失败: {e}"))?;
            Ok("已打开泊舟 AI 小程序安装程序，按提示安装即可。".into())
        }
    }
}

#[cfg(not(windows))]
pub fn install(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请点「打开发布页」自行下载泊舟 AI 小程序".into())
}

/// 启动已装的 PodApp（它是常驻窄条 dock，起来后贴在屏幕边上）。
pub fn launch() -> Result<String, String> {
    let (exe, _) = installed().ok_or_else(|| "还没装泊舟 AI 小程序".to_string())?;
    let dir = exe.parent().map(|p| p.to_path_buf());
    let mut c = std::process::Command::new(&exe);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.spawn().map_err(|e| format!("启动失败: {e}"))?;
    Ok("泊舟 AI 小程序已启动（它是贴在屏幕边上的窄条）。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_lt("0.1.0", "0.1.1"));
        assert!(version_lt("0.1.1", "0.2.0"));
        assert!(!version_lt("0.1.1", "0.1.1"));
        assert!(!version_lt("0.2.0", "0.1.9"));
        // 段数不齐要按 0 补，别把 1.0 判成比 1.0.1 新
        assert!(version_lt("1.0", "1.0.1"));
        assert!(!version_lt("1.0.1", "1.0"));
        // 带 v 前缀 / 预发布后缀不该炸
        assert!(version_lt("v0.1.0", "0.1.1"));
    }

    /// 国内可达的源必须排在最前 —— 这条顺序错了，国内客户就一直去够 Vercel/GitHub。
    #[test]
    fn china_reachable_source_first() {
        assert!(MANIFEST_URLS[0].contains("u-claw.org.cn"));
        assert!(MANIFEST_URLS.last().unwrap().contains("github.com"));
    }

    /// 用 2026-07-30 干净机上 `reg query` 的**实测原文**当夹具。
    /// 这段里 `InstallLocation` 带引号 —— 就是「装好了却报没装」的那颗雷。
    #[cfg(windows)]
    #[test]
    fn reg_value_strips_wrapping_quotes() {
        let out = "\r\nHKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\泊舟 AI 小程序\r\n    DisplayName    REG_SZ    泊舟 AI 小程序\r\n    DisplayVersion    REG_SZ    0.1.1\r\n    InstallLocation    REG_SZ    \"C:\\Users\\x\\AppData\\Local\\泊舟 AI 小程序\"\r\n";
        assert_eq!(reg_value(out, "DisplayVersion").as_deref(), Some("0.1.1"));
        // 关键断言：拿到的必须是干净路径，不带引号
        assert_eq!(
            reg_value(out, "InstallLocation").as_deref(),
            Some("C:\\Users\\x\\AppData\\Local\\泊舟 AI 小程序")
        );
        // `DisplayName` 不能被 `DisplayVersion` 抢走（前缀匹配必须要求后面跟空白）
        assert_eq!(reg_value(out, "DisplayName").as_deref(), Some("泊舟 AI 小程序"));
        assert!(reg_value(out, "NoSuchValue").is_none());
    }

    /// 尾部反斜杠要去掉，否则 join 出 `...\\泊舟 AI 小程序\\\\x.exe`。
    #[cfg(windows)]
    #[test]
    fn reg_value_trims_trailing_slash() {
        let out = "    InstallLocation    REG_SZ    \"C:\\App\\\"\r\n";
        assert_eq!(reg_value(out, "InstallLocation").as_deref(), Some("C:\\App"));
    }
}
