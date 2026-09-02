//! U-King 原生对话 —— Rust curl 流式调虾盘云 + **工具循环 + 审批模式**（copy Codex）。
//!
//! ## 审批模式（对齐 Codex：自动 / 全授权）
//! - `ask`  每步确认：写文件 / 跑命令都要用户点批准。
//! - `auto` 自动模式：写文件自动、跑命令仍要批准（Codex 的 Auto Edit）。
//! - `full` 全授权：写文件 + 跑命令都自动（Codex 的 Full Auto）。
//! **危险命令（格盘/关机/删系统盘等）无论哪个模式一律 Rust 端硬拦**——全授权也不放行。
//!
//! ## 安全（全在 Rust）
//! 文件/命令只在用户选的「工作文件夹」内动；`..`/绝对路径越界拒；命令走硬黑名单 + 超时 + 输出封顶。
//! 绝不信模型或前端传来的路径/命令。
//!
//! ## 工具
//! generate_image（零风险）· generate_video（零风险，异步出片，复用 video.rs Seedance 管线）·
//! list_dir/read_file（读，自动）· write_file（写，按模式审批）·
//! run_command（跑命令，按模式审批 + 硬黑名单；可用它跑 `claude -p`/`codex exec` 委派复杂编程给专业 agent）。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::ipc::Channel;

/// 缺省端点 —— 虾盘云。**这是「默认」，不是「唯一」**（2026-08-21）。
///
/// 以前这里是个写死的 const，工作台对话就只能走虾盘云：客户自己有小米 TokenPlan / DeepSeek
/// 官方套餐，在「AI 设置」里配好了、四个 CLI 都切过去了，唯独工作台那个对话框还在拿
/// 设备钱包的 Key 打我们的端点。**我们把自己写进了别人的必经之路上。**
///
/// 现在端点由调用方给（`base_url`），不给才回落到这里。虾盘云仍是开箱默认 ——
/// 推荐可以，挡路不行。
const DEFAULT_ENDPOINT: &str = "https://api.u-claw.org.cn/v1/chat/completions";

/// 把供应商的 OpenAI base（`https://x/v1`）补成 chat completions 端点。
///
/// 已经指到 `/chat/completions` 的原样返回 —— 客户在「AI 设置」里填的可能是任一种，
/// 我们不该因为他多写/少写一截就把请求打到 404 上。空的一律回落默认端点：
/// **拿不准就走已知能用的那条**，别自己拼一个畸形 URL 再让客户去猜为什么不通。
fn chat_endpoint(base_url: Option<&str>) -> String {
    let b = base_url.unwrap_or("").trim().trim_end_matches('/').to_string();
    if b.is_empty() {
        return DEFAULT_ENDPOINT.to_string();
    }
    if b.ends_with("/chat/completions") {
        b
    } else {
        format!("{b}/chat/completions")
    }
}
const MAX_STEPS: usize = 8;
const MAX_READ_BYTES: usize = 200_000;
const MAX_CMD_OUTPUT: usize = 20_000;
const CMD_OUTPUT_HARD_CAP: usize = 200_000; // 缓冲上限（>MAX_CMD_OUTPUT，好留尾部给 head_tail 首尾截断）
const CMD_TIMEOUT_SECS: u64 = 60;

fn running() -> &'static Mutex<HashMap<String, Child>> {
    static R: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}
fn approvals() -> &'static Mutex<HashMap<String, Sender<bool>>> {
    static A: OnceLock<Mutex<HashMap<String, Sender<bool>>>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(HashMap::new()))
}
fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

#[cfg(windows)]
fn curl_command() -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(crate::installer::system_tool("curl"));
    c.creation_flags(0x0800_0000);
    c
}
#[cfg(not(windows))]
fn curl_command() -> std::process::Command {
    std::process::Command::new("curl")
}

/// PATH 注入（复用 installer::search_paths，让 claude/codex/python/node 等在命令里能找到）。
fn path_env() -> String {
    let mut dirs: Vec<String> = crate::installer::search_paths(crate::installer::portable_node_dir().as_deref())
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    if let Ok(sys) = std::env::var("PATH") {
        dirs.push(sys);
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    dirs.join(sep)
}

/// 危险命令硬黑名单（任何模式都拦，含全授权）。宁可误拦也不误放。
/// 先把空白归一（多空格/Tab → 单空格）——让 `rm  -rf   /`、`rm\t-rf /` 这种也命中子串规则，
/// 挡住"多加空格"绕过。子串匹配天然对 `timeout 30 rm -rf /` 这类外壳包裹也生效（内层仍是子串）。
fn is_dangerous(cmd: &str) -> bool {
    let c = cmd.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    const BAD: &[&str] = &[
        // 格盘 / 分区 / 关机重启 / 系统修复
        "format ", "diskpart", "mkfs", "fdisk", "shutdown", "reboot", "chkdsk /f", "sfc /scannow",
        // 递归删根 / 删系统盘
        "rm -rf /", "rm -fr /", "rm -rf ~", "rm -rf .", "del /f /s /q c:", "rd /s /q c:",
        "rmdir /s /q c:", ":\\windows", "reg delete hklm", "cipher /w", "vol ",
        // 覆写块设备
        "> /dev/sd", "of=/dev/sd", "dd if=",
        // 下载即执行（管道进 shell / IEX）—— 一键中招最常见
        "| sh", "|sh", "| bash", "|bash", "| iex", "|iex", "iex(", "iex ", "invoke-expression",
        "certutil -urlcache", "bitsadmin /transfer",
        // 影卷/备份/启动项破坏 + fork 炸弹 + 全球可写
        "vssadmin delete", "wbadmin delete", "bcdedit /set", ":(){", "chmod -r 777", "chmod 777 /",
    ];
    BAD.iter().any(|b| c.contains(b))
}

/// 原子写：同目录 temp 文件写好 + rename 覆盖 —— 写一半崩也不会损坏客户已有文件（对齐宪法「写入原子」）。
/// Windows 上刚落盘的文件常被杀软/编辑器/索引器瞬时锁（os error 32/33），rename 退避重试。
fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".uking-write-{}.tmp", next_id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content)?;
        let _ = f.sync_all();
    }
    let mut last: Option<std::io::Error> = None;
    for (i, wait) in [25u64, 50, 100, 200, 400].into_iter().enumerate() {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                if i < 4 {
                    std::thread::sleep(Duration::from_millis(wait));
                }
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "rename 失败")))
}

/// 正在跑命令的子进程 PID（按 task_id）——给 kill 整棵进程树用。
fn shell_pids() -> &'static Mutex<HashMap<String, u32>> {
    static S: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 杀整棵进程树（不漏子/孙进程）。Windows 用 `taskkill /F /T /PID`——`cmd /C claude -p` 会派生
/// node 等孙进程，裸 `child.kill()` 只杀 cmd 会留孤儿；`/T` 连整棵树一起收。Unix 杀进程组兜底杀单进程。
/// 这个 pid 现在还在不在。给 `chat_inspect` 判「那份在跑记录是活的还是尸体」用。
///
/// **必须真的去问系统**，不能拿别的事实推：一开始我用「会话心跳里的 pid 是不是它」来判，
/// 结果任何一次 `action run` 自己也会刷新那份心跳 —— 于是一个**正在跑**的轮次被判成
/// `owner_alive:false`，进而被踢出 `stalled_now`，客户正卡着而动作回答「没有卡住的」。
/// 一个会说反话的诊断比没有诊断更坏。
///
/// 探不到就返回 `false`（保守：宁可说「那是残留」也不谎报「正在跑」）。
pub(crate) fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new(crate::installer::system_tool("tasklist"))
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(0x0800_0000)
            .output();
        // 没匹配到时 tasklist 打的是「信息: 没有运行的任务…」，里面不会有这个 pid
        return out
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub(crate) fn kill_tree_by_pid(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new(crate::installer::system_tool("taskkill"))
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .status();
        let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    }
}

/// 命令输出截断：超长时保留「首一半 + 尾一半」，中间省略——只留头会把结尾的报错吞掉（借鉴 grok）。
pub(crate) fn head_tail(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let half = cap / 2;
    let mut h = half;
    while h > 0 && !s.is_char_boundary(h) {
        h -= 1;
    }
    let mut t = s.len() - half;
    while t < s.len() && !s.is_char_boundary(t) {
        t += 1;
    }
    let omitted = t.saturating_sub(h);
    format!("{}\n…（中间省略约 {} 字符，只保留首尾）…\n{}", &s[..h], omitted, &s[t..])
}

