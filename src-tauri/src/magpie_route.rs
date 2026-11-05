//! magpie 路由：把「哪个 AI 用哪一路」搬进 U-King 的 AI 设置。
//!
//! magpie 会把客户的多份订阅 / 多个 Key 合成「路由组」（`group/<id>`）和「自动路由」
//! （`auto/<模型>`）；对 AI 工具来说，它们只是一个模型名。所以本模块只做四件事：
//! 读出有哪些路由、读回某个 AI 现在走哪一路、切过去（并回读确认）、还原。
//!
//! 为什么全走 `magpie <agent> model` 这条 CLI、而不是去读 magpie 自己的数据库：
//! 数据库是它说了算的内部实现，CLI 是它对外承诺的接口。只依赖承诺过的那一层，
//! 它哪天换存储方案，我们不会跟着碎。
//!
//! 🔴 耦合纪律：本模块**只依赖公共层**（`installer` / `fs` / `ulog`）。
//! 需要 `providers::effective_config` 这类功能模块数据时，由组合根 `lib.rs` 取好当参数传进来
//! （见 `state` 的 `cfg`）。`check-module-coupling.mjs` 会拦新增耦合。

use crate::installer;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// magpie 的运行态网关。写死回环地址：这是本机两个进程之间的约定，不是可配置项。
pub const GATEWAY: &str = "http://127.0.0.1:3425";

/// U-King 支持配置的五种 AI 工具：(U-King 配置目标, magpie agent id, magpie 打印的人话名)。
///
/// 只放五条是产品决定：ClawX / OpenCode 在 magpie 里没有对应的 agent 口径，硬塞进来
/// 只会让客户以为「切了、生效了」，其实 magpie 根本不认识这个 agent。
/// 第三列同时用于 UI 展示和匹配 `magpie <agent> model` 的回读行 —— magpie 打的正是
/// `Claude Code model group/x` 这种「人话名 + model + 值」，两处共用一份名字才不会漂。
pub const AGENTS: &[(&str, &str, &str)] = &[
    ("claude", "claude", "Claude Code"),
    ("codex", "codex", "Codex"),
    ("dsh", "dsh", "DeepSeek Harness"),
    ("pi", "pi", "Pi"),
    ("hermes", "hermes", "Hermes Agent"),
];

/// 一条可选的路由（一个路由组，或一条自动路由）。
#[derive(Serialize, Clone, Default, Debug)]
pub struct RouteOption {
    /// `group/smart-split` | `auto/deepseek-v4.1-flash` —— 切过去时原样交给 magpie。
    pub model: String,
    /// 人话名（网关的 `display_name`，缺省退回 model id）。
    pub name: String,
    /// `group` | `auto`。UI 靠它决定放「我的路由组」还是折叠的「自动路由」。
    pub kind: String,
    /// magpie 的推理档位（`reasoning_levels`），没有就空。
    pub levels: Vec<String>,
    /// 组里有哪些成员（只有配置文件兜底来源才有；网关来源为空）。
    pub members: Vec<String>,
}

/// 某个 AI 工具「现在走哪一路」。
#[derive(Serialize, Clone, Default, Debug)]
pub struct ToolRoute {
    pub target: String,
    pub label: String,
    pub agent: String,
    /// 回读到的模型值（已剥 `magpie/` 前缀）。可能是路由，也可能不是。
    pub model: Option<String>,
    /// 是路由就是 `group/x` / `auto/x`，否则 None。
    pub route: Option<String>,
    /// `cli` | `config` | `none` —— 这个结论是从哪来的（排查时第一眼要看的字段）。
    pub source: String,
    /// false = **不知道**，UI 不许把它渲染成「没配」。
    pub readable: bool,
}

/// 设置页「路由」分区一次要的全部数据。
#[derive(Serialize, Clone, Default, Debug)]
pub struct RouteState {
    pub installed: bool,
    pub running: bool,
    pub gateway: String,
    pub routes: Vec<RouteOption>,
    pub tools: Vec<ToolRoute>,
    /// `gateway` | `config` | `none`
    pub routes_source: String,
    /// 只在「本该读到却读失败」时给，用来说清为什么列表是空的。
    pub error: Option<String>,
}

/// 子进程统一超时（秒）。写操作要改真实配置，宁可多等也别半路剪掉；
/// 读操作是并行跑的（见 `read_tools`），所以最坏也就是这一个值，不会叠成五份。
const CLI_TIMEOUT_SECS: u64 = 25;

/// 网关 TCP 探测超时。本机回环 200ms 已经很宽裕（端口没人听是立刻返回的）。
const GATEWAY_PROBE_TIMEOUT_MS: u64 = 200;

/// 没装 magpie 时统一说这句：客户下一步该做什么，一句话说清楚。
fn not_installed() -> String {
    "没找到 magpie 程序，请先在「我的 AI」里安装 magpie（官方桌面版）".to_string()
}

// ============================================================
// 对外主流程
// ============================================================

/// 设置页打开时读一次：装没装、网关在不在、有哪些路由、五个 AI 各走哪一路。
///
/// `cfg` 是组合根（`lib.rs`）传来的兜底数据 `(target, 该工具配置里的 model, base_url)`。
/// 为什么需要兜底：`magpie <agent> model` 是第一权威，但它依赖 magpie CLI 能跑；
/// CLI 被安全软件拦住、或旧版本没实现这个子命令时，退回读各工具自己的配置还能判断
/// 「确实在走本机网关的路由」。两样都拿不到就如实说读不到 —— 绝不猜一个组名糊弄过去。
pub fn state(cfg: &[(String, Option<String>, Option<String>)]) -> RouteState {
    let exe = magpie_exe();
    let installed = exe.is_some();
    let running = running();
    let (routes, routes_source, error) = load_routes(installed, running);
    let tools = read_tools(exe.as_deref(), cfg);
    assemble(installed, running, routes, routes_source, tools, error)
}

