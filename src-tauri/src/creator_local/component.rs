//! Transactional, local-only management for optional creator components.
//!
//! The component download uses only the executable's built-in trusted
//! catalogue. This module never accepts a caller-provided URL or installation
//! destination, and only promotes a verified OpenTu archive below U-King's
//! own component root.

use super::{atomic_write, fresh_id, sha256_file, uking_home, verify_static_bundle};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const SCHEMA: u32 = 1;
const COMPONENT: &str = "opentu";
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_SINGLE_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_FILE_COUNT: u64 = 4_096;
const MAX_PATH_DEPTH: usize = 32;
const MAX_RELATIVE_PATH_BYTES: usize = 240;

/// Metadata must come from U-King's built-in trusted release catalogue.  It is
/// not parsed from the archive and every field is deliberately non-optional.
///
/// This type is an internal boundary, not a GUI/CLI input schema: callers must
/// authenticate the catalogue before constructing it.  Do not pass user input
/// directly to [`install_opentu_archive`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComponentManifest {
    pub schema: u32,
    pub component: String,
    pub bundle_id: String,
    pub archive_bytes: u64,
    pub archive_sha256: String,
    pub integrity_manifest_sha256: String,
    pub unpacked_bytes: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentInstallState {
    NotInstalled,
    Installed,
    Damaged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentInspection {
    pub component: String,
    pub state: ComponentInstallState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentPointer {
    schema: u32,
    manifest: ComponentManifest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedCatalogue {
    schema: u32,
    components: Vec<TrustedCatalogueEntry>,
}

/// This is compiled into the executable.  It is intentionally the only place
/// a download URL enters the component installer; neither GUI nor CLI input
/// can choose a host, hash, or archive path.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedCatalogueEntry {
    schema: u32,
    component: String,
    upstream_version: String,
    upstream_commit: String,
    bundle_id: String,
    bridge_schema: u32,
    archive_format: String,
    url: String,
    archive_bytes: u64,
    archive_sha256: String,
    integrity_manifest_sha256: String,
    unpacked_bytes: u64,
    file_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentOffer {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_bytes: Option<u64>,
}

fn trusted_catalogue() -> Result<TrustedCatalogue, String> {
    let catalogue: TrustedCatalogue = serde_json::from_str(include_str!("../../resources/creator-components/catalog.json"))
        .map_err(|_| "component_error: 内置 OpenTu 组件目录格式无效".to_string())?;
    if catalogue.schema != SCHEMA {
        return Err("component_error: 内置 OpenTu 组件目录版本无效".into());
    }
    Ok(catalogue)
}

fn trusted_offer() -> Result<Option<(ComponentManifest, String)>, String> {
    let catalogue = trusted_catalogue()?;
    let mut matches = catalogue.components.into_iter().filter(|entry| entry.component == COMPONENT);
    let Some(entry) = matches.next() else { return Ok(None); };
    if matches.next().is_some()
        || entry.schema != SCHEMA
        || entry.bridge_schema != SCHEMA
        || entry.archive_format != "tar.gz"
        || entry.upstream_version.trim().is_empty()
        || entry.upstream_commit.len() != 40
        || !entry.upstream_commit.bytes().all(|b| b.is_ascii_hexdigit())
        || !entry.url.starts_with("https://")
        || entry.url.contains([' ', '\t', '\r', '\n', '@', '#'])
    {
        return Err("component_error: 内置 OpenTu 组件目录不可信".into());
    }
    let manifest = ComponentManifest {
        schema: entry.schema,
        component: entry.component,
        bundle_id: entry.bundle_id,
        archive_bytes: entry.archive_bytes,
        archive_sha256: entry.archive_sha256,
        integrity_manifest_sha256: entry.integrity_manifest_sha256,
        unpacked_bytes: entry.unpacked_bytes,
        file_count: entry.file_count,
    };
    manifest.validate()?;
    Ok(Some((manifest, entry.url)))
}

pub fn inspect_opentu_offer() -> Result<ComponentOffer, String> {
    Ok(match trusted_offer()? {
        Some((manifest, _)) => ComponentOffer { available: true, bundle_id: Some(manifest.bundle_id), archive_bytes: Some(manifest.archive_bytes) },
        None => ComponentOffer { available: false, bundle_id: None, archive_bytes: None },
    })
}

fn component_state_root() -> PathBuf {
    uking_home()
}

fn component_root(state_root: &Path) -> PathBuf {
    // This is intentionally distinct from `projects/`: uninstalling an
    // optional UI surface must never be able to remove customer projects.
    state_root.join("components").join(COMPONENT)
}

fn versions_root(root: &Path) -> PathBuf {
    root.join("versions")
}

fn pointer_path(root: &Path) -> PathBuf {
    root.join("current.json")
}

fn valid_hex(value: &str) -> bool {
    value.len() == 64
        && value != "0".repeat(64)
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_bundle_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && !value.starts_with('.')
        && value != "0"
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

impl ComponentManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.component != COMPONENT
            || !valid_bundle_id(&self.bundle_id)
            || self.archive_bytes == 0
            || self.archive_bytes > MAX_ARCHIVE_BYTES
            || !valid_hex(&self.archive_sha256)
            || !valid_hex(&self.integrity_manifest_sha256)
            || self.unpacked_bytes == 0
            || self.unpacked_bytes > MAX_UNPACKED_BYTES
            || self.file_count == 0
            || self.file_count > MAX_FILE_COUNT
        {
            return Err("invalid_manifest: OpenTu 组件清单不可信或包含占位值".into());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn is_link_or_reparse(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

fn checked_real_dir(path: &Path, create: bool, label: &str) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        if !create {
            return Ok(None);
        }
        fs::create_dir(path).map_err(|e| format!("component_error: 无法创建{label}: {e}"))?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("component_error: 无法读取{label}: {e}"))?;
    if !meta.file_type().is_dir() || is_link_or_reparse(&meta) {
        return Err(format!("forbidden: {label}不能是链接、重解析点或普通文件"));
    }
    path.canonicalize().map(Some).map_err(|e| format!("component_error: 无法解析{label}: {e}"))
}

/// Resolve the one component root below a real canonical `.uking` directory.
/// Every mutable caller reruns this after it takes the OS lock, so a junction
/// swap cannot redirect an install or an uninstall toward projects or an
/// outside sentinel directory.
fn checked_component_root(state_root: &Path, create: bool) -> Result<Option<PathBuf>, String> {
    if !state_root.exists() && create {
        fs::create_dir_all(state_root).map_err(|e| format!("component_error: 无法创建 U-King 状态根: {e}"))?;
    }
    let Some(state) = checked_real_dir(state_root, false, "U-King 状态根")? else {
        return Ok(None);
    };
    let components_path = state.join("components");
    let Some(components) = checked_real_dir(&components_path, create, "组件根目录")? else {
        return Ok(None);
    };
    if !components.starts_with(&state) {
        return Err("forbidden: OpenTu 组件根越出 U-King 状态根".into());
    }
    let root_path = components.join(COMPONENT);
    let Some(root) = checked_real_dir(&root_path, create, "OpenTu 组件目录")? else {
        return Ok(None);
    };
    if !root.starts_with(&components) || !root.starts_with(&state) {
        return Err("forbidden: OpenTu 组件目录越界".into());
    }
    let projects = state.join("projects");
    if projects.exists() {
        let project_meta = fs::symlink_metadata(&projects).map_err(|e| format!("component_error: 无法读取项目目录: {e}"))?;
        if !is_link_or_reparse(&project_meta) && project_meta.is_dir() {
            let projects = projects.canonicalize().map_err(|e| format!("component_error: 无法解析项目目录: {e}"))?;
            if root.starts_with(projects) {
                return Err("forbidden: OpenTu 组件目录不能落在客户项目目录中".into());
            }
        }
    }
    Ok(Some(root))
}

fn checked_versions_root(root: &Path, create: bool) -> Result<Option<PathBuf>, String> {
    let path = versions_root(root);
    let Some(versions) = checked_real_dir(&path, create, "OpenTu 组件版本目录")? else {
        return Ok(None);
    };
    if !versions.starts_with(root) {
        return Err("forbidden: OpenTu 组件版本目录越界".into());
    }
    Ok(Some(versions))
}

/// A real OS advisory lock protects the short rename/pointer transaction across
/// processes.  A leftover `.install.lock` file after a crash is harmless: the
/// kernel lock is released with the process, and the next process can reuse it.
struct ComponentLock {
    file: fs::File,
}

impl ComponentLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let lock_path = root
            .parent()
            .ok_or("component_error: OpenTu 组件根没有父目录")?
            .join(".opentu-component.lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(lock_path)
            .map_err(|e| format!("component_error: 无法打开 OpenTu 安装锁: {e}"))?;
        lock_file(&file)?;
        Ok(Self { file })
    }
}

impl Drop for ComponentLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
    }
}

#[cfg(windows)]
fn lock_file(file: &fs::File) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Overlapped { internal: usize, internal_high: usize, offset: u32, offset_high: u32, h_event: *mut std::ffi::c_void }
    #[link(name = "kernel32")]
    extern "system" { fn LockFileEx(handle: *mut std::ffi::c_void, flags: u32, reserved: u32, low: u32, high: u32, overlapped: *mut Overlapped) -> i32; }
    let mut overlapped = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, h_event: std::ptr::null_mut() };
    // Exclusive, non-blocking.  Returning a conflict is safer than a UI action
    // waiting forever behind a hung updater.
    if unsafe { LockFileEx(file.as_raw_handle(), 0x0000_0003, 0, 1, 0, &mut overlapped) } == 0 {
        return Err("target_conflict: 另一个进程正在安装或卸载 OpenTu".into());
    }
    Ok(())
}

