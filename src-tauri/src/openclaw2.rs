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
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROFILE: &str = "uking-openclaw2";
const CONFIG_NAME: &str = "openclaw.json";
const PROFILE_NAME: &str = "profile.json";
const SUPERVISOR_NAME: &str = "supervisor.json";
const INSTALL_NAME: &str = "installed.json";
const MODEL_MARKER_NAME: &str = "model-config.json";
const MODEL_SECRET_PROVIDER: &str = "uking-openclaw2-file";
/// OpenClaw reserves the Gateway itself, browser-control (`base + 2`), then
/// the managed Chromium CDP family (`base + 11` through `base + 110`). Keep
/// a whole family exclusive: choosing only a free gateway port is insufficient.
const CDP_PORT_START_OFFSET: u16 = 11;
const CDP_PORT_END_OFFSET: u16 = 110;

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

/// Sensitive route material resolved by the composition root. It deliberately
/// has no Serialize/Debug implementation: the API key cannot accidentally
/// cross an Action response, progress event, marker, or diagnostic.
pub struct ModelRoute {
    pub source_id: String,
    pub source_name: String,
    pub base: String,
    pub model: String,
    pub key: String,
    pub key_source: String,
}

fn model_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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
fn model_marker_file(p: &Paths) -> PathBuf { p.state.join(MODEL_MARKER_NAME) }
fn model_secrets_dir(p: &Paths) -> PathBuf { p.state.join("secrets") }
fn model_txn_root(p: &Paths) -> PathBuf { p.run.join("model-config-txn") }
fn node_archive_file(p: &Paths, m: &RuntimeManifest) -> PathBuf {
    p.runtime.join(format!("node-v{}-win-x64.zip", m.node.version))
}
fn openclaw_archive_file(p: &Paths, m: &RuntimeManifest) -> PathBuf {
    p.runtime.join(format!("openclaw-{}.tgz", m.openclaw_version))
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
        .filter(|x| x.status == Some(0))
        .map(|x| x.stdout.trim().to_string())
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
    // `installed.json` is diagnostic only. Readiness is based on the actual
    // pinned archives plus the installed package entry, so a forged marker
    // cannot make inspect claim a tampered runtime is ready.
    verify_sha256_file(&node_archive_file(p, m), &m.node.windows_x64_sha256).is_ok()
        && verify_npm_integrity_file(&openclaw_archive_file(p, m), &m.openclaw.integrity).is_ok()
        && read_openclaw_version(p).as_deref() == Some(m.openclaw_version.as_str())
        && cli_file(p).is_file()
}

fn verify_sha256_file(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("读取 OpenClaw2 校验文件失败: {e}"))?;
    if crate::installer::sha256_hex_bytes(&bytes).eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("OpenClaw2 Node SHA-256 不匹配".into())
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let compact: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if compact.is_empty() || compact.len() % 4 != 0 {
        return Err("npm integrity Base64 无效".into());
    }
    let mut out = Vec::with_capacity(compact.len() / 4 * 3);
    for chunk in compact.chunks_exact(4) {
        let a = value(chunk[0]).ok_or("npm integrity Base64 无效")?;
        let b = value(chunk[1]).ok_or("npm integrity Base64 无效")?;
        let c = if chunk[2] == b'=' { 0 } else { value(chunk[2]).ok_or("npm integrity Base64 无效")? };
        let d = if chunk[3] == b'=' { 0 } else { value(chunk[3]).ok_or("npm integrity Base64 无效")? };
        if chunk[2] == b'=' && chunk[3] != b'=' {
            return Err("npm integrity Base64 padding 无效".into());
        }
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' { out.push((b << 4) | (c >> 2)); }
        if chunk[3] != b'=' { out.push((c << 6) | d); }
    }
    Ok(out)
}

#[cfg(windows)]
fn verify_npm_integrity_file(path: &Path, integrity: &str) -> Result<(), String> {
    let expected = integrity
        .strip_prefix("sha512-")
        .ok_or_else(|| "OpenClaw2 仅接受 sha512 npm integrity".to_string())
        .and_then(base64_decode)?;
    if expected.len() != 64 {
        return Err("OpenClaw2 npm integrity 不是 SHA-512 摘要".into());
    }
    let output = run_capture(
        Path::new("certutil.exe"),
        &["-hashfile", &path.to_string_lossy(), "SHA512"],
        &[],
        &std::env::temp_dir(),
        Duration::from_secs(15),
    )?;
    if output.status != Some(0) {
        return Err(format!("OpenClaw2 SHA-512 校验失败: {}", redact_tail(&output.stderr)));
    }
    let actual = output.stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        let hex: String = trimmed.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        (hex.len() == 128 && trimmed.chars().all(|c| c.is_ascii_hexdigit() || c.is_ascii_whitespace())).then_some(hex)
    }).ok_or("OpenClaw2 SHA-512 工具没有返回摘要")?;
    let mut mismatch = 0u8;
    for (byte, pair) in expected.iter().zip(actual.as_bytes().chunks_exact(2)) {
        let parsed = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| "SHA-512 输出无效")?, 16)
            .map_err(|_| "SHA-512 输出无效")?;
        mismatch |= byte ^ parsed;
    }
    if mismatch == 0 { Ok(()) } else { Err("OpenClaw2 npm tarball SHA-512/integrity 不匹配".into()) }
}

