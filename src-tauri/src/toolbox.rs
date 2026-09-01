//! 厨具工具箱 —— 给本机 AI 装「能力工具」（ffmpeg / Chrome / PowerShell 7 / Python …）。
//!
//! 立意：AI agent 要「通做所有软件」，就得把厨具备齐。这里按用途分类，360 式可选一键装：
//! 视频剪辑要 ffmpeg、网页自动化要 Chrome、终端体验要 PowerShell 7 / Windows Terminal、
//! 做 PPT/文档要 Python/LibreOffice。装法走系统包管理器（Windows=winget，macOS=brew）。
//!
//! **独立可插拔**：纯 std（只复用 installer::search_paths 做 PATH 探测，不反向依赖）。
//! 删它只动 lib.rs（去 mod+command+handler）和 App.tsx（去 import+tab）。`#[tauri::command]`
//! 全在 lib.rs 转调（本模块不碰 AppHandle，进度用 `|msg|` 回调传出）。

use serde::Serialize;
use std::path::PathBuf;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 工具定义（静态目录）。
struct ToolDef {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    /// 分类：视频音频 / 网页浏览 / 终端环境 / 文档办公 / 开发工具
    category: &'static str,
    /// 用途标签（漫剧 / 教案 / 网页自动化…），前端按需推荐用
    uses: &'static str,
    winget_id: &'static str,
    brew_id: &'static str,
    /// PATH 里能探到就算已装（可执行名，不含扩展名）
    probe_cmds: &'static [&'static str],
    /// **Windows** 的绝对路径模板（含 %VAR%）能探到就算已装（GUI 应用不进 PATH 时用）
    probe_paths: &'static [&'static str],
    /// **macOS** 的绝对落点。
    ///
    /// 🔴 少了这个字段，Mac 上的 GUI 应用一律探不到：`probe_cmds` 靠 PATH，而
    /// Chrome / LibreOffice 装完根本不往 PATH 放同名命令，`.app` 也不在任何 PATH 目录下。
    /// 实测这台机器 `/Applications/Google Chrome.app` 装着 151.0.7922.138，
    /// 工具箱却一直报 `installed: false` —— 因为三条 probe_paths 全是 `%ProgramFiles%` 风格。
    ///
    /// Linux 不单开一份：那边这些东西本来就在 PATH 上，`probe_cmds` 够用。
    probe_paths_mac: &'static [&'static str],
    /// 只有 Windows 上才存在的东西（Windows Terminal 这种）。**非 Windows 上整条不显示** ——
    /// 在 Mac 的厨具清单里摆一个「请到官网下载 Windows Terminal」，
    /// 对客户是噪音，对 AI 是错误信息。
    windows_only: bool,
    /// pip 包名（非空 = 这件厨具走 `python -m pip`，不走 winget/brew）。
    /// 为什么塞进同一张表而不是另开一条安装路：客户在「厨具工具箱」里看到的是一排能力，
    /// 他不关心哪件走 winget、哪件走 pip；多一条安装路就多一处会漂移的进度/失败处理。
    pip_pkg: &'static str,
}

