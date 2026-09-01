//! 本地用量统计 / **Token 水电表** —— 读各 AI 编程工具**自己记的会话日志**，
//! 聚合「真实用了多少、什么时候用的、花在哪了」。
//!
//! 覆盖（**每一路都实测过它的口径是「每轮增量」还是「会话累计」**——加错方向整份账就是错的）：
//! - **Claude Code**：`~/.claude/projects/**/*.jsonl`，assistant 消息带 `message.model` +
//!   `message.usage.{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens}`，
//!   行上还有 `timestamp`（UTC ISO）和 `cwd`（哪个项目）。
//!   🔴 **一行 ≠ 一次调用**：回复里每个 content block 各写一行（一段文本 + 三个 tool_use = 四行），
//!   而**每一行都带着整个请求的那份 usage**。必须按 `requestId` 去重，逐行相加就是按 block 数
//!   重复计费 —— 本机 7 天实测 30,613 行只对应 16,273 次真实调用，**虚高 1.84 倍**。
//!   （这个错从水电表第一天起就在，2026-08-16 加逐条流水时才露馅：聚合只给总数，
//!   看不出里面有重复。见 `Scan::seen_requests`。）
//! - **Codex CLI**：`~/.codex/sessions/**/rollout-*.jsonl`，`event_msg` 的 `token_count` 事件带
//!   `info.last_token_usage`（**每轮增量**，按增量累加=会话总量，不能把累计的 total 相加）+ 就近的 model；
//!   `session_meta` 行上带该会话的 `cwd`。
//! - **OpenClaw / ClawX**：`~/.openclaw/agents/*/sessions/*.trajectory.jsonl`，`type=="model.completed"`
//!   的行上**顶层**就有 `ts` / `modelId` / `provider` / `workspaceDir`，`data.usage` =
//!   `{input, output, cacheRead, reasoningTokens, total}`。
//!   🔴 **口径实测**：同一份文件里多个 `model.completed` 的 `runId` **各不相同**，且 `output`
//!   会**下降**（9 份多事件文件里 7 份出现下降）——所以每条是**一个 run 的合计、可加**，
//!   不是会话累计。（`total == input + output + cacheRead`，逐条对过账。）
//!   🔴 **同一行里还有 `assistantTexts` / `finalPromptText` / `messagesSnapshot` 三个正文字段，
//!   一个都不许碰** —— 本模块的红线是只取元数据。
//! - **pi**：`~/.pi/agent/sessions/<编码过的项目目录>/*.jsonl`，`usage` =
//!   `{input, output, cacheRead, cacheWrite, reasoning, totalTokens, cost{...}}`。
//!   🔴 **口径实测**：`input` 在同一会话里非单调（1099→83→17564→172）= **每轮增量、可加**。
//!   目录名是 `--C--Users-x-项目--` 这种编码（同 Claude Code 的 `projects/`），反解出项目路径。
//! - **Hermes**：`<hermes home>/state.db` 的账，优先读 `session_model_usage`（逐调用累加
//!   的真账）JOIN `sessions` 拿时间；老版本没有该表才回退读 `sessions` 主表（少记约20%，
//!   2026-08-27 对账实锤 112 会话无一例外）。按 (会话, 模型) 合并后仍是一行一个
//!   「会话×模型」的合计（`model` / 各 token 列 / `started_at` / `api_call_count`）。**可加**。
//!   🔴 只 select 元数据列 —— 正文列（`system_prompt` / `title`）不碰。
//!   🔴 读法用**便携 Node 的 `node:sqlite`**（同 `uuswitch.rs` 的既有做法），**不给 U-King 加
//!   rusqlite 重依赖**（体积优先）。顺带这也是唯一能正确读 WAL 的办法：实测这台机器上
//!   `state.db` 只有 4KB、`state.db-wal` 有 3.2MB，自己写只读解析器会读到一张空表。
//!   🔴 家目录一律问 `installer::hermes_config_dir()`（全仓唯一真相源，它随 Hermes 版本变过）。
//!   🔴 它自己也算了 `estimated_cost_usd`，但那是按**它认得的那家官方价**（`cost_source=
//!   official_docs_snapshot`）算的，客户走虾盘云时价不同。**只取它的 token 数，钱按本表统一口径
//!   重算** —— 混两套定价出来的总数谁也对不上。
//!
//! **含客户自己的 Key（BYOK）**——读的是 CLI 实际调用记录，跟供应商无关：用虾盘云、用自己的
//! DeepSeek 官方、用官方 Claude，都按模型分开统计。每条带 `tool` 标签。
//!
//! ## 用户说了算：算哪些工具、哪些是包月
//! 偏好存 `~/.uking/usage-tools.json`，两份名单都**只由用户显式勾选写**：
//! - `disabled`：不计入这张表的工具。存「关掉的」而不是「开着的」——以后新接一路数据源时，
//!   存量客户自动纳入，不会因为旧配置里没写而静默漏掉一整个工具。
//! - `subscription`：用户标成**包月订阅**的工具（Claude Pro / ChatGPT Plus 登录着跑）。
//!   这些 token 是真烧了，但钱**不是按 token 付的**，折成 ¥ 就是编数字。
//!   所以订阅工具**照常统计 token、金额一律记 0**，并在报告里明说。
//!   🔴 **不猜**：不拿「驱动=官方直连」去推断订阅——官方直连也可能是自己的 API Key。
//!   这件事只有用户知道，就让用户勾。
//!
//! ## 红线（数据安全）
//! - **只读不写、绝不上传**：纯本地聚合，不发任何网络请求。
//! - **只取元数据**：只累加 token 数 / 模型名 / 时间戳 / 次数 / **项目目录**，
//!   **从不读取或存储 prompt / 消息内容**。项目目录只用于本机分账，同样不出这台机器；
//!   对外输出的 `sources[].dir` 已经把家目录脱敏成 `~`。
//!
//! ## 「水电表」为什么要按天、按项目
//! 一个只给总数的账单回答不了「省了没有」。表要能读出**读数随时间的变化**（今天 / 昨天 /
//! 最近 7 天 / 每日曲线）和**哪一路在耗**（哪个模型、哪个工具、哪个项目），
//! 省 token 的动作才有得对照。所有结论都是**确定性算术**，本地毫秒级算完、离线可用、
//! 不烧一个 token —— 判断在核心，不在界面（宪法第 15 条）。
//!
//! ## 诚实边界
//! 输出里的 `sources` 列的是**本机探测到的全部 AI 工具**，不只是算得到的那几个，
//! 每个都写清楚「算不算得到、为什么」——**报告宁可承认瞎，也不能假装看得见**。
//! 已知**永远算不到**的三类（`countable=false`，不是我们没接，是根本没数据可读）：
//! - **Gemini CLI / Qwen Code**：会话 json 里**压根不写 token**（实测 `.gemini` / `.qwen`
//!   全目录 grep 不到任何 token 字段），装了也无从算起。
//! - **WorkBuddy（腾讯 CodeBuddy）/ Cursor 一类 VS Code 系**：用量藏在 VS Code 的
//!   `state.vscdb` **`secret://` 加密区**（实测有 `CodeBuddy-LLMDataReportCACHE-llm-data`）。
//!   那是凭据存储，**我们不碰** —— 能读也不读，这是红线不是能力问题。
//! - **ChatGPT 官方 / 各家网页版**：账在服务端，而且订阅制根本不按 token 计费；
//!   要统计只能靠登录态抓取，越界。
//!
//! ## 独立可插拔（守设计取舍铁律）
//! 只暴露纯函数、不碰 AppHandle；`#[tauri::command]` 写在 lib.rs 转调。删本模块只动 2 个文件
//! （lib.rs 去 mod+command+动作登记、前端去调用）。纯 std + serde_json，不引第三方 crate。
//! 加工具（如 Gemini CLI）= 在本文件加一个 `scan_xxx`，不牵动别处。

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 家目录（支持 UKING_TEST_HOME 沙箱，与其它模块同口径）。
fn home_dir() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
}

fn claude_projects_dir() -> PathBuf {
    home_dir().join(".claude").join("projects")
}

fn codex_sessions_dir() -> PathBuf {
    home_dir().join(".codex").join("sessions")
}

/// 一个（工具, 模型）的聚合用量。
#[derive(Serialize, Clone)]
pub struct LocalUsageItem {
    pub model: String,
    /// 哪个工具用的：`claude` / `codex`。
    pub tool: String,
    /// 估算花费（人民币，按公开报价折算，仅供参考——本地日志不含真实计费）。
    pub cny: f64,
    /// 调用次数（assistant 消息 / token_count 事件条数）。
    pub count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 缓存读。**必须跟非缓存输入分开报**：缓存读比非缓存输入便宜约一个数量级，
    /// 混在一起算会得出错误的省钱结论 —— Token 压缩机的「净收益」算不准就是这个根子。
    /// `Acc` 一直在累加它，只是以前没往外暴露。加 `serde(default)` 让老前端不炸。
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// 缓存写（cache creation）。
    #[serde(default)]
    pub cache_write_tokens: u64,
}

/// 一条省钱建议。
///
/// **判断在核心，不在界面**（宪法第 15 条）：这些是确定性的算术结论，
/// 本地毫秒级就能算完、离线可用、不烧一个 token —— 没有任何理由把它丢给大模型再算一遍。
/// GUI、CLI、MCP 拿到的是同一份，AI 想追问开放式问题再拿数据去问。
#[derive(Serialize)]
pub struct UsageTip {
    /// 稳定 id（前端配图标、测试拿它断言）。
    pub id: &'static str,
    /// 一句话结论。
    pub title: String,
    /// 具体怎么做。
    pub detail: String,
    /// 预估每月能省多少（¥）。**0 = 算不准，就别编一个数**。
    pub saving_cny: f64,
}

/// 本地用量总表（形状对齐前端 UsageBreakdown，另带总计头）。
#[derive(Serialize)]
pub struct LocalUsage {
    pub days: i64,
    pub total_cny: f64,
    pub total_calls: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub items: Vec<LocalUsageItem>,
    /// 数据来源标记，前端据此显示「本地实际用量（含你自己的 Key）」。
    pub source: &'static str,
    /// 本地算出来的省钱建议（可能为空 = 没什么好建议的，那就别硬凑）。
    pub tips: Vec<UsageTip>,
}

/// 内部累加器（统一口径：非缓存输入 / 缓存读 / 缓存写 / 输出）。
#[derive(Default, Clone)]
struct Acc {
    non_cached_input: u64,
    cache_read: u64,
    cache_creation: u64,
    output: u64,
    count: u64,
}

impl Acc {
    fn add(&mut self, o: &Acc) {
        self.non_cached_input += o.non_cached_input;
        self.cache_read += o.cache_read;
        self.cache_creation += o.cache_creation;
        self.output += o.output;
        self.count += o.count;
    }
    fn input_tokens(&self) -> u64 {
        self.non_cached_input + self.cache_read + self.cache_creation
    }
    fn tokens(&self) -> u64 {
        self.input_tokens() + self.output
    }
}

/// 扫描桶：一次扫描把每条记录归到（本地日期, 工具, 模型, 项目）四元组上。
///
/// **先折叠再定价**——按最细的桶各自定价再相加会引入四舍五入漂移，
/// 而「按模型」「按天」「按项目」三张表必须能对得上同一个总数。
/// 所有视图都是从这一份桶折出来的，不存在第二次统计（宪法第 8 条：同一事实只有一份）。
#[derive(Hash, PartialEq, Eq, Clone)]
struct Bucket {
    /// 本地日期 `YYYY-MM-DD`（不是 UTC —— 日志里是 UTC，差 8 小时会让「今天」整段错位）。
    date: String,
    tool: String,
    model: String,
    /// 完整工作目录；空字符串 = 日志里没写。
    project: String,
}

/// 一条**逐条流水**（扫描时顺手留下的原始记录，还没折算成钱）。
///
/// 为什么要单独收一份：聚合键 `Bucket` 只到 `{天, 工具, 模型, 项目}` —— 精确时间戳
/// 进不去，会话身份更进不去。日志里每一次调用都从眼前流过，我们只留了总数，于是
/// 「这笔钱是哪一轮花的」在**扫描那一瞬间**就永久丢了，事后再怎么查表也补不回来。
///
/// 🔴 **只在调用方要明细时才收集**（`Scan::collect_events`）。30 天窗口下 Claude Code
/// 一路就可能有几万条，默认路径不该为一个可选视图背这份内存。
struct RawEvent {
    /// 本地 epoch 秒。只知道哪天的（见 `exact`）给当天 0 点，好让排序仍然稳定。
    epoch: i64,
    /// 时间戳精确到秒吗。pi 的行时间戳键名不固定，拿不到就退回文件名里的日期 ——
    /// 那种只知道「哪天」。**表要敢说自己哪里糊**，不能把 00:00 当成真发生在半夜。
    exact: bool,
    tool: &'static str,
    model: String,
    project: String,
    input: u64,
    cache_read: u64,
    cache_write: u64,
    output: u64,
    /// 这一条代表几次模型调用。
    /// 🔴 **Hermes 一行是一整个会话**（它的 `sessions` 表就是这个粒度，没有逐轮记录），
    /// 可能是几十次调用的合计。不带这个数就会让人把一整天的会话读成"一轮"。
    calls: u64,
    /// 这一条是**会话合计**而非单次调用（只有 Hermes 是 true）。前端据此明说。
    session_rollup: bool,
}