#[cfg(not(windows))]
fn verify_npm_integrity_file(_: &Path, _: &str) -> Result<(), String> {
    Err("OpenClaw2 一期仅提供 Windows x64 私有 runtime".into())
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

fn port_family(base: u16) -> Result<Vec<u16>, String> {
    let end = base
        .checked_add(CDP_PORT_END_OFFSET)
        .ok_or("OpenClaw2 端口过高，无法保留派生浏览器端口")?;
    let mut ports = vec![base, base + 2];
    ports.extend((base + CDP_PORT_START_OFFSET)..=end);
    Ok(ports)
}

/// Bind every member at once so one candidate cannot pass merely because its
/// gateway is free while OpenClaw's derived browser-control/CDP ports collide.
fn reserve_port_family(base: u16) -> Result<Vec<TcpListener>, String> {
    let mut held = Vec::new();
    for port in port_family(base)? {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => held.push(listener),
            Err(_) => {
                return Err(format!("OpenClaw2 端口族 {base}（含派生端口 {port}）已被占用"));
            }
        }
    }
    Ok(held)
}

fn port_family_free(base: u16) -> bool {
    reserve_port_family(base).is_ok()
}

fn choose_port(existing: Option<u16>) -> Result<u16, String> {
    if let Some(port) = existing {
        return Ok(port);
    }
    for port in [19789u16, 20789, 21789] {
        if port_family_free(port) {
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
        model_marker_file(&p),
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
    let model = fs::read_to_string(model_marker_file(&p))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|marker| json!({"configured":true,"provider_id":marker["source_provider"],"provider_key":marker["provider_key"],"model":marker["model"],"probe":marker["probe"]}))
        .unwrap_or_else(|| json!({"configured":false}));
    let mut blockers = Vec::<String>::new();
    if !installed {
        blockers.push("OpenClaw2 私有 runtime 未完成安装或版本不匹配".into());
    }
    if !prepared {
        blockers.push("OpenClaw2 私有 profile 尚未准备好或配置不兼容".into());
    }
    Ok(
        json!({"schema_version":1,"ready":installed && prepared,"blockers":blockers,"installed":installed,"prepared":prepared,"running":running,"state_version":state_version(),"profile":PROFILE,"paths":{"root":p.root,"runtime":p.runtime,"state":p.state,"workspace":p.workspace,"run":p.run,"logs":p.logs},"runtime":{"node_version":node_version,"node_supported":node_ok,"openclaw_version":openclaw_version,"integrity_ok":integrity_ok(&p,&m)},"gateway":{"port":port,"pid":pid,"owned":owned},"model":model}),
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
    let config_ok = private_config_ok(&ps)?;
    // A complete private profile is a replay, including while its own gateway
    // holds the family. Do not turn that benign replay into a destructive port
    // probe or a config rewrite.
    if existing.is_some() && config_ok {
        return Ok(json!({"changed":false,"prepared":true,"profile":PROFILE,"port":chosen,"state_version":state_version()}));
    }
    if existing.is_none() && config_file(&ps).exists() {
        return Err("OpenClaw2 发现没有 profile 的已有私有配置，拒绝覆盖".into());
    }
    // Keep every listener reserved through the two atomic writes below. A
    // gateway-only probe would miss browser-control/CDP collisions.
    let _ports = reserve_port_family(chosen)?;
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
        let archive = node_archive_file(&ps, &m);
        download(&m.node.windows_x64_url, &archive, Duration::from_secs(840))?;
        if verify_sha256_file(&archive, &m.node.windows_x64_sha256).is_err() {
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
    let tarball = openclaw_archive_file(&ps, &m);
    download(&m.openclaw.tarball_url, &tarball, Duration::from_secs(840))?;
    verify_npm_integrity_file(&tarball, &m.openclaw.integrity)?;
    let _ = fs::remove_dir_all(ps.app.join("node_modules"));
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
            &tarball.to_string_lossy(),
        ],
        &[],
        &ps.root,
        Duration::from_secs(900),
    )?;
    if output.status.is_none() {
        return Err("OpenClaw2 npm 安装超时".into());
    }
    if output.status != Some(0) {
        return Err(format!(
            "OpenClaw2 npm 安装失败: {}",
            redact_tail(if output.stderr.is_empty() { &output.stdout } else { &output.stderr })
        ));
    }
    if !integrity_ok(&ps, &m) {
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

fn normalized_model_base(base: &str) -> Result<String, String> {
    let base = base.trim();
    if base.is_empty() || base.len() > 2048 || base.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err("invalid_input: OpenClaw2 endpoint 无效".into());
    }
    let (scheme, rest) = base.split_once("://").ok_or("invalid_input: OpenClaw2 endpoint 必须为绝对 URL")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return Err("invalid_input: OpenClaw2 endpoint 仅支持 HTTP(S)".into());
    }
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || rest.contains('?') || rest.contains('#') {
        return Err("invalid_input: OpenClaw2 endpoint 不允许认证、query 或 fragment".into());
    }
    if scheme.eq_ignore_ascii_case("http") {
        let host = authority.split(':').next().unwrap_or(authority).trim_matches(['[', ']']);
        if !matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1") {
            return Err("invalid_input: OpenClaw2 明文 HTTP 仅允许 loopback".into());
        }
    }
    Ok(base.trim_end_matches('/').to_string())
}

fn model_provider_key(source_id: &str, base: &str) -> String {
    let digest = crate::installer::sha256_hex_bytes(format!("{source_id}\n{base}").as_bytes());
    format!("uking-oc2-{}", &digest[..12])
}

fn model_ref(provider_key: &str, model: &str) -> String { format!("{provider_key}/{model}") }

fn model_secret_file(p: &Paths, nonce: &str) -> PathBuf {
    model_secrets_dir(p).join(format!("model-{nonce}.json"))
}

