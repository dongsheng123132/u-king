//! Hermes Agent 官方源码安装器接线（Windows / macOS），钉死 GitHub release commit。
//!
//! # 为什么不再走 pip（2026-10-19 拍板，废弃的 tarball 路线见 git 历史）
//!
//! 官方 PyPI 的 `hermes-agent` latest 停在 0.19.0（旧），而官方源码最新 release 是
//! `v0.22.2`。直接 pip tarball 的路走不通：v0.22.2 的 pyproject 核心依赖全部带
//! `python_version >= '3.14'` marker（`[tool.uv] environments` 也是 3.14+），现有便携
//! Python 3.12 上 pip 能解析成功但核心依赖一个都不装、import 直接挂；且 tarball 无
//! `.git` / install-stamp，`hermes --version` 会是 `unknown`。所以改走**官方源码安装器
//! （`scripts/install.ps1` / `install.sh`）**，但把内容钉死到某个 release commit——
//! 不能直接 `iex (irm https://hermes-agent.nousresearch.com/install.ps1)`（浮动域名、
//! 未钉版本、会 git stash/reset 用户 checkout 的旧形态），也不能 `-IncludeDesktop`。
//!
//! # 本模块只管「拿到官方脚本并钉 commit 执行 + 验证」，不碰配置写盘
//!
//! 配置真相源仍是 Hermes 自己解析的 `HERMES_HOME`（官方脚本默认：Windows 为
//! `%LOCALAPPDATA%\hermes`、非 Windows 为 `~/.hermes`）——本模块**不写任何模型配置、
//! 不读任何 Key**。虾盘云默认驱动的写盘在 `providers.rs`，与本模块无关。
//!
//! # 阴影问题（「报告是对的、世界是坏的」）
//!
//! 旧 pip 的 `hermes.exe` 在便携 Python Scripts 目录。若不处理，U-King 的 `search_paths`
//! / 终端 PATH / CLI 转发器都会先命中它 → **内部跑 0.19、外部跑 0.22** 的分裂。对策分三处：
//! - `search_paths` 里官方 bin（`%LOCALAPPDATA%\hermes\bin`）优先于便携 Python Scripts
//!   （改动在 `installer.rs`，本模块只提供 `official_bin_dir()`）；
//! - `ensure_cli_command_guard` 的 sources 里官方 bin 排在前面（改动在 `installer.rs`）；
//! - `where_report()` 如实报 `source=official-bin / legacy-pip`、实际路径与版本。
//!
//! # 平台范围
//!
//! 只有 **Windows** 接了安装/升级（`installer.rs` 的 `install_tool_with_intent` 里
//! `cfg(windows)` 分派）。macOS 仍走通用 manifest 的 pip 路径（停在 PyPI 的 0.19.0），
//! 本模块在非 Windows 上 `install` / `upgrade` 直接返回「未接入」，不假装升级成功。
//! 已核对 v0.22.2 的 `scripts/install.sh` 支持 `--non-interactive --commit <sha>
//! --skip-browser --skip-computer-use`，Mac 后续要接时同样的钉 commit 做法可行，
//! 但它的 launcher 落在 `~/.local/bin` 而不是 `$HERMES_HOME/bin`，落点逻辑需另做。
//!
//! # 纯 std，无新依赖
//!
//! JSON 解析复用 `serde_json`，SHA-256 复用公共层 `installer::sha256_hex_bytes`
//! （不直连 `device`，避免新增功能模块耦合边），HTTP 走系统 curl，进程执行走 std。
//! 不引 HTTP 客户端 crate。

// 非 Windows 上只有纯函数与诊断在用，安装路径整体不编译进调用链。
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};

/// 错误消息前缀：带它的失败是「设计内拦截」（沙箱 / Hermes 还在跑），
/// 调用方应按 `precheck_blocked` 处理、不上报 issue。
pub const BLOCKED_MARK: &str = "【预检拦截】";

/// `err` 是否是设计内拦截（见 [`BLOCKED_MARK`]）。
pub fn is_precheck_block(err: &str) -> bool {
    err.starts_with(BLOCKED_MARK)
}

/// 安装 / 升级的公共结果（与 `dshdesk::InstallOutcome` 同形，让 `installer.rs` 的分派
/// 能用同一套 `match` 形状接住）。不需要 `Serialize`：`installer.rs` 会把它拆进
/// 可序列化的 `InstallToolResult`。
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub message: String,
    pub version: Option<String>,
}

// ============================================================
// 官方落点解析（唯一真相源：官方 install.ps1 / install.sh 的默认值）
// ============================================================

/// 官方安装器会把源码 checkout 到这里（Windows 默认；`HERMES_HOME` 未设时）。
pub fn official_install_dir() -> Option<PathBuf> {
    official_home_dir().map(|h| h.join("hermes-agent"))
}

/// 官方安装器发布 `hermes` launcher 的目录。这是 `search_paths` / CLI 转发器必须
/// 优先于便携 Python Scripts 的那一份（阴影问题）。
///
/// - Windows（install.ps1）：`%LOCALAPPDATA%\hermes\bin`（= `$HermesHome\bin`）。
/// - 非 Windows（install.sh）：launcher 发布到 `~/.local/bin`（另一份在 checkout 内的
///   `.hermes/bin`，落点随 checkout 位置漂，不当稳定入口）。**只在 launcher 确实
///   在那儿时才返回**——没装官方版就返回 None，让 `where_report` 如实落到
///   `legacy-pip` / `none`，不编一个不存在的目录出来。
pub fn official_bin_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        official_home_dir().map(|h| h.join("bin"))
    }
    #[cfg(not(windows))]
    {
        let local_bin = PathBuf::from(std::env::var("HOME").ok()?).join(".local").join("bin");
        if launcher_in(&local_bin).is_some() {
            Some(local_bin)
        } else {
            None
        }
    }
}

