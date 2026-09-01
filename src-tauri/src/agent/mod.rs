//! AI Agent 驱动 —— 结构化复原 Codex 能力（Phase 4+）。
//!
//! 终端面板（term.rs）渲染裸 TUI；这里走结构化事件流，给 claude/codex 加卡片面板。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ulog;

pub mod chat;
pub mod claude;
mod cmdline;
pub mod codex;
mod launcher;
mod protocol;
pub mod threads;

/// 「AI 工具专项修复」体检的两个对象。加一个新的 agent 只改这里。
pub const PROBE_TARGETS: &[&str] = &["claude", "codex"];

/// 追加给每个大脑的运行环境约束。**放公共层是因为它对哪个大脑都成立** ——
/// 复制成两份，下次改措辞必然只改一边（宪法第 8 条）。
///
/// 前两条防死锁：常驻服务是无头模式的头号杀手 —— 它不会自己结束，工具调用就永远等不到，
/// 整轮对话跟着挂住，界面上只剩一个转圈。pc-***（2026-08-03）客户问「帮我调试一下这个软件」，
/// claude 在 Bash 里起了项目的 `serve --port 8790`，就这么挂了 25 分钟。
/// 后两条是「小白友好」：这个面板对面坐的多半是不看命令行的人。
pub(crate) const GUARD_PROMPT: &str = "\
【运行环境】你正跑在 U-King 的对话面板里（无头模式，没有终端可以让人按 Ctrl+C），对面多半是不懂命令行的普通用户。
1. 绝不在前台运行不会自己结束的命令：dev / serve / start / watch / tail -f / ping -t 之类。\
需要验证服务能不能起，就用后台方式起并立刻返回，或者直接告诉用户怎么自己双击启动。
2. 每条命令都要能在两分钟内结束。预计更久的（装依赖、构建），先用一句话说明再做。
3. 说人话：少贴命令和路径，多说「我在做什么 / 做完了什么 / 你现在能拿到什么」。
4. 做完了要给出成品文件的完整路径，别只说「已生成」。";

/// 「多久没动静算它挂了」。判据是**静默时长**，不是总时长 —— 一个正经的大活跑二十分钟是合理的
/// （它一路都在吐事件），但五分钟一个字节都没有，那边基本就是死了。
///
/// 为什么是 5 分钟：我们给 claude 的 Bash 工具设了 3 分钟硬上限（`BASH_MAX_TIMEOUT_MS`），
/// 正常静默不会超过三分多；流式又开着 partial 事件，模型在想的时候也一直有动静。
/// 留 5 分钟余量既够宽容，又不至于让人干等。
pub(crate) const STALL_SECS: u64 = 300;

/// 静默看门狗 —— 「多久没动静就把整棵进程树收掉」。
///
/// **两个大脑共用一份**：claude 和 codex 的卡死形态一模一样（在等一个永远不会结束的子进程），
/// 判据和处置也必须一模一样 —— 否则「Claude 会自己停、Codex 会一直挂着」就成了客户眼里的玄学。
///
/// pc-***（2026-08-03）就是死在没有这条线上：客户问「帮我调试一下这个软件」，claude 在 Bash 里
/// 起了项目的 `serve --port 8790`（永不退出），把自己挂死 25 分钟，UI 上只有一个转圈。
pub(crate) struct Watchdog {
    beat: Arc<Mutex<Instant>>,
    finished: Arc<AtomicBool>,
    stalled: Arc<AtomicBool>,
}

impl Watchdog {
    /// 盯住 `pid`：静默超过 `stall` 就 kill 整棵树。
    ///
    /// **杀树不杀单进程**：挂死的典型形态正是它在等一个自己派生出去的常驻子进程，只 kill 父的话
    /// 那个子进程会留成孤儿，继续占端口、吃 CPU、写文件。树一倒，stdout 管道随之关闭，
    /// 调用方那个 `lines()` 循环自然收尾 —— 不需要额外的跨线程唤醒。
    pub fn spawn(pid: u32, stall: Duration) -> Self {
        let wd = Self {
            beat: Arc::new(Mutex::new(Instant::now())),
            finished: Arc::new(AtomicBool::new(false)),
            stalled: Arc::new(AtomicBool::new(false)),
        };
        let (beat, finished, stalled) = (wd.beat.clone(), wd.finished.clone(), wd.stalled.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                let idle = beat.lock().map(|b| b.elapsed()).unwrap_or_default();
                if idle > stall {
                    stalled.store(true, Ordering::Relaxed);
                    chat::kill_tree_by_pid(pid);
                    return;
                }
            }
        });
        wd
    }

    /// 收到一行输出就打一次卡。**空行/非 JSON 行也要打** —— 它们同样证明那头还没死。
    pub fn beat(&self) {
        if let Ok(mut b) = self.beat.lock() {
            *b = Instant::now();
        }
    }

    /// 这一轮收尾了，看门狗下班（别让它在后台空转到超时）。
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }

    /// 是不是被判了卡死。**调用方判定时必须把它排在 interrupted 前面**：看门狗靠杀进程收场，
    /// 进程退出状态跟「人按了停止」长得一模一样，谁在前面谁定性 —— 摆错顺序就会把「它挂了」
    /// 报成「你停了它」，客户从此再也不会告诉我们这里坏过。
    pub fn stalled(&self) -> bool {
        self.stalled.load(Ordering::Relaxed)
    }
}

