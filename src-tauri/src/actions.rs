//! 影核协议 / ActionParity 的最小动作核心 —— **只有协议，没有业务**。
//!
//! GUI、无头 CLI 与未来的 MCP/远端影子都只能经这里调用稳定 Action ID；
//! 各个界面不再各自复制一份「检查命令冲突」逻辑。
//!
//! 依赖方向是单向的：业务模块（installer / hardware / ollama / codex / miniapp）**不认识本文件**，
//! 由组合根 `lib.rs::action_table()` 把它们登记进来。所以删掉任何一个功能模块，
//! 仍然只需要动 `lib.rs` + 前端两个文件 —— 不会因为「登记在动作核心里」多出第三处要改。

use serde::Serialize;
use serde_json::{json, Value};

// ─────────────────────── 错误契约（谁的错 / 该不该重试） ───────────────────────

/// 这个错该赖谁。**远程维护/AI 自动处置的分流器** —— 决定「重试」「让客户充值」还是「开 bug」。
///
/// 为什么要有它：2026-07-28 T-King 出不了片，根因表面是一行 `retriable` 写反了，
/// 实质是**协议里压根没有「这个错该不该重试」这个字段**，于是每个业务模块自己拿正则猜，
/// 猜错了没有任何机制会发现。`report.rs` 那条「别把余额不足/断网/服务器忙当软件 bug 上报」
/// 也是在业务层手工补同一件事。提到协议层就只修一次，而且能被 conformance 断言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Blame {
    /// 客户这边的状态：没充值、没装、没授权。**不是 bug，别上报**，要引导客户。
    User,
    /// 本机到网络那一段：断网、DNS、超时、代理。重试或换网络。
    Network,
    /// 上游服务：渠道抖动、限速、5xx、模型本次失败。**重试多半就好**。
    Upstream,
    /// 我们自己的问题：调用方传错、动作不存在、返回形状不对、没归类的未知错。**该上报**。
    Bug,
}

/// 结构化动作错误。`code` 给机器分支，`blame`/`retriable` 给自动处置，`hint` 给人看。
#[derive(Debug, Clone, Serialize)]
pub struct ActionError {
    pub code: String,
    pub blame: Blame,
    pub retriable: bool,
    pub message: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub hint: String,
}

/// 分类词表。**顺序即优先级，第一条命中就定案** —— 排在前面的是我们自己发的协议错（文案确定），
/// 后面才是上游/系统传上来的自由文本。
///
/// 加词条的规矩：只加**见过的真实报错原文**，别凭想象加。每条最好在注释里留出处，
/// 否则半年后没人敢删。
const ERR_RULES: &[(&str, &str, Blame, bool)] = &[
    // —— 协议层自己发的（前缀确定，最先匹配）——
    ("unknown_action:", "unknown_action", Blame::Bug, false),
    ("confirmation_required:", "confirmation_required", Blame::Bug, false),
    // 状态被别人改过 —— 重新读一次再来是**对的**处置，所以可重试。
    ("conflict:", "conflict", Blame::Bug, true),
    ("invalid_input:", "invalid_input", Blame::Bug, false),
    // —— 客户侧状态：不是 bug，别上报 ——
    ("余额不足", "insufficient_balance", Blame::User, false),
    ("insufficient", "insufficient_balance", Blame::User, false),
    ("quota", "insufficient_balance", Blame::User, false),
    ("欠费", "insufficient_balance", Blame::User, false),
    ("unauthorized", "unauthorized", Blame::User, false),
    ("invalid api key", "unauthorized", Blame::User, false),
    ("鉴权", "unauthorized", Blame::User, false),
    ("未授权", "unauthorized", Blame::User, false),
    ("has no access to model", "model_not_allowed", Blame::User, false),
    ("无可用渠道", "model_not_allowed", Blame::User, false),
    ("还没安装", "not_installed", Blame::User, false),
    ("未安装", "not_installed", Blame::User, false),
    // 🔴 `browser.*` 和 `doc.read` 发的是**带前缀**的 `not_installed: …`，而上面两条中文词
    // 一个都匹配不上 —— 于是「没装 agent-browser」这种纯环境问题一路落到
    // `code=unknown` + `blame=bug`，还附赠一句写给我们自己看的「该往 ERR_RULES 补一条」。
    // 这就是那句 hint 点名要补的那一条。
    ("not_installed:", "not_installed", Blame::User, false),
    // 读文档链（read-doc.py）两个转换器（markitdown / pandoc）全不在时的原话。
    // 🔴 这是**缺可选依赖**不是程序坏了：归 `bug` 会让 CLI / MCP / 远端影子去上报，
    // 而正确处置是引导客户装转换器。pc-*** 实测：CSV 一读，落到 unknown+bug 还带乱码。
    // 🔴 词条刻意收窄到 `no module named 'markitdown'` 而不是裸 `no module named`：
    // 后者会把**我们自己漏打包模块**的 bug（skillpack include 清单漏文件有前科）
    // 归咎客户并静默关掉自动上报 —— ERR_RULES 的规矩是只加见过的真实报错原文，
    // 见过的原文就是这一句。`没有可用的转换器` 是 read-doc.py RuntimeError 的开头，特异、够窄。
    ("没有可用的转换器", "missing_dependency", Blame::User, false),
    ("no module named 'markitdown'", "missing_dependency", Blame::User, false),
    // —— 按设计就不允许的操作：**不是 bug，也不是环境问题**，重试永远是同一个答案 ——
    // 典型：删「官方直连（还原）」那个退路出口、改内置驱动的定义、restore 一个自定义 provider。
    // 不归类的话会落到 `blame=bug` + `code=unknown`，CLI / MCP / 远端影子看到会以为程序坏了
    // 去上报 —— 而它其实正是核心在按规矩挡人。（这条就是 conformance 的 hint 点名要补的。）
    ("不能移除", "refused", Blame::User, false),
    ("不能删除", "refused", Blame::User, false),
    ("不能修改", "refused", Blame::User, false),
    ("不是内置驱动", "refused", Blame::User, false),
    // 工作台：AI 现搭的定义写坏了，或者选的落点核心不许装（盘根 / 家目录 / 别人的非空目录）。
    // 🔴 这两条**必须归 `refused` 不能落到 `blame=bug`**：写坏 manifest 是调用方的事，
    // 而 `bug` 会让 CLI / MCP / 远端影子以为程序坏了去上报 —— 那是把「核心正在按规矩挡人」
    // 报成「U-King 挂了」。错误正文本身已经逐条说清哪儿不合格，照着改再来就行。
    ("工作台定义不合格", "refused", Blame::User, false),
    ("不能当工作台", "refused", Blame::User, false),
    // —— 网络 ——
    ("connection", "network", Blame::Network, true),
    ("连接被关闭", "network", Blame::Network, true),
    ("断网", "network", Blame::Network, true),
    ("dns", "network", Blame::Network, true),
    ("enotfound", "network", Blame::Network, true),
    ("超时", "timeout", Blame::Network, true),
    ("timeout", "timeout", Blame::Network, true),
    ("timed out", "timeout", Blame::Network, true),
    // —— 上游抖动：重试多半就好 ——
    // 「已自动退回本次扣费」是虾盘云→火山 Ark 那条链路的原话（pc-*** 实测：同模型同提示词
    // 16:24 失败、16:27 成功）。这条以前被判成永久错误，是 T-King 全灭的直接原因。
    ("已自动退回", "upstream_transient", Blame::Upstream, true),
    ("任务创建失败", "upstream_transient", Blame::Upstream, true),
    ("请求太频繁", "rate_limited", Blame::Upstream, true),
    ("rate limit", "rate_limited", Blame::Upstream, true),
    ("too many requests", "rate_limited", Blame::Upstream, true),
    ("429", "rate_limited", Blame::Upstream, true),
    ("上游", "upstream_transient", Blame::Upstream, true),
    ("upstream", "upstream_transient", Blame::Upstream, true),
    ("bad gateway", "upstream_transient", Blame::Upstream, true),
    ("service unavailable", "upstream_transient", Blame::Upstream, true),
    ("overload", "upstream_transient", Blame::Upstream, true),
    ("server busy", "upstream_transient", Blame::Upstream, true),
    ("系统繁忙", "upstream_transient", Blame::Upstream, true),
    ("502", "upstream_transient", Blame::Upstream, true),
    ("503", "upstream_transient", Blame::Upstream, true),
    ("504", "upstream_transient", Blame::Upstream, true),
    ("task failed", "upstream_transient", Blame::Upstream, true),
];

impl ActionError {
    /// 把一条自由文本错误归类。**没归上类就是 `unknown` + `blame=bug`** ——
    /// 故意让未归类的错刺眼：它要么真是我们的 bug，要么是词表该补一条。
    /// 两种情况都该被看见，比悄悄算成「客户的问题」强。
    ///
    /// ⚠️ `unknown` 一律 `retriable=false`：协议层不知道这个动作幂不幂等，
    /// 盲目重试一个写动作可能双写。要精确的重试语义，就把真实报错原文加进 [`ERR_RULES`]。
    pub fn classify(message: &str) -> Self {
        let low = message.to_lowercase();
        for (pat, code, blame, retriable) in ERR_RULES {
            if low.contains(&pat.to_lowercase()) {
                return Self {
                    code: (*code).into(),
                    blame: *blame,
                    retriable: *retriable,
                    message: message.into(),
                    hint: hint_for(code).into(),
                };
            }
        }
        Self {
            code: "unknown".into(),
            blame: Blame::Bug,
            retriable: false,
            message: message.into(),
            hint: "没归上类 —— 要么真是 bug，要么该往 actions::ERR_RULES 补一条".into(),
        }
    }

    /// 该不该当成软件 bug 上报。`blame=bug` 才是；客户没钱 / 断网 / 上游抖不是。
    pub fn worth_reporting(&self) -> bool {
        self.blame == Blame::Bug
    }
}

// ─────────────────────── 记账钩子（由组合根注入） ───────────────────────

/// 动作调用流水的落盘钩子。**故意用注入而不是直接 `use crate::ulog`** ——
/// 本文件至今零 `crate::` 依赖，是它将来能被整体抽出去复用的前提；
/// 为了记个日志把业务侧的模块 import 进来，等于把协议核心焊死在这个产品上。
/// 组合根（`lib.rs`）开机时 `set_audit(|l| ulog::write("actions", l))` 一行接上即可。
static AUDIT: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();
/// 调用来源标签（`gui` / `cli` / `mcp`），由各入口自报。没设就是 `gui`（窗口是默认入口）。
static SOURCE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// 结构化行为记录（行为时间轴）。**同 `AUDIT` 一样是注入的** —— 协议核心不认识
/// `journal` 模块，参数全是原始类型，不引任何业务侧的类型。
/// 组合根一行接上：`set_record(journal::record_action)`。
///
/// 签名 = `(来源, 动作 id, 成没成, 耗时 ms, 错误码, 入参字段名)`。
/// **入参只给字段名不给值** —— 和 `audit` 同一条口径，理由见下面 `run_with_progress` 的注释。
type RecordFn = fn(&str, &str, bool, u128, Option<&str>, &[String]);
static RECORD: std::sync::OnceLock<RecordFn> = std::sync::OnceLock::new();

pub fn set_audit(f: fn(&str)) {
    let _ = AUDIT.set(f);
}
pub fn set_record(f: RecordFn) {
    let _ = RECORD.set(f);
}
pub fn set_source(s: &'static str) {
    let _ = SOURCE.set(s);
}
fn source() -> &'static str {
    SOURCE.get().copied().unwrap_or("gui")
}

thread_local! {
    /// 对话大脑（`agent/chat.rs` 的 uking_action 工具）调动作时置 true。
    /// 跳过下面的 audit / record —— AI 的动作由 chat.rs 的 emit tap 记（actor=uking），
    /// 这里再记会把 AI 干的事写成「人干的」，交班报告就失真了。
    static AI_CONTEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 在「对话大脑调动作」的上下文里跑 `f`：该动作不落 audit / record，归属由 chat.rs 的 tap 记。
pub fn with_ai_context<T>(f: impl FnOnce() -> T) -> T {
    let prev = AI_CONTEXT.with(|c| c.get());
    AI_CONTEXT.with(|c| c.set(true));
    let r = f();
    AI_CONTEXT.with(|c| c.set(prev));
    r
}

fn in_ai_context() -> bool {
    AI_CONTEXT.with(|c| c.get())
}

fn audit(line: &str) {
    if let Some(f) = AUDIT.get() {
        f(line);
    }
}

/// 给人看的下一步。空字符串表示没有比 message 更有用的话可说。
fn hint_for(code: &str) -> &'static str {
    match code {
        "insufficient_balance" => "让客户充值后重试，别当 bug 上报",
        "unauthorized" => "Key 无效或没配 —— 去「AI 设置」重新应用一次驱动",
        "model_not_allowed" => "这个 token 没开这个模型的白名单，服务端加一下",
        "not_installed" => "先装上对应工具再调这个动作",
        "refused" => "核心按规矩挡下了，不是故障 —— 换个做法，重试没用",
        "network" | "timeout" => "本机到网络这一段的问题，换网络或稍后重试",
        "upstream_transient" | "rate_limited" => "上游抖动，重试多半就好",
        "conflict" => "状态被别的终端改过了 —— 重新读一次再决定，别覆盖",
        _ => "",
    }
}

