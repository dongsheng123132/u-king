//! 本机全部 AI 的任务（`runtime.ai_tasks.inspect`）—— 「这台电脑上，各家 AI 都在干什么、干过什么」。
//!
//! ## 为什么单开一个模块
//! 任务看板原来只看得见 **U-King 自己工作台里的会话**。可客户机上真正在干活的 AI 远不止这一个
//! 入口：他在另一个终端里开着 Claude Code、在编辑器里挂着 Codex、Hermes 也有自己的会话记录。
//! 「我这台电脑上的 AI 到底都在忙什么」这个问题，55 个动作里一个都答不了 —— 因为我们只统计过
//! **花了多少钱**（`usage_local`），从没统计过**在干哪些活**。
//!
//! ## 数据来源（都是各家工具**自己**写的记录，我们只读不写、不联网）
//! - **Claude Code**：`~/.claude/projects/**/*.jsonl`。首条 `type:"user"` 且 `origin.kind=="human"`
//!   的消息就是这次任务的标题，`cwd` 是工作目录。
//! - **Codex CLI**：`~/.codex/sessions/**/rollout-*.jsonl`。开头 `session_meta` 给 cwd/模型，
//!   标题取首条 `event_msg`/`user_message`（**不能**取 `response_item` 里的 user 消息 ——
//!   那里前几条是 Codex 自己注入的 `<recommended_plugins>` / AGENTS.md，拿它当标题全是同一句话）。
//! - **Hermes**：`~/.hermes/sessions/*.json` + `~/.hermes/profiles/*/sessions/*.json`（整份 JSON，
//!   自带 `messages`）。`HERMES_HOME` 和 `~/.hermes` **两个都扫**，理由见 `hermes_sessions_dirs`。
//! - **OpenClaw / ClawX**：`<home>/agents/<agent>/sessions/<uuid>.jsonl`。首行 `type:"session"`
//!   给 id/cwd，`model_change` 给模型，首条 user `message` 是标题。**两个 home 都扫**
//!   （`~/.openclaw` 和 U-King 便携的 `~/.uking/openclaw`）。
//!   ⚠️ ClawX **没有自己的会话库** —— `%APPDATA%\ClawX` 只是 Electron 外壳，对话落在上面这些目录。
//! - **AI 任务看板**（`~/.uking/board/board.json`）：`uking-board` 技能包让 AI 自己写的任务进度。
//!   它是唯一**自带声明状态**（todo/doing/done/blocked）的来源 —— 那是 AI 自己说的，如实标注。
//!
//! ## 状态是怎么判的（别编）
//! 外部工具的会话记录里**没有**「这次任务成没成」这种字段。所以只用一个能证明的事实：
//! **最近 `ACTIVE_SECS` 秒内这个文件还在被写 → 还在跑**，否则算已结束。
//! 每条都带 `status_from` 说明依据（`mtime` = 我们从文件活跃度推的，`declared` = AI 自己写的）。
//! **绝不给外部会话安一个「出错」状态** —— 我们没有判据，猜一个错的比不给更糟。
//!
//! ## 开销
//! 全程只读本地文件：先按 mtime 粗筛掉窗口外的，再对每个文件**最多**读 `HEAD_LINES` 行 /
//! `HEAD_BYTES` 字节，拿到标题就提前收工（Codex 单个会话文件实测 6MB，全读是白烧）。
//! 每个来源最多扫 `MAX_FILES_PER_TOOL` 个文件，超了如实报 `truncated`。

use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 每个来源最多扫多少个会话文件（按 mtime 由新到旧取）。
const MAX_FILES_PER_TOOL: usize = 200;
/// 单个会话文件最多读多少行 / 多少字节找标题。
const HEAD_LINES: usize = 3_000;
const HEAD_BYTES: u64 = 2 * 1024 * 1024;
/// 从文件**末尾**回读多少字节找 Claude 自己写的会话标题（见 `claude_ai_title`）。
/// 本机实测 83 个会话的末条 `ai-title` 100% 落在最后 128KB 内。
const TAIL_BYTES: u64 = 128 * 1024;
/// 标题截断长度（字符，不是字节 —— 中文按字符切才不会切出半个字）。
const MAX_TITLE_CHARS: usize = 100;
/// 多久没写就不算「还在跑」。
const ACTIVE_SECS: u64 = 300;
/// AI 自己声明的 `doing` / `blocked` 多久没更新就不能再当**现在**用。
///
/// 「进行中」是一个关于**此刻**的断言。board.json 里一条三天前写下的 `doing`
/// 只能证明「AI 三天前说它在做」——之后没人改回去，因为改回去要 AI 自己想起来。
/// 本机实测：16 条 `doing` 里 8 条的 `updated` 是 **12 天前**，它们把看板「进行中」
/// 那一列 94% 填成了陈旧声明，于是用户真正在跑的那 1 条被埋在里面。
/// 超期的降到 `idle`（我们确实不知道它在不在跑），`status_from` 标 `declared_stale`
/// 并在 `note` 里写明原委 —— **降级要留痕**，不然看着像任务丢了。
const DECLARED_FRESH_SECS: i64 = 2 * 24 * 3600;
/// 各来源的递归深度上限。**这个数字是有讲究的，别图省事统一放大**：
/// Claude Code 在 `<项目>/<会话 id>/subagents/agent-*.jsonl` 里还存着**子代理**的逐字记录，
/// 它们带的是**父会话的 sessionId** —— 一并收进来的话，同一个 id 会在看板上冒出好几张卡
/// （React key 撞车），而且子代理是父任务的内部机械，本来就不是「用户的一个活」。
const CLAUDE_DEPTH: usize = 1; // projects/<项目>/*.jsonl，再往里就是 subagents/
const CODEX_DEPTH: usize = 3; // sessions/年/月/日/rollout-*.jsonl
const HERMES_DEPTH: usize = 1;
const OPENCLAW_DEPTH: usize = 1; // 传进去的已经是 <home>/agents/<agent>/sessions 本身

/// 一条 AI 任务（= 一次会话，或看板上 AI 自己登记的一个活）。
#[derive(Serialize)]
pub struct AiTask {
    /// 稳定 id：`<tool>:<session_id>`。同一次会话每次扫描都是同一个 id。
    pub id: String,
    /// **那家工具自己认的会话 id**（不带 `<tool>:` 前缀）。
    /// 单列出来是因为「接着干」要拿它去拼 `--resume`，让前端从 `id` 里切前缀
    /// 等于把 id 的格式变成一份没写下来的契约 —— 哪天前缀变了，切出来的是半个 uuid，
    /// 而 `claude --resume <半个 uuid>` 会**开一个新会话**，界面上还长得像续上了。
    pub session_id: String,
    /// `claude` / `codex` / `hermes` / `board`。
    pub tool: String,
    pub tool_label: String,
    /// 这次任务在干什么（首条真人消息 / 看板标题）。拿不到就退回会话 id。
    pub title: String,
    /// **在这台机器上怎么接着干这一件事** —— 那家工具自己的续接命令，
    /// 原样可粘进终端跑（`claude --resume <sid>` / `codex resume <sid>`）。
    ///
    /// 🔴 空串 = **这家工具没有我们能保证跑得通的续接写法**（Hermes / OpenClaw 的
    /// 续接方式各版本不一，看板任务压根不是会话）。宁可空着让用户自己开，
    /// 也不许拼一条「看着像」的命令 —— 用户按下去开出一个新会话、上下文全丢，
    /// 而界面上跟真的续上了长得一模一样。这跟 `codex.rs` 里「不教半对的写法」同一条纪律。
    pub resume_cmd: String,
    /// 工作目录**真实路径**（前端点卡片要用它开会话）。本动作只在本机返回、不上传。
    pub dir: String,
    /// 目录名（看板卡片副标题用）。
    pub project: String,
    pub model: String,
    /// `running` | `idle` | `waiting_input` | `done`。**不产出 `error`** —— 见模块头。
    pub status: String,
    /// 状态从哪来：`mtime`（我们从文件活跃度推的）/ `declared`（AI 自己写的）/
    /// `declared_stale`（AI 写过，但那句话太老已经不能当「现在」用了 —— 见 `DECLARED_FRESH_SECS`）。
    pub status_from: String,
    /// 首条消息时间（epoch ms）。取不到为 0。
    pub started_at: i64,
    /// 最后活动时间（epoch ms）= 会话文件 mtime。
    pub updated_at: i64,
    /// 补充说明（看板任务的进度/卡点等）。没有就是空串。
    pub note: String,
}

