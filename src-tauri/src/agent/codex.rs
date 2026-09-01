//! Codex CLI exec --json 驱动 —— 把 codex 的 JSONL 事件流解析成和 claude.rs 同一套统一事件，
//! 复用 ChatPanel 渲染成结构化卡片（Codex 大脑不再只是裸终端 TUI）。
//!
//! ## 为什么能这样
//! `codex exec --json` 每行吐一个 JSON 事件（thread.started / turn.* / item.*），和 Claude Code 的
//! `--output-format stream-json` 是一个套路。这里逐行解析 → 映射成 {kind: session/text/text_done/
//! tool_start/tool_input/tool_end/usage/done}（与 protocol.rs 输出同形），前端 ChatPanel 无需改。
//!
//! ## 多轮
//! codex `exec` 一次一进程；多轮靠 `exec resume <thread_id>` 续接（首轮拿 thread.started 的 id 记下）。
//!
//! ## 零依赖 + 免配置
//! 纯 std Command + serde_json。注入虾盘云 env（delegation_env）让 Codex 大脑免客户单独配置直连同一套计费。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::ipc::Channel;

use crate::installer::{portable_node_dir, search_paths};

use super::cmdline;
use super::launcher::{self, Launcher};
use super::{TurnLog, Watchdog, STALL_SECS};

/// 这个大脑在 `threads` 那份落盘表里的名字。
const AGENT: &str = "codex";
/// 正在运行的 codex 子进程（用于中断）。task_id -> Child。
fn running() -> &'static Mutex<HashMap<String, Child>> {
    static R: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 起一条命令。解析交给 `launcher`（和 claude.rs 同一份）：Windows 上 npm 装的 `codex` 是
/// `.cmd` 批处理壳，直接 spawn 它会让所有含换行的参数被 std 拒掉。codex 这条路更要命 ——
/// 专家 persona 是**prepend 进提示词**的，也就是说召唤专家后连提示词本身都必然多行。
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

fn inject_path(c: &mut std::process::Command, delegated: bool) {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let dirs = search_paths(portable_node_dir().as_deref());
    if !dirs.is_empty() {
        let prefix = dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(sep);
        let old = std::env::var("PATH").unwrap_or_default();
        c.env("PATH", format!("{prefix}{sep}{old}"));
    }
    // 代理策略与 agent/claude.rs 同一道分岔（根因见 installer.rs 的系统代理段，2026-08-24）：
    // 委托轮连虾盘云国内镜像 → 清代理（历史行为保留）；自持凭据直连官方 → 补系统代理。
    if delegated {
        for v in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy", "NO_PROXY", "no_proxy"] {
            c.env_remove(v);
        }
        return;
    }
    for (k, v) in crate::installer::proxy_env_for(false) {
        if std::env::var_os(&k).is_none() {
            c.env(k, v);
        }
    }
}

