//! OpenClaw 2 private runtime adapter.
//!
//! This module deliberately knows nothing about ClawX or the legacy OpenClaw
//! install. Every path is rooted below `installer::uking_home()/openclaw2` and
//! every child process receives explicit config/state paths. It is the only
//! implementation behind the five `runtime.openclaw2.*` Actions.

use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROFILE: &str = "uking-openclaw2";
const CONFIG_NAME: &str = "openclaw.json";
const PROFILE_NAME: &str = "profile.json";
const SUPERVISOR_NAME: &str = "supervisor.json";
const INSTALL_NAME: &str = "installed.json";
const MODEL_MARKER_NAME: &str = "model-config.json";
const MODEL_SECRET_PROVIDER: &str = "uking-openclaw2-file";
const NODE_STAGE_NAME: &str = "node-install-staging";
const NODE_STAGE_MARKER: &str = ".uking-openclaw2-node-stage.json";
const NODE_RUNTIME_MARKER: &str = ".uking-openclaw2-node-runtime.json";
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

pub(crate) use crate::model_route::OpenClaw2ModelRoute as ModelRoute;

fn model_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
fn model_test_fault_slot() -> &'static Mutex<Option<(PathBuf, String)>> {
    static FAULT: OnceLock<Mutex<Option<(PathBuf, String)>>> = OnceLock::new();
    FAULT.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn model_test_fault(p: &Paths, stage: &str) -> bool {
    model_test_fault_slot()
        .lock()
        .ok()
        .and_then(|value| value.clone())
        .is_some_and(|(root, active)| root == p.root && active == stage)
}

#[cfg(not(test))]
fn model_test_fault(_: &Paths, _: &str) -> bool {
    false
}

#[cfg(test)]
fn set_model_test_fault(p: &Paths, stage: Option<&str>) {
    let mut fault = model_test_fault_slot().lock().unwrap();
    *fault = stage.map(|stage| (p.root.clone(), stage.to_owned()));
}

fn paths() -> Paths {
    // The portable bundle owns a fixed, marker-verified OpenClaw tree. The
    // normal desktop adapter keeps using its historical private home.
    paths_from_root(
        crate::portable_context::openclaw_root()
            .unwrap_or_else(|| crate::installer::uking_home().join("openclaw2")),
    )
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
fn relocation_backup_file(p: &Paths) -> PathBuf {
    p.state.join(format!(
        "openclaw.json.before-relocation-{}-{}.json",
        std::process::id(),
        now_nanos()
    ))
}
fn supervisor_file(p: &Paths) -> PathBuf {
    p.run.join(SUPERVISOR_NAME)
}
fn install_file(p: &Paths) -> PathBuf {
    p.runtime.join(INSTALL_NAME)
}
fn model_marker_file(p: &Paths) -> PathBuf {
    p.state.join(MODEL_MARKER_NAME)
}
fn model_secrets_dir(p: &Paths) -> PathBuf {
    p.state.join("secrets")
}
fn model_txn_root(p: &Paths) -> PathBuf {
    p.run.join("model-config-txn")
}
fn node_stage_dir(p: &Paths) -> PathBuf {
    p.run.join(NODE_STAGE_NAME)
}
fn node_stage_marker(p: &Paths) -> PathBuf {
    node_stage_dir(p).join(NODE_STAGE_MARKER)
}
fn node_runtime_marker(p: &Paths) -> PathBuf {
    p.node.join(NODE_RUNTIME_MARKER)
}
fn node_archive_file(p: &Paths, m: &RuntimeManifest) -> PathBuf {
    p.runtime
        .join(format!("node-v{}-win-x64.zip", m.node.version))
}
fn openclaw_archive_file(p: &Paths, m: &RuntimeManifest) -> PathBuf {
    p.runtime
        .join(format!("openclaw-{}.tgz", m.openclaw_version))
}

fn ensure_private_path(path: &Path, p: &Paths) -> Result<(), String> {
    if crate::portable_context::current().is_some() {
        crate::portable_context::ensure_owned_path(path)?;
    }
    // Never let a lexical descendant hide behind an existing symlink. Before
    // the root exists there cannot yet be such a descendant; once it exists,
    // both the root and the nearest existing ancestor must canonicalize under
    // the same private root.
    if !path.starts_with(&p.root) {
        Err("拒绝访问 OpenClaw2 私有根目录以外的路径".into())
    } else if !p.root.exists() {
        Ok(())
    } else {
        let root = p
            .root
            .canonicalize()
            .map_err(|e| format!("解析 OpenClaw2 私有根失败: {e}"))?;
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
        &p.root.join("home"),
        &p.root.join("tmp"),
        &p.root.join("cache"),
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
fn extended_absolute_path(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or("OpenClaw2 原子替换路径没有父目录")?;
    let name = path
        .file_name()
        .ok_or("OpenClaw2 原子替换路径没有文件名")?;
    // Both source and destination live in an existing private directory.  Let
    // Windows canonicalize that directory before adding the leaf: this keeps
    // a moved portable profile valid even when its transaction subdirectory
    // would otherwise make a normal Win32 path exceed MAX_PATH.
    let absolute = fs::canonicalize(parent)
        .map_err(|e| format!("规范化 OpenClaw2 原子替换目录失败: {e}"))?
        .join(name);
    let rendered = absolute.to_string_lossy();
    if rendered.starts_with(r"\\?\") {
        return Ok(absolute);
    }
    if let Some(unc) = rendered.strip_prefix(r"\\") {
        return Ok(PathBuf::from(format!(r"\\?\UNC\{unc}")));
    }
    if absolute.is_absolute() {
        return Ok(PathBuf::from(format!(r"\\?\{rendered}")));
    }
    Err("OpenClaw2 原子替换路径不是绝对路径".into())
}

#[cfg(windows)]
fn atomic_replace_windows(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let from = extended_absolute_path(from)?;
    let to = extended_absolute_path(to)?;
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
    if unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING,
        )
    } == 0
    {
        // Capture GetLastError before cleanup. `remove_file` can otherwise
        // overwrite it with a misleading success code.
        let error = std::io::Error::last_os_error();
        let _ = fs::remove_file(from);
        Err(format!("原子替换 OpenClaw2 配置失败: {error}"))
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

fn node_version_matches(actual: &str, expected: &str) -> bool {
    actual.trim().trim_start_matches('v') == expected.trim().trim_start_matches('v')
}

fn node_install_marker(version: &str, kind: &str) -> Value {
    json!({"schema_version":1,"owner":PROFILE,"kind":kind,"node_version":version})
}

fn marker_matches(path: &Path, version: &str, kind: &str) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|marker| marker == node_install_marker(version, kind))
}

/// Only this explicitly marked directory is disposable.  A stale archive or
/// arbitrary folder under the private root is not proof that we created it,
/// so it remains intact and the retry fails closed with a useful diagnosis.
fn clear_owned_node_stage(p: &Paths, m: &RuntimeManifest) -> Result<(), String> {
    let stage = node_stage_dir(p);
    if !stage.exists() {
        return Ok(());
    }
    ensure_private_path(&stage, p)?;
    if !stage.is_dir() || !marker_matches(&node_stage_marker(p), &m.node.version, "node-stage") {
        return Err(
            "OpenClaw2 发现未知或未标记的 Node staging，拒绝自动删除；请保留现场诊断".into(),
        );
    }
    fs::remove_dir_all(&stage).map_err(|e| format!("清理上次 OpenClaw2 Node staging 失败: {e}"))
}

enum PrivateNodeState {
    Missing,
    Ready,
    Unknown,
}

fn private_node_state(p: &Paths, m: &RuntimeManifest) -> PrivateNodeState {
    if !p.node.exists() {
        return PrivateNodeState::Missing;
    }
    if !p.node.is_dir() || !node_exe(p).is_file() || !npm_exe(p).is_file() {
        return PrivateNodeState::Unknown;
    }
    // A current install carries our runtime marker.  The one narrow legacy
    // adoption path is backed by the pinned archive hash and the expected
    // Node layout; it lets the previous adapter's interrupted second pass
    // resume without ever deleting its non-empty runtime directory.
    let marked = marker_matches(&node_runtime_marker(p), &m.node.version, "node-runtime");
    let legacy_archive_proves_origin =
        verify_sha256_file(&node_archive_file(p, m), &m.node.windows_x64_sha256).is_ok();
    if !(marked || legacy_archive_proves_origin) {
        return PrivateNodeState::Unknown;
    }
    match read_node_version(p) {
        Some(version)
            if node_version_matches(&version, &m.node.version) && node_supported(&version) =>
        {
            PrivateNodeState::Ready
        }
        _ => PrivateNodeState::Unknown,
    }
}

fn install_replay_ready(p: &Paths, m: &RuntimeManifest, node_version: Option<&str>) -> bool {
    integrity_ok(p, m)
        && node_version.is_some_and(|version| {
            node_supported(version) && node_version_matches(version, &m.node.version)
        })
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
        let c = if chunk[2] == b'=' {
            0
        } else {
            value(chunk[2]).ok_or("npm integrity Base64 无效")?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            value(chunk[3]).ok_or("npm integrity Base64 无效")?
        };
        if chunk[2] == b'=' && chunk[3] != b'=' {
            return Err("npm integrity Base64 padding 无效".into());
        }
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
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
        return Err(format!(
            "OpenClaw2 SHA-512 校验失败: {}",
            redact_tail(&output.stderr)
        ));
    }
    let actual = output
        .stdout
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            let hex: String = trimmed.chars().filter(|c| c.is_ascii_hexdigit()).collect();
            (hex.len() == 128
                && trimmed
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c.is_ascii_whitespace()))
            .then_some(hex)
        })
        .ok_or("OpenClaw2 SHA-512 工具没有返回摘要")?;
    let mut mismatch = 0u8;
    for (byte, pair) in expected.iter().zip(actual.as_bytes().chunks_exact(2)) {
        let parsed = u8::from_str_radix(
            std::str::from_utf8(pair).map_err(|_| "SHA-512 输出无效")?,
            16,
        )
        .map_err(|_| "SHA-512 输出无效")?;
        mismatch |= byte ^ parsed;
    }
    if mismatch == 0 {
        Ok(())
    } else {
        Err("OpenClaw2 npm tarball SHA-512/integrity 不匹配".into())
    }
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

/// A relocation backup is evidence, not a rolling cache.  Never replace an
/// older backup; write a private temporary file and atomically claim a fresh
/// final name so a later move cannot erase the earlier recovery point.
fn atomic_write_new(path: &Path, bytes: &[u8], p: &Paths) -> Result<(), String> {
    ensure_private_path(path, p)?;
    let parent = path.parent().ok_or("OpenClaw2 备份路径没有父目录")?;
    ensure_private_path(parent, p)?;
    fs::create_dir_all(parent).map_err(|e| format!("创建备份目录失败: {e}"))?;
    if path.exists() {
        return Err("OpenClaw2 重定位备份已存在，拒绝覆盖".into());
    }
    let tmp = parent.join(format!(
        ".uking-openclaw2-backup-{}-{}.tmp",
        std::process::id(),
        now_nanos()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("创建 OpenClaw2 重定位备份失败: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("写入 OpenClaw2 重定位备份失败: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("同步 OpenClaw2 重定位备份失败: {e}"))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(format!("原子提交 OpenClaw2 重定位备份失败: {e}"))
        }
    }
}

