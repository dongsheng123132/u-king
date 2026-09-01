//! 「这个工作台会话，上一轮接的是哪条 AI 线程」—— **落盘**，重启后还在。
//!
//! # 它治什么病
//!
//! `claude -p` / `codex exec` 是一轮一进程：多轮靠 `--resume <session_id>` /
//! `resume <thread_id>` 续接。谁记住这个 id，谁就决定了「下一句话是接着上文说，
//! 还是从零开始」。
//!
//! 原来记在 `claude.rs::last_sessions()` / `codex.rs::last_threads()` 里，
//! 两个 `OnceLock<Mutex<HashMap<..>>>` —— **纯内存**。于是：
//!
//! - 关掉 U-King（哪怕不重启电脑）→ 映射没了；
//! - 下次打开，工作台会话还在（`tasks.json` 落了盘）、聊天记录也还在
//!   （`ChatPanel` 存了 localStorage）→ **界面上一切如常**；
//! - 用户接着上文问一句「那就按刚才说的改吧」→ 后端没有 resume id，
//!   起一个**全新会话**，AI 完全不知道「刚才」是什么。
//!
//! 最坏的形状不是「丢了」，是**丢了但看不出来**：屏幕上明明白白摆着上下文，
//! 只有模型不知道。用户于是判断成「这 AI 怎么变笨了」，而不是「状态没存」。
//!
//! # 三条纪律
//!
//! 1. **写盘是原子的**（临时文件 + rename）。这份文件在用户正干活时被反复重写，
//!    半截文件 = 全部会话的续接能力一起丢（宪法第 10 条）。
//! 2. **内存是缓存，盘是真相**，但读只在进程首次访问时做一次 —— 每轮对话都读盘
//!    是在热路径上加 IO，没必要。同进程内谁都改不了它，多进程的情况见下条。
//! 3. **过期条目要清**。会话 id 不会自己失效，但那家工具的会话记录会被它自己清理；
//!    留着一堆指向已删会话的 id，`--resume` 会当场失败。超过 `KEEP_DAYS` 的直接丢，
//!    退回「开新会话」—— 这是安全的降级方向（少一段上文，不是错一段上文）。
//!
//! # 不做乐观并发的理由
//!
//! U-King 是单实例（多开会导致定时任务 N 倍烧 token，已另行拦截）。真出现两个进程
//! 同时写，最坏结果是一条会话的 resume id 被覆盖成另一条 —— 退化成「开个新会话」，
//! 跟今天每次重启的行为一样，不会写坏别的东西。为这个场景引一套 version 对账
//! 不划算，但**这个判断依赖单实例**，哪天允许多开了要回来重看这段。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 多久没用的续接 id 就丢掉（见模块头第 3 条）。
const KEEP_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;

/// 一条记忆：某个工作台会话在某个大脑上接的那条线程。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Thread {
    /// 那家工具自己的 session_id / thread_id。
    id: String,
    /// 最近一次续上的时间（epoch ms），过期清理用。
    #[serde(default)]
    at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    version: u32,
    /// agent（`claude` / `codex`）→ task_id → 线程。
    #[serde(default)]
    agents: HashMap<String, HashMap<String, Thread>>,
}

fn path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking").join("agent-threads.json")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 从**指定路径**装载并清过期。
///
/// 收 `p` 而不是自己去问 `path()`，是为了让「关掉再打开还在不在」这件事**能被测**：
/// 测试可以在临时目录里做一次真的写-读往返，而不用去改 `USERPROFILE`
///（并行测试串 env 是本仓库栽过的坑，报错里的「实际值」看着还都对）。
/// 只测纯序列化会漏掉真正会坏的那一段 —— 原子写、Windows 的 rename 覆盖、目录不存在。
fn load_from(p: &Path) -> File {
    let mut f: File = std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str::<File>(&s).ok())
        .unwrap_or_default();
    let cutoff = now_ms() - KEEP_DAYS * DAY_MS;
    for m in f.agents.values_mut() {
        m.retain(|_, t| t.at >= cutoff && !t.id.trim().is_empty());
    }
    f.version = 1;
    f
}

