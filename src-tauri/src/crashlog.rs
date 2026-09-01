//! 崩溃取证 —— 「客户说老是崩溃」时，机器上得留下能回答问题的东西。
//!
//! ## 为什么加这个模块（2026-07-30，客户机 pc-*** 实锤）
//!
//! 客户报「运行老是崩溃」，远程连上去查了一圈：Windows 事件日志 0 条、`CrashDumps` 空、
//! 杀软隔离区没有、`action conformance` 36 项全绿。**什么都没有，等于什么都没查到** ——
//! 既不能说它崩过，也不能说它没崩过。翻代码才发现根因：
//!
//! - `report::install_panic_hook` 崩溃时**只发网络**（curl POST），**一个字节都不落盘**。
//!   客户开着代理 / 断网 / GFW 拦一下，这条线索就彻底蒸发。
//! - 前端 `ErrorBoundary` 和 `window.onerror` 同理，全是 `invoke("report_bug")` 走网络。
//! - **最要命的是：完全没有「这次运行是怎么结束的」的记录。** 进程被 360 静默杀掉、被
//!   任务管理器结束、崩了自动重启 —— 这三种在事后取证上长得**一模一样**（都不留事件日志），
//!   而它们恰恰是「老是崩溃」最常见的真身。
//!
//! 所以本模块解决的不是「上报」，是**留痕**：先落盘，再谈上报。
//!
//! ## 三件事
//!
//! 1. **崩溃落盘**（[`record`]）：panic / UI 渲染崩溃 / 未处理 Promise 一律先写
//!    `~/.uking/logs/crash.log`。网络失败与否都不影响留痕。
//! 2. **异常退出检测**（[`begin_session`] / [`end_session`]）：启动时写一份会话标记并每 30 秒
//!    续期，正常退出时删掉。下次启动发现标记还在 = **上次没有正常退出**，且末次心跳时间就是
//!    「大约死在什么时候」，减去启动时间就是「活了多久」。这正是 pc-*** 上我最想要却没有的
//!    那条数据。
//! 3. **可远程查询**（[`inspect`]）：接进影核动作表 `runtime.crash.inspect`，一条命令出结论，
//!    不必再靠人肉翻事件日志。同时因为写的是 `logs/*.log`，`diagnostics.collect` 扫目录
//!    自动就带上了，不用回头改 feedback.rs。
//!
//! ## 诚实边界（别夸大）
//!
//! 「没有正常退出」**不等于**「崩溃」：关机、注销、任务管理器结束进程都会留下同样的标记。
//! 我们分不出来，所以就**如实叫「异常退出」并把时长交出去**，让看的人自己判断 ——
//! 跑了 3 小时的异常退出多半是关机，跑了 12 秒的连着好几条才是崩溃循环。
//! 宁可给出可判读的原始事实，也不编一个「崩溃次数」去糊弄。
//!
//! ## 🔴 一实例一份标记（2026-08-19 修，起因是这套账本自己在说谎）
//!
//! 原来全机器只有**一份** `.session.json`。而 U-King 没有单实例锁，多开是常态 ——
//! 于是第二个实例启动时读到第一个实例的标记，认不出「这是别人、不是我的上一条命」，
//! 张口就记一笔「上次没有正常退出」。实测账本里 34 条 `unclean_exit` **只有 14 条是真的**：
//! 同一个 `prev_pid` 被反复上报，`lived_secs` 一路涨到 116433 秒（32 小时）—— 那个进程
//! 压根没死，一直活着在写心跳。
//!
//! 后果比「数字不准」严重得多：有人拿 `crash.log` 的行数比较版本稳定性，得出「0.9.83 更稳」，
//! 而那比的其实是**「那段时间你多开了几次」**。一个会说反话的账本比没有账本更坏 ——
//! 这正是本模块开头骂过的毛病，由本模块自己犯了一遍。
//!
//! 光加一道「pid 还在不在」的校验**不够**，因为两个实例仍在抢同一个文件：A 正常退出会删掉
//! B 写的那份，之后 B 崩溃就彻底无痕 —— 那是把「重复多记」换成「静默漏记」，方向反了。
//! 所以改成**每个实例写自己的 `.session-<pid>.json`**：谁也不覆盖谁，各删各的；
//! 启动时扫全部，逐个问「这个 pid 现在还是不是一个活着的我自己」——
//! 是就跳过（那是并行实例），不是（或**探不出来**）才结账。失败方向仍然指向多记。
//!
//! ## 诚实边界（别夸大）
//!
//! 设计约束（对齐本项目模块独立铁律）：纯函数、零 `AppHandle`，`#[tauri::command]` 全在
//! `lib.rs` 转调；依赖方向只有「本模块 → ulog / report / installer 公共层」，
//! 不 import 任何**业务**模块。
//!
//! （`installer::pid_image_name` 是 2026-08-19 为上面那件事下沉过去的公共探测能力。
//! 本来 `agent/chat.rs` 里已有一份 `pid_alive`，但那是业务模块 —— 横向 import 会把
//! 「删掉 agent 只该动 2 个文件」这条铁律咬坏，所以按「公共能力复用不复制」下沉，
//! 而不是在这里抄第二份。）

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 日志模块名 → `~/.uking/logs/crash.log`。诊断采集扫目录，写这儿就自动被带上。
const MODULE: &str = "crash";

