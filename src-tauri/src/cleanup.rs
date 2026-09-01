//! 安全卸载 / 逐项清理 —— 诚实列出 U-King 在本机留下的**全部足迹**，逐项可删、可还原配置，
//! 或一键彻底卸载。修「现在的卸载是假的」：旧 `uninstall.rs` 只删 `~/.uking`，把 U-King 真正
//! 推到机器上的东西全留下了 —— 写进 Claude/Codex/ClawX/Hermes 的驱动配置、拷进各工具的技能包、
//! MCP 连接器、Codex 省钱代理、PATH shims，以及帮客户装的 AI 工具/厨具本体。
//!
//! ## 三档（前端按 `group` 分区、`safe` 决定默认是否勾选）
//! - `core`   ：U-King 自己装的（`~/.uking` + 技能包 + 系统集成）—— safe，默认勾。
//! - `config` ：U-King 改过的**别人的**配置（Claude/Codex/ClawX/Hermes 驱动 + MCP + Codex 代理）
//!              —— safe，默认勾；**清除 = 还原到改动前**（走 `*.uking-bak` 备份 / 移除我们写的键），
//!              不是清空 `~/.claude`。
//! - `aitool` ：U-King **帮你装的** AI 工具/厨具本体（Claude Code/Codex/ClawX/Hermes/Ollama/ffmpeg…）
//!              —— **unsafe**，可能你之前就有，默认不勾、每项单独确认、附「你之前就有请勿删」。
//!
//! ## 铁律仍在
//! `config` 档删 = 还原（不清空用户目录）；`aitool` 档默认不勾、逐项确认，且一律走对方**官方卸载
//! 程序 / 包管理器**（`npm uninstall` / `winget uninstall` / `Uninstall*.exe /S`），**绝不 rm -rf
//! 系统目录**。`~/.uking` 本体（`uking-home`）的删除仍走 `uninstall.rs` 的「等本进程退出 → 删目录
//! → 自删」延迟脚本，由 lib.rs 编排（本模块只报告，不在这里删 home）。
//!
//! ## 独立可插拔（横切「撤销中枢」型）
//! 本模块按**允许方向**（新模块 → 老公共助手）读取 `providers` / `mcp` / `codex_proxy` /
//! `context_menu` / `tools` / `toolbox` / `installer` / `uninstall` 的公开撤销 API，不反向被依赖。
//! `#[tauri::command]` 全在 lib.rs 转调（本模块不碰 `AppHandle`，进度用 `|msg|` 回调）。删它只动
//! lib.rs（去 mod + 两个 command）和 `Advanced.tsx`（去这一段面板）。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 一条足迹（前端一行勾选项）。
#[derive(Serialize, Clone)]
pub struct FootprintItem {
    /// 稳定 id（remove 按它分派）
    pub id: String,
    /// 分组：`core` | `config` | `aitool`
    pub group: String,
    /// 显示名
    pub name: String,
    /// 位置 / 说明（让用户看清删的是什么）
    pub detail: String,
    /// true = U-King 自己的（默认勾选）；false = 可能是你的（默认不勾、附警告）
    pub safe: bool,
    /// 非空时前端标黄提示
    pub warn: String,
}

impl FootprintItem {
    fn new(id: &str, group: &str, name: &str, detail: String, safe: bool, warn: &str) -> Self {
        FootprintItem { id: id.into(), group: group.into(), name: name.into(), detail, safe, warn: warn.into() }
    }
}

// ———————————————— 路径助手 ————————————————

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
fn uking_home() -> PathBuf {
    home_dir().join(".uking")
}
fn file_has(p: PathBuf, needle: &str) -> bool {
    std::fs::read_to_string(&p).map(|s| s.contains(needle)).unwrap_or(false)
}

// ———————————————— 演示清场前的用户数据保留 ————————————————

/// 为「安装 → 卸载 → 再安装」录屏保留用户资料。
///
/// 不把资料留在原安装目录（否则下一次安装会被误判为“已经安装”）；而是备份到用户“文档”
/// 下的独立时间戳目录。这样演示可从干净状态重新开始，历史数据也不会丢。
/// Claude/Codex 的主用户目录本来就不会被本卸载器删除；这里额外保护 U-King、OpenClaw、
/// ClawX、Hermes 和 Codex 桌面版的本地历史目录。
pub fn archive_demo_user_data(on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let root = home_dir()
        .join("Documents")
        .join("U-King 演示保留数据")
        .join(format!("backup-{stamp}"));
    std::fs::create_dir_all(&root).map_err(|e| format!("创建历史数据备份目录失败: {e}"))?;

    let home = home_dir();
    let mut sources: Vec<(String, PathBuf)> = vec![
        ("U-King-任务记录".into(), uking_home().join("tasks.json")),
        ("U-King-作图历史".into(), uking_home().join("draw")),
        ("U-King-视频历史".into(), uking_home().join("video")),
        ("OpenClaw-工作数据".into(), home.join(".uclaw")),
    ];
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            sources.push(("ClawX-用户数据".into(), PathBuf::from(appdata).join("ClawX")));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            sources.push(("Hermes-用户数据".into(), local.join("hermes")));
            let packages = local.join("Packages");
            if let Ok(entries) = std::fs::read_dir(packages) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("OpenAI.Codex_") {
                        let package = entry.path();
                        sources.push((format!("Codex桌面版-{name}-LocalState"), package.join("LocalState")));
                        sources.push((format!("Codex桌面版-{name}-RoamingState"), package.join("RoamingState")));
                    }
                }
            }
        }
    }

    let mut copied = 0u64;
    let mut kept = 0usize;
    for (name, source) in sources {
        if !source.exists() {
            continue;
        }
        on_log(&format!("正在保留用户数据：{name}"));
        copied += copy_archive_path(&source, &root.join(name), on_log)?;
        kept += 1;
    }
    if kept == 0 {
        let _ = std::fs::remove_dir(&root);
        return Ok("未发现需要单独备份的历史数据".into());
    }
    Ok(format!("已保留 {kept} 份用户数据（{} MB）：{}", copied / 1_000_000, root.display()))
}