/// 路由组从哪来：网关（运行态，最全）→ 配置文件（离线兜底）。
///
/// 为什么网关读到了还要兜底：网关 `/v1/models` 拿不到时前端不能空着 ——
/// 客户明明在 magpie 里建过组，只是 magpie 刚启动/刚升级，这时候必须还能看见组。
fn load_routes(installed: bool, running: bool) -> (Vec<RouteOption>, &'static str, Option<String>) {
    if !installed {
        // 没装就没有 magpie 路由可言，UI 会走「先去安装」那一态；这里不再白读配置文件。
        return (Vec::new(), "none", None);
    }
    let mut why: Option<String> = None;
    if running {
        match gateway_routes() {
            Ok(list) if !list.is_empty() => return (list, "gateway", None),
            // 网关在跑但一个路由组都没有：不算错误（客户可能还没建组），继续试配置文件。
            Ok(_) => {}
            Err(e) => why = Some(e),
        }
    }
    match config_routes() {
        Ok(list) if !list.is_empty() => (list, "config", None),
        // 配置文件不存在 / 里面没有组 = 正常空态，不编错误吓人；真正读失败才报。
        Ok(_) => (Vec::new(), "none", why),
        Err(e) => {
            let msg = match why {
                Some(prev) => format!("{prev}；{e}"),
                None => e,
            };
            (Vec::new(), "none", Some(msg))
        }
    }
}

/// 切到某一路。**先校验、再 spawn**：模型名不合法时一个进程都不起。
///
/// 为什么要校验：magpie 对乱七八糟的模型名会真的去写各工具配置文件，写坏了客户要手改回来；
/// 而前端传来的值不可信（尤其将来可能从输入框来）。
pub fn apply(agent: &str, model: &str) -> Result<(), String> {
    if !is_known_agent(agent) {
        return Err(format!("不认识的 AI 工具：{agent}"));
    }
    let route = normalize_route(model).ok_or_else(|| {
        format!("只能切到 magpie 的路由（group/… 或 auto/…），收到的是「{model}」")
    })?;
    let exe = magpie_exe().ok_or_else(not_installed)?;
    let out = run_cli(&exe, &[agent, &route])?;
    if !out.status.success() {
        return Err(format!("magpie 切换失败：{}", tail_chars(&cli_text(&out), 200)));
    }
    // 回读走公开入口：写和读用同一套「magpie 装在哪」的探测，客户中途装了/卸了 magpie 也不会读到旧路径。
    verify_readback(&route, read_agent_model(agent).as_deref())
}

/// 还原：`magpie <agent> default`，把该工具变回 magpie 接管前的那份配置。
pub fn reset(agent: &str) -> Result<(), String> {
    if !is_known_agent(agent) {
        return Err(format!("不认识的 AI 工具：{agent}"));
    }
    let exe = magpie_exe().ok_or_else(not_installed)?;
    let out = run_cli(&exe, &[agent, "default"])?;
    if !out.status.success() {
        return Err(format!("magpie 还原失败：{}", tail_chars(&cli_text(&out), 200)));
    }
    verify_reset(agent, read_agent_model(agent).as_deref())
}

/// 直接拉起 magpie 窗口（路由组的创建/编辑仍在 magpie 里做，本版不做）。
pub fn open_app() -> Result<(), String> {
    let exe = magpie_exe().ok_or_else(not_installed)?;
    open_exe(&exe)
}

/// 回读判据：写完必须再问一次 magpie。
///
/// **「命令返回 0」不等于「生效」** —— magpie 自己也可能写失败却返回成功，
/// 所以这里只认回读值。不一致就把「magpie 说成功、回读是 X」原样告诉客户，
/// 不许把没生效说成生效（这是这一版最不能犯的错）。
fn verify_readback(expected: &str, got: Option<&str>) -> Result<(), String> {
    let got_route = got.map(strip_magpie_prefix).as_deref().and_then(normalize_route);
    match (got_route, got) {
        (Some(r), _) if r == expected => Ok(()),
        (Some(r), _) => Err(format!(
            "magpie 说切到 {expected} 了，但回读是 {r} —— 没生效。请打开 magpie 看一眼，或再切一次"
        )),
        (None, Some(raw)) => Err(format!(
            "magpie 说切到 {expected} 了，但回读是「{raw}」—— 没生效。请再切一次"
        )),
        (None, None) => Err(
            "magpie 切换命令成功了，但回读不到当前模型，没法确认是否生效；请打开 magpie 看一眼"
                .to_string(),
        ),
    }
}

/// 还原的回读判据。这里能验的只有一条：**它不再是路由**。
///
/// 为什么要求这么低：`magpie <agent> default` 是把「接管之前那份配置」还回去，那份到底是
/// 什么模型只有 magpie 知道，我们无从比对。但「还原后还读得到 group/auto」一定是没生效。
fn verify_reset(agent: &str, got: Option<&str>) -> Result<(), String> {
    match got.map(strip_magpie_prefix) {
        Some(raw) => match normalize_route(&raw) {
            Some(r) => Err(format!(
                "magpie 说已还原 {agent}，但回读还是路由 {r} —— 没生效。请打开 magpie 手动断开这一路"
            )),
            None => Ok(()),
        },
        None => Err(format!(
            "magpie 还原命令成功了，但回读不到 {agent} 的当前模型，没法确认是否已还原；请打开 magpie 看一眼"
        )),
    }
}

// ============================================================
// magpie 可执行文件 / 网关
// ============================================================

/// 找到本机的 magpie CLI。找不到返回 None（= 没装，UI 据此显示「先去安装」）。
pub fn magpie_exe() -> Option<PathBuf> {
    probe_first(exe_candidates())
}

/// 网关是否在跑（TCP 探测，不真发 HTTP —— 起一次 curl 比 connect 贵得多）。
pub fn running() -> bool {
    use std::net::{Ipv6Addr, SocketAddr, TcpStream};
    use std::time::Duration;
    let dur = Duration::from_millis(GATEWAY_PROBE_TIMEOUT_MS);
    let v4: SocketAddr = ([127, 0, 0, 1], 3425).into();
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, 3425));
    // IPv4 / IPv6 各探一次：只探一个会把「在跑」判成「没跑」（端口探测的同一个坑，
    // `lib.rs` 的 `preview_port_alive` 有同款注释）。
    TcpStream::connect_timeout(&v4, dur).is_ok() || TcpStream::connect_timeout(&v6, dur).is_ok()
}

/// 候选探测注入式：调用方按**优先级顺序**探测，命中给 `Some(路径)`，没命中给 `None`。
///
/// 拆成这个形状是为了能单测「先到先得」这条规则 —— 否则测试要么依赖本机装没装 magpie
/// （测试就成了环境探测器），要么为了造夹具去改真实 home。
pub fn pick_exe(found: Vec<Option<PathBuf>>) -> Option<PathBuf> {
    found.into_iter().flatten().next()
}

