//! 别让电脑睡 —— 夜班助手的头号阻塞（需求榜 N1）。
//!
//! 调度线程活在本进程里，客户走人后 Windows 一睡，线程冻结，**一整晚一条都不跑、还不报错**
//! （错过的班次被静默推到下一个未来班次）。熔断、护栏、交班报告设计得再好，这条不解决全是空转。
//!
//! ## 🔴 诚实边界：它解决了多少，先说清楚
//!
//! - ✅ **挡得住空闲休眠**：没人动键鼠、系统空闲计时器到点自己睡 —— 这是夜班最常见的死法。
//! - ❌ **挡不住合上盖子**，也挡不住开始菜单点「睡眠」/ 按电源键。
//!   这些是用户**显式的电源动作**，`SetThreadExecutionState` 按设计就不拦
//!   —— 真拦得住的话，随便哪个程序都能让你的笔记本合盖不睡，那是更坏的世界。
//!   要覆盖合盖只有一条路：改电源计划的「合上盖子时」动作。**那是改客户的系统设置，我们不做**
//!   （开发宪法第 10 条：不碰用户真实状态）。
//!
//! 这条边界很重要，因为设计文档里那个故事恰恰是「客户 23:00 **合盖**走人」——
//! 本模块**治不了那个**。它治的是「人走了但机器开着」。界面和只读动作必须把这句话原样说给客户听，
//! 不能让人以为合盖也能跑（那种误解比不做这功能更坏：他真的走了，任务真的没跑）。
//!
//! ## 🔴 为什么必须在**长驻线程**里调
//!
//! `SetThreadExecutionState` 是**按线程**记账的：哪个线程调的，就算哪个线程的请求，
//! **线程一退，请求跟着没**。所以绝不能从 Tauri 的命令线程（线程池，用完就还回去）调 ——
//! 那样设完就没了，界面显示「已开启」，机器照睡不误，而且没有任何报错。
//! 本模块的 [`apply`] 因此**只允许调度线程调**（`automation.rs` 的那个 `loop`），
//! 由组合根注入（同 `Runner` / `Notifier` 的手法），每 tick 重申一次，天然自愈。
//!
//! 反过来这也是它的安全网：**进程一退（正常退出 / 崩溃 / 被强杀），操作系统自动收回请求**。
//! 「抑制没撤销 → 客户电脑从此不睡了」这个比不做更坏的结果，在 Windows 上由内核兜住了。
//!
//! ## 独立可插拔
//! 纯 std、零第三方 crate、不碰 `AppHandle`；`automation.rs` **不 import 本模块**，
//! 靠 lib.rs 注入。删掉本模块只动 lib.rs（去 mod + 去注入）+ 前端那一行说明。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// 当前是否已经请求「别睡」。只由调度线程写，其它线程只读（给状态回显用）。
static ON: AtomicBool = AtomicBool::new(false);
/// 最近一次系统调用的返回值（0 = 失败）。留着给诊断用 —— 「设了但系统没认」得看得出来。
static LAST_RET: AtomicU32 = AtomicU32::new(0);
/// macOS 上那个 `caffeinate` 子进程的 pid（0 = 没起）。
static MAC_PID: AtomicU32 = AtomicU32::new(0);

// —— Windows 常量（`winbase.h`）——
// 🔴 **故意不带 `ES_DISPLAY_REQUIRED`**：那会让屏幕整夜亮着，正是要避免的第二件事
// （客户会以为电脑坏了，而且真费电）。只要系统别睡，屏幕该黑就黑。
#[cfg(windows)]
const ES_CONTINUOUS: u32 = 0x8000_0000;
#[cfg(windows)]
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

#[cfg(windows)]
extern "system" {
    /// 返回**调用之前**的状态标志；0 表示失败。
    /// 这个返回值是本功能唯一不用提权就能拿到的证据（`powercfg /requests` 要管理员）。
    fn SetThreadExecutionState(flags: u32) -> u32;
}

/// 让「别睡」的请求跟 `on` 一致。**只允许长驻的调度线程调**（见文件头）。
///
/// 幂等：每个 tick 都可以无脑调一次，状态没变就不碰系统。返回「这次有没有真的改动」。
pub fn apply(on: bool) -> bool {
    if ON.load(Ordering::Relaxed) == on {
        return false;
    }
    let ok = set_native(on);
    // 系统没认就别把状态记成已开 —— 回显骗人比功能缺失更难查。
    ON.store(on && ok, Ordering::Relaxed);
    ok
}

#[cfg(windows)]
fn set_native(on: bool) -> bool {
    // 开：ES_CONTINUOUS 表示「一直有效直到我改口」，ES_SYSTEM_REQUIRED 表示「别让系统睡」。
    // 关：只剩 ES_CONTINUOUS = 清掉所有要求，但仍是「持续」语义（这就是官方的撤销写法）。
    let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
    let prev = unsafe { SetThreadExecutionState(flags) };
    LAST_RET.store(prev, Ordering::Relaxed);
    prev != 0
}