/// 厨具目录。新增一个厨具 = 这里加一行。分类同 category 的会在前端归到一组。
const CATALOG: &[ToolDef] = &[
    ToolDef {
        id: "ffmpeg",
        name: "FFmpeg",
        desc: "视频/音频瑞士军刀。AIGC 技能包的视频拼接(gen-stitch)、长语音合成拼接都靠它。",
        category: "视频音频",
        uses: "视频剪辑·配音拼接·做漫剧·转码",
        winget_id: "Gyan.FFmpeg",
        brew_id: "ffmpeg",
        probe_cmds: &["ffmpeg"],
        probe_paths: &[],
        probe_paths_mac: &[],
        windows_only: false,
        pip_pkg: "",
    },
    ToolDef {
        id: "chrome",
        name: "Google Chrome",
        desc: "给 AI 一个可控浏览器，做网页自动化、抓取、截图、填表（Playwright/浏览器控制需要）。",
        category: "网页浏览",
        uses: "网页自动化·抓取·截图·填表单",
        winget_id: "Google.Chrome",
        brew_id: "google-chrome",
        probe_cmds: &["chrome"],
        probe_paths: &[
            "%ProgramFiles%\\Google\\Chrome\\Application\\chrome.exe",
            "%ProgramFiles(x86)%\\Google\\Chrome\\Application\\chrome.exe",
            "%LOCALAPPDATA%\\Google\\Chrome\\Application\\chrome.exe",
        ],
        probe_paths_mac: &[
            "/Applications/Google Chrome.app",
            "~/Applications/Google Chrome.app",
        ],
        windows_only: false,
        pip_pkg: "",
    },
    ToolDef {
        id: "pwsh",
        name: "PowerShell 7",
        desc: "比系统自带 5.1 更新更强的跨平台终端。AI 跑脚本更稳、报错更少。",
        category: "终端环境",
        uses: "更强的终端·脚本环境",
        winget_id: "Microsoft.PowerShell",
        brew_id: "powershell",
        probe_cmds: &["pwsh"],
        probe_paths: &["%ProgramFiles%\\PowerShell\\7\\pwsh.exe"],
        probe_paths_mac: &["/usr/local/microsoft/powershell"],
        windows_only: false,
        pip_pkg: "",
    },
    ToolDef {
        id: "windows-terminal",
        name: "Windows Terminal",
        desc: "现代多标签终端，取代老旧的 conhost。多任务、体验好。",
        category: "终端环境",
        uses: "多标签终端·更好体验",
        winget_id: "Microsoft.WindowsTerminal",
        brew_id: "",
        probe_cmds: &["wt"],
        probe_paths: &["%LOCALAPPDATA%\\Microsoft\\WindowsApps\\wt.exe"],
        probe_paths_mac: &[],
        windows_only: true,
        pip_pkg: "",
    },
    ToolDef {
        id: "python",
        name: "Python 3",
        desc: "AI 做数据处理、做 PPT(python-pptx)、跑各类脚本的通用引擎。",
        category: "文档办公",
        uses: "做PPT·数据处理·脚本·教案",
        winget_id: "Python.Python.3.12",
        brew_id: "python@3.12",
        probe_cmds: &["python", "python3"],
        probe_paths: &[],
        probe_paths_mac: &[],
        windows_only: false,
        pip_pkg: "",
    },
    ToolDef {
        id: "libreoffice",
        name: "LibreOffice",
        desc: "免费办公套件。AI 可用它生成/转换 PPT、Word、Excel、PDF（无需装 Office）。",
        category: "文档办公",
        uses: "做PPT/文档·格式转换·教案",
        winget_id: "TheDocumentFoundation.LibreOffice",
        brew_id: "libreoffice",
        probe_cmds: &["soffice"],
        probe_paths: &["%ProgramFiles%\\LibreOffice\\program\\soffice.exe"],
        probe_paths_mac: &["/Applications/LibreOffice.app"],
        windows_only: false,
        pip_pkg: "",
    },
    ToolDef {
        id: "markitdown",
        name: "MarkItDown（读文档内核）",
        desc: "微软开源的文档转 Markdown 内核。`uking-office-read` 优先用它读客户拿来的 Word/Excel/PPT/PDF —— 实测比 pandoc 兜底少 33% 字符（= 每次调用少付 33% token）、表格提取好一大截。",
        category: "文档办公",
        uses: "读合同·读招标文件·读报表·总结 PDF",
        winget_id: "",
        brew_id: "",
        probe_cmds: &["markitdown"],
        probe_paths: &[],
        probe_paths_mac: &[],
        windows_only: false,
        // 只装办公要用的四个转换器，**不要 [all]**：那会把 pandas+numpy(91MB) 和
        // azure/youtube/语音转写一起拖进来，办公场景一个都用不上（实测 65MB vs 155MB）。
        pip_pkg: "markitdown[docx,pdf,pptx,xlsx]",
    },
    ToolDef {
        id: "pandoc",
        name: "Pandoc",
        desc: "文档格式转换神器：Markdown ↔ Word/PDF/HTML/EPUB。AI 出稿排版必备。",
        category: "文档办公",
        uses: "文档转换 md/docx/pdf",
        winget_id: "JohnMacFarlane.Pandoc",
        brew_id: "pandoc",
        probe_cmds: &["pandoc"],
        probe_paths: &[],
        probe_paths_mac: &[],
        windows_only: false,
        pip_pkg: "",
    },
    ToolDef {
        id: "git",
        name: "Git",
        desc: "版本控制。AI 拉取开源项目、管理代码改动、跑很多开发工作流都需要。",
        category: "开发工具",
        uses: "版本控制·拉项目·代码管理",
        winget_id: "Git.Git",
        brew_id: "git",
        probe_cmds: &["git"],
        probe_paths: &["%ProgramFiles%\\Git\\cmd\\git.exe"],
        probe_paths_mac: &[],
        windows_only: false,
        pip_pkg: "",
    },
];

