//! 应用内真终端（PTY）—— Rust 拥有伪终端，前端 xterm.js 只渲染 + 转发输入。
//!
//! ## 为什么自己起 PTY 而不弹外部窗口
//! 外部 PowerShell 用 `-NoProfile` 又不注入 PATH，导致 openclaw/hermes 找不到命令。
//! 这里复用 `installer::search_paths`（与安装/验证时完全一致的 PATH），把便携 Node、
//! `%APPDATA%\npm`、便携 Python Scripts 等目录前置进子 shell 的 PATH —— openclaw/hermes
//! 因此能直接跑。
//!
//! ## 生命周期
//! 一个会话 = 一个长驻 shell。收起抽屉只隐藏 UI，不杀进程（openclaw gateway 继续跑）。
//! 输出走 Tauri Channel 流回前端；键盘经 `term_write` 写回 PTY stdin。
//!
//! ## panic=abort 安全
//! release profile 是 `panic="abort"`，reader 线程内一旦 panic 会整体 abort。
//! 因此 reader/writer 热路径**零 unwrap/expect**，全部 `let _ =` / `if let Ok`。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tauri::ipc::Channel;

use crate::installer::{portable_node_dir, search_paths};

/// 升级已被确认后，禁止再分配新的 PTY。这个开关覆盖下载和替换之间的整段窗口，
/// 使替换前的会话复核不再依赖「数一数是否恰好没变」这种竞态猜测。
static UPDATING: AtomicBool = AtomicBool::new(false);
/// `term_open_pty` 已经越过入口检查、但尚未把会话放进表的调用数。
///
/// 冻结转换先封住新入口，再等这批调用把可杀资源都登记完，才真正置 `UPDATING`。这样
/// `UPDATING=true` 之后不可能再凭空出现一条没被升级流程看见的 PTY。
static IN_FLIGHT_OPEN: AtomicUsize = AtomicUsize::new(0);
/// 比 `UPDATING` 更早的一道闸：转换期先置它，阻止新的 open 进入；等在飞 open 清空后才
/// 置 `UPDATING`。两阶段是为了关掉「入口检查刚过、升级恰好置位」的窗口。
static OPENING_BLOCKED: AtomicBool = AtomicBool::new(false);

fn update_transition_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

const UPDATING_REJECTION: &str = "升级进行中，暂不能新开终端；升级完成后即可正常使用";

/// 已通过入口检查的 open 持有此票据，直到 PTY 会话已入表（或建失败的资源已被回收）。
/// Drop 负责所有早退路径，不能让一次错误把升级转换永久卡在「在飞」状态。
struct InFlightOpenGuard;

impl InFlightOpenGuard {
    fn begin() -> Result<Self, String> {
        if OPENING_BLOCKED.load(Ordering::SeqCst) {
            return Err(UPDATING_REJECTION.into());
        }
        IN_FLIGHT_OPEN.fetch_add(1, Ordering::SeqCst);
        // 入口检查和计数之间也可能刚好开始冻结；这次不再分配任何资源。
        if OPENING_BLOCKED.load(Ordering::SeqCst) {
            IN_FLIGHT_OPEN.fetch_sub(1, Ordering::SeqCst);
            return Err(UPDATING_REJECTION.into());
        }
        Ok(Self)
    }
}

impl Drop for InFlightOpenGuard {
    fn drop(&mut self) {
        IN_FLIGHT_OPEN.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 尝试进入升级冻结期。
///
/// 顺序不能反：先封住新 open，再等已经越过入口的调用把 PTY 入表/回收，最后才置
/// `UPDATING`。5 秒只是一轮等待诊断阈值；超过后继续等待而非带着未登记 PTY 强行替换，
/// 因为本函数成功返回的语义就是「此后没有新的 PTY 能建成」。
pub fn term_updating_begin() -> bool {
    let Ok(_transition) = update_transition_lock().lock() else {
        return false;
    };
    if OPENING_BLOCKED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return false;
    }
    let started = Instant::now();
    let mut reported_wait = false;
    while IN_FLIGHT_OPEN.load(Ordering::SeqCst) != 0 {
        if !reported_wait && started.elapsed() >= Duration::from_secs(5) {
            reported_wait = true;
            crate::ulog::write("term", "升级冻结等待在飞终端建立超过 5 秒，继续等待其入表或回收");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    UPDATING.store(true, Ordering::SeqCst);
    true
}

/// 结束升级冻结期；失败路径必须调用，成功路径会随当前进程硬退出而自然清除。
pub fn term_updating_end() {
    UPDATING.store(false, Ordering::SeqCst);
    OPENING_BLOCKED.store(false, Ordering::SeqCst);
}

/// 用 RAII 保证下载、校验或换包失败后不会把「禁止新开终端」泄漏到正常运行期。
pub struct TermUpdatingGuard {
    armed: bool,
}

impl TermUpdatingGuard {
    pub fn begin() -> Result<Self, String> {
        if term_updating_begin() {
            Ok(Self { armed: true })
        } else {
            Err("升级正在进行中，请稍候".into())
        }
    }

    /// 替换脚本/安装程序已成功拉起，当前进程即将退出，不再在 Drop 中解除冻结。
    pub fn keep_until_process_exit(&mut self) {
        self.armed = false;
    }
}

impl Drop for TermUpdatingGuard {
    fn drop(&mut self) {
        if self.armed {
            term_updating_end();
        }
    }
}

struct PtySession {
    master: Box<dyn portable_pty::MasterPty + Send>,
    /// 写入队列 —— **不直接持有 writer**，见 `term_write` 的注释。
    /// 队列断开（本结构被 drop）时 writer 线程自然收尾。
    writer_tx: Sender<Vec<u8>>,
    /// **只留 killer，不留 Child** —— Child 被交给了 waiter 线程去 `wait()`（见 term_open_pty）。
    /// `clone_killer` 就是给这个场景设计的：另一个线程 blocked 在 `.wait` 时照样能发信号。
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    /// 这个会话跑的是哪个工具（claude/codex/openclaw/hermes…）；纯终端无 tag。
    /// 运行面板（list_running）据此聚合「哪些工具在跑」。
    tool: Option<String>,
    /// 实际传给 PTY 的工作目录。升级后只能重开同等环境，不能复活进程内状态。
    cwd: Option<String>,
    /// 通过 `initial_cmd` 启动的命令；手敲的后续输入不冒充成可重放命令。
    cmd: Option<String>,
    /// 前端最后一次报平安的时间（term_ping / term_write / term_resize 都会刷新）。
    /// watchdog 发现它太久没动 = 前端（WebView2 渲染进程）已经死了，回收掉这个会话，
    /// 免得空闲 shell 一路累积把应用拖死（2026-08-06 实机 35 个泄漏会话的教训）。
    last_seen: Instant,
}

/// 前端心跳超过这个时长没来，就认为 WebView2 渲染进程已死，watchdog 回收会话。
/// 阈值要留足余量：窗口缩托盘后 Chromium 会把后台 setInterval 节流到 ~1 次/分钟，
/// 所以 180s ≈ 3 次节流心跳的余量，不会误杀「用户还开着但不可见」的会话。
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(180);
/// watchdog 扫描周期。
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);

// ★ 这里原来还有一条 `MAX_SESSIONS: usize = 16` 的会话数硬上限，已删除。它是拿**数量**
//   去近似「有没有泄漏」，而数量根本不是泄漏的判据 —— 用户合法开 17 个标签，和漏了 17 个
//   孤儿会话，在计数上完全一样。判据换成准确的「还有没有人认领」之后（term_ping 带 alive），
//   孤儿会在 HEARTBEAT_TIMEOUT 内被收走，上限就没有存在意义了。删除原因详见 reap_stale。

/// 把会话从表里摘掉（顺带回收 master / 断开写队列 / 让 reader 见到 EOF）。
///
/// **为什么回收挂在这儿而不在 `list_running` 里**：原来「回收已死会话」只写在 `list_running`
/// 内，而它唯一的调用方 `useRunning.ts` / `RunPanel.tsx` 在 0.8.6 砍 AuxBar 之后就没人挂载了
/// —— 于是每个退出的终端都在 `sessions()` 里留一条死记录（master PTY 句柄 + 写队列 + child
/// 句柄），进程活多久漏多久。回收必须挂在「进程真的退了」这个事实上，不能寄生在某个 UI 轮询接口。
///
/// `kill=true` 用于「前端 Channel 已经没了」的情况：此时 shell 可能还活着，但已经没有任何人
/// 看得到它、也没人关得掉它，留着就是野进程 —— 按这一个 PID 收掉（不碰同名进程）。
fn take_session(id: &str, kill: bool) {
    let Ok(mut map) = sessions().lock() else {
        return;
    };
    let Some(mut s) = map.remove(id) else {
        return; // term_close 已经先一步摘走了
    };
    if kill {
        let _ = s.killer.kill();
    }
    // s 在这里 drop：master 关掉 → reader 线程读到 EOF 退出；writer_tx 断开 → writer 线程退出
    drop(s);
    drop(map);
    schedule_snapshot_write();
}

fn sessions() -> &'static Mutex<HashMap<String, PtySession>> {
    static S: OnceLock<Mutex<HashMap<String, PtySession>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 终端升级快照：只保存足以「重开同样目录和命令的终端」的公开启动信息，
/// 不保存屏幕内容、输入历史或任何 PTY 句柄。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSession {
    pub cwd: Option<String>,
    pub cmd: Option<String>,
    pub tool: Option<String>,
    /// 从命令行抽出的 Claude/Codex 会话标识；旧快照没有这个字段也可照常读取。
    #[serde(default)]
    pub resume_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    /// 快照用途：`upgrade` 才会在新版本中显示恢复卡；`live` 只用于运行期保全。
    /// 旧版本快照没有这个字段，反序列化为空字符串并按升级快照兼容处理。
    #[serde(default)]
    pub kind: String,
    pub sessions: Vec<SnapshotSession>,
}

const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const SNAPSHOT_DEBOUNCE: Duration = Duration::from_millis(500);

fn snapshot_path() -> std::path::PathBuf {
    crate::installer::uking_home().join("term-snapshot.json")
}

/// 运行期会话表变更的防抖快照。它绝不能复用升级恢复卡的文件，否则正常开关终端会
/// 伪造一张「上次升级」卡，也会在用户尚未处理时覆盖真正的升级快照。
fn live_snapshot_path() -> std::path::PathBuf {
    crate::installer::uking_home().join("term-sessions-live.json")
}

fn write_snapshot_file(path: &Path, snapshot: &SnapshotInfo) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err("终端快照路径没有父目录".into());
    };
    std::fs::create_dir_all(parent).map_err(|e| format!("创建终端快照目录失败: {e}"))?;
    let bytes = serde_json::to_vec(snapshot).map_err(|e| format!("编码终端快照失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    let write_result = (|| -> Result<(), String> {
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("写入终端快照失败: {e}"))?;
        file.write_all(&bytes).map_err(|e| format!("写入终端快照失败: {e}"))?;
        file.sync_all().map_err(|e| format!("落盘终端快照失败: {e}"))?;
        // 同目录 rename 是提交点：旧快照保留到新文件完整落盘后，崩溃没有丢快照的空窗。
        std::fs::rename(&tmp, path).map_err(|e| format!("提交终端快照失败: {e}"))
    })();
    if write_result.is_err() {
        // 临时文件失败只清临时文件，绝不能碰尚可恢复的旧快照。
        let _ = std::fs::remove_file(&tmp);
    }
    write_result
}

