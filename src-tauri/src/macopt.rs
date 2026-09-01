//! macOS「AI 优化大师」原生实现 —— 不走 ukrt.exe（那是 Windows 专属二进制），直接在 U-King 里
//! 做体检 + 一键优化，返回与 Windows 版**同一份 JSON**（前端 AiRuntime 不用改）。
//!
//! 设计（与用户定的「最小 · 稳定 · 普适 · 官方方法」一致）：
//!  - Mac 天生比 Windows 友好（UTF-8 原生、bash 内置、无注册表/长路径坑），优化空间小 → 只做几项。
//!  - **只改配置、全部可回滚**（journal 备份改前文件，undo 逆序还原）；**绝不装重软件**。
//!    Node 便携装、Git 走 `xcode-select --install`（官方）都在 optimize_env 里，不在本模块。
//!
//! 跨平台可编译（纯 std + 通用命令字符串），仅在 macOS 被 lib.rs 路由调用；Windows 也能 cargo check
//! 到语法/类型（但不会跑）。本模块不碰 AppHandle，command 包在 lib.rs。
//!
//! 本模块只在 macOS 被调用 → 在 Windows 构建里整体 dead_code；保留 `mod macopt` 是为了让 Windows
//! 的 cargo check 也校验语法/类型，故这里 allow(dead_code) 抑制 Windows 侧的未使用告警。
#![allow(dead_code)]

use std::path::PathBuf;

fn home() -> PathBuf {
    let h = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(h)
}