/// auto 模式下：一条命令是否「明显只读/安全」可自动跑（否则仍要用户批准）。保守——拿不准就当不安全，
/// 让它去问。借鉴 grok auto_mode：只在本机、可撤销、不越界的才放；写盘/删除/装包/网络/git 写/提权/管道链一律问。
fn is_safe_readonly_command(cmd: &str) -> bool {
    if is_dangerous(cmd) {
        return false;
    }
    let c = cmd.trim().to_lowercase();
    if c.is_empty() {
        return false;
    }
    // 含这些一律要问（写重定向/删除移动/建目录/装包/网络/git 改动/提权/后台/命令拼接/子命令替换）
    const UNSAFE_HINT: &[&str] = &[
        ">", "|", "&", ";", "`", "$(", "rm ", "del ", "erase ", "move ", "ren ", "cp ", "mv ",
        "mkdir", "rmdir", "install", "uninstall", "curl ", "wget ", "iwr", "irm", "invoke-web",
        "git push", "git commit", "git reset", "git checkout", "git clean", "git rebase",
        "git merge", "git pull", "git fetch", "sudo", "runas", "start ", "reg ", "sc ", "net ",
        "taskkill", "kill ", "chmod", "chown", "attrib", "set ", "setx", "export ",
    ];
    if UNSAFE_HINT.iter().any(|h| c.contains(h)) {
        return false;
    }
    // 首个有意义 token（跳过 timeout/env 等外壳）
    let first = c
        .split_whitespace()
        .find(|t| !matches!(*t, "timeout" | "env" | "nice" | "stdbuf"))
        .unwrap_or("");
    // git / 包管理 / cargo 只放只读子命令
    let sub = || c.split_whitespace().nth(1).unwrap_or("");
    match first {
        "git" => matches!(
            sub().as_ref(),
            "status" | "log" | "diff" | "show" | "branch" | "remote" | "config" | "rev-parse"
                | "ls-files" | "blame" | "describe" | "tag" | "shortlog"
        ),
        "npm" | "pnpm" | "yarn" => matches!(
            sub().as_ref(),
            "test" | "ls" | "list" | "view" | "outdated" | "why" | "root" | "run"
                | "--version" | "-v"
        ),
        "cargo" => matches!(
            sub().as_ref(),
            "check" | "build" | "test" | "clippy" | "fmt" | "tree" | "--version" | "-v"
        ),
        // 纯只读命令
        other => matches!(
            other,
            "ls" | "dir" | "type" | "cat" | "head" | "tail" | "echo" | "pwd" | "cd" | "whoami"
                | "date" | "find" | "where" | "which" | "tree" | "wc" | "sort" | "uniq" | "grep"
                | "rg" | "ver" | "hostname" | "printenv"
        ),
    }
}

/// diff 预览发给前端时给内容封顶（太大不塞满事件 / 卡 WebView）。按字符边界安全截断。
fn diff_cap(s: &str) -> String {
    const CAP: usize = 60_000;
    if s.len() <= CAP {
        return s.to_string();
    }
    let mut end = CAP;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…（内容过长，diff 预览已截断）", &s[..end])
}

/// 🔴 **每个工具的 `parameters` 必须带 `required` 数组，全可选就写 `[]`。**
///
/// 虾盘云上游对缺 `required` 的函数定义直接回 400 invalid_argument，而且是**整轮请求**被拒 ——
/// 一个工具写漏，挂着它的那轮对话一个字也出不来。0.9.99 预发时实测：`uking_action` 和
/// `list_dir` 两个漏了，于是 GUI 对话（永远挂 uking_action）每一轮必 400，选了工作文件夹
/// 更是连普通聊天都进不去；而 `--chat-test`（不挂工具）全绿 —— 这就是它躲过所有跑道的原因。
///
/// 双向变异验证过（curl 直打 api.u-claw.org.cn，deepseek-v4-flash / v4-pro 同）：
/// 给 `uking_action` 补上 `required` → 200；把 `generate_image` 的 `required` 删掉 → 400。
/// 空数组 `"required": []` 上游接受，所以「全可选」的语义不必为此改成必填。
///
/// 落地保险：`tools_schema_lint()` 把这条变成门禁，`--brain-actions-test` 进网络之前先跑它。
fn tools_spec(has_workspace: bool, allow_actions: bool) -> Value {
    let mut arr = vec![json!({
        "type": "function",
        "function": {
            "name": "generate_image",
            "description": "根据文字描述生成一张图片。用户想画图/作图/出图/生成图片/配图时调用。",
            "parameters": { "type": "object", "properties": { "prompt": { "type": "string", "description": "要画的画面描述（越具体越好）" } }, "required": ["prompt"] }
        }
    })];
    // 生成视频：原生工具（零风险，不需工作文件夹）——治「让做视频它退化成静态图/不调 uking-aigc」。
    // 直连火山 Seedance 真实出片管线（video.rs），异步出片自动进右侧预览。
    arr.push(json!({
        "type": "function",
        "function": {
            "name": "generate_video",
            "description": "根据文字描述生成一段短视频（文生视频，火山 Seedance 模型）。用户想做视频/生成视频/文生视频/短视频/做动画/出片时调用。出片是异步的，通常要等 1~3 分钟。把画面写具体：主体 + 怎么动 + 镜头/景别 + 风格/氛围。",
            "parameters": { "type": "object", "properties": { "prompt": { "type": "string", "description": "视频画面描述（主体+动作+镜头+风格，越具体越好）" } }, "required": ["prompt"] }
        }
    }));
    if has_workspace {
        arr.push(json!({ "type": "function", "function": { "name": "list_dir", "description": "列出工作文件夹里某目录下的文件和子目录。", "parameters": { "type": "object", "properties": { "path": { "type": "string", "description": "相对工作文件夹的路径，空或 \".\" 表示根" } }, "required": [] } } }));
        arr.push(json!({ "type": "function", "function": { "name": "read_file", "description": "读取工作文件夹里一个文本文件的内容。", "parameters": { "type": "object", "properties": { "path": { "type": "string", "description": "相对工作文件夹的文件路径" } }, "required": ["path"] } } }));
        arr.push(json!({ "type": "function", "function": { "name": "write_file", "description": "在工作文件夹里新建文件、或整体覆盖一个文件的全部内容。新建文件或大改时用；只改已有文件的一小段请优先用 edit_file（更省、更安全，不会冲掉别处）。", "parameters": { "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" } }, "required": ["path", "content"] } } }));
        arr.push(json!({ "type": "function", "function": { "name": "edit_file", "description": "对已有文本文件做精确的查找替换（外科式改写，只动匹配到的那段，其余不变）。改代码/文档的局部时首选。old_string 必须在文件里唯一命中（否则会报错让你补足上下文）；要替换所有出现处设 replace_all=true。", "parameters": { "type": "object", "properties": { "path": { "type": "string", "description": "相对工作文件夹的文件路径" }, "old_string": { "type": "string", "description": "要被替换的原文（带足够上下文以唯一定位）" }, "new_string": { "type": "string", "description": "替换成的新内容" }, "replace_all": { "type": "boolean", "description": "是否替换所有出现处，默认 false（要求唯一命中）" } }, "required": ["path", "old_string", "new_string"] } } }));
        arr.push(json!({ "type": "function", "function": { "name": "run_command", "description": "在工作文件夹里执行一条命令行（PowerShell/cmd）。装依赖、跑脚本、跑测试时用；复杂编程任务优先运行 `claude -p \"任务\"`（首选，Claude Code），其次 `codex exec \"任务\"` 或 `hermes`，委派给专业 agent，结果会回流。", "parameters": { "type": "object", "properties": { "command": { "type": "string", "description": "要执行的完整命令" } }, "required": ["command"] } } }));
    }
    // 对话大脑能操作 U-King 自己（影核动作核心）。只挂给 GUI 对话；无头/自动化不挂，
    // 因为写动作要人确认，无人值守下 full 模式会自动放行，不能让它也够得着。
    if allow_actions {
        arr.push(json!({
            "type": "function",
            "function": {
                "name": "uking_action",
                "description": "操作/查询 U-King 自己（这台机器上装的管家）：体检、硬件、Ollama 本地大模型、Token 压缩机、用量花费、优化报告、驱动、定时任务、网站体检、远程协助、AI 作图/视频历史，以及改配置、装工具、备份、清理等管理操作。不知道有哪些可用动作时，把 action_id 传空字符串（\"\"）会返回全部动作清单和用途。写操作（改配置/删除/安装/卸载/备份/清理）会自动弹确认框，用户点了同意才执行，被拒绝就停下。",
                "parameters": { "type": "object", "properties": {
                    "action_id": { "type": "string", "description": "影核动作 id，一律 runtime.* 前缀，形如 runtime.stack.inspect / runtime.usage_meter.inspect / runtime.provider.save / runtime.backup.create。不传或传空则列出全部可用动作" },
                    "input": { "type": "object", "description": "该动作的入参对象（可选）。绝大多数查询动作不需要；写动作按需传。写动作不需要自己带 confirm，确认由用户点击完成" }
                }, "required": [] }
            }
        }));
    }
    Value::Array(arr)
}

