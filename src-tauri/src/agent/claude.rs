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
/// 客户自己在 Claude Code 里设的默认权限模式（`~/.claude/settings.json` 的
/// `permissions.defaultMode`）。没设过 = `None`，那时候才由我们给免审批。
///
/// 🔴 为什么必须读它（2026-11-02 老板口径）：U-King 只负责**装、配、启动、充值**，
/// 不替客户决定他软件里的策略。客户在 Claude Code 里选了「逐条问我」或「先给计划」，
/// 那是他的选择，管家不该在对话面板里把它悄悄改成全放行 —— 反过来也一样。
///
/// 也不能干脆不传：不传 = 走 Claude Code 的默认（逐条审批），而这个面板对面多半是
/// 不看命令行的人，每条命令点一次「允许」等于没法用。所以口径是**跟随**：
/// 客户设过就听他的，没设过才由我们给 `bypassPermissions`。
///
/// 只认 Claude Code 自己那四档 —— 别的字符串透传过去会让 claude 直接报错退出
/// （客户看到的是「这一轮没跑成」，比不跟随更糟）。
fn user_permission_mode() -> Option<String> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("HOME").ok())?;
    let p = std::path::Path::new(&home).join(".claude").join("settings.json");
    let raw = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let m = v.get("permissions")?.get("defaultMode")?.as_str()?.trim().to_string();
    matches!(m.as_str(), "default" | "acceptEdits" | "plan" | "bypassPermissions").then_some(m)
}

