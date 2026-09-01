//! 网站 GEO 体检 —— 1so-geo 技能包（「一搜商答」本地 GEO 工具）的 GUI 后端。
//!
//! 只做一件事：跑 `node ~/.uking/skills/1so-geo/bin/1so.mjs scan --name … --json`，
//! 生成一个自包含的「互联网体检面板.html」（44 渠道自查，纯离线不需 LLM），返回它的路径，
//! 前端再用系统浏览器打开（面板要开一堆"去查"新标签，浏览器最合适）。
//!
//! 独立可插拔（守设计取舍铁律）：本模块只暴露纯函数、不碰 AppHandle；`#[tauri::command]`
//! 写在 lib.rs 转调。删除本模块只需动 2 个文件：lib.rs（去 `mod geo` + command）、
//! App.tsx（去 import + tab）。依赖方向只「geo → installer 公共助手」，不反向。
//! 复用 `installer::{search_paths, portable_node_dir}`，不复制。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// ~/.uking 根目录（与 installer::uking_home 同口径，这里自算保持模块自包含）。
fn uking_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".uking")
}

/// 1so-geo 技能全部运行文件，**内嵌进 exe**（include_str!，纯文本 .mjs/.json/.md，共 ~87KB）。
/// 这样客户装了 U-King 就自带 GEO 能力，不依赖安装/技能包分发流程。改了任一脚本要 +SKILL_VERSION。
const SKILL_FILES: &[(&str, &str)] = &[
    ("bin/1so.mjs", include_str!("../skills/1so-geo/bin/1so.mjs")),
    ("package.json", include_str!("../skills/1so-geo/package.json")),
    ("SKILL.md", include_str!("../skills/1so-geo/SKILL.md")),
    ("src/channels.mjs", include_str!("../skills/1so-geo/src/channels.mjs")),
    ("src/cli.mjs", include_str!("../skills/1so-geo/src/cli.mjs")),
    ("src/config.mjs", include_str!("../skills/1so-geo/src/config.mjs")),
    ("src/probe.mjs", include_str!("../skills/1so-geo/src/probe.mjs")),
    ("src/util.mjs", include_str!("../skills/1so-geo/src/util.mjs")),
    ("src/commands/scan.mjs", include_str!("../skills/1so-geo/src/commands/scan.mjs")),
    ("samples/样板报告.html", include_str!("../skills/1so-geo/samples/样板报告.html")),
];

/// 🔴 **v8 起不再发布的文件 —— 升级时必须从客户机上删掉。**（2026-08-24）
///
/// 为什么：`llm.mjs` 会**自己**去读 `~/.uking/device.json` 里的虾盘云设备钱包 Key。
/// 也就是说，只要客户机上躺着这个文件，任何人敲
/// `node ~/.uking/skills/1so-geo/bin/1so.mjs aicheck --provider openai`
/// 就能拿着**我们的**额度烧 token —— 不需要 GUI，也不需要我们注入任何密钥。
///
/// 🔴 **光把它们从 `SKILL_FILES` 里拿掉是不够的**：`ensure_skill` 原本只写不删，
/// 升级时新文件覆盖旧文件、而**不再发布的旧文件会永远留在磁盘上**。
/// 也就是说对所有已装机的老客户，这个口子会一直开着，且我们这边零信号。
/// 这和「卸载后 `ensure_installed` 把小程序原样装回来」是同一种病：
/// **看着删了，其实没删。**
///
/// 我们自己（人工给客户出报告）用的是**仓库里**那份完整技能包，不是客户机上这份 ——
/// 能力和代码一行没丢，丢的只是「在客户机上能被调起来」这件事。
const REMOVED_FILES: &[&str] = &[
    "src/llm.mjs",
    // pay.mjs 只被 aicheck/inspect 的报告生成器用；它们不发了，它也就没人 import 了。
    "src/pay.mjs",
    "src/commands/aicheck.mjs",
    "src/commands/inspect.mjs",
    "src/commands/detect.mjs",
    "src/commands/generate.mjs",
    "src/commands/ingest.mjs",
    "src/commands/optimize.mjs",
    "src/commands/questions.mjs",
];