#[cfg(windows)]
fn unlock_file(file: &fs::File) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Overlapped { internal: usize, internal_high: usize, offset: u32, offset_high: u32, h_event: *mut std::ffi::c_void }
    #[link(name = "kernel32")]
    extern "system" { fn UnlockFileEx(handle: *mut std::ffi::c_void, reserved: u32, low: u32, high: u32, overlapped: *mut Overlapped) -> i32; }
    let mut overlapped = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, h_event: std::ptr::null_mut() };
    if unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) } == 0 {
        return Err("component_error: 无法释放 OpenTu 安装锁".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn lock_file(file: &fs::File) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" { fn flock(fd: i32, operation: i32) -> i32; }
    if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
        return Err("target_conflict: 另一个进程正在安装或卸载 OpenTu".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn unlock_file(file: &fs::File) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" { fn flock(fd: i32, operation: i32) -> i32; }
    if unsafe { flock(file.as_raw_fd(), 8) } != 0 {
        return Err("component_error: 无法释放 OpenTu 安装锁".into());
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String), String> {
    let mut file = fs::File::open(path).map_err(|_| "component_error: 无法读取已下载的 OpenTu 包".to_string())?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| "component_error: 无法校验已下载的 OpenTu 包".to_string())?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count as u64).ok_or("component_error: 安装包大小溢出")?;
        if total > MAX_ARCHIVE_BYTES {
            return Err("component_error: OpenTu 安装包超过安全大小限制".into());
        }
        hasher.update(&buffer[..count]);
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn archive_path_is_safe(path: &Path) -> Result<(String, PathBuf), String> {
    let raw = path.to_str().ok_or("archive_error: 压缩包路径必须是 UTF-8")?;
    if raw.is_empty()
        || raw.len() > MAX_RELATIVE_PATH_BYTES
        || raw.contains('\\')
        || raw.contains(':')
        || raw.ends_with(['.', ' '])
    {
        return Err("archive_error: 压缩包包含非法 Windows 路径".into());
    }
    let mut clean = PathBuf::new();
    let mut parts = Vec::new();
    for part in path.components() {
        let Component::Normal(part) = part else {
            return Err("archive_error: 压缩包包含绝对路径或路径穿越".into());
        };
        let text = part.to_str().ok_or("archive_error: 压缩包路径必须是 UTF-8")?;
        if text.is_empty() || text.ends_with(['.', ' ']) || reserved_name(text) {
            return Err("archive_error: 压缩包包含保留 Windows 文件名".into());
        }
        clean.push(part);
        parts.push(text);
        if parts.len() > MAX_PATH_DEPTH {
            return Err("archive_error: 压缩包路径层级过深".into());
        }
    }
    if parts.is_empty() {
        return Err("archive_error: 压缩包路径为空".into());
    }
    let normalized = parts.join("/").to_ascii_lowercase();
    Ok((normalized, clean))
}

fn reserved_name(part: &str) -> bool {
    let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

fn make_parent(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let destination = root.join(relative);
    let parent = destination.parent().ok_or("archive_error: 压缩包目标路径无父目录")?;
    fs::create_dir_all(parent).map_err(|e| format!("archive_error: 无法创建解压目录: {e}"))?;
    Ok(destination)
}

fn copy_limited(entry: &mut tar::Entry<'_, GzDecoder<fs::File>>, target: &Path, expected: u64) -> Result<(), String> {
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| format!("archive_error: 无法写入解压文件: {e}"))?;
    let mut remaining = expected;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let take = remaining.min(buffer.len() as u64) as usize;
        let count = entry.read(&mut buffer[..take]).map_err(|e| format!("archive_error: 读取压缩包失败: {e}"))?;
        if count == 0 {
            return Err("archive_error: 压缩包文件内容截断".into());
        }
        use std::io::Write;
        output.write_all(&buffer[..count]).map_err(|e| format!("archive_error: 写入解压文件失败: {e}"))?;
        remaining -= count as u64;
    }
    let mut extra = [0u8; 1];
    if entry.read(&mut extra).map_err(|e| format!("archive_error: 读取压缩包失败: {e}"))? != 0 {
        return Err("archive_error: 压缩包文件长度不一致".into());
    }
    Ok(())
}

fn unpack_archive(archive_path: &Path, staging: &Path, manifest: &ComponentManifest) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|_| "component_error: 无法打开已下载的 OpenTu 包".to_string())?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    let mut entry_count = 0u64;
    let mut unpacked_bytes = 0u64;

    for item in archive.entries().map_err(|_| "archive_error: OpenTu 安装包不是有效 tar.gz")? {
        let mut entry = item.map_err(|_| "archive_error: OpenTu 安装包条目损坏")?;
        let (normalized, relative) = archive_path_is_safe(&entry.path().map_err(|_| "archive_error: 无法读取压缩包路径")?)?;
        let entry_type = entry.header().entry_type();
        let is_dir = entry_type.is_dir();
        let is_file = entry_type.is_file();
        if !is_dir && !is_file {
            return Err("archive_error: OpenTu 安装包不允许链接或特殊文件".into());
        }
        if seen.insert(normalized.clone(), is_file).is_some() {
            return Err("archive_error: OpenTu 安装包存在重复或大小写冲突路径".into());
        }
        entry_count = entry_count.checked_add(1).ok_or("archive_error: 条目数量溢出")?;
        if entry_count > MAX_FILE_COUNT {
            return Err("archive_error: OpenTu 安装包超过安全解压条目限制".into());
        }
        let mut ancestor = String::new();
        for part in normalized.split('/').take(normalized.split('/').count().saturating_sub(1)) {
            if !ancestor.is_empty() { ancestor.push('/'); }
            ancestor.push_str(part);
            if seen.get(&ancestor) == Some(&true) {
                return Err("archive_error: 文件不能同时作为目录".into());
            }
        }
        if is_dir {
            if entry.header().size().map_err(|_| "archive_error: 无法读取压缩包目录大小")? != 0 {
                return Err("archive_error: OpenTu 安装包目录条目大小必须为零".into());
            }
            if seen.iter().any(|(path, file)| *file && path.starts_with(&(normalized.clone() + "/"))) {
                return Err("archive_error: 文件不能同时作为目录".into());
            }
            fs::create_dir_all(staging.join(relative)).map_err(|e| format!("archive_error: 无法创建解压目录: {e}"))?;
            continue;
        }

        let size = entry.header().size().map_err(|_| "archive_error: 无法读取压缩包文件大小")?;
        if size > MAX_SINGLE_FILE_BYTES {
            return Err("archive_error: OpenTu 安装包包含超限文件".into());
        }
        unpacked_bytes = unpacked_bytes.checked_add(size).ok_or("archive_error: 解压大小溢出")?;
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            return Err("archive_error: OpenTu 安装包超过安全解压限制".into());
        }
        let target = make_parent(staging, &relative)?;
        copy_limited(&mut entry, &target, size)?;
    }
    if entry_count != manifest.file_count || unpacked_bytes != manifest.unpacked_bytes {
        return Err("archive_error: OpenTu 安装包内容与可信清单不一致".into());
    }
    Ok(())
}

