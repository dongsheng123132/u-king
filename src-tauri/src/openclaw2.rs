//! OpenClaw 2 private runtime adapter.
//!
//! This module deliberately knows nothing about ClawX or the legacy OpenClaw
//! install. Every path is rooted below `installer::uking_home()/openclaw2` and
//! every child process receives explicit config/state paths. It is the only
//! implementation behind the five `runtime.openclaw2.*` Actions.

use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
#[cfg(not(windows))]
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROFILE: &str = "uking-openclaw2";
const CONFIG_NAME: &str = "openclaw.json";
const PROFILE_NAME: &str = "profile.json";
const SUPERVISOR_NAME: &str = "supervisor.json";
const INSTALL_NAME: &str = "installed.json";

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    schema_version: u32,
    openclaw_version: String,
    node: RuntimeNode,
    openclaw: RuntimeOpenClaw,
}
#[derive(Debug, Deserialize)]
struct RuntimeNode {
    version: String,
    windows_x64_url: String,
    windows_x64_sha256: String,
}
#[derive(Debug, Deserialize)]
struct RuntimeOpenClaw {
    tarball_url: String,
    integrity: String,
}

#[derive(Clone, Debug)]
struct Paths {
    root: PathBuf,
    runtime: PathBuf,
    node: PathBuf,
    app: PathBuf,
    state: PathBuf,
    workspace: PathBuf,
    run: PathBuf,
    logs: PathBuf,
}

fn paths() -> Paths {
    paths_from_root(crate::installer::uking_home().join("openclaw2"))
}
fn paths_from_root(root: PathBuf) -> Paths {
    Paths {
        runtime: root.join("runtime"),
        node: root.join("runtime").join("node"),
        app: root.join("runtime").join("app"),
        state: root.join("state"),
        workspace: root.join("workspace"),
        run: root.join("run"),
        logs: root.join("logs"),
        root,
    }
}

fn manifest() -> Result<RuntimeManifest, String> {
    let m: RuntimeManifest =
        serde_json::from_str(include_str!("../resources/openclaw2-runtime.json"))
            .map_err(|e| format!("OpenClaw2 runtime 清单无效: {e}"))?;
    if m.schema_version != 1
        || m.openclaw_version.is_empty()
        || m.node.version.is_empty()
        || !m.node.windows_x64_url.starts_with("https://")
        || m.node.windows_x64_sha256.len() != 64
        || !m.openclaw.tarball_url.starts_with("https://")
        || !m.openclaw.integrity.starts_with("sha512-")
    {
        return Err("OpenClaw2 runtime 清单不完整或不安全".into());
    }
    Ok(m)
}

fn node_exe(p: &Paths) -> PathBuf {
    if cfg!(windows) {
        p.node.join("node.exe")
    } else {
        p.node.join("bin").join("node")
    }
}
fn npm_exe(p: &Paths) -> PathBuf {
    if cfg!(windows) {
        p.node.join("npm.cmd")
    } else {
        p.node.join("bin").join("npm")
    }
}
fn cli_file(p: &Paths) -> PathBuf {
    p.app
        .join("node_modules")
        .join("openclaw")
        .join("openclaw.mjs")
}
fn config_file(p: &Paths) -> PathBuf {
    p.state.join(CONFIG_NAME)
}
fn profile_file(p: &Paths) -> PathBuf {
    p.state.join(PROFILE_NAME)
}
fn supervisor_file(p: &Paths) -> PathBuf {
    p.run.join(SUPERVISOR_NAME)
}
fn install_file(p: &Paths) -> PathBuf {
    p.runtime.join(INSTALL_NAME)
}

fn ensure_private_path(path: &Path, p: &Paths) -> Result<(), String> {
    // Never let a lexical descendant hide behind an existing symlink. Before
    // the root exists there cannot yet be such a descendant; once it exists,
    // both the root and the nearest existing ancestor must canonicalize under
    // the same private root.
    if !path.starts_with(&p.root) {
        Err("拒绝访问 OpenClaw2 私有根目录以外的路径".into())
    } else if !p.root.exists() {
        Ok(())
    } else {
        let root = p.root.canonicalize().map_err(|e| format!("解析 OpenClaw2 私有根失败: {e}"))?;
        let ancestor = path
            .ancestors()
            .find(|candidate| candidate.exists())
            .ok_or("找不到 OpenClaw2 私有路径的已存在父目录")?
            .canonicalize()
            .map_err(|e| format!("解析 OpenClaw2 私有父目录失败: {e}"))?;
        if ancestor.starts_with(&root) {
            Ok(())
        } else {
            Err("拒绝访问 OpenClaw2 私有根目录以外的路径".into())
        }
    }
}

