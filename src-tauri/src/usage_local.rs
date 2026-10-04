//! 本地用量统计 —— 读各 AI 编程工具**自己记的会话日志**，
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
//! ## 红线（数据安全）
//! - **只读不写、绝不上传**：纯本地聚合，不发任何网络请求。
//! - **只取元数据**：只累加 token 数 / 模型名 / 时间戳 / 次数 / **项目目录**，
//!   **从不读取或存储 prompt / 消息内容**。项目目录只用于本机分账，同样不出这台机器。
//!
//! ## 2026-10-04：「Token 水电表」已删除
//! 原先建在这份扫描之上的水电表（按天 / 按项目 / 缓存分账、逐条流水、数据来源清单、
//! 「包月订阅 / 不算某工具」偏好 `~/.uking/usage-tools.json`）零用量证据，整体删除。
//! 本模块只剩 `breakdown`（按模型汇总 + 省钱建议）；包月偏好随之取消，所有工具一律按公开报价折算。
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

#[derive(Default)]
struct Scan {
    buckets: HashMap<Bucket, Acc>,
    lines: u64,
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

/// 读本地日志，聚合最近 `days` 天的按（工具, 模型）用量。
///
/// 时间窗口两道闸：**文件 mtime 粗筛**（跳过整个陈旧文件，省 IO）+ **逐行时间戳精筛**
/// （一个长会话文件可能横跨窗口边界，只按 mtime 会把窗口外的量算进来）。
/// 跑很多文件，lib.rs 以 spawn_blocking 转调别卡 UI。
pub fn breakdown(days: i64) -> LocalUsage {
    let scan = scan_all(days);
    let items = fold_by_model(&scan);

    let total_cny = round2(items.iter().map(|i| i.cny).sum::<f64>());
    let total_calls = items.iter().map(|i| i.count).sum();
    let total_input_tokens = items.iter().map(|i| i.input_tokens).sum();
    let total_output_tokens = items.iter().map(|i| i.output_tokens).sum();

    let tips = build_tips(days, total_cny, &items);
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

/// 把桶折成「按模型」的明细（和 0.9.63 起的 `breakdown` 口径逐字节一致）。
fn fold_by_model(scan: &Scan) -> Vec<LocalUsageItem> {
    let mut agg: HashMap<(String, String), Acc> = HashMap::new();
    for (b, a) in &scan.buckets {
        agg.entry((b.tool.clone(), b.model.clone())).or_default().add(a);
    }
    let mut items: Vec<LocalUsageItem> = agg
        .into_iter()
        .map(|((tool, model), a)| LocalUsageItem {
            cny: round2(estimate_cny_raw(&model, &a)),
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

// ── 省钱建议（纯算术，不猜）───────────────────────────────────────────────────────

/// 便宜档模型的参考单价（¥/百万 token）。用 deepseek 系当基准 —— 国产、够用、便宜，
/// 也是 U-King 默认推的那档。换算出来的「能省多少」是**同样的 token 换个模型跑**，
/// 不是拍脑袋的折扣。
const CHEAP_IN: f64 = 2.0;
const CHEAP_OUT: f64 = 8.0;

/// 从聚合结果里算出建议。每条要么给得出可核对的数，要么就不给数。
fn build_tips(days: i64, total_cny: f64, items: &[LocalUsageItem]) -> Vec<UsageTip> {
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

/// 扫一遍各路日志，产出最细粒度的桶（按模型汇总从这一份折出来）。
fn scan_all(days: i64) -> Scan {
    let days = days.clamp(1, 365);
    // mtime 粗筛多放宽一天：一个会话文件可能昨天写的、今天才落最后一行。
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs((days as u64 + 1) * 86_400));
    // 逐行精筛的下界（本地日期，字符串比较即可 —— ISO 日期天然可比）。
    let from_date = shift_date(&local_today(), -(days - 1));

    let mut scan = Scan::default();
    // 安全阀：极端历史下别无限跑（正常远达不到）。
    const MAX_LINES: u64 = 3_000_000;
    let home = home_dir();
    scan_claude(&claude_projects_dir(), &cutoff, &from_date, &mut scan, MAX_LINES);
    scan_codex(&codex_sessions_dir(), &cutoff, &from_date, &mut scan, MAX_LINES);
    scan_openclaw(&home.join(".openclaw").join("agents"), &cutoff, &from_date, &mut scan, MAX_LINES);
    scan_hermes(days, &from_date, &mut scan);
    scan_pi(&home.join(".pi").join("agent").join("sessions"), &cutoff, &from_date, &mut scan, MAX_LINES);
    scan
}

// ── Claude Code：~/.claude/projects/**/*.jsonl ────────────────────────────────────

fn scan_claude(dir: &Path, cutoff: &Option<SystemTime>, from_date: &str, scan: &mut Scan, max_lines: u64) {
    walk_recent(dir, cutoff, max_lines, scan, &mut |path, scan| {
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
                // pi 的行时间戳键名不固定，拿不到就退回文件名里的 ISO 时间（目录里就是这么命名的；
                // 那条路只知道哪天，零点是凑出来的 —— 只用于按日期归桶，没问题）。
                let ts = j
                    .get("timestamp")
                    .or_else(|| j.get("ts"))
                    .and_then(|t| t.as_str())
                    .map(String::from)
                    .or_else(|| pi_date_from_name(path));
                let Some(epoch) = local_epoch_of(ts.as_deref()) else { return };
                let date = date_string(epoch);
                if date.as_str() < from_date {
                    return;
                }
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
    // 没有 Node ≥22.5（用其内置 node:sqlite）读不了它的 state.db：Hermes 的用量算不进来，静默跳过。
    let Some(node) = find_node() else { return };
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
        Err(_) => return,
    };
    let Some(rows) = out.as_array() else { return };
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
// 日志里的时间戳全是 **UTC**（`2026-07-31T15:34:07.221Z`），但用量统计必须按**本地日期**
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
/// 用量汇总按它算总账，保证所有地方的「花了多少」同一口径、加得起来。
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

        let mut scan = Scan::default();
        // cutoff=None 不按 mtime 筛；from_date 给个远古日期，让这几行一定落进窗口。
        scan_claude(&dir, &None, "1970-01-01", &mut scan, 1000);
        let _ = std::fs::remove_dir_all(&dir);

        let total: u64 = scan.buckets.values().map(|a| a.count).sum();
        assert_eq!(total, 3, "req_A 的三行只该算一次，加上 req_B 和无 id 的那行 = 3 次");

        let out: u64 = scan.buckets.values().map(|a| a.output).sum();
        assert_eq!(out, 50 + 70 + 90, "重复行的 token 不许再加一遍");
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
        assert!(build_tips(30, 0.4, &items).is_empty());
        assert!(build_tips(30, 0.0, &[]).is_empty());
    }

    /// 贵模型占大头 → 给换档建议，且**省下的钱是算出来的**（同样 token × 便宜档单价之差），
    /// 不是拍脑袋的折扣。
    #[test]
    fn premium_hog_gets_a_switch_tip_with_real_math() {
        let items = vec![
            item("claude", "claude-opus-5", 108.0, 1_000_000, 0), // opus 输入 ¥108/M
            item("claude", "deepseek-v4", 2.0, 1_000_000, 0),
        ];
        let tips = build_tips(30, 110.0, &items);
        let t = tips.iter().find(|t| t.id == "switch_cheap_model").expect("应给换档建议");
        // 同样 100 万输入 token：opus ¥108 → deepseek ¥2，差 ¥106（30 天窗口=每月）
        assert!((t.saving_cny - 106.0).abs() < 0.5, "算出来的是 {}", t.saving_cny);
        assert!(t.detail.contains("1/54"), "先说倍数：{}", t.detail);
    }

    /// 便宜模型占大头时别硬凑建议 —— 它已经是最省的那档了。
    #[test]
    fn cheap_model_hog_gets_no_switch_tip() {
        let items = vec![item("claude", "deepseek-v4", 50.0, 25_000_000, 0)];
        let tips = build_tips(30, 50.0, &items);
        assert!(!tips.iter().any(|t| t.id == "switch_cheap_model"));
    }

    /// 输出占比高 → 提「让 AI 少啰嗦」，因为输出单价是输入的 4~5 倍。
    #[test]
    fn output_heavy_gets_shorter_replies_tip() {
        // deepseek：输入 ¥2/M、输出 ¥8/M。100 万输入(¥2) + 100 万输出(¥8) → 输出占 80%
        let items = vec![item("claude", "deepseek-v4", 10.0, 1_000_000, 1_000_000)];
        assert!(build_tips(30, 10.0, &items).iter().any(|t| t.id == "shorter_replies"));
    }

    /// 天数窗口要折算成「每月」，7 天的数据不能当一个月报。
    #[test]
    fn saving_is_normalised_to_a_month() {
        let items = vec![item("claude", "claude-opus-5", 108.0, 1_000_000, 0)];
        let week = build_tips(7, 110.0, &items);
        let month = build_tips(30, 110.0, &items);
        let w = week.iter().find(|t| t.id == "switch_cheap_model").unwrap().saving_cny;
        let m = month.iter().find(|t| t.id == "switch_cheap_model").unwrap().saving_cny;
        assert!(w > m * 4.0, "7 天花这么多，折成月应该更高：周 {w} vs 月 {m}");
    }

    // ── 日期 / 定价 ──

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

}
