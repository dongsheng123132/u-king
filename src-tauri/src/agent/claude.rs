//! Claude Code stream-json 驱动 —— 把 claude 的结构化事件流解析成 Codex 式卡片。
//!
//! ## 为什么不走 PTY
//! 终端面板（term.rs）已经把 claude 的 TUI 原样渲染了。这里要的是**结构化**：plan 清单、
//! 工具卡片、内联 diff、token 用量。靠 `claude --output-format stream-json` 吐 JSON 事件流，
//! Rust 逐行解析成统一事件，经 Tauri Channel 推前端，React 渲染成卡片。与终端面板并存。
//!
//! ## 进程模型（MVP：一轮一进程 + --resume 续接）
//! claude `-p`（print）模式是一次性的：跑完一轮就退出。多轮对话靠 `--resume <session_id>`
//! 续接上一轮的 session（首轮没有 session_id）。这避免了 stdin 双向管道的复杂度，纯 std 即可。
//! 一个任务记住自己最近的 session_id（HashMap<task_id, last_session_id>）。
//!
//! ## 零依赖
//! 纯 std Command + 线程读管道 + serde_json。PATH 复用 installer::search_paths（让 claude 可解析）。
//! release 是 panic=abort：reader 线程热路径零 unwrap。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;
use tauri::ipc::Channel;

use crate::installer::{portable_node_dir, search_paths};

use super::cmdline;
use super::launcher::{self, Launcher};
use super::protocol::ProtocolState;
use super::{TurnLog, Watchdog, GUARD_PROMPT, STALL_SECS};

/// 这个大脑在 `threads` 那份落盘表里的名字。
const AGENT: &str = "claude";

/// 正在运行的 claude 子进程（用于中断）。task_id -> Child。
fn running() -> &'static Mutex<HashMap<String, Child>> {
    static R: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 起一条命令。**解析交给 `launcher`**：Windows 上 npm 装的 `claude` 是 `.cmd` 批处理壳，
/// 直接 spawn 它会让所有含换行的参数（多行提问 / 专家 persona）被 std 拒掉
/// —— 详见 `launcher.rs` 顶部。这里拿到的 `Launcher` 已经是「node + cli.js」那条安全路径。
fn base_command(l: &Launcher) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(&l.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.args(&l.prefix);
    c
}

/// 前置便携工具目录（与 installer / term 同口径），让 `claude` 可解析。
///
/// `delegated` = 这一轮是不是走虾盘云委托（`providers::delegation_env` 注了 env）。
/// 两者对代理的诉求**相反**，2026-08-24 起分道扬镳（根因见 `sysproxy.rs` 模块注释）：
/// - 委托轮连国内镜像 api.u-claw.org.cn —— **清代理**是对的（老行为保留），
///   客户机的 clash 会把镜像流量也劫去国外，反而连不上。
/// - 自持凭据轮直连官方 api.anthropic.com —— 大陆裸连必被 403 地域拦截
///   （「8 天 9 连败」的真凶）。GUI 双击启动没有 shell 的代理变量可继承，
///   这里把 Windows 系统代理翻成 HTTP(S)_PROXY 补进去；只填缺失键，不覆盖显式设置。
fn inject_path(c: &mut std::process::Command, delegated: bool) {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let dirs = search_paths(portable_node_dir().as_deref());
    if !dirs.is_empty() {
        let prefix = dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(sep);
        let old = std::env::var("PATH").unwrap_or_default();
        c.env("PATH", format!("{prefix}{sep}{old}"));
    }
    if delegated {
        // 清代理（历史行为）：避免客户机 clash 把虾盘云国内镜像劫到国外出口
        for v in [
            "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY",
            "http_proxy", "https_proxy", "all_proxy",
            "NO_PROXY", "no_proxy",
        ] {
            c.env_remove(v);
        }
        return;
    }
    // 自持凭据直连官方：补系统代理。用户/宿主已显式设过的键一律尊重原值。
    for (k, v) in crate::installer::proxy_env_for(false) {
        if std::env::var_os(&k).is_none() {
            c.env(k, v);
        }
    }
}

