//! AI 工具安装器 —— 由「安装 skill」（JSON 清单）驱动的步骤执行器。
//!
//! ## 设计
//!
//! - **skill 清单**：`skills/install-windows.json` 内嵌兜底；启动安装前先尝试从
//!   `https://www.u-king.org/skills/install-windows.json` 拉新版（version 更大即覆盖），
//!   实现「服务器控制下发安装逻辑」。离线 / 网站没上线 → 静默用内嵌版。
//! - **步骤类型**：`ensure_node`（缺 Node 自动装便携版到 ~/.uking/runtime）、
//!   `npm_install`（走 npmmirror 国内源）、`run`（任意命令）。
//! - **循环验证**：steps 跑完 → verify_cmd 验证 → 失败自动跑 repair 步骤 → 再验证。
//! - **流式日志**：每行输出经 `on_log(phase, line)` 回调（tauri 侧转成事件给前端聊天气泡）。
//! - 纯 std + serde_json，HTTP 用系统自带 curl.exe（Win10+ 内置），不引重型 crate。

use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// 内嵌的兜底 skill 清单。
const EMBEDDED_SKILL: &str = include_str!("../skills/install-windows.json");
/// 服务器下发地址（依次尝试，第一个拉到合法 JSON 的生效）。
/// 国内直连 Vercel 经常不通（实测 Mac mini 000），所以第一顺位放新加坡服务器。
const SKILL_URLS: &[&str] = &[
    // u-claw.org.cn 是唯一全国内可达子域（cloud.u-claw.org 部分网络 GFW SNI reset，见 CLAUDE.md/Issue#18）
    "https://u-claw.org.cn/uking/install-windows.json",
    "https://cloud.u-claw.org/uking/install-windows.json",
    "https://www.u-king.org/skills/install-windows.json",
    "https://u-king-org.vercel.app/skills/install-windows.json",
];

/// 免费路线独立 Registry：只接受已人工核验的条目。自动巡检只能生成候选，不能越过
/// `status=reviewed` 直接把新渠道送进客户页面。
const FREE_REGISTRY_URLS: &[&str] = &[
    "https://u-claw.org.cn/uking/free-registry.json",
    "https://cloud.u-claw.org/uking/free-registry.json",
    "https://www.u-king.org/free-registry.json",
];

// ============================================================
// skill 清单模型
// ============================================================

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Skill {
    pub skill: String,
    pub version: u64,
    #[serde(default)]
    pub updated: String,
    pub node: NodeSpec,
    pub npm_registry: String,
    /// 便携 Python（Hermes 等 Python 工具用）；老客户端无此字段不影响 Node 工具
    #[serde(default)]
    pub python: Option<PySpec>,
    /// pip 国内源
    #[serde(default = "default_pip_index")]
    pub pip_index: String,
    /// 运行时下载备用基址（公共镜像挂了回退这里，平时不用，零服务器负担）
    #[serde(default)]
    pub mirror_fallback: Option<String>,
    pub tools: std::collections::BTreeMap<String, ToolSpec>,
    /// 加载来源（embedded / server），运行时填充
    #[serde(default)]
    pub source: String,
    /// 「添加供应商」画廊模板（2026-08-22 P3b）——跟这份 skill 清单同一条热下发通道：
    /// 加新厂商改这里就行，不用发版。字段跟前端 `src/lib/providerTemplates.ts::ProviderTemplate`
    /// 一一对应。**老客户端不认识这个字段，serde 忽略未知字段，下发新清单对它们是安全的**
    /// （同上面 `min_windows_build` 的先例；`Skill`/`ToolSpec` 全篇没有 `deny_unknown_fields`）。
    #[serde(default)]
    pub provider_templates: Vec<RemoteProviderTemplate>,
    /// 「免费额度怎么领」教程（2026-08-24）——跟 `provider_templates` 同一条热下发通道。
    /// 免费羊毛寿命以周计，某家下线/改条件只改线上 JSON，不发版；前端以内嵌的
    /// `src/lib/freeGuide.ts` 兜底。同上，老客户端不认识这个字段，serde 忽略，下发安全。
    #[serde(default)]
    pub free_guide: Option<RemoteFreeGuide>,
}

/// 免费额度教程。**故意不带任何端点和 Key**：条目靠 `template` 按名字指向
/// `provider_templates` 里已有的那条，端点只存在一份（宪法第 8 条：同一事实存几份就漂几份）。
/// 我们也不替客户带 Key —— 没有 Key 是客户自己想办法的事，教程只负责说清去哪领。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteFreeGuide {
    /// 单调递增；客户端拿它跟本地记过的比，大了就提示「免费清单有更新」
    pub version: u32,
    /// 内容最后核实的日期，直接显示给客户 —— 让他知道这份有多旧
    #[serde(default)]
    pub checked: String,
    #[serde(default)]
    pub entries: Vec<RemoteFreeGuideEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteFreeGuideEntry {
    pub name: String,
    /// 模板画廊里的 `name`，逐字相等才给「一键导入」按钮；对不上就只显示说明，不报错
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub key_url: Option<String>,
    /// 客户会撞墙的条件（要不要卡 / 限不限量 / 有没有地域门槛）
    #[serde(default)]
    pub note: Option<String>,
}

/// 一条「添加供应商」模板。`openai_base` 是唯一必填项——空模板没有意义；
/// 其余全部 `#[serde(default)]`，缺了就是「这家没有」，不是解析失败。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteProviderTemplate {
    pub name: String,
    pub openai_base: String,
    #[serde(default)]
    pub anthropic_base: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub small_model: Option<String>,
    #[serde(default)]
    pub key_url: Option<String>,
    #[serde(default)]
    pub key_hint: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeSpec {
    pub version: String,
    /// Windows x64 zip
    pub url: String,
    pub dir_name: String,
    /// macOS（tar.gz），可选：老版本服务器清单没有这些字段也能解析
    #[serde(default)]
    pub url_mac_arm64: String,
    #[serde(default)]
    pub dir_name_mac_arm64: String,
    #[serde(default)]
    pub url_mac_x64: String,
    #[serde(default)]
    pub dir_name_mac_x64: String,
    /// 各平台包的 SHA-256（小写十六进制）。留空 = 只按最小字节兜底、不校验哈希，
    /// 老清单无此字段照常解析（向后兼容）。防代理缓存返回损坏/错误页导致解压出垃圾。
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub sha256_mac_arm64: String,
    #[serde(default)]
    pub sha256_mac_x64: String,
}

fn default_pip_index() -> String {
    "https://mirrors.aliyun.com/pypi/simple/".into()
}

/// 校验刚下载的文件：先看字节数（挡代理返回的错误页/被截断的半包），
/// 再按需比对 SHA-256（挡代理缓存的损坏/被篡改包）。任一不过都当下载失败，
/// 好让上层触发备用源重试。`expected_sha` 为空只查大小；`min_bytes` 为 0 不查大小。
fn verify_download(out: &str, expected_sha: &str, min_bytes: u64) -> Result<(), String> {
    let bytes = std::fs::read(out).map_err(|e| format!("读取下载文件失败: {e}"))?;
    if min_bytes > 0 && (bytes.len() as u64) < min_bytes {
        return Err(format!(
            "下载文件过小（{} 字节，应≥{} 字节），多半是被代理截断或返回了错误页",
            bytes.len(),
            min_bytes
        ));
    }
    let expected = expected_sha.trim();
    if !expected.is_empty() {
        let got = crate::device::sha256_hex_bytes(&bytes);
        if !got.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "文件校验失败（SHA-256 不符，可能被代理缓存损坏/篡改）：期望 {expected}，实得 {got}"
            ));
        }
    }
    Ok(())
}

/// 校验通过之后、解压之前，安装包是不是被别人动过。返回 `Some(结论)` = 被动过。
///
/// ## 为什么要有这个判断（issue #288，0.9.80，客户机装 Hermes）
///
/// 日志链是这样的：SHA-256 校验**通过** → `tar.exe` 报「Damaged tar archive (bad header
/// checksum)」→ 落到内置解压器时**文件已经不在了**（`os error 2`）。一个几百毫秒前刚
/// 逐字节校验过的包，先损坏再消失，只有一种解释：**杀软实时防护把它改写/隔离了**。
/// 那台机器上 360 安全卫士正是激活状态（另两个杀软被它顶成关闭）。
///
/// ## 这个判断值钱在能把结论钉死
///
/// 校验过 = 字节曾经是对的 = **不是网络问题**。而我们原本对所有解压失败一律说
/// 「多为下载包被截断/损坏，请换网络或关代理重试」—— 客户照着去换网络、关代理，
/// 折腾半天一点用没有，因为病根在杀软。**报错指错方向比不报错更坏**：它让人带着
/// 虚假的确定性去排错，还会把真正的线索挤出视野。
fn archive_tampered_after_verify(pkg: &Path, verified_len: u64) -> Option<String> {
    tamper_verdict(pkg, std::fs::metadata(pkg).ok().map(|m| m.len()), verified_len)
}

/// [`archive_tampered_after_verify`] 的**纯判据**（IO 已在外面做完，这里只做判断）。
///
/// 拆出来是为了能确定性地测：把判据和文件系统绑在一起测，就得在测试里真写真删文件，
/// 而 Windows 上删刚落盘的文件会偶发 `PermissionDenied`（杀软正在扫它）——
/// 结果是**一个确定性判据被测成随机挂的用例**（本机实测：单跑过、全量跑挂）。
/// 测试要盯的是「什么情况该扣杀软帽子」，不是操作系统当时的心情。
fn tamper_verdict(pkg: &Path, current_len: Option<u64>, verified_len: u64) -> Option<String> {
    const ADVICE: &str = "请把 %TEMP% 和 ~/.uking 加入杀毒软件（360 / 火绒 / Defender）信任区，或临时退出防护后重试；换网络、关代理都没用。";
    match current_len {
        None => Some(format!(
            "刚校验通过的安装包在解压前从磁盘上消失了（{}）—— 这是杀毒软件删除/隔离的典型表现，不是网络问题。{ADVICE}",
            pkg.display()
        )),
        Some(n) if n != verified_len => Some(format!(
            "刚校验通过的安装包在解压前被改动了（校验时 {verified_len} 字节，解压时 {n} 字节）—— 多为杀毒软件实时防护改写/截断，不是网络问题。{ADVICE}"
        )),
        Some(_) => None,
    }
}

/// 给 URL 追加 cache-bust 查询参数 —— 代理/CDN 把某个包缓存成损坏版时（SHA-256 不符，
/// issue #178/#179/#185），原样重下会命中同一坏缓存；换个 query 串能绕开中间缓存拿到源站新副本。
/// n==0 不改（首次正常下载，不给源站/CDN 添缓存碎片）。
fn with_cache_bust(url: &str, n: u32) -> String {
    if n == 0 {
        return url.to_string();
    }
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}ukcb={n}")
}

/// 下载文件，主 URL 失败时把文件名拼到备用基址重试（备用基址 = 我们服务器）。
/// 平时走公共镜像（零服务器负担），只有镜像挂了才回退服务器。
/// 下载成功后按 `expected_sha` + `min_bytes` 做完整性校验；**校验不过也回退备用源**
/// （公共镜像常见坑：代理缓存返回损坏包/错误页，HTTP 200 但内容是垃圾）。
/// `cache_bust`>0 时给主源+备用源都追加 cache-bust query，绕开被代理缓存住的损坏副本（重试时用）。
fn download_with_fallback(
    url: &str,
    out: &str,
    fallback_base: &Option<String>,
    expected_sha: &str,
    min_bytes: u64,
    cache_bust: u32,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<(), String> {
    // 主源：下载 + 校验，任一失败都进入备用源分支。
    let url_cb = with_cache_bust(url, cache_bust);
    let primary = curl(&["-L", "-sS", "-m", "600", "-o", out, &url_cb])
        .and_then(|_| verify_download(out, expected_sha, min_bytes));
    let e = match primary {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    let Some(base) = fallback_base.as_ref().filter(|b| b.starts_with("https://")) else {
        return Err(e);
    };
    let fname = url.rsplit('/').next().unwrap_or("file");
    // 阿里云 OSS 把 URL 路径里的 `+` 解码成空格（python-build-standalone 文件名含 `+`，
    // 如 cpython-3.12.7+20241016），不编码会 404/返回 452 字节的 NoSuchKey（issue #131）。
    // 兜底源固定是我们的 OSS，这里把文件名的 `+` 编码成 %2B 让它命中真实对象。
    let fname_enc = fname.replace('+', "%2B");
    let alt = format!("{}/{}", base.trim_end_matches('/'), fname_enc);
    let alt_cb = with_cache_bust(&alt, cache_bust);
    on_log("out", "公共镜像下载或校验未过，改用 U-King 服务器备用源…");
    curl(&["-L", "-sS", "-m", "600", "-o", out, &alt_cb])
        .map_err(|e2| format!("主源失败({e})；备用源下载也失败({e2})"))
        .and_then(|_| {
            verify_download(out, expected_sha, min_bytes)
                .map_err(|e2| format!("主源失败({e})；备用源校验也未过({e2})"))
        })
}

/// 便携 Python 规格（python-build-standalone，解压即用、自带 pip）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PySpec {
    pub url: String,
    #[serde(default)]
    pub url_mac_arm64: String,
    #[serde(default)]
    pub url_mac_x64: String,
    /// 各平台包的 SHA-256（小写十六进制）。留空 = 只按最小字节兜底，向后兼容老清单。
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub sha256_mac_arm64: String,
    #[serde(default)]
    pub sha256_mac_x64: String,
}

impl PySpec {
    /// 当前平台的 (下载地址, 期望 SHA-256)。sha256 可能为空（老清单）。
    fn url_for_platform(&self) -> Result<(&str, &str), String> {
        #[cfg(windows)]
        {
            Ok((&self.url, &self.sha256))
        }
        #[cfg(target_os = "macos")]
        {
            let (u, s) = if std::env::consts::ARCH == "aarch64" {
                (&self.url_mac_arm64, &self.sha256_mac_arm64)
            } else {
                (&self.url_mac_x64, &self.sha256_mac_x64)
            };
            if u.is_empty() { Err("skill 缺少 macOS Python 下载地址".into()) } else { Ok((u, s)) }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Err("当前平台暂不支持自动装 Python".into())
        }
    }
}

impl NodeSpec {
    /// 当前平台的 (下载地址, 解压出的目录名, 期望 SHA-256)。sha256 可能为空（老清单）。
    fn for_platform(&self) -> Result<(&str, &str, &str), String> {
        #[cfg(windows)]
        {
            Ok((&self.url, &self.dir_name, &self.sha256))
        }
        #[cfg(target_os = "macos")]
        {
            let (u, d, s) = if std::env::consts::ARCH == "aarch64" {
                (&self.url_mac_arm64, &self.dir_name_mac_arm64, &self.sha256_mac_arm64)
            } else {
                (&self.url_mac_x64, &self.dir_name_mac_x64, &self.sha256_mac_x64)
            };
            if u.is_empty() {
                Err("skill 清单缺少 macOS Node 下载地址".into())
            } else {
                Ok((u, d, s))
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Err("当前平台暂不支持自动装 Node".into())
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub bin: String,
    pub verify_cmd: String,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub repair: Vec<Step>,
    /// 安装所需最小磁盘空间（MB，系统盘）。0 = 不检查。
    /// 磁盘满时 curl 只会报「(23) client returned ERROR on write」天书
    /// （us-server 实测：C 盘剩 20MB 下载 550MB MSIX 炸），装前检查给人话。
    #[serde(default)]
    pub min_free_mb: u64,
    /// 安装所需最低 Windows 内部版本号（build）。0 = 不检查；非 Windows 平台一律不检查。
    ///
    /// 为什么要有：Codex 桌面版是 MSIX，清单里写死 `MinVersion 10.0.19041.0`。Windows 10 1809
    /// （build 17763，LTSC / 政企机器上很常见）上**微软商店和 MSIX 两条路都必然失败** ——
    /// 商店报 `0x803fb103 该包与当前 Windows 版本或平台不兼容`，MSIX 报 `0x80073CFD`。
    /// 而我们的失败流是「步骤失败 → 自动 repair」，于是客户要先等一个 **667MB** 的下载跑完，
    /// 再看到一句 GBK 乱码的报错（issue #357，同型号还有 #339 #335 #334 #328）。
    /// 装不上的原因是这台机器的系统版本，重试一百次也一样 —— 提前拦下，说人话，指 CLI 版。
    ///
    /// 跟 `min_free_mb` 同层：在 steps 之前判定，**不进 repair 循环**。
    /// 老客户端不认识这个字段（serde 忽略未知字段），下发新清单对它们是安全的。
    #[serde(default)]
    pub min_windows_build: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum Step {
    #[serde(rename = "ensure_node")]
    EnsureNode {
        label: String,
        /// 这个工具**跑得起来**所需的最低 Node 版本（如 pi 的 `22.19.0`）。
        ///
        /// 不填 = 老行为：只要机器上有 Node 就认，不管版本（用户自己的环境我们不评判）。
        /// 填了 = 系统 Node 版本不够时**也装我们自己的便携版**并前置进 PATH。
        /// 仍然**不碰用户的 Node**——只是自己带一份，跟中文路径下自带 Python 是同一套做法。
        ///
        /// 为什么非加不可：`npm i -g` 对 `engines` 只警告不拦（除非 engine-strict），
        /// 于是「装成功了、verify 也可能过、真干活时报 Node.js vX+ is required」——
        /// 这正是 issue #75 那个病根的另一面，只不过那次是我们自己的便携版太旧。
        #[serde(default)]
        min: Option<String>,
    },
    #[serde(rename = "npm_install")]
    NpmInstall {
        label: String,
        package: String,
        #[serde(default)]
        force: bool,
        /// 显式拉取可选依赖。codex 的平台二进制（@openai/codex-win32-x64）是
        /// optionalDependency，npmmirror 滞后/代理抖动时会被悄悄跳过 → codex 装上了
        /// 但 `codex --version` 崩。开这个加 `--include=optional` 强制拉。
        #[serde(default)]
        with_optional: bool,
    },
    #[serde(rename = "ensure_python")]
    EnsurePython { label: String },
    #[serde(rename = "pip_install")]
    PipInstall {
        label: String,
        package: String,
        #[serde(default)]
        force: bool,
    },
    #[serde(rename = "run")]
    Run {
        label: String,
        cmd: String,
        /// 只在这个平台上跑：`windows` | `macos` | `linux`。不填 = 每个平台都跑（老行为）。
        ///
        /// 为什么非有不可：**Mac 和 Windows 读的是同一份 `install-windows.json`**（文件名是
        /// 历史包袱）。清单里那些 `%SystemRoot%\…\powershell.exe …` 的步骤在 macOS 上会被
        /// `sh` 执行，报 `sh: line 0: fg: no job control` 然后整条安装被判失败、进修复循环 ——
        /// 而 pip 依赖其实早就装完了（issue #340：客户 hermes 明明能用，界面却说装不上）。
        ///
        /// 老客户端不认识这个字段，serde 默认忽略未知字段，所以**新清单下发给旧版本是安全的**
        /// （行为跟现在一样，不会更坏）。
        #[serde(default)]
        os: Option<String>,
    },
}

/// 当前平台在 skill 清单里的写法。
fn current_os_tag() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

impl Step {
    fn label(&self) -> &str {
        match self {
            Step::EnsureNode { label, .. }
            | Step::Run { label, .. }
            | Step::EnsurePython { label } => label,
            Step::NpmInstall { label, .. } | Step::PipInstall { label, .. } => label,
        }
    }
}

/// 安装结果（回前端）。
#[derive(Debug, Clone, Serialize)]
pub struct InstallToolResult {
    pub ok: bool,
    pub tool: String,
    /// verify_cmd 输出（一般是版本号）
    pub version: Option<String>,
    /// 1 = 一次成功；2 = 经过修复后成功
    pub attempts: u32,
    pub error: Option<String>,
}

// ============================================================
// skill 加载（服务器优先，内嵌兜底）
// ============================================================

/// 加载 skill：先内嵌，再尝试服务器下发（version 更大才覆盖）。
pub fn load_skill() -> Skill {
    let mut skill: Skill =
        serde_json::from_str(EMBEDDED_SKILL).expect("内嵌 install-windows.json 解析失败");
    skill.source = "embedded".into();

    if let Some(remote) = fetch_remote_skill() {
        if remote.version > skill.version {
            let mut remote = remote;
            remote.source = "server".into();
            return remote;
        }
    }
    skill
}

/// 版本检查地址（国内优先）。
const VERSION_URLS: &[&str] = &[
    // u-claw.org.cn 国内可达优先（cloud.u-claw.org 部分网络 SNI reset）
    "https://u-claw.org.cn/uking/version.json",
    "https://cloud.u-claw.org/uking/version.json",
    "https://www.u-king.org/version.json",
];

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    /// 至少有一个版本源返回了可解析的 JSON。false 代表「暂时查不到」，不是「已是最新版」。
    pub checked_ok: bool,
    pub notes: String,
    pub download_url: String,
    /// 历史版本更新日志（服务器 version.json 的 `history`，新→旧）。
    /// 放服务器而不是编进 exe：**改一句话不用发一次版**（同 Feed 的思路）。
    /// 拉不到就是空数组 —— 离线时前端如实说「需要联网才能看历史」，不假装有。
    pub history: Vec<VersionNote>,
    /// ★ 这台机器**自动升级到这个版本失败过几次**（本地账本，见 [`record_update_failure`]）。
    /// ≥1 前端就把横幅从「一键升级」换成「下载安装包重装」——
    /// 自动替换有一堆我们控制不了的失败源（杀软锁文件 / 目录只读 / 安装路径含非 ASCII /
    /// 替换脚本被拦），**在同一条路上反复重试是最没用的建议**。
    pub failed_attempts: u32,
    /// 上次失败的原因原文（给客服看，也给客户一个交代；没失败过是空串）。
    pub fail_reason: String,
    /// 安装包（覆盖安装）地址 —— 自动升级走不通时的兜底路。
    pub installer_url: String,
}

/// 一条历史版本记录。
#[derive(Debug, Serialize, Clone, Default)]
pub struct VersionNote {
    pub version: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub notes: String,
}

/// 「自动升级失败」本地账本。放 exe 同目录（和暂存文件同一处），跟着这份安装走：
/// 换机器 / 重装后自然清零，不会把别的机器的失败带过来。
const UPDATE_HEALTH: &str = ".uking-update-health.json";

/// 记一次自动升级失败。**同一目标版本累加**；换了目标版本从头计数
///（新版可能恰好修了上个版本升不动的原因，不该继承旧账）。
///
/// 为什么要落盘：`self_update` 失败只弹了一句 toast，用户点掉就没了，下次开机横幅照旧
/// 写着「一键升级」—— 于是「点升级 → 失败 → 关掉 → 再看到横幅 → 再点」无限循环，
/// 这正是「老是有新版本，就是升不上去」的体感来源。失败要留痕，UI 才能换条路走。
pub fn record_update_failure(target: &str, reason: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent() else { return };
    let n = record_update_failure_in(dir, target, reason);
    crate::ulog::write("update", &format!("自动升级到 {target} 失败第 {n} 次：{reason}"));
}

/// [`record_update_failure`] 的纯目录版（好测；也让「账本长什么样」只有一处实现）。
/// 返回累计失败次数。
fn record_update_failure_in(dir: &Path, target: &str, reason: &str) -> u64 {
    let path = dir.join(UPDATE_HEALTH);
    let prev = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let same_target = prev
        .as_ref()
        .and_then(|v| v.get("target").and_then(|x| x.as_str()))
        .map(|t| t == target)
        .unwrap_or(false);
    let n = if same_target {
        prev.as_ref().and_then(|v| v.get("failed").and_then(|x| x.as_u64())).unwrap_or(0) + 1
    } else {
        1
    };
    let _ = std::fs::write(
        &path,
        serde_json::json!({ "target": target, "failed": n, "reason": reason }).to_string(),
    );
    n
}

/// 升成功了就把账本抹掉（下次再有新版又是干净的一次机会）。
pub fn clear_update_failures() {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent() else { return };
    let _ = std::fs::remove_file(dir.join(UPDATE_HEALTH));
}

/// 记下「这次要升到哪个版本」。**必须在派生替换脚本之前写** —— 脚本是在本进程退出**之后**
/// 才跑的，它失败时暂存元数据早被消费掉了，重启回来就没人知道那次是想升到哪一版。
/// 没有这一笔，替换脚本的失败只能进日志，进不了账本，界面也就永远不会改口。
#[cfg(windows)]
fn mark_update_pending(dir: &Path, target: &str) {
    let path = dir.join(UPDATE_HEALTH);
    let mut v = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    v["pending"] = serde_json::Value::String(target.to_string());
    let _ = std::fs::write(&path, v.to_string());
}

/// 取出上一轮「想升到哪一版」（读完即清）。
#[cfg(windows)]
fn take_update_pending(dir: &Path) -> Option<String> {
    let path = dir.join(UPDATE_HEALTH);
    let txt = std::fs::read_to_string(&path).ok()?;
    let mut v: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let target = v.get("pending")?.as_str()?.to_string();
    v.as_object_mut()?.remove("pending");
    let _ = std::fs::write(&path, v.to_string());
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}

/// 读账本：`(失败次数, 原因)`，只在 `target` 与传入版本一致时算数。
fn update_failures_for(target: &str) -> (u32, String) {
    let Ok(exe) = std::env::current_exe() else { return (0, String::new()) };
    let Some(dir) = exe.parent() else { return (0, String::new()) };
    update_failures_in(dir, target)
}

/// [`update_failures_for`] 的纯目录版。
fn update_failures_in(dir: &Path, target: &str) -> (u32, String) {
    let Ok(txt) = std::fs::read_to_string(dir.join(UPDATE_HEALTH)) else {
        return (0, String::new());
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) else {
        return (0, String::new());
    };
    if v.get("target").and_then(|x| x.as_str()) != Some(target) {
        return (0, String::new());
    }
    (
        v.get("failed").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        v.get("reason").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    )
}

fn empty_update_info(current: &str) -> UpdateInfo {
    UpdateInfo {
        current: current.to_string(),
        latest: current.to_string(),
        has_update: false,
        checked_ok: false,
        notes: String::new(),
        download_url: if cfg!(target_os = "macos") {
            "https://u-claw.org.cn/uking/".into()
        } else {
            "https://u-claw.org.cn/download/U-King-Setup.exe".into()
        },
        history: Vec::new(),
        failed_attempts: 0,
        fail_reason: String::new(),
        installer_url: if cfg!(target_os = "macos") {
            "https://u-claw.org.cn/uking/".into()
        } else {
            "https://u-claw.org.cn/download/U-King-Setup.exe".into()
        },
    }
}

/// 从已经取得的版本响应中选出最高版本；网络与本地失败账本都留在外层，保证此处可回归测试。
/// 同版本按 VERSION_URLS 的声明顺序取胜，避免镜像短暂不同步时字段跨源混搭。
pub(crate) fn pick_update_from_responses(current: &str, responses: &[(String, String)]) -> UpdateInfo {
    let mut winner: Option<(usize, UpdateInfo)> = None;

    for (response_index, (url, out)) in responses.iter().enumerate() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(out) else { continue };
        let mut candidate = empty_update_info(current);
        candidate.checked_ok = true;
        candidate.latest = v.get("version").and_then(|x| x.as_str()).unwrap_or(current).to_string();
        candidate.notes = v.get("notes").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if let Some(download_url) = v.get("download_url").and_then(|x| x.as_str()) {
            candidate.download_url = download_url.to_string();
        }
        candidate.history = v
            .get("history")
            .and_then(|h| h.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|e| VersionNote {
                        version: e.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        date: e.get("date").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        notes: e.get("notes").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    })
                    .filter(|n| !n.version.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let source_rank = VERSION_URLS
            .iter()
            .position(|known_url| *known_url == url)
            .unwrap_or(VERSION_URLS.len() + response_index);
        let should_replace = match &winner {
            None => true,
            Some((winner_rank, info)) => {
                semver_gt(&candidate.latest, &info.latest)
                    || (!semver_gt(&info.latest, &candidate.latest) && source_rank < *winner_rank)
            }
        };
        if should_replace {
            winner = Some((source_rank, candidate));
        }
    }

    let mut info = winner.map(|(_, info)| info).unwrap_or_else(|| empty_update_info(current));
    info.has_update = info.checked_ok && semver_gt(&info.latest, current);
    info
}

/// 检查是否有新版（拉服务器 version.json 与内置版本比对）。
pub fn check_update() -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let mut responses = Vec::new();

    for url in VERSION_URLS {
        if let Ok(out) = curl(&["-sL", "-m", "6", url]) {
            responses.push(((*url).to_string(), out));
        }
    }

    let mut info = pick_update_from_responses(&current, &responses);
    if info.checked_ok {
        let (n, why) = update_failures_for(&info.latest);
        info.failed_attempts = n;
        info.fail_reason = why;
    }
    info
}

/// 朴素 semver 比较：a > b ？（仅取数字段，够用）。
fn semver_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 自升级用的「绿色版 exe」下载地址（国内优先）。注意：这里要的是可原地替换的
/// 单文件 exe（U-King.exe），不是 NSIS 安装包（U-King-Setup.exe）。
#[cfg(windows)]
const SELF_EXE_URLS: &[&str] = &[
    // 阿里云 OSS 国内主源（5MB/s，远快于新加坡的 19KB/s）
    "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/U-King.exe",
    "https://cloud.u-claw.org/download/U-King.exe",
    "https://www.u-king.org/download/U-King.exe",
];

#[cfg(target_os = "macos")]
const SELF_MAC_ZIP_URLS: &[&str] = &[
    // 阿里云 OSS 国内主源（对齐 Windows 版：实测 <1s 起下载，远快于新加坡的几秒延迟）；
    // 原来只有 cloud.u-claw.org 排第一，该子域在部分国内网络被 SNI 阻断（见项目文档），
    // 会导致 Mac 用户点「一键升级」卡在第一个地址迟迟无反应，误以为没更新可用。
    "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/U-King-Mac.zip",
    "https://cloud.u-claw.org/download/U-King-Mac.zip",
    "https://u-claw.org.cn/download/U-King-Mac.zip",
    "https://www.u-king.org/download/U-King-Mac.zip",
];

// ====== 自升级暂存文件（都放 exe 同目录，dotfile 隐藏，和 current_exe 同盘） ======
// 设计：把「下载」和「套用」拆开 —— 后台静默下好暂存（不动正在运行的 exe），下次裸启动
// 时再原子替换（旧 exe 改名留底 → 新版就位 → 失败回滚）。**根治「升级后双击没反应」**：
// 旧版做的是 `del 旧 → move 新→旧`，慢盘/U盘上 del 成功而 move 失败会让 U-King.exe 凭空
// 消失；新版全程旧 exe 要么没动、要么已回滚，可执行文件永不缺失。
#[cfg(windows)]
const STAGED_EXE: &str = ".U-King.new.exe"; // 后台下好、待套用的新版
#[cfg(windows)]
const STAGED_META: &str = ".U-King.new.json"; // 暂存元数据 {version,size}
#[cfg(windows)]
const APPLY_EXE: &str = ".U-King.apply.exe"; // 套用时从 STAGED_EXE 改名而来（消费一次，防失败死循环）
#[cfg(windows)]
const DL_TMP: &str = ".U-King.dl.tmp"; // 下载中临时名（下完整原子改名为 STAGED_EXE）

// 同一时刻只允许一个下载（后台静默下载 vs 手动「一键升级」可能并发，串行化防同写一文件）。
#[cfg(windows)]
static DL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 当前 exe 路径 + 其所在目录（自升级暂存文件都放这里）。
#[cfg(windows)]
fn exe_dir() -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败: {e}"))?;
    let dir = exe.parent().ok_or("无法定位程序目录")?.to_path_buf();
    Ok((exe, dir))
}

/// 这个文件「像不像」一个真 exe：体积达标 + PE 头是 "MZ"。挡住下到错误页 / 截断 / 0 字节
/// 的坏文件被换进去（坏 exe 正是「双击没反应」的另一来源）。
#[cfg(windows)]
fn looks_like_exe(path: &Path, min: u64) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() < min {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).is_ok() && &magic == b"MZ"
}

/// 下载新版绿色 exe 到暂存文件并校验。**幂等**：若已暂存好同版本（校验通过）直接复用，
/// 不重复下载（后台已下好时，手动点「一键升级」秒回）。成功后 STAGED_EXE 就位 + 写 STAGED_META。
#[cfg(windows)]
fn download_new_exe(expected_version: &str, progress: &dyn Fn(&str, u8)) -> Result<PathBuf, String> {
    let _guard = DL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_exe, dir) = exe_dir()?;
    let staged = dir.join(STAGED_EXE);
    let meta_path = dir.join(STAGED_META);
    let tmp = dir.join(DL_TMP);

    // 幂等：已暂存好且版本号匹配 + 文件完好 → 直接复用
    if looks_like_exe(&staged, 1_000_000) {
        if let Ok(txt) = std::fs::read_to_string(&meta_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if v.get("version").and_then(|x| x.as_str()) == Some(expected_version) {
                    progress("download", 100);
                    return Ok(staged);
                }
            }
        }
    }

    let _ = std::fs::remove_file(&tmp);
    progress("download", 0);
    let mut last_err = String::new();
    for url in SELF_EXE_URLS {
        let total = head_content_length(url).unwrap_or(5_800_000);
        let child = base_command("curl")
            .args(["-sSL", "-A", "Mozilla/5.0 U-King", "-m", "180", "-o", &tmp.to_string_lossy(), url])
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                last_err = format!("启动下载失败: {e}");
                continue;
            }
        };
        // 轮询到 curl 退出，每 200ms 按已落盘大小上报一次（封顶 99%，100% 留给校验通过后）
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    let cur = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
                    let pct = (cur.saturating_mul(100) / total.max(1)).min(99) as u8;
                    progress("download", pct);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Err(_) => break None,
            }
        };
        match status {
            Some(s) if s.success() && looks_like_exe(&tmp, 1_000_000) => {
                // 下载完整 + PE 头正确 → 原子改名到暂存名，再写元数据
                let _ = std::fs::remove_file(&staged);
                if let Err(e) = std::fs::rename(&tmp, &staged) {
                    last_err = format!("暂存改名失败: {e}");
                    let _ = std::fs::remove_file(&tmp);
                    continue;
                }
                let sz = std::fs::metadata(&staged).map(|m| m.len()).unwrap_or(0);
                let _ = std::fs::write(
                    &meta_path,
                    serde_json::json!({ "version": expected_version, "size": sz }).to_string(),
                );
                progress("download", 100);
                return Ok(staged);
            }
            Some(s) if s.success() => {
                last_err = "下载文件异常（疑似错误页 / 截断）".into();
                let _ = std::fs::remove_file(&tmp);
            }
            Some(s) => {
                last_err = format!("下载失败（curl {s}）");
                let _ = std::fs::remove_file(&tmp);
            }
            None => {
                last_err = "下载进程异常".into();
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(if last_err.is_empty() { "下载新版失败".into() } else { last_err })
}

/// 写「原子替换脚本」并 detached 启动。脚本逻辑：等当前进程退出 → 旧 exe **改名**留底（不是 del！）
/// → 新版就位 → 任一步失败就把旧 exe 改回来（回滚）→ 拉起 exe → 自删。
/// **不变量：任何时刻 U-King.exe 要么是旧版、要么是新版，绝不缺失**（根治升级后点不动）。
/// `src_new` = 已校验、待就位的新版 exe（套用流程里是 APPLY_EXE）。
#[cfg(windows)]
fn spawn_swap(dir: &Path, exe: &Path, src_new: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let updater_bat = dir.join(".uking-update.bat");
    let flag = dir.join(".uking-updated");
    let _ = std::fs::remove_file(&flag);

    // ★★ 脚本正文里**一个非 ASCII 字符都不许出现** —— 别再把路径字面量写进 bat。
    // cmd.exe 是按系统 ANSI 代码页（中文 Windows = 936/GBK）逐行解析 .bat 的，而 Rust
    // `fs::write` 落盘的是 UTF-8。安装路径里只要有一个中文/全角字符，所有 `set "EXE=…"`
    // 全成乱码：连第一行 `echo … > "%LOG%"` 都写不出去，脚本无声走完 giveup 再自删 ——
    // 旧 exe 原地不动、**且不留任何日志**，客户只看到「点了升级，程序闪退，还是旧版」。
    // pc-*** 实证（2026-07-30）：用户名 `demo（无密码）`，ACP=936，新包下好校验通过，
    // 但 .uking-update.log 从来不存在。本机等价复现（UTF-8 bat + CreateProcessW）：
    // cmd 退出码 1、日志零字节；改用 %~dp0 推导后退出码 0、日志正常。
    // 解法：目录由 cmd 自己提供的 %~dp0 给出（走命令行参数，不经 bat 正文解码），
    // 正文只拼纯 ASCII 文件名。
    //
    // ★ 脚本里调的系统工具也一律走 %SystemRoot%\System32 绝对路径 —— 和 `base_command`
    // 同一条理由（「不赌客户 PATH」），但这份 bat 一直漏在外面。装了 Git for Windows 的机器
    // 上，裸 `find` 会解析到 GNU find，`tasklist | find "<pid>"` 当场失效 → 脚本以为
    // U-King 已经退出，抢在进程还活着时就去改名，替换必然失败。端到端用例实测抓到。
    if exe.parent() != Some(dir) || src_new.parent() != Some(dir) {
        return Err("升级脚本与程序不在同一目录，无法安全替换".into());
    }
    let ascii_name = |p: &Path, what: &str| -> Result<String, String> {
        let name = p.file_name().and_then(|s| s.to_str()).ok_or_else(|| format!("{what}文件名无法解析"))?;
        if !name.is_ascii() {
            // 如实报错，好过再来一次无声失败。
            return Err(format!("{what}文件名含非 ASCII 字符（{name}），无法用批处理安全替换；请到下载页手动获取新版覆盖安装"));
        }
        Ok(name.to_string())
    };
    let exe_name = ascii_name(exe, "程序")?;
    let new_name = ascii_name(src_new, "新版包")?;
    let bat = updater_script(pid, &exe_name, &new_name);
    std::fs::write(&updater_bat, bat)
        .map_err(|e| format!("写升级脚本失败: {e}"))
        .inspect_err(|e| crate::ulog::write("update", e))?;
    // detached 启动脚本（CREATE_NO_WINDOW 隐窗 + 脱离父进程），随后主进程退出
    base_command("cmd")
        .args(["/C", &updater_bat.to_string_lossy()])
        .spawn()
        .map_err(|e| format!("启动升级脚本失败: {e}"))
        .inspect_err(|e| crate::ulog::write("update", e))?;
    crate::ulog::write(
        "update",
        &format!("替换脚本已启动（等 pid={pid} 退出后换 {exe_name}）；结果见同目录 .uking-update.log"),
    );
    Ok(())
}

/// 生成替换脚本正文。**纯函数**，好让 `updater_script_is_pure_ascii` 用例守着那条唯一的铁律：
/// 正文里不许出现非 ASCII 字节（理由见 [`spawn_swap`] 的长注释）。
fn updater_script(pid: u32, exe_name: &str, new_name: &str) -> String {
    format!(
        "@echo off\r\n\
         set \"D=%~dp0\"\r\n\
         set \"SYS=%SystemRoot%\\System32\"\r\n\
         setlocal enabledelayedexpansion\r\n\
         set \"LOG=%D%.uking-update.log\"\r\n\
         set \"EXE=%D%{exe_name}\"\r\n\
         set \"NEW=%D%{new_name}\"\r\n\
         set \"BAK=%D%.U-King.bak.exe\"\r\n\
         set \"FLAG=%D%.uking-updated\"\r\n\
         echo [%date% %time%] update start pid={pid} > \"%LOG%\"\r\n\
         echo [%date% %time%] dir=%D% exe={exe_name} new={new_name} >> \"%LOG%\"\r\n\
         if not exist \"%NEW%\" goto giveup\r\n\
         rem wait for the running U-King process to exit (up to ~30s)\r\n\
         set /a n=0\r\n\
         :wait\r\n\
         \"%SYS%\\tasklist.exe\" /FI \"PID eq {pid}\" 2>nul | \"%SYS%\\find.exe\" \"{pid}\" >nul\r\n\
         if errorlevel 1 goto rename_old\r\n\
         set /a n+=1\r\n\
         if !n! geq 60 goto rename_old\r\n\
         \"%SYS%\\ping.exe\" -n 1 -w 500 127.0.0.1 >nul\r\n\
         goto wait\r\n\
         :rename_old\r\n\
         echo [%date% %time%] backing up old exe >> \"%LOG%\"\r\n\
         del \"%BAK%\" >nul 2>nul\r\n\
         rem rename old exe as backup (retry: USB/slow disks release locks late, up to ~20s)\r\n\
         set /a m=0\r\n\
         :try_bak\r\n\
         move /Y \"%EXE%\" \"%BAK%\" >nul 2>nul\r\n\
         if not exist \"%EXE%\" goto put_new\r\n\
         set /a m+=1\r\n\
         if !m! geq 40 goto giveup\r\n\
         ping -n 1 -w 500 127.0.0.1 >nul\r\n\
         goto try_bak\r\n\
         :put_new\r\n\
         move /Y \"%NEW%\" \"%EXE%\" >nul 2>nul\r\n\
         if exist \"%EXE%\" goto ok\r\n\
         echo [%date% %time%] move new FAILED, rolling back >> \"%LOG%\"\r\n\
         move /Y \"%BAK%\" \"%EXE%\" >nul 2>nul\r\n\
         goto giveup\r\n\
         :ok\r\n\
         echo [%date% %time%] swap ok >> \"%LOG%\"\r\n\
         del \"%BAK%\" >nul 2>nul\r\n\
         type nul > \"%FLAG%\"\r\n\
         start \"\" \"%EXE%\"\r\n\
         goto done\r\n\
         :giveup\r\n\
         echo [%date% %time%] upgrade skipped/failed, launching existing exe >> \"%LOG%\"\r\n\
         del \"%NEW%\" >nul 2>nul\r\n\
         if exist \"%EXE%\" start \"\" \"%EXE%\"\r\n\
         :done\r\n\
         del \"%~f0\" >nul 2>nul\r\n",
        pid = pid,
        exe_name = exe_name,
        new_name = new_name,
    )
}

/// 把上一轮替换脚本留下的 `.uking-update.log` 收进统一日志，然后删掉原件。
///
/// 那份日志有三重「诊断收不到」：① 躺在**安装目录**而不是 `logs/`，而「技术支持」是按目录
/// 扫 `logs/*.log` 收的；② 是 cmd 用系统 ANSI 代码页写的，不是 UTF-8；③ 下次升级会被覆盖。
/// pc-*** 的教训就是升级失败全程无痕 —— 这一步把它搬到诊断看得见的地方。
/// 时间戳里的中文经 lossy 转换会糊掉，但真正要看的（`swap ok` / `move new FAILED` /
/// `upgrade skipped/failed`）全是 ASCII，照样读得出来。
/// 返回 `Some(失败原因)` 表示上一轮替换是**失败收场**的（脚本走了 `:giveup` 或回滚）。
#[cfg(windows)]
fn ingest_swap_log(dir: &Path) -> Option<String> {
    let p = dir.join(".uking-update.log");
    let bytes = std::fs::read(&p).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut failure = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        crate::ulog::write("update", &format!("[替换脚本] {line}"));
        // 这几句是脚本里写死的纯 ASCII 标记（正文不许有非 ASCII，见 spawn_swap 的长注释），
        // 所以哪怕日志是 cmd 用 GBK 写的、中文时间戳糊成乱码，这里照样认得出来。
        if line.contains("move new FAILED") {
            failure = Some("新版文件替换失败（旧版已自动回滚）".to_string());
        } else if line.contains("upgrade skipped/failed") && failure.is_none() {
            failure = Some("替换脚本没能换掉旧程序（多半是杀软锁住了文件或目录不可写）".to_string());
        }
    }
    let _ = std::fs::remove_file(&p);
    failure
}

