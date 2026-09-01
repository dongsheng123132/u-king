//! 数据基台 —— 本地事件日志（append-only JSONL）。
//!
//! ## 它解决什么
//!
//! `usage_local.rs` 已经能读 Claude/Codex 的会话日志按「工具×模型」聚合，但它是**即时扫描、
//! 不落盘**：算完就没了。于是没有历史序列，更没有「优化前长什么样」——所以「优化后快了多少、
//! 省了多少」这类结论**一条都得不出来**。本模块补的就是那条时间轴。
//!
//! ## 五个事件
//!
//! - `usage`    每天一次的用量滚动快照（分工具、分模型）
//! - `error`    报错（带**错误签名**，让「同一个错」能跨机器计数而不用传原文）
//! - `optimize` ★ **优化锚点**——before/after 全靠这条切。没有它，采集再多也是一锅粥
//! - `fix`      AI 自我修复（`verified` 只认机器复验，不认 AI 自述「我修好了」）
//! - `bench`    微基准占位（T3），格式先定死，以后加基准项不用升 schema
//!
//! ## 红线
//!
//! - **默认只写本地、绝不自动上传**（`consent.json` 默认 `upload:false`）
//! - **绝不写 prompt / 代码 / 路径原文 / Key**。调用方传进来的 `msg` 必须**先脱敏**
//!   （`feedback::desensitize`），本模块只负责存
//! - 所有写入 best-effort：失败就算了，**永不影响主流程、永不 panic**
//!
//! ## 独立可插拔
//!
//! **不 import 任何其它功能模块**（设计取舍铁律②）。环境指纹（envfp）和用量数据（usage_local）
//! 由 `lib.rs` 这个组合根注入。纯 std + serde_json。删本模块只动 lib.rs。
//!
//! schema 定义见 `docs/metrics-schema.md`——改字段必须同步那份文档并升 `SCHEMA_VERSION`。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

/// schema 版本。加/改字段必须 +1。
/// v2（2026-08-04）：新增 `tool_use` / `tool_apply` / `tool_probe` 三类事件，
/// 用来回答「哪些终端工具该下架」。老的五类字段一个没动，v1 的文件照常读。
pub const SCHEMA_VERSION: u32 = 2;

/// 单个月文件的体积上限：超了就停止追加（防极端情况下刷爆磁盘）。
const MAX_MONTH_BYTES: u64 = 16 * 1024 * 1024;

/// 保留多少个月的历史。
const KEEP_MONTHS: i64 = 12;

/// before/after 每侧至少要有几天数据，才允许给出对比结论。
/// 低于这个数就只说「数据不足」——**这比编一个数字强一万倍**。
const MIN_DAYS_EACH_SIDE: u32 = 3;

// ============================================================
// 路径 / 时间
// ============================================================

fn home_dir() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let h = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(h)
}

