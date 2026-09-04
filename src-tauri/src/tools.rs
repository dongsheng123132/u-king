//! 可选 AI 工具清单 —— 管理界面里的「工具市场」。
//!
//! 简化版定位：管家本体是个轻壳，AI 工具按需再装。这里维护一张静态目录，
//! 每个工具知道：
//! - 怎么判断「已装」（探测一个标志路径）
//! - 怎么「安装」（v0.1 先用打开官方安装指引 / 已有脚本的方式，避免在管家里重造下载器）
//!
//! 真正的重型下载（Node 运行时 + OpenClaw ~400MB）留给各工具自己的安装脚本，
//! 与复杂版 toolkit 的 install-windows.bat 一脉相承，这里只做「入口聚合 + 状态展示」。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 一个可安装工具的元信息（回前端渲染卡片）。
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    /// 稳定 id
    pub id: String,
    /// 展示名
    pub name: String,
    /// 一句话简介
    pub summary: String,
    /// 接入深度：deep（深度接入）/ standalone（装机即走）
    pub kind: String,
    /// 是否已在本机检测到
    pub installed: bool,
    /// 安装动作类型：script（跑脚本）/ url（打开网页指引）
    pub action: String,
    /// 安装目标（script 时是脚本路径占位，url 时是网址）
    pub target: String,
    /// 已装后的启动命令（CLI 工具，如 "claude"）。空 = 无独立启动命令。
    /// 前端「打开终端」按钮会打开终端并把这个命令显示/预填给用户。
    pub launch_cmd: String,
    /// GUI 应用启动标识（如 "codex-app" / "clawx"）。非空 = 这是图形程序，
    /// 前端「打开应用」按钮调 `launch_app` 直接启动程序，不进终端。
    pub launch_app: String,
    /// 隐藏入口：true = 不在工具市场/Dock 露出（产品收窄到 4 个核心工具），
    /// 但后端检测/切驱动/启动能力全保留（存量已装用户照常切换/启动）。
    /// 当前隐藏：Codex CLI（走桌面版）、OpenClaw 官方 CLI（走 ClawX 桌面版）。
    #[serde(default)]
    pub hidden: bool,
}

/// 工具注册表的单一真相源（Phase C，2026-09-04）。
///
/// 在这之前「某工具是否存在/怎么探测/怎么显示」分散在四份互相独立的清单里（本文件的
/// `list_tools()`、`providers.rs` 的 `LIST_TOOLS`/`discover_tools_from`、`toolprobe.rs` 的
/// `PROBES`、`lib.rs` 的 `collect_ai_checkup_items`），上新工具（cline）时漏了后两处——
/// 体检和无头探测都不认识它。这张表把「一个工具有哪些身份」收成一行数据，其余三处
/// 从它派生，不再各自维护一份平行清单。
///
/// `id` 必须与 [`list_tools()`] 构造出的每个 `ToolInfo.id` 一一对应（`tool_specs_tests`
/// 单测锁住这条，任何一边加/删工具忘了同步另一边，`cargo test` 就会红）。
pub struct ToolSpec {
    /// 稳定 id，等于 `list_tools()` 里同一工具的 `ToolInfo.id`（如 `"claude-code"`）。
    pub id: &'static str,
    /// 探测/搜索用的可执行文件名。**不一定等于 `id`**——例如 `clawx`（ClawX 桌面版）
    /// 复用的 CLI 可执行文件叫 `openclaw`，不叫 `clawx`；`claude-code`/`qwen-code` 这两个
    /// id 是产品页文案，可执行文件其实是 `claude`/`qwen`。纯 GUI、没有独立 CLI 概念的工具
    /// （Obsidian、UU远程…）留空串，不承担探测语义。
    pub cmd: &'static str,
    /// 归属哪个驱动配置目标（`providers.rs` 里 `apply_provider`/`effective_config` 认的
    /// `target` 字符串）。`None` = 不接驱动切换体系（体检类/纯下载类工具）。
    pub config_target: Option<&'static str>,
    /// 是否进 `providers::LIST_TOOLS`（= 前端「AI 设置」页有没有它独立的 Tab）。
    pub in_list_tools: bool,
    /// 是否进「一键体检」清单；`Some(展示 label)` 时才进，label 就是体检卡片上显示的名字。
    pub in_checkup: Option<&'static str>,
    /// 无头探测参数（`toolprobe.rs` 用）：
    /// - `None` —— 不在探测范围内（不是 CLI，或压根没接入这条跑道，如 `dsh`/Obsidian）。
    /// - `Some(&[])` —— **在探测范围内，但没有可靠的一次性无头入口**（如 `openclaw`：
    ///   它的正常用法是 `gateway run` + 面板，不是一次性推理；硬测会把「我们没测对」
    ///   报成「它坏了」）。`toolprobe.rs` 见到空切片会如实标注「无头入口，测不了」，
    ///   而不是把这个工具从探测结果里彻底抹掉——「没测」和「不存在」是两件事。
    /// - `Some(非空)` —— 真的会拿这组参数去跑一次。
    pub probe_args: Option<&'static [&'static str]>,
    /// 启动判定核心（Phase D，2026-09-04）用：这个工具「该怎么起」。派生依据见各条目内联注释
    /// （引用了 `App.tsx::launchTool` / `ToolAppView.tsx` / `apps.ts::TUI_APPS` 里实际读到的现状，
    /// 不是凭空指定）。`LaunchMode::None` = 没有可执行入口（从开始菜单/桌面图标打开）。
    pub launch_mode: LaunchMode,
    /// `launch_mode == RouteTab` 时，前端要切去哪个 tab（对应 `App.tsx::setTab` 的参数）。
    /// 其余模式下为 `None`。
    pub route_tab: Option<&'static str>,
}

/// 一个工具「该怎么起」——`runtime.tool.inspect`/`runtime.tool.launch` 判定核心的分派方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    /// 独立 GUI 应用（如 ClawX 桌面版）：Rust 直接 `launch_app` 拉起。
    GuiApp,
    /// 一次性/诊断类 CLI，没有专属内嵌终端 tab：Rust 走 `term_open_external` 开系统终端窗口。
    ExternalTerm,
    /// 普通 CLI，有专属内嵌终端 tab：前端拿 `launch_cmd` 自己去内嵌 xterm 跑。
    EmbeddedPty,
    /// 有自己一整套专属页面/时序逻辑的工具（openclaw/hermes/dsh）：前端只需切到那个 tab，
    /// 该 tab 自己的启动逻辑（ToolAppView::handleStart）继续负责，Rust 判定核心不重复实现。
    RouteTab,
    /// 纯打开网址（当前 `TOOL_SPECS` 里没有工具落这一档；`ToolInfo.action=="url"` 的下载类
    /// 走 `openTool`，不经过这条启动判定核心，保留这个变体只是对齐设计文档的分类，防止
    /// 以后真出现「点了就该打开一个网址」的启动需求时要再加一次这套判定逻辑）。
    Url,
    /// 没有可执行入口（GUI 得从开始菜单/桌面图标自己开，我们帮不了）。
    None,
}

/// 一个工具「能不能起」的状态——顺序即语义，`plan()` 按这个顺序判定，见该函数文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStatus {
    Ready,
    NotInstalled,
    NotFoundInPath,
    RejectedCmd,
    NoLauncher,
}

/// `plan()` 的可注入依赖：把「这个工具在 TOOL_SPECS 里能查到的静态形状」和「运行时判据」
/// 分开——`plan()` 本身不碰 TOOL_SPECS/磁盘/PATH，全部由调用方（生产路径 `plan_for`，
/// 或测试）算好传进来。`None` = 在生产路径里表示「这个 tool_id 在 TOOL_SPECS 里找不到」。
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub cmd: String,
    pub launch_cmd: String,
    pub mode: LaunchMode,
    pub route: Option<String>,
}

/// 一次启动判定的完整结果——`runtime.tool.inspect`/`runtime.tool.launch` 的核心产物，
/// 序列化后直接是这两个动作的（部分）输出。
#[derive(Debug, Clone, Serialize)]
pub struct LaunchPlan {
    pub tool_id: String,
    pub cmd: String,
    pub installed: bool,
    pub resolved_path: Option<String>,
    pub source: String,
    /// `term.rs::build_path()` 展开的 PATH 上能不能解析到 `cmd`——这是「显示已装、点了没
    /// 反应」的真正判据（工具装在 `search_paths` 之外的某处，`installed` 判 true，但终端
    /// 起的子进程用的是注入过的 PATH，那条 PATH 上未必找得到它）。
    pub on_terminal_path: bool,
    pub mode: LaunchMode,
    pub route: Option<String>,
    pub launch_cmd: String,
    pub cmd_allowed: bool,
    pub status: LaunchStatus,
    pub blockers: Vec<String>,
}

/// 判定核心：给定一个工具的静态形状（`spec`）和运行时判据（其余参数），算出它现在到底
/// 「能不能起、该怎么起」。**纯函数**——不碰 `TOOL_SPECS`、不碰磁盘、不碰 PATH，全部依赖
/// 由调用方注入，因此可以在不碰真实文件系统的前提下用假数据覆盖全部 5 种 [`LaunchStatus`]。
/// 生产路径 [`plan_for`] 负责把真实判据算好再调用这里。
///
/// status 判定顺序**即语义**，不能调换：
/// 1. `spec` 是 `None`（这个 `tool_id` 不在 `TOOL_SPECS` 里）→ `Err`；
/// 2. `!installed` → `NotInstalled`；
/// 3. 这个 `mode` 需要走终端（`EmbeddedPty`/`ExternalTerm`/`RouteTab`——`GuiApp`/`Url`/`None`
///    不需要，`GuiApp` 靠 `launch_app` 直接拉起，不查 PATH）且 `!on_terminal_path` →
///    `NotFoundInPath`；
/// 4. `launch_cmd` 非空且 `!cmd_allowed`（`term::validate_cmd` 的结果）→ `RejectedCmd`；
/// 5. `mode == LaunchMode::None` → `NoLauncher`；
/// 6. 否则 → `Ready`。
pub fn plan(
    tool_id: &str,
    spec: Option<LaunchSpec>,
    installed: bool,
    on_terminal_path: bool,
    cmd_allowed: bool,
    resolved_path: Option<String>,
    source: &str,
) -> Result<LaunchPlan, String> {
    let Some(spec) = spec else {
        return Err(format!("invalid_input: 未知的 tool_id '{tool_id}'（不在 TOOL_SPECS 里）"));
    };
    let needs_terminal_path = matches!(
        spec.mode,
        LaunchMode::EmbeddedPty | LaunchMode::ExternalTerm | LaunchMode::RouteTab
    );
    let mut blockers: Vec<String> = Vec::new();
    let status = if !installed {
        blockers.push(format!("还没检测到「{tool_id}」已安装，请先安装。"));
        LaunchStatus::NotInstalled
    } else if needs_terminal_path && !on_terminal_path {
        blockers.push(format!(
            "检测到「{tool_id}」已安装，但终端环境的 PATH 里找不到它，需要重新装到默认位置或修复 PATH。"
        ));
        LaunchStatus::NotFoundInPath
    } else if !spec.launch_cmd.is_empty() && !cmd_allowed {
        blockers.push(format!("启动命令「{}」被安全校验拒绝，这是我们的 bug，请反馈。", spec.launch_cmd));
        LaunchStatus::RejectedCmd
    } else if spec.mode == LaunchMode::None {
        blockers.push(format!("「{tool_id}」没有可执行入口，请从开始菜单/桌面图标打开。"));
        LaunchStatus::NoLauncher
    } else {
        LaunchStatus::Ready
    };
    Ok(LaunchPlan {
        tool_id: tool_id.to_string(),
        cmd: spec.cmd.clone(),
        installed,
        resolved_path,
        source: source.to_string(),
        on_terminal_path,
        mode: spec.mode,
        route: spec.route.clone(),
        launch_cmd: spec.launch_cmd.clone(),
        cmd_allowed,
        status,
        blockers,
    })
}