/// 把一行 codex JSONL 事件映射成 ChatPanel 认的统一事件（与 protocol.rs 同形）。
fn map_codex_event(v: &Value, threads_key: &str) -> Vec<Value> {
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match ty {
        "thread.started" => {
            if let Some(tid) = v.get("thread_id").and_then(|s| s.as_str()) {
                // 落盘（见 threads.rs）：关掉 U-King 再打开，这个会话依旧接着上文说。
                super::threads::remember(AGENT, threads_key, tid);
                return vec![json!({ "kind": "session", "session_id": tid })];
            }
            vec![]
        }
        "item.started" | "item.updated" | "item.completed" => {
            let item = v.get("item").cloned().unwrap_or(Value::Null);
            let it_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let completed = ty == "item.completed";
            match it_type {
                "agent_message" => {
                    let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    if text.is_empty() {
                        return vec![];
                    }
                    // completed 给全文 → text_done（ChatPanel 会整体替换本轮 assistant 文本）
                    if completed {
                        vec![json!({ "kind": "text_done", "text": text })]
                    } else {
                        vec![json!({ "kind": "text", "text": text })]
                    }
                }
                "command_execution" => {
                    let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("");
                    if ty == "item.started" {
                        return vec![
                            json!({ "kind": "tool_start", "id": id, "name": "Bash" }),
                            json!({ "kind": "tool_input", "id": id, "name": "Bash", "input": { "command": cmd } }),
                        ];
                    }
                    if completed {
                        let out = item.get("aggregated_output").or_else(|| item.get("output")).and_then(|o| o.as_str()).unwrap_or("");
                        let err = item.get("exit_code").and_then(|c| c.as_i64()).map(|c| c != 0).unwrap_or(false);
                        return vec![json!({ "kind": "tool_end", "id": id, "name": "Bash", "output": out, "is_error": err })];
                    }
                    vec![]
                }
                "file_change" => {
                    if ty == "item.started" {
                        return vec![json!({ "kind": "tool_start", "id": id, "name": "Edit" })];
                    }
                    if completed {
                        let path = item.get("path").and_then(|p| p.as_str()).unwrap_or("");
                        return vec![json!({ "kind": "tool_end", "id": id, "name": "Edit", "output": path, "is_error": false })];
                    }
                    vec![]
                }
                "mcp_tool_call" | "web_search" => {
                    let name = if it_type == "web_search" { "WebSearch" } else { "Wrench" };
                    if ty == "item.started" {
                        return vec![json!({ "kind": "tool_start", "id": id, "name": name })];
                    }
                    if completed {
                        return vec![json!({ "kind": "tool_end", "id": id, "name": name, "output": "", "is_error": false })];
                    }
                    vec![]
                }
                _ => vec![], // reasoning / todo_list 等暂不渲染，避免噪声
            }
        }
        "turn.completed" => {
            let usage = v.get("usage");
            let get = |k: &str| usage.and_then(|u| u.get(k)).and_then(|x| x.as_i64()).unwrap_or(0);
            vec![json!({ "kind": "usage", "input_tokens": get("input_tokens"), "output_tokens": get("output_tokens"), "cache_read_tokens": get("cached_input_tokens") })]
        }
        "turn.failed" => {
            let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("Codex 回合失败");
            vec![json!({ "kind": "done", "status": "error", "message": msg })]
        }
        "error" => {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
            // 重连提示（"Reconnecting... 2/5"）非致命，但**不能吞**。
            //
            // 这正是客户嘴里那个「5 次重连」：Codex 的 `stream_max_retries` 默认就是 5
            // （上游 model-provider-info：`DEFAULT_STREAM_MAX_RETRIES = 5`），流断一次重发一次，
            // 连断 5 次这一轮就失败。上游特意把它 surface 给前端，源码注释写得很直白：
            // 「让用户明白发生了什么，而不是盯着一个看似冻住的屏幕」。
            // 我们以前 `return vec![]` 吞掉，等于亲手把客户送回那块冻住的屏幕 —— 于是
            // 「Codex 卡住不动」成了反馈里的常客，而真相只是网络在重试。
            //
            // 走 `notice`：**不进对话流**（它不是对话内容），前端当瞬时状态显示、done 时清掉。
            if msg.contains("Reconnect") {
                return vec![json!({ "kind": "notice", "text": msg })];
            }
            vec![json!({ "kind": "done", "status": "error", "message": msg })]
        }
        _ => vec![],
    }
}

/// 给一个任务发一条消息，跑一轮 codex exec，结构化事件经 `on_event` Channel 流回前端。
#[tauri::command]
pub async fn codex_send(
    task_id: String,
    prompt: String,
    cwd: Option<String>,
    model: Option<String>,
    system: Option<String>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || run_turn(task_id, prompt, cwd, model, system, on_event))
        .await
        .map_err(|e| format!("codex 任务调度失败: {e}"))?
}