/// 这一轮是不是虾盘云委托：按**这个工具自己**的凭据状态判定（见 providers::claude_delegated）。
fn delegated_turn() -> bool {
    crate::providers::claude_delegated()
}

/// 给一个任务发一条消息，跑一轮 claude，结构化事件经 `on_event` Channel 流回前端。
///
/// - `task_id`：工作台任务 id（同一任务多轮会 --resume）
/// - `prompt`：用户消息
/// - `cwd`：任务文件夹（claude 在此目录工作）
/// - `model`：可选模型覆盖
#[tauri::command]
pub async fn claude_send(
    task_id: String,
    prompt: String,
    cwd: Option<String>,
    model: Option<String>,
    system: Option<String>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    // 在阻塞线程里跑，避免卡 async runtime
    tauri::async_runtime::spawn_blocking(move || run_turn(task_id, prompt, cwd, model, system, on_event))
        .await
        .map_err(|e| format!("claude 任务调度失败: {e}"))?
}

/// 只为渲染卡片而加的参数 —— 这些**绝不能**出现在教给用户的终端命令里。
/// （测试拿它当断言清单：以后谁往真实 argv 里加了新的 GUI 专用 flag 又顺手抄进 teach，这里会红。）
const GUI_ONLY_FLAGS: &[&str] = &[
    "--output-format",
    "stream-json",
    "--include-partial-messages",
    "--verbose",
    "--permission-mode",
    "--append-system-prompt",
    "-p",
];

/// 拼这一轮的 argv。返回 `(真实 argv, 终端交互式等价写法, 提示词有没有内联进 teach)`。
///
/// 拆成纯函数是为了能被测到 —— 「看命令」卖的就是可信，teach 里混进一个 GUI 专用参数，
/// 客户照着敲跑不通，这个功能立刻从加分项变成减分项。
///
/// 真实 argv 一字不差地既执行又展示（同一事实只有一份，宪法第 8 条）；teach 故意**不带**
/// 上面 `GUI_ONLY_FLAGS` 里的东西，也不带 persona（太长，贴进终端没法看）。
fn build_args(
    prompt: &str,
    model: Option<&str>,
    system: Option<&str>,
    resume: Option<&str>,
) -> (Vec<String>, Vec<String>, bool) {
    let model = model.filter(|m| !m.trim().is_empty());
    let mut args: Vec<String> = vec![
        "--output-format".into(), "stream-json".into(),
        "--include-partial-messages".into(),
        "--verbose".into(),
        "--permission-mode".into(), "bypassPermissions".into(), // MVP 先免审批
    ];
    // 运行环境约束 + 专家 persona：append 到 Claude 自己的系统提示后（不覆盖 Claude Code 默认行为）。
    // GUARD 是**无条件**的 —— 没选专家的裸对话才是绝大多数客户走的那条路，防死锁的规矩正是他们最需要的。
    let sys = match system.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{GUARD_PROMPT}\n\n---\n\n{s}"),
        None => GUARD_PROMPT.to_string(),
    };
    args.push("--append-system-prompt".into());
    args.push(sys);
    if let Some(m) = model {
        args.push("--model".into());
        args.push(m.to_string());
    }
    if let Some(sid) = resume {
        args.push("--resume".into());
        args.push(sid.to_string());
    }
    args.push("-p".into());
    args.push(prompt.to_string());

    let mut teach: Vec<String> = Vec::new();
    if let Some(m) = model {
        teach.push("--model".into());
        teach.push(m.to_string());
    }
    if let Some(sid) = resume {
        teach.push("--resume".into()); // 交互模式也认这个 flag，是真的能续上
        teach.push(sid.to_string());
    }
    let inlined = cmdline::inlineable(prompt);
    if inlined {
        teach.push(prompt.to_string());
    }
    (args, teach, inlined)
}

