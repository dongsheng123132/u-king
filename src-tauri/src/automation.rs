//! 自动化（定时任务）—— 「到点了，把这句话交给 AI 去干」，结果落盘、可回看。
//!
//! ## 独立可插拔（宪法第 12 条）
//! 本模块**不认识** `AppHandle`，也不 import `agent` / `providers` / 任何别的功能模块：
//! 「到点了具体怎么干」由组合根 `lib.rs` 注入一个 [`Runner`] 回调。
//! **删掉本模块只动两处**：`lib.rs`（去 mod + command + 动作登记）和前端（去 AutomationPanel）。
//!
//! ## 诚实边界：别把它说成「系统级计划任务」
//! 调度线程活在本进程里 —— **U-King 开着（含缩在托盘里）才会到点执行**。
//! 进程没起时错过的班次**不补跑**，只会把 `next_run_at` 推到下一个未来班次。
//! 这条限制以 `runs_only_while_app_open: true` 出现在只读动作的输出里，
//! 让「前端文案 / CLI / AI 通过 MCP 读到的」是同一份事实，而不是三份可以各自跑偏的说法。
//! （对齐 podapp 的 `portable_available:false`：产品边界属于数据，不属于某一个界面的文案。）
//!
//! ## 时间怎么算（为什么不引 chrono）
//! 体积红线不让引 chrono。这里只需要「本机墙上钟的**当天已过秒数** + **星期几**」，
//! 两行系统调用就够（Windows `GetLocalTime` / Unix `localtime_r`）。
//! 所有排期都算成**相对「现在」的秒数差**，再加到当前时间戳上 ——
//! 全程不做「时间戳 ↔ 日历」换算，也就没有时区换算和闰秒的坑。
//!
//! 纯 std + serde_json，照抄 `tasks.rs` 的 `~/.uking/` 落盘范式，零新依赖。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 「到点了，去干这件事」。真正怎么干（调哪个大脑、怎么拿 Key）由组合根注入。
/// 返回 `Ok(正文)` / `Err(人话错误)`；两者都会原样落进这一次的运行记录。
pub type Runner = Box<dyn Fn(&Job) -> Result<String, String> + Send + Sync + 'static>;

/// 「跑完了，去告诉人一声」。参数 = (任务, 成功与否, 一行摘要)。
///
/// **为什么必须有它**：本模块跑完只写 `~/.uking/automation/*.md` 和列表里的 `last_message`，
/// 客户不打开那个面板就永远不知道跑没跑过 —— 而定时任务卖的恰恰是「你不用盯着」。
/// 跑完不吭声，等于还得盯着，功能定位自相矛盾。
///
/// 同 [`Runner`]：本模块**不认识** `AppHandle`，怎么通知（emit 前端事件 / 改托盘提示）
/// 由组合根 `lib.rs` 注入。没注入就是不通知（无头 CLI 就该这样，别往 stdout 里掺东西）。
pub type Notifier = Box<dyn Fn(&Job, bool, &str) + Send + Sync + 'static>;

/// 条数上限。定时任务是**会花钱**的（每次跑都在烧 token），不设上限等于给自己埋雷。
pub const MAX_JOBS: usize = 30;
/// 间隔型最短 5 分钟：再短就不是「自动化」，是压测自己的账户。
const MIN_INTERVAL_MIN: i64 = 5;
/// 每个任务保留的运行记录条数（超出删最旧的）。
const KEEP_RUNS: usize = 20;
/// 长程记忆总长上限（字符）。8KB 每次注入够上下文用，也防无限膨胀。
const MEMORY_MAX_CHARS: usize = 8_000;
/// 单轮取输出尾部多少字符当「结论」（长输出的结论通常落尾部）。
const MEMORY_TAIL_CHARS: usize = 1_500;
/// 调度线程的心跳。20s 够细（最小排期粒度是分钟），又不至于空转发烫。
const TICK_SECS: u64 = 20;

// ───────────────────────── 数据 ─────────────────────────