/// 内嵌技能版本。改了上面任一脚本就 +1 —— 客户机据此覆盖旧释放（旧版本或缺文件才写，不动用户产物）。
/// v2：aicheck 默认后端改虾盘云内置 Key + 收费方案 CTA 真接入 + llm.mjs reasoning_content 兜底。
/// v3：收费方案重定位为「我们帮你做 GEO+MEO 优化」+ CTA 改支付宝直接付款（收款码/链接，见 aicheck.mjs PAY）。
/// v4：aicheck 评分器（聚合判读）不再单点——judgeModel 失败就回退到「本轮答上来的模型」，
///     全挂再走本地启发式出分。修 pc-***「This token has no access to model gpt-5.4-mini」整份报废。
/// v5：aicheck 报告接入 geo-citation-lab 研究背书——加研究背书条带 +「为什么 AI 没把你排进去」
///     研究洞察区块（5 条方法论 + 反常识结论）+ footer 论文引用，提升报告专业度与收费转化。
/// v6：新增 `inspect` 网页 AI 友好度诊断（inspect.mjs）——抓页面 + robots.txt + llms.txt，
///     100 分制 12 维体检（融合 Auriti GEO/MIT + Princeton KDD 实证 + CN-GEO；覆盖国产 AI 爬虫），
///     并生成 llms.txt / JSON-LD 修复文件。收费方案抽出到共享 pay.mjs（aicheck/inspect 单一真相源）。
/// v7：① aicheck 模型清单改**国产为主**（deepseek/qwen/glm/MiniMax + 海外 gpt/gemini「能测就测」，
///        全部实测通；kimi 纯推理常返空已剔除），评分器换国产 deepseek-v4-flash（更稳/必在白名单）。
///     ② aicheck 失败信息分类——网络/DNS 没通不再误报「余额不足」（客户会误以为 token 没了），
///        按收集到的 error 区分 网络/白名单无权/真欠费 三类给准话（实测：挂代理→node 不走代理→ENOTFOUND）。
/// v8：🔴 **客户端只发离线自查那条链** —— `llm.mjs` / `pay.mjs` 和 7 个会调模型的命令
///     （aicheck/inspect/detect/generate/ingest/optimize/questions）不再随 exe 分发，
///     并在升级时从客户机上**删掉**（见 `REMOVED_FILES`）。
///     起因：`llm.mjs` 自己读 `~/.uking/device.json` 拿虾盘云 Key，任何人在命令行调 aicheck
///     就能烧我们的额度 —— 摘 GUI 按钮挡不住。同时 cli.mjs/probe.mjs 改成动态 import，
///     缺文件时说人话（「没随客户端发布，加微信 hecare888 人工出报告」）而不是整个 CLI 崩掉。
///     新增 `samples/样板报告.html`（预渲染、演示数据、无 JS），GUI 用它做能力展示。
const SKILL_VERSION: u32 = 8;

fn skill_dir() -> PathBuf {
    uking_dir().join("skills").join("1so-geo")
}