/// 无人值守地跑一次已装的 AI CLI（`claude -p "…"` / `codex exec "…"`），把输出收回来。
/// 给「自动化（定时任务）」用 —— 没有人在旁边看，所以要么拿到结果，要么拿到人话错误。
///
/// **直接 spawn argv、不经 `cmd /C`**：定时任务的提示词是客户随手写的，里面可能有引号、
/// 换行、`&`。走 shell 就得自己拼转义，那正是历史上「cmd /C 吃引号 → skill 静默误执行」
/// 那个坑。argv 交给 CreateProcess，没有第二次解析。
///
/// PATH / 虾盘云 env / 进程树清理全部复用本模块和 chat.rs 里已有的那份 ——
/// 交互式那条路和定时这条路必须同口径，否则会出现「GUI 里能跑、到点跑不了」。
pub fn run_oneshot(
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    use std::io::Read;

    let l = launcher::resolve(program);
    let mut c = base_command(&l);
    if let Ok(key) = crate::device::device_key_offline() {
        for (k, v) in crate::providers::delegation_env(&key) {
            c.env(k, v);
        }
    }
    c.args(args);
    if let Some(d) = cwd.filter(|p| std::path::Path::new(p).is_dir()) {
        c.current_dir(d);
    }
    inject_path(&mut c, delegated_turn());
    // 无人值守：任何等人回答的东西都会挂到超时。分页器/凭据提示/彩色全按掉（同 chat.rs::run_shell）。
    for (k, v) in [
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_EDITOR", "true"),
        ("NO_COLOR", "1"),
        ("TERM", "dumb"),
    ] {
        c.env(k, v);
    }
    c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let mut child = c
        .spawn()
        .map_err(|e| format!("起不来 {program}（这台机器装了吗？）: {e}"))?;
    let pid = child.id();

    // 必须**边跑边读**：管道写满会把子进程堵死，然后我们只会看到「超时」这个假象。
    let buf = std::sync::Arc::new(Mutex::new(String::new()));
    let mut readers = Vec::new();
    for pipe in [
        child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
        child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
    ] {
        if let Some(mut p) = pipe {
            let b = buf.clone();
            readers.push(std::thread::spawn(move || {
                let mut tmp = [0u8; 4096];
                loop {
                    match p.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut g) = b.lock() {
                                if g.len() < 200_000 {
                                    g.push_str(&String::from_utf8_lossy(&tmp[..n]));
                                }
                            }
                        }
                    }
                }
            }));
        }
    }

    let start = std::time::Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if start.elapsed() > std::time::Duration::from_secs(timeout_secs) {
            // 整棵树一起收：`claude.cmd` 会派生 node，裸 kill 只杀壳会留孤儿
            super::chat::kill_tree_by_pid(pid);
            let _ = child.kill();
            timed_out = true;
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    for h in readers {
        let _ = h.join();
    }

    // 首+尾截断：只留头会把结尾的报错吞掉，而报错才是我们要给客户看的那部分
    let body = super::chat::head_tail(&buf.lock().map(|g| g.clone()).unwrap_or_default(), 12_000);
    if timed_out {
        return Err(format!("{program} 跑了 {timeout_secs}s 还没结束，已终止\n{body}"));
    }
    match status {
        Some(s) if s.success() => Ok(body.trim().to_string()),
        Some(s) => Err(format!("{program} 退出码 {}\n{body}", s.code().unwrap_or(-1))),
        None => Err(format!("{program} 执行异常\n{body}")),
    }
}