/// 一个来源扫出来什么。**样本量必须报** —— 「没扫到」和「没有」是两回事。
#[derive(Serialize)]
pub struct AiTaskSource {
    pub tool: String,
    pub label: String,
    /// 记录所在目录（给人排障用）。
    pub path: String,
    /// 目录/文件在不在。
    pub present: bool,
    /// 我们**读得动**吗。读不动就 `false` + 在 `note` 里写明原因 ——
    /// 「没读到」和「没有」必须能分开，否则一个 0 会被当成「这台机器上没有」。
    pub readable: bool,
    /// 窗口内看到的文件数。
    pub files_in_window: usize,
    /// 真正扫了几个（被 `MAX_FILES_PER_TOOL` 截断时小于上一项）。
    pub files_scanned: usize,
    /// 从这个来源实际得到几条。**样本量必须报** —— 影核观测记账约定
    /// （docs/PROPOSAL-OBSERVATION-ACCOUNTING §5.2）。0 时 `note` 必须非空。
    pub count: usize,
    pub note: String,
}

#[derive(Serialize, Default)]
pub struct AiTaskCounts {
    pub running: usize,
    pub idle: usize,
    pub waiting_input: usize,
    pub done: usize,
    pub total: usize,
}

#[derive(Serialize)]
pub struct AiTasksReport {
    pub generated_at: i64,
    pub days: i64,
    /// 多久没写就不算「还在跑」（秒）。前端拿它解释状态是怎么来的。
    pub active_window_secs: u64,
    pub tasks: Vec<AiTask>,
    pub sources: Vec<AiTaskSource>,
    pub counts: AiTaskCounts,
    /// 有来源被 `MAX_FILES_PER_TOOL` 截断。截断了就得说，不能装作全看过。
    pub truncated: bool,
    /// 诚实说明（口径、读不动的来源等）。
    pub notes: Vec<String>,
}

// ———————————————— 家目录 / 各家记录目录 ————————————————

/// 家目录（认 `UKING_TEST_HOME` 沙箱，与 `usage_local` / `journal` 同口径）。
fn home_dir() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
}

fn claude_projects_dir() -> PathBuf {
    home_dir().join(".claude").join("projects")
}

fn codex_sessions_dir() -> PathBuf {
    home_dir().join(".codex").join("sessions")
}

/// Hermes 家目录：沙箱 → `HERMES_HOME` → `~/.hermes`，**两个都扫**。
///
/// 🔴 **别拿 `%LOCALAPPDATA%\hermes` 当配置/记录目录** —— 那是它的**安装**目录
/// （pc-*** 全线 404 就是这么来的）。
///
/// ⚠️ 「判别家工具的家目录只认它自己的解析顺序」那条规矩说的是**往哪写配置**：
/// 写进它不读的目录 = 白写。**读历史记录是另一回事** —— 这块板要回答的是「这台电脑上
/// 干过哪些活」，`HERMES_HOME` 被改过的机器上，改之前的会话还留在 `~/.hermes` 里，
/// 只认当前那一个就会让它们凭空消失。所以两边都扫、去重。
/// （本机实测 `HERMES_HOME` 持久化在 HKCU\Environment 指向一个对比实验目录，
/// 只认它的话 `~/.hermes` 里的会话一条都看不到。）
fn hermes_sessions_dirs() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            roots.push(PathBuf::from(t).join(".hermes"));
        }
    }
    if roots.is_empty() {
        if let Ok(h) = std::env::var("HERMES_HOME") {
            if !h.is_empty() {
                roots.push(PathBuf::from(h));
            }
        }
        roots.push(home_dir().join(".hermes"));
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    let push = |d: PathBuf, dirs: &mut Vec<PathBuf>| {
        if d.is_dir() && !dirs.contains(&d) {
            dirs.push(d);
        }
    };
    for root in roots {
        // 顶层 sessions/ + 每个 profile 各自的 sessions/。
        push(root.join("sessions"), &mut dirs);
        if let Ok(rd) = std::fs::read_dir(root.join("profiles")) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    push(e.path().join("sessions"), &mut dirs);
                }
            }
        }
    }
    dirs
}

fn board_file() -> PathBuf {
    home_dir().join(".uking").join("board").join("board.json")
}

/// OpenClaw / ClawX 的会话目录：`<home>/agents/<agent>/sessions/*.jsonl`。
///
/// 🔴 **这台机器上有两个 home，都得扫**：
///  - `~/.openclaw` —— 官方默认，客户自己装的 OpenClaw 和 **ClawX 桌面版**都写这儿；
///  - `~/.uking/openclaw` —— U-King 给内置终端注入的便携 home（`term.rs::openclaw_home()`，
///    照抄它的口径，别另写一份）。只扫其一，那半边的会话就凭空消失。
/// `OPENCLAW_HOME` 若显式设了，也得认（跟着人家自己的解析顺序走）。
///
/// **ClawX 没有自己的会话库**：`%APPDATA%\ClawX` 是 Electron 外壳（缓存/localStorage/
/// clawx-providers.json），对话是它驱动 gateway 产生的，落点就是下面这些目录。
fn openclaw_session_dirs() -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = Vec::new();
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            homes.push(PathBuf::from(&t).join(".openclaw"));
            homes.push(PathBuf::from(&t).join(".uking").join("openclaw"));
        }
    }
    if homes.is_empty() {
        if let Ok(h) = std::env::var("OPENCLAW_HOME") {
            if !h.is_empty() {
                homes.push(PathBuf::from(h));
            }
        }
        homes.push(home_dir().join(".openclaw"));
        homes.push(home_dir().join(".uking").join("openclaw"));
    }
    let mut dirs = Vec::new();
    for h in homes {
        // 同一个 home 可能从两条路进来（OPENCLAW_HOME 正好指着默认位置），去重。
        let agents = h.join("agents");
        let Ok(rd) = std::fs::read_dir(&agents) else { continue };
        for e in rd.flatten() {
            if e.path().is_dir() {
                let s = e.path().join("sessions");
                if s.is_dir() && !dirs.contains(&s) {
                    dirs.push(s);
                }
            }
        }
    }
    dirs
}

// ———————————————— 小工具 ————————————————

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn mtime_ms(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 按字符截断（中文安全）并把换行压成空格 —— 标题是一行，不能把多行正文塞进卡片。
fn clip_title(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let t = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= MAX_TITLE_CHARS {
        return t;
    }
    let head: String = t.chars().take(MAX_TITLE_CHARS).collect();
    format!("{head}…")
}

/// UTC ISO8601（`2026-08-08T02:12:04.129Z`）→ epoch 毫秒。取不到给 0。
///
/// 自己算是因为项目没有 chrono（体积优先）。只认 UTC —— Claude / Codex 写的就是 `Z` 结尾，
/// 不做本地时区推断（推错了比不给更坏）。
fn iso_utc_ms(s: &str) -> i64 {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return 0;
    }
    let num = |from: usize, to: usize| -> i64 { s[from..to].parse::<i64>().unwrap_or(-1) };
    let (y, mo, d) = (num(0, 4), num(5, 7), num(8, 10));
    let (h, mi, sec) = (num(11, 13), num(14, 16), num(17, 19));
    if y < 1970 || !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h < 0 || mi < 0 || sec < 0 {
        return 0;
    }
    // days_from_civil（Howard Hinnant 的公历算法），闰年/世纪全对。
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms_frac = if b.len() > 20 && b[19] == b'.' {
        s[20..].chars().take(3).collect::<String>().parse::<i64>().unwrap_or(0)
    } else {
        0
    };
    ((days * 86_400 + h * 3_600 + mi * 60 + sec) * 1000) + ms_frac
}