/// 工具 schema 门禁：返回所有「参数缺 `required` 数组」的工具名，空 = 合格。
///
/// 不联网、不花钱，所以可以放在任何一条跑道前面白跑。**故意把两种组合都摊开查**
/// （有/无工作文件夹 × 挂/不挂动作核心）—— 漏掉的那两个正好只在其中一种组合里出现。
pub fn tools_schema_lint() -> Vec<String> {
    let mut bad = Vec::new();
    for (ws, act) in [(false, false), (false, true), (true, false), (true, true)] {
        for t in tools_spec(ws, act).as_array().cloned().unwrap_or_default() {
            let name = t.pointer("/function/name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
            let ok = t.pointer("/function/parameters/required").map(|r| r.is_array()).unwrap_or(false);
            if !ok && !bad.contains(&name) {
                bad.push(name);
            }
        }
    }
    bad
}

#[derive(Default, Clone)]
struct ToolCallAcc { id: String, name: String, args: String }

#[tauri::command]
pub async fn chat_send(
    task_id: String,
    messages: Value,
    model: String,
    api_key: String,
    // base_url：供应商的 OpenAI base（`https://x/v1`）。**不传 = 虾盘云**（开箱默认）。
    // 前端从「AI 设置」那份供应商库里取，所以工作台能跟着客户自己配的那家走。
    base_url: Option<String>,
    workspace: Option<String>,
    approval_mode: Option<String>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |v: Value| {
            let _ = on_event.send(v);
        };
        run_chat(task_id, messages, model, api_key, base_url, workspace, approval_mode, true, false, &emit)
    })
    .await
    .map_err(|e| format!("对话调度失败: {e}"))?
}

/// emit：事件出口。GUI 走 Tauri Channel；无头 `--chat-test` 走 println。
fn run_chat(
    task_id: String,
    messages: Value,
    model: String,
    api_key: String,
    base_url: Option<String>,
    workspace: Option<String>,
    approval_mode: Option<String>,
    allow_actions: bool,
    headless: bool,
    emit: &dyn Fn(Value),
) -> Result<(), String> {
    let endpoint = chat_endpoint(base_url.as_deref());
    let mut msgs: Vec<Value> = messages.as_array().cloned().unwrap_or_default();
    // ★ **北桥：把任务本象编译进系统消息**（2Origin「状态 → Context」总线在轻助手这一侧的落点）。
    //
    // 挂成**独立的一条 system**，插在最前面 —— 不去改前端传来的那条：
    // 那条是「你是谁、你能用哪些工具」，这条是「这个任务干到哪了」，两件事分开，
    // 前端将来改它的措辞也不会把状态一起改没。
    //
    // 没有本象就一个字都不加。绝大多数会话没有状态，不该为它们付一份空模板的 token。
    if let Some(o) = crate::origin::load(&task_id) {
        msgs.insert(0, json!({ "role": "system", "content": o.compile_context() }));
    }
    let ws = workspace.filter(|w| Path::new(w).is_dir());
    let mode = approval_mode.unwrap_or_else(|| "ask".into());
    let tools = tools_spec(ws.is_some(), allow_actions);
    // 静默账本 —— 和 claude / codex 共用一份（`agent/mod.rs::TurnLog`）。轻助手没有 stream-json
    // 事件流，阶段靠 `enter` 手工切；阶段名必须和那两条路一致，否则同一份 chat.log 没法横着比。
    let mut tlog = super::TurnLog::start("chat", "uking", &task_id, Some(&model), false);

    // ── 行为时间轴：**只在这一处接** ────────────────────────────────────────────
    // 不去 `exec_tool` 里逐个 arm 插桩（11 处，漏一个时间轴就开始说谎），而是搭在
    // **已有的事件流**上：每个 arm 本来就在一致地 emit `phase:"result"`（成）/ `"error"`（败），
    // 那是被维护着的成败信号，比我另起一套判断可靠。事件里现成带 `path` / `command`，
    // 连入参都不用再解析一遍。
    //
    // 诚实边界：`list_dir` / `read_file` 是**先 emit result 再去读盘**，读失败只返回错误文本
    // 不补发 error 事件 —— 所以这两个「读」偶尔会被记成成功。写和跑命令（时间轴真正关心的那些）
    // 三个 arm 都规规矩矩发 error，不受影响。要修得动那两个 arm 的顺序，不在本次范围。
    let tap = |v: Value| {
        if v["kind"] == json!("tool") {
            let phase = v["phase"].as_str().unwrap_or("");
            // 只认终态：start / output 是过程，记进去会把一次调用变成三条。
            if phase == "result" || phase == "error" {
                let name = v["name"].as_str().unwrap_or("?");
                // 目标一律脱敏后再落盘：路径带用户名、命令行带 Key，而这份记录客户会导出转发。
                // uking_action 没有 path/command，落的是动作 id（v["action"]）。
                let target = match (v["path"].as_str(), v["command"].as_str(), v["action"].as_str()) {
                    (Some(p), _, _) => crate::journal::redact_path(p, ws.as_deref()),
                    (_, Some(c), _) => crate::journal::redact_cmd(c),
                    (_, _, Some(a)) => a.to_string(),
                    _ => String::new(),
                };
                crate::journal::record_tool(
                    "uking",
                    name,
                    &target,
                    phase == "result",
                    0, // 单个工具的耗时这条路上没有，宁可留 0 也不编一个
                    v["message"].as_str().filter(|_| phase == "error"),
                );
            }
        }
        emit(v);
    };
    // 从这里往下，`emit` 一律指向带记录的那个 —— shadow 掉，免得后面有人漏用原始的。
    let emit: &dyn Fn(Value) = &tap;

    for _step in 0..MAX_STEPS {
        tlog.enter("等模型回话");
        let (tool_calls, api_err) = match stream_once(&Value::Array(msgs.clone()), &tools, &model, &api_key, &endpoint, &task_id, emit) {
            Ok(x) => x,
            Err(e) => { tlog.finish("error", None); let _ = emit(json!({ "kind": "done", "status": "error", "message": e })); return Ok(()); }
        };
        if let Some(e) = api_err { tlog.finish("error", None); let _ = emit(json!({ "kind": "done", "status": "error", "message": e })); return Ok(()); }
        if tool_calls.is_empty() { tlog.finish("ok", None); let _ = emit(json!({ "kind": "done", "status": "ok" })); return Ok(()); }
        let tc_json: Vec<Value> = tool_calls.iter().map(|t| json!({ "id": t.id, "type": "function", "function": { "name": t.name, "arguments": t.args } })).collect();
        msgs.push(json!({ "role": "assistant", "content": Value::Null, "tool_calls": tc_json }));
        for t in &tool_calls {
            // 走 `on_event` 而不是 `enter`：顺带把工具计数记对。汇总行里 tools=0 而实际跑了工具，
            // 比不记更坏 —— 一份会少报的日志，下次没人敢信。
            tlog.on_event("tool_start", Some(&t.name));
            let result = exec_tool(&t.name, &t.args, &task_id, ws.as_deref(), &mode, &api_key, headless, emit);
            tlog.on_event("tool_end", None);
            msgs.push(json!({ "role": "tool", "tool_call_id": t.id, "content": result }));
        }
    }
    tlog.finish("ok", None);
    let _ = emit(json!({ "kind": "done", "status": "ok", "note": "达到工具步数上限" }));
    Ok(())
}

