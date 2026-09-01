//! 竞技场（Arena）—— 同任务横向比「谁干活利索」。
//!
//! ## 评分口径（需求榜 A 条已定死）
//! 系统**只出可观测量**：耗时 / 退出码 / 改了哪些文件 / 有没有真产出。质量那一栏
//! **只由人打星** —— 让系统判质量 = 重演 `cli2work` 那次自建判分器把自己的实现
//! 偏好变成评分标准的教训（查裸串「1202」惩罚了写「1,202万元」的，n=1 不可信）。
//!
//! ## 为什么不是影核动作
//! 一跑就烧 token 且非幂等，跟「立即运行一次」同类 —— 按既有规矩**不进动作表**。
//! 但没有 conformance 兜底，所以单开 `--arena-test` 无头跑道。
//!
//! ## 独立可插拔
//! 纯函数，不碰 `AppHandle`；复用 `toolprobe` 的 `resolve_exe` / `path_env` / `kill_tree`
//! （宪法第 12 条复用不复制）。删掉本模块只需动 lib.rs 两处 + 前端。
//!
//! ## 参赛名单
//! 只收**有可靠无头入口**的 CLI（toolprobe 实测过的写法）：claude / codex / hermes /
//! pi / qwen / crush。**openclaw 不带**（无头入口不可靠，实测 `infer model run` 走另一套
//! catalog）；**opencode 不带**（`run` 子命令在本机恒定挂满超时，见 providers.rs 注释）。
//! 每个工具在工作副本里独立跑，互不干扰。

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// 单个参赛者跑一次的死线。真任务比「回一句标记」慢得多（可能要先装依赖/改多个文件），
/// 给足；到点没完 = 对客户而言就是卡死，如实记 `timed_out`，不让它无限占着。
const TIMEOUT: Duration = Duration::from_secs(600);

/// 竞技场参赛名单。顺序即展示顺序。每条都是 toolprobe 验证过无头能跑的写法。
pub const ARENA_TOOLS: &[&str] = &["claude", "codex", "hermes", "pi", "qwen", "crush"];

/// 单个参赛者的结果。**全部是可观测量**，没有「质量分」——质量由人打星（前端的事）。
#[derive(Debug, Clone, Serialize)]
pub struct ArenaResult {
    pub tool: String,
    /// 命令在不在 PATH 上。没装不跑，`note` 写明「未安装」——不是坏，两码事。
    pub installed: bool,
    /// 真跑了吗。false = 没装（或没给这个工具）。
    pub ran: bool,
    /// 进程自己退出还是被超时杀。超时 = 对客户而言卡死，与正常退出不同账。
    pub timed_out: bool,
    /// 退出码（正常退出时）。超时/起不来为 null。
    pub exit_code: Option<i32>,
    /// 墙钟耗时（ms）。
    pub ms: u64,
    /// 有没有真产出（stdout 非空）。「空回」= 问了白问，是另一种失败。
    pub produced: bool,
    /// stdout 尾部（截断，够看结论不塞爆）。
    pub stdout_tail: String,
    /// 失败原因（已截断，不含 Key）。成功时为空。
    pub note: String,
}