fn build_args(
    prompt: &str,
    model: Option<&str>,
    system: Option<&str>,
    resume: Option<&str>,
    permission_mode: Option<&str>,
) -> (Vec<String>, Vec<String>, bool) {
    let model = model.filter(|m| !m.trim().is_empty());
    let mut args: Vec<String> = vec![
        "--output-format".into(), "stream-json".into(),
        "--include-partial-messages".into(),
        "--verbose".into(),
    ];
    // 🔴 权限模式**跟随客户自己配的**（见 `user_permission_mode`）；没配过才由我们给免审批。
    // 以前这里写死 `bypassPermissions`，后果是客户在 Claude Code 里配的
    // `default`（逐条问我）/ `plan`（先给计划、我批了再动）进对话面板**当场失效**，
    // 而且界面上没有任何地方能看出被覆盖了 —— 那是「管家替主人改了他软件的设置」。
    args.push("--permission-mode".into());
    args.push(permission_mode.unwrap_or("bypassPermissions").to_string());
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

/// 「换了模型」这句提示 —— **只在两个模型都确知、且真的不同**时用它。
const MODEL_SWITCH_NOTE: &str =
    "换了模型，上一轮（旧模型建的会话）接不上 —— 这一轮从头开始（之前的记录还在，换回原来那个模型就能接上）。";
/// 本轮模型无法确知时改说这句。两种来源都可能：客户自己配了 Claude（我们不注入模型），
/// 或这一轮压根没拿到设备 Key（注入列表为空）。文案**只陈述「没确认到」**，不猜原因 ——
/// 断上文这个动作照旧（安全的降级方向），但**不许谎称**他「换了模型」。
const MODEL_UNKNOWN_NOTE: &str =
    "这一轮没确认到实际用的是哪个模型，为了不把两种模型的记录混到一起，这一轮先不接上文。";

/// 要不要拿上一轮的 sid 继续 `--resume`（纯函数，回归钉在 `resume_guard_tests`）。
///
/// 三个分支刻意分开：真的换了模型 → 说换模型；模型未知 → 只说不确定；老盘没记过模型
/// （`stored_model = None`）→ 保持原来「照常续接」的行为，发版后不许让存量客户无辜掉一次上文。
fn decide_resume(
    resume: Option<String>,
    stored_model: Option<String>,
    cur_model: Option<String>,
) -> (Option<String>, Option<&'static str>) {
    match (resume, stored_model, cur_model) {
        (None, _, _) => (None, None),
        (Some(sid), Some(stored), Some(cur)) if stored != cur => (None, Some(MODEL_SWITCH_NOTE)),
        (Some(_), Some(_), None) => (None, Some(MODEL_UNKNOWN_NOTE)),
        (r, _, _) => (r, None),
    }
}

/// 这一轮收到的 session id 要不要写进账本（`threads.rs`）。
///
/// 🔴 **失败轮不许覆盖上一次能用的 sid**（sol 会审 2026-11-23，P1）：以前收到 init 里的
/// session id 就立刻记账，而「只收到 init 就被上游拒掉」的轮次也照样记 —— 于是原来那条
/// 能续上的会话被一条**从没成功过的**新 id 顶掉，界面上还写着「换回原来那个模型就能接上」。
/// 判据：这一轮真的跑成了（`code == Some(0)`），或者人按了停止但已经出过内容（那条会话有东西）；
/// 看门狗判死的超时轮**一律不记**（它也没有退出码，跟「人按停止」在进程状态上无法区分）。
/// 另外收尾前若客户**在本会话**点过「清空对话」（[`super::threads::reset_epoch`] 变了），这一笔也不写。
fn should_remember_session(code: Option<i32>, stalled: bool, interrupted: bool, saw_output: bool) -> bool {
    if stalled {
        // 看门狗杀进程也「没有退出码」，但它判的是**这一轮挂了**（timeout）——
        // 跟「人按了停止」在进程状态上长得一模一样，必须靠 stalled 分开，否则超时轮也会记账。
        return false;
    }
    if code == Some(0) {
        return true;
    }
    interrupted && saw_output
}

/// 收尾事件（`done`）的构造 —— 纯函数，**只有一个出口**。
///
/// 抽出来的理由：`hint` 是「什么时候许带、带什么」的判断，藏在一个 60 行的 `run_turn` 里
/// 就没法回归（要验它得真起一个 claude 进程、还得让上游配合报错）。抽成纯函数后，
/// 「超时不带」「成功不带」「真聊太长（探针没命中）不带」「命中配置才带」四条都能钉住。
fn settle_event(
    stalled: bool,
    interrupted: bool,
    code: Option<i32>,
    err_text: String,
    ctx_hint: Option<(String, String, String)>,
) -> Value {
    if stalled {
        return serde_json::json!({
            "kind": "done", "status": "timeout",
            "stall_secs": STALL_SECS,
        });
    }
    if interrupted {
        return serde_json::json!({ "kind": "done", "status": "interrupted" });
    }
    if code == Some(0) {
        return serde_json::json!({ "kind": "done", "status": "ok" });
    }
    match ctx_hint {
        Some((file, key, value)) => serde_json::json!({
            "kind": "done", "status": "error",
            "code": code, "message": err_text,
            "hint": { "kind": "context_limit", "file": file, "key": key, "value": value }
        }),
        None => serde_json::json!({
            "kind": "done", "status": "error",
            "code": code, "message": err_text
        }),
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
    // 「清空对话」计数快照：收尾写账本时由 `threads` 在同一把锁里比对（见 `File::resets`）。
    let epoch_at_start = super::threads::reset_epoch(&task_id);
    let stored = super::threads::recall_with_model(AGENT, &task_id);
    let resume = stored.as_ref().map(|(sid, _)| sid.clone());
    let stored_model = stored.as_ref().and_then(|(_, m)| m.clone());
    // 本轮顶栏"换模型"的值 —— **归一化一次，argv、账本、比对三处同源**
    //（以前 argv 用原值、比对用 trim 值：手填里多打一个空格，两边就会各说各话）。
    let explicit_model = super::threads::norm_model(model.as_deref());
    // 这一轮**真的会注入**给子进程的 env（只算一次，下面 spawn 前直接套用）。
    // 拿不到设备 Key 时这里是空的 —— 那就等于「模型不由我们决定」，口径必须是未知，
    // 不能拿预置值去猜（否则会把不是这个模型的 sid 记成这个模型）。
    let delegation_env: Vec<(String, String)> = match crate::device::device_key_offline() {
        Ok(key) => crate::providers::delegation_env(&key),
        Err(_) => Vec::new(),
    };
    let injected_model = delegation_env
        .iter()
        .find(|(k, _)| k == "ANTHROPIC_MODEL")
        .map(|(_, v)| v.clone());
    // 「跟随驱动设置」与显式填同一个模型时不算换模型 —— 归一口径就这一处：
    //   显式 `--model` → 就是它；
    //   跟随驱动 → **这一轮实际注入了什么**（`injected_model`）；没注入就是未知，不猜。
    // 实测依据（2026-11-23 本机）：settings.json 的 `model` 是 `sonnet[1m]`，不带 `--model` 跑一轮，
    // transcript 里落的是 `deepseek-flash` ⇒ 注入的 `ANTHROPIC_MODEL` 确实压过 settings.json 的 model。
    let cur_model = explicit_model.clone().or(injected_model);

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
    // 🔴 **换模型自动断续接**（2026-09-07 thinking-400 定性）：顶栏"换模型"是每轮生效的，
    // 但旧 sid 里躺着旧模型的 thinking 块 —— 拿它去 --resume，新模型在网关翻译层必 400
    //（new-api#6939 / PR#6998 同类：thinking signature 回放对不上）。model 变了不断不断，
    // 而是明说一句开新轮 —— 静默断等于上下文归零还不告诉人（Opus 评审 B）。
    // 只认"两边都已知且不一样"：老数据没记过模型（stored None）不断，避免发版后全员无辜掉一次上文；
    // 本轮模型无法确知（客户自持配置）时断但不撒谎说「换了模型」，见 `decide_resume`。
    let (resume, model_note) = decide_resume(resume, stored_model, cur_model.clone());
    let thread_note = thread_note.or(model_note.map(str::to_string));
    if let Some(n) = &thread_note {
        let _ = on_event.send(serde_json::json!({ "kind": "text", "text": n }));
    }

    let mut tlog = TurnLog::start("chat", "claude", &task_id, explicit_model.as_deref(), resume.is_some());

    // 并发闸：同一任务已有一轮在跑时，这条消息响亮地失败（根因与取舍见 mod.rs::TurnSlot）。
    // 放在 TurnLog 之后（账本先记下「来过一轮」，再拦），拦的是真正的进程竞态，不是日志。
    let _slot = match super::try_begin_turn(AGENT, &task_id) {
        Ok(s) => s,
        Err(_) => {
            tlog.finish("busy", None);
            return Err("这个任务的上一轮还在跑 —— 等它说完（或点「停止」）再发下一条。".to_string());
        }
    };

    let l = launcher::resolve("claude");
    let mut c = base_command(&l);
    // 注入虾盘云端点 + 设备 Key：让 Claude Code 大脑**免客户单独配置**直接用同一套计费
    //（同 agent/chat.rs 委派 run_command 的做法）。仅子进程 env，不碰用户 ~/.claude 文件。
    // 用的是开轮时算好的同一份 `delegation_env`——模型口径与真正注入的东西必须是同一个事实。
    for (k, v) in &delegation_env {
        c.env(k, v);
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
    let perm = user_permission_mode();
    let (args, teach, inlined) =
        build_args(&prompt, explicit_model.as_deref(), system.as_deref(), resume.as_deref(), perm.as_deref());
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
    //
    // 🔴 这两个数**回官方默认**（2026-11-02，老板口径：U-King 是 360 软件管家，装上去的软件
    // 必须是全功能原版，不许削它自带的能力）。原来 `BASH_MAX_TIMEOUT_MS` 被压到 3 分钟，
    // 后果是客户让 AI 跑一个装依赖 / 建索引 / 跑数据这类正当的长命令时，第三分钟就被我们
    // 从底下掐断 —— 那不是"防卡死"，那是替客户把他的能力砍了一半。
    // 真正的防卡死是 `STALL_SECS` 那层（静默才杀），跟命令最长能跑多久是两件事。
    c.env("BASH_DEFAULT_TIMEOUT_MS", "120000"); // 常规命令 2 分钟（官方默认）
    c.env("BASH_MAX_TIMEOUT_MS", "600000"); // 模型主动加长可以到 10 分钟（官方默认）
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
    // 这一轮收到的 session id 先暂存，**收尾时按成败决定记不记**（见 `should_remember_session`）。
    let mut pending_sid: Option<String> = None;
    // 有没有真的出过内容（文本/工具）。中断前只出过 init 的那些轮，不算「这条会话有东西」。
    let mut saw_output = false;
    if let Some(so) = stdout {
        for line in BufReader::new(so).lines().map_while(Result::ok) {
            let line = line.trim();
            // 🔴 `api_retry` 不算「还活着」—— 它证明的恰恰是**上游没接住**。
            // 在这里给它打卡的代价，是「5 分钟静默即挂」的保护当场失效：一次重试把死线
            // 往后推一次，`max_retries:10` 就能把它推成十几二十分钟（2026-11-02 客户那轮
            // 实测 26 分 11 秒、0 token，界面从头到尾一动不动，最后还只得到一句「我没认出」）。
            // 其余行（含空行 / 非 JSON）照旧算活着 —— 它们才是「那头还在干活」的证据。
            if !line.contains("api_retry") {
                wd.beat();
            }
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
                // 记住 session_id 供下轮 --resume（落盘，见 threads.rs）——
                // **只暂存**，成功收尾时才写账本（失败轮不许顶掉上一次能用的 sid）。
                match ev.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
                    "session" => {
                        if let Some(sid) = ev.get("session_id").and_then(|s| s.as_str()) {
                            pending_sid = Some(sid.to_string());
                        }
                    }
                    // `text_done` 也算：整段回复一次性回来（没有流式 text 分片）时只有它，
                    // 漏掉它会把「看到回复后按停止」的那种轮判成「没出过内容」（sol 第三轮）。
                    "text" | "text_done" | "tool_start" | "tool_input" | "tool_end" => saw_output = true,
                    _ => {}
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

    // 账本只在**这一轮真跑成**（或人按停止但已经出过内容）时才前移 —— 理由见
    // `should_remember_session`：失败轮把 sid 记进去，会顶掉上一次那条能续上的会话。
    // 客户在这一轮跑的中途点过「清空对话」→ 这一笔不记（否则刚清的会话又被接上）。
    // 快照交给 `threads`，**在它那把锁里比对再写**——分两次上锁仍会漏同一个交错。
    if should_remember_session(code, wd.stalled(), interrupted, saw_output) {
        if let Some(sid) = pending_sid.as_deref() {
            super::threads::remember_with_model_at_epoch(
                AGENT,
                &task_id,
                sid,
                cur_model.as_deref(),
                Some(epoch_at_start),
            );
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
        let from_stderr = err_buf.lock().map(|g| g.clone()).unwrap_or_default();
        failure_text(&from_stderr, state.result_error())
    };
    // 🔴 失败原文落盘（2026-11-02 客户「咋老跳出这个」）。
    //
    // 客户截图里只剩半句「我没认出这是哪种问题」，原始报错被截掉了 —— 而我们手里唯一的
    // 证据原本就是客户那张截图。客户**拿不到**：他得先找回那个会话、还得保证窗口没关。
    // 所以把正文写进 `~/.uking/logs/chat.log`（「技术支持」整包会带上它），
    // 排障不必再回头求客户重新截图。
    //
    // 脱敏边界：**只写报错正文**（上游错误 / CLI 诊断），不写提示词、不写工具入参 ——
    // 跟 `TurnLog` 既有规矩一致（这份日志会随技术支持整份上传）。
    // 截尾 400 字：关键信息通常在最后，跟界面上折进代码块的口径一致。
    // 成功轮一个字都不写 —— 日志不是聊天记录。
    let will_fail = !wd.stalled() && !interrupted && code != Some(0);
    if will_fail && !err_text.trim().is_empty() {
        let tail: String = err_text
            .chars()
            .rev()
            .take(400)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        crate::ulog::write("chat", &format!("turn err={tail}"));
    }
    // 收尾事件：卡死/成功/中断/错误。
    // **卡死判定必须排在 interrupted 前面** —— 看门狗是靠杀进程收场的，进程状态上跟「人按了停止」
    // 长得一模一样。谁在前面谁定性，摆错顺序就会把「它挂了」报成「你停了它」，客户再也不会告诉我们。
    // 上下文上限被本机配置写死 → 给界面一个**结构化**的诊断（2026-11-23 实拍）。
    //
    // 只在这条错误上探针：`Prompt is too long` 有真有假 —— 真聊太长时配置是正常的，
    // 探针返回 None，界面保持原来的报错。探针**只读**用户的 `~/.claude/settings.json`，
    // 不改它（那可能是客户自己写的小窗口，改配置这种事必须人点头）。
    let ctx_hint = if err_text.to_ascii_lowercase().contains("prompt is too long") {
        crate::providers::claude_context_limit_probe()
    } else {
        None
    };
    let done = settle_event(wd.stalled(), interrupted, code, err_text, ctx_hint);
    // 日志里的定性**读同一个 done 事件**，不另起一套判断 —— 否则界面说「你停了它」、
    // 日志说「它挂了」，两边都不可信（同一事实只留一份，宪法第 8 条）。
    tlog.finish(
        done.get("status").and_then(|s| s.as_str()).unwrap_or("?"),
        code,
    );
    let _ = on_event.send(done);
    Ok(())
}

/// Claude Code 往 stderr 打的**诊断标签行**里，已核实与「这一轮失败」无关的那几类。
/// 值是方括号里 `claude-code:` 之后那段。**只收实测过、读过源码确认是纯诊断的**，别图省事写成 `claude-code:*`：
/// 将来它新增一个真有信息量的标签，被我们一刀切吞掉，比多显示一行噪声更糟。
///
/// - `unrecognized_model`：模型名不在 Claude Code 内置清单里时打一行
///   `[claude-code:unrecognized_model] {"model":"…","query_source":"…"}`（claude 2.1.280 实测）。
///   走虾盘云网关换成 deepseek / glm 这类非 Anthropic 模型名就必然出现，**跟请求成不成功无关**
///   （纯文字轮次它照打、照样成功）。
const STDERR_NOISE_TAGS: &[&str] = &["unrecognized_model"];

/// 这一行是不是上面那类已知噪声。只认行首（容忍前导空白）的 `[claude-code:<tag>]`。
fn is_stderr_noise(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("[claude-code:")
        .and_then(|rest| rest.split_once(']'))
        .is_some_and(|(tag, _)| STDERR_NOISE_TAGS.contains(&tag.trim()))
}

/// 这一轮失败时**拿什么当失败原因展示**。
///
/// 优先级：stderr 去掉已知噪声行后的正文 → stdout 上 `result` 事件的失败正文 → 最后才退回带噪声的原始 stderr。
///
/// 🔴 为什么要滤噪声：原先是「stderr 非空就用 stderr」。可 claude 的真报错（400 / 403 / 余额…）全在 stdout，
/// stderr 里躺着的只有 `[claude-code:unrecognized_model] …` 这一行 —— 它让 stderr「非空」，于是真正的
/// `API Error: 400 …` 被挤掉，失败卡片上只剩一行跟故障无关的诊断，客户和我们都被它带去查模型名。
/// 压根没有别的可展示时才退回原始 stderr：宁可显示一行噪声，也不要显示空串（空串会让界面退化成「claude 退出码 1」）。
fn failure_text(stderr: &str, result_error: Option<&str>) -> String {
    let real = stderr
        .lines()
        .filter(|l| !is_stderr_noise(l))
        .collect::<Vec<_>>()
        .join("\n");
    let real = real.trim();
    if !real.is_empty() {
        return real.to_string();
    }
    match result_error.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => e.to_string(),
        None => stderr.trim().to_string(),
    }
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
            let (args, teach, _) = build_args(prompt, model, system, resume, None);
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
            let (args, teach, _) = build_args("帮我调试一下这个软件", None, system, None, None);
            let i = args
                .iter()
                .position(|a| a == "--append-system-prompt")
                .expect("运行环境约束必须无条件带上");
            let sys = &args[i + 1];
            // 规矩在 2026-11-02 换了口径：从「绝不跑不会结束的命令」改成「永不返回的进程要用后台起」。
            // 判据跟着换成新那句 —— 承重的事实不变（无头面板没人按 Ctrl+C，卡住的就是常驻进程），
            // 变的只是它不再顺手把「三分钟以上的长活」一起禁掉。
            assert!(sys.contains("永远不返回"), "「永不返回的进程要用后台起」这条规矩没进系统提示");
            assert!(!sys.contains("两分钟内结束"), "「两分钟内结束」已经撤掉，它把 Claude Code 的长活砍了一半");
            if let Some(s) = system.map(str::trim).filter(|s| !s.is_empty()) {
                assert!(sys.contains(s), "专家 persona 被 GUARD 顶掉了");
            }
            assert!(!teach.iter().any(|a| a.contains("运行环境")), "GUARD 漏进 teach: {teach:?}");
        }
    }

    /// 权限模式：**客户自己配了就听他的**，没配过才由我们给免审批。
    ///
    /// 承重的是第一条 —— 以前这里写死 `bypassPermissions`，客户在 Claude Code 里
    /// 选「逐条问我 / 先给计划」进对话面板当场失效。反过来也不能「不传」：
    /// 不传 = 逐条审批，这个无头面板等于没法用。
    #[test]
    fn permission_mode_follows_caller_and_defaults_to_bypass() {
        let (args, _, _) = build_args("p", None, None, None, Some("plan"));
        let i = args.iter().position(|a| a == "--permission-mode").expect("缺 --permission-mode");
        assert_eq!(args.get(i + 1).map(String::as_str), Some("plan"), "客户配过的档位必须原样透传");

        let (args, _, _) = build_args("p", None, None, None, None);
        let i = args.iter().position(|a| a == "--permission-mode").expect("缺 --permission-mode");
        assert_eq!(args.get(i + 1).map(String::as_str), Some("bypassPermissions"), "没配过才用免审批");
    }

    /// 失败卡片的原因文：stderr 里只有 `unrecognized_model` 诊断行时，必须让位给 stdout 上真正的 API 错误。
    #[test]
    fn failure_text_prefers_real_api_error_over_unrecognized_model_noise() {
        let noise = "[claude-code:unrecognized_model] {\"model\":\"glm-5.3-flash\",\"query_source\":\"repl_main_thread\"}\n";
        let api = "API Error: 400 The request is invalid: the request was rejected by an internal MaaS component";
        // 只有噪声 + stdout 有真错误 → 展示真错误，噪声不出现
        let t = failure_text(noise, Some(api));
        assert_eq!(t, api);
        assert!(!t.contains("unrecognized_model"), "噪声不该混进失败原因: {t}");
        // stderr 空（老行为）→ 照旧退到 stdout 的失败正文
        assert_eq!(failure_text("", Some(api)), api);
        assert_eq!(failure_text("  \n", Some(api)), api);
    }

    /// stderr 里有真内容时，仍然以 stderr 为准（老行为不变），只是把夹在里面的噪声行摘掉；
    /// 没有任何别的可展示时才退回带噪声的原文，不给空串。
    #[test]
    fn failure_text_keeps_real_stderr_and_never_returns_empty_when_only_noise() {
        let noise = "[claude-code:unrecognized_model] {\"model\":\"deepseek-v4-flash\"}";
        // 噪声夹在真 stderr 中间 → 真内容保留、噪声摘掉，即使 stdout 也有错误也沿用 stderr 优先
        let mixed = format!("{noise}\nError: spawn failed\n");
        assert_eq!(failure_text(&mixed, Some("API Error: 400 x")), "Error: spawn failed");
        // 只有噪声、stdout 也没错误正文 → 退回原文（宁可显示一行噪声，也不要空串）
        assert_eq!(failure_text(noise, None), noise);
        assert_eq!(failure_text(noise, Some("   ")), noise);
        // 全空 → 空串（调用方自己会退到「claude 退出码 N」）
        assert_eq!(failure_text("", None), "");
    }

    /// 噪声判定只认**已核实**的标签，不能一刀切 `[claude-code:*]`；也只认行首，正文里提到它不算。
    #[test]
    fn stderr_noise_only_matches_known_tags_at_line_start() {
        assert!(is_stderr_noise("[claude-code:unrecognized_model] {\"model\":\"x\"}"));
        assert!(is_stderr_noise("   [claude-code:unrecognized_model]"));
        assert!(!is_stderr_noise("[claude-code:some_future_tag] real problem"));
        assert!(!is_stderr_noise("[claude-code:unrecognized_model"), "没闭合的方括号不算");
        assert!(!is_stderr_noise("Error: see [claude-code:unrecognized_model] above"));
        assert!(!is_stderr_noise("API Error: 400"));
    }

    /// 多行提示词不许内联进 teach（粘进 shell 会断在半路），且要如实报告没内联。
    #[test]
    fn multiline_prompt_reported_as_not_inlined() {
        let (_, teach, inlined) = build_args("第一行\n第二行", None, None, None, None);
        assert!(!inlined);
        assert!(teach.is_empty());

        let (_, teach, inlined) = build_args("写个脚本", None, None, Some("s1"), None);
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

    #[test]
    fn build_args_keeps_resume_in_each_surface_and_omits_blank_model() {
        let (args, teach, _) = build_args("the prompt", Some("model-a"), None, Some("sid-123"), None);
        let resume = args.iter().position(|a| a == "--resume").expect("argv 缺 --resume");
        let prompt = args.iter().position(|a| a == "-p").expect("argv 缺 -p");
        assert_eq!(args.get(resume + 1).map(String::as_str), Some("sid-123"));
        assert!(resume < prompt, "--resume 必须在 -p 之前: {args:?}");
        let taught_resume = teach.iter().position(|a| a == "--resume").expect("teach 缺 --resume");
        let taught_prompt = teach.iter().position(|a| a == "the prompt").expect("teach 缺提示词");
        assert_eq!(teach.get(taught_resume + 1).map(String::as_str), Some("sid-123"));
        assert!(taught_resume < taught_prompt, "teach 的 --resume 必须在提示词之前: {teach:?}");

        let (args, teach, _) = build_args("prompt", None, None, None, None);
        assert!(!args.iter().any(|a| a == "--resume"));
        assert!(!teach.iter().any(|a| a == "--resume"));
        let (args, teach, _) = build_args("prompt", Some("   "), None, None, None);
        assert!(!args.iter().any(|a| a == "--model"));
        assert!(!teach.iter().any(|a| a == "--model"));
        // sol 建议：非空 model 也要被观察到——上面只钉了空白省略，没钉传入生效
        let (args, teach, _) = build_args("prompt", Some("model-a"), None, None, None);
        assert_eq!(args.iter().position(|a| a == "--model").map(|i| args.get(i + 1).map(String::as_str)), Some(Some("model-a")));
        assert!(teach.iter().any(|a| a == "model-a"), "teach 也要带非空 model: {teach:?}");
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::super::protocol::ProtocolState;

    #[test]
    fn result_error_captures_both_error_shapes_and_still_emits_usage() {
        for (result, expected) in [
            (serde_json::json!({"type":"result","is_error":true,"result":"403 token quota is not enough"}), Some("403 token quota is not enough")),
            (serde_json::json!({"type":"result","subtype":"error_during_execution","result":"boom"}), Some("boom")),
            (serde_json::json!({"type":"result","subtype":"success","result":"ignored"}), None),
            (serde_json::json!({"type":"result","is_error":true,"result":"   "}), None),
        ] {
            let mut state = ProtocolState::new(false);
            let evs = state.map_event(&result);
            assert_eq!(evs.len(), 1);
            assert_eq!(evs[0]["kind"], "usage");
            assert_eq!(state.result_error(), expected);
        }
    }

    #[test]
    fn first_init_is_fresh_only_for_a_new_non_resume_turn() {
        let init = serde_json::json!({"type":"system","subtype":"init","session_id":"s"});
        let mut resumed = ProtocolState::new(true);
        assert_eq!(resumed.map_event(&init)[0]["fresh"], false);

        let mut new_turn = ProtocolState::new(false);
        assert_eq!(new_turn.map_event(&init)[0]["fresh"], true);
        assert_eq!(new_turn.map_event(&init)[0]["fresh"], false);
    }

    #[test]
    fn tool_result_handles_text_forms_and_unknown_tool_ids() {
        let mut state = ProtocolState::new(false);
        state.map_event(&serde_json::json!({"type":"content_block_start","content_block":{"type":"tool_use","id":"known","name":"Bash"}}));
        let multi = state.map_event(&serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"known","content":[{"text":"one"},{"image":"ignored"},{"text":"two"}]}]}}));
        assert_eq!(multi[0]["name"], "Bash");
        assert_eq!(multi[0]["output"], "one\ntwo");

        let string = state.map_event(&serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"known","content":"raw"}]}}));
        assert_eq!(string[0]["output"], "raw");
        for content in [serde_json::json!({"not":"text"}), serde_json::Value::Null] {
            let evs = state.map_event(&serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"known","content":content}]}}));
            assert_eq!(evs.len(), 1);
            assert_eq!(evs[0]["output"], "");
        }
        let unknown = state.map_event(&serde_json::json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"never-seen","content":"still render"}]}}));
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0]["kind"], "tool_end");
        assert_eq!(unknown[0]["name"], "");
    }
}