/// 一个来源可能横跨多个目录（Hermes 的两个 home、OpenClaw 的两个 home + 多 agent）。
/// 只报第一个会让人以为「就扫了这一个」—— 多的时候把还有几个也说出来。
fn describe_dirs(dirs: &[PathBuf]) -> String {
    match dirs.len() {
        0 => String::new(),
        1 => dirs[0].to_string_lossy().into_owned(),
        n => format!("{}（另有 {} 个目录）", dirs[0].to_string_lossy(), n - 1),
    }
}

/// 目录名（跨平台按两种分隔符切 —— Claude 记的 cwd 在 Windows 上是反斜杠）。
fn basename(dir: &str) -> String {
    dir.rsplit(|c| c == '/' || c == '\\')
        .find(|s| !s.is_empty())
        .unwrap_or(dir)
        .to_string()
}

/// 递归收集符合条件的文件及其 mtime。深度和数量都有界。
fn collect_files(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    keep: &dyn Fn(&str) -> bool,
    out: &mut Vec<(PathBuf, SystemTime)>,
) {
    if depth > max_depth || out.len() > MAX_FILES_PER_TOOL * 20 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            collect_files(&path, depth + 1, max_depth, keep, out);
        } else if keep(&e.file_name().to_string_lossy()) {
            if let Ok(t) = md.modified() {
                out.push((path, t));
            }
        }
    }
}

/// 粗筛（窗口内）+ 由新到旧排序 + 截断。返回 (取用的文件, 窗口内总数, 是否被截断)。
fn recent_files(
    dirs: &[PathBuf],
    days: i64,
    max_depth: usize,
    keep: &dyn Fn(&str) -> bool,
) -> (Vec<(PathBuf, i64)>, usize, bool) {
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(days.max(1) as u64 * 86_400));
    let mut all = Vec::new();
    for d in dirs {
        collect_files(d, 0, max_depth, keep, &mut all);
    }
    let mut in_window: Vec<(PathBuf, i64)> = all
        .into_iter()
        .filter(|(_, t)| cutoff.map(|c| *t >= c).unwrap_or(true))
        .map(|(p, t)| {
            let ms = t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
            (p, ms)
        })
        .collect();
    in_window.sort_by(|a, b| b.1.cmp(&a.1));
    let total = in_window.len();
    let truncated = total > MAX_FILES_PER_TOOL;
    in_window.truncate(MAX_FILES_PER_TOOL);
    (in_window, total, truncated)
}

/// 逐行读一个会话文件，交给 `f` 判断；`f` 返回 true 表示「够了，收工」。
/// 行数和字节数双重设限 —— Codex 单个会话实测 6MB，全读纯属白烧。
fn scan_head(path: &Path, mut f: impl FnMut(&str) -> bool) {
    let Ok(file) = std::fs::File::open(path) else { return };
    let mut r = BufReader::new(file);
    let mut bytes = 0u64;
    let mut line = String::new();
    for _ in 0..HEAD_LINES {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(n) => bytes += n as u64,
        }
        if f(&line) {
            return;
        }
        if bytes >= HEAD_BYTES {
            return;
        }
    }
}

/// 读一个文件的**末尾** `TAIL_BYTES` 字节，按行切开返回（丢掉开头那半行）。
///
/// 为什么要有这个：Claude Code 把它自己的会话标题（`type:"ai-title"`）**边聊边改**，
/// 一次会话里写十几条，最新那条在文件尾。`scan_head` 从头读、还带提前收工，
/// 结构性地够不着它。而尾巴是定长的，每个文件多一次 seek + 一次小读，
/// 不动「先按 mtime 粗筛、每个文件只读一点」的开销约定。
fn scan_tail(path: &Path) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let Ok(len) = f.metadata().map(|m| m.len()) else { return Vec::new() };
    let from = len.saturating_sub(TAIL_BYTES);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.take(TAIL_BYTES).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    // 从中间切进去多半会把一个多字节字符劈开，`from_utf8_lossy` 顶多在**第一行**留个 �，
    // 而第一行本来就要丢（它是半行）。
    let text = String::from_utf8_lossy(&buf).into_owned();
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines
}

/// Claude Code 自己给这次会话起的标题 —— **`claude --resume` 列表里显示的就是它**。
///
/// 🔴 这是「看板跟 `claude --resume` 对不上」的正主。以前看板拿的是**首条真人消息**，
/// 而用户在 `claude --resume` 里看到的是这条 AI 标题，两边天生不是一个东西。
/// 更糟的是首条消息经常根本不是话：粘贴的文件路径、`[Image: original 1440x2020…]`、
/// 终端回显的一坨框线 —— 本机实测 125 条会话里这类占了一大把，
/// 于是「没记录我干的任务」的观感是准确的：**记了，但记的那一栏认不出来。**
///
/// 取**最后一条**（标题随着会话推进被改写，最后一条才是这次活的最终理解）。
/// 本机实测 83 个有标题的会话，末条 100% 落在最后 `TAIL_BYTES` 内。
/// 拿不到就返回 None，由调用方退回原来的首条真人消息 —— 少一档信息，不是错一档。
fn claude_ai_title(path: &Path) -> Option<String> {
    let mut found: Option<String> = None;
    for line in scan_tail(path) {
        if !line.contains("\"ai-title\"") {
            continue;
        }
        if let Ok(j) = serde_json::from_str::<Value>(&line) {
            if j.get("type").and_then(|v| v.as_str()) != Some("ai-title") {
                continue;
            }
            if let Some(t) = j.get("aiTitle").and_then(|v| v.as_str()) {
                let t = t.trim();
                if !t.is_empty() {
                    found = Some(clip_title(t));
                }
            }
        }
    }
    found
}

/// 由「多久没写」判活。外部工具不写「这次成没成」，所以只报能证明的那一件事。
fn status_by_mtime(updated_at: i64, now: i64) -> &'static str {
    if now.saturating_sub(updated_at) <= (ACTIVE_SECS as i64) * 1000 {
        "running"
    } else {
        "done"
    }
}

// ———————————————— Claude Code ————————————————

fn parse_claude(path: &Path, updated_at: i64, now: i64) -> Option<AiTask> {
    let mut title = String::new();
    // 退而求其次的标题：不是人当场敲的，但确实是这次会话的第一条指令。
    // **定时/无人值守跑起来的会话就长这样**（`isMeta:true`、没有 origin）——
    // 而那恰恰是最该出现在看板上的一类活：人不在场，不看板就永远不知道它跑过。
    let mut fallback = String::new();
    let mut dir = String::new();
    let mut model = String::new();
    let mut session = String::new();
    let mut started_at = 0i64;

    scan_head(path, |line| {
        // 便宜的预筛：这几个字段名不出现就绝无可能命中。
        if !line.contains("\"type\"") {
            return false;
        }
        let Ok(j) = serde_json::from_str::<Value>(line) else { return false };
        if session.is_empty() {
            if let Some(s) = j.get("sessionId").and_then(|v| v.as_str()) {
                session = s.to_string();
            }
        }
        if dir.is_empty() {
            if let Some(c) = j.get("cwd").and_then(|v| v.as_str()) {
                dir = c.to_string();
            }
        }
        let ty = j.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if started_at == 0 {
            if let Some(ts) = j.get("timestamp").and_then(|v| v.as_str()) {
                started_at = iso_utc_ms(ts);
            }
        }
        if ty == "assistant" && model.is_empty() {
            if let Some(m) = j.get("message").and_then(|m| m.get("model")).and_then(|v| v.as_str()) {
                if m != "<synthetic>" {
                    model = m.to_string();
                }
            }
        }
        if ty == "user" && title.is_empty() {
            // `origin.kind=="human"` 是 Claude Code 给「人当场敲的」打的标记，最准 —— 优先用它。
            // 没有这个标记时不能直接丢：定时任务/`-p` 无头跑起来的会话第一条就是 `isMeta:true`，
            // 那条正文就是这次的活。所以留一个 fallback，扫完还没等到 human 的就用它。
            // 两种情况都要挡掉 `<...>` 开头的（工具结果、命令回显、hook 注入的系统块）。
            let human = j.get("origin").and_then(|o| o.get("kind")).and_then(|v| v.as_str()) == Some("human");
            if let Some(c) = j.get("message").and_then(|m| m.get("content")).and_then(|v| v.as_str()) {
                let c = c.trim();
                if !c.is_empty() && !c.starts_with('<') {
                    if human {
                        title = clip_title(c);
                    } else if fallback.is_empty() {
                        fallback = clip_title(c);
                    }
                }
            }
        }
        !title.is_empty() && !dir.is_empty() && !model.is_empty()
    });
    // ★ 标题优先级：Claude 自己写的 `ai-title`（= `claude --resume` 列表里显示的那个）
    //   → 首条真人消息 → 首条非人消息 → 会话 id。
    //   前两者不是「哪个更好看」的问题，是**跟用户在别处看到的同一件事对不对得上**。
    if let Some(t) = claude_ai_title(path) {
        title = t;
    }
    if title.is_empty() {
        title = fallback;
    }

    if session.is_empty() {
        session = path.file_stem().map(|s| s.to_string_lossy().into_owned())?;
    }
    Some(AiTask {
        id: format!("claude:{session}"),
        resume_cmd: format!("claude --resume {session}"),
        session_id: session.clone(),
        tool: "claude".into(),
        tool_label: "Claude Code".into(),
        title: if title.is_empty() { format!("会话 {}", short_id(&session)) } else { title },
        project: basename(&dir),
        dir,
        model,
        status: status_by_mtime(updated_at, now).into(),
        status_from: "mtime".into(),
        started_at,
        updated_at,
        note: String::new(),
    })
}

