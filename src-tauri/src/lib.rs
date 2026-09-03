//! U-King 简化版 (U 盘版) —— Tauri 入口。
//!
//! 模块分工：
//! - `install`       一键装到本地 + 桌面快捷方式
//! - `context_menu`  Windows 右键菜单注册/注销
//! - `tools`         可选 AI 工具市场目录
//! - `tray`          右下角常驻托盘（360 式）
//!
//! 暴露给前端的 command 见下方 `invoke_handler`。

mod agent;
mod airuntime;
mod aitasks;
mod arena;
mod artifacts;
mod backup;
mod browser;
mod chatstore;
mod cleanup;
mod clawx;
mod openclaw2;
mod usb_genie;
mod claude_proxy;
mod codex;
mod codex_proxy;
mod context_menu;
mod crashlog;
mod freerouter;
mod doc;
mod device;
mod draw;
mod envfp;
mod expert;
mod hire;
mod feedback;
mod metrics;
mod model_route;
mod video;
mod vision;
mod reel;
mod fs;
mod geo;
mod guard;
mod hardware;
mod identity;
mod image;
mod install;
mod instance;
mod localllm;
mod advice;
mod actions;
mod automation;
/// 别让电脑睡（夜班助手 N1）。只挡空闲休眠，挡不住合盖 —— 边界见模块头。
mod awake;
/// 浏览器子窗口导航的无头取证（需求榜 P0 #5 的硬那半边）。只在 `--browser-nav-test` 下用。
mod browser_nav_probe;
mod installer;
mod journal;
mod macopt;
mod mcp;
mod mcp_serve;
mod officedoc;
mod origin;
mod org;
mod toolbox;
mod providers;
mod remote_assist;
mod report;
mod rtk;
mod bundled_apps;
mod miniapp;
mod podapp;
mod skillpack;
mod tasks;
mod term;
mod toolprobe;
mod tools;
mod tray;
mod uninstall;
mod ulog;
mod uuswitch;
mod usage;
mod usage_local;
// webview2 无条件编译：模块内部自己按平台分流（非 Windows 下 `installed()` 恒为 true、
// `ensure()` 直接返回 Ready）。**别在这儿加 `#[cfg(windows)]`** —— 那样 Mac 上
// `webview2::ensure()` 会找不到模块（E0433），而这个错在 Windows 开发机上永远看不到。
mod webview2;
#[cfg(windows)]
mod winicon;
mod workbench;
// 沙箱测试的唯一入口。**凡是要改 `UKING_TEST_HOME` 的 `#[test]` 一律走它**，别自己
// `set_var`、别再起第二把锁 —— 各模块各锁各的，等于没锁（见模块头注释）。
#[cfg(test)]
mod testsandbox;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

/// 启动时探测到的环境信息（前端 hero 区展示）。
#[derive(Debug, Clone, Serialize)]
struct AppEnv {
    /// 是否从本地安装目录运行（false = 多半在 U 盘上跑）
    running_from_local: bool,
    /// 本地安装目录路径
    install_dir: String,
    /// 是否已注册右键菜单
    context_menu_registered: bool,
    /// 右键菜单透传进来的目录（--open-dir），无则空
    opened_dir: Option<String>,
    /// 平台
    platform: String,
    /// 用户主目录（OpenCodex 默认会话用，不必先选文件夹即可开聊）
    home_dir: String,
    /// 独立演示卸载绿色版：前端据此只渲染清场界面。
    demo_uninstaller: bool,
}

/// 安装进度事件 payload。
#[derive(Clone, Serialize)]
struct InstallProgress {
    rel: String,
    count: u64,
}

// ============================================================
// Commands
// ============================================================

#[tauri::command]
fn get_env() -> AppEnv {
    AppEnv {
        running_from_local: install::running_from_install_dir(),
        install_dir: install::install_dir().display().to_string(),
        context_menu_registered: context_menu::is_registered(),
        opened_dir: parse_open_dir_arg(),
        platform: std::env::consts::OS.to_string(),
        home_dir: std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default(),
        demo_uninstaller: cfg!(feature = "demo-uninstaller"),
    }
}

/// 一键装到本地（带进度事件 `uking:install_progress`）。
#[tauri::command]
async fn install_local(app: AppHandle) -> Result<install::InstallResult, String> {
    // 在阻塞线程跑文件复制，避免卡住 UI 线程。
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        install::install_to_local(|rel, count| {
            let _ = app2.emit(
                "uking:install_progress",
                InstallProgress {
                    rel: rel.to_string(),
                    count,
                },
            );
        })
    })
    .await
    .map_err(|e| format!("安装任务异常: {e}"))?
}

/// 打开本地安装目录。
#[tauri::command]
fn open_install_dir() -> Result<(), String> {
    install::reveal_install_dir()
}

/// 注册右键菜单（指向本地安装的 exe；若未装到本地则指向当前 exe）。
/// 薄壳，真身是影核动作 `runtime.context_menu.set`（GUI 点了按钮 = 显式确认，传 confirm）。
#[tauri::command]
async fn register_context_menu() -> Result<(), String> {
    run_write_action(actions::CONTEXT_MENU_SET, serde_json::json!({ "enabled": true })).await.map(|_| ())
}

/// 注销右键菜单。
/// 薄壳，真身是影核动作 `runtime.context_menu.set`。
#[tauri::command]
async fn unregister_context_menu() -> Result<(), String> {
    run_write_action(actions::CONTEXT_MENU_SET, serde_json::json!({ "enabled": false })).await.map(|_| ())
}

/// 工具市场目录。
///
/// **必须是 async + spawn_blocking**（测试报告 #011「进阶/AP 模式切换卡顿」的真因）：
/// `tools::list_tools()` 里每个工具的「装没装」都走 `installer::tool_installed`，
/// 而那个函数会**真的把命令起起来跑一次 `--version`**（见 installer::probe）。
/// claude / codex / openclaw / hermes 四个一轮就是 4+ 次进程启动 ——
/// claude.exe 有 253MB，冷启动本来就慢，再叠上杀软实时扫描，几百毫秒到几秒都正常。
///
/// 以前它是同步 command，也就是这一切**跑在主线程上**：进程起得越慢，界面冻得越久。
/// 客户看到的就是「点进阶页要卡一下」。挪到阻塞线程池后，慢照旧慢，但界面不再被冻住
/// （前端本来就是 await，拿到结果才渲染，行为一个字节不变）。
#[tauri::command]
async fn list_tools() -> Vec<tools::ToolInfo> {
    tauri::async_runtime::spawn_blocking(tools::list_tools)
        .await
        .unwrap_or_default()
}

/// ClawX 当前平台官方下载直链（动态读 release-info.json，版本会变；拉不到兜底当前版本）。
/// 前端「下载 ClawX」按钮 / 一键全装收尾点这个拿实时链接，避免硬编码版本号 404。
#[tauri::command]
async fn get_clawx_download_url() -> String {
    tauri::async_runtime::spawn_blocking(tools::clawx_download_url)
        .await
        .unwrap_or_else(|_| "https://oss.intelli-spectrum.com/latest/ClawX-0.4.11-win-x64.exe".into())
}

/// 帮下载 + 静默安装 ClawX（NSIS /S）。小白主路径：点一下全自动。
/// 进度走事件 `uking:clawx_progress`（下载 MB / 安装阶段），前端实时显示不让人以为卡死。
#[tauri::command]
async fn install_clawx(app: AppHandle) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        tools::install_clawx(&move |msg: &str| {
            let _ = app2.emit("uking:clawx_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("安装 ClawX 异常: {e}"))?
}

/// UU远程（网易官方远控）装没装 + 能不能一键装 + 官网下载页。给「技术支持 → 屏幕协助」渲染文案。
/// 薄壳，真身是影核动作 `runtime.uu_remote.inspect` —— 同一事实不留第二份实现，
/// 也让 `action conformance` 自动多一条冒烟测试（形状变了当场变红）。
#[tauri::command]
async fn uu_remote_status() -> serde_json::Value {
    run_action_blocking(actions::UU_REMOTE_INSPECT).await
}

/// 帮下载 + 安装 UU远程（约 86 MB）。进度走事件 `uking:uuremote_progress`。
/// 只帮到「装上」；连接要客户自己在 UU远程 里开远程协助、把 ID+验证码发过来（我们不碰他账号）。
///
/// 薄壳，真身是影核写动作 `runtime.uu_remote.install`。GUI 点按钮 = 显式确认，
/// 所以薄壳补 `confirm:true`；**判断在核心不在薄壳** —— 从 CLI / MCP / 远端影子进来
/// 一样要自己带确认，绕开这个按钮不等于绕开门禁。
#[tauri::command]
async fn install_uu_remote(app: AppHandle) -> Result<String, String> {
    let v = run_write_action_progress(
        app,
        actions::UU_REMOTE_INSTALL,
        "uking:uuremote_progress",
        serde_json::json!({}),
    )
    .await?;
    Ok(v.get("message").and_then(|m| m.as_str()).unwrap_or_default().to_string())
}

/// 泊舟 AI 小程序（PodApp）状态：装没装 / 已装版本 / 最新版 / 有没有新版。
/// 薄壳，真身是影核动作 `runtime.podapp.inspect`。
#[tauri::command]
async fn podapp_status() -> serde_json::Value {
    run_action_blocking(actions::PODAPP_INSPECT).await
}

/// 下载 + 安装/更新泊舟 AI 小程序。进度走事件 `uking:podapp_progress`。
/// 装完之后的自动升级由 PodApp 自己做，U-King 不轮询、不常驻。
#[tauri::command]
async fn install_podapp(app: AppHandle) -> Result<String, String> {
    let v = run_write_action_progress(
        app,
        actions::PODAPP_INSTALL,
        "uking:podapp_progress",
        serde_json::json!({}),
    )
    .await?;
    Ok(v.get("message").and_then(|m| m.as_str()).unwrap_or_default().to_string())
}

/// 启动已装的泊舟 AI 小程序（贴屏边的常驻窄条）。
#[tauri::command]
async fn launch_podapp() -> Result<String, String> {
    let v = run_write_action(actions::PODAPP_LAUNCH, serde_json::json!({})).await?;
    Ok(v.get("message").and_then(|m| m.as_str()).unwrap_or_default().to_string())
}

// ───────────────────────── 自动化（定时任务）─────────────────────────
//
// 组合根职责：把「怎么真正跑一条任务」注入给 automation.rs（它自己不认识 agent/device）。
// 三个大脑一条分发，别在别处再写第二份。

/// 到点了怎么干。**这是 automation 唯一的执行路径** —— GUI 的「立即运行」、
/// 调度线程的到点触发、将来的远端影子，走的都是这一个函数。
fn run_automation_job(job: &automation::Job) -> Result<String, String> {
    // 工作文件夹只在真存在时才放行：给了 = 用户明确授权它在这个文件夹里读写文件/跑命令；
    // 没给 = `tools_spec(false)`，只剩作图/视频这类零风险工具。无人值守下这条边界是硬的。
    let ws = Some(job.dir.clone()).filter(|d| !d.trim().is_empty() && std::path::Path::new(d).is_dir());
    let task_id = format!("auto-{}-{}", job.id, automation::now_ms());
    // 长程记忆：开 `use_memory` 的任务，这一班的 prompt 夹上上一班的进度/结论接着干。
    // 没开 = 原样返回，现有任务行为一字不变。三个引擎共用这一份拼装，别在分支里各写一遍。
    let prompt = automation::with_memory(job, &job.prompt);
    match job.engine.as_str() {
        // 模型**跟随客户在「AI 设置」里选的那个**（`automation_model`），不再写死 preset。
        //
        // 历史：这里先写死过 `deepseek-v4-pro`（「满血才会干活」），后来一刀切改成 preset 的
        // flash（「无人值守最该省钱」）。两次都错在同一件事上 —— **客户看到的模型和半夜真正
        // 替他干活的模型不是一个**，产出质量对不上预期，而他无从知道为什么。客户报的
        // 「定时任务跑出来的东西不对」，这是头号嫌疑。切了官方直连时会自动回落，见该函数注释。
        "uking" => {
            let model = providers::automation_model();
            // 把「这一班到底用了哪个模型」记下来。客户报「定时任务跑出来的东西不对」时，
            // 这是第一个要问的问题，而在此之前它**在任何地方都查不到** —— 运行记录里
            // 只写「大脑：uking」，模型是什么全靠猜。
            ulog::write("automation", &format!("run job={} engine=uking model={model}", job.id));
            agent::chat::run_headless(&task_id, &prompt, ws, None, &model)
        }
        // claude / codex：委派给真身 CLI（注入虾盘云 env，客户不用另外配）。
        // 无人值守：`-p` / `exec` 一次性跑完就退，不进交互。
        engine => {
            let (bare, args): (&str, Vec<String>) = if engine == "claude" {
                ("claude", vec!["-p".into(), prompt.clone()])
            } else {
                ("codex", vec!["exec".into(), prompt.clone()])
            };
            agent::claude::run_oneshot(bare, &args, ws.as_deref(), 900)
        }
    }
}

/// 全部自动化 + 可用性（ready/blockers）。薄壳，真身是影核动作 `runtime.automation.inspect`。
#[tauri::command]
async fn list_automations() -> serde_json::Value {
    run_action_blocking(actions::AUTOMATION_INSPECT).await
}

/// 新增 / 修改一条自动化。
#[tauri::command]
async fn save_automation(job: serde_json::Value) -> Result<serde_json::Value, String> {
    run_write_action(actions::AUTOMATION_SAVE, serde_json::json!({ "job": job })).await
}

#[tauri::command]
async fn remove_automation(id: String) -> Result<serde_json::Value, String> {
    run_write_action(actions::AUTOMATION_REMOVE, serde_json::json!({ "id": id })).await
}

#[tauri::command]
async fn set_automation_enabled(id: String, enabled: bool) -> Result<serde_json::Value, String> {
    run_write_action(
        actions::AUTOMATION_SET_ENABLED,
        serde_json::json!({ "id": id, "enabled": enabled }),
    )
    .await
}

/// 「立即运行一次」。**故意不是影核动作**：每跑一次都在烧 token（非幂等），
/// 而我们还没有幂等键账本 —— 见 `actions.rs` 里 AUTOMATION_* 那段注释。
///
/// 跑完的 `uking:automation_done` 事件**不在这里发** —— 它由 `automation::execute`
/// 里注入的 notifier 统一发（setup 里注册）。这条路和调度线程到点触发是同一条，
/// 在这儿再 emit 一次只会让「立即运行」提示两遍，而定时触发的那次反而没有。
#[tauri::command]
async fn run_automation_now(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || automation::run_now(&id))
        .await
        .map_err(|e| format!("自动化执行任务异常: {e}"))?
}

/// 一条自动化的历史运行记录（新→旧，只给文件名和时间）。
#[tauri::command]
fn list_automation_runs(id: String) -> Vec<serde_json::Value> {
    automation::list_runs(&id)
}

/// 读某次运行的完整结果正文。入参只认 `~/.uking/automation/` 下的纯文件名（automation.rs 里硬拦）。
#[tauri::command]
fn read_automation_run(file: String) -> Result<String, String> {
    automation::read_run(&file)
}

/// 行为时间轴。薄壳，真身是影核动作 `runtime.journal.inspect`。
#[tauri::command]
async fn journal_inspect(days: Option<i64>) -> serde_json::Value {
    run_action_input(actions::JOURNAL_INSPECT, serde_json::json!({ "days": days.unwrap_or(1) })).await
}

/// 开 / 关本地行为记录。
///
/// ★ **故意不做成影核动作** —— 不是漏了。
/// 时间轴是**问责机制**：它记的就是「AI 干了什么」。把「关掉记录」做成动作，就等于把
/// 关闸门的手交给被记录的那一方 —— AI 通过 MCP 调一次 `journal.set_enabled(false)`，
/// 后面干什么都不会留痕，而报告依然一片正常。这不是理论风险，是把审计日志和被审计者
/// 放进同一个权限域的经典错误。
///
/// 所以这个开关**只从 GUI 走**（人坐在机器前，亲手关）。同理 [`journal_clear`]。
/// 代价是 CLI/MCP 关不了它 —— 这个代价是**故意付的**。
#[tauri::command]
fn journal_set_enabled(enabled: bool) -> Result<(), String> {
    // 开关本身要留痕：不然「这段时间为什么是空的」永远查不清 ——
    // 「关了记录」和「什么都没发生」是两件完全不同的事，时间轴必须能区分。
    //
    // 顺序有讲究：**关**的那条得赶在开关落下之前写（之后就写不进去了），
    // **开**的那条得等开关打开之后写。写反了就是那条痕永远不出现。
    if !enabled {
        journal::note("journal.disabled", "用户关闭了行为记录 —— 此后的动作不再留痕");
    }
    journal::set_enabled(enabled)?;
    if enabled {
        journal::note("journal.enabled", "用户开启了行为记录");
    }
    Ok(())
}

/// 清空全部行为记录。客户的数据，客户能自己删干净。理由同上：**只从 GUI 走**。
#[tauri::command]
fn journal_clear() -> Result<(), String> {
    journal::clear()
}

/// uu-switch（去广告版 cc-switch 模型切换器）安装包下载直链（我方下载源，固定名）。
/// 前端「打开下载页 / 手动装」兜底点这个。
#[tauri::command]
fn get_uuswitch_download_url() -> String {
    uuswitch::download_url()
}

/// 下载 + 静默安装 uu-switch（NSIS /S，~8 MB）。进度走事件 `uking:uuswitch_progress`。
/// 装完不改用户任何 AI 配置（切驱动是用户在 uu-switch 里主动做的事）。
#[tauri::command]
async fn install_uuswitch(app: AppHandle) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        uuswitch::install(&move |msg: &str| {
            let _ = app2.emit("uking:uuswitch_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("安装 uu-switch 异常: {e}"))?
}

/// 一键把「虾盘云（Claude + Codex）+ 你在用的工具配置」导入 uu-switch（写 ~/.cc-switch/config.json，
/// 非破坏式合并）。虾盘云用 U-King 设备 Key + 端点 + deepseek-v4-pro / gpt-5.3-codex，两侧切换等效；
/// 在用配置读 ~/.claude、~/.codex 原样搬进来。
#[tauri::command]
async fn import_to_uuswitch(app: AppHandle) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        uuswitch::import_to_uuswitch(&move |msg: &str| {
            let _ = app2.emit("uking:uuswitch_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("导入 uu-switch 异常: {e}"))?
}

/// 进阶区：下载并拉起 Hermes 桌面版安装器（下一步下一步，非静默）。
/// 进度走事件 `uking:hermes_progress`。下载失败返回 Err，前端回退「打开官网下载页」。
#[tauri::command]
async fn install_hermes_app(app: AppHandle) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        tools::install_hermes_app(&move |msg: &str| {
            let _ = app2.emit("uking:hermes_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("安装 Hermes 异常: {e}"))?
}

/// Hermes 官网下载页（前端下载失败时回退打开）。
#[tauri::command]
fn hermes_download_page() -> String {
    tools::hermes_download_page()
}

/// 绿色版「固定到桌面」：给当前运行的 exe 建桌面快捷方式。
#[tauri::command]
async fn pin_to_desktop() -> Result<String, String> {
    let v = run_write_action(actions::DESKTOP_PIN, serde_json::json!({})).await?;
    Ok(action_field(v, "message", serde_json::Value::Null).as_str().unwrap_or("").to_string())
}

// ============================================================
// 一键备份 / 还原（ClawX 对话 + 设置 → U 盘 → 换台电脑接着用）
// ============================================================

/// 建议的默认备份盘（当前 exe 所在盘根，U 盘版多半就是 U 盘）。
#[tauri::command]
fn backup_default_root() -> String {
    backup::default_root()
}

/// 列出某个目录（U 盘根）下已有的快照，新→旧。
#[tauri::command]
async fn list_backups(root: String) -> Vec<backup::BackupEntry> {
    tauri::async_runtime::spawn_blocking(move || backup::list(&root))
        .await
        .unwrap_or_default()
}

/// 备份到 U 盘（进度走事件 `uking:backup_progress`）。
#[tauri::command]
async fn backup_now(app: AppHandle, dest_root: String) -> Result<serde_json::Value, String> {
    let v = run_write_action_progress(
        app,
        actions::BACKUP_CREATE,
        "uking:backup_progress",
        serde_json::json!({ "dest_root": dest_root }),
    )
    .await?;
    Ok(action_field(v, "result", serde_json::json!({})))
}

/// 从某个快照目录还原到本机（整份替换，前置自动备份 + 旧目录留底）。
#[tauri::command]
async fn restore_backup(app: AppHandle, backup_dir: String) -> Result<serde_json::Value, String> {
    let v = run_write_action_progress(
        app,
        actions::BACKUP_RESTORE,
        "uking:backup_progress",
        serde_json::json!({ "backup_dir": backup_dir }),
    )
    .await?;
    Ok(action_field(v, "result", serde_json::json!({})))
}

/// 打开「Codex 桌面版手动安装教程」网页（自动装不上时的兜底引导）。
///
/// 为什么不直接开线上 URL：国内裸网 Vercel(u-king.org) 经常打不开，线上副本又可能没同步。
/// 所以把教程 HTML **内嵌进 exe**（与 website/codex-install.html 同源），运行时写到临时文件、
/// 用默认浏览器打开 —— 离线可用，且页面里的 ms-windows-store:// 链接在真浏览器里才能拉起商店。
const CODEX_GUIDE_HTML: &str = include_str!("../../website/codex-install.html");
/// Codex CLI（命令行版）手动安装教程 —— npm 装不上时的兜底（与桌面版那份独立）。
const CODEX_CLI_GUIDE_HTML: &str = include_str!("../../website/codex-cli-install.html");
/// Claude Code 手动安装教程（npm）。
const CLAUDE_GUIDE_HTML: &str = include_str!("../../website/claude-code-install.html");
/// OpenClaw 龙虾手动安装教程（命令行 openclaw + 图形版 ClawX）。
const OPENCLAW_GUIDE_HTML: &str = include_str!("../../website/openclaw-install.html");
/// Hermes Agent 手动安装教程（pip）。
const HERMES_GUIDE_HTML: &str = include_str!("../../website/hermes-install.html");
/// 「装不上怎么办」通用补救总页（代理/Node/PowerShell/换源 4 大坑）。
const INSTALL_HELP_HTML: &str = include_str!("../../website/install-help.html");
/// 「获取 API Key」各模型教程页（虾盘云推荐 + DeepSeek/GLM/Kimi 平铺）。
const APIKEY_GUIDE_HTML: &str = include_str!("../../website/apikey-guide.html");
/// Codex 高级配置：本地模型（--oss/Ollama）+ 自定义云驱动（model_providers）。进阶选配。
const CODEX_LOCAL_HTML: &str = include_str!("../../website/codex-local-models.html");

/// 教程线上源（依次尝试，第一个拉到合法 HTML 的生效）。国内首选 u-claw.org.cn
/// （与 skill/bug 同源服务器；api/cloud.u-claw.org 国内 SNI 被 reset）。
/// 线上优先 = 我们随时能改教程不必发版；拉不到再回退内嵌副本（离线/不可达也不掉链子）。
fn online_guide_urls(slug: &str) -> Vec<String> {
    [
        "https://u-claw.org.cn/uking",
        "https://www.u-king.org",
        "https://u-king-org.vercel.app",
    ]
    .iter()
    .map(|b| format!("{b}/{slug}"))
    .collect()
}

/// 尝试拉线上教程（短超时），成功且像 HTML 才返回。
fn fetch_online_guide(slug: &str) -> Option<String> {
    for url in online_guide_urls(slug) {
        // -f：HTTP 错误码当失败回退；短超时避免装不上的客户再干等
        if let Ok(body) = installer::curl(&["-fL", "-sS", "-m", "8", &url]) {
            let t = body.trim_start();
            if t.len() > 500 && (t.starts_with("<!doctype") || t.starts_with("<!DOCTYPE") || t.starts_with("<html")) {
                return Some(body);
            }
        }
    }
    None
}

/// 打开教程：先拉线上版（可在线改），失败回退内嵌副本；写临时文件用默认浏览器打开。
/// 真浏览器里 ms-windows-store:// / 复制按钮等才正常；app 内 webview 不行。
/// `slug` = 线上路径（如 `codex-cli-install.html`），`html` = 内嵌兜底。
fn open_embedded_html_with_slug(file_name: &str, slug: &str, html: &str) -> Result<(), String> {
    let body = fetch_online_guide(slug).unwrap_or_else(|| html.to_string());
    let path = std::env::temp_dir().join(file_name);
    std::fs::write(&path, &body).map_err(|e| format!("写教程文件失败: {e}"))?;
    let p = path.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // start "" <file>：空标题参数必带，否则 start 把带空格的路径当标题
        std::process::Command::new(crate::installer::system_tool("cmd"))
            .args(["/C", "start", "", &p])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| format!("打开教程失败: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&p)
            .spawn()
            .map_err(|e| format!("打开教程失败: {e}"))?;
    }
    Ok(())
}

/// 打开「Codex 桌面版手动安装教程」网页（自动装不上时的兜底引导）。
///
/// 为什么不直接开线上 URL：国内裸网 Vercel(u-king.org) 经常打不开，线上副本又可能没同步。
/// 所以把教程 HTML **内嵌进 exe**（与 website/codex-install.html 同源），离线可用。
#[tauri::command]
fn open_codex_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-codex-install.html", "codex-install.html", CODEX_GUIDE_HTML)
}

/// 打开「Codex CLI 手动安装教程」网页（npm 装不上时的兜底，含可复制的命令 + 国内/官方源链接）。
#[tauri::command]
fn open_codex_cli_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-codex-cli-install.html", "codex-cli-install.html", CODEX_CLI_GUIDE_HTML)
}

/// 打开「Claude Code 手动安装教程」网页。
#[tauri::command]
fn open_claude_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-claude-install.html", "claude-code-install.html", CLAUDE_GUIDE_HTML)
}

/// 打开「OpenClaw 手动安装教程」网页（含 ClawX 图形版下载）。
#[tauri::command]
fn open_openclaw_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-openclaw-install.html", "openclaw-install.html", OPENCLAW_GUIDE_HTML)
}

/// 打开「Hermes 手动安装教程」网页（pip）。
#[tauri::command]
fn open_hermes_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-hermes-install.html", "hermes-install.html", HERMES_GUIDE_HTML)
}

/// 打开「装不上怎么办」通用补救总页。
#[tauri::command]
fn open_install_help() -> Result<(), String> {
    open_embedded_html_with_slug("uking-install-help.html", "install-help.html", INSTALL_HELP_HTML)
}

/// 打开「获取 API Key」各模型教程页（虾盘云推荐 + DeepSeek/GLM/Kimi）。
#[tauri::command]
fn open_apikey_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-apikey-guide.html", "apikey-guide.html", APIKEY_GUIDE_HTML)
}

/// 官网入口多端点探测（会审裁决 P0：点击「官网」保证国内能打开）。
///
/// 历史：官网按钮一度直开裸域 https://u-king.org/（境外 200、境内 SNI reset 点不动）。
/// 现在按 sol 复审要求由 **Rust 后端** 探测（前端 fetch 受 CORS 限制不采信），
/// u-claw.org.cn/uking/ 首选（境内实测 200），www.u-king.org 备选（境外可达）；
/// 判据 = `-f` 保证 HTTP 200 + 正文含 `U-King` 与 `<html` 特征（防劫持页/运营商插页返 200），
/// 不是只看 TCP 通。短超时（连接 1s / 总 2s）防「点了没反应」。全挂 fallback 国内地址：
/// 宁可打开副本页，不让入口变死链。进程内缓存成功端点（官网入口是只读且极少变）。
#[tauri::command]
fn resolve_site_url() -> Result<String, String> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<String>> = Mutex::new(None);
    if let Ok(g) = CACHE.lock() {
        if let Some(u) = g.as_ref() {
            return Ok(u.clone());
        }
    }
    const CANDIDATES: [&str; 2] = [
        "https://u-claw.org.cn/uking/",
        "https://www.u-king.org/",
    ];
    let picked = CANDIDATES.iter().find(|u| {
        installer::curl(&["-fL", "-sS", "-m", "2", "--connect-timeout", "1", u])
            .map(|b| b.contains("U-King") && b.to_ascii_lowercase().contains("<html"))
            .unwrap_or(false)
    });
    let final_url = picked.unwrap_or(&CANDIDATES[0]).to_string();
    if let Ok(mut g) = CACHE.lock() {
        *g = Some(final_url.clone());
    }
    Ok(final_url)
}

/// 打开「Codex 高级配置」教程页（本地模型 --oss/Ollama + 自定义云 model_providers，进阶选配）。
#[tauri::command]
fn open_codex_local_guide() -> Result<(), String> {
    open_embedded_html_with_slug("uking-codex-local-models.html", "codex-local-models.html", CODEX_LOCAL_HTML)
}

/// 通用「动态内容中心」入口 —— 打开服务器下发的任意网页（按 slug 路由）。
///
/// 为什么单独做：安装方法/活动/课程这些**会频繁变**的内容，做成服务器 html、软件只留入口，
/// 这样改内容不必发版。以后想加第 N 个同步页面，**前端加个按钮传新 slug、服务器加个 html 即可，
/// 后端零改动**。线上优先（u-claw.org.cn/uking/<slug>）+ 拉不到时显示离线占位（不白屏）。
///
/// 安全：slug 仅允许 `[a-z0-9-_.]`（防路径穿越/注入），且强制 `.html` 结尾。
#[tauri::command]
fn open_online_page(slug: String) -> Result<(), String> {
    let s = slug.trim();
    let safe = !s.is_empty()
        && s.ends_with(".html")
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c));
    if !safe {
        return Err(format!("非法页面标识：{slug}"));
    }
    // 内嵌兜底 = 离线占位页（线上拉到就用线上，拉不到才显示这个，避免白屏）。
    let fallback = format!(
        "<!doctype html><html lang=\"zh\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>U-King</title><style>body{{background:#0b0e16;color:#e6e8ef;\
         font-family:system-ui,'Microsoft YaHei',sans-serif;display:flex;min-height:100vh;\
         margin:0;align-items:center;justify-content:center;text-align:center;padding:24px}}\
         .c{{max-width:480px}}h1{{font-size:20px;margin:0 0 12px}}p{{color:#9aa0b4;line-height:1.7}}\
         a{{color:#7aa2ff}}</style></head><body><div class=\"c\">\
         <h1>暂时无法加载内容</h1>\
         <p>这个页面的内容由服务器实时下发，当前没能连上。<br>\
         请检查网络后重试，或稍后再来看。</p>\
         <p style=\"font-size:12px;color:#5b6178\">页面：{s}</p></div></body></html>"
    );
    // 临时文件名按 slug 取（去掉非法字符已保证），避免多页互相覆盖。
    let file_name = format!("uking-page-{}", s.replace('/', "-"));
    open_embedded_html_with_slug(&file_name, s, &fallback)
}

/// 拉「动态内容清单」JSON —— 最新动态 / AI 学院的**列表数据**（不是整张网页）。
///
/// 为什么单独做：动态/学院做成 app 内原生列表（对齐「使用教程」风格、点侧栏只切 tab 不弹窗），
/// 但列表内容仍要能随服务器改、不发版。所以服务器只下发一份 JSON 清单
/// （`{ "items": [{ title, summary, tag, date, slug?/url? }, …] }` 或裸数组），
/// app 拿到原始 JSON 字符串自己解析、渲染成卡片；点某一条才用 `open_online_page` / 浏览器打开详情。
///
/// 线上优先（u-claw.org.cn/uking/<slug>，与教程/skill 同源）；拉不到返回 Err，前端显示离线占位（不白屏）。
/// 安全：slug 仅允许 `[a-z0-9-_.]`、强制 `.json` 结尾、禁 `..`（防路径穿越/注入）。
#[tauri::command]
async fn fetch_online_feed(slug: String) -> Result<String, String> {
    // 校验是廉价纯字符串运算，放 spawn 外先做。
    let s = slug.trim().to_string();
    let safe = !s.is_empty()
        && s.ends_with(".json")
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c));
    if !safe {
        return Err(format!("非法清单标识：{slug}"));
    }
    // ★ 网络 curl 必须在阻塞线程跑：同步命令会在 **UI 主线程** 执行，串行试 3 个地址
    // （其中 www.u-king.org / vercel 国内连不上，各干等满超时）会把整窗冻住几十秒
    // —— 首屏第一落点就是拉 Feed 的页面，这正是「加载慢/卡死」的头号真因。
    tauri::async_runtime::spawn_blocking(move || {
        for url in online_guide_urls(&s) {
            // -f：HTTP 错误码当失败回退；--connect-timeout 让连不上的域名快速放弃，别干等满 -m
            if let Ok(body) =
                installer::curl(&["-fL", "-sS", "-m", "6", "--connect-timeout", "4", &url])
            {
                let t = body.trim_start();
                if t.starts_with('{') || t.starts_with('[') {
                    return Ok(body);
                }
            }
        }
        Err("拉取动态清单失败（网络不可达或服务器无此清单）".into())
    })
    .await
    .map_err(|e| format!("拉取异常: {e}"))?
}

/// 按工具 id 打开对应手动安装教程（前端统一入口：装失败时按 tool 路由到正确的教程页）。
#[tauri::command]
fn open_install_guide(tool: String) -> Result<(), String> {
    match tool.as_str() {
        "claude-code" => open_claude_guide(),
        "codex" => open_codex_cli_guide(),
        "codex-app" => open_codex_guide(),
        "openclaw" | "clawx" => open_openclaw_guide(),
        "hermes" => open_hermes_guide(),
        // 未知工具或泛指 → 通用补救总页
        _ => open_install_help(),
    }
}

/// Codex 专区：轻量状态（Codex 桌面版装了没 + 驱动接管了没）。
/// computer use 怎么用改成前端图文教程，不再探上游私有运行时路径（瘦身，降维护）。
/// 薄壳，真身是影核动作 `runtime.codex.inspect`。
#[tauri::command]
async fn codex_status() -> serde_json::Value {
    run_action_blocking(actions::CODEX_INSPECT).await
}

/// ★ 用量口径之二：从内置终端开某个工具（`tool` tag 由前端 `apps.ts` 给）。
///
/// 薄壳只多做一件事——记一次 `tool_use`。PTY 逻辑仍全在 term.rs，它不认识 metrics
/// （功能模块之间不互相 import，组合根在这儿接）。`tool` 为空 = 纯开终端，不算谁的使用。
#[tauri::command]
async fn term_open(
    cols: u16,
    rows: u16,
    on_data: tauri::ipc::Channel<Vec<u8>>,
    initial_cmd: Option<String>,
    cwd: Option<String>,
    tool: Option<String>,
) -> Result<String, String> {
    let tag = tool.clone();
    let r = term::term_open_pty(cols, rows, on_data, initial_cmd, cwd, tool).await;
    if r.is_ok() {
        if let Some(t) = tag {
            metrics::log_tool_use(&t, "term");
        }
    }
    r
}

/// 启动一个 GUI 应用（Codex 桌面版 / ClawX），不进终端。
#[tauri::command]
fn launch_app(app: String) -> Result<(), String> {
    let r = tools::launch_app(&app);
    // ★ 用量口径之一：GUI 应用被启动。只记我们自己发起的启动（见 metrics::log_tool_use）。
    if r.is_ok() {
        metrics::log_tool_use(&app, "gui");
    }
    r
}

/// U-Chat「打开方式」：把文件/文件夹在外部应用里打开
/// （资源管理器/在资源管理器中显示/VS Code/Cursor/系统终端/Git Bash）。
#[tauri::command]
fn open_dir_external(path: String, app: String) -> Result<(), String> {
    tools::open_dir_external(&path, &app)
}

/// 放行文件面板预览：把某目录（及其子孙）加进 asset 协议 scope。
///
/// 为什么需要：asset 协议默认只放行 `~/.uking/video/*`（tauri.conf.json 的 assetProtocol.scope），
/// 用户在文件面板打开**任意**工作文件夹后，redline 预览走 `convertFileSrc` 取文件字节 ——
/// 目录不在 scope 里就 **HTTP 403（文件预览失败）**。这里按用户实际打开的根目录动态放行，
/// 既能预览任意文件夹，又不必把全盘静态开进配置（比 `**` 静态放行更收敛）。
#[tauri::command]
fn allow_fs_preview(app: AppHandle, path: String) -> Result<(), String> {
    let p = path.trim();
    if p.is_empty() {
        return Err("空路径".into());
    }
    app.asset_protocol_scope()
        .allow_directory(p, true)
        .map_err(|e| format!("授权预览目录失败: {e}"))
}

/// 办公文档 → PDF（借 LibreOffice headless），给「客户拿来的那一份」出真版式预览。
///
/// 返回 `Ok(None)` = **这台机器没装 LibreOffice，或这个格式不归它管** —— 不是故障，
/// 前端安静退回原来的文字大纲档。返回 PDF 路径时顺手把缓存目录放行进 asset scope，
/// 否则前端 `convertFileSrc` 取字节会 403（同 `allow_fs_preview` 那个坑，别再踩一次）。
///
/// 转换会烧十几秒，放 `spawn_blocking` 里跑，绝不占着 UI 线程。
#[tauri::command]
async fn office_to_pdf(app: AppHandle, path: String) -> Result<Option<String>, String> {
    let pdf = tauri::async_runtime::spawn_blocking(move || officedoc::to_pdf(&path))
        .await
        .map_err(|e| format!("转换任务失败: {e}"))??;
    if let Some(p) = &pdf {
        if let Some(dir) = std::path::Path::new(p).parent() {
            let _ = app.asset_protocol_scope().allow_directory(dir, false);
        }
    }
    Ok(pdf)
}

/// 把窗口隐藏到托盘。
#[tauri::command]
fn hide_to_tray(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

// ---------- v0.2：安装向导 + 驱动切换 ----------

/// 体检：node / npm / claude / codex / git / Claude 桌面版。
///
/// 薄壳：真身是影核动作 `runtime.stack.inspect`，GUI 与 `action run` 走同一条路。
/// 返回的 JSON 形状与迁移前逐字节一致，前端不用改。
#[tauri::command]
async fn detect_stack() -> serde_json::Value {
    run_action_blocking(actions::STACK_INSPECT).await
}

/// Hermes 浏览器接管能力体检：区分「聊天可用」和「能不能打开网页/截图」。
/// 薄壳，真身是影核动作 `runtime.hermes_browser.inspect`。
#[tauri::command]
async fn hermes_browser_status() -> serde_json::Value {
    run_action_blocking(actions::HERMES_BROWSER_INSPECT).await
}

/// 平台路由的 AI 优化体检 JSON：Windows 走 ukrt.exe 薄壳，macOS 走原生 macopt
/// （ukrt 是 Windows 专属二进制）。`airuntime_doctor` 命令与 `--selfcheck` 共用同一份路由，
/// 避免 selfcheck 在 Mac 上误调 ukrt 路径报「读不到 USERPROFILE」（一台 Mac 客户机实锤）。
fn airuntime_doctor_routed() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        macopt::doctor_json()
    }
    #[cfg(not(target_os = "macos"))]
    {
        airuntime::doctor_json()
    }
}

/// 「AI 加速」板块：ukrt/macopt 体检（返回 JSON 字符串，前端 JSON.parse）。
/// 「AI 工具专项修复」的体检：这台机器上 claude / codex 到底解析成了什么、起不起得来。
///
/// 为什么它值得单独存在，而不是并进 `detect_stack`：`detect_stack` 回答的是**装没装**，
/// 而 0.9.83 那个 P0 恰恰证明了「装着、`--version` 也正常、但一个字都跑不出来」是可能的
/// （批处理壳拒绝含换行的参数 → 多行提问和所有 AI 专家全废）。装没装和起不起得来
/// 是两件事，报告里就得是两行。
///
/// 真起一次进程做判断，**烧 0 token**。与无头自检 `--agent-launch-test` 共用 `agent::probe_all()`。
#[tauri::command]
async fn agent_launch_probe() -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(agent::probe_all)
        .await
        .unwrap_or_else(|_| serde_json::json!([]))
}

/// 进程级隔离：只 spawn 独立的 ukrt.exe，崩溃/缺失不连坐其他板块。
/// 薄壳，真身是影核动作 `runtime.optimizer.inspect`。动作把「引擎缺失」当**结论**返回，
/// 这里再翻回老命令的 Result 契约（前端仍然 JSON.parse 成功值 / 走 catch 显示失败）。
#[tauri::command]
async fn airuntime_doctor() -> Result<String, String> {
    let v = run_action_blocking(actions::OPTIMIZER_INSPECT).await;
    if v.get("ok") == Some(&serde_json::Value::Bool(true)) {
        serde_json::to_string(&action_field(v, "report", serde_json::json!({}))).map_err(|e| e.to_string())
    } else {
        Err(action_field(v, "error", serde_json::Value::Null).as_str().unwrap_or("优化引擎不可用").to_string())
    }
}

/// 优化大师「动手」那一半的**唯一实现**：跑一次 ukrt 动作，并在前后各取一次分写锚点。
///
/// ★ 数据基台锚点：改机器的动作，前后各取一次分并写 `optimize` 事件。
/// 锚点写在**后端**而不是等前端转述 —— 前端不调就没有锚点，
/// 而没有锚点，后面所有「优化后快了多少 / 报错少了多少」都切不出来。
/// 🔴 锚点在这一层而不是在 `airuntime_run` 里，是因为现在调用方不止 GUI 一个：
/// 影核动作 `runtime.optimizer.apply` 走的是同一条，CLI / MCP / AI 专家改了机器
/// 也一样留下 before/after。挂在 GUI 那层的话，专家一动手，时间轴上就是一段空白。
fn optimizer_apply(action: &str) -> Result<String, String> {
    let mutating = is_mutating_optimize(action);
    let before = if mutating { doctor_score() } else { None };
    let r = {
        #[cfg(target_os = "macos")]
        {
            macopt::run_action(action)
        }
        #[cfg(not(target_os = "macos"))]
        {
            airuntime::run_action(action)
        }
    };
    if mutating && r.is_ok() {
        metrics_anchor_optimize(action, before, doctor_score());
    }
    r
}

/// 「AI 加速」板块：fix / optimize / undo / defender（模块内白名单校验）。
///
/// 薄壳：fix / optimize / defender 走影核动作 `runtime.optimizer.apply` ——
/// GUI 按钮和 CLI / MCP / AI 装机医生调的是同一条实现（宪法第 13 条）。
/// 🔴 `undo` **没有**登记成动作（`ukrt undo` 每调一次剥一层 journal，不幂等，
/// 见 `actions::OPTIMIZER_APPLY` 注释），所以它仍直连 —— 少一个调用方，
/// 但不假装一个不兑现的幂等承诺。
#[tauri::command]
async fn airuntime_run(action: String) -> Result<String, String> {
    if action == "undo" {
        return tauri::async_runtime::spawn_blocking(|| optimizer_apply("undo"))
            .await
            .map_err(|e| e.to_string())?;
    }
    let v = run_write_action(actions::OPTIMIZER_APPLY, serde_json::json!({ "action": action })).await?;
    Ok(action_field(v, "output", serde_json::Value::Null)
        .as_str()
        .unwrap_or_default()
        .to_string())
}

/// 「AI 加速」板块：以管理员跑一次 `ukrt fix`——开 longpaths + devmode 这两个 HKLM 项，是「到 100 分」
/// 绕不开的一步（非管理员改不了 HKLM）。Windows 弹一次 UAC 复用 journal 可回滚；macOS 无 HKLM 概念、
/// 一键优化本就免管理员，直接跑 optimize 兜住语义。跑完前端重新 doctor 看新分。
#[tauri::command]
async fn airuntime_fix_elevated() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        // 同 airuntime_run：提权 fix 也改机器，一样要留锚点。
        let before = doctor_score();
        let r = {
            #[cfg(target_os = "macos")]
            {
                macopt::optimize()
            }
            #[cfg(not(target_os = "macos"))]
            {
                airuntime::run_fix_elevated()
            }
        };
        if r.is_ok() {
            metrics_anchor_optimize("fix_elevated", before, doctor_score());
        }
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 「AI 加速」板块：匿名上报分数拿百分位（仅 score+version+os，无设备指纹）。best-effort。
#[tauri::command]
async fn airuntime_report_score(score: u32, version: String) -> Option<i64> {
    tauri::async_runtime::spawn_blocking(move || airuntime::report_score(score, &version))
        .await
        .ok()
        .flatten()
}

// ============================================================
// 数据基台（metrics）—— 组合根接线
// ============================================================
//
// `metrics` 不 import `usage_local` / `envfp`，它们也不认识 `metrics`：
// 功能模块之间禁止互相依赖，两边由 lib.rs 这个组合根接起来（与
// `breakdown(days, rtk::is_active())` 注入压缩机状态同一手法）。

/// 把 `usage_local` 的即时聚合落成**当天的用量快照**。
///
/// `usage_local` 是扫完就丢的即时统计，没有时间轴；这一步给它落盘，
/// 「优化前长什么样」才有处可查。同一天重复调是覆盖语义，不会记重。
/// 会扫日志，**只在后台线程调**。
fn metrics_rollup_now() {
    let u = usage_local::breakdown(1, rtk::is_active());
    let rows: Vec<metrics::UsageRow> = u
        .items
        .iter()
        .map(|i| metrics::UsageRow {
            tool: i.tool.clone(),
            model: i.model.clone(),
            calls: i.count,
            // input_tokens 是总输入（含缓存），减掉缓存两项才是非缓存输入
            input: i
                .input_tokens
                .saturating_sub(i.cache_read_tokens + i.cache_write_tokens),
            output: i.output_tokens,
            cache_read: i.cache_read_tokens,
            cache_write: i.cache_write_tokens,
        })
        .collect();
    metrics::log_usage_rollup(&rows);
}

/// 优化大师的哪些动作算「改了这台机器」——只有它们要写锚点（`undo` 不算优化）。
fn is_mutating_optimize(action: &str) -> bool {
    matches!(action, "fix" | "optimize" | "defender")
}

/// ★ 写优化锚点：before/after 全靠这条切。
///
/// 前后各跑一次 doctor 取分。多两次 ukrt 调用（各百来毫秒）——相对于用户主动点的
/// 一次优化（本来就要几秒）完全无感，换来的是**分数变化真实可查**而不是靠前端转述。
fn metrics_anchor_optimize(action: &str, score_before: Option<u32>, score_after: Option<u32>) {
    let recipes = vec![action.to_string()];
    std::thread::spawn(move || {
        let env = serde_json::to_value(envfp::current()).unwrap_or(serde_json::Value::Null);
        metrics::log_optimize(&recipes, score_before, score_after, env);
    });
}

/// 从 doctor 的 JSON 里抠出总分（抠不到就是 None —— **不许拿 0 冒充**）。
fn doctor_score() -> Option<u32> {
    let raw = airuntime_doctor_routed().ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("score").and_then(|s| s.as_u64()).map(|n| n as u32)
}

/// 本地数据报告 —— **不上传也能看**。这是用户愿意开采集的唯一理由。
#[tauri::command]
async fn metrics_report(days: Option<i64>) -> Result<metrics::MetricsReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 组合根注入环境指纹：metrics 不 import envfp，但环境类建议（磁盘满 / 缺 Git /
        // OneDrive / 长路径）全靠它。传 Null 就只出用量类建议 —— 宁可少给，不猜。
        let env = serde_json::to_value(envfp::current()).unwrap_or(serde_json::Value::Null);
        metrics::report(days.unwrap_or(30), env)
    })
    .await
    .map_err(|e| e.to_string())
}

/// 上传同意开关。**默认关**；只有用户显式打开才会外发聚合指标。
#[tauri::command]
async fn metrics_set_consent(upload: bool) -> Result<(), String> {
    metrics::set_consent(upload)
}

/// 环境指纹。字段已是非隐私口径（中文用户名只记 true/false），可直接给客服看。
#[tauri::command]
async fn metrics_env() -> Result<envfp::EnvFingerprint, String> {
    tauri::async_runtime::spawn_blocking(envfp::current)
        .await
        .map_err(|e| e.to_string())
}

/// 手动触发一次用量快照（前端进「数据」页时调一下，别等第二天）。
#[tauri::command]
async fn metrics_rollup() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(metrics_rollup_now)
        .await
        .map_err(|e| e.to_string())
}

/// `EnvToolsResult` 和它的实现都住在 `installer.rs`（四铁律第 1 条：模块暴露纯函数，
/// 组合根只转调）。这里只 re-export 一下，保住老命令的返回类型名不变。
use installer::EnvToolsResult;

/// 「AI 优化大师·一键优化」把它指出的**缺件真正装上**（而不只是提示去别处装）：
/// 便携 Node + 便携 Git（含 bash.exe，Claude Code 的 Bash 工具刚需；pc-*** 实锤缺 git 就用不了）
/// + 便携 PowerShell 7（客户机常只有 5.1，中文易乱码；让 pwsh7 那 3 分能真拿到而不是只提示 winget）。
/// 三者都免管理员、便携落 ~/.uking/runtime，best-effort（一个失败不拖另一个）。进度走
/// 事件 `uking:optimize_env`（纯文本行）。前端在 fix/optimize 之后调它，再重新体检刷新分数。
#[tauri::command]
async fn optimize_env(app: AppHandle) -> Result<EnvToolsResult, String> {
    let v = run_write_action_progress(app, actions::ENV_INSTALL_TOOLS, "uking:optimize_env", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| format!("环境组件安装结果解析失败: {e}"))
}

// ─────────────────────── 影核协议：动作登记表（组合根） ───────────────────────

fn action_json<T: serde::Serialize>(v: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(v).map_err(|e| format!("serialize_action_result: {e}"))
}

/// 只有缺包/错版能触发 npm；已就绪直接回 `changed:false`，Chrome 会话自身失败则原样报错。
fn browser_runtime_install_decision(preflight: Result<serde_json::Value, String>) -> Result<Option<serde_json::Value>, String> {
    match preflight {
        Ok(ready) => Ok(Some(serde_json::json!({
            "changed": false,
            "version": ready["version"],
            "stream": ready["stream"],
            "snapshot": ready["snapshot"],
        }))),
        Err(e) if e.starts_with("not_installed:") || e.starts_with("version_mismatch:") => Ok(None),
        Err(e) => Err(e),
    }
}

/// 「当前各工具在用哪个驱动」这份状态的版本号。
///
/// 直接对 `driver_status()` 的序列化结果取版本 —— 这样版本变不变**完全等于用户看到的
/// 状态变不变**，不会出现「文件时间戳动了但内容没变」的假冲突。多终端 / uu-switch /
/// 用户手改配置，任何一种改动都会让它变。
fn driver_state_version() -> String {
    let snapshot = serde_json::to_string(&providers::driver_status()).unwrap_or_default();
    actions::version_of(&snapshot)
}

/// 自动化的状态版本。同理：直接对「全部任务」的序列化结果取版本 —— 另一个终端
/// （或 GUI 的另一个面板、或将来的远端影子）在你读之后改过任何一条，这里就会变，
/// 带 `expected_state_version` 的写会被核心挡成 conflict 而不是悄悄覆盖。
///
/// 注意排除 `next_run_at`：它每跑一次就会被调度线程推走，算进版本的话，
/// 客户只是等了一分钟，保存就会莫名其妙报冲突。版本要跟着**用户的意图**变，
/// 不是跟着机器的心跳变。
/// 身份的状态版本。用户可以**直接手改** `~/.uking/identity.json`（那是明文文件，
/// 我们鼓励他改），所以并发冲突不是理论问题：他在记事本里改完保存，
/// 界面上还拿着旧的一份点保存，不带版本就会把他手改的内容悄悄吃掉。
fn identity_state_version() -> String {
    let snapshot = serde_json::to_string(&identity::load_identity_in(&identity::uking_dir()))
        .unwrap_or_default();
    actions::version_of(&snapshot)
}

fn automation_state_version() -> String {
    let mut jobs = automation::list();
    for j in jobs.iter_mut() {
        j.next_run_at = 0;
        j.last_run_at = 0;
        j.runs = 0;
    }
    actions::version_of(&serde_json::to_string(&jobs).unwrap_or_default())
}

/// 逐项清理的**唯一实现**。返回「是否需要退出进程才能完成」（删 `~/.uking` 要走延迟脚本）。
/// 进程退出的编排留在 command 层 —— 那是 app 生命周期，不是业务动作。
fn run_footprint_removal(
    ids: &[String],
    preserve_user_data: bool,
    log: &actions::ProgressSink,
) -> Result<bool, String> {
    let has_destructive_tool = ids.iter().any(|id| {
        matches!(
            id.as_str(),
            "uking-home" | "tool-openclaw" | "tool-clawx" | "tool-hermes" | "tool-codex-app"
        )
    });
    if preserve_user_data && has_destructive_tool {
        let msg = cleanup::archive_demo_user_data(log)?;
        log(&format!("✓ {msg}"));
    }
    let remove_home = ids.iter().any(|i| i.as_str() == "uking-home");
    // 先删非 home 的逐项（home 单独走延迟脚本）。best-effort：单项失败照实上报、继续下一项。
    for id in ids.iter().filter(|i| i.as_str() != "uking-home") {
        match cleanup::remove(id, log) {
            Ok(msg) => log(&format!("✓ {msg}")),
            Err(e) => log(&format!("✗ {id}：{e}")),
        }
    }
    if remove_home {
        log("正在安排：退出后彻底清理 ~/.uking …");
        let _ = context_menu::unregister();
        uninstall::run(log)?; // 清 PATH/快捷方式 + 起延迟删 home 脚本
    }
    Ok(remove_home)
}

/// ClawX 托管式配置的**唯一实现**：关 → 写 → 重启。
///
/// 为什么必须先关：ClawX 运行时持有配置的内存副本，不关就写会被它退出时覆盖回去
/// （「配了没反应」的根因）。沙箱（`UKING_TEST_HOME` 非空）下跳过关/重启、只写配置。
fn apply_clawx_managed_inner(
    provider_id: &str,
    api_key: &str,
    model: Option<&str>,
    emit: &actions::ProgressSink,
) -> Result<providers::ApplyResult, String> {
    let sandbox = std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false);
    let was_running = !sandbox && clawx::is_running();
    if was_running {
        emit("正在关闭 ClawX（对话已自动保存）…");
        clawx::graceful_close();
        if !clawx::wait_exited(3500) {
            emit("ClawX 未响应，正在强制结束…");
            clawx::force_kill();
            clawx::wait_exited(4000);
        }
        // 等 ClawX 退出时的落盘 flush 完，避免和我们的写入打架（覆盖回旧配置的根因）
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    emit("正在写入 ClawX 配置…");
    let r = providers::apply_provider(provider_id, api_key, model, &["clawx".to_string()])?;
    if was_running {
        emit("正在重启 ClawX…");
        let _ = clawx::relaunch();
    }
    emit("完成");
    Ok(r)
}

/// 「一键配好全部」+ Codex 省钱路由收尾。**只此一份实现**（宪法第 13 条）。
fn apply_everywhere_with_cheap_route(
    provider_id: &str,
    api_key: &str,
    model: Option<&str>,
    only: Option<&[String]>,
) -> Result<providers::ApplyAllResult, String> {
    let mut r = providers::apply_xiapan_everywhere(provider_id, api_key, model, only)?;
    // ★ 落一次「一键配置对谁成了、对谁没成」。组合根在这儿接 —— providers 不 import metrics。
    // 放在这个函数而不是各调用点：它是「只此一份实现」，GUI / CLI / 动作三条路都过这儿，
    // 记在这里就不会有哪条路漏记。（`--selfcheck` 直接调 providers，绕开这里，沙箱不污染真实数据。）
    for id in &r.configured_ids {
        metrics::log_tool_apply(id, true, "");
    }
    for (i, id) in r.skipped_ids.iter().enumerate() {
        // skipped 与 skipped_ids 成对 push，同序；取不到就留空，不编原因
        let why = r.skipped.get(i).map(String::as_str).unwrap_or("");
        metrics::log_tool_apply(id, false, why);
    }
    // Codex 默认省钱路由：一键配好也走代理（同 apply_provider 的 hook，小白主路径必须覆盖）
    if r.configured.iter().any(|s| s == "Codex") && ensure_codex_cheap_route(provider_id, model).is_some() {
        for s in r.configured.iter_mut() {
            if s == "Codex" {
                *s = "Codex（DeepSeek 省钱路由）".into();
            }
        }
    }
    Ok(r)
}

/// 应用驱动 + Codex 省钱路由收尾。**只此一份实现**，`runtime.driver.apply` 动作和
/// 老命令 `apply_provider` 都调它（宪法第 13 条：业务动作只实现一次）。
fn apply_provider_with_cheap_route(
    provider_id: &str,
    api_key: &str,
    model: Option<&str>,
    targets: &[String],
) -> Result<providers::ApplyResult, String> {
    let mut r = providers::apply_provider(provider_id, api_key, model, targets)?;
    // Codex 默认省钱路由（见 ensure_codex_cheap_route 注释）：写完直连配置后立刻切代理路由
    if r.codex.is_some() && targets.iter().any(|t| t == "codex") {
        if let Some(note) = ensure_codex_cheap_route(provider_id, model) {
            r.codex = r.codex.map(|s| format!("{s} · {note}"));
        }
    }
    Ok(r)
}

/// 当前真正由设备钱包供 Key 的工具；不触碰官方登录、自备 Key 或其它中转。
fn device_wallet_consumer_targets() -> Vec<String> {
    let status = providers::driver_status();
    providers::APPLY_ALL_TARGETS
        .iter()
        .filter(|target| status.active.get(**target).is_some_and(|id| id == "xiapan"))
        .map(|target| (*target).to_string())
        .collect()
}

/// 设备钱包 Key 变化的唯一消费者同步入口。Some 更新，None 先还原再允许清本机钱包。
///
/// 🔴 **逐目标带「它当前的模型」，不传 None。** `apply_*` 对 `model=None` 的语义是
/// 「写 preset 默认模型」（apply_claude 无条件写 `ANTHROPIC_MODEL=p.model`），不是
/// 「沿用现有配置」—— 换 Key 若整体传 None，客户挑的 glm-5 会被静默重置回
/// deepseek-v4-flash（正是 08-22 删掉的旧实现 `refresh_key_where_routed_to_us`
/// 用 driver_status 逐工具防的那个病；那次合并两份实现时只对齐了目标选择，
/// 漏了对齐模型保持）。所以这里按工具各取 `driver_status` 里它现在的模型：
/// 取得到就原样写回，取不到（该工具没有专属模型字段）才落回 preset 默认。
fn sync_device_wallet_consumers(key: Option<&str>) -> Result<(), String> {
    let targets = device_wallet_consumer_targets();
    if targets.is_empty() {
        return Ok(());
    }
    match key {
        Some(key) => {
            let st = providers::driver_status();
            // 逐目标决定「写什么模型 / 要不要碰」：
            // · DriverStatus 有专属字段 → 用它（claude/codex/clawx/hermes/dsh）；
            // · 其余四工具回读各自配置文件的当前模型（effective_config）。
            // 🔴 回读发现该工具当前端点**不指虾盘云** = 客户自己把它切去了别家中转，
            //   本轮整体跳过 —— 换我们的 Key 不该砸掉客户自建的路由
            //   （与 OpenClaw 引擎「尊重 effectivePrimary 路由」同一哲学）。
            // 读不到模型（配置不存在/解析不动）→ None = 写 preset 默认：该工具多半还没
            // 被我们接管过，本来就该按默认配好。
            let mut skips: Vec<String> = Vec::new();
            let model_of = |tool: &str, skips: &mut Vec<String>| -> Option<String> {
                if let Some(m) = match tool {
                    "claude" => st.claude_model.clone(),
                    "codex" => st.codex_model.clone(),
                    "clawx" => st.clawx_model.clone(),
                    "hermes" => st.hermes_model.clone(),
                    "dsh" => st.dsh_model.clone(),
                    "pi" | "opencode" | "qwen" | "crush" | "cline" => {
                        let ec = providers::effective_config(tool);
                        // 端点读得到且不是虾盘云 → 别家的路由，跳过。
                        // 端点读不到（文件不在/没配全）→ 当未接管，照常写默认。
                        if let Some(base) = &ec.base_url {
                            if !providers::is_xiapan_endpoint(base) {
                                skips.push(format!(
                                    "{tool}（当前走 {}，非虾盘云路由，不动）",
                                    base
                                ));
                                return None;
                            }
                        }
                        ec.model
                    }
                    // 其余工具没有专属 model 字段 —— None = 写 preset 默认。
                    _ => None,
                } {
                    Some(m).filter(|m| !m.trim().is_empty())
                } else {
                    None
                }
            };
            // 一目标一调：apply_provider 只收一个全局 model_override，
            // 逐个带各自的模型才能做到「换 Key 不动模型」。目标数 ≤ 全表，开销可忽略。
            // 🔴 单目标失败**不中断**其余目标（对齐被收编的旧实现）：Claude 写失败
            // 不该挡住 Codex / ClawX 拿到新 Key —— 全部跑完再把错误合并上抛。
            let mut errs: Vec<String> = Vec::new();
            for t in &targets {
                let model = model_of(t, &mut skips);
                if model.is_none() && skips.iter().any(|s| s.starts_with(t.as_str())) {
                    continue; // 别家路由，本轮不碰这个目标
                }
                let res = apply_provider_with_cheap_route("xiapan", key, model.as_deref(), std::slice::from_ref(t));
                if let Err(e) = res {
                    errs.push(format!("{t}: {e}"));
                }
            }
            for s in &skips {
                ulog::write("device-wallet", &format!("换 Key 跳过 {s}"));
            }
            if !errs.is_empty() {
                return Err(errs.join("；"));
            }
        }
        None => {
            // 🔴 与 Some 分支对称：逐目标收集错误，单目标还原失败不中断其余目标
            // （pi 终审 2026-08-26 指出的对称性缺口）。
            let mut errs: Vec<String> = Vec::new();
            for t in &targets {
                if let Err(e) = apply_provider_with_cheap_route(
                    "official",
                    "",
                    None,
                    std::slice::from_ref(t),
                ) {
                    errs.push(format!("{t}: {e}"));
                }
            }
            if !errs.is_empty() {
                return Err(errs.join("；"));
            }
        }
    }
    Ok(())
}

/// 全部动作在这里登记。**只有本文件认识业务模块**，`actions.rs` 只有协议 ——
/// 所以删掉功能模块，原则上只动 lib.rs + 前端两处。
/// 🔴 但这条**不是自动成立的**：2026-08-11 删本地大模型时发现 `detect_hardware` 已被
/// AiRuntime.tsx 直接调用，`hardware.rs` 因此删不掉；`mcp.rs` 同样被 cleanup.rs 咬住。
/// 加新模块时要主动守这条，不能事后指望它。
///
/// 加一个动作要想清楚三件事：只读还是会改机器（`effect`）、多久算卡死（`timeout_ms`）、
/// 调用方真的会读哪几个字段（`required`）。第三项就是 `action conformance` 的断言依据 ——
/// 写全了，以后重构悄悄改字段名会当场被抓住。
/// 配方表 —— 「几个动作怎么组合能办成一件事」。**组合根在这儿**，同 `action_table()`：
/// `actions.rs` 是零业务依赖的纯协议核心，它不认识这些配方讲的是什么业务。
///
/// 写配方的三条纪律（跟动作表同源）：
/// 1. **`when` 用客户会说的话写**，不是我们的术语 —— 它是给 AI 做匹配用的，
///    而客户说的是「AI 一直转圈」，不是「sdk-cli 路径静默超阈值」。
/// 2. **`note` 必须回答「为什么这一步在这个位置」**。没有它，配方就退化成一串 id，
///    读的人照样不知道能不能换顺序 —— 而顺序恰恰是配方唯一比动作表多出来的信息。
/// 3. **`verify` 必须可核对**（读哪个动作的哪个字段），不许写「应该就好了」。
///    给不出可核对的判据，说明这条配方我们自己也没验过，那就别放进说明书。
pub(crate) fn recipe_table() -> Vec<actions::Recipe> {
    use actions::{Recipe, Step};
    vec![
        // ★ pc-***（2026-08-03）那一个多小时的手工排障，压成一条配方。
        Recipe {
            id: "recipe.chat.diagnose_stall",
            title: "客户说「AI 一直转圈 / 卡住不动」，查它到底卡在哪",
            when: "客户报「AI 没反应」「转圈半天」「点了没动静」——你还不知道是它在跑一条慢命令、在等模型、还是整个进程已经死了",
            preconditions: &[
                "U-King 0.9.88 以下的版本不写对话日志，第一步会返回 ready:false —— 那时候只能先让客户升级，别在旧版本上查",
            ],
            steps: &[
                Step {
                    action: "runtime.chat.inspect",
                    note: "★ 必须第一步。只有它能区分「工具在跑」和「在等模型」—— phase 字段直接给答案，而这正是最贵的那个分叉：前者查客户的环境，后者查线路和上游，方向完全相反",
                },
                Step {
                    action: "runtime.ai_process.inspect",
                    note: "上一步说没有在跑的轮次、但客户坚持说卡着 → 多半是进程被外部结束了（杀软/清理工具）。这条查的是「崩了还是被杀」，跟上一条不是一回事",
                },
                Step {
                    action: "runtime.network.inspect",
                    note: "phase 停在「等模型回话/首字节」才需要这步。停在「工具执行」时它给不出任何有用信息，跳过",
                },
                Step {
                    action: "runtime.diagnostics.collect",
                    note: "最后再拿整机现状。上来就调它会拿到一大堆日志却答不了「卡在哪一步」那一句 —— 顺序反了就是白烧一遍",
                },
            ],
            verify: "runtime.chat.inspect 的 stalled_now 非空 = 现在真卡着，取其 phase 定位；为空但 recent 里有 status=timeout = 卡过且已被看门狗收尾",
        },
        Recipe {
            id: "recipe.driver.switch_all",
            title: "把这台机器上所有 AI 工具统一切到虾盘云",
            when: "客户说「模型连不上」「报 401」「想统一走一个 Key」「换了机器要重新配」",
            preconditions: &[],
            steps: &[
                Step { action: "runtime.stack.inspect", note: "先看这台机器上到底装了哪几个工具 —— 一个都没装的话切驱动是空转" },
                Step { action: "runtime.driver.inspect", note: "记下改之前是什么，出问题好回滚；也顺便读到 state_version" },
                Step { action: "runtime.driver.apply_everywhere", note: "写动作，要 --yes。后端自己探已装工具，不吃调用方传的列表 —— 传错列表会漏配" },
                Step { action: "runtime.driver.inspect", note: "★ 改完必须回读。「调用成功」不等于「盘上那份真是我们写的」（杀软回滚、别的程序覆盖都发生过）" },
            ],
            verify: "最后一次 runtime.driver.inspect 里，各工具的 provider 都是 xiapan",
        },
        Recipe {
            id: "recipe.token.turn_on_squeezer",
            title: "客户嫌费钱：先看花在哪，再决定要不要开 Token 压缩机",
            when: "客户说「太烧钱了」「token 用得好快」「能不能省点」",
            preconditions: &[],
            steps: &[
                Step { action: "runtime.usage_local.inspect", note: "先拿事实。没看花在哪就开压缩机，是拿一个不知道有没有用的开关去回答一个没量过的问题" },
                Step { action: "runtime.rtk.inspect", note: "看现在装没装、开没开。注意 ready 才算数 —— 装了且开了但 rtk 不在 PATH 上，一分钱都省不到" },
                Step { action: "runtime.rtk.demo", note: "当场跑真 rtk 出压缩前后对比，让客户自己看砍了什么、留了什么。这一步是卖信任，不是卖功能" },
                Step { action: "runtime.rtk.set_enabled", note: "写动作，要 --yes。客户看过 demo 再开，别替他决定" },
            ],
            verify: "runtime.rtk.inspect 的 ready:true（不是 installed:true —— 装了 ≠ 在省）",
        },
        // ★ 办公主线：客户拿来一份**已有的**文件要改几个字。
        // 这条配方存在的意义是钉死「改完必须读回来」—— 不读回来就没人知道到底改没改上。
        Recipe {
            id: "recipe.doc.edit_existing",
            title: "在客户已有的 Word / PPT / Excel 上改字，不动格式",
            when: "客户发来一份文件说「把甲方改成 XX」「年份换一下」「表里所有的 A 换成 B」",
            preconditions: &[
                "第一步 doc.inspect 的 ready 必须是 true —— 技能脚本没同步下来、或者没有便携 Node，后面每一步都会失败",
            ],
            steps: &[
                Step { action: "doc.inspect", note: "先确认这台机器上这条路是通的。跳过它的话，失败会以「改文档起不来」的形式出现在客户面前，而真因是技能包没同步" },
                Step { action: "doc.read", note: "★ 改之前先读。要替换的原文必须**一字不差**（含全角冒号、空格），凭印象写替换对，最常见的结果是 missed 非空、文件原样没动" },
                Step { action: "doc.edit", note: "写动作，要 --yes。给 --out 写到新文件，别就地改 —— 客户的原件是他唯一的底稿" },
                Step { action: "doc.read", note: "★ 改完必须读回来。edit 返回 ok:true 只说明脚本跑完了，不说明改对了；这一步才是判据" },
            ],
            verify: "doc.edit 的 replaced 里每条 count>0 且 missed 为空；再 doc.read 一次，正文里出现新文本、不出现旧文本",
        },
        Recipe {
            id: "recipe.ai.selfdiscover",
            title: "让这台机器上的 AI 知道 U-King 的存在和用法",
            when: "你（AI）是被人叫来干活的，想知道这台机器上还有什么能力可用；或者客户问「怎么让我的 AI 会用你们」",
            preconditions: &[],
            steps: &[
                Step { action: "runtime.identity.inspect", note: "看说明书发布了没、各家 AI 的记忆文件里有没有指过来。ready:false 通常不是没生成，是没人指过去" },
                Step { action: "runtime.identity.link", note: "写动作，要 --yes。把一行指针挂进各家 AI 的全局记忆文件 —— 只生成说明书不挂指针，等于把它锁在抽屉里" },
            ],
            verify: "runtime.identity.inspect 的 ready:true 且 linked_count > 0",
        },
    ]
}

pub(crate) fn action_table() -> Vec<actions::Action> {
    let mut t = vec![
        actions::readonly(
            actions::COMMAND_GUARD_INSPECT,
            "Inspect CLI command priority",
            "Read the resolved and preferred launchers for Claude, Codex, OpenClaw and Hermes without changing the machine.",
            10_000,
            &["platform", "shims_dir", "conflicts", "commands"],
            |_, _, _| action_json(installer::inspect_cli_command_guard()),
        ),
        actions::readonly(
            actions::NETWORK_INSPECT,
            "Inspect runtime network and WSL proxy handoff",
            "Read Windows proxy, process proxy variables and WSL bridge settings without contacting a network or changing the machine.",
            5_000,
            &["platform", "environment_proxies", "wsl", "warnings"],
            |_, _, _| action_json(installer::inspect_runtime_network()),
        ),
        actions::readonly(
            actions::AI_PROCESS_INSPECT,
            "Inspect AI process health and termination evidence",
            "Read local crash dumps, Windows Error Reporting entries and running security software to tell a crashed AI CLI session apart from one killed by another program. Reads only.",
            15_000,
            &["platform", "window_hours", "crash_evidence", "unattributed_dumps", "security_products_running", "verdict", "hint"],
            |_, _, _| action_json(installer::inspect_ai_process_health()),
        ),
        // U-King 自己的崩溃取证。**加它的直接起因**：2026-07-30 客户 pc-*** 报「运行老是崩溃」，
        // 远程查完 Windows 事件日志 / 转储 / 杀软隔离区**全是空的** —— 既证明不了崩过，
        // 也证明不了没崩过。根因是崩溃只上报网络、不落盘，且完全没有「上次是怎么结束的」记录。
        // 现在一条 `action run runtime.crash.inspect --json` 就出结论。
        actions::readonly(
            actions::CRASH_INSPECT,
            "Inspect U-King's own crash and unclean-exit history",
            "Read U-King's local crash log, panic and UI-crash records, and whether previous runs exited cleanly. Reads only; does not depend on Windows Event Log.",
            5_000,
            // `current_session` 故意**不进必填**：无头调用时它天然是 null（没有 GUI 会话），
            // 而 conformance 对必填字段是「不许为 null」—— 声明成必填等于让它每次跑都红。
            &["version", "ready", "blockers", "crashes", "unclean_exits", "events"],
            |_, _, _| Ok(crashlog::inspect()),
        ),
        // 「这个 U-King 是主实例还是并行调试实例」。见 `instance.rs`。
        //
        // **加它的直接起因**：开发时并行跑两个 U-King（验新版时不想关掉正开着的一堆终端）是刚需，
        // 而调试实例的定时任务 / 技能包同步 / Codex 代理自愈是被**刻意静默关掉**的 ——
        // 从界面上跟「这些东西坏了」完全无法区分。这条动作把「你现在是第几个」变成一句话可查，
        // 且 GUI / CLI / MCP / 远端影子问的是同一条实现。
        actions::readonly(
            actions::INSTANCE_INSPECT,
            "Inspect whether this U-King process is the primary instance or a parallel debug sidecar",
            "Report whether this process owns the background singletons (scheduler, skill-pack sync, Codex proxy self-heal) or runs as a parallel debug sidecar alongside another U-King. Reads only.",
            5_000,
            &["role", "ready", "blockers", "pid", "disabled_in_sidecar"],
            |_, _, _| Ok(instance::inspect()),
        ),
        // ★ 对话这一轮跑得怎么样。**加它的直接起因**：2026-08-03 客户 pc-*** 报「u-chat 卡住」，
        // 我连上机器要回答的是「现在卡住了吗、卡在哪一步、卡了多久」—— 55 个动作里一个都答不了，
        // 只能手敲 PowerShell 采样 CPU 增量、拉 transcript 算时间戳间隔，来回一个多小时。
        // `ai_process.inspect` 查的是**客户装的 AI CLI 崩没崩**（事后取证），跟这条不是一回事。
        // 现在 GUI / CLI / MCP 里的 AI 问的是同一条实现：`action run runtime.chat.inspect --json`。
        actions::readonly(
            actions::CHAT_INSPECT,
            "Inspect the current and recent chat turns",
            "Read whether a chat turn is running right now, how long it has been silent and which phase it is stuck in, plus how the last few turns ended. Reads only.",
            5_000,
            &["ready", "blockers", "stall_secs", "running", "stalled_now", "recent"],
            |_, _, _| Ok(agent::chat_inspect()),
        ),
        // ── 办公文档动作核心（doc.*）──
        // 把 uking-office-* 那批技能脚本升格成动作：**不重写，脚本就是实现**（理由见 doc.rs 顶部）。
        // 加它的直接起因：动作表原来 56 个动作全是设备级的，客户真正要 AI 干的文档级活
        // （改合同、读招标文件、出 PDF）只存在于技能包里 —— 也就是说只有装了技能包的 AI 会用，
        // 走 CLI / MCP / 远端影子进来的一律不知道有这回事。
        actions::readonly(
            actions::DOC_INSPECT,
            "Inspect whether office document actions can run here",
            "Check that the office skill scripts are synced on disk and that Node / portable Python are available. Reads only.",
            5_000,
            &["ready", "blockers", "scripts"],
            |_, _, _| Ok(doc::inspect()),
        ),
        actions::readonly_req(
            actions::DOC_READ,
            "Read an existing document as Markdown",
            "Convert an existing .docx/.xlsx/.pptx/.pdf/.csv into Markdown (tables preserved). With keywords, only the relevant passages are returned. Reads the file, changes nothing.",
            120_000,
            serde_json::json!({
                "file": { "type": "string", "description": "Absolute path of the document to read." },
                "keywords": { "type": "string", "description": "Only return passages matching these keywords (space separated). Highly recommended for long documents." },
                "max_chars": { "type": "integer", "minimum": 1, "description": "Cap the returned text length." }
            }),
            // `file` 声明成**必填**：核心会真的拦下少传的调用，conformance 也会如实跳过它
            //（通用体检不替它编造一个文件路径）。第一版写成全可选、在 handler 里手工判，
            // 跑道当场红一条 —— 那条红不是 bug，是声明没说实话。
            &["file"],
            &["ok"],
            |_, input, _| {
                let file = input.get("file").and_then(|v| v.as_str()).unwrap_or("").trim();
                if file.is_empty() {
                    return Err("invalid_input: file 不能是空字符串".into());
                }
                doc::read(
                    file,
                    input.get("keywords").and_then(|v| v.as_str()),
                    input.get("max_chars").and_then(|v| v.as_u64()),
                )
            },
        ),
        // 写动作：改的是客户自己的文件，门禁必开。
        // **idempotent=true 是实话**：同一个源文件 + 同一组替换 → 同一个产物，重放安全。
        // （前提是给了 --out；不给就地改的话第二次跑找不到旧文本，脚本会如实报「没命中」。）
        actions::write(
            actions::DOC_EDIT,
            "Edit text in an existing Word / PowerPoint / Excel file without touching its formatting",
            "Replace text inside an existing .docx/.pptx/.xlsx. Untouched parts keep their original compressed bytes, so formatting, headers, fonts and numbering survive byte-for-byte.",
            180_000,
            "required",
            serde_json::json!({
                "file": { "type": "string", "description": "Absolute path of the document to edit." },
                "replacements": {
                    "type": "array", "items": { "type": "string" },
                    "description": "List of `old=>new` pairs. Literal text only, no regex."
                },
                "out": { "type": "string", "description": "Write to this path instead of editing in place." },
                "all_parts": { "type": "boolean", "description": "Also search headers/footers/notes, not just the main body." }
            }),
            &["file", "replacements"],
            &["ok"],
            |_, input, _| {
                let file = input.get("file").and_then(|v| v.as_str()).unwrap_or("").trim();
                let reps: Vec<String> = input
                    .get("replacements")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                doc::edit(
                    file,
                    &reps,
                    input.get("out").and_then(|v| v.as_str()),
                    input.get("all_parts").and_then(|v| v.as_bool()).unwrap_or(false),
                )
            },
            None,
        ),
        // ── 办公文档「出」动作（doc.create.*）：把从零生成的技能脚本升格成动作，
        // 与 read/edit 同源（脚本就是实现，doc.rs 里不重写）。只登记**真幂等**的写：
        // 同一份 spec/markdown/csv + 同一个 out → 同一份文件（gen 脚本都不注入实时时间）。
        // **doc.create.mail 故意不上**：gen-eml 注入实时 Date 头，同入参重放内容不同，
        // 不满足「重放安全靠幂等」——要么给脚本加 date 覆盖，要么先实现幂等键账本。
        actions::write(
            actions::DOC_CREATE_WORD,
            "Create a new Word document",
            "Generate a new .docx from a structured spec or Markdown text, writing to the given out path. The office skill script is the implementation — untouched parts keep their original bytes.",
            120_000,
            "required",
            serde_json::json!({
                "spec": { "type": "object", "description": "doc.json：{title, blocks:[{type:heading|paragraph|bullets|table|image,...}]}。spec 和 markdown 二选一。" },
                "markdown": { "type": "string", "description": "或给 Markdown 正文（写进临时 .md 再生成）。markdown 和 spec 二选一。" },
                "out": { "type": "string", "description": "输出 .docx 的绝对路径。" }
            }),
            &["out"],
            &["ok", "file"],
            |_, input, _| {
                let out = input.get("out").and_then(|v| v.as_str()).unwrap_or("").to_string();
                doc::create_word(
                    input.get("spec"),
                    input.get("markdown").and_then(|v| v.as_str()),
                    &out,
                )
            },
            None,
        ),
        actions::write(
            actions::DOC_CREATE_SHEET,
            "Create a new Excel workbook",
            "Generate a new .xlsx from a structured spec (sheets with rows/charts) or CSV text, writing to the given out path. The office skill script is the implementation.",
            120_000,
            "required",
            serde_json::json!({
                "spec": { "type": "object", "description": "book.json：{sheets:[{name, rows, chart?}]}。spec 和 csv 二选一。" },
                "csv": { "type": "string", "description": "或给 CSV 文本（首行表头，数字保持数值）。csv 和 spec 二选一。" },
                "out": { "type": "string", "description": "输出 .xlsx 的绝对路径。" }
            }),
            &["out"],
            &["ok", "file"],
            |_, input, _| {
                let out = input.get("out").and_then(|v| v.as_str()).unwrap_or("").to_string();
                doc::create_sheet(
                    input.get("spec"),
                    input.get("csv").and_then(|v| v.as_str()),
                    &out,
                )
            },
            None,
        ),
        actions::write(
            actions::DOC_CREATE_SLIDE,
            "Create a new PowerPoint deck",
            "Generate a new .pptx from a deck spec (title/accent/slides with cover/section/content/quote/end layouts), writing to the given out path. Also produces a same-source .预览.html preview.",
            180_000,
            "required",
            serde_json::json!({
                "spec": { "type": "object", "description": "deck.json：{title, accent, slides:[{type:cover|section|content|quote|end,...}]}" },
                "out": { "type": "string", "description": "输出 .pptx 的绝对路径。" }
            }),
            &["spec", "out"],
            &["ok", "file"],
            |_, input, _| {
                let out = input.get("out").and_then(|v| v.as_str()).unwrap_or("").to_string();
                doc::create_slide(
                    input.get("spec").ok_or_else(|| "invalid_input: 缺少 spec".to_string())?,
                    &out,
                )
            },
            None,
        ),
        actions::write(
            actions::DOC_CREATE_CAD,
            "Create a new CAD drawing",
            "Generate a new .dxf from a spec of layers and entities (rect/line/polyline/circle/arc/text/dim), writing to the given out path. Also produces a preview SVG.",
            120_000,
            "required",
            serde_json::json!({
                "spec": { "type": "object", "description": "图纸 spec：{title, layers[], entities:[{type:rect|line|polyline|circle|arc|text|dim,...}]}" },
                "out": { "type": "string", "description": "输出 .dxf 的绝对路径。" }
            }),
            &["spec", "out"],
            &["ok", "file"],
            |_, input, _| {
                let out = input.get("out").and_then(|v| v.as_str()).unwrap_or("").to_string();
                doc::create_cad(
                    input.get("spec").ok_or_else(|| "invalid_input: 缺少 spec".to_string())?,
                    &out,
                )
            },
            None,
        ),
        // ⚠️ `doc.export.pdf` **故意先不上**：办公文档转 PDF 这台机器上已经有两条实现了 ——
        // `officedoc.rs`（内置预览用，走 LibreOffice + 缓存）和 `uking-pdf/to-pdf.mjs`
        //（客户交付用，md/html 走 Edge、Office 走 LibreOffice）。给其中一条挂上稳定 Action ID，
        // 等于把「哪条才是真的」这个还没定的问题**冻结成对外契约**，以后 AI 一直调的就是它。
        // 同一事实存在几份就会漂移几份（宪法第 8 条）。先定真相源，再上动作。
        // 装机体检。跑 5 个 `<cmd> --version` 子进程，冷机上 npm 一个就可能 2 秒，故给 30s。
        actions::readonly(
            actions::STACK_INSPECT,
            "Inspect installed AI toolchain",
            "Probe node / npm / claude / codex / git plus desktop apps and the portable runtime. Reads only.",
            30_000,
            &["node", "npm", "claude", "codex", "git", "claude_desktop", "codex_app", "portable_node"],
            |_, _, _| action_json(installer::detect_stack()),
        ),
        actions::readonly(
            actions::HARDWARE_INSPECT,
            "Inspect hardware capability for local models",
            "Read RAM, GPU and CPU to recommend a local model tier. Reads only.",
            15_000,
            &["ram_total_mb", "gpu_vendor", "gpu_accelerated", "cpu_cores", "os", "recommend"],
            |_, _, _| action_json(hardware::detect_hardware()),
        ),
        actions::readonly(
            actions::CODEX_INSPECT,
            "Inspect Codex desktop app and driver takeover",
            "Read whether the Codex desktop app is installed and whether U-King manages its driver config. Reads only.",
            15_000,
            &["app_installed", "driver_managed"],
            |_, _, _| action_json(codex::codex_status()),
        ),
        actions::readonly(
            actions::DRIVER_INSPECT,
            "Inspect which AI driver each tool is using",
            "Read the active provider of Claude, Codex, ClawX and Hermes from their real config files. Reads only.",
            10_000,
            &["active", "clawx_installed", "hermes_installed", "claude_own_key", "state_version"],
            |_, _, _| {
                let mut v = action_json(providers::driver_status())?;
                // 把状态版本一并交出去：调用方读到它、决定要改什么、再带着它调
                // runtime.driver.apply。中间被别的终端/uu-switch 改过 → 版本对不上 → 拒绝覆盖。
                if let Some(o) = v.as_object_mut() {
                    o.insert("state_version".into(), serde_json::Value::String(driver_state_version()));
                }
                Ok(v)
            },
        ),
        // 安全卸载的只读扫描。**列表型动作一律包成对象** `{items,count}`：
        // 裸数组一旦发出去就再也加不了字段，而 count 还能白捡一个可断言的量。
        actions::readonly(
            actions::FOOTPRINT_INSPECT,
            "Inspect what U-King left on this machine",
            "Scan U-King's own footprint, changed configs and AI tools it installed. Reads only, deletes nothing.",
            30_000,
            &["items", "count"],
            |_, _, _| {
                let items = cleanup::scan();
                Ok(serde_json::json!({ "count": items.len(), "items": action_json(items)? }))
            },
        ),
        actions::readonly(
            actions::TOOLBOX_INSPECT,
            "Inspect capability tools",
            "Read which capability tools (ffmpeg / Chrome / PowerShell 7 / Python …) are installed. Reads only.",
            30_000,
            &["items", "count"],
            |_, _, _| {
                let items = toolbox::list_tools();
                Ok(serde_json::json!({ "count": items.len(), "items": action_json(items)? }))
            },
        ),
        actions::readonly(
            actions::CREATOR_REEL_PRESETS_INSPECT,
            "Inspect one-click reel presets",
            "List the built-in schema-v1 visual-style presets for one-click reels. Reads only; selecting a preset never enables BGM or changes a user's supplied audio settings.",
            5_000,
            &["schema_version", "count", "presets"],
            |_, _, _| {
                let presets = reel::list_presets();
                Ok(serde_json::json!({ "schema_version": 1, "count": presets.len(), "presets": action_json(presets)? }))
            },
        ),
        // 按次收费的出片只有这一条提交入口。`action_parity_call` 会把其 execution_id
        // 送进 run_video_generation，落为服务端 idempotency_key；重试不会重复扣费。
        actions::with_progress(actions::write(
            actions::CREATOR_VIDEO_SUBMIT,
            "Generate a video clip",
            "Submit a text-to-video or image-to-video job, poll the original task until it finishes, and download its MP4 into U-King history. The ActionParity execution_id is used as the upstream idempotency key, so retrying the same request never creates a second paid task.",
            1_250_000,
            "required",
            serde_json::json!({
                "prompt": { "type": "string", "description": "Describe the desired video. Required even for image-to-video." },
                "model": { "type": "string", "description": "Optional video model id; defaults to the current Seedance Mini offering." },
                "image": { "type": "string", "description": "Optional first-frame image as a data URL, HTTPS URL, or base64." }
            }),
            &["prompt"],
            &["id", "task_id", "status", "have_video"],
            |_, input, progress| {
                let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or_default();
                let model = input.get("model").and_then(|v| v.as_str());
                let image = input.get("image").and_then(|v| v.as_str());
                let execution_id = actions::current_execution_id();
                let id = run_video_generation(prompt, model, image, execution_id.as_deref(), &|id, phase, detail| {
                    progress(&format!("id={id} {phase} {detail}"));
                })?;
                let item = video::list_history()
                    .into_iter()
                    .find(|item| item.id == id)
                    .ok_or_else(|| "video task completed without a history record".to_string())?;
                action_json(item)
            },
            None,
        )),
        actions::readonly(
            actions::RTK_INSPECT,
            "Inspect the token squeezer (RTK)",
            "Read whether RTK is installed, enabled as a Claude Code hook, and how many tokens it saved. Reads only.",
            10_000,
            // required 里钉上 ready：**动作必须回答「能不能用」，不是「装没装」**。
            // 这次 bug 的通用教训 —— installed/enabled 全是 true、形状全对，
            // 但世界是坏的，跑道一个字都没报。
            &["installed", "enabled", "ready", "blockers"],
            |_, _, _| action_json(rtk::status()),
        ),
        // 现场演示：当场跑真的 rtk，把压缩前后原文摆出来。**它是「原理透明」的实现**——
        // 客户/AI 都能自己核对我们砍了什么、留了什么，而不是只能信一个百分比。
        // 没装 rtk 时返回 ready:false（不是报错）：跑道会把它收进 not_ready 段，
        // 「客户没装」是事实不是 bug（readiness 约定）。
        actions::readonly(
            actions::RTK_DEMO,
            "Demonstrate what the token squeezer actually cuts",
            "Run the real rtk over two built-in sample logs (a build log and a test run) and return the before/after text side by side. Proves what is cut and what is kept. Reads only; the samples are embedded, no user files are touched.",
            30_000,
            &["ready", "blockers", "cases"],
            |_, _, _| match rtk::demo() {
                Ok(cases) => Ok(serde_json::json!({
                    "ready": true,
                    "blockers": [],
                    "cases": action_json(cases)?,
                })),
                Err(e) => Ok(serde_json::json!({
                    "ready": false,
                    "blockers": [e],
                    "cases": [],
                })),
            },
        ),
        actions::readonly(
            actions::HERMES_BROWSER_INSPECT,
            "Inspect Hermes browser takeover",
            "Tell apart 'Hermes can chat' from 'Hermes can open pages and screenshot'. Reads only.",
            30_000,
            &["hermes_installed", "browser_ready", "config_dir", "message", "suggestions"],
            |_, _, _| action_json(installer::hermes_browser_status()),
        ),
        // 布尔型动作也包成对象，理由同上：以后要补「哪个 PID / 哪条路径」不用改契约。
        actions::readonly(
            actions::CLAWX_INSPECT,
            "Inspect whether ClawX is running",
            "Read whether the ClawX desktop app currently holds its config in memory. Reads only.",
            5_000,
            &["running"],
            |_, _, _| Ok(serde_json::json!({ "running": clawx::is_running() })),
        ),
        actions::readonly(
            actions::OPENCLAW2_INSPECT,
            "Inspect the isolated OpenClaw 2 runtime",
            "Read only U-King's private OpenClaw 2 runtime and state. It never probes ClawX or legacy OpenClaw paths.",
            5_000,
            &["schema_version", "ready", "blockers", "installed", "prepared", "running", "state_version", "profile", "paths", "runtime", "gateway"],
            openclaw2::action_inspect,
        ),
        actions::readonly(
            actions::OPENCLAW2_PREFLIGHT,
            "Preflight the isolated OpenClaw 2 runtime",
            "Run only the private OpenClaw 2 doctor's lint JSON check, plus private gateway RPC status when it is running. It never repairs or migrates anything.",
            60_000,
            &["ok", "ready", "blockers", "warnings", "runtime", "config", "doctor", "gateway"],
            openclaw2::action_preflight,
        ),
        actions::with_progress(actions::write(
            actions::OPENCLAW2_INSTALL,
            "Install the isolated OpenClaw 2 runtime",
            "Download and verify U-King's pinned private Node and OpenClaw 2 runtime. It never changes PATH, global npm, shims, ClawX, or legacy OpenClaw.",
            900_000,
            "required",
            serde_json::json!({}),
            &[],
            &["changed", "installed", "node_version", "openclaw_version", "integrity_ok", "state_version"],
            openclaw2::action_install,
            Some(openclaw2::state_version),
        )),
        actions::write(
            actions::OPENCLAW2_PREPARE,
            "Prepare the isolated OpenClaw 2 profile",
            "Atomically create U-King's private OpenClaw 2 profile, state, workspace, and token. Existing incompatible configuration is refused rather than overwritten.",
            30_000,
            "required",
            serde_json::json!({
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535, "description": "Optional private gateway port. Omit to choose a stable free default." }
            }),
            &[],
            &["changed", "prepared", "profile", "port", "state_version"],
            openclaw2::action_prepare,
            Some(openclaw2::state_version),
        ),
        actions::write(
            actions::OPENCLAW2_LAUNCH,
            "Launch the isolated OpenClaw 2 gateway",
            "Launch only U-King's private OpenClaw 2 profile under external supervision. It refuses an externally owned port and never exposes the gateway token.",
            60_000,
            "required",
            serde_json::json!({}),
            &[],
            &["changed", "running", "ready", "pid", "port", "dashboard_url", "health", "state_version"],
            openclaw2::action_launch,
            Some(openclaw2::state_version),
        ),
        actions::write(
            actions::OPENCLAW2_CONFIGURE_MODEL,
            "Configure an isolated OpenClaw 2 model",
            "Validate and probe one OpenAI-compatible model in a private OpenClaw 2 transaction. API keys are stored only in a private file secret and never returned.",
            180_000,
            "required",
            serde_json::json!({
                "provider_id": { "type": "string", "minLength": 1 },
                "model": { "type": "string" },
                "api_key": { "type": "string", "writeOnly": true }
            }),
            &["provider_id"],
            &["changed", "configured", "ready", "provider", "model", "validation", "probe", "restart_required", "state_version"],
            |_, input, _| {
                let provider_id = input.get("provider_id").and_then(serde_json::Value::as_str)
                    .ok_or("invalid_input: provider_id 必填")?;
                let api_key = input.get("api_key").and_then(serde_json::Value::as_str);
                let device_key = if api_key.is_some_and(|key| !key.trim().is_empty()) {
                    None
                } else {
                    device::device_key_offline().ok()
                };
                let route = providers::resolve_openai_route_for_openclaw2(
                    provider_id,
                    input.get("model").and_then(serde_json::Value::as_str),
                    api_key,
                    device_key.as_deref(),
                )?;
                openclaw2::configure_model(route)
            },
            Some(openclaw2::state_version),
        ),
        actions::readonly(
            actions::USB_GENIE_INSPECT,
            "Inspect removable USB AI Genie targets",
            "List removable drives and only stat the U-King/AI-Genie paths; it never recursively scans a drive.",
            5_000,
            &["schema_version", "ready", "blockers", "targets", "state_version"],
            usb_genie::action_inspect,
        ),
        actions::with_progress(actions::write(
            actions::USB_GENIE_DEPLOY,
            "Build or refresh a USB AI Genie",
            "Install U-King's pinned PicoClaw runtime into the selected target. It only writes the U-King/AI-Genie subtree and never formats or scans unrelated files. API keys are referenced, never accepted as input or returned.",
            600_000,
            "required",
            serde_json::json!({
                "target_id": { "type": "string", "minLength": 1, "description": "Stable removable-volume identity returned by inspect; a drive letter alone is not trusted." },
                "target_root": { "type": "string", "minLength": 1 },
                "credential_ref": { "type": "string", "enum": ["none", "official_device"], "description": "none creates a credential-free first install and preserves an existing credential during updates; official_device writes the existing U-King device wallet." },
                "zip_path": { "type": "string", "minLength": 1, "description": "P1 local PicoClaw archive source, used only by the smoke/build path." }
            }),
            &["target_id", "target_root", "credential_ref", "zip_path"],
            &["changed", "target_root", "picoclaw_version", "sha256_ok", "credential_mode", "state_version"],
            |action_id, input, progress| {
                // Composition root resolves the optional device-wallet reference;
                // usb_genie itself remains a portable-runtime adapter with no
                // dependency on the wallet implementation.
                let key = if input.get("credential_ref").and_then(serde_json::Value::as_str) == Some("official_device") {
                    Some(device::get_device_key()?.key)
                } else {
                    None
                };
                usb_genie::action_deploy_with_device_key(action_id, input, progress, key)
            },
            Some(usb_genie::action_state_version),
        )),
        actions::readonly_req(
            actions::USB_GENIE_VERIFY,
            "Verify a USB AI Genie target",
            "Verify only the pinned runtime, launcher and generated configuration. No model request is made.",
            30_000,
            serde_json::json!({ "target_id": { "type": "string", "minLength": 1 }, "target_root": { "type": "string", "minLength": 1 } }),
            &["target_id", "target_root"],
            &["ok", "checks", "blockers", "state_version"],
            usb_genie::action_verify,
        ),
        actions::write(
            actions::USB_GENIE_LAUNCH,
            "Open USB AI Genie",
            "Open only this target's ASCII launcher in a new interactive console.",
            60_000,
            "required",
            serde_json::json!({ "target_id": { "type": "string", "minLength": 1 }, "target_root": { "type": "string", "minLength": 1 } }),
            &["target_id", "target_root"],
            &["changed", "launched", "state_version"],
            usb_genie::action_launch,
            Some(usb_genie::action_state_version),
        ),
        actions::destructive(
            actions::USB_GENIE_CREDENTIAL_REMOVE,
            "Remove USB AI Genie credentials",
            "Remove only this target's PicoClaw credential file. This cannot revoke a copied or lost key; rotate it with the provider.",
            10_000,
            serde_json::json!({ "target_id": { "type": "string", "minLength": 1 }, "target_root": { "type": "string", "minLength": 1 } }),
            &["target_id", "target_root"],
            &["changed", "removed", "state_version"],
            usb_genie::action_credential_remove,
        ),
        actions::readonly(
            actions::LOCALLLM_INSPECT,
            "Inspect the four local LLM engines",
            "Report Ollama, llama.cpp, vLLM and SGLang together: installed, whether each is actually usable right now (`ready` + `blockers`), the OpenAI-compatible endpoint when one is serving, which process U-King started, and the local models each engine can load. vLLM and SGLang are reported as `unsupported_here` on Windows/macOS because upstream only ships Linux+CUDA — saying so is the answer, not hiding them. Reads only.",
            15_000,
            &["engines"],
            |_, _, _| {
                Ok(serde_json::json!({
                    "engines": localllm::inspect_all(),
                    "model_dirs": localllm::model_dirs()
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect::<Vec<_>>(),
                    "download_dir": localllm::download_dir().to_string_lossy().to_string(),
                    "local_models": localllm::local_model_files(),
                    "settings": localllm::ENGINE_IDS
                        .iter()
                        .map(|e| (e.to_string(), localllm::settings(e)))
                        .collect::<std::collections::HashMap<_, _>>(),
                }))
            },
        ),
        actions::readonly_opt(
            actions::LOCALLLM_CATALOG,
            "List downloadable local models (the store shelf)",
            "The curated shelf of open-weight models U-King can download, plus which quantisations of each are already on this disk. The shelf itself is hot-delivered (server first, embedded copy as fallback) so it can be refreshed without shipping an exe. Sizes carried here are approximate by design — the exact quantisation list and byte sizes are asked of the model host at download time, because those change upstream and a stale number would blow up the customer's disk. Reads only.",
            30_000,
            serde_json::json!({
                "model_id": { "type": "string", "description": "Ask for one model's real quantisation list (live from the model host: name, total bytes, whether it is already local). Omit to just list the shelf." },
                "refresh": { "type": "boolean", "description": "Bypass the 12h shelf cache and re-fetch it." }
            }),
            &["models"],
            |_, input, _| {
                let refresh = input.get("refresh").and_then(|v| v.as_bool()).unwrap_or(false);
                let models = localllm::catalog(refresh);
                let quants = match input.get("model_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.trim().is_empty() => Some(localllm::catalog_files(id)?),
                    _ => None,
                };
                Ok(serde_json::json!({
                    "models": models,
                    "quants": quants,
                    "download_dir": localllm::download_dir().to_string_lossy().to_string(),
                }))
            },
        ),
        actions::write(
            actions::LOCALLLM_DOWNLOAD,
            "Download a model from the store shelf",
            "Fetch one quantisation of a shelf model into the download folder, resuming a half-finished file rather than starting over, and verifying the finished bytes against the size the host reported. Idempotent: an already-complete file is skipped, so re-running costs nothing. A model is tens of GB — this can run for hours on a home connection.",
            86_400_000,
            "required",
            serde_json::json!({
                "model_id": { "type": "string", "description": "Shelf model id, e.g. qwen3.5-9b." },
                "quant": { "type": "string", "description": "Which quantisation, e.g. Q4_K_M. Ask runtime.localllm.catalog with model_id for the real list — do not guess." },
                "dir": { "type": "string", "description": "Change the download folder first. Models are tens of GB and the system drive is usually not where they belong." }
            }),
            &["model_id", "quant"],
            &["ok", "path"],
            |_, input, _| {
                if let Some(d) = input.get("dir").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
                    localllm::set_download_dir(d)?;
                }
                let id = input["model_id"].as_str().unwrap_or_default().to_string();
                let quant = input["quant"].as_str().unwrap_or_default().to_string();
                let path = localllm::download_model(&id, &quant, &|_m: &str| {})?;
                Ok(serde_json::json!({ "ok": true, "path": path, "model_id": id, "quant": quant }))
            },
            None,
        ),
        actions::write(
            actions::LOCALLLM_START,
            "Start a local inference server",
            "Start one of the local engines and return its OpenAI-compatible endpoint. Idempotent: if U-King already started that engine, the existing endpoint is returned and no second process is spawned. Refuses engines this machine cannot run instead of failing halfway. Run parameters given here are saved and reused next time, so the customer configures context length and port once rather than every launch.",
            120_000,
            "required",
            serde_json::json!({
                "engine": { "type": "string", "enum": ["ollama", "llamacpp", "vllm", "sglang"], "description": "Which engine to start." },
                "model": { "type": "string", "description": "Model to load: a .gguf path for llama.cpp, a HuggingFace folder for vLLM/SGLang. Ignored by Ollama, which loads per request." },
                "port": { "type": "integer", "minimum": 1024, "maximum": 65535, "description": "Port to serve on. Defaults to 18820; Ollama always uses 11434." },
                "ctx": { "type": "integer", "minimum": 0, "maximum": 1048576, "description": "Context length in tokens. 0 leaves it to the engine. Bigger contexts cost memory even before the first token." },
                "gpu_layers": { "type": "integer", "minimum": -1, "maximum": 999, "description": "Layers to offload to the GPU: -1 auto (the engine decides), 0 CPU only, 999 all of them. Forcing 999 on a card without the VRAM makes the server exit on load, which looks to the customer like the button did nothing — so auto is the default." },
                "threads": { "type": "integer", "minimum": 0, "maximum": 256, "description": "CPU threads. 0 lets the engine pick." }
            }),
            &["engine"],
            &["ok", "endpoint"],
            |_, input, _| {
                let engine = input["engine"].as_str().unwrap_or_default().to_string();
                let model = input.get("model").and_then(|v| v.as_str()).unwrap_or_default();
                // 一个参数都没给 = 沿用上次存的；给了任意一个 = 以这次为准并存下来。
                let num = |k: &str| input.get(k).and_then(|v| v.as_i64());
                let opts = if ["port", "ctx", "gpu_layers", "threads"].iter().any(|k| num(k).is_some()) {
                    let cur = localllm::settings(&engine);
                    Some(localllm::RunSettings {
                        port: num("port").map(|v| v as u16).unwrap_or(cur.port),
                        ctx: num("ctx").map(|v| v as u32).unwrap_or(cur.ctx),
                        gpu_layers: num("gpu_layers").map(|v| v as i32).unwrap_or(cur.gpu_layers),
                        threads: num("threads").map(|v| v as u32).unwrap_or(cur.threads),
                    })
                } else {
                    None
                };
                let endpoint = localllm::start(&engine, model, opts)?;
                // 起来了就**当场变成一个可选的驱动**（落进「AI 设置」的供应商列表）。
                // 组合根干这件事、localllm.rs 不碰 providers —— 模块之间不许互相 import。
                let provider = register_local_provider(&engine, &endpoint, model);
                Ok(serde_json::json!({
                    "ok": true, "endpoint": endpoint, "engine": engine, "provider": provider
                }))
            },
            None,
        ),
        actions::write(
            actions::LOCALLLM_STOP,
            "Stop the local inference server U-King started",
            "Stop the process U-King started for that engine. Idempotent: succeeds when nothing is running. Only ever kills the recorded PID, and only after re-checking that PID still carries the image name we launched — never by bare process name, which would take down a customer's own identically-named server.",
            30_000,
            "required",
            serde_json::json!({
                "engine": { "type": "string", "enum": ["ollama", "llamacpp", "vllm", "sglang"], "description": "Which engine to stop." }
            }),
            &["engine"],
            &["ok", "message"],
            |_, input, _| {
                let engine = input["engine"].as_str().unwrap_or_default().to_string();
                let msg = localllm::stop(&engine)?;
                // 停了就把那条驱动撤下去。留着的话「AI 设置」里会摆着一个连不上的选项 ——
                // 客户选中它，配好的工具每次调用都超时，而界面上一切正常。
                let _ = providers::delete_custom_provider(&local_provider_id(&engine));
                Ok(serde_json::json!({ "ok": true, "message": msg, "engine": engine }))
            },
            None,
        ),
        actions::write(
            actions::LOCALLLM_INSTALL,
            "Install a local LLM engine",
            "Install the engine itself (not a model). Ollama installs unattended from mirrored sources; the others report what to do instead of pretending. Idempotent: already installed returns success without touching anything.",
            2_400_000,
            "required",
            serde_json::json!({
                "engine": { "type": "string", "enum": ["ollama", "llamacpp", "vllm", "sglang"], "description": "Which engine to install." },
                "variant": { "type": "string", "enum": ["vulkan", "cuda", "cpu"], "description": "llama.cpp build flavour. Defaults to vulkan when this machine has a GPU, cpu otherwise. CUDA is faster on NVIDIA but needs a ~540MB download (build + CUDA runtime) versus ~35MB for Vulkan, so it is opt-in rather than automatic." }
            }),
            &["engine"],
            &["ok", "message"],
            |_, input, _| {
                let engine = input["engine"].as_str().unwrap_or_default().to_string();
                let variant = input.get("variant").and_then(|v| v.as_str());
                let msg = match engine.as_str() {
                    "ollama" => localllm::install_ollama(&|_m: &str| {})?,
                    // 显卡厂商由组合根问 hardware.rs 再传进去 —— localllm 不 import 别的功能模块。
                    "llamacpp" => localllm::install_llama_server(
                        variant,
                        &hardware::detect_hardware().gpu_vendor,
                        &|_m: &str| {},
                    )?,
                    "vllm" | "sglang" => return Err(
                        format!("{engine} 只能装在 Linux + N 卡（CUDA）上：`pip install {engine}`。Windows / macOS 上装不了 —— 个人电脑请用 Ollama 或 llama.cpp。"),
                    ),
                    other => return Err(format!("没有这个引擎：{other}")),
                };
                Ok(serde_json::json!({ "ok": true, "message": msg, "engine": engine }))
            },
            None,
        ),
        actions::write(
            actions::LOCALLLM_MODEL_ADD,
            "Register a local model folder or import a GGUF into Ollama",
            "`kind=dir` adds a folder to the scan list (models are tens of GB — the system drive is usually not where they live). `kind=gguf` imports one .gguf into Ollama via a generated Modelfile. Idempotent both ways: re-adding the same folder or re-importing the same model changes nothing.",
            600_000,
            "required",
            serde_json::json!({
                "kind": { "type": "string", "enum": ["dir", "gguf"], "description": "Add a model folder, or import a single GGUF into Ollama." },
                "path": { "type": "string", "description": "Folder path for kind=dir, .gguf file path for kind=gguf." },
                "name": { "type": "string", "description": "Model name for kind=gguf. Letters, digits and - _ . : only, because it becomes a command argument." }
            }),
            &["kind", "path"],
            &["ok", "message"],
            |_, input, _| {
                let kind = input["kind"].as_str().unwrap_or_default();
                let path = input["path"].as_str().unwrap_or_default();
                match kind {
                    "dir" => {
                        let dirs = localllm::add_model_dir(path)?;
                        Ok(serde_json::json!({ "ok": true, "message": format!("已添加模型目录（共 {} 个）", dirs.len()), "dirs": dirs }))
                    }
                    "gguf" => {
                        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                        let msg = localllm::import_gguf(path, name)?;
                        Ok(serde_json::json!({ "ok": true, "message": msg }))
                    }
                    other => Err(format!("kind 只能是 dir 或 gguf，给的是 {other}")),
                }
            },
            None,
        ),
        actions::readonly(
            actions::EXPERT_INSPECT,
            "Inspect hired experts (expert packs)",
            "List expert packs under ~/.uking/experts. Each folder is one hired expert; reports which ones passed validation, which were rejected and why, and which declare skill packs that are not synced yet. Reads only; never writes.",
            5_000,
            &["dir", "dir_exists", "packs", "rejected", "truncated", "ready", "blockers"],
            |_, _, _| Ok(serde_json::to_value(expert::inspect()).unwrap_or_default()),
        ),
        actions::readonly_req(
            actions::HIRE_SEARCH,
            "Search the open ecosystem for hireable actors",
            "Search npm (and other registries) for skills/plugins that can be hired into this machine as experts. Returns not just what exists but **how to hire each one** (CLI / skill pack / harness tool). Read-only: never installs, never writes. An empty result reports whether the network answered, so 'nothing found' is never confused with 'could not reach'.",
            20_000,
            serde_json::json!({
                "query": { "type": "string", "description": "What kind of actor you need — e.g. `keywords:dsh-plugin ppt`, `公众号`, `cad`. Registry search syntax is passed through." }
            }),
            // `query` 必填：没有搜索词的搜索没有意义，核心该拦下来而不是回一个空列表
            //（回空列表会被读成「生态里没有」—— 本模块最想防的就是这个误读）。
            &["query"],
            &["query", "hits", "sources", "truncated", "ready", "blockers"],
            |_, input, _| {
                let q = input.get("query").and_then(|v| v.as_str()).unwrap_or_default();
                Ok(serde_json::to_value(hire::search(q)).unwrap_or_default())
            },
        ),
        actions::readonly(
            actions::GEO_INSPECT,
            "Inspect the GEO skill pack",
            "Read whether the 1so-geo skill pack is installed. Reads only.",
            5_000,
            &["installed", "ready", "blockers"],
            |_, _, _| {
                let installed = geo::is_installed();
                // GEO 技能包是一组 node 脚本（bin/1so.mjs）。释放到磁盘 ≠ 跑得起来：
                // 客户机没有 node，点「开始体检」就是一声不吭地失败。
                let node = installer::tool_installed("node");
                let mut blockers = Vec::new();
                if !installed {
                    blockers.push("GEO 技能包还没释放到本机".to_string());
                }
                if !node {
                    blockers.push("找不到 node：技能包是 node 脚本，没有它跑不起来".to_string());
                }
                Ok(serde_json::json!({
                    "installed": installed,
                    "ready": installed && node,
                    "blockers": blockers,
                }))
            },
        ),
        actions::readonly(
            actions::UU_REMOTE_INSPECT,
            "Inspect screen-sharing assistance (UU Remote)",
            "Read whether NetEase UU Remote is installed so the author can see this screen. Reads only.",
            5_000,
            &["installed", "ready", "blockers", "portable_available", "can_auto_install", "download_page"],
            |_, _, _| {
                let installed = tools::uu_remote_is_installed();
                // readiness 回答的是「客户现在能不能接受屏幕协助」，**不是**「我们能不能替他装」。
                // 这两件事在 Mac 上会分叉：装不了一键装，但他自己装完照样能用。
                let can_auto_install = cfg!(windows);
                let mut blockers = Vec::new();
                if !installed {
                    blockers.push(if can_auto_install {
                        "还没装 UU远程（技术支持页点「帮我下载安装」）".to_string()
                    } else {
                        "还没装 UU远程，且当前平台不支持一键装 —— 请到官网下载页自行安装".to_string()
                    });
                }
                Ok(serde_json::json!({
                    "installed": installed,
                    "ready": installed,
                    "blockers": blockers,
                    // 官方只发安装包、没有绿色版（2026-07-29 核对官网下载页）。放进动作输出
                    // 是为了让这个事实只有一份：前端文案、远端影子、客服话术都读同一个字段。
                    "portable_available": false,
                    "can_auto_install": can_auto_install,
                    "download_page": tools::uu_remote_download_page(),
                }))
            },
        ),
        // 泊舟小程序是**独立应用**，自己带 updater —— 这里只报「装没装 / 有没有新版」，
        // 不做第二套升级判据。timeout 给到 20s：要联网取 latest.json，还得按源优先级挨个试。
        actions::readonly(
            actions::PODAPP_INSPECT,
            "Inspect PodApp (泊舟 AI 小程序)",
            "Read whether PodApp is installed, its version, and the latest published version. Reads only; fetches the update manifest over the network.",
            20_000,
            &["installed", "ready", "blockers", "version", "update_available", "can_auto_install"],
            |_, _, _| Ok(podapp::status()),
        ),
        // 自动化（定时任务）。readiness 回答的是「能不能**到点自动跑**」而不是「配了几条」：
        // 拿不到设备 Key = 到点也调不动大脑，那就是 blocker（宪法：报告是对的、世界是坏的，
        // 这种绿色最害人）。输出里的 `runs_only_while_app_open` 是产品边界当数据发 ——
        // GUI 文案、CLI、AI 通过 MCP 读到的是同一句话，不会各自跑偏。
        actions::readonly(
            actions::AUTOMATION_INSPECT,
            "Inspect scheduled automations",
            "List every scheduled automation with its next run time, plus whether automations can actually fire on this machine. Reads only. Note: the scheduler lives in this process — jobs only fire while U-King is running (tray counts).",
            5_000,
            &["ready", "blockers", "count", "enabled", "runs_only_while_app_open", "jobs"],
            // 组合根注入两件 automation 自己不认识的事：设备 Key 拿不拿得到、休眠抑制的现状
            // （含「挡不住合盖」这条边界）。automation 不 import device / awake，删任一模块只动这里。
            |_, _, _| Ok(automation::status(device::device_key_offline().is_ok(), awake::status())),
        ),
        // 优化引擎缺失 / 返回非法 JSON 都是**有效的体检结论**，不是动作失败 ——
        // 所以包成 {ok,report,error} 而不是抛 Err。否则一台没释放 ukrt.exe 的机器
        // 会让整条 conformance 变红，跑道就没人信了。
        actions::readonly(
            actions::OPTIMIZER_INSPECT,
            "Inspect the AI optimizer engine report",
            "Run the read-only optimizer doctor (ukrt on Windows, native on macOS) and return its report. Reads only.",
            60_000,
            &["ok"],
            |_, _, _| {
                Ok(match airuntime_doctor_routed() {
                    Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                        Ok(v) => serde_json::json!({ "ok": true, "report": v, "error": null }),
                        Err(e) => serde_json::json!({ "ok": false, "report": null, "error": format!("优化引擎返回非法 JSON: {e}") }),
                    },
                    Err(e) => serde_json::json!({ "ok": false, "report": null, "error": e }),
                })
            },
        ),
        // ★ 行为时间轴。**「夜班」的第一块地基** —— 在能让 AI 半夜替我们干活之前，
        // 先得能回答「它干了什么」。跟旁边两条的分工：`usage_*` 答「花了多少钱」，
        // `chat.inspect` 答「这一轮卡在哪」，这条答**顺序和归属**（谁干的、动了哪些文件）。
        //
        // 它是纯本地读，不发网络、不碰用户文件，所以能安全地当只读动作放出去 ——
        // 也就自动进了 `action conformance` 跑道。
        actions::readonly_opt(
            actions::JOURNAL_INSPECT,
            "Read the local activity timeline (who did what, when)",
            "Append-only local record of what happened on this machine: which Action Core actions ran (from GUI / CLI / MCP) and which tools the AI called, with outcome and timing. Paths and commands are redacted before they are written. Records U-King actions only — never keyboard, windows, clipboard or other processes. Local only, never uploaded.",
            10_000,
            serde_json::json!({
                "days": { "type": "integer", "minimum": 1, "maximum": 30, "description": "Look-back window in days (default 1 = today)." }
            }),
            &["ready", "blockers", "enabled", "summary", "recent", "records_only_uking_actions", "uploads"],
            |_, input, _| {
                let days = input.get("days").and_then(|v| v.as_i64()).unwrap_or(1);
                action_json(journal::status(days))
            },
        ),
        // ★ 「这台电脑上的 AI 都在忙什么」。跟下面那条 `usage_local` 读的是**同一批会话日志**，
        // 但回答的是两个问题：那个答「花了多少钱」，这个答「在干哪些活」——
        // 55 个动作里以前没有一个能答后者，任务看板也就只看得见我们自己工作台里的会话。
        // days 可选，喂 {} 也能跑（默认 7 天），所以照样进 conformance 体检。
        // ★ 任务本象（2Origin 长期存储层）。跟下面那条 `ai_tasks` 分工是整个架构的分界线：
        // 那个读各家 AI 的**会话记录**（影子 —— 答「谁跑过什么」，换个 harness 接不上），
        // 这个读**对象状态**（本象 —— 答「世界此刻是什么样、验过什么、下一步是什么」，接得上）。
        actions::readonly_opt(
            actions::ORIGIN_INSPECT,
            "Inspect a task's origin state (2Origin)",
            "Read the persisted object-state of a task: goal, what the world looks like now, verified facts, decisions with reasons, and next steps. This is state, not a chat transcript — a different session or a different harness can resume from it. Without task_id, lists all tasks that have state. Reads only.",
            10_000,
            serde_json::json!({
                "task_id": { "type": "string", "description": "Task id. Omit to list every task that has origin state." },
                "compiled": { "type": "boolean", "description": "Also return the context block that would be injected into a harness prompt (default false)." }
            }),
            &["tasks", "count"],
            |_, input, _| {
                let compiled = input.get("compiled").and_then(|v| v.as_bool()).unwrap_or(false);
                let list = match input.get("task_id").and_then(|v| v.as_str()) {
                    Some(id) if !id.trim().is_empty() => origin::load(id).into_iter().collect(),
                    _ => origin::list(),
                };
                let tasks: Vec<serde_json::Value> = list
                    .iter()
                    .map(|o| {
                        let mut v = serde_json::to_value(o).unwrap_or(serde_json::Value::Null);
                        if compiled {
                            if let Some(m) = v.as_object_mut() {
                                m.insert("compiled_context".into(), serde_json::json!(o.compile_context()));
                            }
                        }
                        v
                    })
                    .collect();
                Ok(serde_json::json!({ "count": tasks.len(), "tasks": tasks }))
            },
        ),
        // 🔴 **写动作，但 confirmation=never**，理由要说清楚：这是 AI 记录**它自己刚干完的事**，
        // 每记一条都弹一次确认，等于逼所有人关掉这个功能 —— 而一个没人开的状态层等于没有。
        // 安全边界靠别处兜：落点只在 `~/.uking/origin/`（碰不到客户任何文件）、
        // 入参过 schema、id 过路径穿越过滤、乐观并发拒绝覆盖。
        // 它是**幂等**的：同一份状态重放，结果一样。
        actions::write(
            actions::ORIGIN_SAVE,
            "Save a task's origin state (2Origin)",
            "Persist the object-state of a task so a later session or a different harness can resume without re-asking. Write what the world looks like now, not what was said. facts[].verified must be true only when a machine re-checked it. Pass expected_version (from origin.inspect) to refuse overwriting a state someone else changed. Writes only under ~/.uking/origin/ — never touches your files.",
            10_000,
            "never",
            serde_json::json!({
                "state": { "type": "object", "description": "The task.origin object (spec/kind/id/goal/version/updated_at/current_state/next_steps required)." },
                "expected_version": { "type": "integer", "description": "Version you based this edit on. Mismatch = someone else wrote first; the save is refused." }
            }),
            &["state"],
            &["state"],
            |_, input, _| {
                let mut o: origin::TaskOrigin = serde_json::from_value(input["state"].clone())
                    .map_err(|e| format!("状态格式不对: {e}"))?;
                // 协议头由核心钉死，不吃调用方传的 —— 传错一个字，这份状态就不是本象了。
                o.spec = origin::SPEC.into();
                o.kind = origin::KIND.into();
                let exp = input.get("expected_version").and_then(|v| v.as_u64());
                Ok(serde_json::json!({ "state": origin::save(o, exp)? }))
            },
            None,
        ),
        // —— 工作台模板 ——
        // 预览（inspect）和真装（install）**共用同一份计划**：界面上「看看会干什么」打印的，
        // 就是点确认之后真会干的事。另写一套模拟一定会跟真的漂开，而漂开的那次正好是出事那次。
        actions::readonly_opt(
            actions::WORKBENCH_INSPECT,
            "List workbench templates and preview installing one",
            "A workbench is a plain folder layout plus a WORKBENCH.md that tells any AI what each folder means — not an app. This lists the built-in templates, and if you pass a path, reports whether that folder can host one and exactly what installing would create. Read-only: previewing never creates the folder.",
            5_000,
            serde_json::json!({
                "template": { "type": "string", "description": "Built-in example id to preview (default: the first one). Ignored when `manifest` is given." },
                "manifest": { "type": "object", "description": "A workbench definition built for this specific customer. Preferred over `template` — preview validates it and tells you exactly what is wrong before anything is written." },
                "path": { "type": "string", "description": "Folder to preview installing into. Omit to just list the built-in examples." },
                "overwrite_doc": { "type": "boolean", "description": "Preview as if you asked to refresh WORKBENCH.md and the contract file." }
            }),
            // `ready` / `blockers` 进必答项：可用性约定不是写在文档里就算数的，
            // conformance 会按 required 断言形状，少一个当场变红。
            &["templates", "default_template", "ready", "blockers"],
            |_, input, _| {
                let p = input.get("path").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty())
                    .map(std::path::PathBuf::from);
                let ow = input.get("overwrite_doc").and_then(|v| v.as_bool()).unwrap_or(false);
                // 组合根注入「这台机器上真有哪些技能」—— workbench 不 import skillpack（模块铁律②）
                match workbench::resolve(
                    input.get("template").and_then(|v| v.as_str()),
                    input.get("manifest"),
                    &skillpack::pack_names().iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                ) {
                    Ok(wb) => Ok(workbench::inspect(&wb, p.as_deref(), ow, &skillpack::installed_in("uking-workbench"))),
                    // 🔴 校验没过**不是错误返回**，是「这份定义装不了 + 为什么」——
                    //    只读动作的价值就在于：AI 能先拿这段话把 manifest 改对，再去装。
                    //    直接 Err 会让调用方只看到一句「失败」，还得自己猜哪儿错了。
                    // 形状必须跟成功那条一致：`ready`/`blockers` 是这个动作的必答项，
                    // 少给一个字段，调用方就得写两套解析（而那两套一定会漂）。
                    Err(e) => {
                        let skill_in = skillpack::installed_in("uking-workbench");
                        let mut base = workbench::inspect(&serde_json::Value::Null, None, false, &skill_in);
                        base["target"] = serde_json::json!({
                            "path": p.map(|x| x.display().to_string()),
                            "installable": false,
                            "blockers": [e],
                            "plan": [],
                        });
                        Ok(base)
                    }
                }
            },
        ),
        // 「按客户实际使用情况搭」的事实来源。跟上面那个分工：上面答「这儿能不能装」，
        // 这个答「他手上到底有什么」。**只 stat 不读内容**，撞上限如实报 truncated。
        actions::readonly_req(
            actions::WORKBENCH_SCAN,
            "Take a read-only inventory of a folder the customer actually works in",
            "Count files by extension, biggest subfolders, how much changed in the last 30 days, and how deep it nests — the evidence for shaping a workbench around what this person actually does. Only stats files; never opens one, because it does not yet know which are private. Never reads this machine's AI usage records either: the only inputs are the folder the customer pointed at and what he says. Caps out at 20000 files and reports `truncated` when it does — not seeing something is not the same as it not being there.",
            20_000,
            serde_json::json!({
                "path": { "type": "string", "description": "Folder to inventory. Usually the messy folder he works in today, which is NOT where the workbench will go." }
            }),
            // `path` 声明成**必填**：核心会真的拦下少传的调用，conformance 也会如实跳过它
            //（通用体检不替它编造一个目录去扫）。同 `doc.read` 那条 —— 第一版我也写成了
            // 全可选 + handler 里手工判，跑道当场红一条：**那条红不是 bug，是声明没说实话**。
            &["path"],
            &["root", "files", "by_ext", "top_dirs", "changed_in_30d", "truncated"],
            |_, input, _| {
                let p = input["path"].as_str().unwrap_or_default().trim().to_string();
                workbench::scan(std::path::Path::new(&p))
            },
        ),
        actions::write(
            actions::WORKBENCH_INSTALL,
            "Install a workbench template into a folder",
            "Create the folder layout, per-folder READMEs, the compiled WORKBENCH.md, and the AGENTS.md / CLAUDE.md entry files that AI CLIs auto-load when they open the folder — that pair is what actually makes the conventions reach the model, so each carries the folder table inline rather than just pointing at WORKBENCH.md. Idempotent: re-running only fills in what's missing and never rewrites a file the user edited; if an entry file exists but never mentions WORKBENCH.md, the result says so in `warnings` instead of pretending the install worked. Refuses a drive root, the home directory, or any non-empty folder that isn't already a U-King workbench — there is deliberately no force flag.",
            20_000,
            "required",
            serde_json::json!({
                "path": { "type": "string", "description": "Folder that will become the workbench root. Must be empty, non-existent, or an existing U-King workbench." },
                "manifest": { "type": "object", "description": "The workbench definition built for this specific customer — folder conventions, the rule for each folder, naming, which skills it uses, and what it explicitly cannot do. Preferred: a workbench should be shaped around what this person actually does. Validated before anything is written." },
                "template": { "type": "string", "description": "Built-in example id, used only when no `manifest` is given (a starting point for someone who does not want to describe their work yet)." },
                "overwrite_doc": { "type": "boolean", "description": "Refresh WORKBENCH.md and the contract file. Per-folder READMEs are still never touched." }
            }),
            &["path"],
            &["ok", "workbench", "path", "created", "updated", "skipped", "steps"],
            |_, input, _| {
                let p = input["path"].as_str().unwrap_or_default().trim().to_string();
                if p.is_empty() {
                    // 不给默认落点是故意的 —— 猜错地方就是往客户硬盘乱写。
                    return Err("必须给 path（工作台根目录）。这里没有默认值：猜错地方就是往客户硬盘乱写。".into());
                }
                let ow = input.get("overwrite_doc").and_then(|v| v.as_bool()).unwrap_or(false);
                // AI 现搭的 manifest 优先；没给才回落内置样例。校验在核心，不在提示词里。
                let wb = workbench::resolve(
                    input.get("template").and_then(|v| v.as_str()),
                    input.get("manifest"),
                    &skillpack::pack_names().iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                )?;
                workbench::install(&wb, std::path::Path::new(&p), ow)
            },
            None,
        ),
        actions::readonly_opt(
            actions::AI_TASKS_INSPECT,
            "Inspect what every AI on this machine is working on",
            "List tasks from every AI on this computer — Claude Code, Codex CLI and Hermes session records plus tasks the AI logged itself on the uking-board. Reads local files only; never prompt bodies beyond the first line used as a title, never uploads.",
            30_000,
            serde_json::json!({
                "days": { "type": "integer", "minimum": 1, "maximum": 365, "description": "Look-back window in days (default 7)." }
            }),
            &["days", "tasks", "sources", "counts", "notes"],
            |_, input, _| {
                let days = input.get("days").and_then(|v| v.as_i64()).unwrap_or(7);
                action_json(aitasks::inspect(days))
            },
        )
        // ★ 观测记账：这五个来源**各自会独立失败**（路径变了/没权限/对方换了格式），
        // 所以必须逐个交代死活。声明了它，conformance 就会强制 `sources` 里
        // 每个来源都有 present/readable/count，且 count==0 时必须写明为什么。
        // 不声明的话，一个「五个都没读到」的空结果和「这台机器上真没有」长得一模一样。
        .observing(&["claude", "codex", "hermes", "board", "openclaw"]),
        actions::readonly_opt(
            actions::USAGE_LOCAL_INSPECT,
            "Inspect local AI spend by model",
            "Aggregate local AI session logs by model, including your own keys (BYOK). Covers Claude Code, Codex CLI, OpenClaw/ClawX, Hermes and pi — whichever the user has enabled in ~/.uking/usage-tools.json. Reads metadata only, never prompt text, never uploads.",
            60_000,
            serde_json::json!({
                "days": { "type": "integer", "minimum": 1, "maximum": 365, "description": "Look-back window in days (default 30)." }
            }),
            &["days", "total_cny", "total_calls", "items", "source"],
            |_, input, _| {
                let days = input.get("days").and_then(|v| v.as_i64()).unwrap_or(30);
                // 组合根注入「压缩机在不在生效」：usage_local 不认识 rtk（模块间不互相 import），
                // 但省钱建议里该提这一条。用 is_active() 而不是 status() —— 后者要起子进程。
                action_json(usage_local::breakdown(days, rtk::is_active()))
            },
        ),
        // ★ Token 水电表。跟上面那条 `usage_local` 的分工：那个只回答「按模型花了多少」，
        // 这个回答「**什么时候**用的、**哪个项目**在耗、缓存有没有在帮你省、按这个速度还能用多久」
        // —— 省 token 得有对照才成立，一个只给总数的账单对照不出任何东西。
        // 两者读同一份日志、走同一次扫描代码，不是两套统计（宪法第 8 条）。
        //
        // `balance_cny` 由调用方传：**只读动作不发网络请求**，所以余额不在这里查；
        // 给了才算「还能用几天」，没给就是 null —— 不猜一个数吓客户。
        actions::readonly_opt(
            actions::USAGE_METER_INSPECT,
            "Read the local token meter (usage over time, by project, cache and pace)",
            "Aggregate local AI session logs into a utility-meter style report: daily readings, today/yesterday/7d/window totals, spend by model / tool / project, prompt-cache hit rate and savings, burn pace, and deterministic money-saving tips. Covers Claude Code, Codex CLI, OpenClaw/ClawX, Hermes and pi (whichever the user enabled). `sources` lists EVERY AI tool detected on the machine — including ones that can never be counted — each with the reason, so the totals are never mistaken for the whole picture. Tools the user marked as flat-rate subscriptions still report tokens but always cost 0. Reads metadata only, never prompt text, never uploads, never hits the network.",
            60_000,
            serde_json::json!({
                "days": { "type": "integer", "minimum": 1, "maximum": 365, "description": "Look-back window in days (default 30)." },
                "balance_cny": { "type": "number", "description": "Remaining account balance in CNY. Supply it to get days_left; omitted means days_left is null (never guessed)." },
                "detail": { "type": "integer", "minimum": 0, "maximum": 2000, "description": "Return up to N per-call ledger rows in `events` (newest first; 0 = none). Each row is one model call — EXCEPT Hermes, whose rows are whole-session rollups (`session_rollup: true`). `events_meta.truncated` says how many rows were cut: the rows are NOT the whole window, only its total is." }
            }),
            &["days", "ready", "blockers", "window", "today", "daily", "by_model", "by_project", "cache", "pace", "sources", "source"],
            |_, input, _| {
                let days = input.get("days").and_then(|v| v.as_i64()).unwrap_or(30);
                let balance = input.get("balance_cny").and_then(|v| v.as_f64());
                // 逐条流水：默认不给（30 天窗口下可能好几万条，没人要的时候不该背这份内存）。
                // 上限 2000 由 schema 挡住 —— 入参真的会校验，不是写着好看的。
                let detail = input.get("detail").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                // 同 usage_local：压缩机在不在生效由组合根注入（usage_local 不认识 rtk）。
                action_json(usage_local::meter(days, rtk::is_active(), balance, detail))
            },
        ),
        // 脱敏诊断正文。远程排障就靠它 —— 一条 `action run` 顶掉过去的 `--feedback-test`。
        actions::readonly(
            actions::DIAGNOSTICS_COLLECT,
            "Collect a redacted diagnostics report",
            "Collect the same redacted diagnostics the feedback page sends: versions, install logs, crash forensics. Never includes full keys, tokens, emails or user paths.",
            30_000,
            &["text", "chars"],
            |_, _, _| {
                let text = feedback::collect_diagnostics();
                Ok(serde_json::json!({ "chars": text.chars().count(), "text": text }))
            },
        ),
        // ─────────────── 写动作：会改这台机器 ───────────────
        // 全部 confirmation=required + idempotent。核心强制确认，GUI 传 confirm、CLI 传 --yes、
        // AI 想绕开界面直接调也一样被拦（宪法第 16 条）。
        actions::write(
            actions::DRIVER_APPLY,
            "Switch the AI driver for one or more tools",
            "Write the chosen provider's endpoint, key and model into Claude Code / Codex / ClawX / Hermes / DeepSeek Harness config. Reversible: 'official' removes only U-King-owned state.",
            120_000,
            "required",
            serde_json::json!({
                "provider_id": { "type": "string", "description": "Preset id, e.g. xiapan / deepseek / official." },
                "api_key": { "type": "string", "description": "Key for the provider. Never logged." },
                "model": { "type": "string", "description": "Optional model override." },
                "targets": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["claude", "codex", "clawx", "hermes", "dsh", "qwen", "crush", "opencode", "pi", "cline"] },
                    "description": "Any supported AI tool id. DSH Web and terminal share the dsh target."
                }
            }),
            &["provider_id", "api_key", "targets"],
            &["applied"],
            |_, input, _| {
                let provider_id = input["provider_id"].as_str().unwrap_or_default().to_string();
                let api_key = input["api_key"].as_str().unwrap_or_default().to_string();
                let model = input.get("model").and_then(|v| v.as_str()).map(String::from);
                let targets: Vec<String> = input["targets"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if targets.is_empty() {
                    return Err("invalid_input: targets 不能为空".into());
                }
                let r = apply_provider_with_cheap_route(&provider_id, &api_key, model.as_deref(), &targets)?;
                Ok(serde_json::json!({
                    "applied": action_json(r)?,
                    "state_version": driver_state_version(),
                }))
            },
            Some(driver_state_version),
        ),
        actions::write(
            actions::CONTEXT_MENU_SET,
            "Turn the Explorer right-click entry on or off",
            "Register or unregister the 'Open with U-King' shell entry under HKCU. Windows only; fully reversible.",
            15_000,
            "required",
            serde_json::json!({ "enabled": { "type": "boolean", "description": "true = register, false = unregister." } }),
            &["enabled"],
            &["enabled"],
            |_, input, _| {
                let on = input["enabled"].as_bool().unwrap_or(false);
                if on {
                    context_menu::register(&preferred_exe_path())?;
                } else {
                    context_menu::unregister()?;
                }
                Ok(serde_json::json!({ "enabled": on }))
            },
            None,
        ),
        actions::write(
            actions::RTK_SET_ENABLED,
            "Turn the token squeezer on or off",
            "Merge or remove U-King's RTK hook in the user's ~/.claude/settings.json. Touches only our own keys.",
            30_000,
            "required",
            serde_json::json!({ "enabled": { "type": "boolean", "description": "true = enable the hook, false = remove it." } }),
            &["enabled"],
            &["enabled", "message"],
            |_, input, _| {
                let on = input["enabled"].as_bool().unwrap_or(false);
                let msg = rtk::set_enabled(on)?;
                Ok(serde_json::json!({ "enabled": on, "message": msg }))
            },
            None,
        ),
        // 「一键配好全部 AI」。比 driver.apply **更**该有门禁：它一次把 Key 写进
        // Claude / Codex / ClawX / Hermes 全部已装工具，而且工具清单是后端自己探的。
        //
        // `targets` 可选（0.9.84）：**装没装是事实（后端探），改哪些是意图（用户说）**。
        // 不给 = 探到的全配（老行为）；给了就只配名单里的，且名单里的也得先真装了才写。
        // 加这个字段是因为「无差别覆盖全部已装工具」跟「不抢用户使用权」直接冲突 ——
        // 客户可能就是不想让我们碰他的 Codex。
        actions::write(
            actions::DRIVER_APPLY_EVERYWHERE,
            "Configure installed AI tools at once",
            "Detect which AI tools are installed and write the provider endpoint, key and model into them. Which tools are installed is always detected on this machine, never taken from the caller; `targets` only narrows down which of them the user agreed to change.",
            180_000,
            "required",
            serde_json::json!({
                "provider_id": { "type": "string", "description": "Preset id. Defaults to xiapan." },
                "api_key": { "type": "string", "description": "Leave empty to use this device's built-in key (generated offline)." },
                "model": { "type": "string", "description": "Optional model override." },
                // enum 从 `providers::APPLY_ALL_TARGETS` 现读，**不手抄** ——
                // 手抄的那份从 2026-08-03 起就漏了 pi/qwen/crush/opencode，
                // 对外声明「只认 4 个」而后端实际配 8 个，整整漂了十一天没人发现，
                // 因为 enum 当时根本没人执行（0.9.99 已在 actions.rs 补上）。
                "targets": {
                    "type": "array",
                    "items": { "type": "string", "enum": providers::APPLY_ALL_TARGETS },
                    "description": "Optional. Only configure these tools. Omit to configure every detected tool."
                }
            }),
            &[],
            &["configured", "state_version"],
            |_, input, _| {
                let pid = input.get("provider_id").and_then(|v| v.as_str()).unwrap_or("xiapan").to_string();
                let key = match input.get("api_key").and_then(|v| v.as_str()) {
                    Some(k) if !k.trim().is_empty() => k.to_string(),
                    _ => device::device_key_offline()?,
                };
                let model = input.get("model").and_then(|v| v.as_str());
                let targets: Option<Vec<String>> = input.get("targets").and_then(|v| v.as_array()).map(|a| {
                    a.iter().filter_map(|x| x.as_str().map(String::from)).collect()
                });
                let r = apply_everywhere_with_cheap_route(&pid, &key, model, targets.as_deref())?;
                let mut v = action_json(r)?;
                if let Some(o) = v.as_object_mut() {
                    o.insert("state_version".into(), serde_json::Value::String(driver_state_version()));
                }
                Ok(v)
            },
            Some(driver_state_version),
        ),
        // 自定义 provider 落盘（新增/更新同为 upsert —— 后端本来就是一份实现，
        // 前端分两个命令只为语义清晰，动作层没必要跟着分裂）。
        actions::write(
            actions::PROVIDER_SAVE,
            "Create or update a custom provider preset",
            "Upsert one user-defined provider into ~/.uking/providers.json. Built-in presets are rejected.",
            15_000,
            "required",
            serde_json::json!({ "provider": { "type": "object", "description": "The ProviderPreset to save." } }),
            &["provider"],
            &["saved"],
            |_, input, _| {
                let preset: providers::ProviderPreset = serde_json::from_value(input["provider"].clone())
                    .map_err(|e| format!("invalid_input: provider 字段不是合法的 ProviderPreset: {e}"))?;
                Ok(serde_json::json!({ "saved": action_json(providers::save_custom_provider(preset)?)? }))
            },
            None,
        ),
        // 从列表里移除一个 provider。**内置也能移除**（0.9.84「列表主权归用户」）：
        // 内置立墓碑、自定义真删，两条路都在 `providers::remove_provider_for` 里，这里不分派。
        // 仍标 destructive：自定义那条确实不可恢复（内置可以再「添加」回来，但按最坏的那条声明）。
        // ⚠️ 它只动 U-King 的列表，**不碰客户机器上任何 AI 工具的配置** —— 移除虾盘云 ≠ 把
        // Claude Code 还原成官方。要还原是另一个动作（driver.apply official），故意不在这里连坐。
        //
        // ★ `tool` 是可选的，给了就**只动那一个 AI 的列表**（0.9.9x：每个 AI 各有一份列表，
        // 客户原话「Claude Code 的删除，Hermes 的留下来」）。不给 = 从所有 AI 的列表里拿走、
        // 自定义连定义带 Key 一起删 —— 保持老调用方（CLI / MCP / AI）语义不变：
        // 「从我的列表里删掉它」在没指定 AI 时就该是删干净，而不是悄悄只删了四分之一。
        actions::destructive(
            actions::PROVIDER_DELETE,
            "Remove a provider from the list",
            "Remove one provider from the user's list. With `tool`, only that AI's list is touched (the other AIs keep it, and custom presets keep their definition + key). Without `tool`, it is removed from every AI's list and custom presets are deleted outright. Built-in presets are tombstoned and never come back on their own until the user restores them. Never touches any AI tool's own config files.",
            15_000,
            serde_json::json!({
                "id": { "type": "string", "description": "Provider id (built-in or custom)." },
                "tool": { "type": "string", "enum": providers::LIST_TOOLS, "description": "Only remove it from this AI's list. Omit to remove from every AI's list (and delete custom definitions)." }
            }),
            &["id"],
            &["removed"],
            |_, input, _| {
                let id = input["id"].as_str().unwrap_or_default().to_string();
                let tool = input["tool"].as_str().map(str::to_string);
                providers::remove_provider_for(tool.as_deref(), &id)?;
                Ok(serde_json::json!({ "removed": id, "tool": tool }))
            },
        ),
        // 把一个被移除的内置驱动加回列表（界面上那个不显眼的「添加虾盘云」）。
        // 幂等：已经在列表里再调一次也是 Ok。**只有用户显式调它，列表才会多东西** ——
        // 没有任何自动补种路径，否则「删了又回来」就是换个姿势继续抢。
        actions::write(
            actions::PROVIDER_RESTORE,
            "Restore a removed provider",
            "Put a previously removed preset (e.g. xiapan) back into a list. With `tool`, only that AI's list gets it back; omit to restore it everywhere. Idempotent. Custom providers that were deleted outright cannot be restored — they are gone for good and must be re-entered.",
            15_000,
            "required",
            serde_json::json!({
                "id": { "type": "string", "description": "Provider id to restore (must still have a definition)." },
                "tool": { "type": "string", "enum": providers::LIST_TOOLS, "description": "Only restore it into this AI's list. Omit to restore into every AI's list." }
            }),
            &["id"],
            &["restored"],
            |_, input, _| {
                let id = input["id"].as_str().unwrap_or_default().to_string();
                let tool = input["tool"].as_str().map(str::to_string);
                providers::restore_provider_for(tool.as_deref(), &id)?;
                Ok(serde_json::json!({ "restored": id, "tool": tool }))
            },
            None,
        ),
        // 🔴 **回验：回读工具自己的配置，回答「它真的会照着跑吗」。**
        //
        // 立项当天（2026-08-24）一口气抓到三条它必然会亮红、而旧的字节级回读必然放行的：
        // pi 的 defaultProvider、opencode 的 jsonc 覆盖、codex 的 unknown field。
        // 三条都是「GUI 报已切到 DeepSeek、工具跑的是别人」，而我们这边**零信号** ——
        // 客户唯一能得出的结论就是「这软件的设置不准」（原话）。
        //
        // 只读、无必填入参：不给 `target` 就把所有配置目标都回读一遍（体检式）。
        // `readable:false` = **我们没有这个工具的回读路径**，不是「读了没配」——
        // 调用方（含 GUI）必须把这两种渲染成不同的东西，别把「没查」显示成绿勾。
        actions::readonly_opt(
            actions::PROVIDER_EFFECTIVE,
            "Read back what each AI tool would actually run",
            "Parse each AI tool's OWN config file and report the provider/model it would really use at startup, plus any file that overrides ours (e.g. opencode.jsonc beats opencode.json). This is NOT a read-back of what U-King wrote: byte-level verification only proves the file contains our bytes, not that the tool reads those fields. `readable:false` means we have no read-back path for that tool — that is 'unknown', not 'not configured'.",
            10_000,
            serde_json::json!({
                "target": {
                    "type": "string",
                    "enum": providers::LIST_TOOLS,
                    "description": "Only read back this one target. Omit to read them all."
                }
            }),
            &["targets"],
            |_, input, _| {
                let one = input["target"].as_str();
                let list: Vec<serde_json::Value> = providers::LIST_TOOLS
                    .iter()
                    .filter(|t| one.is_none_or(|w| w == **t))
                    .map(|t| serde_json::to_value(providers::effective_config(t)).unwrap_or_default())
                    .collect();
                Ok(serde_json::json!({ "targets": list }))
            },
        ),
        actions::write(
            actions::RTK_UNINSTALL,
            "Uninstall the token squeezer",
            // 别写死 `rtk.exe`：非 Windows 上那个文件就叫 `rtk`（见 rtk::rtk_exe）。
            "Remove U-King's RTK hook from ~/.claude/settings.json and delete the rtk binary. Touches nothing else the user configured.",
            60_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, _| Ok(serde_json::json!({ "message": rtk::uninstall()? })),
            None,
        ),
        // 幂等靠 install_uu_remote 里的前置去重（已装就跳过下载），不是靠这里声明一句。
        // 带进度：86MB 下载 + 安装能跑一两分钟，声明「无进度」等于让 UI 只能干等转圈。
        actions::with_progress(actions::write(
            actions::UU_REMOTE_INSTALL,
            "Install UU Remote so the author can see this screen",
            "Download and install NetEase UU Remote (~86 MB installer; the vendor ships no portable build). Tries a silent NSIS install and falls back to the visible installer. Connecting is still done by the user inside UU Remote — this never touches their account.",
            900_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, progress| {
                Ok(serde_json::json!({ "message": tools::install_uu_remote(progress)? }))
            },
            None,
        )),
        // 幂等：已是最新就跳过下载（podapp::install 里真兑现，不是这里声明一句）。
        // 「安装」和「更新到最新」是**同一条代码路径** —— 分成两个动作就会有两套版本判据。
        actions::with_progress(actions::write(
            actions::PODAPP_INSTALL,
            "Install or update PodApp (泊舟 AI 小程序)",
            "Download the latest PodApp installer (manifest source order puts the China-reachable mirror first) and install it silently. PodApp updates itself afterwards — this is only the first install / a manual catch-up.",
            600_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, progress| Ok(serde_json::json!({ "message": podapp::install(progress)? })),
            None,
        )),
        // 启动是写动作：它在客户机上起了一个常驻进程。幂等 —— 再点一次 PodApp 自己会聚焦already-running 实例。
        actions::write(
            actions::PODAPP_LAUNCH,
            "Launch PodApp (泊舟 AI 小程序)",
            "Start the installed PodApp dock. Idempotent: launching again just focuses the running instance.",
            30_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, _| Ok(serde_json::json!({ "message": podapp::launch()? })),
            None,
        ),
        // —— 优化大师的「动手」那一半 ——
        //
        // 🔴 补这两条之前，优化大师是**半个能力**：`runtime.optimizer.inspect` 让任何调用方
        // 都看得到分数和问题清单，但「改」只活在 Tauri command 里。结果是 AI 拿到一份
        // 挑不出毛病的诊断，末了只能说「请你自己去侧栏点一下一键优化」——
        // 一个只能看不能动的医生，客户不会认为那是医生。
        actions::write(
            actions::OPTIMIZER_APPLY,
            "Apply one AI-runtime optimization",
            "Run one forward-only repair from the optimizer (fix / optimize / defender) and return its human-readable report. Records a before/after doctor score anchor, so the effect is auditable afterwards. `undo` is deliberately not offered here: it peels one journal layer per call and is therefore not replay-safe.",
            300_000,
            "required",
            serde_json::json!({
                // enum 从 `is_mutating_optimize` 认的那三个来 —— 少一个 `undo`，
                // 而这正是它跟老命令唯一的差别，写在描述里让调用方一眼看到。
                "action": {
                    "type": "string",
                    "enum": ["fix", "optimize", "defender"],
                    "description": "fix = repair what is broken; optimize = apply the full tuning pass; defender = add U-King's AI tool folders to the antivirus exclusion list."
                }
            }),
            &["action"],
            &["action", "output"],
            |_, input, _| {
                let action = input["action"].as_str().unwrap_or_default().to_string();
                let output = optimizer_apply(&action)?;
                Ok(serde_json::json!({ "action": action, "output": output }))
            },
            None,
        ),
        // 幂等由 `installer::ensure_*` 真兑现（已装就探到并秒回，不重下），不是这里声明一句。
        // 带进度：PowerShell 7 缺的时候是 ~106MB 下载，几分钟起步。
        actions::with_progress(actions::write(
            actions::ENV_INSTALL_TOOLS,
            "Install the portable tools the optimizer asks for",
            "Install portable Node, Git (which brings bash.exe that Claude Code's Bash tool needs), PowerShell 7 and the CLI command guard under ~/.uking/runtime. No administrator rights needed. Best-effort: one failure does not stop the others, and every field honestly reports ok / skip / fail: <reason> — `skip` means the step does not exist on this platform, not that it succeeded.",
            900_000,
            "required",
            serde_json::json!({}),
            &[],
            &["node", "git", "pwsh", "command_guard"],
            |_, _, progress| action_json(installer::install_env_tools(progress)),
            None,
        )),
        // 浏览器面板的运行时安装：同一条有确认门的动作供 GUI / CLI / MCP 调用。
        // npm 安装成功不等于可用；最后必须用 browser.stream + snapshot 真验 Chrome 与 daemon。
        actions::with_progress(actions::write(
            actions::BROWSER_RUNTIME_INSTALL,
            "Install and verify the managed browser runtime",
            "Install U-King's pinned agent-browser runtime, then start its stream and capture an accessibility snapshot. Requires confirmation because it downloads and writes local runtime files. Idempotent when the pinned runtime already verifies.",
            900_000,
            "required",
            serde_json::json!({}),
            &[],
            &["changed", "version", "stream", "snapshot"],
            |_, _, progress| {
                // 已是精确版本且 Chrome/stream/snapshot 均真验通过：不触网、不重装。
                // Chrome 自身不可用时也不假装「修复」而重下 npm；只有缺包/错版才进入安装流。
                match browser_runtime_install_decision(browser::runtime_preflight(progress))? {
                    Some(ready) => return Ok(ready),
                    None => {
                        progress("浏览器运行时缺失或版本不匹配，开始安装固定版本…");
                    }
                }
                let skill = installer::load_skill();
                let installed = installer::install_tool(&skill, "agent-browser", &|phase, line| {
                    progress(&format!("{phase}: {line}"));
                });
                if !installed.ok {
                    return Err(installed.error.unwrap_or_else(|| "browser runtime install failed".into()));
                }
                let verified = browser::runtime_preflight(progress)?;
                Ok(serde_json::json!({
                    "changed": true,
                    // 两条路径都以同一份 preflight 的规范版本字段返回，避免
                    // `0.27.0` 和 `agent-browser 0.27.0` 让调用方误判成版本变化。
                    "version": verified["version"],
                    "stream": verified["stream"],
                    "snapshot": verified["snapshot"],
                }))
            },
            None,
        )),
        // —— 身份与「给 AI 的说明书」——
        //
        // **这四个动作的意义**：U-King 的能力早就全是机器可读的（就是这张表），
        // 但装在同一台机器上的**别家 AI** 根本不知道我们存在。`identity.publish`
        // 把这张表现场编译成 `~/.uking/llms.txt`，让任何 AI 一读就知道能调什么。
        // 说明书是**编译产物**：加动作 → 重新 publish → 说明书自动跟上，
        // 永远不会出现「手写文档和动作表对不上」（宪法第 8 条）。
        actions::readonly(
            actions::IDENTITY_INSPECT,
            "Inspect U-King's identity and its AI-facing manual",
            "Read who this U-King is (name, owner, role, traits), where its manual and logs live, and whether the manual has been published so other AIs can discover it. Reads only; never returns secret values.",
            5_000,
            &["spec_version", "ready", "blockers", "identity", "files", "published", "secrets"],
            |_, _, _| Ok(identity::inspect_in(&identity::uking_dir(), &identity::home_dir())),
        ),
        actions::write(
            actions::IDENTITY_SAVE,
            "Save U-King's identity",
            "Write the user-editable identity (name, owner, role, traits, notes) to ~/.uking/identity.json. Plain text by design — it is meant to be read by other AIs.",
            10_000,
            "required",
            serde_json::json!({
                "name":   { "type": "string", "description": "What this U-King is called on this machine." },
                "owner":  { "type": "string", "description": "How the owner wants to be addressed." },
                "role":   { "type": "string", "description": "One-line duty, e.g. 'maritime paperwork'." },
                "traits": { "type": "object", "description": "Free-form key/value attributes." },
                "notes":  { "type": "string", "description": "Free text the owner wants every AI to read." }
            }),
            &[],
            &["saved"],
            |_, input, _| {
                let dir = identity::uking_dir();
                // 只覆盖传进来的字段，没传的保持原样 —— 免得界面上只改名字却把 notes 清空。
                let mut id = identity::load_identity_in(&dir);
                if let Some(v) = input.get("name").and_then(|v| v.as_str()) { id.name = v.into(); }
                if let Some(v) = input.get("owner").and_then(|v| v.as_str()) { id.owner = v.into(); }
                if let Some(v) = input.get("role").and_then(|v| v.as_str()) { id.role = v.into(); }
                if let Some(v) = input.get("notes").and_then(|v| v.as_str()) { id.notes = v.into(); }
                if let Some(v) = input.get("traits").and_then(|v| v.as_object()) { id.traits = v.clone(); }
                identity::save_identity_in(&dir, &id)?;
                // 身份变了说明书就旧了 —— 顺手重编，别让用户改完名字还得记着点「发布」。
                let files = identity::publish_in(&dir, &id, &actions::manifest(), &skillpack::skill_catalog())?;
                Ok(serde_json::json!({ "saved": true, "files": files }))
            },
            Some(identity_state_version),
        ),
        actions::write(
            actions::IDENTITY_PUBLISH,
            "Publish the AI-facing manual (llms.txt)",
            "Compile the live action table into ~/.uking/llms.txt and llms-full.txt so any AI on this machine can discover what U-King can do. Idempotent. Secret values are never written into these files.",
            15_000,
            "required",
            serde_json::json!({}),
            &[],
            &["files"],
            |_, _, _| {
                let dir = identity::uking_dir();
                let id = identity::load_identity_in(&dir);
                let files = identity::publish_in(&dir, &id, &actions::manifest(), &skillpack::skill_catalog())?;
                Ok(serde_json::json!({ "files": files }))
            },
            None,
        ),
        // 🔴 **让说明书真正被发现的那一步。** 只 publish 不 link，等于把说明书锁在抽屉里：
        // 文件生成了、conformance 全绿、客户机上却一个 AI 都不会去读它。
        // 写的是**用户自己的**记忆文件，所以：只增不删（我们的内容全在标记块里）、
        // 首次改动留底 `*.uking-bak`、`linked:false` 能精确撤销回原样。
        actions::write(
            actions::IDENTITY_LINK,
            "Let other AIs discover U-King",
            "Insert (or remove) a one-line pointer to ~/.uking/llms.txt inside the global memory files of the AI tools installed on this machine (Claude Code / Codex / AGENTS.md). Additive and fully reversible: our text lives inside a marked block, the user's own content is never modified.",
            15_000,
            "required",
            serde_json::json!({
                "linked":  { "type": "boolean", "description": "true = insert the pointer, false = remove it. Defaults to true." },
                "targets": { "type": "array", "description": "Optional subset of target ids (claude / codex / agents). Empty = every installed tool." }
            }),
            &[],
            &["discovery"],
            |_, input, _| {
                let home = identity::home_dir();
                let linked = input.get("linked").and_then(|v| v.as_bool()).unwrap_or(true);
                let changed = if linked {
                    let ids: Vec<String> = input
                        .get("targets")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    identity::link_in(&home, &ids)?
                } else {
                    identity::unlink_in(&home)?
                };
                Ok(serde_json::json!({
                    "changed": changed,
                    "discovery": identity::discovery_in(&home),
                }))
            },
            None,
        ),
        // 凭据**单独一个动作**，而不是塞进 identity.save：写的是另一个文件、另一个密级。
        // 混在一起迟早有人把 secrets 当 identity 渲染进明文说明书。
        actions::write(
            actions::IDENTITY_SECRET_SET,
            "Set or clear one credential",
            "Write one credential into ~/.uking/secrets.json (private, never rendered into llms.txt). An empty value deletes the entry. Returns only names, never values.",
            10_000,
            "required",
            serde_json::json!({
                "name":  { "type": "string", "description": "Credential name, e.g. xiapan / openai." },
                "value": { "type": "string", "description": "The secret. Empty string deletes it. Never logged or echoed." }
            }),
            &["name"],
            &["secrets"],
            |_, input, _| {
                let dir = identity::uking_dir();
                let name = input["name"].as_str().unwrap_or_default();
                let value = input.get("value").and_then(|v| v.as_str()).unwrap_or_default();
                identity::set_secret_in(&dir, name, value)?;
                // 凭据清单变了，说明书里的「有哪些 Key」也得跟上（依然只写名字不写值）。
                let id = identity::load_identity_in(&dir);
                let _ = identity::publish_in(&dir, &id, &actions::manifest(), &skillpack::skill_catalog());
                Ok(serde_json::json!({
                    "secrets": identity::secret_summaries(&identity::load_secrets_in(&dir))
                }))
            },
            None,
        ),
        // 换掉本机访问凭证。**必须确认**：它会当场吊销旧凭证，而旧凭证可能正被
        // 客户自己的脚本 / 另一台机器上的配置引用着。核心强制确认不是 GUI 的礼貌 ——
        // CLI / MCP / 远端影子进来一样被 `confirmation_required` 挡下。
        actions::write(
            actions::DEVICE_KEY_ROTATE,
            "Rotate this device's cloud access key",
            "Ask the server for a fresh random access key, move the wallet balance onto it, write it into the local config, then revoke the old key. Two-phase and replay-safe: the old key keeps working until the new one has been verified.",
            60_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, _| Ok(serde_json::json!({ "message": device::rotate_device_key()? })),
            None,
        ),
        // 填入一把已有的密钥。**要确认**：它会顶掉本机当前那把 —— 如果当前这把上还有
        // 余额而客户没有备份，顶掉就意味着他自己找不回来了。核心强制确认，CLI / MCP 同样被挡。
        actions::write(
            actions::DEVICE_KEY_ADOPT,
            "Use an existing cloud access key on this machine",
            "Save an access key you already have (from another computer, another copy, or generated on the website) as this machine's key. The key is verified against the server before anything is written — a typo never gets saved.",
            30_000,
            "required",
            serde_json::json!({
                "key": { "type": "string", "description": "The sk- access key to use on this machine." }
            }),
            &["key"],
            &["message"],
            |_, input, _| {
                let key = input["key"].as_str().unwrap_or_default();
                Ok(serde_json::json!({ "message": device::adopt_device_key(key)? }))
            },
            None,
        ),
        // 只清本机钱包。旧 Key/余额仍在服务端，备份 Key 后随时可用 adopt 找回。
        actions::write(
            actions::DEVICE_WALLET_RESET_LOCAL,
            "Remove the device wallet from this machine",
            "Clear U-King-managed consumers of the current wallet, then remove only this machine's wallet reference. The server wallet, key and balance are not deleted. The next online convergence creates a new zero-balance device wallet.",
            30_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, _| Ok(serde_json::json!({ "message": device::reset_local_device_wallet()? })),
            None,
        ),
        // ── 被管理契约（企业版第一层）──
        // 同份二进制服务个人与企业：个人版 `~/.uking/org.json` 默认 unmanaged，
        // 本动作就报「没被托管」，零行为变化。企业 enroll 后只多一份身份记录；
        // 策略下发 / 遥测回流是后续步骤（需求榜 E2/E3），且遥测必须显式联动
        // metrics consent —— 「managed」本身永远不等于「可以上传」。
        actions::readonly(
            actions::ORG_INSPECT,
            "Inspect enterprise managed state (org)",
            "Read whether this machine is enrolled into an enterprise org (~/.uking/org.json), and which org owns it. Unmanaged by default; personal editions see ready=false with a clear blocker. Reads only.",
            5_000,
            &["mode", "ready", "blockers"],
            |_, _, _| action_json(org::inspect_json()),
        ),
        actions::write(
            actions::ORG_ENROLL,
            "Enroll this machine into an enterprise org",
            "Record the enterprise org identity in ~/.uking/org.json (mode=managed). Idempotent: re-enrolling the same org_id is a no-op. Records identity only — it does NOT turn on any telemetry or policy download (those are gated by explicit consent in later steps).",
            5_000,
            "required",
            serde_json::json!({
                "org_id":     { "type": "string", "description": "Enterprise-assigned org identifier. Required, trimmed." },
                "org_name":   { "type": "string", "description": "Optional display name." },
                "policy_url": { "type": "string", "description": "Optional policy endpoint, reserved for later steps." }
            }),
            &["org_id"],
            &["mode", "ready", "blockers"],
            |_, input, _| {
                let org_id = input["org_id"].as_str().unwrap_or_default();
                let org_name = input.get("org_name").and_then(|v| v.as_str());
                let policy_url = input.get("policy_url").and_then(|v| v.as_str());
                org::enroll(org_id, org_name, policy_url)
            },
            None,
        ),
        actions::write(
            actions::ORG_DISENROLL,
            "Leave the enterprise org (back to personal)",
            "Reset ~/.uking/org.json to the unmanaged default. Idempotent: disenrolling an already-unmanaged machine is a no-op.",
            5_000,
            "required",
            serde_json::json!({}),
            &[],
            &["mode", "ready", "blockers"],
            |_, _, _| org::disenroll(),
            None,
        ),
        // —— 自动化：三个**幂等**的写。存/删/开关重放多少次结果都一样。
        // 「立即运行一次」不在这里：它每跑一次都在烧 token（非幂等），而我们没有幂等键账本 ——
        // 声明一个不兑现的 idempotent 比不声明更坏（调用方一重试就双跑双扣）。
        actions::write(
            actions::AUTOMATION_SAVE,
            "Create or update a scheduled automation",
            "Upsert one scheduled automation (name, prompt, engine, optional working folder, schedule, optional use_memory). Idempotent by id. Note: an automation with a working folder is authorised to read/write files and run commands in that folder unattended. use_memory=true makes each run start from a per-job memory file (~/.uking/automation/<id>-memory.md) so a long job advances across runs; off by default.",
            10_000,
            "required",
            serde_json::json!({
                "job": {
                    "type": "object",
                    "description": "The automation. Schedule kinds: interval (minutes>=5) / daily (at HH:MM) / weekly (at HH:MM + weekdays 0=Sunday)."
                }
            }),
            &["job"],
            &["job"],
            |_, input, _| {
                let job: automation::Job = serde_json::from_value(input["job"].clone())
                    .map_err(|e| format!("自动化格式不对: {e}"))?;
                Ok(serde_json::json!({ "job": automation::upsert(job)? }))
            },
            Some(automation_state_version),
        ),
        actions::write(
            actions::AUTOMATION_REMOVE,
            "Delete a scheduled automation",
            "Remove one scheduled automation by id. Idempotent: deleting a missing id succeeds. Past run records are kept on disk.",
            10_000,
            "required",
            serde_json::json!({ "id": { "type": "string" } }),
            &["id"],
            &["ok"],
            |_, input, _| {
                automation::remove(input["id"].as_str().unwrap_or_default())?;
                Ok(serde_json::json!({ "ok": true }))
            },
            Some(automation_state_version),
        ),
        actions::write(
            actions::AUTOMATION_SET_ENABLED,
            "Enable or disable a scheduled automation",
            "Turn one automation on or off. Turning it on re-arms it for its next slot. Idempotent.",
            10_000,
            "required",
            serde_json::json!({ "id": { "type": "string" }, "enabled": { "type": "boolean" } }),
            &["id", "enabled"],
            &["job"],
            |_, input, _| {
                let id = input["id"].as_str().unwrap_or_default();
                let on = input["enabled"].as_bool().unwrap_or(false);
                Ok(serde_json::json!({ "job": automation::set_enabled(id, on)? }))
            },
            Some(automation_state_version),
        ),
        actions::write(
            actions::DESKTOP_PIN,
            "Pin this portable exe to the desktop",
            "Create a desktop shortcut pointing at the currently running executable.",
            30_000,
            "required",
            serde_json::json!({}),
            &[],
            &["message"],
            |_, _, _| Ok(serde_json::json!({ "message": install::pin_current_exe_to_desktop()? })),
            None,
        ),
        actions::write(
            actions::MEDIA_IMAGE_DESCRIBE,
            "Describe an image with U-King Vision",
            "Send one explicitly selected local image to U-King Vision (Qwen visual chain) and return text only. The selected image is never forwarded to the text-only chat model. This consumes model quota; request_id makes in-process retry safe.",
            190_000,
            "required",
            serde_json::json!({
                "image": { "type": "string", "description": "Absolute local image path (PNG/JPG/WEBP/GIF/BMP/HEIC)." },
                "question": { "type": "string", "description": "Optional question for the vision model." },
                "mode": { "type": "string", "enum": ["describe", "ocr"], "description": "Optional, defaults to describe." },
                "request_id": { "type": "string", "description": "Caller-generated id. Replaying it with identical input returns the in-process cached result instead of another model call." }
            }),
            &["image", "request_id"],
            &["ok", "text", "model", "mode", "elapsed", "source", "cached"],
            |_, input, _| {
                let result = vision::describe(
                    input.get("image").and_then(|v| v.as_str()).unwrap_or_default(),
                    input.get("question").and_then(|v| v.as_str()),
                    input.get("mode").and_then(|v| v.as_str()),
                    input.get("request_id").and_then(|v| v.as_str()),
                )?;
                serde_json::to_value(result).map_err(|e| e.to_string())
            },
            None,
        ),
        actions::write(
            actions::SKILLPACK_INSTALL,
            "Install U-King's skill packs into the installed AI tools",
            "Export the bundled skill packs and copy them into each detected tool's skills directory. Pass `name` to install just one pack (the reverse of runtime.skillpack.uninstall); omit it to sync all. Scripts carry no keys; they read ~/.uking/device.json at runtime.",
            120_000,
            "required",
            serde_json::json!({ "name": { "type": "string", "description": "Optional. Install only this pack, e.g. uking-ppt. Omit to sync all packs." } }),
            &[],
            &["default_dir", "installed"],
            |_, input, _| {
                // 带 name = 只装这一个（按包卸载的反面）；不带 = 老行为，全量同步
                if let Some(name) = input.get("name").and_then(|v| v.as_str()) {
                    let dirs = skillpack::install_pack(name)?;
                    return Ok(serde_json::json!({ "default_dir": dirs.first().cloned().unwrap_or_default(), "installed": dirs }));
                }
                let default_dir = skillpack::export_to(None)?;
                let installed: Vec<serde_json::Value> = skillpack::install_into_tools()
                    .into_iter()
                    .map(|(tool, path, experimental)| {
                        serde_json::json!({ "tool": tool, "path": path, "experimental": experimental })
                    })
                    .collect();
                // 技能装进 OpenClaw/ClawX 了，顺手把 commands.text 打开——否则聊天框里打
                // `/uking-aigc` 只是纯文本发给模型，容易被当成陌生指令格式而拒绝执行。
                let _ = providers::ensure_openclaw_text_commands();
                Ok(serde_json::json!({ "default_dir": default_dir, "installed": installed }))
            },
            None,
        ),
        actions::write(
            actions::EXPERT_DISMISS,
            "Dismiss a hired expert",
            "Delete a hired expert pack from ~/.uking/experts/. Built-in experts are compiled-in constants and cannot be dismissed; asking for one returns dismissed:false rather than an error.",
            15_000,
            "required",
            serde_json::json!({ "id": { "type": "string", "description": "Expert id (folder name under ~/.uking/experts). Only [a-z0-9-], 1..=64." } }),
            &["id"],
            &["dismissed"],
            |_, input, _| {
                let id = input["id"].as_str().unwrap_or_default();
                Ok(serde_json::json!({ "dismissed": expert::dismiss(id)? }))
            },
            None,
        ),
        actions::readonly(
            actions::SKILLPACK_INSPECT,
            "List U-King's bundled skill packs and whether each is installed",
            "Report every bundled skill pack with a one-line summary and how many tool directories currently hold it. `installed` means the folder is really on disk, not that we once called install.",
            10_000,
            &["packs", "ready", "blockers"],
            |_, _, _| {
                let packs: Vec<serde_json::Value> = skillpack::pack_status()
                    .into_iter()
                    .map(|(name, what, installed, dirs)| {
                        serde_json::json!({ "name": name, "what": what, "installed": installed, "dirs": dirs })
                    })
                    .collect();
                let any = packs.iter().any(|p| p["installed"].as_bool().unwrap_or(false));
                Ok(serde_json::json!({
                    "packs": packs,
                    "ready": any,
                    "blockers": if any { vec![] } else { vec!["一个自带技能包都没装 —— AI 现在做不了作图/办公那些活".to_string()] },
                }))
            },
        ),
        actions::write(
            actions::SKILLPACK_UNINSTALL,
            "Remove one of U-King's skill packs from the AI tools",
            "Delete a single bundled skill pack from every tool's skills directory (current and legacy locations). Only U-King's own packs can be removed; unknown names are rejected rather than glob-matched.",
            30_000,
            "required",
            serde_json::json!({ "name": { "type": "string", "description": "Pack name, e.g. uking-ppt. Must be one of U-King's bundled packs." } }),
            &["name"],
            &["removed"],
            |_, input, _| {
                let name = input["name"].as_str().unwrap_or_default();
                Ok(serde_json::json!({ "removed": skillpack::uninstall_pack(name)? }))
            },
            None,
        ),
        actions::readonly(
            actions::READINESS_INSPECT,
            "Can the workbench actually be used right now?",
            "Answers the one question the install flow never answered: is U-Workspace usable on this machine right now. Each check reports whether the thing WORKS, not whether it was installed — `claude` is really executed, the driver is read from disk, the balance is really queried. Every failed check carries a concrete fix.",
            25_000,
            &["checks", "ready", "blockers"],
            |_, _, _| {
                let mut checks: Vec<serde_json::Value> = vec![];
                let mut blockers: Vec<String> = vec![];
                let mut chk = |id: &str, name: &str, ok: bool, detail: String, fix: &str| {
                    checks.push(serde_json::json!({
                        "id": id, "name": name, "ok": ok, "detail": detail, "fix": if ok { "" } else { fix },
                    }));
                    if !ok {
                        blockers.push(format!("{name} —— {detail}。{fix}"));
                    }
                };

                // ① 大脑真跑得起来。**跑 `claude --version`，不是查目录** ——
                //    「npm 装完退出码 0」和「PATH 上有个能跑的 claude」是两件事。
                let st = installer::detect_stack();
                chk(
                    "claude_runs",
                    "Claude Code 能跑",
                    st.claude.found,
                    match &st.claude.version {
                        Some(v) if st.claude.found => format!("`claude --version` → {v}"),
                        _ => "`claude --version` 跑不起来（不在 PATH 上，或装了但坏了）".into(),
                    },
                    "去「首页 · 我的 AI」点一键全安装，它会装好并把 PATH 配上。",
                );

                // ② 驱动配到位 —— 没配的话第一句话就是 401，而客户会归因成「装坏了」
                let drv = providers::driver_status();
                let active = drv.active.get("claude").cloned().unwrap_or_default();
                chk(
                    "driver",
                    "模型驱动已配",
                    !active.trim().is_empty(),
                    if active.trim().is_empty() { "Claude Code 还没接任何供应商".into() } else { format!("当前走 {active}") },
                    "去「AI 设置」选一个供应商（虾盘云开箱即用），或点一键配好。",
                );

                // ③ 余额。🔴 **「没余额」和「查不到」必须分开** —— 网络不通时说「你没钱」
                //    是把「没问到」讲成了「不存在」，客户会跑去重复充值。
                match device::get_device_key() {
                    Ok(dk) => {
                        let n = dk.balance.as_ref().map(|b| b.text.clone()).unwrap_or_default();
                        let has = dk.charged;
                        chk("balance", "账户有余额", has,
                            if has { format!("余额 {n}") } else { "余额为 0 或还没充值".into() },
                            "去「虾盘云 · 充值」充一点；工具本身免费，只有真调 AI 才扣。");
                    }
                    Err(e) => chk("balance", "账户有余额", false,
                        format!("**没查到**（不代表没有）：{e}"), "多半是网络/代理，连上网再点一次自检。"),
                }

                // ④ 技能包真躺在 Claude 的 skills 目录里 —— 少了它「做个 PPT」这类活出不来文件
                let packs = skillpack::pack_status();
                let installed = packs.iter().filter(|(_, _, ins, _)| *ins).count();
                chk("skills", "技能包已就位", installed > 0,
                    format!("{installed}/{} 个自带技能包在工具的 skills 目录里", packs.len()),
                    "去「AI 专家」页把要用的技能包装上（也可以只装用得到的那几个）。");

                let ready = blockers.is_empty();
                // 装机队列里**故意没装**的东西，如实说一句 —— 不说的话它们就是凭空消失了。
                //
                // 🔴 这份列表以前是写死的：hermes 明明装着（shim 在、驱动状态 hermes_installed=true、
                // 甚至用量统计里打了 52 次调用），readiness 还永远把它报成「未安装」——
                // 客户照着提示装完再看还是那句话。装没装必须逐项问机器：
                // hermes 走安装器的统一判据（shim/PATH），ffmpeg / markitdown 走厨具目录的探测。
                let mut optional_not_installed: Vec<serde_json::Value> = Vec::new();
                if !crate::installer::tool_installed("hermes") {
                    optional_not_installed.push(
                        serde_json::json!({ "id": "hermes", "what": "Hermes（唯一自带记忆的 AI）" }),
                    );
                }
                if !toolbox::tool_installed_by_id("ffmpeg") {
                    optional_not_installed.push(serde_json::json!({
                        "id": "env:ffmpeg",
                        "what": "ffmpeg（视频拼接 / 一键成片才用得到，~100MB）"
                    }));
                }
                if !toolbox::tool_installed_by_id("markitdown") {
                    optional_not_installed.push(serde_json::json!({
                        "id": "env:markitdown",
                        "what": "MarkItDown（读 Office 文件更省 token，~65MB）"
                    }));
                }
                Ok(serde_json::json!({
                    "checks": checks,
                    "ready": ready,
                    "blockers": blockers,
                    "optional_not_installed": optional_not_installed,
                }))
            },
        ),
        actions::readonly(
            actions::MINIAPP_INSPECT,
            "List installed mini-apps and whether each one can actually run",
            "Report every installed mini-app: id, name, version, icon, which ActionParity actions it registers, and which host capabilities it was granted. `enabled:false` means it is installed but switched off — its actions stay out of the table.",
            10_000,
            &["apps", "broken", "ready", "blockers"],
            |_, _, _| {
                let apps = miniapp::list();
                let live = apps.iter().filter(|a| a.enabled).count();

                // 目录在、但清单加载失败的。**`list()` 会静默跳过它们** —— 而「装了却调不到
                // 它的动作」正是这么来的：目录明明在，动作表里就是没有。不把这些捞出来，
                // 这条动作就跟它要取代的 `--miniapp-list` 有一模一样的盲区（那个开关当初
                // 就是为补这个盲区才单开的），readiness 也会假绿。
                let mut broken: Vec<serde_json::Value> = vec![];
                if let Ok(rd) = std::fs::read_dir(miniapp::apps_root()) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if !p.is_dir() || e.file_name().to_string_lossy().starts_with('.') {
                            continue;
                        }
                        if let Err(err) = miniapp::load_dir(&p) {
                            broken.push(serde_json::json!({
                                "dir": p.display().to_string(),
                                "error": err,
                            }));
                        }
                    }
                }

                let mut blockers: Vec<String> = vec![];
                if apps.is_empty() && broken.is_empty() {
                    blockers.push("一个小程序都没装 —— 侧栏的小程序区会是空的".into());
                } else if live == 0 && !apps.is_empty() {
                    blockers.push(format!(
                        "装了 {} 个小程序，但全被停用了 —— 它们的动作不在动作表里，AI 也调不到",
                        apps.len()
                    ));
                }
                for b in &broken {
                    blockers.push(format!(
                        "{} 装着但读不出清单（{}）—— 它的动作不会进动作表，界面上跟没装一样",
                        b["dir"].as_str().unwrap_or("?"),
                        b["error"].as_str().unwrap_or("?"),
                    ));
                }

                Ok(serde_json::json!({
                    "apps": apps.iter().map(|a| serde_json::to_value(a).unwrap_or(serde_json::Value::Null)).collect::<Vec<_>>(),
                    "broken": broken,
                    "apps_root": miniapp::apps_root().display().to_string(),
                    "bundled": bundled_apps::count(),
                    // 有坏的就不算 ready —— 「大部分能用」在排障时会被读成「没问题」
                    "ready": live > 0 && broken.is_empty(),
                    "blockers": blockers,
                }))
            },
        ),
        actions::write(
            actions::MINIAPP_UNINSTALL,
            "Uninstall a mini-app",
            "Move the mini-app's folder to the trash area and drop it from the registry, so its actions leave the action table. Its user data under .data/<id>/ is kept unless purge_data is true.",
            30_000,
            "required",
            serde_json::json!({
                "id": { "type": "string", "description": "Mini-app id from runtime.miniapp.inspect." },
                "purge_data": { "type": "boolean", "description": "Also delete the app's user data. Default false — reinstalling normally keeps your stuff." }
            }),
            &["id"],
            &["removed"],
            |_, input, _| {
                let id = input["id"].as_str().unwrap_or_default();
                // 幂等靠这一步：没装就直说没删，而不是报错。`write()` 声明了 idempotent，
                // 声明了就得兑现 —— 重试一个已经成功的卸载不该变成一次失败。
                if miniapp::get(id).is_err() {
                    return Ok(serde_json::json!({ "removed": false }));
                }
                let purge = input["purge_data"].as_bool().unwrap_or(false);
                miniapp::uninstall(id, purge)?;
                Ok(serde_json::json!({ "removed": true }))
            },
            None,
        ),
        actions::write(
            actions::DSH_PLUGIN_INSTALL,
            "Install a plugin into DSH (DeepSeek Harness)",
            "Run `dsh plugin --profile <profile> add <spec>` so the user gets a DSH plugin without opening a terminal. Only that one command is run; the spec comes from our curated list or from what the user pasted.",
            180_000,
            "required",
            serde_json::json!({
                "spec": { "type": "string", "description": "What to install, e.g. github:owner/repo or an npm package name." },
                "profile": { "type": "string", "description": "DSH profile to install into. Defaults to web." }
            }),
            &["spec"],
            &["message"],
            |_, input, _| {
                let spec = input["spec"].as_str().unwrap_or_default().trim().to_string();
                if spec.is_empty() {
                    return Err("invalid_input: spec 不能为空".into());
                }
                // 🔴 spec 直接进 argv，**不拼 shell** —— 它来自用户粘贴，走 shell 就得自己转义，
                // 那正是历史上「cmd /C 吃引号 → skill 静默误执行」那个坑。
                if spec.contains('\n') || spec.contains('\r') {
                    return Err("invalid_input: spec 不能含换行".into());
                }
                let profile = input["profile"].as_str().filter(|p| !p.trim().is_empty()).unwrap_or("web").to_string();
                let out = agent::claude::run_oneshot(
                    "dsh",
                    &["plugin".into(), "--profile".into(), profile, "add".into(), spec],
                    None,
                    170,
                )?;
                Ok(serde_json::json!({ "message": out.lines().rev().take(6).collect::<Vec<_>>().join("
") }))
            },
            None,
        ),
        actions::write(
            actions::MINIAPP_INSTALL,
            "Install a mini-app from a local .ukapp package or folder",
            "Install a mini-app the user picked from disk. Third-party packages are gated: a manifest that asks for any non-read host action is rejected outright, so installing one can never hand it the ability to change this machine without asking.",
            60_000,
            "required",
            serde_json::json!({
                "path": { "type": "string", "description": "Absolute path to a .ukapp package or an unpacked mini-app folder." }
            }),
            &["path"],
            &["id", "name", "version"],
            |_, input, _| {
                let path = input["path"].as_str().unwrap_or_default();
                if path.trim().is_empty() {
                    return Err("invalid_input: path 不能为空".into());
                }
                let info = miniapp::install_from_path(std::path::Path::new(path), "user")?;
                Ok(serde_json::json!({ "id": info.id, "name": info.name, "version": info.version }))
            },
            None,
        ),
        actions::write(
            actions::MINIAPP_RESTORE,
            "Reinstall the mini-apps that ship inside U-King",
            "Clear the 'user deleted this' tombstones and reinstall any built-in mini-app that is missing or outdated. This is the way back from uninstall; apps you installed yourself are untouched.",
            120_000,
            "required",
            serde_json::json!({}),
            &[],
            &["restored", "forgot_removals"],
            |_, _, _| {
                let forgot = miniapp::forget_removals();
                let before: Vec<String> = miniapp::list().into_iter().map(|a| a.id).collect();
                bundled_apps::ensure_installed(&|_| {});
                let after = miniapp::list();
                let restored: Vec<String> = after
                    .iter()
                    .map(|a| a.id.clone())
                    .filter(|id| !before.contains(id))
                    .collect();
                Ok(serde_json::json!({ "restored": restored, "forgot_removals": forgot }))
            },
            None,
        ),
        // ─────────────── 长任务：边跑边报进度 ───────────────
        // 这几条是整个产品最危险的操作（真删除 / 覆盖用户数据 / 关别人的进程）。
        // 之前它们只有前端弹窗保护，核心里没有门禁 —— 现在补上。
        actions::with_progress(actions::destructive(
            actions::FOOTPRINT_REMOVE,
            "Remove selected U-King footprints",
            "Delete or revert the footprint items you picked. Config items revert to their pre-U-King state; AI tools go through their own official uninstaller. Returns whether the app must exit to finish.",
            600_000,
            serde_json::json!({
                "ids": { "type": "array", "description": "Footprint ids from runtime.footprint.inspect." },
                "preserve_user_data": { "type": "boolean", "description": "Archive chat/history to Documents before wiping." }
            }),
            &["ids"],
            &["will_exit"],
            |_, input, log| {
                let ids: Vec<String> = input["ids"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if ids.is_empty() {
                    return Err("invalid_input: ids 不能为空".into());
                }
                let preserve = input.get("preserve_user_data").and_then(|v| v.as_bool()).unwrap_or(false);
                Ok(serde_json::json!({ "will_exit": run_footprint_removal(&ids, preserve, log)? }))
            },
        )),
        actions::with_progress(actions::write(
            actions::BACKUP_CREATE,
            "Back up ClawX chats and settings to a drive",
            "Snapshot ClawX user data and ~/.openclaw into a timestamped folder under the chosen root.",
            1_800_000,
            "required",
            serde_json::json!({ "dest_root": { "type": "string", "description": "Destination root, usually the USB drive." } }),
            &["dest_root"],
            &["result"],
            |_, input, log| {
                let root = input["dest_root"].as_str().unwrap_or_default().to_string();
                let r = backup::backup(&root, env!("CARGO_PKG_VERSION"), log)?;
                Ok(serde_json::json!({ "result": action_json(r)? }))
            },
            None,
        )),
        // destructive 是实话：还原是**整份替换**当前的 ClawX / OpenClaw 数据。
        // 它会自动留底，但「留了底」不等于「可逆」—— 用户得知道自己在覆盖什么。
        actions::with_progress(actions::destructive(
            actions::BACKUP_RESTORE,
            "Restore ClawX chats and settings from a snapshot",
            "Replace the current ClawX / OpenClaw data with a snapshot. The existing data is moved aside first, but this still overwrites what you are using right now.",
            1_800_000,
            serde_json::json!({ "backup_dir": { "type": "string", "description": "Snapshot folder from runtime.backup list." } }),
            &["backup_dir"],
            &["result"],
            |_, input, log| {
                let dir = input["backup_dir"].as_str().unwrap_or_default().to_string();
                let r = backup::restore(&dir, log)?;
                Ok(serde_json::json!({ "result": action_json(r)? }))
            },
        )),
        actions::with_progress(actions::write(
            actions::CLAWX_APPLY_MANAGED,
            "Configure ClawX the managed way (close, write, relaunch)",
            "ClawX holds its config in memory and flushes it back on exit, so writing while it runs is silently undone. This closes it, writes both config layers, then relaunches.",
            180_000,
            "required",
            serde_json::json!({
                "provider_id": { "type": "string", "description": "Preset id." },
                "api_key": { "type": "string", "description": "Leave empty to use this device's built-in key." },
                "model": { "type": "string", "description": "Optional model override." }
            }),
            &["provider_id"],
            &["applied", "state_version"],
            |_, input, log| {
                let pid = input["provider_id"].as_str().unwrap_or_default().to_string();
                let key = match input.get("api_key").and_then(|v| v.as_str()) {
                    Some(k) if !k.trim().is_empty() => k.to_string(),
                    _ => device::device_key_offline()?,
                };
                let model = input.get("model").and_then(|v| v.as_str());
                let r = apply_clawx_managed_inner(&pid, &key, model, log)?;
                Ok(serde_json::json!({ "applied": action_json(r)?, "state_version": driver_state_version() }))
            },
            Some(driver_state_version),
        )),
        actions::with_progress(actions::destructive(
            actions::AITOOL_UNINSTALL,
            "Uninstall one AI tool and everything that makes it look installed",
            "Remove the npm package / stub, ~/.uking/tools/<x>, shims and leftovers; GUI apps go through their own official uninstaller.",
            900_000,
            serde_json::json!({ "tool_id": { "type": "string", "description": "Tool id from the home page cards." } }),
            &["tool_id"],
            &["message"],
            |_, input, log| {
                let tool = input["tool_id"].as_str().unwrap_or_default().to_string();
                Ok(serde_json::json!({ "message": cleanup::uninstall_ai_tool(&tool, log)? }))
            },
        )),
        // ── 浏览器操作（browser.*）── 后端 agent-browser CLI（browser.rs，可替换）。
        // 填表/查资料实测（2026-08-06 Windows）：snapshot 0.45s 返回带 @ref 交互树，
        // fill/select/check/click 每动作 0.4~0.5s，6 字段表单全流程 ~3s。
        // 确认门设计：页面内操作（fill/select/check/click）不打断填表流程；
        // 有外部副作用的提交（发帖/下单/删除）必须走 browser.submit，确认门强制。
        actions::readonly_req(
            browser::BROWSER_OPEN,
            "Open a URL in the agent browser",
            "Navigate the agent browser to an http(s):// or file:// URL and wait for it to load. Reads only, no external side effects.",
            60_000,
            serde_json::json!({ "url": { "type": "string", "description": "http(s):// or file:// URL to open" } }),
            &["url"],
            &["ok", "title", "url"],
            browser::run,
        ),
        actions::readonly_opt(
            browser::BROWSER_SNAPSHOT,
            "Snapshot the current page (interactive tree with @refs)",
            "Return a compact accessibility tree of interactive elements with stable @ref ids for click/fill/select. Much cheaper than raw HTML.",
            30_000,
            serde_json::json!({ "interactive": { "type": "boolean", "description": "Only interactive elements (default true)" } }),
            &["ok", "snapshot"],
            browser::run,
        ),
        actions::readonly_req(
            browser::BROWSER_GET,
            "Read a value from the page",
            "Read text / attribute / title / url from an element or the page. what=text|html|title|url|value|attr, selector=@ref or CSS.",
            30_000,
            serde_json::json!({
                "what": { "type": "string", "description": "What to read: text|html|title|url|value|attr" },
                "selector": { "type": "string", "description": "@ref id from browser.snapshot or CSS selector" }
            }),
            &["what", "selector"],
            &["ok", "value"],
            browser::run,
        ),
        actions::readonly_opt(
            browser::BROWSER_SCREENSHOT,
            "Capture a screenshot of the page",
            "Take a screenshot of the current viewport (or save to path). Useful for vision models.",
            30_000,
            serde_json::json!({ "path": { "type": "string", "description": "Optional absolute path to save the screenshot to" } }),
            &["ok", "path"],
            browser::run,
        ),
        actions::write(
            browser::BROWSER_CLICK,
            "Click an element (in-page interaction)",
            "Click an element by @ref for navigation / expand / select. For submissions with external side effects use browser.submit instead.",
            30_000,
            "never",
            serde_json::json!({ "ref": { "type": "string", "description": "@ref id from browser.snapshot" } }),
            &["ref"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_SUBMIT,
            "Submit a form / perform an action with external side effects",
            "Click the submit element (e.g. 提交/下单/发帖/删除). Requires explicit user confirmation.",
            30_000,
            "required",
            serde_json::json!({ "ref": { "type": "string", "description": "@ref id of the submit button from browser.snapshot" } }),
            &["ref"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_FILL,
            "Fill an input field",
            "Clear and fill a text input / textarea by @ref. In-page state only, no external side effect.",
            30_000,
            "never",
            serde_json::json!({
                "ref": { "type": "string", "description": "@ref id from browser.snapshot" },
                "text": { "type": "string", "description": "Text to fill" }
            }),
            &["ref", "text"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_SELECT,
            "Select a dropdown option",
            "Pick a value from a combobox by @ref. In-page state only.",
            30_000,
            "never",
            serde_json::json!({
                "ref": { "type": "string", "description": "@ref id from browser.snapshot" },
                "value": { "type": "string", "description": "Option value (not display text)" }
            }),
            &["ref", "value"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_CHECK,
            "Check a checkbox",
            "Check a checkbox by @ref. In-page state only.",
            30_000,
            "never",
            serde_json::json!({ "ref": { "type": "string", "description": "@ref id from browser.snapshot" } }),
            &["ref"],
            &["ok"],
            browser::run,
            None,
        ),
        // ── 直播浏览器面板的动作（面板与人共用同一套 browser.*，见 BrowserPanel.tsx）──
        // 全部是页面内交互 / 页面导航，无外部副作用，所以 confirmation="never"（同 click/fill）。
        actions::write(
            browser::BROWSER_BACK,
            "Go back in history",
            "Navigate back one step in the browser history. Page-internal, no external side effect.",
            30_000,
            "never",
            serde_json::json!({}),
            &[],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_FORWARD,
            "Go forward in history",
            "Navigate forward one step in the browser history. Page-internal, no external side effect.",
            30_000,
            "never",
            serde_json::json!({}),
            &[],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_RELOAD,
            "Reload the current page",
            "Reload the current page. Page-internal, no external side effect.",
            30_000,
            "never",
            serde_json::json!({}),
            &[],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_MOUSE,
            "Low-level mouse operation",
            "Move / press / release / wheel the mouse at viewport coordinates. For canvas, maps and elements without @ref.",
            30_000,
            "never",
            serde_json::json!({
                "action": { "type": "string", "description": "move | down | up | wheel" },
                "x": { "type": "number", "description": "Viewport x for move" },
                "y": { "type": "number", "description": "Viewport y for move" },
                "dx": { "type": "number", "description": "Delta x for wheel" },
                "dy": { "type": "number", "description": "Delta y for wheel (positive = scroll down)" }
            }),
            &["action"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_CLICKAT,
            "Click at viewport coordinates",
            "Click at (x, y) viewport coordinates (mouse move + down + up). Use when the element has no @ref.",
            30_000,
            "never",
            serde_json::json!({
                "x": { "type": "number", "description": "Viewport x" },
                "y": { "type": "number", "description": "Viewport y" }
            }),
            &["x", "y"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_TYPE,
            "Type text via keyboard",
            "Type text with real keystrokes into the currently focused element.",
            30_000,
            "never",
            serde_json::json!({ "text": { "type": "string", "description": "Text to type" } }),
            &["text"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_PRESS,
            "Press a key",
            "Press a single key (Enter, Tab, Escape, Control+a, ...) on the focused element / page.",
            30_000,
            "never",
            serde_json::json!({ "key": { "type": "string", "description": "Key name, e.g. Enter / Tab / Escape / Control+a" } }),
            &["key"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::write(
            browser::BROWSER_SCROLL,
            "Scroll the page",
            "Scroll the page by direction (up/down/left/right) and px.",
            30_000,
            "never",
            serde_json::json!({
                "direction": { "type": "string", "description": "up | down | left | right" },
                "px": { "type": "number", "description": "Pixels to scroll (default 100)" }
            }),
            &["direction"],
            &["ok"],
            browser::run,
            None,
        ),
        actions::readonly_opt(
            browser::BROWSER_TABS,
            "List browser tabs",
            "List all open tabs with ids, titles and URLs.",
            30_000,
            serde_json::json!({}),
            &["ok", "tabs"],
            browser::run,
        ),
        actions::readonly_opt(
            browser::BROWSER_STREAM,
            "Get browser live stream info",
            "Ensure the agent-browser session is up and return the ws:// stream address the panel connects to for the live view. Read-only, no navigation.",
            15_000,
            serde_json::json!({}),
            &["ok", "ws_url", "port"],
            browser::run,
        ),
    ];
    // 已装小程序的动作。装一个小程序 = 给这台设备的动作面扩容，
    // CLI / MCP / 影核三个面自动看见，不需要各自再登记一遍。整族共用一个分发 handler。
    t.extend(miniapp::action_specs().into_iter().map(|spec| actions::Action {
        spec,
        handler: |id, input, _| miniapp::run_action(id, input),
        state_fn: None,
    }));
    t
}

/// 已迁到影核动作的老命令共用的薄壳：不卡 UI 地跑一个无入参只读动作。
///
/// 返回 `Value` 而不是 `Result`：这些动作的失败路径（未知 id / 非法入参 / 序列化失败）
/// 全是构造上不可能发生的，老命令也从来不会失败。真出错就带着 `error` 字段回去，
/// 让 devtools 一眼看见，而不是悄悄返回一个空对象。
async fn run_action_blocking(action_id: &'static str) -> serde_json::Value {
    run_action_input(action_id, serde_json::json!({})).await
}

async fn run_action_input(action_id: &'static str, input: serde_json::Value) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || actions::run(action_id, input))
        .await
        .unwrap_or_else(|e| Err(format!("动作执行任务异常: {e}")))
        .unwrap_or_else(|e| serde_json::json!({ "error": e }))
}

/// 写动作的薄壳用。GUI 里点按钮**就是**显式确认，所以这里补上 `confirm: true`；
/// 但确认是核心在判，不是这里在判 —— 从 CLI / MCP / 远端影子进来的调用没这一句，
/// 一样会被核心挡回去（宪法第 16 条：绕开 GUI 按钮不等于绕开权限）。
async fn run_write_action(
    action_id: &'static str,
    mut input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    input["confirm"] = serde_json::Value::Bool(true);
    tauri::async_runtime::spawn_blocking(move || actions::run(action_id, input))
        .await
        .map_err(|e| format!("动作执行任务异常: {e}"))?
}

/// 带进度的写动作薄壳用：跑动作，同时把每条进度 emit 成前端原有的那个事件名
/// （事件名不变 → 前端监听一行都不用改）。
async fn run_write_action_progress(
    app: AppHandle,
    action_id: &'static str,
    event: &'static str,
    mut input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    input["confirm"] = serde_json::Value::Bool(true);
    tauri::async_runtime::spawn_blocking(move || {
        let sink = move |m: &str| {
            let _ = app.emit(event, m.to_string());
        };
        actions::run_with_progress(action_id, input, &sink)
    })
    .await
    .map_err(|e| format!("动作执行任务异常: {e}"))?
}

/// 列表/布尔/字符串型动作的薄壳用：动作输出统一是对象，取出其中一个字段还给老命令，
/// 保住前端原有的返回形状（数组还是数组、布尔还是布尔）。
///
/// `fallback` 不是摆设：动作万一失败（返回的是 `{error}`），取不到字段就得还回**老命令
/// 原本的兜底值**。列表命令过去是 `unwrap_or_default()` 给 `[]`，这里要是漏成 `null`，
/// 前端一个 `.map()` 就白屏 —— 迁移不能把兜底弄丢。
fn action_field(v: serde_json::Value, key: &str, fallback: serde_json::Value) -> serde_json::Value {
    v.get(key).cloned().unwrap_or(fallback)
}

/// 影核协议 Action Core 的桌面适配器。GUI 不直接读 PATH；只调用稳定 Action ID，
/// 与无头 CLI `action run` 共用同一执行入口。
#[tauri::command]
async fn action_run(action_id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || actions::run(&action_id, serde_json::json!({})))
        .await
        .map_err(|e| format!("动作执行任务异常: {e}"))?
}

/// 标准 ActionParity request envelope。旧 `action_run` 继续服务尚未迁移的界面；生成 client
/// 统一调用本入口，两者最终都只落到 `actions::run()`，不复制业务实现。
#[derive(Debug, serde::Deserialize)]
struct ActionParityRequest {
    action_id: String,
    #[serde(default)]
    input: serde_json::Value,
    #[serde(default)]
    confirmed: bool,
    execution_id: Option<String>,
    surface: Option<String>,
}

#[tauri::command]
async fn action_parity_call(mut request: ActionParityRequest) -> serde_json::Value {
    let action_id = request.action_id.clone();
    let execution_id = request
        .execution_id
        .take()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(next_action_execution_id);
    let fallback_action_id = action_id.clone();
    let fallback_execution_id = execution_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        action_parity_call_inner(request, action_id, execution_id)
    })
    .await
    .unwrap_or_else(|error| {
        action_parity_error_envelope(
            &fallback_action_id,
            &fallback_execution_id,
            format!("runtime_join_error: {error}"),
        )
    })
}

fn action_parity_call_inner(
    request: ActionParityRequest,
    action_id: String,
    execution_id: String,
) -> serde_json::Value {
    let _surface = request.surface;
    let mut input = if request.input.is_null() {
        serde_json::json!({})
    } else {
        request.input
    };
    // 只有确认门 = `required` 的动作才在**业务入参**里收 `confirm`（`actions::readonly*` 建的
    // 动作压根没有这个字段，而它们的 schema 是 `additionalProperties:false`）。
    // 🔴 以前这里不看动作、一律塞：于是任何调用方只要说一句「我确认」，只读动作就被自己的
    // 入参校验判成 `invalid_input: 未知字段 confirm`。浏览器面板整块废掉就是这么来的 ——
    // 它的 `runAction` 默认 `confirmed = true`，open/click/type/press/scroll/back… 全军覆没，
    // 只有显式传 false 的 stream / snapshot 活着（所以画面停在「连接浏览器…」）。
    let takes_confirm_field = actions::describe(&action_id)
        .map(|spec| spec.confirmation == "required")
        .unwrap_or(false);
    if let Some(object) = input.as_object_mut() {
        if request.confirmed && takes_confirm_field {
            object.insert("confirm".into(), serde_json::Value::Bool(true));
        } else {
            // `confirmed` 是标准门禁；不能让调用方把 `confirm:true` 偷塞回兼容核心绕过它。
            object.remove("confirm");
        }
    }
    // `execution_id` 是 ActionParity 信封的一部分，不混进业务 input；付费动作从
    // actions::current_execution_id() 取它作为上游幂等键，普通动作完全不受影响。
    match actions::run_with_execution_id(&action_id, input, Some(&execution_id), &|_| {}) {
        Ok(result) => serde_json::json!({
            "ok": true,
            "version": 1,
            "action_id": action_id,
            "execution_id": execution_id,
            "result": result,
        }),
        Err(error) => action_parity_error_envelope(&action_id, &execution_id, error),
    }
}

fn action_parity_error_envelope(action_id: &str, execution_id: &str, message: String) -> serde_json::Value {
    let code = message.split(':').next().unwrap_or("runtime_error").trim();
    let class = match code {
        "confirmation_required" => "confirmation",
        "invalid_input" => "input",
        "conflict" => "conflict",
        "unknown_action" => "not_found",
        _ => "runtime",
    };
    serde_json::json!({
        "ok": false,
        "version": 1,
        "action_id": action_id,
        "execution_id": execution_id,
        "error": { "class": class, "code": code, "message": message }
    })
}

fn next_action_execution_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("uking-{millis:x}-{:x}-{sequence:x}", std::process::id())
}

/// 设备钱包同步真路（`sync_device_wallet_consumers`）的回归钉子。
///
/// 🔴 背景：2026-08-22 合并时删掉了 preview 那份实现（`refresh_key_where_routed_to_us`），
/// 但它的 3 个用例删掉前一直在绿 —— 而**真正发出去的真路一条用例都没有**。
/// 「我核过等价」不是凭据。这三个用例把真路的承诺直接钉死：
/// ① 在用工具拿到新 Key；② 客户自己配的账号一个字不动；③ **换 Key 不重置客户挑的模型**
/// （第③条是 2026-08-26 修掉的真缺陷：真路原来整体传 None，而 apply_* 对 None 的语义
/// 是「写 preset 默认」，客户挑的 glm-5 会被静默换回 deepseek-v4-flash）。
#[cfg(test)]
mod ai_checkup_tests {
    use super::*;

    #[test]
    fn recognizes_managed_and_customer_owned_configs_without_writing_them() {
        crate::testsandbox::with_sandbox("ai-checkup", &[".uking", ".qwen", ".config"], |root| {
            // 这里直接喂分类器的 installed=true：判定单元不需要篡改全进程 APPDATA/PATH，
            // 而生产入口仍只认 installer::tool_installed 的同一口径。
            std::fs::write(
                root.join(".qwen").join("settings.json"),
                r#"{"model":{"name":"glm-5"},"modelProviders":{"openai":[{"envKey":"UKING_MANAGED_API_KEY","baseUrl":"https://api.u-claw.org/v1"}]}}"#,
            ).unwrap();
            let qwen = effective_checkup_item("qwen", "Qwen Code", true, providers::effective_config("qwen"));
            assert_eq!(qwen.state, "ready");
            assert_eq!(qwen.model.as_deref(), Some("glm-5"));
            assert!(qwen.can_auto_fix);

            // Windows 上 Crush 优先跟随 APPDATA；沙箱不改进程级 APPDATA，避免与并行
            // 用例竞争，因此这里把回读层已经承诺的客户路由形状直接交给分类层。
            let crush = effective_checkup_item(
                "crush", "Crush", true,
                providers::EffectiveConfig {
                    target: "crush".into(), readable: true,
                    provider_key: Some("customer-relay".into()),
                    base_url: Some("https://relay.example.com/v1".into()),
                    model: Some("kimi-k3".into()), overridden_by: None,
                },
            );
            assert_eq!(crush.state, "self-managed");
            assert!(!crush.can_auto_fix, "客户中转不许显示自动接管");

            let opencode = effective_checkup_item("opencode", "OpenCode", true, providers::effective_config("opencode"));
            assert_eq!(opencode.state, "idle");
            assert!(opencode.can_auto_fix);

            let claude_dir = root.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            std::fs::write(claude_dir.join(".credentials.json"), r#"{"access_token":"oauth"}"#).unwrap();
            let claude = claude_checkup_item("Claude Code", true);
            assert_eq!(claude.state, "self-managed", "官方 OAuth 必须礼让");

            // 宿主机可能真的装了 CLI，未安装断言只验证体检项自身的安全形状。
            let absent = effective_checkup_item("qwen", "Qwen Code", false, providers::EffectiveConfig::default());
            assert_eq!(absent.state, "absent");
            assert!(!absent.can_auto_fix);

            // ── sol 终审 NO-GO 四条的回归钉 ──
            // ① 回读失败≠没配置：readable=false 时绝不给接管入口。
            let unreadable = effective_checkup_item(
                "pi", "pi", true,
                providers::EffectiveConfig { target: "pi".into(), readable: false, ..Default::default() },
            );
            assert_eq!(unreadable.state, "idle");
            assert!(!unreadable.can_auto_fix, "读不动配置=不知道，不许给接管按钮");
        });
    }

    /// 端点归属要按主机段精确判，contains 会把 evil-u-claw.org 认成自家的。
    #[test]
    fn xiapan_endpoint_matches_host_precisely() {
        for yes in [
            "https://api.u-claw.org/v1",
            "https://u-claw.org",
            "https://api.u-claw.org.cn/v1",
            "http://user:pass@api.u-claw.org:8080/x",
        ] {
            assert!(super::xiapan_endpoint(Some(yes)), "{yes} 应认成虾盘云");
        }
        for no in [
            Some("https://evil-u-claw.org/v1"),
            Some("https://u-claw.org.attacker.example"),
            Some("https://relay.example.com/v1"),
            None,
        ] {
            assert!(!super::xiapan_endpoint(no), "{no:?} 不该认成虾盘云");
        }
    }
}

#[cfg(test)]
mod device_wallet_sync_tests {
    use super::*;

    /// 客户自己在 ClawX 里建的账号 —— 形状抄自 pc-*** 的真实日志。
    const CUSTOMER_ACCOUNT: &str = "custom-custom51";

    fn write_customer_clawx_config(root: &std::path::Path) {
        let dir = root.join("ClawX");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("clawx-providers.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 2,
                "defaultProvider": CUSTOMER_ACCOUNT,
                "defaultProviderAccountId": CUSTOMER_ACCOUNT,
                "providerAccounts": {
                    CUSTOMER_ACCOUNT: {
                        "id": CUSTOMER_ACCOUNT,
                        "vendorId": "custom",
                        "label": "小米 MiMo",
                        "authMode": "api_key",
                        "baseUrl": "https://token-plan-cn.xiaomimimo.com/v1",
                        "model": "mimo-v2.5",
                        "enabled": true,
                        "isDefault": true,
                        "createdAt": "2026-08-21T00:00:00.000Z",
                        "updatedAt": "2026-08-21T00:00:00.000Z"
                    }
                },
                "apiKeys": { CUSTOMER_ACCOUNT: "sk-cus...-key" },
                "providerSecrets": {}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn read_clawx(root: &std::path::Path) -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(root.join("ClawX").join("clawx-providers.json")).unwrap(),
        )
        .unwrap()
    }

    /// 前置：客户先自己配了小米，U-King 再接管 clawx 且客户在下拉里挑了 glm-5
    /// （不是 preset 默认的 deepseek-v4-flash）—— 最坏形状下换 Key。
    fn setup_customer_then_uking_with_glm5(root: &std::path::Path) -> String {
        write_customer_clawx_config(root);
        let p = providers::wallet_sync_test_bridge::builtin_xiapan();
        let account_id = providers::wallet_sync_test_bridge::managed_id(&p);
        providers::wallet_sync_test_bridge::apply_clawx_as_uking(&p, "sk-old", "glm-5").unwrap();
        providers::wallet_sync_test_bridge::record_active("clawx", "xiapan");
        account_id
    }

    #[test]
    fn key_refresh_writes_ours_keeps_theirs_and_never_resets_model() {
        crate::testsandbox::with_sandbox(
            "wallet-sync-refresh",
            &[".uking", "ClawX", ".openclaw"],
            |root| {
                std::env::set_var("USERPROFILE", root);
                std::env::set_var("HOME", root);
                let account_id = setup_customer_then_uking_with_glm5(root);

                sync_device_wallet_consumers(Some("sk-bra...-new-key")).unwrap();

                let v = read_clawx(root);
                // ① 真路必须真的把新 Key 落地（记完账不落地 = pc-*** 那条 401）
                assert_eq!(
                    v["apiKeys"][&account_id].as_str(),
                    Some("sk-bra...-new-key"),
                    "仍归我们管的工具必须拿到新 Key"
                );
                // ② 客户挑的 glm-5 必须原样保留 —— 这条修的是「None=preset 默认」的重置病
                assert_eq!(
                    v["providerAccounts"][&account_id]["model"].as_str(),
                    Some("glm-5"),
                    "换 Key 不许把客户挑的模型重置回 preset 默认"
                );
                // ③ 客户自建的小米账号原样保留
                assert_eq!(
                    v["providerAccounts"][CUSTOMER_ACCOUNT]["baseUrl"].as_str(),
                    Some("https://token-plan-cn.xiaomimimo.com/v1"),
                    "换 Key 是凭证维护，不是路由决策 —— 客户自己的账号一个字不动"
                );
            },
        );
    }

    /// 🔴 四个无 DriverStatus 模型字段的工具（pi/opencode/qwen/crush）换 Key 时：
    /// ① 回读到的当前模型必须原样写回（不许重置成 preset 默认）；
    /// ② 客户自己把工具切到别家中转的，本轮整体跳过 —— 不许砸掉客户路由。
    #[test]
    fn key_refresh_preserves_readback_tools_and_skips_foreign_routes() {
        crate::testsandbox::with_sandbox(
            "wallet-sync-readback",
            &[".uking", ".qwen", ".config"],
            |root| {
                std::env::set_var("USERPROFILE", root);
                std::env::set_var("HOME", root);
                let p = providers::wallet_sync_test_bridge::builtin_xiapan();

                // 🔴 造假的 qwen / crush 可执行文件：tool_installed 先看 search_paths
                // 里文件在不在。不造的话这两个目标被「未安装」短路，用例测的是空转 ——
                // 变异验证已实锤（废掉跳过逻辑测试照绿）。
                let fake_bin = root.join("fakebin");
                std::fs::create_dir_all(&fake_bin).unwrap();
                for name in ["qwen", "crush"] {
                    std::fs::write(fake_bin.join(format!("{name}.cmd")), "@echo fake").unwrap();
                }
                std::env::set_var("APPDATA", root); // search_paths 会扫 %APPDATA%\npm
                std::fs::create_dir_all(root.join("npm")).unwrap();
                for name in ["qwen", "crush"] {
                    std::fs::write(root.join("npm").join(format!("{name}.cmd")), "@echo fake").unwrap();
                }

                // ── qwen：已由我们接管、客户挑了 glm-5 ──
                providers::wallet_sync_test_bridge::apply_qwen_as_uking(&p, "sk-old", "glm-5")
                    .unwrap();
                providers::wallet_sync_test_bridge::record_active("qwen", "xiapan");

                // ── crush：客户自己配了别家中转（非托管 provider + 非虾盘云端点）──
                let crush_dir = root.join(".config").join("crush");
                std::fs::create_dir_all(&crush_dir).unwrap();
                std::fs::write(
                    crush_dir.join("crush.json"),
                    r#"{"models":{"large":{"model":"kimi-k3","provider":"customer-relay"}},
                        "providers":{"customer-relay":{"type":"openai",
                        "base_url":"https://relay.example.com/v1","api_key":"sk-cx"}}}"#,
                )
                .unwrap();
                providers::wallet_sync_test_bridge::record_active("crush", "xiapan");

                sync_device_wallet_consumers(Some("sk-bra...-new-key")).unwrap();

                // ① qwen：模型保住 glm-5，Key 换新
                let q: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(root.join(".qwen").join("settings.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    q["model"]["name"].as_str(),
                    Some("glm-5"),
                    "回读工具换 Key 不许把当前模型重置成 preset 默认"
                );
                assert_eq!(
                    q["env"]["UKING_MANAGED_API_KEY"].as_str(),
                    Some("sk-bra...-new-key"),
                    "归我们管的 qwen 必须拿到新 Key"
                );

                // ② crush：别家路由原样不动
                let c: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(crush_dir.join("crush.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    c["models"]["large"]["model"].as_str(),
                    Some("kimi-k3"),
                    "客户自切别家中转的工具，换 Key 时整体跳过"
                );
                assert!(c.get("UKING_MANAGED_API_KEY").is_none());
            },
        );
    }

    #[test]
    fn wallet_clear_restores_default_back_to_customer_account() {
        crate::testsandbox::with_sandbox(
            "wallet-sync-clear",
            &[".uking", "ClawX", ".openclaw"],
            |root| {
                std::env::set_var("USERPROFILE", root);
                std::env::set_var("HOME", root);
                setup_customer_then_uking_with_glm5(root);

                sync_device_wallet_consumers(None).unwrap();

                let v = read_clawx(root);
                // 我们的托管账号被摘干净，默认指回落到还存在的客户账号
                assert!(
                    v["providerAccounts"]
                        .get("uking-xiapan")
                        .is_none_or(|x| x.is_null()),
                    "清钱包后不许留下指向已吊销 Key 的托管账号"
                );
                assert_eq!(
                    v["defaultProvider"].as_str(),
                    Some(CUSTOMER_ACCOUNT),
                    "默认供应商应还给客户自己配的那个"
                );
            },
        );
    }
}

#[cfg(test)]
mod action_parity_adapter_tests {
    use super::*;

    /// **回归（0.9.99）**：契约对外声明的 `targets` 必须覆盖后端真会配的全部工具。
    ///
    /// 2026-08-03 上架 pi/qwen/crush/opencode 之后，后端分派表有 8 个、
    /// 契约 enum 停在 4 个、前端弹窗也停在 4 个 —— 同一个事实存了三份，漂了三份。
    /// 对 CLI / MCP / 远端影子来说，那四个工具至今「未声明支持」：
    /// 照着 manifest 干活的 AI 会认为配不了它们，而实际上后端一直配得了。
    ///
    /// 这条用例守的是**清单之间的关系**，不是某一份清单的内容 ——
    /// 加第 9 个工具时，改了后端没改契约（或反过来）都会在这里当场红。
    #[test]
    fn apply_everywhere_contract_lists_every_target_the_backend_configures() {
        let table = action_table();
        let spec = table
            .iter()
            .map(|a| &a.spec)
            .find(|s| s.id == actions::DRIVER_APPLY_EVERYWHERE)
            .expect("组合根里应当登记着 driver.apply_everywhere");
        let schema = spec.input_schema.as_ref().expect("写动作必须有入参契约");
        let declared: Vec<&str> = schema["properties"]["targets"]["items"]["enum"]
            .as_array()
            .expect("targets.items 上必须有 enum —— 没有 enum 就是「什么都收」")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            declared,
            providers::APPLY_ALL_TARGETS,
            "契约里的 targets 跟 APPLY_ALL_TARGETS 漂了；enum 必须从那个常量生成，别手抄"
        );
        // 那份常量本身 = 有独立列表偏好的四件套 + 后上架那批。三份清单的关系钉在这儿，
        // 谁单方面改了都会红（LIST_TOOLS 和 APPLY_ALL_TARGETS 是两个事实，只是前缀相同）。
        let want: Vec<&str> = providers::LIST_TOOLS
            .iter()
            .chain(providers::EXTRA_APPLY_TOOLS.iter())
            .copied()
            .collect();
        assert_eq!(
            providers::APPLY_ALL_TARGETS.to_vec(),
            want,
            "APPLY_ALL_TARGETS 应当正好是 LIST_TOOLS + EXTRA_APPLY_TOOLS"
        );
    }

    /// F7（2026-08-22）：优化大师的「动手」那一半升格成动作时，**契约里那个 enum 是唯一的闸门**。
    ///
    /// 守两件事，缺一不可：
    /// 1. `undo` 不在 enum 里 —— `ukrt undo` 每调一次剥一层 journal，登记成动作等于
    ///    对外承诺一个不兑现的幂等（宪法：声明一个不兑现的字段比不声明更坏，重试会双写）。
    /// 2. enum 正好等于 `is_mutating_optimize` 认的那三个 —— 这两份清单是同一个事实的两份副本，
    ///    哪天有人加了第四个前向动作只改一边，就在这里当场红（第 8 条）。
    ///
    /// 🔴 用例只能碰 `undo` 这条**被拒的**路径：`fix` / `optimize` 真会改开发机的注册表和 PATH。
    #[test]
    fn optimizer_apply_offers_exactly_the_idempotent_forward_actions() {
        let table = action_table();
        let spec = table
            .iter()
            .map(|a| &a.spec)
            .find(|s| s.id == actions::OPTIMIZER_APPLY)
            .expect("组合根里应当登记着 optimizer.apply");
        let declared: Vec<&str> = spec.input_schema.as_ref().expect("写动作必须有入参契约")
            ["properties"]["action"]["enum"]
            .as_array()
            .expect("action 上必须有 enum —— 没有 enum 就是「什么都收」")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            !declared.contains(&"undo"),
            "undo 不许进 enum：它每调一次回退一层 journal，不是幂等的写"
        );
        assert!(
            declared.iter().all(|a| is_mutating_optimize(a)),
            "enum 里出现了 is_mutating_optimize 不认的动作：{declared:?}"
        );
        for a in ["fix", "optimize", "defender"] {
            assert!(
                declared.contains(&a),
                "is_mutating_optimize 认 {a}，enum 里却没有 —— 两份清单漂了"
            );
        }
        // enum 真的被执行（0.9.99 之前它是装饰）。走的是**被拒**那条路，不会动这台机器。
        let rejected = actions::run(
            actions::OPTIMIZER_APPLY,
            serde_json::json!({ "action": "undo", "confirm": true }),
        );
        assert!(
            rejected.is_err(),
            "enum 外的 action 必须被入参校验挡下，而不是落到 handler 里真跑一次 ukrt undo"
        );
    }

    /// 装缺件那条也一样：确认是**核心**在强制，不是 GUI 的礼貌。
    /// 没有 `confirm` 的调用（CLI 漏了 --yes、AI 直接调 MCP）必须在跑起来之前就被挡回去 ——
    /// 它跑起来是 ~106MB 下载 + 改用户 PATH，等 handler 里再判就晚了。
    #[test]
    fn env_install_tools_refuses_to_run_without_confirmation() {
        let table = action_table();
        let spec = table
            .iter()
            .map(|a| &a.spec)
            .find(|s| s.id == actions::ENV_INSTALL_TOOLS)
            .expect("组合根里应当登记着 env.install_tools");
        assert_eq!(spec.effect, "write");
        assert_eq!(spec.confirmation, "required");
        assert!(spec.progress_events, "几分钟的下载声明成「无进度」= 让 UI 只能干等转圈");
        let denied = actions::run(actions::ENV_INSTALL_TOOLS, serde_json::json!({}));
        assert!(
            denied.is_err(),
            "缺 confirm 必须被核心挡下 —— 它一跑就是几十上百 MB 下载 + 改 PATH"
        );
    }

    /// 浏览器运行时同样是写动作：没有确认时必须在进入 npm 前被 ActionParity 核心挡住。
    #[test]
    fn browser_runtime_install_refuses_to_run_without_confirmation() {
        let table = action_table();
        let spec = table
            .iter()
            .map(|a| &a.spec)
            .find(|s| s.id == actions::BROWSER_RUNTIME_INSTALL)
            .expect("组合根里应当登记 browser runtime install");
        assert_eq!(spec.effect, "write");
        assert_eq!(spec.confirmation, "required");
        assert!(spec.progress_events);
        assert!(spec.idempotent);
        assert_eq!(
            spec.output_schema.as_ref().and_then(|schema| schema["required"].as_array()).map(|items| items.iter().any(|v| v == "changed")),
            Some(true),
            "调用方必须能区分已就绪的零下载路径"
        );
        let denied = actions::run(actions::BROWSER_RUNTIME_INSTALL, serde_json::json!({}));
        assert!(denied.is_err(), "缺 confirm 不得启动 npm 安装");
    }

    #[test]
    fn browser_runtime_ready_preflight_skips_npm_install() {
        let ready = browser_runtime_install_decision(Ok(serde_json::json!({
            "version": "0.27.0",
            "stream": { "ws_url": "ws://127.0.0.1:53535" },
            "snapshot": { "snapshot": "- document" },
        })))
        .expect("已就绪预检不应报错")
        .expect("已就绪必须选择零下载路径");
        assert_eq!(ready["changed"], false);
        assert_eq!(ready["version"], "0.27.0");
        assert!(browser_runtime_install_decision(Err("not_installed: demo".into())).unwrap().is_none());
        assert!(browser_runtime_install_decision(Err("version_mismatch: demo".into())).unwrap().is_none());
        assert!(browser_runtime_install_decision(Err("browser.stream: Chrome 未就绪".into())).is_err());
    }

    #[test]
    fn generated_client_request_reaches_the_existing_core_with_the_same_execution_id() {
        let request = ActionParityRequest {
            action_id: actions::COMMAND_GUARD_INSPECT.into(),
            input: serde_json::json!({}),
            confirmed: false,
            execution_id: Some("test-execution-1".into()),
            surface: Some("desktop".into()),
        };
        let envelope = action_parity_call_inner(
            request,
            actions::COMMAND_GUARD_INSPECT.into(),
            "test-execution-1".into(),
        );
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["action_id"], actions::COMMAND_GUARD_INSPECT);
        assert_eq!(envelope["execution_id"], "test-execution-1");
        assert!(envelope.get("result").is_some());
    }

    /// 🔴 说一句「我确认」不该把只读动作打死。
    ///
    /// 上一条用例只走了 `confirmed: false`，于是这个 bug 从跑道底下整块钻过去了：适配器
    /// 不看动作要不要确认就往入参里塞 `confirm`，而 `readonly*` 动作的 schema 是
    /// `additionalProperties:false` → 自己的校验判自己 `invalid_input: 未知字段 confirm`。
    /// 浏览器面板（`runAction` 默认 `confirmed = true`）因此 open/click/type/press/scroll 全废。
    #[test]
    fn confirmed_flag_does_not_break_actions_that_need_no_confirmation() {
        let request = ActionParityRequest {
            action_id: actions::COMMAND_GUARD_INSPECT.into(),
            input: serde_json::json!({}),
            confirmed: true,
            execution_id: Some("test-execution-confirmed".into()),
            surface: Some("desktop".into()),
        };
        let envelope = action_parity_call_inner(
            request,
            actions::COMMAND_GUARD_INSPECT.into(),
            "test-execution-confirmed".into(),
        );
        assert_eq!(
            envelope["ok"], true,
            "只读动作收到 confirmed=true 被判错了：{}",
            envelope["error"]["message"]
        );
    }

    #[test]
    fn standard_confirmation_cannot_be_bypassed_through_legacy_input() {
        let request = ActionParityRequest {
            action_id: actions::DESKTOP_PIN.into(),
            input: serde_json::json!({ "confirm": true }),
            confirmed: false,
            execution_id: Some("test-execution-2".into()),
            surface: Some("desktop".into()),
        };
        let envelope = action_parity_call_inner(
            request,
            actions::DESKTOP_PIN.into(),
            "test-execution-2".into(),
        );
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["error"]["code"], "confirmation_required");
    }
}

/// 「AI 优化大师」云端提示层：拉服务器 optimize-advice.json（version 门控 + 内嵌兜底），本机跑
/// 只读白名单探针，返回**当前机器命中**的最新提示（如 Codex 串口崩溃 / TEMP 变慢）。改服务器 JSON
/// 即可热更新到全客户机，不发 exe。网络失败静默回落内嵌版；失败也不拦（返回空）。
#[tauri::command]
async fn fetch_optimize_advice() -> Vec<advice::Advice> {
    tauri::async_runtime::spawn_blocking(advice::collect).await.unwrap_or_default()
}

/// 卸载 U-King：注销右键菜单 + 清 PATH/快捷方式 + 安排退出后删 ~/.uking，然后退出本进程。
/// **只删 U-King 自己装的东西**，绝不碰 ~/.claude / ~/.codex / ~/.openclaw 与用户装的 AI 工具。
/// 前端须先强确认再调。返回 Ok 表示清理已就位、app 即将退出（延迟脚本随后删 ~/.uking）。
#[tauri::command]
async fn uninstall_uking(app: AppHandle) -> Result<(), String> {
    // 右键菜单在 HKCU，注销走 context_menu（best-effort，失败不拦卸载）
    let _ = context_menu::unregister();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        uninstall::run(&|msg: &str| {
            let _ = app2.emit("uking:uninstall_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    // 清理脚本已就位 → 稍后退出本进程（用 process::exit 绕过 prevent_close），脚本随即删 ~/.uking
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(400));
        // 绕过 prevent_close 就绕过了 RunEvent::Exit，得自己销账，否则这次「用户主动卸载」
        // 会在下次启动时被读成一次崩溃。
        crashlog::end_session();
        std::process::exit(0);
    });
    Ok(())
}

/// 「安全卸载 / 逐项清理」扫描：诚实列出本机上 U-King 的**全部足迹**（core/config/aitool 三档）。
/// 只读，不改任何东西——前端据此渲染勾选清单。
/// 薄壳，真身是影核动作 `runtime.footprint.inspect`（动作里包成 {items,count}，这里还原成数组）。
#[tauri::command]
async fn cleanup_scan() -> serde_json::Value {
    action_field(run_action_blocking(actions::FOOTPRINT_INSPECT).await, "items", serde_json::json!([]))
}

/// 「安全卸载 / 逐项清理」执行：按前端勾选的 `ids` 逐项删除/还原，进度走事件 `uking:cleanup_progress`。
/// `preserve_user_data=true` 时，先把 U-King/OpenClaw/ClawX/Hermes/Codex 桌面版的历史资料备份到
/// 「文档/U-King 演示保留数据」，再清场；下一次安装仍从干净状态开始。
/// 若 `ids` 含 `uking-home` → 视为「彻底卸载」：先删完其余项，再注销右键菜单 + 清 PATH/快捷方式 +
/// 安排退出后删 `~/.uking`，最后退出本进程。返回 `true` 表示 app 即将关闭（前端据此提示）。
#[tauri::command]
async fn cleanup_run(
    app: AppHandle,
    ids: Vec<String>,
    preserve_user_data: Option<bool>,
) -> Result<bool, String> {
    let v = run_write_action_progress(
        app,
        actions::FOOTPRINT_REMOVE,
        "uking:cleanup_progress",
        serde_json::json!({ "ids": ids, "preserve_user_data": preserve_user_data.unwrap_or(false) }),
    )
    .await?;
    let will_exit = action_field(v, "will_exit", serde_json::json!(false)).as_bool().unwrap_or(false);
    // 进程退出的编排留在 command 层：这是 app 生命周期，不是业务动作。
    // 无头 `action run` 调同一个动作时不会走到这里 —— CLI 本来就要退出。
    if will_exit {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(600));
            crashlog::end_session(); // 同上：主动退出得销账，别记成崩溃
            std::process::exit(0);
        });
    }
    Ok(will_exit)
}

/// 首页 AI 卡片「卸载」：彻底删掉某个 AI 工具本体 + 一切会被探测成"已装"的残留
/// （npm 包/stub、`~/.uking/tools/<x>`、`~/.uclaw`、shims、GUI app 走官方卸载程序），
/// 修「删了还检测到、重装 U-King 又冒出来」。进度走事件 `uking:uninstall_progress`。
#[tauri::command]
async fn uninstall_ai_tool(app: AppHandle, tool_id: String) -> Result<String, String> {
    let v = run_write_action_progress(
        app,
        actions::AITOOL_UNINSTALL,
        "uking:uninstall_progress",
        serde_json::json!({ "tool_id": tool_id }),
    )
    .await?;
    Ok(action_field(v, "message", serde_json::Value::Null).as_str().unwrap_or("").to_string())
}

/// 技术支持：采集一份**已脱敏**的诊断文本（版本/系统/设备Key前缀/装机状态/日志尾部），
/// 供反馈页展示 · 复制 · 随反馈一起上报。只读，不改任何东西。
/// 薄壳，真身是影核动作 `runtime.diagnostics.collect`。
#[tauri::command]
async fn collect_diagnostics() -> String {
    action_field(run_action_blocking(actions::DIAGNOSTICS_COLLECT).await, "text", serde_json::Value::Null)
        .as_str()
        .unwrap_or("（诊断采集失败）")
        .to_string()
}

/// 技术支持「一键提交」：把用户文字（脱敏）+（可选）诊断，走服务端上报（建 Issue）。同步返回结果。
#[tauri::command]
async fn submit_feedback(
    message: String,
    include_diagnostics: bool,
    shots: Option<Vec<String>>,
    // 只有用户在反馈页勾了「同意上传截图」，前端才会填这个字段（压缩后的 JPEG base64）。
    shot_data: Option<Vec<String>>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        feedback::submit_feedback(
            &message,
            include_diagnostics,
            &shots.unwrap_or_default(),
            &shot_data.unwrap_or_default(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 打开日志所在文件夹（方便用户把日志附到邮件里发反馈）。
#[tauri::command]
async fn open_log_dir() -> Result<(), String> {
    feedback::open_log_dir()
}

// ── 远程协助（remote_assist.rs）────────────────────────────────────────────────
// 独立可插拔：删这个功能只动本文件（去 mod remote_assist + 下面 4 个 command + invoke 注册）
// 与 `Feedback.tsx`（去一个区块）。模块本身不碰 AppHandle，进度用 |msg| 回调传出，这里再 emit。

/// 远程协助当前状态（有没有开、协助编号、还剩多久自动断、审计日志在哪）。只读。
#[tauri::command]
async fn remote_assist_status() -> remote_assist::AssistStatus {
    tauri::async_runtime::spawn_blocking(remote_assist::status)
        .await
        .unwrap_or_default()
}

/// 开启远程协助。**必须由客户在界面上主动点**——这一层是 GUI 的礼貌，真正的门在于
/// 不点就不下载、不启动、不连服务器，作者那边压根看不到这台机器。
/// 下载/连接进度走事件 `uking:remote_assist`。
#[tauri::command]
async fn remote_assist_start(app: AppHandle) -> Result<remote_assist::AssistStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        remote_assist::start(&|msg: &str| {
            let _ = app.emit("uking:remote_assist", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("远程协助任务异常: {e}"))?
}

/// 停止远程协助（只杀我们自己起的那个进程，见 remote_assist::stop 的注释）。幂等。
#[tauri::command]
async fn remote_assist_stop() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(remote_assist::stop)
        .await
        .map_err(|e| format!("远程协助任务异常: {e}"))?
}

/// 打开审计日志目录 —— 客户可以自己核对我们远程执行过哪些命令。
#[tauri::command]
async fn remote_assist_open_audit() -> Result<(), String> {
    remote_assist::reveal_audit_dir()
}

/// 反馈页粘贴的截图落盘（data URL → `~/.uking/feedback/`），返回本机路径。
/// 图不进 Issue（body 8000 字上限塞不下 base64），留本机 + 反馈正文注明，作者按需索取。
#[tauri::command]
async fn save_feedback_shot(data_url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || feedback::save_shot(&data_url))
        .await
        .map_err(|e| e.to_string())?
}

/// 打开截图文件夹（用户把图拖进邮件发给作者）。
#[tauri::command]
async fn open_feedback_shots_dir() -> Result<(), String> {
    feedback::open_shots_dir()
}

/// 「本地大模型」板块：硬件体检 + 推荐档位（这台机器能跑多大的模型）。
/// 薄壳，真身是影核动作 `runtime.hardware.inspect`。
#[tauri::command]
async fn detect_hardware() -> serde_json::Value {
    run_action_blocking(actions::HARDWARE_INSPECT).await
}

/// Token 压缩机（RTK）状态：装没装 / 开没开 / 累计省了多少 token。
/// 薄壳，真身是影核动作 `runtime.rtk.inspect`。
#[tauri::command]
async fn rtk_status() -> serde_json::Value {
    run_action_blocking(actions::RTK_INSPECT).await
}

// ───────────────────── 本地大模型（四引擎，全是薄壳）─────────────────────

/// 本地引擎在「AI 设置」里的驱动 id。固定前缀 = 起停可 upsert / 可撤下，
/// 也一眼看得出这条是我们代管的，不是客户自己加的。
fn local_provider_id(engine: &str) -> String {
    format!("local-{engine}")
}

/// 把「正在跑的本地引擎」登记成一条可选驱动，让它出现在「AI 设置」里，
/// 也就能被一键配进 ClawX / Hermes —— 本地模型不接进配置链路的话，
/// 客户跑起来了也只能自己去改配置文件，等于没做完。
///
/// 🔴 `anthropic_base` 必须留空：这些引擎**只有** OpenAI 兼容端点。填个假的进去，
/// Claude Code 会被配成一个永远 404 的地址，而界面上显示「已配好」。
fn register_local_provider(engine: &str, endpoint: &str, model: &str) -> serde_json::Value {
    let label = match engine {
        "ollama" => "Ollama",
        "llamacpp" => "llama.cpp",
        "vllm" => "vLLM",
        "sglang" => "SGLang",
        other => other,
    };
    // 模型名给个能用的默认：llama.cpp/vLLM 传的是文件或目录路径，取最后一段当模型名。
    let model_name = model
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let preset = providers::ProviderPreset {
        id: local_provider_id(engine),
        name: format!("本地 · {label}"),
        summary: "本机跑的开源模型：离线、免费、数据不出本机。只有 OpenAI 兼容端点，Claude Code / Codex 用不了，配进 ClawX 或 Hermes".into(),
        openai_base: endpoint.to_string(),
        anthropic_base: None,
        model: if model_name.is_empty() { "local-model".into() } else { model_name.clone() },
        small_model: String::new(),
        // Codex 只认 /v1/responses，本地引擎一个都不提供 —— 留空，别给客户端一个会 404 的模型名。
        codex_model: String::new(),
        codex_wire_api: "responses".into(),
        key_url: String::new(),
        key_hint: "本地服务不需要 Key".into(),
        builtin_recharge: false,
        recommended: false,
        builtin: false,
        // 本地服务不校验 Key，但很多客户端要求非空，给一个显然是占位的值。
        api_key: "local".into(),
    };
    match providers::save_custom_provider(preset) {
        Ok(p) => serde_json::json!({ "id": p.id, "name": p.name, "openai_base": p.openai_base }),
        Err(e) => serde_json::json!({ "error": e }),
    }
}

/// 四个引擎一次给全：装没装 / 能不能用 / 卡在哪 / 在跑哪个模型。
#[tauri::command]
async fn localllm_inspect() -> serde_json::Value {
    run_action_blocking(actions::LOCALLLM_INSPECT).await
}

#[tauri::command]
async fn localllm_start(
    engine: String,
    model: Option<String>,
    port: Option<u16>,
    ctx: Option<u32>,
    gpu_layers: Option<i32>,
    threads: Option<u32>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({ "engine": engine });
    if let Some(m) = model.filter(|s| !s.trim().is_empty()) {
        input["model"] = serde_json::Value::String(m);
    }
    if let Some(p) = port.filter(|p| *p >= 1024) {
        input["port"] = serde_json::Value::from(p);
    }
    if let Some(c) = ctx {
        input["ctx"] = serde_json::Value::from(c);
    }
    if let Some(g) = gpu_layers {
        input["gpu_layers"] = serde_json::Value::from(g);
    }
    if let Some(t) = threads {
        input["threads"] = serde_json::Value::from(t);
    }
    run_write_action(actions::LOCALLLM_START, input).await
}

/// 商店货架（可选带一个 model_id 拿它的真实量化清单）。
#[tauri::command]
async fn localllm_catalog(
    model_id: Option<String>,
    refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({});
    if let Some(id) = model_id.filter(|s| !s.trim().is_empty()) {
        input["model_id"] = serde_json::Value::String(id);
    }
    if refresh.unwrap_or(false) {
        input["refresh"] = serde_json::Value::Bool(true);
    }
    // 这条只读动作会失败（模型站连不上、仓库改名），所以走 Result 而不是
    // run_action_input 那条「失败塞进 error 字段」的路 —— 界面要能把原因原样说给客户。
    tauri::async_runtime::spawn_blocking(move || actions::run(actions::LOCALLLM_CATALOG, input))
        .await
        .map_err(|e| format!("动作执行任务异常: {e}"))?
}

/// 下模型。**几十 GB、可能跑几小时**，所以进度实时打到界面上 ——
/// 一个没有进度的长下载，在客户眼里跟死机没有区别。
#[tauri::command]
async fn localllm_download(
    app: tauri::AppHandle,
    model_id: String,
    quant: String,
    dir: Option<String>,
) -> Result<serde_json::Value, String> {
    if let Some(d) = dir.filter(|s| !s.trim().is_empty()) {
        tauri::async_runtime::spawn_blocking(move || localllm::set_download_dir(&d))
            .await
            .map_err(|e| e.to_string())??;
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = localllm::download_model(&model_id, &quant, &move |msg: &str| {
            let _ = app2.emit("uking:localllm_progress", msg.to_string());
        })?;
        Ok::<_, String>(serde_json::json!({ "ok": true, "path": path }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 取消下载。半截文件留着，下次续传 —— 别把「取消」做成「白下了 8 GB」。
#[tauri::command]
async fn localllm_download_cancel() -> String {
    localllm::download_cancel()
}

/// 改模型下载落点（几十 GB，C 盘通常放不下）。
#[tauri::command]
async fn localllm_set_download_dir(dir: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || localllm::set_download_dir(&dir))
        .await
        .map_err(|e| e.to_string())?
}

/// 存运行参数（不启动）。界面上调完就存，别等到点启动才落盘。
#[tauri::command]
async fn localllm_save_settings(
    engine: String,
    port: u16,
    ctx: u32,
    gpu_layers: i32,
    threads: u32,
) -> Result<localllm::RunSettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        localllm::save_settings(&engine, localllm::RunSettings { port, ctx, gpu_layers, threads })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn localllm_stop(engine: String) -> Result<serde_json::Value, String> {
    run_write_action(actions::LOCALLLM_STOP, serde_json::json!({ "engine": engine })).await
}

#[tauri::command]
async fn localllm_install(
    engine: String,
    variant: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({ "engine": engine });
    if let Some(v) = variant.filter(|s| !s.trim().is_empty()) {
        input["variant"] = serde_json::Value::String(v);
    }
    run_write_action(actions::LOCALLLM_INSTALL, input).await
}

#[tauri::command]
async fn localllm_model_add(
    kind: String,
    path: String,
    name: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({ "kind": kind, "path": path });
    if let Some(n) = name.filter(|s| !s.trim().is_empty()) {
        input["name"] = serde_json::Value::String(n);
    }
    run_write_action(actions::LOCALLLM_MODEL_ADD, input).await
}

/// 引擎日志尾巴。**不进动作表**：它不改这台机器的任何状态，是给界面看的一眼窗口。
/// 模型加载慢/失败时，客户唯一能看的就是这个。
#[tauri::command]
async fn localllm_logs(engine: String, lines: Option<usize>) -> String {
    tauri::async_runtime::spawn_blocking(move || localllm::logs(&engine, lines.unwrap_or(200)))
        .await
        .unwrap_or_default()
}

// ───────────────────── 身份 / 给 AI 的说明书（全是薄壳）─────────────────────
//
// GUI 点按钮 = 用户显式确认，所以薄壳补 `confirm:true`；**判断在核心不在薄壳**——
// 从 CLI / MCP / 远端影子进来的一样会被门禁挡。

/// 身份 + 说明书状态（只读）。
#[tauri::command]
async fn identity_status() -> serde_json::Value {
    run_action_blocking(actions::IDENTITY_INSPECT).await
}

/// 保存身份（顺带重编说明书）。
#[tauri::command]
async fn save_identity(patch: serde_json::Value) -> Result<serde_json::Value, String> {
    run_write_action(actions::IDENTITY_SAVE, patch).await
}

/// 一轮对话的花费（¥）—— 对话里那行「这轮花了多少」用它。
///
/// 🔴 **为什么不用上游 CLI 自己报的 `cost_usd`**：那是按它认得的那家官方价算的。
/// 客户走虾盘云跑 `deepseek-v4-flash` 时，Claude Code 拿 Anthropic 的价目表算，
/// 出来要么是 0（不认识这个模型名）要么离谱 —— 显示一个跟真实扣费无关的数字，
/// 比不显示更坏：客户会拿它去对账，然后不信任我们所有的数字。
///
/// 这里用的是**水电表那份唯一价表**（`usage_local::price_per_million`），
/// 所以「这轮花了 ¥x」和「这个月花了 ¥y」是同一个口径，加得起来。
/// 缓存读按输入价的 1/10 计（各家都远低于全价，取一个保守的统一折扣，宁可估高不估低）。
#[tauri::command]
fn chat_cost_cny(model: String, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
    let (pin, pout) = usage_local::price_per_million(&resolve_priced_model(&model));
    let m = 1_000_000.0;
    (input as f64 * pin + cache_write as f64 * pin + cache_read as f64 * pin * 0.1 + output as f64 * pout) / m
}

/// 把「界面上选的那个」翻成**能查到价的模型名**。
///
/// 🔴 为什么需要这一步（2026-08-18 客户问「一个你好 0.11 元是真的吗」时查出来的）：
/// 前端传的是 `model || agent` —— 而模型下拉的**默认值是空**（「模型：跟随驱动设置」），
/// 于是绝大多数用户传进来的其实是 `"claude"` / `"codex"` 这种**大脑名，不是模型名**。
/// `price_per_million("claude")` 一个分支都不匹配 → 落到兜底 15/60，
/// 而他真正在跑的若是虾盘云的 deepseek（2/8），**这一路一直高估 7.5 倍**。
///
/// 修法是去问「这个大脑现在配的是哪个模型」（用户自己在 AI 设置里配的那个），
/// 而不是拿大脑名去猜价。查不到才退到各自的官方默认族 —— 那时至少猜的是对的那一家。
fn resolve_priced_model(model: &str) -> String {
    let m = model.trim();
    let is_engine_name = m.is_empty() || m == "claude" || m == "codex" || m == "claude-cli";
    if !is_engine_name {
        return m.to_string();
    }
    let st = providers::driver_status();
    let configured = if m == "codex" { st.codex_model.as_deref() } else { st.claude_model.as_deref() };
    if let Some(c) = configured.filter(|s| !s.trim().is_empty()) {
        return c.to_string();
    }
    // 没配 = 走官方直连的默认模型。**这时候猜的是对的那一家**：
    // Claude Code 默认 sonnet 族、Codex 默认 gpt-5 族，比落到「未知模型 15/60」准得多。
    if m == "codex" { "gpt-5".into() } else { "sonnet".into() }
}

/// 手动重编说明书 —— 升级完 U-King（动作表变了）点一下就跟上。
#[tauri::command]
async fn publish_identity() -> Result<serde_json::Value, String> {
    run_write_action(actions::IDENTITY_PUBLISH, serde_json::json!({})).await
}

/// 挂 / 撤 指针，让别家 AI 发现我们。
#[tauri::command]
async fn link_identity(linked: bool, targets: Vec<String>) -> Result<serde_json::Value, String> {
    run_write_action(actions::IDENTITY_LINK, serde_json::json!({ "linked": linked, "targets": targets })).await
}

/// 写一条凭据。`value` 空串 = 删除。
#[tauri::command]
async fn set_identity_secret(name: String, value: String) -> Result<serde_json::Value, String> {
    run_write_action(actions::IDENTITY_SECRET_SET, serde_json::json!({ "name": name, "value": value })).await
}

/// 换掉本机的虾盘云访问凭证（余额平移，旧凭证当场吊销）。
#[tauri::command]
async fn rotate_device_key() -> Result<serde_json::Value, String> {
    run_write_action(actions::DEVICE_KEY_ROTATE, serde_json::json!({})).await
}

/// 填入一把已有的访问密钥（换电脑 / 多副本共用 / 网站生成的那把）。
#[tauri::command]
async fn adopt_device_key(key: String) -> Result<serde_json::Value, String> {
    run_write_action(actions::DEVICE_KEY_ADOPT, serde_json::json!({ "key": key })).await
}

/// 从本机移除设备钱包；服务端钱包、旧 Key 和余额不删除。
#[tauri::command]
async fn reset_device_wallet() -> Result<serde_json::Value, String> {
    run_write_action(actions::DEVICE_WALLET_RESET_LOCAL, serde_json::json!({})).await
}

/// 读说明书正文给界面预览 —— 让用户**亲眼看到**外围 AI 会读到什么。
/// 这页卖的是信任：说「不会泄漏你的 Key」不如让他自己翻一遍。
#[tauri::command]
async fn read_llms_doc(full: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = if full { identity::llms_full_path() } else { identity::llms_path() };
        std::fs::read_to_string(&p).map_err(|e| format!("读 {} 失败: {e}", p.display()))
    })
    .await
    .map_err(|e| format!("读取任务异常: {e}"))?
}

/// Token 压缩机现场演示：当场跑真 rtk，返回压缩前后对比。
/// 薄壳，真身是影核动作 `runtime.rtk.demo`。
#[tauri::command]
async fn rtk_demo() -> serde_json::Value {
    run_action_blocking(actions::RTK_DEMO).await
}

/// 安装 Token 压缩机（下载 rtk.exe + 解压，进度走事件 `uking:rtk_progress`）。
#[tauri::command]
async fn rtk_install(app: AppHandle) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        rtk::install(&move |msg: &str| {
            let _ = app2.emit("uking:rtk_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("安装 Token 压缩机异常: {e}"))?
}

/// 开/关 Token 压缩机（往 ~/.claude/settings.json 合并/删我们的 hook；重启 Claude Code 生效）。
#[tauri::command]
async fn rtk_set_enabled(enabled: bool) -> Result<String, String> {
    let v = run_write_action(actions::RTK_SET_ENABLED, serde_json::json!({ "enabled": enabled })).await?;
    Ok(action_field(v, "message", serde_json::Value::Null).as_str().unwrap_or("").to_string())
}

/// 卸载 Token 压缩机（关 hook + 删 rtk.exe，绝不动用户其它配置）。
#[tauri::command]
async fn rtk_uninstall() -> Result<String, String> {
    let v = run_write_action(actions::RTK_UNINSTALL, serde_json::json!({})).await?;
    Ok(action_field(v, "message", serde_json::Value::Null).as_str().unwrap_or("").to_string())
}

/// 竞技场：六个 CLI 同任务横向比（claude / codex / hermes / pi / qwen / crush）。
///
/// 一跑就烧 token 且非幂等，**不进影核动作表**（跟「立即运行一次」同类）—— 前端点
/// 「开赛」就是显式确认。每个参赛者在 `workspace/arena/<tool>/` 独立副本里跑，互不干扰。
/// 返回**可观测量**（耗时/退出码/有没有产出），质量由前端让人打星。
#[tauri::command]
async fn arena_run(
    app: AppHandle,
    task: String,
    workspace: String,
    only: Option<String>,
) -> Result<serde_json::Value, String> {
    let task2 = task.clone();
    let ws = workspace.clone();
    let o = only.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 🔴 **空 / "." 不许当工作目录用。**
        // 前端在没有活动任务时会传 "."，那是 app 的 CWD —— 对已安装的版本就是
        // `%LOCALAPPDATA%\u-king\`。六个 CLI 在那儿真跑真写文件，客户根本找不到产出，
        // 表现就是「点了开赛，好像什么也没发生」。落到一个**可预期、找得到**的地方。
        let trimmed = ws.trim();
        let ws_path = if trimmed.is_empty() || trimmed == "." {
            crate::installer::user_home_dir().join(".uking").join("arena")
        } else {
            std::path::PathBuf::from(trimmed)
        };
        let _ = std::fs::create_dir_all(&ws_path);
        // 把真正用的目录回给前端 ——「跑在哪」必须说出来，否则用户找不到产出
        // 只会以为功能坏了（同一条：不许静默）。
        let _ = app2.emit(
            "uking:arena_progress",
            format!("工作副本根目录：{}", ws_path.display()),
        );
        let results = arena::run_arena(&task2, &ws_path, o.as_deref(), &|m| {
            let _ = app2.emit("uking:arena_progress", m.to_string());
        });
        let json = serde_json::to_value(&results).unwrap_or_else(|_| serde_json::json!([]));
        let _ = app.emit("uking:arena_done", json.clone());
        json
    })
    .await
    .map_err(|e| format!("竞技场调度失败: {e}"))
}

/// 厨具工具箱：全部能力工具 + 已装状态（ffmpeg / Chrome / PowerShell 7 / Python …）。
/// 薄壳，真身是影核动作 `runtime.toolbox.inspect`。
#[tauri::command]
async fn list_capability_tools() -> serde_json::Value {
    action_field(run_action_blocking(actions::TOOLBOX_INSPECT).await, "items", serde_json::json!([]))
}

/// 一键装一个厨具（走 winget/brew，进度走事件 `uking:toolbox_progress`）。
#[tauri::command]
async fn install_capability_tool(app: AppHandle, id: String) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        toolbox::install_tool(&id, &move |msg: &str| {
            let _ = app2.emit("uking:toolbox_progress", msg.to_string());
        })
    })
    .await
    .map_err(|e| format!("安装厨具异常: {e}"))?
}

/// 加载安装 skill（服务器优先，内嵌兜底）。
#[tauri::command]
async fn load_skill() -> Result<installer::Skill, String> {
    tauri::async_runtime::spawn_blocking(installer::load_skill)
        .await
        .map_err(|e| format!("加载 skill 失败: {e}"))
}

/// 读取人工核验过的免费路线清单；断网时返回 null，由前端明确显示内嵌的最后可信版本。
#[tauri::command]
async fn load_free_registry() -> Option<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(installer::fetch_free_registry)
        .await
        .ok()
        .flatten()
}

/// 对话式安装：跑某个工具的安装流，日志走事件 `uking:wizard`。
/// 失败自动上报 bug（采集日志尾部，定期巡视修复）。
#[tauri::command]
async fn install_tool(app: AppHandle, tool_id: String) -> Result<installer::InstallToolResult, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = installer::load_skill();
        let tid = tool_id.clone();
        let log_store = std::sync::Mutex::new(Vec::<String>::new());
        // 装机日志同时落盘 ~/.uking/logs/install.log：客户点「技术支持」时诊断才带得上
        // （此前日志只活在前端气泡里，采不到 —— issue #226 的客户只能手工复制粘贴）。
        installer::install_log_header(&tool_id);
        let r = installer::install_tool(&skill, &tool_id, &|phase: &str, line: &str| {
            let _ = app2.emit(
                "uking:wizard",
                serde_json::json!({ "tool": tid, "phase": phase, "line": line }),
            );
            installer::append_install_log(&tid, phase, line);
            let mut l = log_store.lock().unwrap();
            l.push(format!("[{phase}] {line}"));
            if l.len() > 120 {
                l.drain(..40);
            }
        });
        if !r.ok {
            let tail = log_store.lock().unwrap().join("\n");
            report::report_bug(
                "install_failed",
                &format!("{tool_id} 安装失败: {}", r.error.clone().unwrap_or_default()),
                &format!("skill v{} ({})\n{tail}", skill.version, skill.source),
            );
        }
        r
    })
    .await
    .map_err(|e| format!("安装任务异常: {e}"))
}

/// 环境预检 + 免提权自动修（PATH 丢 System32 等）。
/// 安装流水线自动跑；Wizard 失败界面的「修复环境并重试」按钮也走这里。
#[tauri::command]
async fn env_precheck() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(installer::env_precheck_and_fix)
        .await
        .map_err(|e| format!("预检任务异常: {e}"))
}

/// 驱动预设列表。
///
/// `tool` = 看哪个 AI 的列表（claude / codex / clawx / hermes）——**每个 AI 各有一份**：
/// 在 Claude Code 那页删掉一个供应商，Hermes 那页照样留着。不传 = 不分工具的全局视角
/// （托盘 / 装机向导用，它们本来就是一次对多个工具动手）。
#[tauri::command]
fn list_providers(tool: Option<String>) -> Vec<providers::ProviderPreset> {
    providers::list_providers_for(tool.as_deref())
}

/// 应用驱动到 Claude Code / Codex（cc-switch 式改底层）。
///
/// 薄壳，真身是影核动作 `runtime.driver.apply`。返回 `ApplyResult` 的原样 JSON，
/// 前端零改动。**不带 `expected_state_version`**：GUI 是当前窗口的用户在当下点的，
/// 这就是他要的最新意图；乐观并发是给远端影子/多终端那条路准备的。
///
/// `model` 是 None 就**不塞这个键**（同 `apply_clawx_managed`）。
/// 核心那边现在也把可选字段的 null 当「没给」，两层都补上不是重复：薄壳别把
/// 「没有」翻译成 `null` 是本分，核心照单能收是兜底 —— 少任何一层，
/// 换个调用方（CLI / MCP / AI）就会重演一次「写配置失败：字段 `model` 应为 string」。
#[tauri::command]
async fn apply_provider(
    provider_id: String,
    api_key: String,
    model: Option<String>,
    targets: Vec<String>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({
        "provider_id": provider_id,
        "api_key": api_key,
        "targets": targets,
    });
    if let Some(m) = model {
        input["model"] = serde_json::Value::String(m);
    }
    let v = run_write_action(actions::DRIVER_APPLY, input).await?;
    Ok(action_field(v, "applied", serde_json::json!({})))
}

/// **Codex 默认省钱路由**（2026-07-20 拍板）：给 Codex 配虾盘云时，默认链路 = 本地 DeepSeek
/// 代理（port 15722）。gpt-5.3-codex 上游成本贵几十倍，默认直连会把客户余额和上游账单一起
/// 烧穿（「现金老虎」）。需要海外 GPT 模型的：显式选非 deepseek 模型（尊重、走直连），或联系
/// 客服私聊对接。
/// - 沙箱（UKING_TEST_HOME）跳过：selfcheck 不起真进程、不碰真 ~/.codex
/// - 代理起不来（缺 Node 等）→ 保持直连配置（贵但可用），绝不留死端口配置
/// - 开机/重开后的代理存活由 codex_proxy::resume_if_configured 兜底
pub(crate) fn ensure_codex_cheap_route(provider_id: &str, model: Option<&str>) -> Option<String> {
    if provider_id != "xiapan" {
        return None;
    }
    if std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false) {
        return None;
    }
    // 用户显式指定了非 deepseek 模型 = 海外/自配需求，尊重直连不接管
    if let Some(m) = model {
        let m = m.trim().to_ascii_lowercase();
        if !m.is_empty() && !m.starts_with("deepseek") {
            return None;
        }
    }
    // None → 用上次持久化的路由，没有则默认 DeepSeek Flash（省）。
    codex_proxy::codex_proxy_start(None)
        .ok()
        .map(|_| "已自动开 Codex 省钱路由".to_string())
}

/// 给「只有 OpenAI 端点」的供应商开**本地翻译桥**来驱动 Claude Code（issue #359 / #322）。
///
/// 组合根干的事：起桥（`claude_proxy`）→ 写 Claude Code 配置（`providers`）。
/// 这两个功能模块**互不认识**，谁也不 import 谁，拼装在这儿（宪法第 12 条）。
///
/// 🔴 **不给桥传 Key**：Claude Code 会把我们写进 `ANTHROPIC_AUTH_TOKEN` 的那把 Key
/// 放进请求头，桥直接转发就行 —— 少一份 Key 副本，就少一处泄漏面
/// （不进环境变量、不进命令行、不进进程列表）。
///
/// 返回里带 `runs_only_while_app_open` —— 调用方**必须**把这句话摆在客户眼前：
/// 桥是 U-King 的子进程，U-King 一退桥就没了，那一刻 Claude Code 立刻连不上。
#[tauri::command]
async fn claude_bridge_enable(
    provider_id: String,
    api_key: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let openai_base = providers::openai_base_of(&provider_id)?;
    let model = model.unwrap_or_default();
    let st = claude_proxy::start(&openai_base, "", &model)?;
    let m = if model.trim().is_empty() { None } else { Some(model.trim()) };
    let base = providers::apply_claude_bridged(
        &provider_id,
        api_key.as_deref().unwrap_or(""),
        m,
        &st.base_url,
    )
    // 配置没写成就别把桥晾在那儿空跑（客户看不见它，只会以为「开了但没用」）。
    .inspect_err(|_| {
        let _ = claude_proxy::stop();
    })?;
    Ok(serde_json::json!({
        "ok": true,
        "base_url": base,
        "upstream": st.upstream,
        "runs_only_while_app_open": st.runs_only_while_app_open,
    }))
}

/// 关掉翻译桥，并把 Claude Code 还原成官方直连。
///
/// **必须两件事一起做**：只停桥不改配置的话，Claude Code 还指着一个已经没人监听的
/// 本地端口 —— 客户下次用它会撞上一个莫名其妙的连接错误，而界面上显示「已关闭」。
#[tauri::command]
async fn claude_bridge_disable() -> Result<serde_json::Value, String> {
    claude_proxy::stop()?;
    let r = providers::apply_provider("official", "", None, &["claude".to_string()])?;
    Ok(serde_json::json!({ "ok": true, "claude": r.claude }))
}

/// **一键配好全部** —— 后端自己探测装了哪些工具，把虾盘云 Key 写进去。
/// 小白主路径，api_key 传空时自动用设备内置 Key（不查网络，离线即可拿到）。
///
/// `targets` 给了就只配这几个（用户在勾选框里点的），不给 = 探到的全配。
/// 探测「装没装」永远在后端，`targets` 只做减法 —— 见动作表里那段注释。
#[tauri::command]
async fn apply_xiapan_everywhere(
    provider_id: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    targets: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({});
    if let Some(v) = provider_id { input["provider_id"] = serde_json::Value::String(v); }
    if let Some(v) = api_key { input["api_key"] = serde_json::Value::String(v); }
    if let Some(v) = model { input["model"] = serde_json::Value::String(v); }
    if let Some(v) = targets { input["targets"] = serde_json::json!(v); }
    run_write_action(actions::DRIVER_APPLY_EVERYWHERE, input).await
}

/// ClawX 是否在运行（前端据此决定要不要弹「需要临时关闭 ClawX」确认）。
/// 薄壳，真身是影核动作 `runtime.clawx.inspect`。
#[tauri::command]
async fn clawx_running() -> serde_json::Value {
    action_field(run_action_blocking(actions::CLAWX_INSPECT).await, "running", serde_json::json!(false))
}

/// **托管式把驱动配进 ClawX**：关闭 ClawX → 写两层配置（聊天层 clawx-providers.json +
/// agent 层 openclaw.json/三件套）→ 重启 ClawX。
///
/// 为什么要「关→写→开」：ClawX 运行时持有配置的内存副本，不关就写会被它退出时覆盖回去
/// （「配了没反应」的根因）。前端**在用户确认后**调用本命令（确认弹窗由前端做）。
/// 进度走事件 `uking:clawx_config`。沙箱（UKING_TEST_HOME 非空）下跳过关/重启、只写配置，
/// 供 `--selfcheck` 在不动真 ClawX 的前提下校验写入逻辑。
#[tauri::command]
async fn apply_clawx_managed(
    app: AppHandle,
    provider_id: String,
    api_key: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({ "provider_id": provider_id });
    if let Some(k) = api_key { input["api_key"] = serde_json::Value::String(k); }
    if let Some(m) = model { input["model"] = serde_json::Value::String(m); }
    let v = run_write_action_progress(app, actions::CLAWX_APPLY_MANAGED, "uking:clawx_config", input).await?;
    Ok(action_field(v, "applied", serde_json::json!({})))
}

/// 新增 / 更新自定义 provider（~/.uking/providers.json）。内置不可改。
#[tauri::command]
async fn add_provider(provider: serde_json::Value) -> Result<serde_json::Value, String> {
    let v = run_write_action(actions::PROVIDER_SAVE, serde_json::json!({ "provider": provider })).await?;
    Ok(action_field(v, "saved", serde_json::json!({})))
}

/// 更新自定义 provider（与 add 同为 upsert，分开命名只为前端语义清晰）。
#[tauri::command]
async fn update_provider(provider: serde_json::Value) -> Result<serde_json::Value, String> {
    let v = run_write_action(actions::PROVIDER_SAVE, serde_json::json!({ "provider": provider })).await?;
    Ok(action_field(v, "saved", serde_json::json!({})))
}

/// 从**某一个 AI** 的列表里移除一个 provider（内置也能移除 —— 立墓碑，不会自己回来）。
/// 薄壳，真身是影核动作 `runtime.provider.delete`。
///
/// 带 `tool` = 只动那个 AI 的列表，其余三个一字不动，自定义供应商的定义和 Key 也留着
/// （界面上的垃圾桶按钮走这条）。不带 `tool` = 从所有 AI 的列表里拿走，自定义连定义带 Key
/// 一起删（界面上的「彻底删除」）。
#[tauri::command]
async fn delete_provider(id: String, tool: Option<String>) -> Result<(), String> {
    let mut input = serde_json::json!({ "id": id });
    if let Some(t) = tool {
        input["tool"] = serde_json::Value::String(t);
    }
    run_write_action(actions::PROVIDER_DELETE, input).await.map(|_| ())
}

/// 把被移除的驱动加回某个 AI 的列表（「添加虾盘云」）。薄壳，真身是 `runtime.provider.restore`。
#[tauri::command]
async fn restore_provider(id: String, tool: Option<String>) -> Result<(), String> {
    let mut input = serde_json::json!({ "id": id });
    if let Some(t) = tool {
        input["tool"] = serde_json::Value::String(t);
    }
    run_write_action(actions::PROVIDER_RESTORE, input).await.map(|_| ())
}

/// 某个 AI 的列表里有哪些驱动被用户移除了（前端据此决定「已移除：+ 虾盘云」那行出不出现）。只读。
#[tauri::command]
fn hidden_providers(tool: Option<String>) -> Vec<String> {
    providers::hidden_ids_for(tool.as_deref())
}

/// 某个 AI 当前不在列表里、可以一键加回来的驱动（「添加供应商」弹窗顶部那一排）。只读。
/// 默认状态下就有 DeepSeek / GLM / Kimi / Ollama —— 它们不占列表，但用户想加时一点就有；
/// 被从这个 AI 移走的自定义供应商也在里面（定义还在，能加回来）。
#[tauri::command]
fn addable_providers(tool: Option<String>) -> Vec<providers::ProviderPreset> {
    providers::addable_for(tool.as_deref())
}

/// 保存某个 AI 的供应商显示顺序（第一位 = 首选）。
///
/// **故意不进影核动作表**：它只改 U-King 自己的一份显示偏好，不碰客户机器上任何 AI 工具的
/// 配置，也没有跨界面/AI 调用的业务意义 —— 跟「立即运行一次」不进动作表是同一类判断，
/// 只不过那个是因为非幂等，这个是因为它压根不是业务动作，是界面偏好。
#[tauri::command]
fn set_provider_order(ids: Vec<String>, tool: Option<String>) -> Result<(), String> {
    providers::set_provider_order_for(tool.as_deref(), ids)
}

/// AI 作图：调虾盘云 image 端点出图，用设备内置 Key 计费。
/// 前端主动上报 bug（作图失败 / 其它前端可捕获的错误）。静默后台发送，永不阻塞。
#[tauri::command]
fn report_bug(kind: String, summary: String, detail: String) {
    // 先落盘再上网（同 panic hook 的理由）：前端的白屏 / 未捕获 Promise 全靠这条路留痕，
    // 而它原本**只发网络** —— 客户断网或代理挡一下，界面崩了机器上一点痕迹都没有。
    crashlog::record(&kind, &summary, &detail);
    report::report_bug(&kind, &summary, &detail);
}

/// 把「拿到结果但只有境外 CDN url、b64 下载失败」归一成清晰可操作的错误。
/// 背景（2026-06-22 Mac 裸网实测）：seedream/wanx 出图存字节 TOS / 阿里 OSS 的新加坡 CDN
/// （`ark-...ap-southeast-1...volces.com`），国内裸网 TLS 直接 reset（curl 35）→ ensure_b64 下载空。
/// 这种「成功但拿不到图」要直说，并指路默认的 gpt-image-2（回 b64、不走 CDN，国内稳）。
fn require_image_b64(r: Result<providers::ImageResult, String>) -> Result<providers::ImageResult, String> {
    match r {
        Ok(img) if img.b64.is_some() => Ok(img),
        Ok(_) => Err("出图成功，但该模型把图片存在了境外 CDN、当前网络下载失败（国内常见）。请改用「GPT Image 2」模型重试（它直接返回图片、国内最稳）。".into()),
        Err(e) => Err(e),
    }
}

// ── 网站 GEO 体检（geo.rs：跑 1so-geo 技能包「一搜商答」生成互联网体检面板）──────
// 独立可插拔：删本功能只动 lib.rs（去 mod geo + 下面 3 个 command + invoke 注册）与 App.tsx。
#[tauri::command]
async fn geo_scan(name: String, region: Option<String>) -> Result<geo::GeoScan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        geo::run_scan(&name, region.as_deref().unwrap_or(""))
    })
    .await
    .map_err(|e| format!("体检任务异常: {e}"))?
}

// 🔴 `geo_aicheck` / `geo_inspect` 两个 command 2026-08-24 删除（用户拍板）。
//
// **不是因为功能不好，是因为它们是一条会烧我们钱的口子。** `1so-geo` 的 `llm.mjs`
// 会自己去读 `~/.uking/device.json` 里的虾盘云设备钱包 Key —— 客户机上只要躺着那些脚本，
// 任何人在命令行调 aicheck 就能拿我们的额度跑模型，而我们这边零信号。
// 所以真正的闸门是 `geo.rs::SKILL_FILES` 不再发布它们、`REMOVED_FILES` 把老客户机上那份删掉；
// 这里删 command 只是**不留一个指向空气的入口**。
//
// 🔴 **能力和代码一行没丢**：完整的技能包还在仓库 `src-tauri/skills/1so-geo/`，
// 我们自己给客户人工出报告用的就是它。丢的只是「在客户机上能被调起来」。
// 页面改成「免费自查（离线）+ 样板报告展示 + 加微信」，见 src/Geo.tsx。

/// 样板 GEO 报告的路径（演示数据、静态文件）。前端拿到后用 `geo_open_panel` 打开。
#[tauri::command]
async fn geo_sample_report() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(geo::sample_report)
        .await
        .map_err(|e| format!("取样板报告任务异常: {e}"))?
}

/// GEO 技能包是否已装（前端据此提示"未安装"引导去装）。
/// 薄壳，真身是影核动作 `runtime.geo.inspect`。
#[tauri::command]
async fn geo_installed() -> serde_json::Value {
    action_field(run_action_blocking(actions::GEO_INSPECT).await, "installed", serde_json::json!(false))
}

/// 用系统默认浏览器打开体检面板 HTML（面板要开一堆"去查"新标签，浏览器最合适）。
#[tauri::command]
fn geo_open_panel(path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("面板路径为空".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // start "" <file>：空标题参数必带，否则 start 把带空格/中文的路径当标题
        std::process::Command::new(crate::installer::system_tool("cmd"))
            .args(["/C", "start", "", &path])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| format!("打开体检面板失败: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("打开体检面板失败: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn generate_image(
    prompt: String,
    model: Option<String>,
    size: Option<String>,
    quality: Option<String>,
) -> Result<providers::ImageResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = device::device_key_offline()?;
        let model = model.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "gpt-image-2".into());
        let size = size.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "1024x1024".into());
        let r = require_image_b64(providers::generate_image(&key, &prompt, &model, &size, quality.as_deref()));
        // 成功/失败都落历史（关 app 也不丢）。成功时 b64 落成 png 文件，只在历史里留路径。
        // 用 img.model（实际出图模型）而非请求模型：安全兜底换了 Seedream 时历史如实显示。
        match &r {
            Ok(img) => {
                let _ = draw::save_record(&prompt, &img.model, &size, img.b64.as_deref(), img.revised_prompt.as_deref(), None);
            }
            Err(e) => {
                let _ = draw::save_record(&prompt, &model, &size, None, None, Some(e));
            }
        }
        r
    })
    .await
    .map_err(|e| format!("作图任务异常: {e}"))?
}

/// AI 图生图 / 图片编辑：带参考图调虾盘云 edits 端点。images 为 base64（可带 data: 前缀）。
#[tauri::command]
async fn generate_image_edit(
    prompt: String,
    model: Option<String>,
    size: Option<String>,
    images: Vec<String>,
) -> Result<providers::ImageResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = device::device_key_offline()?;
        let model = model.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "gpt-image-2".into());
        let size = size.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "1024x1024".into());
        let r = require_image_b64(providers::generate_image_edit(&key, &prompt, &model, &size, &images));
        // 成功/失败都落历史（与文生图一致）。出图本体落 png，历史只留文件名。
        // 用 img.model（实际出图模型）：安全兜底换了 Seedream 时历史如实显示 seedream-4-0。
        match &r {
            Ok(img) => {
                let _ = draw::save_record(&prompt, &img.model, &size, img.b64.as_deref(), img.revised_prompt.as_deref(), None);
            }
            Err(e) => {
                let _ = draw::save_record(&prompt, &model, &size, None, None, Some(e));
            }
        }
        r
    })
    .await
    .map_err(|e| format!("图生图任务异常: {e}"))?
}

/// 导出作图：弹原生「另存为」对话框，把磁盘上的 PNG 复制到用户选的位置。
/// 不依赖 `<a download>`（Mac WKWebView 对 data: URL 的 download 经常不灵），后端实打实拷文件。
/// 返回 Some(保存路径) / None(用户取消)。
#[tauri::command]
async fn export_draw(app: AppHandle, id: i64) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let src = draw::draw_file_path(id).ok_or("找不到该图片（可能已被清空）")?;
        let dest = app
            .dialog()
            .file()
            .set_file_name(format!("uking-draw-{id}.png"))
            .add_filter("PNG 图片", &["png"])
            .blocking_save_file();
        match dest {
            Some(fp) => {
                let p = fp.into_path().map_err(|e| format!("路径无效: {e}"))?;
                std::fs::copy(&src, &p).map_err(|e| format!("导出失败: {e}"))?;
                Ok(Some(p.display().to_string()))
            }
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| format!("导出任务异常: {e}"))?
}

/// 导出视频：弹原生「另存为」，把磁盘上的 mp4 复制到用户选的位置。返回 Some(路径)/None(取消)。
#[tauri::command]
async fn export_video(app: AppHandle, id: i64) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let src = video::video_file_path(id).ok_or("找不到该视频（可能未下完或已清空）")?;
        let dest = app
            .dialog()
            .file()
            .set_file_name(format!("uking-video-{id}.mp4"))
            .add_filter("MP4 视频", &["mp4"])
            .blocking_save_file();
        match dest {
            Some(fp) => {
                let p = fp.into_path().map_err(|e| format!("路径无效: {e}"))?;
                std::fs::copy(&src, &p).map_err(|e| format!("导出失败: {e}"))?;
                Ok(Some(p.display().to_string()))
            }
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| format!("导出任务异常: {e}"))?
}

/// 导出「海报+真二维码」合成图：前端已经在 canvas 上把二维码贴到背景图并合成好一张
/// PNG（data URL），这里只弹原生「另存为」把它写到用户选的位置。跟 export_draw/export_video
/// 同款约定，区别是这里没有预先落盘的源文件，收到的是全新数据，落盘用 write 而不是 copy。
#[tauri::command]
async fn export_qr_merge(app: AppHandle, png_base64: String, filename: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        // 容忍 data URL：取最后一个逗号之后的纯 base64（与 providers.rs::generate_image_edit 同一约定）
        let raw = png_base64.rsplit(',').next().unwrap_or(&png_base64);
        let bytes = qr_merge_b64_decode(raw)?;
        if bytes.is_empty() {
            return Err("图片数据为空，合成可能失败了，请重试".into());
        }
        let name = if filename.trim().is_empty() { "uking-qrmerge.png".to_string() } else { filename };
        let dest = app
            .dialog()
            .file()
            .set_file_name(name)
            .add_filter("PNG 图片", &["png"])
            .blocking_save_file();
        match dest {
            Some(fp) => {
                let p = fp.into_path().map_err(|e| format!("路径无效: {e}"))?;
                std::fs::write(&p, &bytes).map_err(|e| format!("导出失败: {e}"))?;
                Ok(Some(p.display().to_string()))
            }
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| format!("导出任务异常: {e}"))?
}

// ── base64 解码（纯 std；与 draw.rs/fs.rs/providers.rs/video.rs 各自一份同款独立小实现，
//    「叶子工具不跨模块耦合」，不新增 crate、不跨模块 import）──
fn qr_merge_b64_decode(s: &str) -> Result<Vec<u8>, String> {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &c) in B64.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let mut buf = 0u32;
    let mut bits = 0;
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = rev[c as usize];
        if v == 255 {
            return Err("图片数据编码异常".into());
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

/// 导出 AI 技能包（作图/视频）：弹原生「选择文件夹」对话框，把内嵌的 SKILL.md + CLI 脚本
/// 写进 `<所选目录>/uking-aigc/` 并打开该目录。前端传了 `dest` 用之；没传则弹选择框；
/// 用户取消选择则退默认 `~/.uking/skills/`。返回技能包根目录路径。
#[tauri::command]
async fn export_skill_pack(app: AppHandle, dest: Option<String>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let chosen: Option<std::path::PathBuf> = match dest {
            Some(d) if !d.trim().is_empty() => Some(std::path::PathBuf::from(d)),
            _ => app
                .dialog()
                .file()
                .blocking_pick_folder()
                .and_then(|fp| fp.into_path().ok()),
        };
        let root = skillpack::export_to(chosen.as_deref())?;
        skillpack::reveal_dir(&root); // 写完顺手打开目录
        Ok(root)
    })
    .await
    .map_err(|e| format!("导出技能包异常: {e}"))?
}

/// 一键装进已装的 AI 工具：先释放到默认 `~/.uking/skills/uking-aigc`，再探测已装工具
/// （Claude Code / OpenClaw·ClawX / Hermes）拷进各自 skills 目录，让 AI 直接发现脚本。
/// 治「客户只复制了说明、没手动拷文件夹 → AI 找不到 gen-*.mjs」的路径发现痛点。
/// 返回 `{ default_dir, installed:[{tool,path,experimental}] }`（含绝对路径，供前端回显 + 拼说明文档；
/// experimental=true 的工具（如 Hermes）前端如实标「实验性」，不与已验证工具混列）。
#[tauri::command]
async fn install_skill_pack() -> Result<serde_json::Value, String> {
    run_write_action(actions::SKILLPACK_INSTALL, serde_json::json!({})).await
}

/// 视频唯一执行核心：提交（带写前日志）→ 轮询 → 下载。GUI、CLI、MCP 和 AI 工具都应转调它，
/// 不能各自拼一套 POST/恢复/落盘流程。`execution_id` 来自 ActionParity 信封时即为上游扣费幂等键。
pub(crate) fn run_video_generation(
    prompt: &str,
    model: Option<&str>,
    image: Option<&str>,
    execution_id: Option<&str>,
    on_progress: &dyn Fn(i64, &str, &str),
) -> Result<i64, String> {
    let key = device::device_key_offline()?;
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or("doubao-seedance-2-0-mini-260615");
    // 写前日志必须早于 POST：进程若死在服务端已扣费、task_id 响应未落本机的缝里，
    // 重启会拿同一幂等键+同一请求体重放，服务端只返回原任务，不再扣一次。
    let pending = video::stage_submit_with_id(prompt, model, image, execution_id)?;
    let task_id = match video::submit(&key, model, prompt, image, Some(&pending.request_id)) {
        Ok(id) => id,
        Err(e) => {
            // 余额/参数/审核是服务端明确拒绝，肯定没有任务；网络/超时则可能已经收单，
            // 必须留下事务让下次同幂等键恢复，不能当失败清掉后再建一条。
            if video::submit_error_is_definitive(&e) {
                video::clear_pending_submit(&pending.request_id);
            }
            return Err(e);
        }
    };
    let id = video::create_record(prompt, model, &task_id)?;
    video::clear_pending_submit(&pending.request_id);
    // 同一 execution_id 的成功重放直接回原产物；不能再下载、更不能重建收费任务。
    if video::video_file_path(id).is_some() {
        return Ok(id);
    }
    if !video::try_begin_run(id) {
        // 另一条相同任务已在本地轮询；返回同一记录给调用方即可。
        return Ok(id);
    }
    on_progress(id, "running", "0%");
    let r = video::run(&key, id, &task_id, &|phase, progress| on_progress(id, phase, progress));
    video::end_run(id);
    r.map(|_| id)
}

/// GUI 薄壳：事件名和返回形状保留，业务只走 `run_video_generation`。
#[tauri::command]
async fn generate_video(
    app: AppHandle,
    prompt: String,
    model: Option<String>,
    image: Option<String>,
) -> Result<i64, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_video_generation(&prompt, model.as_deref(), image.as_deref(), None, &|id, phase, progress| {
            let _ = app2.emit("uking:video_progress", serde_json::json!({"id": id, "phase": phase, "progress": progress}));
        })
    })
    .await
    .map_err(|e| format!("视频任务异常: {e}"))?
}

/// 续跑一条还在 running 的视频记录（app 重开后接着轮询/下载）。
#[tauri::command]
async fn resume_video(app: AppHandle, id: i64) -> Result<i64, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = device::device_key_offline()?;
        let task_id = video::task_id_of(id).ok_or("找不到该视频任务")?;
        if !video::try_begin_run(id) {
            // 启动恢复与页面恢复撞车时，原恢复线程已经负责交付；这里幂等返回，不再开第二个下载者。
            return Ok(id);
        }
        let r = video::run(&key, id, &task_id, &|phase, progress| {
            let _ = app2.emit("uking:video_progress", serde_json::json!({"id": id, "phase": phase, "progress": progress}));
        });
        video::end_run(id);
        r.map(|_| id)
    })
    .await
    .map_err(|e| format!("视频续跑异常: {e}"))?
}

/// 一键成片：只负责本地进程/历史/进度，分镜、作图、视频、配音、拼接全部由成熟的
/// `gen-reel.mjs` 编排器完成。进度事件 `{id,phase,detail}` 给 Reel.tsx 模块级状态消费。
#[tauri::command]
async fn submit_reel(app: AppHandle, params: reel::ReelParams) -> Result<i64, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let params = reel::prepare_params(params)?;
        let id = reel::create_record(&params)?;
        let key = match device::device_key_offline() {
            Ok(key) => key,
            Err(e) => { reel::mark_failed(id, e.clone()); return Err(e); }
        };
        let _ = app2.emit("uking:reel_progress", serde_json::json!({"id": id, "phase": "dialogue", "detail": "【1/5】准备对白与分镜…"}));
        match reel::run(id, &params, &key, &|phase, detail| {
            let _ = app2.emit("uking:reel_progress", serde_json::json!({"id": id, "phase": phase, "detail": detail}));
        }) {
            Ok(()) => Ok(id),
            Err(e) => { reel::mark_failed(id, e.clone()); Err(e) }
        }
    }).await.map_err(|e| format!("一键成片任务异常: {e}"))?
}

/// 失败/中断的成片只能按原参数**重新生成**；不声称可续传，也不掩盖它会产生新费用。
#[tauri::command]
async fn resume_reel(app: AppHandle, id: i64) -> Result<i64, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let params = reel::restart_record(id)?;
        let key = match device::device_key_offline() {
            Ok(key) => key,
            Err(e) => { reel::mark_failed(id, e.clone()); return Err(e); }
        };
        let _ = app2.emit("uking:reel_progress", serde_json::json!({"id": id, "phase": "dialogue", "detail": "【1/5】按原参数重新生成（将产生新费用）…"}));
        match reel::run(id, &params, &key, &|phase, detail| {
            let _ = app2.emit("uking:reel_progress", serde_json::json!({"id": id, "phase": phase, "detail": detail}));
        }) {
            Ok(()) => Ok(id),
            Err(e) => { reel::mark_failed(id, e.clone()); Err(e) }
        }
    }).await.map_err(|e| format!("重新生成成片任务异常: {e}"))?
}

#[tauri::command]
fn list_reel_history() -> Vec<reel::ReelItemOut> { reel::list_history() }

/// 和 read_video 同样只回绝对路径；前端 convertFileSrc 走已存在的 video asset scope 流播大文件。
#[tauri::command]
fn read_reel_file(id: i64) -> Result<String, String> {
    reel::file_path(id).map(|p| p.display().to_string()).ok_or_else(|| "找不到该成片（可能未生成完成或已删除）".into())
}

#[tauri::command]
fn delete_reel(id: i64) -> Result<(), String> { reel::delete_record(id) }

#[tauri::command]
fn list_video_history() -> Vec<video::VideoItemOut> {
    video::list_history()
}

/// 这个进程是主实例还是并行调试实例。**薄壳**：转调 `instance::inspect()`，跟
/// `runtime.instance.inspect` 动作、CLI、MCP 是同一份实现，不是第二份。
///
/// 前端为什么不能只靠 `uking:sidecar-mode` 事件：那个 emit 发生在 `setup()` 里，
/// 而前端的 `listen` 要等 React 挂载才注册 —— **正常情况下事件先发、监听后挂，必然错过**。
/// 事件负责「刚好在场时立刻知道」，这条命令负责「进来先问一遍」，两条都要。
#[tauri::command]
fn instance_role() -> serde_json::Value {
    instance::inspect()
}

/// GUI 一启动就恢复所有“服务端仍在跑 / 已出片待下载”的任务，不要求客户先点进视频页。
/// 每条走同一个 resume_video 动作核心；页面只是另一个调用方，不复制任务逻辑。
fn resume_pending_videos(app: AppHandle) {
    if let Some(pending) = video::pending_submit() {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let app3 = app2.clone();
            let r = tauri::async_runtime::spawn_blocking(move || {
                let key = device::device_key_offline()?;
                let task_id = match video::submit(
                    &key,
                    &pending.model,
                    &pending.prompt,
                    pending.image.as_deref(),
                    Some(&pending.request_id),
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        if video::submit_error_is_definitive(&e) {
                            video::clear_pending_submit(&pending.request_id);
                        }
                        return Err(e);
                    }
                };
                let id = video::create_record(&pending.prompt, &pending.model, &task_id)?;
                video::clear_pending_submit(&pending.request_id);
                if !video::try_begin_run(id) {
                    return Ok(id);
                }
                let run = video::run(&key, id, &task_id, &|phase, progress| {
                    let _ = app3.emit("uking:video_progress", serde_json::json!({"id": id, "phase": phase, "progress": progress}));
                });
                video::end_run(id);
                run.map(|_| id)
            })
            .await;
            match r {
                Err(e) => ulog::write("video", &format!("启动恢复提交事务异常：{e}")),
                Ok(Err(e)) => ulog::write("video", &format!("启动恢复提交事务暂未完成：{e}")),
                Ok(Ok(_)) => {}
            }
        });
    }
    for id in video::recoverable_ids() {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = resume_video(app2, id).await {
                ulog::write("video", &format!("启动恢复 id={id} 暂未完成：{e}"));
            }
        });
    }
}

/// 视频预览：返回磁盘 mp4 的绝对路径，前端用 `convertFileSrc` 转成 asset 协议 URL 直接播放。
/// 不再像旧版那样整个文件读出来编成 base64 data URL 塞进 IPC —— 生成视频常有几 MB～几十 MB，
/// 走 data URL 会让 WebView2 长时间卡在解码/渲染上（和作图那次「放大图片卡死」是同一类坑），
/// asset 协议直接流式读磁盘文件，不经过 IPC 序列化。
#[tauri::command]
fn read_video(id: i64) -> Result<String, String> {
    video::video_file_path(id)
        .map(|p| p.display().to_string())
        .ok_or_else(|| "找不到该视频（可能未下完或已清空）".to_string())
}

#[tauri::command]
fn clear_video_history() -> Result<(), String> {
    video::clear_history()
}

/// 实测驱动连通性（模型真实回话）。
#[tauri::command]
async fn test_provider(
    provider_id: String,
    api_key: String,
    model: Option<String>,
    api: String,
) -> providers::TestResult {
    tauri::async_runtime::spawn_blocking(move || {
        providers::test_provider(&provider_id, &api_key, model.as_deref(), &api)
    })
    .await
    .unwrap_or_else(|e| providers::TestResult {
        ok: false,
        api: "unknown".into(),
        latency_ms: 0,
        reply: None,
        error: Some(format!("测试任务异常: {e}")),
    })
}

/// 动态拉取某 provider 真实可用的模型清单（对齐 cc-switch）。
/// 失败前端退回内置候选 + 手填，不致命。
#[tauri::command]
async fn list_remote_models(provider_id: String, api_key: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        providers::list_remote_models(&provider_id, &api_key)
    })
    .await
    .map_err(|e| format!("拉取模型清单异常: {e}"))?
}

/// 「添加供应商」弹窗的**存前**拉模型清单 —— 表单没保存就没有 provider id，
/// 上面那条走不了。直接拿用户填到一半的 base + Key 去打 `/models`。
#[tauri::command]
async fn list_models_at_endpoint(base_url: String, api_key: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || providers::list_models_at(&base_url, &api_key))
        .await
        .map_err(|e| format!("拉取模型清单异常: {e}"))?
}

/// 「添加供应商」弹窗的**存前**试连（理由见 `providers::probe_openai_endpoint` 的注释：
/// 原来只能盲存，报错在三层之外才冒出来）。
#[tauri::command]
async fn probe_endpoint(base_url: String, api_key: String, model: String) -> providers::TestResult {
    tauri::async_runtime::spawn_blocking(move || {
        providers::probe_openai_endpoint(&base_url, &api_key, &model)
    })
    .await
    .unwrap_or_else(|e| providers::TestResult {
        ok: false,
        api: "openai".into(),
        latency_ms: 0,
        reply: None,
        error: Some(format!("试连异常: {e}")),
    })
}

/// 「AI 作图走哪家」的回显（AI 设置那张卡 + 作图页顶部 banner 都读它）。
/// 不带 Key —— 前端不需要它。
#[tauri::command]
async fn get_draw_route() -> providers::DrawRouteView {
    tauri::async_runtime::spawn_blocking(providers::draw_route_view)
        .await
        // 读一个本地小 json 不该失败；真失败了也得给个能用的形状，不能让作图页整块炸掉。
        .unwrap_or_else(|_| providers::draw_route_view())
}

/// 记一笔「AI 作图走哪家」。**不是 apply_provider** —— 作图是 U-King 内部能力，
/// 它的「应用」只是记下选择（理由见 providers.rs 那节注释），不往任何外部工具写配置。
#[tauri::command]
async fn set_draw_route(provider_id: String, model: String) -> Result<providers::DrawRouteView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        providers::set_draw_route(&provider_id, &model)?;
        Ok(providers::draw_route_view())
    })
    .await
    .map_err(|e| format!("保存作图路由异常: {e}"))?
}

/// 查虾盘云余额。
#[tauri::command]
async fn query_balance(api_key: String) -> Result<providers::Balance, String> {
    tauri::async_runtime::spawn_blocking(move || providers::query_balance(&api_key))
        .await
        .map_err(|e| format!("查询异常: {e}"))?
}

/// 查「钱花在哪了」——按模型分组的用量明细（客户自己看，按需查一次）。
#[tauri::command]
async fn query_usage_breakdown(api_key: String, days: i64) -> Result<providers::UsageBreakdown, String> {
    tauri::async_runtime::spawn_blocking(move || providers::query_usage_breakdown(&api_key, days))
        .await
        .map_err(|e| format!("查询异常: {e}"))?
}

/// 查「本地实际用量」——读 Claude Code 自己记的会话日志按模型聚合（含客户自己的 Key/BYOK，
/// 不依赖我们服务器）。只读元数据、不上传（数据安全）。读很多文件，spawn_blocking 别卡 UI。
#[tauri::command]
async fn query_local_usage(days: Option<i64>) -> serde_json::Value {
    let input = match days {
        Some(d) => serde_json::json!({ "days": d }),
        None => serde_json::json!({}),
    };
    run_action_input(actions::USAGE_LOCAL_INSPECT, input).await
}

/// 「这台电脑上的 AI 都在忙什么」—— 各家 AI 自己写的会话记录 + AI 登记在看板上的任务。
/// 薄壳，真身是影核动作 `runtime.ai_tasks.inspect`（读很多文件，走 spawn_blocking 别卡 UI）。
#[tauri::command]
async fn list_ai_tasks(days: Option<i64>) -> serde_json::Value {
    let input = match days {
        Some(d) => serde_json::json!({ "days": d }),
        None => serde_json::json!({}),
    };
    run_action_input(actions::AI_TASKS_INSPECT, input).await
}

/// Token 水电表：按天读数 + 按项目/工具/模型分账 + 缓存账 + 用得多快 + 省钱建议。
/// 薄壳，真身是影核动作 `runtime.usage_meter.inspect`。
///
/// `balance_cny` 让前端把已经查到的余额传进来（**动作本身不联网**），
/// 有它才算得出「按这个速度还能用几天」。
#[tauri::command]
async fn query_usage_meter(days: Option<i64>, balance_cny: Option<f64>, detail: Option<u32>) -> serde_json::Value {
    let mut input = serde_json::Map::new();
    if let Some(d) = days {
        input.insert("days".into(), serde_json::json!(d));
    }
    if let Some(b) = balance_cny {
        input.insert("balance_cny".into(), serde_json::json!(b));
    }
    // 逐条流水（水电表页那块「流水」）。不传 = 不要，动作那边默认 0。
    if let Some(n) = detail {
        input.insert("detail".into(), serde_json::json!(n));
    }
    run_action_input(actions::USAGE_METER_INSPECT, serde_json::Value::Object(input)).await
}

/// 「数据来源」面板：本机探测到的**全部** AI 工具 + 各自算不算得到 + 用户勾了没有。
///
/// **只 stat 几个目录、不扫日志**，毫秒级返回 —— 开个设置页不该等几百 MB 会话日志扫完。
/// 真正的用量走 `query_usage_meter`。
#[tauri::command]
fn usage_sources() -> Vec<usage_local::SourceStatus> {
    usage_local::detect_sources()
}

/// 保存「算哪些工具 / 哪些是包月」。
///
/// **故意不进影核动作表**：它只改 U-King 自己的一份报表口径偏好，不碰客户机器上任何东西
/// —— 跟 `set_provider_order` 同一类判断（那是界面偏好，这是报表口径），不是业务动作。
/// 另一层考虑：这个开关决定了报告里那个总数**算得全不全**，把它交给 AI 去调，
/// 等于让被统计的一方能自己把账关小（同 `journal_set_enabled` 不进动作表的理由）。
#[tauri::command]
fn set_usage_sources(disabled: Vec<String>, subscription: Vec<String>) -> Result<(), String> {
    usage_local::write_prefs(&usage_local::UsagePrefs { disabled, subscription })
}

/// 当前驱动状态（settings.json / config.toml 回显）。
/// 薄壳，真身是影核动作 `runtime.driver.inspect`。
#[tauri::command]
async fn get_driver_status() -> serde_json::Value {
    run_action_blocking(actions::DRIVER_INSPECT).await
}

/// 每日消耗趋势（最近 N 天）。
#[tauri::command]
fn get_usage_trend(days: Option<usize>) -> usage::UsageTrend {
    usage::trend(days.unwrap_or(14))
}

/// 打开充值页 —— **优先系统浏览器**。
/// 🔴 2026-06-28 pc-*** 实证：充值页点付款是 `location.href` 跳支付宝 PC 收银台，
/// 它在**内嵌 WebView2 子窗口里起不来**（客户「建单成功却付不掉」，订单永远 pending）；
/// 只有真·系统浏览器里支付宝才走得通。故先用 opener 调系统浏览器，
/// 仅当系统连默认浏览器都唤不起时，才退回 webview 子窗口当最后退路
/// （此时支付宝多半仍付不掉，但页面上的微信客服二维码可让客户人工充值，不致彻底无门）。
#[tauri::command]
async fn open_recharge(app: AppHandle, url: String) -> Result<(), String> {
    // 只允许打开我们自己的充值域名，防被前端滥用。
    // u-claw.org.cn = 国内可达充值页（首选）；cloud.u-claw.org 保留兼容旧链路。
    const ALLOWED: [&str; 2] = ["https://u-claw.org.cn/", "https://cloud.u-claw.org/"];
    if !ALLOWED.iter().any(|p| url.starts_with(p)) {
        return Err("非法充值地址".into());
    }
    // ① 正道：调系统浏览器（opener 插件，opener:default 已授权）。支付宝收银台靠它才跑得通。
    match app.opener().open_url(url.clone(), None::<String>) {
        Ok(()) => return Ok(()),
        Err(e) => eprintln!("[recharge] 系统浏览器打开失败，退回内嵌 webview 兜底: {e}"),
    }
    // ② 最后退路：内嵌 webview 子窗口（已存在就聚焦，避免重复开）。
    if let Some(w) = app.get_webview_window("recharge") {
        let _ = w.set_focus();
        let _ = w.eval(&format!("window.location.href={url:?}"));
        return Ok(());
    }
    let parsed = url.parse().map_err(|_| "充值地址解析失败".to_string())?;
    WebviewWindowBuilder::new(&app, "recharge", WebviewUrl::External(parsed))
        .title("U-King · 充值")
        .inner_size(560.0, 760.0)
        .center()
        .resizable(true)
        .build()
        .map_err(|e| format!("打开充值窗口失败: {e}"))?;
    Ok(())
}

/// 本机某个端口上有没有服务在听（给「预览网页」用；测试报告 #015）。
///
/// 为什么需要它：`open_browser` **成功只代表窗口建出来了，不代表页面加载成功**。
/// 所以 3000 端口没服务时，前端的 `catch` 一次都不会进 —— 面板一切正常，
/// 子窗口一片空白。客户看到的就是「预览 localhost:3000 时页面无法正常显示」，
/// 而且完全不知道到底是端口错了、服务没起、还是我们坏了。
///
/// 与其开一个空白窗口让人猜，不如先敲一下门：连得上再开，连不上直接说人话。
/// 只探本机回环地址，`connect_timeout` 300ms —— 本机要么立刻通要么立刻拒，不会卡住 UI。
#[tauri::command]
async fn preview_port_alive(port: u16) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        use std::net::{SocketAddr, TcpStream};
        let dur = std::time::Duration::from_millis(300);
        // v4 / v6 回环都试：有些 dev server 只监听 ::1
        [
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
        ]
        .iter()
        .any(|a| TcpStream::connect_timeout(a, dur).is_ok())
    })
    .await
    .unwrap_or(false)
}

/// 工作台「浏览器」面板：在独立 webview 子窗口打开 URL（localhost / https）。
/// 用子窗口而非 iframe，因 localhost 开发服务器与很多文档站带 X-Frame-Options 会被 deny。
/// label 形如 `browser-<taskId>`，每任务一个，复用则导航。子窗口关闭走正常关（见 on_window_event）。
#[tauri::command]
async fn open_browser(app: AppHandle, url: String, label: String) -> Result<(), String> {
    let ok = url.starts_with("http://localhost")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("https://");
    if !ok {
        return Err("只允许 https 或 http://localhost".into());
    }
    if !label.starts_with("browser-") {
        return Err("非法窗口标识".into());
    }
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        let _ = w.eval(&format!("window.location.href={url:?}"));
        return Ok(());
    }
    let parsed = url.parse().map_err(|_| "地址解析失败".to_string())?;
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title("U-King · 浏览器")
        .inner_size(1000.0, 720.0)
        .center()
        .resizable(true)
        .build()
        .map_err(|e| format!("打开浏览器窗口失败: {e}"))?;
    Ok(())
}

/// `browser_nav` 的入参校验 —— **抽成纯函数就为了能测**。
///
/// 这里是真的攻击面，不是形式主义：`external` 会把一个字符串原样丢给系统浏览器/系统 shell。
/// Windows 上 `file://`、`ms-settings:`、以及各种自定义协议都能被 opener 拉起来，
/// 前端要是哪天把用户输入直接透传进来，就成了「让 U-King 帮我打开任意东西」。
/// 白名单跟 `open_browser` 保持同一套（https / 本机回环），别在两处各写一份。
///
/// label 前缀同理：不校验的话可以拿它去驱动主窗口或别的子窗口（close/eval 都在里面）。
fn validate_nav(label: &str, action: &str, url: Option<&str>) -> Result<(), String> {
    if !label.starts_with("browser-") {
        return Err("非法窗口标识".into());
    }
    const ACTIONS: &[&str] = &["back", "forward", "reload", "focus", "close", "external"];
    if !ACTIONS.contains(&action) {
        return Err(format!("未知动作: {action}"));
    }
    if action == "external" {
        let u = url.unwrap_or("").trim();
        if u.is_empty() {
            return Err("缺少地址".into());
        }
        let ok = u.starts_with("https://")
            || u.starts_with("http://localhost")
            || u.starts_with("http://127.0.0.1");
        if !ok {
            return Err("只允许 https 或 http://localhost".into());
        }
    }
    Ok(())
}

/// 浏览器子窗口的导航控制 —— 后退 / 前进 / 刷新 / 聚焦 / 关闭 / 在系统浏览器打开。
///
/// 为什么不做内嵌浏览器面板：Tauri 的多 webview（把子 webview 嵌进主窗口）在 `unstable`
/// feature 后面，为一个面板给整个 app 换上不稳定 API 不划算。所以子窗口方案保留，
/// 但把「浏览器该有的按钮」补齐 —— 在这之前那个窗口开出来就是条单行道：
/// 页面里点进去了就回不来，只能关掉重开。
///
/// 返回 false = 那个窗口现在没开着（面板据此把按钮灰掉，而不是假装点了有用）。
#[tauri::command]
async fn browser_nav(app: AppHandle, label: String, action: String, url: Option<String>) -> Result<bool, String> {
    validate_nav(&label, &action, url.as_deref())?;
    // 「在系统浏览器打开」不需要子窗口在开着 —— 它本来就是要跳出去
    if action == "external" {
        return app
            .opener()
            .open_url(url.unwrap_or_default(), None::<String>)
            .map(|_| true)
            .map_err(|e| format!("调系统浏览器失败: {e}"));
    }
    let Some(w) = app.get_webview_window(&label) else {
        return Ok(false);
    };
    match action.as_str() {
        "back" => w.eval("history.back()").map_err(|e| e.to_string())?,
        "forward" => w.eval("history.forward()").map_err(|e| e.to_string())?,
        "reload" => w.eval("location.reload()").map_err(|e| e.to_string())?,
        "focus" => w.set_focus().map_err(|e| e.to_string())?,
        "close" => w.close().map_err(|e| e.to_string())?,
        other => return Err(format!("未知动作: {other}")),
    }
    Ok(true)
}

/// 那个浏览器子窗口现在开着吗（面板用来决定按钮灰不灰）。
#[tauri::command]
async fn browser_open(app: AppHandle, label: String) -> bool {
    app.get_webview_window(&label).is_some()
}

/// ★ `--browser-nav-test` 的正文：证明后退 / 前进 / 刷新**真的作用到了外部页面上**。
///
/// 判据全部是「外面看得见的事实」，不是「我们调了这个函数」：
/// - 后退 / 前进 → `WebviewWindow::url()` 读回的**真实地址**变了
/// - 刷新 → 本地服务**再收到一次请求**（地址不变，所以只能用请求次数当证据）
///
/// 走的是 `browser_nav` 里逐字相同的那三行 eval。返回进程退出码（0 = 全过）。
fn run_browser_nav_probe(app: &AppHandle) -> i32 {
    let label = "browser-navtest";
    let mut problems: Vec<String> = Vec::new();
    let mut steps: Vec<serde_json::Value> = Vec::new();

    let (port, hits) = match browser_nav_probe::serve() {
        Ok(v) => v,
        Err(e) => {
            println!("{}", serde_json::json!({ "ok": false, "problems": [format!("起不来本地测试服务: {e}")] }));
            return 1;
        }
    };
    let url_a = format!("http://127.0.0.1:{port}/a");
    let url_b = format!("http://127.0.0.1:{port}/b");

    // 隐藏窗口：`visible(false)`。这条跑道从头到尾不该在屏幕上出现任何东西。
    let win = match tauri::WebviewWindowBuilder::new(
        app,
        label,
        tauri::WebviewUrl::External(url_a.parse().expect("测试 URL 不合法")),
    )
    .title("uking nav probe")
    .visible(false)
    .build()
    {
        Ok(w) => w,
        Err(e) => {
            println!("{}", serde_json::json!({ "ok": false, "problems": [format!("建不出 webview 窗口（这台机器可能没装 WebView2）: {e}")] }));
            return 1;
        }
    };
    let cur = || win.url().map(|u| u.to_string()).unwrap_or_default();

    // ① 先到 A
    if browser_nav_probe::wait_url(cur, "/a", 15_000).is_none() {
        problems.push(format!("页面没能加载到 A（{url_a}）—— 后面的断言全无意义"));
        println!("{}", serde_json::json!({ "ok": false, "problems": problems }));
        return 1;
    }
    steps.push(serde_json::json!({ "step": "load A", "url": cur() }));

    // ② A → B：走 `open_browser` 复用窗口时**同一行** `window.location.href=`
    let _ = win.eval(&format!("window.location.href={url_b:?}"));
    if browser_nav_probe::wait_url(cur, "/b", 10_000).is_none() {
        problems.push("导航到 B 失败 —— eval 根本没作用到页面上".into());
    }
    steps.push(serde_json::json!({ "step": "goto B", "url": cur() }));

    // ③ 后退：**这就是那三个按钮里最要紧的一个**，也是需求榜说「只能真机点」的那条。
    // 🔴 调的是 `browser_nav` 本身，**不是照抄一句 `history.back()`** —— 抄一份的话，
    // 谁把 browser_nav 里那行改坏了这条跑道照样绿，那就成了「跑道自己骗自己」。
    let nav = |action: &str| {
        tauri::async_runtime::block_on(browser_nav(app.clone(), label.to_string(), action.to_string(), None))
    };
    if let Err(e) = nav("back") {
        problems.push(format!("browser_nav(back) 直接报错: {e}"));
    }
    match browser_nav_probe::wait_url(cur, "/a", 10_000) {
        Some(u) => steps.push(serde_json::json!({ "step": "back", "url": u })),
        None => problems.push(format!("点了后退但地址没回到 A（现在是 {}）—— 那个窗口仍旧是条单行道", cur())),
    }

    // ④ 前进
    if let Err(e) = nav("forward") {
        problems.push(format!("browser_nav(forward) 直接报错: {e}"));
    }
    match browser_nav_probe::wait_url(cur, "/b", 10_000) {
        Some(u) => steps.push(serde_json::json!({ "step": "forward", "url": u })),
        None => problems.push(format!("点了前进但地址没回到 B（现在是 {}）", cur())),
    }

    // ⑤ 刷新：地址本来就不变，所以**只能用「服务端又收到一次请求」当证据**。
    let before = hits.b.load(std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = nav("reload") {
        problems.push(format!("browser_nav(reload) 直接报错: {e}"));
    }
    let reloaded = browser_nav_probe::wait_count(
        || hits.b.load(std::sync::atomic::Ordering::Relaxed),
        before + 1,
        10_000,
    );
    if !reloaded {
        problems.push(format!("点了刷新但服务端没再收到请求（仍是 {before} 次）—— 刷新没真的发生"));
    }
    steps.push(serde_json::json!({ "step": "reload", "hits_b_before": before, "hits_b_after": hits.b.load(std::sync::atomic::Ordering::Relaxed) }));

    // ⑥ 窗口没开着时必须如实返回 false，而不是假装点了有用
    if let Err(e) = nav("close") {
        problems.push(format!("browser_nav(close) 直接报错: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    if app.get_webview_window(label).is_some() {
        problems.push("窗口关不掉".into());
    }
    // 窗口没了之后必须**如实返回 false**，而不是假装点了有用（面板据此把按钮灰掉）
    match nav("back") {
        Ok(true) => problems.push("窗口都关了，browser_nav 还报告「点成功了」—— 面板会以为按钮可用".into()),
        Ok(false) => {}
        Err(e) => problems.push(format!("窗口关了之后 browser_nav 该返回 false，却报错: {e}")),
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "ok": problems.is_empty(),
            "problems": problems,
            "steps": steps,
            "covers": "browser_nav 的 back/forward/reload 三行 eval 真的作用到外部页面",
            "does_not_cover": "前端那三个按钮有没有正确接到 browser_nav —— 那是 invoke 接线，仍需在真界面点一次",
        }))
        .unwrap_or_default()
    );
    if problems.is_empty() { 0 } else { 1 }
}

/// 检查更新（拉服务器 version.json 比对内置版本）。
/// U 盘护符口味：直接回「无更新」——服务器绿色 exe 是无护符的下载版，自升级会抹掉护符；
/// U 盘版更新走「换新盘 / 装安装版」。这样升级横幅也不会在 U 盘版里出现。
#[tauri::command]
async fn check_update() -> installer::UpdateInfo {
    if cfg!(feature = "usb-guard") {
        let current = env!("CARGO_PKG_VERSION").to_string();
        return installer::UpdateInfo {
            current: current.clone(),
            latest: current,
            has_update: false,
            // U 盘护符版本有意不走网络检查，不能把这个产品选择误报成网络故障。
            checked_ok: true,
            notes: String::new(),
            download_url: "https://u-claw.org.cn/uking/".into(),
            history: Vec::new(),
            failed_attempts: 0,
            fail_reason: String::new(),
            installer_url: "https://u-claw.org.cn/uking/".into(),
        };
    }
    tauri::async_runtime::spawn_blocking(installer::check_update)
        .await
        .unwrap_or_else(|_| installer::check_update())
}

/// 应用内一键自升级：下新版 → 启动替换脚本 → 退出本进程让脚本覆盖并重启。
/// 成功后本函数返回 Ok(())，前端收到后短暂提示，进程随即退出。
#[tauri::command]
async fn self_update(app: AppHandle, ack_terminal_count: Option<usize>) -> Result<(), String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        installer::self_update(&move |phase: &str, percent: u8| {
            let _ = app2.emit(
                "uking:update_progress",
                serde_json::json!({ "phase": phase, "percent": percent }),
            );
        }, ack_terminal_count)
    })
    .await
    .map_err(|e| format!("升级任务异常: {e}"))??;
    // 替换脚本已在后台等待本进程退出。给前端 800ms 显示提示后【强制】退出本进程。
    // ⚠️ 不能用 app.exit(0)：它会去关主窗口，而主窗口 CloseRequested 被 prevent_close
    // （托盘常驻，见 on_window_event）拦住 → 进程退不掉 → 替换脚本删不掉锁住的旧 exe →
    // 升级卡死（pc-*** 2026-06-23 实证：旧 0.9.16 与新 0.9.17 两进程并存、exe 没换成）。
    // std::process::exit 直接终止本进程，旧 exe 立即解锁，bat 完成「删旧→改名→拉起新版」。
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(800));
        // 自升级重启是**有意**退出：不销账的话每升一次版就自造一条「异常退出」，
        // 而升级恰恰是全量客户都会走的路 —— 噪音会盖过真崩溃。
        crashlog::end_session();
        std::process::exit(0);
    });
    Ok(())
}

/// ★ 自动升级走不通时的兜底：下载官网安装包 → 打开它 → 本进程退出，让它覆盖安装。
///
/// **为什么不是「再点一次一键升级」**：自动替换的失败源（杀软锁 exe、安装目录不可写、
/// 路径含非 ASCII、替换脚本被拦）几乎都不是暂时性的，在同一条路上重试只是重复同一次失败。
/// 覆盖安装是**另一条**路，且不会丢配置（安装包只换程序本体，`~/.uking`/`~/.claude`/
/// `~/.codex` 一个都不碰）。
///
/// 返回安装包路径给前端显示；随后 1.2s 本进程退出（安装程序要替换正在运行的 exe）。
#[tauri::command]
async fn reinstall_latest(app: AppHandle, ack_terminal_count: Option<usize>) -> Result<String, String> {
    #[cfg(windows)]
    {
        let app2 = app.clone();
        let setup = tauri::async_runtime::spawn_blocking(move || {
            // 覆盖安装同样会在当前进程退出时带走 PTY：先拒绝 sidecar，再冻结到下载结束。
            installer::reject_sidecar_self_update()?;
            let mut terminal_update = term::TermUpdatingGuard::begin()?;
            let p = installer::download_installer(&move |phase: &str, percent: u8| {
                let _ = app2.emit(
                    "uking:update_progress",
                    serde_json::json!({ "phase": phase, "percent": percent }),
                );
            })?;
            // 下载完成到拉起安装器之间再验一次：覆盖安装同样会令当前 PTY 随进程退出。
            // 不论调用方是否来自新版确认弹窗，都必须能读会话表；锁中毒时保护性中止。
            let current = term::term_active_count_checked()?;
            if let Some(ack) = ack_terminal_count.filter(|ack| current > *ack) {
                // 本次尚未写快照，不能拿中止路径删掉上次升级遗留的恢复卡。
                return Err(format!(
                    "升级期间新开了 {} 个终端，已中止以保护它们；请重新点击升级",
                    current - ack
                ));
            }
            let snapshot_written = term::snapshot_sessions(&installer::uking_home().join("term-snapshot.json"));
            if let Err(e) = installer::launch_installer(&p) {
                if snapshot_written {
                    let _ = term::term_snapshot_consume();
                }
                return Err(e);
            }
            // 安装器已拉起；后续本进程会硬退出，期间不许再打开新的 PTY。
            installer::keep_terminal_update_until_exit_or_unfreeze(&mut terminal_update);
            Ok::<_, String>(p)
        })
        .await
        .map_err(|e| format!("下载任务异常: {e}"))??;
        // 安装程序起来了才退：早退一步用户会看着 U-King 凭空消失、还不知道装没装。
        // 同 self_update：**不能用 app.exit(0)**（主窗口 CloseRequested 被托盘逻辑拦住，
        // 进程退不掉 → 安装程序换不了正在运行的 exe）。
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            crashlog::end_session(); // 有意退出，别记成崩溃
            std::process::exit(0);
        });
        Ok(setup.to_string_lossy().to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("当前平台请到下载页手动获取新版".into())
    }
}

/// 取出并清除「自升级成功」标记。新版替换后首次启动返回 true，前端据此弹「已升级到 vX」。
/// 非升级启动 / Mac 返回 false。
#[tauri::command]
fn take_update_flag() -> bool {
    installer::take_update_flag()
}

/// 安装/配置完成度（启动时检测，引导用户走完半途退出的流程）。
#[derive(Serialize)]
struct SetupState {
    /// 是否装了至少一个工具（claude / codex）
    has_tool: bool,
    /// 是否配了底层驱动
    has_driver: bool,
    /// 内置 Key 是否已充值
    charged: bool,
    /// ClawX 已装但还没接虾盘云 —— 前端据此弹「是否接入?」非侵入提示条（不自动写）
    clawx_needs_xiapan: bool,
    /// 给前端的引导：none / install_tool / config_driver / recharge / done
    next_step: String,
    /// 一句话引导文案
    hint: String,
}

#[tauri::command]
async fn get_setup_state() -> SetupState {
    tauri::async_runtime::spawn_blocking(|| {
        let detect = installer::detect_stack();
        let driver = providers::driver_status();
        let charged = device::get_device_key().map(|d| d.charged).unwrap_or(false);

        let has_tool = detect.claude.found || detect.codex.found;
        let has_driver = driver.claude_base.is_some() || driver.codex_provider.is_some();
        let clawx_needs_xiapan = providers::clawx_needs_xiapan();

        // 决定下一步：装工具 → 配驱动 → 充值 → 完成
        let (next_step, hint) = if !has_tool {
            ("install_tool", "还没装 AI 工具，点「开始向导」一键装 Claude Code / Codex")
        } else if !has_driver {
            ("config_driver", "工具装好了，但底层驱动还没配 —— 现在 claude/codex 会连不上！点这里配国内驱动")
        } else if !charged {
            ("recharge", "驱动配好了，但虾盘云余额不足。充值补足后就能聊天、写代码、画图")
        } else {
            ("done", "一切就绪，claude / codex 可以直接用")
        };

        SetupState {
            has_tool,
            has_driver,
            charged,
            clawx_needs_xiapan,
            next_step: next_step.into(),
            hint: hint.into(),
        }
    })
    .await
    .unwrap_or_else(|_| SetupState {
        has_tool: false,
        has_driver: false,
        charged: false,
        clawx_needs_xiapan: false,
        next_step: "none".into(),
        hint: String::new(),
    })
}

/// AI 工具体检项：只回读工具自己的现状，前端据此决定是否给出接入入口。
#[derive(Serialize)]
struct AiCheckupItem {
    target: String,
    label: String,
    installed: bool,
    state: String,
    model: Option<String>,
    can_auto_fix: bool,
}

/// AI 工具体检报告。
#[derive(Serialize)]
struct AiCheckupReport {
    charged: bool,
    items: Vec<AiCheckupItem>,
}

/// 端点是不是虾盘云自家域（host 后缀精确匹配）。实现唯一真相源在
/// `providers::is_xiapan_endpoint`（剥 user-info/IPv6/端口后比对）；这里只留
/// Option 签名薄包装给工具体检区用，不做第二份判定逻辑。
fn xiapan_endpoint(base_url: Option<&str>) -> bool {
    base_url.is_some_and(providers::is_xiapan_endpoint)
}

/// 对有独立回读路径的工具统一判定。端点明确属于别人时，宁可礼让客户配置；只有模型
/// 而没有端点时则如实报 ready，避免把读不全误判为可接管。
fn effective_checkup_item(
    target: &str,
    label: &str,
    installed: bool,
    config: providers::EffectiveConfig,
) -> AiCheckupItem {
    if !installed {
        return AiCheckupItem {
            target: target.into(),
            label: label.into(),
            installed: false,
            state: "absent".into(),
            model: None,
            can_auto_fix: false,
        };
    }
    // 🔴 sol 终审抓的真洞：回读失败(readable=false)≠没配置。文件解析不动/被占/写坏时
    // 断言 idle 会给前端接管按钮 → apply 可能把客户自己那份好的配置覆盖掉。
    // 「读不动 = 不知道」，宁可少接管。unknown 时 can_auto_fix=false，只如实灰显。
    if !config.readable {
        return AiCheckupItem {
            target: target.into(),
            label: label.into(),
            installed: true,
            state: "idle".into(),
            model: None,
            can_auto_fix: false,
        };
    }
    let self_managed = config.base_url.as_deref().is_some_and(|base| !xiapan_endpoint(Some(base)));
    let state = if self_managed {
        "self-managed"
    } else if config.model.is_some() {
        "ready"
    } else {
        "idle"
    };
    // 已明确回读到虾盘云的条目也允许「重新接入」：这是修复半截/过期托管配置的路径；
    // 只有模型而没端点的 ready 不给入口，免得把读不全的客户配置当成我们的。
    let can_auto_fix = state == "idle" || (state == "ready" && xiapan_endpoint(config.base_url.as_deref()));
    AiCheckupItem {
        target: target.into(),
        label: label.into(),
        installed: true,
        state: state.into(),
        model: config.model,
        can_auto_fix,
    }
}

fn claude_checkup_item(label: &str, installed: bool) -> AiCheckupItem {
    if !installed {
        return effective_checkup_item("claude", label, false, providers::EffectiveConfig::default());
    }
    // OAuth 凭据是客户直接登录的明确事实，不能因为同时残留旧 env 就抢回驱动。
    let credentials = installer::user_home_dir().join(".claude").join(".credentials.json");
    let config = providers::effective_config("claude");
    let token_present = std::fs::read_to_string(installer::user_home_dir().join(".claude").join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.pointer("/env/ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()).map(str::trim).map(str::to_string))
        .is_some_and(|token| !token.is_empty());
    let state = if credentials.exists() || (token_present && !xiapan_endpoint(config.base_url.as_deref())) {
        "self-managed"
    } else if token_present && xiapan_endpoint(config.base_url.as_deref()) {
        "ready"
    } else {
        "idle"
    };
    AiCheckupItem {
        target: "claude".into(), label: label.into(), installed: true, state: state.into(),
        model: config.model, can_auto_fix: state == "idle",
    }
}

fn ai_checkup_item(target: &str, label: &str, cmd: &str, driver: &providers::DriverStatus) -> AiCheckupItem {
    let installed = installer::tool_installed(cmd);
    if !installed {
        return effective_checkup_item(target, label, false, providers::EffectiveConfig::default());
    }

    match target {
        "claude" => claude_checkup_item(label, true),
        "codex" => {
            let config = providers::effective_config(target);
            // 🔴 同 effective_checkup_item 的 readable 守卫：config.toml 解析不动时（客户
            // 手写了坏语法很常见）断言 idle 会给接管按钮。读不动 = 不知道，不给入口。
            let state = if !config.readable {
                "idle"
            } else if config.base_url.as_deref().is_some_and(|base| !xiapan_endpoint(Some(base))) {
                "self-managed"
            } else if config.model.is_some() || driver.codex_provider.is_some() {
                "ready"
            } else {
                "idle"
            };
            let can_auto_fix = state == "idle" && config.readable;
            AiCheckupItem {
                target: target.into(), label: label.into(), installed: true, state: state.into(),
                model: config.model, can_auto_fix,
            }
        }
        "clawx" => {
            // 🔴 sol 终审 NO-GO 意见②：ClawX 是**运行中进程读自己的配置**，体检此刻写的
            // 托管条目可能被运行中的 ClawX 回写覆盖（我们写配置 ≠ 它运行态收 Key）。
            // P0 一律不给接管入口——ClawX 已有独立口径（driver_status/clawx_needs_xiapan
            // 的顶部提示条），等二期把「ClawX 关闭后收权」做完再回本卡片。
            let state = if providers::clawx_needs_xiapan() { "idle" } else if driver.clawx_model.is_some() { "ready" } else { "idle" };
            AiCheckupItem {
                target: target.into(), label: label.into(), installed: true, state: state.into(),
                model: driver.clawx_model.clone(), can_auto_fix: false,
            }
        }
        "hermes" | "dsh" => {
            let model = if target == "hermes" { driver.hermes_model.clone() } else { driver.dsh_model.clone() };
            let config = providers::effective_config(target);
            if model.is_some() {
                AiCheckupItem {
                    target: target.into(), label: label.into(), installed: true, state: "ready".into(),
                    model, can_auto_fix: false,
                }
            } else {
                effective_checkup_item(target, label, true, config)
            }
        }
        _ => effective_checkup_item(target, label, true, providers::effective_config(target)),
    }
}

/// ai_checkup / doctor_report 共用的条目构造 —— **单一真相源**（宪法第 8 条）：
/// 工具清单以前只活在 ai_checkup 里，「一键体检」要同一份事实，复制一份必然漂移。
fn collect_ai_checkup_items() -> Vec<AiCheckupItem> {
    let driver = providers::driver_status();
    [
        ("claude", "Claude Code", "claude"),
        ("codex", "Codex", "codex"),
        ("clawx", "ClawX", "openclaw"),
        ("hermes", "Hermes", "hermes"),
        ("dsh", "DeepSeek Harness", "dsh"),
        ("qwen", "Qwen Code", "qwen"),
        ("opencode", "OpenCode", "opencode"),
        ("pi", "pi", "pi"),
        ("crush", "Crush", "crush"),
    ]
    .into_iter()
    .map(|(target, label, cmd)| ai_checkup_item(target, label, cmd, &driver))
    .collect()
}

#[tauri::command]
async fn ai_checkup() -> AiCheckupReport {
    tauri::async_runtime::spawn_blocking(|| AiCheckupReport {
        charged: device::get_device_key().map(|d| d.charged).unwrap_or(false),
        items: collect_ai_checkup_items(),
    })
    .await
    .unwrap_or_else(|_| AiCheckupReport { charged: false, items: Vec::new() })
}

/// AI 设置页「一键体检」聚合报告 —— 参考 `claude doctor` / `hermes doctor` 的产品定位：
/// 客户只按一个按钮，拿到一份「这台机器现在能不能好好用 AI」的完整判词，
/// 而不是自己去翻版本号、钱包、环境、一个个工具的配置页。
/// 四类事实一次取齐：① 本体版本/服务器新版 ② 钱包余额 ③ 运行环境 ④ 各 AI 配置状态。
#[derive(Serialize)]
struct DoctorReport {
    update: installer::UpdateInfo,
    wallet: Option<device::DeviceKey>,
    stack: installer::StackDetect,
    tools: Vec<AiCheckupItem>,
}

/// 同步真身（spawn_blocking 里跑）：check_update 会真发网络请求（三条 version URL，
/// curl -m 6）、get_device_key 会查余额 —— 都不能卡 async 运行时。
fn build_doctor_report() -> DoctorReport {
    DoctorReport {
        update: installer::check_update(),
        wallet: device::get_device_key().ok(),
        stack: installer::detect_stack(),
        tools: collect_ai_checkup_items(),
    }
}

#[tauri::command]
async fn doctor_report() -> DoctorReport {
    tauri::async_runtime::spawn_blocking(build_doctor_report)
        .await
        .unwrap_or_else(|_| {
            // spawn_blocking 只有运行时已关才会失败 —— 此时仍尽量给出本地能算的部分，
            // 别返回空壳让前端把「体检坏了」和「运行时没了」混为一谈。
            DoctorReport {
                update: installer::check_update(),
                wallet: None,
                stack: installer::detect_stack(),
                tools: Vec::new(),
            }
        })
}

/// AI 设置「一键升级 CLI 工具」：对单个已装工具重跑安装流水线。
///
/// 为什么「再装一遍」就是升级：npm 步骤装的是 latest、pip 步骤自带 `-U`（installer.rs），
/// 同时白拿安装器全套护栏（便携 Node 预检 / 包名白名单 / 修复循环 / verify 验证）。
/// 日志走 `uking:upgrade` 事件（形状同 install_tool 的 `uking:wizard`），失败自动上报。
#[tauri::command]
async fn upgrade_cli_tool(
    app: AppHandle,
    tool_id: String,
) -> Result<installer::InstallToolResult, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = installer::load_skill();
        // 未安装的先拦下：**没装谈不上升级**，让一键升级逐个跳过而不是白跑一遍流水线。
        // 注意判「装没装」要用 spec.bin（claude-code 的 bin 是 claude），不是 skill id。
        if let Some(spec) = skill.tools.get(&tool_id) {
            if !installer::tool_installed(&spec.bin) {
                return Err(format!("{} 未安装 —— 先去装机向导安装，装了才谈得上升级", spec.name));
            }
        }
        let tid = tool_id.clone();
        let log_store = std::sync::Mutex::new(Vec::<String>::new());
        installer::install_log_header(&format!("upgrade-{tool_id}"));
        let r = installer::install_tool(&skill, &tool_id, &|phase: &str, line: &str| {
            let _ = app2.emit(
                "uking:upgrade",
                serde_json::json!({ "tool": tid, "phase": phase, "line": line }),
            );
            installer::append_install_log(&tid, phase, line);
            let mut l = log_store.lock().unwrap();
            l.push(format!("[{phase}] {line}"));
            if l.len() > 120 {
                l.drain(..40);
            }
        });
        if !r.ok {
            let tail = log_store.lock().unwrap().join("\n");
            report::report_bug(
                "upgrade_failed",
                &format!("{tool_id} 升级失败: {}", r.error.clone().unwrap_or_default()),
                &format!("skill v{} ({})\n{tail}", skill.version, skill.source),
            );
        }
        Ok(r)
    })
    .await
    .map_err(|e| format!("升级任务异常: {e}"))?
}

/// 设备指纹内置 Key（含余额；未充值 charged=false，引导去充值页）。
#[tauri::command]
async fn get_device_key() -> Result<device::DeviceKey, String> {
    tauri::async_runtime::spawn_blocking(device::get_device_key)
        .await
        .map_err(|e| format!("获取设备 Key 异常: {e}"))?
}

/// 生成人话版「AI 体检报告」到桌面 —— 客户能整页截图发售后微信。
/// 借鉴同类做法：`--selfcheck` 出 JSON 给开发看，这个出纯文本给客户/客服看。
/// 汇总运行环境 + 已装工具 + 当前驱动 + 虾盘云账户 + 常见建议，返回写入的文件路径。
#[tauri::command]
async fn save_health_report() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(build_and_write_health_report)
        .await
        .map_err(|e| format!("生成体检报告异常: {e}"))?
}

/// 把 Harness Doctor 的稳定 JSON 压成人话段落。这里只取 summary/status/fix_id，绝不把
/// details 里的绝对路径或任何环境值抄进截图报告；完整排障信息走 Doctor 自己的脱敏 bundle。
fn format_harness_doctor_section(raw: Option<&str>) -> String {
    use std::fmt::Write as _;

    let mut out = String::from("【四大 AI 工具深度体检】\n");
    let Some(raw) = raw else {
        out.push_str("  ⚪ Harness Doctor 未安装（可在下面工具列表一键安装；现有基础体检不受影响）\n");
        return out;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) else {
        out.push_str("  ⚠️ Harness Doctor 返回了无法识别的报告，请重新安装后再试\n");
        return out;
    };
    let summary = &v["summary"];
    let pass = summary["pass"].as_u64().unwrap_or(0);
    let warn = summary["warn"].as_u64().unwrap_or(0);
    let fail = summary["fail"].as_u64().unwrap_or(0);
    let _ = writeln!(out, "  总结：✅ {pass} 通过 · ⚠️ {warn} 提醒 · ❌ {fail} 失败");
    if let Some(checks) = v["checks"].as_array() {
        for item in checks.iter().filter(|item| item["status"].as_str() != Some("pass")).take(12) {
            let mark = if item["status"].as_str() == Some("fail") { "❌" } else { "⚠️" };
            let text = item["summary"].as_str().unwrap_or("未提供说明");
            let fix = item["fix_id"].as_str().map(|id| format!(" · 修复标识 {id}")).unwrap_or_default();
            let _ = writeln!(out, "  {mark} {text}{fix}");
        }
    }
    out.push_str("  完整脱敏支持包：harness-doctor bundle --target all --output harness-support.json\n");
    out
}

fn build_and_write_health_report() -> Result<String, String> {
    use std::fmt::Write as _;

    let s = installer::detect_stack();
    let driver = providers::driver_status();
    let dk = device::get_device_key().ok();
    let clawx = providers::clawx_app_installed();
    let hermes = tools::hermes_app_installed() || installer::tool_installed("hermes");
    let harness_doctor = if installer::tool_installed("harness-doctor") {
        installer::run_tool_capture("harness-doctor --target all --json --no-ports")
            .ok()
            .map(|(_, output)| output)
    } else {
        None
    };

    // ✅ v1.2.3 / ❌ 未检测到
    let probe = |p: &installer::CmdProbe| -> String {
        if p.found {
            format!("✅ {}", p.version.clone().unwrap_or_else(|| "已安装".into()))
        } else {
            "❌ 未检测到".into()
        }
    };
    let yn = |b: bool| if b { "✅ 已安装" } else { "❌ 未检测到" };
    // 把 baseUrl 翻成人话渠道名
    let chan = |base: &Option<String>| -> Option<String> {
        let b = base.as_deref().filter(|b| !b.is_empty())?;
        Some(if providers::is_xiapan_endpoint(b) {
            "虾盘云（内置）".into()
        } else if b.contains("deepseek") {
            "DeepSeek".into()
        } else if b.contains("bigmodel") {
            "智谱 GLM".into()
        } else if b.contains("moonshot") {
            "Kimi".into()
        } else {
            b.replace("https://", "").replace("http://", "")
        })
    };

    let mut r = String::new();
    let ver = env!("CARGO_PKG_VERSION");
    let _ = writeln!(r, "================ U-King · AI 体检报告 ================");
    let _ = writeln!(r, "版本：v{ver}    系统：{} / {}", std::env::consts::OS, std::env::consts::ARCH);
    let _ = writeln!(r, "把这份内容整页截图发给售后微信，可最快定位问题。");
    let _ = writeln!(r);

    let _ = writeln!(r, "【运行环境】");
    let _ = writeln!(r, "  Node.js       {}{}", probe(&s.node), if s.portable_node { "（U-King 便携版）" } else { "" });
    let _ = writeln!(r, "  npm           {}", probe(&s.npm));
    let _ = writeln!(r, "  Git           {}", probe(&s.git));
    match &s.system_proxy {
        Some(p) => { let _ = writeln!(r, "  系统代理       ⚠️ 已开启 {p}（连不上 AI 时先关掉代理软件再试）"); }
        None => { let _ = writeln!(r, "  系统代理       未开启（正常）"); }
    }
    let _ = writeln!(r);

    let _ = writeln!(r, "【AI 工具是否装好】");
    let _ = writeln!(r, "  Claude Code    {}", probe(&s.claude));
    let _ = writeln!(r, "  Codex CLI      {}", probe(&s.codex));
    let _ = writeln!(r, "  Codex 桌面版   {}", yn(s.codex_app));
    let _ = writeln!(r, "  Claude 桌面版  {}", yn(s.claude_desktop));
    let _ = writeln!(r, "  ClawX 桌面版   {}", yn(clawx));
    let _ = writeln!(r, "  Hermes         {}", yn(hermes));
    let _ = writeln!(r);

    r.push_str(&format_harness_doctor_section(harness_doctor.as_deref()));
    let _ = writeln!(r);

    let _ = writeln!(r, "【当前用的模型驱动】");
    let claude_chan = chan(&driver.claude_base);
    let _ = writeln!(
        r,
        "  Claude Code    {}{}",
        claude_chan.clone().unwrap_or_else(|| "官方默认 / 未接管".into()),
        driver.claude_model.as_deref().filter(|m| !m.is_empty()).map(|m| format!(" · {m}")).unwrap_or_default(),
    );
    let _ = writeln!(
        r,
        "  Codex          {}{}",
        driver.codex_provider.as_deref().filter(|p| !p.is_empty()).unwrap_or("官方默认 / 未接管"),
        driver.codex_model.as_deref().filter(|m| !m.is_empty()).map(|m| format!(" · {m}")).unwrap_or_default(),
    );
    if driver.clawx_installed {
        let _ = writeln!(
            r,
            "  ClawX          {}",
            driver.clawx_model.as_deref().filter(|m| !m.is_empty()).map(|m| format!("已接虾盘云 · {m}")).unwrap_or_else(|| "已装 · 未接管（可到②一键接入）".into()),
        );
    }
    let _ = writeln!(r);

    let _ = writeln!(r, "【虾盘云账户】");
    match &dk {
        Some(d) => {
            let head: String = d.key.chars().take(12).collect();
            let _ = writeln!(r, "  内置 Key       {head}…");
            if d.charged {
                let bal = d.balance.as_ref().map(|b| b.text.clone()).unwrap_or_else(|| "可用".into());
                let _ = writeln!(r, "  开通状态       ✅ 已开通 · 余额 {bal}");
            } else {
                let _ = writeln!(r, "  开通状态       ❌ 未开通（到「② 虾盘云·充值」点\"充值开通\"）");
            }
        }
        None => { let _ = writeln!(r, "  内置 Key       ⚠️ 暂时取不到（多为网络问题，稍后重试）"); }
    }
    let _ = writeln!(r);

    // 建议：只列命中的问题，没问题就报平安
    let _ = writeln!(r, "【建议】");
    let mut tips = 0;
    if !s.claude.found && !s.codex.found && !clawx {
        let _ = writeln!(r, "  · 还没装任何 AI 工具 → 回「① 装 AI」点\"一键全安装\"。");
        tips += 1;
    }
    if !s.node.found {
        let _ = writeln!(r, "  · 没检测到 Node.js → 装机时 U-King 会自动装便携版；若反复失败，看「某个工具装不上？」教程。");
        tips += 1;
    }
    if s.system_proxy.is_some() {
        let _ = writeln!(r, "  · 系统代理开着又连不上 AI → 先退出代理/加速器软件，再点\"实测连通\"。");
        tips += 1;
    }
    if claude_chan.is_none() {
        let _ = writeln!(r, "  · Claude 还是\"官方默认\" → 到「② 虾盘云·充值」点\"一键接入虾盘云\"，国内直连。");
        tips += 1;
    }
    if dk.as_ref().map(|d| !d.charged).unwrap_or(false) {
        let _ = writeln!(r, "  · 虾盘云未开通 → 充值后才能聊天/写代码/画图，¥20 起、余额永久有效、不用不扣。");
        tips += 1;
    }
    if tips == 0 {
        let _ = writeln!(r, "  · 一切正常，可以放心使用。有问题随时找售后。");
    }

    // 写桌面（OneDrive 重定向等取不到 Desktop 时退回用户目录）
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "找不到用户主目录".to_string())?;
    let desktop = std::path::Path::new(&home).join("Desktop");
    let dir = if desktop.is_dir() { desktop } else { std::path::PathBuf::from(&home) };
    let path = dir.join("AI体检报告.txt");
    std::fs::write(&path, r).map_err(|e| format!("写体检报告失败: {e}"))?;
    Ok(path.display().to_string())
}

/// AI 修复：让虾盘云上的模型诊断安装失败，返回诊断 + 修复命令（前端确认后再执行）。
#[tauri::command]
async fn ai_diagnose(api_key: String, context: String) -> Result<providers::Diagnosis, String> {
    let r = tauri::async_runtime::spawn_blocking(move || providers::ai_diagnose(&api_key, &context))
        .await
        .map_err(|e| format!("AI 诊断异常: {e}"))?;
    if let Err(e) = &r {
        report::report_bug("ai_diagnose_failed", "AI 诊断失败", e);
    }
    r
}

/// 执行一条 AI 修复命令（带黑名单拦截，日志走 `uking:wizard` 事件）。
#[tauri::command]
async fn run_fix(app: AppHandle, tool_id: String, cmd: String) -> Result<(), String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        installer::run_fix_command(&cmd, &move |phase: &str, line: &str| {
            let _ = app2.emit(
                "uking:wizard",
                serde_json::json!({ "tool": tool_id, "phase": phase, "line": line }),
            );
        })
    })
    .await
    .map_err(|e| format!("修复任务异常: {e}"))?
}

// ============================================================
// 辅助
// ============================================================

/// 优先用本地安装目录的 exe；没装就用当前运行的 exe。
fn preferred_exe_path() -> std::path::PathBuf {
    let local = install::install_dir().join("U-King.exe");
    if local.exists() {
        local
    } else {
        std::env::current_exe().unwrap_or(local)
    }
}

/// 解析命令行 `--open-dir <path>`（右键菜单传入）。
/// 第二次启动被单实例挡回去时，它带来的 `--open-dir` 目录暂存在这里，等前端下一次
/// `get_env` 取走（取出即清）。没有它，客户右键「用 U-King 打开」在窗口已开的情况下
/// 就只是把旧窗口顶到前面，目录悄悄丢了 —— 那比多开一个窗口更让人摸不着头脑。
static PENDING_OPEN_DIR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 从一组参数里取 `--open-dir <路径>`。**独立于 `std::env::args()`** —— 单实例回调拿到的是
/// *另一个进程* 的 argv，用不了本进程的参数。
fn parse_open_dir_from(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--open-dir" {
            if let Some(p) = it.next() {
                if !p.is_empty() {
                    return Some(p.clone());
                }
            }
        }
    }
    None
}

fn parse_open_dir_arg() -> Option<String> {
    // 后来者优先：单实例转发进来的那次点击，比本进程启动时的老参数更能代表用户此刻想干什么。
    if let Some(d) = PENDING_OPEN_DIR.lock().ok().and_then(|mut g| g.take()) {
        return Some(d);
    }
    let args: Vec<String> = std::env::args().collect();
    parse_open_dir_from(&args)
}

// ============================================================
// 入口
// ============================================================

/// `U-King.exe --selfcheck [输出文件]`：无头自检（CI / 开发机校验用），不开窗口。
/// 输出 JSON：环境体检 + skill 加载结果 + 驱动预设；UKING_TEST_HOME 沙箱下还会
/// 实际写入/回读驱动配置走一遍 apply → status 流程。
/// 查沙箱路径有没有漏进真实机器的持久化状态（用户 PATH / shell rc）。
/// 返回漏到了哪些地方；空 = 干净。
fn detect_sandbox_leak(real_home: &str, sandbox: &str) -> Vec<String> {
    let mut leaked = Vec::new();
    #[cfg(windows)]
    {
        // 读注册表原始值，不做展开（展开会把 %VAR% 烤死，是把用户 PATH 写坏的经典手法）。
        let ps = "(Get-Item 'HKCU:\\Environment').GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)";
        if let Ok(o) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .output()
        {
            let cur = String::from_utf8_lossy(&o.stdout).to_string();
            if cur.contains(sandbox) {
                leaked.push("HKCU\\Environment\\Path（用户 PATH）".to_string());
            }
        }
    }
    #[cfg(not(windows))]
    {
        for rc in [".zshrc", ".bashrc", ".bash_profile", ".profile"] {
            let p = std::path::Path::new(real_home).join(rc);
            if std::fs::read_to_string(&p).map(|s| s.contains(sandbox)).unwrap_or(false) {
                leaked.push(p.display().to_string());
            }
        }
    }
    let _ = real_home;
    leaked
}

/// 中文路径装机回归（pc-*** / Issue #222 #223 #318 #319 #323 逼出来的）：
/// 把用户目录指到一个**非 ASCII** 的沙箱，真跑一遍装机流水线。
///
/// 为什么非得单开这条跑道：开发机用户名是 ASCII，于是 `cargo check` / `cargo test` /
/// `--selfcheck` / `action conformance` 在这类 bug 面前**全绿**。而客户机上，我们写进去的
/// pip.ini 是 UTF-8、pip 却按系统 ANSI 代码页（中文 Windows = cp936）读 —— 只有中文用户名
/// 才发作；发作后日志还把它报成「解压不完整」，把人往杀软和网络上引。5 个 issue、至少
/// 3 个客户、跨 4 个版本，我们一次都没自己撞上过。这条跑道就是让发版前先撞一遍。
///
/// 断言三条：
/// ① 一次过（`attempts == 1`）—— 进了修复循环就说明还在空转重下运行时；
/// ② 沙箱里**我们写的 pip 配置必须是纯 ASCII** —— 混一个非 ASCII 字节，这台机器上所有
///    pip 调用都会退出 2，不是某个包装不上而是 pip 整个废掉；
/// ③ verify 真过了（拿到版本号）。
///
/// 跑完删沙箱。注意它会真下载真安装（Hermes ≈ 38MB 便携 Python + 依赖），不是 mock。
fn run_install_test_cjk(tool: &str) -> i32 {
    let sandbox = std::env::temp_dir().join(format!("uking-中文回归-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    if let Err(e) = std::fs::create_dir_all(&sandbox) {
        eprintln!("[FAIL] 建沙箱失败: {e}");
        return 2;
    }
    let sb = sandbox.display().to_string();
    if !sb.is_ascii() {
        eprintln!("[info] 沙箱用户目录（非 ASCII，故意的）: {sb}");
    } else {
        eprintln!("[FAIL] 沙箱路径居然是纯 ASCII，这条跑道就白跑了: {sb}");
        return 2;
    }
    // 记下真实用户目录 —— 跑完要回头断言「我们没碰它」。
    let real_home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    // installer::uking_home() 读 USERPROFILE/HOME；UKING_TEST_HOME 管住其余模块的写入。
    std::env::set_var("USERPROFILE", &sb);
    std::env::set_var("HOME", &sb);
    std::env::set_var("UKING_TEST_HOME", &sb);

    let skill = installer::load_skill();
    let logs = std::sync::Mutex::new(Vec::<String>::new());
    let r = installer::install_tool(&skill, tool, &|phase: &str, line: &str| {
        eprintln!("[{phase}] {line}");
        if let Ok(mut g) = logs.lock() {
            g.push(format!("[{phase}] {line}"));
        }
    });

    // 收集沙箱里所有 pip 配置，逐个验 ASCII（真正的判据不是「装上了」，而是「我们没把它写坏」）。
    let mut bad_cfgs: Vec<String> = Vec::new();
    let mut checked_cfgs: Vec<String> = Vec::new();
    let mut stack = vec![sandbox.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if matches!(p.file_name().and_then(|s| s.to_str()), Some("pip.ini") | Some("pip.conf")) {
                let shown = p.display().to_string();
                checked_cfgs.push(shown.clone());
                match std::fs::read(&p) {
                    Ok(b) if b.is_ascii() => {}
                    Ok(_) => bad_cfgs.push(shown),
                    Err(_) => bad_cfgs.push(format!("{shown}（读不出来）")),
                }
            }
        }
    }

    // ④ 沙箱不许漏到真实机器上。实锤：这条跑道第一次跑就把沙箱的 Scripts 目录写进了开发机
    // 用户 PATH，跑完沙箱一删就留下一条死路径 —— 会污染真实状态的回归跑道，验出来的结论不作数。
    let leaked = detect_sandbox_leak(&real_home, &sb);

    let ok = r.ok && r.attempts == 1 && bad_cfgs.is_empty() && leaked.is_empty();
    let report = serde_json::json!({
        "ok": ok,
        "tool": tool,
        "sandbox_home": sb,
        "install": { "ok": r.ok, "attempts": r.attempts, "version": r.version, "error": r.error },
        "pip_configs_checked": checked_cfgs,
        "pip_configs_non_ascii": bad_cfgs,
        "sandbox_leaked_into": leaked,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    let _ = std::fs::remove_dir_all(&sandbox);
    if ok {
        eprintln!("[OK] 中文路径下装机一次过，且写出的 pip 配置全是纯 ASCII");
        0
    } else {
        eprintln!("[FAIL] 中文路径装机回归未过（见上面 JSON）");
        1
    }
}

fn run_selfcheck(out_path: Option<String>) -> ! {
    // 已迁到影核的探测一律走 actions::run，不再在这里另开一条调用路径 ——
    // 同一事实存在几份就会漂移几份（宪法第 8 条）。JSON 形状与迁移前一致。
    let act = |id: &str| actions::run(id, serde_json::json!({}))
        .unwrap_or_else(|e| serde_json::json!({ "error": e }));
    let detect = act(actions::STACK_INSPECT);
    let skill = installer::load_skill();

    let mut apply_check = serde_json::json!(null);
    let mut custom_check = serde_json::json!(null);
    if std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false) {
        let sandbox = std::env::var("UKING_TEST_HOME").unwrap_or_default();
        // 预置沙箱 .hermes（让 Hermes 也能走真 apply → 覆盖回显回环）。
        // 原始配置指向 deepseek 官方，便于验证「切官方」后 base 不再是虾盘云。
        let hdir = std::path::Path::new(&sandbox).join(".hermes");
        let _ = std::fs::create_dir_all(&hdir);
        if !hdir.join("config.yaml").exists() {
            let _ = std::fs::write(
                hdir.join("config.yaml"),
                "model:\n  provider: deepseek\n  base_url: https://api.deepseek.com/v1\n  default: deepseek-chat\n",
            );
        }
        if !hdir.join(".env").exists() {
            let _ = std::fs::write(
                hdir.join(".env"),
                "OPENAI_API_KEY=sk-orig\nOPENAI_BASE_URL=https://api.deepseek.com/v1\n",
            );
        }
        // 预置 ClawX 配置目录，让 apply_xiapan_everywhere 的自探路径在沙箱里也会覆盖到
        // ClawX + OpenClaw agent 层；真实机器则由 clawx_app_installed() 识别。
        let _ = std::fs::create_dir_all(std::path::Path::new(&sandbox).join("ClawX"));

        let all: [String; 4] = ["claude".into(), "codex".into(), "clawx".into(), "hermes".into()];
        let applied = providers::apply_provider("xiapan", "sk-xp-selfcheck-dummy", None, &all);
        let status = providers::driver_status();
        let active_after_xiapan = status.active.clone();

        // cc-switch 式回显回环：再切「官方还原」，验证 active 表真的翻面（根治 Hermes 老 bug）。
        let reset = providers::apply_provider("official", "-", None, &all);
        let status_official = providers::driver_status();
        let tools = ["claude", "codex", "clawx", "hermes"];
        let want_xiapan = |t: &str| active_after_xiapan.get(t).map(|s| s == "xiapan").unwrap_or(false);
        let want_official = |t: &str| status_official.active.get(t).map(|s| s == "official").unwrap_or(false);
        // 期望：切虾盘云后四个都=xiapan；切官方后四个都=official（Hermes 不再卡在 xiapan）。
        let roundtrip_ok =
            tools.iter().all(|t| want_xiapan(t)) && tools.iter().all(|t| want_official(t));
        let switch_roundtrip = serde_json::json!({
            "active_after_xiapan": active_after_xiapan,
            "active_after_official": status_official.active,
            "hermes_model_after_official": status_official.hermes_model,
            "ok": roundtrip_ok,
            "reset_result": reset.map(|r| serde_json::to_value(r).unwrap()).unwrap_or_else(|e| serde_json::json!({"error": e})),
        });

        // 「一键配好全部」(apply_xiapan_everywhere) 回归校验：这是和 apply_provider 不同的一条路——
        // 后者由调用方显式传 targets，前者后端自己探测决定配谁。这里必须覆盖 Hermes、ClawX，
        // 并回读 ClawX / OpenClaw agent 的实际文件，防止“返回成功但 key 漏写某层”。
        let everywhere = providers::apply_xiapan_everywhere("xiapan", "sk-xp-selfcheck-dummy", None, None);
        let root = std::path::Path::new(&sandbox);
        let read_json = |p: &std::path::Path| -> serde_json::Value {
            std::fs::read_to_string(p)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({}))
        };
        let clawx_cfg = read_json(&root.join("ClawX").join("clawx-providers.json"));
        let openclaw_cfg = read_json(&root.join(".openclaw").join("openclaw.json"));
        let agent_models = read_json(&root.join(".openclaw").join("agents").join("main").join("agent").join("models.json"));
        let agent_auth = read_json(&root.join(".openclaw").join("agents").join("main").join("agent").join("auth-profiles.json"));
        let expected_key = Some("sk-xp-selfcheck-dummy");
        let clawx_file_ok = clawx_cfg.pointer("/defaultProvider").and_then(|v| v.as_str()) == Some("uking-xiapan")
            && clawx_cfg.pointer("/apiKeys/uking-xiapan").and_then(|v| v.as_str()) == expected_key
            && clawx_cfg.pointer("/providerSecrets/uking-xiapan/apiKey").and_then(|v| v.as_str()) == expected_key;
        let openclaw_file_ok = openclaw_cfg.pointer("/models/providers/custom-ukingxia/apiKey").and_then(|v| v.as_str()) == expected_key;
        let agent_models_ok = agent_models.pointer("/providers/custom-ukingxia/apiKey").and_then(|v| v.as_str()) == expected_key;
        let agent_auth_ok = agent_auth.pointer("/profiles/custom-ukingxia:default/key").and_then(|v| v.as_str()) == expected_key;
        let everywhere_json = match &everywhere {
            Ok(r) => serde_json::json!({
                "configured": r.configured,
                "skipped": r.skipped,
                "hermes_included": r.configured.iter().any(|s| s.contains("Hermes")),
                "clawx_included": r.configured.iter().any(|s| s.contains("ClawX")),
                "clawx_needs_restart": r.clawx_needs_restart,
                "clawx_file_ok": clawx_file_ok,
                "openclaw_file_ok": openclaw_file_ok,
                "agent_models_ok": agent_models_ok,
                "agent_auth_ok": agent_auth_ok,
            }),
            Err(e) => serde_json::json!({ "error": e }),
        };

        apply_check = serde_json::json!({
            "apply": applied.map(|r| serde_json::to_value(r).unwrap()).unwrap_or_else(|e| serde_json::json!({"error": e})),
            "status_after": status,
            "switch_roundtrip": switch_roundtrip,
            "apply_all_everywhere": everywhere_json,
        });

        // 自定义 provider 增→列→删 一圈，验证持久化（沙箱内，不污染真实 ~/.uking）
        let demo = providers::ProviderPreset {
            id: String::new(), // 自动 slug
            name: "自检中转站".into(),
            summary: String::new(),
            openai_base: "https://relay.example.com/v1".into(),
            anthropic_base: Some("https://relay.example.com".into()),
            model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
            codex_model: String::new(),
            // 别再写 "chat"：那个值现在会让新版 Codex 整份配置加载失败（#364），
            // 自检夹具拿非法值去跑，读回来的东西也就没法当契约用。
            codex_wire_api: "responses".into(),
            key_url: String::new(),
            key_hint: String::new(),
            builtin_recharge: false,
            recommended: false,
            builtin: false,
            api_key: "sk-relay-demo".into(),
        };
        let saved = providers::save_custom_provider(demo);
        let saved_id = saved.as_ref().map(|p| p.id.clone()).unwrap_or_default();
        let listed_after_add = providers::list_providers().iter().any(|p| p.id == saved_id);
        let reject_builtin = providers::delete_custom_provider("xiapan").is_err();
        let deleted = providers::delete_custom_provider(&saved_id).is_ok();
        let listed_after_del = providers::list_providers().iter().any(|p| p.id == saved_id);
        custom_check = serde_json::json!({
            "saved": saved.map(|p| serde_json::to_value(p).unwrap()).unwrap_or_else(|e| serde_json::json!({"error": e})),
            "in_list_after_add": listed_after_add,
            "reject_delete_builtin": reject_builtin,
            "deleted": deleted,
            "in_list_after_del": listed_after_del,
            "roundtrip_ok": listed_after_add && reject_builtin && deleted && !listed_after_del,
        });
    }

    // 设备指纹内置 Key（含余额查询）
    let device_key = device::get_device_key()
        .map(|d| {
            serde_json::json!({
                "key_prefix": format!("{}…", &d.key[..14.min(d.key.len())]),
                "charged": d.charged,
                "balance": d.balance,
                "recharge_url_ok": d.recharge_url.contains("?key=sk-"),
            })
        })
        .unwrap_or_else(|e| serde_json::json!({"error": e}));
    // 机器指纹来源（ioreg / reg / reg-fallback / machine-id）：debug 判断 Key 漂移时能
    // 一眼看出走的是主路径还是兜底（sol 复审 P2：selfcheck 此前不标来源）。
    let machine_guid_source = device::machine_guid_probe()
        .map(|(_, src)| src)
        .unwrap_or("unavailable");

    // UKING_TEST_KEY：用真实 Key 实测连通（走 app 自己的 curl/JSON 代码路径）
    let mut live = serde_json::json!(null);
    if let Ok(key) = std::env::var("UKING_TEST_KEY") {
        if !key.is_empty() {
            let anth = providers::test_provider("xiapan", &key, None, "anthropic");
            let oai = providers::test_provider("xiapan", &key, None, "openai");
            let bal = providers::query_balance(&key);
            // AI 修复大脑实测：喂一段典型 npm 失败日志，看诊断是否成形
            let diag = providers::ai_diagnose(
                &key,
                "失败工具: Claude Code CLI\n环境体检: node v22 已装, npm 已装\n安装日志尾部:\nnpm error code EEXIST\nnpm error path C:\\nodejs\\claude.cmd\nnpm error EEXIST: file already exists\nnpm error File exists: C:\\nodejs\\claude.cmd\nnpm error Remove the existing file and try again, or run npm with --force",
            );
            // 作图模型探测：候选模型各打一发，看哪个端点 + 模型 id 真出图（确定默认值）
            let img_models = ["gpt-image-2", "seedream-4-0", "wanx2.1-t2i-turbo"];
            let img_probe: Vec<serde_json::Value> = img_models
                .iter()
                .map(|m| match providers::generate_image(&key, "a small red apple on white", m, "1024x1024", None) {
                    Ok(r) => serde_json::json!({ "model": m, "ok": true, "has_b64": r.b64.is_some(), "has_url": r.url.is_some() }),
                    Err(e) => serde_json::json!({ "model": m, "ok": false, "error": e }),
                })
                .collect();

            // 图生图实测：用一张内嵌的小 PNG（64×64 红块）当参考图，真跑 generate_image_edit
            // 的完整 Rust 代码路径（b64_decode→落临时文件→嗅探类型→multipart curl→parse→
            // ensure_b64 下载转 b64）。has_b64=true 即整条链对 live API 验证通过。seedream 出图回 url，
            // 正好压到 ensure_b64 的下载分支。¥0.09/发。
            const TINY_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAeUlEQVR4nO3PQQkAMAzAwMqpfz0TMxF7HINABFzm7H7dcEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFj13zk8EPp8weQAAAAABJRU5ErkJggg==";
            // 用 gpt-image-2（默认/推荐，回 b64 不走境外 CDN）验图生图链路 —— seedream 出图在
            // 境外 CDN，国内裸网下载必失败，拿它当 probe 会一直假阴性（2026-06-22 Mac 实测）。
            let edit_probe = match providers::generate_image_edit(
                &key,
                "把这个红色方块改成蓝色调",
                "gpt-image-2",
                "1024x1024",
                &[TINY_PNG_B64.to_string()],
            ) {
                Ok(r) => serde_json::json!({ "ok": true, "has_b64": r.b64.is_some(), "b64_len": r.b64.as_deref().map(|s| s.len()).unwrap_or(0), "had_url": r.url.is_some() }),
                Err(e) => serde_json::json!({ "ok": false, "error": e }),
            };

            live = serde_json::json!({
                "anthropic": anth,
                "openai": oai,
                "balance": bal.map(|b| serde_json::to_value(b).unwrap()).unwrap_or_else(|e| serde_json::json!({"error": e})),
                "ai_diagnose": diag.map(|d| serde_json::to_value(d).unwrap()).unwrap_or_else(|e| serde_json::json!({"error": e})),
                "image_probe": img_probe,
                "image_edit_probe": edit_probe,
            });
        }
    }

    // UKING_TEST_INSTALL=<tool_id>：真跑一遍安装流水线（steps→npm→verify→repair）
    let mut install_test = serde_json::json!(null);
    if let Ok(tool) = std::env::var("UKING_TEST_INSTALL") {
        if !tool.is_empty() {
            let logs = std::sync::Mutex::new(Vec::<String>::new());
            let r = installer::install_tool(&skill, &tool, &|phase: &str, line: &str| {
                logs.lock().unwrap().push(format!("[{phase}] {line}"));
            });
            let logs = logs.into_inner().unwrap();
            let tail: Vec<&String> = logs.iter().rev().take(25).rev().collect();
            install_test = serde_json::json!({ "result": r, "log_tail": tail });
        }
    }

    // Codex 专区：轻量状态（装了没 + 驱动接管了没）
    let codex_status = act(actions::CODEX_INSPECT);

    // 硬件体检（AI 优化大师的「整机一览」用同一份）
    let hardware = act(actions::HARDWARE_INSPECT);
    // 厨具工具箱：各能力工具装没装
    let toolbox = act(actions::TOOLBOX_INSPECT).get("items").cloned().unwrap_or_default();

    // AI 加速板块：走 runtime.optimizer.inspect（引擎缺失被当结论返回，不是异常）。
    // 先算好再塞进 json! —— json! 里的 `{` 会被当成对象字面量开头，块表达式塞不进去。
    let airuntime_summary = {
        let o = act(actions::OPTIMIZER_INSPECT);
        serde_json::json!({
            "ok": o.get("ok").cloned().unwrap_or(serde_json::Value::Bool(false)),
            "score": o.pointer("/report/score").cloned(),
            "ukrt_version": o.pointer("/report/version").cloned(),
            "error": o.get("error").cloned(),
        })
    };

    let report = serde_json::json!({
        "app": "u-king-mini",
        "version": env!("CARGO_PKG_VERSION"),
        "detect": detect,
        // 装前环境预检：System32/PATH 自修 + fragility 脆弱预检（OneDrive 目录/中文用户名/长路径未开）。
        // warnings 非空即本机有隐性坑；干净机验收 / triage 装机失败时看这段。
        "env_precheck": installer::env_precheck_and_fix(),
        "hardware": hardware,
        "toolbox": toolbox,
        "device_key": device_key,
        "machine_guid_source": machine_guid_source,
        "install_test": install_test,
        "live_test": live,
        "skill": { "source": skill.source, "version": skill.version, "tools": skill.tools.keys().collect::<Vec<_>>() },
        "providers": providers::list_providers().iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        // 真实环境的当前驱动回显（不受沙箱影响）—— 排查「切了没生效」时看这个：
        // hermes_model 走 HERMES_HOME、clawx_model 走 %APPDATA%\ClawX\clawx-providers.json
        "live_driver_status": act(actions::DRIVER_INSPECT),
        // 临时实测开关：UKING_TEST_CLAWX=<model> → 真实切 ClawX 到该模型（测完移除）
        "clawx_apply_test": std::env::var("UKING_TEST_CLAWX").ok().filter(|s| !s.is_empty()).map(|m| {
            let dk = device::device_key_offline().unwrap_or_default();
            let r = providers::apply_provider("xiapan", &dk, Some(&m), &["clawx".to_string()]);
            serde_json::json!({ "model": m, "result": r.map(|x| serde_json::to_value(x).unwrap()).unwrap_or_else(|e| serde_json::json!({"error": e})), "status_after": providers::driver_status().clawx_model })
        }).unwrap_or(serde_json::json!(null)),
        "sandbox_apply": apply_check,
        "custom_provider_roundtrip": custom_check,
        "codex_status": codex_status,
        // AI 加速板块：内嵌 ukrt 释放 + 只读体检（分数 0-100）。干净机验收看这段。
        // 走平台路由 helper（Win→ukrt / Mac→macopt），与 airuntime_doctor 命令一致。
        "airuntime": airuntime_summary,
    });
    let text = serde_json::to_string_pretty(&report).unwrap();
    let path = out_path.unwrap_or_else(|| "uking-selfcheck.json".into());
    let ok = std::fs::write(&path, &text).is_ok();
    std::process::exit(if ok { 0 } else { 1 });
}

// ─────────────────────── 小程序：宿主侧接线 ───────────────────────

/// GUI 侧的宿主能力实现。**API Key 只在这里现形，随即消亡** ——
/// miniapp.rs 没有网络代码也不读 device.json，拿不到的东西泄不了。
struct GuiHost {
    app: AppHandle,
}

impl miniapp::HostBridge for GuiHost {
    fn ai_image_edit(&self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let key = device::device_key_offline()?;
        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|m| !m.trim().is_empty())
            .unwrap_or("gpt-image-2");
        let size = args.get("size").and_then(|v| v.as_str()).unwrap_or("1024x1024");
        let img = args.get("image").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if img.is_empty() {
            return Err("invalid_input: ai.imageEdit 缺少 image".into());
        }
        let r = providers::generate_image_edit(&key, prompt, model, size, &[img])?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    }
    fn ai_image_generate(&self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let key = device::device_key_offline()?;
        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|m| !m.trim().is_empty())
            .unwrap_or("gpt-image-2");
        let size = args.get("size").and_then(|v| v.as_str()).unwrap_or("1024x1024");
        let quality = args.get("quality").and_then(|v| v.as_str());
        let r = providers::generate_image(&key, prompt, model, size, quality)?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    }
    fn ai_chat(&self, _args: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("capability_unavailable: 小程序的对话能力尚未接入".into())
    }
    fn file_save(&self, name: &str, data_url: &str) -> Result<serde_json::Value, String> {
        // 原生「另存为」：路径由**用户**选，小程序无从得知任意路径 —— 这是权限模型的一部分，
        // 不是偷懒。给它一个任意写盘的接口，fs 沙箱就白做了。
        use tauri_plugin_dialog::DialogExt;
        let raw = data_url.rsplit(',').next().unwrap_or(data_url);
        let bytes = qr_merge_b64_decode(raw)?;
        if bytes.is_empty() {
            return Err("要保存的数据是空的".into());
        }
        let name = if name.trim().is_empty() { "uking-miniapp.png" } else { name };
        let dest = self
            .app
            .dialog()
            .file()
            .set_file_name(name)
            .add_filter("PNG 图片", &["png"])
            .blocking_save_file();
        match dest {
            Some(fp) => {
                let p = fp.into_path().map_err(|e| format!("路径无效: {e}"))?;
                std::fs::write(&p, &bytes).map_err(|e| format!("保存失败: {e}"))?;
                Ok(serde_json::json!(p.display().to_string()))
            }
            None => Ok(serde_json::Value::Null),
        }
    }
    fn file_open(&self, filters: &[String]) -> Result<serde_json::Value, String> {
        use tauri_plugin_dialog::DialogExt;
        let exts: Vec<&str> = filters.iter().map(|s| s.as_str()).collect();
        let exts = if exts.is_empty() { vec!["png", "jpg", "jpeg", "webp", "bmp"] } else { exts };
        let picked = self
            .app
            .dialog()
            .file()
            .add_filter("图片", &exts)
            .blocking_pick_file();
        let Some(p) = picked else { return Ok(serde_json::Value::Null) };
        let path = p.into_path().map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&path).map_err(|e| format!("读不到文件: {e}"))?;
        let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        Ok(serde_json::json!({
            "name": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "dataUrl": format!("data:{mime};base64,{}", b64_encode_bytes(&bytes)),
        }))
    }
    fn host_action(&self, id: &str, input: serde_json::Value) -> Result<serde_json::Value, String> {
        actions::run(id, input)
    }
}

fn b64_encode_bytes(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((*c.get(1).unwrap_or(&0) as u32) << 8)
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn json_response(status: u16, v: &serde_json::Value) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .header("cache-control", "no-store")
        .body(v.to_string().into_bytes())
        .unwrap()
}

/// `uking://localhost/…` 的总路由。
fn miniapp_protocol(
    app: &AppHandle,
    path: &str,
    is_post: bool,
    body: &[u8],
) -> tauri::http::Response<Vec<u8>> {
    // 桥脚本
    if path == "__uking/bridge.js" {
        return tauri::http::Response::builder()
            .status(200)
            .header("content-type", "text/javascript; charset=utf-8")
            .header("cache-control", "no-store")
            .body(miniapp::BRIDGE_JS.as_bytes().to_vec())
            .unwrap();
    }
    // 产出箱取图：GUI 拿像素走这里，动作返回的只是引用
    if let Some(id) = path.strip_prefix("artifact/") {
        return match artifacts::read_bytes(id) {
            Some(b) => tauri::http::Response::builder()
                .status(200)
                .header("content-type", "image/png")
                .header("cache-control", "no-store")
                .body(b)
                .unwrap(),
            None => json_response(404, &serde_json::json!({ "ok": false, "error": "artifact not found" })),
        };
    }
    // 能力桥
    if let Some(rest) = path.strip_prefix("rpc/") {
        if !is_post {
            return json_response(405, &serde_json::json!({ "ok": false, "error": "method_not_allowed" }));
        }
        let mut it = rest.splitn(2, '/');
        let app_id = it.next().unwrap_or("");
        let verb = it.next().unwrap_or("");
        let args: serde_json::Value = serde_json::from_slice(body).unwrap_or(serde_json::json!({}));
        let host = GuiHost { app: app.clone() };
        return match miniapp::rpc(app_id, verb, &args, &host) {
            Ok(d) => json_response(200, &serde_json::json!({ "ok": true, "data": d })),
            Err(e) => json_response(200, &serde_json::json!({ "ok": false, "error": e })),
        };
    }
    // 静态资源
    if let Some(rest) = path.strip_prefix("app/") {
        let mut it = rest.splitn(2, '/');
        let app_id = it.next().unwrap_or("");
        let rel = it.next().unwrap_or("");
        let mut s = miniapp::serve(app_id, rel);
        let is_html = s.mime.starts_with("text/html");
        if is_html {
            s.body = miniapp::inject_bridge(&s.body, app_id);
        }
        let csp = miniapp::permissions(app_id)
            .map(|p| miniapp::csp_for(&p))
            .unwrap_or_else(|_| "default-src 'self'".into());
        let mut b = tauri::http::Response::builder()
            .status(s.status)
            .header("content-type", s.mime)
            .header("cache-control", if s.no_store { "no-store" } else { "max-age=60" });
        if is_html {
            // connect-src 'self' 是承重墙：挡住恶意小程序把用户的图外传第三方
            b = b.header("content-security-policy", csp);
        }
        return b.body(s.body).unwrap();
    }
    json_response(404, &serde_json::json!({ "ok": false, "error": "not_found" }))
}

// ─── 小程序：只剩「打开」这一条给宿主用 ───
//
// 2026-08-11 简化：小程序**商店页**（MiniApps.tsx / AppStrip.tsx）已删，
// 随之删掉只有那两个页面在调的四个命令（list_miniapps / install_miniapp_dialog /
// uninstall_miniapp / set_miniapp_pinned）。**运行时和能力全留着** ——
// 泊舟仍可通过影核动作 `runtime.podapp.install` / `.launch` 和 CLI `--miniapp-*` 装、开、更新。
// `open_miniapp` 留下：`--miniapp-open` 走的就是它这条路（和界面点击同一条，不另写一份）。

/// 把终端拉成一个独立窗口（客户 2026-08-18：「终端能拉出来不？做对比之类的」）。
///
/// 用**同一份前端**加 `?pane=terminal` —— `main.tsx` 在入口按这个参数分流，
/// 只挂 `TerminalWindow`，不挂整个 App（两份 App 同时活着会各跑一套定时任务/升级检查）。
///
/// 🔴 **不复用 `open_miniapp` 那条 `uking://` 路**：小程序要的是隔离（独立 origin + 专属 CSP），
/// 终端要的正相反 —— 它得跟主窗口同源，才能调 `term_*` 那批命令。
///
/// ⚠️ 注意「注册表和心跳」是**每个 webview 各一份**，不是共用：`registry.ts` 的 `owned`
/// 和 `main.tsx` 的保活 `setInterval` 都是模块级单例，而两个 webview 是两个独立 JS realm，
/// 各自登记各自报平安。后端 `term_ping(alive)` 只续命列表里命中的会话、不碰别的，
/// 所以两个窗口互不误杀；关掉小窗 = 它那份心跳停 = 它名下的会话按 HEARTBEAT_TIMEOUT 老化回收。
/// 走自定义协议的话它就成了「小程序」，命令一个都调不到。
///
/// 每个工作目录一个窗口（label 带目录哈希）：同一目录再点就把已开的顶到前面，
/// 不然点几次就是几个一模一样的窗口，而每个都带着自己的 PTY。
#[tauri::command]
async fn open_terminal_window(app: AppHandle, cwd: Option<String>, cmd: Option<String>) -> Result<(), String> {
    let dir = cwd.unwrap_or_default();
    // label 只能是 [A-Za-z0-9-_]，中文目录直接当 label 会被 Tauri 拒 —— 用稳定哈希。
    let mut h: u64 = 1469598103934665603;
    for b in dir.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    let label = format!("uking-term-{h:x}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let enc = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    let mut url = format!("index.html?pane=terminal&cwd={}", enc(&dir));
    if let Some(c) = cmd.as_deref().filter(|c| !c.trim().is_empty()) {
        url.push_str(&format!("&cmd={}", enc(c)));
    }
    let title = if dir.is_empty() {
        "U-CLI".to_string()
    } else {
        format!("{} · U-CLI", dir.rsplit(['\\', '/']).next().unwrap_or(&dir))
    };
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(960.0, 640.0)
        .build()
        .map_err(|e| format!("拉出终端窗口失败: {e}"))?;
    Ok(())
}

/// `uking://` 协议被请求到的次数。
///
/// 判据用途：小程序「点了没反应」和「协议压根没被访问」在界面上长得一模一样。
/// 0.9.72 发出去的包就是后者（iframe 跨 scheme 被 WebView2 拒），
/// 而当时没有任何自动化能区分这两种情况 —— `--miniapp-open` 靠这个计数器补上。
static MINIAPP_PROTO_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 打开一个小程序。
///
/// **必须用独立 WebviewWindow，不能用主窗口里的 iframe。**
/// 实测：主窗口在 http(s) 源上，iframe 指向 `uking://` 是跨 scheme，WebView2 直接不放行 ——
/// 表现为容器打开了但永远停在「正在打开…」，而且协议处理器一次都不会被调用
/// （没有日志的话，这和「协议没注册」长得一模一样，极难查）。
/// spike 当初验过的是「uking:// 页面里的 iframe 指向 uking://」——同源，不能推广到主窗口。
#[tauri::command]
async fn open_miniapp(app: AppHandle, id: String) -> Result<(), String> {
    let info = tauri::async_runtime::spawn_blocking({
        let id = id.clone();
        move || miniapp::get(&id)
    })
    .await
    .map_err(|e| e.to_string())??;

    let label = format!("miniapp-{}", info.slug);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let url: tauri::Url = format!("uking://localhost/app/{}/", info.id)
        .parse()
        .map_err(|_| "地址解析失败".to_string())?;
    // label 以 miniapp- 打头 —— invoke_handler 的门禁按这个前缀拒绝宿主命令调用
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::CustomProtocol(url))
        .title(format!("{} · U-King 小程序", info.name))
        .inner_size(1100.0, 760.0)
        .center()
        .build()
        .map_err(|e| format!("打开失败: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn list_artifacts() -> Result<Vec<artifacts::Artifact>, String> {
    tauri::async_runtime::spawn_blocking(artifacts::list)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mark_artifacts_seen(ids: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || artifacts::mark_seen(&ids))
        .await
        .map_err(|e| e.to_string())
}

/// 显示主窗口 —— **Windows 上先挂图标再显示**，顺序不能反。
///
/// 主窗口在 `tauri.conf.json` 里是 `visible:false`：窗口一旦可见，任务栏就把按钮建出来了，
/// 那一刻它拿不到大图标就会退回「按 exe 路径问 shell 要图标」，而**装完立刻被安装程序拉起**
/// 的那一次 shell 那边是冷的 → 按钮定死成通用白纸片，事后补设图标它不认（现场实测，
/// 见 `winicon.rs`）。所以图标必须在 `show()` 之前挂上。
///
/// 幂等：重复调只是重复 show，无副作用。图标挂不上只记一行日志，绝不拦住窗口显示 ——
/// 没图标顶多难看，没窗口就是打不开。
fn show_main_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        eprintln!("[uking] main window NOT FOUND at setup");
        return;
    };
    #[cfg(windows)]
    {
        match w.hwnd().map_err(|e| e.to_string()).and_then(|h| winicon::apply_from_exe(h.0 as isize)) {
            Ok((big, small)) => {
                if !big {
                    ulog::write("ui", "任务栏大图标没挂上（exe 里取不到图标）—— 任务栏会显示系统通用图标");
                }
                let _ = small;
            }
            Err(e) => ulog::write("ui", &format!("窗口图标设置失败：{e}")),
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    eprintln!(
        "[uking] main window: visible={:?} size={:?} pos={:?}",
        w.is_visible(),
        w.inner_size(),
        w.outer_position()
    );
}

/// 把 PATH 上**含有 rtk 的目录**全部摘掉，模拟客户机（rtk 没接进 PATH）的处境。
/// 只摘这些、不清空整条 PATH —— rtk 内部还要调 git / ls，清空了失败的原因就不唯一了。
fn path_without_rtk() -> String {
    let raw = std::env::var("PATH").unwrap_or_default();
    let kept: Vec<_> = std::env::split_paths(&raw)
        .filter(|d| !["rtk", "rtk.exe", "rtk.cmd", "rtk.bat"].iter().any(|n| d.join(n).is_file()))
        .collect();
    std::env::join_paths(kept).map(|s| s.to_string_lossy().to_string()).unwrap_or(raw)
}

/// 在给定 PATH 下用 bash 跑一条命令，回 (退出码, 合并输出)。
fn sh_with_path(bash: &std::path::Path, cmd: &str, path: &str) -> (i32, String) {
    let out = std::process::Command::new(bash)
        .args(["-c", cmd])
        .env("PATH", path)
        .output();
    match out {
        Ok(o) => (
            o.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        ),
        Err(e) => (-1, format!("spawn 失败: {e}")),
    }
}

/// `--rtk-hook-test` 的实现。见调用处的注释。
fn rtk_hook_test() -> i32 {
    let Some(rtk_exe) = rtk::probe_exe_path() else {
        eprintln!("SKIP 本机没装 Token 压缩机（rtk），这条跑道验不了 —— 先 rtk_install");
        return 1;
    };
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("FAIL 拿不到自身路径: {e}");
            return 1;
        }
    };
    // ① 真 spawn 一次自己（客户机上跑的就是这条），喂一条 PreToolUse 报文
    let input = rtk::probe_input_json("git status");
    let mut child = match std::process::Command::new(&exe)
        .arg("rtk-hook")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL 起不来 `rtk-hook`: {e}");
            return 1;
        }
    };
    {
        use std::io::Write;
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(input.as_bytes());
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("FAIL 读 `rtk-hook` 输出失败: {e}");
            return 1;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let mut fails: Vec<String> = Vec::new();

    // ② stdout 必须是干净 JSON（多打一个字节 Claude Code 就解析不了）
    let parsed: Option<serde_json::Value> = serde_json::from_str(stdout.trim()).ok();
    let Some(v) = parsed else {
        eprintln!("FAIL stdout 不是合法 JSON（Claude Code 会直接报错）：{:?}", stdout);
        return 1;
    };
    let cmd = v
        .pointer("/hookSpecificOutput/updatedInput/command")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    println!("改写后命令: {cmd}");
    if !cmd.contains(&rtk_exe.replace('\\', "/")) {
        fails.push(format!("改写后的命令没带 rtk 绝对路径：{cmd}"));
    }

    // ③ 决定性证据：在**摘掉 rtk 的 PATH** 上，裸命令必须失败、改写后的必须成功。
    //    两条一起断言 = 自带变异验证：只留后者的话，改坏了也可能因为开发机 PATH 上
    //    恰好有 rtk 而报绿。
    let scrubbed = path_without_rtk();
    match which_bash() {
        Some(bash) => {
            let (naked_code, naked_out) = sh_with_path(&bash, "rtk git status", &scrubbed);
            if naked_code == 0 {
                fails.push(format!(
                    "对照组没成立：摘掉 rtk 的 PATH 上裸 `rtk` 竟然还跑得通（本跑道此刻证明不了任何事）。输出：{}",
                    naked_out.trim()
                ));
            } else {
                println!("对照组 OK: 裸 `rtk` 在摘净的 PATH 上退出码 {naked_code}（正是客户机现象）");
            }
            // ★ 判据只对**被测对象**敏感：区分「找不到 rtk」（我们要验的）和
            // 「进程压根起不来」（机器的事）。实测发版当天紧跟在全量构建之后跑，
            // 这一步吐过一次 `0xC0000142`（STATUS_DLL_INIT_FAILED，资源压力下起不来进程），
            // 连跑 3 次又全绿 —— **把它算成产品失败就是喊狼**，喊几次之后没人再信这条跑道。
            const DLL_INIT_FAILED: i32 = -1073741502; // 0xC0000142
            let mut attempt = 0;
            let (code, sout) = loop {
                let r = sh_with_path(&bash, &cmd, &scrubbed);
                attempt += 1;
                if r.0 != DLL_INIT_FAILED || attempt >= 3 {
                    break r;
                }
                eprintln!("（第 {attempt} 次遇到 0xC0000142：机器起不来进程，跟被测项无关，重试）");
                std::thread::sleep(std::time::Duration::from_millis(1500));
            };
            if code == 0 {
                println!("改写后命令 OK: 同一条 PATH 上退出码 0 —— PATH 与它无关");
            } else if code == DLL_INIT_FAILED {
                // 重试仍然起不来 = 这台机器此刻跑不了这条跑道。**如实说「没验成」，
                // 而不是说「产品坏了」** —— 两者对读的人意味着完全不同的下一步。
                fails.push(
                    "环境问题，本次没验成：连试 3 次都是 0xC0000142（进程起不来，常见于机器负载高时）。\
                     这**不是**产品失败，等机器闲下来重跑一次"
                        .into(),
                );
            } else {
                fails.push(format!(
                    "改写后的命令在同一条 PATH 上失败（退出码 {code}）：{}",
                    sout.trim()
                ));
            }
        }
        None => fails.push("找不到 bash，验不了「换个 shell 还跑不跑得起来」这半边".into()),
    }

    if fails.is_empty() {
        println!("✓ rtk-hook 全部通过（fail-open + 不依赖 PATH）");
        0
    } else {
        for f in &fails {
            eprintln!("FAIL {f}");
        }
        1
    }
}

/// 找 bash 绝对路径（Windows 上是 Git Bash）。**先解析成绝对路径再 spawn** ——
/// 后面要用改过的 PATH 起它，靠名字解析会解析到哪一个说不准。
fn which_bash() -> Option<std::path::PathBuf> {
    let raw = std::env::var("PATH").unwrap_or_default();
    let names: &[&str] = if cfg!(windows) { &["bash.exe"] } else { &["bash", "sh"] };
    for dir in std::env::split_paths(&raw) {
        for n in names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    #[cfg(windows)]
    for c in ["C:/Program Files/Git/bin/bash.exe", "C:/Program Files (x86)/Git/bin/bash.exe"] {
        let p = std::path::PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// `--help` 的正文。
///
/// **刻意只列入口，不列全部 61 个无头开关** —— 那些是内部跑道，逐条抄进来必然漂移
/// （本仓已经吃过一次「手抄的数字全漂了」的亏，见 CLAUDE.md 的预算那段）。
/// 动作表才是唯一真相源，所以最后一行直接把人/AI 指回 `action list --json`。
fn cli_help_text() -> String {
    // 🔴 路径只报一次，用法行用短名 `u-king-mini`。
    // 这是**终端里给人看的**，跟 llms.txt 那份的取舍相反：那份是 AI 逐条照抄的，
    // 所以每行都写全路径；这份要是每行都塞一遍 48 字符的绝对路径，对齐全毁、根本没法读。
    // Mac 上二进制不在 PATH 上，所以位置必须报，但报一次就够。
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "u-king-mini".into());
    format!(
        "U-King {ver} —— 装在这台机器上的本机 AI 能力层。\n\
         不是聊天机器人，是一组稳定的、带确认门禁的动作。\n\
         \n\
         本机可执行文件（不在 PATH 上时用全路径调）:\n\
         \x20 {exe}\n\
         \n\
         用法（下面的 u-king-mini 换成上面那个路径）:\n\
         \x20 u-king-mini action list --json                      我会哪些动作\n\
         \x20 u-king-mini action describe <id> --json             某个动作的完整签名\n\
         \x20 u-king-mini action run <id> --json --no-input       跑一个只读动作\n\
         \x20 u-king-mini action run <id> --yes --input '<json>'  跑一个要确认的动作\n\
         \x20 u-king-mini action recipes --json                   几个动作怎么组合办成一件事\n\
         \x20 u-king-mini action conformance --json               内建体检（全表跑一遍）\n\
         \x20 u-king-mini mcp serve [--allow-write]               以 MCP 服务端常驻\n\
         \x20 u-king-mini --envfp                                 环境指纹（JSON，非隐私）\n\
         \x20 u-king-mini --version [--json]                      版本\n\
         \n\
         输出约定:\n\
         \x20 stdout 只有最终 JSON（`| jq` 不会被污染），进度和日志一律走 stderr；\n\
         \x20 出错时 JSON 走 stderr、stdout 是空的。退出码 0=成功，非 0=失败。\n\
         \n\
         写动作要不要 `--yes`，看动作表里每条的 confirmation 字段 ——\n\
         不是所有非只读动作都要（浏览器页面内交互就不要）。\n\
         \n\
         给 AI 的完整说明书: ~/.uking/llms.txt\n\
         能力清单以动作表为准，别照文档猜: u-king-mini action list --json\n",
        ver = env!("CARGO_PKG_VERSION"),
    )
}

pub fn run() {
    // 无头自检模式：U-King.exe --selfcheck [out.json]
    let args: Vec<String> = std::env::args().collect();
    // 影核协议通用 CLI：U-King.exe action list|describe|manifest|run <id> --json --no-input
    // 当前切片只提供只读 runtime.command_guard.inspect；未知动作/非法输入返回结构化错误且不副作用。
    // Token 压缩机的 hook 包装器：U-King.exe rtk-hook（读 stdin 出 stdout）。
    //
    // **必须排在所有分支最前面，且这条路径上一个字节都不许往 stdout 多打** ——
    // Claude Code 把 stdout 当 JSON 解析，多一行日志就等于把 hook 弄坏。
    // 它挂在客户的**每一条 Bash 命令**上，所以也不碰 ulog / actions / 单实例那些。
    if args.get(1).map(String::as_str) == Some("rtk-hook") {
        std::process::exit(rtk::run_hook_wrapper());
    }

    // ★ 换 Key 之后把新 Key 写回各 AI 工具 —— 接在组合根，因为只有这儿同时认识
    // 「设备钱包」和「AI 工具」两件事（`device.rs` 不反向依赖 lib.rs，见四铁律）。
    //
    // 🔴 这条线以前是断的。2026-08-19 pc-***：服务端轮换成功、余额搬完，客户端只更新了
    // `~/.uking/device.json`，6 个落点里还全是被吊销的旧 Key → 每次调用 401。而它长得跟
    // 「余额烧光」一模一样（new-api 两种都报「Invalid token」），所以没人往「Key 没写回去」
    // 上想。**记完账不落地，等于没换。**
    //
    // 注册排在所有分支之前：轮换既可能由 GUI 触发，也可能在 CLI / 动作路径上收尾 pending。
    // 走驱动动作核心而不是自己写文件；只更新当前实际使用设备钱包的消费者，
    // 绝不顺手覆盖用户的官方登录、自备 Key 或其它中转。
    device::set_wallet_consumer_hook(Box::new(|new_key: Option<&str>| {
        let targets = device_wallet_consumer_targets();
        let verb = if new_key.is_some() { "写回" } else { "清理" };
        let result = sync_device_wallet_consumers(new_key);
        match &result {
            Ok(()) => ulog::write("device", &format!("设备钱包消费者已{verb}：[{}]", targets.join(","))),
            Err(e) => ulog::write("device", &format!("设备钱包消费者{verb}失败：{e}")),
        }
        result
    }));
    // ★ 自我介绍：U-King --version / --help
    //
    // 🔴 以前这六种写法（--version / -V / version / --help / -h / help）**全是零字节输出
    // + 退出码 0**。对一个主打「AI 可直接调用」的 CLI，这是最基本的介绍缺失：
    // AI 摸不到版本号就没法判断该按哪份说明书办事，也没有任何入口能问出「你会什么」。
    //
    // 排在 rtk-hook 之后、其余所有分支之前 —— rtk-hook 挂在客户的每一条 Bash 命令上，
    // 那条路径上多打一个字节就把 hook 弄坏了，它必须永远第一。
    if args.iter().skip(1).any(|a| matches!(a.as_str(), "--version" | "-V" | "version")) {
        if args.iter().any(|a| a == "--json") {
            println!(
                "{}",
                serde_json::json!({
                    "name": "u-king-mini",
                    "version": env!("CARGO_PKG_VERSION"),
                    "platform": if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" },
                    "exe": std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
                })
            );
        } else {
            println!("u-king-mini {}", env!("CARGO_PKG_VERSION"));
        }
        std::process::exit(0);
    }
    if args.iter().skip(1).any(|a| matches!(a.as_str(), "--help" | "-h" | "help")) {
        // 明确要了帮助就不是错误：正文走 stdout、退出码 0（Unix 惯例）。
        print!("{}", cli_help_text());
        std::process::exit(0);
    }
    // MCP 服务端：U-King.exe mcp serve [--allow-write]
    // 影核协议的第三个面（桌面 / CLI / MCP），三面共用同一份动作核心。
    if args.get(1).map(String::as_str) == Some("mcp") && args.get(2).map(String::as_str) == Some("serve") {
        actions::set_audit(|l| ulog::write("actions", l));
        actions::set_record(journal::record_action);
        actions::set_source("mcp");
        std::process::exit(mcp_serve::serve(args.iter().any(|a| a == "--allow-write")));
    }
    if args.get(1).map(String::as_str) == Some("action") {
        // 动作流水记到 ~/.uking/logs/actions.log。注入而非让 actions.rs 直接 import ulog ——
        // 协议核心保持零业务依赖（见 actions::set_audit 的注释）。
        actions::set_audit(|l| ulog::write("actions", l));
        actions::set_record(journal::record_action);
        actions::set_source("cli");
        let sub = args.get(2).map(String::as_str).unwrap_or("");
        let result: Result<serde_json::Value, String> = match sub {
            "list" => serde_json::to_value(actions::list()).map_err(|e| e.to_string()),
            "describe" => args
                .get(3)
                .ok_or_else(|| "invalid_input: action id required".to_string())
                .and_then(|id| serde_json::to_value(actions::describe(id)?).map_err(|e| e.to_string())),
            "manifest" => Ok(actions::manifest()),
            // 配方清单：「几个动作怎么组合能办成一件事」。跟 manifest 里的 `recipes` 段同一份。
            // U-King.exe action recipes --json
            "recipes" => Ok(actions::recipe_list()),
            // 通用回归跑道：把动作表里每个只读无入参的动作真跑一遍，按 output_schema 断言形状。
            // 加动作 = 自动多一条冒烟测试，不必再往 main() 里堆 `--xxx-test`。
            // U-King.exe action conformance [--only runtime.]
            // 绑定核对：U-King.exe action bindings [--src <前端源码目录>]
            // 开发期用。挂了 data-action-id 却没人核对 = 没挂。
            "bindings" => {
                let src = args
                    .iter()
                    .position(|a| a == "--src")
                    .and_then(|i| args.get(i + 1))
                    .cloned()
                    .unwrap_or_else(|| "src".into());
                let report = actions::bindings(&src);
                println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
                // 只有 stale（源码里的 id 在动作表里不存在）才算失败；
                // 「有动作没按钮」是实话不是错 —— 有些动作本来就只给 CLI/AI 用。
                std::process::exit(if report.get("ok") == Some(&serde_json::Value::Bool(true)) { 0 } else { 1 });
            }
            // 支持包：U-King.exe action bundle [--out x.json] [--no-redact]
            // 远程维护一条命令拿走整机现状（动作探针 + 全部模块日志尾部），
            // 不用再敲二十条 PowerShell 去东拼西凑。默认脱敏。
            "bundle" => {
                let redact = if args.iter().any(|a| a == "--no-redact") {
                    None
                } else {
                    Some(feedback::desensitize as fn(&str) -> String)
                };
                let report = actions::bundle(ulog::all_tails(4096), redact);
                let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into());
                match args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)) {
                    Some(p) => {
                        if let Err(e) = std::fs::write(p, &text) {
                            eprintln!("{}", serde_json::json!({ "ok": false, "error": format!("写不了 {p}: {e}") }));
                            std::process::exit(2);
                        }
                        // stdout 只出结果：给个一行摘要，方便远程侧不下载也能先看一眼。
                        println!("{}", serde_json::json!({
                            "ok": report.get("ok"), "out": p,
                            "probes": report["actions"]["probed"], "failed": report["actions"]["failed"],
                            "bytes": text.len(),
                        }));
                    }
                    None => println!("{text}"),
                }
                // 探针失败是**情报不是工具故障**（客户没装 Ollama 很正常），采集本身成功就退 0。
                std::process::exit(0);
            }
            "conformance" => {
                let only = args
                    .iter()
                    .position(|a| a == "--only")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str);
                let report = actions::conformance(only);
                println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
                // 退出码语义与其它无头模式一致：0=全绿，1=有动作没通过（2 留给协议层错误）。
                std::process::exit(if report.get("ok") == Some(&serde_json::Value::Bool(true)) { 0 } else { 1 });
            }
            // 入参：--input-file <path> 优先，其次 --input <json>，都没有就是空对象。
            // 在 Windows 上 --input-file 不是「便利」而是刚需 —— 图片 data URL 动辄几 MB，
            // 远超 CreateProcess 的 32KB 命令行上限（同 providers.rs 里 `-F "prompt=<file"` 的手法）。
            "run" => (|| -> Result<serde_json::Value, String> {
                let raw: Option<String> = match args.iter().position(|a| a == "--input-file") {
                    Some(i) => {
                        let p = args.get(i + 1).ok_or("invalid_input: --input-file 后面要跟路径")?;
                        Some(std::fs::read_to_string(p).map_err(|e| format!("invalid_input: 读不到 {p}: {e}"))?)
                    }
                    None => args
                        .iter()
                        .position(|a| a == "--input")
                        .and_then(|i| args.get(i + 1))
                        .cloned(),
                };
                let mut input: serde_json::Value = match raw {
                    Some(s) => serde_json::from_str(&s)
                        .map_err(|e| format!("invalid_input: 入参不是合法 JSON: {e}"))?,
                    None => serde_json::json!({}),
                };
                // `--yes` = 显式确认这台机器可以被改（等价于 GUI 里点了那个按钮）。
                // 没有它，写动作会被核心的 confirmation_required 挡下 —— 这正是想要的：
                // 脚本/AI 顺手调一下改不动机器，非得写明白 --yes 才行。
                //
                // 🔴 **确认只走 `--yes` 这一条路**（0.9.99 补）：在这之前
                // `--input '{"confirm":true}'` 同样算确认，于是上面那句「非得写明白
                // --yes 才行」是假的 —— 2026-08-14 一次本意只想探入参校验的调用
                // （`--input '{"confirm":true,"targets":["pi"]}'`，没带 --yes）
                // **真的改了开发机上 Claude Code 和 pi 的驱动配置**。
                // GUI 那条路早就认这个理（`standard_confirmation_cannot_be_bypassed_through_legacy_input`
                // 钉着「input 里塞 confirm 不算确认」），CLI 却认，两个界面对同一件事两套规矩。
                //
                // 它**挡不住**一个铁了心要改机器的 AI（`--yes` 一样打得出来），
                // 换来的是「这次调用有没有得到确认」只有一个地方能看 ——
                // 而不是两个，其中一个还藏在 JSON 里。MCP 那条路不动：
                // 它没有第二条确认通道，`--allow-write` 才是那里的人类授权。
                if let Some(o) = input.as_object_mut() {
                    o.remove("confirm");
                    if args.iter().any(|a| a == "--yes") {
                        o.insert("confirm".into(), serde_json::Value::Bool(true));
                    }
                }
                let id = args.get(3).ok_or("invalid_input: action id required")?;
                // 进度走 **stderr**，stdout 只放最终 JSON —— 管道里 `| jq` 不会被日志污染
                // （宪法第 14 条：stdout 只出结果、stderr 出日志）。
                actions::run_with_progress(id, input, &|m: &str| eprintln!("[progress] {m}"))
            })(),
            _ => Err("invalid_input: use action list|describe|manifest|run|bundle|conformance|bindings".into()),
        };
        match result {
            Ok(value) => {
                println!("{}", serde_json::to_string(&value).unwrap_or_else(|_| "{}".into()));
                std::process::exit(0);
            }
            Err(error) => {
                // 错误也走契约：带 code/blame/retriable/hint，调用方（AI/脚本）不用再拿正则猜
                // 「这该重试吗 / 这是谁的错」。blame=bug 才值得开 issue。
                let e = actions::ActionError::classify(&error);
                eprintln!("{}", serde_json::json!({ "ok": false, "error": e }));
                std::process::exit(2);
            }
        }
    }
    // 小程序运行时自检：U-King.exe --miniapp-test
    // 装→列→跑→卸全在临时目录里做，绝不碰真实 ~/.uking（同 mcp_test_headless 的沙箱做法）。
    if args.iter().any(|a| a == "--miniapp-test") {
        std::process::exit(miniapp::selftest());
    }
    // 补装内置小程序：U-King.exe --miniapp-ensure
    // 正常由首启后台线程做；这里给一个无头入口——客户「小程序不见了 / 装坏了」时一条命令补回来，
    // 也是这条路径唯一能被自动化验证的地方（GUI 那次调用没法在 CI 里跑）。幂等。
    if args.iter().any(|a| a == "--miniapp-ensure") {
        bundled_apps::ensure_installed(&|m| println!("{m}"));
        let n = miniapp::list().len();
        println!("[miniapp-ensure] 完成，现有 {n} 个小程序");
        std::process::exit(0);
    }
    // `--miniapp-list` 已删（2026-08-18）—— 影核动作 `runtime.miniapp.inspect` 严格覆盖了它：
    //   U-King.exe action run runtime.miniapp.inspect --json
    // 那个开关唯一独有的本事是报告「目录在、清单读不出」的坏小程序（`list()` 静默跳过它们），
    // 现在成了动作的 `broken[]` + blockers，还顺带被 `action conformance` 自动盖住 ——
    // 手写开关是永远不会被 conformance 盖住的，这正是「别再加开关」那条的由来。
    // 数据基台无头入口（宪法第 14 条：界面之外必须留一条给机器走的路）：
    //   U-King.exe --metrics-report [天数]   本地数据报告（JSON）
    //   U-King.exe --envfp                   环境指纹（JSON，非隐私，可直接贴给客服）
    // stdout 只出 JSON，方便 `| jq` 和 CI 断言；退出码 0=成功。
    if let Some(i) = args.iter().position(|a| a == "--metrics-report") {
        let days = args.get(i + 1).and_then(|s| s.parse::<i64>().ok()).unwrap_or(30);
        // 先补一次当天快照 —— 否则刚装完跑这条会看到一份空报告，让人以为坏了
        metrics_rollup_now();
        let env = serde_json::to_value(envfp::current()).unwrap_or(serde_json::Value::Null);
        let r = metrics::report(days, env);
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_else(|_| "{}".into()));
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--envfp") {
        let fp = envfp::detect();
        println!("{}", serde_json::to_string_pretty(&fp).unwrap_or_else(|_| "{}".into()));
        std::process::exit(0);
    }
    // ★ Hermes 落点诊断：U-King.exe --hermes-where
    //
    // 客户报 Hermes 报错（尤其 404 / not configured）时**先要这条**。它把两件事摆在一起：
    // Hermes 自己算出来的家目录里现在是什么端点、我们历史上写错的那个目录里是什么。
    // pc-*** 那次全靠人肉读两份 config.yaml 才看出错配 —— 界面一路显示「配置成功」，
    // 因为文件确实写成功了，只是写在了 Hermes 不读的地方。
    //
    // 输出非隐私（只有路径和端点，Key 不出），可直接让客户贴过来。
    // 退出码：0 = 没这类错配 / 1 = 检出错配（脚本可直接判）。
    // 带 `--fix` 时先真跑一次迁移再出报告 —— 两个用处：
    //   ① 客服可以让客户跑一条命令当场自救，不必等下一次启动、也不必等发版；
    //   ② **这是「启动时会自动修」那条链唯一的无头验证入口**。迁移函数本身有单测，
    //      但「装进 GUI 启动线程后还真的跑不跑得起来」一个字节都不在单测里。
    if args.iter().any(|a| a == "--hermes-where") {
        if args.iter().any(|a| a == "--fix") {
            match providers::migrate_hermes_config_from_legacy() {
                Some(msg) => eprintln!("[fix] {msg}"), // 进度走 stderr，stdout 只留 JSON
                None => eprintln!("[fix] 无需迁移"),
            }
        }
        let r = providers::hermes_where();
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_else(|_| "{}".into()));
        std::process::exit(if r["mismatch"].as_bool().unwrap_or(false) { 1 } else { 0 });
    }
    // ★ 被管理契约落点检查：U-King.exe --org-where
    // 企业版第一层。个人版默认 unmanaged（退出码 0）。stdout 只出 JSON。
    // 退出码：0 = 正常（含 unmanaged）/ 1 = managed 却缺 org_id（配置不一致）。
    if args.iter().any(|a| a == "--org-where") {
        let r = org::inspect_json();
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_else(|_| "{}".into()));
        let inconsistent = r["mode"].as_str() == Some("managed") && r["org_id"].is_null();
        std::process::exit(if inconsistent { 1 } else { 0 });
    }
    if let Some(i) = args.iter().position(|a| a == "--selfcheck") {
        // 输出路径是**位置参数**（`--selfcheck [out.json]`）。下一个 token 若还是个
        // 开头的 flag，那是调用方在传别的选项，不是路径 —— 曾经有人跑
        // `--selfcheck --json`，结果在仓库根创建出一个名叫 `--json` 的文件。
        // 拿不准就落默认文件名，也别拿 flag 当文件名用。
        let out = args
            .get(i + 1)
            .filter(|p| !p.starts_with('-'))
            .cloned();
        run_selfcheck(out);
    }
    // Codex 省钱路由无头验证：U-King.exe --codex-route-test
    // 真跑一遍 start→status→stop 的代码路径，打印写出的 config.toml。
    // **必须设 CODEX_HOME 指向沙箱**，否则会覆盖你自己的 ~/.codex/config.toml。
    if args.iter().any(|a| a == "--codex-route-test") {
        if std::env::var("CODEX_HOME").map(|v| v.trim().is_empty()).unwrap_or(true) {
            eprintln!("[FAIL] 请先设 CODEX_HOME 指向沙箱目录，否则会改掉你真实的 codex 配置");
            std::process::exit(2);
        }
        let started = codex_proxy::codex_proxy_start(None);
        println!("start -> {}", serde_json::to_string(&started).unwrap_or_default());
        let st = codex_proxy::codex_proxy_status();
        println!("status -> {st}");
        let home = std::env::var("CODEX_HOME").unwrap_or_default();
        let cfg = std::fs::read_to_string(std::path::Path::new(&home).join("config.toml")).unwrap_or_default();
        println!("--- config.toml ---\n{cfg}");
        // 断言直连形态：认得出是我们写的、不再指本地端口、走 responses
        let direct = st.get("direct").and_then(|v| v.as_bool()).unwrap_or(false);
        let ok = direct
            && cfg.contains("uking_deepseek")
            && !cfg.contains(&format!("127.0.0.1:{}", codex_proxy::PROXY_PORT))
            && cfg.contains("wire_api = \"responses\"");
        println!("{}", if ok { "[OK] 直连配置就位，无本地代理" } else { "[FAIL] 配置形态不对" });
        std::process::exit(if ok { 0 } else { 1 });
    }
    // Anthropic↔OpenAI 翻译桥：U-King.exe --bridge-test
    //
    // 验的是**翻译之外**的那半边 —— 翻译逻辑归 `claude-openai-proxy.selftest.mjs`（45 条断言），
    // 这里只管 Rust 侧：找不找得到 Node、脚本落不落得下去、端口起不起得来、
    // `/health` 认不认我们、停完是不是真停了。这两半都不在对方的覆盖范围里。
    // **不发一个上游请求**（upstream 是个假地址，健康检查不碰它），零 token、可离线跑。
    if args.iter().any(|a| a == "--bridge-test") {
        let before = claude_proxy::status();
        println!("起桥前: running={} ready={} blockers={:?}", before.running, before.ready, before.blockers);
        let mut bad = 0;
        match claude_proxy::start("https://例子.invalid/v1", "", "") {
            Ok(st) => {
                println!("起桥后: {}", serde_json::to_string(&st).unwrap_or_default());
                if !st.running {
                    eprintln!("[FAIL] 起来了却报 running=false");
                    bad += 1;
                }
                if !st.ready || !st.blockers.is_empty() {
                    eprintln!("[FAIL] ready/blockers 对不上：{:?}", st.blockers);
                    bad += 1;
                }
                // 产品边界当数据发：这一位是给 GUI/CLI/MCP 共用的同一句话，丢了就会三处跑偏
                if !st.runs_only_while_app_open {
                    eprintln!("[FAIL] runs_only_while_app_open 必须为 true —— U-King 一退桥就没了");
                    bad += 1;
                }
                if !st.upstream.ends_with("/chat/completions") {
                    eprintln!("[FAIL] 上游端点没归一：{}", st.upstream);
                    bad += 1;
                }
            }
            Err(e) => {
                eprintln!("[FAIL] 起桥失败: {e}");
                bad += 1;
            }
        }
        let _ = claude_proxy::stop();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let after = claude_proxy::status();
        if after.running {
            eprintln!("[FAIL] stop() 之后端口上还有东西在应答 —— 桥没停干净");
            bad += 1;
        }
        println!("停桥后: running={}", after.running);
        println!("{}", if bad == 0 { "[OK] 翻译桥 Rust 侧跑道全过" } else { "[FAIL] 见上" });
        std::process::exit(if bad == 0 { 0 } else { 1 });
    }
    // 中文路径装机回归：U-King.exe --install-test-cjk [tool_id]（默认 hermes）
    if let Some(i) = args.iter().position(|a| a == "--install-test-cjk") {
        let tool = args
            .get(i + 1)
            .filter(|s| !s.starts_with("--"))
            .cloned()
            .unwrap_or_else(|| "hermes".into());
        std::process::exit(run_install_test_cjk(&tool));
    }
    // ★ Token 压缩机 hook 的无头取证：证明改写出来的命令**不依赖 PATH 也跑得起来**。
    //
    // 为什么非单开这条不可：客户机上的失败长这样 —— 状态面板一切正常（installed/enabled
    // 都是 true），`cargo test` 全绿，`action conformance` 全绿，而那台机器上**每一条**
    // Bash 命令都退出码 127。坏掉的东西一个字节都不在动作表里，它在「hook 吐出来的那行
    // 字符串，被另一个 shell 执行时能不能解析到程序」这件事上。
    //
    // 判据刻意设计成**自带变异验证**：同一条 PATH 下，裸 `rtk …` 必须失败、
    // 改写后的必须成功。只断言后者的话，把绝对路径替换删掉、而恰好开发机 PATH 上有 rtk，
    // 这条跑道会照样报绿 —— 那正是老版本给自己发假绿灯的那个姿势。
    //
    // 🔴 **这条不许加 `#[cfg(windows)]`**。它要治的就是一个 macOS 独有的 bug ——
    // 第一版恰好把它插进了下面那条 `#[cfg(windows)]` 和它修饰的 `if` 中间，于是
    // 「为修 Mac bug 写的跑道」在 Mac 上被编译掉了，而 Windows 侧 cargo check /
    // cargo test / release 构建**全绿，一个字都没提**（顺带还把 `--pwsh-test` 的
    // Windows 门弄丢，Mac CI 才炸出来）。跑道被编译掉 = 没有跑道。
    if args.iter().any(|a| a == "--rtk-hook-test") {
        std::process::exit(rtk_hook_test());
    }
    // 便携 PS7 无头验证：U-King.exe --pwsh-test（强制走下载路径，验证 OSS 下载+SHA+解压+便携命中）
    #[cfg(windows)]
    if args.iter().any(|a| a == "--pwsh-test") {
        match installer::ensure_pwsh(&|_l, m| println!("{m}"), true) {
            Ok(p) => {
                println!("PWSH_READY {p}");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("pwsh-test 失败: {e}");
                std::process::exit(1);
            }
        }
    }
    // ★ 升级兜底无头验证：U-King.exe --reinstall-test
    //
    // 验的是「自动升级换不动时改走覆盖安装」那条**新链路的下载半边**：镜像可达、包是真 PE、
    // 落盘路径对。**故意不启动安装程序** —— 那一步会把当前这台机器上的 U-King 换掉，
    // 不是一个能随手跑的回归。装的那半边只能靠干净机实测（见 aliyun-clean-windows-test）。
    //
    // 为什么非要单开这条：这条路只有在**自动升级已经失败**的机器上才会露出来，
    // 而那种机器我们手上一台都没有 —— 不留无头入口，它就只能等客户来替我们验。
    #[cfg(windows)]
    if args.iter().any(|a| a == "--reinstall-test") {
        match installer::download_installer(&|phase, pct| eprintln!("[{phase}] {pct}%")) {
            Ok(p) => {
                let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                println!("{}", serde_json::json!({ "ok": true, "path": p.to_string_lossy(), "bytes": bytes }));
                // 回归跑完不留垃圾：验的是「下得下来、是个真 exe」，包本身没有留存价值。
                let _ = std::fs::remove_file(&p);
                std::process::exit(0);
            }
            Err(e) => {
                println!("{}", serde_json::json!({ "ok": false, "error": e }));
                std::process::exit(1);
            }
        }
    }
    // 反馈诊断无头验证：U-King.exe --feedback-test
    // 打印一份**已脱敏**的诊断正文（就是「技术支持」页会带上、也会随 issue 上报的那份），
    // 用来确认新加的采集段真的落进了正文、且没有把 Key/路径漏出去。全程只读。
    if args.iter().any(|a| a == "--feedback-test") {
        // 走影核动作，与「技术支持」页、与 `action run runtime.diagnostics.collect` 同一条路径。
        let d = actions::run(actions::DIAGNOSTICS_COLLECT, serde_json::json!({}))
            .ok()
            .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
            .unwrap_or_default();
        println!("{d}");
        // 采集不该产出空壳；缺了「AI进程取证」说明取证段没接上。
        let ok = d.contains("AI进程取证") && d.contains("版本:");
        std::process::exit(if ok { 0 } else { 1 });
    }
    // 远程协助无头验证：U-King.exe --assist-test [保持秒数]
    // 走「开启远程协助」按钮的**同一条代码路径**（下载 agent.exe → 起进程 → 查状态 → 停止），
    // 打印协助编号。默认起完就停；给个秒数则先保持那么久，方便运维侧同时跑
    // `remote-agent.exe list` 确认这台机器真的出现在在线列表里（端到端证据，不只是「编译过了」）。
    if let Some(i) = args.iter().position(|a| a == "--assist-test") {
        let hold: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        match remote_assist::start(&|m: &str| eprintln!("[assist] {m}")) {
            Ok(st) => {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": st.running,
                        "device_id": st.device_id,
                        "remaining_secs": st.remaining_secs,
                        "audit_log": st.audit_log,
                    })
                );
                if hold > 0 {
                    eprintln!("[assist] 保持 {hold}s（这期间可在运维侧跑 remote-agent.exe list 核对）…");
                    std::thread::sleep(std::time::Duration::from_secs(hold));
                }
                // 自检不该给机器留下一个还开着的远程通道 —— 无论如何都关掉再退出。
                let stopped = remote_assist::stop().is_ok();
                let after = remote_assist::status();
                eprintln!("[assist] 已停止={stopped} 停止后 running={}", after.running);
                std::process::exit(if st.running && stopped && !after.running { 0 } else { 1 });
            }
            Err(e) => {
                eprintln!("[assist] 失败: {e}");
                std::process::exit(1);
            }
        }
    }
    // ★ 工具可用性实测：U-King.exe --toolstack-probe [tool] [out.json]
    //
    // 「该下架谁」的三个数字里，**这条回答「配了能不能用」**。发版前跑一遍，
    // 每个已装工具真发一句话看它回不回 —— Crush 那个配了半年没生效的 bug，
    // 就是因为没有这条跑道才漏的（形状对不等于能用）。
    //
    // 进度打 stderr、结果 JSON 打 stdout（`| jq` 不会被日志污染）；
    // 有任何**已装但跑不通**的工具 → 退出码 1（没装的不算失败，那是事实不是故障）。
    if let Some(i) = args.iter().position(|a| a == "--toolstack-probe") {
        let rest: Vec<&String> = args.iter().skip(i + 1).filter(|a| !a.starts_with("--")).collect();
        let only = rest.first().filter(|s| !s.ends_with(".json")).map(|s| s.as_str());
        let out_path = rest.iter().find(|s| s.ends_with(".json"));
        // `--sandbox`：先在一个干净沙箱里跑一遍「一键配好全部」，再用沙箱 env 实测。
        // **这才是发版回归该跑的那条** —— 它测的是「我们配出来的能不能用」，
        // 跟这台机器上原本是什么状态无关。不加 --sandbox 则用真实配置跑（排障口径）。
        let sandbox = if args.iter().any(|a| a == "--sandbox") {
            let dir = std::env::temp_dir().join(format!("uking-probe-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            eprintln!("沙箱: {}", dir.display());
            std::env::set_var("UKING_TEST_HOME", &dir);
            let key = match device::device_key_offline() {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("拿不到设备 Key: {e}");
                    std::process::exit(2);
                }
            };
            match providers::apply_xiapan_everywhere("xiapan", &key, None, None) {
                Ok(r) => eprintln!("已在沙箱配好: {}", r.configured.join(" / ")),
                Err(e) => {
                    eprintln!("沙箱配置失败: {e}");
                    std::process::exit(2);
                }
            }
            std::env::remove_var("UKING_TEST_HOME");
            Some(dir)
        } else {
            None
        };
        let results = toolprobe::probe_all(only, sandbox.as_deref(), &|m| eprintln!("  {m}"));
        for r in &results {
            // 只有**真跑过**的才落 metrics —— 「没装」和「没有无头入口」都不是可用性数据，
            // 记进去会把分母搅浑，进而把「我们测不了」算成「它不能用」。
            if r.probed {
                metrics::log_tool_probe(&r.tool, r.ok, r.ms, &r.note);
            }
        }
        let broken: Vec<&str> =
            results.iter().filter(|r| r.probed && !r.ok).map(|r| r.tool.as_str()).collect();
        let unprobeable: Vec<&str> = results
            .iter()
            .filter(|r| r.installed && !r.probed)
            .map(|r| r.tool.as_str())
            .collect();
        let json = serde_json::json!({
            "results": results,
            "installed": results.iter().filter(|r| r.installed).count(),
            "probed": results.iter().filter(|r| r.probed).count(),
            "ok": results.iter().filter(|r| r.ok).count(),
            "broken": broken,
            // 装了但这条跑道测不了的（如 openclaw 只能走 gateway）。**不算失败**，
            // 但必须如实列出来 —— 沉默会让人以为「全测过了」（同 conformance 的 not_ready 段）。
            "unprobeable": unprobeable,
        });
        let text = serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".into());
        if let Some(p) = out_path {
            let _ = std::fs::write(p.as_str(), &text);
        }
        println!("{text}");
        std::process::exit(if broken.is_empty() { 0 } else { 1 });
    }

    // 竞技场无头跑道：U-King.exe --arena-test
    //
    // 竞技场一跑就烧 token（六个 CLI 同任务真跑），**测试不能点火**。这条跑道传一个
    // 不在参赛名单里的工具名，验证 run_arena 骨架但不真调模型：
    //   ① 工作副本隔离（每个参赛者一个独立子目录 `arena/<tool>/`，不共享工作区）
    //   ② 结果形状（对每个工具都有一条 ArenaResult；名单外工具如实报「未安装」不是坏）
    //   ③ 临时目录用完全自删，不污染真实机器
    // 真 spawn/超时/杀树那半边由 toolprobe 的 --toolstack-probe 验过（arena 复用它，
    // 不重复烧钱）；命令形态由 arena.rs 单测盖住。真六 CLI 同跑留给用户在前端点「开赛」。
    if args.iter().any(|a| a == "--arena-test") {
        let root = std::env::temp_dir().join(format!("uking-arena-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        // ★ `--real <tool>`：真跑**一个**参赛者（会烧那一个工具的 token）。
        //
        // 加它的直接起因：原来这条跑道只用一个不存在的工具名验骨架，注释写着
        // 「真六 CLI 同跑留给用户在前端点开赛」—— 于是**真跑那一半从来没被机器验证过**，
        // 用户点开赛发现不好使，我们手上一条证据都没有。烧六个工具的钱不合理，
        // 但烧一个是合理的，所以做成显式开关（同 `--origin-test --live` 的规矩）。
        let real = args.iter().position(|a| a == "--real").and_then(|i| args.get(i + 1)).cloned();
        if let Some(tool) = real {
            let task = args
                .iter()
                .position(|a| a == "--task")
                .and_then(|i| args.get(i + 1).cloned())
                .unwrap_or_else(|| "在当前目录新建 hello.txt，内容写 hi，然后结束。".into());
            println!("[arena-real] 工具={tool}  工作副本={}", root.display());
            let rs = arena::run_arena(&task, &root, Some(&tool), &|m| println!("  {m}"));
            let mut bad = 0;
            for r in &rs {
                println!(
                    "  {} installed={} ran={} timeout={} exit={:?} {}ms produced={} note={}",
                    r.tool, r.installed, r.ran, r.timed_out, r.exit_code, r.ms, r.produced, r.note
                );
                if !r.installed { println!("    → 没装，这条不算失败"); }
                else if !r.ran || r.timed_out { bad += 1; }
            }
            let _ = std::fs::remove_dir_all(&root);
            println!("{}", if bad == 0 { "[OK] 真跑那一半通了" } else { "[FAIL] 真跑挂了" });
            std::process::exit(if bad == 0 { 0 } else { 1 });
        }
        // 一个不存在的工具名 → run_arena 走「未安装」分支，零烧 token
        let results = arena::run_arena("只读任务", &root, Some("__nonexistent__"), &|_| {});
        let mut ok = results.len() == 1 && results[0].tool == "__nonexistent__" && !results[0].installed && !results[0].ran;
        // 工作副本目录应该被建出来（arena/<tool>/ 的骨架），结果形状要能序列化
        let arena_dir = root.join("arena");
        ok &= arena_dir.is_dir();
        let _ = std::fs::remove_dir_all(&root);
        println!("{}", if ok { "[OK] 竞技场链路可用（工作副本隔离 + 未安装如实报 + 零烧 token）" } else { "[FAIL] 竞技场链路异常" });
        std::process::exit(if ok { 0 } else { 1 });
    }

    // 终端无头验证：U-King.exe --term-test "<cmd>"（验证 PTY + PATH 注入，不依赖 GUI）
    if let Some(i) = args.iter().position(|a| a == "--term-test") {
        let cmd = args.get(i + 1).cloned().unwrap_or_else(|| "node --version".into());
        // ★ 先报「前端会拿到的伪终端口径」。客户报「终端里输入老是重复」时，第一件事就是看
        // 这行：backend=conpty 且 buildNumber 有真值，xterm.js 才会按 ConPTY 的折行规则渲染；
        // buildNumber 为 null 说明注册表没探到，前端会整个不传 windowsPty —— 那就会退回
        // 「应用算的行数 ≠ 前端占的行数」，长输入被重画一遍又一遍。这条不进 GUI 也能查。
        let info = term::term_pty_info();
        eprintln!(
            "pty: backend={} buildNumber={}",
            info.backend.as_deref().unwrap_or("(none)"),
            info.build_number.map(|n| n.to_string()).unwrap_or_else(|| "null".into())
        );
        match term::headless_run(&cmd, 8000) {
            Ok(out) => {
                println!("{out}");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("term-test 失败: {e}");
                std::process::exit(1);
            }
        }
    }
    // 终端会话无头验证：U-King.exe --term-session-test
    //
    // 跟上面的 `--term-test` 分工不同：那条走 `headless_run`（一条**独立**的代码路径），
    // 证明不了 GUI 真正走的 `term_open_pty` / `term_write`。这条把那半边补上 —— 写入队列、
    // 按键保序、EOF 哨兵、会话自清，全是 GUI 才走得到而 `action conformance` 完全盖不住的东西。
    // stdout 只出 JSON（`| jq` 友好），非零退出码表示有断言没过。
    if args.iter().any(|a| a == "--term-session-test") {
        match tauri::async_runtime::block_on(term::headless_session_test()) {
            Ok(report) => {
                println!("{report}");
                std::process::exit(0);
            }
            Err(report) => {
                println!("{report}");
                eprintln!("term-session-test 有断言没过");
                std::process::exit(1);
            }
        }
    }
    // Agent 启动器无头验证：U-King.exe --agent-launch-test
    //
    // 验的是 conformance 和 --chat-test 都盖不住的那一层：**参数里带换行还能不能 spawn 起来**。
    // 这台机器上 `claude` / `codex` 到底解析成了什么、含 `\n` 的参数会不会被 std 当场拒掉
    //（`batch file arguments are invalid`）——这两件事一个字节都不在动作表里，而它俩正是
    // 「Claude Code 启动失败 / 是否已安装」的真因。**烧 0 token**：故意传一个不存在的 flag，
    // 我们只关心进程起没起来，不关心它退出码是几。
    if args.iter().any(|a| a == "--agent-launch-test") {
        // 和 GUI「专项修复」调的是同一个 `agent::probe_all()` —— 同一个问题不许有两套判据。
        let mut bad = 0;
        for r in agent::probe_all().as_array().cloned().unwrap_or_default() {
            let name = r["program"].as_str().unwrap_or("?");
            println!("[{name}] resolved = {}", r["resolved"].as_str().unwrap_or(""));
            println!("[{name}] via      = {}", r["via"].as_str().unwrap_or(""));
            if !r["found"].as_bool().unwrap_or(false) {
                println!("[{name}] 跳过实跑：这台机器上没找到它（不算失败）");
                continue;
            }
            if r["via"].as_str() == Some("shim") {
                println!("[{name}] ❌ 仍然解析到批处理壳 —— 多行提示词/专家 persona 会必炸");
                bad += 1;
                continue;
            }
            if r["multiline_ok"].as_bool().unwrap_or(false) {
                println!("[{name}] ✅ 含换行的参数可以 spawn");
            } else {
                println!("[{name}] ❌ spawn 失败: {}", r["error"].as_str().unwrap_or(""));
                bad += 1;
            }
        }
        if bad > 0 {
            eprintln!("agent-launch-test: {bad} 个不合格");
            std::process::exit(1);
        }
        println!("agent-launch-test: 全部通过");
        std::process::exit(0);
    }
    // ★ 任务本象（2Origin）无头验证：U-King.exe --origin-test
    //
    // 验的是 conformance 盖不住的那一层：**状态到底有没有真的进模型**。
    // 动作层「存得下、读得出」跟「模型看得见」是两件事 —— 前者 conformance 能验，
    // 后者只有真跑一轮才知道，而这正是整个 2Origin 状态层成立与否的判据。
    //
    // 分两半，因为**会花钱的检查不该悄悄花钱**（2origin 仓库自己的规矩）：
    //  - 默认半（免费）：写→读→编译，断言交接要素齐、上限生效、并发拒绝覆盖
    //  - `--live` 半（一次 flash 调用）：真跑一轮，**提示词里绝不出现目标**，
    //    看模型能不能只凭注入的状态答出来
    if args.iter().any(|a| a == "--origin-test") {
        let live = args.iter().any(|a| a == "--live");
        let sb = std::env::temp_dir().join("uking-origin-test");
        let _ = std::fs::remove_dir_all(&sb);
        let _ = std::fs::create_dir_all(&sb);
        std::env::set_var("UKING_TEST_HOME", &sb);

        // 刻意用一个**绝不可能被模型猜到**的目标：猜对了才说明是真读到了状态，
        // 而不是从提示词或常识里推出来的。
        const SECRET: &str = "把仓库里所有 .dxf 图纸的图框标题栏统一换成新公司名「昭华智造」";
        let mut o = origin::TaskOrigin::new("chat-test", "T", SECRET);
        o.current_state = "已扫出 17 张 dxf，其中 3 张标题栏是位图不是文字，改不了".into();
        o.next_steps = vec!["先改那 14 张文字的".into(), "3 张位图的单独列清单交人工".into()];
        o.facts = vec![origin::Fact {
            claim: "17 张 dxf 里有 3 张标题栏是位图".into(),
            verified: true,
            source: "doc.read".into(),
            // 「这台机器上的 17 张 dxf」是典型的 machine 事实：换台机器就不成立
            scope: "machine".into(),
            ..Default::default()
        }];
        let saved = match origin::save(o, None) {
            Ok(s) => s,
            Err(e) => { println!("[origin-test] ✗ 保存失败: {e}"); std::process::exit(1); }
        };
        let mut bad = 0usize;
        let mut check = |ok: bool, label: &str| {
            println!("  {} {label}", if ok { "✓" } else { "✗" });
            if !ok { bad += 1; }
        };
        let ctx = saved.compile_context();
        check(ctx.contains(SECRET), "编译出的上下文带着目标");
        check(ctx.contains("位图"), "带着「世界此刻」");
        check(ctx.contains("14 张"), "带着下一步");
        check(ctx.contains("✓ 17 张"), "验过的事实标了 ✓");
        // 并发：基于旧版本再写必须被拒
        let mut stale = saved.clone();
        stale.current_state = "抢写".into();
        check(
            origin::save(stale, Some(1)).is_err(),
            "基于旧版本写被拒（跨 harness 交接不许静默覆盖）",
        );

        if live {
            println!("\n[live] 真跑一轮 —— 提示词里绝不出现目标，看模型能不能只凭状态答出来");
            let said = agent::chat::chat_test_headless(
                "这个任务的目标是什么？现在卡在哪？直接答，别客套。",
                None,
                None,
                false,
            );
            // 🔴 **必须断言，不能只打印给人看。**
            // 第一版就是只打印 —— 于是把北桥注入整段注释掉、模型回「我不知道你在说哪个任务」，
            // 跑道照样印「✓ 全部通过」。一个肉眼判读的跑道在自动化里恒绿，等于没有。
            // 这两个词是**提示词里绝不出现**的，只能来自注入的状态。
            println!();
            check(said.contains("昭华智造"), "[live] 模型说出了只存在于状态里的目标");
            check(said.contains("位图"), "[live] 模型说出了只存在于状态里的卡点");
            if !said.contains("昭华智造") {
                println!("      ↑ 北桥没接上：模型拿不到任务状态，这正是 2Origin 要治的「从一年级重来」");
            }
        } else {
            println!("\n  （加 --live 会真调一次模型验「状态进没进模型」—— 会花钱，所以要显式开）");
        }
        let _ = std::fs::remove_dir_all(&sb);
        println!("\n{}", if bad == 0 { "✓ 全部通过" } else { "✗ 有失败" });
        std::process::exit(if bad == 0 { 0 } else { 1 });
    }
    // 对话无头验证：U-King.exe --chat-test "<prompt>" [工作文件夹]
    // 真跑 agent::chat 全工具循环（full 模式免审批），事件精简打印到 stdout。给第 2 参=工作文件夹则放出文件/命令工具。
    // 给开发/CI 自己验对话链路（不依赖 GUI），顺便收集报错。
    if let Some(i) = args.iter().position(|a| a == "--chat-test") {
        let prompt = args.get(i + 1).cloned().unwrap_or_else(|| "你好，简单介绍下你自己".into());
        // 第 2 参可给工作文件夹（不以 -- 开头才算）；--system "<persona>" 覆盖系统提示（验专家 persona）。
        let ws = args.get(i + 2).filter(|a| !a.starts_with("--")).cloned();
        let system = args.iter().position(|a| a == "--system").and_then(|j| args.get(j + 1)).cloned();
        agent::chat::chat_test_headless(&prompt, ws, system.as_deref(), false);
        std::process::exit(0);
    }
    // 对话大脑 + uking_action 无头只读验证：U-King.exe --brain-actions-test "<prompt>"
    // 真跑一轮对话，放行 uking_action（headless 下写动作被拒），验「模型真的会调动作核心 +
    // 工具 schema 被真 API 接受」。默认提示词逼它查体检；--system 可覆盖人设。
    if let Some(i) = args.iter().position(|a| a == "--brain-actions-test") {
        let prompt = args.get(i + 1).cloned().unwrap_or_else(|| {
            "用 uking_action 工具查一下这台机器的体检情况（runtime.stack.inspect），用简体中文把结果总结给我。".into()
        });
        let system = args.iter().position(|a| a == "--system").and_then(|j| args.get(j + 1)).cloned();
        // 进网络之前先过 schema 门禁：缺 `required` 的工具会让**整轮**请求被上游 400 拒掉
        // （0.9.99 预发实测，见 chat.rs::tools_spec 头注）。不联网、不花钱，所以白跑也值。
        let bad = agent::chat::tools_schema_lint();
        if !bad.is_empty() {
            eprintln!("[FAIL] 这些工具的 parameters 缺 required 数组（虾盘云会整轮 400）：{}", bad.join(", "));
            std::process::exit(2);
        }
        println!("[OK] 工具 schema 门禁通过：每个工具都带 required");
        agent::chat::chat_test_headless(&prompt, None, system.as_deref(), true);
        std::process::exit(0);
    }
    // WebView2 探测的无头验证：U-King.exe --webview2-check
    //
    // 为什么单开一条：**反向在开发机上永远验不到** —— 开发机必然装了 WebView2，否则这个
    // 项目自己都跑不起来，单元测试只能断言「已装」这一半。而真正要命的是另一半：干净机上
    // 探测要是误报「已装」，自检就直接失效，客户照样得到那具静默假死的空壳，而且比没做还糟
    // （我们会以为已经防住了）。给客服也留了条路：客户机「双击没反应」时先让他跑这个。
    // 只探测、不安装、不弹框；退出码 0=已装 1=没装，stdout 只出 JSON。
    if args.iter().any(|a| a == "--webview2-check") {
        let ok = webview2::installed();
        println!("{}", serde_json::json!({ "installed": ok }));
        std::process::exit(if ok { 0 } else { 1 });
    }
    // 自动化「到点了真会干活吗」无头验证：U-King.exe --automation-test [<任务 id>]
    //
    // 为什么必须有这一条：增删改已经被 `action conformance` 盖住了，**但那只证明存得下**。
    // 「到点了真跑不跑得起来」是另一条路（注入 runner → 选大脑 → 出结果 → 落运行记录），
    // 它一个字节都不在动作表里。不给它留无头入口，就只能靠人守着等到九点 —— 那不叫验证。
    //
    // 走的是**和调度线程完全同一条路**（automation::start 注入的同一个 run_automation_job）。
    // 不给 id 就跑一条临时任务（不落列表）；给 id 就真跑那条存着的。会烧 token。
    if let Some(i) = args.iter().position(|a| a == "--automation-test") {
        automation::start(Box::new(run_automation_job));
        let id = args.get(i + 1).filter(|a| !a.starts_with("--")).cloned();
        let result = match &id {
            Some(id) => {
                eprintln!("[automation-test] 跑存着的任务 {id}");
                automation::run_now(id)
            }
            None => {
                eprintln!("[automation-test] 跑一条临时任务（不落列表）");
                automation::execute(&automation::Job {
                    id: "automation-test".into(),
                    name: "无头自测".into(),
                    prompt: "用一句话说明你能帮我做什么。不要反问。".into(),
                    engine: args
                        .iter()
                        .position(|a| a == "--engine")
                        .and_then(|j| args.get(j + 1))
                        .cloned()
                        .unwrap_or_else(|| "uking".into()),
                    dir: String::new(),
                    schedule: automation::Schedule {
                        kind: "daily".into(),
                        minutes: 0,
                        at: "09:00".into(),
                        weekdays: vec![],
                    },
                    enabled: true,
                    created_at: 0,
                    next_run_at: 0,
                    last_run_at: 0,
                    last_ok: None,
                    last_message: String::new(),
                    last_run_file: String::new(),
                    runs: 0,
                    use_memory: false,
                })
            }
        };
        match result {
            Ok(body) => {
                println!("{body}");
                eprintln!("[automation-test] ✓ 跑通，运行记录在 {}", automation::runs_dir().display());
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("[automation-test] ✗ {e}");
                std::process::exit(1);
            }
        }
    }
    // ★「别让电脑睡」无头验证：U-King.exe --awake-test（夜班助手 N1）
    //
    // 为什么必须单开一条：这功能最坏的失败**不是没生效，是生效了收不回来**——
    // 客户的电脑从此不睡，而且他永远不会把这件事联想到 U-King 上。
    // 单测能盖住 apply 的幂等/可逆，但「系统到底认没认这个请求」只有系统能回答，
    // 而 `powercfg /requests` 要管理员权限，客户机上跑不了 —— 所以判据只能是
    // `SetThreadExecutionState` 自己的返回值（它返回**调用前**的状态）。
    //
    // 四条断言：
    //   ① 系统真的接受了请求（探针在临时线程上 set → 读回 → 线程退出，零残留）
    //   ② **屏幕没被点亮**（读回的标志位里不许有 ES_DISPLAY_REQUIRED）—— 不然客户屏幕整夜亮着
    //   ③ apply 可逆且幂等（收得回来）
    //   ④ **边界如实公布**：status() 里必须写着「挡不住合盖」，且 automation 的只读动作
    //      带着这句话一起发 —— 客户以为合盖也能跑，那他真的走了、任务真的没跑。
    if args.iter().any(|a| a == "--awake-test") {
        let probe = awake::probe();
        // 边界字段（挡不挡得住合盖）是静态的，先取无妨；但**打印的那份快照要等跑完再取**，
        // 见下面 `after`：先取会打出 `on:false / last_call_ret:0`，看着像整条链路没跑起来。
        let st = awake::status();
        let mut bad: Vec<String> = Vec::new();
        if cfg!(any(windows, target_os = "macos")) {
            if probe["api_accepted"] != serde_json::json!(true) {
                bad.push("系统没接受「别睡」的请求".into());
            }
            if probe["display_kept_on"] == serde_json::json!(true) {
                bad.push("请求里带上了 ES_DISPLAY_REQUIRED —— 屏幕会整夜亮着".into());
            }
        }
        // 可逆 + 幂等：开 → 再开（不该重复动系统）→ 关 → 再关
        awake::apply(true);
        let on_after_set = awake::is_on();
        let dup = awake::apply(true);
        awake::apply(false);
        let on_after_clear = awake::is_on();
        if cfg!(any(windows, target_os = "macos")) && !on_after_set {
            bad.push("apply(true) 之后回显仍是「没开」".into());
        }
        if dup {
            bad.push("同状态重复 apply 又动了一次系统（不幂等）".into());
        }
        if on_after_clear {
            bad.push("apply(false) 之后仍显示开着 —— 收不回来是最坏的结局".into());
        }
        if st["prevents_lid_close"] != serde_json::json!(false)
            || st["prevents_manual_sleep"] != serde_json::json!(false)
        {
            bad.push("边界没如实公布：合盖 / 手动睡眠是挡不住的".into());
        }
        // 边界得跟着 automation 的只读动作一起发出去（GUI/CLI/MCP 读同一句话）
        let auto = automation::status(false, awake::status());
        if auto["keep_awake"]["prevents_lid_close"] != serde_json::json!(false) {
            bad.push("automation.inspect 里没带上「挡不住合盖」这条边界".into());
        }

        // ★ 最要紧的那一环：**「有启用的任务 → 真的会去申请抑制」这条链**。
        // 上面几条验的是「系统认不认」和「说不说实话」，可「到底什么时候去申请」
        // 一个字节都不在动作表里 —— 判据要是写错（比如照原设计写成「正在跑才抑制」），
        // 前面全绿、客户的机器照睡不误。**沙箱跑，不碰真实 ~/.uking**（宪法第 10 条）。
        let sb = std::env::temp_dir().join(format!("uking-awake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&sb);
        if std::fs::create_dir_all(&sb).is_ok() {
            std::env::set_var("UKING_TEST_HOME", &sb);
            if automation::should_stay_awake() {
                bad.push("一条任务都没有时就想摁住机器不睡".into());
            }
            let job = automation::Job {
                id: "awake-selftest".into(),
                name: "别让电脑睡自检".into(),
                prompt: "x".into(),
                engine: "uking".into(),
                dir: String::new(),
                schedule: automation::Schedule { kind: "interval".into(), minutes: 30, at: String::new(), weekdays: vec![] },
                enabled: true,
                created_at: 0,
                next_run_at: 0,
                last_run_at: 0,
                last_ok: None,
                last_message: String::new(),
                last_run_file: String::new(),
                runs: 0,
                use_memory: false,
            };
            match automation::upsert(job) {
                Ok(_) => {
                    // 🔴 这一条钉的就是设计文档那处更正：任务**在等下一班（没在跑）**时
                    // 就必须摁住。写成「正在跑才抑制」的话，客户 23:00 走人、机器睡了，
                    // 02:00 那班根本不会开始 —— 那一刻永远不会到来。
                    if !automation::should_stay_awake() {
                        bad.push("有启用的任务在等下一班，却不打算阻止休眠 —— 客户走人后机器照睡，这功能等于没做".into());
                    }
                    // 停用后必须松手，不然客户关掉全部任务，电脑还是不睡
                    if automation::set_enabled("awake-selftest", false).is_ok() && automation::should_stay_awake() {
                        bad.push("任务停用了还摁着机器不放 —— 客户关掉全部任务，电脑仍旧不睡".into());
                    }
                }
                Err(e) => bad.push(format!("沙箱里建不出自检任务：{e}")),
            }
            std::env::remove_var("UKING_TEST_HOME");
            let _ = std::fs::remove_dir_all(&sb);
        }
        // 跑完之后再取一次：`last_call_ret` 是系统对**撤销**那一下的回答（非 0 = 认了），
        // `on:false` 是「我们确实松手了」。这两个数才是给排障看的。
        let after = awake::status();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": bad.is_empty(),
                "problems": bad,
                "probe": probe,
                "status": after,
                "boundary_before_run": { "prevents_lid_close": st["prevents_lid_close"], "prevents_manual_sleep": st["prevents_manual_sleep"] },
                "reversible": { "on_after_set": on_after_set, "duplicate_apply_touched_os": dup, "on_after_clear": on_after_clear },
            }))
            .unwrap_or_default()
        );
        std::process::exit(if bad.is_empty() { 0 } else { 1 });
    }
    // 长程办公「记忆跨轮」无头验证：U-King.exe --longtask-test
    //
    // 为什么必须有一条：记忆注入是纯文件 I/O，单测能盖住函数本身，但「到点了真的把上一轮
    // 的结论喂给下一轮」这条链路一个字节都不在动作表里，而尾部截断 / 上限裁剪这类边界
    // 最容易静默出错。跑一个 N 轮假长任务，走和真任务完全同一条路
    // （automation::with_memory / append_memory / execute 的同一套文件），断言：
    //   ① 上下文跨轮存活：第 N 轮的 prompt 里含第 1 轮写进去的事实
    //   ② 回写可验证：memory 文件含全部事实、总长被上限拦住、最新的那班保留
    //   ③ 断点续跑：新会话（新 job，同 id）只靠 memory.md 就能接上
    // 不烧 token：假长任务的「输出」是本地拼的，不进大脑。模型真的会不会用注入的记忆，
    // 归 `--automation-test`（真模型一轮）那条验；本跑道只管「管道不坏」。
    if args.iter().any(|a| a == "--longtask-test") {
        // 沙箱：记忆落 UKING_TEST_HOME/.uking/automation，不污染真实 ~/.uking（宪法第 10 条）。
        let sandbox = std::env::temp_dir().join(format!("uking-longtask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&sandbox);
        std::fs::create_dir_all(&sandbox).expect("建 longtask 沙箱失败");
        std::env::set_var("UKING_TEST_HOME", &sandbox);
        let r: Result<(), String> = (|| {
            let j = |id: &str, runs: i64, use_memory: bool| automation::Job {
                id: id.into(),
                name: "长程自测".into(),
                prompt: "把上个班次的进度接着往下做".into(),
                engine: "uking".into(),
                dir: String::new(),
                schedule: automation::Schedule { kind: "interval".into(), minutes: 5, at: String::new(), weekdays: vec![] },
                enabled: true,
                created_at: 0,
                next_run_at: 0,
                last_run_at: 0,
                last_ok: None,
                last_message: String::new(),
                last_run_file: String::new(),
                runs,
                use_memory,
            };
            let check = |cond: bool, what: &str, detail: String| -> Result<(), String> {
                if cond { Ok(()) } else { Err(format!("{what}：{detail}")) }
            };
            // 默认关的承诺：没开 use_memory，prompt 一字不变。
            let off = j("longtask-off", 0, false);
            check(
                automation::with_memory(&off, "原始") == "原始",
                "默认关",
                "没开 use_memory 也被改了".to_string(),
            )?;
            // ① 上下文跨轮存活 + ③ 断点续跑
            let id = "longtask-mem";
            let n = 3;
            for i in 1..=n {
                let job = j(id, (i - 1) as i64, true);
                let prompt = automation::with_memory(&job, "继续");
                for k in 1..i {
                    let fact = format!("事实{}", if k == 1 { "A" } else { "B" });
                    check(
                        prompt.contains(&fact),
                        &format!("上下文跨轮存活（第 {i} 轮）"),
                        format!("第 {i} 轮的 prompt 丢了第 {k} 轮的事实 {fact}: {prompt}"),
                    )?;
                }
                let fact = format!("事实{}", if i == 1 { "A" } else { "B" });
                let body = format!("这一轮我完成了任务 X{i}。请记住：{fact} = 值{i}。后续要继续。");
                automation::append_memory(id, i as i64, true, &body);
            }
            // ② 回写可验证
            let mem = automation::read_memory(id);
            check(mem.contains("事实A") && mem.contains("事实B"), "回写可验证", format!("memory 没写全: {mem}"))?;
            // ③ 断点续跑：新会话（新 job，同 id）只靠 memory.md 接上。
            // 🔴 必须在「上限裁剪」之前验 —— 上限丢的是最旧的，而 事实A 恰恰是最旧的，
            // 先裁剪再查续跑，等于要求「被丢掉的旧事实还活着」，那是断言顺序错，不是机制坏。
            let fresh = j(id, 0, true);
            let resumed = automation::with_memory(&fresh, "继续");
            check(resumed.contains("事实A"), "断点续跑", format!("新会话没接上记忆: {resumed}"))?;
            // ④ 上限裁剪：连塞超长 body，总长被拦（丢最旧的），最新的那班保留
            for i in 0..8 {
                automation::append_memory(id, 10 + i, true, &"长内容".repeat(1000));
            }
            let capped = automation::read_memory(id);
            check(capped.chars().count() <= 8_000, "记忆上限", format!("{} chars", capped.chars().count()))?;
            check(capped.contains("第 17 班"), "记忆上限", "截断后最新的那班被误删了".into())?;
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&sandbox);
        std::env::remove_var("UKING_TEST_HOME");
        match r {
            Ok(()) => {
                println!("{}", serde_json::json!({ "ok": true, "context_carried": true, "write_back_verified": true, "resume_works": true }));
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("[longtask-test] ✗ {e}");
                std::process::exit(1);
            }
        }
    }
    // GUI 应用启动验证：U-King.exe --launch-test <codex-app|clawx>（走真实 launch_app 代码路径）
    if let Some(i) = args.iter().position(|a| a == "--launch-test") {
        let app = args.get(i + 1).cloned().unwrap_or_default();
        match tools::launch_app(&app) {
            Ok(()) => {
                println!("launched {app}");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("launch-test 失败: {e}");
                std::process::exit(1);
            }
        }
    }

    // 浏览器直播会话无头验证：U-King.exe --browser-test
    // 起 daemon → browser.stream 拿流地址 → open → snapshot 断言 @ref → tabs。
    // 全走动作表（跟 conformance 同一条路），覆盖「面板能不能连上看实时画面」的前置。
    if args.iter().any(|a| a == "--browser-test") {
        let fail = std::cell::Cell::new(0usize);
        let t0 = std::time::Instant::now();
        // 🔴 每一步都**先报「开始跑谁」再跑**，而且走 `run_bounded`（动作自己声明的 timeout_ms）。
        //
        // 老实现两样都没有：直接 `actions::run(...)` 裸调 + 只在**成功之后**才打印一行。
        // 于是浏览器动作一旦不返回，屏幕上什么都不会出现，跑道就那么静静地挂着 ——
        // 实测超过 180 秒没结束，而没有任何一行说它在等谁。
        // 「卡住了」和「正在慢」必须在屏幕上长得不一样，否则没人能排查。
        let step = |name: &str, id: &str, input: serde_json::Value| -> serde_json::Value {
            println!("[browser-test] → {name} …（已用 {}s）", t0.elapsed().as_secs());
            match actions::run_bounded(id, input) {
                Ok(v) => {
                    println!("[browser-test] ✓ {name}: {}", serde_json::to_string(&v).unwrap_or_default());
                    v
                }
                Err(e) => {
                    fail.set(fail.get() + 1);
                    println!("[browser-test] ✗ {name}: {e}");
                    serde_json::json!({})
                }
            }
        };
        let stream = step("browser.stream", browser::BROWSER_STREAM, serde_json::json!({}));
        if !stream
            .get("ws_url")
            .and_then(|v| v.as_str())
            .map(|s| s.starts_with("ws://"))
            .unwrap_or(false)
        {
            fail.set(fail.get() + 1);
            println!("[browser-test] ✗ ws_url 格式不对（面板连不上）");
        }
        step("browser.open", browser::BROWSER_OPEN, serde_json::json!({ "url": "https://example.com" }));
        let snap = step("browser.snapshot", browser::BROWSER_SNAPSHOT, serde_json::json!({}));
        if !snap.get("snapshot").and_then(|v| v.as_str()).unwrap_or("").contains("ref=") {
            fail.set(fail.get() + 1);
            println!("[browser-test] ✗ snapshot 里没有 @ref 交互元素");
        }
        // 交互链端到端：点第一个 link 导航走 → 后退回来。验证新写动作（click/back）走真 agent-browser。
        // 只挑 link（点了真会导航），不挑 heading/button（点了是 no-op，测不出导航）。
        let link_ref = snap
            .get("snapshot")
            .and_then(|v| v.as_str())
            .and_then(|s| s.lines().find(|l| l.contains("link") && l.contains("ref=")))
            .and_then(|l| l.split("ref=").nth(1))
            .and_then(|s| s.split(|c: char| !c.is_ascii_alphanumeric()).next())
            .map(|s| s.to_string());
        if let Some(rf) = link_ref {
            // agent-browser 的 accessibility ref 是 `@eN`；不能在自检里剥掉
            // `@` 后再传裸值，否则会被上游当 CSS selector 拒绝。
            step("browser.click(link)", browser::BROWSER_CLICK, serde_json::json!({ "ref": format!("@{}", rf) }));
            let after = step("browser.back", browser::BROWSER_BACK, serde_json::json!({}));
            let _ = after;
        } else {
            println!("[browser-test] ⚠ 没有 link @ref，跳过点击/后退交互链");
        }
        let tabs = step("browser.tabs", browser::BROWSER_TABS, serde_json::json!({}));
        if !tabs.get("tabs").and_then(|v| v.as_str()).unwrap_or("").contains("example.com") {
            fail.set(fail.get() + 1);
            println!("[browser-test] ✗ tabs 里没有 example.com");
        }
        let n_fail = fail.get();
        let verdict = if n_fail == 0 {
            "全部通过".to_string()
        } else {
            format!("{n_fail} 项失败")
        };
        println!("[browser-test] {verdict}（总耗时 {}s）", t0.elapsed().as_secs());
        std::process::exit(if n_fail == 0 { 0 } else { 1 });
    }

    // 安全卸载无头验证（只读）：U-King.exe --cleanup-scan [out.txt]
    // 扫描本机 U-King 足迹并打印/写文件（不删任何东西）。给了输出文件则写文件（release 是 windows
    // 子系统、RunCommand 抓不到 stdout，与 --selfcheck 同理落盘），否则打印。
    if let Some(i) = args.iter().position(|a| a == "--cleanup-scan") {
        // 数据来自影核动作，这里只负责排版成人读的文本（同一事实不留第二份扫描实现）。
        // 直接吃动作返回的 JSON（FootprintItem 只 derive 了 Serialize），
        // 这里只负责排版成人读的文本 —— 扫描实现不留第二份。
        let items: Vec<serde_json::Value> = actions::run(actions::FOOTPRINT_INSPECT, serde_json::json!({}))
            .ok()
            .and_then(|v| v.get("items").and_then(|i| i.as_array()).cloned())
            .unwrap_or_default();
        let field = |it: &serde_json::Value, k: &str| it.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut out = format!("[cleanup-scan] 共探测到 {} 项\n", items.len());
        for g in ["core", "config", "aitool"] {
            let list: Vec<_> = items.iter().filter(|it| field(it, "group") == g).collect();
            if list.is_empty() {
                continue;
            }
            out.push_str(&format!("== {g} ==\n"));
            for it in list {
                let warn = field(it, "warn");
                out.push_str(&format!(
                    "  [{}] {} | {} | safe={}{}\n",
                    field(it, "id"),
                    field(it, "name"),
                    field(it, "detail"),
                    it.get("safe").and_then(|v| v.as_bool()).unwrap_or(false),
                    if warn.is_empty() { String::new() } else { format!(" | WARN {warn}") }
                ));
            }
        }
        out.push_str("[cleanup-scan] done (read-only)\n");
        match args.get(i + 1).filter(|a| !a.starts_with("--")) {
            Some(path) => {
                let _ = std::fs::write(path, &out);
                println!("[cleanup-scan] written to {path}");
            }
            None => println!("{out}"),
        }
        std::process::exit(0);
    }

    // 安全卸载无头执行：U-King.exe --cleanup-run <id,id,...> [out.txt]
    // 逐项真删/还原（脚本化卸载 + 客户机排障用），结果汇总打印/写文件。含 uking-home = 彻底卸载
    // （注销右键 + 清 PATH/快捷方式 + 安排退出后删 ~/.uking）。**破坏性**，仅供自动化测试/支持使用。
    if let Some(i) = args.iter().position(|a| a == "--cleanup-run") {
        let ids: Vec<String> = args
            .get(i + 1)
            .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        let out_path = args.get(i + 2).filter(|a| !a.starts_with("--")).cloned();
        let log = |m: &str| println!("[cleanup-run] {m}");
        let mut summary = String::new();
        let remove_home = ids.iter().any(|x| x == "uking-home");
        for id in ids.iter().filter(|x| x.as_str() != "uking-home") {
            match cleanup::remove(id, &log) {
                Ok(msg) => summary.push_str(&format!("OK {id}: {msg}\n")),
                Err(e) => summary.push_str(&format!("ERR {id}: {e}\n")),
            }
        }
        if remove_home {
            let _ = context_menu::unregister();
            match uninstall::run(&log) {
                Ok(_) => summary.push_str("OK uking-home: scheduled ~/.uking removal\n"),
                Err(e) => summary.push_str(&format!("ERR uking-home: {e}\n")),
            }
        }
        match out_path {
            Some(p) => {
                let _ = std::fs::write(&p, &summary);
                println!("[cleanup-run] written to {p}");
            }
            None => print!("{summary}"),
        }
        std::process::exit(0);
    }

    // U 盘护符：给一把 U 盘盖章（写 uking.key）。烧盘/补盘用：U-King.exe --arm-usb I:
    if let Some(i) = args.iter().position(|a| a == "--arm-usb") {
        let drive = args.get(i + 1).cloned().unwrap_or_default();
        match guard::arm_usb(&drive) {
            Ok(p) => {
                println!("armed {p}");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("arm-usb 失败: {e}");
                std::process::exit(1);
            }
        }
    }
    // U 盘护符：打印密钥内容（手动放 uking.key / 排查用）：U-King.exe --usb-token
    if args.iter().any(|a| a == "--usb-token") {
        println!("{}", guard::usb_token());
        std::process::exit(0);
    }
    // U 盘护符：无头诊断（客户机上看「为什么被挡」）：U-King.exe --guard-check
    if args.iter().any(|a| a == "--guard-check") {
        println!("{}", guard::diagnose_json());
        std::process::exit(0);
    }

    // 视频无头验证：U-King.exe --video-test "<提示词>" [out.json] [首帧图路径]
    // 真跑 video.rs 全链（submit→poll→download→save，veo-3.1-fast≈¥4.7/条，2026-06-23 实测扣 ¥4.68）。给第 3 个参数=图生视频。
    if let Some(i) = args.iter().position(|a| a == "--video-test") {
        let prompt = args.get(i + 1).cloned().unwrap_or_else(|| "a cute orange cat walking on the moon".into());
        let out = args
            .get(i + 2)
            .cloned()
            .unwrap_or_else(|| std::env::temp_dir().join("video-test-result.json").display().to_string());
        let image = args.get(i + 3).and_then(|p| video::image_file_to_data_url(p).ok());
        // 默认 veo-3.1-fast（实测可用）；要验别的渠道（万相/豆包/即梦等）设 UKING_TEST_VIDEO_MODEL=<id>
        let model_owned = std::env::var("UKING_TEST_VIDEO_MODEL")
            .unwrap_or_else(|_| "veo-3.1-fast-generate-001".to_string());
        let model = model_owned.as_str();
        let progress = std::sync::Mutex::new(Vec::<String>::new());
        let result = (|| -> Result<serde_json::Value, String> {
            let key = device::device_key_offline()?;
            let task_id = video::submit(&key, model, &prompt, image.as_deref(), None)?;
            let id = video::create_record(&prompt, model, &task_id)?;
            video::run(&key, id, &task_id, &|phase, prog| {
                progress.lock().unwrap().push(format!("{phase} {prog}"));
            })?;
            let item = video::list_history().into_iter().find(|r| r.id == id);
            Ok(serde_json::json!({
                "task_id": task_id,
                "id": id,
                "status": item.as_ref().map(|r| r.status.clone()),
                "have_video": item.as_ref().map(|r| r.have_video).unwrap_or(false),
            }))
        })();
        let report = match result {
            Ok(v) => serde_json::json!({ "ok": true, "result": v, "progress": progress.into_inner().unwrap() }),
            Err(e) => serde_json::json!({ "ok": false, "error": e, "progress": progress.into_inner().unwrap() }),
        };
        let _ = std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(0);
    }

    // 备份无头验证：U-King.exe --backup-test <U盘根> [out.json]（真跑 backup→list，不开 GUI、不碰还原）
    // 只读本机 ClawX/~/.openclaw，只往指定盘写快照——安全，不动本机数据。
    // windows 子系统无控制台，结果写 JSON 文件（默认 <root>/backup-test-result.json）。
    if let Some(i) = args.iter().position(|a| a == "--backup-test") {
        let root = args.get(i + 1).cloned().unwrap_or_else(backup::default_root);
        let out = args
            .get(i + 2)
            .cloned()
            .unwrap_or_else(|| format!("{}/backup-test-result.json", root.trim_end_matches(['/', '\\'])));
        let result = backup::backup(&root, env!("CARGO_PKG_VERSION"), |_m| {});
        let json = match &result {
            Ok(r) => serde_json::json!({
                "ok": true,
                "default_root": backup::default_root(),
                "dest_root": root,
                "snapshot": r,
                "list": backup::list(&root),
            }),
            Err(e) => serde_json::json!({ "ok": false, "default_root": backup::default_root(), "dest_root": root, "error": e }),
        };
        let _ = std::fs::write(&out, serde_json::to_string_pretty(&json).unwrap_or_default());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    // 办公文档真版式渲染无头验证：U-King.exe --office-pdf-test <文件.pptx>
    //
    // **为什么非得留这条**：这条路依赖客户机上有没有 LibreOffice，而开发机上多半没装 ——
    // 也就是说「转得成」这半边在我们这儿**永远验不到**，只能到有 LibreOffice 的机器上跑。
    // 没有无头入口就只能靠人开着 GUI 点一份 PPT，那不是回归跑道。
    //
    // stdout 只出 JSON（`| jq` 友好）。`renderer_found:false` 是**事实不是失败** ——
    // 退出码仍为 0，因为「这台机器没装」和「代码坏了」必须分得开。
    if let Some(i) = args.iter().position(|a| a == "--office-pdf-test") {
        let src = args.get(i + 1).cloned().unwrap_or_default();
        let soffice = officedoc::soffice_path().map(|p| p.display().to_string());
        let started = std::time::Instant::now();
        let r = if src.is_empty() { Ok(None) } else { officedoc::to_pdf(&src) };
        let ms = started.elapsed().as_millis() as u64;
        let json = match &r {
            Ok(Some(pdf)) => serde_json::json!({
                "ok": true, "renderer_found": soffice.is_some(), "soffice": soffice,
                "src": src, "pdf": pdf, "ms": ms,
                "bytes": std::fs::metadata(pdf).map(|m| m.len()).unwrap_or(0),
                "convertible": officedoc::is_convertible(&src),
            }),
            Ok(None) => serde_json::json!({
                "ok": true, "renderer_found": soffice.is_some(), "soffice": soffice,
                "src": src, "pdf": serde_json::Value::Null, "ms": ms,
                "convertible": officedoc::is_convertible(&src),
                "note": if soffice.is_none() { "本机没装 LibreOffice —— 预览会退回文字大纲档（这是设计好的降级，不是故障）" }
                        else if src.is_empty() { "没给文件路径" }
                        else { "这个格式不走 LibreOffice（docx/xlsx 前端渲染得更好更快）" },
            }),
            Err(e) => serde_json::json!({
                "ok": false, "renderer_found": soffice.is_some(), "soffice": soffice,
                "src": src, "ms": ms, "error": e,
            }),
        };
        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    // 技能包无头验证：U-King.exe --skillpack-test [导出目录] [out.json]
    // 真跑 skillpack::export_to 释放内嵌的 SKILL.md + 脚本，核验文件齐全且非空。安全：只往指定目录（默认 temp）写。
    if let Some(i) = args.iter().position(|a| a == "--skillpack-test") {
        let dest = args
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| std::env::temp_dir().join("uking-skillpack-test").display().to_string());
        let out = args
            .get(i + 2)
            .cloned()
            .unwrap_or_else(|| std::env::temp_dir().join("skillpack-test-result.json").display().to_string());
        // 遍历**全部**包，别再硬编码 AIGC 那几个文件名 —— 原先只查 aigc 的 5 个文件，
        // 于是后加的 PPT/DOCX/XLSX/office-read/office-edit 一个都没被这条跑道盖住：
        // 加了新包、或者某个包的 include_str! 路径写错，这里照样绿。
        // 现在「加一个包 = 自动多一组断言」，跟 `action conformance` 一个思路。
        let json = match skillpack::export_to(Some(std::path::Path::new(&dest))) {
            Ok(_) => {
                let root = std::path::Path::new(&dest);
                let checks: Vec<serde_json::Value> = skillpack::pack_manifest()
                    .iter()
                    .map(|(pack, rel)| {
                        let p = root.join(pack).join(rel);
                        serde_json::json!({
                            "file": format!("{pack}/{rel}"),
                            "exists": p.exists(),
                            "size": std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                        })
                    })
                    .collect();
                let all_ok = checks
                    .iter()
                    .all(|c| c["exists"].as_bool().unwrap_or(false) && c["size"].as_u64().unwrap_or(0) > 0);
                serde_json::json!({
                    "ok": all_ok,
                    "root": dest,
                    "packs": skillpack::pack_names(),
                    "files": checks,
                })
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        };
        let _ = std::fs::write(&out, serde_json::to_string_pretty(&json).unwrap_or_default());
        std::process::exit(if json["ok"].as_bool().unwrap_or(false) { 0 } else { 1 });
    }

    // 技能包**装进各工具**的无头验证：U-King.exe --skillpack-install-test
    //
    // 跟上面那条分工明确：`--skillpack-test` 验的是「内嵌资源释放得出来」，
    // 这条验的是「释放出来的东西落到了各 AI 真正会去读的目录」——后者才是客户能不能用上的判据，
    // 而这条链（0.9.87 后改成启动即后台同步）此前一条无头验证都没有。
    //
    // 🔴 `skillpack::home_dir()` 认的是 USERPROFILE/HOME，**不认 UKING_TEST_HOME**
    //（本模块自带 home_dir 以保持整块可插拔）。所以跑这条测试**必须自己把 USERPROFILE 指到沙箱**，
    // 否则会往真实用户目录里写。这里显式检查并拒绝在真实 HOME 下跑，免得测试污染开发机。
    if args.iter().any(|a| a == "--skillpack-install-test") {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        if home.is_empty() || !home.to_lowercase().contains("temp") {
            eprintln!(
                "拒绝执行：本测试会真往 HOME 下的各工具 skills 目录写文件。\n\
                 请先把 USERPROFILE（Windows）或 HOME 指到一个临时沙箱目录再跑。\n\
                 当前 HOME = {home}"
            );
            std::process::exit(2);
        }
        // 🔴 光挡住 HOME 不够：Hermes 那条走的是 `%LOCALAPPDATA%\hermes`，**跟 HOME 没关系**。
        // 第一次跑这条测试就是这么漏出去的 —— HOME 指了沙箱，技能包照样写进了真实机器的
        // hermes 目录。沙箱漏一个口子，验出来的「没污染」就不作数（同 --install-test-cjk 第④条）。
        #[cfg(windows)]
        {
            let la = std::env::var("LOCALAPPDATA").unwrap_or_default();
            if !la.to_lowercase().contains("temp") {
                eprintln!(
                    "拒绝执行：LOCALAPPDATA 还指着真实目录，Hermes 那条会绕过 HOME 沙箱写进去。\n\
                     请一并设置 LOCALAPPDATA 到同一个临时沙箱。当前 LOCALAPPDATA = {la}"
                );
                std::process::exit(2);
            }
        }
        // 🔴 同理，`HERMES_HOME` / `CODEX_HOME` 一旦被设，就**压过** HOME 和 LOCALAPPDATA
        //（那是各工具自己的解析顺序，我们对齐了它）。所以护栏必须跟着长：只挡 HOME + LOCALAPPDATA
        // 的话，一个残留的 HERMES_HOME 就能把技能包写进真实机器 —— 而这不是假设，
        // 开发机上就真躺着一个指向 `Y:\compare-upstream\hermes-home` 的残留值（2026-08-05）。
        // **没设**是允许的（那条会回落到已被挡住的默认位置）；设了就必须指向沙箱。
        for key in ["HERMES_HOME", "CODEX_HOME"] {
            let v = std::env::var(key).unwrap_or_default();
            if !v.trim().is_empty() && !v.to_lowercase().contains("temp") {
                eprintln!(
                    "拒绝执行：{key} 指着真实目录，会绕过 HOME 沙箱写进去。\n\
                     请把它一并指到同一个临时沙箱，或直接 unset。当前 {key} = {v}"
                );
                std::process::exit(2);
            }
        }
        let done = skillpack::install_into_tools();
        let json = serde_json::json!({
            "ok": !done.is_empty(),
            "home": home,
            "installed": done
                .iter()
                .map(|(name, path, experimental)| serde_json::json!({
                    "tool": name, "path": path, "experimental": experimental,
                }))
                .collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
        std::process::exit(if json["ok"].as_bool().unwrap_or(false) { 0 } else { 1 });
    }

    // 崩溃自动上报（panic=abort 下 hook 也会先执行）
    report::install_panic_hook();

    // 只迁移旧版 U-King 自己写出的两行 CLI shim：旧写法把 Unicode 用户目录以 UTF-8
    // 字面量塞进 .cmd，cmd.exe 按 ACP 读取时会找不到 Codex。此处不创建 shim、不改 PATH、
    // 不覆盖未知脚本，因此普通启动的副作用只限于修复已识别的历史坏文件。
    let migrated_cli_shims = installer::migrate_legacy_cli_command_guards();
    if migrated_cli_shims > 0 {
        ulog::write("installer", &format!("已迁移 {migrated_cli_shims} 个 Unicode 安全 CLI shim"));
    }

    // 静默自升级「套用」阶段：后台已下好更新版 exe 时，**裸启动**会在 GUI 起来前原子替换并重启
    //（纯本地文件判断，不联网、不拖慢启动）。带 --open-dir 等参数的启动跳过（避免吞掉右键传入
    // 的目录参数），留到下次裸启动再套用。args[0] 是 exe 本身，故 len<=1 即裸启动。
    // U 盘护符口味**不**做静默自升级：服务器上的绿色 exe 是「下载版（无护符）」，自动替换会把护符
    // 悄悄抹掉。U 盘版按「换新盘 / 装到本地后走安装版」拿更新。下载版照旧静默升级。
    #[cfg(windows)]
    if !cfg!(feature = "usb-guard") && args.len() <= 1 && installer::apply_staged_update() {
        std::process::exit(0);
    }

    // U 盘护符（仅「U 盘口味」= 带 usb-guard feature 编译时生效）：没原装 U 盘就弹框拦住，不启动主程序。
    // 下载版未启用 → guard::enforce 内部 ok() 恒 true 直接返回，funnel 零影响。放在自升级套用之后、
    // GUI 起来之前，且所有 --*-test/--selfcheck 无头分支已在上面 exit，自检/CI 不受影响。
    guard::enforce();

    // GUI 侧也记动作流水 —— 「客户到底点了什么」此前完全查不到，
    // 而这正是远程排障第二常问的问题（第一是「报了什么错」）。
    actions::set_audit(|l| ulog::write("actions", l));
    // 同一处产生的第二个用途：进**行为时间轴**（谁在什么时候干了什么）。
    // 影核的红利在这一行 —— GUI / CLI / MCP 三个面各只接一次，动作表以后加多少个动作，
    // 时间轴都自动覆盖，不必回头改这里。
    actions::set_record(journal::record_action);
    actions::set_source("gui");

    // ★ WebView2 自检。**必须在 Builder 之前**，理由见 webview2.rs 顶部：没装 WebView2 时
    // `setup` 根本轮不到执行（卡在创建窗口），把检查写进 setup 等于没写。走到这里说明
    // 所有无头 CLI 分支都已 exit，是真要开界面了 —— 无头自检/CI 不会被这段拦住。
    // 已装时零开销（一次目录 stat）；没装时用原生 MessageBox 说话（不经过正坏着的 WebView）。
    if matches!(webview2::ensure(), webview2::Outcome::Declined | webview2::Outcome::Failed) {
        // 已经弹框讲清原委并开了下载页。此时**必须退出**：再往下走就是那具
        // 「进程在、任务栏有、界面没有、定时任务也不跑」的空壳，客户只会觉得双击没反应。
        return;
    }

    let builder = tauri::Builder::default();

    // ★ 浏览器导航无头取证模式（`--browser-nav-test`，需求榜 P0 #5）。
    // 它需要真的事件循环和真的 WebView2，所以不能像别的无头模式那样在 run() 顶部就退出，
    // 只能走完整的 builder —— 但**必须跳过单实例插件**：否则用户那个 U-King 开着时，
    // 这个进程会被弹回去、顺手把他的窗口顶到前面，那正是最不该发生的事。
    let nav_probe = args.iter().any(|a| a == "--browser-nav-test");

    // ★ `--allow-multi-instance`：**并行调试实例**。默认永远不开，客户机上不存在这条路。
    //
    // 为什么需要它：验一版新构建就得起第二个 GUI，而单实例锁会把它弹回去、顺手把用户正在用的
    // 那个窗口顶到前面 —— 于是「验一下新版」的代价是「打断手上所有的活」（工作台里挂着的
    // 一堆终端全断），结果没人验。`--browser-nav-test` 早就因为同样理由要跳过单实例（见上），
    // 只是那条路绑死在一个特定跑道上，别的场景够不着。
    //
    // 🔴 **它现在的语义不止「跳过单实例」，还包括「钉死当并行调试实例」**（见 `instance.rs`）：
    // 两边共用同一份 `~/.uking`（那正是这功能的前提 —— 验的必须是同一个世界，所以
    // `UKING_TEST_HOME` 沙箱那条路被明确否决过），但一批后台单例活在这个进程里全部关掉：
    // 调度线程 / 技能包同步 / Codex 代理 / 说明书发布 / 崩溃账本 / device key 刷新 / 自升级暂存，
    // `tasks.json` 与 `agent-threads.json` 只读。清单和逐条理由在 `instance::DISABLED_IN_SIDECAR`。
    //
    // **不带这个开关的启动路径一字未改** —— 这是整套机制客户风险为零的全部理由，别去动它。
    let allow_multi = args.iter().any(|a| a == "--allow-multi-instance");
    if allow_multi {
        eprintln!(
            "[U-King] 本进程是并行调试实例（--allow-multi-instance）：跟已在运行的那个共用 ~/.uking，\n\
             但定时任务、技能包同步、Codex 代理自愈、说明书发布、崩溃记账、自升级暂存都不跑，\n\
             任务列表和 AI 续接 id 只读。查角色：action run runtime.instance.inspect --json"
        );
    }
    // 只借「跳过单实例」这一件事，**不复用 `nav_probe` 本身** —— 它在下面还会真的去跑
    // 浏览器导航跑道（`run_browser_nav_probe`）。把两者混成一个布尔，多开预览会莫名其妙
    // 起一个探针然后自己退出，而症状看起来会像「新版启动就崩」。
    let skip_single_instance = nav_probe || allow_multi;

    // 演示卸载绿色版是**独立分发的另一个产品**，却和主程序共用同一份 tauri.conf.json（=同一个
    // identifier，也就是同一把单实例锁）。不排除它的话，客户机上开着 U-King 时那个绿色版
    // 双击没反应 —— 只会把 U-King 的窗口顶到前面，看着就是「这软件坏了」。
    // ★ **单实例必须第一个注册**（官方要求：它要在别的插件初始化前决定这个进程活不活）。
    //
    // 治的是客户机上「同时 3 个 u-king-mini.exe」——关窗口 = 缩托盘不退出，用户以为关了、
    // 再双击又起一个。每个实例都带自己的调度线程（**同一条定时任务被跑 N 遍 = N 倍烧 token**）、
    // 各自往同一批目录同步技能包互相覆写、旧版进程占着 exe 让卸载删不掉。
    //
    // 🔴 **挡的只是 GUI 这条路**：`--selfcheck` / `action run` / `mcp serve` 这些无头模式
    // 都在 `run()` 顶部就 `std::process::exit` 了，根本走不到这里 —— 影核协议的三个面
    // （桌面 / CLI / MCP）照旧能在 GUI 开着时并行工作。要是把锁加在进程入口，运维远程跑
    // 一条 `action run` 就会被自己的界面挡住，那是把能力锁死，不是防重复。
    #[cfg(not(feature = "demo-uninstaller"))]
    let builder = if skip_single_instance { builder } else { builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        // 第二次点击带来的目录先接住，再把老窗口顶到前面，最后让前端重新读一次 env。
        if let Some(dir) = parse_open_dir_from(&argv) {
            if let Ok(mut g) = PENDING_OPEN_DIR.lock() {
                *g = Some(dir);
            }
        }
        show_main_window(app);
        // 🔴 **交棒必须出声**（2026-08-18，客户报「下载的绿色版运行不了，点击没反应」）。
        //
        // 关窗口默认是缩托盘不退出 → 装过 U-King 的机器上它几乎一直在跑 →
        // 双击下载来的绿色版 `U-King.exe`，单实例锁让新进程交棒后**直接退出**，
        // 只把老窗口顶到前面。用户看到的是「我点的这个打不开」，而真相是
        // 「它已经开着了，只是你点的是另一份」。
        //
        // 不许两个实例是对的（多实例 = 定时任务 N 倍烧 token），要修的是**静默**。
        // 判据是**两份 exe 的路径不同** —— 同一份被双击两次不用解释，那是常识；
        // 换一份点不开才需要解释。
        let other = argv.first().map(|s| s.to_string()).unwrap_or_default();
        let mine = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let differs = !other.is_empty()
            && !mine.is_empty()
            && !other.eq_ignore_ascii_case(&mine)
            && std::path::Path::new(&other).file_name().is_some();
        if differs {
            let _ = app.emit("uking:second-instance", serde_json::json!({ "other": other, "mine": mine }));
        }
        // 前端 `refresh()` 会重新 `get_env`，把上面那个目录取走。
        let _ = app.emit("uking:reopen", ());
    })) };

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        // 小程序资源与桥。资源和 RPC 同源（Windows 上都是 http://uking.localhost/…），
        // 所以不触发 CORS；异步变体是必须的 —— 一次 AI 修图上限 600s，
        // 同步处理会冻死整个 webview。
        .register_asynchronous_uri_scheme_protocol("uking", |ctx, req, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                // 坑：自定义 scheme 里第一段会被解析成 **host**。写 `uking://app/x` 的话
                // host=app、path=/x，而且页面内所有相对请求都会带上同一个 host，
                // `/rpc/y` 变成 `uking://app/rpc/y` —— 路由永远对不齐。
                // 所以 authority 固定用无意义的 `localhost`，一切语义只放在 path 里。
                let path = req.uri().path().trim_start_matches('/').to_string();
                let is_post = req.method() == tauri::http::Method::POST;
                let resp = miniapp_protocol(&app, &path, is_post, req.body());
                MINIAPP_PROTO_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // 保留这行：小程序打不开时第一件事就是看协议有没有被请求到。
                // 没有日志的话，「点了没反应」和「协议压根没被访问」长得一模一样 —— 踩过。
                eprintln!(
                    "[uking://] {} /{path} → {} ({}B)",
                    if is_post { "POST" } else { "GET" },
                    resp.status(),
                    resp.body().len()
                );
                responder.respond(resp);
            });
        })
        .setup(move |app| {
            // ★ 浏览器导航无头取证（需求榜 P0 #5 的硬那半边）：证明 `w.eval("history.back()")`
            // 真的让**外部页面**导航了。整个过程窗口 `visible(false)`，屏幕上什么都不出现，
            // 不抢前台、不动鼠标、不截屏。跑完直接退出进程 —— 不起托盘、不起调度线程。
            if nav_probe {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    let code = run_browser_nav_probe(&h);
                    std::process::exit(code);
                });
                return Ok(());
            }
            // ★★ 角色登记。**必须排在下面所有后台 spawn 之前** —— 它决定的就是那些活起不起。
            //
            // 🔴 2026-08-23 的第一版把它排在说明书发布**之后**，破了这句自己写的规矩
            // （说明书是 `~/.uking` 里的共享文件、同名覆盖，两个实例会互相覆写）。
            // 08-24 修那处时又走到另一个极端：**把说明书那段整个删掉了**，
            // 于是全体单实例客户升级后 `~/.uking/llms.txt` 不再刷新 —— 那是整笔被 revert 的真因。
            // 正解是**门控**不是删除：下面那段一行没动，只是在调试实例里不跑。
            //
            // 无头模式（`action run` / `mcp serve` / `--selfcheck`）走不到这儿，
            // 对它们 `inspect()` 报 headless、能力完整 —— 不该因为界面开着就被降权。
            let is_sidecar = allow_multi;
            instance::mark(is_sidecar);
            // ★ 「这两份缓存只读」由组合根注入，**不是让 tasks/agent 去 import instance**
            //   —— 模块独立铁律禁止模块间横向 import，`check-module-coupling` 拦过这一版。
            //   必须在这里、在任何界面动作能被点到之前注入；晚一步就会有一次真的覆盖写。
            tasks::set_readonly(is_sidecar);
            agent::threads::set_readonly(is_sidecar);
            if is_sidecar {
                // 界面上必须说出来。**静默降权比不降权更坏**：看到「定时任务没跑」却找不到
                // 任何线索，人会去查调度器、查配置、查上游，而真相只是「你开了两个」。
                let _ = app.emit("uking:sidecar-mode", serde_json::json!({ "on": true }));
                ulog::write(
                    "instance",
                    &format!(
                        "并行调试实例启动（pid={}），已关闭：{}",
                        std::process::id(),
                        instance::DISABLED_IN_SIDECAR.join(" · ")
                    ),
                );
            }
            // 「给 AI 的说明书」开机自动落盘。**必须在这儿而不是等用户点按钮** ——
            // 这份文件的全部意义是让**别家 AI** 在客户机上发现我们；要是非得先有人
            // 打开 U-King、翻到「我的 U-King」、点一下「生成」，那新装的机器上它就是空的，
            // 而那正是最需要它的时候。
            //
            // 放后台线程：渲染要遍历动作表 + 两次落盘，不该挡住窗口显示。
            // 幂等（原子写覆盖），每次启动重跑一遍正好让升级后的新动作自动进说明书。
            //
            // 调试实例不发：新旧两版的动作表和技能目录不同，而落盘是同名覆盖 ——
            // 两个实例会轮流把对方刚写的说明书刷掉，别家 AI 读到哪一版全看谁最后启动。
            if !is_sidecar {
                std::thread::spawn(|| {
                    let dir = identity::uking_dir();
                    let id = identity::load_identity_in(&dir);
                    match identity::publish_in(&dir, &id, &actions::manifest(), &skillpack::skill_catalog()) {
                        Ok(f) => ulog::write("identity", &format!("说明书已生成: {}", f.join(" | "))),
                        // 失败不致命：说明书没了只是 AI 发现不了我们，app 本身照跑。
                        Err(e) => ulog::write("identity", &format!("说明书生成失败: {e}")),
                    }
                });
            }

            // 无头验证 / 客服排障：U-King.exe --miniapp-open <id|slug>
            //
            // 走的是**和界面点击完全同一条路**（open_miniapp），不是另写一份加载逻辑；
            // 8 秒后按「uking:// 协议有没有被请求到」判定：>0 = 页面真的在加载，0 = 白窗。
            // 加它的原因：0.9.72 的小程序全量打不开，而 `--miniapp-list` 全绿
            //（列表读目录、加载走协议，两条路各自会失败），CI 里没有任何东西能发现。
            if let Some(i) = std::env::args().position(|a| a == "--miniapp-open") {
                let want = std::env::args().nth(i + 1).unwrap_or_default();
                let hit = miniapp::list()
                    .into_iter()
                    .find(|a| a.id == want || a.slug == want)
                    .or_else(|| miniapp::list().into_iter().next());
                let Some(info) = hit else {
                    eprintln!("[miniapp-open] 本机一个小程序都没装");
                    std::process::exit(2);
                };
                eprintln!("[miniapp-open] 打开 {} ({})", info.name, info.id);
                let app2 = app.handle().clone();
                let id = info.id.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = open_miniapp(app2, id).await {
                        eprintln!("[miniapp-open] 打开失败: {e}");
                        std::process::exit(3);
                    }
                });
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(8));
                    let n = MINIAPP_PROTO_HITS.load(std::sync::atomic::Ordering::Relaxed);
                    if n == 0 {
                        eprintln!("[miniapp-open] ✗ 协议一次都没被请求到 —— 窗口是空的（0.9.72 的病）");
                        std::process::exit(1);
                    }
                    eprintln!("[miniapp-open] ✓ 协议被请求 {n} 次，页面真的加载了");
                    std::process::exit(0);
                });
                // 主窗口现在默认隐藏（visible:false），这条诊断路径也照旧把它显示出来 ——
                // 排障时窗口凭空不见了，人会以为是崩了。
                show_main_window(app.handle());
                return Ok(());
            }
            if cfg!(feature = "demo-uninstaller") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title("U-King 演示卸载工具");
                }
            }
            // ★ 主窗口在这儿才第一次可见（配置里是 `visible:false`）。
            // 图标必须赶在可见之前挂上去，理由见 `winicon.rs` 的模块注释：
            // 任务栏按钮一建出来，事后再设图标它不认。
            show_main_window(app.handle());
            // 演示卸载绿色版是一次性工具：不常驻托盘，也不在后台做联网/配置类动作。
            if cfg!(feature = "demo-uninstaller") {
                return Ok(());
            }
            tray::install(app.handle())?;
            // 崩溃取证开一次会话：结上次的账（没正常退出的话留证据）+ 落本次标记 + 起心跳。
            // **必须在这儿而不是 run() 顶上** —— 无头模式（--selfcheck / action run / mcp serve）
            // 都在 tauri::Builder 之前就 exit 了，放上面会让运维远程跑一条 `action run`
            // 就把客户正在跑的 GUI 会话标记覆盖掉，凭空造出一次「异常退出」。
            // 无头验证：U-King.exe --crash-test
            //
            // 验的是 conformance 盖不住的那半边 —— **正常退出会不会销账**。
            // 崩溃那半边可以靠强杀实测，可「托盘退出 → RunEvent::Exit → end_session」这条链
            // 断了的话不会报错，只会让**每个正常退出都变成一次假崩溃**，把真信号淹掉，
            // 而且要等客户装上才发现。所以让它自己走一遍真路：起 GUI → 确认标记落盘 →
            // app.exit(0) → 在 Exit 回调里断言标记已被清掉。
            if std::env::args().any(|a| a == "--crash-test") {
                let app2 = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let marked = crashlog::session_marker_exists();
                    eprintln!("[crash-test] 会话标记已落盘: {marked}");
                    if !marked {
                        eprintln!("[crash-test] ✗ 起了 GUI 却没写会话标记 —— 异常退出检测等于没有");
                        std::process::exit(1);
                    }
                    app2.exit(0); // 走和托盘「退出」完全同一条路
                });
            }
            //
            // 🔴 **调试实例不开会话**。注意理由**不是**「标记全局一份」——那条 2026-08-19 就修好了，
            // 现在每个实例写自己的 `.session-<pid>.json`，`settle_previous` 还会跳过活着的兄弟实例。
            // 真正的漏洞在 `is_live_sibling`：它拿**本进程**的文件名去比对方的镜像名，而
            // 「绿色版 `U-King.exe` + 安装版 `u-king-mini.exe` 同时跑」正是这功能的主场景 ——
            // 名字不同 → 判成「不是兄弟」→ 给还活着的主实例记一笔假 unclean_exit，
            // **并把它那份活标记删掉**（主实例之后真崩了，盘上一点痕迹都没有）。
            // 短命的那条还会 `report_bug` 灌进 bug 采集。
            //
            // （这跟 08-23 那版 leader.rs 被把关打回的第一条是同一个坑：拿自己的镜像名判别人。）
            if !is_sidecar {
                if let Some(prev) = crashlog::begin_session() {
                    // 上报只挑短命的那种（启动即崩 / 崩溃循环）：跑了几小时的异常退出多半是关机，
                    // 全报会把 issue 区淹掉，反而让真信号沉底。本地留痕则一条不落。
                    if prev.looks_like_crash_loop {
                        report::report_bug(
                            "unclean_exit",
                            &format!("疑似崩溃循环：上次只跑了 {} 秒就异常退出", prev.lived_secs),
                            &format!("{}\n\n--- crash.log ---\n{}", prev.summary, prev.log_tail),
                        );
                    }
                }
            }
            // 下面这一整段（到 `stage_pending_update` 为止）是**主实例专属的后台单例活**。
            // 调试实例一条都不跑，逐条理由见 `instance::DISABLED_IN_SIDECAR` 的清单注释。
            //
            // 🔴 门控写成 `if !is_sidecar { … }` 而不是抽成一个函数：抽出来就得有第二个调用点
            // 才划算，而这版**刻意没有晋升**（见 `instance.rs` 模块头），只有这一个调用点。
            // 一个只被调一次的函数，只是把「启动时到底跑了什么」多藏了一层。
            if !is_sidecar {
            // ClawX 例行检查：只放行防火墙（让 ClawX 能联网），**不再后台静默写用户配置**。
            //（旧 auto_heal_clawx 会偷偷把 ClawX 切到虾盘云，已废弃；是否接入改由前端引导用户手动点。）
            std::thread::spawn(providers::clawx_firewall_only);
            // 修 0.9.92 及更早版本写坏的 Codex 配置（`wire_api="chat"` 让新版 Codex 整份
            // 配置加载失败、秒退，Issue #364）。只动带我们标记的文件，拿不到 key 也先救启动。
            std::thread::spawn(providers::heal_codex_wire_api);
            // 修 0.9.94 及更早版本装的**旧版 Token 压缩机 hook**：它把命令改写成裸 `rtk …`，
            // 而 macOS 上我们从没把 shim 接进过 PATH（`prepend_user_path` 是 PowerShell 实现，
            // Mac 分支压根没调）→ 客户机上**每一条 Bash 命令**退出码 127，整台机器的 AI 变废。
            // 能升级成绝对路径写法就升级，升不了就摘掉。**不能等客户来点开关** ——
            // 他看到的是「Claude Code 什么都干不了」，没有任何线索指向这个默认关着的省钱开关。
            std::thread::spawn(|| {
                let _ = rtk::heal_legacy_hook();
            });
            // 首启激活内置指纹 Key：服务端 mint 真实 token + 送体验额度，「插上就能用」靠它。
            // 走 get_device_key（而非裸 ensure_activated）—— 它带「charged=false 时强制再激活」
            // 自愈：救回「本地标了已激活但服务端 token 缺失」的老机器（2026-06-13 加 mint 前装的
            // 机器，否则 401 卡死）。幂等 + 已激活且有余额时零额外开销；后台线程绝不阻塞启动；
            // 沙箱内部自动跳过。
            std::thread::spawn(|| {
                let _ = device::get_device_key();
            });
            // 视频按次扣费，不能把恢复责任绑在“客户有没有点进视频页”上。启动即接管所有
            // running/ready 任务：继续轮询原 task_id，出片后落本机；全过程不再 POST、不再扣费。
            resume_pending_videos(app.handle().clone());
            } // ← `if !is_sidecar` 第一段到此为止
            // 跑完通知人。**这是「你不用盯着」真正成立的前提**：在此之前，任务跑完只写
            // `~/.uking/automation/*.md` 和列表里的 last_message，客户不打开工作台那个
            // 「自动化」面板就永远不知道跑没跑过 —— 于是「到点没跑」和「跑了但我不知道」
            // 在客户那里长得一模一样，我们这边也无从分辨。
            //
            // 两个面一起给：窗口开着 → 前端 toast；缩在托盘 → 托盘悬停提示。
            // 通知是**唯一发出点**（execute 里调 notify），所以调度线程到点触发和
            // 「立即运行一次」走的是同一条路，不会一个有提示一个没有。
            {
                let app2 = app.handle().clone();
                automation::set_notifier(Box::new(move |job, ok, summary| {
                    let _ = app2.emit(
                        "uking:automation_done",
                        serde_json::json!({
                            "id": job.id,
                            "name": job.name,
                            "ok": ok,
                            "summary": summary,
                        }),
                    );
                    if let Some(tray) = app2.tray_by_id("uking-tray") {
                        let _ = tray.set_tooltip(Some(&format!(
                            "U-King · 「{}」{}",
                            job.name,
                            if ok { "刚跑完" } else { "没跑成" }
                        )));
                    }
                }));
            }
            // 自动化（定时任务）调度线程。**只在 GUI 起来时启动** —— 跑一条 `action run` 的
            // 无头进程不该顺手把客户的定时任务全触发一遍。真正怎么干由这里注入（run_automation_job），
            // automation.rs 自己不认识 agent/device。
            //
            // ★ 休眠抑制也在这儿注入（夜班助手 N1）：**必须在 start 之前注册**，
            // 调度线程一起来就会申请第一次 —— 晚一步注册，那次申请就是空的。
            // 抑制由调度线程自己申请（`SetThreadExecutionState` 按线程记账，见 awake.rs 模块头）。
            // 无头 CLI 不注册 = 一行不抑制：跑个 `action run` 不该顺手让客户的电脑整夜不睡。
            automation::set_keep_awake(Box::new(|on| {
                awake::apply(on);
            }));
            // ★ 主实例专属，第二段。**这条是整个并行机制最初的、也是唯一会真花钱的理由**：
            //   两条调度线程各自到点触发同一批定时任务 = 同一件事跑两遍 = 双倍烧 token，
            //   而且两次结果互相覆盖 `last_message`，客户看到的还是一份，完全察觉不到。
            //   别的降权顶多是「少干点活」，只有这条是「多花钱且看不出来」。
            //
            //   上面的 `set_notifier` / `set_keep_awake` **刻意留在门外**：它们只是注入，
            //   不起任何线程、不写任何共享文件。调试实例里用户照样能手点「立即运行一次」，
            //   注入没做的话那次手动运行会静悄悄跑完、连个提示都没有。
            if !is_sidecar {
            automation::start(Box::new(run_automation_job));
            // Codex 省钱路由自愈：客户开过 DeepSeek 本地路由（config 指向 127.0.0.1:15722）
            // 但代理进程已不在（最常见：重启电脑后）→ 自动拉回来，否则 codex 全废且客户不知道
            // 要去哪重开。没开过路由的机器此函数零副作用。后台线程不阻塞启动。
            std::thread::spawn(codex_proxy::resume_if_configured);
            // 同样的代理，运行中也会死（被 Windows 杀 / OOM / 端口被占）—— 启动那一下的
            // resume_if_configured 救不了「跑着跑着没的」。看门狗后台循环，config 还指着
            // 本地端口但没人服务就拉回；直连/已停的 config 不含本地端口，整轮跳过。
            // 只起一次（每次 GUI 启动起一个新线程，不会重复累加）。
            codex_proxy::start_proxy_watchdog();
            // 数据基台：每次启动补一次当天用量快照 + 清掉 12 个月前的历史。
            //
            // 快照是**覆盖语义**（同一天重复调不累加），所以一天开几次都不会记重。
            // 延迟 12s：扫会话日志有 IO，让首屏先稳住。纯本地写入，不联网、不上传
            // （上传要用户在「数据」页显式打开 consent）。失败静默，绝不拦启动。
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(12));
                metrics::prune();
                metrics_rollup_now();
            });
            // 内置小程序落地：随 exe 发货的那几个（抠正 / 图片修补 / 改尺寸）首启自动装好，
            // 断网也有得用。幂等；用户手上更新的版本不会被按回旧版；装不上只记日志不拦启动。
            std::thread::spawn(|| {
                bundled_apps::ensure_installed(&|m| eprintln!("{m}"));
            });
            // 技能包同步：exe 里带的那几套技能（作图/视频 · PPT · 文档 · 表格 · 网页 · 读文档）
            // 每次启动同步进已装 AI 工具的 skills 目录。
            //
            // **为什么必须无条件同步**：`install_skill_pack` 原先只在「召唤专家 / 一键配好全部 AI /
            // 装机向导」里触发 —— 装完就直奔工作台聊天的客户，一次都碰不到。于是**升级换不动技能**：
            // pc-*** 跑着 0.9.87，机器上却还是 7 月 10 日装机那天的两个包（时间戳实锤），
            // exe 里的 PPT/DOCX/XLSX/WEB/VISION 五套一个都没落地。客户在 u-chat 里让它做 PPT，
            // 它手上没有 uking-ppt，只能从零硬写脚本 —— 慢、易错，而且正是卡死的高发场景。
            // 能力跟着版本走，就不能指望客户去点某个按钮。
            //
            // 幂等（同名覆盖），只碰我们自己的 `uking-*` 目录（同 cleanup 的 `pack_names()` 口径）；
            // 延迟 5s 让首屏先稳；失败只进日志不拦启动。
            //
            // ★ **两个目标，缺一不可**：
            //   ① `export_to(None)` → `~/.uking/skills/`：**专家提示词里硬编码的就是这个路径**
            //      （`experts.ts` 有 6 处 `~/.uking/skills/uking-xxx/scripts/…`）。这一份以前
            //      **没有任何代码在启动时刷新** —— 只有用户手点「导出技能包」才写。开发机实测：
            //      那目录停在旧快照，新发的 `uking-office-read` / `uking-office-edit` 根本不在里面，
            //      AI 照提示词跑 `read-doc.py` 直接「文件不存在」。技能发了 ≠ 调得到。
            //   ② `install_into_tools()` → 各 AI 工具自己的 skills 目录（Claude Code / OpenClaw /
            //      `~/.agents/skills` 标准位），那是 CLI 大脑**自动发现**技能的路。
            // 两条路服务两种大脑（轻助手按路径跑脚本 · CLI 按 SKILL.md 自动发现），别只做一半。
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(5));
                // 先把写错地方的 Hermes 配置搬回它真正会读的 home，再同步技能包 ——
                // 顺序不能反：技能包落点和配置落点是同一个判据，先修配置等于先确认落点。
                // 只在真搬了一次时打日志（幂等，绝大多数机器上是 no-op）。
                if let Some(msg) = providers::migrate_hermes_config_from_legacy() {
                    ulog::write("hermes", &msg);
                }
                let _ = skillpack::export_to(None);
                let _ = skillpack::install_into_tools();
            });
            // 静默自升级「下载」阶段：后台把新版绿色 exe 悄悄下到 exe 同目录暂存（不动正在运行
            // 的 exe、不弹任何提示）。下次裸启动时 apply_staged_update 原子替换。延迟 8s 让首屏
            // 先稳住，绝不阻塞启动。绿色版 / 安装版同此一套（current_exe 自动指向各自路径）。
            // U 盘护符口味跳过静默下载暂存（同上：避免用无护符的下载版把护符自动替换掉）。
            #[cfg(windows)]
            if !cfg!(feature = "usb-guard") {
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(8));
                    installer::stage_pending_update();
                });
            }
            } // ← `if !is_sidecar` 第二段到此为止（主实例专属后台活全部结束）
            // macOS：显式 Regular 激活策略（托盘 app 场景下避免被当后台应用），并强制亮窗
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            // 兜底再喊一次（窗口早在上面就显示了，这里只保证「万一上面那条路没走到」也有窗口）。
            show_main_window(app.handle());
            Ok(())
        })
        // 关闭主窗口 = 缩回托盘，而不是退出（360 习惯）；
        // 子窗口（充值 / 浏览器面板）照常关闭，否则会被一并拦住。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !cfg!(feature = "demo-uninstaller") && window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        // 小程序 webview 的 IPC 门禁。
        //
        // 【实测教训，别删】capabilities 里的 `windows: ["main","recharge"]` **拦不住这个**。
        // 那份白名单管的是 plugin/core 权限（core:window:allow-show 之类）；而
        // `generate_handler!` 注册的应用自定义命令默认不受 capability 约束 ——
        // spike 实测：label=miniapp-* 的窗口成功调到了 list_tools 并拿回完整结果。
        // 没有这道闸，一个小程序就能调 generate_image 烧额度、调 install_tool / cleanup_* 动机器。
        //
        // 小程序本来也不需要 Tauri IPC：它的一切能力走 uking://rpc（那条路才有权限核验）。
        // 所以这里一刀切拒绝，且拒在**面之下**——GUI、iframe、devtools 走的都是这一条。
        .invoke_handler({
            // 显式标注：generate_handler! 展开成泛型闭包，绑到 let 上时运行时类型推不出来。
            let inner: Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync> =
                Box::new(tauri::generate_handler![
            get_env,
            install_local,
            open_install_dir,
            register_context_menu,
            unregister_context_menu,
            list_tools,
            get_clawx_download_url,
            install_clawx,
            uu_remote_status,
            install_uu_remote,
            podapp_status,
            install_podapp,
            launch_podapp,
            list_automations,
            save_automation,
            remove_automation,
            set_automation_enabled,
            run_automation_now,
            list_automation_runs,
            read_automation_run,
            journal_inspect,
            journal_set_enabled,
            journal_clear,
            get_uuswitch_download_url,
            install_uuswitch,
            import_to_uuswitch,
            install_hermes_app,
            hermes_download_page,
            pin_to_desktop,
            backup_default_root,
            list_backups,
            backup_now,
            restore_backup,
            open_codex_guide,
            open_codex_cli_guide,
            open_claude_guide,
            open_openclaw_guide,
            open_hermes_guide,
            open_install_help,
            open_apikey_guide,
            open_codex_local_guide,
            resolve_site_url,
            open_online_page,
            fetch_online_feed,
            open_install_guide,
            codex_status,
            launch_app,
            open_dir_external,
            allow_fs_preview,
            office_to_pdf,
            hide_to_tray,
            detect_stack,
            hermes_browser_status,
            detect_hardware,
            localllm_inspect,
            localllm_catalog,
            localllm_download,
            localllm_download_cancel,
            localllm_set_download_dir,
            localllm_save_settings,
            localllm_start,
            localllm_stop,
            localllm_install,
            localllm_model_add,
            localllm_logs,
            rtk_status,
            identity_status,
            save_identity,
            publish_identity,
            chat_cost_cny,
            set_identity_secret,
            rotate_device_key,
            adopt_device_key,
            reset_device_wallet,
            link_identity,
            read_llms_doc,
            rtk_demo,
            rtk_install,
            rtk_set_enabled,
            rtk_uninstall,
            list_capability_tools,
            arena_run,
            install_capability_tool,
            load_skill,
            load_free_registry,
            install_tool,
            env_precheck,
            list_providers,
            apply_provider,
            apply_xiapan_everywhere,
            clawx_running,
            apply_clawx_managed,
            add_provider,
            update_provider,
            delete_provider,
            restore_provider,
            hidden_providers,
            addable_providers,
            set_provider_order,
            generate_image,
            generate_image_edit,
            draw::list_draw_history,
            draw::clear_draw_history,
            export_draw,
            geo_scan,
            geo_sample_report,
            geo_installed,
            geo_open_panel,
            export_video,
            export_qr_merge,
            export_skill_pack,
            install_skill_pack,
            generate_video,
            resume_video,
            list_video_history,
            instance_role,
            read_video,
            clear_video_history,
            submit_reel,
            resume_reel,
            list_reel_history,
            read_reel_file,
            delete_reel,
            report_bug,
            test_provider,
            list_remote_models,
            list_models_at_endpoint,
            probe_endpoint,
            get_draw_route,
            set_draw_route,
            query_balance,
            query_usage_breakdown,
            query_local_usage,
            list_ai_tasks,
            query_usage_meter,
            usage_sources,
            set_usage_sources,
            get_driver_status,
            get_device_key,
            save_health_report,
            ai_diagnose,
            run_fix,
            get_usage_trend,
            check_update,
            self_update,
            reinstall_latest,
            take_update_flag,
            get_setup_state,
            ai_checkup,
            doctor_report,
            upgrade_cli_tool,
            freerouter::freerouter_status,
            freerouter::freerouter_install,
            freerouter::freerouter_set_key,
            freerouter::freerouter_start,
            freerouter::freerouter_stop,
            open_recharge,
            term_open,
            term::term_open_external,
            term::term_active_count,
            term::term_snapshot_pending,
            term::term_snapshot_consume,
            term::term_write,
            term::term_resize,
            term::term_pty_info,
            term::term_close,
            term::term_ping,
            term::list_running,
            term::wait_port,
            term::prepare_openclaw_home,
            term::openclaw_webui_url,
            tasks::list_tasks,
            tasks::upsert_task,
            tasks::remove_task,
            chatstore::chat_archive_append,
            chatstore::chat_archive_load,
            chatstore::chat_archive_replace,
            chatstore::chat_archive_delete,
            chatstore::chat_archive_list,
            chatstore::chat_session_archive,
            chatstore::chat_session_restore,
            chatstore::chat_archived_list,
            chatstore::chat_session_purge,
            tasks::reorder_tasks,
            agent::claude::claude_send,
            agent::claude::claude_interrupt,
            agent::claude::claude_reset,
            agent::codex::codex_send,
            agent::codex::codex_interrupt,
            agent::codex::codex_reset,
            codex_proxy::codex_proxy_start,
            codex_proxy::codex_proxy_stop,
            codex_proxy::codex_proxy_status,
            claude_proxy::claude_bridge_status,
            claude_proxy::claude_bridge_start,
            claude_proxy::claude_bridge_stop,
            claude_bridge_enable,
            claude_bridge_disable,
            agent::chat::chat_send,
            agent::chat::chat_interrupt,
            agent::chat::chat_approve,
            fs::list_dir,
            fs::read_text_file,
            fs::read_file_data_url,
            fs::save_pasted_image,
            fs::produced_file_info,
            fs::open_produced_file,
            fs::reveal_produced_file,
            open_browser,
            browser_nav,
            browser_open,
            preview_port_alive,
            airuntime_doctor,
            agent_launch_probe,
            airuntime_run,
            airuntime_fix_elevated,
            airuntime_report_score,
            optimize_env,
            // 数据基台：本地报告 / 环境指纹 / 手动快照 / 上传同意（默认关）
            metrics_report,
            metrics_env,
            metrics_rollup,
            metrics_set_consent,
            action_run,
            action_parity_call,
            fetch_optimize_advice,
            uninstall_uking,
            cleanup_scan,
            cleanup_run,
            uninstall_ai_tool,
            collect_diagnostics,
            submit_feedback,
            open_log_dir,
            remote_assist_status,
            remote_assist_start,
            remote_assist_stop,
            remote_assist_open_audit,
            save_feedback_shot,
            open_feedback_shots_dir,
            open_miniapp,
            open_terminal_window,
            list_artifacts,
            mark_artifacts_seen,
                ]);
            move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
                let label = invoke.message.webview().label().to_string();
                if label.starts_with("miniapp-") {
                    let cmd = invoke.message.command().to_string();
                    eprintln!("[miniapp] 拒绝宿主命令调用: {label} → {cmd}");
                    invoke
                        .resolver
                        .reject("forbidden: 小程序不得直接调用宿主命令，请走 uking:// 桥");
                    return true;
                }
                inner(invoke)
            }
        })
        // 用 build + run 而不是直接 run，只为了拿到 `RunEvent::Exit`：
        // 崩溃取证靠「会话标记还在不在」判断上次是不是异常退出，所以**正常退出必须销账**，
        // 否则每次托盘退出都会给自己记一笔假崩溃，真信号立刻被噪音埋掉。
        .build(tauri::generate_context!())
        .expect("启动 U-King 失败")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // 主进程退出前清掉所有 PTY 会话，不留孤儿 pwsh（见 term.rs cleanup_all）
                term::cleanup_all();
                // 这里**刻意不给调试实例加门**：`end_session` 自己就只删 `.session-<pid>.json`
                // （只删自己那份），且开头 `SESSION_ACTIVE.swap(false)` 为 false 时直接返回 ——
                // 调试实例没开过会话，这一句本来就是空操作。加一道门只会是**带着解释的死代码**，
                // 而一句听着有道理的假解释比没注释更坏（08-24 revert 就是栽在这上面）。
                crashlog::end_session();
                // --crash-test 的判据落在这儿：销账必须真的把标记删掉。
                if std::env::args().any(|a| a == "--crash-test") {
                    let left = crashlog::session_marker_exists();
                    if left {
                        eprintln!("[crash-test] ✗ 正常退出后标记还在 —— 下次启动会误报一次崩溃");
                    } else {
                        eprintln!("[crash-test] ✓ 正常退出已销账，标记已清");
                    }
                    std::process::exit(if left { 1 } else { 0 });
                }
            }
        });
}

#[cfg(test)]
mod harness_doctor_report_tests {
    use super::format_harness_doctor_section;

    #[test]
    fn missing_doctor_stays_an_honest_optional_result() {
        let out = format_harness_doctor_section(None);
        assert!(out.contains("未安装"));
        assert!(!out.contains("❌"), "可选工具没装不能冒充机器故障");
    }

    #[test]
    fn report_only_projects_safe_summary_fields() {
        let raw = r#"{
          "summary":{"pass":3,"warn":1,"fail":1},
          "checks":[
            {"status":"pass","summary":"Node ok","details":{"path":"C:\\Users\\secret"},"fix_id":null},
            {"status":"warn","summary":"Proxy environment is active","details":{"token":"sk-never-copy-this"},"fix_id":"review_proxy_environment"},
            {"status":"fail","summary":"DSH command missing","details":{"path":"C:\\Users\\secret"},"fix_id":"install_dsh"}
          ]
        }"#;
        let out = format_harness_doctor_section(Some(raw));
        assert!(out.contains("✅ 3 通过 · ⚠️ 1 提醒 · ❌ 1 失败"));
        assert!(out.contains("review_proxy_environment"));
        assert!(out.contains("install_dsh"));
        assert!(!out.contains("secret"));
        assert!(!out.contains("sk-never-copy-this"));
        assert!(!out.contains("Node ok"), "通过项不该把截图报告淹没");
    }
}

#[cfg(test)]
mod browser_nav_tests {
    use super::validate_nav;

    /// `external` 把字符串原样交给系统 opener —— Windows 上 `file://` / `ms-settings:` /
    /// 各种自定义协议都能被拉起来。前端哪天把用户输入直接透传进来，这就成了
    /// 「让 U-King 帮我打开任意东西」。白名单跟 `open_browser` 同一套。
    #[test]
    fn external_only_accepts_https_or_loopback() {
        assert!(validate_nav("browser-t1", "external", Some("https://u-claw.org.cn")).is_ok());
        assert!(validate_nav("browser-t1", "external", Some("http://localhost:3000")).is_ok());
        assert!(validate_nav("browser-t1", "external", Some("http://127.0.0.1:5173/x")).is_ok());

        for bad in [
            "file:///C:/Windows/System32/calc.exe",
            "ms-settings:privacy",
            "javascript:alert(1)",
            "http://evil.example.com", // 非回环的裸 http 也不放
            "",
            "   ",
        ] {
            assert!(
                validate_nav("browser-t1", "external", Some(bad)).is_err(),
                "这个地址不该被放进系统浏览器: {bad:?}"
            );
        }
        assert!(validate_nav("browser-t1", "external", None).is_err(), "没给地址得报错，不能当空串放行");
    }

    /// label 不校验的话，可以拿它去驱动主窗口或别的子窗口 —— 里面有 close 和 eval。
    #[test]
    fn label_must_be_a_browser_child_window() {
        assert!(validate_nav("browser-abc", "reload", None).is_ok());
        for bad in ["main", "", "browse-abc", "../browser-abc"] {
            assert!(validate_nav(bad, "reload", None).is_err(), "非浏览器子窗口不该被驱动: {bad:?}");
        }
    }

    /// 动作白名单在校验层，不在 match 的兜底分支 —— 校验先跑，机器一个字节都不会被碰。
    #[test]
    fn unknown_action_rejected() {
        for a in ["back", "forward", "reload", "focus", "close"] {
            assert!(validate_nav("browser-x", a, None).is_ok(), "{a} 应该是合法动作");
        }
        assert!(validate_nav("browser-x", "eval", Some("https://x.com")).is_err());
        assert!(validate_nav("browser-x", "", None).is_err());
    }
}

#[cfg(test)]
mod port_probe_tests {
    /// `preview_port_alive` 的判据必须真能区分「有人听」和「没人听」——
    /// 它是 #015 那句「本机 N 端口没有服务在跑」的唯一依据，判反了就是在骗客户。
    #[test]
    fn detects_listening_vs_closed_port() {
        use std::net::{SocketAddr, TcpListener, TcpStream};
        let dur = std::time::Duration::from_millis(300);
        let probe = |p: u16| {
            [SocketAddr::from(([127, 0, 0, 1], p)), SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], p))]
                .iter()
                .any(|a| TcpStream::connect_timeout(a, dur).is_ok())
        };
        // 真起一个监听器 → 必须探得到
        let l = TcpListener::bind("127.0.0.1:0").expect("bind 失败");
        let port = l.local_addr().unwrap().port();
        assert!(probe(port), "端口 {port} 上明明有监听器，却探成没有");
        // 关掉 → 必须探不到（否则「没起服务」这句话永远不会出现）
        drop(l);
        assert!(!probe(port), "端口 {port} 已经关了，却还说有服务在跑");
    }
}

/// `--help` / `--version` 的守门测试。
#[cfg(test)]
mod cli_help_tests {
    /// 回归：`--version` / `-V` / `version` / `--help` / `-h` / `help` 六种写法
    /// 曾经**全是零字节输出 + 退出码 0**。对一个主打「AI 可直接调用」的 CLI，
    /// 这是最基本的自我介绍缺失 —— AI 摸不到版本号就没法判断该按哪份说明书办事。
    ///
    /// 这里判的是帮助正文本身（进程退出没法在单测里跑），守住「非空 + 有真内容」。
    #[test]
    fn help_text_actually_says_something_useful() {
        let h = super::cli_help_text();
        assert!(h.len() > 200, "帮助正文只有 {} 字节，等于没写", h.len());
        assert!(h.contains(env!("CARGO_PKG_VERSION")), "帮助里没有版本号");
        // 入口必须在：AI 读完这段应该知道下一步敲什么
        for want in ["action list --json", "action run", "mcp serve", "--envfp"] {
            assert!(h.contains(want), "帮助里少了入口 `{want}`");
        }
        // 退出码契约是这份 CLI 对调用方的承诺，别只写在 llms.txt 里
        assert!(h.contains("退出码 0=成功"), "帮助里没有交代退出码约定");
        // Mac 上二进制不在 PATH 上 —— 位置必须报出来，否则照着敲还是 command not found
        let exe = std::env::current_exe().expect("测试进程总该有 current_exe");
        assert!(h.contains(&exe.display().to_string()), "帮助里没报本机可执行文件位置");
    }

    /// 每行都塞一遍绝对路径会把对齐弄毁、根本没法读 —— 报一次就够。
    /// （llms.txt 那份取舍相反：AI 逐条照抄，所以每行写全路径。）
    #[test]
    fn help_text_states_the_exe_path_once_not_on_every_line() {
        let h = super::cli_help_text();
        let exe = std::env::current_exe().unwrap().display().to_string();
        let hits = h.matches(&exe).count();
        assert_eq!(hits, 1, "全路径在帮助里出现了 {hits} 次，应当只报一次");
    }
}

/// 「给 AI 的说明书」打在**真动作表**上的守门测试。
///
/// **为什么非得单开一条**：`identity.rs` 按模块独立铁律不 import `actions`，
/// 它的单元测试只能用手捏的 manifest fixture —— 而 fixture 是照着**我以为的**形状捏的。
/// 第一版就栽在这儿：字段名写成 `effects.effect`（真实是 `effects.class`），
/// 单元测试全绿，真跑一次却渲染出「0 个只读 + 50 个写」。
/// **测试验的是假设，不是现实。** 这条打在 `actions::manifest()` 上，形状一漂就红。
#[cfg(test)]
mod llms_manifest_tests {
    #[test]
    fn llms_renders_against_the_real_manifest() {
        let m = crate::actions::manifest();
        let id = crate::identity::Identity::default();
        let s = crate::identity::render_llms(&id, &m, &[], &crate::skillpack::skill_catalog());

        // 真实动作表里只读和写**都不该是 0** —— 任何一边归零就说明字段名对不上了。
        let total = m["actions"].as_array().map(|a| a.len()).unwrap_or(0);
        assert!(total > 20, "动作表怎么只有 {total} 条？");
        assert!(!s.contains("：0 个只读"), "只读被算成 0 —— manifest 字段名漂了:\n{}", &s[..s.len().min(1200)]);
        assert!(!s.contains("+ 0 个写"), "写被算成 0 —— manifest 字段名漂了");

        // 抽查几个真实存在的动作 id 必须出现在说明书里
        for want in [crate::actions::STACK_INSPECT, crate::actions::DRIVER_APPLY, crate::actions::IDENTITY_INSPECT] {
            assert!(s.contains(want), "说明书里少了动作 {want}");
        }
        // 破坏性动作必须被打上标记（provider.delete 是清单里唯一常驻的 destructive）
        assert!(s.contains("⚠️ **破坏性**"), "destructive 没被标出来，AI 会当普通写去调");
    }

    /// 说明书曾经笼统写着「写（N 个，**每一个都会改这台机器**）调用前必须让人同意」。
    /// 真实动作表里有 15 个非只读动作是 `confirmation: never`（13 个 browser 页面内交互
    /// + `runtime.origin.save` + 2 个 `app.imagefix.*`）——那是有意的取舍，但**这句话正是
    /// AI 判断「我该不该问用户」的直接依据**，说反了它就会在该问的时候不问。
    ///
    /// 这条守的是：门禁分组必须来自 manifest 的真实字段，不是文案凭空断言。
    #[test]
    fn manual_does_not_claim_every_write_action_needs_confirmation() {
        let m = crate::actions::manifest();
        let id = crate::identity::Identity::default();
        let s = crate::identity::render_llms(&id, &m, &[], &crate::skillpack::skill_catalog());

        let actions = m["actions"].as_array().expect("manifest 得有 actions");
        let ungated = actions
            .iter()
            .filter(|a| a["effects"]["class"].as_str() != Some("read"))
            .filter(|a| a["effects"]["confirmation"].as_str() == Some("never"))
            .count();

        // 🔴 先证明判据非空：一条也没有的话下面全是空对空，就成了恒绿考题。
        assert!(
            ungated > 0,
            "真实动作表里一个无门禁的写动作都没有？那这条用例什么也证明不了 —— \
             要么 manifest 字段漂了，要么这个断言该删"
        );
        assert!(
            !s.contains("每一个都会改这台机器"),
            "说明书又开始笼统断言所有写动作都要确认了，而实际有 {ungated} 个不要"
        );
        assert!(
            s.contains(&format!("### 写 · 不需要确认（{ungated} 个）")),
            "无门禁的那 {ungated} 个动作必须单独成节说清楚，别混在「要人同意」里"
        );
    }

    /// 说明书曾经写死四个技能目录 + 一句「按你是哪个 AI 挑一处，**内容完全相同**」。
    /// 那句话是错的：`skillpack::skill_targets()` 给每个落点都设了门禁（对应的 AI 没装
    /// 就不铺），本机 `~/.agents/skills` 就是空的 —— AI 照着去找只会以为同步坏了。
    /// 现在改成现场 stat，这条守着别退回去写死名单。
    #[test]
    fn manual_does_not_claim_all_skill_dirs_are_identical() {
        let m = crate::actions::manifest();
        let id = crate::identity::Identity::default();
        let s = crate::identity::render_llms(&id, &m, &[], &crate::skillpack::skill_catalog());

        assert!(
            !s.contains("内容完全相同"),
            "又开始笼统断言四个技能目录内容相同了 —— 没装的那家根本不会被铺进去"
        );
        // 空目录必须给出「不是坏了」的解释，否则读的人会得出错误结论。
        // 三家 AI 全装齐的机器上不会出现空目录段，那时这条不适用。
        if s.contains("**当前是空的**") {
            assert!(
                s.contains("不是坏了"),
                "列了空目录却不解释为什么空，等于告诉 AI「同步炸了」"
            );
        }
    }

    /// 说明书是**给 AI 读的地图**，教的命令必须是这台机器上真敲得动的。
    /// 以前全文硬编码 `u-king-mini.exe` + 「装在这台 Windows 电脑上」，
    /// Mac 上照着敲第一条就是 command not found。
    #[test]
    fn manual_teaches_a_command_that_exists_on_this_platform() {
        let m = crate::actions::manifest();
        let id = crate::identity::Identity::default();
        let s = crate::identity::render_llms(&id, &m, &[], &crate::skillpack::skill_catalog());

        #[cfg(not(windows))]
        {
            // 只判渲染器自己产的那部分。动作**描述**里提到 `wsl.exe` 之类是动作自己的文案，
            // 归它自己管（本 PR 顺手清了 rtk.uninstall 那句），不该由这条用例连坐。
            assert!(
                !s.contains("u-king-mini.exe"),
                "非 Windows 的说明书还在教 AI 敲 u-king-mini.exe"
            );
            assert!(!s.contains("装在这台 Windows 电脑上"), "平台措辞没跟着平台走");
        }
        // 三平台共有的判据：教的那条命令得指向真实存在的可执行文件。
        let exe = std::env::current_exe().expect("测试进程总该有 current_exe");
        assert!(
            s.contains(&exe.display().to_string()) || s.contains("u-king-mini"),
            "说明书里找不到可照抄的调用命令"
        );
    }
}
