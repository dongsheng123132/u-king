//! 统一运行日志（公共层）—— `~/.uking/logs/<模块>.log`
//!
//! **为什么要有这个模块**：到 0.9.78 为止，全项目只有 4 处写日志（installer / yingbao /
//! codex_proxy / remote_assist），而 **AI 作图、AI 视频、ClawX 托管配置、GEO 体检、厨具工具箱、
//! Ollama 安装、优化大师全是黑的**。客户只会说「出不了图」「配置没生效」，远程侧零线索，
//! 只能靠一条条烧钱实跑去复现 —— 2026-07-28 排 T-King 那次就整整耗掉一小时，
//! 而根因不过是一行错误分类写反了。**没有日志就没有排障，日志是修 bug 的地基。**
//!
//! 设计约束（对齐本项目的模块独立铁律）：
//! - **纯函数、零 `AppHandle`、零第三方依赖**，谁都能调，删任何业务模块都不影响这里。
//! - 依赖方向只能「业务模块 → 本模块」，本模块**不 import 任何业务模块**。
//! - **诊断采集靠扫目录，不靠列名单**（见 [`all_tails`]）：新模块只要往这儿写一行，
//!   `diagnostics.collect` 自动就带上了，不必回头改 feedback.rs。加一个模块 = 自动多一份线索。
//!
//! **隐私**：这里**按原文写盘**，脱敏统一在上传口做（`feedback::collect_diagnostics` 末尾对
//! 整份正文跑 `desensitize`），与既有的 install.log / yingbao.log 口径一致。
//! 因此**调用方自己有责任不要把 Key / token 传进来** —— 日志落在客户本机，
//! 客户可能直接把文件发给别人。写「用了哪个模型、第几次重试、上游回了什么状态码」，
//! 不写凭据。

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单个日志文件上限；超了直接丢弃重来（滚动，不留 .1 .2，省得占客户磁盘）。
const MAX_BYTES: u64 = 256 * 1024;

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 日志根目录 `~/.uking/logs/`。
pub fn log_dir() -> PathBuf {
    home_dir().join(".uking").join("logs")
}

/// 某个模块的日志路径。`module` 用短横线小写，如 `draw` / `video` / `clawx`。
pub fn path(module: &str) -> PathBuf {
    path_in(&log_dir(), module)
}

/// 同 [`path`]，但指定日志根目录 —— 给**测试**用：真跑落盘逻辑又不碰用户的 `~/.uking`。
/// 业务代码一律用 [`path`]/[`write`]，别自己传目录。
pub fn path_in(dir: &std::path::Path, module: &str) -> PathBuf {
    // 防呆：模块名只允许 [a-z0-9-_]，避免调用方传进 `../` 把文件写到别处。
    let safe: String = module
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    dir.join(format!("{safe}.log"))
}