fn model_marker_matches(p: &Paths, route: &ModelRoute, provider_key: &str) -> Option<Value> {
    let marker: Value = serde_json::from_str(&fs::read_to_string(model_marker_file(p)).ok()?).ok()?;
    if marker.get("owner").and_then(Value::as_str) != Some(PROFILE)
        || marker.get("source_provider").and_then(Value::as_str) != Some(route.source_id.as_str())
        || marker.get("provider_key").and_then(Value::as_str) != Some(provider_key)
        || marker.get("model").and_then(Value::as_str) != Some(route.model.as_str()) {
        return None;
    }
    let secret_name = marker.get("secret_basename").and_then(Value::as_str)?;
    if secret_name.contains(['/', '\\']) || !secret_name.starts_with("model-") { return None; }
    let secret: Value = serde_json::from_str(&fs::read_to_string(model_secrets_dir(p).join(secret_name)).ok()?).ok()?;
    (secret.get("api_key").and_then(Value::as_str) == Some(route.key.as_str())).then_some(marker)
}

fn model_marker_owns_provider(p: &Paths, provider_key: &str) -> bool {
    fs::read_to_string(model_marker_file(p))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|marker| {
            marker.get("owner").and_then(Value::as_str) == Some(PROFILE)
                && marker.get("provider_key").and_then(Value::as_str) == Some(provider_key)
        })
}

fn model_candidate_config(p: &Paths, route: &ModelRoute, provider_key: &str, secret_file: &Path) -> Result<Vec<u8>, String> {
    let mut config: Value = serde_json::from_slice(&fs::read(config_file(p)).map_err(|_| "not_ready: OpenClaw2 私有配置不可读")?)
        .map_err(|_| "not_ready: OpenClaw2 私有配置已损坏")?;
    let root = config.as_object_mut().ok_or("not_ready: OpenClaw2 私有配置形状无效")?;
    let models = root.entry("models").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 models 形状无效")?;
    if !models.contains_key("mode") { models.insert("mode".into(), json!("merge")); }
    let providers = models.entry("providers").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 models.providers 形状无效")?;
    if providers.contains_key(provider_key) && !model_marker_owns_provider(p, provider_key) {
        return Err("validation_failed: OpenClaw2 同名 model provider 不属于本适配器，拒绝覆盖".into());
    }
    providers.insert(provider_key.into(), json!({"baseUrl":route.base,"api":"openai-completions","apiKey":{"source":"file","provider":MODEL_SECRET_PROVIDER,"id":"api_key"},"models":[{"id":route.model,"name":route.model}]}));
    models.insert("primary".into(), json!(model_ref(provider_key, &route.model)));
    let agents = root.entry("agents").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 agents 形状无效")?;
    let defaults = agents.entry("defaults").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 agents.defaults 形状无效")?;
    let default_models = defaults.entry("models").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 agents.defaults.models 形状无效")?;
    default_models.insert(model_ref(provider_key, &route.model), json!({}));
    let secrets = root.entry("secrets").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 secrets 形状无效")?;
    let secret_providers = secrets.entry("providers").or_insert_with(|| json!({})).as_object_mut().ok_or("not_ready: OpenClaw2 secrets.providers 形状无效")?;
    if secret_providers.contains_key(MODEL_SECRET_PROVIDER) && !model_marker_owns_provider(p, provider_key) {
        return Err("validation_failed: OpenClaw2 file secret provider 不属于本适配器，拒绝覆盖".into());
    }
    secret_providers.insert(MODEL_SECRET_PROVIDER.into(), json!({"source":"file","path":secret_file,"mode":"json"}));
    serde_json::to_vec_pretty(&config).map_err(|_| "validation_failed: 无法序列化 OpenClaw2 model 配置".into())
}

fn run_oc_transaction(p: &Paths, candidate: &Path, txn_state: &Path, args: &[&str], timeout: Duration) -> Result<Capture, String> {
    let node = node_exe(p);
    let cli = cli_file(p);
    let mut all = vec![cli.to_string_lossy().to_string(), "--profile".into(), PROFILE.into()];
    all.extend(args.iter().map(|arg| (*arg).into()));
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    let mut env = managed_env(p);
    env.retain(|(key, _)| key != "OPENCLAW_CONFIG_PATH" && key != "OPENCLAW_STATE_DIR" && key != "OPENCLAW_AGENT_DIR");
    env.push(("OPENCLAW_CONFIG_PATH".into(), candidate.to_string_lossy().to_string()));
    env.push(("OPENCLAW_STATE_DIR".into(), txn_state.to_string_lossy().to_string()));
    env.push(("OPENCLAW_AGENT_DIR".into(), txn_state.join("agents").to_string_lossy().to_string()));
    let refs_env: Vec<(&str, &str)> = env.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    run_capture(&node, &refs, &refs_env, &p.workspace, timeout)
}

fn rollback_model_config(p: &Paths, old_config: &[u8], old_marker: Option<&[u8]>, new_secret: &Path, txn: &Path) -> Result<(), String> {
    atomic_write(&config_file(p), old_config, p)?;
    match old_marker {
        Some(bytes) => atomic_write(&model_marker_file(p), bytes, p)?,
        None if model_marker_file(p).exists() => fs::remove_file(model_marker_file(p)).map_err(|_| "无法移除失败的 model marker")?,
        None => {}
    }
    if new_secret.exists() { fs::remove_file(new_secret).map_err(|_| "无法清理失败的 model secret")?; }
    if txn.exists() { fs::remove_dir_all(txn).map_err(|_| "无法清理失败的 model transaction")?; }
    Ok(())
}