/// 确保 1so-geo 技能已释放到 ~/.uking/skills/1so-geo/，返回 bin/1so.mjs 路径。
/// 缺文件或内嵌版本更新才写（只覆盖我们自己分发的脚本，绝不碰 ~/.uking/geo/ 里的用户体检产物）。
fn ensure_skill() -> Result<PathBuf, String> {
    let root = skill_dir();
    let entry = root.join("bin").join("1so.mjs");
    let ver_file = root.join(".uking-embed-version");
    let cur = std::fs::read_to_string(&ver_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if !entry.exists() || cur < SKILL_VERSION {
        for (rel, content) in SKILL_FILES {
            let p = root.join(rel);
            if let Some(par) = p.parent() {
                let _ = std::fs::create_dir_all(par);
            }
            std::fs::write(&p, content).map_err(|e| format!("释放 GEO 技能 {rel} 失败: {e}"))?;
        }
        // 🔴 **先写后删**：不再发布的文件必须从客户机上清掉，否则「从清单里拿掉」
        // 对已装机的老客户等于什么都没做（`ensure_skill` 原本只写不删）。
        // 用 `remove_file` 的 Err 不当失败：文件本来就不存在（新客户）是最常见的情况。
        for rel in REMOVED_FILES {
            let _ = std::fs::remove_file(root.join(rel));
        }
        let _ = std::fs::write(&ver_file, SKILL_VERSION.to_string());
    }
    Ok(entry)
}

/// node 可执行：优先便携版（~/.uking/runtime/node），否则赌 PATH 里的 node
/// （下面 with_path 已把 search_paths 前置注入，双击启动也能找到）。
fn node_program() -> String {
    if let Some(dir) = crate::installer::portable_node_dir() {
        #[cfg(windows)]
        let exe = dir.join("node.exe");
        #[cfg(not(windows))]
        let exe = dir.join("node");
        if exe.exists() {
            return exe.display().to_string();
        }
    }
    "node".into()
}

/// 给子进程注入 PATH（前置便携 Node / npm 全局目录等，和安装/终端完全一致）。
fn inject_path(c: &mut Command) {
    let dirs = crate::installer::search_paths(None);
    if dirs.is_empty() {
        return;
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let old = std::env::var("PATH").unwrap_or_default();
    let prefix = dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(sep);
    c.env("PATH", format!("{prefix}{sep}{old}"));
}

/// 体检结果（前端只需要面板路径 + 覆盖渠道数）。
#[derive(Serialize)]
pub struct GeoScan {
    /// 「互联网体检面板.html」绝对路径。
    pub panel: String,
    /// 覆盖的渠道数（AI搜索/AI对话/传统/社交/视频/百科/地图）。
    pub channels: u32,
    /// 是否跑了自动粗测（MVP 恒 false，留给后续 --auto）。
    pub auto_ran: bool,
}

/// GEO 技能是否就绪（内嵌，顺手释放一次）。正常恒 true，除非磁盘写入失败（如权限）。
pub fn is_installed() -> bool {
    ensure_skill().is_ok()
}

/// 样板报告（演示数据、预渲染、无 JS）的绝对路径 —— 顺手确保技能包已释放。
///
/// 这是页面上「看看报告长什么样」那颗按钮的落点。**它是静态文件**：不联网、不调模型、
/// 不读客户的任何东西，所以永远不会翻车，也永远不会让客户误以为我们分析了他的网站
/// （报告顶部有醒目横幅写明是虚构示例）。
pub fn sample_report() -> Result<String, String> {
    ensure_skill()?;
    let p = skill_dir().join("samples").join("样板报告.html");
    if !p.exists() {
        return Err("样板报告没有释放成功".into());
    }
    Ok(p.display().to_string())
}

/// 跑 `1so scan` 生成体检面板。`name` 必填，`region`（地区）可选、让地图渠道更准。
/// 产物落 ~/.uking/geo/<公司名安全化>/ （每家一个项目目录，互不污染）。
///
/// 纯离线自查模式（不带 --auto，不需 LLM、不联网）。scan 很快（生成 HTML），
/// 但 lib.rs 仍以 spawn_blocking 转调，别卡 UI 主线程（守「同步命令别冻 UI」）。
pub fn run_scan(name: &str, region: &str) -> Result<GeoScan, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("请先填公司名".into());
    }
    crate::ulog::section("geo", &format!("体检开始 name={name} region={region}"));
    // ensure_skill 失败是最常见的一类（技能包没释放 / node 找不到），单独留痕。
    let entry = ensure_skill().inspect_err(|e| crate::ulog::write("geo", &format!("技能包准备失败：{e}")))?;

    let proj = uking_dir().join("geo").join(safe_dir(name));
    std::fs::create_dir_all(&proj).map_err(|e| format!("建体检目录失败: {e}"))?;

    let node = node_program();
    let entry_s = entry.display().to_string();
    let proj_s = proj.display().to_string();
    let region = region.trim();

    let mut args: Vec<&str> = vec![
        entry_s.as_str(),
        "scan",
        "--name",
        name,
        "--json",
        "--quiet",
        "--project",
        proj_s.as_str(),
    ];
    if !region.is_empty() {
        args.push("--region");
        args.push(region);
    }

    let mut c = Command::new(&node);
    c.args(&args);
    inject_path(&mut c);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }

    let out = c
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("启动 node 失败（是不是没装 Node？）: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    // 契约 JSON 走 stdout（--json）。取最后一个 { 开头的行最稳（前面可能混少量日志）。
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| {
            let err = String::from_utf8_lossy(&out.stderr);
            format!("体检没有返回结果：{}", tail(&err, 240))
        })?;

    let v: serde_json::Value =
        serde_json::from_str(line.trim()).map_err(|e| format!("解析体检输出失败: {e}"))?;
    if !v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
        let msg = v
            .get("error")
            .and_then(|s| s.as_str())
            .unwrap_or("体检失败");
        return Err(msg.to_string());
    }

    Ok(GeoScan {
        panel: v
            .get("panel")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string(),
        channels: v.get("channels").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        auto_ran: v.get("autoRan").and_then(|b| b.as_bool()).unwrap_or(false),
    })
}