fn verify_integrity_manifest(staging: &Path, manifest: &ComponentManifest) -> Result<(), String> {
    let integrity = staging.join(".uking-integrity.json");
    if sha256_file(&integrity).map_err(|_| "integrity_error: 缺少 OpenTu 完整性清单")? != manifest.integrity_manifest_sha256 {
        return Err("integrity_error: OpenTu 完整性清单哈希不匹配".into());
    }
    verify_static_bundle(staging)?;

    // `verify_static_bundle` proves each declared file's hash.  Also require
    // it to declare *every* unpacked payload so a verified archive cannot hide
    // executable or future-networking resources beside the canvas files.
    let raw = fs::read(&integrity).map_err(|_| "integrity_error: 无法读取 OpenTu 完整性清单")?;
    let value: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| "integrity_error: OpenTu 完整性清单格式无效")?;
    let files = value.get("files").and_then(|v| v.as_array()).ok_or("integrity_error: OpenTu 完整性清单格式无效")?;
    if files.len() as u64 > MAX_FILE_COUNT {
        return Err("integrity_error: OpenTu 完整性清单条目过多".into());
    }
    for entry in files {
        let path = entry.get("path").and_then(|v| v.as_str()).ok_or("integrity_error: OpenTu 完整性清单路径无效")?;
        archive_path_is_safe(Path::new(path)).map_err(|_| "integrity_error: OpenTu 完整性清单包含非法资源路径")?;
    }
    let declared = files.iter().filter_map(|entry| entry.get("path").and_then(|v| v.as_str())).collect::<BTreeSet<_>>();
    if declared.len() != files.len() {
        return Err("integrity_error: OpenTu 完整性清单包含重复路径".into());
    }
    let mut actual = BTreeSet::new();
    let mut entries = 0u64;
    collect_files(staging, staging, 0, &mut entries, &mut actual)?;
    actual.remove(".uking-integrity.json");
    if actual.len() != declared.len() || !actual.iter().all(|path| declared.contains(path.as_str())) {
        return Err("integrity_error: OpenTu 完整性清单未覆盖全部安装文件".into());
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    current: &Path,
    depth: usize,
    entries: &mut u64,
    output: &mut BTreeSet<String>,
) -> Result<(), String> {
    if depth > MAX_PATH_DEPTH {
        return Err("integrity_error: OpenTu 安装目录层级过深".into());
    }
    for entry in fs::read_dir(current).map_err(|_| "integrity_error: 无法读取 OpenTu 安装目录")? {
        let entry = entry.map_err(|_| "integrity_error: 无法读取 OpenTu 安装目录")?;
        *entries = entries.checked_add(1).ok_or("integrity_error: OpenTu 安装目录条目溢出")?;
        if *entries > MAX_FILE_COUNT {
            return Err("integrity_error: OpenTu 安装目录条目过多".into());
        }
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| "integrity_error: 无法读取 OpenTu 安装文件")?;
        if is_link_or_reparse(&meta) {
            return Err("integrity_error: OpenTu 安装目录不允许链接".into());
        }
        if meta.is_dir() {
            collect_files(root, &entry.path(), depth + 1, entries, output)?;
        } else if meta.is_file() {
            let entry_path = entry.path();
            let relative = entry_path.strip_prefix(root).map_err(|_| "integrity_error: OpenTu 文件越界")?;
            let text = relative.to_str().ok_or("integrity_error: OpenTu 文件路径无效")?.replace('\\', "/");
            archive_path_is_safe(Path::new(&text)).map_err(|_| "integrity_error: OpenTu 文件路径无效")?;
            output.insert(text);
        } else {
            return Err("integrity_error: OpenTu 安装目录包含特殊文件".into());
        }
    }
    Ok(())
}