fn resume_hint_from_command(tool: Option<&str>, cmd: Option<&str>) -> Option<String> {
    if !matches!(tool, Some("claude" | "codex")) {
        return None;
    }
    let mut tokens = cmd?.split_whitespace();
    while let Some(token) = tokens.next() {
        if let Some(id) = token.strip_prefix("--resume=") {
            return (!id.is_empty() && !id.starts_with('-')).then(|| id.to_string());
        }
        if token == "--resume" || token == "-r" {
            return tokens
                .next()
                .filter(|id| !id.is_empty() && !id.starts_with('-'))
                .map(str::to_string);
        }
    }
    None
}

fn snapshot_from_sessions(kind: &str) -> Result<SnapshotInfo, String> {
    let map = sessions()
        .lock()
        .map_err(|_| "终端状态异常，无法保存快照".to_string())?;
    Ok(SnapshotInfo {
        kind: kind.to_string(),
        sessions: map
            .values()
            .map(|session| SnapshotSession {
                cwd: session.cwd.clone(),
                cmd: session.cmd.clone(),
                tool: session.tool.clone(),
                resume_hint: resume_hint_from_command(session.tool.as_deref(), session.cmd.as_deref()),
            })
            .collect(),
    })
}

/// 会话表变更后，合并 500ms 内的多次 open/close 再原子落盘。
///
/// 这是崩溃/断电恢复用的实时快照，空会话也照样写入（恢复卡不会读取 live 文件），
/// 所以关掉最后一个终端不会留下一个已经失真的「仍在运行」快照。
fn schedule_snapshot_write() {
    #[derive(Default)]
    struct DebounceState {
        generation: u64,
        scheduled: bool,
    }

    static STATE: OnceLock<Mutex<DebounceState>> = OnceLock::new();
    let state = STATE.get_or_init(|| Mutex::new(DebounceState::default()));
    let Ok(mut state) = state.lock() else {
        crate::ulog::write("term", "终端快照防抖锁异常，跳过本次落盘");
        return;
    };
    state.generation = state.generation.wrapping_add(1);
    if state.scheduled {
        return;
    }
    state.scheduled = true;
    let mut observed_generation = state.generation;
    drop(state);

    std::thread::spawn(move || loop {
        std::thread::sleep(SNAPSHOT_DEBOUNCE);
        let Some(state) = STATE.get() else {
            return;
        };
        let changed = match state.lock() {
            Ok(mut state) => {
                if state.generation != observed_generation {
                    observed_generation = state.generation;
                    true
                } else {
                    state.scheduled = false;
                    false
                }
            }
            Err(_) => {
                crate::ulog::write("term", "终端快照防抖锁异常，跳过本次落盘");
                return;
            }
        };
        if changed {
            continue;
        }
        // 升级卡未消费前不写实时快照：即使未来两条落盘路径再次靠近，也不能让运行期
        // 防抖任务抢在用户处理恢复卡之前把它的语义抹掉。老快照 kind 为空，按 upgrade 处理。
        if read_snapshot_file(&snapshot_path()).is_some_and(|snapshot| snapshot.kind != "live") {
            return;
        }
        match snapshot_from_sessions("live")
            .and_then(|snapshot| write_snapshot_file(&live_snapshot_path(), &snapshot)) {
            Ok(()) => {}
            Err(e) => crate::ulog::write("term", &format!("终端快照落盘失败：{e}")),
        }
        return;
    });
}

fn snapshot_is_fresh(age: Duration) -> bool {
    age <= SNAPSHOT_MAX_AGE
}

fn read_snapshot_file(path: &Path) -> Option<SnapshotInfo> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    if !snapshot_is_fresh(modified.elapsed().ok()?) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<SnapshotInfo>(&text).ok()
}

/// 在进程硬退出前同步写入可重开信息。返回值只在**本次确实写入新快照**时为 true；
/// 调用方据此决定失败时能否消费它，绝不能误删上次升级留下、用户还没处理的恢复卡。
pub fn snapshot_sessions(path: &Path) -> bool {
    let snapshot = match snapshot_from_sessions("upgrade") {
        Ok(snapshot) => snapshot,
        Err(_) => return false,
    };
    if snapshot.sessions.is_empty() {
        // 本次没有可写条目，旧恢复卡仍属于上次升级；本次中止/空会话都无权删除它。
        return false;
    }
    match write_snapshot_file(path, &snapshot) {
        Ok(()) => true,
        Err(e) => {
            crate::ulog::write("term", &format!("升级前终端快照未写入：{e}"));
            false
        }
    }
}

/// 当前仍由本进程托管的 PTY 数。升级前不能把锁中毒伪装成 0，否则会放行一次未经确认的关闭。
pub fn term_active_count_checked() -> Result<usize, String> {
    sessions()
        .lock()
        .map(|map| map.len())
        .map_err(|_| "终端状态异常，已中止升级以保护运行中的终端".into())
}

/// 前端确认弹窗用的同一份真实计数；锁中毒会成为 IPC 错误，而不是假装没有终端。
#[tauri::command]
pub fn term_active_count() -> Result<usize, String> {
    term_active_count_checked()
}

/// 新版启动后查看是否有未消费且未过期的升级快照（超过 7 天不再提醒）。
#[tauri::command]
pub fn term_snapshot_pending() -> Option<SnapshotInfo> {
    term_snapshot_pending_from_path(&snapshot_path())
}

fn term_snapshot_pending_from_path(path: &Path) -> Option<SnapshotInfo> {
    read_snapshot_file(path).filter(|snapshot| snapshot.kind != "live" && !snapshot.sessions.is_empty())
}

/// 用户已重开或明确忽略后消费快照，避免每次启动重复提醒。
#[tauri::command]
pub fn term_snapshot_consume() -> Result<(), String> {
    let path = snapshot_path();
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("删除终端快照失败: {e}"));
        }
    }
    Ok(())
}

/// 单调递增会话 id（不引 uuid）。
fn next_id() -> String {
    static N: OnceLock<Mutex<u64>> = OnceLock::new();
    let m = N.get_or_init(|| Mutex::new(0));
    let mut g = match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *g += 1;
    format!("t{}", *g)
}

/// 与 installer 完全一致的 PATH（前置便携工具目录），让 openclaw/hermes 等可解析。
fn build_path() -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let dirs = search_paths(portable_node_dir().as_deref());
    let prefix = dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(sep);
    let old = std::env::var("PATH").unwrap_or_default();
    if prefix.is_empty() {
        old
    } else {
        format!("{prefix}{sep}{old}")
    }
}

fn home_dir() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into())
}

/// OpenClaw 数据根：`$HOME/.uking/openclaw`（参考 device.rs::uking_home 的范式）。
/// gateway 与 CLI 共享它 → 互相发现并连同一本地 gateway（ws://127.0.0.1:18789）。
fn openclaw_home() -> String {
    std::path::PathBuf::from(home_dir())
        .join(".uking")
        .join("openclaw")
        .display()
        .to_string()
}

/// 给终端注入 OPENCLAW_* 环境变量 —— 让 OpenCodex 终端里 `openclaw gateway run` 起的 gateway
/// 和其他终端的 `openclaw` CLI 共享同一 home，从而能调它的能力（含 word/excel/ppt 办公技能）。
fn inject_openclaw_env(builder: &mut CommandBuilder) {
    let home = openclaw_home();
    let _ = std::fs::create_dir_all(&home);
    builder.env("OPENCLAW_HOME", &home);
    builder.env("OPENCLAW_STATE_DIR", &home);
    builder.env("OPENCLAW_DISABLE_BONJOUR", "1"); // 便携环境禁 mDNS 广播，免端口/实例碰撞
}