/// 「这一轮要不要续接上文」「这条 sid 要不要写进账本」的回归钉
/// （2026-11-23 `Prompt is too long` 事故里一起暴露的两条：误报换模型、失败轮顶掉可用 sid）。
#[cfg(test)]
mod thread_identity_tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    /// ★ 客户现场那条：显式 `deepseek-flash` 与「跟随驱动设置」实际是同一个模型，
    /// 以前被判成换模型 —— 上文白丢，还弹一句误导人的提示。
    #[test]
    fn follow_driver_matching_stored_model_keeps_context() {
        let (resume, note) = decide_resume(s("sid-1"), s("deepseek-flash"), s("deepseek-flash"));
        assert_eq!(resume.as_deref(), Some("sid-1"), "同一个模型不许断上文");
        assert!(note.is_none(), "同一个模型不许报「换了模型」");
    }

    /// 真换模型（kimi → deepseek）必须断，并**明说**是换模型（thinking-400 那条红线）。
    #[test]
    fn real_model_switch_drops_resume_and_says_so() {
        let (resume, note) = decide_resume(s("sid-1"), s("kimi-k3"), s("deepseek-flash"));
        assert!(resume.is_none());
        assert_eq!(note, Some(MODEL_SWITCH_NOTE));
    }

    /// 客户自持 Claude 配置时我们不知道本轮用哪个模型：断（安全方向），但**不许谎称换了模型**。
    #[test]
    fn unknown_current_model_drops_without_claiming_a_switch() {
        let (resume, note) = decide_resume(s("sid-1"), s("deepseek-flash"), None);
        assert!(resume.is_none());
        assert_eq!(note, Some(MODEL_UNKNOWN_NOTE));
        assert_ne!(note, Some(MODEL_SWITCH_NOTE));
    }

    /// 老盘数据没记过模型（`model: null`）→ 保持老行为照常续接，不许发版后让存量客户无辜掉一次上文。
    #[test]
    fn legacy_unknown_stored_model_still_resumes() {
        let (resume, note) = decide_resume(s("sid-1"), None, s("deepseek-flash"));
        assert_eq!(resume.as_deref(), Some("sid-1"));
        assert!(note.is_none());
    }

    /// 前面几道闸已经把 resume 摘掉时（换过文件夹 / 陈旧 sid），不再叠加「换模型」的提示。
    #[test]
    fn no_model_note_when_resume_already_dropped() {
        let (resume, note) = decide_resume(None, s("kimi-k3"), s("deepseek-flash"));
        assert!(resume.is_none());
        assert!(note.is_none(), "一条失败轮只该有一句原因");
    }

    /// ★ 失败轮不许把 sid 写进账本 —— 只收到 init 就被上游拒掉的那一轮，
    /// 会把上一次那条**能续上的**会话顶掉（界面上还写着「换回原来那个模型就能接上」）。
    #[test]
    fn failed_turn_does_not_overwrite_the_usable_session() {
        // code=None 且不是中断（例如上游报错退出但拿不到退出码）
        assert!(!should_remember_session(None, false, false, true), "失败轮不许记账");
        assert!(!should_remember_session(Some(1), false, false, true), "非 0 退出不许记账");
        assert!(should_remember_session(Some(0), false, false, false), "跑成了才记账");
        assert!(
            should_remember_session(None, false, true, true),
            "人按停止但已经出过内容 —— 那条会话有东西，接着聊是正当需求"
        );
        assert!(!should_remember_session(None, false, true, false), "只出了 init 就被停：不算有内容");
    }

    /// ★ 看门狗判死的超时轮**也没有退出码** —— 不能靠「没有退出码 + 有输出」把它当成中断
    /// 去记账（那一轮界面报的是 timeout，记进去等于把一条判死的会话留给下一轮）。
    #[test]
    fn stalled_turn_is_never_recorded_even_with_output() {
        assert!(!should_remember_session(None, true, true, true), "超时轮不许记账");
        assert!(!should_remember_session(Some(0), true, false, true), "卡死判定优先于退出码");
    }
}