fn create_layout(p: &Paths) -> Result<(), String> {
    for dir in [
        &p.root,
        &p.runtime,
        &p.app,
        &p.state,
        &p.workspace,
        &p.run,
        &p.logs,
    ] {
        ensure_private_path(dir, p)?;
        fs::create_dir_all(dir).map_err(|e| format!("创建 OpenClaw2 私有目录失败: {e}"))?;
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8], p: &Paths) -> Result<(), String> {
    ensure_private_path(path, p)?;
    let parent = path.parent().ok_or("OpenClaw2 配置路径没有父目录")?;
    ensure_private_path(parent, p)?;
    fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let tmp = parent.join(format!(
        ".uking-openclaw2-{}-{}.tmp",
        std::process::id(),
        now_nanos()
    ));
    fs::write(&tmp, bytes).map_err(|e| format!("写入临时配置失败: {e}"))?;
    #[cfg(windows)]
    {
        atomic_replace_windows(&tmp, path)?;
        return Ok(());
    }
    #[cfg(not(windows))]
    fs::rename(&tmp, path).map_err(|e| format!("原子写入配置失败: {e}"))
}

#[cfg(windows)]
fn atomic_replace_windows(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_REPLACE_EXISTING: replace is a single same-volume rename; do
    // not delete the old profile first, because a crash between delete/rename
    // would destroy the only valid private configuration.
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            lp_existing_file_name: *const u16,
            lp_new_file_name: *const u16,
            dw_flags: u32,
        ) -> i32;
    }
    if unsafe { MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), MOVEFILE_REPLACE_EXISTING) } == 0 {
        let _ = fs::remove_file(from);
        Err(format!("原子替换 OpenClaw2 配置失败: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn node_supported(v: &str) -> bool {
    let nums: Vec<u32> = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|x| x.parse().unwrap_or(0))
        .collect();
    let (major, minor, patch) = (
        nums.first().copied().unwrap_or(0),
        nums.get(1).copied().unwrap_or(0),
        nums.get(2).copied().unwrap_or(0),
    );
    match major {
        22 => (minor, patch) >= (22, 3),
        24 => (minor, patch) >= (15, 0),
        m if m >= 25 => (m, minor, patch) >= (25, 9, 0),
        _ => false,
    }
}

fn read_node_version(p: &Paths) -> Option<String> {
    let exe = node_exe(p);
    if !exe.is_file() {
        return None;
    }
    run_capture(&exe, &["--version"], &[], &p.root, Duration::from_secs(5))
        .ok()
        .filter(|x| x.0 == Some(0))
        .map(|x| x.1.trim().to_string())
        .filter(|x| !x.is_empty())
}
fn read_openclaw_version(p: &Paths) -> Option<String> {
    let text = fs::read_to_string(p.app.join("node_modules/openclaw/package.json")).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

fn integrity_ok(p: &Paths, m: &RuntimeManifest) -> bool {
    let Ok(text) = fs::read_to_string(install_file(p)) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    v.get("node_sha256").and_then(Value::as_str) == Some(m.node.windows_x64_sha256.as_str())
        && v.get("openclaw_integrity").and_then(Value::as_str)
            == Some(m.openclaw.integrity.as_str())
        && v.get("openclaw_version").and_then(Value::as_str) == Some(m.openclaw_version.as_str())
}

fn parse_profile(p: &Paths) -> Result<Option<u16>, String> {
    let path = profile_file(p);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("读取 OpenClaw2 profile 失败: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "OpenClaw2 profile 已损坏，拒绝覆盖")?;
    if v.get("profile").and_then(Value::as_str) != Some(PROFILE) {
        return Err("OpenClaw2 profile 不兼容，拒绝覆盖".into());
    }
    let port = v
        .get("port")
        .and_then(Value::as_u64)
        .ok_or("OpenClaw2 profile 缺少 port")?;
    u16::try_from(port)
        .map(Some)
        .map_err(|_| "OpenClaw2 profile 端口无效".into())
}

fn private_config_ok(p: &Paths) -> Result<bool, String> {
    let path = config_file(p);
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("读取 OpenClaw2 配置失败: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "OpenClaw2 配置已损坏，拒绝覆盖")?;
    let port = v.pointer("/gateway/port").and_then(Value::as_u64);
    let profile_port = parse_profile(p)?;
    Ok(port.is_some()
        && profile_port.is_none_or(|expected| port == Some(u64::from(expected)))
        && v.pointer("/gateway/auth/token")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .is_some()
        && v.pointer("/gateway/mode").and_then(Value::as_str) == Some("local")
        && v.pointer("/gateway/bind").and_then(Value::as_str) == Some("loopback")
        && v.pointer("/agents/defaults/workspace")
            .and_then(Value::as_str)
            == Some(p.workspace.to_string_lossy().as_ref()))
}

fn port_free(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}
fn choose_port(existing: Option<u16>) -> Result<u16, String> {
    if let Some(port) = existing {
        return Ok(port);
    }
    for port in [19789u16, 20789, 21789] {
        if port_free(port) {
            return Ok(port);
        }
    }
    Err("OpenClaw2 默认端口 19789/20789/21789 均被占用".into())
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> Result<(), String> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(h: isize, b: *mut u8, n: u32, flags: u32) -> i32;
    }
    let status = unsafe { BCryptGenRandom(0, bytes.as_mut_ptr(), bytes.len() as u32, 0x0000_0002) };
    if status == 0 {
        Ok(())
    } else {
        Err("无法生成 OpenClaw2 gateway 随机令牌".into())
    }
}
#[cfg(not(windows))]
fn fill_random(bytes: &mut [u8]) -> Result<(), String> {
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(bytes))
        .map_err(|e| format!("无法生成 OpenClaw2 gateway 随机令牌: {e}"))
}

pub fn state_version() -> String {
    let p = paths();
    let mut snapshot = String::new();
    for file in [
        profile_file(&p),
        config_file(&p),
        supervisor_file(&p),
        install_file(&p),
    ] {
        snapshot.push_str(&file.to_string_lossy());
        snapshot.push('\n');
        match fs::read(&file) {
            Ok(b) => snapshot.push_str(&crate::installer::sha256_hex_bytes(&b)),
            Err(_) => snapshot.push('-'),
        }
        snapshot.push('\n');
    }
    crate::actions::version_of(&snapshot)
}

pub fn inspect() -> Result<Value, String> {
    let p = paths();
    let m = manifest()?;
    let node_version = read_node_version(&p);
    let node_ok = node_version.as_deref().map(node_supported).unwrap_or(false);
    let openclaw_version = read_openclaw_version(&p);
    let installed = node_ok
        && openclaw_version.as_deref() == Some(m.openclaw_version.as_str())
        && cli_file(&p).is_file();
    let prepared =
        private_config_ok(&p).unwrap_or(false) && parse_profile(&p).ok().flatten().is_some();
    let (port, pid, owned) = supervisor_status(&p).unwrap_or((None, None, false));
    let running = port.map(port_listening).unwrap_or(false) && owned;
    let mut blockers = Vec::<String>::new();
    if !installed {
        blockers.push("OpenClaw2 私有 runtime 未完成安装或版本不匹配".into());
    }
    if !prepared {
        blockers.push("OpenClaw2 私有 profile 尚未准备好或配置不兼容".into());
    }
    Ok(
        json!({"schema_version":1,"ready":installed && prepared,"blockers":blockers,"installed":installed,"prepared":prepared,"running":running,"state_version":state_version(),"profile":PROFILE,"paths":{"root":p.root,"runtime":p.runtime,"state":p.state,"workspace":p.workspace,"run":p.run,"logs":p.logs},"runtime":{"node_version":node_version,"node_supported":node_ok,"openclaw_version":openclaw_version,"integrity_ok":integrity_ok(&p,&m)},"gateway":{"port":port,"pid":pid,"owned":owned}}),
    )
}

pub fn prepare(port: Option<u16>) -> Result<Value, String> {
    if let Some(p) = port {
        if p < 1024 {
            return Err("invalid_input: port 必须在 1024..65535".into());
        }
    }
    let ps = paths();
    create_layout(&ps)?;
    let existing = parse_profile(&ps)?;
    let chosen = match (existing, port) {
        (Some(a), Some(b)) if a != b => return Err("OpenClaw2 已准备为另一个端口，拒绝覆盖".into()),
        (Some(a), _) => a,
        (None, b) => choose_port(b)?,
    };
    if !port_free(chosen) {
        let (_, _, owned) = supervisor_status(&ps)?;
        if !owned {
            return Err(format!("OpenClaw2 端口 {chosen} 已被外部程序占用"));
        }
    }
    let config_ok = private_config_ok(&ps)?;
    let mut changed = false;
    if !profile_file(&ps).exists() {
        atomic_write(
            &profile_file(&ps),
            serde_json::to_string_pretty(
                &json!({"schema_version":1,"profile":PROFILE,"port":chosen}),
            )
            .unwrap()
            .as_bytes(),
            &ps,
        )?;
        changed = true;
    }
    if !config_file(&ps).exists() {
        let token = random_token()?;
        let cfg = json!({"gateway":{"mode":"local","port":chosen,"bind":"loopback","auth":{"mode":"token","token":token}},"agents":{"defaults":{"workspace":ps.workspace.to_string_lossy()}}});
        atomic_write(
            &config_file(&ps),
            serde_json::to_string_pretty(&cfg).unwrap().as_bytes(),
            &ps,
        )?;
        changed = true;
    } else if !config_ok {
        return Err("OpenClaw2 配置与私有 profile 不兼容，拒绝覆盖".into());
    }
    Ok(
        json!({"changed":changed,"prepared":true,"profile":PROFILE,"port":chosen,"state_version":state_version()}),
    )
}

pub fn install(progress: &crate::actions::ProgressSink) -> Result<Value, String> {
    let ps = paths();
    let m = manifest()?;
    if integrity_ok(&ps, &m)
        && read_node_version(&ps)
            .as_deref()
            .map(node_supported)
            .unwrap_or(false)
        && read_openclaw_version(&ps).as_deref() == Some(m.openclaw_version.as_str())
        && cli_file(&ps).is_file()
    {
        return Ok(
            json!({"changed":false,"installed":true,"node_version":read_node_version(&ps),"openclaw_version":m.openclaw_version,"integrity_ok":true,"state_version":state_version()}),
        );
    }
    create_layout(&ps)?;
    progress("下载并校验 OpenClaw2 私有 Node runtime…");
    #[cfg(not(windows))]
    {
        return Err("OpenClaw2 一期仅提供 Windows x64 私有 runtime".into());
    }
    #[cfg(windows)]
    {
        let archive = ps.run.join(format!("node-v{}-win-x64.zip", m.node.version));
        download(&m.node.windows_x64_url, &archive, Duration::from_secs(840))?;
        let bytes = fs::read(&archive).map_err(|e| format!("读取 Node 下载包失败: {e}"))?;
        if crate::installer::sha256_hex_bytes(&bytes) != m.node.windows_x64_sha256 {
            let _ = fs::remove_file(&archive);
            return Err("OpenClaw2 Node SHA-256 不匹配，已拒绝安装".into());
        }
        let extract = ps.run.join("node-extract");
        let _ = fs::remove_dir_all(&extract);
        fs::create_dir_all(&extract).map_err(|e| e.to_string())?;
        run_status(
            Command::new("tar").args([
                "-xf",
                &archive.to_string_lossy(),
                "-C",
                &extract.to_string_lossy(),
            ]),
            Duration::from_secs(120),
            "解压 OpenClaw2 Node",
        )?;
        let source = extract.join(format!("node-v{}-win-x64", m.node.version));
        if !source.join("node.exe").is_file() {
            return Err("OpenClaw2 Node 解压产物不完整".into());
        }
        let _ = fs::remove_dir_all(&ps.node);
        fs::rename(&source, &ps.node).map_err(|e| format!("整理 OpenClaw2 Node 失败: {e}"))?;
        let _ = fs::remove_file(&archive);
        let _ = fs::remove_dir_all(&extract);
    }
    let version = read_node_version(&ps).ok_or("OpenClaw2 私有 Node 无法启动")?;
    if !node_supported(&version) {
        return Err(format!("OpenClaw2 私有 Node 版本不受支持: {version}"));
    }
    progress("安装已固定版本的 OpenClaw2 私有 package…");
    let npm = npm_exe(&ps);
    if !npm.is_file() {
        return Err("OpenClaw2 私有 npm 不存在".into());
    }
    let _ = fs::remove_dir_all(ps.app.join("node_modules"));
    let spec = format!("openclaw@{}", m.openclaw_version);
    let output = run_capture(
        &npm,
        &[
            "install",
            "--prefix",
            &ps.app.to_string_lossy(),
            "--package-lock=false",
            "--no-save",
            "--no-fund",
            "--no-audit",
            "--ignore-scripts",
            "--registry=https://registry.npmjs.org",
            &spec,
        ],
        &[],
        &ps.root,
        Duration::from_secs(900),
    )?;
    if output.0.is_none() {
        return Err("OpenClaw2 npm 安装超时".into());
    }
    if output.0 != Some(0) {
        return Err(format!(
            "OpenClaw2 npm 安装失败: {}",
            redact_tail(&output.1)
        ));
    }
    if read_openclaw_version(&ps).as_deref() != Some(m.openclaw_version.as_str())
        || !cli_file(&ps).is_file()
    {
        return Err("OpenClaw2 私有 package 版本或入口校验失败".into());
    }
    let marker = json!({"schema_version":1,"node_sha256":m.node.windows_x64_sha256,"openclaw_version":m.openclaw_version,"openclaw_integrity":m.openclaw.integrity,"tarball_url":m.openclaw.tarball_url});
    atomic_write(
        &install_file(&ps),
        serde_json::to_string_pretty(&marker).unwrap().as_bytes(),
        &ps,
    )?;
    Ok(
        json!({"changed":true,"installed":true,"node_version":version,"openclaw_version":m.openclaw_version,"integrity_ok":true,"state_version":state_version()}),
    )
}

pub fn preflight() -> Result<Value, String> {
    let ps = paths();
    let report = inspect()?;
    let mut blockers = report
        .get("blockers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut warnings = Vec::<String>::new();
    let mut doctor = json!({"ran":false,"ok":false});
    if report.get("installed").and_then(Value::as_bool) == Some(true)
        && report.get("prepared").and_then(Value::as_bool) == Some(true)
    {
        let out = run_oc(
            &ps,
            &["doctor", "--lint", "--json"],
            Duration::from_secs(60),
        )?;
        doctor = parse_doctor(&out.1, out.0);
        if doctor.get("ok").and_then(Value::as_bool) != Some(true) {
            warnings.push("OpenClaw2 doctor --lint 未通过；未执行 fix".into());
        }
        if let Some(port) = parse_profile(&ps)? {
            if port_listening(port) {
                let g = gateway_status(&ps, port)?;
                if !g.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    blockers.push(Value::String("OpenClaw2 Gateway RPC health 未通过".into()));
                }
            }
        }
    }
    let ready = blockers.is_empty() && doctor.get("ok").and_then(Value::as_bool).unwrap_or(false);
    Ok(
        json!({"ok":ready,"ready":ready,"blockers":blockers,"warnings":warnings,"runtime":report["runtime"],"config":{"private":report["prepared"],"profile":PROFILE},"doctor":doctor,"gateway":report["gateway"]}),
    )
}

pub fn launch() -> Result<Value, String> {
    let ps = paths();
    let status = inspect()?;
    if status.get("installed").and_then(Value::as_bool) != Some(true) {
        return Err("OpenClaw2 未安装或 runtime 校验未通过".into());
    }
    if status.get("prepared").and_then(Value::as_bool) != Some(true) {
        return Err("OpenClaw2 尚未准备私有 profile".into());
    }
    let port = parse_profile(&ps)?.ok_or("OpenClaw2 profile 缺少端口")?;
    if port_listening(port) {
        let (_, _, owned) = supervisor_status(&ps)?;
        if owned {
            let h = gateway_status(&ps, port)?;
            return Ok(
                json!({"changed":false,"running":true,"ready":h["ok"],"pid":supervisor_status(&ps)?.1,"port":port,"dashboard_url":format!("http://127.0.0.1:{port}/"),"health":h,"state_version":state_version()}),
            );
        }
        return Err(format!("OpenClaw2 端口 {port} 被外部进程占用，拒绝接管"));
    }
    let node = node_exe(&ps);
    let cli = cli_file(&ps);
    let mut cmd = Command::new(node);
    cmd.args([
        cli.to_string_lossy().as_ref(),
        "--profile",
        PROFILE,
        "gateway",
        "run",
        "--port",
        &port.to_string(),
    ])
    .current_dir(&ps.workspace)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .env("OPENCLAW_PROFILE", PROFILE)
    .env("OPENCLAW_CONFIG_PATH", config_file(&ps))
    .env("OPENCLAW_STATE_DIR", &ps.state)
    .env("OPENCLAW_AGENT_DIR", ps.state.join("agents"))
    .env("SUPERVISOR_MODE", "external")
    .env("SERVICE_REPAIR_POLICY", "external")
    .env("DISABLE_BONJOUR", "1")
    .env("NO_COLOR", "1")
    .env_remove("OPENCLAW_HOME");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("启动 OpenClaw2 Gateway 失败: {e}"))?;
    atomic_write(&supervisor_file(&ps), serde_json::to_string_pretty(&json!({"schema_version":1,"profile":PROFILE,"pid":child.id(),"port":port,"started_at":now_nanos()})).unwrap().as_bytes(), &ps)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut health = json!({"ok":false,"degraded":false,"rpcOk":false});
    while Instant::now() < deadline {
        if port_listening(port) {
            health = gateway_status(&ps, port)
                .unwrap_or_else(|_| json!({"ok":false,"degraded":false,"rpcOk":false}));
            if health.get("ok").and_then(Value::as_bool) == Some(true) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    let ready = health.get("ok").and_then(Value::as_bool) == Some(true)
        && health.get("degraded").and_then(Value::as_bool) != Some(true)
        && health.get("rpcOk").and_then(Value::as_bool) == Some(true);
    Ok(
        json!({"changed":true,"running":port_listening(port),"ready":ready,"pid":child.id(),"port":port,"dashboard_url":format!("http://127.0.0.1:{port}/"),"health":health,"state_version":state_version()}),
    )
}

fn supervisor_status(p: &Paths) -> Result<(Option<u16>, Option<u32>, bool), String> {
    let text = match fs::read_to_string(supervisor_file(p)) {
        Ok(x) => x,
        Err(_) => return Ok((parse_profile(p)?, None, false)),
    };
    let v: Value = serde_json::from_str(&text).map_err(|_| "OpenClaw2 supervisor 状态已损坏")?;
    let port = v
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|x| u16::try_from(x).ok());
    let pid = v
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|x| u32::try_from(x).ok());
    Ok((
        port,
        pid,
        v.get("profile").and_then(Value::as_str) == Some(PROFILE)
            && port == parse_profile(p)?
            && pid.is_some_and(pid_is_alive),
    ))
}

#[cfg(windows)]
fn pid_is_alive(pid: u32) -> bool {
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const STILL_ACTIVE: u32 = 259;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> isize;
        fn GetExitCodeProcess(process: isize, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | SYNCHRONIZE, 0, pid) };
    if process == 0 {
        return false;
    }
    let mut code = 0;
    let alive = unsafe { GetExitCodeProcess(process, &mut code) } != 0 && code == STILL_ACTIVE;
    unsafe { CloseHandle(process) };
    alive
}