/// 注入 pip / uv 国内镜像 —— Hermes 第一次用 TTS/语音等功能会在自己进程里
/// `uv pip install` / `pip install edge-tts`（site-packages/tools/lazy_deps.py），
/// 不带 index 走默认 PyPI → 国内卡死。pip 走 pip.ini 已兜底，这里再补 uv（uv 只认环境变量）
/// 和 pip 环境变量双保险。客户机实测：卡在 subprocess.run，用户 Ctrl+C 才停。
fn inject_pip_mirror(builder: &mut CommandBuilder) {
    const INDEX: &str = "https://mirrors.aliyun.com/pypi/simple/";
    builder.env("PIP_INDEX_URL", INDEX);
    builder.env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
    builder.env("UV_INDEX_URL", INDEX); // uv 旧字段
    builder.env("UV_DEFAULT_INDEX", INDEX); // uv 新字段
}

/// 把本地回环加进 `NO_PROXY` —— 终端里跑的 AI CLI 全都要连本机端口。
///
/// 客户机常年开着 clash 式梯子（`HTTP_PROXY=http://127.0.0.1:7890`），而终端里的 codex 要连
/// Codex 省钱路由（`127.0.0.1:15722`）、openclaw 要连 gateway（`18789`）。这些请求被塞给梯子后，
/// 梯子不认得这个地址，回一个 **502 Bad Gateway**，客户看到的就是
/// 「unexpected status 502 Bad Gateway: Unknown error, url: http://127.0.0.1:15722/v1/responses」
/// —— 而我们自己的代理日志里**连一条请求记录都没有**（issue #309 实证：尾部只有 4 条「启动」，
/// 零错误）。方向完全指反：看着像我们的代理挂了，其实请求压根没到过它。
///
/// 注意**不能**照抄 `installer.rs::with_path` / `agent/codex.rs` 那样把代理变量整个 `env_remove`：
/// 那两处的子进程只访问国内镜像，清掉是对的；而终端是通用的，用户完全可能就指望这个梯子上外网。
/// 只把回环排除掉，其余流量该走代理照走。
fn inject_no_proxy_loopback(builder: &mut CommandBuilder) {
    const LOOPBACK: &str = "127.0.0.1,localhost,::1";
    let cur = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    let merged = if cur.trim().is_empty() { LOOPBACK.to_string() } else { format!("{cur},{LOOPBACK}") };
    // 大小写都设：curl/python 认小写，Go/Rust 系多认大写，各家不统一。
    builder.env("NO_PROXY", &merged);
    builder.env("no_proxy", &merged);
}

/// cwd 选择：传入路径非空且确为已存在目录则用它，否则回落 home。
/// （工作台按任务文件夹开终端用；原底部抽屉传 None → 行为不变。）
fn resolve_cwd(cwd: Option<String>) -> String {
    cwd.map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && std::path::Path::new(p).is_dir())
        .unwrap_or_else(home_dir)
}

/// 待运行命令校验：放宽到支持带参命令（`claude --resume`、`codex --model x` 等），
/// 同时挡住 shell 注入。
///
/// 规则：按空格切 token —— 首 token（程序名）必须在固定允许集内；其余每个 token 里的
/// **ASCII 字符**只允许 `[A-Za-z0-9-_=./:]` 且不含 `..`（防元字符注入与路径穿越）；
/// **非 ASCII 字符**（如中文提示词）一律放行 —— cmd.exe / PowerShell 的元字符
/// （`&|<>^%!"` `` ` `` `;$(){}` 等）全部是 ASCII，非 ASCII 字符不可能被 shell 当元字符
/// 解释，所以只放行非 ASCII 不会削弱现有的注入防护（见 `src/opencodex/apps.ts` 里
/// cline 的中文一次性任务提示词 `cline 说说你能做什么`，旧的纯 ASCII 白名单会把它整条拒绝）。
/// 空命令、以及超过 512 字符的命令直接拒绝（后者是防呆：bat 单行长度有实际上限）。
///
/// ## 风险留痕：中文命令不要走 `term_open_external`
/// `term_open_external` 把命令写进 UTF-8 编码的 .bat 文件再交给 cmd.exe 解析，但 cmd.exe
/// 解析 .bat 文件用的是当前系统 OEM 代码页，bat 内 `chcp 65001 >nul` 只切了**运行时**代码页，
/// 对同一份 bat 文件后续行本身怎么被解析并不可靠（GBK 系统上可能把中文命令行解析乱）。
/// 含非 ASCII 字符的命令应当只走内嵌 PTY（`term_open_pty`）执行，不应走 `term_open_external`；
/// 这条「该走哪条路由」的判断本阶段未实现，留给后续 `runtime.tool.launch` 动作做，见该函数
/// 附近注释，实现时不要漏掉这一条。
fn validate_cmd(cmd: &str) -> bool {
    const ALLOWED_PROGRAMS: &[&str] =
        &["claude", "codex", "openclaw", "hermes", "dsh", "harness-doctor", "opencode", "pi", "qwen", "crush", "cline", "node", "npm", "git", "ollama"];
    const MAX_LEN: usize = 512;
    if cmd.len() > MAX_LEN {
        return false;
    }
    let mut tokens = cmd.split_whitespace();
    let Some(prog) = tokens.next() else {
        return false;
    };
    if !ALLOWED_PROGRAMS.contains(&prog) {
        return false;
    }
    tokens.all(|t| {
        !t.contains("..")
            && t.chars().all(|c| {
                !c.is_ascii() || c.is_ascii_alphanumeric() || "-_=./:".contains(c)
            })
    })
}

/// 构造交互式 shell 命令。Windows 优先 PowerShell 7（pwsh），回落 Windows PowerShell 5.1；
/// macOS/Unix 用登录 shell。
#[cfg(windows)]
fn shell_builder() -> CommandBuilder {
    // 优先 PowerShell 7 (pwsh)——跟用户外面用的终端一致（新版 profile / 别名 / PSReadLine），
    // 没装才回落老的 Windows PowerShell 5.1。两者都用绝对路径起：客户机 PATH 丢 System32
    // （pc-*** 实锤）时裸名起不来=终端打不开。都不加 -NoProfile（要 profile 里的 PATH/别名），
    // 便携工具目录我们已额外前置进 PATH（build_path）。
    let exe = crate::installer::find_pwsh()
        .unwrap_or_else(|| crate::installer::system_tool("powershell"));
    let mut cmd = CommandBuilder::new(exe);
    cmd.args(["-NoLogo"]);
    cmd
}

#[cfg(not(windows))]
fn shell_builder() -> CommandBuilder {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(shell);
    cmd.arg("-l");
    cmd
}