fn read_current(root: &Path) -> Result<Option<CurrentPointer>, String> {
    let path = pointer_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let pointer: CurrentPointer = serde_json::from_slice(&fs::read(&path).map_err(|_| "component_error: 无法读取当前 OpenTu 指针")?)
        .map_err(|_| "component_error: 当前 OpenTu 指针损坏")?;
    if pointer.schema != SCHEMA {
        return Err("component_error: 当前 OpenTu 指针版本无效".into());
    }
    pointer.manifest.validate()?;
    Ok(Some(pointer))
}

fn install_to_state_root(state_root: &Path, archive_path: &Path, manifest: &ComponentManifest) -> Result<ComponentInspection, String> {
    manifest.validate()?;
    let root = checked_component_root(state_root, true)?.ok_or("component_error: 无法创建 OpenTu 组件目录")?;
    let _lock = ComponentLock::acquire(&root)?;
    let root = checked_component_root(state_root, true)?.ok_or("component_error: OpenTu 组件目录在加锁后消失")?;
    let versions = checked_versions_root(&root, true)?.ok_or("component_error: 无法创建 OpenTu 组件版本目录")?;
    let (archive_bytes, archive_sha256) = hash_file(archive_path)?;
    if archive_bytes != manifest.archive_bytes || archive_sha256 != manifest.archive_sha256 {
        return Err("integrity_error: OpenTu 安装包哈希或大小与可信清单不一致".into());
    }
    let version = versions.join(&manifest.bundle_id);
    if version.exists() {
        let meta = fs::symlink_metadata(&version)
            .map_err(|e| format!("component_error: 无法读取 OpenTu 版本目录: {e}"))?;
        if !meta.file_type().is_dir() || is_link_or_reparse(&meta) {
            return Err("forbidden: OpenTu 版本目录不能是链接或普通文件".into());
        }
        // A prior crash can happen after the version rename but before
        // `current.json` replacement.  Revalidate the already-promoted bytes,
        // then finish that one safe pointer transaction.  Reinstalling an
        // identical verified bundle is therefore idempotent.  This is not a
        // repair API: a damaged pre-existing version is never deleted here.
        verify_integrity_manifest(&version, manifest)?;
        let pointer = CurrentPointer { schema: SCHEMA, manifest: manifest.clone() };
        atomic_write(&pointer_path(&root), &serde_json::to_vec(&pointer).map_err(|e| e.to_string())?)?;
        return Ok(ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::Installed, bundle_id: Some(manifest.bundle_id.clone()), detail: None });
    }
    let staging = root.join(format!(".staging-{}", fresh_id()));
    fs::create_dir(&staging).map_err(|e| format!("component_error: 无法创建同盘安装暂存目录: {e}"))?;
    let result = (|| {
        unpack_archive(archive_path, &staging, manifest)?;
        verify_integrity_manifest(&staging, manifest)?;
        fs::rename(&staging, &version).map_err(|e| format!("component_error: 无法原子提交 OpenTu 版本: {e}"))?;
        let pointer = CurrentPointer { schema: SCHEMA, manifest: manifest.clone() };
        atomic_write(&pointer_path(&root), &serde_json::to_vec(&pointer).map_err(|e| e.to_string())?)?;
        Ok(ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::Installed, bundle_id: Some(manifest.bundle_id.clone()), detail: None })
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Promote one already-downloaded OpenTu archive using an already-authenticated
/// built-in catalogue manifest.  GUI/CLI values must never be passed here.
///
/// This API installs or resumes a verified identical bundle; it deliberately
/// does not repair a damaged existing `bundle_id` by deleting it.
pub fn install_opentu_archive(archive_path: &Path, manifest: &ComponentManifest) -> Result<ComponentInspection, String> {
    install_to_state_root(&component_state_root(), archive_path, manifest)
}

/// Download only the URL and metadata compiled into `catalog.json`, then hand
/// the temp file to the same hash-checked transactional installer.  The caller
/// cannot provide a URL, manifest, or destination.
pub fn install_catalogued_opentu() -> Result<ComponentInspection, String> {
    let Some((manifest, url)) = trusted_offer()? else {
        return Err("not_installed: OpenTu 本地画布组件尚未发布；请等待 U-King 更新组件目录".into());
    };
    let state = component_state_root();
    let root = checked_component_root(&state, true)?.ok_or("component_error: 无法创建 OpenTu 组件目录")?;
    let temporary = root.join(format!(".download-{}.tar.gz", fresh_id()));
    let downloaded = download_catalogued_archive(&url, &temporary, manifest.archive_bytes);
    let installed = downloaded.and_then(|()| install_to_state_root(&state, &temporary, &manifest));
    let _ = fs::remove_file(&temporary);
    installed
}

/// Download a catalogue-controlled archive without going through a shell.  The
/// installer validates its exact byte length and SHA-256 afterwards; download
/// transports only decide how the bytes reach this private temporary path.
fn download_catalogued_archive(url: &str, temporary: &Path, archive_bytes: u64) -> Result<(), String> {
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let mut download = Command::new(curl);
    download.args([
        "--fail", "--silent", "--show-error", "--location",
        "--proto", "=https", "--proto-redir", "=https",
        "--connect-timeout", "15", "--max-time", "300", "--max-filesize",
    ])
    .arg(archive_bytes.to_string())
    .args(["--output"])
    .arg(temporary);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Schannel revocation lookups often fail on customer networks even
        // when HTTPS itself is reachable.  The archive's pinned SHA-256 is
        // still verified before any file is promoted.
        download.arg("--ssl-no-revoke");
        download.creation_flags(0x0800_0000);
    }
    download.arg(url);
    let curl_error = match download.status() {
        Ok(status) if status.success() => return Ok(()),
        Ok(status) => format!("curl exit {}", status.code().unwrap_or(-1)),
        Err(error) => format!("无法启动 curl: {error}"),
    };

    #[cfg(windows)]
    {
        // Avoid accepting a partial curl output as a completed fallback.
        let _ = fs::remove_file(temporary);
        match powershell_download_to_file(url, temporary) {
            Ok(()) => return Ok(()),
            Err(powershell_error) => {
                let _ = fs::remove_file(temporary);
                return Err(format!(
                    "network_error: OpenTu 组件下载失败（{curl_error}; WinINET 回退：{powershell_error}），请检查网络后重试"
                ));
            }
        }
    }

    #[cfg(not(windows))]
    Err(format!("network_error: OpenTu 组件下载失败（{curl_error}），请检查网络后重试"))
}