/// [`plan`] 的生产路径包装：真的去查 `TOOL_SPECS`/`list_tools()`/PATH/`validate_cmd`。
/// 单个工具版本，`plan_all` 遍历 `TOOL_SPECS` 复用它。
pub fn plan_for(tool_id: &str) -> Result<LaunchPlan, String> {
    let spec = TOOL_SPECS.iter().find(|s| s.id == tool_id).map(|s| {
        let info = list_tools().into_iter().find(|t| t.id == tool_id);
        let (launch_cmd, cmd_installed) = match &info {
            Some(t) => (t.launch_cmd.clone(), t.installed),
            None => (String::new(), false),
        };
        (
            LaunchSpec {
                cmd: s.cmd.to_string(),
                launch_cmd,
                mode: s.launch_mode,
                route: s.route_tab.map(|r| r.to_string()),
            },
            cmd_installed,
        )
    });
    let (spec, installed) = match spec {
        Some((s, i)) => (Some(s), i),
        None => (None, false),
    };
    let launch_cmd = spec.as_ref().map(|s| s.launch_cmd.clone()).unwrap_or_default();
    let resolved_path = if launch_cmd.is_empty() {
        None
    } else {
        let prog = launch_cmd.split_whitespace().next().unwrap_or("");
        crate::term::resolve_on_terminal_path(prog)
    };
    let on_terminal_path = resolved_path.is_some();
    let source = if on_terminal_path { "terminal_path".to_string() } else { String::new() };
    let cmd_allowed = launch_cmd.is_empty() || crate::term::validate_cmd(&launch_cmd);
    plan(tool_id, spec, installed, on_terminal_path, cmd_allowed, resolved_path, &source)
}

/// [`plan_for`] 遍历全部 `TOOL_SPECS`——`runtime.tool.inspect` 用。已知 id 一定命中
/// （来自 `TOOL_SPECS` 本身），这里的 `Result` 只是复用同一个签名，不会真的走 `Err` 分支。
pub fn plan_all() -> Vec<LaunchPlan> {
    TOOL_SPECS
        .iter()
        .filter_map(|s| plan_for(s.id).ok())
        .collect()
}

/// Cline 的探测 prompt：**不能是塞进一个 argv 位的整句**。本机实测（`src/opencodex/apps.ts`
/// 同名条目注释记录）它拿 commander 解析位置参数，单 token（哪怕内部带空格、但只占一个
/// argv 位）会被当成未知子命令拒绝，必须拆成多个 argv token 传。这里手工拆开
/// `toolprobe::PROMPT` 的词，不在运行时切（一次性写死更直白，也避免运行时分词规则跑偏）。
///
/// 🔴 **没有本机真机验证过这条命令** —— `toolprobe.rs` 文件头的规矩是「先手工把命令跑通
/// 再往表里加，否则会把『我们命令写错了』报成『这个工具坏了』」，这条是记录在案的例外：
/// 写这张表时环境里没装 Cline，没法验证。发版前务必先手工跑一次再信这份数据。
const CLINE_PROBE_ARGS: &[&str] = &[
    "--json",
    "Reply",
    "with",
    "exactly:",
    crate::toolprobe::MARKER,
];

pub const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        id: "claude-code",
        cmd: "claude",
        config_target: Some("claude"),
        in_list_tools: true,
        in_checkup: Some("Claude Code"),
        probe_args: Some(&["-p", crate::toolprobe::PROMPT]),
        // apps.ts::TUI_APPS 里有专属 tab（id "claude"），Dock 点它走 ToolAppView 内嵌终端；
        // launch_mode 与该现状对齐。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        id: "codex",
        cmd: "codex",
        config_target: Some("codex"),
        in_list_tools: true,
        in_checkup: Some("Codex"),
        probe_args: Some(&["exec", crate::toolprobe::PROMPT]),
        // apps.ts::TUI_APPS 专属 tab id 是 "codex-cli"（toolId: "codex"）—— 有内嵌终端。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        // CLI 版 OpenClaw：探测/装机走这条 id，但驱动配置跟桌面版 `clawx` 共用同一个
        // target（见 `apps.ts` 里 `configTargets: ["clawx"]` 的同款注释）。
        id: "openclaw",
        cmd: "openclaw",
        config_target: Some("clawx"),
        in_list_tools: false,
        in_checkup: None,
        // 见上面 `probe_args` 字段文档：openclaw 没有可靠的一次性无头入口，
        // 空切片 = 「在探测范围内，但测不了」，不是「不存在」。
        probe_args: Some(&[]),
        // App.tsx::launchTool 硬编码分支：`if (t.id === "openclaw") { setTab("openclaw"); return; }`
        // —— 有自己的专属页（ToolAppView 走 gateway 起停 + WebUI 时序），不能当普通 EmbeddedPty。
        launch_mode: LaunchMode::RouteTab,
        route_tab: Some("openclaw"),
    },
    ToolSpec {
        id: "qwen-code",
        cmd: "qwen",
        config_target: Some("qwen"),
        in_list_tools: false,
        in_checkup: Some("Qwen Code"),
        probe_args: Some(&["-p", crate::toolprobe::PROMPT]),
        // apps.ts::TUI_APPS 专属 tab id 是 "qwen"（toolId: "qwen-code"）—— 有内嵌终端。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        // ClawX 桌面版：GUI，装没装走 `providers::clawx_app_installed()`（不是 `cmd` 探测），
        // 但它复用的 CLI 可执行文件确实叫 `openclaw`——`discover_tools_from` 找「clawx 这个
        // 驱动 target 在 PATH 上对应哪个文件」时要用这个 `cmd`，不能拿 id 直接当文件名猜。
        id: "clawx",
        cmd: "openclaw",
        config_target: Some("clawx"),
        in_list_tools: true,
        in_checkup: Some("ClawX"),
        // GUI，没有 CLI 一次性无头入口，从来没进过 `PROBES`。
        probe_args: None,
        // list_tools() 里 launch_app="clawx"（GUI 应用，doLaunchApp 直接 invoke launch_app）。
        launch_mode: LaunchMode::GuiApp,
        route_tab: None,
    },
    ToolSpec {
        id: "hermes",
        cmd: "hermes",
        config_target: Some("hermes"),
        in_list_tools: true,
        in_checkup: Some("Hermes"),
        probe_args: Some(&["-z", crate::toolprobe::PROMPT]),
        // App.tsx::launchTool 硬编码分支：`if (t.id === "hermes") { setTab("hermes"); return; }`
        // —— ToolAppView 把它当 external 应用处理（apps.ts TUI_APPS 的 hermes 条目 external:true），
        // 启动时还要按需配虾盘云（ensureWebToolConfigured），不能当普通 EmbeddedPty。
        launch_mode: LaunchMode::RouteTab,
        route_tab: Some("hermes"),
    },
    ToolSpec {
        id: "dsh",
        cmd: "dsh",
        config_target: Some("dsh"),
        in_list_tools: true,
        in_checkup: Some("DeepSeek Harness"),
        // dsh 从来没进过 `PROBES`（人类主入口是 Web 工作台，不是一次性无头推理）。
        probe_args: None,
        // App.tsx::launchTool 硬编码分支：`if (t.id === "dsh") { setTab("dsh"); return; }`
        // —— ToolAppView 的 launchDshWebUI/launchDshTerminal 有专属等待就绪时序，不能当普通
        // EmbeddedPty。
        launch_mode: LaunchMode::RouteTab,
        route_tab: Some("dsh"),
    },
    // 🔴 下面 pi/opencode/crush/cline 在 `TOOL_SPECS` 里的相对顺序不是随意的：
    // `providers::list_tools_targets()` 按本表原有顺序过滤派生 `LIST_TOOLS`，而
    // `apply_everywhere_contract_lists_every_target_the_backend_configures` 用例要求
    // 派生结果的**顺序**严格等于历史上手写的 `["claude","codex","clawx","hermes","dsh",
    // "pi","opencode","cline"]`（跟 `APPLY_ALL_TARGETS` 字面量顺序对齐）。这里的顺序
    // 就是照那份历史顺序摆的，挪动会让那条用例返工——不是巧合，动之前先看那条用例。
    ToolSpec {
        id: "pi",
        cmd: "pi",
        config_target: Some("pi"),
        in_list_tools: true,
        in_checkup: Some("pi"),
        probe_args: Some(&["-p", crate::toolprobe::PROMPT]),
        // apps.ts::TUI_APPS 专属 tab id 是 "pi" —— 有内嵌终端。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        id: "opencode",
        cmd: "opencode",
        config_target: Some("opencode"),
        in_list_tools: true,
        in_checkup: Some("OpenCode"),
        probe_args: Some(&["run", crate::toolprobe::PROMPT]),
        // apps.ts::TUI_APPS 专属 tab id 是 "opencode" —— 有内嵌终端。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        id: "crush",
        cmd: "crush",
        config_target: Some("crush"),
        in_list_tools: false,
        in_checkup: Some("Crush"),
        probe_args: Some(&["run", crate::toolprobe::PROMPT]),
        // apps.ts::TUI_APPS 专属 tab id 是 "crush" —— 有内嵌终端。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        id: "cline",
        cmd: "cline",
        config_target: Some("cline"),
        in_list_tools: true,
        in_checkup: Some("Cline"),
        probe_args: Some(CLINE_PROBE_ARGS),
        // apps.ts::TUI_APPS 专属 tab id 是 "cline" —— 有内嵌终端。
        launch_mode: LaunchMode::EmbeddedPty,
        route_tab: None,
    },
    ToolSpec {
        id: "harness-doctor",
        cmd: "harness-doctor",
        config_target: None,
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // 有 launch_cmd（list_tools() 里 "harness-doctor --target all --no-ports"），但不在
        // apps.ts::TUI_APPS 里、没有专属内嵌终端 tab —— 一次性诊断脚本，当前 App.tsx::launchTool
        // 对它走的正是通用兜底分支 `invoke("term_open_external", ...)`，与此对齐。
        launch_mode: LaunchMode::ExternalTerm,
        route_tab: None,
    },
    ToolSpec {
        id: "obsidian",
        cmd: "",
        config_target: None,
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // list_tools() 里 launch_cmd=""、launch_app=""，action="url" 跳官网下载页
        // ——App.tsx::launchTool 命中 `if (!t.launch_cmd) { flash(...); return; }`，没有可执行入口。
        launch_mode: LaunchMode::None,
        route_tab: None,
    },
    ToolSpec {
        id: "uu-remote",
        cmd: "",
        config_target: None,
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // 同 obsidian：launch_cmd=""、launch_app=""，只能跳官网下载页。
        launch_mode: LaunchMode::None,
        route_tab: None,
    },
    ToolSpec {
        // Codex 桌面版：config.toml 跟 Codex CLI 共用同一份，`config_target` 复用 "codex"，
        // 但没有独立的「AI 设置」Tab（跟 CLI 共用），体检/探测也都挂在 CLI 那条 id 下。
        id: "codex-app",
        cmd: "",
        config_target: Some("codex"),
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // list_tools() 里 launch_app="codex-app"（GUI，doLaunchApp 直接 invoke launch_app）。
        launch_mode: LaunchMode::GuiApp,
        route_tab: None,
    },
    ToolSpec {
        id: "open365",
        cmd: "",
        config_target: None,
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // list_tools() 里 launch_app="open365"。
        launch_mode: LaunchMode::GuiApp,
        route_tab: None,
    },
    ToolSpec {
        id: "hermes-app",
        cmd: "",
        config_target: Some("hermes"),
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // list_tools() 里 launch_app="hermes-app"。
        launch_mode: LaunchMode::GuiApp,
        route_tab: None,
    },
    ToolSpec {
        id: "uu-switch",
        cmd: "",
        config_target: None,
        in_list_tools: false,
        in_checkup: None,
        probe_args: None,
        // list_tools() 里 launch_app="uu-switch"。
        launch_mode: LaunchMode::GuiApp,
        route_tab: None,
    },
];

#[cfg(test)]
mod tool_specs_tests {
    use super::*;
    use std::collections::BTreeSet;