/// 排期。三种，够覆盖「每天早报 / 每周周报 / 每隔一会儿看一眼」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// `interval` | `daily` | `weekly`
    pub kind: String,
    /// interval：每隔几分钟
    #[serde(default)]
    pub minutes: i64,
    /// daily/weekly：几点几分，`"HH:MM"`，**本机时间**
    #[serde(default)]
    pub at: String,
    /// weekly：星期几，0=周日 … 6=周六
    #[serde(default)]
    pub weekdays: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    /// 显示名（「每天早报」）
    pub name: String,
    /// 到点发给大脑的那句话
    pub prompt: String,
    /// 用哪个大脑：`uking`（自家助手，直连虾盘云，默认）/ `claude` / `codex`
    #[serde(default = "default_engine")]
    pub engine: String,
    /// 工作文件夹。**空 = 只放行「作图/视频」这类零风险工具**；
    /// 填了才等于用户明确授权它在这个文件夹里读写文件、跑命令（前端必须把这句说清楚）。
    #[serde(default)]
    pub dir: String,
    pub schedule: Schedule,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: i64,
    /// 下次执行的时间戳（ms）。**唯一真相源在后端** —— 前端只显示，不自己算。
    #[serde(default)]
    pub next_run_at: i64,
    #[serde(default)]
    pub last_run_at: i64,
    /// 上次跑成没成。从没跑过 = None（和「跑过但失败」是两件事，别混成 false）。
    #[serde(default)]
    pub last_ok: Option<bool>,
    /// 上次结果摘要（截断），列表里直接看
    #[serde(default)]
    pub last_message: String,
    /// 上次完整结果落盘的文件名（在 `~/.uking/automation/` 下，只存文件名不存全路径）
    #[serde(default)]
    pub last_run_file: String,
    #[serde(default)]
    pub runs: i64,
    /// 长程记忆：开 = 下一班把上一班的进度/结论夹进 prompt 接着干（见 `with_memory` / `append_memory`）。
    /// **默认关** —— 不开这条，现有任务行为一字不变；这是「长程办公」的开关，不是默认值。
    #[serde(default)]
    pub use_memory: bool,
}

fn default_engine() -> String {
    "uking".into()
}
fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    jobs: Vec<Job>,
}

impl Default for StoreFile {
    fn default() -> Self {
        Self { version: 1, jobs: Vec::new() }
    }
}

// ───────────────────────── 落盘 ─────────────────────────

/// `~/.uking`。**认 `UKING_TEST_HOME` 沙箱**（同 providers.rs 的 `config_home`）——
/// 开发机上验增删改必须能跑，又不能把自己真实的定时任务改掉。
fn uking_home() -> PathBuf {
    let home = std::env::var("UKING_TEST_HOME")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".uking")
}

fn store_path() -> PathBuf {
    uking_home().join("automations.json")
}

/// 运行记录目录 `~/.uking/automation/`。
pub fn runs_dir() -> PathBuf {
    uking_home().join("automation")
}

/// 读写锁：调度线程和 GUI 会同时改这份文件，别让它们把对方的写覆盖掉。
fn lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_store() -> StoreFile {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str::<StoreFile>(&s).ok())
        .unwrap_or_default()
}

fn write_store(f: &StoreFile) -> Result<(), String> {
    let _ = std::fs::create_dir_all(uking_home());
    let s = serde_json::to_string_pretty(f).map_err(|e| format!("序列化自动化失败: {e}"))?;
    std::fs::write(store_path(), s).map_err(|e| format!("写入 automations.json 失败: {e}"))
}

// ───────────────────────── 本机墙上钟 ─────────────────────────

/// 返回 (当天已过秒数, 星期几)；星期几 0=周日 … 6=周六。拿不到就退化成 (0,0)。
#[cfg(windows)]
fn local_clock() -> (i64, i64) {
    #[repr(C)]
    struct SysTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        ms: u16,
    }
    #[allow(non_snake_case)]
    extern "system" {
        fn GetLocalTime(t: *mut SysTime);
    }
    let mut t = SysTime { year: 0, month: 0, day_of_week: 0, day: 0, hour: 0, minute: 0, second: 0, ms: 0 };
    unsafe { GetLocalTime(&mut t) };
    ((t.hour as i64) * 3600 + (t.minute as i64) * 60 + t.second as i64, t.day_of_week as i64)
}

#[cfg(not(windows))]
fn local_clock() -> (i64, i64) {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const i8,
    }
    extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, tm: *mut Tm) -> *mut Tm;
    }
    unsafe {
        let now: i64 = time(std::ptr::null_mut());
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&now, &mut tm).is_null() {
            return (0, 0);
        }
        ((tm.hour as i64) * 3600 + (tm.min as i64) * 60 + tm.sec as i64, tm.wday as i64)
    }
}

/// `"HH:MM"` → 当天秒数。给不出合法值就是错误（宁可拒绝，也不要静默按 00:00 排期）。
fn parse_hhmm(s: &str) -> Result<i64, String> {
    let (h, m) = s.split_once(':').ok_or_else(|| format!("时间要写成 HH:MM，收到「{s}」"))?;
    let h: i64 = h.trim().parse().map_err(|_| format!("小时不是数字：「{s}」"))?;
    let m: i64 = m.trim().parse().map_err(|_| format!("分钟不是数字：「{s}」"))?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return Err(format!("时间超范围：「{s}」"));
    }
    Ok(h * 3600 + m * 60)
}