#[derive(Default)]
struct Scan {
    buckets: HashMap<Bucket, Acc>,
    /// 逐条流水。`collect_events == false` 时永远是空的。
    events: Vec<RawEvent>,
    collect_events: bool,
    lines: u64,
    /// 每一路扫到几个文件 / 几条会话（`sources` 里如实回显，让人能判断「是没用过还是没扫到」）。
    files: HashMap<String, u64>,
    /// 这次真跑了哪几路（被用户关掉的不在里面）。
    counted: Vec<String>,
    /// 某一路想算但没算成的原因（如 Hermes 缺 Node）。
    failed: HashMap<String, String>,
    /// 已经计过账的 Claude Code `requestId`。
    ///
    /// 🔴 **一次 API 调用会被写成好几行 assistant 消息**（回复里每个 content block 一行：
    /// 一段文本 + 三个 tool_use = 四行），而**每一行都带着整个请求的 `message.usage`**。
    /// 逐行累加 = 同一笔钱按 block 数量重复记 —— 本机 7 天实测 **30,613 行只对应 16,273 次
    /// 真实调用，token 虚高 1.84 倍（45.7% 是重复的）**。
    ///
    /// 这个坑从水电表第一天起就在，聚合把它藏住了：只看总数看不出重复，是**逐条流水**
    /// 一列出来才露的馅（同一秒、同一份 usage、同一个 requestId 连着三条）。
    /// ★ 教训：一个只给总数的指标，连"它自己算错了"都表现不出来。
    ///
    /// 按 requestId 去重（Anthropic 分配，全局唯一，两次真实调用不可能撞）。
    /// 没有这个字段的行（老版本 Claude Code，实测占 9%）照计 —— 宁可少去重，不能误杀真实调用。
    seen_requests: std::collections::HashSet<String>,
}

impl Scan {
    fn bump(&mut self, tool: &str) {
        *self.files.entry(tool.to_string()).or_default() += 1;
    }

    /// 顺手留一条流水。**关掉明细时是空操作**，五路扫描器无脑调用即可 ——
    /// 把「要不要收」的判断集中在这一处，比在五个地方各写一个 if 少一处漏。
    #[allow(clippy::too_many_arguments)]
    fn event(&mut self, e: RawEvent) {
        if self.collect_events {
            self.events.push(e);
        }
    }
}

/// 定价上下文 —— 哪些工具被用户标成了「包月订阅」。
///
/// 包月的 token 是**真烧了**（所以照常统计），但钱**不是按 token 付的**：
/// 拿 API 列表价折成 ¥ 记进「花了多少」就是编数字，客户对着账单会发现全对不上。
/// 所以订阅工具金额一律 0，报告里另说明。宪法：宁可承认瞎，不能假装看得见。
struct Pricing {
    subscription: std::collections::HashSet<String>,
}

impl Pricing {
    fn of(prefs: &UsagePrefs) -> Self {
        Pricing { subscription: prefs.subscription.iter().cloned().collect() }
    }
    /// 不四舍五入 —— 多个桶要相加时必须用这个（同 `estimate_cny_raw` 的理由）。
    fn raw(&self, tool: &str, model: &str, a: &Acc) -> f64 {
        if self.subscription.contains(tool) {
            0.0
        } else {
            estimate_cny_raw(model, a)
        }
    }
}

/// 读本地日志，聚合最近 `days` 天的按（工具, 模型）用量。
///
/// 时间窗口两道闸：**文件 mtime 粗筛**（跳过整个陈旧文件，省 IO）+ **逐行时间戳精筛**
/// （一个长会话文件可能横跨窗口边界，只按 mtime 会把窗口外的量算进来）。
/// 跑很多文件，lib.rs 以 spawn_blocking 转调别卡 UI。
/// `squeezer_active` = Token 压缩机此刻真的在生效吗。
///
/// **由组合根注入，不在这里 import rtk** —— 功能模块之间禁止互相依赖（设计取舍铁律②），
/// 否则删 rtk 模块就得连着改这里。`lib.rs` 知道两边，让它传。
pub fn breakdown(days: i64, squeezer_active: bool) -> LocalUsage {
    let prefs = read_prefs();
    let pricing = Pricing::of(&prefs);
    // `breakdown` 只回答「按模型花了多少」，不出流水 —— 不收 events。
    let scan = scan_all(days, &prefs, false);
    let items = fold_by_model(&scan, &pricing);

    let total_cny = round2(items.iter().map(|i| i.cny).sum::<f64>());
    let total_calls = items.iter().map(|i| i.count).sum();
    let total_input_tokens = items.iter().map(|i| i.input_tokens).sum();
    let total_output_tokens = items.iter().map(|i| i.output_tokens).sum();

    let tips = build_tips(days, total_cny, &items, squeezer_active);
    LocalUsage {
        days,
        total_cny,
        total_calls,
        total_input_tokens,
        total_output_tokens,
        items,
        source: "local",
        tips,
    }
}

// ── Token 水电表 ─────────────────────────────────────────────────────────────────

/// 一段区间的读数（水电表的「表盘」）。
#[derive(Serialize, Default, Clone)]
pub struct Totals {
    pub cny: f64,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 总 token = 输入 + 输出。**这就是「水电表读数」**，单位统一，跨模型可加。
    pub tokens: u64,
}

/// 某一天的读数（曲线上的一个点）。没用的那天照样出一个 0 点 ——
/// 缺口本身是信息（那天没干活），跳过它会让曲线撒谎。
#[derive(Serialize)]
pub struct DayPoint {
    pub date: String,
    pub cny: f64,
    pub tokens: u64,
    pub calls: u64,
}

/// 按某个维度（工具 / 项目）的分账。
#[derive(Serialize)]
pub struct NamedTotals {
    /// 展示名（工具名 / 项目文件夹名）。
    pub name: String,
    /// 项目的完整路径（工具行为空）。只在本机展示，不出这台机器。
    pub detail: String,
    pub cny: f64,
    pub tokens: u64,
    pub calls: u64,
    /// 占总花费的比例 0~1。
    pub share: f64,
}

/// 缓存账：AI 编程里最大的一笔隐形省钱（也是最大的一笔隐形浪费）。
#[derive(Serialize, Default)]
pub struct CacheStats {
    pub non_cached_input: u64,
    /// 命中缓存读进去的（便宜，约原价 1/10）。
    pub cache_read: u64,
    /// 写缓存的（比原价贵 25%，写了没命中就是纯亏）。
    pub cache_creation: u64,
    /// 命中率 = 缓存读 / 全部输入。越高越省。
    pub hit_rate: f64,
    /// **缓存已经替你省下的钱**（这些 token 若按原价算要多花多少）。可核对，不是估的折扣。
    pub saved_cny: f64,
}

/// 用得快不快、还能用多久。
#[derive(Serialize, Default)]
pub struct Pace {
    pub daily_avg_cny: f64,
    /// 按日均折成一个月（30 天）。
    pub month_projection_cny: f64,
    /// 今天花的是日均的几倍（没有日均就 0）。
    pub today_vs_avg: f64,
    /// 余额还能撑几天。**没给余额就是 null，不猜**。
    pub days_left: Option<f64>,
    pub balance_cny: Option<f64>,
}

/// 一条**流水**：一次模型调用花了多少（Hermes 那路是一个会话，见 `session_rollup`）。
///
/// 为什么要有它：聚合表答得了「这个月花了 ¥x」「哪个项目在耗」，答不了
/// **「这笔钱是哪一轮花的」**。而客户盯着余额下降时想问的恰恰是后者
/// （#401 原话「10 元很快就用完了」）—— 一张只给总数的账单没法用来复盘。
#[derive(Serialize)]
pub struct UsageEvent {
    /// 本地时间。`exact_time=false` 时只有 `YYYY-MM-DD`（不知道几点，就不编一个）。
    pub ts: String,
    /// 本地 epoch 秒 —— 前端要自己排序/分组时用，别再去解析上面那个串。
    pub epoch: i64,
    /// 时间戳精确到分吗。false = 只知道是哪天。
    pub exact_time: bool,
    pub tool: String,
    pub tool_label: String,
    pub model: String,
    /// 项目展示名（文件夹名）。空 = 这一路日志里没记工作目录。
    pub project: String,
    /// 已脱敏（家目录换 `~`）的完整路径。只在本机展示，同样不出这台机器。
    pub project_dir: String,
    /// 这一条代表几次模型调用。
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// 输入 + 输出 + 缓存读写 —— 跟表盘那个「读数」同一口径。
    pub tokens: u64,
    /// 按**水电表那份唯一价表**折算（订阅工具恒 0，同聚合表）。
    pub cny: f64,
    /// 🔴 true = 这一条是**一整个会话的合计**，不是一次调用（只有 Hermes 是）。
    /// 界面必须把这件事说出来，否则 ¥3.7 会被读成「这一轮花了 3.7 元」。
    pub session_rollup: bool,
}

/// 流水那一块的元信息 —— **表要敢说自己给的是不是全部**。
#[derive(Serialize)]
pub struct EventsMeta {
    /// 窗口内一共有多少条（截断前）。
    pub total: u64,
    /// 实际返回了多少条。
    pub returned: u64,
    /// 被截掉了多少条（`total - returned`）。>0 时界面要明说「只列了最近 N 条」，
    /// 🔴 悄悄截断会让人把「最近 200 条的合计」当成「这个月的合计」。
    pub truncated: u64,
    /// 这些返回条目的合计花费（**只是这几条**，不是窗口总额）。
    pub returned_cny: f64,
}

/// 一路数据源的覆盖情况 —— 表要敢说自己哪里瞎。
///
/// 列的是**本机探测到的全部 AI 工具**，不只是算得到的那几个：客户看到总数时得能一眼判断
/// 「这是全部，还是只是我装的工具里的一部分」。少列一个，总数就在骗人。
#[derive(Serialize)]
pub struct SourceStatus {
    pub tool: String,
    pub label: String,
    /// 已脱敏（家目录换成 `~`）的日志目录。
    pub dir: String,
    /// 这台机器上装了/用过它吗。
    pub exists: bool,
    /// **我们读不读得到它的 token 账**（跟装没装无关）。false = 再怎么勾也算不出来。
    pub countable: bool,
    /// 用户勾了要算它吗（`countable=false` 的工具这里恒为 false —— 不给一个点了没用的勾）。
    pub enabled: bool,
    /// 用户标了它是**包月订阅**吗（token 照算，金额记 0）。
    pub subscription: bool,
    /// 这次**真的**算进上面的数字了吗 = exists && countable && enabled && 没扫失败。
    pub covered: bool,
    pub files: u64,
    pub note: String,
}

// ── 用户偏好：算哪些工具、哪些是包月 ─────────────────────────────────────────────────

/// `~/.uking/usage-tools.json`。两份名单都只由用户显式勾选写，没有任何自动补种路径。
#[derive(Debug, Default, Clone, Serialize, serde::Deserialize)]
pub struct UsagePrefs {
    /// 不计入这张表的工具 id。**存「关掉的」而不是「开着的」**：以后新接一路数据源，
    /// 存量客户自动纳入，不会因为旧配置里没写就静默漏掉一整个工具的账。
    #[serde(default)]
    pub disabled: Vec<String>,
    /// 用户标成「包月订阅」的工具 id（Claude Pro / ChatGPT Plus 登录着跑）。
    #[serde(default)]
    pub subscription: Vec<String>,
}

impl UsagePrefs {
    fn on(&self, tool: &str) -> bool {
        !self.disabled.iter().any(|d| d == tool)
    }
}

fn prefs_path() -> PathBuf {
    home_dir().join(".uking").join("usage-tools.json")
}

/// 读偏好（不存在/坏了都返回默认 = 全开、无订阅，绝不让水电表罢工）。
pub fn read_prefs() -> UsagePrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 写偏好（原子写：temp + rename）。只认真实存在的工具 id，打错的静默丢掉不如当场拒。
pub fn write_prefs(p: &UsagePrefs) -> Result<(), String> {
    let known: Vec<&str> = TOOL_CATALOG.iter().map(|t| t.id).collect();
    for id in p.disabled.iter().chain(p.subscription.iter()) {
        if !known.contains(&id.as_str()) {
            return Err(format!("未知的工具 id「{id}」（只认 {}）", known.join(" / ")));
        }
    }
    let path = prefs_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 .uking 目录失败: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(p).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("写 usage-tools.json 失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 usage-tools.json 失败: {e}"))
}

/// 本机可能装着的 AI 工具全表 —— **算得到的和算不到的都在这里**。
///
/// `countable=false` 的那几个不是「我们还没接」，是**根本没有本地账可读**（见模块头「诚实边界」）。
/// 把它们也列出来，是因为客户装了 5 个工具却只看到 2 个的账时，必须知道那 3 个去哪了。
struct ToolDef {
    id: &'static str,
    label: &'static str,
    countable: bool,
    /// 算不到时给客户的人话解释（`countable=true` 的留空）。
    why: &'static str,
}