/// 🔴 `GeoAicheck` / `run_aicheck` 和 `GeoInspect` / `run_inspect` 2026-08-24 删除（用户拍板）。
///
/// 它们跑的是 `1so aicheck` / `1so inspect`，而这两个脚本连同 `llm.mjs` 已经不随客户端发布
/// （见上面 `SKILL_FILES` / `REMOVED_FILES`）—— 留着函数只会得到一个「调起来必然失败」的死实现，
/// 而项目里正好有一笔同形的债还没还（需求榜 B6：只有测试在用的死函数）。
///
/// 🔴 **能力没删**：完整技能包仍在仓库 `src-tauri/skills/1so-geo/`，我们人工给客户出报告用的就是它。
/// 要恢复：把文件加回 SKILL_FILES、从 REMOVED_FILES 摘掉、`git show` 这个 commit 取回本段代码。


/// 公司名 → 安全文件夹名（中文保留，非字母数字转 _，截断防超长）。
fn safe_dir(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    cleaned.chars().take(40).collect()
}

/// 取字符串尾 n 个「字符」（按 char，非字节，防中文切一半 panic —— release 是 panic=abort）。
fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.trim().chars().collect();
    if chars.len() <= n {
        chars.into_iter().collect()
    } else {
        chars[chars.len() - n..].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🔴 **客户端发出去的东西里不许有会调模型的路径。**
    ///
    /// 这条用例守的是钱：`1so-geo/src/llm.mjs` 会自己读 `~/.uking/device.json` 拿虾盘云 Key，
    /// 所以只要它随 exe 发到客户机上，命令行就能调起 aicheck 烧我们的额度 ——
    /// 而我们这边零信号。GUI 摘按钮挡不住这条，唯一挡得住的是不发这些文件。
    #[test]
    fn shipped_skill_never_contains_model_calling_code() {
        for (rel, _) in SKILL_FILES {
            assert!(
                !rel.contains("llm.mjs"),
                "llm.mjs 不许进客户端发布清单：它自己会读 device.json 拿我们的 Key"
            );
            for banned in ["aicheck", "ingest", "detect", "generate", "optimize", "questions"] {
                assert!(
                    !rel.contains(banned),
                    "会调模型的命令 {banned} 不许进客户端发布清单（{rel}）"
                );
            }
        }
    }

    /// 发布清单里剩下的 .mjs **不许静态 import llm.mjs** —— 否则加载即失败，
    /// 离线自查会跟着一起废掉（而它是这一页唯一真跑的东西）。
    /// 动态 `import("./llm.mjs")` 是允许的：文件不在时它只是走进「没随客户端发布」那条分支。
    #[test]
    fn shipped_scripts_do_not_statically_import_llm() {
        for (rel, content) in SKILL_FILES {
            if !rel.ends_with(".mjs") {
                continue;
            }
            for line in content.lines() {
                let l = line.trim_start();
                // 静态 import 的形状：行首 `import ... from "./llm.mjs"`（动态的写作 `await import(`）
                if l.starts_with("import ") && l.contains("llm.mjs") {
                    panic!("{rel} 静态 import 了 llm.mjs：{l}\n改成动态 import，否则客户端缺文件时整个 CLI 加载失败");
                }
            }
        }
    }

    /// 从发布清单里拿掉的文件，**必须**同时列进 `REMOVED_FILES` ——
    /// 否则老客户机上那份会永远留着（`ensure_skill` 只写不删），等于什么都没堵。
    #[test]
    fn unshipped_model_files_are_scheduled_for_deletion() {
        for f in [
            "src/llm.mjs",
            "src/commands/aicheck.mjs",
            "src/commands/inspect.mjs",
            "src/commands/ingest.mjs",
            "src/commands/detect.mjs",
            "src/commands/generate.mjs",
            "src/commands/optimize.mjs",
            "src/commands/questions.mjs",
        ] {
            assert!(
                REMOVED_FILES.contains(&f),
                "{f} 不发了，但没进 REMOVED_FILES —— 已装机的老客户那份还在，口子照样开着"
            );
            assert!(
                !SKILL_FILES.iter().any(|(rel, _)| *rel == f),
                "{f} 同时出现在 SKILL_FILES 和 REMOVED_FILES：会被写下去又删掉，自相矛盾"
            );
        }
    }

    /// 样板报告必须是**静态且自证是演示**的：带演示横幅、不含 JS、不含指向充值页的链接。
    #[test]
    fn sample_report_is_static_and_self_declares_demo() {
        let html = SKILL_FILES
            .iter()
            .find(|(rel, _)| rel.contains("样板报告"))
            .map(|(_, c)| *c)
            .expect("样板报告必须随客户端发布");
        assert!(html.contains("演示样例") || html.contains("虚构"), "样板报告必须自证是演示数据");
        assert!(!html.contains("<script"), "样板报告不许带 JS（Mac 上用 Safari 打开）");
        assert!(!html.contains("recharge"), "样板报告不许留自助充值/下单链接");
    }
}