pub fn configure_model(route: ModelRoute) -> Result<Value, String> {
    let _guard = model_mutex().lock().map_err(|_| "not_ready: OpenClaw2 model 配置锁不可用")?;
    let p = paths();
    let report = inspect()?;
    if report.get("installed").and_then(Value::as_bool) != Some(true)
        || report.get("runtime").and_then(|x| x.get("integrity_ok")).and_then(Value::as_bool) != Some(true)
        || report.get("prepared").and_then(Value::as_bool) != Some(true) {
        return Err("not_ready: OpenClaw2 私有 runtime 尚未安装、校验或准备完成".into());
    }
    let base = normalized_model_base(&route.base)?;
    if route.model.trim().is_empty() || route.model.len() > 256 || route.model.bytes().any(|b| b <= b' ') {
        return Err("invalid_input: OpenClaw2 model 无效".into());
    }
    if route.key.trim().is_empty() { return Err("invalid_input: OpenClaw2 API Key 不可为空".into()); }
    let route = ModelRoute { base, model: route.model.trim().into(), ..route };
    let provider_key = model_provider_key(&route.source_id, &route.base);
    if let Some(marker) = model_marker_matches(&p, &route, &provider_key) {
        return Ok(json!({"changed":false,"configured":true,"ready":true,"provider":{"id":route.source_id,"name":route.source_name,"key_source":route.key_source},"model":{"id":route.model,"ref":model_ref(&provider_key,&route.model)},"validation":{"ran":false,"ok":true},"probe":marker.get("probe").cloned().unwrap_or_else(|| json!({"ran":false,"ok":false})),"restart_required":report["running"],"state_version":state_version()}));
    }
    let nonce = random_token()?;
    let txn = model_txn_root(&p).join(&nonce);
    let candidate = txn.join("openclaw.json");
    let txn_state = txn.join("state");
    ensure_private_path(&txn, &p)?;
    fs::create_dir_all(&txn_state).map_err(|_| "validation_failed: 无法创建 OpenClaw2 model transaction")?;
    let old_config = fs::read(config_file(&p)).map_err(|_| "not_ready: OpenClaw2 私有配置不可读")?;
    let old_marker = fs::read(model_marker_file(&p)).ok();
    let secret = model_secret_file(&p, &nonce);
    let result = (|| -> Result<Value, String> {
        atomic_write(&secret, serde_json::to_vec(&json!({"api_key":route.key})).unwrap().as_slice(), &p)?;
        let candidate_bytes = model_candidate_config(&p, &route, &provider_key, &secret)?;
        atomic_write(&candidate, &candidate_bytes, &p)?;
        for args in [["config", "validate", "--json"].as_slice(), ["infer", "model", "run", "--help"].as_slice()] {
            let out = run_oc_transaction(&p, &candidate, &txn_state, args, Duration::from_secs(20))?;
            if out.status != Some(0) { return Err("validation_failed: OpenClaw2 缺少受支持的配置校验或模型推理能力".into()); }
        }
        let validation = run_oc_transaction(&p, &candidate, &txn_state, &["config", "validate", "--json"], Duration::from_secs(30))?;
        if validation.status != Some(0) || serde_json::from_str::<Value>(&validation.stdout).is_err() { return Err("validation_failed: OpenClaw2 candidate 配置校验失败".into()); }
        let began = Instant::now();
        let reference = model_ref(&provider_key, &route.model);
        let probe = run_oc_transaction(&p, &candidate, &txn_state, &["infer", "model", "run", "--local", "--model", &reference, "--prompt", "Reply exactly: openclaw2-probe-ok", "--json"], Duration::from_secs(90))?;
        if probe.status != Some(0) || serde_json::from_str::<Value>(&probe.stdout).is_err() || !probe.stdout.contains("openclaw2-probe-ok") { return Err("probe_failed: OpenClaw2 最窄模型探针失败".into()); }
        atomic_write(&config_file(&p), &candidate_bytes, &p)?;
        if fs::read(config_file(&p)).map_err(|_| "rollback_failed: OpenClaw2 live 配置回读失败")? != candidate_bytes { return Err("rollback_failed: OpenClaw2 live 配置回读不一致".into()); }
        let probe_view = json!({"ran":true,"ok":true,"latency_ms":began.elapsed().as_millis() as u64});
        let marker = json!({"schema_version":1,"owner":PROFILE,"source_provider":route.source_id,"provider_key":provider_key,"model":route.model,"secret_basename":secret.file_name().and_then(|x| x.to_str()).unwrap_or(""),"config_hash":crate::installer::sha256_hex_bytes(&candidate_bytes),"probe":probe_view});
        atomic_write(&model_marker_file(&p), serde_json::to_vec_pretty(&marker).unwrap().as_slice(), &p)?;
        if let Some(old) = old_marker.as_ref().and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok()).and_then(|m| m.get("secret_basename").and_then(Value::as_str).map(str::to_owned)) {
            let old = model_secrets_dir(&p).join(old);
            if old != secret && old.file_name().and_then(|x| x.to_str()).is_some_and(|x| x.starts_with("model-")) { let _ = fs::remove_file(old); }
        }
        let _ = fs::remove_dir_all(&txn);
        Ok(json!({"changed":true,"configured":true,"ready":true,"provider":{"id":route.source_id,"name":route.source_name,"key_source":route.key_source},"model":{"id":route.model,"ref":reference},"validation":{"ran":true,"ok":true},"probe":probe_view,"restart_required":report["running"],"state_version":state_version()}))
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => match rollback_model_config(&p, &old_config, old_marker.as_deref(), &secret, &txn) {
            Ok(()) => Err(error),
            Err(_) => Err("rollback_failed: OpenClaw2 model 配置失败且回滚未完成".into()),
        },
    }
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
    let runtime_ready = report.get("installed").and_then(Value::as_bool) == Some(true)
        && report.get("prepared").and_then(Value::as_bool) == Some(true);
    let (profile_port, profile_pid, profile_owned) = supervisor_status(&ps)?;
    // This is deliberately a fresh status snapshot, rather than the inspect
    // result. A caller can distinguish "not checked because runtime is not
    // ready" from "checked and gateway is not running".
    let mut gateway = json!({"checked":false,"running":false,"port":profile_port,"pid":profile_pid,"owned":profile_owned,"status":Value::Null});
    if runtime_ready {
        let out = run_oc(
            &ps,
            &["doctor", "--lint", "--json"],
            Duration::from_secs(60),
        )?;
        doctor = parse_doctor(&out.stdout, out.status);
        if doctor.get("ok").and_then(Value::as_bool) != Some(true) {
            warnings.push("OpenClaw2 doctor --lint 未通过；未执行 fix".into());
        }
        if let Some(port) = parse_profile(&ps)? {
            gateway["checked"] = json!(true);
            gateway["port"] = json!(port);
            if port_listening(port) {
                gateway["running"] = json!(true);
                let status = gateway_status(&ps, port)
                    .unwrap_or_else(|_| json!({"ok":false,"rpcOk":false,"degraded":false,"status_error":true}));
                if !status.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    blockers.push(Value::String("OpenClaw2 Gateway RPC health 未通过".into()));
                }
                gateway["status"] = status;
            }
        }
    }
    let ready = blockers.is_empty() && doctor.get("ok").and_then(Value::as_bool).unwrap_or(false);
    Ok(
        json!({"ok":ready,"ready":ready,"blockers":blockers,"warnings":warnings,"runtime":report["runtime"],"config":{"private":report["prepared"],"profile":PROFILE},"doctor":doctor,"gateway":gateway}),
    )
}