const TOOL_CATALOG: &[ToolDef] = &[
    ToolDef { id: "claude", label: "Claude Code", countable: true, why: "" },
    ToolDef { id: "codex", label: "Codex CLI", countable: true, why: "" },
    ToolDef { id: "openclaw", label: "OpenClaw / ClawX", countable: true, why: "" },
    ToolDef { id: "hermes", label: "Hermes", countable: true, why: "" },
    ToolDef { id: "pi", label: "pi", countable: true, why: "" },
    ToolDef {
        id: "gemini",
        label: "Gemini CLI / Antigravity",
        countable: false,
        why: "它的会话文件里压根不写 token 数，本机无账可读 —— 不是没接，是没有数据。",
    },
    ToolDef {
        id: "qwen",
        label: "Qwen Code",
        countable: false,
        why: "同 Gemini CLI：会话文件里不写 token 数，无账可读。",
    },
    ToolDef {
        id: "crush",
        label: "Crush",
        countable: false,
        why: "用量在它自己的 crush.db 里，格式未公开且随版本变，暂不接。",
    },
    ToolDef {
        id: "workbuddy",
        label: "WorkBuddy / 腾讯 CodeBuddy",
        countable: false,
        why: "用量存在 VS Code 的加密凭据区（secret://）。那是密钥存储，**我们不碰** —— 这是红线，不是能力问题。",
    },
    ToolDef {
        id: "cherrystudio",
        label: "CherryStudio",
        countable: false,
        why: "桌面版把记录存在自己的浏览器数据库里，格式随版本变，暂不接。",
    },
    ToolDef {
        id: "chatgpt",
        label: "ChatGPT 桌面版 / 网页版",
        countable: false,
        why: "账在 OpenAI 服务器上，本机没有；而且它是包月订阅，本来就不按 token 计费。要统计只能拿你的登录态去抓，那越界了。",
    },
];

/// 每个工具在本机的「装没装」判据 + 展示用目录。只看**用过的痕迹**（数据目录），
/// 不看安装目录 —— 装了没用过的工具列出来只会让人以为漏算了。
fn tool_dir(id: &str) -> PathBuf {
    let h = home_dir();
    match id {
        "claude" => claude_projects_dir(),
        "codex" => codex_sessions_dir(),
        "openclaw" => h.join(".openclaw").join("agents"),
        "hermes" => crate::installer::hermes_config_dir(),
        "pi" => h.join(".pi").join("agent").join("sessions"),
        "gemini" => h.join(".gemini"),
        "qwen" => h.join(".qwen"),
        "crush" => h.join(".crush"),
        "workbuddy" => appdata_roaming().join("WorkBuddy"),
        "cherrystudio" => appdata_roaming().join("CherryStudio"),
        "chatgpt" => appdata_local().join("OpenAI"),
        _ => h,
    }
}

/// 🔴 **这两个也要认沙箱**（2026-08-18 补）。原来它们只在 env 缺失时才退到 `home_dir()`，
/// 而 `UKING_TEST_HOME` 沙箱下 `APPDATA` / `LOCALAPPDATA` 照样存在 —— 于是「沙箱化」的测试
/// 会直接指向真实 AppData。
///
/// 这跟当天删掉真实数据的那次是**同一个形状**：`skillpack.rs::legacy_skill_parents()`
/// 就是这么读 `LOCALAPPDATA` 的，结果删了用户真实的 hermes 技能目录。
/// 本文件目前只读不删所以没出事 —— 但闸门那句原话就是
/// 「只读时看不出问题，加第一个 remove 的那天会删掉真实数据」。形状先改掉，别等它长出删除。
///
/// **它是被修好的扫描器抓出来的**：旧的正则剥注释把这几行藏了很久（0 → 4 处）。
fn sandboxed_appdata(var: &str, sub: &str) -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.trim().is_empty() {
            return PathBuf::from(t).join("AppData").join(sub);
        }
    }
    std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| home_dir())
}

fn appdata_roaming() -> PathBuf {
    sandboxed_appdata("APPDATA", "Roaming")
}

fn appdata_local() -> PathBuf {
    sandboxed_appdata("LOCALAPPDATA", "Local")
}

/// Token 水电表总表。
#[derive(Serialize)]
pub struct Meter {
    pub days: i64,
    /// 这张表在这台机器上**读得出东西吗**（宪法：readiness 回答「能不能用」，不是「装没装」）。
    pub ready: bool,
    pub blockers: Vec<String>,
    /// 整个窗口的合计。
    pub window: Totals,
    pub today: Totals,
    pub yesterday: Totals,
    pub last7: Totals,
    pub daily: Vec<DayPoint>,
    pub by_model: Vec<LocalUsageItem>,
    pub by_tool: Vec<NamedTotals>,
    pub by_project: Vec<NamedTotals>,
    pub cache: CacheStats,
    pub pace: Pace,
    pub tips: Vec<UsageTip>,
    pub sources: Vec<SourceStatus>,
    pub source: &'static str,
    /// 逐条流水（最近的在前）。**只在调用方要明细时才有**（`detail > 0`）——
    /// 默认不带，省得每次读表都背几万条。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<UsageEvent>,
    /// 流水的元信息（有没有被截断）。不要明细时是 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_meta: Option<EventsMeta>,
}

/// 读一次水电表。
///
/// `balance_cny` = 账上还剩多少钱（人民币）。**本模块不发网络请求**（只读动作不该联网），
/// 余额由组合根/调用方传进来；给了才算「还能用几天」，没给就是 `null`——不猜。
/// `detail` = 最多返回几条逐条流水（0 = 不要明细，默认路径）。上限由调用方的契约管。
pub fn meter(days: i64, squeezer_active: bool, balance_cny: Option<f64>, detail: usize) -> Meter {
    let days = days.clamp(1, 365);
    let prefs = read_prefs();
    let pricing = Pricing::of(&prefs);
    let scan = scan_all(days, &prefs, detail > 0);
    let by_model = fold_by_model(&scan, &pricing);
    let window_cny = round2(by_model.iter().map(|i| i.cny).sum::<f64>());

    // ── 按天 ──
    let mut day_acc: HashMap<String, Acc> = HashMap::new();
    // 定价要按模型来，所以按（日期, 工具, 模型）折一层再定价，最后并到日期上。
    let mut day_model: HashMap<(String, String), Acc> = HashMap::new();
    for (b, a) in &scan.buckets {
        day_acc.entry(b.date.clone()).or_default().add(a);
        day_model.entry((b.date.clone(), b.model.clone())).or_default().add(a);
    }
    let mut day_cny: HashMap<String, f64> = HashMap::new();
    for ((date, model), a) in &day_model {
        *day_cny.entry(date.clone()).or_default() += estimate_cny_raw(model, a);
    }

    let today = local_today();
    let dates = date_window(&today, days);
    let daily: Vec<DayPoint> = dates
        .iter()
        .map(|d| {
            let a = day_acc.get(d).cloned().unwrap_or_default();
            DayPoint {
                date: d.clone(),
                cny: round2(day_cny.get(d).copied().unwrap_or(0.0)),
                tokens: a.tokens(),
                calls: a.count,
            }
        })
        .collect();

    let totals_of = |wanted: &[String]| -> Totals {
        let mut acc = Acc::default();
        let mut cny = 0.0;
        for d in wanted {
            if let Some(a) = day_acc.get(d) {
                acc.add(a);
            }
            cny += day_cny.get(d).copied().unwrap_or(0.0);
        }
        Totals {
            cny: round2(cny),
            calls: acc.count,
            input_tokens: acc.input_tokens(),
            output_tokens: acc.output,
            tokens: acc.tokens(),
        }
    };
    let today_t = totals_of(&[today.clone()]);
    let yesterday_t = totals_of(&[shift_date(&today, -1)]);
    let last7_t = totals_of(&date_window(&today, 7));
    let window_t = Totals {
        cny: window_cny,
        calls: by_model.iter().map(|i| i.count).sum(),
        input_tokens: by_model.iter().map(|i| i.input_tokens).sum(),
        output_tokens: by_model.iter().map(|i| i.output_tokens).sum(),
        tokens: by_model.iter().map(|i| i.input_tokens + i.output_tokens).sum(),
    };

    // ── 按工具 / 按项目 ──
    let by_tool = fold_named(&scan, &pricing, window_cny, |b| (tool_label(&b.tool).to_string(), String::new()));
    let by_project = fold_named(&scan, &pricing, window_cny, |b| {
        if b.project.is_empty() {
            ("（日志里没写项目）".into(), String::new())
        } else {
            (project_name(&b.project), b.project.clone())
        }
    });

    // ── 缓存账 ──
    let mut cache = CacheStats::default();
    let mut cache_saved = 0.0;
    let mut model_acc: HashMap<String, Acc> = HashMap::new();
    for (b, a) in &scan.buckets {
        cache.non_cached_input += a.non_cached_input;
        cache.cache_read += a.cache_read;
        cache.cache_creation += a.cache_creation;
        model_acc.entry(b.model.clone()).or_default().add(a);
    }
    for (model, a) in &model_acc {
        // 缓存读按输入价的 0.1 计（见 estimate_cny）。若这些 token 全走原价，多出来的就是省下的。
        let (in_rate, _) = price_per_million(model);
        cache_saved += (a.cache_read as f64 / 1e6) * in_rate * 0.9;
    }
    let total_in = cache.non_cached_input + cache.cache_read + cache.cache_creation;
    cache.hit_rate = if total_in > 0 { cache.cache_read as f64 / total_in as f64 } else { 0.0 };
    cache.saved_cny = round2(cache_saved);

    // ── 用得快不快 ──
    let daily_avg = if days > 0 { window_cny / days as f64 } else { 0.0 };
    let pace = Pace {
        daily_avg_cny: round2(daily_avg),
        month_projection_cny: round2(daily_avg * 30.0),
        today_vs_avg: if daily_avg > 0.0 { round2(today_t.cny / daily_avg) } else { 0.0 },
        days_left: match balance_cny {
            Some(b) if daily_avg > 0.0 && b >= 0.0 => Some(round2(b / daily_avg)),
            _ => None,
        },
        balance_cny,
    };

    // ── 覆盖面 + readiness ──
    let sources = build_sources(&scan, &prefs);
    let covered_any = sources.iter().any(|s| s.covered && s.files > 0);
    let mut blockers = Vec::new();
    if !covered_any {
        // 三种「读不到」得分开说 —— 混成一句话，客户不知道该去装什么、还是去打开哪个开关。
        let installed_uncountable: Vec<&str> =
            sources.iter().filter(|s| s.exists && !s.countable).map(|s| s.label.as_str()).collect();
        let turned_off: Vec<&str> =
            sources.iter().filter(|s| s.exists && s.countable && !s.enabled).map(|s| s.label.as_str()).collect();
        if !turned_off.is_empty() {
            blockers.push(format!("你在「数据来源」里关掉了：{}。打开就会算进来。", turned_off.join(" / ")));
        }
        if !installed_uncountable.is_empty() {
            blockers.push(format!(
                "这台机器上装着 {}，但它们**没有可读的本地 token 账**（原因见「数据来源」逐条说明），谁也算不出来。",
                installed_uncountable.join(" / ")
            ));
        }
        if turned_off.is_empty() && installed_uncountable.is_empty() {
            blockers.push("没读到任何会话日志：这台机器最近还没用过能统计的 AI 工具。".into());
        }
    }
    // 想算却没算成的（如 Hermes 缺 Node）——**必须单独喊出来**，否则它就是一笔悄悄少掉的账。
    for (tool, why) in &scan.failed {
        blockers.push(format!("{}：{why}", tool_label(tool)));
    }

    let mut tips = build_tips(days, window_cny, &by_model, squeezer_active);
    tips.extend(build_meter_tips(days, &cache, &by_project, &pace, window_cny));

    let (events, events_meta) = fold_events(scan.events, &pricing, detail);

    Meter {
        days,
        ready: covered_any,
        blockers,
        window: window_t,
        today: today_t,
        yesterday: yesterday_t,
        last7: last7_t,
        daily,
        by_model,
        by_tool,
        by_project,
        cache,
        pace,
        tips,
        sources,
        source: "local",
        events,
        events_meta,
    }
}

/// 原始流水 → 对外流水：**最近的在前**，按 `limit` 截断，钱走跟聚合表同一条路。
///
/// 🔴 截断必须**先排序再截**，且把截掉多少如实报出来（`EventsMeta.truncated`）。
/// 「只列了最近 200 条」和「这个月一共就这些」在界面上长得一模一样 ——
/// 差别只在有没有人说出来（宪法：宁可承认瞎，不能假装看得见）。
fn fold_events(mut raw: Vec<RawEvent>, pricing: &Pricing, limit: usize) -> (Vec<UsageEvent>, Option<EventsMeta>) {
    if limit == 0 {
        return (Vec::new(), None);
    }
    let total = raw.len() as u64;
    // 时间倒序；同一秒内保持扫描顺序稳定（sort_by 是稳定排序）
    raw.sort_by(|a, b| b.epoch.cmp(&a.epoch));
    raw.truncate(limit);

    let events: Vec<UsageEvent> = raw
        .into_iter()
        .map(|e| {
            // 走 `pricing.raw` 而不是自己乘价表：订阅工具恒 0 这条规则只该有一处实现，
            // 否则流水会给一个聚合表说是 0 的花费编出金额来。
            let acc = Acc {
                non_cached_input: e.input,
                cache_read: e.cache_read,
                cache_creation: e.cache_write,
                output: e.output,
                count: e.calls,
            };
            let cny = pricing.raw(e.tool, &e.model, &acc);
            UsageEvent {
                ts: if e.exact { datetime_string(e.epoch) } else { date_string(e.epoch) },
                epoch: e.epoch,
                exact_time: e.exact,
                tool: e.tool.to_string(),
                tool_label: tool_label(e.tool).to_string(),
                project: if e.project.is_empty() { String::new() } else { project_name(&e.project) },
                project_dir: if e.project.is_empty() {
                    String::new()
                } else {
                    redact_home(Path::new(&e.project))
                },
                model: e.model,
                calls: e.calls,
                input_tokens: e.input,
                output_tokens: e.output,
                cache_read_tokens: e.cache_read,
                cache_write_tokens: e.cache_write,
                tokens: e.input + e.output + e.cache_read + e.cache_write,
                cny: round2(cny),
                session_rollup: e.session_rollup,
            }
        })
        .collect();

    let returned = events.len() as u64;
    let meta = EventsMeta {
        total,
        returned,
        truncated: total.saturating_sub(returned),
        returned_cny: round2(events.iter().map(|e| e.cny).sum::<f64>()),
    };
    (events, Some(meta))
}