/// 一次比试：六个参赛者，塞同一个任务，各自收尾。
///
/// `task` 是给每个 CLI 的同一个提示词；`workspace` 是每个参赛者独立的工作副本根目录
/// （**不直接共享一个目录**——六个 agent 同时改同一份文件会互相踩，比出来的全是乱的）。
/// `only` 限名单（无头验证用）；`emit` 收进度（CLI 打到 stderr）。
pub fn run_arena(
    task: &str,
    workspace: &Path,
    only: Option<&str>,
    emit: &dyn Fn(&str),
) -> Vec<ArenaResult> {
    // 每个参赛者一个独立子目录（<workspace>/arena/<tool>/）。
    let root = workspace.join("arena");
    let _ = std::fs::create_dir_all(&root);
    let mut out = Vec::new();
    let wanted = parse_only(only);
    // `only` 里指定了但不在参赛名单里的名字 → 如实报「不在名单」，不静默吞掉。
    // （调用方勾了一个竞技场不认的 CLI，就该看到一条记录而不是「什么都没发生」。）
    if let Some(w) = &wanted {
        for name in w {
            if !ARENA_TOOLS.contains(name) {
                out.push(ArenaResult {
                    tool: (*name).to_string(),
                    installed: false,
                    ran: false,
                    timed_out: false,
                    exit_code: None,
                    ms: 0,
                    produced: false,
                    stdout_tail: String::new(),
                    note: "不在竞技场参赛名单（无可靠无头入口或未上架）".into(),
                });
            }
        }
    }
    for tool in ARENA_TOOLS {
        if let Some(w) = &wanted {
            if !w.contains(tool) {
                continue;
            }
        }
        let exe = crate::toolprobe::resolve_exe(tool);
        let Some(exe) = exe else {
            emit(&format!("{tool}: 未安装，跳过"));
            out.push(ArenaResult {
                tool: (*tool).into(),
                installed: false,
                ran: false,
                timed_out: false,
                exit_code: None,
                ms: 0,
                produced: false,
                stdout_tail: String::new(),
                note: "未安装（不是故障）".into(),
            });
            continue;
        };
        emit(&format!("{tool}: 开跑…"));
        let dir = root.join(tool);
        let _ = std::fs::create_dir_all(&dir);
        let r = run_one(tool, &exe, task, &dir, &|m| emit(&format!("  {tool}: {m}")));
        let note = match r {
            Ok(v) => {
                let produced = !v.stdout.trim().is_empty();
                emit(&format!(
                    "{tool}: {} ({}ms){}",
                    if v.exit_code == Some(0) && produced { "✓" } else { "✗" },
                    v.ms,
                    if v.exit_code == Some(0) { String::new() } else { format!(" 退出码 {:?}", v.exit_code) }
                ));
                out.push(ArenaResult {
                    tool: (*tool).into(),
                    installed: true,
                    ran: true,
                    timed_out: v.timed_out,
                    exit_code: v.exit_code,
                    ms: v.ms,
                    produced,
                    stdout_tail: tail(&v.stdout, 400),
                    note: String::new(),
                });
                continue;
            }
            Err(e) => e,
        };
        out.push(ArenaResult {
            tool: (*tool).into(),
            installed: true,
            ran: false,
            timed_out: false,
            exit_code: None,
            ms: 0,
            produced: false,
            stdout_tail: String::new(),
            note,
        });
    }
    out
}

struct OneRun {
    exit_code: Option<i32>,
    timed_out: bool,
    ms: u64,
    stdout: String,
}

/// 解析 `only` 名单：支持逗号分隔的多选（前端勾选多个传 `"claude,codex"`）。
/// 空串 / None = 全跑。抽出纯函数是为了测试名单过滤不真跑 CLI。
fn parse_only(only: Option<&str>) -> Option<Vec<&str>> {
    only
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.split(',').map(str::trim).filter(|t| !t.is_empty()).collect())
}

/// 每个工具的无头参数（toolprobe 实测过能跑的形态）。`None` = 不认识的工具。
/// 抽成纯函数是为了测试时只验参数表、不真跑 CLI（竞技场一跑就烧 token）。
fn args_for(tool: &str, task: &str) -> Option<Vec<String>> {
    let args: Vec<String> = match tool {
        "claude" => vec!["-p".into(), task.into()],
        "codex" => vec!["exec".into(), task.into(), "--skip-git-repo-check".into()],
        "hermes" => vec!["-z".into(), task.into()],
        "pi" => vec!["-p".into(), task.into()],
        "qwen" => vec!["-p".into(), task.into()],
        "crush" => vec!["run".into(), task.into()],
        _ => return None,
    };
    Some(args)
}