/// 消费暂存的新版并派生替换脚本：STAGED_EXE 改名为 APPLY_EXE（消费一次），删 STAGED_META，
/// 再 spawn_swap。**消费**很关键：万一这次套用失败，重启回来的旧 exe 找不到 STAGED_META →
/// 不会立刻又套用一次 → 杜绝「失败-重启-再失败」死循环（后台线程稍后会重新下好再试）。
#[cfg(windows)]
fn consume_and_apply(dir: &Path, exe: &Path) -> Result<(), String> {
    let staged = dir.join(STAGED_EXE);
    let apply = dir.join(APPLY_EXE);
    // 先把「这次要升到哪一版」写进账本 —— 元数据马上就要被消费掉，而替换脚本要等本进程
    // 退出后才跑，它失败时能问的只剩这一笔（见 mark_update_pending）。
    if let Ok(txt) = std::fs::read_to_string(dir.join(STAGED_META)) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
                mark_update_pending(dir, ver);
            }
        }
    }
    let _ = std::fs::remove_file(&apply);
    // 刚落盘的新 exe 常被杀软实时扫描短暂锁住（客户机实测：rename 一次性失败后 STAGED_EXE
    // 原地不动，且用户「关窗口」只是缩托盘不重启，apply_staged_update 再也没机会重试，于是
    // 永久卡旧版还反复弹「有新版」）。重试几次，锁通常 <1s 内释放。
    let mut last_err = None;
    for attempt in 0..10 {
        match std::fs::rename(&staged, &apply) {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < 9 {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
        }
    }
    if let Some(e) = last_err {
        let msg = format!("暂存就绪失败: {e}");
        crate::ulog::write("update", &format!("{msg}（新包重命名 10 次都没成功，多半被杀软实时扫描锁住）"));
        return Err(msg);
    }
    let _ = std::fs::remove_file(dir.join(STAGED_META));
    spawn_swap(dir, exe, &apply)
}

/// 后台「静默下载」阶段：检查服务器是否有新版，有就把绿色 exe 悄悄下到暂存（不动正在运行的
/// exe，不弹任何东西）。下次裸启动时 apply_staged_update 会原子替换。绝不阻塞、绝不打扰。
#[cfg(windows)]
pub fn stage_pending_update() {
    let info = check_update();
    if !info.has_update {
        return;
    }
    let Ok((_exe, dir)) = exe_dir() else {
        return;
    };
    // 已暂存好同版本就跳过（download_new_exe 内部也幂等，这里先省一次联网下载）
    if looks_like_exe(&dir.join(STAGED_EXE), 1_000_000) {
        if let Ok(txt) = std::fs::read_to_string(dir.join(STAGED_META)) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if v.get("version").and_then(|x| x.as_str()) == Some(info.latest.as_str()) {
                    return;
                }
            }
        }
    }
    let _ = download_new_exe(&info.latest, &|_, _| {});
}

/// 启动时「套用」阶段（裸启动调用，GUI 起来前）：若暂存了**更新**且**校验通过**的 exe，
/// 原子替换并重启，返回 true（调用方随即 process::exit）。纯本地文件判断，不联网、不拖慢启动。
/// 没有可套用的更新 / 暂存损坏 / 非更新版本 → 清理残留并返回 false。
#[cfg(windows)]
pub fn apply_staged_update() -> bool {
    let Ok((exe, dir)) = exe_dir() else {
        return false;
    };
    // 清掉上次套用失败遗留的 APPLY_EXE（已被 :giveup 删过，这里兜底）
    let _ = std::fs::remove_file(dir.join(APPLY_EXE));
    // 上一轮替换脚本说了什么，先收进统一日志再说 —— 升级失败最需要的就是这几行。
    // ★ 失败还要记进账本：替换脚本是**在我们退出之后**跑的，它放弃时没有任何人能看见
    //（旧 exe 被重新拉起来，界面上又是一条「有新版」）。这正是「老是升不上去」的沉默半边，
    // 只有回到这里读它的日志才补得上。
    let swap_failed = ingest_swap_log(&dir);
    if let Some(target) = take_update_pending(&dir) {
        if let Some(why) = swap_failed {
            record_update_failure(&target, &why);
        }
    }

    let staged = dir.join(STAGED_EXE);
    let meta_path = dir.join(STAGED_META);
    let cleanup = || {
        let _ = std::fs::remove_file(&staged);
        let _ = std::fs::remove_file(&meta_path);
    };

    let Ok(txt) = std::fs::read_to_string(&meta_path) else {
        return false; // 没有暂存元数据 = 没有待套用的更新
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) else {
        cleanup();
        return false;
    };
    let staged_ver = v.get("version").and_then(|x| x.as_str()).unwrap_or("");
    // 不比当前版本新（含相等 = 上次已升级成功的残留）→ 清理，别套用
    if !semver_gt(staged_ver, env!("CARGO_PKG_VERSION")) {
        cleanup();
        return false;
    }
    // 暂存 exe 必须完好：大小与元数据精确一致 + PE 头正确；否则清掉等后台重下
    let size_ok = match v.get("size").and_then(|x| x.as_u64()) {
        Some(sz) => std::fs::metadata(&staged).map(|m| m.len() == sz).unwrap_or(false),
        None => true,
    };
    if !size_ok || !looks_like_exe(&staged, 1_000_000) {
        cleanup();
        return false;
    }
    match consume_and_apply(&dir, &exe) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&meta_path);
            false
        }
    }
}

pub(crate) fn reject_sidecar_self_update() -> Result<(), String> {
    // 不缓存角色：调用时读 instance 诊断真相源，调试副本不该替换主实例的 exe。
    if crate::instance::inspect().get("role").and_then(|v| v.as_str()) == Some("sidecar") {
        return Err("并行调试实例不执行自升级，请在主实例升级".into());
    }
    Ok(())
}

/// 确认弹窗和真正替换之间可能隔着几十秒下载。替换前再核对一次，绝不杀掉确认后新开的终端。
fn reject_new_terminal_sessions(ack_terminal_count: Option<usize>) -> Result<(), String> {
    // 即便老前端/脚本没有传 ack，也必须先证明会话表可读；锁中毒绝不能被当作 0 放行。
    let current = crate::term::term_active_count_checked()?;
    if let Some(ack) = ack_terminal_count.filter(|ack| current > *ack) {
        // 本次尚未写快照；上次升级留下的恢复卡只能由用户处理，不能被这次中止误删。
        return Err(format!(
            "升级期间新开了 {} 个终端，已中止以保护它们；请重新点击升级",
            current - ack
        ));
    }
    Ok(())
}

/// 替换脚本/安装程序已成功拉起后保持升级冻结，但给「脚本被安全软件拦住、当前进程又没退出」
/// 这一理论边角留一个 30 秒逃生口。正常情况下进程已退出，线程会随之消失；仍活着才解冻。
pub(crate) fn keep_terminal_update_until_exit_or_unfreeze(guard: &mut crate::term::TermUpdatingGuard) {
    guard.keep_until_process_exit();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(30));
        crate::term::term_updating_end();
    });
}

/// 软件内「一键升级」（手动按钮，Windows）：确保新版已下到暂存（后台下好则秒回）→ 消费 →
/// 派生原子替换脚本 → 退出当前进程让脚本接管。返回 Ok(()) 表示替换流程已启动（进程随即退出）。
#[cfg(windows)]
pub fn self_update(progress: &dyn Fn(&str, u8), ack_terminal_count: Option<usize>) -> Result<(), String> {
    reject_sidecar_self_update()?;
    // 从用户确认到替换完成整段冻结新 PTY；任何失败经 guard 的 Drop 解冻。
    let mut terminal_update = crate::term::TermUpdatingGuard::begin()?;
    let info = check_update(); // 拿服务器最新版本号作暂存标签
    // 自升级是「最容易无声失败、失败后果最重」的一条链路，却曾是唯一没接日志的模块：
    // 客户说「点了升级没反应」，机器上翻不到半行记录（pc-*** 实证）。每一步都留痕。
    crate::ulog::section("update", &format!("一键升级 {} → {}", info.current, info.latest));
    // 每一步失败都记进账本：连续失败到 1 次，界面就不再劝「再点一次」，改推覆盖安装。
    let fail = |e: &String| record_update_failure(&info.latest, e);
    let (exe, dir) = exe_dir().inspect_err(&fail)?;
    download_new_exe(&info.latest, progress).inspect_err(&fail)?;
    reject_new_terminal_sessions(ack_terminal_count)?;
    crate::ulog::write("update", "新版包已就位并校验通过，开始替换");
    progress("swap", 0);
    // lib.rs 成功后硬 exit(0)，不会经过退出钩子；必须在 consume 前同步落盘。
    let snapshot_written = crate::term::snapshot_sessions(&uking_home().join("term-snapshot.json"));
    let applied = consume_and_apply(&dir, &exe);
    if applied.is_ok() {
        // 已经拉起替换脚本，当前进程会马上硬退出；保留冻结直到退出。
        keep_terminal_update_until_exit_or_unfreeze(&mut terminal_update);
    } else if snapshot_written {
        // 只消费本次刚写出的快照；旧恢复卡不能因本次失败而被删掉。
        let _ = crate::term::term_snapshot_consume();
    }
    applied.inspect_err(&fail)
}

#[cfg(target_os = "macos")]
fn shell_quote_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn mac_app_bundle() -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败: {e}"))?;
    let app = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("app"))
        .ok_or("当前程序不在 .app 包内，无法原地升级")?
        .to_path_buf();
    let parent = app.parent().ok_or("无法定位 .app 所在目录")?.to_path_buf();
    Ok((app, parent))
}

#[cfg(target_os = "macos")]
fn ensure_parent_writable(parent: &Path) -> Result<(), String> {
    let probe = parent.join(format!(".uking-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| {
        format!(
            "应用所在目录不可写，无法静默替换 .app（{}）。请手动下载新版安装，或把 U-King.app 放到当前用户可写目录。",
            e
        )
    })?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

#[cfg(target_os = "macos")]
fn find_staged_app(root: &Path) -> Option<PathBuf> {
    let direct = root.join("U-King.app");
    if direct.join("Contents/MacOS/u-king-mini").exists() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(root).ok()? {
        let p = entry.ok()?.path();
        if p.extension().and_then(|x| x.to_str()) == Some("app")
            && p.join("Contents/MacOS/u-king-mini").exists()
        {
            return Some(p);
        }
    }
    None
}

/// 读 .app 包的 CFBundleShortVersionString（tauri 产物 Info.plist 是 XML，朴素解析够用）。
#[cfg(target_os = "macos")]
fn app_bundle_version(app: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let key = "<key>CFBundleShortVersionString</key>";
    let after = &plist[plist.find(key)? + key.len()..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    Some(after[start..end].trim().to_string())
}

#[cfg(target_os = "macos")]
fn download_new_mac_app(progress: &dyn Fn(&str, u8)) -> Result<(PathBuf, PathBuf), String> {
    let (_app, parent) = mac_app_bundle()?;
    ensure_parent_writable(&parent)?;
    let root = parent.join(format!(".U-King.update.{}", std::process::id()));
    let zip = root.join("U-King-Mac.zip");
    let unpack = root.join("unpack");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&unpack).map_err(|e| format!("创建升级暂存目录失败: {e}"))?;

    progress("download", 0);
    let mut last_err = String::new();
    for url in SELF_MAC_ZIP_URLS {
        let total = head_content_length(url).unwrap_or(30_000_000);
        let child = base_command("curl")
            .args([
                "-fL",
                "-sS",
                "-A",
                "Mozilla/5.0 U-King",
                "-m",
                // 更新包只有几 MB，300s 足够覆盖很慢的网络；原先 1800s(30分钟) 会让「地址被墙/
                // 卡住不通」的情况在切下一个镜像前枯等半小时，用户体感就是「点了升级没反应」
                // （对齐 Windows 版 download_new_exe 的 180s，Mac 包稍大给多一点余量）。
                "300",
                "-o",
                &zip.to_string_lossy(),
                url,
            ])
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                last_err = format!("启动下载失败: {e}");
                continue;
            }
        };
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    let cur = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
                    let pct = (cur.saturating_mul(100) / total.max(1)).min(95) as u8;
                    progress("download", pct);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                Err(_) => break None,
            }
        };
        if !matches!(status, Some(s) if s.success()) {
            last_err = "下载 Mac 更新包失败".into();
            let _ = std::fs::remove_file(&zip);
            continue;
        }

        let _ = std::fs::remove_dir_all(&unpack);
        std::fs::create_dir_all(&unpack).map_err(|e| format!("创建解压目录失败: {e}"))?;
        let unzip = base_command("ditto")
            .args(["-x", "-k", &zip.to_string_lossy(), &unpack.to_string_lossy()])
            .status()
            .map_err(|e| format!("解压更新包失败: {e}"))?;
        if !unzip.success() {
            last_err = "解压 Mac 更新包失败".into();
            continue;
        }
        if let Some(app) = find_staged_app(&unpack) {
            // 陈旧包防护：线上 Mac zip 可能落后 version.json（0.9.42 事故实锤：服务器版本号
            // 0.9.42 但四个镜像的 U-King-Mac.zip 里都还是 0.9.39 —— 客户点「一键升级」等于
            // 原地换同版，体感「升级了还是老版本」）。解包后核实真实版本，不比当前新就换下一个
            // 镜像；全部陈旧则明确报错，绝不静默空转。
            let staged = app_bundle_version(&app).unwrap_or_default();
            if semver_gt(&staged, env!("CARGO_PKG_VERSION")) {
                progress("download", 100);
                return Ok((root, app));
            }
            last_err = format!(
                "服务器上的 Mac 更新包还是 v{}（当前已是 v{}）—— 新版 Mac 包尚未上线，请稍后再试",
                if staged.is_empty() { "未知".to_string() } else { staged },
                env!("CARGO_PKG_VERSION")
            );
            continue;
        }
        last_err = "更新包里没有可用的 U-King.app".into();
    }
    let _ = std::fs::remove_dir_all(&root);
    Err(if last_err.is_empty() { "下载 Mac 更新包失败".into() } else { last_err })
}

#[cfg(target_os = "macos")]
fn spawn_mac_swap(app: &Path, root: &Path, staged_app: &Path) -> Result<(), String> {
    let parent = app.parent().ok_or("无法定位 .app 所在目录")?;
    let script = parent.join(".uking-update-mac.sh");
    let log = parent.join(".uking-update-mac.log");
    let bak = parent.join(".U-King.previous.app");
    let pid = std::process::id();
    let body = format!(
        r#"#!/bin/bash
set -u
PID={pid}
APP={app}
NEW={new}
BAK={bak}
ROOT={root}
LOG={log}
echo "[$(date)] mac update start pid=$PID" > "$LOG"
n=0
while kill -0 "$PID" >/dev/null 2>&1; do
  n=$((n+1))
  if [ "$n" -ge 80 ]; then break; fi
  sleep 0.5
done
if [ ! -d "$NEW" ]; then
  echo "[$(date)] missing staged app" >> "$LOG"
  open "$APP" >/dev/null 2>&1 || true
  exit 1
fi
rm -rf "$BAK"
# 刚解压好的 .app 可能被 Spotlight/Gatekeeper 短暂占用；重试几次再判失败（对齐 Windows 版
# 同款教训：一次性 mv 失败就放弃 = 永久卡旧版）。
try_mv() {{
  n=0
  while [ "$n" -lt 10 ]; do
    if mv "$1" "$2" 2>/dev/null; then return 0; fi
    n=$((n+1))
    sleep 0.3
  done
  return 1
}}
if ! try_mv "$APP" "$BAK"; then
  echo "[$(date)] backup old app failed" >> "$LOG"
  open "$APP" >/dev/null 2>&1 || true
  exit 1
fi
if ! try_mv "$NEW" "$APP"; then
  echo "[$(date)] move new app failed, rollback" >> "$LOG"
  rm -rf "$APP"
  mv "$BAK" "$APP" >/dev/null 2>&1 || true
  open "$APP" >/dev/null 2>&1 || true
  exit 1
fi
xattr -rd com.apple.quarantine "$APP" >/dev/null 2>&1 || true
touch "$APP/Contents/MacOS/.uking-updated" >/dev/null 2>&1 || true
open "$APP" >/dev/null 2>&1 || true
rm -rf "$BAK" "$ROOT"
rm -f "$0"
"#,
        pid = pid,
        app = shell_quote_path(app),
        new = shell_quote_path(staged_app),
        bak = shell_quote_path(&bak),
        root = shell_quote_path(root),
        log = shell_quote_path(&log),
    );
    std::fs::write(&script, body).map_err(|e| format!("写 Mac 升级脚本失败: {e}"))?;
    let _ = base_command("chmod").args(["700", &script.to_string_lossy()]).status();
    base_command("sh")
        .arg(&script)
        .spawn()
        .map_err(|e| format!("启动 Mac 升级脚本失败: {e}"))?;
    Ok(())
}

/// macOS 应用内一键升级：下载 U-King-Mac.zip → 解包 U-King.app → 退出后替换 .app 并重启。
#[cfg(target_os = "macos")]
pub fn self_update(progress: &dyn Fn(&str, u8), ack_terminal_count: Option<usize>) -> Result<(), String> {
    reject_sidecar_self_update()?;
    let mut terminal_update = crate::term::TermUpdatingGuard::begin()?;
    let info = check_update();
    let fail = |e: &String| record_update_failure(&info.latest, e);
    let (app, _parent) = mac_app_bundle().inspect_err(&fail)?;
    let (root, staged_app) = download_new_mac_app(progress).inspect_err(&fail)?;
    reject_new_terminal_sessions(ack_terminal_count)?;
    progress("swap", 0);
    let snapshot_written = crate::term::snapshot_sessions(&uking_home().join("term-snapshot.json"));
    let applied = spawn_mac_swap(&app, &root, &staged_app);
    if applied.is_ok() {
        keep_terminal_update_until_exit_or_unfreeze(&mut terminal_update);
    } else if snapshot_written {
        let _ = crate::term::term_snapshot_consume();
    }
    applied.inspect_err(&fail)
}

/// 其它非 Windows/macOS 平台暂不支持软件内自升级，引导去下载页。
#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn self_update(_progress: &dyn Fn(&str, u8), _ack_terminal_count: Option<usize>) -> Result<(), String> {
    Err("当前平台暂不支持应用内升级，请到下载页获取新版".into())
}

/// 安装包地址（国内优先）。**覆盖安装**这条兜底路要的是 NSIS 安装包（U-King-Setup.exe），
/// 不是自升级用的绿色单文件 exe —— 后者正是在这台机器上换不动的那个东西。
#[cfg(windows)]
const SETUP_URLS: &[&str] = &[
    "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/U-King-Setup.exe",
    "https://u-claw.org.cn/download/U-King-Setup.exe",
    "https://www.u-king.org/download/U-King-Setup.exe",
];

/// 下载目录（拿不到就退回临时目录）。放「下载」里而不是藏在 temp：
/// 万一自动打开安装程序这一步也被拦（有些杀软会拦子进程），用户还能自己去下载夹双击。
#[cfg(windows)]
fn downloads_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let d = PathBuf::from(&home).join("Downloads");
        if d.is_dir() {
            return d;
        }
    }
    std::env::temp_dir()
}

/// ★ 自动升级走不通时的兜底：**下载官网安装包并打开它**，走一次覆盖安装。
///
/// 为什么要有这条路（而不是让用户「再点一次一键升级」）：自动替换的失败源基本都不是暂时性的
/// —— 杀软盯着 exe 目录、安装目录不可写、装在含非 ASCII 的路径下、替换脚本被拦 ——
/// 同一条路重试一百次还是同样的结果，而覆盖安装是**另一条**路：安装程序自己有权限模型、
/// 有杀软信任度、也不依赖我们那段替换脚本。**配置不会丢**：安装包只覆盖程序本体，
/// `~/.uking` / `~/.claude` / `~/.codex` 一个都不碰。
///
/// 返回安装包落盘路径。调用方拿到 Ok 之后应当退出本进程（安装程序要替换正在运行的 exe）。
#[cfg(windows)]
pub fn download_installer(progress: &dyn Fn(&str, u8)) -> Result<PathBuf, String> {
    let dir = downloads_dir();
    let dst = dir.join("U-King-Setup.exe");
    let tmp = dir.join(".U-King-Setup.part");
    let _ = std::fs::remove_file(&tmp);
    crate::ulog::section("update", "自动升级失败后走覆盖安装：下载官网安装包");

    progress("download", 0);
    let mut last_err = String::new();
    for url in SETUP_URLS {
        let total = head_content_length(url).unwrap_or(4_500_000);
        let child = base_command("curl")
            .args(["-sSL", "-A", "Mozilla/5.0 U-King", "-m", "300", "-o", &tmp.to_string_lossy(), url])
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                last_err = format!("启动下载失败: {e}");
                continue;
            }
        };
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    let cur = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
                    progress("download", (cur.saturating_mul(100) / total.max(1)).min(99) as u8);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Err(_) => break None,
            }
        };
        // 和自升级同一条校验口径：够大 + PE 头是 MZ。下到错误页/半截包绝不往下走 ——
        // 「双击安装包没反应」比升级失败更难查。
        if matches!(status, Some(s) if s.success()) && looks_like_exe(&tmp, 1_000_000) {
            let _ = std::fs::remove_file(&dst);
            std::fs::rename(&tmp, &dst).map_err(|e| format!("安装包落盘失败: {e}"))?;
            progress("download", 100);
            crate::ulog::write("update", &format!("安装包已下载：{}", dst.display()));
            return Ok(dst);
        }
        last_err = "安装包下载失败或文件异常（疑似错误页 / 截断）".into();
        let _ = std::fs::remove_file(&tmp);
    }
    let _ = std::fs::remove_file(&tmp);
    crate::ulog::write("update", &format!("安装包下载失败：{last_err}"));
    Err(if last_err.is_empty() { "安装包下载失败".into() } else { last_err })
}

/// 打开安装程序（不静默：这一步就是要让用户看见「正在重新安装」，一路下一步）。
#[cfg(windows)]
pub fn launch_installer(setup: &Path) -> Result<(), String> {
    base_command(&setup.to_string_lossy())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开安装程序失败: {e}（安装包在 {}，可手动双击）", setup.display()))
}

/// HEAD 取 Content-Length，作下载进度的分母；重定向链里取最终那个有效值，拿不到返回 None。
#[cfg(any(windows, target_os = "macos"))]
fn head_content_length(url: &str) -> Option<u64> {
    let out = curl(&["-sIL", "-A", "Mozilla/5.0 U-King", "-m", "12", url]).ok()?;
    let mut last = None;
    for line in out.lines() {
        let low = line.to_ascii_lowercase();
        if let Some(v) = low.trim().strip_prefix("content-length:") {
            if let Ok(n) = v.trim().parse::<u64>() {
                if n > 0 {
                    last = Some(n);
                }
            }
        }
    }
    last
}

/// 取出并清除「自升级成功」标记（替换脚本在覆盖成功后于 exe 同目录建 `.uking-updated`）。
/// 返回 true ⇒ 本次启动是自升级替换后的首启，前端据此弹「已升级到新版」。顺手清理升级残留。
pub fn take_update_flag() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = exe.parent() else {
        return false;
    };
    let flag = dir.join(".uking-updated");
    if flag.exists() {
        let _ = std::fs::remove_file(&flag);
        // 升成功了，失败账本一并抹掉 —— 否则下次有新版时横幅会顶着上次的旧账，
        // 一上来就劝人「去下安装包」，而这台机器明明刚刚自动升成功过。
        clear_update_failures();
        // 清理本轮自升级的全部暂存残留（成功升级后首启时一次扫干净）
        for f in [
            ".uking-update.bat",
            ".uking-update.log",
            ".U-King.new.exe",
            ".U-King.new.json",
            ".U-King.apply.exe",
            ".U-King.bak.exe",
            ".U-King.dl.tmp",
        ] {
            let _ = std::fs::remove_file(dir.join(f));
        }
        return true;
    }
    false
}

fn fetch_remote_skill() -> Option<Skill> {
    for url in SKILL_URLS {
        if let Ok(out) = curl(&["-sL", "-m", "6", url]) {
            if let Ok(s) = serde_json::from_str::<Skill>(&out) {
                return Some(s);
            }
        }
    }
    None
}

// ============================================================
// 环境探测
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct CmdProbe {
    pub found: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackDetect {
    pub node: CmdProbe,
    pub npm: CmdProbe,
    pub claude: CmdProbe,
    pub codex: CmdProbe,
    pub git: CmdProbe,
    pub claude_desktop: bool,
    /// Codex 桌面版（微软商店 MSIX / Mac .app）
    pub codex_app: bool,
    /// 便携 Node 是否已装到 ~/.uking/runtime
    pub portable_node: bool,
    /// 系统代理（开启时 = "host:port"）。claude/codex 等工具会走系统代理，
    /// 但 U-King 的 curl 实测不走 —— 代理节点失效时会出现「实测全绿、工具连不上」
    /// 的误导局面（Mac mini 实测踩过：mihomo 死节点劫持 api.u-claw.org 返回 503）。
    /// 探测出来让前端给用户提示。
    pub system_proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HermesBrowserStatus {
    pub hermes_installed: bool,
    pub browser_ready: bool,
    pub agent_browser: CmdProbe,
    pub config_dir: String,
    pub cloud_provider: Option<String>,
    pub browser_use_key: bool,
    pub browserbase_key: bool,
    pub browserbase_project: bool,
    pub firecrawl_key: bool,
    pub cdp_url: bool,
    pub message: String,
    pub suggestions: Vec<String>,
}

/// 跑 `<cmd> --version` 探测；带上便携 Node 的 PATH。
fn probe(cmd: &str) -> CmdProbe {
    let line = format!("{cmd} --version");
    match run_capture(&line, portable_node_dir().as_deref()) {
        Ok((0, out)) => CmdProbe {
            found: true,
            version: Some(out.lines().next().unwrap_or("").trim().to_string()),
        },
        _ => CmdProbe {
            found: false,
            version: None,
        },
    }
}

/// 统一的「命令是否已装」判据。两路兜底，任一命中即判已装（**按代价从小到大试**）：
///  ② 在 search_paths 各目录里存在可执行文件（cmd / cmd.cmd / cmd.exe / cmd.bat / cmd.ps1）—— 先试，几乎不花时间
///  ① `<cmd> --version` 退出 0（注入便携 PATH，与 detect_stack 同口径）—— ② 没命中才试，要起进程
///
/// 为什么要 ②：有些 CLI（如 openclaw）**不支持 `--version`** 或 `--version` 会挂/非零退出，
/// 只靠 ① 会把「明明装了、gateway 都能跑」的工具误判成未装（客户实测：openclaw 灰显、
/// 点了还让装）。② 直接看文件在不在，绕开 `--version` 的兼容性坑。
///
/// 为什么要 ①：工具装在 search_paths 之外（系统 PATH 上的别处）时，只有它能发现。
///
/// “文件不存在”后再起 `<cmd> --version` 是昂贵兜底，尤其 Python CLI 启动一次可达 2~3 秒。
/// AI 设置页、工具中心、侧栏会在同一轮分别问一次，旧实现把同一批缺失命令重复探测，
/// 页面每进一次就多等约 4 秒。负结果短缓存 60 秒；每次仍先查文件，所以 U-King 刚装好的
/// CLI 会立即命中，不会被旧的 false 挡住。缓存不持久化，重启后自然重新确认。
static TOOL_PROBE_MISSES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
> = std::sync::OnceLock::new();

pub fn tool_installed(cmd: &str) -> bool {
    // ★ 顺序是 ②→① 而不是 ①→②，这不是风格问题，是性能问题（测试报告 #011）。
    //
    // 结果是 `① OR ②`，OR 可交换 —— 两种顺序**答案完全一样**，但代价差几个数量级：
    //   ② 看文件在不在：几乎 0ms
    //   ① 起进程跑 `--version`：本机实测 claude 272ms / codex 303ms / openclaw 316ms /
    //      **hermes 2331ms**，四个加起来约 3.2 秒
    // 而绝大多数调用都发生在「工具确实装着」的机器上 —— 那正是 ② 一定命中的情况。
    // 先跑 ① 等于每次都花 3 秒去确认一件看一眼文件就知道的事。
    //
    // ① 仍然保留兜底：工具装在 search_paths 之外（比如系统 PATH 上的别处）时只有它能发现。
    let exts: &[&str] = if cfg!(windows) {
        &["", ".cmd", ".exe", ".bat", ".ps1"]
    } else {
        &[""]
    };
    for dir in search_paths(portable_node_dir().as_deref()) {
        for ext in exts {
            if dir.join(format!("{cmd}{ext}")).exists() {
                return true;
            }
        }
    }
    let misses = TOOL_PROBE_MISSES
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if misses
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(cmd)
        .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(60))
    {
        return false;
    }
    let found = probe(cmd).found;
    let mut cache = misses.lock().unwrap_or_else(|e| e.into_inner());
    if found {
        cache.remove(cmd);
    } else {
        cache.insert(cmd.to_string(), std::time::Instant::now());
    }
    found
}

/// 浏览器能力共用的 Chrome 探测真相源。Chrome 作为 GUI 应用通常不在 PATH，
/// 因而不能只复用 `tool_installed("chrome")`；浏览器面板和厨具工具箱都应问这里，
/// 避免各自维护 Program Files / macOS app bundle 的路径表。
pub(crate) fn chrome_installed() -> bool {
    #[cfg(windows)]
    {
        ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(|var| std::env::var(var).ok())
            .map(|base| std::path::PathBuf::from(base).join("Google/Chrome/Application/chrome.exe"))
            .any(|path| path.is_file())
    }
    #[cfg(target_os = "macos")]
    {
        [
            std::path::PathBuf::from("/Applications/Google Chrome.app"),
            user_home_dir().join("Applications/Google Chrome.app"),
        ]
        .into_iter()
        .any(|path| path.is_dir())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        tool_installed("chrome")
    }
}

/// 让其它模块按**安装器同一套 PATH 解析规则**运行一个只读 CLI，并拿到合并输出。
///
/// 不能在调用方裸 `Command::new`：双击启动 U-King 时，进程 PATH 经常没有 npm 全局目录，
/// 会出现安装器判定「已装」、体检报告却说「命令不存在」的自相矛盾。
pub fn run_tool_capture(cmdline: &str) -> Result<(i32, String), String> {
    run_capture(cmdline, portable_node_dir().as_deref())
}

pub fn detect_stack() -> StackDetect {
    StackDetect {
        node: probe("node"),
        npm: probe("npm"),
        claude: probe("claude"),
        codex: probe("codex"),
        git: probe("git"),
        claude_desktop: claude_desktop_installed(),
        codex_app: codex_app_installed(),
        portable_node: portable_node_dir().is_some(),
        system_proxy: system_proxy(),
    }
}

pub fn hermes_browser_status() -> HermesBrowserStatus {
    let hermes_installed = tool_installed("hermes");
    let _ = ensure_hermes_utils_shim();
    let agent_browser = probe("agent-browser");
    let config_dir = hermes_config_dir();
    let env_file = read_key_values(&config_dir.join(".env"));
    let cloud_provider = read_hermes_browser_provider(&config_dir.join("config.yaml"));

    let browser_use_key = env_present("BROWSER_USE_API_KEY", &env_file);
    let browserbase_key = env_present("BROWSERBASE_API_KEY", &env_file);
    let browserbase_project = env_present("BROWSERBASE_PROJECT_ID", &env_file);
    let firecrawl_key = env_present("FIRECRAWL_API_KEY", &env_file);
    let cdp_url = env_present("BROWSER_CDP_URL", &env_file) || env_present("CAMOFOX_URL", &env_file);

    let cloud_ready = match cloud_provider.as_deref().map(|s| s.to_ascii_lowercase()) {
        Some(p) if p == "browser-use" || p == "browser_use" => browser_use_key,
        Some(p) if p == "browserbase" => browserbase_key && browserbase_project,
        Some(p) if p == "firecrawl" => firecrawl_key,
        Some(p) if p == "local" => false,
        Some(_) | None => browser_use_key || (browserbase_key && browserbase_project) || firecrawl_key,
    };
    let browser_ready = agent_browser.found || cdp_url || cloud_ready;

    let (message, suggestions) = if !hermes_installed {
        (
            "Hermes 还没有安装，先点装 AI 里的 Hermes 安装。".to_string(),
            vec!["安装后 U-King 会自动写虾盘云模型配置，普通聊天就能直接用。".to_string()],
        )
    } else if agent_browser.found {
        (
            "浏览器接管已就绪：检测到本地 agent-browser。".to_string(),
            vec![
                "Hermes 可以尝试打开网页、截图、读取页面内容。".to_string(),
                "需要登录态的网站仍建议用 Codex 专区或 ClawX 接管当前浏览器。".to_string(),
            ],
        )
    } else if cdp_url {
        (
            "浏览器接管已就绪：检测到 CDP/Camofox 浏览器地址。".to_string(),
            vec!["如果网页任务失败，先确认对应浏览器进程仍在运行。".to_string()],
        )
    } else if cloud_ready {
        (
            "浏览器接管已就绪：检测到云浏览器 Key。".to_string(),
            vec!["云浏览器适合公开网页；已登录网站通常还要单独处理登录态。".to_string()],
        )
    } else {
        (
            "Hermes 聊天和模型调用可用；浏览器接管未配置。".to_string(),
            vec![
                "要让 Hermes 自己打开网页/截图，需要安装 agent-browser 并初始化浏览器，或配置 Browser Use/Browserbase。".to_string(),
                "抖店、后台、已登录网页这类任务，优先用 Codex 专区或 ClawX。".to_string(),
            ],
        )
    };

    HermesBrowserStatus {
        hermes_installed,
        browser_ready,
        agent_browser,
        config_dir: config_dir.display().to_string(),
        cloud_provider,
        browser_use_key,
        browserbase_key,
        browserbase_project,
        firecrawl_key,
        cdp_url,
        message,
        suggestions,
    }
}

/// Hermes 的配置目录（**全仓唯一真相源**，providers.rs / skillpack.rs 都以此为准）。
///
/// 严格照 Hermes 自己的解析顺序抄，一个字都不许加：
/// ```python
/// # hermes_constants.py::get_hermes_home()  —— Hermes 自称的 single source of truth
/// val = os.environ.get("HERMES_HOME", "").strip()
/// return Path(val) if val else Path.home() / ".hermes"
/// ```
/// 即 **`HERMES_HOME` → `~/.hermes`，没有第三个候选**。
///
/// 🔴 **别再把 `%LOCALAPPDATA%\hermes` 加回来**（pc-*** / 另一台客户机，2026-08-05）：
/// 那是 Hermes 的**安装目录**（venv、源码、日志都在里面，所以「看着很像配置目录」，
/// 而且装过 Hermes 的机器上它必然存在），Hermes 运行时**从不读它**。老代码让它优先，
/// 于是「一键配好全部 AI」把虾盘云端点+Key 写进了一个永远不会被读的地方：
/// Hermes 继续用 `~/.hermes` 里的旧配置 —— 模型名是我们写的 `deepseek-v4-flash`、
/// 端点却是 DeepSeek 官方 → 拿虾盘云的模型名去打官方 → **HTTP 404**，且代码级必现
/// （两台客户机同一报错）。技能包同样落空：`~/.hermes/skills` 下一个 uking-* 都没有。
///
/// 当年那句「实测 hermes CLI 实际读这里」是被**开发机 shell 里的 `HERMES_HOME` 残留**
/// 骗的（指向 `Y:\compare-upstream\hermes-home`）—— 跟 skillpack.rs 那次「13 个包全被认」
/// 同源。判别家工具的家目录，只认它自己源码里的解析顺序，不认我们的观察。
/// 用户家目录，**认 `UKING_TEST_HOME` 沙箱**。公共层的唯一口径。
///
/// 🔴 **为什么非要有这一份**：这个事实（「沙箱下家在哪」）原本在十几个模块里各写各的，
/// 而 `identity.rs` 是**唯一一份都没写的** —— 于是它的 `llms.txt` / `identity.json` /
/// `secrets.json` 全部逃出沙箱。2026-08-08 我本想在沙箱里验一下说明书，
/// 结果 `runtime.identity.publish` 直接写了真实的 `~/.uking/llms.txt`。
///
/// 更要命的是同一个 `home_dir()` 还喂着 `runtime.identity.link` —— 那个动作往
/// **别家 AI 的记忆文件**（`~/.claude/CLAUDE.md` 等）里写。沙箱兜不住它，
/// 意味着一次「隔离测试」就能改到用户真实的 CLAUDE.md，
/// 而影核 `U-KING-PILOT.md` 的安全边界明写着试点必须跑在隔离数据上。
///
/// 宪法第 8 条在这儿现形：同一事实存在几份就会漂移几份，漏掉的那一份不会报错，
/// 只会安静地写到真实机器上。新代码一律用这个，别再复制第 N 份。
pub fn user_home_dir() -> PathBuf {
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.trim().is_empty() {
            return PathBuf::from(t);
        }
    }
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn hermes_config_dir() -> PathBuf {
    // 沙箱优先（开发机校验时不碰真实配置）。这条是我们自己的，不属于 Hermes 的解析顺序。
    if let Ok(t) = std::env::var("UKING_TEST_HOME") {
        if !t.trim().is_empty() {
            return PathBuf::from(t).join(".hermes");
        }
    }
    // ① `HERMES_HOME` —— 跟 Hermes 自己的第一档一致。
    if let Ok(p) = std::env::var("HERMES_HOME") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    // ② ★ **问 Hermes 自己要**（结果缓存，见 `probed_hermes_home`）。
    //
    // 🔴 这一档是被同一个坑正反各踩一次逼出来的（2026-08-05，一天之内）：
    //   - 先是硬编码成「没设 HERMES_HOME 就用 `%LOCALAPPDATA%\hermes`」；
    //   - 然后 pc-*** 的 404 让人改成「Hermes 只认 `~/.hermes`，压根没有 LOCALAPPDATA」。
    // 两次都是**照着某一台机器的观察去猜一个全局常量**。翻它自己的
    // `hermes_constants._get_platform_default_hermes_home()` 才看清：
    // **`win32` 走 `%LOCALAPPDATA%\hermes`，`~/.hermes` 是非 Windows 的默认** ——
    // 也就是说两次硬编码各对一半，而客户全在 Windows 上。
    //
    // 所以别再挑边：能问就问，问不到才按平台回落。这样上游哪天改了默认，我们跟着变。
    if let Some(p) = probed_hermes_home() {
        return p;
    }
    // ③ 问不到（没装 Hermes / 找不到解释器 / 探测超时）才按平台默认回落，
    //    口径与 `_get_platform_default_hermes_home()` 逐字对齐。
    platform_default_hermes_home()
}

/// Hermes 的**平台默认**家目录，口径抄自它自己的 `_get_platform_default_hermes_home()`：
/// `win32` → `%LOCALAPPDATA%\hermes`（LOCALAPPDATA 空则 `~/AppData/Local/hermes`）；其余 → `~/.hermes`。
/// **别按「我们这台机器上看到的样子」改这个函数** —— 要改先去读上游那一段。
fn platform_default_hermes_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    #[cfg(windows)]
    {
        match std::env::var("LOCALAPPDATA") {
            Ok(la) if !la.trim().is_empty() => PathBuf::from(la).join("hermes"),
            _ => Path::new(&home).join("AppData").join("Local").join("hermes"),
        }
    }
    #[cfg(not(windows))]
    {
        Path::new(&home).join(".hermes")
    }
}