/// 把桶折成「按模型」的明细（和 0.9.63 起的 `breakdown` 口径逐字节一致）。
fn fold_by_model(scan: &Scan, pricing: &Pricing) -> Vec<LocalUsageItem> {
    let mut agg: HashMap<(String, String), Acc> = HashMap::new();
    for (b, a) in &scan.buckets {
        agg.entry((b.tool.clone(), b.model.clone())).or_default().add(a);
    }
    let mut items: Vec<LocalUsageItem> = agg
        .into_iter()
        .map(|((tool, model), a)| LocalUsageItem {
            cny: round2(pricing.raw(&tool, &model, &a)),
            count: a.count,
            input_tokens: a.input_tokens(),
            output_tokens: a.output,
            cache_read_tokens: a.cache_read,
            cache_write_tokens: a.cache_creation,
            model,
            tool,
        })
        .collect();
    // 按花费降序（花得最多的排前面）。
    items.sort_by(|x, y| y.cny.partial_cmp(&x.cny).unwrap_or(std::cmp::Ordering::Equal));
    items
}

/// 按任意维度折一张分账表。`key` 返回 (展示名, 详情)。
fn fold_named(
    scan: &Scan,
    pricing: &Pricing,
    total_cny: f64,
    key: impl Fn(&Bucket) -> (String, String),
) -> Vec<NamedTotals> {
    // 同样是「先折叠再定价」：先按 (维度, 工具, 模型) 折，定完价再并到维度上。
    // **工具也得进 key** —— 定价要看它是不是被标了包月（同一个模型在包月工具里是 0 元，
    // 在按量工具里不是；不分开折就会把两边混成一个价）。
    let mut per: HashMap<String, (String, Acc, f64)> = HashMap::new();
    let mut per_model: HashMap<(String, String, String), Acc> = HashMap::new();
    for (b, a) in &scan.buckets {
        let (name, detail) = key(b);
        let e = per.entry(name.clone()).or_insert_with(|| (detail, Acc::default(), 0.0));
        e.1.add(a);
        per_model.entry((name, b.tool.clone(), b.model.clone())).or_default().add(a);
    }
    for ((name, tool, model), a) in &per_model {
        if let Some(e) = per.get_mut(name) {
            e.2 += pricing.raw(tool, model, a);
        }
    }
    let mut rows: Vec<NamedTotals> = per
        .into_iter()
        .map(|(name, (detail, a, cny))| NamedTotals {
            name,
            detail,
            cny: round2(cny),
            tokens: a.tokens(),
            calls: a.count,
            share: if total_cny > 0.0 { cny / total_cny } else { 0.0 },
        })
        .collect();
    rows.sort_by(|x, y| y.cny.partial_cmp(&x.cny).unwrap_or(std::cmp::Ordering::Equal));
    rows
}

/// 工具 id → 展示名。单一来源就是 [`TOOL_CATALOG`]，别在别处再抄一份。
fn tool_label(tool: &str) -> &str {
    TOOL_CATALOG.iter().find(|t| t.id == tool).map(|t| t.label).unwrap_or(tool)
}

/// 取工作目录的最后一段当项目名（`D:\work\my-app` → `my-app`）。
fn project_name(dir: &str) -> String {
    dir.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(dir)
        .to_string()
}

/// 把家目录换成 `~`，别把用户名写进任何可能被拿去排障的输出里。
fn redact_home(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let h = home_dir().to_string_lossy().replace('\\', "/");
    if !h.is_empty() && s.starts_with(&h) {
        format!("~{}", &s[h.len()..])
    } else {
        s
    }
}

/// 各路数据源的覆盖情况 —— **本机探测到的全部 AI 工具**，算得到的和算不到的都列。
///
/// 「表上没有」和「没有用量」是两件完全不同的事；再加上「你自己关掉了」和「它根本没账」
/// 又是两件不同的事。四种状态必须各说各的，否则客户只会得出一个「这表不准」的结论，
/// 然后再也不看它。
fn build_sources(scan: &Scan, prefs: &UsagePrefs) -> Vec<SourceStatus> {
    TOOL_CATALOG
        .iter()
        .map(|t| {
            let dir = tool_dir(t.id);
            let exists = dir.exists();
            let enabled = t.countable && prefs.on(t.id);
            let subscription = prefs.subscription.iter().any(|s| s == t.id);
            let files = scan.files.get(t.id).copied().unwrap_or(0);
            let failed = scan.failed.get(t.id);
            let covered = exists && enabled && failed.is_none();
            let note = if !t.countable {
                t.why.to_string()
            } else if let Some(why) = failed {
                why.clone()
            } else if !prefs.on(t.id) {
                "你把它关掉了，**没有算进上面的数字**。想算就在这里打开。".into()
            } else if !exists {
                "还没在这台机器上用过（没有会话记录）。".into()
            } else if subscription {
                "已标为**包月订阅**：token 照常统计，但**金额记 0** —— 包月不按 token 付费，折成钱就是编数字。".into()
            } else {
                String::new()
            };
            SourceStatus {
                tool: t.id.into(),
                label: t.label.into(),
                dir: redact_home(&dir),
                exists,
                countable: t.countable,
                enabled,
                subscription,
                covered,
                files,
                note,
            }
        })
        .collect()
}

/// 只探测「本机装了哪些 AI 工具、各自算不算得到」——**不扫日志**，给设置界面画勾选列表用。
/// 毫秒级返回（只 stat 几个目录），跟真正的扫描分开：开个设置页不该等几百 MB 日志扫完。
pub fn detect_sources() -> Vec<SourceStatus> {
    build_sources(&Scan::default(), &read_prefs())
}

// ── 省钱建议（纯算术，不猜）───────────────────────────────────────────────────────

/// 便宜档模型的参考单价（¥/百万 token）。用 deepseek 系当基准 —— 国产、够用、便宜，
/// 也是 U-King 默认推的那档。换算出来的「能省多少」是**同样的 token 换个模型跑**，
/// 不是拍脑袋的折扣。
const CHEAP_IN: f64 = 2.0;
const CHEAP_OUT: f64 = 8.0;

/// 从聚合结果里算出建议。每条要么给得出可核对的数，要么就不给数。
fn build_tips(days: i64, total_cny: f64, items: &[LocalUsageItem], squeezer_active: bool) -> Vec<UsageTip> {
    let mut tips = Vec::new();
    // 花得太少（几毛钱）时任何建议都是噪音，直接不给。
    if total_cny < 1.0 || items.is_empty() {
        return tips;
    }
    let per_month = |v: f64| if days > 0 { v * 30.0 / days as f64 } else { v };

    // ① 贵模型占大头 —— 同样的 token 换便宜档要多少钱，差额就是能省的。
    //    只对**确实贵**的模型提（便宜模型自己跟自己比没意义）。
    let top = &items[0];
    if is_premium(&top.model) && top.cny >= total_cny * 0.5 {
        let cheap = (top.input_tokens as f64 / 1e6) * CHEAP_IN + (top.output_tokens as f64 / 1e6) * CHEAP_OUT;
        let save = per_month(top.cny - cheap);
        if save > 1.0 {
            tips.push(UsageTip {
                id: "switch_cheap_model",
                title: format!("{} 占了你 {:.0}% 的花费", top.model, top.cny / total_cny * 100.0),
                // **先说倍数再说钱**：倍数是稳的（同一批 token，只换单价），
                // 绝对金额继承了「按公开列表价折算」这个假设 —— 包月用户、走虾盘云的客户
                // 实际单价都不一样。把不稳的那个数标清楚出处，别让它冒充账单。
                detail: format!(
                    "同样这些 token 换成 deepseek 这类国产便宜档，花费只有约 1/{:.0}（¥{:.2} vs ¥{:.2}，按公开报价估算）。\
                     日常改代码、跑命令用便宜档，硬骨头再切回来。",
                    if cheap > 0.0 { top.cny / cheap } else { 1.0 },
                    cheap,
                    top.cny
                ),
                saving_cny: round2(save),
            });
        }
    }

    // ② 输出 token 占比高 —— 输出单价普遍是输入的 4~5 倍，让 AI 少啰嗦最直接。
    let out_cost: f64 = items.iter().map(|i| (i.output_tokens as f64 / 1e6) * price_per_million(&i.model).1).sum();
    if out_cost >= total_cny * 0.45 {
        tips.push(UsageTip {
            id: "shorter_replies",
            title: format!("{:.0}% 的钱花在 AI 的「输出」上", out_cost / total_cny * 100.0),
            detail: "输出单价通常是输入的 4~5 倍。在 CLAUDE.md 里加一句「直接给结果，不复述、不解释」，\
                     省的是最贵的那部分。"
                .into(),
            saving_cny: 0.0,
        });
    }

    // ③ 压缩机没开 —— 只在真有 Claude Code 用量时提，且**不编一个精确数字**：
    //    hook 只压 Bash 命令的输出，占输入的多少因人而异，给个假精度不如不给。
    if !squeezer_active && items.iter().any(|i| i.tool == "claude") {
        tips.push(UsageTip {
            id: "enable_squeezer",
            title: "Token 压缩机还没在生效".into(),
            detail: "它把 AI 跑命令后那些啰嗦输出压扁再喂给模型，报错和 diff 一个不丢。\
                     去「Token 压缩机」页开一下，那页有现场演示可以先看效果。"
                .into(),
            saving_cny: 0.0,
        });
    }

    tips
}

/// 水电表独有的建议（要有按天/按项目/缓存这些维度才算得出来）。
fn build_meter_tips(days: i64, cache: &CacheStats, by_project: &[NamedTotals], pace: &Pace, total_cny: f64) -> Vec<UsageTip> {
    let mut tips = Vec::new();
    if total_cny < 1.0 {
        return tips;
    }

    // ① 缓存一直在重建 = 每轮都在重新灌上下文。**不给省钱金额**：
    //    能挽回多少完全取决于你后续还会不会命中，编一个数就是假精度。
    let total_in = cache.non_cached_input + cache.cache_read + cache.cache_creation;
    if total_in > 200_000 && cache.hit_rate < 0.35 {
        tips.push(UsageTip {
            id: "cache_cold",
            title: format!("上下文缓存命中率只有 {:.0}%", cache.hit_rate * 100.0),
            detail: "缓存命中的输入只按原价约 1/10 收费，写缓存反而贵 25%——一直重建就是一直在多花钱。\
                     常见原因：每次都开新会话（用 /resume 接着上次聊）、频繁改 CLAUDE.md（一改全部作废）、\
                     来回切模型（各自一套缓存）。"
                .into(),
            saving_cny: 0.0,
        });
    }

    // ② 某个项目吃掉大头 —— 纯信息，但这是「该在哪儿动手」的第一个指路牌。
    if let Some(top) = by_project.first() {
        if top.share >= 0.4 && by_project.len() > 1 && !top.detail.is_empty() {
            tips.push(UsageTip {
                id: "top_project",
                title: format!("「{}」一个项目占了 {:.0}% 的花费", top.name, top.share * 100.0),
                detail: format!(
                    "这个项目 {days} 天内花了约 ¥{:.2}。要省钱先从它下手：这类大头项目最容易靠\
                     「给它单独配便宜档模型 + 把重复问的东西写进 CLAUDE.md」立竿见影。",
                    top.cny
                ),
                saving_cny: 0.0,
            });
        }
    }

    // ③ 今天特别猛 —— 只在确实是今天、且倍数明显时提，别天天报警。
    if pace.today_vs_avg >= 2.5 && pace.daily_avg_cny > 0.2 {
        tips.push(UsageTip {
            id: "today_spike",
            title: format!("今天的用量是日均的 {:.1} 倍", pace.today_vs_avg),
            detail: "不一定有问题（可能就是今天活多）。但如果你没觉得干了更多活，\
                     多半是某个会话在反复重灌大上下文——去「按项目」那一栏看是哪个。"
                .into(),
            saving_cny: 0.0,
        });
    }

    // ④ 余额要见底 —— 有余额才算，没余额不猜。
    if let (Some(left), Some(_)) = (pace.days_left, pace.balance_cny) {
        if left < 7.0 {
            tips.push(UsageTip {
                id: "balance_low",
                title: format!("按这个速度，余额大约还能用 {:.0} 天", left.max(0.0)),
                detail: "按最近的日均花费推算的。想撑久一点：先把上面那条「换便宜档」做掉，\
                         效果比省着用明显得多。"
                    .into(),
                saving_cny: 0.0,
            });
        }
    }

    tips
}

/// 是不是「贵档」模型（换掉最有省钱空间的那批）。
fn is_premium(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    ["opus", "sonnet", "gpt-5", "gpt5", "codex", "o1", "o3", "grok", "gpt-4"]
        .iter()
        .any(|k| m.contains(k))
}

// ── 扫描 ──────────────────────────────────────────────────────────────────────────

