//! Codex ↔ DeepSeek 本地翻译代理的启停 + Codex 配置切换（独立可插拔模块）。
//!
//! 只有 Codex 需要它（Claude/Hermes/U-King 助手都直连 DeepSeek）。启动=Codex 走本地代理→DeepSeek（省）；
//! 停止=还原成之前的配置（虾盘云 gpt-5.x-codex，贵几十倍）。代理是内嵌的 Node 脚本（便携 Node 跑）。
//!
//! 纯 std：写脚本 include_str! → 便携 node 起进程 → 改 ~/.codex/config.toml（起前备份、停后还原）。

use std::io::Write;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Mutex, OnceLock};

pub const PROXY_PORT: u16 = 15722;
const PROXY_SCRIPT: &str = include_str!("../resources/codex-deepseek-proxy.mjs");
/// 走代理时 Codex 默认用的 DeepSeek 模型（便宜、非推理，聊天/编程够用）。
const DEEPSEEK_MODEL: &str = "deepseek-v4-flash";

fn running() -> &'static Mutex<Option<Child>> {
    static R: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

fn uking_home() -> PathBuf {
    dirs_home().join(".uking")
}
fn dirs_home() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// ~/.codex/config.toml（尊重 CODEX_HOME）。
fn codex_config_path() -> PathBuf {
    let home = std::env::var("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|_| dirs_home().join(".codex"));
    home.join("config.toml")
}
fn codex_backup_path() -> PathBuf {
    codex_config_path().with_extension("toml.uking-codexproxy-bak")
}

/// 当前 Codex 路由（选了哪个模型 / 走虾盘云还是用户自定义端点）。持久化到
/// ~/.uking/codex-route.json —— status 回显 + resume 自愈都据它用「对的模型」重启代理。
/// kind=xiapan：走虾盘云 chat/completions + 设备 Key（我们赚，默认）；
/// kind=custom：用户自己的 OpenAI 兼容端点 + 自己的 Key（BYOK，粘 key/端点，客户自付）。
#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct CodexRoute {
    #[serde(default = "default_kind")]
    pub kind: String,
    pub model: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub key: String,
}
fn default_kind() -> String {
    "xiapan".into()
}
fn default_route() -> CodexRoute {
    CodexRoute {
        kind: "xiapan".into(),
        model: DEEPSEEK_MODEL.into(),
        label: "DeepSeek V4 Flash（省）".into(),
        base_url: String::new(),
        key: String::new(),
    }
}
fn route_file() -> PathBuf {
    uking_home().join("codex-route.json")
}
fn load_route() -> Option<CodexRoute> {
    serde_json::from_str(&std::fs::read_to_string(route_file()).ok()?).ok()
}
fn save_route(r: &CodexRoute) {
    if let Ok(s) = serde_json::to_string_pretty(r) {
        let _ = std::fs::create_dir_all(uking_home());
        let _ = std::fs::write(route_file(), s);
    }
}

/// 虾盘云 chat/completions 默认端点（custom 路由回退用；xiapan 现在走直连不经这里）。
const XIAPAN_CHAT: &str = "https://api.u-claw.org.cn/v1/chat/completions";

/// ★ 直连模式：虾盘云的 OpenAI 兼容根地址（Codex 自己拼 `/responses`）。
const XIAPAN_BASE: &str = "https://api.u-claw.org.cn/v1";

/// ★ 直连模式专用模型名 —— 服务端把它映射到 `deepseek-v4-flash`，价格完全一致。
///
/// 为什么要单独一个名字：`/v1/responses` 能不能透传，取决于 new-api 里**渠道的类型**。
/// 老的 DeepSeek 类渠道（type=43）会试图把 responses 转换成 chat 格式，而那个转换没实现，
/// 直接报 `convert_request_failed`；只有 OpenAI 类渠道（type=1）才是原样透传。
/// 所以服务端单独建了一条 type=1 渠道、只挂这一个模型名 —— 新名字只存在于那条渠道上，
/// 路由不可能跑偏，真出问题禁用它就回到老样子，**存量客户和其它工具一个字节都不受影响**。
///
/// ⚠️ 这个名字**同时**是 `providers.rs` 里虾盘云预设的 `codex_model`（切驱动那条路写的配置）。
/// 两条路都可能先落地，值不一样就会「界面写着 A、实际跑的是 B」——
/// 所以这里不再自己抄一份，见 `direct_model()`。
fn direct_model() -> String {
    crate::providers::xiapan_codex_model()
}