/// 算下一次执行时间（ms）。**永远返回一个严格在「现在」之后的时刻** ——
/// 排期全靠「相对现在的秒数差」，不做时间戳↔日历换算。
pub fn next_after_now(s: &Schedule) -> i64 {
    let now = now_ms();
    let (sod, wday) = local_clock();
    match s.kind.as_str() {
        "interval" => now + s.minutes.max(MIN_INTERVAL_MIN) * 60_000,
        "daily" => {
            let t = parse_hhmm(&s.at).unwrap_or(9 * 3600);
            let mut d = t - sod;
            if d <= 0 {
                d += 86_400;
            }
            now + d * 1000
        }
        "weekly" => {
            let t = parse_hhmm(&s.at).unwrap_or(9 * 3600);
            for i in 0..8i64 {
                let wd = (wday + i) % 7;
                if !s.weekdays.contains(&wd) {
                    continue;
                }
                let d = i * 86_400 + (t - sod);
                if d > 0 {
                    return now + d * 1000;
                }
            }
            now + 7 * 86_400 * 1000
        }
        // 走到这里说明 validate 漏了；给一个明天的时刻总好过 0（0 = 永远待执行 = 每 tick 都跑）
        _ => now + 86_400 * 1000,
    }
}

const WEEK_CN: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

/// 排期的人话说法。**一份实现三处用**（GUI 列表、CLI、动作输出），别在前端再写一遍。
pub fn describe(s: &Schedule) -> String {
    match s.kind.as_str() {
        "interval" => {
            let m = s.minutes.max(MIN_INTERVAL_MIN);
            if m % 60 == 0 {
                format!("每 {} 小时", m / 60)
            } else {
                format!("每 {m} 分钟")
            }
        }
        "daily" => format!("每天 {}", s.at),
        "weekly" => {
            let days: Vec<&str> = s
                .weekdays
                .iter()
                .filter_map(|d| WEEK_CN.get(*d as usize).copied())
                .collect();
            format!("每{} {}", days.join("、").replace('周', ""), s.at)
        }
        other => format!("未知排期（{other}）"),
    }
}

// ───────────────────────── 增删改查（纯函数，不碰 AppHandle）─────────────────────────

fn validate(job: &Job) -> Result<(), String> {
    if job.name.trim().is_empty() {
        return Err("给这个自动化起个名字".into());
    }
    if job.prompt.trim().is_empty() {
        return Err("写清楚到点了让 AI 干什么".into());
    }
    if !matches!(job.engine.as_str(), "uking" | "claude" | "codex") {
        return Err(format!("不认识的大脑：{}", job.engine));
    }
    if !job.dir.trim().is_empty() && !Path::new(job.dir.trim()).is_dir() {
        return Err(format!("工作文件夹不存在：{}", job.dir));
    }
    match job.schedule.kind.as_str() {
        "interval" => {
            if job.schedule.minutes < MIN_INTERVAL_MIN {
                return Err(format!("间隔最少 {MIN_INTERVAL_MIN} 分钟"));
            }
        }
        "daily" => {
            parse_hhmm(&job.schedule.at)?;
        }
        "weekly" => {
            parse_hhmm(&job.schedule.at)?;
            if job.schedule.weekdays.is_empty() {
                return Err("每周至少选一天".into());
            }
            if job.schedule.weekdays.iter().any(|d| !(0..=6).contains(d)) {
                return Err("星期取值必须在 0~6".into());
            }
        }
        other => return Err(format!("不认识的计划类型：{other}")),
    }
    Ok(())
}

/// 列出全部（按创建时间倒序）。顺带把「错过的班次」推到下一个未来班次并落盘 ——
/// 这样 GUI 一打开看到的 `next_run_at` 就已经是真的，不用前端自己猜。
pub fn list() -> Vec<Job> {
    let _g = lock().lock();
    let mut f = read_store();
    let now = now_ms();
    let mut dirty = false;
    for j in f.jobs.iter_mut() {
        if j.next_run_at <= now {
            j.next_run_at = next_after_now(&j.schedule);
            dirty = true;
        }
    }
    if dirty {
        let _ = write_store(&f);
    }
    f.jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    f.jobs
}

/// 新增 / 更新（按 id 去重）。幂等：同一个 id 传同样的内容重放，结果一样。
pub fn upsert(mut job: Job) -> Result<Job, String> {
    job.name = job.name.trim().to_string();
    job.dir = job.dir.trim().to_string();
    validate(&job)?;

    let _g = lock().lock();
    let mut f = read_store();
    let now = now_ms();
    if let Some(old) = f.jobs.iter_mut().find(|j| j.id == job.id) {
        // 运行历史属于「机器记的」，不接受前端回传覆盖 —— 否则一次编辑就把跑过几次抹了
        job.created_at = if old.created_at > 0 { old.created_at } else { now };
        job.last_run_at = old.last_run_at;
        job.last_ok = old.last_ok;
        job.last_message = old.last_message.clone();
        job.last_run_file = old.last_run_file.clone();
        job.runs = old.runs;
        job.next_run_at = next_after_now(&job.schedule); // 改了排期就重排
        *old = job.clone();
    } else {
        if f.jobs.len() >= MAX_JOBS {
            return Err(format!("最多 {MAX_JOBS} 条自动化，先删掉几条再加"));
        }
        if job.id.trim().is_empty() {
            job.id = format!("auto-{now}");
        }
        job.created_at = now;
        job.next_run_at = next_after_now(&job.schedule);
        f.jobs.push(job.clone());
    }
    write_store(&f)?;
    Ok(job)
}