/// 收尾事件里 `hint` 的带与不带（2026-11-23 事故：一句「你好」被说成「聊太长了」）。
#[cfg(test)]
mod settle_event_tests {
    use super::*;

    fn hint() -> Option<(String, String, String)> {
        Some(("/h/.claude/settings.json".into(), "CLAUDE_CODE_MAX_CONTEXT_TOKENS".into(), "4000".into()))
    }

    /// ★ 客户现场那条：报错 + 探针命中本机的小上限 ⇒ 必须带结构化 hint，
    /// 界面才有依据说「不是聊天记录太长」。
    #[test]
    fn context_limit_hint_rides_along_with_the_error() {
        let ev = settle_event(false, false, Some(1), "Prompt is too long".into(), hint());
        assert_eq!(ev["status"], "error");
        assert_eq!(ev["hint"]["kind"], "context_limit");
        assert_eq!(ev["hint"]["value"], "4000");
        assert_eq!(ev["hint"]["key"], "CLAUDE_CODE_MAX_CONTEXT_TOKENS");
        assert_eq!(ev["message"], "Prompt is too long", "原始报错必须原样保留");
    }

    /// 真聊太长（探针没命中）⇒ 不带 hint，界面保持原来的分类建议。
    #[test]
    fn genuine_long_conversation_gets_no_hint() {
        let ev = settle_event(false, false, Some(1), "Prompt is too long".into(), None);
        assert_eq!(ev["status"], "error");
        assert!(ev.get("hint").is_none(), "配置正常时不许乱指配置");
    }

    /// 成功 / 中断 / 超时都不该带 hint —— 它们根本不是这个病。
    #[test]
    fn other_outcomes_never_carry_the_hint() {
        let ok = settle_event(false, false, Some(0), String::new(), hint());
        assert_eq!(ok["status"], "ok");
        assert!(ok.get("hint").is_none());

        let interrupted = settle_event(false, true, None, "x".into(), hint());
        assert_eq!(interrupted["status"], "interrupted");

        let timeout = settle_event(true, false, None, String::new(), hint());
        assert_eq!(timeout["status"], "timeout");
        assert_eq!(timeout["stall_secs"], STALL_SECS);
        assert!(timeout.get("hint").is_none());
    }
}