/// 直连模式的展示名。**必须和实际跑的模型一致** —— 沿用用户上次选的 label 会出现
/// 「界面写着 DeepSeek V4 Pro、实际跑的是 flash」，回显骗人比不显示更糟。
const DIRECT_LABEL: &str = "DeepSeek V4 Flash（省 · 直连）";

/// 这条路由能不能不用本地代理、让 Codex 直连虾盘云。
///
/// **只有 `kind=xiapan`（走我们自己的端点）才行**：直连要求上游原生支持 Responses API，
/// 这是我们验证过的；而 `kind=custom` 是 BYOK 用户自己填的端点，什么形状都可能，
/// 没法假设它支持 responses —— 那条继续走本地代理翻译，行为一字不变。
fn can_direct(route: &CodexRoute) -> bool {
    route.kind != "custom"
}

/// base_url → chat/completions 完整端点；空则回退虾盘云。
/// 归一逻辑本身在公共层（`installer::to_chat_completions_url`），两个翻译桥共用一份；
/// **兜底端点各留各的** —— codex 这条空了回退虾盘云，claude 那条空了直接报错。
fn to_chat_endpoint(base: &str) -> String {
    crate::installer::to_chat_completions_url(base).unwrap_or_else(|| XIAPAN_CHAT.into())
}

/// 代理自己写的日志（`~/.uking/codex-proxy.log`）。代理进程的 stdout/stderr 定向到 null
/// （CREATE_NO_WINDOW 不能弹黑窗），所以它自己写文件；反馈页的诊断采集会带上尾部。
pub fn log_path() -> PathBuf {
    uking_home().join("codex-proxy.log")
}

/// 释放内嵌代理脚本到 ~/.uking/codex-deepseek-proxy.mjs。
fn ensure_script() -> Result<PathBuf, String> {
    let dir = uking_home();
    std::fs::create_dir_all(&dir).map_err(|e| format!("建 ~/.uking 失败: {e}"))?;
    let p = dir.join("codex-deepseek-proxy.mjs");
    std::fs::write(&p, PROXY_SCRIPT).map_err(|e| format!("写代理脚本失败: {e}"))?;
    Ok(p)
}

// `find_node()` 已下沉到公共层 `installer::find_node()` —— 第二个模块（claude_proxy 的
// messages↔chat 桥）也要起 Node 脚本，同一份四级兜底不能有两份（宪法第 8/12 条）。
use crate::installer::find_node;