/// 跑一条命令取 stdout（trim）；非 0 或起不来返回 None。CREATE_NO_WINDOW 只 Windows 需要，Mac 无所谓。
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn which(name: &str) -> Option<String> {
    // Finder 启动的 app PATH 很窄，补上 homebrew / 便携目录再找
    let mut paths: Vec<String> = Vec::new();
    let h = home();
    paths.push(h.join(".uking/runtime/node").display().to_string());
    paths.push("/opt/homebrew/bin".into());
    paths.push("/usr/local/bin".into());
    if let Ok(p) = std::env::var("PATH") {
        paths.push(p);
    }
    let joined = paths.join(":");
    let out = std::process::Command::new("/usr/bin/which")
        .arg(name)
        .env("PATH", &joined)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ———————————————— 体检项 ————————————————

struct Check {
    id: &'static str,
    category: &'static str,
    name: &'static str,
    status: &'static str, // pass | warn | fail | info
    points: u32,
    earned: u32,
    detail: String,
    fix_hint: &'static str,
}

fn pass(id: &'static str, cat: &'static str, name: &'static str, pts: u32, detail: String) -> Check {
    Check { id, category: cat, name, status: "pass", points: pts, earned: pts, detail, fix_hint: "" }
}
fn miss(id: &'static str, cat: &'static str, name: &'static str, pts: u32, hard: bool, detail: String, fix: &'static str) -> Check {
    Check { id, category: cat, name, status: if hard { "fail" } else { "warn" }, points: pts, earned: 0, detail, fix_hint: fix }
}
/// 不计分提示：无法自动修的项（如中文用户名）——不作为「永久扣分」黑洞拖住满分，但仍摆出来。
fn info(id: &'static str, cat: &'static str, name: &'static str, detail: String) -> Check {
    Check { id, category: cat, name, status: "info", points: 0, earned: 0, detail, fix_hint: "" }
}

const CAT_BASE: &str = "环境基础";
const CAT_ENC: &str = "编码与路径";
const CAT_AI: &str = "AI 工具链";
const CAT_TOKEN: &str = "省 Token 配置";

fn read(p: &PathBuf) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn checks() -> Vec<Check> {
    let h = home();
    let mut v = Vec::new();

    // —— 环境基础 ——
    v.push(match run("node", &["--version"]).or_else(|| which("node").and_then(|p| run(&p, &["--version"]))) {
        Some(ver) => pass("node", CAT_BASE, "Node.js", 8, ver),
        None => miss("node", CAT_BASE, "Node.js", 8, true, "未安装".into(), "点「一键优化」自动装便携 Node"),
    });
    // Mac 的 git 来自 Xcode 命令行工具；xcode-select -p 有输出即已装
    let git_ok = run("git", &["--version"]).is_some() || which("git").is_some() || run("xcode-select", &["-p"]).is_some();
    v.push(if git_ok {
        pass("git", CAT_BASE, "Git", 8, run("git", &["--version"]).unwrap_or_else(|| "已装（Xcode 命令行工具）".into()))
    } else {
        miss("git", CAT_BASE, "Git", 8, true, "未安装".into(), "点「一键优化」触发 xcode-select --install（Apple 官方）")
    });

    // —— 编码与路径 ——
    // Mac 的「乱码免疫」= shell 里有 UTF-8 locale（LANG/LC_ALL），部分 CLI/Python 缺了会乱码
    let zshrc = h.join(".zshrc");
    let zt = read(&zshrc);
    let has_lang = zt.contains("LANG=") && zt.contains("UTF-8");
    v.push(if has_lang || std::env::var("LANG").map(|l| l.contains("UTF-8")).unwrap_or(false) {
        pass("locale", CAT_ENC, "终端 UTF-8 locale", 6, "已配 UTF-8".into())
    } else {
        miss("locale", CAT_ENC, "终端 UTF-8 locale", 6, false, "~/.zshrc 未设 UTF-8 locale，部分工具可能乱码".into(), "一键优化写 export LANG=en_US.UTF-8（追加块，可回滚）")
    });
    v.push(match run("git", &["config", "--global", "core.quotepath"]).as_deref() {
        Some("false") => pass("git-quotepath", CAT_ENC, "Git 中文路径显示", 3, "core.quotepath=false".into()),
        _ => miss("git-quotepath", CAT_ENC, "Git 中文路径显示", 3, false, "中文文件名会显示成转义码，AI 读不懂".into(), "git config --global core.quotepath false"),
    });
    let hs = h.display().to_string();
    v.push(if hs.is_ascii() && !hs.contains(' ') {
        pass("home", CAT_ENC, "用户目录路径", 6, hs)
    } else {
        // 用户名含非 ASCII/空格无法自动改——不再永久扣 6 分把满分卡死，降为不计分提示（仍露出来）。
        info("home", CAT_ENC, "用户目录路径", format!("{hs}（含非 ASCII/空格）· 无法自动修：个别工具可能翻车，建议 AI 项目放纯英文路径（如 ~/ai-work）"))
    });

    // —— AI 工具链 ——
    v.push(match which("claude") {
        Some(p) => pass("claude", CAT_AI, "Claude Code", 10, p),
        None => miss("claude", CAT_AI, "Claude Code", 10, true, "未安装".into(), "用装机向导一键安装"),
    });
    v.push(match which("codex") {
        Some(p) => pass("codex", CAT_AI, "Codex CLI", 6, p),
        None => miss("codex", CAT_AI, "Codex CLI", 6, false, "未安装".into(), "用装机向导一键安装"),
    });
    let cmd_md = h.join(".claude").join("CLAUDE.md");
    v.push(if std::fs::metadata(&cmd_md).map(|m| m.len() > 100).unwrap_or(false) {
        pass("claude-md", CAT_AI, "全局 CLAUDE.md", 4, "已配".into())
    } else {
        miss("claude-md", CAT_AI, "全局 CLAUDE.md", 4, false, "缺失或过短，AI 每次都要重新摸索你的环境".into(), "点「一键优化」自动写入本机环境速览（系统/代理/已装 CLI，可改可删 · 可复原）")
    });

    // —— 省 Token 配置 ——
    let settings = h.join(".claude").join("settings.json");
    let n_allow = count_allow(&read(&settings));
    v.push(if n_allow >= 5 {
        pass("claude-perms", CAT_TOKEN, "Claude 权限允许清单", 6, format!("{n_allow} 条（少弹确认 = 少烧来回）"))
    } else {
        miss("claude-perms", CAT_TOKEN, "Claude 权限允许清单", 6, false, format!("仅 {n_allow} 条，常用命令每次都要人工确认"), "一键优化写入安全允许清单（可回滚）")
    });
    let npmrc = h.join(".npmrc");
    let ns = read(&npmrc);
    v.push(if ns.contains("fund=false") && ns.contains("audit=false") {
        pass("npmrc", CAT_TOKEN, "npm 输出降噪", 5, "fund/audit 已关".into())
    } else {
        miss("npmrc", CAT_TOKEN, "npm 输出降噪", 5, false, "npm install 的噪音全进 AI 上下文烧 token".into(), "一键优化写 ~/.npmrc（fund/audit/progress 关）")
    });

    // —— 代理诊断（只读、不计分，与 Windows ukrt::proxy_info 同口径） ——
    // 不给客户配代理（虾盘云走国内直连、无需梯子），而是把「有没有代理在跑、它可能在拦你」讲清楚。
    v.push(proxy_check());

    v
}

/// 探测正在运行的代理软件（ps -A -o comm=，小写包含匹配；起不来则空）。Mac 常见客户端。
fn detect_proxy_procs() -> Vec<&'static str> {
    let out = run("/bin/ps", &["-A", "-o", "comm="]).unwrap_or_default();
    let low = out.to_lowercase();
    const K: &[(&str, &str)] = &[
        ("clash", "Clash / ClashX"),
        ("mihomo", "Mihomo(Clash)"),
        ("sing-box", "sing-box"),
        ("v2ray", "V2Ray"),
        ("xray", "Xray"),
        ("surge", "Surge"),
        ("shadowrocket", "小火箭"),
        ("quantumult", "Quantumult"),
    ];
    let mut f: Vec<&'static str> = Vec::new();
    for (n, l) in K {
        if low.contains(n) && !f.contains(l) {
            f.push(l);
        }
    }
    f
}