pub const COMMAND_GUARD_INSPECT: &str = "runtime.command_guard.inspect";
pub const NETWORK_INSPECT: &str = "runtime.network.inspect";
pub const AI_PROCESS_INSPECT: &str = "runtime.ai_process.inspect";
/// U-King **自己**崩没崩。注意跟 `AI_PROCESS_INSPECT` 分工：那个查的是「客户装的 AI CLI
/// 是崩了还是被杀」（读 WER / 转储 / 杀软），这个查的是我们自己的进程 ——
/// 靠自建的会话标记 + 心跳，**不依赖 Windows 事件日志**（实测客户机上那儿常年是空的）。
pub const CRASH_INSPECT: &str = "runtime.crash.inspect";
/// 这个 U-King 进程是「主实例」还是「并行调试实例」（见 `instance.rs`）。
///
/// **它回答的是一个否则无解的问题**：并行跑两个 U-King 时，调试实例的定时任务 / 技能包同步 /
/// Codex 代理自愈全被刻意关掉了 —— 而降权是**静默**的，从界面上跟「调度器坏了」长得一模一样。
/// 没有这条动作，排障的人会去查调度器、查配置、查上游，而真相只是「你开了两个」。
pub const INSTANCE_INSPECT: &str = "runtime.instance.inspect";
pub const STACK_INSPECT: &str = "runtime.stack.inspect";
pub const HARDWARE_INSPECT: &str = "runtime.hardware.inspect";
pub const CODEX_INSPECT: &str = "runtime.codex.inspect";
pub const DRIVER_INSPECT: &str = "runtime.driver.inspect";
pub const FOOTPRINT_INSPECT: &str = "runtime.footprint.inspect";
pub const TOOLBOX_INSPECT: &str = "runtime.toolbox.inspect";
pub const RTK_INSPECT: &str = "runtime.rtk.inspect";
pub const RTK_DEMO: &str = "runtime.rtk.demo";
pub const HERMES_BROWSER_INSPECT: &str = "runtime.hermes_browser.inspect";
pub const CLAWX_INSPECT: &str = "runtime.clawx.inspect";
pub const GEO_INSPECT: &str = "runtime.geo.inspect";
pub const UU_REMOTE_INSPECT: &str = "runtime.uu_remote.inspect";
pub const PODAPP_INSPECT: &str = "runtime.podapp.inspect";
pub const AUTOMATION_INSPECT: &str = "runtime.automation.inspect";
pub const OPTIMIZER_INSPECT: &str = "runtime.optimizer.inspect";
/// 本机**各家 AI** 的任务（不只是我们自己工作台里的会话）：Claude Code / Codex CLI /
/// Hermes 各自的会话记录 + AI 用 `uking-board` 技能包自己登记的任务。
/// 跟 `USAGE_LOCAL_INSPECT` 读的是同一批会话日志，但回答的是两个问题：
/// 那个答**花了多少钱**，这个答**在干哪些活**。
/// 任务本象（2Origin 长期存储层）：这个任务**干到哪了、验过什么、下一步是什么**。
/// 跟 `AI_TASKS_INSPECT` 分工明确 —— 那个读各家 AI 的**会话记录**（影子，答「谁跑过什么」），
/// 这个读**对象状态**（本象，答「世界此刻是什么样」）。前者换个 harness 接不上，后者能。
pub const ORIGIN_INSPECT: &str = "runtime.origin.inspect";
pub const AI_TASKS_INSPECT: &str = "runtime.ai_tasks.inspect";
/// 工作台模板：有哪些模板、这个目录现在什么状况、装的话会干什么。
/// **预览就是安装计划本身**（同一份实现），不是另写一套模拟 —— 模拟会跟真的漂开。
pub const WORKBENCH_INSPECT: &str = "runtime.workbench.inspect";
/// 只读盘点客户**现在那个乱文件夹**里有什么 —— 「按他的实际使用情况搭工作台」的事实来源。
/// 跟 `WORKBENCH_INSPECT` 分工明确：那个答「这儿能不能装、装了会建什么」，
/// 这个答「他手上到底有什么」。**只 stat 不读内容**，也不碰他机器上的使用记录。
pub const WORKBENCH_SCAN: &str = "runtime.workbench.scan";
pub const USAGE_LOCAL_INSPECT: &str = "runtime.usage_local.inspect";
pub const USAGE_METER_INSPECT: &str = "runtime.usage_meter.inspect";
pub const DIAGNOSTICS_COLLECT: &str = "runtime.diagnostics.collect";
/// U-King 自己的身份 + 「给 AI 的说明书」状态。**外围 AI 的第一站** ——
/// 别家 AI 装在同一台机器上，靠它知道「这儿站着一个能干活的 U-King」。
/// 带 `ready`/`blockers`：说明书没生成 = 谁都发现不了我们，这功能就是废的。
/// 「对话现在卡住了吗、卡在哪一步」。跟 `AI_PROCESS_INSPECT` 分工明确 ——
/// 那个查**客户装的 AI CLI 是崩了还是被杀**（事后取证），这个查**我们自己这一轮跑得怎么样**
/// （当场状态 + 阶段归属）。pc-*** 那次两个问题都要回答，而只有前者有动作。
pub const CHAT_INSPECT: &str = "runtime.chat.inspect";
// —— 办公文档动作（doc.*）。前缀刻意不是 `runtime.` ——
// `runtime.*` 回答「这台机器怎么样」，`doc.*` 回答「这份文件怎么改」，
// 是两类完全不同的东西，混一个前缀会让 AI 在 56 个设备动作里翻找一个改文档的能力。
pub const DOC_INSPECT: &str = "doc.inspect";
pub const DOC_READ: &str = "doc.read";
pub const DOC_EDIT: &str = "doc.edit";
// —— 办公文档「出」动作（doc.create.*）：把 gen-docx/xlsx/pptx/dxf 那批从零生成脚本
// 升格成动作（与 doc.read/edit 同源：脚本就是实现，doc.rs 里不重写）。
// 只登记**真幂等**的写：同一份 spec/markdown/csv + 同一个 out → 同一份文件
// （四个 gen 脚本都不注入实时时间）。**doc.create.mail 故意不上**：gen-eml 注入实时
// Date 头，同入参重放内容不同，不满足「重放安全靠幂等」——要上必须先给脚本加 date
// 覆盖，或实现幂等键账本。挂个不兑现的幂等字段比不声明更坏（重试会双写）。
pub const DOC_CREATE_WORD: &str = "doc.create.word";
pub const DOC_CREATE_SHEET: &str = "doc.create.sheet";
pub const DOC_CREATE_SLIDE: &str = "doc.create.slide";
pub const DOC_CREATE_CAD: &str = "doc.create.cad";
pub const IDENTITY_INSPECT: &str = "runtime.identity.inspect";
/// 行为时间轴：「**谁**在什么时候干了什么」。跟旁边几个用量类动作分工明确 ——
/// `usage_*` 回答「花了多少钱」，`chat.inspect` 回答「这一轮卡在哪」，
/// 这条回答**顺序和归属**：哪些是人点的、哪些是 AI 自己干的、动过哪些文件。
/// 「夜班」的地基：没有时间轴，就没有「正常长什么样」，也就无从判断什么叫失控。
pub const JOURNAL_INSPECT: &str = "runtime.journal.inspect";
/// 被管理契约（企业版第一层）：这台机器**在被管吗**、归谁管。
/// 默认 `unmanaged`（个人版零影响），enroll 后只是多一份身份记录，不做任何自动动作。
pub const ORG_INSPECT: &str = "runtime.org.inspect";
pub const EXPERT_INSPECT: &str = "runtime.expert.inspect";
pub const HIRE_SEARCH: &str = "runtime.hire.search";
/// 本地大模型：四个引擎各自「能不能用」。**一次给全四个**，因为客户真正的问题是
/// 「我这台该用哪个」，逐个问等于让他自己做对比。
pub const LOCALLLM_INSPECT: &str = "runtime.localllm.inspect";
/// 本地大模型的「商店货架」：能下哪些开源模型 + 本地已经有哪些量化。
/// 货架线上热下发（改货架不发 exe），**但量化清单和体积一律现问模型站** ——
/// 写死在我们这儿的体积隔天就是错的，而错的方向是客户的盘被下爆。
pub const LOCALLLM_CATALOG: &str = "runtime.localllm.catalog";
/// 一键成片可选的受控风格目录。只读；提交时后端仍会白名单校验 id，目录不是授权依据。
pub const CREATOR_REEL_PRESETS_INSPECT: &str = "runtime.creator.reel_presets.inspect";
/// 单段视频生成的唯一提交入口。`request_id` 是按次收费请求的幂等键；GUI、CLI、MCP
/// 重放同一个输入必须取回原任务，不能再建一条。
pub const CREATOR_VIDEO_SUBMIT: &str = "runtime.creator.video.submit";

/// `manifest().state.queries` 用：全部只读查询动作。加动作时别忘了这里 ——
/// 影核清单里少一个，远端影子就看不见它。
pub const READ_ACTIONS: &[&str] = &[
    COMMAND_GUARD_INSPECT, NETWORK_INSPECT, AI_PROCESS_INSPECT, CRASH_INSPECT, INSTANCE_INSPECT, STACK_INSPECT,
    HARDWARE_INSPECT, CODEX_INSPECT, DRIVER_INSPECT,
    FOOTPRINT_INSPECT, TOOLBOX_INSPECT, RTK_INSPECT, RTK_DEMO, HERMES_BROWSER_INSPECT,
    CLAWX_INSPECT, GEO_INSPECT, UU_REMOTE_INSPECT, PODAPP_INSPECT, AUTOMATION_INSPECT,
    OPTIMIZER_INSPECT, ORIGIN_INSPECT, AI_TASKS_INSPECT, USAGE_LOCAL_INSPECT, USAGE_METER_INSPECT, DIAGNOSTICS_COLLECT,
    IDENTITY_INSPECT, CHAT_INSPECT, DOC_INSPECT, DOC_READ, JOURNAL_INSPECT, ORG_INSPECT,
    WORKBENCH_INSPECT, WORKBENCH_SCAN, EXPERT_INSPECT, HIRE_SEARCH, LOCALLLM_INSPECT,
    LOCALLLM_CATALOG, CREATOR_REEL_PRESETS_INSPECT,
];

// —— 写动作（会改这台机器）——
pub const DRIVER_APPLY: &str = "runtime.driver.apply";
pub const CONTEXT_MENU_SET: &str = "runtime.context_menu.set";
pub const RTK_SET_ENABLED: &str = "runtime.rtk.set_enabled";
pub const DRIVER_APPLY_EVERYWHERE: &str = "runtime.driver.apply_everywhere";
pub const PROVIDER_SAVE: &str = "runtime.provider.save";
pub const PROVIDER_DELETE: &str = "runtime.provider.delete";
pub const PROVIDER_RESTORE: &str = "runtime.provider.restore";
/// 「我们写的配置，那个工具真的会照着跑吗」—— 回读工具**自己的**配置文件。
/// 立项理由见 `providers::EffectiveConfig`：在它之前，「切换成功」的唯一凭据是
/// 逐字节回读比对，那只能证明「文件里是我写的内容」。
pub const PROVIDER_EFFECTIVE: &str = "runtime.provider.effective";
pub const RTK_UNINSTALL: &str = "runtime.rtk.uninstall";
pub const DESKTOP_PIN: &str = "runtime.desktop.pin";
pub const SKILLPACK_INSTALL: &str = "runtime.skillpack.install";
/// 图片识别会把用户显式选择的图片发往视觉模型并消耗额度；因此是确认型写动作，
/// 不能伪装成普通 read 后被 CLI/MCP 自动重试。
pub const MEDIA_IMAGE_DESCRIBE: &str = "media.image.describe";
/// 按**单个包名**卸载技能包。幂等：已经没了再调一次照样返回成功、`removed` 为空。
///
/// 在此之前本产品**只有装、没有拆**（客户 2026-08-18：「安装了太多预制 skill，还无法删除」）——
/// 「安全卸载」页那条 `skills-in-tools` 是全删，粒度太粗：辞掉一个专家不该被迫清空所有技能。
pub const SKILLPACK_UNINSTALL: &str = "runtime.skillpack.uninstall";
/// 自带技能包清单 + 每个装没装。只读。
/// 「用户自己定装哪些」的前提是**看得见现在有哪些** —— 没有这条，装/删两个动作在界面上就是盲操作。
pub const SKILLPACK_INSPECT: &str = "runtime.skillpack.inspect";
/// 解聘一个**招进来的**专家（删 `~/.uking/experts/<id>/`）。内置专家辞不掉 —— 它们是代码常量。
/// 幂等：已经没了再调一次返回 `dismissed:false`，不报错。
pub const EXPERT_DISMISS: &str = "runtime.expert.dismiss";
/// 「装完了，UChat 现在到底能不能用」。只读。
///
/// 🔴 装机链路一直只回答**装没装**，不回答**能不能用**（客户 2026-08-18：
/// 「以 claude 跑起来为基准才是对的……要收尾自检」）。这两件事差得远，
/// CLAUDE.md 里的原话就是这个场景：Token 压缩机 `installed:true` 形状全对、
/// conformance 全绿，但裸 `rtk` 不在 PATH 上，客户开了两天一点没省 ——
/// **报告是对的，世界是坏的**。
///
/// 装机失败占全部 bug 的 49%，其中一大半是「装完了但用不了」而不是「装的时候报错」——
/// 后者客户会截图给我们，前者他只会觉得这软件不行。
pub const READINESS_INSPECT: &str = "runtime.readiness.inspect";
/// 已装小程序清单。只读。
///
/// 小程序运行时（`miniapp.rs`）一直活着 —— 2026-08-11「第三刀」删的是**商店页**，
/// 不是能力。结果：开发机上此刻装着 4 个小程序、正往动作表里注册 4 个动作
/// （`app.imagefix.*` / `app.idcard.*` / `app.resize.*`），而 GUI 里**一个入口都没有**，
/// 用户既看不见也删不掉。这跟客户抱怨的「预制 skill 删不掉」是同一个病在另一层。
/// 没有这条，装/删/开三件事在界面上全是盲操作。
pub const MINIAPP_INSPECT: &str = "runtime.miniapp.inspect";
/// 卸载一个小程序。目录先挪进回收站再摘注册表，**默认不动它的用户数据**
/// （`.data/<id>/` 故意放在 app 目录之外，就是为了重装不丢东西）。
/// 幂等：没装的再调一次返回 `removed:false`，不报错 —— `write()` 一律声明 idempotent，
/// 声明了就得真兑现。
pub const MINIAPP_UNINSTALL: &str = "runtime.miniapp.uninstall";
/// 给 DSH 装一个插件（`dsh plugin --profile <p> add <spec>`）。
///
/// 🔴 **这是「插件生态」的正确投法**（2026-08-18 定）：我们内置了 DSH，而 DSH 那边
/// **已经有供给侧**；自己开一个没人上架的市场是空货架。装机清单里本来就在用这条命令
/// 给 DSH 装我们自己的两个插件（缓存前缀 / 持续对话终端），这里只是把同一条路露给用户。
///
/// 只跑 `dsh plugin add`，不碰别的 —— spec 由用户从我们筛过的清单里点，或自己粘。
pub const DSH_PLUGIN_INSTALL: &str = "runtime.dsh.plugin_install";
/// 从本机一个 `.ukapp` 包 / 目录装一个小程序。
///
/// 「能装能删，用户自己定」的**装**那半。装第三方包之前那道闸在 `miniapp.rs`：
/// 清单里的 `host_actions` **只允许只读动作**，声明写动作的包装不上
/// （`--miniapp-test` 有断言守着，且已变异验证 —— 把闸门去掉那条当场变红）。
pub const MINIAPP_INSTALL: &str = "runtime.miniapp.install";
/// 把随 exe 内置的小程序补装回来，并撤掉所有「用户删过」的墓碑。
///
/// 删除的回头路（宪法 10：任何写入都要可回滚）。没有这条，`uninstall` 就是单向门 ——
/// 而单向门会让人**不敢删**，「能删能装、用户自己定」就只剩一半。
pub const MINIAPP_RESTORE: &str = "runtime.miniapp.restore";
pub const UU_REMOTE_INSTALL: &str = "runtime.uu_remote.install";
pub const PODAPP_INSTALL: &str = "runtime.podapp.install";
pub const PODAPP_LAUNCH: &str = "runtime.podapp.launch";
// 身份与说明书。三个都幂等：同样的入参重放，结果一样。
// `IDENTITY_PUBLISH` 是**编译**动作 —— 它把动作表现场渲染成 llms.txt，
// 所以「加了新动作要重新发布说明书」这件事只有一条路径，不会出现手写的第二份。
pub const IDENTITY_SAVE: &str = "runtime.identity.save";
pub const IDENTITY_PUBLISH: &str = "runtime.identity.publish";
pub const IDENTITY_SECRET_SET: &str = "runtime.identity.secret_set";
/// 换掉本机的虾盘云访问凭证：服务端签发新的、余额平移、旧的当场吊销。
///
/// **登记为幂等**是有依据的，不是嘴上说说：轮换在服务端是两阶段的（stage → commit），
/// 重放时 stage 会原样返回那把已经 mint 好的 pending key、commit 认出已完成直接返回成功。
/// 重放不会多花一分钱、也不会多产生一把凭证。
///
/// 为什么要有它：老体系的 key 是客户端按 MachineGuid 算出来的，**换不掉** ——
/// 某个客户的 key 泄露了我们只能让他换电脑。见 `device.rs` 文件头。
pub const DEVICE_KEY_ROTATE: &str = "runtime.device.key_rotate";
/// 填入一把已有的访问密钥（换电脑 / 多副本共用 / 老手用网站生成的那把）。
///
/// **幂等**：填同一把重放，结果一样（写同一个字符串）。落盘前必须验通 ——
/// 填错一个字符就静默保存的话，客户会得到一台「看起来配好了、一发消息就报错」的机器。
pub const DEVICE_KEY_ADOPT: &str = "runtime.device.key_adopt";
/// 只移除本机对设备钱包的引用；服务端钱包、Key 与余额均保留。
pub const DEVICE_WALLET_RESET_LOCAL: &str = "runtime.device.wallet_reset_local";
/// 把「本机有 U-King，能力清单见 ~/.uking/llms.txt」这一行指针挂进各家 AI 的全局记忆文件。
/// **这才是让说明书真正被发现的那一步** —— 只生成文件不挂指针，等于把说明书锁在抽屉里。
/// `linked:false` 时改成撤销（只删我们那一块，用户内容原样留下）。
pub const IDENTITY_LINK: &str = "runtime.identity.link";
// 自动化（定时任务）。**只登记幂等的写**：存/删/开关都能重放，结果一样。
// 「立即运行一次」故意**不进动作表** —— 它每跑一次都在烧 token（非幂等），
// 而我们没有 `idempotency_key` 账本；声明一个不兑现的幂等字段比不声明更坏（重试会双跑）。
/// 保存任务本象。**幂等**（同一份状态重放结果一样），带 `expected_version` 乐观并发 ——
/// 跨 harness 交接本就是两个进程轮流写同一个状态，后写的不许静默覆盖。
pub const ORIGIN_SAVE: &str = "runtime.origin.save";
/// 把工作台模板装到客户指定的文件夹。**幂等**：重跑只补缺的，他改过的文件一个字节不动。
/// 三道闸门在核心里（盘根/家目录、非空外来目录、不覆盖已有文件），**故意没有「强制覆盖」** ——
/// 客户最容易随手选中「桌面」，装错地方撒一堆文件夹比装不上难收拾得多。
pub const WORKBENCH_INSTALL: &str = "runtime.workbench.install";
// 本地大模型（四引擎）。启停都**幂等**：已经在跑就原样返回那个端点，不重复起进程。
/// 起本地推理服务，返回 OpenAI 兼容端点。幂等：已在跑就返回现有端点。
pub const LOCALLLM_START: &str = "runtime.localllm.start";
/// 停掉**我们启动的**那个进程。幂等：没在跑也算成功。
/// 🔴 只按 PID 关且关前核对镜像名 —— 按裸名字关会误杀客户自己起的同名服务。
pub const LOCALLLM_STOP: &str = "runtime.localllm.stop";
/// 装引擎（当前只有 Ollama 能一键装；llama.cpp 给下载指引，vLLM/SGLang 在
/// Windows/macOS 上直接拒绝并说明原因）。幂等：装过了直接返回已安装。
pub const LOCALLLM_INSTALL: &str = "runtime.localllm.install";
/// 添加模型目录 / 导入 GGUF。幂等：同一个目录/同一把模型重放结果一样。
pub const LOCALLLM_MODEL_ADD: &str = "runtime.localllm.model_add";
/// 从商店下一个模型。**幂等**：已经下齐的直接跳过、下了一半的接着下（curl -C -），
/// 重放不会多花一个字节的流量。写动作是因为它往客户的盘上放几十 GB 东西。
pub const LOCALLLM_DOWNLOAD: &str = "runtime.localllm.download";
pub const AUTOMATION_SAVE: &str = "runtime.automation.save";
pub const AUTOMATION_REMOVE: &str = "runtime.automation.remove";
pub const AUTOMATION_SET_ENABLED: &str = "runtime.automation.set_enabled";
// 被管理契约（企业版第一层）。两个都幂等：同样入参重放结果一样；
// 确认由协议层强制（confirmation=required）。
pub const ORG_ENROLL: &str = "runtime.org.enroll";
pub const ORG_DISENROLL: &str = "runtime.org.disenroll";
/// 优化大师的**动手那一半**。此前只有 `OPTIMIZER_INSPECT`（看分数）是动作，
/// 「改」只活在 Tauri command 里 —— 于是 GUI 能修，CLI / MCP / AI 专家只能干看着，
/// 只好去教用户「你自己去侧栏点一下」。这条把它补齐成一个动作两个调用方。
///
/// 🔴 **只收 fix / optimize / defender 三个前向动作，故意不收 `undo`**：
/// `ukrt undo` 每调一次回退一层 journal，重放会一路往回剥 —— 不满足「只登记幂等的写」。
/// undo 仍留在 GUI（那里有一次点击等于一层的直觉），要上动作得先给 ukrt 一个
/// 「回退到某个 journal id」的定点语义。
pub const OPTIMIZER_APPLY: &str = "runtime.optimizer.apply";
// 带进度的长任务（波次 3）
pub const FOOTPRINT_REMOVE: &str = "runtime.footprint.remove";
pub const BACKUP_CREATE: &str = "runtime.backup.create";
pub const BACKUP_RESTORE: &str = "runtime.backup.restore";
pub const CLAWX_APPLY_MANAGED: &str = "runtime.clawx.apply_managed";
pub const AITOOL_UNINSTALL: &str = "runtime.aitool.uninstall";
/// 把优化大师指出的**缺件真正装上**（便携 Node / Git / PowerShell 7 / CLI 命令守卫）。
/// 幂等由 `installer::ensure_*` 自己兑现（已装就探到并秒回，不重下）。
/// 跟 `OPTIMIZER_APPLY` 分工：那条改这台机器的**设置**，这条补这台机器**缺的东西**。
pub const ENV_INSTALL_TOOLS: &str = "runtime.env.install_tools";
/// 安装并验收内置浏览器面板依赖。必须走 ActionParity 确认门，不能由 GUI 直调旧安装命令绕过。
pub const BROWSER_RUNTIME_INSTALL: &str = "runtime.browser.install";