/// 扫一遍**用户勾选要算的**各路日志，产出最细粒度的桶。所有视图都从这一份折出来。
///
/// 没勾的一路都不扫（省 IO），并且在 `sources` 里如实标成「你关掉了」——
/// 关掉和算不到是两回事，混成一句话客户就分不清该去装什么还是该去打开开关。
fn scan_all(days: i64, prefs: &UsagePrefs, collect_events: bool) -> Scan {
    let days = days.clamp(1, 365);
    // mtime 粗筛多放宽一天：一个会话文件可能昨天写的、今天才落最后一行。
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs((days as u64 + 1) * 86_400));
    // 逐行精筛的下界（本地日期，字符串比较即可 —— ISO 日期天然可比）。
    let from_date = shift_date(&local_today(), -(days - 1));

    let mut scan = Scan { collect_events, ..Default::default() };
    // 安全阀：极端历史下别无限跑（正常远达不到）。
    const MAX_LINES: u64 = 3_000_000;

    for t in TOOL_CATALOG.iter().filter(|t| t.countable && prefs.on(t.id)) {
        scan.counted.push(t.id.to_string());
        let dir = tool_dir(t.id);
        match t.id {
            "claude" => scan_claude(&dir, &cutoff, &from_date, &mut scan, MAX_LINES),
            "codex" => scan_codex(&dir, &cutoff, &from_date, &mut scan, MAX_LINES),
            "openclaw" => scan_openclaw(&dir, &cutoff, &from_date, &mut scan, MAX_LINES),
            "pi" => scan_pi(&dir, &cutoff, &from_date, &mut scan, MAX_LINES),
            "hermes" => scan_hermes(days, &from_date, &mut scan),
            _ => {}
        }
    }
    scan
}

// ── Claude Code：~/.claude/projects/**/*.jsonl ────────────────────────────────────

fn scan_claude(dir: &Path, cutoff: &Option<SystemTime>, from_date: &str, scan: &mut Scan, max_lines: u64) {
    walk_recent(dir, cutoff, max_lines, scan, &mut |path, scan| {
        scan.bump("claude");
        for_each_line(path, max_lines, scan, &mut |line, scan| {
            scan.lines += 1;
            // 便宜的预筛：要被计入的行**必然**同时含 `assistant`（type）和 `usage`（token 字段）。
            // 缺任一就绝无可能命中，直接跳过，省掉一次完整 JSON 解析 —— 会话日志里绝大多数行
            // 是用户消息和工具结果，全解析纯属白烧 CPU（30 天窗口实测 445MB）。
            // 误判方向是安全的：偶尔多解析一行（正文里恰好出现这两个词），照样被下面的判断挡掉。
            if !line.contains("assistant") || !line.contains("usage") {
                return;
            }
            let Ok(j) = serde_json::from_str::<Value>(line) else {
                return;
            };
            if j.get("type").and_then(|t| t.as_str()) != Some("assistant") {
                return;
            }
            let Some(msg) = j.get("message") else { return };
            let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("");
            // 跳过 Claude Code 内部合成消息（错误/中断占位，非真实 API 调用）
            if model.is_empty() || model == "<synthetic>" {
                return;
            }
            let Some(epoch) = local_epoch_of(j.get("timestamp").and_then(|t| t.as_str())) else {
                return;
            };
            let date = date_string(epoch);
            if date.as_str() < from_date {
                return;
            }
            let u = msg.get("usage");
            let get = |k: &str| u.and_then(|u| u.get(k)).and_then(|v| v.as_u64()).unwrap_or(0);
            let input = get("input_tokens");
            let output = get("output_tokens");
            let cache_read = get("cache_read_input_tokens");
            let cache_creation = get("cache_creation_input_tokens");
            if input + output + cache_read + cache_creation == 0 {
                return;
            }
            // 🔴 同一次 API 调用被拆成多行、每行都带整份 usage —— 只认第一行（见 `seen_requests`）。
            // 没有 requestId 的老行照计：宁可少去重，不能把真实调用误杀成重复。
            if let Some(rid) = j.get("requestId").and_then(|v| v.as_str()) {
                if !scan.seen_requests.insert(rid.to_string()) {
                    return;
                }
            }
            let project = j.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
            scan.event(RawEvent {
                epoch,
                exact: true,
                tool: "claude",
                model: model.to_string(),
                project: project.clone(),
                input,
                cache_read,
                cache_write: cache_creation,
                output,
                calls: 1,
                session_rollup: false,
            });
            let e = scan
                .buckets
                .entry(Bucket { date, tool: "claude".into(), model: model.to_string(), project })
                .or_default();
            e.non_cached_input += input;
            e.output += output;
            e.cache_read += cache_read;
            e.cache_creation += cache_creation;
            e.count += 1;
        });
    });
}

// ── Codex CLI：~/.codex/sessions/**/rollout-*.jsonl ───────────────────────────────

fn scan_codex(dir: &Path, cutoff: &Option<SystemTime>, from_date: &str, scan: &mut Scan, max_lines: u64) {
    // Codex 的 model 是「跟踪当前会话正在用的模型」——按行推进，token_count 归给当前 model。
    // cwd 同理：只在开头的 session_meta 里出现一次，整份文件共用。
    // 每个文件独立跟踪（会话不跨文件），所以在文件粒度重置。
    walk_recent(dir, cutoff, max_lines, scan, &mut |path, scan| {
        scan.bump("codex");
        let mut cur_model = String::from("codex");
        let mut cwd = String::new();
        for_each_line(path, max_lines, scan, &mut |line, scan| {
            scan.lines += 1;
            // 同 Claude 侧的预筛。Codex 这边要留三类行：token_count 事件（真正计数的）、
            // 任何带 model 的行（跟踪「当前会话在用哪个模型」）、以及带 cwd 的 session_meta。
            if !line.contains("token_count") && !line.contains("model") && !line.contains("cwd") {
                return;
            }
            let Ok(j) = serde_json::from_str::<Value>(line) else {
                return;
            };
            if let Some(m) = extract_codex_model(&j) {
                cur_model = m;
            }
            if cwd.is_empty() {
                if let Some(c) = j.get("payload").and_then(|p| p.get("cwd")).and_then(|c| c.as_str()) {
                    cwd = c.to_string();
                }
            }
            if j.get("payload").and_then(|p| p.get("type")).and_then(|t| t.as_str()) != Some("token_count") {
                return;
            }
            let lt = j
                .get("payload")
                .and_then(|p| p.get("info"))
                .and_then(|i| i.get("last_token_usage"));
            let get = |k: &str| lt.and_then(|l| l.get(k)).and_then(|v| v.as_u64()).unwrap_or(0);
            let input = get("input_tokens");
            let cached = get("cached_input_tokens");
            let output = get("output_tokens") + get("reasoning_output_tokens");
            if input + output == 0 {
                return;
            }
            let Some(epoch) = local_epoch_of(j.get("timestamp").and_then(|t| t.as_str())) else {
                return;
            };
            let date = date_string(epoch);
            if date.as_str() < from_date {
                return;
            }
            scan.event(RawEvent {
                epoch,
                exact: true,
                tool: "codex",
                model: cur_model.clone(),
                project: cwd.clone(),
                // 同下：input_tokens 含缓存部分，这里也得拆，否则流水每行都比聚合表多算一份
                input: input.saturating_sub(cached),
                cache_read: cached,
                cache_write: 0,
                output,
                calls: 1,
                session_rollup: false,
            });
            let e = scan
                .buckets
                .entry(Bucket { date, tool: "codex".into(), model: cur_model.clone(), project: cwd.clone() })
                .or_default();
            // Codex 的 input_tokens 含缓存部分；拆出非缓存 + 缓存读，与 Claude 口径统一。
            e.non_cached_input += input.saturating_sub(cached);
            e.cache_read += cached;
            e.output += output;
            e.count += 1;
        });
    });
}

// ── OpenClaw / ClawX：~/.openclaw/agents/*/sessions/*.trajectory.jsonl ────────────
//
// 🔴 口径实测（本机 16 份 trajectory）：每个 `model.completed` 的 `runId` **各不相同**，
// 且 `output` 会下降（9 份多事件文件里 7 份下降）——每条 = **一个 run 的合计，可加**。
// 要是当成会话累计去取最后一条，就会把前面所有 run 的量整个丢掉。
//
// 🔴 同一行里躺着 `assistantTexts` / `finalPromptText` / `messagesSnapshot` 三个正文字段。
// 我们**只取 `data.usage` 的四个数**，正文一个字节都不进内存以外的任何地方。
fn scan_openclaw(dir: &Path, cutoff: &Option<SystemTime>, from_date: &str, scan: &mut Scan, max_lines: u64) {
    walk_recent(dir, cutoff, max_lines, scan, &mut |path, scan| {
        // 只认 trajectory —— 同目录下还有别的 jsonl，扫了也白扫。
        if !path.to_string_lossy().contains(".trajectory.") {
            return;
        }
        scan.bump("openclaw");
        for_each_line(path, max_lines, scan, &mut |line, scan| {
            scan.lines += 1;
            // 预筛（同 Claude 侧的理由）：要计入的行必然同时含这两个词。
            if !line.contains("model.completed") || !line.contains("usage") {
                return;
            }
            let Ok(j) = serde_json::from_str::<Value>(line) else { return };
            if j.get("type").and_then(|t| t.as_str()) != Some("model.completed") {
                return;
            }
            let model = j.get("modelId").and_then(|m| m.as_str()).unwrap_or("");
            if model.is_empty() {
                return;
            }
            let Some(epoch) = local_epoch_of(j.get("ts").and_then(|t| t.as_str())) else { return };
            let date = date_string(epoch);
            if date.as_str() < from_date {
                return;
            }
            let u = j.get("data").and_then(|d| d.get("usage"));
            let get = |k: &str| u.and_then(|u| u.get(k)).and_then(|v| v.as_u64()).unwrap_or(0);
            let input = get("input");
            let cache_read = get("cacheRead");
            let cache_write = get("cacheWrite");
            // reasoningTokens 实测**已含在 output 里**（total == input + output + cacheRead 逐条对过账），
            // 再加一次就是重复计费。
            let output = get("output");
            if input + output + cache_read + cache_write == 0 {
                return;
            }
            let project = j.get("workspaceDir").and_then(|c| c.as_str()).unwrap_or("").to_string();
            // OpenClaw 这一条 = **一个 run 的合计**（runId 各不相同，口径见本节顶部注释），
            // 已经是它能给到的最细粒度，不是会话累计。
            scan.event(RawEvent {
                epoch,
                exact: true,
                tool: "openclaw",
                model: model.to_string(),
                project: project.clone(),
                input,
                cache_read,
                cache_write,
                output,
                calls: 1,
                session_rollup: false,
            });
            let e = scan
                .buckets
                .entry(Bucket { date, tool: "openclaw".into(), model: model.to_string(), project })
                .or_default();
            e.non_cached_input += input;
            e.output += output;
            e.cache_read += cache_read;
            e.cache_creation += cache_write;
            e.count += 1;
        });
    });
}

// ── pi：~/.pi/agent/sessions/<编码过的项目目录>/*.jsonl ────────────────────────────
//
// 🔴 口径实测：同一会话里 `input` 非单调（1099→83→17564→172）= **每轮增量、可加**。
fn scan_pi(dir: &Path, cutoff: &Option<SystemTime>, from_date: &str, scan: &mut Scan, max_lines: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        if scan.lines >= max_lines {
            return;
        }
        let proj_dir = ent.path();
        if !proj_dir.is_dir() {
            continue;
        }
        // 目录名是编码过的项目路径（`--C--Users-x-proj--`），解回来给「按项目」那张表用。
        let project = decode_pi_project(&ent.file_name().to_string_lossy());
        walk_recent(&proj_dir, cutoff, max_lines, scan, &mut |path, scan| {
            scan.bump("pi");
            for_each_line(path, max_lines, scan, &mut |line, scan| {
                scan.lines += 1;
                if !line.contains("usage") || !line.contains("totalTokens") {
                    return;
                }
                let Ok(j) = serde_json::from_str::<Value>(line) else { return };
                // usage 可能挂在行上，也可能在 message 里（两种形态都见过）。
                let u = j.get("usage").or_else(|| j.get("message").and_then(|m| m.get("usage")));
                let Some(u) = u else { return };
                let get = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                let input = get("input");
                let output = get("output");
                let cache_read = get("cacheRead");
                let cache_write = get("cacheWrite");
                if input + output + cache_read + cache_write == 0 {
                    return;
                }
                let model = j
                    .get("model")
                    .or_else(|| j.get("message").and_then(|m| m.get("model")))
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                if model.is_empty() {
                    return;
                }
                // pi 的行时间戳键名不固定，拿不到就退回文件名里的 ISO 时间（目录里就是这么命名的）。
                let line_ts = j
                    .get("timestamp")
                    .or_else(|| j.get("ts"))
                    .and_then(|t| t.as_str())
                    .map(String::from);
                // 🔴 退回文件名那条路只知道**哪天**（`pi_date_from_name` 拼的是 `T00:00:00Z`，
                // 那个零点是凑出来的，不是真发生的时刻）。流水里必须标成「只知道日期」——
                // 否则一整天的调用会全部显示成 08:00，看着精确其实是我们编的。
                let exact = line_ts.is_some();
                let ts = line_ts.or_else(|| pi_date_from_name(path));
                let Some(raw_epoch) = local_epoch_of(ts.as_deref()) else { return };
                let date = date_string(raw_epoch);
                if date.as_str() < from_date {
                    return;
                }
                // 不精确的归到当天本地 0 点：排序仍稳定，又不会伪装成某个具体时刻
                let epoch = if exact { raw_epoch } else { epoch_of_date(&date) };
                scan.event(RawEvent {
                    epoch,
                    exact,
                    tool: "pi",
                    model: model.to_string(),
                    project: project.clone(),
                    input,
                    cache_read,
                    cache_write,
                    output,
                    calls: 1,
                    session_rollup: false,
                });
                let e = scan
                    .buckets
                    .entry(Bucket { date, tool: "pi".into(), model: model.to_string(), project: project.clone() })
                    .or_default();
                e.non_cached_input += input;
                e.output += output;
                e.cache_read += cache_read;
                e.cache_creation += cache_write;
                e.count += 1;
            });
        });
    }
}