/// 进程内缓存。首次访问时从盘上装载并顺手清过期。
fn cache() -> &'static Mutex<File> {
    static C: OnceLock<Mutex<File>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(load_from(&path())))
}

/// 原子落盘：先写同目录的临时文件，再 rename 覆盖。**不带 `?`** ——
/// 存不下续接 id 不该让这一轮对话失败（那才是更大的损失），
/// 失败只是退回「下次开新会话」，跟没这个模块时一个样。
fn flush_to(p: &Path, f: &File) {
    let Some(dir) = p.parent() else { return };
    let _ = std::fs::create_dir_all(dir);
    let Ok(s) = serde_json::to_string_pretty(f) else { return };
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, s).is_err() {
        return;
    }
    // 直接 rename 覆盖。**别在这之前先 remove_file** —— Rust 的 `fs::rename` 在 Windows 上
    // 走的是 `MoveFileEx(..., MOVEFILE_REPLACE_EXISTING)`，本来就能覆盖；先删一次反而凭空
    // 造出一个「文件不存在」的窗口，那一瞬崩掉就把**全部**会话的续接能力一起丢了。
    //（这里原本写着「Windows 上 rename 覆盖会失败，得先删」并照做了 —— 是变异验证把它证伪的：
    // 把那行删掉，用例照样全绿。绿的原因不是用例没盖到，是那行本来就没在起作用。）
    if std::fs::rename(&tmp, p).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// 只读模式开关。**默认关**（写照旧），由组合根 `lib.rs` 在本进程是「并行调试实例」时打开。