/// 标记这个动作会边跑边报进度。清单里如实声明，调用方才知道要不要挂监听 ——
/// 一个会跑几分钟却声明「无进度」的动作，等于让 UI 只能干等着转圈。
pub fn with_progress(mut a: Action) -> Action {
    a.spec.progress_events = true;
    a
}

/// 动作实现。
///
/// - 带 `id`：让一整族动作（例如全部小程序动作）共用一个分发函数，不必给每个 id 各写一个函数指针。
/// - 带 `progress`：长任务边跑边报进度。**统一签名而不是分裂成「带进度」「不带进度」两种
///   handler** —— 协议里一旦有两种 handler，迟早会长出第三种。不报进度的动作忽略它即可，
///   `run()` 传的是个空实现。
pub type Handler = fn(&str, Value, &ProgressSink) -> Result<Value, String>;

/// 进度回调。要求 `Send + Sync`：动作跑在工作线程上，业务模块（cleanup / backup）
/// 的回调签名本来就是这个，统一成一份省得两头转接。
pub type ProgressSink = dyn Fn(&str) + Send + Sync;

/// 字段是 `String` 而不是 `&'static str`：动作不再只有编译期写死的几个，
/// 已装小程序的动作也要摊进同一张表（见 `miniapp::action_specs`），静态字符串装不下。
#[derive(Clone, Serialize)]
pub struct ActionSpec {
    pub id: String,
    pub title: String,
    pub description: String,
    pub effect: String,
    pub confirmation: String,
    pub idempotent: bool,
    pub timeout_ms: u64,
    /// 会不会边跑边报进度。清单里如实声明，调用方据此决定要不要挂监听。
    pub progress_events: bool,
    /// 小程序动作带自己的契约；宿主内置动作留空，由 `manifest()` 补默认值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bindings: Option<Value>,
    /// ★ **观测记账**（影核提案 docs/PROPOSAL-OBSERVATION-ACCOUNTING）：
    /// 这个只读动作的结果由哪些**可独立失败**的来源汇总而来。
    ///
    /// 为什么需要它：一个聚合动作返回空，有两种完全不同的意思 ——
    /// 「这台机器上确实没有」和「我没看到」（路径写错 / 没权限 / 对方换了格式）。
    /// 不声明的话两者形状一模一样，`conformance` 全绿，**报告是对的、世界是空的**。
    /// 一个 OS 的 syscall 不会把 EACCES 和空目录都返回空列表，影核也不该。
    ///
    /// 声明了它，`conformance` 就会强制结果里有 `sources` 记账（见 `check_observation`）。
    /// 单来源、来源不可用时必然抛错的简单读**不用声明**，别制造无谓负担。
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub observes: &'static [&'static str],
}

impl Action {
    /// 声明这个动作**汇总了哪些可独立失败的来源**（影核观测记账）。
    /// 建造器写法，挂在动作登记的尾巴上，不打断 `readonly(...)` 那串参数。
    pub fn observing(mut self, sources: &'static [&'static str]) -> Self {
        self.spec.observes = sources;
        self
    }
}

/// 一个已登记的动作 = 契约 + 实现（+ 写动作的状态版本来源）。
pub struct Action {
    pub spec: ActionSpec,
    pub handler: Handler,
    /// 这个动作所写状态的**当前版本**。写动作带上它，调用方就能用
    /// `expected_state_version` 做乐观并发：版本对不上说明状态在你读之后被人改过，
    /// 核心直接返回 `conflict` 拒绝执行。宪法第 16 条：
    /// 「最后写入者获胜」不许当未声明的默认。只读动作留 None。
    pub state_fn: Option<fn() -> String>,
}

/// 配方（recipe）—— **「几个动作按什么顺序组合，能办成一件人说得出口的事」**。
///
/// ## 为什么动作表不够
/// 动作表是 sitemap：56 个原子动作、每个都有签名。但 AI 拿到它跟人拿到一个没有说明书的
/// 工具栏一样 —— 组合空间是阶乘级的，每次现推既慢又不可复现，而且**组合里的顺序约束
/// 只有我们知道**：`clawx.apply_managed` 必须「关进程 → 写配置 → 重启」，直接写配置无效；
/// 查「对话卡住了吗」必须**先** `chat.inspect` 再 `diagnostics.collect`，反过来会拿到一大堆
/// 日志却答不了那一句。这类知识一个字节都不在动作签名里，它是厂商知识。
///
/// 这正是网站 llms.txt 值钱的地方：它不是 sitemap，是「这个站是干什么的、常见任务怎么走」。
///
/// ## 它不是新协议
/// 是影核 manifest 的一段（`recipes`）+ 说明书里的一节。**新协议意味着第二套 ID 空间、
/// 第二套校验、第二套 conformance** —— 同一事实存在几份就会漂移几份（宪法第 8 条）。
/// 配方引用的就是现有 action id，不另起名字。
///
/// ## 它必须被校验
/// 一条 step 写了不存在的动作 id，比没有这条配方更坏 —— AI 会照着它去调一个空。
/// 所以 `conformance()` 会把配方一起过一遍，**引用不存在的动作直接判 fail**
/// （同 `action bindings` 里 `stale` 算失败的道理）。
pub struct Recipe {
    pub id: &'static str,
    pub title: &'static str,
    /// 什么情况下该用它 —— 用**客户会说的话**写，不是用我们的术语。
    pub when: &'static str,
    /// 人话前置条件。给不出就留空数组，别编。
    pub preconditions: &'static [&'static str],
    pub steps: &'static [Step],
    /// 怎么算办成了。**必须是可核对的**（读哪个动作的哪个字段），不能是「应该就好了」。
    pub verify: &'static str,
}

/// 配方里的一步。`note` 回答「**为什么这一步在这个位置**」—— 没有它，配方就退化成
/// 一串动作 id，读的人（和 AI）照样不知道能不能换顺序。
pub struct Step {
    pub action: &'static str,
    pub note: &'static str,
}

fn recipes() -> Vec<Recipe> {
    crate::recipe_table()
}

/// 配方清单（`action recipes --json` / manifest 里的 `recipes` 段共用这一份）。
pub fn recipe_list() -> Value {
    let known: std::collections::HashSet<String> = list().into_iter().map(|a| a.id).collect();
    Value::Array(
        recipes()
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "title": r.title,
                    "when": r.when,
                    "preconditions": r.preconditions,
                    "steps": r.steps.iter().map(|s| json!({
                        "action": s.action,
                        "note": s.note,
                        // 就地标出来，读清单的人不用自己去比对动作表
                        "known": known.contains(s.action),
                    })).collect::<Vec<_>>(),
                    "verify": r.verify,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// 配方体检：每一步引用的动作必须真实存在。返回 `(检查结果, 失败数)`。
fn check_recipes() -> (Vec<Value>, usize) {
    let known: std::collections::HashSet<String> = list().into_iter().map(|a| a.id).collect();
    check_recipes_against(&recipes(), &known)
}

/// 纯函数版，给测试用 —— 「引用了不存在的动作要判 fail」这条本身必须被测到，
/// 否则它哪天被写反（`missing.is_empty()` 少个 `!`）就再也不会有人发现。
fn check_recipes_against(
    rs: &[Recipe],
    known: &std::collections::HashSet<String>,
) -> (Vec<Value>, usize) {
    let mut out = Vec::new();
    let mut fail = 0;
    for r in rs {
        let missing: Vec<&str> = r
            .steps
            .iter()
            .map(|s| s.action)
            .filter(|a| !known.contains(*a))
            .collect();
        if !missing.is_empty() {
            fail += 1;
        }
        out.push(json!({
            "id": r.id,
            "steps": r.steps.len(),
            "status": if missing.is_empty() { "pass" } else { "fail" },
            "missing_actions": missing,
        }));
    }
    (out, fail)
}

/// **可用性约定（readiness）**：描述一项「能力」的只读动作，输出里应带
/// `ready: bool` + `blockers: [String]`，回答的是**能不能用**，不是**装没装**。
///
/// 这条是被真 bug 逼出来的：Token 压缩机 `installed:true / enabled:false` 全都如实，
/// 形状挑不出毛病，`conformance` 全绿 —— 但 hook 改写出的裸 `rtk` 不在 PATH 上，
/// 客户开了两天一点没省。**报告是对的，世界是坏的**，而跑道只断言形状。
///
/// 加了这条约定之后，同一个问题会在三个面同时显形：GUI 的能力卡、`action run` 的
/// JSON、以及 AI 通过 MCP 读到的结果。`conformance` 也会把所有 `ready:false` 的
/// 能力汇总成一段 —— 那一段就是「这台机器上哪些功能其实是废的」。
///
/// 声明一个只读、无入参的宿主内置动作。
///
/// `required` 是**调用方真的会读**的顶层字段。它不是完整 JSON Schema —— 手写完整 schema
/// 必然与 Rust 结构体漂移，写了也不敢信。只钉住关键字段，`conformance()` 就能抓到真正致命的
/// 那类回归：重构悄悄改名/删掉了某个前端在读的字段。
pub fn readonly(
    id: &str,
    title: &str,
    description: &str,
    timeout_ms: u64,
    required: &[&str],
    handler: Handler,
) -> Action {
    Action {
        spec: ActionSpec {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            effect: "read".into(),
            confirmation: "never".into(),
            idempotent: true,
            timeout_ms,
            progress_events: false,
            input_schema: Some(json!({ "type": "object", "additionalProperties": false })),
            output_schema: Some(json!({
                "type": "object",
                "required": required.iter().map(|s| Value::String((*s).into())).collect::<Vec<_>>(),
            })),
            bindings: None,
            observes: &[],
        },
        handler,
        state_fn: None,
    }
}

/// 声明一个**会改机器**的动作。
///
/// 三件事是协议层强制的，不指望每个 handler 自己记得（宪法第 16 条
/// 「确认与权限在权威核心强制——绕开 GUI 按钮不等于绕开权限」）：
/// 1. `confirmation="required"` 的动作，入参里必须显式带 `confirm: true`。
///    GUI 按钮点了才传，CLI 要 `--yes`，AI 想绕过 GUI 直接调也一样被拦。
/// 2. 可选 `expected_state_version`：和 `state_fn` 读出来的当前版本对不上就返回
///    `conflict` 并**拒绝执行**。这是给远端影子/多终端用的乐观并发。
/// 3. 只登记 `idempotent: true` 的写动作 —— **重放安全靠幂等，不靠幂等键账本**。
///    真要加非幂等的写（发消息、扣款那种），必须先实现 idempotency_key 账本；
///    在那之前声明一个不兑现的 idempotency_key 字段比不声明更坏：重试会双写。
#[allow(clippy::too_many_arguments)]
pub fn write(
    id: &str,
    title: &str,
    description: &str,
    timeout_ms: u64,
    confirmation: &str,
    input_properties: Value,
    required_in: &[&str],
    required_out: &[&str],
    handler: Handler,
    state_fn: Option<fn() -> String>,
) -> Action {
    let mut props = input_properties.as_object().cloned().unwrap_or_default();
    let mut required: Vec<Value> = required_in.iter().map(|s| Value::String((*s).into())).collect();
    if confirmation == "required" {
        props.insert(
            "confirm".into(),
            json!({ "type": "boolean", "description": "Must be true. Explicit intent to change this machine." }),
        );
        required.push(Value::String("confirm".into()));
    }
    props.insert(
        "expected_state_version".into(),
        json!({
            "type": "string",
            "description": "Optional. The state_version you read before deciding. Mismatch returns conflict instead of overwriting."
        }),
    );
    Action {
        spec: ActionSpec {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            effect: "write".into(),
            confirmation: confirmation.into(),
            idempotent: true,
            timeout_ms,
            progress_events: false,
            input_schema: Some(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": Value::Object(props),
                "required": required,
            })),
            output_schema: Some(json!({
                "type": "object",
                "required": required_out.iter().map(|s| Value::String((*s).into())).collect::<Vec<_>>(),
            })),
            bindings: None,
            observes: &[],
        },
        handler,
        state_fn,
    }
}