    /// `TOOL_SPECS` 的 id 集合必须和 `list_tools()` 实际构造出的 `ToolInfo.id` 集合完全一致
    /// ——加/删一个工具，两边有一边忘了同步，这条测试就会红，而不是等到运行时才发现
    /// 体检/探测清单里少了一个工具。
    ///
    /// 注：`list_tools()` 里 codex-app/open365/hermes-app/uu-switch 四项挂在
    /// `#[cfg(windows)]`/`#[cfg(any(windows, target_os = "macos"))]` 后面，`TOOL_SPECS`
    /// 目前没有对应套 cfg（const 数组内按元素条件编译不方便）。这条测试只在实际跑它的平台
    /// 上生效——本仓库 `cargo test` 目前只在 Windows 机器上跑，跟这四项的 cfg 覆盖一致；
    /// 如果以后要在纯 Linux CI 上跑这条测试，需要把 `TOOL_SPECS` 也拆成按平台 cfg 的子表。
    #[test]
    fn tool_specs_ids_match_list_tools_ids() {
        let list_ids: BTreeSet<String> = list_tools().into_iter().map(|t| t.id).collect();
        let spec_ids: BTreeSet<String> =
            TOOL_SPECS.iter().map(|s| s.id.to_string()).collect();
        assert_eq!(
            list_ids, spec_ids,
            "TOOL_SPECS 的 id 集合必须和 list_tools() 构造出的 ToolInfo id 集合完全一致"
        );
    }
}

#[cfg(test)]
mod launch_plan_tests {
    use super::*;

    fn spec(mode: LaunchMode, route: Option<&str>, launch_cmd: &str) -> LaunchSpec {
        LaunchSpec {
            cmd: "some-cmd".to_string(),
            launch_cmd: launch_cmd.to_string(),
            mode,
            route: route.map(|r| r.to_string()),
        }
    }

    /// 状态判定第 1 条：tool_id 不在 TOOL_SPECS 里（这里用 `spec = None` 模拟）→ Err。
    #[test]
    fn unknown_tool_id_is_err() {
        let result = plan("no-such-tool", None, false, false, true, None, "");
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("invalid_input"), "错误信息应带 invalid_input 前缀：{msg}");
    }

    /// 状态判定第 2 条：未安装 → NotInstalled，即便其余判据都是"通过"的。
    #[test]
    fn not_installed_wins_even_if_everything_else_ok() {
        let s = spec(LaunchMode::EmbeddedPty, None, "claude");
        let p = plan("claude-code", Some(s), false, true, true, None, "").unwrap();
        assert_eq!(p.status, LaunchStatus::NotInstalled);
        assert!(!p.blockers.is_empty());
    }

    /// 状态判定第 3 条：已安装，但需要终端环境的模式（EmbeddedPty/ExternalTerm/RouteTab）
    /// 在 PATH 上找不到 → NotFoundInPath。这是要重点锁的场景：装在非默认位置，
    /// installed=true（比如别的探测方式认出来了），但 on_terminal_path=false。
    #[test]
    fn installed_but_not_on_terminal_path_is_not_found_in_path() {
        let s = spec(LaunchMode::EmbeddedPty, None, "claude");
        let p = plan(
            "claude-code",
            Some(s),
            true,
            false,
            true,
            Some("C:/some/weird/place/claude.exe".to_string()),
            "custom_probe",
        )
        .unwrap();
        assert_eq!(p.status, LaunchStatus::NotFoundInPath);
    }

    /// GuiApp 模式不需要终端 PATH——即便 on_terminal_path=false，也不该被判成 NotFoundInPath。
    #[test]
    fn gui_app_mode_does_not_require_terminal_path() {
        let s = spec(LaunchMode::GuiApp, None, "");
        let p = plan("clawx", Some(s), true, false, true, None, "").unwrap();
        assert_eq!(p.status, LaunchStatus::Ready);
    }

    /// 状态判定第 4 条：launch_cmd 非空但被 validate_cmd 拒绝 → RejectedCmd。
    #[test]
    fn rejected_cmd_when_validate_cmd_fails() {
        let s = spec(LaunchMode::EmbeddedPty, None, "rm -rf /");
        let p = plan("claude-code", Some(s), true, true, false, None, "").unwrap();
        assert_eq!(p.status, LaunchStatus::RejectedCmd);
    }

    /// launch_cmd 为空时不校验 cmd_allowed（空字符串不该被当成"被拒绝的命令"）。
    #[test]
    fn empty_launch_cmd_skips_cmd_allowed_check() {
        let s = spec(LaunchMode::GuiApp, None, "");
        let p = plan("clawx", Some(s), true, true, false, None, "").unwrap();
        assert_ne!(p.status, LaunchStatus::RejectedCmd);
    }

    /// 状态判定第 5 条：mode == None（没有任何启动方式）→ NoLauncher。
    #[test]
    fn no_launcher_when_mode_is_none() {
        let s = spec(LaunchMode::None, None, "");
        let p = plan("obsidian", Some(s), true, true, true, None, "").unwrap();
        assert_eq!(p.status, LaunchStatus::NoLauncher);
    }

    /// 全部判据通过 → Ready，且 route/launch_cmd 原样透传。
    #[test]
    fn all_checks_pass_is_ready() {
        let s = spec(LaunchMode::RouteTab, Some("hermes"), "hermes");
        let p = plan(
            "hermes",
            Some(s),
            true,
            true,
            true,
            Some("C:/tools/hermes.cmd".to_string()),
            "terminal_path",
        )
        .unwrap();
        assert_eq!(p.status, LaunchStatus::Ready);
        assert_eq!(p.route, Some("hermes".to_string()));
        assert_eq!(p.launch_cmd, "hermes");
        assert_eq!(p.mode, LaunchMode::RouteTab);
    }

    /// RouteTab 模式也需要终端 PATH（hermes/dsh 底层还是要能在终端里解析到命令）。
    #[test]
    fn route_tab_mode_requires_terminal_path() {
        let s = spec(LaunchMode::RouteTab, Some("dsh"), "dsh");
        let p = plan("dsh", Some(s), true, false, true, None, "").unwrap();
        assert_eq!(p.status, LaunchStatus::NotFoundInPath);
    }
}

fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".uking")
}

/// ClawX 官方国内下载源（阿里云 OSS）。版本会变，**不能硬编码版本号文件名**
/// —— 官方发新版会删旧文件（实测 0.4.10 → 0.4.11 后旧链 NoSuchKey/404，客户装不上）。
/// 这里拉 `release-info.json` 动态取当前平台直链；拉不到才回退到一个兜底版本。
const CLAWX_RELEASE_MANIFEST: &str = "https://oss.intelli-spectrum.com/latest/release-info.json";
/// 兜底直链：网络/解析失败时用。**发现客户报 404 就把这里更到 release-info.json 的当前版本**。
const CLAWX_FALLBACK_URL: &str = "https://oss.intelli-spectrum.com/latest/ClawX-0.4.11-win-x64.exe";

/// Hermes 桌面版（Nous Research 官方 Electron app）Windows 安装器直链。
/// 重要：① 域名 nousresearch.com 是**国际站**，国内裸网可能慢/不稳 —— 下载失败一律回退
/// 「打开官网下载页」让用户自己下，绝不卡死（进阶区工具，不进小白一键流）。
/// ② `?build=` 哈希官方升版会变（暂无可动态取的 release manifest），发现 404 就更新这里。
/// ③ 这个 .exe 只是 ~7.5MB 下载器壳，真正运行时(Electron + Python/Node/ffmpeg + hermes runtime)
///    首次启动才联网拉到 `%LOCALAPPDATA%\hermes` —— 所以慢网客户「装完首启动很慢」属正常，非 bug。
const HERMES_APP_URL: &str =
    "https://hermes-assets.nousresearch.com/Hermes-Setup.exe?build=9362ce2575e0";
/// 下载失败时打开的官网下载页（让用户自己点平台对应的下载按钮）。
const HERMES_APP_DOWNLOAD_PAGE: &str = "https://hermes-agent.nousresearch.com/";

/// UU远程（原网易 GameViewer 远程，网易官方出品）下载页。手机/平板/另一台电脑远控这台机器 ——
/// 定位「出门在外用手机盯着 AI 在电脑上干活、随时接管」。官方站 uuyc.163.com，全平台客户端都在这。
/// action=url 跳官网下载页（不在 app 内装，装法各平台不同，官网自己选平台最稳）。
const UU_REMOTE_DOWNLOAD_PAGE: &str = "https://uuyc.163.com/download/";

/// UU远程安装包的**稳定**下载入口（网易官方 release API，302 到当次直链）。
///
/// 🔴 别硬编码 302 之后那条直链：实测形如
/// `a56.gdl.netease.com/UURemote_Setup_4.34.0.8979_0723104500_gwqd.exe?key1=…&key2=…`，
/// **既带版本号又带签名参数**，官方升个版或签名过期就 404 —— 只能存这个 API 端点，让 curl -L 自己跟。
/// `dl/1` = Windows、`dl/4` = macOS（从官网下载页里的 `pc_link` 变量实测取得，2026-07-29 核对）。
///
/// ⚠️ **UU远程没有绿色版 / 免安装版**（官网下载页逐项核对过，只有各平台安装包），所以「点一下就能用」
/// 做不到，能做到的极限是「帮你下好 + 尽量静默装上」—— 别在文案上吹成绿色版。
const UU_REMOTE_DL_WIN: &str = "https://api.nrd.nie.163.com/api/v1/release/dl/1?channel=gwqd";

/// Windows 安装包实测大小（2026-07-29：89,696,744 字节 / v4.34.0）。只用来算下载进度百分比
/// 和「下太小 = 下到错误页了」的下限，官方升版体积浮动不影响正确性。
const UU_REMOTE_WIN_MB: u64 = 86;