fn run_turn(
    task_id: String,
    prompt: String,
    cwd: Option<String>,
    model: Option<String>,
    system: Option<String>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    // 取上一轮 session_id（有则 --resume 续接）。**从盘上取** —— 关掉 U-King 再打开、
    // 甚至重启电脑，这个会话依旧接着上文说。以前记在进程内存里，界面上会话和聊天记录
    // 都还在、只有模型不知道「刚才」是什么，看起来像 AI 变笨了。见 `threads.rs`。
    let resume = super::threads::recall(AGENT, &task_id);

    // 🔴 **续接前先看这个会话在不在这个目录下**（2026-08-18 客户实拍：
    // 「一旦我中途切换文件夹，就会出现报错」，底层命令 `claude --resume <sid>`
    // 回 `No conversation found with session ID: …`）。
    //
    // 根因：**Claude Code 的会话是按工作目录存的** —— `~/.claude/projects/<目录转义>/<sid>.jsonl`。
    // 在 A 目录建的 sid，换到 B 目录再 `--resume` 就是找不到。而我们只按 task_id 记 sid，
    // 不记它是在哪个目录建的，于是换目录后每一轮都必炸。
    //
    // 拿一个必然找不到的 sid 去 resume，换来的是一条客户看不懂的英文报错；
    // 事前判一下，最差也只是「这一轮不接上文」—— 而且**会明说**，不闷着。
    let (resume, thread_note) = match resume {
        Some(sid) => match claude_session_dir_of(&sid) {
            // 找到了，而且就在当前目录下 → 正常续接
            Some(dir) if Some(&dir) == cwd.as_deref().map(slug_project_dir).as_ref() => (Some(sid), None),
            // 找到了，但在别的目录下 → 换过文件夹。别拿它去撞墙
            Some(_) => (None, Some("上一轮的会话是在**另一个文件夹**里建的，Claude Code 的对话按目录分开存 —— 这一轮从头开始（之前的记录还在，换回原来那个文件夹就能接上）。".to_string())),
            // 整个 projects 目录里都没有这个 sid → 陈旧（历史被清过 / 手动删过）
            None if claude_projects_root().is_dir() => (None, Some("上一轮的会话记录找不到了（多半是清理过 Claude Code 的历史）—— 这一轮从头开始。".to_string())),
            // 读不出来就别猜，保持原行为
            None => (Some(sid), None),
        },
        None => (None, None),
    };
    if let Some(n) = &thread_note {
        let _ = on_event.send(serde_json::json!({ "kind": "text", "text": n }));
    }

    let mut tlog = TurnLog::start("chat", "claude", &task_id, model.as_deref(), resume.is_some());

    let l = launcher::resolve("claude");
    let mut c = base_command(&l);
    // 注入虾盘云端点 + 设备 Key：让 Claude Code 大脑**免客户单独配置**直接用同一套计费
    //（同 agent/chat.rs 委派 run_command 的做法）。仅子进程 env，不碰用户 ~/.claude 文件。
    if let Ok(key) = crate::device::device_key_offline() {
        for (k, v) in crate::providers::delegation_env(&key) {
            c.env(k, v);
        }
    }
    // ★ **北桥：把任务本象编译进系统提示。**
    //
    // 这是 2Origin 里「状态 → Context」那条总线在 U-King 的落点。没有它，
    // 状态存了也白存 —— 存下来只有人能看，模型看不见，接手方照样得回头问人。
    //
    // 挂在 `--append-system-prompt` 而不是拼进用户 prompt：用户那句话是**这一轮**要干的事，
    // 状态是**这个任务一直以来**的样子，两者混在一起会让模型分不清「现在要我做什么」。
    //
    // 没有本象就一个字都不加 —— 绝大多数会话没有状态，不该为它们付一份空模板的 token。
    let system = match crate::origin::load(&task_id) {
        Some(o) => {
            let ctx = o.compile_context();
            Some(match system.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => format!("{ctx}\n---\n\n{s}"),
                None => ctx,
            })
        }
        None => system,
    };
    let (args, teach, inlined) = build_args(&prompt, model.as_deref(), system.as_deref(), resume.as_deref());
    c.args(&args);

    // 「看命令」：真实命令 + 终端交互式等价写法，**在 spawn 之前**发 —— claude 没装时进程起不来，
    // 但用户照样能看到我们试图跑的是哪一条（排障时这行比任何报错都管用）。
    // 展示的是**解包后**真跑的那条（可能是 `node …\cli.js …`）—— 同一事实只有一份，
    // 而且它顺便回答了「这台机器上到底哪个 claude 在跑」，比 .cmd 壳的路径更有信息量。
    let _ = on_event.send(cmdline::event(&l.program, &l.full_args(&args), "claude", &teach, inlined));

    if let Some(d) = cwd.as_deref().filter(|p| std::path::Path::new(p).is_dir()) {
        c.current_dir(d);
    }
    inject_path(&mut c, delegated_turn());
    // 给 claude 自己的 Bash 工具设硬上限：**让它自己超时**，把「命令超时了」当成工具错误回给模型，
    // 模型会换个方式接着干，对话不断。这比等我们从外面把整个 claude 杀掉温和得多 ——
    // 外部 kill 是兜底（见 STALL_SECS），这里才是第一道闸。
    c.env("BASH_DEFAULT_TIMEOUT_MS", "120000"); // 常规命令 2 分钟
    c.env("BASH_MAX_TIMEOUT_MS", "180000"); // 模型主动加长也不许超过 3 分钟
    c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let mut child = c.spawn().map_err(|e| {
        // 起不来也要落一行：否则日志里只有一条孤零零的 "turn start"，
        // 排障的人会以为是卡在模型上，其实是这台机器压根没装 claude。
        tlog.finish("spawn_failed", None);
        format!("启动 claude 失败（是否已安装？）: {e}")
    })?;
    let pid = child.id();
    tlog.spawned(pid);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // 把 child 存进 running，供中断
    if let Ok(mut m) = running().lock() {
        // 同任务旧进程先清掉句柄（理论上不会有，保险）
        m.insert(task_id.clone(), child);
    }

    // 看门狗：静默超过 STALL_SECS 就把整棵进程树收掉（实现和理由见 `agent/mod.rs::Watchdog`）
    let wd = Watchdog::spawn(pid, Duration::from_secs(STALL_SECS));

    // stderr 收集（claude 的报错 / 非 JSON 噪声）
    let err_buf = std::sync::Arc::new(Mutex::new(String::new()));
    let err_store = err_buf.clone();
    let err_h = stderr.map(|se| {
        std::thread::spawn(move || {
            for line in BufReader::new(se).lines().map_while(Result::ok) {
                if let Ok(mut g) = err_store.lock() {
                    g.push_str(&line);
                    g.push('\n');
                }
            }
        })
    });

    // stdout 逐行解析 stream-json
    let mut state = ProtocolState::new(resume.is_some());
    if let Some(so) = stdout {
        for line in BufReader::new(so).lines().map_while(Result::ok) {
            wd.beat(); // 有一行就算「还活着」——空行/非 JSON 行也算
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue; // 非 JSON 行（极少）忽略
            };
            for ev in state.map_event(&v) {
                tlog.on_event(
                    ev.get("kind").and_then(|k| k.as_str()).unwrap_or(""),
                    ev.get("name").and_then(|n| n.as_str()),
                );
                // 记住 session_id 供下轮 --resume（落盘，见 threads.rs）
                if ev.get("kind").and_then(|k| k.as_str()) == Some("session") {
                    if let Some(sid) = ev.get("session_id").and_then(|s| s.as_str()) {
                        super::threads::remember(AGENT, &task_id, sid);
                    }
                }
                let _ = on_event.send(ev);
            }
        }
    }
    if let Some(h) = err_h {
        let _ = h.join();
    }
    wd.finish(); // 管道读完 = 这一轮已收尾，看门狗下班

    // 取回 child wait + 从 running 移除
    let child_opt = running().lock().ok().and_then(|mut m| m.remove(&task_id));
    let interrupted;
    let code;
    match child_opt {
        Some(mut ch) => {
            // 若已被 interrupt kill，wait 会立刻返回
            let status = ch.wait().map_err(|e| {
                tlog.finish("wait_failed", None);
                format!("等待 claude 失败: {e}")
            })?;
            code = status.code();
            interrupted = !status.success() && code.is_none();
        }
        None => {
            // 被中断移走了
            interrupted = true;
            code = None;
        }
    }

    // 收尾正文：stderr 优先，**为空时退到 stdout 上那条 result 的失败正文**。
    //
    // 🔴 只看 stderr 是一个真实故障：claude 的上游报错（403 / 余额不足 / 限流）全走
    // stdout 的 stream-json，stderr 一个字都没有 → message="" → 界面只显示
    // 「claude 退出码 1」→ 分类器一条规则都匹配不上 → 落到「我没认出这是哪种问题，
    // 多半是没装好 / 驱动没配对」。客户 2026-08-18「老用户不能用了」的真因是余额
    // 不够单次预扣（403 token quota is not enough），却被这句话支去重配驱动。
    // **原因当时就渲染在同一个屏幕上** —— 我们认得出，只是没喂给自己的分类器。
    let err_text = {
        let from_stderr = err_buf.lock().map(|g| g.trim().to_string()).unwrap_or_default();
        if from_stderr.is_empty() {
            state.result_error().unwrap_or_default().to_string()
        } else {
            from_stderr
        }
    };
    // 收尾事件：卡死/成功/中断/错误。
    // **卡死判定必须排在 interrupted 前面** —— 看门狗是靠杀进程收场的，进程状态上跟「人按了停止」
    // 长得一模一样。谁在前面谁定性，摆错顺序就会把「它挂了」报成「你停了它」，客户再也不会告诉我们。
    let done = if wd.stalled() {
        serde_json::json!({
            "kind": "done", "status": "timeout",
            "stall_secs": STALL_SECS,
        })
    } else if interrupted {
        serde_json::json!({ "kind": "done", "status": "interrupted" })
    } else if code == Some(0) {
        serde_json::json!({ "kind": "done", "status": "ok" })
    } else {
        serde_json::json!({
            "kind": "done", "status": "error",
            "code": code, "message": err_text
        })
    };
    // 日志里的定性**读同一个 done 事件**，不另起一套判断 —— 否则界面说「你停了它」、
    // 日志说「它挂了」，两边都不可信（同一事实只留一份，宪法第 8 条）。
    tlog.finish(
        done.get("status").and_then(|s| s.as_str()).unwrap_or("?"),
        code,
    );
    let _ = on_event.send(done);
    Ok(())
}

