//! 技术支持 —— 用户主动反馈 + **脱敏**诊断采集（独立可插拔）。
//!
//! ## 铁律
//! - **脱敏后才外发**：绝不上传完整 Key / Token / 邮箱 / 用户名路径（客户端本就当可被反编译，
//!   诊断更不能夹带隐私）。`desensitize` 统一收口。
//! - **纯函数**：`#[tauri::command]` 全在 `lib.rs` 转调，本模块不碰 `AppHandle`；发送走
//!   `report::report_feedback`（与自动上报同链路，服务端建 Issue）。
//!
//! ## 独立可插拔
//! 删本模块只动 `lib.rs`（去 `mod feedback` + 3 个 command + generate_handler）与前端
//! `Feedback.tsx` + 侧栏入口。依赖方向只向老的公共助手（`report` / `device` / `installer`）借力。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// 作者联系邮箱 hefangsheng@gmail.com 在前端 Feedback.tsx 硬编码展示 + 拼 mailto（非密钥）。

fn username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

// ———————————————— 脱敏 ————————————————

fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}
fn has_alpha(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}

/// 一个"词"是否像密钥/令牌 → 该整词替换为 `***`。
/// 命中任一：① `sk-`/`sk_` 前缀（虾盘云设备 Key、new-api token）；② 明显的长随机串
/// （≥28 字符、字母数字混合、无点号 —— 排除 URL/域名/版本号/模型 id 这类可读串）。
fn looks_like_secret(word: &str) -> bool {
    let w = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if w.is_empty() {
        return false;
    }
    let lower = w.to_ascii_lowercase();
    if lower.starts_with("sk-") || lower.starts_with("sk_") {
        return true;
    }
    // 纯 [A-Za-z0-9_-] 的长随机串（无 . / : 等），字母+数字都有。
    let only_key_chars = w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    only_key_chars && w.len() >= 28 && has_digit(w) && has_alpha(w)
}

/// 一个"词"是否是邮箱 → 替换为 `***@***`（保留是邮箱这个语义，抹掉具体地址）。
fn looks_like_email(word: &str) -> bool {
    let at = match word.find('@') {
        Some(i) => i,
        None => return false,
    };
    let (local, rest) = word.split_at(at);
    let domain = &rest[1..];
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.')
}

/// 把非 ASCII 字节转成 `%XX` 形式（只用于「日志里 URL 编码过的用户名」比对，不是通用编码器）。
fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn scrub_word(word: &str) -> String {
    if looks_like_email(word) {
        return "***@***".to_string();
    }
    if looks_like_secret(word) {
        return "***".to_string();
    }
    word.to_string()
}

/// 抹掉会泄露隐私/密钥的片段：
/// - `sk-…` / 长随机令牌 → `***`；邮箱 → `***@***`
/// - 本机用户名（及其在路径里的出现）→ `用户`（避免暴露真实姓名/账号）
///
/// 不引 regex（本项目纯 std），用"按分隔符切词 + 逐词判定"实现；分隔符保留原样，
/// 只替换词本身，故 URL / 版本号 / 模型 id 这类可读信息不受影响，诊断仍然有用。
pub fn desensitize(input: &str) -> String {
    // 先做整串替换：本机用户名（路径里最常泄露的真实身份）。
    let user = username();
    let pre = if user.len() >= 2 {
        // 明文 + **URL 编码**两种形态都要抹。中文用户名在 OpenClaw 日志里是以 file:// URL
        // 出现的（`C:/Users/%E6%B3%A2/AppData/...`），只替换明文会漏 —— issue #226 的正文
        // 里就实实在在泄露了这一串百分号编码的中文用户名。
        let mut s = input.replace(&user, "用户");
        for enc in [percent_encode(&user), percent_encode(&user).to_lowercase()] {
            if enc.len() >= 3 && enc != user {
                s = s.replace(&enc, "用户");
            }
        }
        s
    } else {
        input.to_string()
    };

    // 词字符：字母/数字/ _ - . @ + —— 把邮箱/密钥当一个词整体判定；斜杠/冒号等作分隔符保留，
    // 让路径与 URL 结构保持可读（只有其中的密钥片段会被单独命中替换）。
    let mut out = String::with_capacity(pre.len());
    let mut cur = String::new();
    let is_word_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@' | '+');
    for ch in pre.chars() {
        if is_word_char(ch) {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                out.push_str(&scrub_word(&cur));
                cur.clear();
            }
            out.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push_str(&scrub_word(&cur));
    }
    out
}