/// 单个参赛者：spawn → 边读边收 → 到点杀树。复用 toolprobe 的路径/杀树能力。
fn run_one(
    tool: &str,
    exe: &Path,
    task: &str,
    dir: &Path,
    emit: &dyn Fn(&str),
) -> Result<OneRun, String> {
    let args = args_for(tool, task).ok_or_else(|| format!("未知工具：{tool}"))?;
    let mut c = Command::new(exe);
    c.args(&args);
    c.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", crate::toolprobe::path_env());
    // 抑制分页器/交互提示：否则 agent 跑 git 之类的命令会卡在 less 上干等到超时。
    for (k, v) in [("PAGER", "cat"), ("GIT_PAGER", "cat"), ("GIT_TERMINAL_PROMPT", "0"), ("NO_COLOR", "1")] {
        c.env(k, v);
    }
    // 委派编程的 CLI 继承虾盘云端点 + 设备 Key（同 agent/chat.rs 的 run_shell）：
    // 让竞技场里的每个 agent **免客户单独配置**直接用同一套计费。
    if let Ok(key) = crate::device::device_key_offline() {
        for (k, v) in crate::providers::delegation_env(&key) {
            c.env(k, v);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = c.spawn().map_err(|e| format!("启动失败：{e}"))?;

    // stdout/stderr 必须并发读走：管道写满没人读，子进程会阻塞在写上，
    // 于是它永远不退出、我们永远等不到 —— 把死锁误判成「这个工具很慢」。
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let h_out = std::thread::spawn(move || read_all(&mut so));
    let h_err = std::thread::spawn(move || read_all(&mut se));

    let started = Instant::now();
    let mut exit_code: Option<i32> = None;
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                exit_code = s.code();
                break false;
            }
            Ok(None) => {
                if started.elapsed() >= TIMEOUT {
                    break true;
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(e) => {
                emit(&format!("等待失败：{e}"));
                break false;
            }
        }
    };
    if timed_out {
        emit(&format!("超过 {}s 超时，杀掉整棵进程树", TIMEOUT.as_secs()));
        crate::toolprobe::kill_tree(&mut child);
    }
    let stdout = h_out.join().unwrap_or_default();
    let _stderr = h_err.join().unwrap_or_default();
    let ms = started.elapsed().as_millis() as u64;
    Ok(OneRun { exit_code, timed_out, ms, stdout })
}

fn read_all<R: Read>(r: &mut Option<R>) -> String {
    let Some(r) = r.as_mut() else {
        return String::new();
    };
    let mut buf = Vec::new();
    let _ = r.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn tail(s: &str, n: usize) -> String {
    let t = s.trim();
    let t: String = t.chars().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect();
    t.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参赛名单不该带无头入口不可靠的工具（openclaw / opencode）。
    /// 带了就是拿一个必挂的选手去凑数，比试结果全是噪音。
    #[test]
    fn arena_roster_excludes_unprobeable_tools() {
        assert!(!ARENA_TOOLS.contains(&"openclaw"), "openclaw 无头入口不可靠，不该进竞技场");
        assert!(!ARENA_TOOLS.contains(&"opencode"), "opencode run 在本机必挂，不该进竞技场");
    }

    /// 名单里的每个工具都有无头参数（`args_for` 不返回 None）。
    /// 只验参数表不真跑 —— 竞技场一跑就烧 token，测试不能点火。
    #[test]
    fn every_roster_tool_has_a_command_shape() {
        for tool in ARENA_TOOLS {
            let args = args_for(tool, "hi");
            assert!(args.is_some(), "{} 没有无头参数表（漏了 args_for 分发）", tool);
            let args = args.unwrap();
            assert!(!args.is_empty(), "{} 的参数表不该是空的", tool);
        }
    }

    /// 名单外的工具（openclaw/opencode 那种无头不可靠的）不应有参数表 ——
    /// 真有的话 run_one 会真跑它，而它是必挂的选手。
    #[test]
    fn tools_outside_roster_have_no_command() {
        for tool in ["openclaw", "opencode", "nope"] {
            assert!(args_for(tool, "hi").is_none(), "{} 不该有参数表（不在参赛名单里）", tool);
        }
    }

    /// `only` 名单解析：逗号分隔多选 / 空 / None 三种形态。
    #[test]
    fn only_list_parses_comma_separated_and_empty() {
        assert_eq!(parse_only(None), None, "None = 全跑");
        assert_eq!(parse_only(Some("")), None, "空串 = 全跑");
        assert_eq!(parse_only(Some("claude, codex")), Some(vec!["claude", "codex"]), "逗号+空格都要吞");
        // 前端勾选多个 → 名单含它们；前端不认识的串也会原样进名单（过滤在 run_arena 按 ARENA_TOOLS 做）
        assert_eq!(parse_only(Some("claude,nope")), Some(vec!["claude", "nope"]));
    }

    /// 名单里带一个未安装的工具不该影响其它参赛者 —— 过滤逻辑按工具各自判。
    #[test]
    fn wanted_filter_is_per_tool() {
        let wanted = parse_only(Some("claude,crush")).unwrap();
        assert!(wanted.contains(&"claude"));
        assert!(wanted.contains(&"crush"));
        assert!(!wanted.contains(&"hermes"), "没勾的不能混进来");
    }

    /// 名单外名字 → 如实报「不在名单」记录，不静默吞掉（--arena-test 靠它零烧 token 验骨架）。
    #[test]
    fn unknown_only_name_reports_not_in_roster() {
        let root = std::env::temp_dir().join(format!("uking-arena-unknown-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let results = run_arena("只读任务", &root, Some("__nonexistent__"), &|_| {});
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(results.len(), 1, "名单外名字应恰好返回一条记录");
        assert_eq!(results[0].tool, "__nonexistent__");
        assert!(!results[0].installed && !results[0].ran, "名单外 = 未安装未跑");
        assert!(results[0].note.contains("不在竞技场参赛名单"), "note 要说明原因：{}", results[0].note);
    }
}