/// 代理诊断项（info，不计分）：排雷指引而非埋雷配置——见 checks.rs::proxy_info 的同款理由。
fn proxy_check() -> Check {
    let env_proxy = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY")).ok().filter(|s| !s.is_empty());
    let git_proxy = run("git", &["config", "--global", "http.proxy"]);
    let procs = detect_proxy_procs();
    if env_proxy.is_none() && git_proxy.is_none() && procs.is_empty() {
        return info(
            "proxy",
            CAT_BASE,
            "代理 / 梯子",
            "未检测到代理。虾盘云走国内直连（u-claw.org.cn），本就无需梯子——这是正常、推荐的状态。".into(),
        );
    }
    let mut parts: Vec<String> = Vec::new();
    if !procs.is_empty() {
        parts.push(format!("正在运行：{}", procs.join("、")));
    }
    if let Some(p) = &env_proxy {
        parts.push(format!("HTTP(S)_PROXY={p}"));
    }
    if let Some(p) = &git_proxy {
        parts.push(format!("git http.proxy={p}"));
    }
    info(
        "proxy",
        CAT_BASE,
        "代理 / 梯子",
        format!(
            "{}。⚠ 虾盘云本身走国内直连、不需要代理。若遇连不上模型 / 作图时好时坏，多半是代理在拦——先临时退出代理、或关掉全局 / TUN 模式再试。",
            parts.join("；")
        ),
    )
}

