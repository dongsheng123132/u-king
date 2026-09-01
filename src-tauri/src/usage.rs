//! 用量趋势 —— 把每次查到的虾盘云余额/累计消耗存成本地快照，算每日消耗曲线。
//!
//! 虾盘云 API 只能查到「总充值额 hard_limit」和「累计消耗 total_usage」两个瞬时值，
//! 查不到按天的明细。所以我们每次查余额时存一条快照到 `~/.uking/usage-history.json`，
//! 相邻快照的 total_usage 之差 = 这段时间的消耗，按自然日聚合就得到「每日消耗」柱图。
//!
//! 纯 std + serde_json，无第三方依赖。日期用 `date` 命令取（跨平台都有），
//! 避免引 chrono。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 一条快照（查余额时记一次）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    /// 本地日期 YYYY-MM-DD
    date: String,
    /// 本地时间戳（HH:MM，仅展示用）
    time: String,
    /// 累计消耗（虾盘云 total_usage，单位 USD×100）
    total_usage: f64,
    /// 总充值额（hard_limit_usd）
    hard_limit: f64,
}

/// 每日消耗（回前端画柱图）。
#[derive(Debug, Clone, Serialize)]
pub struct DailyUsage {
    pub date: String,
    /// 当日消耗的 token 数（已按 ¥1=50万 token 换算）
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageTrend {
    /// 最近 N 天（升序），缺的天补 0
    pub daily: Vec<DailyUsage>,
    /// 今日消耗 token
    pub today_tokens: i64,
    /// 最近 7 天合计
    pub week_tokens: i64,
    /// 快照数量（数据是否充足的提示）
    pub samples: usize,
}

fn history_path() -> PathBuf {
    let home = std::env::var("UKING_TEST_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".uking").join("usage-history.json")
}

fn load() -> Vec<Snapshot> {
    // 过滤掉日期非法的坏快照（pc-*** 客户机 1600+ 条 `Get-Date`/`-Format` 假数据）：
    // 坏数据不展示、不参与趋势计算 —— 否则会画出一条假的「持续扣费」曲线。
    std::fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<Snapshot>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| is_valid_date(&s.date))
        .collect()
}

fn save(snaps: &[Snapshot]) {
    let p = history_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, serde_json::to_string_pretty(snaps).unwrap_or_default());
}

/// 本地日期 + 时间（用系统 date，跨平台）。
fn now_date_time() -> (String, String) {
    let out = run_date();
    let s = out.trim();
    // 期望 "2026-06-12 15:30"。先校验日期部分，防止 PowerShell 把命令文本回显
    // 出来时（pc-*** 实锤：cmd /C 引号嵌套在部分 Windows 上输出的是命令本身，
    // `Get-Date`/`-Format` 被当数据写进了 usage-history.json）把垃圾写进历史文件。
    let mut it = s.split_whitespace();
    let d = it.next().unwrap_or("").to_string();
    if !is_valid_date(&d) {
        return (String::new(), String::new());
    }
    let t = it.next().unwrap_or("").to_string();
    (d, t)
}

/// 形如 `2026-06-12` 的合法日期（长度 10、数字/连字符、以 20 开头）。
/// 坏数据（命令文本、乱码）一律判非法，不进历史文件、不参与趋势计算。
fn is_valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && s.starts_with("20")
        && (0..10).all(|i| b[i].is_ascii_digit() || b[i] == b'-')
}