/// 递归复制历史资料；跳过可再生缓存和 node_modules，避免备份运行时、拖慢录制流程。
fn copy_archive_path(
    source: &Path,
    destination: &Path,
    on_log: &(dyn Fn(&str) + Send + Sync),
) -> Result<u64, String> {
    let meta = std::fs::symlink_metadata(source)
        .map_err(|e| format!("读取历史数据失败（{}）: {e}", source.display()))?;
    if meta.file_type().is_symlink() {
        return Ok(0);
    }
    if meta.is_file() {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建备份目录失败: {e}"))?;
        }
        std::fs::copy(source, destination)
            .map_err(|e| format!("备份 {} 失败: {e}", source.display()))?;
        return Ok(meta.len());
    }
    if !meta.is_dir() {
        return Ok(0);
    }
    std::fs::create_dir_all(destination).map_err(|e| format!("创建备份目录失败: {e}"))?;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(source).map_err(|e| format!("读取目录失败（{}）: {e}", source.display()))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_ascii_lowercase();
        if matches!(lower.as_str(), "node_modules" | "cache" | "code cache" | "gpucache" | "logs") {
            continue;
        }
        bytes += copy_archive_path(&entry.path(), &destination.join(name), on_log)?;
    }
    if bytes > 100_000_000 {
        on_log(&format!("已备份 {:.0} MB…", bytes as f64 / 1_000_000.0));
    }
    Ok(bytes)
}

/// U-King 拷进各 AI 工具的技能包目录。scan 与 remove 共用，只返回**当前存在**的
/// （精确匹配本 app 装的那几个包名，不用 `uking-*` 泛匹配）。
///
/// 🔴 **目录表不在这里** —— 一律问 `skillpack::all_skill_dirs_on_disk()` 要。
/// 此前这里自己硬编码了一份，跟 `skillpack::install_into_tools()` 那份漂了：
/// 装进 `~/.codex/skills`、`~/.agents/skills` 的包扫不到，Hermes 那条扫的还是已知的错落点。
/// 宪法第 8 条 —— 同一事实存在几份就会漂移几份。
fn skill_dirs() -> Vec<PathBuf> {
    crate::skillpack::all_skill_dirs_on_disk()
}

/// 我们插进**别家 AI 记忆文件**的指针块，外加撤销后留下的 `*.uking-bak`。
///
/// 🔴 为什么必须列在这张表里（2026-08-18 客户反馈：「一来就乱加 agentme claude.me，
/// 要能删，真删」）：撤销能力其实**早就有**（`identity::unlink_in`，沙箱实测能逐字节还原、
/// 用户自己写的内容一个字节不动），入口也有（「我的 U-King」页每个目标一个开关）。
/// 但客户找的是**这张「列出 U-King 在这台电脑上留下的全部东西」的表** —— 表里没有，
/// 他就有充分理由认为删不掉。**能力存在 ≠ 客户找得到**，而这张表的承诺是「全部」。
///
/// 另外 `unlink` 完了 `CLAUDE.md.uking-bak` 会留在原地没人收 —— 那也是我们的足迹。
fn identity_files() -> Vec<PathBuf> {
    // 🔴 用 `identity::home_dir()`（= 公共层 `installer::user_home_dir()`，**认 UKING_TEST_HOME**），
    // **不要**用本文件的 `home_dir()` —— 那个直接读 USERPROFILE。identity.rs:50 的注释记着这个坑：
    // 别家 AI 的记忆文件一旦逃出沙箱，一次「隔离测试」就能改到用户真实的 CLAUDE.md。
    // 本函数只读，但下面 remove 分支的 `unlink_in` 是**真删**，两处必须同一个 home。
    let home = crate::identity::home_dir();
    let mut v = Vec::new();
    // 「哪些落点当前挂着指针」问 identity 自己要（`discovery_in` 的 `linked` 字段），
    // 别在这儿重新读文件找标记 —— 标记常量是它的私有实现，复制一份就是等着漂（宪法 8）。
    for d in crate::identity::discovery_in(&home) {
        if d.get("linked").and_then(|x| x.as_bool()) == Some(true) {
            if let Some(p) = d.get("path").and_then(|x| x.as_str()) {
                v.push(PathBuf::from(p));
            }
        }
    }
    // 撤销后留在原地没人收的备份（identity.rs:744 固定拼 `.md.uking-bak`）
    for t in crate::identity::link_targets_in(&home) {
        let bak = t.path.with_extension("md.uking-bak");
        if bak.exists() {
            v.push(bak);
        }
    }
    v
}

fn shortcut_exists() -> bool {
    #[cfg(windows)]
    {
        if let Ok(h) = std::env::var("USERPROFILE") {
            return Path::new(&h).join("Desktop").join("U-King.lnk").exists();
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(h) = std::env::var("HOME") {
            let l = Path::new(&h).join("Desktop").join("U-King.app");
            return l.exists() || std::fs::symlink_metadata(&l).is_ok();
        }
    }
    false
}

/// 用户 PATH 里有没有指向 `~/.uking` 的目录（便携 node/git/python/shims）。
#[cfg(windows)]
fn has_uking_path() -> bool {
    let needle = uking_home().display().to_string().to_lowercase();
    let out = Command::new(crate::installer::system_tool("reg"))
        .args(["query", "HKCU\\Environment", "/v", "Path"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&needle)).unwrap_or(false)
}
#[cfg(not(windows))]
fn has_uking_path() -> bool {
    if let Ok(h) = std::env::var("HOME") {
        let needle = uking_home().display().to_string();
        if let Ok(txt) = std::fs::read_to_string(Path::new(&h).join(".zshrc")) {
            return txt.lines().any(|l| l.contains("export PATH") && l.contains(&needle));
        }
    }
    false
}

fn ollama_installed() -> bool {
    #[cfg(windows)]
    if let Ok(la) = std::env::var("LOCALAPPDATA") {
        if PathBuf::from(la).join("Programs").join("Ollama").join("ollama.exe").exists() {
            return true;
        }
    }
    let exe = if cfg!(windows) { "ollama.exe" } else { "ollama" };
    crate::installer::search_paths(None).iter().any(|d| d.join(exe).exists())
}

// ———————————————— 配置足迹探测（U-King 有没有改过某工具的配置）————————————————

fn claude_configured() -> bool {
    let p = home_dir().join(".claude").join("settings.json");
    std::fs::read_to_string(&p)
        .map(|s| s.contains("ANTHROPIC_BASE_URL") && (s.contains("u-claw") || s.contains("u-king")))
        .unwrap_or(false)
}
fn codex_driver_configured() -> bool {
    file_has(home_dir().join(".codex").join("config.toml"), "u-claw")
}
fn codex_proxy_on() -> bool {
    file_has(home_dir().join(".codex").join("config.toml"), "uking_deepseek")
}
fn clawx_configured() -> bool {
    #[cfg(windows)]
    if let Ok(ad) = std::env::var("APPDATA") {
        return file_has(PathBuf::from(ad).join("ClawX").join("clawx-providers.json"), "u-claw");
    }
    false
}
fn hermes_configured() -> bool {
    #[cfg(windows)]
    if let Ok(la) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(la).join("hermes");
        return file_has(base.join(".env"), "u-claw")
            || file_has(base.join("auth.json"), "u-claw")
            || file_has(base.join("config.yaml"), "u-claw");
    }
    #[cfg(not(windows))]
    if let Ok(h) = std::env::var("HOME") {
        let base = PathBuf::from(h).join(".hermes");
        return file_has(base.join(".env"), "u-claw") || file_has(base.join("auth.json"), "u-claw");
    }
    false
}