fn count_allow(json: &str) -> usize {
    if let Some(i) = json.find("\"allow\"") {
        if let Some(j) = json[i..].find('[') {
            let start = i + j + 1;
            if let Some(k) = json[start..].find(']') {
                return json[start..start + k].matches('"').count() / 2;
            }
        }
    }
    0
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// 体检 → JSON（与 Windows ukrt 同结构，前端直接 JSON.parse）。
pub fn doctor_json() -> Result<String, String> {
    let cs = checks();
    let total: u32 = cs.iter().map(|c| c.points).sum();
    let earned: u32 = cs.iter().map(|c| c.earned).sum();
    let score = if total == 0 { 0 } else { (earned * 100 + total / 2) / total };
    let mut items = String::new();
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        let bonus = c.points - c.earned;
        items.push_str(&format!(
            "{{\"id\":\"{}\",\"category\":\"{}\",\"name\":\"{}\",\"status\":\"{}\",\"points\":{},\"earned\":{},\"bonus\":{},\"detail\":\"{}\",\"fix_hint\":\"{}\"}}",
            c.id, c.category, c.name, c.status, c.points, c.earned, bonus, json_escape(&c.detail), json_escape(c.fix_hint)
        ));
    }
    Ok(format!(
        "{{\"tool\":\"macopt\",\"version\":\"1.0\",\"score\":{score},\"earned\":{earned},\"total\":{total},\"checks\":[{items}]}}"
    ))
}

// ———————————————— 一键优化（只改配置，可回滚） ————————————————

fn journal_dir() -> PathBuf {
    home().join(".uking").join("macopt").join("journal")
}

/// 改前把文件备份进 journal（同一次 action 一个时间桶目录，undo 按目录逆序还原）。
fn backup(bucket: &PathBuf, p: &PathBuf) {
    let _ = std::fs::create_dir_all(bucket);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    // 记录原始绝对路径（.orig 同名 + .path 存原路径），undo 据此还原
    let dst = bucket.join(&name);
    if p.exists() {
        let _ = std::fs::copy(p, &dst);
    } else {
        // 原本不存在 → 记个空标记，undo 时删掉我们新建的
        let _ = std::fs::write(bucket.join(format!("{name}.absent")), b"");
    }
    let _ = std::fs::write(bucket.join(format!("{name}.path")), p.display().to_string());
}