fn managed_env(p: &Paths) -> Vec<(String, String)> {
    vec![
        ("OPENCLAW_PROFILE".into(), PROFILE.into()),
        ("OPENCLAW_CONFIG_PATH".into(), config_file(p).to_string_lossy().to_string()),
        ("OPENCLAW_STATE_DIR".into(), p.state.to_string_lossy().to_string()),
        ("OPENCLAW_AGENT_DIR".into(), p.state.join("agents").to_string_lossy().to_string()),
        ("OPENCLAW_SUPERVISOR_MODE".into(), "external".into()),
        ("OPENCLAW_SERVICE_REPAIR_POLICY".into(), "external".into()),
        ("OPENCLAW_DISABLE_BONJOUR".into(), "1".into()),
        ("NO_COLOR".into(), "1".into()),
    ]
}

fn gateway_argv(p: &Paths, port: u16) -> Vec<String> {
    vec![
        cli_file(p).to_string_lossy().to_string(),
        "--profile".into(),
        PROFILE.into(),
        "gateway".into(),
        "run".into(),
        "--port".into(),
        port.to_string(),
    ]
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
    launch_private_gateway(&ps, port)
}

/// Start only the private command line. The public Action performs install
/// and profile validation first; keeping the spawn/ownership path separate
/// makes its race-handling testable against a real private child process.
fn launch_private_gateway(ps: &Paths, port: u16) -> Result<Value, String> {
    if port_listening(port) {
        let (_, _, owned) = supervisor_status(ps)?;
        if owned {
            let h = gateway_status(ps, port)?;
            return Ok(
                json!({"changed":false,"running":true,"ready":h["ok"],"pid":supervisor_status(ps)?.1,"port":port,"dashboard_url":format!("http://127.0.0.1:{port}/"),"health":h,"state_version":state_version()}),
            );
        }
        return Err(format!("OpenClaw2 端口 {port} 被外部进程占用，拒绝接管"));
    }
    // Verify the complete derived family together, then release it immediately
    // before spawning. Holding listeners through spawn makes Gateway conflict
    // with its own base/browser-control/CDP listeners. The unavoidable tiny
    // TOCTOU window is closed below by health plus strict process ownership.
    let ports = reserve_port_family(port)?;
    drop(ports);
    let node = node_exe(ps);
    let mut cmd = Command::new(node);
    let argv = gateway_argv(ps, port);
    cmd.args(&argv)
    .current_dir(&ps.workspace)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .env_remove("OPENCLAW_HOME");
    for (key, value) in managed_env(ps) {
        cmd.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 OpenClaw2 Gateway 失败: {e}"))?;
    let identity = (0..10)
        .find_map(|_| {
            let found = process_identity(child.id());
            if found.is_none() { std::thread::sleep(Duration::from_millis(100)); }
            found
        })
        .ok_or_else(|| {
            let _ = child.kill();
            "无法核对刚启动的 OpenClaw2 Gateway 进程归属，已终止".to_string()
        })?;
    let state_dir = ps.state.canonicalize().unwrap_or_else(|_| ps.state.clone());
    let marker = json!({
        "schema_version":1,
        "profile":PROFILE,
        "pid":child.id(),
        "port":port,
        "started_at":now_nanos(),
        "process_started":identity.started,
        "image":identity.image,
        "argv":identity.command_line,
        "state_dir":state_dir,
    });
    write_supervisor_or_kill(&mut child, ps, &marker)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut health = json!({"ok":false,"degraded":false,"rpcOk":false});
    while Instant::now() < deadline {
        if port_listening(port) {
            health = gateway_status(ps, port)
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
    // The listeners had to be released before spawn, so do not trust that the
    // opened port still belongs to us. Re-check the exact image/argv/state and
    // creation identity after the health probe; otherwise fail closed.
    let (_, observed_pid, owned) = supervisor_status(ps)?;
    if !owned || observed_pid != Some(child.id()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("OpenClaw2 Gateway 启动后进程归属核对失败，已终止".into());
    }
    Ok(
        json!({"changed":true,"running":port_listening(port),"ready":ready,"pid":child.id(),"port":port,"dashboard_url":format!("http://127.0.0.1:{port}/"),"health":health,"state_version":state_version()}),
    )
}

fn write_supervisor_or_kill(child: &mut std::process::Child, p: &Paths, marker: &Value) -> Result<(), String> {
    if let Err(e) = atomic_write(
        &supervisor_file(p),
        serde_json::to_string_pretty(marker).unwrap().as_bytes(),
        p,
    ) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("写入 OpenClaw2 supervisor 状态失败，已终止刚启动进程: {e}"));
    }
    Ok(())
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
    let owned = v.get("profile").and_then(Value::as_str) == Some(PROFILE)
        && port == parse_profile(p)?
        && pid.is_some_and(|id| supervisor_owns_process(p, id, port.unwrap_or_default(), &v));
    Ok((port, pid, owned))
}

fn supervisor_owns_process(p: &Paths, pid: u32, port: u16, marker: &Value) -> bool {
    let Some(identity) = process_identity(pid) else { return false };
    identity_matches(p, port, marker, &identity)
}

fn identity_matches(p: &Paths, port: u16, marker: &Value, identity: &ProcessIdentity) -> bool {
    let expected_node = node_exe(p).canonicalize().unwrap_or_else(|_| node_exe(p));
    let expected_cli = cli_file(p).canonicalize().unwrap_or_else(|_| cli_file(p));
    let expected_state = p.state.canonicalize().unwrap_or_else(|_| p.state.clone());
    let marker_state = marker.get("state_dir").and_then(Value::as_str);
    let marker_started = marker.get("process_started").and_then(Value::as_str);
    let image = PathBuf::from(&identity.image).canonicalize().unwrap_or_else(|_| PathBuf::from(&identity.image));
    let command = identity.command_line.to_ascii_lowercase();
    let expected_cli_arg = command_path(&expected_cli).to_ascii_lowercase();
    image == expected_node
        && marker_state == Some(expected_state.to_string_lossy().as_ref())
        && marker_started == Some(identity.started.as_str())
        && command.contains(&expected_cli_arg)
        && command.contains("--profile")
        && command.contains(PROFILE)
        && command.contains("gateway")
        && command.contains("run")
        && command.contains("--port")
        && command.contains(&port.to_string())
}

#[derive(Debug, Deserialize)]
struct ProcessIdentity {
    #[serde(rename = "ExecutablePath")]
    image: String,
    #[serde(rename = "CommandLine")]
    command_line: String,
    #[serde(rename = "CreationDate")]
    started: String,
}

#[cfg(windows)]
fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    let script = format!(
        "$p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}'; if ($null -eq $p) {{ exit 2 }}; $p | Select-Object ExecutablePath,CommandLine,CreationDate | ConvertTo-Json -Compress"
    );
    let out = run_capture(
        Path::new("powershell.exe"),
        &["-NoProfile", "-NonInteractive", "-Command", &script],
        &[],
        &std::env::temp_dir(),
        Duration::from_secs(5),
    ).ok()?;
    (out.status == Some(0)).then(|| serde_json::from_str(&out.stdout).ok()).flatten()
}