/// 同 `write`，但这个动作**删掉的东西回不来**（用户自建的配置、生成的作品……）。
/// `effect="destructive"` 会让清单里的 `reversible` 变成 false、`risk` 变 high ——
/// 让读清单的人和 AI 一眼看出「这条撤不回来」。确认强制为 required，没得选。
#[allow(clippy::too_many_arguments)]
pub fn destructive(
    id: &str,
    title: &str,
    description: &str,
    timeout_ms: u64,
    input_properties: Value,
    required_in: &[&str],
    required_out: &[&str],
    handler: Handler,
) -> Action {
    let mut a = write(id, title, description, timeout_ms, "required", input_properties, required_in, required_out, handler, None);
    a.spec.effect = "destructive".into();
    a
}

/// 状态版本：把一份状态快照压成一个短字符串。**不是密码学哈希**，只用来判「变没变」，
/// 所以纯 std 的 FNV-1a 足够，不必为此引 crate（体积优先）。
pub fn version_of(snapshot: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in snapshot.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("v1-{h:016x}")
}

/// 同 `readonly`，但收一组**全可选**的入参。喂 `{}` 必须能跑（handler 自己取默认值），
/// 所以它照样进 `conformance` 体检 —— 「有参数」不等于「必须给参数」。
pub fn readonly_opt(
    id: &str,
    title: &str,
    description: &str,
    timeout_ms: u64,
    properties: Value,
    required_out: &[&str],
    handler: Handler,
) -> Action {
    let mut a = readonly(id, title, description, timeout_ms, required_out, handler);
    a.spec.input_schema = Some(json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
    }));
    a
}

/// 只读、但**有必填入参**的动作（`doc.read` 要一个文件路径才有意义）。
///
/// 跟 [`readonly_opt`] 分开是因为必填这件事有两个真实后果，缺一不可：
/// 1. `validate_input` 会真的拦下少传的调用 —— 声明了不执行等于骗自己；
/// 2. `conformance` 会**如实跳过**它（通用体检不替它编造一个文件路径），
///    而不是拿空入参跑一遍再把「你没给文件」记成一条失败。第一版我用 `readonly_opt`
///    在 handler 里手工判必填，跑道当场红了一条 —— 那条红不是 bug，是声明没说实话。
pub fn readonly_req(
    id: &str,
    title: &str,
    description: &str,
    timeout_ms: u64,
    properties: Value,
    required_in: &[&str],
    required_out: &[&str],
    handler: Handler,
) -> Action {
    let mut a = readonly(id, title, description, timeout_ms, required_out, handler);
    a.spec.input_schema = Some(json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": required_in,
    }));
    a
}

fn table() -> Vec<Action> {
    crate::action_table()
}

pub fn list() -> Vec<ActionSpec> {
    table().into_iter().map(|a| a.spec).collect()
}

pub fn describe(id: &str) -> Result<ActionSpec, String> {
    list()
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("unknown_action: {id}"))
}

/// 内部执行策略用 `required`，因为核心按这个值强制 `confirm:true`；ActionParity 0.5
/// 的 wire enum 把同一语义叫 `always`。只在导出边界映射，不能为了迎合 wire 格式去改
/// 核心门禁用词，否则 CLI / GUI / MCP 会一起失去确认保护。
fn wire_confirmation(value: &str) -> &str {
    match value {
        "required" => "always",
        other => other,
    }
}

/// U-King 核心把 `confirm` 放在业务入参里执行门禁；标准 request envelope 把它放在
/// `confirmed`。导出时删掉重复字段，由 Adapter 在进入旧核心前恢复，调用方只确认一次。
fn wire_input_schema(schema: Option<Value>, confirmation: &str) -> Value {
    let mut schema = schema.unwrap_or_else(|| json!({ "type": "object", "additionalProperties": false }));
    if confirmation == "required" {
        if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
            properties.remove("confirm");
        }
        if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
            required.retain(|field| field.as_str() != Some("confirm"));
        }
    }
    schema
}

/// ActionParity 清单的一小段可执行投影。发布清单可直接复用这个结构，避免文档与核心漂移。
pub fn manifest() -> Value {
    let specs = list();
    let has_miniapp = specs.iter().any(|a| a.id.starts_with("app."));
    let actions = specs
        .into_iter()
        .map(|a| {
            // 小程序动作带着自己的 bindings（miniapp / cli / mcp）进来，但在**合并后的宿主清单**里
            // 还得补一条 desktop 绑定：用户是从 U-King 首页那排图标点进小程序的，
            // 所以它在桌面面上确实可达。不补的话 strict parity 会判「desktop 面缺绑定」——
            // 而且那不是误报，是这份清单没把可达路径说清楚。
            let bindings = match a.bindings.clone() {
                Some(Value::Array(mut b)) => {
                    if !b.iter().any(|x| x.get("surface").and_then(|s| s.as_str()) == Some("desktop")) {
                        b.push(json!({
                            "surface": "desktop",
                            "target": format!("uking:miniapp/open#{}", a.id)
                        }));
                    }
                    Value::Array(b)
                }
                _ => json!([
                    { "surface": "desktop", "target": "tauri:command/action_run" },
                    { "surface": "cli", "target": format!("cli:action run {} --json --no-input", a.id) }
                ]),
            };
            json!({
                "id": a.id,
                "title": a.title,
                "description": a.description,
                "input_schema": wire_input_schema(a.input_schema.clone(), &a.confirmation),
                "output_schema": a.output_schema.clone().unwrap_or_else(|| json!({ "type": "object" })),
                // risk / audit_required 从 effect 推，不再一律写死 low/false ——
                // 一个会改用户 ~/.claude 的动作在清单里自称「低风险、不用留痕」，
                // 那清单就是在骗读它的人（和读它的 AI）。
                "effects": {
                    "class": a.effect,
                    "risk": match a.effect.as_str() {
                        "read" => "low",
                        "destructive" => "high",
                        _ => "medium",
                    },
                    "reversible": a.effect != "destructive",
                    "confirmation": wire_confirmation(&a.confirmation),
                    "audit_required": a.effect != "read"
                },
                "execution": {
                    "headless": true,
                    "idempotent": a.idempotent,
                    "cancellable": false,
                    "timeout_ms": a.timeout_ms,
                    "progress_events": a.progress_events
                },
                "bindings": bindings
            })
        })
        .collect::<Vec<_>>();
    // 装了小程序就得把它们的面一并声明，否则 bindings 会指向未声明的 surface，
    // 合并出来的清单过不了上游校验。
    let mut surfaces = vec![
        json!({ "id": "desktop", "kind": "gui", "required_for_parity": true }),
        json!({ "id": "cli", "kind": "cli", "required_for_parity": true }),
    ];
    if has_miniapp {
        // required_for_parity=false 是实话：宿主自己的 runtime.* 动作没有、也不该有小程序界面。
        // 标成 true 会要求每个动作都绑到这个面上，那是把「有这个面」和「人人都在这个面上」混为一谈。
        surfaces.push(json!({
            "id": "miniapp", "kind": "gui", "required_for_parity": false,
            "test_driver": "uking-miniapp-webview",
            "description": "Installed U-King MiniApps, served over the uking:// protocol."
        }));
        surfaces.push(json!({
            "id": "mcp", "kind": "mcp", "required_for_parity": false,
            "exclusion_reason": "Only installed MiniApp Actions are exposed through MCP; host runtime Actions are not yet in MCP parity.",
            "description": "Exposed through `U-King.exe mcp serve`."
        }));
    }
    json!({
        // 🔴 **必须跟我们实际校验用的那份 schema 一致**：影核上游至今是 0.5.0
        // （`node_modules/action-parity/schema` 里 `spec_version` 是 `const: "0.5.0"`）。
        //
        // 这里一度写着 `0.6.0`，注释是「加了 recipes 段，纯增量」—— 那是**试点单方面
        // 分叉了版本号**，直接后果是 `action-parity:verify` 长期红着：
        //   /: must NOT have additional properties
        //   /spec_version: must be equal to constant
        // 而试点红着 = 规范没有活的验证，比少一个字段严重得多。
        //
        // 为什么 recipes 不在这儿了：0.5.0 的 schema 是 `additionalProperties: false`，
        // **且没有给扩展留任何命名空间** —— 实现者有规范之外的东西时，只剩「藏起来 /
        // 自己改版本号 / 不做」三条路，我们上次选了第二条。配方**一点没丢**，
        // 它的真源本来就是 `recipes()`：CLI 走 `action recipes --json`、
        // 说明书走 `identity::render_recipes(recipe_list())`，都不经过 manifest。
        // 等上游接受了 `x-` 扩展命名空间（见影核 docs/PROPOSAL-MANIFEST-EXTENSIONS），
        // 再以 `x-recipes` 名正言顺地放回来。
        "spec_version": "0.5.0",
        "application": { "id": "org.u-king.desktop", "name": "U-King", "version": env!("CARGO_PKG_VERSION") },
        "generated_from": { "generator": "U-King Action Registry", "revision": env!("CARGO_PKG_VERSION") },
        "conformance_targets": ["AP-1", "AP-2"],
        "surfaces": surfaces,
        "actions": actions,
        "state": { "queries": READ_ACTIONS, "events": [] }
    })
}

/// 所有界面共用的动作执行入口。
///
/// 顺序是刻意的：**权限 → 校验 → 并发 → 执行**，全部发生在任何副作用之前。
/// 权限排在解析入参之前，是因为「这台机器能不能被改」比「参数写对没有」更该先答；
/// 也让 `conformance` 能安全地拿空入参去敲一下门禁，确认它真的锁着。
pub fn run(id: &str, input: Value) -> Result<Value, String> {
    run_with_progress(id, input, &|_| {})
}

/// 同 `run`，但错误是**结构化**的（`code`/`blame`/`retriable`/`hint`）。
///
/// 远程维护 / MCP / 自动重试走这条：调用方能据 `blame` 分流 —— `upstream` 重试、
/// `user` 引导客户、`bug` 才开 issue。老的 `run()` 继续返回字符串，前端和既有薄壳零改动。
pub fn run_checked(id: &str, input: Value) -> Result<Value, ActionError> {
    run(id, input).map_err(|e| ActionError::classify(&e))
}

/// 同 `run`，但把长任务的进度逐条交给 `progress`。GUI 在这里 emit 事件，
/// 无头 CLI 把它打到 **stderr**（stdout 只放最终结果，宪法第 14 条）。
pub fn run_with_progress(id: &str, input: Value, progress: &ProgressSink) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let keys: Vec<String> = input
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let out = run_inner(id, input, progress);
    let ms = started.elapsed().as_millis();
    let err = out.as_ref().err().map(|e| ActionError::classify(e));
    // 统一记账：一处生效全部动作。远程维护第二常问的是「客户到底点了什么」——
    // 在这之前完全查不到（actions.rs 一行日志都不写）。
    // **只记入参的字段名，不记值** —— provider.save 之类的入参里有 Key，
    // 日志落在客户本机、客户可能直接转发给别人，记值就是泄漏。
    // 对话大脑调进来的动作跳过这里（AI 的动作由 chat.rs 的 tap 记，见 with_ai_context）。
    if !in_ai_context() {
        audit(&format!(
            "{} {} ms={} keys=[{}]{}",
            source(),
            id,
            ms,
            keys.join(","),
            match &err {
                None => " ok".to_string(),
                Some(c) => format!(" err code={} blame={:?} retriable={}", c.code, c.blame, c.retriable),
            }
        ));
        // 🔴 P2-3（黑盒报告）：`blame=Bug` 的 hint 明说「没归上类 —— 该往 ERR_RULES 补一条」，
        // 即程序在主动要求人来看。但原来只落归类结果，**原始 message 没落盘**，
        // 事后无法从磁盘追查「该补哪条 ERR_RULES」。这里补上原文 ——
        // 必须过 `desensitize` 脱敏（错误文本可能带路径/用户名，日志在客户本机）。
        if let Some(c) = &err {
            if c.worth_reporting() {
                audit(&format!(
                    "  └ raw: {}",
                    crate::feedback::desensitize(&c.message)
                ));
            }
        }
        // 同一件事的**第二个用途**：进行为时间轴（谁在什么时候干了什么）。
        // 不是第二份记录 —— 上面那行是给人读的自由文本，这里是给机器读的逐条事件；
        // 两者**同一处产生、同一份口径**（同样只给字段名不给值），不会漂移。
        if let Some(f) = RECORD.get() {
            f(source(), id, err.is_none(), ms, err.as_ref().map(|c| c.code.as_str()), &keys);
        }
    }
    out
}