/// 一键优化（fix + optimize 合一，Mac 只有配置项）：git config / ~/.zshrc locale+PATH / ~/.npmrc / ~/.claude/settings.json。
pub fn optimize() -> Result<String, String> {
    let h = home();
    let bucket = journal_dir().join(stamp());
    let mut applied: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // 1) git config quotepath
    if run("git", &["config", "--global", "core.quotepath"]).as_deref() == Some("false") {
        skipped.push("Git 中文路径显示：已达标".into());
    } else if run("git", &["config", "--global", "core.quotepath", "false"]).is_some()
        || std::process::Command::new("git").args(["config", "--global", "core.quotepath", "false"]).status().map(|s| s.success()).unwrap_or(false)
    {
        applied.push("Git 中文路径显示：core.quotepath=false".into());
    } else {
        skipped.push("Git 中文路径显示：git 不可用，跳过".into());
    }

    // 2) ~/.zshrc：UTF-8 locale + 补 PATH（便携 Node / homebrew）
    let zshrc = h.join(".zshrc");
    let zt = read(&zshrc);
    if zt.contains("# >>> ukrt >>>") {
        skipped.push("~/.zshrc：已配置".into());
    } else {
        backup(&bucket, &zshrc);
        let block = format!(
            "\n# >>> ukrt >>>（U-King 优化，可在 U-King 里一键复原）\nexport LANG=${{LANG:-en_US.UTF-8}}\nexport PATH=\"$HOME/.uking/runtime/node:/opt/homebrew/bin:/usr/local/bin:$PATH\"\n# <<< ukrt <<<\n"
        );
        if std::fs::write(&zshrc, format!("{zt}{block}")).is_ok() {
            applied.push("~/.zshrc：UTF-8 locale + 工具 PATH（追加块，原内容不动）".into());
        } else {
            skipped.push("~/.zshrc：写入失败".into());
        }
    }

    // 3) ~/.npmrc 降噪
    let npmrc = h.join(".npmrc");
    let old = read(&npmrc);
    let want = [("fund", "false"), ("audit", "false"), ("progress", "false"), ("loglevel", "error")];
    let mut lines: Vec<String> = old.lines().map(|s| s.to_string()).collect();
    let mut changed = false;
    for (k, val) in want {
        let prefix = format!("{k}=");
        let newl = format!("{k}={val}");
        match lines.iter_mut().find(|l| l.trim_start().starts_with(&prefix)) {
            Some(l) => {
                if *l != newl {
                    *l = newl;
                    changed = true;
                }
            }
            None => {
                lines.push(newl);
                changed = true;
            }
        }
    }
    if changed {
        backup(&bucket, &npmrc);
        if std::fs::write(&npmrc, lines.join("\n") + "\n").is_ok() {
            applied.push("npm 输出降噪：~/.npmrc（fund/audit/progress 关，其余行保留）".into());
        } else {
            skipped.push("npm 输出降噪：写入失败".into());
        }
    } else {
        skipped.push("npm 输出降噪：已达标".into());
    }

    // 4) ~/.claude/settings.json 安全允许清单（缺则建/合并；非法 JSON 不动）
    match merge_claude_allow(&h, &bucket) {
        Ok(Some(n)) => applied.push(format!("Claude 允许清单：合并 {n} 条安全命令（保留原有配置，可回滚）")),
        Ok(None) => skipped.push("Claude 允许清单：已全在列或文件非法，未改".into()),
        Err(e) => skipped.push(format!("Claude 允许清单：{e}")),
    }

    // 5) 全局 ~/.claude/CLAUDE.md（与 Windows(ukrt) 版同一套语义，见 fixes.rs::global_claude_md）：
    //    缺失 → 写；我们写的 → **每次都重新生成、只刷分隔线以上**；用户自己的 → 一个字不碰。
    //
    //    🔴 这里以前是「只在缺失时写」，也就是写一次就永远冻结。后果和 Windows 版栽过的一样：
    //    文件落盘那一刻可能就是错的（装机流程还没跑完），而从此再没有任何机制更正它。
    //    「可刷新」和「不覆盖用户内容」不矛盾，前提是分清哪半边是谁的 —— 分隔线就是那条界。
    {
        let dir = h.join(".claude");
        let md = dir.join("CLAUDE.md");
        let fresh = build_env_md_mac(&h);
        match std::fs::read_to_string(&md) {
            Err(_) => {
                let _ = std::fs::create_dir_all(&dir);
                backup(&bucket, &md);
                if std::fs::write(&md, &fresh).is_ok() {
                    applied.push("全局 CLAUDE.md：写入本机约定（可改可删，可复原）".into());
                } else {
                    skipped.push("全局 CLAUDE.md：写入失败".into());
                }
            }
            Ok(old) if !old.contains(ENV_MD_MARK) => {
                skipped.push("全局 CLAUDE.md：已存在且不是本工具生成的，未覆盖（避免动你的内容）".into());
            }
            Ok(old) => match merge_env_md(&old, &fresh) {
                None => skipped.push("全局 CLAUDE.md：结构与预期不符，未刷新（避免动你的内容）".into()),
                Some(want) if want == old => skipped.push("全局 CLAUDE.md：已是最新".into()),
                Some(want) => {
                    backup(&bucket, &md);
                    if std::fs::write(&md, &want).is_ok() {
                        applied.push("全局 CLAUDE.md：刷新了上半段（你写的「你的偏好」原样保留，旧版留底可回滚）".into());
                    } else {
                        skipped.push("全局 CLAUDE.md：刷新失败".into());
                    }
                }
            },
        }
    }

    Ok(format!(
        "✔ 已优化 {} 项：\n{}\n{}",
        applied.len(),
        applied.iter().map(|s| format!("  ✔ {s}")).collect::<Vec<_>>().join("\n"),
        if skipped.is_empty() { String::new() } else { skipped.iter().map(|s| format!("  ↷ {s}")).collect::<Vec<_>>().join("\n") }
    ))
}