/// 无头自检：起一个 PTY 跑一条命令，把输出收集回来（验证 PATH 注入 + ConPTY 正常）。
/// 给 `--term-test <cmd>` 用，不依赖 GUI / xterm。
pub fn headless_run(cmd: &str, timeout_ms: u64) -> Result<String, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 100, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("openpty: {e}"))?;
    let mut builder = shell_builder();
    builder.env("PATH", build_path());
    builder.env("TERM", "xterm-256color");
    inject_openclaw_env(&mut builder);
    inject_pip_mirror(&mut builder);
    inject_no_proxy_loopback(&mut builder);
    builder.cwd(home_dir());
    let mut child = pair.slave.spawn_command(builder).map_err(|e| format!("spawn: {e}"))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| format!("reader: {e}"))?;
    let mut writer = pair.master.take_writer().map_err(|e| format!("writer: {e}"))?;

    let out = std::sync::Arc::new(Mutex::new(Vec::<u8>::new()));
    let out2 = out.clone();
    let t = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut g) = out2.lock() {
                        g.extend_from_slice(&buf[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    });

    let _ = writer.write_all(cmd.as_bytes());
    let _ = writer.write_all(b"\r\n");
    let _ = writer.write_all(b"exit\r\n");
    let _ = writer.flush();

    // 简单超时等待
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < timeout_ms as u128 {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    drop(pair.master);
    let _ = t.join();

    let g = out.lock().map_err(|_| "lock")?;
    Ok(String::from_utf8_lossy(&g).to_string())
}

/// 无头验证一个终端会话的完整生命周期：`U-King.exe --term-session-test`
///
/// 验的是 **GUI 才走得到、`action conformance` 又完全盖不住**的那条路 —— `term_open_pty` /
/// `term_write` 的写入队列、按键保序、EOF 哨兵、会话自清。这四件事一个字节都不在动作表里，
/// 不留这条入口就只能靠人开着窗口点。`tauri::ipc::Channel` 可以直接 `new`（不需要 webview），
/// 所以干净机 / CI 上一样跑得起来。
///
/// 两个会话分开验，互不污染：
///   A：按键保序 → 自然 `exit` → 退出说明行 + EOF 空帧 → 会话已从表里摘掉
///   B：一次灌 1MB → `term_write` 必须**立刻返回**（老实现在这里会阻塞好几秒，而它跑在 UI 主线程）
pub async fn headless_session_test() -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn collector() -> (Channel<Vec<u8>>, Arc<Mutex<String>>, Arc<AtomicBool>) {
        let text = Arc::new(Mutex::new(String::new()));
        let eof = Arc::new(AtomicBool::new(false));
        let (t2, e2) = (text.clone(), eof.clone());
        let ch: Channel<Vec<u8>> = Channel::new(move |body| {
            let bytes: Vec<u8> = body.deserialize().unwrap_or_default();
            if bytes.is_empty() {
                e2.store(true, Ordering::Relaxed); // ★ 空帧 = EOF 哨兵（前端据此置灰标签）
            } else if let Ok(mut g) = t2.lock() {
                g.push_str(&String::from_utf8_lossy(&bytes));
            }
            Ok(())
        });
        (ch, text, eof)
    }
    let sleep = |ms: u64| std::thread::sleep(std::time::Duration::from_millis(ms));

    /// 握手式等就绪：反复敲一条带唯一标记的命令，直到回显里**确认它真的被执行了**。
    ///
    /// 为什么不用「固定 sleep」也不用「输出静默即就绪」—— 这两版在干净机上各栽过一次
    /// （开发机早装了 PS7，这条冷启动路本地一次都走不到）：
    ///   · 固定 2.5s：冷机要先下 106MB PS7，PS7 首启远不止 2.5s，命令敲进没准备好的进程被吞掉；
    ///   · 静默 500ms：PS7 首启分几段吐输出（banner → profile → 提示符），中间的停顿足够骗过它。
    /// 标记出现 **≥2 次** 才算数：一次是输入回显，一次是命令真的执行后的输出 —— 只看回显
    /// 说明不了 shell 已经在处理命令。
    fn wait_shell_ready(text: &std::sync::Arc<Mutex<String>>, id: &str) -> bool {
        for n in 0..30 {
            let probe = format!("UKREADY{n}");
            if term_write(id.to_string(), format!("echo {probe}\r")).is_err() {
                return false;
            }
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let hit = text
                    .lock()
                    .map(|g| g.matches(probe.as_str()).count() >= 2)
                    .unwrap_or(false);
                if hit {
                    return true;
                }
            }
        }
        false
    }
    let mut fails: Vec<String> = Vec::new();

    // ---- A：保序 + EOF 哨兵 + 自清 ----
    let (ch_a, text_a, eof_a) = collector();
    let id_a = term_open_pty(120, 30, ch_a, None, None, Some("selftest".into())).await?;
    if !wait_shell_ready(&text_a, &id_a) {
        fails.push("shell 60s 内没能执行一条 echo（起不来？）".into());
    }
    // 清掉 PS7 下载日志 / banner / 握手回显，免得污染下面的顺序断言
    if let Ok(mut g) = text_a.lock() {
        g.clear();
    }
    for i in 1..=6u8 {
        term_write(id_a.clone(), format!("echo UK{i}\r"))?;
    }
    // 回显里 UK1..UK6 必须**原序**出现（改成 async + spawn_blocking 就会在这里翻车）。
    // ★ 同样不许写死 sleep 等结果 —— 冷机（2 核、刚解压完 106MB PS7）跑完 6 条命令远不止
    // 2.5 秒，干净机上就是这么误报成「没保序」的。改成轮询到齐为止。
    let in_order = |s: &str| {
        let mut cursor = 0usize;
        for i in 1..=6u8 {
            match s[cursor..].find(&format!("UK{i}")) {
                Some(p) => cursor += p + 3,
                None => return false,
            }
        }
        true
    };
    let mut order_ok = false;
    for _ in 0..150 {
        sleep(200);
        let seen = text_a.lock().map(|g| g.clone()).unwrap_or_default();
        if in_order(&seen) {
            order_ok = true;
            break;
        }
    }
    if !order_ok {
        fails.push("按键顺序：30s 内 UK1..UK6 没有按序出现在回显里".into());
    }

    term_write(id_a.clone(), "exit\r".into())?;
    for _ in 0..60 {
        if eof_a.load(Ordering::Relaxed) {
            break;
        }
        sleep(100);
    }
    if !eof_a.load(Ordering::Relaxed) {
        fails.push("EOF 哨兵：exit 之后没收到空帧（前端会一直以为终端还活着）".into());
    }
    let tail = text_a.lock().map(|g| g.clone()).unwrap_or_default();
    if !tail.contains("进程已退出") {
        fails.push("退出说明行：没收到「[进程已退出…]」那一行".into());
    }
    // 会话自清：EOF 之后表里不该还留着这条（否则句柄泄漏，同 list_running 那个老坑）
    if term_write(id_a.clone(), "x".into()).is_ok() {
        fails.push("会话自清：进程退出后 sessions() 里仍留着这条记录".into());
    }

    // ---- B：大块写入不阻塞 ----
    let (ch_b, text_b, _e_b) = collector();
    let id_b = term_open_pty(120, 30, ch_b, None, None, Some("selftest".into())).await?;
    wait_shell_ready(&text_b, &id_b);
    let t0 = std::time::Instant::now();
    term_write(id_b.clone(), "A".repeat(1024 * 1024))?;
    let write_ms = t0.elapsed().as_millis();
    // 队列化之后这里是纯入队。老实现实测 1MB 要 **7.3 秒**（而且跑在 UI 主线程上），所以
    // 1.5s 的门槛仍有 5 倍余量、足够把回归揪出来。**不敢卡得再紧**：干净机（2 核、刚解压完
    // PS7）上实测量到过 187ms —— 那是 1MB 分配 + 线程调度的正常抖动，不是阻塞，卡 200ms 会误报。
    if write_ms > 1500 {
        fails.push(format!("写入阻塞：1MB 的 term_write 花了 {write_ms}ms（应当立刻返回）"));
    }
    let _ = term_close(id_b);

    let report = format!(
        "{{\"ok\":{},\"write_1mb_ms\":{},\"order_ok\":{},\"eof_sentinel\":{},\"fails\":{}}}",
        fails.is_empty(),
        write_ms,
        order_ok,
        eof_a.load(Ordering::Relaxed),
        serde_json::to_string(&fails).unwrap_or_else(|_| "[]".into()),
    );
    if fails.is_empty() {
        Ok(report)
    } else {
        Err(report)
    }
}