fn run_inner(id: &str, input: Value, progress: &ProgressSink) -> Result<Value, String> {
    let action = table()
        .into_iter()
        .find(|a| a.spec.id == id)
        .ok_or_else(|| format!("unknown_action: {id}"))?;

    // ① 确认门禁。核心强制，不是 GUI 的礼貌 —— 从 CLI / MCP / 远端影子进来一样要过。
    if action.spec.confirmation == "required" && input.get("confirm") != Some(&json!(true)) {
        return Err(format!(
            "confirmation_required: `{id}` 会改这台机器，必须显式传 confirm=true（CLI 用 --yes）"
        ));
    }
    // ② 入参校验。
    validate_input(&action.spec, &input)?;
    // ③ 乐观并发。带了 expected_state_version 就必须对得上，否则拒绝执行而不是覆盖。
    if let Some(expected) = input.get("expected_state_version").and_then(|v| v.as_str()) {
        let Some(state_fn) = action.state_fn else {
            return Err(format!("conflict: `{id}` 不提供状态版本，别拿 expected_state_version 调它"));
        };
        let current = state_fn();
        if current != expected {
            return Err(format!(
                "conflict: 状态在你读到之后被改过（expected={expected} current={current}）—— 重新读一次再决定，不覆盖"
            ));
        }
    }
    (action.handler)(id, input, progress)
}

/// 入参校验。**不是完整 JSON Schema 实现**，只做四件真能挡住 bug 的事：
/// 拒绝未声明的字段、检查必填字段在不在、检查字段的基本类型（含数组元素）、
/// **执行写在契约里的 `enum`**（顶层字段和数组元素两处）。
///
/// 为什么非要有这个：`additionalProperties:false` 写在契约里却没人执行，
/// 等于骗自己 —— 一个打错的参数名（`dayz` 之于 `days`）会被静默忽略、
/// 动作照跑并返回**默认值算出来的错答案**。这类错最难查，因为它不报错。
///
/// 🔴 `enum` 是 0.9.99 补上的，补之前它整整是一句**装饰**：
/// `runtime.driver.apply_everywhere` 的 `targets` 声明只认
/// `claude/codex/clawx/hermes`，实测传 `["pi"]` 照样跑通并真的改了机器。
/// 同一份契约里的 `additionalProperties:false` 有人执行、`enum` 没人执行 ——
/// 而调用方（尤其是照着 manifest 生成入参的 AI）没法从契约里看出这个区别。
/// 小程序动作那边的 validator（`miniapp.rs`）一直是执行 enum 的，
/// **宿主自己反而没有**：能力早就有了，只是没接到这条路上。
///
/// 只管 `additionalProperties:false` 的宿主契约。小程序动作有自己的 validator，
/// 没声明 schema 的（契约未知）一律放行 —— 宁可少拦一个，不能拦死一个本来能用的。
///
/// **可选字段上的 `null` 一律当「没给」**（见 `null_on_optional_field_means_absent`）：
/// 这不是通融，是照着调用方的语言收 —— JS 的 `undefined` 过一趟 IPC 就是 `null`，
/// AI 从 JSON Schema 生成入参时把「不覆盖模型」写成 `"model": null` 同样天经地义。
/// handler 一侧本来就是 `.get(k).and_then(as_str)`，收到 null 和收不到完全同一条路。
/// 必填字段给 null 照旧拒 —— 那是真的少了东西，不是「省略了可选项」。
fn validate_input(spec: &ActionSpec, input: &Value) -> Result<(), String> {
    let Some(schema) = &spec.input_schema else { return Ok(()) };
    if schema.get("additionalProperties") != Some(&json!(false)) {
        return Ok(());
    }
    let Some(obj) = input.as_object() else {
        return Err("invalid_input: 入参必须是 JSON 对象".into());
    };
    let props = schema.get("properties").and_then(|p| p.as_object());
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for (k, v) in obj {
        let Some(ps) = props.and_then(|p| p.get(k)) else {
            return Err(format!("invalid_input: 未知字段 `{k}`（该动作不收这个参数）"));
        };
        if v.is_null() && !required.contains(&k.as_str()) {
            continue;
        }
        if let Some(t) = ps.get("type").and_then(|t| t.as_str()) {
            if !type_matches(v, t) {
                return Err(format!("invalid_input: 字段 `{k}` 应为 {t}"));
            }
        }
        if let Some(e) = ps.get("enum").and_then(|e| e.as_array()) {
            if !e.contains(v) {
                return Err(format!("invalid_input: 字段 `{k}` 只认 {}", enum_hint(e)));
            }
        }
        // 🔴 `minimum` / `maximum`：**声明了就得执行**，这是 `enum` 那件事的第三遍。
        //
        // 之前它俩纯属装饰：`runtime.usage_meter.inspect` 的 `days` 写着 `maximum: 365`，
        // 传 99999 照样受理 —— handler 里 `days.clamp(1, 365)` 把它悄悄改成 365，
        // 返回一份「365 天」的报告，而调用方以为自己拿到的是 99999 天的。
        // **静默 clamp 比报错坏**：报错他会改参数，静默他会拿错误的口径去做决定
        // （「这是我一年的花费」其实只是一个月）。九个字段全是这种边界，一并接上。
        if let Some(n) = v.as_f64() {
            if let Some(min) = ps.get("minimum").and_then(Value::as_f64) {
                if n < min {
                    return Err(format!("invalid_input: 字段 `{k}` 不能小于 {min}"));
                }
            }
            if let Some(max) = ps.get("maximum").and_then(Value::as_f64) {
                if n > max {
                    return Err(format!("invalid_input: 字段 `{k}` 不能大于 {max}"));
                }
            }
        }
        // 数组元素：约束写在 `items` 上，只看顶层那个 "array" 等于没看。
        // `targets: ["pi"]` 就是这么混过去的 —— 顶层类型对，元素没人管。
        if let Some(arr) = v.as_array() {
            if let Some(items) = ps.get("items") {
                for (i, x) in arr.iter().enumerate() {
                    if let Some(t) = items.get("type").and_then(|t| t.as_str()) {
                        if !type_matches(x, t) {
                            return Err(format!("invalid_input: 字段 `{k}[{i}]` 应为 {t}"));
                        }
                    }
                    if let Some(e) = items.get("enum").and_then(|e| e.as_array()) {
                        if !e.contains(x) {
                            return Err(format!(
                                "invalid_input: 字段 `{k}[{i}]` 只认 {}",
                                enum_hint(e)
                            ));
                        }
                    }
                }
            }
        }
    }
    for r in required {
        if !obj.contains_key(r) {
            return Err(format!("invalid_input: 缺少必填字段 `{r}`"));
        }
    }
    Ok(())
}

/// 报错里把允许值原样列出来。**不截断**：调用方（人或 AI）拿到这句话就该能直接改对，
/// 让他再去翻一次 `action describe` 等于把一次可自愈的错变成一次往返。
fn enum_hint(e: &[Value]) -> String {
    e.iter()
        .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn type_matches(v: &Value, t: &str) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "null" => v.is_null(),
        _ => true,
    }
}

/// 动作**必须**有入参吗（`required` 非空）？入参全是可选的不算 ——
/// 那种动作喂 `{}` 就能跑，理应进体检；把它和「真的要参数」混为一谈会白白丢掉覆盖率。
///
/// 压根没声明 input_schema 的也算「要入参」：契约未知，体检不替它编造输入。
fn requires_input(spec: &ActionSpec) -> bool {
    match &spec.input_schema {
        None => true,
        Some(s) => s.get("required").and_then(|r| r.as_array()).is_some_and(|r| !r.is_empty()),
    }
}

// ─────────────────────── 一致性体检（通用回归跑道） ───────────────────────

#[derive(Serialize)]
pub struct ActionCheck {
    pub id: String,
    /// pass / fail / skip
    pub status: String,
    pub ms: u64,
    /// 失败或跳过的原因；通过时为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// output_schema 里声明了、实际返回却缺失（或为 null）的顶层字段。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// 动作按 readiness 约定报的 `ready`。**ready=false 不算测试失败** ——
    /// 「客户没装 Ollama」是事实不是 bug。但它必须被看见。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
}

/// 遍历动作表，把每个**只读且无入参**的动作真跑一遍，按 `output_schema.required` 断言返回形状。
///
/// 这就是取代十几个手写 `--xxx-test` 开关的那条通用跑道：新增一个动作 = 自动多一条冒烟测试，
/// 不用再去 `main()` 里加一个 `--foo-test`（那些开关彼此不一致，有的写文件有的打印，
/// 有的有退出码有的没有，早就成了负债）。
///
/// 写动作（effect != read）一律跳过并写明原因 —— 体检绝不改机器。跳过的都列出来，
/// 不做「静默不跑」：否则一份全绿报告会读成「全覆盖」，而它并不是。
/// 按动作**自己声明的 `timeout_ms`** 跑一次，超时返回 `Err` —— 调用方永远不会无界等待。
///
/// 为什么要有它：`run()` 是裸调用，卡住就一起卡。体检那条路（`run_one`）早就用
/// 「线程 + `recv_timeout`」兜住了，但 `--browser-test` 直接调 `run()`，
/// 于是浏览器动作一旦不返回，整条跑道就永远挂着 —— 实测超过 180 秒没结束，
/// 而**没有任何一行输出说它在等谁**。
///
/// 兜底的口径跟体检**共用同一个来源**：动作自己声明的 `timeout_ms`。
/// 不在调用方另写一个数字 —— 那样两处迟早对不上，对不上的那次正好是出事那次。
/// 超时的线程留着跑完，进程随后退出；这些都是一次性 CLI，不是常驻服务。
pub fn run_bounded(id: &str, input: Value) -> Result<Value, String> {
    let budget = list()
        .into_iter()
        .find(|s| s.id == id)
        .map(|s| std::time::Duration::from_millis(s.timeout_ms))
        // 动作表里没有这个 id：交给 run() 自己去报「未知动作」，别在这儿等。
        .unwrap_or_else(|| std::time::Duration::from_secs(30));
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = id.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(run(&owned, input));
    });
    rx.recv_timeout(budget)
        .unwrap_or_else(|_| Err(format!("timeout: 超过自己声明的 timeout_ms={}", budget.as_millis())))
}

/// 整条体检的**总预算**。单个动作早就有 `timeout_ms` 兜底（见 `run_one`），
/// 但 85 个动作各自最多 10 秒 = 最坏 850 秒 —— 每一步都合规，合起来仍然像挂死。
///
/// 实测本机跑了 2 分钟没结束、`--browser-test` 超过 180 秒没结束，当时的判断是「挂了」；
/// 真相是**它在正常地慢**，而跑道一个字都不说，于是「慢」和「死」在屏幕上长得一模一样。
/// 所以这里补的是两样东西：**看得见的阶段**（每个动作一行进度）和**总的上界**。
///
/// 默认给得很宽（10 分钟）：它是**兜底，不是配额** —— 正常跑一次绝不该碰到它。
/// 真碰到了就说明有东西不对，那时报告必须如实说「还有 N 个没跑」，而不是把
/// 一次没跑完的体检算成通过。可用 `UKING_CONFORMANCE_BUDGET_MS` 覆盖（CI 想收紧时）。
fn conformance_budget() -> std::time::Duration {
    let ms = std::env::var("UKING_CONFORMANCE_BUDGET_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600_000);
    std::time::Duration::from_millis(ms)
}

pub fn conformance(only: Option<&str>) -> Value {
    let mut checks: Vec<ActionCheck> = Vec::new();
    // 进度打到 **stderr**：stdout 要保持是一整份可解析的 JSON（本仓库的 CLI 口径）。
    // 跑的人看得见「现在卡在哪个动作」，机器读到的东西一个字节没变。
    let specs: Vec<_> = list()
        .into_iter()
        .filter(|s| only.map(|p| s.id.starts_with(p)).unwrap_or(true))
        .collect();
    let total_planned = specs.len();
    let started = std::time::Instant::now();
    let budget = conformance_budget();
    // 预算用尽后没能跑的动作 —— **必须报出来**，不能静默少跑几个还说自己通过了。
    let mut not_run: Vec<String> = Vec::new();

    for (i, spec) in specs.into_iter().enumerate() {
        if started.elapsed() >= budget {
            not_run.push(spec.id.clone());
            continue;
        }
        eprintln!(
            "[conformance] {}/{} {} …（已用 {}s）",
            i + 1,
            total_planned,
            spec.id,
            started.elapsed().as_secs()
        );
        if spec.effect != "read" {
            // 写动作不能真跑，但**门禁可以安全地敲**：确认检查发生在入参校验和 handler 之前，
            // 拿空入参调进去只会被挡回来，机器一个字节都不会被改。
            // 这条断言的价值：确保「必须确认」的动作真的锁着 —— 哪天有人不小心把
            // confirmation 写成 never，或者把门禁挪到 handler 里面去，这里当场变红。
            if spec.confirmation == "required" {
                checks.push(gate_check(&spec.id));
            } else {
                checks.push(skipped(
                    &spec.id,
                    format!("effect={} 且 confirmation=never：没有门禁可敲，体检绝不改机器", spec.effect),
                ));
            }
            continue;
        }
        if requires_input(&spec) {
            checks.push(skipped(
                &spec.id,
                "有必填入参、或压根没声明 input_schema：通用体检不替它编造输入".into(),
            ));
            continue;
        }
        checks.push(run_one(&spec));
    }
    let pass = checks.iter().filter(|c| c.status == "pass").count();
    let fail = checks.iter().filter(|c| c.status == "fail").count();
    let skip = checks.iter().filter(|c| c.status == "skip").count();
    // 「这台机器上哪些能力其实是废的」—— 远程排障一眼就要看到的那一段。
    // 刻意**不计入 ok**：客户没装 Ollama 不是 bug，但也绝不能不说。
    let not_ready: Vec<Value> = checks
        .iter()
        .filter(|c| c.ready == Some(false))
        .map(|c| json!({ "id": c.id, "blockers": c.blockers }))
        .collect();
    // 配方跟动作一起体检。`--only` 过滤动作时不跑配方（那会让「只查一个动作」的报告
    // 莫名其妙地因为别处的配方变红）。
    let (recipe_checks, recipe_fail) = if only.is_some() { (Vec::new(), 0) } else { check_recipes() };
    if !not_run.is_empty() {
        eprintln!(
            "[conformance] ⚠ 总预算 {}s 用尽，还有 {} 个动作没跑：{}",
            budget.as_secs(),
            not_run.len(),
            not_run.join(", ")
        );
    }
    eprintln!("[conformance] 完成：{}s", started.elapsed().as_secs());
    json!({
        // 🔴 **没跑完就不能算通过**。少跑几个却报 ok=true，读的人会以为全查过了 ——
        // 那正是本仓库反复在修的「统计者谎报」。默认预算 10 分钟，正常跑碰不到这条。
        "ok": fail == 0 && recipe_fail == 0 && not_run.is_empty(),
        "not_run": not_run,
        "not_ready": not_ready,
        "recipes": recipe_checks,
        "recipe_fail": recipe_fail,
        "version": env!("CARGO_PKG_VERSION"),
        // ★ 别把 debug 的 ms 当性能结论。实测同一份 352MB 日志：debug 2905ms / release 751ms，
        // 差 4 倍不止，而且**快慢关系都可能反过来**（`str::contains`、`read_until` 这类
        // 在 debug 下没优化）。这里把 profile 打进报告，就是防止有人（包括我自己）再照着
        // debug 的数字下「这个动作太慢」的判断 —— 那次判断错了一整轮。
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "total": checks.len(),
        "pass": pass, "fail": fail, "skip": skip,
        "checks": checks,
    })
}

// ─────────────────────── 支持包（一条命令拿走整机现状） ───────────────────────

/// 递归把每个字符串叶子过一遍脱敏函数。
///
/// **不是**把整份 JSON 序列化成文本再脱敏 —— 那样替换可能插进引号把 JSON 弄坏，
/// 再 parse 回来就炸了。按叶子处理，结构永远完好。
fn redact_leaves(v: &Value, f: fn(&str) -> String) -> Value {
    match v {
        Value::String(s) => Value::String(f(s)),
        Value::Array(a) => Value::Array(a.iter().map(|x| redact_leaves(x, f)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), redact_leaves(x, f))).collect()),
        other => other.clone(),
    }
}