// ———————————————— 日志 ————————————————

/// OpenClaw 网关日志目录（Windows：`%LOCALAPPDATA%\Temp\openclaw`，兜底 `TEMP\openclaw`）。
/// 日志文件名 → 中文段标题。诊断正文是给人读的（作者 / 客户都会看到），
/// 段头写 `draw` 不如写「AI 作图」一眼能定位。没登记的模块原样显示，不隐藏、不报错。
fn friendly_log_name(stem: &str) -> String {
    match stem {
        "install" => "U-King 装机",
        "draw" => "AI 作图",
        "video" => "AI 视频",
        "clawx" => "ClawX 托管配置",
        "geo" => "网站 GEO 体检",
        "toolbox" => "厨具工具箱",
        "ollama" => "本地大模型 Ollama",
        "airuntime" => "AI 优化大师",
        // 以下是「有副作用」的动作 —— 删 / 改用户东西，出事时最需要能对质的就是它们
        "uninstall" => "一键卸载",
        "cleanup" => "逐项清理",
        "backup" => "备份/还原",
        "install-local" => "装到本地",
        "miniapp" => "小程序",
        "rtk" => "Token 压缩机",
        "mcp" => "MCP 连接器",
        "skillpack" => "AI 技能包",
        "uuswitch" => "uu-switch 导入",
        "launch" => "启动 GUI 应用",
        other => other,
    }
    .to_string()
}