/// UU远程 Windows 端是否已装。
///
/// 🔴 **这个函数错一次，用户就装两遍。** 2026-07-29 干净 Windows 云机实测：`/S` 静默安装
/// 明明成功了（服务在跑、文件已落地），但当时只认扁平的 `%ProgramFiles%\GameViewer`，
/// 而官方**实际**装在 `%ProgramFiles%\Netease\GameViewer\`（**厂商名多一层**）——
/// 于是 `install_uu_remote` 轮询 60 秒探不到 → 误判静默失败 → 又拉起一个可视安装器
/// 挂在那等人点。反馈页也会永远显示「帮我下载安装」、永远不显示「已安装」。
/// 同一类坑 macOS 上踩过一次（只认中文 app 名，见下方 `#[cfg(not(windows))]` 分支）。
///
/// 所以现在两条口径，任一命中即算已装：
/// ① 目录 —— 带 `Netease\` 厂商层的真实落点，外加老版本/其它渠道的扁平名兜底；
/// ② **卸载表** —— Windows 认定「装没装」的权威处，也是 `cleanup.rs` 卸载时找的同一处。
///    装到非系统盘 / 自定义目录时 ① 会漏，② 兜得住。
#[cfg(windows)]
fn uu_remote_installed() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
    // 实测落点在前，扁平名在后（老版本 / 非官方渠道兜底）。
    let dirs = [
        "Netease\\GameViewer",
        "Netease\\UURemote",
        "网易UU远程",
        "UU远程",
        "GameViewer",
        "UURemote",
    ];
    let roots = [
        Path::new(&local).join("Programs"),
        PathBuf::from(&pf),
        PathBuf::from(&pf86),
    ];
    for root in &roots {
        for d in &dirs {
            if root.join(d).is_dir() {
                return true;
            }
        }
    }

    // ② 卸载表兜底。实测键名是 `GameViewer`（DisplayName = 「UU远程」）。
    for hive in [
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ] {
        for key in ["GameViewer", "UURemote"] {
            let hit = std::process::Command::new(crate::installer::system_tool("reg"))
                .args(["query", &format!("{hive}\\{key}"), "/v", "DisplayName"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if hit {
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
fn uu_remote_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        // macOS 端官方 app 包名实测是英文 `UURemote.app`（一台 Mac 客户机实锤，
        // 装了却被 U-King 报「没安装成功」）。中文名 `网易UU远程.app`/`UU远程.app` 是照搬
        // Windows 目录名，Mac 上根本不存在 → 只认中文名会把「明明装了」误判成没装。
        // 旧名 GameViewer 一并认；`UUBooster.app` 是网易 UU 加速器（另一款），不算。
        for app in ["UURemote.app", "网易UU远程.app", "UU远程.app", "GameViewer.app"] {
            if Path::new("/Applications").join(app).exists() {
                return true;
            }
        }
        return false;
    }
    #[allow(unreachable_code)]
    false
}

/// 动态取 ClawX 当前平台的官方下载直链。失败兜底到 CLAWX_FALLBACK_URL（绝不返回坏链）。
/// 平台键：Windows→win.x64、macOS arm→mac.arm64、macOS x86→mac.x64。
pub fn clawx_download_url() -> String {
    // release-info.json 需带 User-Agent，否则 OSS 可能返回空响应（见下载源 memory）。
    let raw = crate::installer::curl(&[
        "-sS",
        "-m",
        "20",
        "-A",
        "U-King/1.0",
        CLAWX_RELEASE_MANIFEST,
    ]);
    let url = raw.ok().and_then(|s| {
        let v: serde_json::Value = serde_json::from_str(&s).ok()?;
        let dl = v.get("downloads")?;
        #[cfg(target_os = "macos")]
        let key = if std::env::consts::ARCH == "aarch64" {
            ("mac", "arm64")
        } else {
            ("mac", "x64")
        };
        #[cfg(not(target_os = "macos"))]
        let key = ("win", "x64");
        dl.get(key.0)?.get(key.1)?.as_str().map(String::from)
    });
    url.filter(|u| u.starts_with("https://"))
        .unwrap_or_else(|| CLAWX_FALLBACK_URL.to_string())
}

/// OpenClaw 是否已装（探测 ~/.uclaw 或 ~/.uking/tools/openclaw）。
fn openclaw_installed() -> bool {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let p1 = Path::new(&home)
        .join(".uclaw")
        .join("core")
        .join("node_modules")
        .join("openclaw");
    let p2 = uking_home().join("tools").join("openclaw");
    p1.exists() || p2.exists()
}

fn tool_dir_installed(sub: &str) -> bool {
    uking_home().join("tools").join(sub).exists()
}

/// Obsidian 是否已装。Windows 默认按用户装到 `%LOCALAPPDATA%\Obsidian\Obsidian.exe`，
/// 「为所有用户装」会落 Program Files；Mac 是 `/Applications/Obsidian.app`。
fn obsidian_installed() -> bool {
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        return Path::new(&local).join("Obsidian").join("Obsidian.exe").exists()
            || Path::new(&pf).join("Obsidian").join("Obsidian.exe").exists();
    }
    #[cfg(not(windows))]
    {
        Path::new("/Applications/Obsidian.app").exists()
    }
}


// =====================================================================
//  Open365（开源电脑管家 · 独立可插拔）
//  —— 无广告替代「安全卫士」类工具：网络修复 / 垃圾清理 / 启动项 / 强力卸载 /
//     安全护盾(开 Windows 自带 Defender+防火墙+更新) / 守夜模式(AI 通宵不熄屏)。
//  它是独立小工具（PowerShell 引擎 + 系统 csc 编译的 WinForms 壳，~200KB），
//  U-King 只做「检测 + 一键装到本地 + 拉起」，不与其耦合。
//  删除本集成：回退 tools.rs 这几处 + pack-usb.sh 的 Open365 拷贝行即可（≤2 文件）。
//  仅 Windows（纯 PowerShell + WinForms，Mac/Linux 不适用）。
// =====================================================================

/// 装到本地后的常驻目录：`~/.uking/tools/open365`。
fn open365_local_dir() -> PathBuf {
    uking_home().join("tools").join("open365")
}

/// 找 Open365 源：优先本地已装目录；否则找运行 exe 同级的 `Open365/`（U 盘随盘带）。
/// 判据是「有 Open365.exe 或 install.ps1」——两者任一即可跑起来。
fn open365_source_dir() -> Option<PathBuf> {
    let local = open365_local_dir();
    if local.join("Open365.exe").exists() || local.join("install.ps1").exists() {
        return Some(local);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let usb = dir.join("Open365");
            if usb.join("Open365.exe").exists() || usb.join("install.ps1").exists() {
                return Some(usb);
            }
        }
    }
    None
}

/// Open365 联网下载源（U 盘没随盘带、本地也没装时的兜底）。open365.zip 里是运行所需文件
/// （Open365.exe / engine / gui / install.ps1 …）在根，解压到 `~/.uking/tools/open365` 即可跑。
#[cfg(windows)]
const OPEN365_ZIP_URLS: &[&str] = &[
    "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/open365.zip",
    "https://cloud.u-claw.org/download/open365.zip",
    "https://u-claw.org.cn/download/open365.zip",
];

/// 联网下载 Open365 到本地目录并解压（curl 拉 zip → tar.exe 解，PowerShell 兜底）。
/// 只在 U 盘没带、本地也没装时兜底（launch_open365 里调）。成功返回本地目录（已就绪）。
#[cfg(windows)]
fn download_open365() -> Result<PathBuf, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let local = open365_local_dir();
    std::fs::create_dir_all(&local).map_err(|e| format!("建目录失败: {e}"))?;
    let zip = std::env::temp_dir().join("open365-uking.zip");
    let _ = std::fs::remove_file(&zip);

    let mut got = false;
    for url in OPEN365_ZIP_URLS {
        let status = std::process::Command::new(crate::installer::system_tool("curl"))
            .args(["-fsSL", "-A", "Mozilla/5.0 U-King", "-m", "30", "-o", &zip.to_string_lossy(), url])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        // zip ~100KB；>20KB 才算下到真包（挡错误页/截断）
        if matches!(status, Ok(s) if s.success())
            && std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0) > 20_000
        {
            got = true;
            break;
        }
        let _ = std::fs::remove_file(&zip);
    }
    if !got {
        return Err("下载 Open365 失败（网络不通）。稍后重试，或从带 Open365 文件夹的 U 盘运行。".into());
    }

    // 解压到本地目录：优先系统 tar.exe（Win10+ 内置，稳），失败回退 PowerShell Expand-Archive。
    let tar_ok = std::process::Command::new(crate::installer::system_tool("tar"))
        .args(["-xf", &zip.to_string_lossy(), "-C", &local.to_string_lossy()])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !tar_ok {
        let ps = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            zip.to_string_lossy(),
            local.to_string_lossy()
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = std::fs::remove_file(&zip);

    if local.join("install.ps1").exists() || local.join("Open365.exe").exists() {
        Ok(local)
    } else {
        Err("Open365 解压后文件不完整，请重试。".into())
    }
}

/// 给 Command 加「不弹黑窗」（Windows CREATE_NO_WINDOW），其他平台原样。
trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}
impl NoWindow for std::process::Command {
    #[cfg(windows)]
    fn no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
    #[cfg(not(windows))]
    fn no_window(&mut self) -> &mut Self {
        self
    }
}

