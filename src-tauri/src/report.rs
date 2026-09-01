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

/// 上报一个 bug（后台线程，静默）。
///
/// `kind`：install_failed / ai_diagnose_failed / apply_failed / panic …
/// `summary`：一行摘要（进 Issue 标题）
/// `detail`：日志尾部等上下文（截断到 6KB）
pub fn report_bug(kind: &str, summary: &str, detail: &str) {
    let kind = kind.to_string();
    let summary = truncate(summary, 160);
    let detail = truncate(detail, 6 * 1024);

    // 先落**本地**数据基台，再尝试上传。顺序不能反 —— 上面那段注释里的教训就是：
    // 域名在国内不可达时，历史上国内客户的 bug **一条都没收到**。本地这条 append
    // 不依赖网络，永远写得进去，客户机上永远查得到。
    // `msg` 进 metrics 前必须脱敏（红线：metrics 只存，不负责脱敏）。
    crate::metrics::log_error(&kind, None, &crate::feedback::desensitize(&summary));

    std::thread::spawn(move || {
        let _ = send(&kind, &summary, &detail, &[]);
    });
}

/// `shots`：客户明确勾选「同意上传」时才有值 —— 已压缩的 JPEG base64（不含 data: 前缀）。
/// 服务端用建 issue 的同一把 token 传进仓库，客户端不接触任何凭证。
fn send(kind: &str, summary: &str, detail: &str, shots: &[String]) -> Result<(), String> {
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
    send("user_feedback", &summary, &detail, shots)
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
    send(kind, summary, &truncate(detail, 2048), &[])
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
}
