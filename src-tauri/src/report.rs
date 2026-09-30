//! 自动 bug 采集 —— 客户端出错时静默上报，汇成 GitHub Issue 供定期巡视。
//!
//! ## 链路
//!
//! 客户端（本模块） → POST `https://www.u-king.org/api/bug`（Vercel function）
//! → 校验/限流/去重 → 在私有仓库 `dongsheng123132/u-king-mini` 建 Issue（label: bug-report）
//!
//! ## 原则
//!
//! - **后台线程发送，永不阻塞 UI、永不影响主流程**（上报失败就算了）
//! - **只采诊断数据**：版本、OS、错误种类、日志尾部、设备 Key 前 12 位（去重用，不含完整 Key）
//! - **客户端不含任何密钥**：GitHub token 只存在服务端受控目录（权限 600）
//! - 防滥用在服务端做（payload 校验 + 限流 + 按 hash 去重）
//! - **客户端先节流再发**（`throttle`）：同签名 24h 只发 1 次 + 单设备每小时上限 10 次。
//!   2026-09-08 reports 仓被上百条 `[auto][install_failed]` 淹没的教训 —— 服务端按 hash
//!   去重挡不住「每台机器错误文本都不同」的洪水（Cline 二进制缺失、代理连不上，每台标题
//!   都不一样）。节流只拦**网络发送**，本地 `metrics`/`crashlog` 落盘一条不少，信号不丢，
//!   只是不再每台机器每天开几十个 Issue。用户主动点的「一键提交」(`report_feedback`)
//!   永不节流。

use serde::{Deserialize, Serialize};
use serde_json::json;

/// 第一顺位走国内可达的 u-claw.org.cn 反代（/uking/bug → 服务端采集服务）。
/// ⚠️ api.u-claw.org / cloud.u-claw.org 国内裸网 SNI 被 GFW reset（客户机 pc-*** 2026-06-17
/// 实测：TCP 443 通但 HTTPS「连接被关闭」），所以历史上国内客户的 bug 一条都没收到——
/// 全卡在不可达域名。u-claw.org.cn/uking/bug 国内裸网 200（实测建 Issue 成功）。
///
/// 🔴 **2026-08-19 删掉了两个兜底，别加回来：**
///  · `www.u-king.org/api/bug` —— 站点 06-17 搬新加坡后这条路由就不存在了，实测 404，
///    留着只是让失败多绕一圈。
///  · `u-king-org.vercel.app/api/bug` —— **这条是泄露路径**。那个 Vercel 部署还活着
///    （实测仍返回 400 = 在正常收），但跑的是 06-17 的旧代码，落点写死 `u-king-mini`。
///    而 u-king-mini 即将开源 → 客户真名路径和**未脱敏截图**会进公开仓库，被搜索引擎
///    收走就撤不回来。新链路的落点是私有的工单仓，且有「不是私有就不写」的护栏；
///    旧的那份两样都没有。
///
/// 代价说清楚：剩下两个域名指的是**同一台**新加坡机器，那台挂了两个一起挂 ——
/// Vercel 那条本来是唯一的异地冗余。但一条会把客户隐私写进公开仓库的冗余是负资产，
/// 宁可丢这条上报。真要补异地冗余，得再起一个落点同样是私有仓库的实例，而不是留着旧的。
const REPORT_URLS: &[&str] = &[
    "https://u-claw.org.cn/uking/bug",
    "https://api.u-claw.org/uking/bug",
];

/// `report_bug` 对 detail 的截断上限（字节）。单一真相源：`compose_install_report` 按它给自己
/// 拼的内容定预算，保证拼出来的东西不会再被 `truncate` 从头部砍掉（那会把首个错误一起砍了）。
pub(crate) const DETAIL_MAX_BYTES: usize = 6 * 1024;

/// 首个错误快照段（「首个错误及前文」）自身的字节上限。
const FIRST_ERR_CTX_MAX_BYTES: usize = 1500;