/// 按顺序探测候选路径，返回第一个真存在的**文件**。
fn probe_first(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    pick_exe(candidates.into_iter().map(|p| p.is_file().then_some(p)).collect())
}

/// magpie CLI 的候选位置，按「最可能是客户那一份」排序。
fn exe_candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        // ① U-King「我的 AI」自己装的那一份（跟 desktop_app 的 magpie 落点同一处，
        //    模块之间不 import，口径靠这条注释对齐）；② 官方安装器的默认位置。
        let home = installer::user_home_dir();
        v.push(home.join(".uking").join("apps").join("magpie").join("magpie.exe"));
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.trim().is_empty() {
                v.push(Path::new(&local).join("Programs").join("magpie").join("magpie.exe"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // macOS 官方是 .app 包，CLI 就是包里的可执行文件（本机实测路径见桌面版安装器）。
        let home = installer::user_home_dir();
        for root in [PathBuf::from("/Applications"), home.join("Applications")] {
            v.push(root.join("Magpie.app").join("Contents").join("MacOS").join("magpie"));
        }
    }
    // 兜底：PATH 上手工装的那种。macOS 从 Finder 起时 PATH 只有系统那几条，
    // 所以这里走 installer::search_paths（它补了 /opt/homebrew/bin、~/.local/bin 等）。
    v.extend(path_candidates());
    v
}

/// PATH 上的 `magpie` / `magpie.exe` 候选（不判断存在性，交给 `probe_first`）。
fn path_candidates() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["magpie.exe"] } else { &["magpie"] };
    let mut dirs = installer::search_paths(None);
    if let Some(p) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    let mut out = Vec::new();
    for d in dirs {
        for n in names {
            out.push(d.join(n));
        }
    }
    out
}

/// 从 magpie 网关读路由组。
///
/// HTTP 一律走系统 curl（仓库纪律：不加 HTTP 依赖），并且必须 `--noproxy *`：
/// 客户机上常年挂着系统代理，代理会把 127.0.0.1 也一起接走（clash 类工具甚至回自己的
/// 拦截页），结果就是「网关明明在跑，界面却报读不到」。
fn gateway_routes() -> Result<Vec<RouteOption>, String> {
    let url = format!("{GATEWAY}/v1/models");
    let body = installer::curl(&["-sS", "-m", "3", "--noproxy", "*", &url])
        .map_err(|e| format!("读 magpie 网关失败：{}", tail_chars(&e, 200)))?;
    Ok(parse_gateway_models(&body))
}

/// 配置文件兜底：magpie 自己建的 `providers.json`。
fn config_routes() -> Result<Vec<RouteOption>, String> {
    let Some(path) = probe_first(config_candidates()) else {
        // 文件不存在 = 客户还没在 magpie 里建过组，不算错误（UI 有空态文案）。
        return Ok(Vec::new());
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读不到 magpie 配置文件 {}：{e}", path.display()))?;
    Ok(parse_config_groups(&text))
}

/// `providers.json` 的候选位置。`~/.config` 是 magpie 的默认位置；
/// Windows 两处是它按平台惯例的落点（APPDATA / LOCALAPPDATA）。
fn config_candidates() -> Vec<PathBuf> {
    let mut v = vec![installer::user_home_dir()
        .join(".config")
        .join("magpie")
        .join("providers.json")];
    // 用 extend 而不是 cfg 块里 push：非 Windows 上没有任何追加项，
    // 「vec![] 之后紧跟 push」那种写法会在 mac/Linux 触发多一个 lint（CI 的 clippy 账本按平台算）。
    v.extend(win_config_candidates());
    v
}

#[cfg(windows)]
fn win_config_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in ["APPDATA", "LOCALAPPDATA"] {
        if let Ok(root) = std::env::var(var) {
            if !root.trim().is_empty() {
                v.push(Path::new(&root).join("magpie").join("providers.json"));
            }
        }
    }
    v
}

#[cfg(not(windows))]
fn win_config_candidates() -> Vec<PathBuf> {
    Vec::new()
}

// ============================================================
// CLI 回读 / 执行
// ============================================================

/// 单独读某个 agent 当前模型（剥没剥前缀由调用方决定，这里给原始回读值）。
pub fn read_agent_model(agent: &str) -> Option<String> {
    let exe = magpie_exe()?;
    read_agent_model_with(&exe, agent)
}

fn read_agent_model_with(exe: &Path, agent: &str) -> Option<String> {
    let label = label_of(agent)?;
    let out = run_cli(exe, &[agent, "model"]).ok()?;
    if !out.status.success() {
        return None;
    }
    parse_agent_model(label, &installer::decode_console(&out.stdout))
}

/// 跑一次 magpie CLI。启动失败 / 超时都翻成人话 —— 客户看不懂 `os error 740`。
fn run_cli(exe: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut c = cli_command(exe);
    c.args(args);
    installer::output_with_timeout(c, CLI_TIMEOUT_SECS).ok_or_else(|| {
        format!("magpie 没有在 {CLI_TIMEOUT_SECS} 秒内返回（可能被安全软件拦住或界面卡住），请打开 magpie 手动操作")
    })
}

/// 统一建子进程的命令：Windows 上加 CREATE_NO_WINDOW。
///
/// 不加的话：GUI 进程每回读一次就闪一个黑窗，设置页一打开就是五次 —— 客户会以为中了病毒。
#[cfg(windows)]
fn cli_command(exe: &Path) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(exe);
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}

#[cfg(not(windows))]
fn cli_command(exe: &Path) -> std::process::Command {
    std::process::Command::new(exe)
}

/// CLI 输出拼成一段人话：stdout / stderr 分开解码。
///
/// 为什么分开：Windows 上 CLI 本体吐 UTF-8、cmd/系统报错吐 GBK，同一条命令里两种编码会混着来
/// （`installer::decode_console` 就是干这个的；别图省事 `from_utf8_lossy` 一把梭）。
fn cli_text(out: &std::process::Output) -> String {
    let mut s = installer::decode_console(&out.stdout);
    let err = installer::decode_console(&out.stderr);
    if !err.trim().is_empty() {
        if !s.trim().is_empty() {
            s.push('\n');
        }
        s.push_str(&err);
    }
    s
}

/// 只回传尾部约 `n` 个字符，并把换行压成空格。
///
/// magpie 报错常带一大段 usage，客户看得懂的是最后那句；前端 toast 是一行，不能炸成多行。
fn tail_chars(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    if chars.len() <= n {
        return flat;
    }
    chars[chars.len() - n..].iter().collect()
}