/// 探测结果缓存。**只缓存「问出来的那一档」**，不缓存整个 `hermes_config_dir()` ——
/// 沙箱（`UKING_TEST_HOME`）和 `HERMES_HOME` 是每次现读的环境变量，`cargo test` 里多个用例
/// 会各自设不同的值，整体缓存会让它们互相串（本项目在「测试用例抢 `UKING_TEST_HOME`」上栽过）。
static PROBED_HERMES_HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

fn probed_hermes_home() -> Option<PathBuf> {
    PROBED_HERMES_HOME.get_or_init(probe_hermes_home).clone()
}

/// 真去跑一次 Hermes 自带的解析函数。拿不到就 `None`（调用方回落平台默认），**绝不 panic、绝不卡住**。
///
/// 为什么用它的 Python 而不是 `hermes` CLI：`hermes --version` 本机实测就要 2.3 秒（见
/// `tool_installed` 的注释），而且没有哪条子命令会单独打印家目录；`python -c` 只导一个常量模块，
/// 快一个数量级，拿到的还是**同一个** single-source-of-truth 函数。
fn probe_hermes_home() -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    let py_names: &[&str] = if cfg!(windows) { &["python.exe"] } else { &["python3", "python"] };
    // 只找**和 hermes 装在一起**的解释器：系统 PATH 上随便一个 python 大概率没装 hermes_agent，
    // 白起一个进程还可能把「没装」误报成「探测失败」。
    for dir in search_paths(portable_node_dir().as_deref()) {
        if !exts.iter().any(|e| dir.join(format!("hermes{e}")).exists()) {
            continue;
        }
        // venv 布局：`<env>/Scripts/{hermes,python}.exe`；便携 python 布局：`<env>/Scripts/hermes` + `<env>/python.exe`。
        let mut cands: Vec<PathBuf> = Vec::new();
        for n in py_names {
            cands.push(dir.join(n));
            if let Some(par) = dir.parent() {
                cands.push(par.join(n));
                cands.push(par.join("bin").join(n));
            }
        }
        for py in cands {
            if !py.exists() {
                continue;
            }
            if let Some(p) = ask_hermes_for_home(&py) {
                return Some(p);
            }
        }
    }
    None
}

/// 跑 `<python> -c "...get_hermes_home()"`，**自带超时**（宪法第 9 条：凡会卡的一律超时）。
/// `capture()` 走的是 `output()`，没有超时 —— 这条挂在装机/启动路径上，卡住就是客户看着白屏。
fn ask_hermes_for_home(py: &Path) -> Option<PathBuf> {
    // 🔴 必须 `sys.stdout.buffer.write(...encode('utf-8'))`，不能用 `sys.stdout.write(...)`。
    // Windows 上 Python 往**管道**写 stdout 时用的是本地代码页（中文机器 cp936/GBK），不是 UTF-8。
    // 下面拿到的字节我们按 UTF-8 解，中文用户名一进来就烂：pc-*** 的 `C:\Users\demo（无密码）`
    // 被解成 （中文字节被本地代码页错解成乱码），路径坏掉 → 建目录报「拒绝访问 (os error 5)」→
    // `driver.apply_everywhere` 静默跳过 Hermes（还退出码 0）。同一份诊断里 `legacy.dir` 是对的，
    // 因为它不走这条探测 —— 两个字段一对，就知道坏的是解码不是路径本身。
    // 让它按字节吐 UTF-8，解码这一步就不再依赖客户机的代码页。
    const CODE: &str = "import sys,hermes_constants as h;\
sys.stdout.buffer.write(str(h.get_hermes_home()).encode('utf-8'))";
    let mut c = base_command(py.to_str()?);
    c.args(["-c", CODE])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = c.spawn().ok()?;
    // 轮询而不是 `output()`：超时能真把它杀掉。输出只有一行，远不到管道缓冲上限，不会死锁。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None; // 没装 hermes_agent 的解释器会 ImportError —— 那是「问错人」，不是答案
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let p = PathBuf::from(&s);
    // 只认绝对路径：相对路径说明它自己也没算明白，宁可回落平台默认。
    (!s.is_empty() && p.is_absolute()).then_some(p)
}

/// Hermes 的**旧落点**：0.9.90 及以前 U-King 误判的那个目录（Windows 上的安装目录）。
/// 只用于「把写错地方的配置搬回来」的一次性迁移，**绝不能再当写入目标**。
/// 非 Windows 恒为 None（Mac/Linux 上从来没有过这条错误分支）。
pub fn hermes_legacy_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        // 沙箱下也要能造出旧落点，否则迁移逻辑没法在开发机上验。
        if let Ok(t) = std::env::var("UKING_TEST_HOME") {
            if !t.trim().is_empty() {
                let p = PathBuf::from(t).join("LocalAppData").join("hermes");
                return p.is_dir().then_some(p);
            }
        }
        let local = std::env::var("LOCALAPPDATA").ok()?;
        if local.trim().is_empty() {
            return None;
        }
        let p = PathBuf::from(local).join("hermes");
        p.is_dir().then_some(p)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn read_key_values(path: &Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        out.insert(k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string());
    }
    out
}

fn env_present(key: &str, file_env: &std::collections::BTreeMap<String, String>) -> bool {
    let from_file = file_env.get(key).map(|v| !v.trim().is_empty()).unwrap_or(false);
    from_file || std::env::var(key).map(|v| !v.trim().is_empty()).unwrap_or(false)
}

fn read_hermes_browser_provider(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_browser = false;
    let mut browser_indent = 0usize;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let indent = raw.len().saturating_sub(raw.trim_start().len());
        if line == "browser:" {
            in_browser = true;
            browser_indent = indent;
            continue;
        }
        if in_browser && indent <= browser_indent {
            in_browser = false;
        }
        if in_browser && line.starts_with("cloud_provider:") {
            return line
                .split_once(':')
                .map(|(_, v)| v.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|v| !v.is_empty());
        }
    }
    None
}

/// Hermes 兼容性自愈：hermes-agent 0.16+ 自带完整 utils.py【模块】（含 fast_safe_load /
/// atomic_replace / atomic_yaml_write / is_truthy_value，随 wheel 发货）。早期 U-King 会写一个
/// `utils/` shim【包】，Python 里同名【包】优先于【模块】→ 盖过 hermes 自带 utils.py → hermes
/// 一 import 就崩（ImportError: cannot import name 'fast_safe_load' from 'utils'）。
///
/// 改成 heal：只删这个挡路的残缺 shim 包（当 hermes 真身 utils.py 也在时），绝不再建 shim。
/// **必须在 Rust 进程内做**——skill 的 run 步骤无论用 `python -c "..."` 还是 PowerShell `$var`，
/// 经 `cmd /C` 都会被引号/变量踩踏（干净 Windows 真机实测：python -c 报 SyntaxError；PowerShell
/// 的 `$base` 被吃成空、静默失效、shim 删不掉）。进程内 std::fs 无这些坑，稳。
fn ensure_hermes_utils_shim() -> Result<(), String> {
    let Some(py) = portable_python_exe() else {
        return Ok(());
    };
    let Some(root) = py.parent() else {
        return Ok(());
    };
    let site = root.join("Lib").join("site-packages");
    let shim_pkg = site.join("utils");
    let shim_init = shim_pkg.join("__init__.py");
    let real_module = site.join("utils.py");
    // 只在「shim 包」与「hermes 真身 utils.py 模块」同时存在时删包：真身在才敢删（防误删），
    // 删完 Python 自动改用真身 utils.py。真身缺席时不动（hermes 还没装，或极少数以包形式发货）。
    if shim_init.exists() && real_module.exists() {
        // 若这个 utils 包本身就完整（极少见：真身以包形式装的），别误删。
        let init_text = std::fs::read_to_string(&shim_init).unwrap_or_default();
        let pkg_is_complete =
            init_text.contains("def fast_safe_load(") && init_text.contains("def atomic_replace(");
        if !pkg_is_complete {
            std::fs::remove_dir_all(&shim_pkg)
                .map_err(|e| format!("删除挡路的 Hermes utils shim 包失败: {e}"))?;
        }
    }
    Ok(())
}

/// 临时目录所在盘的剩余空间（MB）。失败返回 None（不阻塞安装）。
#[cfg(windows)]
fn temp_disk_free_mb() -> Option<u64> {
    // 盘符取自 temp_dir（下载落这里），PowerShell 读 Free 字节数（fsutil 输出受语言影响不好解析）
    let drive = std::env::temp_dir()
        .to_string_lossy()
        .chars()
        .next()?
        .to_ascii_uppercase();
    let ps = format!("(Get-PSDrive -Name {drive} -ErrorAction SilentlyContinue).Free");
    let (code, out) = run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", &ps], None).ok()?;
    if code != 0 {
        return None;
    }
    out.trim().parse::<u64>().ok().map(|b| b / 1024 / 1024)
}

#[cfg(not(windows))]
fn temp_disk_free_mb() -> Option<u64> {
    // df -k 输出第 2 行第 4 列 = 可用 KB
    let tmp = std::env::temp_dir();
    let (code, out) = run_capture_raw("df", &["-k", &tmp.display().to_string()], None).ok()?;
    if code != 0 {
        return None;
    }
    out.lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse::<u64>()
        .ok()
        .map(|kb| kb / 1024)
}

/// 读一个注册表值。**公共层唯一实现** —— envfp（环境指纹）和 term（ConPTY 版本号）都调它。
///
/// 解析口径来自 envfp 踩过的坑：**值里可能带空格**，所以不能取最后一个 token
/// （那样 `ProcessorNameString` 会被截成 "i9-12900H"）。正确做法是定位类型标记 `REG_xxx`，
/// 取它之后的全部内容。这条一旦各模块各写一份就会各漂移一份（宪法第 8 条），故下沉到这里。
///
/// 行形如：`    ProcessorNameString    REG_SZ    12th Gen Intel(R) Core(TM) i9-12900H`
#[cfg(windows)]
pub fn reg_query(key: &str, value: &str) -> Option<String> {
    let (code, out) = run_capture_raw("reg", &["query", key, "/v", value], None).ok()?;
    if code != 0 {
        return None;
    }
    let line = out.lines().find(|l| l.contains(value))?;
    let after_type = line.split("REG_").nth(1)?;
    // after_type 形如 "SZ    12th Gen Intel(R) ..." —— 跳过类型名本身
    let rest = after_type.split_once(char::is_whitespace)?.1;
    let v = rest.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Windows 内部版本号（如 22631 / 19045 / 17763）。**探不到就 `None`**。
///
/// 公共层实现，`term.rs`（xterm 折行分支）和 `install_tool_inner`（`min_windows_build` 预检）
/// 共用这一份 —— 这条一旦各模块各写一份就会各漂移一份（宪法第 8 条）。
///
/// 🔴 `None` 一律按「不知道」处理，**不许当成版本旧**：那是把我们探测失败说成客户机器有问题
/// （同 envfp 里 `pwsh_old` 探不到版本不判旧那条）。
#[cfg(windows)]
pub fn windows_build_number() -> Option<u32> {
    reg_query(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuild")
        .and_then(|s| s.trim().parse::<u32>().ok())
}

#[cfg(not(windows))]
pub fn windows_build_number() -> Option<u32> {
    None
}

/// 系统代理探测（开启才返回 "host:port"）。
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let (code, out) = run_capture_raw("reg", &["query", KEY, "/v", "ProxyEnable"], None).ok()?;
    if code != 0 || !out.contains("0x1") {
        return None;
    }
    let (code, out) = run_capture_raw("reg", &["query", KEY, "/v", "ProxyServer"], None).ok()?;
    if code != 0 {
        return None;
    }
    // 行形如 "    ProxyServer    REG_SZ    127.0.0.1:7897"
    out.lines().find_map(|l| {
        let l = l.trim();
        l.starts_with("ProxyServer")
            .then(|| l.split_whitespace().last().map(str::to_string))
            .flatten()
    })
}

#[cfg(target_os = "macos")]
fn system_proxy() -> Option<String> {
    let (code, out) = run_capture_raw("scutil", &["--proxy"], None).ok()?;
    if code != 0 {
        return None;
    }
    let get = |k: &str| {
        out.lines()
            .find(|l| l.trim_start().starts_with(k))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
    };
    if get("HTTPSEnable")? == "1" {
        Some(format!("{}:{}", get("HTTPSProxy")?, get("HTTPSPort")?))
    } else {
        None
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_proxy() -> Option<String> {
    None
}

/// Codex 桌面版探测。
/// Windows 是微软商店 MSIX（包名 OpenAI.Codex）：先查 %LOCALAPPDATA%\OpenAI\Codex
/// 数据目录（快，启动过就有），没有再查包注册表（装了没启动过的情况）。
#[cfg(windows)]
pub fn codex_app_installed() -> bool {
    if std::env::var("LOCALAPPDATA")
        .map(|d| Path::new(&d).join("OpenAI").join("Codex").exists())
        .unwrap_or(false)
    {
        return true;
    }
    matches!(
        run_capture_raw(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", "if (Get-AppxPackage -Name OpenAI.Codex) { exit 0 } else { exit 1 }"],
            None,
        ),
        Ok((0, _))
    )
}

#[cfg(not(windows))]
pub fn codex_app_installed() -> bool {
    Path::new("/Applications/Codex.app").exists()
        || std::env::var("HOME")
            .map(|h| Path::new(&h).join("Applications/Codex.app").exists())
            .unwrap_or(false)
}

/// Claude 桌面版探测。
#[cfg(windows)]
fn claude_desktop_installed() -> bool {
    std::env::var("LOCALAPPDATA")
        .map(|d| Path::new(&d).join("AnthropicClaude").exists())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn claude_desktop_installed() -> bool {
    Path::new("/Applications/Claude.app").exists()
        || std::env::var("HOME")
            .map(|h| Path::new(&h).join("Applications/Claude.app").exists())
            .unwrap_or(false)
}

// ============================================================
// 安装执行
// ============================================================

/// 执行一个工具的完整安装流：steps → verify →（失败）repair → verify。
///
/// `on_log(phase, line)`：phase ∈ step / out / verify / repair / done / error。
/// 安装某工具（skill 步骤 → 验证 → 失败则跑 repair）。**codex 专属最后防线**：正常 + repair
/// 都没救回来时，直接下 OpenAI 官方**单文件二进制**（我们镜像到阿里云 OSS，国内快、断点续传）
/// 装到 `~/bin/codex.exe`（Win）/ `~/.local/bin/codex`（Mac）。这是 **Mac 唯一的兜底**——
/// skill 的 `run` 步骤全是 Windows 命令（`%SystemRoot%\...`、PowerShell），Mac 上走 `sh`
/// 第一步就 command not found，`run_steps` 首败即返回 → npm 一失败就彻底没救（老版本铁证）。
pub fn install_tool(
    skill: &Skill,
    tool_id: &str,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> InstallToolResult {
    let res = install_tool_inner(skill, tool_id, on_log);
    if res.ok || tool_id != "codex" {
        return res;
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        on_log(
            "repair",
            "codex 走 npm 仍未成功，启用官方二进制兜底（阿里云国内镜像，断点续传）…",
        );
        match ensure_codex_binary(on_log) {
            Ok(dest) => {
                if let Some(spec) = skill.tools.get(tool_id) {
                    if let Ok(v) = verify(spec, on_log) {
                        on_log(
                            "done",
                            &format!("Codex CLI 二进制兜底安装成功 · {v}（{}）", dest.display()),
                        );
                        post_install(tool_id, on_log);
                        return InstallToolResult {
                            ok: true,
                            tool: tool_id.into(),
                            version: Some(v),
                            attempts: 3,
                            error: None,
                        };
                    }
                }
                on_log("error", "二进制已落地但 codex --version 仍未通过");
                res
            }
            Err(e) => {
                on_log("error", &format!("官方二进制兜底也失败：{e}"));
                res
            }
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        res
    }
}

fn install_tool_inner(
    skill: &Skill,
    tool_id: &str,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> InstallToolResult {
    let Some(spec) = skill.tools.get(tool_id) else {
        return fail(tool_id, 0, format!("skill 清单里没有工具 {tool_id}"));
    };

    // 磁盘空间预检（min_free_mb > 0 才查）：满盘时下载报错是天书，提前拦下来说人话
    if spec.min_free_mb > 0 {
        if let Some(free_mb) = temp_disk_free_mb() {
            if free_mb < spec.min_free_mb {
                let msg = format!(
                    "系统盘空间不足：仅剩 {free_mb} MB，安装 {} 需要至少 {} MB。请清理磁盘（删除大文件 / 清空回收站 / 系统-存储-清理建议）后重试。",
                    spec.name, spec.min_free_mb
                );
                on_log("error", &msg);
                return fail(tool_id, 0, msg);
            }
        }
    }

    // 系统版本预检（min_windows_build > 0 才查）：装不上的原因是这台机器的 Windows 版本，
    // 不是网络也不是杀软 —— 提前拦下，别让客户先等一个 667MB 的 repair 下载再看乱码报错。
    // **探不到 build 就放行**：探测失败不等于版本不够，不许把我们探不到说成客户机器有问题。
    if spec.min_windows_build > 0 {
        if let Some(build) = windows_build_number() {
            if build < spec.min_windows_build {
                let msg = format!(
                    "{} 装不了：它要求 Windows 内部版本 {} 或更高，这台机器是 {}。\
                     这是系统版本限制（微软商店和离线安装包都会拒），重试也一样 —— \
                     请改用命令行版（功能相同、对系统版本无要求），或升级 Windows 后再试。",
                    spec.name, spec.min_windows_build, build
                );
                on_log("error", &msg);
                return fail(tool_id, 0, msg);
            }
        }
    }

    // 环境预检 + 免提权自动修（PATH 丢 System32 等）。修了什么/剩什么都进日志——
    // 失败时这些行随 report_bug 的日志尾部一起上报，triage 一眼分清环境问题 vs 我们的 bug。
    let pre = env_precheck_and_fix();
    for f in pre["fixed"].as_array().into_iter().flatten() {
        if let Some(s) = f.as_str() {
            on_log("step", &format!("环境自修复：{s}"));
        }
    }
    for i in pre["issues"].as_array().into_iter().flatten() {
        if let Some(s) = i.as_str() {
            on_log("out", &format!("环境预检提示：{s}"));
        }
    }
    // fragility 警告（OneDrive 目录 / 中文用户名 / 长路径未开）——不阻断，但随日志上报，
    // 装失败时 triage 一眼分清是环境脆弱 vs 我们的 bug。
    for wn in pre["warnings"].as_array().into_iter().flatten() {
        if let Some(s) = wn.as_str() {
            on_log("out", &format!("环境注意：{s}"));
        }
    }

    // 安装前先放开 PowerShell 执行策略 —— 否则 npm 全局工具的 .ps1 启动器
    // 在 PowerShell 里会被默认 Restricted 策略拦死（claude/codex 报"禁止运行脚本"）。
    ensure_powershell_policy(on_log);

    // 第一轮：正常步骤
    if let Err(e) = run_steps(skill, &spec.steps, on_log) {
        on_log("error", &format!("安装步骤失败：{e}"));
        // 步骤失败也尝试修复流
        return run_repair(skill, tool_id, spec, on_log);
    }
    match verify(spec, on_log) {
        Ok(v) => {
            on_log("done", &format!("{} 安装成功 · {v}", spec.name));
            post_install(tool_id, on_log);
            InstallToolResult {
                ok: true,
                tool: tool_id.into(),
                version: Some(v),
                attempts: 1,
                error: None,
            }
        }
        Err(e) => {
            on_log("verify", &format!("首次验证未通过（{e}），进入自动修复…"));
            run_repair(skill, tool_id, spec, on_log)
        }
    }
}

/// 装完某工具后的收尾动作：尽量把默认虾盘云驱动也写好，做到“安装成功 = 可直接试用”。
fn post_install(tool_id: &str, on_log: &(dyn Fn(&str, &str) + Send + Sync)) {
    match tool_id {
        "openclaw" => match write_openclaw_xiapan() {
            Ok(path) => on_log(
                "done",
                &format!("已为 OpenClaw 配好默认模型「虾盘云·DeepSeek V4」，开箱即用（{path}）"),
            ),
            Err(e) => on_log(
                "out",
                &format!("OpenClaw 已装好，但自动配虾盘云失败（{e}），可在管理台手动切驱动"),
            ),
        },
        "hermes" => {
            match ensure_hermes_utils_shim() {
                Ok(()) => on_log("out", "已检查 Hermes Python 兼容补丁"),
                Err(e) => on_log("out", &format!("Hermes Python 兼容补丁写入失败（{e}）")),
            }
            match write_hermes_xiapan() {
                Ok(path) => on_log(
                    "done",
                    &format!("已为 Hermes 配好默认模型「虾盘云·DeepSeek V4」，启动网页版即可试用（{path}）"),
                ),
                Err(e) => on_log(
                    "out",
                    &format!("Hermes 已装好，但自动配虾盘云失败（{e}），可在管理台手动切驱动"),
                ),
            }
        }
        "codex" | "codex-app" => match write_codex_xiapan() {
            Ok(path) => on_log(
                "done",
                &format!("已为 Codex 接好虾盘云驱动，充值后即可使用（{path}）"),
            ),
            Err(e) => on_log(
                "out",
                &format!("Codex 已装好，但自动接虾盘云失败（{e}），可在 Codex 专区点「一键接虾盘云」"),
            ),
        },
        _ => {}
    }
}

fn write_codex_xiapan() -> Result<String, String> {
    let key = crate::device::device_key_offline()?;
    let targets = vec!["codex".to_string()];
    let r = crate::providers::apply_provider("xiapan", &key, None, &targets)?;
    if r.codex.is_some() {
        Ok("~/.codex/config.toml".into())
    } else {
        Err("Codex 配置未写入（请稍后在 Codex 专区重试）".into())
    }
}

// ============================================================
// Codex 专属：官方单文件二进制兜底（跨平台，npm 链路失败时的最后防线）
// ============================================================

/// codex 单文件二进制的落地目录：Win = `~/bin`、Mac = `~/.local/bin`。
/// 两者都已在 `search_paths()` 里（装前 create_dir_all 保证存在后被扫到）。
#[cfg(any(windows, target_os = "macos"))]
fn codex_bin_dir() -> Result<PathBuf, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "找不到用户目录".to_string())?;
    #[cfg(windows)]
    {
        Ok(Path::new(&home).join("bin"))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Path::new(&home).join(".local").join("bin"))
    }
}

/// 递归找名为 `name` 的文件，返回首个匹配的完整路径（压缩包可能带子目录）。
#[cfg(any(windows, target_os = "macos"))]
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let direct = root.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(hit) = find_file(&p, name) {
                return Some(hit);
            }
        } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(p);
        }
    }
    None
}