/// 已启用的精选 MCP 连接器（`claude mcp list` ∩ CURATED）—— 只认 U-King 装的这几个，
/// 绝不动用户自己加的 MCP server。返回展示名。
fn curated_mcp() -> Vec<String> {
    let installed = crate::mcp::list_installed();
    crate::mcp::CURATED
        .iter()
        .filter(|c| installed.iter().any(|n| n == c.id))
        .map(|c| c.name.to_string())
        .collect()
}

// ———————————————— 扫描 ————————————————

/// 扫描本机上 U-King 的全部足迹（只返回**当前探测到**的项，前端直接渲染）。
/// 一次扫描要问的全部「有没有」。**先并行问完，再按固定顺序拼列表** ——
/// 拼装顺序和判断逻辑跟串行版一字不差，所以输出可以逐字节对得上。
struct Probes {
    skills: Vec<PathBuf>,
    /// 我们往**别家 AI 的记忆文件**（~/.claude/CLAUDE.md、~/.codex/AGENTS.md、~/AGENTS.md）
    /// 插过指针块的那些文件，外加撤销后留下的 `*.uking-bak`。
    identity_files: Vec<PathBuf>,
    shortcut: bool,
    ctx_menu: bool,
    uking_path: bool,
    claude_cfg: bool,
    codex_cfg: bool,
    clawx_cfg: bool,
    hermes_cfg: bool,
    mcp: Vec<String>,
    codex_proxy: bool,
    tool_claude: bool,
    tool_codex: bool,
    tool_openclaw: bool,
    tool_hermes: bool,
    tool_dsh: bool,
    tool_harness_doctor: bool,
    codex_app: bool,
    clawx_app: bool,
    hermes_app: bool,
    ollama: bool,
    uuswitch: bool,
    market: Vec<crate::tools::ToolInfo>,
    kits: Vec<crate::toolbox::ToolStatus>,
}

/// 并行跑完所有探针。
///
/// 为什么值得并行：这些探针**全是在等子进程**，不是在算。实测单项耗时
/// `claude mcp list` 3625ms、`claude --version` 1991ms、`Get-AppxPackage` 1315ms、
/// `hermes --version` 751ms…… 串起来就是「安全卸载」页 5.5 秒的白等。
/// 它们彼此无关且全部只读，同时问完即可，总耗时收敛到**最慢的那一个**。
///
/// 用 `std::thread::scope`：不引第三方 crate（体积优先），也不需要 Arc/Mutex ——
/// 每条线程只写自己那一份结果。
fn probe_all() -> Probes {
    std::thread::scope(|s| {
        let skills = s.spawn(skill_dirs);
        let identity_files = s.spawn(identity_files);
        let shortcut = s.spawn(shortcut_exists);
        let ctx_menu = s.spawn(crate::context_menu::is_registered);
        let uking_path = s.spawn(has_uking_path);
        let claude_cfg = s.spawn(claude_configured);
        let codex_cfg = s.spawn(codex_driver_configured);
        let clawx_cfg = s.spawn(clawx_configured);
        let hermes_cfg = s.spawn(hermes_configured);
        let mcp = s.spawn(curated_mcp);
        let codex_proxy = s.spawn(codex_proxy_on);
        // 四个 CLI 各起一条 `--version`，同时问。
        let tool_claude = s.spawn(|| crate::installer::tool_installed("claude"));
        let tool_codex = s.spawn(|| crate::installer::tool_installed("codex"));
        let tool_openclaw = s.spawn(|| crate::installer::tool_installed("openclaw"));
        let tool_hermes = s.spawn(|| crate::installer::tool_installed("hermes"));
        let tool_dsh = s.spawn(|| crate::installer::tool_installed("dsh"));
        let tool_harness_doctor = s.spawn(|| crate::installer::tool_installed("harness-doctor"));
        let codex_app = s.spawn(|| {
            #[cfg(windows)]
            { crate::installer::codex_app_installed() }
            #[cfg(not(windows))]
            { false }
        });
        let clawx_app = s.spawn(|| {
            #[cfg(windows)]
            { crate::providers::clawx_app_installed() }
            #[cfg(not(windows))]
            { false }
        });
        let hermes_app = s.spawn(|| {
            #[cfg(windows)]
            { crate::tools::find_hermes_app_exe().is_some() }
            #[cfg(not(windows))]
            { false }
        });
        let ollama = s.spawn(ollama_installed);
        let uuswitch = s.spawn(|| {
            #[cfg(windows)]
            { crate::uuswitch::installed() }
            #[cfg(not(windows))]
            { false }
        });
        let market = s.spawn(|| {
            #[cfg(windows)]
            { crate::tools::list_tools() }
            #[cfg(not(windows))]
            { Vec::new() }
        });
        let kits = s.spawn(crate::toolbox::list_tools);

        // 任何一条线程 panic 都只吞掉它自己那一项（取默认值），不连累整份清单 ——
        // 「安全卸载」页宁可少列一项也不该整页打不开。
        Probes {
            skills: skills.join().unwrap_or_default(),
            identity_files: identity_files.join().unwrap_or_default(),
            shortcut: shortcut.join().unwrap_or(false),
            ctx_menu: ctx_menu.join().unwrap_or(false),
            uking_path: uking_path.join().unwrap_or(false),
            claude_cfg: claude_cfg.join().unwrap_or(false),
            codex_cfg: codex_cfg.join().unwrap_or(false),
            clawx_cfg: clawx_cfg.join().unwrap_or(false),
            hermes_cfg: hermes_cfg.join().unwrap_or(false),
            mcp: mcp.join().unwrap_or_default(),
            codex_proxy: codex_proxy.join().unwrap_or(false),
            tool_claude: tool_claude.join().unwrap_or(false),
            tool_codex: tool_codex.join().unwrap_or(false),
            tool_openclaw: tool_openclaw.join().unwrap_or(false),
            tool_hermes: tool_hermes.join().unwrap_or(false),
            tool_dsh: tool_dsh.join().unwrap_or(false),
            tool_harness_doctor: tool_harness_doctor.join().unwrap_or(false),
            codex_app: codex_app.join().unwrap_or(false),
            clawx_app: clawx_app.join().unwrap_or(false),
            hermes_app: hermes_app.join().unwrap_or(false),
            ollama: ollama.join().unwrap_or(false),
            uuswitch: uuswitch.join().unwrap_or(false),
            market: market.join().unwrap_or_default(),
            kits: kits.join().unwrap_or_default(),
        }
    })
}