/// 结构化事件账本。刻意**不叫 `.log`** —— `ulog::all_tails` 只扫 `*.log`，
/// 免得诊断正文里塞进一份没人读的 JSON。
const HISTORY_FILE: &str = ".crash-history.json";

/// 本实例的会话标记文件名前缀 —— 完整形如 `.session-12345.json`。
///
/// **带 pid 是这套账本能不能信的关键**，理由见模块头「一实例一份标记」。
const SESSION_PREFIX: &str = ".session-";
const SESSION_SUFFIX: &str = ".json";

/// 0.9.x ~ 1.0.2 用的单文件标记。**只在启动时读一次做结账，之后永不再写。**
///
/// 留着这段迁移是因为：升级上来的机器盘上就躺着一份，直接无视等于把用户最后一次
/// 异常退出的证据丢掉 —— 而那可能正是他来找你的原因。
const LEGACY_SESSION_FILE: &str = ".session.json";

/// 账本最多留多少条 —— 够看出「是不是反复崩」，又不会把客户磁盘吃掉。
const MAX_EVENTS: usize = 40;

/// 心跳间隔。30 秒 = 事后能把死亡时间定位到半分钟内，代价是每 30 秒写 ~100 字节。
const HEARTBEAT_SECS: u64 = 30;

/// **首拍**心跳的延迟，比常规间隔短得多。
///
/// 实测逼出来的（本机真跑：启动 8 秒后强杀，重启读到「跑了约 0 秒」）：`lived_secs` 是
/// 「末次心跳 - 启动」，所以死在首拍之前的运行一律读成 0 —— 而**启动即崩恰恰全落在这个区间**，
/// 正是最该看清的那一类，却偏偏一个数字都给不出。首拍提前到 3 秒，把 0~30 秒这段填上。
const FIRST_BEAT_SECS: u64 = 3;

/// 上次运行活了多久算「短命」。短命的异常退出 = 启动即崩 / 崩溃循环，是真信号，
/// 值得占用一次自动上报；跑了很久的多半是关机，只留本地不打扰服务端。
const SHORT_RUN_SECS: u64 = 120;