/// 删掉排在落地目录之前、半装坏掉的 codex 启动器 —— 否则 npm 只装上了 `codex.cmd`
/// 启动器却没拉到平台二进制时，那个坏启动器会在 PATH 里挡住我们兜底装的好二进制，
/// verify 依然失败。走到二进制兜底 = 现有 codex 必然是坏的/缺的，删掉总是安全。
#[cfg(any(windows, target_os = "macos"))]
fn remove_shadowing_codex(keep: &Path) {
    let names: &[&str] = if cfg!(windows) {
        &["codex", "codex.cmd", "codex.ps1", "codex.exe"]
    } else {
        &["codex"]
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = portable_node_dir() {
        dirs.push(d);
    }
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(Path::new(&appdata).join("npm"));
    }
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(Path::new(&home).join(".npm-global").join("bin"));
    }
    for dir in dirs {
        if dir == keep {
            continue;
        }
        for n in names {
            let p = dir.join(n);
            if p.exists() {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// 下大文件：主源（OSS 国内，快）+ 断点续传（`curl -C -`）+ 源内多次重试；不行再换兜底源
/// （GitHub）。换源时清半包（跨源字节不可续），同源多次尝试靠 `-C -` 从断点续下。
#[cfg(any(windows, target_os = "macos"))]
fn download_big_resume(
    primary: &str,
    secondary: &str,
    out: &str,
    min_bytes: u64,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<(), String> {
    let mut last = String::new();
    for (idx, url) in [primary, secondary].iter().enumerate() {
        if url.is_empty() {
            continue;
        }
        let _ = std::fs::remove_file(out); // 换源：半包不可跨源续传，清掉重来
        for attempt in 1..=4 {
            // -C - 断点续传；--retry 处理瞬时抖动；-m 给足慢网时间（30 分钟）
            let r = run_capture_raw(
                "curl",
                &[
                    "-fL", "-sS", "-C", "-", "--retry", "3", "--retry-delay", "3", "-m", "1800",
                    "--proxy", "", "-o", out, url,
                ],
                None,
            );
            let sz = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
            match r {
                Ok((0, _)) if sz >= min_bytes => return Ok(()),
                Ok((code, o)) => {
                    last = format!("curl 退出码 {code}（已下 {sz} 字节）：{}", tail(&o, 120))
                }
                Err(e) => last = e,
            }
            let src = if idx == 0 { "国内镜像" } else { "GitHub" };
            on_log(
                "out",
                &format!("{src} 下载重试 {attempt}/4（已续 {} MB）…", sz / 1_000_000),
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    Err(format!("codex 二进制下载失败（OSS + GitHub 均未成功）：{last}"))
}

/// codex 官方单文件二进制兜底（跨平台）。见 `install_tool` 顶部说明。成功返回落地路径。
#[cfg(any(windows, target_os = "macos"))]
pub fn ensure_codex_binary(
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<PathBuf, String> {
    // 平台/架构 → (OSS 文件名, GitHub 资产名, 压缩包内二进制名, 落地文件名)。
    // ★ Win zip 里有三个 exe，真身是 codex-x86_64-pc-windows-msvc.exe（别拿 command-runner，
    //   那个跑起来报 "no pipe-in"）；Mac tar.gz 里是 codex-<arch>-apple-darwin（不叫 codex）。
    #[cfg(windows)]
    let (oss_name, gh_asset, inner, out_name): (&str, &str, &str, &str) =
        if std::env::consts::ARCH == "aarch64" {
            (
                "codex-win-arm64.zip",
                "codex-aarch64-pc-windows-msvc.exe.zip",
                "codex-aarch64-pc-windows-msvc.exe",
                "codex.exe",
            )
        } else {
            (
                "codex-win-x64.zip",
                "codex-x86_64-pc-windows-msvc.exe.zip",
                "codex-x86_64-pc-windows-msvc.exe",
                "codex.exe",
            )
        };
    #[cfg(target_os = "macos")]
    let (oss_name, gh_asset, inner, out_name): (&str, &str, &str, &str) =
        if std::env::consts::ARCH == "aarch64" {
            (
                "codex-mac-arm64.tar.gz",
                "codex-aarch64-apple-darwin.tar.gz",
                "codex-aarch64-apple-darwin",
                "codex",
            )
        } else {
            (
                "codex-mac-x64.tar.gz",
                "codex-x86_64-apple-darwin.tar.gz",
                "codex-x86_64-apple-darwin",
                "codex",
            )
        };

    let oss =
        format!("https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/runtimes/{oss_name}");
    let gh = format!("https://github.com/openai/codex/releases/download/rust-v0.143.0/{gh_asset}");

    let tmp = std::env::temp_dir().join(oss_name);
    let tmp_s = tmp.display().to_string();
    // 官方二进制解压后 250~340MB，压缩包 90~114MB；50MB 兜底挡代理错误页/半包。
    download_big_resume(&oss, &gh, &tmp_s, 50_000_000, on_log)?;
    on_log("out", "下载完成，解压官方 codex 二进制…");

    let work = std::env::temp_dir().join("uking-codex-bin");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("建临时目录失败: {e}"))?;
    extract_archive(&tmp_s, &work)?;

    let src = find_file(&work, inner).ok_or_else(|| format!("压缩包里没找到 {inner}"))?;

    let dest_dir = codex_bin_dir()?;
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("建目标目录失败: {e}"))?;
    let dest = dest_dir.join(out_name);
    let _ = std::fs::remove_file(&dest); // 旧的可能占用，尽力删
    std::fs::copy(&src, &dest)
        .map_err(|e| format!("拷贝 codex 到 {} 失败: {e}", dest.display()))?;
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("chmod")
            .args(["+x", &dest.display().to_string()])
            .status();
    }

    // 清掉可能挡路的坏启动器 + 把落地目录持久化进用户 PATH + 塞进当前进程 PATH（让紧接着的
    // verify 立刻看到），三管齐下保证兜底装的好二进制真的能被 `codex --version` 命中。
    remove_shadowing_codex(&dest_dir);
    let _ = add_user_path(&dest_dir);
    prepend_process_path(&[dest_dir.clone()]);

    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_dir_all(&work);
    Ok(dest)
}

/// 给 OpenClaw 写一份默认接虾盘云的配置（设备内置 Key + api.u-claw.org）。
///
/// OpenClaw 读 `OPENCLAW_CONFIG_PATH`，默认 `~/.openclaw/openclaw.json`。
/// 已存在配置时只补 models.providers.xiapan + 设默认模型，不覆盖用户其它设置。
fn write_openclaw_xiapan() -> Result<String, String> {
    let key = crate::device::device_key_offline()?;
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "找不到用户目录".to_string())?;
    let dir = std::path::Path::new(&home).join(".openclaw");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 .openclaw 目录失败: {e}"))?;
    let path = dir.join("openclaw.json");

    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }

    // gateway 默认 token（不覆盖已有）
    let obj = root.as_object_mut().unwrap();
    obj.entry("gateway").or_insert_with(|| {
        serde_json::json!({ "mode": "local", "auth": { "token": "uclaw" } })
    });

    // models.providers.xiapan（OpenAI 兼容端点）
    let models = obj
        .entry("models")
        .or_insert_with(|| serde_json::json!({ "mode": "merge", "providers": {} }));
    if !models.is_object() {
        *models = serde_json::json!({ "mode": "merge", "providers": {} });
    }
    let models_obj = models.as_object_mut().unwrap();
    models_obj.entry("mode").or_insert_with(|| serde_json::json!("merge"));
    let providers = models_obj
        .entry("providers")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(pobj) = providers.as_object_mut() {
        pobj.insert(
            "xiapan".into(),
            serde_json::json!({
                "baseUrl": "https://api.u-claw.org.cn/v1",
                "apiKey": key,
                "api": "openai-completions",
                "timeoutSeconds": 600,
                // 输出上限（openclaw 文档 model-providers.md：provider 级 maxTokens 是所有模型默认）。
                // 必须够大：推理型模型（deepseek-v4-pro）先烧 reasoning_content 再写正文，预算太小
                // 就正文空、stopReason=length →「无法生成回复」。8192 够"思考+正文"都放下。
                "maxTokens": 8192,
                "models": [
                    { "id": "deepseek-v4-pro", "name": "虾盘云 DeepSeek V4 Pro" },
                    { "id": "deepseek-v4-flash", "name": "虾盘云 DeepSeek V4 Flash" },
                    { "id": "claude-sonnet-4-6", "name": "虾盘云 Claude Sonnet" },
                    { "id": "gpt-5.4", "name": "虾盘云 GPT-5.4" }
                ]
            }),
        );
    }
    let agents = obj
        .entry("agents")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(aobj) = agents.as_object_mut() {
        let defaults = aobj
            .entry("defaults")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(dobj) = defaults.as_object_mut() {
            dobj.insert(
                "model".into(),
                // 默认 flash（非推理型，不会因思考吃光预算而正文空→「无法生成回复」，见 providers.rs 注释）；
                // pro 作 fallback：flash 万一报错才升级到满血 pro（pro 已在上面 models[] 里，引用合法）。
                serde_json::json!({ "primary": "xiapan/deepseek-v4-flash", "fallbacks": ["xiapan/deepseek-v4-pro"] }),
            );
            dobj.insert("timeoutSeconds".into(), serde_json::json!(600));
        }
    }

    let text = serde_json::to_string_pretty(&root).map_err(|e| format!("序列化配置失败: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("写 openclaw.json 失败: {e}"))?;
    Ok(path.display().to_string())
}

/// 给 Hermes 写一份默认接虾盘云的配置。真正写法集中在 providers.rs，和“AI 设置”
/// 页面同口径，避免安装器和驱动管理各写一套 schema。
fn write_hermes_xiapan() -> Result<String, String> {
    let key = crate::device::device_key_offline()?;
    let targets = vec!["hermes".to_string()];
    let r = crate::providers::apply_provider("xiapan", &key, None, &targets)?;
    if r.hermes.is_some() {
        Ok("Hermes config.yaml / .env".into())
    } else {
        Err("Hermes 配置未写入（请稍后在 AI 设置里重试）".into())
    }
}

fn run_repair(
    skill: &Skill,
    tool_id: &str,
    spec: &ToolSpec,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> InstallToolResult {
    if spec.repair.is_empty() {
        return fail(tool_id, 1, "安装未通过验证，且无修复步骤".into());
    }
    on_log("repair", "开始自动修复重装…");
    if let Err(e) = run_steps(skill, &spec.repair, on_log) {
        return fail(tool_id, 2, format!("修复步骤失败：{e}"));
    }
    match verify(spec, on_log) {
        Ok(v) => {
            on_log("done", &format!("{} 修复后安装成功 · {v}", spec.name));
            post_install(tool_id, on_log);
            InstallToolResult {
                ok: true,
                tool: tool_id.into(),
                version: Some(v),
                attempts: 2,
                error: None,
            }
        }
        Err(e) => fail(tool_id, 2, format!("修复后仍未通过验证：{e}")),
    }
}

fn fail(tool: &str, attempts: u32, err: String) -> InstallToolResult {
    InstallToolResult {
        ok: false,
        tool: tool.into(),
        version: None,
        attempts,
        error: Some(err),
    }
}

fn run_steps(
    skill: &Skill,
    steps: &[Step],
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<(), String> {
    for step in steps {
        on_log("step", step.label());
        match step {
            Step::EnsureNode { min, .. } => ensure_node_min(skill, min.as_deref(), on_log)?,
            Step::NpmInstall { package, force, with_optional, .. } => {
                // skill 可能来自服务器下发：包名严格校验，防被篡改后注入任意命令
                if !valid_npm_package(package) {
                    return Err(format!("非法 npm 包名（已拦截）：{package}"));
                }
                if !skill.npm_registry.starts_with("https://") {
                    return Err("npm registry 必须是 https（已拦截）".into());
                }
                // npmmirror 偶发对刚发布/同步中的包返回 404。主装和 repair 若仍只重试同一源，
                // 会在同一个窗口里确定性失败（issue #259/#260/#261，OpenClaw 同设备连续三报）。
                // 保留国内镜像为主源；失败后只回退到编译内置的 npm 官方 HTTPS 源。每一轮仍复用
                // 包名白名单、禁代理、固定 prefix、optional/force 参数，不接受服务器下发任意备用源。
                let mut registries = vec![skill.npm_registry.as_str()];
                for fallback in NPM_FALLBACK_REGISTRIES {
                    if !registries.contains(fallback) {
                        registries.push(fallback);
                    }
                }
                let mut errors = Vec::new();
                let mut installed = false;
                for (index, registry) in registries.iter().enumerate() {
                    if index > 0 {
                        on_log(
                            "out",
                            &format!("npm 主源安装失败，切换备用源 {registry} 重试…"),
                        );
                    }
                    let cmd = npm_install_command(package, registry, *with_optional, *force);
                    match run_stream(&cmd, portable_node_dir().as_deref(), on_log) {
                        Ok(()) => {
                            installed = true;
                            break;
                        }
                        Err(e) => errors.push(format!("{registry}: {e}")),
                    }
                }
                if !installed {
                    return Err(format!("npm 主源与备用源均安装失败：{}", errors.join("；")));
                }
            }
            Step::EnsurePython { .. } => ensure_python(skill, on_log)?,
            Step::PipInstall { package, force, .. } => {
                if !valid_pip_package(package) {
                    return Err(format!("非法 pip 包名（已拦截）：{package}"));
                }
                if !skill.pip_index.starts_with("https://") {
                    return Err("pip index 必须是 https（已拦截）".into());
                }
                // 用裸 `python -m pip`（便携 Python 目录已经在 search_paths 里注入 PATH）。
                // 不用绝对路径加引号 —— cmd /C 对 "带引号路径开头" 的命令有吃引号的坑。
                portable_python_exe().ok_or("便携 Python 未就绪")?;
                // wheelhouse 式 preflight（借鉴竞品 LastAI）：装前先探依赖源可达。连不上且本地缓存也空 →
                // 早失败给人话，别让 pip 在子进程里干等到超时（客户机实测卡在 subprocess.run 过）；
                // 缓存非空则放行，离线靠缓存也装得上。主源连不上时还要探兜底源——只有主源+全部兜底源都不可达
                // 才算真连不上（否则主源 host 宕了会把「其实兜底源能装」的客户误判成断网）。
                if !pip_mirror_reachable(&skill.pip_index)
                    && !PIP_FALLBACK_INDEXES.iter().any(|m| pip_mirror_reachable(m))
                {
                    if pip_cache_has_wheels() {
                        on_log("out", "pip 依赖源暂时连不上，改用本地已缓存的 wheel 安装…");
                    } else {
                        return Err(format!(
                            "pip 依赖源（{} 及兜底镜像）都连不上，且本地没有可复用的缓存：请检查网络后重试。",
                            skill.pip_index
                        ));
                    }
                }
                // --prefer-binary：优先装预编译 wheel、不从源码 build（客户机无编译器时的「现场 build 必败」根因）。
                // --extra-index-url 兜底源：主源（aliyun）偶发对某个包返回「from versions: none」（同步空档，
                // 明明包在、Python 版本也兼容却一个版本都解析不到）时，pip 会自动去兜底源找同一个包 →
                // 单源无兜底导致 Hermes 装不上的根因（pc-*** / Issue #200）就此消除。主装与 repair 走同一分支，一处修全覆盖。
                let mut cmd = format!(
                    "python -m pip install -U {package} -i {} --prefer-binary --disable-pip-version-check --no-warn-script-location",
                    skill.pip_index
                );
                for fb in PIP_FALLBACK_INDEXES {
                    cmd.push_str(&format!(" --extra-index-url {fb}"));
                }
                if *force {
                    cmd.push_str(" --force-reinstall");
                }
                run_stream(&cmd, tool_path_dir().as_deref(), on_log)?;
                // pip 装完，Scripts 目录此时已存在 → 持久化进用户 PATH，让 hermes 全局可敲。
                persist_python_scripts_path(on_log);
                // pip 装完 hermes 后立刻自愈 utils shim（删挡路的旧 shim 包，让 hermes 自带 utils.py
                // 生效）——放这里确保在 verify(hermes --version) 之前跑，否则残留 shim 会让 verify 直接崩。
                // 非 hermes 的 pip 装是 no-op（没 shim 就不动）。skill 里那步 PowerShell 因 shell 引号坑
                // 不可靠，真正靠这里的进程内 heal。
                let _ = ensure_hermes_utils_shim();
            }
            Step::Run { cmd, os, .. } => {
                // 平台不对就跳过，**但要说出来** —— 静默跳过会让「这步到底跑没跑」
                // 在日志里查无对证，而客户报障时我们只有这份日志。
                if let Some(want) = os.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    if !want.eq_ignore_ascii_case(current_os_tag()) {
                        on_log("out", &format!("跳过（这一步只在 {want} 上需要，本机是 {}）", current_os_tag()));
                        continue;
                    }
                }
                // 同样过危险命令黑名单（与 AI 修复共用）
                let lower = cmd.to_lowercase();
                if let Some(bad) = FIX_BLACKLIST.iter().find(|b| lower.contains(*b)) {
                    return Err(format!("skill 步骤含危险命令（包含 `{bad}`），已拦截"));
                }
                run_stream(cmd, tool_path_dir().as_deref(), on_log)?;
            }
        }
    }
    Ok(())
}

/// pip 包名校验（PyPI 命名：字母数字 . _ -）。
fn valid_pip_package(p: &str) -> bool {
    !p.is_empty()
        && p.len() < 120
        && p.chars().all(|c| c.is_ascii_alphanumeric() || ".-_=<>[]".contains(c))
        && !p.contains("..")
}

/// 验证/运行时给子进程注入的便携工具目录：便携 Node + 便携 Python 的 bin/scripts。
/// （单独的 portable_node_dir 仍保留给纯 node 工具用，这个是合并版给 run/pip 用。）
fn tool_path_dir() -> Option<PathBuf> {
    // 返回 node 目录（with_path 里 search_paths 已会补 python/homebrew，
    // 但 pip 装的 hermes.exe 在 python 的 Scripts，需显式加）
    portable_node_dir()
}

/// npm 包名白名单字符校验：@scope/name 或 name，只允许小写字母/数字/@/./-/_//。
fn valid_npm_package(p: &str) -> bool {
    !p.is_empty()
        && p.len() < 120
        && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "@./-_".contains(c))
        && !p.contains("..")
}

/// npm 国内镜像同步空档/临时 404 时的最后兜底。只接受编译进 exe 的固定 HTTPS 源，
/// 不从服务器 skill 读取备用列表，避免扩大被篡改清单的命令执行面。
const NPM_FALLBACK_REGISTRIES: &[&str] = &["https://registry.npmjs.org"];

fn npm_install_command(
    package: &str,
    registry: &str,
    with_optional: bool,
    force: bool,
) -> String {
    // --proxy="" --https-proxy="" 强制本次安装不走代理。
    // 客户机的代理可能写死在全局 npmrc（%APPDATA%\npm\etc\npmrc），
    // env 层清空盖不住它，只有命令行 flag 能覆盖 npmrc 配置。
    let mut cmd = format!(
        "npm install -g {package} --registry={registry} --proxy=\"\" --https-proxy=\"\" --no-fund --no-audit"
    );
    if with_optional {
        // 强制拉可选依赖（平台二进制子包）。默认 npm 会拉，但 --no-optional
        // 可能写死在全局 npmrc，命令行显式覆盖。
        cmd.push_str(" --include=optional");
    }
    if force {
        cmd.push_str(" --force");
    }
    // macOS：系统 node 的默认全局 prefix 是 /usr/local（要 sudo，普通用户装不进），
    // 且客户机 ~/.npmrc 可能残留坏 prefix。显式钉到可写且在 search_paths 的 ~/.local。
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        cmd.push_str(&format!(" --prefix \"{home}/.local\""));
    }
    // Windows：客户机 npm prefix 可能被其他便携工具指到 U 盘/exFAT，导致硬链接失败。
    // 钉到我们确定在 search_paths 里的便携 Node；没有则回到 Windows npm 默认全局目录。
    #[cfg(windows)]
    {
        let prefix = portable_node_dir()
            .or_else(|| std::env::var("APPDATA").ok().map(|a| Path::new(&a).join("npm")));
        if let Some(p) = prefix {
            cmd.push_str(&format!(" --prefix \"{}\"", p.display()));
        }
    }
    cmd
}

/// verify_cmd 验证，返回首行输出（版本号）。
/// 刚被 npm/tar 落盘的可执行文件（claude.cmd / codex.exe 等）常被杀软实时扫描瞬时锁住，
/// 此时立刻验证会报「找不到指定的文件」「进程无法访问」「batch file cannot be found」
/// 「不是有效的 Win32 应用程序」等——过 1~2 秒锁就释放了。命中这类特征就退避重试几次，
/// 而不是真正的语义性失败（如版本不兼容）直接放弃。同一病根见 consume_and_apply 的
/// rename 重试（issue #50/#51/#52/#71/#108/#112 实锤，历史上只有 1 次 verify 机会）。
fn looks_like_transient_lock(err: &str) -> bool {
    const MARKERS: &[&str] = &[
        "找不到指定的文件",
        "找不到指定的路径",
        "进程无法访问",
        "另一个程序正在使用",
        "cannot access the file because it is being used",
        "batch file cannot be found",
        "不是有效的 win32 应用程序",
        "is not recognized as an internal or external command",
        "不是内部或外部命令",
        "系统找不到指定的文件",
    ];
    let lower = err.to_lowercase();
    MARKERS.iter().any(|m| lower.contains(&m.to_lowercase()))
}

fn verify(spec: &ToolSpec, on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<String, String> {
    on_log("verify", &format!("验证：{}", spec.verify_cmd));
    let mut last_err = String::new();
    // 杀软瞬时锁重试：6 次、退避递增到 ~10s 总时长（issue #136 实锤——原来 4×1s≈4s
    // 顶不住某些客户机杀软对刚落盘 claude.exe 的持锁，验证被误判失败进而误报装机失败）。
    // 跨平台放在这里，别塞进单字段 verify_cmd（那会在 Mac 上崩，见 skill v32→v33 回滚 #143）。
    for attempt in 0..6u64 {
        match run_capture(&spec.verify_cmd, portable_node_dir().as_deref()) {
            Ok((0, out)) => {
                let v = out.lines().next().unwrap_or("ok").trim().to_string();
                on_log("verify", &format!("验证通过：{v}"));
                return Ok(v);
            }
            Ok((code, out)) => last_err = format!("退出码 {code}：{}", tail(&out, 200)),
            Err(e) => last_err = e,
        }
        if attempt < 5 && looks_like_transient_lock(&last_err) {
            let wait = 700 * (attempt + 1); // 700/1400/2100/2800/3500ms，累计 ~10.5s
            on_log("verify", &format!("验证未通过，疑似杀软瞬时锁住刚落盘的文件，{wait}ms 后重试…"));
            std::thread::sleep(std::time::Duration::from_millis(wait));
            continue;
        }
        break;
    }
    Err(last_err)
}

// ============================================================
// AI 修复命令执行（用户确认后才会调到这里）
// ============================================================

/// AI 给出的修复命令里禁止出现的片段（小写匹配）。宁可误杀不可放过。
const FIX_BLACKLIST: &[&str] = &[
    // Windows
    "format ",
    "diskpart",
    "bcdedit",
    "shutdown",
    "restart-computer",
    "vssadmin",
    "cipher /w",
    "reg delete hklm",
    "reg add hklm",
    "rd /s /q c:\\",
    "rmdir /s /q c:\\",
    "del /f /s /q c:\\",
    "remove-item -recurse c:\\",
    "schtasks /create",
    "netsh advfirewall",
    "takeown",
    "icacls c:\\",
    // macOS / Unix
    "rm -rf /",
    "rm -fr /",
    "sudo ",
    "diskutil erase",
    "mkfs",
    "dd if=",
    "launchctl ",
    "csrutil",
    "spctl --master-disable",
    "chmod -r 777 /",
];

/// 执行一条 AI 修复命令（带黑名单校验 + 流式日志）。
pub fn run_fix_command(
    cmd: &str,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<(), String> {
    let lower = cmd.to_lowercase();
    if let Some(bad) = FIX_BLACKLIST.iter().find(|b| lower.contains(*b)) {
        return Err(format!("已拦截危险命令（包含 `{bad}`），拒绝执行"));
    }
    on_log("step", &format!("AI 修复：{cmd}"));
    run_stream(cmd, portable_node_dir().as_deref(), on_log)
}

// ============================================================
// 安装日志落盘（诊断用；公共能力，feedback 等模块只读借用）
// ============================================================

/// 安装日志文件 `~/.uking/logs/install.log`。
///
/// 为什么要落盘：装机日志此前**只存在于前端气泡**（`uking:wizard` 事件 → Wizard.tsx 内存），
/// 客户点「技术支持」时我们采不到它，只能靠用户自己复制粘贴（issue #226 实锤：客户手工
/// 贴了一大段 Hermes 装机日志）。落一份滚动日志后，`feedback::collect_diagnostics` 直接带上。
pub fn install_log_path() -> PathBuf {
    uking_home().join("logs").join("install.log")
}

/// 日志上限：超过就砍掉前半（保尾部——报错总在最后）。
const INSTALL_LOG_MAX: u64 = 512 * 1024;

/// 追加一行安装日志。**best-effort**：写失败静默跳过，绝不影响安装本身。
pub fn append_install_log(tool: &str, phase: &str, line: &str) {
    use std::io::Write;
    let p = install_log_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // 超限先截半（读尾部再整写回，日志本身不大，简单可靠）。
    if std::fs::metadata(&p).map(|m| m.len() > INSTALL_LOG_MAX).unwrap_or(false) {
        if let Some(tail) = tail_of_file(&p, INSTALL_LOG_MAX / 2) {
            let _ = std::fs::write(&p, format!("…（已截断早期日志）\n{tail}"));
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "[{tool}/{phase}] {line}");
    }
}

/// 写一条带时间的分隔头（每次开始装某个工具时调一次）。
pub fn install_log_header(tool: &str) {
    append_install_log(
        tool,
        "开始",
        &format!("======== {} (UTC) ========", utc_stamp()),
    );
}

/// 安装日志尾部（最多 n 字节），给诊断采集用。没有日志返回 None。
pub fn install_log_tail(n: u64) -> Option<String> {
    tail_of_file(&install_log_path(), n)
}

/// 读文件尾部最多 n 字节（UTF-8 边界用 lossy 兜）。
fn tail_of_file(path: &Path, n: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(n))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let s = String::from_utf8_lossy(&buf).into_owned();
    (!s.trim().is_empty()).then_some(s)
}

/// `YYYY-MM-DD HH:MM:SS`（UTC）。纯算术，不起子进程 —— 每行日志都要用，不能像
/// `usage.rs` 那样调 PowerShell 取时间。
fn utc_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days（Howard Hinnant 算法）：天数 → 年月日
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = era * 400 + yoe + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

// ============================================================
// 便携 Node（缺 Node 时自动装）
// ============================================================

/// `~/.uking`。**公共层的唯一口径** —— 别在自己模块里再拼一次 `home.join(".uking")`
/// （已经有过几份，`UKING_TEST_HOME` 一改就漏掉几份）。
pub fn uking_home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".uking")
}

/// 便携 Node 的可执行目录（已存在才返回）。npm 全局包的启动器也落在这里。
/// Windows：node/ 本身；macOS：node/bin/。
pub fn portable_node_dir() -> Option<PathBuf> {
    let base = uking_home().join("runtime").join("node");
    #[cfg(windows)]
    {
        base.join("node.exe").exists().then_some(base)
    }
    #[cfg(not(windows))]
    {
        let bin = base.join("bin");
        bin.join("node").exists().then_some(bin)
    }
}

/// 找一个能用的 `node` 可执行文件：便携 Node → U-King `search_paths` → **系统 PATH** →
/// 常见安装位置。
///
/// 放在公共层是因为**不止一个模块要起 Node 脚本**（`codex_proxy` 的 responses↔chat 桥、
/// `claude_proxy` 的 messages↔chat 桥），而这四级兜底是踩出来的：客户机很多是系统 Node
/// （`C:\Program Files\nodejs`），只查前两处会漏，报成「没找到 Node」。
/// 复制第二份就会漂移第二份（宪法第 8/12 条）—— 要加兜底只在这儿加。
pub fn find_node() -> Option<PathBuf> {
    let names = if cfg!(windows) { &["node.exe", "node"][..] } else { &["node"][..] };
    // 1) 便携 Node
    if let Some(dir) = portable_node_dir() {
        for cand in [dir.join("node.exe"), dir.join("bin").join("node"), dir.join("node")] {
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    // 2) U-King search_paths
    for dir in search_paths(portable_node_dir().as_deref()) {
        for n in names {
            let c = dir.join(n);
            if c.exists() {
                return Some(c);
            }
        }
    }
    // 3) 系统 PATH（客户机常是系统 Node）
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path.split(sep).filter(|d| !d.is_empty()) {
            for n in names {
                let c = Path::new(dir).join(n);
                if c.exists() {
                    return Some(c);
                }
            }
        }
    }
    // 4) 常见系统安装位置兜底
    #[cfg(windows)]
    {
        for base in ["C:\\Program Files\\nodejs\\node.exe", "C:\\Program Files (x86)\\nodejs\\node.exe"] {
            let p = Path::new(base);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }
    }
    #[cfg(not(windows))]
    {
        for base in ["/usr/local/bin/node", "/opt/homebrew/bin/node", "/usr/bin/node"] {
            let p = Path::new(base);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }
    }
    None
}

/// `base_url` → chat/completions 完整端点（兼容用户填带 / 不带 `/v1`）。空串返回 `None`，
/// **默认端点由调用方自己决定** —— 两个翻译桥的兜底不一样，把它写死在这儿就绑死了。
///
/// 与 llm.mjs 的端点归一同思路：末段是 `/vN` 就直接补 `/chat/completions`，否则补
/// `/v1/chat/completions`。同样是「两个模块都要用 → 下沉公共层」，别复制第二份。
pub fn to_chat_completions_url(base: &str) -> Option<String> {
    let b = base.trim().trim_end_matches('/');
    if b.is_empty() {
        return None;
    }
    if b.ends_with("/chat/completions") {
        return Some(b.to_string());
    }
    let last_is_ver = b
        .rsplit('/')
        .next()
        .map(|s| s.len() > 1 && s.starts_with('v') && s[1..].chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(false);
    Some(if last_is_ver {
        format!("{b}/chat/completions")
    } else {
        format!("{b}/v1/chat/completions")
    })
}

/// 便携 Git（PortableGit）的可执行目录 (cmd=git.exe, bin=bash.exe)，已解压才返回。
/// Windows 专属：Claude Code 的 Bash 工具刚需 bash.exe（pc-*** 实锤「node/claude 都在、就缺
/// git → 用不了」）。装机向导当年只装 node、绕开 git，这里由「AI 优化大师·一键优化」补上。
/// Mac/Linux 的 git 走系统（Xcode CLT），不便携装 → 返回空。
pub fn portable_git_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let base = uking_home().join("runtime").join("git");
        if base.join("cmd").join("git.exe").exists() {
            return vec![base.join("cmd"), base.join("bin")];
        }
    }
    Vec::new()
}

/// 便携 Python 的 python 可执行文件（已存在才返回）。
/// python-build-standalone 解压后是 `python/python.exe`(win) 或 `python/bin/python3`(unix)。
pub fn portable_python_exe() -> Option<PathBuf> {
    let base = uking_home().join("runtime").join("python");
    #[cfg(windows)]
    {
        let p = base.join("python.exe");
        p.exists().then_some(p)
    }
    #[cfg(not(windows))]
    {
        let p = base.join("bin").join("python3");
        p.exists().then_some(p)
    }
}

/// 读文档（`doc.read`）用的 Python：**便携版优先，没有就用系统的**。
///
/// 🔴 跟 [`portable_python_exe`] 的区别是这里允许回落，别把两者混用：
/// 装 pip 包、跑 Hermes 那类事必须落在我们自己管的便携环境里（版本和依赖可控），
/// 而**读一份 Word/PDF 只是跑一次一次性脚本，系统 python3 完全够用**。
///
/// 以前这里没有回落，于是 macOS/Linux 上 `doc.read` 一律返回「找不到便携 Python」——
/// 哪怕系统 `/usr/bin/python3` 就在那儿，哪怕同一个二进制的 `runtime.toolbox.inspect`
/// 同时在报 `python: installed=true`（它走的是 [`crate::toolbox::python_exe`]）。
/// 说明书里「改客户已有 Word/PPT/Excel」那条配方第 2 步就是 `doc.read`，整条路因此在 Mac 上必挂。
pub fn python_for_docs() -> Option<PathBuf> {
    portable_python_exe().or_else(crate::toolbox::python_exe)
}

/// 便携 Python 装 pip 包后，可执行脚本（hermes.exe 等）所在目录。
/// Windows：python/Scripts；unix：python/bin。
pub fn portable_python_scripts_dir() -> Option<PathBuf> {
    let base = uking_home().join("runtime").join("python");
    #[cfg(windows)]
    let d = base.join("Scripts");
    #[cfg(not(windows))]
    let d = base.join("bin");
    d.exists().then_some(d)
}

/// 确保便携 Python 就绪（下载 python-build-standalone，自带 pip）。
/// 解压前把可能残留的半截目录清掉：先 remove_dir_all 重试几次(抗杀软瞬时锁)，仍不行就
/// 改名 `<dir>.broken.<n>` 挪开(挪开后解压走干净路径，不再撞 tar「Can't unlink already-existing
/// object」——issue #142 实锤：python/vcruntime140.dll 被杀软锁住，tar 覆盖时删不掉旧文件)。
fn clear_dir_before_extract(dir: &Path, on_log: &(dyn Fn(&str, &str) + Send + Sync)) {
    if !dir.exists() {
        return;
    }
    for i in 0..4u64 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500 * (i + 1)));
    }
    // 持久锁删不掉 → 改名挪开，让解压去干净目录（改名通常能过，即便个别文件被扫描）
    for n in 0..50 {
        let away = dir.with_extension(format!("broken.{n}"));
        if !away.exists() && std::fs::rename(dir, &away).is_ok() {
            on_log("out", "已把残留的旧运行时目录挪开，改用干净目录重装");
            return;
        }
    }
    on_log("out", "警告：残留目录清理失败（可能被占用），解压或仍失败");
}

/// 便携 Python 装完自检：真跑一次 import + pip，验证解压完整。
/// 截断的 tar 里 python.exe 先解出来、`Lib/encodings` 或 `site-packages/pip` 没解全时，
/// 「文件存在」检查会误放行，等 Hermes 跑起来才崩「No module named 'encodings'」(#219/#208/#207/#186)
/// 或「No module named pip」(#187)。这一步把「静默半损坏」变成「当场发现 → 上层清理重装」。
/// python-build-standalone 按 exe 相对路径找 Lib，不依赖 PATH；base_command 注入 CREATE_NO_WINDOW 无黑窗。
fn python_smoke_ok() -> Result<(), PySmokeErr> {
    let Some(py) = portable_python_exe() else {
        return Err(PySmokeErr::Broken("未找到 python.exe".into()));
    };
    let pys = py.display().to_string();
    // ① 标准库完整性：encodings 是「No module named 'encodings'」的病灶，ssl/ctypes 是 pip/hermes 刚需。
    match run_capture_raw(&pys, &["-c", "import encodings, ssl, ctypes; print('UKING_PY_OK')"], None) {
        Ok((0, out)) if out.contains("UKING_PY_OK") => {}
        Ok((_c, out)) => {
            return Err(PySmokeErr::Broken(format!("标准库自检未过（解压不完整）：{}", tail(&out, 200))))
        }
        Err(e) => return Err(PySmokeErr::Broken(format!("无法运行 python 自检：{e}"))),
    }
    // ② pip 在位（#187「No module named pip」= 截断的 tar 没解出 site-packages/pip）。
    match run_capture_raw(&pys, &["-m", "pip", "--version"], None) {
        Ok((0, _)) => Ok(()),
        // pip 起得来、只是读不了配置 —— Python 本身是好的，病灶是我们自己写的 pip.ini。
        // 这一支必须先于下面的「解压不完整」判定，否则会被误诊成损坏、白删 Python 重下（pc-***）。
        Ok((_c, out)) if out.contains(PIP_BAD_CONFIG_MARK) => {
            Err(PySmokeErr::PipConfig(tail(&out, 200)))
        }
        Ok((_c, out)) => Err(PySmokeErr::Broken(format!("pip 自检未过（解压不完整）：{}", tail(&out, 200)))),
        Err(e) => Err(PySmokeErr::Broken(format!("无法运行 pip 自检：{e}"))),
    }
}

/// pip 读不了配置文件时的固定报错片段（`ConfigurationFileCouldNotBeLoaded`，pip 各版本通用）。
/// 完整形如 `Configuration file contains invalid cp936 characters in ...\pip.ini.`；
/// 代码页随系统不同（cp936/cp932/cp949…），所以只匹配前半截。
const PIP_BAD_CONFIG_MARK: &str = "Configuration file contains invalid";

/// 便携 Python 自检失败的两种病因 —— 它们的处方完全相反，混作一谈会把「配置坏」
/// 当成「解压坏」治：删掉整个运行时重下几十 MB，病还在，用户已装的包也没了（pc-***）。
enum PySmokeErr {
    /// pip 跑得起来，但读不了我们写的 pip.ini。处方：删配置，**别动 Python**。
    PipConfig(String),
    /// 运行时本身半损坏（截断的 tar 缺 encodings/pip 等）。处方：清理重装。
    Broken(String),
}

impl PySmokeErr {
    fn msg(&self) -> &str {
        match self {
            PySmokeErr::PipConfig(s) | PySmokeErr::Broken(s) => s,
        }
    }
}

/// 确保便携 Python 就绪（下载 python-build-standalone，自带 pip）。
/// 「装完自检 + 失败清理重下重解」的自愈循环：把一批客户机 Python bug 的共同根因——
/// 「静默留下半损坏运行时」（encodings/pip 缺失、截断 tar、代理缓存坏 SHA、tar.exe 缺失）——
/// 变成「发现即自修」。解压兜底见 extract_archive（tar.exe 缺失时走内置纯 Rust 解压器）。
fn ensure_python(skill: &Skill, on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<(), String> {
    // 已装：也跑一次自检。存量客户可能是上个版本留下的半损坏态（python.exe 在、Lib 不全），
    // 之前直接「已就绪」放行永远修不好 —— 现在自检不过就落到下面清理重装，顺手治好老机器。
    if portable_python_exe().is_some() {
        // 存量客户的便携 Python 可能没写过 pip 国内源（老版本装的）→ 补写，幂等。
        // 🔴 这一步必须排在自检**前面**：顺序反了的话，老版本写坏的那份 pip.ini 会让自检
        // 失败、被误判成解压损坏，于是先白删一次 Python 重下几十 MB，才轮到重写配置 ——
        // pc-*** 就是这么循环了一周。写配置是幂等的，提前没有副作用，还顺手治好老机器。
        write_portable_pip_config(&skill.pip_index, on_log);
        let mut verdict = python_smoke_ok();
        if let Err(PySmokeErr::PipConfig(e)) = &verdict {
            // 配置坏、Python 是好的：删掉配置再验一次，绝不落到下面的清理重装。
            on_log("out", &format!("pip 读不了这份配置，已移除后重试（不动 Python、不影响已装的包）：{e}"));
            remove_portable_pip_config();
            verdict = python_smoke_ok();
        }
        match verdict {
            Ok(()) => {
                on_log("out", "便携 Python 已就绪（自检通过）");
                return Ok(());
            }
            Err(e) => on_log(
                "out",
                &format!("便携 Python 已存在但自检未过（疑似上次解压损坏），清理后重装：{}", e.msg()),
            ),
        }
    }
    let spec = skill.python.as_ref().ok_or("skill 清单缺少 python 配置")?;
    let (url, sha) = spec.url_for_platform()?;

    let runtime = uking_home().join("runtime");
    std::fs::create_dir_all(&runtime).map_err(|e| format!("创建 runtime 目录失败: {e}"))?;
    let pkg = std::env::temp_dir().join("uking-python.tar.gz");
    let pkg_s = pkg.display().to_string();

    // 最多 3 轮：下载(带 cache-bust 绕坏缓存) → 清残留 → 解压 → 装完自检；任一步坏就整轮重来。
    const MAX_ATTEMPTS: u32 = 3;
    let mut last = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        let cache_bust = attempt - 1; // 首轮 0（正常下载），后续轮换 query 串绕开代理坏缓存
        if attempt == 1 {
            on_log("out", "下载便携 Python（含 pip，npmmirror 国内源）…");
        } else {
            on_log(
                "out",
                &format!("便携 Python 未就绪，第 {attempt}/{MAX_ATTEMPTS} 轮重新下载解压（绕开代理缓存）…"),
            );
        }
        // 便携 Python 实际 ~25MB+；5MB 兜底挡代理错误页/半包（清单没配 sha256 时也生效）。
        if let Err(e) =
            download_with_fallback(url, &pkg_s, &skill.mirror_fallback, sha, 5_000_000, cache_bust, on_log)
        {
            last = format!("下载 Python 失败: {e}");
            on_log("out", &last);
            continue;
        }
        on_log("out", "下载完成，解压中…");
        // 记下「校验通过那一刻」的大小 —— 解压失败时用它区分「下载坏了」和「杀软动了手」。
        let verified_len = std::fs::metadata(&pkg).map(|m| m.len()).unwrap_or(0);
        // python-build-standalone 解压出顶层目录就叫 python/
        // #142 修：解压前清掉可能残留的半截 python/（含被杀软锁住的 vcruntime140.dll），
        // 否则 tar 覆盖已存在的锁定文件会报「Can't unlink already-existing object」。
        clear_dir_before_extract(&runtime.join("python"), on_log);
        if let Err(e) = extract_archive(&pkg_s, &runtime) {
            last = match archive_tampered_after_verify(&pkg, verified_len) {
                Some(verdict) => format!("{verdict}（解压器原始报错：{e}）"),
                None => e,
            };
            on_log("out", &format!("解压未成功，将重试：{last}"));
            continue;
        }
        let _ = std::fs::remove_file(&pkg);
        // 装完自检：截断的 tar 会缺 encodings/pip，「文件存在」检查放行、跑起来才崩 —— 这里当场拦下。
        match python_smoke_ok() {
            Ok(()) => {
                // 把 pip 国内源永久写进便携 Python 根目录。否则 Hermes 运行时 lazy_deps.py
                // 第一次用 TTS/语音 等功能会 `pip install edge-tts` 走默认 PyPI → 国内卡死
                // （客户机实测：tts_tool.py→lazy_deps.py 卡在 subprocess.run，用户 Ctrl+C 才停）。
                // pip.ini 放在 python 根（sys.prefix），不在 site-packages，包重装也不会被冲掉。
                write_portable_pip_config(&skill.pip_index, on_log);
                // 🔴 写完配置**再验一次 pip**。老的顺序是「自检 → 写配置 → 报成功」，我们自己
                // 写坏的配置永远验不到 —— 于是日志上那句「已装好并通过自检」说的是上一秒的事实，
                // 下一秒 pip 就废了，而且要等下一次进来才暴露、还被误诊成解压损坏（pc-***：
                // 这句「通过自检」连打了一周，pip 一次都没能跑起来）。
                if let Err(PySmokeErr::PipConfig(e)) = python_smoke_ok() {
                    on_log("out", &format!("pip 读不了刚写的配置，已移除该配置（pip 仍可用，只是不走镜像）：{e}"));
                    remove_portable_pip_config();
                }
                on_log(
                    "out",
                    &format!("便携 Python 已装好并通过自检：{}", runtime.join("python").display()),
                );
                return Ok(());
            }
            Err(e) => {
                last = e.msg().to_string();
                on_log("out", &format!("便携 Python 自检未过，将清理重装：{last}"));
            }
        }
    }
    let _ = std::fs::remove_file(&pkg);
    // ⚠️ 别再把所有失败都往「网络/代理」上推。安装包带 SHA-256 校验，能走到解压这一步
    // 就说明**下载的字节是对的**；此后再失败，真凶几乎都在本机（杀软拦截 / 旧目录被占用）。
    // 老文案让客户去换网络关代理，白折腾还治不了病 —— issue #284/#286~#290 全是这么来的。
    Err(format!(
        "便携 Python 装好后自检始终未过（已重试 {MAX_ATTEMPTS} 轮）：{last}。\
         安装包过了 SHA-256 校验，说明下载没问题 —— 请优先排查本机：\
         ① 把 %TEMP% 和 ~/.uking 加入 360 / 火绒 / Defender 的信任区；\
         ② 关掉正在跑的 python / hermes 进程（旧目录被占用会导致解压覆盖失败）"
    ))
}

/// 给便携 Python 写 pip 国内源配置（Windows: pip.ini / unix: pip.conf，均在 sys.prefix）。
/// 让一切 pip 调用（含 Hermes 运行时 lazy_deps 的懒装）都走阿里云镜像，幂等覆盖。
/// pip 兜底源：主源（aliyun）偶发同步空档时——对某个包返回「from versions: none」，明明包在、
/// Python 版本也兼容，却一个版本都解析不到——pip 会自动去这些 extra 源找同一个包。全用国内可达镜像
/// （tsinghua/ustc/腾讯云均实测有货），**不放 pypi.org**（国内客户可能慢/不通，反拖慢）。对齐便携
/// Node/Python 早有的 mirror_fallback。实锤：pc-***（Issue #200，0.9.59）单源 aliyun 无兜底 →
/// Hermes 装不上，连 repair 强制重装也永败。它们都是同一 PyPI 的镜像，无依赖混淆风险。
const PIP_FALLBACK_INDEXES: &[&str] = &[
    "https://pypi.tuna.tsinghua.edu.cn/simple/",
    "https://mirrors.ustc.edu.cn/pypi/simple/",
    "https://mirrors.cloud.tencent.com/pypi/simple/",
];

/// 从 https 镜像 URL 提取 host（做 trusted-host 用）。
fn pip_index_host(url: &str) -> Option<&str> {
    url.strip_prefix("https://").and_then(|s| s.split('/').next())
}

pub fn write_portable_pip_config(pip_index: &str, on_log: &(dyn Fn(&str, &str) + Send + Sync)) {
    let index = if pip_index.starts_with("https://") {
        pip_index
    } else {
        "https://mirrors.aliyun.com/pypi/simple/"
    };
    // 从 index-url 提取 host 作为 trusted-host（https 镜像一般无需，但写上更稳）
    let host = pip_index_host(index).unwrap_or("mirrors.aliyun.com");
    // 兜底源也写进 pip.ini 的 extra-index-url —— 客户在便携 Python 里手敲 pip / 任何后续 pip 装
    // 都自动带兜底（不止 U-King 装机命令那一次），与命令行 --extra-index-url 双保险。
    let extra_urls = PIP_FALLBACK_INDEXES.join(" ");
    let mut trusted: Vec<&str> = vec![host];
    for m in PIP_FALLBACK_INDEXES {
        if let Some(h) = pip_index_host(m) {
            trusted.push(h);
        }
    }
    let trusted_hosts = trusted.join(" ");
    let base = uking_home().join("runtime").join("python");
    #[cfg(windows)]
    let cfg = base.join("pip.ini");
    #[cfg(not(windows))]
    let cfg = base.join("pip.conf");
    // wheelhouse 式离线韧性（借鉴竞品 LastAI）：把下载过的 wheel 落到 ~/.uking/cache/pip 常驻缓存，
    // 装第二个 Python 工具 / 修复重装时能直接复用、离线也装得上；prefer-binary 优先装预编译 wheel、
    // 不从源码 build —— 客户机没编译器时正是「现场 build 必败」的根因，指定它可绕开。
    let cache_dir = pip_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);
    let (body, dropped) = pip_config_body(index, &extra_urls, &trusted_hosts, &cache_dir.display().to_string());
    match std::fs::write(&cfg, body) {
        Ok(()) => {
            on_log("out", &format!("已为便携 Python 配置 pip 国内源（{host} + 兜底镜像）+ 本地 wheel 缓存"));
            if dropped > 0 {
                on_log("out", &format!("（pip 配置里 {dropped} 行含非 ASCII 字符已跳过，改用环境变量下发，避免 pip 读配置报错）"));
            }
        }
        Err(e) => on_log("err", &format!("写 pip 配置失败（不致命）：{e}")),
    }
}

/// 生成 pip.ini 正文，返回 `(正文, 被丢弃的行数)`。**纯函数，只为能被用例钉死这条不变量：
/// 正文必须是纯 ASCII。**
///
/// pip 读配置文件用的是系统 ANSI 代码页（`locale.getpreferredencoding()`，中文 Windows =
/// cp936），而我们写文件是 UTF-8。只要正文里有一个非 ASCII 字节，pip 就会对**每一次调用**
/// 报 `Configuration file contains invalid cp936 characters` 并退出 2 —— 不是某个包装不上，
/// 是这台机器上所有 pip 调用全废（pc-***：中文用户名进了 cache-dir（客户机用户名为中文），Hermes 连着
/// 一周装不上，还被自愈误判成解压损坏、反复删 Python 重下几十 MB）。
///
/// 因此：① cache-dir 只在路径纯 ASCII 时才写（非 ASCII 时缓存改由 `PIP_CACHE_DIR` 环境变量
/// 下发，见 `with_path`，功能不丢）；② 再逐行过一道闸门，兜住将来任何一条非 ASCII 混进来的路。
/// 丢一行配置顶多慢一点，丢掉整个 pip 就是工具永远装不上 —— 这笔账不用算。
fn pip_config_body(index: &str, extra_urls: &str, trusted_hosts: &str, cache_dir: &str) -> (String, usize) {
    let cache_line = if cache_dir.is_ascii() {
        format!("cache-dir = {cache_dir}\n")
    } else {
        String::new()
    };
    let raw = format!(
        "[global]\nindex-url = {index}\nextra-index-url = {extra_urls}\ntrusted-host = {trusted_hosts}\ndisable-pip-version-check = true\n{cache_line}\n[install]\nprefer-binary = true\n"
    );
    let mut body = String::new();
    let mut dropped = 0usize;
    for line in raw.lines() {
        if line.is_ascii() {
            body.push_str(line);
            body.push('\n');
        } else {
            dropped += 1;
        }
    }
    (body, dropped)
}

