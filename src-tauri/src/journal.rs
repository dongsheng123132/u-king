//! 行为时间轴（Journal）—— 「**谁**，在什么时候，干了**什么**，结果如何」。
//!
//! ## 为什么要有它，以及它跟已有三份记录的分界
//!
//! 仓库里已经有三套记录设施，各答各的问题。再加一套之前必须先说清楚边界，
//! 否则就是第四份会漂移的事实（宪法第 8 条）：
//!
//! | 模块 | 记什么 | 回答的问题 | 语义 |
//! |---|---|---|---|
//! | [`crate::ulog`] | 模块自由文本 | 「这个模块内部发生了什么」（人读，排障） | 滚动覆盖 |
//! | [`crate::metrics`] | 五类聚合指标 | 「**变好没有**」（optimize 锚点切 before/after） | usage 按天**覆盖** |
//! | `agent::TurnLog` | 单轮静默时长 | 「这一轮**卡在哪个阶段**」 | 每轮一条 |
//! | **本模块** | **逐条行为事件** | **「谁在什么时候干了什么、按什么顺序」** | **append-only，永不覆盖** |
//!
//! 前三者都答不了「昨晚 AI 到底动了什么」——`ulog` 是散在各模块的自由文本没法聚合，
//! `metrics` 的 usage 是**覆盖语义的每日快照**（按设计就没有逐条），`TurnLog` 只管时长。
//! 而「夜班」要交出的第一份东西恰恰是**一条可追溯的时间轴**：
//! 人干了什么 → AI 干了什么 → 结果如何。**记录是后面一切（熔断、护栏、回滚、交班报告）的地基**：
//! 没有时间轴，就没有「正常长什么样」，也就无从判断什么叫失控。
//!
//! ## 边界：只记 U-King 域内的动作，不做机器监控
//!
//! **只记两类事**：① 影核动作被执行（人通过 GUI/CLI 干的）② AI 调了工具。
//! **绝不记**：键鼠、窗口切换、剪贴板、浏览器历史、其它进程 —— 那是监控软件的范畴，
//! 不是这个产品该有的能力，也不该靠「客户没注意」来获得。这条边界是硬的，写在这儿是为了
//! 将来有人想往这里加「顺便记一下客户开了什么软件」时，先读到这段。
//!
//! ## 隐私（硬编码，不靠调用方自觉）
//!
//! - **不记 prompt 正文、不记文件内容、不记入参的值**（只记字段名 —— 沿用 `actions.rs`
//!   既有做法：`provider.save` 的入参里有 Key，记值就是泄漏）
//! - **路径一律脱敏**（[`redact_path`]）：工作区内折成 `<工作区>/相对路径`，
//!   工作区外只留文件名。绝对路径里带用户名，而客户会把这些文件直接转发给客服
//! - **默认只写本地，本模块零网络代码**（一行 curl 都没有，`diagnostics.collect`
//!   也不自动带上它 —— 要发得客户自己按导出）
//! - 一个开关能真的关掉（[`set_enabled`]），关了就是一个字节都不写
//!
//! ## 独立可插拔
//!
//! 纯 std + serde_json，**零 `AppHandle`、零业务模块 import**，和 `ulog.rs` 同层。
//! 依赖方向只能「业务模块 → 本模块」。删掉本模块只动 `lib.rs` 和前端两处。
//! `actions.rs`（纯协议核心）**不 import 本模块** —— 走它既有的注入范式
//! （`set_audit` 旁边加一个 `set_record`），组合根在开机时接上。

use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 事件流 schema 版本。**改字段必须升它**，否则读旧文件的代码会静默算错。
pub const SCHEMA_VERSION: u32 = 1;

/// 保留天数。超过的按天整文件删。30 天足够回答「上周那晚出了什么事」，
/// 又不会在客户机上无限长大。
const KEEP_DAYS: usize = 30;