pub fn scan() -> Vec<FootprintItem> {
    let p = probe_all();
    let mut v: Vec<FootprintItem> = Vec::new();

    // —— core：U-King 自己装的 ——
    if uking_home().exists() {
        v.push(FootprintItem::new(
            "uking-home",
            "core",
            "U-King 本体与便携运行时",
            format!(
                "{}（便携 Node/Git/Python、技能包源、作图/视频历史、设备 Key、shims）",
                uking_home().display()
            ),
            true,
            "清除会关闭 U-King 来完成清理；之后不装到本地/不插 U 盘将无法使用",
        ));
    }
    let skills = &p.skills;
    if !skills.is_empty() {
        let d = skills.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("；");
        v.push(FootprintItem::new(
            "skills-in-tools",
            "core",
            "拷进各 AI 工具的技能包",
            format!("{} 个目录：{}", skills.len(), d),
            true,
            "",
        ));
    }
    // 别家 AI 记忆文件里的指针块 + 撤销后留下的备份。
    // 🔴 这一项以前不在表里，于是客户在「列出全部足迹」的页面上找不到它、判断成「删不掉」——
    // 而撤销能力其实一直都在（identity::unlink_in）。**能力存在 ≠ 客户找得到。**
    if !p.identity_files.is_empty() {
        let d = p.identity_files.iter().map(|x| x.display().to_string()).collect::<Vec<_>>().join("；");
        v.push(FootprintItem::new(
            "identity-pointers",
            "core",
            "写进别家 AI 记忆文件的指针",
            format!("{} 处：{}", p.identity_files.len(), d),
            true,
            "清除只摘掉我们那段带标记的块并删掉备份；你自己写在这些文件里的内容一个字节都不动",
        ));
    }
    {
        let mut parts: Vec<&str> = Vec::new();
        if p.shortcut {
            parts.push("桌面快捷方式");
        }
        if p.ctx_menu {
            parts.push("右键菜单");
        }
        if p.uking_path {
            parts.push("用户 PATH 项");
        }
        if !parts.is_empty() {
            v.push(FootprintItem::new("system-integration", "core", "系统集成", parts.join(" · "), true, ""));
        }
    }

    // —— config：U-King 改过的别人配置（删=还原到改前）——
    if p.claude_cfg {
        v.push(FootprintItem::new(
            "config-claude",
            "config",
            "Claude Code 驱动配置",
            "~/.claude/settings.json 里 U-King 写的端点/Key（清除=还原官方直连，其它设置不动）".into(),
            true,
            "",
        ));
    }
    if p.codex_cfg {
        v.push(FootprintItem::new(
            "config-codex",
            "config",
            "Codex 驱动配置",
            "~/.codex/config.toml + auth.json（清除=回滚 *.uking-bak 备份）".into(),
            true,
            "",
        ));
    }
    if p.clawx_cfg {
        v.push(FootprintItem::new(
            "config-clawx",
            "config",
            "ClawX 驱动配置",
            "clawx-providers.json 里 U-King 写的虾盘云供应商（清除=移除该供应商）".into(),
            true,
            "",
        ));
    }
    if p.hermes_cfg {
        v.push(FootprintItem::new(
            "config-hermes",
            "config",
            "Hermes 驱动配置",
            "Hermes 里 U-King 写的凭据（清除=回滚备份/移除注入）".into(),
            true,
            "",
        ));
    }
    let mcp = &p.mcp;
    if !mcp.is_empty() {
        v.push(FootprintItem::new("mcp", "config", "MCP 连接器（Claude Code）", mcp.join(" · "), true, ""));
    }
    if p.codex_proxy {
        v.push(FootprintItem::new(
            "codex-proxy",
            "config",
            "Codex 省钱本地代理",
            "端口 15722（清除=停代理并还原 Codex 直连）".into(),
            true,
            "",
        ));
    }

    // —— aitool：U-King 帮你装的工具/厨具本体（默认不勾、谨慎）——
    let warn_tool = "U-King 帮你装的；若你之前就自己装过，清除会真的卸载它——不确定就别勾";
    if p.tool_claude {
        v.push(FootprintItem::new(
            "tool-claude",
            "aitool",
            "Claude Code（命令行）",
            "npm 全局包 @anthropic-ai/claude-code".into(),
            false,
            warn_tool,
        ));
    }
    if p.tool_codex {
        v.push(FootprintItem::new(
            "tool-codex",
            "aitool",
            "Codex CLI（命令行）",
            "npm 全局包 @openai/codex".into(),
            false,
            warn_tool,
        ));
    }
    if p.tool_openclaw {
        v.push(FootprintItem::new(
            "tool-openclaw",
            "aitool",
            "OpenClaw CLI（原版）",
            "npm 全局包 openclaw（含 ~/.uclaw 与 U-King 残留）".into(),
            false,
            warn_tool,
        ));
    }
    if p.tool_hermes {
        v.push(FootprintItem::new(
            "tool-hermes-cli",
            "aitool",
            "Hermes Agent（命令行）",
            "便携 Python 中的 hermes-agent 包与 U-King 残留".into(),
            false,
            warn_tool,
        ));
    }
    if p.tool_dsh {
        v.push(FootprintItem::new(
            "tool-dsh",
            "aitool",
            "DeepSeek Harness（命令行）",
            "npm 全局包 @deepseek-ai/dsh".into(),
            false,
            warn_tool,
        ));
    }
    if p.tool_harness_doctor {
        v.push(FootprintItem::new(
            "tool-harness-doctor",
            "aitool",
            "Harness Doctor（AI 工具体检）",
            "npm 全局包 harness-doctor".into(),
            false,
            warn_tool,
        ));
    }
    if p.codex_app {
        v.push(FootprintItem::new(
            "tool-codex-app",
            "aitool",
            "Codex 桌面版",
            "Microsoft Store / MSIX 应用 OpenAI.Codex".into(),
            false,
            warn_tool,
        ));
    }
    if p.clawx_app {
        v.push(FootprintItem::new(
            "tool-clawx",
            "aitool",
            "ClawX 桌面版",
            "图形版 AI（走 ClawX 官方卸载程序）".into(),
            false,
            warn_tool,
        ));
    }
    if p.hermes_app {
        v.push(FootprintItem::new(
            "tool-hermes",
            "aitool",
            "Hermes 桌面版",
            "Nous 官方图形版（走官方卸载程序）".into(),
            false,
            warn_tool,
        ));
    }
    if p.ollama {
        v.push(FootprintItem::new(
            "tool-ollama",
            "aitool",
            "Ollama（本地大模型引擎）",
            "含已下载的模型（数 GB，走官方卸载程序）".into(),
            false,
            warn_tool,
        ));
    }
    if p.uuswitch {
        v.push(FootprintItem::new(
            "tool-uu-switch",
            "aitool",
            "uu-switch 模型切换器",
            "U-King 安装的 cc-switch / uu-switch 应用（走 MSI 官方卸载）".into(),
            false,
            warn_tool,
        ));
    }
    #[cfg(windows)]
    if uking_home().join("tools").join("open365").exists() {
        v.push(FootprintItem::new(
            "tool-open365",
            "aitool",
            "Open365 电脑管家",
            "U-King 本地工具目录与桌面快捷方式".into(),
            false,
            warn_tool,
        ));
    }
    // Obsidian / UU 远程由官网安装，U-King 只提供下载入口；演示清场也把已装状态列出来，
    // 但执行时仍只调用 Windows 注册表里登记的原始卸载程序，不直接删除安装目录。
    #[cfg(windows)]
    {
        let market = &p.market;
        if market.iter().any(|t| t.id == "obsidian" && t.installed) {
            v.push(FootprintItem::new(
                "tool-obsidian",
                "aitool",
                "Obsidian 知识库",
                "通过 Obsidian 官方卸载程序处理；不会删除你的笔记库文件夹".into(),
                false,
                "官网安装的软件，确认这台演示机没有要保留的资料后再勾选",
            ));
        }
        if market.iter().any(|t| t.id == "uu-remote" && t.installed) {
            v.push(FootprintItem::new(
                "tool-uu-remote",
                "aitool",
                "UU远程（手机控电脑）",
                "通过网易 UU 远程官方卸载程序处理".into(),
                false,
                "官网安装的软件，确认不再需要远控服务后再勾选",
            ));
        }
    }
    for t in &p.kits {
        if t.installed {
            v.push(FootprintItem::new(
                &format!("kit-{}", t.id),
                "aitool",
                &format!("厨具：{}", t.name),
                t.desc.clone(),
                false,
                "系统级工具，可能你其它软件也在用，慎删",
            ));
        }
    }

    v
}