///
/// 🔴 为什么是注入而不是去问 `instance` 模块：模块独立铁律禁止新模块之间横向 import
/// （`check-module-coupling` 会当场拦下 —— 它拦过这一版）。要共享要么下沉到公共层，
/// 要么让组合根去问、把结果传进来。这里选后者：`threads.rs` 压根不需要认识「并行实例」，
/// 它只需要知道「这轮要不要落盘」。
static READONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 并行调试实例启动时由 `lib.rs` 调一次。
pub fn set_readonly(on: bool) {
    READONLY.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn flush(f: &File) {
    // 🔴 **并行调试实例只读**（见 `instance.rs`）。两个 U-King 并行跑时各有一份 `cache()`，
    // 都往同一个 `agent-threads.json` 落盘 = 最后写入者获胜（宪法 16 禁的那条）。
    // 更糟的是这份存的正是「续接哪条 AI 线程」—— 被对方的旧快照覆盖掉，
    // 用户会看到满屏上下文而 AI 完全不知道刚才是什么，判断成「这 AI 变笨了」。
    //
    // 调试实例照旧在**进程内**记（`cache()` 是内存的，续接在本次运行里完全正常），
    // 只是关掉就没了 —— 而主实例那份不会被踩。失败方向：调试实例少存一点，不是主实例丢状态。
    //
    // **刻意放在 `flush` 而不是 `flush_to`**：`flush_to` 收显式路径，是给单元测试做
    // 真 IO 往返用的（见它自己的注释），拦在那儿会让测试恒绿地什么都不写。
    if READONLY.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    flush_to(&path(), f);
}

/// 这个工作台会话上一轮接的是哪条线程（没有 = 该开新会话）。
pub fn recall(agent: &str, task_id: &str) -> Option<String> {
    cache()
        .lock()
        .ok()?
        .agents
        .get(agent)?
        .get(task_id)
        .map(|t| t.id.clone())
}

/// 记住这一轮的线程 id。**每轮都调**（那家工具可能在续接后换了 id），
/// id 相同就只刷新时间戳、不重复写盘。
pub fn remember(agent: &str, task_id: &str, id: &str) {
    let id = id.trim();
    if id.is_empty() || task_id.is_empty() {
        return;
    }
    let Ok(mut f) = cache().lock() else { return };
    let m = f.agents.entry(agent.to_string()).or_default();
    let now = now_ms();
    match m.get_mut(task_id) {
        // 同一条线程连着聊，只是刷新「最近用过」——一天最多写一次盘，别在热路径上抖 IO。
        Some(t) if t.id == id => {
            let stale = now - t.at > DAY_MS;
            t.at = now;
            if !stale {
                return;
            }
        }
        _ => {
            m.insert(task_id.to_string(), Thread { id: id.to_string(), at: now });
        }
    }
    flush(&f);
}

/// 忘掉这个会话的上下文（「新对话」按钮）。下一轮从零开始。
pub fn forget(agent: &str, task_id: &str) {
    let Ok(mut f) = cache().lock() else { return };
    if let Some(m) = f.agents.get_mut(agent) {
        if m.remove(task_id).is_some() {
            flush(&f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一个自己的临时目录 —— 不共享路径就不需要锁，也不会跟别的用例串。
    /// **故意让父目录不存在**：`flush_to` 必须自己把它建出来（首次运行的真实形状）。
    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("uking-threads-test-{tag}-{}", std::process::id()))
            .join("agent-threads.json")
    }

    /// ★ 这条就是「关掉 U-King 再打开，会话还接得上吗」的判据。
    ///
    /// 做一次**真的**写-读往返（真文件、真目录、真 rename），而不是只把结构体
    /// 序列化再反序列化 —— 会坏的正是原子写、Windows 的 rename 覆盖、目录不存在
    /// 这几段，纯序列化测试对它们全盲。
    #[test]
    fn thread_id_survives_a_restart() {
        let p = tmp_path("restart");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());

        // 第一次「运行」：记下这轮的 session_id。
        let mut f = load_from(&p);
        assert!(f.agents.is_empty(), "全新机器上该是空的");
        f.agents
            .entry("claude".into())
            .or_default()
            .insert("sess-abc".into(), Thread { id: "sid-1".into(), at: now_ms() });
        flush_to(&p, &f);
        assert!(p.exists(), "落盘没成 —— 重启后必然从零开始");

        // 覆盖写一次（同一个会话又聊了一轮，id 变了）—— 落盘目标已存在的那条路。
        let mut f = load_from(&p);
        f.agents.get_mut("claude").unwrap().insert(
            "sess-abc".into(),
            Thread { id: "sid-2".into(), at: now_ms() },
        );
        flush_to(&p, &f);

        // 第二次「运行」（= 重启后）：新进程从盘上装载。
        let back = load_from(&p);
        assert_eq!(
            back.agents["claude"]["sess-abc"].id, "sid-2",
            "重启后没取回最近那条线程 —— 用户会得到一个失忆的 AI"
        );
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 过期的、以及 id 为空的条目，装载时就该被丢掉。
    /// 空 id 尤其要挡：它会拼出 `--resume ` 这种半条命令。
    #[test]
    fn expired_and_blank_threads_are_dropped_on_load() {
        let p = tmp_path("expire");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        let now = now_ms();

        let mut f = File::default();
        let m = f.agents.entry("claude".into()).or_default();
        m.insert("fresh".into(), Thread { id: "s-new".into(), at: now });
        m.insert("stale".into(), Thread { id: "s-old".into(), at: now - (KEEP_DAYS + 1) * DAY_MS });
        m.insert("blank".into(), Thread { id: "  ".into(), at: now });
        flush_to(&p, &f);

        let back = load_from(&p);
        let m = &back.agents["claude"];
        assert_eq!(m.len(), 1, "该只剩没过期且 id 非空的那条，实得 {:?}", m.keys().collect::<Vec<_>>());
        assert_eq!(m["fresh"].id, "s-new");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 文件坏了（半截 / 不是 JSON）不许 panic，也不许把坏内容当成有效状态。
    /// 退回「空表 = 开新会话」是安全方向：少一段上文，不是错一段上文。
    #[test]
    fn corrupt_file_degrades_to_empty_not_panic() {
        let p = tmp_path("corrupt");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{\"agents\": {\"claude\": {\"a\": ").unwrap(); // 半截
        assert!(load_from(&p).agents.is_empty(), "坏文件该退成空表");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 两个大脑各记各的：codex 的 thread_id 不许被 claude 的覆盖掉。
    #[test]
    fn agents_do_not_share_a_namespace() {
        let p = tmp_path("ns");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        let now = now_ms();
        let mut f = File::default();
        f.agents.entry("claude".into()).or_default()
            .insert("same-task".into(), Thread { id: "claude-sid".into(), at: now });
        f.agents.entry("codex".into()).or_default()
            .insert("same-task".into(), Thread { id: "codex-tid".into(), at: now });
        flush_to(&p, &f);

        let back = load_from(&p);
        assert_eq!(back.agents["claude"]["same-task"].id, "claude-sid");
        assert_eq!(back.agents["codex"]["same-task"].id, "codex-tid");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn readonly_keeps_memory_but_defers_disk_until_writable() {
        struct ResetReadonly;
        impl Drop for ResetReadonly {
            fn drop(&mut self) {
                set_readonly(false);
            }
        }

        let sb = crate::testsandbox::enter_raw("threads-readonly-observable");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::set_var("HOME", sb.root());
        let _reset = ResetReadonly;
        let p = sb.root().join(".uking/agent-threads.json");
        let _ = std::fs::remove_file(&p);

        set_readonly(true);
        remember("test-ro-agent", "test-ro-task", "sid-memory-only");
        assert_eq!(recall("test-ro-agent", "test-ro-task").as_deref(), Some("sid-memory-only"));
        assert!(!p.exists(), "只读轮不许写出 agent-threads.json");

        set_readonly(false);
        remember("test-ro-agent", "test-ro-task", "sid-persisted");
        let disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(disk.pointer("/agents/test-ro-agent/test-ro-task/id").and_then(|v| v.as_str()), Some("sid-persisted"));
    }

    #[test]
    fn remember_debounces_and_forget_updates_disk() {
        struct ResetReadonly;
        impl Drop for ResetReadonly {
            fn drop(&mut self) {
                set_readonly(false);
            }
        }

        let sb = crate::testsandbox::enter_raw("threads-debounce-forget");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::set_var("HOME", sb.root());
        let _reset = ResetReadonly;
        set_readonly(false);
        let p = sb.root().join(".uking/agent-threads.json");
        let _ = std::fs::remove_file(&p);

        remember("test-debounce-agent", "test-debounce-task", "sid-first");
        assert!(p.exists(), "首次 remember 必须落盘");
        std::fs::remove_file(&p).unwrap();
        remember("test-debounce-agent", "test-debounce-task", "sid-first");
        assert!(!p.exists(), "相同 id 的去抖 remember 不许重建文件");

        remember("test-debounce-agent", "test-debounce-task", "sid-second");
        let disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(disk.pointer("/agents/test-debounce-agent/test-debounce-task/id").and_then(|v| v.as_str()), Some("sid-second"));

        forget("test-debounce-agent", "test-debounce-task");
        assert_eq!(recall("test-debounce-agent", "test-debounce-task"), None);
        let disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(disk.pointer("/agents/test-debounce-agent/test-debounce-task").is_none());

        std::fs::remove_file(&p).unwrap();
        forget("test-debounce-agent", "test-debounce-absent");
        assert!(!p.exists(), "forget 不存在的 key 不许无故写盘");
    }

    #[test]
    fn load_keeps_entries_just_inside_retention_and_drops_old_ones() {
        let p = tmp_path("retention-boundary");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        let now = now_ms();
        let mut f = File::default();
        let m = f.agents.entry("test-retention-agent".into()).or_default();
        m.insert("inside".into(), Thread { id: "sid-inside".into(), at: now - KEEP_DAYS * DAY_MS + 5_000 });
        m.insert("outside".into(), Thread { id: "sid-outside".into(), at: now - KEEP_DAYS * DAY_MS - 5_000 });
        flush_to(&p, &f);

        let back = load_from(&p);
        let m = &back.agents["test-retention-agent"];
        assert_eq!(m.get("inside").map(|t| t.id.as_str()), Some("sid-inside"));
        assert!(!m.contains_key("outside"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