/// 返回工具目录（带「已装」状态）。
/// action = "install"：走对话式安装向导（skill 驱动，可真装）；"url"：打开官网指引。
pub fn list_tools() -> Vec<ToolInfo> {
    let mut v = vec![
        ToolInfo {
            id: "claude-code".into(),
            name: "Claude Code CLI".into(),
            summary: "Anthropic 官方命令行编程助手。一键安装 + 国内驱动直连。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("claude"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "claude".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // Codex CLI 上首页（2026-07-13 产品决策）：Codex CLI + Codex 桌面版两个都露出、都可装。
            // U-Workspace 委派编程首选 claude -p，codex exec 次之，两个 Codex 入口都要能装上。
            id: "codex".into(),
            name: "Codex CLI".into(),
            summary: "OpenAI 的本地编程 agent（命令行）。一键安装 + 国内驱动直连；U-Workspace 可委派调用。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("codex"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "codex".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // ★ 2026-08-03 复活（原 hidden=true，2026-07-07 定的「人类入口只留 ClawX 桌面版」）。
            // 那条决策的前提是「GUI 优先」；现在主推反过来了 —— CLI 干活、U-Workspace 当壳、
            // 抢工作台的 GUI 一律降级，于是 ClawX 桌面版让位，OpenClaw 的人类入口回到 CLI。
            // 「双配置面」那个老坑没有消失，但它从来不是靠藏入口解决的：靠的是切驱动 target
            // 统一写 "clawx"（providers.rs），两个入口读同一份配置。
            id: "openclaw".into(),
            name: "OpenClaw CLI（龙虾）".into(),
            summary: "原版 OpenClaw 命令行（龙虾）。终端里直接对话干活，也能起网页版网关；切驱动与 ClawX 共用同一份配置，不会打架。".into(),
            kind: "deep".into(),
            installed: crate::installer::tool_installed("openclaw") || openclaw_installed(),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "openclaw".into(),
            launch_app: "".into(),
            // ★ 2026-08-05 隐藏 CLI 入口。**两条一起看**：ClawX 桌面版（另一条 id）
            // 08-03 已降级隐藏，所以这一改之后 OpenClaw 生态在**主推面上不再出现**，
            // 只在「进阶 / App 版」页仍可达。这是有意的收窄，不是漏了一处。
            // 配置链一个字节没动（apps.ts 的 configTargets 仍指 "clawx"，apply/备份/
            // gateway 全在），已装的客户照配照用。
            hidden: true,
        },
        ToolInfo {
            // ★ 2026-08-03 新上架。本机实测四条门槛全过（详见 apps.ts 同名条目的注释）：
            // npm 12 包/19s · `~/.qwen/settings.json` 接虾盘云 · `qwen -p` exit 0 且 stdout 干净
            // · 只读工具调用默认审批档即通过。同轮被刷掉的：OpenCode（`run` 挂 90s 零输出）。
            id: "qwen-code".into(),
            name: "Qwen Code".into(),
            summary: "阿里通义开源的终端编程 agent（fork 自 Gemini CLI）。中文强、装得轻，一键接虾盘云；支持 `qwen -p` 非交互塞任务。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("qwen"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "qwen".into(),
            launch_app: "".into(),
            // 2026-08-05 隐藏：实测能用但 35.7s 全场最慢，且与主推线重叠
            hidden: true,
        },
        ToolInfo {
            // ★ 2026-08-03 上架。四条门槛实测全过，且同任务同模型下**上下文只有 Claude Code 的 1/5**
            // （5,000 vs 24,300 token）——这就是它"快"的全部来源，不是更聪明。详见 apps.ts 同名条目。
            id: "pi".into(),
            name: "pi".into(),
            summary: "轻量开源终端 agent。同样的模型，它塞进去的上下文只有别家的 1/5，所以更快更省；工具白名单可控。适合搭 deepseek-v4-flash 跑量。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("pi"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "pi".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // ★ 2026-08-03 上架，**仅 TUI**。社区最大的开源 coding agent（★192k），
            // 交互式界面成熟；但非交互 `opencode run` 本机三轮实测恒定挂死零输出
            // （详见 apps.ts / providers::apply_opencode 的注释），所以它不进竞技场。
            id: "opencode".into(),
            name: "OpenCode".into(),
            summary: "社区最大的开源终端编程 agent（★19 万）。交互式界面成熟、生态插件多，一键接虾盘云。注：装包 153MB、文件多，慢网要等一会儿；它的非交互模式在 Windows 上跑不起来，只当终端界面用。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("opencode"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "opencode".into(),
            launch_app: "".into(),
            // 2026-08-05 隐藏：与主推线重叠 + 153MB/18718 文件，慢网装得最久。
            // 2026-08-24 取消隐藏（用户点名：「在我的 ai 里边也要一个」）——
            // 🔴 隐藏它的代价一直没被算进来：`App.tsx` 是 `raw.filter(x => !x.hidden)` 全局过滤，
            // 所以**连已经装了 opencode 的用户也一个像素都看不到它**，既不能启动也不能卸载。
            // 「装得慢」是安装时才付的代价，不该换来「装完也找不到」。慢的那条已写进 summary
            // 里明说（153MB），让用户自己决定，而不是替他决定看不见。
            hidden: false,
        },
        ToolInfo {
            // ★ 2026-08-03 新上架。Charm 出品（★27k），Go 单二进制。实测：npm 48 包/7s、
            // `crush run` exit 0 且支持管道。配虾盘云有个必踩的坑写在 providers::apply_crush。
            id: "crush".into(),
            name: "Crush".into(),
            summary: "Charm 出品的终端 AI 助手，界面精致、启动快，支持管道（cat 文件 | crush run \"…\"）。一键接虾盘云。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("crush"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "crush".into(),
            launch_app: "".into(),
            // 2026-08-05 隐藏：已修好能用（12.1s），隐藏理由是与 Claude Code/Codex 重叠
            hidden: true,
        },
        ToolInfo {
            // OpenClaw 的「桌面版」= ClawX（官方 Electron GUI）。261MB，不在 app 内装，
            // action=url 跳官方国内 OSS 直链下载；装完 U-King 能检测(clawx_app_installed)
            // + 切驱动(providers::apply_clawx 写 ~/.openclaw/openclaw.json) + 一键启动(launch_app=clawx)。
            id: "clawx".into(),
            name: "OpenClaw 桌面版（ClawX）".into(),
            // ★ 2026-08-05 改文案。原文是「同一个脑子，功能完整，**日常用它即可**」——
            // 那是 08-03 降级之前的主推口径，降级后没跟着改，于是「不主推」的定位和
            // 「日常用它即可」的文案在同一个界面上打架，客户照文案理解就是我们在推它。
            // 新文案只说**它有而 Claude Code / Hermes 没有的那两样**（图形界面、微信/IM 接入），
            // 这才是它值得留在备选区的理由；重叠的部分（同一个脑子、功能完整）不再吆喝。
            summary: "OpenClaw 的官方图形版。**备选**：日常干活主推 Claude Code / Hermes（命令行更快更稳），ClawX 的独有价值是图形界面 + 微信等 IM 接入 —— 要用手机上的聊天软件指挥 AI 才装它。点开下载官方国内直链，一键接虾盘云。".into(),
            kind: "deep".into(),
            installed: crate::providers::clawx_app_installed(),
            action: "url".into(),
            // 列表里先放兜底直链（list_tools 是同步高频调用，不能在这拉网络）；
            // 真正下载时前端调 get_clawx_download_url 拿实时直链（动态读 release-info.json）。
            target: CLAWX_FALLBACK_URL.into(),
            launch_cmd: "".into(),
            launch_app: "clawx".into(),
            // ★ 2026-08-05 放回首页（撤销 08-03 的 hidden=true 降级）。
            //
            // 08-03 降级的理由是「抢工作台的第三方 GUI 一律撤下」，但那一刀把「不主推」
            // 执行成了「找不到」：ClawX 唯一的露出面变成「进阶 / App 版」页，而**有一批
            // 客户就是冲这个图形界面来的**（用户 08-05 明确：「有些人就用这个界面」）。
            // 让他们去翻「进阶」页才找得到自己天天用的东西，是把我们的主线偏好当成了
            // 他们的使用方式。
            //
            // 🔴 **「露出」和「默认装」是两件事，别再把它们绑在一起** —— 这正是 08-03
            // 那刀砍过头的原因。分工是：
            //   · 露不露 = 这里的 `hidden`（首页/工具市场/Dock 都读它）
            //   · 装不装 = `Wizard.tsx::pickTools` 里那份**写死的队列名单**
            // ClawX 不在那份名单里（0.9.85 就移出去了），所以 hidden=false **不会**让它
            // 被「一键全安装」拖进来装 261MB —— 它是 action:"url" 的卡片，客户自己点了
            // 才下载。0.9.85 定的「主推 Claude Code / Hermes，ClawX 备选」口径一字未变，
            // 它体现在 summary 文案和装机队列里，不靠藏入口。
            //
            // 检测/切驱动/托管式配置能力从头到尾一个都没动过。
            hidden: false,
        },
        ToolInfo {
            id: "hermes".into(),
            name: "Hermes Agent（Nous 官方）".into(),
            summary: "Nous Research 自进化 AI 智能体，打开就是终端对话界面（默认已接虾盘云）。官网安装器在国内 git 必败，U-King 用 pip 国内源直装。".into(),
            kind: "deep".into(),
            installed: crate::installer::tool_installed("hermes") || tool_dir_installed("hermes"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "hermes".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // DeepSeek 官方 Harness 仍处于 Developer Preview：安装清单锁定经过验证的 rc 版本，
            // 卡片只负责安装/检测/进入专属页。人类主入口是 Web 工作台；一次性自动化走 headless，
            // 不把它冒充成 Claude Code/Hermes 那种持续交互 TUI。
            id: "dsh".into(),
            name: "DeepSeek Harness（官方预览版）".into(),
            summary: "DeepSeek 官方智能体框架。U-King 一键安装并打开本地 Web 工作台；也支持 headless 一次性任务，当前锁定已验证的 0.1.0-rc.6。".into(),
            kind: "deep".into(),
            installed: crate::installer::tool_installed("dsh"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "dsh web".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // ★ 2026-08-29 上架（opus + sol 会审裁定）。四门槛 2026-08-29 沙箱实测全过：
            // npm 328 包/17s；auth 非交互接虾盘云；headless 真调 6.6s；工具调用真改文件。
            // 体量全场最大（328MB），summary 明写，让客户自己决定装不装（对齐 opencode 08-24
            // 的教训：「装得慢」是安装时才付的代价，不该换来「装完找不到」）。
            id: "cline".into(),
            name: "Cline CLI".into(),
            summary: "开源 AI 编程 agent（GitHub ★67k）。特色是自动化编排：headless JSON 任务流、定时任务、多项目并行看板。体量约 330MB，慢网安装要等一会儿；一键接虾盘云。".into(),
            kind: "standalone".into(),
            installed: crate::installer::tool_installed("cline"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "cline".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            id: "harness-doctor".into(),
            name: "Harness Doctor（AI 工具体检）".into(),
            summary: "只读检查 DeepSeek Harness、Claude Code、Codex 和 OpenClaw：版本、Node、配置、端口、PATH 冲突；可生成不含 Key 和用户名的脱敏支持包。".into(),
            kind: "utility".into(),
            installed: crate::installer::tool_installed("harness-doctor"),
            action: "install".into(),
            target: "".into(),
            launch_cmd: "harness-doctor --target all --no-ports".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // Obsidian：进阶工具，给想搭「个人知识库」的高级用户。本体是 markdown 笔记库，
            // 配合 ClawX / Hermes 读这个文件夹就成了「AI 能查的知识库」。不进一键全安装队列
            // （小白用不上 + 300MB），只在工具市场放卡片，action:url 跳官网下载，自己点开装。
            id: "obsidian".into(),
            name: "Obsidian 知识库".into(),
            summary: "本地 markdown 笔记库（进阶）。把资料/笔记存进它的文件夹，再让 ClawX/Hermes 指向该文件夹，就成了「AI 能查能答」的个人知识库。点开下载官网安装包。".into(),
            kind: "standalone".into(),
            installed: obsidian_installed(),
            action: "url".into(),
            target: "https://obsidian.md/download".into(),
            launch_cmd: "".into(),
            launch_app: "".into(),
            hidden: false,
        },
        ToolInfo {
            // UU远程（网易官方）：手机/平板/另一台电脑远控这台机器。定位「让 AI 在电脑上干活时，
            // 人在外面也能用手机随时盯着、随时接管」—— 培养「手机遥控电脑用 AI」的习惯 + 顺带做安装推荐。
            // action=url 跳官网下载页（全平台客户端都在那，官网选平台最稳）。
            id: "uu-remote".into(),
            name: "UU远程（手机控电脑）".into(),
            summary: "网易官方远控：手机/平板/另一台电脑随时连回这台机器。出门在外也能看 AI 在电脑上干活、随时接管操作。两端登同一账号即连，点开下载官网客户端。".into(),
            kind: "standalone".into(),
            installed: uu_remote_installed(),
            action: "url".into(),
            target: UU_REMOTE_DOWNLOAD_PAGE.into(),
            launch_cmd: "".into(),
            launch_app: "".into(),
            hidden: false,
        },
        // 核心工具：Claude Code CLI / Hermes CLI（命令行）
        // + Codex 桌面版 / ClawX 桌面版（图形）。Codex CLI、OpenClaw 官方 CLI 已标
        // hidden=true（上面），后端检测/切驱动/装机能力全保留，仅不在市场露出。
    ];
    // Codex 桌面版：Windows 一键装（微软商店渠道 + 国内镜像 MSIX 兜底）；
    // macOS 露出检测/启动/手动教程入口，安装走官方 DMG（不做假静默）。
    #[cfg(any(windows, target_os = "macos"))]
    v.insert(
        2,
        ToolInfo {
            id: "codex-app".into(),
            name: "Codex 桌面版".into(),
            summary: "OpenAI Codex 图形界面版，与 CLI 共用驱动。Windows 可一键装；Mac 走官方 DMG 手动安装。中文需在 Settings → Language 设，部分账号暂只能英文。".into(),
            kind: "standalone".into(),
            installed: crate::installer::codex_app_installed(),
            action: if cfg!(windows) { "install" } else { "url" }.into(),
            target: if cfg!(target_os = "macos") {
                "https://developers.openai.com/codex/app".into()
            } else {
                "".into()
            },
            // 桌面版图形程序：点「打开应用」直接启动
            launch_cmd: "".into(),
            launch_app: "codex-app".into(),
            hidden: false,
        },
    );
    // Open365 开源电脑管家：无广告替代「安全卫士」——网络修复 / 垃圾清理 / 启动项 /
    // 强力卸载 / 安全护盾（开 Windows 自带三道防线）/ 守夜模式（AI 通宵不熄屏）。
    // launch_app=open365：已装(或 U 盘随盘带)→「打开应用」拉起；首点自动装到本地 + 建桌面快捷方式。
    // 仅 Windows（纯 PowerShell + WinForms）。
    #[cfg(windows)]
    v.push(ToolInfo {
        id: "open365".into(),
        name: "Open365 电脑管家（开源）".into(),
        summary: "无广告 · 无弹窗 · 无捆绑，替代「安全卫士」：一键修网络 / 清垃圾 / 管开机启动 / 强力卸载 / 开齐 Windows 自带杀毒·防火墙·更新；还有「守夜模式」让 AI 通宵干活不熄屏。首次点会装到本地并建桌面快捷方式。".into(),
        kind: "standalone".into(),
        // 永远「可打开」：Windows 下按需下载 —— 点了本地/U 盘没有就联网拉 open365.zip（launch_open365）。
        // 若报 installed=false，前端会走 onOpen 只打开官网页（客户「点了没下载到」的真因）。
        installed: true,
        action: "url".into(),
        target: "https://u-claw.org.cn/uking/".into(),
        launch_cmd: "".into(),
        launch_app: "open365".into(),
        hidden: false,
    });
    // Hermes 桌面版（Nous 官方 Electron app）：进阶区工具，hidden=true 不进小白市场/Dock，
    // 只在「进阶/App 版」页露出（Advanced.tsx 自己调 list_tools 取状态）。后端检测/启动能力保留。
    #[cfg(windows)]
    v.push(ToolInfo {
        id: "hermes-app".into(),
        name: "Hermes 桌面版（Nous 官方）".into(),
        summary: "Nous Research 自进化 AI 智能体的官方图形版。下一步下一步装好，再照教程把虾盘云 Key 填进去。".into(),
        kind: "deep".into(),
        installed: hermes_app_installed(),
        action: "install".into(),
        target: HERMES_APP_DOWNLOAD_PAGE.into(),
        launch_cmd: "".into(),
        launch_app: "hermes-app".into(),
        hidden: true,
    });
    // uu-switch —— 去广告版 cc-switch AI 模型切换器（我方 fork）。GUI 应用：一个窗口统一管
    // 所有 AI 工具（Claude Code / Codex …）的模型驱动，一键切换 + 内置计量看板。action=install
    // 走后端 install_uuswitch（下载 NSIS 静默装，不改用户任何 AI 配置）；装完 launch_app 启动。
    // 仅 Windows 露出（本轮只发 Windows 包，未托管 Mac 包）；删本卡只动本处 + uuswitch.rs + App.tsx。
    #[cfg(windows)]
    v.push(ToolInfo {
        id: "uu-switch".into(),
        name: "uu-switch 模型切换器".into(),
        summary: "一个窗口管好所有 AI 工具的模型驱动（Claude Code/Codex…），一键切换 + 用量看板。基于 cc-switch 的去广告纯净版，后续可从 U-King 一键导入虾盘云。".into(),
        kind: "standalone".into(),
        installed: crate::uuswitch::installed(),
        action: "install".into(),
        target: crate::uuswitch::download_url(),
        launch_cmd: "".into(),
        launch_app: "uu-switch".into(),
        hidden: false,
    });
    v
}

/// 帮用户下载并**静默安装** ClawX（NSIS `/S`）。小白主路径：点一下全自动，不用手点下一步。
/// 流程：动态取直链 → 下到临时目录(curl -o，重试 OSS/官方) → 跑 `<exe> /S`(NSIS 静默) →
/// 等装完(轮询 clawx_app_installed)。静默装失败(杀软拦/权限)→ 回退「非静默」拉起安装界面。
/// 返回人话进度给前端 toast。仅 Windows（ClawX 只有 Windows/Mac/Linux 各自包，这里管 Win）。
#[cfg(windows)]
pub fn install_clawx(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 前置去重（幂等）：已装就别再下 210MB 重装 —— 修「装了没检测到 / Wizard『一键全安装』无条件重装 → 又装一次」。
    // 后端兜底，不管前端哪条路调进来都安全（App.tsx openTool 有 fresh 去重，但 Wizard 的『一键全安装』没有）。
    // 口径 = clawx_app_installed()，与「已装」徽标和装完轮询完全一致，不引入第三套判据。
    if crate::providers::clawx_app_installed() {
        on_progress("检测到 ClawX 已安装，跳过重复安装。");
        return Ok("ClawX 已安装（跳过重复安装）。".into());
    }

    let url = clawx_download_url();
    let tmp = std::env::temp_dir().join("ClawX-Setup-uking.exe");
    let _ = std::fs::remove_file(&tmp);

    // 下载（curl -o；ClawX 包 ~210MB）。spawn 后边等边轮询临时文件大小报 MB 进度 ——
    // 让小白看到「正在下载 12 MB / 210 MB」而不是干等以为卡死。
    on_progress("开始下载 ClawX（约 210 MB，网络好约 1-3 分钟，请耐心等）…");
    let mut child = std::process::Command::new(crate::installer::system_tool("curl"))
        .args([
            "-sSL",
            "-A",
            "Mozilla/5.0 U-King",
            "-m",
            "600",
            "-o",
            &tmp.to_string_lossy(),
            &url,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动下载失败: {e}"))?;
    // 总大小未知（OSS 不一定回 Content-Length），按经验 ~210MB 估算百分比，给个安心感。
    const EST_TOTAL_MB: u64 = 210;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,           // 下载进程结束
            Ok(None) => {}                  // 还在下
            Err(_) => break,
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
        let mb = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) / 1_000_000;
        if mb > 0 {
            let pct = ((mb * 100) / EST_TOTAL_MB).min(99);
            on_progress(&format!("正在下载 ClawX… {mb} / {EST_TOTAL_MB} MB（{pct}%，请耐心等）"));
        }
    }
    let sz = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if sz < 50_000_000 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "ClawX 下载失败（{} 字节）。可在工具卡点「打开下载页」手动下。",
            sz
        ));
    }
    on_progress("下载完成，正在安装 ClawX…");

    // NSIS 静默安装：/S。部分机器静默会被杀软拦 → 失败回退拉起常规安装界面。
    let silent = std::process::Command::new(&tmp)
        .arg("/S")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match silent {
        Ok(mut child) => {
            // 等静默装完（最多 ~120s 轮询，给慢盘/杀软扫描留余量）；装好 clawx_app_installed 会变 true
            for _ in 0..60 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if crate::providers::clawx_app_installed() {
                    let _ = std::fs::remove_file(&tmp);
                    on_progress("安装完成，正在放行网络 + 接入虾盘云…");
                    // 装完立刻预放行防火墙：ClawX 第一次开会监听本地端口，不预放行小白
                    // 极可能点掉 Windows 弹窗导致联网失败。best-effort，失败有前端浮层兜底。
                    add_clawx_firewall_rule();
                    return Ok("ClawX 已静默安装完成，正在自动接入虾盘云…".into());
                }
            }
            // 轮询到点仍没探测到。**绝不能在静默安装进程还在跑时又拉一个安装器** ——
            // 那正是客户看到的「装着装着又弹出一个安装窗」的根源（重复安装）。分情形处理：
            if matches!(child.try_wait(), Ok(None)) {
                // ① 静默进程还在后台装（慢盘/大扫描）→ 交给它继续，不抢第二个安装器；
                //    装好后 auto_heal_clawx / 下次检测会认出来。
                on_progress("ClawX 仍在后台安装（慢盘/杀软扫描会拖慢），装好会自动接入，请再等一会儿。");
                return Ok("ClawX 正在后台安装，稍候会自动完成并接入虾盘云。".into());
            }
            // ② 静默进程已退出：再确认一次是否其实已装好（探测偶尔滞后于进程退出，别误判成失败）。
            if crate::providers::clawx_app_installed() {
                let _ = std::fs::remove_file(&tmp);
                add_clawx_firewall_rule();
                return Ok("ClawX 已安装完成，正在自动接入虾盘云…".into());
            }
            // ③ 静默确实失败（被杀软拦等）→ 才回退可视安装界面让用户点。
            on_progress("自动安装较慢，已打开安装界面，按提示一路点「下一步」即可（约 2-3 分钟）…");
            let _ = std::process::Command::new(&tmp).spawn();
            Ok("已为你打开 ClawX 安装程序，按提示点「下一步」装完即可。".into())
        }
        Err(_) => {
            // 连静默都起不来：直接拉常规界面
            std::process::Command::new(&tmp)
                .spawn()
                .map_err(|e| format!("启动 ClawX 安装程序失败: {e}"))?;
            Ok("已打开 ClawX 安装程序，按提示安装即可。".into())
        }
    }
}