// ———————————————— 逐项删除 ————————————————

/// 删除单条足迹。`uking-home` 在此**只报告不删**（删 home 会关闭 app，由 lib.rs 编排延迟脚本）。
/// best-effort：返回一句人话结果，失败返回 Err 让前端如实展示、继续下一项。
pub fn remove(id: &str, on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    // 逐项清理会真删文件 / 还原别人的配置，**不可逆**。每一项删了什么必须留痕：
    // 这是事后判「是我们删的还是客户自己弄的」的唯一依据，也是被误报时的自证材料。
    crate::ulog::section("cleanup", &format!("清理项 id={id}"));
    let notify = on_log;
    let on_log_wrapped = move |m: &str| {
        crate::ulog::write("cleanup", m);
        notify(m);
    };
    let on_log: &(dyn Fn(&str) + Send + Sync) = &on_log_wrapped;
    match id {
        "uking-home" => Ok("将在「彻底卸载并关闭」时清理".into()),

        "skills-in-tools" => {
            let dirs = skill_dirs();
            if dirs.is_empty() {
                return Ok("没有需要清理的技能包".into());
            }
            let mut n = 0;
            for d in &dirs {
                if std::fs::remove_dir_all(d).is_ok() {
                    n += 1;
                }
            }
            Ok(format!("已删除 {n}/{} 个技能包目录", dirs.len()))
        }

        "identity-pointers" => {
            // 同 identity_files()：必须是认沙箱的那个 home，否则测试会删到用户真实的 CLAUDE.md
            let home = crate::identity::home_dir();
            // 摘指针块交给 identity 自己做 —— 它按标记精确定位，用户内容零改动（有单测守着）
            let changed = crate::identity::unlink_in(&home)?;
            // 备份是我们留的，一并收走（不收就是「清理完还剩一堆 .uking-bak」）
            let mut baks = 0;
            for t in crate::identity::link_targets_in(&home) {
                let bak = t.path.with_extension("md.uking-bak");
                if bak.exists() && std::fs::remove_file(&bak).is_ok() {
                    baks += 1;
                }
            }
            on_log(&format!("已摘除 {} 处指针、删除 {baks} 份备份", changed.len()));
            Ok(format!("已摘除 {} 处指针块、清掉 {baks} 份备份（你自己的内容没动）", changed.len()))
        }

        "system-integration" => {
            crate::uninstall::remove_user_path_entries(on_log);
            crate::uninstall::remove_shortcut(on_log);
            let _ = crate::context_menu::unregister();
            Ok("已清除 PATH 项 / 桌面快捷方式 / 右键菜单".into())
        }

        "config-claude" => restore_cfg("claude"),
        "config-codex" => restore_cfg("codex"),
        "config-clawx" => restore_cfg("clawx"),
        "config-hermes" => restore_cfg("hermes"),

        "mcp" => {
            let installed = crate::mcp::list_installed();
            let mut n = 0;
            let mut errs: Vec<String> = Vec::new();
            for c in crate::mcp::CURATED {
                if installed.iter().any(|x| x == c.id) {
                    match crate::mcp::remove(c.id) {
                        Ok(_) => n += 1,
                        Err(e) => errs.push(format!("{}: {e}", c.id)),
                    }
                }
            }
            if errs.is_empty() {
                Ok(format!("已移除 {n} 个 MCP 连接器"))
            } else {
                Err(errs.join("；"))
            }
        }

        "codex-proxy" => crate::codex_proxy::codex_proxy_stop()
            .map(|_| "已停止 Codex 省钱代理并还原 Codex 直连".to_string()),

        "tool-claude" => npm_uninstall("@anthropic-ai/claude-code", on_log),
        "tool-codex" => {
            let r = npm_uninstall("@openai/codex", on_log);
            // 二进制兜底安装落点：~/bin/codex.exe（skill 的兜底路径）
            if let Ok(h) = std::env::var("USERPROFILE") {
                let _ = std::fs::remove_file(Path::new(&h).join("bin").join("codex.exe"));
            }
            r
        }
        "tool-openclaw" => crate::cleanup::uninstall_ai_tool("openclaw", on_log),
        "tool-hermes-cli" => crate::cleanup::uninstall_ai_tool("hermes", on_log),
        "tool-dsh" => crate::cleanup::uninstall_ai_tool("dsh", on_log),
        "tool-harness-doctor" => crate::cleanup::uninstall_ai_tool("harness-doctor", on_log),
        // Cline（2026-08-29 上架）：npm 全局包 + 328MB，走官方 npm uninstall，不 rm。
        "tool-cline" => npm_uninstall("cline", on_log),

        #[cfg(windows)]
        "tool-codex-app" => uninstall_appx("OpenAI.Codex", on_log),
        #[cfg(windows)]
        "tool-clawx" => uninstall_app(crate::tools::find_clawx_exe(), on_log),
        #[cfg(windows)]
        "tool-hermes" => uninstall_app(crate::tools::find_hermes_app_exe(), on_log),
        "tool-ollama" => uninstall_ollama(on_log),
        #[cfg(windows)]
        "tool-uu-switch" => uninstall_uuswitch(on_log),
        #[cfg(windows)]
        "tool-open365" => uninstall_ai_tool("open365", on_log),
        #[cfg(windows)]
        "tool-obsidian" => uninstall_registered_app(&["Obsidian"], "Obsidian", on_log),
        #[cfg(windows)]
        "tool-uu-remote" => uninstall_registered_app(&["UU远程", "UURemote", "GameViewer"], "UU远程", on_log),

        other if other.starts_with("kit-") => crate::toolbox::uninstall_tool(&other[4..], on_log),

        other => Err(format!("未知清理项: {other}")),
    }
}

