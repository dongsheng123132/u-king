//! 「AI 加速」板块后端 —— U-King AI Runtime（ukrt.exe）的薄壳。
//!
//! 进程级隔离：本模块只负责定位并调用独立的 ukrt.exe 子进程、收 stdout。
//! ukrt 崩溃/缺失只影响本板块（前端显示错误卡），不连坐其他板块。
//! 铁律：只暴露纯函数，不碰 AppHandle；#[tauri::command] 包装写在 lib.rs。
//! 依赖方向：本模块零依赖其他业务模块（纯 std）。

use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 内嵌的 ukrt.exe（构建时打进 u-king-mini.exe，运行时按需释放到 ~/.uking/ukrt/）。
/// 单一真相源：resources/ukrt.exe = ai-runtime 仓库 cli 的构建产物，发版时同步拷入。
/// 体积代价 ~430KB（静态 CRT——干净机免 VC++ Redist，0xC0000135 实测踩坑后的取舍）。
const UKRT_EMBEDDED: &[u8] = include_bytes!("../resources/ukrt.exe");

/// 保证 ukrt.exe 可用（找不到就释放内嵌版本）：
/// 1) UKRT_PATH 显式指定（开发调试）→ 尊重
/// 2) ~/.uking/ukrt/ukrt.exe 且大小与内嵌一致 → 用（不一致视为旧版/半包 → 走 4 重释放，升级 U-King 自动带新 ukrt）
/// 3) 当前 exe 同目录（U 盘旁挂）→ 尊重
/// 4) 释放内嵌字节：写 .tmp 再 rename（原子防半包；被占用则报错不硬写）
pub fn ensure_ukrt() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("UKRT_PATH") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Ok(pb);
        }
    }
    let home = std::env::var("USERPROFILE").map_err(|_| "读不到 USERPROFILE".to_string())?;
    let dir = PathBuf::from(&home).join(".uking").join("ukrt");
    let dst = dir.join("ukrt.exe");
    let same_size = std::fs::metadata(&dst).map(|m| m.len() == UKRT_EMBEDDED.len() as u64).unwrap_or(false);
    if same_size {
        return Ok(dst);
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(d) = me.parent() {
            let pb = d.join("ukrt.exe");
            if pb.exists() {
                return Ok(pb);
            }
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 ukrt 目录失败：{}", e))?;
    let tmp = dir.join("ukrt.exe.tmp");
    std::fs::write(&tmp, UKRT_EMBEDDED).map_err(|e| format!("释放 ukrt 失败：{}", e))?;
    std::fs::rename(&tmp, &dst).map_err(|e| format!("替换 ukrt 失败（可能被占用）：{}", e))?;
    Ok(dst)
}


/// doctor 体检，返回 ukrt 的 JSON 字符串（前端直接 JSON.parse）。
pub fn doctor_json() -> Result<String, String> {
    exec(&["doctor", "--json"], true)
}

/// fix / optimize / undo。动作白名单（对齐 term.rs::validate_cmd 思路，挡注入）。
/// 返回 ukrt 的人话输出（含 ✔/↷ 行与「61 → 76 (+15)」对比卡）。
pub fn run_action(action: &str) -> Result<String, String> {
    if !matches!(action, "fix" | "optimize" | "undo" | "defender") {
        return Err(format!("不允许的动作：{}", action));
    }
    // 优化大师会改本机环境（PATH / 注册表 / Defender 排除项），是**有副作用**的动作。
    // 出问题时「这台机器上到底跑过哪些优化、成没成」必须查得到，否则连回滚都无从谈起。
    crate::ulog::write("airuntime", &format!("执行优化动作 {action}"));
    let r = exec(&[action], false);
    match &r {
        Ok(_) => crate::ulog::write("airuntime", &format!("动作 {action} 完成")),
        Err(e) => crate::ulog::write("airuntime", &format!("动作 {action} 失败：{e}")),
    }
    r
}

/// 提权跑 `ukrt fix`（一次 UAC）——开 longpaths + devmode 这两个 HKLM 注册表项，是「到 100 分」
/// 绕不开的一步（非管理员改不了 HKLM）。用 powershell `Start-Process -Verb RunAs` 弹一次 UAC，提权进程
/// **复用 ukrt 自己的 journal**（同一 `%USERPROFILE%\.uking`），改动可回滚。提权进程在独立会话、stdout
/// 拿不到 → 跑完由前端重新 doctor 看新分。退出码：0 成功；1223=用户取消 UAC；其它=未完成。
#[cfg(windows)]
pub fn run_fix_elevated() -> Result<String, String> {
    let exe = ensure_ukrt()?;
    // 单引号内两两转义防注入；路径来自 ensure_ukrt（我们自己的产物），非用户输入。
    let ps = format!(
        "try {{ $p = Start-Process -FilePath '{}' -ArgumentList 'fix' -Verb RunAs -WindowStyle Hidden -PassThru -Wait -ErrorAction Stop; exit $p.ExitCode }} catch {{ exit 1223 }}",
        exe.to_string_lossy().replace('\'', "''")
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().map_err(|e| format!("提权启动失败：{}", e))?;
    match out.status.code().unwrap_or(-1) {
        0 => Ok("已以管理员完成硬修复：长路径 / 开发者模式已开启（改动已记入 journal，可在系统设置里关闭）".into()),
        1223 => Err("已取消管理员授权（UAC）。其它项仍可用「一键优化」处理；长路径 / 开发者模式需要管理员才能开。".into()),
        c => Err(format!("提权修复未完成（退出码 {}）。可右键 U-King → 以管理员身份运行后重试。", c)),
    }
}

#[cfg(not(windows))]
pub fn run_fix_elevated() -> Result<String, String> {
    Err("提权修复仅 Windows 需要；macOS 的一键优化无需管理员。".into())
}

/// 匿名上报 AI 战斗力分数 → 拿服务端真实百分位（「超过全国 X%」）。
/// 隐私铁律：**只发 score + version + os，绝不含设备指纹 / Key / 路径**。
/// 走系统 curl.exe（Win10+ 内置），2s 超时，失败静默返回 None（前端回退本地表）。
/// 端点走国内可达域 u-claw.org.cn（对齐 SNI 阻断结论）。
pub fn report_score(score: u32, version: &str) -> Option<i64> {
    let body = format!("{{\"score\":{},\"version\":\"{}\",\"os\":\"windows\"}}", score, version.replace('"', ""));
    let mut cmd = Command::new("curl.exe");
    cmd.args([
        "-s",
        "--max-time",
        "2",
        "-X",
        "POST",
        "-H",
        "Content-Type: application/json",
        "-d",
        &body,
        "https://u-claw.org.cn/uking/score",
    ]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    // 极简解析（不引 serde）：找 "percentile": <数字>
    let key = "\"percentile\"";
    let i = s.find(key)? + key.len();
    let rest = s[i..].trim_start_matches([':', ' ']);
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse::<i64>().ok()
}

/// 与 installer/term 完全一致的 PATH：把便携 Node/Git/Python、`%APPDATA%\npm`、`~/bin` 等前置，
/// 让 ukrt 的 `where codex/claude` 与 U-King 主程序检测**同一口径**。否则 ukrt 只吃 U-King 进程继承的
/// PATH——客户明明用装机向导装了 codex（落 `%APPDATA%\npm\codex.cmd` 或便携 node 目录），ukrt 却报「未安装」
/// （pc-… 实锤）。复用公共助手 `installer::search_paths`（term.rs 同款做法），不复制逻辑。
fn ukrt_path_env() -> String {
    use crate::installer::{portable_node_dir, search_paths};
    let sep = if cfg!(windows) { ";" } else { ":" };
    let dirs = search_paths(portable_node_dir().as_deref());
    let prefix = dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(sep);
    let old = std::env::var("PATH").unwrap_or_default();
    match (prefix.is_empty(), old.is_empty()) {
        (true, _) => old,
        (false, true) => prefix,
        (false, false) => format!("{prefix}{sep}{old}"),
    }
}

fn exec(args: &[&str], json_expected: bool) -> Result<String, String> {
    let exe = ensure_ukrt()?;
    let mut cmd = Command::new(&exe);
    cmd.args(args);
    cmd.env("PATH", ukrt_path_env());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().map_err(|e| format!("启动 ukrt 失败：{}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // 注意：doctor 分数 <100 时 ukrt 退出码为 1，是正常语义不算失败；只看 stdout。
    if stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "ukrt 无输出".into() } else { err });
    }
    if json_expected && !stdout.starts_with('{') {
        let head: String = stdout.chars().take(200).collect();
        return Err(format!("ukrt 返回非 JSON：{}", head));
    }
    Ok(stdout)
}