/// 官方安装器的 HermesHome：`HERMES_HOME` 环境变量优先，其次平台默认。
///
/// 🔴 口径与官方脚本逐字对齐（install.ps1 `param()` 与 install.sh 第 27 行），
/// **不是** `installer::hermes_config_dir()` 的完整解析（那个会去真跑一次 Hermes 问它自己，
/// 代价 2 秒多，诊断与安装时没必要）。两者在 `HERMES_HOME` 未设时结果一致：
/// Windows `%LOCALAPPDATA%\hermes`，其余 `~/.hermes`。
pub fn official_home_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("HERMES_HOME") {
        let v = v.trim();
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    platform_default_home()
}

/// 平台默认 HermesHome，口径抄官方脚本（不碰 `%LOCALAPPDATA%` 之外的路径）。
fn platform_default_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA").ok().map(PathBuf::from);
        if let Some(local) = local.filter(|p| !p.as_os_str().is_empty()) {
            return Some(local.join("hermes"));
        }
        // LOCALAPPDATA 空：回落 `%USERPROFILE%\AppData\Local\hermes`（与官方脚本的
        // `$env:LOCALAPPDATA` 为空时 PowerShell 会解析到空、installer 侧回退口径一致）。
        std::env::var("USERPROFILE").ok().map(|u| {
            Path::new(&u).join("AppData").join("Local").join("hermes")
        })
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(|h| Path::new(&h).join(".hermes"))
    }
}

/// 官方 bin 目录里实际存在的 hermes launcher（Windows 优先 `.exe`，其次 `.cmd`；非 Windows 裸名）。
pub fn official_launcher() -> Option<PathBuf> {
    let bin = official_bin_dir()?;
    launcher_in(&bin)
}

fn launcher_in(bin: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        for ext in ["exe", "cmd", "bat"] {
            let p = bin.join(format!("hermes.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let p = bin.join("hermes");
        p.is_file().then_some(p)
    }
}

/// 旧 pip 落点：便携 Python Scripts 里的 hermes launcher（存在才返回）。
/// `where_report` 用它区分「官方 bin 里没有、但旧 pip 那份还在」的阴影形态。
pub fn legacy_pip_launcher() -> Option<PathBuf> {
    let scripts = crate::installer::portable_python_scripts_dir()?;
    #[cfg(windows)]
    for ext in ["exe", "cmd", "bat"] {
        let p = scripts.join(format!("hermes.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(not(windows))]
    {
        let p = scripts.join("hermes");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

// ============================================================
// GitHub release 解析（纯函数，可测）
// ============================================================

/// GitHub API：latest published release（跳 draft / prerelease）。
const RELEASE_API: &str = "https://api.github.com/repos/NousResearch/hermes-agent/releases/latest";

/// 只认这个仓库，防止未来 URL 被篡改时把下载引到别的 repo / 域名。
const REPO_FULL_NAME: &str = "NousResearch/hermes-agent";

/// 从 releases/latest 响应里解析出的最小发行信息。`commit` 可能只是 `target_commitish`
/// 的暂定值，最终值由 [`deref_tag_commit`] 对 annotated tag 解引用后覆盖。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesRelease {
    pub tag: String,
    pub commit: String,
    pub published_at: String,
}

/// 解析 releases/latest JSON。纯函数：不联网，坏输入一律 Err（不给安装留下一丝注入面）。
pub fn parse_latest_release(body: &str) -> Result<HermesRelease, String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("GitHub releases/latest 不是合法 JSON：{e}"))?;

    // 只认 NousResearch/hermes-agent：html_url 形如
    // https://github.com/NousResearch/hermes-agent/releases/tag/v0.22.2
    let html = v
        .get("html_url")
        .and_then(|u| u.as_str())
        .ok_or("GitHub 响应缺少 html_url")?;
    let expected_prefix = format!("https://github.com/{REPO_FULL_NAME}/");
    if !html.starts_with(&expected_prefix) {
        return Err(format!("GitHub 响应不是 {REPO_FULL_NAME} 的 release（html_url={html}）"));
    }

    if v.get("draft").and_then(|d| d.as_bool()).unwrap_or(true) {
        return Err("latest release 是 draft，跳过（等正式发布）".into());
    }
    if v.get("prerelease").and_then(|d| d.as_bool()).unwrap_or(true) {
        return Err("latest release 是 prerelease，跳过（不装预发布）".into());
    }

    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or("GitHub 响应缺少 tag_name")?
        .trim()
        .to_string();
    validate_tag(&tag)?;

    // target_commitish 在 annotated tag 场景可能指向 tag 对象而非 commit，
    // 所以这里只是暂定值，调用方还要走 resolve_commit 解引用。
    let commit = v
        .get("target_commitish")
        .and_then(|c| c.as_str())
        .ok_or("GitHub 响应缺少 target_commitish")?
        .trim()
        .to_string();
    if !commit.chars().all(|c| c.is_ascii_hexdigit()) || commit.len() < 7 {
        return Err(format!("target_commitish 不是合法 commit sha：{commit}"));
    }

    let published_at = v
        .get("published_at")
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();

    Ok(HermesRelease { tag, commit, published_at })
}

/// tag 必须是 `v` 开头的 semver 形状（`v0.22.2`、`v0.22.2-rc.1`），
/// 且只含安全字符。这个函数是「下载 URL 拼接」和「-Commit 参数」的最后一道闸。
///
/// 🔴 **校验原串，不 trim 后接受**：调用方手里那个字符串才是要进 URL / -Commit 的东西，
/// 末尾带 `\n`、中间带空格都必须拒——静默 trim 会造出「校验用干净串、拼接用原串」的
/// 错位，校验形同虚设（见 `validate_tag_rejects_injection_shapes`）。
pub fn validate_tag(tag: &str) -> Result<(), String> {
    if tag.is_empty() {
        return Err("release tag 为空".into());
    }
    // 严格字符白名单：字母数字 + `. - _`。拒绝 `/` `\` `;` `&` `%` 空格换行等一切可注入形状。
    if !tag.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)) {
        return Err(format!("release tag 含非法字符（已拦截）：{tag:?}"));
    }
    if tag.contains("..") {
        return Err(format!("release tag 含路径穿越（已拦截）：{tag:?}"));
    }
    let Some(num) = tag.strip_prefix('v') else {
        return Err(format!("release tag 不是 v 开头的 semver（已拦截）：{tag:?}"));
    };
    let (core, _pre) = num.split_once('-').unwrap_or((num, ""));
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!("release tag 不是合法 semver（已拦截）：{tag:?}"));
    }
    Ok(())
}