fn run_turn(task_id: String, prompt: String, cwd: Option<String>, model: Option<String>, system: Option<String>, on_event: Channel<Value>) -> Result<(), String> {
    let resume = super::threads::recall(AGENT, &task_id);
    // 静默账本 —— 和 claude 那条路共用一份实现（理由见 `agent/mod.rs::TurnLog`）。
    let mut tlog = TurnLog::start("chat", "codex", &task_id, model.as_deref(), resume.is_some());
    // 用户原话 —— teach（教人在终端敲的那条）只该带这一句。下面 `prompt` 会被前置一大段
    // 运行环境约束/persona，拿它去判 `inlineable` 会永远是 false，客户复制到的命令就没了提示词。
    let user_text = prompt.clone();
    // codex exec 无 system flag：只能把运行环境约束 + 专家 persona 前置到 prompt 里。
    // 只在首轮（无 resume）加：续接轮的上下文里已经有了，每轮重发一遍纯属烧 token。
    let prompt = match &resume {
        Some(_) => prompt,
        None => match system.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => format!("{}\n\n---\n\n{s}\n\n---\n\n{prompt}", super::GUARD_PROMPT),
            None => format!("{}\n\n---\n\n{prompt}", super::GUARD_PROMPT),
        },
    };

    let l = launcher::resolve("codex");
    let mut c = base_command(&l);
    // 注入虾盘云端点 + 设备 Key：让 Codex 大脑免客户单独配置直连同一套计费（同 claude.rs / 委派）。
    if let Ok(key) = crate::device::device_key_offline() {
        for (k, v) in crate::providers::delegation_env(&key) {
            c.env(k, v);
        }
    }
    // argv 先攒 Vec 再一次性交给 Command，「看命令」才能摆出一字不差的真实命令（同 claude.rs）。
    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(tid) = &resume {
        args.push("resume".into());
        args.push(tid.clone());
    }
    args.push("--json".into());
    args.push("--skip-git-repo-check".into());
    if let Some(m) = model.as_deref().filter(|m| !m.trim().is_empty()) {
        args.push("-m".into());
        args.push(m.to_string());
    }
    args.push(prompt.clone());
    c.args(&args);

    // 「看命令」：真实命令 + 终端交互式等价写法。teach 不带 exec/--json（那是无头出 JSONL 用的），
    // 也不带 resume —— codex 交互模式的续接写法各版本不一，宁可教一条一定能跑的，不教半对的。
    {
        let mut teach: Vec<String> = Vec::new();
        if let Some(m) = model.as_deref().filter(|m| !m.trim().is_empty()) {
            teach.push("-m".into());
            teach.push(m.to_string());
        }
        let inlined = cmdline::inlineable(&user_text);
        if inlined {
            teach.push(user_text.clone());
        }
        let _ = on_event.send(cmdline::event(&l.program, &l.full_args(&args), "codex", &teach, inlined));
    }

    if let Some(d) = cwd.as_deref().filter(|p| std::path::Path::new(p).is_dir()) {
        c.current_dir(d);
    }
    inject_path(&mut c, crate::providers::codex_delegated());
    c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let mut child = c.spawn().map_err(|e| {
        tlog.finish("spawn_failed", None);
        format!("启动 codex 失败（是否已安装？）: {e}")
    })?;
    let pid = child.id();
    tlog.spawned(pid);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut m) = running().lock() {
        m.insert(task_id.clone(), child);
    }

    // 看门狗：静默超过 STALL_SECS 就把整棵进程树收掉（实现和理由见 `agent/mod.rs::Watchdog`）
    let wd = Watchdog::spawn(pid, Duration::from_secs(STALL_SECS));

    let err_buf = Arc::new(Mutex::new(String::new()));
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

    if let Some(so) = stdout {
        for line in BufReader::new(so).lines().map_while(Result::ok) {
            wd.beat(); // 有一行就算「还活着」——空行/非 JSON 行也算
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            for ev in map_codex_event(&v, &task_id) {
                tlog.on_event(
                    ev.get("kind").and_then(|k| k.as_str()).unwrap_or(""),
                    ev.get("name").and_then(|n| n.as_str()),
                );
                let _ = on_event.send(ev);
            }
        }
    }
    if let Some(h) = err_h {
        let _ = h.join();
    }
    wd.finish(); // 管道读完 = 这一轮已收尾，看门狗下班

    let child_opt = running().lock().ok().and_then(|mut m| m.remove(&task_id));
    let (interrupted, code) = match child_opt {
        Some(mut ch) => {
            let status = ch.wait().map_err(|e| {
                tlog.finish("wait_failed", None);
                format!("等待 codex 失败: {e}")
            })?;
            (!status.success() && status.code().is_none(), status.code())
        }
        None => (true, None),
    };
    let err_text = err_buf.lock().map(|g| g.trim().to_string()).unwrap_or_default();
    // 卡死判定排在 interrupted 前面：看门狗靠杀进程收场，进程状态跟「人按了停止」一模一样，
    // 顺序摆错就会把「它挂了」报成「你停了它」（同 claude.rs）。
    let done = if wd.stalled() {
        json!({ "kind": "done", "status": "timeout", "stall_secs": STALL_SECS })
    } else if interrupted {
        json!({ "kind": "done", "status": "interrupted" })
    } else if code == Some(0) {
        json!({ "kind": "done", "status": "ok" })
    } else {
        json!({ "kind": "done", "status": "error", "code": code, "message": err_text })
    };
    // 定性读同一个 done 事件，不另起一套判断（同 claude.rs）。
    tlog.finish(done.get("status").and_then(|s| s.as_str()).unwrap_or("?"), code);
    let _ = on_event.send(done);
    Ok(())
}

/// 中断某任务正在跑的 codex。**杀整棵树**（理由见 `claude.rs::claude_interrupt`：
/// 只 kill 自己会把它派生的 shell/node 留成孤儿，界面停了机器没停 = 骗人）。
#[tauri::command]
pub fn codex_interrupt(task_id: String) -> Result<(), String> {
    if let Ok(mut m) = running().lock() {
        if let Some(mut ch) = m.remove(&task_id) {
            super::chat::kill_tree_by_pid(ch.id());
            let _ = ch.kill();
        }
    }
    Ok(())
}

/// 重置某任务的 codex 多轮上下文（忘掉 thread_id，下轮从头开始）。
/// 落盘那份也得清 —— 只清内存的话，重启后「新对话」会自己撤销。
#[tauri::command]
pub fn codex_reset(task_id: String) -> Result<(), String> {
    super::threads::forget(AGENT, &task_id);
    Ok(())
}
