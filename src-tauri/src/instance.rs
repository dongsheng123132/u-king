//! 「这个 U-King 进程扮演什么角色」的唯一登记处 —— 主实例 / 并行调试实例 / 无头。
//!
//! # 这是干什么的
//!
//! 开发时要**同时开两个 U-King**：一个是你正在用的（工作台里挂着一堆终端，关掉 = 打断手上所有的活），
//! 另一个是刚构建出来待验的新版。两边共用**同一份 `~/.uking`** —— 这是前提，不是妥协：
//! 「并行验的是同一份世界」正是这功能的意义，`UKING_TEST_HOME` 沙箱把 `~/.uking` 整个挪走，
//! 验的就不是同一个东西了，所以那条路被明确否决过。
//!
//! 共用一份世界，就必须隔离「**谁负责后台**」。第二个实例的界面 / 终端 / 对话 / 工作目录跟第一个
//! 完全一样，但一批后台单例活被刻意关掉，清单见 [`DISABLED_IN_SIDECAR`]。
//!
//! # 🔴 为什么没有「主从选举」「OS 锁」「自动晋升」
//!
//! 2026-08-23 做过一版完整的 leader/follower（`leader.rs`，546 行，OS 锁 + 心跳 + 晋升），
//! 08-24 全部 revert。08-24 复盘时发现一条决定性事实：
//!
//! **发货版里第二个 GUI 本来就起不来。** `tauri_plugin_single_instance` 按 app identifier
//! 把它弹回去（绿色版和安装版共用同一份 `tauri.conf.json` = 同一个 identifier，见 `lib.rs`
//! 单实例插件那段注释）。唯一能跳过单实例的入口就是 `--allow-multi-instance` ——
//! **一个客户永远不会打的开关**。
//!
//! 于是那 546 行选举逻辑对全体客户是「纯风险、零收益」：它防的那件事（客户开两个）
//! 单实例插件早就防住了，而它自己的 `claim()` 却跑在**每一个客户的启动路径**上。
//! 所以现在整套机制收进开关后面：
//!
//! - **不带开关 = 主实例，启动路径跟这功能出现之前一字不差**（这是它客户风险为零的全部理由）
//! - **带开关 = 钉死当并行调试实例，压根不参与任何抢占**
//!
//! 「不选举就会双主」这条顾虑在这个设计下不成立，逐组合枚举：
//!
//! | 组合 | 结果 |
//! |---|---|
//! | 不带 A 已开 + 不带 B 再开 | B 被单实例插件弹回（今天的行为，没变） |
//! | 不带 A + 带开关 B | A 主、B 调试实例 ✓ |
//! | 带开关 B 先开、不带 A 后开 | A 照常当主（B 没注册单实例插件、不持任何锁）✓ |
//! | 两个都带开关 | 两个调试实例，没人跑后台 |
//! | 只开了带开关的那一个 | 同上 |
//!
//! 双主一次都不出现。最后两行是**唯一的失败模式**，方向是「少干活」不是「多花钱」，
//! 有顶栏横幅明说，且只发生在自己人手上 —— 而选举方案竭力回避的「双调度线程 = 同一条
//! 定时任务跑两遍 = 双倍烧 token 且客户完全看不出来」，恰好是危险的那一头。
//!
//! 代价是**没有自动晋升**：关掉主实例之后，调试实例不会接管后台，要重启一次。
//! 对「构建 → 验 → 关掉」的开发用法无所谓。真要做客户侧的新旧版并行升级，
//! `git show 5c16023` 那版 OS 锁 + 晋升已被两轮把关磨过，捡回来即可。
//!
//! # 刻意不提供 `is_sidecar()`
//!
//! 门控**全部**发生在组合根 `lib.rs::setup()` 里，它自己手上就有那个 bool。
//! 这里留一个方便的 getter，下一个人一定会顺手在某个模块里调它 —— 而模块独立铁律禁止
//! 新模块之间横向 import（`check-module-coupling` 拦过一版）。只读缓存那两处
//! （`tasks::set_readonly` / `agent::threads::set_readonly`）同理，是**注入**不是查询。

use std::sync::atomic::{AtomicU8, Ordering};

const ROLE_HEADLESS: u8 = 0;
const ROLE_PRIMARY: u8 = 1;
const ROLE_SIDECAR: u8 = 2;

/// 默认 `headless`：无头模式（`action run` / `mcp serve` / `--selfcheck`）在 `tauri::Builder`
/// 之前就退出了，压根走不到 `setup()`，永远不会被标成主或从。
///
/// 🔴 **别把 headless 跟 sidecar 混成一档**。无头进程的能力是**完整的** ——
/// 把它也报成「降权了」，等于让每一条 CLI / MCP 调用都自称残废。
static ROLE: AtomicU8 = AtomicU8::new(ROLE_HEADLESS);

/// GUI 起来时由组合根调一次。`sidecar = true` 当且仅当命令行带了 `--allow-multi-instance`。
pub fn mark(sidecar: bool) {
    ROLE.store(
        if sidecar { ROLE_SIDECAR } else { ROLE_PRIMARY },
        Ordering::Relaxed,
    );
}