/// 还原某工具的驱动配置到 U-King 改动前（走 providers 的「官方直连」路径，含 *.uking-bak 回滚）。
fn restore_cfg(target: &str) -> Result<String, String> {
    crate::providers::apply_provider("official", "", None, &[target.to_string()])
        .map(|_| "已还原到 U-King 改动前的配置".to_string())
}

// ———————————————— aitool 卸载底座（一律走官方卸载程序 / 包管理器）————————————————

/// 在便携 Node + 系统 PATH 里解析可执行文件全路径（Windows npm 是 npm.cmd）。
fn resolve_bin(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let exts: &[&str] = &[".cmd", ".exe", ".bat", ""];
    #[cfg(not(windows))]
    let exts: &[&str] = &[""];
    for dir in crate::installer::search_paths(crate::installer::portable_node_dir().as_deref()) {
        for e in exts {
            let p = dir.join(format!("{name}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// 给子进程注入 search_paths（让 npm 找得到 node）。
fn with_path(c: &mut Command) {
    let mut all = crate::installer::search_paths(crate::installer::portable_node_dir().as_deref());
    if let Some(cur) = std::env::var_os("PATH") {
        all.extend(std::env::split_paths(&cur));
    }
    if let Ok(j) = std::env::join_paths(all) {
        c.env("PATH", j);
    }
}

fn npm_uninstall(pkg: &str, on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let npm = resolve_bin("npm").ok_or("未找到 npm —— 若该工具不是用 npm 装的，请手动卸载")?;
    on_log(&format!("正在卸载 {pkg}（npm uninstall -g）…"));
    let mut c = Command::new(&npm);
    c.args(["uninstall", "-g", pkg]);
    with_path(&mut c);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    let out = c.output().map_err(|e| format!("起 npm 失败: {e}"))?;
    if out.status.success() {
        Ok(format!("已卸载 {pkg}"))
    } else {
        let e = String::from_utf8_lossy(&out.stderr);
        Err(format!("卸载失败：{}", e.trim()))
    }
}

/// 在安装目录里找官方卸载程序（NSIS `Uninstall*.exe` / Inno `unins*.exe`）。
#[cfg(windows)]
fn find_uninstaller(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.ends_with(".exe") && (n.contains("uninstall") || n.contains("unins")) {
            return Some(e.path());
        }
    }
    None
}

/// 跑某桌面 App 的官方卸载程序（静默）。`exe` = 该 app 的主 exe（同目录找卸载器）。
/// 找不到卸载器 → 报清楚指引，**绝不 rm 系统目录**。
#[cfg(windows)]
fn uninstall_app(exe: Option<PathBuf>, on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let exe = exe.ok_or("未找到该应用的安装位置")?;
    let dir = exe.parent().ok_or("无法定位安装目录")?;
    let unins = find_uninstaller(dir)
        .ok_or("未找到官方卸载程序，请到 Windows 设置 → 应用 里手动卸载")?;
    let name = unins.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    let args: &[&str] = if name.contains("unins") {
        &["/VERYSILENT", "/NORESTART"] // Inno Setup
    } else {
        &["/S"] // NSIS
    };
    on_log(&format!("正在运行官方卸载程序：{}", unins.display()));
    let mut child = Command::new(&unins)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动卸载程序失败: {e}"))?;
    let _ = child.wait();
    Ok("已触发官方卸载程序（若弹出界面按提示完成即可）".into())
}

/// 从 Windows 的「已安装应用」登记项启动原厂卸载程序。
///
/// 只用于 U-King 仅提供下载入口、没有自己的安装器的工具（如 Obsidian / UU 远程）。
/// 不按目录名递归删除：注册表找不到原厂卸载串就直接报明白，让用户去 Windows 设置处理。
#[cfg(windows)]
fn uninstall_registered_app(
    display_terms: &[&str],
    label: &str,
    on_log: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, String> {
    let terms = display_terms
        .iter()
        .map(|s| format!("'*{}*'", s.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let ps = format!(
        r#"
$roots = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)
$terms = @({terms})
$cmd = $null
foreach ($root in $roots) {{
  foreach ($key in @(Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue)) {{
    $p = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
    if ($p.DisplayName -and ($terms | Where-Object {{ $p.DisplayName -like $_ }})) {{
      $cmd = $p.QuietUninstallString
      if (-not $cmd) {{ $cmd = $p.UninstallString }}
      if ($cmd) {{ break }}
    }}
  }}
  if ($cmd) {{ break }}
}}
if (-not $cmd) {{ exit 2 }}
Start-Process -FilePath $env:ComSpec -ArgumentList @('/c', $cmd) -Wait
exit 0
"#
    );
    on_log(&format!("正在启动 {label} 的官方卸载程序…"));
    let out = Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("启动 {label} 卸载程序失败: {e}"))?;
    if out.status.code() == Some(2) {
        Err(format!("未找到 {label} 的官方卸载程序，请到 Windows 设置 → 应用 里手动卸载"))
    } else if out.status.success() {
        Ok(format!("已启动 {label} 官方卸载程序"))
    } else {
        Err(format!("{label} 卸载程序执行失败"))
    }
}

fn uninstall_ollama(on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    #[cfg(windows)]
    {
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            let dir = PathBuf::from(la).join("Programs").join("Ollama");
            if let Some(u) = find_uninstaller(&dir) {
                on_log(&format!("正在卸载 Ollama：{}", u.display()));
                let mut child = Command::new(&u)
                    .args(["/VERYSILENT", "/NORESTART"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .map_err(|e| format!("启动卸载程序失败: {e}"))?;
                let _ = child.wait();
                return Ok("已触发 Ollama 卸载程序（模型文件如需一并删请手动清理 ~/.ollama）".into());
            }
        }
        Err("未找到 Ollama 卸载程序，请到 设置 → 应用 手动卸载".into())
    }
    #[cfg(not(windows))]
    {
        let _ = on_log;
        Err("请用 `brew uninstall ollama` 或删除 /Applications/Ollama.app 手动卸载".into())
    }
}

// ═══════════════════ 单工具「彻底卸载」（首页 AI 卡片「卸载」用）═══════════════════
//
// 修「删了还能检测到、重装 U-King 又冒出来」：卸载一个 AI 工具时，把**所有会被
// `installer::tool_installed` / `tools.rs` 探测成"已装"的残留**一起清干净：
//   ① 工具本体（npm 包 / pip 包 / 官方卸载程序 / Appx）
//   ② npm 全局 bin 残留 stub（`%APPDATA%\npm\<cmd>.cmd` —— "手删目录没 npm uninstall" 的元凶）
//   ③ `~/.uking/tools/<sub>`、`~/.uclaw`（目录型探测源，存活于 $HOME → 重装 U-King 仍在）
//   ④ `~/.uking/shims/<cmd>.*`（覆盖安装建的转发脚本，前置在用户 PATH，指向已删本体只会添乱）
// 这样卸载后 `detect` 立刻变"未装"，重装 U-King 也不会再探到。
//
// **铁律仍在**：这是"帮你装的工具本体"（cleanup 的 aitool 档）—— 前端必须二次确认
// （"若你之前自己装过，这会真的删掉它"）。GUI app 一律走对方官方卸载程序，绝不 rm 系统目录。

/// `~/.uking/shims` —— 覆盖安装建的转发 .cmd 目录。
fn uking_shims_dir() -> PathBuf {
    uking_home().join("shims")
}

/// 删掉某些命令在 `~/.uking/shims` 下的转发脚本（各扩展名）。返回删除个数。
fn purge_shims(cmds: &[&str]) -> usize {
    let dir = uking_shims_dir();
    let exts = ["", ".cmd", ".exe", ".bat", ".ps1"];
    let mut n = 0;
    for c in cmds {
        for e in exts {
            let p = dir.join(format!("{c}{e}"));
            if p.exists() && std::fs::remove_file(&p).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// 删掉我们直接下发到 `~/bin` 和 `~/.local/bin` 的裸二进制（codex 官方 binary 走这条路）。
///
/// 这两个目录都在 `installer::search_paths` 里，`tool_installed` 只要在其中看到文件就判「已装」。
/// 原来只删了 `~/bin/codex.exe`，而且外面套着 `USERPROFILE` —— Mac 上根本没这个变量，
/// 等于 Mac 的 `~/.local/bin/codex` 从没被清过：卸载完仍然显示「已安装」，
/// 客户只剩「打开」按钮，重装无门（issue #237）。
fn purge_home_bins(cmds: &[&str]) {
    let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) else {
        return;
    };
    let exts: &[&str] = if cfg!(windows) {
        &["", ".cmd", ".exe", ".bat", ".ps1"]
    } else {
        &[""]
    };
    for sub in ["bin", ".local/bin"] {
        let dir = Path::new(&home).join(sub);
        for c in cmds {
            for e in exts {
                let _ = std::fs::remove_file(dir.join(format!("{c}{e}")));
            }
        }
    }
}

/// 删掉 npm 全局 bin 里遗留的转发 stub（`%APPDATA%\npm\<cmd>.cmd` 等）——
/// 用户"手删了 node_modules 但没 npm uninstall"时，这个 stub 会让 `search_paths` 里
/// 命中而误判"已装"。npm uninstall 正常会连带删掉它，这里是双保险。
#[cfg(windows)]
fn purge_npm_stub(cmds: &[&str]) {
    if let Ok(ad) = std::env::var("APPDATA") {
        let dir = PathBuf::from(ad).join("npm");
        for c in cmds {
            for e in ["", ".cmd", ".ps1", ".exe"] {
                let _ = std::fs::remove_file(dir.join(format!("{c}{e}")));
            }
        }
    }
}
#[cfg(not(windows))]
fn purge_npm_stub(_cmds: &[&str]) {}

/// best-effort 删目录（存在才删）；返回是否删了。
fn rm_dir_best_effort(p: &Path) -> bool {
    p.exists() && std::fs::remove_dir_all(p).is_ok()
}

/// pip 卸载便携 Python 里的包（hermes 用）。找不到 pip 不致命（本体可能已被删，仍继续清残留）。
fn pip_uninstall(pkg: &str, on_log: &(dyn Fn(&str) + Send + Sync)) {
    let Some(pip) = resolve_bin("pip") else {
        return;
    };
    on_log(&format!("正在卸载 {pkg}（pip uninstall）…"));
    let mut c = Command::new(&pip);
    c.args(["uninstall", "-y", pkg]);
    with_path(&mut c);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    let _ = c.output();
}

/// 卸载 Appx/MSIX 包（Codex 桌面版）。仅 Windows。
#[cfg(windows)]
fn uninstall_appx(name: &str, on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    on_log("正在卸载 Codex 桌面版…");
    let ps = format!("Get-AppxPackage -Name {name} | Remove-AppxPackage");
    let out = Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("启动卸载失败: {e}"))?;
    if out.status.success() {
        Ok("已卸载 Codex 桌面版".into())
    } else {
        Err(format!("卸载失败：{}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// 卸载 uu-switch（我方 MSI 安装）：优先跑注册表里的静默卸载串，兜底删安装目录。仅 Windows。
#[cfg(windows)]
fn uninstall_uuswitch(on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    on_log("正在卸载 uu-switch…");
    // 注册表卸载串（HKCU 优先，per-user MSI；再 HKLM）。找到 QuietUninstallString/UninstallString 就跑。
    let ps = r#"
$roots = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)
$done = $false
foreach ($r in $roots) {
  Get-ChildItem $r -ErrorAction SilentlyContinue | ForEach-Object {
    $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($p.DisplayName -like '*uu-switch*' -or $p.DisplayName -like '*uu switch*') {
      $u = $p.QuietUninstallString; if (-not $u) { $u = $p.UninstallString }
      if ($u) {
        if ($u -match 'msiexec') { $u = ($u -replace '/I','/X') + ' /qn /norestart' }
        cmd /c $u
        $done = $true
      }
    }
  }
}
if ($done) { 'ok' } else { 'notfound' }
"#;
    let out = Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("启动卸载失败: {e}"))?;
    let ok = String::from_utf8_lossy(&out.stdout).contains("ok");
    // 兜底：卸载串没找到/没删净时，删掉安装目录，至少让检测变"未装"。
    if let Some(exe) = crate::uuswitch::find_exe() {
        if let Some(dir) = exe.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    if ok {
        Ok("已卸载 uu-switch".into())
    } else {
        Ok("已移除 uu-switch（若开始菜单仍有残留项，可在 Windows 设置→应用 里清理）".into())
    }
}

// 前端 App.tsx 里 UNINSTALLABLE 集合镜像下方 match 支持的 tool_id（url 型第三方工具 Obsidian/UU远程
// 不由我们装，不提供一键卸载）。改这里的支持列表时同步前端那份。

/// 彻底卸载某个 AI 工具（首页卡片「卸载」）。删本体 + 一切会被探测成"已装"的残留，
/// 卸载后 `detect` 即变"未装"、重装 U-King 也不再冒出来。返回一句人话结果。
pub fn uninstall_ai_tool(tool_id: &str, on_log: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    match tool_id {
        "claude-code" | "claude" => {
            let r = npm_uninstall("@anthropic-ai/claude-code", on_log);
            purge_npm_stub(&["claude"]);
            purge_shims(&["claude"]);
            r.map(|_| "已卸载 Claude Code（含残留清理）".into())
                .or_else(|_| Ok("已清理 Claude Code 残留（npm 未装或已移除）".into()))
        }
        "codex" => {
            let r = npm_uninstall("@openai/codex", on_log);
            purge_home_bins(&["codex"]);
            purge_npm_stub(&["codex"]);
            purge_shims(&["codex"]);
            r.map(|_| "已卸载 Codex CLI（含残留清理）".into())
                .or_else(|_| Ok("已清理 Codex CLI 残留（npm 未装或已移除）".into()))
        }
        "openclaw" => {
            let r = npm_uninstall("openclaw", on_log);
            // 目录型探测源：~/.uclaw（旧 U-Claw 运行实例）、~/.uking/tools/openclaw
            if let Ok(h) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
                let _ = rm_dir_best_effort(&Path::new(&h).join(".uclaw"));
            }
            let _ = rm_dir_best_effort(&uking_home().join("tools").join("openclaw"));
            purge_npm_stub(&["openclaw"]);
            purge_shims(&["openclaw"]);
            let _ = r;
            Ok("已卸载 OpenClaw CLI（含 ~/.uclaw、技能目录、shim 残留清理）".into())
        }
        "hermes" => {
            pip_uninstall("hermes-agent", on_log);
            let _ = rm_dir_best_effort(&uking_home().join("tools").join("hermes"));
            purge_shims(&["hermes"]);
            Ok("已卸载 Hermes（含残留清理）".into())
        }
        "dsh" => {
            let r = npm_uninstall("@deepseek-ai/dsh", on_log);
            purge_npm_stub(&["dsh"]);
            purge_shims(&["dsh"]);
            r.map(|_| "已卸载 DeepSeek Harness（含残留清理）".into())
                .or_else(|_| Ok("已清理 DeepSeek Harness 残留（npm 未装或已移除）".into()))
        }
        "harness-doctor" => {
            let r = npm_uninstall("harness-doctor", on_log);
            purge_npm_stub(&["harness-doctor"]);
            purge_shims(&["harness-doctor"]);
            r.map(|_| "已卸载 Harness Doctor（含残留清理）".into())
                .or_else(|_| Ok("已清理 Harness Doctor 残留（npm 未装或已移除）".into()))
        }
        "ollama" => uninstall_ollama(on_log),

        #[cfg(windows)]
        "codex-app" => uninstall_appx("OpenAI.Codex", on_log),
        #[cfg(windows)]
        "clawx" => uninstall_app(crate::tools::find_clawx_exe(), on_log),
        #[cfg(windows)]
        "hermes-app" => uninstall_app(crate::tools::find_hermes_app_exe(), on_log),
        #[cfg(windows)]
        "uu-switch" => uninstall_uuswitch(on_log),
        #[cfg(windows)]
        "open365" => {
            // 我方轻量工具：删本地目录 + 桌面快捷方式即可（没有官方卸载程序）。
            let _ = rm_dir_best_effort(&uking_home().join("tools").join("open365"));
            if let Ok(h) = std::env::var("USERPROFILE") {
                let _ = std::fs::remove_file(Path::new(&h).join("Desktop").join("Open365.lnk"));
            }
            Ok("已卸载 Open365 电脑管家".into())
        }

        other => Err(format!("暂不支持一键卸载：{other}（请到 Windows 设置→应用 手动卸载）")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「进阶 → 逐项清理」必须**列得出**我们写进别家 AI 记忆文件的指针，并且清得干净。
    ///
    /// 🔴 为什么值得一条用例：撤销能力（`identity::unlink_in`）一直都有、单测也一直绿，
    /// 但它**没被登记进这张足迹表**——客户在那个自称「列出全部」的页面上找不到它，
    /// 于是合理地判断成「删不掉」（2026-08-18 反馈原话：「一来就乱加 agentme claude.me，
    /// 要能删，真删」）。**能力存在 ≠ 客户找得到**，而这条断言守的正是「找得到」。
    ///
    /// 顺带钉死两件事：
    ///  - 清完连 `*.uking-bak` 一起收走（不然「清理完还剩一堆备份」）
    ///  - 用户自己写在 CLAUDE.md 里的内容**逐字节不动**
    #[test]
    fn footprint_lists_and_removes_identity_pointers() {
        let sb = crate::testsandbox::enter("cleanup-identity", &[".claude"]);
        let home = sb.root();
        let claude_md = home.join(".claude").join("CLAUDE.md");
        const MINE: &str = "# 我自己的约定\n\n永远用中文回答。\n";
        std::fs::write(&claude_md, MINE).unwrap();

        // 🔴 断言必须打在 `scan()` 上，**不是** `identity_files()`。
        // 客户抱怨的是「在逐项清理那一页找不到」——那一页渲染的是 `scan()` 的输出。
        // 只断言探针函数会假绿：把 scan() 里那段 push 注释掉，探针照样返回非空、测试照样绿，
        // 而页面上一个字都没有。（这条是变异验证当场逼出来的：第一版就是这么写的。）
        let has_item = || scan().iter().any(|i| i.id == "identity-pointers");

        // 没挂指针时不该出现（只列**当前**足迹，不列「理论上可能有的」）
        assert!(!has_item(), "还没挂指针，清单里就冒出来了");

        crate::identity::link_in(home, &["claude".to_string()]).unwrap();
        assert!(
            has_item(),
            "挂了指针却没进「逐项清理」清单 —— 客户就是在这一页找不到它，才判断成删不掉"
        );

        let msg = remove("identity-pointers", &|_: &str| {}).unwrap();
        assert!(msg.contains("指针"), "回执没说清做了什么：{msg}");

        let after = std::fs::read_to_string(&claude_md).unwrap();
        assert_eq!(after, MINE, "用户自己写的内容被动了 —— 这是绝对红线");
        assert!(
            !claude_md.with_extension("md.uking-bak").exists(),
            "清完还剩 .uking-bak，那也是我们的足迹"
        );
        assert!(!has_item(), "清完了还列在清单里");
    }
}