/// 删除一条。幂等：不存在也返回 Ok（重放安全）。**不删它的运行记录**——留着可回看。
pub fn remove(id: &str) -> Result<(), String> {
    let _g = lock().lock();
    let mut f = read_store();
    f.jobs.retain(|j| j.id != id);
    write_store(&f)
}

/// 开 / 关。幂等。
pub fn set_enabled(id: &str, on: bool) -> Result<Job, String> {
    let _g = lock().lock();
    let mut f = read_store();
    let job = f.jobs.iter_mut().find(|j| j.id == id).ok_or_else(|| format!("没有这条自动化：{id}"))?;
    job.enabled = on;
    if on {
        job.next_run_at = next_after_now(&job.schedule);
    }
    let out = job.clone();
    write_store(&f)?;
    Ok(out)
}

/// 就地改一条（内部用，改完立即落盘）。
fn patch(id: &str, f: impl FnOnce(&mut Job)) {
    let _g = lock().lock();
    let mut store = read_store();
    if let Some(j) = store.jobs.iter_mut().find(|j| j.id == id) {
        f(j);
        let _ = write_store(&store);
    }
}

fn get(id: &str) -> Option<Job> {
    let _g = lock().lock();
    read_store().jobs.into_iter().find(|j| j.id == id)
}

// ───────────────────────── 运行记录 ─────────────────────────

/// 结果摘要：列表里一眼能看的一行（去掉换行、截断）。
fn summarize(body: &str) -> String {
    let one: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= 120 {
        one
    } else {
        one.chars().take(120).collect::<String>() + "…"
    }
}

/// 把这一次的完整结果写成一个 .md，返回文件名。写失败不算任务失败（只是回看不了）。
fn write_run(job: &Job, ok: bool, body: &str, started: i64) -> String {
    let dir = runs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return String::new();
    }
    let name = format!("{}-{}.md", sanitize(&job.id), started);
    let text = format!(
        "# {}\n\n- 结果：{}\n- 排期：{}\n- 大脑：{}\n- 工作文件夹：{}\n\n## 提示词\n\n{}\n\n## 输出\n\n{}\n",
        job.name,
        if ok { "成功" } else { "失败" },
        describe(&job.schedule),
        job.engine,
        if job.dir.is_empty() { "（无，只放行作图/视频）" } else { &job.dir },
        job.prompt,
        body
    );
    if std::fs::write(dir.join(&name), text).is_err() {
        return String::new();
    }
    prune_runs(&job.id);
    name
}

/// 文件名安全：只留字母数字和 `-_`，其余换成 `_`。防 id 里带路径分隔符写到别处去。
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn prune_runs(job_id: &str) {
    let dir = runs_dir();
    let prefix = format!("{}-", sanitize(job_id));
    let mut mine: Vec<String> = std::fs::read_dir(&dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with(&prefix) && n.ends_with(".md"))
                .collect()
        })
        .unwrap_or_default();
    if mine.len() <= KEEP_RUNS {
        return;
    }
    mine.sort(); // 名字里嵌了时间戳 → 字典序≈时间序（同一 id 下位数相同）
    for old in mine.iter().take(mine.len() - KEEP_RUNS) {
        let _ = std::fs::remove_file(dir.join(old));
    }
}

/// 列一条自动化的历史运行记录（新→旧，只给文件名和时间，不读正文）。
pub fn list_runs(job_id: &str) -> Vec<Value> {
    let prefix = format!("{}-", sanitize(job_id));
    let mut out: Vec<Value> = std::fs::read_dir(runs_dir())
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with(&prefix) && n.ends_with(".md"))
                .map(|n| {
                    let ts: i64 = n
                        .trim_start_matches(&prefix)
                        .trim_end_matches(".md")
                        .parse()
                        .unwrap_or(0);
                    json!({ "file": n, "at": ts })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b["at"].as_i64().unwrap_or(0).cmp(&a["at"].as_i64().unwrap_or(0)));
    out
}

/// 读一条运行记录的正文。**只认 `runs_dir` 下的纯文件名** —— 带路径分隔符的一律拒，
/// 不给「读任意文件」留口子（这个入参最终会从 GUI / CLI / MCP 三个面进来）。
pub fn read_run(file: &str) -> Result<String, String> {
    if file.is_empty() || file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err("非法的运行记录文件名".into());
    }
    std::fs::read_to_string(runs_dir().join(file)).map_err(|e| format!("读不到这条运行记录: {e}"))
}

// ───────────────────────── 长程记忆（上下文一致）─────────────────────────