/// 删掉我们写的 pip 配置。用于「pip 读不了这份配置」时的自救：pip 没有配置照样能跑
/// （顶多不走国内镜像、慢一点），而带着一份读不了的配置则是**每一次 pip 调用都必败**。
/// 绝不因为配置坏就去删 Python —— 那既治不好病，还会把用户已装好的包一起清掉。
fn remove_portable_pip_config() {
    let base = uking_home().join("runtime").join("python");
    #[cfg(windows)]
    let cfg = base.join("pip.ini");
    #[cfg(not(windows))]
    let cfg = base.join("pip.conf");
    let _ = std::fs::remove_file(&cfg);
}

/// 便携 pip 的常驻 wheel 缓存目录（~/.uking/cache/pip）。下载过的依赖包留这，
/// 二次装 / 修复重装 / 离线时直接复用，不重下 —— wheelhouse 思路的落地。
fn pip_cache_dir() -> PathBuf {
    uking_home().join("cache").join("pip")
}

/// 本地 wheel 缓存里有没有可复用的 .whl（有 = 即便镜像连不上也可能装得上）。
/// 只浅扫 6 层够用（pip cache 结构 wheels/<hash 分桶>/...），不做全盘递归。
fn pip_cache_has_wheels() -> bool {
    fn any_whl(p: &Path, depth: u32) -> bool {
        if depth > 6 {
            return false;
        }
        let Ok(rd) = std::fs::read_dir(p) else {
            return false;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                if any_whl(&path, depth + 1) {
                    return true;
                }
            } else if path.extension().map(|x| x.eq_ignore_ascii_case("whl")).unwrap_or(false) {
                return true;
            }
        }
        false
    }
    any_whl(&pip_cache_dir(), 0)
}

/// 装前探 pip 依赖源可达（短超时、绕代理，与安装子进程同款清代理）。**必须用 HEAD（-I）而非 GET**：
/// pip simple 索引根页有 40MB+，GET 会把整包拉下来、-m 10 秒下不完 → 好网也误判连不上（实测 exit 28）。
/// HEAD 只探头、几十毫秒返回。curl 退出 0 = 已跟服务器握手并拿到响应头（HTTP 状态无所谓）。连不上返回 false。
fn pip_mirror_reachable(index_url: &str) -> bool {
    #[cfg(windows)]
    const NULL_DEVICE: &str = "NUL";
    #[cfg(not(windows))]
    const NULL_DEVICE: &str = "/dev/null";
    matches!(
        run_capture_raw(
            "curl",
            &["-sS", "-I", "-o", NULL_DEVICE, "--connect-timeout", "6", "-m", "10", index_url],
            None,
        ),
        Ok((0, _))
    )
}

/// 把便携 Python 的 Scripts 目录持久化进用户 PATH —— pip 装的 hermes.exe 在这。
/// app 自身探测靠 search_paths()（临时注入）不受影响，但客户在普通终端敲 `hermes`
/// 会找不到。2026-06-17 实测：pc-*** / pc-*** 两台都中——安装器只持久化了便携 Node，
/// 漏了 python/Scripts，得逐台远程手动补 PATH。这里在 pip 装完工具后补上（幂等，已有则跳过）。
/// Scripts 目录要等 pip 至少装过一个带脚本的包才存在，所以放在 PipInstall 步骤之后调。
fn persist_python_scripts_path(on_log: &(dyn Fn(&str, &str) + Send + Sync)) {
    // 沙箱里不许碰真实用户 PATH（对齐 ensure_cli_command_guard 的同款护栏）。
    // 实锤：`--install-test-cjk` 第一次跑就把沙箱的 Scripts 目录写进了开发机的用户 PATH，
    // 沙箱跑完即删 → 留下一条指向不存在目录的死路径。回归跑道自己污染真实状态，
    // 那它验出来的一切都不作数（宪法第 10 条：测试进沙箱，不碰用户真实状态）。
    if std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false) {
        on_log("out", "测试沙箱中跳过用户 PATH 修改（便携 Python Scripts）");
        return;
    }
    if let Some(scripts) = portable_python_scripts_dir() {
        match add_user_path(&scripts) {
            Ok(()) => on_log("out", &format!("已把 {} 加入用户 PATH（hermes 等工具全局可用）", scripts.display())),
            Err(e) => on_log("err", &format!("便携 Python Scripts 加入用户 PATH 失败（不致命）：{e}")),
        }
    }
}

/// 确保 Node 可用：系统 PATH 有就用系统的；否则下载便携版到 ~/.uking/runtime/node。
/// 把 CurrentUser 的 PowerShell 执行策略设成 RemoteSigned（免管理员，幂等）。
/// 修「无法加载 claude.ps1，因为在此系统上禁止运行脚本」这个 npm-on-PowerShell 通病。
#[cfg(windows)]
fn ensure_powershell_policy(on_log: &(dyn Fn(&str, &str) + Send + Sync)) {
    let ps = "if ((Get-ExecutionPolicy -Scope CurrentUser) -notin @('RemoteSigned','Unrestricted','Bypass')) { Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned -Force }";
    match run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", ps], None) {
        Ok((0, _)) => on_log("out", "已放开 PowerShell 脚本运行限制（CurrentUser RemoteSigned）"),
        _ => {} // 失败不致命，cmd 窗口照样能用 .cmd 启动器
    }
}

#[cfg(not(windows))]
fn ensure_powershell_policy(_on_log: &(dyn Fn(&str, &str) + Send + Sync)) {}

/// 装前环境预检 + 免提权自动修。返回 `{ ok, issues: [人话], fixed: [做了什么] }`。
///
/// 目前只治真实撞过的故障：客户手改系统环境变量弄丢 System32（pc-*** 实锤，
/// `C:\Windows\system32` 被写坏成 `C:\;indows\system32`）。U-King 自己的调用已全部
/// 绝对路径化免疫（见 `system_tool`），但**装好的工具**仍吃 PATH——claude.cmd 要
/// 找 node、客户自己开终端敲 codex 都靠它。修法免管理员：把 System32 追加进
/// **用户级** PATH（进程 PATH = Machine+User 拼接，效果等同），新开进程即生效。
/// 装前「脆弱环境」预检（借鉴竞品 LastAI 的 fragility-check）——只读、只报警，不自动改。
/// 这几样都真把安装/运行搞挂过，但都不能安全地自动修（改不了用户名、不该替用户搬 OneDrive、
/// 开长路径要管理员），所以只出「仍需注意」级提示交用户处置，不进 issues、不翻 ok。
#[cfg(windows)]
fn fragility_warnings() -> Vec<String> {
    let mut w: Vec<String> = vec![];

    // ① 数据目录落在 OneDrive 同步盘：AI 工具会装上万个小文件，OneDrive 实时上传会瞬时锁文件，
    //    rename/unlink 报「另一个程序正在使用此文件」，装到一半崩。
    let home_s = uking_home().display().to_string();
    if home_s
        .split(|c| c == '\\' || c == '/')
        .any(|seg| seg.to_ascii_lowercase().starts_with("onedrive"))
    {
        w.push(format!(
            "用户目录被 OneDrive 接管（{home_s}）：AI 工具装机涉及上万个小文件，OneDrive 实时同步可能锁文件导致中断。建议对 .uking 文件夹设「始终保留在此设备上」，或把用户目录移出 OneDrive。"
        ));
    }

    // ② 中文/非 ASCII 用户名：npm、pip 的默认前缀路径含用户名，非 ASCII 会让个别工具路径解析崩
    //    （claude 的坏 npmrc 前缀实锤，Windows/Mac 都中）。
    let username = std::env::var("USERNAME").unwrap_or_default();
    if username.bytes().any(|b| b >= 128) {
        w.push(format!(
            "Windows 用户名含中文/特殊字符（{username}）：个别 AI 工具的安装路径解析对非英文用户名不友好，可能装失败。若反复装不上，可新建纯英文名的本地账户再装。"
        ));
    }

    // ③ 长路径未开：深层 node_modules 常超 260 字符，未开时 npm 报路径过长（ENAMETOOLONG）。
    //    开它要写 HKLM（管理员），本函数被 `--selfcheck` 无头调用，**绝不能在这里弹 UAC** ——
    //    所以这里仍然只检测。但**不再把活推回给客户**：见 `repairable_ids()` 与 Wizard 里那颗按钮。
    //
    // 🔴 原来这条写的是「以管理员运行 reg add HKLM\... 后重启生效」—— 让客户去敲注册表命令。
    //    存量盘点里 23 台机器卡在这：**我们检测到了、也早就有修它的能力**
    //    （`airuntime_fix_elevated`，一次 UAC 开长路径 + 开发者模式，已 journal 可回滚），
    //    只是装机链路不认识它。★ **检测到了却把活推回给客户，等于没检测。**
    if long_paths_enabled() == Some(false) {
        w.push(
            "系统未开启「长路径支持」：npm 装深层依赖会报路径过长（ENAMETOOLONG）。U-King 可以替你开启（会弹一次管理员授权）。".into(),
        );
    }

    w
}

/// 预检里**我们自己能修**的项 → 稳定 id。给界面用来长出一颗真按钮，而不是让客户读一段命令。
///
/// 为什么单开一个字段而不是把 `warnings` 改成结构体：`warnings: string[]` 已经有消费方
/// （Wizard、`--selfcheck` 的 JSON、装机失败 triage），改形状要动一圈；加字段是向后兼容的。
/// 为什么返回 id 不返回中文：界面要 i18n，**按文案匹配会在翻译后当场失灵**。
///
/// id 与界面动作的对应关系写在 `Wizard.tsx` 那颗按钮上 —— 这里只回答「能不能修」，
/// 不回答「怎么修」（修的能力在 `airuntime`，前端组合，Rust 侧不互相 import，守四铁律）。
#[cfg(windows)]
fn repairable_ids() -> Vec<&'static str> {
    let mut r = vec![];
    if long_paths_enabled() == Some(false) {
        r.push("long_paths");
    }
    r
}

/// 读注册表 LongPathsEnabled：None=读不到，Some(true/false)=开/关。
#[cfg(windows)]
fn long_paths_enabled() -> Option<bool> {
    let (_, out) = run_capture_raw(
        "reg",
        &["query", "HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem", "/v", "LongPathsEnabled"],
        None,
    )
    .ok()?;
    parse_long_paths_query(&out)
}

/// 从 `reg query` 的输出里解出 LongPathsEnabled。**单独拆出来是为了能测** ——
/// 这里是「静默漏检」最可能藏身的地方：解析失败返回 `None`，于是警告不出、`repairable`
/// 也不出，客户**什么提示都看不到**，而日志里一切正常。★ 失败方向是假阴性，最难发现。
///
/// 真实输出形如（注意前面有空行和缩进）：
/// ```text
/// HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\FileSystem
///     LongPathsEnabled    REG_DWORD    0x1
/// ```
fn parse_long_paths_query(out: &str) -> Option<bool> {
    let line = out.lines().find(|l| l.contains("LongPathsEnabled"))?;
    let tok = line.split_whitespace().last()?;
    let v = tok.strip_prefix("0x").and_then(|h| i64::from_str_radix(h, 16).ok())?;
    Some(v == 1)
}

pub fn env_precheck_and_fix() -> serde_json::Value {
    #[cfg(windows)]
    {
        let mut issues: Vec<String> = vec![];
        let mut fixed: Vec<String> = vec![];
        // System32 关键组件本体是否存在（缺文件 = 系统级损坏，修不了只能说人话）
        for t in ["curl", "tar", "cmd"] {
            let p = system_tool(t);
            if !std::path::Path::new(&p).exists() {
                issues.push(format!("Windows 自带组件缺失：{p}（系统本身异常，建议系统修复/重装）"));
            }
        }
        // PATH 是否含 System32；缺则追加到用户级 PATH（SetEnvironmentVariable 自带广播）
        let ps = "$sys=Join-Path $env:SystemRoot 'System32'; \
$m=[Environment]::GetEnvironmentVariable('Path','Machine'); if($null -eq $m){$m=''}; \
$u=[Environment]::GetEnvironmentVariable('Path','User'); if($null -eq $u){$u=''}; \
$entries=($m+';'+$u).Split(';') | ForEach-Object { $_.Trim().TrimEnd('\\') }; \
if($entries -contains $sys){'PATH_OK'} else { try { [Environment]::SetEnvironmentVariable('Path',($u.TrimEnd(';')+';'+$sys),'User'); 'FIXED_USER_PATH' } catch { 'FIX_FAILED' } }";
        match run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", ps], None) {
            Ok((_, out)) => {
                if out.contains("FIXED_USER_PATH") {
                    fixed.push("系统 PATH 缺少 System32（常见于手动改过环境变量），已自动补进用户 PATH；新开的终端/应用生效".into());
                } else if out.contains("FIX_FAILED") {
                    issues.push("系统 PATH 缺少 System32 且自动修复失败：请在「编辑系统环境变量」给 Path 补上 C:\\Windows\\System32".into());
                }
            }
            Err(e) => issues.push(format!("环境预检没跑起来：{e}")),
        }
        // fragility 警告不进 issues、不翻 ok —— 只是「仍需注意」级建议，不阻断安装。
        let warnings = fragility_warnings();
        // `repairable`：这几项我们自己能修，界面据此长出真按钮（见 repairable_ids 的注释）。
        let repairable = repairable_ids();
        serde_json::json!({ "ok": issues.is_empty(), "issues": issues, "fixed": fixed, "warnings": warnings, "repairable": repairable })
    }
    #[cfg(not(windows))]
    {
        serde_json::json!({ "ok": true, "issues": [], "fixed": [], "warnings": [], "repairable": [] })
    }
}

/// 便携 Node 装完自检（与 python_smoke_ok 同款理念）：真跑 node -v + npm -v。
/// 截断的 .zip 会让 node.exe 不完整（跑不起来）或 node_modules/npm 缺（node 过、npm 挂），
/// 「文件存在」检查会误放行，等装 claude/codex 时才崩 —— 这里当场发现 → 上层清理重装。
/// 只测我们自己的便携 Node（portable_node_dir），绝不碰用户系统 Node。
fn node_smoke_ok() -> Result<(), String> {
    let Some(dir) = portable_node_dir() else {
        return Err("未找到便携 node".into());
    };
    match run_capture("node --version", Some(&dir)) {
        Ok((0, _)) => {}
        Ok((_c, out)) => return Err(format!("node 自检未过（解压不完整）：{}", tail(&out, 200))),
        Err(e) => return Err(format!("无法运行 node 自检：{e}")),
    }
    // 装 claude/codex 全靠 npm；node_modules/npm 被截断时 node -v 过但 npm 挂 → 提前拦下。
    match run_capture("npm --version", Some(&dir)) {
        Ok((0, _)) => Ok(()),
        Ok((_c, out)) => Err(format!("npm 自检未过（解压不完整）：{}", tail(&out, 200))),
        Err(e) => Err(format!("无法运行 npm 自检：{e}")),
    }
}

pub fn ensure_node(skill: &Skill, on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<(), String> {
    ensure_node_min(skill, None, on_log)
}

/// 「AI 优化大师指出的缺件」四件套的安装结果。每项只有三种取值：
/// `ok` / `skip`（**这一步在这个平台不存在**，不是成功也不是失败）/ `fail: <原因>`。
#[derive(Serialize, Deserialize)]
pub struct EnvToolsResult {
    pub node: String,
    pub git: String,
    pub pwsh: String,
    pub command_guard: String,
}

/// 把优化大师指出的缺件真正装上（而不只是提示去别处装）：
/// 便携 Node + 便携 Git（含 bash.exe，Claude Code 的 Bash 工具刚需；pc-*** 实锤缺 git 就用不了）
/// + 便携 PowerShell 7（客户机常只有 5.1，中文易乱码）+ CLI 命令优先级守卫。
/// 三者都免管理员、便携落 `~/.uking/runtime`，**best-effort：一个失败不拖另一个**。
///
/// 影核动作 `runtime.env.install_tools` 和老命令 `optimize_env` 都调这一份，
/// 这里是唯一实现（宪法第 13 条）。
///
/// 🔴 **住在 installer.rs 而不是 lib.rs**：它一个外部模块都不跨，纯粹是 `ensure_*` 的编排 ——
/// 按四铁律第 1 条（模块只暴露纯函数，`#[tauri::command]` 写在 lib.rs 转调）它该在这儿。
/// 首版顺手写进了 lib.rs，那正是「所有耦合被挤进同一个文件」这个病的又一小口
/// （lib.rs 已经是全项目最容易打架的一点，拆它在预算清单上）。
///
/// 进度回调故意用裸 `&(dyn Fn(&str) + Send + Sync)` 而不是 `actions::ProgressSink`：
/// 那样 installer 就要反向依赖协议层，而依赖方向只能「新模块 → 老的公共助手」。
/// 两者结构完全一致，调用点直接传得进去。
pub fn install_env_tools(progress: &(dyn Fn(&str) + Send + Sync)) -> EnvToolsResult {
    let log = |_stream: &str, msg: &str| progress(msg);
    let skill = load_skill();
    let node = match ensure_node(&skill, &log) {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("fail: {e}"),
    };
    let git = match ensure_git(&log) {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("fail: {e}"),
    };
    // PS7：ensure_pwsh 先探系统/便携已装（find_pwsh），有就秒回；缺才下便携版（~106MB，一次性）。
    // Windows 专属（Mac 的 pwsh 由 brew/系统管，不便携装 → skip 不计入失败）。
    #[cfg(windows)]
    let pwsh = match ensure_pwsh(&log, false) {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("fail: {e}"),
    };
    #[cfg(not(windows))]
    let pwsh = "skip".to_string();
    // CLI 命令优先级守卫是 Windows 专属（转发 .cmd 壳、改用户 PATH）；非 Windows 的实现是
    // 空 no-op。它回 Ok 就报「✔ 就绪」等于在 Mac 上宣布一件**从没发生过的事**做成了 ——
    // 和 pwsh 一样如实报 skip。
    let command_guard = match ensure_cli_command_guard(&log) {
        Ok(()) => (if cfg!(windows) { "ok" } else { "skip" }).to_string(),
        Err(e) => format!("fail: {e}"),
    };
    EnvToolsResult { node, git, pwsh, command_guard }
}

/// `ensure_node` 加一条「这个工具跑得起来所需的最低版本」。
/// `min` 为 None 时行为与老 `ensure_node` 逐字节一致。
pub fn ensure_node_min(
    skill: &Skill,
    min: Option<&str>,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<(), String> {
    let portable_dir = portable_node_dir();
    if let Ok((0, v)) = run_capture("node --version", portable_dir.as_deref()) {
        let required = skill.node.version.trim_start_matches('v');
        let current = v.trim().trim_start_matches('v');
        // 现有 Node（系统的或便携的）够不够这个工具跑：不够就往下走去装便携版。
        // **不动用户自己的 Node**，只是自己带一份并前置进 PATH —— 同中文路径下自带 Python 的做法。
        let too_old_for_tool = min
            .map(|m| semver_gt(m.trim_start_matches('v'), current))
            .unwrap_or(false);
        if too_old_for_tool {
            on_log(
                "out",
                &format!(
                    "当前 Node.js {} 低于本工具要求的 v{}，改用 U-King 自带的便携版（不改动你自己的 Node）…",
                    v.trim(),
                    min.unwrap_or("").trim_start_matches('v')
                ),
            );
        }
        // 只在「已就绪的是我们自己管理的便携版」且版本低于清单当前要求时才强制升级重装：
        // 清单后来把 node.version 提高了（如某工具改吃 22.19+），但老客户机上更早批次装的
        // 便携 Node 永远不会被重新检查版本，导致「Node 已就绪」判断永远通过、装出来的工具
        // 一验证就报 "Node.js vX+ is required"（issue #75 实锤）。系统级 Node（非我们装的）
        // 版本旧不在此列——那是用户自己的环境，我们没有理由去覆盖它。
        let stale_portable = portable_dir.is_some() && semver_gt(required, current);
        if !stale_portable && !too_old_for_tool {
            // 我们自己的便携 Node：顺带自检，半损坏（上次截断解压）就落到下面清理重装，顺手治老机器。
            // 系统 Node（portable_dir 为空）只信不碰：不自检、不重装。
            if portable_dir.is_some() {
                match node_smoke_ok() {
                    Ok(()) => {
                        on_log("out", &format!("Node.js 已就绪（自检通过）：{}", v.trim()));
                        return Ok(());
                    }
                    Err(e) => on_log(
                        "out",
                        &format!("便携 Node 已存在但自检未过（疑似上次解压损坏），清理后重装：{e}"),
                    ),
                }
            } else {
                on_log("out", &format!("Node.js 已就绪：{}", v.trim()));
                return Ok(());
            }
        } else {
            on_log("out", &format!("便携 Node.js 版本过旧（{} < 需要 {}），重新下载新版…", v.trim(), skill.node.version));
        }
    } else {
        on_log("out", "未检测到 Node.js，开始下载便携版（npmmirror 国内源）…");
    }
    let runtime = uking_home().join("runtime");
    std::fs::create_dir_all(&runtime).map_err(|e| format!("创建 runtime 目录失败: {e}"))?;

    let (url, dir_name, sha) = skill.node.for_platform()?;
    let pkg = std::env::temp_dir().join(if cfg!(windows) { "uking-node.zip" } else { "uking-node.tar.gz" });
    let pkg_s = pkg.display().to_string();

    // 与 ensure_python 同款自愈循环：最多 3 轮 下载(带 cache-bust 绕坏缓存) → 清残留 → 解压
    // → 整理目录(rename 抗杀软锁) → 装完自检；任一步坏就整轮重来。
    const MAX_ATTEMPTS: u32 = 3;
    let mut last = String::new();
    let mut installed_bin: Option<PathBuf> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        let cache_bust = attempt - 1; // 首轮 0（正常下载），后续轮换 query 串绕开代理坏缓存
        if attempt > 1 {
            on_log(
                "out",
                &format!("便携 Node 未就绪，第 {attempt}/{MAX_ATTEMPTS} 轮重新下载解压（绕开代理缓存）…"),
            );
        }
        // 便携 Node 实际 ~28MB(win)/40MB(mac)；5MB 兜底挡代理错误页/半包（清单没配 sha256 时也生效）。
        if let Err(e) =
            download_with_fallback(url, &pkg_s, &skill.mirror_fallback, sha, 5_000_000, cache_bust, on_log)
        {
            last = format!("下载 Node 失败: {e}");
            on_log("out", &last);
            continue;
        }
        on_log("out", "下载完成，解压中…");
        // 同 Python：记下校验通过那一刻的大小，好把「下载坏了」和「杀软动了手」分开。
        let verified_len = std::fs::metadata(&pkg).map(|m| m.len()).unwrap_or(0);
        // 清掉上一轮可能残留的半截解压目录（node-vX-...），避免与本轮解压叠加脏文件。
        clear_dir_before_extract(&runtime.join(dir_name), on_log);
        if let Err(e) = extract_archive(&pkg_s, &runtime) {
            last = match archive_tampered_after_verify(&pkg, verified_len) {
                Some(verdict) => format!("{verdict}（解压器原始报错：{e}）"),
                None => e,
            };
            on_log("out", &format!("解压未成功，将重试：{last}"));
            continue;
        }
        // node-v22.x-<platform> → node（固定目录名，PATH 永久有效）。刚解压的文件常被杀软实时扫描
        // 瞬时锁住，remove_dir_all/rename 一次性失败会报「目录不是空的 (os error 145)」（issue #50）；退避重试。
        let extracted = runtime.join(dir_name);
        let target = runtime.join("node");
        let mut rename_err = None;
        for a2 in 0..10 {
            if target.exists() {
                let _ = std::fs::remove_dir_all(&target);
            }
            match std::fs::rename(&extracted, &target) {
                Ok(()) => {
                    rename_err = None;
                    break;
                }
                Err(e) => {
                    rename_err = Some(e);
                    if a2 < 9 {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }
            }
        }
        if let Some(e) = rename_err {
            last = format!("整理 Node 目录失败: {e}");
            on_log("out", &last);
            continue;
        }
        let _ = std::fs::remove_file(&pkg);
        // 装完自检：截断的 zip 让 node/npm 跑不起来时当场拦下 → 清理重装。
        match node_smoke_ok() {
            Ok(()) => {
                installed_bin = Some(portable_node_dir().unwrap_or(target));
                break;
            }
            Err(e) => {
                last = e;
                on_log("out", &format!("便携 Node 自检未过，将清理重装：{last}"));
                clear_dir_before_extract(&runtime.join("node"), on_log);
            }
        }
    }
    let Some(bin_dir) = installed_bin else {
        let _ = std::fs::remove_file(&pkg);
        return Err(format!(
            "便携 Node 装好后自检始终未过（已重试 {MAX_ATTEMPTS} 轮）：{last}。多为下载被代理缓存损坏或本机 Windows 组件异常，请换网络/关代理后重试"
        ));
    };

    // 持久化 PATH（Windows 写 HKCU；macOS 追加 ~/.zshrc），新开终端即可直接用
    add_user_path(&bin_dir)?;
    // 当前进程 PATH 立刻前置，好让紧随其后的「重新体检」/验证子进程看到新 Node（不必等重启）。
    prepend_process_path(std::slice::from_ref(&bin_dir));
    on_log(
        "out",
        &format!("便携 Node 已装好并通过自检：{}（已加入用户 PATH）", bin_dir.display()),
    );
    Ok(())
}

// 便携 Git（PortableGit，Git for Windows 官方绿色版，含 cmd/git.exe + bin/bash.exe）。
// 主源 npmmirror 国内直连；SHA-256 挡代理缓存损坏/半包（实测 62954096 字节）。备用源走
// skill.mirror_fallback（阿里云深圳 OSS 的 runtimes 目录，需同名上传 PortableGit-*.7z.exe）。
#[cfg(windows)]
const GIT_URL: &str = "https://registry.npmmirror.com/-/binary/git-for-windows/v2.47.1.windows.1/PortableGit-2.47.1-64-bit.7z.exe";
#[cfg(windows)]
const GIT_SHA: &str = "4f3f21f4effcb659566883ee1ed3ae403e5b3d7a0699cee455f6cd765e1ac39c";

/// 确保 git + bash 可用 —— 缺则下载便携 PortableGit 解压到 ~/.uking/runtime/git（免管理员）。
/// Claude Code 的 Bash 工具在 Windows 上刚需 git-bash；装机向导当年只装 node、绕开 git，这里补上。
/// 非 Windows（Mac）的 git 走 Xcode Command Line Tools，不便携装 → 直接返回 Ok（由前端引导 xcode-select）。
#[cfg(windows)]
pub fn ensure_git(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<(), String> {
    // ① 我们自己装过的便携 Git 已在 → 跳过。
    if !portable_git_dirs().is_empty() {
        on_log("out", "便携 Git 已就绪（含 Bash）");
        return Ok(());
    }
    // ② 系统已装 Git for Windows（含 bash）→ 尊重用户环境，不重复装。
    if Path::new("C:\\Program Files\\Git\\bin\\bash.exe").exists()
        || Path::new("C:\\Program Files (x86)\\Git\\bin\\bash.exe").exists()
    {
        on_log("out", "系统已装 Git（含 Bash），跳过");
        return Ok(());
    }
    on_log("out", "未检测到 Git，开始下载便携 Git（npmmirror 国内源，~60MB，免管理员）…");
    let runtime = uking_home().join("runtime");
    std::fs::create_dir_all(&runtime).map_err(|e| format!("创建 runtime 目录失败: {e}"))?;
    let dest = runtime.join("git");

    let pkg = std::env::temp_dir().join("uking-portablegit.7z.exe");
    let pkg_s = pkg.display().to_string();
    let skill = load_skill();
    // 便携 Git ~60MB；50MB 兜底挡代理错误页/半包（SHA 已核对确切字节）。
    download_with_fallback(GIT_URL, &pkg_s, &skill.mirror_fallback, GIT_SHA, 50_000_000, 0, on_log)
        .map_err(|e| format!("下载便携 Git 失败: {e}"))?;
    on_log("out", "下载完成，解压中（7-zip 自解压，静默）…");

    // 旧的半包目录先清掉，避免 SFX 往里叠加脏文件。
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }
    std::fs::create_dir_all(&dest).ok();
    // Git for Windows 的 PortableGit 是 7-zip 自解压 exe：`-y`(全部确认) `-o<目录>`(静默解压)。
    // run_capture_raw 是 argv 数组直传（不走 shell）→ 路径含空格也不用引号，且注入 CREATE_NO_WINDOW。
    let o_arg = format!("-o{}", dest.display());
    let (code, out) = run_capture_raw(&pkg_s, &["-y", &o_arg], None)?;
    let git_exe = dest.join("cmd").join("git.exe");
    let bash_exe = dest.join("bin").join("bash.exe");
    if !git_exe.exists() || !bash_exe.exists() {
        return Err(format!(
            "便携 Git 解压后未找到 git.exe / bash.exe（退出码 {code}）：{}",
            tail(&out, 200)
        ));
    }
    let _ = std::fs::remove_file(&pkg);
    // 写用户 PATH：cmd（git）+ bin（bash），新开终端即可直接用；search_paths 也已注入这两目录。
    let cmd_dir = dest.join("cmd");
    let bin_dir = dest.join("bin");
    let _ = add_user_path(&cmd_dir);
    let _ = add_user_path(&bin_dir);
    // 当前进程 PATH 立刻前置，好让紧随其后的「重新体检」（ukrt 子进程继承本进程 env）看到新 git/bash。
    prepend_process_path(&[cmd_dir, bin_dir]);
    on_log("out", &format!("便携 Git 已装好：{}（git + bash 已加入 PATH）", dest.display()));
    Ok(())
}

/// 非 Windows（Mac）：git 走 Apple 官方命令行工具，不便携装。缺 git 就唤起 `xcode-select --install`
/// 的系统弹窗（用户点「安装」即可，不用 sudo 密码、不用 Homebrew）—— 这就是 Mac 上的「官方方法」。
#[cfg(not(windows))]
pub fn ensure_git(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<(), String> {
    if matches!(run_capture("git --version", None), Ok((0, _))) {
        on_log("out", "Git 已就绪（Xcode 命令行工具）");
        return Ok(());
    }
    on_log("out", "未检测到 Git —— 正在唤起 Apple「命令行开发者工具」安装弹窗，请在弹窗里点『安装』…");
    let _ = std::process::Command::new("xcode-select").arg("--install").status();
    Ok(())
}

/// 内置纯 Rust 解压 .tar.gz —— 不依赖 System32\tar.exe，也不依赖 .NET/PowerShell，
/// 救 tar.exe 缺失/损坏的客户机（issue #182/#198：报「系统找不到指定的文件」os error 2，
/// 多为 System32 组件被删/损坏，我们改不了客户系统，但可以不依赖它）。
/// 截断/损坏的包会在这里报错 → 上层 ensure_python 的「重试 + 装完自检」兜住。
#[cfg(windows)]
fn extract_tar_gz_rs(pkg: &str, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(pkg).map_err(|e| format!("打开压缩包失败: {e}"))?;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(f));
    let mut ar = tar::Archive::new(gz);
    ar.set_overwrite(true); // 覆盖 tar.exe 可能已写一半的残留文件
    ar.set_preserve_permissions(false); // Windows 无 unix 权限位，跳过
    ar.set_preserve_mtime(false); // 不关心时间戳，省一次 IO
    // 逐条解，为的是**跳过 .pdb**（见 skip_in_archive）。整包 unpack 做不到这件事。
    let entries = ar.entries().map_err(|e| format!("内置解压器解压失败: {e}"))?;
    for entry in entries {
        let mut e = entry.map_err(|e| format!("内置解压器解压失败: {e}"))?;
        let skip = e.path().map(|p| skip_in_archive(&p)).unwrap_or(false);
        if skip {
            continue;
        }
        e.unpack_in(dest).map_err(|e| format!("内置解压器解压失败: {e}"))?;
    }
    Ok(())
}

/// 这个条目要不要跳过。目前只有一类：**`.pdb` 调试符号**。
///
/// 为什么值得单独判一下（issue #284/#286/#288/#324，0.9.80 一天内 9 条）：
/// 客户机上内置解压器失败的文件**总是 `.pdb`** —— `_asyncio.pdb`、`_elementtree.pdb`、
/// `_ssl.pdb`。它们是 python-build-standalone 附带的调试符号，**跑 Python 一个字节都用不到**，
/// 却在本机实测里占 40 个文件 / 61.5 MB：写盘更久 = 被杀软实时扫描盯上的窗口更长，
/// 而且 `.pdb` 正是杀软/EDR 最爱扫的那类文件。
///
/// 跳过它们是纯赚：少写 61 MB、少一整类失败点，运行时功能完全不变
/// （真要调试 Python 崩溃栈的人不会用我们这份便携运行时）。
///
/// ⚠️ 这**治不了**「包被整个删掉」那一类（`Failed to open …uking-python.tar.gz`）——
/// 那是杀软把 %TEMP% 里的包隔离了，得靠 archive_tampered_verdict 那条诊断去指认。
fn skip_in_archive(path: &Path) -> bool {
    path.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdb"))
}

#[cfg(windows)]
fn extract_archive(pkg: &str, dest: &Path) -> Result<(), String> {
    // .zip 用 Expand-Archive；.tar.gz（Python 便携版）用 Win10+ 自带的 tar.exe
    let dest_s = dest.display().to_string();
    // 用全路径的系统 bsdtar（System32\tar.exe）+ 正斜杠路径。
    // 不能用裸 "tar"：从 Git Bash 启动时 PATH 里 git 的 GNU/MSYS tar 会抢先，
    // 它把含盘符的 -C 路径当成 host:path（报 Cannot connect to C:）。
    // ★ zip 也走 tar.exe（Win10 1803+ 自带 bsdtar 能解 zip）：客户机 PowerShell
    //   可能是老版本（没有 Expand-Archive cmdlet）/ .NET TLS 太老，Expand-Archive
    //   会吐 `ServicePointManager` + 中文乱码错。tar.exe 不依赖 PowerShell 版本/TLS。
    let dest_fwd = dest_s.replace('\\', "/");
    let pkg_fwd = pkg.replace('\\', "/");
    let systar = std::env::var("SystemRoot")
        .map(|r| format!("{r}\\System32\\tar.exe"))
        .unwrap_or_else(|_| "C:\\Windows\\System32\\tar.exe".into());
    // zip 用 -xf（bsdtar 按魔数自动识别），tar.gz 用 -xzf。
    let mode = if pkg.ends_with(".zip") { "-xf" } else { "-xzf" };
    // ① 优先系统 bsdtar（健康机器最快最稳）。tar.exe 可能整个缺失（客户机 Windows 组件被删/损坏，
    // issue #145/#147/#182/#198：报「系统找不到指定的文件」os error 2）——那种情况 run_capture_raw
    // 返回 Err，别直接失败，落到下面兜底。
    // .tar.gz 跳过 .pdb 调试符号（理由见 skip_in_archive；内置解压器那条路也一样跳）。
    // zip 不加这个参数：Node 的 zip 里本来就没有 .pdb，少传一个参数少一分兼容风险。
    let mut tar_args: Vec<&str> = vec![mode, &pkg_fwd, "-C", &dest_fwd];
    if !pkg.ends_with(".zip") {
        tar_args.push("--exclude=*.pdb");
    }
    let tar_err = match run_capture_raw(&systar, &tar_args, None) {
        Ok((0, _)) => return Ok(()),
        Ok((_code, out)) => tail(&out, 250),
        Err(e) => e,
    };
    // ② .tar.gz（便携 Python）：系统 tar 失败/缺失 → 内置纯 Rust 解压器兜底。
    //    不依赖 System32\tar.exe，也不依赖 .NET/PowerShell（Expand-Archive 只解 .zip 且老 .NET 也会炸），
    //    是 tar.exe 缺失机器上装 Python/Hermes 的唯一救命路（issue #182/#198）。
    if !pkg.ends_with(".zip") {
        return match extract_tar_gz_rs(pkg, dest) {
            Ok(()) => Ok(()),
            Err(e2) => Err(format!(
                "解压失败：系统 tar 未成功（{tar_err}）；内置解压器也失败（{e2}）—— 若安装包已过 SHA-256 校验，\
                 病根就不在下载：优先看杀毒软件是否删/锁了 %TEMP% 里的包，以及旧运行时目录是否被占用"
            )),
        };
    }
    // ③ .zip（便携 Node）：tar.exe 失败才回退 PowerShell Expand-Archive，
    //    且先强制 TLS1.2/1.3 再解，避免老 PowerShell 默认 TLS1.0 的连带报错。
    let ps = format!(
        "[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; \
         Expand-Archive -Force -LiteralPath '{pkg}' -DestinationPath '{dest_s}'"
    );
    let (c2, o2) =
        run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", &ps], None)?;
    if c2 == 0 {
        return Ok(());
    }
    Err(format!("解压失败（tar+ps 均失败）：{tar_err} / {}", tail(&o2, 200)))
}

#[cfg(not(windows))]
fn extract_archive(pkg: &str, dest: &Path) -> Result<(), String> {
    let (code, out) = run_capture_raw("tar", &["-xzf", pkg, "-C", &dest.display().to_string()], None)?;
    if code != 0 {
        return Err(format!("解压 Node 失败：{}", tail(&out, 300)));
    }
    Ok(())
}

/// 把目录前置进**当前进程**的 PATH —— 让紧接着 spawn 的子进程（如 ukrt 重新体检、验证命令）
/// 立刻看到刚装好的便携工具，不必等 U-King 重启。用户级 PATH（add_user_path）已持久化，但
/// 已运行进程的 env 是启动时快照 —— 否则「一键优化装完 git，重新体检还说缺 git」（分数不涨）。
pub fn prepend_process_path(dirs: &[PathBuf]) {
    let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    if dirs.is_empty() {
        return;
    }
    let cur = std::env::var("PATH").unwrap_or_default();
    let add = dirs.join(PATH_SEP);
    let next = if cur.is_empty() { add } else { format!("{add}{PATH_SEP}{cur}") };
    std::env::set_var("PATH", next);
}

/// 把目录置顶到用户 PATH（Windows）。不同于 add_user_path 的"追加"：CLI 同名冲突时，
/// 只有置顶才能保证新开的 PowerShell / Codex 终端先命中 U-King 的可信转发器。
///
/// 不使用 setx（会截断超长 PATH），通过 .NET API 原子写 CurrentUser。路径来自本机
/// `~/.uking`，但仍转义单引号，避免罕见的用户名字符破坏 PowerShell 字符串。
#[cfg(windows)]
pub fn prepend_user_path(dir: &Path) -> Result<(), String> {
    let d = dir.display().to_string().replace('\'', "''");
    let ps = format!(
        r#"$p=[Environment]::GetEnvironmentVariable('Path','User'); if($null -eq $p){{$p=''}}; $d='{d}'; $rest=@($p -split ';' | Where-Object {{ $_ -and $_ -ne $d }}); [Environment]::SetEnvironmentVariable('Path', (($d)+$(if($rest.Count){{';'+($rest -join ';')}}else{{''}})), 'User')"#
    );
    let (code, out) = run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", &ps], None)?;
    if code == 0 {
        Ok(())
    } else {
        Err(format!("写入用户 PATH 失败：{}", tail(&out, 200)))
    }
}

/// AI 优化大师的「CLI 命令优先级守卫」。
///
/// Windows 会把 PATH 前面目录中的无扩展名 Bash 脚本也当作 Application；例如 Git Bash
/// 的 `~/bin/claude` 会抢在 `%APPDATA%\\npm\\claude.cmd` 前，表现为 Claude 已安装但
/// PowerShell / Codex 内嵌终端静默退出。这里不删除、不改用户原文件，而是在
/// `~/.uking/shims` 建立只指向真实 `.cmd` / `.exe` 的小转发器，并把该目录置顶。
///
/// 同一机制覆盖 Claude、Codex、OpenClaw、Hermes，避免每个 CLI 再各踩一次同名启动器坑。
#[derive(Serialize)]
pub struct CommandGuardCommand {
    pub name: String,
    pub preferred_path: Option<String>,
    pub resolved_path: Option<String>,
    pub shadowed: bool,
}

#[derive(Serialize)]
pub struct CommandGuardInspection {
    pub platform: String,
    pub shims_dir: String,
    pub conflicts: usize,
    pub commands: Vec<CommandGuardCommand>,
}

/// 只读检查：真实 shell 会解析到什么，以及 U-King 认可的 `.cmd/.exe` 是否被同名脚本抢占。
/// 这个结果是 GUI、CLI 与未来 MCP 影子共用的 canonical State，绝不创建 shim 或改 PATH。
pub fn inspect_cli_command_guard() -> CommandGuardInspection {
    #[cfg(windows)]
    {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        let user_bin = Path::new(&home).join("bin");
        let user_local_bin = Path::new(&home).join(".local").join("bin");
        let mut sources = Vec::new();
        if let Some(node) = portable_node_dir() {
            sources.push(node);
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            sources.push(Path::new(&appdata).join("npm"));
        }
        // 不调用 where.exe：客户机 PATH 中可能有断开的网络盘，where 会逐目录访问并把
        // 体检卡到分钟级。Action Core 只扫描已在内存的 PATH 字符串和确定文件路径。
        let path_dirs = std::env::var("PATH")
            .unwrap_or_default()
            .split(';')
            .filter(|p| !p.trim().is_empty())
            .map(|p| PathBuf::from(p.trim_matches('"')))
            .collect::<Vec<_>>();
        let path_position = |dir: &Path| {
            path_dirs
                .iter()
                .position(|p| p.to_string_lossy().eq_ignore_ascii_case(&dir.to_string_lossy()))
                .unwrap_or(usize::MAX)
        };
        let commands = ["claude", "codex", "openclaw", "hermes"]
            .into_iter()
            .map(|name| {
                let preferred = sources.iter().find_map(|dir| {
                    [".cmd", ".exe"]
                        .iter()
                        .map(|ext| dir.join(format!("{name}{ext}")))
                        .find(|p| p.is_file())
                });
                let user_script = [&user_bin, &user_local_bin]
                    .into_iter()
                    .map(|dir| dir.join(name))
                    .find(|p| {
                        p.is_file()
                            && std::fs::read(p).ok().is_some_and(|bytes| bytes.starts_with(b"#!"))
                    });
                // 只认用户 ~/bin 里的 Unix shebang 转发器抢占可信 `.cmd/.exe` 的确定性
                // 冲突；npm 同目录的 Git-Bash 辅助脚本与 U-Claw 便携启动器都不会误报。
                let shadowed = match (&preferred, &user_script) {
                    (Some(expected), Some(script)) => {
                        let script_parent = script.parent().map(path_position).unwrap_or(usize::MAX);
                        let expected_parent = expected.parent().map(path_position).unwrap_or(usize::MAX);
                        script_parent < expected_parent
                    }
                    _ => false,
                };
                let resolved = if shadowed { user_script } else { preferred.clone() };
                CommandGuardCommand {
                    name: name.into(),
                    preferred_path: preferred.map(|p| p.display().to_string()),
                    resolved_path: resolved.map(|p| p.display().to_string()),
                    shadowed,
                }
            })
            .collect::<Vec<_>>();
        let conflicts = commands.iter().filter(|c| c.shadowed).count();
        return CommandGuardInspection {
            platform: "windows".into(),
            shims_dir: uking_home().join("shims").display().to_string(),
            conflicts,
            commands,
        };
    }

    // Unix：没有 `.cmd/.exe` 之分，也没有 shim 机制，但「PATH 里谁排在前面谁赢」这个
    // 问题同样成立，而且比 Windows 更好答 —— 按 `:` 切 PATH 顺着找第一个可执行文件，
    // 那就是 shell 真正会跑的东西。
    //
    // 🔴 这里以前是个返回 `commands: []` + `conflicts: 0` 的空桩。空桩比「未实现」更糟：
    // 调用方（包括读说明书的 AI）会把 `conflicts: 0` 读成「体检通过，无冲突」，
    // 而实际上四个 CLI 装没装、被谁抢占，一个字都没查。
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;

        let is_exec = |p: &Path| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        };

        // 同 Windows 分支：不调用 `which`，只扫已在内存的 PATH 字符串，
        // 免得客户机上挂掉的网络盘把体检拖到分钟级。
        let path_dirs = std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        // U-King 自己管的便携 Node 的 bin/ —— 它装的全局包启动器落在这儿。
        let managed_dir = portable_node_dir();

        let commands = ["claude", "codex", "openclaw", "hermes"]
            .into_iter()
            .map(|name| {
                // shell 会解析到的那一个：PATH 里第一个命中。
                let resolved = path_dirs.iter().map(|d| d.join(name)).find(|p| is_exec(p));
                // U-King 认可的那一个：只认便携 Node 里的；没装便携 Node 就是 None，
                // 此时无所谓「被抢占」，据实报 false，不编一个冲突出来。
                let preferred = managed_dir
                    .as_ref()
                    .map(|d| d.join(name))
                    .filter(|p| is_exec(p));
                let shadowed = match (&preferred, &resolved) {
                    (Some(want), Some(got)) => want != got,
                    _ => false,
                };
                CommandGuardCommand {
                    name: name.into(),
                    preferred_path: preferred.map(|p| p.display().to_string()),
                    resolved_path: resolved.map(|p| p.display().to_string()),
                    shadowed,
                }
            })
            .collect::<Vec<_>>();
        let conflicts = commands.iter().filter(|c| c.shadowed).count();

        CommandGuardInspection {
            platform: if cfg!(target_os = "macos") { "macos" } else { "linux" }.into(),
            shims_dir: uking_home().join("shims").display().to_string(),
            conflicts,
            commands,
        }
    }
}