/// 在**独立系统终端窗口**里运行命令（取代内嵌终端页）。
///
/// 为什么需要它：客户想要「打开终端 = 一个独立 app」，关掉 U-King 主窗口后终端照常活着
/// （openclaw gateway 不被一起杀）。难点是——直接 `start cmd` 弹出来的窗口里 PATH 没有便携
/// Node/Python，openclaw/hermes/claude/codex 全都 command not found（这正是当初做内嵌终端
/// 的原因，见本文件顶部注释）。
///
/// 解法：写一个临时 `.bat`，开头 `set PATH=<注入便携工具的完整PATH>` + 注入 OPENCLAW_*，
/// 再 `cmd /K <用户命令>` 保持窗口常驻。用 `cmd /C start` 把这个 bat 拉成一个**全新、独立**
/// 的控制台进程 —— 它不是 U-King 的子窗口，U-King 退出也不影响它。
///
/// 安全：命令仍过 `validate_cmd` 白名单（claude/codex/openclaw/hermes/...），挡 shell 注入。
/// 空命令（纯开终端）允许，此时只 `cmd /K` 给个已注入 PATH 的交互 shell。
///
/// ⚠️ 含非 ASCII 字符（如中文提示词）的命令目前**仍会**被这里的 `.bat` 路由接受（`validate_cmd`
/// 已放行非 ASCII），但 bat 文件按 OEM 代码页解析、`chcp 65001` 只切运行时代码页这一风险尚未
/// 在这里处理（详见 `validate_cmd` 函数上方注释）。本阶段不在此处加检测拒绝——「打开独立终端」
/// 是通用功能（cwd 为空、纯开一个 shell 也走这里），现在加限制风险不对称；路由判断留给
/// 后续 `runtime.tool.launch` 动作实现，不要漏掉。
#[tauri::command]
pub fn term_open_external(cmd: Option<String>, cwd: Option<String>) -> Result<(), String> {
    let cmd = cmd.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    if let Some(c) = &cmd {
        if !validate_cmd(c) {
            crate::ulog::write("launch", &format!("term_open_external 拒绝：命令未过白名单 {c}"));
            return Err(format!("不允许的命令：{c}"));
        }
    }

    #[cfg(windows)]
    {
        use std::io::Write as _;
        let path = build_path();
        let home = openclaw_home();
        let _ = std::fs::create_dir_all(&home);
        let workdir = resolve_cwd(cwd);

        // run 行：有命令就 /K 跑命令并保活；纯开终端就 /K 进交互 shell。
        let run_line = match &cmd {
            Some(c) => format!("cmd /K \"{c}\""),
            None => "cmd /K".to_string(),
        };

        let bat = format!(
            "@echo off\r\n\
             chcp 65001 >nul\r\n\
             title U-King \u{7ec8}\u{7aef}\r\n\
             set \"PATH={path}\"\r\n\
             set \"OPENCLAW_HOME={home}\"\r\n\
             set \"OPENCLAW_STATE_DIR={home}\"\r\n\
             set \"OPENCLAW_DISABLE_BONJOUR=1\"\r\n\
             set \"PIP_INDEX_URL=https://mirrors.aliyun.com/pypi/simple/\"\r\n\
             set \"PIP_DISABLE_PIP_VERSION_CHECK=1\"\r\n\
             set \"UV_INDEX_URL=https://mirrors.aliyun.com/pypi/simple/\"\r\n\
             set \"UV_DEFAULT_INDEX=https://mirrors.aliyun.com/pypi/simple/\"\r\n\
             cd /d \"{workdir}\"\r\n\
             echo [U-King] \u{5df2}\u{6ce8}\u{5165}\u{5de5}\u{5177}\u{8def}\u{5f84}\u{ff0c}openclaw / hermes / claude / codex \u{53ef}\u{76f4}\u{63a5}\u{8fd0}\u{884c}\r\n\
             {run_line}\r\n",
        );

        // 每个窗口用唯一文件名（进程 id + 单调计数），避免连开多个终端互相覆盖 bat。
        let bat_file = std::env::temp_dir()
            .join(format!("uking_term_{}_{}.bat", std::process::id(), next_id()));
        std::fs::File::create(&bat_file)
            .and_then(|mut f| f.write_all(bat.as_bytes()))
            .map_err(|e| {
                let msg = format!("\u{5199}\u{7ec8}\u{7aef}\u{542f}\u{52a8}\u{811a}\u{672c}\u{5931}\u{8d25}: {e}");
                crate::ulog::write("launch", &format!("term_open_external {msg}"));
                msg
            })?;

        // `cmd /C start "" <bat>` —— start 把 bat 拉成独立新控制台进程（与 U-King 无父子关系）。
        // 头一个空 "" 是 start 的窗口标题占位（不可省，否则带引号的路径会被当标题）。
        std::process::Command::new(crate::installer::system_tool("cmd"))
            .args(["/C", "start", ""])
            .arg(&bat_file)
            .spawn()
            .map_err(|e| {
                let msg = format!("\u{542f}\u{52a8}\u{72ec}\u{7acb}\u{7ec8}\u{7aef}\u{5931}\u{8d25}: {e}");
                crate::ulog::write("launch", &format!("term_open_external {msg}"));
                msg
            })?;
        crate::ulog::write("launch", &format!("term_open_external ✓ cmd={}", cmd.as_deref().unwrap_or("-")));
        return Ok(());
    }

    // macOS：写一个临时 `.command` 脚本（注入便携工具 PATH + OPENCLAW_*/pip 镜像），
    // 用 `open -a Terminal` 拉成一个**独立的 Terminal.app 窗口** —— 与 Windows 的独立终端
    // 同款体验：关掉 U-King 主窗口它照常活着。脚本末尾 `exec $SHELL -il` 保持交互（命令跑完
    // 不闪退）。命令同样过 `validate_cmd` 白名单。
    //
    // 不实现 macOS 分支的后果（v0.9.13 实测）：客户点工具卡「打开终端」→ `term_open_external`
    // 直接 `Err` → 前端回落到「同一个」内嵌终端页，多次点击 pendingCmd 互相覆盖，
    // 看起来就是「几个终端都没打开」。
    #[cfg(target_os = "macos")]
    {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt;
        let path = build_path();
        let home = openclaw_home();
        let _ = std::fs::create_dir_all(&home);
        let workdir = resolve_cwd(cwd);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());

        // run 行：有命令就先跑命令，跑完 exec 交互 shell 保活；纯开终端直接交互 shell。
        let run_line = match &cmd {
            Some(c) => format!("{c}\nexec \"{shell}\" -il\n"),
            None => format!("exec \"{shell}\" -il\n"),
        };

        let script = format!(
            "#!/bin/zsh\n\
             export PATH=\"{path}\"\n\
             export OPENCLAW_HOME=\"{home}\"\n\
             export OPENCLAW_STATE_DIR=\"{home}\"\n\
             export OPENCLAW_DISABLE_BONJOUR=1\n\
             export PIP_INDEX_URL=\"https://mirrors.aliyun.com/pypi/simple/\"\n\
             export PIP_DISABLE_PIP_VERSION_CHECK=1\n\
             export UV_INDEX_URL=\"https://mirrors.aliyun.com/pypi/simple/\"\n\
             export UV_DEFAULT_INDEX=\"https://mirrors.aliyun.com/pypi/simple/\"\n\
             cd \"{workdir}\"\n\
             echo \"[U-King] 已注入工具路径，openclaw / hermes / claude / codex 可直接运行\"\n\
             {run_line}",
        );

        // 每个窗口唯一文件名（进程 id + 单调计数），避免连开多个终端互相覆盖脚本。
        let script_file = std::env::temp_dir()
            .join(format!("uking_term_{}_{}.command", std::process::id(), next_id()));
        std::fs::File::create(&script_file)
            .and_then(|mut f| f.write_all(script.as_bytes()))
            .map_err(|e| format!("写终端启动脚本失败: {e}"))?;
        // .command 需可执行才能被 Terminal.app 当脚本跑
        let _ = std::fs::set_permissions(&script_file, std::fs::Permissions::from_mode(0o755));

        std::process::Command::new("open")
            .args(["-a", "Terminal"])
            .arg(&script_file)
            .spawn()
            .map_err(|e| format!("启动独立终端失败: {e}"))?;
        return Ok(());
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (cmd, cwd);
        Err("\u{5f53}\u{524d}\u{5e73}\u{53f0}\u{6682}\u{53ea}\u{652f}\u{6301} Windows / macOS \u{72ec}\u{7acb}\u{7ec8}\u{7aef}".into())
    }
}

/// PowerShell 7 的准备状态（进程内共享）。
///
/// ★ 为什么要有这道闸：`ensure_pwsh` 要下 **106MB**。原来 `term_open_pty` 每次被调都无条件跑一遍
/// —— 连开三个标签就是三个**并发**的 106MB 下载；而下载失败（慢网客户的常态）更糟：没有任何
/// 失败记忆，**每开一个新终端就整个重来一次**，客户的体感就是「每次开终端都卡在下载上」。
///
/// 两件事：① 一把闸串行化 —— 后来的调用者等第一个下完，直接复用它的结果，不再重复下载；
/// ② 失败后进冷却期，期内不再重试。**不是永久放弃** —— 网好了下一次照样会自己起来。
#[cfg(windows)]
struct PwshGate {
    ready: bool,
    failed_at: Option<std::time::Instant>,
}

#[cfg(windows)]
fn pwsh_gate() -> &'static Mutex<PwshGate> {
    static G: OnceLock<Mutex<PwshGate>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(PwshGate {
            ready: false,
            failed_at: None,
        })
    })
}

#[cfg(windows)]
const PWSH_RETRY_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// 准备 PS7（**阻塞**，必须在 `spawn_blocking` 里调）。`say` 把进度打进发起这次的终端窗格。
#[cfg(windows)]
fn prepare_pwsh_once(say: &(dyn Fn(&str) + Send + Sync)) {
    let mut g = match pwsh_gate().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    // 能走到这儿说明已经拿到闸：前一个调用者若已经下完，这里直接复用，不再下第二遍
    if g.ready || crate::installer::find_pwsh().is_some() {
        g.ready = true;
        return;
    }
    if let Some(t) = g.failed_at {
        if t.elapsed() < PWSH_RETRY_COOLDOWN {
            say("\x1b[33m! 上次没能装上 PowerShell 7，本次先用系统自带的 5.1（稍后会自动再试）。\x1b[0m\n\n");
            return;
        }
    }
    say("\x1b[36m⏳ 正在准备 PowerShell 7 环境（仅首次，约 106MB，请稍候）…\x1b[0m\n");
    let logger = |_lvl: &str, msg: &str| say(&format!("  {msg}\n"));
    match crate::installer::ensure_pwsh(&logger, false) {
        Ok(_) => {
            g.ready = true;
            g.failed_at = None;
            say("\x1b[32m✓ PowerShell 7 就绪。\x1b[0m\n\n");
        }
        Err(e) => {
            g.failed_at = Some(std::time::Instant::now());
            say(&format!(
                "\x1b[33m! 暂未能准备 PowerShell 7（{e}），本次先用系统自带 PowerShell 5.1。\x1b[0m\n\n"
            ));
        }
    }
}