fn managed_profile_value(p: &Paths, port: u16, config: &[u8]) -> Value {
    json!({"schema_version":2,"profile":PROFILE,"owner":PROFILE,"port":port,"managed_root":p.root,"config_hash":crate::installer::sha256_hex_bytes(config)})
}
fn managed_profile_for_config(p: &Paths, config: &[u8]) -> Result<PathBuf, String> {
    let profile: Value = serde_json::from_slice(
        &fs::read(profile_file(p)).map_err(|_| "OpenClaw2 缺少受管 profile，拒绝重定位")?,
    )
    .map_err(|_| "OpenClaw2 受管 profile 已损坏，拒绝重定位")?;
    let hash = crate::installer::sha256_hex_bytes(config);
    if profile.get("schema_version").and_then(Value::as_u64) != Some(2)
        || profile.get("profile").and_then(Value::as_str) != Some(PROFILE)
        || profile.get("owner").and_then(Value::as_str) != Some(PROFILE)
        || profile.get("config_hash").and_then(Value::as_str) != Some(hash.as_str())
    {
        return Err("OpenClaw2 配置不具备受管 profile/owner/hash 证明，拒绝重定位".into());
    }
    profile
        .get("managed_root")
        .and_then(Value::as_str)
        .filter(|root| Path::new(root).is_absolute())
        .map(PathBuf::from)
        .ok_or("OpenClaw2 受管 profile 缺少旧根目录，拒绝重定位".into())
}
fn write_managed_profile(p: &Paths, port: u16, config: &[u8]) -> Result<(), String> {
    atomic_write(
        &profile_file(p),
        &serde_json::to_vec_pretty(&managed_profile_value(p, port, config))
            .map_err(|_| "无法序列化 OpenClaw2 受管 profile")?,
        p,
    )
}
fn refresh_managed_profile_hash(p: &Paths, config: &[u8]) -> Result<(), String> {
    let Ok(text) = fs::read_to_string(profile_file(p)) else {
        return Ok(());
    };
    let mut profile: Value =
        serde_json::from_str(&text).map_err(|_| "OpenClaw2 profile 已损坏，拒绝更新受管配置")?;
    if profile.get("schema_version").and_then(Value::as_u64) != Some(2) {
        return Ok(());
    }
    let port = profile
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|x| u16::try_from(x).ok())
        .ok_or("OpenClaw2 受管 profile 缺少端口")?;
    if profile.get("profile").and_then(Value::as_str) != Some(PROFILE)
        || profile.get("owner").and_then(Value::as_str) != Some(PROFILE)
        || profile.get("managed_root").and_then(Value::as_str)
            != Some(p.root.to_string_lossy().as_ref())
    {
        return Err("OpenClaw2 profile 不是当前受管配置，拒绝更新".into());
    }
    let object = profile
        .as_object_mut()
        .ok_or("OpenClaw2 受管 profile 形状无效")?;
    object.insert(
        "config_hash".into(),
        json!(crate::installer::sha256_hex_bytes(config)),
    );
    object.insert("port".into(), json!(port));
    atomic_write(
        &profile_file(p),
        &serde_json::to_vec_pretty(&profile).map_err(|_| "无法序列化 OpenClaw2 受管 profile")?,
        p,
    )
}
fn no_running_relocation_instance(p: &Paths) -> Result<(), String> {
    if let Ok(text) = fs::read_to_string(supervisor_file(p)) {
        let marker: Value = serde_json::from_str(&text)
            .map_err(|_| "OpenClaw2 supervisor 状态已损坏，拒绝在重定位时修改配置")?;
        if marker
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .is_some_and(|pid| process_identity(pid).is_some())
        {
            return Err("OpenClaw2 Gateway 正在运行，拒绝重定位配置".into());
        }
    }
    if parse_profile(p)?.is_some_and(port_listening) {
        return Err("OpenClaw2 profile 端口正在使用，拒绝重定位配置".into());
    }
    Ok(())
}
fn relocate_managed_portable_config(p: &Paths) -> Result<bool, String> {
    let config_path = config_file(p);
    let old_bytes = fs::read(&config_path).map_err(|_| "OpenClaw2 私有配置不可读")?;
    let old_config = snapshot_file(&config_path);
    let old_marker = snapshot_file(&model_marker_file(p));
    let old_profile = snapshot_file(&profile_file(p));
    let mut config: Value =
        serde_json::from_slice(&old_bytes).map_err(|_| "OpenClaw2 私有配置已损坏，拒绝重定位")?;
    let old_workspace = config
        .pointer("/agents/defaults/workspace")
        .and_then(Value::as_str)
        .ok_or("OpenClaw2 配置缺少 workspace，拒绝重定位")?;
    if old_workspace == p.workspace.to_string_lossy() {
        return Ok(false);
    }
    no_running_relocation_instance(p)?;
    let old_root = managed_profile_for_config(p, &old_bytes)?;
    if old_root == p.root || PathBuf::from(old_workspace) != old_root.join("workspace") {
        return Err("OpenClaw2 workspace 不是受管旧根目录引用，拒绝重定位".into());
    }
    let provider_pointer = format!("/secrets/providers/{MODEL_SECRET_PROVIDER}");
    let mut marker = None;
    if let Some(provider) = config.pointer(&provider_pointer) {
        let value: Value = serde_json::from_slice(
            &fs::read(model_marker_file(p))
                .map_err(|_| "OpenClaw2 file secret 缺少受管 marker，拒绝重定位")?,
        )
        .map_err(|_| "OpenClaw2 file secret marker 已损坏，拒绝重定位")?;
        let name = value
            .get("secret_basename")
            .and_then(Value::as_str)
            .filter(|name| !name.contains(['/', '\\']) && name.starts_with("model-"))
            .ok_or("OpenClaw2 file secret marker 缺少安全名称，拒绝重定位")?;
        let old_hash = crate::installer::sha256_hex_bytes(&old_bytes);
        if value.get("owner").and_then(Value::as_str) != Some(PROFILE)
            || value.get("config_hash").and_then(Value::as_str) != Some(old_hash.as_str())
            || provider.get("source").and_then(Value::as_str) != Some("file")
            || provider.get("mode").and_then(Value::as_str) != Some("json")
            || provider.get("path").and_then(Value::as_str)
                != Some(old_root.join("state").join("secrets").join(name).to_string_lossy().as_ref())
        {
            return Err("OpenClaw2 file secret 不是受管旧根目录引用，拒绝重定位".into());
        }
        let secret = model_secrets_dir(p).join(name);
        ensure_private_path(&secret, p)?;
        if !secret.is_file() {
            return Err("OpenClaw2 移动后的受管 file secret 不存在，拒绝重定位".into());
        }
        marker = Some(value);
    } else if model_marker_file(p).exists() {
        return Err("OpenClaw2 model marker 与配置不一致，拒绝重定位".into());
    }
    *config
        .pointer_mut("/agents/defaults/workspace")
        .ok_or("OpenClaw2 配置 workspace 形状无效，拒绝重定位")? = json!(p.workspace);
    if let Some(provider) = config.pointer_mut(&provider_pointer) {
        let name = marker
            .as_ref()
            .and_then(|m| m.get("secret_basename"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        provider["path"] = json!(model_secrets_dir(p).join(name));
    }
    let new_bytes =
        serde_json::to_vec_pretty(&config).map_err(|_| "无法序列化 OpenClaw2 重定位配置")?;
    let result = (|| -> Result<(), String> {
        atomic_write_new(&relocation_backup_file(p), &old_bytes, p)?;
        atomic_write(&config_path, &new_bytes, p)?;
        if let Some(mut value) = marker {
            value["config_hash"] = json!(crate::installer::sha256_hex_bytes(&new_bytes));
            atomic_write(
                &model_marker_file(p),
                &serde_json::to_vec_pretty(&value)
                    .map_err(|_| "无法序列化 OpenClaw2 model marker")?,
                p,
            )?;
        }
        write_managed_profile(
            p,
            parse_profile(p)?.ok_or("OpenClaw2 受管 profile 缺少端口")?,
            &new_bytes,
        )?;
        if model_test_fault(p, "relocation_profile") {
            return Err("validation_failed: OpenClaw2 重定位 profile 提交注入失败".into());
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(true),
        Err(error) => restore_file_snapshot(p, &config_path, &old_config)
            .and_then(|_| restore_file_snapshot(p, &model_marker_file(p), &old_marker))
            .and_then(|_| restore_file_snapshot(p, &profile_file(p), &old_profile))
            .map(|_| false)
            .map_err(|_| "rollback_failed: OpenClaw2 重定位失败且回滚未完成".into())
            .and_then(|_| Err(error)),
    }
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
                return Err(format!(
                    "OpenClaw2 端口族 {base}（含派生端口 {port}）已被占用"
                ));
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
    state_version_for(&paths())
}

fn state_version_for(p: &Paths) -> String {
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
    // The marker itself only names a secret. Hash the currently referenced
    // secret too, so an out-of-band key replacement invalidates optimistic
    // state rather than letting a stale configure request overwrite it.
    if let Some(name) = fs::read_to_string(model_marker_file(p))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|marker| {
            marker
                .get("secret_basename")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    {
        let secret = model_secrets_dir(p).join(name);
        snapshot.push_str("current-model-secret\n");
        match fs::read(secret) {
            Ok(bytes) => snapshot.push_str(&crate::installer::sha256_hex_bytes(&bytes)),
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
    let mut config_ok = private_config_ok(&ps)?;
    if !config_ok && crate::portable_context::current().is_some() && config_file(&ps).is_file() {
        if relocate_managed_portable_config(&ps)? {
            config_ok = private_config_ok(&ps)?;
        }
    }
    // A complete private profile is a replay, including while its own gateway
    // holds the family. Do not turn that benign replay into a destructive port
    // probe or a config rewrite.
    if existing.is_some() && config_ok {
        return Ok(
            json!({"changed":false,"prepared":true,"profile":PROFILE,"port":chosen,"state_version":state_version()}),
        );
    }
    if existing.is_none() && config_file(&ps).exists() {
        return Err("OpenClaw2 发现没有 profile 的已有私有配置，拒绝覆盖".into());
    }
    // Keep every listener reserved through the two atomic writes below. A
    // gateway-only probe would miss browser-control/CDP collisions.
    let _ports = reserve_port_family(chosen)?;
    let mut changed = false;
    if !config_file(&ps).exists() {
        if profile_file(&ps).exists() {
            return Err("OpenClaw2 profile 存在但配置缺失，拒绝重建".into());
        }
        let token = random_token()?;
        let cfg = json!({"gateway":{"mode":"local","port":chosen,"bind":"loopback","auth":{"mode":"token","token":token}},"agents":{"defaults":{"workspace":ps.workspace.to_string_lossy()}}});
        let cfg = serde_json::to_vec_pretty(&cfg).unwrap();
        atomic_write(&config_file(&ps), &cfg, &ps)?;
        write_managed_profile(&ps, chosen, &cfg)?;
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
    let complete_archives = integrity_ok(&ps, &m);
    let node_version = complete_archives.then(|| read_node_version(&ps)).flatten();
    if complete_archives && install_replay_ready(&ps, &m, node_version.as_deref()) {
        return Ok(
            json!({"changed":false,"installed":true,"node_version":node_version,"openclaw_version":m.openclaw_version,"integrity_ok":true,"state_version":state_version()}),
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
        match private_node_state(&ps, &m) {
            PrivateNodeState::Ready => progress("复用已校验的 OpenClaw2 私有 Node runtime…"),
            PrivateNodeState::Unknown => {
                return Err(
                    "OpenClaw2 发现非空但未验证的私有 Node runtime，拒绝自动删除；请保留现场诊断"
                        .into(),
                )
            }
            PrivateNodeState::Missing => {
                clear_owned_node_stage(&ps, &m)?;
                let archive = node_archive_file(&ps, &m);
                if verify_sha256_file(&archive, &m.node.windows_x64_sha256).is_err() {
                    download(&m.node.windows_x64_url, &archive, Duration::from_secs(840))?;
                }
                if verify_sha256_file(&archive, &m.node.windows_x64_sha256).is_err() {
                    let _ = fs::remove_file(&archive);
                    return Err("OpenClaw2 Node SHA-256 不匹配，已拒绝安装".into());
                }
                let stage = node_stage_dir(&ps);
                fs::create_dir_all(&stage)
                    .map_err(|e| format!("创建 OpenClaw2 Node staging 失败: {e}"))?;
                atomic_write(
                    &node_stage_marker(&ps),
                    serde_json::to_vec_pretty(&node_install_marker(&m.node.version, "node-stage"))
                        .unwrap()
                        .as_slice(),
                    &ps,
                )?;
                run_status(
                    Command::new("tar").args([
                        "-xf",
                        &archive.to_string_lossy(),
                        "-C",
                        &stage.to_string_lossy(),
                    ]),
                    Duration::from_secs(120),
                    "解压 OpenClaw2 Node",
                )?;
                let source = stage.join(format!("node-v{}-win-x64", m.node.version));
                if !source.join("node.exe").is_file() || !source.join("npm.cmd").is_file() {
                    return Err(
                        "OpenClaw2 Node staging 解压产物不完整；已保留私有诊断现场以便安全重试"
                            .into(),
                    );
                }
                atomic_write(
                    &source.join(NODE_RUNTIME_MARKER),
                    serde_json::to_vec_pretty(&node_install_marker(
                        &m.node.version,
                        "node-runtime",
                    ))
                    .unwrap()
                    .as_slice(),
                    &ps,
                )?;
                fs::rename(&source, &ps.node)
                    .map_err(|e| format!("整理 OpenClaw2 Node staging 失败: {e}"))?;
                clear_owned_node_stage(&ps, &m)?;
            }
        }
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
            redact_tail(if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            })
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
    let (scheme, rest) = base
        .split_once("://")
        .ok_or("invalid_input: OpenClaw2 endpoint 必须为绝对 URL")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return Err("invalid_input: OpenClaw2 endpoint 仅支持 HTTP(S)".into());
    }
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || rest.contains('?') || rest.contains('#') {
        return Err("invalid_input: OpenClaw2 endpoint 不允许认证、query 或 fragment".into());
    }
    if scheme.eq_ignore_ascii_case("http") {
        let host = authority
            .split(':')
            .next()
            .unwrap_or(authority)
            .trim_matches(['[', ']']);
        if !matches!(
            host.to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "::1"
        ) {
            return Err("invalid_input: OpenClaw2 明文 HTTP 仅允许 loopback".into());
        }
    }
    Ok(base.trim_end_matches('/').to_string())
}

fn model_provider_key(source_id: &str, base: &str) -> String {
    let digest = crate::installer::sha256_hex_bytes(format!("{source_id}\n{base}").as_bytes());
    format!("uking-oc2-{}", &digest[..12])
}

fn model_ref(provider_key: &str, model: &str) -> String {
    format!("{provider_key}/{model}")
}

fn model_secret_file(p: &Paths, nonce: &str) -> PathBuf {
    model_secrets_dir(p).join(format!("model-{nonce}.json"))
}

fn model_marker_matches(p: &Paths, route: &ModelRoute, provider_key: &str) -> Option<Value> {
    let marker: Value =
        serde_json::from_str(&fs::read_to_string(model_marker_file(p)).ok()?).ok()?;
    if marker.get("owner").and_then(Value::as_str) != Some(PROFILE)
        || marker.get("source_provider").and_then(Value::as_str) != Some(route.source_id.as_str())
        || marker.get("provider_key").and_then(Value::as_str) != Some(provider_key)
        || marker.get("model").and_then(Value::as_str) != Some(route.model.as_str())
    {
        return None;
    }
    let secret_name = marker.get("secret_basename").and_then(Value::as_str)?;
    if secret_name.contains(['/', '\\']) || !secret_name.starts_with("model-") {
        return None;
    }
    let secret: Value =
        serde_json::from_str(&fs::read_to_string(model_secrets_dir(p).join(secret_name)).ok()?)
            .ok()?;
    (secret.get("api_key").and_then(Value::as_str) == Some(route.key.as_str())).then_some(marker)
}

fn model_owned_marker(p: &Paths) -> Option<Value> {
    fs::read_to_string(model_marker_file(p))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|marker| marker.get("owner").and_then(Value::as_str) == Some(PROFILE))
}

fn model_candidate_config(
    p: &Paths,
    route: &ModelRoute,
    provider_key: &str,
    secret_file: &Path,
) -> Result<Vec<u8>, String> {
    // The marker is the capability that lets this adapter replace *its own*
    // prior generation.  It is intentionally not scoped to the new key: an
    // endpoint/provider switch derives a different key but must still remove
    // the old private slot atomically.  No marker means every occupied slot
    // belongs to somebody else and is therefore untouchable.
    let owned_marker = model_owned_marker(p);
    let old_provider_key = owned_marker
        .as_ref()
        .and_then(|m| m.get("provider_key").and_then(Value::as_str))
        .map(str::to_owned);
    let old_model = owned_marker
        .as_ref()
        .and_then(|m| m.get("model").and_then(Value::as_str))
        .map(str::to_owned);
    let old_ref = old_provider_key
        .as_deref()
        .zip(old_model.as_deref())
        .map(|(key, model)| model_ref(key, model));
    let mut config: Value = serde_json::from_slice(
        &fs::read(config_file(p)).map_err(|_| "not_ready: OpenClaw2 私有配置不可读")?,
    )
    .map_err(|_| "not_ready: OpenClaw2 私有配置已损坏")?;
    let root = config
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 私有配置形状无效")?;
    let models = root
        .entry("models")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 models 形状无效")?;
    if !models.contains_key("mode") {
        models.insert("mode".into(), json!("merge"));
    }
    let providers = models
        .entry("providers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 models.providers 形状无效")?;
    if providers.contains_key(provider_key) && old_provider_key.as_deref() != Some(provider_key) {
        return Err(
            "validation_failed: OpenClaw2 同名 model provider 不属于本适配器，拒绝覆盖".into(),
        );
    }
    if let Some(old) = old_provider_key
        .as_deref()
        .filter(|old| *old != provider_key)
    {
        providers.remove(old);
    }
    // OpenClaw 2026.8.1's `json` file provider addresses values with an
    // absolute JSON Pointer. A bare key is rejected before inference.
    providers.insert(provider_key.into(), json!({"baseUrl":route.base,"api":"openai-completions","apiKey":{"source":"file","provider":MODEL_SECRET_PROVIDER,"id":"/api_key"},"models":[{"id":route.model,"name":route.model}]}));
    // `models.primary` was an early adapter mistake.  Remove only the exact
    // value that our own marker proves we wrote; preserve any foreign value.
    if models.get("primary").and_then(Value::as_str) == old_ref.as_deref() {
        models.remove("primary");
    }
    let agents = root
        .entry("agents")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 agents 形状无效")?;
    let defaults = agents
        .entry("defaults")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 agents.defaults 形状无效")?;
    let default_models = defaults
        .entry("models")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 agents.defaults.models 形状无效")?;
    if let Some(old) = old_ref.as_deref() {
        default_models.remove(old);
    }
    let reference = model_ref(provider_key, &route.model);
    default_models.insert(reference.clone(), json!({}));
    let default_model = defaults
        .entry("model")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 agents.defaults.model 形状无效")?;
    default_model.insert("primary".into(), json!(reference));
    let secrets = root
        .entry("secrets")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 secrets 形状无效")?;
    let secret_providers = secrets
        .entry("providers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("not_ready: OpenClaw2 secrets.providers 形状无效")?;
    if secret_providers.contains_key(MODEL_SECRET_PROVIDER) && owned_marker.is_none() {
        return Err(
            "validation_failed: OpenClaw2 file secret provider 不属于本适配器，拒绝覆盖".into(),
        );
    }
    secret_providers.insert(
        MODEL_SECRET_PROVIDER.into(),
        json!({"source":"file","path":secret_file,"mode":"json"}),
    );
    serde_json::to_vec_pretty(&config)
        .map_err(|_| "validation_failed: 无法序列化 OpenClaw2 model 配置".into())
}

fn run_oc_transaction(
    p: &Paths,
    candidate: &Path,
    txn_state: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Capture, String> {
    let node = node_exe(p);
    let cli = cli_file(p);
    let mut all = vec![
        cli.to_string_lossy().to_string(),
        "--profile".into(),
        PROFILE.into(),
    ];
    all.extend(args.iter().map(|arg| (*arg).into()));
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    let mut env = managed_env(p);
    env.retain(|(key, _)| {
        key != "OPENCLAW_CONFIG_PATH" && key != "OPENCLAW_STATE_DIR" && key != "OPENCLAW_AGENT_DIR"
    });
    env.push((
        "OPENCLAW_CONFIG_PATH".into(),
        candidate.to_string_lossy().to_string(),
    ));
    env.push((
        "OPENCLAW_STATE_DIR".into(),
        txn_state.to_string_lossy().to_string(),
    ));
    env.push((
        "OPENCLAW_AGENT_DIR".into(),
        txn_state.join("agents").to_string_lossy().to_string(),
    ));
    let refs_env: Vec<(&str, &str)> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    run_capture(&node, &refs, &refs_env, &p.workspace, timeout)
}

/// CLI stderr can contain a rendered config value.  Keep only a stable,
/// non-secret category and exit condition in Action errors; callers can tell
/// a schema/pointer mistake from a missing command without receiving a key,
/// URL, candidate path, or upstream response body.
fn config_diagnostic(phase: &str, out: &Capture) -> String {
    let text = format!("{}\n{}", out.stdout, out.stderr).to_ascii_lowercase();
    let category = if out.status.is_none() {
        "timeout"
    } else if text.contains("json pointer") || (text.contains("secret") && text.contains("pointer"))
    {
        "secret_ref_pointer"
    } else if text.contains("secret") {
        "secret_ref"
    } else if text.contains("schema")
        || text.contains("invalid config")
        || text.contains("validation")
    {
        "schema"
    } else if text.contains("unknown command")
        || text.contains("not found")
        || text.contains("usage:")
    {
        "unsupported_command"
    } else {
        "command_failed"
    };
    format!(
        "validation_failed: {phase} (exit={}, diagnostic={category})",
        out.status
            .map(|code| code.to_string())
            .unwrap_or_else(|| "timeout".into())
    )
}

#[derive(Clone)]
struct FileSnapshot {
    bytes: Option<Vec<u8>>,
    modified: Option<SystemTime>,
}

fn snapshot_file(path: &Path) -> FileSnapshot {
    FileSnapshot {
        bytes: fs::read(path).ok(),
        modified: fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok()),
    }
}

fn restore_file_snapshot(p: &Paths, path: &Path, snapshot: &FileSnapshot) -> Result<(), String> {
    match &snapshot.bytes {
        Some(bytes) => {
            atomic_write(path, bytes, p)?;
            if let Some(modified) = snapshot.modified {
                restore_file_mtime(path, modified)?;
            }
        }
        None if path.exists() => {
            fs::remove_file(path).map_err(|_| "无法移除失败的 OpenClaw2 model 文件")?
        }
        None => {}
    }
    Ok(())
}

#[cfg(windows)]
fn restore_file_mtime(path: &Path, modified: SystemTime) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetFileTime(
            file: isize,
            creation: *const FileTime,
            access: *const FileTime,
            write: *const FileTime,
        ) -> i32;
    }
    let ticks = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "无法恢复 OpenClaw2 model 文件时间")?
        .as_nanos()
        / 100
        + 116_444_736_000_000_000u128;
    let time = FileTime {
        low: ticks as u32,
        high: (ticks >> 32) as u32,
    };
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| "无法恢复 OpenClaw2 model 文件时间")?;
    if unsafe {
        SetFileTime(
            file.as_raw_handle() as isize,
            std::ptr::null(),
            std::ptr::null(),
            &time,
        )
    } == 0
    {
        Err("无法恢复 OpenClaw2 model 文件时间".into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn restore_file_mtime(_: &Path, _: SystemTime) -> Result<(), String> {
    Ok(())
}

fn rollback_model_config(
    p: &Paths,
    old_config: &FileSnapshot,
    old_marker: &FileSnapshot,
    new_secret: &Path,
    txn: &Path,
) -> Result<(), String> {
    restore_file_snapshot(p, &config_file(p), old_config)?;
    restore_file_snapshot(p, &model_marker_file(p), old_marker)?;
    if new_secret.exists() {
        fs::remove_file(new_secret).map_err(|_| "无法清理失败的 model secret")?;
    }
    if txn.exists() {
        fs::remove_dir_all(txn).map_err(|_| "无法清理失败的 model transaction")?;
    }
    Ok(())
}

pub fn configure_model(route: ModelRoute) -> Result<Value, String> {
    let p = paths();
    configure_model_at_with_probe(&p, route, true, true)
}

/// Commit a validated configuration without an inference request. A newly
/// bound device wallet legitimately starts at zero balance, so normal setup
/// must not turn a paid model invocation into an implicit activation gate.
pub fn configure_model_without_probe(route: ModelRoute) -> Result<Value, String> {
    let p = paths();
    configure_model_at_with_probe(&p, route, true, false)
}

fn configure_model_at(
    p: &Paths,
    route: ModelRoute,
    require_runtime_ready: bool,
) -> Result<Value, String> {
    configure_model_at_with_probe(p, route, require_runtime_ready, true)
}

fn configure_model_at_with_probe(
    p: &Paths,
    route: ModelRoute,
    require_runtime_ready: bool,
    run_model_probe: bool,
) -> Result<Value, String> {
    let _guard = model_mutex()
        .lock()
        .map_err(|_| "not_ready: OpenClaw2 model 配置锁不可用")?;
    let running = if require_runtime_ready {
        let report = inspect()?;
        if report.get("installed").and_then(Value::as_bool) != Some(true)
            || report
                .get("runtime")
                .and_then(|x| x.get("integrity_ok"))
                .and_then(Value::as_bool)
                != Some(true)
            || report.get("prepared").and_then(Value::as_bool) != Some(true)
        {
            return Err("not_ready: OpenClaw2 私有 runtime 尚未安装、校验或准备完成".into());
        }
        report
            .get("running")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    } else {
        false
    };
    let base = normalized_model_base(&route.base)?;
    if route.model.trim().is_empty()
        || route.model.len() > 256
        || route.model.bytes().any(|b| b <= b' ')
    {
        return Err("invalid_input: OpenClaw2 model 无效".into());
    }
    if route.key.trim().is_empty() {
        return Err("invalid_input: OpenClaw2 API Key 不可为空".into());
    }
    let route = ModelRoute {
        base,
        model: route.model.trim().into(),
        ..route
    };
    let provider_key = model_provider_key(&route.source_id, &route.base);
    if let Some(marker) = model_marker_matches(&p, &route, &provider_key) {
        return Ok(
            json!({"changed":false,"configured":true,"ready":true,"provider":{"id":route.source_id,"name":route.source_name,"key_source":route.key_source},"model":{"id":route.model,"ref":model_ref(&provider_key,&route.model)},"validation":{"ran":false,"ok":true},"probe":marker.get("probe").cloned().unwrap_or_else(|| json!({"ran":false,"ok":false})),"restart_required":running,"state_version":state_version()}),
        );
    }
    let nonce = random_token()?;
    let txn = model_txn_root(&p).join(&nonce);
    let candidate = txn.join("openclaw.json");
    let txn_state = txn.join("state");
    ensure_private_path(&txn, &p)?;
    fs::create_dir_all(&txn_state)
        .map_err(|_| "validation_failed: 无法创建 OpenClaw2 model transaction")?;
    let old_config = snapshot_file(&config_file(&p));
    if old_config.bytes.is_none() {
        return Err("not_ready: OpenClaw2 私有配置不可读".into());
    }
    let old_marker = snapshot_file(&model_marker_file(&p));
    let old_profile = snapshot_file(&profile_file(&p));
    let secret = model_secret_file(&p, &nonce);
    let result = (|| -> Result<Value, String> {
        atomic_write(
            &secret,
            serde_json::to_vec(&json!({"api_key":route.key}))
                .unwrap()
                .as_slice(),
            &p,
        )?;
        let candidate_bytes = model_candidate_config(&p, &route, &provider_key, &secret)?;
        atomic_write(&candidate, &candidate_bytes, &p)?;
        // A no-probe setup deliberately makes no inference request: a newly
        // issued wallet may have no balance yet.  It only needs the real JSON
        // config validation below.  The infer command capability is relevant
        // only when this flow will actually run the explicit paid probe.
        if run_model_probe {
            for (args, phase) in [
                (
                    ["config", "validate", "--json"].as_slice(),
                    "private CLI config-validate capability check",
                ),
                (
                    ["infer", "model", "run", "--help"].as_slice(),
                    "private CLI infer capability check",
                ),
            ] {
                let out =
                    run_oc_transaction(&p, &candidate, &txn_state, args, Duration::from_secs(20))?;
                if out.status != Some(0) {
                    return Err(config_diagnostic(phase, &out));
                }
            }
        }
        let validation = run_oc_transaction(
            &p,
            &candidate,
            &txn_state,
            &["config", "validate", "--json"],
            Duration::from_secs(30),
        )?;
        if validation.status != Some(0) {
            return Err(config_diagnostic(
                "candidate config validation",
                &validation,
            ));
        }
        if serde_json::from_str::<Value>(&validation.stdout).is_err() {
            return Err("validation_failed: candidate config validation returned non-JSON stdout (exit=0, diagnostic=non_json_stdout)".into());
        }
        let reference = model_ref(&provider_key, &route.model);
        let probe_view = if run_model_probe {
            let began = Instant::now();
            let probe = run_oc_transaction(
                &p,
                &candidate,
                &txn_state,
                &[
                    "infer",
                    "model",
                    "run",
                    "--local",
                    "--model",
                    &reference,
                    "--prompt",
                    "Reply exactly: openclaw2-probe-ok",
                    "--json",
                ],
                Duration::from_secs(90),
            )?;
            if probe.status != Some(0) {
                return Err(config_diagnostic("model probe command", &probe).replacen(
                    "validation_failed:",
                    "probe_failed:",
                    1,
                ));
            }
            if serde_json::from_str::<Value>(&probe.stdout).is_err()
                || !probe.stdout.contains("openclaw2-probe-ok")
            {
                return Err(
                    "probe_failed: OpenClaw2 最窄模型探针返回无效 JSON 或未确认固定响应".into(),
                );
            }
            json!({"ran":true,"ok":true,"latency_ms":began.elapsed().as_millis() as u64})
        } else {
            json!({"ran":false,"ok":false,"reason":"explicit_probe_required"})
        };
        atomic_write(&config_file(&p), &candidate_bytes, &p)?;
        if model_test_fault(&p, "live_commit") {
            return Err("validation_failed: OpenClaw2 live 配置提交注入失败".into());
        }
        if fs::read(config_file(&p)).map_err(|_| "rollback_failed: OpenClaw2 live 配置回读失败")?
            != candidate_bytes
        {
            return Err("rollback_failed: OpenClaw2 live 配置回读不一致".into());
        }
        let marker = json!({"schema_version":1,"owner":PROFILE,"source_provider":route.source_id,"key_source":route.key_source,"provider_key":provider_key,"model":route.model,"secret_basename":secret.file_name().and_then(|x| x.to_str()).unwrap_or(""),"config_hash":crate::installer::sha256_hex_bytes(&candidate_bytes),"probe":probe_view});
        if model_test_fault(&p, "marker_commit") {
            return Err("validation_failed: OpenClaw2 model marker 提交注入失败".into());
        }
        atomic_write(
            &model_marker_file(&p),
            serde_json::to_vec_pretty(&marker).unwrap().as_slice(),
            &p,
        )?;
        refresh_managed_profile_hash(&p, &candidate_bytes)?;
        if let Some(old) = old_marker
            .bytes
            .as_ref()
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .and_then(|m| {
                m.get("secret_basename")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
        {
            let old = model_secrets_dir(&p).join(old);
            if old != secret
                && old
                    .file_name()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.starts_with("model-"))
            {
                let _ = fs::remove_file(old);
            }
        }
        let _ = fs::remove_dir_all(&txn);
        Ok(
            json!({"changed":true,"configured":true,"ready":true,"provider":{"id":route.source_id,"name":route.source_name,"key_source":route.key_source},"model":{"id":route.model,"ref":reference},"validation":{"ran":true,"ok":true},"probe":probe_view,"restart_required":running,"state_version":state_version()}),
        )
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => match rollback_model_config(&p, &old_config, &old_marker, &secret, &txn)
            .and_then(|_| restore_file_snapshot(&p, &profile_file(&p), &old_profile))
        {
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
                let status = gateway_status(&ps, port).unwrap_or_else(
                    |_| json!({"ok":false,"rpcOk":false,"degraded":false,"status_error":true}),
                );
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
    let mut env = vec![
        ("OPENCLAW_PROFILE".into(), PROFILE.into()),
        (
            "OPENCLAW_CONFIG_PATH".into(),
            config_file(p).to_string_lossy().to_string(),
        ),
        (
            "OPENCLAW_STATE_DIR".into(),
            p.state.to_string_lossy().to_string(),
        ),
        (
            "OPENCLAW_AGENT_DIR".into(),
            p.state.join("agents").to_string_lossy().to_string(),
        ),
        ("OPENCLAW_SUPERVISOR_MODE".into(), "external".into()),
        ("OPENCLAW_SERVICE_REPAIR_POLICY".into(), "external".into()),
        ("OPENCLAW_DISABLE_BONJOUR".into(), "1".into()),
        ("NO_COLOR".into(), "1".into()),
    ];
    if crate::portable_context::current().is_some() {
        // The packaged workspace bundle selects its audited fs-safe sidecar
        // only under this explicit production marker. Desktop OpenClaw keeps
        // the upstream strict policy and never loads the sidecar.
        env.push(("UKING_PORTABLE_COMPAT_EXFAT".into(), "1".into()));
        let home = p.root.join("home");
        let tmp = p.root.join("tmp");
        let cache = p.root.join("cache");
        for (key, value) in [
            ("OPENCLAW_HOME", p.root.clone()),
            ("HOME", home.clone()),
            ("USERPROFILE", home.clone()),
            ("APPDATA", home.join("AppData/Roaming")),
            ("LOCALAPPDATA", home.join("AppData/Local")),
            ("TEMP", tmp.clone()),
            ("TMP", tmp.clone()),
            ("TMPDIR", tmp),
            ("npm_config_cache", cache),
        ] {
            env.push((key.into(), value.to_string_lossy().to_string()));
        }
    }
    env
}

/// The portable gateway is an appliance.  Inheriting a host shell's provider
/// credentials lets OpenClaw discover plugins that were never configured in
/// this package (and can make their consent check reject a fresh profile).
/// Keep only the Windows process basics and opt-in proxy routing; all
/// OpenClaw, Node and model-provider settings come from `managed_env` below.
fn configure_managed_openclaw_child(
    command: &mut Command,
    env: &[(String, String)],
    portable: bool,
) {
    if portable {
        command.env_clear();
        for key in [
            "SystemRoot",
            "WINDIR",
            "COMSPEC",
            "PATH",
            "PATHEXT",
            "SYSTEMDRIVE",
            "OS",
            "PROCESSOR_ARCHITECTURE",
            "PROCESSOR_ARCHITEW6432",
            "NUMBER_OF_PROCESSORS",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    } else {
        // Desktop OpenClaw intentionally retains its historical host
        // environment and profile behavior.
        command.env_remove("OPENCLAW_HOME");
    }
    for (key, value) in env {
        command.env(key, value);
    }
}

fn is_managed_openclaw_env(env: &[(&str, &str)]) -> bool {
    env.iter().any(|(key, _)| *key == "OPENCLAW_PROFILE")
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
    let mut status = inspect()?;
    if status.get("installed").and_then(Value::as_bool) != Some(true) {
        return Err("OpenClaw2 未安装或 runtime 校验未通过".into());
    }
    // A marked package may have been moved while closed. Re-enter the same
    // prepare core so the verified relocation path runs before launch; desktop
    // profiles keep their previous explicit-prepare behavior.
    if status.get("prepared").and_then(Value::as_bool) != Some(true)
        && crate::portable_context::current().is_some() {
        prepare(None)?;
        status = inspect()?;
    }
    if status.get("prepared").and_then(Value::as_bool) != Some(true) {
        return Err("OpenClaw2 尚未准备私有 profile".into());
    }
    let port = parse_profile(&ps)?.ok_or("OpenClaw2 profile 缺少端口")?;
    launch_private_gateway(&ps, port)
}

/// Stops only the process proved by the marker, executable, command line and
/// creation-time identity. Never select a process by the generic node name.
pub fn stop() -> Result<Value, String> {
    let ps = paths();
    let (port, pid, owned) = supervisor_status(&ps)?;
    let (port, pid) = match (port, pid, owned) {
        (Some(port), Some(pid), true) => (port, pid),
        (_, Some(_), false) => {
            return Err("OpenClaw2 supervisor 不能证明该进程属于此包，拒绝停止".into())
        }
        _ => return Ok(json!({"changed":false,"stopped":true,"state_version":state_version()})),
    };
    #[cfg(windows)]
    {
        let pid_text = pid.to_string();
        let out = run_capture(
            Path::new("taskkill.exe"),
            &["/PID", &pid_text, "/F", "/T"],
            &[],
            &ps.workspace,
            Duration::from_secs(10),
        ).map_err(|_| "停止受管 OpenClaw2 Gateway 失败".to_string())?;
        if out.status != Some(0) && process_identity(pid).is_some() {
            return Err("停止受管 OpenClaw2 Gateway 失败".into());
        }
    }
    #[cfg(not(windows))]
    {
        return Err("OpenClaw2 便携预览当前仅支持 Windows x64".into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_identity(pid).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    if process_identity(pid).is_some() || port_listening(port) {
        return Err("受管 OpenClaw2 Gateway 未在期限内停止".into());
    }
    fs::remove_file(supervisor_file(&ps)).map_err(|e| format!("清理受管 Gateway 状态失败: {e}"))?;
    Ok(json!({"changed":true,"stopped":true,"state_version":state_version()}))
}

/// Verify the owned gateway and construct its private dashboard URL. The URL
/// itself never crosses an Action boundary: it contains the gateway token and
/// is handed only to the operating-system browser opener.
pub fn dashboard_target() -> Result<String, String> {
    let ps = paths();
    let (port, _pid, owned) = supervisor_status(&ps)?;
    let port = port.ok_or("OpenClaw2 尚未启动")?;
    if !owned || !port_listening(port) {
        return Err("OpenClaw2 Gateway 未由此便携包运行，拒绝打开面板".into());
    }
    let config: Value = serde_json::from_slice(
        &fs::read(config_file(&ps)).map_err(|_| "OpenClaw2 私有配置不可读")?,
    )
    .map_err(|_| "OpenClaw2 私有配置已损坏")?;
    let token = config
        .pointer("/gateway/auth/token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or("OpenClaw2 私有 gateway token 不存在")?;
    Ok(format!("http://127.0.0.1:{port}/#token={token}"))
}

#[cfg(windows)]
pub(crate) fn open_system_browser(url: &str) -> Result<(), String> {
    use std::iter::once;
    let operation: Vec<u16> = "open".encode_utf16().chain(once(0)).collect();
    let target: Vec<u16> = url.encode_utf16().chain(once(0)).collect();
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    if unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    } <= 32
    {
        Err("打开 OpenClaw 面板的系统浏览器失败".into())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub(crate) fn open_system_browser(url: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|_| "打开 OpenClaw 面板的系统浏览器失败".into())
}
fn open_verified_dashboard_target(
    target: &str,
    opener: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Value, String> {
    opener(target)?;
    Ok(json!({"changed":false,"opened":true,"state_version":state_version()}))
}
fn open_dashboard_with(opener: impl FnOnce(&str) -> Result<(), String>) -> Result<Value, String> {
    let target = dashboard_target()?;
    open_verified_dashboard_target(&target, opener)
}
/// Action core shared by GUI, CLI and MCP. It returns no token-bearing URL.
pub fn open_dashboard() -> Result<Value, String> {
    open_dashboard_with(open_system_browser)
}

/// A wallet change may update only the secret that this adapter proved it
/// created from the device wallet. It never discovers or touches host tools.
pub fn sync_device_wallet_key(key: Option<&str>) -> Result<(), String> {
    sync_device_wallet_key_at(&paths(), key)
}

fn sync_device_wallet_key_at(p: &Paths, key: Option<&str>) -> Result<(), String> {
    let Some(marker) = model_owned_marker(&p) else {
        return Ok(());
    };
    if marker.get("key_source").and_then(Value::as_str) != Some("device_wallet") {
        return Ok(());
    }
    let name = marker
        .get("secret_basename")
        .and_then(Value::as_str)
        .filter(|name| !name.contains(['/', '\\']) && name.starts_with("model-"))
        .ok_or("OpenClaw2 受管 model marker 缺少安全 secret 名称")?;
    let secret = model_secrets_dir(&p).join(name);
    match key {
        Some(key) if !key.trim().is_empty() => {
            // The marker is only a historical assertion. Re-check the live
            // config generation before replacing a file secret, otherwise a
            // wallet balance refresh could overwrite a user-edited route.
            verify_device_wallet_generation(p, &marker, &secret)?;
            atomic_write(
                &secret,
                serde_json::to_vec(&json!({"api_key":key}))
                    .unwrap()
                    .as_slice(),
                p,
            )
        }
        _ => clear_device_wallet_model(&p, &marker, name, &secret),
    }
}

/// Prove that a marker, live config and file secret are still the exact
/// device-wallet generation that this adapter created. Both update and clear
/// paths use it; a stale marker must never authorize either mutation.
fn verify_device_wallet_generation(
    p: &Paths,
    marker: &Value,
    secret: &Path,
) -> Result<(String, String), String> {
    let bytes = fs::read(config_file(p))
        .map_err(|_| "OpenClaw2 私有配置不可读，拒绝修改钱包 consumer")?;
    let hash = crate::installer::sha256_hex_bytes(&bytes);
    let provider_key = marker
        .get("provider_key")
        .and_then(Value::as_str)
        .ok_or("OpenClaw2 受管 model marker 缺少 provider，拒绝修改钱包 consumer")?;
    let model = marker
        .get("model")
        .and_then(Value::as_str)
        .ok_or("OpenClaw2 受管 model marker 缺少 model，拒绝修改钱包 consumer")?;
    if marker.get("owner").and_then(Value::as_str) != Some(PROFILE)
        || marker.get("config_hash").and_then(Value::as_str) != Some(hash.as_str())
        || !secret.is_file()
    {
        return Err("OpenClaw2 受管 model marker 与当前配置不一致，拒绝修改钱包 consumer".into());
    }
    let config: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "OpenClaw2 私有配置已损坏，拒绝修改钱包 consumer")?;
    let provider = config
        .pointer(&format!("/models/providers/{provider_key}"))
        .ok_or("OpenClaw2 当前配置缺少受管 model provider，拒绝修改钱包 consumer")?;
    let file_provider = config
        .pointer(&format!("/secrets/providers/{MODEL_SECRET_PROVIDER}"))
        .ok_or("OpenClaw2 当前配置缺少受管 file secret provider，拒绝修改钱包 consumer")?;
    if provider.pointer("/apiKey/source").and_then(Value::as_str) != Some("file")
        || provider.pointer("/apiKey/provider").and_then(Value::as_str)
            != Some(MODEL_SECRET_PROVIDER)
        || provider.pointer("/apiKey/id").and_then(Value::as_str) != Some("/api_key")
        || file_provider.get("source").and_then(Value::as_str) != Some("file")
        || file_provider.get("mode").and_then(Value::as_str) != Some("json")
        || file_provider.get("path").and_then(Value::as_str)
            != Some(secret.to_string_lossy().as_ref())
    {
        return Err("OpenClaw2 当前配置不是受管 file secret consumer，拒绝修改".into());
    }
    let reference = model_ref(provider_key, model);
    if config
        .pointer("/agents/defaults/model/primary")
        .and_then(Value::as_str)
        != Some(reference.as_str())
    {
        return Err("OpenClaw2 当前默认模型不是受管钱包 consumer，拒绝修改".into());
    }
    Ok((provider_key.to_owned(), model.to_owned()))
}

/// Removing a device wallet must remove the adapter-owned consumer before the
/// wallet core commits its reset.  A marker alone is insufficient authority:
/// require that the live config, provider and file secret still describe the
/// exact generation the marker recorded.  A running owned gateway is stopped
/// first, so it cannot retain the credential in process memory after success.
fn clear_device_wallet_model(
    p: &Paths,
    marker: &Value,
    _name: &str,
    secret: &Path,
) -> Result<(), String> {
    let (provider_key, model) = verify_device_wallet_generation(p, marker, secret)?;
    let old_config = snapshot_file(&config_file(p));
    let old_marker = snapshot_file(&model_marker_file(p));
    let old_secret = snapshot_file(secret);
    let old_profile = snapshot_file(&profile_file(p));
    let bytes = old_config
        .bytes
        .as_ref()
        .ok_or("OpenClaw2 私有配置不可读，拒绝移除钱包 consumer")?;
    let mut config: Value = serde_json::from_slice(bytes)
        .map_err(|_| "OpenClaw2 私有配置已损坏，拒绝移除钱包 consumer")?;
    let reference = model_ref(&provider_key, &model);
    let (port, _pid, owned) = supervisor_status(p)?;
    if port.is_some_and(port_listening) && !owned {
        return Err("OpenClaw2 Gateway 归属无法证明，拒绝移除钱包 consumer".into());
    }
    if owned {
        stop()?;
    }
    let root = config
        .as_object_mut()
        .ok_or("OpenClaw2 私有配置形状无效，拒绝移除钱包 consumer")?;
    if let Some(providers) = root
        .get_mut("models")
        .and_then(|x| x.get_mut("providers"))
        .and_then(Value::as_object_mut)
    {
        providers.remove(&provider_key);
    }
    if let Some(models) = root
        .get_mut("agents")
        .and_then(|x| x.get_mut("defaults"))
        .and_then(|x| x.get_mut("models"))
        .and_then(Value::as_object_mut)
    {
        models.remove(&reference);
    }
    if let Some(model_slot) = root
        .get_mut("agents")
        .and_then(|x| x.get_mut("defaults"))
        .and_then(|x| x.get_mut("model"))
        .and_then(Value::as_object_mut)
    {
        model_slot.remove("primary");
    }
    if let Some(providers) = root
        .get_mut("secrets")
        .and_then(|x| x.get_mut("providers"))
        .and_then(Value::as_object_mut)
    {
        providers.remove(MODEL_SECRET_PROVIDER);
    }
    let candidate = serde_json::to_vec_pretty(&config)
        .map_err(|_| "无法序列化 OpenClaw2 钱包 consumer 清除配置")?;
    let result = (|| -> Result<(), String> {
        atomic_write(&config_file(p), &candidate, p)?;
        fs::remove_file(model_marker_file(p))
            .map_err(|_| "无法移除 OpenClaw2 受管 model marker")?;
        fs::remove_file(secret).map_err(|_| "无法移除 OpenClaw2 受管 file secret")?;
        if model_test_fault(p, "wallet_clear_profile") {
            return Err("validation_failed: OpenClaw2 钱包 consumer 清除注入失败".into());
        }
        refresh_managed_profile_hash(p, &candidate)
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => restore_file_snapshot(p, &config_file(p), &old_config)
            .and_then(|_| restore_file_snapshot(p, &model_marker_file(p), &old_marker))
            .and_then(|_| restore_file_snapshot(p, secret, &old_secret))
            .and_then(|_| restore_file_snapshot(p, &profile_file(p), &old_profile))
            .map_err(|_| "rollback_failed: OpenClaw2 钱包 consumer 清除失败且回滚未完成".into())
            .and_then(|_| Err(error)),
    }
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
        .stderr(Stdio::null());
    configure_managed_openclaw_child(
        &mut cmd,
        &managed_env(ps),
        crate::portable_context::current().is_some(),
    );
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
            if found.is_none() {
                std::thread::sleep(Duration::from_millis(100));
            }
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

fn write_supervisor_or_kill(
    child: &mut std::process::Child,
    p: &Paths,
    marker: &Value,
) -> Result<(), String> {
    if let Err(e) = atomic_write(
        &supervisor_file(p),
        serde_json::to_string_pretty(marker).unwrap().as_bytes(),
        p,
    ) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "写入 OpenClaw2 supervisor 状态失败，已终止刚启动进程: {e}"
        ));
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
    let Some(identity) = process_identity(pid) else {
        return false;
    };
    identity_matches(p, port, marker, &identity)
}

fn identity_matches(p: &Paths, port: u16, marker: &Value, identity: &ProcessIdentity) -> bool {
    let expected_node = node_exe(p).canonicalize().unwrap_or_else(|_| node_exe(p));
    let expected_cli = cli_file(p).canonicalize().unwrap_or_else(|_| cli_file(p));
    let expected_state = p.state.canonicalize().unwrap_or_else(|_| p.state.clone());
    let marker_state = marker.get("state_dir").and_then(Value::as_str);
    let marker_started = marker.get("process_started").and_then(Value::as_str);
    let image = PathBuf::from(&identity.image)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&identity.image));
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
    )
    .ok()?;
    (out.status == Some(0))
        .then(|| serde_json::from_str(&out.stdout).ok())
        .flatten()
}

#[cfg(not(windows))]
fn process_identity(_: u32) -> Option<ProcessIdentity> {
    None
}

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

const CAPTURE_STREAM_MAX_BYTES: usize = 512 * 1024;

fn drain_capture_pipe<R: Read + Send + 'static>(
    mut pipe: R,
    buffer: Arc<Mutex<Vec<u8>>>,
    complete: Arc<AtomicUsize>,
) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut captured) = buffer.lock() {
                        let remaining = CAPTURE_STREAM_MAX_BYTES.saturating_sub(captured.len());
                        captured.extend_from_slice(&chunk[..read.min(remaining)]);
                    }
                    // Keep reading after the bounded buffer is full.  Draining
                    // is what prevents a verbose OpenClaw child from blocking
                    // on a full Windows pipe.
                }
            }
        }
        complete.fetch_add(1, Ordering::Release);
    });
}

fn capture_text(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    buffer
        .lock()
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
        .unwrap_or_default()
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
    let env: Vec<(&str, &str)> = owned_env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
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
        .and_then(|config| {
            config
                .pointer("/gateway/auth/token")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}
/// Gateway status is Action output, so it must never carry credentials even
/// when an upstream version puts them into a deeply nested diagnostics object.
fn redact_gateway_json(value: &mut Value, private_token: Option<&str>) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object.iter_mut() {
                let lower = key.to_ascii_lowercase();
                if [
                    "token",
                    "secret",
                    "password",
                    "authorization",
                    "cookie",
                    "credential",
                    "api_key",
                    "apikey",
                ]
                .iter()
                .any(|needle| lower.contains(needle))
                {
                    *nested = Value::String("[redacted]".into());
                } else {
                    redact_gateway_json(nested, private_token);
                }
            }
        }
        Value::Array(items) => {
            for nested in items {
                redact_gateway_json(nested, private_token);
            }
        }
        Value::String(text) => {
            if let Some(token) = private_token.filter(|token| !token.is_empty()) {
                if text.contains(token) {
                    *text = text.replace(token, "[redacted]");
                }
            }
        }
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
        .stderr(Stdio::piped());
    if is_managed_openclaw_env(env) {
        let owned: Vec<(String, String)> = env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        configure_managed_openclaw_child(
            &mut c,
            &owned,
            crate::portable_context::current().is_some(),
        );
    } else {
        c.env_remove("OPENCLAW_HOME");
        for (k, v) in env {
            c.env(k, v);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let mut child = c
        .spawn()
        .map_err(|e| format!("启动 OpenClaw2 子进程失败: {e}"))?;
    let pid = child.id();
    let stdout = Arc::new(Mutex::new(Vec::new()));
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let readers_complete = Arc::new(AtomicUsize::new(0));
    if let Some(pipe) = child.stdout.take() {
        drain_capture_pipe(pipe, stdout.clone(), readers_complete.clone());
    } else {
        readers_complete.fetch_add(1, Ordering::Release);
    }
    if let Some(pipe) = child.stderr.take() {
        drain_capture_pipe(pipe, stderr.clone(), readers_complete.clone());
    } else {
        readers_complete.fetch_add(1, Ordering::Release);
    }
    let begin = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status.code();
        }
        if begin.elapsed() >= timeout {
            // A CLI wrapper can leave Node grandchildren holding stdout or
            // stderr.  Kill the whole tree before returning; never call
            // wait_with_output here because an inherited pipe can keep it
            // blocked after the direct child has exited.
            crate::agent::chat::kill_tree_by_pid(pid);
            let _ = child.kill();
            let reap_deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < reap_deadline {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // Reader threads may still be draining bytes that were already written
    // when `try_wait` observed exit.  Give them a short bounded grace period;
    // do not join forever if a leaked descendant inherited a pipe handle.
    let drain_deadline = Instant::now() + Duration::from_secs(2);
    while readers_complete.load(Ordering::Acquire) < 2 && Instant::now() < drain_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(Capture {
        status,
        stdout: capture_text(&stdout),
        stderr: capture_text(&stderr),
    })
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
        Err(format!(
            "{what}失败: {}",
            redact_tail(if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            })
        ))
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
pub fn action_stop(_: &str, _: Value, _: &crate::actions::ProgressSink) -> Result<Value, String> {
    stop()
}
pub fn action_open_dashboard(
    _: &str,
    _: Value,
    _: &crate::actions::ProgressSink,
) -> Result<Value, String> {
    open_dashboard()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_runtime_manifest() -> RuntimeManifest {
        RuntimeManifest {
            schema_version: 1,
            openclaw_version: "2026.8.1-test".into(),
            node: RuntimeNode {
                version: "24.15.0".into(),
                windows_x64_url: "https://example.invalid/node.zip".into(),
                windows_x64_sha256: crate::installer::sha256_hex_bytes(b"node-archive"),
            },
            openclaw: RuntimeOpenClaw {
                tarball_url: "https://example.invalid/openclaw.tgz".into(),
                integrity: "sha512-3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw==".into(),
            },
        }
    }

    /// A complete, deliberately local wallet-consumer generation.  Tests use
    /// this instead of configure_model so clear/rollback coverage never needs
    /// Node, a gateway, or a real wallet credential.
    fn wallet_clear_fixture(tag: &str) -> (Paths, Value, PathBuf) {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-wallet-clear-{tag}-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        let provider_key = "wallet-provider";
        let model = "wallet-demo";
        let reference = model_ref(provider_key, model);
        let secret = model_secrets_dir(&p).join("model-wallet-clear.json");
        let config = json!({
            "gateway": {"mode":"local", "port":37615, "bind":"loopback", "auth":{"mode":"token", "token":"test-gateway-token"}},
            "agents": {"defaults": {
                "workspace": p.workspace,
                "model": {"primary": reference},
                "models": {reference.clone(): {"alias":"wallet managed"}, "other/model": {"must_survive":true}},
                "user_default": "must_survive"
            }},
            "models": {"providers": {
                provider_key.to_string(): {"apiKey":{"source":"file", "provider":MODEL_SECRET_PROVIDER, "id":"/api_key"}, "models":[{"id":model}]},
                "user-provider": {"must_survive":true}
            }},
            "secrets": {"providers": {
                MODEL_SECRET_PROVIDER.to_string(): {"source":"file", "path":secret, "mode":"json"},
                "user-secret": {"must_survive":true}
            }},
            "user": {"must_survive":true}
        });
        let bytes = serde_json::to_vec_pretty(&config).unwrap();
        atomic_write(&config_file(&p), &bytes, &p).unwrap();
        atomic_write(&secret, br#"{"api_key":"sk-test-wallet-only"}"#, &p).unwrap();
        let marker = json!({
            "schema_version":1,
            "owner":PROFILE,
            "key_source":"device_wallet",
            "provider_key":provider_key,
            "model":model,
            "secret_basename":"model-wallet-clear.json",
            "config_hash":crate::installer::sha256_hex_bytes(&bytes)
        });
        atomic_write(
            &model_marker_file(&p),
            &serde_json::to_vec_pretty(&marker).unwrap(),
            &p,
        ).unwrap();
        write_managed_profile(&p, 37615, &bytes).unwrap();
        (p, marker, secret)
    }
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
        assert_eq!(
            argv,
            vec![
                cli_file(&p).to_string_lossy().to_string(),
                "--profile".into(),
                PROFILE.into(),
                "gateway".into(),
                "run".into(),
                "--port".into(),
                "19789".into(),
            ]
        );
        let env = managed_env(&p)
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            env.get("OPENCLAW_SUPERVISOR_MODE").map(String::as_str),
            Some("external")
        );
        assert_eq!(
            env.get("OPENCLAW_SERVICE_REPAIR_POLICY")
                .map(String::as_str),
            Some("external")
        );
        assert_eq!(
            env.get("OPENCLAW_DISABLE_BONJOUR").map(String::as_str),
            Some("1")
        );
        assert!(!env.contains_key("OPENCLAW_HOME"));
    }
    #[cfg(windows)]
    #[test]
    fn npm_integrity_is_checked_against_the_actual_tarball_bytes() {
        let file = std::env::temp_dir().join(format!(
            "uking-openclaw2-integrity-{}.tgz",
            std::process::id()
        ));
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
        assert_eq!(
            serde_json::from_str::<Value>(&out.stdout).unwrap()["ok"],
            true
        );
        assert!(out.stderr.contains("diagnostic-secret"));
        assert!(!out.stdout.contains("diagnostic-secret"));
    }
    #[test]
    fn interrupted_owned_node_staging_is_removed_for_a_safe_retry() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-owned-stage-{}", now_nanos())),
        );
        let m = test_runtime_manifest();
        create_layout(&p).unwrap();
        fs::create_dir_all(node_stage_dir(&p)).unwrap();
        atomic_write(
            &node_stage_marker(&p),
            serde_json::to_vec(&node_install_marker(&m.node.version, "node-stage"))
                .unwrap()
                .as_slice(),
            &p,
        )
        .unwrap();
        fs::write(
            node_stage_dir(&p).join("interrupted-partial.bin"),
            b"partial",
        )
        .unwrap();
        clear_owned_node_stage(&p, &m).unwrap();
        assert!(
            !node_stage_dir(&p).exists(),
            "只清理带本适配器标记的 staging"
        );
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn unknown_nonempty_node_runtime_is_preserved_and_refused() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-unknown-runtime-{}", now_nanos())),
        );
        let m = test_runtime_manifest();
        create_layout(&p).unwrap();
        let foreign = p.node.join("foreign-sentinel.txt");
        fs::create_dir_all(&p.node).unwrap();
        fs::write(&foreign, b"do-not-delete").unwrap();
        assert!(matches!(
            private_node_state(&p, &m),
            PrivateNodeState::Unknown
        ));
        assert_eq!(fs::read(&foreign).unwrap(), b"do-not-delete");
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn complete_runtime_replay_short_circuits_before_any_download() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-replay-{}", now_nanos())),
        );
        let m = test_runtime_manifest();
        create_layout(&p).unwrap();
        fs::write(node_archive_file(&p, &m), b"node-archive").unwrap();
        fs::write(openclaw_archive_file(&p, &m), b"abc").unwrap();
        fs::create_dir_all(cli_file(&p).parent().unwrap()).unwrap();
        fs::write(cli_file(&p), b"// pinned entry").unwrap();
        fs::write(
            p.app.join("node_modules/openclaw/package.json"),
            serde_json::to_vec(&json!({"version":m.openclaw_version})).unwrap(),
        )
        .unwrap();
        assert!(
            install_replay_ready(&p, &m, Some("v24.15.0")),
            "完整固定产物必须走幂等分支，不触网"
        );
        let _ = fs::remove_dir_all(&p.root);
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
    #[test]
    fn relocation_rewrites_only_proved_workspace_and_file_secret_references() {
        let old_root =
            std::env::temp_dir().join(format!("uking-openclaw2-relocate-old-{}", now_nanos()));
        let new_root =
            std::env::temp_dir().join(format!("uking-openclaw2-relocate-new-{}", now_nanos()));
        let old = paths_from_root(old_root.clone());
        create_layout(&old).unwrap();
        let secret_name = "model-relocate.json";
        let config = json!({"gateway":{"mode":"local","port":37601,"bind":"loopback","auth":{"mode":"token","token":"test-token"}},"agents":{"defaults":{"workspace":old.workspace}},"secrets":{"providers":{MODEL_SECRET_PROVIDER:{"source":"file","path":old.state.join("secrets").join(secret_name),"mode":"json"}}},"user":{"must_survive":true}});
        let old_bytes = serde_json::to_vec_pretty(&config).unwrap();
        atomic_write(&config_file(&old), &old_bytes, &old).unwrap();
        atomic_write(
            &model_secrets_dir(&old).join(secret_name),
            br#"{"api_key":"secret"}"#,
            &old,
        )
        .unwrap();
        atomic_write(&model_marker_file(&old), &serde_json::to_vec_pretty(&json!({"schema_version":1,"owner":PROFILE,"secret_basename":secret_name,"config_hash":crate::installer::sha256_hex_bytes(&old_bytes)})).unwrap(), &old).unwrap();
        write_managed_profile(&old, 37601, &old_bytes).unwrap();
        fs::rename(&old_root, &new_root).unwrap();
        let new = paths_from_root(new_root.clone());
        assert!(relocate_managed_portable_config(&new).unwrap());
        let relocated: Value =
            serde_json::from_slice(&fs::read(config_file(&new)).unwrap()).unwrap();
        assert_eq!(
            relocated
                .pointer("/agents/defaults/workspace")
                .and_then(Value::as_str),
            Some(new.workspace.to_string_lossy().as_ref())
        );
        assert_eq!(
            relocated
                .pointer(&format!("/secrets/providers/{MODEL_SECRET_PROVIDER}/path"))
                .and_then(Value::as_str),
            Some(
                new.state
                    .join("secrets")
                    .join(secret_name)
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(relocated["user"]["must_survive"], true);
        let backups = fs::read_dir(&new.state)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("openclaw.json.before-relocation-"))
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1, "每次移动保留唯一且不覆盖的原配置备份");
        assert_eq!(fs::read(&backups[0]).unwrap(), old_bytes);
        let new_bytes = fs::read(config_file(&new)).unwrap();
        let profile: Value =
            serde_json::from_slice(&fs::read(profile_file(&new)).unwrap()).unwrap();
        let marker: Value =
            serde_json::from_slice(&fs::read(model_marker_file(&new)).unwrap()).unwrap();
        assert_eq!(profile["managed_root"], json!(new.root));
        assert_eq!(
            profile["config_hash"],
            json!(crate::installer::sha256_hex_bytes(&new_bytes))
        );
        assert_eq!(
            marker["config_hash"],
            json!(crate::installer::sha256_hex_bytes(&new_bytes))
        );
        let _ = fs::remove_dir_all(new_root);
    }

    #[test]
    fn relocation_profile_fault_restores_all_three_files_and_keeps_prior_backup() {
        let old_root = std::env::temp_dir().join(format!("uking-openclaw2-relocation-rollback-old-{}", now_nanos()));
        let new_root = std::env::temp_dir().join(format!("uking-openclaw2-relocation-rollback-new-{}", now_nanos()));
        let old = paths_from_root(old_root.clone());
        create_layout(&old).unwrap();
        let secret_name = "model-relocation-rollback.json";
        let config = json!({
            "gateway":{"mode":"local","port":37616,"bind":"loopback","auth":{"mode":"token","token":"test-token"}},
            "agents":{"defaults":{"workspace":old.workspace}},
            "secrets":{"providers":{MODEL_SECRET_PROVIDER.to_string():{"source":"file","path":old.state.join("secrets").join(secret_name),"mode":"json"}}},
            "user":{"must_survive":true}
        });
        let old_bytes = serde_json::to_vec_pretty(&config).unwrap();
        atomic_write(&config_file(&old), &old_bytes, &old).unwrap();
        atomic_write(&model_secrets_dir(&old).join(secret_name), br#"{"api_key":"sk-relocation-test"}"#, &old).unwrap();
        atomic_write(&model_marker_file(&old), &serde_json::to_vec_pretty(&json!({"schema_version":1,"owner":PROFILE,"secret_basename":secret_name,"config_hash":crate::installer::sha256_hex_bytes(&old_bytes)})).unwrap(), &old).unwrap();
        write_managed_profile(&old, 37616, &old_bytes).unwrap();
        fs::rename(&old_root, &new_root).unwrap();
        let p = paths_from_root(new_root.clone());
        let prior_backup = p.state.join("openclaw.json.before-relocation-prior-proof.json");
        fs::write(&prior_backup, b"older recovery evidence must survive").unwrap();
        let before = [
            (config_file(&p), snapshot_file(&config_file(&p))),
            (model_marker_file(&p), snapshot_file(&model_marker_file(&p))),
            (profile_file(&p), snapshot_file(&profile_file(&p))),
        ];
        set_model_test_fault(&p, Some("relocation_profile"));
        let error = relocate_managed_portable_config(&p).unwrap_err();
        set_model_test_fault(&p, None);
        assert!(error.starts_with("validation_failed:"), "{error}");
        for (path, snapshot) in before {
            assert_eq!(snapshot_file(&path).bytes, snapshot.bytes, "{path:?} bytes must roll back");
            assert_eq!(snapshot_file(&path).modified, snapshot.modified, "{path:?} mtime must roll back");
        }
        assert_eq!(fs::read(&prior_backup).unwrap(), b"older recovery evidence must survive");
        assert!(fs::read_dir(&p.state).unwrap().flatten().any(|entry| {
            entry.path().file_name().and_then(|name| name.to_str()).is_some_and(|name| {
                name.starts_with("openclaw.json.before-relocation-") && name != "openclaw.json.before-relocation-prior-proof.json"
            })
        }), "本次失败前写下的独立备份仍须保留，不能覆盖既有恢复证据");
        let _ = fs::remove_dir_all(new_root);
    }
    #[test]
    fn relocation_refuses_unproved_old_workspace_without_writing() {
        let old_root =
            std::env::temp_dir().join(format!("uking-openclaw2-refuse-old-{}", now_nanos()));
        let new_root =
            std::env::temp_dir().join(format!("uking-openclaw2-refuse-new-{}", now_nanos()));
        let old = paths_from_root(old_root.clone());
        create_layout(&old).unwrap();
        let bytes = serde_json::to_vec_pretty(&json!({"gateway":{"mode":"local","port":37602,"bind":"loopback","auth":{"mode":"token","token":"test-token"}},"agents":{"defaults":{"workspace":old_root.join("user-workspace")}}})).unwrap();
        atomic_write(&config_file(&old), &bytes, &old).unwrap();
        write_managed_profile(&old, 37602, &bytes).unwrap();
        fs::rename(&old_root, &new_root).unwrap();
        let new = paths_from_root(new_root.clone());
        assert!(relocate_managed_portable_config(&new)
            .unwrap_err()
            .contains("不是受管旧根目录引用"));
        assert_eq!(fs::read(config_file(&new)).unwrap(), bytes);
        assert!(!fs::read_dir(&new.state)
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("openclaw.json.before-relocation-")));
        let _ = fs::remove_dir_all(new_root);
    }

    #[test]
    fn clear_device_wallet_model_removes_only_the_matching_managed_generation() {
        let (p, marker, secret) = wallet_clear_fixture("success");
        clear_device_wallet_model(&p, &marker, "model-wallet-clear.json", &secret).unwrap();

        let config: Value = serde_json::from_slice(&fs::read(config_file(&p)).unwrap()).unwrap();
        let provider_key = marker["provider_key"].as_str().unwrap();
        let reference = model_ref(provider_key, marker["model"].as_str().unwrap());
        assert!(config.pointer(&format!("/models/providers/{provider_key}")).is_none());
        assert!(config.pointer(&format!("/secrets/providers/{MODEL_SECRET_PROVIDER}")).is_none());
        assert!(config.pointer("/agents/defaults/model/primary").is_none());
        assert!(config.pointer(&format!("/agents/defaults/models/{reference}")).is_none());
        assert_eq!(config["models"]["providers"]["user-provider"]["must_survive"], true);
        assert_eq!(config["secrets"]["providers"]["user-secret"]["must_survive"], true);
        assert_eq!(config["agents"]["defaults"]["models"]["other/model"]["must_survive"], true);
        assert_eq!(config["user"]["must_survive"], true);
        assert!(!model_marker_file(&p).exists(), "只移除受管 marker");
        assert!(!secret.exists(), "只移除受管 file secret");
        let profile: Value = serde_json::from_slice(&fs::read(profile_file(&p)).unwrap()).unwrap();
        assert_eq!(profile["config_hash"], json!(crate::installer::sha256_hex_bytes(&fs::read(config_file(&p)).unwrap())));
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn clear_device_wallet_model_fault_restores_config_marker_secret_and_profile() {
        let (p, marker, secret) = wallet_clear_fixture("rollback");
        let before = [
            (config_file(&p), snapshot_file(&config_file(&p))),
            (model_marker_file(&p), snapshot_file(&model_marker_file(&p))),
            (secret.clone(), snapshot_file(&secret)),
            (profile_file(&p), snapshot_file(&profile_file(&p))),
        ];
        set_model_test_fault(&p, Some("wallet_clear_profile"));
        let error = clear_device_wallet_model(&p, &marker, "model-wallet-clear.json", &secret).unwrap_err();
        set_model_test_fault(&p, None);
        assert!(error.starts_with("validation_failed:"), "{error}");
        for (path, snapshot) in before {
            assert_eq!(snapshot_file(&path).bytes, snapshot.bytes, "{path:?} bytes must roll back");
            assert_eq!(snapshot_file(&path).modified, snapshot.modified, "{path:?} mtime must roll back");
        }
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn stale_device_wallet_generation_refuses_key_refresh_without_touching_any_file() {
        let (p, _marker, secret) = wallet_clear_fixture("stale-some");
        let mut changed: Value = serde_json::from_slice(&fs::read(config_file(&p)).unwrap()).unwrap();
        changed["user"]["edited_after_wallet_setup"] = json!(true);
        atomic_write(
            &config_file(&p),
            &serde_json::to_vec_pretty(&changed).unwrap(),
            &p,
        ).unwrap();
        let before = [
            (config_file(&p), snapshot_file(&config_file(&p))),
            (model_marker_file(&p), snapshot_file(&model_marker_file(&p))),
            (secret.clone(), snapshot_file(&secret)),
            (profile_file(&p), snapshot_file(&profile_file(&p))),
        ];
        let error = sync_device_wallet_key_at(&p, Some("sk-new-test-wallet-key")).unwrap_err();
        assert!(error.contains("不一致"), "{error}");
        for (path, snapshot) in before {
            assert_eq!(snapshot_file(&path).bytes, snapshot.bytes, "{path:?} bytes must not change");
            assert_eq!(snapshot_file(&path).modified, snapshot.modified, "{path:?} mtime must not change");
        }
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn byok_marker_skips_wallet_key_refresh_without_touching_any_file() {
        let (p, mut marker, secret) = wallet_clear_fixture("byok-some");
        marker["key_source"] = json!("explicit");
        atomic_write(
            &model_marker_file(&p),
            &serde_json::to_vec_pretty(&marker).unwrap(),
            &p,
        ).unwrap();
        let before = [
            (config_file(&p), snapshot_file(&config_file(&p))),
            (model_marker_file(&p), snapshot_file(&model_marker_file(&p))),
            (secret.clone(), snapshot_file(&secret)),
            (profile_file(&p), snapshot_file(&profile_file(&p))),
        ];
        sync_device_wallet_key_at(&p, Some("sk-new-test-wallet-key")).unwrap();
        for (path, snapshot) in before {
            assert_eq!(snapshot_file(&path).bytes, snapshot.bytes, "{path:?} bytes must not change");
            assert_eq!(snapshot_file(&path).modified, snapshot.modified, "{path:?} mtime must not change");
        }
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn dashboard_opener_returns_no_token_bearing_url() {
        let target = "http://127.0.0.1:37603/#token=never-return-this";
        let mut opened = String::new();
        let result = open_verified_dashboard_target(target, |url| {
            opened = url.to_owned();
            Ok(())
        })
        .unwrap();
        assert_eq!(opened, target);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("never-return-this"));
    }
    #[cfg(windows)]
    fn private_node_for_gateway_test(p: &Paths) -> Option<PathBuf> {
        let out = Command::new("where.exe").arg("node.exe").output().ok()?;
        let source = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)?;
        let destination = node_exe(p);
        fs::create_dir_all(destination.parent()?).ok()?;
        fs::copy(source, &destination).ok()?;
        Some(destination)
    }
    #[cfg(windows)]
    #[test]
    fn run_capture_drains_large_pipes_and_times_out_without_waiting_for_eof() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-capture-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        let node = private_node_for_gateway_test(&p).expect("测试机需要 Node");
        let flood = p.root.join("flood.mjs");
        fs::write(
            &flood,
            "process.stdout.write('o'.repeat(128 * 1024)); process.stderr.write('e'.repeat(128 * 1024));",
        )
        .unwrap();
        let output = run_capture(
            &node,
            &[&flood.to_string_lossy()],
            &[],
            &p.workspace,
            Duration::from_secs(5),
        )
        .expect("大输出子进程必须可回收");
        assert_eq!(output.status, Some(0));
        assert_eq!(output.stdout.len(), 128 * 1024);
        assert_eq!(output.stderr.len(), 128 * 1024);

        let began = Instant::now();
        let timed_out = run_capture(
            &node,
            &["-e", "setInterval(() => {}, 1000)"],
            &[],
            &p.workspace,
            Duration::from_millis(300),
        )
        .expect("超时子进程必须可回收");
        assert!(timed_out.status.is_none());
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "超时路径不能等待继承管道的 EOF"
        );
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    #[test]
    fn portable_child_environment_excludes_host_model_and_node_injection() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-env-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        let node = private_node_for_gateway_test(&p).expect("测试机需要 Node");
        let script = p.root.join("env-probe.mjs");
        fs::write(
            &script,
            r#"const names=['DASHSCOPE_API_KEY','OPENAI_API_KEY','ANTHROPIC_API_KEY','NODE_OPTIONS','NODE_PATH','OPENCLAW_HOME','OPENCLAW_CONFIG_PATH','OPENCLAW_PROFILE','SystemRoot','WINDIR','COMSPEC','PATH','PATHEXT'];
console.log(JSON.stringify(Object.fromEntries(names.map((name) => [name, process.env[name] ?? null]))));"#,
        )
        .unwrap();
        let inherited = [
            ("DASHSCOPE_API_KEY", "host-dashscope-key"),
            ("OPENAI_API_KEY", "host-openai-key"),
            ("ANTHROPIC_API_KEY", "host-anthropic-key"),
            ("NODE_OPTIONS", "--require host-injection"),
            ("NODE_PATH", "C:\\host-node-path"),
            ("OPENCLAW_HOME", "C:\\host-openclaw-home"),
        ];
        let previous: Vec<(&str, Option<std::ffi::OsString>)> = inherited
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in inherited {
            std::env::set_var(key, value);
        }
        let owned = vec![
            ("OPENCLAW_PROFILE".into(), PROFILE.into()),
            ("OPENCLAW_HOME".into(), p.root.to_string_lossy().to_string()),
            (
                "OPENCLAW_CONFIG_PATH".into(),
                config_file(&p).to_string_lossy().to_string(),
            ),
        ];
        let output = {
            let mut command = Command::new(node);
            command.arg(script).stdout(Stdio::piped()).stderr(Stdio::piped());
            configure_managed_openclaw_child(&mut command, &owned, true);
            command.output().expect("受管环境子进程必须可运行")
        };
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let observed: Value = serde_json::from_slice(&output.stdout).unwrap();
        for key in [
            "DASHSCOPE_API_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "NODE_OPTIONS",
            "NODE_PATH",
        ] {
            assert!(observed[key].is_null(), "{key} 不能进入便携 OpenClaw 子进程");
        }
        assert_eq!(observed["OPENCLAW_HOME"], p.root.to_string_lossy().as_ref());
        assert_eq!(
            observed["OPENCLAW_CONFIG_PATH"],
            config_file(&p).to_string_lossy().as_ref()
        );
        assert_eq!(observed["OPENCLAW_PROFILE"], PROFILE);
        assert!(observed["PATH"].as_str().is_some_and(|value| !value.is_empty()));
        assert!(
            observed["SystemRoot"].as_str().is_some_and(|value| !value.is_empty())
                || observed["WINDIR"].as_str().is_some_and(|value| !value.is_empty())
        );
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    fn unused_gateway_port_base() -> u16 {
        (32_000u16..64_000u16)
            .step_by(131)
            .find(|base| reserve_port_family(*base).is_ok())
            .expect("应能找到完整可用的 OpenClaw2 端口族")
    }
    #[cfg(windows)]
    #[test]
    fn configure_model_without_probe_never_checks_or_runs_infer() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-no-probe-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        private_node_for_gateway_test(&p).expect("测试机需要 Node");
        fs::create_dir_all(cli_file(&p).parent().unwrap()).unwrap();
        fs::write(
            cli_file(&p),
            r#"const args = process.argv.slice(2);
if (args.includes('infer')) { console.error('infer must not run'); process.exit(41); }
if (args.includes('config') && args.includes('validate') && args.includes('--json')) {
  console.log(JSON.stringify({ok:true})); process.exit(0);
}
process.exit(2);
"#,
        )
        .unwrap();
        fs::write(
            config_file(&p),
            serde_json::to_vec(&json!({"gateway":{"auth":{"token":"gateway-sentinel"}}}))
                .unwrap(),
        )
        .unwrap();
        let result = configure_model_at_with_probe(
            &p,
            ModelRoute {
                source_id: "demo".into(),
                source_name: "Demo".into(),
                base: "https://example.com/v1".into(),
                model: "demo-chat".into(),
                key: "no-probe-secret".into(),
                key_source: "explicit".into(),
            },
            false,
            false,
        )
        .expect("no-probe 配置不能调用 infer 能力检查");
        assert_eq!(result["probe"]["ran"], false);
        assert_eq!(result["validation"]["ok"], true);
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    #[test]
    fn configure_model_runs_private_candidate_then_keeps_secret_out_of_live_outputs() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-model-txn-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        private_node_for_gateway_test(&p).expect("测试机需要 Node");
        fs::create_dir_all(cli_file(&p).parent().unwrap()).unwrap();
        fs::write(cli_file(&p), r#"import fs from 'node:fs';
const args = process.argv.slice(2); const fault = fs.existsSync('model-fault.txt') ? fs.readFileSync('model-fault.txt', 'utf8').trim() : '';
if (fault === 'validate' && args.includes('validate')) { console.error('validate failed'); process.exit(1); }
if (fault === 'infer' && args.includes('infer') && !args.includes('--help')) { console.error('infer failed'); process.exit(1); }
if (args.includes('validate') || args.includes('--help')) { console.log(JSON.stringify({ok:true})); process.exit(0); }
if (args.includes('infer')) { console.log(JSON.stringify({reply:'openclaw2-probe-ok'})); process.exit(0); }
process.exit(2);
"#).unwrap();
        fs::write(config_file(&p), serde_json::to_vec(&json!({"gateway":{"auth":{"token":"gateway-sentinel"}},"workspace":{"sentinel":true}})).unwrap()).unwrap();
        let route = || ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo".into(),
            base: "https://example.com/v1".into(),
            model: "demo-chat".into(),
            key: "model-secret-never-output".into(),
            key_source: "explicit".into(),
        };
        let first = configure_model_at(&p, route(), false).unwrap();
        let live = fs::read_to_string(config_file(&p)).unwrap();
        let marker = fs::read_to_string(model_marker_file(&p)).unwrap();
        let output = serde_json::to_string(&first).unwrap();
        assert_eq!(first["changed"], true);
        assert!(live.contains("gateway-sentinel"));
        assert!(!live.contains("model-secret-never-output"));
        assert!(!marker.contains("model-secret-never-output"));
        assert!(!output.contains("model-secret-never-output"));
        let secret_name = serde_json::from_str::<Value>(&marker).unwrap()["secret_basename"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(model_secrets_dir(&p).join(&secret_name).is_file());
        let second = configure_model_at(&p, route(), false).unwrap();
        assert_eq!(
            second["changed"], false,
            "same private route 不许二次 probe/write"
        );
        let old_key = model_provider_key("demo", "https://example.com/v1");
        let route_b = ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo B".into(),
            base: "https://other.example/v1".into(),
            model: "demo-next".into(),
            key: "other-secret-never-output".into(),
            key_source: "explicit".into(),
        };
        let new_key = model_provider_key(&route_b.source_id, &route_b.base);
        let switched = configure_model_at(&p, route_b, false).unwrap();
        let switched_config: Value =
            serde_json::from_slice(&fs::read(config_file(&p)).unwrap()).unwrap();
        assert_eq!(switched["changed"], true);
        assert!(
            switched_config["models"]["providers"]
                .get(&old_key)
                .is_none(),
            "旧自有 provider 必须随 A→B 清理"
        );
        assert!(switched_config["agents"]["defaults"]["models"]
            .get(&format!("{old_key}/demo-chat"))
            .is_none());
        assert_eq!(
            switched_config["agents"]["defaults"]["model"]["primary"],
            format!("{new_key}/demo-next")
        );
        assert!(
            model_secrets_dir(&p).join(secret_name).exists() == false,
            "旧自有 secret 必须在新 marker 提交后清理"
        );
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    #[test]
    fn configure_model_rolls_back_each_transaction_stage_without_touching_prior_generation() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-model-rollback-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        private_node_for_gateway_test(&p).expect("测试机需要 Node");
        fs::create_dir_all(cli_file(&p).parent().unwrap()).unwrap();
        fs::write(cli_file(&p), r#"import fs from 'node:fs';
const args = process.argv.slice(2); const fault = fs.existsSync('model-fault.txt') ? fs.readFileSync('model-fault.txt', 'utf8').trim() : '';
if (fault === 'validate' && args.includes('validate')) process.exit(1);
if (fault === 'infer' && args.includes('infer') && !args.includes('--help')) process.exit(1);
console.log(JSON.stringify(args.includes('infer') ? {reply:'openclaw2-probe-ok'} : {ok:true}));
"#).unwrap();
        fs::write(config_file(&p), serde_json::to_vec(&json!({"gateway":{"auth":{"token":"legacy-gateway-sentinel"}},"workspace":{"legacy":true}})).unwrap()).unwrap();
        let a = ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo".into(),
            base: "https://example.com/v1".into(),
            model: "demo-chat".into(),
            key: "old-key-not-output".into(),
            key_source: "explicit".into(),
        };
        configure_model_at(&p, a, false).unwrap();
        let marker: Value =
            serde_json::from_slice(&fs::read(model_marker_file(&p)).unwrap()).unwrap();
        let old_secret = model_secrets_dir(&p).join(marker["secret_basename"].as_str().unwrap());
        let before_config = snapshot_file(&config_file(&p));
        let before_marker = snapshot_file(&model_marker_file(&p));
        let before_secret = snapshot_file(&old_secret);
        let b = || ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo B".into(),
            base: "https://other.example/v1".into(),
            model: "demo-next".into(),
            key: "new-key-not-output".into(),
            key_source: "explicit".into(),
        };
        for (fault, expected) in [
            ("validate", "validation_failed:"),
            ("infer", "probe_failed:"),
            ("live_commit", "validation_failed:"),
            ("marker_commit", "validation_failed:"),
        ] {
            let file_fault = p.workspace.join("model-fault.txt");
            if matches!(fault, "validate" | "infer") {
                fs::write(&file_fault, fault).unwrap();
            } else {
                let _ = fs::remove_file(&file_fault);
            }
            if matches!(fault, "live_commit" | "marker_commit") {
                set_model_test_fault(&p, Some(fault));
            }
            let error = configure_model_at(&p, b(), false).unwrap_err();
            set_model_test_fault(&p, None);
            let _ = fs::remove_file(&file_fault);
            assert!(error.starts_with(expected), "{fault}: {error}");
            for (path, before) in [
                (&config_file(&p), &before_config),
                (&model_marker_file(&p), &before_marker),
                (&old_secret, &before_secret),
            ] {
                assert_eq!(
                    snapshot_file(path).bytes,
                    before.bytes,
                    "{fault}: {path:?} bytes"
                );
                assert_eq!(
                    snapshot_file(path).modified,
                    before.modified,
                    "{fault}: {path:?} mtime"
                );
            }
        }
        let version_before = state_version_for(&p);
        fs::write(&old_secret, b"{\"api_key\":\"external-change\"}").unwrap();
        assert_ne!(
            version_before,
            state_version_for(&p),
            "secret 内容是 optimistic state 的组成部分"
        );
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    #[test]
    fn configure_model_does_not_touch_legacy_or_clawx_sentinels() {
        let sb = crate::testsandbox::enter_raw("openclaw2-model-legacy-sentinels");
        std::env::set_var("USERPROFILE", sb.root());
        std::env::remove_var("HOME");
        let sentinels = [
            sb.root().join(".openclaw/old-config.json"),
            crate::installer::uking_home().join("openclaw/old-config.json"),
            sb.root().join("AppData/Roaming/ClawX/old-config.json"),
        ];
        for path in &sentinels {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"legacy-openclaw-sentinel").unwrap();
        }
        let before = sentinels
            .iter()
            .map(|path| snapshot_file(path))
            .collect::<Vec<_>>();
        let p = paths_from_root(sb.root().join("openclaw2-adapter"));
        create_layout(&p).unwrap();
        private_node_for_gateway_test(&p).expect("测试机需要 Node");
        fs::create_dir_all(cli_file(&p).parent().unwrap()).unwrap();
        fs::write(cli_file(&p), r#"const args = process.argv.slice(2); console.log(JSON.stringify(args.includes('infer') ? {reply:'openclaw2-probe-ok'} : {ok:true}));"#).unwrap();
        fs::write(
            config_file(&p),
            b"{\"gateway\":{\"auth\":{\"token\":\"private-only\"}}}",
        )
        .unwrap();
        let route = ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo".into(),
            base: "https://example.com/v1".into(),
            model: "demo-chat".into(),
            key: "never-in-sentinel".into(),
            key_source: "explicit".into(),
        };
        configure_model_at(&p, route, false).unwrap();
        for (path, snapshot) in sentinels.iter().zip(before) {
            assert_eq!(
                snapshot_file(path).bytes,
                snapshot.bytes,
                "legacy sentinel bytes changed: {path:?}"
            );
            assert_eq!(
                snapshot_file(path).modified,
                snapshot.modified,
                "legacy sentinel mtime changed: {path:?}"
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn private_gateway_starts_after_port_reservations_are_released() {
        let p = paths_from_root(std::env::temp_dir().join(format!(
            "uking-openclaw2-real-gateway-{}-{}",
            std::process::id(),
            now_nanos()
        )));
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
        fs::write(
            profile_file(&p),
            serde_json::to_string(&json!({"schema_version":1,"profile":PROFILE,"port":port}))
                .unwrap(),
        )
        .unwrap();
        fs::write(
            config_file(&p),
            serde_json::to_string(&json!({"gateway":{"auth":{"token":"fake-status-token"}}}))
                .unwrap(),
        )
        .unwrap();
        let launched = launch_private_gateway(&p, port).unwrap();
        let pid = launched["pid"].as_u64().unwrap() as u32;
        assert_eq!(launched["running"], true);
        assert_eq!(launched["ready"], true);
        assert_eq!(launched["health"]["nested"]["token"], "[redacted]");
        assert_eq!(
            supervisor_status(&p).unwrap(),
            (Some(port), Some(pid), true)
        );
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .output();
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_identity(pid).is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = fs::remove_dir_all(&p.root);
    }
    #[cfg(windows)]
    #[test]
    fn supervisor_marker_failure_terminates_the_child() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-marker-{}", std::process::id())),
        );
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
        assert!(
            child.try_wait().unwrap().is_some(),
            "marker 失败后 child 必须被杀死"
        );
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
            command_line: format!(
                "\"{}\" \"{}\" --profile {PROFILE} gateway run --port 19789",
                node.display(),
                cli.display()
            ),
            started: "first-process".into(),
        };
        let marker = json!({"state_dir":state,"process_started":"first-process"});
        assert!(identity_matches(&p, 19789, &marker, &identity));
        let reused = ProcessIdentity {
            started: "reused-pid".into(),
            ..identity
        };
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
        let before = sentinels
            .iter()
            .map(|file| {
                (
                    crate::installer::sha256_hex_bytes(&fs::read(file).unwrap()),
                    fs::metadata(file).unwrap().modified().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        prepare(None).unwrap();
        for (file, (hash, mtime)) in sentinels.iter().zip(before) {
            assert_eq!(
                crate::installer::sha256_hex_bytes(&fs::read(file).unwrap()),
                hash
            );
            assert_eq!(fs::metadata(file).unwrap().modified().unwrap(), mtime);
        }
    }
    #[test]
    fn model_candidate_preserves_private_unknowns_and_only_uses_file_secret_ref() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-model-candidate-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        fs::write(config_file(&p), serde_json::to_vec(&json!({"gateway":{"auth":{"token":"gateway-private"}},"workspace":{"keep":true},"unknown":{"keep":"yes"},"models":{"providers":{"someone-else":{"keep":true}}}})).unwrap()).unwrap();
        let route = ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo".into(),
            base: "https://example.com/v1".into(),
            model: "demo-chat".into(),
            key: "never-in-config".into(),
            key_source: "explicit".into(),
        };
        let key = model_provider_key(&route.source_id, &route.base);
        let candidate =
            model_candidate_config(&p, &route, &key, &model_secret_file(&p, "next")).unwrap();
        let text = String::from_utf8(candidate).unwrap();
        assert!(!text.contains("never-in-config"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["gateway"]["auth"]["token"], "gateway-private");
        assert_eq!(value["workspace"]["keep"], true);
        assert_eq!(value["unknown"]["keep"], "yes");
        assert_eq!(value["models"]["mode"], "merge");
        assert_eq!(value["models"]["providers"]["someone-else"]["keep"], true);
        assert_eq!(
            value["models"]["providers"][key.as_str()]["apiKey"]["source"],
            "file"
        );
        assert_eq!(
            value["models"]["providers"][key.as_str()]["apiKey"]["id"],
            "/api_key",
            "2026.8.1 json SecretRef 必须使用绝对 JSON Pointer"
        );
        assert_eq!(
            value["agents"]["defaults"]["model"]["primary"],
            model_ref(&key, "demo-chat")
        );
        assert!(
            value["models"].get("primary").is_none(),
            "根 models.primary 不是权威槽位"
        );
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn fixed_2026_8_1_schema_candidate_keeps_file_secret_ref_and_model_slots_calibrated() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../resources/openclaw2-2026.8.1-schema-candidate.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["secrets"]["providers"][MODEL_SECRET_PROVIDER]["source"],
            "file"
        );
        assert_eq!(
            fixture["secrets"]["providers"][MODEL_SECRET_PROVIDER]["mode"],
            "json"
        );
        assert_eq!(
            fixture["models"]["providers"]["uking-oc2-fixture"]["apiKey"]["source"],
            "file"
        );
        assert_eq!(
            fixture["models"]["providers"]["uking-oc2-fixture"]["apiKey"]["id"],
            "/fixture_token"
        );
        assert_eq!(
            fixture["agents"]["defaults"]["model"]["primary"],
            "uking-oc2-fixture/fixture-model"
        );
    }
    #[test]
    fn validation_diagnostic_is_actionable_without_echoing_candidate_secrets() {
        let out = Capture { status: Some(1), stdout: String::new(), stderr: "invalid config: File secret reference id must be an absolute JSON pointer; value=model-secret-never-output".into() };
        let message = config_diagnostic("candidate config validation", &out);
        assert!(message.contains("diagnostic=secret_ref_pointer"));
        assert!(!message.contains("model-secret-never-output"));
    }
    #[test]
    fn model_candidate_refuses_unmarked_or_third_party_slots() {
        let p = paths_from_root(
            std::env::temp_dir().join(format!("uking-openclaw2-model-collision-{}", now_nanos())),
        );
        create_layout(&p).unwrap();
        let route = ModelRoute {
            source_id: "demo".into(),
            source_name: "Demo".into(),
            base: "https://example.com/v1".into(),
            model: "demo-chat".into(),
            key: "never-in-config".into(),
            key_source: "explicit".into(),
        };
        let key = model_provider_key(&route.source_id, &route.base);
        fs::write(config_file(&p), serde_json::to_vec(&json!({"models":{"providers":{key.clone():{"third_party":true}}},"secrets":{"providers":{MODEL_SECRET_PROVIDER:{"third_party":true}}}})).unwrap()).unwrap();
        let error =
            model_candidate_config(&p, &route, &key, &model_secret_file(&p, "next")).unwrap_err();
        assert!(error.starts_with("validation_failed:"));
        let _ = fs::remove_dir_all(&p.root);
    }
    #[test]
    fn model_endpoint_rejects_remote_http_and_credential_url() {
        assert!(normalized_model_base("https://api.example.com/v1").is_ok());
        assert!(normalized_model_base("http://127.0.0.1:11434/v1").is_ok());
        for invalid in [
            "http://example.com/v1",
            "https://u:p@example.com/v1",
            "https://example.com/v1?q=x",
            "https://example.com/v1#x",
        ] {
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
        let confirmation = crate::actions::run(
            crate::actions::OPENCLAW2_CONFIGURE_MODEL,
            json!({"provider_id":"xiapan"}),
        )
        .unwrap_err();
        assert!(confirmation.contains("confirmation_required:"));
        let model_unknown = crate::actions::run(
            crate::actions::OPENCLAW2_CONFIGURE_MODEL,
            json!({"confirm":true,"provider_id":"xiapan","unknown":true}),
        )
        .unwrap_err();
        assert!(model_unknown.contains("未知字段"));
    }
}