/// 长程记忆文件：`~/.uking/automation/<sanitize(id)>-memory.md`。
/// 与运行记录**同目录、扁平** —— 删除/备份/清理跟运行记录同一套，不另起一套目录结构。
fn memory_path(job_id: &str) -> PathBuf {
    runs_dir().join(format!("{}-memory.md", sanitize(job_id)))
}

/// 读长程记忆。文件本身有上限，读进来即有限。
pub fn read_memory(job_id: &str) -> String {
    std::fs::read_to_string(memory_path(job_id)).unwrap_or_default()
}

/// 拼装下一轮的 prompt：把长程记忆夹在开头当上下文。
/// 没开 `use_memory` / 记忆为空 = **原样返回** —— 这是「默认关」承诺的一部分。
pub fn with_memory(job: &Job, prompt: &str) -> String {
    if !job.use_memory {
        return prompt.to_string();
    }
    let mem = read_memory(&job.id);
    let mem = mem.trim();
    if mem.is_empty() {
        return prompt.to_string();
    }
    format!(
        "【此前各轮的进度与结论，请在此基础上继续，别重复已完成的工作】\n{mem}\n\n【本次任务】\n{prompt}"
    )
}

/// 追加一轮的长程记忆。只取输出尾部（结论通常落尾部），总长被 `MEMORY_MAX_CHARS` 拦住（丢最旧的）。
/// **写失败不算任务失败** —— 记忆丢了只是下一轮没上下文，跟运行记录一样是「回看不了」。
pub fn append_memory(job_id: &str, run_no: i64, ok: bool, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    let tail: String = body
        .chars()
        .rev()
        .take(MEMORY_TAIL_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let entry = format!(
        "\n\n## 第 {run_no} 班（{}）\n{}",
        if ok { "成功" } else { "失败" },
        tail.trim()
    );
    let mut all = read_memory(job_id);
    all.push_str(&entry);
    let excess = all.chars().count().saturating_sub(MEMORY_MAX_CHARS);
    if excess > 0 {
        all = all.chars().skip(excess).collect();
    }
    if std::fs::create_dir_all(runs_dir()).is_err() {
        return;
    }
    let _ = std::fs::write(memory_path(job_id), all);
}

// ───────────────────────── 调度线程 ─────────────────────────

static RUNNER: OnceLock<Runner> = OnceLock::new();
static NOTIFIER: OnceLock<Notifier> = OnceLock::new();
static KEEP_AWAKE: OnceLock<KeepAwake> = OnceLock::new();

/// 「有任务在等下一班时，别让电脑闲着自己睡」。参数 = 该不该抑制。
///
/// 同 [`Runner`] / [`Notifier`]：本模块**不认识** `awake` 模块，怎么抑制由组合根注入
/// —— 删掉那个模块只动 lib.rs。**返回值故意丢弃**：抑制失败（老系统 / 权限）
/// 绝不能影响任务本身跑不跑。
pub type KeepAwake = Box<dyn Fn(bool) + Send + Sync>;

/// 注册「跑完怎么通知人」。只生效一次（重复注册忽略），无头模式不注册 = 不通知。
pub fn set_notifier(n: Notifier) {
    let _ = NOTIFIER.set(n);
}

/// 注册休眠抑制钩子。不注册 = 什么都不做（无头 CLI / 单测走这条）。
pub fn set_keep_awake(k: KeepAwake) {
    let _ = KEEP_AWAKE.set(k);
}

/// 现在**该不该**抑制休眠 = 还有启用的任务在等下一班。
///
/// 🔴 判据是「在等下一班」，**不是「正在跑」**。设计文档原本写的是「班次开始时抑制、
/// 结束时撤销」，那治不了它自己讲的那个故事：客户 23:00 走人、机器睡了，02:00 那班
/// **根本不会开始** —— 等「班次开始」再抑制，那一刻永远不会到来。
/// 所以必须在**空窗期**就把机器摁住。
pub fn should_stay_awake() -> bool {
    let _g = lock().lock();
    read_store().jobs.iter().any(|j| j.enabled && j.next_run_at > 0)
}

/// 把「该不该睡」同步给系统。每 tick 重申一次 —— 幂等，而且任务被删/停用后能自动松手。
fn sync_keep_awake() {
    if let Some(k) = KEEP_AWAKE.get() {
        let want = should_stay_awake();
        // 同 notify：抑制这件事再怎么样也不能把调度线程带走。
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| k(want)));
    }
}

/// 通知一声。**绝不让通知的失败影响任务本身** —— 任务已经跑完落盘了，
/// 通知只是锦上添花；这里 panic 会把调度线程带走，后面所有任务都不再执行。
fn notify(job: &Job, ok: bool, summary: &str) {
    if let Some(n) = NOTIFIER.get() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| n(job, ok, summary)));
    }
}

/// 当前正在跑的那条的 id。定时任务是烧钱操作，**同一时刻只跑一条**。
fn busy() -> &'static Mutex<Option<String>> {
    static B: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(None))
}