// ———————————————— Codex CLI ————————————————

fn parse_codex(path: &Path, updated_at: i64, now: i64) -> Option<AiTask> {
    let mut title = String::new();
    let mut dir = String::new();
    let mut model = String::new();
    let mut session = String::new();
    let mut started_at = 0i64;

    scan_head(path, |line| {
        if !line.contains("\"payload\"") {
            return false;
        }
        let Ok(j) = serde_json::from_str::<Value>(line) else { return false };
        let Some(p) = j.get("payload") else { return false };
        let pt = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if pt == "session_meta" {
            if let Some(s) = p.get("session_id").and_then(|v| v.as_str()) {
                session = s.to_string();
            }
            if let Some(c) = p.get("cwd").and_then(|v| v.as_str()) {
                dir = c.to_string();
            }
            if let Some(ts) = p.get("timestamp").and_then(|v| v.as_str()) {
                started_at = iso_utc_ms(ts);
            }
        }
        if model.is_empty() {
            if let Some(m) = p.get("model").and_then(|v| v.as_str()) {
                model = m.to_string();
            }
        }
        // ★ 标题只认 `event_msg`/`user_message` —— `response_item` 里的 user 消息前几条是
        // Codex 自己注入的 `<recommended_plugins>` / AGENTS.md 正文，拿它当标题全是同一句话。
        if pt == "user_message" && title.is_empty() {
            if let Some(m) = p.get("message").and_then(|v| v.as_str()) {
                title = clip_title(unwrap_delegation(m.trim()));
            }
        }
        !title.is_empty() && !dir.is_empty() && !model.is_empty()
    });

    if session.is_empty() {
        session = path.file_stem().map(|s| s.to_string_lossy().into_owned())?;
    }
    Some(AiTask {
        id: format!("codex:{session}"),
        // `codex resume <session_id>` 是 codex exec 的续接写法（同 agent/codex.rs 用的那条）。
        resume_cmd: format!("codex resume {session}"),
        session_id: session.clone(),
        tool: "codex".into(),
        tool_label: "Codex CLI".into(),
        title: if title.is_empty() { format!("会话 {}", short_id(&session)) } else { title },
        project: basename(&dir),
        dir,
        model,
        status: status_by_mtime(updated_at, now).into(),
        status_from: "mtime".into(),
        started_at,
        updated_at,
        note: String::new(),
    })
}

/// Codex 的子线程任务外面裹一层 `<codex_delegation><input>真正的活</input>…`。
/// 不剥开的话看板上一排卡片全叫 `<codex_delegation>`。
fn unwrap_delegation(m: &str) -> &str {
    if let Some(rest) = m.split_once("<input>") {
        if let Some((inner, _)) = rest.1.split_once("</input>") {
            return inner.trim();
        }
    }
    m
}

/// 会话 id 太长，退回显示时只留头 8 位。
fn short_id(s: &str) -> String {
    s.chars().take(8).collect()
}

// ———————————————— OpenClaw / ClawX ————————————————

/// 一条 OpenClaw / ClawX 会话。格式（`<home>/agents/<agent>/sessions/<uuid>.jsonl`）：
/// 首行 `{"type":"session","id","cwd","timestamp"}`，随后 `model_change` 给 provider/modelId，
/// 再往后 `{"type":"message","message":{"role":"user","content":…}}` 就是人说的第一句。
/// `content` 可能是字符串，也可能是 `[{type:"text",text:…}]` 数组（带附件的那种）。
fn parse_openclaw(path: &Path, updated_at: i64, now: i64) -> Option<AiTask> {
    let mut title = String::new();
    let mut dir = String::new();
    let mut model = String::new();
    let mut session = String::new();
    let mut started_at = 0i64;

    scan_head(path, |line| {
        if !line.contains("\"type\"") {
            return false;
        }
        let Ok(j) = serde_json::from_str::<Value>(line) else { return false };
        match j.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "session" => {
                if let Some(s) = j.get("id").and_then(|v| v.as_str()) {
                    session = s.to_string();
                }
                if let Some(c) = j.get("cwd").and_then(|v| v.as_str()) {
                    dir = c.to_string();
                }
                if let Some(ts) = j.get("timestamp").and_then(|v| v.as_str()) {
                    started_at = iso_utc_ms(ts);
                }
            }
            "model_change" => {
                if let Some(m) = j.get("modelId").and_then(|v| v.as_str()) {
                    model = m.to_string();
                }
            }
            "message" if title.is_empty() => {
                let Some(msg) = j.get("message") else { return false };
                if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                    return false;
                }
                let text = match msg.get("content") {
                    Some(Value::String(s)) => s.trim().to_string(),
                    Some(Value::Array(a)) => a
                        .iter()
                        .find_map(|b| b.get("text").and_then(|v| v.as_str()))
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    _ => String::new(),
                };
                // 跟另外三家一样挡掉 `<…>` 开头的系统注入块。
                if !text.is_empty() && !text.starts_with('<') {
                    title = clip_title(&text);
                }
            }
            _ => {}
        }
        !title.is_empty() && !dir.is_empty() && !model.is_empty()
    });

    if session.is_empty() {
        session = path.file_stem().map(|s| s.to_string_lossy().into_owned())?;
    }
    Some(AiTask {
        id: format!("openclaw:{session}"),
        // OpenClaw 的续接写法跟着 gateway / agent 走，各版本不一 —— 不编一条。见字段注释。
        resume_cmd: String::new(),
        session_id: session.clone(),
        tool: "openclaw".into(),
        tool_label: "OpenClaw / ClawX".into(),
        title: if title.is_empty() { format!("会话 {}", short_id(&session)) } else { title },
        project: basename(&dir),
        dir,
        model,
        status: status_by_mtime(updated_at, now).into(),
        status_from: "mtime".into(),
        started_at,
        updated_at,
        note: String::new(),
    })
}

// ———————————————— Hermes ————————————————