/// **支持包**：一条命令把这台机器的现状打成一份 JSON。
///
/// 为什么要有它：远程维护一台客户机，此前要一条条敲 PowerShell 去拼
/// （2026-07-28 排 T-King 那次敲了二十来条，光处理 GUI-exe 不等待、引号、编码就耗掉大半时间）。
/// 而这些信息**本来就都在动作表和日志目录里** —— 缺的只是「一次拿走」这个动作。
///
/// 内容 = 版本/环境 + 动作表统计 + **每个只读无入参动作的真实返回** + 全部模块日志尾部。
/// 失败的动作带结构化错误（`code`/`blame`/`retriable`），远程侧不用再猜。
///
/// 参数由组合根注入，保持本文件零业务依赖：
/// - `logs`：`(模块名, 日志尾部)`，实参给 `ulog::all_tails(n)`
/// - `redact`：脱敏函数，实参给 `feedback::desensitize`；`None` = 不脱敏（本机排查用）
pub fn bundle(logs: Vec<(String, String)>, redact: Option<fn(&str) -> String>) -> Value {
    let specs = list();
    let mut probes: Vec<Value> = Vec::new();
    for spec in &specs {
        if spec.effect != "read" || requires_input(spec) {
            continue;
        }
        // 线程 + `recv_timeout` 卡死线，同 conformance 的做法。
        // **支持包尤其不能被一个卡住的动作拖死** —— 那等于在最需要它的时候（客户机出问题）失效。
        // 超时的线程留着跑完，进程随后就退；这是一次性 CLI 采集，不是常驻服务。
        let id = spec.id.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let t0 = std::time::Instant::now();
        std::thread::spawn(move || {
            let _ = tx.send(run(&id, json!({})));
        });
        let outcome = rx.recv_timeout(std::time::Duration::from_millis(spec.timeout_ms));
        let ms = t0.elapsed().as_millis() as u64;
        probes.push(match outcome {
            Ok(Ok(v)) => json!({ "id": spec.id, "ok": true, "ms": ms, "value": v }),
            Ok(Err(e)) => json!({ "id": spec.id, "ok": false, "ms": ms, "error": ActionError::classify(&e) }),
            Err(_) => json!({ "id": spec.id, "ok": false, "ms": ms, "error": ActionError {
                code: "timeout".into(), blame: Blame::Bug, retriable: true,
                message: format!("超过自己声明的 timeout_ms={}", spec.timeout_ms),
                hint: "动作卡住了 —— 支持包不等它，剩下的照常采集".into(),
            }}),
        });
    }
    let failed: Vec<&Value> = probes.iter().filter(|p| p["ok"] == json!(false)).collect();
    // 「这台机器上哪些功能其实是废的」—— 和 conformance 同口径：ready=false 不算失败，但必须被看见。
    let not_ready: Vec<Value> = probes
        .iter()
        .filter(|p| p["value"]["ready"] == json!(false))
        .map(|p| json!({ "id": p["id"], "blockers": p["value"]["blockers"] }))
        .collect();
    let out = json!({
        "schema": "uking.support-bundle/1",
        "ok": failed.is_empty(),
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            // debug 的 ms 不能当性能结论（同 conformance 的教训），据此别下判断。
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        },
        "actions": {
            "total": specs.len(),
            "read": specs.iter().filter(|s| s.effect == "read").count(),
            "write": specs.iter().filter(|s| s.effect != "read").count(),
            "probed": probes.len(),
            "failed": failed.len(),
        },
        "not_ready": not_ready,
        "probes": probes,
        "logs": logs.into_iter().map(|(k, v)| json!({ "module": k, "tail": v })).collect::<Vec<_>>(),
    });
    match redact {
        Some(f) => redact_leaves(&out, f),
        None => out,
    }
}

// ─────────────────────── 绑定核对（GUI 控件 ↔ 动作） ───────────────────────

/// 扫前端源码里的 `data-action-id`，和动作表对一遍。
///
/// 为什么要有这个：光在按钮上挂个属性、没人核对，等于没挂 —— 动作改名后属性会静静地变成
/// 死字符串，而「这个按钮绑的是哪个动作」正是宪法第 14/15 条要求可被机器检查的东西。
///
/// 三类结果：
/// - `bound`   ：动作有 GUI 控件（可能不止一个，多个界面调同一动作是正常的）
/// - `unbound` ：动作没有 GUI 控件。**不算失败** —— 有些动作本来就只给 CLI/AI 用，
///               或只在向导自动流程里跑，没有按钮是实话。
/// - `stale`   ：源码里的 id 在动作表里找不到。**算失败** —— 这是改名后忘了同步，
///               自动化点它会点空。
pub fn bindings(src_dir: &str) -> Value {
    let ids: Vec<String> = list().into_iter().map(|a| a.id).collect();
    let mut found: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut stale: Vec<Value> = vec![];
    scan_dir(std::path::Path::new(src_dir), &ids, &mut found, &mut stale);

    let bound: Vec<Value> = found
        .iter()
        .map(|(id, files)| json!({ "id": id, "controls": files }))
        .collect();
    let unbound: Vec<&String> = ids.iter().filter(|i| !found.contains_key(*i)).collect();
    // ★ **反方向**：界面能触发的业务能力里，有多少还没进动作表。
    //
    // 上面三类查的是「动作有没有控件」；真正决定「AI 能不能替人操作这个软件」的是反过来那句：
    // **控件能干的事，动作表里有没有**。没有的那些，AI 就只能去点像素。
    //
    // 判据故意不是「数按钮」：切标签、展开折叠、滚动都是界面动作，宪法第 13 条明说不进核心，
    // 拿按钮总数当分母只会逼人把 UI 动作也登记进去，把动作表撑成一张噪音表。
    // 用 `invoke("<command>")` 做代理 —— **一个会调进 Rust 的控件，按定义就不只是界面动作**。
    //
    // 这份清单**没有 pass/fail**：不是每个 command 都该成为动作（`hide_to_tray` 就不该）。
    // 它是一张工作清单，不是一个要刷高的分数。唯一算失败的仍然只有 `stale`。
    let mut ui_commands: std::collections::BTreeSet<String> = Default::default();
    scan_invokes(std::path::Path::new(src_dir), &mut ui_commands);
    json!({
        "ok": stale.is_empty(),
        "src": src_dir,
        "actions": ids.len(),
        "ui_commands": ui_commands.iter().collect::<Vec<_>>(),
        "ui_commands_count": ui_commands.len(),
        "bound": bound,
        "unbound": unbound,
        "stale": stale,
    })
}

fn scan_dir(
    dir: &std::path::Path,
    ids: &[String],
    found: &mut std::collections::BTreeMap<String, Vec<String>>,
    stale: &mut Vec<Value>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        if ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            scan_dir(&p, ids, found, stale);
            continue;
        }
        if !matches!(p.extension().and_then(|e| e.to_str()), Some("tsx") | Some("ts")) {
            continue;
        }
        let Ok(txt) = std::fs::read_to_string(&p) else { continue };
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        // 找 data-action-id="a b c"：一个控件可以绑多个动作（开关类按钮就是开/关两个）。
        for (i, _) in txt.match_indices("data-action-id=\"") {
            let rest = &txt[i + "data-action-id=\"".len()..];
            let Some(end) = rest.find('"') else { continue };
            let line = txt[..i].matches('\n').count() + 1;
            for token in rest[..end].split_whitespace() {
                if ids.iter().any(|x| x == token) {
                    found.entry(token.to_string()).or_default().push(format!("{name}:{line}"));
                } else {
                    stale.push(json!({ "id": token, "at": format!("{name}:{line}") }));
                }
            }
        }
        // 生成 client 的常量也是真实绑定，而且比复制字符串更不容易漂移：
        // data-action-id={ACTION.RUNTIME_DRIVER_APPLY}
        for (i, _) in txt.match_indices("data-action-id={ACTION.") {
            let rest = &txt[i + "data-action-id={ACTION.".len()..];
            let Some(end) = rest.find('}') else { continue };
            let symbol = rest[..end].trim();
            let line = txt[..i].matches('\n').count() + 1;
            if let Some(id) = ids.iter().find(|id| action_symbol(id) == symbol) {
                found.entry(id.clone()).or_default().push(format!("{name}:{line}"));
            } else {
                stale.push(json!({ "id": format!("ACTION.{symbol}"), "at": format!("{name}:{line}") }));
            }
        }
    }
}

/// 扫前端里所有 `invoke("<command>")` 的命令名（含 `invoke<T>("x")` 这种带泛型的写法）。
///
/// 只认**字面量**：`invoke(cmdVar)` 这类动态调用扫不到，如实漏掉好过瞎猜一个名字 ——
/// 这份清单的用途是「照着它一条条判断该不该进动作表」，混进编出来的名字就没法用了。
fn scan_invokes(dir: &std::path::Path, out: &mut std::collections::BTreeSet<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        if ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            scan_invokes(&p, out);
            continue;
        }
        if !matches!(p.extension().and_then(|e| e.to_str()), Some("tsx") | Some("ts")) {
            continue;
        }
        let Ok(txt) = std::fs::read_to_string(&p) else { continue };
        for (i, _) in txt.match_indices("invoke") {
            let rest = &txt[i + "invoke".len()..];
            // 跳过可能的泛型参数 `<any>` / `<Foo[]>`
            let rest = match rest.strip_prefix('<') {
                Some(r) => match r.find('>') {
                    Some(k) => &r[k + 1..],
                    None => continue,
                },
                None => rest,
            };
            let Some(r) = rest.strip_prefix('(') else { continue };
            let r = r.trim_start();
            let Some(q) = r.strip_prefix('"').or_else(|| r.strip_prefix('\'')) else { continue };
            let Some(end) = q.find(['"', '\'']) else { continue };
            let name = &q[..end];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.insert(name.to_string());
            }
        }
    }
}

fn action_symbol(id: &str) -> String {
    id.chars()
        .map(|character| if character.is_ascii_alphanumeric() { character.to_ascii_uppercase() } else { '_' })
        .collect()
}

fn skipped(id: &str, reason: String) -> ActionCheck {
    ActionCheck { id: id.into(), status: "skip".into(), ms: 0, reason: Some(reason), missing: vec![], ready: None, blockers: vec![] }
}

/// 敲一下写动作的确认门禁：不带 `confirm` 调它，**必须**被 `confirmation_required` 挡回来。
/// 任何别的结果都是 fail —— 尤其是 `Ok`，那意味着门没锁、机器刚被改了。
fn gate_check(id: &str) -> ActionCheck {
    let t0 = std::time::Instant::now();
    let outcome = run(id, json!({}));
    let ms = t0.elapsed().as_millis() as u64;
    let (status, reason) = match &outcome {
        Err(e) if e.starts_with("confirmation_required") => ("pass", Some("门禁锁着（未确认被拒）".into())),
        Err(e) => ("fail", Some(format!("门禁没先拦住，先报了别的错：{e}"))),
        Ok(_) => ("fail", Some("🔴 门禁失效：没给 confirm 也执行了".into())),
    };
    ActionCheck { id: id.into(), status: status.into(), ms, reason, missing: vec![], ready: None, blockers: vec![] }
}

/// conformance 判卡死时给 `timeout_ms` 的放宽倍数。**release 一分不放（1），debug 放 6 倍。**
///
/// 🔴 为什么必须放宽：`timeout_ms` 描述的是**发出去那个 release 二进制**的行为，
/// 可 `pnpm run action-parity:verify` 跑的是 `cargo run`（debug，未优化）。
/// 2026-08-22 实测同一条 `runtime.usage_meter.inspect`：**release 10s、debug 49s**（5 倍），
/// 而它的预算正好是 60s —— 于是客户/开发机上会话日志一多，verify 就随机红一条，
/// 而**红出来的样子是「动作卡死」，真因却是「拿 debug 的速度去判 release 的预算」**。
/// 这是本项目最忌的那类跑道自证：跑道自己错了，却把账记在被测对象头上。
///
/// 取 6 而不是 5：实测比值就是 5，留一档余量；再大就等于放弃「抓卡死」这个目的了。
/// 🔴 超时文案里**两个数都要出现**（放宽值 + 契约值），否则读报告的人会把 360000 当成真实预算。
///
/// 独立成函数是为了**能被确定性地断言**：靠跑一次真动作去证明放宽起没起作用，
/// 结果取决于当天日志攒了多少 —— 实测那一次 59549ms，差 451ms 没越线，什么也证明不了。
fn conformance_slack() -> u64 {
    slack_for_build(cfg!(debug_assertions))
}

/// 策略本体，**把 `cfg!` 提成参数**——否则 release 那一支在测试里根本够不着：
/// 用例跑在 debug 下，`if cfg!(debug_assertions) { slack } else { 1 } == slack` 是恒真的重言式。
/// 2026-08-22 我第一版就是这么写的，变异验证里「连 release 也放宽」那一刀**没被抓住**。
/// 一条不会失败的检查等于没有检查。
fn slack_for_build(debug: bool) -> u64 {
    if debug {
        6
    } else {
        1
    }
}