/// 心跳线程只该有一个（`begin_session` 万一被调两次也不重复起线程）。
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 上次运行没有正常收尾时，[`begin_session`] 交出来的事实。
///
/// 刻意只给**事实**（活了多久、日志尾巴）和一个**判据**（像不像崩溃循环），不给结论：
/// 「异常退出」这件事我们分不出关机 / 被杀 / 崩溃，编一个结论只会误导下一个排障的人。
pub struct UncleanExit {
    /// 上次从启动到**末次心跳**活了多少秒。这是**下界**不是精确值：心跳之后到真正死掉
    /// 那段没人记录（首拍 3 秒、之后 30 秒一拍）。判「像不像崩溃循环」够用了。
    pub lived_secs: u64,
    /// 短命 = 值得自动上报。跑得久的多半是关机，只留本地免得淹了 issue 区。
    pub looks_like_crash_loop: bool,
    /// 一句话摘要（进 Issue 标题）。
    pub summary: String,
    /// crash.log 的尾巴（进 Issue 正文）。
    pub log_tail: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn read_json(p: &std::path::Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// 写 JSON。**永不 panic、永不返回错误** —— 取证失败绝不能反过来弄崩业务。
///
/// **原子写**（临时文件 + rename）。整份回写的账本有个很坏的失败模式：写到一半进程没了
/// （本项目 `panic=abort`，崩溃路径上这是常态），盘上剩半截 JSON，下次 `read_json` 解析失败
/// → `unwrap_or_default()` 把**整份历史静默清零**。而这个模块存在的全部理由就是「客户说老是
/// 崩溃时，机器上得留下东西」——恰恰在崩得最凶的时候把证据清掉，等于没有这个模块。
fn write_json(p: &std::path::Path, v: &serde_json::Value) {
    let Some(parent) = p.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(v) else { return };
    // 同目录的临时文件：跨卷 rename 会失败，`.tmp` 跟目标放一起才保证是同一个卷。
    // 后缀不是 `.log`，`ulog::all_tails` 扫不到它，不会漏进诊断正文。
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, &bytes).is_ok() {
        // Windows 上 rename 一样会覆盖已存在的目标（MoveFileEx + MOVEFILE_REPLACE_EXISTING），
        // 不必先删 —— 先删反而开了一个「删完还没写就断电」的空窗。
        if std::fs::rename(&tmp, p).is_ok() {
            return;
        }
    }
    let _ = std::fs::remove_file(&tmp);
    // rename 失败（杀软占住 / 权限 / 跨卷）就退回直接写：宁可这一次不原子，也别丢这条事件。
    let _ = std::fs::write(p, &bytes);
}

/// 往账本里追加一条事件（同时写一行人类可读的 crash.log）。
///
/// 两处落盘由**这一个函数**发起：JSON 给机器断言，log 给人和 AI 读。
///
/// 🔴 **顺序是刻意的：先 JSON，后文本日志。** 原来反着写，而这两步之间随时可能 abort
/// （panic 路径上尤其如此）—— 2026-08-18 的黑盒测试在测试机上抓到了实据：`crash.log` 有一条
/// `[panic]`，`.crash-history.json` 里一条都没有，于是 `runtime.crash.inspect` 报
/// `crashes: 0`，**而磁盘上明明躺着一次崩溃**。这个模块的立项理由正是「客户报老是崩溃、
/// 查了一圈什么都没有」，它自己复现了那个症状。
///
/// 现在最坏情况反过来：账本有、日志少一行。失败方向必须指向「多记」而不是「少记」——
/// 少记会让人得出「没崩过」的**错误结论**，多记只是多看一眼。
/// 两边仍可能不一致（比如这次崩在中间），所以 [`inspect_in`] 还会交叉核对，不假设它们相等。
fn push_event(dir: &std::path::Path, kind: &str, summary: &str, extra: serde_json::Value) {
    let p = dir.join(HISTORY_FILE);
    let mut events: Vec<serde_json::Value> = read_json(&p)
        .and_then(|v| v.get("events").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let mut ev = serde_json::json!({
        "kind": kind,
        "summary": summary,
        "at": now(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    if let (Some(o), Some(x)) = (ev.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    events.push(ev);
    // 只留最近 MAX_EVENTS 条（老的先丢）。
    if events.len() > MAX_EVENTS {
        events.drain(..events.len() - MAX_EVENTS);
    }
    write_json(&p, &serde_json::json!({ "events": events }));
    crate::ulog::write_in(dir, MODULE, &format!("[{kind}] {summary}"));
}

/// 记一次崩溃/错误。**先落盘，再由调用方决定要不要上报** —— 顺序是刻意的：
/// 网络那步可能挂 15 秒、可能在 `panic=abort` 下压根没跑完，落盘则永远来得及。
///
/// `kind`：`panic` / `ui_crash` / `ui_error` / `ui_rejection` …
/// 调用方有责任不要把 Key / token 传进来（同 ulog：本模块按原文写盘，脱敏在上传口做）。
pub fn record(kind: &str, summary: &str, detail: &str) {
    record_in(&crate::ulog::log_dir(), kind, summary, detail);
}

/// 同 [`record`]，但指定取证目录 —— 给测试用：真跑落盘逻辑又不碰用户的 `~/.uking`。
pub fn record_in(dir: &std::path::Path, kind: &str, summary: &str, detail: &str) {
    push_event(dir, kind, summary, serde_json::json!({ "detail": head(detail, 1200) }));
}

/// 本进程那份标记的路径。
fn my_session_path(dir: &std::path::Path) -> PathBuf {
    dir.join(format!("{SESSION_PREFIX}{}{SESSION_SUFFIX}", std::process::id()))
}

/// 本进程的镜像名（`u-king-mini.exe` / 测试二进制名 …）。
fn my_image_name() -> Option<String> {
    Some(std::env::current_exe().ok()?.file_name()?.to_string_lossy().to_string())
}

/// 盘上除自己以外的所有会话标记 —— 每份都是「某个实例还没销账」的候选。
///
/// 顺带把老的单文件标记（[`LEGACY_SESSION_FILE`]）一起捞进来做迁移。它匹配不上
/// `.session-` 前缀（第 9 个字符是 `.` 不是 `-`），所以不会被下面的目录扫描重复捞一次。
fn stale_session_files(dir: &std::path::Path) -> Vec<(PathBuf, serde_json::Value)> {
    let mut out: Vec<(PathBuf, serde_json::Value)> = Vec::new();
    let legacy = dir.join(LEGACY_SESSION_FILE);
    if let Some(v) = read_json(&legacy) {
        out.push((legacy, v));
    }
    let me = my_session_path(dir);
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p == me {
            continue; // 自己这一轮的，还活着呢
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.starts_with(SESSION_PREFIX) || !name.ends_with(SESSION_SUFFIX) {
            continue;
        }
        if let Some(v) = read_json(&p) {
            out.push((p, v));
        }
    }
    out
}

/// 这份标记的主人是不是**另一个还活着的自己**（= 多开，不是异常退出）。
///
/// 两道都得过：pid 还在 **且** 镜像名跟自己一样。只判前者会栽在 pid 复用上 ——
/// 把一次真崩溃说成「那是并行实例」，恰好漏掉最该报的那一类。
///
/// 探不出来（`probe` 返回 `None`）一律判 `false` → 照旧记一笔异常退出。
/// **失败方向必须指向多记**：多记只是多看一眼，漏记会让人得出「没崩过」的错误结论。
fn is_live_sibling(pid: u32, probe: &dyn Fn(u32) -> Option<String>) -> bool {
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    match (my_image_name(), probe(pid)) {
        (Some(mine), Some(theirs)) => theirs.eq_ignore_ascii_case(&mine),
        _ => false,
    }
}

/// 把一份没销账的标记记成 `unclean_exit`。
fn record_unclean(dir: &std::path::Path, prev: &serde_json::Value) -> UncleanExit {
    let started = prev.get("started_at").and_then(|v| v.as_u64()).unwrap_or(0);
    let beat = prev.get("beat_at").and_then(|v| v.as_u64()).unwrap_or(started);
    let ver = prev.get("version").and_then(|v| v.as_str()).unwrap_or("?").to_string();
    let pid = prev.get("pid").and_then(|v| v.as_u64()).unwrap_or(0);
    let lived = beat.saturating_sub(started);
    // 说「至少」而不是「约」：`lived` = 末次心跳 - 启动，心跳之后到真正死掉那段没人记，
    // 所以它天生是**下界**。写成「约」会让人拿它当精确值去推断，宁可说得保守。
    let summary = format!("上次运行没有正常退出（至少跑了 {lived} 秒，末次心跳 pid={pid} 版本 {ver}）");

    push_event(
        dir,
        "unclean_exit",
        &summary,
        serde_json::json!({
            "prev_started_at": started,
            "prev_last_beat_at": beat,
            "lived_secs": lived,
            "prev_version": ver,
            "prev_pid": pid,
            // 我们分不出关机/被杀/崩溃，如实标注判据，别让读的人以为这是结论。
            "note": "没有正常退出：崩溃、被杀软或任务管理器结束、关机/注销都会这样。跑得越短越像崩溃。",
        }),
    );

    UncleanExit {
        lived_secs: lived,
        // 短命 = 启动即崩 / 崩溃循环，是真信号；跑了很久的多半是关机，不值得占一次上报。
        looks_like_crash_loop: lived <= SHORT_RUN_SECS,
        summary,
        log_tail: crate::ulog::tail_in(dir, MODULE, 4096).unwrap_or_default(),
    }
}

/// 给上一轮结账：扫出所有别人留下的标记，**逐个**判「主人还在不在」。
///
/// 抽成独立函数（而不是埋在 `begin_session` 里）是为了**能被真测**：
/// 「上次没干净退出认不认得出来」一个字节都不在动作表里，形状体检（conformance）盖不住它，
/// 只能靠喂一份陈旧标记进去、断言它被读成什么。
fn settle_previous(dir: &std::path::Path) -> Option<UncleanExit> {
    settle_previous_with(dir, &crate::installer::pid_image_name)
}

/// 同上，但存活探测可注入 —— 「多开不该记成崩溃」这条**只能**这么测：
/// 真去造一个活着的同名进程既慢又不可靠，而它恰恰是这次修复的全部要点。
fn settle_previous_with(dir: &std::path::Path, probe: &dyn Fn(u32) -> Option<String>) -> Option<UncleanExit> {
    let mut worst: Option<UncleanExit> = None;
    for (path, prev) in stale_session_files(dir) {
        let pid = prev.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if is_live_sibling(pid, probe) {
            // 另一个实例正跑着。**标记留着别删** —— 那是它的命，得由它自己销账。
            continue;
        }
        let ue = record_unclean(dir, &prev);
        let _ = std::fs::remove_file(&path);
        // 多份一起结账时，把**最短命**的那条交出去：它最像崩溃循环，最值得占一次上报。
        worst = match worst {
            Some(w) if w.lived_secs <= ue.lived_secs => Some(w),
            _ => Some(ue),
        };
    }
    worst
}

/// 落一份本次会话的标记（心跳也复用它，只是 `beat_at` 换成当前时刻）。
fn write_marker(dir: &std::path::Path, started: u64) {
    write_json(
        &my_session_path(dir),
        &serde_json::json!({
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
            "started_at": started,
            "beat_at": now(),
        }),
    );
}

/// 开一次会话：先把「上次是怎么结束的」结账，再落本次标记，最后起心跳线程。
///
/// **只该在 GUI 真的要起来时调一次**。所有无头模式（`--selfcheck` / `action run` /
/// `mcp serve` …）都在这之前就 `process::exit` 了，不会污染标记 —— 否则运维远程跑一条
/// `action run` 就会把客户正在跑的 GUI 会话标记覆盖掉，反而制造假崩溃。
///
/// 返回：上次异常退出的详情（`None` = 上次是正常退出，或这是第一次跑）。
/// **要不要为它上报由调用方决定** —— 本模块只管取证落盘，不认识网络，
/// 好让「删掉上报」和「删掉取证」互不牵连（模块独立铁律）。
pub fn begin_session() -> Option<UncleanExit> {
    if SESSION_ACTIVE.swap(true, Ordering::SeqCst) {
        return None;
    }
    let dir = crate::ulog::log_dir();
    let unclean = settle_previous(&dir);
    let started = now();
    write_marker(&dir, started);

    // —— 心跳 ——
    // 存在的意义：没有它，异常退出只知道「上次没退干净」，不知道死在什么时候、活了多久，
    // 而「活了多久」正是区分崩溃循环和关机的唯一依据。
    std::thread::spawn(move || {
        let mut wait = FIRST_BEAT_SECS;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            if !SESSION_ACTIVE.load(Ordering::SeqCst) {
                return;
            }
            write_marker(&crate::ulog::log_dir(), started);
            wait = HEARTBEAT_SECS;
        }
    });

    unclean
}

/// 正常收尾：删标记 + 留一行。**幂等**，多调几次没关系。
///
/// 漏调的代价是「多报一次异常退出」（假阳性），所以凡是有意的退出路径都该叫一下：
/// 托盘退出走 `RunEvent::Exit`，绕过 `prevent_close` 的 `process::exit`（卸载 / 自升级重启）
/// 得手动调 —— 否则每次自升级都会给自己记一笔假崩溃。
pub fn end_session() {
    if !SESSION_ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    // **只删自己那份。** 老版本删的是全机器共用的那一份 —— 多开时等于替还在跑的兄弟实例
    // 销了账，之后它真崩了盘上一点痕迹都没有（静默漏记，比多记坏得多）。
    let _ = std::fs::remove_file(my_session_path(&crate::ulog::log_dir()));
    crate::ulog::write(MODULE, "[exit] 正常退出");
}

/// 当前会话标记还在不在。`--crash-test` 的判据；也是「begin_session 到底跑没跑」的直接证据。
pub fn session_marker_exists() -> bool {
    my_session_path(&crate::ulog::log_dir()).exists()
}

/// 崩溃取证快照 —— 影核动作 `runtime.crash.inspect` 的实现。
///
/// 带 `ready` / `blockers`：日志目录写不进去时，这套取证等于没有 ——
/// 「报告是对的，世界是坏的」那类坑必须能被一眼看见，不能让空账本冒充「没崩过」。
pub fn inspect() -> serde_json::Value {
    inspect_in(&crate::ulog::log_dir())
}

/// 同 [`inspect`]，但指定取证目录（测试用）。
pub fn inspect_in(dir: &std::path::Path) -> serde_json::Value {
    let events: Vec<serde_json::Value> = read_json(&dir.join(HISTORY_FILE))
        .and_then(|v| v.get("events").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let count = |k: &str| events.iter().filter(|e| e.get("kind").and_then(|v| v.as_str()) == Some(k)).count();
    let in_ledger = count("panic") + count("ui_crash");
    let unclean = count("unclean_exit");

    // 交叉核对文本日志：**不假设两处落盘一定一致**。
    // 账本满 MAX_EVENTS 后老事件会被裁掉，而 crash.log 里那几行还在，这时日志多是正常的；
    // 但账本没满还比日志少，就是真漂移（老 bug 留下的历史数据也会这样）。
    // 两边取大者当 `crashes` —— 这个数字的语义是「我们至少知道崩过几次」，
    // 报小了会让人得出「没崩过」的错误结论，报大了只是多看一眼日志。
    let in_log = crashes_in_log(dir);
    let crashes = in_ledger.max(in_log);

    // 最近的排前面 —— 排障时先看最后发生的那条。
    let mut recent = events.clone();
    recent.reverse();
    recent.truncate(20);

    // 现在盘上可能同时躺着好几份标记（一实例一份）。**把它们全交出去**：
    // 「这台机器上正跑着几个 U-King」以前是隐形的，而它恰恰是这套账本从前说谎的根源
    //（多开 → 互相把对方记成异常退出），也是「定时任务 N 倍烧 token」的根源。
    // 让它变成一个能被一眼看见的数字，比在注释里警告有用。
    let live = live_sessions(dir);
    // `current_session` 保持老语义：优先自己那份；本进程没有会话（无头 CLI 跑 inspect
    // 就是这样）时退回**任意一个活着的实例** —— 否则运维远程查一台正在跑的机器会得到
    // `null`，看着像「它没在跑」，而那正是这个字段要回答的问题。
    let session = read_json(&my_session_path(dir))
        .or_else(|| live.first().cloned())
        .unwrap_or(serde_json::Value::Null);
    // 取证可用 = 日志目录真的写得进去，**就这一条**。
    //
    // 刻意**不**把「当前没有在跑的会话」算成 blocker：无头 CLI 进程本来就没有 GUI 会话，
    // 那是正常状态不是故障。写进 blockers 的话，每台客户机跑 conformance 都会冒一条假告警，
    // 而 `not_ready` 段的价值全在于「这里列出来的功能是真的废了」—— 掺一条常态噪音，
    // 下次真出问题就没人当回事了。会话在不在，看 `current_session` 字段自己判。
    //
    // 只判「目录建得起来」不够：目录已存在时只读盘上 create_dir_all 也会成功，
    // 所以老老实实试写一个探针文件。
    // 账本没满却比日志少 = 真漂移，如实说出来。
    //
    // 刻意**不**把它算进 `blockers`：漂移多半是历史数据（老版本先写日志后写账本留下的），
    // 修好之后它也永远在那儿 —— 那会变成一条**永久红灯**，而永久红灯的下场是被人学会忽略，
    // 下次真出事就没人看了。取证本身没坏，坏的是某几条旧记录，说清楚即可。
    let drift = if events.len() < MAX_EVENTS { in_log.saturating_sub(in_ledger) } else { 0 };

    let mut blockers: Vec<String> = Vec::new();
    let probe = dir.join(".write-probe");
    if std::fs::create_dir_all(dir).is_err() {
        blockers.push("日志目录建不起来（~/.uking/logs）".into());
    } else if std::fs::write(&probe, b"1").is_err() {
        blockers.push("日志目录写不进去（磁盘满或权限不足）".into());
    } else {
        let _ = std::fs::remove_file(&probe);
    }

    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "ready": blockers.is_empty(),
        "blockers": blockers,
        "crashes": crashes,
        // 两个来源都交出去：数字对不上时，看的人能自己判断信哪个、漂了多少，
        // 不必反过来怀疑整份报告。`crashes` = 两者取大。
        "crashes_in_ledger": in_ledger,
        "crashes_in_log": in_log,
        "ledger_drift": drift,
        "unclean_exits": unclean,
        "events": recent,
        "current_session": session,
        // 多开会 N 倍消耗定时任务的 token，也会让人误以为「U-King 自己重启了」。
        // 数字 ≥2 就是「这台机器上开了不止一个」，不必再靠翻任务管理器猜。
        "live_instances": live.len(),
        "live_sessions": live,
        "log_tail": crate::ulog::tail_in(dir, MODULE, 8192).unwrap_or_default(),
    })
}

/// 盘上所有**主人还活着**的会话标记，自己那份排在最前。
///
/// 每份都要真去问一次系统（子进程），所以封顶 8 份：真有 8 个实例在跑，问题早就不是
/// 「数得准不准」了；而让一个只读诊断动作在坏掉的机器上起几十个子进程，才是新麻烦。
fn live_sessions(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    if let Some(mine) = read_json(&my_session_path(dir)) {
        out.push(mine);
    }
    for (_, v) in stale_session_files(dir).into_iter().take(8) {
        let pid = v.get("pid").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if is_live_sibling(pid, &crate::installer::pid_image_name) {
            out.push(v);
        }
    }
    out
}

/// 数 `crash.log` 里有几行崩溃 —— 用来跟 JSON 账本交叉核对。
///
/// 读整份日志而不是尾巴：`ulog` 的单文件上限是 256 KB，一次读完不会撑爆内存，
/// 而只读尾巴会漏掉前面的崩溃、把「漂移」算小 —— 这个函数的存在意义正是抓漂移。
fn crashes_in_log(dir: &std::path::Path) -> usize {
    let Some(text) = crate::ulog::tail_in(dir, MODULE, 1 << 20) else { return 0 };
    // 匹配 push_event 写下的 `[{kind}] {summary}`，只认崩溃类，不含 unclean_exit。
    text.lines().filter(|l| l.contains("[panic]") || l.contains("[ui_crash]")).count()
}

/// 保开头 n 个字符 —— 崩溃摘要的信息量在前面（异常类型、消息），尾部多是栈的公共部分。
fn head(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一个独立临时目录 —— 绝不碰用户真实的 `~/.uking`（宪法第 10 条）。
    /// 不用随机数：进程 id + 递增计数器就够唯一，且可复现。
    fn tmp(tag: &str) -> PathBuf {
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("uking-crashlog-test-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 落一份「上次运行」的陈旧标记：`lived` 秒前启动，末次心跳在 `dead_ago` 秒前。
    fn stale_marker(dir: &std::path::Path, lived: u64, dead_ago: u64) {
        stale_marker_pid(dir, 4321, lived, dead_ago);
    }

    /// 同上，但指定 pid —— 多实例场景要能造出「好几份互不相干的标记」。
    fn stale_marker_pid(dir: &std::path::Path, pid: u32, lived: u64, dead_ago: u64) {
        let beat = now() - dead_ago;
        write_json(
            &dir.join(format!("{SESSION_PREFIX}{pid}{SESSION_SUFFIX}")),
            &serde_json::json!({ "pid": pid, "version": "0.9.80", "started_at": beat - lived, "beat_at": beat }),
        );
    }

    /// 存活探测的两种假身份。**测试一律注入**，不去问真系统：
    /// 真造一个活着的同名进程既慢又不可靠，而「主人还活着算不算异常退出」正是这次修复的全部要点。
    fn dead(_pid: u32) -> Option<String> {
        None
    }
    fn alive_sibling(_pid: u32) -> Option<String> {
        my_image_name() // 跟自己同名 = 另一个 U-King 实例
    }

    /// ★ 本模块存在的理由（pc-*** 回归测试）：崩溃必须**落盘**，不能只发网络。
    /// 这条挂了就意味着「客户断网时崩溃现场一个字节都不剩」的老毛病回来了。
    #[test]
    fn crash_lands_on_disk_without_any_network() {
        let d = tmp("disk");
        record_in(&d, "panic", "应用崩溃", "thread 'main' panicked at foo.rs:1: 越界");

        let v = inspect_in(&d);
        assert_eq!(v["crashes"], 1, "崩溃必须被记下来：{v}");
        assert!(d.join("crash.log").exists(), "crash.log 必须真的落在磁盘上");
        assert!(
            v["log_tail"].as_str().unwrap_or_default().contains("应用崩溃"),
            "日志尾巴要能读到摘要：{v}"
        );
    }

    /// 🔴 2026-08-18 黑盒测试在测试机上抓到的现场：`crash.log` 里躺着一条 `[panic]`，
    /// 账本里一条都没有 → `inspect` 报 `crashes: 0`。**这正是本模块立项要消灭的症状**
    /// （「客户报老是崩溃，查了一圈什么都没有」），却由本模块自己制造出来。
    ///
    /// 根因是 `push_event` 先写文本日志再读-改-写 JSON，两步之间随时可能 abort。
    /// 顺序已经掉过来（先 JSON 后日志），但**顺序只能改变失败方向，消不掉窗口**，
    /// 所以 `inspect` 必须交叉核对而不是闭着眼睛信 JSON —— 这条测的是后者。
    #[test]
    fn crash_count_never_undercounts_what_the_log_already_proves() {
        let d = tmp("drift");
        // 只有日志、没有账本：模拟"写完日志就死在中间"那一拍。
        crate::ulog::write_in(&d, MODULE, "[panic] 应用崩溃");

        let v = inspect_in(&d);
        assert_eq!(v["crashes_in_ledger"], 0, "前提：账本确实没记上");
        assert_eq!(v["crashes"], 1, "日志已证明崩过一次，crashes 不许报 0：{v}");
        assert_eq!(v["ledger_drift"], 1, "漂了多少要如实说出来，不能悄悄兜掉：{v}");
    }

    /// 账本被写坏（写到一半断电 / abort）不该把**整份历史**清零。
    ///
    /// 原来是 `fs::write` 整份回写，中途死掉就留半截 JSON，下次 `read_json` 解析失败
    /// → `unwrap_or_default()` 静默清空。现在改成临时文件 + rename，
    /// 最坏是丢**这一次**的更新，已经落盘的历史一条不少。
    #[test]
    fn torn_ledger_does_not_wipe_history_and_next_write_recovers() {
        let d = tmp("torn");
        record_in(&d, "panic", "第一次崩", "boom");
        // 手工把账本截成半截 JSON（就是非原子写被打断时盘上的样子）。
        let p = d.join(HISTORY_FILE);
        let raw = std::fs::read_to_string(&p).expect("账本要在");
        std::fs::write(&p, &raw[..raw.len() / 2]).expect("截断");

        // 坏账本 + 好日志：交叉核对必须仍然报得出崩过一次。
        let v = inspect_in(&d);
        assert_eq!(v["crashes"], 1, "账本坏了不等于没崩过，日志还在：{v}");

        // 下一次落盘要能自愈成合法 JSON，且不留 .tmp 垃圾。
        record_in(&d, "panic", "第二次崩", "boom2");
        let after = std::fs::read_to_string(&p).expect("账本要在");
        serde_json::from_str::<serde_json::Value>(&after).expect("写完必须是合法 JSON");
        assert_eq!(inspect_in(&d)["crashes"], 2, "两次崩溃都得数上");
        assert!(!p.with_extension("tmp").exists(), "临时文件不许留在客户磁盘上");
    }

    /// ★ pc-*** 上我最想要却没有的那条数据：上次是不是没正常退出、活了多久。
    #[test]
    fn stale_marker_is_read_as_unclean_exit() {
        let d = tmp("unclean");
        stale_marker(&d, 12, 1); // 只跑了 12 秒就没了 = 启动即崩

        let got = settle_previous_with(&d, &dead).expect("有陈旧标记就必须认出异常退出");
        assert!((11..=13).contains(&got.lived_secs), "活了多久要算对，实际 {}", got.lived_secs);
        assert!(got.looks_like_crash_loop, "12 秒就死 = 崩溃循环，必须够格上报");

        let v = inspect_in(&d);
        assert_eq!(v["unclean_exits"], 1, "异常退出要进账本：{v}");
    }

    /// 跑了 3 小时才没的，多半是关机/注销 —— 如实记录，但**不该**触发崩溃上报，
    /// 否则每台客户机每天关机都发一条 issue，真崩溃立刻被淹。
    #[test]
    fn long_run_is_recorded_but_not_flagged_as_crash_loop() {
        let d = tmp("long");
        stale_marker(&d, 3 * 3600, 1);

        let got = settle_previous_with(&d, &dead).expect("仍然是异常退出，要留痕");
        assert!(!got.looks_like_crash_loop, "跑了 3 小时不该被当成崩溃循环");
        assert_eq!(inspect_in(&d)["unclean_exits"], 1, "但账本里一条都不能少");
    }

    /// 正常退出删掉标记 → 下次启动不该凭空多出一次「崩溃」。假阳性比没有更坏。
    #[test]
    fn clean_exit_leaves_nothing_to_settle() {
        let d = tmp("clean");
        write_marker(&d, now() - 60);
        let _ = std::fs::remove_file(my_session_path(&d)); // end_session 干的事
        assert!(settle_previous_with(&d, &dead).is_none(), "标记被正常清掉后，不许报异常退出");
        assert_eq!(inspect_in(&d)["unclean_exits"], 0);
    }

    /// 🔴 **这次修复的主判据**：另一个实例正跑着 ≠ 上次崩了。
    ///
    /// 老实现全机器共用一份 `.session.json`，多开时第二个实例读到第一个的标记就记一笔
    /// 「异常退出」—— 实测账本 34 条里 20 条是这么来的，还有人拿这些数字比较版本稳定性，
    /// 得出「0.9.83 更稳」。**一个会说反话的账本比没有账本更坏。**
    #[test]
    fn live_sibling_instance_is_not_an_unclean_exit() {
        let d = tmp("sibling");
        stale_marker_pid(&d, 4321, 600, 1);

        assert!(
            settle_previous_with(&d, &alive_sibling).is_none(),
            "主人还活着（多开）就不许记成异常退出"
        );
        assert_eq!(inspect_in(&d)["unclean_exits"], 0, "账本里一条都不该有");
        assert!(
            d.join(format!("{SESSION_PREFIX}4321{SESSION_SUFFIX}")).exists(),
            "别人的标记必须留着 —— 那是它的命，得由它自己销账"
        );
    }

    /// 探不出来（`tasklist` 挂了 / 被杀软钩住 / 非 Windows 上没有 ps）时的失败方向：
    /// **宁可多记一笔，也不许漏**。漏记会让人得出「没崩过」的错误结论。
    #[test]
    fn undetectable_owner_still_gets_recorded() {
        let d = tmp("probe-fail");
        stale_marker_pid(&d, 4321, 30, 1);
        assert!(settle_previous_with(&d, &dead).is_some(), "探测失败必须退回记一笔，不许静默跳过");
        assert_eq!(inspect_in(&d)["unclean_exits"], 1);
    }

    /// pid 会被系统回收再分配。只判「pid 还在」的话，一个**陌生**进程会被认成自己人，
    /// 于是真崩溃被说成「那是并行实例」—— 恰好漏掉最该报的那一类。
    #[test]
    fn recycled_pid_of_a_stranger_is_not_mistaken_for_us() {
        let d = tmp("recycled");
        stale_marker_pid(&d, 4321, 8, 1);
        // pid 活着，但跑的是别人（记事本）。
        let stranger = |_pid: u32| Some("notepad.exe".to_string());
        let got = settle_previous_with(&d, &stranger).expect("陌生进程占了这个 pid ≠ 我还活着");
        assert!(got.looks_like_crash_loop, "8 秒就死，仍然是崩溃循环");
    }

    /// 一实例一份标记：两个实例各自的记录**互不覆盖**，各结各的账。
    ///
    /// 这条是「为什么不能只加一道 pid 校验」的回归 —— 单文件方案下 A 正常退出会删掉 B 的标记，
    /// B 之后真崩了盘上一点痕迹都没有（把重复多记换成静默漏记，方向反了）。
    #[test]
    fn each_instance_settles_only_its_own_marker() {
        let d = tmp("multi");
        stale_marker_pid(&d, 4321, 10, 1); // 短命的那个
        stale_marker_pid(&d, 8765, 9000, 1); // 长跑的那个

        let got = settle_previous_with(&d, &dead).expect("两份都没销账，得都记上");
        assert_eq!(inspect_in(&d)["unclean_exits"], 2, "两份标记 = 两条记录，不许合并成一条");
        assert!(got.looks_like_crash_loop, "交出去的该是最短命那条（最像崩溃循环、最值得上报）");

        // 自己那份不该被别人的结账波及。
        write_marker(&d, now() - 5);
        assert!(settle_previous_with(&d, &dead).is_none(), "自己这一轮还活着，不该给自己记异常退出");
        assert!(my_session_path(&d).exists(), "自己的标记不许被自己扫掉");
    }

    /// 老版本升级上来时，盘上那份 `.session.json` 得被认领一次再删 ——
    /// 直接无视等于把用户最后一次异常退出的证据丢掉，而那可能正是他来找你的原因。
    #[test]
    fn legacy_single_file_marker_is_migrated_once() {
        let d = tmp("legacy");
        let beat = now() - 1;
        write_json(
            &d.join(LEGACY_SESSION_FILE),
            &serde_json::json!({ "pid": 4321, "version": "1.0.2", "started_at": beat - 42, "beat_at": beat }),
        );

        let got = settle_previous_with(&d, &dead).expect("老标记也得结账");
        assert!((41..=43).contains(&got.lived_secs), "时长要算对，实际 {}", got.lived_secs);
        assert!(!d.join(LEGACY_SESSION_FILE).exists(), "认领完就该删掉，别每次启动重记一遍");
        assert!(settle_previous_with(&d, &dead).is_none(), "第二次启动不许再记一笔");
    }

    /// 目录写得进去才谈得上取证 —— `ready` 得如实反映，别让空账本冒充「没崩过」。
    #[test]
    fn readiness_reflects_writability_not_session_presence() {
        let d = tmp("ready");
        let v = inspect_in(&d);
        assert_eq!(v["ready"], true, "目录可写就该 ready：{v}");
        assert!(v["current_session"].is_null(), "没会话时如实给 null，而不是当成故障");
        assert_eq!(v["blockers"].as_array().map(|a| a.len()), Some(0), "别把「没有 GUI 会话」写成 blocker");
    }

    /// 账本得能滚动，否则崩溃循环的机器会把 JSON 撑到几百 MB。
    #[test]
    fn history_is_capped() {
        let mut events: Vec<u32> = (0..MAX_EVENTS as u32 + 15).collect();
        if events.len() > MAX_EVENTS {
            events.drain(..events.len() - MAX_EVENTS);
        }
        assert_eq!(events.len(), MAX_EVENTS, "账本必须封顶");
        assert_eq!(events[0], 15, "丢的该是最老的那批");
    }

    /// 摘要保开头 —— 保成尾部会得到一堆「at std::rt::lang_start」这种公共栈帧，看不出崩在哪。
    #[test]
    fn summary_keeps_head() {
        let s = "thread 'main' panicked at src/lib.rs:42: 索引越界".repeat(80);
        let h = head(&s, 20);
        assert!(h.starts_with("thread 'main'"), "要保开头：{h}");
        assert!(h.ends_with('…'), "截断要有省略号：{h}");
        assert_eq!(head("短的", 20), "短的", "没超长就别动它");
    }

    /// 短命判据是区分「崩溃循环」和「关机」的唯一依据，阈值写错整套判读就反了。
    #[test]
    fn short_run_threshold_separates_crashloop_from_shutdown() {
        assert!(12 <= SHORT_RUN_SECS, "启动即崩（12 秒）必须算短命");
        assert!(3 * 3600 > SHORT_RUN_SECS, "跑了 3 小时不该被当成崩溃循环");
    }
}