/// `--C--Users-me-Desktop-proj--` → `C:/Users/me/Desktop/proj`。
///
/// 只求「按项目分账」那张表上的名字对得上，**不保证能还原出真实存在的路径**：
/// 原始路径里的 `-` 和分隔符编码后无法区分，硬还原会把 `my-app` 拆成 `my/app`。
/// 所以只还原盘符，其余照原样留着 —— 宁可显示得糙一点，也不显示一个错的路径。
fn decode_pi_project(name: &str) -> String {
    let s = name.trim_matches('-');
    // 开头的 `C--` 是盘符
    if let Some(rest) = s.strip_prefix("C--").or_else(|| s.strip_prefix("c--")) {
        return format!("C:/{rest}");
    }
    s.to_string()
}

/// 从 pi 的会话文件名里取日期（`2026-08-04T09-02-39-170Z_<id>.jsonl`）。
fn pi_date_from_name(path: &Path) -> Option<String> {
    let n = path.file_name()?.to_str()?;
    let d = n.get(0..10)?;
    if d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-' {
        // 拼成 local_epoch_of 认得的 ISO；时间给个 00:00 是**凑数的**，调用方据此把
        // exact 标成 false（见 scan_pi），别让它冒充一个真实发生的时刻
        Some(format!("{d}T00:00:00.000Z"))
    } else {
        None
    }
}

// ── Hermes：<hermes home>/state.db 的账（优先 session_model_usage 真账，回退 sessions 主表）
//
// 🔴 用**便携 Node 的 `node:sqlite`** 读（同 uuswitch.rs 的既有做法），不加 rusqlite 重依赖。
// 顺带这也是唯一能正确读 WAL 的办法 —— 实测 state.db 4KB / state.db-wal 3.2MB，
// 自己写只读页解析器会读到一张空表，然后理直气壮地报「Hermes 没用量」。
//
// 🔴 只 select 元数据列。同一张表里还有 `system_prompt` / `title`，那是正文，不取。
fn scan_hermes(days: i64, from_date: &str, scan: &mut Scan) {
    let db = crate::installer::hermes_config_dir().join("state.db");
    if !db.is_file() {
        return;
    }
    let Some(node) = find_node() else {
        scan.failed.insert(
            "hermes".into(),
            "要读它的 state.db 得有 Node ≥22.5（用其内置 node:sqlite）—— 这台机器上没找到，所以 Hermes 的用量**没有算进上面的数字**。装一次 Node 即可。".into(),
        );
        return;
    };
    // 🔴 `--eval` 模式下 `process.argv` 是 `[node.exe, 第一个用户参数, ...]` ——
    // **没有脚本路径那一项**，所以用户参数从 `argv[1]` 起，不是普通脚本的 `argv[2]`。
    // 照普通脚本的下标写会拿数字去当数据库路径开，然后报一句和真实原因八竿子打不着的
    // 「unable to open database file」（实测踩过）。
    //
    // 🔴 数据源优先用 `session_model_usage`（逐调用累加的**真账**），JOIN 回 `sessions`
    // 拿 started_at / 兜底过滤；按 (session_id, model) 分组合并，行粒度与旧行一致
    // （一行 = 一个会话的一个模型的合计）。直接读 `sessions` 主表会系统性少记约 20%
    // —— 主表的 token 列是快照式落盘，会话后半段（尤其长会话压缩后续跑）的增量
    // 不一定回写；2026-08-27 全量对账实锤：112 个近期会话里主表 ≤ 真账无一例外，
    // 净差 +3.12 亿 token。
    // 真账表不存在（老版本 Hermes）时原样回退读 sessions 主表——宁可维持现状，不读挂。
    let js = r#"import { DatabaseSync } from "node:sqlite";
const db = new DatabaseSync(process.argv[1], { readOnly: true });
const since = Number(process.argv[2]);
let rows;
try {
  // 只取元数据列 —— system_prompt / title 是正文，一列都不碰。
  rows = db.prepare(
    "select sm.model as model, s.started_at as started_at, " +
    "sum(sm.input_tokens) as input_tokens, sum(sm.output_tokens) as output_tokens, " +
    "sum(sm.cache_read_tokens) as cache_read_tokens, sum(sm.cache_write_tokens) as cache_write_tokens, " +
    "sum(sm.api_call_count) as api_call_count " +
    "from session_model_usage sm join sessions s on s.id = sm.session_id " +
    "where s.started_at >= ? group by sm.session_id, sm.model"
  ).all(since);
} catch (e) {
  // 🔴 只有「表不存在」（老版本 Hermes）才允许回退旧口径——那是版本差异，不是故障。
  // 锁冲突/损坏等其他错误必须如实报错非零退出：Rust 端会把这一路标成失败
  // （「没有算进上面的数字」），宁可缺数也不静默掉回旧口径造成约20%低报（外审实抓）。
  const msg = String((e && e.message) || e);
  if (!/no such table/i.test(msg)) {
    process.stderr.write("usage-meter: " + msg);
    process.exit(3);
  }
  rows = db.prepare(
    "select model, started_at, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, api_call_count " +
    "from sessions where started_at >= ?"
  ).all(since);
}
process.stdout.write(JSON.stringify(rows));
"#;
    // 时间下界给宽一天（同 mtime 粗筛的理由：边界会话）。started_at 是 unix 秒（浮点）。
    let since = (now_secs() - (days + 1) * 86_400).max(0);
    let out = match run_node_json(&node, js, &[&db.to_string_lossy(), &since.to_string()]) {
        Ok(v) => v,
        Err(e) => {
            scan.failed.insert("hermes".into(), format!("读它的 state.db 失败（{e}），**没有算进上面的数字**。"));
            return;
        }
    };
    let Some(rows) = out.as_array() else { return };
    scan.bump("hermes");
    for r in rows {
        let get = |k: &str| r.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        let input = get("input_tokens");
        let output = get("output_tokens");
        let cache_read = get("cache_read_tokens");
        let cache_write = get("cache_write_tokens");
        if input + output + cache_read + cache_write == 0 {
            continue;
        }
        let model = r.get("model").and_then(|m| m.as_str()).unwrap_or("");
        if model.is_empty() {
            continue;
        }
        let started = r.get("started_at").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let epoch = started as i64 + local_offset_secs();
        let date = date_string(epoch);
        if date.as_str() < from_date {
            continue;
        }
        // 一行是一整个会话；用它自己记的调用次数，没有就按 1 次算（别把会话数说成调用数）。
        let calls = get("api_call_count").max(1);
        // 🔴 **这一条流水不是"一次调用"，是一整个会话的合计** —— Hermes 的 `sessions` 表
        // 就是这个粒度，它没有逐轮记录，我们变不出来。`session_rollup` 让界面明说这件事：
        // 不说的话，一条 ¥3.7 的 hermes 记录会被读成"这一轮花了 3.7 元"，
        // 而它可能是 40 轮的合计。**表要敢说自己哪里糙。**
        scan.event(RawEvent {
            epoch,
            // started_at 是会话**开始**的时刻，精确；但它代表的区间可能横跨几小时。
            exact: true,
            tool: "hermes",
            model: model.to_string(),
            project: String::new(),
            input,
            cache_read,
            cache_write,
            output,
            calls,
            session_rollup: true,
        });
        // Hermes 的 sessions 表没有 cwd —— 它分不到项目，`project` 留空（表上显示「未知项目」）。
        let e = scan
            .buckets
            .entry(Bucket { date, tool: "hermes".into(), model: model.to_string(), project: String::new() })
            .or_default();
        e.non_cached_input += input;
        e.output += output;
        e.cache_read += cache_read;
        e.cache_creation += cache_write;
        // 一行是一整个会话；用它自己记的调用次数，没有就按 1 次算（别把会话数说成调用数）。
        e.count += get("api_call_count").max(1);
    }
}

/// 便携 Node（`~/.uking/runtime/node`）优先，否则系统 node。找不到就 None ——
/// 调用方据此如实说「这一路没算进来、以及为什么」，绝不静默当成 0。
fn find_node() -> Option<String> {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    let cand = home_dir().join(".uking").join("runtime").join("node").join(exe);
    if cand.exists() {
        return Some(cand.to_string_lossy().into_owned());
    }
    // 系统 node：真跑一下确认存在（PATH 里有没有不能靠猜）。
    let mut c = std::process::Command::new(exe);
    c.arg("--version");
    no_window(&mut c);
    match c.output() {
        Ok(o) if o.status.success() => Some(exe.to_string()),
        _ => None,
    }
}

#[cfg(windows)]
fn no_window(c: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(not(windows))]
fn no_window(_c: &mut std::process::Command) {}

/// 跑一段内联 node 脚本收 JSON。**带硬超时**——只读动作绝不能挂死在子进程上
/// （宪法第 9 条：凡会卡的一律超时）。
fn run_node_json(node: &str, script: &str, args: &[&str]) -> Result<Value, String> {
    use std::io::Read;
    let mut c = std::process::Command::new(node);
    c.args(["--experimental-sqlite", "--input-type=module", "--eval", script, "--"]);
    c.args(args);
    c.stdout(std::process::Stdio::piped());
    c.stderr(std::process::Stdio::null());
    no_window(&mut c);
    let mut child = c.spawn().map_err(|e| format!("起不来 node: {e}"))?;
    // 10 秒足够读一张会话表；超了就杀掉，宁可这一路缺数据也不让整张表卡住。
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("超时（10 秒）".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("等 node 退出失败: {e}")),
        }
    }
    let mut buf = String::new();
    if let Some(mut so) = child.stdout.take() {
        let _ = so.read_to_string(&mut buf);
    }
    serde_json::from_str(&buf).map_err(|e| format!("输出不是合法 JSON: {e}"))
}

/// 从 Codex 一行里就近取 model（payload.model / turn_context.model / info.model）。
/// 只认字符串值——JSON Schema 定义里的 `model` 是对象（`{"type":"string"}`），`as_str()` 自动过滤掉。
fn extract_codex_model(j: &Value) -> Option<String> {
    let p = j.get("payload")?;
    let cand = p
        .get("model")
        .or_else(|| p.get("turn_context").and_then(|t| t.get("model")))
        .or_else(|| p.get("info").and_then(|t| t.get("model")))
        .and_then(|m| m.as_str())?;
    if cand.is_empty() {
        None
    } else {
        Some(cand.to_string())
    }
}

// ── 遍历助手（时间窗口 = 文件 mtime 粗筛）────────────────────────────────────────

/// 递归找近 `cutoff` 内改动的 .jsonl，对每个**文件**回调。
fn walk_recent(
    dir: &Path,
    cutoff: &Option<SystemTime>,
    max_lines: u64,
    scan: &mut Scan,
    on_file: &mut dyn FnMut(&Path, &mut Scan),
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        if scan.lines >= max_lines {
            return;
        }
        let p = ent.path();
        let Ok(ft) = ent.file_type() else { continue };
        if ft.is_dir() {
            walk_recent(&p, cutoff, max_lines, scan, on_file);
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(cut) = cutoff {
            if let Ok(modt) = ent.metadata().and_then(|m| m.modified()) {
                if modt < *cut {
                    continue;
                }
            }
        }
        on_file(&p, scan);
    }
}

/// 逐行读一个 jsonl。两个刻意的选择，都是实测逼出来的：
///
/// 1. **流式读，不 `read_to_string`** —— 30 天窗口下这些日志有几百 MB，
///    整文件读进 String 是白白多一次几百 MB 的分配和拷贝。
/// 2. **`read_until` + 复用缓冲区，不用 `BufReader::lines()`** —— `lines()` 每行都
///    `String::new()` 一次。实测（352MB 真实日志夹具）用 `lines()` 比原来的
///    `read_to_string` **慢了近一倍**：per-line 分配的开销直接盖过预筛省下的解析。
///    复用一个 buf 才两头都占到。
fn for_each_line(path: &Path, max_lines: u64, scan: &mut Scan, on_line: &mut dyn FnMut(&str, &mut Scan)) {
    use std::io::BufRead;
    let Ok(f) = std::fs::File::open(path) else { return };
    // 会话日志单行可以很长（整段工具输出），缓冲区给大一点少几次 syscall。
    let mut rd = std::io::BufReader::with_capacity(256 * 1024, f);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    loop {
        if scan.lines >= max_lines {
            return;
        }
        buf.clear();
        match rd.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        // 非 UTF-8 的行直接跳过（日志本该是 UTF-8；坏行不该让整份统计罢工）。
        let Ok(line) = std::str::from_utf8(&buf) else { continue };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        on_line(line, scan);
    }
}