/// 安装脚本下载 URL：raw.githubusercontent.com 上钉死 tag 的脚本。
/// 绝不用 hermes-agent.nousresearch.com 浮动域名（那里不钉版本、会随主分支漂移）。
pub fn install_script_url(tag: &str) -> Result<String, String> {
    validate_tag(tag)?;
    Ok(format!(
        "https://raw.githubusercontent.com/{REPO_FULL_NAME}/{tag}/scripts/install.ps1"
    ))
}

/// 对 annotated tag 解引用拿到真 commit sha：
/// `git/ref/tags/<tag>` → 若 `object.type == "tag"` 再取 `git/tags/<sha>` → `object.sha`。
/// 纯判断部分拆在 [`deref_tag_commit`]，网络在调用方。
pub fn deref_tag_commit(tag_ref_json: &str, tag_obj_json: &str) -> Result<String, String> {
    let tag_ref: serde_json::Value = serde_json::from_str(tag_ref_json)
        .map_err(|e| format!("git/ref/tags 不是合法 JSON：{e}"))?;
    let obj = tag_ref
        .get("object")
        .ok_or("git/ref/tags 响应缺少 object")?;
    let obj_sha = obj
        .get("sha")
        .and_then(|s| s.as_str())
        .ok_or("git/ref/tags 响应缺少 object.sha")?;
    let obj_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");

    let commit = if obj_type == "tag" {
        // annotated tag：再取一层 object.sha。注意收尾 to_string()——两个分支的借用源
        // 生命周期不同（tag_obj 是本块局部、obj_sha 活得更久），不落成 owned 会 E0597。
        let tag_obj: serde_json::Value = serde_json::from_str(tag_obj_json)
            .map_err(|e| format!("git/tags 不是合法 JSON：{e}"))?;
        tag_obj
            .get("object")
            .and_then(|o| o.get("sha"))
            .and_then(|s| s.as_str())
            .ok_or("git/tags 响应缺少 object.sha")?
            .to_string()
    } else if obj_type == "commit" {
        obj_sha.to_string()
    } else {
        return Err(format!("git/ref/tags 的 object.type 无法识别：{obj_type}"));
    };
    if !commit.chars().all(|c| c.is_ascii_hexdigit()) || commit.len() < 7 {
        return Err(format!("解引用得到的 commit sha 非法：{commit}"));
    }
    Ok(commit)
}

// ============================================================
// 输出门禁（纯函数，可测）
// ============================================================

/// 截取字符串尾部（错误消息回显用）。与 `installer` 内部的 `tail` 同形，
/// 但本模块不该越界调用它的私有函数；错误路径上只取尾部、不 panic。
fn tail(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.trim().to_string();
    }
    let mut start = s.len() - n;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", s[start..].trim())
}

/// 「import 挂了」的失败特征（不区分大小写）：v0.22.2 核心依赖要 Python 3.14，现有便携
/// Python 3.12 上 pip 能解析成功但一个依赖都不装 → 启动即 `ModuleNotFoundError`。
/// 出现在输出的**任何一行**都是假成功，整篇都要拦。
const IMPORT_ERROR_MARKERS: &[&str] = &["modulenotfounderror", "importerror"];

/// 版本本身解析不出来的特征：tarball / 无 install-stamp 的源码树，`hermes --version`
/// 的**首行**就是 `unknown`。只看首行——健康输出的后续行（`Install method:`、
/// `Update status:` 之类）本来就会带这个词，整篇找会把好版本误杀。
const UNKNOWN_VERSION: &str = "unknown";