#[cfg(windows)]
fn powershell_download_to_file(url: &str, destination: &Path) -> Result<(), String> {
    let command = powershell_download_command(url, destination)?;
    let mut process = Command::new("powershell.exe");
    process.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &command]);
    use std::os::windows::process::CommandExt;
    process.creation_flags(0x0800_0000);
    match process.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("PowerShell exit {}", status.code().unwrap_or(-1))),
        Err(error) => Err(format!("无法启动 PowerShell: {error}")),
    }
}

/// Build an encoded PowerShell command, so an URL or private temporary path is
/// never interpolated into PowerShell source.  Redirects are disabled here;
/// curl handles catalogue HTTPS-to-HTTPS redirects, while this WinINET retry
/// can only download the original pinned HTTPS URL.
#[cfg(windows)]
fn powershell_download_command(url: &str, destination: &Path) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("拒绝非 HTTPS 下载地址".into());
    }
    let destination = destination.to_str().ok_or("下载临时路径不是有效 Unicode")?;
    let url64 = base64_encode(url.as_bytes());
    let destination64 = base64_encode(destination.as_bytes());
    let source = format!(
        "$ErrorActionPreference='Stop';$url=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{url64}'));$dest=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{destination64}'));if(-not $url.StartsWith('https://',[StringComparison]::OrdinalIgnoreCase)){{exit 2}};try{{Invoke-WebRequest -Uri $url -OutFile $dest -TimeoutSec 300 -MaximumRedirection 0 -UseBasicParsing;exit 0}}catch{{exit 1}}"
    );
    let utf16: Vec<u8> = source.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Ok(base64_encode(&utf16))
}

#[cfg(windows)]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    for chunk in bytes.chunks(3) {
        let value = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        result.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        result.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        result.push(if chunk.len() > 1 { TABLE[((value >> 6) & 0x3f) as usize] as char } else { '=' });
        result.push(if chunk.len() > 2 { TABLE[(value & 0x3f) as usize] as char } else { '=' });
    }
    result
}

/// Resolve exactly the `current.json` version.  The loopback server never
/// falls back to an app resource, CWD, or environment override.
pub fn active_opentu_static_root() -> Result<PathBuf, String> {
    let state = component_state_root();
    let root = checked_component_root(&state, false)?.ok_or("not_installed: OpenTu 本地画布组件尚未安装")?;
    let pointer = read_current(&root)?.ok_or("not_installed: OpenTu 本地画布组件尚未安装")?;
    let versions = checked_versions_root(&root, false)?.ok_or("not_installed: OpenTu 组件版本目录不存在")?;
    let version = versions.join(&pointer.manifest.bundle_id);
    verify_integrity_manifest(&version, &pointer.manifest)?;
    Ok(version)
}