// ── 日期（纯 std，不引日期库）────────────────────────────────────────────────────
//
// 日志里的时间戳全是 **UTC**（`2026-07-31T15:34:07.221Z`），但水电表必须按**本地日期**
// 分桶 —— 在东八区，UTC 日期会让 00:00~08:00 的用量整段算到「昨天」，
// 「今天花了多少」当场就是错的。std 没有本地时区，所以自己取一次系统的偏移量。
//
// 偏移只取一次并套用到整个窗口：跨夏令时切换的那一天会有 1 小时误差。
// 中国无夏令时；其它时区这点误差不影响「哪天用得多」的判断，不值得为它引一个日期库。

/// 本机时区相对 UTC 的偏移（秒）。拿不到就当 UTC（0）。
#[cfg(windows)]
fn local_offset_secs() -> i64 {
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
    let local_sod = (t.hour as i64) * 3600 + (t.minute as i64) * 60 + t.second as i64;
    let utc_sod = now_secs().rem_euclid(86_400);
    let mut d = local_sod - utc_sod;
    // 归一到 (-12h, +14h]：偏移量的真实取值范围。
    if d <= -12 * 3600 {
        d += 86_400;
    }
    if d > 14 * 3600 {
        d -= 86_400;
    }
    d
}

#[cfg(not(windows))]
fn local_offset_secs() -> i64 {
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
            return 0;
        }
        tm.gmtoff
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 把日志里的 UTC ISO 时间戳换成**本地 epoch 秒**。解析不了就 None（那条不计）。
///
/// 逐条流水要的是「几点几分」，而这个数原先算出来、格式化成日期后
/// 就扔掉。抽出来给两边共用 —— 复用不复制（宪法第 12 条），也省得两处各写一遍闰年换算
/// 然后哪天漂成两个答案。
fn local_epoch_of(ts: Option<&str>) -> Option<i64> {
    let s = ts?;
    let b = s.as_bytes();
    // 只认固定形状 `YYYY-MM-DDTHH:MM:SS...`，别为几种奇形怪状写个通用解析器。
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |from: usize, to: usize| s.get(from..to)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let epoch = days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss;
    Some(epoch + local_offset_secs())
}

/// 本地 epoch 秒 → `YYYY-MM-DD HH:MM`（流水行的时间列）。
fn datetime_string(local_epoch: i64) -> String {
    let secs_in_day = local_epoch.rem_euclid(86_400);
    format!(
        "{} {:02}:{:02}",
        date_string(local_epoch),
        secs_in_day / 3600,
        (secs_in_day % 3600) / 60
    )
}

/// 某天 `YYYY-MM-DD` 的本地 0 点 epoch —— 只知道日期的那几条（pi 退回文件名、Hermes
/// 只有 started_at 时不需要）用它当排序键。
fn epoch_of_date(date: &str) -> i64 {
    let num = |from: usize, to: usize| date.get(from..to).and_then(|s| s.parse::<i64>().ok());
    match (num(0, 4), num(5, 7), num(8, 10)) {
        (Some(y), Some(m), Some(d)) => days_from_civil(y, m, d) * 86_400,
        _ => 0,
    }
}