fn stream_once(msgs: &Value, tools: &Value, model: &str, api_key: &str, endpoint: &str, task_id: &str, emit: &dyn Fn(Value)) ->Result<(Vec<ToolCallAcc>, Option<String>), String> {
    let body = json!({ "model": model, "messages": msgs, "tools": tools, "stream": true });
    let body_str = serde_json::to_string(&body).map_err(|e| format!("请求体序列化失败: {e}"))?;
    let mut c = curl_command();
    c.args(["-sS", "-N", "--proxy", "", "--connect-timeout", "20", "-m", "300", "-X", "POST", endpoint, "-H", &format!("Authorization: Bearer {api_key}"), "-H", "Content-Type: application/json", "--data-binary", "@-"]);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("启动 curl 失败: {e}"))?;
    if let Some(mut si) = child.stdin.take() { let _ = si.write_all(body_str.as_bytes()); }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut m) = running().lock() { m.insert(task_id.to_string(), child); }
    let err_buf = Arc::new(Mutex::new(String::new()));
    let err_store = err_buf.clone();
    let err_h = stderr.map(|se| std::thread::spawn(move || { for line in BufReader::new(se).lines().map_while(Result::ok) { if let Ok(mut g) = err_store.lock() { g.push_str(&line); g.push('\n'); } } }));
    let mut api_err: Option<String> = None;
    let mut tools_acc: Vec<ToolCallAcc> = Vec::new();
    if let Some(so) = stdout {
        for line in BufReader::new(so).lines().map_while(Result::ok) {
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                if line.contains("\"error\"") { if let Ok(v) = serde_json::from_str::<Value>(line) { if let Some(m) = v.pointer("/error/message").and_then(|m| m.as_str()) { api_err = Some(m.to_string()); } } }
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" { break; }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue; };
            if let Some(m) = v.pointer("/error/message").and_then(|m| m.as_str()) { api_err = Some(m.to_string()); continue; }
            let delta = v.pointer("/choices/0/delta");
            if let Some(txt) = delta.and_then(|d| d.pointer("/content")).and_then(|c| c.as_str()) { if !txt.is_empty() { let _ = emit(json!({ "kind": "delta", "text": txt })); } }
            if let Some(tcs) = delta.and_then(|d| d.get("tool_calls")).and_then(|t| t.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                    while tools_acc.len() <= idx { tools_acc.push(ToolCallAcc::default()); }
                    let slot = &mut tools_acc[idx];
                    if let Some(id) = tc.get("id").and_then(|i| i.as_str()) { if !id.is_empty() { slot.id = id.to_string(); } }
                    if let Some(name) = tc.pointer("/function/name").and_then(|n| n.as_str()) { if !name.is_empty() { slot.name = name.to_string(); } }
                    if let Some(a) = tc.pointer("/function/arguments").and_then(|a| a.as_str()) { slot.args.push_str(a); }
                }
            }
        }
    }
    if let Some(h) = err_h { let _ = h.join(); }
    if let Some(mut ch) = running().lock().ok().and_then(|mut m| m.remove(task_id)) { let _ = ch.wait(); }
    if api_err.is_none() { let e = err_buf.lock().map(|g| g.trim().to_string()).unwrap_or_default(); if tools_acc.is_empty() && !e.is_empty() { api_err = Some(e); } }
    Ok((tools_acc.into_iter().filter(|t| !t.name.is_empty()).collect(), api_err))
}

fn resolve_in_workspace(workspace: &str, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim().trim_start_matches(['/', '\\']);
    let root = Path::new(workspace).canonicalize().map_err(|_| "工作文件夹无效".to_string())?;
    let mut p = root.clone();
    for comp in Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(seg) => p.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_) => return Err("路径越界（不允许 .. 或绝对路径）".into()),
        }
    }
    if !p.starts_with(&root) { return Err("路径越界".into()); }
    Ok(p)
}

/// 弹审批 → 阻塞等前端 chat_approve（180s 超时默认拒）。
fn ask_approval(task_id: &str, detail: Value, emit: &dyn Fn(Value)) ->bool {
    let approval_id = format!("{task_id}-{}", next_id());
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    if let Ok(mut m) = approvals().lock() { m.insert(approval_id.clone(), tx); }
    let mut ev = detail;
    ev["kind"] = json!("approval");
    ev["id"] = json!(approval_id);
    let _ = emit(ev);
    let ok = rx.recv_timeout(Duration::from_secs(180)).unwrap_or(false);
    if let Ok(mut m) = approvals().lock() { m.remove(&approval_id); }
    ok
}

/// 委派编程命令（首 token 是 claude/codex/hermes）→ 给长超时，因为写代码可能跑几分钟。
fn is_delegation(cmd: &str) -> bool {
    matches!(cmd.trim_start().split_whitespace().next(), Some("claude") | Some("codex") | Some("hermes"))
}

/// 在工作区跑命令：注入 PATH + 委派编程用的虾盘云 env、cwd=工作区、超时 kill、输出封顶。
/// 去掉命令输出里的 ANSI 转义序列（颜色/光标控制）+ 其它 C0 控制字符 —— 前端是 <pre> 不是终端，
/// 这些原样显示成「←[32m」「[?25l」之类的乱码/异常字符（客户反馈「执行日志英文乱码」的真因）。
/// 纯 std，不引依赖。保留 \n \r \t 与所有可见（含中文）字符。GBK 中文乱码是另一码事（需编码库），不在此。
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    // CSI：ESC [ 参数… 结束字节(@-~)
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&n) { break; }
                    }
                }
                Some(']') => {
                    // OSC：ESC ] … BEL 或 ESC \
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n == '\x07' { break; }
                        if n == '\x1b' { if chars.peek() == Some(&'\\') { chars.next(); } break; }
                    }
                }
                _ => { chars.next(); } // 其它 ESC x 双字符序列，跳掉下一个
            }
            continue;
        }
        if c == '\n' || c == '\r' || c == '\t' || c >= ' ' { out.push(c); }
    }
    out
}

/// 委派 `claude -p`/`codex exec`/`hermes` 时子进程继承虾盘云端点 + 对话 Key（免客户单独配置）+ 长超时。
/// 输出**边跑边流**给前端（phase:"output" 增量 chunk），长命令(claude -p 写几分钟)不再干瞪眼。
/// 命令输出流的增量解码：每读到一段字节调一次，返回这一轮可以推走的文本（可能为空）。
///
/// 三种情况（测试报告 #002 的乱码就是老代码不分这三种、逐块 lossy 造成的）：
/// - 到目前为止是完整 UTF-8 → 全部解出；
/// - 结尾是没到齐的 UTF-8 多字节字符（读取按 4096 切块，正切在中文中间）→ 推完整前缀，
///   剩 ≤3 字节留 `pending` 等下一轮；
/// - 中段真有非法字节 = 这条流压根不是 UTF-8（cmd 内建命令的管道输出永远走系统 ANSI
///   代码页，chcp 65001 只管外部程序）→ 置 `acp_mode`，整条流此后固定按 ACP 解，
///   别一半 UTF-8 一半 GBK 来回猜。
fn drain_decoded(pending: &mut Vec<u8>, acp_mode: &mut bool) -> String {
    if !*acp_mode {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                let out = s.to_string();
                pending.clear();
                return out;
            }
            Err(e) if e.error_len().is_none() => {
                let cut = e.valid_up_to();
                let bytes: Vec<u8> = pending.drain(..cut).collect();
                return String::from_utf8_lossy(&bytes).into_owned();
            }
            Err(_) => *acp_mode = true,
        }
    }
    // ACP（GBK 类双字节）同样会被 4096 切断：结尾悬着半个字就留 1 字节到下一轮
    let keep = trailing_dbcs_lead(pending);
    let flush: Vec<u8> = pending.drain(..pending.len() - keep).collect();
    ansi_to_string(&flush)
}

/// 按系统 ANSI 代码页解码（cmd 内建命令 echo/dir/type 的管道输出走的就是它，chcp 管不着）。
/// 用 Windows 自带 MultiByteToWideChar —— 纯 std FFI 不引 crate，且在繁体/日文机器上自动是对的代码页。
#[cfg(windows)]
fn ansi_to_string(bytes: &[u8]) -> String {
    #[link(name = "kernel32")]
    extern "system" {
        fn MultiByteToWideChar(cp: u32, flags: u32, src: *const u8, srclen: i32, dst: *mut u16, dstlen: i32) -> i32;
    }
    const CP_ACP: u32 = 0;
    if bytes.is_empty() { return String::new(); }
    unsafe {
        let need = MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
        if need <= 0 { return String::from_utf8_lossy(bytes).into_owned(); }
        let mut wide = vec![0u16; need as usize];
        let got = MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), bytes.len() as i32, wide.as_mut_ptr(), need);
        if got <= 0 { return String::from_utf8_lossy(bytes).into_owned(); }
        String::from_utf16_lossy(&wide[..got as usize])
    }
}
#[cfg(not(windows))]
fn ansi_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// GBK 类双字节流的结尾是不是悬着半个字：从头扫（首字节 0x81-0xFE 吃两个字节），
/// 尾巴正好剩一个首字节就留 1 个到下一轮 —— 双字节字符被 4096 切断时不出坏字。
fn trailing_dbcs_lead(bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        if (0x81..=0xFE).contains(&bytes[i]) {
            if i + 1 == bytes.len() { return 1; }
            i += 2;
        } else {
            i += 1;
        }
    }
    0
}