/// 我们生成的那份的指纹；没有这一串就是用户自己的文件，一个字都不许动。
/// **必须与 Windows(ukrt) 版 `fixes.rs::ENV_MD_MARK` 逐字节一致** —— 同一个客户可能两边都跑过。
const ENV_MD_MARK: &str = "U-King 自动生成";
/// 分隔线：以上是我们负责刷新的，以下「你的偏好」是用户的，永不覆盖。
const ENV_MD_SPLIT: &str = "\n---\n## 你的偏好";

/// 分隔线以上取新生成的，以下（「你的偏好」）原样保留用户写的。
/// `None` = 结构不认识，别动这个文件。纯字符串函数，好测（IO 留在调用方）。
fn merge_env_md(old: &str, fresh: &str) -> Option<String> {
    let cut = fresh.find(ENV_MD_SPLIT)?;
    let keep = old.find(ENV_MD_SPLIT)?;
    Some(format!("{}{}", &fresh[..cut], &old[keep..]))
}

/// 组装 macOS「本机约定」markdown。
///
/// 🔴 **这份文件是客户每次 AI 调用都要重放一遍的固定开销**（实测：会话消耗的大头是
/// 「每次工具调用重放整个上下文」，不是用户打的字）。所以它只配放两种东西：
///   1. **不会过期的约定**（`~/.uking` 别重复装、Finder 的窄 PATH 坑）；
///   2. **一条指向实时真值的指针**。
///
/// 🔴 **绝不缓存版本号 / 装没装**：缓存一份会烂的答案，不如留一行取答案的命令。
/// `runtime.stack.inspect` 这个只读动作**只要 350 字符 JSON** 就给全 node/npm/git/claude/codex
/// 版本 + 便携 node + 系统代理，且永远不会过期（Windows 版栽在缓存上的经过见 fixes.rs 同名函数）。
fn build_env_md_mac(h: &PathBuf) -> String {
    // macopt 编译在 u-king-mini 内部，所以 current_exe 就是动作核心入口（Windows 那边是独立的
    // ukrt.exe，得另算路径）。取不到就退回裸名字，宁可少一段路径也别写错的。
    let exe = std::env::current_exe()
        .map(|p| format!("\"{}\"", p.display()))
        .unwrap_or_else(|_| "u-king-mini".into());
    let hs = h.display().to_string();
    let home_warn = if !hs.is_ascii() || hs.contains(' ') {
        "（⚠ 含非 ASCII/空格，个别工具易翻车，AI 项目建议放纯英文路径如 ~/ai-work）"
    } else {
        ""
    };
    format!(
        "# CLAUDE.md — 本机约定（U-King 自动生成 · 可改可删）\n\
\n\
> 分隔线以上每次「一键优化」重写；以下「你的偏好」是你的，永不覆盖。不想要就删掉本文件。\n\
> 只放不会过期的约定 —— 版本号、装没装用下面这条现查，过期的环境说明比没有更坏。\n\
\n\
## 环境真值：现查\n\
```\n\
{exe} action run runtime.stack.inspect --json --no-input\n\
```\n\
只读 JSON，一次给全 node/npm/git/claude/codex 版本、便携 node、系统代理。\n\
更多动作 `action list --json`；说明书 `~/.uking/llms.txt`（按需检索，别整份读）。\n\
\n\
## 约定与坑\n\
- 便携 Node/工具在 `~/.uking`，别在系统里重复装。\n\
- 终端 zsh、UTF-8 原生；**Finder 启动的 App PATH 很窄**，命令找不到先怀疑 PATH。\n\
- 用户目录：{hs}{home_warn}\n\
\n\
## 省 token\n\
开销大头是**每次工具调用重放整个上下文**，不是你打的字 —— 会话越长越贵。\n\
上下文到 15 万就 `/clear` 重开（实测省四成）；读文件给行号范围、先搜再读；截图看完立刻清。\n\
\n\
---\n\
## 你的偏好（人工填，可留空）\n\
- 常用语言 / 框架：\n\
- 别碰的目录 / 文件：\n\
- 特殊约定：\n"
    )
}