/// epoch 秒（已含本地偏移）→ `YYYY-MM-DD`。
fn date_string(local_epoch: i64) -> String {
    let (y, m, d) = civil_from_days(local_epoch.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// 本机今天的日期。
fn local_today() -> String {
    date_string(now_secs() + local_offset_secs())
}

/// 日期加减天数。
fn shift_date(date: &str, delta: i64) -> String {
    let b = date.as_bytes();
    if b.len() < 10 {
        return date.to_string();
    }
    let num = |from: usize, to: usize| date.get(from..to).and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    let days = days_from_civil(num(0, 4), num(5, 7), num(8, 10)) + delta;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 以 `end` 结尾、长度 `n` 的日期序列（旧 → 新）。
fn date_window(end: &str, n: i64) -> Vec<String> {
    (0..n.max(1)).rev().map(|i| shift_date(end, -i)).collect()
}

// Howard Hinnant 的公历↔天数换算（公认实现，纯整数运算，无闰年特例分支）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ── 花费估算 ──────────────────────────────────────────────────────────────────────

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 按公开报价粗估花费（人民币），**四舍五入到分**。**仅供参考**——本地日志不知道每次实际
/// 走的是哪个供应商（虾盘云便宜、官方贵），这里按各模型「公开列表价」折 ¥ 给个量级，
/// 回答「大头花在哪个模型」。
/// 单价 = (¥/百万 非缓存输入, ¥/百万 输出)；缓存读按输入的 0.1、缓存写按输入的 1.25 计。
///
/// 生产路径现在一律走 [`Pricing::raw`]（它还要判「这个工具是不是包月」），本函数只剩
/// 单测在用 —— 那条用例钉的正是「先折叠再定价」：几百个桶各自 round 一次会让
/// 「按天」和「按模型」两张表对不上账。留着它，那条用例才写得出来。
#[cfg(test)]
fn estimate_cny(model: &str, non_cached_input: u64, cache_creation: u64, cache_read: u64, output: u64) -> f64 {
    round2(estimate_cny_raw(
        model,
        &Acc { non_cached_input, cache_creation, cache_read, output, count: 0 },
    ))
}

/// 同上，但**不四舍五入** —— 要把多个桶加起来时必须用这个，
/// 否则几百个桶各自 round 一次，「按天」和「按模型」两张表会对不上账。
fn estimate_cny_raw(model: &str, a: &Acc) -> f64 {
    let (in_rate, out_rate) = price_per_million(model);
    let m = 1_000_000.0;
    (a.non_cached_input as f64 / m) * in_rate
        + (a.cache_creation as f64 / m) * in_rate * 1.25
        + (a.cache_read as f64 / m) * in_rate * 0.1
        + (a.output as f64 / m) * out_rate
}

/// 各模型 ¥/百万 token（input, output）。按公开列表价 ×~7.2 折 ¥，只求量级对。
/// 每百万 token 的人民币价（输入, 输出）。**全仓唯一一份价表** ——
/// 水电表按它算总账，对话里「这轮花了多少」也按它算，两个数才对得上。
///
/// 🔴 别拿上游 CLI 自己报的 `cost_usd`：那是按**它认得的那家官方价**算的
/// （Claude Code 拿 Anthropic 价目表算 deepseek 模型 = 要么 0 要么离谱），
/// 客户走虾盘云时和真实扣费无关。宁可用自己的口径，也不显示一个对不上的数。
pub fn price_per_million(model: &str) -> (f64, f64) {
    let m = model.to_ascii_lowercase();
    let has = |s: &str| m.contains(s);
    if has("opus") {
        (108.0, 540.0)
    } else if has("sonnet") || has("fable") {
        (21.6, 108.0)
    } else if has("haiku") {
        (5.8, 28.8)
    } else if has("deepseek") {
        (2.0, 8.0)
    } else if has("gpt-5") || has("gpt5") || has("codex") || has("o1") || has("o3") {
        (30.0, 120.0)
    } else if has("gpt-4o") || has("gpt-4") || has("gpt") {
        (18.0, 72.0)
    } else if has("gemini") {
        (5.0, 20.0)
    } else if has("qwen") {
        (4.0, 12.0)
    } else if has("glm") {
        (4.0, 12.0)
    } else if has("kimi") || has("moonshot") {
        (4.0, 12.0)
    } else if has("grok") {
        (20.0, 100.0)
    } else if has("minimax") {
        (3.0, 12.0)
    } else {
        (15.0, 60.0) // 未知模型兜底
    }
}

// ── 测试：省钱建议 + 日期换算（纯算术，不读盘）──────────────────────────────────────
//
// 扫日志那部分要几百 MB 真日志才测得动，这里钉住**结论逻辑**和**日期换算**：
// 阈值一改、单价一改，建议就会静默变味 —— 客户看到的是「建议」，错了比不给更糟；
// 日期错一天，「今天花了多少」这块表盘就整个是假的。
#[cfg(test)]
mod tests {
    use super::*;

    /// 🔴 **同一次 API 调用被写成多行，只能计一次。**
    ///
    /// Claude Code 把一次回复里的每个 content block 各写一行 assistant 消息，
    /// 而**每行都带着整个请求的 `message.usage`**。逐行相加 = 按 block 数重复计费，
    /// 本机 7 天实测虚高 1.84 倍。
    ///
    /// 这个 bug 从水电表第一天起就在，活到 2026-08-16 才被发现，原因写在这儿值得记：
    /// **扫日志那段一直没有测试**（上面那行注释写着「要几百 MB 真日志才测得动」）——
    /// 而它根本不需要几百 MB，只需要两行。造一个临时目录就能跑。
    /// 「测不动」是个没验证过的假设，它替这个 bug 挡了很久。
    #[test]
    fn claude_one_api_call_counted_once_even_when_split_across_lines() {
        let dir = std::env::temp_dir().join(format!("uking-usage-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");

        // 一次调用（req_A）拆成 3 行：一段文本 + 两个 tool_use。三行的 usage 一模一样。
        // 外加一次真调用（req_B）和一行没有 requestId 的老格式。
        let line = |rid: Option<&str>, out: u64| {
            let r = rid.map(|r| format!(r#""requestId":"{r}","#)).unwrap_or_default();
            format!(
                r#"{{"type":"assistant","timestamp":"2026-08-16T05:08:40.259Z","cwd":"C:/proj",{r}"message":{{"model":"claude-opus-5","usage":{{"input_tokens":10,"output_tokens":{out},"cache_read_input_tokens":100,"cache_creation_input_tokens":0}}}}}}"#
            )
        };
        let body = [
            line(Some("req_A"), 50),
            line(Some("req_A"), 50),
            line(Some("req_A"), 50),
            line(Some("req_B"), 70),
            line(None, 90),
        ]
        .join("\n");
        std::fs::write(dir.join("s.jsonl"), body).expect("写临时日志");

        let mut scan = Scan { collect_events: true, ..Default::default() };
        // cutoff=None 不按 mtime 筛；from_date 给个远古日期，让这几行一定落进窗口。
        scan_claude(&dir, &None, "1970-01-01", &mut scan, 1000);
        let _ = std::fs::remove_dir_all(&dir);

        let total: u64 = scan.buckets.values().map(|a| a.count).sum();
        assert_eq!(total, 3, "req_A 的三行只该算一次，加上 req_B 和无 id 的那行 = 3 次");

        let out: u64 = scan.buckets.values().map(|a| a.output).sum();
        assert_eq!(out, 50 + 70 + 90, "重复行的 token 不许再加一遍");

        // 流水也得跟着去重 —— 两边走同一条扫描路径，不该出现「总数对了、明细还是重复的」
        assert_eq!(scan.events.len(), 3, "流水条数要和计费次数一致");
    }

    /// 不要明细时一条流水都不收（默认路径不为一个可选视图背几万条的内存）。
    #[test]
    fn events_are_not_collected_unless_asked() {
        let mut scan = Scan::default();
        scan.event(RawEvent {
            epoch: 0,
            exact: true,
            tool: "claude",
            model: "m".into(),
            project: String::new(),
            input: 1,
            cache_read: 0,
            cache_write: 0,
            output: 1,
            calls: 1,
            session_rollup: false,
        });
        assert!(scan.events.is_empty(), "collect_events=false 时必须是空操作");
    }

    /// 截断要**先按时间排序再截**，而且截掉多少必须如实报出来。
    /// 悄悄只留最近 N 条，会让人把这几条的合计当成整个窗口的合计。
    #[test]
    fn ledger_truncation_keeps_newest_and_admits_what_it_cut() {
        let raw: Vec<RawEvent> = (0..10)
            .map(|i| RawEvent {
                epoch: i as i64 * 60,
                exact: true,
                tool: "claude",
                model: "claude-opus-5".into(),
                project: String::new(),
                input: 1,
                cache_read: 0,
                cache_write: 0,
                output: 1,
                calls: 1,
                session_rollup: false,
            })
            .collect();
        let pricing = Pricing { subscription: Default::default() };
        let (events, meta) = fold_events(raw, &pricing, 3);
        let meta = meta.expect("要了明细就该有元信息");
        assert_eq!(events.len(), 3);
        assert_eq!(meta.total, 10);
        assert_eq!(meta.truncated, 7, "截掉 7 条就要说截掉 7 条");
        // 最近的在前
        assert!(events[0].epoch > events[1].epoch && events[1].epoch > events[2].epoch);
        assert_eq!(events[0].epoch, 9 * 60, "留下的必须是最新的那几条，不是碰巧排在前面的");
    }

    /// 包月订阅的工具：token 照报，钱恒 0 —— 流水和聚合表必须是同一条规则。
    /// 各算各的话，聚合说 ¥0、流水给个金额，客户对不上账就会连别的数字一起不信。
    #[test]
    fn subscription_rows_report_tokens_but_zero_money() {
        let raw = vec![RawEvent {
            epoch: 0,
            exact: true,
            tool: "claude",
            model: "claude-opus-5".into(),
            project: String::new(),
            input: 100_000,
            cache_read: 0,
            cache_write: 0,
            output: 100_000,
            calls: 1,
            session_rollup: false,
        }];
        let mut subscription = std::collections::HashSet::new();
        subscription.insert("claude".to_string());
        let (events, _) = fold_events(raw, &Pricing { subscription }, 10);
        assert_eq!(events[0].cny, 0.0, "标了包月就不许折出金额");
        assert_eq!(events[0].tokens, 200_000, "但 token 照报——量是真烧了的");
    }

    fn item(tool: &str, model: &str, cny: f64, input: u64, output: u64) -> LocalUsageItem {
        LocalUsageItem {
            model: model.into(),
            tool: tool.into(),
            cny,
            count: 100,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    /// 花了几毛钱的人不需要省钱建议 —— 那时候任何建议都是噪音。
    #[test]
    fn pennies_get_no_advice() {
        let items = vec![item("claude", "claude-opus-4-8", 0.4, 10_000, 2_000)];
        assert!(build_tips(30, 0.4, &items, true).is_empty());
        assert!(build_tips(30, 0.0, &[], true).is_empty());
    }

    /// 贵模型占大头 → 给换档建议，且**省下的钱是算出来的**（同样 token × 便宜档单价之差），
    /// 不是拍脑袋的折扣。
    #[test]
    fn premium_hog_gets_a_switch_tip_with_real_math() {
        let items = vec![
            item("claude", "claude-opus-5", 108.0, 1_000_000, 0), // opus 输入 ¥108/M
            item("claude", "deepseek-v4", 2.0, 1_000_000, 0),
        ];
        let tips = build_tips(30, 110.0, &items, true);
        let t = tips.iter().find(|t| t.id == "switch_cheap_model").expect("应给换档建议");
        // 同样 100 万输入 token：opus ¥108 → deepseek ¥2，差 ¥106（30 天窗口=每月）
        assert!((t.saving_cny - 106.0).abs() < 0.5, "算出来的是 {}", t.saving_cny);
        assert!(t.detail.contains("1/54"), "先说倍数：{}", t.detail);
    }

    /// 便宜模型占大头时别硬凑建议 —— 它已经是最省的那档了。
    #[test]
    fn cheap_model_hog_gets_no_switch_tip() {
        let items = vec![item("claude", "deepseek-v4", 50.0, 25_000_000, 0)];
        let tips = build_tips(30, 50.0, &items, true);
        assert!(!tips.iter().any(|t| t.id == "switch_cheap_model"));
    }

    /// 压缩机没生效才提；已经在省了就别再唠叨。
    #[test]
    fn squeezer_tip_only_when_inactive() {
        let items = vec![item("claude", "deepseek-v4", 50.0, 25_000_000, 0)];
        assert!(build_tips(30, 50.0, &items, false).iter().any(|t| t.id == "enable_squeezer"));
        assert!(!build_tips(30, 50.0, &items, true).iter().any(|t| t.id == "enable_squeezer"));
        // Codex-only 的用户不提（hook 只接管 Claude Code，提了也没用）
        let codex_only = vec![item("codex", "gpt-5-codex", 50.0, 1_600_000, 0)];
        assert!(!build_tips(30, 50.0, &codex_only, false).iter().any(|t| t.id == "enable_squeezer"));
    }

    /// 输出占比高 → 提「让 AI 少啰嗦」，因为输出单价是输入的 4~5 倍。
    #[test]
    fn output_heavy_gets_shorter_replies_tip() {
        // deepseek：输入 ¥2/M、输出 ¥8/M。100 万输入(¥2) + 100 万输出(¥8) → 输出占 80%
        let items = vec![item("claude", "deepseek-v4", 10.0, 1_000_000, 1_000_000)];
        assert!(build_tips(30, 10.0, &items, true).iter().any(|t| t.id == "shorter_replies"));
    }

    /// 天数窗口要折算成「每月」，7 天的数据不能当一个月报。
    #[test]
    fn saving_is_normalised_to_a_month() {
        let items = vec![item("claude", "claude-opus-5", 108.0, 1_000_000, 0)];
        let week = build_tips(7, 110.0, &items, true);
        let month = build_tips(30, 110.0, &items, true);
        let w = week.iter().find(|t| t.id == "switch_cheap_model").unwrap().saving_cny;
        let m = month.iter().find(|t| t.id == "switch_cheap_model").unwrap().saving_cny;
        assert!(w > m * 4.0, "7 天花这么多，折成月应该更高：周 {w} vs 月 {m}");
    }

    // ── 水电表 ──

    /// 公历换算得能来回跑通，且认得出闰年 —— 日期错一天，整块表盘就是假的。
    #[test]
    fn civil_calendar_roundtrips() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 7, 31), (2024, 12, 31), (2100, 3, 1)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d), "{y}-{m}-{d}");
        }
        // 跨月/跨年/闰日加减
        assert_eq!(shift_date("2026-03-01", -1), "2026-02-28");
        assert_eq!(shift_date("2024-03-01", -1), "2024-02-29");
        assert_eq!(shift_date("2026-01-01", -1), "2025-12-31");
        assert_eq!(shift_date("2026-12-31", 1), "2027-01-01");
    }

    /// 时间戳按**本地日期**分桶。东八区的 UTC 23:30 已经是第二天了 ——
    /// 直接切 ISO 字符串前 10 位（UTC 日期）会把这段用量记到昨天。
    #[test]
    fn timestamps_bucket_by_local_date_not_utc() {
        let off = local_offset_secs();
        let got = local_epoch_of(Some("2026-07-31T23:30:00.000Z")).map(date_string).expect("该解析得出");
        // 手算一遍期望值，跟被测函数走不同的路：epoch → 加偏移 → 取日期
        let epoch = days_from_civil(2026, 7, 31) * 86_400 + 23 * 3600 + 30 * 60;
        assert_eq!(got, date_string(epoch + off));
        if off >= 1800 {
            assert_eq!(got, "2026-08-01", "东时区应该已经跨到第二天");
        }
    }

    /// 形状不对的时间戳一律丢弃，不要静默按今天算 —— 那会把陈年老账记成今天的花费。
    #[test]
    fn broken_timestamps_are_dropped_not_guessed() {
        for bad in ["", "2026/07/31 10:00:00", "昨天", "2026-07-31"] {
            assert!(local_epoch_of(Some(bad)).is_none(), "不该认：{bad}");
        }
        assert!(local_epoch_of(None).is_none());
    }

    /// 窗口是「含今天在内的 N 天」，且旧 → 新排好序。
    #[test]
    fn date_window_includes_today_and_is_ordered() {
        let w = date_window("2026-07-31", 3);
        assert_eq!(w, vec!["2026-07-29", "2026-07-30", "2026-07-31"]);
        assert_eq!(date_window("2026-07-31", 1), vec!["2026-07-31"]);
    }

    /// 桶的定价必须**先折叠再定价**：几百个桶各自四舍五入再相加会漂移，
    /// 「按天」和「按模型」两张表就对不上账了。
    #[test]
    fn folding_before_pricing_avoids_rounding_drift() {
        // 300 个各自 ¥0.004 的小桶：先 round 再加 = ¥0.00；先加再 round = ¥1.20
        let one = Acc { non_cached_input: 2_000, cache_creation: 0, cache_read: 0, output: 0, count: 1 };
        let per_bucket_rounded: f64 = (0..300).map(|_| estimate_cny("deepseek-v4", 2_000, 0, 0, 0)).sum();
        let folded = round2((0..300).map(|_| estimate_cny_raw("deepseek-v4", &one)).sum::<f64>());
        assert_eq!(per_bucket_rounded, 0.0, "逐桶四舍五入会把钱抹成 0");
        assert!((folded - 1.2).abs() < 0.01, "折叠后应该是 ¥1.20，得到 {folded}");
    }

    /// 缓存命中率低才提示；已经在高命中就别唠叨。量太小也不提（噪音）。
    #[test]
    fn cache_tip_only_when_cold_and_meaningful() {
        let cold = CacheStats { non_cached_input: 900_000, cache_read: 100_000, cache_creation: 0, hit_rate: 0.1, saved_cny: 0.0 };
        let warm = CacheStats { non_cached_input: 100_000, cache_read: 900_000, cache_creation: 0, hit_rate: 0.9, saved_cny: 0.0 };
        let tiny = CacheStats { non_cached_input: 900, cache_read: 100, cache_creation: 0, hit_rate: 0.1, saved_cny: 0.0 };
        let p = Pace::default();
        assert!(build_meter_tips(30, &cold, &[], &p, 50.0).iter().any(|t| t.id == "cache_cold"));
        assert!(!build_meter_tips(30, &warm, &[], &p, 50.0).iter().any(|t| t.id == "cache_cold"));
        assert!(!build_meter_tips(30, &tiny, &[], &p, 50.0).iter().any(|t| t.id == "cache_cold"));
        // 花了几毛钱的人一条都不该收到
        assert!(build_meter_tips(30, &cold, &[], &p, 0.5).is_empty());
    }

    /// 没给余额就不许出现「还能用几天」—— 宁可不说，也不能猜一个数吓客户。
    #[test]
    fn no_balance_means_no_days_left_claim() {
        let cache = CacheStats::default();
        let no_bal = Pace { daily_avg_cny: 10.0, days_left: None, balance_cny: None, ..Default::default() };
        assert!(!build_meter_tips(30, &cache, &[], &no_bal, 300.0).iter().any(|t| t.id == "balance_low"));
        let low = Pace { daily_avg_cny: 10.0, days_left: Some(3.0), balance_cny: Some(30.0), ..Default::default() };
        assert!(build_meter_tips(30, &cache, &[], &low, 300.0).iter().any(|t| t.id == "balance_low"));
        let plenty = Pace { daily_avg_cny: 1.0, days_left: Some(300.0), balance_cny: Some(300.0), ..Default::default() };
        assert!(!build_meter_tips(30, &cache, &[], &plenty, 300.0).iter().any(|t| t.id == "balance_low"));
    }

    /// 项目名取路径最后一段，Windows 反斜杠和 Unix 斜杠都要认。
    #[test]
    fn project_name_takes_last_segment() {
        assert_eq!(project_name(r"C:\Users\me\Desktop\my-app"), "my-app");
        assert_eq!(project_name("/home/me/work/my-app/"), "my-app");
        assert_eq!(project_name("my-app"), "my-app");
    }

    /// 空表也要是**合法的一张表**：ready=false + 说清为什么，而不是抛错或者给一堆 0 假装正常。
    #[test]
    fn empty_machine_reports_not_ready_with_reason() {
        let m = Meter {
            days: 30,
            ready: false,
            blockers: vec!["x".into()],
            window: Totals::default(),
            today: Totals::default(),
            yesterday: Totals::default(),
            last7: Totals::default(),
            daily: vec![],
            by_model: vec![],
            by_tool: vec![],
            by_project: vec![],
            events: vec![],
            events_meta: None,
            cache: CacheStats::default(),
            pace: Pace::default(),
            tips: vec![],
            sources: build_sources(&Scan::default(), &UsagePrefs::default()),
            source: "local",
        };
        // 本机可能装的 AI 工具要**全列**，不只列算得到的那几个 —— 客户装了 5 个只看到 2 个的账时，
        // 必须知道那 3 个去哪了。
        assert_eq!(m.sources.len(), TOOL_CATALOG.len(), "工具清单要全列出来");
        // 算不到的每条都得给出**为什么**，不能只写一句「不支持」。
        for s in m.sources.iter().filter(|s| !s.countable) {
            assert!(!s.note.trim().is_empty(), "{} 算不到，却没说为什么", s.label);
            assert!(!s.enabled, "{} 根本算不到，不该给一个点了没用的勾", s.label);
        }
        // 算得到的那几路默认全开（默认偏好下没人被静默漏掉）。
        let countable: Vec<_> = m.sources.iter().filter(|s| s.countable).collect();
        assert!(countable.len() >= 5, "claude/codex/openclaw/hermes/pi 五路都该算得到");
        assert!(countable.iter().all(|s| s.enabled), "默认偏好下算得到的应当全开");
        // 脱敏：任何路径都不许把家目录原样吐出去
        let home = home_dir().to_string_lossy().replace('\\', "/");
        assert!(m.sources.iter().all(|s| !s.dir.contains(&home)), "路径要脱敏成 ~");
    }

    /// 包月订阅的工具：**token 照算，金额记 0**。
    /// 拿 API 列表价把包月的用量折成 ¥ 记进「花了多少」，客户对着账单会发现全对不上 ——
    /// 那比不报更糟。
    #[test]
    fn subscription_tools_count_tokens_but_not_money() {
        let prefs = UsagePrefs { disabled: vec![], subscription: vec!["claude".into()] };
        let pricing = Pricing::of(&prefs);
        let a = Acc { non_cached_input: 1_000_000, cache_read: 0, cache_creation: 0, output: 1_000_000, count: 3 };
        assert_eq!(pricing.raw("claude", "claude-opus-4-8", &a), 0.0, "包月的不该折出钱来");
        assert!(pricing.raw("codex", "claude-opus-4-8", &a) > 0.0, "没标包月的照常算钱");
        // token 本身一个都不能少 —— 「不折钱」不等于「不统计」
        assert_eq!(a.tokens(), 2_000_000);
    }

    /// 未知工具 id 当场拒 —— 静默丢掉会让用户以为勾上了，实际一直没生效。
    #[test]
    fn unknown_tool_id_is_rejected() {
        let bad = UsagePrefs { disabled: vec!["nope".into()], subscription: vec![] };
        assert!(write_prefs(&bad).is_err(), "打错的工具 id 该被拒");
    }

    /// 偏好存「关掉的」而不是「开着的」：以后新接一路数据源，存量客户自动纳入。
    #[test]
    fn prefs_store_the_disabled_list_so_new_tools_default_on() {
        let p = UsagePrefs { disabled: vec!["pi".into()], subscription: vec![] };
        assert!(!p.on("pi"), "关掉的就是关掉的");
        assert!(p.on("claude"), "没关的默认开着");
        assert!(p.on("某个还没写出来的新工具"), "以后新加的一路默认就该算进来，不能等用户去勾");
    }
}