#[cfg(not(windows))]
pub fn install_clawx(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请到工具卡点「打开下载页」安装 ClawX".into())
}

/// UU远程官网下载页（前端「自己去官网下」兜底 / 非 Windows 平台走这条）。
pub fn uu_remote_download_page() -> String {
    UU_REMOTE_DOWNLOAD_PAGE.to_string()
}

/// UU远程是否已装（给前端按钮换文案用；口径与工具市场的「已装」徽标同一个函数，不另起一套）。
pub fn uu_remote_is_installed() -> bool {
    uu_remote_installed()
}

/// 帮客户下载 + 安装 UU远程（网易官方远控）。给「技术支持 → 远程协助」用。
///
/// ## 为什么要有它
/// U-King 自带的远程协助（remote_assist.rs）只能**跑命令**，看不到屏幕。「按钮点了没反应」
/// 「弹窗看不懂」这类问题必须看现场，得靠真远控。而客户自己去装的失败率很高：找官网、
/// 挑平台、在一堆「高速下载器」里挑真包 —— 这一段正是我们该替他走完的。
///
/// ## 诚实边界（别在文案里越过）
/// - **没有绿色版**：官方只发安装包（~86MB），所以最好的结果也是「装上」，不是「免安装打开就用」。
/// - **连接不归我们管**：装完要客户自己在 UU远程 里开「远程协助」，把 ID + 验证码发过来。
///   我们不碰他的账号，也不代他授权。
///
/// ## `/S` 有据可依（2026-07-29 实测）
/// 拉安装包前 3MB 里能 grep 到 `Nullsoft` / `NSIS` 标记 → **确实是 NSIS 打的包**，`/S` 是它的
/// 标准静默旗标，不是瞎试。但 NSIS 脚本可以自定义页面绕过静默，所以仍然**只认「探测到装上了」**
/// 才算成功，探不到就回退可视安装界面 —— 判据是机器状态，不是「我发了 /S 所以应该装上了」。
#[cfg(windows)]
pub fn install_uu_remote(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 幂等：已装就别再下 86MB（同 install_clawx 的前置去重，口径复用 uu_remote_installed）。
    if uu_remote_installed() {
        on_progress("检测到 UU远程 已安装，跳过下载。");
        return Ok("UU远程 已安装。打开它 →「远程协助」，把 ID 和验证码发给作者即可。".into());
    }

    let tmp = std::env::temp_dir().join("UURemote-Setup-uking.exe");
    let _ = std::fs::remove_file(&tmp);

    on_progress(&format!(
        "开始下载 UU远程（网易官方，约 {UU_REMOTE_WIN_MB} MB，网络好约 1-2 分钟）…"
    ));
    let mut child = std::process::Command::new(crate::installer::system_tool("curl"))
        .args([
            "-sSL", // -L 必须有：官方入口是 302 到带签名的直链
            "-A",
            "Mozilla/5.0 U-King",
            "-m",
            "600",
            "-o",
            &tmp.to_string_lossy(),
            UU_REMOTE_DL_WIN,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动下载失败: {e}"))?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
        let mb = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) / 1_000_000;
        if mb > 0 {
            let pct = ((mb * 100) / UU_REMOTE_WIN_MB).min(99);
            on_progress(&format!("正在下载 UU远程… {mb} / {UU_REMOTE_WIN_MB} MB（{pct}%）"));
        }
    }
    // 下限卡在 40MB：CDN 抽风/被代理拦时会回一个几 KB 的错误页，HTTP 200 但内容是垃圾，
    // 直接拿去执行就是「双击没反应」这种最难查的故障（installer.rs 的下载校验同一个教训）。
    let sz = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if sz < 40_000_000 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "UU远程下载失败（只拿到 {sz} 字节，多半是网络被拦）。可以点「打开官网下载页」自己下。"
        ));
    }
    on_progress("下载完成，正在安装 UU远程…");

    // 先试静默（NSIS 惯例 /S）。**没验证过这个包认不认**，所以下面严格按「探测到装上了」才算成功。
    match std::process::Command::new(&tmp).arg("/S").creation_flags(CREATE_NO_WINDOW).spawn() {
        Ok(mut installer) => {
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if uu_remote_installed() {
                    let _ = std::fs::remove_file(&tmp);
                    return Ok("UU远程已装好。打开它 →「远程协助」，把 ID 和验证码发给作者即可。".into());
                }
            }
            // 轮询到点还没探到。**绝不能在前一个安装进程还活着时再拉一个** —— 那就是客户见过的
            // 「装着装着又弹出一个安装窗」（install_clawx 修过同一个坑，这里照抄结论）。
            if matches!(installer.try_wait(), Ok(None)) {
                on_progress("UU远程仍在后台安装（慢盘/杀软扫描会拖慢），装完在开始菜单能找到它。");
                return Ok("UU远程正在后台安装，稍等一会儿即可在开始菜单找到。".into());
            }
            if uu_remote_installed() {
                let _ = std::fs::remove_file(&tmp);
                return Ok("UU远程已装好。打开它 →「远程协助」，把 ID 和验证码发给作者即可。".into());
            }
            // 静默确实没成（多半这包不吃 /S）→ 才拉可视界面让用户点下一步。
            on_progress("自动安装没成功，已打开官方安装界面，按提示点「下一步」即可。");
            let _ = std::process::Command::new(&tmp).spawn();
            Ok("已为你打开 UU远程安装程序，按提示点「下一步」装完即可。".into())
        }
        Err(_) => {
            std::process::Command::new(&tmp)
                .spawn()
                .map_err(|e| format!("启动 UU远程安装程序失败: {e}"))?;
            Ok("已打开 UU远程安装程序，按提示安装即可。".into())
        }
    }
}

/// 非 Windows：不做「假静默」。Mac 的 dmg 要挂载 + 拖进 Applications，没在真机验证过的自动化
/// 不如老老实实把官网打开 —— 前端会把按钮换成「打开官网下载页」。
#[cfg(not(windows))]
pub fn install_uu_remote(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请点「打开官网下载页」安装 UU远程".into())
}

/// 启动一个 GUI 应用（codex-app / clawx）。CLI 工具走终端，这里只管图形程序。
pub fn launch_app(app: &str) -> Result<(), String> {
    // 「点了没反应」是最难远程排的一类投诉 —— 客户看不到任何报错，我们也看不到。
    // 记下点的是哪个应用、找没找到 exe、报了什么，一条就能分清是没装、路径没探到、还是起崩了。
    let r = launch_app_inner(app);
    match &r {
        Ok(_) => crate::ulog::write("launch", &format!("启动 {app} ✓")),
        Err(e) => crate::ulog::write("launch", &format!("启动 {app} ✗ {e}")),
    }
    r
}