/// 单个厨具的运行时状态（给前端）。
#[derive(Serialize, Clone)]
pub struct ToolStatus {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub category: String,
    pub uses: String,
    pub installed: bool,
    /// 本平台能否一键装（winget/brew 可用且有对应包 id）
    pub can_install: bool,
    /// 手动安装命令提示（一键失败时给用户看）
    pub manual_hint: String,
}

/// 展开 `%VAR%`（Windows 路径模板）和开头的 `~`（unix 落点，如 `~/Applications/...`）。
///
/// `~` 走 `installer::user_home_dir()` 而不是直接读 `$HOME` —— 那一份认 `UKING_TEST_HOME`，
/// 沙箱测试才不会探到真实用户目录。
fn expand_env(t: &str) -> String {
    let mut s = t.to_string();
    for var in [
        "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "LOCALAPPDATA",
        "USERPROFILE", "APPDATA", "ProgramData", "SystemRoot", "windir",
    ] {
        if let Ok(v) = std::env::var(var) {
            s = s.replace(&format!("%{var}%"), &v);
        }
    }
    if let Some(rest) = s.strip_prefix("~/") {
        s = crate::installer::user_home_dir().join(rest).display().to_string();
    }
    s
}

/// 读注册表里的**持久** PATH（HKCU / HKLM）。winget 装完会立刻改持久 PATH，但当前进程
/// 的 env 还是启动时的旧值 —— 直接读注册表能立刻探到刚装的工具，不必重启应用。
#[cfg(windows)]
fn persistent_path_dirs() -> Vec<PathBuf> {
    use std::os::windows::process::CommandExt;
    let mut out = Vec::new();
    for hive in [
        "HKCU\\Environment",
        "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
    ] {
        let Ok(o) = std::process::Command::new("reg")
            .args(["query", hive, "/v", "Path"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else { continue };
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines() {
            let t = line.trim();
            if !t.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case("Path")) {
                continue;
            }
            for ty in ["REG_EXPAND_SZ", "REG_SZ"] {
                if let Some(pos) = line.find(ty) {
                    for p in line[pos + ty.len()..].trim().split(';').filter(|s| !s.is_empty()) {
                        out.push(PathBuf::from(expand_env(p)));
                    }
                }
            }
        }
    }
    out
}

/// 探测用的目录集：installer::search_paths（便携 node/git/python 等）+ **真实系统 PATH**
/// （ffmpeg 这类装在任意 PATH 目录的工具靠这个）+ winget 的 CLI shim 目录（winget 装完的
/// 命令行工具落在这，且刚装完当前进程 PATH 可能还没刷新，直接查这个目录能立刻探到）。
fn probe_dirs() -> Vec<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut dirs = crate::installer::search_paths(None);
    if let Ok(path) = std::env::var("PATH") {
        for p in path.split(sep).filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(p));
        }
    }
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("WinGet").join("Links"));
        }
        dirs.extend(persistent_path_dirs()); // winget 装完立刻改注册表 PATH，读它才能即时探到
    }
    dirs
}