/// 起代理进程 with route。**换模型/换端点必须重启进程**（env 只在进程启动时读一次），
/// 所以这里先杀掉在跑的旧代理、稍等端口释放，再用新 route 的模型/端点/Key 起一个新的。
fn spawn_proxy_with(route: &CodexRoute) -> Result<(), String> {
    // 杀旧（切模型要重启；kill 后短暂等一下让 15722 端口释放，避免新进程 EADDRINUSE）
    let had_old = {
        let mut g = running().lock().map_err(|_| "锁失败".to_string())?;
        g.take().map(|mut ch| ch.kill()).is_some()
    };
    if had_old {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let script = ensure_script()?;
    let node = find_node().ok_or("没找到 Node（便携 Node 未装？先装一个工具会自动装）")?;
    // xiapan：虾盘云端点 + 设备 Key。custom：用户端点 + 用户 Key（Key 空则回退设备 Key）。
    let (upstream, key) = if route.kind == "custom" {
        let k = if route.key.trim().is_empty() {
            crate::device::device_key_offline().map_err(|e| format!("拿不到设备 Key: {e}"))?
        } else {
            route.key.clone()
        };
        (to_chat_endpoint(&route.base_url), k)
    } else {
        let k = crate::device::device_key_offline().map_err(|e| format!("拿不到设备 Key: {e}"))?;
        (XIAPAN_CHAT.to_string(), k)
    };

    let mut c = std::process::Command::new(&node);
    c.arg(&script)
        .env("UKING_CODEX_PROXY_PORT", PROXY_PORT.to_string())
        .env("UKING_CODEX_KEY", &key)
        .env("UKING_CODEX_UPSTREAM", &upstream)
        .env("UKING_CODEX_MODEL", &route.model)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = c.spawn().map_err(|e| format!("起代理进程失败: {e}"))?;
    if let Ok(mut g) = running().lock() {
        *g = Some(child);
    }
    Ok(())
}

/// 把 Codex 配置指向我们的路由（起前备份原配置，停时还原）。
///
/// **两种形态，同一个 provider id**：
/// - 直连（`kind=xiapan`）：`base_url` 直指虾盘云，Codex 自己发 `/v1/responses`，**不起本地代理**。
///   DeepSeek 2026-07-31 起原生支持 Responses API（官方文档明说是"为了满足大家对 Codex 的需求"），
///   我们那层"收 responses → 转 chat/completions"的翻译代理就没必要存在了。
/// - 代理（`kind=custom`）：保持原样指向 127.0.0.1:15722，BYOK 端点仍靠本地翻译。
///
/// provider 内部 id 一律保持 `uking_deepseek` 不变 —— restore/resume/status 和 `cleanup.rs`
/// 的足迹识别全靠这个标记，换掉会让**存量客户的配置认不出来**（卸载时清不干净、还原时回滚不了）。
/// 变的只是 `base_url` / 认证那几行。
fn write_codex_proxy_config(route: &CodexRoute) -> Result<(), String> {
    let path = codex_config_path();
    if let Some(par) = path.parent() {
        std::fs::create_dir_all(par).ok();
    }
    // 备份原配置（仅当还没备份过，避免二次启动把代理配置当原配置备份掉）
    let bak = codex_backup_path();
    if path.exists() && !bak.exists() {
        std::fs::copy(&path, &bak).map_err(|e| format!("备份 codex 配置失败: {e}"))?;
    }
    // 直连固定跑 DIRECT_MODEL（服务端只有那条 type=1 渠道支持 responses），所以展示名也得跟着改 ——
    // 沿用旧 label 会出现「写着 DeepSeek V4 Pro、实际跑的是 flash」这种回显骗人的情况
    // （本机 --codex-route-test 实测到：label=DeepSeek V4 Pro / model=…-flash-codex）。
    let label = if can_direct(route) {
        DIRECT_LABEL
    } else if route.label.trim().is_empty() {
        route.model.as_str()
    } else {
        route.label.as_str()
    };
    let name = label.replace('"', "'");
    let cfg = if can_direct(route) {
        // 直连：Key 直接写进配置（Codex 的 env_key 要求用户自己设环境变量，装机场景做不到）。
        // 这里放的是**设备内置 Key**，本来就是发给这台机器用的，和 auth.json 里存的等价。
        let key = crate::device::device_key_offline().map_err(|e| format!("拿不到设备 Key: {e}"))?;
        let dm = direct_model();
        format!(
            "# managed by U-King (Codex 模型路由 · 直连)\n\
             model = \"{dm}\"\n\
             model_provider = \"uking_deepseek\"\n\n\
             [model_providers.uking_deepseek]\n\
             name = \"U-King · {name}\"\n\
             base_url = \"{XIAPAN_BASE}\"\n\
             wire_api = \"responses\"\n\
             requires_openai_auth = false\n\
             experimental_bearer_token = \"{key}\"\n"
        )
    } else {
        format!(
            "# managed by U-King (Codex 模型路由 · 本地代理)\n\
             model = \"{model}\"\n\
             model_provider = \"uking_deepseek\"\n\n\
             [model_providers.uking_deepseek]\n\
             name = \"U-King · {name}\"\n\
             base_url = \"http://127.0.0.1:{PROXY_PORT}/v1\"\n\
             wire_api = \"responses\"\n\
             requires_openai_auth = false\n\
             experimental_bearer_token = \"uking-local-proxy\"\n",
            model = route.model,
        )
    };
    let mut f = std::fs::File::create(&path).map_err(|e| format!("写 codex 配置失败: {e}"))?;
    f.write_all(cfg.as_bytes()).map_err(|e| format!("写 codex 配置失败: {e}"))?;
    Ok(())
}

/// 还原 Codex 配置（从备份恢复；没备份就删掉我们写的代理配置，让 codex 回默认）。
fn restore_codex_config() {
    let path = codex_config_path();
    let bak = codex_backup_path();
    // 只有当前配置确实还是我们的代理配置时才动它——用户可能已在别处把 codex 切去其它
    // 供应商（apply_codex 直写覆盖了代理配置），这时拿旧备份盖回去会吃掉用户的新选择。
    // 过期备份顺手清掉，避免下次误还原（省钱路由默认开之后这条路径更常见）。
    let on_proxy = std::fs::read_to_string(&path)
        .map(|s| s.contains("uking_deepseek"))
        .unwrap_or(false);
    if !on_proxy {
        let _ = std::fs::remove_file(&bak);
        return;
    }
    if bak.exists() {
        let _ = std::fs::copy(&bak, &path);
        let _ = std::fs::remove_file(&bak);
    } else if path.exists() {
        // 只在确认是我们写的（含标记）时删，避免误删用户配置
        if std::fs::read_to_string(&path).map(|s| s.contains("managed by U-King")).unwrap_or(false) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// 启动 / 切换 Codex 模型路由：按 route 起代理（换模型=重启）+ 切 codex 配置 + 持久化选择。
/// route 省略（内部默认省钱路由调用）→ 用上次持久化的路由，没有则默认 DeepSeek Flash（省）。
#[tauri::command]
pub fn codex_proxy_start(route: Option<CodexRoute>) -> Result<serde_json::Value, String> {
    let route = route.or_else(load_route).unwrap_or_else(default_route);
    let direct = can_direct(&route);
    if direct {
        // 直连不需要本地代理。**把可能还活着的旧代理杀掉** —— 从旧版升上来的客户，
        // 15722 上往往还跑着一个上一版起的进程；不收拾它，端口白占着不说，
        // 下次 `resume_if_configured` 探到「端口有人服务」还会以为一切正常。
        stop_proxy_process();
    } else {
        spawn_proxy_with(&route)?;
    }
    write_codex_proxy_config(&route)?;
    save_route(&route);
    crate::ulog::write(
        "codex",
        &format!(
            "Codex 路由已切换：{}（kind={} model={}）",
            if direct { "直连虾盘云 responses，无本地代理" } else { "本地代理翻译" },
            route.kind,
            if direct { direct_model() } else { route.model.clone() }
        ),
    );
    Ok(serde_json::json!({
        "running": true, "port": PROXY_PORT, "direct": direct,
        "model": if direct { direct_model() } else { route.model.clone() },
        "kind": route.kind,
        "label": if direct { DIRECT_LABEL.to_string() } else { route.label.clone() }
    }))
}

/// 杀掉本地代理进程（幂等；没有就什么都不做）。
///
/// 分两步，缺一不可：
/// 1. 本进程亲手起的那个 `Child`；
/// 2. **上一次运行遗留的孤儿代理** —— Windows 上子进程比父进程长寿，客户关掉 U-King
///    （其实只是缩托盘）甚至重启 U-King 之后，老代理往往还在 15722 上听着。切到直连时
///    不收拾它，端口就白占着，而且 `resume_if_configured` 探到「端口有人服务」还会
///    误判成一切正常。本机实测：改造前起的代理活了 7 小时还在。
///
/// 只按 **PID + 端口** 定位，绝不按镜像名杀 —— `node.exe` 是客户机上到处都是的名字，
/// `taskkill /IM node.exe` 会连客户自己跑的服务一起端掉（宪法：绝不按裸镜像名结束进程）。
fn stop_proxy_process() {
    if let Ok(mut g) = running().lock() {
        if let Some(mut ch) = g.take() {
            let _ = ch.kill();
        }
    }
    kill_orphan_proxy();
}

/// 干掉占着 15722 的孤儿代理进程（按端口找 PID，再核对确实是我们的脚本才杀）。
#[cfg(windows)]
fn kill_orphan_proxy() {
    // netstat 找监听 15722 的 PID
    let Ok(out) = crate::installer::run_capture_raw(
        "netstat",
        &["-ano", "-p", "TCP"],
        None,
    ) else {
        return;
    };
    let needle = format!(":{PROXY_PORT}");
    for line in out.1.lines() {
        if !line.contains(&needle) || !line.contains("LISTENING") {
            continue;
        }
        let Some(pid) = line.split_whitespace().last().and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid == 0 || pid == std::process::id() {
            continue;
        }
        // 核对这个 PID 的命令行确实是我们的代理脚本，别误杀占了同一端口的别家程序
        let ps = format!(
            "(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").CommandLine"
        );
        let is_ours = crate::installer::run_capture_raw(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &ps],
            None,
        )
        .map(|(_, s)| s.contains("codex-deepseek-proxy"))
        .unwrap_or(false);
        if !is_ours {
            crate::ulog::write("codex", &format!("端口 {PROXY_PORT} 被 pid={pid} 占用，但不是我们的代理，未动它"));
            continue;
        }
        let _ = crate::installer::run_capture_raw("taskkill", &["/PID", &pid.to_string(), "/F"], None);
        crate::ulog::write("codex", &format!("已清理遗留的本地代理进程 pid={pid}（直连模式不再需要它）"));
    }
}

#[cfg(not(windows))]
fn kill_orphan_proxy() {}

/// 停止：杀代理 + 还原 codex 配置（回到贵的 gpt-5.x-codex）。
#[tauri::command]
pub fn codex_proxy_stop() -> Result<serde_json::Value, String> {
    stop_proxy_process();
    restore_codex_config();
    crate::ulog::write("codex", "Codex 路由已停止，配置已还原");
    Ok(serde_json::json!({ "running": false }))
}

/// 15722 端口现在有没有进程在听。启动自愈 + 运行中看门狗共用同一探针。
fn proxy_port_serving() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], PROXY_PORT)),
        std::time::Duration::from_millis(400),
    )
    .is_ok()
}