fn run_shell(command: &str, cwd: &str, task_id: &str, api_key: &str, emit: &dyn Fn(Value)) -> String {
    #[cfg(windows)]
    let mut c = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        // 中文 Windows 的 cmd 默认 GBK 输出，from_utf8_lossy 一解全是 �（测试报告 #002 的乱码）。
        // 先把本条 cmd 的代码页切到 UTF-8 再跑真命令；>nul 吞掉 chcp 自己那行「Active code page」。
        // ⚠ `65001` 和 `>` 之间的空格**不能省**：数字紧贴 `>` 会被 cmd 解析成句柄重定向
        // （`chcp 65001>nul` 实际执行 `chcp 6500 1>nul`，代码页根本没切）——
        // run_shell_chinese_output_not_mojibake 用例逮的就是它。
        //
        // /C 的载荷用 raw_arg 原样透传：std 的自动引号转义（\"）和 cmd 的引号解析规则对不上，
        // 命令里一带引号就被吃/被拆（装机侧 skill 执行器踩过同一坑，根治也是 raw_arg）。
        c.arg("/C");
        c.raw_arg(format!("chcp 65001 >nul & {command}"));
        c.creation_flags(0x0800_0000);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    c.current_dir(cwd).env("PATH", path_env()).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    // 抑制分页器/交互提示：否则 agent 跑 git log / git commit / 要凭据的命令会卡在 less 或输入上，干等到超时。
    for (k, v) in [("PAGER", "cat"), ("GIT_PAGER", "cat"), ("GIT_TERMINAL_PROMPT", "0"), ("GIT_EDITOR", "true"), ("NO_COLOR", "1"), ("TERM", "dumb")] {
        c.env(k, v);
    }
    // 委派编程的 CLI（claude/codex）继承虾盘云端点 + 对话 Key：客户没配过也能直接用同一套计费。
    // 仅注入到子进程环境，不碰用户的 ~/.claude / ~/.codex 配置文件。
    if !api_key.is_empty() {
        for (k, v) in crate::providers::delegation_env(api_key) { c.env(k, v); }
    }
    let timeout_secs = if is_delegation(command) { 600 } else { CMD_TIMEOUT_SECS };
    let mut child = match c.spawn() { Ok(c) => c, Err(e) => return format!("启动命令失败：{e}") };
    let pid = child.id();
    // 登记 PID：chat_interrupt 能停正在跑的命令（含 claude -p 派生的整棵进程树）
    if let Ok(mut m) = shell_pids().lock() { m.insert(task_id.to_string(), pid); }
    let buf = Arc::new(Mutex::new(String::new()));
    let mut hs = Vec::new();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)] {
        if let Some(mut p) = pipe {
            let b = buf.clone();
            hs.push(std::thread::spawn(move || {
                let mut tmp = [0u8; 4096];
                // 4096 一刀常常正切在多字节字符中间 —— 逐块 lossy 会把切点那个中文变成 �
                // （测试报告 #002 乱码来源之一，跟代码页无关、UTF-8 输出也中招）。
                // 所以攒字节：每轮只推「边界完整」的前缀，结尾没凑齐的留给下一轮。
                let mut pending: Vec<u8> = Vec::new();
                // 一旦断定这条流不是 UTF-8（cmd 内建命令的管道输出永远走系统 ANSI 代码页，
                // chcp 65001 只管外部程序），整条流固定按 ACP 解 —— 别一半 UTF-8 一半 GBK 来回猜。
                let mut acp_mode = false;
                let push = |b: &Arc<Mutex<String>>, s: &str| {
                    if let Ok(mut g) = b.lock() {
                        if g.len() < CMD_OUTPUT_HARD_CAP { g.push_str(&strip_ansi(s)); }
                    }
                };
                loop {
                    match p.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            pending.extend_from_slice(&tmp[..n]);
                            let s = drain_decoded(&mut pending, &mut acp_mode);
                            if !s.is_empty() { push(&b, &s); }
                        }
                    }
                }
                // EOF：进程半途被杀确实可能停在半个字符上，剩多少推多少
                if !pending.is_empty() {
                    let s = if acp_mode { ansi_to_string(&pending) } else { String::from_utf8_lossy(&pending).into_owned() };
                    push(&b, &s);
                }
            }));
        }
    }
    let start = Instant::now();
    let mut timed_out = false;
    let mut streamed = 0usize; // 已 emit 给前端的字节数，只推增量
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }
        // 增量流：把缓冲里还没推过的新输出发给前端（emit 只在本主线程调，无 Send 问题）
        if let Ok(g) = buf.lock() {
            if g.len() > streamed {
                let chunk: String = g[streamed..].to_string();
                streamed = g.len();
                let _ = emit(json!({ "kind": "tool", "name": "run_command", "phase": "output", "command": command, "chunk": chunk }));
            }
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) { kill_tree_by_pid(pid); let _ = child.kill(); timed_out = true; break; }
        std::thread::sleep(Duration::from_millis(150));
    }
    for h in hs { let _ = h.join(); }
    if let Ok(mut m) = shell_pids().lock() { m.remove(task_id); }
    // 收尾：进程退出后 reader 线程可能又写了最后一段，补推
    if let Ok(g) = buf.lock() {
        if g.len() > streamed {
            let _ = emit(json!({ "kind": "tool", "name": "run_command", "phase": "output", "command": command, "chunk": g[streamed..].to_string() }));
        }
    }
    // 首+尾截断：只留头会把结尾报错吞掉，模型看不到错在哪
    let mut out = head_tail(&buf.lock().map(|g| g.clone()).unwrap_or_default(), MAX_CMD_OUTPUT);
    if timed_out { out.push_str(&format!("\n（超时 {timeout_secs}s，已终止）")); }
    if out.trim().is_empty() { out = "（命令已执行，无输出）".into(); }
    out
}