/// 单个进程代理变量的脱敏投影。认证信息永不返回给 GUI、CLI 或日志。
#[derive(Serialize)]
pub struct ProxyEnvironmentEntry {
    pub name: String,
    pub endpoint: String,
}

/// `.wslconfig` 中与 Windows→WSL 代理交接有关的最小状态。只读配置文件，不启动 WSL，
/// 避免 WSL 首次初始化、失效发行版或网络盘导致环境体检卡住。
#[derive(Serialize)]
pub struct WslProxyBridge {
    pub executable_found: bool,
    pub config_found: bool,
    pub auto_proxy: Option<bool>,
    pub mirrored_networking: Option<bool>,
}

/// 影核协议 `runtime.network.inspect` 的规范状态：只检查配置形状，绝不拨号、测速或改代理。
#[derive(Serialize)]
pub struct RuntimeNetworkInspection {
    pub platform: String,
    pub system_proxy: Option<String>,
    pub environment_proxies: Vec<ProxyEnvironmentEntry>,
    pub wsl: WslProxyBridge,
    pub warnings: Vec<String>,
}

fn redact_proxy_endpoint(value: &str) -> String {
    let value = value.trim();
    let (scheme, rest) = value
        .split_once("://")
        .map(|(scheme, rest)| (format!("{scheme}://"), rest))
        .unwrap_or_else(|| (String::new(), value));
    match rest.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}***@{host}"),
        None => format!("{scheme}{rest}"),
    }
}

fn comparable_proxy_endpoint(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('/')
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("socks5h://")
        .trim_start_matches("socks5://")
        .to_ascii_lowercase()
}

/// 只有 WSL 桥用得上，而 WSL 桥是 Windows 独有的 —— 不加这个门，非 Windows 构建会报 dead_code。
#[cfg(windows)]
fn wslconfig_bool(config: &str, name: &str) -> Option<bool> {
    config.lines().find_map(|line| {
        let line = line.split(['#', ';']).next().unwrap_or_default().trim();
        let (key, value) = line.split_once('=')?;
        if key.trim().eq_ignore_ascii_case(name) {
            match value.trim().to_ascii_lowercase().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// 只读网络/WSL 配置检查。它刻意不请求外网、不执行 `wsl.exe`，所以离线、代理坏掉或 WSL
/// 发行版损坏时仍能在很短时间内给出稳定结论；连通性验证和修复以后会作为独立有确认动作加入。
///
/// **平台边界**：进程代理变量和系统代理是每个平台都有的东西，所以这两段在函数主干里，
/// 三平台共用；只有 WSL 桥是 Windows 独有，关在 `cfg(windows)` 里。
///
/// 🔴 别再把整个函数体塞回 `cfg(windows)`。第一版就是那么写的，`cfg(not(windows))`
/// 那半边返回一个全空结构体 —— 于是 Mac 上明明开着全局代理，这个动作也一口咬定「没有代理」。
/// 更糟的是同一个二进制的 `--envfp` 走的是 [`detect_stack`] 那条路，它**是**对的，
/// 两个出口对同一台机器给出相反答案。配方一（「AI 一直转圈」）第 3 步就靠它定位线路问题，
/// 给反了会把排障方向整个带偏。macOS 的 [`system_proxy`] 实现（`scutil --proxy`）
/// 那时候就已经存在了，只是从来没人从这里调用它。
pub fn inspect_runtime_network() -> RuntimeNetworkInspection {
    // ── 跨平台：当前进程的代理变量（脱敏后才进结果） ──
    let mut environment_proxies = Vec::new();
    for name in [
        "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy",
    ] {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                environment_proxies.push(ProxyEnvironmentEntry {
                    name: name.into(),
                    endpoint: redact_proxy_endpoint(&value),
                });
            }
        }
    }

    // ── 跨平台：系统代理。Windows 读注册表，macOS 读 `scutil --proxy`，其余平台返回 None ──
    let system_proxy_raw = system_proxy();

    // ── Windows 独有：WSL 代理桥 ──
    #[cfg(windows)]
    let wsl = {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        let wslconfig = Path::new(&home).join(".wslconfig");
        let wsl_config = std::fs::metadata(&wslconfig)
            .ok()
            .filter(|m| m.len() <= 128 * 1024)
            .and_then(|_| std::fs::read_to_string(&wslconfig).ok());
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        WslProxyBridge {
            executable_found: Path::new(&system_root).join("System32").join("wsl.exe").is_file(),
            config_found: wsl_config.is_some(),
            auto_proxy: wsl_config.as_deref().and_then(|s| wslconfig_bool(s, "autoProxy")),
            mirrored_networking: wsl_config.as_deref().and_then(|s| wslconfig_bool(s, "networkingMode").or_else(|| {
                s.lines()
                    .find_map(|line| line.split(['#', ';']).next().unwrap_or_default().split_once('='))
                    .and_then(|(key, value)| key.trim().eq_ignore_ascii_case("networkingMode").then(|| value.trim().eq_ignore_ascii_case("mirrored")))
            })),
        }
    };
    #[cfg(not(windows))]
    let wsl = WslProxyBridge {
        executable_found: false,
        config_found: false,
        auto_proxy: None,
        mirrored_networking: None,
    };

    // ── 跨平台：一致性告警 ──
    let mut warnings = Vec::new();
    let mut endpoints = environment_proxies
        .iter()
        .map(|p| comparable_proxy_endpoint(&p.endpoint))
        .collect::<Vec<_>>();
    endpoints.sort();
    endpoints.dedup();
    if endpoints.len() > 1 {
        warnings.push("当前进程的多个代理变量指向不同地址；Claude、Codex 与子终端可能各自走不同代理。".into());
    }
    if let Some(system) = &system_proxy_raw {
        let system_endpoint = comparable_proxy_endpoint(system);
        if !endpoints.is_empty() && !endpoints.contains(&system_endpoint) {
            // 措辞去掉「Windows」——这条现在三平台都会命中。
            warnings.push("系统代理与当前进程代理变量不一致；从不同入口启动的 CLI 可能表现不同。".into());
        }
    }
    #[cfg(windows)]
    if wsl.executable_found
        && (system_proxy_raw.is_some() || !environment_proxies.is_empty())
        && wsl.auto_proxy == Some(false)
    {
        warnings.push("`.wslconfig` 显式关闭了 autoProxy；如在 WSL 内运行 Claude/Codex，Windows 代理不会自动继承。".into());
    }

    RuntimeNetworkInspection {
        platform: if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
        .into(),
        system_proxy: system_proxy_raw.as_deref().map(redact_proxy_endpoint),
        environment_proxies,
        wsl,
        warnings,
    }
}

// ———————————————— AI 进程健康取证（runtime.ai_process.inspect） ————————————————

/// 取证回看窗口。
const AI_PROCESS_WINDOW_HOURS: u64 = 72;

/// 会按**镜像名批量结束进程**的那一类软件。它们是「AI 会话莫名其妙断了」最常见的外因。
#[cfg(windows)]
const SECURITY_PRODUCT_IMAGES: &[(&str, &str)] = &[
    ("QQPCTray.exe", "腾讯电脑管家"),
    ("QQPCRTP.exe", "腾讯电脑管家"),
    ("360tray.exe", "360 安全卫士"),
    ("ZhuDongFangYu.exe", "360 主动防御"),
    ("360sd.exe", "360 杀毒"),
    ("kxetray.exe", "金山毒霸"),
    ("kxescore.exe", "金山毒霸"),
    ("HipsTray.exe", "火绒安全"),
    ("HipsDaemon.exe", "火绒安全"),
    ("MsMpEng.exe", "Windows Defender"),
];

/// 一条本机崩溃痕迹。`kind` = `crash_dump` / `wer_report`。
#[derive(Serialize)]
pub struct AiProcessCrashEvidence {
    pub kind: String,
    pub name: String,
    pub age_hours: u64,
}

#[derive(Serialize)]
pub struct AiProcessInspection {
    pub platform: String,
    pub window_hours: u64,
    /// 窗口内与 AI 命令行工具相关的崩溃转储 / WER 记录。
    pub crash_evidence: Vec<AiProcessCrashEvidence>,
    /// 窗口内**无法归属**的崩溃转储数量：文件名只有 PID（`52488.dmp`），WER 当时没能解析出
    /// 进程名。不足以判定是 AI 工具崩了，但也不能当作「什么都没有」——单独报出来供人工确认。
    pub unattributed_dumps: usize,
    /// 正在运行的安全 / 优化类软件。
    pub security_products_running: Vec<String>,
    /// `crash_evidence_found` | `no_crash_evidence`
    pub verdict: String,
    pub hint: String,
}

/// 名字里带这些词就算与 AI 命令行工具相关。
fn ai_process_related(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["claude", "codex", "openclaw", "hermes", "node"].iter().any(|k| n.contains(k))
}

fn entry_age_hours(p: &Path, now: std::time::SystemTime) -> Option<u64> {
    let modified = std::fs::metadata(p).ok()?.modified().ok()?;
    now.duration_since(modified).ok().map(|d| d.as_secs() / 3600)
}

/// 文件名只剩 PID（`52488.dmp`）—— WER 没解析出进程名，无法归属到具体程序。
fn unattributed_dump_name(name: &str) -> bool {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit())
}

/// 扫一个目录，收窗口内、名字与 AI 工具相关的条目；顺带数出无法归属的转储。
fn collect_crash_evidence(
    dir: &Path,
    kind: &str,
    out: &mut Vec<AiProcessCrashEvidence>,
    unattributed: &mut usize,
) {
    let now = std::time::SystemTime::now();
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let related = ai_process_related(&name);
        if !related && !unattributed_dump_name(&name) {
            continue;
        }
        let Some(age_hours) = entry_age_hours(&entry.path(), now) else { continue };
        if age_hours > AI_PROCESS_WINDOW_HOURS {
            continue;
        }
        if related {
            out.push(AiProcessCrashEvidence { kind: kind.into(), name, age_hours });
        } else {
            *unattributed += 1;
        }
    }
}

#[cfg(windows)]
fn running_security_products() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let Ok(out) = std::process::Command::new(system_tool("tasklist"))
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        // 形如 `"claude.exe","66924","Console","1","451,528 K"`，取第一个字段。
        let image = line.trim_start_matches('"').split('"').next().unwrap_or_default();
        if image.is_empty() {
            continue;
        }
        for (needle, label) in SECURITY_PRODUCT_IMAGES {
            if image.eq_ignore_ascii_case(needle) && !found.iter().any(|f| f == label) {
                found.push((*label).to_string());
            }
        }
    }
    found
}

#[cfg(not(windows))]
fn running_security_products() -> Vec<String> {
    Vec::new()
}

/// 跑一条命令并**最多等 `secs` 秒**。超时返回 `None`，子进程留给系统收。
///
/// std 的 `Command::output()` 没有超时，而本文件里几个探测助手跑在启动路径上 ——
/// `tasklist` 被杀软钩住卡死过（客户机 pc-*** 那次查了半天），一卡就是「界面起不来」，
/// 而它们全都是**可有可无的诊断**：宁可探不到，不许卡（宪法第 9 条）。
fn output_with_timeout(mut cmd: std::process::Command, secs: u64) -> Option<std::process::Output> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()?.ok()
}

/// 这个 pid 现在跑的是哪个镜像名（`u-king-mini.exe` / `node.exe` …）。
///
/// `None` = 进程不在了**或者**探测失败。两者刻意不区分：调用方拿它做保守判断时，
/// 「查不出来」和「已经没了」该走同一条路 —— 宁可当它死了多记一笔，也别当它活着漏记。
///
/// **为什么要镜像名而不是「在不在」**：pid 会被系统回收再分配。判「上一轮那个会话是不是
/// 还活着」时若只看 pid 存在，撞上复用就会把一个陌生进程认成自己人，进而把一次**真崩溃**
/// 判成「那只是并行实例」—— 恰好是最不能漏的那一类。
pub fn pid_image_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(system_tool("tasklist"));
        c.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(std::process::Stdio::null());
        let out = output_with_timeout(c, 3)?;
        // 命中形如 `"u-king-mini.exe","66364","Console","1","36,672 K"`。
        // 没命中时 tasklist 打的是本地化提示（中文机器上还是 GBK，`from_utf8_lossy` 会变乱码），
        // 但那行**没有引号字段** —— 所以「只认以 `"` 开头的行」天然把它排掉，
        // 不必跟本地化文案较劲（那正是「按提示文字判断」在非中文机器上会翻车的地方）。
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().find(|l| l.starts_with('"'))?;
        let image = line.trim_start_matches('"').split('"').next()?;
        (!image.is_empty()).then(|| image.to_string())
    }
    #[cfg(not(windows))]
    {
        let mut c = std::process::Command::new("ps");
        c.args(["-p", &pid.to_string(), "-o", "comm="]).stdin(std::process::Stdio::null());
        let out = output_with_timeout(c, 3)?;
        // 进程不存在时 `ps` 退出码非 0 且 stdout 为空 → 自然落到 None。
        // macOS 的 `comm` 给的是完整路径，只留文件名，跟 Windows 侧的语义对齐。
        let text = String::from_utf8_lossy(&out.stdout);
        let name = text.trim().rsplit('/').next().unwrap_or_default().to_string();
        (!name.is_empty()).then_some(name)
    }
}

/// 只读取证：**AI 会话莫名其妙断了，到底是自己崩的还是被别人杀的。**
///
/// 强杀（`taskkill /IM` / 安全软件 / 任意 `TerminateProcess`）**不会**留下 WER 记录或崩溃
/// 转储 —— 所以「什么痕迹都没有」本身就是决定性结论：不是崩溃，是被外部结束。客户只会报
/// 「claude 老是断」，没有这条判据谁也说不清该查哪边。
///
/// 全程只读：翻两个本地目录 + 一次 `tasklist`，不联网、不改任何东西。
pub fn inspect_ai_process_health() -> AiProcessInspection {
    let mut crash_evidence = Vec::new();
    let mut unattributed_dumps = 0usize;

    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let local = Path::new(&local);
            collect_crash_evidence(
                &local.join("CrashDumps"),
                "crash_dump",
                &mut crash_evidence,
                &mut unattributed_dumps,
            );
            let wer = local.join("Microsoft").join("Windows").join("WER");
            for bucket in ["ReportArchive", "ReportQueue"] {
                collect_crash_evidence(
                    &wer.join(bucket),
                    "wer_report",
                    &mut crash_evidence,
                    &mut unattributed_dumps,
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let reports = Path::new(&home).join("Library").join("Logs").join("DiagnosticReports");
            collect_crash_evidence(
                &reports,
                "crash_dump",
                &mut crash_evidence,
                &mut unattributed_dumps,
            );
        }
    }

    crash_evidence.sort_by_key(|e| e.age_hours);
    crash_evidence.truncate(20);

    let security_products_running = running_security_products();
    let found = !crash_evidence.is_empty();
    let hint = if found {
        format!(
            "近 {AI_PROCESS_WINDOW_HOURS} 小时内有 AI 工具的崩溃痕迹，按「进程自己崩了」排查（先看内存占用与插件）。"
        )
    } else {
        let who = if security_products_running.is_empty() {
            "先回想最近有没有跑过备份还原 / 卸载 / 清理优化类操作".to_string()
        } else {
            format!("在跑的安全软件：{}", security_products_running.join("、"))
        };
        let unknown = if unattributed_dumps > 0 {
            format!("另有 {unattributed_dumps} 个无法归属的崩溃转储（文件名只有 PID），必要时人工确认。")
        } else {
            String::new()
        };
        format!(
            "没有任何崩溃痕迹 —— 强杀不会留下转储或 WER 记录，所以会话中断基本可判定为被**外部结束进程**（安全软件、清理/还原类工具、`taskkill /IM`），不是崩溃。{who}。{unknown}"
        )
    };

    AiProcessInspection {
        platform: if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
        .into(),
        window_hours: AI_PROCESS_WINDOW_HOURS,
        crash_evidence,
        unattributed_dumps,
        security_products_running,
        verdict: if found { "crash_evidence_found" } else { "no_crash_evidence" }.into(),
        hint,
    }
}

#[cfg(windows)]
/// Build an ACP-safe `.cmd` body. Never interpolate a Unicode absolute path: Rust writes UTF-8
/// while cmd.exe reads batch files in the system ANSI code page.
fn cli_command_guard_script(target: &Path, home: &Path, appdata: Option<&Path>) -> Option<String> {
    let (root_var, suffix) = if let Ok(suffix) = target.strip_prefix(home) {
        ("USERPROFILE", suffix)
    } else if let Some(appdata) = appdata {
        ("APPDATA", target.strip_prefix(appdata).ok()?)
    } else {
        return None;
    };
    let suffix = suffix.to_string_lossy().replace('/', "\\");
    if !suffix.is_ascii() || suffix.is_empty() { return None; }
    let call = target.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
    let invoke = if call {
        format!("call \"%{root_var}%\\{suffix}\" %*")
    } else {
        format!("\"%{root_var}%\\{suffix}\" %*")
    };
    Some(format!("@echo off\r\nrem U-King CLI command guard\r\n{invoke}\r\n"))
}

/// 进入免费页时的轻量刷新。失败返回 None，由前端保留编进程序的最后可信清单；不把
/// 网络错误伪装成空清单，也不缓存客户 Key（Registry 从设计上不允许有 Key 字段）。
pub fn fetch_free_registry() -> Option<serde_json::Value> {
    for url in FREE_REGISTRY_URLS {
        let Ok(out) = curl(&["-sL", "-m", "6", url]) else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&out) else { continue };
        let reviewed = value.get("status").and_then(|v| v.as_str()) == Some("reviewed");
        let version = value.get("version").and_then(|v| v.as_u64());
        let entries = value.get("entries").and_then(|v| v.as_array());
        if reviewed && version.is_some() && entries.is_some() {
            return Some(value);
        }
    }
    None
}

#[cfg(windows)]
fn is_legacy_uking_command_guard(old: &str, name: &str) -> bool {
    let lines: Vec<_> = old.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let expected_tail = format!("\\.uking\\runtime\\node\\{name}.cmd\" %*");
    lines.len() == 2
        && lines[0].eq_ignore_ascii_case("@echo off")
        && lines[1].starts_with('"')
        && lines[1].to_ascii_lowercase().ends_with(&expected_tail)
}

/// Upgrade only the exact two-line guard written by pre-marker U-King releases. This startup
/// migration does not create guards, alter PATH, or overwrite an unknown user script.
#[cfg(windows)]
pub fn migrate_legacy_cli_command_guards() -> usize {
    let Ok(home) = std::env::var("USERPROFILE") else { return 0 };
    let home = PathBuf::from(home);
    let shims = uking_home().join("shims");
    let mut migrated = 0;
    for name in ["claude", "codex", "pi", "openclaw", "hermes"] {
        let shim = shims.join(format!("{name}.cmd"));
        let old = std::fs::read_to_string(&shim).unwrap_or_default();
        if !is_legacy_uking_command_guard(&old, name) { continue; }
        let target = home.join(".uking").join("runtime").join("node").join(format!("{name}.cmd"));
        if !target.is_file() { continue; }
        if let Some(content) = cli_command_guard_script(&target, &home, None) {
            if std::fs::write(&shim, content).is_ok() { migrated += 1; }
        }
    }
    migrated
}

#[cfg(not(windows))]
pub fn migrate_legacy_cli_command_guards() -> usize { 0 }

#[cfg(windows)]
pub fn ensure_cli_command_guard(on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<(), String> {
    if std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false) {
        on_log("out", "CLI 命令优先级守卫：测试沙箱中跳过用户 PATH 修改");
        return Ok(());
    }

    let home = std::env::var("USERPROFILE").map_err(|_| "找不到 USERPROFILE".to_string())?;
    let home_path = PathBuf::from(&home);
    let appdata = std::env::var("APPDATA").ok();
    let appdata_path = appdata.as_deref().map(Path::new);
    let mut sources = Vec::new();
    if let Some(node) = portable_node_dir() {
        sources.push(node);
    }
    if let Some(appdata) = appdata_path {
        sources.push(appdata.join("npm"));
    }
    // 仅作为最后一层候选：这里会选择 .cmd/.exe，绝不会把无扩展名的 Bash 转发脚本再包装进去。
    sources.push(Path::new(&home).join("bin"));
    sources.push(Path::new(&home).join(".local").join("bin"));

    let mut targets = Vec::new();
    for name in ["claude", "codex", "pi", "openclaw", "hermes"] {
        let target = sources.iter().find_map(|dir| {
            [".cmd", ".exe"]
                .iter()
                .map(|ext| dir.join(format!("{name}{ext}")))
                .find(|p| p.is_file())
        });
        if let Some(target) = target {
            targets.push((name, target));
        }
    }
    if targets.is_empty() {
        on_log("out", "CLI 命令优先级守卫：未发现可转发的已安装 AI 命令，跳过");
        return Ok(());
    }

    let shims = uking_home().join("shims");
    std::fs::create_dir_all(&shims).map_err(|e| format!("创建 CLI 转发目录失败：{e}"))?;
    let mut guarded = Vec::new();
    for (name, target) in targets {
        let shim = shims.join(format!("{name}.cmd"));
        let Some(content) = cli_command_guard_script(&target, &home_path, appdata_path) else {
            on_log("out", &format!("CLI 命令优先级守卫：跳过 {}（避免写入 Unicode 批处理路径）", target.display()));
            continue;
        };
        // 不覆盖用户手工放进 U-King 目录的未知脚本；仅迁移旧版 U-King 两行转发器。
        if shim.exists() {
            let old = std::fs::read_to_string(&shim).unwrap_or_default();
            if !old.is_empty() && !old.contains("U-King CLI command guard") && !is_legacy_uking_command_guard(&old, name) {
                on_log("out", &format!("CLI 命令优先级守卫：保留已有 {}（未覆盖未知转发器）", shim.display()));
                continue;
            }
        }
        std::fs::write(&shim, content).map_err(|e| format!("写入 {} 失败：{e}", shim.display()))?;
        guarded.push(name);
    }
    if guarded.is_empty() {
        return Ok(());
    }

    prepend_user_path(&shims)?;
    prepend_process_path(&[shims.clone()]);
    on_log(
        "out",
        &format!(
            "CLI 命令优先级守卫已启用：{}；未删除任何原文件。请关闭并重开外部终端 / Codex 后再试。",
            guarded.join("、")
        ),
    );
    Ok(())
}

#[cfg(not(windows))]
pub fn ensure_cli_command_guard(_on_log: &(dyn Fn(&str, &str) + Send + Sync)) -> Result<(), String> {
    Ok(())
}

/// 沙箱里不许碰真实用户 PATH —— **闸门放在唯一咽喉上，不是逐个调用点**。
///
/// 这条护栏原来只长在 `persist_python_scripts_path` 一处，而 `add_user_path` 另有
/// 三个调用点（便携 Node 的 bin、便携 Git 的 cmd 与 bin）**全都没设防**：
/// `UKING_TEST_HOME` 沙箱里跑一次装机，就会把沙箱目录写进开发机的用户 PATH，
/// 沙箱跑完即删 → 留下指向不存在目录的死路径。
/// **回归跑道自己污染真实状态，那它验出来的一切都不作数。**
///
/// 「每个调用点各加一次判断」正是本项目反复复发的那个形状 —— 漏一个就等于没有。
/// 所以判断挪进 `add_user_path` 自身：**从此不存在「忘了加护栏」的调用点**。
fn sandboxed() -> bool {
    std::env::var("UKING_TEST_HOME").map(|v| !v.is_empty()).unwrap_or(false)
}

/// 把目录追加进用户 PATH（不存在才加）。
#[cfg(windows)]
fn add_user_path(dir: &Path) -> Result<(), String> {
    if sandboxed() {
        return Ok(()); // 沙箱：该装的照装，就是不碰真实用户 PATH
    }
    // 用 PowerShell API，避免 setx 1024 字符截断
    let d = dir.display().to_string();
    let ps = format!(
        r#"$p=[Environment]::GetEnvironmentVariable('Path','User'); if(($p -split ';') -notcontains '{d}'){{ [Environment]::SetEnvironmentVariable('Path', ($p.TrimEnd(';') + ';{d}'), 'User') }}"#
    );
    let (code, out) = run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", &ps], None)?;
    if code == 0 {
        Ok(())
    } else {
        Err(format!("写入用户 PATH 失败：{}", tail(&out, 200)))
    }
}

#[cfg(not(windows))]
fn add_user_path(dir: &Path) -> Result<(), String> {
    if sandboxed() {
        return Ok(()); // 同 Windows 分支：沙箱不碰真实用户的 ~/.zshrc
    }
    // macOS 默认 zsh：追加 ~/.zshrc（已有则跳过）。app 自身探测不依赖这条（见 search_paths）。
    let home = std::env::var("HOME").map_err(|_| "找不到 HOME".to_string())?;
    let rc = Path::new(&home).join(".zshrc");
    let line = format!("export PATH=\"{}:$PATH\"", dir.display());
    let existing = std::fs::read_to_string(&rc).unwrap_or_default();
    if !existing.contains(&line) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&rc)
            .map_err(|e| format!("写 .zshrc 失败: {e}"))?;
        writeln!(f, "\n# U-King 便携 Node\n{line}").map_err(|e| format!("写 .zshrc 失败: {e}"))?;
    }
    Ok(())
}

// ============================================================
// 命令执行底座（cmd /C + 便携 Node PATH + 不闪黑窗）
// ============================================================

#[cfg(windows)]
fn base_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    // 系统自带工具一律绝对路径，不赌客户 PATH：真实客户机（pc-***）手动改坏
    // 机器级 PATH（C:\Windows\system32 → C:\;indows\system32）后，裸 curl/tar/cmd
    // 全部解析失败，凡下载/解压/跑 shell 的步骤全军覆没。
    let resolved = match program {
        "curl" | "tar" | "cmd" | "reg" | "where" | "taskkill" | "tasklist" | "powershell" => {
            system_tool(program)
        }
        other => other.to_string(),
    };
    let mut c = std::process::Command::new(resolved);
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：GUI 程序下别闪黑窗
    c
}

/// Windows 系统工具的 System32 绝对路径（powershell 在 WindowsPowerShell\v1.0 子目录）。
/// 各模块统一走这里，别再写裸 `Command::new("powershell")`。
#[cfg(windows)]
pub fn system_tool(name: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    match name {
        "powershell" => format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"),
        _ => format!("{root}\\System32\\{name}.exe"),
    }
}