/// 解析 `hermes --version` 输出：退出码 0 之外的第二道门。
///
/// - 空输出 / 只有空白 → 拒（命令能启动但没吐出任何版本）。
/// - 首行是 `unknown` → 拒（源码树没有 install-stamp 的假成功，见模块头）。
/// - 含 `ModuleNotFoundError` / `ImportError` → 拒（依赖没装全）。
/// - 否则返回第一行原文（版本命令只报 commit sha 也接受并原样显示）。
pub fn parse_version_output(out: &str) -> Result<String, String> {
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Err("hermes --version 输出为空".into());
    }
    let first = trimmed.lines().next().unwrap_or(trimmed).trim();
    if first.to_lowercase().contains(UNKNOWN_VERSION) {
        return Err(format!(
            "hermes --version 首行是 {UNKNOWN_VERSION}（源码树没有 install-stamp / 版本没解析出来）：{first}"
        ));
    }
    let lower = trimmed.to_lowercase();
    if let Some(m) = IMPORT_ERROR_MARKERS.iter().find(|m| lower.contains(**m)) {
        return Err(format!(
            "hermes --version 输出含「{m}」：{}",
            tail(trimmed, 300)
        ));
    }
    Ok(first.to_string())
}

// ============================================================
// 联网 + 安装流程
// ============================================================