fn exec_tool(name: &str, args_json: &str, task_id: &str, workspace: Option<&str>, mode: &str, api_key: &str, headless: bool, emit: &dyn Fn(Value)) ->String {
    // 空参数当 {}；非空但不是合法 JSON 时，把原始参数回显给模型让它改（别静默变 Null 导致工具莫名失败）
    let args: Value = {
        let t = args_json.trim();
        if t.is_empty() {
            json!({})
        } else {
            match serde_json::from_str(t) {
                Ok(v) => v,
                Err(e) => {
                    let mut end = t.len().min(1500);
                    while end > 0 && !t.is_char_boundary(end) {
                        end -= 1;
                    }
                    return format!("工具参数不是合法 JSON（{e}）。你发来的原始参数是：\n{}\n请修正成合法 JSON 后重试。", &t[..end]);
                }
            }
        }
    };
    match name {
        "generate_image" => {
            let prompt = args.get("prompt").and_then(|p| p.as_str()).unwrap_or("").trim().to_string();
            if prompt.is_empty() { return "作图失败：没有拿到画面描述".into(); }
            let _ = emit(json!({ "kind": "tool", "name": "generate_image", "phase": "start", "prompt": prompt }));
            let key = match crate::device::device_key_offline() { Ok(k) => k, Err(e) => { let _ = emit(json!({ "kind": "tool", "name": "generate_image", "phase": "error", "message": e })); return format!("作图失败：{e}"); } };
            match crate::providers::generate_image(&key, &prompt, "gpt-image-2", "1024x1024", None) {
                Ok(img) => { let _ = emit(json!({ "kind": "tool", "name": "generate_image", "phase": "result", "b64": img.b64, "url": img.url })); "已生成图片并展示给用户。".into() }
                Err(e) => { let _ = emit(json!({ "kind": "tool", "name": "generate_image", "phase": "error", "message": e })); format!("作图失败：{e}") }
            }
        }
        "generate_video" => {
            let prompt = args.get("prompt").and_then(|p| p.as_str()).unwrap_or("").trim().to_string();
            if prompt.is_empty() { return "做视频失败：没有拿到画面描述".into(); }
            let _ = emit(json!({ "kind": "tool", "name": "generate_video", "phase": "start", "prompt": prompt }));
            // 文生视频默认 Seedance Fast（画质/成本平衡，直连火山、中文好）——治「视频很丑」。
            // 统一走 lib.rs 的动作核心（提交→落 running 记录→轮询下载），跟「AI 视频」页与
            // ActionParity 的 CLI/MCP 入口完全同源；进度每 5s 流给前端（phase:"output"）。
            const VIDEO_MODEL: &str = "doubao-seedance-2-0-fast-260128";
            match crate::run_video_generation(&prompt, Some(VIDEO_MODEL), None, None, &|_id, _phase, progress| {
                let _ = emit(json!({ "kind": "tool", "name": "generate_video", "phase": "output", "chunk": progress }));
            }) {
                Ok(id) => {
                    let path = crate::video::video_file_path(id).map(|p| p.display().to_string()).unwrap_or_default();
                    let _ = emit(json!({ "kind": "tool", "name": "generate_video", "phase": "result", "path": path, "id": id }));
                    "已生成视频并展示在右侧预览。".into()
                }
                Err(e) => { let _ = emit(json!({ "kind": "tool", "name": "generate_video", "phase": "error", "message": e })); format!("做视频失败：{e}") }
            }
        }
        "list_dir" | "read_file" | "write_file" | "edit_file" | "run_command" => {
            let Some(ws) = workspace else { return "请用户先在对话页选一个「工作文件夹」，才能操作文件 / 跑命令。".into(); };
            match name {
                "list_dir" => {
                    let dir = match resolve_in_workspace(ws, args.get("path").and_then(|p| p.as_str()).unwrap_or("")) { Ok(p) => p, Err(e) => return e };
                    let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let _ = emit(json!({ "kind": "tool", "name": "list_dir", "phase": "result", "path": rel }));
                    match std::fs::read_dir(&dir) {
                        Ok(rd) => { let mut n: Vec<String> = rd.flatten().map(|e| { let s = e.file_name().to_string_lossy().to_string(); if e.path().is_dir() { format!("{s}/") } else { s } }).collect(); n.sort(); if n.is_empty() { "（空目录）".into() } else { n.join("\n") } }
                        Err(e) => format!("列目录失败：{e}"),
                    }
                }
                "read_file" => {
                    let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let file = match resolve_in_workspace(ws, rel) { Ok(p) => p, Err(e) => return e };
                    let _ = emit(json!({ "kind": "tool", "name": "read_file", "phase": "result", "path": rel }));
                    match std::fs::read(&file) { Ok(b) => { let t = b.len() > MAX_READ_BYTES; let mut s = String::from_utf8_lossy(&b[..b.len().min(MAX_READ_BYTES)]).to_string(); if t { s.push_str("\n…（文件过大，已截断）"); } s } Err(e) => format!("读文件失败：{e}") }
                }
                "write_file" => {
                    let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    let file = match resolve_in_workspace(ws, rel) { Ok(p) => p, Err(e) => return e };
                    let old = std::fs::read_to_string(&file).ok();
                    let is_new = old.is_none();
                    // 写：ask 模式要审批（带 diff 预览：old/new 让前端出 Codex 式 +/- 视图）；auto/full 自动
                    if mode == "ask" && !ask_approval(task_id, json!({ "tool": "write_file", "path": rel, "is_new": is_new, "old": diff_cap(old.as_deref().unwrap_or("")), "new": diff_cap(&content) }), emit) {
                        let _ = emit(json!({ "kind": "tool", "name": "write_file", "phase": "error", "path": rel, "message": "用户拒绝或超时" }));
                        return "用户拒绝了这次写入。".into();
                    }
                    match atomic_write(&file, content.as_bytes()) {
                        Ok(_) => { let _ = emit(json!({ "kind": "tool", "name": "write_file", "phase": "result", "path": rel, "bytes": content.len(), "is_new": is_new, "old": diff_cap(old.as_deref().unwrap_or("")), "new": diff_cap(&content) })); format!("已写入 {rel}（{} 字节）", content.len()) }
                        Err(e) => { let _ = emit(json!({ "kind": "tool", "name": "write_file", "phase": "error", "path": rel, "message": e.to_string() })); format!("写文件失败：{e}") }
                    }
                }
                "edit_file" => {
                    let rel = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let old_string = args.get("old_string").and_then(|s| s.as_str()).unwrap_or("");
                    let new_string = args.get("new_string").and_then(|s| s.as_str()).unwrap_or("");
                    let replace_all = args.get("replace_all").and_then(|b| b.as_bool()).unwrap_or(false);
                    let file = match resolve_in_workspace(ws, rel) { Ok(p) => p, Err(e) => return e };
                    let old_content = match std::fs::read_to_string(&file) { Ok(c) => c, Err(e) => return format!("读文件失败（edit_file 只能改已有文本文件，新建请用 write_file）：{e}") };
                    if old_string.is_empty() { return "edit_file 需要 old_string（要被替换的原文），不能为空。".into(); }
                    // 唯一性契约（对齐 Codex/grok search_replace）：命中 0 或多都拒，逼模型带准上下文
                    let count = old_content.matches(old_string).count();
                    if count == 0 { return format!("在 {rel} 里没找到要替换的原文。请先 read_file 看准确内容，old_string 需与文件里一字不差（含缩进/换行）。"); }
                    if count > 1 && !replace_all { return format!("old_string 在 {rel} 里匹配到 {count} 处，定位不唯一。请在 old_string 里多带上下文使其唯一，或设 replace_all=true 全部替换。"); }
                    let new_content = if replace_all { old_content.replace(old_string, new_string) } else { old_content.replacen(old_string, new_string, 1) };
                    let replaced = if replace_all { count } else { 1 };
                    // 改：ask 模式要审批（带 diff 预览：发 old_string/new_string 这个改动片段，diff 精准=只显示改的那段，像 Codex）；auto/full 自动
                    if mode == "ask" && !ask_approval(task_id, json!({ "tool": "edit_file", "path": rel, "old": diff_cap(old_string), "new": diff_cap(new_string) }), emit) {
                        let _ = emit(json!({ "kind": "tool", "name": "edit_file", "phase": "error", "path": rel, "message": "用户拒绝或超时" }));
                        return "用户拒绝了这次改动。".into();
                    }
                    match atomic_write(&file, new_content.as_bytes()) {
                        Ok(_) => { let _ = emit(json!({ "kind": "tool", "name": "edit_file", "phase": "result", "path": rel, "replaced": replaced, "old": diff_cap(old_string), "new": diff_cap(new_string) })); format!("已改 {rel}（替换 {replaced} 处）") }
                        Err(e) => { let _ = emit(json!({ "kind": "tool", "name": "edit_file", "phase": "error", "path": rel, "message": e.to_string() })); format!("写文件失败：{e}") }
                    }
                }
                "run_command" => {
                    let command = args.get("command").and_then(|c| c.as_str()).unwrap_or("").trim().to_string();
                    if command.is_empty() { return "没有拿到命令".into(); }
                    if is_dangerous(&command) {
                        let _ = emit(json!({ "kind": "tool", "name": "run_command", "phase": "error", "command": command, "message": "危险命令已被安全策略拦截" }));
                        return "该命令涉及格盘/关机/删系统等危险操作，已被 U-King 安全策略拦截，拒绝执行。".into();
                    }
                    // 审批：full 全自动；auto 只对「明显只读安全」的命令自动跑（像 Codex Auto），其余问；ask 一律问
                    let auto_ok = mode == "full" || (mode == "auto" && is_safe_readonly_command(&command));
                    if !auto_ok && !ask_approval(task_id, json!({ "tool": "run_command", "command": command }), emit) {
                        let _ = emit(json!({ "kind": "tool", "name": "run_command", "phase": "error", "command": command, "message": "用户拒绝或超时" }));
                        return "用户拒绝了这条命令。".into();
                    }
                    let _ = emit(json!({ "kind": "tool", "name": "run_command", "phase": "start", "command": command }));
                    let out = run_shell(&command, ws, task_id, api_key, emit);
                    // 输出已在 run_shell 里边跑边流（phase:"output"），result 只收尾标记完成，不再重复整段
                    let _ = emit(json!({ "kind": "tool", "name": "run_command", "phase": "result", "command": command }));
                    format!("命令输出：\n{out}")
                }
                _ => unreachable!(),
            }
        }
        "uking_action" => exec_uking_action(task_id, &args, mode, headless, emit),
        other => format!("未知工具：{other}"),
    }
}

/// 对话大脑调影核动作。写动作的确认**交给核心判**：第一次不带 confirm 调 → 核心按
/// `confirmation=required` 挡回（confirmation_required）→ 弹审批 → 过审才补 confirm=true 重调。
/// 判据在核心，不在这里抄一份「哪些动作要确认」。全授权（full）下直接放行，同 run_command 的 full；
/// **headless（无头/自动化）下写动作一律拒绝** —— 没人能点批准，绝不能自动放行。
fn exec_uking_action(task_id: &str, args: &Value, mode: &str, headless: bool, emit: &dyn Fn(Value)) -> String {
    let action_id = args
        .get("action_id")
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    // 空 / list / help → 返回活的动作清单（读动作表，不硬编码，动作表会自己长大）。
    if action_id.is_empty() || action_id == "list" || action_id == "help" {
        return uking_action_manifest();
    }
    let mut input = match args.get("input") {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(other) => return format!("uking_action 的 input 必须是 JSON 对象（收到 {other}）"),
    };
    let _ = emit(json!({ "kind": "tool", "name": "uking_action", "phase": "start", "action": action_id }));
    // 带 AI 上下文：动作的归属由上面的 tap 记，不让 actions.rs 把 AI 干的事写成「人干的」。
    let first = crate::actions::with_ai_context(|| crate::actions::run_checked(&action_id, input.clone()));
    match first {
        Ok(v) => uking_action_result(&action_id, v, emit),
        Err(e) if e.code == "confirmation_required" => {
            // 无人值守（无头测试/自动化）：写动作绝不自动执行 —— 没有前端弹窗可点，放行就是替用户做决定。
            if headless {
                let _ = emit(json!({ "kind": "tool", "name": "uking_action", "phase": "error", "action": action_id, "message": "无人值守下写操作不自动执行" }));
                return format!("`{action_id}` 是写操作（会改这台机器），无人值守的对话不允许自动执行。请只调只读动作（清单里标【读】的，如 runtime.stack.inspect / runtime.usage_local.inspect / runtime.rtk.inspect）。");
            }
            let ok = if mode == "full" {
                true
            } else {
                let title = crate::actions::describe(&action_id)
                    .map(|a| a.title)
                    .unwrap_or_else(|_| action_id.clone());
                ask_approval(
                    task_id,
                    json!({
                        "tool": "uking_action",
                        "action": format!("{title}（{action_id}）"),
                        "input_keys": input.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
                    }),
                    emit,
                )
            };
            if !ok {
                let _ = emit(json!({ "kind": "tool", "name": "uking_action", "phase": "error", "action": action_id, "message": "用户拒绝或超时" }));
                return format!("用户拒绝了 {action_id}。");
            }
            input["confirm"] = json!(true);
            match crate::actions::with_ai_context(|| crate::actions::run_checked(&action_id, input)) {
                Ok(v) => uking_action_result(&action_id, v, emit),
                Err(e) => {
                    let _ = emit(json!({ "kind": "tool", "name": "uking_action", "phase": "error", "action": action_id, "message": e.message }));
                    format!("{action_id} 失败：{}", e.message)
                }
            }
        }
        Err(e) => {
            let _ = emit(json!({ "kind": "tool", "name": "uking_action", "phase": "error", "action": action_id, "message": e.message }));
            format!("{action_id} 失败：{}", e.message)
        }
    }
}