fn log_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(la).join("Temp").join("openclaw");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let p = std::env::temp_dir().join("openclaw");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// 目录里最近修改的 `openclaw-*.log`。
fn latest_openclaw_log(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.starts_with("openclaw-") && name.ends_with(".log") {
            if let Ok(m) = e.metadata() {
                let t = m.modified().unwrap_or(std::time::UNIX_EPOCH);
                if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                    best = Some((t, e.path()));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// 读文件尾部最多 `n` 字节（大日志不整读；UTF-8 边界用 lossy 兜）。
fn tail_bytes(path: &Path, n: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(n);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

// ———————————————— 诊断采集 ————————————————

/// 采集一份**已脱敏**的诊断文本（版本 / 系统 / 设备 Key 前缀 / 工具安装状态 / 日志尾部）。
/// 供反馈页展示、复制、随反馈一起上报。全程只读，不改任何东西。
pub fn collect_diagnostics() -> String {
    let mut s = String::new();
    s.push_str("== U-King 诊断（已脱敏）==\n");
    s.push_str(&format!("版本: {}\n", env!("CARGO_PKG_VERSION")));
    s.push_str(&format!("系统: {} / {}\n", std::env::consts::OS, std::env::consts::ARCH));
    s.push_str(&format!("设备Key(前缀): {}\n", crate::device::get_device_key_cached_prefix()));

    // 已装工具（只读探测，与「已装」徽标同口径）。
    let claude = crate::installer::tool_installed("claude");
    let codex = crate::installer::tool_installed("codex");
    let hermes = crate::installer::tool_installed("hermes");
    #[cfg(windows)]
    let clawx = crate::providers::clawx_app_installed();
    #[cfg(not(windows))]
    let clawx = false;
    s.push_str(&format!(
        "已装: claude={} codex={} hermes={} clawx={}\n",
        claude, codex, hermes, clawx
    ));
    // ★ AI CLI 的**版本号**，不只是装没装。
    //
    // 这几个工具自己会静默自动升级，而升级会改行为 —— 2026-08-10 就栽过一次：
    // Claude Code 某个 2.1.2xx 新增「未知模型窗口强制」，全体客户几天内先后中招，
    // 客户描述是「任务老是中断」。当时诊断正文里只有 `claude=true`，
    // 「大家的 claude 是不是同时跳版本了」这个问题**在我们的数据里问不出来**，
    // 只能人肉去比对二进制。多这一行，下次同类事件一眼可见。
    {
        // `current()` 走 24h 缓存 —— 反馈页不该为这几行等上几秒（hermes --version 实测 2.3s）。
        let fp = crate::envfp::current();
        let or_dash = |s: &str| if s.trim().is_empty() { "-".to_string() } else { s.to_string() };
        s.push_str(&format!(
            "AI CLI 版本: claude={} codex={} openclaw={} hermes={}\n",
            or_dash(&fp.claude_ver),
            or_dash(&fp.codex_ver),
            or_dash(&fp.openclaw_ver),
            or_dash(&fp.hermes_ver),
        ));
    }

    // ★ 工具栈账（本地 metrics 聚合）：配得上 / 能用 / 有人用。
    //
    // 挂在诊断正文里是**唯一的数据回收路径**（2026-08-04 定）：客户主动点反馈时才带上，
    // 走的是现成的脱敏链路，零新通道、天然有同意，不动「默认只写本地不上传」那条红线。
    // 代价要说在明处：只有报障的客户会反馈，**样本天然有偏**（不好用的才来说话），
    // 所以这段只能当线索，不能当「全体客户的使用率」。
    let ts = crate::metrics::report(30, serde_json::Value::Null).toolstack;
    if !ts.is_empty() {
        s.push_str("-- 工具栈（近30天，本地记录）--\n");
        for r in &ts {
            let probe = match r.probe_ok {
                Some(true) => "实测可用",
                Some(false) => "实测不可用",
                None => "未实测",
            };
            s.push_str(&format!(
                "  {}: 用了{}次/{}天 配置成功{}次 失败{}次 {}\n",
                r.tool, r.used, r.used_days, r.apply_ok, r.apply_fail, probe
            ));
        }
    }

    // AI 进程健康取证 —— 反馈里第二高频的是「claude 老是断 / 突然就没了」。有没有崩溃痕迹
    // 是决定性判据：强杀不留 WER 和转储，所以「查无痕迹」直接把方向从「进程崩了」翻到
    // 「被别的程序结束了」，省掉一整轮来回问。
    let ai = crate::installer::inspect_ai_process_health();
    s.push_str(&format!("AI进程取证: {} (近{}h)\n", ai.verdict, ai.window_hours));
    if !ai.security_products_running.is_empty() {
        s.push_str(&format!("在跑的安全软件: {}\n", ai.security_products_running.join("、")));
    }
    for e in ai.crash_evidence.iter().take(5) {
        s.push_str(&format!("  崩溃痕迹: [{}] {} ({}h前)\n", e.kind, e.name, e.age_hours));
    }

    // 代理设置 —— 客户机常年开着 clash 式梯子，而它是**方向最容易指反**的一类故障：
    // issue #309「codex 报 502」看着像我们的省钱路由挂了，可代理日志里零错误 ——
    // 请求压根没到过它，是被梯子截走后回的 502。之前这条一个字都没采，只能反复问
    // 客户「你开梯子了吗」。复用 `runtime.network.inspect` 那份探测（脱敏也在里面），
    // 不另写一份：同一事实存在两份就会漂移两份。
    let net = crate::installer::inspect_runtime_network();
    let mut proxies: Vec<String> =
        net.environment_proxies.iter().map(|p| format!("{}={}", p.name, p.endpoint)).collect();
    if let Some(sys) = &net.system_proxy {
        proxies.push(format!("系统代理={sys}"));
    }
    if proxies.is_empty() {
        s.push_str("代理: 未设\n");
    } else {
        s.push_str(&format!("代理: {}\n", proxies.join(" ")));
        // 回环有没有被排除 —— 这正是「连本机端口却被梯子截走」的判据，直接给结论别让人自己推
        let no = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")).unwrap_or_default();
        let bypassed = no.contains("127.0.0.1") || no.contains("localhost");
        s.push_str(&format!(
            "  回环绕过代理: {}\n",
            if bypassed { "是" } else { "否 —— 连本机端口（Codex 省钱路由 15722 / gateway 18789）会被代理截走" }
        ));
    }
    for w in net.warnings.iter().take(3) {
        s.push_str(&format!("  代理告警: {w}\n"));
    }

    // U-King 自己的**装机日志**尾部 —— 反馈里最高频的就是「XX 装不上」，而装机日志此前只在
    // 前端气泡里，采不到（issue #226 的客户只能手工把日志复制粘贴进反馈框）。放在 OpenClaw
    // 日志前面：装机失败比网关运行期报错更常见，尾部预算优先给它。
    if let Some(tail) = crate::installer::install_log_tail(6144) {
        s.push_str("\n-- U-King 装机日志尾部 --\n");
        s.push_str(tail.trim());
        s.push('\n');
    }

    // Codex 省钱路由的本地代理日志尾部。这条链路此前是**全盲**的：代理是独立 Node 子进程，
    // stdout/stderr 都被定向到 null（不能弹黑窗），报错只出现在 Codex 界面上，我们零线索 ——
    // 客户只会说「codex 又坏了」。日志里只有角色序列和上游状态码，没有任何对话正文。
    let proxy_log = crate::codex_proxy::log_path();
    if proxy_log.exists() {
        if let Some(tail) = tail_bytes(&proxy_log, 3072) {
            if !tail.trim().is_empty() {
                s.push_str("\n-- Codex 省钱路由代理日志尾部 --\n");
                s.push_str(tail.trim());
                s.push('\n');
            }
        }
    }

    // U-King 各功能模块的运行日志（`~/.uking/logs/*.log`：作图 / 视频 / T-King 成片 /
    // ClawX 托管配置 / GEO / 工具箱 / Ollama / 优化大师…）。
    //
    // **按扫目录取，不按名单取** —— 这是刻意的：只要哪个模块调了一次 `ulog::write`，
    // 它的日志就自动出现在诊断里，不必回头改这里。否则迟早出现「功能加了、日志也写了，
    // 但忘了接进诊断，客户报错照样查不到」——那正是 0.9.78 之前 T-King 的处境。
    // install 已在上面用更大预算单独取过，跳过免得重复占正文额度。
    for (module, tail) in crate::ulog::all_tails(2048) {
        if module == "install" {
            continue;
        }
        s.push_str(&format!("\n-- {} 日志尾部 --\n", friendly_log_name(&module)));
        s.push_str(tail.trim());
        s.push('\n');
    }

    // OpenClaw 网关日志尾部（最常见的运行期报错都在这）。
    if let Some(dir) = log_dir() {
        if let Some(log) = latest_openclaw_log(&dir) {
            if let Some(tail) = tail_bytes(&log, 4096) {
                s.push_str("\n-- OpenClaw 日志尾部 --\n");
                s.push_str(tail.trim());
                s.push('\n');
            }
        }
    }

    desensitize(&s)
}

// ———————————————— 截图（粘贴的图片） ————————————————

/// 用户粘贴的截图落盘目录 `~/.uking/feedback/`。
///
/// 为什么不直接把图发出去：上报链路终点是 GitHub Issue，body 有 8000 字上限，base64 图片
/// 塞不下。所以图**留在本机**，反馈正文里注明「附了 N 张截图 + 路径」，页面给「打开截图
/// 文件夹」让用户拖进邮件发给作者 —— 零服务端改动，今天就能用。
pub fn shots_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".uking").join("feedback")
}

/// 保存一张粘贴进来的截图。`data_url` 形如 `data:image/png;base64,xxxx`。
/// 返回落盘的绝对路径（前端展示 + 提交时随正文带上）。
pub fn save_shot(data_url: &str) -> Result<String, String> {
    let (mime, b64) = data_url
        .strip_prefix("data:")
        .and_then(|s| s.split_once(";base64,"))
        .ok_or_else(|| "不是有效的图片数据".to_string())?;
    if !mime.starts_with("image/") {
        return Err("只支持粘贴图片".into());
    }
    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    };
    let bytes = b64_decode(b64)?;
    // 10MB 上限：粘贴的截图正常几百 KB，超了多半是误操作贴了大图。
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("图片太大（超过 10MB），请压缩后再试".into());
    }
    let dir = shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("建截图目录失败：{e}"))?;
    // 文件名用 epoch 毫秒 + 序号，避免同一毫秒连续粘贴覆盖。
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut path = dir.join(format!("shot-{ms}.{ext}"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("shot-{ms}-{n}.{ext}"));
        n += 1;
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("保存截图失败：{e}"))?;
    Ok(path.display().to_string())
}