/// 单日文件上限。一天写满 8MB 说明有东西在疯狂空转 —— 那本身是要查的事，
/// 但不能让它把客户磁盘吃光。触顶后**停写并留一条 truncated 记录**，
/// 不静默丢（静默丢等于时间轴从某一刻起是骗人的）。
const MAX_DAY_BYTES: u64 = 8 * 1024 * 1024;

// ───────────────────────── 路径 ─────────────────────────

fn home_dir() -> PathBuf {
    // 认 UKING_TEST_HOME 沙箱：测试要真跑落盘，又不能污染开发机自己的时间轴。
    std::env::var("UKING_TEST_HOME")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.uking/journal/`。
pub fn journal_dir() -> PathBuf {
    home_dir().join(".uking").join("journal")
}

fn switch_path() -> PathBuf {
    journal_dir().join("enabled.json")
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 某个 UTC 日期的文件。
///
/// **为什么按 UTC 分文件而查询按本地时间**：文件名只是滚动/清理的实现细节，
/// 复用 `ulog::date_utc` 就不必在仓库里存第二份历法算法；而客户问的「今天」是本地的，
/// 所以 [`query`] 按 `since_ms` 时间窗过滤、并且**多读一个前一天的文件**兜住时区差
/// （UTC+8 的「昨晚 11 点」落在前一个 UTC 文件里，少读一个就会把整个夜班漏掉）。
fn day_file(secs: i64) -> PathBuf {
    journal_dir().join(format!("{}.jsonl", crate::ulog::date_utc(secs)))
}

// ───────────────────────── 开关 ─────────────────────────

/// 开关缓存：`-1` 未知 / `0` 关 / `1` 开。
///
/// [`record`] 会被每一个动作和每一次 AI 工具调用打到，**不能每条都去读一次磁盘**。
/// 只有 [`set_enabled`] 会失效它 —— 客户手改 `enabled.json` 要重启才生效，可以接受
/// （那不是给人手改的文件，界面上有开关）。
static ENABLED_CACHE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

/// 本地记录开着吗。**默认开** —— 它是本地的、不上传的，而且是夜班/交班报告的唯一数据源，
/// 关着等于产品没有记忆。但必须能真关掉，且界面要明说在记什么（见 `NightShift.tsx`）。
pub fn enabled() -> bool {
    use std::sync::atomic::Ordering;
    match ENABLED_CACHE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => {
            let on = match std::fs::read_to_string(switch_path()) {
                Ok(s) => serde_json::from_str::<Value>(&s)
                    .ok()
                    .and_then(|v| v["enabled"].as_bool())
                    .unwrap_or(true),
                Err(_) => true, // 没有开关文件 = 从没关过 = 开
            };
            ENABLED_CACHE.store(on as i8, Ordering::Relaxed);
            on
        }
    }
}

/// 开 / 关本地记录。幂等。关掉之后 [`record`] 一个字节都不写。
pub fn set_enabled(on: bool) -> Result<(), String> {
    let dir = journal_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("建不了 {}: {e}", dir.display()))?;
    std::fs::write(switch_path(), json!({ "enabled": on }).to_string())
        .map_err(|e| format!("写不了记录开关: {e}"))?;
    ENABLED_CACHE.store(on as i8, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// 让开关缓存失效（切沙箱后必须调，否则读到的是上一个 home 的开关）。测试用。
#[cfg(test)]
fn invalidate_cache() {
    ENABLED_CACHE.store(-1, std::sync::atomic::Ordering::Relaxed);
}

// ───────────────────────── 脱敏 ─────────────────────────

/// 这个路径是绝对路径吗（`/x`、`C:\x`、`\\server\share`）。
fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/')
        || p.starts_with('\\')
        || (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
}

/// 路径脱敏。**绝不返回绝对路径** —— 里面带用户名（中文名尤其敏感），
/// 而客户会把导出的时间轴直接发给客服/群里。
///
/// - **相对路径 + 有工作区** → `<工作区>/原样`。AI 工具收到的就是相对路径（`src/a.rs`），
///   这条最常走。**必须保住目录层级** —— 只留文件名的话，「AI 动了哪些文件」这个
///   夜班最该回答的问题就退化成一串没有上下文的文件名（三个 `a.rs` 分不清是哪三个）。
/// - 绝对路径且在工作区内 → 同上，砍掉前缀
/// - 其余（工作区外 / 没给工作区）→ `…/文件名`，只保文件名不保路径
pub fn redact_path(p: &str, workspace: Option<&str>) -> String {
    let norm = |s: &str| s.replace('\\', "/");
    let path = norm(p.trim());
    if path.is_empty() {
        return "…".into();
    }
    let ws = workspace.map(|w| norm(w.trim().trim_end_matches(['/', '\\']))).filter(|w| !w.is_empty());
    if let Some(ws) = ws {
        // 相对路径：工具层已经把它限制在工作区内（`resolve_in_workspace` 拒 `..` 和绝对路径），
        // 所以直接当工作区内处理。`..` 仍防一手 —— 万一将来那道闸松了，这里不该跟着漏。
        if !is_absolute(&path) && !path.split('/').any(|seg| seg == "..") {
            return format!("<工作区>/{}", path.trim_start_matches("./"));
        }
        // Windows 路径大小写不敏感，比对前统一小写（只用于判断，输出仍取原文片段）
        if path.to_lowercase().starts_with(&ws.to_lowercase()) {
            let rel = path[ws.len()..].trim_start_matches('/');
            return if rel.is_empty() { "<工作区>".into() } else { format!("<工作区>/{rel}") };
        }
    }
    match path.rsplit('/').next() {
        Some(name) if !name.is_empty() => format!("…/{name}"),
        _ => "…".into(),
    }
}

/// 命令脱敏：只留**首个 token**（`git` / `npm` / `claude`）+ 参数个数。
///
/// 为什么不留全文：命令行里常年混着 Key（`--key sk-…`）、URL、绝对路径。
/// 而时间轴要回答的是「它跑了什么类型的命令、跑了多少次」，首 token 就够；
/// 要看全文去 `ulog` 的模块日志（那份不导出、不进反馈）。
pub fn redact_cmd(cmd: &str) -> String {
    let mut it = cmd.split_whitespace();
    match it.next() {
        Some(head) => {
            let n = it.count();
            if n == 0 { head.to_string() } else { format!("{head} (+{n} 参数)") }
        }
        None => "(空命令)".into(),
    }
}

// ───────────────────────── 写 ─────────────────────────

/// 谁在干这件事 —— 由「从哪个面进来的」推出来。
///
/// **`mcp` 算 AI 不算人**：`mcp serve` 的调用方按定义就是 AI（这是它存在的理由）。
/// 把它算成 human 会让时间轴在最关键的地方说谎 —— 将来 AI 通过 MCP 调了写动作，
/// 交班报告会写成「你自己干的」。同理 `scheduler`（定时任务无人值守）也是 AI。
pub fn actor_for(via: &str) -> &'static str {
    match via {
        "gui" | "cli" => "human",
        "mcp" | "scheduler" | "night" => "ai",
        _ => "system",
    }
}

/// 写一条。**永不 panic、永不返回错误** —— 记录写失败绝不能影响业务（同 `ulog::write`）。
/// 关了开关就直接返回。
pub fn record(ev: Value) {
    if !enabled() {
        return;
    }
    // 只收对象。**这不是洁癖，是防 abort**：`ev["v"] = …` 走 serde_json 的 `IndexMut<&str>`，
    // 它对 null 会自动变成对象，但对**数组 / 字符串 / 数字直接 panic**。
    // 而 release 是 `panic = "abort"`，且本函数挂在**全部动作的必经之路**上 ——
    // 一个传错类型的调用方就能让整个 app 当场消失。上面那句「永不 panic」得是真的。
    let Value::Object(mut map) = ev else {
        crate::ulog::write("journal", "record() 收到非对象事件，已丢弃（调用方传错了类型）");
        return;
    };
    let now = now_ms();
    map.insert("v".into(), json!(SCHEMA_VERSION));
    map.insert("at".into(), json!(now));
    let ev = Value::Object(map);
    let dir = journal_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let p = day_file(now / 1000);
    // 触顶：停写，但留一条说明。静默丢会让时间轴从某一刻起变成骗人的。
    if let Ok(m) = std::fs::metadata(&p) {
        if m.len() > MAX_DAY_BYTES {
            let marker = dir.join(".truncated");
            if !marker.exists() {
                let _ = std::fs::write(&marker, format!("{now}"));
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                    let _ = writeln!(
                        f,
                        "{}",
                        json!({ "v": SCHEMA_VERSION, "at": now, "actor": "system", "kind": "note",
                                "name": "journal.truncated", "ok": false,
                                "note": "当天记录超过上限，后续事件未记录" })
                    );
                }
            }
            return;
        }
    }
    // 只在**当天第一条**时清理过期文件。`prune` 要 `read_dir`，而 record 会被每一个动作
    // 和每一次 AI 工具调用打到 —— 每条都扫一遍目录是纯浪费。
    let first_of_day = !p.exists();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        // 一行一个 JSON。序列化本身不会带换行（serde_json 紧凑模式）。
        let _ = writeln!(f, "{ev}");
    }
    if first_of_day {
        prune();
    }
}

/// 影核动作被执行了一次。**由组合根注入给 `actions.rs`**（协议核心不认识本模块）。
///
/// `input_keys` 只有**字段名**没有值 —— 这是 `actions.rs` 既有的口径，别改成传值。
pub fn record_action(via: &str, id: &str, ok: bool, ms: u128, err_code: Option<&str>, input_keys: &[String]) {
    record(json!({
        "actor": actor_for(via),
        "via": via,
        "kind": "action",
        "name": id,
        "ok": ok,
        "ms": ms as i64,
        "err": err_code,
        "keys": input_keys,
    }));
}

/// 自由文本字段的长度上限。**在水槽这一端设防，不指望每个调用方自觉** ——
/// 上游的错误原文可能是一整段 stderr，一条记录撑爆整天的文件就等于把时间轴弄丢了。
const MAX_FIELD_CHARS: usize = 200;

fn cap(s: &str) -> String {
    if s.chars().count() <= MAX_FIELD_CHARS {
        s.to_string()
    } else {
        s.chars().take(MAX_FIELD_CHARS).collect::<String>() + "…"
    }
}

/// AI 调了一次工具。`target` 必须是**已脱敏**的（调用方用 [`redact_path`] / [`redact_cmd`]）。
///
/// `ms=0` 表示「这条路上拿不到耗时」—— 留 0 不编数字（宁可缺，不可假）。
pub fn record_tool(agent: &str, tool: &str, target: &str, ok: bool, ms: u128, err: Option<&str>) {
    record(json!({
        "actor": "ai",
        "via": "chat",
        "agent": agent,
        "kind": "tool",
        "name": tool,
        "target": cap(target),
        "ok": ok,
        "ms": ms as i64,
        "err": err.map(cap),
    }));
}

/// 留一条系统痕迹（班次开始/结束、开关被改…）。
pub fn note(name: &str, note: &str) {
    record(json!({ "actor": "system", "via": "system", "kind": "note", "name": name, "ok": true, "note": note }));
}

/// 删掉超过 [`KEEP_DAYS`] 的整天文件。
fn prune() {
    let Ok(rd) = std::fs::read_dir(journal_dir()) else { return };
    let mut days: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    if days.len() <= KEEP_DAYS {
        return;
    }
    days.sort(); // 文件名是 YYYY-MM-DD → 字典序 = 时间序
    for old in days.iter().take(days.len() - KEEP_DAYS) {
        let _ = std::fs::remove_file(journal_dir().join(old));
    }
}

// ───────────────────────── 读 ─────────────────────────

/// 读 `since_ms` 之后的事件（旧→新）。`limit` 是**尾部**上限（要最近的那些）。
///
/// 为什么多读一天：文件按 UTC 分，本地「今天」会跨到前一个 UTC 文件里（UTC+8 差 8 小时）。
/// 少读一个，客户早上看「昨晚的夜班」就是空的。
pub fn query(since_ms: i64, limit: usize) -> Vec<Value> {
    let now_s = now_ms() / 1000;
    let mut out: Vec<Value> = Vec::new();
    // 起点 = max(since 的前一天, 还留着的最早一天)。
    // **必须夹在 earliest 上而不是 0**：传 since_ms=0（「给我全部」）时若从 1970 那天起算，
    // 循环会在几十年前的空文件上耗尽 KEEP_DAYS 次迭代，**一条今天的记录都读不到**。
    let earliest = now_s - KEEP_DAYS as i64 * 86_400;
    let start = (since_ms / 1000 - 86_400).max(earliest);
    // 终点多兜一天：文件按 UTC 分，本地「今晚」可能已经落进下一个 UTC 文件。
    let mut day = start;
    while day <= now_s + 86_400 {
        let p = day_file(day);
        day += 86_400;
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if v["at"].as_i64().unwrap_or(0) >= since_ms {
                out.push(v);
            }
        }
    }
    out.sort_by_key(|v| v["at"].as_i64().unwrap_or(0));
    if out.len() > limit {
        out.drain(..out.len() - limit); // 留最近的
    }
    out
}

/// 时间轴摘要 —— 夜班页/交班报告的原料。
///
/// **报样本量**（`total`）：条数太少时任何「分布」都是噪音，界面得能说「数据不足」
/// 而不是画一个看着很像结论的饼图（对齐 `metrics.rs` 那三条出数规矩）。
pub fn summary(since_ms: i64) -> Value {
    let evs = query(since_ms, 100_000);
    let mut human = 0i64;
    let mut ai = 0i64;
    let mut system = 0i64;
    let mut failed = 0i64;
    let mut writes = 0i64;
    let mut by_name: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut files: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for e in &evs {
        match e["actor"].as_str().unwrap_or("") {
            "human" => human += 1,
            "ai" => ai += 1,
            _ => system += 1,
        }
        if !e["ok"].as_bool().unwrap_or(true) {
            failed += 1;
        }
        let name = e["name"].as_str().unwrap_or("?").to_string();
        // 「动过文件」= AI 的写类工具。夜班最想一眼看到的就是这个数。
        if matches!(name.as_str(), "write_file" | "edit_file") {
            writes += 1;
            if let Some(t) = e["target"].as_str() {
                *files.entry(t.to_string()).or_insert(0) += 1;
            }
        }
        *by_name.entry(name).or_insert(0) += 1;
    }
    let top = |m: std::collections::HashMap<String, i64>, n: usize| -> Vec<Value> {
        let mut v: Vec<(String, i64)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(n).map(|(k, c)| json!({ "name": k, "count": c })).collect()
    };
    json!({
        "since": since_ms,
        "total": evs.len(),
        "human": human,
        "ai": ai,
        "system": system,
        "failed": failed,
        "file_writes": writes,
        "top_actions": top(by_name, 10),
        "touched_files": top(files, 20),
        "first_at": evs.first().and_then(|e| e["at"].as_i64()),
        "last_at": evs.last().and_then(|e| e["at"].as_i64()),
    })
}

/// 清空全部记录（客户的数据，客户能自己删干净）。
pub fn clear() -> Result<(), String> {
    let dir = journal_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(()) }; // 没有目录 = 已经是空的
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            std::fs::remove_file(&p).map_err(|err| format!("删不掉 {}: {err}", p.display()))?;
        }
    }
    let _ = std::fs::remove_file(dir.join(".truncated"));
    Ok(())
}

/// 只读状态 —— 影核动作 `runtime.journal.inspect` 的返回。
///
/// 带 `ready` + `blockers`（readiness 约定）：回答**「能不能记」**而不是「有没有记过」。
/// 关了开关 = 有意为之，不算 blocker，但要如实说；写不进磁盘才是真的坏。
pub fn status(days: i64) -> Value {
    let days = days.clamp(1, KEEP_DAYS as i64);
    let since = now_ms() - days * 86_400_000;
    let on = enabled();
    let mut blockers: Vec<String> = Vec::new();
    let dir = journal_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        blockers.push(format!("写不了 {} —— 行为记录存不下来", dir.display()));
    }
    json!({
        "ready": blockers.is_empty() && on,
        "blockers": blockers,
        "enabled": on,
        "schema_version": SCHEMA_VERSION,
        "keep_days": KEEP_DAYS,
        "dir": dir.to_string_lossy(),
        // 产品边界当数据发（同 automation 的 runs_only_while_app_open）：
        // GUI 文案 / CLI / MCP 读的是同一句话，不会三处各自跑偏。
        "records_only_uking_actions": true,
        "uploads": false,
        "days": days,
        "summary": summary(since),
        "recent": query(since, 200),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 绝对路径**永远**不许原样进时间轴 —— 里面有用户名，客户会把导出直接转发出去。
    #[test]
    fn absolute_paths_never_survive_redaction() {
        let ws = Some("C:\\Users\\张三\\proj");
        assert_eq!(redact_path("C:\\Users\\张三\\proj\\src\\a.rs", ws), "<工作区>/src/a.rs");
        assert_eq!(redact_path("C:/Users/张三/proj/src/a.rs", ws), "<工作区>/src/a.rs");
        // 工作区外：只剩文件名
        assert_eq!(redact_path("C:\\Users\\张三\\.ssh\\id_rsa", ws), "…/id_rsa");
        assert_eq!(redact_path("/home/zhangsan/.env", None), "…/.env");
        for p in ["C:\\Users\\张三\\x", "/home/zhangsan/x"] {
            for out in [redact_path(p, ws), redact_path(p, None)] {
                assert!(!out.contains("张三") && !out.contains("zhangsan"), "泄漏了用户名: {out}");
            }
        }
    }

    /// AI 工具收到的**就是相对路径**（这条最常走），目录层级必须保住 ——
    /// 只留文件名的话，「AI 动了哪些文件」会退化成一串分不清彼此的 `a.rs`。
    /// 真机 e2e 跑出 `…/note.txt` 才发现的：当时相对路径掉进了「工作区外」那一支。
    #[test]
    fn relative_paths_keep_their_directories() {
        let ws = Some("C:\\Users\\张三\\proj");
        assert_eq!(redact_path("note.txt", ws), "<工作区>/note.txt");
        assert_eq!(redact_path("src/deep/a.rs", ws), "<工作区>/src/deep/a.rs");
        assert_eq!(redact_path("./src/a.rs", ws), "<工作区>/src/a.rs");
        assert_eq!(redact_path(".", ws), "<工作区>/.");
        // 没有工作区就没有「区内」可言，只能留文件名
        assert_eq!(redact_path("src/a.rs", None), "…/a.rs");
        // `..` 不许被当成工作区内的相对路径（工具层已拦，这里不跟着漏第二道）
        assert_eq!(redact_path("../../.ssh/id_rsa", ws), "…/id_rsa");
    }

    /// 命令只留首 token —— 参数里常年混着 Key / URL / 绝对路径。
    #[test]
    fn command_redaction_drops_arguments() {
        assert_eq!(redact_cmd("git status"), "git (+1 参数)");
        assert_eq!(redact_cmd("claude -p --key sk-xp-secret"), "claude (+3 参数)");
        assert_eq!(redact_cmd("ls"), "ls");
        assert!(!redact_cmd("curl -H 'Authorization: Bearer sk-real-key' x").contains("sk-real-key"));
    }

    /// MCP / 定时任务的调用方是 **AI 不是人**。搞反了，交班报告会把 AI 干的事
    /// 写成「你自己干的」—— 那正是这份时间轴最不能出错的地方。
    #[test]
    fn mcp_and_scheduler_are_ai_not_human() {
        assert_eq!(actor_for("gui"), "human");
        assert_eq!(actor_for("cli"), "human");
        assert_eq!(actor_for("mcp"), "ai");
        assert_eq!(actor_for("scheduler"), "ai");
        assert_eq!(actor_for("night"), "ai");
        assert_eq!(actor_for("没见过的面"), "system");
    }

    /// 真跑一遍落盘 —— 开关、写、读、清空。
    ///
    /// 这几件事必须在**同一个** `#[test]` 里：它们共享 `UKING_TEST_HOME` 这个进程级环境变量，
    /// 而 cargo 默认多线程跑测试，拆成几个用例会互相踩沙箱（读到对方的开关/文件）。
    ///
    /// 🔴 但「合并进一个用例」**只挡得住模块内的并行**。别的模块（org / aitasks / providers …）
    /// 同时改同一个环境变量照样把这条踩红（实测稳定读到 0 条）—— 那要靠 `testsandbox`
    /// 的全进程唯一锁，不是靠合并用例。
    #[test]
    fn switch_and_roundtrip_on_disk() {
        crate::testsandbox::with_sandbox("journal-roundtrip", &[], |_| {
        invalidate_cache(); // 换了 home，上一个 home 的开关缓存作废
        let _ = clear();

        // ① 关了就得真的一个字节都不写 —— 否则这个开关是骗人的
        set_enabled(false).expect("写开关");
        assert!(!enabled());
        record(json!({ "actor": "human", "kind": "action", "name": "不该被记下来", "ok": true }));
        assert_eq!(query(0, 100).len(), 0, "关了开关还在写");

        // ② 开了要写得进去。**since_ms=0 必须能读到今天的记录** —— 这条守的是一个真 bug：
        //    起点若从 1970 那天算起，循环会在几十年前的空文件上耗尽天数上限，一条都读不到。
        set_enabled(true).expect("写开关");
        record_action("gui", "runtime.stack.inspect", true, 12, None, &["days".to_string()]);
        record_tool("uking", "write_file", "<工作区>/src/a.rs", true, 3, None);
        let all = query(0, 100);
        assert_eq!(all.len(), 2, "since=0 应读到今天全部记录，实际 {}", all.len());
        assert!(all.iter().all(|e| e["v"] == json!(SCHEMA_VERSION) && e["at"].as_i64().unwrap_or(0) > 0));

        // ③ 摘要要认得出「人干的」和「AI 干的」，并数出动过几个文件
        let s = summary(0);
        assert_eq!(s["total"], json!(2));
        assert_eq!(s["human"], json!(1), "GUI 来的动作应算人干的");
        assert_eq!(s["ai"], json!(1), "AI 工具调用应算 AI 干的");
        assert_eq!(s["file_writes"], json!(1));

        // ④ 非对象事件必须被丢掉而**不是 panic**。release 是 panic=abort，而 record()
        //    在全部动作的必经之路上 —— 这里真 panic 了就是整个 app 消失，不是少一条日志。
        for bad in [json!([1, 2]), json!("裸字符串"), json!(42), json!(true), json!(null)] {
            record(bad); // 不 panic 即通过
        }
        assert_eq!(query(0, 100).len(), 2, "非对象事件不该被写进去，也不该冲掉已有的");

        // ⑤ 客户能把自己的数据删干净
        clear().expect("清空");
        assert_eq!(query(0, 100).len(), 0, "清空后还有残留");
        });
        // 出了沙箱 home 已还原，缓存里还留着沙箱那份开关状态，得作废掉。
        invalidate_cache();
    }
}