/// 起一个 PTY 会话。返回 session_id；输出通过 `on_data` Channel 流回前端。
///
/// **不带 `#[tauri::command]`、且刻意不叫 `term_open`** —— 注册的是 `lib.rs` 里的
/// `term_open` 薄壳，它多记一条 `tool_use` 用量事件。本模块不认识 metrics（功能模块之间
/// 不互相 import，组合根在 lib.rs）。名字必须错开：`#[tauri::command]` 生成的
/// `__cmd__xxx` 宏落在 crate root，同名会直接撞成 E0255/E0659。
pub async fn term_open_pty(
    cols: u16,
    rows: u16,
    on_data: Channel<Vec<u8>>,
    initial_cmd: Option<String>,
    cwd: Option<String>,
    tool: Option<String>,
) -> Result<String, String> {
    // 必须在 PowerShell 下载、PTY 分配、命令解析等任何资源动作之前拿到在飞票据。
    // 票据直到会话入表才释放，升级冻结便能无竞态地等待这批调用全部可见。
    let _opening = InFlightOpenGuard::begin()?;

    // 校验「启动即执行」的初始命令，越早越好：命令非法就直接拒绝这次 open 请求，不必先
    // 付出 PS7 下载/PTY 分配的代价，也不能静默丢掉命令后仍然开一个空终端——那样调用方
    // 以为命令生效了，实际什么都没跑，是另一种静默失败（手敲进已打开终端的后续输入走
    // `term_write`，不在这里，也不受这条校验约束）。
    let initial_cmd = match initial_cmd.map(|cmd| cmd.trim().to_string()) {
        Some(cmd) if cmd.is_empty() => None,
        Some(cmd) if !validate_cmd(&cmd) => {
            crate::ulog::write("term", &format!("term_open_pty 拒绝：初始命令未过白名单 {cmd}"));
            return Err(format!("不允许的命令：{cmd}"));
        }
        other => other,
    };

    // 首次开终端且客户机没有 PowerShell 7 时，下发便携 PS7（很多老机器只有 5.1，中文易乱码、
    // 无 PSReadLine）。进度直接写进本终端窗格（on_data 已就绪），下完这一次之后 shell_builder
    // 里的 find_pwsh 就命中便携版、秒开。下载失败回落 5.1，终端照常能开。
    #[cfg(windows)]
    if crate::installer::find_pwsh().is_none() {
        // 进度提示、成功/失败文案全部放进 prepare_pwsh_once —— 只有真要下载的那一次才刷屏，
        // 后来的调用者拿到闸时已经就绪，安安静静秒开。
        let on_data_log = on_data.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let say = move |s: &str| {
                let _ = on_data_log.send(s.replace('\n', "\r\n").into_bytes());
            };
            prepare_pwsh_once(&say);
        })
        .await;
    }

    let resolved_cwd = resolve_cwd(cwd);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty 失败: {e}"))?;

    let mut builder = shell_builder();
    builder.env("PATH", build_path());
    builder.env("TERM", "xterm-256color");
    inject_openclaw_env(&mut builder);
    inject_pip_mirror(&mut builder);
    inject_no_proxy_loopback(&mut builder);
    builder.cwd(&resolved_cwd);

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|e| format!("启动 shell 失败: {e}"))?;
    // Child 本体交给 waiter 线程去阻塞 wait()；会话表里只留这个 killer 供 term_close 用
    let killer = child.clone_killer();
    // waiter 线程要在会话收尾后往前端发「已退出」，得有自己的一份 Channel
    let on_data_for_waiter = on_data.clone();
    // slave 端关掉，否则 reader 永远等不到 EOF
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader 失败: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take writer 失败: {e}"))?;

    let id = next_id();

    // writer 线程：前端来的输入先进队列，由这个线程慢慢往 PTY 里灌。
    //
    // ★ 为什么必须排队、而不是在命令里直接写（这是「粘一大段就整窗卡死」的根因）：
    // 往 PTY 写是**阻塞**的 —— 实测对面不读时吞吐只有 ~140KB/s，粘 256KB 卡 1.8 秒、
    // 粘 1MB 卡 7.3 秒。而 `term_write` 是同步 `#[tauri::command]`，同步命令跑在 **UI 主线程**
    //（见 fetch_online_feed 那次 24 秒冻结的教训），当时还全程握着 `sessions()` 全局锁 ——
    // 于是一个终端写卡住，其它所有终端连同整个界面一起僵住。
    //
    // 用 std 无界 mpsc：`send` 永不阻塞，`term_write` 退化成纯入队（微秒级）；FIFO 天然保证
    // **按键顺序**。这也是没改成 `async fn` + `spawn_blocking` 的原因 —— 那样两次快速按键会被
    // 线程池换序，打出来的字符会乱，是比原 bug 更糟的新 bug。
    let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut writer = writer;
        // 队列断开（PtySession 被 drop）时循环自然结束，writer 随之释放。
        for chunk in writer_rx {
            if writer.write_all(&chunk).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    // reader 线程：PTY 输出 → Channel（零 unwrap，panic=abort 安全）
    let reader_id = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // master 被 waiter/term_close 摘掉后走到这儿
                Ok(n) => {
                    if on_data.send(buf[..n].to_vec()).is_err() {
                        // 前端没了（窗口关闭 / webview 刷新）。shell 可能还活着，而且再没人
                        // 看得到、也没人关得掉 —— 收掉它，别留野进程。
                        take_session(&reader_id, true);
                        return;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // ★ waiter 线程：阻塞等子进程退出，然后收尾 + 通知前端。
    //
    // **不能指望 reader 读到 EOF 来发现进程退出** —— 这是 Windows 上实测栽过的坑：ConPTY 只要
    // master 句柄还在（我们存在 sessions() 里），子进程退出并不会让 reader 收到 EOF，它会一直
    // 干等着。`headless_run` 里之所以要显式 `drop(pair.master)` 才等得到 reader 收尾，就是这个
    // 原因。第一版把收尾写在 reader 里，`--term-session-test` 当场报
    // 「exit 之后没收到空帧 / 会话没自清」，就是它。
    //
    // 所以改成谁最有资格谁来判：waiter 拿着 Child 阻塞 `wait()`，进程一退就摘会话
    //（drop 掉 master → reader 随之收到 EOF 退出、writer_tx 断开 → writer 线程退出），
    // 再把「已退出」告诉前端。session 里只留 clone_killer 出来的 killer 供 term_close 用。
    let waiter_id = id.clone();
    let on_data_w = on_data_for_waiter;
    std::thread::spawn(move || {
        let mut child = child;
        let code = match child.wait() {
            Ok(st) => format!("（退出码 {}）", st.exit_code()),
            Err(_) => String::new(),
        };
        // 给 reader 一点时间把进程最后吐的那几行读完再拆 master，免得尾部输出被截掉
        std::thread::sleep(std::time::Duration::from_millis(150));
        take_session(&waiter_id, false);
        // ★ 告诉前端「这个终端死了」。以前这里什么都不发，前端于是毫无感知：用户敲了 exit、
        // 或者 claude/hermes 自己崩了之后，标签绿点照亮、敲什么都没反应（term_write 的错误被
        // `.catch(() => {})` 吞掉），只能自己关掉重开。
        // 先发一行看得见的说明，再发一个**空帧**当 EOF 哨兵 —— 真实输出永远不会是 0 字节
        //（读到 0 我们是 break 不发送的），所以空帧不会跟输出混淆。
        let _ = on_data_w.send(format!("\r\n\x1b[90m[进程已退出{code}]\x1b[0m\r\n").into_bytes());
        let _ = on_data_w.send(Vec::new());
    });

    let mut killer = killer;
    match sessions().lock() {
        Ok(mut map) => {
            map.insert(
                id.clone(),
                PtySession {
                    master: pair.master,
                    writer_tx: writer_tx.clone(),
                    killer,
                    tool,
                    cwd: Some(resolved_cwd),
                    cmd: initial_cmd.clone(),
                    last_seen: Instant::now(),
                },
            );
        }
        Err(_) => {
            // 不能把已经分配的 PTY 藏在表外：升级闸只承认「已入表」的资源。
            let _ = killer.kill();
            return Err("终端会话锁异常".into());
        }
    }
    // 会话一出生就挂上 watchdog（幂等，只起一次）：没有它，前端渲染进程崩了之后
    // 空闲 shell 会永远泄漏 —— reader 线程阻塞在 read() 上，send 失败清理根本走不到。
    spawn_watchdog();

    // 待运行命令（白名单校验：放宽到带参命令，挡 shell 注入，见 validate_cmd）
    if let Some(cmd) = initial_cmd {
        let mut line = cmd.into_bytes();
        line.extend_from_slice(b"\r\n");
        let _ = writer_tx.send(line);
    }

    schedule_snapshot_write();

    Ok(id)
}

/// 前端 xterm.js 需要知道「对面的伪终端是什么」才能正确处理**行折行**。
///
/// ## 不给它会怎样（这不是可选优化，是 Windows 下的必配项）
/// ConPTY 到行尾时自己插一个换行、且**不打 wrapped 标记**；xterm.js 不知道对面是 ConPTY，
/// 就按自己那套再折一次 → 同一段文字前端占的行数比应用以为的多。TUI 应用（Claude Code
/// 的多行输入框就是典型）每次按键都发「光标上移 N 行、清掉、重画」，N 是**它算的**行数，
/// 清不到前端多出来的那几行 → 旧的那份留在屏上，新的画在下面 → **输入越长、重复越多**。
/// 现象就是客户说的「老是重复」。xterm.js 官方为此专门开了 `windowsPty` 选项，我们一直没传。
///
/// buildNumber 必须给真值：xterm.js 用它区分 21376 之前/之后两套 ConPTY 折行行为，
/// 猜错等于换一种错法。非 Windows 返回 `None`，前端就不传这个选项（macOS 不需要）。
#[tauri::command]
pub fn term_pty_info() -> PtyInfo {
    PtyInfo {
        backend: if cfg!(windows) { Some("conpty".into()) } else { None },
        build_number: windows_build_number(),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyInfo {
    /// `"conpty"`（Windows）或 `None`（其它平台，前端不传 windowsPty）
    pub backend: Option<String>,
    /// Windows 内部版本号（如 22631）；探不到或非 Windows 为 `None`
    pub build_number: Option<u32>,
}

/// Windows 内部版本号。**探不到就 `None`** —— 不许拿一个猜的数字冒充，那会让 xterm.js
/// 选错折行分支，比不传更糟（同 envfp 里「探不到不等于版本旧」那条）。
/// 实现在公共层 `installer::windows_build_number()`，这里只是转调（同一事实一份实现）。
fn windows_build_number() -> Option<u32> {
    crate::installer::windows_build_number()
}

/// 键盘输入 / 粘贴 → PTY stdin。
///
/// **只入队，不落笔**：真正的写在本会话的 writer 线程里做（见 `term_open_pty`）。
/// 这个命令是同步的，也就是跑在 UI 主线程上 —— 所以它内部一个阻塞操作都不能有：
/// 全局锁只用来取一下队列句柄（微秒级），`Sender::send` 是无界的、永不阻塞。
/// 粘一大段的耗时因此全部落在后台线程，界面不再冻结。
#[tauri::command]
pub fn term_write(session_id: String, data: String) -> Result<(), String> {
    let tx = {
        let mut map = sessions().lock().map_err(|_| "终端会话锁异常")?;
        let s = map.get_mut(&session_id).ok_or("会话不存在")?;
        s.last_seen = Instant::now(); // 有交互 = 前端还活着
        s.writer_tx.clone()
    };
    // 队列断了 = writer 线程已收尾 = 这个终端已经死了（reader 那边同时会发 EOF 哨兵）
    tx.send(data.into_bytes()).map_err(|_| "终端已退出".to_string())
}

/// 终端尺寸变化。
#[tauri::command]
pub fn term_resize(session_id: String, cols: u16, rows: u16) -> Result<(), String> {
    let mut map = sessions().lock().map_err(|_| "终端会话锁异常")?;
    let s = map.get_mut(&session_id).ok_or("会话不存在")?;
    s.last_seen = Instant::now();
    s.master
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize 失败: {e}"))
}

/// 关闭会话（杀进程 + 释放）。
#[tauri::command]
pub fn term_close(session_id: String) -> Result<(), String> {
    // kill=true：主动关就是要它死。之后 waiter 的 wait() 返回，发现会话已被摘走就只发一条
    // 「已退出」通知（前端那边标签已经关了，会忽略），线程随即收尾。
    take_session(&session_id, true);
    Ok(())
}

/// 前端心跳：webview 还活着就定期调一次（main.tsx 全局 setInterval，20s）。
/// 无参刷新全部会话 —— 语义是「webview 存活」，而不是「某个终端在被使用」：
/// 用户开着终端跑 10 分钟长命令时心跳照发，会话不会被误回收；
/// 只有渲染进程死了（JS 停摆）心跳才会断，watchdog 才有资格收尸。
/// `alive` = 前端**还在用**的会话 id（见 src/opencodex/term/registry.ts）。
///
/// ★ 为什么心跳必须带归属：老版本不带参数，于是这里把会话表**整张**刷成 now。后果是
/// WebView2 崩溃/刷新后遗留的孤儿会话（前端 Channel 早没人接了）会被新前端的心跳一起
/// 续命，`HEARTBEAT_TIMEOUT` 那条回收路径**永远不可能触发** —— 只要还有任何一个前端活着，
/// 孤儿就是永生的。2026-08-06「实机 35 个泄漏会话」就是这么来的。
///
/// 当时没去修归属，而是加了个按会话总数杀人的 MAX_SESSIONS 兜底，结果误伤正常终端
///（详见 reap_stale 的注释）。带上 alive 之后，没人认领的会话自然老死，兜底已删除。
///
/// `alive` 为 None 时退回全表刷新，保持与旧前端的兼容（宁可漏，不可误杀）。
#[tauri::command]
pub fn term_ping(alive: Option<Vec<String>>) -> Result<(), String> {
    if let Ok(mut map) = sessions().lock() {
        let now = Instant::now();
        match alive {
            Some(ids) => {
                for (id, s) in map.iter_mut() {
                    if ids.iter().any(|x| x == id) {
                        s.last_seen = now;
                    }
                }
            }
            None => {
                for s in map.values_mut() {
                    s.last_seen = now;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use std::sync::Mutex;

    // UPDATING / OPENING_BLOCKED / IN_FLIGHT_OPEN 是进程级状态；默认 cargo test 会并行跑，
    // 所有触碰升级闸的测试必须串行，免得一个测试把另一个的前置条件改掉。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn updating_begin_end_is_idempotent_and_reusable() {
        let _test_lock = TEST_LOCK.lock().unwrap();
        // 这个 static 属于整个进程；先清理，避免未来新增测试留下状态时互相污染。
        term_updating_end();
        assert!(term_updating_begin(), "首次 begin 必须抢到冻结");
        assert!(!term_updating_begin(), "重复 begin 不得重复置位");
        term_updating_end();
        term_updating_end(); // end 是幂等的
        assert!(term_updating_begin(), "begin → end 后必须可以复用");
        term_updating_end();
    }

    #[test]
    fn updating_gate_rejects_pty_before_any_allocation() {
        let _test_lock = TEST_LOCK.lock().unwrap();
        term_updating_end();
        assert!(term_updating_begin());
        let ch: Channel<Vec<u8>> = Channel::new(|_| Ok(()));
        let err = tauri::async_runtime::block_on(term_open_pty(
            80,
            24,
            ch,
            None,
            None,
            None,
        ))
        .expect_err("升级冻结期不允许分配新的 PTY");
        assert_eq!(err, UPDATING_REJECTION);
        term_updating_end();
    }

    #[test]
    fn updating_waits_until_already_admitted_open_is_accounted_for() {
        let _test_lock = TEST_LOCK.lock().unwrap();
        term_updating_end();
        let opening = InFlightOpenGuard::begin().expect("未冻结时应能取得 open 票据");
        let begin = std::thread::spawn(term_updating_begin);
        std::thread::sleep(Duration::from_millis(25));
        assert!(!UPDATING.load(Ordering::SeqCst), "在飞 open 未清零前不得置 UPDATING");
        drop(opening);
        assert!(begin.join().expect("升级线程不应 panic"));
        assert!(UPDATING.load(Ordering::SeqCst));
        term_updating_end();
    }

    #[test]
    fn snapshot_round_trip_and_shape() {
        let path = std::env::temp_dir().join(format!(
            "uking-term-snapshot-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let expected = SnapshotInfo {
            kind: "upgrade".into(),
            sessions: vec![SnapshotSession {
                cwd: Some("C:/work".into()),
                cmd: Some("codex".into()),
                tool: Some("codex".into()),
                resume_hint: None,
            }],
        };
        write_snapshot_file(&path, &expected).expect("测试快照应可写入");
        assert_eq!(read_snapshot_file(&path), Some(expected));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn corrupt_or_empty_snapshot_is_never_pending() {
        let path = std::env::temp_dir().join(format!(
            "uking-term-snapshot-empty-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"not json").expect("测试文件应可写入");
        assert_eq!(read_snapshot_file(&path), None);

        write_snapshot_file(&path, &SnapshotInfo { kind: "upgrade".into(), sessions: vec![] }).expect("空快照应可写入");
        assert!(read_snapshot_file(&path).is_some());
        assert!(read_snapshot_file(&path).filter(|snapshot| !snapshot.sessions.is_empty()).is_none());

        assert!(!snapshot_sessions(&path));
        assert!(path.exists(), "本次未写快照时不得删除上次留下的恢复卡");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn snapshot_age_boundary_is_inclusive_at_seven_days() {
        assert!(snapshot_is_fresh(SNAPSHOT_MAX_AGE));
        assert!(!snapshot_is_fresh(SNAPSHOT_MAX_AGE + Duration::from_secs(1)));
    }

    #[test]
    fn live_snapshot_does_not_hide_upgrade_pending_card() {
        let dir = std::env::temp_dir().join(format!(
            "uking-term-snapshot-kinds-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let upgrade_path = dir.join("term-snapshot.json");
        let live_path = dir.join("term-sessions-live.json");
        let upgrade = SnapshotInfo {
            kind: "upgrade".into(),
            sessions: vec![SnapshotSession {
                cwd: Some("C:/work".into()),
                cmd: Some("codex".into()),
                tool: Some("codex".into()),
                resume_hint: None,
            }],
        };
        write_snapshot_file(&upgrade_path, &upgrade).expect("升级快照应可写入");
        write_snapshot_file(&live_path, &SnapshotInfo { kind: "live".into(), sessions: vec![] })
            .expect("实时快照应可写入");

        assert_eq!(term_snapshot_pending_from_path(&upgrade_path), Some(upgrade));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn extracts_resume_hint_from_supported_flags() {
        assert_eq!(
            resume_hint_from_command(Some("claude"), Some("claude --resume 01HZX")),
            Some("01HZX".into())
        );
        assert_eq!(
            resume_hint_from_command(Some("codex"), Some("codex -r thread-42 --model gpt-5")),
            Some("thread-42".into())
        );
        assert_eq!(
            resume_hint_from_command(Some("claude"), Some("claude --resume=01HZX")),
            Some("01HZX".into())
        );
        assert_eq!(
            resume_hint_from_command(Some("codex"), Some("codex --resume --model gpt-5")),
            None
        );
        assert_eq!(
            resume_hint_from_command(Some("pnpm"), Some("pnpm -r build")),
            None
        );
        assert_eq!(
            resume_hint_from_command(Some("codex"), Some("codex --model gpt-5")),
            None
        );
    }
}

/// 幂等启动后台回收线程（只起一次）。
fn spawn_watchdog() {
    static STARTED: OnceLock<()> = OnceLock::new();
    let _ = STARTED.get_or_init(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(WATCHDOG_INTERVAL);
            reap_stale();
        });
    });
}

/// 回收**没人认领**的会话：`last_seen` 超过 HEARTBEAT_TIMEOUT 没被刷新，就说明再没有
/// 任何前端把它报进 `term_ping` 的 alive 列表 —— 要么渲染进程死了，要么那个标签早就关了
/// 却漏掉了 `term_close`。这两种都是真孤儿，杀掉是对的。
///
/// ★ 2026-08-15 删掉了这里的第二条规则「会话数超过 MAX_SESSIONS(16) 就回收最旧的」。
/// 它有两个错，叠起来正好是「用户刚开的终端秒退、退出码 1」：
///
///  1. **判据错**：拿会话总数近似泄漏。用户合法开 17 个标签，跟漏了 17 个孤儿，计数上
///     一模一样，于是正常使用被当成泄漏处理。
///  2. **实现错**：它按 `last_seen` 排序取「最旧」，但 `last_seen` 根本不是会话年龄 ——
///     老版 `term_ping` 每 20s 把全表刷成同一个 Instant，排序键**全部相等**。
///     `sort_by_key` 是稳定排序，键相等时顺序 = HashMap 的迭代顺序，而 Rust 默认
///     HashMap 用随机种子。所谓「回收最旧的」实际是**随机挑一个活着的会话杀掉**。
///
/// 而 portable-pty 在 Windows 上 kill 走的是 `TerminateProcess(handle, 1)`，退出码恰好是
/// 1 —— 用户看到的就是 pwsh 连同里面正在跑的 Claude Code 一起消失，屏幕上只留一行
/// 「[进程已退出（退出码 1）]」，没有任何报错可查（因为根本不是它自己退的）。
///
/// 归属判据（谁还在 ping 它）是准确的，有了它就不需要再按数量去猜。终端想开多少开多少。
///
/// 锁纪律：先持锁收集要杀的 id，**释放锁后**再逐个 take_session —— std Mutex 不可重入，
/// take_session 内部自己会 lock，持锁调用就是自锁死。
fn reap_stale() {
    let mut to_kill: Vec<String> = Vec::new();
    if let Ok(map) = sessions().lock() {
        let now = Instant::now();
        for (id, s) in map.iter() {
            if now.duration_since(s.last_seen) > HEARTBEAT_TIMEOUT {
                to_kill.push(id.clone());
            }
        }
    }
    for id in to_kill {
        take_session(&id, true);
    }
}

/// 应用退出前的总清扫：把还活着的 PTY 会话全部杀掉，避免主进程退出后
/// 残留孤儿 pwsh（Windows 不会随父进程自动回收子进程）。挂在 RunEvent::Exit 上。
pub fn cleanup_all() {
    let ids: Vec<String> = sessions()
        .lock()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    for id in ids {
        take_session(&id, true);
    }
}

/// 一个运行中的工具实例（运行面板用）。
#[derive(serde::Serialize)]
pub struct RunningTool {
    pub tool: String,
    pub session_id: String,
}

/// 列出当前正在运行的「带工具 tag」的 PTY 会话（运行面板据此显示绿点 + 停止按钮）。
///
/// 这里**不再兼职回收死会话** —— 回收已经挂到 waiter 线程上（进程一退就摘，见 `take_session`）。
/// 原来把回收写在这个函数里是个隐患：它唯一的调用方在 0.8.6 砍 AuxBar 之后就没人挂载了，
/// 于是回收也跟着没人触发。现在表里剩下的都是活的，直接列即可。
#[tauri::command]
pub fn list_running() -> Vec<RunningTool> {
    let mut out = Vec::new();
    if let Ok(map) = sessions().lock() {
        for (id, s) in map.iter() {
            if let Some(tool) = &s.tool {
                out.push(RunningTool {
                    tool: tool.clone(),
                    session_id: id.clone(),
                });
            }
        }
    }
    out
}

/// 本地端口是否已有服务在监听 —— OpenClaw gateway WebUI「起没起」的判据。
/// 连一下 127.0.0.1:port，连得上即认为 gateway 已在听。
pub fn port_listening(port: u16) -> bool {
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        std::time::Duration::from_millis(250),
    )
    .is_ok()
}

/// 轮询等端口就绪，最多 `timeout_ms`。就绪即返回 true，超时返回 false。
/// 给 OpenClaw 网页版用：起 gateway 后调它，端口一通就开网页（取代固定 6s 延时，
/// 既不过早白屏也不干等）。
#[tauri::command]
pub async fn wait_port(port: u16, timeout_ms: u64) -> bool {
    let cap = timeout_ms.min(60_000);
    tauri::async_runtime::spawn_blocking(move || {
        let start = std::time::Instant::now();
        loop {
            if port_listening(port) {
                return true;
            }
            if start.elapsed().as_millis() >= cap as u128 {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    })
    .await
    .unwrap_or(false)
}

/// OpenClaw 网页版固定 token —— 起 gateway 前写进我们自己的 openclaw home，
/// 让 `--allow-unconfigured` 用这个确定值而非随机生成，网页 URL 的 #token 才对得上。
const OPENCLAW_TOKEN: &str = "uclaw";

/// 起 gateway **之前**调用：确保我们自己的 openclaw home 存在，且 gateway token = 已知值。
///
/// 根因修复：`openclaw gateway run --allow-unconfigured`（OPENCLAW_HOME=~/.uking/openclaw）
/// 在没有配置时会**随机生成** gateway token 写进 `~/.uking/openclaw/openclaw.json`，
/// 而前端历史死写 `#token=uclaw` → token 对不上 → 网页「认证不匹配」打不开。
/// 这里先把 token 钉成 uclaw（合并写入，保留 openclaw.json 其它字段），让起出来的 gateway
/// 用确定 token。只在端口空闲、即将全新启动时调用（此刻无 live gateway 持旧 token，覆盖安全）。
#[tauri::command]
pub fn prepare_openclaw_home() -> Result<(), String> {
    let home = openclaw_home();
    std::fs::create_dir_all(&home).map_err(|e| format!("创建 openclaw 目录失败: {e}"))?;
    let path = std::path::Path::new(&home).join("openclaw.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = root.as_object_mut().unwrap();
    let gw = obj
        .entry("gateway")
        .or_insert_with(|| serde_json::json!({ "mode": "local" }));
    if !gw.is_object() {
        *gw = serde_json::json!({ "mode": "local" });
    }
    let gwo = gw.as_object_mut().unwrap();
    gwo.entry("mode").or_insert_with(|| serde_json::json!("local"));
    let auth = gwo.entry("auth").or_insert_with(|| serde_json::json!({}));
    if !auth.is_object() {
        *auth = serde_json::json!({});
    }
    auth.as_object_mut()
        .unwrap()
        .insert("token".into(), serde_json::json!(OPENCLAW_TOKEN));
    let text = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化配置失败: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("写 openclaw.json 失败: {e}"))
}

/// 读我们自己 openclaw home 里 gateway 的**真实** token（读不到回退 uclaw）。
fn read_openclaw_token() -> String {
    let path = std::path::Path::new(&openclaw_home()).join("openclaw.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("gateway")?
                .get("auth")?
                .get("token")?
                .as_str()
                .map(str::to_string)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| OPENCLAW_TOKEN.to_string())
}

/// OpenClaw 网页版 URL —— 用 gateway **真实** token 拼（不再前端死写 uclaw）。
/// gateway 起好后调它：无论 openclaw 实际用了哪个 token（uclaw 或它自己生成的），都对得上。
#[tauri::command]
pub fn openclaw_webui_url() -> String {
    format!("http://127.0.0.1:18789/#token={}", read_openclaw_token())
}

#[cfg(test)]
mod tests {
    use super::{term_open_pty, validate_cmd};
    use tauri::ipc::Channel;

    #[test]
    fn allows_plain_and_parametered() {
        assert!(validate_cmd("claude"));
        assert!(validate_cmd("claude --resume"));
        assert!(validate_cmd("codex --model gpt-5.3-codex"));
        assert!(validate_cmd("codex --no-alt-screen"));
        assert!(validate_cmd("openclaw gateway run --port 18789"));
        assert!(validate_cmd("dsh web"));
        assert!(validate_cmd("dsh --profile headless --help"));
        assert!(validate_cmd("harness-doctor --target all --no-ports"));
        assert!(validate_cmd("npm install -g openclaw"));
        assert!(validate_cmd("pi --tools read"));
        assert!(validate_cmd("opencode --continue"));
    }

    #[test]
    fn rejects_injection_and_unknown_programs() {
        assert!(!validate_cmd(""));
        assert!(!validate_cmd("rm -rf /"));
        assert!(!validate_cmd("claude; rm -rf x")); // ';' 不在字符集
        assert!(!validate_cmd("claude && evil"));
        assert!(!validate_cmd("claude | tee out"));
        assert!(!validate_cmd("git ../escape")); // 路径穿越
        assert!(!validate_cmd("claude ..\\x")); // Windows 风格路径穿越
        assert!(!validate_cmd("powershell -c whoami")); // 程序名不在白名单
    }

    /// 非 ASCII 放行：中文提示词能通过，但 ASCII 层面的 shell 元字符依旧全部被挡。
    #[test]
    fn allows_non_ascii_but_still_blocks_ascii_metacharacters() {
        let cases: &[(&str, bool)] = &[
            ("cline 说说你能做什么", true),
            ("cline history", true),
            ("cline & calc", false),      // '&' 是 ASCII 元字符
            ("cline %PATH%", false),      // '%' 变量展开
            ("cline <evil>", false),
            ("cline `whoami`", false),
            ("cline \"quoted\"", false),
            ("cline $(whoami)", false),
        ];
        for (cmd, expected) in cases {
            assert_eq!(validate_cmd(cmd), *expected, "cmd={cmd:?}");
        }
    }

    #[test]
    fn rejects_over_length_command() {
        let long_cmd = format!("claude {}", "a".repeat(600));
        assert!(long_cmd.len() > 512);
        assert!(!validate_cmd(&long_cmd));
    }

    #[test]
    fn term_open_pty_rejects_invalid_initial_cmd() {
        // 无头场景下不需要 webview，Channel::new 直接可用（见 headless_session_test 的用法）。
        let ch: Channel<Vec<u8>> = Channel::new(|_| Ok(()));
        let result = tauri::async_runtime::block_on(term_open_pty(
            80,
            24,
            ch,
            Some("rm -rf /".into()),
            None,
            None,
        ));
        assert!(result.is_err(), "非法初始命令应被拒绝，而不是静默忽略并开出空终端");
    }
}