#[cfg(not(windows))]
fn process_identity(_: u32) -> Option<ProcessIdentity> { None }

fn port_listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(250),
    )
    .is_ok()
}
#[derive(Debug)]
struct Capture {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_oc(p: &Paths, args: &[&str], timeout: Duration) -> Result<Capture, String> {
    let node = node_exe(p);
    let cli = cli_file(p);
    let mut all = vec![
        cli.to_string_lossy().to_string(),
        "--profile".into(),
        PROFILE.into(),
    ];
    all.extend(args.iter().map(|x| (*x).into()));
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    let owned_env = managed_env(p);
    let env: Vec<(&str, &str)> = owned_env.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    run_capture(&node, &refs, &env, &p.workspace, timeout)
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
    let mut v: Value = serde_json::from_str(&out.stdout).unwrap_or_else(|_| json!({}));
    let rpc = v
        .get("rpcOk")
        .and_then(Value::as_bool)
        .or_else(|| v.pointer("/rpc/ok").and_then(Value::as_bool))
        .unwrap_or(false);
    let degraded = v.get("degraded").and_then(Value::as_bool).unwrap_or(false);
    let ok = out.status == Some(0) && rpc && !degraded;
    v["rpcOk"] = json!(rpc);
    v["degraded"] = json!(degraded);
    v["ok"] = json!(ok);
    redact_gateway_json(&mut v, private_gateway_token(p).as_deref());
    Ok(v)
}

/// `canonicalize` on Windows uses the extended `\\?\` spelling, while WMI
/// reports the normal spelling in CommandLine. Compare that argument in its
/// shell-visible form; image and state still use canonical paths above.
fn command_path(path: &Path) -> String {
    let rendered = path.to_string_lossy().replace('/', "\\");
    rendered
        .strip_prefix(r"\\?\")
        .unwrap_or(rendered.as_str())
        .to_owned()
}
fn private_gateway_token(p: &Paths) -> Option<String> {
    fs::read_to_string(config_file(p))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|config| config.pointer("/gateway/auth/token").and_then(Value::as_str).map(str::to_owned))
}
/// Gateway status is Action output, so it must never carry credentials even
/// when an upstream version puts them into a deeply nested diagnostics object.
fn redact_gateway_json(value: &mut Value, private_token: Option<&str>) {
    match value {
        Value::Object(object) => for (key, nested) in object.iter_mut() {
            let lower = key.to_ascii_lowercase();
            if ["token", "secret", "password", "authorization", "cookie", "credential", "api_key", "apikey"]
                .iter().any(|needle| lower.contains(needle)) {
                *nested = Value::String("[redacted]".into());
            } else { redact_gateway_json(nested, private_token); }
        },
        Value::Array(items) => for nested in items { redact_gateway_json(nested, private_token); },
        Value::String(text) => if let Some(token) = private_token.filter(|token| !token.is_empty()) {
            if text.contains(token) { *text = text.replace(token, "[redacted]"); }
        },
        _ => {}
    }
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
) -> Result<Capture, String> {
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
            return Ok(Capture {
                status: s.code(),
                stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        if begin.elapsed() >= timeout {
            let _ = child.kill();
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            return Ok(Capture {
                status: None,
                stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            });
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
    if out.status == Some(0) {
        Ok(())
    } else {
        Err(format!("{what}失败: {}", redact_tail(if out.stderr.is_empty() { &out.stdout } else { &out.stderr })))
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
    fn derived_port_family_is_complete_and_rejects_a_control_collision() {
        let family = port_family(19789).unwrap();
        assert_eq!(family.len(), 102);
        assert_eq!(family[0], 19789);
        assert_eq!(family[1], 19791);
        assert_eq!(family[2], 19800);
        assert_eq!(*family.last().unwrap(), 19899);
        let base = 31_000;
        let control = TcpListener::bind((Ipv4Addr::LOCALHOST, base + 2)).unwrap();
        assert!(reserve_port_family(base).unwrap_err().contains("派生端口"));
        drop(control);
        assert!(reserve_port_family(base).is_ok());
    }
    #[test]
    fn managed_launch_has_only_the_private_argv_and_exact_openclaw_env() {
        let p = paths_from_root(std::env::temp_dir().join("uking-openclaw2-launch-plan"));
        let argv = gateway_argv(&p, 19789);
        assert_eq!(argv, vec![
            cli_file(&p).to_string_lossy().to_string(), "--profile".into(), PROFILE.into(),
            "gateway".into(), "run".into(), "--port".into(), "19789".into(),
        ]);
        let env = managed_env(&p).into_iter().collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(env.get("OPENCLAW_SUPERVISOR_MODE").map(String::as_str), Some("external"));
        assert_eq!(env.get("OPENCLAW_SERVICE_REPAIR_POLICY").map(String::as_str), Some("external"));
        assert_eq!(env.get("OPENCLAW_DISABLE_BONJOUR").map(String::as_str), Some("1"));
        assert!(!env.contains_key("OPENCLAW_HOME"));
    }
    #[cfg(windows)]
    #[test]
    fn npm_integrity_is_checked_against_the_actual_tarball_bytes() {
        let file = std::env::temp_dir().join(format!("uking-openclaw2-integrity-{}.tgz", std::process::id()));
        fs::write(&file, b"abc").unwrap();
        let good = "sha512-3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw==";
        assert!(verify_npm_integrity_file(&file, good).is_ok());
        assert!(verify_npm_integrity_file(&file, "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==").is_err());
        let _ = fs::remove_file(file);
    }
    #[cfg(windows)]
    #[test]
    fn stdout_json_is_not_contaminated_by_stderr() {
        let out = run_capture(
            Path::new("powershell.exe"),
            &["-NoProfile", "-NonInteractive", "-Command", "[Console]::Out.Write('{\"ok\":true}'); [Console]::Error.Write('diagnostic-secret')"],
            &[],
            &std::env::temp_dir(),
            Duration::from_secs(5),
        ).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out.stdout).unwrap()["ok"], true);
        assert!(out.stderr.contains("diagnostic-secret"));
        assert!(!out.stdout.contains("diagnostic-secret"));
    }
    #[test]
    fn gateway_status_redaction_is_recursive_and_replaces_private_token_values() {
        let token = "test-private-gateway-token";
        let mut status = json!({"token":token,"nested":{"accessToken":token,"url":format!("ws://127.0.0.1/?token={token}"),"items":[{"credentials":{"cookie":token}}]}});
        redact_gateway_json(&mut status, Some(token));
        let wire = serde_json::to_string(&status).unwrap();
        assert!(!wire.contains(token));
        assert_eq!(status["token"], "[redacted]");
        assert_eq!(status["nested"]["accessToken"], "[redacted]");
        assert_eq!(status["nested"]["items"][0]["credentials"], "[redacted]");
    }
    #[test]
    fn preflight_explicitly_reports_unchecked_not_running_without_private_runtime() {
        let sb = crate::testsandbox::enter_raw("openclaw2-preflight-not-running");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::remove_var("HOME");
        let result = preflight().unwrap();
        assert_eq!(result["gateway"]["checked"], false);
        assert_eq!(result["gateway"]["running"], false);
        assert!(result["gateway"].get("status").is_some());
    }
    #[cfg(windows)]
    fn private_node_for_gateway_test(p: &Paths) -> Option<PathBuf> {
        let out = Command::new("where.exe").arg("node.exe").output().ok()?;
        let source = String::from_utf8_lossy(&out.stdout).lines().next().map(str::trim)
            .filter(|line| !line.is_empty()).map(PathBuf::from)?;
        let destination = node_exe(p);
        fs::create_dir_all(destination.parent()?).ok()?;
        fs::copy(source, &destination).ok()?;
        Some(destination)
    }
    #[cfg(windows)]
    fn unused_gateway_port_base() -> u16 {
        (32_000u16..64_000u16).step_by(131)
            .find(|base| reserve_port_family(*base).is_ok())
            .expect("应能找到完整可用的 OpenClaw2 端口族")
    }
    #[cfg(windows)]
    #[test]
    fn private_gateway_starts_after_port_reservations_are_released() {
        let p = paths_from_root(std::env::temp_dir().join(format!("uking-openclaw2-real-gateway-{}-{}", std::process::id(), now_nanos())));
        create_layout(&p).unwrap();
        let Some(node) = private_node_for_gateway_test(&p) else {
            let _ = fs::remove_dir_all(&p.root);
            return;
        };
        assert_eq!(node, node_exe(&p));
        let port = unused_gateway_port_base();
        fs::create_dir_all(cli_file(&p).parent().unwrap()).unwrap();
        fs::write(cli_file(&p), r#"import net from 'node:net';
const args = process.argv.slice(2);
if (args.includes('status')) { console.log(JSON.stringify({rpcOk:true,degraded:false,nested:{token:'fake-status-token'}})); process.exit(0); }
const index = args.indexOf('--port'); const port = Number(args[index + 1]);
const server = net.createServer(); server.listen(port, '127.0.0.1'); setInterval(() => {}, 1000);
"#).unwrap();
        fs::write(profile_file(&p), serde_json::to_string(&json!({"schema_version":1,"profile":PROFILE,"port":port})).unwrap()).unwrap();
        fs::write(config_file(&p), serde_json::to_string(&json!({"gateway":{"auth":{"token":"fake-status-token"}}})).unwrap()).unwrap();
        let launched = launch_private_gateway(&p, port).unwrap();
        let pid = launched["pid"].as_u64().unwrap() as u32;
        assert_eq!(launched["running"], true);
        assert_eq!(launched["ready"], true);
        assert_eq!(launched["health"]["nested"]["token"], "[redacted]");
        assert_eq!(supervisor_status(&p).unwrap(), (Some(port), Some(pid), true));
        let _ = Command::new("taskkill.exe").args(["/PID", &pid.to_string(), "/F", "/T"]).output();
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_identity(pid).is_some() && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(50)); }
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    #[test]
    fn supervisor_marker_failure_terminates_the_child() {
        let p = paths_from_root(std::env::temp_dir().join(format!("uking-openclaw2-marker-{}", std::process::id())));
        let _ = fs::remove_dir_all(&p.root);
        create_layout(&p).unwrap();
        // A directory at the marker path makes the atomic file replacement fail.
        fs::create_dir_all(supervisor_file(&p)).unwrap();
        let mut child = Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 30 127.0.0.1 > nul"])
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(write_supervisor_or_kill(&mut child, &p, &json!({"pid":pid})).is_err());
        assert!(child.try_wait().unwrap().is_some(), "marker 失败后 child 必须被杀死");
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn pid_reuse_is_rejected_when_creation_identity_changes() {
        let p = paths_from_root(std::env::temp_dir().join("uking-openclaw2-pid-reuse"));
        let node = node_exe(&p);
        let cli = cli_file(&p);
        let state = p.state.clone();
        let identity = ProcessIdentity {
            image: node.to_string_lossy().to_string(),
            command_line: format!("\"{}\" \"{}\" --profile {PROFILE} gateway run --port 19789", node.display(), cli.display()),
            started: "first-process".into(),
        };
        let marker = json!({"state_dir":state,"process_started":"first-process"});
        assert!(identity_matches(&p, 19789, &marker, &identity));
        let reused = ProcessIdentity { started: "reused-pid".into(), ..identity };
        assert!(!identity_matches(&p, 19789, &marker, &reused));
    }
    #[test]
    fn prepare_does_not_touch_legacy_openclaw_or_clawx_sentinels() {
        let sb = crate::testsandbox::enter_raw("openclaw2-legacy-sentinels");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::remove_var("HOME");
        let sentinels = [
            sb.root().join(".openclaw/old.txt"),
            crate::installer::uking_home().join("openclaw/old.txt"),
            sb.root().join("AppData/Roaming/ClawX/old.txt"),
        ];
        for file in &sentinels {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"legacy sentinel").unwrap();
        }
        let before = sentinels.iter().map(|file| {
            (crate::installer::sha256_hex_bytes(&fs::read(file).unwrap()), fs::metadata(file).unwrap().modified().unwrap())
        }).collect::<Vec<_>>();
        prepare(None).unwrap();
        for (file, (hash, mtime)) in sentinels.iter().zip(before) {
            assert_eq!(crate::installer::sha256_hex_bytes(&fs::read(file).unwrap()), hash);
            assert_eq!(fs::metadata(file).unwrap().modified().unwrap(), mtime);
        }
    }
    #[test]
    fn model_candidate_preserves_private_unknowns_and_only_uses_file_secret_ref() {
        let p = paths_from_root(std::env::temp_dir().join(format!("uking-openclaw2-model-candidate-{}", now_nanos())));
        create_layout(&p).unwrap();
        fs::write(config_file(&p), serde_json::to_vec(&json!({"gateway":{"auth":{"token":"gateway-private"}},"workspace":{"keep":true},"unknown":{"keep":"yes"},"models":{"providers":{"someone-else":{"keep":true}}}})).unwrap()).unwrap();
        let route = ModelRoute { source_id:"demo".into(), source_name:"Demo".into(), base:"https://example.com/v1".into(), model:"demo-chat".into(), key:"never-in-config".into(), key_source:"explicit".into() };
        let key = model_provider_key(&route.source_id, &route.base);
        let candidate = model_candidate_config(&p, &route, &key, &model_secret_file(&p, "next")).unwrap();
        let text = String::from_utf8(candidate).unwrap();
        assert!(!text.contains("never-in-config"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["gateway"]["auth"]["token"], "gateway-private");
        assert_eq!(value["workspace"]["keep"], true);
        assert_eq!(value["unknown"]["keep"], "yes");
        assert_eq!(value["models"]["mode"], "merge");
        assert_eq!(value["models"]["providers"]["someone-else"]["keep"], true);
        assert_eq!(value["models"]["providers"][key.as_str()]["apiKey"]["source"], "file");
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn model_endpoint_rejects_remote_http_and_credential_url() {
        assert!(normalized_model_base("https://api.example.com/v1").is_ok());
        assert!(normalized_model_base("http://127.0.0.1:11434/v1").is_ok());
        for invalid in ["http://example.com/v1", "https://u:p@example.com/v1", "https://example.com/v1?q=x", "https://example.com/v1#x"] {
            assert!(normalized_model_base(invalid).is_err(), "{invalid}");
        }
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
            crate::actions::OPENCLAW2_CONFIGURE_MODEL,
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