/// 正在跑的那些轮 —— 给 [`chat_inspect`] 用，**这是 pc-*** 那次唯一真正缺的东西**。
///
/// 那天我人连上了客户机，问的是「现在这一轮卡住了吗、卡在哪一步、卡了多久」，
/// 而 55 个动作里**没有一个能回答** —— 只能手敲 PowerShell 量 CPU 增量、翻 transcript
/// 时间戳、数子进程，来回一个多小时。日志（[`TurnLog`]）解决的是「事后查得出」，
/// 这张表解决的是「当场问得到」：AI 自己调一次就知道，不必有人守在机器前。
fn live() -> &'static Mutex<std::collections::HashMap<String, LiveTurn>> {
    static L: std::sync::OnceLock<Mutex<std::collections::HashMap<String, LiveTurn>>> =
        std::sync::OnceLock::new();
    L.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

struct LiveTurn {
    engine: String,
    model: String,
    started: Instant,
    last: Instant,
    phase: String,
    pid: u32,
    /// 上次落盘的时刻（见 [`flush_live`] 的节流理由）。
    flushed: Instant,
}

/// 在跑的那几轮**落一份盘**：`~/.uking/logs/.chat-live.json`。
///
/// 🔴 **不落盘这个动作就是废的**：远程排障时跑的是 `U-King.exe action run runtime.chat.inspect`
/// —— 那是**另一个进程**，进程内的表它一个字都看不见，只会返回一个空 `running`，
/// 而空数组在客户正卡着的时候等于说「一切正常」。宁可没有这个动作，也不能有一个会说反话的。
///
/// **按阶段变化 + 5 秒节流**写：流式 text 事件一轮几百个，每个都写盘是把日志目录当磁盘压测。
/// 5 秒的误差对 150s/300s 这两条线毫无影响。
fn flush_live(map: &std::collections::HashMap<String, LiveTurn>) {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let items: Vec<serde_json::Value> = map
        .iter()
        .map(|(task, l)| {
            serde_json::json!({
                "task_id": task,
                "engine": l.engine,
                "model": l.model,
                "pid": l.pid,
                "phase": l.phase,
                "elapsed_secs": l.started.elapsed().as_secs(),
                // 存「写盘那一刻已经静默了多久」；读的一方再加上文件之后又过去的时间。
                "idle_at_write_secs": l.last.elapsed().as_secs(),
            })
        })
        .collect();
    let body = serde_json::json!({
        "owner_pid": std::process::id(),
        "written_at": now_unix,
        "turns": items,
    });
    let p = ulog::log_dir().join(LIVE_FILE);
    let Some(parent) = p.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    // 临时文件 + rename：读的一方可能正好撞上写。读到半份 JSON 会解析失败 → 又变成
    // 「什么都没在跑」这个反话，正是上面要避免的那件事。
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, body.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

const LIVE_FILE: &str = ".chat-live.json";

/// 一轮对话的**静默账本** —— 回答的是「卡在哪一步」，不是「卡了多久」。
///
/// pc-***（2026-08-03）逼出来的：客户 u-chat 里同一条消息静默了 21 分 19 秒，
/// 我们人连到了那台机器、拿到了完整 transcript、量了网络（首字节 0.94s、流式 1274 个 chunk
/// 最大间隔 0.92s）、翻了服务端同窗口 311 条请求（最慢 83 秒）—— **仍然定不了那 21 分钟
/// 花在哪**，因为 `~/.uking/logs/` 里压根没有 chat 这一份。
///
/// [`Watchdog`] 管的是「卡死了要收尾」，这份账本管的是「收尾之后查得出原因」。两件事，
/// 少哪个都不行：只有看门狗 = 每次都只知道「又挂了」；只有日志 = 客户已经等了二十分钟。
///
/// **只记时长和阶段**：不记提示词、不记工具入参、不记文件内容 —— 客户的活是他自己的，
/// 而这份日志会随「技术支持」整份上传（`ulog::all_tails` 扫目录自动带上）。
pub(crate) struct TurnLog {
    module: &'static str,
    /// 这一轮在 [`live`] 表里的键（= task_id）。
    key: String,
    t0: Instant,
    last: Instant,
    /// 当前处在哪个阶段（下一段静默要算到它头上）。
    phase: String,
    /// 最长的一段静默 + 它发生在哪个阶段。
    worst: (Duration, String),
    events: u32,
    tools: u32,
}

impl TurnLog {
    /// `engine` = claude / codex / uking；`module` 决定落到 `~/.uking/logs/<module>.log`；
    /// `task_id` 是这一轮在 [`live`] 表里的键（同一个会话重发会覆盖上一条，正确 —— 上一轮已收尾）。
    pub fn start(
        module: &'static str,
        engine: &str,
        task_id: &str,
        model: Option<&str>,
        resumed: bool,
    ) -> Self {
        let now = Instant::now();
        let m = model.unwrap_or("(默认)");
        ulog::write(module, &format!("turn start engine={engine} model={m} resume={resumed}"));
        if let Ok(mut g) = live().lock() {
            g.insert(
                task_id.to_string(),
                LiveTurn {
                    engine: engine.to_string(),
                    model: m.to_string(),
                    started: now,
                    last: now,
                    phase: "启动中".into(),
                    pid: 0,
                    flushed: now,
                },
            );
            flush_live(&g);
        }
        Self {
            module,
            key: task_id.to_string(),
            t0: now,
            last: now,
            // 首段静默既包含「进程起没起来」也包含「模型首字节」，合成一段就分不清了；
            // 但再细分需要在 spawn 后单独打一拍 —— 见 [`TurnLog::spawned`]。
            phase: "启动中".into(),
            worst: (Duration::ZERO, "启动中".into()),
            events: 0,
            tools: 0,
        }
    }

    /// 子进程已经起来了。把「起进程」和「等模型首字节」切成两段 ——
    /// 这两件事的处置完全不同（一个查装机，一个查线路），混成一段等于没记。
    pub fn spawned(&mut self, pid: u32) {
        self.enter_inner("等模型首字节");
        if let Ok(mut g) = live().lock() {
            if let Some(l) = g.get_mut(&self.key) {
                l.pid = pid;
                l.flushed = Instant::now();
            }
            flush_live(&g);
        }
        ulog::write(self.module, &format!("spawned pid={pid}"));
    }

    /// 收到一个结构化事件。`kind` 用 `protocol.rs` 那套；`tool` 只在工具事件上有值。
    pub fn on_event(&mut self, kind: &str, tool: Option<&str>) {
        self.events += 1;
        let next = match kind {
            // 工具跑起来了 —— 接下来这段静默是**工具在跑**，不是模型在等
            "tool_start" | "tool_input" => {
                if kind == "tool_start" {
                    self.tools += 1;
                }
                format!("工具执行:{}", tool.unwrap_or("?"))
            }
            // 工具回来了 —— 接下来这段是**等模型下一步**（pc-*** 最后那 17 分钟就在这儿）
            "tool_end" => "等模型回话".into(),
            // 消息正在流 —— 这段静默是**流中断档**（pc-*** 那 21 分钟就在这儿）
            "text" | "text_done" => "消息流中".into(),
            _ => self.phase.clone(),
        };
        self.enter(&next);
    }

    /// 直接切阶段 —— 给**没有 stream-json 事件流**的大脑用（`chat.rs` 是 Rust 自己发的 HTTP
    /// + 工具循环，没有 `kind` 可映射）。阶段名要和 [`Self::on_event`] 那套对齐，
    /// 否则同一份 chat.log 里两个大脑各说各话，排障时没法横向比。
    pub fn enter(&mut self, phase: &str) {
        self.enter_inner(phase);
    }

    /// 收尾：把最后一段静默也算进去，写一行汇总。
    ///
    /// **status 由调用方给**（它才知道 stalled / interrupted / ok 的优先级），这里不猜。
    pub fn finish(&mut self, status: &str, code: Option<i32>) {
        self.enter_inner(&self.phase.clone()); // 结算尾段
        // 先摘牌再写日志：这一轮已经不在跑了，`chat_inspect` 一秒都不该再把它算成「正在跑」。
        if let Ok(mut g) = live().lock() {
            g.remove(&self.key);
            flush_live(&g);
        }
        let (worst, at) = (self.worst.0.as_secs(), self.worst.1.clone());
        ulog::write(
            self.module,
            &format!(
                "turn end status={status} code={} total={}s events={} tools={} 最长静默={worst}s@{at}",
                code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
                self.t0.elapsed().as_secs(),
                self.events,
                self.tools,
            ),
        );
    }

    /// 结算「上一段静默」并切到下一个阶段。
    fn enter_inner(&mut self, next: &str) {
        let gap = self.last.elapsed();
        if gap > self.worst.0 {
            self.worst = (gap, self.phase.clone());
        }
        // 单段静默超过一半死线就当场记一行 —— 别等收尾。客户往往在卡住的当口就来问了，
        // 那会儿 turn 还没结束，汇总行一个字都还没写出来。
        if gap.as_secs() * 2 >= STALL_SECS {
            ulog::write(
                self.module,
                &format!("静默 {}s（阶段：{}）—— 死线 {STALL_SECS}s", gap.as_secs(), self.phase),
            );
        }
        self.last = Instant::now();
        self.phase = next.to_string();
        if let Ok(mut g) = live().lock() {
            let mut dirty = false;
            if let Some(l) = g.get_mut(&self.key) {
                dirty = l.phase != self.phase || l.flushed.elapsed() >= Duration::from_secs(5);
                l.last = self.last;
                l.phase = self.phase.clone();
                if dirty {
                    l.flushed = Instant::now();
                }
            }
            if dirty {
                flush_live(&g);
            }
        }
    }
}

#[cfg(test)]
mod turnlog_tests {
    use super::*;

    /// 账本的价值全在「把静默算到对的阶段头上」。算错阶段比不记更坏 ——
    /// 它会把「等模型」的问题指成「工具卡住」，下一个人照着查一整天。
    #[test]
    fn worst_gap_is_attributed_to_the_phase_it_happened_in() {
        let mut t = TurnLog::start("chat-test", "claude", "t-phase", None, false);
        t.spawned(0);
        t.on_event("tool_start", Some("Bash")); // → 进入「工具执行」
        std::thread::sleep(Duration::from_millis(120)); // 这段静默属于工具执行
        t.on_event("tool_end", None); // 结算上一段
        assert!(t.worst.1.starts_with("工具执行"), "静默被记到了 {}", t.worst.1);
        assert!(t.worst.0 >= Duration::from_millis(100));

        // 再来一段更长的、发生在「等模型回话」阶段的静默，它应当取代上一条
        std::thread::sleep(Duration::from_millis(260));
        t.on_event("text", None);
        assert_eq!(t.worst.1, "等模型回话", "更长的那段没顶掉旧的");
    }

    /// 解析必须真的解析出来。**用 [`TurnLog::finish`] 亲手写的那种行去测**，不是我另编一条 ——
    /// 自己捏的 fixture 验的是假设不是现实（identity 那次的教训）。
    #[test]
    fn turn_end_line_parses_back_into_numbers() {
        let line = "[2026-08-03 09:12:52] turn end status=timeout code=- total=1279s events=4 tools=2 最长静默=1279s@等模型回话";
        let v = parse_turn_end(line);
        assert_eq!(v["status"], "timeout");
        assert_eq!(v["total_secs"], 1279);
        assert_eq!(v["worst_idle_secs"], 1279, "最长静默解析成 0 = 报告永远说「没卡过」");
        assert_eq!(v["worst_phase"], "等模型回话");
        assert_eq!(v["at"], "2026-08-03 09:12:52");
    }

    /// 收尾必须结算**尾段**：pc-*** 最后那 17 分钟就发生在最后一个事件之后，
    /// 只结算「事件之间」的间隔的话，那 17 分钟一秒都不会出现在日志里。
    #[test]
    fn finish_settles_the_trailing_silence() {
        let mut t = TurnLog::start("chat-test", "claude", "t-tail", None, false);
        t.on_event("tool_end", None); // → 「等模型回话」
        std::thread::sleep(Duration::from_millis(150));
        t.finish("timeout", None);
        assert_eq!(t.worst.1, "等模型回话");
        assert!(t.worst.0 >= Duration::from_millis(120), "尾段静默没被结算");
    }
}

#[cfg(test)]
mod watchdog_tests {
    use super::*;

    /// 起一个**真的会挂住**的子进程，用 1 秒死线的看门狗盯它，断言它真被收走了。
    ///
    /// 这条测试守的是 pc-*** 那个 bug 本身：以前这里什么都没有，客户就那么等了 25 分钟。
    /// 断言的是**进程真的没了**，不是「标志位翻了」—— 标志位翻了但进程还在，才是最坏的情况
    /// （界面说停了、机器上还在跑）。
    #[cfg(windows)]
    #[test]
    fn watchdog_kills_a_silent_process() {
        use std::os::windows::process::CommandExt;
        // ping -n 30 = 挂住约 30 秒且不往我们的管道写东西，正好模拟「卡住的子进程」
        let mut child = std::process::Command::new(crate::installer::system_tool("ping"))
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("起不来 ping，这台机器有问题");
        let pid = child.id();

        let wd = Watchdog::spawn(pid, Duration::from_secs(1));
        assert!(!wd.stalled(), "刚起来就判卡死了");

        // 等看门狗动手（1s 死线 + 200ms 轮询 + taskkill 自身耗时，给到 8s 足够宽）
        let killed = (0..80).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            matches!(child.try_wait(), Ok(Some(_)))
        });
        assert!(killed, "看门狗没能把卡住的进程收掉 —— 死线形同虚设");
        assert!(wd.stalled(), "进程被杀了却没标成卡死，收尾会误报成「用户中断」");
        let _ = child.wait();
    }

    /// 一直有动静就不许动手 —— 误杀一个正在正常干活的长任务，比不加死线更糟。
    #[cfg(windows)]
    #[test]
    fn watchdog_spares_a_busy_process() {
        use std::os::windows::process::CommandExt;
        let mut child = std::process::Command::new(crate::installer::system_tool("ping"))
            .args(["-n", "10", "127.0.0.1"])
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("起不来 ping");
        let wd = Watchdog::spawn(child.id(), Duration::from_secs(1));
        // 持续打卡 3 秒（模拟一路在吐事件的长任务），期间死线绝不该触发
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(200));
            wd.beat();
        }
        assert!(!wd.stalled(), "还在吐事件却被判了卡死");
        assert!(matches!(child.try_wait(), Ok(None)), "正在干活的进程被误杀了");
        wd.finish();
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// 把 chat.log 里一行 `turn end …` 解析成结构。
///
/// **抽成纯函数是为了能被测**：这行字是我们自己写的，格式一改这里就静默错 ——
/// 而它错了的表现是「最长静默永远 0 秒」，看起来完全正常，没人会怀疑。
fn parse_turn_end(l: &str) -> serde_json::Value {
    let f = |k: &str| -> Option<String> {
        l.split(k).nth(1).map(|r| r.split_whitespace().next().unwrap_or("").to_string())
    };
    let worst = f("最长静默=").unwrap_or_default();
    let (secs, phase) = worst.split_once('@').unwrap_or((worst.as_str(), ""));
    serde_json::json!({
        "at": l.split(']').next().map(|s| s.trim_start_matches('[')).unwrap_or(""),
        "status": f("status=").unwrap_or_default(),
        "total_secs": f("total=").unwrap_or_default().trim_end_matches('s').parse::<u64>().unwrap_or(0),
        "worst_idle_secs": secs.trim_end_matches('s').parse::<u64>().unwrap_or(0),
        "worst_phase": phase,
    })
}