/// PowerShell 7 (`pwsh.exe`) 的绝对路径；找不到返回 None（调用方回落到 5.1 的
/// `system_tool("powershell")`）。给内嵌终端 `shell_builder` 用，让 U-Workspace 里的终端
/// 跟用户外面用的终端一致（新版 profile / 别名 / PSReadLine），而不是老的 Windows PowerShell 5.1。
///
/// 覆盖三种安装形态：
///  ① 独立安装包：`%ProgramFiles%\PowerShell\7\pwsh.exe`（真 exe，最稳）；
///  ② 微软商店 MSIX 版：只暴露执行别名 `%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe`
///     —— 0 字节 AppExecLink reparse 点；卸载后会**残留坏别名**，必须真跑一次才算数（见
///     `pwsh_usable`）；
///  ③ PATH 上的 pwsh.exe（search_paths 兜底，同样先探再信）。
///
/// 不去 glob `C:\Program Files\WindowsApps\...`：该目录对普通用户 ACL 受限，列目录会 Access Denied。
/// 存在性判断用 symlink_metadata —— AppExecLink 上 `Path::exists()`（走 metadata 解析 reparse）
/// 会失败而误报 false。
#[cfg(windows)]
pub fn find_pwsh() -> Option<String> {
    let present = |p: &Path| std::fs::symlink_metadata(p).is_ok() || p.exists();

    // ① 独立安装包
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Ok(root) = std::env::var(var) {
            let p = Path::new(&root).join("PowerShell").join("7").join("pwsh.exe");
            if present(&p) {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    // ② 微软商店版执行别名（per-user，可读可执行）。🔴 只判存在不够：卸载商店版会残留
    //    0 字节 AppExecLink 别名，symlink_metadata 看「文件在」，CreateProcessW 却报
    //    「找不到适用的应用许可证」（os error -1058406399），终端永远开不出来 —— 先探再信。
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = Path::new(&local).join("Microsoft").join("WindowsApps").join("pwsh.exe");
        if present(&p) && pwsh_usable(&p) {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    // ③ PATH 兜底（PATH 上同样可能是坏别名/被掏空的目录，采信前先探一次）
    for dir in search_paths(None) {
        let p = dir.join("pwsh.exe");
        if present(&p) && pwsh_usable(&p) {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    // ④ 我们自己下发的便携 PS7（客户机原本只有 5.1 时，ensure_pwsh 下过来的）
    portable_pwsh()
}

/// .NET「自包含」应用必须自带的四个原生宿主文件。
///
/// 杀软（pc-*** 实锤：360 安全卫士）会把它们当成「自带 .NET 运行时的可疑程序」删掉：
/// 那台机器上 289 个文件、182MB 一个不少，**只少这四个**，于是 pwsh.exe 一起手就
/// `A fatal error was encountered. 'hostpolicy.dll' ... not found` + 退出码 `0x80008083`。
#[cfg(windows)]
const PWSH_HOST_FILES: [&str; 4] = ["hostfxr.dll", "hostpolicy.dll", "coreclr.dll", "clrjit.dll"];

/// 这份 pwsh 目录缺哪些宿主文件。**纯函数、不 spawn** —— 单测能直接摆出「被掏空」的现场。
#[cfg(windows)]
fn pwsh_missing_host_files(dir: &Path) -> Vec<&'static str> {
    PWSH_HOST_FILES.iter().copied().filter(|f| !dir.join(f).exists()).collect()
}

/// 「这一份 pwsh **能不能跑**」——不是「pwsh.exe 在不在」。
///
/// 先看四个宿主文件（能给出人话原因：谁被删了），再真起一次进程（挡住「文件都在但照样起不来」，
/// 比如半截解压、坏 zip、SmartScreen 拦截）。
#[cfg(windows)]
pub fn pwsh_health(exe: &Path) -> Result<(), String> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let missing = pwsh_missing_host_files(dir);
    if !missing.is_empty() {
        return Err(format!("被安全软件删掉了 .NET 宿主文件：{}", missing.join("、")));
    }
    match run_capture_raw(&exe.to_string_lossy(), &["-NoLogo", "-NoProfile", "-Command", "exit 0"], None) {
        Ok((0, _)) => Ok(()),
        Ok((code, out)) => Err(format!("起不来（退出码 {code}）：{}", tail(&out, 200))),
        Err(e) => Err(format!("起不来：{e}")),
    }
}

/// pwsh 可用性探测缓存（进程级）：路径 → 「真跑过一次且退出码 0」。`find_pwsh` 在每次开终端的
/// 路径上都会被调，按路径缓存避免每次开终端都对同一个候选重复 spawn。
#[cfg(windows)]
fn pwsh_probe_cache() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, bool>> {
    static C: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>,
    > = std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 「这份 pwsh **能不能被 CreateProcess 拉起来**」——真起一次进程验证（同 `pwsh_health` 哲学，
/// 但不带宿主文件检查：商店别名是 reparse 点、没有平铺的宿主文件目录）。
/// 结果按路径缓存：可用/不可用各只 spawn 一次，之后本进程直接查表。
///
/// 🔴 为什么存在：微软商店版 PowerShell 卸载后残留 0 字节 AppExecLink 别名
/// （`%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe`），文件存在但 CreateProcessW 报
/// 「找不到适用的应用许可证」（os error -1058406399）——只判存在会让内嵌终端永远开不出来。
#[cfg(windows)]
fn pwsh_usable(exe: &Path) -> bool {
    let key = exe.to_path_buf();
    let mut c = match pwsh_probe_cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(ok) = c.get(&key) {
        return *ok;
    }
    let (ok, why) = match run_capture_raw(
        &exe.to_string_lossy(),
        &["-NoLogo", "-NoProfile", "-Command", "exit 0"],
        None,
    ) {
        Ok((0, _)) => (true, String::new()),
        Ok((code, out)) => (false, format!("退出码 {code}：{}", tail(&out, 160))),
        Err(e) => (false, e),
    };
    if !ok {
        // 缓存保证同一坏路径只记这一次，不会刷屏
        crate::ulog::write("installer", &format!("pwsh {} 起不来，弃用：{why}", exe.display()));
    }
    c.insert(key, ok);
    ok
}

/// 「上一次装出来的是坏的」标记：`~/.uking/runtime/pwsh.blocked`，内容就是坏的原因。
#[cfg(windows)]
fn pwsh_blocked_marker() -> PathBuf {
    uking_home().join("runtime").join("pwsh.blocked")
}

/// 便携 PS7 的健康判定缓存：一个进程只探一次（`find_pwsh` 在每次开终端的路径上）。
#[cfg(windows)]
fn portable_pwsh_cache() -> &'static std::sync::Mutex<Option<Option<String>>> {
    static C: std::sync::OnceLock<std::sync::Mutex<Option<Option<String>>>> = std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(None))
}

/// 装好之后重新判一次（`ensure_pwsh` 成功/失败后调）。
#[cfg(windows)]
fn forget_portable_pwsh() {
    if let Ok(mut c) = portable_pwsh_cache().lock() {
        *c = None;
    }
}

/// 便携 PowerShell 7（`~/.uking/runtime/pwsh/pwsh.exe`）——**验过能跑**才返回。
///
/// 🔴 原来这里只判文件在不在。pc-*** 上 360 删了四个宿主文件，形状全对，于是：
/// `ensure_pwsh` 报「便携 PowerShell 7 已就绪」→ `find_pwsh` 每次都把这份坏的交给终端 →
/// 用户每开一个终端就是一行「进程已退出（退出码 2147516547）」，体感是「老是崩溃」。
/// 而且**重装 U-King 也修不好**：pwsh.exe 还在 → 探测短路 → 永远跳过重下。
///
/// 判据是「能不能用」，不是「装没装」（同 readiness：`installed:true` 形状全对不等于世界是好的）。
#[cfg(windows)]
pub fn portable_pwsh() -> Option<String> {
    let mut c = match portable_pwsh_cache().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(v) = c.as_ref() {
        return v.clone();
    }
    let exe = uking_home().join("runtime").join("pwsh").join("pwsh.exe");
    let v = if !(std::fs::symlink_metadata(&exe).is_ok() || exe.exists()) {
        None
    } else {
        match pwsh_health(&exe) {
            Ok(()) => Some(exe.to_string_lossy().into_owned()),
            Err(why) => {
                // 记一笔：① 本进程不必再 spawn 一次就知道坏；② `ensure_pwsh` 据此**拒绝白下 106MB**
                //（杀软会照删不误，自动重下只是每次开终端烧一遍流量）；③ 终端里能把原因原样说给用户
                let _ = std::fs::write(pwsh_blocked_marker(), &why);
                crate::ulog::write("installer", &format!("便携 pwsh 不可用：{why}（已回落系统 PowerShell）"));
                None
            }
        }
    };
    *c = Some(v.clone());
    v
}

/// 便携 PowerShell 7 下载源（阿里云深圳 OSS，国内直连 5MB/s，免管理员）+ 期望 SHA-256。
/// 官方 v7.4.6 LTS win-x64（PowerShell-7.4.6-win-x64.zip，111161483 字节），已镜像到 OSS runtimes。
#[cfg(windows)]
const PWSH_OSS_URL: &str =
    "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/runtimes/pwsh-win-x64.zip";
#[cfg(windows)]
const PWSH_SHA256: &str = "ed49ce5adb2162cc4a835d740486be729ba904627cca71fcb6c2b95be11b993d";

/// 确保 PowerShell 7 可用，返回 pwsh.exe 绝对路径。系统/已装便携版有就直接返回；否则从 OSS
/// 下载便携 PS7（~106MB，一次性，免管理员/免 UAC）解压到 `~/.uking/runtime/pwsh`。
///
/// 为什么要它：很多客户机只有 Windows 自带的 PowerShell 5.1（老、中文易乱码、无 PSReadLine），
/// U-Workspace 内嵌终端本会回落 5.1。装一份便携 7 让所有机器的终端都用上现代 shell（UTF-8 默认）。
///
/// `force_download=true`（仅测试用）：跳过系统探测直接走下载路径（验证便携链路，因为开发机自带 PS7
/// 会让系统探测短路）。生产路径一律传 false。失败返回 Err，调用方回落 5.1，终端照常能开。
#[cfg(windows)]
pub fn ensure_pwsh(
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
    force_download: bool,
) -> Result<String, String> {
    if force_download {
        // 强制模式 = **人工触发的修复**（`--pwsh-test`）：先撕掉「别再下了」的封条再判
        let _ = std::fs::remove_file(pwsh_blocked_marker());
        forget_portable_pwsh();
        if let Some(p) = portable_pwsh() {
            return Ok(p); // 便携版已在**且跑得动**就直接用，不重复下
        }
    } else if let Some(p) = find_pwsh() {
        return Ok(p);
    }

    // 上一次装出来的是被掏空的 → **不自动重下**。杀软会反复删同样几个文件，自动重下只是
    // 每次开终端白烧 106MB，结果一样坏。把原因和修法原样交给用户，让他决定（这条会打进终端窗格）。
    if let Ok(why) = std::fs::read_to_string(pwsh_blocked_marker()) {
        return Err(format!(
            "上次装好的便携 PowerShell 7 {why}；已改用系统自带的 PowerShell 5.1。\
             要修：把 {} 加进杀毒软件（360 安全卫士 / Windows Defender）的信任区，\
             再跑一次 U-King.exe --pwsh-test",
            uking_home().display()
        ));
    }

    on_log(
        "out",
        "未检测到 PowerShell 7，开始下载便携版（阿里云深圳源，约 106MB，一次性，免管理员）…",
    );
    let runtime = uking_home().join("runtime");
    std::fs::create_dir_all(&runtime).map_err(|e| format!("创建 runtime 目录失败: {e}"))?;

    let pkg = std::env::temp_dir().join("uking-pwsh.zip");
    let pkg_s = pkg.display().to_string();
    // 专用下载（不走 download_with_fallback 的 600s 硬超时）：106MB 慢网客户要更长超时 + 断点续传。
    // `--proxy ""` 绕开客户机 clash 式代理（OSS 深圳国内直连，代理反而常握手失败）。SHA-256 挡半包/篡改。
    let _ = curl(&[
        "-fL", "-sS", "-C", "-", "--retry", "3", "--retry-delay", "5", "-m", "1800", "--proxy",
        "", "-o", &pkg_s, PWSH_OSS_URL,
    ])
    .map_err(|e| format!("下载 PowerShell 7 失败: {e}"))?;
    if let Err(e) = verify_download(&pkg_s, PWSH_SHA256, 100_000_000) {
        let _ = std::fs::remove_file(&pkg); // 校验不过删掉半包，下次重新下（-C - 才不会续在坏文件上）
        return Err(format!("PowerShell 7 下载校验未过: {e}"));
    }
    on_log("out", "下载完成，解压中（展开约 260MB，请稍候）…");

    // pwsh 的 zip 是扁平布局（pwsh.exe 在根 + Modules/ 等支持目录）。先解压到临时目录再原子换名，
    // 避免中途失败留下半截 pwsh/ 让 find_pwsh 误判已装好。
    let staging = runtime.join("pwsh.tmp");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建解压临时目录失败: {e}"))?;
    extract_archive(&pkg_s, &staging)?;

    // 杀软瞬时锁重试（对齐 ensure_node 的 rename 重试同款病根/疗法）。
    let target = runtime.join("pwsh");
    let mut last_err = None;
    for attempt in 0..10 {
        if target.exists() {
            let _ = std::fs::remove_dir_all(&target);
        }
        match std::fs::rename(&staging, &target) {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < 9 {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
        }
    }
    if let Some(e) = last_err {
        return Err(format!("整理 pwsh 目录失败: {e}"));
    }
    let _ = std::fs::remove_file(&pkg);

    let exe = target.join("pwsh.exe");
    if !(std::fs::symlink_metadata(&exe).is_ok() || exe.exists()) {
        return Err("解压后未找到 pwsh.exe".into());
    }
    // ★ 装完必须**真起一次**才敢说「就绪」。老版本到上一行就报成功了 —— 而 pc-*** 上
    // 杀软是在解压完之后才把四个宿主文件删掉的，文件形状全对、报告全绿、终端每次秒退。
    forget_portable_pwsh();
    if let Err(why) = pwsh_health(&exe) {
        let _ = std::fs::write(pwsh_blocked_marker(), &why);
        crate::ulog::write("installer", &format!("便携 pwsh 装完却不可用：{why}"));
        return Err(format!("装完却跑不起来：{why}"));
    }
    let _ = std::fs::remove_file(pwsh_blocked_marker());
    let exe_s = exe.to_string_lossy().into_owned();
    on_log("out", &format!("便携 PowerShell 7 已就绪：{exe_s}"));
    Ok(exe_s)
}

#[cfg(not(windows))]
fn base_command(program: &str) -> std::process::Command {
    std::process::Command::new(program)
}

/// 非 Windows 平台直接返回原名（PATH 正常可信）。
#[cfg(not(windows))]
pub fn system_tool(name: &str) -> String {
    name.to_string()
}

/// 探测/安装时额外注入 PATH 的目录。
///
/// 双击启动时（Explorer / Finder）给的 PATH 经常不含工具目录（npm 全局目录、
/// homebrew 等往往只在 shell profile 里加了 PATH），导致明明装了 claude/codex
/// 却「未检测到」。所以子进程 PATH 永远前置这几个已知位置，不赌系统 PATH。
pub fn search_paths(extra: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = extra {
        v.push(d.to_path_buf());
    }
    // 便携 Python 的脚本目录（hermes 等 pip 装的工具）+ python 本体目录
    if let Some(s) = portable_python_scripts_dir() {
        v.push(s);
    }
    if let Some(p) = portable_python_exe() {
        if let Some(dir) = p.parent() {
            v.push(dir.to_path_buf());
        }
    }
    // 便携 Git 的 cmd（git.exe）+ bin（bash.exe）—— Claude Code 的 Bash 工具刚需 bash；
    // 注入后终端/验证/独立 Hermes 窗口都能找到 git 与 bash（Windows 才有，Mac 返回空）。
    for d in portable_git_dirs() {
        v.push(d);
    }
    #[cfg(windows)]
    {
        // npm 默认全局 prefix，claude.cmd / codex.cmd 在这
        if let Ok(appdata) = std::env::var("APPDATA") {
            let npm = Path::new(&appdata).join("npm");
            if npm.exists() {
                v.push(npm);
            }
        }
        // 用户常把 CLI 手动放进 ~/bin、~/.local/bin（非标准但很常见）—— 不扫会漏判「未安装」
        if let Ok(home) = std::env::var("USERPROFILE") {
            for sub in ["bin", ".local/bin"] {
                let p = Path::new(&home).join(sub);
                if p.exists() {
                    v.push(p);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        // macOS：Finder 启动的 app PATH 只有 /usr/bin:/bin:/usr/sbin:/sbin
        for d in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let p = PathBuf::from(d);
            if p.exists() {
                v.push(p);
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            for sub in [".npm-global/bin", ".local/bin"] {
                let p = Path::new(&home).join(sub);
                if p.exists() {
                    v.push(p);
                }
            }
        }
    }
    v
}

/// PATH 分隔符。
#[cfg(windows)]
const PATH_SEP: &str = ";";
#[cfg(not(windows))]
const PATH_SEP: &str = ":";

/// PATH 前置已知工具目录（见 `search_paths`）。
fn with_path(c: &mut std::process::Command, extra: Option<&Path>) {
    // 安装/修复子进程一律清掉代理环境变量。
    // 客户机常装 clash 式代理（HTTP_PROXY=http://127.0.0.1:7890 等），代理端口
    // 不稳/没开时，npm install / pip install 会卡死或报「lookup "": no such host」。
    // registry/index 都是国内镜像，直连可达，本就不该走代理 —— 与下载层 curl 一致
    // （curl 已主动绕代理，见本文件 `curl()` 注释）。env 层清空覆盖 pip 与子 curl；
    // npm 的代理还可能写死在全局 npmrc，靠命令行 --proxy="" 兜（见 NpmInstall）。
    for v in [
        "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
        "http_proxy", "https_proxy", "all_proxy", "no_proxy",
    ] {
        c.env_remove(v);
    }
    // pip 的 wheel 缓存目录走**环境变量**下发，不写进 pip.ini：pip 读配置文件用的是系统 ANSI
    // 代码页（`locale.getpreferredencoding()`，中文 Windows = cp936），我们写文件却是 UTF-8。
    // 路径含中文用户名时，那串 UTF-8 字节在 cp936 里解不出来，pip 会对**每一次调用**都报
    // 「Configuration file contains invalid cp936 characters」并退出 2 —— 不是某个包装不上，
    // 是这台机器上所有 pip 调用全废（pc-*** 实锤）。环境变量是 Unicode，不过 locale 解码
    // 这一关，所以中文路径下缓存能力照样保住。与 pip.ini 里的 cache-dir 是同一个值，只是载体不同。
    c.env("PIP_CACHE_DIR", pip_cache_dir());

    let dirs = search_paths(extra);
    if dirs.is_empty() {
        return;
    }
    let old = std::env::var("PATH").unwrap_or_default();
    let prefix = dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(PATH_SEP);
    c.env("PATH", format!("{prefix}{PATH_SEP}{old}"));
}

/// 平台 shell：Windows `cmd /C`，Unix `sh -c`。
fn shell_command(cmdline: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = base_command("cmd");
        // 🔴 用 raw_arg 原样透传命令行，别用 .args(["/C", cmdline]) —— 后者会走 Rust 的自动
        // 参数引号转义，把命令里的内层 `"`（如 `powershell -Command "..."` / `python -c "..."`）
        // 转义成 `\"`；cmd.exe 不认反斜杠转义，会把 `\"` 当字面量往下传，PowerShell/python 拿到
        // 以 `"` 开头的乱码 → 复杂命令被 mangle、静默误执行（真机实测：codex-app 的
        // verify_cmd 假通过把整条命令当版本号、hermes shim 步 $var 变空、python -c 报
        // SyntaxError）。raw_arg 后 `cmd /C <cmdline>` 拿到原始命令行、内层引号完好，
        // PowerShell/python 才能正确解析执行。简单命令（无内层引号）行为不变。
        c.raw_arg(format!("/C {cmdline}"));
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = base_command("sh");
        c.args(["-c", cmdline]);
        c
    }
}

/// 按行读一个流，**用 lossy 解码**，而不是 `BufReader::lines()`。
///
/// 🔴 `lines()` 产出的是 `Result<String>`：非 UTF-8 的行是 `Err`，而 `map_while(Result::ok)`
/// **遇到第一个 Err 就停止迭代** —— 不是跳过那一行，是从那一行起**整条流不再读**。
/// 中文 Windows 上子进程输出是 GBK（路径里的中文用户名、系统本地化报错全在里面），
/// 所以只要吐出第一行带中文的输出，后面全部丢失：越是撞上编码类故障的机器，我们越拿不到
/// 它的错误原文。pc-*** 与 Issue #223 的失败上报里只剩一句「命令退出码 2：<整条命令行>」，
/// pip 真正说的「Configuration file contains invalid cp936 characters」一个字都没留下 ——
/// 这个 bug 因此在 320 条 issue 里潜伏了一周多、跨了至少 3 个客户。
/// 改 lossy 后坏字节退化成 `�`，**但行还在、流也读得下去**，错误原文进得了 Err、进得了
/// report_bug。`capture()` 早就是 lossy 的（所以自检那条日志能看到 cp936 报错，装机这条看不到），
/// 两边就此对齐。
fn read_lines_lossy<R: std::io::Read>(r: R, mut on_line: impl FnMut(String)) {
    let mut br = std::io::BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match br.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                    buf.pop();
                }
                on_line(String::from_utf8_lossy(&buf).into_owned());
            }
        }
    }
}

/// 跑命令行（平台 shell），合并 stdout/stderr 按行流式回调。
fn run_stream(
    cmdline: &str,
    extra_path: Option<&Path>,
    on_log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<(), String> {
    let mut c = shell_command(cmdline);
    with_path(&mut c, extra_path);
    c.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());

    let mut child = c.spawn().map_err(|e| format!("启动命令失败 `{cmdline}`: {e}"))?;

    // stderr 在子线程读，stdout 主线程读，都按行回调
    let stderr = child.stderr.take();
    let err_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let err_store = err_lines.clone();
    let h = stderr.map(|se| {
        std::thread::spawn(move || {
            read_lines_lossy(se, |line| {
                if let Ok(mut g) = err_store.lock() {
                    g.push(line);
                }
            });
        })
    });

    if let Some(so) = child.stdout.take() {
        read_lines_lossy(so, |line| {
            if !line.trim().is_empty() {
                on_log("out", &line);
            }
        });
    }
    if let Some(h) = h {
        let _ = h.join();
    }
    // stderr 不一定是错误（npm 的进度就走 stderr），统一作普通输出展示
    for line in err_lines.lock().unwrap().iter() {
        if !line.trim().is_empty() {
            on_log("out", line);
        }
    }

    let status = child.wait().map_err(|e| format!("等待命令失败: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        // 之前失败信息只回显整条命令行（PowerShell 一行常几百字符），真正的异常文本
        // （stderr 里 throw/Write-Error 的那句话）虽然已经实时进了 on_log("out",..)，
        // 但没进最终返回的 Err，导致 report_bug 采到的日志尾部和 issue 标题全是命令行
        // 片段——诊断不出真实原因（多次巡视实锤：Codex MSIX / Hermes pip 安装失败类
        // issue 标题被截断成命令片段，看不出为什么失败）。改成优先带上 stderr 尾部。
        let err_tail = err_lines.lock().unwrap().join("\n");
        let code = status.code().unwrap_or(-1);
        if err_tail.trim().is_empty() {
            Err(format!("命令退出码 {code}：{cmdline}"))
        } else {
            Err(format!("命令退出码 {code}：{}", tail(&err_tail, 300)))
        }
    }
}

/// 跑命令行并捕获输出（探测 / 验证用）。
fn run_capture(cmdline: &str, extra_path: Option<&Path>) -> Result<(i32, String), String> {
    let mut c = shell_command(cmdline);
    with_path(&mut c, extra_path);
    capture(c, cmdline)
}

/// 直接跑一个程序收输出（不经 shell）。系统工具走绝对路径、隐窗、注入 search_paths。
/// `pub(crate)`：公共能力复用不复制（模块铁律③），`codex_proxy` 清理孤儿代理时要用它
/// 跑 netstat/taskkill —— 那两个正是 `base_command` 会解析成 System32 绝对路径的工具。
pub(crate) fn run_capture_raw(program: &str, args: &[&str], extra_path: Option<&Path>) -> Result<(i32, String), String> {
    let mut c = base_command(program);
    c.args(args);
    with_path(&mut c, extra_path);
    capture(c, program)
}

fn capture(mut c: std::process::Command, what: &str) -> Result<(i32, String), String> {
    let out = c
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("启动 {what} 失败: {e}"))?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), s))
}

/// 系统 curl.exe（Win10+ 内置）。providers.rs 也复用。
pub fn curl(args: &[&str]) -> Result<String, String> {
    // Windows schannel：证书吊销服务器不可达时（受限网络/离线）会报 CRYPT_E_REVOCATION_OFFLINE
    // （curl 35 / 0x80092013，issue #204 作图/下载握手挂）。我们只连自己的服务器、下载都按 SHA-256
    // 校验，关掉「吊销在线检查」是安全的，能救受限网络下的下载/接口握手。非 schannel 平台该 flag 为 no-op。
    let mut full: Vec<&str> = Vec::with_capacity(args.len() + 1);
    full.push("--ssl-no-revoke");
    full.extend_from_slice(args);
    let (code, out) = run_capture_raw("curl", &full, None)?;
    if code == 0 {
        return Ok(out);
    }
    // Windows：部分机器系统 curl 用的 schannel 跟我们服务器 TLS 握手失败（退出码 35/exit 0 空响应）。
    // .NET TLS 栈没这个问题，所以回退到 PowerShell Invoke-RestMethod 重试同一个请求。
    // 但「超时」(28) 是上游慢，换栈也救不了，且会再赔一个完整超时周期（作图 300s→可能叠成 10 分钟）
    // —— 直接把超时透出去，别二次干等。
    #[cfg(windows)]
    if code != 28 {
        if let Some(s) = curl_via_dotnet(args) {
            return Ok(s);
        }
    }
    Err(format!("curl 退出码 {code}：{}", tail(&out, 300)))
}

/// 把一组 curl 参数翻译成 PowerShell .NET 请求（仅覆盖本项目用到的形态：
/// GET / POST、-H 头、--data @file 或 --data 字面、URL）。绕开 schannel。
#[cfg(windows)]
fn curl_via_dotnet(args: &[&str]) -> Option<String> {
    let mut method = "GET".to_string();
    let mut url = String::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body: Option<String> = None;
    // 跟随 curl 的 -m 超时（作图/视频要几分钟）。无 -m 时保持 30s；夹到 [30,600] 防极端值。
    let mut timeout_s: u32 = 30;

    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-m" | "--max-time" => {
                if let Some(t) = args.get(i + 1) {
                    if let Ok(n) = t.parse::<u32>() {
                        timeout_s = n.clamp(30, 600);
                    }
                    i += 1;
                }
            }
            "-X" => {
                if let Some(m) = args.get(i + 1) {
                    method = m.to_string();
                    i += 1;
                }
            }
            "-H" => {
                if let Some(h) = args.get(i + 1) {
                    if let Some((k, v)) = h.split_once(':') {
                        headers.push((k.trim().to_string(), v.trim().to_string()));
                    }
                    i += 1;
                }
            }
            "--data" | "-d" | "--data-binary" => {
                if let Some(d) = args.get(i + 1) {
                    method = if method == "GET" { "POST".into() } else { method };
                    body = Some(if let Some(path) = d.strip_prefix('@') {
                        std::fs::read_to_string(path).unwrap_or_default()
                    } else {
                        d.to_string()
                    });
                    i += 1;
                }
            }
            a if a.starts_with("http") => url = a.to_string(),
            _ => {}
        }
        i += 1;
    }
    if url.is_empty() {
        return None;
    }

    // body 落临时文件，用 -InFile 读，避免引号/中文/转义地狱
    let body_file = body.as_ref().map(|b| {
        let p = std::env::temp_dir().join(format!("uking-net-{}.json", std::process::id()));
        let _ = std::fs::write(&p, b);
        p
    });

    let mut hlines = String::new();
    for (k, v) in &headers {
        hlines.push_str(&format!("$h['{}']='{}';", ps_escape(k), ps_escape(v)));
    }
    let body_arg = body_file
        .as_ref()
        .map(|p| format!(" -InFile '{}' -ContentType 'application/json'", p.display()))
        .unwrap_or_default();

    // 响应**以原始字节写到临时文件**，再由 Rust 按 UTF-8 读 —— 关键修复中文乱码：
    // PowerShell 把 .Content 打到 stdout 会用系统代码页（中文机是 GBK）重编码，
    // Rust 当 UTF-8 读就花了。写字节流（RawContentStream / GetResponseStream）不经
    // 控制台编码，原样落盘，UTF-8 中文不丢。
    let resp_file = std::env::temp_dir().join(format!("uking-resp-{}.bin", std::process::id()));
    let resp_path = resp_file.display().to_string();
    let resp_ps = resp_path.replace('\\', "\\\\");

    let script = format!(
        "[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13; \
         $h=@{{}}; {hlines} \
         try {{ \
           $resp = Invoke-WebRequest -Uri '{url}' -Method {method} -Headers $h{body_arg} -TimeoutSec {timeout_s} -UseBasicParsing; \
           [IO.File]::WriteAllBytes('{resp_ps}', $resp.RawContentStream.ToArray()); \
         }} catch {{ \
           if($_.Exception.Response){{ \
             $s=$_.Exception.Response.GetResponseStream(); $ms=New-Object IO.MemoryStream; $s.CopyTo($ms); \
             [IO.File]::WriteAllBytes('{resp_ps}', $ms.ToArray()); \
           }} \
         }}"
    );

    let r = run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None);
    if let Some(p) = body_file {
        let _ = std::fs::remove_file(p);
    }
    // 读响应字节文件（UTF-8），不经 PowerShell 的控制台编码
    let result = match r {
        Ok((0, _)) => std::fs::read(&resp_file)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .filter(|s| !s.trim().is_empty()),
        _ => None,
    };
    let _ = std::fs::remove_file(&resp_file);
    result
}

#[cfg(windows)]
fn ps_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// 取字符串尾部（约 `n` 字节），**按字符边界切**。
///
/// 老写法是 `&s[s.len() - n..]` —— **字节**切片。切点一旦落在多字节字符中间就 panic，
/// 而 release profile 是 `panic=abort`：这不是「记一条错误」，是**整个应用当场没了**。
///
/// 客户机实证（issue #303，0.9.81）：拉 version.json 的 curl 失败后，拿响应正文去拼错误
/// 消息，而 0.9.81 那份很长的中文更新说明正好让切点落在「的」上 →
/// `byte index 15811 is not a char boundary` → 进程 abort → crashlog 记下
/// 「跑了 0 秒就异常退出」，客户看到的是「一打开就闪退」。
///
/// 这个函数有 16 个调用点，**全在错误路径上**，而错误消息几乎必然带中文 —— 也就是说
/// 「出错」本身成了「崩溃」的触发器：越是该给用户看清原因的时候，应用越是直接消失。
/// 往后挪到最近的字符边界（结果只会更短，不会更长）。
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

// ============================================================================
// Windows 系统代理 → 子进程环境变量（2026-08-24 从独立的 sysproxy.rs 下沉进来；
// 单开模块会新增 agent → sysproxy 一条模块间耦合边，而 agent 本来就依赖 installer 公共层）。
//
// ## 为什么要有这段（U-Chat Claude 大脑「8 天 9 连败」的根因修复）
//
// `agent/claude.rs` / `agent/codex.rs` 的 inject_path 一直**无条件清空**子进程的全部代理
// 变量（HTTP_PROXY/HTTPS_PROXY/…），初衷是「客户机 clash 拦死虾盘云国内镜像」——对走
// 虾盘云委托的轮子这是对的（api.u-claw.org.cn 国内直连，代理反而添乱）。
//
// 但 Claude Code 用**用户自己的官方凭据**时（.claude/.credentials.json 存在，
// 我们按产品红线一个 env 都不注，见 providers::delegation_env），它要直连
// **api.anthropic.com** —— 大陆网络裸连会被 403 地域拦截（实测正文
// `Failed to authenticate. API Error: 403 Request not allowed`，不到 1 秒即败）。
// 清掉代理 = 强迫它走最通不了的那条路。
//
// 客户从终端起 U-King 时 shell 常带代理变量，而**双击启动的 GUI 进程没有这些变量**
// ——于是同一个 bug 表现成「有时好、有时 3~4 秒快速失败」，08-16 起连续 9 轮全灭
// （chat.log），开发机终端里却怎么都复现不出来。
//
// 做法：读 HKCU Internet Settings 的 ProxyEnable/ProxyServer/ProxyOverride（只读，
// 纯 std reg query，与 context_menu 同一手法），翻成 HTTP_PROXY/HTTPS_PROXY/NO_PROXY。
// 判据是注册表而不是进程 env：GUI 双击启动的进程恰恰没有 env 代理，这正是要修的场景。

/// 读一个注册表值。非 Windows / 读取失败一律 None（尽力而为的补环境，不是关键路径）。
#[cfg(windows)]
fn sysproxy_reg_query(name: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            name,
        ])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW：无头/GUI 都不许闪黑窗
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // 行形如 `    ProxyServer    REG_SZ    127.0.0.1:7897` —— 取最后一个空白分隔段。
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines()
        .find(|l| l.contains(name))
        .and_then(|l| l.split_whitespace().last())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(not(windows))]
fn sysproxy_reg_query(_name: &str) -> Option<String> {
    None
}

/// 读 Windows 系统代理，返回应注入子进程的 (key, value) 列表。
///
/// - 系统代理没开 → 空（维持「清代理」后的裸连）
/// - 开了 → `[HTTP_PROXY, HTTPS_PROXY] = http://<server>`；ProxyOverride 条目翻进 NO_PROXY
///   （localhost 一类本地豁免必须带上，否则本机探针/回调会被自己代理截胡）
pub fn system_proxy_env() -> Vec<(String, String)> {
    // ProxyEnable 是 REG_DWORD，开了是 0x1
    if sysproxy_reg_query("ProxyEnable").as_deref() != Some("0x1") {
        return vec![];
    }
    let server = match sysproxy_reg_query("ProxyServer") {
        Some(s) if !s.is_empty() => s,
        _ => return vec![],
    };
    // 注册表里通常是 `127.0.0.1:7897`；也可能是 `http=…;https=…;socks=…` 的分段式，
    // 那种取 https/http 段，都没有就放弃 —— 猜错的代理比没有代理更糟。
    let proxy = if server.contains('=') {
        let pick = |proto: &str| -> Option<String> {
            server.split(';').find_map(|seg| {
                let seg = seg.trim();
                seg.strip_prefix(proto)
                    .map(|rest| rest.trim_start_matches('=').to_string())
                    .filter(|s| !s.is_empty())
            })
        };
        match pick("https=").or_else(|| pick("http=")) {
            Some(p) => p,
            None => return vec![],
        }
    } else {
        server
    };
    let url = if proxy.contains("://") {
        proxy
    } else {
        format!("http://{proxy}")
    };

    let mut v = vec![
        ("HTTP_PROXY".to_string(), url.clone()),
        ("HTTPS_PROXY".to_string(), url.clone()),
    ];
    // 本地豁免：Windows 默认含 `<local>`（本地主机名不走代理）；具名条目原样带上。
    let mut no = "localhost,127.0.0.1,::1".to_string();
    if let Some(ovr) = sysproxy_reg_query("ProxyOverride") {
        let list: Vec<String> = ovr
            .split([';', ','])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && *s != "<local>")
            .collect();
        if !list.is_empty() {
            no.push(',');
            no.push_str(&list.join(","));
        }
    }
    v.push(("NO_PROXY".to_string(), no));
    v
}

/// 按「这一轮连的是谁」决定子进程的代理环境。
///
/// - `delegated=true`（虾盘云委托，端点是国内镜像）：清代理是对的 → 返回空，调用方照旧 env_remove。
/// - `delegated=false`（用户自持凭据直连官方端点）：返回系统代理 env，调用方逐条 c.env() 注入。
///
/// 注入侧约定：**只填子进程缺失的键**（调用方先查 std::env::var_os 是否已存在），
/// 用户/宿主显式设过的代理哪怕和系统不一致也尊重原值。
pub fn proxy_env_for(delegated: bool) -> Vec<(String, String)> {
    if delegated {
        return vec![];
    }
    system_proxy_env()
}

#[cfg(test)]
mod sysproxy_tests {
    use super::*;

    /// 委托轮（国内镜像）永远不该带代理 —— 这是老行为的正确半边，别回退。
    #[test]
    fn delegated_gets_no_proxy() {
        assert!(proxy_env_for(true).is_empty());
    }

    /// 形状约束：键名固定、URL 带 scheme、NO_PROXY 恒含 localhost 豁免。
    /// 本机系统代理没开时返回空也算过（这条断言只在「有代理」的机器上才有内容）。
    #[test]
    fn system_proxy_shape() {
        let v = system_proxy_env();
        if v.is_empty() {
            return; // 这台机器没开系统代理（或 TUN 模式），没有形状可言
        }
        let keys: Vec<&str> = v.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"]);
        for (k, val) in &v {
            if *k != "NO_PROXY" {
                assert!(val.contains("://"), "{k}={val} 缺 scheme");
            }
        }
        let no = v.iter().find(|(k, _)| k == "NO_PROXY").unwrap();
        assert!(no.1.contains("localhost"), "NO_PROXY 必须带本地豁免：{}", no.1);
    }
}

mod tests {
    use super::*;