fn parse_hermes(path: &Path, updated_at: i64, now: i64) -> Option<AiTask> {
    let text = std::fs::read_to_string(path).ok()?;
    let j: Value = serde_json::from_str(&text).ok()?;
    // 没有 `messages` 就不是一次会话（文件名过滤之外的第二道闸）——
    // 与其把一个形状不对的文件硬渲染成卡片，不如让它根本进不来。
    if !j.get("messages").map(|m| m.is_array()).unwrap_or(false) {
        return None;
    }
    let session = j
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    let title = j
        .get("messages")
        .and_then(|m| m.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
                .and_then(|m| m.get("content").and_then(|v| v.as_str()))
        })
        .map(clip_title)
        .unwrap_or_default();
    Some(AiTask {
        id: format!("hermes:{session}"),
        // Hermes 的会话恢复没有稳定的命令行写法（且它连 cwd 都不记）—— 不编一条。
        resume_cmd: String::new(),
        session_id: session.clone(),
        tool: "hermes".into(),
        tool_label: "Hermes".into(),
        title: if title.is_empty() { format!("会话 {}", short_id(&session)) } else { title },
        // Hermes 的会话记录里没有工作目录字段 —— 没有就是没有，不拿家目录冒充。
        dir: String::new(),
        project: String::new(),
        model: j.get("model").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        status: status_by_mtime(updated_at, now).into(),
        status_from: "mtime".into(),
        started_at: j.get("session_start").and_then(|v| v.as_str()).map(iso_utc_ms).unwrap_or(0),
        updated_at,
        note: String::new(),
    })
}

// ———————————————— AI 自己写的任务看板（uking-board 技能包）————————————————

/// `~/.uking/board/board.json`：`uking-board` 技能包让 AI 把任务进展落盘的地方。
///
/// 它跟上面三个来源本质不同：**状态是 AI 自己声明的**（todo/doing/done/blocked），
/// 不是我们从文件活跃度推的 —— 所以 `status_from` 标 `declared`，别混为一谈。
///
/// ## 单条任务的时间是怎么来的（别再用文件 mtime 冒充）
/// 原来 36 条全填 `file_ms`（整份 board.json 的 mtime），于是 8 月 7 号写的和 8 月 16 号写的
/// 在卡片上一模一样都是「3 天前」—— **那是文件的时间，不是任务的时间**。
/// 单条里只有 `updated`：`2026-08-07 17:02:48`，一个**不带时区**的本地时间串。
/// 没有 chrono 折不成 epoch，但**两个同格式的串相减，时区自己抵消**，所以：
///
/// ```text
/// 这条距今 ≈ (最新那条的 updated − 本条的 updated) + (now − board.json 的 mtime)
/// ```
///
/// 🔴 误差方向是**保守的**：文件最后一次写盘通常晚于最新那条 `updated`
/// （本机实测差 9 小时 —— 最后那次写盘只改了别的字段），所以这个估计会**低估**陈旧度。
/// 低估 = 该标陈旧的可能没标，绝不会把新鲜的冤枉成陈旧。这个方向是故意选的。
/// `2026-08-07 17:02:48` → 一个**只用来相减**的秒数。
///
/// 🔴 **这不是 epoch，别拿它跟 `SystemTime` 比。** 那个串没带时区，折成绝对时刻要知道
/// 本机时区偏移，纯 std 拿不到。但两个同格式的串相减，偏移自己抵消 —— 本函数只服务于相减。
/// 格式不对（少字段 / 非数字 / 月日越界）一律 `None`，绝不猜一个默认值：
/// 猜出来的时间会让一条陈旧任务显示成「刚刚」，比没有时间更坏。
fn civil_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |a: usize, z: usize| -> Option<i64> { s.get(a..z)?.parse::<i64>().ok() };
    let (y, mo, d) = (n(0, 4)?, n(5, 7)?, n(8, 10)?);
    let (h, mi, sec) = (n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    // days_from_civil（Howard Hinnant 的公历算法）—— 十几行纯算术，不值得为它引 chrono。
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + mi * 60 + sec)
}

fn parse_board() -> (Vec<AiTask>, AiTaskSource) {
    let path = board_file();
    let present = path.exists();
    let mut src = AiTaskSource {
        tool: "board".into(),
        label: "AI 任务看板".into(),
        path: path.to_string_lossy().into_owned(),
        present,
        readable: present,
        files_in_window: usize::from(present),
        files_scanned: usize::from(present),
        count: 0,
        note: if present {
            "状态由 AI 自己声明（uking-board 技能包写的），不是我们推的".into()
        } else {
            "还没有 AI 用过 uking-board 技能包".into()
        },
    };
    if !present {
        return (Vec::new(), src);
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        src.readable = false;
        src.note = "board.json 读不出来（权限或正在被写）".into();
        return (Vec::new(), src);
    };
    let Ok(j) = serde_json::from_str::<Value>(&text) else {
        src.readable = false;
        src.note = "board.json 不是合法 JSON".into();
        return (Vec::new(), src);
    };
    // 整份看板的落盘时间。单条任务的时间由它 + `updated` 串的**相对差**推出来，见函数头。
    let file_ms = mtime_ms(&path);
    let now = now_ms();
    // 看板上最新的那条 `updated`。拿它当「file_ms 那一刻」的锚点，
    // 每条任务的时间 = file_ms − (锚点 − 本条)。一条 `updated` 都解析不出来时退回老行为。
    let anchor = j
        .get("tasks")
        .and_then(|t| t.as_object())
        .map(|m| {
            m.values()
                .filter_map(|t| t.get("updated").and_then(|v| v.as_str()).and_then(civil_secs))
                .max()
        })
        .unwrap_or(None);
    let mut out = Vec::new();
    if let Some(map) = j.get("tasks").and_then(|t| t.as_object()) {
        for (id, t) in map {
            let declared = t.get("status").and_then(|v| v.as_str()).unwrap_or("todo");
            // blocked = 卡在外部条件上，最贴近看板的「等待输入」那一列；
            // 它**不是**出错 —— AI 说自己卡住了，不代表这活失败了。
            let status = match declared {
                "doing" => "running",
                "done" => "done",
                "blocked" => "waiting_input",
                _ => "idle",
            };
            let folder = t.get("folder").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let progress = t.get("progress").and_then(|v| v.as_str()).unwrap_or_default();
            let updated = t.get("updated").and_then(|v| v.as_str()).unwrap_or_default();

            // 这条任务自己的时间（见函数头）。解析不出来就退回文件 mtime —— 老行为，
            // 但**只对解析不出来的那几条**退回，不是所有条一起退回。
            let updated_at = match (anchor, civil_secs(updated)) {
                (Some(a), Some(s)) => file_ms - (a - s) * 1000,
                _ => file_ms,
            };
            // 🔴 陈旧只降 `doing` / `blocked` —— 它们是关于**现在**的断言，会过期。
            // `done` 是关于过去的断言（这活干完了），放一年也还是真的，不许降。
            let stale = (now - updated_at) / 1000 > DECLARED_FRESH_SECS;
            let demote = stale && matches!(status, "running" | "waiting_input");
            let (status, status_from) = if demote {
                ("idle", "declared_stale")
            } else {
                (status, "declared")
            };

            let mut note = String::new();
            if demote {
                // 降级必须留痕：不写这句，用户只会看到任务从「进行中」凭空消失。
                note.push_str(&format!(
                    "AI 在 {} 声明「{}」，之后 {} 天没再更新 —— 现在它在不在跑我们不知道",
                    if updated.is_empty() { "更早" } else { updated },
                    declared,
                    (now - updated_at) / 1000 / 86_400,
                ));
            }
            if !progress.is_empty() {
                if !note.is_empty() {
                    note.push_str(" · ");
                }
                note.push_str(&clip_title(progress));
            }
            if !updated.is_empty() && !demote {
                if !note.is_empty() {
                    note.push_str(" · ");
                }
                note.push_str(updated);
            }
            out.push(AiTask {
                id: format!("board:{id}"),
                // 看板任务**不是一次会话**，没有可续接的对象。它记的是「这件事干到哪了」，
                // 接手方式是把这段进度读给下一个 AI —— 那是任务护照那条路，不是 --resume。
                resume_cmd: String::new(),
                session_id: id.clone(),
                tool: "board".into(),
                tool_label: "AI 任务看板".into(),
                title: clip_title(t.get("title").and_then(|v| v.as_str()).unwrap_or(id)),
                project: basename(&folder),
                dir: folder,
                model: String::new(),
                status: status.into(),
                status_from: status_from.into(),
                started_at: 0,
                updated_at,
                note,
            });
        }
    }
    src.count = out.len();
    (out, src)
}