fn inspection_error(error: String) -> ComponentInspection {
    ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::Damaged, bundle_id: None, detail: Some(error) }
}

fn inspect_at_state_root(state_root: &Path) -> ComponentInspection {
    let root = match checked_component_root(state_root, false) {
        Ok(None) => return ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::NotInstalled, bundle_id: None, detail: None },
        Err(error) => return inspection_error(error),
        Ok(Some(root)) => root,
    };
    let pointer = match read_current(&root) {
        Ok(None) => return ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::NotInstalled, bundle_id: None, detail: None },
        Err(error) => return ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::Damaged, bundle_id: None, detail: Some(error) },
        Ok(Some(pointer)) => pointer,
    };
    let versions = match checked_versions_root(&root, false) {
        Ok(Some(versions)) => versions,
        Ok(None) => return inspection_error("component_error: OpenTu 组件版本目录不存在".into()),
        Err(error) => return inspection_error(error),
    };
    let version = versions.join(&pointer.manifest.bundle_id);
    let result = verify_integrity_manifest(&version, &pointer.manifest);
    match result {
        Ok(()) => ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::Installed, bundle_id: Some(pointer.manifest.bundle_id), detail: None },
        Err(error) => ComponentInspection { component: COMPONENT.into(), state: ComponentInstallState::Damaged, bundle_id: Some(pointer.manifest.bundle_id), detail: Some(error) },
    }
}

pub fn inspect_opentu() -> ComponentInspection {
    inspect_at_state_root(&component_state_root())
}

fn uninstall_at_state_root(state_root: &Path, server_active: bool) -> Result<(), String> {
    if server_active {
        return Err("target_conflict: 本地 OpenTu 服务仍在运行，停止后才能卸载".into());
    }
    let Some(root) = checked_component_root(state_root, false)? else { return Ok(()); };
    let _lock = ComponentLock::acquire(&root)?;
    let Some(root) = checked_component_root(state_root, false)? else { return Ok(()); };
    fs::remove_dir_all(root).map_err(|e| format!("component_error: 无法卸载 OpenTu 组件: {e}"))
}