pub fn metrics_dir() -> PathBuf {
    home_dir().join(".uking").join("metrics")
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix 秒 → (年, 月, 日)（UTC）。civil_from_days，Howard Hinnant 算法。
/// 纯算术不起子进程——与 `installer::utc_stamp` 同一份算法（那份是私有的，
/// 且只输出「此刻」的字符串，这里要能换算任意时间戳，故各持一份）。
fn ymd(secs: u64) -> (i64, u32, u32) {
    let days = (secs as i64).div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = era * 400 + yoe + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

fn day_key(secs: u64) -> String {
    let (y, m, d) = ymd(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

fn month_key(secs: u64) -> String {
    let (y, m, _) = ymd(secs);
    format!("{y:04}-{m:02}")
}

fn month_file(secs: u64) -> PathBuf {
    metrics_dir().join(format!("events-{}.jsonl", month_key(secs)))
}

// ============================================================
// 写入
// ============================================================

/// 追加一条事件。**best-effort：任何失败都静默吞掉，绝不影响主流程。**
fn append(mut ev: Value) {
    let ts = now_secs();
    if let Some(o) = ev.as_object_mut() {
        o.insert("v".into(), json!(SCHEMA_VERSION));
        o.insert("ts".into(), json!(ts));
        o.insert("app".into(), json!(env!("CARGO_PKG_VERSION")));
    }
    let dir = metrics_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = month_file(ts);
    // 体积闸：超了就不再追加（宁可丢数据，不能刷爆客户磁盘）
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) >= MAX_MONTH_BYTES {
        return;
    }
    let line = match serde_json::to_string(&ev) {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// 一行用量（由 `lib.rs` 从 `usage_local::LocalUsageItem` 映射过来 —— 本模块不 import 它）。
pub struct UsageRow {
    pub tool: String,
    pub model: String,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// 每天一次的用量滚动快照。同一天重复调用会**先删掉当天旧行再写**（覆盖不累加），
/// 所以多开、重启都不会把同一天的量记重。
pub fn log_usage_rollup(rows: &[UsageRow]) {
    let ts = now_secs();
    let today = day_key(ts);
    dedupe_day(&month_file(ts), &today);
    for r in rows {
        if r.calls == 0 {
            continue;
        }
        append(json!({
            "ev": "usage",
            "day": today,
            "tool": r.tool,
            "model": r.model,
            "calls": r.calls,
            "in": r.input,
            "out": r.output,
            "cache_read": r.cache_read,
            "cache_write": r.cache_write,
        }));
    }
}

/// 把当月文件里 `ev=usage 且 day=<today>` 的行剔掉（快照是覆盖语义，不是累加）。
fn dedupe_day(path: &PathBuf, today: &str) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let total = content.lines().count();
    let kept: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let v: Value = match serde_json::from_str(l) {
                Ok(v) => v,
                Err(_) => return true, // 解析不了的行原样保留，不做破坏性清理
            };
            !(v.get("ev").and_then(Value::as_str) == Some("usage")
                && v.get("day").and_then(Value::as_str) == Some(today))
        })
        .collect();
    if kept.len() != total {
        // 全被剔掉时要写**空串**：写 "\n" 会在文件头留一个空行，
        // 而空行解析失败又会被上面的 filter 当「不认识的行」永久保留下来。
        let out = if kept.is_empty() { String::new() } else { kept.join("\n") + "\n" };
        let _ = std::fs::write(path, out);
    }
}

/// 记一条报错。`msg` **必须由调用方先脱敏**。
pub fn log_error(kind: &str, tool: Option<&str>, msg: &str) {
    append(json!({
        "ev": "error",
        "kind": kind,
        "tool": tool.unwrap_or(""),
        "sig": error_signature(msg),
        "msg": truncate(msg, 200),
    }));
}

/// ★ 优化锚点 —— before/after 全靠这条切。`env` 是环境指纹快照（由 lib.rs 注入）。
pub fn log_optimize(recipes: &[String], score_before: Option<u32>, score_after: Option<u32>, env: Value) {
    append(json!({
        "ev": "optimize",
        "recipes": recipes,
        "score_before": score_before,
        "score_after": score_after,
        "env": env,
    }));
}

// ============================================================
// 工具栈三件套（v2）—— 回答「该下架谁」
// ============================================================
//
// 下架一个工具要三个数字，缺一条就会砍错人：
//   ① 配得上吗（`tool_apply`）—— 一键配置对它成功还是跳过
//   ② 配上了真能用吗（`tool_probe`）—— **真跑一句话**，不是看配置文件长得对不对
//   ③ 客户真用了吗（`tool_use`）—— 没人用的才该下架
//
// ② 单列一类是被 Crush 那个 bug 逼出来的：配置写了半年、`configured` 报得漂漂亮亮，
// 而 crush 读的是另一个目录，一次都没生效。**只有真跑过才算数**，形状对不能当能用。
//
// 三类都只写本地。别在这儿判断「该不该下架」——那是看报告的人的决定，
// 我们只负责把数字摆出来，包括难看的那些。

/// 客户**真的用了**某个工具。`how`：`term`（从内置终端开的）/ `gui`（launch_app 启的）。
///
/// 口径故意窄：只记**我们自己发起**的启动，不去解析各家 CLI 的会话日志。
/// 那 6 个工具日志格式各不相同、还随版本漂移，是长期维护负担；而「有没有人用」
/// 这个问题，我们自己的启动记录就够回答了。**别把这个数字当完整用量**——
/// 客户在系统终端里自己敲 `pi` 我们看不见，报告里必须写明这条边界。
pub fn log_tool_use(tool: &str, how: &str) {
    if tool.trim().is_empty() {
        return;
    }
    append(json!({ "ev": "tool_use", "tool": tool, "how": how }));
}

/// 一键配置对某个工具的结果。`ok=false` 时 `note` 说明原因（未安装 / 用户已有配置 / 写失败）。
pub fn log_tool_apply(tool: &str, ok: bool, note: &str) {
    append(json!({
        "ev": "tool_apply",
        "tool": tool,
        "ok": ok,
        "note": truncate(note, 160),
    }));
}

/// ★ 真实可用性实测：拿配好的驱动**真跑一句话**，看它回不回话。
///
/// `ok=false` 时 `note` 是脱敏后的失败原因。`ms` 是耗时 —— 慢到客户不愿用，
/// 跟不能用一样是下架理由，所以哪怕 ok 也记时间。
pub fn log_tool_probe(tool: &str, ok: bool, ms: u64, note: &str) {
    append(json!({
        "ev": "tool_probe",
        "tool": tool,
        "ok": ok,
        "ms": ms,
        "note": truncate(note, 160),
    }));
}

// ============================================================
// 错误签名
// ============================================================

/// FNV-1a 64。**自己写而不用 `DefaultHasher`**：后者的算法官方保留变更权利，
/// 换个 Rust 版本签名就变了，跨机器/跨版本聚合会全部对不上。
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 错误签名：把数字/路径/十六进制归一后取 hash，让「同一个错」能跨机器计数，
/// 而**不必上传错误原文**。例：「仅剩 416 MB」和「仅剩 413 MB」是同一个签名。
pub fn error_signature(msg: &str) -> String {
    let head: String = msg.chars().take(400).collect();
    let norm: Vec<String> = head
        .split_whitespace()
        .map(|tok| {
            // **整段**路径压成占位，而不只是压分隔符：盘符、用户名、临时目录逐机不同，
            // 只压分隔符的话「同一个 npm 错误」在每台机器上仍会得到不同签名，
            // 跨机器聚合彻底失效 —— 而那正是签名唯一要解决的问题。
            if tok.contains('\\') || tok.contains('/') {
                return "<path>".to_string();
            }
            // 数字串压成 #：「仅剩 416 MB」和「仅剩 413 MB」是同一个错
            let mut s = String::with_capacity(tok.len());
            let mut in_num = false;
            for c in tok.chars() {
                if c.is_ascii_digit() {
                    if !in_num {
                        s.push('#');
                        in_num = true;
                    }
                } else {
                    in_num = false;
                    s.extend(c.to_lowercase());
                }
            }
            s
        })
        .collect();
    format!("{:08x}", fnv1a(&norm.join(" ")) & 0xffff_ffff)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

// ============================================================
// 上传同意（默认关）
// ============================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Consent {
    /// 是否允许上传聚合指标。**默认 false**，必须用户显式打开。
    pub upload: bool,
    /// 是否已经问过（避免反复弹）。
    pub asked: bool,
}

impl Default for Consent {
    fn default() -> Self {
        Consent { upload: false, asked: false }
    }
}

fn consent_path() -> PathBuf {
    metrics_dir().join("consent.json")
}

pub fn consent() -> Consent {
    std::fs::read_to_string(consent_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn set_consent(upload: bool) -> Result<(), String> {
    let dir = metrics_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c = Consent { upload, asked: true };
    std::fs::write(
        consent_path(),
        serde_json::to_string(&c).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

// ============================================================
// 读取 / 报告
// ============================================================

#[derive(Serialize, Default)]
pub struct ModelRow {
    pub tool: String,
    pub model: String,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// 有几天出现过（样本量，UI 要显示——没有它读者无法判断可信度）
    pub days: u32,
}

#[derive(Serialize)]
pub struct ErrorRow {
    pub sig: String,
    pub kind: String,
    pub tool: String,
    pub count: u64,
    /// 最近一次的（已脱敏）摘要
    pub last_msg: String,
}

#[derive(Serialize)]
pub struct CompareRow {
    pub tool: String,
    pub errors_per_day_before: f64,
    pub errors_per_day_after: f64,
    pub tokens_per_call_before: f64,
    pub tokens_per_call_after: f64,
    /// **算不出就是 None，绝不编一个 0** —— 分母为零时任何百分比都是假的
    pub errors_delta_pct: Option<f64>,
    pub tokens_delta_pct: Option<f64>,
}

#[derive(Serialize)]
pub struct Compare {
    pub anchor_ts: u64,
    pub recipes: Vec<String>,
    pub before_days: u32,
    pub after_days: u32,
    /// 两侧样本都够 `MIN_DAYS_EACH_SIDE` 才为 true。false 时 UI **不许**显示大字结论
    pub sufficient: bool,
    pub rows: Vec<CompareRow>,
}

/// 按天的时间序列 —— UI 画趋势用。聚合表看不出「哪天开始变糟」。
#[derive(Serialize, Default)]
pub struct DayRow {
    pub day: String,
    pub calls: u64,
    pub tokens: u64,
    pub errors: u64,
}

/// 一条建议。
///
/// **判断在核心，不在界面**（宪法第 15 条）：这些是确定性的算术/阈值结论，
/// 本地毫秒级算完、离线可用、不烧一个 token。GUI / CLI / MCP 拿到同一份。
#[derive(Serialize)]
pub struct Advice {
    /// 稳定 id（前端配图标、测试拿它断言）
    pub id: &'static str,
    /// high = 现在就会咬人 / medium = 迟早出事 / low = 可以顺手改善
    pub severity: &'static str,
    /// 一句话结论
    pub title: String,
    /// 具体怎么做
    pub detail: String,
    /// ★ **凭什么这么说** —— 支撑这条建议的实际数字。
    /// 没有证据的建议就是猜，不许给（本项目所有结论的统一规矩）。
    pub evidence: String,
    /// 优化大师能一键做掉的话，这里给动作名；不能就是 None
    pub action: Option<&'static str>,
}

#[derive(Serialize)]
pub struct MetricsReport {
    pub schema: u32,
    pub days: i64,
    pub events: u64,
    /// 最早一条事件的时间戳 —— 决定所有结论的可信上限
    pub first_ts: Option<u64>,
    pub models: Vec<ModelRow>,
    /// 按天的序列（升序），UI 画趋势
    pub daily: Vec<DayRow>,
    pub errors: Vec<ErrorRow>,
    pub compare: Option<Compare>,
    /// 有证据支撑的建议（按严重度排序）。**空数组是合法结果** —— 没什么好建议的就别硬凑
    pub advice: Vec<Advice>,
    /// ★ 诚实说明：样本不足、数据缺口、没有锚点……全写这里，UI 原样显示
    pub notes: Vec<String>,
    /// 上传同意状态（默认关）
    pub upload_consent: bool,
    /// ★ 工具栈：每个工具「配得上 / 能用 / 有人用」三个数字，给「该下架谁」做依据
    pub toolstack: Vec<ToolRow>,
}

/// 一个终端工具的三个数字。**不在这里下结论**——「该不该下架」是看报告的人的决定，
/// 我们只把数字摆出来，包括难看的那些。
#[derive(Serialize, Default, Clone)]
pub struct ToolRow {
    pub tool: String,
    /// 一键配置对它成功过几次
    pub apply_ok: u64,
    /// 一键配置跳过/失败几次（原因见 `last_apply_note`）
    pub apply_fail: u64,
    pub last_apply_note: String,
    /// 可用性实测：最近一次的结论。`None` = **从没测过**，不是「不能用」。
    pub probe_ok: Option<bool>,
    pub probe_ms: u64,
    pub probe_note: String,
    /// 被启动过多少次（我们自己发起的：内置终端 + launch_app）
    pub used: u64,
    /// 用过的天数 —— 比总次数更能说明「是不是真在用」：
    /// 一天点 20 次可能只是在反复试到底怎么才能用，20 天各点 1 次才是真的在用。
    pub used_days: u64,
}

/// 读最近 `days` 天的事件。
fn load_events(days: i64) -> Vec<Value> {
    let now = now_secs();
    let cutoff = now.saturating_sub((days.max(1) as u64) * 86_400);
    let mut out = Vec::new();
    // 跨月窗口：把窗口覆盖到的每个月文件都读一遍
    let mut months: Vec<String> = Vec::new();
    let mut t = cutoff;
    while t <= now {
        let k = month_key(t);
        if !months.contains(&k) {
            months.push(k);
        }
        t += 86_400;
    }
    let now_month = month_key(now);
    if !months.contains(&now_month) {
        months.push(now_month);
    }
    for m in months {
        let p = metrics_dir().join(format!("events-{m}.jsonl"));
        if let Ok(c) = std::fs::read_to_string(&p) {
            for l in c.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(l) {
                    if v.get("ts").and_then(Value::as_u64).unwrap_or(0) >= cutoff {
                        out.push(v);
                    }
                }
            }
        }
    }
    out
}

fn u64f(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}
fn strf(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// 本地报告 —— **不上传也能看**，这是用户愿意开采集的唯一理由。
///
/// `env` = 环境指纹（由 lib.rs 注入；metrics 不 import envfp）。传 `Value::Null`
/// 就只出用量类建议，环境类建议全部跳过 —— 宁可少给，不猜。
pub fn report(days: i64, env: Value) -> MetricsReport {
    let evs = load_events(days);
    let up = consent().upload;
    report_from(&evs, days, up, &env)
}

/// 报告的**纯函数内核**（不碰文件系统）。
///
/// 单独抽出来是为了能真测：before/after 对比是整个基台唯一会变成「大字结论」的地方，
/// 它算错就等于对客户撒谎，必须有确定性测试钉住，而不是靠在真机上跑一遍看着像对的。
fn report_from(evs: &[Value], days: i64, upload_consent: bool, env: &Value) -> MetricsReport {
    let mut notes: Vec<String> = Vec::new();

    let first_ts = evs.iter().filter_map(|e| e.get("ts").and_then(Value::as_u64)).min();

    // —— 分工具 × 分模型 ——
    let mut agg: HashMap<(String, String), (ModelRow, std::collections::HashSet<String>)> = HashMap::new();
    for e in evs.iter().filter(|e| strf(e, "ev") == "usage") {
        let key = (strf(e, "tool"), strf(e, "model"));
        let slot = agg.entry(key.clone()).or_insert_with(|| {
            (
                ModelRow { tool: key.0.clone(), model: key.1.clone(), ..Default::default() },
                std::collections::HashSet::new(),
            )
        });
        slot.0.calls += u64f(e, "calls");
        slot.0.input += u64f(e, "in");
        slot.0.output += u64f(e, "out");
        slot.0.cache_read += u64f(e, "cache_read");
        slot.0.cache_write += u64f(e, "cache_write");
        slot.1.insert(strf(e, "day"));
    }
    let mut models: Vec<ModelRow> = agg
        .into_values()
        .map(|(mut r, d)| {
            r.days = d.len() as u32;
            r
        })
        .collect();
    models.sort_by(|a, b| b.calls.cmp(&a.calls));

    // —— 报错 top ——
    let mut emap: HashMap<String, ErrorRow> = HashMap::new();
    for e in evs.iter().filter(|e| strf(e, "ev") == "error") {
        let sig = strf(e, "sig");
        let row = emap.entry(sig.clone()).or_insert(ErrorRow {
            sig,
            kind: strf(e, "kind"),
            tool: strf(e, "tool"),
            count: 0,
            last_msg: String::new(),
        });
        row.count += 1;
        row.last_msg = strf(e, "msg");
    }
    let mut errors: Vec<ErrorRow> = emap.into_values().collect();
    errors.sort_by(|a, b| b.count.cmp(&a.count));
    errors.truncate(10);

    // —— before / after ——
    let anchor = evs
        .iter()
        .filter(|e| strf(e, "ev") == "optimize")
        .max_by_key(|e| u64f(e, "ts"))
        .cloned();

    let compare = match &anchor {
        None => {
            notes.push("还没有「一键优化」记录，所以给不出前后对比——优化一次之后这里才会出现。".into());
            None
        }
        Some(a) => {
            let ats = u64f(a, "ts");
            let c = build_compare(&evs, a, ats);
            if !c.sufficient {
                notes.push(format!(
                    "优化前后各需至少 {MIN_DAYS_EACH_SIDE} 天数据才能下结论（现在前 {} 天 / 后 {} 天）。先继续用几天。",
                    c.before_days, c.after_days
                ));
            }
            Some(c)
        }
    };

    if models.is_empty() {
        notes.push("还没有用量快照。装了 Claude Code / Codex 并跑过之后，每天会自动记一次。".into());
    }
    if let Some(f) = first_ts {
        let span_days = now_secs().saturating_sub(f) / 86_400;
        if span_days < 7 {
            notes.push(format!("数据只覆盖 {span_days} 天，样本还太少，任何结论都当参考。"));
        }
    }

    // —— 按天序列（升序）——
    let mut dmap: HashMap<String, DayRow> = HashMap::new();
    for e in evs {
        match strf(e, "ev").as_str() {
            "usage" => {
                let day = strf(e, "day");
                let r = dmap.entry(day.clone()).or_insert(DayRow { day, ..Default::default() });
                r.calls += u64f(e, "calls");
                r.tokens += u64f(e, "in") + u64f(e, "out");
            }
            "error" => {
                // 错误事件没有 day 字段（它不是快照），按 ts 归日
                let day = day_key(u64f(e, "ts"));
                let r = dmap.entry(day.clone()).or_insert(DayRow { day, ..Default::default() });
                r.errors += 1;
            }
            _ => {}
        }
    }
    let mut daily: Vec<DayRow> = dmap.into_values().collect();
    daily.sort_by(|a, b| a.day.cmp(&b.day));

    let advice = build_advice(&models, &errors, env);
    let toolstack = build_toolstack(evs, &mut notes);

    MetricsReport {
        schema: SCHEMA_VERSION,
        days,
        events: evs.len() as u64,
        first_ts,
        models,
        daily,
        errors,
        compare,
        advice,
        notes,
        upload_consent,
        toolstack,
    }
}

/// 把 `tool_apply` / `tool_probe` / `tool_use` 三类事件按工具聚合。
///
/// **不做任何「该下架」的判断** —— 只出数字。判断需要的东西这里没有：这台机器不代表全体客户，
/// 而且「没人用」也可能是因为「配不上所以没法用」，因果得人来看。
fn build_toolstack(evs: &[Value], notes: &mut Vec<String>) -> Vec<ToolRow> {
    let mut map: HashMap<String, ToolRow> = HashMap::new();
    // 用过的天数要去重，单独攒
    let mut use_days: HashMap<String, Vec<String>> = HashMap::new();

    for e in evs {
        let tool = strf(e, "tool");
        if tool.is_empty() {
            continue;
        }
        let row = map.entry(tool.clone()).or_insert_with(|| ToolRow { tool: tool.clone(), ..Default::default() });
        match strf(e, "ev").as_str() {
            "tool_apply" => {
                if e.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    row.apply_ok += 1;
                } else {
                    row.apply_fail += 1;
                    let n = strf(e, "note");
                    if !n.is_empty() {
                        row.last_apply_note = n;
                    }
                }
            }
            "tool_probe" => {
                // 只留**最近一次**：可用性是「现在能不能用」，历史失败不该拖着现在
                row.probe_ok = Some(e.get("ok").and_then(Value::as_bool).unwrap_or(false));
                row.probe_ms = u64f(e, "ms");
                row.probe_note = strf(e, "note");
            }
            "tool_use" => {
                row.used += 1;
                let d = day_key(u64f(e, "ts"));
                let v = use_days.entry(tool).or_default();
                if !v.contains(&d) {
                    v.push(d);
                }
            }
            _ => {}
        }
    }
    for (tool, days) in use_days {
        if let Some(r) = map.get_mut(&tool) {
            r.used_days = days.len() as u64;
        }
    }
    let mut out: Vec<ToolRow> = map.into_values().collect();
    // 「最可能该下架的」排前面：没人用的 → 用得少的
    out.sort_by(|a, b| a.used.cmp(&b.used).then(a.tool.cmp(&b.tool)));

    if out.is_empty() {
        notes.push("还没有工具栈数据。点过一次「一键配好全部」、或从内置终端开过工具之后才会有。".into());
    } else if out.iter().all(|r| r.probe_ok.is_none()) {
        notes.push("工具栈里还没有可用性实测数据 —— 那要跑 `--toolstack-probe`，客户机上不会自动跑（每跑一轮都在烧 token）。".into());
    }
    // 这条边界必须每次都说：报告里的「用了几次」天然偏低，别拿它当完整用量下结论
    if out.iter().any(|r| r.used > 0) {
        notes.push("「用了几次」只统计从 U-King 里发起的启动；客户自己在系统终端敲命令的部分我们看不见，真实用量只会更高。".into());
    }
    out
}

// ============================================================
// 建议引擎
// ============================================================
//
// 每条规则的铁律：**必须带得出 evidence**（支撑它的实际数字）。
// 给不出证据的就是猜，宁可不给 —— 客户看到的是「建议」，错了比不给更糟。

fn env_u64(env: &Value, k: &str) -> Option<u64> {
    env.get(k).and_then(Value::as_u64)
}
fn env_bool(env: &Value, k: &str) -> Option<bool> {
    env.get(k).and_then(Value::as_bool)
}
fn env_str(env: &Value, k: &str) -> String {
    env.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn build_advice(models: &[ModelRow], errors: &[ErrorRow], env: &Value) -> Vec<Advice> {
    let mut out: Vec<Advice> = Vec::new();

    // ———— 环境类（env 为 Null 时整段跳过，不猜）————
    if env.is_object() {
        let os = env_str(env, "os");

        // ★ 系统盘空间 —— 仓库里 install_failed 的头号解释变量
        if let (Some(free), Some(total)) = (env_u64(env, "sys_disk_free_mb"), env_u64(env, "sys_disk_total_mb")) {
            if total > 0 && free < 12_000 {
                let sev = if free < 5_000 { "high" } else { "medium" };
                out.push(Advice {
                    id: "disk_low",
                    severity: sev,
                    title: "系统盘快满了，装 AI 工具大概率会失败".into(),
                    detail: "openclaw / hermes 一次要装几个 G。建议先清出 15 GB 以上再装；「安全卸载」页能逐项清理已知足迹。".into(),
                    evidence: format!("系统盘可用 {:.1} GB / 共 {:.0} GB", free as f64 / 1024.0, total as f64 / 1024.0),
                    action: None,
                });
            }
        }

        // Git 缺失 —— Claude Code 的 Bash 工具刚需
        if env_str(env, "git_ver").is_empty() {
            out.push(Advice {
                id: "git_missing",
                severity: "high",
                title: "没装 Git，Claude Code 的命令行工具用不了".into(),
                detail: "「一键优化」会装便携 Git（含 bash.exe），免管理员、不污染系统。".into(),
                evidence: "探测不到 git 版本".into(),
                action: Some("optimize"),
            });
        }
        // Node 缺失 / 过老
        let node = env_str(env, "node_ver");
        if node.is_empty() {
            out.push(Advice {
                id: "node_missing",
                severity: "high",
                title: "没装 Node，AI 命令行工具装不上".into(),
                detail: "「一键优化」会装便携 Node 到 ~/.uking/runtime，免管理员。".into(),
                evidence: "探测不到 node 版本".into(),
                action: Some("optimize"),
            });
        } else if let Some(major) = node.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
            if major < 18 {
                out.push(Advice {
                    id: "node_old",
                    severity: "medium",
                    title: "Node 版本太老，新版 AI 工具会装不上".into(),
                    detail: "Claude Code / Codex CLI 要求 Node 18+。「一键优化」可以装一份便携新版，不动你系统里那个。".into(),
                    evidence: format!("当前 Node {node}，低于要求的 18"),
                    action: Some("optimize"),
                });
            }
        }

        // 家目录被 OneDrive 接管 —— 装机高频炸点
        if env_bool(env, "home_onedrive") == Some(true) {
            out.push(Advice {
                id: "home_onedrive",
                severity: "high",
                title: "家目录在 OneDrive 里，AI 工具会被同步搞坏".into(),
                detail: "OneDrive 会在后台锁文件、按需下载占位符，node_modules 这种几万小文件的目录首当其冲。建议把 AI 工具装到非同步盘。".into(),
                evidence: "检测到家目录/桌面被 OneDrive 重定向".into(),
                action: None,
            });
        }
        // 长路径没开
        if os == "windows" && env_bool(env, "long_paths") == Some(false) {
            out.push(Advice {
                id: "long_paths_off",
                severity: "medium",
                title: "Windows 长路径没开，深层依赖会装失败".into(),
                detail: "npm 的嵌套依赖很容易超过 260 字符。「一键优化」里的修复项会开这个开关（需要一次 UAC），可回滚。".into(),
                evidence: "LongPathsEnabled = 0".into(),
                action: Some("fix"),
            });
        }
        // 中文/空格路径
        if env_bool(env, "path_nonascii") == Some(true) {
            out.push(Advice {
                id: "path_nonascii",
                severity: "medium",
                title: "用户名含中文，部分 AI 工具会报路径错".into(),
                detail: "这是已知坑：个别工具链对非 ASCII 路径处理不干净。装到英文路径的盘上可以绕开；遇到装不上先怀疑这条。".into(),
                evidence: "家目录路径含非 ASCII 字符".into(),
                action: None,
            });
        }
        // Defender 实时防护
        if env_bool(env, "defender_rt") == Some(true) {
            out.push(Advice {
                id: "defender_rt",
                severity: "low",
                title: "Defender 实时防护开着，装包会明显变慢".into(),
                detail: "npm 装几万个小文件时每个都要过一次扫描。「一键优化」可以给 AI 工具目录加排除项（只加目录白名单，不关防护）。".into(),
                evidence: "实时防护已启用".into(),
                action: Some("defender"),
            });
        }
        // ★ PowerShell 5.1 —— AI 在 Windows 上最大的隐形时间黑洞
        //
        // 客户不会报「转义有问题」，他只会觉得「AI 好笨，同一条命令试三次」。
        // 实际是：agent 先写 Bash → 报错 → 套一层 `powershell -Command` → 转义层层叠加；
        // 加上 5.1 默认 GBK 输出，AI 读到乱码判断不了成败，于是再试一次。
        // 每一轮重试都是真金白银的 token + 几十秒等待，而且**不会出现在任何报错统计里**。
        if os == "windows" {
            let pm = env_u64(env, "pwsh_major").unwrap_or(0);
            if pm > 0 && pm < 7 {
                out.push(Advice {
                    id: "pwsh_old",
                    severity: "medium",
                    title: "只有 PowerShell 5.1，AI 跑命令容易在引号和编码上反复翻车".into(),
                    detail: "5.1 默认 GBK 输出，AI 读到乱码判断不了命令成没成，就会重试；\
                             转义规则也和 7 不同，容易越套越深。「一键优化」装的是**便携** PowerShell 7，\
                             免管理员、不动你系统里那个。"
                        .into(),
                    evidence: format!("当前 PowerShell 主版本 {pm}（建议 7+）"),
                    action: Some("optimize"),
                });
            }
        }
    }

    // ———— 用量类 ————

    // 缓存命中率低 = 在为同一段上下文反复付全价
    for m in models.iter().filter(|m| m.calls >= 200) {
        let cached = m.cache_read;
        let total = m.cache_read + m.cache_write + m.input;
        if total == 0 {
            continue;
        }
        let ratio = cached as f64 / total as f64;
        if ratio < 0.70 {
            out.push(Advice {
                id: "cache_low",
                severity: "medium",
                title: format!("{} 的上下文缓存命中偏低，在重复付全价", m.model),
                detail: "常见原因：会话开得太碎、频繁清空上下文、或每次都换项目目录。同一个任务尽量在一个会话里连续做完，缓存才吃得上。".into(),
                evidence: format!(
                    "缓存命中 {:.0}%（{} 次调用中，缓存读 {} / 非缓存输入 {} token）",
                    ratio * 100.0,
                    m.calls,
                    cached,
                    m.input
                ),
                action: None,
            });
            break; // 只报最严重的一个，别刷屏
        }
    }

    // 同一个错反复出现 = 在修错的地方（宪法第 7 条）
    if let Some(e) = errors.iter().find(|e| e.count >= 3) {
        out.push(Advice {
            id: "error_repeat",
            severity: "high",
            title: "同一个错误在反复出现，不是偶然".into(),
            detail: "反复回来的 bug 通常说明修的地方不对。把这条的原文发给客服/AI 助手，比自己再试一次有用。".into(),
            evidence: format!("「{}」近期出现 {} 次", e.last_msg, e.count),
            action: None,
        });
    }

    // 高的排前面
    let rank = |s: &str| match s {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    };
    out.sort_by_key(|a| rank(a.severity));
    out
}

fn build_compare(evs: &[Value], anchor: &Value, ats: u64) -> Compare {
    let mut before_days = std::collections::HashSet::new();
    let mut after_days = std::collections::HashSet::new();
    // (calls, tokens, errors) 分别按工具累加
    let mut b: HashMap<String, (u64, u64, u64)> = HashMap::new();
    let mut a: HashMap<String, (u64, u64, u64)> = HashMap::new();

    for e in evs {
        let ts = u64f(e, "ts");
        let is_after = ts >= ats;
        let tool = strf(e, "tool");
        let side = if is_after { &mut a } else { &mut b };
        match strf(e, "ev").as_str() {
            "usage" => {
                let day = strf(e, "day");
                if is_after {
                    after_days.insert(day);
                } else {
                    before_days.insert(day);
                }
                let s = side.entry(tool).or_insert((0, 0, 0));
                s.0 += u64f(e, "calls");
                s.1 += u64f(e, "in") + u64f(e, "out");
            }
            "error" => {
                let s = side.entry(tool).or_insert((0, 0, 0));
                s.2 += 1;
            }
            _ => {}
        }
    }

    let bd = before_days.len() as u32;
    let ad = after_days.len() as u32;

    let mut tools: Vec<String> = b.keys().chain(a.keys()).cloned().collect();
    tools.sort();
    tools.dedup();

    let rows = tools
        .into_iter()
        .filter(|t| !t.is_empty())
        .map(|tool| {
            let (bc, bt, be) = b.get(&tool).copied().unwrap_or((0, 0, 0));
            let (ac, at, ae) = a.get(&tool).copied().unwrap_or((0, 0, 0));
            let epd_b = per(be, bd as u64);
            let epd_a = per(ae, ad as u64);
            let tpc_b = per(bt, bc);
            let tpc_a = per(at, ac);
            CompareRow {
                tool,
                errors_per_day_before: epd_b,
                errors_per_day_after: epd_a,
                tokens_per_call_before: tpc_b,
                tokens_per_call_after: tpc_a,
                errors_delta_pct: pct(epd_b, epd_a),
                tokens_delta_pct: pct(tpc_b, tpc_a),
            }
        })
        .collect();

    Compare {
        anchor_ts: ats,
        recipes: anchor
            .get("recipes")
            .and_then(Value::as_array)
            .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        before_days: bd,
        after_days: ad,
        sufficient: bd >= MIN_DAYS_EACH_SIDE && ad >= MIN_DAYS_EACH_SIDE,
        rows,
    }
}

fn per(n: u64, d: u64) -> f64 {
    if d == 0 {
        0.0
    } else {
        (n as f64) / (d as f64)
    }
}

/// 变化百分比。**基线为 0 就返回 None** —— 从 0 到任何值都不是「涨了 N%」，编不得。
fn pct(before: f64, after: f64) -> Option<f64> {
    if before <= f64::EPSILON {
        return None;
    }
    Some(((after - before) / before) * 100.0)
}

// ============================================================
// 维护
// ============================================================

/// 删掉超过 `KEEP_MONTHS` 个月的历史文件。启动时调一次即可。
pub fn prune() {
    let dir = metrics_dir();
    let now = now_secs();
    let (cy, cm, _) = ymd(now);
    let cur = cy * 12 + i64::from(cm);
    let rd = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // events-YYYY-MM.jsonl
        let stem = match name.strip_prefix("events-").and_then(|s| s.strip_suffix(".jsonl")) {
            Some(s) => s,
            None => continue,
        };
        let mut it = stem.split('-');
        let y: i64 = match it.next().and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => continue,
        };
        let m: i64 = match it.next().and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => continue,
        };
        if cur - (y * 12 + m) > KEEP_MONTHS {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_groups_same_error_across_machines() {
        // 只有数字不同 = 同一个错，必须同签名（否则「系统盘不足」永远聚不出规律）
        let a = error_signature("系统盘空间不足：仅剩 416 MB，安装需要 2 GB");
        let b = error_signature("系统盘空间不足：仅剩 413 MB，安装需要 2 GB");
        assert_eq!(a, b, "只差数字的同类错误必须同签名");
        // 不同的错必须不同签名
        let c = error_signature("写入驱动配置失败 provider=ollama");
        assert_ne!(a, c, "不同错误不能撞签名");
        assert_eq!(a.len(), 8, "签名固定 8 位十六进制");
    }

    #[test]
    fn signature_ignores_path_noise() {
        let a = error_signature(r"npm error C:\Users\aaa\node_modules 装不上");
        let b = error_signature(r"npm error D:\Users\bbb\node_modules 装不上");
        assert_eq!(a, b, "路径差异不该产生不同签名");
    }

    #[test]
    fn pct_refuses_to_fabricate_on_zero_baseline() {
        assert_eq!(None, pct(0.0, 5.0), "基线为 0 时不许编百分比");
        let v = pct(10.0, 5.0).unwrap();
        assert!((v + 50.0).abs() < 1e-9, "10→5 应该是 -50%，实际 {v}");
    }

    #[test]
    fn ymd_matches_known_dates() {
        assert_eq!((1970, 1, 1), ymd(0));
        // 2026-07-29T00:00:00Z = 1785283200
        assert_eq!((2026, 7, 29), ymd(1_785_283_200));
    }

    #[test]
    fn consent_defaults_to_no_upload() {
        // 默认必须是不上传 —— 这条塌了就是隐私事故
        assert!(!Consent::default().upload);
    }

    // ———————— before / after 对比（会变成「大字结论」的地方，必须钉死）————————

    const DAY: u64 = 86_400;

    fn usage_ev(ts: u64, tool: &str, calls: u64, tokens: u64) -> Value {
        json!({"v":1,"ts":ts,"ev":"usage","day":day_key(ts),"tool":tool,
               "model":"m","calls":calls,"in":tokens,"out":0,
               "cache_read":0,"cache_write":0})
    }
    fn error_ev(ts: u64, tool: &str) -> Value {
        json!({"v":1,"ts":ts,"ev":"error","kind":"install_failed","tool":tool,
               "sig":"deadbeef","msg":"炸了"})
    }
    fn anchor_ev(ts: u64) -> Value {
        json!({"v":1,"ts":ts,"ev":"optimize","recipes":["fix"],
               "score_before":60,"score_after":90,"env":{}})
    }

    /// 优化前每天 2 个错、优化后每天 1 个错 → 必须算出 -50%，且样本足够。
    #[test]
    fn compare_splits_on_anchor_and_computes_delta() {
        let t0 = 1_700_000_000u64;
        let anchor_at = t0 + 4 * DAY;
        let mut evs = vec![anchor_ev(anchor_at)];
        // 前 4 天：每天 1 次 usage + 2 个错
        for d in 0..4 {
            evs.push(usage_ev(t0 + d * DAY, "claude", 10, 1000));
            evs.push(error_ev(t0 + d * DAY, "claude"));
            evs.push(error_ev(t0 + d * DAY, "claude"));
        }
        // 后 4 天：每天 1 次 usage + 1 个错
        for d in 4..8 {
            evs.push(usage_ev(t0 + d * DAY, "claude", 10, 1000));
            evs.push(error_ev(t0 + d * DAY, "claude"));
        }
        let r = report_from(&evs, 30, false, &Value::Null);
        let c = r.compare.expect("有锚点就必须有对比");
        assert!(c.sufficient, "前后各 4 天，应当判为样本足够");
        assert_eq!(4, c.before_days);
        assert_eq!(4, c.after_days);
        let row = c.rows.iter().find(|x| x.tool == "claude").expect("要有 claude 这一行");
        assert!((row.errors_per_day_before - 2.0).abs() < 1e-9, "前: {}", row.errors_per_day_before);
        assert!((row.errors_per_day_after - 1.0).abs() < 1e-9, "后: {}", row.errors_per_day_after);
        let d = row.errors_delta_pct.expect("基线非 0，应当算得出");
        assert!((d + 50.0).abs() < 1e-9, "2/天 → 1/天 应为 -50%，实际 {d}");
    }

    /// 样本不够时**不许**声称结论 —— sufficient=false 且 notes 里要说清楚。
    #[test]
    fn compare_refuses_conclusion_when_samples_thin() {
        let t0 = 1_700_000_000u64;
        let evs = vec![
            usage_ev(t0, "claude", 10, 1000),
            anchor_ev(t0 + DAY),
            usage_ev(t0 + DAY, "claude", 10, 1000),
        ];
        let r = report_from(&evs, 30, false, &Value::Null);
        let c = r.compare.expect("有锚点");
        assert!(!c.sufficient, "每侧只有 1 天，绝不能判为样本足够");
        assert!(
            r.notes.iter().any(|n| n.contains("至少")),
            "样本不足必须在 notes 里说人话，实际 notes={:?}",
            r.notes
        );
    }

    /// 没有锚点 = 给不出对比，而且要明说原因，不能默默返回空。
    #[test]
    fn no_anchor_means_no_comparison_but_says_why() {
        let evs = vec![usage_ev(1_700_000_000, "claude", 10, 1000)];
        let r = report_from(&evs, 30, false, &Value::Null);
        assert!(r.compare.is_none());
        assert!(
            r.notes.iter().any(|n| n.contains("一键优化")),
            "没有锚点要说明原因，实际 notes={:?}",
            r.notes
        );
    }

    // ———————— 建议引擎 ————————

    fn model(tool: &str, name: &str, calls: u64, input: u64, cache_read: u64) -> ModelRow {
        ModelRow {
            tool: tool.into(),
            model: name.into(),
            calls,
            input,
            cache_read,
            ..Default::default()
        }
    }

    /// 没有环境指纹（Null）时，环境类建议必须**一条都不出** —— 宁可少给，不猜。
    #[test]
    fn advice_stays_silent_without_fingerprint() {
        let a = build_advice(&[], &[], &Value::Null);
        assert!(a.is_empty(), "没指纹就不该有环境类建议，实际 {a:?}", a = a.len());
    }

    /// 每条建议都必须带得出 evidence —— 没证据的建议就是猜。
    #[test]
    fn every_advice_carries_evidence() {
        let env = json!({
            "os": "windows", "sys_disk_free_mb": 2000u64, "sys_disk_total_mb": 480_000u64,
            "git_ver": "", "node_ver": "", "long_paths": false,
            "path_nonascii": true, "home_onedrive": true, "defender_rt": true,
        });
        let errs = vec![ErrorRow {
            sig: "deadbeef".into(),
            kind: "install_failed".into(),
            tool: "hermes".into(),
            count: 5,
            last_msg: "系统盘空间不足".into(),
        }];
        let a = build_advice(&[], &errs, &env);
        assert!(a.len() >= 6, "这套环境该触发多条建议，实际 {}", a.len());
        for x in &a {
            assert!(!x.evidence.trim().is_empty(), "建议 {} 没带证据", x.id);
            assert!(!x.title.trim().is_empty(), "建议 {} 没标题", x.id);
        }
        // high 必须排在 medium/low 前面
        let sev: Vec<&str> = a.iter().map(|x| x.severity).collect();
        let first_med = sev.iter().position(|s| *s == "medium").unwrap_or(sev.len());
        assert!(
            !sev[first_med..].contains(&"high"),
            "high 必须排在前面，实际顺序 {sev:?}"
        );
    }

    /// 磁盘充裕就不该报「快满了」—— 阈值错了会天天误报，比不报更糟。
    #[test]
    fn disk_advice_only_fires_when_actually_low() {
        let roomy = json!({"os":"windows","sys_disk_free_mb":200_000u64,"sys_disk_total_mb":480_000u64,
                           "git_ver":"2.49.0","node_ver":"22.14.0","long_paths":true,
                           "path_nonascii":false,"home_onedrive":false,"defender_rt":false});
        let a = build_advice(&[], &[], &roomy);
        assert!(!a.iter().any(|x| x.id == "disk_low"), "磁盘够用不该报警");
        let tight = json!({"os":"windows","sys_disk_free_mb":3_000u64,"sys_disk_total_mb":480_000u64,
                           "git_ver":"2.49.0","node_ver":"22.14.0","long_paths":true,
                           "path_nonascii":false,"home_onedrive":false,"defender_rt":false});
        let b = build_advice(&[], &[], &tight);
        let d = b.iter().find(|x| x.id == "disk_low").expect("只剩 3G 必须报");
        assert_eq!("high", d.severity, "低于 5G 应判 high");
        assert!(d.evidence.contains("GB"), "证据要带具体数字: {}", d.evidence);
    }

    /// PowerShell 建议只在「探到了、且低于 7」时给。
    /// 探不到（0）绝不能当成「版本旧」——那是把探测失败说成客户机器有问题。
    #[test]
    fn pwsh_advice_distinguishes_old_from_unknown() {
        let base = |pm: u64| {
            json!({"os":"windows","sys_disk_free_mb":200_000u64,"sys_disk_total_mb":480_000u64,
                   "git_ver":"2.49.0","node_ver":"22.14.0","long_paths":true,"pwsh_major":pm,
                   "path_nonascii":false,"home_onedrive":false,"defender_rt":false})
        };
        let old = build_advice(&[], &[], &base(5));
        let a = old.iter().find(|x| x.id == "pwsh_old").expect("5.1 该建议升级");
        assert!(a.evidence.contains('5'), "证据要带版本号: {}", a.evidence);
        // 已经是 7：不该报
        assert!(!build_advice(&[], &[], &base(7)).iter().any(|x| x.id == "pwsh_old"));
        // 探不到：不该报（探测失败 ≠ 版本旧）
        assert!(!build_advice(&[], &[], &base(0)).iter().any(|x| x.id == "pwsh_old"));
        // 非 Windows 无此概念
        let mac = json!({"os":"macos","pwsh_major":0u64});
        assert!(!build_advice(&[], &[], &mac).iter().any(|x| x.id == "pwsh_old"));
    }

    /// 缓存命中高就不该建议；低且样本足才建议。
    #[test]
    fn cache_advice_respects_threshold_and_sample_size() {
        // 命中 97%：不该报
        let good = vec![model("claude", "opus", 8633, 275_539, 2_488_974_216)];
        assert!(!build_advice(&good, &[], &Value::Null).iter().any(|x| x.id == "cache_low"));
        // 命中低但只有 10 次调用：样本太小，不该报
        let thin = vec![model("claude", "opus", 10, 900_000, 100_000)];
        assert!(!build_advice(&thin, &[], &Value::Null).iter().any(|x| x.id == "cache_low"));
        // 命中 10% 且 500 次调用：该报
        let bad = vec![model("claude", "opus", 500, 900_000, 100_000)];
        let a = build_advice(&bad, &[], &Value::Null);
        let c = a.iter().find(|x| x.id == "cache_low").expect("命中低且样本足，应当建议");
        assert!(c.evidence.contains("500"), "证据要带调用次数: {}", c.evidence);
    }

    /// 偶发一次的错不算「反复」，3 次才算（宪法第 7 条：反复回来 = 在修错的地方）。
    #[test]
    fn repeat_error_advice_needs_repetition() {
        let once = vec![ErrorRow {
            sig: "a".into(), kind: "k".into(), tool: "t".into(), count: 1, last_msg: "偶发".into(),
        }];
        assert!(!build_advice(&[], &once, &Value::Null).iter().any(|x| x.id == "error_repeat"));
        let thrice = vec![ErrorRow {
            sig: "a".into(), kind: "k".into(), tool: "t".into(), count: 3, last_msg: "又炸了".into(),
        }];
        let a = build_advice(&[], &thrice, &Value::Null);
        let e = a.iter().find(|x| x.id == "error_repeat").expect("3 次应当报");
        assert!(e.evidence.contains('3'), "证据要带次数: {}", e.evidence);
    }

    /// 分工具是刚需：claude 和 codex 的结论不能混成一锅。
    #[test]
    fn compare_keeps_tools_separate() {
        let t0 = 1_700_000_000u64;
        let anchor_at = t0 + 3 * DAY;
        let mut evs = vec![anchor_ev(anchor_at)];
        for d in 0..3 {
            evs.push(usage_ev(t0 + d * DAY, "claude", 10, 1000));
            evs.push(usage_ev(t0 + d * DAY, "codex", 10, 1000));
            evs.push(error_ev(t0 + d * DAY, "claude")); // 只有 claude 报错
        }
        for d in 3..6 {
            evs.push(usage_ev(t0 + d * DAY, "claude", 10, 1000));
            evs.push(usage_ev(t0 + d * DAY, "codex", 10, 1000));
        }
        let r = report_from(&evs, 30, false, &Value::Null);
        let c = r.compare.unwrap();
        let cl = c.rows.iter().find(|x| x.tool == "claude").unwrap();
        let cx = c.rows.iter().find(|x| x.tool == "codex").unwrap();
        assert!(cl.errors_per_day_before > 0.0, "claude 优化前应有错误率");
        assert_eq!(0.0, cx.errors_per_day_before, "codex 没报过错，不该被 claude 的错污染");
        assert_eq!(None, cx.errors_delta_pct, "基线为 0 的工具不许给百分比");
    }
}