pub fn running_id() -> Option<String> {
    busy().lock().ok().and_then(|b| b.clone())
}

/// 起调度线程（只起一次）。组合根在 GUI setup 时调用；无头 CLI 模式**不调** ——
/// 跑一条 `action run` 不该顺手把客户的定时任务全触发一遍。
pub fn start(runner: Runner) {
    if RUNNER.set(runner).is_err() {
        return; // 已经起过了
    }
    std::thread::spawn(|| {
        // 启动先把「关着这段时间错过的班次」推到下一个未来班次。**不补跑** ——
        // 开机一瞬间连烧十条任务的 token，是惊吓不是自动化。
        let _ = list();
        // 🔴 休眠抑制必须在**这条线程**里申请：`SetThreadExecutionState` 按线程记账，
        // 哪条线程调的算哪条线程的，线程一退请求就没了。从 Tauri 命令线程（线程池，
        // 用完就还回去）申请，会设完就没、界面还显示「已开启」——最难查的那种坏。
        // 反过来这也是安全网：进程一退，操作系统自动收回，不会留下「客户电脑从此不睡」。
        sync_keep_awake();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(TICK_SECS));
            tick();
            // 每 tick 重申一次：任务被删 / 停用后能自动松手，不用到处埋撤销点。
            sync_keep_awake();
        }
    });
}

fn tick() {
    let now = now_ms();
    let due: Vec<Job> = {
        let _g = lock().lock();
        read_store()
            .jobs
            .into_iter()
            .filter(|j| j.enabled && j.next_run_at > 0 && j.next_run_at <= now)
            .collect()
    };
    for job in due {
        // 先把 next 推走再跑：跑得久（出片要几分钟）也不会在下一 tick 被重复触发
        patch(&job.id, |j| j.next_run_at = next_after_now(&j.schedule));
        // 失败不抛：execute 已经把成败连同正文写进这条任务的运行记录了。
        // 一条任务跑挂了不能把调度线程带走，否则**后面所有任务都不再执行**，而且没人会发现。
        let _ = execute(&job);
    }
}

/// 真跑一条。同步阻塞（调度线程里天然串行；`run_now` 由组合根丢到 blocking 线程池）。
pub fn execute(job: &Job) -> Result<String, String> {
    let runner = RUNNER.get().ok_or("调度器没启动（无头模式下不起调度线程）")?;
    let occupied = {
        let mut b = busy().lock().map_err(|_| "调度器状态锁坏了")?;
        match b.clone() {
            Some(cur) => Some(cur),
            None => {
                *b = Some(job.id.clone());
                None
            }
        }
    };
    // 撞车了这一班就跳过（下一班照常）。**但必须留痕** —— 没人在看屏幕，
    // 静默跳过就是「客户明早发现没早报，却哪儿都查不到为什么」。
    if let Some(cur) = occupied {
        let msg = format!("另一条自动化正在跑（{cur}），这一班跳过了");
        let file = write_run(job, false, &msg, now_ms());
        patch(&job.id, |j| {
            j.last_ok = Some(false);
            j.last_message = summarize(&msg);
            j.last_run_file = file.clone();
        });
        // 跳过也要说 —— 静默跳过就是「客户明早发现没早报，却哪儿都查不到为什么」
        notify(job, false, &msg);
        return Err(msg);
    }
    let started = now_ms();
    let result = runner(job);
    if let Ok(mut b) = busy().lock() {
        *b = None;
    }
    let (ok, body) = match &result {
        Ok(s) => (true, s.clone()),
        Err(e) => (false, e.clone()),
    };
    let file = write_run(job, ok, &body, started);
    let summary = summarize(&body);
    patch(&job.id, |j| {
        j.last_run_at = started;
        j.last_ok = Some(ok);
        j.last_message = summary.clone();
        j.last_run_file = file.clone();
        j.runs += 1;
    });
    // 长程记忆：把这轮结果追加进记忆，下一班 `with_memory` 时能接着干。
    // `job.runs` 还是跑前值，所以 +1 才是本轮。跳过的那班（上面的早退分支）不进记忆。
    if job.use_memory {
        append_memory(&job.id, job.runs + 1, ok, &body);
    }
    // 落盘之后才通知：通知里带的「看结果」要能真的读到那个文件
    notify(job, ok, &summary);
    result
}

/// 「立即运行一次」。不动排期（下一班照旧），纯粹是让人当场看到效果 —— 演示全靠它。
pub fn run_now(id: &str) -> Result<String, String> {
    let job = get(id).ok_or_else(|| format!("没有这条自动化：{id}"))?;
    execute(&job)
}

// ───────────────────────── 只读状态（影核动作用）─────────────────────────