/// The caller owns the live loopback server and must prove it is stopped.
pub fn uninstall_opentu(server_active: bool) -> Result<(), String> {
    uninstall_at_state_root(&component_state_root(), server_active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::{Builder, Header};

    fn sha(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    fn append(builder: &mut Builder<GzEncoder<fs::File>>, path: &str, bytes: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, bytes).unwrap();
    }

    fn append_dir(builder: &mut Builder<GzEncoder<fs::File>>, path: &str) {
        let mut header = Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, path, &[][..]).unwrap();
    }

    fn good_archive(root: &Path, name: &str) -> (PathBuf, ComponentManifest) {
        let index = b"<html>local</html>";
        let integrity = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "files": [{"path": "index.html", "sha256": sha(index)}]
        })).unwrap();
        let archive = root.join(name);
        let file = fs::File::create(&archive).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);
        append(&mut builder, "index.html", index);
        append(&mut builder, ".uking-integrity.json", &integrity);
        builder.into_inner().unwrap().finish().unwrap();
        let bytes = fs::read(&archive).unwrap();
        (archive, ComponentManifest {
            schema: SCHEMA,
            component: COMPONENT.into(),
            bundle_id: "1.1.6".into(),
            archive_bytes: bytes.len() as u64,
            archive_sha256: sha(&bytes),
            integrity_manifest_sha256: sha(&integrity),
            unpacked_bytes: (index.len() + integrity.len()) as u64,
            file_count: 2,
        })
    }

    #[test]
    fn rejects_placeholder_manifest_fields() {
        crate::testsandbox::with_sandbox("opentu-component-placeholder", &[], |root| {
            let (_, mut manifest) = good_archive(root, "placeholder.tar.gz");
            manifest.archive_bytes = 0;
            assert!(manifest.validate().unwrap_err().starts_with("invalid_manifest:"));
            manifest.archive_bytes = 1;
            manifest.archive_sha256 = "0".repeat(64);
            assert!(manifest.validate().unwrap_err().starts_with("invalid_manifest:"));
        });
    }

    #[test]
    fn embedded_catalogue_offers_only_the_built_in_component() {
        crate::testsandbox::with_sandbox("opentu-component-catalogue", &[], |_| {
            // `trusted_offer` takes no caller-controlled input. It must read
            // only the `include_str!` catalogue compiled into this binary,
            // never an Action, GUI, CLI, or replaceable resource path.
            let offer = inspect_opentu_offer().unwrap();
            assert!(offer.available);
            assert_eq!(offer.bundle_id.as_deref(), Some("opentu-1.1.6-uking.7"));
            assert_eq!(offer.archive_bytes, Some(18_559_802));
            let (manifest, _) = trusted_offer().unwrap().unwrap();
            assert_eq!(manifest.bundle_id, "opentu-1.1.6-uking.7");
            assert_eq!(manifest.archive_bytes, 18_559_802);
            assert!(active_opentu_static_root()
                .unwrap_err()
                .starts_with("not_installed:"));

            // Keep strict trusted-catalogue parsing covered without making a
            // test delivery URL part of the compiled-in release catalogue.
            let fixture: TrustedCatalogue = serde_json::from_str(r#"{
                "schema": 1,
                "components": [{
                    "schema": 1,
                    "component": "opentu",
                    "upstream_version": "1.1.6",
                    "upstream_commit": "48802871554c5b8221b4c5d70baff0b68d00df46",
                    "bundle_id": "opentu-1.1.6-uking.1",
                    "bridge_schema": 1,
                    "archive_format": "tar.gz",
                    "archive_bytes": 18452610,
                    "archive_sha256": "0e2842fab019eaa4c49e77e819b1079d8ba8fc7b0b0571108e3f8f08ddb3fcdf",
                    "integrity_manifest_sha256": "d18999bb11e001eef3baba39060aa2942272496d15adda2cdd777bb5de96bff5",
                    "unpacked_bytes": 36838778,
                    "file_count": 248,
                    "url": "https://example.com/opentu-1.1.6-uking.1.tar.gz"
                }]
            }"#).unwrap();
            assert_eq!(fixture.schema, SCHEMA);
            let entry = fixture.components.into_iter().next().unwrap();
            assert_eq!(entry.component, COMPONENT);
            assert!(entry.url.starts_with("https://example.com/"));
            assert!(!entry.url.contains([' ', '\t', '\r', '\n', '@', '#']));
            ComponentManifest {
                schema: entry.schema,
                component: entry.component,
                bundle_id: entry.bundle_id,
                archive_bytes: entry.archive_bytes,
                archive_sha256: entry.archive_sha256,
                integrity_manifest_sha256: entry.integrity_manifest_sha256,
                unpacked_bytes: entry.unpacked_bytes,
                file_count: entry.file_count,
            }.validate().unwrap();

            // The catalogue is a closed schema.  A GUI/CLI caller cannot
            // smuggle an alternate download URL or hash through an extra
            // field; `trusted_offer` has no input parameter either.
            assert!(serde_json::from_str::<TrustedCatalogue>(r#"{
                "schema": 1,
                "components": [],
                "url": "https://attacker.invalid/opentu.tar.gz"
            }"#).is_err());
        });
    }

    #[cfg(windows)]
    #[test]
    fn powershell_fallback_uses_encoded_values_and_disables_redirects() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        let url = "https://example.com/a';&.tar.gz";
        let destination = Path::new(r"C:\Temp\a';&.tar.gz");
        let encoded = powershell_download_command(url, destination).unwrap();
        // The outer command line contains encoded UTF-16 source only, so
        // apostrophes and command separators from either value cannot escape
        // into PowerShell syntax.
        assert!(!encoded.contains(url));
        assert!(!encoded.contains(&destination.display().to_string()));
        let script = decode_powershell_command(&encoded);
        assert!(script.contains("FromBase64String"));
        assert!(script.contains("-MaximumRedirection 0"));
        assert!(script.contains("-TimeoutSec 300"));
        assert!(powershell_download_command("http://example.com/a", destination).is_err());
    }

    #[cfg(windows)]
    fn decode_powershell_command(encoded: &str) -> String {
        fn digit(byte: u8) -> u8 {
            match byte {
                b'A'..=b'Z' => byte - b'A', b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52, b'+' => 62, b'/' => 63,
                _ => panic!("unexpected base64 byte"),
            }
        }
        let mut bytes = Vec::new();
        for part in encoded.as_bytes().chunks(4) {
            let value = ((digit(part[0]) as u32) << 18) | ((digit(part[1]) as u32) << 12)
                | (if part[2] == b'=' { 0 } else { (digit(part[2]) as u32) << 6 })
                | if part[3] == b'=' { 0 } else { digit(part[3]) as u32 };
            bytes.push((value >> 16) as u8);
            if part[2] != b'=' { bytes.push((value >> 8) as u8); }
            if part[3] != b'=' { bytes.push(value as u8); }
        }
        String::from_utf16(&bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn installs_and_failure_keeps_old_current() {
        crate::testsandbox::with_sandbox("opentu-component-transaction", &[], |root| {
            let state = root.join(".uking");
            let components = component_root(&state);
            let (archive, manifest) = good_archive(root, "good.tar.gz");
            assert_eq!(install_to_state_root(&state, &archive, &manifest).unwrap().state, ComponentInstallState::Installed);
            let current = fs::read(pointer_path(&components)).unwrap();
            // A repeated delivery (including one after an updater restart) is
            // a verified no-op except for safely refreshing current.json.
            assert_eq!(install_to_state_root(&state, &archive, &manifest).unwrap().state, ComponentInstallState::Installed);
            assert_eq!(fs::read(pointer_path(&components)).unwrap(), current);
            let (_, mut broken) = good_archive(root, "broken.tar.gz");
            broken.bundle_id = "1.1.7".into();
            broken.archive_sha256 = "a".repeat(64);
            assert!(install_to_state_root(&state, &archive, &broken).unwrap_err().starts_with("integrity_error:"));
            assert_eq!(fs::read(pointer_path(&components)).unwrap(), current);
            assert_eq!(inspect_at_state_root(&state).state, ComponentInstallState::Installed);
        });
    }

    #[test]
    fn pointer_commit_failure_is_recoverable_from_promoted_version() {
        crate::testsandbox::with_sandbox("opentu-component-pointer-recovery", &[], |root| {
            let state = root.join(".uking");
            let components = component_root(&state);
            let (archive, mut manifest) = good_archive(root, "recover.tar.gz");
            manifest.bundle_id = "1.1.7".into();
            // A directory at the pointer filename makes the final atomic
            // replacement fail after the staged version has been renamed.
            fs::create_dir_all(pointer_path(&components)).unwrap();
            assert!(install_to_state_root(&state, &archive, &manifest).is_err());
            let version = versions_root(&components).join("1.1.7");
            assert!(version.join("index.html").is_file());
            assert!(matches!(inspect_at_state_root(&state).state, ComponentInstallState::Damaged));
            fs::remove_dir(pointer_path(&components)).unwrap();
            // The existing verified version finishes the interrupted pointer
            // transaction; it is not redownloaded or overwritten.
            assert_eq!(install_to_state_root(&state, &archive, &manifest).unwrap().state, ComponentInstallState::Installed);
            assert_eq!(inspect_at_state_root(&state).state, ComponentInstallState::Installed);
        });
    }

    fn hostile_archive(root: &Path, name: &str, paths: &[&str], link: bool) -> PathBuf {
        let archive = root.join(name);
        let encoder = GzEncoder::new(fs::File::create(&archive).unwrap(), Compression::default());
        let mut builder = Builder::new(encoder);
        if link {
            let mut header = Header::new_gnu();
            header.set_entry_type(tar::EntryType::new(b'2'));
            header.set_size(0);
            header.set_cksum();
            builder.append_link(&mut header, "linked", "index.html").unwrap();
        } else {
            for path in paths {
                if *path == "../escape" {
                    // tar's convenience path setter correctly refuses `..`,
                    // so create the hostile header at byte level.  The reader
                    // must still reject a malicious archive received from a
                    // third party.
                    let mut header = Header::new_gnu();
                    header.set_path("safe").unwrap();
                    header.set_size(1);
                    header.set_mode(0o644);
                    let raw = header.as_mut_bytes();
                    raw[..100].fill(0);
                    raw[..9].copy_from_slice(b"../escape");
                    header.set_cksum();
                    builder.append(&header, &b"x"[..]).unwrap();
                } else {
                    append(&mut builder, path, b"x");
                }
            }
        }
        builder.into_inner().unwrap().finish().unwrap();
        archive
    }

    fn permissive_manifest(archive: &Path) -> ComponentManifest {
        let bytes = fs::read(archive).unwrap();
        ComponentManifest { schema: SCHEMA, component: COMPONENT.into(), bundle_id: "hostile".into(), archive_bytes: bytes.len() as u64, archive_sha256: sha(&bytes), integrity_manifest_sha256: "a".repeat(64), unpacked_bytes: 1, file_count: 1 }
    }

    #[test]
    fn rejects_traversal_links_and_duplicate_paths() {
        crate::testsandbox::with_sandbox("opentu-component-hostile", &[], |root| {
            for (name, paths, link) in [
                ("traversal.tar.gz", vec!["../escape"], false),
                ("link.tar.gz", vec![], true),
                ("duplicate.tar.gz", vec!["index.html", "INDEX.html"], false),
            ] {
                let archive = hostile_archive(root, name, &paths, link);
                let manifest = permissive_manifest(&archive);
                let state = root.join(format!(".uking-{name}"));
                let error = install_to_state_root(&state, &archive, &manifest).unwrap_err();
                assert!(error.starts_with("archive_error:"), "{name}: {error}");
            }
        });
    }

    #[test]
    fn directory_budget_and_depth_rejection_preserve_current() {
        crate::testsandbox::with_sandbox("opentu-component-tar-budget", &[], |root| {
            let state = root.join(".uking");
            let component = component_root(&state);
            let (good, good_manifest) = good_archive(root, "old.tar.gz");
            install_to_state_root(&state, &good, &good_manifest).unwrap();
            let old_current = fs::read(pointer_path(&component)).unwrap();

            let many = root.join("many-dirs.tar.gz");
            let mut builder = Builder::new(GzEncoder::new(fs::File::create(&many).unwrap(), Compression::default()));
            for number in 0..=MAX_FILE_COUNT {
                append_dir(&mut builder, &format!("d-{number}"));
            }
            builder.into_inner().unwrap().finish().unwrap();
            let many_bytes = fs::read(&many).unwrap();
            let many_manifest = ComponentManifest {
                schema: SCHEMA, component: COMPONENT.into(), bundle_id: "1.1.7".into(),
                archive_bytes: many_bytes.len() as u64, archive_sha256: sha(&many_bytes),
                integrity_manifest_sha256: "a".repeat(64), unpacked_bytes: 1, file_count: MAX_FILE_COUNT,
            };
            assert!(install_to_state_root(&state, &many, &many_manifest).unwrap_err().starts_with("archive_error:"));
            assert_eq!(fs::read(pointer_path(&component)).unwrap(), old_current);

            let deep = hostile_archive(root, "deep.tar.gz", &[&format!("{}index.html", "a/".repeat(MAX_PATH_DEPTH))], false);
            let mut deep_manifest = permissive_manifest(&deep);
            deep_manifest.bundle_id = "1.1.8".into();
            assert!(install_to_state_root(&state, &deep, &deep_manifest).unwrap_err().starts_with("archive_error:"));
            assert_eq!(fs::read(pointer_path(&component)).unwrap(), old_current);
        });
    }

    #[cfg(windows)]
    #[test]
    fn component_junction_never_touches_outside_sentinel_or_projects() {
        crate::testsandbox::with_sandbox("opentu-component-junction", &[], |root| {
            let state = root.join(".uking");
            let outside = root.join("outside-sentinel");
            let projects = state.join("projects").join("canvas-customer");
            fs::create_dir_all(&outside).unwrap();
            fs::write(outside.join("sentinel.txt"), "must survive").unwrap();
            fs::create_dir_all(&projects).unwrap();
            fs::write(projects.join("canvas.json"), "customer data").unwrap();
            fs::create_dir_all(&state).unwrap();
            let link = state.join("components");
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .status()
                .expect("mklink should start");
            assert!(status.success(), "test junction should be creatable");
            let (archive, manifest) = good_archive(root, "junction.tar.gz");
            assert!(install_to_state_root(&state, &archive, &manifest).unwrap_err().starts_with("forbidden:"));
            assert!(uninstall_at_state_root(&state, false).unwrap_err().starts_with("forbidden:"));
            assert_eq!(fs::read_to_string(outside.join("sentinel.txt")).unwrap(), "must survive");
            assert_eq!(fs::read_to_string(projects.join("canvas.json")).unwrap(), "customer data");
            let _ = fs::remove_dir(&link);
        });
    }

    #[test]
    fn uninstall_requires_stopped_server_and_preserves_projects() {
        crate::testsandbox::with_sandbox("opentu-component-uninstall", &[], |root| {
            let state = root.join(".uking");
            let components = component_root(&state);
            let projects = state.join("projects").join("canvas-customer");
            fs::create_dir_all(&projects).unwrap();
            fs::write(projects.join("canvas.json"), "customer data").unwrap();
            fs::create_dir_all(&components).unwrap();
            fs::write(components.join("current.json"), "{}") .unwrap();
            assert!(uninstall_at_state_root(&state, true).unwrap_err().starts_with("target_conflict:"));
            assert!(components.exists());
            uninstall_at_state_root(&state, false).unwrap();
            assert!(!components.exists());
            assert_eq!(fs::read_to_string(projects.join("canvas.json")).unwrap(), "customer data");
        });
    }
}