// ============================================================
// 工具行（五个 AI 各走哪一路）
// ============================================================

/// 五个 agent 的回读。
///
/// **并行**跑：五个 `magpie <agent> model` 互不依赖，串行最坏会把超时叠成 5×25 秒 ——
/// 打开设置页转两分钟，客户只会以为 U-King 卡死了。并行后最坏仍是一个超时周期。
fn read_tools(
    exe: Option<&Path>,
    cfg: &[(String, Option<String>, Option<String>)],
) -> Vec<ToolRoute> {
    // 小结构体而不是五元组：`Vec<(&str, &str, &str, Option<&str>, Option<&str>)>` 这种
    // 类型 clippy 的 type_complexity 会报，而且读的人得数到第五个才知道哪列是什么。
    let tasks: Vec<ToolTask> = AGENTS
        .iter()
        .map(|(target, agent, label)| {
            let hit = cfg.iter().find(|(t, _, _)| t == target);
            ToolTask {
                target,
                agent,
                label,
                fallback_model: hit.and_then(|(_, m, _)| m.as_deref()),
                fallback_base: hit.and_then(|(_, _, b)| b.as_deref()),
            }
        })
        .collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = tasks
            .iter()
            .map(|t| {
                s.spawn(move || {
                    tool_route(exe, t.target, t.agent, t.label, t.fallback_model, t.fallback_base)
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

/// 一行「读这个 AI 现在走哪一路」的任务描述（AGENTS 三列 + 组合根给的兜底值）。
struct ToolTask<'a> {
    target: &'static str,
    agent: &'static str,
    label: &'static str,
    fallback_model: Option<&'a str>,
    fallback_base: Option<&'a str>,
}

/// 单个工具行的判据顺序：CLI 回读 → 各工具自己的配置 → 不知道。
fn tool_route(
    exe: Option<&Path>,
    target: &str,
    agent: &str,
    label: &str,
    fallback_model: Option<&str>,
    fallback_base: Option<&str>,
) -> ToolRoute {
    let mut t = ToolRoute {
        target: target.to_string(),
        label: label.to_string(),
        agent: agent.to_string(),
        ..Default::default()
    };

    // ① CLI 回读最权威：magpie 自己写进各工具配置的那份，它说是什么就是什么。
    if let Some(exe) = exe {
        if let Some(raw) = read_agent_model_with(exe, agent) {
            let model = strip_magpie_prefix(&raw);
            t.route = normalize_route(&model);
            t.model = Some(model);
            t.source = "cli".to_string();
            t.readable = true;
            return t;
        }
    }

    // ② 兜底：用组合根读到的各工具配置。此时只有两种结论配得上「知道」：
    //    - 模型名本身就是路由（claude/pi 这类会把它存进 model 字段）；
    //    - base_url 压根不指向本机网关 —— 那就明确没走 magpie 路由。
    //    其余一律 readable=false：base 指向网关但模型名看不出是哪一路，就是不猜。
    let base = fallback_base.unwrap_or("");
    let model = fallback_model.map(strip_magpie_prefix);
    t.route = model.as_deref().and_then(normalize_route);
    // 三样都没有 = 这个工具的配置我们压根没读到，只能报「不知道」。
    let has_any = fallback_model.is_some() || fallback_base.is_some();
    t.readable = has_any && (t.route.is_some() || !base_is_magpie(base));
    t.model = model;
    t.source = if has_any { "config".to_string() } else { "none".to_string() };
    t
}

// ============================================================
// 纯函数（都有单测；不许依赖本机有没有装 magpie）
// ============================================================

/// 配置目标 → magpie agent id。两者目前同名，也要走这张表：
/// 哪天某个工具的 agent id 变了，只改 `AGENTS` 一处就够。
pub fn agent_of(target: &str) -> Option<&'static str> {
    AGENTS.iter().find(|(t, _, _)| *t == target).map(|(_, a, _)| *a)
}

fn label_of(agent: &str) -> Option<&'static str> {
    AGENTS.iter().find(|(_, a, _)| *a == agent).map(|(_, _, l)| *l)
}

fn is_known_agent(agent: &str) -> bool {
    AGENTS.iter().any(|(_, a, _)| *a == agent)
}

/// 从 `magpie <agent> model` 的输出里取回读值。
///
/// 真实输出形如：
/// ```text
/// Claude Code model group/smart-split
/// Pi model magpie/group/smart-split
/// Claude Code model default
/// ```
/// 人话名里带空格，所以不能按「第几个 token」硬切：按 `model` 这个字段名定位，
/// 再取它后面的最后一个 token。
///
/// 为什么还留一层「标签没对上也算」：标签是 magpie 打印的文案，它换文案时硬报「读不到」
/// 在 UI 上等于「没配」，会骗客户。但 usage 里的占位符（`<model>`、`[value]`）和
/// `error:` / `warning:` 这类行必须拒掉 —— 把帮助文本当成模型名比读不到更坏。
pub fn parse_agent_model(label: &str, out: &str) -> Option<String> {
    let label = label.trim();
    let mut loose: Option<String> = None;
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        // 报错/用法行不是回读行（`error: model not found` 会撞上下面的字段名定位）
        match words.first() {
            Some(w) if !w.ends_with(':') => {}
            _ => continue,
        }
        let Some(pos) = words.iter().position(|w| w.eq_ignore_ascii_case("model")) else {
            continue;
        };
        // 字段名前面必须有人话名、后面必须真有值
        if pos == 0 || pos + 1 >= words.len() {
            continue;
        }
        let value = words[words.len() - 1];
        if !looks_like_model_value(value) {
            continue;
        }
        if !label.is_empty() && words[..pos].join(" ").eq_ignore_ascii_case(label) {
            return Some(value.to_string());
        }
        if loose.is_none() {
            loose = Some(value.to_string());
        }
    }
    loose
}

/// 值像不像真的模型名：拒掉 usage 占位符。
fn looks_like_model_value(v: &str) -> bool {
    !v.is_empty() && !v.contains('<') && !v.contains('>') && !v.starts_with('[')
}

/// 把模型名归一成 `group/x` / `auto/x`；不是路由就 None。
///
/// 剥 `magpie/` 是因为同一路在不同地方叫法不同：pi 这类 agent 的配置里存
/// `magpie/group/x`，网关和 UI 里就是 `group/x`。不归一的话「刚切过去」会被判成
/// 没生效，客户会看到一条假的失败提示。
pub fn normalize_route(model: &str) -> Option<String> {
    let m = model.trim();
    let m = m.strip_prefix("magpie/").unwrap_or(m);
    let (kind, id) = m.split_once('/')?;
    if kind != "group" && kind != "auto" {
        return None;
    }
    let id = id.trim();
    // 空 id（`group/`）和带空白的 id 切过去必然失败，提前在 Rust 里拒掉。
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        return None;
    }
    Some(format!("{kind}/{id}"))
}