fn launch_app_inner(app: &str) -> Result<(), String> {
    match app {
        "codex-app" => launch_codex_app(),
        "clawx" => launch_clawx_app(),
        "hermes-app" => launch_hermes_app(),
        "uu-switch" => crate::uuswitch::launch(),
        "open365" => launch_open365(),
        // openclaw 网页版不在这里启动（要 AppHandle 开内嵌网页窗）；前端按 id 拦截切到 openclaw 页。
        // 万一漏拦走到这里，给条清楚指引而不是「未知应用」。
        "openclaw-webui" => Err("请在「我的 AI」点 OpenClaw 网页版打开".into()),
        other => Err(format!("未知应用 {other}")),
    }
}

/// 「打开方式」把文件 / 文件夹在外部应用里打开（对齐 Codex 工作台的文件夹管理）。
/// app：
/// - `explorer`  资源管理器/Finder 打开该目录（传文件时改为「显示并选中」）
/// - `reveal`    在资源管理器/Finder 中显示并选中该文件/文件夹
/// - `vscode` / `cursor`  用编辑器打开（文件或目录都行）
/// - `terminal`  系统终端，cwd = 该目录（传文件时取其父目录）
/// - `gitbash`   Git Bash，cwd = 该目录（仅 Windows；找不到 git-bash.exe 给指引）
/// - `openas`    系统「打开方式」选择框（仅 Windows；让客户自己挑用哪个软件开）
///
/// 编辑器/Git Bash 没装时给出可读错误——前端按需只列用户常用的。
pub fn open_dir_external(path: &str, app: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() { return Err("还没选文件/文件夹".into()); }
    let p = Path::new(path);
    let is_file = p.is_file();
    // 终端/Git Bash 以「所在目录」为工作目录：传文件时取其父目录
    let dir = if is_file {
        p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };
    match app {
        "explorer" => {
            // 文件 → 在资源管理器中选中它；目录 → 直接打开
            if is_file { return reveal_in_file_manager(path); }
            #[cfg(windows)]
            std::process::Command::new("explorer.exe").arg(path).spawn().map_err(|e| format!("打开资源管理器失败：{e}"))?;
            #[cfg(target_os = "macos")]
            std::process::Command::new("open").arg(path).spawn().map_err(|e| format!("打开 Finder 失败：{e}"))?;
            #[cfg(all(not(windows), not(target_os = "macos")))]
            std::process::Command::new("xdg-open").arg(path).spawn().map_err(|e| e.to_string())?;
            Ok(())
        }
        "reveal" => reveal_in_file_manager(path),
        "vscode" | "cursor" => {
            let bin = if app == "cursor" { "cursor" } else { "code" };
            #[cfg(windows)]
            {
                // code/cursor 在 Windows 是 .cmd，须走 cmd /C；PATH 复用 installer::search_paths 补全
                let mut c = std::process::Command::new("cmd");
                c.args(["/C", bin, path]).no_window().env("PATH", editor_path_env());
                c.spawn().map_err(|e| format!("启动编辑器失败：{e}"))?;
            }
            #[cfg(not(windows))]
            std::process::Command::new(bin).arg(path).spawn().map_err(|_| format!("没找到 {bin}，请确认已安装并加入 PATH"))?;
            Ok(())
        }
        "terminal" => crate::term::term_open_external(None, Some(dir)),
        "gitbash" => open_git_bash(&dir),
        // 「用其他程序打开…」= 系统自带的**打开方式**选择框。
        //
        // 为什么值得单列：`openPath` 只会用**已注册的默认程序**打开，而客户手上常见的是
        // 「这个 .zip 我想用 7-Zip 而不是资源管理器」「这个 .md 我想用 Typora」——
        // 以前只能自己去资源管理器里右键。菜单里已经有 VS Code 一个硬编码入口，
        // 但硬编码永远只覆盖我们想到的那几个；把系统的选择框调出来，客户装了什么就有什么。
        //
        // 只有 Windows 有这个对话框（`OpenAs_RunDLL`）。macOS 没有等价的命令行入口
        // （Finder 的「打开方式」只能在 Finder 里点），所以老实说不支持并指路，不假装能干。
        "openas" => open_with_dialog(path),
        other => Err(format!("不支持的打开方式：{other}")),
    }
}

/// 系统自带的「打开方式」选择框。**两份定义按平台切**，不在函数体里塞 `#[cfg]` 块 ——
/// 那种写法（`#[cfg(windows)] { … } #[cfg(not(windows))] Err(…)` 当尾表达式）在本机
/// cargo check 是过的，但 Mac 侧我编不了、赌不起；仓库里 `ensure_git` 等已有的做法就是两份定义。
///
/// 为什么值得有：`openPath` 只会用**已注册的默认程序**开，而客户想的是「这个 zip 用 7-Zip、
/// 这个 md 用 Typora」。硬编码一个 VS Code 永远只覆盖我们想到的那几个。
#[cfg(windows)]
fn open_with_dialog(path: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // raw_arg 精确控制引号：走普通 .arg() 时 std 会把整串再包一层，rundll32 解析不了
    // （同 reveal_in_file_manager 那个坑）。
    std::process::Command::new("rundll32.exe")
        .raw_arg(format!("shell32.dll,OpenAs_RunDLL \"{path}\""))
        .spawn()
        .map_err(|e| format!("打开「打开方式」失败：{e}"))?;
    Ok(())
}

/// 非 Windows：没有等价的命令行入口（访达的「打开方式」只能在访达里点）。老实说不支持并指路，
/// 不假装能干。前端也按平台把这个菜单项藏了（`lib/platform.ts`），这里是第二道。
#[cfg(not(windows))]
fn open_with_dialog(_path: &str) -> Result<(), String> {
    Err("这个系统没有「打开方式」对话框，请在访达/文件管理器里右键该文件选择程序".into())
}