/// 可用性（readiness）：回答**能不能到点自动跑**，不是「有没有配任务」。
///
/// `key_ok` 由组合根传进来（本模块不 import device/providers）—— 拿不到设备 Key
/// 就是「配了也白配」，必须进 blockers，别让报告绿着而世界是坏的。
/// `awake` = 休眠抑制的现状 + **它治不了什么**，由组合根注入（本模块不认识 `awake` 模块，
/// 同 `breakdown(days, rtk::is_active())` 的手法）。传 `Value::Null` = 这条链路没接。
pub fn status(key_ok: bool, awake: Value) -> Value {
    let jobs = list();
    let enabled: Vec<&Job> = jobs.iter().filter(|j| j.enabled).collect();
    let mut blockers: Vec<String> = Vec::new();
    if !key_ok {
        blockers.push("拿不到设备 Key —— 到点也调不动大脑，先去「AI 设置」配好驱动/充值".into());
    }
    if std::fs::create_dir_all(uking_home()).is_err() {
        blockers.push(format!("写不了 {} —— 自动化存不下来", uking_home().display()));
    }
    let next = enabled
        .iter()
        .filter(|j| j.next_run_at > 0)
        .min_by_key(|j| j.next_run_at)
        .map(|j| json!({ "id": j.id, "name": j.name, "at": j.next_run_at, "schedule": describe(&j.schedule) }));
    json!({
        "ready": blockers.is_empty(),
        "blockers": blockers,
        "count": jobs.len(),
        "enabled": enabled.len(),
        "max": MAX_JOBS,
        // 产品边界当数据发（不是某个界面的文案）：GUI / CLI / MCP 读到的是同一句话
        "runs_only_while_app_open": true,
        // ★ 同上：休眠抑制**挡得住空闲休眠、挡不住合盖**。这句必须跟着数据走，
        // 否则界面上写一句、CLI 里写另一句，客户合盖走人以为有人替他干活。
        "keep_awake": awake,
        "scheduler_started": RUNNER.get().is_some(),
        "running_id": running_id(),
        "next": next,
        "jobs": jobs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sch(kind: &str, minutes: i64, at: &str, weekdays: Vec<i64>) -> Schedule {
        Schedule { kind: kind.into(), minutes, at: at.into(), weekdays }
    }

    #[test]
    fn hhmm_rejects_garbage_instead_of_defaulting_to_midnight() {
        assert!(parse_hhmm("09:30").is_ok());
        assert!(parse_hhmm("24:00").is_err());
        assert!(parse_hhmm("9点半").is_err());
        assert!(parse_hhmm("").is_err());
    }

    /// 排期必须**永远指向未来**。返回 0 或过去的时刻 = 每个 tick 都触发 = 烧光账户。
    #[test]
    fn next_run_is_always_in_the_future() {
        let now = now_ms();
        for s in [
            sch("interval", 5, "", vec![]),
            sch("daily", 0, "00:00", vec![]),
            sch("daily", 0, "23:59", vec![]),
            sch("weekly", 0, "08:00", vec![0, 3]),
            sch("bogus", 0, "", vec![]),
        ] {
            assert!(next_after_now(&s) > now, "排期没指向未来: {s:?}");
        }
    }

    /// 运行记录只认纯文件名 —— 这个入参会从 GUI / CLI / MCP 三个面进来。
    #[test]
    fn read_run_refuses_path_traversal() {
        for bad in ["../../.claude/settings.json", "a/b.md", "a\\b.md", "..", ""] {
            assert!(read_run(bad).is_err(), "居然放行了: {bad}");
        }
    }

    #[test]
    fn validate_catches_the_ways_people_actually_break_it() {
        let base = Job {
            id: "t".into(),
            name: "测试".into(),
            prompt: "写一条文案".into(),
            engine: "uking".into(),
            dir: String::new(),
            schedule: sch("daily", 0, "09:00", vec![]),
            enabled: true,
            created_at: 0,
            next_run_at: 0,
            last_run_at: 0,
            last_ok: None,
            last_message: String::new(),
            last_run_file: String::new(),
            runs: 0,
            use_memory: false,
        };
        assert!(validate(&base).is_ok());
        assert!(validate(&Job { name: "  ".into(), ..base.clone() }).is_err());
        assert!(validate(&Job { prompt: "".into(), ..base.clone() }).is_err());
        assert!(validate(&Job { engine: "gpt5".into(), ..base.clone() }).is_err());
        assert!(validate(&Job { schedule: sch("interval", 1, "", vec![]), ..base.clone() }).is_err());
        assert!(validate(&Job { schedule: sch("weekly", 0, "09:00", vec![]), ..base.clone() }).is_err());
        assert!(validate(&Job { schedule: sch("weekly", 0, "09:00", vec![9]), ..base.clone() }).is_err());
        assert!(validate(&Job { dir: "Z:/绝对不存在的文件夹".into(), ..base }).is_err());
    }

    #[test]
    fn describe_reads_like_a_human_wrote_it() {
        assert_eq!(describe(&sch("interval", 30, "", vec![])), "每 30 分钟");
        assert_eq!(describe(&sch("interval", 120, "", vec![])), "每 2 小时");
        assert_eq!(describe(&sch("daily", 0, "09:00", vec![])), "每天 09:00");
        assert_eq!(describe(&sch("weekly", 0, "18:00", vec![1, 5])), "每一、五 18:00");
    }

    /// 长程记忆默认关：不开 use_memory 的任务，prompt 一字不变（现有任务行为不被改）。
    /// 不碰磁盘，可以独立成一个用例。
    #[test]
    fn with_memory_is_a_noop_when_opt_out() {
        let j = Job { use_memory: false, ..base_job() };
        assert_eq!(with_memory(&j, "原始提示词"), "原始提示词");
    }

    /// 长程记忆全链路：跨轮携带 / 回写可验证 / 断点续跑 / 尾部截断 / 上限裁剪。
    ///
    /// **必须合成一个 `#[test]`**：它们共享 `UKING_TEST_HOME` 这个进程级环境变量，而
    /// cargo 默认多线程跑测试，拆成几个用例会互相踩沙箱（读对方的文件、甚至写进对方目录）。
    ///
    /// 🔴 光合成一个用例**不够** —— 那只挡住了本模块内部。别的模块同时改这个变量照样
    /// 把它踩红（实测：第 3 轮读不到前两轮的事实）。真正的互斥在 `crate::testsandbox`。
    #[test]
    fn long_memory_roundtrip_on_disk() {
        let _sb = crate::testsandbox::enter("automation-memory", &[]);
        let a = Job { id: "mem-a".into(), use_memory: true, ..base_job() };

        // ① 空记忆 = 原样返回
        assert_eq!(with_memory(&a, "开始"), "开始");

        // ② 上下文跨轮存活：第 N 轮的 prompt 必须含前面各轮写进去的事实
        append_memory(&a.id, 1, true, "这一轮我调研了 A。请记住：事实A 已经完成。");
        let p2 = with_memory(&a, "继续");
        assert!(p2.contains("事实A"), "第 2 轮没带上第 1 轮的事实: {p2}");
        append_memory(&a.id, 2, true, "这一轮我做了 B。请记住：事实B 也完成。");
        let p3 = with_memory(&a, "继续");
        assert!(p3.contains("事实A") && p3.contains("事实B"), "第 3 轮丢了前两轮的事实: {p3}");

        // ③ 断点续跑：新会话（新 job，同 id，runs=0）只靠 memory 文件就能接上
        let fresh = Job { id: "mem-a".into(), runs: 0, use_memory: true, ..base_job() };
        let resumed = with_memory(&fresh, "继续");
        assert!(resumed.contains("事实A"), "新会话没接上记忆: {resumed}");

        // ④ 回写可验证 + 尾部截断：结论必须进记忆，开头细节不全塞
        // 2500 个**互不相同**的码点当头部 —— 如果全量塞进，开头必在；只留尾部 1500 的话，
        // 开头 100 个字符必然被丢掉（尾部是后段码点，跟前 100 个码点不重叠）。
        // 🔴 别用 `a..z` 取模序列当「唯一前缀」—— 那是重复模式，开头字符串在尾部照样出现。
        let head: String = (0x1F600u32..(0x1F600 + 2500)).filter_map(char::from_u32).collect();
        let head_first: String = head.chars().take(100).collect();
        let mut body = head.clone();
        body.push_str("结论：改完了，可以发版");
        append_memory(&a.id, 3, true, &body);
        let mem = read_memory(&a.id);
        assert!(mem.contains("结论：改完了，可以发版"), "结论没进记忆: {mem}");
        assert!(!mem.contains(&head_first), "开头细节不该全量塞进记忆");
        assert!(mem.chars().count() <= MEMORY_TAIL_CHARS + 200, "记忆长度超了尾部截断: {}", mem.chars().count());

        // ⑤ 上限裁剪：连塞超长 body，总长被 MEMORY_MAX_CHARS 拦住（丢最旧的，最新的那班保留）
        for i in 0..8 {
            append_memory(&a.id, 10 + i, true, &"长内容".repeat(1000));
        }
        let capped = read_memory(&a.id);
        assert!(capped.chars().count() <= MEMORY_MAX_CHARS, "记忆没被上限拦住: {} chars", capped.chars().count());
        assert!(capped.contains("第 17 班"), "截断后最新的那班被误删了");
        // 恢复 env + 清沙箱由 `_sb` 出作用域负责（断言炸了也照样还原）。
    }

    fn base_job() -> Job {
        Job {
            id: "t".into(),
            name: "测试".into(),
            prompt: "写一条文案".into(),
            engine: "uking".into(),
            dir: String::new(),
            schedule: sch("daily", 0, "09:00", vec![]),
            enabled: true,
            created_at: 0,
            next_run_at: 0,
            last_run_at: 0,
            last_ok: None,
            last_message: String::new(),
            last_run_file: String::new(),
            runs: 0,
            use_memory: false,
        }
    }
}