/// 并行调试实例里被关掉的后台单例活。**这份清单是给人看的诊断依据，必须跟
/// `lib.rs::setup()` 里真正的门控一一对应** —— 写了却没门控（或反过来）比不写更坏：
/// 排障的人会照着它排除掉正确的假设。改门控时回来改这里。
pub const DISABLED_IN_SIDECAR: &[&str] = &[
    // 唯一会真花钱的一条：两条调度线程各自到点触发同一批定时任务 = 同一件事跑两遍。
    "automation.scheduler",
    // 技能包是 include_str! 编进各自 exe 的，新旧两版内容不同，而同步是同名覆盖 ——
    // 两个实例会轮流把对方的技能刷掉，且完全没有报错。
    "skillpack.sync",
    // 15722 只有一个：两边各起一个代理会 EADDRINUSE，而 kill_orphan_proxy 会把对方的
    // 代理认成孤儿杀掉 —— 互相 kill，客户看到的是「codex 时好时坏」。
    "codex_proxy.selfheal",
    // 同一个 task_id 被两个进程轮询，出片时两边都往本机落一份、都去改同一条任务记录。
    "video.resume",
    // 新旧两版内嵌的小程序版本不同，各自 ensure 会互相按回自己那版。
    "bundled_apps.ensure",
    "metrics.rollup",
    // 防火墙 / codex config.toml / Claude Code hook 三条一次性自愈：幂等 ≠ 可并发，
    // 两个进程同时读-改-写同一个文件会写出交错的半截内容。
    "config.selfheal",
    // 说明书是 ~/.uking 里的共享文件、同名覆盖 —— 新旧两版会互相覆写对方刚发布的那份。
    "identity.publish",
    // device.json 经不起并发读-改-写（2026-08-19 换 Key 事故：新 key 没写回 6 个落点，
    // 客户机次次 401）。调试实例照旧**读**得到已有的 key，只是不去刷新。
    "device.key.refresh",
    "update.stage",
    // 崩溃会话记账。理由**不是**「标记全局一份」（08-19 已修成每实例一份 `.session-<pid>.json`），
    // 而是 `crashlog::is_live_sibling` 拿**本进程**的文件名去比对方的镜像名 ——
    // 绿色版 `U-King.exe` vs 安装版 `u-king-mini.exe` 名字不同，主实例会被判成「不是兄弟」→
    // 给还活着的它记一笔假 unclean_exit 并删掉它那份活标记。
    "crashlog.session",
    // 这两条是只读注入，不是「不跑」：读得到、用得了，只是不落盘 ——
    // 主实例那份用户正经在用的任务列表和 AI 续接 id 一个字节都不会被踩。
    "tasks.json.write",
    "agent-threads.json.write",
];

/// 给前端 / 诊断 / `runtime.instance.inspect` 看的一份角色说明。**只读**。
///
/// `ready` 按项目约定答的是**「能不能用」而不是「装没装」**：调试实例的界面完全能用，
/// 但它不是一个功能完整的 U-King —— 所以 `ready=false` + 把缺的那几样写进 `blockers`。
/// 报 `ready:true` 会让「开了两天定时任务一次没跑」这种事继续静默下去
/// （形状全对、conformance 全绿、世界是坏的）。
pub fn inspect() -> serde_json::Value {
    let raw = ROLE.load(Ordering::Relaxed);
    let role = match raw {
        ROLE_PRIMARY => "primary",
        ROLE_SIDECAR => "sidecar",
        _ => "headless",
    };
    let sidecar = raw == ROLE_SIDECAR;
    let blockers: Vec<String> = if sidecar {
        DISABLED_IN_SIDECAR
            .iter()
            .map(|k| format!("并行调试实例已关闭：{k}（由主实例负责；关掉本实例再正常启动即恢复）"))
            .collect()
    } else {
        Vec::new()
    };
    serde_json::json!({
        "role": role,
        "ready": !sidecar,
        "blockers": blockers,
        "pid": std::process::id(),
        "disabled_in_sidecar": DISABLED_IN_SIDECAR,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三态各自的 `ready` / `blockers` 形状。
    ///
    /// 🔴 承重的是**第一条**：无头必须 `ready=true` 且 `blockers` 空。把它跟 sidecar 混成
    /// 一档的话，每条 `action run` / `mcp serve` 都会自称降权 —— 而它们的能力是完整的。
    ///
    /// 不碰 env（`ROLE` 是纯内存原子量），所以不会串到别的模块的并行用例上
    /// （改 `UKING_TEST_HOME` 把 `automation` 用例打红过一次，本仓库记过）。
    /// 三态串行验完再还原，避免和同进程内其它用例互相看到中间态。
    #[test]
    fn three_roles_report_honestly() {
        let saved = ROLE.load(Ordering::Relaxed);

        ROLE.store(ROLE_HEADLESS, Ordering::Relaxed);
        let v = inspect();
        assert_eq!(v["role"], "headless");
        assert_eq!(v["ready"], true, "无头能力是完整的，不该自称降权");
        assert_eq!(v["blockers"].as_array().unwrap().len(), 0);

        mark(false);
        let v = inspect();
        assert_eq!(v["role"], "primary");
        assert_eq!(v["ready"], true);
        assert_eq!(v["blockers"].as_array().unwrap().len(), 0);

        mark(true);
        let v = inspect();
        assert_eq!(v["role"], "sidecar");
        assert_eq!(v["ready"], false, "降权必须报 ready=false，否则静默");
        assert_eq!(
            v["blockers"].as_array().unwrap().len(),
            DISABLED_IN_SIDECAR.len(),
            "blockers 必须逐条列出，一句笼统的「降权了」排不了障"
        );

        ROLE.store(saved, Ordering::Relaxed);
    }

    /// 清单本身的卫生：不重复、不空。
    ///
    /// 重复项会让 `blockers` 里出现两行一模一样的话 —— 看的人会以为是两处不同的东西。
    #[test]
    fn disabled_list_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for k in DISABLED_IN_SIDECAR {
            assert!(!k.is_empty());
            assert!(seen.insert(*k), "{k} 在清单里出现了两次");
        }
    }
}