#[cfg(test)]
mod env_md_tests {
    use super::*;

    /// 模板产物必须**自己认得自己**，否则「活的」会静默变回「死的」：
    ///  - 少了 `ENV_MD_MARK`：下次一键优化认不出这是自己写的，当成用户文件永远跳过；
    ///  - 少了 `ENV_MD_SPLIT`：`merge_env_md` 返回 None，走「结构不符」分支，同样再不刷新。
    /// 两种都表现为「客户那份从此冻结在写下的那一刻」—— 正是这次要修掉的那个坑。
    #[test]
    fn generated_template_is_self_refreshable() {
        let md = build_env_md_mac(&PathBuf::from("/Users/test"));
        assert!(md.contains(ENV_MD_MARK), "模板丢了指纹，下次优化会认不出自己写的：\n{md}");
        assert!(md.contains(ENV_MD_SPLIT), "模板丢了分隔线，刷新会永久跳过：\n{md}");
        let again = merge_env_md(&md, &md).expect("自己的产物必须能被自己合并");
        assert_eq!(again, md, "对自己合并不幂等，说明分隔线切点不稳");
    }

    /// 刷新机器事实时，用户写在「你的偏好」里的东西必须一个字不少地留下。
    #[test]
    fn merge_keeps_user_prefs() {
        let fresh = build_env_md_mac(&PathBuf::from("/Users/test"));
        let old = format!("# 旧版（U-King 自动生成）\n过期内容{ENV_MD_SPLIT}（人工填）\n- 常用语言：Rust\n- 别碰：~/财务\n");
        let got = merge_env_md(&old, &fresh).expect("结构正常应能合并");
        assert!(got.contains("常用语言：Rust") && got.contains("~/财务"), "用户偏好被吃掉了：{got}");
        assert!(!got.contains("过期内容"), "上半段没换掉：{got}");
    }

    /// 结构认不出来就别动 —— 宁可不刷新，也不能猜着改用户的文件。
    #[test]
    fn merge_refuses_unknown_structure() {
        let fresh = build_env_md_mac(&PathBuf::from("/Users/test"));
        assert!(merge_env_md("我自己手写的 CLAUDE.md，没有分隔线", &fresh).is_none());
    }

    /// 缓存会过期的机器事实是这份文件的原罪，别让它回来。
    #[test]
    fn template_caches_no_rotting_facts() {
        let md = build_env_md_mac(&PathBuf::from("/Users/test"));
        for banned in ["## 已装 AI 工具与运行时", "- Node.js：", "- Git：", "- Codex CLI：", "- Claude Code："] {
            assert!(!md.contains(banned), "模板又开始缓存会过期的事实 `{banned}`：\n{md}");
        }
        assert!(md.contains("runtime.stack.inspect"), "得留一条取真值的指针：\n{md}");
    }
}

const SAFE_ALLOW: &[&str] = &[
    "Bash(git status:*)",
    "Bash(git diff:*)",
    "Bash(git log:*)",
    "Bash(git show:*)",
    "Bash(git branch:*)",
    "Bash(ls:*)",
    "Bash(pwd)",
    "Bash(node --version)",
    "Bash(npm --version)",
];