/// macOS：`caffeinate -i` 抑制空闲休眠。
///
/// 🔴 带 `-w <本进程 pid>`：让 caffeinate **自己盯着我们**，我们一没（正常退出 / panic / 被 kill）
/// 它跟着退。比「在退出路径里记得杀掉它」可靠得多 —— 那种写法漏掉任何一条退出路径，
/// 客户的 Mac 就从此不睡了，而这正是比不做更坏的那个结果。
#[cfg(target_os = "macos")]
fn set_native(on: bool) -> bool {
    if on {
        match std::process::Command::new("caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                MAC_PID.store(child.id(), Ordering::Relaxed);
                true
            }
            Err(_) => false,
        }
    } else {
        let pid = MAC_PID.swap(0, Ordering::Relaxed);
        if pid == 0 {
            return true;
        }
        std::process::Command::new("kill")
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn set_native(_on: bool) -> bool {
    false // 其它平台不支持：如实返回失败，别假装开了
}

/// 现在是不是正在请求「别睡」。给只读动作回显用（任何线程都能读）。
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// 这台机器上这功能到底能不能用 + 它治不了什么。给 `runtime.automation.inspect` 当数据发 ——
/// **产品边界当数据发**，GUI 文案 / CLI / MCP 读的是同一句话，不会三处各自跑偏。
pub fn status() -> serde_json::Value {
    serde_json::json!({
        "supported": cfg!(any(windows, target_os = "macos")),
        "on": is_on(),
        "prevents_idle_sleep": true,
        // 🔴 这两条必须出现在输出里：客户以为合盖也能跑，那他真的走了、任务真的没跑。
        "prevents_lid_close": false,
        "prevents_manual_sleep": false,
        "note": if cfg!(any(windows, target_os = "macos")) {
            "有启用的定时任务时，会阻止电脑「闲着自己睡」。但**合上盖子、或手动点睡眠，仍然会睡** —— 那是你显式的电源动作，任何程序都拦不住（也不该拦）。夜里要跑任务，请让电脑开着盖子。"
        } else {
            "这个系统上不支持阻止休眠，定时任务只在电脑醒着且 U-King 开着时才跑。"
        },
        "last_call_ret": LAST_RET.load(Ordering::Relaxed),
    })
}

/// ★ 无头自检用的**决定性探针**：在一条**用完即退的临时线程**上真调一次系统 API，
/// 把「系统认没认」读回来。
///
/// 为什么这么写：
/// - `SetThreadExecutionState` 返回的是**调用前**的状态，所以「设了之后再读一次」才能证明它生效。
/// - 但读的那一下（用 `ES_CONTINUOUS`）会**清掉** `SYSTEM_REQUIRED`，所以绝不能在
///   真正持有请求的那条线程上探 —— 会把真实的抑制擦掉。开条临时线程探，
///   按线程记账的特性正好让它跟真实状态完全隔离，线程一退什么都不剩。
/// - `powercfg /requests` 要管理员权限，客户机上跑不了，所以不能拿它当判据。
pub fn probe() -> serde_json::Value {
    #[cfg(windows)]
    {
        let h = std::thread::spawn(|| {
            // 干净线程的初始状态（应为 0 标志位，只带 ES_CONTINUOUS 语义）
            let before = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            // 再设一次「只要持续、不要求」——返回值就是**我们刚设进去的那个状态**
            let readback = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            (before, readback)
        });
        let (before, readback) = h.join().unwrap_or((0, 0));
        let accepted = before != 0 && readback != 0 && (readback & ES_SYSTEM_REQUIRED) != 0;
        return serde_json::json!({
            "platform": "windows",
            "api_accepted": accepted,
            "before": before,
            "readback": readback,
            // 屏幕不能被我们点亮 —— 这条也要有证据，不然「不带 DISPLAY_REQUIRED」只是句口号
            "display_kept_on": (readback & 0x0000_0002u32) != 0,
        });
    }
    #[cfg(not(windows))]
    {
        serde_json::json!({
            "platform": if cfg!(target_os = "macos") { "macos" } else { "other" },
            "api_accepted": cfg!(target_os = "macos"),
            "note": "macOS 走 caffeinate 子进程，探针只报支持与否；真实生效靠 automation 那条链路",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 这个功能最坏的失败不是「没生效」，是「生效了收不回来」——
    /// 客户电脑从此不睡。所以 apply 必须是可逆且幂等的。
    #[test]
    fn apply_is_idempotent_and_reversible() {
        // 从干净状态起（本用例跑在自己的测试线程上，按线程记账不会影响别处）
        let first = apply(true);
        assert!(is_on() || !cfg!(any(windows, target_os = "macos")), "开了就该回显开着");
        let second = apply(true);
        assert!(!second, "同样的状态再调一次不该碰系统（幂等）");
        let _ = first;

        apply(false);
        assert!(!is_on(), "关了必须回显关着 —— 收不回来是这功能最坏的结局");
        assert!(!apply(false), "重复关同样幂等");
    }

    /// 🔴 边界必须以**数据**形式发出去，不能只写在界面文案里：
    /// GUI / CLI / MCP 三处读同一句话，才不会各自跑偏成「合盖也能跑」。
    #[test]
    fn status_admits_what_it_cannot_do() {
        let s = status();
        assert_eq!(s["prevents_lid_close"], serde_json::json!(false), "合盖挡不住，必须如实说");
        assert_eq!(s["prevents_manual_sleep"], serde_json::json!(false));
        let note = s["note"].as_str().unwrap_or("");
        assert!(!note.is_empty(), "得给客户一句人话");
        if cfg!(any(windows, target_os = "macos")) {
            assert!(note.contains("盖"), "那句人话里必须点明合盖这件事");
        }
    }

    /// 探针必须真的调到系统 API 并拿到系统的回答，而不是返回一句「大概行」。
    #[cfg(windows)]
    #[test]
    fn probe_reads_back_from_the_os() {
        let p = probe();
        assert_eq!(p["api_accepted"], serde_json::json!(true), "系统没认这个请求");
        // ★ 不带 ES_DISPLAY_REQUIRED 这条也要有证据：读回来的标志位里不许有它
        assert_eq!(p["display_kept_on"], serde_json::json!(false), "不许让屏幕整夜亮着");
    }
}