/// 极简 base64 解码（纯 std，不引 crate —— 与本项目「体积优先」一致）。
fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = val(c).ok_or_else(|| "图片数据损坏（非法 base64）".to_string())?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if out.is_empty() {
        return Err("图片数据为空".into());
    }
    Ok(out)
}

/// 打开截图文件夹（让用户把图拖进邮件）。
pub fn open_shots_dir() -> Result<(), String> {
    let dir = shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("建截图目录失败：{e}"))?;
    open_dir(&dir)
}

/// 提交用户反馈（同步）：把用户文字（脱敏）+（可选）诊断，走 `report::report_feedback` 上报。
/// 返回人话结果给前端。`message` 空则拒绝（避免空反馈）。
/// `shots`：截图在客户机的落盘路径（永远随正文带上，只是文字）。
/// `shot_data`：**客户明确勾选「同意上传截图」时**前端压好的 JPEG base64；不勾选就是空数组。
/// 图片上传是可选增强 —— 传失败绝不能影响反馈本身，收到客户的话永远比收到图重要。
pub fn submit_feedback(
    message: &str,
    include_diagnostics: bool,
    shots: &[String],
    shot_data: &[String],
) -> Result<String, String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("请先写一句你遇到的问题或建议".into());
    }
    let clean_msg = desensitize(msg);
    let summary = clean_msg.lines().next().unwrap_or("用户反馈").to_string();

    let mut detail = String::new();
    detail.push_str("【用户反馈】\n");
    detail.push_str(&clean_msg);
    detail.push('\n');
    // 截图留在客户机（issue body 塞不下图），这里只记数量+文件名，作者需要时可让客户
    // 用「打开截图文件夹」发过来。路径过脱敏（中文用户名等）。
    if !shots.is_empty() {
        detail.push_str(&format!(
            "\n【用户附了 {} 张截图（在客户机本地，需要时请向用户索取）】\n",
            shots.len()
        ));
        for s in shots {
            detail.push_str(&format!("- {}\n", desensitize(s)));
        }
    }
    if include_diagnostics {
        detail.push('\n');
        detail.push_str(&collect_diagnostics());
    }

    crate::report::report_feedback(&summary, &detail, shot_data)
        .map(|_| "反馈已提交，谢谢！我们会尽快查看。".to_string())
        .map_err(|e| format!("提交失败（网络不通？可改用邮件反馈）：{e}"))
}