/// 拉取并解析最新发行信息，再对 annotated tag 解引用拿到真 commit sha。
/// `deref_tag_commit` 失败时回落到 `target_commitish`（已经在 parse 里校验过是 hex）。
fn latest_release(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<HermesRelease, String> {
    let body = crate::installer::curl(&["-sL", "-m", "30", "-H", "Accept: application/vnd.github+json", RELEASE_API])
        .map_err(|e| format!("查 GitHub 最新 release 失败：{e}"))?;
    let mut rel = parse_latest_release(&body)?;
    // 解引用 annotated tag：不信任 target_commitish 一定指向 commit。
    let tag_ref_url = format!(
        "https://api.github.com/repos/{REPO_FULL_NAME}/git/ref/tags/{}",
        rel.tag
    );
    match crate::installer::curl(&["-sL", "-m", "30", &tag_ref_url]) {
        Ok(ref_json) => {
            let obj_sha = serde_json::from_str::<serde_json::Value>(&ref_json)
                .ok()
                .and_then(|v| v.get("object")?.get("sha")?.as_str().map(str::to_string));
            if let Some(sha) = obj_sha {
                let tag_obj_url = format!("https://api.github.com/repos/{REPO_FULL_NAME}/git/tags/{sha}");
                let obj_json = crate::installer::curl(&["-sL", "-m", "30", &tag_obj_url])
                    .unwrap_or_default();
                if let Ok(commit) = deref_tag_commit(&ref_json, &obj_json) {
                    rel.commit = commit;
                } else if let Some(fallback) = serde_json::from_str::<serde_json::Value>(&ref_json)
                    .ok()
                    .and_then(|v| {
                        v.get("object")?
                            .get("sha")?
                            .as_str()
                            .map(str::to_string)
                    })
                    .filter(|s| s.chars().all(|c| c.is_ascii_hexdigit()) && s.len() >= 7)
                {
                    // 轻量 tag：object 就是 commit。
                    rel.commit = fallback;
                }
            }
        }
        Err(e) => {
            on_progress(&format!("（提示）解引用 tag 失败（{e}），暂用 target_commitish；安装器仍会按 -Commit 钉死"));
        }
    }
    Ok(rel)
}

/// 下载官方安装脚本到临时文件，返回路径 + sha256 + 字节数。
fn download_script(
    tag: &str,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(PathBuf, String, u64), String> {
    let url = install_script_url(tag)?;
    let tmp_dir = crate::installer::http_temp_dir()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let out = tmp_dir.join(format!("hermes-install-{}-{}.ps1", std::process::id(), stamp));

    // 直接跑系统 curl（不经 installer::curl 的 .NET 回退——那个回退不支持 -o 落文件）。
    // GitHub 的 TLS 在 schannel 下正常；-m 超时是硬约束。
    let (code, err) = crate::installer::run_capture_raw(
        "curl",
        &["-L", "-sS", "-m", "180", "-o", &out.display().to_string(), &url],
        None,
    )
    .map_err(|e| format!("下载官方安装脚本失败：{e}"))?;
    if code != 0 {
        let _ = std::fs::remove_file(&out);
        return Err(format!("下载官方安装脚本失败（curl {code}）：{}", tail(&err, 200)));
    }

    let bytes = std::fs::read(&out).map_err(|e| format!("读取安装脚本失败：{e}"))?;
    if bytes.is_empty() {
        let _ = std::fs::remove_file(&out);
        return Err("下载到的安装脚本为空（0 字节），已拒绝".into());
    }
    let sha = crate::installer::sha256_hex_bytes(&bytes);
    on_progress(&format!(
        "官方安装脚本已就绪：tag={tag} commit 待钉，sha256={sha}，{} 字节",
        bytes.len()
    ));
    Ok((out, sha, bytes.len() as u64))
}

/// 探测是否有 hermes 进程正在跑（只探测，**绝不强杀**）。
/// 返回 None = 没在跑；Some(msg) = 正在跑，安装必须中止并请用户先退出。
fn hermes_running() -> Option<String> {
    #[cfg(windows)]
    {
        // 按 ExecutablePath 前缀匹配官方源码目录（hermes 主进程 / gateway 都从那里跑）。
        // 与 installer::stop_processes_under 同款 CIM 查询，但这里只有 Stop 之外的部分。
        let install = official_install_dir()?;
        let prefix = format!("{}\\", install.display().to_string().trim_end_matches('\\'));
        let script = "$r=$env:UKING_HERMES_ROOT.TrimEnd('\\')+'\\'; \
            $ps=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($r,[StringComparison]::OrdinalIgnoreCase) }); \
            Write-Output $ps.Count";
        let mut c = std::process::Command::new(crate::installer::system_tool("powershell"));
        use std::os::windows::process::CommandExt;
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .env("UKING_HERMES_ROOT", install.as_os_str())
            .creation_flags(0x0800_0000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let out = crate::installer::output_with_timeout(c, 10)?;
        let text = crate::installer::decode_console(&out.stdout);
        let n = text.lines().last().and_then(|l| l.trim().parse::<usize>().ok()).unwrap_or(0);
        (n > 0).then(|| format!("检测到 {n} 个 Hermes 进程正从 {prefix} 运行")).or_else(|| {
            // 兜底：launcher 也可能以镜像名 hermes.exe 出现。
            hermes_image_running_windows()
        })
    }
    #[cfg(not(windows))]
    {
        let mut c = std::process::Command::new("pgrep");
        c.args(["-f", "hermes-agent"]).stdin(std::process::Stdio::null());
        match crate::installer::output_with_timeout(c, 5) {
            Some(out) if out.status.success() => {
                let n = out.stdout.iter().filter(|&&b| b == b'\n').count();
                (n > 0).then(|| format!("检测到 {n} 个 Hermes 相关进程正在运行"))
            }
            _ => None,
        }
    }
}

#[cfg(windows)]
fn hermes_image_running_windows() -> Option<String> {
    let mut c = std::process::Command::new(crate::installer::system_tool("tasklist"));
    use std::os::windows::process::CommandExt;
    c.args(["/FI", "IMAGENAME eq hermes.exe", "/FO", "CSV", "/NH"])
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null());
    let out = crate::installer::output_with_timeout(c, 5)?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find(|l| l.starts_with('"')).map(|_| "检测到 hermes.exe 正在运行".to_string())
}

/// 用官方 bin 目录优先的 PATH 跑一条命令，返回 (退出码, 合并输出)。
/// 这里不复用 installer 私有 `shell_command` / `with_path`（模块边界），但路径与
/// 代理清理口径和它们一致；官方 bin 排在最前是「阴影问题」的钥匙。
fn run_with_official_path(cmdline: &str, timeout_secs: u64) -> Option<(i32, String)> {
    let mut c = shell_like_command(cmdline);
    prepend_official_path(&mut c);
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = crate::installer::output_with_timeout(c, timeout_secs)?;
    let mut text = crate::installer::decode_console(&out.stdout);
    text.push_str(&crate::installer::decode_console(&out.stderr));
    Some((out.status.code().unwrap_or(-1), text))
}

#[cfg(windows)]
fn shell_like_command(cmdline: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(crate::installer::system_tool("cmd"));
    // 与 installer::shell_command 同款 raw_arg：内层引号原样透传，不被 Rust 转义成 `\"`。
    c.raw_arg(format!("/C {cmdline}"));
    c.creation_flags(0x0800_0000);
    c
}

#[cfg(not(windows))]
fn shell_like_command(cmdline: &str) -> std::process::Command {
    let mut c = std::process::Command::new("sh");
    c.args(["-c", cmdline]);
    c
}

/// 清代理 + 官方 bin 优先 + search_paths 拼 PATH（与 installer::with_path 同口径）。
fn prepend_official_path(c: &mut std::process::Command) {
    for v in [
        "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
        "http_proxy", "https_proxy", "all_proxy", "no_proxy",
    ] {
        c.env_remove(v);
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(bin) = official_bin_dir() {
        dirs.push(bin);
    }
    dirs.extend(crate::installer::search_paths(None));
    let old = std::env::var("PATH").unwrap_or_default();
    if dirs.is_empty() {
        return;
    }
    let prefix = dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(sep);
    c.env("PATH", format!("{prefix}{sep}{old}"));
}

/// 沙箱护栏（`install` / `upgrade` / `verify` 共用，**先于任何网络与进程动作**）。
/// 返回 `Some(带 BLOCKED_MARK 的消息)` = 必须当场中止：官方安装器会写
/// `%LOCALAPPDATA%` 与用户 PATH，验证会去执行真实 PATH 上的 hermes —— 沙箱
/// （`UKING_TEST_HOME`）下两者都越界（写用户真实状态、读沙箱外的世界）。
fn sandbox_block() -> Option<String> {
    std::env::var("UKING_TEST_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|_| {
            format!(
                "{BLOCKED_MARK}沙箱（UKING_TEST_HOME）下禁止运行 Hermes 官方安装器/验证（会写 %LOCALAPPDATA%% 与用户 PATH）；请在隔离测试机验证。"
            )
        })
}

/// 安装 / 升级共用的同一份核心。升级不另走一套——重跑同一个「钉 commit + 官方脚本」。
fn run_official_install(
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<InstallOutcome, String> {
    if let Some(blocked) = sandbox_block() {
        return Err(blocked);
    }

    if let Some(running) = hermes_running() {
        return Err(format!(
            "{BLOCKED_MARK}{running}。请先退出 Hermes（及 gateway）再继续，安装不会替你强杀进程。"
        ));
    }

    let rel = latest_release(on_progress)?;
    on_progress(&format!(
        "官方最新 release：{}（发布于 {}），钉 commit {}",
        rel.tag,
        if rel.published_at.is_empty() { "未知".into() } else { rel.published_at.clone() },
        rel.commit
    ));

    let (script_path, script_sha, _size) = download_script(&rel.tag, on_progress)?;
    // 落日志：tag + commit + 脚本哈希，出问题时能核对「到底跑的是哪个版本的内容」。
    on_progress(&format!(
        "执行官方安装器（tag={} commit={} sha256={}）…",
        rel.tag, rel.commit, script_sha
    ));

    run_install_script(&script_path, &rel.commit, on_progress)?;
    // 脚本用完即删（内容已由哈希记录；避免残留到用户机器上）。
    let _ = std::fs::remove_file(&script_path);

    verify_installed(on_progress)
}

/// 起进程 + 逐行回显 + 退出码判定。两个平台的 [`run_install_script`] 只在「怎么起
/// 进程」上不同，读流、回显、失败取尾是同一份（原本抄了两遍，这里是唯一一份）。
fn drive_install_script(
    c: &mut std::process::Command,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    use std::io::BufRead;
    let mut child = c.spawn().map_err(|e| format!("启动官方安装器失败：{e}"))?;
    let stderr = child.stderr.take();
    let err_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let err_store = err_lines.clone();
    let h = stderr.map(|se| {
        std::thread::spawn(move || {
            let br = std::io::BufReader::new(se);
            for line in br.split(b'\n') {
                let Ok(line) = line else { continue };
                let mut line = line;
                while matches!(line.last().copied(), Some(b'\n') | Some(b'\r')) {
                    line.pop();
                }
                if let Ok(mut g) = err_store.lock() {
                    g.push(crate::installer::decode_console(&line));
                }
            }
        })
    });
    if let Some(so) = child.stdout.take() {
        let br = std::io::BufReader::new(so);
        for line in br.split(b'\n') {
            let Ok(line) = line else { continue };
            let mut line = line;
            while matches!(line.last().copied(), Some(b'\n') | Some(b'\r')) {
                line.pop();
            }
            let text = crate::installer::decode_console(&line);
            if !text.trim().is_empty() {
                on_progress(&text);
            }
        }
    }
    if let Some(h) = h {
        let _ = h.join();
    }
    for line in err_lines.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if !line.trim().is_empty() {
            on_progress(line);
        }
    }
    let status = child.wait().map_err(|e| format!("等待官方安装器失败：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        let tail_text = err_lines.lock().unwrap_or_else(|e| e.into_inner()).join("\n");
        Err(format!(
            "官方安装器退出码 {}：{}",
            status.code().unwrap_or(-1),
            tail(&tail_text, 300)
        ))
    }
}

/// 跑官方安装脚本。Windows `install.ps1` / 非 Windows `install.sh` 都已确认支持
/// `-NonInteractive` + `-Commit <sha>` + `-SkipBrowser` + `-SkipComputerUse`。
/// 绝不 `-IncludeDesktop`；客户要浏览器工具可 `hermes pm install agent-browser`。
#[cfg(windows)]
fn run_install_script(
    script_path: &Path,
    commit: &str,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let ps = crate::installer::system_tool("powershell");
    let mut c = std::process::Command::new(&ps);
    c.args([
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", &script_path.display().to_string(),
        "-NonInteractive",
        "-Commit", commit,
        "-SkipBrowser",
        "-SkipComputerUse",
    ])
    .creation_flags(0x0800_0000)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
    drive_install_script(&mut c, on_progress)
}

#[cfg(not(windows))]
fn run_install_script(
    script_path: &Path,
    commit: &str,
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let mut c = std::process::Command::new("bash");
    c.args([
        &script_path.display().to_string(),
        "--non-interactive",
        "--commit", commit,
        "--skip-browser",
        "--skip-computer-use",
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
    drive_install_script(&mut c, on_progress)
}

/// 装后验证：launcher 存在 + `hermes --version` 退出 0 + 输出非空/非 unknown/无导入错误。
/// 绝不把旧 pip 的 0.19.0 当成功。
fn verify_installed(
    on_progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<InstallOutcome, String> {
    let bin = official_bin_dir()
        .ok_or_else(|| "找不到官方 Hermes 安装目录（%LOCALAPPDATA%\\hermes 或 HERMES_HOME）".to_string())?;
    let launcher = launcher_in(&bin)
        .ok_or_else(|| format!("官方 launcher 不在 {}（安装脚本可能没把 hermes 命令发布出来）", bin.display()))?;
    on_progress(&format!("验证官方 launcher：{}", launcher.display()));

    // 配置目录可写 + 实际 --version：两条都在同一轮验证里做掉，别拆成两遍进程。
    config_dir_writable_probe(on_progress);

    let Some((code, out)) = run_with_official_path("hermes --version", 45) else {
        return Err("hermes --version 超时或无法启动（45 秒）".into());
    };
    if code != 0 {
        return Err(format!(
            "hermes --version 退出码 {code}：{}",
            tail(&out, 200)
        ));
    }
    let version = parse_version_output(&out)?;
    on_progress(&format!("Hermes 验证通过：{version}"));
    Ok(InstallOutcome {
        message: format!("Hermes 已安装到官方目录（launcher：{}），版本 {version}", launcher.display()),
        version: Some(version),
    })
}

/// 配置目录可写探针：写一个临时文件再删。失败只提示，不把「装好了」误判成「装坏」——
/// 配置目录不可写属于客户机权限问题，但必须说清楚，否则后续写配置会静默失败。
fn config_dir_writable_probe(on_progress: &(dyn Fn(&str) + Send + Sync)) {
    let Some(home) = official_home_dir() else { return };
    let probe = home.join(format!(".uking-write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"uking") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(e) => {
            on_progress(&format!(
                "注意：Hermes 配置目录 {} 不可写（{e}），后续写配置可能失败",
                home.display()
            ));
        }
    }
}

/// 安装入口（供 `installer.rs` 的 Windows 特判分派调用；与 `upgrade` 同一核心）。
/// 沙箱护栏在**平台分支之前**：任何平台进沙箱都必须是同一条拦截（消息与退出形状一致）。
pub fn install(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<InstallOutcome, String> {
    if let Some(blocked) = sandbox_block() {
        return Err(blocked);
    }
    #[cfg(not(windows))]
    {
        let _ = on_progress;
        // 非 Windows 本轮没接：Mac 落到通用清单（pip 0.19.0）。没验过的落点/验证口径
        // 不许手滑执行，直接拒。
        Err("本平台未接入 Hermes 官方安装器（本轮只接 Windows）；macOS 仍走原 pip 清单（0.19.0）。".into())
    }
    #[cfg(windows)]
    {
        run_official_install(on_progress)
    }
}

/// 升级入口：**不另走一套**——官方安装器本身就是「拉最新 release + 钉 commit」，
/// 重跑即升级（与官方 `hermes update` 同一机制，但不引入浮动分支）。
pub fn upgrade(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<InstallOutcome, String> {
    if let Some(blocked) = sandbox_block() {
        return Err(blocked);
    }
    #[cfg(not(windows))]
    {
        let _ = on_progress;
        Err("本平台未接入 Hermes 官方安装器（本轮只接 Windows）；macOS 仍走原 pip 清单（0.19.0）。".into())
    }
    #[cfg(windows)]
    {
        run_official_install(on_progress)
    }
}

/// 单独的装后验证：不联网、不安装、不杀进程 —— 只查 launcher 落点 + 跑一次
/// `hermes --version` 过门禁（见 [`parse_version_output`]）。供诊断 / `action run` 复用。
/// 沙箱里拒跑：它会去执行真实 PATH 上的 hermes，读到的是沙箱外的世界。
pub fn verify(on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<InstallOutcome, String> {
    if let Some(blocked) = sandbox_block() {
        return Err(blocked);
    }
    #[cfg(not(windows))]
    {
        let _ = on_progress;
        Err("本平台未接入 Hermes 官方安装器（本轮只接 Windows）；落点诊断见 where_report()。".into())
    }
    #[cfg(windows)]
    {
        verify_installed(on_progress)
    }
}

/// 落点诊断（无头入口 `providers::hermes_where` 复用，主会话接线）：
/// 如实报 source=official-bin / legacy-pip、实际 launcher 路径与版本。**不读、不泄任何 Key。**
pub fn where_report() -> serde_json::Value {
    let official = official_launcher();
    let legacy = legacy_pip_launcher();
    let official_version = official
        .as_ref()
        .and_then(|_| run_with_official_path("hermes --version", 45))
        .map(|(code, out)| {
            if code == 0 {
                out.trim().to_string()
            } else {
                format!("(退出码 {code}) {}", out.trim())
            }
        });
    let source = if official.is_some() {
        "official-bin"
    } else if legacy.is_some() {
        "legacy-pip"
    } else {
        "none"
    };
    serde_json::json!({
        "source": source,
        "official_bin_dir": official_bin_dir().map(|p| p.display().to_string()),
        "official_launcher": official.map(|p| p.display().to_string()),
        "official_version": official_version,
        "legacy_pip_launcher": legacy.map(|p| p.display().to_string()),
    })
}

// ============================================================
// 测试（纯夹具，不联网、不真装）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_latest_release_accepts_v0222_shape() {
        let body = r#"{
            "html_url": "https://github.com/NousResearch/hermes-agent/releases/tag/v0.22.2",
            "tag_name": "v0.22.2",
            "draft": false,
            "prerelease": false,
            "target_commitish": "d923a5eb487a17c0a68801bba9ef0f56c81040a8",
            "published_at": "2026-10-15T08:21:32Z"
        }"#;
        let rel = parse_latest_release(body).expect("合法 release 应解析成功");
        assert_eq!(rel.tag, "v0.22.2");
        assert_eq!(rel.commit, "d923a5eb487a17c0a68801bba9ef0f56c81040a8");
    }

    #[test]
    fn parse_latest_release_rejects_wrong_repo() {
        let body = r#"{
            "html_url": "https://github.com/evil/hermes-agent/releases/tag/v0.22.2",
            "tag_name": "v0.22.2",
            "draft": false,
            "prerelease": false,
            "target_commitish": "d923a5eb487a17c0a68801bba9ef0f56c81040a8"
        }"#;
        assert!(parse_latest_release(body).is_err());
    }

    #[test]
    fn parse_latest_release_rejects_draft_and_prerelease() {
        for (key, val) in [("draft", "true"), ("prerelease", "true")] {
            let body = format!(
                r#"{{
                    "html_url": "https://github.com/NousResearch/hermes-agent/releases/tag/v0.22.2",
                    "tag_name": "v0.22.2",
                    "draft": false,
                    "prerelease": false,
                    "target_commitish": "d923a5eb487a17c0a68801bba9ef0f56c81040a8",
                    "{key}": {val}
                }}"#
            );
            assert!(parse_latest_release(&body).is_err(), "{key}={val} 必须拒");
        }
    }

    #[test]
    fn validate_tag_rejects_injection_shapes() {
        for bad in ["", "v", "main", "v0.22.2/../../x", "v0.22.2;id", "v0.22.2%2f", "v0.22.2 x", "v0.22.2&cmd", "v0.22.2\n", "v0.22..2", "v.22.2"] {
            assert!(validate_tag(bad).is_err(), "应拒绝：{bad:?}");
        }
        for good in ["v0.22.2", "v0.22.2-rc.1", "v1.2.3"] {
            assert!(validate_tag(good).is_ok(), "应接受：{good:?}");
        }
    }

    #[test]
    fn install_script_url_is_pinned_and_https() {
        let url = install_script_url("v0.22.2").expect("合法 tag 应产出 URL");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/NousResearch/hermes-agent/v0.22.2/scripts/install.ps1"
        );
        assert!(install_script_url("v0.22.2;rm -rf /").is_err());
    }

    #[test]
    fn deref_tag_commit_handles_annotated_and_lightweight() {
        // annotated tag → 取 git/tags/<sha>.object.sha
        let tag_ref = r#"{"object":{"sha":"7b77e29eedd85a8497ce2821448dac7964dc46f6","type":"tag"}}"#;
        let tag_obj = r#"{"object":{"sha":"d923a5eb487a17c0a68801bba9ef0f56c81040a8","type":"commit"}}"#;
        assert_eq!(
            deref_tag_commit(tag_ref, tag_obj).unwrap(),
            "d923a5eb487a17c0a68801bba9ef0f56c81040a8"
        );
        // lightweight tag → object.sha 直接是 commit
        let tag_ref2 = r#"{"object":{"sha":"d923a5eb487a17c0a68801bba9ef0f56c81040a8","type":"commit"}}"#;
        assert_eq!(
            deref_tag_commit(tag_ref2, "{}").unwrap(),
            "d923a5eb487a17c0a68801bba9ef0f56c81040a8"
        );
    }

    #[test]
    fn parse_version_output_rejects_bad_and_accepts_good() {
        for bad in ["", "   \n  ", "unknown", "version: unknown", "Traceback ... ModuleNotFoundError: No module named 'hermes_cli'", "ImportError: cannot import"] {
            assert!(parse_version_output(bad).is_err(), "应拒绝：{bad:?}");
        }
        for good in ["0.22.2", "d923a5eb487a17c0a68801bba9ef0f56c81040a8", "hermes 0.22.2 (commit d923a5e)"] {
            assert!(parse_version_output(good).is_ok(), "应接受：{good:?}");
        }
        // 多行的健康输出：`unknown` 出现在**后续行**（安装方式/更新状态解析不到是常态）
        // 不算假成功 —— 只有首行是 unknown 才说明源码树没有 install-stamp。
        let healthy = "0.22.2 (commit d923a5eb)\nInstall directory: C:\\Users\\demo\\AppData\\Local\\hermes\\hermes-agent\nInstall method: source\nUpdate status: unknown";
        assert_eq!(
            parse_version_output(healthy).expect("首行是真版本就该过"),
            "0.22.2 (commit d923a5eb)"
        );
        // 反面：首行 unknown + 后面还有真版本号 → 仍然拒
        assert!(parse_version_output("unknown\n0.22.2").is_err());
        // import 挂了出现在任何一行都要拒
        assert!(parse_version_output("0.22.2\nTraceback: ModuleNotFoundError: hermes_cli").is_err());
    }

    #[test]
    fn install_rejects_in_sandbox_without_touching_network() {
        crate::testsandbox::with_sandbox("hermesdist-sandbox", &[], |_root| {
            let calls = std::sync::atomic::AtomicUsize::new(0);
            let progress = |_msg: &str| { let _ = calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed); };
            let err = install(&progress).unwrap_err();
            assert!(err.contains("沙箱"), "沙箱拒绝消息不对：{err}");
            assert!(is_precheck_block(&err), "沙箱拦截必须带 BLOCKED_MARK（上层据此不建 issue）：{err}");
            assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0, "沙箱里不许发任何网络请求/打日志");
            // 升级与验证同一条护栏
            assert!(is_precheck_block(&upgrade(&progress).unwrap_err()));
            assert!(is_precheck_block(&verify(&progress).unwrap_err()));
            assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0, "upgrade/verify 在沙箱里同样一步都不能迈出去");
        });
    }

    #[test]
    fn official_home_respects_hermes_home_env() {
        crate::testsandbox::with_sandbox("hermesdist-home", &[], |_root| {
            std::env::set_var("HERMES_HOME", "D:\\hermes-custom");
            assert_eq!(
                official_home_dir().map(|p| p.display().to_string()),
                Some("D:\\hermes-custom".into())
            );
        });
    }
}