fn run_one(spec: &ActionSpec) -> ActionCheck {
    let required: Vec<String> = spec
        .output_schema
        .as_ref()
        .and_then(|s| s.get("required"))
        .and_then(|r| r.as_array())
        .map(|r| r.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let id = spec.id.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let t0 = std::time::Instant::now();
    // 每个动作单开一条线程 + `recv_timeout`：声明了 timeout_ms 却卡死的动作必须被抓出来，
    // 而不是让整条体检跑道跟着一起挂（宪法第 9 条）。超时的线程会留着跑完，
    // 进程随后就退出，无所谓 —— 这是一次性的 CLI 体检，不是常驻服务。
    std::thread::spawn(move || {
        let _ = tx.send(run(&id, json!({})));
    });
    let slack = conformance_slack();
    let budget = std::time::Duration::from_millis(spec.timeout_ms * slack);
    let outcome = rx.recv_timeout(budget);
    let ms = t0.elapsed().as_millis() as u64;

    let mut ready: Option<bool> = None;
    let mut blockers: Vec<String> = vec![];
    let (status, reason, missing) = match outcome {
        Err(_) => (
            "fail",
            Some(if slack == 1 {
                format!("timeout: 超过自己声明的 timeout_ms={}", spec.timeout_ms)
            } else {
                // debug 下把「真实预算 × 放宽倍数」都写出来，别让人把放宽值当成契约值。
                format!(
                    "timeout: 超过 timeout_ms={} 的 {slack} 倍（debug 构建放宽；契约值是 {} ms）",
                    spec.timeout_ms * slack,
                    spec.timeout_ms
                )
            }),
            vec![],
        ),
        // ★ **外部依赖没装，在只读动作上不是失败，是 not_ready。**
        //
        // 这跟 readiness 约定是同一条：「客户没装 Ollama」是事实不是 bug，但绝不能不说。
        // 区别只在于表达方式 —— Ollama 那类动作自己返回 `ready:false`，
        // 而 `browser.*` 这类是直接抛 `not_installed`（它们返回的是内容不是能力描述，
        // 硬塞一个 ready 字段反而别扭）。两种写法应当得到同一种待遇。
        //
        // 干净机实测（0.9.93）：没装 agent-browser 的机器上
        // `browser.snapshot/screenshot/tabs/stream` 四条全判 fail，于是
        // **任何没装 agent-browser 的客户机 conformance 恒红** —— 红得毫无信息量，
        // 真出问题时反而没人看。现在归进 `not_ready`：不计入 ok，但一条不落地列出来。
        Ok(Err(e)) if e.starts_with("not_installed") => {
            ready = Some(false);
            blockers = vec![e.trim_start_matches("not_installed:").trim().to_string()];
            ("pass", None, vec![])
        }
        Ok(Err(e)) => ("fail", Some(e), vec![]),
        Ok(Ok(value)) => match value.as_object() {
            None => ("fail", Some("output 不是 JSON 对象".to_string()), vec![]),
            Some(obj) => {
                // 按 readiness 约定收一下「能不能用」。有就记，没有就算了 ——
                // 不是每个动作都描述一项能力。
                ready = obj.get("ready").and_then(Value::as_bool);
                blockers = obj
                    .get("blockers")
                    .and_then(|b| b.as_array())
                    .map(|b| b.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let missing: Vec<String> = required
                    .into_iter()
                    .filter(|k| obj.get(k).is_none_or(Value::is_null))
                    .collect();
                if !missing.is_empty() {
                    ("fail", Some("output_schema 声明的字段缺失或为 null".to_string()), missing)
                } else if let Some(bad) = check_observation(&spec.observes, obj) {
                    // ★ 形状对 ≠ 真看到了东西。声明了 observes 就必须交代每个来源的死活。
                    ("fail", Some(bad), missing)
                } else {
                    ("pass", None, missing)
                }
            }
        },
    };
    ActionCheck { id: spec.id.clone(), status: status.into(), ms, reason, missing, ready, blockers }
}

/// ★ **观测记账检查**（影核提案 docs/PROPOSAL-OBSERVATION-ACCOUNTING §5.4）。
///
/// 声明了 `observes` 的动作，必须交代**每一个**来源的死活。返回 `Some(原因)` = 违规。
///
/// 🔴 **这里查的不是「读到几条」，是「有没有记账」。**
/// 客户机上没装 Codex，`count:0` 是事实不是 bug —— 跟 `not_ready` 一个道理，如实报出来即可。
/// 违规的是**沉默**：一个 0 摆在那儿不说为什么，调用方（尤其是 AI）会把
/// 「我没看到」如实转述成「你这台机器上没有」，语气和真相一样笃定。
fn check_observation(observes: &[&str], obj: &serde_json::Map<String, Value>) -> Option<String> {
    if observes.is_empty() {
        return None;
    }
    let Some(sources) = obj.get("sources").and_then(Value::as_array) else {
        return Some(format!(
            "声明了 observes={observes:?} 却没返回 sources 记账 —— 空结果说不清是「没有」还是「没读到」"
        ));
    };
    for want in observes {
        let Some(src) = sources.iter().find(|s| {
            s.get("tool").and_then(Value::as_str) == Some(want)
        }) else {
            return Some(format!("sources 里缺来源 `{want}` —— 少一个来源就是少一块世界，不许静默省略"));
        };
        // present / readable 必须显式：`present:false` 的 0 和 `readable:false` 的 0
        // 在语义上是两个结果 —— 前者是世界的事实，后者是我们的限制。
        for key in ["present", "readable"] {
            if src.get(key).and_then(Value::as_bool).is_none() {
                return Some(format!("来源 `{want}` 没给 `{key}`（必须是 bool）"));
            }
        }
        let count = src.get("count").and_then(Value::as_u64);
        let Some(count) = count else {
            return Some(format!("来源 `{want}` 没给 `count`（样本量必须报）"));
        };
        // 0 必须能被解释。这就是「不许沉默」落到运行时的那一条。
        if count == 0 {
            let note = src.get("note").and_then(Value::as_str).unwrap_or("");
            if note.trim().is_empty() {
                return Some(format!(
                    "来源 `{want}` 是 0 条却没写 note —— 「没有」和「没读到」必须分得开"
                ));
            }
        }
    }
    // 截断了就得说，不能装作全看过。
    if obj.get("truncated").and_then(Value::as_bool).is_none() {
        return Some("声明了 observes 却没给 `truncated` —— 取样被截断时必须能说出来".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// conformance 的 debug 放宽策略。**这条用例存在的理由是「真跑一遍证明不了」**：
    /// 2026-08-22 那次实测 `usage_meter` 用了 59549ms —— 差 451ms 没越过 60000 的线，
    /// 于是那一次跑绿既不能证明放宽起了作用，也不能证明没起。
    /// 判据取决于当天日志攒了多少 = 不是判据。所以把策略拎出来直接断言。
    ///
    /// 🔴 三件事一起钉：① debug 下真的放宽了（不是写了个 1）；② release 下**一分不放**
    ///（放宽是给跑道自己的补偿，不许悄悄变成给动作的宽限）；③ 超时文案里契约值必须还在
    ///（只报放宽值 = 让人把 360000 当成真实预算，比不报更误导）。
    #[test]
    fn conformance_slack_only_compensates_debug_builds_and_still_shows_the_contract_value() {
        let slack = conformance_slack();
        // 用例本身就跑在 debug 下（cargo test 默认 debug），所以这里必然是放宽那一支。
        assert!(cfg!(debug_assertions), "这条用例假定跑在 debug 构建下");
        assert!(
            slack > 1,
            "debug 比 release 慢约 5 倍（实测 49s vs 10s），不放宽就会拿跑道自己的慢去判动作卡死"
        );
        assert!(
            slack <= 10,
            "放宽到 {slack} 倍等于放弃「抓卡死」这个目的了 —— 一个永远不会红的超时判据不是判据"
        );

        // 🔴 release 那一支必须**直接调到**，不能靠 cfg 分支「推断」——
        // 推断版在 debug 下是恒真重言式，变异验证里「连 release 也放宽」那一刀从它底下溜了过去。
        assert_eq!(
            slack_for_build(false),
            1,
            "release 构建必须一分不放：放宽是给跑道自己的补偿，不许悄悄变成给动作的宽限"
        );
        assert_eq!(slack_for_build(true), slack, "debug 那一支要和实际用的一致");

        // ③ 文案：放宽值和契约值都要在，缺一个就会被误读。
        let contract = 60_000u64;
        let msg = format!(
            "timeout: 超过 timeout_ms={} 的 {slack} 倍（debug 构建放宽；契约值是 {} ms）",
            contract * slack,
            contract
        );
        assert!(msg.contains(&contract.to_string()), "超时文案丢了契约值 {contract}");
        assert!(
            msg.contains(&(contract * slack).to_string()),
            "超时文案丢了实际用的放宽预算"
        );
    }

    /// 观测记账检查本身的规格说明。**一条不会失败的检查等于没有检查** ——
    /// 所以每种违规都必须在这里被逮住。
    #[test]
    fn observation_accounting_catches_silence() {
        let obs = &["claude", "codex"];
        let ok = json!({
            "sources": [
                {"tool":"claude","present":true,"readable":true,"count":3,"note":""},
                {"tool":"codex","present":false,"readable":false,"count":0,"note":"没装 Codex"}
            ],
            "truncated": false
        });
        assert!(check_observation(obs, ok.as_object().unwrap()).is_none(), "合规的记账不该被判违规");

        // 没声明 observes 的动作完全不受影响（不制造无谓负担）
        assert!(check_observation(&[], json!({}).as_object().unwrap()).is_none());

        // ① 压根没记账
        let v = json!({"tasks": [], "truncated": false});
        assert!(check_observation(obs, v.as_object().unwrap()).unwrap().contains("没返回 sources"));

        // ② 少一个来源 —— 静默省略一块世界
        let v = json!({"sources":[{"tool":"claude","present":true,"readable":true,"count":1}],"truncated":false});
        assert!(check_observation(obs, v.as_object().unwrap()).unwrap().contains("缺来源"));

        // ③ ★ 0 条却不解释 —— 本提案的核心场景
        let v = json!({
            "sources":[
                {"tool":"claude","present":true,"readable":true,"count":0,"note":"  "},
                {"tool":"codex","present":true,"readable":true,"count":2}
            ],
            "truncated": false});
        assert!(check_observation(obs, v.as_object().unwrap()).unwrap().contains("没写 note"));

        // ④ 分不清「没有」和「读不动」
        let v = json!({
            "sources":[
                {"tool":"claude","present":true,"count":1},
                {"tool":"codex","present":true,"readable":true,"count":2}
            ],
            "truncated": false});
        assert!(check_observation(obs, v.as_object().unwrap()).unwrap().contains("readable"));

        // ⑤ 截断了不说
        let v = json!({"sources":[
            {"tool":"claude","present":true,"readable":true,"count":1},
            {"tool":"codex","present":true,"readable":true,"count":2}]});
        assert!(check_observation(obs, v.as_object().unwrap()).unwrap().contains("truncated"));
    }

    /// 错误分类是**契约**，不是尽力而为 —— 这些断言就是它的规格说明。
    /// 每条都用见过的真实报错原文，改词表时这里当场兜住。
    #[test]
    fn classify_real_world_errors() {
        // 出处：pc-***（2026-07-28）。同模型同提示词 16:24 失败 16:27 成功 —— 必须可重试。
        // 这条以前被判成永久错误，是 T-King 一键成片全灭的直接原因。
        let e = ActionError::classify("火山视频任务创建失败，已自动退回本次扣费。");
        assert_eq!(e.code, "upstream_transient");
        assert_eq!(e.blame, Blame::Upstream);
        assert!(e.retriable, "上游抖动必须可重试，否则就是 T-King 那个 bug");
        assert!(!e.worth_reporting(), "上游抖动不是我们的 bug，别上报");

        // 客户没钱不是 bug —— report.rs 那条「别把余额不足当 bug 上报」的规则，这里是它的源头。
        for msg in ["余额不足，请充值", "insufficient user quota"] {
            let e = ActionError::classify(msg);
            assert_eq!(e.blame, Blame::User, "{msg}");
            assert!(!e.retriable, "{msg}：重试也还是没钱");
            assert!(!e.worth_reporting(), "{msg}");
        }

        // 鉴权 / 白名单：重试无益，要人去改配置。
        assert_eq!(ActionError::classify("Unauthorized").code, "unauthorized");
        assert_eq!(
            ActionError::classify("token has no access to model").blame,
            Blame::User
        );

        // 核心按规矩挡下的操作：**不是 bug**。归不上类的话会落成 blame=bug + code=unknown，
        // CLI / MCP / 远端影子看到会以为程序坏了去上报 —— 而它正是核心在正常工作。
        // 真实来源：`action run runtime.provider.delete --input '{"id":"official"}'`（跨进程实测）。
        for msg in [
            "「官方直连（还原）」不能移除 —— 它是还原成官方登录的出口，删了就没退路了",
            "「虾盘云」是内置驱动，不能修改或覆盖",
            "「custom-x」不是内置驱动，自定义供应商删除后需要重新添加",
        ] {
            let e = ActionError::classify(msg);
            assert_eq!(e.blame, Blame::User, "{msg}");
            assert_eq!(e.code, "refused", "{msg}");
            assert!(!e.retriable, "{msg}：重试永远是同一个答案");
            assert!(!e.worth_reporting(), "{msg}：这不是 bug，别上报");
        }

        // 网络：可重试，但赖的是网不是上游 —— 处置不同（换网 vs 换渠道）。
        let e = ActionError::classify("connection was closed");
        assert_eq!(e.blame, Blame::Network);
        assert!(e.retriable);

        // 协议层自己发的错，文案确定，必须精确归到 bug（是调用方/我们写错了）。
        assert_eq!(ActionError::classify("unknown_action: runtime.nope").code, "unknown_action");
        assert_eq!(
            ActionError::classify("confirmation_required: `x` 会改这台机器").blame,
            Blame::Bug
        );
        // conflict 例外：状态被别人改过，重新读一次再来是对的处置 —— 所以可重试。
        let e = ActionError::classify("conflict: 状态在你读到之后被改过（expected=a current=b）");
        assert!(e.retriable, "conflict 的正确处置是重读再试");

        // 缺可选依赖：引导装依赖，不是报 bug。pc-***（2026-08-24）doc.read 实测原文。
        // 🔴 词条必须窄到 `no module named 'markitdown'`：裸 `no module named` 会把我们
        // 自己漏打包模块的 bug 静默归咎客户、关掉自动上报 —— 那类错就该落 unknown+bug 刺眼。
        let e = ActionError::classify(
            "读文档 没有输出 JSON。stderr: [错误] 没有可用的转换器。装其一即可：markitdown: No module named 'markitdown'",
        );
        assert_eq!(e.code, "missing_dependency");
        assert_eq!(e.blame, Blame::User);
        assert!(!e.worth_reporting(), "缺依赖不是 bug，别上报");
        // 别的模块缺包 = 我们的打包 bug，必须保持刺眼（unknown+bug+上报），不许被宽词条吃掉
        let e = ActionError::classify("读文档 没有输出 JSON。stderr: No module named 'requests'");
        assert_eq!(e.code, "unknown", "没见过原文的缺包不许归 missing_dependency");
        assert_eq!(e.blame, Blame::Bug);
        assert!(e.worth_reporting());
    }

    /// 没归上类必须刺眼：`unknown` + `blame=bug` + 不可重试。
    /// 悄悄算成「客户的问题」会让真 bug 永远不被看见；盲目重试未知的写动作可能双写。
    #[test]
    fn unclassified_is_loud_and_not_retried() {
        let e = ActionError::classify("某种从没见过的鬼话");
        assert_eq!(e.code, "unknown");
        assert_eq!(e.blame, Blame::Bug);
        assert!(!e.retriable, "协议层不知道动作幂不幂等，未知错绝不自动重试");
        assert!(e.worth_reporting(), "未归类的错必须能被看见");
    }

    /// 脱敏必须按叶子走，且保持 JSON 结构完好 ——
    /// 整份序列化成文本再替换，替换串里带引号就会把 JSON 弄坏，parse 回来直接炸。
    #[test]
    fn redaction_walks_leaves_and_keeps_shape() {
        fn fake_redact(s: &str) -> String {
            if s.starts_with("sk-") { "***".into() } else { s.into() }
        }
        let v = json!({ "a": "sk-secret", "b": [ "sk-x", 1, true ], "c": { "d": "keep" } });
        let r = redact_leaves(&v, fake_redact);
        assert_eq!(r["a"], json!("***"));
        assert_eq!(r["b"][0], json!("***"));
        assert_eq!(r["b"][1], json!(1), "非字符串叶子不能被动");
        assert_eq!(r["b"][2], json!(true));
        assert_eq!(r["c"]["d"], json!("keep"));
    }

    /// 造一个和 `runtime.driver.apply` 同形状的写动作契约：三个必填 + 一个可选 `model`。
    fn driver_apply_like() -> Action {
        write(
            "test.driver.apply",
            "t",
            "d",
            1_000,
            "required",
            json!({
                "provider_id": { "type": "string" },
                "api_key": { "type": "string" },
                "model": { "type": "string" },
                "targets": { "type": "array" }
            }),
            &["provider_id", "api_key", "targets"],
            &["applied"],
            |_, _, _| Ok(json!({ "applied": {} })),
            None,
        )
    }

    /// 🔴 `minimum` / `maximum` **不是装饰**。
    ///
    /// 这是 `enum` 那件事的第三遍（`additionalProperties` 有人执行 → `enum` 到 0.9.99 才有
    /// → 数值边界到现在）。不执行的后果比不声明更坏：handler 里普遍写着 `clamp(1, 365)`，
    /// 于是传 99999 天会**静默**拿到 365 天的报告，调用方还以为是 99999 天的 ——
    /// 报错他会改参数，静默他会拿着错误的口径去做决定。
    #[test]
    fn numeric_bounds_are_enforced_not_silently_clamped() {
        let a = write(
            "test.bounded",
            "t",
            "d",
            1_000,
            "none",
            json!({ "days": { "type": "integer", "minimum": 1, "maximum": 365 } }),
            &[],
            &["ok"],
            |_, _, _| Ok(json!({ "ok": true })),
            None,
        );
        assert_eq!(validate_input(&a.spec, &json!({ "days": 30 })), Ok(()));
        assert_eq!(validate_input(&a.spec, &json!({ "days": 1 })), Ok(()), "边界值本身要收");
        assert_eq!(validate_input(&a.spec, &json!({ "days": 365 })), Ok(()), "边界值本身要收");
        assert!(
            validate_input(&a.spec, &json!({ "days": 99999 })).is_err(),
            "超上限必须拒，不能悄悄 clamp 成 365 再返回一份口径不同的报告"
        );
        assert!(validate_input(&a.spec, &json!({ "days": 0 })).is_err(), "低于下限必须拒");
    }

    /// **回归（0.9.70~0.9.72 全量事故）**：可选字段给 `null` 必须等于「没给」。
    ///
    /// 事故本体：前端 `invoke("apply_provider", { model: null })` 过 IPC 到核心就是
    /// `"model": null`，校验判成「字段 `model` 应为 string」→ 装机向导「写入底层配置」、
    /// 「还原官方直连」、AI 设置页 per-tool 切驱动、Codex 专区一键接入**全部**写不进去，
    /// 客户看到的就是那句 `写配置失败：invalid_input: 字段 model 应为 string`。
    #[test]
    fn null_on_optional_field_means_absent() {
        let a = driver_apply_like();
        let input = json!({
            "provider_id": "xiapan",
            "api_key": "sk-x",
            "model": null,
            "targets": ["claude"],
            "confirm": true
        });
        assert_eq!(validate_input(&a.spec, &input), Ok(()));
    }

    /// 必填字段给 null 是真的少东西，照旧拒 —— 放行 null 不等于放行空。
    #[test]
    fn null_on_required_field_still_rejected() {
        let a = driver_apply_like();
        let input = json!({ "provider_id": null, "api_key": "sk-x", "targets": ["claude"], "confirm": true });
        assert!(validate_input(&a.spec, &input).is_err());
    }

    /// 真类型错（数字冒充字符串）仍然要拦。这条校验的价值就在这儿，别为了修 null 把它一起废了。
    #[test]
    fn wrong_type_still_rejected() {
        let a = driver_apply_like();
        let input = json!({ "provider_id": "xiapan", "api_key": "sk-x", "model": 7, "targets": ["claude"], "confirm": true });
        assert_eq!(
            validate_input(&a.spec, &input),
            Err("invalid_input: 字段 `model` 应为 string".to_string())
        );
    }

    /// 打错的参数名依旧当场报错（`dayz` 之于 `days`），不许静默走默认值。
    #[test]
    fn unknown_field_still_rejected() {
        let a = driver_apply_like();
        let input = json!({ "provider_id": "xiapan", "api_key": "sk-x", "targets": ["claude"], "confirm": true, "modle": "x" });
        assert!(validate_input(&a.spec, &input).unwrap_err().contains("未知字段"));
    }

    /// 缺必填仍然报「缺少必填字段」，不被 null 那条捷径吃掉。
    #[test]
    fn missing_required_still_rejected() {
        let a = driver_apply_like();
        let input = json!({ "provider_id": "xiapan", "confirm": true });
        assert!(validate_input(&a.spec, &input).unwrap_err().contains("缺少必填字段"));
    }

    /// 造一个带 `enum` 的契约：一个顶层枚举字段 + 一个元素带枚举的数组
    /// （形状照抄 `runtime.driver.apply_everywhere` 的 `targets`）。
    fn enum_contract_like() -> Action {
        write(
            "test.enum.apply",
            "t",
            "d",
            1_000,
            "required",
            json!({
                "mode": { "type": "string", "enum": ["merge", "replace"] },
                "targets": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["claude", "pi"] }
                }
            }),
            &[],
            &["applied"],
            |_, _, _| Ok(json!({ "applied": {} })),
            None,
        )
    }

    /// **回归（0.9.99）**：契约里写了 `enum` 就必须执行。
    ///
    /// 事故本体：`runtime.driver.apply_everywhere` 的 `targets` 声明只认
    /// claude/codex/clawx/hermes，实测 `--input '{"confirm":true,"targets":["pi"]}'`
    /// 照跑不误、**真的改了这台机器的配置**。声明一个不兑现的约束比不声明更坏 ——
    /// 调用方（尤其是照着 manifest 生成入参的 AI）会以为越界会被挡下。
    #[test]
    fn enum_declared_is_enforced() {
        let a = enum_contract_like();
        assert_eq!(
            validate_input(&a.spec, &json!({ "mode": "merge", "confirm": true })),
            Ok(())
        );
        let err = validate_input(&a.spec, &json!({ "mode": "append", "confirm": true }))
            .unwrap_err();
        assert!(err.contains("invalid_input"), "错误码要能被 classify 认出来：{err}");
        assert!(err.contains("merge / replace"), "报错要列出允许值，别让人再翻一次文档：{err}");
    }

    /// 数组元素的 enum 同样要执行 —— 约束写在 `items` 上，只看顶层那个 "array" 等于没看。
    #[test]
    fn enum_on_array_items_is_enforced() {
        let a = enum_contract_like();
        assert_eq!(
            validate_input(&a.spec, &json!({ "targets": ["claude", "pi"], "confirm": true })),
            Ok(())
        );
        let err = validate_input(&a.spec, &json!({ "targets": ["claude", "gemini"], "confirm": true }))
            .unwrap_err();
        assert!(err.contains("targets[1]"), "要指出是第几个元素越界：{err}");
        // 元素类型也一起守住：`["claude", 7]` 顶层是数组、类型「对」，
        // 而 handler 那边 `filter_map(as_str)` 会把 7 静默丢掉 —— 又是一次不报错的错。
        assert!(
            validate_input(&a.spec, &json!({ "targets": ["claude", 7], "confirm": true }))
                .unwrap_err()
                .contains("targets[1]")
        );
    }

    /// 空数组和缺字段都不该被 enum 误伤（`targets` 省略 = 配探到的全部，是老行为）。
    #[test]
    fn enum_does_not_bite_absent_or_empty() {
        let a = enum_contract_like();
        assert_eq!(validate_input(&a.spec, &json!({ "confirm": true })), Ok(()));
        assert_eq!(
            validate_input(&a.spec, &json!({ "targets": [], "confirm": true })),
            Ok(())
        );
        // 可选字段给 null 仍然当「没给」—— 0.9.70 那条铁律不能被 enum 撞翻。
        assert_eq!(
            validate_input(&a.spec, &json!({ "mode": null, "confirm": true })),
            Ok(())
        );
    }

    /// 配方引用了不存在的动作 → 必须判 fail。
    ///
    /// 这条守的是配方层唯一的硬约束：一条 step 指向空动作，比没有这条配方更坏 ——
    /// AI 会照着它去调一个不存在的东西，然后把失败归因到自己身上。
    #[test]
    fn a_recipe_pointing_at_a_missing_action_fails() {
        use std::collections::HashSet;
        let known: HashSet<String> = ["runtime.real.one".to_string()].into_iter().collect();
        let good = Recipe {
            id: "r.good", title: "t", when: "w", preconditions: &[],
            steps: &[Step { action: "runtime.real.one", note: "n" }],
            verify: "v",
        };
        let bad = Recipe {
            id: "r.bad", title: "t", when: "w", preconditions: &[],
            steps: &[Step { action: "runtime.renamed.away", note: "n" }],
            verify: "v",
        };
        let (checks, fail) = check_recipes_against(&[good, bad], &known);
        assert_eq!(fail, 1, "指向不存在动作的配方没被判失败 —— 这条检查形同虚设");
        assert_eq!(checks[0]["status"], "pass");
        assert_eq!(checks[1]["status"], "fail");
        assert_eq!(checks[1]["missing_actions"][0], "runtime.renamed.away");
    }

    /// **打在真配方表上**：仓库里现有的每一条配方，每一步都得指向真实存在的动作。
    /// 上面那条测的是判据本身，这条测的是现实 —— 只有前者的话，改名一个动作照样静默漂移。
    #[test]
    fn every_shipped_recipe_points_at_real_actions() {
        let (checks, fail) = check_recipes();
        assert_eq!(fail, 0, "有配方指向了不存在的动作: {checks:?}");
        assert!(!checks.is_empty(), "配方表空了 —— 说明书里那段会整段消失");
    }

    /// 内部门禁词和 wire enum 可以独立演进：核心继续认 `required`，导出给上游的 0.5
    /// Manifest 必须是 `always`。这条守住「改版本号却忘了映射字段」的半升级状态。
    #[test]
    fn manifest_maps_internal_confirmation_to_the_current_wire_contract() {
        let internal = describe(crate::actions::DRIVER_APPLY).expect("driver action exists");
        assert_eq!(internal.confirmation, "required");

        let exported = manifest();
        // 🔴 必须跟我们**实际校验用的那份 schema** 一致（影核上游至今 0.5.0）。
        // 这里一度写着 `0.6.0` —— 而本用例自己的文档注释写的是「导出给上游的 **0.5**
        // Manifest」，断言和注释自相矛盾却没人发现，因为它只钉数字、不钉理由。
        // 后果是 `action-parity:verify` 长期红着（试点红着 = 规范没有活的验证）。
        assert_eq!(exported["spec_version"], "0.5.0");
        assert_eq!(exported["generated_from"]["generator"], "U-King Action Registry");

        // ★ 比钉版本号更管用的一道闸：**顶层键不许超出 0.5.0 schema 允许的集合**。
        // 上次分叉正是「先加了 `recipes` 顶层段，再顺手把版本号改成 0.6.0」——
        // 只钉版本号拦不住这种「先越界、再改号」。schema 是 additionalProperties:false，
        // 多一个键就是全盘校验失败，所以这里必须是白名单而不是黑名单。
        // 要放新东西，先让上游接受扩展命名空间（见影核 PROPOSAL-MANIFEST-EXTENSIONS），
        // 而不是在这儿加一行。
        const ALLOWED_TOP_LEVEL: &[&str] = &[
            "$schema", "spec_version", "application", "generated_from",
            "conformance_targets", "surfaces", "actions", "state",
        ];
        let extra: Vec<&String> = exported
            .as_object()
            .expect("manifest 是对象")
            .keys()
            .filter(|k| !ALLOWED_TOP_LEVEL.contains(&k.as_str()))
            .collect();
        assert!(
            extra.is_empty(),
            "manifest 多了 0.5.0 schema 不认的顶层键 {extra:?} —— \
             `action-parity:verify` 会当场红（additionalProperties:false）。\
             别改这里的白名单，先去上游要扩展命名空间"
        );
        let driver = exported["actions"]
            .as_array()
            .and_then(|actions| actions.iter().find(|action| action["id"] == DRIVER_APPLY))
            .expect("driver action is exported");
        assert_eq!(driver["effects"]["confirmation"], "always");
        assert!(driver["input_schema"]["properties"].get("confirm").is_none());
        assert!(!driver["input_schema"]["required"]
            .as_array()
            .expect("required is an array")
            .iter()
            .any(|field| field == "confirm"));
    }

    #[test]
    fn generated_action_constants_are_valid_gui_binding_markers() {
        let root = std::env::temp_dir().join(format!("uking-action-bindings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test source directory");
        std::fs::write(
            root.join("App.tsx"),
            r#"<button data-action-id={ACTION.RUNTIME_NETWORK_INSPECT}>Inspect</button>"#,
        )
        .expect("write test source");
        let ids = vec![NETWORK_INSPECT.to_string()];
        let mut found = std::collections::BTreeMap::new();
        let mut stale = Vec::new();
        scan_dir(&root, &ids, &mut found, &mut stale);
        let _ = std::fs::remove_dir_all(&root);

        assert!(found.contains_key(NETWORK_INSPECT));
        assert!(stale.is_empty());
    }

    /// 按声明造一个占位值，给全表扫描用。
    ///
    /// 🔴 **必须先看 `enum`，再看 `type`。** 只按 type 造值时，带 enum 的字符串字段会拿到
    /// `"x"` —— 而 enum 从 0.9.99 起是**真执行**的（在那之前它是装饰，见那次修复），
    /// 于是 `runtime.localllm.start` 这种 `engine: ollama|llamacpp|vllm|sglang` 的动作
    /// 会被判 `invalid_input`，报出来的却是「不收可选字段=null」—— **断言消息指向了错的地方**，
    /// 照它去查会一路查进 null 处理逻辑，而真因在占位符自己身上。
    /// ★ 契约变严之后，围绕契约的测试脚手架也得跟着变严，否则它会以别人的名义报错。
    fn placeholder_for(ps: &Value) -> Value {
        if let Some(first) = ps.get("enum").and_then(Value::as_array).and_then(|a| a.first()) {
            return first.clone();
        }
        match ps.get("type").and_then(Value::as_str) {
            Some("array") => json!([]),
            Some("object") => json!({}),
            Some("boolean") => json!(true),
            Some("integer") | Some("number") => json!(1),
            _ => json!("x"),
        }
    }

    /// **全表守卫**：动作表里每一个动作，把「必填给占位值 + 全部可选字段给 null」喂进去
    /// 都必须过校验。单测一个 driver.apply 只能守住这一次的事故；这条守住的是同一个错
    /// 在第 39 个动作上重演 —— 加动作 = 自动多一条这个断言。
    #[test]
    fn every_optional_field_accepts_null() {
        let specs = list();
        // 防「空跑绿灯」：表要是空的，下面那圈循环一条都不断言，测试照样过。
        assert!(specs.len() > 30, "动作表只有 {} 条，这测试等于没跑", specs.len());
        for spec in specs {
            let Some(schema) = spec.input_schema.clone() else { continue };
            if schema.get("additionalProperties") != Some(&json!(false)) {
                continue;
            }
            let required: Vec<String> = schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|r| r.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default();
            let mut input = serde_json::Map::new();
            if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                for (k, ps) in props {
                    let v = if required.contains(k) {
                        placeholder_for(ps)
                    } else {
                        Value::Null
                    };
                    input.insert(k.clone(), v);
                }
            }
            let input = Value::Object(input);
            assert_eq!(
                validate_input(&spec, &input),
                Ok(()),
                "动作 `{}` 不收「可选字段 = null」，换个调用方就会当场写不进去",
                spec.id
            );
        }
    }
}