/// 保**尾部**至多 n 字节（切在 char 边界，绝不切坏 UTF-8）；被截过就在最前面补个「…」。
/// 结果恒 ≤ n 字节（`…` 自己占 3 字节，已计入）。n 小到放不下省略号时返回空串。
fn keep_tail_bytes(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    if n < '…'.len_utf8() {
        return String::new();
    }
    let mut start = s.len() - (n - '…'.len_utf8());
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

/// 拼装装机失败上报的 detail，**保住首个错误**。
///
/// 为什么要有：装机日志在内存里只留最近 ≤120 行（超了丢最早 40 行），而 `report_bug` 又会把
/// detail 截到**最后** 6KB。主步骤里的首个错误（往往是真因）随后被 repair 的大量输出淹没：
/// 首错既被 drain 掉、又在 6KB 截断里排在最前面，上报到手只剩 repair 的连锁报错，约 9% 的
/// 失败因此判不出原因。调用方在第一次出现 `error` 阶段时快照当时的最后 ≤12 行（含那条 error 行，
/// 且它是快照的最后一行），失败时连同当前日志尾部一起交给这里。
///
/// - 无快照、或「首错行仍在 tail 里 且 整体不超预算」→ **原样旧格式** `header\n{tail}`（行为不变）；
/// - 否则 → `header` + `[首个错误及前文]`（快照，自身 ≤ 1500 字节，超了保尾部）+ `[日志尾部]`，
///   tail 段按「budget − 前两段长度」保尾部截断。最终总长 ≤ budget（字节），
///   传 `DETAIL_MAX_BYTES` 时 `report_bug` 的 `truncate` 便不会再动它。
pub(crate) fn compose_install_report(
    header: &str,
    first_err_ctx: Option<&[String]>,
    tail_lines: &[String],
    budget: usize,
) -> String {
    let tail = tail_lines.join("\n");
    let legacy = format!("{header}\n{tail}");
    let Some(ctx) = first_err_ctx.filter(|c| !c.is_empty()) else {
        return legacy;
    };
    // 快照最后一行就是那条 error 行；它还在当前 tail 里、且旧格式装得下 → 不需要额外拼段
    let err_line = &ctx[ctx.len() - 1];
    if tail_lines.iter().any(|l| l == err_line) && legacy.len() <= budget {
        return legacy;
    }
    let ctx_text = keep_tail_bytes(&ctx.join("\n"), FIRST_ERR_CTX_MAX_BYTES.min(budget / 4));
    let head = format!("{header}\n[首个错误及前文]\n{ctx_text}\n[日志尾部]\n");
    let room = budget.saturating_sub(head.len());
    format!("{head}{}", keep_tail_bytes(&tail, room))
}

/// 装机 / 升级流水线的日志收集 + 失败上报共用件（`install_ai_tool_shared` 与 `upgrade_cli_tool` 都用它，
/// 两处各抄一份会漂移：首错快照、预检拦截不上报这些规则只此一份）。
///
/// - `record`：每条日志进内存尾巴（`[phase] line`，超 120 行丢最早 40 行）；**第一次**出现 `error` 阶段时
///   快照当时最后 ≤12 行（含那条 error 行），只存第一次。为什么：尾巴会被 repair 的大量输出挤掉、
///   `report_bug` 又只保 detail 的**尾部**，首个错误（往往是真因）就丢了（约 9% 的失败判不出因）。
/// - `report_if_failed`：失败且不是预检拦截才上报，detail 走 `compose_install_report`。
pub(crate) struct InstallLogTail {
    lines: std::sync::Mutex<Vec<String>>,
    first_err_ctx: std::sync::Mutex<Option<Vec<String>>>,
}

/// 首个错误快照最多带的行数（含那条 error 行）。
const FIRST_ERR_CTX_LINES: usize = 12;
/// 内存尾巴的行数上限；超了丢最早的 LOG_TAIL_DRAIN 行。
const LOG_TAIL_MAX_LINES: usize = 120;
const LOG_TAIL_DRAIN: usize = 40;

impl InstallLogTail {
    pub(crate) fn new() -> Self {
        Self { lines: std::sync::Mutex::new(Vec::new()), first_err_ctx: std::sync::Mutex::new(None) }
    }

    /// 记一条日志。锁被毒化时静默丢弃（上报的辅助信息，不能因它 panic 拖垮装机线程）。
    pub(crate) fn record(&self, phase: &str, line: &str) {
        let Ok(mut l) = self.lines.lock() else { return };
        l.push(format!("[{phase}] {line}"));
        if phase == "error" {
            if let Ok(mut snap) = self.first_err_ctx.lock() {
                if snap.is_none() {
                    *snap = Some(l[l.len().saturating_sub(FIRST_ERR_CTX_LINES)..].to_vec());
                }
            }
        }
        if l.len() > LOG_TAIL_MAX_LINES {
            l.drain(..LOG_TAIL_DRAIN);
        }
    }

    /// 拼上报 detail（纯读，好测）。
    fn detail(&self, header: &str) -> String {
        let tail_lines = self.lines.lock().map(|l| l.clone()).unwrap_or_default();
        let first_err = self.first_err_ctx.lock().ok().and_then(|g| g.clone());
        compose_install_report(header, first_err.as_deref(), &tail_lines, DETAIL_MAX_BYTES)
    }

    /// 失败才上报：成功不报；`precheck_blocked`（磁盘不足 / Windows 版本过低这类设计内拦截，不是 bug）
    /// 也不报——本地 install.log 照写、界面照常拿到失败结果。`kind` 分别是 install_failed / upgrade_failed。
    pub(crate) fn report_if_failed(
        &self,
        kind: &str,
        summary: &str,
        r: &crate::installer::InstallToolResult,
        header: &str,
    ) {
        if should_report_install_failure(r) {
            report_bug(kind, summary, &self.detail(header));
        }
    }
}

/// 这次安装/升级结果要不要上报 issue：失败、且不是环境预检拦截。
pub(crate) fn should_report_install_failure(r: &crate::installer::InstallToolResult) -> bool {
    !r.ok && !r.precheck_blocked
}

/// 上报一个 bug（后台线程，静默）。
///
/// `kind`：install_failed / ai_diagnose_failed / apply_failed / panic …
/// `summary`：一行摘要（进 Issue 标题）
/// `detail`：日志尾部等上下文（截断到 6KB）
pub fn report_bug(kind: &str, summary: &str, detail: &str) {
    let kind = kind.to_string();
    let summary = truncate(summary, 160);
    let detail = truncate(detail, DETAIL_MAX_BYTES);

    // 先落**本地**数据基台，再尝试上传。顺序不能反 —— 上面那段注释里的教训就是：
    // 域名在国内不可达时，历史上国内客户的 bug **一条都没收到**。本地这条 append
    // 不依赖网络，永远写得进去，客户机上永远查得到。
    // `msg` 进 metrics 前必须脱敏（红线：metrics 只存，不负责脱敏）。
    crate::metrics::log_error(&kind, None, &crate::feedback::desensitize(&summary));

    std::thread::spawn(move || {
        let _ = send(&kind, &summary, &detail, &[], true);
    });
}

/// `shots`：客户明确勾选「同意上传」时才有值 —— 已压缩的 JPEG base64（不含 data: 前缀）。
/// 服务端用建 issue 的同一把 token 传进仓库，客户端不接触任何凭证。
///
/// `throttle`：自动上报传 true（同签名 24h 只发 1 次 + 每小时上限，见本文件节流节）；
/// 用户主动反馈传 false（用户亲手点的，永不拦）。
fn send(kind: &str, summary: &str, detail: &str, shots: &[String], throttle: bool) -> Result<(), String> {
    if throttle && !throttle_allow(kind, summary) {
        return Ok(()); // 被节流：本地 metrics/crashlog 已落盘，只是不再发网络
    }
    let device = crate::device::get_device_key_cached_prefix();
    let body = json!({
        "app": "u-king-mini",
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "kind": kind,
        "summary": summary,
        "detail": detail,
        "device": device,
        "shots": shots,
    });

    let tmp = std::env::temp_dir().join(format!("uk-bugdump-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(&body).unwrap()).map_err(|e| e.to_string())?;
    let data = format!("@{}", tmp.display());

    let mut last = String::new();
    for url in REPORT_URLS {
        let r = crate::installer::curl(&[
            "-sS",
            "-m",
            "15",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-H",
            concat!("x-uking-version: ", env!("CARGO_PKG_VERSION")),
            "--data",
            &data,
            url,
        ]);
        match r {
            Ok(_) => {
                let _ = std::fs::remove_file(&tmp);
                return Ok(());
            }
            Err(e) => last = e,
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last)
}

/// 用户**主动**反馈（技术支持页「一键提交」用）。与自动上报同一条链路（服务端建 Issue），
/// 但**同步**发送并把成功/失败如实返回给前端（自动上报是 fire-and-forget，反馈要给用户确认）。
/// `summary`/`detail` 须由调用方**先脱敏**（feedback.rs 负责），本函数只管发。
pub fn report_feedback(summary: &str, detail: &str, shots: &[String]) -> Result<(), String> {
    // 标题取**开头**：`truncate` 保的是尾部（对日志正确——报错都在最后），但用户反馈的第一句
    // 开头才是主语，截尾会得到一条「…的时候就没反应了」这种看不出说啥的 Issue 标题。
    let summary = truncate_head(summary, 160);
    let detail = truncate(detail, 10 * 1024);
    send("user_feedback", &summary, &detail, shots, false)
}

/// 安装 panic hook：崩溃也上报（panic=abort 下 hook 仍会先执行）。
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        // **先落盘再发网络**，顺序是刻意的（2026-07-30 pc-*** 教训）：
        // 下面那步要拨 curl、最多耗 15 秒，客户断网 / 开代理 / 被 GFW 拦一下就彻底送不出去，
        // 而 `panic=abort` 随时可能在它跑完前把进程掐掉 —— 那样崩溃现场一个字节都不剩，
        // 事后远程连上去查，事件日志、转储、隔离区全是空的，等于没崩过。
        // 写本地文件是微秒级且不依赖任何外部条件，永远来得及。
        //
        // 两条本地落盘都留着，各干各的、不是重复：
        //   · crashlog = **取证**，原文全留，供事后人肉/远程排障读；
        //   · metrics  = **事件日志**，进数据基台做统计，所以必须先脱敏
        //     （红线：metrics 只存，不负责脱敏，脱敏是调用方的事）。
        crate::crashlog::record("panic", "应用崩溃", &msg);
        crate::metrics::log_error("panic", None, &crate::feedback::desensitize(&msg));
        // 同步快发（最多 5 秒），abort 前尽力送出
        let _ = send_blocking_quick("panic", "应用崩溃", &msg);
        // 不再调用标准 default hook：它把 panic 信息写 stderr，写失败会**二次 panic**
        // （windows_subsystem=windows 下 stderr 常是已关闭的管道 → os error 232「管道正在被
        // 关闭」，正是 #191 的崩溃根因）。这里自己用忽略错误的写法输出，坏管道时静默跳过，
        // 绝不因打印失败再崩一次。
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "panic: {msg}");
    }));
}