/// 动作清单（活读动作表，不硬编码）——模型不确定有哪些动作时用。
fn uking_action_manifest() -> String {
    let specs = crate::actions::list();
    let mut out = format!("U-King 可调动作（{} 个）：\n", specs.len());
    for a in &specs {
        let flag = if a.confirmation == "required" { "【写·需确认】" } else { "【读】" };
        out.push_str(&format!("{flag} {} —— {}\n", a.id, a.description));
    }
    out
}

/// 动作成功结果转成给模型看的文本：字符串直用，JSON 美化，封顶防塞爆模型上下文。
fn uking_action_result(action_id: &str, v: Value, emit: &dyn Fn(Value)) -> String {
    let _ = emit(json!({ "kind": "tool", "name": "uking_action", "phase": "result", "action": action_id }));
    let text = match v {
        Value::String(s) => s,
        other => serde_json::to_string_pretty(&other).unwrap_or_else(|_| "（动作成功，但结果无法序列化）".into()),
    };
    const CAP: usize = 30_000;
    if text.len() <= CAP {
        text
    } else {
        let mut end = CAP;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n…（结果过长，已截断）", &text[..end])
    }
}

#[tauri::command]
pub fn chat_approve(approval_id: String, approved: bool) -> Result<(), String> {
    if let Some(tx) = approvals().lock().ok().and_then(|mut m| m.remove(&approval_id)) { let _ = tx.send(approved); }
    Ok(())
}

#[tauri::command]
pub fn chat_interrupt(task_id: String) -> Result<(), String> {
    // 停流式请求（curl 子进程）
    if let Ok(mut m) = running().lock() { if let Some(mut ch) = m.remove(&task_id) { let _ = ch.kill(); } }
    // 停正在跑的命令（含 claude -p / codex exec 派生的整棵进程树，不留孤儿）
    if let Some(pid) = shell_pids().lock().ok().and_then(|mut m| m.remove(&task_id)) { kill_tree_by_pid(pid); }
    Ok(())
}

/// 无 GUI 地跑完一整轮对话，把助手说的话收成一个 String 返回。
///
/// 给「自动化（定时任务）」和任何**没有人在看屏幕**的调用方用 —— 它们要的是结果正文，
/// 不是一串事件。**不是第二份实现**：真身仍是 `run_chat`，这里只是换一个 emit（收字符串）。
///
/// 两个刻意的取舍：
/// - `approval_mode = full`：没人能点批准，`ask` 会永远卡住。所以**工具面必须靠 workspace 收窄** ——
///   不给工作文件夹时 `tools_spec(false)` 只放行作图/视频（零风险），给了才等于用户授权读写+跑命令。
/// - `task_id` 由调用方给：中断/进程树 kill 靠它定位，两条并发的自动化不能共用一个 id。
pub fn run_headless(
    task_id: &str,
    prompt: &str,
    workspace: Option<String>,
    system: Option<&str>,
    model: &str,
) -> Result<String, String> {
    let key = crate::device::device_key_offline().map_err(|e| format!("拿不到设备 Key: {e}"))?;
    let messages = json!([
        { "role": "system", "content": system.unwrap_or(HEADLESS_SYSTEM) },
        { "role": "user", "content": prompt }
    ]);
    let text = std::sync::Mutex::new(String::new());
    let failure = std::sync::Mutex::new(String::new());
    {
        let emit = |v: Value| match v.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
            "delta" => {
                if let (Ok(mut s), Some(d)) = (text.lock(), v.get("text").and_then(|t| t.as_str())) {
                    s.push_str(d);
                }
            }
            // 工具调用留一行痕迹：不然「它到底画了图没有」只能靠猜
            "tool" => {
                if v.get("phase").and_then(|p| p.as_str()) == Some("done") {
                    if let Ok(mut s) = text.lock() {
                        s.push_str(&format!(
                            "\n[用了工具: {}]\n",
                            v.get("name").and_then(|n| n.as_str()).unwrap_or("?")
                        ));
                    }
                }
            }
            // done 带 status=error 时 run_chat 自己是 Ok 的 —— 不接这一路就会把失败报成成功
            "done" => {
                if v.get("status").and_then(|s| s.as_str()) == Some("error") {
                    if let Ok(mut f) = failure.lock() {
                        *f = v.get("message").and_then(|m| m.as_str()).unwrap_or("对话失败").to_string();
                    }
                }
            }
            _ => {}
        };
        run_chat(
            task_id.into(),
            messages,
            model.into(),
            key,
            None, // 无头跑道：走缺省端点（虾盘云），跟历史行为一致
            workspace,
            Some("full".into()),
            false,
            true,
            &emit,
        )?;
    }
    let err = failure.into_inner().unwrap_or_default();
    if !err.is_empty() {
        return Err(err);
    }
    let out = text.into_inner().unwrap_or_default().trim().to_string();
    if out.is_empty() {
        return Err("大脑没吐出任何内容（可能余额不足或上游抖动）".into());
    }
    Ok(out)
}

/// 无头场景的默认人设：没人在旁边追问，所以要**一次交付完整结果**，别反问。
const HEADLESS_SYSTEM: &str = "你是 U-King 助手，正在**无人值守**地执行一条定时任务：没有人能回答你的追问，\
所以不要反问、不要说「请告诉我…」，直接按你的专业判断一次性把活干完并给出完整成果。\
想画图调 generate_image；想做视频调 generate_video。给了工作文件夹时可用 list_dir/read_file/write_file/edit_file/run_command。\
输出用简体中文，结构清晰、可以直接拿去用。";