#[cfg(not(windows))]
fn pid_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}
fn port_listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(250),
    )
    .is_ok()
}
fn run_oc(p: &Paths, args: &[&str], timeout: Duration) -> Result<(Option<i32>, String), String> {
    let node = node_exe(p);
    let cli = cli_file(p);
    let mut all = vec![
        cli.to_string_lossy().to_string(),
        "--profile".into(),
        PROFILE.into(),
    ];
    all.extend(args.iter().map(|x| (*x).into()));
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    run_capture(
        &node,
        &refs,
        &[
            ("OPENCLAW_PROFILE", PROFILE),
            (
                "OPENCLAW_CONFIG_PATH",
                config_file(p).to_string_lossy().as_ref(),
            ),
            ("OPENCLAW_STATE_DIR", p.state.to_string_lossy().as_ref()),
            (
                "OPENCLAW_AGENT_DIR",
                p.state.join("agents").to_string_lossy().as_ref(),
            ),
            ("SUPERVISOR_MODE", "external"),
            ("SERVICE_REPAIR_POLICY", "external"),
            ("DISABLE_BONJOUR", "1"),
            ("NO_COLOR", "1"),
        ],
        &p.workspace,
        timeout,
    )
}
fn gateway_status(p: &Paths, port: u16) -> Result<Value, String> {
    let out = run_oc(
        p,
        &[
            "gateway",
            "status",
            "--port",
            &port.to_string(),
            "--require-rpc",
            "--json",
        ],
        Duration::from_secs(10),
    )?;
    let mut v: Value = serde_json::from_str(&out.1).unwrap_or_else(|_| json!({}));
    let rpc = v
        .get("rpcOk")
        .and_then(Value::as_bool)
        .or_else(|| v.pointer("/rpc/ok").and_then(Value::as_bool))
        .unwrap_or(false);
    let degraded = v.get("degraded").and_then(Value::as_bool).unwrap_or(false);
    let ok = out.0 == Some(0) && rpc && !degraded;
    v["rpcOk"] = json!(rpc);
    v["degraded"] = json!(degraded);
    v["ok"] = json!(ok);
    Ok(v)
}
fn parse_doctor(text: &str, code: Option<i32>) -> Value {
    let mut v: Value = serde_json::from_str(text).unwrap_or_else(|_| json!({"parse_error":true}));
    let ok = code == Some(0)
        && !v
            .get("parse_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    v["ok"] = json!(ok);
    v["mode"] = json!("lint");
    v
}
fn run_capture(
    exe: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
    timeout: Duration,
) -> Result<(Option<i32>, String), String> {
    let mut c = Command::new(exe);
    c.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("OPENCLAW_HOME");
    for (k, v) in env {
        c.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let mut child = c
        .spawn()
        .map_err(|e| format!("启动 OpenClaw2 子进程失败: {e}"))?;
    let begin = Instant::now();
    loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            return Ok((s.code(), text));
        }
        if begin.elapsed() >= timeout {
            let _ = child.kill();
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            return Ok((None, String::from_utf8_lossy(&out.stdout).to_string()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn run_status(c: &mut Command, timeout: Duration, what: &str) -> Result<(), String> {
    let exe = c.get_program().to_string_lossy().to_string();
    let args: Vec<String> = c
        .get_args()
        .map(|x| x.to_string_lossy().to_string())
        .collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_capture(Path::new(&exe), &refs, &[], &std::env::temp_dir(), timeout)?;
    if out.0 == Some(0) {
        Ok(())
    } else {
        Err(format!("{what}失败: {}", redact_tail(&out.1)))
    }
}
fn download(url: &str, out: &Path, timeout: Duration) -> Result<(), String> {
    let mut c = Command::new(crate::installer::system_tool("curl"));
    c.args([
        "-fL",
        "--proxy",
        "",
        "--connect-timeout",
        "20",
        "--max-time",
        &timeout.as_secs().to_string(),
        "-o",
        &out.to_string_lossy(),
        url,
    ]);
    run_status(&mut c, timeout, "下载 OpenClaw2 runtime")
}
fn redact_tail(s: &str) -> String {
    s.lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(400)
        .collect()
}

pub fn action_inspect(
    _: &str,
    _: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    inspect()
}
pub fn action_install(
    _: &str,
    _: Value,
    log: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    install(log)
}
pub fn action_prepare(
    _: &str,
    input: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    prepare(
        input
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|x| u16::try_from(x).ok()),
    )
}
pub fn action_preflight(
    _: &str,
    _: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    preflight()
}
pub fn action_launch(_: &str, _: Value, _: &crate::actions::ProgressSink) -> Result<Value, String> {
    launch()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn node_ranges_are_exact() {
        for v in ["v22.22.2", "v23.0.0", "v24.14.9", "v25.8.9"] {
            assert!(!node_supported(v), "{v}");
        }
        for v in ["v22.22.3", "v24.15.0", "v25.9.0", "v26.0.0"] {
            assert!(node_supported(v), "{v}");
        }
    }
    #[test]
    fn private_paths_do_not_overlap_legacy_homes() {
        let p = paths();
        for x in [
            PathBuf::from(".openclaw"),
            crate::installer::uking_home().join("openclaw"),
            PathBuf::from("ClawX"),
        ] {
            assert_ne!(p.root, x);
            assert!(!p.root.ends_with("openclaw"));
        }
    }
    #[test]
    fn derived_private_tree_has_no_legacy_descendant() {
        let p = paths_from_root(std::env::temp_dir().join("uking-openclaw2-test-private"));
        for child in [
            &p.runtime,
            &p.node,
            &p.app,
            &p.state,
            &p.workspace,
            &p.run,
            &p.logs,
        ] {
            assert!(child.starts_with(&p.root));
            assert!(!child.to_string_lossy().contains("ClawX"));
            assert!(!child.to_string_lossy().contains(".openclaw"));
        }
    }
    #[test]
    fn prepare_is_idempotent_and_never_returns_the_gateway_token() {
        let sb = crate::testsandbox::enter_raw("openclaw2-prepare");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::remove_var("HOME");
        let first = prepare(None).expect("首次准备私有 profile");
        assert_eq!(first["changed"], true);
        let wire = serde_json::to_string(&first).unwrap();
        let ps = paths();
        let config = fs::read_to_string(config_file(&ps)).unwrap();
        let token = serde_json::from_str::<Value>(&config)
            .unwrap()
            .pointer("/gateway/auth/token")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        assert!(!wire.contains(&token), "Action 输出不得泄漏 gateway token");
        let second = prepare(None).expect("重放应成功");
        assert_eq!(second["changed"], false);
        assert_eq!(first["port"], second["port"], "端口必须持久化重放");
        assert!(ps.root.starts_with(sb.root()));
        assert!(!sb.root().join(".openclaw").exists());
        assert!(!sb.root().join("AppData/ClawX").exists());
    }
    #[test]
    fn occupied_requested_port_is_refused_before_any_private_config_write() {
        let sb = crate::testsandbox::enter_raw("openclaw2-port-conflict");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::remove_var("HOME");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(prepare(Some(port)).unwrap_err().contains("占用"));
        assert!(!profile_file(&paths()).exists());
    }
    #[test]
    fn malformed_private_config_is_refused_not_replaced() {
        let sb = crate::testsandbox::enter_raw("openclaw2-bad-config");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::remove_var("HOME");
        let _ = prepare(None).unwrap();
        let ps = paths();
        fs::write(config_file(&ps), b"{ definitely-not-json").unwrap();
        assert!(prepare(None).unwrap_err().contains("损坏"));
        assert_eq!(
            fs::read_to_string(config_file(&ps)).unwrap(),
            "{ definitely-not-json"
        );
    }
    #[test]
    fn action_contract_has_confirmation_unknown_field_and_conflict_guards() {
        let listed = crate::actions::list();
        for id in [
            crate::actions::OPENCLAW2_INSPECT,
            crate::actions::OPENCLAW2_INSTALL,
            crate::actions::OPENCLAW2_PREPARE,
            crate::actions::OPENCLAW2_PREFLIGHT,
            crate::actions::OPENCLAW2_LAUNCH,
        ] {
            assert!(listed.iter().any(|a| a.id == id), "{id} 未注册");
        }
        let unknown = crate::actions::run(
            crate::actions::OPENCLAW2_PREPARE,
            json!({"confirm":true,"unknown":1}),
        )
        .unwrap_err();
        assert!(unknown.contains("未知字段"));
        let conflict = crate::actions::run(
            crate::actions::OPENCLAW2_PREPARE,
            json!({"confirm":true,"expected_state_version":"v1-stale"}),
        )
        .unwrap_err();
        assert!(conflict.contains("conflict:"));
    }
}