/// ★ **「对话现在卡住了吗、卡在哪一步」** —— `runtime.chat.inspect` 的实现。
///
/// pc-*** 那次，这个问题我是拿 PowerShell 一条条量出来的：`Get-CimInstance` 数子进程、
/// 采样两次 CPU 看增量、把 transcript 拉回来算时间戳间隔。**全都是机器该自己回答的事。**
/// 影核协议的意义就在这儿：同一个问题，GUI、CLI、MCP 里的 AI 问的是同一条实现，
/// 不是让每个调用方各自去猜。
///
/// 三段回答，对应排障时真正要分清的三件事：
/// - `running` —— **现在**有没有在跑的一轮，各自静默了多久、停在哪个阶段
/// - `stalled_now` —— 其中静默已经过半死线的（这才是「现在有麻烦」，不必人去比大小）
/// - `recent` —— 最近几轮**怎么收的场**（timeout / interrupted / ok），从 chat.log 读
///
/// `ready` 回答的是**这台机器上这条路能不能被观测**，不是「装没装」：0.9.88 之前的版本
/// 一个字节都不写，日志为空时必须直说「查不到」，而不是返回一份空数组让人以为一切正常。
pub fn chat_inspect() -> serde_json::Value {
    let hint = STALL_SECS / 2;
    // **一律读盘，不读进程内那张表** —— 远程排障跑的是另一个进程（`U-King.exe action run …`），
    // 只有落盘那份是两个进程都看得见的同一个事实。GUI 自己调时读的也是它刚写下的那份。
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let raw = std::fs::read_to_string(ulog::log_dir().join(LIVE_FILE)).unwrap_or_default();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let owner_pid = doc.get("owner_pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let written_at = doc.get("written_at").and_then(|v| v.as_u64()).unwrap_or(0);
    let since_write = now_unix.saturating_sub(written_at);
    // 写这份文件的那个 U-King 还在不在 —— **去问系统，别拿别的事实推**（理由见 `chat::pid_alive`）。
    // 判成尸体的那些照样出现在 `running` 里（带 `owner_alive:false`），只是不进 `stalled_now`：
    // 「U-King 自己没了」和「对话卡住了」是两种病，混在一起会把人往错的方向引。
    let owner_alive: Option<bool> =
        Some(owner_pid == std::process::id() || chat::pid_alive(owner_pid));
    let mut running: Vec<serde_json::Value> = doc
        .get("turns")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut t| {
            let idle = t["idle_at_write_secs"].as_u64().unwrap_or(0) + since_write;
            let elapsed = t["elapsed_secs"].as_u64().unwrap_or(0) + since_write;
            if let Some(o) = t.as_object_mut() {
                o.remove("idle_at_write_secs");
                o.insert("idle_secs".into(), idle.into());
                o.insert("elapsed_secs".into(), elapsed.into());
                o.insert("owner_alive".into(), match owner_alive {
                    Some(v) => serde_json::Value::Bool(v),
                    None => serde_json::Value::Null,
                });
            }
            t
        })
        .collect();
    running.sort_by_key(|v| std::cmp::Reverse(v["idle_secs"].as_u64().unwrap_or(0)));
    // 「现在有麻烦的」= 静默过半死线 **且** 写它的那个 U-King 还活着。
    // owner 已经不在的那些是**上一轮没收尾的残留**，是另一种病（U-King 自己没了），
    // 混进 stalled_now 会把人往「对话卡住」上引 —— 它们照样出现在 running 里，带 owner_alive:false。
    let stalled_now: Vec<serde_json::Value> = running
        .iter()
        .filter(|v| v["idle_secs"].as_u64().unwrap_or(0) >= hint && v["owner_alive"] != serde_json::Value::Bool(false))
        .cloned()
        .collect();

    // 最近几轮的收尾。只解析 `turn end` 行 —— 它一行里就有定性 + 总时长 + 最长静默 + 阶段。
    let tail = ulog::tail("chat", 16 * 1024).unwrap_or_default();
    let mut recent: Vec<serde_json::Value> = tail
        .lines()
        .filter(|l| l.contains("turn end"))
        .rev()
        .take(10)
        .map(parse_turn_end)
        .collect();
    recent.reverse(); // 时间正序，读起来跟日志一致

    let observable = !tail.trim().is_empty();
    serde_json::json!({
        "ready": observable,
        "blockers": if observable { vec![] } else { vec![
            "这台机器上还没有对话日志：要么是 0.9.88 之前的版本（那时这条路一个字节都不写），要么是从没用过对话面板。查不到不等于没问题。".to_string()
        ]},
        "stall_secs": STALL_SECS,
        "hint_secs": hint,
        "owner_pid": owner_pid,
        "owner_alive": match owner_alive { Some(v) => serde_json::Value::Bool(v), None => serde_json::Value::Null },
        "running": running,
        "stalled_now": stalled_now,
        "recent": recent,
        "log_path": ulog::path("chat").to_string_lossy(),
    })
}

/// 给前端的 JSON（`agent_launch_probe` 命令）。形状和无头自检打印的是同一份数据。
pub fn probe_all() -> serde_json::Value {
    serde_json::Value::Array(
        PROBE_TARGETS
            .iter()
            .map(|p| {
                let r = launcher::probe(p);
                serde_json::json!({
                    "program": r.program,
                    "resolved": r.resolved,
                    "via": r.via,
                    "found": r.found,
                    "multiline_ok": r.multiline_ok,
                    "error": r.error,
                })
            })
            .collect::<Vec<_>>(),
    )
}