/// 往 ~/.claude/settings.json 的 permissions.allow 合并安全清单（极简 JSON 处理，避免引 serde 到本模块）。
/// 返回 Ok(Some(n)) 加了 n 条；Ok(None) 无需改 / 非法 JSON 跳过。用最简策略：文件不存在则新建模板；
/// 存在但没有 "allow" 数组则不硬改（避免破坏用户配置，交给 Windows 版 serde 精确合并的等价保守策略）。
fn merge_claude_allow(h: &PathBuf, bucket: &PathBuf) -> Result<Option<usize>, String> {
    let dir = h.join(".claude");
    let p = dir.join("settings.json");
    if !p.exists() {
        let _ = std::fs::create_dir_all(&dir);
        backup(bucket, &p);
        let allow = SAFE_ALLOW.iter().map(|a| format!("      \"{a}\"")).collect::<Vec<_>>().join(",\n");
        let body = format!("{{\n  \"permissions\": {{\n    \"allow\": [\n{allow}\n    ]\n  }}\n}}\n");
        std::fs::write(&p, body).map_err(|e| format!("写 settings.json 失败: {e}"))?;
        return Ok(Some(SAFE_ALLOW.len()));
    }
    let raw = read(&p);
    let trimmed = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    // 只在已有 "allow": [ ... ] 时做保守追加：找到数组，逐条补缺失项（字符串包含判断）。
    let Some(ai) = trimmed.find("\"allow\"") else {
        return Ok(None); // 没有 allow 字段 → 不硬塞（保守，避免破坏用户结构）
    };
    let Some(lb_rel) = trimmed[ai..].find('[') else { return Ok(None) };
    let lb = ai + lb_rel;
    let Some(rb_rel) = trimmed[lb..].find(']') else { return Ok(None) };
    let rb = lb + rb_rel;
    let inside = &trimmed[lb + 1..rb];
    let mut to_add: Vec<&str> = Vec::new();
    for a in SAFE_ALLOW {
        if !inside.contains(a) {
            to_add.push(a);
        }
    }
    if to_add.is_empty() {
        return Ok(None);
    }
    let sep = if inside.trim().is_empty() { "" } else { "," };
    let addition = format!("{sep}{}", to_add.iter().map(|a| format!("\n      \"{a}\"")).collect::<Vec<_>>().join(","));
    let mut out = String::with_capacity(trimmed.len() + addition.len());
    out.push_str(&trimmed[..rb]);
    out.push_str(&addition);
    out.push_str(&trimmed[rb..]);
    backup(bucket, &p);
    std::fs::write(&p, out).map_err(|e| format!("写 settings.json 失败: {e}"))?;
    Ok(Some(to_add.len()))
}

/// 一键复原：还原最近一次 journal 桶里的文件（删掉我们新建的、恢复我们改过的）。
pub fn undo() -> Result<String, String> {
    let jd = journal_dir();
    let mut buckets: Vec<PathBuf> = std::fs::read_dir(&jd)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    if buckets.is_empty() {
        return Ok("没有可复原的优化记录".into());
    }
    buckets.sort();
    let bucket = buckets.pop().unwrap();
    let mut restored = 0;
    if let Ok(rd) = std::fs::read_dir(&bucket) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().to_string();
            if !name.ends_with(".path") {
                continue;
            }
            let orig = read(&ent.path());
            let orig_path = PathBuf::from(orig.trim());
            let base = name.trim_end_matches(".path").to_string();
            let absent = bucket.join(format!("{base}.absent"));
            if absent.exists() {
                // 原本不存在 → 删掉我们新建的
                let _ = std::fs::remove_file(&orig_path);
                restored += 1;
            } else {
                let backup_file = bucket.join(&base);
                if backup_file.exists() {
                    let _ = std::fs::copy(&backup_file, &orig_path);
                    restored += 1;
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&bucket);
    Ok(format!("已复原最近一次优化（还原 {restored} 个文件；可连续点复原更早的）"))
}

/// 分发（对齐 Windows airuntime::run_action）。Mac 没有 defender（那是 Windows 概念）。
pub fn run_action(action: &str) -> Result<String, String> {
    match action {
        "fix" | "optimize" => optimize(),
        "undo" => undo(),
        "defender" => Ok("macOS 无需杀软白名单（该项为 Windows 专属）".to_string()),
        other => Err(format!("不允许的动作：{other}")),
    }
}

/// 极简时间桶名（不用 SystemTime 精度也行，用进程 id + journal 现有桶数保证唯一）。
fn stamp() -> String {
    let n = std::fs::read_dir(journal_dir()).map(|rd| rd.count()).unwrap_or(0);
    format!("{}-{}", std::process::id(), n + 1)
}