/// 无头自测（`U-King.exe --chat-test "<prompt>" [工作文件夹]` / `--brain-actions-test`）：真跑一轮
/// 工具循环，事件精简打印到 stdout。用 full 模式（无 GUI 可审批）。给工作文件夹则放出文件/命令工具。
/// `allow_actions=true` 时挂 uking_action 工具（headless 下只放行读动作，写动作被拒）。
/// 给开发/CI 自己验对话不依赖 GUI。
/// 返回值 = 模型这一轮吐出的正文。
///
/// 🔴 **必须有返回值**：老版本只往 stdout 打印，于是任何"跑一轮看看"的跑道都只能
/// 让人肉眼判读 —— 而肉眼判读的跑道在自动化里恒绿。`--origin-test --live` 正是
/// 靠这个返回值断言「模型确实只凭注入的状态答对了」，注入一坏它就得变红。
pub fn chat_test_headless(prompt: &str, workspace: Option<String>, system: Option<&str>, allow_actions: bool) -> String {
    let key = match crate::device::device_key_offline() {
        Ok(k) => k,
        Err(e) => { println!("[chat-test] 拿不到设备 Key: {e}"); return String::new(); }
    };
    let default_sys = if allow_actions {
        "你是 U-King 助手。要查/操作 U-King 自己（体检/用量/硬件/驱动等）调 uking_action 工具；不确定有哪些动作就把它 action_id 传空字符串列出清单。画图调 generate_image；有工作文件夹时用 list_dir/read_file/write_file/edit_file/run_command。写操作（会改机器的）在本环境不被允许，别尝试。能动手就动手，别只描述。"
    } else {
        "你是 U-King 助手。用户想画图调 generate_image；有工作文件夹时：看用 list_dir/read_file，新建/整体覆盖用 write_file，局部改用 edit_file（查找替换、更省更安全），跑命令用 run_command。能动手就动手，别只描述。"
    };
    let sys = system.unwrap_or(default_sys);
    let messages = json!([
        { "role": "system", "content": sys },
        { "role": "user", "content": prompt }
    ]);
    println!("[chat-test] prompt: {prompt}  workspace: {workspace:?}");
    // 累积模型正文，供调用方断言（见函数头）。闭包借用它，所以要 Mutex。
    let said = std::sync::Mutex::new(String::new());
    let emit = |v: Value| {
        match v.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
            "delta" => {
                let t = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
                print!("{t}");
                let _ = std::io::stdout().flush();
                if let Ok(mut b) = said.lock() { b.push_str(t); }
            }
            "tool" => println!(
                "\n[tool:{} {}] {}",
                v.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                v.get("phase").and_then(|p| p.as_str()).unwrap_or(""),
                v.get("command").or_else(|| v.get("path")).or_else(|| v.get("action")).or_else(|| v.get("prompt")).and_then(|x| x.as_str()).unwrap_or("")
            ),
            "done" => println!(
                "\n[done: {} {}]",
                v.get("status").and_then(|s| s.as_str()).unwrap_or(""),
                v.get("message").and_then(|m| m.as_str()).unwrap_or("")
            ),
            _ => {}
        }
    };
    let _ = run_chat("chat-test".into(), messages, "deepseek-v4-flash".into(), key, None, workspace, Some("full".into()), allow_actions, true, &emit);
    said.into_inner().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 工作台解绑虾盘云之后，端点由调用方给 —— 这里是唯一会**静默打到 404** 的地方，
    /// 所以把四种输入形状钉死。客户在「AI 设置」里填 base 时多写/少写一截 `/chat/completions`
    /// 都很常见，我们不该因为这个把请求打歪了再让他猜为什么不通。
    #[test]
    fn chat_endpoint_falls_back_and_never_double_appends() {
        // 不给 / 给空 / 给一串空白 → 一律回落虾盘云（拿不准就走已知能用的那条）
        assert_eq!(chat_endpoint(None), DEFAULT_ENDPOINT);
        assert_eq!(chat_endpoint(Some("")), DEFAULT_ENDPOINT);
        assert_eq!(chat_endpoint(Some("   ")), DEFAULT_ENDPOINT);
        // 常规 base → 补齐
        assert_eq!(
            chat_endpoint(Some("https://token-plan-cn.xiaomimimo.com/v1")),
            "https://token-plan-cn.xiaomimimo.com/v1/chat/completions"
        );
        // 结尾多个斜杠 → 不该拼出 `//chat/completions`
        assert_eq!(
            chat_endpoint(Some("https://api.deepseek.com/v1/")),
            "https://api.deepseek.com/v1/chat/completions"
        );
        // 客户已经把整条路径填进来了 → 原样用，别再补一截
        assert_eq!(
            chat_endpoint(Some("https://api.deepseek.com/v1/chat/completions")),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    /// #002 乱码用例之一：UTF-8 流在任意切块位置都不许把多字节字符切成 �。
    /// 模拟 4096 分块：对同一段中英混排，在每个可能的字节位置切成两块，
    /// 走真实的 drain_decoded 增量解码，结果必须逐字节等于原文。
    #[test]
    fn chunked_utf8_never_emits_replacement_char() {
        let text = "编译通过 ✓ done：中文 & emoji 🚀 混排 abc";
        let bytes = text.as_bytes();
        for cut in 0..=bytes.len() {
            let mut out = String::new();
            let mut pending: Vec<u8> = Vec::new();
            let mut acp = false;
            for chunk in [&bytes[..cut], &bytes[cut..]] {
                pending.extend_from_slice(chunk);
                out.push_str(&drain_decoded(&mut pending, &mut acp));
            }
            out.push_str(&String::from_utf8_lossy(&pending));
            assert!(!acp, "合法 UTF-8 不该触发 ACP 模式（切在 {cut}）");
            assert_eq!(out, text, "切在字节 {cut} 处出了乱码");
            assert!(!out.contains('\u{FFFD}'));
        }
    }

    /// GBK（ACP 双字节）流同理：任意切块位置的增量解码，必须和整段一次解出的结果一致
    /// ——对比对象是 ansi_to_string(全量)，所以断言不依赖测试机的具体代码页。
    #[test]
    fn chunked_ansi_matches_whole_decode() {
        // 「中文测试OK」的 GBK 字节（0xD6D0 CEC4 B2E2 CAD4 + ASCII）
        let bytes: &[u8] = &[0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4, 0x4F, 0x4B];
        let whole = ansi_to_string(bytes);
        for cut in 0..=bytes.len() {
            let mut out = String::new();
            let mut pending: Vec<u8> = Vec::new();
            let mut acp = false;
            for chunk in [&bytes[..cut], &bytes[cut..]] {
                pending.extend_from_slice(chunk);
                out.push_str(&drain_decoded(&mut pending, &mut acp));
            }
            if !pending.is_empty() { out.push_str(&ansi_to_string(&pending)); }
            assert_eq!(out, whole, "切在字节 {cut} 处和整段解码不一致");
        }
    }

    /// #002 乱码用例之二（仅 Windows）：真跑一条会输出中文的命令（echo 是 cmd 内建，
    /// 管道输出走 GBK，chcp 都管不着）—— 这条用例在旧代码上必红。
    #[test]
    #[cfg(windows)]
    fn run_shell_chinese_output_not_mojibake() {
        let out = run_shell("echo 中文测试OK", ".", "utf8-test", "", &|_| {});
        assert!(out.contains("中文测试OK"), "输出被解成了乱码：{out}");
        assert!(!out.contains('\u{FFFD}'), "输出含 �：{out}");
    }

    /// uking_action 空 action_id → 返回活的动作清单（读动作表，不硬编码 —— 动作表自己会长大）。
    #[test]
    fn uking_action_empty_lists_manifest() {
        let out = exec_uking_action("t", &json!({"action_id": ""}), "full", false, &|_| {});
        assert!(out.contains("U-King 可调动作"), "清单头没出来：{out}");
        assert!(out.contains("runtime.command_guard.inspect"), "清单里没有真实动作 id：{out}");
        assert!(out.contains("【读】"), "清单没标读/写：{out}");
    }

    /// uking_action 打错动作 id → 核心返回 unknown_action，原样透传给模型（不静默吞错）。
    #[test]
    fn uking_action_unknown_action_is_loud() {
        let out = exec_uking_action("t", &json!({"action_id": "runtime.nope"}), "full", false, &|_| {});
        assert!(out.contains("unknown_action"), "应把 unknown_action 透传给模型：{out}");
    }

    /// uking_action 调只读动作 → 真跑核心、emit phase=result 事件（时间轴靠它记）、把结果回给模型。
    #[test]
    fn uking_action_read_runs_and_emits_result() {
        let events = std::sync::Mutex::new(Vec::new());
        let emit = |v: Value| { events.lock().unwrap().push(v); };
        let out = exec_uking_action("t", &json!({"action_id": "runtime.command_guard.inspect"}), "full", false, &emit);
        assert!(!out.is_empty(), "读动作应有返回：{out}");
        let has_result = events.lock().unwrap().iter().any(|e| e["phase"] == "result" && e["action"] == "runtime.command_guard.inspect");
        assert!(has_result, "应 emit phase=result 事件");
    }

    /// 写动作的门禁契约：核心不带 confirm 必须挡回 confirmation_required —— 这是
    /// exec_uking_action 里 `e.code == "confirmation_required"` 分支所依赖的判据。
    /// （handler 根本不会执行，所以动态挑一个写动作测也不改机器。）
    #[test]
    fn uking_action_write_gate_rejects_without_confirm() {
        let write = crate::actions::list()
            .into_iter()
            .find(|a| a.confirmation == "required")
            .expect("动作表里应有 confirmation=required 的写动作");
        let e = crate::actions::run_checked(&write.id, json!({})).unwrap_err();
        assert_eq!(e.code, "confirmation_required", "{} 不带 confirm 必须被门禁挡回", write.id);
    }

    /// headless 下写动作绝不自动执行：即使 mode=full 也不放行 —— 没有前端弹窗可点，
    /// 放行就是替用户做决定。core 门禁挡回后直接返回「无人值守」提示，handler 不会执行。
    #[test]
    fn uking_action_write_rejected_headless() {
        let write = crate::actions::list()
            .into_iter()
            .find(|a| a.confirmation == "required")
            .expect("动作表里应有写动作");
        let events = std::sync::Mutex::new(Vec::new());
        let emit = |v: Value| { events.lock().unwrap().push(v); };
        let out = exec_uking_action("t", &json!({"action_id": write.id.clone()}), "full", true, &emit);
        assert!(out.contains("无人值守"), "headless 写动作应被拒：{out}");
        let has_error = events.lock().unwrap().iter().any(|e| e["phase"] == "error");
        assert!(has_error, "应 emit phase=error 事件");
    }
}