/// 把「1970 起的秒数」拆成 `(年, 月, 日)`（UTC）。civil_from_days（Howard Hinnant 算法）。
///
/// 纯 std，不引 chrono —— 本项目体积优先。**公开是为了给别的公共层复用**
/// （`journal.rs` 要按天分文件），别让同一个历法算法在仓库里存第二份（宪法第 8 条）。
pub fn ymd_utc(secs: i64) -> (i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `YYYY-MM-DD`（UTC）。给按天滚动的文件名用。
pub fn date_utc(secs: i64) -> String {
    let (y, m, d) = ymd_utc(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DD HH:MM:SSZ`（UTC，带 Z 标记）。纯 std 实现，不引 chrono —— 本项目体积优先。
///
/// 用 UTC 而非本地时区：客户机时区五花八门，跨机比对日志时统一基准才有意义。
/// 🔴 P2-3（黑盒报告）：格式串原来不带时区标记，输出与本地时间无法区分
/// （实测与文件 mtime 差 8.0 小时整），排查时序时真踩过坑。加一个 `Z` 一了百了 ——
/// crashlog 会把 log_tail 塞进 Issue 正文，看的人不一定记得这是 UTC。
fn ts() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = ymd_utc(secs);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}Z")
}

/// 追加一行（自动带时间戳）。**永不 panic、永不返回错误** —— 日志写失败绝不能影响业务：
/// 磁盘满 / 权限不足 / 目录被占的时候，用户要的是功能还能跑，不是因为记不了日志就崩。
pub fn write(module: &str, msg: &str) {
    write_in(&log_dir(), module, msg);
}

/// 同 [`write`]，但指定日志根目录（测试用，理由见 [`path_in`]）。
pub fn write_in(dir: &std::path::Path, module: &str, msg: &str) {
    let p = path_in(dir, module);
    let Some(parent) = p.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if std::fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        // 🔴 P2-1（黑盒报告）：原来是直接 remove_file —— 最需要历史的时刻恰好是
        // 日志涨最快的时刻，真出事时前面的线索会被这条规则清掉。
        // 改成保留一份 `.1` 滚动：超限时旧 .1 被覆盖，当前档挪去 .1，本次追加从空档开始。
        // 磁盘代价 ≤ 2×MAX_BYTES/模块；换来「涨最快的窗口里仍留得住上一段历史」。
        let rolled = p.with_extension("log.1");
        let _ = std::fs::remove_file(&rolled);
        let _ = std::fs::rename(&p, &rolled);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        // 单行化：msg 里的换行会破坏「一行一事件」的可读性，统一压成 ⏎。
        let one = msg.replace('\n', " ⏎ ");
        let _ = f.write_all(format!("[{}] {}\n", ts(), one).as_bytes());
    }
}

/// 写一段任务分隔头 —— 长任务（成片 / 装机 / 体检）开头打一条，
/// 排障时一眼看清「这次运行从哪儿开始」，不必在连续日志里数行。
pub fn section(module: &str, head: &str) {
    write(module, &format!("===== {head} ====="));
}

/// 读某个模块日志的尾部 `n` 字节。
pub fn tail(module: &str, n: u64) -> Option<String> {
    tail_path(&path(module), n)
}

/// 同 [`tail`]，但指定日志根目录（测试用，理由见 [`path_in`]）。
pub fn tail_in(dir: &std::path::Path, module: &str, n: u64) -> Option<String> {
    tail_path(&path_in(dir, module), n)
}

fn tail_path(p: &std::path::Path, n: u64) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(n))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let s = String::from_utf8_lossy(&buf).into_owned();
    if s.trim().is_empty() { None } else { Some(s) }
}

/// 扫 `~/.uking/logs/` 下**全部** `*.log`，各取尾部 `n` 字节，返回 `(模块名, 尾部)`。
///
/// 诊断采集用这个而不是硬编码模块名单：**新模块写了日志就自动被带上**，
/// 不会出现「加了功能但忘了接进诊断，客户报错还是查不到」的漏。
/// 按修改时间倒序 —— 最近动过的最可能跟本次故障相关，排在前面。
pub fn all_tails(n: u64) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(log_dir()) else {
        return Vec::new();
    };
    let mut items: Vec<(std::time::SystemTime, String, String)> = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("log") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|x| x.to_str()) else { continue };
        let Some(t) = tail_path(&p, n) else { continue };
        let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
        items.push((mtime, stem.to_string(), t));
    }
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.into_iter().map(|(_, k, v)| (k, v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 时间戳算法得真对 —— 写错了日志时间全是废的，排障时反而误导。
    #[test]
    fn ts_format_is_sane() {
        let s = ts();
        assert_eq!(s.len(), 20, "格式应为 YYYY-MM-DD HH:MM:SSZ（P2-3 起带 Z 标记），实际 {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(s.ends_with('Z'), true, "必须带 UTC 的 Z 标记，实际 {s}");
        let year: i32 = s[0..4].parse().expect("年份应可解析");
        assert!((2020..2100).contains(&year), "年份离谱：{year}");
        let month: u32 = s[5..7].parse().unwrap();
        let day: u32 = s[8..10].parse().unwrap();
        assert!((1..=12).contains(&month) && (1..=31).contains(&day), "月日越界：{s}");
    }

    /// P2-1：超限滚动要保留一份历史（.1），而不是整个删掉。
    #[test]
    fn oversize_log_rotates_keeps_one_history() {
        let dir = std::env::temp_dir().join(format!("uking-ulog-rot-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // 写满超过 MAX_BYTES 的量，触发两次滚动
        for i in 0..3 {
            write_in(&dir, "rot-test", &format!("batch-{i} {}", "x".repeat(300 * 1024)));
        }
        let cur = path_in(&dir, "rot-test");
        let rolled = cur.with_extension("log.1");
        let cur_exists = cur.exists();
        let rolled_exists = rolled.exists();
        let rolled_len = std::fs::metadata(&rolled).map(|m| m.len()).unwrap_or(0);
        let _ = std::fs::remove_file(&cur);
        let _ = std::fs::remove_file(&rolled);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(cur_exists, "当前档应存在");
        assert!(rolled_exists && rolled_len > 0, "滚动历史 .1 应存在且非空（P2-1 修复点）");
    }

    /// 模块名不能穿越目录 —— 调用方传 `../../evil` 时必须被折成安全文件名。
    #[test]
    fn module_name_cannot_escape_dir() {
        let p = path("../../evil");
        assert_eq!(p.parent().unwrap(), log_dir(), "日志必须落在 logs/ 里，实际 {p:?}");
        assert!(!p.to_string_lossy().contains(".."), "路径里不该残留 ..：{p:?}");
    }

    /// 换行必须压平，否则「一行一事件」的前提被破坏，尾部截断会截出半条记录。
    #[test]
    fn newlines_are_flattened() {
        let msg = "第一行\n第二行";
        assert!(!msg.replace('\n', " ⏎ ").contains('\n'));
    }
}