    #[test]
    fn update_picker_uses_highest_version_from_out_of_order_responses() {
        let responses = vec![
            (VERSION_URLS[2].to_string(), r#"{"version":"1.1.0"}"#.to_string()),
            (VERSION_URLS[1].to_string(), r#"{"version":"1.3.0"}"#.to_string()),
            (VERSION_URLS[0].to_string(), r#"{"version":"1.2.0"}"#.to_string()),
        ];

        let info = pick_update_from_responses("1.0.0", &responses);
        assert!(info.checked_ok);
        assert_eq!(info.latest, "1.3.0");
        assert!(info.has_update);
    }

    #[test]
    fn update_picker_keeps_winning_source_fields_together() {
        let responses = vec![
            (VERSION_URLS[0].to_string(), r#"{"version":"1.1.0","notes":"旧日志","download_url":"https://example.com/old.exe","history":[{"version":"1.1.0","notes":"旧历史"}]}"#.to_string()),
            (VERSION_URLS[1].to_string(), r#"{"version":"1.2.0","notes":"新日志","download_url":"https://example.com/new.exe","history":[{"version":"1.2.0","notes":"新历史"}]}"#.to_string()),
        ];

        let info = pick_update_from_responses("1.0.0", &responses);
        assert_eq!(info.latest, "1.2.0");
        assert_eq!(info.notes, "新日志");
        assert_eq!(info.download_url, "https://example.com/new.exe");
        assert_eq!(info.history.first().map(|n| n.notes.as_str()), Some("新历史"));
    }

    #[test]
    fn update_picker_marks_empty_responses_as_unchecked() {
        let responses = vec![(VERSION_URLS[0].to_string(), String::new())];

        let info = pick_update_from_responses("1.0.0", &responses);
        assert!(!info.checked_ok);
        assert_eq!(info.latest, "1.0.0");
        assert!(!info.has_update);
    }

    #[test]
    fn update_picker_breaks_version_ties_by_declared_source_order() {
        let responses = vec![
            (VERSION_URLS[2].to_string(), r#"{"version":"1.2.0","notes":"第三源"}"#.to_string()),
            (VERSION_URLS[0].to_string(), r#"{"version":"1.2.0","notes":"第一源"}"#.to_string()),
        ];

        let info = pick_update_from_responses("1.0.0", &responses);
        assert_eq!(info.notes, "第一源");
    }

    /// P3b 上线前的兼容性判据（Fable 5 标为最高风险项）：给一份**老客户端从没见过**
    /// 的 skill 清单（多一个顶层字段 `provider_templates` + 里面还塞了老结构完全没有的
    /// 嵌套字段），确认 `Skill` 结构体照样能解析、`provider_templates` 之外的字段一个不少。
    ///
    /// 这不是纸面推理——`Skill`/`ToolSpec`/`NodeSpec`/`PySpec` 全篇 grep 不到
    /// `deny_unknown_fields`（serde 默认行为=忽略不认识的字段），`min_windows_build`
    /// 那次加字段也是靠这同一条规则安全下发的（见它自己的注释）。这条测试把「新字段对老
    /// 客户端安全」从一句断言变成一个可回归的判据：以后改 `Skill` 结构体，
    /// 不小心加了 `deny_unknown_fields` 或类似收紧，这条会先红，而不是等客户机装机失败。
    #[test]
    fn unknown_top_level_field_does_not_break_old_style_parsing() {
        let json = serde_json::json!({
            "skill": "uking-install-windows",
            "version": 1,
            "node": {
                "version": "v22.0.0",
                "url": "https://example.com/node.zip",
                "dir_name": "node-x"
            },
            "npm_registry": "https://registry.npmmirror.com",
            "tools": {},
            // 模拟未来某次改动顺手在顶层加的、这份测试写就那天完全不存在的字段
            "provider_templates": [
                { "name": "示例", "openai_base": "https://example.com/v1", "unknown_nested_field": 12345 }
            ],
            "yet_another_future_field_nobody_has_seen_yet": { "nested": true }
        });
        let parsed: Result<Skill, _> = serde_json::from_str(&json.to_string());
        assert!(
            parsed.is_ok(),
            "老客户端遇到没见过的顶层/嵌套字段应当照常解析，不能因为多了字段就整份清单解析失败：{:?}",
            parsed.err()
        );
        let skill = parsed.unwrap();
        assert_eq!(skill.skill, "uking-install-windows");
        assert_eq!(skill.provider_templates.len(), 1);
        assert_eq!(skill.provider_templates[0].name, "示例");
    }

    /// `free_guide` 的双向兼容判据（2026-08-24 加字段时补）。两个方向都要成立，缺一个就出事：
    ///  - **新清单 → 老/新客户端**：带 `free_guide` 的清单能解析（下同上面那条的规则）；
    ///  - **老清单 → 新客户端**：**线上那份现在还没有这个字段**，解析必须落成 `None`
    ///    而不是报错 —— 忘了 `Option` + `default` 的话，新版一发出去就会把整份清单读失败，
    ///    连带装机的 node/tools 一起没了。这个方向才是会真出人命的那个。
    #[test]
    fn free_guide_is_optional_in_both_directions() {
        let base = serde_json::json!({
            "skill": "uking-install-windows",
            "version": 1,
            "node": { "version": "v22.0.0", "url": "https://example.com/node.zip", "dir_name": "node-x" },
            "npm_registry": "https://registry.npmmirror.com",
            "tools": {},
        });

        // 老清单（没有这个字段）——线上此刻就是这个形状
        let old: Skill = serde_json::from_value(base.clone()).expect("没有 free_guide 的清单必须照常解析");
        assert!(old.free_guide.is_none(), "缺字段 = 没有教程，不是解析失败");

        // 新清单
        let mut v = base;
        v["free_guide"] = serde_json::json!({
            "version": 7,
            "checked": "2026-08-24",
            "entries": [
                { "name": "示例", "template": "OpenRouter", "summary": "有免费档",
                  "unknown_nested_field": 1 }
            ]
        });
        let new: Skill = serde_json::from_value(v).expect("带 free_guide 的清单必须能解析");
        let g = new.free_guide.expect("free_guide 应当解析出来");
        assert_eq!(g.version, 7);
        assert_eq!(g.entries.len(), 1);
        assert_eq!(g.entries[0].template.as_deref(), Some("OpenRouter"));
        // 教程条目**不许**自带端点或 Key：端点只存在于 provider_templates 一份（宪法 8），
        // Key 一律客户自己领（2026-08-24 用户拍板：没 Key 不是我们的问题）。
        let fields = serde_json::to_value(&g.entries[0]).unwrap();
        for banned in ["openai_base", "anthropic_base", "api_key", "key"] {
            assert!(fields.get(banned).is_none(), "教程条目不该有 {banned} 字段");
        }
    }

    /// 反向判据：真正跑在 exe 里的那份内嵌清单（`skills/install-windows.json`）
    /// 本身必须一直能被当前 `Skill` 结构体解析——防止只改了 JSON 却没跟结构体对齐。
    #[test]
    fn embedded_skill_still_parses_with_provider_templates_field() {
        let skill: Skill = serde_json::from_str(EMBEDDED_SKILL).expect("内嵌清单应当能解析");
        assert!(!skill.tools.is_empty(), "内嵌清单不该是空的");
    }

    /// 长路径检测的解析。装机失败存量里第 2 大桶（23 台）走的就是这条判据，
    /// 而它**唯一的失败方向是假阴性**：解析不出来就返回 `None` → 警告不出、
    /// `repairable` 也不出 → 客户什么提示都看不到，日志里却一切正常。
    ///
    /// 现场输出取自真机（pc-*** 实取），带前导空行和缩进；`0x0` 那一档是 23 台客户机的形状，
    /// 手上两台机器都是 `0x1`，**测不到**，所以只能在这儿把它钉死。
    #[test]
    fn parse_long_paths_query_reads_real_reg_output() {
        let on = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\FileSystem\r\n    LongPathsEnabled    REG_DWORD    0x1\r\n\r\n";
        assert_eq!(parse_long_paths_query(on), Some(true), "0x1 = 已开启");

        // 🔴 这一档决定 23 台客户机看不看得见那颗修复按钮。
        let off = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\FileSystem\r\n    LongPathsEnabled    REG_DWORD    0x0\r\n\r\n";
        assert_eq!(parse_long_paths_query(off), Some(false), "0x0 = 未开启，必须能判出来");

        // 键不存在（Windows 10 早期版本的默认状态）：reg 会打到 stderr、stdout 没这行。
        // 返回 None 是对的 —— 我们**不知道**，不该假装知道；但要确保它不会被当成 Some(true)。
        let missing = "\u{9519}\u{8bef}: \u{7cfb}\u{7edf}\u{627e}\u{4e0d}\u{5230}\u{6307}\u{5b9a}\u{7684}\u{6ce8}\u{518c}\u{8868}\u{9879}\u{6216}\u{503c}\u{3002}\r\n";
        assert_eq!(parse_long_paths_query(missing), None, "读不到就是 None，不许猜成已开启");

        // 只有键名没有值（畸形输出）：同样必须是 None，不能 panic、更不能判成开启。
        assert_eq!(parse_long_paths_query("    LongPathsEnabled\r\n"), None);
        // 十进制而非 0x 前缀（不该出现，但别 panic）
        assert_eq!(parse_long_paths_query("    LongPathsEnabled    REG_DWORD    1\r\n"), None);
    }

    /// 摆出 pc-*** 上 360 留下的现场：289 个文件一个不少，**只少四个 .NET 宿主文件**。
    ///
    /// 为什么这条值得单独存在：老判据是「`pwsh.exe` 在不在」，这个现场下它**永远是绿的** ——
    /// 报告说已就绪，客户每开一个终端都是 `退出码 2147516547`。判据得能分辨这两种世界。
    /// 只测纯函数那半截：不 spawn、不碰 env（env 是进程全局的，并行测试串起来就是 flake）。
    #[cfg(windows)]
    #[test]
    fn gutted_pwsh_names_exactly_what_the_av_ate() {
        let d = std::env::temp_dir().join(format!("uking-pwsh-health-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pwsh.exe"), b"stub").unwrap();
        std::fs::write(d.join("System.Private.CoreLib.dll"), b"stub").unwrap();

        assert_eq!(
            pwsh_missing_host_files(&d),
            vec!["hostfxr.dll", "hostpolicy.dll", "coreclr.dll", "clrjit.dll"],
            "被掏空的 pwsh 必须点名说出少了谁 —— 不然终端里只能吐一句「起不来」"
        );
        // 反向：四个补齐就不该再报缺（此时才轮到真 spawn 去验，那一层交给 --pwsh-test 真机跑）
        for f in PWSH_HOST_FILES {
            std::fs::write(d.join(f), b"stub").unwrap();
        }
        assert!(pwsh_missing_host_files(&d).is_empty());

        let _ = std::fs::remove_dir_all(&d);
    }

    /// **反向用例**：沙箱里写用户 PATH 必须是空操作 —— 真实用户 PATH 一个字节都不能动。
    ///
    /// 为什么这条值得单独存在：护栏原来只长在 `persist_python_scripts_path` 一处，
    /// 而 `add_user_path` 另有三个调用点（便携 Node 的 bin、便携 Git 的 cmd 与 bin）
    /// 全都没设防 —— 沙箱跑一次装机就把沙箱目录写进开发机的用户 PATH，跑完即删，
    /// 留下死路径。**跑道自己污染真实状态，它验出来的一切都不作数。**
    ///
    /// 判据钉在**真实用户 PATH 的内容**上，不是钉在「我调了那个函数吗」——
    /// 后者是「存在性检查冒充验证」，护栏被挪走照样绿。
    #[test]
    fn sandbox_never_touches_real_user_path() {
        let sentinel = std::path::PathBuf::from("Z:\\uking-sandbox-sentinel-never-persist");
        let before = read_user_path_for_test();
        // 🔴 先证明**判据本身有内容**：读不出用户 PATH 时下面两条断言会双双空对空恒绿，
        // 那就成了本仓库点过名的「恒绿考题」—— 护栏被拆掉它照样绿。
        #[cfg(windows)]
        assert!(
            !before.trim().is_empty(),
            "读不到真实用户 PATH —— 判据是空的，这条用例证明不了任何事"
        );

        crate::testsandbox::with_sandbox("addpath", &[], |_root| {
            assert!(sandboxed(), "进了沙箱 UKING_TEST_HOME 却没生效，这条用例就白跑了");
            // 沙箱里：必须**成功返回**（装机流程不该因为护栏而失败），但什么都没写。
            add_user_path(&sentinel).expect("沙箱里写 PATH 应当是无害的空操作，不该报错");
        });

        let after = read_user_path_for_test();
        assert!(
            !after.contains("uking-sandbox-sentinel-never-persist"),
            "沙箱把哨兵目录写进了真实用户 PATH —— 护栏没兜住"
        );
        assert_eq!(before, after, "沙箱改动了真实用户 PATH");
    }

    /// 读当前真实用户 PATH（只读，不写）。非 Windows 上读 ~/.zshrc 内容代替。
    fn read_user_path_for_test() -> String {
        #[cfg(windows)]
        {
            let ps = "[Environment]::GetEnvironmentVariable('Path','User')";
            run_capture_raw("powershell", &["-NoProfile", "-NonInteractive", "-Command", ps], None)
                .map(|(_, out)| out)
                .unwrap_or_default()
        }
        #[cfg(not(windows))]
        {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::Path::new(&h).join(".zshrc"))
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default()
        }
    }

    /// 便携 Python 解压必须**跳过 .pdb 调试符号**，其余文件一个不少。
    ///
    /// 由 issue #284/#286/#288/#324 逼出来（0.9.80 一天内 9 条）：客户机上内置解压器
    /// 失败的文件**总是 .pdb**（`_asyncio.pdb`、`_elementtree.pdb`）。这些是调试符号，
    /// 跑 Python 一个字节都用不到，本机实测却占 40 个文件 / 61.5 MB —— 写盘越久，
    /// 被杀软实时扫描盯上的窗口越长，而 `.pdb` 恰是杀软最爱扫的那类。
    ///
    /// 这条用例守两头：**该跳的跳了**（不然改动白做）、**不该跳的一个没少**
    /// （不然就是把 Python 解坏了，比原来更糟）。
    #[cfg(windows)]
    #[test]
    fn extract_skips_pdb_but_keeps_everything_else() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join(format!("uking-pdb-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("建临时目录");
        let pkg = tmp.join("t.tar.gz");

        // 造一个像便携 Python 的包：正常文件 + 调试符号混在一起
        {
            let f = std::fs::File::create(&pkg).expect("建包");
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            for (name, body) in [
                ("python/python.exe", &b"MZfake"[..]),
                ("python/Lib/encodings/__init__.py", &b"# codecs"[..]),
                ("python/DLLs/_asyncio.pyd", &b"pyd"[..]),
                ("python/DLLs/_asyncio.pdb", &b"DEBUG-SYMBOLS-SHOULD-NOT-LAND"[..]),
                ("python/DLLs/_elementtree.pdb", &b"DEBUG-SYMBOLS-SHOULD-NOT-LAND"[..]),
            ] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, name, body).expect("写条目");
            }
            b.into_inner().expect("收尾").finish().expect("gz 收尾").flush().ok();
        }

        let dest = tmp.join("out");
        std::fs::create_dir_all(&dest).expect("建目标目录");
        extract_tar_gz_rs(&pkg.display().to_string(), &dest).expect("解压应当成功");

        assert!(dest.join("python/python.exe").exists(), "python.exe 被漏掉了 —— 解压把 Python 解坏了");
        assert!(dest.join("python/Lib/encodings/__init__.py").exists(), "encodings 被漏掉了（客户报错正是缺它）");
        assert!(dest.join("python/DLLs/_asyncio.pyd").exists(), ".pyd 是真模块，不该跳");
        assert!(!dest.join("python/DLLs/_asyncio.pdb").exists(), ".pdb 应当被跳过");
        assert!(!dest.join("python/DLLs/_elementtree.pdb").exists(), ".pdb 应当被跳过");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 只认扩展名，别把 `xxx.pdb.py` 这种名字里带 pdb 的真文件误伤。
    #[test]
    fn skip_rule_matches_extension_only() {
        assert!(skip_in_archive(Path::new("python/DLLs/_ssl.pdb")));
        assert!(skip_in_archive(Path::new("a/b/C.PDB")));
        assert!(!skip_in_archive(Path::new("python/DLLs/_ssl.pyd")));
        assert!(!skip_in_archive(Path::new("pkg/pdb.py")));
        assert!(!skip_in_archive(Path::new("pkg/my.pdb.py")));
    }

    /// 回归钉子（issue #340）：**Windows 专属的 run 步骤必须标 `os: "windows"`。**
    ///
    /// Mac 和 Windows 读的是**同一份** `install-windows.json`（文件名是历史包袱）。清单里那些
    /// `%SystemRoot%\…\powershell.exe` / `call …` / `exit /b` 的步骤在 macOS 上会被 `sh` 执行，
    /// 报 `sh: line 0: fg: no job control` → 整条安装被判失败 → 进修复循环 → 修复步骤同样是
    /// PowerShell、同样失败。客户看到的是「Mac 装不上」，而 pip 依赖其实早就装完了、
    /// hermes 命令甚至能用（#340 现场：`已装: hermes=true`，界面却说装不上）。
    ///
    /// 这条为什么必须是用例：**开发机是 Windows，怎么测都是绿的** —— 和 pip.ini 那个坑同一个形状。
    #[test]
    fn windows_only_steps_are_tagged_so_mac_skips_them() {
        let skill: Skill = serde_json::from_str(EMBEDDED_SKILL).expect("内嵌清单解析失败");
        let mut untagged: Vec<String> = Vec::new();
        for (tool_id, tool) in &skill.tools {
            for (bucket, steps) in [("steps", &tool.steps), ("repair", &tool.repair)] {
                for (i, step) in steps.iter().enumerate() {
                    let Step::Run { cmd, os, .. } = step else { continue };
                    let c = cmd.to_lowercase();
                    let windows_only = c.contains("%systemroot%")
                        || c.contains("powershell.exe")
                        || c.contains("exit /b")
                        || c.contains("call npm");
                    if windows_only && os.as_deref() != Some("windows") {
                        untagged.push(format!("{tool_id}.{bucket}[{i}]: {}", &cmd[..cmd.len().min(70)]));
                    }
                }
            }
        }
        assert!(
            untagged.is_empty(),
            "这些步骤是 Windows 专属写法却没标 os=windows，Mac 上会把整条安装拖垮：\n{}",
            untagged.join("\n")
        );
    }

    /// 平台标签只认这三个值 —— 写错（比如 "win"）会被当成「和本机不匹配」**静默跳过**，
    /// 于是 Windows 上该跑的步骤也不跑了，而且没有任何报错。
    #[test]
    fn os_tags_are_spelled_correctly() {
        let skill: Skill = serde_json::from_str(EMBEDDED_SKILL).expect("内嵌清单解析失败");
        for (tool_id, tool) in &skill.tools {
            for steps in [&tool.steps, &tool.repair] {
                for step in steps.iter() {
                    if let Step::Run { os: Some(tag), .. } = step {
                        assert!(
                            matches!(tag.as_str(), "windows" | "macos" | "linux"),
                            "{tool_id} 里有个拼错的平台标签：{tag:?}"
                        );
                    }
                }
            }
        }
    }

    /// 回归钉子（pc-***）：**pip.ini 正文必须是纯 ASCII，任何情况下都是。**
    ///
    /// pip 读配置用系统 ANSI 代码页（`locale.getpreferredencoding()`，中文 Windows = cp936），
    /// 我们写文件却是 UTF-8。混进一个非 ASCII 字节 → pip 对**每一次调用**报
    /// `Configuration file contains invalid cp936 characters` 退出 2，这台机器上的 pip 就此全废。
    ///
    /// 这条为什么必须是用例而不是靠人看：开发机用户名是 ASCII，`cargo check`、`pnpm build`、
    /// 无头自检、conformance 全都绿 —— 病只在中文用户名的客户机上发作，而那时日志还会把它
    /// 报成「解压不完整」，指向完全错误的方向（真实代价：客户一周装不上 Hermes，
    /// 期间被自愈反复删 Python 重下几十 MB）。
    #[test]
    fn pip_config_body_is_always_ascii() {
        // 非 ASCII 的占位用户名：cache-dir 必须被整行丢掉，其余配置照常保留。
        let (body, dropped) = pip_config_body(
            "https://mirrors.aliyun.com/pypi/simple/",
            "https://pypi.tuna.tsinghua.edu.cn/simple/",
            "mirrors.aliyun.com pypi.tuna.tsinghua.edu.cn",
            r"C:\Users\张三\.uking\cache\pip",
        );
        assert!(body.is_ascii(), "pip.ini 正文混进了非 ASCII：{body:?}");
        assert_eq!(dropped, 0, "cache-dir 应在拼装阶段就被略过，而不是靠闸门兜");
        assert!(!body.contains("cache-dir"), "非 ASCII 的 cache-dir 不许写进 pip.ini");
        assert!(body.contains("index-url = https://mirrors.aliyun.com/pypi/simple/"));
        assert!(body.contains("prefer-binary = true"));

        // ASCII 路径：cache-dir 该写还是要写，别把功能一起砍了。
        let (body, dropped) = pip_config_body(
            "https://mirrors.aliyun.com/pypi/simple/",
            "https://pypi.tuna.tsinghua.edu.cn/simple/",
            "mirrors.aliyun.com",
            r"C:\Users\dev\.uking\cache\pip",
        );
        assert!(body.is_ascii());
        assert_eq!(dropped, 0);
        assert!(body.contains(r"cache-dir = C:\Users\dev\.uking\cache\pip"));

        // 闸门兜底：非 ASCII 从别的字段（如服务器下发的镜像 URL）混进来也必须被拦下。
        let (body, dropped) = pip_config_body(
            "https://镜像.example.com/simple/",
            "https://pypi.tuna.tsinghua.edu.cn/simple/",
            "mirrors.aliyun.com",
            r"C:\Users\dev\.uking\cache\pip",
        );
        assert!(body.is_ascii(), "闸门没拦住非 ASCII 的 index-url：{body:?}");
        assert_eq!(dropped, 1, "只该丢掉出问题的那一行");
        assert!(body.contains("prefer-binary = true"), "其余配置不该被牵连");
    }

    /// 回归钉子（pc-*** / Issue #223）：子进程输出**必须 lossy 解码，不许整行丢**。
    ///
    /// 老写法 `lines().map_while(Result::ok)` 遇到非 UTF-8 的行会**停止读整条流**（实测这条
    /// 用例在老写法下拿到的是 `[]`，一行都没有）。中文 Windows 上子进程输出是 GBK，报错里只要
    /// 带一个中文（路径中的用户名、本地化错误文案）后面就全没了，失败上报退化成
    /// 「命令退出码 2：<命令行>」—— 于是**越是撞上编码故障的机器，我们越拿不到它的错误原文**。
    /// 这条用例直接喂 GBK 字节，断言行还在、后续行也还在、且能认出关键英文部分。
    #[test]
    fn subprocess_output_survives_non_utf8_lines() {
        // pip 在中文 Windows 上真正吐出来的那一行：ASCII 报错 + GBK 编码的中文用户名。
        // 某中文用户名的 GBK 字节例 CD F5 BF A1（不是合法 UTF-8，老写法到这里整行丢弃）。
        let mut bytes = b"Configuration file contains invalid cp936 characters in C:\\Users\\".to_vec();
        bytes.extend_from_slice(&[0xCD, 0xF5, 0xBF, 0xA1]);
        bytes.extend_from_slice(b"\\.uking\\runtime\\python\\pip.ini.\r\n");
        bytes.extend_from_slice(b"second line stays too\n");

        let mut got = Vec::new();
        read_lines_lossy(&bytes[..], |l| got.push(l));

        assert_eq!(got.len(), 2, "非 UTF-8 的行被丢了：{got:?}");
        assert!(got[0].contains(PIP_BAD_CONFIG_MARK), "错误原文没留下：{:?}", got[0]);
        assert!(got[0].ends_with("pip.ini."), "行尾被截断：{:?}", got[0]);
        assert!(!got[0].ends_with('\r'), "CRLF 没清干净：{:?}", got[0]);
        assert_eq!(got[1], "second line stays too");
    }

    /// 回归钉子（pc-***）：pip 读不了配置 ≠ Python 解压坏了，两者处方相反。
    /// 误判成 `Broken` 会去删整个便携 Python 重下几十 MB —— 病治不好，用户已装的包还没了。
    #[test]
    fn pip_config_error_is_not_mistaken_for_a_broken_extract() {
        let out = r"Configuration file contains invalid cp936 characters in C:\Users\x\.uking\runtime\python\pip.ini.";
        assert!(out.contains(PIP_BAD_CONFIG_MARK));
        // 代码页随系统变（日文 cp932 / 韩文 cp949），标记只匹配前半截才不会漏。
        assert!("Configuration file contains invalid cp932 characters in x".contains(PIP_BAD_CONFIG_MARK));
        // 真的解压坏了长这样，绝不能被这条标记误捕。
        assert!(!"No module named pip".contains(PIP_BAD_CONFIG_MARK));
    }

    /// 回归钉子（测试报告 #011）：`tool_installed` 必须**先看文件、后起进程**。
    ///
    /// 两种顺序答案完全一样（是个 OR），所以功能测试永远抓不到这条 —— 它只体现在时间上。
    /// 而 `list_tools` 每次打开页面都调它一轮，本机实测起进程的代价是：
    /// claude 272ms / codex 303ms / openclaw 316ms / **hermes 2331ms**，合计约 3.2 秒。
    /// 这就是客户说的「切进阶页卡一下」。
    ///
    /// 只对「文件确实躺在 search_paths 里」的工具断言 —— 没装的本来就该走 probe，慢是应该的。
    /// 所以这条用例在没装任何工具的干净机上会自动跳过，不会假红。
    #[test]
    fn tool_installed_looks_at_files_before_spawning_processes() {
        let exts: &[&str] = if cfg!(windows) {
            &["", ".cmd", ".exe", ".bat", ".ps1"]
        } else {
            &[""]
        };
        let dirs = search_paths(portable_node_dir().as_deref());
        let mut checked = 0;
        for cmd in ["claude", "codex", "openclaw", "hermes", "node"] {
            let on_disk = dirs
                .iter()
                .any(|d| exts.iter().any(|e| d.join(format!("{cmd}{e}")).exists()));
            if !on_disk {
                continue; // 这台机器上没装它，跳过
            }
            checked += 1;
            let t0 = std::time::Instant::now();
            assert!(tool_installed(cmd), "{cmd} 的文件就在 search_paths 里，却被判成未安装");
            let ms = t0.elapsed().as_millis();
            assert!(
                ms < 250,
                "{cmd} 的 tool_installed 花了 {ms}ms —— 它又去起进程跑 `--version` 了。\
                 文件检查是 0 成本的，必须排在 probe 前面（见函数头注释）"
            );
        }
        eprintln!("（本机可断言的工具数：{checked}）");
    }

    /// `tail` 在任何切点上都不许 panic。
    ///
    /// 它有 16 个调用点、**全在错误路径上**，而错误消息几乎必然带中文；release 是
    /// `panic=abort`，切在字符中间就是整个应用当场消失（issue #303：客户「一打开就闪退」，
    /// crashlog 里是「跑了 0 秒就异常退出」）。所以把每一个可能的 n 都过一遍 ——
    /// 只要有一个切点落在多字节字符中间，这条用例就会以 panic 的形式变红。
    #[test]
    fn tail_never_splits_a_multibyte_char() {
        // 中文 + emoji（4 字节）+ ASCII 混排，尽量覆盖各种字符宽度
        let s = "① 工作台左栏大改，挑专家 🧭 的确如此 abc".repeat(30);
        for n in 1..=s.len() {
            let out = tail(&s, n);
            assert!(out.is_char_boundary(0), "n={n} 切出了非法字符串");
        }
        // 短于 n 时原样返回（去空白），不加省略号
        assert_eq!(tail("abc", 10), "abc");
        assert_eq!(tail("中文", 100), "中文");
        // 真实回归：#303 的现场形状 —— 长中文 JSON 尾部取 300 字节
        let json = format!(r#"{{"version":"0.9.81","notes":"{}"}}"#, "工作台左栏大改，挑专家、配定时任务都不用离开工作台".repeat(200));
        assert!(tail(&json, 300).ends_with("\"}"));
    }

    /// 自升级替换脚本正文**必须全 ASCII**。
    ///
    /// cmd.exe 按系统 ANSI 代码页（中文 Windows = 936/GBK）解析 .bat，Rust 写的是 UTF-8。
    /// 正文里只要混进一个非 ASCII 字符（历史写法把安装路径直接内插进去，而 pc-*** 的
    /// 用户名是 `demo（无密码）`），整份脚本当场乱码：连第一行 echo 到日志都写不出去，
    /// 升级**无声失败还查无实据**。路径一律靠 %~dp0 在运行时推导，正文只放 ASCII 文件名。
    #[test]
    fn updater_script_is_pure_ascii() {
        let s = updater_script(12345, "u-king-mini.exe", ".U-King.apply.exe");
        assert!(
            s.is_ascii(),
            "替换脚本正文出现非 ASCII 字符 —— 中文路径的机器上会整份乱码：{:?}",
            s.chars().filter(|c| !c.is_ascii()).collect::<String>()
        );
        // 路径必须来自 %~dp0，不能是内插的绝对路径
        assert!(s.contains("set \"D=%~dp0\""), "缺少 %~dp0 推导");
        assert!(s.contains("set \"EXE=%D%u-king-mini.exe\""));
        assert!(s.contains("set \"NEW=%D%.U-King.apply.exe\""));
        assert!(!s.contains(":\\"), "正文里不该出现绝对路径盘符");
    }

    #[cfg(windows)]
    #[test]
    fn cli_guard_uses_only_ascii_and_recognizes_the_legacy_unicode_shim() {
        let home = Path::new(r"C:\Users\demo（无密码）");
        let target = home.join(r".uking\runtime\node\codex.cmd");
        let script = cli_command_guard_script(&target, home, None).expect("portable target");
        assert!(script.is_ascii(), "cmd body must not contain a Unicode path: {script:?}");
        assert!(script.contains(r#"call "%USERPROFILE%\.uking\runtime\node\codex.cmd" %*"#));
        let old = format!("@echo off\r\n\"{}\" %*\r\n", target.display());
        assert!(is_legacy_uking_command_guard(&old, "codex"));
        let old_pi = format!("@echo off\r\n\"{}\" %*\r\n", home.join(r".uking\runtime\node\pi.cmd").display());
        assert!(is_legacy_uking_command_guard(&old_pi, "pi"));
        assert!(!is_legacy_uking_command_guard("@echo off\r\ncall C:\\custom.cmd %*\r\n", "codex"));
    }

    /// 「自动升级失败」账本：**同一目标版本累加、换版本清零、读得回原因**。
    ///
    /// 这条守的是「老是有新版本、就是升不上去」那个循环的出口：界面要在失败**第一次**之后
    /// 就改口推「下载安装包重装」，靠的就是这个计数。计数错了（比如每次都被覆盖成 1、
    /// 或旧版本的账被算到新版本头上），界面要么永远不改口、要么一上来就劝人重装。
    #[test]
    fn update_failure_ledger_counts_per_target_version() {
        let dir = std::env::temp_dir().join(format!("uking-updhealth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(update_failures_in(&dir, "0.9.89").0, 0, "没失败过就该是 0");

        assert_eq!(record_update_failure_in(&dir, "0.9.89", "杀软锁住了 exe"), 1);
        assert_eq!(record_update_failure_in(&dir, "0.9.89", "杀软锁住了 exe"), 2);
        let (n, why) = update_failures_in(&dir, "0.9.89");
        assert_eq!(n, 2);
        assert_eq!(why, "杀软锁住了 exe", "失败原因要能原样读回来（要给用户一个交代）");

        // 换了目标版本 = 换了一次机会：新版可能恰好修了升不动的原因，不许继承旧账。
        assert_eq!(record_update_failure_in(&dir, "0.9.90", "下载失败"), 1);
        assert_eq!(update_failures_in(&dir, "0.9.89").0, 0, "旧版本的账不该算到别的版本头上");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 替换脚本是在**本进程退出之后**才跑的：它失败时暂存元数据已被消费，
    /// 唯一还能问「那次想升到哪一版」的就是 pending 标记。丢了它 = 脚本的失败永远进不了账本，
    /// 界面也就永远不会改口（这正是升级失败里最沉默的那一半）。
    #[cfg(windows)]
    #[test]
    fn pending_target_survives_until_next_start() {
        let dir = std::env::temp_dir().join(format!("uking-updpending-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(take_update_pending(&dir), None, "没升级过就没有 pending");
        mark_update_pending(&dir, "0.9.89");
        assert_eq!(take_update_pending(&dir).as_deref(), Some("0.9.89"));
        assert_eq!(take_update_pending(&dir), None, "取过一次就该清掉，不能每次开机都记一笔失败");

        // pending 与失败计数共用一个文件，互相不能踩：先记了一次失败，再标 pending，两者都在。
        record_update_failure_in(&dir, "0.9.89", "替换脚本没换成");
        mark_update_pending(&dir, "0.9.89");
        assert_eq!(update_failures_in(&dir, "0.9.89").0, 1, "标 pending 不该抹掉已有的失败计数");
        assert_eq!(take_update_pending(&dir).as_deref(), Some("0.9.89"));
        assert_eq!(update_failures_in(&dir, "0.9.89").0, 1, "取 pending 也不该动失败计数");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 端到端：在**中文目录**里真跑一遍替换脚本，断言旧文件确实被换成新的。
    ///
    /// 上面那条只证明「正文不乱码」，证明不了「真能换」。这条把整份脚本在
    /// `…\demo（无密码）\` 这种路径下用 cmd 实跑：这正是 pc-*** 的现场形状。
    /// 被替换目标用 `.cmd`（内容 `@exit`），因为脚本结尾会 `start` 它 —— 拿假 exe 当靶子
    /// 会让 Windows 弹「不是有效应用」的框，测试不该在别人屏幕上弹东西。
    #[cfg(windows)]
    #[test]
    fn updater_script_swaps_under_non_ascii_path() {
        let dir = std::env::temp_dir().join("uking-swap-test").join("demo（无密码）");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (target, staged) = ("u-king-test.cmd", ".U-King.apply.cmd");
        std::fs::write(dir.join(target), b"@rem OLD\r\n@exit\r\n").unwrap();
        std::fs::write(dir.join(staged), b"@rem NEW\r\n@exit\r\n").unwrap();

        // pid 取一个合法但几乎不可能存在的值，让脚本的等待循环立刻放行
        let bat = dir.join(".uking-update.bat");
        std::fs::write(&bat, updater_script(999_999, target, staged)).unwrap();
        // ⚠️ **不看退出码**：脚本最后一行 `del "%~f0"` 把自己删了，cmd 随后想读下一行却
        // 发现文件没了，必定报「找不到指定的文件」并以 1 退出 —— 这时替换其实已经全部做完。
        // 生产里 spawn_swap 是 detached 起的，没人读这个退出码。判据只能是**留下的事实**。
        let _ = base_command("cmd").args(["/C", &bat.to_string_lossy()]).output();

        let got = std::fs::read_to_string(dir.join(target)).unwrap_or_default();
        // ⚠️ 这份日志是 **cmd 用系统 ANSI 代码页（中文机 = GBK）写的**：`%date%` 里带「周五」
        // 这类中文，不是合法 UTF-8。用 read_to_string 会直接 Err，再 unwrap_or_default()
        // 就悄悄变成空串 —— 于是「升级明明成功了」被读成「一个字都没写」。按字节读再有损转。
        let log = std::fs::read(dir.join(".uking-update.log"))
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        let flag = dir.join(".uking-updated").exists();
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        // 清理放到断言之后，失败时目录还留着好人工看

        assert!(
            !log.is_empty(),
            "没写出 .uking-update.log —— 正是 pc-*** 的症状：脚本整份乱码，连第一行 echo 都落不了盘。\n\
             目录={dir:?}\n目录残留={leftover:?}"
        );
        assert!(log.contains("swap ok"), "日志里没有 swap ok，实际内容：{log}｜目录残留：{leftover:?}");
        assert!(got.contains("NEW"), "目标文件没被换成新版，实际内容：{got:?}");
        assert!(flag, "缺少 .uking-updated 标记（前端靠它显示「已升级到新版」）");
        // 全过了才清场；挂了就把现场留在 %TEMP%\uking-swap-test 里等人来看
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> String {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, bytes).unwrap();
        p.display().to_string()
    }

    #[test]
    fn verify_download_checks_size_and_hash() {
        // "abc" 的 SHA-256（FIPS 180-4 标准向量）
        let sha_abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let f = write_tmp("uking-verify-test-abc.bin", b"abc");

        // 正确哈希 + 大小达标 → 通过
        assert!(verify_download(&f, sha_abc, 3).is_ok());
        // 哈希留空 → 只查大小，通过
        assert!(verify_download(&f, "", 3).is_ok());
        // 大小写不敏感
        assert!(verify_download(&f, &sha_abc.to_uppercase(), 0).is_ok());
        // 错误哈希 → 拒绝（挡损坏/篡改包）
        assert!(verify_download(&f, "deadbeef", 0).is_err());
        // 文件过小 → 拒绝（挡代理错误页/半包）
        assert!(verify_download(&f, "", 999).is_err());

        let _ = std::fs::remove_file(&f);
    }

    /// 「校验通过后包被动过」的判据得站得住 —— 它现在是我们给客户的**结论性**指路：
    /// 说「不是网络问题、去看杀软」。判反了就是把人往沟里带（老文案正因如此坑了 6 条 issue）。
    #[test]
    fn tampered_archive_verdict_only_fires_when_file_really_changed() {
        let p = Path::new("C:/tmp/uking-python.tar.gz");

        assert!(tamper_verdict(p, Some(10), 10).is_none(), "大小一致 = 没人动过，不许扣杀软的帽子");

        let changed = tamper_verdict(p, Some(7), 10).expect("大小对不上必须报");
        assert!(changed.contains("被改动"), "得说清是被改动了：{changed}");
        assert!(changed.contains("不是网络问题"), "结论必须钉死方向：{changed}");

        let gone = tamper_verdict(p, None, 10).expect("文件没了必须报");
        assert!(gone.contains("消失"), "得说清是消失了：{gone}");
        assert!(gone.contains("信任区"), "得给出可执行的下一步：{gone}");

        // 反向守一道：三条结论都不许再把客户往「换网络 / 关代理」上引 ——
        // 老文案就是这么坑掉 6 条 issue 的，回退了这里当场变红。
        for msg in [&changed, &gone] {
            assert!(!msg.contains("请换网络"), "结论里不许再出现换网络的指路：{msg}");
        }
    }

    #[test]
    fn embedded_manifest_carries_sha256() {
        // 证明「JSON 有 sha256 字段 → 结构体读得到 → for_platform 返回它」整条线通。
        let skill: Skill = serde_json::from_str(EMBEDDED_SKILL).expect("内嵌清单解析失败");
        assert_eq!(skill.node.sha256.len(), 64, "Node Windows sha256 应为 64 位十六进制");
        let py = skill.python.as_ref().expect("清单应含 python");
        assert_eq!(py.sha256.len(), 64, "Python Windows sha256 应为 64 位十六进制");
    }

    #[test]
    fn npm_install_keeps_hardening_across_registry_fallback() {
        let primary = npm_install_command(
            "openclaw",
            "https://registry.npmmirror.com",
            true,
            true,
        );
        let fallback = npm_install_command(
            "openclaw",
            NPM_FALLBACK_REGISTRIES[0],
            true,
            true,
        );

        assert!(primary.contains("--registry=https://registry.npmmirror.com"));
        assert!(fallback.contains("--registry=https://registry.npmjs.org"));
        for cmd in [&primary, &fallback] {
            assert!(cmd.contains("npm install -g openclaw"));
            assert!(cmd.contains("--proxy=\"\" --https-proxy=\"\""));
            assert!(cmd.contains("--no-fund --no-audit"));
            assert!(cmd.contains("--include=optional"));
            assert!(cmd.contains("--force"));
        }
    }

    #[test]
    fn cache_bust_appends_query() {
        // n==0 不改（首次正常下载，不给 CDN 添缓存碎片）
        assert_eq!(with_cache_bust("https://x/a.tar.gz", 0), "https://x/a.tar.gz");
        // 无 query → 加 ?ukcb=
        assert_eq!(with_cache_bust("https://x/a.tar.gz", 2), "https://x/a.tar.gz?ukcb=2");
        // 已有 query → 加 &ukcb=
        assert_eq!(with_cache_bust("https://x/a?b=1", 3), "https://x/a?b=1&ukcb=3");
    }

    // 内置纯 Rust 解压器是「tar.exe 缺失/损坏」客户机装 Python 的唯一救命路（issue #182/#198）。
    // 构造含嵌套目录的小 .tar.gz（模拟 python/ + Lib/encodings/），解开后校验落位、内容、可覆盖重解。
    #[cfg(windows)]
    #[test]
    fn rust_extractor_unpacks_targz() {
        let tmp = std::env::temp_dir().join(format!("uking-extract-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // 写一个 .tar.gz：python/hello.txt + python/Lib/encodings/__init__.py（模拟标准库嵌套）
        let gz_path = tmp.join("sample.tar.gz");
        {
            let f = std::fs::File::create(&gz_path).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            let mut ar = tar::Builder::new(enc);
            for (name, data) in [
                ("python/hello.txt", "hi".as_bytes()),
                ("python/Lib/encodings/__init__.py", "# encodings".as_bytes()),
            ] {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                ar.append_data(&mut h, name, data).unwrap();
            }
            ar.into_inner().unwrap().finish().unwrap();
        }

        let dest = tmp.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        // 首解：验证嵌套目录 + 内容
        extract_tar_gz_rs(&gz_path.display().to_string(), &dest).unwrap();
        let hello = dest.join("python").join("hello.txt");
        let enc_init = dest.join("python").join("Lib").join("encodings").join("__init__.py");
        assert!(hello.exists(), "python/hello.txt 应解出");
        assert!(enc_init.exists(), "嵌套 python/Lib/encodings/__init__.py 应解出");
        assert_eq!(std::fs::read_to_string(&hello).unwrap(), "hi");
        // 再解一次：覆盖已存在文件不报错（模拟 tar.exe 已写一半残留后由内置解压器接管）
        extract_tar_gz_rs(&gz_path.display().to_string(), &dest).unwrap();
        assert!(hello.exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 回归：`runtime.command_guard.inspect` 在非 Windows 上曾经是个返回 `commands: []`
    /// 的空桩，还附赠一个 `conflicts: 0` 的假绿灯 —— 调用方（含读说明书的 AI）会把它
    /// 读成「体检通过、无冲突」，而实际上一个字都没查。
    ///
    /// 这条用例**与装了什么无关**：四个名字必须条条有交代（查过了，装没装如实报），
    /// 所以它在空仓 CI 上也不会恒绿 —— 一旦有人把实现改回空 `Vec`，长度断言立刻红。
    #[test]
    fn command_guard_accounts_for_every_command_on_every_platform() {
        let r = inspect_cli_command_guard();
        let names: Vec<&str> = r.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["claude", "codex", "openclaw", "hermes"],
            "四个 CLI 必须条条有交代，不能整个空着（空桩回归）"
        );
        // 报了路径就得真有那个文件 —— 不许拿一个猜出来的路径充数。
        for c in &r.commands {
            if let Some(p) = &c.resolved_path {
                assert!(
                    std::path::Path::new(p).exists(),
                    "{} 报了 resolved_path={p}，但盘上没有这个文件",
                    c.name
                );
            }
            // 没认出「我们管的那个」就无所谓被谁抢占，不许凭空造冲突。
            if c.preferred_path.is_none() {
                assert!(!c.shadowed, "{} 没有 preferred_path 却报 shadowed=true", c.name);
            }
        }
        assert_eq!(
            r.conflicts,
            r.commands.iter().filter(|c| c.shadowed).count(),
            "conflicts 必须等于 shadowed 的条数"
        );
    }

    /// 回归：`runtime.network.inspect` 的函数体曾经整个关在 `cfg(windows)` 里，
    /// 非 Windows 那半边返回全空 —— 于是 Mac 上明明开着全局代理它也一口咬定「没有代理」，
    /// 而同一个二进制的 `--envfp`（走 [`detect_stack`]）却是对的。**同机双出口互相打脸。**
    ///
    /// ⚠️ 这条的判据强弱取决于运行机器：没配代理的机器上两边都是 None，它只能证明「没矛盾」。
    /// 但真正会退化的那种机器（配了代理的）上它是硬判据 —— 一旦有人把实现塞回 cfg 块，
    /// 这里立刻 Some vs None 炸开。
    #[test]
    fn network_inspection_agrees_with_the_stack_probe_about_the_system_proxy() {
        let via_action = inspect_runtime_network().system_proxy;
        let via_probe = system_proxy().as_deref().map(redact_proxy_endpoint);
        assert_eq!(
            via_action, via_probe,
            "同一台机器上两个出口对系统代理给出了不同答案 —— 这正是 cfg 空桩那个坑"
        );
    }
}