/// config 还指着本地代理端口吗？这是「该有代理进程在跑」的判据：
/// 代理形态（custom/BYOK）base_url 是 `127.0.0.1:{PROXY_PORT}`；直连 / 已还原的
/// config 里没有这个字符串。status（direct 判定）、看门狗（要不要拉回）、
/// resume_if_configured（存量迁移）三处共用一个口径，不许各写各的。
fn cfg_points_at_local_proxy(cfg: &str) -> bool {
    cfg.contains(&format!("127.0.0.1:{PROXY_PORT}"))
}

/// U-King 启动时的路由自愈（修「开了省钱路由的客户，重启电脑/重开 U-King 后 codex 废了」）：
/// codex config.toml 还指着我们的本地代理（`uking_deepseek` 标记）但 15722 端口没人服务
/// → 自动拉起代理进程。**只自愈、不新开**：没开过路由（无标记）绝不动 codex 配置。
///
/// 为什么会出现这个状态：代理是 U-King 子进程，Windows 上子进程可比父进程长寿——关掉
/// U-King 代理往往还活着（codex 照常能用），但重启电脑后代理没了、config 还指着
/// 127.0.0.1:15722 → codex 全废，客户只能重开 U-King 手动再点一次「启动省钱路由」。
/// best-effort：失败静默（前端 Codex 专区仍可手动启动）。
pub fn resume_if_configured() {
    let Ok(cfg) = std::fs::read_to_string(codex_config_path()) else {
        return;
    };
    if !cfg.contains("uking_deepseek") {
        return; // 没开过我们的路由 → 绝不动客户的 codex 配置
    }
    let route = load_route().unwrap_or_else(default_route);

    // ★ 存量客户迁移：配置还指着本地代理，但这条路由现在能直连了 → 就地改写成直连。
    // 这是**从旧版升上来的客户唯一的迁移入口**（他们不会主动去 Codex 专区再点一次），
    // 不做的话他们会一直吊在 15722 上，白留一个 Node 进程，还继续吃「梯子把发往本机的
    // 请求截走」那个 502。改写是幂等的：写完 cfg 里就没有 127.0.0.1 了，下次直接跳过。
    if can_direct(&route) {
        if cfg_points_at_local_proxy(&cfg) {
            stop_proxy_process(); // 顺手收拾掉可能还活着的旧代理
            match write_codex_proxy_config(&route) {
                Ok(()) => crate::ulog::write("codex", "已把 Codex 从本地代理迁到直连（无需再占 15722）"),
                Err(e) => crate::ulog::write("codex", &format!("迁到直连失败，保持原样：{e}")),
            }
        }
        return; // 直连不需要代理进程
    }

    // 以下是 custom(BYOK) 路由：仍靠本地代理翻译，维持原有自愈逻辑。
    // 端口已有人服务（上次的代理进程还活着）→ 不重复起，避免新进程 bind 失败留一个死 Child
    if proxy_port_serving() {
        return;
    }
    let _ = spawn_proxy_with(&route);
}