// ———————————————— 入口 ————————————————

/// 扫本机各家 AI 的任务记录。**只读，不联网，不改任何东西。**
pub fn inspect(days: i64) -> AiTasksReport {
    let days = days.clamp(1, 365);
    let now = now_ms();
    let mut tasks: Vec<AiTask> = Vec::new();
    let mut sources: Vec<AiTaskSource> = Vec::new();
    let mut truncated = false;

    // —— Claude Code ——
    {
        let dir = claude_projects_dir();
        let present = dir.exists();
        let (files, total, cut) = if present {
            recent_files(&[dir.clone()], days, CLAUDE_DEPTH, &|n| n.ends_with(".jsonl"))
        } else {
            (Vec::new(), 0, false)
        };
        truncated |= cut;
        let scanned = files.len();
        let before = tasks.len();
        for (p, ms) in files {
            if let Some(t) = parse_claude(&p, ms, now) {
                tasks.push(t);
            }
        }
        sources.push(AiTaskSource {
            tool: "claude".into(),
            label: "Claude Code".into(),
            path: dir.to_string_lossy().into_owned(),
            present,
            readable: present,
            files_in_window: total,
            files_scanned: scanned,
            count: tasks.len() - before,
            note: if present { String::new() } else { "这台机器上没装 Claude Code，或它还没跑过".into() },
        });
    }

    // —— Codex CLI ——
    {
        let dir = codex_sessions_dir();
        let present = dir.exists();
        let (files, total, cut) = if present {
            recent_files(&[dir.clone()], days, CODEX_DEPTH, &|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
        } else {
            (Vec::new(), 0, false)
        };
        truncated |= cut;
        let scanned = files.len();
        let before = tasks.len();
        for (p, ms) in files {
            if let Some(t) = parse_codex(&p, ms, now) {
                tasks.push(t);
            }
        }
        sources.push(AiTaskSource {
            tool: "codex".into(),
            label: "Codex CLI".into(),
            path: dir.to_string_lossy().into_owned(),
            present,
            readable: present,
            files_in_window: total,
            files_scanned: scanned,
            count: tasks.len() - before,
            note: if present { String::new() } else { "这台机器上没装 Codex CLI，或它还没跑过".into() },
        });
    }

    // —— Hermes ——
    {
        let dirs = hermes_sessions_dirs();
        let present = dirs.iter().any(|d| d.exists());
        let (files, total, cut) = if present {
            // 🔴 **只认 `session_*.json`**：同一个目录里还躺着 `request_dump_*.json`
            // （出错时的请求转储，字段是 timestamp/reason/request/error，压根没有 messages）。
            // 按 `*.json` 一把抓的话，本机 11 个转储会被当成 11 条「任务」摆上看板 ——
            // 那不是把信息给多了，是把错的说成真的。
            recent_files(&dirs, days, HERMES_DEPTH, &|n| n.starts_with("session_") && n.ends_with(".json"))
        } else {
            (Vec::new(), 0, false)
        };
        truncated |= cut;
        let scanned = files.len();
        let before = tasks.len();
        for (p, ms) in files {
            if let Some(t) = parse_hermes(&p, ms, now) {
                tasks.push(t);
            }
        }
        sources.push(AiTaskSource {
            tool: "hermes".into(),
            label: "Hermes".into(),
            path: describe_dirs(&dirs),
            present,
            readable: present,
            files_in_window: total,
            files_scanned: scanned,
            count: tasks.len() - before,
            note: if present { String::new() } else { "这台机器上没装 Hermes，或它还没跑过".into() },
        });
    }

    // —— AI 自己写的任务看板 ——
    {
        let (mut board, src) = parse_board();
        tasks.append(&mut board);
        sources.push(src);
    }

    // —— OpenClaw / ClawX ——
    {
        let dirs = openclaw_session_dirs();
        let present = !dirs.is_empty();
        // `*.trajectory.jsonl` 是同一次会话的**逐步轨迹**（跟 `<uuid>.jsonl` 一一对应），
        // 收进来等于每个会话数两遍。`sessions.json` 是索引不是会话。
        let (files, total, cut) = if present {
            recent_files(&dirs, days, OPENCLAW_DEPTH, &|n| {
                n.ends_with(".jsonl") && !n.ends_with(".trajectory.jsonl")
            })
        } else {
            (Vec::new(), 0, false)
        };
        truncated |= cut;
        let scanned = files.len();
        let before = tasks.len();
        for (p, ms) in files {
            if let Some(t) = parse_openclaw(&p, ms, now) {
                tasks.push(t);
            }
        }
        sources.push(AiTaskSource {
            tool: "openclaw".into(),
            label: "OpenClaw / ClawX".into(),
            path: describe_dirs(&dirs),
            present,
            readable: present,
            files_in_window: total,
            files_scanned: scanned,
            count: tasks.len() - before,
            note: if present {
                String::new()
            } else {
                "这台机器上没装 OpenClaw / ClawX，或它还没跑过".into()
            },
        });
    }

    // 由新到旧。看板要的是「最近在忙什么」，最新的排最前。
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    // id 去重（排序之后做 → 留下的必是最新那条）。上面已经用深度上限挡掉了子代理记录这个
    // 已知的撞车源，这里是兜底：**id 撞了在界面上是 React key 撞车**，症状是卡片鬼畜刷新
    // 或者点 A 打开 B，而不是一条报错——所以宁可在数据层就保证唯一。
    let mut seen = std::collections::HashSet::new();
    tasks.retain(|t| seen.insert(t.id.clone()));
    // 去重后回填每个来源的条数 —— 各来源自己数的那一份是去重**前**的，
    // 留着就会出现「五个来源加起来 ≠ 总数」，那种自相矛盾的面板没人会信。
    for s in sources.iter_mut() {
        s.count = tasks.iter().filter(|t| t.tool == s.tool).count();
        // ★ 观测记账（影核 docs/PROPOSAL-OBSERVATION-ACCOUNTING §5.2）：
        // **0 必须能被解释**。一个光秃秃的「Hermes 0」，用户分不清是「它没有会话」
        // 还是「我们没读到」—— 而这两件事一个是世界的事实、一个是我们的 bug。
        // 这条规矩是 conformance 强制的：0 条且 note 为空 = 违规。
        if s.count == 0 && s.note.is_empty() {
            s.note = if !s.present {
                format!("这台机器上没有 {} 的记录目录", s.label)
            } else if !s.readable {
                format!("{} 的记录读不动（权限或格式）", s.label)
            } else if s.files_in_window == 0 {
                format!("装了，但最近 {days} 天内没有新会话")
            } else {
                // 有文件、却一条都没解析出来 —— 最值得警觉的一种 0：
                // 多半是对方换了格式，而不是「他没干活」。
                format!(
                    "窗口内有 {} 个文件却一条都没解析出来 —— 多半是它换了记录格式，该查",
                    s.files_in_window
                )
            };
        }
    }

    let mut counts = AiTaskCounts::default();
    for t in &tasks {
        match t.status.as_str() {
            "running" => counts.running += 1,
            "idle" => counts.idle += 1,
            "waiting_input" => counts.waiting_input += 1,
            _ => counts.done += 1,
        }
    }
    counts.total = tasks.len();

    let mut notes = vec![
        format!("只看最近 {days} 天。各家 AI 的会话记录里没有「这次成没成」这种字段，所以状态只按一条能证明的事实判：{ACTIVE_SECS} 秒内这个会话文件还在被写 = 还在跑。"),
        "外部 AI 的会话一律不给「出错」状态 —— 我们没有判据，猜一个错的比不给更糟。".into(),
    ];
    if truncated {
        notes.push(format!("有来源的会话文件超过 {MAX_FILES_PER_TOOL} 个，只取了最新的那批 —— 更早的没扫。"));
    }

    AiTasksReport {
        generated_at: now,
        days,
        active_window_secs: ACTIVE_SECS,
        tasks,
        sources,
        counts,
        truncated,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 五个来源各造一份**真实形状**的记录，跑完整 `inspect()`，断言每一家都被认出来。
    ///
    /// 为什么值得单开这么一条：五个解析器认的是**别人家**的文件格式，而那些格式只在
    /// 对方升级时悄悄变。`conformance` 只能证明这个动作「跑得动、形状对」——
    /// 开发机上有真数据它绿，客户机上一条都读不出来它**照样绿**（0 条也是合法返回）。
    /// 只有拿已知输入断言已知输出，才盯得住「格式变了 / 路径写错了 / 标题取错字段」。
    ///
    /// 🔴 必须是**一个** `#[test]`：`UKING_TEST_HOME` 是进程级环境变量，拆成多个用例会互相抢。
    #[test]
    fn every_source_is_parsed_from_a_synthetic_home() {
        use std::fs;
        // 沙箱进出、`UKING_TEST_HOME` 的设与还原、目录清理，全部交给 testsandbox 的
        // 那把全局锁。**别在这儿自己 set_var** —— 这条用例正是因为裸设环境变量，
        // 被别的模块并行踩成「Hermes 一条都没读到」。
        let sb = crate::testsandbox::enter("aitasks-fixture", &[]);
        let root = sb.root().to_path_buf();
        let w = |p: &Path, s: &str| {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, s).unwrap();
        };

        // —— Claude Code：人敲的那条带 origin.kind=human ——
        w(
            &root.join(".claude/projects/proj-a/sess1.jsonl"),
            "{\"type\":\"user\",\"sessionId\":\"c1\",\"cwd\":\"D:\\\\work\\\\alpha\",\"timestamp\":\"2026-08-01T00:00:00.000Z\",\"origin\":{\"kind\":\"human\"},\"message\":{\"role\":\"user\",\"content\":\"把合同里的甲方改一下\"}}\n\
             {\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-5\"}}\n",
        );
        // 定时/无头跑起来的那种：isMeta:true、没有 origin —— 标题必须仍然取得到。
        w(
            &root.join(".claude/projects/proj-a/sess2.jsonl"),
            "{\"type\":\"user\",\"sessionId\":\"c2\",\"cwd\":\"D:\\\\work\\\\beta\",\"isMeta\":true,\"timestamp\":\"2026-08-01T00:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"每日巡视\"}}\n",
        );
        // 子代理记录：带**父会话的 id**，必须被深度上限挡在外面（否则 id 撞车）。
        w(
            &root.join(".claude/projects/proj-a/c1/subagents/agent-x.jsonl"),
            "{\"type\":\"user\",\"sessionId\":\"c1\",\"cwd\":\"D:\\\\work\\\\alpha\",\"origin\":{\"kind\":\"human\"},\"message\":{\"role\":\"user\",\"content\":\"子代理内部\"}}\n",
        );

        // —— Codex：标题只能来自 event_msg/user_message，不能来自 response_item ——
        w(
            &root.join(".codex/sessions/2026/08/01/rollout-x.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"type\":\"session_meta\",\"session_id\":\"x1\",\"cwd\":\"D:\\\\work\\\\gamma\",\"timestamp\":\"2026-08-01T00:00:00.000Z\",\"model\":\"gpt-5.4-codex\"}}\n\
             {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<recommended_plugins>别拿我当标题</recommended_plugins>\"}]}}\n\
             {\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"<codex_delegation><input>跑一遍服务器巡检</input></codex_delegation>\"}}\n",
        );

        // —— Hermes：真会话 vs 出错转储（转储绝不能变成一张卡）——
        w(
            &root.join(".hermes/sessions/session_20260801_000000_000000.json"),
            "{\"session_id\":\"h1\",\"model\":\"deepseek-v4-flash\",\"session_start\":\"2026-08-01T00:00:00.000Z\",\"messages\":[{\"role\":\"user\",\"content\":\"写个周报\"}]}",
        );
        w(
            &root.join(".hermes/sessions/request_dump_20260801_000000_000000.json"),
            "{\"session_id\":\"h-dump\",\"reason\":\"error\",\"request\":{},\"error\":\"boom\"}",
        );

        // —— OpenClaw / ClawX：两个 home 都要认 ——
        w(
            &root.join(".openclaw/agents/main/sessions/o1.jsonl"),
            "{\"type\":\"session\",\"id\":\"o1\",\"cwd\":\"D:\\\\work\\\\delta\",\"timestamp\":\"2026-08-01T00:00:00.000Z\"}\n\
             {\"type\":\"model_change\",\"modelId\":\"deepseek-v4-pro\"}\n\
             {\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"做一张宣传海报\"}}\n",
        );
        // 轨迹文件跟上面那条是同一次会话，收进来就会数两遍。
        w(
            &root.join(".openclaw/agents/main/sessions/o1.trajectory.jsonl"),
            "{\"type\":\"session\",\"id\":\"o1\"}\n",
        );
        // U-King 便携 home（内置终端用的那个）——只扫默认 home 的话这条会消失。
        w(
            &root.join(".uking/openclaw/agents/main/sessions/o2.jsonl"),
            "{\"type\":\"session\",\"id\":\"o2\",\"cwd\":\"D:\\\\work\\\\eps\",\"timestamp\":\"2026-08-01T00:00:00.000Z\"}\n\
             {\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"剪个短视频\"}]}}\n",
        );

        // —— AI 自己写的看板：状态是它**声明**的，不是我们推的 ——
        w(
            &root.join(".uking/board/board.json"),
            "{\"version\":1,\"tasks\":{\"t1\":{\"id\":\"t1\",\"title\":\"浏览器玩法\",\"status\":\"doing\",\"folder\":\"D:\\\\work\\\\zeta\",\"progress\":\"Gate0 已过\",\"updated\":\"2026-08-07 17:02:48\"},\
              \"t2\":{\"id\":\"t2\",\"title\":\"等接口\",\"status\":\"blocked\"},\
              \"t3\":{\"id\":\"t3\",\"title\":\"待排期\",\"status\":\"todo\"},\
              \"t4\":{\"id\":\"t4\",\"title\":\"半年前说在做\",\"status\":\"doing\",\"updated\":\"2026-01-01 00:00:00\"},\
              \"t5\":{\"id\":\"t5\",\"title\":\"半年前说做完了\",\"status\":\"done\",\"updated\":\"2026-01-01 00:00:00\"}}}",
        );

        let r = inspect(365);

        let by = |tool: &str| -> Vec<&AiTask> { r.tasks.iter().filter(|t| t.tool == tool).collect() };
        let title_of = |tool: &str, id: &str| -> String {
            r.tasks
                .iter()
                .find(|t| t.tool == tool && t.id.ends_with(id))
                .map(|t| t.title.clone())
                .unwrap_or_else(|| format!("<{tool}:{id} 没被认出来>"))
        };

        // 五个来源全部读到
        assert_eq!(by("claude").len(), 2, "Claude：子代理记录被算进来了？");
        assert_eq!(by("codex").len(), 1);
        assert_eq!(by("hermes").len(), 1, "Hermes：request_dump 被当成会话了");
        assert_eq!(by("openclaw").len(), 2, "OpenClaw：两个 home / 轨迹文件去重不对");
        assert_eq!(by("board").len(), 5);

        // 标题取自正确的字段
        assert_eq!(title_of("claude", "c1"), "把合同里的甲方改一下");
        assert_eq!(title_of("claude", "c2"), "每日巡视", "isMeta 的定时会话丢了标题");
        assert_eq!(title_of("codex", "x1"), "跑一遍服务器巡检", "Codex 标题取错字段或没剥 delegation");
        assert_eq!(title_of("hermes", "h1"), "写个周报");
        assert_eq!(title_of("openclaw", "o1"), "做一张宣传海报");
        assert_eq!(title_of("openclaw", "o2"), "剪个短视频", "便携 home 没被扫");

        // id 唯一（撞了会在界面上表现为 React key 撞车，不是一条报错）
        let mut ids: Vec<&str> = r.tasks.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "有重复的任务 id");

        // 状态：外部会话一律按 mtime 推（刚写的 → running）；看板那三条是 AI 自己声明的
        for t in by("claude").iter().chain(by("codex").iter()).chain(by("openclaw").iter()) {
            assert_eq!(t.status_from, "mtime");
        }
        let board_status = |id: &str| {
            r.tasks.iter().find(|t| t.id == format!("board:{id}")).map(|t| t.status.as_str()).unwrap_or("?")
        };
        assert_eq!(board_status("t1"), "running", "doing 应映射成进行中");
        assert_eq!(board_status("t2"), "waiting_input", "blocked 归「等待输入」，不是「出错」");
        assert_eq!(board_status("t3"), "idle");

        // ★ 陈旧声明必须降级。「进行中」是关于**现在**的断言，半年前那句 `doing`
        //   证明不了它现在在跑 —— 真机上 16 条 doing 有 8 条是 12 天前写的，
        //   把用户唯一那条真在跑的会话埋在了里面。
        assert_eq!(board_status("t4"), "idle", "半年前声明的 doing 不能还算「进行中」");
        let t4 = r.tasks.iter().find(|t| t.id == "board:t4").unwrap();
        assert_eq!(t4.status_from, "declared_stale", "降级了就不能再自称 declared");
        assert!(t4.note.contains("2026-01-01"), "降级必须留痕，否则看着像任务凭空消失：{}", t4.note);
        // 🔴 `done` 是关于**过去**的断言，不会过期，放多久都不许降。
        assert_eq!(board_status("t5"), "done", "已完成不该因为年代久远被降级");
        assert_eq!(
            r.tasks.iter().find(|t| t.id == "board:t5").unwrap().status_from,
            "declared",
        );
        // 没降级的照旧如实标 declared
        for id in ["t1", "t2", "t3"] {
            assert_eq!(
                r.tasks.iter().find(|t| t.id == format!("board:{id}")).unwrap().status_from,
                "declared",
                "{id} 不该被降级",
            );
        }
        // ★ 单条任务的时间必须是**它自己的**，不是整份 board.json 的 mtime。
        //   原来 36 条全填 file_ms，于是 8-07 写的和 8-16 写的在卡片上都显示「3 天前」。
        let t1_at = r.tasks.iter().find(|t| t.id == "board:t1").unwrap().updated_at;
        assert!(t4.updated_at < t1_at, "t4 比 t1 老半年，时间戳必须能区分开");

        // ★ 绝不给外部会话安「出错」——没有判据就不猜
        assert!(r.tasks.iter().all(|t| t.status != "error"), "外部会话不该有 error 状态");

        // 每个来源自报的条数必须跟实际渲染的对得上（否则面板会自相矛盾）
        for s in &r.sources {
            assert_eq!(s.count, by(&s.tool).len(), "来源 {} 自报条数对不上", s.tool);
        }
        // 目录清理由 `sb` 出作用域时负责（panic 也清）。
    }

    /// 一台**什么 AI 都没装**的机器上：不能崩、不能瞎报，而且要说清「没有」不是「没读到」。
    #[test]
    fn empty_machine_reports_nothing_without_lying() {
        let _sb = crate::testsandbox::enter("aitasks-empty", &[]);
        let r = inspect(7);

        assert_eq!(r.counts.total, 0);
        assert!(r.tasks.is_empty());
        assert!(!r.truncated);
        // 五个来源都得列出来并说明「没装」——静默少一个来源，客户会以为我们查过了。
        assert_eq!(r.sources.len(), 5, "来源清单不该因为空就少几项");
        for s in &r.sources {
            assert!(!s.present, "{} 在空机器上不该 present", s.tool);
            assert!(!s.note.is_empty(), "{} 没说明为什么是 0", s.tool);
        }
    }

    #[test]
    fn iso_utc_ms_parses_and_rejects() {
        // 1970-01-01T00:00:00Z = 0；一天后 = 86400000。
        assert_eq!(iso_utc_ms("1970-01-01T00:00:00Z"), 0);
        assert_eq!(iso_utc_ms("1970-01-02T00:00:00Z"), 86_400_000);
        // 带毫秒。
        assert_eq!(iso_utc_ms("1970-01-01T00:00:01.500Z"), 1_500);
        // 闰年边界：2024-02-29 存在，算出来必须比 2024-02-28 正好多一天。
        let a = iso_utc_ms("2024-02-28T00:00:00Z");
        let b = iso_utc_ms("2024-02-29T00:00:00Z");
        assert_eq!(b - a, 86_400_000);
        // 垃圾输入给 0，不 panic（release 是 panic=abort，热路径炸了整个进程没）。
        assert_eq!(iso_utc_ms(""), 0);
        assert_eq!(iso_utc_ms("not-a-date"), 0);
        assert_eq!(iso_utc_ms("2026-13-99T99:99:99Z"), 0);
    }

    #[test]
    fn clip_title_is_char_safe_and_single_line() {
        assert_eq!(clip_title("  hello\nworld  "), "hello world");
        let long = "中".repeat(MAX_TITLE_CHARS + 50);
        let out = clip_title(&long);
        // 按**字符**截断：中文一个字 3 字节，按字节切会切出半个字变成乱码。
        assert_eq!(out.chars().count(), MAX_TITLE_CHARS + 1); // +1 是省略号
        assert!(out.ends_with('…'));
    }

    #[test]
    fn unwrap_delegation_strips_codex_wrapper() {
        let m = "<codex_delegation>\n  <source_thread_id>x</source_thread_id>\n  <input>去把巡检跑一遍</input>\n</codex_delegation>";
        assert_eq!(unwrap_delegation(m), "去把巡检跑一遍");
        // 没有包装就原样返回。
        assert_eq!(unwrap_delegation("普通消息"), "普通消息");
    }

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("C:\\Users\\me\\proj"), "proj");
        assert_eq!(basename("/home/me/proj"), "proj");
        assert_eq!(basename("/home/me/proj/"), "proj");
    }

    #[test]
    fn status_by_mtime_only_claims_what_it_can_prove() {
        let now = 1_000_000_000i64;
        assert_eq!(status_by_mtime(now - 1_000, now), "running");
        assert_eq!(status_by_mtime(now - (ACTIVE_SECS as i64 + 1) * 1000, now), "done");
    }

    /// `civil_secs` 只承诺一件事：**同格式的两个串相减是对的**。
    /// 格式不对时必须 `None` —— 猜一个默认值会让陈旧任务显示成「刚刚」，比没时间更坏。
    #[test]
    fn civil_secs_is_only_good_for_subtracting() {
        let a = civil_secs("2026-08-07 17:02:48").unwrap();
        let b = civil_secs("2026-08-16 02:20:00").unwrap();
        assert_eq!(b - a, 8 * 86_400 + 9 * 3_600 + 17 * 60 + 12, "真机上那两条的实际间隔");
        // 跨闰年 2 月末（days_from_civil 抄错的话这条会歪一天）
        assert_eq!(
            civil_secs("2024-03-01 00:00:00").unwrap() - civil_secs("2024-02-28 00:00:00").unwrap(),
            2 * 86_400,
            "2024 是闰年，2/28 到 3/1 是两天",
        );
        assert_eq!(
            civil_secs("2026-03-01 00:00:00").unwrap() - civil_secs("2026-02-28 00:00:00").unwrap(),
            86_400,
            "2026 不是闰年，2/28 到 3/1 是一天",
        );
        // 垃圾输入一律 None，不许兜底成 0（0 会被当成 1970 年 → 万物皆陈旧）
        for bad in ["", "2026-08-07", "2026/08/07 17:02:48", "20260807 170248", "abcd-ef-gh ij:kl:mn", "2026-13-01 00:00:00", "2026-08-32 00:00:00", "2026-08-07 24:00:00"] {
            assert!(civil_secs(bad).is_none(), "{bad:?} 不该被解析出来");
        }
    }
}