#[cfg(windows)]
fn run_date() -> String {
    use std::os::windows::process::CommandExt;
    // 直接调 powershell，每个参数独立传 —— 不再包一层 `cmd /C "......"`：
    // 那个引号嵌套在部分 Windows 上会把命令文本回显到 stdout（pc-*** 实锤），
    // 直接调没有嵌套，Rust 会为每个 arg 正确引用。
    std::process::Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-NonInteractive", "-Command", "Get-Date -Format 'yyyy-MM-dd HH:mm'"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn run_date() -> String {
    std::process::Command::new("date")
        .args(["+%Y-%m-%d %H:%M"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// 记一条快照（查余额成功时调用）。同一天保留首尾即可，但简单起见全存、按容量裁剪。
pub fn record(total_usage: f64, hard_limit: f64) {
    let (date, time) = now_date_time();
    if date.is_empty() {
        return;
    }
    let mut snaps = load();
    snaps.push(Snapshot {
        date,
        time,
        total_usage,
        hard_limit,
    });
    // 最多留 2000 条（够算几个月趋势）
    let len = snaps.len();
    if len > 2000 {
        snaps.drain(0..len - 2000);
    }
    save(&snaps);
}

/// 算最近 `days` 天的每日消耗趋势。
pub fn trend(days: usize) -> UsageTrend {
    let snaps = load();
    let samples = snaps.len();

    // 每天取「最后一条」的 total_usage 作为当天结束值
    let mut last_of_day: BTreeMap<String, f64> = BTreeMap::new();
    for s in &snaps {
        last_of_day.insert(s.date.clone(), s.total_usage);
    }

    // total_usage 单位 USD×100；消耗(USD) = Δ/100；token = USD × 500000
    // 当日消耗 = 当天末值 - 前一天末值
    let dates: Vec<String> = last_of_day.keys().cloned().collect();
    let mut daily_map: BTreeMap<String, i64> = BTreeMap::new();
    for (i, d) in dates.iter().enumerate() {
        let cur = last_of_day[d];
        let prev = if i == 0 {
            // 首日没有前值，用当天最早一条做基线（同日内的增量）
            snaps
                .iter()
                .find(|s| &s.date == d)
                .map(|s| s.total_usage)
                .unwrap_or(cur)
        } else {
            last_of_day[&dates[i - 1]]
        };
        let delta_usd = (cur - prev).max(0.0) / 100.0;
        let tokens = (delta_usd * 500_000.0).round() as i64;
        daily_map.insert(d.clone(), tokens);
    }

    // 取最近 days 天，缺的补 0（需要连续日期 —— 用快照里出现过的日期，简单处理：直接取末尾 days 条）
    let recent: Vec<DailyUsage> = daily_map
        .iter()
        .rev()
        .take(days)
        .map(|(d, t)| DailyUsage {
            date: d.clone(),
            tokens: *t,
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let (today_date, _) = now_date_time();
    let today_tokens = daily_map.get(&today_date).copied().unwrap_or(0);
    let week_tokens: i64 = recent.iter().rev().take(7).map(|d| d.tokens).sum();

    UsageTrend {
        daily: recent,
        today_tokens,
        week_tokens,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_validation_rejects_garbage() {
        // pc-*** 实锤的坏数据形态：`Get-Date` / `-Format` 被当日期写进了历史文件
        assert!(is_valid_date("2026-08-06"));
        assert!(!is_valid_date("Get-Date"));
        assert!(!is_valid_date("-Format"));
        assert!(!is_valid_date("2026-8-6")); // 非零填充
        assert!(!is_valid_date(""));
        assert!(!is_valid_date(" 2026-08-06")); // 带空格
    }

    #[test]
    fn load_filters_garbage_snapshots() {
        // 客户机那份 1600+ 条假数据的复现：坏快照必须被滤掉，不展示、不算趋势
        let sb = crate::testsandbox::enter("usage-history", &[".uking"]);
        let dir = sb.root().join(".uking");
        std::fs::write(
            dir.join("usage-history.json"),
            r#"[
              {"date":"Get-Date","time":"-Format","total_usage":6493.276,"hard_limit":5.16},
              {"date":"2026-08-05","time":"16:04","total_usage":6493.276,"hard_limit":5.16}
            ]"#,
        )
        .unwrap();
        let snaps = load();
        assert_eq!(snaps.len(), 1, "坏快照必须被滤掉，只留合法日期");
        assert_eq!(snaps[0].date, "2026-08-05");
    }
}