/// 在系统文件管理器里「显示并选中」某项（对齐 Codex 的「在文件夹管理工具打开」）。
fn reveal_in_file_manager(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        // explorer 的选中语法是 `/select,"<全路径>"`。用 raw_arg 精确控制引号——
        // 走普通 .arg() 时 std 见路径含空格会把整个 `/select,...` 连同前缀一起包引号，
        // explorer 无法解析（对齐仓库既有 raw_arg 修引号被吃的教训）。
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn()
            .map_err(|e| format!("在资源管理器中显示失败：{e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", path])
            .spawn()
            .map_err(|e| format!("在 Finder 中显示失败：{e}"))?;
        Ok(())
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        // Linux 无通用「显示并选中」，退回打开父目录
        let parent = Path::new(path)
            .parent()
            .map(|d| d.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        std::process::Command::new("xdg-open").arg(parent).spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 在指定目录打开 Git Bash（Windows 专属）。找不到 git-bash.exe 时给可读指引。
#[cfg(windows)]
fn open_git_bash(dir: &str) -> Result<(), String> {
    let exe = find_git_bash().ok_or("未找到 Git Bash（请先安装 Git for Windows）")?;
    // git-bash.exe 支持 `--cd=<path>` 指定起始目录；它自带 mintty 窗口，不加 no_window
    std::process::Command::new(&exe)
        .arg(format!("--cd={dir}"))
        .spawn()
        .map_err(|e| format!("启动 Git Bash 失败：{e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn open_git_bash(_dir: &str) -> Result<(), String> {
    Err("Git Bash 仅 Windows 可用".into())
}

/// 探测 git-bash.exe 常见安装位置（Program Files / 64 位重定向 / 用户级安装）。
#[cfg(windows)]
fn find_git_bash() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Ok(base) = std::env::var(var) {
            cands.push(Path::new(&base).join("Git").join("git-bash.exe"));
        }
    }
    if let Ok(la) = std::env::var("LOCALAPPDATA") {
        cands.push(Path::new(&la).join("Programs").join("Git").join("git-bash.exe"));
    }
    cands.into_iter().find(|p| p.exists())
}

/// 编辑器查找用的 PATH（系统 PATH + 便携 Node/npm 全局 bin，防 code/cursor 不在裸 PATH 里）。
#[cfg(windows)]
fn editor_path_env() -> String {
    let mut dirs: Vec<String> = crate::installer::search_paths(crate::installer::portable_node_dir().as_deref())
        .into_iter().map(|p| p.display().to_string()).collect();
    if let Ok(sys) = std::env::var("PATH") { dirs.push(sys); }
    dirs.join(";")
}

/// 启动 Hermes 桌面版（Electron .exe，找安装位置直接拉起）。
#[cfg(windows)]
fn launch_hermes_app() -> Result<(), String> {
    let exe = find_hermes_app_exe().ok_or("未找到 Hermes.exe（请先安装 Hermes 桌面版）")?;
    std::process::Command::new(&exe)
        .no_window()
        .spawn()
        .map_err(|e| format!("启动 Hermes 失败: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn launch_hermes_app() -> Result<(), String> {
    std::process::Command::new("open")
        .args(["-a", "Hermes"])
        .spawn()
        .map_err(|e| format!("启动 Hermes 失败: {e}"))?;
    Ok(())
}

/// 启动 Codex 桌面版（MSIX，AppUserModelID = OpenAI.Codex_...!App）。
/// 用 `explorer shell:AppsFolder\<AUMID>` 拉起，跟开始菜单点图标等效。
#[cfg(windows)]
fn launch_codex_app() -> Result<(), String> {
    // 取 Appx 的 PackageFamilyName，拼 AUMID。Codex 的入口 App id 是 "App"。
    let out = std::process::Command::new(crate::installer::system_tool("powershell"))
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-AppxPackage -Name OpenAI.Codex).PackageFamilyName",
        ])
        .no_window()
        .output()
        .map_err(|e| format!("查 Codex 包失败: {e}"))?;
    let pfn = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pfn.is_empty() {
        return Err("未找到 Codex 桌面版（请先安装）".into());
    }
    let aumid = format!("{pfn}!App");
    // 用 explorer.exe 拉 shell:AppsFolder（实测可靠：裸 "explorer"/CREATE_NO_WINDOW 会启动失败）。
    // 不加 no_window —— explorer 本身不弹窗，加了反而干扰激活。
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{aumid}"))
        .spawn()
        .map_err(|e| format!("启动 Codex 桌面版失败: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn launch_codex_app() -> Result<(), String> {
    // macOS：open -a Codex
    std::process::Command::new("open")
        .args(["-a", "Codex"])
        .spawn()
        .map_err(|e| format!("启动 Codex 失败: {e}"))?;
    Ok(())
}

/// 在常见安装落点里找到 ClawX.exe（NSIS 安装版）。launch 和防火墙放行共用。
#[cfg(windows)]
pub fn find_clawx_exe() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
    let candidates = [
        // 「为所有用户装」→ Program Files（最常见，老代码漏了这条导致识别到也启动不了）
        Path::new(&pf).join("ClawX").join("ClawX.exe"),
        Path::new(&pf86).join("ClawX").join("ClawX.exe"),
        Path::new(&local).join("Programs").join("ClawX").join("ClawX.exe"),
        Path::new(&home).join("ClawX").join("ClawX.exe"),
        Path::new(&home).join("Desktop").join("ClawX").join("ClawX.exe"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// 在常见 electron-builder 落点里找 Hermes 桌面版的 exe。检测「已装」与「打开应用」共用。
///
/// ★ 实测（2026-06-20，本机真装一遍）：官方桌面版 productName = **"Hermes Studio"**，
/// exe 就叫 **`Hermes Studio.exe`**（带空格），且是 **perMachine** 安装 → 默认落
/// `C:\Program Files\Hermes Studio\Hermes Studio.exe`（不是 `%LOCALAPPDATA%\Programs`，
/// 也不叫 `Hermes.exe`）。证据：安装器内嵌 `app-64.7z` 里就是这个 exe + 同目录
/// `Uninstall Hermes Studio.exe`，更新缓存目录叫 `hermes-studio-updater`。
/// perUser 配置才落 `%LOCALAPPDATA%\Programs`——一并兜底；旧/分叉构建可能叫 Hermes，也试。
#[cfg(windows)]
pub fn find_hermes_app_exe() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let pf86 =
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
    // 安装根：Program Files（官方 perMachine 默认）优先，再退 perUser 的 %LOCALAPPDATA%\Programs。
    let roots = [
        PathBuf::from(&pf),
        PathBuf::from(&pf86),
        Path::new(&local).join("Programs"),
    ];
    // (安装目录名, exe 名)——实测的 "Hermes Studio" 放第一位，旧名兜底。
    let products: [(&str, &str); 3] = [
        ("Hermes Studio", "Hermes Studio.exe"),
        ("Hermes", "Hermes.exe"),
        ("Hermes Agent", "Hermes Agent.exe"),
    ];
    for root in &roots {
        for (dir, exe) in &products {
            let p = root.join(dir).join(exe);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Hermes 桌面版是否已装（探测 Electron exe）。仅 Windows 有 app 接管；其他平台返回 false。
pub fn hermes_app_installed() -> bool {
    #[cfg(windows)]
    {
        find_hermes_app_exe().is_some()
    }
    #[cfg(not(windows))]
    {
        Path::new("/Applications/Hermes.app").exists()
    }
}

/// 帮用户**下载并运行 Hermes 桌面版安装器**（下一步下一步，非静默）。
/// 进阶区工具：装机半自动即可，配置交给用户照教程手填（GUI app 自动配模型坑太多，不做）。
/// 流程：下载 Hermes-Setup.exe(~7.5MB 壳) → 拉起安装界面让用户点「下一步」。
/// 下载失败（国际站慢/被墙）→ 返回 Err，前端回退「打开官网下载页」。仅 Windows。
#[cfg(windows)]
pub fn install_hermes_app(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let tmp = std::env::temp_dir().join("Hermes-Setup-uking.exe");
    let _ = std::fs::remove_file(&tmp);

    on_progress("正在下载 Hermes 安装器（约 8 MB）…");
    let status = std::process::Command::new(crate::installer::system_tool("curl"))
        .args([
            "-sSL",
            "-A",
            "Mozilla/5.0 U-King",
            "-m",
            "120",
            "-o",
            &tmp.to_string_lossy(),
            HERMES_APP_URL,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("启动下载失败: {e}"))?;

    let sz = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    // 壳子约 7.5MB；< 2MB 基本是下崩了（国际站不通/超时拿到错误页）。
    if !status.success() || sz < 2_000_000 {
        let _ = std::fs::remove_file(&tmp);
        return Err("Hermes 安装器下载失败（官网在国内可能较慢）。已为你打开官网下载页，请手动下载安装。".into());
    }

    on_progress("下载完成，正在打开 Hermes 安装界面，请按提示点「下一步」…");
    // 非静默：拉起安装界面让用户下一步下一步（用户明确要「能装一部分就行，不强求全自动」）。
    std::process::Command::new(&tmp)
        .spawn()
        .map_err(|e| format!("启动 Hermes 安装程序失败: {e}"))?;
    Ok("已打开 Hermes 安装程序，按提示点「下一步」装完即可。装好后回到本页点「打开 Hermes」，再照教程把虾盘云 Key 填进去。".into())
}

#[cfg(not(windows))]
pub fn install_hermes_app(_on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    Err("当前平台请到官网下载 Hermes 桌面版安装包".into())
}

/// Hermes 官网下载页（下载失败时前端回退打开）。
pub fn hermes_download_page() -> String {
    HERMES_APP_DOWNLOAD_PAGE.to_string()
}

/// 当前进程是否以管理员身份运行（netsh 改防火墙必须管理员，否则白跑还喷错）。
/// 用 `net session`（仅管理员能成功）静默探测，不引第三方 crate。
#[cfg(windows)]
fn is_elevated() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("net")
        .args(["session"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 给 ClawX.exe 预先加防火墙「允许访问」规则（入站 + 出站）。
///
/// 注意：实测 ClawX 只监听 127.0.0.1（本地回环）、对外是出站连接，Windows Defender
/// 默认放行出站、localhost 不过防火墙，**多数机器根本不弹防火墙窗**。所以这条只在
/// 「恰好以管理员运行」时锦上添花；普通权限直接跳过（加不上还会喷错），靠前端
/// 「请点允许访问」浮层兜底即可。规则名带 "U-King ClawX" 便于幂等。仅 Windows。
#[cfg(windows)]
pub fn add_clawx_firewall_rule() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 非管理员加不上防火墙规则 —— 直接跳过，免得每次启动喷「需要提升」错误。
    if !is_elevated() {
        return false;
    }
    let Some(exe) = find_clawx_exe() else {
        return false;
    };
    let exe = exe.to_string_lossy().to_string();
    // 入站 + 出站各加一条 allow。先按规则名删旧的（幂等，避免重复堆积），再加新的。
    let mut ok = false;
    for (name, dir) in [("U-King ClawX In", "in"), ("U-King ClawX Out", "out")] {
        let _ = std::process::Command::new("netsh")
            .args(["advfirewall", "firewall", "delete", "rule", &format!("name={name}")])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let status = std::process::Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={name}"),
                &format!("dir={dir}"),
                "action=allow",
                &format!("program={exe}"),
                "enable=yes",
                "profile=any",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        if matches!(status, Ok(s) if s.success()) {
            ok = true;
        }
    }
    ok
}

#[cfg(not(windows))]
pub fn add_clawx_firewall_rule() -> bool {
    false
}

/// 启动 ClawX 桌面版（Electron .exe，找安装位置直接拉起）。
#[cfg(windows)]
fn launch_clawx_app() -> Result<(), String> {
    let exe = find_clawx_exe().ok_or("未找到 ClawX.exe（请先安装 ClawX 桌面版）")?;
    std::process::Command::new(&exe)
        .no_window()
        .spawn()
        .map_err(|e| format!("启动 ClawX 失败: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn launch_clawx_app() -> Result<(), String> {
    std::process::Command::new("open")
        .args(["-a", "ClawX"])
        .spawn()
        .map_err(|e| format!("启动 ClawX 失败: {e}"))?;
    Ok(())
}

/// 拉起 Open365.exe。
///
/// **必须走 ShellExecute，不能用普通 spawn**：Open365 的清单里写了 `requireAdministrator`，
/// 而 `Command::spawn`（CreateProcess）拉不起需要提升的程序 —— 直接 `os error 740
/// 请求的操作需要提升`，客户点「打开应用」什么都不发生（线上实测 2026-07-27）。
/// PowerShell 的 `Start-Process` 默认就是 ShellExecute，会正常弹 UAC；
/// 它自带的 install.ps1 一直是这么起的，这里对齐同一种起法。
#[cfg(windows)]
fn spawn_open365_exe(exe: &Path, work_dir: &Path) -> Result<(), String> {
    let ps = format!(
        "Start-Process -FilePath '{}' -WorkingDirectory '{}'",
        exe.to_string_lossy().replace('\'', "''"),
        work_dir.to_string_lossy().replace('\'', "''")
    );
    std::process::Command::new(crate::installer::system_tool("powershell"))
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .no_window()
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 Open365 失败: {e}"))
}

/// 本版 U-King 期望的 Open365 版本 —— 本地低于它就刷新（无论当初是从 U 盘还是联网装的）。
/// 发 Open365 新版时同步抬这个数；抬了但客户拿不到新包也不致命（下不动就照常起老版本）。
#[cfg(windows)]
const OPEN365_EXPECTED_VERSION: (u32, u32, u32) = (1, 3, 0);

/// 读某个 Open365 目录的 VERSION，解析成 (主,次,修订)。读不到 / 解析不了当 (0,0,0)。
#[cfg(windows)]
fn open365_version_of(dir: &Path) -> (u32, u32, u32) {
    let raw = std::fs::read_to_string(dir.join("VERSION")).unwrap_or_default();
    let first = raw
        .trim_start_matches('\u{feff}') // 可能带 UTF-8 BOM
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let mut it = first.split('.').map(|p| p.trim().parse::<u32>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// U 盘随盘带的那份 Open365（仅当这次是从带 Open365/ 的盘运行时才有）。
#[cfg(windows)]
fn open365_usb_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("Open365");
    if dir.join("Open365.exe").exists() || dir.join("install.ps1").exists() {
        Some(dir)
    } else {
        None
    }
}

#[cfg(windows)]
fn launch_open365() -> Result<(), String> {
    let local = open365_local_dir();
    let local_exe = local.join("Open365.exe");

    // 1) 本地已装好 → 先看要不要就地升级，再启动。
    //    原来这里是「装过就直接起」，等于装过一次的客户永远停在旧版本：
    //    高 DPI 挤成一团那版是这样，影核改造前缺 core/ 的旧安装更糟 ——
    //    GUI 各页会静默显示「读取失败」，不报错、看不出是版本旧了。
    if local_exe.exists() {
        // 刷新源：优先 U 盘随盘带那份；没有（= 当初是联网兜底装的）就重新下一次。
        //
        // 只认 U 盘是上一版没修干净的地方：**联网装的那批客户永远升不上去** ——
        // 本机实测就是这样，装着 1.2.2（高 DPI 挤成一团那版、还没有 core/），
        // 而 U 盘刷新分支根本轮不到它。判据改成「本地版本 < 我们这一版随附的版本」，
        // 与来源无关。
        let stale = open365_version_of(&local) < OPEN365_EXPECTED_VERSION
            || !local.join("core").join("action-core.ps1").exists();
        let refresh_src = if stale {
            match open365_usb_dir() {
                Some(d) => Some(d),
                // U 盘上没有 → 联网重下（下不动就跳过，照常启动老版本，绝不把人卡在门外）
                None => download_open365().ok().filter(|d| *d != local),
            }
        } else {
            None
        };
        if let Some(src) = refresh_src {
            let newer = open365_version_of(&src) > open365_version_of(&local);
            let missing_core = !local.join("core").join("action-core.ps1").exists()
                && src.join("core").join("action-core.ps1").exists();
            // 只覆盖/补齐文件，不删用户目录里的别的东西；exe 被占用（正开着）就跳过本次刷新
            if (newer || missing_core)
                && crate::install::copy_dir_recursive(&src, &local, &src, &mut |_, _| {}).is_ok()
            {
                // 刷新过就走 install.ps1 重编一次，保证 exe 与 gui/ 源码是同一版
                let install = local.join("install.ps1");
                if install.exists() {
                    return std::process::Command::new(crate::installer::system_tool("powershell"))
                        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                        .arg(&install)
                        .current_dir(&local)
                        .no_window()
                        .spawn()
                        .map(|_| ())
                        .map_err(|e| format!("升级 Open365 失败: {e}"));
                }
            }
        }
        spawn_open365_exe(&local_exe, &local)?;
        return Ok(());
    }

    // 2) 找源（本地源 / U 盘随盘带）；都没有 → 联网下载兜底（解压进本地目录）
    let src = match open365_source_dir() {
        Some(s) => s,
        None => download_open365()?,
    };
    let run_dir = if src == local {
        local.clone()
    } else {
        std::fs::create_dir_all(&local).map_err(|e| format!("建目录失败: {e}"))?;
        crate::install::copy_dir_recursive(&src, &local, &src, &mut |_, _| {})
            .map_err(|e| format!("复制 Open365 到本地失败: {e}"))?;
        local.clone()
    };

    // 3) 有 install.ps1 → 一键装（编译 exe + 桌面快捷方式 + 启动托盘）
    let install = run_dir.join("install.ps1");
    if install.exists() {
        std::process::Command::new(crate::installer::system_tool("powershell"))
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&install)
            .current_dir(&run_dir)
            .no_window()
            .spawn()
            .map_err(|e| format!("启动 Open365 安装失败: {e}"))?;
        return Ok(());
    }

    // 4) 没 install.ps1 但有预编译 exe → 直接起
    let exe = run_dir.join("Open365.exe");
    if exe.exists() {
        spawn_open365_exe(&exe, &run_dir)?;
        return Ok(());
    }
    Err("Open365 文件不完整（缺 Open365.exe / install.ps1）".into())
}

#[cfg(not(windows))]
fn launch_open365() -> Result<(), String> {
    Err("Open365 电脑管家目前仅支持 Windows".into())
}


// 工具「是否已装」统一走 installer::tool_installed（注入便携 PATH + 真跑 --version），
// 不再用裸 `where`/`which` —— 后者不注入 PATH，会与装机向导的判定打架（漏判已装）。