/// 代理运行中看门狗周期（秒）。别太密：端口探测是 400ms 超时的 TCP connect，
/// 太密只是白烧 IO；也别太疏：代理死了要尽快拉回。
const WATCHDOG_INTERVAL_SECS: u64 = 15;
/// 连续失败多少次就退避（秒）。坏上游（BYOK 端点挂了）时代理会反复秒退，
/// 没有这个上限看门狗会变成无限重启风暴 —— 自愈救不了死端点，别假装能。
const WATCHDOG_BACKOFF_SECS: u64 = 120;

/// 代理**运行中**自愈：`resume_if_configured` 只覆盖 U-King 启动那一下，覆盖不了
/// 「代理跑着跑着被 Windows 杀掉 / OOM / 端口被占」——那种情况 config 还指着
/// 127.0.0.1:15722，但没人服务，codex 全废，客户只能重开 U-King 才恢复。
///
/// 本看门狗后台循环：**只要 config 仍指本地端口**（custom/BYOK 形态才有的特征），
/// 就探端口有没有人服务，没有就拉回代理。直连形态 / 用户已停止的 config 里不含
/// `127.0.0.1:{PROXY_PORT}`，整轮跳过 —— 不会把直连客户改成代理，也不会跟
/// `codex_proxy_stop` 打架（停止会先还原 config，config 一改这里就看不见本地端口了）。
///
/// 防风暴：连续失败 `WATCHDOG_BACKOFF_SECS` 秒退避，不无限重试。
pub fn start_proxy_watchdog() {
    std::thread::spawn(|| {
        let mut consec_fails: u32 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(WATCHDOG_INTERVAL_SECS));
            // 只关心「config 还指着本地端口」的形态 —— 那是 custom(BYOK) 代理在用的。
            let cfg = std::fs::read_to_string(codex_config_path()).unwrap_or_default();
            if !cfg_points_at_local_proxy(&cfg) {
                consec_fails = 0; // 直连 / 已停 / 没开过 → 不归看门狗管
                continue;
            }
            if proxy_port_serving() {
                consec_fails = 0;
                continue;
            }
            let route = load_route().unwrap_or_else(default_route);
            match spawn_proxy_with(&route) {
                Ok(()) => {
                    consec_fails = 0;
                    crate::ulog::write("codex", "看门狗：代理运行中退出，已自动拉回");
                }
                Err(e) => {
                    consec_fails += 1;
                    crate::ulog::write("codex", &format!("看门狗：拉起代理失败（第{consec_fails}次）：{e}"));
                    if consec_fails >= 3 {
                        // 死端点救不回来：退避一阵再试，别把端口探测当无限重启器。
                        // 退避期间 codex 仍然坏着，但至少不会每 15s 空转拉一次。
                        crate::ulog::write("codex", "看门狗：连续失败退避（BYOK 上游可能挂了，等客户手动处理）");
                        std::thread::sleep(std::time::Duration::from_secs(WATCHDOG_BACKOFF_SECS));
                    }
                }
            }
        }
    });
}