fn send_blocking_quick(kind: &str, summary: &str, detail: &str) -> Result<(), String> {
    send(kind, summary, &truncate(detail, 2048), &[], true)
}

/// 保**尾部** n 个字符 —— 日志用（报错总在最后）。
fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().rev().take(n).collect();
        cut.chars().rev().collect()
    }
}

/// 保**开头** n 个字符 —— 标题用（第一句话的开头才有信息量）。
fn truncate_head(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

// ============================================================
// 客户端上报节流（2026-09-08，reports 仓洪水教训）
// ============================================================
//
// 只拦**网络发送**：本地 metrics/crashlog 落盘在节流之前（report_bug 里），信号不丢。
// 两条规则（都按单设备本地时钟）：
//   1. 同签名 24h 只发 1 次 —— 签名 = kind + 归一化后的 summary（数字→#、空白折叠、
//      截 120 字符）。同一类错误每小时报一次就够了，剩下的本地有数。
//   2. 每小时全局上限 10 次 —— 签名归一化也兜不住的新花样（每台标题都不同），由总量兜底。
//
// 状态落 `~/.uking/report-throttle.json`（best-effort：读不到/写不进一律放行，
// 上报链路不因节流文件坏掉而静默 —— 丢上报比多上报更坏）。
// 用户主动反馈（report_feedback）不走这里。

/// 同一签名 24 小时内最多发 1 次。
const THROTTLE_SIG_WINDOW_SECS: u64 = 24 * 3600;
/// 单设备每小时最多发 10 次（所有签名合计）。
const THROTTLE_HOUR_CAP: u32 = 10;
const THROTTLE_HOUR_SECS: u64 = 3600;
/// 状态文件条数上限（防无限膨胀；超了先扔最老的）。
const THROTTLE_MAX_SIGS: usize = 200;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SigRec {
    first: u64,
    last: u64,
    count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ThrottleState {
    #[serde(default)]
    sigs: std::collections::HashMap<String, SigRec>,
    #[serde(default)]
    hour_start: u64,
    #[serde(default)]
    hour_n: u32,
}

/// 签名归一化：数字段→`#`（「失败 3 次」和「失败 47 次」是同一类），空白折叠，截 120 字符。
/// 路径/版本号里的数字也会被吃掉 —— 这是故意的：签名只用来「认出重复」，不用来定位，
/// 定位看 Issue 正文里的原文 summary（服务端收到的是未归一化的）。
fn sig_of(kind: &str, summary: &str) -> String {
    let mut out = String::with_capacity(summary.len().min(120) + kind.len() + 1);
    out.push_str(kind);
    out.push('|');
    let mut in_digit = false;
    let mut in_space = false;
    for c in summary.chars() {
        if c.is_ascii_digit() {
            if !in_digit {
                out.push('#');
                in_digit = true;
            }
            in_space = false;
        } else if c.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
            in_digit = false;
        } else {
            out.push(c);
            in_digit = false;
            in_space = false;
        }
        if out.len() >= 120 + kind.len() + 1 {
            break;
        }
    }
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 纯决策函数（不碰磁盘，可单测）：该发返回 true，同时把计数写进 state。
fn decide(state: &mut ThrottleState, kind: &str, summary: &str, now: u64) -> bool {
    // 小时窗口滚动
    if now.saturating_sub(state.hour_start) >= THROTTLE_HOUR_SECS {
        state.hour_start = now;
        state.hour_n = 0;
    }
    if state.hour_n >= THROTTLE_HOUR_CAP {
        return false;
    }
    let sig = sig_of(kind, summary);
    match state.sigs.get_mut(&sig) {
        Some(rec) if now.saturating_sub(rec.first) < THROTTLE_SIG_WINDOW_SECS => {
            // 同一窗口内：只放行第一次，之后的全拦（但更新 last，供排查看「最后一次见到」）。
            rec.last = now;
            rec.count += 1;
            return false;
        }
        _ => {}
    }
    // 新签名，或旧窗口已过 24h → 重新开窗
    if state.sigs.len() >= THROTTLE_MAX_SIGS {
        // 扔最老的：按 last 排序，保留最新的 MAX-1 条
        let mut keys: Vec<(String, u64)> =
            state.sigs.iter().map(|(k, r)| (k.clone(), r.last)).collect();
        keys.sort_by_key(|(_, last)| *last);
        for (k, _) in keys.into_iter().take(state.sigs.len().saturating_sub(THROTTLE_MAX_SIGS - 1))
        {
            state.sigs.remove(&k);
        }
    }
    state.sigs.insert(sig, SigRec { first: now, last: now, count: 1 });
    state.hour_n += 1;
    true
}

fn throttle_path() -> std::path::PathBuf {
    // ~/.uking 下（metrics 同级），复用 metrics 的 UKING_TEST_HOME 沙箱口径。
    crate::metrics::metrics_dir()
        .parent()
        .map(|p| p.join("report-throttle.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("report-throttle.json"))
}

/// 读状态 → 决策 → 写回。全程 best-effort：任何一步失败都**放行**（丢上报比多上报更坏）。
fn throttle_allow(kind: &str, summary: &str) -> bool {
    let now = now_secs();
    let path = throttle_path();
    let mut state: ThrottleState = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let allow = decide(&mut state, kind, summary, now);
    if allow {
        // 写不进也不拦这次（已经决定发了），下次重读旧状态而已。
        let _ = std::fs::write(&path, serde_json::to_string(&state).unwrap_or_default());
    } else {
        // 被拦的也要更新 last/count 落盘吗？不 —— 洪水时每次错误都写一次文件，
        // 磁盘 IO 反而成了新噪音。下次放行时 count 会重开窗，无所谓。
    }
    allow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_keeps_head_log_keeps_tail() {
        let long = "开头很重要".repeat(50); // 250 字符
        let title = truncate_head(&long, 20);
        assert!(title.starts_with("开头很重要"), "标题要保开头: {title}");
        assert!(title.ends_with('…'), "截断要有省略号: {title}");
        // 日志相反：保尾部（报错都在最后）
        let log = format!("{}最后一行报错", "x".repeat(500));
        assert!(truncate(&log, 40).ends_with("最后一行报错"), "日志要保尾部");
    }

    #[test]
    fn sig_normalizes_numbers_and_spaces() {
        // 「失败 3 次」和「失败 47 次」是同一签名（洪水时每台数字都不同）
        assert_eq!(
            sig_of("install_failed", "cline 安装失败，重试 3 次"),
            sig_of("install_failed", "cline 安装失败，重试 47 次")
        );
        // kind 不同 = 不同签名
        assert_ne!(
            sig_of("install_failed", "x 3"),
            sig_of("panic", "x 3")
        );
    }

    #[test]
    fn same_sig_reports_once_per_day() {
        let mut st = ThrottleState::default();
        assert!(decide(&mut st, "install_failed", "cline 安装失败 1", 1_000_000));
        // 同一窗口内：数字不同也被归一化拦住
        assert!(!decide(&mut st, "install_failed", "cline 安装失败 2", 1_000_100));
        assert!(!decide(&mut st, "install_failed", "cline 安装失败 3", 1_003_600));
        // 24h 窗口过了 → 重新放行
        assert!(decide(&mut st, "install_failed", "cline 安装失败 4", 1_000_000 + 86_400 + 1));
    }

    fn filler_lines(tag: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("[out] {tag} 行 {i:04} 一些中文填充内容 padding padding")).collect()
    }

    /// 长 tail（repair 输出把首个错误挤出去）时：首错段必须还在、总长 ≤ 预算、
    /// tail 段保尾部，且 report_bug 里的 truncate 不会再动它（否则首错又被从头部砍掉）。
    #[test]
    fn install_report_keeps_first_error_when_tail_is_long() {
        let header = "skill v99 (embedded)";
        let ctx: Vec<String> = vec![
            "[step] 装 Node".into(),
            "[out] npm ERR! 前文".into(),
            "[error] 安装步骤失败：真因在这里".into(),
        ];
        let tail = filler_lines("repair", 200); // ~12KB，首错行早已不在其中
        let out = compose_install_report(header, Some(&ctx), &tail, DETAIL_MAX_BYTES);
        assert!(out.len() <= DETAIL_MAX_BYTES, "总长 {} 超预算", out.len());
        assert!(out.starts_with(header));
        assert!(out.contains("[首个错误及前文]"), "缺首错段");
        assert!(out.contains("安装步骤失败：真因在这里"), "首错行丢了");
        assert!(out.contains("[日志尾部]"), "缺尾部段");
        assert!(out.ends_with(tail.last().unwrap().as_str()), "tail 要保尾部（最后一行必在）");
        assert_eq!(truncate(&out, DETAIL_MAX_BYTES), out, "report_bug 的 truncate 不该再动它");
    }

    /// 首错行仍在 tail 里（且整体装得下）→ 输出与旧格式逐字节一致；无快照同理。
    #[test]
    fn install_report_is_legacy_format_when_first_error_still_in_tail() {
        let header = "skill v1 (server)";
        let tail: Vec<String> = vec!["[step] a".into(), "[error] 首错".into(), "[out] b".into()];
        let ctx: Vec<String> = vec!["[step] a".into(), "[error] 首错".into()];
        let legacy = format!("{header}\n{}", tail.join("\n"));
        assert_eq!(compose_install_report(header, Some(&ctx), &tail, DETAIL_MAX_BYTES), legacy);
        assert_eq!(compose_install_report(header, None, &tail, DETAIL_MAX_BYTES), legacy);
        assert_eq!(compose_install_report(header, Some(&[]), &tail, DETAIL_MAX_BYTES), legacy);
    }

    /// 首错行还在 tail 里，但 tail 已经大到旧格式会被 truncate 从头砍：也要改走三段式保住首错。
    #[test]
    fn install_report_uses_sections_when_legacy_would_overflow_even_if_error_in_tail() {
        let header = "skill v1 (server)";
        let mut tail = vec!["[error] 首错在 tail 最前面".to_string()];
        tail.extend(filler_lines("repair", 200));
        let ctx: Vec<String> = vec!["[error] 首错在 tail 最前面".into()];
        let out = compose_install_report(header, Some(&ctx), &tail, DETAIL_MAX_BYTES);
        assert!(out.len() <= DETAIL_MAX_BYTES);
        assert!(out.contains("[首个错误及前文]\n[error] 首错在 tail 最前面"));
        assert!(out.ends_with(tail.last().unwrap().as_str()));
    }

    /// 快照段自身封顶 ~1500 字节（超了保尾部——那条 error 行在快照最后，必须留住）；
    /// 全中文（3 字节/字）的 tail 在任意预算下都不许切坏 UTF-8、不许超预算。
    #[test]
    fn install_report_caps_first_error_section_and_respects_char_boundaries() {
        let header = "skill v1 (embedded)";
        let mut ctx: Vec<String> = (0..12).map(|i| format!("[out] 前文第{i}行 {}", "长".repeat(80))).collect();
        ctx.push("[error] 最后这条才是首错".into());
        let tail: Vec<String> = (0..300).map(|i| format!("[out] 全中文第{i}行：{}", "字".repeat(30))).collect();
        for budget in 2000..2100usize {
            let out = compose_install_report(header, Some(&ctx), &tail, budget);
            assert!(out.len() <= budget, "budget={budget} 实际 {}", out.len());
            assert!(out.contains("[error] 最后这条才是首错"), "budget={budget} 首错行丢了");
        }
        let out = compose_install_report(header, Some(&ctx), &tail, DETAIL_MAX_BYTES);
        let first_sec = out.split("[首个错误及前文]\n").nth(1).unwrap().split("\n[日志尾部]\n").next().unwrap();
        assert!(first_sec.len() <= 1500, "首错段 {} 字节，超了 1500", first_sec.len());
    }

    fn result_for_test(ok: bool, blocked: bool) -> crate::installer::InstallToolResult {
        crate::installer::InstallToolResult {
            ok,
            tool: "demo".into(),
            version: None,
            attempts: 1,
            error: if ok { None } else { Some("boom".into()) },
            precheck_blocked: blocked,
        }
    }

    /// 上报判定：成功不报；预检拦截不报；普通失败才报（install 与 upgrade 共用这一条规则）。
    #[test]
    fn install_failure_reporting_skips_success_and_precheck_blocked() {
        assert!(!should_report_install_failure(&result_for_test(true, false)));
        assert!(!should_report_install_failure(&result_for_test(false, true)));
        assert!(should_report_install_failure(&result_for_test(false, false)));
    }

    /// 收集器：尾巴封顶 120 行（超了丢最早 40 行）；首个 error 出现时快照最后 ≤12 行、只存第一次；
    /// 后面 repair 洪水把首错挤出尾巴后，detail 里首错段仍在、总长 ≤ 6KB。
    #[test]
    fn install_log_tail_snapshots_first_error_and_survives_flood() {
        let t = InstallLogTail::new();
        for i in 0..20 {
            t.record("out", &format!("准备步骤 {i}"));
        }
        t.record("error", "安装步骤失败：npm 报 ENOENT（这是真因）");
        t.record("error", "第二个 error：不该覆盖首错快照");
        t.record("repair", "开始自动修复重装…");
        for i in 0..400 {
            t.record("out", &format!("repair 输出洪水 {i} {}", "冗长的日志内容 ".repeat(6)));
        }
        assert!(t.lines.lock().unwrap().len() <= 120, "尾巴该封顶 120 行");
        let snap = t.first_err_ctx.lock().unwrap().clone().expect("应有首错快照");
        assert!(snap.len() <= 12 && snap.last().unwrap().contains("这是真因"), "{snap:?}");
        let d = t.detail("skill v57 (embedded)");
        assert!(d.len() <= DETAIL_MAX_BYTES, "detail {} 字节超预算", d.len());
        assert!(d.contains("[首个错误及前文]") && d.contains("这是真因"), "首错被淹没了");
        assert!(!d.contains("第二个 error"), "首错快照只存第一次");
        assert!(d.contains("repair 输出洪水 399"), "尾部要保住最后的输出");
    }

    #[test]
    fn hour_cap_bounds_novel_flood() {
        let mut st = ThrottleState::default();
        // 每个签名都不同（注意：数字会被归一化吃掉，所以用不同字母构造差异），
        // 总量兜底：每小时 10 次
        for i in 0..10u8 {
            let s = format!("完全不同的错误-{}-尾巴", (b'a' + i) as char);
            assert!(decide(&mut st, "k", &s, 2_000_000), "第 {i} 个该放行");
        }
        assert!(!decide(&mut st, "k", "完全不同的错误-k-尾巴", 2_000_001), "第 11 个该拦");
        // 下一小时滚动 → 重新放行
        assert!(decide(&mut st, "k", "完全不同的错误-z-尾巴", 2_000_000 + 3600 + 1));
    }
}