/// 剥掉 magpie 的命名空间前缀（只剥一层；`magpie/magpie/x` 不会被一路剥光）。
fn strip_magpie_prefix(model: &str) -> String {
    let m = model.trim();
    m.strip_prefix("magpie/").unwrap_or(m).to_string()
}

/// 网关 `/v1/models` → 路由列表。只留 `group/` `auto/` 开头的，**group 排前面**。
///
/// 排序理由：auto 可能有几十条，混在组列表里客户根本找不到自己建的组；UI 那边
/// 「自动路由」是默认折叠的，所以这里只保证 group 先、同类内部保持网关原顺序（稳定排序）。
pub fn parse_gateway_models(json: &str) -> Vec<RouteOption> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(list) = v.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in list {
        let Some(id) = m.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        let Some(kind) = route_kind(id) else {
            continue;
        };
        let name = m
            .get("display_name")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(id);
        let levels = m
            .get("reasoning_levels")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        out.push(RouteOption {
            model: id.to_string(),
            name: name.to_string(),
            kind: kind.to_string(),
            levels,
            members: Vec::new(),
        });
    }
    out.sort_by_key(|r| if r.kind == "group" { 0 } else { 1 });
    out
}

/// `providers.json` → 路由列表（离线兜底）。只认 `groups[]`，形状 `{id,name,members,routing}`。
pub fn parse_config_groups(json: &str) -> Vec<RouteOption> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(list) = v.get("groups").and_then(|g| g.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in list {
        let Some(id) = g
            .get("id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let name = g
            .get("name")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(id);
        let members = g
            .get("members")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        out.push(RouteOption {
            // 配置文件里只有组；自动路由是 magpie 运行时拼出来的，不在这个文件里。
            model: format!("group/{id}"),
            name: name.to_string(),
            kind: "group".to_string(),
            levels: Vec::new(),
            members,
        });
    }
    out
}

/// `group/` / `auto/` 前缀识别（半截 id 不算）。
fn route_kind(id: &str) -> Option<&'static str> {
    if id.len() > "group/".len() && id.starts_with("group/") {
        Some("group")
    } else if id.len() > "auto/".len() && id.starts_with("auto/") {
        Some("auto")
    } else {
        None
    }
}

/// 这个 base_url 是不是指向本机 magpie 网关。
///
/// 端口必须整段相等：`...:34250` 是另一个端口，用 `contains` 会把它误判成网关，
/// 然后给客户一个假的「正在走 magpie 路由」结论。
pub fn base_is_magpie(base: &str) -> bool {
    let b = base.trim().to_ascii_lowercase();
    const HOSTS: &[&str] = &["127.0.0.1:3425", "localhost:3425", "[::1]:3425"];
    HOSTS.iter().any(|host| {
        // 端口后面只允许是路径/查询/结尾（同上面 doc 的 `:34250` 那个坑）
        b.match_indices(*host).any(|(i, _)| {
            !matches!(b[i + host.len()..].chars().next(), Some(c) if c.is_ascii_digit())
        })
    })
}

/// `RouteState` 的唯一拼装点。网关字符串也只许在这里写一次 ——
/// 同一事实存在几份就会漂几份（宪法第 8 条），前端不该看见两个不一样的 gateway。
pub fn assemble(
    installed: bool,
    running: bool,
    routes: Vec<RouteOption>,
    routes_source: &str,
    tools: Vec<ToolRoute>,
    error: Option<String>,
) -> RouteState {
    RouteState {
        installed,
        running,
        gateway: GATEWAY.to_string(),
        routes,
        tools,
        routes_source: routes_source.to_string(),
        error,
    }
}

// ============================================================
// 平台差异：怎么把 magpie 窗口拉起来
// ============================================================

/// 从可执行文件反推 macOS 的 `.app` 目录
/// （`…/Magpie.app/Contents/MacOS/magpie` → `…/Magpie.app`）。
///
/// 拆成纯函数是为了能单测：这条路径规则错了，Mac 上「打开 magpie」永远没反应，
/// 而本机没装 magpie 的开发机连试都试不出来。
#[cfg(target_os = "macos")]
fn bundle_dir_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("app"))
                .unwrap_or(false)
        })
        .map(Path::to_path_buf)
}

/// Windows：与 desktop_app 打开桌面版 magpie 的口径一致 —— GUI 版带 `app` 参数才会弹窗口。
/// 模块之间不许互相 import，口径靠这条注释对齐（改那边记得回来看这里）。
#[cfg(windows)]
fn open_exe(exe: &Path) -> Result<(), String> {
    cli_command(exe)
        .arg("app")
        .spawn()
        .map_err(|e| format!("打开 magpie 失败：{e}"))?;
    Ok(())
}