/// 该厨具是否已装：PATH/winget 目录探到任一 probe_cmd，或**本平台**的任一绝对落点存在。
///
/// 🔴 绝对落点必须按平台取。以前只查 `probe_paths`（全是 `%ProgramFiles%` 风格），
/// 于是 Mac 上 Chrome / LibreOffice 这类 GUI 应用一律探不到 —— 它们既不往 PATH 放同名命令，
/// `.app` 也不在任何 PATH 目录下，两条判据双双落空，结果报「没装」。
fn is_installed(def: &ToolDef) -> bool {
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };
    let dirs = probe_dirs();
    for cmd in def.probe_cmds {
        let fname = format!("{cmd}{exe_suffix}");
        if dirs.iter().any(|d| d.join(&fname).exists()) {
            return true;
        }
    }
    let abs_paths: &[&str] = if cfg!(target_os = "macos") { def.probe_paths_mac } else { def.probe_paths };
    for p in abs_paths {
        if PathBuf::from(expand_env(p)).exists() {
            return true;
        }
    }
    false
}

/// 系统包管理器是否可用（Windows=winget / macOS=brew）。
#[cfg(windows)]
fn pkg_mgr() -> Option<PathBuf> {
    winget_exe()
}
#[cfg(not(windows))]
fn pkg_mgr() -> Option<PathBuf> {
    for p in ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"] {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    crate::installer::search_paths(None)
        .into_iter()
        .map(|d| d.join("brew"))
        .find(|p| p.exists())
}

/// winget.exe 定位（应用进程 PATH 常不含 WindowsApps，需显式找）。
#[cfg(windows)]
fn winget_exe() -> Option<PathBuf> {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("Microsoft").join("WindowsApps").join("winget.exe");
        if p.exists() {
            return Some(p);
        }
    }
    crate::installer::search_paths(None)
        .into_iter()
        .map(|d| d.join("winget.exe"))
        .find(|p| p.exists())
}