/// 打开日志所在文件夹（让用户手动把日志附到邮件里）。找不到则打开临时目录兜底。
pub fn open_log_dir() -> Result<(), String> {
    let dir = log_dir().unwrap_or_else(std::env::temp_dir);
    open_dir(&dir)
}

/// 用系统文件管理器打开目录（日志 / 截图共用）。
fn open_dir(dir: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(&dir)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("打开文件夹失败：{e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败：{e}"))?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开文件夹失败：{e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets_keeps_signal() {
        // sk- 设备 Key / new-api token → 抹掉
        let out = desensitize("token=sk-abc123 failed");
        assert!(!out.contains("sk-abc123"), "sk- key should be redacted: {out}");
        assert!(out.contains("***"), "should contain redaction marker: {out}");
        assert!(out.contains("failed"), "普通词保留: {out}");

        // 长随机串（无点号）→ 抹掉
        let long = "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6"; // 32 位混合
        let out2 = desensitize(long);
        assert_eq!(out2, "***", "long random token redacted: {out2}");

        // 邮箱 → ***@***
        let out3 = desensitize("联系 zhang.san@example.com 谢谢");
        assert!(out3.contains("***@***"), "email redacted: {out3}");
        assert!(!out3.contains("example.com"), "email domain gone: {out3}");
    }

    #[test]
    fn redacts_percent_encoded_username() {
        // #226 实锤：中文用户名在 OpenClaw 日志里以 file:// URL 出现（`C:/Users/%E6%B3%A2/...`），
        // 只替换明文会漏，反馈正文里就这么泄露出去了。
        assert_eq!(percent_encode("波"), "%E6%B3%A2");
        std::env::set_var("USERNAME", "波");
        let out = desensitize("file:///C:/Users/%E6%B3%A2/AppData/Local/x.log 和 C:\\Users\\波\\a");
        assert!(!out.contains("%E6%B3%A2"), "URL 编码的用户名要抹掉: {out}");
        assert!(!out.contains('波'), "明文用户名要抹掉: {out}");
        assert!(out.contains("AppData"), "路径其余部分保留可读: {out}");
    }

    #[test]
    fn b64_decode_roundtrip() {
        // "hi!" → aGkh（无填充）；"hi" → aGk=（带填充）
        assert_eq!(b64_decode("aGkh").unwrap(), b"hi!");
        assert_eq!(b64_decode("aGk=").unwrap(), b"hi");
        assert!(b64_decode("").is_err(), "空数据要报错");
        assert!(b64_decode("!!!").is_err(), "非法字符要报错");
    }

    #[test]
    fn save_shot_rejects_non_image() {
        assert!(save_shot("data:text/plain;base64,aGk=").is_err(), "只收图片");
        assert!(save_shot("不是 data url").is_err(), "格式不对要报错");
    }

    #[test]
    fn keeps_readable_diagnostics() {
        // 版本号 / 模型 id / URL / OS 名不该被误伤
        for s in ["0.9.67", "deepseek-v4-pro", "gpt-5.3-codex", "windows", "x86_64"] {
            assert_eq!(desensitize(s), s, "readable token must survive: {s}");
        }
        // URL 结构保留（其中没有密钥）
        let url = "https://api.u-claw.org.cn/v1";
        assert!(desensitize(url).contains("u-claw.org.cn"), "url host kept");
    }
}