/// macOS：优先 `open -a <bundle>` —— 交给 LaunchServices 按 app 名义启动，
/// 比直接 spawn 包内二进制可靠；从 PATH 找到的 CLI（没有 bundle）退化成无参启动，
/// 按 magpie 自己的帮助说明就是「打开窗口」。
#[cfg(target_os = "macos")]
fn open_exe(exe: &Path) -> Result<(), String> {
    let spawned = if let Some(bundle) = bundle_dir_of(exe) {
        std::process::Command::new("open").arg("-a").arg(bundle).spawn()
    } else {
        std::process::Command::new(exe).spawn()
    };
    spawned.map_err(|e| format!("打开 magpie 失败：{e}"))?;
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn open_exe(_exe: &Path) -> Result<(), String> {
    Err("当前平台暂不支持直接打开 magpie".to_string())
}

// ============================================================
// 单测
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── 夹具：形状取自本机只读实测（`magpie <agent> model` / 网关 `/v1/models` /
    //    `providers.json` 的字段名与嵌套都是真的），模型名、组名、成员全换成一眼假的
    //    demo 占位 —— 公开仓里不许出现真实订阅/客户端信息。

    #[test]
    fn parse_agent_model_reads_last_token_after_model_field() {
        // 三种真实形态：无前缀路由、带 magpie/ 前缀、非路由的 default
        assert_eq!(
            parse_agent_model("Claude Code", "Claude Code model group/demo-group"),
            Some("group/demo-group".to_string())
        );
        assert_eq!(
            parse_agent_model("Pi", "Pi model magpie/group/demo-group"),
            Some("magpie/group/demo-group".to_string())
        );
        assert_eq!(
            parse_agent_model("Claude Code", "Claude Code model default"),
            Some("default".to_string())
        );
        // 非路由的普通模型名（客户自己换过模型）也要如实读出来，UI 才能显示「不是路由」
        assert_eq!(
            parse_agent_model("Codex", "Codex model demo-vendor/demo-model"),
            Some("demo-vendor/demo-model".to_string())
        );
        // 末尾换行 / 前后空行不能影响
        assert_eq!(
            parse_agent_model("DeepSeek Harness", "\nDeepSeek Harness model magpie/group/x\n\n"),
            Some("magpie/group/x".to_string())
        );
        // 带方括号的模型名是真实存在的（如 `opus[1m]`），不能当占位符拒掉
        assert_eq!(
            parse_agent_model("Claude Code", "Claude Code model demo-model[fast]"),
            Some("demo-model[fast]".to_string())
        );
    }

    #[test]
    fn parse_agent_model_matches_label_case_insensitively() {
        // magpie 换大小写不该让我们判成「读不到」
        assert_eq!(
            parse_agent_model("claude code", "Claude Code model group/x"),
            Some("group/x".to_string())
        );
    }

    #[test]
    fn parse_agent_model_falls_back_when_label_changed() {
        // 标签是 magpie 的文案，它改了名字时宁可多认一层：读不到在 UI 上等于「没配」，
        // 那是骗客户；读错一个值反而会被回读校验挡下来。
        assert_eq!(
            parse_agent_model("Pi", "Rename Later Agent model group/x"),
            Some("group/x".to_string())
        );
    }

    #[test]
    fn parse_agent_model_rejects_usage_and_garbage() {
        // usage 占位符、报错行、空输出都必须是 None —— 把帮助文本当模型名比读不到更坏
        for (label, out) in [
            ("Claude Code", "Usage: magpie <agent> model <model>"),
            ("Claude Code", "error: model not found"),
            ("Claude Code", "warning: model list empty"),
            ("Claude Code", ""),
            ("Claude Code", "   \n  \n"),
            ("Claude Code", "model group/x"), // 字段名前面没人话名：不是回读行
            ("Claude Code", "Claude Code model"), // 字段名后面没值
        ] {
            assert_eq!(parse_agent_model(label, out), None, "这段不该解析出模型: {out:?}");
        }
    }

    #[test]
    fn normalize_route_accepts_both_prefix_styles() {
        assert_eq!(normalize_route("group/demo"), Some("group/demo".to_string()));
        assert_eq!(normalize_route("auto/demo-model"), Some("auto/demo-model".to_string()));
        assert_eq!(normalize_route("magpie/group/demo"), Some("group/demo".to_string()));
        assert_eq!(normalize_route(" magpie/auto/demo-model "), Some("auto/demo-model".to_string()));
        // `magpie/` 只剥一层
        assert_eq!(normalize_route("magpie/magpie/group/x"), None);
    }

    #[test]
    fn normalize_route_rejects_non_routes() {
        for bad in [
            "",
            "   ",
            "default",
            "demo-vendor/demo-model",
            "group/",
            "auto/",
            "magpie/",
            "group/demo group", // id 里带空格：切过去必然失败
            "GROUP/demo",       // magpie 的 id 是小写
            "groupx/demo",
        ] {
            assert_eq!(normalize_route(bad), None, "这个值不该被当成路由: {bad:?}");
        }
    }

    #[test]
    fn parse_gateway_models_keeps_only_routes_group_first() {
        let json = r#"{
            "object": "list",
            "data": [
                {"id": "auto/demo-model", "display_name": "演示自动路由", "reasoning_levels": ["low", "high"]},
                {"id": "group/demo-group", "display_name": "演示路由组", "reasoning_levels": []},
                {"id": "demo-vendor/demo-model-a", "display_name": "不是路由，得滤掉"},
                {"id": "group/", "display_name": "半截 id，得滤掉"},
                {"id": "auto/demo-no-name"},
                {"id": "group/demo-blank-name", "display_name": "   "},
                {"id": 42, "display_name": "id 不是字符串"}
            ]
        }"#;
        let got = parse_gateway_models(json);
        let models: Vec<&str> = got.iter().map(|r| r.model.as_str()).collect();
        assert_eq!(
            models,
            vec![
                "group/demo-group",
                "group/demo-blank-name",
                "auto/demo-model",
                "auto/demo-no-name"
            ],
            "group 必须排在 auto 前面，且非路由项一个都不许混进来"
        );
        assert_eq!(got[0].name, "演示路由组");
        assert_eq!(got[0].levels.len(), 0);
        // 网关来源没有 members（成员的权威数据在 providers.json 里）
        assert!(got[0].members.is_empty());
        assert_eq!(got[1].name, "group/demo-blank-name", "display_name 是空白时退回 id");
        assert_eq!(got[2].name, "演示自动路由");
        assert_eq!(got[2].levels, vec!["low".to_string(), "high".to_string()]);
        assert_eq!(got[2].kind, "auto");
        assert_eq!(got[3].name, "auto/demo-no-name");
        // 网关给的路由必须自洽：模型名再归一一次还是它自己（前端拿它去切，不能切丢）
        for r in &got {
            assert_eq!(normalize_route(&r.model).as_deref(), Some(r.model.as_str()));
        }
    }

    #[test]
    fn parse_gateway_models_survives_broken_body() {
        assert!(parse_gateway_models("").is_empty());
        assert!(parse_gateway_models("<html>proxy intercepted</html>").is_empty());
        assert!(parse_gateway_models(r#"{"data": "not-an-array"}"#).is_empty());
        assert!(parse_gateway_models(r#"{"error": {"message": "boom"}}"#).is_empty());
    }

    #[test]
    fn parse_config_groups_reads_groups_array() {
        let json = r#"{
            "providers": [],
            "groups": [
                {"id": "demo-one", "name": "演示组一", "members": ["demo/a", "demo/b"], "routing": "order"},
                {"id": "demo-two", "members": []},
                {"id": "", "name": "没有 id 的组得跳过"},
                {"name": "只有名字没有 id，也得跳过"},
                {"id": "demo-three", "name": "  ", "members": ["demo/c", 42, ""]}
            ]
        }"#;
        let got = parse_config_groups(json);
        let models: Vec<&str> = got.iter().map(|r| r.model.as_str()).collect();
        assert_eq!(
            models,
            vec!["group/demo-one", "group/demo-two", "group/demo-three"],
            "id 为空的组不能进列表"
        );
        assert_eq!(got[0].name, "演示组一");
        assert_eq!(got[0].members, vec!["demo/a".to_string(), "demo/b".to_string()]);
        // 没有 name 的组用 id 顶上，UI 不能出现空白名字
        assert_eq!(got[1].name, "demo-two");
        assert!(got[1].members.is_empty());
        assert_eq!(got[2].name, "demo-three", "name 是空白时退回 id");
        // 成员里混进非字符串 / 空串要滤掉，否则前端 chips 会渲染出空白
        assert_eq!(got[2].members, vec!["demo/c".to_string()]);
        assert!(got.iter().all(|r| r.kind == "group" && r.levels.is_empty()));
    }

    #[test]
    fn parse_config_groups_survives_broken_body() {
        assert!(parse_config_groups("").is_empty());
        assert!(parse_config_groups("{}").is_empty());
        assert!(parse_config_groups(r#"{"groups": "oops"}"#).is_empty());
        assert!(parse_config_groups(r#"{"providers": []}"#).is_empty());
    }

    #[test]
    fn pick_exe_prefers_first_hit() {
        let a = PathBuf::from("/opt/demo/a/magpie");
        let b = PathBuf::from("/opt/demo/b/magpie");
        // 顺序就是优先级：先到的赢，后面的（哪怕更有名）不许抢
        assert_eq!(pick_exe(vec![None, Some(a.clone()), Some(b.clone())]), Some(a.clone()));
        assert_eq!(pick_exe(vec![Some(b.clone())]), Some(b));
        assert_eq!(pick_exe(vec![None, None]), None);
        assert_eq!(pick_exe(Vec::new()), None);
    }

    #[test]
    fn base_is_magpie_matches_loopback_gateway_only() {
        for hit in [
            "http://127.0.0.1:3425",
            "http://127.0.0.1:3425/v1",
            "http://localhost:3425/v1",
            "http://[::1]:3425",
            "HTTP://LOCALHOST:3425/",
        ] {
            assert!(base_is_magpie(hit), "这个 base 应该判成 magpie 网关: {hit}");
        }
        for miss in [
            "",
            "https://api.example.com/v1",
            "http://127.0.0.1:34250", // 34250 是另一个端口，整段不相等
            "http://127.0.0.1:3426",
            "http://gateway.example.com:3425",
        ] {
            assert!(!base_is_magpie(miss), "这个 base 不该判成 magpie 网关: {miss}");
        }
    }

    #[test]
    fn agent_of_covers_exactly_the_five_tools() {
        assert_eq!(agent_of("claude"), Some("claude"));
        assert_eq!(agent_of("dsh"), Some("dsh"));
        assert_eq!(agent_of("hermes"), Some("hermes"));
        // ClawX / OpenCode 本版不支持 —— 面板里不出现，也不假装能切
        assert_eq!(agent_of("opencode"), None);
        assert_eq!(agent_of("clawx"), None);
        assert_eq!(agent_of(""), None);
    }

    #[test]
    fn read_tools_without_cli_uses_config_fallback_in_agents_order() {
        // exe=None = 只走兜底，不起任何子进程（和真机无关，任何机器都能跑）。
        // 这条同时守住「五个工具一行不少、顺序跟 AGENTS 一致」—— 前端是按 target 找行的。
        let cfg = vec![
            (
                "claude".to_string(),
                Some("group/demo".to_string()),
                Some("http://127.0.0.1:3425".to_string()),
            ),
            (
                "codex".to_string(),
                Some("demo-model".to_string()),
                Some("https://api.example.com/v1".to_string()),
            ),
        ];
        let got = read_tools(None, &cfg);
        let targets: Vec<&str> = got.iter().map(|t| t.target.as_str()).collect();
        assert_eq!(
            targets,
            vec!["claude", "codex", "dsh", "pi", "hermes"],
            "工具行必须按 AGENTS 顺序给全，前端按下标/target 取"
        );
        assert_eq!(got[0].route.as_deref(), Some("group/demo"));
        assert_eq!(got[0].label, "Claude Code");
        assert_eq!(got[1].model.as_deref(), Some("demo-model"));
        assert!(got[1].readable, "base 不指向网关 = 明确没走路由");
        for t in &got[2..] {
            assert_eq!(t.source, "none", "{} 的配置没给，来源只能是 none", t.target);
            assert!(!t.readable, "{} 没有数据时必须报不知道", t.target);
        }
    }

    #[test]
    fn tool_route_prefers_cli_style_model_without_spawning() {
        // exe = None 模拟「CLI 读不到」：这条路径不发任何子进程，能在开发机安全跑。
        let t = tool_route(None, "claude", "claude", "Claude Code", Some("group/demo"), Some("http://127.0.0.1:3425"));
        assert_eq!(t.route.as_deref(), Some("group/demo"));
        assert_eq!(t.model.as_deref(), Some("group/demo"));
        assert!(t.readable);
        assert_eq!(t.source, "config");
        assert_eq!(t.target, "claude");
        assert_eq!(t.label, "Claude Code");
    }

    #[test]
    fn tool_route_says_unknown_when_gateway_but_no_route_name() {
        // 确实在走网关（base 命中），但配置里的模型名看不出是哪一路 —— 不许猜，报不知道
        let t = tool_route(None, "dsh", "dsh", "DeepSeek Harness", Some("demo-model"), Some("http://127.0.0.1:3425"));
        assert_eq!(t.route, None);
        assert!(!t.readable, "指向网关但认不出路由时必须 readable=false");
        // 模型值仍然给出去（UI 可以当线索显示，但不许渲染成「没配」）
        assert_eq!(t.model.as_deref(), Some("demo-model"));
        assert_eq!(t.source, "config");
    }

    #[test]
    fn tool_route_says_not_routed_when_base_is_elsewhere() {
        let t = tool_route(None, "codex", "codex", "Codex", Some("demo-model"), Some("https://api.example.com/v1"));
        assert_eq!(t.route, None);
        assert!(t.readable, "base 不指向网关 = 明确没走路由，这是「知道」不是「不知道」");
        assert_eq!(t.model.as_deref(), Some("demo-model"));
        assert_eq!(t.source, "config");
    }

    #[test]
    fn tool_route_without_any_source_is_unknown_not_unconfigured() {
        let t = tool_route(None, "pi", "pi", "Pi", None, None);
        assert_eq!(t.source, "none");
        assert!(!t.readable);
        assert_eq!(t.model, None);
        assert_eq!(t.route, None);
    }

    #[test]
    fn tool_route_strips_magpie_prefix_from_fallback_model() {
        let t = tool_route(None, "pi", "pi", "Pi", Some("magpie/group/demo"), Some(""));
        assert_eq!(t.model.as_deref(), Some("group/demo"));
        assert_eq!(t.route.as_deref(), Some("group/demo"));
        assert!(t.readable);
    }

    #[test]
    fn verify_readback_reports_the_real_value_not_success() {
        // 回读一致才叫成功；`magpie/` 前缀与不带前缀都算同一路
        assert!(verify_readback("group/demo", Some("group/demo")).is_ok());
        assert!(verify_readback("group/demo", Some("magpie/group/demo")).is_ok());
        // 不一致必须 Err，且把两边都写进人话里（客户才知道该找谁）
        let e = verify_readback("group/demo", Some("group/other")).unwrap_err();
        assert!(e.contains("group/demo") && e.contains("group/other"), "错误里要同时有目标和实际值: {e}");
        let e = verify_readback("group/demo", Some("default")).unwrap_err();
        assert!(e.contains("default"), "回读到非路由值也要如实报出来: {e}");
        // 压根读不到：不许说成功
        assert!(verify_readback("group/demo", None).is_err());
    }

    #[test]
    fn verify_reset_only_checks_it_is_no_longer_a_route() {
        // 还原后是什么模型我们不知道（magpie 把原来那份还回去），能给的最低判据是「不再是路由」
        assert!(verify_reset("claude", Some("default")).is_ok());
        assert!(verify_reset("claude", Some("demo-vendor/demo-model")).is_ok());
        assert!(verify_reset("claude", None).is_err(), "读不到就不能说还原成功");
        let e = verify_reset("claude", Some("magpie/group/demo")).unwrap_err();
        assert!(e.contains("group/demo"), "还读得到路由就是没还原，得报出来: {e}");
    }

    #[test]
    fn apply_rejects_bad_input_before_touching_the_machine() {
        // 这两条必须在 spawn 之前返回：模型名不合法 / agent 不认识时，
        // 一个 magpie 进程都不该起（测试也因此能在任何机器上跑）。
        let e = apply("claude", "demo-vendor/demo-model").unwrap_err();
        assert!(e.contains("group/") && e.contains("auto/"), "错误里要告诉客户能填什么: {e}");
        let e = apply("no-such-agent", "group/demo").unwrap_err();
        assert!(e.contains("no-such-agent"), "错误里要带上不认识的名字: {e}");
        let e = reset("no-such-agent").unwrap_err();
        assert!(e.contains("no-such-agent"));
        // 空白模型名同样在本地拒掉
        assert!(apply("claude", "   ").is_err());
    }

    #[test]
    fn read_agent_model_ignores_unknown_agents_without_spawning() {
        // 不认识的 agent 在 `label_of` 那步就返回 None，一个子进程都不起 ——
        // 所以这条在任何机器上都跑得动，也顺手守住「别对野生 agent 名乱调 magpie」。
        assert_eq!(read_agent_model("no-such-agent"), None);
    }

    #[test]
    fn assemble_fills_gateway_once_and_keeps_order() {
        let routes = vec![RouteOption {
            model: "group/demo".to_string(),
            name: "演示组".to_string(),
            kind: "group".to_string(),
            ..Default::default()
        }];
        let tools = vec![ToolRoute {
            target: "claude".to_string(),
            label: "Claude Code".to_string(),
            agent: "claude".to_string(),
            source: "cli".to_string(),
            readable: true,
            ..Default::default()
        }];
        let s = assemble(true, false, routes, "config", tools, Some("演示错误".to_string()));
        assert!(s.installed);
        assert!(!s.running);
        assert_eq!(s.gateway, GATEWAY);
        assert_eq!(s.routes_source, "config");
        assert_eq!(s.routes.len(), 1);
        assert_eq!(s.routes[0].model, "group/demo");
        assert_eq!(s.tools.len(), 1);
        assert_eq!(s.tools[0].label, "Claude Code");
        assert_eq!(s.error.as_deref(), Some("演示错误"));
        // 默认值形状：前端拿到 null 时不该崩
        let empty = assemble(false, false, Vec::new(), "none", Vec::new(), None);
        assert_eq!(empty.gateway, GATEWAY);
        assert_eq!(empty.routes_source, "none");
        assert!(empty.error.is_none());
    }

    #[test]
    fn tail_chars_keeps_the_end_of_a_multiline_error() {
        // CLI 报错常带 usage，客户看得懂的是最后一句；换行压成空格省得 toast 炸行
        let s = "usage: demo\nusage: demo\n最后一句才是原因";
        let t = tail_chars(s, 4);
        assert_eq!(t, "才是原因", "应当只留尾部字符");
        assert!(!t.contains('\n'));
        // 短文本原样返回（含中间空格与换行折叠）
        assert_eq!(tail_chars("a\nb", 100), "a b");
        assert_eq!(tail_chars("", 10), "");
    }

    #[test]
    fn strip_magpie_prefix_is_single_layer() {
        assert_eq!(strip_magpie_prefix("magpie/group/x"), "group/x");
        assert_eq!(strip_magpie_prefix("group/x"), "group/x");
        assert_eq!(strip_magpie_prefix(" magpie/auto/y "), "auto/y");
        assert_eq!(strip_magpie_prefix("magpie/magpie/group/x"), "magpie/group/x");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bundle_dir_of_finds_the_app_before_the_binary() {
        let exe = PathBuf::from("/Applications/Magpie.app/Contents/MacOS/magpie");
        assert_eq!(bundle_dir_of(&exe), Some(PathBuf::from("/Applications/Magpie.app")));
        // 从 PATH 找来的裸 CLI 没有 bundle：调用方据此走「无参启动」那条分支
        assert_eq!(bundle_dir_of(Path::new("/opt/homebrew/bin/magpie")), None);
    }
}