/// 状态：省钱路由现在到底通不通。
///
/// ⚠️ `running` 的判据分两种，别只看进程：**直连模式压根没有进程**，
/// 拿「Node 子进程还活着吗」去问它，答案永远是 false —— 前端会显示成「没开」，
/// 而客户明明正用着。直连下的事实判据是「codex 配置指向我们、且不再指本地端口」。
#[tauri::command]
pub fn codex_proxy_status() -> serde_json::Value {
    let cfg = std::fs::read_to_string(codex_config_path()).unwrap_or_default();
    let on_proxy = cfg.contains("uking_deepseek");
    let direct = on_proxy && !cfg_points_at_local_proxy(&cfg);
    let proc_alive = running()
        .lock()
        .ok()
        .map(|mut g| {
            if let Some(ch) = g.as_mut() {
                // try_wait: Ok(None)=还活着；Ok(Some)=已退出
                matches!(ch.try_wait(), Ok(None))
            } else {
                false
            }
        })
        .unwrap_or(false);
    let route = load_route().unwrap_or_else(default_route);
    serde_json::json!({
        // 直连：配置就位即算在跑（无进程可查）；代理：仍要求进程活着
        "running": if direct { true } else { proc_alive },
        "direct": direct,
        "on_proxy": on_proxy, "port": PROXY_PORT,
        "model": if direct { direct_model() } else { route.model.clone() },
        "kind": route.kind,
        "label": if direct { DIRECT_LABEL.to_string() } else { route.label.clone() }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(kind: &str) -> CodexRoute {
        CodexRoute { kind: kind.into(), model: "m".into(), label: "L".into(), base_url: String::new(), key: String::new() }
    }

    /// BYOK（custom）**必须**继续走本地代理。
    ///
    /// 直连的前提是上游原生支持 Responses API —— 那是我们对自己端点验证过的事实。
    /// custom 是用户自己填的端点，什么形状都可能；把它也切成直连，等于拿"我们的上游支持"
    /// 去替"用户的上游"打包票，BYOK 客户会当场全废。
    #[test]
    fn byok_never_goes_direct() {
        assert!(!can_direct(&route("custom")), "custom(BYOK) 端点不保证支持 responses，必须走代理翻译");
        assert!(can_direct(&route("xiapan")), "虾盘云端点已验证支持 responses，应走直连");
        assert!(can_direct(&route("")), "kind 缺省视同 xiapan（default_kind）");
    }

    /// 直连配置的三条硬约束（任一条破了，存量客户就出事）。
    #[test]
    fn direct_config_keeps_marker_and_drops_local_port() {
        // 用固定 key 走纯字符串拼装，避免测试依赖真实设备指纹
        let dm = direct_model();
        let cfg = format!(
            "# managed by U-King (Codex 模型路由 · 直连)\nmodel = \"{dm}\"\n\
             model_provider = \"uking_deepseek\"\n\n[model_providers.uking_deepseek]\n\
             base_url = \"{XIAPAN_BASE}\"\nwire_api = \"responses\"\n"
        );
        // ① provider id 不许变 —— restore/resume/cleanup.rs 全靠它认出「这是我们写的」
        assert!(cfg.contains("uking_deepseek"), "换掉标记会让存量客户的配置认不出来");
        // ② 直连里不许再出现本地端口，否则 status/resume 会把它当成还在代理模式
        assert!(!cfg.contains(&format!("127.0.0.1:{PROXY_PORT}")), "直连配置不该再指本地代理端口");
        // ③ 必须是 responses 协议 + 那个专用模型名（服务端只有 type=1 渠道挂了它）
        assert!(cfg.contains("wire_api = \"responses\""));
        assert!(cfg.contains(&dm), "直连必须用专用模型名，裸名走的是不支持 responses 的老渠道");
        // ④ 单一真相源：直连模型必须就是虾盘云预设的 codex_model（切驱动那条路写的同一个值）。
        //    2026-08-02 之前这两处各存一份，预设是贵的 gpt-5.3-codex、直连是便宜的，
        //    哪条路后落地就跑哪个 —— 客户的账单取决于他点按钮的顺序。
        assert_eq!(dm, crate::providers::xiapan_codex_model(), "直连模型和虾盘云预设漂移了");
        assert!(!dm.is_empty(), "拿不到模型名就会写出 model = \"\" 的死配置");
    }

    /// 看门狗判据：只有「config 还指着本地代理端口」才算「该有代理进程在跑」。
    /// 这是看门狗（要不要拉回）+ status（direct 判定）+ resume_if_configured（存量迁移）
    /// 三处共用的同一句话，测试钉死它别被改飘 —— 改飘了就有人 / 有机器分不清
    /// 代理形态和直连形态，status 会说「没开」、看门狗会拉一个不需要的进程。
    #[test]
    fn cfg_points_at_local_proxy_is_the_watchdog_gate() {
        // 代理形态（custom/BYOK）：base_url 就是本地端口
        let proxy_cfg = format!(
            "model = \"deepseek-v4-flash\"\nmodel_provider = \"uking_deepseek\"\n\
             [model_providers.uking_deepseek]\nbase_url = \"http://127.0.0.1:{PROXY_PORT}/v1\"\n"
        );
        assert!(cfg_points_at_local_proxy(&proxy_cfg), "代理形态必须被看门狗认出");

        // 直连形态：base_url 是虾盘云，不含本地端口 → 看门狗必须整轮跳过
        let direct_cfg = format!(
            "model = \"deepseek-v4-flash-codex\"\nmodel_provider = \"uking_deepseek\"\n\
             [model_providers.uking_deepseek]\nbase_url = \"{XIAPAN_BASE}\"\nwire_api = \"responses\"\n"
        );
        assert!(!cfg_points_at_local_proxy(&direct_cfg), "直连形态不该被看门狗当成代理");

        // 还原形态（客户切回官方/没开过）：既没我们的标记也没本地端口
        assert!(!cfg_points_at_local_proxy(""), "空 config 不该被看门狗当成代理");
        assert!(!cfg_points_at_local_proxy("model = \"gpt-5.3-codex\"\n"), "别家配置不该被看门狗当成代理");
    }
}