/// python 可执行文件：U-King 便携版优先（跟 skill 的 ensure_python 落点一致），
/// 否则 PATH 上的 python。给 `pip_pkg` 那类厨具用。
///
/// 也是 [`crate::installer::python_for_docs`] 的回落来源 —— 读文档那条路以前只认
/// 便携版，在系统自带 python3 的 macOS/Linux 上一律判死。别再复制第四份探测逻辑。
pub(crate) fn python_exe() -> Option<PathBuf> {
    let name = if cfg!(windows) { "python.exe" } else { "python3" };
    if let Some(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok() {
        let p = PathBuf::from(home).join(".uking").join("runtime").join("python").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    crate::installer::search_paths(None)
        .into_iter()
        .flat_map(|d| [d.join(name), d.join(if cfg!(windows) { "python.exe" } else { "python" })])
        .find(|p| p.exists())
}

fn manual_hint(def: &ToolDef) -> String {
    if !def.pip_pkg.is_empty() {
        // 必须写 `python -m pip`，不能裸跑 `pip` —— 客户机上 pip 可能指向别的解释器
        return format!("python -m pip install \"{}\"", def.pip_pkg);
    }
    if cfg!(windows) {
        format!("winget install --id {} -e", def.winget_id)
    } else if !def.brew_id.is_empty() {
        format!("brew install {}", def.brew_id)
    } else {
        format!("请到官网下载 {}", def.name)
    }
}

/// 这件厨具在本平台上存不存在。
///
/// 非 Windows 上把 `windows_only` 的整条藏掉：在 Mac 的清单里摆一个
/// 「请到官网下载 Windows Terminal」，对客户是噪音，对读清单的 AI 是错误信息
/// （它会以为这台机器能装、该装）。
fn visible_here(def: &ToolDef) -> bool {
    cfg!(windows) || !def.windows_only
}

/// 按 id 探测单个厨具装没装（只读，无副作用）。
///
/// readiness 的 `optional_not_installed` 用它逐项探测 —— 那个列表以前是写死的，
/// 客户明明装了 hermes/ffmpeg 也照样被提示「去装」（pc-*** 实锤：hermes 0.19.0
/// 在跑、readiness 还说「未安装」）。**「未装」必须问机器，不许背答案。**
pub(crate) fn tool_installed_by_id(id: &str) -> bool {
    CATALOG.iter().filter(|d| d.id == id).any(is_installed)
}

/// 全部厨具 + 已装状态（前端目录用）。
pub fn list_tools() -> Vec<ToolStatus> {
    let mgr_ok = pkg_mgr().is_some();
    // pip 系厨具不看 winget/brew 在不在，看 python 在不在 —— 两条安装路的可用性判据不一样，
    // 用同一个 `mgr_ok` 会把「winget 缺失」误报成「markitdown 装不了」。
    let py_ok = python_exe().is_some();
    CATALOG
        .iter()
        .filter(|d| visible_here(d))
        .map(|d| {
            if !d.pip_pkg.is_empty() {
                return ToolStatus {
                    id: d.id.into(),
                    name: d.name.into(),
                    desc: d.desc.into(),
                    category: d.category.into(),
                    uses: d.uses.into(),
                    installed: is_installed(d),
                    can_install: py_ok,
                    manual_hint: manual_hint(d),
                };
            }
            let pkg_id = if cfg!(windows) { d.winget_id } else { d.brew_id };
            ToolStatus {
                id: d.id.into(),
                name: d.name.into(),
                desc: d.desc.into(),
                category: d.category.into(),
                uses: d.uses.into(),
                installed: is_installed(d),
                can_install: mgr_ok && !pkg_id.is_empty(),
                manual_hint: manual_hint(d),
            }
        })
        .collect()
}

/// 一键装一个厨具。进度走回调（前端 toast 实时显示）。
/// Windows 走 winget（--silent + 接受协议 + 关交互）；轮询到探测已装即成功。
pub fn install_tool(id: &str, on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let def = CATALOG
        .iter()
        .filter(|d| visible_here(d))
        .find(|d| d.id == id)
        .ok_or_else(|| format!("未知工具: {id}"))?;
    if is_installed(def) {
        return Ok(format!("{} 已安装。", def.name));
    }
    // winget/brew 装机在客户机上失败率不低（没装应用安装程序、源被墙、权限），
    // 而这些失败此前只在 UI 上闪一下就没了，远程什么都看不到。
    crate::ulog::section("toolbox", &format!("安装 {} (id={id})", def.name));
    // 把进度回调包一层：每条推给界面的话同时落日志，不必在每个 return 点补一遍，
    // 也不会漏掉中途的状态。`on_progress` 在本函数里只被平调用、从不传递给别人，包装是安全的。
    let notify = on_progress;
    let on_progress = |m: &str| {
        crate::ulog::write("toolbox", m);
        notify(m);
    };
    // ★ pip 系厨具（markitdown）：走 `python -m pip`，不碰 winget/brew。
    // 同步跑完就返回 —— pip 没有「装完还要等系统注册」的问题，不需要下面那套轮询探测。
    if !def.pip_pkg.is_empty() {
        let py = python_exe().ok_or_else(|| {
            "找不到 Python —— 先在厨具工具箱里装 Python，或跑一次任意 pip 系工具的安装（会自动装便携版）".to_string()
        })?;
        on_progress(&format!("开始安装 {}（python -m pip，走国内源）…", def.name));
        let mut cmd = std::process::Command::new(&py);
        cmd.args(["-m", "pip", "install", "--disable-pip-version-check", def.pip_pkg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().map_err(|e| format!("启动 pip 失败: {e}"))?;
        // 🔴 判据是「装完真能 import」，不是 pip 的退出码：pip 报 0 但包没落地的情况
        // 在中文用户名 + 编码错乱的机器上真实发生过（pc-*** 那条线）。
        let importable = std::process::Command::new(&py)
            .args(["-c", "import markitdown"])
            .stdin(std::process::Stdio::null())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if importable {
            on_progress(&format!("{} 安装完成 ✓", def.name));
            return Ok(format!("{} 已装好，读文档能力就绪。", def.name));
        }
        let why = String::from_utf8_lossy(&out.stderr);
        let why = why.lines().rev().take(3).collect::<Vec<_>>().join(" ");
        return Err(format!(
            "{} 没装上（pip 退出码 {:?}）：{}。手动命令：{}",
            def.name,
            out.status.code(),
            why.trim(),
            manual_hint(def)
        ));
    }

    let mgr = pkg_mgr().ok_or_else(|| {
        if cfg!(windows) {
            "未找到 winget（应用安装程序）。请在 Microsoft Store 更新「应用安装程序」后重试，或用下方手动命令安装。".to_string()
        } else {
            "未找到 Homebrew。请先装 brew（brew.sh）再重试，或按手动命令安装。".to_string()
        }
    })?;

    on_progress(&format!("开始安装 {}（走系统包管理器，可能弹一次 UAC 授权）…", def.name));

    #[cfg(windows)]
    let args: Vec<String> = vec![
        "install".into(),
        "--id".into(),
        def.winget_id.into(),
        "-e".into(),
        "--silent".into(),
        "--accept-package-agreements".into(),
        "--accept-source-agreements".into(),
        "--disable-interactivity".into(),
    ];
    #[cfg(not(windows))]
    let args: Vec<String> = vec!["install".into(), def.brew_id.into()];

    let mut cmd = std::process::Command::new(&mgr);
    cmd.args(&args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("启动安装器失败: {e}"))?;

    // 轮询：装好(探测到) → 成功；进程退出仍没探到 → 报错给手动命令。
    // 大包(Chrome/LibreOffice ~200MB)给足时间：最多 ~10 分钟。
    for i in 0..300 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if is_installed(def) {
            on_progress(&format!("{} 安装完成 ✓", def.name));
            return Ok(format!("{} 已装好，AI 现在可以用它了。", def.name));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if is_installed(def) {
                    on_progress(&format!("{} 安装完成 ✓", def.name));
                    return Ok(format!("{} 已装好。", def.name));
                }
                let code = status.code().unwrap_or(-1);
                crate::ulog::write(
                    "toolbox",
                    &format!("{} 安装未完成，安装器退出码 {code}", def.name),
                );
                return Err(format!(
                    "{} 安装未完成（安装器退出码 {code}）。多半是没点 UAC 授权，或该包需手动确认。可在终端手动跑：{}",
                    def.name,
                    manual_hint(def)
                ));
            }
            Ok(None) => {
                if i % 3 == 0 {
                    on_progress(&format!("正在安装 {}… 下载+安装中，请稍候", def.name));
                }
            }
            Err(_) => break,
        }
    }
    Err(format!("{} 安装超时。可手动跑：{}", def.name, manual_hint(def)))
}

/// 卸载一个厨具（走系统包管理器：Windows=winget uninstall / macOS=brew uninstall）。
/// 给 cleanup 的「安全卸载」用——复用 CATALOG + pkg_mgr，不重复造轮子。best-effort：
/// 装了才卸；卸完探测不到即成功，否则回退手动命令提示。**只卸包管理器装的，不 rm 系统目录**。
pub fn uninstall_tool(id: &str, on_progress: &(dyn Fn(&str) + Send + Sync)) -> Result<String, String> {
    let def = CATALOG
        .iter()
        .filter(|d| visible_here(d))
        .find(|d| d.id == id)
        .ok_or_else(|| format!("未知工具: {id}"))?;
    if !is_installed(def) {
        return Ok(format!("{} 未安装（无需卸载）。", def.name));
    }
    let mgr = pkg_mgr().ok_or_else(|| {
        if cfg!(windows) {
            "未找到 winget，无法自动卸载。请到 设置 → 应用 手动卸载。".to_string()
        } else {
            "未找到 Homebrew，无法自动卸载。".to_string()
        }
    })?;
    on_progress(&format!("正在卸载 {}（走系统包管理器）…", def.name));

    #[cfg(windows)]
    let args: Vec<String> = vec![
        "uninstall".into(),
        "--id".into(),
        def.winget_id.into(),
        "-e".into(),
        "--silent".into(),
        "--accept-source-agreements".into(),
        "--disable-interactivity".into(),
    ];
    #[cfg(not(windows))]
    let args: Vec<String> = vec!["uninstall".into(), def.brew_id.into()];

    let mut cmd = std::process::Command::new(&mgr);
    cmd.args(&args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("启动卸载器失败: {e}"))?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    if !is_installed(def) {
        on_progress(&format!("{} 已卸载 ✓", def.name));
        Ok(format!("{} 已卸载。", def.name))
    } else {
        let code = out.status.code().unwrap_or(-1);
        Err(format!(
            "{} 卸载未完成（退出码 {code}）。可手动跑：{} uninstall …",
            def.name,
            if cfg!(windows) { "winget" } else { "brew" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：Mac 的厨具清单里曾经摆着「Windows Terminal」，提示语还是
    /// 「请到官网下载 Windows Terminal」。对客户是噪音，对读清单的 AI 是错误信息 ——
    /// 它会以为这台机器能装、该装。
    ///
    /// 前端藏了不算数，安装入口也得挡住，否则走 CLI / MCP 照样能点进来。
    #[test]
    fn windows_only_tools_are_not_offered_off_windows() {
        // 判据非空：目录里确实存在至少一件 windows_only 的东西，否则这条什么也没证明。
        let declared = CATALOG.iter().filter(|d| d.windows_only).count();
        assert!(declared > 0, "目录里一件 windows_only 都没有 —— 这条用例失去判据了");

        let listed = list_tools();
        #[cfg(not(windows))]
        {
            for d in CATALOG.iter().filter(|d| d.windows_only) {
                assert!(
                    !listed.iter().any(|t| t.id == d.id),
                    "非 Windows 的清单里还摆着 {}",
                    d.id
                );
                // 列表藏了、入口没挡 = 只是把问题挪到了另一个门
                assert!(
                    install_tool(d.id, &|_| {}).is_err(),
                    "{} 在非 Windows 上仍然可以被装",
                    d.id
                );
            }
        }
        #[cfg(windows)]
        assert_eq!(listed.len(), CATALOG.len(), "Windows 上不该藏任何一件");
    }

    /// `probe_paths_mac` 里写的是 `~/Applications/...` 这种落点，展开不了就等于没写。
    /// 走 `installer::user_home_dir()` 而不是裸 `$HOME`，沙箱测试才不会探到真实用户目录。
    #[test]
    fn expand_env_understands_a_leading_tilde() {
        let got = expand_env("~/Applications/Google Chrome.app");
        assert!(!got.starts_with('~'), "`~` 没展开：{got}");
        assert!(
            got.ends_with("Applications/Google Chrome.app"),
            "展开后把尾巴弄丢了：{got}"
        );
        assert!(
            std::path::Path::new(&got).is_absolute(),
            "展开后不是绝对路径，exists() 判不了：{got}"
        );
    }

    /// GUI 应用装完不往 PATH 放同名命令，`.app` 也不在任何 PATH 目录下 ——
    /// 所以只要一件厨具是靠绝对路径认的（Windows 侧写了 probe_paths），
    /// macOS 侧就必须也有落点，否则那件东西在 Mac 上永远报「没装」。
    #[test]
    fn gui_tools_probed_by_absolute_path_on_windows_also_have_a_macos_landing_spot() {
        for d in CATALOG.iter().filter(|d| !d.windows_only && !d.probe_paths.is_empty()) {
            // git 这类命令行工具 Windows 侧写绝对路径只是兜底，PATH 上本来就有；
            // 判据落在「PATH 上根本不会有同名命令」的那些人身上。
            // 🔴 Windows 上可执行文件带后缀：`git` 在磁盘上叫 `git.exe`，npm 装的 CLI 叫 `xxx.cmd`。
            //    只 join 裸命令名的话，这个「PATH 上找得到吗」在 Windows 上**恒为 false**，
            //    于是 git 这类命令行工具也被要求给 macOS 绝对路径落点 —— 本用例在 Mac 上绿、
            //    在 Windows 上红。（这条用例本身是在 Mac 上写的、在 Mac 上跑的。）
            let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
            let path_findable = d.probe_cmds.iter().any(|c| {
                probe_dirs()
                    .iter()
                    .any(|dir| exts.iter().any(|e| dir.join(format!("{c}{e}")).exists()))
            });
            if path_findable {
                continue;
            }
            assert!(
                !d.probe_paths_mac.is_empty(),
                "{} 在 Windows 上靠绝对路径认，PATH 上又找不到，但没给 macOS 落点 —— \
                 它在 Mac 上会永远报「没装」",
                d.id
            );
        }
    }
}