/// 中断某任务正在跑的 claude。
///
/// **杀整棵树，不是杀那一个进程**：claude 干活时会派生 shell、node、python…… 只 kill 它自己，
/// 那些孩子会变成孤儿继续跑（继续占端口、继续吃 CPU、继续写文件）。客户点了「停止」，
/// 界面上确实停了，机器上却没停 —— 那是在骗人。`run_oneshot` 早就用的 `kill_tree_by_pid`，
/// 交互这条路一直漏着（pc-*** 实锤：停下后 `serve --port 8790` 还活着）。
#[tauri::command]
pub fn claude_interrupt(task_id: String) -> Result<(), String> {
    if let Ok(mut m) = running().lock() {
        if let Some(mut ch) = m.remove(&task_id) {
            super::chat::kill_tree_by_pid(ch.id());
            let _ = ch.kill();
        }
    }
    Ok(())
}

/// 清掉某任务的多轮上下文（下次从新会话开始，不 --resume）。
/// 落盘那份也得清 —— 只清内存的话，重启后「新对话」会自己撤销。
#[tauri::command]
pub fn claude_reset(task_id: String) -> Result<(), String> {
    super::threads::forget(AGENT, &task_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「看命令」的核心承诺：教给用户的那条命令里，一个 GUI 专用参数都不许有。
    /// 破了这条 = 客户照着敲跑不通 = 这个功能不如不做。
    #[test]
    fn teach_never_leaks_gui_only_flags() {
        let persona = "你是一个建站专家".repeat(20);
        for (prompt, model, system, resume) in [
            ("写个脚本", None, None, None),
            ("写个脚本", Some("deepseek-v4-pro"), None, Some("sess-1")),
            ("写个脚本", Some("  "), Some(persona.as_str()), Some("sess-2")), // 空白 model 要当没传
            ("第一行\n第二行", None, Some(persona.as_str()), None),
        ] {
            let (args, teach, _) = build_args(prompt, model, system, resume);
            for flag in GUI_ONLY_FLAGS {
                assert!(!teach.iter().any(|a| a == flag), "teach 混进了 GUI 专用参数 {flag}: {teach:?}");
            }
            // persona 不该出现在 teach 里（太长，贴终端没法看）
            assert!(!teach.iter().any(|a| a.contains("建站专家")), "teach 不该带 persona: {teach:?}");
            // 真实 argv 反过来必须完整 —— 别为了好看把执行也阉了
            assert!(args.windows(2).any(|w| w[0] == "--output-format" && w[1] == "stream-json"));
            assert_eq!(args.last().map(String::as_str), Some(prompt), "真实 argv 末位必须是原样提示词");
            assert!(model.map(|m| m.trim().is_empty()).unwrap_or(true) == !args.iter().any(|a| a == "--model"));
        }
    }

    /// 防死锁的运行环境约束必须**无条件**带上 —— 没选专家的裸对话才是绝大多数客户走的那条路，
    /// 而那正是 pc-*** 挂死 25 分钟的场景。同时它绝不能漏进 teach：那行命令是给人贴进终端的，
    /// 糊一大段系统提示上去没法看，也没法敲。
    #[test]
    fn guard_prompt_always_appended_never_taught() {
        for system in [None, Some("你是一个建站专家"), Some("   ")] {
            let (args, teach, _) = build_args("帮我调试一下这个软件", None, system, None);
            let i = args
                .iter()
                .position(|a| a == "--append-system-prompt")
                .expect("运行环境约束必须无条件带上");
            let sys = &args[i + 1];
            assert!(sys.contains("不会自己结束的命令"), "防死锁那条规矩没进系统提示");
            if let Some(s) = system.map(str::trim).filter(|s| !s.is_empty()) {
                assert!(sys.contains(s), "专家 persona 被 GUARD 顶掉了");
            }
            assert!(!teach.iter().any(|a| a.contains("运行环境")), "GUARD 漏进 teach: {teach:?}");
        }
    }

    /// 多行提示词不许内联进 teach（粘进 shell 会断在半路），且要如实报告没内联。
    #[test]
    fn multiline_prompt_reported_as_not_inlined() {
        let (_, teach, inlined) = build_args("第一行\n第二行", None, None, None);
        assert!(!inlined);
        assert!(teach.is_empty());

        let (_, teach, inlined) = build_args("写个脚本", None, None, Some("s1"));
        assert!(inlined);
        assert_eq!(teach, vec!["--resume", "s1", "写个脚本"]);
    }
}

/* ---- Claude Code 的会话落盘布局（按目录分开存） ------------------------------
 *
 * `~/.claude/projects/<目录转义>/<session-id>.jsonl`
 * 目录转义 = 绝对路径里每个非字母数字字符换成 `-`
 * （实测 `C:\Users\me\AppData\Local\Temp` → `C--Users-me-AppData-Local-Temp`）。
 *
 * 🔴 **负例不靠这条转义规则**：CJK / 全角路径（客户机就是 `C:\Users\demo（无密码）`）
 * 上游怎么转义我们没实测过，照自己的规则算出来一个对不上的名字，
 * 会把「能接上」误判成「接不上」。所以判「在不在」用**扫目录找文件**（不依赖转义），
 * 只有判「是不是同一个目录」才用转义 —— 那一步算错最多是保守地不续接，不会误伤。
 * ------------------------------------------------------------------------- */

fn claude_projects_root() -> std::path::PathBuf {
    crate::installer::user_home_dir().join(".claude").join("projects")
}

/// 绝对路径 → Claude Code 的项目目录名。
fn slug_project_dir(p: &str) -> String {
    p.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// 这个 session id 的记录躺在哪个项目目录下？扫出来的是**目录名**，不是路径。
/// 返回 None = 整个 projects 里都没有它（或读不了）。
fn claude_session_dir_of(sid: &str) -> Option<String> {
    let want = format!("{sid}.jsonl");
    let rd = std::fs::read_dir(claude_projects_root()).ok()?;
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        if e.path().join(&want).is_file() {
            return Some(e.file_name().to_string_lossy().to_string());
        }
    }
    None
}

#[cfg(test)]
mod resume_guard_tests {
    use super::*;

    /// 转义规则对得上实测样本 —— 这条一旦漂了，「是不是同一个目录」就会全判错。
    #[test]
    fn slug_matches_observed_layout() {
        assert_eq!(
            slug_project_dir(r"C:\Users\me\AppData\Local\Temp"),
            "C--Users-me-AppData-Local-Temp"
        );
        assert_eq!(slug_project_dir(r"C:\tmp\mv"), "C--tmp-mv");
    }

    /// 🔴 **非 ASCII 路径不许 panic、也不许算出个空串** —— 客户机的用户名就是
    /// `demo（无密码）`。算得对不对我们没实测过（所以负例不靠它），但它必须是个
    /// 稳定的、长度对得上的名字，不能把整段中文吃掉。
    #[test]
    fn slug_handles_cjk_without_eating_it() {
        let s = slug_project_dir(r"C:\Users\demo（无密码）\Downloads");
        assert!(!s.is_empty());
        assert!(s.starts_with("C--Users-demo"));
        assert!(s.ends_with("Downloads"));
    }

    /// 代理分岔的**机制级**验证（2026-08-24「8 天 9 连败」根因修复的守卫）：
    /// 同一个构造函数，委托轮子进程必须**没有**代理变量，自持凭据轮必须**有**
    /// （前提：这台机器开了系统代理 —— 没开的机器两条都空，断言自动退化成恒等式）。
    ///
    /// 用真 `Command` + `get_envs()` 而不是 mock：要钉住的是「spawn 出去的世界长什么样」，
    /// 探针越真，将来有人把 env_remove 改回无条件、或把注入键名打错时这条越会当场变红。
    #[test]
    fn proxy_policy_splits_by_delegated() {
        // 委托轮：镜像直连，**预置进去的代理也必须被摘掉**（env_remove 的可观测语义：
        // get_envs 里该键还在，但显式值变成 None —— spawn 时子进程拿不到它）
        let mut dirty = std::process::Command::new("node");
        dirty.env("HTTP_PROXY", "http://junk:1").env("NO_PROXY", "junk");
        inject_path(&mut dirty, true);
        let val_of = |name: &str| -> Option<Option<String>> {
            dirty
                .get_envs()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.map(|x| x.to_string_lossy().to_string()))
        };
        assert_eq!(
            val_of("HTTP_PROXY"),
            Some(None),
            "委托轮没摘掉预置的 HTTP_PROXY（子进程会继承宿主代理，镜像流量会被劫走）"
        );
        assert_eq!(val_of("NO_PROXY"), Some(None), "委托轮没摘掉预置的 NO_PROXY");

        // 🔴 再把**宿主进程**的代理变量摘干净验注入侧：开发机 shell 十有八九带着代理，
        // 「只填缺失键」会如实跳过 —— 那是正确行为，不能让断言把它当失败。
        let saved = [
            "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy",
            "NO_PROXY", "no_proxy",
        ]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v)))
        .collect::<Vec<_>>();
        for (k, _) in &saved {
            std::env::remove_var(k);
        }
        let mut clean = std::process::Command::new("node");
        inject_path(&mut clean, false);
        let owned = clean
            .get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| (k.to_string_lossy().to_string(), v.to_os_string().to_string_lossy().to_string()))
            })
            .collect::<std::collections::HashMap<String, String>>();

        // 自持轮：系统代理开着就必须补进显式 env；没开（TUN 机器）则没有形状可言
        let sys = crate::installer::system_proxy_env();
        if !sys.is_empty() {
            for (k, v) in &sys {
                assert_eq!(
                    owned.get(k).map(String::as_str),
                    Some(v.as_str()),
                    "自持凭据轮缺系统代理 {k}（8 天 9 连败的根因就是它裸连官方被 403）"
                );
            }
        }
        // 只填缺失键的约定：宿主已设的值，注入侧必须**跳过**（显式 map 里不得出现系统代理的同名键）
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
        let mut guarded = std::process::Command::new("node");
        inject_path(&mut guarded, false);
        let guarded_has_https = guarded
            .get_envs()
            .any(|(k, _)| k.eq_ignore_ascii_case("HTTPS_PROXY"));
        assert!(!guarded_has_https, "宿主已设 HTTPS_PROXY 时注入侧必须跳过，不许覆盖");
        // 还原宿主现场（并行跑的其他测试可能依赖原值）。HTTPS_PROXY 是本测试自己设的，
        // 必须无条件摘掉 —— 在原本没有该变量的干净机/CI 上，留着会把死代理泄漏进
        // 测试进程，污染同进程后续会起子进程的用例（fable5 复审抓到的）。
        std::env::remove_var("HTTPS_PROXY");
        for (k, v) in &saved {
            std::env::set_var(k, v);
        }
    }
}
