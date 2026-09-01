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

/// Codex 的会话记录根目录：`~/.codex/sessions/年/月/日/rollout-…-<thread_id>.jsonl`。
/// 与 `claude_projects_root()`（claude.rs）同级的「盘上真相」入口。
fn codex_sessions_root() -> std::path::PathBuf {
    crate::installer::user_home_dir().join(".codex").join("sessions")
}

/// 这个 thread id 在盘上还有没有记录？**扫文件名，不递归、不解析内容**。
///
/// 🔴 **按文件名后缀找，不按目录转义算**（负例教训见 `claude.rs::claude_session_dir_of`
/// 头上那段）：rollout 文件名形如 `rollout-<时间戳>-<tid>.jsonl`（本机实测），tid 在
/// **末尾**、前面带时间戳前缀 —— 所以判「在不在」用「文件名以 `<tid>.jsonl` 结尾」。
/// tid 是定长 UUID（36 字符），后缀匹配不会误配到别的 thread；判错方向也保守
/// （误判「没有」= 丢一轮上下文，误判「有」= 拿坏 tid 去撞墙）。
/// 目录只有三层固定结构，walk 代价可忽略。
fn codex_thread_on_disk(tid: &str) -> bool {
    let want = format!("{tid}.jsonl");
    let root = codex_sessions_root();
    let Ok(years) = std::fs::read_dir(&root) else {
        return false;
    };
    for y in years.flatten() {
        let Ok(months) = std::fs::read_dir(y.path()) else { continue };
        for m in months.flatten() {
            let Ok(days) = std::fs::read_dir(m.path()) else { continue };
            for d in days.flatten() {
                let Ok(files) = std::fs::read_dir(d.path()) else { continue };
                for f in files.flatten() {
                    if f.file_name().to_string_lossy().ends_with(&want) {
                        return true;
                    }
                }
            }
        }
    }
    false
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
    // 并发闸：同一任务已有一轮在跑时，这条消息响亮地失败（根因与取舍见 mod.rs::TurnSlot）。
    let _slot = match super::try_begin_turn(AGENT, &task_id) {
        Ok(s) => s,
        Err(_) => {
            return Err("这个任务的上一轮还在跑 —— 等它说完（或点「停止」）再发下一条。".to_string());
        }
    };
    // 取上一轮 thread_id（有则 resume 续接）。**从盘上取** —— 关掉 U-King 再打开、
    // 甚至重启电脑，这个会话依旧接着上文说。以前记在进程内存里，界面上会话和聊天记录
    // 都还在、只有模型不知道「刚才」是什么，看起来像 AI 变笨了。见 `threads.rs`。
    let resume = super::threads::recall(AGENT, &task_id);

    // 🔴 **续接前先看盘上还有没有这份记录**（补齐到 claude 侧同等待遇；2026-08-18 那次
    // 治的是 claude，codex 同病：thread 记录被清理 / 换机器恢复了 threads.json 后，
    // `exec resume <tid>` 回 `no rollout found`（实测），每一轮都炸在同一条客户看不懂的
    // 英文报错上。事前判一下，最差也只是「这一轮不接上文」—— 而且**会明说**，不闷着。
    // 注：codex 的 resume 不像 claude 那样按目录隔离（本机实测：A 目录建的 thread 换到
    // B 目录照常续上），所以这里只判「在不在」，不判「在哪个目录」。
    // sessions 根目录整个读不了（新机器/权限异常）就别猜，保持原行为 —— 同 claude 的保守分支。
    let (resume, thread_note) = match resume {
        Some(tid) if codex_sessions_root().is_dir() && !codex_thread_on_disk(&tid) => {
            // 记录已不在盘上 = 这个 tid 永远续不回来了，忘掉它（claude 侧不 forget，
            // 因为那边的 sid「换回原目录还能接上」；这边不 forget 的话每一轮都重复报）。
            super::threads::forget(AGENT, &task_id);
            (
                None,
                Some("上一轮的会话记录找不到了（多半是清理过 Codex 的历史）—— 这一轮从头开始。".to_string()),
            )
        }
        other => (other, None),
    };
    if let Some(n) = &thread_note {
        let _ = on_event.send(serde_json::json!({ "kind": "text", "text": n }));
    }
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

#[cfg(test)]
mod resume_guard_tests {
    use super::super::try_begin_turn;
    use super::*;
    use std::fs;

    /// 合成一份「某天某目录下有这个 thread 的 rollout」的沙箱，
    /// 断言能被 [`codex_thread_on_disk`] 找到 —— 这条一旦漂了，
    /// 「记录还在却误判成丢了」会造成每一轮都放弃续接。
    /// 沙箱锁/还原/清理走全进程唯一入口（见 lib.rs::testsandbox）。
    #[test]
    fn finds_rollout_by_filename_in_sandbox() {
        crate::testsandbox::with_sandbox("codex-resume-hit", &[], |root| {
            let tid = "019d380c-b51f-7570-9251-9ae779bd9ab3";
            let day = root.join(".codex/sessions/2026/03/29");
            fs::create_dir_all(&day).unwrap();
            fs::write(day.join(format!("rollout-2026-03-29T13-24-10-{tid}.jsonl")), "{}\n").unwrap();
            assert!(codex_thread_on_disk(tid));
            // 没有的 id 不能撞上有的：文件名必须**整段**对上，不是子串包含
            assert!(!codex_thread_on_disk("ffffffff-ffff-ffff-ffff-ffffffffffff"));
        });
    }

    /// 空机器（没有 ~/.codex/sessions）：判「不在盘上」，不崩、不猜。
    #[test]
    fn missing_sessions_root_reports_absent() {
        crate::testsandbox::with_sandbox("codex-resume-miss", &[], |_root| {
            assert!(!codex_thread_on_disk("0199d674-3f13-73b2-8819-e7a0ae623aa0"));
        });
    }

    /// TurnSlot 语义：同任务第二个进入者被拒；持有人掉落后槽位释放（可再进）；
    /// 不同 agent 同 task_id 互不干扰。**并发闸是进程级真值** —— 锁泄漏 = 任务永久发不出消息，
    /// 所以释放路径必须有测试盯着。
    #[test]
    fn turn_slot_blocks_then_releases() {
        let g1 = try_begin_turn("codex", "slot-task-a").expect("首轮应拿到槽位");
        assert!(
            try_begin_turn("codex", "slot-task-a").is_err(),
            "同任务第二重入必须被拒"
        );
        // 另一个大脑、另一个任务：不受牵连
        assert!(try_begin_turn("claude", "slot-task-a").is_ok());
        assert!(try_begin_turn("codex", "slot-task-b").is_ok());
        drop(g1);
        assert!(
            try_begin_turn("codex", "slot-task-a").is_ok(),
            "持有人掉落后必须释放"
        );
    }

    #[test]
    fn event_mapping_matrix_covers_text_tools_usage_and_safe_ignores() {
        let cases = [
            (serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"done"}}), "text_done", Some("done")),
            (serde_json::json!({"type":"item.updated","item":{"type":"agent_message","text":"part"}}), "text", Some("part")),
            (serde_json::json!({"type":"item.started","item":{"type":"file_change","id":"edit-1"}}), "tool_start", None),
            (serde_json::json!({"type":"item.completed","item":{"type":"file_change","id":"edit-1","path":"src/main.rs"}}), "tool_end", Some("src/main.rs")),
        ];
        for (input, kind, payload) in cases {
            let evs = map_codex_event(&input, "t");
            assert_eq!(evs.len(), 1, "{input}");
            assert_eq!(evs[0]["kind"], kind, "{input}");
            // sol 建议：不只守事件分类，载荷字段也钉住（text 原文 / Edit 的 output=path）
            if let Some(p) = payload {
                assert_eq!(evs[0][if kind == "text_done" || kind == "text" { "text" } else { "output" }], p, "{input}");
            }
            if kind == "tool_start" || kind == "tool_end" {
                assert_eq!(evs[0]["name"], "Edit", "{input}");
            }
        }
        assert_eq!(map_codex_event(&serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":""}}), "t").len(), 0);
        for input in [
            serde_json::json!({"type":"unknown"}),
            serde_json::json!({"type":"item.completed","item":null}),
            serde_json::json!({"type":"item.completed","item":{"type":"reasoning"}}),
            serde_json::json!({"type":"item.completed","item":{"type":"todo_list"}}),
        ] {
            assert!(map_codex_event(&input, "t").is_empty(), "{input}");
        }

        let started = serde_json::json!({"type":"item.started","item":{"type":"command_execution","id":"cmd-1","command":"git status --short"}});
        let evs = map_codex_event(&started, "t");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0]["kind"], "tool_start");
        assert_eq!(evs[1]["kind"], "tool_input");
        assert_eq!(evs[1]["input"]["command"], "git status --short");

        for (input, output, is_error) in [
            (serde_json::json!({"type":"item.completed","item":{"type":"command_execution","id":"cmd-2","output":"fallback","aggregated_output":"preferred","exit_code":1}}), "preferred", true),
            (serde_json::json!({"type":"item.completed","item":{"type":"command_execution","id":"cmd-3","output":"ok","exit_code":0}}), "ok", false),
            (serde_json::json!({"type":"item.completed","item":{"type":"command_execution","id":"cmd-4"}}), "", false),
        ] {
            let evs = map_codex_event(&input, "t");
            assert_eq!(evs.len(), 1);
            assert_eq!(evs[0]["output"], output);
            assert_eq!(evs[0]["is_error"].as_bool(), Some(is_error));
        }

        let usage = map_codex_event(&serde_json::json!({"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7,"cached_input_tokens":5}}), "t");
        assert_eq!(usage[0]["input_tokens"], 11);
        assert_eq!(usage[0]["output_tokens"], 7);
        assert_eq!(usage[0]["cache_read_tokens"], 5);
        let absent_usage = map_codex_event(&serde_json::json!({"type":"turn.completed"}), "t");
        assert_eq!(absent_usage[0], serde_json::json!({"kind":"usage","input_tokens":0,"output_tokens":0,"cache_read_tokens":0}));

        let failed = map_codex_event(&serde_json::json!({"type":"turn.failed","error":{"message":"no quota"}}), "t");
        assert_eq!(failed[0]["status"], "error");
        assert_eq!(failed[0]["message"], "no quota");
        let failed_fallback = map_codex_event(&serde_json::json!({"type":"turn.failed"}), "t");
        assert_eq!(failed_fallback[0]["message"], "Codex 回合失败");
    }

    #[test]
    fn reconnect_error_is_a_notice_not_a_silent_or_fatal_event() {
        let reconnect = map_codex_event(&serde_json::json!({"type":"error","message":"Reconnecting... 2/5"}), "t");
        assert_eq!(reconnect.len(), 1);
        assert_eq!(reconnect[0]["kind"], "notice");
        for message in ["401 unauthorized", "Connection reset"] {
            let evs = map_codex_event(&serde_json::json!({"type":"error","message":message}), "t");
            assert_eq!(evs.len(), 1);
            assert_eq!(evs[0]["kind"], "done");
            assert_eq!(evs[0]["status"], "error");
            // sol 建议：普通错误原文透传（类别对但文案丢了，客户看到的报错就空了）
            assert_eq!(evs[0]["message"], message);
        }
    }

    #[test]
    fn thread_started_emits_session_and_persists_thread_id() {
        struct ResetReadonly;
        impl Drop for ResetReadonly {
            fn drop(&mut self) {
                super::super::threads::set_readonly(false);
            }
        }

        let sb = crate::testsandbox::enter_raw("codex-thread-start-persist");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::set_var("HOME", sb.root());
        let _reset = ResetReadonly;
        super::super::threads::set_readonly(false);
        let tid = "019d380c-t5-thread-start";
        let task = "test-codex-thread-start-t5";
        let evs = map_codex_event(&serde_json::json!({"type":"thread.started","thread_id":tid}), task);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["kind"], "session");
        assert_eq!(evs[0]["session_id"], tid);
        assert_eq!(super::super::threads::recall("codex", task).as_deref(), Some(tid));
        let disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(sb.root().join(".uking/agent-threads.json")).unwrap()).unwrap();
        assert_eq!(disk.pointer("/agents/codex/test-codex-thread-start-t5/id").and_then(|v| v.as_str()), Some(tid));

        assert!(map_codex_event(&serde_json::json!({"type":"thread.started"}), "test-codex-thread-start-missing").is_empty());
        assert_eq!(super::super::threads::recall("codex", "test-codex-thread-start-missing"), None);
    }

    #[test]
    fn codex_thread_on_disk_rejects_near_misses_and_shallow_layouts() {
        crate::testsandbox::with_sandbox("codex-resume-near-miss", &[], |root| {
            let tid = "019d380c-negative-thread";
            let day = root.join(".codex/sessions/2026/03/29");
            fs::create_dir_all(&day).unwrap();
            for name in [format!("{tid}.json"), format!("{tid}.jsonl.bak"), format!("{tid}x.jsonl")] {
                fs::write(day.join(name), "{}\n").unwrap();
            }
            let shallow = root.join(".codex/sessions/2027/04");
            fs::create_dir_all(&shallow).unwrap();
            fs::write(shallow.join(format!("{tid}.jsonl")), "{}\n").unwrap();
            assert!(!codex_thread_on_disk(tid));
        });
    }
}
